# Scale bench: candidate search at 10k, 100k and 1M memories per tenant

This bench measures how each candidate search approach for OML's recall scales:
latency (cold and warm, p50/p95/p99), index build time and size, and write
cost, on OML's real schema with its real fences. It measures speed and cost
only. Relevance is the recall eval's job (`eval/recall/` on `feat/recall-eval`),
and nothing here says which approach finds the right memory.

The findings are in [REPORT.md](REPORT.md). The generated tables are in
[results/tables.md](results/tables.md), the EXPLAIN excerpts in
[results/explain.md](results/explain.md), and the raw numbers in `results/*.json`.

## Files

| file | what it does |
|---|---|
| `gen.py` | builds synthetic memories, workarea trees, readable scopes, queries and write texts from the recall eval's seeds |
| `seed_corpus.json`, `seed_queries.json` | `eval/recall/corpus.json` and `queries.json`, data version 1, copied from `origin/feat/recall-eval` |
| `run.py` | runs one scale: creates the database, migrates it, loads the data, builds each approach, measures it, writes the results JSON |
| `run_all.sh` | the sequence that produced `results/`: 10k, 100k, 10 tenants × 100k, 1M text (resumed once, `s1m_text2.json`), 1M vectors (halfvec and float), the stored-tsvector supplement at 10k and 100k, and the two added cases |
| `extra.py` | the added cases: exact halfvec scan against HNSW (with and without iterative scan) under 10% and 1% readable shares, and a learned-sparse inverted table |
| `report.py` | turns `results/*.json` into `results/tables.md`, `results/p95.svg`, `results/explain.md` and the `oml-bench/1` files `results/<method>/<rows>.json` the docs generator renders |

## Method

**Cluster.** The bench used a throwaway PostgreSQL 18.6 cluster made with
`initdb`. Its data directory was `/srv/build/pg-scale/data`, it listened on
127.0.0.1:55439, and it used the system's binaries. It was not the system
cluster. pgvector 0.8.1 came from the distribution's `postgresql-18-pgvector`
package, fetched with `apt download` and unpacked under `/srv/build/pgext`.
PostgreSQL 18's `extension_control_path` and `dynamic_library_path` loaded it
without any change to the system. The cluster settings were
`shared_buffers 2GB`, `effective_cache_size 8GB`, `work_mem 16MB`,
`maintenance_work_mem 1GB` (4GB for the HNSW build), `random_page_cost 1.1`,
`jit off` and `wal_level minimal`. The host has 8 AMD EPYC-Genoa vCPUs and
15 GiB of memory, and the data sits on an ext4 loop filesystem on a Hetzner
volume.

**Schema and roles, as the lead runs them.** Each run creates a fresh
database owned by `oml_owner`, a login role that is not a superuser. Every file
in `migrations/` is applied in order as that role, with
`oml.serving_role = 'oml_serving'`, as `PgStore::migrate` applies them. The
result is the 0007 grant matrix, the 0008 tenant keys, the 0009 per-tenant list
partitions with `ENABLE` and `FORCE ROW LEVEL SECURITY`, and the
`tenant_isolation` policy on each partition. Each tenant's partition is built
the way `create_partition_on` builds it: `LIKE memories`, the same policy, then
`ATTACH`. Every measured query runs as `oml_serving`, in a transaction that
first calls `set_config('oml.tenant_id', <tenant>, true)`, as `set_tenant`
does. Every statement is server-prepared (`prepare_threshold=0`), as sqlx's
persistent statements are.

**Bench additions to the schema, and why.**

- `memories.idents text` holds the identifier-like tokens of the claim,
  computed by the application. It exists only for approach e's identifier
  index.
- The bulk load COPYs each tenant's rows into a fresh table, builds today's
  indexes on it (timed), then attaches it. `memories_created_event_id_fkey` is
  dropped before the load and re-added `NOT VALID`. The loaded rows therefore
  need no journal rows, while every write in the write-cost runs is still
  checked against `journal`.
- Approach c adds two stored generated tsvector columns and a GIN index on
  each, and drops today's expression GIN index, which the stored simple column
  replaces.
- Approach d adds `bm25_df (tenant, workarea, term, df)` and
  `bm25_scope (tenant, workarea, n, len)`. Both carry row security and the
  `tenant_isolation` policy, and an `AFTER INSERT OR UPDATE` trigger maintains
  them.
