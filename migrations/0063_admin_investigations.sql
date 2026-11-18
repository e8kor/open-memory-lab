-- Admin investigations: one reason per run of admin reads (ADR-0089 d10,
-- d15; #423).
--
-- admin_investigations: one row per investigation an admin key opened with
-- POST /v2/admin/investigations {reason}, folded from
-- admin.investigation_opened. The reason is stored in clear, as the signed
-- event carries it (ADR-0005 d3 as amended by ADR-0090 d7). Each admin read
-- of claim text names one open investigation of its own key, and its fold,
-- memory.admin_read's, counts the read here. An investigation stays open
-- until its key closes it, admin.investigation_closed (the operator,
-- 2026-11-17: no idle expiry); oml doctor names one open for more than
-- seven days.
--
-- The row is derived: the journal's opened, read and closed events rebuild
-- it (ADR-0090 d18).
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.
--
-- What it costs a live database: one new, empty table with one index and its
-- policy; the foreign key to tenants takes SHARE ROW EXCLUSIVE on tenants
-- until the commit. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

CREATE TABLE admin_investigations (
    tenant_id   uuid        NOT NULL REFERENCES tenants (id),
    id          uuid        NOT NULL,
    key_id      uuid        NOT NULL,
    reason      text        NOT NULL CHECK (reason <> ''),
    opened_at   timestamptz NOT NULL,
    opened_seq  bigint      NOT NULL,
    reads       bigint      NOT NULL DEFAULT 0 CHECK (reads >= 0),
    closed_at   timestamptz,
    closed_seq  bigint,
    PRIMARY KEY (tenant_id, id),
    CHECK ((closed_at IS NULL) = (closed_seq IS NULL))
);
COMMENT ON TABLE admin_investigations IS
    'An admin key''s investigation: the reason its reads of claim text are made under, open until '
    'the key closes it (ADR-0089 d10, d15). Owning page: docs/data/postgres-schema.md.';
CREATE INDEX admin_investigations_open ON admin_investigations (tenant_id, key_id)
    WHERE closed_at IS NULL;

ALTER TABLE admin_investigations ENABLE ROW LEVEL SECURITY;
ALTER TABLE admin_investigations FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON admin_investigations
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON admin_investigations TO %I',
                   current_setting('oml.serving_role'));
END $$;
