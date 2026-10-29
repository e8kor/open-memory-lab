<!--
id: adr/ADR-0013-reads-served-from-postgres
tier: engineering
status: accepted design
answers: why reads are served from Postgres as the one serving tier; why every read is journalled; why steering is applied ahead of hits
-->

# ADR-0013 — Reads are served from Postgres, journalled: one serving tier, a logged read path, steering ahead of hits

**Status:** accepted design · 2026-10-29

Everything this record decides is design — no code exists. The mechanics of the serving tier
(index layout, query shapes, the read-log schema) are owned by
[architecture/retrieval.md](../architecture/retrieval.md); the simulation predicate's channel is
owned by [architecture/simulations.md](../architecture/simulations.md); this record decides and
gives the reasons.

## Context

OML has two stores that hold promoted content: PostgreSQL, the system of record (ADR-0002), and
Delta Lake, the derived body lakehouse (ADR-0003). A client read — recall, full-text search,
vector search, fetch, graph read — has to come from exactly one of them, and the choice is not a
performance question first. It is an audit question: OML's product claim is that the service can
answer *who read what, when an auditor asks*, and a read path decides whether that sentence can be
true. It is also a policy question: every read must apply tenancy (RLS, ADR-0007), grant scope
(ADR-0008), workarea status (ADR-0009), and the simulation audience predicate (ADR-0011) — rules
that live as Postgres rows and are enforced in Postgres.

Two observations frame the answer. Semantic plus keyword retrieval is table stakes for a memory
provider — agents recall by meaning as often as by term, and a provider that serves only one of
the two forces every client to build the other outside the gate. And OML's authority is
Postgres, so the serving tier is Postgres's own: tsvector and pgvector live inside the store
that already enforces every read-time rule. What remains is to decide the read source, the read
log, the simulation fence, the vector layout, the steering contract, and whether the server
composes answers.

## Decision

- **d1 — Every client read is served from Postgres, never from the lake.** The serving tier is
  tsvector full-text search plus pgvector embeddings over promoted rows, with bodies decrypted on
  the serving path only (the serving path is the one place that holds DEKs, per ADR-0004 and
  ADR-0005). The lake serves reconciliation, lineage, metadata analytics and future journalled
  exports (ADR-0003) — no client read, ever. One read path means one place where RLS, grants,
  workarea status and the simulation predicate are all applied, and one place where the read can
  be logged (d2).

- **d2 — Reads are logged, outside the hash chain, with anchored digests and a retention bound.**
  A read event records key, role, workarea, the memory ids returned or a digest of the query, and
  a timestamp. Read events live in a retention-bounded log *outside* the journal's hash chain —
  read volume must not contend with the per-tenant chain head (ADR-0002 d3), and a retention
  bound on chain rows would contradict the chain. The bridge is anchoring: each batch of read
  events has its digest written into the journal as a signed event, so an auditor can verify that
  the retained log is the log that was anchored, without the chain carrying the rows. The
  retention window is tenant policy; evidence exports (ADR-0015) scope their read
  log to that window and say so; `read_events` appear in the erasure inventory (ADR-0005). The
  defensible formulation, used everywhere: this is **a read log that answers who read what, when
  an auditor asks** — no regulator's requirement is named without a sourced regulation and
  article.

- **d3 — Every read path applies the simulation predicate.** FTS, vector search, fetch and graph
  reads all filter on `provenance ≠ simulation OR key ∈ audience` (ADR-0011). Simulation rows
  stay out of the shared FTS and vector indexes and are indexed per simulation, so the fence does
  not depend on a WHERE clause being remembered in every query — a shared index simply does not
  contain the rows. The cost is one extra index set per live simulation; the benefit is that
  forgetting the predicate on some future read path fails toward invisibility, not leakage.

- **d4 — pgvector layout: per-tenant partitions, per-partition HNSW, filters inside the scan.**
  `memory_embeddings` is list-partitioned by `tenant_id` with an HNSW index per partition and RLS
  on top, iterative scan enabled, and a workarea-path filter column so subtree-scoped queries
  filter during the scan rather than after it. Embeddings are computed by `oml-indexer` under its
  own credentials per tenant — never the signing key; it reads promoted text on the serving side
  of decryption, and its exact decrypt-grant shape is bound to the embedding decision
  ([RS-1](../product/roadmap.md), per [ADR-0007](ADR-0007-tenancy-first-class.md) d3) — and
  embedding rows are erasable (they are plaintext-derived and appear in the ADR-0005 inventory). Recall quality, latency, and the exact-versus-approximate
  trade are **unmeasured** — each is a tracked FR/RS row with an exit criterion on the
  [roadmap](../product/roadmap.md).

