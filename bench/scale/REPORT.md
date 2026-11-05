# Scale bench report: `bm25-postings` at 10k, 100k and 165k memories per tenant

Measured 2026-11-05 on `feat/token-postings` (migrations 0001–0021), PostgreSQL 17.9, on one
8 vCPU / 15 GiB host shared with other build lanes: the load average was 4 to 11 during the
final 10k, 100k and 165k runs and up to 29 during the earlier ones, so every latency here is an
upper bound for one client. Method: [README.md](README.md). Raw numbers:
`results/postings-{10k,100k,165k}.json`. The data is the scale lane's synthetic text; it carries
over in shape, not in relevance.

Every recall here is one `PgStore::retrieve` call, warm, as the serving role under row security:
its transaction and every statement in it. `bm25` is v0.5.5's statement over the stored lexemes;
`bm25-postings` reads the integer postings with the compiled constants, `FREQUENT_SHARE` 0.1 and
`POSTING_BUDGET` 10,000. 165,000 memories put 99,721 served rows in the `universe` scope: the
"100,000 readable rows" of ADR-0052 d9.

## Amended 2026-11-05: prune only past the budget

The first rule left out every frequent form whatever the budget, which cost LoCoMo's R@100
(`eval/bench/REPORT.md`). The rule now reads every form while the query's readable postings fit
`POSTING_BUDGET`, and prunes as before only past it. Re-run at 165,000 memories on `ac57cd8`
(`results/postings-165k-budget-first.json`), on a busier host than the first run (load average
5 to 12; `bm25`'s own universe p95 rose from 493 to 841 ms between the two runs):

| rule | leaf p95 | project p95 | universe p95 | same list as bm25 | top-10 overlap |
|---|---:|---:|---:|---:|---:|
| bm25 | 26.0 | 389 | 841 | – | – |
| first rule (earlier run) | 9.2 | 12.1 | 29.9 | 0.921 | 0.975 |
| **budget first** | **14.6** | **24.0** | **41.7** | **0.942** | **0.979** |

The universe p95 stays under ADR-0052 d9's 50 ms.

Then words were pruned whole, every form together (review F1), each recall's statements read one
REPEATABLE READ snapshot, and the served index was re-measured, on `66979b4`, load average 2.3 to
4.7 (`results/postings-165k-whole-words.json`):

| rule | leaf p95 | project p95 | universe p95 | same list as bm25 | top-10 overlap |
|---|---:|---:|---:|---:|---:|
| bm25, same run | 22.3 | 248 | 485 | – | – |
| **whole words, budget first** | **5.9** | **15.9** | **25.5** | **0.927** | **0.971** |
| `(kind, token_id)` instead of the served index | 223 | 175 | 154 | – | – |

A key holding 1,000 own held rows: universe p95 62.0 ms; 10,000: 274 ms (`bm25`: 641 and 510).
The 0021 fill took 64.9 s; a promotion 7.4 ms p50 with the trigger, 1.2 without. Leaf and project scopes now read every form of
a query whose postings fit the budget, so they cost more and match `bm25` more often. The tables
below are the first rule's.

## Headline: warm p95 (ms) by scope

| memories | readable rows (leaf / project / universe) | bm25 leaf / project / universe | bm25-postings leaf / project / universe | unpruned postings universe |
|---:|---|---|---|---:|
| 10,000 | 473 / 1,471 / 6,104 | 13.1 / 25.0 / 49.1 | 5.0 / 5.2 / 14.0 | 12.1 |
| 100,000 | 2,182 / 12,608 / 60,341 | 27.1 / 171 / 326 | 5.7 / 12.8 / 26.6 | 59.5 |
| 165,000 | 2,380 / 13,981 / 99,721 | 22.6 / 218 / 493 | 9.2 / 12.1 / **29.9** | 107 |

- **ADR-0052 d9's bar holds**: at 99,721 readable rows, universe scope, `bm25-postings` answers
  at 29.9 ms p95 (p50 14.0), under the 50 ms bound and against `bm25`'s 493 ms. Its cost no longer
  follows the readable rows: leaf scopes stay under 10 ms at every scale.
- **Unpruned postings** return `bm25`'s list exactly (same ids in the same order for all 330
  queries at every scale) and already beat the scan, but a universe query of common words reads
  long posting lists: 107 ms p95 at 165k. Pruning is what holds the bound.
