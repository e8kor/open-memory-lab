<!--
id: engineering/spikes/spike-3-rls
tier: engineering
status: recorded findings · 2026-10-31 — on PostgreSQL 18.6 S3.1 and S3.4 fail on the statistics of a cross-tenant FTS index, which the owner reads in pg_stats and EXPLAIN's estimate follows (ADR amendment needed, or an implementation fix, awaiting the maintainer's decision), S3.3 passes, S3.2's mechanism did not run, and S3.2's claim about OML's roles is open because no run can test it; the pinned pg17 has not run
answers: what M0 spike 3 found for S3.1–S3.4 (FORCE and the owner, the owner/service split as a PostgreSQL mechanism and as a claim about OML's roles, SET LOCAL across pooled transactions, the hostile cross-tenant probe), on which versions, what failed and why, which channel the role split closes and which it does not, and what each finding asks of the design
-->

# Findings: spike 3, RLS hardening

**Status:** recorded findings · 2026-10-31 — on PostgreSQL 18.6 S3.1 and S3.4 fail on the statistics of a cross-tenant FTS index, which the owner reads in pg_stats and EXPLAIN's estimate follows (ADR amendment needed, or an implementation fix, awaiting the maintainer's decision), S3.3 passes, S3.2's mechanism did not run, and S3.2's claim about OML's roles is open because no run can test it; the pinned pg17 has not run

