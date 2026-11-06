---
status: Measured
verified: 2026-11-06
answers: "what each search pipeline measured on BEIR SciFact: quality, per-category results, latency, the reference numbers and the limits of the run"
---

# BEIR SciFact

*These are measurements by the OML maintainers on their own hardware, not independent results. Generated from the committed `oml-bench/1` results.*

Suite `beir-scifact`, data `beir/scifact/test (ir_datasets 0.6.3)`: 5,183 memories, 300 queries; a method marked sampled asked a seeded subset, named in its row.

??? note "Run details"

    Measured 2026-11-05 on commits `2123e90`, `66979b4`, `a1ad0c9`, `d3fdb72`, `f86c31c`; build release, host 8 vCPU / 15 GB, postgres 17.9; build release, host 8 vCPU / 15 GB, postgres 18.6; build release, host 8 vCPU / 15 GB, load average 3.6 to 14 from other lanes, postgres 18.6; build release, host 8 vCPU / 15 GB, shared, load 8-12, postgres 18.

**How to reproduce.** Build `oml` and `oml-server-lead` in release mode, install `eval/bench/requirements.txt` in a scratch virtual environment, then run `eval/bench/run.sh <out-dir> scifact` against a throwaway PostgreSQL superuser cluster. The harness loads the data over HTTP, promotes it with reviewer keys, asks every query through `oml_read` and writes the result files listed below.

[All suites](index.md)

## Quality

| method | queries | nDCG@10 | R@10 | R@20 | R@100 | RR@10 | S@1 | S@5 | zero |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| keyword-all | 300 | 0.013 | 0.013 | – | 0.013 | 0.013 | 0.013 | 0.013 | 0.987 |
| keyword-any | 300 | 0.515 | 0.632 | – | 0.781 | 0.488 | 0.403 | 0.603 | 0.000 |
| bm25 | 300 | 0.685 | 0.822 | 0.866 | 0.928 | 0.650 | 0.540 | 0.783 | 0.000 |
| bm25-trigram | 100 (sampled) | 0.611 | 0.749 | – | 0.906 | 0.578 | 0.480 | 0.710 | 0.000 |
| bm25-postings | 300 | 0.685 | 0.822 | 0.866 | 0.928 | 0.650 | 0.540 | 0.783 | 0.000 |
| sparse | 300 | 0.710 | 0.826 | 0.856 | 0.931 | 0.681 | 0.613 | 0.780 | 0.000 |
| bm25-sparse | 300 | 0.730 | 0.853 | 0.877 | 0.957 | 0.697 | 0.610 | 0.800 | 0.000 |
| bm25-sparse-rrf | 300 | 0.731 | 0.852 | 0.882 | 0.954 | 0.701 | 0.620 | 0.797 | 0.000 |
| bm25-k10 | 300 | 0.685 | 0.822 | – | 0.822 | 0.650 | 0.540 | 0.783 | 0.000 |
| bm25-rerank-d20 | 100 (sampled) | 0.699 | 0.805 | – | 0.925 | 0.676 | 0.580 | 0.800 | 0.000 |
| bm25-sampled | 100 (sampled) | 0.696 | 0.800 | – | 0.925 | 0.671 | 0.580 | 0.780 | 0.000 |
| bm25-trigram-k10 | 100 (sampled) | 0.612 | 0.754 | – | 0.754 | 0.580 | 0.480 | 0.720 | 0.000 |
| keyword-any-k10 | 300 | 0.515 | 0.632 | – | 0.632 | 0.488 | 0.403 | 0.603 | 0.000 |
| reference-bm25s-k1-0.9-b-0.4 | 300 | 0.676 | 0.801 | – | 0.913 | 0.641 | 0.547 | 0.757 | 0.000 |
| reference-bm25s-k1-1.2-b-0.75 | 300 | 0.679 | 0.808 | – | 0.913 | 0.643 | 0.543 | 0.753 | 0.000 |

