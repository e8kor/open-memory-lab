-- 0065_session_key_account — a person's review session key is charged to
-- its person's account (ADR-0071 d1, d3; ADR-0082 d10; ADR-0071 d1's note
-- of 2026-11-19, the operator's decision; the review of PR #494, H1).
--
-- `<user>-review` carries no user_root_id, so the claims and proposals it
-- writes charged no account and its chains belonged to none: a person's
-- writes through it were held to no limit. A key's account is now
-- oml_key_account: its user_root_id, or for the review capacity's session
-- key, the one named `<session_for>-review`, the user root its session_for
-- names. The person's root is the workspace of that name with user_root
-- set; workspace names are unique in a tenant (0006). Curation is not
-- charged to the person: `<user>-curate` writes stay unmetered, as they
-- were before this migration, and `<user>-admin` is mapped to no account.
-- The chain account the fold reads and the source the drift report
-- compares with read it, as the core and MemStore do, so the counters and
-- the recount agree.
--
-- What it costs a live database: two CREATE OR REPLACE FUNCTION and one
-- CREATE FUNCTION, catalog changes only. No row is rewritten and no
-- counter is recomputed: no release let a review key start a claim (v0.8.0
-- is the first), so no chain whose first version a review key wrote exists
-- before this migration, and every other key's account is the one it was.
------------------------------------------------------------------------------

SET LOCAL lock_timeout = '5s';

CREATE FUNCTION oml_key_account(t uuid, k uuid) RETURNS uuid
    LANGUAGE sql STABLE AS $$
    SELECT CASE
             WHEN a.user_root_id IS NULL AND a.credential = 'session'
                  AND a.name = a.session_for || '-review'
             THEN (SELECT w.id FROM workspaces w
                    WHERE w.tenant_id = t AND w.user_root AND w.name = a.session_for)
             ELSE a.user_root_id
           END
      FROM api_keys a
     WHERE a.tenant_id = t AND a.id = k
$$;

CREATE OR REPLACE FUNCTION oml_chain_account(t uuid, w uuid, ck text) RETURNS uuid
    LANGUAGE sql STABLE AS $$
    SELECT oml_key_account(t, first.author_key_id)
      FROM (SELECT m.author_key_id
              FROM memories m
             WHERE m.tenant_id = t AND m.workspace_id = w AND m.claim_key = ck
               AND m.version = 1
             ORDER BY m.created_at
             LIMIT 1) first
$$;

CREATE OR REPLACE FUNCTION oml_account_source(t uuid)
    RETURNS TABLE (user_root uuid, memories bigint, text_bytes bigint, keys bigint,
                   connections bigint)
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
