---
status: Stable
verified: 2026-11-11
answers: "how to run OML's recall eval against a build of your own, what it needs, what passing means, and how to compare search pipelines on it"
---

# Measure recall on your build

The recall eval loads 200 memories and 121 labelled queries into a server built from your checkout, runs every query through `oml_read` once per search pipeline, and compares the numbers with the ones committed in the repository. Run it after you change the server, or to see what `bm25-sparse` would gain over the default before you load its model. The data and the metrics are described in [`eval/recall/README.md`](https://github.com/e8kor/open-memory-lab/blob/main/eval/recall/README.md).

It measures OML's ranking on a fixed, made-up corpus, not your own memories. A pass says your build ranks as the released one does.

## What you need

- A checkout of the repository and Rust through `rustup`.
- `psql`, `jq`, `curl`, `sha256sum` and Python 3. The eval's scripts use only Python's standard library.
- A PostgreSQL superuser on a cluster you can throw away. The script creates a database and two roles, and drops them when it ends. The compose database is one: `docker compose up -d postgres`.
- About 350 MB for the two model files, which go to `.tmp/oml-models`, plus a release build of `oml-server-lead` and `oml`.

## Run it

```sh
docker compose up -d postgres
PGPORT=5432 eval/recall/selfhost.sh
```

The script builds the two binaries, fetches the reranker and the sparse encoder from their pinned revisions and checks each file's SHA-256, boots a server on `127.0.0.1:8437` against a new database, loads the corpus with one key and approves it with another, and runs the queries under eight pipelines. Results go to `.tmp/recall-eval`, one `served_<pipeline>.json` each.

A missing tool, or no `PGPORT`, stops it before it builds anything, and the message names what is missing. A model file that does not match its pinned digest stops it before the server starts.

| Variable | Default | Effect |
|---|---|---|
| `PGPORT` | none | The throwaway cluster's port. Required, so the script never reaches a server you did not name. |
| `PGHOST`, `PGUSER`, `PGPASSWORD` | `127.0.0.1`, `oml`, `oml_dev_only` | The superuser the script connects as. |
| `EVAL_PIPELINES` | all eight CI measures | For example `"bm25-postings bm25-sparse"` to compare the default with the recommended pipeline. A list without `bm25-rerank`, `sparse` or `bm25-sparse` fetches no model. |
| `EVAL_DEFAULT` | `bm25-postings` | The pipeline that must pass its gate. |
| `OML_BIN_DIR` | build from the checkout | A directory holding `oml-server-lead` and `oml` already built, such as a release's. |
| `EVAL_MODELS` | `.tmp/oml-models` | Where the model files are kept. A file already there is checked, not fetched again. |

## What passing means

The script exits 0 when the default pipeline meets its gate (success@5 at least 0.86 and success@1 at least 0.67), when no category of it falls by more than one query against the committed result, and when `keyword-all` reproduces the v0.5.0 recall. Every pipeline's summary is printed, and a pipeline other than the default that misses its gate is reported without failing the run.

To compare two pipelines, read `summary.overall` in their files. On the committed run, `bm25-sparse` puts an answer in the top five for 0.983 of the queries and `bm25-postings` for 0.934 (`eval/recall/results/served_bm25_sparse.json`, `served_bm25_postings.json`). Latency in these files is one client on loopback against 200 memories; [latency at scale](../benchmarks/scale.md) covers larger scopes.

## In CI

The `recall-eval` job runs this script, with the binaries it built and the model files it caches, so the command on this page is the one that gates a change. It runs when a pull request changes the Rust build's inputs (`crates/`, `migrations/`, the API contracts) or `eval/`, and on every push to `main`. A change to the documentation alone does not run it.
