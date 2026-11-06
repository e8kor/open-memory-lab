-- 0038_usage_metering — per-account usage meters (ADR-0070): the counter row
-- each metered commit moves in its own transaction, the per-day flows, the
-- append-only daily ledger, an account's quota profile, and the one
-- definition of every stock meter that the fill below and the drift report
-- both recompute from. Forward-only (docs/data/postgres-schema.md
-- "Migrations"): no earlier file is edited. Normative shapes:
-- docs/data/postgres-schema.md "usage metering (ADR-0070)".
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.
--
-- Safe on a live database: four new tables, two functions, a check widened
-- on user_quotas (at most six rows a person, new in 0037), and a fill of
-- the new rows from the stored ones; a lock is waited for at most 5 s, then
-- the migration fails and is run again in a quieter moment.

SET LOCAL lock_timeout = '5s';

------------------------------------------------------------------------------
-- account_meters — one account's four stocks (ADR-0070 d2, d3), moved by the
-- commit that folds a metered event, in its transaction. An account is the
-- id of a person's root workarea (ADR-0070 d1).
------------------------------------------------------------------------------
CREATE TABLE account_meters (
    tenant_id   uuid   NOT NULL REFERENCES tenants (id),
    user_root   uuid   NOT NULL,
    memories    bigint NOT NULL DEFAULT 0,
    text_bytes  bigint NOT NULL DEFAULT 0,
    keys        bigint NOT NULL DEFAULT 0,
    connections bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, user_root)
);
COMMENT ON TABLE account_meters IS
    'One account''s stock meters, moved in the metered commit''s own transaction (ADR-0070 d3). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- account_activity — one account's flows in one UTC day (ADR-0070 d2): the
-- versions it submitted, moved by the commit, and the recalls it made.
------------------------------------------------------------------------------
CREATE TABLE account_activity (
    tenant_id uuid   NOT NULL REFERENCES tenants (id),
    user_root uuid   NOT NULL,
    day       date   NOT NULL,
    writes    bigint NOT NULL DEFAULT 0,
    recalls   bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, user_root, day)
);
COMMENT ON TABLE account_activity IS
    'One account''s writes and recalls in one UTC day, kept 62 days (ADR-0070 d2, d4). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- usage_daily — the ledger: one row per account per closed UTC day,
-- appended once and never updated, kept 25 months (ADR-0070 d4).
------------------------------------------------------------------------------
CREATE TABLE usage_daily (
    tenant_id   uuid        NOT NULL REFERENCES tenants (id),
    user_root   uuid        NOT NULL,
    day         date        NOT NULL,
    profile     text,
    memories    bigint      NOT NULL,
    text_bytes  bigint      NOT NULL,
    writes      bigint      NOT NULL,
    recalls     bigint      NOT NULL,
    keys        bigint      NOT NULL,
    connections bigint      NOT NULL,
    recorded_at timestamptz NOT NULL,
    PRIMARY KEY (tenant_id, user_root, day)
);
CREATE INDEX usage_daily_day ON usage_daily (tenant_id, day);
COMMENT ON TABLE usage_daily IS
    'The usage ledger: one append-only row per account per closed UTC day, kept 25 months '
    '(ADR-0070 d4). Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- account_profiles — the quota profile an admin assigned, folded from
-- account.profile_set; an absent row is the deployment's default for the
-- account (ADR-0070 d5).
------------------------------------------------------------------------------
CREATE TABLE account_profiles (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    user_root uuid NOT NULL,
    profile   text NOT NULL,
    PRIMARY KEY (tenant_id, user_root)
);
COMMENT ON TABLE account_profiles IS
    'The quota profile an admin assigned an account, folded from account.profile_set (ADR-0070 d5). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- user_quotas: the two quotas ADR-0070 d5 adds, and a ceiling wide enough
-- for a byte count. The table is new in 0037 and holds at most six rows a
-- person, so the validation's scan is short.
------------------------------------------------------------------------------
ALTER TABLE user_quotas DROP CONSTRAINT user_quotas_quota_check;
ALTER TABLE user_quotas DROP CONSTRAINT user_quotas_max_count_check;
ALTER TABLE user_quotas
    ADD CONSTRAINT user_quotas_quota
        CHECK (quota IN ('memories', 'text_bytes', 'writes_per_month', 'keys', 'connections',
                         'requests_per_minute')) NOT VALID;
ALTER TABLE user_quotas
    ADD CONSTRAINT user_quotas_max_count
        CHECK (max_count BETWEEN 0 AND 1000000000000) NOT VALID;
-- lint:allow validate user_quotas is new in 0037 and holds at most six rows a person
ALTER TABLE user_quotas VALIDATE CONSTRAINT user_quotas_quota;
ALTER TABLE user_quotas VALIDATE CONSTRAINT user_quotas_max_count;

------------------------------------------------------------------------------
-- The one definition of a chain's account and of a live chain (ADR-0070
-- d2): the account is the author's of the chain's first version; a chain is
-- live while a version of it is held, promoted or demoted and its promoted
-- tip is not a retirement. The fold reads both before and after a memory
-- fold, by the memories_claim_version index; the source below reads them
-- for every chain.
------------------------------------------------------------------------------
CREATE FUNCTION oml_chain_account(t uuid, w uuid, ck text) RETURNS uuid
    LANGUAGE sql STABLE AS $$
    SELECT k.user_root_id
      FROM memories m JOIN api_keys k ON k.tenant_id = m.tenant_id AND k.id = m.author_key_id
     WHERE m.tenant_id = t AND m.workarea_id = w AND m.claim_key = ck
     ORDER BY m.version, m.created_at
     LIMIT 1
