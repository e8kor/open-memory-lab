-- The serving role destroys an erased version's data key in the erase's own
-- transaction (ADR-0005 d2, amended 2026-11-17; ADR-0089 d16; #455).
--
-- artifact_keys was the lake writer's alone (0031): the serving role held no
-- grant on it. An erase now nulls each keyed version's wrapped key and
-- stamps destroyed_at in the transaction that journals lifecycle.erase, so
-- the serving role gains exactly that UPDATE and the columns its WHERE
-- reads, never a read of wrapped_dek or kek_id and never an INSERT: a
-- version the lake writer has not keyed gets its destroyed row from the
-- lake writer when it folds the same event, as it does today.
--
-- Numbered 0056, after the reservations of bulk/v082-b (0053 to 0055).
--
-- What it costs a live database: catalog changes only; no table is scanned.
-- A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

DO $$
BEGIN
    EXECUTE format(
        'GRANT SELECT (tenant_id, artifact_id, destroyed_at), UPDATE (wrapped_dek, destroyed_at) '
        'ON artifact_keys TO %I',
        current_setting('oml.serving_role'));
END $$;

COMMENT ON TABLE artifact_keys IS
    'Per-version data keys sealing lake bodies, wrapped by the lake KEK; erase nulls the wrapped key '
    '(ADR-0005 d2, ADR-0060 d6). The serving role holds only a column grant: SELECT on tenant_id, '
    'artifact_id and destroyed_at and UPDATE on wrapped_dek and destroyed_at, so it destroys a key '
    'in the erase transaction and reads none (0056). Owning page: docs/data/postgres-schema.md.';
