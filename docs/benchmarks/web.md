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
| `guardrails` | 14.6 | 19.3 | 20.4 | 24.2 | +4.9 | yes | 4 | 2 | 1.7 |
| `workareas` | 80.6 | 100 | 79.3 | 93.1 | -7.2 | yes | 15 | 4 | 2.3 |
| `admin/workareas` | 72.4 | 90.0 | 97.6 | 126 | +35.7 | no | 24 | 6 | 2.3 |
| `memories` | 94.0 | 116 | 84.7 | 115 | -1.2 | yes | 4 | 2 | 3.4 |
| `memories?q` | 33.3 | 42.1 | 38.2 | 45.9 | +3.8 | yes | 4 | 2 | 3.7 |
| `atlas` | 350 | 420 | 355 | 405 | -14.4 | yes | 4 | 2 | 2.3 |
| `data/memories` | 431 | 526 | 363 | 416 | -109.8 | yes | 6 | 2 | 2.7 |
| `admin/memories` | 116 | 149 | 129 | 150 | +1.1 | yes | 3 | 2 | 3.4 |
| `admin/atlas` | 23.0 | 29.3 | 33.1 | 38.4 | +9.1 | no | 4 | 2 | 2.2 |
| `admin/data/memories` | 132 | 162 | 128 | 150 | -12.1 | yes | 5 | 2 | 3.0 |
| `memory` | 9.6 | 12.4 | 18.8 | 21.9 | +9.5 | no | 3 | 2 | 2.0 |
| `admin/memory` | 48.5 | 83.1 | 62.0 | 83.9 | +0.8 | yes | 3 | 2 | 2.0 |
| `admin/curate` | 4.6 | 6.0 | 9.5 | 11.0 | +5.0 | yes | 2 | 1 | 1.3 |
| `review` | 5.7 | 7.3 | 9.0 | 10.7 | +3.4 | yes | 2 | 1 | 1.2 |
| `admin/review` | – | – | 35.9 | 42.6 | – | – | 4 | 3 | 3.4 |
| `keys` | 17.1 | 21.6 | 27.8 | 33.0 | +11.5 | no | 3 | 2 | 2.0 |
| `admin/keys` | – | – | 18.5 | 22.0 | – | – | 3 | 2 | 1.8 |
| `start` | 10.3 | 13.1 | 23.1 | 26.6 | +13.4 | no | 3 | 2 | 2.3 |
| `start/status` | 9.7 | 12.5 | 13.9 | 16.6 | +4.2 | yes | 1 | 1 | 1.2 |
| `start/export` | 13.0 | 16.4 | 17.4 | 20.3 | +3.9 | yes | 1 | 1 | 2.9 |
| `setup` | 5.6 | 7.3 | 9.5 | 11.3 | +4.0 | yes | 2 | 1 | 1.8 |
| `admin/people` | 4.8 | 6.3 | 14.5 | 17.1 | +10.7 | no | 3 | 2 | 1.7 |
| `admin/users` | 7.8 | 10.2 | 18.4 | 22.0 | +11.8 | no | 3 | 2 | 1.8 |
| `admin/review (held)` | 17.2 | 21.2 | – | – | – | – | – | – | – |
| `admin/review (queue)` | 9.4 | 12.0 | – | – | – | – | – | – | – |

Health probe: in-process 0.4 ms p50, 0.6 ms p95; oml-web 0.4 ms p50, 0.5 ms p95.

## 48 workareas, 240 memories

