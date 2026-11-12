---
status: Stable
verified: 2026-11-06
answers: "which SLOs an operator can run an OML server to, how the error budget governs releases, what capacity has been measured, and how an alert becomes an action"
---

# Capacity and SLOs

[Observability](observability.md) fixes the SLI formulas and leaves every target to you. This page is a
worked policy for one server on one host. It gives the targets, what a spent error budget stops, the
capacity measured so far, and the path from an alert to a person. Another deployment sets its own
numbers. Each number below names its source.

## SLOs

Each SLO is a target over a rolling 30-day window. Its error budget is `1 − target` of the window's
events. Thresholds sit on histogram bucket edges, because the request and recall histograms double
from 1 ms, and a threshold between two edges is interpolated, not counted.

| SLO | SLI (good / all) | Target, 30 d | Error budget | Why this target |
|---|---|---|---|---|
| Availability, `/readyz` | blackbox probes through the edge, every 30 s, answering 200 | 99.5% | 3 h 36 min of failing probes | One server, no failover ([deploy](deployment.md)). Every upgrade restarts it, and a host reboot takes minutes. 99.9% leaves 43 min a month. The probe counts a full outage, which request metrics cannot, because a server that is down exports nothing. |
| Availability, MCP | `/mcp` requests answered without a 5xx | 99.5% | 0.5% of requests | A 401 or a tool error is the caller's fault and counts as good. The target matches `/readyz`, because they fail together. |
| Recall latency, first-stage pipelines | recalls without a reranker within 128 ms | 95% | 5% | `bm25-postings` measured a 25.5 ms universe p95 at 165,000 memories (`bench/scale/REPORT.md`) against a 50 ms bar. 128 ms is the next bucket edge, with room for the server's own work. |
| Recall latency, `bm25-rerank` | recalls within 1.024 s | 95% | 5% | The rerank deadline is 1,000 ms (`search.rerank.timeout_ms`). Measured p95 is 246 ms at one client and 721 ms at four. |
| Write acceptance | `POST /v1/memories`, supersede and `oml_write` within 256 ms, from the request log | 95% | 5% | The held insert costs 8.08 ms p95 at 165,000 memories (`bench/scale/REPORT.md`, write cost); 256 ms is about 30 times that. Read from the log store, because the request histogram has no method label. |
| Review latency | verdicts within 34.1 h of the write, `oml_review_wait_seconds` | 90% | 10% | Review is a person's act. The histogram counts at the verdict, so an item still waiting stays invisible until judged. |

### Error budget policy

- **Budget left:** releases go live as they are cut, each rehearsed ([Upgrade OML](upgrades.md)).
- **Budget spent for an SLO:** no behavior-changing release goes live until that SLO's 30-day SLI
  is back above target. A fix for the cause, a security fix and a rollback go at once. Record the
  cause beside the incident.
- **A planned upgrade spends budget like any outage.** None is excused.
- **Thin traffic.** With fewer than 20 events in a window, one slow event is 5% of it. So the
  burn-rate alerts need a minimum count, and you judge the budget over 30 days.

### Burn-rate alerts

Alert on availability and recall latency with multiwindow, multi-burn-rate rules, as the Google SRE
workbook chapter "Alerting on SLOs" describes:

| Tier | Long window | Short window | Burn rate | Budget spent at that rate | Severity |
|---|---|---|---|---|---|
| fast | 1 h | 5 min | above 14.4 | 2% of 30 days' budget in 1 h | critical |
| slow | 6 h | 30 min | above 6 | 5% in 6 h | warning |

Both windows must exceed the rate, so an alert stops soon after the burning stops. The MCP and
recall alerts also need at least 10 events in the 1 h window and 20 in the 6 h window. Generate the
rules and the SLO dashboard from one SLI definition, so they cannot disagree.

## Measure capacity

All figures come from an 8-vCPU, 15 GiB host shared with other work, never an idle host. Every latency
is for one client unless a row says otherwise.

