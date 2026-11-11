-- Idempotency claims for the web console's acts (ADR-0070 d3 layer 4, d5;
-- #233).
--
-- An act that commits in more than one transaction now claims its
-- `Idempotency-Key` before its first: a row with `committed_at` null, in a
-- transaction of its own. The act's outcome is written into that row after
-- its last commit, and a refused act deletes the row, so a resubmission acts
-- again. A claim's `response` names the claim's id and when it was taken, so
-- only the attempt that took it settles or releases it, and an operator can
-- see how old it is. A recorded row (`committed_at` set) is never updated or
-- deleted by the serving role: both statements name `committed_at IS NULL`,
-- and UPDATE is granted on `response` and `committed_at` alone, so no
-- statement of that role can rewrite a row's key, tenant or digest. The table
-- already holds a nullable `response` and `committed_at` (0001), so no column
-- changes.
--
-- What it costs a live database, stated: GRANT takes ACCESS SHARE on
-- `idempotency` and COMMENT takes SHARE UPDATE EXCLUSIVE, measured on
-- PostgreSQL 18.6 (pg_locks, 2026-11-11). Neither conflicts with a read or a
-- write of the table, so no request waits on this file and it waits on
-- none; SHARE UPDATE EXCLUSIVE waits only for a VACUUM, ANALYZE or index
-- build running on the table, at most 5 s, then the migration fails and is
-- run again. No row is read or rewritten. Forward-only, as every migration
-- here: undoing it is REVOKE UPDATE (response, committed_at), DELETE.

SET LOCAL lock_timeout = '5s';

DO $$
BEGIN
    EXECUTE format('GRANT UPDATE (response, committed_at), DELETE ON idempotency TO %I',
                   current_setting('oml.serving_role'));
END $$;

COMMENT ON TABLE idempotency IS
    'Idempotency records: same key + different request_digest is a typed error; '
    'replay with the same digest returns the stored response. A row with '
    'committed_at null is a claim an act holds until it records or is refused. '
    'Owning page: docs/data/postgres-schema.md.';
