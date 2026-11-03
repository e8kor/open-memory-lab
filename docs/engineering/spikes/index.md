<!--
id: engineering/spikes/index
tier: engineering
status: design draft for review · decided — the chain-vector cross-check and the spike 3 harness are built; spikes 1, 2 and 4 are not run
answers: how the critical path's M0 experiments run in this repository — which independent libraries reproduce the chain-format test vectors, how the RLS spike builds its roles and data, where each finding is recorded, and which spikes have not run
-->

# M0 spikes

**Status:** design draft for review · decided — the chain-vector cross-check and the spike 3 harness are built; spikes 1, 2 and 4 are not run

The [critical path](../../product/critical-path.md) owns M0: the four spikes, their scenario
matrices, the pass assertions and the recorded-finding template. This page does not restate
them ([ADR-0018](../../adr/ADR-0018-the-documentation-set.md) d5). It decides what the critical
path leaves open: the tools and libraries each experiment uses, how the RLS spike gets its roles
and data, where a finding is written, and what CI runs. It also lists which experiments have run.

## What has run

| Experiment | Harness | Finding | State |
|---|---|---|---|
| Chain-format test vectors, cross-checked outside Rust (M0 exit) | `scripts/check_chain_vectors.py` | [chain vectors](chain-vectors.md) | run 2026-10-31; CI job `chain-vectors` defined |
| Spike 1: delta-rs transaction-identifier conformance (S1.1–S1.6) | none | none | **not run**: it needs a pinned MinIO container and a delta-rs harness, and the host that recorded these findings has no Docker |
| Spike 2: conditional put under degraded quorum (S2.1–S2.4) | none | none | **not run**: it needs a four-node MinIO and an S3 account, and none is available here |
| Spike 3: RLS hardening (S3.1–S3.4) | `testing/spikes/rls/spike_rls.py` | [spike 3](spike-3-rls.md) | run 2026-10-31 on PostgreSQL 18.6 as the table owner: S3.3 passes; S3.1 and S3.4 fail on planner statistics, which FORCE does not filter; S3.2's mechanism needs a role that can create roles, so it runs only in CI job `rls-spike` on `pgvector/pgvector:pg17`, which has not run yet; S3.2's claim about OML's own roles is open on every host, because no deployment's roles are specified |
| Spike 4: pgvector ceiling probe (S4.1–S4.4) | none | none | **not run**: it needs the pinned `pgvector/pgvector:pg17` image with a sync standby, and the local cluster has no pgvector |

A scenario without a recorded finding is open. M0's exit stays open until every row of the
critical path's matrices has a finding on the pinned versions.

## Decisions

### The vectors are read from the page that declares them

