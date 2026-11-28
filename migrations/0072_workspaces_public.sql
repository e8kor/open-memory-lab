-- 0072_workspaces_public — one flag, public, on a workspace node, off by
-- default; set, every reading key of the tenant reads the node's approved
-- items without a grant (ADR-0098 d1, d10).
--
-- workspaces.public is written by workspace.public_set alone, in the commit
-- where the admin act that sets or clears it takes effect: a clear's own
-- commit, a set's under sole_admin, and otherwise the commit of the second
-- admin person's confirmation (d6, d9). workspaces.public_act_id names that
-- act, set and clear alike, so the workspace read serves since when and
-- whether it was sole_admin from the act's row. Every existing workspace
-- starts not public, so the upgrade changes no key's reach and no recall.
-- The row is derived: the journal's workspace.public_set events rebuild it
-- (ADR-0090 d18).
--
-- workspaces_public is a partial index on the tenant's public nodes: the
-- one read effective access adds per request (the bottleneck register's
-- "Effective access" row). The table's row security covers both columns.
--
-- What it costs a live database: one column with a constant default, which
-- PostgreSQL 11 and later add without rewriting a row, and one nullable
-- column with no default, a catalog change; and one CREATE INDEX whose
-- predicate no existing row meets, built by one scan of workspaces, a table
-- of tens to thousands of rows. ALTER TABLE takes ACCESS EXCLUSIVE on workspaces until the
-- commit, so every read that resolves a workspace waits for the migration.
-- A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE workspaces ADD COLUMN public boolean NOT NULL DEFAULT false;
ALTER TABLE workspaces ADD COLUMN public_act_id uuid;
COMMENT ON COLUMN workspaces.public IS
    'Whether every reading key of the tenant reads the node''s approved items without a grant; '
    'written by workspace.public_set alone (ADR-0098 d1, d6).';
COMMENT ON COLUMN workspaces.public_act_id IS
    'The admin act whose effect last set or cleared public (ADR-0098 d9).';

CREATE INDEX workspaces_public ON workspaces (tenant_id) WHERE public;
