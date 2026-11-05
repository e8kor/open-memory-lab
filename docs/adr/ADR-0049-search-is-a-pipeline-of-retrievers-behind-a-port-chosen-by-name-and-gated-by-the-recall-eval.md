<!--
id: adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval
tier: engineering
status: proposed
answers: how search plugs into the hexagonal core; which port a retriever implements, what the core hands it and what it may never decide; where fusion, the budget cut, steering and held rows live; what a named pipeline is and which ones v0.5.5 serves; how a deployment, a tenant and a request each choose one, and what is refused; what a recall answers about the pipeline, the matched words and a fallback; the laws every adapter passes; how the recall eval gates a pipeline before it may be the default, and what it measured; which API changes break v0.5.0 callers and how to migrate; the doctor lines, the request log field and the metric; the increments after this one; and how this record relates to ADR-0048
-->

# ADR-0049 — Search is a pipeline of retrievers behind a port, chosen by name and gated by the recall eval

**Status:** proposed · 2026-11-04

**Release:** introduced in v0.5.5 (increment 1, with ADR-0048's S1–S3 as its pipelines); later
increments not assigned ([releases](../product/releases.md#decisions))

**Amends:** ADR-0048 (proposed, on branch `feat/code-map`): ADR-0048 stays the record of *what*
search becomes and *when*, stage by stage; this record is *how* each stage plugs in. ADR-0048
d10's `search` values (`keyword`, `text`, `hybrid`) are replaced by pipeline names (d4), and its
default rule is replaced by d9's. Also ADR-0025 (d8: the score is a pipeline's; d9: the read
digest gains `search` and `match`), ADR-0038 (d3: the recall line; d6: the failure hook runs the
default pipeline), ADR-0039 (d4: `oml_read` gains `search` and `match`), ADR-0042 (d2: the
request line gains `search`; d12: one metric family)

## Context

**Recall was one statement.** Up to v0.5.0 the store port carried `recall_fts` and
`recall_own_held`, and the PostgreSQL adapter answered both with `plainto_tsquery('simple', …)`,
every word required, ranked by `ts_rank`. The in-memory test store, which `oml-app`'s use-case
tests run on, kept a row when *any* word matched, so the tests exercised a more forgiving recall
than users got. OML's recall eval (`eval/recall/`, 200 memories, 121 graded queries in 11
categories) measured the served statement at success@5 0.17 with 83% of queries answered by
nothing ([eval report](https://github.com/e8kor/open-memory-lab/blob/feat/recall-eval/eval/recall/REPORT.md)).

**ADR-0048 decided what search becomes.** Seven stages ordered by measured gain per cost: any
word (S1), identifier parts, stems and rarity (S2), trigrams fused by reciprocal rank (S3),
filters (S4), write-time keywords (S5), a reranker (S6) and embeddings (S7), each gated on the
eval. It left open how a stage enters the code without each one rewriting the recall use case.

**The operator's direction, 2026-11-04.** Target the best measured search design; make search
pluggable so its behaviour can change at deployment time and on the fly; fit the hexagonal
architecture ([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md)); improve in
increments. Recall and remember are the product, and breaking API compatibility before 1.0 is
allowed, as [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) did.

**The rule that must hold.** "R3, the gate changes visibility, not ranking … Review never
reorders results, and steering rows, which precede hits, are never ranked"
([ADR-0001](ADR-0001-the-constitution.md) d3).

## Decision

- **d1 — The goal pipeline is several retrievers, fused, optionally reranked, then the core's
  budget cut.** Candidate generation by a keyword retriever with rarity weights (BM25-class), a
  trigram retriever and, later, a dense retriever; fusion by reciprocal rank; an optional
  reranker over the top 50; then the `k` and `max_tokens` cut the core already applies. This is
  ADR-0048's S7 with S6, the best its measurements found (hybrid 0.97 success@5 offline, a
  reranker +0.09 success@1 in scratch). It is reached one increment at a time (d10), each an
  adapter or a pipeline, never a change to the core's invariants.

- **d2 — A `Retriever` port ranks; the core decides what may be ranked.** `oml-app`'s
  `ports::Retriever` has three methods: `retrievers()` names the retriever kinds the adapter
  serves; `retrieve(tenant, kind, scope, query)` returns one ranked list, each candidate with its
  id, its `body_bytes`, the retriever's score and the query terms it holds; `suggest(tenant,
  scope, words, limit)` returns spellings for `did_you_mean`. The **candidate scope** is computed
  by the core and handed in whole: the readable hit nodes (subtree ∩ reader, joined by
  `universe` for a user root, ADR-0044 d10), the hit kinds, and the own held keys and nodes only
  when `include_unreviewed` asked. An adapter never widens it: an SQL adapter runs under row
  security as well, and the core re-checks every row it serves, as before. Steering is read by
  the core, first and unranked, and never enters a retriever, fusion or a reranker. Held rows
  enter only as ADR-0025 d4 and ADR-0030 allow, and a held row is scored by the same function
  as a promoted one. No ranking input reads a verdict, a reviewer or `promoted_at`. The journal
  store port is a `Retriever` too (`JournalStore: Retriever`), because the keyword retrievers
  read the same PostgreSQL rows ([ADR-0013](ADR-0013-reads-served-from-postgres.md) d1); a
  retriever over another store, as an embedder's index would be, gets a handle of its own. Cost:
  the port is generic, not `dyn`, like every port here, so a new retriever kind is a new enum
  variant and a release, not a runtime plugin; [ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md)'s
  out-of-process plugin route stays open for a provider-backed embedder (d10).

- **d3 — Fusion is pure and lives in the core.** `search::fuse` merges a pipeline's lists:
  `Single` serves one list in its own order; `Rrf { k: 60 }` scores each candidate by the sum of
  `1 / (60 + rank)` over the lists that hold it, the constant ADR-0048 d4 names. It reads ranks
  and scores only, unions each candidate's matched terms and records which retrievers found it,
  and orders by score descending then id ascending, so equal input gives equal output. A fused
  pipeline reads each list 50 deep before fusing, so a candidate one list ranks first is not lost
  to another's cut. The budget cut stays after fusion, in the core, unchanged
  ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d7).

- **d4 — A pipeline is chosen by name: the deployment enables, a tenant picks, a request may
  name.** A **pipeline** is a fixed list of retriever kinds and a fusion; the build knows four:

    | Pipeline | Retrievers | Fusion | Stage |
    |---|---|---|---|
    | `keyword-all` | every word, exact, `simple`, `ts_rank` | single | v0.5.0's recall, kept for comparison |
    | `keyword-any` | any word, exact, `simple`, `ts_rank` | single | S1 |
    | `bm25` | identifier parts, `simple` and `english` lexemes, BM25 over the readable rows | single | S2 |
    | `bm25-trigram` | `bm25` and `pg_trgm` word similarity | reciprocal rank, 60 | S3 |

    The deployment's `search.enabled` lists the pipelines it serves (default `keyword-any` and
    `bm25`; `bm25-trigram` when a deployment names it, d12) and `search.default` the one a recall naming none runs (default
    `bm25`, by d9 and d12); `OML_SEARCH__ENABLED` and `OML_SEARCH__DEFAULT` in the environment.
    A tenant's admin sets the tenant's default among the enabled ones with `POST
    /v2/search/default` or `oml search set-default <pipeline>`, journalled as
    `search.default_set` with the name and the admin key, folded into `tenants.search_default`,
    and in force from the next request with no restart; `deployment` (or `null`) clears it.
    `GET /v2/search` and `oml search show` answer the enabled set, both defaults and the one in
    effect. A request names one with `search` on `oml_read` and on `POST /v1/memories/recall`;
    no new MCP tool (ADR-0039 d8). Precedence: the request's, else the tenant's while the
    deployment still serves it, else the deployment's. A name the build does not know, or the
    deployment does not enable, is refused `request.validation` with `details.field: "/search"`
    and the served names, never replaced by another pipeline; the same name on the admin route is
    refused the same way. The names are a closed set, so the request log and the metric stay
    bounded. Per-request free-form pipelines are not offered (rejected alternatives).

- **d5 — `match` chooses any word or every word, and a fallback says so.** `match: "any"` is the
  default. `match: "all"` keeps only rows holding every query word, in every retriever; when that
  leaves nothing, the core runs the pipeline again with any word and answers
  `search.fallback: "any_term"`. Under `all` the trigram list is empty, since a similarity is
  not a word match. `keyword-all` needs every word under either value and never falls back.

- **d6 — Every recall says what ran and why each hit matched.** The response gains `search`:
  `pipeline`, `match`, `terms` (the query's words as the pipeline read them), `fallback` (`null`
  or `any_term`) and `did_you_mean` (spellings from readable rows alone, for a thin recall; else
  `[]`; d12 replaces this record's first reading, a trigram pipeline whose hits all miss a word). Each hit gains `matched`: `terms` (the
  query words it holds), `of` (how many the query had) and `via` (the retrievers whose lists
  held it: `keyword_all`, `keyword_any`, `bm25`, `trigram`). The score stays, comparable within
  one answer only. The read log's `query_digest` gains `search` (the pipeline that ran) and
  `match`. This is ADR-0048 d9's `matched`, with `via` naming retrievers rather than match ways;
  its `explain` counts are not built in this increment.

- **d7 — Laws every adapter passes, in-memory and PostgreSQL alike.** `oml-port-contracts`
  holds the retriever laws, run by `testing/oml-port-contracts/tests/doubles.rs` against the
  in-memory store and by `crates/adapters/oml-pg/tests/pg_store.rs` against PostgreSQL, for every
  retriever kind the adapter serves: `a_retriever_never_returns_a_row_outside_the_scope`
  (another node's row, another key's held row, steering, a held row when no own keys are named;
  an empty scope ranks nothing); `a_retriever_orders_deterministically_by_score_then_id`;
  `a_row_outside_the_scope_never_changes_a_candidates_score` (so a rarity statistic cannot leak
  unreadable rows); `the_keyword_retrievers_match_all_or_any_word_on_every_store` (independent
  expected sets, so the two stores agree with each other); and, where served,
  `bm25_finds_an_identifier_by_its_parts_and_a_word_by_its_stem` and
  `trigram_finds_a_misspelled_words_memory` (with `did_you_mean` taken only from the scope).
  PostgreSQL alone proves `keyword_all_returns_exactly_the_v0_5_0_rows_and_order`: the two v0.5.0
  statements, kept byte for byte as `RECALL_FTS` and `RECALL_OWN_HELD`, merged and cut as the core
  merged them, equal the `keyword_all` retriever's list for every query and `k` of a fixture.
  The in-memory store serves `keyword_all` and `keyword_any` only, with PostgreSQL's semantics on
  plain words; its tokenizer still splits where PostgreSQL's parser keeps a path or a version
  whole, which the laws avoid and this record names. A composition over it that enables `bm25`
  is refused at start.

- **d8 — Every failure has a refusal or a doctor line.** A configuration naming an unknown
  pipeline, an empty set, or a default outside the set stops the start. A pipeline whose
  retriever the store cannot run stops the start naming both; for `bm25-trigram` that is
  `pg_trgm` absent, with the remedy: `CREATE EXTENSION pg_trgm` as a role with `CREATE` on the
  database, then restart. Migration 0020 creates the extension as the owner role, which holds
  `CREATE` where it owns the database, as every documented profile has it
  ([ADR-0034](ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d2); `pg_trgm` is a trusted
  extension, so no superuser step is needed there, and where the owner lacks `CREATE` the
  migration goes on without it; the lead then serves the default pipelines, which need none
  (d12). `/readyz` names the enabled set and the default. `oml doctor` prints
  `search: default <p>; enabled <list>`, warns when the default is `keyword-all`, and, read by an
  admin key, warns when the tenant's journalled default is one the deployment no longer serves
  (the deployment's then applies). The request line gains `search` and `search_fallback`; the
  metric `oml_recalls_total{pipeline, fallback}` counts recalls by pipeline.

- **d9 — The default is the best pipeline that passes its gate in CI, measured, not planned.**
  The recall eval runs against the served pipelines through `oml_read`, in CI, as the job
  `recall-eval` (`eval/recall/served.sh`: a lead and a throwaway database, the corpus written by
  one key and approved by another, then `run.py --search <pipeline>` for each, then
  `gate.py`). A pipeline may be the default only when its gate passes, and the job fails when the
  default's does not, when any category of it falls by more than one query against its committed
  result, or when `keyword-all` does not reproduce v0.5.0's served lists (scores and members at
  each score; the order inside a tie follows ids a fresh load draws anew). The gates are
  ADR-0048 d1's: `keyword-any` success@5 ≥ 0.70 and zero results ≤ 0.05; `bm25` success@5 ≥
  0.86 and success@1 ≥ 0.67; `bm25-trigram` success@5 ≥ 0.92 with partial and typo ≥ 0.91.
  Measured on 2026-11-04 (data version 1, 200 promoted rows, `k` 10, `max_tokens` 2000, through
  `oml_read` over `/mcp` on this branch's release build, PostgreSQL 18 on the 8-core host, one
  client on loopback; committed as `eval/recall/results/served_*.json`):

    | Pipeline | success@1 | success@5 | recall@10 | nDCG@10 | zero results | median ms | p95 ms | gate |
    |---|---:|---:|---:|---:|---:|---:|---:|---|
    | `keyword-all` | 0.17 | 0.17 | 0.12 | 0.16 | 0.83 | 15.9 | 18.8 | reproduces v0.5.0 |
    | `keyword-any` | 0.65 | 0.81 | 0.73 | 0.69 | 0.03 | 19.4 | 29.6 | passes |
    | `bm25` | 0.82 | 0.91 | 0.91 | 0.86 | 0.00 | 33.9 | 52.1 | passes |
    | `bm25-trigram` | 0.78 | 0.94 | 0.93 | 0.86 | 0.00 | 73.9 | 100.6 | passes |

    So `bm25-trigram` was the first default: the highest success@5 among the pipelines that pass;
    partial and typo reach 1.00. *d12 replaces it with `bm25`, on the same gate and the public
    benchmarks this table did not have.* `bm25` ranks the first hit better (0.82 against 0.78) at under half the
    latency; a tenant that wants it sets it (d4). A run on a debug build the same day measured
    0.83, 0.92 and 0.95 success@5 for the last three: ties break by artifact ids a fresh load draws
    anew, so a run moves by about one query. `keyword-any` measured 0.81, above ADR-0048's 0.73
    for `simple` OR, which ranked by `ts_rank_cd`; this pipeline ranks by `ts_rank`. The eval's
    caveats hold: one model wrote the corpus, queries and labels; no held rows, one workarea, no
    unanswerable query, so false positives from any-word matching are unmeasured (FR-83).
    Latency: ADR-0048 d11's budget, a p95 at most twice the every-word recall's, is met by
    `keyword-any` (1.6 times) and missed by `bm25` (2.8 times) and `bm25-trigram` (5.4 times).
    The trigram list is the larger cost: no function is `LEAKPROOF`, so under row security the
    `<%` operator scans the tenant's partition rather than using its index (ADR-0034 d13). The
    default is chosen by the gate, as directed, and the latency is a measured cost carried
    openly: FR-84 measures both statements at 10,000 and 100,000 rows before the public server's
    default changes, and a deployment that cannot pay it sets `search.default = "bm25"`.

- **d10 — Increments, each an adapter or a pipeline.** *Increment 1 (this record's build):* the
  port, pipeline selection at all three levels, `keyword-all` moved behind the port unchanged,
  the in-memory store made to agree with PostgreSQL, `keyword-any` (S1), `bm25` (S2: migration
  0020's `oml_search_text` normaliser and `oml_search_vector`, BM25 in SQL over the readable rows)
  and `bm25-trigram` (S3: `pg_trgm`, reciprocal rank fusion, `did_you_mean`), the eval job, and the
  default set by the gate. *Next:* S4, filters inside the candidate scope (`kinds` as today;
  `since`/`until`; up to four `workareas`; ADR-0047's anchors), which extend `CandidateScope`,
  never an adapter's own predicate; the `explain` counts of ADR-0048 d9; eval version 2
  (unanswerable, filter, held and supersession queries; FR-83). *Then:* S6, a `Reranker` port
  (`rerank(query, candidates) -> reordered candidates`, reading only candidates the core
  passes, storing nothing) with a `candle` cross-encoder adapter, run after fusion and before the
  cut, opt-in per request (`rerank: true`) until its p95 fits d11 of ADR-0048 (FR-85). *Then:*
  S7, an `Embedder` port (`embed(texts) -> vectors`, in the deployment by default, a provider
  only by the owner's journalled choice) and a `pgvector` dense retriever adapter, and the
  `hybrid` pipeline fusing `bm25`, `trigram` and dense by reciprocal rank. Each new pipeline
  enters the build disabled, joins the eval job, and becomes the default only through d9's gate.
  S5 (write-time keywords) is a field of `oml_write` and a weighted vector, a change to the
  `bm25` adapter's statement, not to the port.

- **d11 — The breaking changes, and how to migrate.** Pre-release, so the cleanest contract wins
  over shims (ADR-0039):
    1. **The default recall matches any word.** A v0.5.0 caller that relied on every word being
       required gets more hits: send `search: "keyword-all"` (while the deployment enables it) or
       `match: "all"`.
    2. **The recall response gains `search`, and each hit `matched`.** A client that refuses
       unknown members must accept them; `oml-sdk` layers are regenerated.
    3. **`query_digest` changes for every recall**, since it digests `search` and `match`: a read
       log compared across the upgrade does not match by digest.
    4. **`bm25-trigram` needs `pg_trgm`.** It is not in the default `search.enabled` (d12); a
       deployment that names it on a database without the extension must create it once as a
       role that can, and the lead refuses to start otherwise, naming both.
    5. **The store port changed**: `JournalStore::recall_fts` and `recall_own_held` are gone,
       replaced by `Retriever::retrieve`; an out-of-tree adapter implements the new port.
    6. **The plugin's guidance changed**: the server instructions, the `oml_read` description,
       the `oml-memory` skill, the librarian and the session-start line no longer ask for "2-4
       exact keywords" and say any word may match; ADR-0038 d6's failure hook runs the default
       pipeline. Measured with the hook's own parameters (the query minus stop words, `k` 3,
       200 tokens; `results/hook_*.json`): success@3 0.24 under `keyword-all` with 75% of
       queries answered by nothing, 0.88 under `bm25-trigram` with none empty, at a median of
       72.5 ms against 14.4 ms. So the hook now nearly always injects up to three hits under its
       "may be irrelevant" line, and they are right far more often; its 1-second timeout and
       breaker stand.

- **d12 — Lexemes are stored once, the vocabulary is kept by the write, and `bm25` is the
  default; decided on measurements, 2026-11-04.** The public benchmark harness (`eval/bench/`)
  found the first build's ranking sound (served `bm25` within 0.017 nDCG@10 of BEIR's BM25) and
  its latency the blocker: 3.7 s per query on 5,183 SciFact abstracts, because the `bm25`
  statement rebuilt every scope row's text vector per query (3.2 s of it) and `did_you_mean`
  rebuilt the scope's vocabulary per query (about 2 s under load).
    1. **Stored lexemes.** Migration 0020 adds `memories.search_vector` and `search_len`, set by
       a `BEFORE` trigger on every insert and on any update naming the claim or either column, so
       the vector is computed once per write (a generated `search_len` could not read the
       generated vector and computed it twice) and a write cannot store lexemes that are not its
       claim's. 0020 fills both for every existing row under each tenant's row security; a
       partition created later takes the columns through `LIKE … INCLUDING ALL` and the trigger
       through `ATTACH PARTITION`. No text index is built on them (item 5). The `bm25` statement reads them and no row's text
       (`the_bm25_statement_reads_the_stored_lexemes_and_never_a_rows_claim`). Row security and the
       per-tenant partitions are unchanged; the owner creates the columns, the serving role reads
       them. A v0.5.4-shaped database migrated forward ranks exactly as one written after 0020
       (`a_v0_5_4_database_with_memories_migrates_through_0020_…`).
    2. **Each occurrence counts once.** The first vector concatenated the text with its
       identifier-split copy and `simple` with `english`, so a word said once had a term
       frequency of two to four, and a word whose stem differs counted under two lexemes. The
       stored vector holds the `simple` words, then only the identifier parts the text lacks, then
       only the `english` stems neither holds, the stems taken once from the text and once from
       the parts it lacks (`runs` holds `run` once, so it never outranks a row that says `run`);
       a query word scores its best form; a query's
       English stop words are dropped when it has other words (they score near nothing and made
       nearly every row a candidate). Served, after the review's fix below: SciFact nDCG@10 0.660
       → 0.685, NFCorpus 0.308 → 0.321, LoCoMo 0.499 → 0.503. Laws: `bm25_scores_a_word_a_lone_row_says_once_at_its_weight`,
       `bm25_counts_an_inflected_form_once_as_it_counts_the_exact_word`,
       `bm25_reads_a_query_without_its_english_stop_words_unless_it_has_no_other`. The first
       build of this item still took the stems from the text and its split copy together, so
       an inflected word had two positions; an independent review found it, the law above
       failed on it (0.396 against 0.288), and the numbers in item 6 are measured after the fix.
    3. **Statistics stay per query over the readable scope.** Two readings were measured on
       SciFact ×10 (51,830 rows, ten workareas), statement only: (a) document frequencies taken
       from the rows that hold a query form, inside the readable scope, which are exactly the
       scope's document frequencies, with the row count and mean length from `search_len`; (b)
       per-workarea counts kept at write time and summed over the readable workareas. (b) saved
       about 4% (p50 802 against 836 ms at 50k rows, 80.8 against 80.5 ms at 5k), because the
       cost is reading the candidates' term frequencies, which (b) still pays, while it would add
       a counter to every promotion, supersession, retirement and demotion. (a) is built; no table
       holds a statistic across workareas, as the rejected alternative below requires.
    4. **`did_you_mean` from a maintained vocabulary, only when thin.** `search_vocabulary` holds,
       per workarea, the `simple` words of its promoted served tips of the kinds recall serves
       (`memory`, `derived_fact`) that are no retirement version, with the count of those rows; a trigger adds and removes words in the write's own
       transaction, and 0020 fills it for the rows already promoted. A recall offers spellings
       only when it found fewer than three hits (`DID_YOU_MEAN_BELOW`, or fewer than its `k`),
       for the words no hit holds, read from the readable nodes' vocabulary, under every pipeline
       but `keyword-all`, whose v0.5.0 answer had none. A held row's, another workarea's, a
       steering row's, another tenant's on the same node id, a replaced version's and a retired
       claim's words are never offered
       (`did_you_mean_draws_only_on_the_words_of_promoted_served_tips_in_the_scope`). The serving
       role scans the readable nodes' vocabulary: 58 ms per word over SciFact's workarea
       vocabulary of about 50,000 words in the served database, against about 2 s before. A
       trigram index on `word` does not help it: on a 50,001-word table under forced row
       security, `word % q` with `pg_trgm.similarity_threshold` 0.4 ran a sequential scan in 158
       ms as the policy-bound role, against 0.05 ms through the index as a role that bypasses the
       policy and 131 ms for the `similarity() > 0.4` form the statement uses, so no index is
       built (item 5).
    5. **No index the serving role cannot use.** `@@`, `%` and `<%` are not leakproof, so under
       forced row security no GIN or trigram index serves the serving role (ADR-0034 d13); 0020
       builds none on `search_vector`, `search_vocabulary` or the trigram text, which the first
       build of this record did, so no write maintains one. Measured with
       `ts_match_vq` marked `LEAKPROOF` on a throwaway cluster, the `bm25` statement's p50 went
       from 615 to 442 ms at 51,830 rows and was unchanged at 5,183, since most queries hold a
       word thousands of rows share: the `LEAKPROOF` step would not make `bm25` fast at that size.
    6. **The default is `bm25`.** Re-measured after the review's fixes (2026-11-05, release build
       of `d3fdb72`, PostgreSQL 17.9, the generated [benchmark pages](../api/benchmarks/index.md)):
       on the recall eval `bm25` passes its gate, success@5 0.926 and success@1 0.835 at a median
       of 16.3 ms and a p95 of 21.2 ms over 200 rows, and `bm25-trigram` reaches success@5 0.950
       but misses its own partial gate by one query (0.909 against 0.91). On the public sets
       `bm25` ranks above `bm25-trigram` by 0.074 nDCG@10 on SciFact (0.685 against 0.611), 0.054
       on NFCorpus (0.321 against 0.267) and 0.020 on LoCoMo (0.503 against 0.483), at a
       fortieth of its latency on abstracts (p50 83 against 3,707 ms at `k` 100). d9's rule, the
       highest success@5 among the passing pipelines, is replaced: the default is the passing
       pipeline that ranks best on the external sets, since the synthetic set's typo and partial
       categories, 22 queries one model wrote, should not outweigh 2,158 queries others wrote.
       `bm25-trigram` stays available to a deployment that enables it for misspellings, and a
       thin misspelt recall under `bm25` offers `did_you_mean`.
    7. **Hydration.** At `k` 100 a recall read each hit's workarea ancestry with its own
       recursive query; recall now reads each workarea's once
       (`a_recall_reads_each_served_workareas_ancestry_once_however_many_hits_it_serves`).
       SciFact's `bm25` at `k` 100 went from 176 / 214 ms (p50 / p95) to 88 / 119 ms on that
       change alone, and measures 83 / 113 ms after the review's fixes; at `k` 10 it measures
       71 / 99 ms, inside the target of a 100 ms p95 at 5,000 rows on abstract-length rows, the
       worst case OML's short claims meet. LoCoMo's turns answer in 31 / 38 ms at `k` 100.
    8. **Latency grows with the readable rows; it is not solved.** Under row security every
       pipeline reads the stored lexemes of every row the key may read. Measured: served `bm25`
       over 51,830 abstracts in one workarea, 604 / 849 ms at `k` 100 and 572 / 807 ms at `k` 10;
       the statement alone over 51,830 abstracts in the lab, 639 / 947 ms (before the stem fix),
       where the first build's statement took 15.3 s at its median. The scale lane
       (`bench/scale/REPORT-baseline.md`) puts an every-word match over a stored `simple` vector past 50 ms near
       70,000 readable rows; `bm25` reads more per row. Larger scopes need what item 13 names;
       this record does not claim them. Loading the 51,830 copies through `review next` took 37
       minutes, a few rows a second while tens of thousands were held (FR-87).
    9. **Room for a learned sparse leg.** An inference-free learned sparse retriever (a
       `(memory, token, weight)` table written at promotion and scored by a token-weight sum) would
       sit beside these columns as a table and a retriever of its own, under the same
       partitioning and row security; nothing here stores the lexical leg's statistics in a shape
       it would have to share.
    10. **Known gap.** NFCorpus measures 0.321 nDCG@10 served against BEIR's published 0.325 and
       Pyserini's single-field 0.322; an offline textbook BM25 over the same memories (bm25s, k1
       0.9, b 0.4, Snowball stems, stop words removed) measures 0.318. Before the stem fix the
       served figure was 0.315. What remains is about the mapping (title and abstract as one
       body) and the analyser (Snowball's `english_stem` against Lucene's Porter, PostgreSQL's
       parser keeping hyphenated words whole); it is recorded on the benchmark pages and not
       chased further here.
    11. **The default needs no extension.** `search.enabled` defaults to `keyword-any` and `bm25`;
       `bm25-trigram`, which needs `pg_trgm`, is served only where a deployment names it, and a
       lead that names it without the extension refuses to start naming it. 0020 goes on without
       the extension where the owner may not create it or contrib is absent, and `did_you_mean`
       then answers `[]`
       (`a_lead_with_the_shipped_search_starts_without_pg_trgm_and_one_naming_bm25_trigram_is_refused`).
    12. **Erasure must rewrite the lexemes.** `search_vector` holds the claim's words. Erasure is
       not built; when it is, it must rewrite `claim`, which the trigger turns into the new
       lexemes, and take the version out of the vocabulary; an erasure that changed only the
       status would leave the erased text in `search_vector`. The stored vectors also depend on
       the `english` configuration and the database's character classes: changing either needs
       the vectors recomputed (`UPDATE memories SET claim = claim` per tenant).
    13. **What comes next is decided elsewhere.** The operator decided on 2026-11-05 that text
       search moves to an integer token table, `(memory, token_id, weight)`, whose index row
       security can use; that is its own record. This item's columns and statement are the
       lexical leg until then.


## Consequences

- **Pages changed with this record**: [retrieval](../architecture/retrieval.md) (how recall
  works now, pluggable), [HTTP API](../api/http-api.md), [MCP plugin](../api/mcp-plugin.md) and
  `mcp-tools.json` (`search`, `match`, `matched`, `search` in the answer),
  `openapi.yaml`, [configuration](../operations/configuration.md) (`search.enabled`,
  `search.default`), the [CLI reference](../cli/reference.md) (`oml search`, `oml read --search
  --match`), the [releases](../product/releases.md) page, the eval's README and REPORT, the
  plugin's skill, librarian and README, and the server instructions.
- **Roadmap rows**: FR-83 (false positives of any-word matching, measured by eval version 2),
  FR-84 (the BM25 statement's and the trigram index's cost at 10,000 and 100,000 rows, and the
  latency budget), FR-85 (a reranker's CPU cost in `candle`) are ADR-0048's and stay open; this
  record adds none.
- **Costs**: the BM25 statement reads every readable row's stored lexemes per query, so its cost
  grows with the scope (d12); the stored vector and the vocabulary cost write time and storage: loading SciFact wrote 62 rows a second and approved 77 a second with
  8 writers and 6 reviewers, where the first build loaded LoCoMo at 98 and 103, and the
  vocabulary held 88,127 words for 14,000 promoted abstracts (20 MB); `pg_trgm` stays a requirement of
  `did_you_mean` and of `bm25-trigram`, not of the default ranking.
- **Roadmap rows**: FR-86 (chain-head contention under concurrent writes) and FR-87 (one row
  reviewed at a time per key), both found by the benchmark loader.
- **Expected conflicts**: v0.5.1's guardrails touch recall (a `guardrail` kind refused in
  `kinds`): the refusal belongs in the transport's kind parsing and the core's `HIT_KINDS`, both
  of which this change leaves in place, so the merge touches the same `recall` function in
  `oml-app` but not the retrievers. v0.5.2's brief reads promoted rows by confirmation, not by
  query, and its migrations are 0017–0019; this record's is 0020.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Search as a separate service now** | A second process and a second store of candidates, which row security does not reach and which ADR-0013 d1 rejects for reads; the port keeps that door open (an adapter may call one) without paying for it before a retriever needs it. |
| **Adapters that own visibility** | Each adapter would re-implement grants, denials, user roots and held-row rules, and one missed predicate leaks a row. The core computes the scope once and re-checks every served row; adapters rank only. |
| **Per-request free-form pipelines** | A caller composing retrievers and fusion constants would make every request a new, unmeasured ranking, unbounded metric labels, and no gate to pass. Named pipelines are measured before they are served. |
| **One hard-coded strategy, replaced in place** | Each stage would rewrite the recall use case and lose the comparison baseline; a tenant could not keep the old behaviour while the new one is judged. |
| **A runtime plugin per retriever (`dyn`, loaded at start)** | The ports are generic for `Send` futures without boxing (ADR-0019); a retriever is few and slow to add, so a release per kind costs less than an ABI. Revisit for provider-backed embedders. |
| **Per-workarea document frequencies kept at write time** (d12) | Measured 4% faster than computing them per query, because reading the candidates' term frequencies is the cost either way; it would add a counter to every state change of a row and a second source of truth for ranking. |
| **Improving `bm25-trigram` as the default (trigram only for words with no match)** (d12) | Its trigram list scans the tenant's partition under row security whatever words it is given, so the gain would be quality on misspellings alone; the external sets have none, and `did_you_mean` from the vocabulary now answers a thin misspelt recall. Left opt-in. |
| **A tenant-wide statistics table for BM25** | Lets rows a key cannot read move the ranks of rows it can (ADR-0048 d3); the law `a_row_outside_the_scope_never_changes_a_candidates_score` forbids it. |
| **Keeping "every word" as the default until 1.0** | The eval's 0.17 against 0.94 is the product's main gap, and pre-release breaking changes are allowed (ADR-0039); `keyword-all` stays nameable for anyone who needs it. |
