<!--
id: product/critical-path
tier: client
status: design draft for review · decided, not built
answers: which milestone must exist before which, M0 through M6, and why — build order and dependency without dates
-->

# Critical path

**Status:** design draft for review · decided, not built

This page orders the build. It carries no dates: no team sizing exists, so a duration would be
an invented number, and the [roadmap](roadmap.md)'s rule against invented numbers applies to
schedules too. What this page commits is **order and dependency** — which milestone must exist
before which, and why. A milestone is done when its exit criteria close — mostly roadmap FR
rows with verifiable criteria — not when its code merges.

## M0 — De-risking spikes, before any milestone

**Goal.** Retire the ecosystem assumptions that could invalidate recorded decisions, before
code accumulates on top of them. Each spike is a **runnable experiment**: candidate versions
named up front, a numbered scenario matrix with an exact pass/fail assertion per row, a stated
environment, and a finding recorded in the template at the end of this section — a spike without
an assertion is a demo, and a finding nobody recorded never happened. Version candidates below
are exactly that: candidates at writing time, confirmed or replaced by the recorded finding; no
version is a fact until a finding pins it.

**Shared environment.** All four spikes run on the standalone rig shape: one host, local
SSD/NVMe storage, containerized Postgres and MinIO at the pins the
[deployment](../operations/deployment.md) page declares (`pgvector/pgvector:pg17`; the MinIO tag
the spike itself pins). Spike 2 adds a four-node MinIO topology. The exact host spec is recorded
per finding, never assumed.

### Spike 1 — delta-rs transaction-identifier conformance

Tests [ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md) d4: the Delta log's per-table
transaction identifiers as the **sole** recovery authority. **Candidates:** the newest `deltalake`
(delta-rs) and `object_store` crate releases at spike time — both pre-1.0, so the exact pins are
the finding's first recorded field. Run every scenario on both backends: `file://` (the
standalone profile's backend, [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md))
and pinned MinIO.

