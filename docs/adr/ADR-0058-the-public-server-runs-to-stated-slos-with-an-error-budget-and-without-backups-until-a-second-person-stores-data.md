<!--
id: adr/ADR-0058-the-public-server-runs-to-stated-slos-with-an-error-budget-and-without-backups-until-a-second-person-stores-data
tier: engineering
status: accepted · 2026-11-06
answers: where a public server's production policy lives; which SLOs it runs to, with which targets and windows, and why; what spending an error budget stops; how SLOs alert; when backups become mandatory; how it is upgraded; from which sources usage is measured and what monitoring may never read; whether the operator's organisation keeps recall query text
-->

# ADR-0058 — A public server runs to stated SLOs with an error budget

**Status:** accepted · 2026-11-06 (was proposed) · built in v0.6.0

## Context

A server that other people depend on needs a promise it defends and a rule for what a breach
stops. [Observability](../operations/observability.md#slos) fixes the SLI formulas and leaves
every target to deployment policy, so a server with alerts on fixed thresholds has no promise
behind them. Backups and the upgrade procedure were likewise unwritten: an upgrade whose
migration refused the live schema at boot cost about five minutes on the previous binaries
before the procedure below existed.

The policy is held against what a single-host deployment is: one lead, no follower, one
responder, the request log, the metrics and their closed label sets
([ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md)),
the recall telemetry of
[ADR-0054](ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md)
and tenant isolation by row security
([ADR-0034](ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md)).

## Decision

- **d1 — One page holds the deployment policy.** [Operating OML](../operations/operating.md)
  states the targets, the error budget policy, the measured capacity, the data protection,
  incident response, the upgrade procedure and the security posture. Observability keeps the
  formulas; the operating guide keeps the numbers, each with its source. Each operator sets
  their own targets; nothing here is a default of the software.

- **d2 — Six SLOs over 30-day windows, reviewed after 30 days of data.** Availability of
  `/readyz` at the edge, 99.5%; availability of MCP (no 5xx on `/mcp`), 99.5%; recall latency for
  pipelines without a reranker, 95% within 128 ms; `bm25-rerank` recall latency, 95% within
  1.024 s; write acceptance, 95% within 256 ms; review latency, 90% of verdicts within 34.1 h.
  Thresholds sit on histogram bucket edges, since a threshold between two edges is interpolated,
  not counted. The reasons for each number are in
  [operating](../operations/operating.md#slos). The cost: a new deployment's targets rest on
  few days of data and a light load, so they are revisited once Prometheus holds 30 days.

- **d3 — A spent error budget stops behaviour-changing releases.** While an SLO's 30-day SLI is
  below target, only a fix for the cause, a security fix or a rollback is deployed. Planned
  upgrades spend budget like any outage. This makes 99.5% the number that bounds how often the
  operator may restart the lead, which is the trade the target was chosen for.

- **d4 — Availability and recall-latency SLOs alert on their burn rate, in two windows each.**
  Critical at a burn rate above 14.4 over 1 h and 5 min; warning above 6 over 6 h and 30 min; the
  MCP and recall alerts also need 10 events in the hour and 20 in six hours, so one slow recall on
  a quiet night does not page. Write and review latency are shown without alerts until their
  traffic makes a rate meaningful. The fixed-threshold rules stay as symptom alerts; the recall
  p95 rule splits, 500 ms for first-stage pipelines and 900 ms for `bm25-rerank`.

- **d5 — Backups come before anyone else's data.** A deployment takes scheduled backups, a WAL
  archive or off-host dumps, and rehearses one restore, before the first of: a key or a sign-in
  allowlist entry for anyone but the operator, a second organisation, or memories the operator
  would not recreate by hand. Until then a lost volume or host loses everything since the last
  dump, and the operator knows it. Pre-upgrade dumps on the data volume protect an upgrade and
  nothing else.

- **d6 — Every upgrade is verified, rehearsed on a copy, and preceded by a dump.** Verify the
  release's signatures and sums; restore a dump into a throwaway cluster and boot the new lead
  against it; dump; keep the old binaries; restart; check `/readyz`, `oml doctor` and the probe.
  A release that fails the rehearsal does not go live. The cost is about one rehearsal per
  release in operator time.

- **d7 — Usage is measured from metrics, the request log and statistics views; monitoring never
  reads a tenant row.** Prometheus families with closed labels, the request and MCP call lines in
  the log store read per query (ids and principals never become stored series), and `pg_stat_*`
  views through a `pg_monitor` role that is never granted a tenant table, a fold table or the
  journal. What needs those rows waits for the lake
  ([ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md)).

- **d8 — The operator's organisation may keep recall query text.** The operator opts their own
  organisation in to `telemetry.query_text` (ADR-0054 d3): text kept 30 days, read by its admin
  keys only, through `oml usage settings --query-text on`, journalled as `telemetry.settings_set`.
  No other organisation's setting changes; each one decides for itself.

## Consequences

- [Operating OML](../operations/operating.md) is in the Operate section; observability's SLO
  section is unchanged and remains the formulas' owner.
- Dashboards and burn-rate rules read the SLI from one definition, so they cannot disagree.
- Items that need code (review-queue gauges, a method label or write histogram, pre-registered
  label sets, derived-layer metrics) and the measurements behind the capacity table are open
  issues: [#99](https://github.com/e8kor/open-memory-lab/issues/99),
  [#102](https://github.com/e8kor/open-memory-lab/issues/102),
  [#103](https://github.com/e8kor/open-memory-lab/issues/103).
- d5 is a decision to carry a known risk until its trigger. The trigger reopens it, not a date.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **99.9% availability** | 43 minutes a month on one host with no follower: one boot failure plus routine restarts leaves little, and a lost volume without backups breaks it outright. A target the deployment cannot meet teaches nothing. Reopened with a follower. |
| **Targets in observability.md** | That page is the software's, read by every operator; another deployment's hardware and load would inherit numbers chosen for one. |
| **Request-histogram write SLI** | `oml_http_request_duration_seconds` has no method label, so listings on `/v1/memories` would count as writes. The request log carries the method. |
| **Single-threshold SLO alerts** (bad share above budget over 5 minutes) | Pages on every slow recall at night and stays quiet on a slow leak; the two-window pairs page on a fast burn and warn on a slow one. |
| **Backups from day one on a single-user server** | Costs a daily dump, an off-host target, keyring handling and a rehearsed restore, to protect data its owner can recreate. d5's trigger adds them when that stops being true. |
| **Grant the monitoring role read access to tenant tables for usage counts** | It would make the monitoring stack a reader of every organisation's memories, outside row security's audience. The lake is the designed reader. |
