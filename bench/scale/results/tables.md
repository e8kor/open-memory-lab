## Headline: warm p95 latency (ms) as the serving role under row security

| approach | 10k | 100k | 1M | 10 tenants × 100k | slope (log-log) | crosses 50 ms p95 at |
|---|---:|---:|---:|---:|---:|---|
| a) today: simple AND + ts_rank | 101 | 539 | ≥5,000† | 339 | 0.85 | already at 10k |
| b) simple OR + ts_rank | 129 | 936 | ≥5,000† | 582 | 0.79 | already at 10k |
| b) simple OR + ts_rank_cd | 153 | 1,003 | ≥5,000† | 644 | 0.76 | already at 10k |
| c) english+simple stored, OR | 16.5 | 148 | 4,215 | 146 | 1.20 | ~30k |
| d) BM25, query-time stats | 170 | 2,284 | ≥5,000† | 1,390 | 0.73 | already at 10k |
| d) BM25, maintained stats | 171 | 2,306 | ≥5,000† | 1,484 | 0.73 | already at 10k |
| e) pg_trgm on text | 218 | 2,063 | ≥5,000† | 2,066 | 0.68 | already at 10k |
| e) pg_trgm on identifiers | 50.7 | 503 | 4,535 | 511 | 0.98 | already at 10k |
| a') AND over a stored simple tsvector | 6.3 | 56.6 | 1,400 | – | 1.17 | ~67k |
| b') OR over a stored simple tsvector | 10.2 | 110 | 2,716 | – | 1.21 | ~42k |
| d') BM25 maintained, stored tsvector | 51.7 | 677 | ≥5,000† | – | 0.99 | already at 10k |
| f) pgvector HNSW 384-d (1M: halfvec) | 2.9 | 9.8 | 15.4 | – | 0.36 | not by 1M (fit: ~18.6M) |

## The same statements without row security (superuser, same tenant predicate), warm p95 (ms)

What each index could give if its operator were usable under the policy; not a configuration OML may run (ADR-0034 d13).

| approach | 10k | 100k | 1M | 10 tenants × 100k |
|---|---:|---:|---:|---:|
| a) today: simple AND + ts_rank | 1.4 | 2.7 | 34.7 | 3.1 |
| b) simple OR + ts_rank | 79.3 | 1,197 | 4,822 | 1,129 |
| c) english+simple stored, OR | 58.4 | 625 | 3,049 | 138 |
| e) pg_trgm on text | 13.0 | 110 | 983 | 120 |
| e) pg_trgm on identifiers | 2.7 | 16.5 | 140 | 18.9 |
| a') AND over a stored simple tsvector | 1.6 | 2.5 | 36.2 | – |
| a) on one unpartitioned table | – | – | – | 12.5 |
| b) on one unpartitioned table | – | – | – | 1,258 |

## Cold and warm latency, p50 / p95 / p99 (ms), under row security

Cold: the cluster restarted and the OS page cache of its files and of the loop image evicted before the pass; the pass itself warms the cache, so it is a cold start, not 330 cold queries.


### 10k memories per tenant

