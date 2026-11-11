---
status: Measured
verified: 2026-11-06
answers: "what to take from the benchmarks, which search pipeline is the default and what each pipeline measured on every benchmark suite: quality, latency, the date, commit and environment of each run"
---

# Benchmarks

**What to take from this.**

- Recall quality is keyword-search grade: nDCG@10 of 0.32 to 0.87 depending on the corpus. No embeddings model or language model judges an answer.
- Recall took under 66 ms at the 95th percentile for 200 to 5,882 memories. At the database, 99,721 readable memories took 25.5 ms (`bench/scale/REPORT.md`).
- These are the maintainers' runs on one shared host, not independent results.
- They are retrieval metrics. They are not the answer-accuracy scores other memory products publish for LoCoMo, which a language model judges, and cannot be compared with them.

*These are measurements by the OML maintainers on their own hardware, not independent results. Generated from the committed `oml-bench/1` results.*

## Results at a glance

The default search pipeline, `bm25-postings`, on each suite. nDCG@10 and S@5 measure quality; p95 is the client's wall time per recall. The BEIR column is the published BM25 baseline for the same suite ([BEIR](https://arxiv.org/abs/2104.08663)), where one exists.

| suite | memories | queries | nDCG@10 | BEIR BM25 | S@5 | p95 ms | run |
|---|---:|---:|---:|---:|---:|---:|---|
| [Synthetic recall set](recall-synthetic.md) | 200 | 121 | 0.873 | – | 0.934 | 48.0 | 2026-11-05 |
| [Grown recall set](recall-grown.md) | 200 | 665 | 0.869 | – | 0.913 | 32.5 | 2026-11-11 |
| [BEIR SciFact](beir-scifact.md) | 5,183 | 300 | 0.685 | 0.665 | 0.783 | 65.6 | 2026-11-05 |
| [BEIR NFCorpus](beir-nfcorpus.md) | 3,633 | 323 | 0.321 | 0.325 | 0.656 | 54.8 | 2026-11-05 |
| [LoCoMo](locomo.md) | 5,882 | 1,535 | 0.503 | – | 0.628 | 37.0 | 2026-11-05 |

On SciFact the default scores 0.685 against the baseline's 0.665; on NFCorpus the default scores 0.321 against the baseline's 0.325.

## Metric definitions

- **nDCG@10**: ranking quality over the top 10 hits, 0 to 1; higher is better.
- **R@k**: the share of the relevant memories found in the top k.
- **RR@10**: the reciprocal rank of the first relevant hit within 10, averaged over queries.
- **S@1, S@5** (success at k): the share of queries with a relevant hit in the top 1 or top 5.
- **zero**: the share of queries that returned no hit.
- **p50, p95, p99**: the client's wall time per recall in milliseconds, nearest-rank percentiles.

These are retrieval metrics. No language model judges any answer, so there is no judge noise.

## Why `bm25-postings` is the default

The default is `bm25-postings` (the deployment's `search.default` when unset; [settings](../operations/configuration.md)). It scores within 0.003 nDCG@10 of `bm25` on the quality suites. It is also the one pipeline an index serves under row security, so its latency grows slowest with the rows. The pipelines that score higher read every readable row's stored text vectors or add a model stage. The [benchmark report](https://github.com/e8kor/open-memory-lab/blob/main/eval/bench/REPORT.md) gives the reasoning.

## Methodology

Every number went through the served path. An agent key wrote the memories over HTTP, reviewer keys of other principals promoted them, and `oml_read` over `/mcp` recalled them, one query at a time. Latency is the client's wall time per call, in milliseconds. Reference rows are offline and have no latency. Every suite page lists all pipelines, the run's commit and host, and what the result does not show. The host was shared with other jobs, so every latency is an upper bound for one client.

??? note "The latency ceiling"

    **Why latency grows with the rows.** Under row security no text index serves a recall. Every pipeline except `bm25-postings` therefore reads the stored text vectors of every row the key may read, and its cost grows with those rows. Stored vectors made a row several times cheaper, not free. An every-word match crossed 50 ms near 70,000 readable rows (`bench/scale/REPORT-baseline.md`). `bm25` reads more per row, so on abstract-length text it crosses sooner, as the tables show. `bm25-postings` reads integer postings, which row security lets an index serve, so its cost follows the postings it reads. It measured 25.5 ms p95 at 99,721 readable memories, universe scope, against `bm25`'s 485 ms. That figure was measured at the database, not through the client, on 2026-11-05 on a host shared with other jobs, so treat it as an upper bound (`bench/scale/REPORT.md`).

## Suites

- [Synthetic recall set](recall-synthetic.md): 200 memories, 121 queries, 8 pipelines.
- [Grown recall set](recall-grown.md): 200 memories, 665 queries, 4 pipelines.
- [BEIR SciFact](beir-scifact.md): 5,183 memories, 300 queries, 15 pipelines.
- [BEIR NFCorpus](beir-nfcorpus.md): 3,633 memories, 323 queries, 12 pipelines.
- [LoCoMo](locomo.md): 5,882 memories, 1,535 queries, 12 pipelines.
- [SciFact ×10 (latency at 50k rows)](scale.md): 51,830 memories, 300 queries, 5 pipelines.

??? note "Run details: dates, commits and environments"

    | suite | date | commits | environment |
    |---|---|---|---|
    | Synthetic recall set | 2026-11-05 (some files carry no date) | `3c70c71`, `71b7b69`, `c257083` | host 8 vCPU / 15 GB, load average 3.6 to 14 from other lanes, postgres 18.6, build release<br>host 8 vCPU / 15 GB, postgres 17.9, build release<br>host 8 vCPU / 15 GB, postgres 18.6, build release |
    | Grown recall set | 2026-11-11 | `08892ff1` | host 8 vCPU / 15 GB, postgres 18.6, build release |
    | BEIR SciFact | 2026-11-05 | `2123e90`, `66979b4`, `a1ad0c9`, `d3fdb72`, `f86c31c` | host 8 vCPU / 15 GB, load average 3.6 to 14 from other lanes, postgres 18.6, build release<br>host 8 vCPU / 15 GB, postgres 17.9, build release<br>host 8 vCPU / 15 GB, postgres 18.6, build release<br>host 8 vCPU / 15 GB, shared, load 8-12, postgres 18, build release |
    | BEIR NFCorpus | 2026-11-05 | `2123e90`, `66979b4`, `a1ad0c9`, `d3fdb72`, `f86c31c` | host 8 vCPU / 15 GB, load average 3.6 to 14 from other lanes, postgres 18.6, build release<br>host 8 vCPU / 15 GB, postgres 17.9, build release<br>host 8 vCPU / 15 GB, postgres 18.6, build release<br>host 8 vCPU / 15 GB, shared, load 8-12, postgres 18, build release |
    | LoCoMo | 2026-11-05 | `2123e90`, `66979b4`, `a1ad0c9`, `d3fdb72`, `f86c31c` | host 8 vCPU / 15 GB, load average 3.6 to 14 from other lanes, postgres 18.6, build release<br>host 8 vCPU / 15 GB, postgres 17.9, build release<br>host 8 vCPU / 15 GB, postgres 18.6, build release<br>host 8 vCPU / 15 GB, shared, load 8-12, postgres 18, build release |
    | SciFact ×10 (latency at 50k rows) | 2026-11-05 | `d3fdb72` | host 8 vCPU / 15 GB, postgres 17.9, build release |
