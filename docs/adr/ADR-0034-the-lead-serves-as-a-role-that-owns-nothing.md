<!--
id: adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing
tier: engineering
status: proposed
answers: what FORCE ROW LEVEL SECURITY fences and what the owner/serving role split fences instead; the two Postgres roles and their privileges; how the lead migrates under postgres.owner_url and serves under postgres.url; why one URL is refused at startup; why _sqlx_migrations is the one table without row-level security; how a single-role install, the live host among them, is split; the per-tenant FTS index, how a read reaches its tenant's partition, who creates a partition and which channels stay open; foreign keys that carry tenant_id; the checks and the CI change that close it; the build order after iteration 3
-->

# ADR-0034 — The lead migrates as an owner role and serves as a role that owns nothing; `_sqlx_migrations` is the one table without row-level security

**Status:** proposed · 2026-10-31

**Amends:** ADR-0007 (d2), naming `_sqlx_migrations` as the one table without row-level security
(d6); no other decision here amends a record

## Context

M0 spike 3 (`docs/engineering/spikes/spike-3-rls.md`, on `feat/m0-spikes`) tested the three
commitments on which [security](../architecture/security.md) rests C6's tenant isolation
([commitments](../product/commitments.md)): `FORCE ROW LEVEL SECURITY` on every table, an
owner/migration-role split and `SET LOCAL`. It ran the [critical
path](../product/critical-path.md#spike-3-rls-hardening)'s S3.1–S3.4 on 2026-10-31, as the table
owner on the host's PostgreSQL 18.6 (`a7a5d08`), then as an owner and a service role in CI's
`rls-spike` on the pinned 17.11 (`bb20087` and its CI log). `FORCE` filtered the owner's table reads
in every shape probed, but not its `pg_stats` view: set to tenant A, the owner read tenant B's
lexemes for `memories_claim_fts`, one GIN index over every tenant, where the service role saw 0
rows. Nor its DDL: it lifted `FORCE` by `ALTER TABLE`. The split is not built: `oml-server-lead`
migrates and serves over one `PgStore` on `postgres.url`, its `Config` lacking the
`postgres.owner_url` that [configuration](../operations/configuration.md) declares. sqlx's migrator
creates `_sqlx_migrations` with no row-level security, and ADR-0007 d2 exempts no table.

The maintainer decided on 2026-10-31 what d1 and d6 state and that the split is built, and after the
pg17 run that the FTS index becomes per tenant (d11) and keys into tenant tables carry `tenant_id`
(d12), as ADR-0007 d4 and security.md's STRIDE row already promise ("no cross-tenant index to leak
through"): implementation fixes, not amendments. Decided, not built.

## Decision

- **d1 — `FORCE` fences table reads; the split fences `pg_stats` and DDL.** `FORCE` stays on every
  table but `_sqlx_migrations` (d6), held to what S3.1 showed: it filters the owner's table reads.
  d2–d5 keep the owner's `pg_stats` view and DDL from workloads. No split closes EXPLAIN's row
  estimate, which reached both roles on 17.11; FR-45 holds it, with `reltuples`.

- **d2 — Two roles: an owner that owns everything, a serving role that owns nothing.** The operator
  names them; this repository's payloads call them `oml_owner` and `oml_server`. Each holds no
  attribute but `LOGIN` (`rolsuper`, `rolcreatedb`, `rolcreaterole`, `rolreplication` and
  `rolbypassrls` false). The **owner** logs in through `postgres.owner_url`, owns the database,
  everything in `public` and `_sqlx_migrations`, and runs every migration. The **serving role** logs
  in through `postgres.url`, owns nothing, is a member of no role, has no `CREATE` on the database
  or `public`, and holds beyond `PUBLIC`'s only `CONNECT`, `USAGE` on `public` and d3's privileges.
  The server reads no role name from configuration: each is its URL's `current_user`.

- **d3 — The serving role holds only the DML its statements use; a new table refuses it until its
  migration grants them.** Each table's migration grants what the adapter's statements on it use,
  with `USAGE` on a sequence an `INSERT` draws on, as `outbox`'s: `SELECT` and `INSERT` alone on
  `journal` (ADR-0002 d4); `DELETE` only where a statement deletes, as on `review_queue`; never
  `TRUNCATE`, `REFERENCES`, `TRIGGER` or `MAINTAIN`, whole-table operations that "are not subject to
  row security" ([row security](https://www.postgresql.org/docs/17/ddl-rowsecurity.html),
  [privileges](https://www.postgresql.org/docs/17/ddl-priv.html), fetched 2026-10-31). The grantee
  is the setting `oml.serving_role`, which the lead sets on every owner connection; a granting
  migration without it fails at `current_setting` (`42704`). postgres-schema.md declares the
  per-table matrix, under its sync rule.

- **d4 — The lead migrates under the owner URL, closes that pool, then serves under the serving
  URL.** `Config` gains `postgres.owner_url` (`OML_POSTGRES__OWNER_URL`), required. Boot: both
  pools; d5's first checks; `migrate()` on the owner pool, `oml.serving_role` set; d5's second
  checks; the owner pool closed, both role names logged; then today's keyring, fork check,
  enrollment token and listener, over the serving pool. Afterwards the lead opens an owner
  connection only for d11's partition DDL, which it runs, as it migrates, until
  [deployment](../operations/deployment.md)'s bootstrap workload or a record of its own takes both.
  The cost: the owner credential stays in the lead's file or environment, so the split fences a
  serving connection's statements (a server bug, an injection), not a compromised lead.

- **d5 — One URL is refused at startup, by name; no single-URL mode exists.** The lead exits
  non-zero with `postgres.role_split` and a reason before any listener binds. Before migrating:
  `postgres.owner_url` is unset (`owner_url_missing`); both URLs log in as one role, compared by
  login, not URL (`same_role`); the logins see different databases (`different_databases`); the
  serving role holds an attribute or membership d2 refuses or can create in the database or `public`
  (`serving_role_privileged`), or owns the database or an object in it, as in every unsplit install
  (`serving_role_owns`). After migrating, by `has_table_privilege`, so a grant through `PUBLIC` or a
  role counts: the serving role holds `TRUNCATE`, `REFERENCES`, `TRIGGER` or `MAINTAIN` on any table
  (`serving_role_privileged`); any of those four, or `SELECT`, `INSERT`, `UPDATE` or `DELETE`, on
  `_sqlx_migrations` (`serving_role_reads_migrations`); no `SELECT` on `journal`, as when
  `postgres.url` names a role the migrations did not grant (`serving_role_ungranted`); a table but
  that one, a partition included, is not the owner's, lacks `ENABLE` or `FORCE`, or has no policy
  (`table_unfenced`); or a function in `public` is `SECURITY DEFINER` (`prosecdef`), which runs a
  serving call as the owner (`definer_function`).

- **d6 — `_sqlx_migrations` is the one table without row-level security, and only the owner reaches
  it.** ADR-0007 d2's "row-level security keyed by `tenant_id` on every Postgres table" gains one
  named exemption: sqlx's record of which migrations ran, which has no tenant data and no
  `tenant_id` to key a policy on. Its fence is privilege: the split's first migration revokes every
  privilege on it from `PUBLIC`, and d5 refuses a serving role that holds any. No other table takes
  this exemption without a record.

- **d7 — A single-role install splits once, by moving ownership to a new owner.** A role that is not
  a superuser, the live host's among them, keeps its URL and becomes the serving role; a superuser,
  the docker profile's `POSTGRES_USER`, cannot (d5), so it stays the administrator, both roles are
  created and `postgres.url` moves to the serving one. A superuser runs the split in one outage,
  since the old build cannot run once ownership moves: the backup
  [upgrades](../operations/upgrades.md) requires, taken as a superuser, since `pg_dump` errors when
  its role cannot bypass row security
  ([pg_dump](https://www.postgresql.org/docs/17/app-pgdump.html)); the roles, under d2; ownership
  moved in the install's database, by `REASSIGN OWNED BY <role> TO <owner>` for a role that is not a
  superuser, after checking `pg_database`, since it moves every database the role owns on the
  cluster ([REASSIGN OWNED](https://www.postgresql.org/docs/17/sql-reassign-owned.html)), and for a
  superuser, which that refuses as the bootstrap role (`pg_shdepend.c`, `REL_17_STABLE` at
  `ce2797d`; all three fetched 2026-10-31), by `ALTER … OWNER TO` on the database and each table and
  function in `public`, whose indexes, TOAST tables and owned sequences follow (`tablecmds.c`); then
  `postgres.owner_url` and the build carrying this record. Until that build starts, rollback
  reverses the move; after it, the backup and [restore](../operations/restore.md). The live host's
  split is **not run** and waits for the maintainer; its success is both role names in its log and
  `/readyz` 200, recorded here with date and output.

- **d8 — The checks that close it.** Every Postgres suite migrates through the owner login of a
  uniquely named role pair minted for its database, `oml.serving_role` set, and asserts over the
  serving login, the journal guard law excepted: it runs as the owner, since privileges refuse the
  serving role first (`42501`, asserted beside it). Minting takes a superuser, as CI's `cargo test
  (Postgres)` job has; a suite that cannot mint fails, as one without Postgres does:
    - the serving role sees nothing of another tenant in a tenant table's `pg_stats`, two tenants
      seeded and analysed, while the control, the owner's view, shows tenant B's lexemes;
    - the serving role cannot `ALTER`, `DROP` or create a table, lift `FORCE`, `TRUNCATE` a table,
      or `SELECT` or `INSERT` on one whose migration grants nothing (`42501`), and has no privilege
      on `_sqlx_migrations`;
    - a boot over one URL, with no owner URL, over an unsplit database or after a migration grants
      `TRUNCATE` exits with `postgres.role_split` and its reason, no listener bound;
    - once the listener binds, `pg_stat_activity` shows no owner connection outside d11's DDL.

- **d9 — `rls-spike` runs with the split, d11 and d12.** Its harness (on `feat/m0-spikes`) creates
  `_sqlx_migrations` as sqlx's migrator does before it applies the files, and the service role takes
  its grants from them through `oml.serving_role`; its checks and `RECORDED_FAILURES` change as
  d10's checks state, and `spike-3-rls.md` keeps their names.

- **d10 — The build lands after iteration 3, in dependency order, each step closed by a check.**
    1. `feat/iteration-3` and `feat/m0-spikes` merge. Check: `rls-spike`'s first pg17 run, met.
    2. The split, in one landing: its migration (d6's revoke, `REVOKE CREATE ON SCHEMA public FROM
       PUBLIC`, each table's grants), postgres-schema.md's matrix, d4, d5, d8, d9, both roles and
       URLs in `docker-compose.yml`, `scripts/poc-demo.sh`, CI's demo job, README's "Running the
       PoC" and the Dockerfile's comment, and a second Secret key and the kind job's role pair in
       the proposed ADR-0033's chart. Check: d8's tests, `rls-spike` recording the two `estimate`
       checks alone, and CI's `cargo test (Postgres)`, `demo and quickstart`, `docker profile` and
       `kubernetes chart` jobs.
    3. d12's migration. Check: under tenant A, a reference to tenant B's row fails as one to a
       random id does (`23503`, one constraint) for each key d12 rewrites, in the integration tier
       and `rls-spike`; `pg_constraint` shows every key into a table with `tenant_id` carrying it.
    4. d11's migration and tenant predicate, and postgres-schema.md's FTS sketch. Check: a tenant's
       fenced partition exists before its row commits, and a failed creation commits no tenant; the
       adapter's FTS read scans its tenant's partition alone; the serving role sees no partition's
       `pg_stats`; d5 holds over partitions; `rls-spike` as in step 2.
    5. A release, then the live host's split. Check: its output, recorded in d7.

- **d11 — The FTS index is per tenant: `memories` is list-partitioned by `tenant_id`, each partition
  with its own GIN index.** The migration rebuilds `memories` as `PARTITION BY LIST (tenant_id)`, a
  partition per tenant filled under its context, keyed `(tenant_id, id)` since unique keys "must
  include all of the partition key columns", and `memories_claim_fts` becomes a partitioned index,
  so each partition has its own
  ([partitioning](https://www.postgresql.org/docs/17/ddl-partitioning.html), fetched 2026-10-31);
  the designed `memories_fts` and `held_fts` take the same form. Every adapter statement on
  `memories` adds `tenant_id = $n`, bound to the tenant it sets in `oml.tenant_id`, so the plan
  keeps that partition alone or drops the rest as it starts (the same page); RLS stays the fence.
    - *Who creates a partition:* the owner, before the commit whose fold creates the tenant's row
      (`tenant.genesis`, in the bootstrap claim today; `tenant.created` once built). The use case
      mints the id and calls a store port that `oml-pg` serves on an owner connection opened for
      that call alone, creating the partition with `ENABLE`, `FORCE` and its parent's policy; a
      failed DDL fails the act before any commit. `memory_embeddings`' partitions (ADR-0013 d4) take
      the same path once that table is built.
    - *Stays open:* EXPLAIN's estimate (d1) and each partition's `reltuples` and bound, which give
      every role one tenant's row count and id, are FR-45's; by this record's reading of the planner
      (`REL_17_STABLE` at `f1ba2b8`, fetched 2026-10-31), a partition's GIN index serves no
      RLS-bound query, `@@` not being leakproof (S3.4), so FTS reads scan (FR-4, FR-7).

- **d12 — A foreign key into a tenant table carries `tenant_id`.** The migration rewrites the three
  keys the pg17 run caught taking another tenant's ids (`workareas.parent_id`,
  `memories.created_event_id`, `review_queue.memory_id`), and `api_keys.default_workarea_id` and
  `rotated_from`, as `(tenant_id, col)` → `(tenant_id, id)`, the form 0004 gave `grants`, adding
  each `UNIQUE (tenant_id, …)` it needs, so a cross-tenant reference is refused as one to no row is;
  on a provider-level `api_keys` row, whose `tenant_id` is NULL, "the row is not required to have a
  match" ([CREATE TABLE](https://www.postgresql.org/docs/17/sql-createtable.html), fetched
  2026-10-31). `journal.api_key_id` and `api_keys.issued_by` name a provider-level key by design, as
  when an operator key issues a tenant's first admin key ([HTTP API](../api/http-api.md)): the
  migration drops their foreign key, as 0002 dropped `review_queue.held_by_key`'s for provider-level
  keys, and each holds the id of the key its request authenticated with, which the use case has read
  and no client names.

## Consequences

- **FR-44 and FR-45 open** on the [roadmap](../product/roadmap.md), marked proposed: FR-44 for the
  split and the read log's partition DDL, FR-45 for the estimate and catalog channels d11 leaves.
- **On acceptance, and not before:** ADR-0007's Status line gains `amended by ADR-0034`, its d2 text
  kept as history ([status lifecycle](index.md#status-lifecycle)). security.md's tenancy paragraph
  says that `FORCE` binds the owner's table reads, not its `pg_stats` view or DDL, which the split
  keeps from every workload, that the owner only migrates and creates partitions, and that
  `_sqlx_migrations` is the one exemption, fenced by privilege; its STRIDE information-disclosure
  row adds FR-45's channels. configuration.md, deployment.md's lead row, upgrades.md,
  postgres-schema.md, retrieval.md's FTS paragraph, the critical path's S3.1 row and the register's
  D6 cell follow.
- **Costs** beyond those the decisions state: a superuser for the test suites and the demo (d8);
  planning that grows with tenants where a statement is not pruned at planning (d11). `read_events`
  partition creation and `DROP`, its retention act ([postgres-schema](../data/postgres-schema.md)),
  wait for the bootstrap workload or a record of their own.
- **No surface change:** no HTTP operation, error code or event type, so `info.version` stays
  `0.2.0-design`; `postgres.role_split` is a startup error outside the [error
  taxonomy](../api/errors.md).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **One role, `FORCE` as the whole fence** | S3.1: `FORCE` filters neither the owner's `pg_stats` nor its DDL, so the serving connection of such an install can do both. |
| **A development mode accepting one URL, with a warning on `/readyz` or in `oml doctor`** | ADR-0023 binds C6's split in all three profiles and keeps the binaries profile-blind, with no `--profile` flag (d8); a mode relaxing C6 is that flag. A warning leaves the leak on where installs start, to save one role in an init script. |
| **Fixed role names in the migrations** | Roles are cluster-wide: the maintainer's host runs the live database beside throwaway test databases on one cluster, and fixed names would give them all one serving role. |
| **Row-level security on `_sqlx_migrations`** | No `tenant_id` to key a policy on; a policy keyed on the role restates the privilege fence, and under `FORCE` binds the migrator's own reads. |
| **A migrate-only invocation, so the serving process never holds the owner URL** | That is the bootstrap workload [deployment](../operations/deployment.md) designs, with the standalone migrate verb of FR-28; it changes every payload's upgrade step and is its own change, which removes d4's cost. |
| **Keeping the old role as owner and adding a new serving role** | No ownership moves, but the credential that has sat in the serving process all along keeps ownership, and `postgres.url` changes; d7 hands ownership to a fresh credential. |
| **Statistics target 0 on the one FTS index, instead of partitions** | Target 0 "disables collection of statistics for that column" ([ANALYZE](https://www.postgresql.org/docs/17/sql-analyze.html), fetched 2026-10-31), which would leave every role FTS's default selectivity, unmeasured, but one index would still span tenants, which ADR-0007 d4 and the STRIDE row rule out; it stays FR-45's candidate beside the partitions. |
| **A `DEFAULT` partition for tenants without one of their own** | The tenants in it share one index again, and a later partition for one of them must first move its rows out. |
| **A `SECURITY DEFINER` function the serving role calls to create a partition** | Any serving connection could call it, an injection included; d5 refuses definer functions for that reason. d11's owner connection is reachable from the tenant-creating act alone. |
| **A leakproof `@@` wrapper, so the GIN index serves RLS-bound queries** | Only a superuser can mark a function `LEAKPROOF` (S3.4), PostgreSQL does not mark `ts_match_vq` so, and d2's roles hold no superuser; it is FR-4's and FR-7's question, with numbers. |
