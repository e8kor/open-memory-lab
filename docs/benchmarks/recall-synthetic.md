---
status: Measured
verified: 2026-11-06
answers: "what each search pipeline measured on Synthetic recall set: quality, per-category results, latency, the reference numbers and the limits of the run"
---

# Synthetic recall set

*These are measurements by the OML maintainers on their own hardware, not independent results. Generated from the committed `oml-bench/1` results.*

Suite `recall-synthetic`, data `eval/recall data version 2`: 200 memories, 121 queries; a method marked sampled asked a seeded subset, named in its row.

??? note "Run details"

    Measured 2026-11-05, date not recorded on commits `3c70c71`, `71b7b69`, `c257083`, `not recorded`; build release, host 8 vCPU / 15 GB, postgres 17.9; build release, host 8 vCPU / 15 GB, postgres 18.6; build release, host 8 vCPU / 15 GB, load average 3.6 to 14 from other lanes, postgres 18.6.

**How to reproduce.** Build `oml` and `oml-server-lead` in release mode, install `eval/bench/requirements.txt` in a scratch virtual environment, then run `eval/bench/run.sh <out-dir> synthetic` against a throwaway PostgreSQL superuser cluster. The harness loads the data over HTTP, promotes it with reviewer keys, asks every query through `oml_read` and writes the result files listed below.

[All suites](index.md)

## Quality

| method | queries | nDCG@10 | R@10 | R@20 | R@100 | RR@10 | S@1 | S@5 | zero |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| keyword-all | 121 | 0.156 | 0.125 | – | – | 0.174 | 0.174 | 0.174 | 0.826 |
| keyword-any | 121 | 0.689 | 0.725 | – | – | 0.712 | 0.636 | 0.843 | 0.033 |
| bm25 | 121 | 0.876 | 0.910 | – | – | 0.876 | 0.835 | 0.926 | 0.000 |
| bm25-trigram | 121 | 0.874 | 0.924 | – | – | 0.863 | 0.802 | 0.950 | 0.000 |
| bm25-postings | 121 | 0.873 | 0.910 | – | – | 0.872 | 0.826 | 0.934 | 0.000 |
| bm25-rerank | 121 | 0.906 | 0.910 | – | – | 0.923 | 0.893 | 0.950 | 0.000 |
| sparse | 121 | 0.891 | 0.937 | – | – | 0.880 | 0.818 | 0.967 | 0.000 |
| bm25-sparse | 121 | 0.910 | 0.942 | – | – | 0.907 | 0.860 | 0.983 | 0.000 |

??? note "Chart: nDCG@10 by pipeline"

    <svg role="img" aria-label="nDCG@10 on Synthetic recall set" viewBox="0 0 680 238" width="100%" style="max-width:680px" xmlns="http://www.w3.org/2000/svg">
    <title>nDCG@10 on Synthetic recall set</title>
    <text x="0" y="16" font-size="13" fill="currentColor">nDCG@10 on Synthetic recall set</text>
    <text x="232" y="41" font-size="11" text-anchor="end" fill="currentColor">keyword-all</text>
    <rect x="240" y="28" width="63.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="309.4" y="41" font-size="12" fill="currentColor">0.156</text>
    <text x="232" y="67" font-size="11" text-anchor="end" fill="currentColor">keyword-any</text>
    <rect x="240" y="54" width="280.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="526.1" y="67" font-size="12" fill="currentColor">0.689</text>
    <text x="232" y="93" font-size="11" text-anchor="end" fill="currentColor">bm25</text>
    <rect x="240" y="80" width="356.2" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="602.2" y="93" font-size="12" fill="currentColor">0.876</text>
    <text x="232" y="119" font-size="11" text-anchor="end" fill="currentColor">bm25-trigram</text>
    <rect x="240" y="106" width="355.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="601.4" y="119" font-size="12" fill="currentColor">0.874</text>
    <text x="232" y="145" font-size="11" text-anchor="end" fill="currentColor">bm25-postings</text>
    <rect x="240" y="132" width="355.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="601.0" y="145" font-size="12" fill="currentColor">0.873</text>
    <text x="232" y="171" font-size="11" text-anchor="end" fill="currentColor">bm25-rerank</text>
    <rect x="240" y="158" width="368.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="614.4" y="171" font-size="12" fill="currentColor">0.906</text>
    <text x="232" y="197" font-size="11" text-anchor="end" fill="currentColor">sparse</text>
    <rect x="240" y="184" width="362.3" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="608.3" y="197" font-size="12" fill="currentColor">0.891</text>
    <text x="232" y="223" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse</text>
    <rect x="240" y="210" width="370.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="616.0" y="223" font-size="12" fill="currentColor">0.910</text>
    </svg>

