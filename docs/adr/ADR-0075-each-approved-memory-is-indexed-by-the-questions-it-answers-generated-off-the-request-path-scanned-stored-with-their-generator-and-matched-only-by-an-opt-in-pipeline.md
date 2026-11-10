<!--
id: adr/ADR-0075-each-approved-memory-is-indexed-by-the-questions-it-answers-generated-off-the-request-path-scanned-stored-with-their-generator-and-matched-only-by-an-opt-in-pipeline
tier: engineering
status: proposed
answers: what hypothetical question indexing is for and what the recall eval says the gap is today; what the literature measured for doc2query, docTTTTTquery, HyDE, field-weighted BM25 and generated-question indexing since 2024; when questions are generated and for which versions never; the backfill policy and how it is stopped, capped and tracked; where questions are stored, with which generator, how every change to them is journalled, and what happens to them on supersession, retirement, erasure and a model change; how they are indexed and scored with the memory text, and why a row without questions is never pushed below its own text's score; the opt-in pipeline and what a hit says; where the model runs and when the server reaches the network; how a generated question is filtered for grounding and genericity and scanned, and what it may never do; the interface ADR-0072's phase 3 reads; the eval gates, their power, the latency and size budgets, the bill per 1,000 approvals and the backfill cost; how the drain behaves when the generator is down; when dense embeddings of questions are revisited; the phases and their tests; what the operator still has to answer
-->

# ADR-0075 — Each approved memory is indexed by the questions it answers, generated off the request path, scanned, stored with their generator, and matched only by an opt-in pipeline

**Status:** proposed · 2026-11-10 · revised after review round 1 the same day (see the Review log)

**In short.** When a memory version is approved, a generator writes up to five questions that the
memory answers. The server keeps only questions grounded in the memory's own words, scans them,
stores them with the model and prompt that wrote them, and indexes them as a second field of the
version. A new pipeline, `bm25-questions`, scores the memory text and its questions together by
field-weighted BM25 (BM25F), and each hit names the question it matched. Nothing unreviewed is ever
expanded. A question changes ranking, never visibility, is never served as memory text, and can
only add to a row's score: a row without questions scores exactly what its own text earns. Recall
makes no model call. Phase 0 is an offline run, on a labelled set grown large enough to see the
gain, that decides whether the server work is worth building.

**Decisions at a glance.**

| | Decision | Phase |
|---|---|---|
| d1 | Questions are generated when a version becomes served, by a queue the lead drains outside any transaction; never for a held, unreviewed or in-progress-only version; backfill only by an admin's act | 1 |
| d2 | One current question set per version, stored with its generator; every set written, replaced or dropped is journalled and bumps a tenant sequence; a replaced set's text stays until erase | 1 |
| d3 | Questions are postings of kind 3 (`KIND_QUESTIONS`), one field per version, with their own counts; never ADR-0072's memory kind 3 | 1 |
| d4 | `bm25-questions` scores text and questions as two fields of one BM25F document; opt-in; a row without questions scores its text alone; a hit names its question | 1 and 2, one release |
| d5 | A `QuestionGenerator` port with one adapter in phase 1, the local one unless phase 0 says otherwise; off by default; no egress | 1 |
| d6 | A question must be grounded in its memory, specific and clean: grounding, genericity, caps and the scan; any finding drops it; a version with a hold finding gets none | 1 |
| d7 | ADR-0072 d10 reads per-workarea kind 3 counts, through both of its reads, and a coverage share net of expired versions, nothing else | 3 |
| d8 | Gates sized to see the gain (power shown), non-inferiority on BEIR, neutrality at partial coverage, an adversarial suite per adapter, latency under load, size, and the bill | 0 to 2 |
| d9 | Dense embeddings of questions are deferred until a named trigger fires | — |
| d10 | Phase 0 (with 0a, the grown labelled set) decides; phases 1 and 2 ship in one release; phase 3 when #207 is scheduled | 0 to 3 |
| d11 | Operations: an outage pauses the drain and it resumes on its own; parked entries come back with backoff; a backfill is estimated, capped, stoppable and tracked; the bill is metered; doctor names each state | 1 |

