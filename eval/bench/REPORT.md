# Recall benchmark: report

Measured 2026-11-04 on a release build of `0ff5c9e` (the search port, ADR-0049) against a
throwaway PostgreSQL 18.6 on one 8 vCPU / 15 GB host shared with other build lanes, through
the harness in this directory ([README.md](README.md)). Every memory went in through the
HTTP API and was promoted by a reviewer key of another principal; every query went out
through `oml_read` over `/mcp` with `k` 100 and no token budget. Metrics are ir_measures'.
The committed numbers are in `results/<suite>/<method>.json` (schema `oml-bench/1`) and
`results/tables.md`.

## What was run

| suite | memories | queries | one memory is | queries asked |
|---|---:|---:|---|---|
| synthetic (eval/recall) | 200 | 121 | a short claim | all |
| BEIR SciFact, test | 5,183 | 300 | `<title>. <abstract>` | all |
| BEIR NFCorpus, test | 3,633 | 323 | `<title>. <abstract>` | all, 3 at a time (its latency is under 3 concurrent clients) |
| LoCoMo (`locomo10.json`) | 5,882 | 1,981 (1,535 in categories 1-4) | one dialogue turn with its session date; ten conversations in ten workareas | all |

Nothing was subsampled. Five of LoCoMo's 1,986 questions have no usable evidence label (four
name none, one names `D30:05`, which is no turn's id) and are not asked.

## Results

nDCG@10 / Success@5 / R@100, then the median latency per query:

| pipeline | synthetic | SciFact | NFCorpus | LoCoMo (cat. 1-4) |
|---|---|---|---|---|
| keyword-all | 0.150 / 0.174 / 0.125, 8 ms | 0.013 / 0.013 / 0.013, 652 ms | 0.188 / 0.365 / 0.092, 495 ms | 0.001 / 0.001 / 0.001, 27 ms |
| keyword-any | 0.682 / 0.802 / 0.841, 26 ms | 0.514 / 0.603 / 0.781, 1,453 ms | 0.275 / 0.560 / 0.208, 639 ms | 0.342 / 0.435 / 0.747, 114 ms |
| **bm25** | **0.861** / 0.909 / 0.942, 48 ms | **0.660** / **0.757** / **0.899**, 3,703 ms | **0.308** / **0.610** / 0.239, 2,742 ms | **0.499** / **0.616** / 0.829, 150 ms |
| bm25-trigram (default) | 0.856 / **0.934** / **0.959**, 60 ms | 0.589 / 0.690 / 0.895, 7,995 ms | 0.285 / 0.585 / **0.264**, 5,235 ms | 0.471 / 0.581 / **0.832**, 224 ms |
| offline BM25 reference (bm25s, k1 0.9, b 0.4) | 0.837 / 0.909 / 0.904 | 0.676 / 0.757 / 0.913 | 0.318 / 0.647 / 0.246 | 0.468 / 0.585 / 0.812 |
| BEIR BM25, published | | 0.665 / – / 0.908 | 0.325 / – / 0.250 | |

Zero-result rate: keyword-all 0.83 (synthetic), 0.99 (SciFact), 0.51 (NFCorpus), 1.00
(LoCoMo); keyword-any 0.03, 0.00, 0.08, 0.00; bm25 0.00, 0.00, 0.05, 0.00; bm25-trigram 0.00
everywhere. The other measures (R@10, RR@10, Success@1, p95, p99) are in `results/`.

LoCoMo by category, nDCG@10 (bm25 / bm25-trigram): single-hop 0.57 / 0.54 (841 questions),
temporal 0.60 / 0.57 (320), multi-hop 0.27 / 0.24 (282), open-domain 0.25 / 0.19 (92), and
adversarial 0.56 / 0.52 (446, outside the headline).

## Sanity check against published BM25

| | SciFact nDCG@10 | SciFact R@100 | NFCorpus nDCG@10 | NFCorpus R@100 |
|---|---:|---:|---:|---:|
| BEIR BM25 (Anserini, title and text as two fields, k1 0.9, b 0.4) | 0.665 | 0.908 | 0.325 | 0.250 |
| bm25s over OML's memories (one body), k1 0.9, b 0.4 | 0.676 | 0.913 | 0.318 | 0.246 |
| bm25s over OML's memories, k1 1.2, b 0.75 | 0.679 | 0.913 | 0.318 | 0.247 |
| OML `bm25`, served | 0.660 | 0.899 | 0.308 | 0.239 |

