-- 0015_web — the web pages' email allowlist and sessions (ADR-0045 d2, d4,
-- d5). Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier
-- file is edited. 0014 is left to ADR-0044's user roots, which land before
-- this file in the v0.5.0 build order.
-- Normative shapes: docs/data/postgres-schema.md "web (ADR-0045)".
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- web_emails — the allowlist, folded from web.email_allowed,
-- web.email_changed and web.email_removed. The email is held ASCII-lower-
-- cased and found by email_hash, the digest the journal carries in its place
-- (oml-domain web::Email::digest); email_hash is unique in the deployment,
-- so a sign-in, which happens before any tenant is known, finds at most one
-- entry (ADR-0045 d2). The provider subjects are pinned at each provider's
-- first sign-in, an operational write that journals nothing (d4).
------------------------------------------------------------------------------
CREATE TABLE web_emails (
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    email_hash     bytea       NOT NULL CHECK (octet_length(email_hash) = 32),
    email          text        NOT NULL CHECK (email = lower(email) AND octet_length(email) <= 254),
    user_name      oml_name    NOT NULL CHECK (char_length(user_name) <= 56),
    admin          boolean     NOT NULL,
    google_subject text,
    github_subject text,
    created_at     timestamptz NOT NULL,
    PRIMARY KEY (tenant_id, email_hash),
    CONSTRAINT web_emails_email_hash UNIQUE (email_hash)
);

COMMENT ON TABLE web_emails IS
    'The web allowlist: an email, the user it signs in as and its admin flag; unique in the deployment (ADR-0045 d2). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- web_sessions — one row per signed-in browser, under the SHA-256 of a
-- fresh 256-bit token, never the token (ADR-0045 d5). Operational state:
-- not journalled, ended at sign-out, at expiry, and in the commit that
-- changes or removes its entry.
------------------------------------------------------------------------------
CREATE TABLE web_sessions (
    token_hash bytea       PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    tenant_id  uuid        NOT NULL,
    email_hash bytea       NOT NULL,
    user_name  oml_name    NOT NULL,
    admin      boolean     NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL CHECK (expires_at > created_at),
    FOREIGN KEY (tenant_id, email_hash) REFERENCES web_emails (tenant_id, email_hash)
);

CREATE INDEX web_sessions_entry ON web_sessions (tenant_id, email_hash);

COMMENT ON TABLE web_sessions IS
    'Web sessions: SHA-256 of the token only, never journalled (ADR-0045 d5). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- api_keys: the session credential (ADR-0045 d6). A session key has no
-- value, so no hash, as a connector key has none; it records the allowlist
-- user it serves, and only a session key does.
------------------------------------------------------------------------------
ALTER TABLE api_keys DROP CONSTRAINT api_keys_credential_check;
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_credential_check
        CHECK (credential IN ('bearer', 'connection', 'session'));
ALTER TABLE api_keys ADD COLUMN session_for oml_name;
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_session_user
        CHECK ((credential = 'session') = (session_for IS NOT NULL));

------------------------------------------------------------------------------
-- web_emails.added_by — the admin key that added the entry, under which the
-- entry's first sign-in issues the person's session keys (ADR-0045 d6).
------------------------------------------------------------------------------
ALTER TABLE web_emails ADD COLUMN added_by uuid NOT NULL;

------------------------------------------------------------------------------
-- Row-level security — as 0004: FORCE, keyed by the transaction-local
-- oml.tenant_id. An entry's email digest and a session's token digest are
-- found before the tenant is known by 0001's key_resolution form, an exact
-- match on oml.api_key_hash, SELECT only; the matched row's tenant_id then
-- sets oml.tenant_id for everything after.
------------------------------------------------------------------------------
ALTER TABLE web_emails ENABLE ROW LEVEL SECURITY;
ALTER TABLE web_emails FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON web_emails
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY key_resolution ON web_emails FOR SELECT
    USING (email_hash = decode(NULLIF(current_setting('oml.api_key_hash', true), ''), 'hex'));

ALTER TABLE web_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE web_sessions FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON web_sessions
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY key_resolution ON web_sessions FOR SELECT
    USING (token_hash = decode(NULLIF(current_setting('oml.api_key_hash', true), ''), 'hex'));

------------------------------------------------------------------------------
-- The serving role's grants, in 0007's form (postgres-schema.md
-- "Privileges"). No table here grants TRUNCATE, REFERENCES, TRIGGER or
-- MAINTAIN.
------------------------------------------------------------------------------

-- web_emails: the three folds and the pin.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON web_emails TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- web_sessions: a session's row, its read, and its end. A row is never
-- updated.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, DELETE ON web_sessions TO %I',
                   current_setting('oml.serving_role'));
END $$;
