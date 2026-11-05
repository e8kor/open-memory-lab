<!--
id: adr/ADR-0058-the-public-server-runs-to-stated-slos-with-an-error-budget-and-without-backups-until-a-second-person-stores-data
tier: engineering
status: proposed
answers: where the public server's deployment policy lives; which SLOs it runs to, with which targets and windows, and why; what spending an error budget stops; how SLOs alert; why the server has no backups yet and what makes it get them; how it is upgraded; from which sources usage is measured and what monitoring may never read; whether the operator's organisation keeps recall query text
-->

# ADR-0058 — The public server runs to stated SLOs with an error budget, and without backups until a second person stores data

**Status:** proposed · 2026-11-05

**Release:** deployment policy for memory.doublegate.io; no release builds it.

## Context

The maintainers' public server, memory.doublegate.io
([ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md)), has run
since 2026-10-30 and v0.5.7 since 2026-11-05. The operator asked on 2026-11-05 to start running it
as a production system: requirements, measurements of how it is used, and metrics, dashboards and
alerts that say how it behaves.

[Observability](../operations/observability.md#slos) fixes the SLI formulas and states that
"target percentages are deployment policy and do not appear on this page". No page held that
policy, so the server had alerts on fixed thresholds (a 1 s request p95, a 500 ms recall p95, a 2%
5xx share) but no promise they defended and no rule for what a breach stops. Its data had no
backup by the operator's choice of 2026-11-04 ("the live data has no value yet"), recorded only in
the operator's notes. The upgrade procedure that grew from the v0.5.3 incident (a migration
refused the live schema at boot; about 5 minutes on the previous binaries) was likewise unwritten.

Held against: one host with one lead and no follower; one responder; traffic of 63 routed
requests in 48 hours (`oml_http_request_duration_seconds_count`, 2026-11-05); the request log,
the metrics and their closed label sets of
[ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md);
the recall telemetry of
[ADR-0054](ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md);
and tenant isolation by row security
([ADR-0034](ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md)).

## Decision

- **d1 — One page holds the public server's deployment policy.**
  [Production](../operations/production.md) states the targets, the error budget policy, the
  capacity measured, the data protection, incident response, the upgrade procedure, the security
  posture and the gap list for memory.doublegate.io. Observability keeps the formulas; this page
  keeps the numbers, each with its source. Another deployment writes its own; nothing here is a
  default of the software.