| approach | cold p50 | cold p95 | cold p99 | first query | warm p50 | warm p95 | warm p99 | timeouts | empty results |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| a) today: simple AND + ts_rank | 19.0 | 100 | 113 | 339 | 17.9 | 101 | 125 | 0 | 286 of 330 |
| b) simple OR + ts_rank | 21.4 | 150 | 185 | 358 | 19.9 | 129 | 176 | 0 | 14 of 330 |
| b) simple OR + ts_rank_cd | 21.8 | 160 | 294 | 349 | 21.2 | 153 | 282 | 0 | 14 of 330 |
| c) english+simple stored, OR | 3.0 | 16.4 | 28.9 | 601 | 2.6 | 16.5 | 28.2 | 0 | 7 of 330 |
| d) BM25, query-time stats | 23.9 | 179 | 284 | 389 | 26.2 | 170 | 255 | 0 | 14 of 330 |
| d) BM25, maintained stats | 25.7 | 196 | 274 | 424 | 23.3 | 171 | 289 | 0 | 14 of 330 |
| e) pg_trgm on text | 39.3 | 221 | 346 | 812 | 38.3 | 218 | 336 | 0 | 197 of 330 |
| e) pg_trgm on identifiers | 8.8 | 49.3 | 105 | 667 | 8.7 | 50.7 | 105 | 0 | 298 of 330 |
| a') AND over a stored simple tsvector | 1.5 | 5.1 | 9.4 | 625 | 1.8 | 6.3 | 10.7 | 0 | 285 of 330 |
| b') OR over a stored simple tsvector | 2.1 | 9.6 | 21.5 | 518 | 2.1 | 10.2 | 19.2 | 0 | 13 of 330 |
| d') BM25 maintained, stored tsvector | 6.2 | 60.3 | 107 | 726 | 5.5 | 51.7 | 95.3 | 0 | 13 of 330 |
| f) pgvector HNSW 384-d (1M: halfvec) | 1.5 | 9.9 | 384 | 384 | 1.3 | 2.9 | 4.7 | 0 | 0 of 330 |

### 100k memories per tenant

| approach | cold p50 | cold p95 | cold p99 | first query | warm p50 | warm p95 | warm p99 | timeouts | empty results |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| a) today: simple AND + ts_rank | 115 | 521 | 556 | 1,066 | 116 | 539 | 576 | 0 | 266 of 330 |
| b) simple OR + ts_rank | 133 | 899 | 957 | 1,017 | 131 | 936 | 1,074 | 0 | 8 of 330 |
| b) simple OR + ts_rank_cd | 147 | 1,090 | 1,585 | 1,024 | 141 | 1,003 | 1,445 | 0 | 8 of 330 |
| c) english+simple stored, OR | 12.6 | 147 | 276 | 1,119 | 13.2 | 148 | 261 | 0 | 4 of 330 |
| d) BM25, query-time stats | 159 | 2,247 | 2,660 | 1,331 | 156 | 2,284 | 2,540 | 0 | 8 of 330 |
| d) BM25, maintained stats | 162 | 2,369 | 2,605 | 1,155 | 154 | 2,306 | 2,593 | 0 | 8 of 330 |
| e) pg_trgm on text | 224 | 2,036 | 3,004 | 1,105 | 214 | 2,063 | 2,973 | 0 | 170 of 330 |
| e) pg_trgm on identifiers | 48.1 | 526 | 1,037 | 1,180 | 47.3 | 503 | 922 | 0 | 291 of 330 |
| a') AND over a stored simple tsvector | 8.0 | 58.3 | 69.1 | 1,117 | 7.4 | 56.6 | 69.1 | 0 | 266 of 330 |
| b') OR over a stored simple tsvector | 10.9 | 122 | 199 | 1,116 | 11.3 | 110 | 197 | 0 | 8 of 330 |
| d') BM25 maintained, stored tsvector | 33.2 | 722 | 986 | 1,160 | 28.2 | 677 | 967 | 0 | 8 of 330 |
| f) pgvector HNSW 384-d (1M: halfvec) | 2.7 | 236 | 2,502 | 5,608 | 1.7 | 9.8 | 14.5 | 0 | 0 of 330 |

### 1M memories per tenant

