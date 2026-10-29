# Critical path

**Status:** design draft for review · decided, not built

This page orders the build. It carries no dates: no team sizing exists, so a duration would be
an invented number, and the [roadmap](roadmap.md)'s rule against invented numbers applies to
schedules too. What this page commits is **order and dependency** — which milestone must exist
before which, and why. A milestone is done when its exit criteria close — mostly roadmap FR
rows with verifiable criteria — not when its code merges.

## M0 — De-risking spikes, before any milestone

**Goal.** Retire the ecosystem assumptions that could invalidate recorded decisions, before
code accumulates on top of them. In order:

1. **delta-rs transaction-identifier conformance.** Verify that applied-version state survives
   a checkpoint written mid-sequence, `VACUUM`, restart, and the transaction-identifier
   retention settings — and pin those settings. A failure here invalidates
   [ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md) d4 (the Delta log as sole
   recovery authority); the named fallback is a Postgres applied-ledger as the recovery
   authority. Feeds [FR-3](roadmap.md)'s extended harness.
2. **Conditional put under degraded quorum.** Test conditional PUT on MinIO with a degraded
   read quorum and verify lockless delta-rs commits on both MinIO and S3; pin the exact MinIO
   release.
3. **RLS hardening.** `FORCE ROW LEVEL SECURITY` on every tenant table, the
   owner/migration-role split, `SET LOCAL` tenant discipline in pooled transactions — the
   assumptions [commitments](commitments.md) C6 names. Cheap; do immediately.
4. **pgvector ceiling probe.** HNSW build memory at corpus scale, per-tenant LIST-partition
   overhead at thousands of tenants, iterative-scan caps. Folds into [FR-4](roadmap.md) and
   [FR-7](roadmap.md).

**Exit.** Each spike's finding recorded; any invalidated decision re-opened as an ADR
amendment before M1 builds on it. **Everything depends on M0's findings standing.**

## M1 — System of record

**Goal.** The one commit point exists and the constitution is code.

**Items.** The `oml-domain` pure crate with invariant property tests
([ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d1); the Postgres schema
with per-tenant chains, advisory-lock commit and the outbox
([storage](../architecture/storage.md)); a minimal `oml-server` — submit, review, promote,
read-own — with keys, grants and the bootstrap ceremony
([roles and keys](../architecture/roles-and-keys.md)); the CLI minimum
([ADR-0015](../adr/ADR-0015-cli-administration.md)); the local Helm profile
([deployment](../operations/deployment.md)).

**Exit.** Invariant tests over the pure core pass; one-commit-point and chain-order semantics
demonstrated on the local profile. **Every later milestone depends on M1.**

## M2 — Provider floor

**Goal.** OML is usable as an agent's **only** memory. This is the milestone that makes the
product a provider rather than a governance demo — and it is exactly the `/v1` provider ring of
[ADR-0022](../adr/ADR-0022-two-ring-api.md), whose compatibility covenant (FR-21) is declared
against the surface this milestone finishes.

**Items.** Retrieval (FTS plus pgvector, [retrieval](../architecture/retrieval.md));
attachments ([ADR-0004](../adr/ADR-0004-bodies-and-attachments.md)); the MCP facade with
idempotency ([ADR-0014](../adr/ADR-0014-http-api-and-mcp-facade.md)); the read log
([ADR-0013](../adr/ADR-0013-reads-served-from-postgres.md)). Plus five gaps carried as
first-class items, not polish:

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

## M3 — Derived lakehouse

**Goal.** The derived side exists and its exactly-once claim is verified, not assumed.

**Items.** Outbox → lake-writer → bronze/silver with the [FR-3](roadmap.md) extended
conformance harness (kill, restart, checkpoint, `VACUUM`, retention); [FR-16](roadmap.md)
throughput and commit-latency measurements; reconciler v1 with the closed finding vocabulary
([reconciliation](../architecture/reconciliation.md),
[ADR-0017](../adr/ADR-0017-reconciliation.md)).

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

**Items.** The scaled Helm profile; the measurement campaigns — [FR-2](roadmap.md) review
drain rate, [FR-4](roadmap.md) search quality, [FR-7](roadmap.md) store ceiling,
[FR-15](roadmap.md) scan latency, [FR-18](roadmap.md) reconciler runtime and quorum write
latency; a security pass against [security](../architecture/security.md)'s threat model; and
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
