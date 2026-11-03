-- 0007_role_split — the serving role holds only the DML the adapter's
-- statements use, and only the owner reaches _sqlx_migrations (ADR-0034 d2,
-- d3, d6). Forward-only (docs/data/postgres-schema.md "Migrations"):
-- 0001–0006 are never edited, a correction is the next numbered file.
-- Normative shape: docs/data/postgres-schema.md "Privileges", the per-table
-- grant matrix this file makes true.
--
-- Run by the owner role, which owns what the migrations create. The grantee
-- is the setting oml.serving_role, which the lead sets on every owner
-- connection to its serving URL's current_user (PgStore::migrate); no role
-- name is written here or read from configuration (ADR-0034 d2). A run
-- without the setting fails at current_setting with 42704 and grants
-- nothing. Each grant is the DML a statement in crates/adapters/oml-pg and
-- crates/bins/oml-server-lead uses on that table, with USAGE on a sequence an
-- INSERT draws on; TRUNCATE, REFERENCES, TRIGGER and MAINTAIN are granted on
-- no table, being whole-table operations row security does not bind
-- (ADR-0034 d3). A table a later migration adds refuses the serving role
-- until that migration grants it.

------------------------------------------------------------------------------
-- _sqlx_migrations — the one table without row-level security: no tenant
-- data and no tenant_id to key a policy on. Its fence is privilege
-- (ADR-0034 d6).
------------------------------------------------------------------------------
REVOKE ALL ON _sqlx_migrations FROM PUBLIC;

-- No role but the owner creates in public (ADR-0034 d2).
REVOKE CREATE ON SCHEMA public FROM PUBLIC;

------------------------------------------------------------------------------
-- The grants, one table per block (postgres-schema.md "Privileges").
------------------------------------------------------------------------------

-- tenants: the genesis insert (ON CONFLICT DO NOTHING), the naming's
-- UPDATE, and the reads, enumerate_tenants' under 0003's startup policy
-- among them.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON tenants TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- workareas: workarea.create, re-parent, status, crew and the naming's
-- name and path UPDATEs.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON workareas TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- api_keys: issuance, rotation, revocation and the naming's UPDATEs, and
-- the key read by hash.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON api_keys TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- journal: SELECT and INSERT alone; append-only by privilege before the
-- guard trigger (ADR-0002 d4).
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT ON journal TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- journal_chain_head: the head read and the upsert (ON CONFLICT DO UPDATE).
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON journal_chain_head TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- outbox: INSERT alone, with USAGE on the bigserial sequence it draws seq
-- from; the adapter reads no outbox row.
DO $$
BEGIN
    EXECUTE format('GRANT INSERT ON outbox TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT USAGE ON SEQUENCE %s TO %I',
                   pg_get_serial_sequence('outbox', 'seq'),
                   current_setting('oml.serving_role'));
END $$;

-- memories: the hold, the status, promotion and tip UPDATEs, and every read.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON memories TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- review_queue: the one table a statement deletes from, at a verdict; the
-- claim's UPDATE and its FOR NO KEY UPDATE row lock.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON review_queue TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- idempotency: the record (ON CONFLICT DO NOTHING) and the replay read.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT ON idempotency TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- grants: grant.issued and key_access' read.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT ON grants TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- key_denials: the denial, its lift (UPDATE lifted_epoch) and the read.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON key_denials TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- crews: crew.created, crew.updated and the naming's crews.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON crews TO %I',
                   current_setting('oml.serving_role'));
END $$;