| approach | cold p50 | cold p95 | cold p99 | first query | warm p50 | warm p95 | warm p99 | timeouts | empty results |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| a) today: simple AND + ts_rank | 129 | ≥5,000† | ≥5,000† | ≥5,000† | 129 | ≥5,000† | ≥5,000† | 29 | 276 of 330 |
| b) simple OR + ts_rank | 161 | ≥5,000† | ≥5,000† | ≥5,000† | 158 | ≥5,000† | ≥5,000† | 39 | 48 of 330 |
| b) simple OR + ts_rank_cd | 172 | ≥5,000† | ≥5,000† | ≥5,000† | 171 | ≥5,000† | ≥5,000† | 40 | 49 of 330 |
| c) english+simple stored, OR | 69.1 | ≥5,000† | ≥5,000† | ≥5,000† | 62.8 | 4,215 | ≥5,000† | 10 | 16 of 330 |
| d) BM25, query-time stats | 253 | ≥5,000† | ≥5,000† | ≥5,000† | 239 | ≥5,000† | ≥5,000† | 43 | 53 of 330 |
| d) BM25, maintained stats | 258 | ≥5,000† | ≥5,000† | ≥5,000† | 236 | ≥5,000† | ≥5,000† | 43 | 53 of 330 |
| e) pg_trgm on text | 377 | ≥5,000† | ≥5,000† | ≥5,000† | 363 | ≥5,000† | ≥5,000† | 61 | 196 of 330 |
| e) pg_trgm on identifiers | 88.8 | ≥5,000† | ≥5,000† | ≥5,000† | 79.3 | 4,535 | ≥5,000† | 11 | 277 of 330 |
| a') AND over a stored simple tsvector | 41.5 | 4,511 | ≥5,000† | ≥5,000† | 40.2 | 1,400 | 2,737 | 0 | 267 of 330 |
| b') OR over a stored simple tsvector | 54.6 | ≥5,000† | ≥5,000† | ≥5,000† | 47.2 | 2,716 | ≥5,000† | 4 | 14 of 330 |
| d') BM25 maintained, stored tsvector | 92.0 | ≥5,000† | ≥5,000† | ≥5,000† | 71.7 | ≥5,000† | ≥5,000† | 18 | 28 of 330 |
| f) pgvector HNSW 384-d (1M: halfvec) | 14.1 | 3,541 | ≥5,000† | ≥5,000† | 9.2 | 15.4 | 21.3 | 0 | 0 of 330 |

### 10 tenants × 100k in one partitioned table (the measured tenant is one of ten)

| approach | warm p50 | warm p95 | warm p99 | no-RLS warm p95 |
|---|---:|---:|---:|---:|
| a) today: simple AND + ts_rank | 107 | 339 | 357 | 3.1 |
| b) simple OR + ts_rank | 125 | 582 | 637 | 1,129 |
| b) simple OR + ts_rank_cd | 136 | 644 | 914 | – |
| c) english+simple stored, OR | 13.5 | 146 | 257 | 138 |
| d) BM25, query-time stats | 158 | 1,390 | 1,737 | – |
| d) BM25, maintained stats | 162 | 1,484 | 1,768 | – |
| e) pg_trgm on text | 218 | 2,066 | 2,994 | 120 |
| e) pg_trgm on identifiers | 50.6 | 511 | 967 | 18.9 |
| a) on one unpartitioned table | 128 | 1,148 | 1,419 | 12.5 |
| b) on one unpartitioned table | 169 | 2,335 | 3,293 | 1,258 |

## Warm p95 (ms) by query category at 1,000,000 memories

| approach | exact | identifier | inflection | long | multihop | negation | paraphrase | partial | question | temporal | typo |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| a) today: simple AND + ts_rank | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | 3,163 |
| b) simple OR + ts_rank | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | 3,426 |
| b) simple OR + ts_rank_cd | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | 3,152 |
| c) english+simple stored, OR | 2,711 | 2,952 | 2,388 | ≥5,000† | ≥5,000† | 4,854 | 4,658 | 1,764 | ≥5,000† | 864 | 1,624 |
| d) BM25, query-time stats | ≥5,000† | ≥5,000† | 3,741 | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | 3,467 |
| d) BM25, maintained stats | ≥5,000† | ≥5,000† | 3,451 | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | 3,469 |
| e) pg_trgm on text | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† | ≥5,000† |
| e) pg_trgm on identifiers | 4,034 | 3,339 | 3,798 | ≥5,000† | ≥5,000† | 4,802 | 4,957 | 3,104 | ≥5,000† | 4,497 | 2,906 |
| a') AND over a stored simple tsvector | 1,023 | 1,047 | 964 | 3,301 | 1,560 | 1,376 | 1,276 | 804 | 1,884 | 1,326 | 750 |
| b') OR over a stored simple tsvector | 1,483 | 1,484 | 1,323 | ≥5,000† | 3,320 | 2,714 | 2,464 | 985 | 4,477 | 2,925 | 940 |
| d') BM25 maintained, stored tsvector | 1,823 | 1,684 | 1,456 | ≥5,000† | ≥5,000† | 4,577 | 3,784 | 1,159 | ≥5,000† | ≥5,000† | 1,131 |
| f) pgvector HNSW 384-d (1M: halfvec) | 22.9 | 15.1 | 15.2 | 16.4 | 14.7 | 14.4 | 14.5 | 17.0 | 16.1 | 14.1 | 13.0 |

