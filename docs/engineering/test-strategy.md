<!--
id: engineering/test-strategy
tier: engineering
status: design draft for review · decided, not built
answers: which test tier proves which claim, which named properties and conformance ids are citable, where test data comes from
-->

# Test strategy

**Status:** design draft for review · decided, not built

This page decides how OML is tested before any test exists. It assigns every class of claim to
exactly one tier, names the properties and conformance tests so other pages can cite them by id,
and fixes the test-data policy. The structure follows the hexagon
([ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md)): the constitution is proven
where it lives (the pure core), adapters are proven against their port contracts, and the composed
system is proven against real infrastructure. Nothing here is built; every "passes" below is a
design commitment, and the rows that will hold the first real results are
[FR-3](../product/roadmap.md), [FR-22](../product/roadmap.md) and the invariant-test exit of
[critical path](../product/critical-path.md) M1.

## The tiers

| Tier | Subject | Infrastructure | Where it runs |
|---|---|---|---|
| Domain properties | `oml-domain`, the pure core | none — no database, no clock, no I/O | every `cargo test`, every PR |
| Port contracts | every adapter and every test double, per port trait | the adapter's own backend (containers) for real adapters; none for doubles | every PR (doubles), container jobs (adapters) |
| Integration | composed binaries against real Postgres + MinIO | containers, per object-store backend | container jobs, nightly and pre-merge |
| Crash harness | the lake-writer's exactly-once claim ([FR-3](../product/roadmap.md)) | real Postgres + per-backend object store, kill injection | nightly and per release |
| Conformance | the three install profiles as one product ([FR-22](../product/roadmap.md)) | a running deployment of each profile | per release; the dated three-column matrix |

