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
workarea. With those three rules, each workarea scoring only its own rows from the summed
counts, the merged answer equalled the global one on every query of six partitions, scores
included, with pruning active on 75% of the SciFact queries. Every shortcut lost quality: merging per-workarea fused
answers by score or by reciprocal rank lost up to 0.79 of success@5 at 500 workareas.

On one PostgreSQL a split buys latency and throughput from idle cores, and costs throughput
once the cores are busy. At 100,000 memories and 50 workareas, with 1 to 4 clients on 8 vCPUs,
grouping the scope into 2 to 8 statements cut the sparse leg's p95 from about 95 ms to 19 to
56 ms and served 1.6 to 4.3 times the recalls a second; with 8 clients it served 7 to 14% fewer
(five rounds, ranges not overlapping). One statement per workarea cost 220 to 330 ms at 500
workareas at every size. So a split is a latency tool for a lightly loaded server, and the shape
ADR-0073's nodes need, where each shard reads on its own cores; it does not reduce the postings a
query reads, which is #104's lever.

The prototype: [`mapreduce.py`](mapreduce.py) (quality, standard library) and
[`../../bench/scale/fanout.py`](../../bench/scale/fanout.py) (latency, psycopg). Results:
[`results/mapreduce.json`](results/mapreduce.json),
[`../../bench/scale/results/fanout.json`](../../bench/scale/results/fanout.json) and
[`../../bench/scale/results/fanout-throughput.json`](../../bench/scale/results/fanout-throughput.json).
Revised 2026-11-12 after review: each workarea now scores itself from its own rows and the summed
counts, pruning included, and the throughput run is repeated and interleaved.

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

**When scores are comparable, merge by score.** In the prototype each workarea is its own
index over its own rows (`Shard` in `mapreduce.py`). Round 1 asks every workarea for its row
count, summed length and each query form's document frequency over its rows alone; the reducer
sums them and decides the pruned words from the sums with `oml-pg`'s rule (`PruneRule::pruned`,
`FREQUENT_SHARE` 0.1, `POSTING_BUDGET` 10,000). Round 2 hands every workarea those numbers; it
scores its own rows from its own postings and returns each leg's top 100. The global run is a
separate index over the union that counts its own statistics. Measured on all six partitions:

- round 1's sums equalled the union's own counts on 100% of queries;
- each merged leg equalled the global leg, ids and scores, on 100% of queries for `sparse`,
  `bm25` and both legs of `bm25-sparse`, the largest score difference 0.0 (float equality);
- the fused top 10 equalled the global top 10 with its scores on 100% of queries.

Pruning took part: on SciFact 75.3% of queries pruned at least one word (5.8% on the skew split,
none on the topics split), and in every one of them a workarea deciding pruning from its own
counts would have pruned differently from the union. Round 1 fixes it, so the exact merge never
sees that difference.

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
| skew | 11 | 279 | 278 | 96.7% | 79.0 / 97 / 99 |
| scifact-5 | 5 | 970 | 396 | 0% | 26.4 / 31 / 34 |
| scifact-50 | 50 | 4,619 | 963 | 0.7% | 6.3 / 8 / 10 |
| scifact-zipf-50 | 50 | 3,063 | 1,253 | 100% | 24.3 / 30 / 36 |
| scifact-500 | 500 | 4,631 | 4,608 | 0% | 2.6 / 3 / 4 |

The threshold round matters when workareas are many and remote: at 50 workareas it ships 4.8
times fewer rows for the same answer. At 500 the floor of 10 per leg dominates. Under skew (one
workarea holding 93% of the rows) the large workarea needs nearly the full 100 every time, so no
protocol saves it work. (Fewer rows ship than `N × 200` because a workarea returns only the rows
its kept words generate.)

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
- Pruning is `oml-pg`'s rule over the offline analyzer's counts. The candidates are the rows the
  kept words' forms generate, each scored with every word; the served statement also cuts the
  generated rows at `k × RESCORE_DEPTH` before rescoring, which the prototype does not.

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