- **d5 — Steering is a claim kind with an ordering contract.** A steering row is written by a
  writer key, held on arrival and non-author-reviewed like any claim (ADR-0010, ADR-0012), and
  carried in the `kind` column (`steering`, per the data model). The contract: a recall scoped
  to a workarea returns the steering in force there **before** the hits,
  and a steering row is **never** a ranked hit — direction and relevance do not mix in one list.
  This is R3 applied to reads: the gate (and the steering that passed it) changes visibility and
  position-by-contract, never ranking. There is no separate delivery channel — OML's clients
  fetch steering through the same recall verbs as everything else (ADR-0014).

- **d6 — Grounded Q&A is out of scope for v1.** OML's clients are agents that recall and reason
  themselves; a server-side answer composer would put a model inside a provider whose design runs
  no model (ADR-0012: external crews review; the provider itself holds no inference), and would
  create a second read surface whose outputs are not claims — unreviewable, unattributable,
  outside the journal. If OML ever wants composed answers, they arrive behind a new decision
  that must scope the answerer to what the reader may read and keep the signing key out of the
  composer — never as a silent addition to the read path.

## Consequences

- One store answers every client read, so read-time policy — RLS, grants, status, simulation
  audience — is enforced where the policy rows live, and a read that happened is a read that was
  logged. The cost is that Postgres carries the whole read load *and* the write load; the Postgres
  scale ceiling and partition strategy FR on the [roadmap](../product/roadmap.md) carries that
  risk with an exit criterion, and d1 is the decision that makes the FR worth measuring.
- Read performance claims do not exist yet. Recall quality, read latency and index behaviour under
  per-tenant partitioning are unmeasured — stated here once and marked on every page that touches
  them (decided, not built; unmeasured — see the roadmap rows).
- The read log's honesty boundary is explicit: inside the retention window, anchored and
  verifiable; outside it, gone, and exports say so. A tenant choosing a short window is choosing a
  short answer to the auditor's question — the policy knob and its consequence are the same
  sentence.
- Steering gains a stable contract clients can build on: steering first, hits after, no rank
  interleaving. Agents that ignore steering are visible in the read log (they were served it),
  which is the only enforcement a memory provider can honestly offer.
- Anything that wants lake reads — per-audience decrypted exports, analytics surfaces — arrives as
  a journalled export feature (tracked FR on the roadmap), never as a query endpoint.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Serve reads from the lake (lake-direct reads)** | The audit hole. A read from parquet bypasses the one place reads are logged and policy is enforced: no read event, no RLS, no grant attenuation, no simulation predicate — every rule would need a faithful reimplementation in a second engine, and the two would drift. Lake bodies are also per-artifact ciphertext (ADR-0004), so a lake reader needs DEKs, widening key custody; and the lake is asynchronous behind the outbox frontier, so lake reads serve a stale tip without being able to say how stale. |
| **A separate search service (OpenSearch/Elasticsearch)** | A third store with its own durability, its own ingest pipeline, and its own reconciliation story — a second ADR-0017 just for search, plus a second enforcement point for RLS/grants/simulation that Postgres already enforces natively. Deferred, not forbidden: the trigger is the named FR on the [roadmap](../product/roadmap.md) — pgvector/tsvector measurably failing recall or latency exit criteria at a real tenant scale. Until that measurement exists, a dedicated search tier is cost without evidence. |
| **Read events inside the hash chain** | Every read would contend on the per-tenant chain head (ADR-0002 d3), and retention-bounding chain rows contradicts an append-only chain. Anchored batch digests (d2) keep the verifiability and drop both costs. |
| **Not logging reads at all** | Cheaper and simpler, and it forfeits the product sentence — "who read what, when an auditor asks" — that distinguishes a governed memory provider from a database with an API. |
| **Grounded Q&A in v1** | d6. A model inside the provider, producing unjournalled prose from journalled claims, with its own cost, custody and review questions — for clients that are themselves reasoning agents. Declined. |