## Warm p50 / p95 (ms) by readable scope, under row security

leaf: the leaf and universe; project: a project, its leaves and universe; universe: every workarea. From the runs that recorded it (1M, and the 10k/100k supplement for a and c).

| approach | scale | leaf | project | universe |
|---|---|---:|---:|---:|
| a) today: simple AND + ts_rank | 1M | 98.1 / 134 | 142 / 261 | 3,309 / ≥5,000† |
| b) simple OR + ts_rank | 1M | 117 / 176 | 185 / 449 | ≥5,000† / ≥5,000† |
| b) simple OR + ts_rank_cd | 1M | 123 / 209 | 191 / 561 | ≥5,000† / ≥5,000† |
| c) english+simple stored, OR | 1M | 35.6 / 189 | 72.1 / 328 | 2,532 / ≥5,000† |
| d) BM25, query-time stats | 1M | 140 / 320 | 277 / 788 | ≥5,000† / ≥5,000† |
| d) BM25, maintained stats | 1M | 138 / 334 | 263 / 792 | ≥5,000† / ≥5,000† |
| e) pg_trgm on text | 1M | 193 / 376 | 403 / 818 | ≥5,000† / ≥5,000† |
| e) pg_trgm on identifiers | 1M | 42.2 / 126 | 91.4 / 265 | 3,719 / ≥5,000† |
| a') AND over a stored simple tsvector | 10k | 1.2 / 2.9 | 2.0 / 3.9 | 5.8 / 10.7 |
| a') AND over a stored simple tsvector | 100k | 2.0 / 5.9 | 8.6 / 29.0 | 53.2 / 69.1 |
| a') AND over a stored simple tsvector | 1M | 20.3 / 54.2 | 43.5 / 108 | 1,240 / 2,737 |
| b') OR over a stored simple tsvector | 10k | 1.2 / 3.1 | 2.5 / 6.0 | 8.0 / 19.7 |
| b') OR over a stored simple tsvector | 100k | 2.8 / 7.6 | 13.2 / 50.7 | 83.0 / 197 |
| b') OR over a stored simple tsvector | 1M | 26.0 / 85.8 | 57.8 / 215 | 1,776 / ≥5,000† |
| d') BM25 maintained, stored tsvector | 10k | 2.8 / 13.3 | 6.1 / 31.8 | 16.0 / 104 |
| d') BM25 maintained, stored tsvector | 100k | 7.8 / 31.1 | 38.8 / 193 | 275 / 967 |
| d') BM25 maintained, stored tsvector | 1M | 35.6 / 129 | 83.5 / 261 | 2,649 / ≥5,000† |
| f) pgvector HNSW 384-d (1M: halfvec) | 1M | 11.4 / 17.0 | 8.4 / 15.2 | 4.1 / 5.1 |

## Index build time (s) and size (MiB) per tenant

