# Scale bench: the postings lane

This directory measures `bm25-postings` ([ADR-0052](../../docs/adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md))
against `bm25` at a given number of memories per tenant, on OML's real schema and migrations,
as the serving role under row security. It measures speed, space and write cost, and how far
pruning moves the list from `bm25`'s; relevance on real text is `eval/bench/`'s job.

The data and the queries come from the scale lane's generator (`gen.py`, `seed_corpus.json`,
`seed_queries.json`, from the baseline lane below, measured on 2026-11-04): synthetic memories templated from the recall eval's 200 seeds, one workarea per
about 2,000 memories under `universe` and its projects, 65% promoted served tips, 15% held, the
rest replaced, rejected or demoted; 330 queries, 30 in each of the eval's 11 categories, each
with a leaf (40%), project (40%) or `universe` (20%) scope as the lead computes it. The findings
are in [REPORT.md](REPORT.md); the numbers in `results/postings-*.json`.

## Files

| file | what it does |
|---|---|
| `gen.py`, `seed_*.json` | the scale lane's generator and seeds |
| `run.py`, `extra.py`, `run_all.sh`, `report.py` | the baseline lane: every candidate approach at 10k, 100k and 1M ([BASELINE.md](BASELINE.md)) |
| `postings.py` | one run at one scale: database, migrations through 0020, load, 0021's fill timed, space, the query sweep, the access-index variant, own held rows and write cost |
| `../../crates/adapters/oml-pg/examples/search_scale.rs` | the driver: each query through `PgStore::retrieve`, the adapter's own statements, warm, one JSON line per call |

## The baseline lane

Before ADR-0052, the same generator measured every candidate search approach at 10k, 100k and 1M
memories per tenant on migrations 0001–0016 under row security: today's `simple` match, any-word
OR, stored `english` and `simple` vectors, BM25 in SQL, `pg_trgm`, pgvector HNSW, an exact
halfvec scan, a learned-sparse inverted table and ADR-0050's similar check. That lane found that
no text index serves a recall under row security, which ADR-0049 and ADR-0052 cite. Its method
and limits are in [BASELINE.md](BASELINE.md), its findings in
[REPORT-baseline.md](REPORT-baseline.md), and its numbers in `results/` beside the postings runs
(`s*.json`, `x_*.json`, `mt10x100k.json`, `tables.md`, `explain.md`, `p95.svg`, and one
`oml-bench/1` file per approach and scale under `results/<method>/`).

## Running it

A throwaway PostgreSQL cluster with a superuser, never the system's, and a scratch venv with
`psycopg`:

```sh
cargo build --release -p oml-pg --example search_scale
uv venv .tmp/scale-venv && uv pip install --python .tmp/scale-venv/bin/python "psycopg[binary]"
PGHOST=127.0.0.1 PGPORT=55481 PGUSER=<superuser> \
SEARCH_SCALE=target/release/examples/search_scale \
    .tmp/scale-venv/bin/python bench/scale/postings.py --rows 100000 \
    --out bench/scale/results/postings-100k.json
```

Each run creates its own database and role pair and drops them when it ends (`--keep` keeps
them). The load writes rows with a `COPY` into the tenant's partition before it is attached,
computing each row's stored lexemes with 0020's `oml_search_vector`, then applies 0021, so the
fill time is 0021's own over that many rows; it then vacuums the postings partition, as
autovacuum would, so the visibility map lets the served index answer without the heap. The
`created_event_id` key is dropped before the load so loaded rows need no journal rows; nothing
else in the schema differs.

## What it measures

- **Latency**: each query twice through the driver, the second timed (warm), as the wall time of
  one `retrieve` call: its transaction and every statement it runs. `bm25`, `bm25_postings` with
  nothing pruned (which returns `bm25`'s list), and `bm25_postings` at each pair of prune
  constants in `SWEEP` and at the compiled ones. Per scope p50 and p95, the share answered by the
  scan, the share with a word pruned, and against `bm25`: the share of identical lists and the
  mean overlap of the top 10.
- **The access index** (ADR-0052 open question 1): the same queries with d1's first index,
  `(kind, token_id)` over every posting, in place of the served covering index, and its size.
- **Own held rows** (open question 7): a key holding 1,000 and 10,000 held rows, recalled with
  them, under `bm25` and `bm25_postings`.
- **Space**: the postings partition's heap and both indexes, the two count tables, and the
  tenant's memories heap.
- **Write cost**: a held insert, its promotion and a supersession, each in its own transaction as
  the serving role, with the postings trigger and with it disabled.
- **The fill**: 0021 applied over the loaded rows, in seconds.
