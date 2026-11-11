---
status: Measured
verified: 2026-11-06
answers: "what each search pipeline measured on Grown recall set: quality, per-category results, latency, the reference numbers and the limits of the run"
---

# Grown recall set

*These are measurements by the OML maintainers on their own hardware, not independent results. Generated from the committed `oml-bench/1` results.*

Suite `recall-grown`, data `eval/recall grown set version 1, corpus version 1`: 200 memories, 665 queries; a method marked sampled asked a seeded subset, named in its row.

??? note "Run details"

    Measured 2026-11-11 on commits `08892ff1`; build release, host 8 vCPU / 15 GB, postgres 18.6.

**How to reproduce.** Build `oml` and `oml-server-lead` in release mode, install `eval/bench/requirements.txt` in a scratch virtual environment, then run `eval/bench/run.sh <out-dir> grown` against a throwaway PostgreSQL superuser cluster. The harness loads the data over HTTP, promotes it with reviewer keys, asks every query through `oml_read` and writes the result files listed below.

[All suites](index.md)

## Quality

| method | queries | nDCG@10 | R@10 | R@20 | R@100 | RR@10 | S@1 | S@5 | zero |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| bm25-postings | 665 | 0.869 | 0.943 | 0.970 | 0.982 | 0.845 | 0.789 | 0.913 | 0.000 |
| bm25-rerank | 665 | 0.916 | 0.943 | 0.970 | 0.982 | 0.906 | 0.878 | 0.941 | 0.000 |
| bm25-sparse | 665 | 0.918 | 0.971 | 0.984 | 0.994 | 0.900 | 0.857 | 0.956 | 0.000 |
| bm25-sparse-rrf | 665 | 0.897 | 0.965 | 0.981 | 0.994 | 0.875 | 0.818 | 0.947 | 0.000 |

??? note "Chart: nDCG@10 by pipeline"

    <svg role="img" aria-label="nDCG@10 on Grown recall set" viewBox="0 0 680 134" width="100%" style="max-width:680px" xmlns="http://www.w3.org/2000/svg">
    <title>nDCG@10 on Grown recall set</title>
    <text x="0" y="16" font-size="13" fill="currentColor">nDCG@10 on Grown recall set</text>
    <text x="232" y="41" font-size="11" text-anchor="end" fill="currentColor">bm25-postings</text>
    <rect x="240" y="28" width="350.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="596.1" y="41" font-size="12" fill="currentColor">0.869</text>
    <text x="232" y="67" font-size="11" text-anchor="end" fill="currentColor">bm25-rerank</text>
    <rect x="240" y="54" width="369.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="615.1" y="67" font-size="12" fill="currentColor">0.916</text>
    <text x="232" y="93" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse</text>
    <rect x="240" y="80" width="370.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="616.0" y="93" font-size="12" fill="currentColor">0.918</text>
    <text x="232" y="119" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse-rrf</text>
    <rect x="240" y="106" width="361.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="607.6" y="119" font-size="12" fill="currentColor">0.897</text>
    </svg>

## By category

??? note "Results by category"

    nDCG@10 / S@5 for each category, with its query count.

    | category | n | bm25-postings | bm25-rerank | bm25-sparse | bm25-sparse-rrf |
    |---|---:|---:|---:|---:|---:|
    | all-categories | 665 | 0.87 / 0.91 | 0.92 / 0.94 | 0.92 / 0.96 | 0.90 / 0.95 |
    | paraphrase | 303 | 0.77 / 0.83 | 0.85 / 0.89 | 0.85 / 0.91 | 0.83 / 0.90 |
    | question | 362 | 0.95 / 0.98 | 0.97 / 0.99 | 0.97 / 0.99 | 0.95 / 0.99 |

## Latency

Client wall time per recall, in milliseconds, nearest-rank percentiles.

| method | p50 | p95 | p99 |
|---|---:|---:|---:|
| bm25-postings | 23.5 | 32.5 | 37.3 |
| bm25-rerank | 142 | 240 | 273 |
| bm25-sparse | 42.5 | 49.7 | 53.8 |
| bm25-sparse-rrf | 38.3 | 48.1 | 51.9 |

??? note "Chart: p95 latency by pipeline"

    <svg role="img" aria-label="p95 latency on Grown recall set" viewBox="0 0 680 134" width="100%" style="max-width:680px" xmlns="http://www.w3.org/2000/svg">
    <title>p95 latency on Grown recall set</title>
    <text x="0" y="16" font-size="13" fill="currentColor">p95 latency on Grown recall set</text>
    <text x="232" y="41" font-size="11" text-anchor="end" fill="currentColor">bm25-postings</text>
    <rect x="240" y="28" width="50.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="296.0" y="41" font-size="12" fill="currentColor">32.5 ms</text>
    <text x="232" y="67" font-size="11" text-anchor="end" fill="currentColor">bm25-rerank</text>
    <rect x="240" y="54" width="370.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="616.0" y="67" font-size="12" fill="currentColor">240 ms</text>
    <text x="232" y="93" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse</text>
    <rect x="240" y="80" width="76.6" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="322.6" y="93" font-size="12" fill="currentColor">49.7 ms</text>
    <text x="232" y="119" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse-rrf</text>
    <rect x="240" y="106" width="74.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="320.1" y="119" font-size="12" fill="currentColor">48.1 ms</text>
    </svg>

## What this result does not show

Phi-3-mini drafted the queries. Which model wrote the corpus is unknown, so whether the two are of different families, as ADR-0075 d8 asks, is unverified. Each query labels only the memory it was drafted from, so a second memory that also answers is not credited. The set exists to give ADR-0075's phase 0 a baseline with 90% power; compare pipelines with each other on it, not with BEIR or LoCoMo.

## Limits of this run

The synthetic corpus's 200 memories with the grown set's question-shaped and paraphrased queries (phase 0a), drafted by Phi-3-mini (the corpus's writer is unknown, so whether it is another family is unverified) and labelled with the memory each was drafted from; success counts grade 2; k 100, no token budget;

- English only.
- English only. bm25-postings takes its candidates from the postings of the query forms it reads; when a query's readable postings pass POSTING_BUDGET (10,000) it leaves out the forms held by more than 10% of the readable rows, so a row holding only those is no candidate, and below the budget it reads every form and ranks as bm25.

Sources: `eval/bench/results/recall-grown/bm25-postings.json`, `eval/bench/results/recall-grown/bm25-rerank.json`, `eval/bench/results/recall-grown/bm25-sparse.json`, `eval/bench/results/recall-grown/bm25-sparse-rrf.json`.
