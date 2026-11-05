<!--
id: adr/ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget
tier: engineering
status: proposed
answers: where reranking sits in a recall and what the core hands it; which runtime runs the model (candle, tract or ort) and why; which model, on measured evidence and licences; how the model reaches an install, online and offline; how deep it reranks; the concurrency cap, the deadline and what a skipped rerank answers; how a relevance floor is defined, what an answer below it says, and whether it is on by default; the pipeline and setting names; the metric, request-line fields and doctor lines; the laws and tests; the rejected alternatives; and what stays open
-->

# ADR-0053 — Reranking is a local cross-encoder stage after fusion, off by default until it fits the latency budget

**Status:** proposed · 2026-11-05

**Release:** not assigned ([releases](../product/releases.md#decisions))

**Amends:** ADR-0049 (d4: a fifth pipeline name, `bm25-rerank`; d6: the answer's `search` gains
`rerank`, `rerank_reason`, `floor` and `below_floor`, and each hit's `matched` gains `rerank` and
`rank_before`; d8: one more refusal at start and one more metric family; d10: S6 is built as
this record decides, with the `Reranker` port held as `Arc<dyn Reranker>` and the pipeline,
not a request flag, as the opt-in)

## Context

**The plan's next step.** ADR-0048 orders search's stages by measured gain per cost; its S6 is a
reranker over the fused list, and ADR-0049 d10 sketched the port (`rerank(query, candidates)`,
reading only what the core passes, storing nothing), a `candle` adapter, and an opt-in until its
p95 fits ADR-0048 d11's budget (a p95 at most twice the every-word recall's). The operator
asked on 2026-11-05 for the reranker and, with it, a relevance floor, so a recall can answer
"nothing relevant" instead of serving the best of nothing: `bm25` never answers zero hits on the
recall eval (ADR-0049 d12).

**What was measured first.** An earlier scratch test put `ms-marco-MiniLM-L-6-v2` over the top
50 and raised the synthetic success@1 from 0.79 to 0.88 at 33 to 176 ms. The research note on
retrieval methods (2026-11-04; sbert's tables, BEIR Table 2) puts a cross-encoder over BM25 at
+11% nDCG@10 averaged over BEIR (SciFact 0.688, NFCorpus 0.350, against 0.665 and 0.325).

**The constraints.** The gate changes visibility, not ranking, so no ranking input reads a
verdict (ADR-0001 d3). The core computes visibility and a reranker may score only rows the core
would serve (ADR-0049 d2). No hosted model in the read path; the host is 8 vCPU with no GPU.
Licences must allow commercial use, which excludes `jina-reranker-v2` (CC-BY-NC-4.0). The
implementation is pure Rust (ADR-0006). The `oml` CLI ships static musl and Windows builds that
must not grow, since only the lead runs a model. Each hit keeps its `matched` explanation.

**The host the numbers come from.** Every latency below was measured on the 8-vCPU host while
other sessions' builds and evals ran beside it, a load average of 15 to 29 in the first round and 3 to 8 in the second, after an independent review; each table names its own; the relative order
of the runtimes and of the depths held across repeated runs, the absolute milliseconds did not,
and they are reported as measured, not as what an idle host would give (FR-88).

## Decision

- **d1 — Reranking is a stage of the core, after fusion and before the cuts.** A pipeline that
  reranks reads its first stage `search.rerank.depth` deep (or `k` deep, when `k` is larger),
  fuses as before, then hands the head of that list to the reranker: the core hydrates those
  rows, keeps only the rows it would serve (promoted served tips on the readable hit nodes, the
  caller's own held rows when it asked, no retirement version, the requested kinds: the same
  predicate the served loop applies, now evaluated once before the reranker and reused after),
  and passes the query and their texts. The head comes back ordered by relevance, ties keeping
  the first stage's order, which is itself total; the tail past the depth follows in the first
  stage's order. Then the `k` cut, the floor (d6), the budget cut and the re-check, as ADR-0025
  d7 and ADR-0049 d3 left them. Steering never reaches a reranker. Pure functions in
  `oml_app::search` do the reordering (`rerank_order`) and the floor (`apply_floor`), so the
  order is decided in the core and an adapter only scores. Cost: the head's rows are read
  before the cuts, up to `depth` rows where the plain pipeline reads `k`; they are reused for
  the hits, so a recall reads its rows once.

- **d2 — A `Reranker` port, and a cross-encoder adapter in candle.** `oml_app::ports::Reranker`
  has `model()` and `score(query, passages) -> relevances`, one per passage in order, each in
  `[0, 1]`, from the query and that passage alone. It is held as `Arc<dyn Reranker>`, as the
  request observer is, because a deployment may run without one and a sixth type parameter on
  `App` would touch every use case for an optional collaborator; the boxed future costs nothing
  beside the model. The adapter, `oml-rerank`, runs a BERT sequence classifier
  (`BertForSequenceClassification`: the encoder, its pooler and a one-logit head) through
  `candle-transformers`, and the relevance is the logistic of the logit, so one number reads the
  same way on every query (d6). The tokenizer is the model's own `tokenizer.json` through
  `tokenizers` with its pure-Rust regex backend (the default features would compile the C
  oniguruma and the C++ esaxx). The adapter builds each pair itself: the query is tokenised once
  and cut to a quarter of `search.rerank.max_tokens`, so a pasted stack trace never leaves a
  passage without room and never fails the rerank; each passage is cut to 4 KiB before the
  tokenizer reads it, so a megabyte memory costs what its start costs, then to the tokens the
  query leaves; pairs are padded by the adapter and batched sixteen at a time by token count, so
  the default head of ten is one forward pass. A failed rerank logs its error text, which holds
  no passage, as `rerank_failed`.

    **The runtime, by evidence** (same pairs, same host, same hour):

    | | build | per-call time, 20 short pairs | 10 short pairs | binary | CLI | licence and stack |
    |---|---|---:|---:|---|---|---|
    | candle 0.11, one thread | pure Rust, from crates.io | 869 to 1,000 ms | 304 to 528 ms | lead 28.4 MB (18.8 MB before) | unchanged | MIT/Apache-2.0 |
    | tract 0.23 (ONNX), as built | pure Rust, from crates.io | 769 ms; 20 pairs of 250 words failed to plan (`Reshape`) | 524 ms | 38.2 MB for the spike alone | not measured | MIT/Apache-2.0 |
    | onnxruntime 1.31 (Python, for scale) | C++ runtime | 432 ms (1 thread), 458 ms (4) | 281 ms, 226 ms | not built | not built | MIT, but C++ |

    candle is chosen. Its time sits within a factor of two of onnxruntime's on the same pairs,
    it builds from crates.io with nothing but Rust, and it loads the safetensors weights the
    model's repository publishes. `ort`, the Rust binding to onnxruntime, was not built: it has
    no stable release on crates.io (2.0 is a release candidate), it links the C++ runtime that
    ADR-0006 keeps out of the lead, and its prebuilt binaries are fetched at build time and
    exist for no musl target. tract matched candle's speed and failed to plan the model for
    long passages without more shape work. Measured: on a loaded host candle split over every
    core measured three times slower than on one thread (932 against 304 ms for ten pairs), so
    the reranker has a pool of its own, `search.rerank.threads` (default 2) threads that every
    rerank shares, and the lead sets candle's split (`RAYON_NUM_THREADS`) to the same number
    before it starts any thread, so the two agree and the stage never takes more of the host.
    Measured served over the synthetic set at one client (L6, depth 10; load average 6.5 to 8
    from other work):

    | threads | p50 ms | p95 ms |
    |---:|---:|---:|
    | 1 | 178 | 288 |
    | **2** | 133 | 224 |
    | 4 | 120 | 174 |

    Four threads buy 13 ms at the median for twice the cores; two leave an 8-vCPU host's other
    six to the request runtime and PostgreSQL.

    **The CLI does not grow.** Only `oml-server-lead` depends on `oml-rerank`; `cargo tree -p
    oml-cli` holds no candle, tokenizers or gemm crate, so the musl and Windows CLI builds are
    untouched. The lead's Linux binary grows by about 9.6 MB (18.8 MB on the benchmark build of
    2026-11-04, 28.4 MB on this branch's release build).

- **d3 — The model is pinned by digest and placed by the operator, or fetched once at start
  when allowed.** `oml-rerank` pins each model it serves by repository, commit and the SHA-256
  of `config.json`, `tokenizer.json` and `model.safetensors`. The lead reads them from
  `search.rerank.model_dir`, an absolute path (default `<state>/models/<model>`, `<state>`
  being systemd's `STATE_DIRECTORY`, else `$XDG_STATE_HOME/oml`, else `~/.local/state/oml`;
  never the working directory, and a relative value stops the start), checks every digest before parsing a
  byte, and refuses to start naming a missing or different file. With `search.rerank.fetch =
  true` it first fetches the missing files from the pinned commit over HTTPS, checks each digest
  before writing it, and writes it under a `.partial` name first, so a cut fetch leaves nothing
  a later start would take for the model. `fetch` defaults to `false`: a server reaches the
  network at start only when its operator says so. An offline install copies the three files
  into the directory, for example with `scripts/fetch_rerank_model.sh <dir>` on a staging host,
  which checks the same digests; CI caches them the same way. The model is not embedded in the
  binary: 91 MB of weights would ride every lead whether it reranks or not.

- **d4 — The model is `ms-marco-MiniLM-L6-v2`, and the depth is 10.** Measured offline over the
  served `bm25` lists of the synthetic set (121 queries, onnxruntime, the same first stage for
  every model), then served:

    | model (licence) | depth 10: nDCG@10 / S@1 / S@5 | depth 20 | depth 50 | model time p50 at depth 20 |
    |---|---|---|---|---:|
    | none (`bm25`) | 0.861 / 0.826 / 0.909 | | | |
    | `ms-marco-MiniLM-L4-v2` (Apache-2.0) | 0.891 / 0.868 / 0.967 | 0.893 / 0.868 / 0.959 | 0.889 / 0.868 / 0.959 | 56 ms |
    | `ms-marco-MiniLM-L6-v2` (Apache-2.0) | 0.902 / 0.893 / 0.959 | **0.904 / 0.893 / 0.959** | 0.900 / 0.893 / 0.959 | 86 ms |
    | `mxbai-rerank-xsmall-v1` (Apache-2.0) | 0.889 / 0.860 / 0.967 | 0.884 / 0.868 / 0.959 | 0.883 / 0.876 / 0.959 | 448 ms |
    | `bge-reranker-base` (MIT) | 0.898 / 0.876 / 0.967 | 0.899 / 0.884 / 0.959 | 0.892 / 0.884 / 0.959 | 771 ms |

    L6 ranks the first hit best of the four; the two larger models do not rank better on these
    queries and cost five to nine times L6's time, so they are rejected here. L4 is the cheaper
    choice a deployment may name (`search.rerank.model`). Depth: 50 ranks no better than 20 at
    more than twice the time, since a passage `bm25` ranks below 20 rarely answers. Served, the
    two depths and two models through `oml_read` over the synthetic set at `k` 100 (the bench
    harness; one client; no rerank skipped):

    | served | nDCG@10 | S@1 | S@5 | p50 ms | p95 ms |
    |---|---:|---:|---:|---:|---:|
    | `bm25` | 0.874 | 0.835 | 0.926 | 10.5 | 16.5 |
    | L6, depth 20 | 0.901 | 0.893 | 0.959 | 377 | 663 |
    | **L6, depth 10** | 0.902 | 0.893 | 0.950 | 275 | 371 |
    | L4, depth 20 | 0.891 | 0.868 | 0.959 | 273 | 484 |
    | L4, depth 10 | 0.893 | 0.876 | 0.959 | 176 | 229 |

    Depth 10 keeps L6's first hit and loses one query at success@5 for about half the p95, so 10
    is the default and 20 a setting; L4 at depth 10 is the fastest and 0.017 below L6 on the
    first hit. (These rows predate the pair handling of d2 and ran with candle on every core.)

    **The pair length is 256 tokens** (`search.rerank.max_tokens`, 64 to 512). Measured with L6
    at depth 10: the synthetic set ranks the same at 256 as at 512 (nDCG@10 0.902, S@1 0.893,
    S@5 0.950), since a claim is far shorter; on 100 SciFact abstracts 256 gives 0.694 nDCG@10
    and S@5 0.76 at a p50 of 1,447 ms, against 0.695 and 0.79 at 3,783 ms for 512 (load average
    3 to 8). Neither beats `bm25` on abstracts (0.696 on the same sample), and OML's memories are
    claims, so the shorter pair is the default.

    The recall eval, served through `oml_read` at the defaults (release build, PostgreSQL 18.6,
    `k` 10, 2,000 tokens, depth 10, 256 tokens, 2 threads, a 1 s deadline, no floor; load
    average 3.3 to 4.0; committed as `eval/recall/results/served_bm25_rerank.json`):

    | pipeline | success@1 | success@5 | recall@10 | nDCG@10 | median ms | p95 ms |
    |---|---:|---:|---:|---:|---:|---:|
    | `bm25`, same run | 0.826 | 0.934 | 0.910 | 0.872 | 6.1 | 10.0 |
    | `bm25-rerank` | **0.893** | **0.950** | 0.910 | **0.906** | 123 | 215 |

    On the public sets, served at `k` 100 with L6 at depth 20, each beside `bm25` on the same
    seeded sample in the same hour (`eval/bench/results/*/bm25-rerank-d20.json` and
    `bm25-sampled.json`):

    | set (queries) | `bm25` nDCG@10 / S@1 / S@5 | `bm25-rerank` | rerank p50 / p95 ms |
    |---|---|---|---:|
    | LoCoMo, categories 1-4 (302 of 1,535) | 0.534 / 0.401 / 0.662 | **0.635 / 0.553 / 0.762** | 2,522 / 3,367 |
    | SciFact (100 of 300) | 0.696 / 0.580 / 0.780 | 0.699 / 0.580 / 0.800 | 4,563 / 5,711 |
    | NFCorpus (100 of 323) | 0.320 / 0.480 / 0.630 | 0.330 / 0.470 / 0.630 | 5,247 / 15,467 |

    LoCoMo's short turns gain most: single-hop success@1 0.494 → 0.654, multi-hop 0.212 →
    0.394, temporal 0.469 → 0.563, open-domain 0.000 → 0.188 (16 queries). The abstracts gain
    little: SciFact +0.004 nDCG@10 and NFCorpus +0.010, below BEIR's published +0.023 and
    +0.025 for a larger cross-encoder, and a 512-token pair of an abstract costs ten times a
    claim's. OML's memories are claims, so the claim-shaped sets are the ones that decide.

- **d5 — A concurrency cap and a deadline, and a skipped rerank says so.** At most
  `search.rerank.max_concurrent` inferences run at once (default 2); a call waits for a slot,
  and the whole call, waiting included, has `search.rerank.timeout_ms` (default 1,000). A call
  still waiting at the deadline answers `busy`, one whose model is still running answers
  `timeout`, a tokenizer or tensor error answers `failed`; in each case the recall serves the
  first stage's order with `search.rerank: "skipped"` and `search.rerank_reason` naming which,
  never an error to the caller. A late inference keeps its slot until it ends, so the cap bounds
  the CPU in use, not only the calls answered. Measured over the synthetic set at the defaults
  (depth 10, 256 tokens, 2 threads, 2 slots, 1 s), one, four and eight clients at once; the
  load average from other work is beside each, and another session may have been running tests
  on the host at the time:

    | model | clients | p50 / p95 ms | reranks applied | load average |
    |---|---:|---|---:|---|
    | L6 | 1 | 140 / 246 | 121 of 121 | 3.1 → 3.0 |
    | L6 | 4 | 348 / 721 | 120 (1 timeout) | 3.0 → 3.1 |
    | L6 | 8 | 805 / 1,018 | 87 (32 timeout, 2 busy) | 3.1 → 3.4 |
    | L4 | 1 | 100 / 161 | 121 of 121 | 3.1 → 2.9 |
    | L4 | 4 | 225 / 462 | 121 of 121 | 2.9 → 3.1 |
    | L4 | 8 | 489 / 791 | 120 (1 timeout) | 3.1 → 3.3 |

    Before the pair handling and the pool of d2 (depth 20, candle on every core, load 9 to 15),
    four clients got 20 of 121 reranks and eight 12. The model is CPU-bound, so past the slots
    the wait grows with the clients; with the defaults no recall waits past the deadline, and
    the ones that would serve `bm25`'s order and say so, which is the behaviour this decision
    wants. The defaults are the measured trade on this host: two
    slots leave cores to PostgreSQL and the rest of the lead, and one second is past the p95 of
    a reranked recall over short claims at one client; abstract-length rows need a higher
    deadline or a lower depth, and doctor says when reranks are being skipped (d9).

- **d6 — The floor is a relevance, off by default; below it the answer says how many it
  withheld.** The threshold is a reranker relevance in `[0, 1]` (`search.rerank.floor`, or
  `off`). It applies only when the rerank was applied: the candidates of the first `k` scored at
  or above it stay, in order, and every other one, scored below it or never scored because it
  lay past the depth, is withheld; `search.floor` names the threshold and `search.below_floor`
  counts what it withheld. An answer whose candidates all fall below it has `hits: []`,
  `below_floor` above zero and the query's `terms`, which says "nothing relevant", not "no
  word matched", and offers no `did_you_mean`, since its words matched. Only rows the core
  admits are counted: the tail past the depth is hydrated and admitted before the floor counts
  it. Relevances and the floor print to six places, as set. When the rerank was skipped no
  floor applies and `search.floor` is `null`.
  *A normalised first-stage score as the fallback floor was measured and rejected:* `bm25`'s top
  score divided by the query's word count, swept on the recall eval, empties 64% of unanswerable
  queries only by emptying 13% of answerable ones, and 82% by emptying 28% (below). *Off by
  default, by the eval:* data version 2 adds 22 unanswerable queries (category
  `unanswerable`, no memory labelled, scored apart from the 121 answerable ones, whose metrics
  stay comparable with version 1). Swept over the served `bm25-rerank` run with no floor (`eval/bench/rerank_floor.py`):

    | floor | unanswerable answered with nothing | answerable answered with nothing | success@1 | success@5 |
    |---:|---:|---:|---:|---:|
    | off | 0 of 22 | 0 of 121 | 0.893 | 0.950 |
    | 0.001 | 7 (0.32) | 4 (0.03) | 0.884 | 0.942 |
    | 0.005 | 11 (0.50) | 8 (0.07) | 0.860 | 0.909 |
    | 0.02 | 13 (0.59) | 13 (0.11) | 0.835 | 0.868 |
    | 0.1 | 15 (0.68) | 17 (0.14) | 0.810 | 0.835 |
    | `bm25` score per word ≥ 1.0 | 14 (0.64) | 16 (0.13) | 0.736 | 0.818 |
    | `bm25` score per word ≥ 1.5 | 18 (0.82) | 34 (0.28) | 0.636 | 0.686 |

    Every floor that empties a third of the unanswerable queries also empties answerable ones
    and costs success@5; the unanswerable queries here are written in the domain's own words
    ("which Redis version runs the rate limiter", where the corpus names Redis but no version),
    and the model scores such near misses well above unrelated text. A recall that answers
    nothing for a question it could have answered costs an agent more than one weak hit it reads
    and discards, so the floor ships off, settable per deployment for those who want the
    trade. LoCoMo's adversarial questions (98 of the sample) presume something the conversation
    never says, yet their evidence turns exist; a floor of 0.1 empties 6% of them and also 6% of
    the multi-hop and 19% of the open-domain questions, and 0.001 empties 1% of them, so the
    floor does not tell them apart either.

- **d7 — The answer explains the rerank beside the first stage.** Each hit keeps `matched`
  (`terms`, `of`, `via`, from the first stage) and gains `rerank`, its relevance, and
  `rank_before`, its rank in the first stage's list, both `null` for a hit the reranker did not
  score. A reranked hit's `score` is its relevance, so the ordering contract, score descending,
  still reads true within the reranked head; the tail past the depth carries its first-stage
  score, and scores compare only among hits whose `rerank` is alike set or `null`. The read
  log's digest already holds the pipeline, so it changes with it.

- **d8 — Names, and what stops the start.** The pipeline is `bm25-rerank` (`bm25`, then the
  reranker); it is not in `search.enabled`'s default and needs a loaded model. Settings, each
  also `OML_SEARCH__RERANK__*`: `search.rerank.model` (`ms-marco-MiniLM-L6-v2`;
  `ms-marco-MiniLM-L4-v2`), `.model_dir`, `.fetch` (`false`), `.depth` (10, 1 to 100), `.floor`
  (`off`, or 0 to 1), `.max_concurrent` (2), `.threads` (2), `.max_tokens` (256, 64 to 512),
  `.timeout_ms` (1,000). The start stops naming the key on a model this build does not pin, a
  relative `model_dir`, a depth, floor, thread count or pair length out of range, or a fetch
  that is not `true` or `false`; on an enabled reranking pipeline with no model loaded
  (`search.enabled names bm25-rerank, which reranks, and no reranker is loaded`); and on a
  missing or different model file, naming it and the remedy. A deployment that serves no
  reranking pipeline reads no model file and fetches nothing.

- **d9 — Metric, request line, doctor, and the gate.** The metric `oml_reranks_total{outcome,
  reason}` (outcome `applied` or `skipped`, reason `none`, `busy`, `timeout` or `failed`) counts
  reranking stages; the request line gains `search_rerank` and `search_rerank_reason`.
  `/readyz`'s `search.rerank` names the model, the depth, the floor and the reranks skipped
  since start by reason; `oml doctor` prints `search: rerank <model>, depth <n>, floor <f|off>`
  and warns `search_rerank` when any were busy or late, naming the settings that change it, and
  `search_rerank_failed` when any failed, pointing at the `rerank_failed` log event. A score
  that is not a finite relevance in `[0, 1]` fails the rerank rather than sorting first. The
  recall eval gates `bm25-rerank` at success@1 ≥ 0.87 and success@5 ≥ 0.94, above `bm25`'s
  committed 0.835 and 0.934, and fails it when any query skipped the rerank, since a skipped
  rerank serves `bm25`'s order; CI caches the pinned files, runs `bm25-rerank` beside the other
  pipelines with a deadline of 60 seconds so a slow runner measures the ranking, and runs the
  adapter's real-model tests. **The default does not change.** `bm25-rerank` passes its
  quality gate, and misses ADR-0048 d11's latency budget by more than an order of magnitude on
  this host (a p95 of 215 ms against `bm25`'s 10 ms on the recall eval, where the budget is
  about twice the every-word recall's 19 ms); ADR-0049 d12's rule picks the default
  among passing pipelines on quality, and this record adds that a default must also fit the
  budget, as ADR-0049 d10 already required of the reranker. `bm25` stays the default; a
  deployment enables `bm25-rerank` and a request or a tenant names it.

- **d10 — Laws and tests.** `oml-port-contracts` holds the reranker laws, run against the test
  double (`FakeReranker`, the share of query words a passage holds) and, in `oml-rerank`'s
  integration test, the real model: `a_reranker_scores_each_passage_once_in_order`,
  `a_reranker_scores_lie_in_the_unit_interval`,
  `a_reranker_scores_a_passage_alone_as_among_others` (so no other row, readable or not,
  reaches a candidate's relevance), and `a_reranker_ranks_an_answer_above_an_unrelated_passage`.
  The real-model tests are `#[ignore]`d with the reason `needs the model's files in
  OML_RERANK_MODEL_DIR`, so a run without the model lists them as ignored, and CI runs them
  with `--ignored` against its cached files, candle, gemm and tokenizers built at opt-level 3
  in test builds; they include a megabyte passage inside a short deadline and a 2,000-word
  query that still scores every passage. One asserts candle's relevances equal the
  model's own ONNX export under onnxruntime to 1e-3 on fixed pairs. The core's stage is tested
  over the in-memory store answering the `bm25` kind with its any-word list
  (`MemStore::answering_bm25_with_any_word`, a first stage for the later stages, never a law of
  BM25): the head reordered, only admitted rows handed to the reranker, an own held row scored
  as a promoted one, `busy`, `timeout` and `failed` each serving the first stage's order with no
  floor, the floor withholding and counting, an explicit empty answer, the tail in first-stage
  order, and a pipeline that does not rerank never calling it.

## Consequences

- **Pages changed with this record**: [retrieval](../architecture/retrieval.md) (the stage and
  the pipeline), [HTTP API](../api/http-api.md), `openapi.yaml` and `mcp-tools.json` (the
  `bm25-rerank` name, `matched.rerank`, `rank_before`, `search.rerank`, `rerank_reason`,
  `floor`, `below_floor`), [configuration](../operations/configuration.md) (`search.rerank.*`),
  the recall eval's README (data version 2), the benchmark harness's README, and the benchmark
  pages.
- **Roadmap rows**: FR-88 (the reranker's latency on an idle host and under concurrent recalls,
  and the default's budget), FR-89 (a floor that empties unanswerable queries without emptying
  answerable ones: a calibrated or trained relevance, measured on eval data version 2 and
  LongMemEval's abstention questions). ADR-0048's FR-85 (a reranker's CPU cost in candle) is
  answered for this host by d2 and d4 and stays open until FR-88 closes.
- **Costs**: up to `depth` rows hydrated per reranked recall; the model's 91 MB in memory once
  per lead, and per running rerank the activations of one batch, at 256 tokens about 150 MB
  at worst (attention scores of 16 × 12 × 256 × 256 floats, a few alive at once), so about
  0.3 GB at the default two slots; the lead's binary 9.6 MB larger; CPU bounded by the slots
  and the pool's threads.
- **Expected conflicts**: ADR-0050 to ADR-0052 are on other branches and their numbers taken;
  FR-90 and above are used on other branches.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **An LLM reranker (listwise, RankGPT or RankZephyr-class)** | A 7B decoder per recall is seconds on 8 vCPU, and a hosted one puts a model provider in the read path, which the constraints exclude. |
| **ColBERT-style late interaction now (`answerai-colbert-small-v1`)** | It needs each memory's token vectors stored at write time, a second store of the text under row security, and a MaxSim kernel; it may be cheaper per recall than a cross-encoder, and it is the next measurement after FR-88, not this increment. |
| **A hosted rerank API** | A provider sees every query and every candidate's text, and the recall waits on a network the deployment does not control. |
| **`ort` (onnxruntime) as the runtime** | No stable crates.io release, a C++ runtime against ADR-0006, binaries fetched at build time, none for musl (d2). |
| **`bge-reranker-base` or `mxbai-rerank-xsmall-v1` as the model** | No better than L6 on the eval at five to nine times its time (d4). `jina-reranker-v2` is CC-BY-NC-4.0, so not for a commercially usable install. |
| **The model embedded in the binary** | 91 MB on every lead whether or not it reranks, and a model update would be a release (d3). |
| **A `rerank: true` request flag** | ADR-0049 chose closed pipeline names for the request log and the metrics; a flag would make every pipeline two, unmeasured. The pipeline is the opt-in. |
| **The floor on by default** | Measured: it empties answerable queries before it empties most unanswerable ones (d6). |
| **A floor on a normalised `bm25` score** | Measured worse than the reranker's at every threshold (d6). |

## Open questions

- Latency on an idle host, and the p95 at 4 and 8 concurrent recalls there, before any default
  changes (FR-88). The public-set rows ran with a cap of 8, so LoCoMo's shared the host with the
  NFCorpus run; their latency is an upper bound.
- Whether a smaller head (10) with L6, or L4 at 20, fits ADR-0048 d11's budget on abstract-length
  rows; this host could not say while loaded.
- Whether a floor can be made useful by calibration on more unanswerable data, or by scoring the
  floor against the query's own best `bm25` evidence (FR-89).
- Whether reranking `bm25-trigram`'s fused list helps misspellings, which a cross-encoder reads
  as other words.