Source of the published row: Thakur et al., *BEIR*, arXiv:2104.08663, Table 2 (nDCG@10) and
Table 9 (Recall@100); its section 4 names Anserini with "the default Lucene parameters (k=0.9
and b=0.4)" and the title and passage indexed "as separate fields".

OML's served `bm25` lands within 0.005 (SciFact) and 0.017 (NFCorpus) nDCG@10 of the
published BM25, and within 0.011 R@100 on both: the pipeline is a working BM25, and the
harness, which loads and scores through the served path, reproduces a known number. The
offline reference splits the gap in two:

- **The mapping costs nothing measurable.** Joining title and abstract into one body moves
  nDCG@10 by +0.011 (SciFact) and −0.007 (NFCorpus) against Anserini's two fields, about the
  size of the difference between BM25 implementations.
- **k1 and b do not explain the rest**: k1 1.2 / b 0.75 against 0.9 / 0.4 changes the
  reference by at most 0.003.
- **The served implementation is 0.010 to 0.016 below a textbook BM25 on long English
  text.** Reading `bm25_sql` and `oml_search_vector` (migration 0020), the differences are:
  every word is indexed twice (`oml_search_text` appends the identifier-split copy to the
  text, so the smallest tf is 2 and saturation starts later); a word whose English stem
  differs from it is counted under both lexemes, so its weight is about double that of a
  word whose stem is itself; stop words stay in the `simple` lexemes and are scored (with a
  small IDF); a row's length is its count of distinct lexemes rather than of tokens; and the
  stemmer is Snowball's `english_stem`, not Lucene's Porter. On short claims the same choices
  win: `bm25` beats the reference by 0.024 (synthetic) and 0.031 (LoCoMo) nDCG@10, where
  identifier parts and exact forms help; on abstracts they cost about a point and a half.

## What the results say about S1-S3

- **S1 (`keyword-any`) was needed.** `keyword-all`, the recall served up to v0.5.0, finds
  nothing for 99% of SciFact claims and 99.6% of LoCoMo questions: a natural sentence almost
  never has every word in one memory. Any-word matching takes SciFact from 0.013 to 0.514
  nDCG@10 and LoCoMo from 0.001 to 0.342.
- **S2 (`bm25`) is the best pipeline on every independently built set**, by 0.03 to 0.16
  nDCG@10 over `keyword-any`, and it reproduces published BM25 within 0.017.
- **S3 (`bm25-trigram`, the default) is worse than `bm25` on all three external sets**:
  nDCG@10 −0.071 on SciFact, −0.023 on NFCorpus, −0.028 on LoCoMo, and Success@5 −0.067,
  −0.025, −0.036. On SciFact it lowers nDCG@10 for 81 queries and raises it for 39; the two
  lists share 5.9 of their top 10 on average, so fusion pulls in rows the trigram list ranks
  high and BM25 does not. It wins where the synthetic set put it to work (typo, partial) and
  on R@100 (+0.025 NFCorpus, +0.003 LoCoMo), and it removes NFCorpus's zero-result queries
  (single words such as "okra" or "Zoloft" that no relevant abstract contains, so the trigram
  hits for them are not relevant either). The default was chosen on 121 queries one model
  wrote, 11 of them misspelt; on 2,158 queries written by others it costs 2 to 7 points of
  nDCG@10 and roughly doubles the latency. This is a measurement, not a change: the default
  belongs to ADR-0049's gate, which these sets do not feed yet.
- **Latency, not ranking, is the limit S1-S3 leave.** Median per query, one client, by rows
  in the scope: 48 ms at 200 short claims, 150 ms at about 590 dialogue turns (one LoCoMo
  conversation), 3.7 s at 5,183 abstracts (SciFact), 8.0 s for `bm25-trigram` there. Two
  statements account for most of it, measured directly on the loaded database:
  - the `bm25` statement rebuilds `oml_search_vector(claim)` for every row in scope on every
    query, to read term frequencies and lengths; that expression alone took 3.2 s over
    SciFact's scope;
  - `did_you_mean` (ADR-0049 d6) builds the scope's whole vocabulary on every query in which
    no hit holds every word, which is most natural-language queries, under every pipeline;
    it took about 2 s over SciFact's scope under load, and is why `keyword-all` took 652 ms
    to return nothing while its own statement ran in 4.7 ms.

  FR-84 marks 10,000 and 100,000 rows as unmeasured; at 5,000 rows of abstract-length text
  the default pipeline already takes seconds per query.