| # | Scenario | Pass assertion |
|---|---|---|
| S1.1 | Write N batches carrying txn (appId, version); kill the writer between a Delta commit and the frontier-cache update; restart | no batch applied twice or lost: table contents equal the source fold exactly; the recorded txn version equals the last committed batch |
| S1.2 | Force a checkpoint mid-sequence, then restart | txn appId/version state readable after the checkpoint; the S1.1 equality still holds on resume |
| S1.3 | `VACUUM`, then restart | txn state survives; a failure here **breaks ADR-0003 d4** and triggers the named fallback — a Postgres applied-ledger as recovery authority — as an ADR amendment before M1 |
| S1.4 | Force transaction-identifier retention expiry (retention set below the test's duration) | the expiry behaviour is characterized, and a retention setting is pinned such that expiry cannot occur within the lake-writer's maximum restart gap — the `SetTransaction`-expiry hazard [FR-16](roadmap.md) names |
| S1.5 | Two concurrent commit attempts on `file://` (the local-filesystem conditional-put self-test) | exactly one wins; the loser surfaces a conflict error, never a corrupt log |
| S1.6 | Lake root on NFS | startup **refuses** with a reason, per ADR-0023's NFS refusal — a working write here is a fail, because it would be an unfenced one |

Feeds [FR-3](roadmap.md)'s extended harness, which keeps these cases green forever after.

### Spike 2 — conditional put under degraded quorum

Tests the second fence of [ADR-0006](../adr/ADR-0006-pure-rust.md) d3: conditional PUT
(`If-None-Match`) as the lockless Delta commit mechanism. **Candidates:** a MinIO release at or
after conditional-write support; the spike pins the **exact release tag**, and that one pin is
the fact the compose file and Helm chart consume
([deployment](../operations/deployment.md)). Environment: four-node erasure-coded MinIO, plus S3.

| # | Scenario | Pass assertion |
|---|---|---|
| S2.1 | Two writers race `PUT If-None-Match` on the same key, healthy cluster | exactly one succeeds, the other receives the conditional-put refusal — atomicity holds |
| S2.2 | The same race with one node stopped (degraded but within write quorum) | the same single-winner outcome; any deviation is recorded and the deployment page's quorum guidance amended before M3 |
| S2.3 | delta-rs lockless commit under commit contention on MinIO | conflict resolved through the conditional put: one commit wins, the other retries against the new version — no lock table, no corruption |
| S2.4 | S2.3 repeated on S3 | same assertion |

### Spike 3 — RLS hardening

Tests the assumptions [commitments](commitments.md) C6 names. Cheap; do immediately.
**Candidates:** Postgres 17 via the pinned `pgvector/pgvector:pg17` image.

| # | Scenario | Pass assertion |
|---|---|---|
| S3.1 | `FORCE ROW LEVEL SECURITY` on every tenant table; query as the table **owner** | the owner's reads are filtered too — FORCE binds the owner, which is the clause the design leans on |
| S3.2 | Owner/migration-role split: migrations run under the owner role; application roles carry no `BYPASSRLS` | migrations succeed; no application role can read across the policy |
| S3.3 | `SET LOCAL` tenant discipline in pooled transactions: interleave two tenants' transactions on one connection | the tenant setting never survives a commit or rollback; a query with no tenant set returns zero rows or a typed error (which one is recorded — it becomes the documented behaviour) |
| S3.4 | Hostile cross-tenant probe: a session set to tenant A runs every read shape the fold tables use, hunting tenant B rows | zero rows, in every shape |

### Spike 4 — pgvector ceiling probe

Probes the quantities [FR-4](roadmap.md) and [FR-7](roadmap.md) will measure properly, early
enough to catch a wall. **Candidates:** the pgvector version bundled in the pinned pg17 image;
corpus and dimension values are generator parameters recorded per finding (the embedding model
itself is open — [RS-1](roadmap.md)), reusing the corpus recipes of the
[benchmark harness](../engineering/benchmark-harness.md).

| # | Scenario | Pass assertion |
|---|---|---|
| S4.1 | HNSW index build at 100 000 and 1 000 000 vectors, `maintenance_work_mem` varied | build memory and time **recorded** per setting — the spike's output is the recorded curve, not a verdict; an out-of-memory wall at these sizes is the finding that re-opens index strategy before M2 |
| S4.2 | LIST partitions at 1 000 and 5 000 tenants, one HNSW index each | planner time, catalog overhead and per-partition memory recorded; any hard limit found is recorded with its trigger |
| S4.3 | Iterative index scans under a low-selectivity workarea-subtree filter | the iterative-scan caps are characterized; a filtered query returns the full requested k or the shortfall is recorded |
| S4.4 | After a `remote_apply` commit, an HNSW and a tsvector query on the sync standby | both return the new row; the pgvector standby-serving version threshold (claimed 0.7.0 — an [ADR-0024](../adr/ADR-0024-performance-foundation.md) unverified-ledger item) verified against the release notes and recorded. Folds into FR-25 scenario 10 |

### The recorded-finding template

One finding per scenario, committed to the repository and linked from the ADR clause it tests:

```
spike-finding: S<spike>.<scenario>
date:          YYYY-MM-DD
versions:      <exact pins: deltalake, object_store, MinIO tag, Postgres, pgvector, toolchain>
settings:      <every non-default setting the scenario ran under>
environment:   <host CPU/RAM/storage, topology>
verdict:       pass | fail | inconclusive
adr-clause:    <the ADR and clause this confirms or breaks, e.g. ADR-0003 d4>
action:        none | ADR amendment opened (<link>)
```

**Exit.** Every scenario above has a recorded finding; any invalidated decision re-opened as an
ADR amendment before M1 builds on it; and the **chain-format test vectors generated**: the
cross-implementation vectors the [chain format](../data/chain-format.md) page specifies — RFC 8785
(JCS) canonical bytes, SHA-256 hashes, RFC 8032 Ed25519 signatures, and the genesis event
(`prev_hash` of 32 zero bytes, `tenant.genesis` at seq 1) — generated, cross-checked against at
least one independent non-Rust JCS/Ed25519 implementation so the vectors are corroborated rather
than self-confirmed, and committed as the fixtures every later tier verifies
([test strategy](../engineering/test-strategy.md)). **Everything depends on M0's findings
standing.**

## M1 — System of record

**Goal.** The one commit point exists and the constitution is code.

**Items.** The `oml-domain` pure crate with invariant property tests
([ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d1); the Postgres schema
with per-tenant chains, advisory-lock commit and the outbox
([storage](../architecture/storage.md)); a minimal `oml-server` — submit, review, promote,
read-own — with keys, grants and the bootstrap ceremony
([roles and keys](../architecture/roles-and-keys.md)); the CLI minimum
([ADR-0015](../adr/ADR-0015-cli-administration.md)); the **standalone profile**
([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)) — the cheapest end-to-end
rig and the dev profile the whole build runs on; nothing in M1's exit needs an orchestrator.

**Exit.** Invariant tests over the pure core pass; one-commit-point and chain-order semantics
demonstrated on the standalone profile. **Every later milestone depends on M1.**

## M2 — Provider floor

**Goal.** OML is usable as an agent's **only** memory. This is the milestone that makes the
product a provider rather than a governance demo — and it is exactly the `/v1` provider ring of
[ADR-0022](../adr/ADR-0022-two-ring-api.md), whose compatibility covenant (FR-21) is declared
against the surface this milestone finishes.

**Items.** Retrieval (FTS plus pgvector, [retrieval](../architecture/retrieval.md));
attachments ([ADR-0004](../adr/ADR-0004-bodies-and-attachments.md)); the MCP facade with
idempotency ([ADR-0014](../adr/ADR-0014-http-api-and-mcp-facade.md)); the read log
([ADR-0013](../adr/ADR-0013-reads-served-from-postgres.md)); the **docker-compose profile**
joins here ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)) — the
provider-floor milestone is when early users first want a trivial install; and the hot-path
discipline of [ADR-0024](../adr/ADR-0024-performance-foundation.md) binds from the first
provider-ring endpoint, with [FR-25](roadmap.md)'s harness built alongside. Plus five gaps
carried as first-class items, not polish:

