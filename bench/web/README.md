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
   workareas, by +6.2 to +42.7 ms p95 (memories +6.2, admin memory +9.1), and on 15 of 21 at 48,
   by +10.1 to +31.2 ms. Within it at 6: the user Workareas view, the Atlas, both Atlas JSON
   routes and the data export, each dominated by one heavy read the move did not change. v0.7.0
   ships with this miss stated, by the operator's decision of 2026-11-11; v0.7.1 fixes it with
   fewer calls per page ([#286](https://github.com/e8kor/open-memory-lab/issues/286)): a `whoami`
   that resolves once and reads the person block only when asked, the search's key read beside
   the recall, and the subtree's offers from the tree call.
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

Two readings follow, offered for the operator's acceptance and not yet decisions (ADR-0070's
table of 2026-11-11): the per-session in-flight cap of d3 layer 6 is not needed, since no page
but the Workareas views has more than four calls in flight and those are held to `FAN_OUT`; and
pages need no request-rate share of their own, since the Workareas views need fewer calls
instead.

## Findings of 2026-11-12 (#286)

`oml-web-6` and `oml-web-48` again after
[#286](https://github.com/e8kor/open-memory-lab/issues/286): the session check's `whoami` sends
`person=false` and resolves its credential once, the text search reads the person's keys beside
the recall, and the admin Atlas reads the scope's offers in its counted tree. Same seed, same
host. `oml-web-6` ran at `b3c7a757`, its one-minute load 1.92 to 2.35; `oml-web-48` at `cdf88fe0`
(the same code but the admin Atlas's fallback for an API one release behind), 1.31 to 2.44. The
results files hold these runs; the runs of 2026-11-11 are in git history and in ADR-0070's table of
that date.

1. **The budget is missed on 7 of the 21 pages at 6 workareas**, by +9.1 to +35.7 ms p95, where 16
   missed: the admin Workareas view (+35.7), the admin Atlas (+9.1), the memory page (+9.5), Keys
   (+11.5), Start (+13.4), People (+10.7) and Users (+11.8).
2. **At 48 workareas it is missed on 11 of 21**, by +5.1 to +15.0 ms, where 15 missed: the
   guardrails (+5.4), the admin Memories list (+8.9), the admin Atlas (+9.1), the memory page
   (+10.4), the admin memory page (+9.5), Curate (+5.8), Keys (+12.4), Start (+15.0), the Start
   status (+5.1), People (+11.3) and Users (+12.3).
3. **Where the two lists differ.** The admin Workareas view misses at 6 workareas and not at 48
   (-151.5): its in-process counterpart grows with the workareas faster than its bounded fan-out.
   The guardrails, Curate and the Start status sit within 1 ms of the line at both sizes. The admin
   Memories list and the admin memory page are within it at 6 (+1.1, +0.8) and over it at 48 (+8.9,
   +9.5). A first 48-workarea run at `b3c7a757` showed the admin Workareas view at +61.9 ms p95
   (532 ms p50); it ended at a load of 8.67 as another job started on the host, and run again at
   1.31 to 2.44 the view takes 349 ms p50 with the same 150 calls and the same store calls per call.
4. **The session check costs about half what it did.** The check's `whoami` takes 3.2 to 7.0 ms p50
   at the lead where it took 7.8 to 17.4; Review and Setup, which call nothing else, add +3.4 to
   +4.5 ms p95, within the budget, where they added +9.5 to +10.8.
5. **The text search and the admin Atlas make two rounds.** The search adds +3.8 ms p95 at 6
   workareas and -2.3 at 48 where it added +32.0 and +31.2; the Atlas +9.1 at both where it added
   +20.4 and +21.4, its counted tree alone taking about the in-process page's whole time at the lead.
6. **What still misses at both sizes.** The admin Atlas, the memory page, Keys, Start, People and
   Users: the session check's round, and for the five pages of three calls in two rounds the session
   gate's reads paid twice. The hop outside the lead stays at most 3.7 ms p95 at both sizes.

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