| index | 10k build | 10k size | 100k build | 100k size | 1M build | 1M size |
|---|---:|---:|---:|---:|---:|---:|
| heap (COPY), today's columns | 0.56 | 4.6 | 5.25 | 46.1 | 57.93 | 459.6 |
| today's GIN to_tsvector('simple') | 0.27 | 1.3 | 0.94 | 10.6 | 5.84 | 80.7 |
| primary key | 0.03 | 0.5 | 0.13 | 4.8 | 0.52 | 47.5 |
| (tenant, workarea, status) | 0.02 | 0.1 | 0.08 | 0.7 | 0.50 | 7.2 |
| served tip (partial unique) | 0.03 | 0.4 | 0.12 | 4.2 | 1.08 | 42.1 |
| claim version (partial unique) | 0.03 | 0.6 | 0.15 | 6.4 | 0.74 | 68.7 |
| author | 0.03 | 0.1 | 0.08 | 0.7 | 0.42 | 7.0 |
| c) table rewrite adding two stored tsvectors (heap after) | 0.72 | 12.5 | 5.47 | 118.2 | 47.31 | 1,175.7 |
| c) GIN on stored simple tsvector | 0.13 | 1.5 | 1.33 | 11.6 | 11.70 | 88.2 |
| c) GIN on stored english tsvector | 0.08 | 1.2 | 0.33 | 9.4 | 2.40 | 69.7 |
| d) maintained df table + PK (rows below) | 0.30 | 4.0 | 2.01 | 18.4 | 26.40 | 189.5 |
| e) GIN gin_trgm_ops on claim | 0.37 | 4.7 | 1.45 | 37.6 | 12.04 | 265.4 |
| e) GIN gin_trgm_ops on identifiers | 0.08 | 1.2 | 0.31 | 9.6 | 2.42 | 73.7 |
| f) vector(384) table (COPY) | 1.28 | 16.2 | 11.38 | 156.7 | 112 | 868.5 |
| f) HNSW m 16, ef_construction 64 | 2.44 | 19.5 | 7.36 | 195.3 | 101 | 1,116.1 |

Maintained df rows (tenant, workarea, term): 26556, 122106, 1220672 at 10k, 100k, 1M.

## Write cost per memory (ms), serving role, synchronous_commit off

Insert = journal row + held memories row in one transaction; promote = the UPDATE that makes it a served tip. Stages are cumulative except the BM25 trigger, measured on today's indexes and then dropped; the stage's own cost is its row minus the row it builds on.

| stage | 10k insert p50 / p95 | 10k promote p50 / p95 | 100k insert | 100k promote | 1M insert | 1M promote |
|---|---:|---:|---:|---:|---:|---:|
| today's indexes | 0.76 / 2.52 | 0.50 / 1.07 | 2.49 / 4.52 | 0.89 / 1.43 | 3.70 / 12.71 | 0.97 / 2.00 |
| + BM25 df trigger (on today's) | 1.11 / 3.30 | 1.14 / 2.11 | 2.44 / 7.38 | 1.96 / 9.30 | 1.05 / 1.71 | 1.20 / 2.26 |
| c) stored english+simple, 2 GIN (replaces today's GIN) | 1.23 / 3.65 | 0.79 / 1.19 | 1.81 / 5.83 | 0.59 / 1.77 | 4.84 / 10.68 | 1.06 / 2.57 |
| + e) two trigram GIN | 0.97 / 3.66 | 0.60 / 1.36 | 2.38 / 8.05 | 0.95 / 2.10 | 4.84 / 13.50 | 1.18 / 2.82 |
| f) one vector row into the HNSW table | 1.80 / 4.99 | – | 4.53 / 18.53 | – | 5.77 / 20.09 | – |

## ADR-0050 d1 write-time similar check (ms added per write)

Served-tip recall plus own-held recall, any word (simple OR), k 3, the new text as the query, at a leaf workarea's scope (leaf + universe), under row security.

| scale | p50 | p95 | p99 | mean candidates returned |
|---|---:|---:|---:|---:|
| 10k | 13.3 | 87.7 | 98.6 | 5.6 |
| 100k | 39.8 | 152 | 422 | 6.0 |
| 1M | 128 | 180 | 306 | 6.0 |
| 10 × 100k | 39.9 | 160 | 414 | 6.0 |

## BM25 statistics against row security; HNSW recall

| scale | terms checked | maintained df = query-time df under RLS | tenant-wide df differs from readable df |
|---|---:|---:|---:|
| 10k | 157 | 157 | 95 |
| 100k | 158 | 158 | 115 |
| 1M | 163 | 163 | 104 |

The main runs' HNSW recall check is not reported: its exact side reused the HNSW plan (PostgreSQL 18 kept the disabled index scan), so it compared the index with itself. Added case 1 below measures recall against a forced exact scan.