Under concurrent clients, at 100,000 memories and 50 workareas (`results/fanout-throughput.json`,
re-run 2026-11-12 on a quiet host: load average 1.47 before the run, every other lane idle; the
load rose to 14.7 from the bench's own backends). Each client runs all 100 queries once per
cell; every round runs every cell, client counts in turn and the mode order rotated per round;
five rounds, each cell's median and its range over them:

| clients | mode | recalls a second | p50 ms | p95 ms |
|---:|---|---:|---:|---:|
| 1 | `single` | 23.2 (22.7–24.3) | 41.1 (39.4–41.4) | 94.2 (91.5–97.9) |
| 1 | `fanout-groups-2` | 39.2 (38.5–41.1) | 23.5 (23.0–24.9) | 53.5 (52.4–54.6) |
| 1 | `fanout-groups-4` | 51.7 (50.2–61.2) | 16.6 (14.6–18.0) | 40.7 (35.2–42.1) |
| 1 | `fanout-groups-8` | 99.3 (93.9–102.5) | 9.7 (9.2–9.9) | 18.6 (17.4–20.5) |
| 2 | `single` | 45.4 (44.7–46.9) | 41.1 (40.4–41.7) | 95.8 (95.4–97.0) |
| 2 | `fanout-groups-2` | 75.7 (73.3–75.9) | 24.6 (24.2–24.8) | 53.7 (51.9–56.4) |
| 2 | `fanout-groups-4` | 127.3 (120.9–131.4) | 14.8 (14.4–15.1) | 30.9 (28.8–34.2) |
| 2 | `fanout-groups-8` | 126.4 (125.4–131.2) | 14.9 (14.1–15.3) | 28.6 (27.8–29.3) |
| 4 | `single` | 89.8 (88.9–91.0) | 41.7 (41.0–42.3) | 99.2 (95.4–100.1) |
| 4 | `fanout-groups-2` | 148.1 (148.0–152.0) | 24.7 (24.6–25.0) | 56.4 (54.3–57.9) |
| 4 | `fanout-groups-4` | 147.7 (143.0–150.1) | 25.0 (24.9–25.2) | 54.8 (53.3–57.5) |
| 4 | `fanout-groups-8` | 142.6 (141.3–147.1) | 25.4 (25.1–26.1) | 53.1 (51.8–53.3) |
| 8 | `single` | 174.7 (173.0–178.4) | 41.8 (41.6–42.0) | 100.8 (98.4–101.8) |
| 8 | `fanout-groups-2` | 162.4 (162.1–163.4) | 45.3 (45.0–45.8) | 105.7 (104.4–105.9) |
| 8 | `fanout-groups-4` | 155.7 (154.9–158.4) | 46.8 (46.1–47.0) | 106.9 (102.7–107.5) |
| 8 | `fanout-groups-8` | 150.1 (147.0–151.7) | 49.0 (48.8–49.9) | 103.6 (100.4–106.7) |

The ranges do not overlap between `single` and any split at any client count, so the differences
below are not drift. (The first throughput run, one pass per cell in a fixed order while the load
rose from 3.1 to 4.7, is superseded by this one.)

What the numbers say:

- **Below 10,000 memories nothing gains.** The statement takes 2 to 5 ms; the split adds a
  round trip and a plan per statement.
- **One statement per workarea is never worth it past a handful.** At 500 workareas it takes
  220 to 330 ms whatever the size: 500 round trips on 8 connections.
- **While cores are idle, a split wins in latency and in throughput.** With 1 to 4 clients on 8
  vCPUs, splitting a 100,000-memory scope cut p50 from about 41 ms to 10 to 25 ms and p95 from
  about 95 ms to 19 to 56 ms, and served 1.6 to 4.3 times the recalls a second (4 clients: 148
  against 90, +65%). The cores to split onto should match what is idle: with 1 client 8 groups
  win, with 4 clients 2 groups do as well as 8.
- **Once the cores are busy it costs.** With 8 clients the split served 7 to 14% fewer recalls
  a second (150 to 162 against 175) at a p95 3 to 6% higher: the same postings are read, plus a
  plan, a round trip and a merge per group. It also holds P connections per recall.
- **So it is a latency tool for a lightly loaded server, not capacity.** Where it helps depends
  on the recall concurrency against the cores, which the server would have to read per recall
  (for instance, split only while fewer than half the connections are busy). That control, and
  the 50 ms bar's case at 100,000 memories, are what a build would have to measure.
- **PostgreSQL's own parallel query does not do it.** The planner took a Gather at every point
  and it did not help, or cost about 10 ms of worker start at the small sizes.

**Two harnesses, two numbers for the same split.** The single-client table gives
`fanout-groups-8` at 100,000 memories and 50 workareas 18.1 / 31.8 ms (p50 / p95); the
throughput table gives 9.7 / 18.6 for one client. `single` agrees in both (41.5 / 100.7 against
41.1 / 94.2), so the split's gain is 2.1 to 4.3 times at p50 whichever is right. Re-run on the
quiet host, the first harness gave 16.7 / 27.3 with only `single` and the split interleaved,
12.5 / 22.4 with every mode, 19.3 / 32.6 with the split alone, and 18.8 / 29.5 with psycopg's
statement preparation off; the throughput harness gave 9.2 to 9.9 / 17.4 to 20.5 over five
rounds. So host load, the other modes and prepared generic plans are ruled out. What is left is
how each harness times: the first runs a single-statement reference recall and a warm run of the
same query before each timed split, leaving seven of eight cores idle for about 40 ms between
splits; the second runs the split back to back on connections that run nothing else. The cause
is not confirmed; the conclusions above use the throughput harness, which repeats and reports its
range. The four re-runs are in `bench/scale/results/fanout-reconcile/`.

Two further reads cost less than splitting a scope. Today `bm25-sparse` reads its two lists one
after the other (`bench/scale/REPORT.md`: 272 ms p95 at 100,000 memories for both lists, 100
deep); reading them on two connections at once makes the recall cost its slower leg, not the
sum. Not measured here. And #104's bound on the postings a query reads lowers the work itself,
which the split only spreads.

**Across nodes it is the only design.** Under ADR-0073 phase 3 a scope spans units on different
nodes, and no single statement can read them. There each source reads its own postings on its
own cores, so the capacity argument above does not apply, and the cost is the slowest source plus
two network rounds, which d9 bounds with hedging. The threshold round in section 2 keeps round
2's payload near 1,000 rows at 50 sources instead of 4,600.

## 4. How it composes

- **R3, visibility per shard.** Each shard's statement filters `workarea_id = ANY(readable)` and
  `state = 1` under row security, as today; a shard never returns a row its caller cannot read,
  and round 1's counts are over R, which is a subset of what the key reads (ADR-0072 d4.2, d6).
  The exact merge adds no new surface: a row's score depends on the row, the query and R's
  statistics, so no unreadable row moves a readable one. The shortcuts in section 2 break that
  property in a different way: a workarea's own statistics or bounds would make a hit's score
  depend on which workarea it lives in and what else that workarea holds.
- **The caller's own held rows go to one shard.** Both statements read a second branch beside the
  workarea filter: the postings of the caller's own held rows by id (`$4`, `p.memory_id =
  ANY($4::uuid[])`, `oml-pg` `sparse_sql` and `bm25_postings_sql`), with no `workarea_id`
  predicate. Handed to every shard, each would return the same held rows, and under BM25 the
  summed round 1 would count them once per shard. So exactly one shard carries `$4`: the one
  serving the scope's own nodes, where ADR-0072 d7 keeps own held rows (and ADR-0030 d3, d4 an
  in-progress scope's), and every other shard gets an empty list. Its round 1 adds the held rows'
  counts once, as `POSTINGS_OWN_HELD_DF` does today, and under a reference set it adds none (R
  holds served rows alone, d7). The prototype has no held rows; this is the rule a built version
  needs and a test it must carry: a held row appears once, with the score the single statement
  gives it.
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

**Never one statement per workarea; a grouped split on one database is worth building later,
gated on load.** One statement per workarea is slower at every size measured. A grouped split
(the scope cut into P statements on P connections, each leg merged by score) gives the global
answer exactly, and at 100,000 memories it cut p95 from about 95 ms to 19 to 56 ms and added
throughput while the server had idle cores; it served 7 to 14% fewer recalls once 8 clients
filled 8 vCPUs. It is not the fix for #104 (the postings a query reads), and it is not needed
below about 10,000 memories in scope. Build it after #104's bound, as an adaptive split: P chosen
per recall from the idle backends or cores (P 1 under load), only above a scope size, measured
with `fanout.py --throughput` against the 50 ms bar with the bound in place.

**Build later, inside the work already planned:**

1. **Smallest useful version, on one database:** let `bm25-sparse` answer `wide` and a list by
   reading each leg over R at depth 100, fusing over R and restricting to the searched sources
   after fusion: the `Rrf` path's shape with CC's bounds. It lifts ADR-0072 d7.2's refusal
   (#205's named follow-up) with no new statement and no fan-out.
2. **Read `bm25-sparse`'s two legs concurrently** in the core: the recall costs the slower leg.
   Measure it with `sparse.py` before building.
3. **The adaptive grouped split on one database**, after #104's bound, gated as above.
4. **ADR-0073 phase 3 (#211) builds d9 as written**, with the threshold round as an option for
   round 2 once units are remote, and with the caller's own held rows on one source only
   (section 4).

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
- For a split inside one database: the scope size above which it runs, how P follows the load
  (and that P is 1 when the server is busy), the connections a recall may hold, and that it is
  exact by construction (round 1 over the whole scope, legs merged before fusion).
- That the caller's own held rows (`$4`) go to exactly one source, the scope's.

## Inputs

| input | source | pin |
|---|---|---|
| BEIR SciFact | `https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/scifact.zip`, CC BY-NC 2.0 (`eval/bench/README.md`); not committed | sha256 `536e14446a0ba56ed1398ab1055f39fe852686ecad24a6306c80c490fa8e0165`; `corpus.jsonl` (5,183), `queries.jsonl`, `qrels/test.tsv` (300 queries) |
| doc-v3-distill | `opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill`, Apache-2.0, revision `babf71f3c48695e2e53a978208e8aba48335e3c0` | `config.json` `ee977804…cbe3`, `tokenizer.json` `91f1def9…1854`, `model.safetensors` `83a3cc97…f70a`, `idf.json` `da23a1c0…dd`: the digests `oml-rerank`'s `DOC_V3_DISTILL` pins, checked after download |
| the recall corpus and queries | `eval/recall/corpus.json`, `queries.json` (data version 2) | this commit |

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