**Open questions: recommended answers, pending the operator.** Each is detailed at the end.

| | Question | Recommended, pending operator |
|---|---|---|
| 1 | Which generator ships first? | `local` (an in-process T5), behind the one adapter phase 1 builds. No non-loopback endpoint on memory.doublegate.io. |
| 2 | Exclude approved memories that carry a hold finding? | Yes, with the count visible in `status` and doctor. |
| 3 | Run phase 0 now? | Yes, phase 0a first (grow the labelled set), then phase 0; phases 1 and 2 stay behind its verdict. |
| 4 | Correct #201's 0.17? | Yes: done in #201's body on 2026-11-10, with today's baseline and its source. |

**Amends:** ADR-0049 (d4: the pipeline table gains `bm25-questions`; d6: `matched` gains
`question` and `question_terms`, and `via` gains `questions`), ADR-0052 (d1 and d3: `kind` gains `3`,
question term frequencies; d7: postings of kind 3 follow the version and leave with an erase or a
regeneration), ADR-0005 (d4: the erasure inventory gains the question tables and kind 3 postings),
ADR-0065 (d13, as ADR-0074 d14 amends it: the recorded rescan walks stored questions), ADR-0054
(d2: `recall_events` gains `questions_seq`), and ADR-0072 (phase 2's test 13 and its review log now
read "memory-kind-3 postings", a one-line clarification made in this pull request; posting kind 3
is this record's question field)

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
  what the memory's own text earns (d4's laws).
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
    no lock or transaction spans a model call. One task per lead takes due entries
    `FOR UPDATE SKIP LOCKED` under a 5-minute lease and commits. It then reads the version's search
    text with no lock (d5's input), calls the generator outside any transaction under
    `search.questions.timeout` (default 60 s), and filters and scans the output (d6). Only then does
    it open the write transaction: it takes the version `FOR SHARE`, checks that the version is
    still served and that the entry's `enqueued_at` is the one it read, and writes the set, its
    questions, their kind 3 postings and counts, the journal event and the sequence bump (d2). If
    either check fails, it discards the output and writes nothing. A supersession, retirement or
    erase therefore never waits on a model.
  - **Failures** are of two kinds (d11). A generator that cannot be reached pauses the lead's drain
    and counts against no entry. A failure of one version (unparseable output, a timeout while other
    calls succeed) keeps the entry, names `last_error` and retries after `2^attempts` seconds, at
    most an hour; after 5 such failures the entry is parked, and d11 brings it back. A version not
    yet drained has no question field; `bm25-questions` scores it by its text alone (d4).
  - **Order and fairness.** `priority` 0 (new approvals) is drained before `priority` 1
    (backfill), oldest first per tenant, at most 16 entries per tenant per pass as ADR-0052 d15
    takes them, tenants in turn. So one tenant's backfill never delays another tenant's new
    approvals.
  - **Where the model runs.** The generator runs on its own bounded pool of
    `search.questions.threads` (default 1), apart from the request runtime, as the reranker's pool
    is bounded (ADR-0053). d8's latency gate is measured with the drain saturated.
  - **Backfill** is an admin's act, never the lead's own: `oml admin questions backfill
    [--tenant t] [--stale] [--limit n] [--dry-run]`, with `--stop` and `--resume` (d11). It is
    journalled as `search.questions_backfill_started` with the generator and the estimate. It
    enqueues, at `priority` 1, the served versions with no current set (or, under `--stale`, a set
    from another generator) in resumable steps of 1,000, as `SPARSE_BACKFILL_STEP` does, at most
    `search.questions.rate` versions a minute per lead (default 60, chosen, not measured; phase 0
    measures it, #236). Turning a tenant on (`oml admin questions enable`) does not backfill by
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
    | `search_questions_queue` | as `search_sparse_queue`: `enqueued_at`, `priority` (0 new, 1 backfill), `attempts`, `next_at`, `last_error`, `parked_at`, `park_count`, `leased_until` |
    | `search_questions_state` | per tenant: `enabled`, `current_generator_id`, `questions_seq` (a monotonic counter), `backfill_after`, `backfill_total`, `backfill_limit`, `backfill_stopped_at`, `backfilled_at` |
    | `search_question_coverage` | `(tenant_id, workarea_id, generator_id)` key: `rows`, the served versions in the workarea whose current set came from that generator, kept by the trigger (d7) |

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
    `search_postings` row per distinct lexeme across the set, `weight` the number of kept questions
    holding it (at most 5, so one question repeating a word counts once), `doc_len` the sum of
    weights, `memory_kind` the version's, and `state` the version's, read `FOR SHARE` in d1's write
    transaction. Question words such as "how" and "what" are English stop words and are dropped
    first.
  - **Counts.** The trigger keeps `search_term_rows` and `search_workarea_rows` for kind 3 as it
    keeps them for kind 1 (ADR-0052 d5), so kind 3 has its own document frequencies and its own
    mean field length over the rows that have a set, summed over the readable nodes at query time,
    less expired versions as kind 1's are (d4).
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
    and `df_3(t)` the two kinds' document frequencies of a kept query word t, `avg_1` and `avg_3`
    the mean field lengths (`avg_3` over rows with a set). Then:
    - n(t) = min(N, df_1(t) + df_3(t)), an upper bound on the number of rows holding t in either
      field, so the IDF below is never larger than the true two-field IDF;
    - idf(t) = ln(1 + (N − n(t) + 0.5) / (n(t) + 0.5)), the form `bm25` already computes in
      `oml-pg`, the maximum over a word's forms as there;
    - B_1(d) = 1 − b_1 + b_1 · len_1(d)/avg_1 and B_3(d) = 1 − b_3 + b_3 · len_3(d)/avg_3, each
      field normalised by its own length;
    - x_t(d) = tf_1(t,d)/B_1(d) + w_q · tf_3(t,d)/B_3(d), with the second term 0 for a row without
      a current set;
    - S(d) = Σ_t idf(t) · g(x_t(d)), with g(x) = x(k1 + 1)/(k1 + x).

    k1 is 1.2 and b_1 0.75, as `bm25` today. w_q and b_3 are set in phase 0 by five-fold
    cross-validation on the grown labelled set (d8), with b_3 = 0 as the prior since the field is
    capped. Pruning reads n(t): a word whose n(t) exceeds `FREQUENT_SHARE` of N is dropped, and the
    budget counts both kinds' postings against `POSTING_BUDGET` (ADR-0052 d6). The candidates are
    the rows either kind's postings hold for the kept words, rescored once.
  - **Law 1, neutrality.** If d has no current set, an empty one, or a set holding none of the
    kept query words, then x_t(d) = tf_1(t,d)/B_1(d) for every t and S(d) = S_text(d), the score of
    its text alone under the same statistics. A set's length enters only B_3, which divides only
    tf_3, so a set that misses the query cannot lower its row. This is why each field has its own
    length normalisation: with Robertson et al.'s one weighted document length, five questions
    would lengthen the row and lower its text's score on every query they miss.
  - **Law 2, a question only adds.** g increases, so S(d) ≥ S_text(d) for every row. A scan finding
    that drops a question or a set (d6) returns the row to S_text(d) at most, never below it: the
    gate may take away what unreviewed text added and never what reviewed text earns. A drop also
    moves n(t) by at most one per word for every row alike, as any posting change does. Rows with
    questions can still pass a row without them on a query their questions match; that is the
    feature, and d8 measures its cost to uncovered rows.
  - **Law 3, the over-matching bound.** g is concave with g(0) = 0, so g(a + b) ≤ g(a) + g(b), and
    a row's gain from its questions is
    Δ(d) = S(d) − S_text(d) ≤ Σ_{t in both the query and the set} idf(t) · g(w_q · m_t / B_3(d)),
    where m_t ≤ 5 is the number of questions holding t. With b_3 = 0, a word's gain is at most
    idf(t) · g(5 · w_q), less than idf(t) · (k1 + 1): a question match on t is worth at most what
    5 · w_q occurrences of t in the text would be worth, at t's absolute IDF. A generic word that
    generators emit across a tenant raises df_3 and so n(t), which lowers its IDF for every row,
    and above `FREQUENT_SHARE` of N it is pruned. For example, at w_q 0.3 and b_3 0, g(1.5) is
    1.22; a word held by 5% of rows has idf about ln 20, 3.0, so it adds at most 3.7, below a single
    text match on a word held by 1% of rows (idf about ln 100, 4.6, g(1) = 1). The bound is a test
    (d10); how many ranks it allows on real sets is d8's adversarial suite.
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
    rescan or an admin. Replay across such an event is best-effort, and the page says so.

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
    hold-class finding, approved or dismissed by a person, gets no questions, and its queue entry
    ends `outcome: empty` with the reason `source_finding`. Under `oml-scan/1.x`, the hold class is
    ADR-0065's; under ADR-0074, if accepted, it is d1's. A rescan that adds such a finding after the
    set was written drops the set (below).
  - **Shape** filters run first, in the generator's order, and drop a question that is longer than
    200 characters, spans more than one line, does not end in `?`, holds `://`, a backtick fence or
    a character of the hidden-character table (ADR-0074 d5 if accepted, ADR-0065's escaping
    otherwise), or repeats another question of the set after NFC, case folding and whitespace
    folding.
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
    already rank the memory through kind 1; grounding stops the generator from adding more of them.
  - **The scan.** Each remaining question is scanned by the composed rule set (`oml-scan/1.x`
    today, `/2` under ADR-0074) as one free-text field. **Any finding drops the question**, whatever
    its verdict: refuse, hold or annotate. A memory's author may need such words, so a memory is held
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
  - **Over-matching** is bounded by d4's law 3 (IDF, `w_q`, the per-question count cap and
    pruning), by grounding and the novel-lexeme cap, and by the genericity filter. d8's
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
     `search_workarea_rows` under the same policy; that split is reported, not scored. ADR-0072
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
     replay can say what it read.

- **d8 — The gates: a gain the eval can see, no harm on BEIR, no harm to rows without questions,
  an adversarial suite per adapter, a latency budget under load, a size budget, and the bill
  stated before it is run up.**
  - **Why the first draft's gates could not decide.** On the 121-query set the baseline is 113 of
    121; the draft's gate, four queries more, is 3.3 points. Four wins and no losses give a
    one-sided exact McNemar p of 0.0625 (0.5^4). By exact enumeration of the binomial, a true gain
    of 3.3 points is detected at one-sided α 0.05 with power 0.37 at n 121 if there are no losses,
    and 0.26 if losses run at a third of the wins (wins 5.0%, losses 1.7% of queries). Half the set
    was also to be used for tuning. So the 121-query set is reported, never gated on.
  - **Phase 0a grows the labelled set.** The sample needed for 80% power at one-sided α 0.05,
    for a gain of δ = 3.3 points with wins p10 5.0% and losses p01 1.7%, by Connor's paired-sample
    formula n = (z_α √(p10 + p01) + z_β √(p10 + p01 − δ²))² / δ², with z_α 1.645 and z_β 0.842, is
    (1.645 · 0.259 + 0.842 · 0.257)² / 0.033² ≈ 378; exact enumeration of the one-sided McNemar test
    gives 415. With no losses it is 185 by the formula and 205 exactly. Phase 0a therefore writes at
    least 420 new question-shaped and paraphrased queries over the synthetic corpus, labelled
    against it. They are written by a person or by a model of a different family from the one that
    wrote the corpus, so a generator from the corpus's family cannot win by echoing its phrasing;
    a person checks the labels of a 10% sample. Per-query results of every run are committed.
  - **Tuning.** w_q and b_3 are chosen by five-fold cross-validation on the grown set; every figure
    gated on is out-of-fold.
  - **Quality, measured offline in phase 0 and served in phase 2:**

    | Set | Baseline (`bm25-postings`) | Gate for `bm25-questions` | Test |
    |---|---|---|---|
    | grown set, ≥ 420 queries (0a) | measured in 0a | success@5 gain ≥ 3.3 points, and one-sided exact McNemar p < 0.05 on the discordant pairs | paired, per query |
    | synthetic, 121 queries | success@5 0.934, nDCG@10 0.873 | reported, with no category falling by more than one query | descriptive |
    | LoCoMo, 1,535 queries in 10 conversations | nDCG@10 0.503 | gain ≥ 0.010, and a one-sided paired randomisation test that flips signs per conversation (1,024 assignments), p < 0.05 | clustered by conversation |
    | SciFact, 300 | nDCG@10 0.685 | non-inferior: lower bound of the 95% paired bootstrap interval of the difference above −0.01 | paired bootstrap, 10,000 resamples |
    | NFCorpus, 323 | nDCG@10 0.321 | non-inferior, as SciFact | paired bootstrap |
    | partial coverage | — | at 95% and 80% coverage, uncovered rows chosen at random and from the hold-finding class: queries whose right memory has no set are non-inferior to `bm25-postings` on them (the bootstrap bound above −0.01 nDCG@10) | paired bootstrap |
    | adversarial, per adapter | — | ten planted memories in each of three styles, generic, bait for a named set of ten target queries, and text addressed to the generator: no planted memory enters the top 5 of more than one non-target query, and the target queries' right memories leave the top 5 in at most one of ten, on the grown set and LoCoMo | per style |

    For reference: `bm25-sparse` measured 0.983 / 0.910 on the synthetic set and 0.550 on LoCoMo,
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
    running the same candle T5 phase 1 would ship, and from which it sets the default rate. For
    scale only, at a hosted small model's list price of $0.10 per million tokens in and $0.50 out
    (Claude Haiku 5.5, Anthropic's price list as of 2026-10-06), 1,000 approvals would cost about
    $0.06 typical and $0.23 worst, and a 100,000-memory backfill about $5.60 and $23; no hosted
    endpoint is built here (d5). docTTTTTquery's own figure, about 40 TPU hours for 5 queries over
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
    | 0a | the grown labelled set (≥ 420 queries, d8), its labels and a person's 10% check | #235 | now: it changes no server code | the set committed with its provenance; the power computation rerun on it |
    | 0 | offline generation for the grown set, the synthetic set, SciFact, NFCorpus and LoCoMo with `local` and an LLM; d6's filters and scan; offline BM25F scoring with w_q and b_3 by five-fold cross-validation; the coverage arms; the adversarial suite; the capped-sparse comparison; the latency proxy; rate, tokens and kind 3 postings per memory; the checkpoint register | #235 | after 0a | d8's gates offline; every d8 report; a go or stop verdict on #201, naming the adapter; on stop, or when capped `bm25-sparse` clears the same gates inside the latency budget, this record is marked declined and #236 to #238 closed |
    | 1 | migration `search_questions`, the trigger, the queue and drain, the port and phase 0's one adapter, d6's filters and scan, kind 3 postings and counts, the journal and `questions_seq`, `oml admin questions` (`enable`, `adopt`, `backfill` with `--dry-run`, `--limit`, `--stop`, `--resume`, `status`, `show`, `drop`, `retry-parked`), erase and rescan, d11's outage handling, doctor and metrics | #236 | after phase 0 says go; released with phase 2 | an unreviewed, held or in-progress-only version never gets a set; a version with a hold finding gets none; a question with any finding, or failing grounding, genericity or a cap, is dropped, counted and recorded without its text; the generator's input is that one version's own text; a version superseded or erased during generation gets no set; supersession, retirement and rejection flip kind 3 with kind 1; erase deletes every set, question, posting, count and coverage row in one transaction; every set change writes one journal event and bumps `questions_seq`; a lead whose generator differs from the tenant's drains nothing; a tenant with questions off enqueues nothing; an unreachable generator parks no entry and the drain resumes on its own; a parked entry returns after its backoff; a backfill stops, resumes and honours its limit; d8's size budget at 100,000 |
    | 2 | `RetrieverKind::Questions`, `bm25-questions`, `matched.question` and `question_terms`, `questions_seq` in `recall_events`, the served eval and the benchmark pages, #176's page | #237 | with phase 1, in one release | d8's gates served; the latency budget with the drain saturated; d4's law 1 (a row with no set, an empty set or a set missing the query scores exactly its text's score) and law 2 (dropping a set never lowers a row below that score), as property tests; law 3's bound holds for every row of the eval sets; byte-identical answers across calls and two processes for fixed inputs; a replay across a set change names the change; a question of an unreadable version changes no byte, count or `did_you_mean`; no question text in `text`, the hook, an export or the lake; `did_you_mean` offers no word only questions hold |
    | 3 | d7's kind 3 counts, through both reads, and coverage in ADR-0072's statistics read, recorded in `recall_events` | #238 | when #207 is scheduled, not before | ADR-0072 tests 14 and 15; the counts law, with an expired version contributing nothing |

- **d11 — Operations: an outage pauses the drain and it resumes on its own; parked entries come
  back with backoff; a backfill is estimated, capped, stoppable and tracked; the bill is metered;
  doctor names each state.**
  - **Generator down.** A connect error, a timeout of the probe, a 5xx, a 429 or a model that fails
    to load means the generator is unreachable. The lead then pauses its whole drain, counts no
    attempt against any entry, and probes with one entry after 30 s, doubling to at most 15 minutes,
    with jitter. The first success resumes the drain. Doctor's `search_questions_generator_down`
    names the generator and since when, and the gauge `oml_questions_generator_up` reads 0. An
    outage therefore parks nothing, and every approval made during it gets its questions afterwards.
  - **Parked entries come back.** A parked entry is retried after 1 day, then 2, then 4
    (`park_count`), and after the third return it stays parked until an admin's `oml admin
    questions retry-parked [--tenant t]` or a new version re-enqueues it. An adopted generator change
    also unparks every entry of the tenant, since the failure may have been the old model's.
  - **The estimate first.** `backfill --dry-run`, `enable` and `adopt` print the versions to
    generate, the tokens in and out by d8's arithmetic (typical and worst), and the time at the
    configured rate. `backfill` journals the estimate it started with.
  - **Control.** `--limit n` caps a run at n versions, recorded as `backfill_limit`.
    `backfill --stop` removes the run's untaken `priority` 1 entries, keeps its resume point and
    records `backfill_stopped_at`, journalled as `search.questions_backfill_stopped`; `--resume`
    continues from the resume point. `oml admin questions status [--tenant t]` gives, per tenant,
    the backfill's done and total, the rate measured over the last hour and an ETA, the backlog by
    priority, parked and failed counts, sets per generator, stale sets, and `empty` sets by reason,
    `source_finding` among them, so the versions excluded by open question 2 are counted.
  - **Metering.** `oml_questions_tokens_total{direction}` (`in`, `out`) counts tokens for either
    adapter, and each set stores its own (d2), so a tenant's bill can be summed.
  - **Doctor lines:** `search_questions_backlog`, `search_questions_failed`,
    `search_questions_parked`, `search_questions_stale`, `search_questions_undrained` (questions on
    and no lead drains the tenant), `search_questions_generator_down`,
    `search_questions_generator_mismatch`, `search_questions_backfill` (a run's progress and ETA),
    `search_questions_unused` (generation on and no request or default used `bm25-questions` in 7
    days, so the tenant pays for no reader), and `search_questions_unserved` (a tenant whose
    default is `bm25-questions` with generation off or no sets, so it behaves as `bm25-postings`).

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
  changes. #201's acceptance maps to #235 to #237. ADR-0072's #207 consumes #238.
- The constitution holds, row by row:

    | | R1 | R2 | R3 | R4 |
    |---|---|---|---|---|
    | generation | no agent act; a lead's task after a version becomes served | only served versions, which review made served; none with a hold finding; one version's own text in, nothing else | — | reads a version, writes no memory |
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
| d4 | **A candidate default now** | The default stays model-free at write and at recall until ADR-0049 d9's evidence says otherwise, and this record has none yet. |
| d4 | **Serve questions as their own hits** | A question would then be served as memory text, which d6 forbids. |
| d5 | **HyDE: a hypothetical answer at recall** | A model call per recall, latency (QuOTE reports about 12 times the query time of plain chunks for HyDE on SQuAD) and variance, against #176. |
| d5 | **Two adapters in phase 1** | The endpoint adapter alone brings egress, a person-session gate, an erasure caveat and its own outage modes; the reranker shipped one local adapter (ADR-0053 d3). Phase 0 measures both and phase 1 builds the winner. |
| d5 | **A provider API as the generator** | The server would send approved text to a third party, and erasure cannot reach a provider's copy; the reranker's stance is no network unless the operator says so (ADR-0053 d3). A later amendment may allow it with a per-tenant egress switch. |
| d5 | **Regenerate on recall when the stored set is stale** | A model call on the read path, and an answer that moves with the generator mid-session. |
| d5 | **Beam search for repeatability** | docTTTTTquery measured top-k sampling more effective; stored questions already give repeatability, so the seed is a convenience. |
| d6 | **Hold a question with a finding for a person** | A person would review machine text no reader needs; dropping one question of five costs less than a review. |
| d6 | **Filter by a relevance model (Doc2Query--)** | A second model at write; DeeperImpact found such filtering unhelpful with learned weights. d6's grounding filter keeps Doc2Query--'s motive with a lexical rule and no model, and phase 0 reports what it costs. |
| d6 | **Generate for versions whose hold finding a person approved** | The approval judges the memory for its readers, not as a model's input; a generator is the reader ADR-0074 d2 says can be talked round. |
| d7 | **Hand ADR-0072 the question text** | A selection needs counts, not text; text across workareas would be a second channel for d6's "never served as memory text". |
| d7 | **A coverage count kept by the drain** | Supersession, retirement, erase and rescan drops change state in the trigger, not the drain, so the count would go stale and break d7's invariants. |
| d9 | **Dense question embeddings first (QuOTE)** | A vector index under row security is unmeasured, a model would run at recall to embed the query, and ADR-0048 puts meaning last, after the lexical attempt. |
| d11 | **Park on five failures of any kind (the sparse queue's rule)** | Five failures take about 62 s (2 + 4 + 8 + 16 + 32), so a routine endpoint restart would park every approval made during it, for good. |

## Open questions for the operator

Each answer below is recommended, pending the operator.

1. **Which generator ships first.** Phase 0 measures `local` and an LLM. Recommended: `local` (an
   in-process T5, no egress, no per-token cost), behind the one adapter phase 1 builds, unless the
   LLM clears d8's gates by a further 3.3 points on the grown set or 0.01 more LoCoMo nDCG@10, and
   then only as a loopback endpoint. No non-loopback endpoint on the public server
   (memory.doublegate.io).
2. **Versions whose hold finding a person approved** get no questions (d6), so they are found by
   their text alone, at exactly their text's score (d4's law 1). Recommended: yes, as written; they
   are few, the generator is the reader ADR-0074 warns about, and `status` and doctor count them
   (d11).
3. **Run phase 0 now, ahead of #201's "When to try it"?** #201 waits for a labelled real-usage miss
   set and for workareas of hundreds to thousands of memories. Recommended: yes, phase 0a first, to
   grow the labelled set to at least 420 queries, then phase 0, offline, changing no server code,
   at one generation run over about 15,000 memories (200 + 5,183 + 3,633 + 5,882) per adapter, time
   boxed by #235. Phases 1 and 2 stay behind phase 0's verdict and ship together.
4. **Correct #201's numbers.** Recommended: yes, and done on 2026-11-10: #201's body now gives
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