- **d2 — Six SLOs over 30-day windows, reviewed after 30 days of data.** Availability of
  `/readyz` at the edge, 99.5%; availability of MCP (no 5xx on `/mcp`), 99.5%; recall latency for
  pipelines without a reranker, 95% within 128 ms; `bm25-rerank` recall latency, 95% within
  1.024 s; write acceptance, 95% within 256 ms; review latency, 90% of verdicts within 34.1 h.
  Thresholds sit on histogram bucket edges, since a threshold between two edges is interpolated,
  not counted. The reasons for each number are in [production](../operations/production.md#slos);
  the cost is that they rest on two days of data and on a load of one person, so they are
  revisited once Prometheus holds 30 days (from 2026-12-04).

- **d3 — A spent error budget stops behaviour-changing releases to live.** While an SLO's 30-day
  SLI is below target, only a fix for the cause, a security fix or a rollback goes to live.
  Planned upgrades spend budget like any outage. This makes 99.5% the number that bounds how
  often the operator may restart the lead, which is the trade the target was chosen for.

- **d4 — Availability and recall-latency SLOs alert on their burn rate, in two windows each.**
  Critical at a burn rate above 14.4 over 1 h and 5 min; warning above 6 over 6 h and 30 min; the
  MCP and recall alerts also need 10 events in the hour and 20 in six hours, so one slow recall on
  a quiet night does not page. Write and review latency are shown without alerts until their
  traffic makes a rate meaningful. The fixed-threshold rules stay as symptom alerts; the recall
  p95 rule splits, 500 ms for first-stage pipelines and 900 ms for `bm25-rerank`, whose ordinary
  answer (about 0.5 s on live) sat on the shared line.

- **d5 — No backups until a second person stores data; the risk is stated, not hidden.** The
  server keeps no scheduled backup, no WAL archive and no off-host copy; the pre-upgrade dumps
  share the data volume and protect an upgrade only. Backups, a rehearsed restore and the
  backup-failed alert are added before the first of: a key or a sign-in allowlist entry for anyone
  but the operator, a second organisation, or memories the operator would not recreate by hand.
  Until then a lost volume or host loses everything since the last dump, and the production page
  says so in its first data-protection line.

- **d6 — Every upgrade is verified, rehearsed on a copy of live, and preceded by a dump.**
  Verify the release's signatures and sums; restore a dump of live into a throwaway cluster and
  boot the new lead against it; dump live; keep the old binaries; restart; check `/readyz`,
  `oml doctor` and the probe. A release that fails the rehearsal does not reach live. This is the
  v0.5.3 lesson as procedure; the cost is about one rehearsal per release in operator time.

- **d7 — Usage is measured from metrics, the request log and statistics views; monitoring never
  reads a tenant row.** Prometheus families with closed labels, the request and MCP call lines in
  Loki read per query (ids and principals never become stored series), and `pg_stat_*` views
  through the `prometheus` role (`pg_monitor`, read-only). That role is never granted a tenant
  table, a fold table or the journal. What needs those rows waits for the lake
  ([ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md)) and is marked "from lake (pending)" on
  the dashboards.

- **d8 — The operator's organisation keeps recall query text.** The operator opts its own
  organisation in to `telemetry.query_text` (ADR-0054 d3): text kept 30 days, read by its admin
  keys only, through `oml usage settings --query-text on`, journalled as `telemetry.settings_set`.
  No other organisation's setting changes; each one decides for itself.

## Consequences

- [Production](../operations/production.md) is added to the operations section; observability's
  SLO section is unchanged and remains the formula's owner.
- The host's monitoring generators gain an "OML usage" and an "OML SLOs and error budgets"
  dashboard and eight burn-rate rules; they read the SLI from one definition. They live on the
  host, outside this repository, which the gap list records (gap 12).
- The gap list in [production](../operations/production.md#what-production-ready-still-lacks)
  is the honesty ledger for "production-ready". Its items that need code (review-queue gauges, a
  method label or write histogram, pre-registered label sets, the designed write-path and
  derived-layer metrics) become roadmap rows when a release plans them; the measurement items are
  existing rows FR-7, FR-18, FR-25, FR-88 and FR-100.
- d5 is a decision to carry a known risk. It is reopened by its trigger, not by a date.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **99.9% availability** | 43 minutes a month on one host with no follower: one boot failure like v0.5.3 (about 5 min) plus routine restarts leaves little, and a lost volume without backups breaks it outright. A target the deployment cannot meet teaches nothing. Reopened with a follower (gap 2). |
| **Targets in observability.md** | That page is the software's, read by every operator; another deployment's hardware and load would inherit numbers chosen for this one. |
| **Request-histogram write SLI** | `oml_http_request_duration_seconds` has no method label, and on live the `GET` listings on `/v1/memories` outnumber the writes (7 to 0 in the request log, 2026-11-05), so the SLI would measure reads. The request log carries the method. |
| **Single-threshold SLO alerts** (bad share above budget over 5 minutes) | Pages on every slow recall at night and stays quiet on a slow leak; the two-window pairs page on a fast burn and warn on a slow one. |
| **Backups now** | Costs a daily dump, an off-host target, keyring handling and a rehearsed restore, to protect 7 memory versions of test data. Accepted again by the trigger in d5, not postponed indefinitely. |
| **Grant the monitoring role read access to tenant tables for usage counts** | It would make the monitoring stack a reader of every organisation's memories, outside row security's audience. The lake is the designed reader. |
