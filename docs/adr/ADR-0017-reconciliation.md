<!--
id: adr/ADR-0017-reconciliation
tier: engineering
status: accepted design
answers: why reconciliation is an incremental journalled fold from the journal alone, and why its findings form a closed vocabulary
-->

# ADR-0017 — Reconciliation is an incremental, journalled fold with a closed finding vocabulary

**Status:** accepted design · 2026-10-29

Everything this record decides is design — no code exists. The comparison procedure, the finding
schema and the repair flows are owned by
[architecture/reconciliation.md](../architecture/reconciliation.md); this record decides and gives
the reasons. Its ground is [ADR-0002](ADR-0002-postgresql-system-of-record.md) (the journal is the
system of record and the only fold source) and
[ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) (the lake is derived, outbox-fed, with a
frontier and a dead-letter table).

## Context

A derived store earns its keep only if its divergence from the authority is found, named and
repaired — otherwise "the lake is a projection" is a hope, not a property. ADR-0003 deliberately
admits divergence: the frontier lags, poison rows dead-letter, and a crash is recovered by replay.
Something must therefore answer, continuously and cheaply: does the lake at a named version equal
what folding the journal says it should hold, and do the Postgres projections equal their own
fold?

[ADR-0002](ADR-0002-postgresql-system-of-record.md) already makes replay the canonical recovery
and the canonical test oracle: projections are rebuildable by folding the journal. What that
leaves undesigned is the service that runs the oracle in production, incrementally, against
a lakehouse, with repairs that are themselves auditable. That is this record's job.

Three constraints bind the design. The reconciler must not become a second authority (it compares
and proposes; it never decides what is true — the journal already did). It must hold no decryption
capability, because a service that reads everything and decrypts everything is the largest prize
in the system (key custody per ADR-0006; privileged-service rules per ADR-0007).
And its cost must not grow with history, or it will be run less often exactly as the history it
guards grows.

## Decision

- **d1 — Reconciliation is incremental, from a persisted, journalled cursor.** `oml-reconciler`
  keeps a cursor — last reconciled seq plus the running chain hash at that seq — and each run folds
  the journal forward from it. The cursor is itself journalled, so "what has been verified, and
  against what hash" is part of the evidence, not a scratch file. Full-genesis verification — the
  whole chain, every table, from seq 1 — remains an occasional, explicitly named job, not the
  steady state; the steady state costs what the new events cost.

- **d2 — The journal is the fold source; the lake is compared at a named Delta version.** Expected
  state is computed only by folding journal events (ADR-0002); observed state is read from Postgres
  projections and from the lake pinned at a named Delta version, never "latest while we read". The
  running chain hash in the cursor means the fold also re-verifies the chain as it advances.

- **d3 — Comparisons are cardinality- and state-aware.** The unit of comparison is
  (artifact_id, version, state, row count) — not event counts, which miss duplicates, and not
  presence alone, which misses a row in the wrong state. Digest comparison runs over **ciphertext
  digests** — journalled per body and attachment in the submission events (ADR-0002 d5), so the
  fold, not the store under audit, supplies the expected digest — which is what lets the
  reconciler verify bytes keylessly (d6).

- **d4 — The finding vocabulary is closed:** `missing`, `orphan`, `duplicate`, `state-mismatch`,
  `digest-mismatch`, `stale-frontier`, `dead-lettered`. Every divergence the reconciler can report
  is one of these; a divergence that fits none of them is a design gap and amends this record
  rather than minting a free-text finding. Closed vocabulary is what makes findings aggregable,
  alertable and testable — one conformance test per class — at the cost that the vocabulary must
  be maintained deliberately.

- **d5 — Repairs flow through the journal's own machinery, never around it.**
    - **Lake repairs** re-emit through the outbox, built from the **current** journal fold and
      carrying the fold seq; the lake-writer refuses any repair op older than the artifact's last
      applied fold seq, so a stale repair can never regress a newer state.
    - **Metadata repairs** are a journalled re-fold of the artifact from the journal — the
      projection row is recomputed from the authority, and the act is recorded.
    - A `dead-lettered` finding's repair is the journalled re-enqueue ADR-0003 d6 names.
  No repair writes to the lake or the projections directly.