The tiers are ordered by cost, and the ordering is the point: a constitution bug must be
reproducible in milliseconds without a container, which is why
[ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d1 keeps the core pure — the
core being testable without infrastructure is the cheapest correctness instrument the design owns.

## Domain properties

`oml-domain` carries a property-test suite (proptest) in which every constitution invariant and
every chain and erasure invariant is a **named property**. The names below are design identifiers
— normative in shape, adjustable in spelling at implementation without a design change (the
[schema page](../data/postgres-schema.md)'s convention) — and once a property exists, other pages
may cite it by id instead of re-asserting the invariant.

| Property | Statement under test | Declared in |
|---|---|---|
| `P-R1-CEILING` | for every role and every generated verb sequence, no verb outside the role matrix is reachable — a scanner outcome type has no promoting arm, an agent surface has no sign/demote/delete arm | [ADR-0001](../adr/ADR-0001-the-constitution.md) d1 |
| `P-R2-NONAUTHOR` | no generated event sequence produces a ratification whose verdict principal appears in the author's derivation lineage closure, pending and ratified edges alike | [ADR-0001](../adr/ADR-0001-the-constitution.md) d2 |
| `P-R3-VISIBILITY` | a held row never appears in any read result computed for a key other than its author or the reviewer it is currently served to, or, inside an in-progress subtree at a scope inside it, a key the [permission matrix](../architecture/roles-and-keys.md#permission-matrix)'s footnote 6 allows | [ADR-0001](../adr/ADR-0001-the-constitution.md) d3, the proposed [ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d5 |
| `P-R3-BOUNDARY` | no read scoped outside an in-progress subtree returns, counts or names a row inside it before its commit | the proposed [ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d10 |
| `P-UNREVIEWED-BLOCK` | a key with a blocking reader grant effective on node N never reads another key's held row on N | the proposed [ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d7 |
| `P-COMMIT-HELD` | no `workarea.status_transition` with `in_progress_released: true` is journalled while any version in the subtree is held, including under a concurrent capture or reopen | the proposed [ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d12 |
| `P-R4-NO-SILENT-DELETE` | every transition that removes a row from a read surface is preceded, in the same fold, by a journalled act (demotion, erasure, simulation end) | [ADR-0001](../adr/ADR-0001-the-constitution.md) d4 |
| `P-EXC-BOUNDED` | `self_signed` rows are readable only inside their development workarea, never promote beyond it, and never qualify for silver or gold materialization | [ADR-0001](../adr/ADR-0001-the-constitution.md) d5 |
| `P-CHAIN-GENESIS` | the first event of every tenant chain is `tenant.genesis` at seq 1 with `prev_hash` equal to 32 zero bytes; no other event may carry that `prev_hash` form | [chain format](../data/chain-format.md) |
| `P-CHAIN-LINK` | for every generated chain, each event's `prev_hash` equals the SHA-256 of the previous event's `signed_bytes`, seq is contiguous, and re-canonicalizing (RFC 8785 JCS) then re-hashing reproduces the stored hash | [chain format](../data/chain-format.md) |
| `P-CHAIN-SIG` | every signature verifies as pure Ed25519 (RFC 8032) over `signed_bytes` — not over the hash — under the signing key of the event's era | [chain format](../data/chain-format.md) |
| `P-ERASE-COMPLETE` | after an erase event's fold, every copy class in the erasure inventory is purged or key-destroyed, and the attribution fields the design commits to keeping survive | [ADR-0005](../adr/ADR-0005-erasure-by-crypto-shredding.md), [audit and lineage](../architecture/audit-and-lineage.md) |
| `P-TAINT-MONOTONE` | a claim whose lineage closure touches any simulation row is tainted; taint never disappears by adding edges, and an untainted result never cites a tainted row outside its audience | [ADR-0011](../adr/ADR-0011-simulations.md) |
| `P-ALGEBRA-DENY-WINS` | effective access equals the union of a key's grants, each attenuated by its own deny entries, masked by global key denials — and no generated grant/deny combination widens access | [workareas](../architecture/workareas.md) |
| `P-LADDER` | workarea status transitions are exactly the ladder's declared transitions; a universe root never leaves `committed` and never accepts a direct claim write | [workareas](../architecture/workareas.md) |

Two notes bind the suite's honesty:

- `P-R2-NONAUTHOR` proves closure over the **declared** graph only. Undeclared derivation is
  undetectable by design ([ADR-0001](../adr/ADR-0001-the-constitution.md) d6), and the property's
  documentation carries that caveat verbatim.
- The chain properties run twice: once over generated chains, and once over the committed
  cross-implementation test vectors that [chain format](../data/chain-format.md) specifies and
  [critical path](../product/critical-path.md) M0 generates. Property tests alone would only prove
  the Rust implementation agrees with itself; the vectors are what make the format a format.

## Port contracts

[ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d2 makes every infrastructure
dependency an adapter behind a trait. The test consequence is the **port law**: for each port
trait there is exactly one contract suite, and **every implementation of the trait — the real
adapter and its in-memory test double alike — runs the identical suite**. The suites live in a
dedicated workspace crate (`oml-port-contracts`, [workspace plan](workspace-plan.md)) as generic
functions over the trait, so a new adapter inherits its obligations by construction and cannot
pass a weaker test than the double it replaces.

The law exists because the doubles are what the domain and application tests run against on every
PR: a double that drifts from the real adapter's semantics silently invalidates every test above
it. The contract suite is the drift detector. Contracts assert the semantics the design already
states, per port — for the journal-and-metadata store port, for example: commit atomicity (the
one-transaction commit point of [ADR-0002](../adr/ADR-0002-postgresql-system-of-record.md)),
per-tenant seq assignment under contention, idempotency-record claim-before-side-effect, and the
append-only refusal. Real-adapter contract runs use the adapter's actual backend in a container;
the sqlx adapter's run doubles as the compile-time query check against the migrated schema
([CI and toolchain](ci-and-toolchain.md)).

## Integration tier

Integration tests drive composed binaries — not traits — against real Postgres and real MinIO in
containers, plus the `file://` backend, matching the three object-store backends
[FR-22](../product/roadmap.md) names. This tier owns what no pure test can see: RLS behaviour
under pooled connections (the [critical path](../product/critical-path.md) M0 spike 3 assertions,
kept green forever after), advisory-lock serialization under concurrency, outbox stable-prefix
draining across concurrent committing transactions, and the admission pipeline end to end
including quarantine placement. Container pins follow the
[deployment](../operations/deployment.md) page's pins — one fact declared once, consumed by
compose, chart and test rig alike.

## The FR-3 crash harness

The exactly-once claim of [storage](../architecture/storage.md) is tested by destruction, not by
review. The harness (`oml-crash-harness`, [workspace plan](workspace-plan.md)) supervises a real
lake-writer against a seeded journal and kills it at **named kill points**, then restarts it and
compares the lake against the journal fold:

1. after the outbox batch read, before any Delta write;
2. mid-batch, after some but not all per-table Delta commits;
3. after a Delta commit, before the frontier-cache update — the window where the Delta
   transaction id, not the cache, must be the recovery authority
   ([ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md) d4);
4. during checkpoint writing, and during `VACUUM` — the cases
   [FR-16](../product/roadmap.md) added, because `SetTransaction` expiry would silently reset
   recovery to "never applied".

The invariant asserted after every kill schedule, on every backend (`file://`, pinned MinIO, S3):
the lake equals the journal fold exactly — no event applied twice, none lost — and the dead-letter
path is the only exit. Kill points are reached by fault injection at the port boundary (the
hexagon pays for itself here: a kill point is a failing adapter wrapper, not a patched binary).
Passing runs are what let the documentation write "exactly-once, verified by conformance test
`CONF-LAKE-001`" instead of "exactly-once"; until the harness runs, every such sentence keeps its
unverified marker per [ADR-0018](../adr/ADR-0018-the-documentation-set.md).

## The FR-22 conformance suite

The conformance suite is a **profile-parameterized binary** (`oml-conformance`): one program,
pointed at a running deployment, taking the profile and backend as parameters —
`oml-conformance --profile standalone|docker|kubernetes --backend file|minio|s3`. It is the
instrument that closes [FR-22](../product/roadmap.md)'s three-column matrix and, once the provider
ring freezes, the covenant guard of [FR-21](../product/roadmap.md): every `/v1` operation is
exercised by a conformance test that future changes must keep green.

Every test has a **named id**, `CONF-<AREA>-<NNN>`, stable once assigned and never reused — the
same rule the [roadmap](../product/roadmap.md) applies to row numbers, for the same reason: the id
is a citation target. A documentation page may then say "verified by conformance test
`CONF-CHAIN-002`" and the sentence is checkable. The areas mirror FR-22's list:

| Area | Covers | Example ids |
|---|---|---|
| `CONF-CONST-*` | the constitution invariants, observed through the API: verb ceilings per transport, non-author review refusal, held-row invisibility | `CONF-CONST-001` — a writer key's promote attempt is refused on both rings |
| `CONF-CHAIN-*` | commit and chain semantics: genesis shape, contiguity, signature verification against the exported head, chain order equals visibility order | `CONF-CHAIN-002` — an exported chain verifies against the [chain format](../data/chain-format.md) test vectors' rules |
| `CONF-ADMIT-*` | admission end to end: hold, scan veto, serve binding and expiry, idempotent replay inside the committed 24-hour minimum window | `CONF-ADMIT-003` — same idempotency key, different digest is the typed error |
| `CONF-ERASE-*` | erasure end to end across the copy classes reachable in the profile | `CONF-ERASE-001` — post-erase, body unrecoverable, attribution intact |
| `CONF-LAKE-*` | the FR-3 crash harness, run per backend, including the NFS refusal | `CONF-LAKE-001` — the kill-schedule equality above |
| `CONF-V1-*` | every provider-ring operation, the FR-21 covenant surface | `CONF-V1-001` — submit → review → promote → recall round trip |

A cell in FR-22's matrix is the dated output of one full run; the suite emits a machine-readable
report in the same shape the [benchmark harness](benchmark-harness.md) emits, so the matrix is
generated, never hand-maintained.

## Test data and fixtures

- **No production data, ever.** Test corpora are generated, and every corpus commits its
  generation recipe — seed, generator version, size and claim-kind mix — next to the generator
  (`oml-corpus`, [workspace plan](workspace-plan.md)). A fixture that cannot be regenerated from
  its recipe is a defect.
- Generated text is NFC-normalized at generation to match admission's normalization rule, so
  fixtures cannot mask a normalization bug by already being normalized differently.
- Chain fixtures are the [chain format](../data/chain-format.md) test vectors, never ad-hoc
  chains, so every tier verifies the same bytes.
- Benchmark corpora and their recall ground truth share this policy and are specified in the
  [benchmark harness](benchmark-harness.md); the recall corpus recipe is committed for the same
  reason test recipes are — a number nobody can regenerate is a number nobody can check.
- No public dataset is a dependency of any tier unless a page names it, with its license; as of
  this design none is named.
