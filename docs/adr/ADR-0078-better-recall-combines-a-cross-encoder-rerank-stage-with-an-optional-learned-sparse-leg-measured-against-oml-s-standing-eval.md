---
id: adr/ADR-0078-better-recall-combines-a-cross-encoder-rerank-stage-with-an-optional-learned-sparse-leg-measured-against-oml-s-standing-eval
tier: engineering
status: proposed · 2026-11-07
answers: which layered retrieval improvements are safe and measurable over OML's held-out eval, how those layers fit OML's retrievers, lexical postings, valid time and review model, and which algorithmic paths are deferred
---

# ADR-0078 — Better recall combines a cross-encoder rerank stage with an optional learned-sparse leg, measured against OML's standing eval

**Status:** proposed · 2026-11-07
**Amends:** ADR-0048 (d1–d3, d6: rerank gated by eval), ADR-0049 (d2, d8, d10),
ADR-0053 (d8, d10), ADR-0057 (d3), ADR-0059 d2, ADR-0052, ADR-0047

## Context

OML's recall algorithm is deliberately conservative. It reads only what the key may read,
puts its candidate set per workarea ancestry, then runs one of the named retriever
pipelines and two optional stages: fusion and reranking (`crates/oml-app/src/app.rs:5730`,
`app.rs:5810`, `app.rs:6400`). The default today is `bm25-postings`; its OML eval result is
the reference all candidates must beat on the same frozen data.

The external retrieval landscape is large: dense retrieval, learned-sparse retrieval,
cross-encoder reranking, late interaction, hybrid fusion, query expansion, temporal graphs,
memory compression and dedicated vector engines. Most published improvements come from BEIR
or agent-memory benchmarks whose task shape, privacy constraints and authority limits differ
from OML's workarea-scoped, reviewable memories.

OML does not start from weakness. Its current algorithm is clean, auditable and already
staged around eval-gated change (`ADR-0048`, `ADR-0049`, `ADR-0053`, `ADR-0054`). The
mistake would be replacing that core with a single learned store. The lower-risk direction
is to add layers at measurable stages, retain lexical retrieval, and keep the PostgreSQL commit
point as authority.

## Decision

- **d1 — Cross-encoder rerank is the first semantic layer.** It runs only after
  `fuse_capped` (some candidates, depth-limited) and only on admitted candidate rows.
  This reuses the current port held as `Arc<dyn Reranker>`
  (`crates/oml-app/src/ports.rs:1946`, `ports.rs:1952`) and the existing
  `rerank = Applied | Skipped` outcome accounting.
  ADR-0049's suggested target is `+0.09` success@1, but its achieved value on OML's
  standing eval remains unmeasured as #138. A run using `model()` or the fallback on no
  deployment must document its base and release.

- **d2 — The reranking stage is opt-in by default in production.** The pipeline selection
  `pipeline_for` at `crates/oml-app/src/app.rs:6371-6390` and search config
  (`search.rs` ~ 420, 426, 432, 486) remain unchanged once the pipeline is chosen.
  A build with no reranker answers `NoReranker`; a server deploying the optional candle adapter may elide a
  served recall per `oml-reranks-total` and `search_rerank`:
  `oml_recall_stage_duration_seconds` is the stage's proof.
  This follows ADR-0049 d4 and d6, ADR-0053 d10.

- **d3 — Candidate retrieval has grown to two sparse legs before a dense one.** OML's current
  `bm25-postings`, `bm25-sparse` and `bm25-trigram` tiers form the baseline. The proposed
  first semantic leg is a learned sparse retriever (SPLADE-style), fused through the existing
  weighted-CC mechanism at beta weights 0.3/0.7, or by RRF at reciprocal rank
  `k = 60` (ADR-0049 d3).
  Its lexicon and weights are published as integer postings in the same leakproof table set
  (ADR-0052 d1).

- **d4 — The retrieval leg must respect the safety posture defined for lexical postings.**
  Text operators are not safe under row security (ADR-0052, migration 0020); lexical IDs
  (`sha2` `bigint` or model token ids) continue to represent the indexed language. Every
  retriever step that lands in this branch is:
  - scope-limited first,
  - integrity-checked (seal/check),
  - subject to admission, and
  - never a credential-conferring step on its own (no hidden policy will be introduced by a
    text or token-weight change returned by the algorithm).

- **d5 — Tie the valid-time window to each candidate.** A candidate must ground both the
  memory's declared valid time and its status as `held`/`promoted`/`retired`, and a version
  already superseded or retired by a reviewed replacement (ADR-0059 d2) must be demoted.
  Validitor time and the token's or remembered correctness must appear in evaluation calls
  before any semantic change to the core signal. Metrics `search` (`matched`, `fallback` and
  `access`) are recorded for observability.

- **d6 — Only add residual graph / temporal steps if sparse/recall fine-tuning fails.**
  OML already has `supports` and `contradicts` edges (ADR-0046) and valid-time semantics
  (ADR-0059). Those edges are preferred to free-form graph extraction. Graph traversal steps
  may use signed, typed edges as explicit, but must be exposed by the retriever abstraction
  and supervised by ADR-0064 or explicit tool and policy as page services.

- **d7 — Make integration-testing the authority for all algorithmic change.**
  Use kinds and code evidence listed in ADR-0048, 0049 and 0051, plus the scale limitations
  in section 11 "Future version tracks" and related notes. A new algorithm passes only on a
  min date, but is never permitted to violate latency/concurrency/leakage constraints in CI.

