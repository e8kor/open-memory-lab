-- Upload sessions (ADR-0004 d3, as amended 2026-11-17; #451).
--
-- A key opens a session declaring the size and SHA-256 of what it sends,
-- stores fixed 8 MiB parts as objects at storage.url, and completes it
-- against the declared digest. A row holds metadata only: the parts are
-- XChaCha20-Poly1305 ciphertext under part_key, the session's own key, which
-- is nulled when the session expires, is abandoned, fails its digest or the
-- verb that bound it ends; nulling it is the shred (ADR-0005's inventory row
-- "upload staging"). objects_removed_at records that the parts are gone. No
-- event names an upload, so nothing here is journalled, and the table is
-- outside the lake's twin (ADR-0090 d18), as staging is.
--
-- No foreign key to api_keys: a key row is never deleted (ADR-0001 R4), and
-- one would take a SHARE ROW EXCLUSIVE lock on api_keys. The application
-- checks the opening key on every route.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.
--
-- What it costs a live database: one new, empty table with its indexes and
-- its policy. The foreign key to tenants takes SHARE ROW EXCLUSIVE on tenants
-- until the commit, which a tenant's creation or settings change waits for;
-- nothing reads or rewrites a row. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

CREATE TABLE upload_sessions (
    tenant_id          uuid        NOT NULL REFERENCES tenants (id),
    id                 uuid        NOT NULL,
    key_id             uuid        NOT NULL,
    purpose            text        NOT NULL CHECK (purpose IN ('import')),
    size               bigint      NOT NULL CHECK (size > 0),
    sha256             text        NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    received           bigint[]    NOT NULL DEFAULT '{}',
    state              text        NOT NULL CHECK (state IN
                           ('open', 'completed', 'bound', 'failed', 'expired', 'abandoned')),
    part_key           bytea       CHECK (part_key IS NULL OR length(part_key) = 32),
    created_at         timestamptz NOT NULL,
    touched_at         timestamptz NOT NULL,
    expires_at         timestamptz,
    completed_at       timestamptz,
    bound_at           timestamptz,
    objects_removed_at timestamptz,
    PRIMARY KEY (tenant_id, id),
    -- An open or unbound session always expires; a bound one ends with
    -- the verb that bound it.
    CHECK (state NOT IN ('open', 'completed') OR expires_at IS NOT NULL)
);
COMMENT ON TABLE upload_sessions IS
    'Upload sessions (ADR-0004 d3, as amended 2026-11-17): metadata only; the parts are '
    'ciphertext at storage.url under part_key, nulled to shred. Never journalled. '
    'Owning page: docs/data/postgres-schema.md.';

-- The key's and the tenant's limits count the open and unbound sessions.
CREATE INDEX upload_sessions_live ON upload_sessions (tenant_id, key_id)
    WHERE state IN ('open', 'completed');
-- The sweep reads what expired and what is left to remove.
CREATE INDEX upload_sessions_expiry ON upload_sessions (tenant_id, expires_at)
    WHERE state IN ('open', 'completed');
CREATE INDEX upload_sessions_unswept ON upload_sessions (tenant_id)
    WHERE part_key IS NULL AND objects_removed_at IS NULL;

-- Row-level security — as 0004: FORCE, keyed by the transaction-local
-- oml.tenant_id.
ALTER TABLE upload_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE upload_sessions FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON upload_sessions
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

-- The serving role's grants, in 0007's form. Rows are never deleted: a
-- session's row outlives its key and its objects.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON upload_sessions TO %I',
                   current_setting('oml.serving_role'));
END $$;
