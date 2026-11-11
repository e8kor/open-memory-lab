---
status: Measured
verified: 2026-11-06
answers: "how much time the web console's hop over the API adds to each page against ADR-0070's 5 ms p95 budget, how many API calls and sequential rounds each page makes, and where the added time goes"
---

# Web console overhead

*These are measurements by the OML maintainers on their own hardware, not independent results. Generated from the committed `oml-web-bench/1` results.*

[All suites](index.md)

Each page a signed-in admin can load, timed twice on the same data: served in process by the lead from before the pages moved, and served by `oml-web` over the public API on loopback. The budget is the second's p95 no more than 5 ms above the first's (ADR-0070 d8). The time is the client's, request sent to answer read over a keep-alive loopback connection; the health probe's time, given under each table, is the floor of that exchange.

*API calls* and *rounds* are read from the lead's request lines: the calls the page made and the longest chain of them that did not overlap, which is the round trips a LAN adds. *Outside the lead* is the page's time less the time at least one of its calls was in the lead's handler: `oml-web`'s own work, the hop's serialization and the loopback exchanges.

## 6 workareas, 240 memories

| page | in process p50 | p95 | oml-web p50 | p95 | p95 added | within budget | API calls | rounds | outside the lead p95 |
|---|---:|---:|---:|---:|---:|---|---:|---:|---:|
| `guardrails` | 14.6 | 19.3 | 25.4 | 30.7 | +11.5 | no | 4 | 2 | 1.8 |
| `workareas` | 80.6 | 100 | 84.5 | 99.9 | -0.4 | yes | 15 | 4 | 2.4 |
| `admin/workareas` | 72.4 | 90.0 | 106 | 133 | +42.7 | no | 24 | 6 | 2.4 |
| `memories` | 94.0 | 116 | 97.6 | 122 | +6.2 | no | 4 | 2 | 3.6 |
| `memories?q` | 33.3 | 42.1 | 63.4 | 74.2 | +32.0 | no | 4 | 3 | 4.3 |
| `atlas` | 350 | 420 | 370 | 424 | +4.5 | yes | 4 | 2 | 2.4 |
| `data/memories` | 431 | 526 | 377 | 431 | -94.6 | yes | 6 | 2 | 2.7 |
| `admin/memories` | 116 | 149 | 136 | 160 | +10.3 | no | 3 | 2 | 3.4 |
| `admin/atlas` | 23.0 | 29.3 | 42.4 | 49.7 | +20.4 | no | 5 | 3 | 2.6 |
| `admin/data/memories` | 132 | 162 | 136 | 160 | -2.1 | yes | 6 | 3 | 3.0 |
| `memory` | 9.6 | 12.4 | 23.7 | 28.0 | +15.6 | no | 3 | 2 | 2.0 |
| `admin/memory` | 48.5 | 83.1 | 66.9 | 92.2 | +9.1 | no | 3 | 2 | 2.0 |
| `admin/curate` | 4.6 | 6.0 | 14.4 | 17.4 | +11.4 | no | 2 | 1 | 1.4 |
| `review` | 5.7 | 7.3 | 14.1 | 16.8 | +9.5 | no | 2 | 1 | 1.3 |
| `admin/review` | – | – | 40.4 | 47.8 | – | – | 4 | 3 | 3.5 |
| `keys` | 17.1 | 21.6 | 33.5 | 39.6 | +18.0 | no | 3 | 2 | 2.1 |
| `admin/keys` | – | – | 23.9 | 28.5 | – | – | 3 | 2 | 1.9 |
| `start` | 10.3 | 13.1 | 32.2 | 38.8 | +25.6 | no | 3 | 2 | 2.4 |
| `start/status` | 9.7 | 12.5 | 18.4 | 21.9 | +9.5 | no | 1 | 1 | 1.3 |
| `start/export` | 13.0 | 16.4 | 17.7 | 20.7 | +4.2 | yes | 1 | 1 | 3.0 |
| `setup` | 5.6 | 7.3 | 14.7 | 17.4 | +10.0 | no | 2 | 1 | 1.8 |
| `admin/people` | 4.8 | 6.3 | 19.6 | 23.8 | +17.4 | no | 3 | 2 | 1.8 |
| `admin/users` | 7.8 | 10.2 | 23.6 | 28.2 | +18.1 | no | 3 | 2 | 1.8 |
| `admin/review (held)` | 17.2 | 21.2 | – | – | – | – | – | – | – |
| `admin/review (queue)` | 9.4 | 12.0 | – | – | – | – | – | – | – |

