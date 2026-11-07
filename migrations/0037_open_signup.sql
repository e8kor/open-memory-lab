-- 0037_open_signup — open sign-up's rows (ADR-0068 d2, d5, d6, d8): the terms
-- version a self-signed-up entry accepted and the time an entry was
-- disabled, one person's quota overrides, and the deletion requests.
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. Normative shapes: docs/data/postgres-schema.md "web (ADR-0045)"
-- and "open sign-up (ADR-0068)".
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.
--
-- Safe on a live database: two nullable columns on web_emails, a metadata
-- change, and two new tables; a lock is waited for at most 5 s, then the
-- migration fails and is run again in a quieter moment.

SET LOCAL lock_timeout = '5s';

------------------------------------------------------------------------------
-- web_emails: the terms version a person accepted when they signed up
-- themselves (NULL for an entry an admin added), and the time an admin
-- disabled the person (NULL while enabled).
------------------------------------------------------------------------------
ALTER TABLE web_emails ADD COLUMN terms_version text;
ALTER TABLE web_emails ADD COLUMN disabled_at timestamptz;

------------------------------------------------------------------------------
-- user_quotas — one person's overrides, folded from web.quota_set; an
-- absent row or quota is the deployment's default (ADR-0068 d5).
------------------------------------------------------------------------------
CREATE TABLE user_quotas (
    tenant_id  uuid   NOT NULL REFERENCES tenants (id),
    user_root  uuid   NOT NULL,
    quota      text   NOT NULL
        CHECK (quota IN ('memories', 'keys', 'connections', 'requests_per_minute')),
    max_count  bigint NOT NULL CHECK (max_count BETWEEN 0 AND 1000000),
    PRIMARY KEY (tenant_id, user_root, quota)
);
COMMENT ON TABLE user_quotas IS
    'One person''s quota overrides, folded from web.quota_set (ADR-0068 d5). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- web_deletion_requests — a person's request to have their account deleted,
-- folded from web.deletion_requested and closed by web.deletion_closed; a
-- closed row is kept (ADR-0068 d6).
------------------------------------------------------------------------------
CREATE TABLE web_deletion_requests (
    tenant_id    uuid        NOT NULL REFERENCES tenants (id),
    user_name    oml_name    NOT NULL,
    requested_at timestamptz NOT NULL,
    closed_at    timestamptz,
    PRIMARY KEY (tenant_id, user_name, requested_at)
);
CREATE UNIQUE INDEX web_deletion_requests_pending
    ON web_deletion_requests (tenant_id, user_name) WHERE closed_at IS NULL;
COMMENT ON TABLE web_deletion_requests IS
    'Deletion requests, pending until an admin closes them; never removed (ADR-0068 d6). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- Row-level security — as 0004: FORCE, keyed by the transaction-local
-- oml.tenant_id.
------------------------------------------------------------------------------
ALTER TABLE user_quotas ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_quotas FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON user_quotas
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE web_deletion_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE web_deletion_requests FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON web_deletion_requests
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

------------------------------------------------------------------------------
-- The serving role's grants, in 0007's form (postgres-schema.md
-- "Privileges"). A deletion request is never deleted.
------------------------------------------------------------------------------
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON user_quotas TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON web_deletion_requests TO %I',
                   current_setting('oml.serving_role'));
END $$;
