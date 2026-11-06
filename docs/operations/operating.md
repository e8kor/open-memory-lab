---
status: Stable
verified: 2026-11-06
answers: "which SLOs an operator can run an OML server to, how the error budget governs releases, what capacity has been measured, how data is protected, how an alert becomes an action, how to upgrade, and what the security posture is"
---

# Operating OML

[Observability](observability.md) fixes the SLI formulas and leaves every target to the
operator. This page is a worked policy for one lead on one host
([ADR-0058](../adr/ADR-0058-a-public-server-runs-to-stated-slos-with-an-error-budget.md)): the
targets, what a spent error budget stops, the capacity measured so far, data protection, the path
from an alert to a person, the upgrade procedure and the security posture. Another deployment
sets its own numbers; each number below names its source.

## SLOs

Each SLO is a target over a rolling 30-day window, so its error budget is `1 − target` of the
window's events. Thresholds sit on histogram bucket edges, because the request and recall
histograms double from 1 ms
([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md)
d12) and a threshold between two edges is interpolated, not counted.

| SLO | SLI (good / all) | Target, 30 d | Error budget | Why this target |
|---|---|---|---|---|
| Availability, `/readyz` | blackbox probes through the edge, every 30 s, answering 200 | 99.5% | 3 h 36 min of failing probes | One lead, no failover ([deployment](deployment.md)): every upgrade restarts it and a host reboot takes minutes. 99.9% leaves 43 min a month. The probe counts a full outage, which request metrics cannot: a lead that is down exports nothing. |
| Availability, MCP | `/mcp` requests answered without a 5xx | 99.5% | 0.5% of requests | A 401 or a tool error is the caller's and counts as good. Same target as `/readyz`: they fail together. |
| Recall latency, first-stage pipelines | recalls without a reranker within 128 ms | 95% | 5% | `bm25-postings` measured a 25.5 ms universe p95 at 165,000 memories (`bench/scale/REPORT.md`) against [ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) d9's 50 ms bar; 128 ms is the next bucket edge, with room for the lead's own work. |
| Recall latency, `bm25-rerank` | recalls within 1.024 s | 95% | 5% | The rerank deadline is 1,000 ms (`search.rerank.timeout_ms`); [ADR-0053](../adr/ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md) d5 measured p95 246 ms at one client and 721 ms at four. |
| Write acceptance | `POST /v1/memories`, supersede and `oml_write` within 256 ms, from the request log | 95% | 5% | The held insert costs 8.08 ms p95 at 165,000 memories (`bench/scale/REPORT.md`, write cost); 256 ms is about 30 times that. Read from the log store, because the request histogram has no method label. |
| Review latency | verdicts within 34.1 h of the write, `oml_review_wait_seconds` | 90% | 10% | Review is a person's act ([ADR-0055](../adr/ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)). The histogram counts at the verdict, so an item still waiting is invisible until judged. |

### Error budget policy

- **Budget left:** releases go live as they are cut, each rehearsed (see Upgrade).
- **Budget spent for an SLO:** no behaviour-changing release goes live until that SLO's 30-day SLI
  is back above target. A fix for the cause, a security fix and a rollback go at once. Record the
  cause beside the incident.
- **A planned upgrade spends budget like any outage.** None is excused.
- **Thin traffic.** With fewer than 20 events in a window one slow event is 5% of it, so the
  burn-rate alerts need a minimum count and the budget is judged over 30 days.

### Burn-rate alerts

Multiwindow, multi-burn-rate alerts after the Google SRE workbook, chapter "Alerting on SLOs", for
the availability and recall-latency SLOs:

| Tier | Long window | Short window | Burn rate | Budget spent at that rate | Severity |
|---|---|---|---|---|---|
| fast | 1 h | 5 min | above 14.4 | 2% of 30 days' budget in 1 h | critical |
| slow | 6 h | 30 min | above 6 | 5% in 6 h | warning |

Both windows must exceed the rate, so an alert stops soon after the burning stops. The MCP and
recall alerts also need at least 10 events in the 1 h window and 20 in the 6 h one. Generate the
rules and the SLO dashboard from one SLI definition, so the two cannot disagree.

