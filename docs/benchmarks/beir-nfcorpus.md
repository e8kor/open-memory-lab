---
status: Measured
verified: 2026-11-06
answers: "what each search pipeline measured on BEIR NFCorpus: quality, per-category results, latency, the reference numbers and the limits of the run"
---

# BEIR NFCorpus

*These are measurements by the OML maintainers on their own hardware, not independent results. Generated from the committed `oml-bench/1` results.*

Suite `beir-nfcorpus`, data `beir/nfcorpus/test (ir_datasets 0.6.3)`: 3,633 memories, 323 queries; a method marked sampled asked a seeded subset, named in its row.

??? note "Run details"

    Measured 2026-11-05 on commits `2123e90`, `66979b4`, `a1ad0c9`, `d3fdb72`, `f86c31c`; build release, host 8 vCPU / 15 GB, postgres 17.9; build release, host 8 vCPU / 15 GB, postgres 18.6; build release, host 8 vCPU / 15 GB, load average 3.6 to 14 from other lanes, postgres 18.6; build release, host 8 vCPU / 15 GB, shared, load 8-12, postgres 18.

**How to reproduce.** Build `oml` and `oml-server-lead` in release mode, install `eval/bench/requirements.txt` in a scratch virtual environment, then run `eval/bench/run.sh <out-dir> nfcorpus` against a throwaway PostgreSQL superuser cluster. The harness loads the data over HTTP, promotes it with reviewer keys, asks every query through `oml_read` and writes the result files listed below.

[All suites](index.md)

## Quality

| method | queries | nDCG@10 | R@10 | R@20 | R@100 | RR@10 | S@1 | S@5 | zero |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| keyword-all | 323 | 0.187 | 0.083 | – | 0.092 | 0.317 | 0.282 | 0.362 | 0.511 |
| keyword-any | 323 | 0.274 | 0.130 | – | 0.210 | 0.474 | 0.406 | 0.560 | 0.081 |
| bm25 | 323 | 0.321 | 0.155 | 0.184 | 0.247 | 0.526 | 0.440 | 0.656 | 0.046 |
| bm25-trigram | 100 (sampled) | 0.267 | 0.147 | – | 0.245 | 0.482 | 0.380 | 0.600 | 0.000 |
| bm25-postings | 323 | 0.321 | 0.155 | 0.184 | 0.247 | 0.526 | 0.440 | 0.656 | 0.046 |
| sparse | 323 | 0.343 | 0.164 | 0.191 | 0.276 | 0.544 | 0.464 | 0.656 | 0.009 |
| bm25-sparse | 323 | 0.349 | 0.166 | 0.196 | 0.277 | 0.565 | 0.483 | 0.662 | 0.009 |
| bm25-sparse-rrf | 323 | 0.344 | 0.166 | 0.194 | 0.272 | 0.557 | 0.461 | 0.678 | 0.009 |
| bm25-rerank-d20 | 100 (sampled) | 0.330 | 0.162 | – | 0.243 | 0.543 | 0.470 | 0.630 | 0.040 |
| bm25-sampled | 100 (sampled) | 0.320 | 0.152 | – | 0.243 | 0.550 | 0.480 | 0.630 | 0.040 |
| reference-bm25s-k1-0.9-b-0.4 | 323 | 0.318 | 0.145 | – | 0.246 | 0.522 | 0.440 | 0.647 | 0.046 |
| reference-bm25s-k1-1.2-b-0.75 | 323 | 0.318 | 0.145 | – | 0.247 | 0.523 | 0.436 | 0.644 | 0.046 |

