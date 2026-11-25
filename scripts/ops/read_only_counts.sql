-- The read-only production counts ADR-0097 d27 item 1 asks for before the
-- v0.8.6 upgrade (ADR-0097, Consequences, "Production migration"). One row
-- per tenant and count; it changes nothing: the whole script runs in one
-- READ ONLY transaction, which PostgreSQL refuses to write in, and ends in a
-- ROLLBACK. Run it with scripts/ops/read_only_counts.sh, which says how.
--
-- It runs as the database owner role, which is NOSUPERUSER NOBYPASSRLS and
-- under FORCE ROW LEVEL SECURITY reads one tenant at a time, as the serving
-- role does (ADR-0034). So it first lists the tenants under
-- oml.startup_check, the one setting the tenants table's
-- lead_startup_enumeration policy admits a list under (migration 0003), and
-- then, for each tenant, clears that setting, sets oml.tenant_id and runs
-- the counts: psql's \gexec runs each column of each row the listing
-- returns as its own statement, with no function and nothing created. SET
-- LOCAL is set_config(name, value, true): it lasts to the transaction's end.
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL oml.startup_check = 'on';

\echo 'count,tenant,value'

SELECT format('SET LOCAL oml.startup_check = %L', ''),
       format('SET LOCAL oml.tenant_id = %L', id),
       $counts$
-- The counts, for the tenant oml.tenant_id names.
-- key_person: the person a key acts for, as the user root's id: a user
-- key's root, an accepted link's root, or a session key's user's root.
-- node_root: the user root a node lies in, the root itself included; none
-- for a shared node. Common table expressions, since a READ ONLY
-- transaction creates nothing, a temporary view included.
WITH key_person AS (
  SELECT k.tenant_id, k.id AS key_id, k.role, k.used_by, k.credential, k.revoked_at,
         COALESCE(k.user_root_id, k.linked_user_id, s.id) AS person_root
  FROM api_keys k
  LEFT JOIN workspaces s
    ON k.session_for IS NOT NULL AND s.tenant_id = k.tenant_id AND s.user_root
   AND s.name = k.session_for
), node_root AS (
  SELECT w.tenant_id, w.id AS node_id, r.id AS root_id
  FROM workspaces w
  JOIN workspaces r ON r.tenant_id = w.tenant_id AND r.user_root AND w.path <@ r.path
), tenant AS (
  SELECT id, COALESCE(name::text, id::text) AS name FROM tenants
)
-- 1. Live admin keys linked to no person, session keys aside.
SELECT 'unlinked_admin_keys' AS count, t.name AS tenant, count(kp.key_id) AS value
FROM tenant t
LEFT JOIN key_person kp
  ON kp.tenant_id = t.id AND kp.role = 'admin' AND kp.revoked_at IS NULL
 AND kp.credential <> 'session' AND kp.person_root IS NULL
GROUP BY t.name
UNION ALL
-- 2. Live admin keys used by a service.
SELECT 'service_admin_keys', t.name, count(k.id)
FROM tenant t
LEFT JOIN api_keys k
  ON k.tenant_id = t.id AND k.role = 'admin' AND k.used_by = 'service' AND k.revoked_at IS NULL
GROUP BY t.name
UNION ALL
-- 3. Grants of live keys on a node inside a user root other than the
--    key's own person's: capacity grants onto <user>-review or
--    <user>-curate in another person's root among them.
SELECT 'cross_root_grants', t.name, count(g.id)
FROM tenant t
LEFT JOIN (grants g
  JOIN key_person kp ON kp.tenant_id = g.tenant_id AND kp.key_id = g.key_id
                             AND kp.revoked_at IS NULL
  JOIN node_root nr ON nr.tenant_id = g.tenant_id AND nr.node_id = g.workspace_node_id)
  ON g.tenant_id = t.id AND kp.person_root IS DISTINCT FROM nr.root_id
GROUP BY t.name
UNION ALL
-- 4. Live user keys an admin key issued for a person other than the
--    admin's own, session keys aside.
SELECT 'admin_minted_user_keys', t.name, count(k.id)
FROM tenant t
LEFT JOIN (api_keys k
  JOIN key_person issuer ON issuer.tenant_id = k.tenant_id AND issuer.key_id = k.issued_by
                                 AND issuer.role = 'admin')
  ON k.tenant_id = t.id AND k.user_root_id IS NOT NULL AND k.revoked_at IS NULL
 AND k.credential <> 'session' AND issuer.person_root IS DISTINCT FROM k.user_root_id
GROUP BY t.name
UNION ALL
-- 5. Connect codes an admin key minted for a key in a person's root.
SELECT 'admin_minted_connect_codes', t.name, count(j.seq)
FROM tenant t
LEFT JOIN (journal j
  JOIN api_keys minter ON minter.tenant_id = j.tenant_id
                      AND minter.id = (j.payload->>'issued_by_key_id')::uuid AND minter.role = 'admin'
  JOIN key_person target ON target.tenant_id = j.tenant_id
                                AND target.key_id = (j.payload->>'key_id')::uuid
                                AND target.person_root IS NOT NULL)
  ON j.tenant_id = t.id AND j.event_type = 'connection.code_issued'
GROUP BY t.name
UNION ALL
-- 6. Open admin investigations.
SELECT 'open_investigations', t.name, count(i.id)
FROM tenant t
LEFT JOIN admin_investigations i ON i.tenant_id = t.id AND i.closed_at IS NULL
GROUP BY t.name
UNION ALL
-- 7. Whether the tenant ever had two admin persons: the distinct users an
--    admin allowlist entry ever named (allowed, changed or signed up as
--    admin), from the journal; 2 or more means it did (sole_admin, ADR-0097
--    Q2).
SELECT 'admin_persons_ever', t.name, count(DISTINCT j.payload->>'user')
FROM tenant t
LEFT JOIN journal j
  ON j.tenant_id = t.id
 AND j.event_type IN ('web.email_allowed', 'web.email_changed', 'web.signed_up')
 AND (j.payload->>'admin')::boolean
GROUP BY t.name
ORDER BY 1, 2
$counts$
FROM tenants
ORDER BY id
\gexec

ROLLBACK;