- **Statistics stay inside the scope.** The synthetic set returned the same metrics with 200
  rows in the tenant and again with 14,898, and no LoCoMo question received a turn from
  another conversation (`foreign_hits` 0 in every run).
- **Adversarial questions look like answerable ones to recall.** LoCoMo's category 5, whose
  premise the conversation never states, finds its evidence turns about as well as
  single-hop questions do (0.56 nDCG@10). Recall gives no sign that nothing answers the
  question; that needs a score threshold or a reranker, and labels that say "no memory
  answers this".

## Loading at scale through the API

One row per request, as the routes take them: 8 writer threads and 6 reviewer keys of
different principals. LoCoMo's 5,882 rows were written in 60 s (98 a second) and approved
in 57 s (103 a second); NFCorpus's 3,133 in 46 s and 28 s. Two findings:

- At 8 concurrent writers, writes and verdicts meet `503 system.unavailable`, "chain head
  contention", marked retryable: 435 retries for LoCoMo, 337 for NFCorpus. The loader retries
  them; the first SciFact load, written before it did, stopped on one after 4,681 approvals
  and was resumed (its committed load log is the resumed pass). The server's request log
  shows 144 store calls per write.
- `review next` serves a key its own claimed row again until it is judged, so one key
  reviews one row at a time; parallel review needs several reviewer keys, as a crew would
  have.

## What the harness needs next (S4-S7)

- **S6, rerankers.** Rerank `bm25`'s top 50 and score with the same harness. BEIR's Table 2
  gives the target on the same sets: BM25 with a cross-encoder reaches 0.688 (SciFact) and
  0.350 (NFCorpus) nDCG@10, against BM25's 0.665 and 0.325. A reranker is also the place for
  an "answers nothing" score that LoCoMo's adversarial category can test.
- **S7, embeddings.** The adapters and qrels are already pipeline-agnostic; a dense or
  hybrid pipeline is one more `--search` value. BEIR's dense rows (TAS-B: 0.643 SciFact,
  0.319 NFCorpus) are the comparison; LoCoMo's multi-hop and open-domain categories (0.25 to
  0.27 nDCG@10 under `bm25`) are where lexical recall is weakest.
- **S4, filters.** None of the current sets asks a filtered question. LoCoMo's session dates
  can become time filters on the temporal category; a kinds filter needs a set with mixed
  kinds, which the synthetic set can grow.
- **Temporal and supersession.** LongMemEval's knowledge-update and temporal-reasoning
  questions (MIT; per-turn `has_answer` labels) on a fixed subset of question haystacks, with
  updated facts written through `/v1/memories/{artifact_id}/supersede`, so recall is scored
  on serving the tip. It is the next adapter; each question brings about 500 turns and its
  own workarea, so a 50-question subset is about 25,000 rows.
- **Unanswerable queries.** LongMemEval's 30 abstention questions (`_abs`) and LoCoMo's
  adversarial category, scored on what recall returns when the answer is absent.
- **Scale and latency.** FiQA (57,638 documents) or TREC-COVID (171,332) for the 10,000 and
  100,000 row latency FR-84 asks for, once the two per-query statements above stop
  rebuilding text vectors over the scope; at today's cost a 57,000-row scope would take tens
  of seconds per query.
- **Short-claim mapping of long documents.** One sentence per memory, the best sentence
  standing for its abstract (MaxP), so BEIR is also measured in OML's own unit.
- **Other languages.** Every set here is English; MIRACL or mMARCO would test the `simple`
  lexemes that ADR-0048 kept for non-English text.

## Limits of this run

- The host was shared: other lanes compiled Rust during the runs, so latencies are an upper
  bound for this hardware, and NFCorpus's are under 3 concurrent clients.
- SciFact's load and four runs took about 76 minutes, over the 45-minute box the task set,
  because `bm25-trigram` ran 300 queries at 8 s each. NFCorpus ran 3 queries at a time to
  stay inside it. No set was subsampled.
- PostgreSQL ran with `fsync` and `synchronous_commit` off, which speeds loading and does not
  change what recall returns.