`scripts/check_chain_vectors.py` reads the test key's PEM blocks, each vector's stated
`signed_bytes`, its byte count, `hash` and `sig`, and the stated raw public key and fingerprint
from [chain format](../../data/chain-format.md) itself, and the page stays the fixture every tier
verifies ([test strategy](../test-strategy.md#test-data-and-fixtures)). The script holds the vectors'
**inputs**: the nine signed fields of each event, in a member order that is deliberately not
canonical, so that the canonicalizer does the sorting. It also holds the body plaintext and the
stand-in rule of vector 2. It does not copy derived values. It computes `plaintext_digest` and
`ciphertext_digest` from the body, the genesis `prev_hash` from 32 zero bytes, and each later
`prev_hash` from the hash it computed for the previous vector. If one of its inputs no longer
matches the value the page's bytes carry, the script names that field and fails, so the inputs
cannot drift silently from the page.

### The Rust tests' copies are compared with the page

The Rust tests keep their own copies of the vectors as literals.
`crates/oml-domain/tests/vectors.rs` and `crates/bins/oml-cli/tests/vectors.rs` each hold the three
`signed_bytes` and the hashes and signatures, and `crates/oml-domain/tests/keys_and_format.rs`
repeats vector 1's hash. Each Rust test checks the Rust code against its own copy, so until
2026-10-31 the copies agreed with the page only because they had been copied by hand: had one
drifted, the Rust tests and this check would both have stayed green. The script therefore also
reads every `*.rs` file in the repository, outside `target/` and hidden directories, and does not
change any. A file that names a vector's event id, the vector tenant, the test key, or a stated
hash or signature is a copy. In a copy:

- every raw string literal that carries a vector's `"event_id"` must be that vector's
  `signed_bytes`, byte for byte;
- a vector's JSON outside a raw string literal fails the check, because the check does not
  compare it there;
- every hex run of 64 or 128 digits must be a value the page states or the script derives from
  it: a hash, a signature, the public key, the fingerprint, the PKCS#8 seed, the zero hash, or one
  of vector 2's digests.

With `cargo test` in `ci.yml`, that links the two implementations: the Rust code reproduces the
Rust literals, the literals equal the page, and the page equals what the Python libraries compute.
The better end state is Rust tests that read the page, so that no copy exists. That change is in
`crates/`, which `feat/iteration-3` is rewriting, so it is a follow-up for that branch, not part
of this one. When the copies are gone, the check reports that no Rust file holds a copy, and it
passes.

### Two independent implementations of each primitive, none of them Rust

| Primitive | Implementations | Why both |
|---|---|---|
| RFC 8785 (JCS) | `rfc8785` (Trail of Bits, pure Python); `jcs` (Anders Rundgren's reference code, packaged by titusz) | Rundgren is an author of RFC 8785, and the two codebases share nothing |
| RFC 8032 Ed25519 | `cryptography` (calls OpenSSL); PyNaCl (calls libsodium) | the page's signatures came from OpenSSL 3.5.5, so an OpenSSL-only check could agree with the page by sharing its generator; libsodium shares nothing with OpenSSL or with `ed25519-dalek` |
| SHA-256 | `hashlib` (calls OpenSSL); libsodium (through PyNaCl) | the page's hashes came from `hashlib`, so libsodium is the independent one |

Pure Ed25519 is deterministic (RFC 8032 §5.1.6), so each Ed25519 library signs every
reproduced `signed_bytes` with the test key and must produce exactly the stated `sig`. Each
library also verifies the stated `sig` against the stated public key. The script then runs the
page's [verification procedure](../../data/chain-format.md#verification) steps 1–7 and 9 over the
three events. It also runs the page's own claim in reverse: it flips one byte in each
`signed_bytes`, puts the events in every other order, and drops each event in turn, and each of
those mutations must fail the steps the page names. A check that cannot fail is not counted as a check.

### Libraries are pinned through uv, inline

Both harnesses are single-file scripts with [PEP 723](https://peps.python.org/pep-0723/) inline
metadata that pins each direct dependency exactly. Each has a committed lock file
(`<script>.lock`, written by `uv lock --script`), and runs as
`uv run --locked --script <script>`. `--locked` refuses to run when the lock file and the inline
pins disagree. The doc gates keep using the system `python3`, which these two scripts do not
need.

### Spike 3 applies the migrations itself, with data between them

`testing/spikes/rls/spike_rls.py` (psycopg 3 and psycopg-pool, pinned as above) creates a
throwaway database with a unique name, applies `migrations/*.sql` in filename order, and drops
the database when it ends, whatever the result. It seeds two tenants after `0003` and before
`0004`. That way the backfills in `0004` and `0005` run over rows of both tenants while
`FORCE ROW LEVEL SECURITY` binds the role running them, and "migrations succeed" covers more
than an empty schema. Every text value of tenant B carries a marker string. The hostile probe
fails if a B marker, B's tenant id or any of B's row ids appears in its results.

Every seed row goes in through one helper, which records the table it writes. The checks that
need rows in a table (each tenant sees its own rows and none of the other's) apply to the
recorded tables. A table that a later migration adds, and that no seeder writes, still gets the
checks that need no rows: `ENABLE` and `FORCE` and a policy (S3.1), zero rows with no tenant
set, and its full scans in S3.4. Its finding reports it as **unprobed for row visibility**, and
so does the run's last line. An unprobed table does not fail the run. It also is not evidence:
no row of either tenant exists in it, so no read of it could show a leak, until a seeder writes
it.

### The role split is built when the connecting role can build it

The critical path's S3.2 needs an owner role and a separate application role. The harness mints
roles only when the connecting role is a superuser:

- **CI** (`pgvector/pgvector:pg17`, whose `POSTGRES_USER` is a superuser): the script mints an
  owner role and a service role with unique names, both `NOSUPERUSER NOBYPASSRLS`, and creates
  the database owned by the owner role. It runs the migrations as the owner, then grants the
  service role DML on every table except `journal`, where it gets `SELECT, INSERT` only (the
  append-only rule of [ADR-0002](../../adr/ADR-0002-postgresql-system-of-record.md) d4). It runs
  S3.1 as the owner, S3.2's mechanism as both, and S3.3 and S3.4 as both roles. Both roles are
  dropped at the end.
- **Elsewhere** (a role that is not a superuser and can create databases, as on the host that
  recorded the first findings):
  the connecting role owns the database and runs S3.1, S3.3 and S3.4 as the owner, which
  `FORCE` binds. S3.2's mechanism is reported **not run** with its reason. The script never
  escalates.
- `--require-all` turns a scenario that did not run into a failure. CI passes it, so a CI run
  cannot pass with a scenario skipped.

### S3.2 is two findings, and the one about OML's roles stays open

The critical path's S3.2 asserts that migrations succeed and that "no application role can read
across the policy". The harness can test PostgreSQL's part of that and cannot test OML's part,
so it reports them as two findings:

- **S3.2 (mechanism):** a role that does not own the tables, holds DML grants and has no
  `BYPASSRLS` reads only its tenant's rows. It cannot lift `FORCE`, disable RLS, drop or create a
  policy, take ownership, `SET ROLE` to the owner, grant itself `UPDATE` on `journal`, or turn
  `row_security` off. With the `journal` grants above, it cannot update, delete or truncate the
  journal. Migrations succeed under the owner while `FORCE` binds it. This finding can pass.
- **S3.2 (OML's roles):** the roles an OML deployment uses are split from the owner, and none
  holds `BYPASSRLS`. The harness mints its own roles and writes their grants. Checking their
  attributes would only show that it ran its own `CREATE ROLE` statements, so it does not do
  that. It treats them as preconditions of the mechanism finding: if they are not as minted, that
  finding is inconclusive. OML has no roles of its own to test.
  [Deployment](../../operations/deployment.md) names the bootstrap workload's owner role and a
  service role, and [configuration](../../operations/configuration.md) designs `postgres.owner_url`.
  No page states those roles' attributes and grants, and the lead as built migrates and serves as
  one role ([spike 3](spike-3-rls.md#s31)). This finding is **inconclusive, not testable** on
  every host, CI included. It does not fail the run, and the run's last line names it. It stays
  open until a deployment's bootstrap specifies roles and grants. The harness then takes them
  from there, in place of its own.

### Every scenario carries a control that must fail

Each scenario runs the same assertion once against a weakened setup, and that run must
observe the leak. S3.1 lifts `FORCE` on one table inside a transaction it rolls back. S3.2's
mechanism runs, as the owner and rolled back, each DDL statement the service role was refused,
and each must take effect, so that each refusal comes from the role and not from the statement.
It also counts the service role's rows with the tenant set to B. S3.3 sets the tenant with
session-level `SET`. S3.4 runs every probe again
with `oml.tenant_id` set to B, and hands its probe function B's rows. If a control does
not leak, the assertion could not have caught the failure it exists for, and the script reports
the scenario as inconclusive.

### A recorded failure is expected, strictly

A finding can be a failure. If CI then stayed red, it would stop telling anyone anything, and if
the failing check were removed, the finding would vanish with it. So the harness keeps a short
list of failed checks that a finding records (`RECORDED_FAILURES` in the script, each entry naming
its finding's anchor). A listed check that fails prints `FAIL, recorded` and does not fail the
run. A listed check that passes, or does not run, does fail the run, so a fixed failure gets its
finding re-recorded instead of going stale. A failure not on the list fails the run as usual.
This is a strict expected failure: the scenario's verdict stays `fail`, and only the exit code
treats a known result as known. A recorded failure belongs to the role that showed it.
`S3.4/owner/estimate` is recorded, and `S3.4/service/estimate` is not, because no run has shown
it, so CI fails if the service role sees the same estimates (as the
[spike 3 findings](spike-3-rls.md#s34) expect). Recording a failure does not decide it. A failed scenario's
`action` line, on its finding and in the script's output, says **ADR amendment needed** with its
clause and "awaiting maintainer decision" until the maintainer decides. The harness never prints
`none` for a failure.

### Findings live here, one block per scenario

Each finding is the critical path's template, filled in, with an evidence list under it. It sits
on the experiment's page under this directory, and the critical path links to the pages. The
`versions` field names the Postgres the run used. A run on a version other than the pin is
recorded as that version's result and does not stand for the pin. When a scenario breaks an ADR
clause, its finding says **ADR amendment needed** and gives the evidence. The amendment itself is
a separate record, and this page does not write it.

### CI runs both as new jobs

`.github/workflows/m0-spikes.yml` holds two jobs. `chain-vectors` runs the vector check.
`rls-spike` runs spike 3 against a `pgvector/pgvector:pg17` service with `--require-all`. Both
use the pinned uv and Python versions the workflow names. They keep the critical path's
findings reproducible on every pull request. That is what
[test strategy](../test-strategy.md#integration-tier) asks of spike 3 when it says "kept green
forever after", and it ties `rls-spike` to `migrations/` (next section).

### The harness follows the migrations

`rls-spike` applies whatever `migrations/` holds, so a pull request that adds a migration runs the
spike against it. A new table is reported unprobed, as above, and does not fail the job. A new
migration does need the harness updated in the same change when any of these holds:

- it renames or constrains a column that a seed statement writes (seeding happens after 0003 and
  after the last migration);
- it renames or drops a column that S3.4's copies of the adapter's read shapes name (taken
  from `crates/adapters/oml-pg/src/lib.rs` at `02a385d`), or the adapter's shapes change;
- it changes a recorded failure, for example by dropping the expression index
  `memories_claim_fts`. The stale entry then fails the run, and the finding is re-recorded.

The harness was run against the in-flight rewrite. On 2026-10-31 it ran on the host's 18.6, as
the owner, over this branch's 0001–0005 plus `migrations/0006_names.sql` from `feat/iteration-3`
at `65c07dc`, copied into scratch with `--migrations`. That migration adds the RLS table `crews`,
moves `tenants.name` and `workareas.name` to `prior_name`, and adds a `name` column of the domain
`oml_name`. With the harness as first committed, that run failed in two places. S3.1 reported
`crews` empty for both tenants. S3.3's no-tenant write was refused with `23514`, because its
workarea name was not an `oml_name`, so the policy never got to refuse it with `42501`. With the
harness as it stands, the run gives the verdicts of the 0001–0005 run and reports `crews` as
unprobed. Three changes made that happen. The non-empty checks apply to seeded tables only. The
no-tenant write uses a row that every constraint of either schema accepts. The non-leakproof
probe function is handed whole rows, where before it was handed named columns that 0006 leaves
NULL. That run does not cover the pinned pg17 or the service role. When `feat/iteration-3`
merges, `crews` stays unprobed until a seeder writes it. The same run, repeated on 2026-10-31 after
S3.2 became two findings and the estimate became a check, gave the verdicts of the 0001–0005 run
again, with the estimate's recorded failure among them, and reported `crews` as unprobed.

## Rejected alternatives

- **A JSON fixture file holding the vectors.** It would be a second declaration site for bytes
  that chain format already states, and the two would drift (ADR-0018 d5).
- **Leaving the Rust copies to the Rust tests.** Each Rust test compares the Rust code with its
  own copy, so a copy that drifted from the page would keep both suites green, and the
  cross-check would corroborate the page but not the code.
- **A list of the files that hold copies.** A list misses a copy added in a new file. Finding the
  copies by the values they hold catches one wherever it is added.
- **Reporting S3.2 as one finding with the harness's role attributes as its checks.** A CI run
  would then print S3.2 `pass` with action `none` for a property that OML's roles do not yet
  have.
- **A `BYPASSRLS` role as S3.2's control.** It shows only that `BYPASSRLS` bypasses RLS, and no
  check of S3.2 could have failed for want of it.
- **Re-checking with the OpenSSL CLI alone.** OpenSSL produced the page's signatures, so this
  would confirm the page against itself. It also has no JCS.
- **A JCS written for the script.** It would confirm only that the script agrees with its
  author's reading of RFC 8785, which is the self-confirmation M0's exit forbids.
- **Spike 3 as a Rust integration test.** The spike tests what Postgres does with these
  migrations, not the adapter. A Rust test in `crates/` would also land in code that is being
  rewritten. The integration tier will carry these assertions in Rust once the adapter settles
  ([test strategy](../test-strategy.md#integration-tier)).
- **pgTAP.** The pinned image does not ship it, and installing an extension needs the superuser
  that this harness avoids outside CI.
- **psql scripts.** A psql script cannot hold one pooled connection across interleaved
  transactions, capture notices from a probe function, or tell a typed error from zero rows
  without parsing its output.
- **A private superuser cluster on the developer host, to run S3.2 locally.** It would add a
  third environment that neither the pin nor the deployment uses, to save one CI run.
- **Scoping `rls-spike` by a path filter, or running it advisory until the rewrite lands.** The
  branch it would spare is the one that changes `migrations/`. A path filter on `migrations/`
  would still run it there, and a path filter is set per workflow, so it would also scope
  `chain-vectors`. An advisory job would show a real cross-tenant leak as green.
- **Requiring every table to hold seeded rows.** That would fail every migration that adds a
  table until the harness seeds it, which here means the in-flight rewrite. Reporting the table
  as unprobed keeps the gap in the output without blocking.
