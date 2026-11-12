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
| `guardrails` | 14.6 | 19.3 | 10.5 | 13.3 | -6.0 | yes | 4 | 2 | 1.7 |
| `workareas` | 80.6 | 100 | 57.7 | 70.7 | -29.6 | yes | 15 | 4–5 | 2.3 |
| `admin/workareas` | 72.4 | 90.0 | 71.1 | 96.3 | +6.2 | no | 24 | 6–7 | 2.2 |
| `memories` | 94.0 | 116 | 68.2 | 102 | -14.1 | yes | 4 | 2 | 3.4 |
| `memories?q` | 33.3 | 42.1 | 28.9 | 38.2 | -3.9 | yes | 4 | 2 | 3.7 |
| `atlas` | 350 | 420 | 331 | 397 | -22.6 | yes | 4 | 2 | 2.3 |
| `data/memories` | 431 | 526 | 343 | 409 | -117.0 | yes | 6 | 2 | 2.6 |
| `admin/memories` | 116 | 149 | 118 | 145 | -3.9 | yes | 3 | 2 | 3.4 |
| `admin/atlas` | 23.0 | 29.3 | 24.6 | 29.9 | +0.6 | yes | 4 | 2 | 2.2 |
| `admin/data/memories` | 132 | 162 | 119 | 147 | -14.9 | yes | 5 | 2–3 | 3.0 |
| `memory` | 9.6 | 12.4 | 9.3 | 11.5 | -0.9 | yes | 3 | 2 | 1.9 |
| `admin/memory` | 48.5 | 83.1 | 53.0 | 83.0 | -0.1 | yes | 3 | 2 | 1.9 |
| `admin/curate` | 4.6 | 6.0 | 4.5 | 5.7 | -0.4 | yes | 2 | 1–2 | 1.3 |
| `review` | 5.7 | 7.3 | 4.2 | 5.5 | -1.8 | yes | 2 | 1 | 1.2 |
| `admin/review` | – | – | 25.0 | 30.8 | – | – | 4 | 3 | 3.4 |
| `keys` | 17.1 | 21.6 | 18.1 | 22.7 | +1.2 | yes | 3 | 2–3 | 1.9 |
| `admin/keys` | – | – | 10.8 | 13.4 | – | – | 3 | 2 | 1.8 |
| `start` | 10.3 | 13.1 | 13.3 | 16.6 | +3.5 | yes | 3 | 2 | 2.3 |
| `start/status` | 9.7 | 12.5 | 9.3 | 11.5 | -1.0 | yes | 1 | 1 | 1.2 |
| `start/export` | 13.0 | 16.4 | 12.5 | 15.0 | -1.4 | yes | 1 | 1 | 2.9 |
| `setup` | 5.6 | 7.3 | 4.8 | 6.0 | -1.3 | yes | 2 | 1 | 1.7 |
| `admin/people` | 4.8 | 6.3 | 6.8 | 8.7 | +2.3 | yes | 3 | 2 | 1.6 |
| `admin/users` | 7.8 | 10.2 | 11.3 | 13.9 | +3.8 | yes | 3 | 2 | 1.7 |
| `admin/review (held)` | 17.2 | 21.2 | – | – | – | – | – | – | – |
| `admin/review (queue)` | 9.4 | 12.0 | – | – | – | – | – | – | – |

Health probe: in-process 0.4 ms p50, 0.6 ms p95; oml-web 0.4 ms p50, 0.5 ms p95.

## 48 workareas, 240 memories

