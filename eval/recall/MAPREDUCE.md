# Map-reduce recall: each workarea ranks its own rows, a reducer merges them

**Asked** 2026-11-11 by the operator: can `bm25-sparse` run per workarea, in parallel, and the
lists be merged like a map-reduce? **Run** 2026-11-12, branch `research/sparse-mapreduce` from
`e5217966`, offline. Research only: no served path changes, no ADR changes status.

**Answer.** Yes, exactly, and ADR-0073 d9 already designs it. The sparse leg needs nothing
global: its score is the model's document weight times `idf.json`'s query weight, so a
workarea's scores are the union's scores, and a merge by score returns the global answer
byte for byte. The BM25 leg needs the union's `n`, `avgdl`, per-form document frequencies and
pruned words, which are sums of the per-workarea counts OML already keeps. The fusion needs
each leg's minimum and maximum over its merged top 100, so the reducer must fuse, never the
workarea. With those three rules the merged answer equalled the global one on every query of
six partitions, scores included. Every shortcut lost quality: merging per-workarea fused
answers by score or by reciprocal rank lost up to 0.79 of success@5 at 500 workareas.

On one PostgreSQL it buys latency only from idle cores. Cutting a large scope into 8 groups,
one statement each on its own connection, took a lone recall's sparse leg at 100,000 memories
from 100.7 to 31.8 ms p95. With 8 clients on the 8 vCPUs the same split served 8 to 14% fewer
recalls a second at no better p95, and one statement per workarea cost 220 to 330 ms at 500
workareas at every size. The lever for the sparse leg on one database stays #104's bound on the
postings a query reads; fan-out is the shape for ADR-0073's nodes, where each shard reads on its
own cores.

The prototype: [`mapreduce.py`](mapreduce.py) (quality, standard library) and
[`../../bench/scale/fanout.py`](../../bench/scale/fanout.py) (latency, psycopg). Results:
[`results/mapreduce.json`](results/mapreduce.json) and
[`../../bench/scale/results/fanout.json`](../../bench/scale/results/fanout.json).

## 1. Map: what a workarea can compute alone

What each served pipeline's score reads, from the code:

| leg | score of a row | statistic it reads | can a workarea compute it alone? |
|---|---|---|---|
| `sparse` | `sum(q.w × p.weight)`: `p.weight` the model's document weight, written once per version; `q.w` the query token's weight from the model's `idf.json` (`oml-rerank` `sparse.rs`, `encode_query`) | none: "no corpus statistic is read" (`oml-pg` `sparse_sql`) | **yes**, and its score is the union's |
| `bm25-postings` | BM25, k1 1.2, b 0.75, `idf = ln(1 + (n − df + 0.5) / (df + 0.5))`, best form per word (`bm25_postings_sql`) | `n`, `avgdl`, `df` per form, passed as `$7`–`$9`; the pruned words, decided from `df` against `FREQUENT_SHARE` and `POSTING_BUDGET` | **only with the union's statistics**: `n`, the summed lengths and each `df` are sums of `search_workarea_rows` and `search_term_rows` over the workareas; pruning is a function of those sums |
| `bm25-sparse` | `0.3 × norm(bm25) + 0.7 × norm(sparse)`, each list min-max normalised over its own top 100 (`Fusion::Cc`, `CC_DEPTH`) | each leg's maximum and its 100th score | **no**: the bounds belong to the merged lists. The reducer fuses |

So the map step is: round 1 sums the per-workarea counts for the query's forms (ADR-0072 d4.2
already does this for `wide`, 10.9 ms p95 at 1,000 candidate workareas, d12) and fixes the
pruned words; round 2 has each workarea return each leg's list with raw scores. That is
ADR-0073 d9's two rounds as written.

The learned sparse leg has no IDF over the corpus; its `idf.json` is a constant shipped with the
model. A sparse score is therefore stable under any partition, including a partition across
nodes, and the measurement agrees: `sparse` `exact` equals `global` with identical scores on all
six partitions.

## 2. Reduce: how to merge, and how deep each workarea must return