| page | in process p50 | p95 | oml-web p50 | p95 | p95 added | within budget | API calls | rounds | outside the lead p95 |
|---|---:|---:|---:|---:|---:|---|---:|---:|---:|
| `guardrails` | 17.8 | 20.3 | 17.1 | 25.4 | +5.1 | no | 4 | 2–3 | 2.9 |
| `workareas` | 612 | 685 | 378 | 445 | -240.2 | yes | 99 | 14–16 | 4.2 |
| `admin/workareas` | 496 | 565 | 532 | 627 | +61.9 | no | 150 | 21–26 | 3.7 |
| `memories` | 106 | 122 | 70.1 | 94.0 | -28.2 | yes | 4 | 2 | 4.5 |
| `memories?q` | 70.9 | 81.3 | 51.8 | 77.5 | -3.8 | yes | 4 | 2 | 4.1 |
| `atlas` | 401 | 442 | 270 | 436 | -5.7 | yes | 4 | 2 | 4.1 |
| `data/memories` | 495 | 550 | 277 | 422 | -127.9 | yes | 6 | 2 | 4.1 |
| `admin/memories` | 126 | 146 | 99.6 | 150 | +4.5 | yes | 3 | 2 | 4.4 |
| `admin/atlas` | 26.6 | 29.9 | 26.7 | 39.3 | +9.4 | no | 4 | 2–3 | 3.4 |
| `admin/data/memories` | 149 | 170 | 101 | 151 | -18.7 | yes | 5 | 2–3 | 3.7 |
| `memory` | 10.4 | 12.1 | 14.4 | 22.2 | +10.1 | no | 3 | 2–3 | 3.5 |
| `admin/memory` | 41.2 | 54.0 | 38.7 | 54.1 | +0.0 | yes | 3 | 2–3 | 3.0 |
| `admin/curate` | 4.7 | 5.5 | 7.7 | 11.5 | +5.9 | no | 2 | 1 | 1.7 |
| `review` | 6.0 | 7.1 | 7.3 | 10.9 | +3.8 | yes | 2 | 1–2 | 1.7 |
| `admin/review` | – | – | 70.2 | 112 | – | – | 4 | 3 | 4.4 |
| `keys` | 18.6 | 21.2 | 20.7 | 32.2 | +10.9 | no | 3 | 2–3 | 3.5 |
| `admin/keys` | – | – | 14.1 | 21.4 | – | – | 3 | 2–3 | 2.7 |
| `start` | 11.7 | 13.3 | 17.5 | 27.0 | +13.7 | no | 3 | 2 | 4.1 |
| `start/status` | 10.9 | 12.6 | 10.7 | 17.4 | +4.8 | yes | 1 | 1 | 2.1 |
| `start/export` | 15.1 | 17.2 | 11.9 | 19.7 | +2.5 | yes | 1 | 1 | 2.6 |
| `setup` | 6.0 | 6.9 | 7.6 | 11.6 | +4.7 | yes | 2 | 1–2 | 3.5 |
| `admin/people` | 5.2 | 6.0 | 11.0 | 16.9 | +10.9 | no | 3 | 2 | 3.3 |
| `admin/users` | 8.4 | 9.9 | 13.5 | 20.9 | +11.0 | no | 3 | 2 | 2.9 |
| `admin/review (held)` | 53.9 | 60.2 | – | – | – | – | – | – | – |
| `admin/review (queue)` | 43.1 | 49.2 | – | – | – | – | – | – | – |

Health probe: in-process 0.4 ms p50, 0.5 ms p95; oml-web 0.2 ms p50, 0.9 ms p95.

## Runs

| run | date | commit | iterations | one-minute load, before and after | host |
|---|---|---|---:|---|---|
| `bench/web/results/in-process-6.json` | 2026-11-11 | `4dedf532` | 500 after 50 | 2.69, 2.2 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/oml-web-6.json` | 2026-11-12 | `b3c7a75745b6` | 500 after 50 | 1.92, 2.35 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/in-process-48.json` | 2026-11-11 | `4dedf532` | 200 after 20 | 2.75, 1.41 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |
| `bench/web/results/oml-web-48.json` | 2026-11-12 | `b3c7a75745b6` | 200 after 20 | 1.1, 8.67 | 8 CPUs, postgres (PostgreSQL) 18.6 (Ubuntu 18.6-0ubuntu0.26.04.1) |

Both shapes ran on one shared host, so a latency is an upper bound. The in-process lead is commit `4dedf532`, the last before phase (b), with the sign-in double of `bench/web/before-e2e-sign-in.patch`; the admin Review view was then two pages, the queue and the held list, and the admin Keys view did not exist. How to run it: `bench/web/README.md`.
