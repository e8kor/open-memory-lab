# Learned sparse and dense retrieval beside bm25: offline report

Measured 2026-11-05 against `main` at `33953f5` (v0.5.5). This is the offline experiment that
ADR-0052 (on `design/token-index-r2`) defers kind 2's model to (open question 6), and it answers
whether learned sparse and/or dense hybrid retrieval should be served. No served path ran: the
memories, queries and qrels are the served benchmark's own (`eval/bench/adapters.py`, same seed,
every query), the scorer is `eval/bench/bench.py`'s (ir_measures), and the bm25 leg is the served
`bm25` statement with its scores. Code: `eval/bench/offline/`. Files: `<suite>/<method>.json`
(schema `oml-bench/1`, kind `offline`, latency null, plus `cost`, `vs_bm25` and, for CC, the
`weights`) and [tables.md](tables.md) (every method on every set, LoCoMo and synthetic per
category, gains with significance).

## Verdicts

| method | verdict | nDCG@10 gain over bm25 (synthetic / SciFact / NFCorpus / LoCoMo) | cost on this CPU | fit |
|---|---|---|---|---|
| **bm25 + doc-v3-distill, convex combination** (`cc-bm25+sparse-distill`, weights 0.3 / 0.7) | **serve**, as ADR-0052's `bm25-sparse`, with weight floor 0.1 and `SPARSE_MAX_TOKENS` 128 | +0.038 / +0.045 / +0.031 / +0.049, all p < 0.05 | write: 7.1 abstracts/s, 52 dialogue turns/s (about 19 ms per short memory, 8 threads, fp32); read: 0.04 ms to encode a query, no model | no corpus statistic at all; each hit decomposes into per-token `idf × weight`; Apache-2.0; English only |
| doc-v3-distill alone (`sparse`) | don't serve alone; it is the fused leg | +0.018 (ns) / +0.022 (ns) / +0.025 / +0.030 | as above | as above |
| doc-v3-gte, alone or fused | **don't**, for now | alone +0.017 (ns) / +0.043 / +0.040 / +0.041; fused +0.029 / +0.050 / +0.039 / +0.061 | half distill's write rate (3.3 abstracts/s, 26 turns/s); 338–466 non-zero tokens per document, up to 1,773 | Apache-2.0, but remote code that transformers 5 loads broken (three repairs, below), and a `candle` port would be a new architecture. Fused, it beats distill by 0.005–0.013 on three sets and loses synthetic by 0.009 |
| dense bge-small-en-v1.5, fused (`cc-bm25+bge-small`) | needs more | +0.037 / +0.047 / +0.042 / +0.041 | write: 18 abstracts/s, 159 turns/s; read: one encoder pass per query, 14 / 20 ms p50 / p95 at 4 threads | MIT; cosine needs no corpus statistic; a hit explains nothing beyond "close in meaning". Alone it loses LoCoMo by 0.064 |
| dense snowflake-arctic-embed-s, fused (`cc-bm25+arctic-s`) | needs more: the dense leg to try first | +0.033 / +0.045 / +0.029 / +0.066 | as bge-small | Apache-2.0; the largest single gain on LoCoMo multi-hop (0.288 → 0.403) and synthetic paraphrase (0.495 → 0.711) |
| bm25 + distill + arctic-s, CC 0.2 / 0.4 / 0.4 | needs more: the target once a dense leg is served | +0.047 / +0.063 / +0.041 / +0.076 | the two added | the best headline on SciFact (0.749) and LoCoMo (0.580) |
| RRF, k 60, as the fusion | don't | 0.003–0.048 below CC in 30 of 32 fused cells | none | needs no weight, but CC's single weight tuned on 121 synthetic queries transferred within 0.011 of each set's own best |

"(ns)": a paired sign-flip randomisation test (10,000 draws, two-sided) gives p ≥ 0.05 against
bm25 on that set.

## Headline

nDCG@10 / R@10 / S@1 / S@5. LoCoMo over categories 1–4 (1,535 questions); synthetic success
counts grade 2 and nDCG gains the grade (bench.py's scorer, so bm25 reads 0.874 where
`eval/recall` reports 0.876).

