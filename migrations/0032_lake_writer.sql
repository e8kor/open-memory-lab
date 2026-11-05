-- 0032_lake_writer — the lake writer's two tables in Postgres: the keys that
-- seal the bodies it materializes, and the outbox rows it could not
-- materialize (ADR-0003 d6, ADR-0005 d1, d2, ADR-0060 d6, d14).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. Normative shapes: docs/data/postgres-schema.md "artifact_keys"
-- and "lake_dead_letter".
--
-- Run by the owner role, with oml.serving_role set as 0007 requires. The
-- serving role is granted nothing here: no statement of the lead reads or
-- writes either table (ADR-0034 d3). The lake writer's own login is granted
-- by oml-lakewriter itself, through the owner URL, at its start
-- (ADR-0060 d13), because no migration knows that role's name.
--
-- Safe on a live database: both tables are new and unreferenced, so nothing
-- waits on them; a lock on `tenants` for the foreign key is waited for at most
-- 5 s, then the migration fails and is run again in a quieter moment.

SET LOCAL lock_timeout = '5s';

------------------------------------------------------------------------------
-- artifact_keys — one data key per materialized claim version, wrapped by
-- the lake's key-encryption key; erase destroys it (ADR-0005 d1, d2). The
-- shape is postgres-schema.md's: wrapped_dek is NULL exactly when
-- destroyed_at is set, and a destroyed row is never re-keyed.
------------------------------------------------------------------------------
CREATE TABLE artifact_keys (
    tenant_id    uuid           NOT NULL REFERENCES tenants (id),
    artifact_id  uuid           NOT NULL,
    wrapped_dek  bytea,
    kek_id       text           NOT NULL,
    created_at   timestamptz(6) NOT NULL DEFAULT now(),
    destroyed_at timestamptz(6),
    PRIMARY KEY (tenant_id, artifact_id),
    CONSTRAINT artifact_keys_destroyed CHECK ((wrapped_dek IS NULL) = (destroyed_at IS NOT NULL))
);
COMMENT ON TABLE artifact_keys IS
    'Per-version data keys sealing lake bodies, wrapped by the lake KEK; erase nulls the wrapped key '
    '(ADR-0005 d2, ADR-0060 d6). Owning page: docs/data/postgres-schema.md.';

ALTER TABLE artifact_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE artifact_keys FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON artifact_keys
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

------------------------------------------------------------------------------
-- lake_dead_letter — an outbox row a lake table could not take, by table and
-- reason, never its payload: the row stays in the outbox and the journal,
-- where a repair re-reads it (ADR-0003 d6, ADR-0060 d14).
------------------------------------------------------------------------------
CREATE TABLE lake_dead_letter (
    tenant_id   uuid           NOT NULL REFERENCES tenants (id),
    outbox_seq  bigint         NOT NULL,
    lake_table  text           NOT NULL
        CHECK (lake_table IN ('bronze.events', 'silver.claims', 'gold.lineage')),
    reason      text           NOT NULL CHECK (reason ~ '^[a-z_]{1,60}$'),
    at          timestamptz(6) NOT NULL DEFAULT now(),
    repaired_at timestamptz(6),
    PRIMARY KEY (tenant_id, outbox_seq, lake_table)
);
COMMENT ON TABLE lake_dead_letter IS
    'Outbox rows a lake table skipped, with the reason; the lake is knowingly incomplete until '
    'repaired (ADR-0003 d6, ADR-0060 d14). Owning page: docs/data/postgres-schema.md.';

ALTER TABLE lake_dead_letter ENABLE ROW LEVEL SECURITY;
ALTER TABLE lake_dead_letter FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON lake_dead_letter
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