- Approach f adds `memory_vectors`, list-partitioned per tenant and fenced by
  the same policy, with an HNSW index on each partition.

**Data.** `gen.py` derives every memory from one of the recall eval's 200
seed texts with templated variation:

- numbers are drawn again;
- identifiers are mutated or replaced from a pool that grows by about one
  identifier per six memories, so the vocabulary grows with the corpus, Zipf
  distributed;
- topic words are swapped within the topic;
- 35% of memories get a second clause from another seed on the same topic;
- some get ticket, host or version references.

The mean claim is 176 characters (the seeds' mean is 121), and the 90th
percentile is 277. Each tenant has a tree: `universe`, then projects, then
leaves, with about one workarea per 2,000 memories and at least 20. That gives
20 workareas at 10k, 50 at 100k and 500 at 1M. Leaf sizes follow a Pareto
distribution.

| state | share |
|---|---:|
| promoted served tips | 65% |
| promoted replaced versions | 10% |
| held | 15% |
| rejected | 6% |
| demoted | 4% |

| kind | share |
|---|---:|
| memory | 85% |
| derived_fact | 9% |
| steering | 5% |
| guardrail | 1% |

About 1% of tips are retirements.

**Queries.** There are 330 queries per run, 30 in each of the eval's 11
categories: exact, paraphrase, inflection, partial, typo, identifier,
question, long, negation, temporal and multihop. Each category holds its 11
seed queries as written, plus variants with identifiers and numbers swapped
from the corpus generator's pool. Each query draws one readable scope as the
lead computes it, subtree(scope) ∩ readable plus `universe` (ADR-0044 d10):

| scope | share | workareas read |
|---|---:|---|
| a leaf | 40% | the leaf and `universe` |
| a project | 40% | the project, its leaves and `universe` |
| `universe` | 20% | every workarea |

The other parameters are kinds `memory` and `derived_fact`, and `k` 10.

**Approaches.** The SQL for each is in `run.py`, in `APPROACHES`.

| id | statement |
|---|---|
| `a_simple_and` | `RECALL_FTS` verbatim: `to_tsvector('simple') @@ plainto_tsquery`, `ts_rank`, with today's GIN expression index |
| `b_simple_or_rank`, `b_simple_or_rank_cd` | the same with the query's lexemes joined by OR, ranked by `ts_rank` or `ts_rank_cd` |
| `c_dual_or` | stored `english` and `simple` tsvectors with a GIN index each; any word on either; the sum of both `ts_rank`s |
| `d_bm25_querytime` | BM25 (k1 1.2, b 0.75) over the readable served rows; N and avgdl counted over the readable rows and df over the candidates, all at query time under row security |
| `d_bm25_maintained` | the same score with N, length and df read from the maintained per-workarea tables, summed over the readable workareas |
| `e_trgm_text`, `e_trgm_ident` | pg_trgm `word_similarity` (`<%`, threshold 0.6) on the claim and on its identifiers, each with GIN `gin_trgm_ops` |
| `f_hnsw` | pgvector 0.8.1 HNSW (m 16, ef_construction 64, ef_search 40, `iterative_scan = relaxed_order`), cosine, 384 dimensions, with the same scope filter |
| ADR-0050 check | per write, the served-tip and own-held any-word recalls with `k` 3 and the new text as the query, at a leaf's scope |

**Measurement.**

- **Cold pass.** Each approach first runs a cold pass: the cluster is stopped,
  `POSIX_FADV_DONTNEED` is issued over every file in the data directory and
  over the loop image that backs `/srv/build` (the host's page cache would
  otherwise serve the loop device), the cluster is started, and the 330
  queries run once. Later queries in the pass find a warmer cache, so this is a
  cold start, not 330 cold queries. `first query` in the tables is the one
  truly cold query.
- **Warm pass.** The 330 queries then run again.
- **Timing.** Latency is the client-side time of the SELECT alone, excluding
  `BEGIN`, `set_config` and `COMMIT`. One client runs with no concurrent load.
  The statement timeout is 10 s, and a timed-out query counts as 10 s.
- **No-RLS comparison.** Approaches a, b, c and e also run warm as the
  superuser with the same tenant predicate. That is what the plan would be
  without the policy, and OML may not run that way.
- **EXPLAIN.** `EXPLAIN (ANALYZE, BUFFERS)` runs on one universe-scope query
  per category (exact, long, identifier, typo) for each approach.
- **Write cost.** 300 writes run as the serving role with
  `synchronous_commit off`. Each write is a journal row and a held memories
  row in one transaction, then the promotion UPDATE in a second. Turning
  synchronous commit off leaves the statement and index work in the figure and
  takes out the fsync, which costs the same for every approach. The stages are
  measured cumulatively: base, then base plus the BM25 trigger (dropped
  afterwards), then dual, then dual plus trigram. The vector insert is measured
  on its own table.
- **Index build.** Each index's build time is a single `CREATE INDEX`, and its
  size is `pg_relation_size`.

## Limits

- **Synthetic text.** It is templated from 200 seeds written by one model.
  Its vocabulary grows with N, but more slowly and more regularly than a real
  team's would, and some swapped sentences read as nonsense. Posting-list
  lengths for common words (the long, question and negation queries) are
  realistic in shape, but not calibrated against a real OML tenant, because
  none this size exists.
- **Synthetic embeddings.** Each vector is the seed text's centre (a topic
  centre plus a per-seed offset) plus Gaussian noise, normalised. They cluster
  as model embeddings do, but no model produced them. The HNSW latency and
  size figures carry over; the recall@10 figure measures the index against
  exact search on these vectors and says nothing about semantic quality.
