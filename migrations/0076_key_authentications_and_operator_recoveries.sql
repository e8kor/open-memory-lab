-- 0076_key_authentications_and_operator_recoveries — the server
-- authentication table recovery's dormancy reads (ADR-0097 d19, the
-- operator's answer 24 of 2026-11-28; ADR-0042 d8 as amended), and the
-- operator recoveries its background sweep completes (d19, answer 22).
--
-- key_authentications: one row per tenant, key and UTC day, with a count.
-- The lead writes a key's row at most once a day per process, at the key's
-- first authentication of that day, the day's later ones coalesced in
-- memory: one upsert per key per day (the bottleneck register). A sweep
-- deletes rows of a day more than 30 days back. The rows carry ids and a
-- count, never a principal, address or request id; they are operational,
-- not derived from the journal, and no replay rebuilds them.
--
-- key_authentications_installed: one row, the time this migration ran. The
-- table covers from the oldest retained row's day, bounded below by it, so
-- a fresh install covers nothing and recover fails closed with
-- log_too_short (answer 24). Deployment wide, it carries no tenant data.
--
-- operator_recoveries: one row per `oml operator recover` request, folded
-- from operator.recovery_requested, and moved once to completed
-- (operator.recovery) or cancelled (operator.recovery_cancelled). At most
-- one requested row per tenant. The address the new admin person's entry
-- gets is kept here for the 72 hours, as web_emails keeps an entry's; the
-- journal carries its digest alone.
--
-- What it costs a live database: three new tables, one row in one of them;
-- no existing row is read or rewritten.

SET LOCAL lock_timeout = '5s';

CREATE TABLE key_authentications (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    key_id uuid NOT NULL,
    day date NOT NULL,
    count bigint NOT NULL CHECK (count > 0),
    PRIMARY KEY (tenant_id, key_id, day)
);
CREATE INDEX key_authentications_day ON key_authentications (tenant_id, day);
COMMENT ON TABLE key_authentications IS
    'One row per tenant, key and UTC day a key authenticated, kept 30 days: recovery reads dormancy from it (ADR-0097 d19, answer 24).';

ALTER TABLE key_authentications ENABLE ROW LEVEL SECURITY;
ALTER TABLE key_authentications FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON key_authentications
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
-- The coverage read is deployment wide: the oldest day any tenant's row
-- holds, under the 0003 startup-enumeration setting (answer 24).
CREATE POLICY coverage_read ON key_authentications FOR SELECT
    USING (current_setting('oml.startup_check', true) = 'on');

CREATE TABLE key_authentications_installed (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    installed_at timestamptz NOT NULL DEFAULT now()
);
COMMENT ON TABLE key_authentications_installed IS
    'When key_authentications was installed: its coverage is bounded below by this time (ADR-0097, answer 24).';
INSERT INTO key_authentications_installed DEFAULT VALUES;

CREATE TABLE operator_recoveries (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    id uuid NOT NULL,
    person oml_name NOT NULL,
    email text NOT NULL CHECK (email = lower(email) AND octet_length(email) <= 254),
    state text NOT NULL CHECK (state IN ('requested', 'completed', 'cancelled')),
    requested_at timestamptz NOT NULL,
    requested_seq bigint NOT NULL,
    settled_at timestamptz,
    settled_seq bigint,
    PRIMARY KEY (tenant_id, id)
);
CREATE UNIQUE INDEX operator_recoveries_one_requested ON operator_recoveries (tenant_id)
    WHERE state = 'requested';
COMMENT ON TABLE operator_recoveries IS
    'The operator''s recoveries of a dormant tenant, requested, then completed by the sweep or cancelled by an admin sign-in (ADR-0097 d19).';

ALTER TABLE operator_recoveries ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_recoveries FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON operator_recoveries
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON key_authentications TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT ON key_authentications_installed TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON operator_recoveries TO %I',
                   current_setting('oml.serving_role'));
END $$;