| method | synthetic (121) | SciFact (300) | NFCorpus (323) | LoCoMo (1,535) |
|---|---|---|---|---|
| bm25 (served statement) | 0.874 / 0.910 / 0.835 / 0.934 | 0.685 / 0.822 / 0.540 / 0.783 | 0.322 / 0.155 / 0.446 / 0.656 | 0.503 / 0.635 / 0.383 / 0.628 |
| sparse-distill | 0.892 / 0.939 / 0.818 / 0.975 | 0.708 / 0.833 / 0.597 / 0.783 | 0.346 / 0.165 / 0.471 / 0.659 | 0.533 / 0.669 / 0.414 / 0.655 |
| sparse-gte | 0.891 / 0.940 / 0.810 / 0.950 | 0.728 / 0.831 / 0.637 / 0.793 | 0.362 / 0.174 / 0.501 / 0.672 | 0.544 / 0.684 / 0.411 / 0.685 |
| bge-small | 0.852 / 0.906 / 0.793 / 0.950 | 0.706 / 0.830 / 0.590 / 0.783 | 0.344 / 0.162 / 0.440 / 0.641 | 0.439 / 0.593 / 0.307 / 0.569 |
| arctic-s | 0.873 / 0.899 / 0.826 / 0.950 | 0.696 / 0.815 / 0.593 / 0.760 | 0.330 / 0.156 / 0.483 / 0.635 | 0.508 / 0.664 / 0.372 / 0.651 |
| rrf bm25+sparse-distill | 0.897 / 0.934 / 0.835 / 0.967 | 0.727 / 0.842 / 0.610 / 0.800 | 0.346 / 0.166 / 0.483 / 0.678 | 0.535 / 0.663 / 0.423 / 0.654 |
| **cc bm25+sparse-distill** | **0.912** / 0.942 / 0.868 / 0.984 | **0.730** / 0.856 / 0.610 / 0.803 | **0.353** / 0.168 / 0.495 / 0.662 | **0.552** / 0.685 / 0.431 / 0.677 |
| cc bm25+sparse-gte | 0.903 / 0.928 / 0.860 / 0.967 | 0.735 / 0.859 / 0.617 / 0.817 | 0.361 / 0.171 / 0.508 / 0.684 | 0.565 / 0.711 / 0.434 / 0.698 |
| cc bm25+bge-small | 0.911 / 0.938 / 0.901 / 0.967 | 0.732 / 0.875 / 0.590 / 0.813 | 0.364 / 0.174 / 0.492 / 0.678 | 0.544 / 0.708 / 0.397 / 0.696 |
| cc bm25+arctic-s | 0.907 / 0.926 / 0.860 / 0.967 | 0.731 / 0.859 / 0.617 / 0.810 | 0.351 / 0.165 / 0.505 / 0.681 | 0.570 / 0.722 / 0.430 / 0.722 |
| cc bm25+sparse-distill+arctic-s | 0.921 / 0.950 / 0.860 / 0.975 | 0.749 / 0.881 / 0.633 / 0.827 | 0.363 / 0.172 / 0.511 / 0.684 | 0.580 / 0.728 / 0.445 / 0.720 |
| cc bm25+sparse-distill+bge-small | 0.921 / 0.944 / 0.884 / 0.975 | 0.748 / 0.876 / 0.627 / 0.830 | 0.371 / 0.177 / 0.495 / 0.675 | 0.561 / 0.721 / 0.416 / 0.708 |

Two checks that the numbers measure what they say. The encoders reproduce their cards on BEIR:
distill 0.708 / 0.346 (card 0.708 / 0.345), gte 0.728 / 0.362 (card 0.725 / 0.360), bge-small
0.706 / 0.344 (card 71.28 / 34.3). The bm25 leg reproduces the served run of `d3fdb72` on the
same data: 0.6851 / 0.3216 / 0.5033 against 0.6851 / 0.3214 / 0.5033 (NFCorpus's 0.0002 is
ties broken by fresh uuids).

**LoCoMo by category** (nDCG@10; multi-hop 282, temporal 320, open-domain 92, single-hop 841;
adversarial 446, outside the headline):

| method | multi-hop | temporal | open-domain | single-hop | adversarial |
|---|---:|---:|---:|---:|---:|
| bm25 | 0.288 | 0.594 | 0.229 | 0.571 | 0.562 |
| sparse-distill | 0.332 | 0.637 | 0.289 | 0.588 | 0.393 |
| cc bm25+sparse-distill | 0.348 | 0.646 | 0.307 | 0.612 | 0.463 |
| cc bm25+arctic-s | 0.403 | 0.637 | 0.339 | 0.625 | 0.422 |
| cc bm25+sparse-distill+arctic-s | 0.389 | 0.660 | 0.353 | 0.638 | 0.431 |

