-- 0013_oauth_clients — the provider tenant's OAuth state: the client cache
-- and the authorization-request rows (ADR-0031 d5, d9). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): 0001–0012 are never edited,
-- a correction is the next numbered file.
-- Normative shapes: docs/data/postgres-schema.md "connections (ADR-0031)".
-- Both tables hold state from before any connect code names a key, so they
-- belong to the reserved provider tenant (the nil uuid, oml-app's
-- types::PROVIDER_TENANT), under its RLS, and hold no tenant content.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- oauth_clients — clients the authorization server knows: a cached client ID
-- metadata document (cimd) or a dynamic registration (dcr) (ADR-0031 d5).
-- connected_at is stamped by the first Connect that names the client; the
-- cache's eviction and the removal of an unused registration read it, never
-- a tenant's connections.
------------------------------------------------------------------------------
CREATE TABLE oauth_clients (
    client_id       text        PRIMARY KEY,   -- a metadata document's https URL, or server-minted
    tenant_id       uuid        NOT NULL,
    kind            text        NOT NULL CHECK (kind IN ('cimd', 'dcr')),
    client_name     text        NOT NULL,
    redirect_uris   text[]      NOT NULL,      -- each inside the redirect allowlist (d6)
    metadata_digest bytea       NOT NULL,      -- SHA-256 of the document or registration as received
    fetched_at      timestamptz,               -- cimd: kept at most 24 hours from it
    max_age         integer     CHECK (max_age BETWEEN 0 AND 86400),
                                               -- cimd: its Cache-Control lifetime in seconds
                                               -- from fetched_at, capped at 24 hours
    last_used_at    timestamptz,               -- cimd: the eviction order
    connected_at    timestamptz,               -- the first Connect that named this client
    created_at      timestamptz NOT NULL,
    CONSTRAINT oauth_clients_provider_tenant
        CHECK (tenant_id = '00000000-0000-0000-0000-000000000000'::uuid)
);

-- The cache's eviction order: at most 10 000 cimd rows, the least recently
-- used one whose client has not connected evicted first (d5).
CREATE INDEX oauth_clients_evictable ON oauth_clients (kind, last_used_at)
    WHERE connected_at IS NULL;

-- Unused registrations: a dcr row is removed 24 hours after its creation
-- while connected_at is null, and at most 10 000 such rows exist (d5).
CREATE INDEX oauth_clients_unconnected ON oauth_clients (kind, created_at)
    WHERE connected_at IS NULL;

COMMENT ON TABLE oauth_clients IS
    'OAuth clients: cached metadata documents and dynamic registrations; provider tenant only (ADR-0031 d5). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- oauth_authorizations — an authorization request whose connect code
-- verified (ADR-0031 d9); before that the request is only its sealed handle
-- and has no row. A confirmation, Connect or Cancel, deletes the row by
-- handle_hash under the provider tenant and reads back code_tenant_id. The
-- code's id, tenant and key are plain uuids with no reference: the code is
-- another tenant's row, and a key carrying tenant_id cannot point at it from
-- the provider tenant.
------------------------------------------------------------------------------
CREATE TABLE oauth_authorizations (
    id                uuid        PRIMARY KEY,
    tenant_id         uuid        NOT NULL,
    handle_hash       bytea       NOT NULL UNIQUE,  -- SHA-256 of the confirmation's 256-bit handle
    client_id         text        NOT NULL,         -- no reference: a cached document may be evicted
    client_kind       text        NOT NULL CHECK (client_kind IN ('cimd', 'dcr')),
    metadata_digest   bytea       NOT NULL,         -- copied from the sealed handle
    redirect_uri      text        NOT NULL,
    redirect_uri_sent boolean     NOT NULL,         -- for the token endpoint's rule
    code_challenge    text        NOT NULL,         -- S256 only
    state             text,                         -- echoed verbatim
    expires_at        timestamptz NOT NULL,         -- the sealed handle's
    code_id           uuid        NOT NULL,         -- copied from the verified connect code
    code_tenant_id    uuid        NOT NULL,
    key_id            uuid        NOT NULL,
    CONSTRAINT oauth_authorizations_provider_tenant
        CHECK (tenant_id = '00000000-0000-0000-0000-000000000000'::uuid)
);

COMMENT ON TABLE oauth_authorizations IS
    'Authorization requests whose connect code verified: the handle''s SHA-256, never the handle; '
    'provider tenant only (ADR-0031 d9). Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- Row-level security — as 0004: FORCE, keyed by the transaction-local
-- oml.tenant_id, which is the provider tenant for every statement on these
-- tables. No key_resolution policy: a handle is found under the provider
-- tenant, never before a tenant is set.
------------------------------------------------------------------------------
ALTER TABLE oauth_clients ENABLE ROW LEVEL SECURITY;
ALTER TABLE oauth_clients FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON oauth_clients
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE oauth_authorizations ENABLE ROW LEVEL SECURITY;
ALTER TABLE oauth_authorizations FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON oauth_authorizations
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

------------------------------------------------------------------------------
-- The serving role's grants, in 0007's form (postgres-schema.md
-- "Privileges"). No table here grants TRUNCATE, REFERENCES, TRIGGER or
-- MAINTAIN.
------------------------------------------------------------------------------

-- oauth_clients: the cache's write and refetch, last_used_at and the
-- connected_at stamp, eviction and the removal of unused registrations.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON oauth_clients TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- oauth_authorizations: the row at a correct code, the confirmation's
-- DELETE … RETURNING, and the removal 24 hours after it lapses unused. A
-- row is never updated.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, DELETE ON oauth_authorizations TO %I',
                   current_setting('oml.serving_role'));
END $$;
