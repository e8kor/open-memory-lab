-- The v0.8.0 rename (ADR-0091 d6): workarea becomes workspace, the role
-- dreamer becomes reviewer (d13), the statuses committed and validated become
-- protected and frozen (d14), and a key's held_by becomes used_by, its value
-- human becoming person (d15).
--
-- One transaction. No applied migration is edited: 0001-0046 keep their
-- text and their sqlx checksums, and this file is the one place after them
-- that spells the former names, because it names what it renames (d11).
--
--   relations    workareas, workarea_transitions, search_workarea_rows
--   columns      every column whose name holds the token, and held_by,
--                approver_held_by
--   names        every index, constraint and sequence whose name holds the
--                token, enumerated from pg_catalog when this runs, so the
--                partition-local names a tenant's partitions carry are
--                renamed too
--   functions    every function whose stored body names a renamed relation
--                or column, re-created from its own definition with the
--                token replaced: a PL/pgSQL or SQL body is stored as text and
--                does not follow a rename, while policies, indexes and
--                constraints are stored parsed and do
--   values       api_keys.role and grants.role dreamer -> reviewer;
--                api_keys.used_by and memories.approver_used_by human ->
--                person; api_keys.guardrail_scope workarea -> workspace;
--                the statuses of workspaces and their transitions; the act
--                names of live idempotency rows (workarea.create and the
--                rest), under each tenant's row security
--   checks       the role, holder, scope, status and one-rung checks of
--                0001, 0004, 0016, 0017 and 0028, replaced with the new
--                values
--   comments     every comment that said workarea or the former statuses
--
-- Nothing signed or sealed is rewritten: the journal keeps its rows as
-- signed, and every reader reads them through the former-name map (d4).
--
-- What it costs a live database: catalog renames, plus one UPDATE of
-- api_keys, grants, workspaces, workspace_transitions, the promoted rows of
-- memories that carry an approver, and idempotency, and the replaced checks
-- validated in one scan each of those small tables. The upgrade runs it with
-- the server stopped (d12), so the ACCESS EXCLUSIVE locks it takes are free;
-- a lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

------------------------------------------------------------------------------
-- The checks that name a former value go first, so the values can change.
------------------------------------------------------------------------------
ALTER TABLE api_keys
    DROP CONSTRAINT api_keys_role_check,
    DROP CONSTRAINT api_keys_profile,
    DROP CONSTRAINT api_keys_held_by_check,
    DROP CONSTRAINT api_keys_guardrail_scope_check;
ALTER TABLE grants DROP CONSTRAINT grants_role_check;
ALTER TABLE workareas
    DROP CONSTRAINT workareas_status_check,
    DROP CONSTRAINT workareas_universe_committed;
ALTER TABLE workarea_transitions
    DROP CONSTRAINT workarea_transitions_from_status_check,
    DROP CONSTRAINT workarea_transitions_to_status_check,
    DROP CONSTRAINT workarea_transitions_one_rung;

------------------------------------------------------------------------------
-- Relations and columns.
------------------------------------------------------------------------------
ALTER TABLE workareas RENAME TO workspaces;
ALTER TABLE workarea_transitions RENAME TO workspace_transitions;
ALTER TABLE search_workarea_rows RENAME TO search_workspace_rows;

ALTER TABLE api_keys RENAME COLUMN default_workarea_id TO default_workspace_id;
ALTER TABLE api_keys RENAME COLUMN held_by TO used_by;
ALTER TABLE brief_edges RENAME COLUMN workarea_id TO workspace_id;
ALTER TABLE grants RENAME COLUMN workarea_node_id TO workspace_node_id;
ALTER TABLE key_denials RENAME COLUMN workarea_node_id TO workspace_node_id;
ALTER TABLE memories RENAME COLUMN workarea_id TO workspace_id;
ALTER TABLE memories RENAME COLUMN approver_held_by TO approver_used_by;
ALTER TABLE recall_events RENAME COLUMN scope_workarea_id TO scope_workspace_id;
ALTER TABLE recall_events RENAME COLUMN source_workarea_id TO source_workspace_id;
ALTER TABLE recall_events RENAME COLUMN reference_workarea_ids TO reference_workspace_ids;
ALTER TABLE recall_events RENAME COLUMN federated_workarea_ids TO federated_workspace_ids;
ALTER TABLE search_postings RENAME COLUMN workarea_id TO workspace_id;
ALTER TABLE search_term_rows RENAME COLUMN workarea_id TO workspace_id;
ALTER TABLE search_vocabulary RENAME COLUMN workarea_id TO workspace_id;
ALTER TABLE search_workspace_rows RENAME COLUMN workarea_id TO workspace_id;
ALTER TABLE similar_keys RENAME COLUMN workarea_id TO workspace_id;
ALTER TABLE workspace_transitions RENAME COLUMN workarea_id TO workspace_id;

