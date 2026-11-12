<!--
id: adr/ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits
tier: engineering
status: accepted design · 2026-11-12
answers: what an account is and which column names it; the six meters and exactly what each counts; how a quota check stays exact when two writers race at the limit; how doctor finds a counter that drifted from its source; the daily usage ledger, its retention and why no meter is a per-account metric label; named quota profiles, an account's profile and overrides, the 80% warning and the 100% refusal; the endpoints that read usage and set a profile; what the pages show; what the counts never hold; what it amends in ADR-0068
-->

# ADR-0071 — Every account is metered by counters the write updates in its own transaction, and named quota profiles set its limits

**Status:** accepted design · 2026-11-12, by the operator, for usage metering; proposed 2026-11-06. The acceptance covers d5's un-assigning, written on 2026-11-12 after the review of the port.

**Release:** v0.7.3, the release `openapi.yaml` names for the new members

**Amends:** ADR-0068 (d5: the `memories` quota counts live chains, not every state; two quotas join; the
doctor threshold moves from 90% to 80%; usage is read from counters, not counted at each check)

## Context

The operator asked, on 2026-11-06, to track memories per account to hold quotas for the public
server's monetisation. Open sign-up
([ADR-0068](ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md)
d5) checks four quotas by counting rows at each check: distinct memories in the root, live keys,
open connections, and a per-key request rate. A count read before the commit is not exact. Two
writers at 999 of 1,000 both read 999 and both commit. The memories count is also a scan that grows
with the person's store.

Plans, prices and billing are the operator's and stay out of this repository. A self-hoster needs
the same counts and limits without them. So the code provides meters, a ledger and named quota
profiles, and the operator names the profiles and their numbers in configuration.

Two existing records bound this one. Recall telemetry
([ADR-0054](ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md))
records each recall, with its query text only where a tenant opts in. The request log
([ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md))
keeps one line per request for 30 days and keeps per-key detail out of Prometheus (d12). A user root
is a person's space ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
d2, d3; [ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md)).

## Decision

- **d1 — An account is a person: their user root and everything their keys write. The column is
  `user_root`.** An account is named by the id of the person's root workarea `universe.<user>`,
  the `user_root` column of every table below, as in `user_quotas`. A key belongs to the account
  its `api_keys.user_root_id` names, session keys included. A write is charged to the account of
  the key that wrote it. A tenant key, whose `user_root_id` is null, belongs to no account. Its
  writes are counted in the tenant's aggregate metrics only. A key a person's root holds cannot
  write outside it (ADR-0044 d3), so "what their keys write" and "what lies in their root" name
  the same rows today. The key decides because it stays true if a grant outside the root is ever
  allowed.

- **d2 — Six meters, each with one definition.**

    | Meter | Counts | Kind |
    |---|---|---|
    | `memories` | live chains the account started: a chain (`workarea_id`, `claim_key`) whose first version a key of the account wrote, that holds a version `held`, `promoted` or `demoted`, and whose promoted tip is not a retirement. A rejected-only, retired or erased chain is not counted. A new version of a chain is not a new memory. | stock |
    | `text_bytes` | UTF-8 bytes of the claim text, as captured after NFC, of every version a key of the account wrote, rejected versions included, since R4 keeps them | stock |
    | `writes` | versions a key of the account submitted in the UTC calendar month: every remember, learn and supersede that was held, counted on the UTC day of the version's own `created_at`. A refused write and an idempotent replay are not writes | flow, per month |
    | `recalls` | recall requests a key of the account made in the UTC calendar month, on HTTP or MCP, whatever they returned. A recall hands its count to a queue and never waits for the write (d8) | flow, per month |
    | `keys` | the account's live bearer and connector keys; session keys are not counted (ADR-0068 d5) | stock |
    | `connections` | open OAuth connections of those keys, the live ones: a revoked key's open connection leaves the meter with the key (ADR-0068 d5) | stock |

    No attachment meter exists, because every write refuses attachments. One joins when
    attachments are built. The `memories` definition replaces ADR-0068 d5's "every state", the
    operator's recommendation of 2026-11-06. A spammer's rejected writes still cost them
    `text_bytes` and `writes`, so they are not free.

