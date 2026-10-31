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
(`docs/engineering/spikes/spike-3-rls.md` at `a7a5d08`). Three force this record:

- `FORCE` filters the owner's table reads in every shape probed, but not its view of planner
  statistics: set to tenant A, the owner read tenant B's lexemes in `pg_stats` for the expression
  index `memories_claim_fts` (S3.1, S3.4). Nor its DDL: it lifted `FORCE` with `ALTER TABLE …
  NO FORCE ROW LEVEL SECURITY`.
- The split is not built. `oml-server-lead` connects one `PgStore` to `postgres.url`, runs
  `migrate()` on it and serves over the same pool (`crates/bins/oml-server-lead/src/main.rs`); its
  `Config` has no `postgres.owner_url`, the key [configuration](../operations/configuration.md)
  declares.
- sqlx's migrator creates `_sqlx_migrations` with no row-level security and no policy. It holds no
  tenant data and no `tenant_id`, and ADR-0007 d2 exempts no table.

The maintainer decided on 2026-10-31 what d1 and d6 state, and that the split is built; d2–d5 and
d7–d11 are what building it takes. Decided, not built.

## Decision

- **d1 — `FORCE` fences table reads; the split fences `pg_stats` and DDL.** `FORCE` stays on every
  table but `_sqlx_migrations` (d6) and is held to what S3.1 showed: the owner's table reads are
  filtered. The owner's `pg_stats` view and DDL are kept from workloads by d2–d5, since no
  request is served as the owner. Not closed here: EXPLAIN's row estimate, which follows the same
  statistics for every role the policy binds (S3.4's reading of PostgreSQL's source, unmeasured
  for the serving role until `rls-spike` runs), so no split closes it; it stays the spike's open
  finding against ADR-0007 d4 and security.md's STRIDE row, ADR amendment needed, awaiting the
  maintainer (d10). Nor `pg_class.reltuples`, an S3.4 observation for M6.

- **d2 — Two roles: an owner that owns everything, a serving role that owns nothing.** The operator
  names them; this repository's payloads call them `oml_owner` and `oml_server`, the name
  [postgres-schema](../data/postgres-schema.md) already gives the server's role. Each holds no
  attribute but `LOGIN`: `rolsuper`, `rolcreatedb`, `rolcreaterole`, `rolreplication` and
  `rolbypassrls` are all false. The **owner** logs in through `postgres.owner_url`, owns the
  database, everything in `public` and `_sqlx_migrations`, and runs every migration. The **serving
  role** logs in through `postgres.url`; it owns nothing, is a member of no role (no
  `pg_auth_members` row has it as `member`), has no `CREATE` on the database or on `public`, and
  holds `CONNECT`, `USAGE` on `public` and d3's table privileges only. The server reads no role
  name from configuration: each is the `current_user` of its URL's login.

