---
status: Measured
verified: 2026-11-06
answers: "how much time the web console's hop over the API adds to each page against ADR-0070's 5 ms p95 budget, how many API calls and sequential rounds each page makes, and where the added time goes"
---

# Web console overhead

*These are measurements by the OML maintainers on their own hardware, not independent results. Generated from the committed `oml-web-bench/1` results.*

[All suites](index.md)

Each page a signed-in admin can load, timed twice on the same data: served in process by the lead from before the pages moved, and served by `oml-web` over the public API on loopback. The budget is the second's p95 no more than 5 ms above the first's (ADR-0070 d8). The time is the client's, request sent to answer read over a keep-alive loopback connection; the health probe's time, given under each table, is the floor of that exchange. A page marked *re-baselined* is held to its own measured `oml-web` p95 instead, by the operator's decision of 2026-11-13: the person's Review page, which now reads the held listing and draws up to 80 cards where the in-process page listed nothing (ADR-0070's as-built rows).

*API calls* and *rounds* are read from the lead's request lines: the calls the page made and the longest chain of them that did not overlap, which is the round trips a LAN adds. *Outside the lead* is the page's time less the time at least one of its calls was in the lead's handler: `oml-web`'s own work, the hop's serialization and the loopback exchanges.

## 6 workareas, 240 memories

| page | in process p50 | p95 | oml-web p50 | p95 | p95 added | within budget | API calls | rounds | outside the lead p95 |
|---|---:|---:|---:|---:|---:|---|---:|---:|---:|
| `guardrails` | 14.6 | 19.3 | 11.5 | 13.4 | -5.9 | yes | 4 | 2 | 1.8 |
| `workareas` | 80.6 | 100 | 62.1 | 71.9 | -28.4 | yes | 15 | 4 | 2.4 |
| `admin/workareas` | 72.4 | 90.0 | 83.8 | 101 | +10.7 | no | 24 | 6–7 | 2.4 |
| `memories` | 94.0 | 116 | 90.2 | 107 | -8.9 | yes | 4 | 2 | 4.2 |
| `memories?q` | 33.3 | 42.1 | 34.5 | 39.8 | -2.3 | yes | 4 | 2 | 4.4 |
| `atlas` | 350 | 420 | 362 | 413 | -6.6 | yes | 5 | 2 | 2.4 |
| `data/memories` | 431 | 526 | 370 | 418 | -108.1 | yes | 5 | 2 | 2.7 |
| `admin/memories` | 116 | 149 | 129 | 146 | -3.0 | yes | 4 | 2 | 4.1 |
| `admin/atlas` | 23.0 | 29.3 | 26.3 | 30.4 | +1.1 | yes | 4 | 2 | 2.2 |
| `admin/data/memories` | 132 | 162 | 128 | 147 | -15.8 | yes | 5 | 2 | 3.0 |
| `memory` | 9.6 | 12.4 | 9.9 | 11.7 | -0.7 | yes | 3 | 2 | 2.0 |
| `admin/memory` | 48.5 | 83.1 | 61.8 | 84.3 | +1.2 | yes | 4 | 3 | 2.5 |
| `admin/curate` | 4.6 | 6.0 | 4.6 | 5.6 | -0.4 | yes | 2 | 1 | 1.3 |
| `review` | 5.7 | 7.3 | 18.5 | 21.4 | +14.1 | yes, re-baselined at 21.4 | 3 | 2 | 5.7 |
| `admin/review` | – | – | 24.3 | 28.0 | – | – | 5 | 3 | 4.9 |
| `keys` | 17.1 | 21.6 | 20.2 | 23.5 | +1.9 | yes | 4 | 2 | 2.0 |
| `admin/keys` | – | – | 11.5 | 13.6 | – | – | 3 | 2 | 1.8 |
| `start` | 10.3 | 13.1 | 14.4 | 17.2 | +4.0 | yes | 3 | 2 | 2.3 |
| `start/status` | 9.7 | 12.5 | 10.1 | 11.7 | -0.7 | yes | 1 | 1 | 1.1 |
| `start/export` | 13.0 | 16.4 | 13.5 | 15.4 | -1.0 | yes | 1 | 1 | 2.7 |
| `setup` | 5.6 | 7.3 | 5.0 | 6.0 | -1.4 | yes | 2 | 1 | 1.7 |
| `admin/people` | 4.8 | 6.3 | 7.2 | 8.7 | +2.3 | yes | 3 | 2 | 1.7 |
| `admin/users` | 7.8 | 10.2 | 12.1 | 14.3 | +4.2 | yes | 3 | 2 | 1.7 |
| `admin/review (held)` | 17.2 | 21.2 | – | – | – | – | – | – | – |
| `admin/review (queue)` | 9.4 | 12.0 | – | – | – | – | – | – | – |

Health probe: in-process 0.4 ms p50, 0.6 ms p95; oml-web 0.4 ms p50, 0.5 ms p95.

## 48 workareas, 240 memories

| page | in process p50 | p95 | oml-web p50 | p95 | p95 added | within budget | API calls | rounds | outside the lead p95 |
|---|---:|---:|---:|---:|---:|---|---:|---:|---:|
| `guardrails` | 17.8 | 20.3 | 12.5 | 15.2 | -5.1 | yes | 4 | 2 | 1.9 |
| `workareas` | 612 | 685 | 287 | 387 | -297.9 | yes | 99 | 14–15 | 2.5 |
| `admin/workareas` | 496 | 565 | 288 | 350 | -215.7 | yes | 150 | 21–23 | 1.8 |
| `memories` | 106 | 122 | 49.0 | 64.1 | -58.1 | yes | 4 | 2 | 3.3 |
| `memories?q` | 70.9 | 81.3 | 58.3 | 74.9 | -6.4 | yes | 4 | 2 | 4.3 |
| `atlas` | 401 | 442 | 381 | 429 | -12.4 | yes | 5 | 2 | 3.8 |
| `data/memories` | 495 | 550 | 380 | 433 | -116.8 | yes | 5 | 2 | 3.2 |
| `admin/memories` | 126 | 146 | 127 | 147 | +0.6 | yes | 4 | 2 | 4.3 |
| `admin/atlas` | 26.6 | 29.9 | 27.3 | 31.4 | +1.6 | yes | 4 | 2 | 2.4 |
| `admin/data/memories` | 149 | 170 | 128 | 147 | -22.4 | yes | 5 | 2 | 3.4 |
| `memory` | 10.4 | 12.1 | 9.9 | 11.9 | -0.2 | yes | 3 | 2 | 2.0 |
| `admin/memory` | 41.2 | 54.0 | 48.0 | 60.5 | +6.5 | no | 4 | 3 | 2.5 |
| `admin/curate` | 4.7 | 5.5 | 4.6 | 5.5 | -0.1 | yes | 2 | 1 | 1.3 |
| `review` | 6.0 | 7.1 | 20.1 | 23.3 | +16.2 | yes, re-baselined at 23.3 | 3 | 2 | 5.6 |
| `admin/review` | – | – | 57.5 | 67.0 | – | – | 5 | 3 | 5.3 |
| `keys` | 18.6 | 21.2 | 19.8 | 23.1 | +1.9 | yes | 4 | 2 | 1.9 |
| `admin/keys` | – | – | 11.5 | 13.7 | – | – | 3 | 2 | 1.8 |
| `start` | 11.7 | 13.3 | 15.0 | 18.0 | +4.6 | yes | 3 | 2 | 2.4 |
| `start/status` | 10.9 | 12.6 | 10.3 | 12.4 | -0.2 | yes | 1 | 1 | 1.2 |
| `start/export` | 15.1 | 17.2 | 14.2 | 16.2 | -1.0 | yes | 1 | 1 | 2.6 |
| `setup` | 6.0 | 6.9 | 4.9 | 5.9 | -1.1 | yes | 2 | 1 | 1.7 |
| `admin/people` | 5.2 | 6.0 | 7.2 | 8.7 | +2.6 | yes | 3 | 2 | 1.7 |
| `admin/users` | 8.4 | 9.9 | 11.9 | 14.1 | +4.2 | yes | 3 | 2 | 1.7 |
| `admin/review (held)` | 53.9 | 60.2 | – | – | – | – | – | – | – |
| `admin/review (queue)` | 43.1 | 49.2 | – | – | – | – | – | – | – |

Health probe: in-process 0.4 ms p50, 0.5 ms p95; oml-web 0.4 ms p50, 0.5 ms p95.

## Runs

| run | date | commit | iterations | one-minute load, before and after | host |
|---|---|---|---:|---|---|
| `bench/web/results/in-process-6.json` | 2026-11-11 | `4dedf532` | 500 after 50 | 2.69, 2.2 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/oml-web-6.json` | 2026-11-13 | `1b965508caa0` | 500 after 50 | 0.63, 1.31 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/in-process-48.json` | 2026-11-11 | `4dedf532` | 200 after 20 | 2.75, 1.41 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/oml-web-48.json` | 2026-11-13 | `1b965508caa0` | 200 after 20 | 0.82, 3.15 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |

Both shapes ran on one shared host, so a latency is an upper bound. The in-process lead is commit `4dedf532`, the last before phase (b), with the sign-in double of `bench/web/before-e2e-sign-in.patch`; the admin Review view was then two pages, the queue and the held list, and the admin Keys view did not exist. How to run it: `bench/web/README.md`.