- own-held recall (`include_own_held`) and `oml_get` on MCP — [FR-10](roadmap.md);
- `oml_supersede` on MCP and the journalled author retire-proposal — [FR-10](roadmap.md);
- `max_tokens` on recall with deterministic truncation — [FR-11](roadmap.md);
- batch submit with per-item idempotency, and generated Python/TypeScript SDKs —
  [FR-12](roadmap.md);
- boundaries hygiene on the [positioning page](positioning.md): no automatic extraction, no
  profiles, consolidation lives client-side — named decisions, not omissions.

**Exit.** FR-10, FR-11 and FR-12 close. M2's definition of done includes the
agent-only-memory scenario passing: an agent that stores to OML and nothing else can read back
what it wrote, search it under a token budget, update it, and propose retiring it. Depends on
M1.

**Progress.** What FR-10 and FR-11 ask for is built, and the agent-only-memory scenario is a
test in the repository's suite. Neither row has closed: both close when that scenario passes in
continuous integration, and the [roadmap](roadmap.md) cites that run when they do. FR-12's
Python and TypeScript clients are built; its batch write path and upload wrapper are not, and it
still gates M2.

## M3 — Derived lakehouse

**Goal.** The derived side exists and its exactly-once claim is verified, not assumed.

**Items.** Outbox → lake-writer → bronze/silver with the [FR-3](roadmap.md) extended
conformance harness (kill, restart, checkpoint, `VACUUM`, retention — run per object-store
backend: `file://`, pinned MinIO, S3); [FR-16](roadmap.md)
throughput and commit-latency measurements; reconciler v1 with the closed finding vocabulary
([reconciliation](../architecture/reconciliation.md),
[ADR-0017](../adr/ADR-0017-reconciliation.md)); the **Kubernetes local profile** (k3d +
CloudNativePG + pinned MinIO) joins here — the first milestone that exercises the
k8s-specific machinery (leases, CronJob) against the lake
([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)).