| page | in process p50 | p95 | oml-web p50 | p95 | p95 added | within budget | API calls | rounds | outside the lead p95 |
|---|---:|---:|---:|---:|---:|---|---:|---:|---:|
| `guardrails` | 17.8 | 20.3 | 12.2 | 14.5 | -5.8 | yes | 4 | 2 | 1.8 |
| `workareas` | 612 | 685 | 269 | 346 | -338.5 | yes | 99 | 14–15 | 2.4 |
| `admin/workareas` | 496 | 565 | 289 | 339 | -226.6 | yes | 150 | 21–23 | 1.7 |
| `memories` | 106 | 122 | 48.7 | 60.7 | -61.5 | yes | 4 | 2 | 2.6 |
| `memories?q` | 70.9 | 81.3 | 54.1 | 69.4 | -11.9 | yes | 4 | 2 | 3.5 |
| `atlas` | 401 | 442 | 370 | 427 | -14.8 | yes | 4 | 2 | 3.6 |
| `data/memories` | 495 | 550 | 374 | 431 | -119.3 | yes | 6 | 2 | 3.1 |
| `admin/memories` | 126 | 146 | 126 | 152 | +5.8 | no | 3 | 2 | 3.3 |
| `admin/atlas` | 26.6 | 29.9 | 25.7 | 31.4 | +1.5 | yes | 4 | 2 | 2.3 |
| `admin/data/memories` | 149 | 170 | 124 | 150 | -20.1 | yes | 5 | 2 | 3.5 |
| `memory` | 10.4 | 12.1 | 9.6 | 11.9 | -0.2 | yes | 3 | 2 | 1.9 |
| `admin/memory` | 41.2 | 54.0 | 42.2 | 53.4 | -0.6 | yes | 3 | 2 | 2.0 |
| `admin/curate` | 4.7 | 5.5 | 4.7 | 5.8 | +0.2 | yes | 2 | 1 | 1.3 |
| `review` | 6.0 | 7.1 | 4.3 | 5.3 | -1.8 | yes | 2 | 1 | 1.2 |
| `admin/review` | – | – | 89.4 | 105 | – | – | 4 | 3 | 3.6 |
| `keys` | 18.6 | 21.2 | 19.3 | 23.1 | +1.9 | yes | 3 | 2 | 2.0 |
| `admin/keys` | – | – | 11.4 | 13.7 | – | – | 3 | 2 | 1.8 |
| `start` | 11.7 | 13.3 | 14.2 | 17.3 | +3.9 | yes | 3 | 2 | 2.3 |
| `start/status` | 10.9 | 12.6 | 10.2 | 12.8 | +0.2 | yes | 1 | 1 | 1.3 |
| `start/export` | 15.1 | 17.2 | 13.5 | 16.8 | -0.4 | yes | 1 | 1 | 2.8 |
| `setup` | 6.0 | 6.9 | 4.8 | 6.1 | -0.8 | yes | 2 | 1 | 1.7 |
| `admin/people` | 5.2 | 6.0 | 6.9 | 8.8 | +2.7 | yes | 3 | 2 | 1.7 |
| `admin/users` | 8.4 | 9.9 | 11.6 | 14.6 | +4.8 | yes | 3 | 2 | 1.7 |
| `admin/review (held)` | 53.9 | 60.2 | – | – | – | – | – | – | – |
| `admin/review (queue)` | 43.1 | 49.2 | – | – | – | – | – | – | – |

Health probe: in-process 0.4 ms p50, 0.5 ms p95; oml-web 0.4 ms p50, 0.5 ms p95.

## Runs

| run | date | commit | iterations | one-minute load, before and after | host |
|---|---|---|---:|---|---|
| `bench/web/results/in-process-6.json` | 2026-11-11 | `4dedf532` | 500 after 50 | 2.69, 2.2 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/oml-web-6.json` | 2026-11-12 | `9a2f4bd2` | 500 after 50 | 1.99, 1.64 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/in-process-48.json` | 2026-11-11 | `4dedf532` | 200 after 20 | 2.75, 1.41 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/oml-web-48.json` | 2026-11-12 | `9a2f4bd2` | 200 after 20 | 1.81, 1.59 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |

Both shapes ran on one shared host, so a latency is an upper bound. The in-process lead is commit `4dedf532`, the last before phase (b), with the sign-in double of `bench/web/before-e2e-sign-in.patch`; the admin Review view was then two pages, the queue and the held list, and the admin Keys view did not exist. How to run it: `bench/web/README.md`.
