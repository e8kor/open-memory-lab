<!--
id: adr/ADR-0075-each-approved-memory-is-indexed-by-the-questions-it-answers-generated-off-the-request-path-scanned-stored-with-their-generator-and-matched-only-by-an-opt-in-pipeline
tier: engineering
status: proposed
answers: what hypothetical question indexing is for and what the recall eval says the gap is today; what the literature measured for doc2query, docTTTTTquery, HyDE and generated-question indexing since 2024; when questions are generated and for which versions never; the backfill policy; where questions are stored, with which generator, and what happens to them on supersession, retirement, erasure and a model change; how they are indexed and weighted; the opt-in pipeline and what a hit says; where the model runs and when the server reaches the network; how a generated question is scanned and what it may never do; the interface ADR-0072's phase 3 reads; the eval gates, the latency and size budgets and the backfill cost; when dense embeddings of questions are revisited; the phases and their tests; what the operator still has to answer
-->

# ADR-0075 — Each approved memory is indexed by the questions it answers, generated off the request path, scanned, stored with their generator, and matched only by an opt-in pipeline

**Status:** proposed · 2026-11-10

**In short.** When a memory version is approved, a generator writes up to five questions that the
memory answers. The server scans them, stores them with the model and prompt that wrote them, and
indexes them as BM25 postings in a field of their own. A new pipeline, `bm25-questions`, matches a
query against those questions as well as the memory text, and each hit names the question it
matched. Nothing unreviewed is ever expanded. A question changes ranking, never visibility, and is
never served as memory text. Recall makes no model call. Phase 0 is an offline run that decides
whether the server work is worth building.

**Decisions at a glance.**

| | Decision | Phase |
|---|---|---|
| d1 | Questions are generated when a version becomes served, by a queue the lead drains; never for a held, unreviewed or in-progress-only version; backfill only by an admin's act | 1 |
| d2 | One question set per version, stored with its generator; it follows the version's state and leaves only with an erase or a regeneration | 1 |
| d3 | Questions are postings of `kind` 3, one field per version, with their own counts and pruning | 1 |
| d4 | `bm25-questions` fuses `bm25_postings` and `questions` by convex combination; opt-in; a hit names its question | 2 |
| d5 | A `QuestionGenerator` port; off by default; the server reaches the network only when its operator names a remote endpoint and a tenant allows it | 1 |
| d6 | Every question is filtered and scanned, and any finding drops it; a version with a hold finding gets no questions | 1 |
| d7 | ADR-0072 d10 reads per-workarea kind 3 counts and a coverage share, nothing else | 3 |
| d8 | Gates on the synthetic set, LoCoMo and BEIR; a latency budget; a size budget; the backfill's cost stated | 0, 2 |
| d9 | Dense embeddings of questions are deferred until a named trigger fires | — |
| d10 | Four phases, one issue each; phase 0 decides whether the others are built | 0 to 3 |

**Amends:** ADR-0049 (d4: the pipeline table gains `bm25-questions`; d6: `matched` gains
`question` and `question_terms`, and `via` gains `questions`), ADR-0052 (d1 and d3: `kind` gains `3`,
question term frequencies; d7: postings of kind 3 follow the version and leave with an erase or a
regeneration), ADR-0005 (d4: the erasure inventory gains the question tables and kind 3 postings),
ADR-0065 (d13, as ADR-0074 d14 amends it: the recorded rescan walks stored questions)

**Supplies:** ADR-0072 d10 (the generated-questions selection list of its phase 3, #207)

## Context

**What #201 asks.** Recall finds a memory when the question and the memory share intent but few
words. Each approved memory stores a small set of questions it answers. Recall matches the query
against them as well as the text, and a hit says which question it matched. The design constraints
in #201 are taken as given: approved memories only; questions are index data, not claims; stored,
not regenerated; explainable; an opt-in pipeline; lexical first.

**What the eval says the gap is, corrected.** #201 cites 0.17 success@5 for the default. That figure
is `keyword-all`, v0.5.0's every-word recall (ADR-0049 d9's table: 0.17 success@5, 0.83 zero
results). Since 2026-11-05 the default is `bm25-postings` (ADR-0052 d9), and on the synthetic set
(121 queries, [recall-synthetic](../benchmarks/recall-synthetic.md)) it measured success@5 0.934 and
nDCG@10 0.873: 8 of 121 queries miss the top 5. The misses gather in one category. **Paraphrase**,
the vocabulary gap #201 describes, measured nDCG@10 0.50 and success@5 0.73 (8 of 11), the lowest of
the eleven categories. Temporal (0.82), typo (0.82) and multihop (0.91) follow. So the case for
questions is narrower than #201 states. It rests on paraphrased and question-shaped queries, and on
the external sets where the default trails: SciFact nDCG@10 0.685, NFCorpus 0.321, LoCoMo 0.503
([benchmarks](../benchmarks/index.md)).