- **d3 — The serving role holds only the DML its statements use; a new table is read-only to it
  until its migration names its writes.** The migration that creates a table grants, in the same
  file, what the adapter's statements on it use: `SELECT` and `INSERT` alone on `journal`
  (ADR-0002 d4, and C3's reason its querying role must not own it); `DELETE` only where a
  statement deletes, as on `review_queue`. `TRUNCATE`, `REFERENCES` and `TRIGGER` are never
  granted. The grantee is the session setting `oml.serving_role`, which the lead sets on every
  owner connection to the serving login (d4); a granting migration without it fails at
  `current_setting` with `42704`, so nothing is granted to a guessed role. The split's first
  migration sets the owner's default privileges in `public` to `SELECT` on tables and `USAGE` on
  sequences for that role, nothing else: a forgotten write grant fails as `42501` in the
  integration tier, never as a writable table. The per-table matrix is
  declared on postgres-schema.md beside the migrations, under its sync rule.

- **d4 — The lead migrates under the owner URL, closes that pool, then serves under the serving
  URL.** `Config` gains `postgres.owner_url` (`OML_POSTGRES__OWNER_URL`), required. Boot: both
  pools; d5's first checks; `migrate()` on the owner pool, `oml.serving_role` set; d5's second
  checks; the owner pool closed and both role names logged; then, as today, keyring, fork check,
  enrollment token and listener, over the serving pool. No request is served while an owner
  connection is open, and the lead migrates until [deployment](../operations/deployment.md)'s
  bootstrap workload exists. The cost: the owner credential sits in the lead's file or
  environment for the process's lifetime, so the split fences what a serving connection can issue
  (a server bug, an injection), not a compromised lead.

- **d5 — One URL is refused at startup, by name; no single-URL mode exists.** The lead exits
  non-zero with `postgres.role_split` and a reason before any listener binds: before migrating,
  when `postgres.owner_url` is unset (`owner_url_missing`); both URLs log in as one role,
  compared by login since two strings can name one role (`same_role`); the logins see different
  databases (`different_databases`); the serving role holds an attribute or a membership d2
  refuses, or can create in the database or `public` (`serving_role_privileged`); or it owns the
  database or any object in it (`serving_role_owns`), the state of every unsplit install. After
  migrating, when it holds any privilege on `_sqlx_migrations`, as `has_table_privilege` computes
  it rather than `relacl`, so a grant through `PUBLIC` or a role counts
  (`serving_role_reads_migrations`); a table other than that one is not the owner's, lacks
  `ENABLE` or `FORCE`, or has no policy (`table_unfenced`); or it cannot `SELECT` a table the
  migrations created, as when `postgres.url` names a role they did not grant
  (`serving_role_ungranted`). When built, `oml doctor`'s
  role-split check (ADR-0023 d3) reports these as failures.

- **d6 — `_sqlx_migrations` is the one table without row-level security, and only the owner
  reaches it.** ADR-0007 d2's "row-level security keyed by `tenant_id` on every Postgres table"
  gains one named exemption: sqlx's record of which migrations ran, which has no tenant data and
  no `tenant_id` to key a policy on. Its fence is privilege: the split's first migration revokes
  every privilege on it from `PUBLIC`; d3's defaults miss it, since they apply only to objects
  created later ([ALTER DEFAULT PRIVILEGES](https://www.postgresql.org/docs/17/sql-alterdefaultprivileges.html),
  fetched 2026-10-31) and the migrator reads the table before applying anything; and d5 refuses a
  serving role with any privilege on it. No other table takes this exemption without a record.

- **d7 — A single-role install splits once, by moving ownership to a new owner.** A role that is
  not a superuser, the live host's among them, keeps its URL and becomes the serving role. A
  superuser, the docker profile's `POSTGRES_USER` on every existing volume, cannot (d5): it stays
  the administrator, both roles are created, and `postgres.url` moves to the new serving role. A
  superuser runs the transfer:
    1. Stop the server; take the backup [upgrades](../operations/upgrades.md) requires as a
       superuser, since `pg_dump` errors when its role cannot bypass row security
       ([pg_dump](https://www.postgresql.org/docs/17/app-pgdump.html), fetched 2026-10-31).
    2. Create the owner, and for a superuser the serving role, under d2's rule; take from a role
       that becomes the serving role every attribute and membership d2 refuses.
    3. Move ownership in the install's database. For a role that is not a superuser, `REASSIGN
       OWNED BY <role> TO <owner>`, which also moves every database the role owns on the cluster
       ([REASSIGN OWNED](https://www.postgresql.org/docs/17/sql-reassign-owned.html), fetched
       2026-10-31), so the operator checks `pg_database` for that role first. For a superuser,
       since `REASSIGN OWNED` refuses the bootstrap one as pinned (`pg_shdepend.c`,
       `REL_17_STABLE` at `ce2797d`, fetched 2026-10-31): `ALTER DATABASE`, then `ALTER TABLE`
       per table in `public` and `ALTER FUNCTION` per function there, each `OWNER TO` the owner;
       a table's indexes, TOAST table and owned sequences follow it (`tablecmds.c`, same commit).
    4. Add `postgres.owner_url`, and for a superuser point `postgres.url` at the serving role;
       install the build carrying this record and start it; its first boot applies the split's
       migration, which grants the serving role (d3).

    The old build cannot run after step 3, so steps 1–4 are one outage. Rollback before step 4 is
    step 3 reversed; after it, the backup and [restore](../operations/restore.md).

- **d8 — The live host's entry, not run.** As the maintainer's deployment notes of 2026-10-31
  record it (not in this repository), `oml.doublegate.io` runs `oml-server.service`, server 0.2.0,
  on PostgreSQL 18; its role `oml_server` (no superuser, no `BYPASSRLS`) applied migrations
  0001–0005 on boot and owns every table of database `oml_server`. **Not run**; every superuser
  step waits for the maintainer:
    1. `systemctl stop oml-server.service`; a superuser `pg_dump` of `oml_server`.
    2. In `sudo -u postgres psql -d oml_server`, once no role `oml_owner` exists: `CREATE ROLE
       oml_owner LOGIN PASSWORD …`; d7 step 2 for `oml_server`; d7 step 3, after `pg_database`
       shows which of the host's other databases `oml_server` owns.
    3. Add `OML_POSTGRES__OWNER_URL` for `oml_owner` to `/etc/oml/oml.env`, readable by the unit's
       user; `OML_POSTGRES__URL` stays on `oml_server`.
    4. Install the cosign-verified release carrying this record; start the unit; expect both role
       names in the log and `/readyz` 200, which d5 allows only over a split database. Record the
       date and output here.

- **d9 — The checks that close it.** Every Postgres suite, the existing ones included, migrates
  through the owner login of a uniquely named role pair minted for its database, with
  `oml.serving_role` set, and runs its assertions over the serving login. CI's `cargo test
  (Postgres)` job mints it as the superuser of the pinned `pgvector/pgvector:pg17`; a suite whose
  role cannot create roles reports not run, which fails CI, as the spike's `--require-all` does:
    - the serving role reading `pg_stats` of a tenant table sees nothing of another tenant (two
      tenants seeded and analysed; the control, the owner's view, must show tenant B's lexemes);
    - the serving role cannot `ALTER` or `DROP` a table, lift `FORCE` or create a table (`42501`),
      and has no privilege on `_sqlx_migrations`;
    - a boot with the serving URL equal to the owner URL exits with `postgres.role_split`
      (`same_role`) and binds no listener, as do one with no owner URL (`owner_url_missing`) and
      one over an unsplit database (`serving_role_owns`);
    - once the listener binds, `pg_stat_activity` shows no connection of the owner;
    - a table a migration adds is readable by the serving role and refuses its `INSERT` until a
      migration grants it.

- **d10 — `rls-spike` runs with the split, and its `pg_stats` expectations move to the serving
  role.** In `testing/spikes/rls/spike_rls.py` (on `feat/m0-spikes`) the service role takes its
  privileges from the migrations through `oml.serving_role`, not the harness's own `GRANT` over
  every table. S3.1 keeps its owner checks of table reads, all `FORCE` is held to, and its
  `pg_stats` check becomes an observation; S3.4's `pg_stats` verdict is the serving role's
  (`S3.4/service/pg_stats`), with the owner's leak as its control, which must leak.
  `S3.1/owner/pg_stats` and `S3.4/owner/pg_stats` leave `RECORDED_FAILURES`. `S3.4/owner/estimate`
  stays on it and `S3.4/service/estimate` joins it, both pointing to `spike-3-rls.md#s34` and open
  against ADR-0007 d4; this record does not close them (d1). S3.2 gains the serving role's refused
  `ALTER TABLE` and `DROP TABLE`. If `S3.4/service/pg_stats` fails on pg17, d1's reading fails, and
  the finding returns to the maintainer as ADR amendment needed.

- **d11 — The build lands after iteration 3, in dependency order, each step closed by a check.**
    1. `feat/iteration-3` (the rewrite of `crates/` and `migrations/`) and `feat/m0-spikes` merge.
       Check: `rls-spike`'s first pg17 run, its result recorded on `spike-3-rls.md`.
    2. The split, in one landing: its migration, after iteration 3's last (`REVOKE ALL ON
       _sqlx_migrations FROM PUBLIC`, `REVOKE CREATE ON SCHEMA public FROM PUBLIC`, each table's
       privileges, d3's defaults), postgres-schema.md's matrix and journal sketch, d10's harness
       change; `oml-pg`'s owner connection setting `oml.serving_role`, `migrate()` on it and d5's
       checks; `oml-server-lead`'s `postgres.owner_url` and d4's boot order; d9's suites; a
       Postgres init script in `docker-compose.yml` creating both roles, both URLs on `oml-server`;
       `scripts/poc-demo.sh` minting a uniquely named role pair per run, as the harness does; CI's
       demo job; README's "Running the PoC", needing a role that can create roles, both URLs on
       its commands; the Dockerfile's migrate-on-boot comment; a second Secret key in the proposed
       ADR-0033's chart if it has landed. Check: d9's tests, `rls-spike` green with
       `RECORDED_FAILURES` holding only the two estimate checks, and CI's `cargo test (Postgres)`,
       `demo and quickstart` and `docker profile` jobs green.
    3. A release, then d8. Check: FR-44's exit criterion.

## Consequences

- **FR-44 opens** on the [roadmap](../product/roadmap.md), marked proposed; it closes when d9's
  tests and `rls-spike`'s serving-role `pg_stats` and DDL checks pass in CI on pg17 and d8 has
  run. The estimate checks stay open (d1, d10).
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
  install, the development rig's included (d5); a demo role that can create roles (d11).
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
| **Fixed role names in the migrations** | Roles are cluster-wide: the maintainer's host runs the live database beside throwaway test databases on one cluster, and fixed names would give them all one serving role. |
| **Row-level security on `_sqlx_migrations`** | No `tenant_id` to key a policy on; a policy keyed on the role restates the privilege fence, and under `FORCE` binds the migrator's own reads. |
| **A migrate-only invocation, so the serving process never holds the owner URL** | That is the bootstrap workload [deployment](../operations/deployment.md) designs, with the standalone migrate verb of FR-28; it changes every payload's upgrade step and is its own change, which removes d4's cost. |
| **Keeping the old role as owner and adding a new serving role** | No ownership moves, but the credential that has sat in the serving process all along keeps ownership, and `postgres.url` changes; d7 hands ownership to a fresh credential. |
| **Recreating a docker volume (`docker compose down -v`) instead of moving its ownership** | It discards the install's memories and journal; d7's superuser case keeps them for one outage. |
