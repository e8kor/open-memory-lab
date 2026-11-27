-- 0074_admin_read_counts_notices_and_host_verbs — the admin read counted per
-- person whose root it read (ADR-0097 d16).
--
-- admin_read_counts: one row per person and month (YYYY-MM, UTC), the count
-- of admin-read pages that served a row of that person's own root. Folded
-- from memory.admin_read, whose fold already moves its investigation's read
-- count: one counter write more per admin-read page (the bottleneck
-- register's "per-person admin-read count" row). The person reads their own
-- rows on their own page. The rows are derived: the journal's
-- memory.admin_read events rebuild them, the person read off each artifact's
-- workspace (ADR-0090 d18).
--
-- What it costs a live database: one new, empty table with its primary key
-- and its policy; no existing row is read or rewritten.

SET LOCAL lock_timeout = '5s';

CREATE TABLE admin_read_counts (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    person oml_name NOT NULL,
    month text NOT NULL CHECK (month ~ '^[0-9]{4}-[0-9]{2}$'),
    reads bigint NOT NULL CHECK (reads > 0),
    PRIMARY KEY (tenant_id, person, month)
);
COMMENT ON TABLE admin_read_counts IS
    'Admin-read pages that served a row of the person''s own root, per month; the person sees the count (ADR-0097 d16).';

ALTER TABLE admin_read_counts ENABLE ROW LEVEL SECURITY;
ALTER TABLE admin_read_counts FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON admin_read_counts
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON admin_read_counts TO %I',
                   current_setting('oml.serving_role'));
END $$;
