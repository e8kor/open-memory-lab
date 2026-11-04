<!--
id: adr/ADR-0048-hybrid-search-for-every-memory-filters-keywords-and-meaning-fused-under-one-budget
tier: engineering
status: proposed
answers: how recall combines structured filters, the keyword search OML serves and a semantic search over embeddings, for every kind of memory; how the candidate lists are fused and budgeted; how held rows are treated so the constitution holds; which embedding model runs where, at what cost and with what privacy; what of the earlier vector design is reused; how the agent asks for it without a new tool; what must not degrade; which downsides are arguments and which are opportunities; and the questions left for the maintainer
-->

# ADR-0048 — Hybrid search for every memory: filters, keywords and meaning fused under one budget

**Status:** proposed · 2026-11-04

**Release:** planned, not assigned ([releases](../product/releases.md#decisions))

**Amends:** ADR-0013 (d4), ADR-0025 (d8, d13), ADR-0039 (d4); answers RS-1 at acceptance

## Context

**Recall today matches words, all of them.** A recall is PostgreSQL full-text search over
promoted text under the `simple` configuration: every word of the query must match and nothing is
stemmed ([retrieval](../architecture/retrieval.md)). The plugin teaches agents to recall with two
to four exact words for that reason
([ADR-0038](ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md)
d3, d6). A memory that says "release" is not found by "deploy"; one that says "the lead refuses to
start" is not found by "server won't boot". This is the main gap between OML and every product the
code-map survey of 2026-11-04 looked at, and it limits ordinary memories as much as code
knowledge ([ADR-0047](ADR-0047-project-knowledge-and-a-code-skeleton-follow-the-development-lifecycle.md)).

**The vector design exists; no code does.** [ADR-0013](ADR-0013-reads-served-from-postgres.md)
d4 decided, and [retrieval](../architecture/retrieval.md#pgvector-layout) declares, a
`memory_embeddings` table list-partitioned per tenant with an HNSW index per partition, row-level
security on top, iterative scan, a workarea-path filter column, embeddings computed by a separate
`oml-indexer` workload holding no signing key, embedding rows in the erasure inventory, and a
fusion rule for held rows that have no embedding. [ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md)
d4 names the embedder port. The model and where it runs are open as [RS-1](../product/roadmap.md);
quality and latency as [FR-4](../product/roadmap.md); the whole tier is not scheduled
([releases](../product/releases.md#not-scheduled)). A search of every branch on 2026-11-04 found no
embedding code: no `oml-vectors` or `oml-embed-*` crate exists, the files named `vectors` are
test vectors for the journal's chain format, and `feat/vocab-item5` holds vocabulary release
vectors, unrelated. What is reusable is the design above, whole.

**What the evidence says, labelled.** Cursor, measuring its own product, reports that semantic
search gave "on average 12.5% higher accuracy in answering questions (6.5%–23.5% depending on the
model)" and that "the combination of these two leads to the best outcomes" with grep
([Cursor, 2025-11-06](https://cursor.com/blog/semsearch)). Sourcegraph, in the other direction,
says "Cody does not support embeddings on Cody Enterprise because we have replaced them with
Sourcegraph Search" ([Cody FAQ](https://sourcegraph.com/docs/cody/faq)); Continue deprecated its
embeddings-based `@Codebase` ([Continue](https://docs.continue.dev/reference/deprecated-codebase)).
Both moves were away from embeddings as the *only* retrieval for code, which this record does not
propose. Nothing is measured on OML.

**The rule that must hold.** "R3, the gate changes visibility, not ranking. Nothing is readable
by others until it is ratified. Review never reorders results, and steering rows, which precede
hits, are never ranked" ([ADR-0001](ADR-0001-the-constitution.md) d3). Decided, not built.

## Decision

- **d1 — A recall draws candidates from up to two ranked lists inside one filtered set, and fuses
  them by reciprocal rank.** The **filters** (d6) decide the candidate set before anything is
  ranked: tenant, the key's read access and ADR-0030's boundary (on its branch), and the
  request's kind, workarea, anchor and `claim_key` filters. Inside that set, the **keyword list**
  is today's full-text search, unchanged, and the **semantic list** is the nearest neighbours of
  the query's embedding under the HNSW index. Each list keeps its first `k × 4` candidates, and
  the two are fused by reciprocal rank fusion: a row's score is the sum over the lists it appears
  in of 1 / (60 + its rank there), the constant from Cormack, Clarke and Büttcher's SIGIR 2009
  paper, unmeasured on OML (FR-81). Ties break by `artifact_id`, as ADR-0025 d8 orders today. Rank
  fusion needs no normalisation of `ts_rank` against a cosine distance, which ADR-0038 found has
  no fair form.

- **d2 — Visibility is decided before ranking, and review never reorders.** A row the key may not
  read is never a candidate, so no ranking ever sees it. Held rows stay out of the shared
  indexes. A held row the key may read (its own, with `include_unreviewed`, or inside an
  in-progress workarea under ADR-0030) is scored by the same function as a promoted row: it
  enters the keyword list as today, and enters the semantic list through a **fenced held
  embedding** stored beside the fenced held full-text row
  ([held text containment](../architecture/retrieval.md#held-text-containment)) under the same
  containment, so a held row ranks where it would rank once promoted. Promotion moves the
  embedding to the shared index without recomputing it. This amends retrieval's rule that held
  rows have no embedding, which made held rows rank below their promoted place. Steering stays
  first, unranked, and outside fusion. The constitution's sentence holds as written.

- **d3 — The embedding model runs inside the deployment by default, behind the embedder port.**
  Three places were weighed:

    | Where | Cost | Privacy | Verdict |
    |---|---|---|---|
    | **Inside the deployment**: `oml-indexer` runs an open text-embedding model on the CPU through `candle`, a pure-Rust inference library | CPU per promoted row and per query; one model file in the image | no memory text leaves the deployment | default |
    | **A provider's API**, through an `oml-embed-*` plugin | per-token fees; a network hop per query | every memory's text and every query go to the provider | allowed per deployment, journalled as a setting, named on the user page |
    | **On the client** | none for the server | none leaves the client | rejected: every client would need the same model, and the server could not tell a computed vector from a planted one |

    Inside the deployment keeps [ADR-0006](ADR-0006-pure-rust.md)'s pure-Rust rule, which
    leaves room for an external endpoint without foreclosing it. The model is one per deployment,
    its identifier stored with every embedding row; changing it re-embeds the tenant, and recall
    serves keyword-only until that ends. Which open model, and its dimension, is chosen by FR-81's
    measurement, not here. This answers RS-1 at acceptance.

- **d4 — The query is embedded at recall, and a promoted row is embedded after its promotion.**
  `oml-indexer` reads the outbox and embeds each promoted version's text, so a promotion is found
  by keywords at once and by meaning after the indexer's lag, unmeasured (FR-82). The recall
  embeds the query in the request, under a timeout of its own; a timeout or a missing index
  serves the keyword list alone and marks the result `degraded: "semantic_unavailable"`, never an
  error. Query embeddings are not stored and not logged; the read log keeps its digest of the
  request (ADR-0025 d9). Erasing a memory removes its embedding, as the inventory already says.

- **d5 — Each hit says why it was served.** A hit gains `matched`: the lists it came from
  (`keyword`, `semantic`) with its rank in each, and the filters it passed (`anchor`, `kind`,
  `claim_key`). It carries no raw score, since scores do not compare across lists or servers. An
  agent that sees a semantic-only hit knows its words were not in the text; a person on the user
  page sees why a memory surfaced.

- **d6 — `oml_read` gains a mode and filters, and no tool.** In recall mode: `mode` (`hybrid`,
  `keyword` or `semantic`; default `hybrid` once d9's gate passes, `keyword` until then);
  `workareas`, up to four scopes read as one, each under its own access and boundary, so an agent
  on a branch reads its branch and the repository's committed knowledge in one call (ADR-0047 d5);
  `anchor_repo`, `anchor_path_prefix`, `anchor_symbol` and `claim_key_prefix`. In listing mode:
  the same filters. `kinds` keeps its meaning, and a kind kept out of the default set, such as
  `code` (ADR-0047 d2) or the vocabulary kinds, is served only when named. `k`, `max_tokens`, the
  budget unit and the longest-prefix cut are unchanged
  ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d6, d7). The HTTP recall and
  `GET /v1/memories` take the same fields. This amends ADR-0039 d4 and ADR-0025 d8, d13; ADR-0039
  d8 is met by parameters.

- **d7 — Filters run inside the scan.** The anchor filters and `claim_key_prefix` are indexed
  columns on the full-text and embedding rows, as the workarea path already is
  ([retrieval](../architecture/retrieval.md#pgvector-layout)), so a filtered semantic search does
  not under-return by discarding candidates after the scan. Iterative scan stays on; its ceiling
  is part of FR-4.

- **d8 — Ordinary recall must not get worse.** The keyword list is computed exactly as today, so
  `mode: keyword` returns today's results. A tenant switch, `search.semantic`, turns the semantic
  list off. The recall's latency budget is the keyword path's plus the query embedding, measured
  under FR-82 before `hybrid` becomes the default.

- **d9 — A labelled recall set gates the default.** Before `hybrid` becomes the default, a
  labelled set of queries over a fixed corpus (ordinary memories from the maintainer's own use and
  `code` entries from ADR-0047's evaluation, each query with the memories a person marked
  relevant) measures recall at `k` 3 and 10, and latency at stated corpus sizes, for each mode.
  `hybrid` becomes the default when it beats `keyword` on recall at no worse than the stated
  latency. The set's size is unmeasured (FR-81).

- **d10 — The build, each step closed by named tests.**
    1. **Filters first**, useful without vectors: `claim_key_prefix_and_anchor_filters_narrow_recall_and_listing`,
       `workareas_reads_each_scope_under_its_own_access`, `a_hit_names_the_lists_and_filters_it_matched`.
    2. **Indexer and embeddings:** `the_indexer_embeds_a_promoted_version_once`,
       `a_held_embedding_is_served_only_to_a_key_that_reads_the_held_row`,
       `promotion_moves_the_held_embedding_without_recomputing_it`,
       `erasure_removes_the_embedding`.
    3. **Fusion:** `rrf_orders_by_the_sum_of_reciprocal_ranks_then_artifact_id`,
       `a_held_row_ranks_where_it_would_rank_promoted`,
       `steering_stays_first_and_unranked_under_hybrid`,
       `mode_keyword_returns_todays_results_byte_for_byte`,
       `a_query_embedding_timeout_serves_keyword_marked_degraded`.
    4. **The labelled set** (d9) and the switch of the default.

- **d11 — Every new failure has a doctor line.** `oml doctor` (admin) names: an indexer lagging
  behind the outbox, with its age; a tenant whose embeddings name more than one model (a
  re-embedding in progress); a provider plugin configured, with the provider's name; recall
  answering `degraded` above a share in the last hour; and the HNSW iterative scan hitting its
  ceiling.

## Downsides: argument or opportunity

| Downside | Verdict | Reasoning, and what it implies |
|---|---|---|
| Keyword-only recall misses paraphrase | solved by this record | d1; the keyword list stays for exact names, which embeddings handle poorly. |
| Embeddings move text off the machine | argument, scoped | Inside the deployment by default (d3); a provider is the deployment owner's explicit, journalled choice. |
| Cost: CPU per row and per query | argument | Unmeasured (FR-82); the switch (d8) and the keyword fallback bound it. |
| Semantic results are hard to explain | opportunity | `matched` (d5) explains each hit better than today's bare rank. |
| Code-search vendors moved away from embeddings | argument, answered | They dropped embeddings as the only retrieval; d1 keeps keywords and filters beside them. |
| Held rows ranked below their place | solved | Fenced held embeddings (d2). |
| A fourth index to erase | argument | Already in the inventory (ADR-0013 d4); erasure itself is not built (ADR-0005). Implies no new obligation. |
| Model drift on change | argument | One model per deployment, re-embedding with keyword fallback (d3). |
| Unproven on OML | argument | Implies the gate (d9). |

**What must not degrade, and how it is protected.** R1–R2: search writes nothing and reviews
nothing. R3: a row the key may not read is never a candidate, and held rows are scored by the
same function as promoted ones (d2). R4: embeddings are derived rows, erased with their memory.
The three MCP tools stay three (d6). Ordinary recall: `mode: keyword` is today's search exactly,
and `hybrid` is the default only after it measures better (d8, d9). Review keeps its meaning:
nothing in ranking depends on a verdict.

## Consequences

- **Roadmap rows.** RS-1 is answered by d3 at acceptance, its model choice deferred to FR-81.
  FR-81 opens (the labelled set's size, the fusion constant and the model); FR-82 opens (indexer
  lag, query-embedding latency and CPU cost). FR-4's exit stays as written and runs against d3's
  choice.
- **The tier moves from not scheduled to planned, not assigned** at acceptance.
- **Pages to change at acceptance.** Retrieval (fusion, held embeddings, filters inside the
  scan), the schema (embedding and filter columns), the HTTP API, openapi.yaml, the agent plugin
  and mcp-tools.json (`mode`, the filters, `matched`, `degraded`), the `oml-memory` skill (recall
  by meaning, not two exact words), configuration (`search.semantic`, the embedder), and each
  amended record's Status line.

## Open questions for the maintainer

| # | Question | Options | Recommended |
|---|---|---|---|
| 1 | Where does the model run? | a. inside the deployment, pure Rust; b. a provider's API by default; c. on the client | a (d3) |
| 2 | Are held rows embedded? | a. yes, fenced like held full-text rows; b. no, held rows rank by keywords only | a (d2): ADR-0047's branch areas are mostly held |
| 3 | Which fusion? | a. reciprocal rank, constant 60; b. a weighted sum of normalised scores; c. semantic re-ranks the keyword list | a (d1): no score normalisation needed |
| 4 | What becomes the default mode, and when? | a. `hybrid` after the labelled set; b. `hybrid` at once; c. `keyword` always, `hybrid` on request | a (d9) |
| 5 | Does `workareas` (several scopes) belong here? | a. yes, up to four; b. no, an agent recalls once per scope | a (d6): one budget across scopes |
| 6 | Should the filters ship before the vectors? | a. yes, as step 1; b. together | a (d10): ADR-0047 needs them and they need no model |
| 7 | Is a provider plugin allowed on the public server? | a. no, inside only; b. yes, disclosed | a: the public server's users chose no provider |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A fourth MCP tool, `oml_search`** | ADR-0039 d8: `mode` and filters on `oml_read` carry it. |
| **Semantic search alone** | Exact names, identifiers and error strings are what keywords find best; the vendors that dropped embeddings dropped them as sole retrieval. |
| **A separate vector database** | ADR-0013 d1 serves every read from PostgreSQL so every read-time rule is enforced where it lives. |
| **Filtering after the vector scan** | Under-returns silently; retrieval already rejects it. |
| **Re-ranking with a model at query time** | A second model call per recall with no measurement; revisit after FR-81. |
| **Boosting reviewed rows** | Review would reorder results, which R3 forbids. |