**Document expansion already exists here, and it is the bar.** Kind 2 postings are a learned sparse
encoder's expansion of each version (`doc-v3-distill`, ADR-0052 d14 to d17). `bm25-sparse` measured
success@5 0.983 on the synthetic set and nDCG@10 0.730, 0.349 and 0.550 on SciFact, NFCorpus and
LoCoMo, above the default on every set. It stays opt-in for one reason: latency. Its two lists took
272 ms p95 at 100,000 memories, universe scope, against 29.2 ms for `bm25_postings`, because kind 2
keeps no counts to prune by and its expansions make a common token's postings many (ADR-0052 d16,
#104). Generated questions are a second kind of expansion. They are lexical, so they keep d5's counts
and d6's pruning, and they can be read and named in an answer. This record therefore has to show
two things: a gain over the default, and a gain at a latency `bm25-sparse` does not reach.

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
  single TPU". And: "copying terms has the effect of term re-weighting while expanding with new terms
  mitigates the vocabulary mismatch problem".
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
  <https://arxiv.org/abs/2301.03266>: "Doc2Query is indeed prone to hallucination"; filtering the
  generated queries "prior to indexing" can "improve the retrieval effectiveness of Doc2Query by up
  to 16%" while "cutting the index size by 33%".
- Mansour, Zhuang, Zuccon and Mackenzie, *Revisiting Document Expansion and Filtering for Effective
  First-Stage Retrieval*, SIGIR 2024 (read from the listing at
  <https://chatpaper.com/chatpaper/paper/65327>, as the arXiv page could not be fetched): a
  reproduction of Doc2Query-- that found filtering harms recall-based metrics on several collections
  and is not helpful when term weights are learned. This record filters by safety and shape (d6),
  not by a relevance model, and measures recall as well as precision (d8).
- Weller et al., *When do Generative Query and Document Expansions Fail?*, Findings of EACL 2024,
  <https://aclanthology.org/2024.findings-eacl.134/>: "there exists a strong negative correlation
  between retriever performance and gains from expansion"; "expansion improves scores for weaker
  models, but generally harms stronger models", since expansions "add additional noise that makes it
  difficult to discern between the top relevant documents". This is the warning d8's no-harm gates
  are built around, and the reason questions get a field of their own with a tuned weight (d3, d4).
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
verdict before it; a question field kept apart from the text is BM25 with a field weight in place of
docTTTTTquery's undelimited append; and the eval has to show that OML's default, already a strong
lexical retriever, is not the "stronger model" Weller et al. found expansion harms.

**The forces.**

- **R2 and R3.** Nothing unreviewed is expanded, and the gate changes visibility, not ranking
  ([ADR-0001](ADR-0001-the-constitution.md)). Questions are written by a model nobody reviews, so
  they may only rank rows a key could already read, and only rows a second identity approved.
- **Repeatability** (#176). The same key, query, approved set and time give the same answer.
  Recall stays model-free, so the generator's output must be stored and named, never recomputed.
- **ADR-0074.** A generator reading an approved memory can be steered by text inside it, and its
  output reaches a model when a hit names its question. Generated text is a write path the scan has
  to cover.
- **Occam** (ADR-0039 d8). No new tool. A pipeline name and one hit field.
- **Row security.** Only leakproof integer equalities reach an index (ADR-0052 d1). Questions
  therefore become integer postings under the same policy, not a text index.

Decided, not built.

## Decision

- **d1 — Questions are generated when a version becomes served, from a queue the lead drains off
  the request path; never for a held, unreviewed or in-progress-only version; and a backfill runs
  only when an admin starts it.**
  - **The trigger.** The `search_postings` trigger (ADR-0052 d8, d15) enqueues a version into
    `search_questions_queue` in the transaction that makes it served: a promotion, or a
    supersession's successor. It enqueues only a hit kind (`memory`, `derived_fact`, as postings do),
    only for a tenant whose `search_questions_state.enabled` is on, and only when the version has no
    set under the tenant's current generator (d2). Nothing is enqueued at write, for a held version,
    for a rejected one, or for a version served only to its author inside an in-progress subtree
    (ADR-0030 d2): those are unreviewed, and #201 says nothing unreviewed is expanded. A version
    that leaves served before it is drained leaves the queue in the same transaction.
  - **The drain** is the sparse queue's shape (ADR-0052 d15), reused rather than redesigned. One
    task per lead takes due entries `FOR UPDATE SKIP LOCKED` under a 5-minute lease. It reads the
    version `FOR SHARE`, calls the generator, filters and scans the output (d6), and writes the set,
    its questions and their kind 3 postings and counts in one transaction. That transaction deletes
    the entry only if its `enqueued_at` is the one read. A failure keeps the entry, names
    `last_error` and retries after `2^attempts` seconds, at most an hour. After 5 failures the entry
    is parked. A version not yet drained is absent from the `questions` list only; `bm25_postings`
    still finds it.
  - **Order.** New approvals are drained before backfill, oldest first per tenant, so a backfill
    never delays a fresh memory's questions.
  - **Backfill** is an admin's act, never the lead's own: `oml admin questions backfill
    [--tenant t] [--stale]`, journalled as `search.questions_backfill_started` with the generator.
    It enqueues the served versions with no set (or, under `--stale`, a set from another generator)
    in resumable steps of 1,000, as `SPARSE_BACKFILL_STEP` does, at most
    `search.questions.rate` versions a minute per lead (default 60, chosen, not measured; #236).
    Turning a tenant on (`oml admin questions enable`) does not backfill by itself; the answer to
    the enable says how many served versions have no set and names the command. A lead never
    regenerates on start or on a model change (d5).
  - The cost: questions arrive after the approval. Between the two a new memory is found by its
    text alone, and an answer can change when its questions land. #176's page lists that (d4).

- **d2 — One question set per version, stored with the generator that wrote it; the set follows its
  version's state and leaves only with an erase or a regeneration.** The migration is
  `search_questions`, numbered by the phase 1 branch when it is built (0040 is the next free number
  today; 0036 to 0039 are reserved). It creates, each tenant-partitioned, `FORCE ROW LEVEL SECURITY`
  under the `tenant_isolation` policy, with partitions made in ADR-0034 d11's owner step beside
  `search_postings`:

    | Table | Holds |
    |---|---|
    | `search_question_generators` | one row per generator: `id`, `adapter`, `model`, `model_digest`, `prompt_version` (`oml-questions/1`), `prompt_sha256`, `max_questions`, the decoding parameters, `created_at`. No tenant data, so it is the one table here that is not partitioned. |
    | `search_question_sets` | `(tenant_id, memory_id)` key: `generator_id`, `generated_at`, `kept` (0 to 5), `dropped` (a list of `{reason, class, rule}`, never the dropped text), `outcome` (`written`, `empty`, `failed`) |
    | `search_questions` | `(tenant_id, memory_id, ordinal)` key: `text`, at most 200 characters |
    | `search_questions_queue` | as `search_sparse_queue`: `enqueued_at`, `attempts`, `next_at`, `last_error`, `parked_at`, `leased_until` |
    | `search_questions_state` | per tenant: `enabled`, `egress` (d5), `backfill_after`, `backfilled_at` |

  A set belongs to one **version**, not to a claim. A supersession serves a new version, which gets
  its own set; the replaced version's kind 3 postings go to state 2 with its kind 1 postings, and
  its question rows stay. Retirement and rejection do the same. So questions are superseded with
  their memory, as #201 asks. A replaced version's questions stay stored until an erase, because
  they are derived from bytes R4 keeps, and ADR-0052 d7 keeps a replaced version's postings for the
  same reason. **An erase** deletes the version's set, questions and kind 3 postings and counts in the
  erase event's transaction; ADR-0005 d4's inventory gains these rows beside "tsvector entries",
  with the same disposition. **A regeneration** (d5) replaces a version's set, questions and kind 3
  postings in one transaction and journals nothing per version; the backfill that ran it is
  journalled. Questions are derived index data, and a model change may discard them. That is the
  one deletion here outside an erase, and it deletes no byte of any memory. The cost: one row per
  question and one per set, about 0.05 GiB at 100,000 versions by the arithmetic in d8.

- **d3 — Questions are postings of `kind` 3, one field per version, with their own counts, their
  own pruning and a weight of their own.** A version's kept questions are tokenised as kind 1 is
  (ADR-0049 d12's `simple` and `english` lexemes, `oml_token_id`, ADR-0052 d2) and written as one
  document per version: one `search_postings` row per distinct lexeme across the set, `weight` its
  term frequency in the set, `doc_len` the set's length, `memory_kind` the version's, and `state` the
  version's, read `FOR SHARE`. The trigger keeps `search_term_rows` and `search_workarea_rows` for
  kind 3 as it keeps them for kind 1 (ADR-0052 d5), so kind 3 has its own document frequencies and
  its own mean field length, summed over the readable nodes at query time, and d6's pruning applies
  to it unchanged. Question words such as "how" and "what" are English stop words and are dropped
  before any of this. The rest are pruned by their readable count like any other word.
  - **One field per version, not one document per question.** A version with five questions would
    otherwise get five chances to match, and the set's length would not normalise.
  - **A field of its own, not docTTTTTquery's append.** Appending would change kind 1's counts and
    the default's scores for every tenant, could not be weighted down when it hurts (Weller et al.),
    and could not say which words came from a question. A field keeps `bm25-postings` byte for byte
    and gives d4 a weight to tune.
  - `kind` 3 is not ADR-0072's "memory kind 3": that is `memory_kind` 3, a descriptor, under kind 1.
    Descriptors get no questions (they are not a hit kind).
  The cost: a third copy of a version's words in postings, budgeted in d8.

- **d4 — `bm25-questions` fuses `bm25_postings` and a `questions` list by convex combination; it is
  opt-in, it needs no model at recall, and each hit names the question it matched.**
  - **The retriever.** `RetrieverKind::Questions` (`questions`) reads kind 3 postings by the served
    index, scope and state alike, and scores BM25 (k1 1.2, b 0.75) with kind 3's readable
    statistics, through ADR-0052 d4's path and d6's pruning. The own held rows contribute nothing,
    since they have no questions (d1). The in-memory store does not serve it, and a composition over
    it that enables the pipeline is refused at start (ADR-0049 d7).
  - **The pipeline.** `bm25-questions` = `bm25_postings` and `questions`, fused by ADR-0052 d13's
    convex combination at weight `w_q` for `questions`. Phase 0 sets `w_q` on a seeded half of the
    synthetic set and reports it on the other half and on the external sets (d8). It enters
    disabled. A deployment lists it in `search.enabled` and a tenant picks it (ADR-0049 d4). Serving
    it reads stored postings only, so a lead serves it with no model file; a lead with no generator
    configured runs no drain, and doctor's `search_questions_undrained` warns when a tenant has
    questions on and no lead drains its queue. This record does not make `bm25-questions` a
    candidate default. The default stays model-free at write as well as at recall (#176), and a
    later record may propose otherwise with ADR-0049 d9's evidence.
  - **What a hit says.** `matched.via` gains `questions`. A hit the `questions` list held gains
    `matched.question`: `{text, ordinal, generator}`, the one stored question of its set that holds
    the most query IDF (kind 3's), ties broken by the lower `ordinal`, read by `(memory_id)` for
    the hits alone. `generator` is `oml-questions/1 · <model>`. `matched.question_terms` lists the
    query words the hit matched only through its questions, so a match by question is never
    presented as a word the memory says. ADR-0052 d17 does the same for `expanded`. `text` is
    escaped on every text surface as ADR-0074 d12 escapes served text.
  - **Repeatability.** For fixed inputs, now including the stored question sets, the answer is
    byte-identical across calls and processes, `recall_id` aside. #176's page gains three things that
    change an answer: a set written for a served version, a set regenerated by a backfill, and a
    question dropped by a rescan (d6). `recall_events.pipeline` already names the pipeline, so
    ADR-0054 d11's replay reruns it over the questions stored at replay time; a replay after a
    regeneration differs, and the replay says which generator it read.

- **d5 — A `QuestionGenerator` port; generation is off by default; the server reaches the network
  only when its operator names a remote endpoint and a tenant's admin allows its text to leave.**
  - **The port.** `oml_app::ports::QuestionGenerator` takes a version's search text (the text kind 1
    reads, after ADR-0056's format adapter), cut at 512 tokens as docTTTTTquery's input was, and
    returns at most `max_questions` strings or a named failure. Each adapter has a `GeneratorId`
    built from its adapter, model, digest, prompt version, prompt SHA-256 and decoding parameters.
    The prompt is compiled in, versioned `oml-questions/<n>`, and frames the memory as quoted data
    to ask questions about, never as instructions.
  - **Two adapters, and phase 0 picks which ships first** (#235): **`local`**, a doc2query-style T5
    checkpoint run in candle inside the lead, pinned by digest and placed by the operator or fetched
    once at start when `fetch = true`, exactly as ADR-0053 d3 places the reranker; and **`endpoint`**,
    an OpenAI-compatible chat endpoint the operator names (`search.questions.endpoint.url`, a key
    from a file). A loopback endpoint (llama.cpp, vLLM or Ollama on the same host) sends nothing off
    the host. A non-loopback one sends approved memory text to whoever runs it. The candidate
    checkpoint's licence is checked against `deny.toml` before it is pinned.
  - **Off by default.** `search.questions.generator` is unset by default. A lead with it unset loads
    no model, opens no connection and drains nothing. Text leaves the host only when both hold: the
    operator configured a non-loopback endpoint, and the tenant's `egress` is on.
    `oml admin questions egress on` is journalled as `search.questions_egress_set` and needs a
    person session, by ADR-0074 d25's mechanism, since it sends the tenant's approved text to a third party. While
    it is off, that tenant's queue entries wait, and doctor's `search_questions_egress` names the
    tenant. Doctor and the configuration page say what an erase cannot reach: text already sent to
    an endpoint.
  - **Decoding.** Top-k sampling, k 10, as docTTTTTquery measured it above beam search, with a seed
    of the first eight bytes of SHA-256 over the version id and the `GeneratorId`. A rerun on the
    same build and CPU then tends to give the same questions. Repeatability never depends on it,
    because the questions are stored.
  - **A model or prompt change** is a new `GeneratorId`. New approvals use it at once. Existing
    sets keep theirs and stay served, so one tenant can hold sets from two generators, each hit
    naming its own. Doctor's `search_questions_stale` counts the sets under a generator other than
    the current one, and only an admin's `backfill --stale` replaces them (d1). A model change is
    never silent and never automatic: every set says which generator wrote it.

- **d6 — Every question is filtered and scanned, and any finding drops it; a version with a hold
  finding gets no questions; a question changes ranking, never visibility, and is never served as
  memory text.**
  - **The input side.** A generator reading an injection can be steered by it. A version with any
    hold-class finding (ADR-0074 d1), approved or dismissed by a person, gets no questions, and its
    queue entry ends `outcome: empty` with the reason `source_finding`. A rescan that adds such a
    finding after the set was written drops the set (below).
  - **Shape filters** run first and drop a question that is longer than 200 characters, spans more
    than one line, does not end in `?`, holds `://`, a backtick fence or a character of ADR-0074 d5's
    hidden-character table, or repeats another question of the set after NFC, case folding and
    whitespace folding. At most `max_questions` (5) are kept, in the generator's order.
  - **The scan.** Each remaining question is scanned by the composed rule set (`oml-scan/1.x` today,
    `/2` under ADR-0074) as one free-text field. **Any finding drops the question**, whatever its
    verdict: refuse, hold or annotate. ADR-0074 d1 refuses only text that no true memory needs, and
    holds the rest for a person, because a memory's author may need those words. A generated
    question has no author who needs it and no reviewer who will read it. So the threshold for a
    question is any finding, and a drop costs one question of five.
  - **Logged, never kept.** A drop increments `oml_questions_dropped_total{reason}`, where reason is
    the shape rule or the scan class. It adds `{reason, class, rule}` to the set's `dropped`, never
    the text, as ADR-0065 d8 keeps findings, and writes one request-log line per set with drops. A
    set whose every question was dropped is `outcome: empty`.
  - **Rescan.** `oml admin rescan --record` (ADR-0074 d14) also walks stored questions and drops,
    with its postings, any question a newer rule set finds something in. It drops every question
    of a version whose source gained a hold finding, and reports both counts. A question is index
    data, so dropping one changes no memory and needs no person.
  - **Visibility.** Kind 3 postings carry the version's `state` and `workarea_id` and are read under
    the core's `CandidateScope` (ADR-0049 d2), so a question ranks only rows the key already reads,
    and only served ones. A question cannot make a row visible, and its counts are the readable rows'
    (ADR-0052 d5).
  - **Never memory text.** A question is served only as `matched.question` on a hit already served
    with its own text. It is never a hit's `text`, never in the session-start hook's frame or a
    brief, never exported, never drained to the lake (ADR-0060), never a source of
    `did_you_mean` (kind 3 lexemes do not enter `search_vocabulary`), and never shown as a field of
    an ADR-0072 descriptor. The MCP instructions add one sentence: a matched question was written by
    a model to index the memory and says nothing the memory does not.
  - **Over-matching.** A memory written to make its generator emit generic questions can try to
    match every query. The bounds are the field weight `w_q`, BM25's IDF (generic words score
    little), the five-question cap, and pruning. d8's adversarial memory measures what is left.

- **d7 — ADR-0072's phase 3 reads per-workarea kind 3 counts and a coverage share, through the
  statistics read it already makes, and nothing else.** ADR-0072 d10 scores a candidate workarea "by
  BM25 over its memories' questions". This record supplies the inputs, and ADR-0072 keeps the
  scoring:
  1. **Counts.** The statistics read over R (ADR-0072 d12) gains `kind = 3` beside `kind = 1`: for
     each candidate s and kept query word w, `df_s(w)` (served versions in s whose question field
     holds w), `rows_s` (served versions in s with a set) and `total_len_s`, from the same
     `search_term_rows` and `search_workarea_rows` by their primary key. It reads no question text
     and no posting.
  2. **Coverage.** `coverage_s` = `rows_s` of kind 3 over `rows_s` of kind 1 for the hit kinds,
     and the generator ids in force in s, from `search_question_sets` grouped by workarea, kept as a
     count row per `(workarea, generator)` by the drain. ADR-0072 needs it because a workarea whose
     backfill has not run would otherwise rank low for want of questions, not for want of answers.
     How #207 uses coverage (a floor, or scoring only fully covered candidates) is ADR-0072's to
     decide on its eval.
  3. **Invariants** this record guarantees and #238 tests: kind 3 counts are the served versions'
     alone; a held, superseded, retired, rejected or erased version contributes nothing; a question
     held only by unreadable rows changes no count; and the counts summed over a workarea equal a
     count of its served versions' kind 3 postings under the policy (ADR-0072 tests 14 and 15).
  4. **Repeatability.** The generator ids read are recorded in `recall_events` when a selection
     used them, beside ADR-0072 d9's fields, so ADR-0054 d11's replay can say what it read.

- **d8 — The gates: a gain on the synthetic set and LoCoMo, no harm on BEIR, a latency budget, a
  size budget, and the backfill's cost stated before it runs.** One query of the synthetic set is
  0.83 points of success@5, and one of a category of 11 is 9.1 points; ADR-0049 d9 treats one query
  as noise, and no margin below is smaller than four queries on the whole set.
  - **Quality, measured offline in phase 0 and served in phase 2,** against the committed baselines:

    | Set | Baseline (`bm25-postings`) | Gate for `bm25-questions` | For reference |
    |---|---|---|---|
    | synthetic, 121 queries | success@5 0.934, nDCG@10 0.873 | success@5 ≥ 0.967 (117 of 121, four queries more); paraphrase success@5 ≥ 0.91 (10 of 11, from 8); no category falls by more than one query | `bm25-sparse` 0.983 / 0.910; `bm25-rerank` 0.950 / 0.906 |
    | LoCoMo, 1,535 | nDCG@10 0.503 | ≥ 0.513 | `bm25-sparse` 0.550 |
    | SciFact, 300 | nDCG@10 0.685 | ≥ 0.680 (no harm) | `bm25-sparse` 0.730; docT5query over BM25 in BEIR +0.010 |
    | NFCorpus, 323 | nDCG@10 0.321 | ≥ 0.316 (no harm) | `bm25-sparse` 0.349; docT5query over BM25 in BEIR +0.003 |
    | adversarial | — | one planted memory with generic questions enters the top 5 of at most one synthetic query it does not answer | — |

    The gain is asked of the synthetic set and LoCoMo, whose queries are questions over short
    memories, the shape #201 targets. Only no harm is asked of the BEIR sets, where BEIR's own
    docT5query gain was 0.003 to 0.010 and Weller et al. predict little for a strong retriever. One
    model wrote the synthetic set's corpus, queries and labels, so a generator from the same family
    could echo its phrasing; LoCoMo's gate guards against a gain that is only that. A pipeline that
    misses a gate is not offered. When phase 0 misses, the record stops (d10).
  - **Real usage.** When ADR-0054 d11's real-usage set holds at least 100 labelled misses (queries
    whose right memory was not in the top 5), the share of them `bm25-questions` brings into the top
    5 is reported beside the gates. It is no gate until then, because no such set exists yet.
  - **Latency.** `bm25-questions` p95 ≤ 50 ms at 100,000 memories, universe scope (ADR-0052 d9's
    bar), and at most 15 ms above `bm25-postings` on the same run. Its cost is one more pruned
    postings read, one rescore and one `(memory_id)` read for the named questions of the returned
    hits. At 1,000,000 it is measured with #103, and `bm25-postings`' own bar there is unresolved
    (ADR-0052 d10).
  - **Size,** budgeted and then measured in phase 1 (#236). Kind 1 measured about 36 postings per
    version, and heap, indexes and counts of about 1.1 GiB per 165,000 memories, about 6.7 GiB per
    1,000,000 by linear extrapolation (ADR-0052 d10). The budget is kind 3 at no more than kind 1:
    about 0.67 GiB at 100,000 memories and 6.7 GiB at 1,000,000, by the same extrapolation.
    #201's "2–4× the postings" assumed raw words. Five questions of about eight content words,
    after stop words and de-duplication, are expected below kind 1's 36 lexemes, and phase 0 counts
    them. The question text is arithmetic, not a measurement: five questions of at most 200
    characters, about 100 bytes each with row overhead, is about 0.05 GiB at 100,000 and 0.5 GiB at
    1,000,000. When a budget is exceeded, `max_questions` falls by amendment.
  - **Backfill cost,** stated as arithmetic until phase 0 measures it. With the compiled prompt at
    about 150 tokens, a memory cut at 512 tokens, and five questions of at most 64 tokens each (320
    out, about 75 typical), the worst case at 100,000 memories is about 66 million tokens in and 32
    million out. A typical case, counting text at four bytes a token as recall budgets do, with the
    synthetic set's 121-character mean memory, is about 18 million in and 7.5 million out. Ten times
    each at 1,000,000. docTTTTTquery's own figure, about 40 TPU hours for 5 queries over 8.8 million
    passages, is the external reference. At the default rate of 60 versions a minute per lead, a
    backfill takes about 28 hours at 100,000 and about 11.6 days at 1,000,000. Phase 0 measures the
    `local` adapter's real rate on the 8 vCPU host and sets the default from it. An `endpoint`
    adapter's money cost is the operator's provider's price and is not stated here.

- **d9 — Dense embeddings of the questions are deferred, and return only when a named trigger
  fires.** They need a vector index, an embedding per question and a model at recall to embed the
  query. Under row security no operator class has yet been shown to serve the serving role. That was
  ADR-0052's lesson for text, and nothing here measures pgvector against it. Revisit when all three
  hold: `bm25-questions` passed d8 and paraphrase success@5 still has two or more misses; on the
  real-usage set, at least half the remaining misses share no kept word with any question of the
  right memory; and a vector index has been measured serving the serving role inside the 50 ms
  budget at 100,000. QuOTE's results are dense, and would be the reference for that record.

- **d10 — Four phases, one issue each in v0.7.1; phase 0 decides whether the rest are built.**

    | Phase | Builds | Issue | Built when | Acceptance (each test red first by breaking its clause) |
    |---|---|---|---|---|
    | 0 | offline generation for the synthetic set, SciFact, NFCorpus and LoCoMo with both adapters; d6's filters and scan; offline scoring with `w_q` tuned on a seeded half; generation rate and postings per memory | #235 | now: it changes no server code | d8's quality gates offline; the adversarial memory; the rate, tokens and kind 3 postings per memory reported; a go or stop verdict on #201, and on stop this record is marked declined and #236 to #238 closed |
    | 1 | migration `search_questions`, the trigger, the queue and drain, the port and phase 0's adapter, d6's filters and scan, kind 3 postings and counts, `oml admin questions`, erase and rescan, doctor and metrics | #236 | after phase 0 says go | an unreviewed, held or in-progress-only version never gets a set; a version with a hold finding gets none; a question with any finding is dropped, counted and recorded without its text; supersession, retirement and rejection flip kind 3 with kind 1; erase deletes set, questions, postings and counts in one transaction; a tenant with questions off enqueues nothing; a generator change marks sets stale and changes none until a backfill; d8's size budget at 100,000 |
    | 2 | `RetrieverKind::Questions`, `bm25-questions`, `matched.question` and `question_terms`, the served eval and the benchmark pages, #176's page | #237 | after phase 1 | d8's gates served; the latency budget; byte-identical answers across calls and two processes for fixed inputs; a question of an unreadable version changes no byte, count or `did_you_mean`; no question text in `text`, the hook, an export or the lake; `did_you_mean` offers no word only questions hold |
    | 3 | d7's kind 3 counts and coverage in ADR-0072's statistics read, recorded in `recall_events` | #238 | after phase 2, before #207 | ADR-0072 tests 14 and 15; the counts law |

## Consequences

- `docs/architecture/retrieval.md` gains a section on question indexing, and the schema page the
  five tables and `kind` 3, when phase 1 is built; `configuration.md` gains
  `search.questions.generator`, `.rate`, `.endpoint.url` and `.fetch`; the HTTP API, MCP reference
  and `openapi.yaml` gain `matched.question`, `matched.question_terms` and `via: questions`, and the
  pipeline table gains `bm25-questions`, in phase 2, not before (ADR-0039 d9).
- `oml-app` gains the `QuestionGenerator` port and the drain; `oml-pg` the migration, the trigger's
  kind 3 branch, the retriever and the counts; `oml-domain` the shape filters as a pure function.
  No adapter decides what may be ranked (ADR-0049 d2).
- Doctor lines: `search_questions_backlog`, `search_questions_failed`, `search_questions_parked`,
  `search_questions_stale`, `search_questions_undrained`, `search_questions_egress`. Metrics with
  closed labels, inside ADR-0054 d1's series budget: `oml_questions_generated_total{outcome}`,
  `oml_questions_dropped_total{reason}` and the gauge `oml_questions_backlog`.
- Erasure: ADR-0005 d4's inventory gains `search_question_sets`, `search_questions` and kind 3 rows
  of `search_postings` and the count tables. Text sent to a non-loopback endpoint is outside it, and
  the egress setting says so when it is turned on.
- #176 gains the stored question sets as an input and the three events of d4 as changes. #201's
  acceptance maps to #235 to #237. ADR-0072's #207 consumes #238.
- The constitution holds, row by row:

    | | R1 | R2 | R3 | R4 |
    |---|---|---|---|---|
    | generation | no agent act; a lead's task after a promotion | only versions a second identity approved; none with a hold finding | — | reads a version, writes no memory |
    | questions | index data, never a claim | never reviewed, so never served as text and never visible on their own | change a served row's rank, never who reads it | follow their version; leave only with an erase or a regeneration, neither touching a memory's bytes |
    | settings | an admin's act; egress a person's | — | — | journalled |

## Rejected alternatives

| Decision | Alternative | Why not |
|---|---|---|
| d1 | **Generate at write, for held versions** | It would expand unreviewed text, against #201's first constraint and R2, and a reviewer would judge a memory whose ranking a model had already shaped. |
| d1 | **Generate inside the promotion's transaction** | A model call on the commit path, which every commit in a tenant serialises behind (ADR-0052 d5, #100); a slow or failed generator would hold up review. |
| d1 | **Backfill automatically on enable or on a model change** | A silent and possibly paid run over every served version, and an answer that drifts under the tenant while it runs; #201 asks for "backfill on demand, never silent". |
| d1 | **Questions for in-progress memories served to their author** | They are unreviewed (ADR-0030), and the author can find their own text by its own words. |
| d2 | **Questions as claims, reviewed like memories** | Five reviews per memory, for text no reader asked for; #201 calls them index data, not claims. |
| d2 | **Keep every regenerated set** | Derived data with no reader; the generator id on each set and the journalled backfill say what changed, and keeping old sets would multiply d8's budget by the number of model changes. |
| d2 | **Delete a replaced version's questions at supersession** | ADR-0052 d7 keeps a replaced version's postings until erase, and as-of reads (ADR-0036) and replay need the version's state, not a hole; an erase still removes them. |
| d3 | **Append the questions to the text (docTTTTTquery)** | It changes `bm25-postings`' counts and scores for every tenant, cannot be weighted down when expansion hurts a strong retriever (Weller et al.), and cannot say which words came from a question. |
| d3 | **One document per question** | A version gets one chance per question to match, and field length no longer normalises. Naming the matched question needs only the hit's few question rows (d4). |
| d3 | **Questions as kind 2 tokens** | Kind 2 keeps no counts and so cannot prune, which is why `bm25-sparse` misses its latency bar (ADR-0052 d16). |
| d4 | **Reciprocal rank fusion** | It discards how strongly a question matched; for `bm25-sparse`, ADR-0052 d13 measured convex combination above RRF in 30 of 32 fused cells offline and on three of four sets served. Phase 0 reports both. |
| d4 | **A candidate default now** | The default stays model-free at write and at recall until ADR-0049 d9's evidence says otherwise, and this record has none yet. |
| d4 | **Serve questions as their own hits** | A question would then be served as memory text, which d6 forbids. |
| d5 | **HyDE: a hypothetical answer at recall** | A model call per recall, latency (QuOTE reports about 12 times the query time of plain chunks for HyDE on SQuAD) and variance, against #176. |
| d5 | **A provider API as the default generator** | The server would send approved text to a third party by default; the reranker's stance is no network unless the operator says so (ADR-0053 d3), and erasure cannot reach a provider's copy. |
| d5 | **Regenerate on recall when the stored set is stale** | A model call on the read path, and an answer that moves with the generator mid-session. |
| d5 | **Beam search for repeatability** | docTTTTTquery measured top-k sampling more effective; stored questions already give repeatability, so the seed is a convenience. |
| d6 | **Hold a question with a finding for a person** | A person would review machine text no reader needs; dropping one question of five costs less than a review. |
| d6 | **Filter by a relevance model (Doc2Query--)** | A second model at write, and Mansour et al. found such filtering harms recall on several collections; d6 filters by safety and shape only. |
| d6 | **Generate for versions whose hold finding a person approved** | The approval judges the memory for its readers, not as a model's input; a generator is the reader ADR-0074 d2 says can be talked round. |
| d7 | **Hand ADR-0072 the question text** | A selection needs counts, not text; text across workareas would be a second channel for d6's "never served as memory text". |
| d9 | **Dense question embeddings first (QuOTE)** | A vector index under row security is unmeasured, a model would run at recall to embed the query, and ADR-0048 puts meaning last, after the lexical attempt. |

## Open questions for the operator

1. **Which generator ships first.** Phase 0 measures both. The recommendation is `local` (an
   in-process T5, no egress, no per-token cost) unless an `endpoint` LLM clears d8's gates by four or
   more further synthetic queries or 0.01 more LoCoMo nDCG@10. Should a non-loopback endpoint be
   allowed at all on the public server (memory.doublegate.io)? The default answer is no: egress
   stays off there.
2. **Versions whose hold finding a person approved** get no questions (d6). Is that acceptable,
   given those memories are then found by their text alone? The recommendation is yes: they are
   few, and the generator is the reader ADR-0074 warns about.
3. **Run phase 0 now, ahead of #201's "When to try it"?** #201 waits for a labelled real-usage miss
   set and for workareas of hundreds to thousands of memories. Phase 0 is offline, changes no server
   code and costs one generation run over about 15,000 memories (200 + 5,183 + 3,633 + 5,882). The recommendation is to run it now
   and leave phases 1 and 2 behind #201's conditions and phase 0's verdict.
4. **Correct #201's numbers.** Its 0.17, 0.73 and 0.97 are `keyword-all`, `keyword-any` and an older
   hybrid run. The record cites today's 0.934 for the default. Should #201's body be edited to match?
