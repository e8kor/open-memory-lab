# ADR-0003 — Delta Lake is the derived body lakehouse: outbox-fed, single-writer, never the authority

**Status:** accepted design · 2026-10-29

Everything this record decides is design — no code exists. The table layouts, the transaction
protocol column by column, and the maintenance schedule are owned by
[architecture/storage.md](../architecture/storage.md) and
[data/delta-tables.md](../data/delta-tables.md); this record decides and gives the reasons.

## Context

PostgreSQL is the system of record and the sole commit point (ADR-0002). The lake exists for the
work that should not run against the serving store: reconciliation (ADR-0017 compares the lake
against the journal fold), lineage, metadata analytics, and future journalled per-audience
exports. It serves no client read — reads come from Postgres, journalled (ADR-0013 of this
register) — so the lake's purpose is deliberately narrower than "the data platform": it is the
substrate the provider itself analyses and audits, holding the content of promoted memories with
who/when/where attribution, bodies encrypted at rest.

That narrowing is deliberate. A lake that clients may read as an alternate source drags in a
whole class of machinery — freshness contracts, removal probes, per-role source selection,
count-equality guards — all of it existing only to make a lagging projection safe to serve. OML
retires that whole class of problem by not serving: a lake nobody reads for authorization or
recall needs a frontier, not a freshness contract.

Three constraints shape the mechanics. The lake is downstream of a transactional outbox
(ADR-0002 d1/d3), so its input is already ordered and durable. The lake writer must survive crash
and failover without double-applying or skipping a batch. And a single poison row must not halt
materialization for a tenant forever.

## Decision

- **d1 — Purpose is narrowed and the lake serves no client read.** Reconciliation, lineage,
  metadata analytics, and future journalled per-audience exports. No recall, search, graph or
  fetch path touches it. The benefit is that staleness is an internal quantity (frontier lag, a
  `doctor` and reconciler concern) rather than a client-visible consistency contract; the cost is
  that the lake is useless for serving latency, which is accepted on purpose.

- **d2 — Exactly one lake-writer deployment drains the outbox in micro-batches.** A singleton with
  a leader lease; split-brain during failover is bounded by Delta's conditional put (put-if-absent)
  and by transaction-id conflict as the second fence — both pinned by the runtime decision
  (ADR-0006 of this register) and wired by the deployment (ADR-0016). One writer keeps commit
  conflict handling out of the design; the cost is a materialization throughput ceiling of one
  process, revisited only if a measured lag demands it.

- **d3 — No held body ever reaches the lake.** Bronze mirrors journal events with digest-only
  payloads (which is all the journal carries, ADR-0002 d5), so an event about a held claim
  contributes a signed envelope and digests, never content. Silver materialization is enqueued by
  promotion only: a body enters the lake the first time an identity that did not write it has
  ratified it, and not before. Gold holds curated tables and lineage for validated workareas.
  Unreviewed content never enters a consumer-readable zone.

