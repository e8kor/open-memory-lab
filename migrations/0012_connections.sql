-- 0012_connections — the connector key's credential, connect codes,
-- connections and their tokens (ADR-0031 d7, d8, d10). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): 0001–0009 are never edited,
-- a correction is the next numbered file. 0010 and 0011 are other lanes'
-- numbers and this file takes neither.
-- Normative shapes: docs/data/postgres-schema.md "api_keys, grants,
-- key_denials, bans" and "connections (ADR-0031)". oauth_clients and
-- oauth_authorizations, the provider tenant's deployment-level state, are a
-- later migration's.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- api_keys.credential — folded from key.issued.credential (ADR-0031 d7). A
-- connector key has no bearer value, so its hash is NULL; a bearer key's
-- hash is NULL only after a re-fold, which restores every key but not its
-- hash (postgres-schema.md "Folds of the journal vs operational tables").
-- Every row this migration finds was issued as a bearer key and folds as
-- one through the column's default, which is then dropped: a row written
-- since names its credential.
------------------------------------------------------------------------------
ALTER TABLE api_keys
    ADD COLUMN credential text NOT NULL DEFAULT 'bearer'
        CHECK (credential IN ('bearer', 'connection'));
ALTER TABLE api_keys ALTER COLUMN credential DROP DEFAULT;
ALTER TABLE api_keys ALTER COLUMN hash DROP NOT NULL;
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_connector_has_no_hash
        CHECK (credential = 'bearer' OR hash IS NULL);

------------------------------------------------------------------------------
-- connect_codes — the code that opens a connector key's connection, stored
-- only as the SHA-256 of its twelve decoded symbols (ADR-0031 d8). voided_seq
-- is the seq of the connection.code_issued, connection.revoked or
-- key.revoked that voided it.
------------------------------------------------------------------------------
CREATE TABLE connect_codes (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    key_id      uuid        NOT NULL,
    hash        bytea       NOT NULL UNIQUE,   -- SHA-256 of the 12 decoded symbols
    issued_seq  bigint      NOT NULL,          -- journal seq of its connection.code_issued
    expires_at  timestamptz NOT NULL,
    redeemed_at timestamptz,                   -- its first correct entry on the page
    voided_seq  bigint,
    -- the target of oauth_connections' tenant-scoped code reference
    CONSTRAINT connect_codes_tenant_id_id UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id)
);

-- One live code per key: minting a code voids every unredeemed one of the
-- key in its commit (ADR-0031 d8).
CREATE UNIQUE INDEX connect_codes_live ON connect_codes (tenant_id, key_id)
    WHERE redeemed_at IS NULL AND voided_seq IS NULL;

COMMENT ON TABLE connect_codes IS
    'Connect codes: SHA-256 only, never the code; one live code per key (ADR-0031 d8). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- oauth_connections — a row is created at Connect, state 'authorized', and
-- opens at the token exchange (ADR-0031 d10). The client is named, never
-- referenced: oauth_clients is the provider tenant's.
------------------------------------------------------------------------------
CREATE TABLE oauth_connections (
    id                     uuid        PRIMARY KEY,
    tenant_id              uuid        NOT NULL,
    key_id                 uuid        NOT NULL,
    code_id                uuid        NOT NULL,
    client_id              text        NOT NULL,
    client_kind            text        NOT NULL CHECK (client_kind IN ('cimd', 'dcr')),
    client_metadata_digest bytea       NOT NULL,
    redirect_uri           text        NOT NULL,
    redirect_uri_sent      boolean     NOT NULL,
    resource               text        NOT NULL,         -- <issuer>/mcp at Connect (d10, d11)
    code_challenge         text        NOT NULL,
    auth_code_hash         bytea       UNIQUE,           -- kept after redemption, so a second
                                                         -- redemption is detected (d10)
    auth_code_expires_at   timestamptz,
    auth_code_redeemed_at  timestamptz,
    state                  text        NOT NULL
        CHECK (state IN ('authorized', 'open', 'revoked')),
    opened_seq             bigint,                       -- its connection.opened
    revoked_seq            bigint,                       -- its connection.revoked, or the
                                                         -- connection.opened that replaced it
    revoked_cause          text
        CHECK (revoked_cause IN ('admin', 'refresh_reuse', 'code_reuse', 'replaced')),
    created_at             timestamptz NOT NULL,
    -- the target of oauth_tokens' tenant-scoped connection reference
    CONSTRAINT oauth_connections_tenant_id_id UNIQUE (tenant_id, id),
    -- one connect code, at most one connection
    CONSTRAINT oauth_connections_tenant_id_code_id UNIQUE (tenant_id, code_id),
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id),
    CONSTRAINT oauth_connections_code_id_fkey
        FOREIGN KEY (tenant_id, code_id) REFERENCES connect_codes (tenant_id, id)
);

