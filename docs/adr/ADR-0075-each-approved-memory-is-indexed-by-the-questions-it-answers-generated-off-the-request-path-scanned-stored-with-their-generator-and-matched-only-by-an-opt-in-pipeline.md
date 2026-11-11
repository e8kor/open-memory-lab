<!--
id: adr/ADR-0075-each-approved-memory-is-indexed-by-the-questions-it-answers-generated-off-the-request-path-scanned-stored-with-their-generator-and-matched-only-by-an-opt-in-pipeline
tier: engineering
status: proposed
answers: what hypothetical question indexing is for and what the recall eval says the gap is today; what the literature measured for doc2query, docTTTTTquery, HyDE, field-weighted BM25 and generated-question indexing since 2024; when questions are generated and for which versions never; the backfill policy and how it is stopped, capped and tracked; where questions are stored, with which generator, how every change to them is journalled, and what happens to them on supersession, retirement, erasure and a model change; how they are indexed and scored with the memory text, and why a row without questions is never pushed below its own text's score; the opt-in pipeline and what a hit says; where the model runs and when the server reaches the network; how a generated question is filtered for grounding and genericity and scanned, and what it may never do; the interface ADR-0072's phase 3 reads; the eval gates, their power, the latency and size budgets, the bill per 1,000 approvals and the backfill cost; how the drain behaves when the generator is down; when dense embeddings of questions are revisited; the phases and their tests; what the operator decided on 2026-11-10; the phase 0 gate as the operator redesigned it on 2026-11-11: whether generated questions beat or add to `bm25-sparse` on question-shaped queries, its power and its decision rule; what phase 0 measured and why the operator parked question indexing on 2026-11-11
-->

# ADR-0075 — Each approved memory is indexed by the questions it answers, generated off the request path, scanned, stored with their generator, and matched only by an opt-in pipeline

**Status:** proposed · 2026-11-10 · revised after review rounds 1, 2 and 3 the same day (see the Review log) · phase 0's gate amended by the operator's decision of 2026-11-11 (d12) · phase 0 run, no go, and question indexing parked by the operator on 2026-11-11 (d13)

**In short.** When a memory version is approved, a generator writes up to five questions that the
memory answers. The server keeps only questions grounded in the memory's own words, scans them,
stores them with the model and prompt that wrote them, and indexes them as a second field of the
version. A new pipeline, `bm25-questions`, scores the memory text and its questions together by
field-weighted BM25 (BM25F), and each hit names the question it matched. Nothing unreviewed is ever
expanded. A question changes ranking, never visibility, is never served as memory text, and can
only add to a row's score: a row without questions scores exactly what `bm25-postings` gives it,
with the same words pruned, whatever other rows' questions say. A word counts once per question
set, however many questions repeat it. Recall makes no model call. Phase 0 is an offline run, on a labelled set grown large enough to see the
gain, that decides whether the server work is worth building. **It ran on 2026-11-11 and did not
go:** generated questions neither beat `bm25-sparse` nor added to it, so question indexing is
parked and v0.7.1 recommends `bm25-sparse` (d13).

**Decisions at a glance.**