- **What pruning moves**: against `bm25`'s top 10, the served constants keep the same list for 92%
  of the queries at 165k (95% at 100k) and 97% of the top-10 members (98.7% at 100k). On the
  external sets the same constants cost at most 0.0005 nDCG@10 (`eval/bench/REPORT.md`).
- **The scan answered** 0 of 330 queries at 100k and 165k, and 2 of 330 at 10k.

## The constants (ADR-0052 open question 2)

At 165,000 memories, warm p95 in ms, and agreement with `bm25`'s top 10:

| frequent share | posting budget | universe p95 | all-scope p95 | same list | top-10 overlap |
|---:|---:|---:|---:|---:|---:|
| none | none | 107 | 49.9 | 1.000 | 1.000 |
| 0.1 | 20,000 | 46.7 | 23.2 | 0.949 | 0.987 |
| **0.1** | **10,000** | **29.9** | **18.4** | **0.921** | **0.975** |
| 0.1 | 5,000 | 14.1 | 10.5 | 0.876 | 0.945 |
| 0.2 | 10,000 | 24.9 | 19.7 | 0.936 | 0.978 |
| 0.05 | 10,000 | 31.9 | 18.1 | 0.827 | 0.947 |

0.1 is the share at which the external sets lose nothing measurable (0.05 costs NFCorpus 0.0019
nDCG@10); 10,000 is the largest budget measured that keeps the 165k universe p95 well under 50 ms
on a loaded host, where 20,000 sits at its edge.

## The access index (ADR-0052 open question 1)

The index d1 first wrote, `(kind, token_id)` over every posting, reads a token's postings across
the whole tenant and filters workarea and state from the heap. The built one,
`search_postings_served`, holds the served postings alone, ordered by `(kind, token_id,
workarea_id)` and carrying `memory_kind`, `memory_id`, `weight`, `doc_len` and `tenant_id`, so a
recall is an index-only read of its own workareas' postings:

| memories | index | size MiB | leaf p95 | project p95 | universe p95 |
|---:|---|---:|---:|---:|---:|
| 100,000 | `(kind, token_id)` | 25.7 | 105 | 95.6 | 89.4 |
| 100,000 | served, covering | 267 | 5.7 | 12.8 | 26.6 |
| 165,000 | `(kind, token_id)` | 41.9 | 148 | 137 | 138 |
| 165,000 | served, covering | 455 | 9.2 | 12.1 | 29.9 |

Three to eighteen times faster, at about ten times the index's size. A state flip into or out of
served now touches the index, so it is no heap-only update (the write costs below include it).

## Own held rows (ADR-0052 open question 7)

A key holding many held rows, recalling with them (`include_unreviewed`), warm p95 in ms:

| memories | own held rows | bm25 universe | bm25-postings leaf / project / universe |
|---:|---:|---:|---|
| 100,000 | 1,000 | 281 | 8.2 / 27.7 / 57.6 |
| 100,000 | 10,000 | 337 | 15.2 / 170 / 238 |
| 165,000 | 1,000 | 481 | 9.1 / 24.4 / 59.2 |
| 165,000 | 10,000 | 654 | 16.8 / 61.3 / 266 |

The own held rows are counted at query time from the versions and their postings, so their cost
grows with how many the key holds in the scope: 1,000 add about 30 ms at the universe scope, and
10,000 put it at 238–266 ms, still below `bm25`'s scan. A bulk import awaiting review (FR-87)
is the case that reaches it.

## Space, the fill and writes

| memories | postings | heap MiB | served index MiB | memory_id index MiB | counts MiB | memories heap MiB | 0021 fill s |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 10,000 | 354,861 | 34.2 | 31.2 | 3.4 | 4.2 | 9.7 | 3.4 |
| 100,000 | 3,561,632 | 344 | 267 | 34.1 | 30.6 | 98.3 | 39.0 |
| 165,000 | 5,884,714 | 568 | 455 | 56.5 | 55.6 | 162 | 70.4 |

About 36 postings per memory (the lexemes, identifier parts and stems of a 176-character claim),
all states kept (65% served, 15% held, 20% off). Postings, both indexes and the counts take about
7 times the memories heap: about 1.1 GiB per 165,000 memories, so about 6.7 GiB per 1,000,000
by linear extrapolation. The 0021 fill grows linearly, about 0.4 s per 1,000 memories, so about
7 minutes of held writes at 1,000,000 (FR-103).