-- At most one open connection per key (ADR-0031 d7).
CREATE UNIQUE INDEX oauth_connections_open ON oauth_connections (tenant_id, key_id)
    WHERE state = 'open';

COMMENT ON TABLE oauth_connections IS
    'Connections of connector keys: one per connect code, at most one open per key (ADR-0031 d7, d10). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- oauth_tokens — a connection's access and refresh tokens, stored only as
-- the SHA-256 of the token string.
------------------------------------------------------------------------------
CREATE TABLE oauth_tokens (
    hash          bytea       PRIMARY KEY,      -- SHA-256 of the token string
    tenant_id     uuid        NOT NULL,
    connection_id uuid        NOT NULL,
    kind          text        NOT NULL CHECK (kind IN ('access', 'refresh')),
    expires_at    timestamptz NOT NULL,         -- set at minting; rotation never changes it
    rotated_at    timestamptz,                  -- refresh only: presenting it after this is reuse
    created_at    timestamptz NOT NULL,
    CONSTRAINT oauth_tokens_connection_id_fkey
        FOREIGN KEY (tenant_id, connection_id) REFERENCES oauth_connections (tenant_id, id)
);

COMMENT ON TABLE oauth_tokens IS
    'Access and refresh tokens of connections: SHA-256 only, never the token (ADR-0031 d10). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- Row-level security — as 0004: FORCE, keyed by the transaction-local
-- oml.tenant_id. The code, authorization-code and token digests are found
-- before the tenant is known by the lookup that finds api_keys.hash: 0001's
-- key_resolution form, an exact match on oml.api_key_hash, SELECT only; the
-- matched row's tenant_id then sets oml.tenant_id for everything after
-- (postgres-schema.md "connections (ADR-0031)").
------------------------------------------------------------------------------
ALTER TABLE connect_codes ENABLE ROW LEVEL SECURITY;
ALTER TABLE connect_codes FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON connect_codes
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY key_resolution ON connect_codes FOR SELECT
    USING (hash = decode(NULLIF(current_setting('oml.api_key_hash', true), ''), 'hex'));

ALTER TABLE oauth_connections ENABLE ROW LEVEL SECURITY;
ALTER TABLE oauth_connections FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON oauth_connections
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY key_resolution ON oauth_connections FOR SELECT
    USING (auth_code_hash = decode(NULLIF(current_setting('oml.api_key_hash', true), ''), 'hex'));

ALTER TABLE oauth_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE oauth_tokens FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON oauth_tokens
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY key_resolution ON oauth_tokens FOR SELECT
    USING (hash = decode(NULLIF(current_setting('oml.api_key_hash', true), ''), 'hex'));

------------------------------------------------------------------------------
-- The serving role's grants, in 0007's form (postgres-schema.md
-- "Privileges"): the DML the connection statements the schema page states
-- use. No table here grants TRUNCATE, REFERENCES, TRIGGER or MAINTAIN.
------------------------------------------------------------------------------

-- connect_codes: the mint, the digest read, redemption and voiding.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON connect_codes TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- oauth_connections: Connect, the exchange's and the revocation's
-- conditional updates, the clearing of auth_code_hash, and the removal of
-- an authorized row whose code lapsed unexchanged.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON oauth_connections TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- oauth_tokens: minting, the digest read, rotation's conditional update, and
-- the removal at a token's expiry or its connection's revocation.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON oauth_tokens TO %I',
                   current_setting('oml.serving_role'));
END $$;