| | Decision | Phase |
|---|---|---|
| d1 | Questions are generated when a version becomes served, by a queue the lead drains outside any transaction; never for a held, unreviewed or in-progress-only version; backfill only by an admin's act | 1 |
| d2 | One current question set per version, stored with its generator; every set written, replaced or dropped is journalled and bumps a tenant sequence; a replaced set's text stays until erase | 1 |
| d3 | Questions are postings of kind 3 (`KIND_QUESTIONS`), one field per version, a word counted once per set, with their own counts and a count of the rows where a word is new; never ADR-0072's memory kind 3 | 1 |
| d4 | `bm25-questions` scores text and questions as two fields of one BM25F document; the text keeps `bm25-postings`' statistics and pruning exactly; opt-in; a row without questions scores what `bm25-postings` gives it; a hit names its question | 1 and 2, one release |
| d5 | A `QuestionGenerator` port with one adapter in phase 1, the local one unless phase 0 says otherwise; off by default; no egress | 1 |
| d6 | A question must be grounded in its memory, specific, distinct from its set and clean: grounding, genericity, near-duplicates, caps and the scan; any finding drops it; a version with a finding whose class holds by default gets none, whatever a tenant ceiling says | 1 |
| d7 | ADR-0072 d10 reads per-workarea kind 3 counts, through both of its reads, and a coverage share net of expired versions, nothing else | 3 |
| d8 | One superiority gate with 90% power shown, two arms in a fixed sequence, non-inferiority arms sized to 99% each, a displacement bound at partial coverage, the joint power stated; an adversarial suite per adapter, recall and write latency under load, size, and the bill | 0 to 2 |
| d9 | Dense embeddings of questions are deferred until a named trigger fires | — |
| d10 | Phase 0 (with 0a, the grown labelled set) decides; phases 1 and 2 ship in one release; phase 3 when #207 is scheduled | 0 to 3 |
| d11 | Operations: an outage, told from a bad version by a compiled-in canary, pauses the drain and it resumes on its own; parked entries come back with backoff; a backfill is estimated, capped, stoppable and tracked; the bill is metered; doctor names each state | 1 |
| d12 | Amended 2026-11-11: phase 0 asks whether questions beat `bm25-sparse` (primary) or add to it (secondary) on the 687 question-shaped queries, each by an exact paired test at 0.025 sized for 80% power at a 2.5-point gain; non-inferiority on the other 99 with a computed margin; a go ships the pipeline that passed, opt-in, and otherwise v0.7.1 recommends `bm25-sparse` and questions are parked | 0 |
| d13 | Decided 2026-11-11: phase 0 ran and did not go (questions 0.926 against `bm25-sparse`'s 0.958 success@5 on 687 question-shaped queries, 2 wins and 24 losses; added to it, +0.1 points, 95% interval −0.4 to +0.7); question indexing is parked, phases 1 to 3 are not built, `bm25-sparse` is v0.7.1's recommended pipeline, and ADR-0072's phase 3 drops generated questions as a signal (ADR-0072 carries the amendment note) | — |

**Open questions: decided by the operator, 2026-11-10.** Each is detailed at the end.

| | Question | Decided by the operator, 2026-11-10 |
|---|---|---|
| 1 | Which generator ships first? | A local generator first: `local` (an in-process T5), behind the one adapter phase 1 builds; the endpoint only if it then beats `local` by the same paired test. No non-loopback endpoint on memory.doublegate.io. |
| 2 | Exclude approved memories that carry a hold finding? | Yes: no questions are generated for them, with the count visible in `status` and doctor. |
| 3 | Run phase 0 now? | Run phase 0a before building: phase 0a first (grow the labelled set to at least 550 queries), then phase 0; phases 1 and 2 stay behind its verdict. |
| 4 | Correct #201's 0.17? | Already corrected: done in #201's body on 2026-11-10, with today's baseline and its source. |
| 5 | Phase 0's gate, after 0a's joint power came out at −0.08 (decided 2026-11-11) | Redesign it as d12: questions must beat or add to `bm25-sparse` on question-shaped queries, with the power computed on measured discordance; if they do not clearly win, v0.7.1 recommends `bm25-sparse` instead. |
| 6 | Phase 0's outcome (decided 2026-11-11) | "Park questions, promote sparse": question indexing is parked with phase 0's evidence (d13), and v0.7.1 documents and recommends `bm25-sparse`. |

**Amends:** ADR-0049 (d4: the pipeline table gains `bm25-questions`; d6: `matched` gains
`question` and `question_terms`, and `via` gains `questions`), ADR-0052 (d1 and d3: `kind` gains `3`,
with a weight of 1 per lexeme and a `novel` flag; d5: `search_term_rows` gains `novel_rows` for kind
3; d7: postings of kind 3 follow the version and leave with an erase or a regeneration; d11: under
`bm25-questions` the answer's `search` gains `pruned_questions`), ADR-0005 (d4: the erasure inventory gains the question tables and kind 3 postings),
ADR-0065 (d13, as ADR-0074 d14 amends it: the recorded rescan walks stored questions), ADR-0054
(d2: `recall_events` gains `questions_seq`), and ADR-0072 (phase 2's test 13 and its review log now
read "memory-kind-3 postings", a one-line clarification made in this pull request; posting kind 3
is this record's question field; and d9's list of what changes an answer in phase 3 now names a
question set written, regenerated or dropped, as d4 here does)

**Supplies:** ADR-0072 d10 (the generated-questions selection list of its phase 3, #207)

## Context

**What #201 asks.** Recall finds a memory when the question and the memory share intent but few
words. Each approved memory stores a small set of questions it answers. Recall matches the query
against them as well as the text, and a hit says which question it matched. The design constraints
in #201 are taken as given: approved memories only; questions are index data, not claims; stored,
not regenerated; explainable; an opt-in pipeline; lexical first.

**What the eval says the gap is, corrected.** #201 cited 0.17 success@5 for the default. That figure
is `keyword-all`, v0.5.0's every-word recall (ADR-0049 d9's table: 0.17 success@5, 0.83 zero
results); #201's body was corrected on 2026-11-10. Since 2026-11-05 the default is `bm25-postings`
(ADR-0052 d9), and on the synthetic set (121 queries,
[recall-synthetic](../benchmarks/recall-synthetic.md)) it measured success@5 0.934 and nDCG@10
0.873: 8 of 121 queries miss the top 5. The misses are paraphrase 3, temporal 2, typo 2 and
multihop 1 (the category table of that page). **Paraphrase**, the vocabulary gap #201 describes,
measured nDCG@10 0.50 and success@5 0.73 (8 of 11), the lowest of the eleven categories. Generated
questions do not carry a user's typo and rarely carry a date, so the case rests on paraphrased and
question-shaped queries, and on the external sets where the default trails: SciFact nDCG@10 0.685,
NFCorpus 0.321, LoCoMo 0.503 ([benchmarks](../benchmarks/index.md)). The 121-query set is too small
to show that case (d8), which is why phase 0a grows it.

**Document expansion already exists here, and it is the bar.** Kind 2 postings are a learned sparse
encoder's expansion of each version (`doc-v3-distill`, ADR-0052 d14 to d17). `bm25-sparse` measured
success@5 0.983 on the synthetic set and nDCG@10 0.730, 0.349 and 0.550 on SciFact, NFCorpus and
LoCoMo, above the default on every set. It stays opt-in for one reason: latency. Its two lists took
272 ms p95 at 100,000 memories, universe scope, against 29.2 ms for `bm25_postings`, because kind 2
keeps no counts to prune by and its expansions make a common token's postings many (ADR-0052 d16,
#104, open). Generated questions are a second kind of expansion competing with a fix to the first.
They are lexical, so they keep d5's counts and d6's pruning, and they can be read and named in an
answer. This record therefore has to show a gain over the default at a latency `bm25-sparse` does
not reach, and phase 0 compares it with a capped `bm25-sparse` so that the verdict weighs the two
investments (d8, rejected alternatives).

**What the literature measured.** Fetched and quoted from the primary sources, 2026-11-10:

- Nogueira, Yang, Lin and Cho, *Document Expansion by Query Prediction*, arXiv 1904.08375, 2019
  (doc2query), <https://arxiv.org/abs/1904.08375>: "we propose a simple method that predicts which
  queries will be issued for a given document"; "From the perspective of a question answering
  system, this might comprise questions the document can potentially answer"; and "In a
  latency-critical regime, retrieval results alone (without re-ranking) approach the effectiveness
  of more computationally expensive neural re-rankers but are much faster."
- Nogueira and Lin, *From doc2query to docTTTTTquery*, 2019,
  <https://cs.uwaterloo.ca/~jimmylin/publications/Nogueira_Lin_2019_docTTTTTquery-v2.pdf>: "These
  predicted questions are then appended to the original documents, which are then indexed as
  before." "We find that the top-k sampling decoder produces more effective queries than beam
  search." On MS MARCO passages, MRR@10 (dev) was 0.184 for BM25, 0.259 with 5 sampled queries, 0.272
  with 20 and 0.278 with 80, at 55 to 66 ms a query. "Since neural inference is applied prior to
  indexing, the increase in query latency is attributable solely to longer documents." Expansion
  cost: "sampling 5 queries per document for 8.8M documents requires approximately 40 hours on a
  single TPU". "copying terms has the effect of term re-weighting while expanding with new terms
  mitigates the vocabulary mismatch problem", and "doc2query and docTTTTTquery produce similar
  proportions of copied (67%) and new words (33%) with respect to the original document". A third
  of a question's words are new by design: that is both the gain and the attack surface (d6).
- Thakur et al., *BEIR*, arXiv 2104.08663, Table 2 (nDCG@10),
  <https://arxiv.org/pdf/2104.08663v4>: BM25 against docT5query, SciFact 0.665 → 0.675, NFCorpus
  0.325 → 0.328; on average "+ 1.6%" over BM25, outperforming it "on 11/18 datasets". So expansion
  by generated queries helps out of domain, by a little.
- Gao, Ma, Lin and Callan, *Precise Zero-Shot Dense Retrieval without Relevance Labels*, arXiv
  2212.10496 (HyDE), <https://arxiv.org/abs/2212.10496>: "Given a query, HyDE first zero-shot
  instructs an instruction-following language model" to write a document that "is unreal and may
  contain false details". HyDE is the mirror image of this record: a model call per recall. #201
  and ADR-0072's rejected alternatives (d4) already rule a recall-time model out.
- Gospodinov, MacAvaney and Macdonald, *Doc2Query--: When Less is More*, ECIR 2023,
  <https://arxiv.org/abs/2301.03266>: "Doc2Query is indeed prone to hallucination"; "using a
  relevance model to remove poor-quality queries can improve the retrieval effectiveness of
  Doc2Query by up to 16%", "while simultaneously reducing mean query execution time by 23% and
  cutting the index size by 33%". d6's grounding filter is a lexical stand-in for that relevance
  model.
- Basnet, Gou, Mallia and Suel, *DeeperImpact: Optimizing Sparse Learned Index Structures*, arXiv
  2405.17093, <https://arxiv.org/abs/2405.17093>: "the recently proposed Doc2Query -- query
  filtration does not enhance retrieval quality when used with DeepImpact". Mansour, Zhuang, Zuccon
  and Mackenzie, *Revisiting Document Expansion and Filtering for Effective First-Stage Retrieval*,
  SIGIR 2024, report the same for learned term weights; its arXiv page could not be fetched and it
  is read from a listing (<https://chatpaper.com/chatpaper/paper/65327>), so no figure here rests on
  it. OML's default weights terms by BM25, not by a learned model, so this caveat bounds rather than
  rules out d6's filter, and phase 0 reports what the filter drops and what that costs.
- Weller et al., *When do Generative Query and Document Expansions Fail?*, Findings of EACL 2024,
  <https://aclanthology.org/2024.findings-eacl.134/>: "there exists a strong negative correlation
  between retriever performance and gains from expansion"; "expansion improves scores for weaker
  models, but generally harms stronger models", since expansions "add additional noise that makes it
  difficult to discern between the top relevant documents". This is the warning d8's no-harm gates
  are built around, and the reason questions get a field of their own with a tuned weight (d3, d4).
- Kuo, Chiu, Ma and Cheng, *Doc2Query++*, arXiv 2510.09557, 2025,
  <https://arxiv.org/abs/2510.09557>: names "noise from concatenation harming dense retrieval" as a
  limit of expansion, and proposes a "Dual-Index Fusion strategy" that "isolates text and query
  signals". Its setting is dense; d3's separate field is the lexical form of the same separation.
- Lu, Robertson and Macfarlane, *Field-Weighted XML Retrieval Based on BM25*, INEX 2005, LNCS 3977,
  <https://www.staff.city.ac.uk/~sbrp622/papers/city_INEX05.pdf>, describing Robertson, Zaragoza
  and Taylor, *Simple BM25 extension to multiple weighted fields*, CIKM 2004: "many systems use a
  linear combination of the scores obtained from scoring every field"; "Robertson et al [11]
  discuss the dangers of linear combination in detail and propose an alternative solution, the
  linear combination of term frequencies based on BM25", that is, weight each field's term
  frequency, sum, and saturate once with one IDF. d4 adopts this (BM25F), with each field
  normalised by its own length.
- Neeser, Latimer, Khatri, Latimer and Ramakrishnan, *QuOTE: Question-Oriented Text Embeddings*,
  arXiv 2502.10976, 2025, <https://arxiv.org/abs/2502.10976>: QuOTE "augments chunks with
  hypothetical questions that the chunk can potentially answer". Its tables report top-1 context
  accuracy on Natural Questions of 32.92% for plain chunks, 33.23% for HyDE and 38.00% for QuOTE, and
  query processing on SQuAD of about 100 s, 1,176 s and 131 s. Gains level off at 10 to 15 questions
  per chunk, and smaller local generators trail gpt-4o by a few points (read from
  <https://ar5iv.labs.arxiv.org/html/2502.10976>). Its two tables give different SQuAD baselines, so
  only the Natural Questions figures are cited here. QuOTE embeds questions densely; this record
  indexes them lexically first (d3, d9).

In OML's terms: generation at write time, after review, is docTTTTTquery with a second identity's
verdict before it; a question field kept apart from the text and scored with it by BM25F is
docTTTTTquery's append with a weight and an off switch; and the eval has to show that OML's
default, already a strong lexical retriever, is not the "stronger model" Weller et al. found
expansion harms.

**The forces.**

- **R2 and R3.** Nothing unreviewed is expanded, and the gate changes visibility, not ranking
  ([ADR-0001](ADR-0001-the-constitution.md)). Questions are written by a model nobody reviews, so
  they may only rank rows a key could already read, only rows that review made served, and only
  upward: a scan finding that removes a question may take away what the question added, never
  what the memory's own text earns, and no row's questions, its own or another's, change what any
  row's text earns (d4's laws).
- **Repeatability** (#176). The same key, query, approved set and time give the same answer.
  Recall stays model-free, so the generator's output must be stored and named, never recomputed,
  and every change to the stored sets must be visible to replay (d2).
- **ADR-0074.** A generator reading an approved memory can be steered by text inside it, and its
  output reaches a model when a hit names its question. Generated text is a write path the scan has
  to cover, and the reviewer never saw it.
- **Occam** (ADR-0039 d8). No new tool. A pipeline name and one hit field; one adapter.
- **Row security.** Only leakproof integer equalities reach an index (ADR-0052 d1). Questions
  therefore become integer postings under the same policy, not a text index.

Decided, not built.

## Decision

- **d1 — Questions are generated when a version becomes served, from a queue the lead drains off
  the request path and outside any transaction; never for a held, unreviewed or in-progress-only
  version; and a backfill runs only when an admin starts it.**
  - **The trigger.** The `search_postings` trigger (ADR-0052 d8, d15) enqueues a version into
    `search_questions_queue`, at `priority` 0, in the transaction that makes it served: a
    promotion, or a supersession's successor. It enqueues only a hit kind (`memory`,
    `derived_fact`, as postings do), only for a tenant whose `search_questions_state.enabled` is on,
    and only when the version has no current set under the tenant's `current_generator_id` (d2).
    Nothing is enqueued at write, for a held version, for a rejected one, or for a version served
    only to its author inside an in-progress subtree (ADR-0030 d2): those are unreviewed, and #201
    says nothing unreviewed is expanded. Generation follows the served state alone, so a version
    approved `same_user` where ADR-0074 d26 lets that count is generated for like any other served
    version. A version that leaves served before it is drained leaves the queue in the same
    transaction.
  - **The drain** reuses the sparse queue's shape (ADR-0052 d15) with one change of order, so that
    no lock or transaction spans a model call. Each worker of d1's pool takes **one** due entry
    `FOR UPDATE SKIP LOCKED`, sets `leased_by` to its lead and `leased_until` to 5 minutes ahead,
    and commits; one entry's work is a timeout of at most 60 s and one write, so the lease cannot
    expire under a batch. It then reads the version's search text with no lock (d5's input), calls
    the generator outside any transaction under `search.questions.timeout` (default 60 s), and
    filters and scans the output (d6). Only then does it open the write transaction. It first
    deletes the queue entry `WHERE leased_by` is this lead and `enqueued_at` is the one it read, and
    requires exactly one row deleted; it takes the version `FOR SHARE` and checks that it is still
    served; and only then writes the set, its questions, their kind 3 postings and counts, the
    journal event and the sequence bump (d2). If any check fails, it rolls back and discards the
    output: an entry another lead took after the lease lapsed is written by one of them only, never
    twice, so no set, event, `questions_seq` bump or token count is doubled. A supersession,
    retirement or erase therefore never waits on a model.
  - **Failures** are of two kinds (d11), and the canary tells them apart. A generator that cannot
    be reached pauses the lead's drain and counts against no entry. When a call fails or times out,
    the worker runs d11's compiled-in canary input before it classes the failure: if the canary
    fails too, the generator is down; if the canary succeeds, the failure is the version's. A
    version's failure (unparseable output, or a timeout the canary does not share) keeps the entry,
    names `last_error` and retries after `2^attempts` seconds, at most an hour; after 5 such
    failures the entry is parked, and d11 brings it back. So a version that always times out is
    parked after five attempts and never stalls the drain, even with `threads` 1. A version not yet
    drained has no question field; `bm25-questions` scores it by its text alone (d4).
  - **Order and fairness.** `priority` 0 (new approvals) is taken before `priority` 1 (backfill),
    oldest first per tenant, tenants in turn, at most 16 entries per tenant per pass as ADR-0052
    d15 orders them, each still taken and leased one at a time. So one tenant's backfill never
    delays another tenant's new approvals.
  - **Where the model runs.** The generator runs on its own bounded pool of
    `search.questions.threads` (default 1), apart from the request runtime, as the reranker's pool
    is bounded (ADR-0053). d8's latency gate is measured with the drain saturated.
  - **The write path is shared.** A set write appends a journal event, so it takes the tenant's
    chain head, which every commit in the tenant serialises on (ADR-0002, #100), and it updates the
    kind 3 rows of `search_term_rows` and `search_workarea_rows`. Those rows are keyed by `kind`
    (ADR-0052 d5), so a set write never contends with a promotion's kind 1 count rows, but a
    backfill at 60 sets a minute adds 60 short transactions a minute to the chain head for as long
    as it runs (about 28 hours at 100,000). The write transaction does the deletes, inserts and
    count updates first and appends the journal event last, so it holds the chain head only for the
    append. d8 gates promotion, supersession and erase latency with the drain saturated on the same
    tenant; if that gate misses, phase 1 writes up to 16 sets of one tenant per transaction, each
    with its own event and its own `questions_seq` step, so replay still sees every set change
    apart, and the chain head is taken once per 16.
  - **Backfill** is an admin's act, never the lead's own: `oml admin questions backfill
    [--tenant t] [--stale] [--limit n] [--dry-run]`, with `--stop` and `--resume` (d11). It is
    journalled as `search.questions_backfill_started` with the generator and the estimate. It
    enqueues, at `priority` 1, the served versions with no current set (or, under `--stale`, a set
    from another generator) in resumable steps of 1,000, as `SPARSE_BACKFILL_STEP` does.
    `search.questions.rate` (default 60 a minute per lead, chosen, not measured; phase 0 measures
    it, #236) throttles the **drain's** takes of `priority` 1 entries, not the enqueue, so a large
    backfill sits in the queue and new approvals pass it; `threads` bounds the CPU the drain uses at
    any rate. Turning a tenant on (`oml admin questions enable`) does not backfill by
    itself; its answer gives the count of served versions with no set, d11's estimate, and the
    command. A lead never regenerates on start or on a model change (d5).
  - The cost: questions arrive after the approval. Between the two a new memory is found by its
    text alone, and an answer can change when its questions land. d2 journals that change and #176's
    page lists it (d4).

- **d2 — One current question set per version, stored with the generator that wrote it; every set
  written, replaced or dropped is journalled and bumps a tenant sequence; the set follows its
  version's state, and its postings leave with an erase or a regeneration.** The migration is
  `search_questions`, numbered by the phase 1 branch when it is built (0040 is the next free number
  today; 0036 to 0039 are reserved). It creates, each tenant-partitioned, `FORCE ROW LEVEL SECURITY`
  under the `tenant_isolation` policy, with partitions made in ADR-0034 d11's owner step beside
  `search_postings`, except the first:

    | Table | Holds |
    |---|---|
    | `search_question_generators` | one row per generator: `id`, `adapter`, `model`, `model_digest`, `weights_licence`, `data_licence` (d5's register), `prompt_version` (`oml-questions/1`), `prompt_sha256`, `max_questions`, the decoding parameters, `created_at`. No tenant data, so it is the one table here that is not partitioned. |
    | `search_question_sets` | `(tenant_id, memory_id, set_seq)` key: `workarea_id`, `generator_id`, `generated_at`, `replaced_at` (null for the current set; one current set per version by a partial unique index), `kept` (0 to 5), `dropped` (a list of `{reason, class, rule}`, never the dropped text), `outcome` (`written`, `empty`, `failed`), `tokens_in`, `tokens_out` |
    | `search_questions` | `(tenant_id, memory_id, set_seq, ordinal)` key: `text`, at most 200 characters, and `dropped_at` for a question a rescan or an admin dropped |
    | `search_questions_queue` | as `search_sparse_queue`: `enqueued_at`, `priority` (0 new, 1 backfill), `attempts`, `next_at`, `last_error`, `parked_at`, `park_count`, `leased_by`, `leased_until`, and `tokens_in` and `tokens_out` summed over the entry's failed attempts |
    | `search_questions_state` | per tenant: `enabled`, `current_generator_id`, `questions_seq` (a monotonic counter), `backfill_after`, `backfill_total`, `backfill_limit`, `backfill_taken`, `backfill_stopped_at`, `backfilled_at`, and `tokens_in_total` and `tokens_out_total`, every token the tenant's generation spent, kept or not (d11) |
    | `search_question_coverage` | `(tenant_id, workarea_id, generator_id)` key: `rows`, the served versions in the workarea whose current set came from that generator, kept by the trigger (d7). It is not net of expired versions; d7.2 says how it is read |

  - **A set belongs to one version**, not to a claim. A supersession serves a new version, which
    gets its own set; the replaced version's kind 3 postings go to state 2 with its kind 1
    postings, and its question rows stay. Retirement and rejection do the same. So questions are
    superseded with their memory, as #201 asks, and a replaced version's questions stay until an
    erase, because they are derived from bytes R4 keeps, as ADR-0052 d7 keeps a replaced version's
    postings.
  - **Every change is journalled and sequenced.** Each set the drain writes, each regeneration,
    each question or set a rescan drops (d6), and each admin drop writes one journal event,
    `search.questions_set` (`memory_id`, `set_seq`, `generator_id`, `outcome`, `kept`, the drop
    reasons, never question text), and increments the tenant's `questions_seq` in the same
    transaction. That is one event per served version, as its approval already writes one.
    `recall_events` records the `questions_seq` a recall read (d4), so replay can say exactly which
    set changes lie between a recall and its replay.
  - **A regeneration** (d1's `--stale`) marks the old set `replaced_at`, deletes its kind 3 postings
    and counts, and writes the new set and its postings in one transaction. The old set's question
    text stays until an erase: it is about 0.05 GiB per generation at 100,000 versions (d8), and it
    lets a replay explain an answer that a regeneration changed.
  - **The current generator is stored**, not read from configuration: `current_generator_id` is
    set by `oml admin questions enable` and changed only by `oml admin questions adopt`, journalled
    as `search.questions_generator_adopted`. A lead drains a tenant only when its configured
    `GeneratorId` equals the tenant's; otherwise it refuses that tenant and doctor's
    `search_questions_generator_mismatch` names both ids. Two leads configured differently, for
    example during a rolling upgrade, therefore never write sets from two generators.
  - **An erase** deletes the version's sets (current and replaced), questions, kind 3 postings and
    counts, and its coverage contribution, in the erase event's transaction; ADR-0005 d4's inventory
    gains these rows beside "tsvector entries", with the same disposition. The journal events keep
    no text, so nothing of the version survives in them.
  - The cost: one row per question and one per set per generation, and one journal event per
    served version, about 0.05 GiB of text at 100,000 versions per generation (d8).

- **d3 — Questions are postings of kind 3, one field per version, with their own counts.** The
  posting kind is named `KIND_QUESTIONS` (3) everywhere this record and ADR-0072 cite it. It is
  never ADR-0072's "memory kind 3": that is `memory_kind` 3, a descriptor, written under posting
  kind 1. Descriptors get no questions (they are not a hit kind).
  - **The field.** A version's kept questions are tokenised as kind 1 is (ADR-0049 d12's `simple`
    and `english` lexemes, `oml_token_id`, ADR-0052 d2) and written as one field per version: one
    `search_postings` row per distinct lexeme across the set, `weight` 1 however many questions hold
    it, `doc_len` the number of distinct lexemes, `memory_kind` the version's, `state` the
    version's, read `FOR SHARE` in d1's write transaction, and `novel`, true when no kind 1 posting
    of the same version has that token. A word therefore counts once per set: a generator that
    repeats a word across five questions gives it no more weight than one question does (d4's law
    3). Question words such as "how" and "what" are English stop words and are dropped first.
  - **Counts.** The trigger keeps `search_term_rows` and `search_workarea_rows` for kind 3 as it
    keeps them for kind 1 (ADR-0052 d5), so kind 3 has its own document frequencies `df_3` and its
    own mean field length, summed over the readable nodes at query time, less expired versions as
    kind 1's are; d7 reads these. `search_term_rows` also gains `novel_rows` for kind 3: the served
    versions whose question field holds the token and whose text does not, kept on the same state
    flips from the postings' `novel` flag and less expired versions the same way. `df_1(t) +
    novel_rows(t)` is then exactly the number of rows holding t in either field, with no row
    counted twice (d4).
  - **One field per version, not one document per question.** A version with five questions would
    otherwise get five chances to match, and the set's length would not normalise.
  - **A field of its own, not docTTTTTquery's append.** Appending would change kind 1's counts and
    the default's scores for every tenant, could not be weighted down when it hurts (Weller et al.),
    and could not say which words came from a question. A field keeps `bm25-postings` byte for
    byte and gives d4 a weight to tune.
  The cost: a third copy of a version's words in postings, budgeted in d8.

- **d4 — `bm25-questions` scores a version's text and its questions as two fields of one BM25F
  document; it is opt-in, it needs no model at recall, a row without questions scores its text
  alone, and each hit names the question it matched.**
  - **Why not fuse two lists.** The first draft fused `bm25_postings` and a `questions` list by
    ADR-0052 d13's convex combination. That normalises each list by min-max over its own candidates,
    so a weak match on one generic question word becomes the questions list's maximum and earns the
    full weight whatever its IDF, and a row with no questions scores at most `1 − w_q`. The first
    breaks the over-matching bound; the second demotes every row a backlog or a scan finding left
    without questions. BM25F fixes both, which is the reason Robertson et al. give for combining
    term frequencies rather than scores.
  - **The score.** For a query, over the readable, served, unexpired rows of the scope (ADR-0052 d5
    and `POSTINGS_EXPIRED`, as `bm25-postings` reads them), let N be the kind 1 row count, `df_1(t)`
    the kind 1 document frequency of a kept query word t, `novel_rows(t)` d3's count of rows whose
    questions hold t and whose text does not, and `avg_1` the mean text length. The text keeps
    `bm25-postings`' statistics exactly, and the questions get a union statistic of their own:
    - idf_1(t) = ln(1 + (N − df_1(t) + 0.5) / (df_1(t) + 0.5)), the form `bm25` already computes in
      `oml-pg`, the maximum over a word's forms as there: `bm25-postings`' own IDF;
    - n_U(t) = min(N, df_1(t) + novel_rows(t)), exactly the rows holding t in either field, each
      counted once, and idf_q(t) the same form over n_U(t), so idf_q(t) ≤ idf_1(t);
    - y_t(d) = tf_1(t,d)/B_1(d), with B_1(d) = 1 − b_1 + b_1 · len_1(d)/avg_1, the text term
      `bm25-postings` saturates;
    - q_t(d) = 1 if d's current set holds t and 0 otherwise (d3's binary weight), and
      x_t(d) = y_t(d) + w_q · q_t(d);
    - T(d) = Σ_t idf_1(t) · g(y_t(d)), the text score, and
      S(d) = T(d) + Σ_t idf_q(t) · [g(x_t(d)) − g(y_t(d))], with g(x) = x(k1 + 1)/(k1 + x).

    This is BM25F as Robertson et al. combine fields, term frequencies summed before one
    saturation, with one change: the increment the question field adds is weighted by the union
    IDF, and the text term keeps the text's. When no question holds a word outside its own row's
    text, n_U = df_1 and S is plain two-field BM25F. k1 is 1.2 and b_1 0.75, as `bm25` today. The
    question field has no length normalisation: b_3 is fixed at 0, because its term frequency is
    binary and the field is capped at five questions of at most eight content lexemes (d6), and a
    normalisation would let a short, dense set raise its own words. Only w_q is tuned, in phase 0,
    by five-fold cross-validation on the grown labelled set (d8), over a grid inside (0, 1).
    Here t ranges over the kept query words, as in `bm25-postings`: y_t takes the word's text form
    as `bm25-postings` scores it, q_t is 1 when the set holds either form of the word, and
    `novel_rows`, n_U and idf_q are computed per form, idf_q being the maximum over forms as idf_1
    is. So idf_q ≤ idf_1 holds form by form, and a word counts once per set, not once per form.
    Under federation the scope's rows are ADR-0072's R, and `novel_rows` and the kind 3 counts are
    summed over R as kind 1's are, so no hit's score depends on which sources were selected
    (ADR-0072 d4, its test 6).
  - **Pruning and candidates.** The text field is pruned exactly as `bm25-postings` prunes it:
    `df_1` against `FREQUENT_SHARE` of N, then `POSTING_BUDGET` over the kind 1 forms (ADR-0052
    d6); nothing of kind 3 enters either decision. The question field is pruned on its own: a word whose n_U(t) exceeds `FREQUENT_SHARE`
    of N adds nothing through questions, and kind 3 postings are read under a `POSTING_BUDGET` of
    their own, the kept word with the most kind 3 postings pruned first. Since n_U ≥ df_1, a word
    pruned from the text by frequency is pruned from the questions too. A question-field prune is
    not completed in step 5: that word's question increment is 0, which law 2 allows. The answer's
    `search.pruned` is `bm25-postings`' list, unchanged, and `search.pruned_questions` lists the
    words pruned from the question field alone, so an agent can see why the two pipelines disagree.
  - **Completion, the scan path and `match: "all"`.** ADR-0052 d4's step 5 adds the pruned forms'
    contributions for the best 4k candidates by partial score. Here it completes the union of two
    windows: `bm25-postings`' own, the best 4k kind 1 candidates by partial T, and the best 4k by
    partial S. A window chosen by S alone would let covered rows crowd an uncovered row out: with k
    1, four covered rows matching on their questions fill the window, and an uncovered row whose
    pruned words would have ranked it first under `bm25-postings` is neither completed nor returned
    (round 3). With the union, every row `bm25-postings` completes is completed here with its full
    T, for at most 4k more `(memory_id)` reads. When every kind 1 form is pruned, `bm25-postings`
    answers by `scan` (ADR-0052 d6); `bm25-questions` then takes T from the same scan, adds the
    question increments of the scan's candidates from their kind 3 rows by `(memory_id)`, and says
    `search.access` `scan`. Under `match: "all"` a row must hold every query word in its text,
    decided as `bm25-postings` decides it, so "every answer holds every word" stays a statement
    about reviewed text; questions only reorder those rows.
    The candidates are the rows the kept kind 1 postings hold, which are `bm25-postings`'
    candidates, and the rows the kept kind 3 postings hold, each scored once.
  - **Law 1, neutrality against the default.** For every row d whose current set holds none of
    the query words the question field kept (no set, an empty set, or a set that misses), every q_t
    is 0, so x_t = y_t, the second sum of S is 0, and S(d) = T(d). T reads N, `df_1`, `avg_1`, k1,
    b_1, the kept words and step 5's completion, all of them `bm25-postings`' inputs and none of
    them touched by kind 3, so T(d) is the score `bm25-postings` gives d on the same request, and
    d's text words are pruned as `bm25-postings` prunes them. No other row's questions enter T: a
    backfill, a regeneration or a drop elsewhere never moves the score of a row without questions;
    it moves only the covered rows around it. At 0% coverage kind 3 holds nothing, the candidate set
    is `bm25-postings`', and the answer is byte-identical to `bm25-postings`' but for the pipeline
    name. Under pruning, every row of `bm25-postings`' completion window is completed here with
    the same T, by the union of windows above, and the `scan` path and `match: "all"` keep T the
    same way. These are phase 2 property tests (d10). Separate per-field statistics are also why a set's
    length can never lower its row: with Robertson et al.'s one weighted document length, five
    questions would lengthen the row and lower its text's score on every query they miss.
  - **Law 2, a question only adds.** g increases and idf_q ≥ 0, so every increment
    g(y + w_q · q) − g(y) is at least 0 and S(d) ≥ T(d) for every row. A scan finding that drops a
    question or a set (d6) returns its row's increment toward 0 and never below T(d): the gate may
    take away what unreviewed text added and never what reviewed text earns. A drop also lowers
    `novel_rows` by at most one per word, which moves idf_q, and so the increments of other covered
    rows, and moves no row's T. Rows with questions can still pass a row without them on a query
    their questions match; that is the feature, and d8's displacement bound measures its cost.
  - **Law 3, the over-matching bound.** g is concave with g(0) = 0, so g(y + w_q) − g(y) ≤ g(w_q),
    and a row's gain from its questions is
    Δ(d) = S(d) − T(d) ≤ Σ_{t in both the query and the set} idf_q(t) · g(w_q).
    Since w_q < 1, g(w_q) < g(1) = 1: **a question match on t is worth less than one occurrence of
    t in a text of average length, at an IDF no larger than the text's, however many of the set's
    questions hold t.** A word that generators add across a tenant, where the texts lack it, raises
    `novel_rows` and so n_U, lowering its idf_q for every row, and above `FREQUENT_SHARE` of N the
    question field prunes it. A word copied from the text counts once in n_U, so the text's
    salient words keep their IDF. For example, at w_q 0.3, g(0.3) is 0.44; a word held by 5% of
    rows in either field has idf_q about ln 20, 3.0, so it adds at most 1.3, below a single text
    match on a word held by 1% of rows (idf about ln 100, 4.6, g(1) = 1). The bound is a test
    (d10); how many ranks it allows on real sets is d8's adversarial suite.
  - **Repetition no longer amplifies.** Round 2's counterexample: a memory approved with a
    trailing "Related: deploy keys" line, whose five grounded, distinct questions each hold
    "deploy" and "keys" and add "rotate", against an uncovered right memory "rotate deploy keys" of
    the same length, on "how do I rotate deploy keys", with all three words at one IDF I. Under the
    round 1 form (tf_3 the number of questions, 5) the bait scored 2 · g(1 + 1.5) + g(1.5) =
    2 · 1.49 + 1.22 = 4.19 I against the right memory's 3 · g(1) = 3.0 I. Under d3's binary weight it
    scores at most 2 · g(1.3) + g(0.3) = 2 · 1.14 + 0.44 = 2.73 I, below 3.0 I; its gain from
    questions is at most 3 · 0.44 = 1.32 I on any three-word query, against 2.19 I before. A right
    memory about 24% longer than the mean text (B_1 1.18) still ties the bait, as a long memory
    already trails a short one under `bm25-postings`; d8's fourth adversarial style measures that
    residue.
  - **The pipeline.** `bm25-questions` is one retriever, `RetrieverKind::Questions` (`questions`),
    reading kind 1 and kind 3 postings by the served index, scope and state alike, through
    ADR-0052 d4's path and d6's pruning. The own held rows contribute their text, since they have no
    questions (d1). The in-memory store does not serve it, and a composition over it that enables
    the pipeline is refused at start (ADR-0049 d7). It enters disabled. A deployment lists it in
    `search.enabled`, and a request names it with `search: "bm25-questions"` on `oml_read` and
    `/v1/memories/recall`, or a tenant admin makes it the tenant's default (ADR-0049 d4). Serving it
    reads stored postings only, so a lead serves it with no model file. This record does not make
    it a candidate default; the default stays model-free at write as well as at recall (#176), and a
    later record may propose otherwise with ADR-0049 d9's evidence.
  - **What a hit says.** `matched.via` gains `questions`. A hit whose score its questions raised
    gains `matched.question`: `{text, ordinal}`, the one current question of its set that holds the
    most query IDF, ties broken by the lower `ordinal`, read by `(memory_id)` for the hits alone; the
    generator is in `recall_events` and on `oml_get`, not in every hit. `matched.question_terms`
    lists the query words the hit matched only through its questions, so a match by question is
    never presented as a word the memory says. ADR-0052 d17 does the same for `expanded`. `text` is
    escaped on every text surface as ADR-0074 d12 escapes served text.
  - **Repeatability.** For fixed inputs, now including the stored question sets, the answer is
    byte-identical across calls and processes, `recall_id` aside. `recall_events` gains
    `questions_seq` beside `pipeline`. ADR-0054 d11's replay reruns over the sets current at replay
    time; when the tenant's `questions_seq` is unchanged the replay is byte-identical, and when it
    has moved the replay says so and lists, from d2's journal, the set changes between the two, with
    the replaced sets' text still readable. #176's page gains three events that change an answer: a
    set written for a served version, a set regenerated by a backfill, and a question dropped by a
    rescan or an admin. Replay across such an event is best-effort, and the page says so. The page
    also says that a set written for row A can change row B's answer: B's rank against A, and, when
    B has a set of its own, B's question increment through idf_q. It never changes B's text score
    (law 1), and `questions_seq` records it either way.

- **d5 — A `QuestionGenerator` port with one adapter in phase 1; generation is off by default, and
  the server reaches no network for it.**
  - **The port.** `oml_app::ports::QuestionGenerator` takes one version's own search text and
    returns at most `max_questions` strings, the token counts in and out, or a named failure that
    says whether the generator was unreachable or the version failed (d11). The input is exactly
    the text kind 1 indexes for that version, after ADR-0056's format adapter, cut at 512 tokens as
    docTTTTTquery's input was. It holds no attachment-extracted text, no text of a `derives_from`
    source, no anchor and no other version: one call carries one version of one tenant, never a
    batch. Each adapter has a `GeneratorId` built from its adapter, model, digest, prompt version,
    prompt SHA-256, `max_questions` and decoding parameters. The prompt is compiled in, versioned
    `oml-questions/<n>`, and frames the memory as quoted data to ask questions about, never as
    instructions; a T5 checkpoint has no instruction channel and takes the text alone.
  - **One adapter ships** (#235 decides which): **`local`**, a doc2query-style T5 checkpoint run in
    candle inside the lead on d1's pool, pinned by digest and placed by the operator or fetched once
    at start when `fetch = true`, exactly as ADR-0053 d3 places the reranker. `fetch` is the only
    network access, at start, to the URL the operator names, and it is off by default. Phase 0
    also measures an LLM behind an OpenAI-compatible chat endpoint. If it wins by open question 1's
    margin, phase 1 builds `endpoint` instead, and only for a **loopback** endpoint (llama.cpp, vLLM
    or Ollama on the same host): the resolved address must be loopback and is pinned for the
    connection, redirects are refused, and proxy variables are ignored. A non-loopback endpoint
    sends approved memory text to a third party, which an erase cannot reach; it is not built here.
    Allowing one is its own amendment, which must carry a per-tenant egress switch set by a person
    session (ADR-0074 d25), a journal event, and the erasure caveat on doctor and the configuration
    page.
  - **The checkpoint register.** `deny.toml` checks crate licences, not model weights. Pinning a
    checkpoint records, beside `model_digest`, the licence of its weights and of its training data,
    checked by a person; public doc2query checkpoints are trained on MS MARCO, whose terms phase 0
    checks for a hosted service. A licence finding stops `local` in phase 0, and the record falls
    back to the endpoint arm or stops.
  - **Off by default.** `search.questions.generator` is unset by default. A lead with it unset
    loads no model, opens no connection and drains nothing.
  - **Decoding.** Top-k sampling, k 10, as docTTTTTquery measured it above beam search, with a seed
    of the first eight bytes of SHA-256 over the version id and the `GeneratorId`. A rerun on the
    same build and CPU then tends to give the same questions. Repeatability never depends on it,
    because the questions are stored.
  - **A model or prompt change** is a new `GeneratorId`. It applies to a tenant only when an admin
    adopts it (d2); until then the leads configured with it drain nothing for that tenant and doctor
    says so. After adoption, new approvals use it at once, existing sets keep theirs and stay
    served, and doctor's `search_questions_stale` counts the sets under another generator. Only
    `backfill --stale` replaces them (d1). A model change is never silent and never automatic.

- **d6 — A question must be grounded in its memory, specific, short and clean, and any finding
  drops it; a version with a hold finding gets no questions; a question changes ranking, never
  visibility, and is never served as memory text.**
  - **The input side.** A generator reading an injection can be steered by it. A version with any
    finding whose class holds by default, approved or dismissed by a person, gets no questions, and
    its queue entry ends `outcome: empty` with the reason `source_finding`. The default is the
    class's verdict before any tenant policy: ADR-0065's under `oml-scan/1.x`, and ADR-0074 d1's if
    that record is accepted. A tenant ceiling that lowers a class to `annotate`, or an allowlist
    entry (ADR-0074 d22), spares the tenant's readers a held queue; it does not open the version
    to the generator, which is the reader ADR-0074 d2 says can be talked round and whose output
    ranks in every reader's recall. A rescan that adds such a finding after the set was written
    drops the set (below).
  - **Shape** filters run first, in the generator's order, and drop a question that is longer than
    200 characters, spans more than one line, does not end in `?`, holds `://`, a backtick fence or
    a character of the hidden-character table (ADR-0074 d5 if accepted, ADR-0065's escaping
    otherwise), or repeats another question of the set after NFC, case folding and whitespace
    folding.
  - **Diversity.** A question whose content lexemes overlap an earlier kept question's by a Jaccard
    index of 0.5 or more is dropped as `near_duplicate`, in the generator's order. Five rephrasings
    of one question add nothing to ranking under d3's binary weight, and they crowd out questions
    that would cover another intent of the memory.
  - **Genericity.** After stop words, a question with fewer than two content lexemes is dropped,
    and so is one whose content lexemes all lie in the generic lexicon compiled with the prompt
    (`oml-questions/1`: words such as "configure", "recommended", "way", "use", "work", "best"),
    versioned with it.
  - **Grounding**, the lexical stand-in for Doc2Query--'s relevance model. A question keeps at most
    8 content lexemes, or is dropped. At least half of them must be lexemes of the version's own
    kind 1 field (by the `english` form), or the question is dropped. Across the set, at most 10
    lexemes absent from the kind 1 field are kept; a question that would exceed that is dropped. So
    a set can add new vocabulary, which is the point (docTTTTTquery's 33% new words), but a memory
    cannot make its generator emit a list of words the reviewer never saw. A trailing "Related:
    credential rotation, deploy keys" line in an approved memory is reviewed text, so its words
    already rank the memory through kind 1. Grounding stops the generator from adding new words;
    it does not stop it repeating the line's words in every question. That repetition is stopped by
    d3's binary weight, which counts a word once per set, so the line's words gain at most g(w_q)
    each through questions (d4's law 3 and its worked counterexample).
  - **The scan.** Each remaining question is scanned by the composed rule set (`oml-scan/1.x`
    today, `/2` under ADR-0074) as one free-text field. **Any finding drops the question**, whatever
    its verdict: refuse, hold or annotate, and whatever a tenant ceiling or allowlist says
    (ADR-0074 d22). A memory's author may need such words, so a memory is held
    for a person; a generated question has no author who needs it and no reviewer who will read it,
    and a drop costs one question of five. At most `max_questions` (5) survive, in the generator's
    order.
  - **Logged, never kept.** A drop increments `oml_questions_dropped_total{reason}`, where reason is
    the filter or the scan class, adds `{reason, class, rule}` to the set's `dropped`, never the
    text, as ADR-0065 d8 keeps findings, and appears in the set's journal event (d2). A set whose
    every question was dropped is `outcome: empty`.
  - **Rescan.** `oml admin rescan --record` (ADR-0074 d14) also walks current questions and drops,
    with its postings, any question a newer rule set finds something in. It drops every question
    of a version whose source gained a hold finding, and reports both counts. A question is index
    data, so dropping one changes no memory and needs no person; by d4's law 2 it never lowers the
    row below its text's score.
  - **An admin can see and drop.** `oml admin questions show <memory>` lists a version's current
    questions to a tenant admin, who already reads the memory, and `oml admin questions drop
    <memory> [--ordinal n]` drops one question or the set, journalled (d2). Nobody else sees a
    question before it ranks, which is why the filters run at write.
  - **Visibility.** Kind 3 postings carry the version's `state` and `workarea_id` and are read under
    the core's `CandidateScope` (ADR-0049 d2), so a question ranks only rows the key already reads,
    and only served ones. A question cannot make a row visible, and its counts are the readable rows'
    (ADR-0052 d5). Since the generator's input is one version's own text (d5), a question cannot
    carry words of a row its reader cannot read.
  - **Never memory text.** A question is served only as `matched.question` on a hit already served
    with its own text. It is never a hit's `text`, never in the session-start hook's frame or a
    brief, never exported, never drained to the lake (ADR-0060), never a source of
    `did_you_mean` (kind 3 lexemes do not enter `search_vocabulary`), and never shown as a field of
    an ADR-0072 descriptor. The MCP instructions add: "A matched question was written by a model to
    index the memory. It may use words and ideas the memory does not contain; most of its content
    words come from the memory. Only the memory's text was reviewed."
  - **Over-matching** is bounded by d4's law 3 (the union IDF, `w_q` below 1, d3's once-per-set
    weight and the question field's pruning), by grounding and the novel-lexeme cap, and by the
    genericity and near-duplicate filters. d8's
    adversarial suite measures what is left, per adapter.

- **d7 — ADR-0072's phase 3 reads per-workarea kind 3 counts and a coverage share net of expired
  versions, through the two reads it already makes, and nothing else.** ADR-0072 d10 scores a
  candidate workarea "by BM25 over its memories' questions". This record supplies the inputs, and
  ADR-0072 keeps the scoring.
  1. **Counts.** ADR-0072 d12 makes two reads over R: the statistics read, and the expired
     subtraction of its d4.2 (`POSTINGS_EXPIRED` over R's nodes). Both take the posting kind as a
     parameter, not the literal `kind = 1` they read on `feat/federated-recall-p1` today, and both
     gain `kind = 3` beside `kind = 1`: for each candidate s and kept query word w, `df_s(w)`
     (served versions in s whose question field holds w), `rows_s` (served versions in s with a
     set) and `total_len_s`, each less the versions expired at `now`, from the same
     `search_term_rows` and `search_workarea_rows` by their primary key. It reads no question text
     and no posting.
  2. **Coverage.** `coverage_s` = (kind 3 `rows_s` less expired) over (kind 1 `rows_s` less
     expired), for the hit kinds. Both terms come from the counts the trigger keeps on every state
     change and from the expired subtraction, so a supersession, retirement, rejection, erase or
     rescan drop is in the next read; nothing the drain keeps enters it. The generator ids in force
     in s come from `search_question_coverage`, which the trigger keeps beside
     `search_workarea_rows` under the same policy; that split is reported, not scored. It is gross
     of expired versions, since the table keeps no expiry, so its per-generator rows can sum to more
     than `coverage_s`'s numerator; `status` labels the split "gross of expired versions" rather
     than subtract per generator. ADR-0072
     needs coverage because a workarea whose backfill has not run would otherwise rank low for want
     of questions, not for want of answers. How #207 uses it (a floor, or scoring only fully covered
     candidates) is ADR-0072's to decide on its eval.
  3. **Invariants** this record guarantees and #238 tests: kind 3 counts are the served,
     unexpired versions' alone; a held, superseded, retired, rejected, erased or expired version
     contributes nothing; a question held only by unreadable rows changes no count; and the counts
     summed over a workarea equal a count of its served, unexpired versions' kind 3 postings under
     the policy (ADR-0072 tests 14 and 15).
  4. **Repeatability.** The `questions_seq` and generator ids read are recorded in
     `recall_events` when a selection used them, beside ADR-0072 d9's fields, so ADR-0054 d11's
     replay can say what it read. `coverage_s`, the per-generator split and the generator ids are
     recorded there only, never in a key's answer: a reader who could poll coverage around an
     approval would learn whether that memory drew a `source_finding`.

- **d8 — The gates: a gain the eval can see, no harm on BEIR, no harm to rows without questions,
  an adversarial suite per adapter, a latency budget under load, a size budget, and the bill
  stated before it is run up.**
  - **Why the first draft's gates could not decide.** On the 121-query set the baseline is 113 of
    121; the draft's gate, four queries more, is 3.3 points. Four wins and no losses give a
    one-sided exact McNemar p of 0.0625 (0.5^4). By exact enumeration of the binomial, a true gain
    of 3.3 points is detected at one-sided α 0.05 with power 0.37 at n 121 if there are no losses,
    and 0.26 if losses run at a third of the wins (wins 5.0%, losses 1.7% of queries). Half the set
    was also to be used for tuning. So the 121-query set is reported, never gated on.
  - **Phase 0a grows the labelled set.** The planning effect is a gain of δ = 3.3 points of
    success@5, with wins p10 5.0% and losses p01 1.7% of queries. Connor's paired-sample formula,
    n = (z_α √(p10 + p01) + z_β √(p10 + p01 − δ²))² / δ², with z_α 1.645 (one-sided α 0.05), gives
    (1.645 · 0.259 + 0.842 · 0.257)² / 0.033² ≈ 378 for 80% power and
    (1.645 · 0.259 + 1.282 · 0.257)² / 0.033² ≈ 523 for 90%. Exact enumeration of the one-sided
    conditional McNemar test (every count of wins W and losses L, weighted by its multinomial
    probability, passing when the binomial tail P(Bin(W + L, 0.5) ≥ W) is below 0.05) gives 412
    for 80% and 546 for 90%; with no losses, 203 for 80%. Phase 0a therefore writes at least **550**
    new question-shaped and paraphrased queries over the synthetic corpus, labelled against it,
    for 90% power (0.902 at n 550 by the same enumeration, and at least 0.90 at every n from 547 to 719, so the
    set may overshoot 550 without losing power). They are written by a person or by a
    model of a family different from both the corpus's writer and phase 0's endpoint generator, so
    neither generator can win by echoing its own family's phrasing; a person checks the labels of a
    10% sample. Per-query results of every run are committed. 0a's rerun repeats this computation
    with the wins and losses measured on the grown set.
  - **Tuning.** w_q is chosen by five-fold cross-validation on the grown set, over a grid inside
    (0, 1); every figure gated on is out-of-fold.
  - **Why the gain is gated on the test, not on the point estimate.** Round 1's gate was "gain
    ≥ 3.3 points and p < 0.05", with n sized for a true gain of exactly 3.3. When the true gain
    equals the threshold, the observed gain clears it about half the time at any n: by the same
    enumeration at n 420, P(p < 0.05) is 0.81 but P(p < 0.05 and gain ≥ 3.3) is 0.52. The gate now
    rests on the test, with δ the effect its power targets, and keeps a floor of 1.5 points so that
    a significant but trivial gain does not pass; at n 550 the floor changes the power by less than
    0.001 (0.902 either way).
  - *Amended 2026-11-11 (d12):* for phase 0, d12's comparison against `bm25-sparse` replaces the
    superiority gate, the three non-inferiority arms and the joint power below; the arms d12 moves
    to phase 2 and the adversarial suite, latency, write-latency, size and bill stay as written.
  - **Quality, measured offline in phase 0 and served in phase 2.** One superiority gate, three
    non-inferiority arms and one displacement bound:

    | Set | Baseline (`bm25-postings`) | Gate for `bm25-questions` | Power at the planning effect |
    |---|---|---|---|
    | grown set, ≥ 550 queries (0a) | measured in 0a | one-sided exact McNemar p < 0.05 on the discordant pairs, and a success@5 gain ≥ 1.5 points | 0.90 at δ 3.3 |
    | synthetic, 121 queries | success@5 0.934, nDCG@10 0.873 | reported per category, never gated (0.26 to 0.37 power, above) | — |
    | SciFact, 300 | nDCG@10 0.685 | non-inferior: lower bound of the 95% paired bootstrap interval (10,000 resamples) of the nDCG@10 difference above −m | 0.99 at a true difference of 0, by m's choice below |
    | NFCorpus, 323 | nDCG@10 0.321 | non-inferior, as SciFact | 0.99, as SciFact |
    | LoCoMo, 1,535 queries in 10 conversations | nDCG@10 0.503 | non-inferior, clustered: a one-sided randomisation test that flips the signs of the ten per-conversation differences plus m (1,024 assignments), p < 0.025; the gain is reported | 0.99, as SciFact, from the per-conversation SD |
    | partial coverage, ≥ 300 queries pooled | `bm25-postings` on the same queries | displacement: at 95% coverage, the one-sided 95% Clopper–Pearson upper bound on the share of queries whose right memory has no set and leaves the top 5 is below 4%; at 300 queries, at most 6 displaced (bound 3.9%) | 0.967 at a true 1% displacement, 0.999 at 0.5% |
    | adversarial, per adapter | — | ten planted memories in each of four styles, generic, bait for a named set of ten target queries, text addressed to the generator, and a reviewed trailing line whose words the generator repeats across every question: no planted memory enters the top 5 of more than one non-target query, and the target queries' right memories leave the top 5 in at most one of ten, on the grown set and LoCoMo | an engineering bound, not a test |

  - **The non-inferiority margin m is sized for power, not fixed.** A fixed margin of 0.01 nDCG@10
    has power Φ(0.01 √n / σ − 1.96) at a true difference of 0, where σ is the per-query SD of the
    paired difference; if σ were 0.10 on SciFact's 300 queries, that would be 0.41. Phase 0a reruns
    `bm25-postings` and `bm25-sparse` on each set with per-query output (the committed results hold
    aggregates only), takes σ_i of their paired difference as the planning SD, since kind 2 is the
    expansion OML has measured, and sets m_i = max(0.01, (1.960 + 2.326) · σ_i / √n_i), which gives
    99% power at a true difference of 0. LoCoMo uses the SD σ of its ten per-conversation means,
    √10 and Student's t with 9 degrees of freedom, m = (2.262 + 2.821) · σ / √10: with normal
    quantiles the ten-conversation sign-flip test reaches about 0.965, not 0.99, and with t
    quantiles 0.991 (round 3, 4,000 simulated runs of the exact 1,024-assignment test). A margin is capped at 0.02; an arm whose margin hits the cap gates at 0.02, and its lower
    power enters the joint figure below.
  - **The displacement bound.** With law 1, a row without questions keeps its exact
    `bm25-postings` score, so the only harm left is covered competitors passing it, and that harm
    is one-directional. Round 1's bootstrap on about 21 such queries per set was decided by one or
    two queries; this arm pools the grown set, LoCoMo, SciFact and NFCorpus and leaves without sets
    the right memories of a random sample of their queries, at least 300 in all, then removes sets
    from further random rows until coverage is 95%. Half of the uncovered right memories are
    chosen at random and half are given a planted hold-class finding (a scan rule hit inserted into
    the memory before generation), since none of the external sets has scan findings of its own.
    The tolerable rate is 4%: at 95% coverage an uncovered right memory is about 5% of queries, so
    4% of them displaced costs about 0.2 points of success@5 overall, against the 3.3 the record
    must gain. 80% coverage is reported with the same count.
  - **The joint power.** A go needs every gate to pass. A false go, a go when the chosen arm gains
    nothing, needs a superiority test to reject wrongly; with the arms tested in the fixed sequence
    below, the go decision's false-positive rate is at most 0.05, and each non-inferiority null is
    held at 0.025. Its power is at least 1 minus the sum of the gates' type II errors, by the
    union bound and without assuming independence: 1 − (0.098 + 3 · 0.01 + 0.033) ≈ **0.84**, when
    the true effect is the planned one (a 3.3-point gain, no difference on BEIR or LoCoMo, at most
    1% displacement). 0a's rerun restates it with the measured σ_i and any capped margin, and if it
    falls below 0.80, 0a grows the set further before phase 0 runs. The figure covers the
    statistical gates; the adversarial suite and the latency, write-latency and size gates are
    pass/fail engineering bounds outside it.
  - **Pre-registered arms, tested in a fixed sequence.** The verdict names one of two arms fixed
    before phase 0 runs: `local` and the endpoint LLM, each with five questions and every d6 filter
    on. Gating both against `bm25-postings` on one set and taking the better would let either test's
    error through, a false-go rate up to 0.10. So: (1) the first arm, `local`, or the endpoint when
    d5's register stops `local`, faces every gate above; (2) only if it passes, the endpoint faces
    the first arm by the same one-sided exact McNemar test and 1.5-point floor on the grown set, and
    is chosen only if that passes and it clears every other gate too. A fixed sequence keeps the
    family-wise error at 0.05 and n at 550 (splitting α at 0.025 an arm would need 663 for 90%, by
    the same enumeration). If step 1 fails the record stops, and the endpoint's figures are
    reported for a later amendment with a run of its own. The other variants,
    five against ten questions, each filter off, and other values of the filters' constants, are
    reported and are never chosen among on the gated sets; adopting one is a later amendment with
    a run of its own.
  - **Reported beside the gates.** For reference, `bm25-sparse` measured 0.983 / 0.910 on the synthetic set and 0.550 on LoCoMo,
    and BEIR's docT5query gained 0.003 to 0.010 over BM25. Phase 0 also reports, beside the gates,
    five against ten questions per version (QuOTE plateaus at 10 to 15); what each filter of d6
    dropped and the gain with each filter off; the share of generated questions that reproduce a
    benchmark query verbatim after normalisation (a sign the generator saw the set in training),
    and the gates without those questions when the share exceeds 1%; and, as the competing
    investment, `bm25-sparse` with each version's kind 2 capped at its 36 highest-weighted
    expansion terms (kind 1's mean postings per version, ADR-0052 d10), scored offline on the same
    sets with the latency proxy below. A pipeline that misses a gate is not offered. When phase 0
    misses, the record stops (d10).
  - **Real usage.** When ADR-0054 d11's real-usage set holds at least 100 labelled misses (queries
    whose right memory was not in the top 5), the share of them `bm25-questions` brings into the top
    5 is reported beside the gates. It is no gate until then, because no such set exists yet.
  - **Latency.** `bm25-questions` p95 ≤ 50 ms at 100,000 memories, universe scope (ADR-0052 d9's
    bar), and at most 15 ms above `bm25-postings` on the same run, **with the question drain
    saturated** on the same host (d1's pool at its default). Its cost is one more kind of postings
    read, one rescore and one `(memory_id)` read for the named questions of the returned hits. The
    100,000-memory bench gets its kind 3 postings from phase 0's generated sets loaded into the scale
    database, not from a 28-hour backfill. Phase 0 runs the same read shape on those postings as a
    latency proxy, so its verdict covers latency as well as quality. At 1,000,000 it is measured with
    #103, and `bm25-postings`' own bar there is unresolved (ADR-0052 d10).
  - **Write latency under a backfill.** d1's set writes share the tenant's chain head. With the
    drain saturated on the same tenant at its default rate, the p95 of promotion, supersession and
    erase is at most 20% or 2 ms above its baseline on the same run without the drain, whichever is
    larger, measured in phase 2's bench. If it misses, phase 1 batches set writes as d1 describes
    and the gate is rerun.
  - **Size,** budgeted and then measured in phase 1 (#236). Kind 1 measured about 36 postings per
    version, and heap, indexes and counts of about 1.1 GiB per 165,000 memories, about 6.7 GiB per
    1,000,000 by linear extrapolation (ADR-0052 d10). The budget is kind 3 at no more than kind 1 in
    total: about 0.67 GiB at 100,000 memories and 6.7 GiB at 1,000,000, by the same extrapolation.
    d6's caps bound a set at 40 content lexemes per form, so on short memories (the synthetic set's
    mean is 121 characters) kind 3 can exceed a version's kind 1; phase 0 reports the ratio per
    corpus, and the budget is the total. #201's "2–4× the postings" assumed raw words. The question
    text is arithmetic, not a measurement: five questions of at most 200 characters, about 100 bytes
    each with row overhead, is about 0.05 GiB at 100,000 and 0.5 GiB at 1,000,000 per generation
    kept (d2). When a budget is exceeded, `max_questions` falls by amendment.
  - **The bill**, stated as arithmetic until phase 0 measures it, and metered once built (d11).
    With the compiled prompt at about 150 tokens, a memory cut at 512 tokens, and five questions of
    at most 64 tokens each (320 out, about 75 typical), and counting text at four bytes a token as
    recall budgets do with the synthetic set's 121-character mean memory:

    | Volume | Tokens in, typical / worst | Tokens out, typical / worst | Time at 60 a minute per lead |
    |---|---|---|---|
    | 1,000 approvals | 0.18 M / 0.66 M | 0.075 M / 0.32 M | about 17 minutes |
    | 100,000 (backfill) | 18 M / 66 M | 7.5 M / 32 M | about 28 hours |
    | 1,000,000 (backfill) | 180 M / 660 M | 75 M / 320 M | about 11.6 days |

    For `local`, the cost is CPU time on d1's pool, which phase 0 measures on the 8 vCPU host by
    running the same candle T5 phase 1 would ship, and from which it sets the default rate. No
    hosted endpoint is built here (d5), so no price per token is quoted; the token columns are what
    a later amendment that allows one would price, from a price page it cites. docTTTTTquery's own figure, about 40 TPU hours for 5 queries over
    8.8 million passages, is the external reference.

- **d9 — Dense embeddings of the questions are deferred, and return only when a named trigger
  fires.** They need a vector index, an embedding per question and a model at recall to embed the
  query. Under row security no operator class has yet been shown to serve the serving role. That was
  ADR-0052's lesson for text, and nothing here measures pgvector against it. Revisit when all three
  hold: `bm25-questions` passed d8 and paraphrase success@5 on the grown set still has two or more
  misses in a hundred; on the real-usage set, at least half the remaining misses share no kept word
  with any question of the right memory; and a vector index has been measured serving the serving
  role inside the 50 ms budget at 100,000. QuOTE's results are dense, and would be the reference for
  that record.

- **d10 — Phase 0 decides; phases 1 and 2 ship in one release; phase 3 is built when #207 is
  scheduled.** Each phase is one issue in v0.7.1. Phase 1 alone would spend generation CPU and
  write postings nothing reads, so it never ships without phase 2, and `oml admin questions enable`
  refuses on a build that does not serve `bm25-questions`.

    | Phase | Builds | Issue | Built when | Acceptance (each test red first by breaking its clause) |
    |---|---|---|---|---|
    | 0a | the grown labelled set (≥ 550 queries, d8), its labels and a person's 10% check; per-query reruns of `bm25-postings` and `bm25-sparse` on every set for the planning SDs | #235 | now: it changes no server code | the set committed with its provenance; the power computation, each margin m_i and the joint power rerun on it, the joint power at least 0.80 |
    | 0 | offline generation for the grown set, the synthetic set, SciFact, NFCorpus and LoCoMo with `local` and an LLM; d6's filters and scan; offline BM25F scoring with w_q by five-fold cross-validation; the two pre-registered arms in d8's fixed sequence; the displacement arm; the adversarial suite; the capped-sparse comparison; the latency proxy; rate, tokens and kind 3 postings per memory; the checkpoint register | #235 | after 0a | d8's gates offline; every d8 report; a go or stop verdict on #201, naming the adapter; on stop, or when capped `bm25-sparse` clears the same gates inside the latency budget, this record is marked declined and #236 to #238 closed |
    | 1 | migration `search_questions`, the trigger, the queue and drain, the port and phase 0's one adapter, d6's filters and scan, kind 3 postings and counts, the journal and `questions_seq`, `oml admin questions` (`enable`, `adopt`, `backfill` with `--dry-run`, `--limit`, `--stop`, `--resume`, `status`, `show`, `drop`, `retry-parked`), erase and rescan, d11's outage handling, doctor and metrics | #236 | after phase 0 says go; released with phase 2 | an unreviewed, held or in-progress-only version never gets a set; a version with a finding whose class holds by default gets none, under a tenant ceiling or an allowlist too; a question with any finding, or failing grounding, genericity or a cap, is dropped, counted and recorded without its text; the generator's input is that one version's own text; a version superseded or erased during generation gets no set; an entry whose lease lapsed and was taken by a second lead is written once, with one event, one `questions_seq` step and one token count; a version that always times out while the canary succeeds is parked after five attempts and the drain goes on; a canary failure counts no attempt; a word in all five questions writes one posting of weight 1; `novel_rows` counts a row only when its text lacks the token; a near-duplicate question is dropped; supersession, retirement and rejection flip kind 3 with kind 1; erase deletes every set, question, posting, count and coverage row in one transaction; every set change writes one journal event and bumps `questions_seq`; a lead whose generator differs from the tenant's drains nothing; a tenant with questions off enqueues nothing; an unreachable generator parks no entry and the drain resumes on its own; a parked entry returns after its backoff, and `retry-parked` is journalled; a backfill stops, resumes and honours its limit across the resume; a failed or discarded attempt's tokens reach the tenant's totals; d8's size budget at 100,000 |
    | 2 | `RetrieverKind::Questions`, `bm25-questions`, `matched.question` and `question_terms`, `questions_seq` in `recall_events`, the served eval and the benchmark pages, #176's page | #237 | with phase 1, in one release | d8's gates served; the recall latency budget and the write latency gate with the drain saturated; d4's law 1 as property tests (at any coverage, every row with no set, an empty set or a set missing the query has exactly its `bm25-postings` score, and `search.pruned` equals `bm25-postings`'; at 0% coverage the answer is byte-identical to `bm25-postings`' but for the pipeline name; a set written for one row changes no other row's text score; with a budget-pruned word, every row of `bm25-postings`' completion window keeps its full score, on the `scan` path and under `match: "all"` too; with federation on, a hit's score is the same whether a source is selected or not, ADR-0072's test 6) and law 2 (dropping a set never lowers a row below that score); law 3's bound, idf_q(t) · g(w_q) per word, holds for every row of the eval sets; byte-identical answers across calls and two processes for fixed inputs; a replay across a set change names the change; a question of an unreadable version changes no byte, count or `did_you_mean`; no question text in `text`, the hook, an export or the lake; `did_you_mean` offers no word only questions hold |
    | 3 | d7's kind 3 counts, through both reads, and coverage in ADR-0072's statistics read, recorded in `recall_events` | #238 | when #207 is scheduled, not before | ADR-0072 tests 14 and 15; the counts law, with an expired version contributing nothing |

- **d11 — Operations: an outage, told from a bad version by a compiled-in canary, pauses the drain
  and it resumes on its own; parked entries come back with backoff; a backfill is estimated, capped, stoppable and tracked; the bill is metered;
  doctor names each state.**
  - **The canary.** Each adapter carries a canary input compiled in with the prompt
    (`oml-questions/1`): a fixed memory text of about 40 tokens, no tenant's data, with a timeout
    of a quarter of `search.questions.timeout`. It is how a failure is classed: after a failed or
    timed-out call, the worker runs the canary; a canary that fails means the generator is down, and
    one that succeeds means the version failed (d1). With `threads` 1 there are no other calls to
    compare against, so the canary is the only way to tell a slow or pathological version from an
    outage.
  - **Generator down.** A connect error, a canary that fails or times out, a 5xx, a 429 or a model
    that fails to load means the generator is unreachable. The lead then pauses its whole drain,
    counts no attempt against any entry, and probes **with the canary, never a queue entry**, after
    30 s, doubling to at most 15 minutes, with jitter. The first canary success resumes the drain.
    A version that always times out therefore cannot hold the drain: the canary succeeds, its
    timeouts count as its own attempts, and it is parked after five. Doctor's `search_questions_generator_down`
    names the generator and since when, and the gauge `oml_questions_generator_up` reads 0. An
    outage therefore parks nothing, and every approval made during it gets its questions afterwards.
  - **Parked entries come back.** A parked entry is retried after 1 day, then 2, then 4
    (`park_count`), and after the third return it stays parked until an admin's `oml admin
    questions retry-parked [--tenant t]`, journalled as `search.questions_parked_retried` with the
    count, or until the version leaves served, which removes its entry (d1). A supersession serves
    a new version with a new `memory_id`, which d1 enqueues on its own; the parked entry of the old
    version leaves the queue then. An adopted generator change also unparks every entry of the
    tenant, since the failure may have been the old model's.
  - **The estimate first.** `backfill --dry-run`, `enable` and `adopt` print the versions to
    generate, the tokens in and out by d8's arithmetic (typical and worst), and the time at the
    configured rate. `backfill` journals the estimate it started with.
  - **Control.** `--limit n` caps a run at n versions in total, recorded as `backfill_limit`;
    `backfill_taken` counts what the run has enqueued, and `--resume` continues the same run, so it
    enqueues at most `backfill_limit − backfill_taken` more. A new limit needs a new run.
    `backfill --stop` removes the run's untaken `priority` 1 entries, keeps its resume point and
    records `backfill_stopped_at`, journalled as `search.questions_backfill_stopped`; `--resume`
    continues from the resume point. `oml admin questions status [--tenant t]` gives, per tenant,
    the backfill's done and total, the rate measured over the last hour and an ETA, the backlog by
    priority, parked and failed counts, sets per generator, stale sets, and `empty` sets by reason,
    `source_finding` among them, so the versions excluded by open question 2 are counted.
  - **Metering.** `oml_questions_tokens_total{direction}` (`in`, `out`) counts tokens for either
    adapter. Each set stores its own; a failed attempt adds its tokens to its queue entry, and every
    attempt, kept, failed, timed out or discarded by d1's checks, adds its tokens to the tenant's
    `tokens_in_total` and `tokens_out_total` in `search_questions_state` (d2), in a short
    transaction of its own when the write was discarded. A tenant's bill is
    those totals, not the sum over its sets, which would leave out the attempts no set holds.
  - **Doctor lines:** `search_questions_backlog`, `search_questions_failed`,
    `search_questions_parked`, `search_questions_stale`, `search_questions_undrained` (questions on
    and no lead drains the tenant), `search_questions_generator_down`,
    `search_questions_generator_mismatch`, `search_questions_backfill` (a run's progress and ETA),
    `search_questions_unused` (generation on and no request or default used `bm25-questions` in 7
    days, so the tenant pays for no reader), and `search_questions_unserved` (a tenant whose
    default is `bm25-questions` with generation off or no sets, so it behaves as `bm25-postings`).

- **d12 — Phase 0's gate, as the operator redesigned it on 2026-11-11: generated questions must
  beat `bm25-sparse` or add to it on question-shaped queries, by a paired test the set can see,
  or v0.7.1 recommends `bm25-sparse` and the question work is parked.** It amends d8 for phase 0.
  - **Why d8's gate could not decide.** 0a's rerun on the grown set
    (`eval/bench/results/phase0a/power.md` on PR #245) capped all three non-inferiority margins at
    0.02, and the union bound over d8's arms came to 1 − (0.053 + 0.571 + 0.083 + 0.336 + 0.033)
    = −0.08, which says nothing. It also measured the competitor d8 only reported: `bm25-sparse`
    reached success@5 0.956 (95% interval 0.938 to 0.969) on the 665 grown queries, against
    `bm25-postings`' 0.913, with 31 wins and 2 losses. A question field that beats
    `bm25-postings` and not `bm25-sparse` would cost a generator, a queue, a migration and kind 3
    postings for less than a pipeline OML already serves. So the question phase 0 answers is
    whether questions beat or add to `bm25-sparse`.
  - **The set.** The grown set's 665 queries and the synthetic set's 121 answerable ones, 786 in
    all. The question-shaped categories, `question` and `paraphrase`, hold 687 (665 + 22); the
    other nine categories hold 99. Success@5 is the measure, per query, as d8's.
  - **Primary: beat.** `bm25-questions` (d4, five questions, every d6 filter on, w_q by d8's
    five-fold cross-validation) against `bm25-sparse` (ADR-0052 d13's convex combination, 0.3
    `bm25-postings` and 0.7 `sparse`) on the 687: a one-sided exact conditional McNemar test on
    the discordant pairs, p < 0.025, and an observed gain of at least 1 point.
  - **Secondary: add.** `bm25-sparse` with `bm25-questions` in place of its `bm25-postings` leg
    (the same 0.3 / 0.7 weights, w_q by the same cross-validation) against `bm25-sparse` on the
    687, by the same test.
  - **One alpha for two tests.** A go needs either, so the two share the go's false-positive rate
    by Bonferroni: 0.025 each, at most 0.05 together.
  - **The power computation.** With π_d the share of queries on which the two pipelines disagree
    and δ the gain, wins are p10 = (π_d + δ)/2 and losses p01 = (π_d − δ)/2. Exact enumeration of
    the test above (every count of wins and losses weighted by its multinomial probability, d8's
    method, `eval/bench/power.py`'s `mcnemar_power` at alpha 0.025 and the 1-point floor) gives the
    least gain detected with 80% power at n 687; Connor's formula, n = (z_α √π_d + z_β √(π_d −
    δ²))² / δ², agrees within 0.1 point. The planning π_d are the discordances the served
    pipelines measured on these 687 queries:

    | planning discordance | π_d | least gain, 80% power (exact) | 90% (exact) | Connor, 80% |
    |---|---|---|---|---|
    | `bm25-sparse` vs `bm25-postings`, 34 of 687 | 0.0495 | 2.5 points | 2.8 points | 2.4 points |
    | `bm25-sparse` vs `bm25-rerank`, 19 of 687 | 0.0277 | 1.9 points | 2.1 points | 1.8 points |
    | `bm25-sparse` vs `bm25-sparse-rrf`, 10 of 687 | 0.0146 | 1.4 points | not reachable | 1.3 points |

    The gate plans on the first row, the largest discordance a lexical change has shown against
    `bm25-sparse`: **a gain of 2.5 points of success@5 is the threshold the set detects with
    80% power** (0.813 by enumeration). A pair that disagrees less is seen at a smaller gain, so
    the plan is the conservative one. The ceiling is `bm25-sparse`'s own misses: 31 of the 687
    (4.5%) leave the top 5, so a 2.5-point gain needs questions to rescue about 17 more of those than
    they lose elsewhere. As d8 found in round 1, requiring the observed gain to reach the planned
    effect would pass a true 2.5-point gain about half the time, so the gate rests on the test,
    and the 1-point floor keeps a significant but trivial gain out; at the planned effect the
    floor costs under 0.001 of power. Phase 0 reports the observed π_d and the least gain
    detectable at it.
  - **Non-inferiority on the other categories.** Each candidate is compared with the pipeline it
    extends, on the 99 other queries: `bm25-questions` with `bm25-postings`, the combination with
    `bm25-sparse`. A question that harms an exact, identifier or typo query harms it against the
    text it was added to; comparing `bm25-questions` with `bm25-sparse` there would gate on the
    sparse leg's own lead (5 wins in 99), not on what questions did, so that comparison is
    reported. The arm passes when the lower end of the 95% paired bootstrap interval (10,000
    resamples) of the success@5 difference is above −m. m is computed, not capped: at a true
    difference of 0 the paired difference's SD is √π_d, so m = (z_0.975 + z_0.90) √π_d / √n, with
    π_d 5 of 99 (`bm25-sparse` against `bm25-postings` there), gives **m = 0.073** for 90% power
    (0.063 for 80%, 0.081 for 95%). The 99 queries rule out a loss of about 7 points and no less;
    that is what they can support, and phase 2's served eval is where a finer bound is measured.
  - **The joint power.** A go through the primary needs it and its non-inferiority arm, so its
    power is at least 1 − (0.187 + 0.10) ≈ **0.71** at the planned effect, by the union bound;
    either path to a go only adds to it. That is below d8's 0.80, and it is stated rather than
    raised: the operator asked for 80% on the primary comparison and a margin the set supports,
    and a 95% non-inferiority arm would widen m to 8.1 points to gain 0.05.
  - **What leaves phase 0.** SciFact, NFCorpus and LoCoMo's non-inferiority arms and the
    displacement arm move to phase 2's served eval, run before the release with margins computed
    as above and never capped; they no longer gate phase 0. On a go, phase 0 then runs d8's
    adversarial suite and its latency proxy before #236 starts. **The budgets stay** as d8 states
    them: generation cost, index size (kind 3 at no more than kind 1 in total), recall latency
    (p95 ≤ 50 ms at 100,000 memories, at most 15 ms above `bm25-postings`) and write latency.
  - **The generator.** The arm is `local`, a doc2query T5 (d5): not a Phi model, since a Phi
    model drafted the grown set's queries (d8). A doc2query T5 writes MS MARCO-style queries,
    mostly without a closing `?`, which d6's shape filter would drop. The adapter therefore
    closes an unpunctuated generation with `?` before d6 runs, a reading fixed here before any
    retrieval was scored; d6 as worded is reported beside the arm and never chosen.
  - **The decision rule.** A go is the primary and its non-inferiority arm passing, or the
    secondary and its non-inferiority arm passing. A go ships the pipeline that passed, opt-in:
    `bm25-questions` on the primary; on the secondary alone, the question field as
    `bm25-sparse`'s lexical leg, named in phase 2. Otherwise **v0.7.1 recommends `bm25-sparse` as
    the documented pipeline for question-style use**, #236 to #238 are parked with phase 0's
    evidence, and this record stays proposed until a later run of its own reopens it.
  - **The label check.** A person's check of a seeded 10% of the grown set's labels
    (`person_check` in `eval/recall/queries-grown.json`, PR #245) is still required. Until it
    passes, every phase 0 figure is provisional and no verdict is final. *Done 2026-11-11 (d13):*
    the operator checked the 67 sampled labels and corrected one (g0113), and phase 0 was rerun
    on the corrected set.

- **d13 — Phase 0's outcome, and the operator's decision of 2026-11-11: generated questions
  neither beat `bm25-sparse` nor add to it, so question indexing is parked and v0.7.1 recommends
  `bm25-sparse`.** The operator's words: "Park questions, promote sparse."
  - **The label check.** The operator checked the seeded 10% sample of the grown set (67 queries,
    seed 20261111) on 2026-11-11: 58 marked correct, 6 unsure, 3 wrong. The three wrong marks
    (g0413, g0422, g0456) and five of the unsure ones (g0354, g0642, g0650, g0651, g0654) object to
    the fictional company's stack (its cloud provider, Kotlin, Gradle), not to whether the labelled
    memory answers the query. The sixth unsure mark is g0113, "What database table contains
    financial records for the team?", labelled only the invoices table (m033). The operator's note
    called the query vague, with the ledger (m037, m038) and payments (m043) also financial
    records, and asked to add m037 and m038 or drop the query. The operator then chose, on
    2026-11-11, to add m037 and m038 and keep m033: version 2 of the set labels all three grade 2.
    The operator reviewed the 67 sampled labels; one (g0113) was corrected at their decision; no
    pass threshold was set in advance.
  - **Who wrote the corpus is unknown.** d8 asks that the grown set's writer (Phi-3-mini) be of
    another family than the corpus's writer. The operator does not know which model wrote
    `corpus.json` (2026-11-11), so that condition is unverified. If they were one family, queries
    and memories would share phrasing, which flatters lexical matching for every pipeline; it
    is unlikely to explain a paired loss of 24 to 2, and the reopen clause below, real recall misses, is
    the remedy.
  - **What phase 0 measured** (PR #252, `eval/bench/results/phase0/report.md`, on version 2;
    offline, over migration 0020's lexemes, with `bm25-postings` matching the served success@5 on
    786 of 786 queries and `bm25-sparse` on 784, the other two, g0468 and g0590, rank-5 and
    rank-6 boundary flips that favour the offline `bm25-sparse`). Success@5:

    | pipeline | all 786 | question-shaped 687 | other 99 |
    |---|---|---|---|
    | `bm25-postings` | 0.916 | 0.911 | 0.950 |
    | `bm25-sparse` | 0.963 | 0.958 | 1.000 |
    | `bm25-questions` (`local`, a doc2query T5, five questions per memory, d6's filters) | 0.927 | 0.926 | 0.939 |
    | `bm25-sparse` with the question field | 0.964 | 0.959 | 1.000 |

    Primary, `bm25-questions` against `bm25-sparse` on the 687: 2 wins, 24 losses, −3.2 points
    (95% paired bootstrap −4.7 to −1.9), p 1.00: it fails. Secondary, the combination against
    `bm25-sparse`: 3 wins, 2 losses, +0.1 points (−0.4 to +0.7), p 0.50: it fails, and its
    interval rules out an added gain above 0.7 points. Both non-inferiority arms pass. At the
    observed discordance (0.038) the set had 80% power for a 2.2-point gain, so the test could see
    the gain d12 asked for. d8's original arm, questions against `bm25-postings`, gained 1.5 points
    (13 wins, 3 losses, p 0.011), mostly on paraphrases: questions help the lexical pipeline, and
    `bm25-sparse` already makes that gain and more. Ten questions per memory, no filters, or d6's
    shape filter as worded change none of this (2.8 to 4.1 points behind `bm25-sparse`). The
    correction to g0113 changes no success@5 figure and no test above; it moves nDCG@10 only.
  - **The cost it would have taken.** Generation ran at 0.77 s per memory on 4 CPU threads, and
    kind 3 would have added 0.575 of kind 1's postings; phases 1 and 2 would add a generator
    port, a drain, a migration and a pipeline, for a ranking `bm25-sparse` already beats.
  - **The decision.** By d12's rule, and decided by the operator on 2026-11-11: question indexing
    is parked. Phases 1 to 3 (#236 to #238) and #201 close as not planned; phase 0 (#235) is done.
    v0.7.1 documents `bm25-sparse` as the recommended recall pipeline, with these numbers and its
    cost. ADR-0072's phase 3 (#207) no longer reads generated questions: with nothing generated,
    d7's counts have no writer, so its descriptor score keeps ontology terms alone. That follows
    from having no writer; phase 0 measured per-memory recall, not ADR-0072's source selection.
    ADR-0072 carries a dated note to this effect at each place it names generated questions. d1 to d11 keep their text as the
    design a reopening would start from; nothing in them is built.
  - **What would reopen it.** A run of its own: a labelled set of real recall misses that
    `bm25-sparse` leaves out of the top 5, on which a question arm beats or adds to `bm25-sparse`
    by d12's tests. The record stays proposed, as d12 says; its status is the operator's to change.

## Consequences

- `docs/architecture/retrieval.md` gains a section on question indexing, and the schema page the
  six tables and kind 3, when phase 1 is built; `configuration.md` gains
  `search.questions.generator`, `.rate`, `.threads`, `.timeout`, `.max_questions` and `.fetch`
  (and `.endpoint.url` and `.endpoint.key_file` only if phase 0 picks the loopback endpoint), with
  the "turning it on" order below; the HTTP API, MCP reference and `openapi.yaml` gain
  `matched.question`, `matched.question_terms` and `via: questions`, and the pipeline table gains
  `bm25-questions`, in the phase 1 and 2 release, not before (ADR-0039 d9). The plugin's memory skill
  names the pipeline.
- **Turning it on**, in order: (1) the operator sets `search.questions.generator` and places the
  checkpoint; (2) the operator lists `bm25-questions` in `search.enabled`; (3) a tenant admin runs
  `oml admin questions enable`, which records the generator and prints the estimate; (4) the admin
  runs `backfill --dry-run`, then `backfill`, optionally with `--limit`; (5) requests name
  `bm25-questions`, or the admin makes it the tenant's default. Doctor's `unused` and `unserved`
  lines catch steps 5 and 1 left out.
- `oml-app` gains the `QuestionGenerator` port, the drain and the outage state; `oml-pg` the
  migration, the trigger's kind 3 branch, the retriever and the counts; `oml-domain` the shape,
  genericity and grounding filters as a pure function. No adapter decides what may be ranked
  (ADR-0049 d2).
- Metrics with closed labels, inside ADR-0054 d1's series budget:
  `oml_questions_generated_total{outcome}`, `oml_questions_dropped_total{reason}`,
  `oml_questions_tokens_total{direction}`, and the gauges `oml_questions_backlog{priority}` and
  `oml_questions_generator_up`. Doctor lines are d11's.
- Erasure: ADR-0005 d4's inventory gains `search_question_sets`, `search_questions`,
  `search_question_coverage` and kind 3 rows of `search_postings` and the count tables. Nothing
  leaves the host, so nothing is outside it.
- #176 gains the stored question sets and `questions_seq` as inputs and the three events of d4 as
  changes. #201's acceptance maps to #235 to #237. ADR-0072's #207 consumes #238. *Parked
  2026-11-11 (d13):* #201 and #236 to #238 are closed as not planned and #207 reads no questions;
  these consequences describe the design a reopening starts from.
- The constitution holds, row by row:

    | | R1 | R2 | R3 | R4 |
    |---|---|---|---|---|
    | generation | no agent act; a lead's task after a version becomes served | only served versions, which review made served; none with a finding whose class holds by default; one version's own text in, nothing else | — | reads a version, writes no memory |
    | questions | index data, never a claim | never reviewed, so grounded, filtered and scanned, never served as text and never visible on their own | change a served row's rank, never who reads it; only add to a row's score, so a finding never lowers a row below its own text's score (d4) | follow their version; postings leave with an erase or a regeneration, text with an erase, neither touching a memory's bytes |
    | settings | an admin's act | — | — | journalled, every set change too |

## Rejected alternatives

| Decision | Alternative | Why not |
|---|---|---|
| d1 | **Generate at write, for held versions** | It would expand unreviewed text, against #201's first constraint and R2, and a reviewer would judge a memory whose ranking a model had already shaped. |
| d1 | **Generate inside the promotion's transaction, or hold `FOR SHARE` across the call** | A model call on the commit path, which every commit in a tenant serialises behind (ADR-0052 d5, #100); a slow generator would hold up review, a supersession or an erase. |
| d1 | **Backfill automatically on enable or on a model change** | A silent and possibly paid run over every served version, and an answer that drifts under the tenant while it runs; #201 asks for "backfill on demand, never silent". |
| d1 | **Questions for in-progress memories served to their author** | They are unreviewed (ADR-0030), and the author can find their own text by its own words. |
| d1 | **Prune kind 2 (#104) instead of adding questions** | Not rejected, compared: `bm25-sparse` already clears every quality gate here and fails only latency, so a capped kind 2 may be the cheaper investment. Phase 0 scores it on the same sets with the same latency proxy, and the record stops if it clears the gates inside the budget (d8, d10). |
| d2 | **Questions as claims, reviewed like memories** | Five reviews per memory, for text no reader asked for; #201 calls them index data, not claims. |
| d2 | **Keep a replaced set's postings** | They would rank nothing current and multiply d8's budget by the number of model changes; the replaced set's text, the journal and `questions_seq` let replay explain what changed. |
| d2 | **Delete a replaced version's questions at supersession** | ADR-0052 d7 keeps a replaced version's postings until erase, and replay needs the version's state, not a hole; an erase still removes them. |
| d2 | **The current generator from lead configuration** | Two leads configured differently would write sets from two generators, and a trigger cannot read configuration. |
| d3 | **Append the questions to the text (docTTTTTquery)** | It changes `bm25-postings`' counts and scores for every tenant, cannot be weighted down when expansion hurts a strong retriever (Weller et al.), and cannot say which words came from a question. |
| d3 | **One document per question** | A version gets one chance per question to match, and field length no longer normalises. Naming the matched question needs only the hit's few question rows (d4). |
| d3 | **Questions as kind 2 tokens** | Kind 2 keeps no counts and so cannot prune, which is why `bm25-sparse` misses its latency bar (ADR-0052 d16). |
| d4 | **Two lists fused by convex combination (the first draft, ADR-0052 d13)** | Per-list min-max turns a weak generic match into the list's maximum, so IDF no longer bounds over-matching, and caps a row without questions at `1 − w_q`, so a backlog or a scan finding demotes it. |
| d4 | **Reciprocal rank fusion** | It discards how strongly a question matched and, like any two-list fusion, gives a row present in both lists a lift a row in one cannot get. |
| d4 | **BM25F with one weighted document length (Robertson et al.'s form)** | A row's questions would lengthen it and lower its text's score on every query they miss, which breaks law 1. |
| d4 | **One IDF from n(t) = df_1 + df_3 (round 1)** | About two thirds of a generated question's words are copied from its memory (docTTTTTquery), so a row holding t in both fields counted twice: a word in 6% of texts that generators copy into 5% of sets reached 11%, above `FREQUENT_SHARE`, and was pruned for every row, rows without questions included. The text term now keeps `df_1` and its pruning, and the question increment uses the exact union, `df_1 + novel_rows`. |
| d4 | **One union IDF for both terms, even exact** | Every new word a generator adds anywhere would lower the text term of rows without questions, so a backfill would move their scores; law 1 would hold only against `bm25-questions`' own statistics, not against the default. |
| d3 | **Term frequency as the number of questions holding a word (round 1)** | Five grounded questions repeating one reviewed word gave it g(1 + 5 · w_q) where one text occurrence gives g(1): 1.49 against 1.0 at w_q 0.3, so a "Related:" line could outrank the right memory (d4's worked counterexample). |
| d4 | **Tune b_3 with w_q** | With b_3 above 0 a short, dense set gets B_3 below 1 and raises its own words (at b_3 0.75 and half the mean length, B_3 is 0.625), and law 3's bound would need min B_3 = 1 − b_3. A binary, capped field has little length to normalise. |
| d4 | **A candidate default now** | The default stays model-free at write and at recall until ADR-0049 d9's evidence says otherwise, and this record has none yet. |
| d4 | **Serve questions as their own hits** | A question would then be served as memory text, which d6 forbids. |
| d5 | **HyDE: a hypothetical answer at recall** | A model call per recall, latency (QuOTE reports about 12 times the query time of plain chunks for HyDE on SQuAD) and variance, against #176. |
| d5 | **Two adapters in phase 1** | The endpoint adapter alone brings egress, a person-session gate, an erasure caveat and its own outage modes; the reranker shipped one local adapter (ADR-0053 d3). Phase 0 measures both and phase 1 builds the winner. |
| d5 | **A provider API as the generator** | The server would send approved text to a third party, and erasure cannot reach a provider's copy; the reranker's stance is no network unless the operator says so (ADR-0053 d3). A later amendment may allow it with a per-tenant egress switch. |
| d5 | **Regenerate on recall when the stored set is stale** | A model call on the read path, and an answer that moves with the generator mid-session. |
| d5 | **Beam search for repeatability** | docTTTTTquery measured top-k sampling more effective; stored questions already give repeatability, so the seed is a convenience. |
| d6 | **Hold a question with a finding for a person** | A person would review machine text no reader needs; dropping one question of five costs less than a review. |
| d6 | **Filter by a relevance model (Doc2Query--)** | A second model at write; DeeperImpact found such filtering unhelpful with learned weights. d6's grounding filter keeps Doc2Query--'s motive with a lexical rule and no model, and phase 0 reports what it costs. |
| d6 | **Follow the tenant's effective verdict (ADR-0074 d22) when excluding versions** | A ceiling spares a tenant's readers a held queue; it says nothing about feeding the text to a model that can be talked round, whose questions rank in every reader's recall. |
| d8 | **Gate both arms against the baseline and take the better** | Either test's error passes, so the false-go rate doubles to 0.10; a fixed sequence keeps 0.05 at the same n. |
| d4 | **Complete the best 4k by the combined score only** | Covered rows can crowd an uncovered row out of completion, so a row `bm25-postings` returns first is not returned, against law 1. |
| d6 | **Generate for versions whose hold finding a person approved** | The approval judges the memory for its readers, not as a model's input; a generator is the reader ADR-0074 d2 says can be talked round. |
| d7 | **Hand ADR-0072 the question text** | A selection needs counts, not text; text across workareas would be a second channel for d6's "never served as memory text". |
| d7 | **A coverage count kept by the drain** | Supersession, retirement, erase and rescan drops change state in the trigger, not the drain, so the count would go stale and break d7's invariants. |
| d9 | **Dense question embeddings first (QuOTE)** | A vector index under row security is unmeasured, a model would run at recall to embed the query, and ADR-0048 puts meaning last, after the lexical attempt. |
| d11 | **Park on five failures of any kind (the sparse queue's rule)** | Five failures take about 62 s (2 + 4 + 8 + 16 + 32), so a routine endpoint restart would park every approval made during it, for good. |
| d11 | **Probe an outage with the oldest queue entry** | The oldest entry may be the version that caused the timeout, and an outage counts no attempt, so a version that always times out would hold every tenant's drain on that lead for good. |
| d1 | **Lease 16 entries per pass under one lease** | Sixteen 60 s timeouts outlive a 5-minute lease, a second lead takes the same entries, and both writes pass their checks: a doubled event, `questions_seq` step and token count. |

## Open questions for the operator

Each question below was decided by the operator on 2026-11-10, with the recommended answer.

1. **Which generator ships first.** Phase 0 measures `local` and an LLM. Decided by the operator, 2026-11-10: a local generator first, `local` (an
   in-process T5, no egress, no per-token cost), behind the one adapter phase 1 builds. The LLM
   arm replaces it only by d8's fixed sequence: `local` passes every gate, and the LLM then beats
   `local` by the same one-sided exact McNemar test with the 1.5-point floor on the grown set and
   passes every other gate, and then only as a loopback endpoint. No non-loopback endpoint on the public server
   (memory.doublegate.io).
2. **Versions whose hold finding a person approved**, or whose class a tenant ceiling lowered,
   get no questions (d6), so they are found by
   their text alone, at exactly their `bm25-postings` score (d4's law 1). Decided by the operator, 2026-11-10: yes, as written; they
   are few, the generator is the reader ADR-0074 warns about, and `status` and doctor count them
   (d11).
3. **Run phase 0 now, ahead of #201's "When to try it"?** #201 waits for a labelled real-usage miss
   set and for workareas of hundreds to thousands of memories. Decided by the operator, 2026-11-10: yes, phase 0a first, to
   grow the labelled set to at least 550 queries, then phase 0, offline, changing no server code,
   at one generation run over about 15,000 memories (200 + 5,183 + 3,633 + 5,882) per adapter, time
   boxed by #235. Phases 1 and 2 stay behind phase 0's verdict and ship together.
4. **Correct #201's numbers.** Decided by the operator, 2026-11-10: already corrected, on 2026-11-10: #201's body now gives
   `bm25-postings`' 0.934 success@5 and 0.873 nDCG@10 on the synthetic set as the baseline, with
   its source, names 0.17, 0.73 and 0.97 as `keyword-all`, `keyword-any` and an older hybrid run,
   and corrects its latency and size rows against d8.

## Review log

**Round 1, 2026-11-10.** Two reviews. Information retrieval and security: 1 blocker, 11 majors,
9 minors. Product and operator: 0 blockers, 9 majors, 9 minors. Every blocker and major is taken;
the minors are taken but for moving the literature to its own page (there is no `docs/research/`
here) and shortening the title, which is the record's handle in links already made. What changed:

- **Fusion (IR B1, IR M1, IR m1, IR m2).** Two lists fused by min-max convex combination are
  replaced by BM25F with per-field length normalisation and one IDF from both fields (d4). Three
  laws are stated and proved: a row without questions scores exactly its text's score; a question
  only adds, so a finding never lowers a row below its text's score; and a row's gain from its
  questions is bounded by its words' absolute IDF and `w_q`. Phase 0 tunes `w_q` and `b_3`, with
  `b_3` = 0 as the prior.
- **Ranking bait (IR M2, IR M7).** Grounding (half of each question's content words from the
  memory), at most 8 content words a question and 10 new words a set, a generic-question filter,
  then the scan (d6); an admin can list and drop a version's questions; a ten-memory, three-style
  adversarial suite per adapter (d8). The MCP sentence no longer claims a question says nothing the
  memory does not.
- **Gates (IR M3).** The power computation is shown: the 121-query set detects a 3.3-point gain
  with power 0.26 to 0.37; phase 0a grows a labelled set to at least 420 queries, written outside
  the corpus's model family. Gates are now a one-sided exact McNemar test, a per-conversation
  randomisation test on LoCoMo, BEIR non-inferiority by paired bootstrap, five-fold
  cross-validation, committed per-query results, and partial-coverage arms (d8).
- **Records and schema (IR M4, IR M5, product m3).** Every set change is journalled and bumps
  `questions_seq`, recorded in `recall_events`; a replaced set's text stays until erase; the tenant
  stores `current_generator_id` and a lead with another refuses; sets carry `workarea_id`;
  coverage comes from trigger-kept counts less expired versions, with a trigger-kept per-generator
  table (d2, d7).
- **The drain (IR M6, IR M9, product M4).** No lock or transaction spans the model call; a
  bounded pool; priority and per-tenant turns; latency gated with the drain saturated (d1, d8).
- **Operations (product M1, M2, M3, m1, m2, m4, m7).** An outage pauses the drain and parks
  nothing; parked entries come back after 1, 2 and 4 days; `retry-parked`; dry-run estimates; the
  bill per 1,000 approvals with a worked price line and a token metric; `--limit`, `--stop`,
  `--resume` and a progress readout; the full doctor list, including unused and unserved; a
  "turning it on" order (d11, Consequences).
- **Scope (product M5, M9, IR M10, IR m4).** Phases 1 and 2 ship together and `enable` refuses
  until the build serves the pipeline; one adapter, loopback only if it is the endpoint, egress left
  to its own amendment; a checkpoint register for weights and training-data licences (d5, d10).
- **The competing investment (IR M11, product M6).** A capped `bm25-sparse` is scored in phase 0
  on the same sets and proxy, and the record stops if it clears the gates in budget.
- **ADR-0072 (IR M8, product M7, product M8).** Posting kind 3 is `KIND_QUESTIONS` and never
  memory kind 3; ADR-0072's test 13 and its review-log line now say "memory-kind-3", the one-line
  clarification made in this pull request. d7 names both of ADR-0072's reads, the expired
  subtraction included, and phase 3 is built when #207 is scheduled.
- **Minors.** Generation follows the served state, `same_user` approvals included (IR m5); the
  generator's input is one version's own text, without attachments or sources (IR m6, and the
  visibility note); ADR-0036 is no longer cited, the behaviour under `oml-scan/1.x` is stated, and
  Mansour et al. carry no figure (IR m7); the 100,000-memory bench takes phase 0's postings (IR m8);
  a contamination check (IR m9); five against ten questions, and the cap of five named as a size
  choice (product m6); `generator` left out of each hit (product's note on the hit); the open
  questions sit under the decision table with their recommendations (product m9).

**Round 2, 2026-11-10.** One fresh review (IR method, security, operations): 0 blockers, 6 majors,
8 minors; of round 1's 21 blockers and majors, 15 closed and 6 partly closed. Every major and minor
is taken, and the six partly closed rows close with N1 to N6. What changed:

- **Statistics (N1; round 1 IR M1, IR m8).** n(t) = df_1 + df_3 counted a word twice where a
  question copied it from its own text, so `bm25-questions` pruned and deflated the text's salient
  words for every row. The text term now keeps `bm25-postings`' N, `df_1`, `avg_1` and pruning
  exactly; the question increment is weighted by idf_q over the exact union `df_1 + novel_rows`,
  where kind 3 postings carry a `novel` flag and `search_term_rows` gains `novel_rows` (d3, d4).
  Law 1 now reads against the default: a row without questions has exactly its `bm25-postings`
  score at any coverage, and at 0% coverage the answer is byte-identical; both are phase 2 property
  tests. `search.pruned` is `bm25-postings`' list and `search.pruned_questions` names the
  question-field prunes. #176's page says a set written for one row can move another's rank, never
  its text score.
- **Repetition (N2; round 1 IR M2).** Kind 3 term frequency is binary, one per set; the worked
  counterexample now scores 2.73 I against the right memory's 3.0 I (4.19 I before), and law 3
  reads "worth less than one text occurrence, at an IDF no larger than the text's". A
  near-duplicate filter drops a question overlapping an earlier one by Jaccard 0.5; d6's "Related:"
  sentence is corrected; the adversarial suite gains a fourth style (d3, d4, d6, d8).
- **Power (N3; round 1 IR M3).** The gain is gated on the one-sided exact McNemar test with a
  1.5-point floor, not on a point estimate at δ: by exact enumeration, "gain ≥ 3.3 and p < 0.05" had
  power 0.52 at n 420. The grown set is now at least 550 queries, 0.90 power at δ 3.3 (546 by
  enumeration, 523 by Connor's formula). LoCoMo and BEIR are non-inferiority arms whose margins
  phase 0a sizes to 99% power from measured per-query SDs, capped at 0.02. The joint power is at
  least 0.84 by the union bound, and the false-go rate at most 0.05 (d8).
- **Partial coverage (N4).** Replaced by a pooled displacement bound: at least 300 queries whose
  right memory has no set, at 95% coverage, at most 6 of 300 displaced from the top 5 (Clopper–
  Pearson bound 3.9% below a tolerable 4%), power 0.967 at a true 1%; the hold class is planted
  synthetic findings (d8).
- **The drain (N5; product M1).** A compiled-in canary classes every failure and probes every
  outage, so a version that always times out is parked after five attempts and never stalls the
  drain; entries are leased one at a time, and the write deletes its own leased entry and requires
  one row, so a lapsed lease writes once (d1, d11).
- **Write path (N6; round 1 IR M9).** Set writes take the chain head; the journal append is last,
  and phase 2 gates promotion, supersession and erase p95 with the drain saturated, with 16 sets a
  transaction as the fallback (d1, d8).
- **Minors.** b_3 is fixed at 0 (m1); the per-generator coverage table is labelled gross of expired
  versions (m2); coverage and generator ids stay in `recall_events`, never in an answer (m3); every
  attempt's tokens reach the tenant's totals (m4); the rate throttles the drain, `--limit` is the
  run's total across resumes, `retry-parked` is journalled, and a superseded version's parked entry
  leaves with its served state (m5); the unverified price line is dropped (m6); two arms are
  pre-registered, the rest reported, and the query writer's family differs from the endpoint
  generator's (m7); `search.pruned` is answered (m8).

**Round 3, 2026-11-10, the final round.** One fresh review (IR method, security, operations,
readability): 0 blockers, 4 majors, 7 minors; of round 2's 6 majors, 5 closed and 1 partly closed,
and all 8 minors closed. The neutrality algebra (N1), the once-per-set count (N2) and 550 queries
at 0.902 power by exact enumeration (N3) were rechecked and hold. Every major and minor is taken.
What changed:

- **Completion (R3-M1; round 2 N1).** Step 5 completes only the best 4k candidates by partial
  score, so covered rows could crowd an uncovered row out of completion under pruning, and a row
  `bm25-postings` returns first would not be returned. Step 5 now completes the union of
  `bm25-postings`' window and the combined score's; law 1 and its phase 2 test cover pruning (d4).
- **LoCoMo's margin (R3-M2).** Ten clusters need Student's t: m = 5.083 · σ / √10 gives 0.991
  power where the normal quantiles gave about 0.965 (d8).
- **Two arms (R3-M3).** Gating both arms against the baseline let a false go through at up to
  0.10. The arms are now tested in a fixed sequence, `local` first, the endpoint only against
  `local` by the same paired test; open question 1 no longer rests on a 3.3-point point estimate
  (d8, open question 1).
- **The hold class (R3-M4).** A version is excluded by its class's default verdict, before any
  tenant ceiling or allowlist of ADR-0074 d22, and the question scan ignores both (d6).
- **Minors.** 80% power is reached at 412 queries and, with no losses, at 203 (m1); the `scan`
  path (m2), `match: "all"` (m3) and word forms (m4) are stated; under federation the counts are
  R's, and ADR-0072 d9 names the set-level events (m5); the joint power says what it covers (m6);
  the status line (m7).

**Amendment, 2026-11-11: the operator's decision on phase 0's gate.** 0a's rerun on PR #245 put
d8's joint power at −0.08, every non-inferiority margin at its 0.02 cap, and `bm25-sparse` at
0.956 success@5 on the grown set. The operator decided to redesign the gate around the competitor
OML already serves: d12 asks whether questions beat `bm25-sparse` or add to it on the 687
question-shaped queries, by two exact paired tests at 0.025 each, sized from measured
discordance (2.5 points at 80% power), with non-inferiority on the other 99 queries at a margin
computed from the set (0.073), not capped. The external sets' arms and the displacement arm move
to phase 2; the budgets stay; the 10% label check still holds every figure provisional. If
questions do not clearly win, v0.7.1 recommends `bm25-sparse` for question-style use. Open
question 5 records the decision.

**Outcome, 2026-11-11: phase 0 did not go, and the operator parked question indexing.** The
operator checked the grown set's 10% sample (58 correct, 6 unsure, 3 wrong) and chose to add two
answering memories to one vague query, g0113, which was marked unsure (version 2, PR #245). Phase 0
on that set (PR #252): `bm25-questions` 0.926 against `bm25-sparse`'s 0.958 success@5 on the 687
question-shaped queries (2 wins, 24 losses), and the combination 0.959 (3 wins, 2 losses, p 0.50).
Which model wrote the corpus is unknown, so d8's different-family condition is unverified (d13).
ADR-0072 carries a note at each place its phase 3 names generated questions. The operator decided "Park questions, promote
sparse"; d13 records the evidence and open question 6 the decision.
