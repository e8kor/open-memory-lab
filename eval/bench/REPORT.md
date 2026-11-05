# Recall benchmark: report

Re-measured 2026-11-05 on a release build of `d3fdb72`, the stored-lexeme search after the
review's fixes (ADR-0049 d12: an inflected word's stem counted once, one ancestry read per
workarea, the `bm25` default), against a throwaway PostgreSQL 17.9 on one 8 vCPU / 15 GB host
shared with other build lanes, through the harness in this directory ([README.md](README.md)).
Every memory went in through the HTTP API and was promoted by a reviewer key of another
principal; every query went out through `oml_read` over `/mcp` with `k` 100 and no token budget
unless stated. Metrics are ir_measures'. The committed numbers are in
`results/<suite>/<method>.json` (schema `oml-bench/1`) and render as the API docs' benchmark
pages (`docs/api/benchmarks/`); the first run (`0ff5c9e`) and the run before the review's fixes
(`4a8f5ba`, `9a6a0d0`) are kept under `results/archive/` and shown beside the current one.

## Before and after

nDCG@10, then the median and p95 latency per query at `k` 100, one client:

| pipeline | synthetic | SciFact (5,183 rows) | NFCorpus (3,633) | LoCoMo (5,882, ~590 per query) |
|---|---|---|---|---|
| bm25, first build (`0ff5c9e`) | 0.861, 48 ms | 0.660, 3,703 / 3,865 ms | 0.308, 2,742 ms | 0.499, 150 ms |
| **bm25, now** | **0.874**, 16 / 23 ms | **0.685**, 83 / 113 ms | **0.321**, 69 / 93 ms | **0.503**, 31 / 38 ms |
| bm25-trigram, first build | 0.856, 60 ms | 0.589, 7,995 ms | 0.285, 5,235 ms | 0.471, 224 ms |
| bm25-trigram, now | 0.868, 39 / 51 ms | 0.611, 3,707 / 5,528 ms (100 queries) | 0.267, 2,172 / 3,079 ms (100 queries) | 0.483, 89 / 111 ms |
| keyword-any, now | 0.687, 24 / 51 ms | 0.515, 815 / 1,087 ms | 0.274, 586 / 1,016 ms | 0.345, 61 / 85 ms |
| keyword-all, now | 0.150, 16 ms | 0.013, 629 ms | 0.187, 477 ms | 0.001, 32 ms |
| offline BM25 (bm25s, k1 0.9, b 0.4) | 0.837 | 0.676 | 0.318 | 0.468 |
| BEIR BM25, published | | 0.665 | 0.325 | |

## Integer postings: `bm25-postings` (ADR-0052)

Measured 2026-11-05 on a release build of this branch (`feat/token-postings`), `bm25` and
`bm25-postings` asked one after the other through `oml_read` against the same loaded corpus,
PostgreSQL 17.9, on the same host shared with other build lanes (load average 10 to 29 during
the runs, so the latencies are an upper bound for one client). `bm25-postings` reads `bm25`'s
score from integer postings that row security lets an index serve, leaves out of what it reads the
query forms held by more than 10% of the readable rows (`FREQUENT_SHARE` 0.1) and then the most
frequent kept form while the kept ones sum above 10,000 postings (`POSTING_BUDGET`), and rescores
the best `4 × k` candidates with every form.

| suite | nDCG@10 bm25 / postings | R@100 bm25 / postings | p50 ms bm25 / postings | p95 ms bm25 / postings |
|---|---|---|---|---|
| synthetic (200) | 0.874 / 0.874 | 0.930 / 0.930 | 12.4 / 9.3 | 18.6 / 13.0 |
| SciFact (5,183) | 0.6851 / 0.6851 | 0.928 / 0.928 | 100 / 49.8 | 174 / 91.9 |
| NFCorpus (3,633) | 0.3214 / 0.3209 | 0.247 / 0.245 | 65.6 / 35.6 | 89.3 / 63.7 |
| LoCoMo (5,882, categories 1-4) | 0.5033 / 0.5030 | 0.813 / 0.767 | 25.6 / 14.5 | 36.9 / 24.2 |