------------------------------------------------------------------------------
-- Every index, sequence and constraint name holding the token, from the
-- catalog: a partitioned parent's constraint first, which its partitions
-- inherit, then whatever a partition carries under a name of its own.
--
-- workspace is one byte longer than workarea, and a name PostgreSQL derived
-- for a partition's index can already fill the 63-byte limit with a numbered
-- suffix (`..._claim__idx1`); such a name loses the last byte before its
-- suffix, so two derived names never truncate onto one another.
------------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION pg_temp.oml_renamed(old text) RETURNS text
LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    renamed text := replace(old, 'workarea', 'workspace');
    suffix  text := coalesce(substring(renamed from '(_[a-z]+[0-9]*)$'), '');
    base    text;
    n       int := 0;
BEGIN
    IF octet_length(renamed) > 63 THEN
        renamed := left(renamed, 63 - octet_length(suffix)) || suffix;
    END IF;
    base := renamed;
    -- Two derived names that meet after the cut keep apart by a number, as
    -- PostgreSQL keeps derived names apart.
    WHILE to_regclass(quote_ident(renamed)) IS NOT NULL LOOP
        n := n + 1;
        renamed := left(base, 63 - octet_length(n::text)) || n::text;
    END LOOP;
    RETURN renamed;
END $$;

DO $$
DECLARE
    r record;
BEGIN
    FOR r IN
        SELECT c.relname
          FROM pg_class c
         WHERE c.relnamespace = 'public'::regnamespace
           AND c.relkind IN ('i', 'I', 'S')
           AND c.relname LIKE '%workarea%'
         ORDER BY c.relkind DESC, c.relname
    LOOP
        EXECUTE format('ALTER INDEX %I RENAME TO %I', r.relname,
                       pg_temp.oml_renamed(r.relname));
    END LOOP;
    FOR r IN
        SELECT co.conname, co.conrelid::regclass AS rel
          FROM pg_constraint co
          JOIN pg_class c ON c.oid = co.conrelid
         WHERE co.connamespace = 'public'::regnamespace
           AND co.conname LIKE '%workarea%'
           AND co.conislocal
         ORDER BY c.relkind DESC, co.conname
    LOOP
        -- A rename on a partitioned parent renames the partitions' inherited
        -- copy as well, so a later row may already be gone.
        IF EXISTS (SELECT 1 FROM pg_constraint
                    WHERE conrelid = r.rel AND conname = r.conname) THEN
            EXECUTE format('ALTER TABLE %s RENAME CONSTRAINT %I TO %I', r.rel, r.conname,
                           pg_temp.oml_renamed(r.conname));
        END IF;
    END LOOP;
END $$;

------------------------------------------------------------------------------
-- Functions whose stored body names a renamed relation or column.
------------------------------------------------------------------------------
DO $$
DECLARE
    f record;
BEGIN
    FOR f IN
        SELECT p.oid
          FROM pg_proc p
         WHERE p.pronamespace = 'public'::regnamespace
           AND p.prosrc LIKE '%workarea%'
    LOOP
        EXECUTE replace(pg_get_functiondef(f.oid), 'workarea', 'workspace');
    END LOOP;
END $$;

