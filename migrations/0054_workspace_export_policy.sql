-- Workspace export policy and allowances (ADR-0084 d10; ADR-0071 d2 as
-- amended by ADR-0084; #425).
--
-- tenants.export_policy is the tenant's `export` policy: `off`, `own_root`
-- (the default: a person exports only inside their own root) or `allowed`
-- (shared subtrees too). export_allowances holds an admin's allowance of one
-- shared workspace's export under `own_root`, folded from
-- `export.allowance_set`; until is null for an allowance without an end.
-- account_activity.exports and usage_daily.exports count the bytes of the
-- account's exports on the day, the `exports` meter. tenant_activity.exports
-- counts the bytes of every export of the tenant on the day, a key with no
-- account root's included, so no export goes uncounted (ADR-0084 I19); its
-- rows are kept 62 days, as account_activity's are.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.
--
-- What it costs a live database: three columns with constant defaults, which
-- PostgreSQL adds without rewriting a row, each under an ACCESS EXCLUSIVE
-- lock held for the catalog change only; two new, empty tables with their
-- policies, whose foreign keys take SHARE ROW EXCLUSIVE on tenants until the
-- commit. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

-- lint:allow column-check both added columns take a constant default that satisfies their CHECK, so the validating scan reads every existing row and refuses none; tenants holds one row per tenant and account_activity one per account and day for at most 62 days (its retention, ADR-0071 d4), so the scan is bounded by accounts times 62, and it runs under the lock the ADD COLUMN takes either way

ALTER TABLE tenants
    ADD COLUMN export_policy text NOT NULL DEFAULT 'own_root'
        CHECK (export_policy IN ('off', 'own_root', 'allowed'));
COMMENT ON COLUMN tenants.export_policy IS
    'Who may export a workspace bundle: off, own_root (a person, inside their own root) or allowed '
    '(shared subtrees too). ADR-0084 d10.';

CREATE TABLE export_allowances (
    tenant_id     uuid        NOT NULL REFERENCES tenants (id),
    workspace_id  uuid        NOT NULL,
    until         timestamptz,
    set_by_key_id uuid        NOT NULL,
    event_id      uuid        NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id)
);
COMMENT ON TABLE export_allowances IS
    'An admin''s allowance of one shared workspace''s export under the own_root policy, folded from '
    'export.allowance_set (ADR-0084 d10). It reads no content. Owning page: docs/data/postgres-schema.md.';

ALTER TABLE export_allowances ENABLE ROW LEVEL SECURITY;
ALTER TABLE export_allowances FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON export_allowances
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE account_activity
    ADD COLUMN exports bigint NOT NULL DEFAULT 0 CHECK (exports >= 0);
COMMENT ON COLUMN account_activity.exports IS
    'Bytes of the account''s workspace exports on the day, the exports meter (ADR-0084 d10).';
ALTER TABLE usage_daily
    ADD COLUMN exports bigint NOT NULL DEFAULT 0;
COMMENT ON COLUMN usage_daily.exports IS
    'Bytes of the account''s workspace exports on the day (ADR-0084 d10).';

CREATE TABLE tenant_activity (
    tenant_id uuid   NOT NULL REFERENCES tenants (id),
    day       date   NOT NULL,
    exports   bigint NOT NULL DEFAULT 0 CHECK (exports >= 0),
    PRIMARY KEY (tenant_id, day)
);
COMMENT ON TABLE tenant_activity IS
    'Bytes of every workspace export of the tenant in one UTC day, a key with no account root''s '
    'included, kept 62 days: the tenant''s exports meter (ADR-0084 I19). '
    'Owning page: docs/data/postgres-schema.md.';

ALTER TABLE tenant_activity ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_activity FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON tenant_activity
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON export_allowances TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON tenant_activity TO %I',
                   current_setting('oml.serving_role'));
END $$;
