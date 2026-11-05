# Recall benchmark

The recall eval in [`../recall/`](../recall/README.md) measures OML's recall on 200 memories
and 121 queries one model wrote, in English, with no unanswerable query. This harness runs
the same served recall over benchmarks other people built: BEIR's SciFact and NFCorpus and
the LoCoMo conversations, and the synthetic set again through the same code so the numbers
sit side by side. The findings are in [REPORT.md](REPORT.md).

It writes nothing it measures by hand. Every memory goes in through `POST /v1/memories` with
an agent key and is promoted through `/v2/review/next` and `/v2/review/verdicts` by reviewer
keys of other principals; every query goes out through `oml_read` over `POST /mcp`, the tool
the `oml` plugin calls. The metrics come from [ir_measures](https://ir-measur.es/)
(Apache-2.0) over [pytrec_eval](https://github.com/cvangysel/pytrec_eval) (MIT), and the BEIR
data through [ir_datasets](https://ir-datasets.com/) (MIT), rather than from code written here.

## Files

| file | what it is |
|---|---|
| `adapters.py` | one adapter per dataset: `load(cache, seed, limit)` returns memories, queries and qrels; also the published BM25 numbers used as a sanity check |
| `bench.py` | `workareas`, `load`, `run` and `report`; reuses `eval/recall/run.py`'s HTTP client |
| `boot.sh` | boots a lead against a throwaway database, creates the workareas and the keys, and serves until stopped |
| `run.sh` | boot, load, run every pipeline and print the tables, in one command; the nightly workflow runs it |
| `reference.py` | ranks the same memories offline with [bm25s](https://github.com/xhluca/bm25s) (MIT), to split a gap to a published number between the mapping and the served implementation |
| `export.py` | writes every result in the common schema below, and converts `eval/recall`'s served results too |
| `rerank_probe.py` | reorders a served run's first-stage lists offline with each cross-encoder through onnxruntime, at several depths, to choose a model and a depth before one is served (ADR-0053 d4) |
| `rerank_floor.py` | sweeps a relevance floor over runs made with none: unanswerable queries emptied against answerable ones lost, and LoCoMo's categories (ADR-0053 d6) |
| `requirements.txt` | the Python packages, pinned |
| `results/<suite>/<method>.json` | the committed results, one file per suite and method, schema `oml-bench/1` |
| `results/tables.md`, `results/load-*.json` | the report tables and each dataset's load log |

## Running it

A PostgreSQL superuser on a throwaway cluster, a release build, and a scratch venv (never the
system's, never inside the repository):

```sh
cargo build --release --bin oml-server-lead --bin oml
uv venv /srv/build/bench-venv
uv pip install --python /srv/build/bench-venv/bin/python -r eval/bench/requirements.txt
PGHOST=127.0.0.1 PGPORT=55440 PGUSER=postgres OML_BIN_DIR=target/release \
BENCH_PYTHON=/srv/build/bench-venv/bin/python \
    eval/bench/run.sh .tmp/bench synthetic scifact nfcorpus locomo
```

Datasets download on first use into `BENCH_CACHE` (default `/srv/build/bench-data`). To keep
one lead up across many runs, start `boot.sh` yourself and call `bench.py` against its state
directory:

```sh
S=/srv/build/bench-state
eval/bench/boot.sh $S $(python eval/bench/bench.py workareas scifact locomo) &
python eval/bench/bench.py load --state $S scifact
python eval/bench/bench.py run  --state $S --out .tmp/bench scifact [--search bm25] [--limit 100]
python eval/bench/reference.py .tmp/bench scifact
python eval/bench/bench.py report .tmp/bench > eval/bench/results/tables.md
```

`boot.sh` drops its database when it is stopped. `run --limit N` asks N queries drawn with
the fixed `--seed` (default 20261104); the corpus is always loaded whole.

### The reranking pipeline

`bm25-rerank` needs the reranker's files, which are never committed: fetch them once with
`scripts/fetch_rerank_model.sh <dir>`, which checks the pinned digests, and boot with
`BENCH_PIPELINES="bm25 bm25-rerank" OML_SEARCH__RERANK__MODEL_DIR=<dir>`; the lead reads every
`OML_SEARCH__RERANK__*` setting from the environment it inherits. Raise
`OML_SEARCH__RERANK__TIMEOUT_MS` and `OML_SEARCH__RERANK__MAX_CONCURRENT` for a quality run, so
no rerank is skipped, and keep the defaults for a latency run. `run --workers N` asks N queries
at once. The raw rows record each recall's `rerank` outcome and each hit's relevance.

## What is measured

Each query is asked once per pipeline (`search`: `keyword-all`, `keyword-any`, `bm25`,
`bm25-trigram`), with `k` 100 and no `max_tokens`, one query at a time so the latency is a
single client's. From that one ranked list: **nDCG@10**, **R@10**, **R@100**, **RR@10** (MRR
cut at 10), **Success@1** and **Success@5**, each the ir_measures measure of that name. A
query whose recall served nothing scores 0 on every measure; pytrec_eval alone would drop it
from the mean. Beside them: the **zero-result rate**, and the **p50** and **p95** of the
client's wall time per `/mcp` call. The served order is the ranking: each hit's score is
replaced by its rank before scoring, so a pipeline's score scale never matters.

Relevance is grade ≥ 1 for every measure, except Success@k on the synthetic set, which counts
grade 2 ("answers it") as `eval/recall/metrics.py` does. The synthetic numbers differ from
`eval/recall`'s in the second decimal: pytrec_eval's nDCG gains the grade itself, where
`metrics.py` gains `2^grade - 1`, and a fresh load draws new artifact ids, which reorders ties.

## Results schema

Every committed result is one file, `results/<suite>/<method>.json`, in the schema
`oml-bench/1`, so the documentation site can render any suite without knowing which harness
produced it:

```json
{"schema": "oml-bench/1", "suite": "beir-scifact", "suite_version": "beir/scifact/test (ir_datasets 0.6.3)",
 "kind": "recall", "method": "bm25", "date": "2026-11-04", "commit": "<sha of the OML build measured>",
 "environment": {"host": "8 vCPU / 15 GB", "postgres": "18.6", "build": "release"},
 "n_queries": 300, "n_docs": 5183, "sampled": null,
 "metrics": {"ndcg@10": 0.0, "recall@10": 0.0, "recall@100": 0.0, "mrr@10": 0.0,
             "success@1": 0.0, "success@5": 0.0, "zero_result_rate": 0.0},
 "per_category": {"<category>": {"success@5": 0.0, "n_queries": 0}},
 "latency_ms": {"p50": 0.0, "p95": 0.0, "p99": 0.0},
 "reference": {"source": "BEIR BM25", "metric": "ndcg@10", "value": 0.665, "url": "https://arxiv.org/abs/2104.08663"},
 "notes": "the limits, in one sentence"}
```

- Suites: `recall-synthetic` (eval/recall's served results, converted), `beir-scifact`,
  `beir-nfcorpus`, `locomo`, `beir-scifact-x10`. Earlier runs move to
  `results/archive/<commit>/`; `scripts/gen_bench_pages.py` renders every file into the API
  docs' benchmark pages, current and earlier side by side. Methods: the four served pipelines, and the two offline
  references (`reference-bm25s-k1-0.9-b-0.4`, `reference-bm25s-k1-1.2-b-0.75`), whose
  `latency_ms` values are null because they are not a served path.
- `sampled` is null when every query was asked, else `{seed, fraction}`.
- A metric that was not measured is null (`recall@100` of `recall-synthetic`, which runs at
  `k` 10). `latency_ms` percentiles are nearest-rank over the per-query client wall time of
  every query asked (for `locomo`, adversarial included).
- For `locomo`, `metrics` is over categories 1-4; `per_category` holds each category,
  adversarial included, and `all-categories`.
- Numbers are as measured, rounded to 4 decimals at most.

```sh
python eval/bench/export.py --raw <bench.py --out dir> --commit <sha> --date 2026-11-04 \
    --postgres 18.6 --host "8 vCPU / 15 GB"
```

The raw per-query files `bench.py run` writes (ranked ids, latency, per-query scores) stay in
its `--out` directory and are not committed: LoCoMo's alone are several megabytes.

## Datasets and how each maps onto memories

OML's unit is one memory: a short, checkable claim. None of these benchmarks was built in that
unit, so each mapping is stated with what it costs.

| dataset | license | memories | queries | mapping |
|---|---|---:|---:|---|
| `synthetic` | this repository's | 200 | 121 | as `eval/recall` |
| `scifact` (BEIR test) | CC BY-NC 2.0 | 5,183 | 300 | one abstract is one memory: `<title>. <abstract>` |
| `nfcorpus` (BEIR test) | "free to use for academic purposes" | 3,633 | 323 | as SciFact |
| `arguana` (BEIR) | CC BY 4.0 | 8,674 | 1,406 | as SciFact; adapter only, not run (the queries are whole arguments) |
| `scifact-x10` | as SciFact | 51,830 | 300 | SciFact and nine copies with 10% of words replaced (seeds 20261105 to 20261113), one workarea, for latency at scale; qrels name the original copy |
| `locomo` (`locomo10.json`) | CC BY-NC 4.0 | 5,882 | 1,981 | one dialogue turn is one memory, `[<session date>] <speaker>: <text>`, with a shared photo's caption; one workarea per conversation |

- **BEIR.** A memory is an abstract, a median of about 1,400 characters, several claims long.
  BEIR's own BM25 indexes the title and the text as two fields with Anserini (k1 0.9, b 0.4);
  here both are one body. Relevance is BEIR's test qrels as published (NFCorpus has grades 1
  and 2).
- **LoCoMo.** Each of the ten conversations is its own haystack: its turns go in a workarea of
  their own and its questions are asked only there, so a question never sees another
  conversation. The qrels are each question's `evidence` dialogue ids; the five questions
  whose evidence names no turn are dropped (four name none, one names `D30:05`). Category 5 (adversarial: the question presumes
  something the conversation never says) is reported apart and left out of the headline,
  since its evidence turns are what a careful answer would decline from. Category names follow
  the LoCoMo repository's numbering (1 multi-hop, 2 temporal, 3 open-domain, 4 single-hop).
- **Not run.** LongMemEval (MIT, 500 questions) gives every question its own haystack of
  about 40 sessions (`_s`, about 115k tokens) with per-turn `has_answer` labels; mapped one
  turn per memory that is roughly 500 memories per question, 250,000 for the set, and a
  workarea per question. It is the next adapter, on a fixed subset. MemoryAgentBench (MIT
  code) scores answers, not retrieval, so it gives no qrels to score recall against. MTEB's
  retrieval tasks are BEIR's datasets again behind another loader.

Nothing downloaded is committed: the licenses differ and two forbid commercial use. The
results files hold ids and scores, never dataset text.

## Limits

- The loader is concurrent but takes one row per request, as the routes do: there is no batch
  write or batch verdict, and nothing is written to a table directly. With 8 writers and 6
  reviewers a 5,000-row dataset loads in about two minutes (the load logs in `results/` hold
  the times). Concurrent writes meet retryable `503` "chain head contention", which the loader
  retries and counts.
- Recall reads every readable row's stored lexemes per query: under row security no text index
  serves a read (ADR-0034 d13), so latency grows with the scope. `bm25-trigram`'s trigram list
  costs seconds per query at 5,000 abstracts; `run --limit N` asks a seeded subset, and the
  export marks it `sampled`.
- `run --k N` asks N hits instead of 100, to measure latency at the size an agent asks; its
  results export as `<method>-k<N>`.
- English only, and all lexical; there is no unanswerable query with a "nothing" label yet.
  REPORT.md lists what the harness needs for S4-S7.
- CI: `.github/workflows/bench.yml` runs synthetic and SciFact weekly and on demand, and
  reports without gating. Pull requests keep the `recall-eval` job, which downloads nothing.