$$;

CREATE FUNCTION oml_chain_live(t uuid, w uuid, ck text) RETURNS boolean
    LANGUAGE sql STABLE AS $$
    SELECT EXISTS (SELECT 1 FROM memories m
                    WHERE m.tenant_id = t AND m.workarea_id = w AND m.claim_key = ck
                      AND m.status IN ('held', 'promoted', 'demoted'))
       AND NOT EXISTS (SELECT 1 FROM memories m
                        WHERE m.tenant_id = t AND m.workarea_id = w AND m.claim_key = ck
                          AND m.status = 'promoted' AND m.is_tip AND m.retirement)
$$;

------------------------------------------------------------------------------
-- oml_account_source — every account's stocks recomputed from the rows, by
-- ADR-0070 d2: what the fill writes and what the drift report compares the
-- counters with (d3). Read under the caller's row security.
------------------------------------------------------------------------------
CREATE FUNCTION oml_account_source(t uuid)
    RETURNS TABLE (user_root uuid, memories bigint, text_bytes bigint, keys bigint,
                   connections bigint)
    LANGUAGE sql STABLE AS $$
    WITH chains AS (
        SELECT m.workarea_id, m.claim_key,
               (array_agg(m.author_key_id ORDER BY m.version, m.created_at))[1] AS first_author,
               bool_or(m.status IN ('held', 'promoted', 'demoted')) AS has_live,
               bool_or(m.status = 'promoted' AND m.is_tip AND m.retirement) AS retired
          FROM memories m WHERE m.tenant_id = t
         GROUP BY m.workarea_id, m.claim_key
    ), mem AS (
        SELECT k.user_root_id AS root, count(*) AS n
          FROM chains c JOIN api_keys k ON k.tenant_id = t AND k.id = c.first_author
         WHERE c.has_live AND NOT c.retired AND k.user_root_id IS NOT NULL
         GROUP BY k.user_root_id
    ), bytes AS (
        SELECT k.user_root_id AS root, sum(m.body_bytes)::bigint AS n
          FROM memories m JOIN api_keys k ON k.tenant_id = m.tenant_id AND k.id = m.author_key_id
         WHERE m.tenant_id = t AND k.user_root_id IS NOT NULL AND m.status <> 'erased'
         GROUP BY k.user_root_id
    ), ks AS (
        SELECT k.user_root_id AS root, count(*) AS n
          FROM api_keys k
         WHERE k.tenant_id = t AND k.user_root_id IS NOT NULL AND k.revoked_at IS NULL
           AND k.credential <> 'session'
         GROUP BY k.user_root_id
    ), cs AS (
        SELECT k.user_root_id AS root, count(*) AS n
          FROM oauth_connections c JOIN api_keys k ON k.tenant_id = c.tenant_id AND k.id = c.key_id
         WHERE c.tenant_id = t AND k.user_root_id IS NOT NULL AND c.state = 'open'
         GROUP BY k.user_root_id
    ), roots AS (
        SELECT root FROM mem UNION SELECT root FROM bytes
        UNION SELECT root FROM ks UNION SELECT root FROM cs
    )
    SELECT r.root, coalesce(mem.n, 0), coalesce(bytes.n, 0), coalesce(ks.n, 0), coalesce(cs.n, 0)
      FROM roots r
      LEFT JOIN mem ON mem.root = r.root
      LEFT JOIN bytes ON bytes.root = r.root
      LEFT JOIN ks ON ks.root = r.root
      LEFT JOIN cs ON cs.root = r.root
$$;

------------------------------------------------------------------------------
-- Row-level security — as 0004: FORCE, keyed by the transaction-local
-- oml.tenant_id.
------------------------------------------------------------------------------
ALTER TABLE account_meters ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_meters FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON account_meters
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE account_activity ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_activity FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON account_activity
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE usage_daily ENABLE ROW LEVEL SECURITY;
ALTER TABLE usage_daily FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON usage_daily
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE account_profiles ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_profiles FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON account_profiles
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

------------------------------------------------------------------------------
-- The fill: every existing tenant's counters from its rows, so a live
-- database starts with no drift (ADR-0070 Consequences), under each
-- tenant's row security, the tenants enumerated under 0003's startup policy
-- as 0021 enumerates them. The month's writes are its held versions.
------------------------------------------------------------------------------
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        INSERT INTO account_meters (tenant_id, user_root, memories, text_bytes, keys, connections)
        SELECT t, s.user_root, s.memories, s.text_bytes, s.keys, s.connections
          FROM oml_account_source(t) s;
        INSERT INTO account_activity (tenant_id, user_root, day, writes)
        SELECT t, k.user_root_id, (m.created_at AT TIME ZONE 'UTC')::date, count(*)
          FROM memories m JOIN api_keys k ON k.tenant_id = m.tenant_id AND k.id = m.author_key_id
         WHERE m.tenant_id = t AND k.user_root_id IS NOT NULL
           AND m.created_at >= now() - interval '62 days'
         GROUP BY 1, 2, 3;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;

------------------------------------------------------------------------------
-- The serving role's grants, in 0007's form (postgres-schema.md
-- "Privileges"). The ledger is never updated: its rows are inserted once
-- and deleted only by the 25-month retention (ADR-0070 d4).
------------------------------------------------------------------------------
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON account_meters TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON account_activity TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, DELETE ON usage_daily TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON account_profiles TO %I',
                   current_setting('oml.serving_role'));
END $$;