Every learned leg loses the adversarial category (bm25 0.562, fused 0.42–0.53). Those questions
presume something the conversation never says, and their evidence turns are the ones that share
the false premise's words, which exact matching finds best.

**Synthetic paraphrase** (nDCG@10, 11 queries): bm25 0.495, sparse-distill 0.632, cc
bm25+sparse-distill 0.592, bge-small 0.674, arctic-s 0.694, cc bm25+arctic-s 0.711, cc
bm25+sparse-distill+arctic-s 0.702. Learned sparse closes about a quarter of the gap to 1.0
and a dense leg about two fifths. Typo queries go the other way: sparse 0.955, dense 0.800–0.877.

## Fusion

- **CC over RRF.** A convex combination of each leg's scores, min-max normalised over that
  query's own top 100, beats RRF (k 60, equal weights) in 30 of 32 fused cells, by up to 0.048
  (LoCoMo, bm25+sparse-gte+bge-small); RRF wins bm25+arctic-s on NFCorpus by 0.003 and
  bm25+gte+arctic-s on SciFact by 0.001. This agrees with Bruch et al. (research note, source 27).
- **The weight transfers.** Tuned on synthetic alone (nDCG@10, grid 0.05 for two legs, 0.1 for
  three) and applied unchanged, each two-leg fusion lands within 0.011 of the best weight for
  that set (the per-set curves are in the raw `oracle.json` and were never used to choose):
  bm25+distill 0.730 against 0.737 on SciFact, 0.353 against 0.353 on NFCorpus, 0.552 against
  0.556 on LoCoMo; the largest loss is bm25+arctic-s on LoCoMo, 0.570 against 0.581. The
  synthetic curve for bm25+distill is flat between bm25 weights 0.15 and 0.35 (0.908–0.912).
- **Normalisation reads only this query's readable candidates**, so a row the key cannot read
  never sets a leg's minimum or maximum; RRF reads only ranks.

## Cost (this CPU)

