-- web_session_credentials — the capacity credentials of a web session
-- (ADR-0070 d2): for each capacity the session's entry reaches when it
-- opens (web, review, curate where the key exists, admin for an admin's
-- entry), the SHA-256 of `omls_` + base62(HMAC-SHA256(session token,
-- "oml/web-capacity/v1/<capacity>")), written in the commit that opens the
-- session. Never the credential: it is derived again from the cookie's
-- token by oml-web and nothing at rest decrypts into it. Operational state,
-- not journalled. A row ends with its session: the foreign key cascades
-- every way a session ends (sign-out, expiry, an entry change, a disabled
-- user), so no revocation is needed.
--
-- Numbered 0039, after the scan's rule set (0038). A new table and its
-- policies, and on web_sessions the (tenant_id, token_hash) key the
-- credentials' foreign key names, so that it carries tenant_id as every
-- foreign key into a tenant table does: its index is built over the live
-- sessions only, rows that expire within a session's lifetime, so the build
-- holds the lock for no longer than reading them; a lock is waited for at
-- most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE web_sessions
    ADD CONSTRAINT web_sessions_tenant_token UNIQUE (tenant_id, token_hash);

CREATE TABLE web_session_credentials (
    credential_hash bytea PRIMARY KEY CHECK (octet_length(credential_hash) = 32),
    tenant_id       uuid  NOT NULL,
    token_hash      bytea NOT NULL,
    capacity        text  NOT NULL CHECK (capacity IN ('web', 'review', 'curate', 'admin')),
    UNIQUE (token_hash, capacity),
    FOREIGN KEY (tenant_id, token_hash) REFERENCES web_sessions (tenant_id, token_hash)
        ON DELETE CASCADE
);

COMMENT ON TABLE web_session_credentials IS
    'Capacity credentials of a web session: SHA-256 only, ending with the session (ADR-0070 d2). '
    'Owning page: docs/data/postgres-schema.md.';

-- Row-level security as 0015's web_sessions: FORCE, keyed by the
-- transaction-local oml.tenant_id, and found before the tenant is known by
-- an exact match on oml.api_key_hash, SELECT only.
ALTER TABLE web_session_credentials ENABLE ROW LEVEL SECURITY;
ALTER TABLE web_session_credentials FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON web_session_credentials
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY key_resolution ON web_session_credentials FOR SELECT
    USING (credential_hash = decode(NULLIF(current_setting('oml.api_key_hash', true), ''), 'hex'));

-- The serving role writes a session's credentials and reads one; the
-- cascade removes them. A row is never updated.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, DELETE ON web_session_credentials TO %I',
                   current_setting('oml.serving_role'));
END $$;
