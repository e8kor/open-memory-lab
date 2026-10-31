<!--
id: adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing
tier: engineering
status: proposed
answers: what FORCE ROW LEVEL SECURITY fences and what the owner/serving role split fences instead; the two Postgres roles, their privileges and the default privileges of later tables; how the lead migrates under postgres.owner_url and serves under postgres.url; why one URL is refused at startup; why _sqlx_migrations is the one table without row-level security; how a single-role install, the live host among them, is split; the checks and the CI change that close it; the build order after iteration 3
-->

# ADR-0034 — The lead migrates as an owner role and serves as a role that owns nothing; `_sqlx_migrations` is the one table without row-level security

**Status:** proposed · 2026-10-31

**Amends:** ADR-0007 (d2) — names `_sqlx_migrations` as the one table without row-level security (d6); the reading of `FORCE` in d1 amends nothing

## Context

C6 ([commitments](../product/commitments.md)) rests tenant isolation on three commitments that
[security](../architecture/security.md) declares: `FORCE ROW LEVEL SECURITY` on every table, an
owner/migration-role split, and `SET LOCAL`. M0 spike 3 ran S3.1–S3.4 of the
[critical path](../product/critical-path.md#spike-3-rls-hardening) on 2026-10-31, on the host's
PostgreSQL 18.6 as the table owner; its findings are on branch `feat/m0-spikes`
(`docs/engineering/spikes/spike-3-rls.md` at `6ccd2b6`). Three force this record:

- `FORCE` filters the owner's table reads in every shape probed, but not its view of planner
  statistics: set to tenant A, the owner read tenant B's lexemes in `pg_stats` for the expression
  index `memories_claim_fts` (S3.1, S3.4). Nor its DDL: it lifted `FORCE` with `ALTER TABLE …
  NO FORCE ROW LEVEL SECURITY`.
- The split is not built. `oml-server-lead` connects one `PgStore` to `postgres.url`, runs
  `migrate()` on it and serves over the same pool (`crates/bins/oml-server-lead/src/main.rs`); its
  `Config` has no `postgres.owner_url`, the key [configuration](../operations/configuration.md)
  declares. A v0.2.0 lead left its one role owning all 12 tables of a throwaway database.
- sqlx's migrator creates `_sqlx_migrations` with no row-level security and no policy. It holds no
  tenant data and no `tenant_id`, and ADR-0007 d2 exempts no table.

The maintainer decided on 2026-10-31 what d1 and d6 state, and that the split is built; d2–d5 and
d7–d11 are what building it takes. Decided, not built.

## Decision

- **d1 — `FORCE` fences table reads; the split fences statistics and DDL.** `FORCE` stays on every
  table but `_sqlx_migrations` (d6) and is held to what S3.1 showed: the owner's table reads are
  filtered. The owner's `pg_stats` view and DDL are kept from workloads by d2–d5, since no
  request is served as the owner. ADR-0007 d2 needs no amendment for this; security.md's wording
  changes at acceptance (Consequences). Not closed here: catalog statistics outside `pg_stats`,
  such as row estimates and `pg_class.reltuples`, which S3.4 sent to M6's security pass.

- **d2 — Two roles: an owner that owns everything, a serving role that owns nothing.** The operator
  names them; this repository's payloads call them `oml_owner` and `oml_server`, the name
  [postgres-schema](../data/postgres-schema.md) already gives the server's role. Neither is a
  superuser or holds `BYPASSRLS`, `CREATEROLE` or `CREATEDB`. The **owner** logs in through
  `postgres.owner_url`, owns the database, everything in `public` and `_sqlx_migrations`, and runs
  every migration. The **serving role** logs in through `postgres.url`; it owns nothing, is not a
  member of the owner, has no `CREATE` on the database or on `public`, and holds `CONNECT`, `USAGE`
  on `public` and d3's table privileges only. The server reads no role name from configuration:
  each is the `current_user` of its URL's login.

- **d3 — The serving role holds only the DML its statements use; a new table is read-only to it
  until its migration names its writes.** The migration that creates a table grants, in the same
  file, what the adapter's statements on it use: `SELECT` and `INSERT` alone on `journal`
  (ADR-0002 d4, and C3's reason its querying role must not own it); `DELETE` only where a
  statement deletes, as on `review_queue`. `TRUNCATE`, `REFERENCES` and `TRIGGER` are never
  granted. The grantee is the session setting `oml.serving_role`, which the lead sets on every
  owner connection to the serving login (d4); a granting migration without it fails at
  `current_setting` with `42704`, as S3.3 recorded for an unset setting, so nothing is granted to
  a guessed role. The split's first migration sets the owner's default privileges in `public` to
  `SELECT` on tables and `USAGE` on sequences for that role, nothing else: a forgotten write grant
  fails as `42501` in the integration tier, never as a writable table. The per-table matrix is
  declared on postgres-schema.md beside the migrations, under its sync rule.

- **d4 — The lead migrates under the owner URL, closes that pool, then serves under the serving
  URL.** `Config` gains `postgres.owner_url` (`OML_POSTGRES__OWNER_URL`), required. Boot: both
  pools; d5's first checks; `migrate()` on the owner pool, `oml.serving_role` set; d5's second
  checks; the owner pool closed and both role names logged; then, as today, keyring, fork check,
  enrollment token and listener, over the serving pool. No request is served while an owner
  connection is open. This is the migration step [deployment](../operations/deployment.md) gives
  the bootstrap workload, which is not built; the lead performs it until that workload exists. The
  cost: the owner credential sits in the lead's file or environment for the process's lifetime,
  so a compromised lead host or process yields it. The split fences what a serving connection can
  issue (a server bug, an injection), not a compromised lead.

- **d5 — One URL is refused at startup, by name; no single-URL mode exists.** The lead exits
  non-zero with `postgres.role_split` and a reason before any listener binds: before migrating,
  when `postgres.owner_url` is unset (`owner_url_missing`); both URLs log in as one role,
  compared by login since two strings can name one role (`same_role`); the logins see different
  databases (`different_databases`); the serving role is a superuser, holds `BYPASSRLS`,
  `CREATEROLE` or `CREATEDB`, can create in the database or `public`, or is a member of the owner
  (`serving_role_privileged`); or it owns the
  database or any object in it (`serving_role_owns`), the state of every unsplit install. After
  migrating, when it holds any privilege on `_sqlx_migrations` (`serving_role_reads_migrations`);
  a table other than that one is not the owner's, lacks `ENABLE` or `FORCE`, or has no policy
  (`table_unfenced`); or it cannot `SELECT` a table the migrations created, as when `postgres.url`
  names a role they did not grant (`serving_role_ungranted`). The reason is the spike's: in a
  single-role install the serving connection is the owner, which reads other tenants' words in
  `pg_stats` and can lift `FORCE`; and C6's split binds in all three profiles
  ([ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md), Consequences), the development rig
  included. When built, `oml doctor`'s role-split check (ADR-0023 d3) reports these as failures.

- **d6 — `_sqlx_migrations` is the one table without row-level security, and only the owner
  reaches it.** ADR-0007 d2's "row-level security keyed by `tenant_id` on every Postgres table"
  gains one named exemption: the table in which sqlx's migrator records which migrations ran, with
  checksums and times. It has no tenant data and no `tenant_id` to key a policy on. Its fence is
  privilege: the split's first migration revokes every privilege on it from `PUBLIC`; d3's
  defaults miss it, since they apply only to objects created later
  ([ALTER DEFAULT PRIVILEGES](https://www.postgresql.org/docs/17/sql-alterdefaultprivileges.html),
  fetched 2026-10-31) and the migrator reads the table before applying anything; and d5 refuses a
  serving role with any privilege on it. No other table takes this exemption without a record.

- **d7 — A single-role install splits once, by moving ownership off the role it serves as.** That
  role keeps its URL and becomes the serving role; a new owner takes what it owns. Changing an
  owner needs a superuser, or the current owner able to `SET ROLE` to a new owner holding `CREATE`
  on the schema ([ALTER TABLE](https://www.postgresql.org/docs/17/sql-altertable.html), fetched
  2026-10-31); that membership is one d5 refuses, so a superuser runs the transfer:
    1. Stop the server; take the backup [upgrades](../operations/upgrades.md) requires, as a
       superuser: `pg_dump` sets `row_security` off and errors when its role cannot bypass row
       security ([pg_dump](https://www.postgresql.org/docs/17/app-pgdump.html), fetched
       2026-10-31), and `FORCE` leaves neither role able to.
    2. Create the owner with d2's attributes; remove `CREATEDB` and `CREATEROLE` from the role.
    3. `ALTER DATABASE … OWNER TO` the owner; list what the role owns (`pg_class`, `pg_proc`,
       `pg_type` by owner) and issue one `ALTER … OWNER TO` per object, each partition separately
       because `OWNER` never recurses to descendant tables (same page), until the list is empty.
    4. Add `postgres.owner_url`, install the build carrying this record and start it; its first
       boot applies the split's migration, which grants the serving role (d3).

    Before the split, the new build refuses with `owner_url_missing` on the old configuration,
    `same_role` with the owner URL naming the old role, and `serving_role_owns` while ownership is
    unmoved. The old build cannot run after step 3, so steps 1–4 are one outage. Rollback before
    step 4 is step 3 reversed; after it, the backup and [restore](../operations/restore.md).

- **d8 — The live host's entry, not run.** As the maintainer's deployment notes of 2026-10-31
  record it (not in this repository), `oml.doublegate.io` runs `oml-server.service`, server 0.2.0,
  on PostgreSQL 18; its role `oml_server` (no superuser, no `BYPASSRLS`) applied migrations
  0001–0005 on boot and owns every table of database `oml_server`. **Not run**; every superuser
  step waits for the maintainer:
    1. `systemctl stop oml-server.service`; a superuser `pg_dump` of `oml_server`.
    2. In `sudo -u postgres psql -d oml_server`, once no role `oml_owner` exists: `CREATE ROLE
       oml_owner LOGIN PASSWORD … NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE`; `ALTER ROLE
       oml_server NOCREATEDB NOCREATEROLE`; `ALTER DATABASE oml_server OWNER TO oml_owner`; d7
       step 3 until `oml_server` owns nothing.
    3. Add `OML_POSTGRES__OWNER_URL` for `oml_owner` to `/etc/oml/oml.env`, readable by the unit's
       user (d4's cost here); `OML_POSTGRES__URL` stays on `oml_server`.
    4. Install the cosign-verified release carrying this record; start the unit; expect both role
       names in the log and `/readyz` 200; confirm with the spike's catalog query that every table
       but `_sqlx_migrations` is `oml_owner`'s and `oml_server` holds no privilege on that one.
       Record the date and output here.

- **d9 — The checks that close it.** Rust integration tests on the pinned
  `pgvector/pgvector:pg17` in CI's `cargo test (Postgres)` job, whose superuser mints a uniquely
  named role pair per test database; a test whose role cannot create roles reports not run, and
  CI fails on not run, as the spike's `--require-all` does:
    - the serving role reading `pg_stats` of a tenant table sees nothing of another tenant (two
      tenants seeded and analysed; the control, the owner's view, must show tenant B's lexemes);
    - the serving role cannot `ALTER` or `DROP` a table, lift `FORCE` or create a table (`42501`);
    - the serving role has no privilege on `_sqlx_migrations`;
    - a boot with the serving URL equal to the owner URL exits with `postgres.role_split`
      (`same_role`) and binds no listener, as do one with no owner URL (`owner_url_missing`) and
      one over an unsplit database (`serving_role_owns`);
    - once the listener binds, `pg_stat_activity` shows no connection of the owner;
    - a table a migration adds is readable by the serving role and refuses its `INSERT` until a
      migration grants it.

- **d10 — `rls-spike` runs with the split, and its owner expectations move to the serving
  role.** In `testing/spikes/rls/spike_rls.py` (on `feat/m0-spikes`) the service role takes its
  privileges from the migrations through `oml.serving_role`, not the harness's own `GRANT` over
  every table. S3.1 keeps its owner checks of table reads, all `FORCE` is held to, and its
  `pg_stats` check becomes an observation. S3.4's verdict is the serving role's,
  `S3.4/service/pg_stats` included, with the owner's `pg_stats` leak as its control, which must leak.
  `S3.1/owner/pg_stats` and `S3.4/owner/pg_stats` leave `RECORDED_FAILURES`, which is then empty,
  and S3.2 gains the serving role's refused `ALTER TABLE` and `DROP TABLE`. If the serving role's
  `pg_stats` check fails on pg17, d1's reading fails, and the finding returns to the maintainer as
  ADR amendment needed.

- **d11 — The build lands after iteration 3, in dependency order, each step closed by a check.**
    1. `feat/iteration-3` (the rewrite of `crates/` and `migrations/`) and `feat/m0-spikes` merge.
       Check: `rls-spike` green on `main`, findings as recorded.
    2. The split's migration, numbered after iteration 3's last: `REVOKE ALL ON _sqlx_migrations
       FROM PUBLIC`, `REVOKE CREATE ON SCHEMA public FROM PUBLIC`, each table's privileges, d3's
       defaults; with it, postgres-schema.md's matrix and journal sketch and d10's harness change.
       Check: `rls-spike` green, `RECORDED_FAILURES` empty.
    3. `oml-pg`: an owner connection that sets `oml.serving_role`, `migrate()` on it, and d5's
       checks returning the reason. Check: d9's catalog tests.
    4. `oml-server-lead`: `postgres.owner_url` and d4's boot order. Check: d9's boot tests.
    5. Payloads: a Postgres init script in `docker-compose.yml` creating both roles, both URLs on
       `oml-server`; `scripts/poc-demo.sh`, `scripts/quickstart.sh`, CI's demo job, the
       Dockerfile's migrate-on-boot comment; a second Secret key in the proposed ADR-0033's chart
       if it has landed. Check: CI's demo and quickstart jobs green.
    6. A release, then d8. Check: FR-44's exit criterion.

## Consequences

- **FR-44 opens** on the [roadmap](../product/roadmap.md), marked proposed; it closes when d9 and
  `rls-spike` with the split pass in CI on pg17 and d8 has run.
- **On acceptance, and not before:** ADR-0007's Status line gains `amended by ADR-0034`, its d2
  text kept as history ([status lifecycle](index.md#status-lifecycle)). security.md's tenancy
  paragraph is reworded: `FORCE` binds an owner's table reads, not its `pg_stats` view or DDL; the
  split keeps those from every workload; the owner role only migrates, through the lead's boot
  until a bootstrap workload exists; "no table is exempted for convenience" names
  `_sqlx_migrations`, fenced by privilege. The same acceptance edits configuration.md (the lead
  reads `postgres.owner_url`), deployment.md's lead row, who migrates on upgrades.md and
  postgres-schema.md, the critical path's S3.1 row and the register's D6 cell.
- **Costs:** the owner credential in the lead's configuration (d4); one outage per single-role
  install (d7); every new table's migration names its writes (d3); a second credential in every
  install, the development rig's included (d5).
- **Unverified until `rls-spike` runs on pg17:** that the serving role's `pg_stats` shows nothing
  of another tenant (d10).
- **No surface change:** no HTTP operation, error code or event type, so `info.version` stays
  `0.2.0-design`; `postgres.role_split` is a startup error outside the
  [error taxonomy](../api/errors.md). Other workloads' roles come with those workloads.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **One role, `FORCE` as the whole fence** | S3.1: `FORCE` filters neither the owner's `pg_stats` nor its DDL, so the serving connection of such an install can do both. |
| **A development mode accepting one URL, with a warning on `/readyz` or in `oml doctor`** | ADR-0023 binds C6's split in all three profiles and keeps the binaries profile-blind, with no `--profile` flag (d8); a mode relaxing C6 is that flag. A warning leaves the leak on where installs start, to save one role in an init script. |
| **Default privileges granting every DML verb** | A table that must not change stays writable until a migration remembers to revoke; read-only by default makes a forgotten grant an error. |
| **Fixed role names in the migrations** | Roles are cluster-wide: the maintainer's host runs the live database beside throwaway test databases on one cluster, and fixed names would give them all one serving role. |
| **Row-level security on `_sqlx_migrations`** | No `tenant_id` to key a policy on; a policy keyed on the role restates the privilege fence, and under `FORCE` binds the migrator's own reads. |
| **A migrate-only invocation, so the serving process never holds the owner URL** | That is the bootstrap workload [deployment](../operations/deployment.md) designs, with the standalone migrate verb of FR-28; it changes every payload's upgrade step and is its own change, which removes d4's cost. |
| **Keeping the old role as owner and adding a new serving role** | No ownership moves, but the credential that has sat in the serving process all along keeps ownership, and `postgres.url` changes; d7 hands ownership to a fresh credential. |