## By category

??? note "Results by category"

    nDCG@10 / S@5 for each category, with its query count.

    | category | n | keyword-all | keyword-any | bm25 | bm25-trigram | bm25-postings | bm25-rerank | sparse | bm25-sparse |
    |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
    | exact | 11 | 0.84 / 1.00 | 0.91 / 1.00 | 0.96 / 1.00 | 0.94 / 1.00 | 0.96 / 1.00 | 0.96 / 1.00 | 0.96 / 1.00 | 0.96 / 1.00 |
    | identifier | 11 | 0.71 / 0.73 | 0.81 / 0.82 | 1.00 / 1.00 | 0.99 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 |
    | inflection | 11 | 0.00 / 0.00 | 0.55 / 0.82 | 0.97 / 1.00 | 0.95 / 1.00 | 0.97 / 1.00 | 0.97 / 1.00 | 0.93 / 1.00 | 0.96 / 1.00 |
    | long | 11 | 0.00 / 0.00 | 0.62 / 0.91 | 0.96 / 1.00 | 0.93 / 1.00 | 0.96 / 1.00 | 0.90 / 0.91 | 0.87 / 0.91 | 0.92 / 1.00 |
    | multihop | 11 | 0.00 / 0.00 | 0.48 / 0.91 | 0.83 / 0.91 | 0.79 / 1.00 | 0.83 / 0.91 | 0.89 / 1.00 | 0.89 / 1.00 | 0.90 / 1.00 |
    | negation | 11 | 0.17 / 0.18 | 0.82 / 1.00 | 0.97 / 1.00 | 0.97 / 1.00 | 0.97 / 1.00 | 0.99 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 |
    | paraphrase | 11 | 0.00 / 0.00 | 0.43 / 0.55 | 0.50 / 0.73 | 0.53 / 0.73 | 0.50 / 0.73 | 0.61 / 0.73 | 0.63 / 0.73 | 0.59 / 0.82 |
    | partial | 11 | 0.00 / 0.00 | 0.64 / 0.73 | 0.94 / 0.91 | 0.90 / 0.91 | 0.91 / 1.00 | 0.99 / 1.00 | 0.93 / 1.00 | 0.96 / 1.00 |
    | question | 11 | 0.00 / 0.00 | 0.76 / 0.82 | 0.93 / 1.00 | 0.90 / 0.91 | 0.93 / 1.00 | 0.94 / 1.00 | 0.85 / 1.00 | 0.92 / 1.00 |
    | temporal | 11 | 0.00 / 0.00 | 0.83 / 0.91 | 0.76 / 0.82 | 0.82 / 0.91 | 0.76 / 0.82 | 0.90 / 1.00 | 0.79 / 1.00 | 0.83 / 1.00 |
    | typo | 11 | 0.00 / 0.00 | 0.71 / 0.82 | 0.82 / 0.82 | 0.88 / 1.00 | 0.82 / 0.82 | 0.82 / 0.82 | 0.95 / 1.00 | 0.95 / 1.00 |

## Latency

Client wall time per recall, in milliseconds, nearest-rank percentiles.

