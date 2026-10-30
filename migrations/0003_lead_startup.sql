-- 0003_lead_startup — the one policy oml-server-lead's startup needs beyond
-- 0001/0002. Forward-only (docs/data/postgres-schema.md "Migrations").
--
-- The startup fork check (ADR-0002 d7) verifies every tenant's stored chain
-- head against the stored signed events before the lead takes writes, and the
-- bootstrap ceremony must know whether the database is empty (deployment.md:
-- the enrollment secret exists only against an empty deployment). Both need to
-- enumerate tenant ids, and 0001's FORCE row-level security deliberately
-- admits only the one tenant named by oml.tenant_id — correct for every port
-- operation, impossible for a whole-deployment startup sweep.
--
-- This policy opens SELECT on tenants (ids and names only — no content
-- tables) to a session that has set the transaction-local marker
-- oml.startup_check = 'on'. The service role is the only role connecting in
-- the PoC profiles, so this widens nothing across trust boundaries; it only
-- lets the lead ask "which chains exist" before verifying each one under the
-- ordinary per-tenant RLS context.
CREATE POLICY lead_startup_enumeration ON tenants FOR SELECT
    USING (current_setting('oml.startup_check', true) = 'on');