These are the findings for the four scenarios of the critical path's
[spike 3](../../product/critical-path.md#spike-3-rls-hardening), one per scenario, in its
template. The harness is `testing/spikes/rls/spike_rls.py`, and its decisions (roles, seeding,
controls, recorded failures) are on the [M0 spikes](index.md) page.

**Where they ran.** On the host PostgreSQL 18.6, as a role that can create databases but not
roles. The connecting role therefore owned the database and every table, ran S3.1, S3.3 and
S3.4 as that owner, and could not run S3.2's mechanism. The pin is `pgvector/pgvector:pg17`. CI
job `rls-spike` runs every scenario there as an owner role and a separate service role, and
that job has not run yet. None of the verdicts below is a pg17 result, and none of them is a
result for a non-owner role.

| Scenario | Verdict on 18.6 (owner role) | Pinned pg17, owner and service roles |
|---|---|---|
| [S3.1](#s31) FORCE binds the owner | **fail**: table reads are filtered, planner statistics are not | not yet run (CI `rls-spike`) |
| [S3.2 (mechanism)](#s32-mechanism) a non-owner without BYPASSRLS stays inside the policy | **inconclusive**: not run here, the role cannot create roles | not yet run (CI `rls-spike`) |
| [S3.2 (OML's roles)](#s32-oml-roles) OML's roles are split, none holds BYPASSRLS | **inconclusive**: not testable, no deployment's roles are specified | the same: not testable in CI either |
| [S3.3](#s33) SET LOCAL across pooled transactions | **pass**: a read with no tenant returns 0 rows | not yet run (CI `rls-spike`) |
| [S3.4](#s34) hostile cross-tenant probe | **fail**: every read shape of the fold tables returns 0 rows; `pg_stats` and EXPLAIN's row estimate do not | not yet run (CI `rls-spike`) |

The two failures are one fact, seen from two scenarios: the statistics of
`memories_claim_fts`, one expression index over every tenant's claims. They reach a session set
to tenant A along two channels:

- **`pg_stats`** shows the index's lexemes to the owner. Whether it shows them to a role that
  does not own the index is CI's `S3.4/service/pg_stats`. This is the channel the role split can
  close.
- **EXPLAIN's row estimate** follows the same lexemes for the owner, while `FORCE` binds it
  ([S3.4](#s34)). PostgreSQL's documentation and source give the same estimate to every role
  whose query the policy binds, the service role included. CI's `S3.4/service/estimate` will
  measure that, and has not run. The role split does not close this channel.

The three owner checks behind the two failures (`S3.1/owner/pg_stats`, `S3.4/owner/pg_stats`,
`S3.4/owner/estimate`) are entries in the harness's `RECORDED_FAILURES`. CI stays green while they
reproduce, and fails when they stop. The two service-role checks are not on the list, so CI fails
if either leaks. Recording a failure does not settle it: each finding says ADR amendment needed
until the maintainer decides, and the harness prints the same action. The run seeded every table
of migrations 0001–0005, so no table was left unprobed.

## S3.1 — FORCE binds the table owner {#s31}

```text
spike-finding: S3.1
date:          2026-10-31
versions:      PostgreSQL 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1), the host's; the pinned
               pgvector/pgvector:pg17 not yet run; psycopg 3.3.6 (libpq 18.6),
               psycopg-pool 3.3.3, Python 3.13.13, uv 0.11.6
settings:      server defaults (row_security=on, plan_cache_mode=auto, read committed,
               UTF8); migrations 0001-0005 applied by the harness, one transaction each,
               two tenants seeded after 0003; ANALYZE after seeding
environment:   Linux 7.0.0-38-generic x86_64, 8 vCPU AMD EPYC-Genoa, 15.2 GiB RAM; host
               Postgres on 127.0.0.1:5432; connecting role oml (CREATEDB, not a superuser)
               owns the throwaway database and all 11 tables
verdict:       fail: the owner's table reads are filtered, and its view of the planner
               statistics of the expression index memories_claim_fts is not
adr-clause:    ADR-0007 d2, through the FORCE commitment of security.md (commitments C6);
               ADR-0007 d4 and security.md's STRIDE information-disclosure mitigation,
               for the statistics of the cross-tenant index memories_claim_fts
action:        ADR amendment needed (ADR-0007 d2 and d4; security.md's FORCE commitment
               and its STRIDE information-disclosure mitigation), or an implementation
               fix that makes the FTS index per tenant, which this run does not test;
               awaiting maintainer decision; recorded in the harness as
               S3.1/owner/pg_stats
```

**What held.** All 11 tables have `ENABLE` and `FORCE ROW LEVEL SECURITY` and a policy. Set to
tenant A, the owner sees 29 rows over the 11 tables and none of tenant B. Set to tenant B, it sees
B's 29 and none of A's. With no tenant set, it sees 0 rows in every table. `SET row_security =
off` does not unfilter a forced table: the read raises `42501 query would be affected by
row-level security policy for table "memories"`. The control lifted `FORCE` on `memories`
inside a transaction it rolled back, and the owner, set to tenant A, then read tenant B's 4
memories. `FORCE` is what filtered the owner, and it was set again after the rollback.

**What failed.** The owner, set to tenant A, read `pg_stats` and got one row, for the expression
index `memories_claim_fts` (`to_tsvector('simple', claim)`). Its `most_common_elems` holds the
lexemes of both tenants' claims, tenant B's among them: `secret`, `tenant-b-secret`, `revised`.
That is tenant B's memory text, as words, read by a session set to tenant A.

**Why.** On this server the view's filter is

```sql
WHERE ((NOT a.attisdropped) AND has_column_privilege(c.oid, a.attnum, 'select'::text)
  AND ((c.relrowsecurity = false) OR (NOT row_security_active(c.oid))))
```

and `c` is the relation the statistics belong to. For an expression index, that relation is the
index. An index carries no row-level security of its own, so the RLS test passes, and the
table's owner holds the column privilege. The PostgreSQL documentation for
[pg_stats](https://www.postgresql.org/docs/17/view-pg-stats.html) (fetched 2026-10-31) says the
view "allows access only to rows of pg_statistic that correspond to tables the user has
permission to read". The owner holds that privilege, so the row is shown, and the documentation
says nothing about row-level security there. This finding does not test whether a role that does
not own the index sees the row. CI's service-role run checks that (`S3.4/service/pg_stats`, not
on the recorded list, so a leak there fails CI). The planner does not read statistics through
this view, and the estimate it derives from the same lexemes reaches roles that do not own the
index ([S3.4](#s34)).

**Observed.** The owner can also lift `FORCE` with `ALTER TABLE … NO FORCE ROW LEVEL SECURITY`, as
the control shows. `FORCE` binds what the owner reads from the table. It does not bind the
owner's DDL, and it does not bind the owner's view of the catalog. Both are reasons the querying
role must not be the owner, and that is S3.2's split.

**Reading for the action.** The maintainer has not decided, so the finding says **ADR
amendment needed**. It touches two sets of clauses, and the second limits what the first can
settle.

- **`FORCE` and the owner (ADR-0007 d2, security.md's `FORCE` sentence).** There are two readings.
  In one, d2 binds the roles OML workloads use, and security.md's split keeps those roles off
  ownership: "migrations run under a separate owner role no workload uses". Then `FORCE` need only
  fence the owner's table reads, and security.md takes the page edit below. In the other,
  security.md's `FORCE` sentence promises that `FORCE` fences the owner, and the owner's view of
  `pg_stats` breaks that promise. Under either reading the split confines at most the `pg_stats`
  channel, and only if CI's `S3.4/service/pg_stats` shows the service role does not see the row.
  It does not confine EXPLAIN's estimate, which follows the same statistics for the owner under
  `FORCE` and, by PostgreSQL's documentation and source, for every role the policy binds
  ([S3.4](#s34)). So "the split keeps the leak from every workload" is not available, under either
  reading.
- **The cross-tenant FTS index (ADR-0007 d4, security.md's STRIDE row).** d4 decides "Tenant
  isolation extends through every storage layer". security.md's information-disclosure
  mitigation says: "Embeddings and FTS are physically partitioned per tenant — there is no
  cross-tenant index to leak through". `memories_claim_fts` is one index over every tenant's
  claims, and its statistics leak through `pg_stats` (the owner, measured) and through EXPLAIN's
  estimate (the owner, measured; the service role, not yet measured, CI's `S3.4/service/estimate`).
  The designed `memories_fts`
  and `held_fts` ([postgres-schema](../../data/postgres-schema.md#full-text-search-and-embeddings))
  are each one table with one GIN index across tenants. Only `memory_embeddings` is
  list-partitioned per tenant. So the schema sketch already disagrees with the STRIDE row before
  any code, and a wording edit to security.md does not settle this half. Either d4 and the STRIDE
  row are amended to say that FTS statistics are shared across tenants, or the implementation,
  and the schema sketch, make the FTS index per tenant. This run does not test whether a
  per-tenant index closes either channel.

The finding asks for three more things whichever way the maintainer decides:

- **The split is not built.** The evidence is a run outside the harness, recorded under
  "Evidence: the lead migrates as its serving role" below. `oml-server-lead` 0.2.0 (commit
  `030d7b8`) ran the migrations on boot over its one `postgres.url`, and that URL's role then
  owned all 12 tables: the 11 of migrations 0001–0005 and `_sqlx_migrations`. The lead's source
  at that commit agrees. `crates/bins/oml-server-lead/src/main.rs` connects one `PgStore` to
  `postgres.url`, calls `migrate()` on it and serves over the same store, and the lead's
  `Config` has no `postgres.owner_url`, the owner URL that
  [configuration](../../operations/configuration.md) designs. As built, the role that serves
  tenant requests is the owner, so both the statistics leak and the `NO FORCE` capability reach
  it. Using either still needs SQL that the server does not issue. This is an implementation
  gap against C6: the split is decided and not built.
- **A page edit for security.md**, proposed and not made here: say that `FORCE` binds an owner's
  table reads but not its view of planner statistics or its DDL, that the split, not `FORCE`, is
  what keeps the DDL and the `pg_stats` view from a workload, and that no role split keeps the
  planner's estimates from one. That edit covers the `FORCE` half only. The STRIDE row is the
  amendment-or-fix question above.
- **`_sqlx_migrations`: ADR amendment needed (ADR-0007 d2), or an implementation fix.**
  ADR-0007 d2 decides "Row-level security keyed by `tenant_id` on every Postgres table. No table
  is exempted for convenience", and security.md says the same. The lead's sqlx migrator creates
  `_sqlx_migrations` with no RLS, no `FORCE` and no policy (the run below). The table holds no
  tenant data and has no `tenant_id` to key a policy on. So either d2 is amended to name the
  migrator's ledger as an exemption, or the implementation brings that table under d2. Which one
  is the maintainer's decision. The harness applies the SQL files directly, so its database has
  no such table, and none of its checks cover it.

**Evidence: the lead migrates as its serving role.** This run happened on 2026-10-31, outside the
harness, against a throwaway database on the host's PostgreSQL 18.6. The connecting role was
`oml` (CREATEDB, not a superuser), the same as in the harness run. `env -i` keeps every `OML_*`
variable out of the environment, because the lead reads those variables over the file
(`crates/bins/oml-server-lead/src/lib.rs`, `Config::load`). The configuration file, with the
scratch path and the database name replaced:

```toml
[postgres]
url = "postgresql://oml:oml_dev_only@127.0.0.1:5432/<throwaway>"

[server]
listen = "127.0.0.1:8341"

[kms]
adapter = "file"
keyring_path = "<scratch>/keyring"
```

```text
$ psql -h 127.0.0.1 -U oml -d oml_dev -c "CREATE DATABASE <throwaway>"
$ env -i PATH=/usr/bin:/bin HOME=<scratch> /usr/local/bin/oml-server-lead --config <scratch>/oml.toml
INFO oml_server_lead: build version=0.2.0 commit=030d7b8005ed668032c1a620805b0549cd8acbec ...
INFO oml_server_lead: migrations applied
INFO oml_server_lead: startup fork check passed tenants=0
INFO oml_server_lead: serving both rings addr=127.0.0.1:8341

$ psql -h 127.0.0.1 -U oml -d <throwaway> -c "SELECT c.relname, pg_get_userbyid(c.relowner) AS owner,
    c.relrowsecurity, c.relforcerowsecurity,
    (SELECT count(*) FROM pg_policy p WHERE p.polrelid = c.oid) AS policies
  FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
  WHERE n.nspname = 'public' AND c.relkind IN ('r','p') ORDER BY 1"
      relname       | owner | relrowsecurity | relforcerowsecurity | policies
--------------------+-------+----------------+---------------------+----------
 _sqlx_migrations   | oml   | f              | f                   |        0
 api_keys           | oml   | t              | t                   |        2
 grants             | oml   | t              | t                   |        1
 idempotency        | oml   | t              | t                   |        1
 journal            | oml   | t              | t                   |        1
 journal_chain_head | oml   | t              | t                   |        1
 key_denials        | oml   | t              | t                   |        1
 memories           | oml   | t              | t                   |        1
 outbox             | oml   | t              | t                   |        1
 review_queue       | oml   | t              | t                   |        1
 tenants            | oml   | t              | t                   |        2
 workareas          | oml   | t              | t                   |        1
(12 rows)
```

`_sqlx_migrations` listed versions 1–5, all `success = t`. The lead was then stopped and the
database dropped. The run's log also printed the empty deployment's single-use enrollment token,
which is left out here. Re-running it takes the steps above and nothing else. The repository has
no script for it, because the harness, not the lead, is what CI runs.

## S3.2 — owner/migration-role split, no BYPASSRLS {#s32}

S3.2 asserts that migrations succeed and that "no application role can read across the policy".
The harness reports that as two findings
([M0 spikes](index.md#s32-is-two-findings-and-the-one-about-omls-roles-stays-open)).
The first is PostgreSQL's mechanism, which a run can test. The second is the claim about OML's
own roles, which no run can test yet.

### S3.2 (mechanism) {#s32-mechanism}

```text
spike-finding: S3.2 (mechanism)
date:          2026-10-31
versions:      PostgreSQL 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1), the host's; the pinned
               pgvector/pgvector:pg17 not yet run; psycopg 3.3.6 (libpq 18.6),
               psycopg-pool 3.3.3, Python 3.13.13, uv 0.11.6
settings:      as S3.1
environment:   as S3.1; the connecting role has CREATEDB and neither CREATEROLE nor
               superuser, so the harness cannot mint the owner and service roles
verdict:       inconclusive: not run on this host
adr-clause:    ADR-0007 d2 (no BYPASSRLS role), through the role split of security.md
               (commitments C6; C3 for the journal re-grant): the PostgreSQL mechanism
               the split relies on
action:        run in CI job rls-spike on the pinned pg17 with --require-all, and record
               that result here
```

**What did run.** The half of the mechanism that needs no second role ran. Migrations 0001–0005
succeed under the owner role while `FORCE` binds it, over two tenants' rows seeded between 0003
and 0004. The backfills of 0004 and 0005 reach both tenants: every key's `issued_seq` and
`issued_by` match its `key.issued`, and every memory's `version`, `kind`, `body_bytes`, `is_tip`
and `promoted_at` match what 0005 derives. Each migration enumerates tenants through 0003's
startup policy and runs under the ordinary tenant context, as its comments say, and `FORCE` does
not stop it.

**What CI asserts** once it runs, as the service role: set to tenant A, it sees only A's rows,
and with no tenant set it sees 0 rows. Each of these is refused with `42501`: `NO FORCE`, `DISABLE ROW LEVEL SECURITY`,
`DROP POLICY`, `CREATE POLICY`, `OWNER TO`, `SET ROLE` to the owner, and, with the `journal`
grants the harness wrote, `UPDATE`, `DELETE` and `TRUNCATE` on `journal` with the tenant set.
`SET row_security = off` raises instead of unfiltering, and a self-`GRANT UPDATE ON journal`
leaves the privilege absent. Two controls must leak: the owner runs each of the four DDL
statements in a transaction that rolls back, and each takes effect, so each refusal comes from
the role and not from the statement; and the service role's count with tenant B set shows B's
rows. The role attributes, no superuser, `BYPASSRLS`, `CREATEROLE` or `CREATEDB`, no relation
owned and no membership in the owner role, are the harness's own setup. They are preconditions,
not checks: if they are not as minted, the finding is inconclusive.

### S3.2 (OML's roles) {#s32-oml-roles}

```text
spike-finding: S3.2 (OML's roles)
date:          2026-10-31
versions:      none apply: no run of the harness can test this claim
settings:      none
environment:   every host, CI included
verdict:       inconclusive: not testable; OML specifies no deployment's database roles
               and grants
adr-clause:    ADR-0007 d2 (no role used by any OML workload holds BYPASSRLS), through
               the role split of security.md (commitments C6)
action:        keep open; when a deployment's bootstrap specifies its roles and grants,
               take them from there and test those roles, not the harness's
```

**Why it is not testable.** "No application role can read across the policy" is a claim about
the roles an OML deployment uses. OML has none to test. [Deployment](../../operations/deployment.md)
names the bootstrap workload's owner role and a service role created at init, and
[configuration](../../operations/configuration.md) designs `postgres.owner_url`. No page states
those roles' attributes and grants statement by statement. The lead as built migrates on boot as
its serving role ([S3.1](#s31), the lead run), so for the PoC as built S3.2's premise does not
hold. The harness's owner and service roles are its own. It mints them `NOBYPASSRLS` and writes
their grants itself, so asserting their attributes would show only that it ran its own
`CREATE ROLE` statements. A CI run therefore prints this finding as inconclusive, and never as
`pass` with action `none`. The finding does not fail the run, and the run's last line names it as
open.

## S3.3 — SET LOCAL across pooled transactions {#s33}

```text
spike-finding: S3.3
date:          2026-10-31
versions:      PostgreSQL 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1), the host's; the pinned
               pgvector/pgvector:pg17 not yet run; psycopg 3.3.6 (libpq 18.6),
               psycopg-pool 3.3.3, Python 3.13.13, uv 0.11.6
settings:      as S3.1; psycopg_pool.ConnectionPool(min_size=1, max_size=1,
               autocommit=True), so one backend serves every checkout (one pid seen)
environment:   as S3.1; run as the owner role, which FORCE binds
verdict:       pass; a read with no tenant set returns 0 rows, and a write with no tenant
               set is refused with 42501
adr-clause:    ADR-0007 d3, through the SET LOCAL commitment of security.md (commitments
               C6); ADR-0024 d4 for prepared statements on a pooled connection
action:        none for the scenario; a page edit is needed on postgres-schema.md, whose
               policy sketch behaves differently (below)
```

**What held**, all on one backend:

- On a fresh connection, `oml.tenant_id` is unset (`NULL`) and every table shows 0 rows.
- Eight interleaved transactions A, B, A, B, … ended by commit and by rollback, and set the
  tenant by `set_config(…, true)` (the adapter's form) and by `SET LOCAL`. Each saw only its own
  tenant's memories, and after each one the setting read `''` and 0 rows showed.
- A transaction aborted by an error leaves `''` and 0 rows.
- `SET LOCAL` inside a savepoint that rolls back reverts to the outer tenant, and the outer
  commit clears it.
- One protocol-level prepared statement, executed under A, B, none, A, B, none, returns each
  tenant's rows only and none with no tenant. A SQL `PREPARE` under `plan_cache_mode =
  force_generic_plan`, executed eight times alternating A and B, returns each tenant's rows only.
  The policy's `current_setting` is evaluated at execution, and no cached plan carries a
  tenant.
- `SET LOCAL` outside a transaction block has no effect. The server warns `25P01 SET LOCAL can
  only be used in transaction blocks`, and the next statement shows 0 rows.
- A connection returned to the pool inside an open transaction comes back rolled back, with
  `''` and 0 rows.
- The control was a session-level `SET oml.tenant_id`. It survived into the next pooled checkout,
  whose transaction read tenant A's 4 memories. That is the leak `SET LOCAL` prevents.

**The recorded behaviour.** The critical path asks which of two behaviours holds for a query with
no tenant set, zero rows or a typed error, and says the answer becomes the documented behaviour.
**It is zero rows**, for reads, both on a connection that never set a tenant (`NULL`) and on one
that set one and ended the transaction (`''`). The migrations' policies give this through
`NULLIF(current_setting('oml.tenant_id', true), '')::uuid`. A write with no tenant set is a typed
error, `42501 new row violates row-level security policy`.

**What disagrees.** The sketch on [postgres-schema](../../data/postgres-schema.md#row-level-security)
writes the predicate as `current_setting('oml.tenant_id')::uuid`. That form raises `42704`
(unrecognized parameter) on a connection that never set the tenant, and `22P02` (`''` is not a
uuid) on a connection that set one before. So under the sketch the error would depend on the
connection's history. That page and the migrations are normative together (its sync rule), and
the migrations are being rewritten on another branch. The edit is therefore left to that
change: the sketch should take the migrations' form and say that a read with no tenant returns
0 rows.

## S3.4 — hostile cross-tenant probe {#s34}

```text
spike-finding: S3.4
date:          2026-10-31
versions:      PostgreSQL 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1), the host's; the pinned
               pgvector/pgvector:pg17 not yet run; psycopg 3.3.6 (libpq 18.6),
               psycopg-pool 3.3.3, Python 3.13.13, uv 0.11.6
settings:      as S3.1; every probe runs in a transaction set to tenant A the way the
               adapter sets it, with tenant B's values in its parameters (the WHERE-clause
               bug), and rolls back
environment:   as S3.1; run as the owner role, which FORCE binds
verdict:       fail: every read shape of the fold tables returns 0 rows of tenant B;
               pg_stats and EXPLAIN's row estimate do not (S3.1's failure, seen from a
               tenant session)
adr-clause:    ADR-0007 d2 and d6 (RLS as the second fence, judged against the hostile
               co-tenant); ADR-0007 d4 and security.md's STRIDE information-disclosure
               mitigation, for the statistics of the cross-tenant index memories_claim_fts
action:        ADR amendment needed (ADR-0007 d2 and d4; security.md's FORCE commitment
               and its STRIDE information-disclosure mitigation), or an implementation
               fix that makes the FTS index per tenant, which this run does not test;
               awaiting maintainer decision; recorded in the harness as
               S3.4/owner/pg_stats and S3.4/owner/estimate; the observations below go to
               M6's security pass, and one goes to FR-4 and FR-7
```

**What held.** Every probe below returned 0 rows of tenant B and raised no error under tenant A.
Its control, the same probe with the tenant set to B, found B's rows each time, so each probe could
have found what it hunted.

- **The adapter's 24 shapes at `02a385d`** (`crates/adapters/oml-pg/src/lib.rs`): chain head,
  journal range, tail-scan subjects, get, hydrate, recall FTS, own-held recall FTS, steering, the
  own and tips listings with every filter and keyset, the claim-key state aggregate, served tip,
  held successors, review claim-next (join, `NOT EXISTS`, `FOR NO KEY UPDATE … SKIP LOCKED`) and
  its update, key by hash, issued seq, a key's grants and denials, workarea get, ancestry and
  subtree (recursive), children, idempotency get.
- **14 more shapes that name B's rows:** a join on B's ids, an `IN` subquery, `EXISTS`, `count`,
  jsonb `->>` and `@>`, array `@>`, FTS on a word only B uses, `FOR UPDATE`, `FOR SHARE`,
  `UPDATE … RETURNING`, the tip-moving `UPDATE … FROM`, `DELETE … RETURNING`, and a cast that
  raises an error naming B's text (it raises one under tenant B and none under tenant A).
- **33 shapes with no tenant predicate:** a full scan and a `GROUP BY` tenant of each of the 11
  tables, five joins, `LATERAL`, `UNION ALL`, a window, `ORDER BY … LIMIT`, array overlap and a
  regular expression. Each returned tenant A's rows and no value of tenant B.
- **23 index probes, one for each index of the schema,** with seq scans off (and bitmap scans
  off for btree). 22 of the plans used the index the probe names. The exception is the GIN
  index, explained below.
- A cursor, `COPY memories TO STDOUT` and `COPY (SELECT * FROM journal) TO STDOUT`.
- A non-leakproof plpgsql function of cost 0.0001, in the `WHERE` clause over 6 tables. Under
  tenant A it was handed 23 values, none of them B's. Under tenant B it was handed B's 23.

**What failed.** Two checks, both on the statistics of `memories_claim_fts`. No read shape of
the fold tables touches either channel, so each leak needs SQL that the server does not issue.

- **`pg_stats`**, as in [S3.1](#s31): the session set to tenant A reads tenant B's lexemes in the
  statistics of `memories_claim_fts`.
- **EXPLAIN's row estimate.** Under tenant A, EXPLAIN estimates 2 rows for an FTS query on
  `secret`, a word only tenant B's memories hold, and 1 row for `zzzunusedword`, which no memory
  holds. Both queries return 0 rows. For `visible`, a word only tenant A's memories hold, it
  estimates 2 rows, and the query returns 4. The estimate follows a word's frequency over every
  tenant's rows, not over the rows the policy shows. So a session set to tenant A can learn that
  another tenant's memories hold a word: it runs EXPLAIN and compares the estimate with what its
  own rows hold. This page's first version recorded the estimate as an observation. It is now
  the check `S3.4/<role>/estimate`, and the owner's failure is recorded.

**Why the estimate is not an owner channel.** The owner measured it while `FORCE` bound the
owner, and under the policy every role reaches the estimator along the same path. The
PostgreSQL documentation for
[planner statistics and security](https://www.postgresql.org/docs/17/planner-stats-security.html)
(fetched 2026-10-31) limits the planner's use of statistics to a user with `SELECT` on the table
or a leakproof operator, and then says: "This restriction applies only to cases where the planner
would need to execute a user-defined operator on one or more values from `pg_statistic`". The
source at `REL_17_STABLE` commit `ce2797d` (fetched 2026-10-31) shows that the FTS estimator is
not such a case. In `src/backend/utils/adt/selfuncs.c`, an expression index's statistics may be
handed to a non-leakproof operator only when `all_rows_selectable` holds, and that returns false
whenever an RLS policy applies ("For all rows to be accessible, there must be no securityQuals
from security barrier views or RLS policies"). `statistic_proc_security_check` is where that
result is enforced. In `src/backend/tsearch/ts_selfuncs.c`, `tsquerysel` reads the
most-common-elements slot of the statistics tuple, compares lexemes with its own comparator, and
never calls `statistic_proc_security_check`. The service role holds `SELECT` on `memories` and
runs under the same policy, so on this reading it gets the same estimates. That is a reading of
the source, not a measurement. CI's `S3.4/service/estimate` will measure it, and has not run. It
is not on the recorded list, so if the reading holds, CI fails on its first run, and this
finding records the result then. No role split closes this channel.

**Observed: none of these reads a tenant B row, and the security pass should weigh each.**

- **RLS keeps the GIN index out of every FTS plan.** `@@` (`ts_match_vq`) is not leakproof, and
  neither are `@>` and `&&` on arrays or `@>` and `->>` on jsonb. `=` on uuid is. Set to tenant A,
  the FTS probe's plan scans `memories_author` on the policy's `tenant_id` and filters every
  candidate row with `to_tsvector`. With `FORCE` lifted (rolled back), the same plan uses
  `memories_claim_fts`. The PostgreSQL documentation for
  [`LEAKPROOF`](https://www.postgresql.org/docs/17/sql-createfunction.html) (fetched 2026-10-31)
  states the rule: "the system will enforce conditions from security policies and security
  barrier views before any user-supplied conditions from the query itself that contain
  non-leakproof functions", and only a superuser can mark a function leakproof. The adapter's
  recall shapes therefore use no FTS index under RLS. The designed `memories_fts_gin` and
  `held_fts_gin` ([postgres-schema](../../data/postgres-schema.md)) sit under the same policies,
  so they would serve no RLS-bound query either. This is a measured plan, not a latency number.
  It belongs with [FR-4](../../product/roadmap.md) and [FR-7](../../product/roadmap.md), and it may
  re-open the FTS index strategy before M2's retrieval work. The maintainer decides whether that
  needs a record.
- **Catalog row counts span tenants.** `pg_class.reltuples` for `memories` reads 8, both tenants'
  rows, while tenant A sees 4, and EXPLAIN's estimate for a full scan under tenant A is 4.
  security.md lists timing and contention as the side channels v1 does not address, and it lists
  neither these counts nor the estimate above. Unlike the estimate, this one is not a check.
- **Foreign keys check without RLS.** A tenant A workarea naming tenant B's universe as its parent
  is accepted, and naming a random id is refused with `23503`. So tenant A can test whether an id
  exists in another tenant, and can store a reference across tenants. The same holds for
  `memories.created_event_id` → `journal(event_id)` and `review_queue.memory_id` → `memories(id)`.
  The adapter's ancestry walk already refuses a parent outside the tenant. 0004 gave `grants` and
  `key_denials` foreign keys that name the tenant, and these three references could take the same
  form, which is a migration item.
- **A global unique index is an oracle.** A tenant A key row carrying tenant B's key hash is
  refused with `23505` on `api_keys_hash_key`. Using the oracle needs the hash, which is the
  SHA-256 of a secret key value.
- **Two policies open rows by design.** 0003's `lead_startup_enumeration` shows every tenant's id
  and name to a session that sets `oml.startup_check = 'on'`, whatever `oml.tenant_id` says. 0001's
  `key_resolution` shows the key row whose hash a session puts in `oml.api_key_hash`. Both
  settings have the same trust as `oml.tenant_id`: only server code sets them.

## For the maintainer

- **ADR amendment needed (ADR-0007 d2 and d4; security.md's `FORCE` commitment and its STRIDE
  information-disclosure mitigation), or an implementation fix, awaiting the maintainer's
  decision.** S3.1 and S3.4 fail on the statistics of one FTS index over every tenant's claims.
  The `FORCE` half has two readings, and neither has been chosen (S3.1, "Reading for the action").
  The d4 and STRIDE half is not settled by a wording edit. Either they are amended to say that FTS
  statistics are shared across tenants, or the implementation, and the schema sketch, whose
  `memories_fts` and `held_fts` are not partitioned per tenant either, make the FTS index per
  tenant. Whether a per-tenant index closes both channels is untested.
- **A cross-tenant disclosure the role split does not close:** EXPLAIN's row estimate tells a
  session set to tenant A whether another tenant's memories hold a word (S3.4). It is measured for
  the owner. For the service role, PostgreSQL's documentation and source say it holds, and CI's
  `S3.4/service/estimate` will measure it once CI runs. The split keeps the owner's DDL from a
  workload, and its `pg_stats` view too if CI's `S3.4/service/pg_stats` passes. It does not keep
  this estimate from one.
- **ADR amendment needed (ADR-0007 d2), or an implementation fix, awaiting the maintainer's
  decision:** `_sqlx_migrations` has no RLS, no `FORCE` and no policy, and d2 exempts no table
  (S3.1, the lead run).
- **Implementation:** the lead's migrate-on-boot makes the serving role the owner (S3.1, the lead
  run). The split is what C6 decides, and it is not built. S3.2 (OML's roles) stays open until a
  deployment's bootstrap specifies its roles and grants.
- **Page edits, proposed and not made:** security.md, on what `FORCE` binds; postgres-schema.md,
  on the policy predicate form and the zero-row behaviour.
- **A decision to weigh:** whether a GIN index under RLS is still the FTS strategy, given that RLS
  keeps it out of every plan.
- **The pinned run:** CI job `rls-spike` on `pgvector/pgvector:pg17` decides S3.2's mechanism and
  the service-role half of S3.3 and S3.4, `S3.4/service/estimate` among them, and these findings
  are updated with its result.