**When scores are comparable, merge by score.** Each leg merged by score (ties by id), cut at
100, then fused at the reducer, equals the global answer. Measured: `exact` matches `global`'s
top 10 *and* its scores on 100% of queries, every pipeline, every partition (tables below).

**How deep.** A single-list pipeline needs `k` per workarea: the global top `k` is inside the
union of the workareas' top `k`. A CC pipeline needs 100 per leg per workarea, because the
bounds are the merged 100th score. That fixed depth ships `N × 200` rows at most. A threshold
round (Fagin's idea; ADR-0073 d9 does not have it yet) cuts it while staying exact: each
workarea returns `d0 = max(10, ⌈200 / N⌉)` per leg, the reducer takes the 100th best score it
holds as a threshold, and only a workarea whose `d0`-th score is at or above it is asked for the
rest. Rows shipped per `bm25-sparse` query, mean:

| partition | workareas | fixed depth (100 per leg) | threshold round | queries needing a second round | depth one workarea needed, mean / p95 / max |
|---|---:|---:|---:|---:|---|
| topics | 11 | 133 | 131 | 8.3% | 11.8 / 20 / 21 |
| skew | 11 | 286 | 284 | 96.7% | 79.0 / 97 / 99 |
| scifact-5 | 5 | 999 | 400 | 0% | 26.4 / 31 / 34 |
| scifact-50 | 50 | 7,705 | 999 | 0.7% | 6.3 / 8 / 10 |
| scifact-zipf-50 | 50 | 4,596 | 1,409 | 100% | 24.2 / 30 / 36 |
| scifact-500 | 500 | 7,846 | 7,690 | 0% | 2.6 / 3 / 4 |

The threshold round matters when workareas are many and remote: at 50 workareas it ships 7.7
times fewer rows for the same answer. At 500 the floor of 10 per leg dominates; a floor of
`⌈200 / N⌉` with no minimum would ship about 3,000. Under skew (one workarea holding 93% of the
rows) the large workarea needs nearly the full 100 every time, so no protocol saves it work.

**When scores are not comparable, nothing recovers the global answer.** Three ways a reducer
could merge per-workarea answers that each workarea fused on its own statistics, against the
global answer, `bm25-sparse`, success@5 (global, then each):

| partition | global | local statistics, legs merged, fused at the reducer | fused per workarea, merged by score | merged by reciprocal rank (k 60) | merged by CORI |
|---|---:|---:|---:|---:|---:|
| topics, 11 | 0.975 | 0.975 | 0.793 | 0.545 | 0.917 |
| skew, 11 | 0.967 | 0.950 | 0.802 | 0.595 | 0.884 |
| scifact-5 | 0.803 | 0.803 | 0.780 | 0.760 | 0.700 |
| scifact-50 | 0.803 | 0.790 | 0.350 | 0.160 | 0.620 |
| scifact-zipf-50 | 0.803 | 0.803 | 0.317 | 0.140 | 0.653 |
| scifact-500 | 0.803 | 0.783 | 0.040 | 0.013 | 0.597 |

- **Per-workarea fusion is the error.** A workarea's CC maps its own best row to 1.0, so every
  workarea's best looks equally good. At 500 workareas the merged top 10 is ten workareas'
  firsts, whatever they hold: success@5 0.04.
- **Reciprocal rank across workareas is interleaving.** It ranks each workarea's first above
  every workarea's second. It is the right tool across legs over one collection (`bm25-sparse-rrf`
  fuses after the exact merge), not across workareas.
- **CORI** (Callan, Lu and Croft, SIGIR 1995) reweights by a workarea's belief for the query and
  recovers part of the loss, never all of it, and it adds a second score that ADR-0072 d7 would
  have to explain.
- **Local BM25 statistics alone** cost little once the reducer fuses: 0.000 to 0.020 of success@5
  on `bm25-sparse`, because the 0.7 sparse weight carries the order. On `bm25` alone they cost
  more: under skew success@5 falls from 0.926 to 0.479 (a rare word in a small crew outranks the
  same word in `universe`), and at 500 workareas from 0.730 to 0.613. The round that fixes the
  statistics is cheap (section 1), so there is no reason to skip it.

