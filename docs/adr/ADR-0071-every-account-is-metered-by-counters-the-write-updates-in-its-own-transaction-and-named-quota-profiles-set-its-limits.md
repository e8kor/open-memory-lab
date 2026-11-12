<!--
id: adr/ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits
tier: engineering
status: proposed
answers: what an account is and which column names it; the six meters and exactly what each counts; how a quota check stays exact when two writers race at the limit; how doctor finds a counter that drifted from its source; the daily usage ledger, its retention and why no meter is a per-account metric label; named quota profiles, an account's profile and overrides, the 80% warning and the 100% refusal; the endpoints that read usage and set a profile; what the pages show; what the counts never hold; what it amends in ADR-0068
-->

# ADR-0071 — Every account is metered by counters the write updates in its own transaction, and named quota profiles set its limits

**Status:** proposed · 2026-11-06

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
    | `writes` | versions a key of the account submitted in the UTC calendar month: every remember, learn and supersede that was held. A refused write and an idempotent replay are not writes | flow, per month |
    | `recalls` | recall requests a key of the account made in the UTC calendar month, on HTTP or MCP, whatever they returned | flow, per month |
    | `keys` | the account's live bearer and connector keys; session keys are not counted (ADR-0068 d5) | stock |
    | `connections` | open OAuth connections of those keys (ADR-0068 d5) | stock |

    No attachment meter exists, because every write refuses attachments. One joins when
    attachments are built. The `memories` definition replaces ADR-0068 d5's "every state", the
    operator's recommendation of 2026-11-06. A spammer's rejected writes still cost them
    `text_bytes` and `writes`, so they are not free.

- **d3 — A quota check is exact: the write's own transaction moves the account's counter and
  refuses past the limit.** `account_meters` holds one row per account with its four stocks.
  `account_activity` holds one row per account per UTC day with its two flows. The commit that
  folds a metered event updates them in its own transaction, under the tenant's chain lock
  (ADR-0002 d3). The events are a held version, a promotion, a rejection, a supersession, a key
  issued or revoked, and a connection opened or revoked. For a memory fold the store reads whether the
  chain is live before and after the fold, both by index on the chain, and moves `memories` by
  the difference. The commit carries the account's limits. When a meter the commit raises passes
  its limit, the store refuses the commit with `quota_reached`, and the transaction rolls back
  with the journal row. The client gets `429 limit.quota`
  `{quota, limit, used}` and nothing is journalled. The application checks the same counters
  before it signs, so most refusals cost no signature. The check inside the commit is the one
  that holds. Of two writers racing at the last free memory, exactly one commits. Both checks
  follow every idempotency replay: a write retried under its key answers its record at the limit,
  and a commit under a recorded key is refused as recorded before its ceiling.

    Counters can drift from their rows only through a bug or a hand edit. `GET
    /v2/admin/usage/drift` recomputes every account's stocks from the rows, by the definitions
    of d2. It also sums the month's flows from `account_activity`. It lists each account whose
    counter differs. `oml doctor` under an admin key prints `warn usage: <n> accounts drift from
    their rows (<user> <meter> counter <a> rows <b>, …)`, and nothing when none does. Nothing
    repairs a counter by itself. A silent repair would hide the bug that caused the drift.

- **d4 — Usage history is a ledger of daily rows, appended once and kept 25 months. No meter is a
  per-account metric label.** `usage_daily` holds one row per account per closed UTC day: the
  four stocks as the rollup read them, that day's two flows, and the account's profile. The
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
    - **A profile that disappeared.** An account whose profile the configuration no longer declares
      is held to zero on every quota until an admin reassigns it. It fails closed, and `oml
      doctor` prints `warn usage: <n> accounts on profile <name>, which the configuration does
      not declare`.
    - **The two thresholds.** A meter at 80% of its limit or more is in state `warning`. At its
      limit it is in state `reached`, and the next write that would pass the limit is refused
      `limit.quota`. The usage endpoints, `/web/start` and the account page show both states.
      The doctor line of ADR-0068 d5 moves from 90% to 80%, so there is one threshold.
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
    | `GET /v2/admin/usage/drift` | an admin key | the drift report of d3 |

    The own-usage route takes no account parameter, so no request can name another account. A
    tenant key, which belongs to no account, gets `access.not_found`. The admin routes sit on
    the lab ring with the other admin routes (ADR-0022), not on `/v1`. `/web/start` and the
    account page show the person's meters as `oml-meter` bars with their state. `/web/admin/users`
    lists every account with its profile and meters. The pages read through the same use cases
    as the endpoints.

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

    **As landed on main, after ADR-0070 (2026-11-12).** The pages are `oml-web`'s and read over
    `/v2`, so the lead serves none of the three named above. `GET /v2/whoami`'s `person` and each
    row of `GET /v2/web/users` carry the `text_bytes` and `writes` counts beside the ones they had.
    They also carry the `text_bytes` and `writes_per_month` limits, null when unlimited. The
    onboarding and users pages show memories, keys and connections as before. The meters card with
    its two states, and the profile and meter columns of the users page, are not built in `oml-web`.
    The five routes above take a key and no session credential (ADR-0070 d2).

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
    | an account on an undeclared profile | every quota is zero, `limit.quota` | `warn usage: … profile` (d5) |
    | `PUT …/profile` naming an undeclared profile or quota | `request.validation`, `profile` / `overrides` | — |
    | a rollup pass that fails | — | log `usage_rollup_failed`; the next pass appends what it missed |
    | a recall count that fails to write | — (the recall is served: reading never blocks) | the month's `recalls` reads low; nothing recomputes it |
    | `period` not `YYYY-MM` | `request.validation`, `period` | — |

## Consequences

- Migration 0044 adds `account_meters`, `account_activity`, `usage_daily` and `account_profiles`
  under the tenant RLS of 0004. It widens the `user_quotas` check to six quotas and sets the
  serving grants (`usage_daily`: SELECT, INSERT and DELETE for the 25-month retention, no UPDATE).
  It fills `account_meters` and `account_activity` from the rows already stored, so a live
  database starts with no drift.
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