Reference: BEIR BM25 (Anserini, k1 0.9, b 0.4, title and text fields), ndcg@10 0.325 ([source](https://arxiv.org/abs/2104.08663)).

??? note "Chart: nDCG@10 by pipeline"

    <svg role="img" aria-label="nDCG@10 on BEIR NFCorpus" viewBox="0 0 680 362" width="100%" style="max-width:680px" xmlns="http://www.w3.org/2000/svg">
    <title>nDCG@10 on BEIR NFCorpus</title>
    <text x="0" y="16" font-size="13" fill="currentColor">nDCG@10 on BEIR NFCorpus</text>
    <text x="232" y="41" font-size="11" text-anchor="end" fill="currentColor">keyword-all</text>
    <rect x="240" y="28" width="198.3" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="444.3" y="41" font-size="12" fill="currentColor">0.187</text>
    <text x="232" y="67" font-size="11" text-anchor="end" fill="currentColor">keyword-any</text>
    <rect x="240" y="54" width="290.3" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="536.3" y="67" font-size="12" fill="currentColor">0.274</text>
    <text x="232" y="93" font-size="11" text-anchor="end" fill="currentColor">bm25</text>
    <rect x="240" y="80" width="340.2" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="586.2" y="93" font-size="12" fill="currentColor">0.321</text>
    <text x="232" y="119" font-size="11" text-anchor="end" fill="currentColor">bm25-trigram</text>
    <rect x="240" y="106" width="283.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="529.0" y="119" font-size="12" fill="currentColor">0.267</text>
    <text x="232" y="145" font-size="11" text-anchor="end" fill="currentColor">bm25-postings</text>
    <rect x="240" y="132" width="340.2" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="586.2" y="145" font-size="12" fill="currentColor">0.321</text>
    <text x="232" y="171" font-size="11" text-anchor="end" fill="currentColor">sparse</text>
    <rect x="240" y="158" width="363.5" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="609.5" y="171" font-size="12" fill="currentColor">0.343</text>
    <text x="232" y="197" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse</text>
    <rect x="240" y="184" width="370.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="616.0" y="197" font-size="12" fill="currentColor">0.349</text>
    <text x="232" y="223" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse-rrf</text>
    <rect x="240" y="210" width="365.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="611.1" y="223" font-size="12" fill="currentColor">0.344</text>
    <text x="232" y="249" font-size="11" text-anchor="end" fill="currentColor">bm25-rerank-d20</text>
    <rect x="240" y="236" width="349.9" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="595.9" y="249" font-size="12" fill="currentColor">0.330</text>
    <text x="232" y="275" font-size="11" text-anchor="end" fill="currentColor">bm25-sampled</text>
    <rect x="240" y="262" width="339.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="585.6" y="275" font-size="12" fill="currentColor">0.320</text>
    <text x="232" y="301" font-size="11" text-anchor="end" fill="currentColor">reference-bm25s-k1-0.9-b-0.4</text>
    <rect x="240" y="288" width="337.2" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="583.2" y="301" font-size="12" fill="currentColor">0.318</text>
    <text x="232" y="327" font-size="11" text-anchor="end" fill="currentColor">reference-bm25s-k1-1.2-b-0.75</text>
    <rect x="240" y="314" width="337.5" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="583.5" y="327" font-size="12" fill="currentColor">0.318</text>
    <line x1="584.7" y1="24" x2="584.7" y2="340" stroke="currentColor" stroke-dasharray="4 3"/>
    <text x="584.7" y="354" font-size="11" text-anchor="middle" fill="currentColor">published 0.325</text>
    </svg>

## Latency

Client wall time per recall, in milliseconds, nearest-rank percentiles.

| method | p50 | p95 | p99 |
|---|---:|---:|---:|
| keyword-all | 477 | 513 | 537 |
| keyword-any | 586 | 1,016 | 1,072 |
| bm25 | 51.8 | 83.8 | 106 |
| bm25-trigram | 2,172 | 3,047 | 3,391 |
| bm25-postings | 38.0 | 54.8 | 77.2 |
| sparse | 35.7 | 49.4 | 63.6 |
| bm25-sparse | 48.5 | 65.9 | 78.8 |
| bm25-sparse-rrf | 34.4 | 58.7 | 182 |
| bm25-rerank-d20 | 5,247 | 15,467 | 16,744 |
| bm25-sampled | 69.5 | 140 | 197 |
| reference-bm25s-k1-0.9-b-0.4 | offline | offline | offline |
| reference-bm25s-k1-1.2-b-0.75 | offline | offline | offline |

??? note "Chart: p95 latency by pipeline"

    <svg role="img" aria-label="p95 latency on BEIR NFCorpus" viewBox="0 0 680 290" width="100%" style="max-width:680px" xmlns="http://www.w3.org/2000/svg">
    <title>p95 latency on BEIR NFCorpus</title>
    <text x="0" y="16" font-size="13" fill="currentColor">p95 latency on BEIR NFCorpus</text>
    <text x="232" y="41" font-size="11" text-anchor="end" fill="currentColor">keyword-all</text>
    <rect x="240" y="28" width="12.3" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="258.3" y="41" font-size="12" fill="currentColor">513 ms</text>
    <text x="232" y="67" font-size="11" text-anchor="end" fill="currentColor">keyword-any</text>
    <rect x="240" y="54" width="24.3" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="270.3" y="67" font-size="12" fill="currentColor">1,016 ms</text>
    <text x="232" y="93" font-size="11" text-anchor="end" fill="currentColor">bm25</text>
    <rect x="240" y="80" width="2.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="248.0" y="93" font-size="12" fill="currentColor">83.8 ms</text>
    <text x="232" y="119" font-size="11" text-anchor="end" fill="currentColor">bm25-trigram</text>
    <rect x="240" y="106" width="72.9" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="318.9" y="119" font-size="12" fill="currentColor">3,047 ms</text>
    <text x="232" y="145" font-size="11" text-anchor="end" fill="currentColor">bm25-postings</text>
    <rect x="240" y="132" width="1.3" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="247.3" y="145" font-size="12" fill="currentColor">54.8 ms</text>
    <text x="232" y="171" font-size="11" text-anchor="end" fill="currentColor">sparse</text>
    <rect x="240" y="158" width="1.2" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="247.2" y="171" font-size="12" fill="currentColor">49.4 ms</text>
    <text x="232" y="197" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse</text>
    <rect x="240" y="184" width="1.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="247.6" y="197" font-size="12" fill="currentColor">65.9 ms</text>
    <text x="232" y="223" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse-rrf</text>
    <rect x="240" y="210" width="1.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="247.4" y="223" font-size="12" fill="currentColor">58.7 ms</text>
    <text x="232" y="249" font-size="11" text-anchor="end" fill="currentColor">bm25-rerank-d20</text>
    <rect x="240" y="236" width="370.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="616.0" y="249" font-size="12" fill="currentColor">15,467 ms</text>
    <text x="232" y="275" font-size="11" text-anchor="end" fill="currentColor">bm25-sampled</text>
    <rect x="240" y="262" width="3.3" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="249.3" y="275" font-size="12" fill="currentColor">140 ms</text>
    </svg>

## What this result does not show

NFCorpus is nutrition and medical text. It does not predict recall over short chat notes.

## Limits of this run

One abstract per memory as '<title>. <text>' (BEIR indexes two fields); k 100, no token budget; English only. Known gap: served bm25 measures below BEIR's published 0.325 and Pyserini's single-field 0.322; the offline textbook BM25 over the same memories measures 0.318, so most of the gap is the one-body mapping and the analyser (Snowball against Lucene's Porter, hyphenated words kept whole), not the ranking.

- Offline bm25s over the same memories (Lucene BM25, English stop words, Snowball stems), not a served path;
- Served bm25 over the same seeded sample, on the same host in the same hour, for comparison with bm25-rerank-d20; the full-set bm25 row is the reference run.
- Known gap: served bm25 measures below BEIR's published 0.325 and Pyserini's single-field 0.322; the offline textbook BM25 over the same memories measures 0.318, so most of the gap is the one-body mapping and the analyser (Snowball against Lucene's Porter, hyphenated words kept whole), not the ranking. bm25's list reranked by ms-marco-MiniLM-L6-v2 through candle on the CPU at depth 20 (the default became 10 after this run), with a cap of 8 and a 60 s deadline so no rerank was skipped; the host carried a load average of 9 to 29 from other work, and the LoCoMo run shared it with the NFCorpus run, so its latency is an upper bound (.
- Known gap: served bm25 measures below BEIR's published 0.325 and Pyserini's single-field 0.322; the offline textbook BM25 over the same memories measures 0.318, so most of the gap is the one-body mapping and the analyser (Snowball against Lucene's Porter, hyphenated words kept whole), not the ranking. bm25-postings takes its candidates from the postings of the query forms it reads; when a query's readable postings pass POSTING_BUDGET (10,000) it leaves out the forms held by more than 10% of the readable rows, so a row holding only those is no candidate, and below the budget it reads every form and ranks as bm25.

Sources: `eval/bench/results/beir-nfcorpus/keyword-all.json`, `eval/bench/results/beir-nfcorpus/keyword-any.json`, `eval/bench/results/beir-nfcorpus/bm25.json`, `eval/bench/results/beir-nfcorpus/bm25-trigram.json`, `eval/bench/results/beir-nfcorpus/bm25-postings.json`, `eval/bench/results/beir-nfcorpus/sparse.json`, `eval/bench/results/beir-nfcorpus/bm25-sparse.json`, `eval/bench/results/beir-nfcorpus/bm25-sparse-rrf.json`, `eval/bench/results/beir-nfcorpus/bm25-rerank-d20.json`, `eval/bench/results/beir-nfcorpus/bm25-sampled.json`, `eval/bench/results/beir-nfcorpus/reference-bm25s-k1-0.9-b-0.4.json`, `eval/bench/results/beir-nfcorpus/reference-bm25s-k1-1.2-b-0.75.json`.
