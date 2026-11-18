-- Workspace export policy and allowances (ADR-0084 d10; ADR-0071 d2 as
-- amended by ADR-0084; #425).
--
-- tenants.export_policy is the tenant's `export` policy: `off`, `own_root`
-- (the default: a person exports only inside their own root) or `allowed`
-- (shared subtrees too). export_allowances holds an admin's allowance of one
-- shared workspace's export under `own_root`, folded from
-- `export.allowance_set`; until is null for an allowance without an end.
-- account_activity.exports and usage_daily.exports count the bytes of the
-- account's exports on the day, the `exports` meter.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.
--
-- What it costs a live database: three columns with constant defaults, which
-- PostgreSQL adds without rewriting a row, each under an ACCESS EXCLUSIVE
-- lock held for the catalog change only; one new, empty table with its
-- policy, whose foreign key takes SHARE ROW EXCLUSIVE on tenants until the
-- commit. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

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

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON export_allowances TO %I',
                   current_setting('oml.serving_role'));
END $$;