Full tables, `nDCG@10`, top-10 overlap and agreement for `sparse` and `bm25` too:
`python3 eval/recall/mapreduce.py run …` prints them; the numbers are in
[`results/mapreduce.json`](results/mapreduce.json).

### The partitions and the checks

- **topics**: the 200-memory corpus, one workarea per topic (11), its 121 answerable queries.
  The offline `sparse` scores 0.967 success@5 and 0.891 nDCG@10, exactly the served `sparse`
  (`results/served_sparse.json`), so the offline sparse leg is the served one. The offline BM25
  uses a light suffix stemmer in place of Snowball (0.926 / 0.833 against the served
  `bm25-postings`' 0.934 / 0.873), and its fused `bm25-sparse` scores 0.975 / 0.914 against the
  served 0.983 / 0.910. Every variant uses the same analyzer, so the comparisons hold; the
  absolute BM25 numbers are not the served ones.
- **skew**: the same plus 2,400 SciFact abstracts in `database`, as `fed2.py` pads it.
- **scifact-N**: BEIR SciFact, 5,183 abstracts and 300 test queries, cut uniformly at random into
  5, 50 and 500 workareas (seed 20261111), and **scifact-zipf-50**, 50 workareas sized 1/rank
  (1,150 to 23 rows). The sparse model encoded all 5,383 texts through `sparse_encode` at the
  pinned revision (digests checked), 2.3 to 9.4 texts a second on 6 threads on a shared host.
- Not tested: pruning. At these sizes no query's postings cross `POSTING_BUDGET`, so the pruned
  words never differ between a workarea and the union. A workarea deciding its own pruning would
  be a fourth source of disagreement; d9's round 1 fixes pruning with the statistics.

## 3. Does it help latency?

Measured on one throwaway PostgreSQL 18.6 on an 8 vCPU host shared with other lanes (load
average 2.0 to 2.9 during the run), the sparse leg's statement without row security, `LIMIT 100`
(`CC_DEPTH`), 100 SciFact queries (19.6 query tokens on average; the recall eval's have 7.9),
the real doc-v3-distill postings (122.6 per memory) reused by digest at 1,000, 10,000 and 100,000
memories (118,588 to 12,259,847 postings; 580 MiB of covering index at 100,000). Every mode ran
on each query in turn, the order rotated per query, and each timed run is the second. p50 / p95
ms per recall, one client; every mode returned the same top 100 as `single` on every query:

| memories | workareas | `single` | `single-parallel` | `fanout-each` | `fanout-groups-2` | `fanout-groups-4` | `fanout-groups-8` |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 5 | 1.6 / 2.7 | 12.0 / 13.6 | 2.7 / 4.2 | 1.8 / 2.6 | 2.4 / 3.4 | – |
| 1,000 | 50 | 1.8 / 3.5 | 12.3 / 14.1 | 23.4 / 30.7 | 1.9 / 3.7 | 2.6 / 3.9 | 4.6 / 6.5 |
| 1,000 | 500 | 3.4 / 4.8 | 14.3 / 16.2 | 223.4 / 291.3 | 3.0 / 4.1 | 3.5 / 4.6 | 6.1 / 8.1 |
| 10,000 | 5 | 8.2 / 15.5 | 12.2 / 15.2 | 4.5 / 8.8 | 6.7 / 14.2 | 5.6 / 9.8 | – |
| 10,000 | 50 | 9.1 / 19.6 | 8.3 / 12.7 | 28.9 / 38.1 | 6.3 / 13.1 | 5.1 / 9.5 | 5.7 / 7.9 |
| 10,000 | 500 | 10.5 / 17.4 | 13.8 / 16.2 | 230.6 / 291.5 | 8.3 / 14.9 | 6.7 / 12.2 | 7.1 / 10.8 |
| 100,000 | 5 | 34.7 / 89.6 | 35.5 / 90.2 | 20.4 / 34.3 | 25.7 / 54.9 | 24.7 / 42.3 | – |
| 100,000 | 50 | 41.5 / 100.7 | 41.4 / 102.9 | 28.8 / 38.4 | 23.2 / 59.2 | 25.9 / 38.4 | 18.1 / 31.8 |
| 100,000 | 500 | 43.5 / 108.9 | 43.7 / 110.7 | 239.8 / 328.6 | 31.6 / 60.8 | 29.8 / 56.4 | 24.2 / 37.5 |

`single` is today's shape; `single-parallel` lets PostgreSQL's parallel query take the statement
(4 workers per gather, its costs zeroed); `fanout-each` is one statement per workarea on a pool
of 8 connections; `fanout-groups-P` cuts the workareas into P groups, one statement per group,
each merged by score.

Under load, at 100,000 memories and 50 workareas, each client running all 100 queries once after
a warm pass (`results/fanout-throughput.json`):

| clients | mode | recalls a second | p50 ms | p95 ms |
|---:|---|---:|---:|---:|
| 1 | `single` | 23.4 | 39.8 | 95.3 |
| 1 | `fanout-groups-4` | 60.2 | 13.9 | 40.2 |
| 1 | `fanout-groups-8` | 104.8 | 9.1 | 16.6 |
| 4 | `single` | 89.9 | 41.8 | 95.7 |
| 4 | `fanout-groups-4` | 146.3 | 26.0 | 53.3 |
| 4 | `fanout-groups-8` | 138.1 | 26.0 | 55.0 |
| 8 | `single` | 172.5 | 42.8 | 100.2 |
| 8 | `fanout-groups-4` | 158.5 | 46.4 | 103.0 |
| 8 | `fanout-groups-8` | 147.9 | 48.6 | 104.9 |

What the numbers say:

- **Below 10,000 memories nothing gains.** The statement takes 2 to 5 ms; the split adds a
  round trip and a plan per statement.
- **One statement per workarea is never worth it past a handful.** At 500 workareas it takes
  220 to 330 ms whatever the size: 500 round trips on 8 connections.
- **A few groups help a lone recall at 10,000 and 100,000 memories**: p95 2.6 to 3.2 times lower
  at 100,000 (100.7 to 31.8 ms at 50 workareas). The gain is partly more cores and partly smaller
  aggregates: each group sums and sorts its own candidates, and a common query token's candidates
  are most of the scope.
- **It adds no capacity.** With 8 clients on 8 vCPUs every core is already busy, and the split
  serves 8 to 14% fewer recalls a second at the same p95. Its gain is borrowed from idle cores,
  and it holds P connections per recall while it lasts.
- **PostgreSQL's own parallel query does not do it.** The planner took a Gather at every point
  and it did not help, or cost about 10 ms of worker start at the small sizes.

Two further reads cost less than splitting a scope. Today `bm25-sparse` reads its two lists one
after the other (`bench/scale/REPORT.md`: 272 ms p95 at 100,000 memories for both lists, 100
deep); reading them on two connections at once makes the recall cost its slower leg, not the
sum. Not measured here. And #104's bound on the postings a query reads lowers the work itself,
which the split only spreads.

**Across nodes it is the only design.** Under ADR-0073 phase 3 a scope spans units on different
nodes, and no single statement can read them. There each source reads its own postings on its
own cores, so the capacity argument above does not apply, and the cost is the slowest source plus
two network rounds, which d9 bounds with hedging. The threshold round in section 2 keeps round
2's payload near 1,000 rows at 50 sources instead of 7,700.

## 4. How it composes

- **R3, visibility per shard.** Each shard's statement filters `workarea_id = ANY(readable)` and
  `state = 1` under row security, as today; a shard never returns a row its caller cannot read,
  and round 1's counts are over R, which is a subset of what the key reads (ADR-0072 d4.2, d6).
  The exact merge adds no new surface: a row's score depends on the row, the query and R's
  statistics, so no unreadable row moves a readable one. The shortcuts in section 2 break that
  property in a different way: a workarea's own statistics or bounds would make a hit's score
  depend on which workarea it lives in and what else that workarea holds.
- **ADR-0072 federation.** d7.1 (score independence) holds for the exact merge, and the
  measurement is its test 6 extended to `sparse` and `bm25-sparse`. d7.2 refuses `Cc` under
  `wide` and a list "until its bounds are taken over R". Merging each leg over R to 100, fusing
  at the reducer and restricting to the searched sources after fusion is exactly that, and it
  needs no fan-out: on one database each leg is one statement over R, as the `Rrf` path already
  reads its legs (phase 1 as built, d7.2 review M4). `fed2.py`'s replay already computes it
  offline.
- **ADR-0073 distributed nodes.** d9 is this map-reduce: round 1 statistics from each unit's
  replica, round 2 each retriever's list "to the pipeline's depth for that retriever (… at least
  100 for `Cc` …), never just `k`", merged per retriever by score, fused at the coordinator. The
  prototype confirms d9's exactness claim for the sparse retriever and the fused pipeline, and
  adds the depth data above.

## 5. Recommendation

**Do not build per-workarea fan-out on one database now.** One statement per workarea is
slower at every size measured. Grouped fan-out lowers a lone recall's p95 at 10,000 memories and
more, but serves fewer recalls a second once the cores are busy, and the sparse leg's problem
(#104) is the postings a query reads, which a split spreads and does not reduce. If a large
scope's sparse p95 still misses the bar after #104's bound, a grouped split that runs only when
the server has idle cores is the option to measure then, with a concurrency test as its gate.

**Build later, inside the work already planned:**

1. **Smallest useful version, on one database:** let `bm25-sparse` answer `wide` and a list by
   reading each leg over R at depth 100, fusing over R and restricting to the searched sources
   after fusion: the `Rrf` path's shape with CC's bounds. It lifts ADR-0072 d7.2's refusal
   (#205's named follow-up) with no new statement and no fan-out.
2. **Read `bm25-sparse`'s two legs concurrently** in the core: the recall costs the slower leg.
   Measure it with `sparse.py` before building.
3. **ADR-0073 phase 3 (#211) builds d9 as written**, with the threshold round as an option for
   round 2 once units are remote.

**What an ADR amendment would need to decide:**

- ADR-0072 d7.2: `Cc` under `wide` and a list is served by fusing each leg's top 100 over R and
  restricting after fusion; the refusal goes; test 6 extends to `bm25-sparse`.
- ADR-0073 d9 round 2: whether a retriever's depth may be reached in a threshold round (a source
  returns `d0` first, the coordinator asks again only where its threshold says so), what `d0` is,
  and how the hedge and the snapshot rules apply to the second ask, so the answer stays exact.
- That a source never fuses and never ranks on its own statistics: the reducer receives raw
  per-retriever lists only. This is d9's reading already; the measurement above is the reason to
  write it as a rule.
- Whether the lead reads a fused pipeline's legs concurrently, and what that costs in
  connections per recall.
- Nothing about fan-out inside one database: it stays out of the design unless the measurement
  above is repeated with #104's bound in place and shows a gain under load.

## Rerunning

```sh
# the sparse encoder, once (its target can go afterwards)
CARGO_TARGET_DIR=<scratch>/target cargo build --release -p oml-rerank --example sparse_encode
python3 eval/recall/mapreduce.py encode --encoder <scratch>/target/release/examples/sparse_encode \
    --model <doc-v3-distill dir> --threads 6 --scifact-zip scifact.zip --cache <scratch>/enc
python3 eval/recall/mapreduce.py run --scifact-zip scifact.zip --cache <scratch>/enc --out eval/recall/results
# a throwaway PostgreSQL 18, never the system's; psycopg 3 in a scratch venv
<venv>/bin/python bench/scale/fanout.py --dsn "host=<socket dir> port=<port> user=<superuser> dbname=postgres" \
    --cache <scratch>/enc --out bench/scale/results/fanout.json
<venv>/bin/python bench/scale/fanout.py --dsn … --cache <scratch>/enc \
    --out bench/scale/results/fanout-throughput.json --throughput
```

The PostgreSQL used `shared_buffers` 1 GB, `fsync` off and unlogged tables: a benchmark's
settings, not a server's.