- **One host, one client.** There is no concurrency, so these are service
  times, not throughput. The host's other tenants (the live server's cluster
  and other builds) shared CPU and the volume during the runs. The p99 values
  carry that noise.
- **Imperfect cold cache.** The cache is evicted by advice
  (`POSIX_FADV_DONTNEED`), not by dropping the kernel cache. The volume is
  network block storage, and a cold read from it is slower than from a local
  NVMe disk.
- **The 1M runs are split.** The vector runs (halfvec, then float) loaded the
  memories with today's indexes only, because the 18 GB loop filesystem could
  not hold every approach's indexes at 1M at once. The 1M text run stopped at
  the BM25 correctness check (a 5 s statement timeout, since lifted for that
  check) and resumed on a fresh load with the same seed (`s1m_text2.json`);
  `report.py` merges the two.
- **1M uses a 5 s statement timeout** (10 s elsewhere). A timed-out query
  counts as 5,000 ms and is marked `≥5,000†`, so a p95 at the timeout is a
  lower bound, and so is any log-log slope fitted through it. The no-RLS
  comparison at 1M is one pass after the RLS passes, not a warm-up and a pass.
- **The multi-tenant run predates the stored-tsvector variants** and per-scope
  timing, so it has neither.
- **The learned-sparse case stops at 300k.** 60 postings per memory is 60M
  rows at 1M, more than the build filesystem holds; the 1M row is a log-log
  extrapolation and is marked as one, in the tables and in the
  `oml-bench/1` files (`"extrapolated": true`).
- **Vector recall comes from added case 1 only.** The main runs' recall check
  disabled index scans for its exact side, and PostgreSQL 18 kept the HNSW plan
  (marked `Disabled: true`), so it compared the index with itself. `extra.py`
  forces the exact side with `ORDER BY (embedding <=> q) + 0`.
- **1M float `vector(384)` was not measured.** The other builds sharing
  `/srv/build` left too little space. 1M is measured as halfvec, and the float
  1M `oml-bench/1` point is a two-point extrapolation marked
  `"extrapolated": true`.
- **Write measurements are 300 writes per stage.** GIN's pending list
  (`fastupdate`, 4 MB) is flushed by some of them, which is part of the p95.

## Reproduce

```sh
# a throwaway cluster
initdb -D /srv/build/pg-scale/data -U postgres --auth=trust -E UTF8 --locale=C.UTF-8
# append the settings above (port 55439, extension_control_path, ...) to postgresql.conf
pg_ctl -D /srv/build/pg-scale/data -l /srv/build/pg-scale/log start
psql -h 127.0.0.1 -p 55439 -U postgres -c "CREATE ROLE oml_owner LOGIN" -c "CREATE ROLE oml_serving LOGIN"
# pgvector without touching the system
apt download postgresql-18-pgvector && dpkg-deb -x postgresql-18-pgvector_*.deb /srv/build/pgext

./run_all.sh                  # about 3 hours on this host
python report.py
pg_ctl -D /srv/build/pg-scale/data stop && rm -rf /srv/build/pg-scale /srv/build/pgext
```

The Python dependencies are `psycopg` 3 and `numpy`.
