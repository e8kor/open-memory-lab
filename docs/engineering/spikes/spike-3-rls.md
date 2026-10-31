<!--
id: engineering/spikes/spike-3-rls
tier: engineering
status: recorded findings · 2026-10-31 — on PostgreSQL 18.6 S3.1 and S3.4 fail for the table owner on planner statistics, S3.3 passes, S3.2 did not run; the pinned pg17 has not run
answers: what M0 spike 3 found for S3.1–S3.4 (FORCE and the owner, the owner/service split, SET LOCAL across pooled transactions, the hostile cross-tenant probe), on which versions, what failed and why, and what each finding asks of the design
-->

# Findings: spike 3, RLS hardening

**Status:** recorded findings · 2026-10-31 — on PostgreSQL 18.6 S3.1 and S3.4 fail for the table owner on planner statistics, S3.3 passes, S3.2 did not run; the pinned pg17 has not run

These are the findings for the four scenarios of the critical path's
[spike 3](../../product/critical-path.md#spike-3-rls-hardening), one per scenario, in its
template. The harness is `testing/spikes/rls/spike_rls.py`, and its decisions (roles, seeding,
controls, recorded failures) are on the [M0 spikes](index.md) page.

**Where they ran.** On the host PostgreSQL 18.6, as a role that can create databases but not
roles. The connecting role therefore owned the database and every table, ran S3.1, S3.3 and
S3.4 as that owner, and could not run S3.2. The pin is `pgvector/pgvector:pg17`. CI job
`rls-spike` runs all four scenarios there as an owner role and a separate service role, and
that job has not run yet. None of the verdicts below is a pg17 result, and none of them is a
result for a non-owner role.

| Scenario | Verdict on 18.6 (owner role) | Pinned pg17, owner and service roles |
|---|---|---|
| [S3.1](#s31) FORCE binds the owner | **fail**: table reads are filtered, planner statistics are not | not yet run (CI `rls-spike`) |
| [S3.2](#s32) owner/service split, no BYPASSRLS | **inconclusive**: not run here, the role cannot create roles | not yet run (CI `rls-spike`) |
| [S3.3](#s33) SET LOCAL across pooled transactions | **pass**: a read with no tenant returns 0 rows | not yet run (CI `rls-spike`) |
| [S3.4](#s34) hostile cross-tenant probe | **fail**: every read shape of the fold tables returns 0 rows; planner statistics do not | not yet run (CI `rls-spike`) |

The two failures are one fact, seen from two scenarios. Both are entries in the harness's
`RECORDED_FAILURES`, so CI stays green while they reproduce and fails when they stop.

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
adr-clause:    ADR-0007 d2, through the FORCE commitment of security.md (commitments C6)
action:        no ADR amendment, on this finding's reading, which the maintainer confirms
               (below); recorded in the harness as S3.1/owner/pg_stats
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
on the recorded list, so a leak there fails CI).

**Observed.** The owner can also lift `FORCE` with `ALTER TABLE … NO FORCE ROW LEVEL SECURITY`, as
the control shows. `FORCE` binds what the owner reads from the table. It does not bind the
owner's DDL, and it does not bind the owner's view of the catalog. Both are reasons the querying
role must not be the owner, and that is S3.2's split.

**Reading for the action.** ADR-0007 d2 binds the roles OML workloads use, and security.md's
split keeps those roles off ownership: "migrations run under a separate owner role no workload
uses". If the split holds, the leak above reaches only the role that runs migrations, and that
role can lift `FORCE` anyway. So this finding asks for no amendment to the decision. It does
ask for three things:

- **The split is not built.** On 2026-10-31, `oml-server-lead` 0.2.0 (commit `030d7b8`) was started
  against a throwaway database. It ran the migrations on boot over its own `postgres.url`, and its
  role then owned all 11 tables. As built, the role that serves tenant requests is the owner, so
  the statistics leak and the `NO FORCE` capability both reach it. Using either still needs SQL
  that the server does not issue.
- **A page edit for security.md**, proposed and not made here: say that `FORCE` binds an owner's
  table reads but not its view of planner statistics or its DDL, and that the split, not `FORCE`,
  is what keeps those from a workload.
- **`_sqlx_migrations`.** The lead's sqlx migrator creates this table, with neither RLS nor
  `FORCE`. It holds no tenant data, but security.md says every Postgres table carries a policy
  and that "there is no table exempted for convenience". Either that sentence or that table
  needs to change. The harness applies the SQL files directly, so its database has no such
  table.

## S3.2 — owner/migration-role split, no BYPASSRLS {#s32}

```text
spike-finding: S3.2
date:          2026-10-31
versions:      PostgreSQL 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1), the host's; the pinned
               pgvector/pgvector:pg17 not yet run; psycopg 3.3.6 (libpq 18.6),
               psycopg-pool 3.3.3, Python 3.13.13, uv 0.11.6
settings:      as S3.1
environment:   as S3.1; the connecting role has CREATEDB and neither CREATEROLE nor
               superuser, so the harness cannot mint the owner and service roles
verdict:       inconclusive: not run on this host
adr-clause:    ADR-0007 d2 (no BYPASSRLS role), through the role split of security.md
               (commitments C6; C3 for the journal re-grant)
action:        run in CI job rls-spike on the pinned pg17 with --require-all, and record
               that result here
```

**What did run.** The half of S3.2 that needs no second role ran. Migrations 0001–0005 succeed
under the owner role while `FORCE` binds it, over two tenants' rows seeded between 0003 and 0004.
The backfills of 0004 and 0005 reach both tenants: every key's `issued_seq` and `issued_by` match
its `key.issued`, and every memory's `version`, `kind`, `body_bytes`, `is_tip` and `promoted_at`
match what 0005 derives. Each migration enumerates tenants through 0003's startup policy and runs
under the ordinary tenant context, as its comments say, and `FORCE` does not stop it.

**What CI asserts** once it runs: neither role is a superuser or holds `BYPASSRLS`, `CREATEROLE` or
`CREATEDB`; the service role owns no relation and is not a member of the owner role. As the
service role, tenant A sees only A's rows and no tenant sees 0 rows. Every one of these is refused
with `42501`: `NO FORCE`, `DISABLE ROW LEVEL SECURITY`, `DROP POLICY`, `CREATE POLICY`, `OWNER
TO`, `UPDATE`, `DELETE` and `TRUNCATE` on `journal`, and `SET ROLE` to the owner. `SET row_security =
off` raises instead of unfiltering, and a self-`GRANT UPDATE ON journal` leaves the privilege
absent. The control is a `NOLOGIN BYPASSRLS` role, which must read tenant B's rows while set to
tenant A.

**Observed elsewhere.** The design's split exists only in this harness. The shipped lead migrates
on boot as its serving role (S3.1, above), so for the PoC as built S3.2's premise does not hold.

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
               pg_stats does not (S3.1's failure, seen from a tenant session)
adr-clause:    ADR-0007 d2 and d6 (RLS as the second fence, judged against the hostile
               co-tenant)
action:        as S3.1; recorded in the harness as S3.4/owner/pg_stats; the observations
               below go to M6's security pass, and one goes to FR-4 and FR-7
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

**What failed.** `pg_stats`, as in [S3.1](#s31): the session set to tenant A reads tenant B's
lexemes in the statistics of `memories_claim_fts`. No read shape of the fold tables touches
`pg_stats`, so the leak needs SQL the server does not issue.

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
- **The planner's estimate is an oracle.** Under tenant A, `EXPLAIN` estimates 2 rows for a word
  only tenant B's memories hold and 1 row for a word no tenant holds, and both queries return 0
  rows. `pg_class.reltuples` for `memories` reads 8, both tenants' rows, while tenant A sees 4.
  Catalog statistics span tenants. security.md lists timing and contention as the side channels
  v1 does not address, and it does not list these.
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

- **ADR amendment needed: none, on this page's reading.** S3.1 and S3.4 fail only for the table
  owner, which the decided split keeps off every workload (S3.1, "Reading for the action"). If the
  maintainer reads ADR-0007 d2, or security.md's `FORCE` sentence, as promising that `FORCE` alone
  fences the owner, the failure breaks that promise, and an amendment is needed.
- **Implementation:** the lead's migrate-on-boot makes the serving role the owner. The split is
  what C6 decides, and it is not built.
- **Page edits, proposed and not made:** security.md, on what `FORCE` binds; postgres-schema.md,
  on the policy predicate form and the zero-row behaviour; security.md or the sqlx table, on
  `_sqlx_migrations`.
- **A decision to weigh:** whether a GIN index under RLS is still the FTS strategy, given that RLS
  keeps it out of every plan.
- **The pinned run:** CI job `rls-spike` on `pgvector/pgvector:pg17` decides S3.2 and the
  service-role half of S3.3 and S3.4, and these findings are updated with its result.