- **d3 — A quota check is exact: the write's own transaction moves the account's counter and
  refuses past the limit.** `account_meters` holds one row per account with its four stocks.
  `account_activity` holds one row per account per UTC day with its two flows. The commit that
  folds a metered event updates them in its own transaction, under the tenant's chain lock
  (ADR-0002 d3). The events are a held version, a promotion, a rejection, a supersession, a key
  issued or revoked, and a connection opened or revoked. A version captured under a
  `claim_key` no row carries starts its chain, which is its author's account's and live once
  held, so that commit reads nothing. A chain's first version is its earliest version 1: a
  `claim_key` whose every version was rejected is released, and the claim captured under it
  next is a version 1 that joins the chain (ADR-0025 d18). For that write and any other memory
  fold the store reads the chain's account and whether the chain is live before the fold, and
  whether it is live after, and moves `memories` by the difference. Each read is an
  index probe on the chain: `memories_chain_first` for the account, `memories_claim_version` and
  `memories_served_tip` for the liveness. A chain that is no account's is read once and no meter
  moves. An account's stocks move in one statement and its day's writes in another, so inside
  the lock a new memory by an account key costs two statements, a revision or a claim under a
  released key four, a promotion or a rejection two or three, one on a chain that is no
  account's, and a tenant key's new claim none. The application reads the
  account's limits once, before the head, and the commit carries them. When a meter the commit raises passes
  its limit, the store refuses the commit with `quota_reached`, and the transaction rolls back
  with the journal row. The client gets `429 limit.quota`
  `{quota, limit, used}` and nothing is journalled. The application checks the same counters
  before it signs, so most refusals cost no signature. The check inside the commit is the one
  that holds. Of two writers racing at the last free memory, exactly one commits. Both checks
  follow every idempotency replay: a write retried under its key answers its record at the limit,
  and a commit under a recorded key is refused as recorded before its ceiling. A chain's account
  is the author of its earliest version 1. The review of the build measured two outcomes of
  that on 2026-11-12, for a tenant key that wrote in a person's root: a tenant writer key that
  revived a person's released `claim_key` there raised that person's `memories` with no check
  against their ceiling, and a person who claims a key a tenant key first used there is charged
  no memory; their bytes and their write are counted. From v0.7.3 a new memory in a person's root
  by a key that is not theirs is refused `submission.user_root_refused`
  ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
  d16, proposed; #329), so no new write produces the first outcome, and the second arises only
  over a version 1 a tenant key wrote there before. The versions written before keep the
  accounting this paragraph states, and the drift report recomputes it the same way. A successor
  never starts a chain or changes its account. One raises `memories` when it is ratified, the
  successor of a ratified retirement, which makes the chain live again: in a person's root d16
  refuses it from any key but theirs, and from their own keys it is held to no ceiling, since a
  ratification's commit carries none (#340).

    Counters can drift from their rows only through a bug or a hand edit. `GET
    /v2/admin/usage/drift` recomputes every account's stocks from the rows, by the definitions
    of d2. It also sums the month's flows from `account_activity`. It lists each account whose
    counter differs. `oml doctor` under an admin key prints `warn usage: <n> accounts drift from
    their rows (<user> <meter> counter <a> rows <b>, …)`, and nothing when none does. Nothing
    repairs a counter by itself. A silent repair would hide the bug that caused the drift. A
    counter never goes below zero: the table refuses it, and a fold that takes one off a counter
    already at zero, which only a drifted counter is, leaves it there and commits, so no
    rejection or revocation is refused for its account's bookkeeping. The
    [runbook](../operations/runbooks.md#a-usage-counter-drifts-from-its-rows) says what an
    operator does with a drift.

- **d4 — Usage history is a ledger of daily rows, appended once and kept 25 months. No meter is a
  per-account metric label.** `usage_daily` holds one row per account per closed UTC day: the
  four stocks as the rollup read them, that day's two flows, and the profile the account is on:
  the one an admin assigned, else `quota.signup_profile` in the sign-up tenant, and null for an
  account under no limits (d5). The row keeps that name after the configuration moves on. The
  lead rolls up every hour. Each pass appends the rows of every closed day after the last
  rolled-up one, `ON CONFLICT DO NOTHING`, so two leads append each row once. The serving role
  may insert and read the rows, never update them. The same pass deletes rows older than 25
  months and `account_activity` rows older than 62 days. A stock is the value at the rollup,
  within the hour after midnight UTC, not at midnight itself. The cost is that a write in that
  hour lands in the earlier day's stock. Prometheus gets aggregates only: the closed `quota`
  label of `oml_quota_refused_total` (ADR-0042 d12) gains the two new quotas, and no account
  ever becomes a label.

- **d5 — A quota profile is a named set of limits in configuration. An account points at one,
  and an admin may override one limit, journalled.**
    - **Profiles.** `default` is built from the `quota.*` settings, as ADR-0068 d1 set them. `quota.profiles`
      lists further names. Each named profile reads `quota.profile.<name>.<quota>`, and a limit it
      does not set is `default`'s. The quotas are `memories`, `text_bytes`, `writes_per_month`,
      `keys`, `connections` and `requests_per_minute`. `text_bytes` and `writes_per_month` are
      unlimited in `default` until set.
    - **Which profile.** An account the operator has not assigned is on `quota.signup_profile`
      (default `default`) when it is in the sign-up tenant, and under no limits otherwise. It is
      metered either way.
    - **Assigning.** `PUT /v2/admin/accounts/{account}/profile` `{profile, overrides}` replaces the
      account's profile and its overrides. It is journalled as `account.profile_set` and folded
      into `account_profiles` and `user_quotas`. `web.quota_set` keeps writing one override.
    - **Un-assigning.** `DELETE /v2/admin/accounts/{account}/profile` takes
      the assigned profile and every override away. It is journalled as `account.profile_set`
      with a null `profile` and no overrides, and the account is on the deployment's default
      again: `quota.signup_profile` in the sign-up tenant, no limits outside it. An admin key
      alone may; every other role is `access.verb_ceiling`. It deletes no memory, key or count.
      Without it an account outside the sign-up tenant could not return to no limits once
      assigned.
    - **A profile that disappeared.** An account whose profile the configuration no longer declares
      is refused every write, key and connection as `limit.quota`: those five quotas are zero
      until an admin reassigns it or the configuration declares the profile again. It fails
      closed for writes and still reads, at `default`'s request rate, since reading never
      blocks. `oml doctor` prints `warn usage: <n> accounts on profile <name>, which the
      configuration does not declare`.
    - **The two thresholds.** A meter at 80% of its limit or more is in state `warning`. At its
      limit it is in state `reached`, and the next write that would pass the limit is refused
      `limit.quota`. The usage endpoints carry both states, and so do the pages for the five
      meters a console session can read (d6, as amended 2026-11-12). The doctor line of ADR-0068 d5 moves from 90% to 80%, so there is one
      threshold.
    - **Reading never blocks.** No quota refuses a recall, a get or a list, and `recalls` has no
      limit. `requests_per_minute` stays a rate (ADR-0068 d5), not a meter.

    Plans, prices and the mapping of a plan to a profile are the operator's. No profile name but
    `default` appears in code.

- **d6 — Usage is read and set through documented endpoints, and the pages are their clients.**

    | Route | Who | Answers |
    |---|---|---|
    | `GET /v1/account/usage` | any key of an account, for its own account | the account's meters, limits, percentages, states, profile and month |
    | `GET /v2/admin/accounts/{account}/usage` | an admin key of the tenant | the same, for one account by root id or user name |
    | `GET /v2/admin/usage?period=YYYY-MM[&format=csv]` | an admin key | the month's ledger rows, every account, as JSON or CSV for billing |
    | `PUT /v2/admin/accounts/{account}/profile` | an admin key | `account.profile_set` (d5) |
    | `DELETE /v2/admin/accounts/{account}/profile` | an admin key | `account.profile_set` with no profile (d5) |
    | `GET /v2/admin/usage/drift` | an admin key | the drift report of d3 |

    The own-usage route takes no account parameter, so no request can name another account. A
    tenant key, which belongs to no account, gets `access.not_found`. The admin routes sit on
    the lab ring with the other admin routes (ADR-0022), not on `/v1`. The pages are `oml-web`'s
    since ADR-0070 and read what a console session can read, which is `person` on `GET /v2/whoami`
    and the rows of `GET /v2/web/users`. The note below says what they show and what they do not.

    **The ledger's consumer is outside this repository.** `GET /v2/admin/usage?period=YYYY-MM` is the
    whole export: the closed UTC days of one month, one row per account per day, read with an admin
    key. Add `format=csv` for `text/csv`, whose header is
    `day,account,user,profile,memories,text_bytes,writes,recalls,keys,connections`. Without it the
    answer is `{"period", "rows"}`, each row holding `day`, `account`, `user`, `profile` and the six
    meters under those names. `period` is required and a bad one is refused `validation`. Every value
    is a count or an id (d7), so the file holds no memory text, query or address. Prices, plans and
    the billing run that reads this file live in a private repository the operator keeps, and no
    billing code is in this one. A stock meter is the value at the rollup (d4), so a consumer that
    totals a month sums the flows (`writes`, `recalls`) and reads the stocks as they stand on
    each day. (`crates/transports/oml-transport-http/src/metering.rs`, `admin_usage_route`.)

    **As landed on main, after ADR-0070 (2026-11-12), for v0.7.3, amended the same day.** The
    pages are `oml-web`'s and read over `/v2`, and the lead draws no page.
    `GET /v2/whoami`'s `person` and each row of `GET /v2/web/users` carry the `text_bytes` and
    `writes` counts beside the ones they had. They also carry the `text_bytes` and
    `writes_per_month` limits, null when unlimited. A "Your usage" card on the Start page and on
    the Keys page, which is the person's page, draws the five meters those fields carry:
    memories, text stored, writes this month, keys and connections, each with its count, its limit
    where one holds, and a mark at 80% of the limit and at the limit by the rule of d5, and the
    request rate beside them. The users page draws the same five as columns, a dash where the API
    served no count, and a mark only on a row whose `under_quotas` is true. Three things are not
    on those fields and are not shown: the profile's name, the month, and the `recalls` count.
    The six routes above (five paths; `PUT` and `DELETE` share one) take a key and no session
    credential (ADR-0070 d2), so the pages do not read them, and a person outside the sign-up
    tenant whom no profile limits has no `usage` on `person`, so their pages show no card.
    Showing the three needs a route a console session may call or three fields on `person` and
    on the users rows; neither is built (#322). The users page has no act that assigns or takes
    away a profile, for the same reason; that is not built either (#322). Its "Set a quota" form
    sets `memories`, `keys`, `connections` and `requests_per_minute`, the four
    `POST /v2/web/users/{user}/quota` takes; `text_bytes` and `writes_per_month` are set through
    the profile route with an admin key, and the card and the page say so. The Keys page gained
    one call for the card, `GET /v2/whoami` with the person block, sent in the same round as the
    key list. The call is unmeasured, on a page measured inside its budget without it: +1.2 and
    +1.9 ms p95 after #309 ([web benchmark](../benchmarks/web.md); #322).

- **d7 — Meters are counts. They never hold text, a query or an address.** `account_meters`,
  `account_activity` and `usage_daily` hold ids, a profile name, dates and integers. A recall
  adds one to a count and records nothing of what was asked. The privacy policy names the
  counts, their purpose and their 25 months. That line is the operator's, in the private legal
  texts.

- **d8 — The failures, their refusals and their doctor lines.**

    | Failure | Refusal | Doctor line |
    |---|---|---|
    | a write, key or connection past its limit | `429 limit.quota` `{quota, limit, used}` | `warn quota` at 80% (ADR-0068 d5) |
    | two writers at the last free unit | the second gets `limit.quota` from the commit | — |
    | a counter that differs from its rows | — | `warn usage: … drift` (d3) |
    | an account on an undeclared profile | every write, key and connection is `limit.quota`; reads answer | `warn usage: … profile` (d5) |
    | the drift report cannot be read | — | `warn usage: the drift report was not read (<reason>)` |
    | `PUT …/profile` naming an undeclared profile or quota | `request.validation`, `profile` / `overrides` | — |
    | a rollup pass that fails | — | log `usage_rollup_failed`; the next pass appends what it missed |
    | a recall count that the queue drops or that fails to write | — (the recall is served: reading never blocks) | log `usage_recall_counts_lost` with the count; the month's `recalls` reads low; nothing recomputes it |
    | `period` not `YYYY-MM` | `request.validation`, `period` | — |

## Consequences

- Migration 0045 adds `account_meters`, `account_activity`, `usage_daily` and `account_profiles`
  under the tenant RLS of 0004, and the index `memories_chain_first`, each chain's versions 1
  by when they were stored, on every tenant's partition of `memories`. It widens the `user_quotas` check to six quotas and sets the
  serving grants (`usage_daily`: SELECT, INSERT and DELETE for the 25-month retention, no UPDATE).
  It fills `account_meters` and `account_activity` from the rows already stored, so a live
  database starts with no drift: each account's `memories`, `text_bytes`, `keys` and
  `connections` by d2's definitions, and its `writes` per day for the last 62 days from each
  stored version's `created_at`. `recalls` starts at zero, since nothing stored counts them. It
  assigns no profile and adds no limit. The migration's header names the locks it holds and its
  measured time, the index build's and the fill's: 550 to 634 ms with 100,000 versions stored.
  It runs at the lead's start, so that time is the upgrade's.
- Two things a deployment with open sign-up sees change, beside the meters. An admin key that
  issues a key naming a person is now held to that person's `keys` quota by the commit's
  ceiling, so a sixth key for a person at five is `limit.quota`; ADR-0068 d5 checked the
  person's own issuance only. And `oml doctor` warns at 80% of a quota where it warned at 90%.
  `memories` cannot count more than ADR-0068 d5 counted for any account: a person's keys write
  only inside their root (d1), and rejected-only and retired chains no longer count.
- `account.profile_set` joins the [event catalog](../data/event-catalog.md). `limit.quota`'s
  `details.quota` gains `text_bytes` and `writes_per_month`.
- [http-api](../api/http-api.md) and openapi.yaml gain the five routes, and the SDKs are
  regenerated.
- [configuration](../operations/configuration.md) gains `quota.profiles`,
  `quota.profile.<name>.<quota>`, `quota.signup_profile`, `quota.text_bytes` and
  `quota.writes_per_month`.
- The rollup pass's cost on a large deployment is one insert per account per day, unmeasured
  ([#164](https://github.com/e8kor/open-memory-lab/issues/164) tracks the quota defaults with it).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Count rows at each check (ADR-0068 d5 as built)** | Not exact: the count is read before the commit, so two writers at the limit both pass. The memories count is also a scan that grows with the store. |
| **A trigger on `memories` maintaining the counters** | Exact too, but it moves the meter's definition out of the fold, which is where every other row change is decided. The fold also knows which key wrote and which limits hold. A trigger knows neither. |
| **Count every version, or every state, as a memory** | Versions would punish revising a fact, which is what supersession is for. Counting rejected chains would let a reviewer's rejections fill a person's quota. `text_bytes` and `writes` charge for those writes instead. |
| **A metering service reading the journal or the outbox** | Eventually consistent, so it cannot refuse a write at the limit. It would also add a second consumer of the journal for a count the commit already knows. |
| **Per-account labels on Prometheus metrics** | One series per person per meter breaks the series budget of ADR-0042 d12 and puts account ids in a system with no erasure. The ledger is the per-account record. |
| **Profiles as database rows edited through the API** | A profile is the operator's plan, decided with prices that live outside this repository. Configuration keeps it next to them, and a change is a deploy the operator reviews. Assigning an account to a profile is per-person data, so it is journalled. |
| **Plans and prices in code** | They are the operator's commercial terms, which change without a release, and a self-hoster has none. A named profile is the whole mechanism the code needs. |
| **Repair drift automatically in the doctor pass** | A drift is a bug in a fold or a hand edit. Rewriting the counter would hide it and change a person's limit without a journal entry. |
| **Admin usage routes on `/v1` as asked** | `/v1` is the provider ring that agents speak. Admin acts live on `/v2` with the other admin routes (ADR-0022), so the own-usage read is `/v1` and the admin routes are `/v2/admin/…`. |