- **d4 — Exactly-once by per-table Delta application transaction ids; the Delta log is the sole
  recovery authority.** Every batch commit carries appId `oml-lakewriter` with version = the last
  applied outbox seq, recorded and checked **per table** on replay: a table skips any batch whose
  seq is at or below its last transaction version. Batches are cut only from the outbox's **stable
  prefix**: outbox `seq` is assigned at insert but visible only at commit, so the cut bounds
  batch-end by the snapshot horizon — rows whose inserting transaction is older than every
  transaction still active at scan time — and the frontier advances only within that prefix. A
  late-committing row therefore lands in a later batch, never behind the frontier, and a
  long-running transaction surfaces as frontier lag (ADR-0017's `stale-frontier`), never as loss.
  The Postgres frontier column is a cache for
  observability, never consulted for recovery; the global frontier advances only when every table
  the batch touched has committed. A retried batch is reconstructed deterministically from the
  (frontier, batch-end seq) pair recorded in the transaction action — never re-cut by time or
  size, and never different from the first attempt, because nothing can commit into recorded
  bounds after the cut. Whether delta-rs exposes application transaction ids in exactly this shape is cited from
  delta-rs documentation with a fetch date at implementation-spec time, and "exactly-once,
  verified by conformance test" is the exit criterion of the delta-rs conformance FR on the
  [roadmap](../product/roadmap.md). Until that row closes, the mechanism is a design assumption,
  not a demonstrated property of OML's Rust path.

- **d5 — Retract is a MERGE, in the same stream, in strict order.** Demote, erase, simulation-end
  and reassign apply as a MERGE on (artifact_id, version) setting state, processed in strict
  outbox seq order in the same single stream as materialization. One stream means a retract can
  never be applied before the row it retracts, and no second pipeline needs its own ordering
  story. Erasure of bytes is not the MERGE's job — lake bodies are ciphertext and erasure destroys
  the DEK (ADR-0005 of this register); the MERGE records the state.

- **d6 — Poison rows dead-letter; the stream never blocks.** After a bounded retry budget the
  lake-writer journals a materialize-skipped event, moves the row to a dead-letter table, and
  advances the frontier. The reconciler reports dead-lettered rows as a named finding class
  (ADR-0017 d4), and repair is a journalled re-enqueue. The benefit is liveness; the cost is that
  the lake is knowingly, visibly incomplete until repair — visible being the point: a journalled
  skip plus a named finding, never a silent gap.

- **d7 — The lake row carries everything reconciliation needs, with bodies encrypted and
  attribution in the clear.** Per-tenant table prefixes; rows carry artifact_id, workarea_id (the
  stable key, with path-at-write kept as history), writer identity, principal, timestamps,
  provenance class, plaintext digest and ciphertext digest (both journalled in the submission
  event, ADR-0002 d5, so the fold — not the store — owns the reconciler's comparand), and state.
  Bodies are per-artifact-DEK ciphertext. This is the lake's purpose honored in full — pure memory data, who and when wrote
  them, what space — with bodies encrypted at rest; the attribution columns' own erasure boundary
  is stated where erasure is decided (ADR-0005 of this register), not glossed here.

## Consequences

- The authority never points at its projection; the binding runs the other way and is
  structural, not declarative. The lake is written only from the journal's outbox, under
  per-tenant prefixes, and every commit's transaction id ties the batch to a journal seq (d4),
  so every lake byte traces to a journal event by construction — no sidecar file asserts it.
- The lake is eventually consistent with the journal by design, and the lag has a name — the
  frontier — surfaced by `oml status`/`oml doctor` and checked by the reconciler (stale-frontier
  is a finding class, ADR-0017 d4). Nothing downstream may treat a lake row as current eligibility.
- Recovery after any crash or failover is mechanical: read each table's last transaction version
  from the Delta log, reconstruct the batch deterministically, replay; tables already committed
  skip it (d4). No operator judgment is in the loop.
- Dead-lettering makes incompleteness a first-class, journalled, repairable state rather than a
  stuck queue (d6) — at the price that "the lake has everything" is never assumed, only verified
  (ADR-0017).
- Erasure composes: digest-only bronze never needs touching; silver/gold ciphertext becomes
  unreadable when the DEK is destroyed, with parquet versions and checkpoints holding ciphertext
  until VACUUM/checkpoint cleanup — a retention bound that is part of the erasure SLO, owned by
  ADR-0005 of this register.
- The conformance burden is named: exactly-once is a claim with a test, not an adjective — the
  delta-rs exactly-once conformance FR on the [roadmap](../product/roadmap.md) carries the exit
  criterion, and the capability citation carries a fetch date when written.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Delta as the authority, Postgres as the cache** | The dual-write argument, settled in ADR-0002: the commit point must atomically bind journal, metadata and outbox, and no transaction spans the two stores. A lake authority puts object-store commit latency on the write path or makes divergence routine. |
| **Event sourcing into Kafka between Postgres and the lake** | A third store with its own durability, ordering, retention and operations, sitting between two stores that must each still be reconciled to the journal. The in-transaction outbox already provides an ordered, replayable, exactly-once-feedable stream; a broker adds a hop and a second offset/seq bookkeeping without removing any of the lake-writer's obligations. |
| **Serving client reads from the lake** | Retired by purpose (d1). A served lagging projection needs freshness tests, removal probes and per-role source selection just so a removal is never shown late. OML serves from Postgres (ADR-0013) and the entire class of machinery disappears. |
| **Synchronous lake write in the commit path** | Couples write availability and latency to the object store: an object-store commit is a multi-round-trip protocol whose latency and failure modes would land on every acknowledged write. Delta commit latency is unmeasured for OML, but the coupling argument alone decides. |
| **Multiple lake writers (per tenant or per table)** | Concurrent Delta commits mean conflict resolution, per-writer frontiers and a real split-brain surface — permanent complexity bought against an unmeasured throughput need. One writer with per-table transaction ids (d4) is the simple correct thing; scale-out is a revisit trigger when frontier lag is measured, not assumed. |
| **Postgres frontier as the recovery authority** | The recovery authority must live where the commit lands. A crash between a Delta commit and the frontier update would otherwise double-apply or skip a batch; with the Delta log's transaction version authoritative (d4), that window cannot lie. |
| **Re-cutting retried batches by time or size** | A retried batch with different boundaries defeats the idempotency check: the per-table version comparison can then skip rows the first attempt never committed or re-apply ones it did. Deterministic reconstruction from (frontier, batch-end seq) makes retry a replay, not a new decision. |
| **Blocking the stream on a poison row** | Head-of-line blocking halts all materialization for the tenant until a human intervenes, and the journal shows nothing. Dead-lettering (d6) trades completeness for liveness and records the trade in the journal where the reconciler and the operator can see it. |