------------------------------------------------------------------------------
-- Stored values, under each tenant's row security, the tenants enumerated
-- under 0003's startup policy as 0044 enumerates them.
------------------------------------------------------------------------------
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE api_keys
           SET role = CASE role WHEN 'dreamer' THEN 'reviewer' ELSE role END,
               used_by = CASE used_by WHEN 'human' THEN 'person' ELSE used_by END,
               guardrail_scope = CASE guardrail_scope WHEN 'workarea' THEN 'workspace'
                                                      ELSE guardrail_scope END
         WHERE tenant_id = t
           AND (role = 'dreamer' OR used_by = 'human' OR guardrail_scope = 'workarea');
        UPDATE grants SET role = 'reviewer'
         WHERE tenant_id = t AND role = 'dreamer';
        UPDATE memories SET approver_used_by = 'person'
         WHERE tenant_id = t AND approver_used_by = 'human';
        UPDATE workspaces
           SET status = CASE status WHEN 'committed' THEN 'protected'
                                    WHEN 'validated' THEN 'frozen' END
         WHERE tenant_id = t AND status IN ('committed', 'validated');
        UPDATE workspace_transitions
           SET from_status = CASE from_status WHEN 'committed' THEN 'protected'
                                              WHEN 'validated' THEN 'frozen'
                                              ELSE from_status END,
               to_status = CASE to_status WHEN 'committed' THEN 'protected'
                                          WHEN 'validated' THEN 'frozen'
                                          ELSE to_status END
         WHERE tenant_id = t
           AND (from_status IN ('committed', 'validated')
                OR to_status IN ('committed', 'validated'));
        -- An act's claim is keyed `<key id>:<nil id>:<act>:<client key>`;
        -- only the act names of ADR-0070 d5 change. The match is a pattern:
        -- a client's own key that itself began with one of those act names
        -- and a colon would be rewritten too, a risk the operator accepted
        -- (ADR-0091 implementation notes).
        UPDATE idempotency
           SET key = regexp_replace(key,
                   '^([0-9a-f-]{36}:[0-9a-f-]{36}:)workarea\.(create|transition|ratify|mode):',
                   '\1workspace.\2:')
         WHERE tenant_id = t
           AND key ~ '^[0-9a-f-]{36}:[0-9a-f-]{36}:workarea\.(create|transition|ratify|mode):';
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END $$;

------------------------------------------------------------------------------
-- The checks again, in the new values. Every row was rewritten above, so each
-- holds; the tables are small and the server is stopped.
------------------------------------------------------------------------------
-- lint:allow constraint api_keys, grants, workspaces and workspace_transitions are small, every row was rewritten in this transaction, and the upgrade runs with the server stopped (ADR-0091 d12), so one scan each under the held lock is the cheaper path than a second migration
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_role_check
        CHECK (role IN ('reader', 'writer', 'reviewer', 'admin', 'operator')),
    ADD CONSTRAINT api_keys_used_by_check
        CHECK (used_by IN ('agent', 'person', 'service')),
    ADD CONSTRAINT api_keys_guardrail_scope_check
        CHECK (guardrail_scope IN ('workspace', 'ancestors')),
    ADD CONSTRAINT api_keys_profile
        CHECK (profile IS NULL
               OR (profile = 'curator' AND role = 'reviewer' AND used_by = 'agent'));
ALTER TABLE grants
    ADD CONSTRAINT grants_role_check CHECK (role IN ('reader', 'writer', 'reviewer'));
ALTER TABLE workspaces
    ADD CONSTRAINT workspaces_status_check
        CHECK (status IN ('development', 'protected', 'frozen')),
    ADD CONSTRAINT workspaces_universe_protected
        CHECK (parent_id IS NOT NULL OR status = 'protected');
ALTER TABLE workspace_transitions
    ADD CONSTRAINT workspace_transitions_from_status_check
        CHECK (from_status IN ('development', 'protected', 'frozen')),
    ADD CONSTRAINT workspace_transitions_to_status_check
        CHECK (to_status IN ('development', 'protected', 'frozen')),
    ADD CONSTRAINT workspace_transitions_one_rung
        CHECK ((from_status = 'development' AND to_status = 'protected')
               OR (from_status = 'protected' AND to_status = 'frozen'));

------------------------------------------------------------------------------
-- Comments.
------------------------------------------------------------------------------
COMMENT ON TABLE workspaces IS
    'Governed workspace tree, status ladder development→protected→frozen. Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN workspaces.accept_federated IS
    'Whether recalls scoped elsewhere may search this node, folded from workspace.federation_set; grants nothing (ADR-0072 d5). Owning page: docs/data/postgres-schema.md.';
COMMENT ON TABLE brief_edges IS
    'Claims joined to a workspace''s brief, pending until ratified by a key that is not the proposer; never deleted. Owning page: docs/data/postgres-schema.md.';
COMMENT ON TABLE search_vocabulary IS
    'Per workspace, the simple words of its promoted served tips and how many say each; did_you_mean''s only source (ADR-0049 d6, d12). Owning page: docs/data/postgres-schema.md.';
COMMENT ON TABLE search_term_rows IS
    'Per workspace, memory kind and token, how many served versions hold the token (ADR-0052 d5). Owning page: docs/data/postgres-schema.md.';
COMMENT ON TABLE search_workspace_rows IS
    'Per workspace and memory kind, the served versions and their summed search_len (ADR-0052 d5). Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN recall_events.sources IS
    'The recall''s sources on a federated tenant: "scope", "wide" or the list''s workspace ids; NULL with federation off (ADR-0072 d9).';