| Quantity | Measured | Source |
|---|---|---|
| `bm25-postings` recall, universe scope, 99,721 readable rows (165,000 memories) | p95 25.5 ms; `bm25`'s scan took 485 ms in the same run | `bench/scale/REPORT.md`, whole-words run |
| Same, at 100,000 and 10,000 memories | universe p95 26.6 and 14.0 ms | `bench/scale/REPORT.md`, headline table |
| A key's own held rows, 165,000 memories | 1,000 held: 59.2 ms p95; 10,000 held: 266 ms | `bench/scale/REPORT.md`, own held rows |
| Postings storage | 1.1 GiB per 165,000 memories; 6.7 GiB per 1,000,000 by linear extrapolation, not measured | `bench/scale/REPORT.md`, space table |
| Write cost in the store, 165,000 memories | insert held 2.40 / 8.08 ms, promote 4.54 / 8.94 ms, supersede 6.35 / 12.6 ms (p50 / p95) | `bench/scale/REPORT.md`, write cost |
| Postings fill (migration 0021) | 70.4 s at 165,000 memories, about 0.4 s per 1,000 | `bench/scale/REPORT.md` |
| Learned sparse recall, 100,000 memories | universe p95 164 ms, over the 50 ms bar, so it is off by default | `bench/scale/REPORT.md`, learned sparse lane |
| The widen hint's signal, 100,000 synthetic rows over 50 workareas | built in 1.4 s off the request path; 1,332,730 counts, about 21 MB; peak resident set 72.5 MB over the whole run, the SQL data load included, 63.4 MB above the 9.1 MB before the build; a `scope` recall with it held p95 14.6 ms (2026-11-12, host load average 2.6 to 7.9) | `oml-pg`'s `federation_timing`, `the_widen_signal_on_a_tenant_of_100k_rows` |
| Review queue under load | six reviewer keys approved about 77 rows a second with 5,000 held, 2 to 14 a second with 45,000 held, before the review index | `eval/bench/REPORT.md` |

The widen hint's signals take at most about 128 MB of the lead's memory, 32 MB per tenant, and one build at a time adds up to about 100 MB while it runs: 63.4 MB was measured for a signal of 1.33 million counts, and 100 MB is that figure scaled to a tenant's 2 million-count share ([ADR-0072](../adr/ADR-0072-recall-searches-the-trusted-workareas-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md#the-widen-hint-as-built-2026-11-11)). Size the lead's memory limit with about 230 MB of headroom for them.

The limits bind in this order as a server grows: concurrent reranked recalls, the data volume, and a key
holding thousands of unreviewed rows. More than four reranked recalls at once start to time out and fall back
to the first-stage order.

We have not measured 1,000,000 memories, cold caches, an idle host or the per-tenant write ceiling under the
chain lock.

## Protect the data

Back up the database and the keyring together ([Back up and restore](restore.md)). Without a scheduled
backup, a WAL archive and an off-host copy, a failed disk loses every memory, journal event and key since
the last dump. Losing the keyring alone leaves the chain readable and verifiable by its public key, but the
server can no longer extend it ([runbooks](runbooks.md)).

Take backups, and rehearse one restore, before any of these happens: a sign-in allowlist entry for
someone else, a second organization, or memories you could not recreate by hand. Alert on a failed backup
([runbooks](runbooks.md)).

Logs and telemetry hold personal data: client IP, principal and user agent. They are kept 30 days
([observability](observability.md)). The server's purge keeps recall events 30 days. An organization that opts
in keeps recall query text for the same 30 days, readable by its admin keys only.

## From alert to action

1. **Detect.** Evaluate rules for the host, PostgreSQL, the server, recall health and the SLO burn rates every
   minute. Give each rule a `runbook` annotation that names the first command or dashboard.
2. **Notify.** Route `critical` to a channel that wakes someone. Critical means the server or PostgreSQL is
   down, `/readyz` is failing, a fast burn, or a disk over 90% full. Route `warning`, which means act the same
   day, to one that does not. Name a second responder, because one person is a single point of failure.
3. **Act.** Open the rule's runbook line, then the dashboards. Run `oml doctor` under the admin key
   ([runbooks](runbooks.md)). Silence planned work for its window. It still spends budget.
4. **Record.** Write a dated note for an incident that spent budget or lost a write. Give the cause, the effect in
   minutes and the release that fixed it.

To upgrade without spending the budget twice, rehearse first ([Upgrade OML](upgrades.md)).

## Harden the host

- **Network.** Only the reverse proxy listens publicly. The server, its metrics listener, PostgreSQL and every
  exporter bind loopback. Metrics never ride the API port.
- **Process.** Run the server as its own user with `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`,
  `PrivateTmp` and `PrivateDevices`. Keep environment files readable by that user alone, and the keyring directory `0700`.
- **Database.** The serving role owns nothing and has no `BYPASSRLS`. Every tenant table is under forced row
  security. Give the monitoring role `pg_monitor`, which reads statistics views and no tenant table.
- **Not provided by OML:** a web application firewall or edge rate limit, intrusion detection, off-host log
  shipping and an external security review. Put them in front of a public server.

## Read usage without reading memories

Monitoring never reads tenant rows. Read usage from Prometheus families with closed labels, the request and
tool-call lines in the log store, and PostgreSQL statistics views. A dashboard on them shows active keys per
day, requests by route, tool and client, the held-to-decided flow, recalls by pipeline and the zero-hit share.
What needs tenant rows, such as a memory's kind or verdicts per reviewer, reads from [the lake](lake.md).