AMD EPYC Genoa, 8 vCPU, 15 GB, shared with other lanes. `cost.py` on fixed samples, one model at
a time, 1-minute load average 2.5–7.2 while it ran. The full encodes ran under load 14–27 and
recorded 2–3× lower throughput (each result's `cost` field keeps both).

| model | params | write: SciFact abstracts/s (1,500 chars) | write: LoCoMo turns/s (175 chars) | read: query encode p50 / p95 |
|---|---:|---:|---:|---|
| doc-v3-distill | 67M | 7.1 | 52.1 | 0.04 / 0.08 ms (tokenizer + idf lookup) |
| doc-v3-gte | 133M | 3.3 | 25.7 | 0.04 / 0.09 ms |
| bge-small-en-v1.5 | 33M | 18.0 | 159.1 | 24.0 / 34.0 ms at 1 thread, 14.4 / 20.5 at 4, 11.5 / 15.0 at 8 |
| arctic-embed-s | 33M | 18.4 | 159.0 | 24.2 / 33.7 ms at 1 thread, 14.3 / 19.4 at 4, 12.0 / 16.5 at 8 |

fp32 torch, batch 32, documents at 8 threads. Under a load above the core count, 8 OpenMP
threads took a dense query to 320–820 ms p50–p95 where 4 threads held 16 / 22 ms: a served
encoder pins fewer threads than the cores it can count on. Int8 and ONNX exports were not
measured.

Not measured: either leg's read inside Postgres. The sparse leg would score
`sum(q_idf × weight)` over ADR-0052's kind 2 postings (d4) and the dense leg an exact scan over
readable rows; this experiment scored both in numpy, so it says nothing about their SQL latency.

## ADR-0052 open question 6: kind 2's model

- **Where the model comes from.** `opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill`,
  revision `babf71f3c48695e2e53a978208e8aba48335e3c0`, Apache-2.0, DistilBERT-shaped, 67M, with
  the query table `idf.json` in the same repository; paper arXiv:2411.04403. The card names
  neither its training data nor the corpus the IDF table was computed from (not verified).
  doc-v3-gte: revision `1646fef40807937e8e130c66d327a26421c408d5`, Apache-2.0, architecture code
  loaded from `Alibaba-NLP/new-impl`.
- **Weight floor and `SPARSE_MAX_TOKENS`.** Unpruned, a distill document holds 203–239 non-zero
  tokens on average (p95 248–282, max 564 on one LoCoMo turn) whatever its length: a
  120-character memory expands to about 200 tokens. Document weights on SciFact run 0.008 (p5),
  0.126 (median), 0.699 (p99), 1.34 (max). Pruning (`prune.py`; mean nnz / sparse alone / CC
  with bm25, nDCG@10):

  | floor | max | synthetic | SciFact | NFCorpus | LoCoMo |
  |---:|---:|---|---|---|---|
  | 0 | – | 203 / 0.892 / 0.912 | 239 / 0.708 / 0.730 | 217 / 0.346 / 0.353 | 212 / 0.533 / 0.552 |
  | 0.05 | – | 139 / 0.892 / 0.912 | 178 / 0.706 / 0.730 | 163 / 0.346 / 0.352 | 143 / 0.530 / 0.551 |
  | **0.1** | **128** | **96 / 0.890 / 0.909** | **124 / 0.710 / 0.730** | **118 / 0.343 / 0.350** | **97 / 0.530 / 0.550** |
  | 0.2 | – | 51 / 0.889 / 0.907 | 81 / 0.687 / 0.722 | 72 / 0.337 / 0.346 | 48 / 0.520 / 0.544 |
  | 0 | 64 | 64 / 0.890 / 0.908 | 64 / 0.673 / 0.701 | 64 / 0.334 / 0.345 | 64 / 0.521 / 0.542 |
  | 0 | 32 | 32 / 0.868 / 0.899 | 32 / 0.617 / 0.675 | 32 / 0.313 / 0.331 | 32 / 0.495 / 0.527 |

  Floor 0.1 with at most 128 tokens halves the postings (about 100 per short memory, 124 per
  abstract) for at most 0.003 of fused nDCG@10; at 64 tokens SciFact loses 0.029. For ADR-0052's
  table that is about 100 kind 2 rows per memory against about 35 kind 1 rows (LoCoMo's median
  `search_len`, 175-character turns), so kind 2 would hold about three quarters of the postings.
  gte needs floor 0.2 to reach a similar size (72–120 tokens); unpruned it reaches 1,773 tokens
  on one LoCoMo turn (87 turns above 1,000).
- **Reach beyond English** (`reach.py`, one sentence per language; no qrels, so this bounds the
  query side and measures no quality). Both models use `bert-base-uncased`'s 30,522-piece
  vocabulary:

  | language | pieces per word | [UNK] share |
  |---|---:|---:|
  | English | 1.4 | 0.00 |
  | German | 2.8 | 0.00 |
  | Polish | 3.4 | 0.00 |
  | Arabic | 5.1 | 0.00 |
  | Ukrainian | 6.3 | 0.00 |
  | Chinese | – | 0.73 |
  | Japanese | – | 0.31 |

  Latin-script words split into pieces that still match exactly; Cyrillic and Arabic fall to
  near-character pieces; most CJK characters are `[UNK]` and drop out of the query. The model is
  English. A tenant writing in another language keeps `bm25`, and kind 2 is opt-in per tenant or
  per pipeline until a non-English set with qrels says otherwise.

## Fit with OML's constraints

- **No statistics over unreadable rows.** Sparse reads none: the query weights are a table
  shipped with the model and each document's weights are its own, so it is stricter than bm25,
  which counts the readable scope per query. Dense reads none. CC reads only this query's
  readable candidates. The bm25 leg keeps ADR-0048 d3's scoped statistics.
- **Explainability.** A sparse hit decomposes by token. A real case, synthetic paraphrase query
  "how do we undo a bad release", top hit (not relevant): `release` 5.13 × 1.07 = 5.46 (in the
  text), `do` 2.86 × 0.03, `how` 2.76 × 0.01 (expansions the text does not contain), total
  5.56. Most of a document's 200 tokens are such expansions, so an explanation names a token
  absent from the text as the model's expansion; word pieces (`##ing`) read worse than lexemes.
  Dense explains nothing beyond its cosine.
- **Licences.** distill, gte and arctic-embed-s Apache-2.0, bge-small MIT: all allow commercial
  use.
- **No hosted model in the read path.** Sparse needs no model at read; dense needs a local 33M
  encoder per query (12–24 ms p50 here).

## What gte needed under transformers 5.22

It loaded without an error and encoded noise until three repairs, all in `encode.py`:
`lm_head.decoder.weight` is not in the checkpoint and was left randomly initialised instead of
tied to the word embeddings (the load report says `MISSING`); the non-persistent rotary and
position buffers came back uninitialised (an `IndexError` on a position id near 10^18); and the
remote code calls `get_extended_attention_mask`, which transformers 5 removed. With them it
matches its card.

## Method

- **Data.** `adapters.py` as served: SciFact and NFCorpus test (`<title>. <text>` per memory),
  LoCoMo one turn per memory with each conversation its own scope, the 200 / 121 synthetic set.
- **bm25 leg.** `served_bm25.py` loads the memories into a throwaway PostgreSQL 18.6 cluster,
  fills `search_vector` / `search_len` with migration 0020's own functions, and runs `oml-pg`'s
  `bm25_sql()` with the scope reduced to the query's group, keeping each hit's score (the served
  run files keep ranks only).
- **Sparse.** Document: MLM logits, max over tokens, `log(1 + log(1 + relu(x)))`, special tokens
  zeroed, truncation at 512 tokens (the card sets none; a few SciFact abstracts exceed 512).
  Query: the tokenizer's ids, special tokens dropped, each once, weighted by `idf.json`. Score
  `sum(idf(t) × w_doc(t))` over the scope.
- **Dense.** sentence-transformers, CLS pooling, unit vectors, the card's retrieval prompt on
  queries, exact cosine over the scope.
- **Fusion.** Each leg's top 100; RRF k 60; CC weights tuned on synthetic only.
- **Significance.** Paired sign-flip randomisation on per-query nDCG@10 against bm25, 10,000
  draws, two-sided (`vs_bm25` in each file).

Reproduce (the venv, model cache and encodings were deleted afterwards: 1.2 GB, 1.1 GB, 92 MB):

```sh
uv venv /srv/build/sparse-eval/venv
uv pip install --python /srv/build/sparse-eval/venv/bin/python --index-url https://download.pytorch.org/whl/cpu torch==2.15.0
uv pip install --python /srv/build/sparse-eval/venv/bin/python -r eval/bench/offline/requirements.txt
export HF_HOME=<cache> IR_DATASETS_HOME=/srv/build/bench-data/ir_datasets PY=/srv/build/sparse-eval/venv/bin/python
$PY eval/bench/offline/served_bm25.py --dsn "host=127.0.0.1 port=<throwaway> user=postgres" --runs R synthetic scifact nfcorpus locomo
for m in sparse-distill sparse-gte bge-small arctic-s; do
  $PY eval/bench/offline/encode.py --threads 4 --enc E $m synthetic locomo nfcorpus scifact; done
$PY eval/bench/offline/evaluate.py --enc E --runs R --out RAW synthetic scifact nfcorpus locomo
$PY eval/bench/offline/cost.py --enc E sparse-distill sparse-gte bge-small arctic-s
$PY eval/bench/offline/prune.py --enc E --runs R --weights RAW/weights.json --model sparse-distill synthetic scifact nfcorpus locomo
$PY eval/bench/offline/reach.py sparse-distill
$PY eval/bench/offline/export_offline.py --raw RAW --enc E --commit 33953f5 --date 2026-11-05
```

The files under `results/offline/` are not rendered into the API docs' benchmark pages
(`scripts/gen_bench_pages.py` skips them): none of these methods is a served path.

## What moves "needs more" to "serve"

1. Kind 2's read in Postgres under row security at 10,000 and 100,000 rows, floor 0.1 and 128
   tokens, p50 / p95 beside `bm25-postings` (ADR-0052 d4, #103).
2. A `candle` port of distill whose weights match this encoder's to 1e-4 on the synthetic set,
   and its CPU per write on the lead (#104).
3. For dense: an exact cosine scan over readable rows in Postgres (pgvector `halfvec`, no index)
   at the same sizes, and the query encoder pinned to 4 threads under concurrent recalls;
   arctic-s first.
4. A non-English set with qrels before kind 2 is any tenant's default.
