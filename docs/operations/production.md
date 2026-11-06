<!--
id: operations/production
tier: engineering
status: proposed · 2026-11-05
answers: what the maintainers' public server at memory.doublegate.io promises (SLOs, error budget), what it has been measured to carry, how its data is protected and what it is not protected against, how an alert becomes an action, how it is upgraded, its security posture, how its use is measured, and what it still lacks before it is called production-ready
-->

# Production: memory.doublegate.io

**Status:** proposed · 2026-11-05 ([ADR-0058](../adr/ADR-0058-the-public-server-runs-to-stated-slos-with-an-error-budget-and-without-backups-until-a-second-person-stores-data.md))

**Release:** deployment policy for one deployment, not a release; the server runs v0.5.7.

[Observability](observability.md) fixes the SLI formulas and leaves every target to deployment
policy. This page is that policy for the maintainers' public server
([ADR-0043](../adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md)): the
targets, the error budget and what spending it stops, the capacity measured so far, the data
protection the server has and the protection it does not, the path from an alert to a person, the
upgrade procedure, the security posture, and the gap list. Every number names its source; a number
from the live server names the query and the date.

## The deployment, as of 2026-11-05

| What | Value | Source |
|---|---|---|
| Server | `oml-server-lead` v0.5.7, one lead, no followers, systemd unit `oml-server` as user `oml`, listening on 127.0.0.1:8090 only | `/readyz` `build.version`; `systemctl cat oml-server` |
| Edge | Caddy, TLS from Let's Encrypt, `memory.doublegate.io` proxied to the lead | `/etc/caddy/oml.caddy` |
| Store | PostgreSQL 18 main cluster on the same host; owner role `oml_owner`, serving role `oml_server` (no superuser, no `BYPASSRLS`) | [ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md); `role_split_booted` log line |
| Host | 8 vCPU, 15 GiB RAM, no swap; root disk 38 GB (77% used), data volume 40 GB (52% used) holding PostgreSQL, Prometheus and Loki | `nproc`, `free -g`, `df -h`, 2026-11-05 20:20 CET |
| Search | `bm25-postings` and the opt-in `bm25-rerank` (MiniLM-L6 cross-encoder, 2 threads, 2 slots, 1 s deadline) enabled; learned sparse off | `oml_recalls_total{pipeline}`; [ADR-0053](../adr/ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md) d2, d5 defaults |
| Data held | 1 organisation, 7 memory versions, 33 journal events, 7 keys, 4 workareas; database 10.96 MB | `pg_stat_user_tables_n_live_tup`, `pg_database_size_bytes{datname="oml_server"}` = 10,958,527, 2026-11-05 |
| Traffic | 688 requests in the 48 h to 2026-11-05 20:20 CET, of which 63 matched a route (the rest are `unmatched` 404s: scanners); 0 answered 5xx | `sum(increase(oml_http_request_duration_seconds_count[2d]))`, same with `route!="unmatched"` and `status=~"5.."` |
| Monitoring | Prometheus (30 days, 2 GB cap), Loki (30 days) fed by Alloy, Grafana at monitoring.doublegate.io, alerts to Telegram | the host's monitoring runbook, `oml-deploy/monitoring/README.md` (outside this repository) |

The server has one real user, the operator. Every target below is set for that load and revisited
after 30 days of data (d2).

## SLOs

Each SLO is a target over a rolling 30-day window, so its **error budget** is `1 − target` of the
window's events. The SLIs are the formulas of [observability](observability.md#slos), made
concrete; thresholds sit on histogram bucket edges, since the request and recall histograms double
from 1 ms ([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md)
d12) and a threshold between two edges would be interpolated, not counted.

