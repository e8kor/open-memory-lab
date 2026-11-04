-- 0014_user_roots — user roots, user keys, node grants and the two kinds of
-- workarea namespace (ADR-0044 d1, d2, d4, d5, d12). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): 0001–0013 are never edited,
-- a correction is the next numbered file.
-- Normative shapes: docs/data/postgres-schema.md "workareas and edges" and
-- "api_keys, grants, key_denials, bans".
--
-- Nothing is rewritten and nothing is journalled (ADR-0044 d12): every row
-- this migration finds folds as its journal events say, since none of them
-- carries the new fields — every workarea shared and no user root, every
-- key a tenant key, every grant covering its node's subtree. A NOT NULL
-- column with a constant DEFAULT is added without rewriting the table
-- (PostgreSQL 17 documentation, ALTER TABLE, "Notes").
--
-- Run by the owner role, with oml.serving_role set as 0007 requires; the
-- table-level privileges 0007 gave the serving role cover the new columns.

------------------------------------------------------------------------------
-- workareas: a user root is a direct child of universe, marked at creation
-- and never moved (d1). namespace_root_id is the user root whose namespace
-- holds the row's name: NULL for a shared row, universe and every user root
-- included, the root a row lies strictly beneath otherwise (d5). The key
-- names the tenant with the id, as every reference onto workareas does,
-- because a foreign-key check runs without row-level security (0004).
------------------------------------------------------------------------------
ALTER TABLE workareas
    ADD COLUMN user_root         boolean NOT NULL DEFAULT false,
    ADD COLUMN namespace_root_id uuid,
    ADD CONSTRAINT workareas_namespace_root
        FOREIGN KEY (tenant_id, namespace_root_id) REFERENCES workareas (tenant_id, id),
    -- A user root sits under universe and holds a shared name.
    ADD CONSTRAINT workareas_user_root_shape
        CHECK (NOT user_root OR (parent_id IS NOT NULL AND namespace_root_id IS NULL));
COMMENT ON COLUMN workareas.user_root IS
    'universe.<user>, created by the first key naming its user; fixed at creation (ADR-0044 d1).';
COMMENT ON COLUMN workareas.namespace_root_id IS
    'The user root whose namespace holds this name; NULL in the shared namespace (ADR-0044 d5).';

------------------------------------------------------------------------------
-- Names are unique in the shared namespace or within one user root's: the
-- one tenant-wide constraint of 0006 becomes two (ADR-0044 d5). The store
-- answers a refusal of either as name.taken (errors.md). The unique
-- constraint on (tenant_id, namespace_root_id, name) admits any number of
-- shared rows, whose namespace_root_id is NULL; the partial index holds
-- those.
------------------------------------------------------------------------------
ALTER TABLE workareas DROP CONSTRAINT workareas_tenant_id_name;
CREATE UNIQUE INDEX workareas_shared_name ON workareas (tenant_id, name)
    WHERE namespace_root_id IS NULL;
ALTER TABLE workareas
    ADD CONSTRAINT workareas_user_name UNIQUE (tenant_id, namespace_root_id, name);

------------------------------------------------------------------------------
-- api_keys: the user root of the person a key acts for, fixed for its life;
-- NULL for a tenant key, every key issued before this migration included
-- (ADR-0044 d2, d12).
------------------------------------------------------------------------------
ALTER TABLE api_keys
    ADD COLUMN user_root_id uuid,
    ADD CONSTRAINT api_keys_user_root
        FOREIGN KEY (tenant_id, user_root_id) REFERENCES workareas (tenant_id, id);
COMMENT ON COLUMN api_keys.user_root_id IS
    'The user root of a user key; NULL for a tenant key (ADR-0044 d2).';

------------------------------------------------------------------------------
-- grants: a grant covers its node's subtree or the node alone, and a node
-- grant carries no deny entry (ADR-0044 d4).
------------------------------------------------------------------------------
ALTER TABLE grants
    ADD COLUMN covers text NOT NULL DEFAULT 'subtree'
        CHECK (covers IN ('subtree', 'node')),
    ADD CONSTRAINT grants_node_has_no_deny
        CHECK (covers = 'subtree' OR deny = '[]'::jsonb);