- **d6 — The reconciler is read-only and keyless.** It runs under its own credentials
  (no signing key, no KMS decrypt grant), reads under RLS, and emits repair proposals through
  `oml-server` into the outbox. Ciphertext-digest comparison (d3) is what makes keyless
  verification sufficient; plaintext-digest verification belongs to the serving path, which holds
  DEKs (ADR-0004). Operationally it is a CronJob with `concurrencyPolicy: Forbid`
  and a deadline (ADR-0016), so two reconcilers never race one cursor.

- **d7 — Findings have identity, and repair halts before it loops.** Every finding carries a
  stable identity and an attempt counter. The same finding recurring N times halts auto-repair for
  that artifact and raises an operator condition: a divergence that survives N repairs is a defect
  in the writer, the fold or the comparison, and re-repairing it forever would convert a bug into
  a steady-state write load that masks itself. N is configuration with a journalled default;
  surfacing is `oml status` / `oml doctor` (ADR-0015).

## Consequences

- "The lake is a projection" becomes checkable: at any named Delta version, the reconciler can say
  whether the lake equals the journal fold, name each divergence from a closed list, and show the
  journalled repair for it. That sentence — not "consistent", not "guaranteed" — is the claim the
  evidence export makes.
- The dead-letter path (ADR-0003 d6) is closed end to end: skip is journalled, the finding class
  names it, the repair re-enqueues it, and a repair that keeps failing halts and pages (d7).
- Reconciliation adds no new trust: no key, no direct writes, one more reader under RLS. The cost
  is indirection — every repair takes the outbox round trip and lands at the lake-writer's pace.
- The cursor's running chain hash means routine reconciliation continuously re-verifies the chain
  tail for free; a chain break surfaces as a reconciler failure, not only at `oml-server`'s
  startup fork check (ADR-0002 d7).
- Repair convergence, reconciler throughput and the right N for the halt rule are unmeasured —
  design commitments with exit criteria tracked on the [roadmap](../product/roadmap.md), not
  properties of a running system.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Full verification on every run** | Cost grows with history, so the run gets slower exactly as the corpus it protects grows, and the operator's response is to run it less. Incremental-with-journalled-cursor (d1) keeps the steady state proportional to new events and keeps full-genesis as a deliberate, named act. |
| **The Delta log as the fold source** | The lake is derived (ADR-0003); folding it to verify it verifies the lake against itself. The Delta log's transaction versions are the lake-writer's recovery authority for *replay position* (ADR-0003 d4) — a different question from *what should exist*, which only the journal answers. |
| **A reconciler that repairs in place, with write credentials** | It would need lake write access and would put a second writer beside the singleton (reopening everything ADR-0003 d2 closed), and a direct fix leaves no journal trace — a silent mutation by the most-privileged reader in the system. Repairs through the outbox (d5) keep one writer, one order, and an auditable act per repair. |
| **A decrypting reconciler verifying plaintext digests** | A service holding every tenant's DEK-decrypt grant plus read-everything scope is the system's largest single prize, bought to catch a corruption class (ciphertext intact, plaintext wrong) that per-artifact authenticated encryption already surfaces on the serving path. Keyless ciphertext verification (d3, d6) covers storage-layer integrity, which is reconciliation's job. |
| **Open-ended (free-text) findings** | Unaggregable, unalertable, untestable; every new writer bug invents a new string and no dashboard or conformance suite can enumerate what reconciliation may say. The closed vocabulary (d4) is deliberately the harder discipline. |
| **Event-count or row-count-only comparison** | Counts can match while a duplicate hides a missing row, or while a row sits in the wrong state. Cardinality- and state-aware tuples (d3) are the cheapest comparison that cannot be fooled this way. |
| **Unbounded auto-repair** | A recurring finding re-repaired forever is a defect converted into permanent write load, invisible precisely because the reconciler keeps "fixing" it. The attempt counter and halt (d7) turn the loop into an operator condition with the evidence attached. |