## Capacity measured so far

Measured on an 8-vCPU, 15 GiB host shared with other work, never on an idle host; every latency is
for one client unless a row says otherwise.

| Quantity | Measured | Source |
|---|---|---|
| `bm25-postings` recall, universe scope, 99,721 readable rows (165,000 memories) | p95 25.5 ms; `bm25`'s scan took 485 ms in the same run | `bench/scale/REPORT.md`, whole-words run |
| Same, at 100,000 and 10,000 memories | universe p95 26.6 and 14.0 ms | `bench/scale/REPORT.md`, headline table |
| A key's own held rows, 165,000 memories | 1,000 held: 59.2 ms p95; 10,000 held: 266 ms | `bench/scale/REPORT.md`, own held rows |
| Postings storage | 1.1 GiB per 165,000 memories; 6.7 GiB per 1,000,000 by linear extrapolation, not measured | `bench/scale/REPORT.md`, space table |
| Write cost in the store, 165,000 memories | insert held 2.40 / 8.08 ms, promote 4.54 / 8.94 ms, supersede 6.35 / 12.6 ms (p50 / p95) | `bench/scale/REPORT.md`, write cost |
| Postings fill (migration 0021) | 70.4 s at 165,000 memories, about 0.4 s per 1,000 | `bench/scale/REPORT.md` |
| Learned sparse recall, 100,000 memories | universe p95 164 ms, over ADR-0052 d9's 50 ms bar, so it is off by default | `bench/scale/REPORT.md`, learned sparse lane |
| Reranker, L6, depth 10, defaults (2 threads, 2 slots, 1 s) | p50 / p95: 1 client 140 / 246 ms; 4 clients 348 / 721 ms; 8 clients 805 / 1,018 ms with 32 timeouts and 2 busy of 121 | ADR-0053 d5 |
| Review queue under load | six reviewer keys approved about 77 rows a second with 5,000 held, 2 to 14 a second with 45,000 held, before ADR-0055's index | ADR-0055, `eval/bench/REPORT.md` |