Reference: BEIR BM25 (Anserini, k1 0.9, b 0.4, title and text fields), ndcg@10 0.665 ([source](https://arxiv.org/abs/2104.08663)).

??? note "Chart: nDCG@10 by pipeline"

    <svg role="img" aria-label="nDCG@10 on BEIR SciFact" viewBox="0 0 680 440" width="100%" style="max-width:680px" xmlns="http://www.w3.org/2000/svg">
    <title>nDCG@10 on BEIR SciFact</title>
    <text x="0" y="16" font-size="13" fill="currentColor">nDCG@10 on BEIR SciFact</text>
    <text x="232" y="41" font-size="11" text-anchor="end" fill="currentColor">keyword-all</text>
    <rect x="240" y="28" width="6.7" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="252.7" y="41" font-size="12" fill="currentColor">0.013</text>
    <text x="232" y="67" font-size="11" text-anchor="end" fill="currentColor">keyword-any</text>
    <rect x="240" y="54" width="260.5" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="506.5" y="67" font-size="12" fill="currentColor">0.515</text>
    <text x="232" y="93" font-size="11" text-anchor="end" fill="currentColor">bm25</text>
    <rect x="240" y="80" width="346.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="592.6" y="93" font-size="12" fill="currentColor">0.685</text>
    <text x="232" y="119" font-size="11" text-anchor="end" fill="currentColor">bm25-trigram</text>
    <rect x="240" y="106" width="309.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="555.0" y="119" font-size="12" fill="currentColor">0.611</text>
    <text x="232" y="145" font-size="11" text-anchor="end" fill="currentColor">bm25-postings</text>
    <rect x="240" y="132" width="346.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="592.6" y="145" font-size="12" fill="currentColor">0.685</text>
    <text x="232" y="171" font-size="11" text-anchor="end" fill="currentColor">sparse</text>
    <rect x="240" y="158" width="359.2" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="605.2" y="171" font-size="12" fill="currentColor">0.710</text>
    <text x="232" y="197" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse</text>
    <rect x="240" y="184" width="369.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="615.4" y="197" font-size="12" fill="currentColor">0.730</text>
    <text x="232" y="223" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse-rrf</text>
    <rect x="240" y="210" width="370.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="616.0" y="223" font-size="12" fill="currentColor">0.731</text>
    <text x="232" y="249" font-size="11" text-anchor="end" fill="currentColor">bm25-k10</text>
    <rect x="240" y="236" width="346.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="592.6" y="249" font-size="12" fill="currentColor">0.685</text>
    <text x="232" y="275" font-size="11" text-anchor="end" fill="currentColor">bm25-rerank-d20</text>
    <rect x="240" y="262" width="353.7" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="599.7" y="275" font-size="12" fill="currentColor">0.699</text>
    <text x="232" y="301" font-size="11" text-anchor="end" fill="currentColor">bm25-sampled</text>
    <rect x="240" y="288" width="351.9" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="597.9" y="301" font-size="12" fill="currentColor">0.696</text>
    <text x="232" y="327" font-size="11" text-anchor="end" fill="currentColor">bm25-trigram-k10</text>
    <rect x="240" y="314" width="309.8" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="555.8" y="327" font-size="12" fill="currentColor">0.612</text>
    <text x="232" y="353" font-size="11" text-anchor="end" fill="currentColor">keyword-any-k10</text>
    <rect x="240" y="340" width="260.5" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="506.5" y="353" font-size="12" fill="currentColor">0.515</text>
    <text x="232" y="379" font-size="11" text-anchor="end" fill="currentColor">reference-bm25s-k1-0.9-b-0.4</text>
    <rect x="240" y="366" width="342.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="588.1" y="379" font-size="12" fill="currentColor">0.676</text>
    <text x="232" y="405" font-size="11" text-anchor="end" fill="currentColor">reference-bm25s-k1-1.2-b-0.75</text>
    <rect x="240" y="392" width="343.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="589.6" y="405" font-size="12" fill="currentColor">0.679</text>
    <line x1="576.5" y1="24" x2="576.5" y2="418" stroke="currentColor" stroke-dasharray="4 3"/>
    <text x="576.5" y="432" font-size="11" text-anchor="middle" fill="currentColor">published 0.665</text>
    </svg>

## Latency

Client wall time per recall, in milliseconds, nearest-rank percentiles.

| method | p50 | p95 | p99 |
|---|---:|---:|---:|
| keyword-all | 629 | 672 | 701 |
| keyword-any | 815 | 1,075 | 1,203 |
| bm25 | 86.1 | 116 | 130 |
| bm25-trigram | 3,707 | 5,454 | 6,284 |
| bm25-postings | 46.2 | 65.6 | 73.5 |
| sparse | 36.4 | 50.5 | 57.9 |
| bm25-sparse | 52.6 | 83.7 | 90.6 |
| bm25-sparse-rrf | 50.5 | 89.6 | 131 |
| bm25-k10 | 70.5 | 98.5 | 109 |
| bm25-rerank-d20 | 4,563 | 5,711 | 5,896 |
| bm25-sampled | 102 | 140 | 155 |
| bm25-trigram-k10 | 3,664 | 5,334 | 6,358 |
| keyword-any-k10 | 630 | 1,090 | 1,165 |
| reference-bm25s-k1-0.9-b-0.4 | offline | offline | offline |
| reference-bm25s-k1-1.2-b-0.75 | offline | offline | offline |

??? note "Chart: p95 latency by pipeline"

    <svg role="img" aria-label="p95 latency on BEIR SciFact" viewBox="0 0 680 368" width="100%" style="max-width:680px" xmlns="http://www.w3.org/2000/svg">
    <title>p95 latency on BEIR SciFact</title>
    <text x="0" y="16" font-size="13" fill="currentColor">p95 latency on BEIR SciFact</text>
    <text x="232" y="41" font-size="11" text-anchor="end" fill="currentColor">keyword-all</text>
    <rect x="240" y="28" width="43.5" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="289.5" y="41" font-size="12" fill="currentColor">672 ms</text>
    <text x="232" y="67" font-size="11" text-anchor="end" fill="currentColor">keyword-any</text>
    <rect x="240" y="54" width="69.7" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="315.7" y="67" font-size="12" fill="currentColor">1,075 ms</text>
    <text x="232" y="93" font-size="11" text-anchor="end" fill="currentColor">bm25</text>
    <rect x="240" y="80" width="7.5" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="253.5" y="93" font-size="12" fill="currentColor">116 ms</text>
    <text x="232" y="119" font-size="11" text-anchor="end" fill="currentColor">bm25-trigram</text>
    <rect x="240" y="106" width="353.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="599.4" y="119" font-size="12" fill="currentColor">5,454 ms</text>
    <text x="232" y="145" font-size="11" text-anchor="end" fill="currentColor">bm25-postings</text>
    <rect x="240" y="132" width="4.2" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="250.2" y="145" font-size="12" fill="currentColor">65.6 ms</text>
    <text x="232" y="171" font-size="11" text-anchor="end" fill="currentColor">sparse</text>
    <rect x="240" y="158" width="3.3" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="249.3" y="171" font-size="12" fill="currentColor">50.5 ms</text>
    <text x="232" y="197" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse</text>
    <rect x="240" y="184" width="5.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="251.4" y="197" font-size="12" fill="currentColor">83.7 ms</text>
    <text x="232" y="223" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse-rrf</text>
    <rect x="240" y="210" width="5.8" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="251.8" y="223" font-size="12" fill="currentColor">89.6 ms</text>
    <text x="232" y="249" font-size="11" text-anchor="end" fill="currentColor">bm25-k10</text>
    <rect x="240" y="236" width="6.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="252.4" y="249" font-size="12" fill="currentColor">98.5 ms</text>
    <text x="232" y="275" font-size="11" text-anchor="end" fill="currentColor">bm25-rerank-d20</text>
    <rect x="240" y="262" width="370.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="616.0" y="275" font-size="12" fill="currentColor">5,711 ms</text>
    <text x="232" y="301" font-size="11" text-anchor="end" fill="currentColor">bm25-sampled</text>
    <rect x="240" y="288" width="9.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="255.1" y="301" font-size="12" fill="currentColor">140 ms</text>
    <text x="232" y="327" font-size="11" text-anchor="end" fill="currentColor">bm25-trigram-k10</text>
    <rect x="240" y="314" width="345.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="591.6" y="327" font-size="12" fill="currentColor">5,334 ms</text>
    <text x="232" y="353" font-size="11" text-anchor="end" fill="currentColor">keyword-any-k10</text>
    <rect x="240" y="340" width="70.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="316.6" y="353" font-size="12" fill="currentColor">1,090 ms</text>
    </svg>

## What this result does not show

SciFact is scientific abstracts and claims. It does not predict recall over short chat notes.

## Limits of this run

One abstract per memory as '<title>. <text>' (BEIR indexes two fields); k 100, no token budget; English only.

- Offline bm25s over the same memories (Lucene BM25, English stop words, Snowball stems), not a served path;
- Asked with k 10, the size an agent asks, to measure latency at that k; recall@100 reads the shorter list.
- Served bm25 over the same seeded sample, on the same host in the same hour, for comparison with bm25-rerank-d20; the full-set bm25 row is the reference run.
- English only. bm25's list reranked by ms-marco-MiniLM-L6-v2 through candle on the CPU at depth 20 (the default became 10 after this run), with a cap of 8 and a 60 s deadline so no rerank was skipped; the host carried a load average of 9 to 29 from other work, and the LoCoMo run shared it with the NFCorpus run, so its latency is an upper bound (.
- English only. bm25-postings takes its candidates from the postings of the query forms it reads; when a query's readable postings pass POSTING_BUDGET (10,000) it leaves out the forms held by more than 10% of the readable rows, so a row holding only those is no candidate, and below the budget it reads every form and ranks as bm25.

Sources: `eval/bench/results/beir-scifact/keyword-all.json`, `eval/bench/results/beir-scifact/keyword-any.json`, `eval/bench/results/beir-scifact/bm25.json`, `eval/bench/results/beir-scifact/bm25-trigram.json`, `eval/bench/results/beir-scifact/bm25-postings.json`, `eval/bench/results/beir-scifact/sparse.json`, `eval/bench/results/beir-scifact/bm25-sparse.json`, `eval/bench/results/beir-scifact/bm25-sparse-rrf.json`, `eval/bench/results/beir-scifact/bm25-k10.json`, `eval/bench/results/beir-scifact/bm25-rerank-d20.json`, `eval/bench/results/beir-scifact/bm25-sampled.json`, `eval/bench/results/beir-scifact/bm25-trigram-k10.json`, `eval/bench/results/beir-scifact/keyword-any-k10.json`, `eval/bench/results/beir-scifact/reference-bm25s-k1-0.9-b-0.4.json`, `eval/bench/results/beir-scifact/reference-bm25s-k1-1.2-b-0.75.json`.
