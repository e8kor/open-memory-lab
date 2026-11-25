-- 0067_workspaces_meter — the seventh meter, `workspaces` (ADR-0095 d4,
-- d10): the live workspaces under an account's root, the root excluded,
-- whoever created them. A workspace is never deleted, so every row counts.
-- account_meters.workspaces is the stock the create's own commit moves and
-- holds to its limit (ADR-0071 d3); usage_daily.workspaces is the ledger's
-- copy of it each closed day (d4); user_quotas admits the quota by name, so
-- an admin's override of it is stored (d5). oml_account_source gains the
-- column, recomputed from workspaces.namespace_root_id, the user root whose
-- namespace holds a node's name (0014): the fill below and the drift report
-- read it, so a live database starts with no drift. Forward-only: no
-- earlier file is edited.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires. The
-- fill reads and writes under each tenant's row security as 0065's does, so
-- an owner that is NOSUPERUSER NOBYPASSRLS runs it under FORCE.
--
-- What it costs a live database: two columns with a constant default, which
-- PostgreSQL adds without rewriting a row, each under an ACCESS EXCLUSIVE
-- lock; a check replaced on user_quotas, validated by one scan of that small
-- table; the source function dropped and created again with its new column,
-- which the serving role is granted nothing on, since it runs as its caller;
-- and the fill, one aggregate of workspaces and one UPDATE of account_meters
-- per tenant, a row per account. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

-- lint:allow column-check the added column takes a constant default that satisfies its CHECK, so the validating scan reads every existing row and refuses none; account_meters holds one row per account, and it runs under the lock the ADD COLUMN takes either way

ALTER TABLE account_meters
    ADD COLUMN workspaces bigint NOT NULL DEFAULT 0 CHECK (workspaces >= 0);
COMMENT ON COLUMN account_meters.workspaces IS
    'Live workspaces under the account''s root, the root excluded, moved by workspace.create in its '
    'own transaction (ADR-0095 d4).';

ALTER TABLE usage_daily
    ADD COLUMN workspaces bigint NOT NULL DEFAULT 0;
COMMENT ON COLUMN usage_daily.workspaces IS
    'Live workspaces under the account''s root at the day''s rollup (ADR-0095 d4).';

------------------------------------------------------------------------------
-- user_quotas: the seventh quota by name (ADR-0095 d4). The table holds at
-- most seven rows a person.
------------------------------------------------------------------------------
ALTER TABLE user_quotas DROP CONSTRAINT user_quotas_quota;
ALTER TABLE user_quotas
    ADD CONSTRAINT user_quotas_quota
        CHECK (quota IN ('memories', 'text_bytes', 'writes_per_month', 'keys', 'connections',
                         'requests_per_minute', 'workspaces')) NOT VALID;
-- lint:allow validate user_quotas holds at most seven rows a person, and every stored row names one of the six quotas the replaced check admitted
ALTER TABLE user_quotas VALIDATE CONSTRAINT user_quotas_quota;

------------------------------------------------------------------------------
-- oml_account_source — 0065's definition with the workspaces stock: the
-- nodes whose name lies in a user root's namespace, which is every node
-- below that root and never the root itself (0014's user-root
-- check). Its result type changes, so it is dropped and created again.
------------------------------------------------------------------------------
DROP FUNCTION oml_account_source(uuid);

CREATE FUNCTION oml_account_source(t uuid)
    RETURNS TABLE (user_root uuid, memories bigint, text_bytes bigint, keys bigint,
                   connections bigint, workspaces bigint)
    LANGUAGE sql STABLE AS $$
    WITH accounts AS (
        SELECT k.id, coalesce(k.user_root_id, p.id) AS root
          FROM api_keys k
          LEFT JOIN workspaces p
            ON k.user_root_id IS NULL AND k.credential = 'session'
           AND k.name = k.session_for || '-review'
           AND p.tenant_id = k.tenant_id AND p.user_root AND p.name = k.session_for
         WHERE k.tenant_id = t
    ), chains AS (
        SELECT m.workspace_id, m.claim_key,
               (array_agg(m.author_key_id ORDER BY m.version, m.created_at))[1] AS first_author,
               bool_or(m.status IN ('held', 'promoted', 'demoted')) AS has_live,
               bool_or(m.status = 'promoted' AND m.is_tip AND m.retirement) AS retired
          FROM memories m WHERE m.tenant_id = t
         GROUP BY m.workspace_id, m.claim_key
    ), mem AS (
        SELECT a.root, count(*) AS n
          FROM chains c JOIN accounts a ON a.id = c.first_author
         WHERE c.has_live AND NOT c.retired AND a.root IS NOT NULL
         GROUP BY a.root
    ), bytes AS (
        SELECT a.root, sum(m.body_bytes)::bigint AS n
          FROM memories m JOIN accounts a ON a.id = m.author_key_id
         WHERE m.tenant_id = t AND a.root IS NOT NULL AND m.status <> 'erased'
         GROUP BY a.root
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
           AND k.revoked_at IS NULL AND k.credential <> 'session'
         GROUP BY k.user_root_id
    ), ws AS (
        SELECT w.namespace_root_id AS root, count(*) AS n
          FROM workspaces w
         WHERE w.tenant_id = t AND w.namespace_root_id IS NOT NULL
         GROUP BY w.namespace_root_id
    ), roots AS (
        SELECT root FROM mem UNION SELECT root FROM bytes
        UNION SELECT root FROM ks UNION SELECT root FROM cs UNION SELECT root FROM ws
    )
    SELECT r.root, coalesce(mem.n, 0), coalesce(bytes.n, 0), coalesce(ks.n, 0), coalesce(cs.n, 0),
           coalesce(ws.n, 0)
      FROM roots r
      LEFT JOIN mem ON mem.root = r.root
      LEFT JOIN bytes ON bytes.root = r.root
      LEFT JOIN ks ON ks.root = r.root
      LEFT JOIN cs ON cs.root = r.root
      LEFT JOIN ws ON ws.root = r.root
$$;

------------------------------------------------------------------------------
-- The fill: each account's workspaces from the tree, under each tenant's row
-- security, the tenants enumerated under 0003's startup policy as 0065's
-- fill enumerates them; the startup marker is set before the loop, since
-- the tenants row is read under it. An account with no meter row gets one
-- from the source; the other stocks of an existing row are left as they are.
------------------------------------------------------------------------------
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE account_meters am
           SET workspaces = s.workspaces
          FROM oml_account_source(t) s
         WHERE am.tenant_id = t AND am.user_root = s.user_root
           AND am.workspaces IS DISTINCT FROM s.workspaces;
        INSERT INTO account_meters
               (tenant_id, user_root, memories, text_bytes, keys, connections, workspaces)
        SELECT t, s.user_root, s.memories, s.text_bytes, s.keys, s.connections, s.workspaces
          FROM oml_account_source(t) s
        ON CONFLICT (tenant_id, user_root) DO NOTHING;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;