The binding limits as a server grows are, in order: concurrent reranked recalls (more than four at
once start to time out and fall back to the first-stage order), the data volume, and a key holding
thousands of unreviewed rows. Not measured: 1,000,000 memories, cold caches, an idle host and the
per-tenant write ceiling under the chain lock; the open measurements are
[#99](https://github.com/e8kor/open-memory-lab/issues/99),
[#100](https://github.com/e8kor/open-memory-lab/issues/100),
[#102](https://github.com/e8kor/open-memory-lab/issues/102) and
[#103](https://github.com/e8kor/open-memory-lab/issues/103).

## Data protection

Back up the database and the signing keyring together ([restore](restore.md)). A server with no
scheduled backup, no WAL archive and no off-host copy loses every memory, journal event and key
since its last dump when the data volume or the host fails, with no point-in-time recovery. Losing
the keyring alone leaves the chain readable and verifiable by its public key but no longer
extendable ([signing-key compromise](runbooks.md#signing-key-compromise)).

Take backups, and rehearse one restore, before the first of: a key or sign-in allowlist entry for
anyone but the operator, a second organisation, or memories the operator would not recreate by
hand (ADR-0058 d5). Run a daily `pg_dump` as the PostgreSQL superuser, because forced row security
stops the owner role's dump, copy it off the host with the keyring under its own protection,
declare a retention window in [deployment](deployment.md)'s backup table and alert on
[backup failed](runbooks.md#backup-failed). A pre-upgrade dump on the data volume protects an
upgrade and nothing else, because it shares the volume it would restore.

Logs and telemetry hold personal data (client IP, principal, user agent) and are kept 30 days
([observability](observability.md#log-retention)). Recall events are kept 30 days by the lead's
purge; an organisation that opts in keeps recall query text for the same 30 days, readable by its
admin keys only
([ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) d3).

## From alert to action

1. **Detect.** Rules for the host, PostgreSQL, the lead, recall health and the SLO burn rates
   evaluate every minute. Give each a `runbook` annotation naming the first command or dashboard.
2. **Notify.** Route `critical` (lead or PostgreSQL down, `/readyz` failing, fast burn, disk over
   90%) to a channel that wakes someone, and `warning` (act the same day) to one that does not.
   Name a second responder: one person is a single point of failure.
3. **Act.** Open the rule's runbook line, then the dashboards; run `oml doctor` under the admin
   key; see [runbooks](runbooks.md). Silence planned work for its window; it still spends budget.
4. **Record.** An incident that spent budget or lost a write gets a dated note with cause, effect
   in minutes and the fix's release.

## Upgrade

What [upgrades](upgrades.md) declares, as a procedure:

1. **Verify the release.** Check every cosign signature and `SHA256SUMS` before installing.
2. **Rehearse on a copy.** `pg_dump` the database, restore into a throwaway PostgreSQL 18 cluster
   with the owner and serving roles and a copy of the keyring, boot the new lead against it on
   another loopback port, and check the migrations, `oml doctor` and a recall over the copied
   data. Delete the copy afterwards.
3. **Dump first**, named for the version it precedes.
4. **Swap and restart.** Keep the running binaries as `<name>.v<old>`, install the new ones,
   restart; migrations run at boot as the owner role.
5. **Check.** `/readyz` names the new version, the `role_split_booted` log line appears, `oml
   doctor` answers `ok` for the agent and admin keys, and the probe stays green.
6. **Roll back** by the previous binaries only while no migration has run; after a migration,
   rollback is the dump of step 3 ([upgrades](upgrades.md#rollback-binaries-only-never-schema)).

A release that fails step 2 does not reach step 3. A spent error budget stops step 4 for any
release that is not a fix.

## Security posture

- **Network.** Only the reverse proxy listens publicly. The lead, its metrics port, PostgreSQL and
  every exporter bind loopback; metrics never ride the API port (ADR-0042 d11).
- **Process.** Run the lead as its own user with `NoNewPrivileges`, `ProtectSystem=strict`,
  `ProtectHome`, `PrivateTmp` and `PrivateDevices`; keep environment files group-readable by that
  user only and the keyring directory `0700`.
- **Database.** The serving role owns nothing and has no `BYPASSRLS`; every tenant table is under
  forced row security ([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md)).
  The monitoring role is `pg_monitor`, reads statistics views only and is never granted a tenant
  table (ADR-0058 d7).
- **Identity.** Agents hold keys; people sign in with Google or GitHub against an allowlist
  ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md));
  claude.ai connects by a one-time connect code
  ([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md)).
- **Supply chain.** Releases are signed with cosign in CI; verify before install.
- **Not provided by OML:** a web application firewall or rate limit at the edge, intrusion
  detection, off-host log shipping and an external security review. Put them in front of a public
  server.

## Usage analytics

Read usage from what exists, under the rule that monitoring never reads tenant rows (ADR-0058
d7): Prometheus families with closed labels; the request and MCP call lines in the log store; and
PostgreSQL statistics views, which give row counts per table. A dashboard built on them shows
active keys, users and organisations per day, requests by route, tool and client, the
held-to-decided flow, recalls by pipeline and the zero-hit share. What needs the journal or tenant
rows (a memory's kind, verdicts per reviewer, query topics) reads from [the lake](lake.md).

## Limits today

| Limit | Effect | Tracked |
|---|---|---|
| One lead, no follower | Every upgrade and reboot is an outage; the availability target absorbs them | [#117](https://github.com/e8kor/open-memory-lab/issues/117) |
| `oml_chain_append_duration_seconds` and `oml_rls_denied_total` are designed, not emitted | The per-tenant write ceiling and isolation probes are invisible to metrics | [observability](observability.md#metrics-per-service) |
| `oml_review_queue_depth` and `oml_review_queue_oldest_age_seconds` are designed, not emitted | A held item's wait is seen only at its verdict | [observability](observability.md#metrics-per-service) |
| The request histogram has no method label | The write SLI is read from the log store | [observability](observability.md#slos) |
| Labelled families appear at their first event after a restart | `increase()` misses the first event on a quiet server | [observability](observability.md#slos) |