**Exit.** FR-3 closes; FR-16 has numbers. Depends on M1 (the outbox) and M0 spikes 1–2.

## M4 — Governance completeness

**Goal.** The differentiator milestone: everything the evidence story promises exists end to
end.

**Items.** The full workarea ladder ([workareas](../architecture/workareas.md)); simulations
([ADR-0011](../adr/ADR-0011-simulations.md)); crypto-shredding with the copy-class inventory
and KEK rotation ([ADR-0005](../adr/ADR-0005-erasure-by-crypto-shredding.md)); evidence
export ([audit and lineage](../architecture/audit-and-lineage.md)); bans; and the vocabulary
slice ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md), [FR-19](roadmap.md)) — prefixes,
the term lifecycle with atomic deprecation, releases with the materialized closure, the vocab
scanner and read surface. Vocabularies depend on M1 (pipeline, claim edges, domain invariants)
and M2 (MCP facade, the recall `terms` filter) but not on M3's lake; they sit here because the
`held_by` attestation and the human-gated ceremonies are governance machinery.

**Exit.** [FR-1](roadmap.md)'s erasure path demonstrated end to end; [FR-17](roadmap.md)'s
erasure-SLO bounds measured; [FR-19](roadmap.md)'s vocabulary slice demonstrated end to end.
Depends on M1–M3 — the evidence export reads the journal, and the
inventory is only checkable once the reconciler runs. [FR-20](roadmap.md)'s vocabulary
measurements join M6's campaigns.

**Progress.** Not started. One check that M1 demonstrated now waits on this milestone: since
each key's event feed shows only the events that key may see (M2), the feed no longer carries
the whole signed history, so the command-line check of that history against a running server
stops with an error that says so. It returns when the server serves the export of the whole
history that this milestone's evidence export includes.

## M5 — Dataset onboarding (post-v1)

**Goal.** [ADR-0020](../adr/ADR-0020-dataset-onboarding.md)'s smallest useful slice: pinned
file sources only (Parquet/CSV by digest), full-replace only, a hard 1M-row cap, no FTS or
vector indexing.

**Items.** The import manifest pipeline, `import_rows`, seeded sampling review, the
per-manifest wrapping-key tier, the resurrection fence in its reduced form.

**Exit.** [FR-13](roadmap.md) closes; [FR-14](roadmap.md) has numbers; RS-4 through RS-8
decided. This milestone sits **after** FR-2, FR-3 and FR-7 have numbers, deliberately: pouring
ten million rows into a system whose single-row review throughput, exactly-once behavior and
storage ceiling have no numbers would be measuring nothing against nothing. Depends on M2–M4.

## M6 — Scale and measurement

**Goal.** The claims become numbers.

**Items.** The scaled Helm profile with cluster mode (followers, lead failover —
[FR-24](roadmap.md)); the measurement campaigns — [FR-2](roadmap.md) review
drain rate, [FR-4](roadmap.md) search quality, [FR-7](roadmap.md) store ceiling,
[FR-15](roadmap.md) scan latency, [FR-18](roadmap.md) reconciler runtime and quorum write
latency, [FR-25](roadmap.md) provider-ring latency against targets, [FR-20](roadmap.md)
vocabulary measurements; [FR-22](roadmap.md) profile conformance (the three-profile matrix)
and [FR-23](roadmap.md) migration rehearsal; a security pass against
[security](../architecture/security.md)'s threat model; and
[FR-9](roadmap.md) re-verified at any publication.

**Exit.** The named rows close with dates and setups. Depends on everything above.

## What is deliberately not on the path