| method | p50 | p95 | p99 |
|---|---:|---:|---:|
| keyword-all | 14.2 | 18.6 | 22.8 |
| keyword-any | 18.9 | 29.7 | 36.8 |
| bm25 | 16.3 | 21.2 | 22.4 |
| bm25-trigram | 30.4 | 41.9 | 45.0 |
| bm25-postings | 10.1 | 48.0 | 101 |
| bm25-rerank | 128 | 215 | 271 |
| sparse | 9.3 | 15.4 | 23.2 |
| bm25-sparse | 12.5 | 18.7 | 37.5 |

??? note "Chart: p95 latency by pipeline"

    <svg role="img" aria-label="p95 latency on Synthetic recall set" viewBox="0 0 680 238" width="100%" style="max-width:680px" xmlns="http://www.w3.org/2000/svg">
    <title>p95 latency on Synthetic recall set</title>
    <text x="0" y="16" font-size="13" fill="currentColor">p95 latency on Synthetic recall set</text>
    <text x="232" y="41" font-size="11" text-anchor="end" fill="currentColor">keyword-all</text>
    <rect x="240" y="28" width="32.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="278.0" y="41" font-size="12" fill="currentColor">18.6 ms</text>
    <text x="232" y="67" font-size="11" text-anchor="end" fill="currentColor">keyword-any</text>
    <rect x="240" y="54" width="51.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="297.1" y="67" font-size="12" fill="currentColor">29.7 ms</text>
    <text x="232" y="93" font-size="11" text-anchor="end" fill="currentColor">bm25</text>
    <rect x="240" y="80" width="36.5" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="282.5" y="93" font-size="12" fill="currentColor">21.2 ms</text>
    <text x="232" y="119" font-size="11" text-anchor="end" fill="currentColor">bm25-trigram</text>
    <rect x="240" y="106" width="72.1" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="318.1" y="119" font-size="12" fill="currentColor">41.9 ms</text>
    <text x="232" y="145" font-size="11" text-anchor="end" fill="currentColor">bm25-postings</text>
    <rect x="240" y="132" width="82.5" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="328.5" y="145" font-size="12" fill="currentColor">48.0 ms</text>
    <text x="232" y="171" font-size="11" text-anchor="end" fill="currentColor">bm25-rerank</text>
    <rect x="240" y="158" width="370.0" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="616.0" y="171" font-size="12" fill="currentColor">215 ms</text>
    <text x="232" y="197" font-size="11" text-anchor="end" fill="currentColor">sparse</text>
    <rect x="240" y="184" width="26.4" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="272.4" y="197" font-size="12" fill="currentColor">15.4 ms</text>
    <text x="232" y="223" font-size="11" text-anchor="end" fill="currentColor">bm25-sparse</text>
    <rect x="240" y="210" width="32.2" height="18" fill="currentColor" fill-opacity="0.55"/>
    <text x="278.2" y="223" font-size="12" fill="currentColor">18.7 ms</text>
    </svg>

## What this result does not show

One model wrote the corpus, the queries and the labels, and its name was not recorded. The scores compare OML pipelines with each other; do not compare them with BEIR or LoCoMo.

## Limits of this run

One model wrote corpus, queries and labels; success counts grade 2, nDCG gains 2^g-1, k 10 under a 2,000-token budget, so recall@100 is not measured.

- Of 22 unanswerable queries, kept out of these metrics, 0.000 were answered with no hit.
- Of 22 unanswerable queries, kept out of these metrics, 0.000 were answered with no hit. bm25-rerank at its defaults (ms-marco-MiniLM-L6-v2, depth 10, 256 tokens, 2 threads, a 1 s deadline, floor off); no rerank was skipped; the host's load average was 3.3 to 4.0 from other work, and another reviewer may have been running tests beside it (.

Sources: `eval/bench/results/recall-synthetic/keyword-all.json`, `eval/bench/results/recall-synthetic/keyword-any.json`, `eval/bench/results/recall-synthetic/bm25.json`, `eval/bench/results/recall-synthetic/bm25-trigram.json`, `eval/bench/results/recall-synthetic/bm25-postings.json`, `eval/bench/results/recall-synthetic/bm25-rerank.json`, `eval/bench/results/recall-synthetic/sparse.json`, `eval/bench/results/recall-synthetic/bm25-sparse.json`.