## Risk and scalability notes

- **SPLADE-style learned sparse:** inference boundary and model version must be reviewed.
  Proven benefit at BEIR over BM25 claims is often English and query-pattern dependent
  (SPLADE-v3, arXiv:2403.06789, >40 MRR@10 on MS MARCO dev, +2% BEIR over
  SPLADE++/BM25). Treat it as a hypothesis in OML eval.
- **Cross-encoder reranks:** promising but only on top 50; local p95 has to stay bounded.
- **ColBERT/late interaction:** quality promise is real but storage/indexing overhead and RLS
  or scope companion mechanics constrain it. It is a stage-level candidate, not a stored
  authority.
- **Dense pgvector/HNSW):** license, build/analyze, partition, privacy-remediation
  (clean-up) and cost constraints; use it only behind the existing Retriever port on a
  derived basis.
- **Graph memory:** extraction errors and consistency overhead come with graph; use
  typed `supports`/`contradicts` edges and temporal validity before general graph
  generation.
- **Full-context LLM calls:** unlikely to prove better long-term recall at scale due cost
  at 1,000,000 memories; they remain a non-default experiment.
- **Compression:** only via `restricted` derived claims, not lossy opaque summaries.

## Minimum credible evaluation

Before defaulting any alternative:

1. Versioned eval corpus and query set, with held-out questions as #138 requires.
2. A baseline request route under equal scope, max tokens and no key scope leak.
3. Same pipeline answer JSON, including `search`, `fallback`, `terms`, `access` and budget.
4. A/B query count and score/hit counts by stage, including each candidate leg.
5. Candidate floor (`apply_floor`) validation where low-scoring candidates are withheld.
6. A recall outcome surface is tracked in the journal and telemetry
   (`recall_events`, `recall_outcomes`).
7. Leakage/isolation tests over tenant and workarea access.
8. Exact and valid-time suppression after policy change, explicit scope of union and
   supersession.
9. CI pipeline and reproducibility of rerank latency p95/p99 by scope size.
10. A documented setting (`search.enabled`, pipeline, model digest, concurrency,
    budget, floor, raced-window cases).

## Consequences

- **Costs.** Schema and index mechanisms stay unchanged for the core lexical path. A
  complete implementation requires ADR amendment controls and version tracking (`git`
  history). After rerank integration, budgets add a local model backpool and memory, plus
  a learned-sparse model. If semantic retrieval adds weight, OML must constrain model hash
  and build-score truthfulness as with code skeleton. Semantic kinds set does not yet
  exist, but duplicate noise should not be introduced.
- **Docs.** Expected to update:
  - search stages, pipeline name and evaluator scope
  - expected metrics for rerank, budget cuts and hit counts
  - failure modes (`search.refused`, search `unavailable`) as required by new layers
  - training arch references, model digest handling
  - certainty no claims that lack evals
- **Tests.**
  - `cargo test --workspace --locked` where DB is available
  - `cargo test -p oml-app --test rerank`
  - new fusion/sparse/vector/rerank gates
  - ADR eval checks and schema numbers
  - builder/CI checks exposed in `cd/cd.rs` or equivalent?
- **Failure modes.** Each new mode labels any candidate leg, and OML's same posture:
  `search.not_available`, `search.pipeline_unavailable`, and invalid setting family
  remains the rule.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| Direct PostgreSQL replacement with an external vector DB | Breaks authority, audit/playbook and row-safe memory's service semantics (`storage.md` and roles-and-keys pages). Breaks ADR-0013. |
| Full context for every recall | Cost and long-context degradation; recall query text treatment (ADR-0025, ADR-0057) remains costly and context-limited. |
| Dense embedding baseline alone | Quality may rise, but isolated read security remains questionable. (Measured only under a model and scope gate.) |
| ColBERT as first step | Storage/time expansion without evidence or integration availability. |
| Automatic graph extraction, first | extraction is poor to trust and consistency burden (ADR-0047 d1 already rejects broad per-symbol code mapping; graph extraction needs eval harder than semantic rerank). |
| LLM rewriting all queries (HyDE) | Extra latency plus fabrication risk; sensitive-input egress; no default per scope restrictions without TODO. |
| Lossy compression in the memory core | Threatens provenance and reviewability. |

## Trial eligibility

A new algorithm leg is not a built tool until all gates pass. In particular it should
support dual-serving with existing lexical pipeline and 0-1-0.8 style experimentally
projection中心城区 only.

## Open questions

1. p95 budget class for `rerank`.
2. Model digest/rewriting tool chain is reviewable, as in code skeleton
   (`ADR-0047` d4).
3. Recall quality workloads per use case: how many acceptable cases use only what
   lexical ranking does achieve.

## Rejected roads, short

- **One model over all memories.** The use case stage ordering and trust ceiling.
- **Early dense.** Without integration conditions, we cannot say it is better.
- **A new dictionary for each leg.** Telemetry and provenance in OML require a singular
  external model, which makes it harder to audit.

## Checklist for the build

- [ ] ADR amendment accepted.
- [ ] Evaluation corpus fixed.
- [ ] Candidate parameters recorded.
- [ ] Schema/index/pipeline/model digests added.
- [ ] `search.enabled` / default unchanged or explicitly moved.
- [ ] Read-boundary tests passed.
- [ ] Eval and CI pass.
