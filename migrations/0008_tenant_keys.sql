-- 0008_tenant_keys — a foreign key into a tenant table carries tenant_id
-- (ADR-0034 d12, d10 step 3). Forward-only (docs/data/postgres-schema.md
-- "Migrations"): 0001–0007 are never edited, a correction is the next
-- numbered file. Normative shape: docs/data/postgres-schema.md "Keys into
-- tenant tables".
--
-- PostgreSQL runs a foreign-key check without row-level security, so a
-- reference on id alone accepts another tenant's row and answers whether
-- that id exists anywhere. Each key below is rewritten as (tenant_id, col)
-- → (tenant_id, id), the form 0004 gave grants and 0006 gave the crew
-- references, so a cross-tenant reference is refused as one to no row is:
-- 23503, from the one constraint. Each keeps its 0001 name, so the refusal
-- the adapter maps reads as it did. The keys are MATCH SIMPLE: on a
-- provider-level api_keys row, whose tenant_id is NULL, "the row is not
-- required to have a match" (CREATE TABLE, PostgreSQL 17).
--
-- Adding a key validates every existing row, without row-level security, so
-- a database that already holds a cross-tenant reference fails this
-- migration with 23503, naming the constraint and the key, and commits
-- nothing; no row is rewritten to make the key fit.
--
-- The single-column keys into tenant tables that 0001 also declared are
-- gone already: 0002 dropped memories.workarea_id's and 0005 dropped
-- memories.superseded_by with its key.
--
-- Run by the owner role. The migration adds constraints and their unique
-- indexes and grants nothing: the serving role's DML on each table is
-- 0007's, unchanged (postgres-schema.md "Privileges").

------------------------------------------------------------------------------
-- The targets: a UNIQUE (tenant_id, …) on each referenced table that lacks
-- one. workareas and api_keys have theirs from 0004.
------------------------------------------------------------------------------
ALTER TABLE journal
    ADD CONSTRAINT journal_tenant_id_event_id UNIQUE (tenant_id, event_id);
ALTER TABLE memories
    ADD CONSTRAINT memories_tenant_id_id UNIQUE (tenant_id, id);

------------------------------------------------------------------------------
-- The rewritten keys.
------------------------------------------------------------------------------
ALTER TABLE workareas
    DROP CONSTRAINT workareas_parent_id_fkey,
    ADD CONSTRAINT workareas_parent_id_fkey
        FOREIGN KEY (tenant_id, parent_id) REFERENCES workareas (tenant_id, id);

ALTER TABLE memories
    DROP CONSTRAINT memories_created_event_id_fkey,
    ADD CONSTRAINT memories_created_event_id_fkey
        FOREIGN KEY (tenant_id, created_event_id) REFERENCES journal (tenant_id, event_id);

ALTER TABLE review_queue
    DROP CONSTRAINT review_queue_memory_id_fkey,
    ADD CONSTRAINT review_queue_memory_id_fkey
        FOREIGN KEY (tenant_id, memory_id) REFERENCES memories (tenant_id, id);

ALTER TABLE api_keys
    DROP CONSTRAINT api_keys_default_workarea_id_fkey,
    ADD CONSTRAINT api_keys_default_workarea_id_fkey
        FOREIGN KEY (tenant_id, default_workarea_id) REFERENCES workareas (tenant_id, id),
    DROP CONSTRAINT api_keys_rotated_from_fkey,
    ADD CONSTRAINT api_keys_rotated_from_fkey
        FOREIGN KEY (tenant_id, rotated_from) REFERENCES api_keys (tenant_id, id);

------------------------------------------------------------------------------
-- The provider-level references hold no foreign key. journal.api_key_id and
-- api_keys.issued_by name a provider-level key by design, as when an
-- operator key issues a tenant's first admin key (docs/api/http-api.md), so
-- no key carrying tenant_id could match them. Each holds the id of the key
-- its request authenticated with, which the use case has read and no client
-- names; 0002 dropped review_queue.held_by_key's key for the same reason.
------------------------------------------------------------------------------
ALTER TABLE journal
    DROP CONSTRAINT journal_api_key_id_fkey;
ALTER TABLE api_keys
    DROP CONSTRAINT api_keys_issued_by_fkey;