Write cost, p50 / p95 ms, each step its own transaction as the serving role:

| memories | insert held | promote | supersede |
|---:|---|---|---|
| 100,000, no trigger | 1.35 / 2.62 | 1.67 / 3.12 | 2.81 / 4.87 |
| 100,000, with postings | 1.98 / 4.16 | 3.57 / 8.01 | 5.48 / 12.5 |
| 165,000, no trigger | 1.51 / 4.68 | 1.67 / 4.76 | 3.32 / 6.83 |
| 165,000, with postings | 2.40 / 8.08 | 4.54 / 8.94 | 6.35 / 12.6 |

A promotion pays about 2–3 ms more p50: its postings enter the served index and its tokens'
counts are updated. One client; contention between promotions in one workarea is FR-102.

## Not measured

- **1,000,000 memories.** The build host's two filesystems had 1.5 to 5 GiB free while other lanes
  ran, and 1,000,000 memories need about 7 GiB for the postings alone. The 1M figures above are
  extrapolations, as FR-100 says they must not stay.
- **Cold caches.** Every number is warm: the second of two runs of each query.

## The learned sparse lane (2026-11-05, ADR-0052 d16)

`sparse.py` on this branch's release build, PostgreSQL 18.6, through `PgStore::retrieve` as the
serving role, warm (`results/sparse-10k.json`, `results/sparse-100k.json`). p95 ms, universe
scope / every scope:

| memories (readable at universe) | load average | `bm25_postings` k 10 | `sparse` k 10 | `bm25-sparse`'s two lists, 100 deep |
|---|---|---:|---:|---:|
| 10,000 (6,104) | 11 to 13, an upper bound | 10.2 / 7.4 | 16.9 / 9.4 | 28.3 / 23.1 |
| 100,000 (60,341) | 1.7 to 2.1 | 29.2 / 20.4 | 164 / 78 | 272 / 114 |

About 104 kind 2 postings per version; at 100,000 memories the kind 2 heap is 1,276 MiB and the
served index 985 MiB. The 100,000 point reuses the 10,000 point's encodings (`--reuse`: each
version takes one of 9,189 encoded texts by its text's digest), so it measures the read, not the
ranking. The sparse leg misses ADR-0052 d9's 50 ms bar at 100,000 readable rows: FR-118.

## The similar check (ADR-0050 d17), 2026-11-05

`similar.py` at 10k, 100k and 1M memories per tenant, PostgreSQL 18 on the build host (8 vCPU,
15 GB), while another lane's benchmark and its sparse encoder ran on the same host (load average
8 to 12), so the times are a busy host's. Each write's keys through `oml_pg::SIMILAR_CANDIDATES` as
the serving role under row security, then the read of its candidates' rows by id: the two
statements the lead runs per write. Half the 330 writes are a loaded memory with one word added,
half new texts. ms, warm:

| scale | lookup p50 / p95 | with the rows p50 / p95 | universe scope, with the rows p50 / p95 | candidates p50 (max 32) | keys (MiB, with indexes) | 0026's fill |
|---|---:|---:|---:|---:|---:|---:|
| 10k | 0.41 / 0.62 | 0.82 / 1.25 | 0.89 / 1.49 | 1 | 21 | 3.6 s |
| 100k | 0.53 / 1.04 | 0.90 / 1.82 | 1.19 / 2.28 | 2 | 213 | 38 s |
| 1M | 1.69 / 10.7 | 2.95 / 15.7 | 7.34 / 24.8 | 8 | 2,124 | 1,147 s |

The check this replaces, one any-word recall per write (case g of the baseline), cost 128 ms p50
and 180 ms p95 at 1M; this one costs 2.95 and 15.7. The plan is an index-only scan of
`similar_keys_key` under the policy at every scale. The generator templates its memories from
200 seeds, so its texts share far more words than real ones: at 1M a universe-scope write meets the
32-candidate cap every time, which real text should rarely do. The keys take about twice the
memories heap (2,124 MiB against 985 at 1M). The trigger adds 0.4 ms p50 to a held insert at 10k,
2.1 at 100k and 6.2 at 1M (10.8 to 17.0), and nothing measurable to a promotion. 0026's fill holds
the trigger's lock on `memories` for its whole run, 19 minutes at 1M on this host, so writes wait
that long during the upgrade, as 0021's fill does (FR-103).
