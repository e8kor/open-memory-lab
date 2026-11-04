# Recall benchmark: report

Re-measured 2026-11-04 on a release build of the stored-lexeme search (`4a8f5ba`, ADR-0049 d12;
the SciFact ×10 and `k` 10 runs on `9a6a0d0`, which adds one ancestry read per workarea and the `bm25` default) against
a throwaway PostgreSQL 17.9 on one 8 vCPU / 15 GB host shared with other build lanes, through
the harness in this directory ([README.md](README.md)). Every memory went in through the HTTP
API and was promoted by a reviewer key of another principal; every query went out through
`oml_read` over `/mcp` with `k` 100 and no token budget unless stated. Metrics are
ir_measures'. The committed numbers are in `results/<suite>/<method>.json` (schema
`oml-bench/1`) and render as the API docs' benchmark pages (`docs/api/benchmarks/`); the first
run, on `0ff5c9e`, is kept under `results/archive/0ff5c9e/`.

## Before and after

nDCG@10, then the median and p95 latency per query at `k` 100, one client:

| pipeline | synthetic | SciFact (5,183 rows) | NFCorpus (3,633) | LoCoMo (5,882, ~590 per query) |
|---|---|---|---|---|
| bm25, before (`0ff5c9e`) | 0.861, 48 ms | 0.660, 3,703 / 3,865 ms | 0.308, 2,742 ms | 0.499, 150 ms |
| **bm25, after** | 0.859, 23 / 65 ms | **0.684**, 88 / 119 ms (176 / 214 before one ancestry read per workarea) | **0.315**, 132 / 176 ms | **0.501**, 98 / 118 ms |
| bm25-trigram, before | 0.856, 60 ms | 0.589, 7,995 ms | 0.285, 5,235 ms | 0.471, 224 ms |
| bm25-trigram, after | 0.868, 39 / 82 ms | 0.609, 3,806 / 5,484 ms (100 queries) | 0.269, 2,218 / 3,120 ms (100 queries) | 0.483, 146 / 178 ms |
| keyword-any, after | 0.693, 45 / 125 ms | 0.515, 880 / 1,141 ms | 0.273, 647 / 1,115 ms | 0.345, 136 / 165 ms |
| keyword-all, after | 0.150, 16 ms | 0.013, 632 ms | 0.186, 484 ms | 0.001, 31 ms |
| offline BM25 (bm25s, k1 0.9, b 0.4) | 0.837 | 0.676 | 0.318 | 0.468 |
| BEIR BM25, published | | 0.665 | 0.325 | |

## Latency at the size an agent asks, and at scale

`bm25` on `9a6a0d0`, which reads each workarea's ancestry once per recall instead of once per hit
(ADR-0049 d12), p50 / p95 per query, one client:

| scope | rows read | `k` 100 | `k` 10 |
|---|---:|---:|---:|
| SciFact, served | 5,183 | 88 / 119 ms (was 176 / 214 before the ancestry change) | 77 / 108 ms |
| SciFact ×10, served, the rows promoted when the run started | 8,825 | 141 / 183 ms | 123 / 168 ms |
| SciFact ×10, the `bm25` statement alone, lab, under row security | 51,830 | 639 / 947 ms | |
| the first build's `bm25` statement alone, lab, no row security, 10 queries | 51,830 | 15,287 / 21,697 ms | |

The 100 ms p95 target at 5,000 rows is missed by 8% at `k` 10 on abstract-length rows, the worst
case: a SciFact memory is about 1,400 characters, where OML's claims are a sentence or two, and
LoCoMo's turns (about 590 per conversation) answer in 98 / 118 ms at `k` 100 on the build before
the ancestry change. Latency grows with the readable rows: under row security no text index
serves a read (ADR-0034 d13), so every recall reads the stored lexemes of every row in its scope.
The scale lane (`feat/scale-bench`) measured an every-word match over a stored `simple` vector
past 50 ms near 70,000 rows; `bm25` reads more per row. Stored lexemes moved the curve down by
an order of magnitude; they did not flatten it. The served 51,830-row point was not reached:
see loading below.

## What changed (ADR-0049 d12)

- **Stored lexemes.** Each version's search vector and its length are stored generated columns
  of `memories`, written once; the `bm25` statement reads them and no row's text. On SciFact
  that took `bm25` from 3.7 s to 176 ms per query.
- **Each occurrence counts once**, a word scores its best form, and a query's English stop words
  are dropped when it has other words. That moved SciFact from 0.660 to 0.684 nDCG@10, above
  the published 0.665 and the offline textbook BM25's 0.676; NFCorpus from 0.308 to 0.315
  (offline 0.318); LoCoMo from 0.499 to 0.501. R@100 on LoCoMo fell from 0.829 to 0.812: a
  question made mostly of stop words now ranks on its remaining words.
- **`did_you_mean` from a maintained vocabulary, only when fewer than three hits came back.** No
  query reads a row for it; a thin misspelt recall reads the readable workareas' vocabulary.
- **`bm25` is the default.** On the CI recall eval it passes its gate (success@5 0.934,
  success@1 0.777, median 13.8 ms at 200 rows, `k` 10 and a 2,000-token budget); on every
  external set it ranks above `bm25-trigram`, which stays enabled for misspellings.