- **Question answering over memories.** OML serves hits, not answers —
  [ADR-0013](../adr/ADR-0013-reads-served-from-postgres.md) declines it.
- **Automatic extraction from conversations.** The explicit-claims thesis declines it; the
  [positioning page](positioning.md) carries the boundary.
- **User or agent profiles.** Declined; `derived_fact` claims and dreamer crews are the
  substitute, per the same boundary.
- **A web console.** Declined in [ADR-0015](../adr/ADR-0015-cli-administration.md).

## Traceability

One row per milestone: the [roadmap](roadmap.md) rows its exit closes (or feeds, where the exit
produces findings rather than closures), the ADRs it exercises for the first time, and the API
surface it adds. The roadmap stays the honesty ledger — a row "closes" here only because that
milestone's exit criteria say so there.

| Milestone | FR/RS rows it closes | ADRs exercised | API surface added |
|---|---|---|---|
| M0 | none closed — findings feed [FR-3](roadmap.md), [FR-4](roadmap.md), [FR-7](roadmap.md), [FR-16](roadmap.md), and the chain-format test vectors | ADR-0002, ADR-0003, ADR-0006, ADR-0023, ADR-0024 — their ecosystem assumptions put under test | none |
| M1 | none closed — the exit (invariant tests, one-commit-point and chain-order semantics) is the precondition of every later row | ADR-0001, ADR-0002, ADR-0006, ADR-0010, ADR-0015, ADR-0019, ADR-0023 (standalone) | minimal slices of both rings: submit and read-own on `/v1`; review and promote on `/v2`; the bootstrap ceremony |
| M2 | [FR-10](roadmap.md), [FR-11](roadmap.md), [FR-12](roadmap.md); [FR-21](roadmap.md)'s covenant becomes declarable (gated on M2, closed at the v1-stable declaration) | ADR-0004, ADR-0013, ADR-0014, ADR-0022, ADR-0024 | the full `/v1` provider ring; the MCP tool catalog; batch submit; generated SDKs |
| M3 | [FR-3](roadmap.md); [FR-16](roadmap.md) gets its numbers | ADR-0003, ADR-0016, ADR-0017 | none new on the provider ring; reconciler findings surface as [ADR-0017](../adr/ADR-0017-reconciliation.md) declares |
| M4 | [FR-1](roadmap.md), [FR-17](roadmap.md), [FR-19](roadmap.md) | ADR-0005, ADR-0008, ADR-0009, ADR-0011, ADR-0012, ADR-0021 | the `/v2` lab-ring governance surface: workarea ladder ceremonies, simulations, erasure, evidence export, vocabulary lifecycle |
| M5 | [FR-13](roadmap.md); [FR-14](roadmap.md) gets its numbers; [RS-4](roadmap.md)–[RS-8](roadmap.md) decided | ADR-0020 | the import-manifest surface on `/v2` |
| M6 | [FR-2](roadmap.md), [FR-4](roadmap.md), [FR-7](roadmap.md), [FR-15](roadmap.md), [FR-18](roadmap.md), [FR-20](roadmap.md), [FR-22](roadmap.md), [FR-23](roadmap.md), [FR-24](roadmap.md), [FR-25](roadmap.md); [FR-9](roadmap.md) re-verified at publication | ADR-0023 (cluster mode, migration path), ADR-0024 (the harness against its rules), plus the security pass against [security](../architecture/security.md) | none — M6 measures what exists |

Rows deliberately absent from every "closes" cell, because no milestone's exit closes them:
[FR-5](roadmap.md), [FR-6](roadmap.md), [FR-8](roadmap.md) (commitments without a milestone
home yet), and the open questions [RS-1](roadmap.md)–[RS-3](roadmap.md),
[RS-9](roadmap.md)–[RS-14](roadmap.md), each of which closes by a recorded decision, not by a
build milestone — [RS-1](roadmap.md) must in practice resolve before M2's retrieval work and
[RS-14](roadmap.md) before M2's hydration path, and that ordering pressure is stated here rather
than hidden.