| SLO | SLI (good / all) | Target, 30 d | Error budget | Why this target |
|---|---|---|---|---|
| **Availability, `/readyz`** | blackbox probes of `https://memory.doublegate.io/readyz` through Caddy, every 30 s, answering 200 | 99.5% | 0.5%: 3 h 36 min of failing probes in 30 days | One host, one lead, no failover ([deployment](deployment.md)): every upgrade restarts the lead (7 to 9 s for v0.4.1 to v0.5.4, the operator's deployment notes) and a host reboot takes minutes. 99.9% leaves 43 min a month: the v0.5.3 boot failure alone took about 5 min (incident of 2026-11-04), and a lost volume, with no backup to restore, would take longer than that to rebuild. The probe counts a full outage, which request metrics cannot: a lead that is down exports nothing ([observability](observability.md#slos)). |
| **Availability, MCP** | `/mcp` requests answered without a 5xx | 99.5% | 0.5% of `/mcp` requests | Agents reach the server through MCP; a 401 or a tool error is the caller's and counts as good. Same target as `/readyz`: they fail together. |
| **Recall latency, first-stage pipelines** | recalls of every pipeline without a reranker (`bm25-postings` today) answered within 128 ms, `oml_recall_duration_seconds` | 95% | 5% of those recalls | `bm25-postings` measured a 25.5 ms universe p95 at 165,000 memories on this class of host (`bench/scale/REPORT.md`, whole-words run) against [ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) d9's 50 ms bar; 128 ms is the next bucket edge with room for the lead's own work and for the build lanes that share this host (load average up to 29 during the bench). |
| **Recall latency, `bm25-rerank`** | `bm25-rerank` recalls answered within 1.024 s | 95% | 5% | The rerank deadline is 1,000 ms (`search.rerank.timeout_ms`, ADR-0053 d5); a recall past 1.024 s waited past it or its first stage was slow. ADR-0053 d5 measured p95 246 ms at one client and 721 ms at four. |
| **Write acceptance** | HTTP `POST /v1/memories` and supersede, and MCP `oml_write`, answered within 256 ms, from the request log | 95% | 5% | The bench's held insert costs 8.08 ms p95 at 165,000 memories (`bench/scale/REPORT.md`, write cost); the lead adds the scan, signing and the chain lock. 256 ms is about 30 times the store's cost. Read from Loki, because the request histogram has no method label and cannot tell a write from a listing on `/v1/memories` (gap 7). |
| **Review latency** | verdicts given within 122,880 s (34.1 h) of the write, `oml_review_wait_seconds` | 90% | 10% | Review is a person's act ([ADR-0055](../adr/ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)); one reviewing session a day keeps most writes under a day and a half. The histogram counts at the verdict, so an item still waiting is invisible until judged (gap 6). |

Availability and recall latency alert on their burn rate (below). Write and review latency are on
the SLO dashboard without alerts: their traffic is too thin for a rate to mean anything yet.

### Error budget policy

- **Budget left above zero:** releases go to live as they are cut, each rehearsed (below).
- **Budget spent for an SLO:** no release that changes behaviour goes to live until that SLO's
  30-day SLI is back above its target; a fix for the cause, a security fix and a rollback go at
  once. The operator records the cause beside the incident.
- **A planned upgrade spends budget like any outage.** None is excused, as
  [observability](observability.md#slos) requires for the takeover window.
- **Thin traffic.** With fewer than 20 events in a window, one slow event is 5% of it; the
  burn-rate alerts below require a minimum count and the budget is judged over the 30 days, not
  over a quiet night.

### Burn-rate alerts

Multiwindow, multi-burn-rate alerts after the Google SRE workbook (chapter "Alerting on SLOs"),
for the two availability SLOs and the two recall-latency SLOs, to Telegram:

| Tier | Long window | Short window | Burn rate | Budget spent at that rate | Severity |
|---|---|---|---|---|---|
| fast | 1 h | 5 m | above 14.4 | 2% of 30 days' budget in 1 h | critical |
| slow | 6 h | 30 m | above 6 | 5% in 6 h | warning |

Both windows must exceed the rate, so an alert stops soon after the burning stops. The MCP and
recall alerts also need at least 10 events in the 1 h window and 20 in the 6 h one. The host's
generator (`oml-deploy/monitoring/build_alerts.py`) reads the SLI expressions from the same
definition as the SLO dashboard, so the two cannot disagree.

## Capacity measured so far

Measured on hosts of this class (8 vCPU, 15 GiB, shared with build lanes), never on an idle host;
every latency is an upper bound for one client unless a column says otherwise.

| Quantity | Measured | Source |
|---|---|---|
| `bm25-postings` recall, universe scope, 99,721 readable rows (165,000 memories) | p95 25.5 ms (leaf 5.9, project 15.9); `bm25`'s scan 485 ms in the same run | `bench/scale/REPORT.md`, whole-words run, `results/postings-165k-whole-words.json` |
| Same, at 100,000 and 10,000 memories | universe p95 26.6 and 14.0 ms | `bench/scale/REPORT.md`, headline table |
| A key's own held rows, universe scope, 165,000 memories | 1,000 held: 59.2 ms p95; 10,000 held: 266 ms | `bench/scale/REPORT.md`, own held rows |
| Postings storage | about 36 postings and 7 times the memories heap per memory: 1.1 GiB per 165,000 memories; 6.7 GiB per 1,000,000 by linear extrapolation, not measured | `bench/scale/REPORT.md`, space table; [FR-100](../product/roadmap.md) |
| Write cost in the store, 165,000 memories | insert held 2.40 / 8.08 ms, promote 4.54 / 8.94 ms, supersede 6.35 / 12.6 ms (p50 / p95) | `bench/scale/REPORT.md`, write cost |
| Postings fill (migration 0021) | 70.4 s at 165,000 memories, about 0.4 s per 1,000 | `bench/scale/REPORT.md` |
| Learned sparse encoder | 12.7 claims a second at 2 threads, 17.4 at 8, one at a time; backfill at 100,000 rows not measured | [FR-101](../product/roadmap.md) row, ADR-0052 d14 |
| Learned sparse recall, 100,000 memories | universe p95 164 ms, over ADR-0052 d9's 50 ms: off on live | `bench/scale/REPORT.md`, learned sparse lane; [FR-118](../product/roadmap.md) |
| Reranker, L6, depth 10, defaults (2 threads, 2 slots, 1 s) | p50 / p95: 1 client 140 / 246 ms; 4 clients 348 / 721 ms, 1 timeout of 121; 8 clients 805 / 1,018 ms, 32 timeouts and 2 busy of 121 | ADR-0053 d5 |
| Reranker on live | one `bm25-rerank` recall of the two since the 18:48 restart took 0.49 s; both in the (0.256, 0.512] s bucket | `oml_recall_duration_seconds{pipeline="bm25-rerank"}`, 2026-11-05 |
| Review queue under load | six reviewer keys approved about 77 rows a second with 5,000 held, 2 to 14 a second with 45,000 held, before ADR-0055's index | ADR-0055 context, `eval/bench/REPORT.md` |

What this means for the live server: at 7 memories it is five orders of magnitude below the
measured range. The binding limits as it grows are, in order: concurrent reranked recalls (more
than four at once start to time out and fall back to `bm25-postings`' order, by design), the
data volume (postings at 1.1 GiB per 165,000 memories against 19 GB free), and a key holding
thousands of unreviewed rows. Not measured: 1,000,000 memories, cold caches, an idle host, and the
per-tenant write ceiling under the chain lock ([FR-7](../product/roadmap.md),
[FR-18](../product/roadmap.md), [FR-25](../product/roadmap.md)).

## Data protection

**Backups: none.** The server takes no scheduled backup, keeps no WAL archive and holds no copy
off the host. This is the operator's decision for now (ADR-0058 d5), stated plainly with its risk:

- **What is lost, and when.** A failure of the data volume, an accidental `DROP`, a corrupting
  migration that commits, or the loss of the host loses every memory, every journal event and every
  key since the last manual dump, with no point-in-time recovery. The signing keyring in
  `/var/lib/oml` sits on the root disk, a different disk from the database; losing it leaves the
  chain readable and verifiable by its public key but no longer extendable by this deployment
  ([signing-key compromise](runbooks.md#signing-key-compromise) is the nearest runbook).
- **What exists.** Five `pg_dump` files taken by the operator: before the volume move of
  2026-11-04 and before the upgrades to v0.5.0, v0.5.5, v0.5.6 and v0.5.7, in `/srv/data/backups`
  on the same data volume: it protects an upgrade from itself and nothing else,
  because it shares the volume it would restore.
- **Why accepted.** The data is the operator's own test data (one organisation, 7 memory
  versions); recreating it costs less than running backups and rehearsing restores today.
- **The trigger to add them.** Backups and one rehearsed [restore](restore.md) are added before
  the first of: a key or a sign-in allowlist entry for anyone but the operator; a second
  organisation; or memories the operator would not recreate by hand. Setting `signup.mode = open`
  ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md))
  creates such entries, so it waits for this too. The scope is then a daily
  `pg_dump` (as the PostgreSQL superuser, since forced row security stops the owner role's dump)
  copied off the host, the keyring copied with it under its own protection, a retention window
  declared in [deployment](deployment.md)'s backup table, and the [backup failed](runbooks.md#backup-failed)
  alert. Until then [restore](restore.md) and [disaster recovery](disaster-recovery.md) describe
  procedures this deployment cannot run.

**Logs and telemetry** hold personal data (client IP, principal, user agent) and are kept 30 days
in journald, Loki and Caddy's access log ([observability](observability.md#log-retention)). Recall
events are kept 30 days by the lead's purge; the operator's organisation keeps recall query text
for the same 30 days, readable by its admin keys only (ADR-0058 d8,
[ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) d3).

## Incident response

1. **Detect.** 33 Grafana-managed rules (host, PostgreSQL, the lead, recall health and the SLO
   burn rates) evaluate every minute. Each carries a `runbook` annotation naming the first command
   or dashboard to open.
2. **Notify.** Every rule routes to the `oml-telegram` contact point: grouped by alert name,
   30 s wait, 5 min interval, repeated every 4 h while firing. `critical` means act now (lead or
   PostgreSQL down, `/readyz` failing, fast SLO burn, disk over 90%); `warning` means act the same
   day. One person, the operator, receives them; there is no second responder (gap 9).
3. **Act.** The rule's runbook line, then the dashboards: OML SLOs, OML traffic, OML quality, OML
   users, OML usage, PostgreSQL and host at monitoring.doublegate.io; `oml doctor` under the admin
   key; [runbooks](runbooks.md) for conditions this page does not cover. Planned work is silenced
   in Grafana (Alerting, Silences) for its window, and still spends error budget.
4. **Record.** An incident that spent budget or lost a write gets a dated note with cause, effect
   in minutes and the fix's release; the v0.5.3 boot failure of 2026-11-04 (migration 0017's
   check refused the live schema, about 5 minutes on the previous binaries, fixed in v0.5.4 with an
   upgrade-path test) is the pattern.

## Upgrade procedure

What [upgrades](upgrades.md) declares, as this deployment runs it since the v0.5.3 incident:

1. **Verify the release.** Download the release assets and verify every cosign signature and
   `SHA256SUMS` before installing anything.
2. **Rehearse on a copy of live.** `pg_dump` the live database, restore it into a throwaway
   PostgreSQL 18 cluster under `/srv/build`, with the `oml_owner` and `oml_server` roles and a
   copy of the keyring, boot the new lead against it on another loopback port, and check the
   migrations, `oml doctor` and a recall over the copied data. Delete the copy afterwards.
3. **Dump first.** `pg_dump` of live into `/srv/data/backups`, named for the version it precedes.
4. **Swap and restart.** Keep the running binaries as `<name>.v<old>`, install the new ones,
   restart the unit; migrations run at boot as the owner role. The restart took 7 to 9 s from v0.4.1
   to v0.5.4 (the operator's deployment notes).
5. **Check.** `/readyz` names the new version, the `role_split_booted` line appears, `oml doctor`
   answers `ok` for the agent and the admin key, the SLO dashboard's probe line stays green.
6. **Roll back** by the previous binaries only while no migration has run; after a migration,
   rollback is the dump of step 3 ([upgrades](upgrades.md#rollback-binaries-only-never-schema)).

A release that does not pass step 2 does not reach step 3. A spent error budget stops step 4 for
any release that is not a fix (the policy above).

## Security posture

- **Network.** Only Caddy listens on public ports. The lead (8090), its metrics (9464),
  PostgreSQL, Prometheus, Loki, Grafana and every exporter bind 127.0.0.1; metrics never ride the
  API port ([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d11).
- **Process.** The unit runs as `oml` with `NoNewPrivileges`, `ProtectSystem=strict`,
  `ProtectHome`, `PrivateTmp`, `PrivateDevices` and the kernel protections; `/etc/oml/oml.env` and
  the web sign-in secrets are `root:oml 0640` and `root 0600`; the keyring directory is `0700`.
- **Database.** The serving role owns nothing and has no `BYPASSRLS`; every tenant table is under
  forced row security ([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md)).
  The monitoring role `prometheus` is `pg_monitor`, read-only by default, and reads statistics
  views only; it is never granted a tenant table (ADR-0058 d7).
- **Identity.** Agents hold keys, people sign in with Google or GitHub against an allowlist of one
  address ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md));
  claude.ai connects by a one-time connect code
  ([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md)). Grafana allows login only,
  no sign-up and no anonymous access.
- **Supply chain.** Releases are signed with cosign in CI and verified on the host before install.
- **Not in place:** a web application firewall or rate limit at the edge, intrusion detection,
  off-host log shipping, and an external security review (gap 10).

## Usage analytics

How the server is used is read from what exists now, under the rule that monitoring never reads
tenant rows (ADR-0058 d7): Prometheus families with closed labels; the request and MCP call lines
in Loki, where ids, principals, clients and workarea ids are read per query and never stored as
series; and PostgreSQL statistics views (`pg_stat_user_tables`) through the `prometheus` role, which
give deployment-wide row counts per table: memory versions written, rows entering and leaving the
review queue, journal events. The **OML usage** dashboard shows active keys, users and
organisations per day, requests by route, tool and client, writes by kind, the held-to-decided
flow, recalls by pipeline, the zero-hit share, what readers did next, and workarea activity. What
needs the journal or tenant rows (a memory's kind, held and promoted counts per workarea, verdicts
per reviewer, query topics) is marked "from lake (pending)" and waits for the Delta lake writer
([ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md)).

## What production-ready still lacks

| # | Gap | Effect today | Closes when |
|---|---|---|---|
| 1 | No backups, no WAL archive, no off-host copy (above) | Any volume, host or operator error loses everything since the last pre-upgrade dump | the trigger in Data protection fires; a restore is rehearsed |
| 2 | One host, one lead, no follower | Every upgrade and reboot is an outage; the availability target is set to absorb them | a follower or a second host ([deployment](deployment.md)) |
| 3 | The lead and its database share a host with build lanes | Benchmarks measured load averages up to 29; recall latency on live moves with them | build lanes move off the serving host |
| 4 | Lake writer, reconciler and indexer are not built; the designed lag metrics (`oml_outbox_frontier_lag_seconds`, `oml_dead_letter_count`, `oml_reconcile_*`) do not exist | No signal for the derived layers; usage panels that need the journal wait | ADR-0003's writer ships |
| 5 | `oml_chain_append_duration_seconds` and `oml_rls_denied_total` are designed, not emitted | The per-tenant write ceiling and isolation probes are invisible to metrics | [observability](observability.md#metrics-per-service) builds them |
| 6 | `oml_review_queue_depth` and `oml_review_queue_oldest_age_seconds` are designed, not emitted | A held item's wait is seen only at its verdict; the queue depth comes from a pg_stat estimate | the review-queue gauges are built |
| 7 | The request histogram has no method label | The write SLI is read from Loki, not Prometheus | a `method` label (closed set) or a write histogram |
| 8 | Labelled families appear at their first event after a restart, so `increase()` misses that event | On a quiet server the first recall, write or verdict after each restart is missing from rates | families pre-registered at zero for their closed label sets |
| 9 | One responder, one notification channel | An alert at night waits for the operator | a second person on call, or a second channel |
| 10 | No edge rate limit, WAF, intrusion detection or external review | Abuse is met by the lead's own refusals only | a review before the first organisation other than the operator's |
| 11 | Unmeasured: 1,000,000 memories, cold caches, an idle host, the per-tenant write ceiling, the reranker on an idle host | Capacity above 165,000 memories is extrapolation | [FR-7](../product/roadmap.md), [FR-18](../product/roadmap.md), [FR-25](../product/roadmap.md), [FR-88](../product/roadmap.md), [FR-100](../product/roadmap.md) |
| 12 | The host's unit file, Caddy site and monitoring generators live on the host, outside this repository | A rebuilt host depends on the operator's notes | they are committed under `deploy/` |
| 13 | SLO targets rest on two days of data | Targets may be loose or tight | the 30-day review of ADR-0058 d2 |

## What this page does not decide

The SLI formulas ([observability](observability.md)), the release contents
([releases](../product/releases.md)), and every other deployment's targets: another operator sets
their own.