## Sanity check against published BM25
| | SciFact nDCG@10 | SciFact R@100 | NFCorpus nDCG@10 | NFCorpus R@100 |
|---|---:|---:|---:|---:|
| BEIR BM25 (Anserini, title and text as two fields, k1 0.9, b 0.4) | 0.665 | 0.908 | 0.325 | 0.250 |
| bm25s over OML's memories (one body), k1 0.9, b 0.4 | 0.676 | 0.913 | 0.318 | 0.246 |
| bm25s over OML's memories, k1 1.2, b 0.75 | 0.679 | 0.913 | 0.318 | 0.247 |
| OML `bm25`, served, first build (`0ff5c9e`) | 0.660 | 0.899 | 0.308 | 0.239 |
| OML `bm25`, served, stored lexemes (`4a8f5ba`) | 0.684 | 0.925 | 0.315 | 0.246 |

Source of the published row: Thakur et al., *BEIR*, arXiv:2104.08663, Table 2 (nDCG@10) and
Table 9 (Recall@100); its section 4 names Anserini with "the default Lucene parameters (k=0.9
and b=0.4)" and the title and passage indexed "as separate fields".

The first build's served `bm25` landed within 0.005 (SciFact) and 0.017 (NFCorpus) nDCG@10 of
the published BM25, and within 0.011 R@100 on both: the pipeline is a working BM25, and the
harness, which loads and scores through the served path, reproduces a known number. The
offline reference splits the gap in two:

- **The mapping costs nothing measurable.** Joining title and abstract into one body moves
  nDCG@10 by +0.011 (SciFact) and −0.007 (NFCorpus) against Anserini's two fields, about the
  size of the difference between BM25 implementations.
- **k1 and b do not explain the rest**: k1 1.2 / b 0.75 against 0.9 / 0.4 changes the
  reference by at most 0.003.
- **The first build's served implementation was 0.010 to 0.016 below a textbook BM25 on long
  English text; the stored-lexeme build removed the causes it could** (ADR-0049 d12) and now
  measures +0.008 (SciFact) and −0.003 (NFCorpus) against the same textbook reference.
  Reading `bm25_sql` and `oml_search_vector` (migration 0020), the differences are:
  every word is indexed twice (`oml_search_text` appends the identifier-split copy to the
  text, so the smallest tf is 2 and saturation starts later); a word whose English stem
  differs from it is counted under both lexemes, so its weight is about double that of a
  word whose stem is itself; stop words stay in the `simple` lexemes and are scored (with a
  small IDF); a row's length is its count of distinct lexemes rather than of tokens; and the
  stemmer is Snowball's `english_stem`, not Lucene's Porter. On short claims the same choices
  win: `bm25` beats the reference by 0.024 (synthetic) and 0.031 (LoCoMo) nDCG@10, where
  identifier parts and exact forms help; on abstracts they cost about a point and a half.

## The first run's findings (`0ff5c9e`)

Kept as measured; the latency findings below are what ADR-0049 d12 fixed, and the numbers above
supersede them.

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
different principals. With the stored lexemes and the vocabulary trigger (`results/load-*.json`):
SciFact's 5,183 rows were written in 84 s (62 a second) and approved in 67 s (77 a second);
NFCorpus's 3,633 in 60 s and 45 s; LoCoMo's 5,882 in 66 s and 62 s; SciFact ×10's 51,830
were written in about 25 minutes, 48 a second at first and 25 by the end, and 8,825 of them
were approved in about 50 minutes before the load was stopped: `review next` sorts the held
queue on every call, and with 45,000 held six reviewer keys approved 2 to 14 rows a second. The first run wrote LoCoMo at 98 a second and approved it at 103: computing the
vector at write and keeping the vocabulary costs roughly a third of the write rate on these
sets. Two findings, now roadmap rows:

- At 8 concurrent writers, writes and verdicts meet `503 system.unavailable`, "chain head
  contention", marked retryable: 534 to 775 retries per 5,000-row set. The loader retries them.
  The server's request log showed 144 store calls per write (FR-86).
- `review next` serves a key its own claimed row again until it is judged, so one key
  reviews one row at a time; parallel review needs several reviewer keys, as a crew would
  have (FR-87).

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
- **Scale and latency.** SciFact ×10 gives one point at 51,830 rows; FiQA (57,638 documents)
  or TREC-COVID (171,332) would give a natural corpus at that size and above. Under row
  security no text index serves a read (ADR-0034 d13), so every pipeline's cost grows with the
  readable rows: the scale lane's measurement (`feat/scale-bench`) puts an AND over a stored
  `simple` vector past 50 ms near 70,000 rows, and `bm25` is several times that statement's
  cost. Latency is not solved beyond a few thousand rows of abstract-length text.
- **Short-claim mapping of long documents.** One sentence per memory, the best sentence
  standing for its abstract (MaxP), so BEIR is also measured in OML's own unit.
- **Other languages.** Every set here is English; MIRACL or mMARCO would test the `simple`
  lexemes that ADR-0048 kept for non-English text.

## Limits of this run

- The host was shared with other lanes, so latencies are an upper bound for this hardware.
  Every query ran one at a time.
- `bm25-trigram` asked a seeded 100 of SciFact's and NFCorpus's queries and 30 of SciFact ×10's
  (seed 20261104), because its trigram list costs seconds per query there; every other method
  asked every query.
- SciFact ×10's quality is not comparable to SciFact's: its qrels name only the original copy,
  so nine near duplicates compete with every relevant abstract. It is there for latency.
- PostgreSQL 17.9 ran with `fsync` off, which speeds loading and does not change what recall
  returns.
