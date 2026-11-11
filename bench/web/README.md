# Web bench: the console's hop over the API

This directory measures what [ADR-0070](../../docs/adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md)
d8 budgets: each page's p95 served by `oml-web` over the public HTTP API, against the same page
served in process by the lead before phase (b) moved it, on the same data
([#170](https://github.com/e8kor/open-memory-lab/issues/170)). The budget is 5 ms p95 on
loopback, plus two round trips on a LAN. The findings are below, the numbers in
`results/*.json`, the decisions they take in ADR-0070's table of 2026-11-11, and the generated
page is [docs/benchmarks/web.md](../../docs/benchmarks/web.md).

## Findings of 2026-11-11

Four runs on one host, 8 CPUs, PostgreSQL 18.6 on a throwaway cluster (production pins 17):
`oml-web-6` and `oml-web-48` at `f3857167`, `in-process-6` and `in-process-48` at `4dedf532`,
500 iterations after 50 at 6 workareas and 200 after 20 at 48. The one-minute load average was
1.3 to 3.0; the bench's own processes and PostgreSQL account for most of it, and no build ran
beside it. Every figure is an upper bound for one client.

1. **The budget is missed on 16 of the 21 pages with an in-process counterpart** at 6
   workareas, and on 15 at 48, by +9.5 to +42.7 ms p95 at 6. Within it: the user Workareas view,
   the Atlas, both Atlas JSON routes and the data export, each dominated by one heavy read the
   move did not change.
2. **The miss is the session check, not the hop.** Every page opens with one round of two
   `GET /v2/whoami` calls, the web and the admin credentials in parallel. Each takes 7.8 to
   17.4 ms p50 at the lead with 8 to 19 store calls: the credential is resolved for the key and
   again for the person block, then the grants and the person's limits are read. The pages that
   call nothing else show that round alone: Review, Curate and Setup spend 13.0 to 13.2 ms p50 in
   the lead against 4.6 to 5.7 ms for the whole page in process, +9.5 to +11.4 ms p95.
3. **The hop itself is within budget.** Outside the lead, a page's time less the time its calls
   were in the lead's handler, is 1.0 to 1.5 ms p50 on a one-round page and at most 4.3 ms p95
   on any page, against 0.5 to 1.0 ms in process: `oml-web`'s routing and template, both sides'
   JSON and the loopback exchanges. Serialization and templates are not where the time goes.
4. **Three pages add a round or a call of their own.** The text search reads the person's keys
   before the recall, a third round: +32.0 ms p95. The admin Atlas reads the scope's offers after
   the tree, 3 rounds: +20.4 ms. The admin Memories search runs after the session check: +10.3 ms.
5. **The Workareas views fan out with the tree.** The user view makes 3 + 2N calls for N
   workareas (99 at 48), the admin view 3 + 3N over its N nodes (150 at 49), at most 8 in flight
   (`oml_web::workareas::FAN_OUT`): 14 and 21 to 22 sequential rounds at 48. Over loopback the
   bounded concurrency made them faster than in process (377 ms p50 against 612), but a LAN adds
   14 to 22 round trips, not two. One offers call costs 24 to 48 ms p50 at the lead.
6. **Rounds against the LAN allowance.** Two or fewer on 17 of the 23 pages; three on the text
   search, the admin Atlas, its JSON and the admin Review view; 4 and 6 on the two Workareas views
   at 6 workareas, growing with N.
7. **Rate.** Every page but the two Workareas views spends at most 4 requests of any one session
   key per view, most 1 or 2. The user Workareas view spends 2N of the review key (96 at 48), the
   admin view 2N + 2 of the admin key over its N nodes (100 at 49).

## Files

| file | what it does |
|---|---|
| `run.py` | one run: a throwaway PostgreSQL cluster, the lead and (unless `--in-process`) `oml-web` as release binaries on loopback, the bootstrap claim, one allowlisted admin signed in through the real routes, the seed, then every `GET` page timed in turn; writes one `oml-web-bench/1` file |
| `before-e2e-sign-in.patch` | the sign-in double of `crates/bins/oml-server-lead/src/e2e.rs`, applied to commit `4dedf532`, the last before phase (b), so the in-process lead signs a person in without a provider call |
| `results/` | `oml-web-<N>.json` and `in-process-<N>.json`, N the workareas seeded |

## What a run measures

- **Page time**: the client's wall time per `GET`, request sent to answer read, over one
  keep-alive loopback connection to the page's server. The server's health probe, timed the same
  way after each round, is the floor of that exchange (`healthz_ms`).
- **API calls and rounds**: each page request carries `X-Request-Id: wb<n>`, which `oml-web`
  honours and numbers its calls under (`wb<n>.<k>`). The lead's request lines give each call's
  handler time, store time, route and key, and when it ended. A page's rounds are the longest
  chain of its calls that did not overlap at the lead: the round trips a LAN adds.
- **Outside the lead**: page time less the time at least one of the page's calls was in the
  lead's handler, which is `oml-web`'s own work, both sides' serialization and the loopback
  exchanges.
- **Calls by key**: how many of a page's calls each session key spent, the share of
  `quota.requests_per_minute` one page view costs that key.

The pages are sent in turn, one round of every page per iteration, so load that drifts during a
run spreads over every page alike. Percentiles are nearest rank. The seed is N workareas in the
person's root, 240 memories written by a user agent key across them, two thirds approved by two
human reviewer keys in turn and the rest held.

## Running it

A release build without the reranker and with the sign-in double, and PostgreSQL's binaries:

```sh
export CARGO_TARGET_DIR=.tmp/web-bench-target
cargo build --release --locked -p oml-server-lead --no-default-features --features e2e-sign-in -p oml-web
python3 bench/web/run.py --bin-dir "$CARGO_TARGET_DIR/release" \
    --pg-bin /usr/lib/postgresql/18/bin --work .tmp/web-bench \
    --workareas 6 --out bench/web/results/oml-web-6.json
```

The in-process side, from a worktree at `4dedf532`:

```sh
git worktree add --detach .tmp/web-before 4dedf532
git -C .tmp/web-before apply "$PWD/bench/web/before-e2e-sign-in.patch"
(cd .tmp/web-before && CARGO_TARGET_DIR=$PWD/target cargo build --release --locked \
    -p oml-server-lead --no-default-features --features e2e-sign-in)
python3 bench/web/run.py --bin-dir .tmp/web-before/target/release --in-process --commit 4dedf532 \
    --pg-bin /usr/lib/postgresql/18/bin --work .tmp/web-bench \
    --workareas 6 --out bench/web/results/in-process-6.json
```

Then `python3 scripts/gen_bench_pages.py` writes the page. A run makes its own scratch
directory, starts its cluster on a free port and removes both when it ends (`--keep` keeps the
directory); it reads neither `OML_URL` nor `OML_API_KEY`, since both processes start with an
environment of their own. `quota.requests_per_minute` is set high so no page meets `limit.rate`.