Health probe: in-process 0.4 ms p50, 0.6 ms p95; oml-web 0.4 ms p50, 0.5 ms p95.

## 48 workareas, 240 memories

| page | in process p50 | p95 | oml-web p50 | p95 | p95 added | within budget | API calls | rounds | outside the lead p95 |
|---|---:|---:|---:|---:|---:|---|---:|---:|---:|
| `guardrails` | 17.8 | 20.3 | 28.2 | 33.2 | +12.9 | no | 4 | 2 | 2.0 |
| `workareas` | 612 | 685 | 377 | 498 | -186.9 | yes | 99 | 14 | 2.6 |
| `admin/workareas` | 496 | 565 | 350 | 434 | -131.7 | yes | 150 | 21–22 | 1.8 |
| `memories` | 106 | 122 | 57.6 | 70.8 | -51.4 | yes | 4 | 2 | 2.8 |
| `memories?q` | 70.9 | 81.3 | 94.0 | 112 | +31.2 | no | 4 | 3 | 4.2 |
| `atlas` | 401 | 442 | 414 | 460 | +18.2 | no | 4 | 2 | 3.6 |
| `data/memories` | 495 | 550 | 424 | 464 | -86.1 | yes | 6 | 2 | 3.3 |
| `admin/memories` | 126 | 146 | 147 | 162 | +15.6 | no | 3 | 2 | 3.3 |
| `admin/atlas` | 26.6 | 29.9 | 45.3 | 51.2 | +21.4 | no | 5 | 3 | 2.6 |
| `admin/data/memories` | 149 | 170 | 146 | 163 | -6.3 | yes | 6 | 3 | 3.4 |
| `memory` | 10.4 | 12.1 | 25.1 | 28.6 | +16.5 | no | 3 | 2 | 2.0 |
| `admin/memory` | 41.2 | 54.0 | 54.9 | 70.7 | +16.7 | no | 3 | 2 | 2.1 |
| `admin/curate` | 4.7 | 5.5 | 15.1 | 17.5 | +11.9 | no | 2 | 1 | 1.4 |
| `review` | 6.0 | 7.1 | 14.7 | 17.2 | +10.1 | no | 2 | 1 | 1.3 |
| `admin/review` | – | – | 112 | 124 | – | – | 4 | 3 | 3.6 |
| `keys` | 18.6 | 21.2 | 35.4 | 39.9 | +18.7 | no | 3 | 2 | 2.1 |
| `admin/keys` | – | – | 24.9 | 28.8 | – | – | 3 | 2 | 1.9 |
| `start` | 11.7 | 13.3 | 34.6 | 39.6 | +26.3 | no | 3 | 2 | 2.4 |
| `start/status` | 10.9 | 12.6 | 19.8 | 22.7 | +10.1 | no | 1 | 1 | 1.2 |
| `start/export` | 15.1 | 17.2 | 19.3 | 22.0 | +4.8 | yes | 1 | 1 | 2.9 |
| `setup` | 6.0 | 6.9 | 15.1 | 17.7 | +10.8 | no | 2 | 1 | 1.8 |
| `admin/people` | 5.2 | 6.0 | 20.5 | 23.4 | +17.3 | no | 3 | 2 | 1.8 |
| `admin/users` | 8.4 | 9.9 | 24.4 | 28.7 | +18.8 | no | 3 | 2 | 1.9 |
| `admin/review (held)` | 53.9 | 60.2 | – | – | – | – | – | – | – |
| `admin/review (queue)` | 43.1 | 49.2 | – | – | – | – | – | – | – |

Health probe: in-process 0.4 ms p50, 0.5 ms p95; oml-web 0.4 ms p50, 0.5 ms p95.

## Runs

| run | date | commit | iterations | one-minute load, before and after | host |
|---|---|---|---:|---|---|
| `bench/web/results/in-process-6.json` | 2026-11-11 | `4dedf532` | 500 after 50 | 2.69, 2.2 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/oml-web-6.json` | 2026-11-11 | `f385716753d0` | 500 after 50 | 1.31, 2.95 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/in-process-48.json` | 2026-11-11 | `4dedf532` | 200 after 20 | 2.75, 1.41 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/oml-web-48.json` | 2026-11-11 | `f385716753d0` | 200 after 20 | 2.07, 2.8 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |

Both shapes ran on one shared host, so a latency is an upper bound. The in-process lead is commit `4dedf532`, the last before phase (b), with the sign-in double of `bench/web/before-e2e-sign-in.patch`; the admin Review view was then two pages, the queue and the held list, and the admin Keys view did not exist. How to run it: `bench/web/README.md`.