- **nDCG@10 moves by at most 0.0005**, within ADR-0052 d9's 0.005 on every set. The recall eval's
  gate passes with `bm25`'s own numbers (success@1 0.826, success@5 0.934;
  `eval/recall/results/served_bm25_postings.json`).
- **What pruning costs is the tail.** LoCoMo's R@100 falls from 0.813 to 0.767: a turn that
  holds only words frequent in its conversation (the speakers' names are) is no longer a
  candidate. The top ten barely move; a recall that pages past the first hundred loses
  some of those turns.
- **About half the latency** at these sizes, through the whole served path; the scale lane
  (`bench/scale/REPORT.md`) measures the part that grows with the rows a key reads.
- **How the constants were chosen.** `prune_sweep.py` asked every query through the adapter's
  `retrieve` at nine pairs of constants (`results/prune-sweep.json`). On the four sets a share of
  0.1 cost at most 0.0005 nDCG@10 and 0.05 cost 0.0019 (NFCorpus); the budget did not bind below
  6,000 rows. The budget was set on the scale lane, where it decides the universe scope's p95.

### Where the candidates come from, and the LoCoMo tail (amended 2026-11-05)

`bm25-postings` takes its candidates from the postings of the query forms it reads; rescoring the
best `4 × k` with every form reorders those candidates and adds none. The first rule left out
every form held by more than 10% of the readable rows whatever the budget, so a row holding only
such forms was never a candidate. LoCoMo shows it: each query reads one conversation of about 590
turns, where most query words are frequent (99% of queries had a word pruned), and R@100 fell from
0.813 to 0.767 while the top ten barely moved. The rule now prunes only once the query's readable
postings pass `POSTING_BUDGET`; below it every form is read and the list is `bm25`'s. Served, k
100, the same build for both pipelines, before (`1c57c49`) and after (`ac57cd8`):

| suite | R@20 bm25 | R@20 postings before → after | R@100 postings before → after | nDCG@10 postings before → after | p95 ms postings before → after |
|---|---:|---|---|---|---|
| SciFact | 0.8656 | 0.8656 → 0.8656 | 0.928 → 0.928 | 0.6851 → 0.6851 | 72.0 → 66.1 |
| NFCorpus | 0.1840 | 0.1836 → 0.1840 | 0.245 → 0.248 | 0.3203 → 0.3208 | 62.7 → 53.0 |
| LoCoMo | 0.6971 | 0.6935 → 0.6971 | 0.767 → 0.813 | 0.5029 → 0.5034 | 29.9 → 37.9 |

After the change all three lists equal `bm25`'s on every measure. R@20 is what a reranker reading
the first 10 to 20 hits sees; it had lost 0.0027 on LoCoMo and 0.0004 on NFCorpus. Raw numbers:
`results/budget-first.json`; the scale lane's latency for the same rule is in
`bench/scale/REPORT.md`.

## Latency at the size an agent asks, and at scale

`bm25`, p50 / p95 per query, one client:

| scope | rows read | `k` 100 | `k` 10 |
|---|---:|---:|---:|
| SciFact, served | 5,183 | 83 / 113 ms | 71 / 99 ms |
| SciFact ×10, served, every row promoted | 51,830 | 604 / 849 ms | 572 / 807 ms |
| SciFact ×10, the `bm25` statement alone, lab, under row security, before the stem fix | 51,830 | 639 / 947 ms | |
| the first build's `bm25` statement alone, lab, no row security, 10 queries | 51,830 | 15,287 / 21,697 ms | |

At 5,000 rows the default meets a 100 ms p95 at `k` 10 on abstract-length rows, the worst case
(a SciFact memory is about 1,400 characters, where OML's claims are a sentence or two; LoCoMo's
turns answer in 31 / 38 ms at `k` 100), and misses it at `k` 100 by 13%. Latency grows with the
readable rows: under row security no text index serves a read (ADR-0034 d13), so every recall
reads the stored lexemes of every row in its scope. The scale lane (`feat/scale-bench`) measured
an every-word match over a stored `simple` vector past 50 ms near 70,000 rows; `bm25` reads more
per row: served over 51,830 abstracts it takes 604 / 849 ms at `k` 100 and 572 / 807 ms at
`k` 10. Stored lexemes moved the curve down by more than an order of magnitude; they did not
flatten it.

## What changed (ADR-0049 d12)

- **Stored lexemes.** Each version's search vector and its length are stored generated columns
  of `memories`, written once; the `bm25` statement reads them and no row's text. On SciFact
  that took `bm25` from 3.7 s to 176 ms per query, and reading each workarea's ancestry once
  per recall took it to 83 ms.
- **Each occurrence counts once**, a word scores its best form, and a query's English stop words
  are dropped when it has other words. That moved SciFact from 0.660 to 0.685 nDCG@10, above
  the published 0.665 and the offline textbook BM25's 0.676; NFCorpus from 0.308 to 0.321
  (offline 0.318); LoCoMo from 0.499 to 0.503. R@100 on LoCoMo fell from 0.829 to 0.813: a
  question made mostly of stop words now ranks on its remaining words. The run before the
  review's fixes still counted an inflected word's stem twice (`runs` outranked `run`); fixing
  it moved NFCorpus from 0.315 to 0.321 and synthetic success@1 from 0.777 to 0.835.
- **`did_you_mean` from a maintained vocabulary, only when fewer than three hits came back.** No
  query reads a row for it; a thin misspelt recall reads the readable workareas' vocabulary.
- **`bm25` is the default**, and the default set (`keyword-any`, `bm25`) needs no extension. On
  the CI recall eval it passes its gate (success@5 0.926, success@1 0.835, median 16.3 ms at 200
  rows, `k` 10 and a 2,000-token budget); on every external set it ranks above `bm25-trigram`,
  which a deployment enables for misspellings.

## Sanity check against published BM25
| | SciFact nDCG@10 | SciFact R@100 | NFCorpus nDCG@10 | NFCorpus R@100 |
|---|---:|---:|---:|---:|
| BEIR BM25 (Anserini, title and text as two fields, k1 0.9, b 0.4) | 0.665 | 0.908 | 0.325 | 0.250 |
| bm25s over OML's memories (one body), k1 0.9, b 0.4 | 0.676 | 0.913 | 0.318 | 0.246 |
| bm25s over OML's memories, k1 1.2, b 0.75 | 0.679 | 0.913 | 0.318 | 0.247 |
| OML `bm25`, served, first build (`0ff5c9e`) | 0.660 | 0.899 | 0.308 | 0.239 |
| OML `bm25`, served, stored lexemes (`4a8f5ba`) | 0.684 | 0.925 | 0.315 | 0.246 |
| OML `bm25`, served, stems counted once (`d3fdb72`) | 0.685 | 0.928 | 0.321 | 0.248 |

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
  measures +0.009 (SciFact) and +0.003 (NFCorpus) against the same textbook reference, and
  0.321 against Pyserini's single-field 0.322 and BEIR's two-field 0.325 on NFCorpus.
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
SciFact's 5,183 rows were written in 64 s (82 a second) and approved in 55 s (94 a second);
NFCorpus's 3,633 in 47 s and 41 s; LoCoMo's 5,882 in 61 s and 60 s; SciFact ×10's 51,830
were written in 22 minutes (39 a second) and approved in 37 minutes (24 a second), with 4,053
retries; approval ran at a few rows a second while tens of thousands were held and sped up as
the queue shrank: `review next` sorts the held
queue on every call, and with 45,000 held six reviewer keys approved 2 to 14 rows a second. The first run wrote LoCoMo at 98 a second and approved it at 103: computing the
vector at write and keeping the vocabulary now costs a few percent of it (LoCoMo written at 96 a
second and approved at 98), where the run before the review's fixes, which computed the vector
twice per write, wrote SciFact at 62 a second. Two findings, now roadmap rows:

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