## Data shape

| run | rows per tenant | tenants | workareas per tenant | served rows readable at universe | database size (MiB) |
|---|---:|---:|---:|---:|---:|
| 10k | 10,000 | 1 | 17 | 6,096 | 79.6 |
| 100k | 100,000 | 1 | 46 | 60,440 | 595.2 |
| 1M | 1,000,000 | 1 | 451 | 604,769 | 2,050.9 |
| 1M (vector run) | 1,000,000 | 1 | 451 | 604,769 | 2,755.8 |
| 10 × 100k | 100,000 | 10 | 46 | 60,440 | 2,234.8 |

## Added case 1: exact halfvec scan vs HNSW under selective scopes (warm, ms; recall@10 vs exact)

The same statement (`ORDER BY embedding <=> q LIMIT 10` with the scope filter) as the serving role under row security, on halfvec(384); exact = `ORDER BY (embedding <=> q) + 0`, which no index can serve: a scan of the tenant's partition; HNSW ef_search 40. The readable share is of the tenant's served rows.

| scale | readable share | exact p50 / p95 | HNSW, no iterative scan: p50 / p95 / recall@10 / short | HNSW, relaxed_order: p50 / p95 / recall@10 / short |
|---|---:|---:|---:|---:|
| 10k | 100% | 3.5 / 7.3 | 0.5 / 1.0 / 0.999 / 0 | 0.5 / 1.0 / 0.999 / 0 |
| 10k | 10% | 1.7 / 3.5 | 0.5 / 1.0 / 0.453 / 277 | 1.4 / 2.3 / 0.992 / 0 |
| 10k | 1% | 1.4 / 3.4 | 0.6 / 1.1 / 0.204 / 309 | 1.5 / 3.0 / 0.999 / 0 |
| 100k | 100% | 27.8 / 31.7 | 1.2 / 1.8 / 0.975 / 0 | 1.1 / 1.9 / 0.975 / 0 |
| 100k | 10% | 20.1 / 24.6 | 1.0 / 1.6 / 0.379 / 296 | 1.1 / 1.9 / 0.992 / 0 |
| 100k | 1% | 17.7 / 26.4 | 0.8 / 1.0 / 0.086 / 324 | 5.1 / 12.3 / 0.950 / 0 |

HNSW on halfvec(384) in the main workload (leaf / project / universe scopes):

| scale | warm p50 | warm p95 | cold p95 | first query (cold) | HNSW MiB | build s |
|---|---:|---:|---:|---:|---:|---:|
| 100k | 1.7 | 8.2 | 127 | 5,892 | 111.6 | 6.6 |
| 1M | 9.2 | 15.4 | 3,541 | ≥5,000† | 1,116.1 | 101 |

## Added case 2: learned-sparse inverted table, SUM(weight) GROUP BY memory (ms)

60 non-zero Zipf-distributed tokens per memory over a 30,522-token vocabulary, 10 query tokens, btree on token_id, the serving role under row security (token_id = ANY(int[]) is a leakproof operator, so the index is usable under the policy). Pruned = the same queries without the 100 most frequent token ids. The 1M row is a log-log extrapolation from the measured scales (60M postings did not fit on the shared build filesystem).

| scale | postings | heap MiB | token btree MiB | warm p50 | warm p95 | pruned p50 | pruned p95 | cold p95 | postings touched p50 | write 60 postings p50 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 30k | 1,633,056 | 132.0 | 11.6 | 59.9 | 96.6 | 1.9 | 4.9 | 101 | 110,170 | 1.80 |
| 100k | 5,441,729 | 438.5 | 36.9 | 217 | 328 | 6.5 | 15.3 | 323 | 360,548 | 2.71 |
| 200k | 10,883,870 | 877.0 | 73.1 | 443 | 735 | 12.5 | 29.6 | 771 | 698,964 | 5.81 |
| 1M (extrapolated) | ~60,000,000 | 4,369.3 | 346.7 | 2,443 | 3,959 | 61.0 | 137 | 4,042 | – | – |
