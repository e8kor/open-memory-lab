<!--
id: adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing
tier: engineering
status: proposed
answers: what FORCE ROW LEVEL SECURITY fences and what the owner/serving role split fences instead; the two Postgres roles, their privileges and the default privileges of later tables; how the lead migrates under postgres.owner_url and serves under postgres.url; why one URL is refused at startup; why _sqlx_migrations is the one table without row-level security; how a single-role install, the live host among them, is split; the per-tenant FTS index, how a read reaches its tenant's partition, who creates a partition and which channels stay open; foreign keys that carry tenant_id; the checks and the CI change that close it; the build order after iteration 3
-->

# ADR-0034 — The lead migrates as an owner role and serves as a role that owns nothing; `_sqlx_migrations` is the one table without row-level security

**Status:** proposed · 2026-10-31

**Amends:** ADR-0007 (d2) — names `_sqlx_migrations` as the one table without row-level security
(d6); the reading of `FORCE` in d1, the per-tenant FTS index (d12) and the tenant-carrying keys
(d13) amend nothing

## Context

C6 ([commitments](../product/commitments.md)) rests tenant isolation on three commitments that
[security](../architecture/security.md) declares: `FORCE ROW LEVEL SECURITY` on every table, an
owner/migration-role split, and `SET LOCAL`. M0 spike 3 ran S3.1–S3.4 of the [critical
path](../product/critical-path.md#spike-3-rls-hardening) on 2026-10-31, on the host's PostgreSQL
18.6 as the table owner (`docs/engineering/spikes/spike-3-rls.md`, branch `feat/m0-spikes` at
`a7a5d08`). Three facts force this record:

- `FORCE` filters the owner's table reads in every shape probed, but not its view of planner
  statistics: set to tenant A, the owner read tenant B's lexemes in `pg_stats` for the expression
  index `memories_claim_fts` (S3.1, S3.4). Nor its DDL: it lifted `FORCE` by `ALTER TABLE`.
- The split is not built: `oml-server-lead` migrates and serves over one `PgStore` on
  `postgres.url` (`crates/bins/oml-server-lead/src/main.rs`), and its `Config` lacks the
  `postgres.owner_url` that [configuration](../operations/configuration.md) declares.
- sqlx's migrator creates `_sqlx_migrations` with no row-level security and no policy. It holds no
  tenant data and no `tenant_id`, and ADR-0007 d2 exempts no table.

CI's `rls-spike` then ran every scenario on the pinned PostgreSQL 17.11 as an owner and a service
role (2026-10-31; the page at `bb20087` and its CI log): the service role sees 0 `pg_stats` rows;
EXPLAIN's estimate for an FTS query still tells either role whether another tenant's memories hold
a word, `memories_claim_fts` being one GIN index over every tenant; `@@` (`ts_match_vq`) is not
leakproof, so under RLS that index serves no query and every FTS read scans; three foreign keys
let a tenant store, and test for, another tenant's ids (d13); `pg_class.reltuples` spans tenants.

The maintainer decided on 2026-10-31 what d1 and d6 state and that the split is built, and after
that run that the FTS index becomes per tenant (d12) and keys into tenant tables carry `tenant_id`
(d13), as ADR-0007 d4 and security.md's STRIDE row already promise ("no cross-tenant index to leak
through"): implementation fixes, not amendments. Decided, not built.

## Decision

- **d1 — `FORCE` fences table reads; the split fences `pg_stats` and DDL.** `FORCE` stays on every
  table but `_sqlx_migrations` (d6) and is held to what S3.1 showed: the owner's table reads are
  filtered. The owner's `pg_stats` view and DDL are kept from workloads by d2–d5, since no request
  is served as the owner. No split closes EXPLAIN's row estimate, which reached both roles on
  17.11: d12 closes it for the reads the server sends, and FR-45 holds the rest with `reltuples`.

- **d2 — Two roles: an owner that owns everything, a serving role that owns nothing.** The operator
  names them; this repository's payloads call them `oml_owner` and `oml_server`. Each holds no
  attribute but `LOGIN`: `rolsuper`, `rolcreatedb`, `rolcreaterole`, `rolreplication` and
  `rolbypassrls` are all false. The **owner** logs in through `postgres.owner_url`, owns the
  database, everything in `public` and `_sqlx_migrations`, and runs every migration. The **serving
  role** logs in through `postgres.url`; it owns nothing, is a member of no role, has no `CREATE` on
  the database or on `public`, and holds, beyond what `PUBLIC` holds, `CONNECT`, `USAGE` on `public`
  and d3's table privileges. The server reads no role name from configuration: each is the
  `current_user` of its URL's login.

- **d3 — The serving role holds only the DML its statements use; a new table is read-only to it
  until its migration names its writes.** A table's migration grants, in the same file, what the
  adapter's statements on it use: `SELECT` and `INSERT` alone on `journal` (ADR-0002 d4); `DELETE`
  only where a statement deletes, as on `review_queue`; never `TRUNCATE`, `REFERENCES` or `TRIGGER`.
  The grantee is the session setting `oml.serving_role`, which the lead sets on every owner
  connection (d4); without it a granting migration fails at `current_setting` (`42704`). The split's
  first migration sets the owner's default privileges in `public` to `SELECT` on tables and `USAGE`
  on sequences for that role, so a forgotten write grant fails as `42501` in the integration tier,
  never as a writable table. postgres-schema.md declares the per-table matrix, under its sync rule.

- **d4 — The lead migrates under the owner URL, closes that pool, then serves under the serving
  URL.** `Config` gains `postgres.owner_url` (`OML_POSTGRES__OWNER_URL`), required. Boot: both
  pools; d5's first checks; `migrate()` on the owner pool, `oml.serving_role` set; d5's second
  checks; the owner pool closed and both role names logged; then, as today, keyring, fork check,
  enrollment token and listener, over the serving pool. No request is served while the owner pool
  is open; after it closes, the lead opens an owner connection only for d12's partition DDL, and it
  migrates until [deployment](../operations/deployment.md)'s bootstrap workload exists. The cost:
  the owner credential stays in the lead's file or environment, so the split fences a serving
  connection's statements (a server bug, an injection), not a compromised lead.

- **d5 — One URL is refused at startup, by name; no single-URL mode exists.** The lead exits
  non-zero with `postgres.role_split` and a reason before any listener binds: before migrating, when
  `postgres.owner_url` is unset (`owner_url_missing`); both URLs log in as one role, compared by
  login since two strings can name one role (`same_role`); the logins see different databases
  (`different_databases`); the serving role holds an attribute or a membership d2 refuses, or can
  create in the database or `public` (`serving_role_privileged`); or it owns the database or any
  object in it (`serving_role_owns`), the state of every unsplit install. After migrating, when it
  holds any privilege on `_sqlx_migrations`, as `has_table_privilege` computes it, so a grant
  through `PUBLIC` or a role counts (`serving_role_reads_migrations`); a table other than that one,
  a partition included, is not the owner's, lacks `ENABLE` or `FORCE`, or has no policy
  (`table_unfenced`); a function in `public` is `SECURITY DEFINER` (`prosecdef`), which runs a
  serving call as the owner (`definer_function`); or it cannot `SELECT` a table the migrations
  created, as when `postgres.url` names a role they did not grant (`serving_role_ungranted`).

- **d6 — `_sqlx_migrations` is the one table without row-level security, and only the owner
  reaches it.** ADR-0007 d2's "row-level security keyed by `tenant_id` on every Postgres table"
  gains one named exemption: sqlx's record of which migrations ran, which has no tenant data and
  no `tenant_id` to key a policy on. Its fence is privilege: the split's first migration revokes
  every privilege on it from `PUBLIC`; d3's defaults never reach it, since the migrator creates it
  first and defaults apply only to later objects
  ([ALTER DEFAULT PRIVILEGES](https://www.postgresql.org/docs/17/sql-alterdefaultprivileges.html),
  fetched 2026-10-31); d5 refuses a serving role with any privilege on it. No other table takes
  this exemption without a record.

- **d7 — A single-role install splits once, by moving ownership to a new owner.** A role that is
  not a superuser, the live host's among them, keeps its URL and becomes the serving role. A
  superuser, the docker profile's `POSTGRES_USER` on every existing volume, cannot (d5): it stays
  the administrator, both roles are created, and `postgres.url` moves to the new serving role. A
  superuser runs the transfer, one outage, since the old build cannot run after step 3:
    1. Stop the server; take the backup [upgrades](../operations/upgrades.md) requires as a
       superuser, since `pg_dump` errors when its role cannot bypass row security
       ([pg_dump](https://www.postgresql.org/docs/17/app-pgdump.html), fetched 2026-10-31).
    2. Create the owner, and for a superuser the serving role, under d2's rule; take from a role
       that becomes the serving role every attribute and membership d2 refuses.
    3. Move ownership in the install's database. For a role that is not a superuser, `REASSIGN
       OWNED BY <role> TO <owner>`, after checking `pg_database`, since it also moves every
       database the role owns on the cluster
       ([REASSIGN OWNED](https://www.postgresql.org/docs/17/sql-reassign-owned.html), fetched
       2026-10-31). For a superuser, which `REASSIGN OWNED` refuses as the pinned bootstrap role
       (`pg_shdepend.c`, `REL_17_STABLE` at `ce2797d`, fetched 2026-10-31): `ALTER DATABASE`, then
       `ALTER TABLE` per table in `public` and `ALTER FUNCTION` per function there, each `OWNER TO`
       the owner; a table's indexes, TOAST table and owned sequences follow it (`tablecmds.c`).
    4. Add `postgres.owner_url`, and for a superuser point `postgres.url` at the serving role;
       install and start the build carrying this record, whose first boot applies its migrations
       (d3, d12, d13). Rollback before this step reverses step 3; after it, the backup and
       [restore](../operations/restore.md).

- **d8 — The live host's entry, not run.** As the maintainer's deployment notes of 2026-10-31 record
  it (not in this repository), `oml.doublegate.io` runs server 0.2.0 on PostgreSQL 18 as
  `oml_server` (no superuser, no `BYPASSRLS`), which owns every table of database `oml_server`. It
  takes d7 as a role that is not a superuser: `oml_owner` created, `OML_POSTGRES__OWNER_URL` added
  to `/etc/oml/oml.env`, the cosign-verified release carrying this record installed. Success is both
  role names in the log and `/readyz` 200, recorded here with date and output. **Not run**; every
  superuser step waits for the maintainer.

- **d9 — The checks that close it.** Every Postgres suite, the existing ones included, migrates
  through the owner login of a uniquely named role pair minted for its database, with
  `oml.serving_role` set, and runs its assertions over the serving login, except the journal guard
  law (`journal_refuses_update_and_delete`), which runs as the owner, since privileges refuse the
  serving role first (`42501`, asserted beside it). Minting takes a superuser, which CI's `cargo
  test (Postgres)` job has in the pinned `pgvector/pgvector:pg17`; a suite whose role cannot mint
  the pair fails, as one without Postgres does today:
    - the serving role reading `pg_stats` of a tenant table sees nothing of another tenant (two
      tenants seeded and analysed; the control, the owner's view, must show tenant B's lexemes);
    - the serving role cannot `ALTER`, `DROP` or create a table or lift `FORCE` (`42501`), and has
      no privilege on `_sqlx_migrations`;
    - a boot over one URL (`same_role`), with no owner URL (`owner_url_missing`) or over an
      unsplit database (`serving_role_owns`) exits with `postgres.role_split`, no listener bound;
    - once the listener binds, `pg_stat_activity` shows no owner connection outside d12's DDL;
    - a new table is readable by the serving role and refuses its `INSERT` until granted.

- **d10 — `rls-spike` runs with the split, d12 and d13.** In `testing/spikes/rls/spike_rls.py` (on
  `feat/m0-spikes`) the harness creates `_sqlx_migrations` as sqlx's migrator does before it
  applies the files, and the service role takes its privileges from them through
  `oml.serving_role`, not the harness's own `GRANT`. S3.1 keeps its owner checks of table reads and
  observes its `pg_stats`; S3.4's verdict is `S3.4/service/pg_stats`, the owner's leak its control;
  S3.2 gains the serving role's refused `ALTER TABLE` and `DROP TABLE`; `S3.1/owner/pg_stats` and
  `S3.4/owner/pg_stats` leave `RECORDED_FAILURES`. `S3.4/<role>/estimate` takes the adapter's FTS
  read, tenant A bound at planning; once d12 lands, `S3.4/service/estimate` and
  `S3.4/owner/estimate` leave the list, and the same read with no planning-time tenant, or naming
  tenant B, joins it as `S3.4/<role>/estimate_unbound` (FR-45). With d13 comes
  `S3.4/<role>/fk_oracle`, not recorded: under tenant A, a reference to tenant B's row and one to a
  random id are refused alike, `23503` on one constraint, for d13's three keys. The adapter's FTS
  plan and each partition's `reltuples` are observations. If `S3.4/<role>/estimate` fails once d12
  lands, d12's reading returns to the maintainer as ADR amendment needed.

- **d11 — The build lands after iteration 3, in dependency order, each step closed by a check.**
    1. `feat/iteration-3` (the rewrite of `crates/` and `migrations/`) and `feat/m0-spikes` merge.
       Check: `rls-spike`'s first pg17 run recorded on `spike-3-rls.md`, met (17.11, `bb20087`).
    2. The split, in one landing: its migration after iteration 3's last (d6's revoke, `REVOKE
       CREATE ON SCHEMA public FROM PUBLIC`, each table's privileges, d3's defaults);
       postgres-schema.md's matrix and journal sketch; d4 and d5 in `oml-pg` and
       `oml-server-lead`; d9's suites; d10's harness change; both roles and URLs in
       `docker-compose.yml`, `scripts/poc-demo.sh` (a role pair per run), CI's demo job and
       README's "Running the PoC", which then needs a superuser; the Dockerfile's migrate-on-boot
       comment; a second Secret key in the proposed ADR-0033's chart if it has landed. Check:
       d9's tests, `rls-spike` with `RECORDED_FAILURES` holding the two `estimate` checks alone,
       and CI's `cargo test (Postgres)`, `demo and quickstart` and `docker profile` jobs.
    3. d13's migration and `fk_oracle`. Check:
       `a_reference_to_another_tenants_row_is_refused_as_one_to_no_row` for each of the three
       keys, `every_key_into_a_tenant_table_carries_tenant_id` over `pg_constraint` but the
       migration's named exceptions, and `fk_oracle` green.
    4. d12's migration, partition DDL and tenant predicate, and postgres-schema.md's FTS sketch.
       Check: `creating_a_tenant_creates_its_fenced_partition_before_the_tenant_commits`,
       `a_failed_partition_creation_journals_no_tenant`,
       `the_adapters_fts_read_plans_its_tenants_partition_alone`,
       `the_serving_role_sees_no_pg_stats_row_of_a_partition`, d5's boot checks over partitions,
       and `rls-spike` with `RECORDED_FAILURES` holding the two `estimate_unbound` checks alone.
    5. A release, then d8. Check: d8's output, recorded there.

- **d12 — The FTS index is per tenant: `memories` is list-partitioned by `tenant_id`, each
  partition with its own GIN index.** The migration rebuilds `memories`, which iteration 3's recall
  searches, as `PARTITION BY LIST (tenant_id)`, a partition per existing tenant filled under that
  tenant's context. Its key becomes `(tenant_id, id)`, since a partitioned table's unique keys
  "must include all of the partition key columns", and `memories_claim_fts` a partitioned index,
  so each partition, a later one included, has its own
  ([partitioning](https://www.postgresql.org/docs/17/ddl-partitioning.html), fetched 2026-10-31).
  The designed `memories_fts` and `held_fts` take the same form. Every adapter statement on
  `memories` adds `tenant_id = $n`, bound to the tenant it sets in `oml.tenant_id`, so the plan
  keeps that tenant's partition alone, or drops the rest as it starts (the same page); RLS stays
  the fence. That closes any index across tenants, and the estimate of the adapter's FTS read,
  tenant bound at planning, which draws on that tenant's statistics alone (d10).
    - *Who creates a partition:* the owner, at tenant creation. The act mints the id; on one owner
      connection it creates the partition with `ENABLE`, `FORCE` and its parent's policy, so d5
      holds for it; only then does the serving transaction journal `tenant.created`, so a failed
      DDL journals nothing. This runtime DDL joins what the Costs leave to the bootstrap workload
      or a record of its own; until one takes it the lead runs it, as it migrates (d4), and
      `memory_embeddings`' partitions (ADR-0013 d4) take the same path once that table is built.
    - *Stays open*, by this record's reading of the planner (`REL_17_STABLE` at `f1ba2b8`, fetched
      2026-10-31): a partition's GIN index serves no RLS-bound query either, since the parent's
      policy reaches each partition as a security qual (`apply_child_basequals`) that `@@` cannot
      precede (`restriction_is_securely_promotable`), so FTS reads scan one tenant's rows (FR-4,
      FR-7). SQL the server does not send still reads another tenant's statistics by naming its
      id, or with no planning-time tenant, where an Append sums every partition's estimate
      (`set_append_rel_size`); each partition's `reltuples` and bound give every role one
      tenant's row count and id (FR-45). A candidate for the estimate, unmeasured: statistics
      target 0 on each partition's FTS expression, which "disables collection of statistics for
      that column" ([ANALYZE](https://www.postgresql.org/docs/17/sql-analyze.html), fetched
      2026-10-31).

- **d13 — A foreign key into a tenant table carries `tenant_id`.** The pg17 run accepted three
  cross-tenant references to bare ids: `workareas.parent_id` → `workareas (id)`,
  `memories.created_event_id` → `journal (event_id)` and `review_queue.memory_id` → `memories
  (id)`. The migration rewrites these, and every other key whose referenced table has `tenant_id`
  (iteration 3 has more, `memories.workarea_id` and `superseded_by` among them), as `(tenant_id,
  col)` → `(tenant_id, id)`, the form 0004 gave `grants`, adding each `UNIQUE (tenant_id, …)` it
  needs, so a cross-tenant reference is refused as one to no row is. Where the referencing
  `tenant_id` is NULL, as on `api_keys`' provider-level rows, "the row is not required to have a
  match" ([CREATE TABLE](https://www.postgresql.org/docs/17/sql-createtable.html), fetched
  2026-10-31), and a key that names a provider-level row by design cannot take this form: the
  migration names each such key and how it stays checked.

## Consequences

- **FR-44 and FR-45 open** on the [roadmap](../product/roadmap.md), marked proposed: FR-44 for the
  split and the read log's partition DDL, FR-45 for the estimate and catalog channels d12 leaves.
- **On acceptance, and not before:** ADR-0007's Status line gains `amended by ADR-0034`, its d2 text
  kept as history ([status lifecycle](index.md#status-lifecycle)). security.md's tenancy paragraph
  says that `FORCE` binds an owner's table reads, not its `pg_stats` view or DDL, which the split
  keeps from every workload; that the owner role only migrates and creates tenants' partitions, in
  the lead until a bootstrap workload exists; and that `_sqlx_migrations` is the one table
  exempted, fenced by privilege; its STRIDE information-disclosure row adds FR-45's channels to
  timing and contention. configuration.md, deployment.md's lead row, upgrades.md,
  postgres-schema.md, retrieval.md's FTS paragraph, the critical path's S3.1 row and the
  register's D6 cell follow.
- **Costs:** the owner credential in the lead's configuration (d4); one outage per single-role
  install (d7); every new table's migration names its writes (d3); a second credential in every
  install, the development rig's included (d5); a superuser for the test suites and the demo (d9,
  d11); a partition, an index and an owner connection per tenant, planning that grows with tenants
  where a statement is not pruned at planning, and a tenant predicate on every `memories`
  statement (d12); `tenant_id` in every key into a tenant table (d13). `read_events` partition
  creation and `DROP`, its retention act ([postgres-schema](../data/postgres-schema.md)), wait for
  the bootstrap workload or a record of their own, and nothing creates or drops them until then.
- **No surface change:** no HTTP operation, error code or event type, so `info.version` stays
  `0.2.0-design`; `postgres.role_split` is a startup error outside the [error
  taxonomy](../api/errors.md). Other workloads' roles come with those workloads.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **One role, `FORCE` as the whole fence** | S3.1: `FORCE` filters neither the owner's `pg_stats` nor its DDL, so the serving connection of such an install can do both. |
| **A development mode accepting one URL, with a warning on `/readyz` or in `oml doctor`** | ADR-0023 binds C6's split in all three profiles and keeps the binaries profile-blind, with no `--profile` flag (d8); a mode relaxing C6 is that flag. A warning leaves the leak on where installs start, to save one role in an init script. |
| **Fixed role names in the migrations** | Roles are cluster-wide: the maintainer's host runs the live database beside throwaway test databases on one cluster, and fixed names would give them all one serving role. |
| **Row-level security on `_sqlx_migrations`** | No `tenant_id` to key a policy on; a policy keyed on the role restates the privilege fence, and under `FORCE` binds the migrator's own reads. |
| **A migrate-only invocation, so the serving process never holds the owner URL** | That is the bootstrap workload [deployment](../operations/deployment.md) designs, with the standalone migrate verb of FR-28; it changes every payload's upgrade step and is its own change, which removes d4's cost. |
| **Keeping the old role as owner and adding a new serving role** | No ownership moves, but the credential that has sat in the serving process all along keeps ownership, and `postgres.url` changes; d7 hands ownership to a fresh credential. |
| **Statistics target 0 on the one FTS index, instead of partitions** | By the source above it would leave every role FTS's default selectivity, unmeasured, but one index would still span tenants, which ADR-0007 d4 and the STRIDE row rule out; d12 keeps it as FR-45's candidate beside the partitions. |
| **A `DEFAULT` partition for tenants without one of their own** | The tenants in it share one index again, and a later partition for one of them must first move its rows out. |
| **A `SECURITY DEFINER` function the serving role calls to create a partition** | Any serving connection could call it, an injection included; d5 refuses definer functions for that reason. d12's owner connection is reachable from the tenant-creating act alone. |
| **A leakproof `@@` wrapper, so the GIN index serves RLS-bound queries** | Only a superuser can mark a function `LEAKPROOF` (S3.4), PostgreSQL does not mark `ts_match_vq` so, and d2's roles hold no superuser; it is FR-4's and FR-7's question, with numbers. |
