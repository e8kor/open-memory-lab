-- Workspace imports and the import ledger (ADR-0084 d6-d11, R11, I4; #426).
--
-- imports: one row per import a person's writer key ran, bound to the one
-- upload session that carried the decrypted bundle (I4). The upload's own
-- part_key is the staging key, nulled when the import ends, so no staging
-- table exists here (ADR-0005's "upload staging" row is that session).
-- import_ledger: per target workspace and source, the newest export instant
-- and release imported, and the fingerprint of the key the source's first
-- import pinned (d8, R11). import_versions: every version an import wrote,
-- with the source version it carries, so an import can be rolled back
-- (d11). memories.import_provenance: the source server, tenant, workspace,
-- artifact, version, digest, author pseudonym and verbatim origin of an
-- imported version (d4, amending ADR-0063 d1). workspace.imported journals
-- the end of each import; its fold sets the row done and moves the ledger.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.
--
-- What it costs a live database: three new, empty tables with their
-- indexes and policies, and one nullable column on memories with no
-- default, which rewrites no row. ALTER TABLE memories takes ACCESS
-- EXCLUSIVE on memories and its partitions for the catalog change only;
-- the foreign keys to tenants take SHARE ROW EXCLUSIVE on tenants until the
-- commit. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories ADD COLUMN import_provenance jsonb;
COMMENT ON COLUMN memories.import_provenance IS
    'An imported version''s source provenance (ADR-0084 d4); null for every version not imported.';

CREATE TABLE imports (
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    id                  uuid        NOT NULL,
    target_workspace_id uuid        NOT NULL,
    importer_key_id     uuid        NOT NULL,
    upload_id           uuid        NOT NULL,
    source_server_id    text        NOT NULL,
    source_tenant_id    text        NOT NULL,
    manifest_digest     text        NOT NULL,
    signer_fingerprint  text        NOT NULL,
    authenticated       boolean     NOT NULL,
    allow_older         boolean     NOT NULL,
    state               text        NOT NULL CHECK (state IN ('running', 'done', 'failed')),
    counts              jsonb       NOT NULL DEFAULT '{}',
    refusals            jsonb       NOT NULL DEFAULT '[]',
    created_at          timestamptz NOT NULL,
    ended_at            timestamptz,
    PRIMARY KEY (tenant_id, id),
    -- One upload binds once (ADR-0004 d3).
    UNIQUE (tenant_id, upload_id),
    CHECK ((state = 'running') = (ended_at IS NULL))
);
COMMENT ON TABLE imports IS
    'Workspace imports (ADR-0084 d11, I4): one per import, bound to one upload session. '
    'Owning page: docs/data/postgres-schema.md.';
CREATE INDEX imports_target_source ON imports
    (tenant_id, target_workspace_id, source_server_id, source_tenant_id);

CREATE TABLE import_ledger (
    tenant_id               uuid        NOT NULL REFERENCES tenants (id),
    target_workspace_id     uuid        NOT NULL,
    source_server_id        text        NOT NULL,
    source_tenant_id        text        NOT NULL,
    exported_at             text        NOT NULL,
    release_label           text,
    release_manifest_digest text,
    signer_fingerprint      text        NOT NULL,
    updated_at              timestamptz NOT NULL,
    PRIMARY KEY (tenant_id, target_workspace_id, source_server_id, source_tenant_id)
);
COMMENT ON TABLE import_ledger IS
    'Per target workspace and source: the newest export instant and release imported and the '
    'pinned signer fingerprint (ADR-0084 d8, R11). Owning page: docs/data/postgres-schema.md.';

CREATE TABLE import_versions (
    tenant_id          uuid   NOT NULL REFERENCES tenants (id),
    import_id          uuid   NOT NULL,
    memory_id          uuid   NOT NULL,
    source_artifact_id text   NOT NULL,
    source_version     bigint NOT NULL,
    outcome            text   NOT NULL CHECK (outcome IN ('new', 'changed', 'removed', 'conflict')),
    PRIMARY KEY (tenant_id, import_id, memory_id),
    FOREIGN KEY (tenant_id, import_id) REFERENCES imports (tenant_id, id)
);
COMMENT ON TABLE import_versions IS
    'Every version an import wrote and the source version it carries (ADR-0084 d11). '
    'Owning page: docs/data/postgres-schema.md.';
CREATE INDEX import_versions_memory ON import_versions (tenant_id, memory_id);

-- Row-level security — as 0004: FORCE, keyed by the transaction-local
-- oml.tenant_id.
ALTER TABLE imports ENABLE ROW LEVEL SECURITY;
ALTER TABLE imports FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON imports
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
ALTER TABLE import_ledger ENABLE ROW LEVEL SECURITY;
ALTER TABLE import_ledger FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON import_ledger
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
ALTER TABLE import_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE import_versions FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON import_versions
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

-- The serving role's grants, in 0007's form. Nothing is deleted: an
-- import's row and its versions are kept for its rollback (d11, d13).
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON imports, import_ledger TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT ON import_versions TO %I',
                   current_setting('oml.serving_role'));
END $$;
