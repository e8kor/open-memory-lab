<!--
id: adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound
tier: engineering
status: accepted in part (measurement: d1–d6, d11–d13); proposed (ranking: d7–d10)
answers: which recall and remember health figures reach Prometheus and Grafana, with which labels and within which series budget; what a recall event records, where it is stored, under which fence, for how long, who reads it, and how its purge squares with no silent deletion; how erasure reaches it; what the event write may cost a recall; which reader actions label a recall's hits as used or unused, how a label finds its event, and what bounds noise and abuse; the explicit feedback member on the existing read, and its use case; how usage ranks a hit, whose usage counts, how its influence is bounded, decayed and kept fair to new memories, how it resists feedback loops and gaming, and why it never reads a verdict; whether a new version inherits its predecessor's usage; how a hit explains its usage; the pipeline that offers it and the evidence that may make it the default; the real-usage eval set and its privacy; what remember-side quality is tracked and for whom; the rejected roads; the cost at 1,000,000 memories and 100 recalls a second; the open questions; the test plan and the doctor lines
-->

# ADR-0054 — Recall is measured by what readers do next, and only other readers' use moves a rank, within a bound

**Status:** accepted in part · 2026-11-05. The measurement half, d1 to d6 and d11 to d13 as amended
below, is accepted; the ranking half, d7 to d10, stays proposed until open question 6 is decided.

**Release:** the measurement half, introduced in v0.5.7; the ranking half, not assigned
([releases](../product/releases.md#decisions))

**Amends:** ADR-0049 (proposed): d3 (fusion gains a convex combination beside reciprocal rank),
d4 (the pipeline table gains `bm25-postings-usage`), d6 (each hit's `matched` gains `usage`, and
`matched.via` gains `usage`), d9 (the gate for a pipeline whose leg the eval sets cannot exercise).
ADR-0052 (proposed) d9 (its pipeline table gains the same row). ADR-0042 d12 (new metric
families, under its label rule). ADR-0005 d4 (the erasure inventory gains the telemetry tables).
ADR-0034 d2, d3 and d11 (new tenant-partitioned tables, the serving role's grants on them, and an
owner step that rotates their day partitions). ADR-0039 d4 (`oml_read` and the HTTP recall gain
an optional `feedback` member, and a recall's answer gains `recall_id`; no new tool, by d8).

## The split, and what acceptance amended

*Recorded 2026-11-05 by the operator's decision for v0.5.7.* The record decides two things that
can be judged apart: what is measured about recall and remember (d1 to d6, d11, d12, and d13's
measurement laws and doctor lines), and whether usage moves a rank (d7 to d10, and d13's ranking
laws). The second waits on open question 6, a one-principal tenant, which is not decided. The
first is accepted, built in v0.5.7, with these amendments; where a decision below says otherwise,
this section governs.

| Decision | Status |
|---|---|
| d1 health metrics, d2 `recall_events` and `recall_outcomes`, d3 query text by opt-in, d4 erasure, d5 inferred labels, d6 the `feedback` member, d11 the real-usage replay, d12 remember-side quality, d13's measurement laws and doctor lines | accepted, built v0.5.7 but for the items marked *not built* below |
| d7 the usage leg, d8 credits and caps, d9 `bm25-postings-usage` and convex fusion, d10 the evidence gate; `usage_credits`, `usage_state`, `oml_usage_credits_total`, `matched.usage`, `usage_applied`, `interleaved`, `telemetry.experiments`; d13's ranking laws | proposed, pending open question 6 |

**Open questions settled on acceptance.**

1. *A frozen eval set* (question 1): an admin may confirm an eval set that is kept beyond 30 days,
   opt-in per tenant and journalled. *Not built in v0.5.7*: the confirmed set's table, its
   retention setting and its erase path are a later step (FR-108); until then d11's set is
   rolling.
2. *Partition count* (question 2): **by tenant alone**, as `memories` is; no partition by time.
   Settled first as weekly partitions and changed on review the same day, for this reason: a
   week can be dropped only once its last instant is 30 days past, and by then the minute purge
   has deleted every row in it, so the drop removes empty tables and saves no deletion, while it
   costs owner DDL on a schedule (ADR-0034 d4 allows the owner at boot and for a tenant's
   creation, not on a timer), a `DETACH` that locks the tenant's partition, and a new failure in
   which every event is dropped once the rotation falls four weeks behind. So each tenant's two
   partitions are created once, with the tenant, on the owner path ADR-0034 d11 already holds;
   the serving role deletes rows older than 30 days in bounded batches, each its own
   transaction, by a `(tenant_id, created_at)` index; and every read filters the same bound.
   The cost: the retention is paid in row deletions and vacuum, which FR-104 measures.
3. *An offline annotator* (question 3): deferred. No model grades a tenant's queries in v0.5.7.
4. *Usage with the reranker* (question 4): measured later, with the ranking half.
5. *The starting constants* (question 5): placeholders, set by d10's replay. The measurement
   half's own constants (d5's windows, the caps below, the 30 days) are starting values too, and
   FR-105 measures the windows.

**Amended on acceptance.**

- **d1.** Built families: `oml_recall_answers_total{pipeline, result}`,
  `oml_recall_duration_seconds{pipeline}` (ADR-0042 d12's 17 bounds),
  `oml_recall_outcomes_total{label}` (the six labels of d5 and the two feedback labels),
  `oml_recall_events_total{outcome}` (`written`, `dropped_queue_full`, `dropped_store_error`,
  `dropped_no_partition`), `oml_recall_labels_dropped_total{reason}` (`rate_limited`,
  `queue_full`, `store_error`), `oml_telemetry_queue_depth`, `oml_labeller_lag_seconds`,
  `oml_telemetry_purge_overdue`,
  `oml_memories_unrecalled{promoted_for}`, `oml_remember_duplicates_total{relation}` (`same_text`,
  `none`), `oml_review_wait_seconds{verdict}` and `oml_web_exchanges_failed_total{provider,
  reason}`; and `oml_reranks_total` as before. The remember side counts what main can see: an
  exact duplicate of a served memory's text in the same workarea, and, by verdict, the time from
  submit to verdict, whose count by `verdict` is the rejection rate; ADR-0050's
  `oml_remember_similar_total` and `oml_remember_warnings_total` arrive with its similar check.
  The rejection rate is global: a per-tenant label would put `tenant_id` on a series, which
  ADR-0042 d12 forbids until FR-7, so the per-key and per-tenant figures are PostgreSQL's.
  `oml_keys_rejection_share`, `GET /v2/usage/report` and `oml usage report` are *not built*;
  the admin reads events with `GET /v2/usage/events`. All the families together add at most 300
  series to a lead, inside the 400 budget. Counts move off `/readyz` with them: the skipped
  reranks and the failed sign-in exchanges are metrics, never in a probe that answers anyone.
  The dashboard is committed as `docs/operations/dashboards/oml-quality.json`, beside the
  observability page; the operator imports it, and the chart's provisioning is *not built*.
- **d2.** The tables are `recall_events` and `recall_outcomes`, migration 0023, list-partitioned by
  tenant alone (question 2); every partition has `ENABLE` and `FORCE ROW LEVEL SECURITY` and the
  `tenant_isolation` policy, and is created with its tenant by the owner path of ADR-0034 d11,
  or, for a tenant a lead before 0023 created, at the lead's next start; no owner connection
  opens on a schedule, so ADR-0034 d4 holds as written. `usage_applied` and `interleaved` belong
  to the ranking half and are not created. `pipeline` and `access` are text checked against
  their closed spellings, not codes, so a pipeline another branch adds needs no code table. A
  recall-level label (`requery`, `no_use`) has no `artifact_id`. The serving role holds
  `SELECT`, `INSERT`, `UPDATE` (for d3's nulling and d4's erase) and `DELETE` (for the purge) on
  both tables and owns neither. Every minute the lead's pass first records whether a tenant
  holds a row past the 30 days and the pass's minute (`oml_telemetry_purge_overdue`, the purge's
  real lag), then deletes such rows in batches of 5,000, each committed. A tenant admin reading
  the events is journalled as `telemetry.events_read`, with the count, how many carried text
  and the page's bounds, never a query, as an admin read of a memory is (ADR-0045 d21).
- **d3.** The setting is journalled as `telemetry.settings_set` and folded into
  `tenants.telemetry_query_text_since`, the instant it was turned on, `NULL` when off. The text
  is kept only for an event created at or after that instant, and the rule is the database's:
  an owner-owned `BEFORE INSERT OR UPDATE` trigger on `recall_events`, `SECURITY INVOKER`, nulls
  any other text, whichever statement of the serving role writes it, and the serving role holds
  no `TRIGGER` privilege to disable it. The trigger reads the tenant's row `FOR SHARE`, so an
  opt-out committing while a writer's batch is open waits for the batch and then nulls its text,
  and a batch that starts after the opt-out reads it. Turning it off nulls the text of
  the tenant's events in the same commit as the fold, not in the background. The routes are
  `GET /v2/usage/settings` and `POST /v2/usage/settings`, and the CLI verb is `oml usage
  settings [--query-text on|off]`.
- **d4.** Erase is not built (ADR-0052 d7), so the rule is built as the function its build calls:
  `oml_telemetry_erase(tenant, version_ids)` nulls the query text of every event that served an
  erased version, zeroes its query digest (a short query's digest inverts by word list), removes the version from `hit_ids` and `hit_scores`, and deletes its labels,
  under the tenant's row security. ADR-0005 d4's inventory gains the rows of d4, less the two the
  ranking half adds.
- **d5.** The labeller does not read the journal: after a get by id, a write citing
  `derives_from` or a supersession commits, the lead hands the act to the same bounded queue as
  the events, and the writer attaches it by SQL to the acting key's most recent event within the
  window whose `hit_ids` hold the version. `requery` is attached when the next event is
  written; `no_use` by a pass every minute over events 30 minutes to 24 hours old. A key's
  inferred labels past 1,000 a UTC day are dropped and counted
  (`oml_recall_labels_dropped_total{reason="rate_limited"}`), since no credit exists yet to
  withhold.
- **d6.** The rate limits: at most 20 ids per `feedback`; a key's feedback members checked in a
  UTC day, refused or not, past 1,000 answer `limit.rate` before any lookup, so made-up recall
  ids cost nothing past the cap; and a key's stored explicit labels past 1,000 a UTC day answer
  the same. A member names only a `recall_id` this key made, which another key's or another
  tenant's id is refused as. A `recall_id` the writer has not yet stored is looked up again after
  the queue is flushed once, and that flush never waits for queue room: when the queue is full,
  the member is refused as unknown rather than holding the read. On a deployment of several
  replicas, feedback reaching another replica within the writer's second is refused
  `request.validation`, a limit of the replicas a lead serves today (none).
- **d11.** The replay is `POST /v2/usage/eval` and `oml usage eval --pipelines a,b`. It runs the
  newest 500 labelled recalls at most and stops at 60 seconds, saying so in `capped`; it refuses
  a reranking pipeline, so it never takes a live recall's reranker slot; and it is journalled as
  `telemetry.replayed` with the pipelines and the counts. **How it scopes as each key:** it
  reads each original key's record again, skips a revoked or vanished key's recalls and counts
  them, and runs the recall as that key's caller with the grants and denials the key holds now,
  no own held rows and no text, inside the lead; it never presents or stores the key's secret,
  and the answer holds numbers alone. It writes no event and no label of its own. The
  `served-replay` suite of `eval/bench/` and the first committed run are *not built*.
- **d11.** The replay is `POST /v2/usage/eval` and `oml usage eval --pipelines a,b`, as decided.
  It writes no event and no label of its own. The `served-replay` suite of `eval/bench/` and the
  first committed run are *not built*.
- **d13.** The measurement laws are built as tests against PostgreSQL as the serving role
  (`crates/bins/oml-server-lead/tests/recall_telemetry.rs`, and the queue's law in
  `crates/oml-app/tests/recall_telemetry.rs`):
  `feedback_for_another_keys_recall_or_an_unserved_id_is_refused_and_the_read_not_run`,
  `a_recall_answers_the_same_when_the_event_queue_is_full`,
  `no_agent_writer_reader_or_dreamer_key_reads_a_recall_event_on_any_transport`,
  `no_tenant_reads_another_tenants_recall_events_and_the_probe_finds_a_leak_without_the_fence`,
  `query_text_is_null_unless_the_tenant_opted_in`,
  `no_row_older_than_30_days_is_read_and_the_purge_deletes_it_and_reports_the_lag`,
  `an_opt_out_committing_during_a_batch_keeps_that_batchs_text_out`,
  `feedback_is_capped_per_key_and_another_tenants_key_cannot_label_this_tenants_recall`,
  `an_admin_reading_recall_events_or_replaying_them_is_journalled`,
  `an_erased_version_leaves_no_text_no_hit_and_no_label_in_the_telemetry`,
  `every_quality_metric_label_is_from_its_declared_set`,
  `a_replay_scores_the_labelled_queries_and_its_answer_holds_no_text_and_no_id` and
  `a_get_a_citation_and_a_second_recall_in_other_words_label_the_keys_recall`. The fence law
  carries its control in the test, the same probe run against a broken clause; the opt-in law
  writes text as the serving role, by INSERT and UPDATE, and finds none stored; the MCP probes
  call `oml_read` in every mode and `oml_workarea` with real arguments;
  the admin check, the insert's opt-in rule and the feedback's key check were each shown red
  by hand against a broken clause before the change was kept. The event write's cost on a
  recall was measured as negligible on a debug build (FR-104 stays open for the scale lane). Doctor
  reads the telemetry's health from the metrics listener: events dropped above 1% of recalls,
  the queue's depth above half its bound, rows past the 30 days not yet purged, and the labeller's lag above 10 minutes.

## Context

**Nothing measures recall where it is used.** Every number that chose a pipeline so far came from
a set someone wrote for the purpose: the recall eval's 121 synthetic queries
([ADR-0049](ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md)
d9), SciFact, NFCorpus and LoCoMo (d12), the remember eval's version 1
([ADR-0050](https://github.com/e8kor/open-memory-lab/blob/feat/remember/docs/adr/ADR-0050-remembering-checks-what-is-already-held-replaces-instead-of-repeating-and-says-how-the-memory-will-be-found.md)
d16, on `feat/remember`). The served lead counts recalls by pipeline and access
(`oml_recalls_total`, [ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md)
d6) and reranks by outcome ([ADR-0053](ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md)
d9), and logs one line per request with no query text
([ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md)
d2, d8). Whether an agent used what a recall returned, asked again in other words, or found
nothing it could use is recorded nowhere. ADR-0050 d16 already names real usage as the source of
its version 2, and the designed read log of [retrieval](../architecture/retrieval.md#the-read-log)
(`read_events`, not scheduled) answers who read what for an auditor, not whether it helped.

**The operator's decisions, 2026-11-05.** (a) Track recall and remember quality, and use usage as
a ranking signal now. (b) Recall events store the query digest always and the query text only
where the tenant opts in, kept 30 days.

**What must hold.** R1, agents never promote, sign, demote or delete
([ADR-0001](ADR-0001-the-constitution.md) d1). R2, the reviewer is never the author, excluded by
key and by principal over the lineage closure (d2). R3, the gate changes visibility, not ranking:
no ranking input reads a verdict, a reviewer or `promoted_at` (d3; ADR-0049 d2). Held rows are
the author's alone ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d4). R4, no silent
deletion (d4). ADR-0049 d2: the core computes the candidate scope, and a row outside it never
changes a candidate's score; ADR-0052 d5 keeps every statistic to the readable rows by counting
per workarea and summing over the readable ones. ADR-0042 d12: metric labels never carry a name,
a principal or an address, and no `tenant_id` until FR-7. ADR-0039 d8: a new agent capability is
a parameter, a resource or a hook before it is a tool, and every call traces to a documented use
case. The constitution's limit holds here too: what the service cannot see (two principals one
person controls) it cannot exclude (ADR-0001 d6).

Decided, not built.

## Decision

- **d1 — Health is aggregates in Prometheus, with closed labels and a series budget; anything per
  memory, per query or per key is served from PostgreSQL to the tenant's admin.** Each metric
  below is counted by the lead or by the labeller (d5) and exported on the metrics listener
  (ADR-0042 d11), under ADR-0042 d12's rule: every label from a closed set, no `tenant_id`, no
  key, principal, workarea, memory id, query word or digest.

    | Metric | Type | Labels (values) | Series |
    |---|---|---|---:|
    | `oml_recall_answers_total` | counter | `pipeline` (≤ 8 served names), `result` (`hits`, `empty`, `below_floor`) | ≤ 24 |
    | `oml_recall_duration_seconds` | histogram, ADR-0042 d12's 17 bounds | `pipeline` | ≤ 160 |
    | `oml_recalls_total` | counter (ADR-0052 d6, unchanged) | `pipeline`, `access` (`postings`, `scan`, `none`) | ≤ 24 |
    | `oml_reranks_total` | counter (ADR-0053 d9, unchanged) | its own | unchanged |
    | `oml_recall_outcomes_total` | counter | `label` (`get`, `derived`, `superseded`, `feedback_useful`, `feedback_not_useful`, `requery`, `no_use`) | 7 |
    | `oml_recall_events_total` | counter | `outcome` (`written`, `dropped_queue_full`, `dropped_store_error`, `dropped_no_partition`) | 4 |
    | `oml_usage_credits_total` | counter | `outcome` (`counted`, `author`, `duplicate`, `rate_limited`, `unreadable_source`) | 5 |
    | `oml_labeller_lag_seconds` | gauge | none | 1 |
    | `oml_remember_similar_total` | counter | `relation` (`none`, `same_text`, `shares`), `on_similar` (`hold`, `refuse`) | 6 |
    | `oml_remember_warnings_total` | counter | `finding` (ADR-0050 d6's six `quality.*` classes) | 6 |
    | `oml_review_wait_seconds` | histogram, bounds 1 min to 32 days doubling | `verdict` (`approve`, `reject`) | ≤ 40 |
    | `oml_review_rejections_total` | counter | `duplicate_of` (`set`, `unset`) | 2 |
    | `oml_keys_rejection_share` | histogram over keys with ≥ 10 verdicts in 7 days, bounds 0.05 … 1.0 by 0.05 | none | 22 |
    | `oml_memories_unrecalled` | gauge, refreshed by the labeller every 10 minutes | `promoted_for` (`7d`, `30d`, `90d`, `older`) | 4 |

    The budget is 400 new series per lead, about 300 used above; a law holds every label of
    these families to its declared set (d13). Gauges that need a query are refreshed by the
    labeller and read from memory at scrape, so a scrape runs no SQL (ADR-0042 d12's reason for
    leaving such gauges designed). The Grafana dashboard `oml-quality`, committed under
    `deploy/grafana/` and provisioned by the Helm chart, has these panels:

    | Panel | Reads |
    |---|---|
    | Zero-hit share and below-floor share, per pipeline | `oml_recall_answers_total` |
    | Rerank skipped share, by reason | `oml_reranks_total` |
    | Scan-fallback share of `bm25-postings*` | `oml_recalls_total{access="scan"}` |
    | Follow-up rate: requery, get, derived and feedback per recall | `oml_recall_outcomes_total` ÷ `oml_recall_events_total{outcome="written"}` |
    | Latency p50 / p95 / p99 per pipeline | `oml_recall_duration_seconds` |
    | Remember-side duplicate warnings, held against refused | `oml_remember_similar_total` |
    | Time to review, p50 / p95, by verdict | `oml_review_wait_seconds` |
    | Rejection rate, overall and as a distribution over keys | `oml_review_rejections_total`, `oml_keys_rejection_share` |
    | Memories never recalled since promotion, by age | `oml_memories_unrecalled` |
    | Telemetry health: events dropped, credits refused by reason, labeller lag | the three telemetry families |

    The rejection rate **per key**, the memories never recalled **by id**, and the queries that
    found nothing are answered by `GET /v2/usage/report` and `oml usage report` to a tenant admin
    key alone, from PostgreSQL (d2), never as labels. The cost: an operator who wants one key's
    rejection rate in Grafana cannot have it, by design; the admin page and the CLI have it.

- **d2 — `recall_events` records each recall, partitioned by tenant and by day, under row
  security; it is telemetry, not a claim, and it is kept 30 days.** Migration 0023 creates:

    | Column | Type | Holds |
    |---|---|---|
    | `tenant_id` | `uuid` | the partition key and the `tenant_isolation` policy's column |
    | `recall_id` | `uuid` (v7) | the event's id, returned in the answer as `recall_id` (d6) |
    | `created_at` | `timestamptz` | the recall's start; the day sub-partition key |
    | `key_id` | `uuid` | the calling key |
    | `principal` | `text` | the key's principal, copied, for d7's exclusion and de-duplication |
    | `scope_workarea_id` | `uuid` | the resolved `workarea` node the recall read |
    | `source_workarea_id` | `uuid` | the key's own workarea (ADR-0028 d1), or its user root (ADR-0044), where d7 attributes its usage |
    | `pipeline` | `smallint` | the pipeline that ran, by its closed name's code |
    | `access` | `smallint` | `postings`, `scan` or `none` |
    | `query_digest` | `bytea` | the read log's `query_digest` ([retrieval](../architecture/retrieval.md#the-read-log)), always |
    | `query_text` | `text` | the NFC query, only where the tenant's `telemetry.query_text` is `on` (d3); otherwise `NULL` |
    | `hit_ids` | `uuid[]` | the version ids served after the `k` and budget cuts, in rank order, so a hit's rank is its index; at most 20 stored |
    | `hit_scores` | `real[]` | each served hit's score, parallel to `hit_ids` |
    | `usage_applied` | `boolean` | whether d7's leg took part, so d10 can tell logged rankings apart |
    | `interleaved` | `smallint[]` | for an interleaved recall (d10), the team of each hit; else `NULL` |

    Own held hits are stored as served, since an author reading its own held rows is still a
    read; they never produce a usage credit (d7). `recall_outcomes (tenant_id, recall_id,
    created_at, artifact_id, label, source, at)` holds d5's labels, partitioned the same way.
    `usage_credits (tenant_id, version_id, principal, window_day)`, unique, holds which principal
    has been credited for which version on which UTC day (d8), partitioned by tenant and day.
    `usage_state (tenant_id, version_id, source_workarea_id, positives real, examinations real,
    as_of timestamptz, first_returned_at, last_returned_at)`, partitioned by tenant only, holds
    d7's decayed accumulators, one row per version and source workarea that has any.

    **The fence.** Each table is list-partitioned by `tenant_id` with `ENABLE` and `FORCE ROW LEVEL
    SECURITY` and the `tenant_isolation` policy, created by the owner in ADR-0034 d11's partition
    step; the three event tables are further range-partitioned by UTC day. Statistics on
    `query_text` and `principal` are off (`SET STATISTICS 0`, FR-45's rule), so `pg_stats` holds
    no tenant's query words. The serving role holds `INSERT` and `SELECT` on the event tables,
    `DELETE` on them only for d2's fallback purge, and `SELECT`, `INSERT` and `UPDATE` on
    `usage_state`; it owns none of them (ADR-0034 d2, d3).

    **Who reads.** A tenant admin key, through `GET /v2/usage/report` (aggregates, the per-key and
    per-memory lists of d1), `GET /v2/usage/events` (events with their labels; `query_text` only
    where opted in), `oml usage report` and the admin page
    ([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)).
    No agent, writer, reader or dreamer key reads an event, a label or a count, on any transport:
    the routes refuse them `auth.forbidden`, and `oml_read` has no mode that reaches them. Usage
    reaches an agent only as the `matched.usage` explanation of a hit it may already read (d9).

    **Retention, and R4.** Events, labels and credits are kept 30 days: the owner's
    `oml admin telemetry rotate` (a daily systemd timer or the chart's CronJob, under the owner
    URL, since the lead serves under a role that owns nothing) creates the next seven day
    partitions and drops each day partition whose last instant is more than 30 days past. Every
    read of these tables filters `created_at > now() - 30 days`, and when a partition is overdue
    the lead's background task deletes its rows in batches under the serving role, so the bound
    does not depend on the timer; doctor warns (d13). `usage_state` holds no event, no key, no
    principal and no text; its rows decay (d7) and are deleted when both accumulators fall below
    1/1,000 of one credit. **These tables are telemetry, not claims.** They hold no memory's text,
    no version, no verdict and no journal event; no claim is derived from them; dropping a day
    deletes nothing R4 protects, which is the claims, their versions and the journal
    ([ADR-0001](ADR-0001-the-constitution.md) d4). The purge is not silent: the retention is
    stated on the configuration page, shown by `oml usage report`, and each rotation logs one
    line naming the tenant count, the partitions dropped and the rows they held. The request
    log's 30 days rest on the same reading (ADR-0042 d8). A recall event is not the read log:
    `read_events`, when built, answers who read what for an auditor, under a retention the tenant
    sets and with batch digests anchored in the journal; `recall_events` answers whether a recall
    helped, and is never offered as an audit record (rejected alternatives).

    **The write never sits on a recall's path.** The recall builds the event from what it already
    holds and hands it to a bounded in-process queue (10,000 events) without waiting; a writer
    task inserts the queue in batches (at most 500 rows or every second) in its own transaction
    under the serving role, never inside the recall's `REPEATABLE READ, READ ONLY` snapshot
    (ADR-0052 d12). A full queue, a store error or a missing partition drops the event and counts
    it (`oml_recall_events_total{outcome}`); the recall's answer and latency are unchanged. The
    cost, stated: a crash loses at most the queue, and telemetry may undercount under load. The
    added cost on the recall is one allocation and a channel send, budgeted at 50 µs p99,
    unmeasured (FR-104). No outbox row is written, since a recall commits nothing.

- **d3 — The query text is stored only where the tenant opts in.** `telemetry.query_text` is a
  tenant setting, `off` by default, set by a tenant admin with `POST /v2/usage/settings` or `oml
  usage set --query-text on|off`, journalled as `telemetry.settings_set` with the admin key, and
  in force from the next recall. Turning it off stops new text at once and nulls the text of the
  tenant's live events in the background; the next rotation finishes what the background job has
  not. The digest is always stored. A digest of a short query is not anonymous: a word list
  inverts it, as ADR-0052 d2 says of token hashes, so the digest is treated as personal data
  under the same 30 days. The opt-in's purpose is d11's eval set, and the admin page says so
  where it is switched.

- **d4 — Erasure reaches telemetry: ADR-0005 d4's inventory gains four rows.**

    | Copy class | Holds | Disposition at erase |
    |---|---|---|
    | `recall_events.query_text` | **plaintext a reader wrote**, which may quote erased content | the erase event's job nulls the text of every live event whose `hit_ids` hold an erased version, in batches over the tenant's day partitions, and journals `telemetry.erase_completed`; the 30-day purge bounds it if the job lags. In base backups and WAL until the backup window ends, as tsvector entries are |
    | `recall_events.hit_ids` / `hit_scores`, `recall_outcomes` | version ids and scores, metadata | the same job removes the erased ids and their labels; bounded by the 30-day purge |
    | `usage_state`, `usage_credits` | counts by version id | deleted in the erase event's transaction, by the `(version_id)` index |
    | the real-usage eval runs (d11) | aggregate metrics only, no text and no ids | never needs touching |

    So an erased memory stops influencing any rank at erase, and every telemetry row that names it
    is gone within the job's run and at most 30 days. Erase is not built (ADR-0052 d7); this is
    the rule its build follows.

- **d5 — Outcome labels are inferred by an asynchronous labeller and attach to one event and one
  hit.** The labeller is a task of the lead that reads the tenant's own events and the journal's
  committed writes under the serving role, after commit, never on a request's path. A label is
  `(recall_id, artifact_id, label, source, at)`.

    | Label | Inferred when | Strength |
    |---|---|---|
    | `get` | the same key fetches a hit by id (`oml_read` get, `GET /v1/memories/{id}`) within 10 minutes of the recall that returned it | positive |
    | `derived` | the same key writes a claim whose `derives_from` cites a hit within 24 hours of the recall that returned it | positive, the strongest inferred |
    | `superseded` | the same key supersedes a hit within 24 hours | positive for d11's eval only: the superseded version leaves the served set, so it carries no rank |
    | `feedback_useful` / `feedback_not_useful` | d6's explicit member names the hit | explicit |
    | `requery` | the same key recalls again in the same scope within 2 minutes with a different digest, and no positive was inferred on the first recall's hits | negative on the first recall |
    | `no_use` | 30 minutes pass with no positive on any hit of the recall | weak negative: it enters ranking only as an examination without a positive (d7) |

    **Attachment.** An explicit label names its `recall_id`, so it attaches exactly. An inferred
    label attaches to the key's most recent event, inside the window, whose `hit_ids` hold the
    artifact's version, read by the `(tenant_id, key_id, created_at)` index of the day partitions
    the window spans. A get or a derivation that no event explains attaches to nothing and is
    counted as `unattached` in the labeller's log line; it is not a credit, since d7's rate needs
    the examination it would be a positive of.

    **Noise and abuse limits.** Labels describe the key that recalled, so only that key's own
    actions label its events; another key's get labels nothing of this key's. A label never
    attaches to an event older than 24 hours. Each `(recall_id, artifact_id, label)` is stored
    once. A key's labels past 1,000 a day are stored but not credited
    (`oml_usage_credits_total{outcome="rate_limited"}`). The windows are starting values, chosen
    by the operator's reading of how an agent works; whether they capture what a reader did is
    d11's first measurement (FR-105). The honest limit: an agent that recalls and then reads the
    text the answer already carried (recall returns text unless `include_text: false`) leaves no
    trace at all, which is why d6 exists.

- **d6 — An optional `feedback` member on the existing read, and the use case it serves.** A
  recall's answer gains `recall_id`. `oml_read`, in any of its modes, and `POST
  /v1/memories/recall` accept `feedback: {recall_id, useful: [artifact_id…], not_useful:
  [artifact_id…]}`, at most 20 ids in all. It is accepted only for a `recall_id` this key issued
  in the last 24 hours, and only for ids that recall served to it; anything else is refused
  `request.validation` with `details.field: "/feedback/…"`, and the read itself is not run, so
  a malformed member never passes unnoticed. A valid member is queued to the labeller like an
  event and never changes the read it rides on. `oml_read` with `feedback` and no `query` or
  `artifact_id` is the listing mode as today; the plugin's skill sends feedback on the agent's
  next read. **The use case:** *an agent tells the server which memories of its last recall
  answered its question and which did not, so that recall improves for the next reader.* It is
  needed because the inferred positives miss the common case: recall serves each hit's text, so
  an agent that found its answer usually neither fetches it nor cites it. It is written into the
  use-case call map with this record's build. **Why no tool** (ADR-0039 d8): it is a parameter of
  the read the agent makes next anyway, carries no result of its own, and needs no permission
  rule that `oml_read`'s does not have; a fourth tool would cost every client's tool list for a
  call the agent makes unprompted only when its skill tells it to.

- **d7 — Usage ranks a hit by the rate at which other readers used it when shown, smoothed,
  log-scaled, capped and decayed, and it never fetches a candidate.** For a candidate version
  `v` and a recalling key `r`:

    - **Whose usage.** A credit (d8) for `v` counts only when its principal and key are neither
      `v`'s author nor any author in `v`'s lineage closure, the closure R2 excludes from a verdict
      (ADR-0001 d2); an author cannot rank its own work up, by fetching, citing or feedback. Own
      held hits never credit. Admin reads (`memory.admin_read`) and the review queue's serves are
      not recalls and produce no event.
    - **Where it counts, and who may see it.** A credit is kept under its **source workarea**, the
      crediting key's own workarea (`source_workarea_id`), in `usage_state`. A recall sums
      `usage_state` over the source workareas that `r` may read, the promoted nodes of its
      candidate scope's computation (ADR-0049 d2), exactly as ADR-0052 d5 sums word counts over
      readable nodes. So the fact "readers in workarea X used `v`" moves a rank only for a reader
      who may read X; a private team's usage of a `universe` claim never reorders another team's
      recall. A derivation credits only when the citing version is served, since a held citing
      row is its author's alone; a rejected or withdrawn citer never credits. That reads which
      rows are served, the visibility the gate decides, and never the verdict, the reviewer or
      `promoted_at`, as ADR-0052's counts do.
    - **The rate.** Positives `P` are the decayed credits; examinations `E` are the decayed
      impressions by crediting principals, each weighted by the probability that a hit at its
      rank is read, `1 / log2(1 + rank)`, the position model nDCG uses. With the pseudo-count `α`
      and prior `p0`, constants of the build (starting values 10 and 0.1):
      `rate = (P + α·p0) / (E + α)`. A memory with no examinations scores exactly `p0`: new
      memories start neutral, never last.
    - **Log-scaled and capped.** The leg's value is `u = 0.5 + clamp(log2(rate / p0), −L, L) /
      (2·L)`, with `L` 2 (a quarter to four times the prior): in `[0, 1]`, `0.5` for no
      evidence, below it for memories shown often and used rarely.
    - **Decayed.** `P` and `E` are accumulators that halve every `H` days (starting value 30):
      `usage_state` stores them with `as_of`, a credit adds to them after multiplying by
      `2^(−Δt/H)`, and a recall reads them decayed to now. So a memory used a year ago and never
      since returns to `p0`.
    - **Bounded.** The leg is fused by convex combination (d9) at weight `w ≤ 0.15` on its
      absolute scale, never renormalised per query. Two candidates can swap only when the text
      legs' normalised scores differ by less than `w`, so usage reorders near-ties and never
      lifts a weak text match over a strong one. **Usage never adds a candidate**: it scores only
      the candidates the text retrievers returned, and a memory no query word reaches is never
      served for its popularity.
    - **No verdict.** The leg's inputs are `usage_state` and the candidate list. A law holds that
      changing any verdict, reviewer or `promoted_at` while the served set stays the same changes
      no `u` (d13).

    **A new version starts at `p0`; usage is not inherited.** Credits are kept by version, and a
    supersession's successor begins with none. The usage endorsed the predecessor's text, which
    the successor changed; inheriting it would let any writer of a popular claim replace its
    content and keep its rank until readers noticed, which is the author ranking its own work up
    by another route. The cost, carried openly: a corrected memory may rank below its stale
    predecessor's level of standing for a while, which matters little because the predecessor is
    no longer served, and the successor competes at the neutral prior, not below it.

- **d8 — Anti-gaming: one credit per principal per version per day, a daily cap per key, and an
  honest limit.** A credit is the unit `P` counts. Within one UTC day, a principal credits a
  version at most once (`usage_credits` is unique on `(version_id, principal, window_day)`),
  whatever mix of gets, derivations and feedback it sent, and whichever of its keys sent them;
  its impressions of that version count once that day too. This is stronger than a per-session
  rule, which the lead could not enforce: it issues no MCP session (the facade is stateless,
  [MCP plugin](../api/mcp-plugin.md)), and `X-Correlation-Id` is a label, never an input to a
  decision (ADR-0042 d5, d6). A key credits at most 200 versions a day; past that its labels are
  stored for d11 and credit nothing. An explicit `not_useful` adds an examination with no
  positive; it never subtracts below the prior's floor, which `L` bounds. **The limit:**
  principals are what an admin issues; one person holding many principals, or several people
  agreeing to use one memory, can raise it to `L`'s cap at `w`'s weight and no further. The
  service cannot see that collusion, as it cannot see undeclared derivation (ADR-0001 d6);
  `oml usage report` lists, for each version near the cap, how many principals credited it, so
  an admin can.

- **d9 — `bm25-postings-usage`, a named pipeline fused by convex combination, explains each hit's
  usage.** The core's fusion gains `Cc { legs }`: each leg's score, min-max normalised over this
  query's own candidates for a text leg and taken on its absolute `[0, 1]` scale for the usage
  leg, weighted and summed, ties by id ascending as d3 of ADR-0049 orders them. The offline
  experiment found a convex combination above reciprocal rank in 30 of 32 fused cells
  (`eval/bench/results/offline/REPORT.md` on `exp/sparse-eval`); normalising each text leg over
  the query's readable candidates keeps a row outside the scope from setting any minimum or
  maximum. Usage is not a `Retriever`: it retrieves nothing. The store port gains
  `usage(tenant, readable_sources, candidate_ids) -> Vec<UsageEvidence>`, one index read by
  `(version_id, source_workarea_id)`, uuid equalities only, so row security keeps it an index
  condition (ADR-0052's reason).

    | Pipeline | Legs | Fusion | Enters |
    |---|---|---|---|
    | `bm25-postings-usage` | `bm25_postings`, then `usage` over its candidates | convex combination, text `1 − w`, usage `w` (starting `w` 0.1) | with 0023, disabled |

    A request, a tenant or a deployment chooses it by name (ADR-0049 d4). A hit gains
    `matched.usage: {u, contribution, credits, examinations, prior: bool}`, where `contribution` is
    `w × (u − 0.5)`, the amount usage moved its fused score, `credits` and `examinations` are the
    decayed figures rounded to one decimal, and `prior` is `true` when the hit had no evidence;
    `matched.via` gains `usage` when `contribution` is not zero. The figures come only from
    readable sources, so they tell a reader nothing about workareas it cannot read. Combining
    usage with the reranker of ADR-0053 is not offered: a reranker after fusion would discard the
    leg, and one before it is a second decision (open question 4).

- **d10 — It becomes the default only through ADR-0049's evidence gate, measured on real usage.**
  The recall eval and the external sets hold no usage, so on them the leg scores every hit `p0`
  and the pipeline must rank exactly as `bm25-postings` (a law, d13): they prove it harmless,
  not useful. Three further measurements, each committed with its date, data window and build:
    1. **Held-out replay (d11).** For a tenant that opts in, `usage_state` is rebuilt from the
       events of days −30 to −8 only, and the labelled events of days −7 to −1 are replayed under
       both pipelines; the leg never sees a credit from the week it is judged on, nor from the
       key whose query is replayed. Gate: nDCG@10 at least 0.01 above `bm25-postings`, paired
       randomisation test p < 0.05, over at least 500 labelled recalls; no fall of more than
       0.005 on the slice of recalls whose labelled hit was promoted in the 14 days before.
    2. **Team-draft interleaving**, only in tenants that set `telemetry.experiments` `on`. A
       share of their recalls, 10% by a hash of `(key_id, UTC day)`, interleave the two pipelines'
       lists (Radlinski, Kurup and Joachims, 2008); the answer says so (`search.interleaved:
       {pipelines, share}`) and each hit names its team in `matched.team`; credits attribute to a
       team. Gate: the usage pipeline wins more interleaved recalls with a credit than it loses,
       sign test p < 0.05, over at least 1,000 such recalls. Interleaving changes what those
       recalls show, which is why it is opt-in, disclosed, and capped at 10%.
    3. **Fairness and the loop.** The share of top-5 impressions going to memories promoted in
       the last 14 days falls by at most 10% relative under the usage pipeline in the replay;
       the leg's added p95 is at most 5 ms at 1,000,000 memories (FR-104).
    A counterfactual estimate from the logged rankings alone (inverse propensity scoring) is not
    the gate: the served rankings are deterministic, so most alternative rankings have no logged
    probability, and the estimate would rest on the position model alone. Interleaving supplies
    the randomness the estimate lacks. The constants `α`, `p0`, `L`, `H` and `w` are chosen on
    the replay of one tenant and confirmed on the interleaving of another before the gate is
    read, so the gate does not grade the data that tuned it. Until all three hold, the pipeline
    is opt-in, and `bm25-postings` stays the default.

- **d11 — The real-usage eval set is built in the tenant's database, from opted-in text and
  labels, and only numbers leave it.** A labelled query is an event whose `query_text` is
  stored (d3) and which has at least one explicit label, or a `derived` or `get` label, with
  graded relevance: `feedback_useful` and `derived` 2, `get` and `superseded` 1,
  `feedback_not_useful` 0; `requery` and `no_use` alone make no labelled query, since they say
  the answer was missing, not which memory was right. `POST /v2/usage/eval` (tenant admin) and
  `oml usage eval --pipelines a,b` replay each labelled query under its original key's scope as
  it reads now (a revoked key's queries are skipped and counted), through the named pipelines,
  inside the lead, and answer `oml-bench/1` metrics only: success@1, success@5, nDCG@10, R@10,
  per slice, with counts. No query text, hit or memory id leaves the database in the answer.
  `eval/bench/` gains a `served-replay` suite that calls the route and commits the result
  beside the external sets; the maintainer's own tenant, opted in with the consent of its crews
  (ADR-0050 d16's version 2 source), supplies the first committed run. The set lives as long as
  its text: 30 days, so it is rolling, and a run records its window rather than a fixed set.
  The cost: no two runs a month apart measure the same queries; a frozen set needs text kept
  longer, which the operator has not decided (open question 1).

- **d12 — Remember-side quality is tracked for curation, and nothing of it enters ranking.**
  Tracked: the similar check's outcomes and whether a refused write was resent or superseded
  instead (ADR-0050 d1); `quality.*` warnings by class (d6 there); time from submit to verdict;
  rejections, with `duplicate_of` (ADR-0050 d11), per key; supersessions within 30 days of
  promotion, per workarea (churn); memories never returned by a recall since promotion, and not
  returned in 30 and 90 days; and stale tips (ADR-0050 d7). It is for a tenant's admins and
  reviewers: a key whose writes are rejected often needs its skill or its scope looked at; a
  workarea of unused memories is a curation list. A person may act on it through the verbs
  their role holds; no agent may, by R1, and no list proposes a demotion by itself. Verdicts are
  read here, which is why none of this reaches the ranking: a law (d13) holds the ranking
  input to `usage_state`, which no verdict writes.

- **d13 — Laws, tests and doctor lines.** Laws, in `oml-port-contracts` against PostgreSQL as the
  serving role (`OML_TEST_REQUIRE_SPLIT=1`):
  `with_no_usage_bm25_postings_usage_ranks_as_bm25_postings`;
  `an_authors_own_gets_citations_and_feedback_never_move_its_memory`;
  `a_principal_in_the_lineage_closure_never_credits_the_version`;
  `usage_from_a_workarea_the_reader_cannot_read_never_changes_its_ranks`;
  `usage_never_adds_a_candidate_the_text_legs_did_not_return`;
  `two_candidates_swap_only_when_their_text_scores_differ_by_less_than_w`;
  `changing_a_verdict_reviewer_or_promoted_at_with_the_served_set_unchanged_changes_no_usage`;
  `a_held_citing_row_credits_nothing_until_it_is_served`;
  `a_successor_version_starts_at_the_prior`;
  `a_principal_credits_a_version_once_a_day_whatever_its_keys_send`;
  `a_keys_credits_past_the_daily_cap_are_stored_and_not_counted`;
  `usage_decays_to_the_prior_with_its_half_life`;
  `feedback_for_another_keys_recall_or_an_unserved_id_is_refused_and_the_read_not_run`;
  `a_recall_answers_the_same_when_the_event_queue_is_full`;
  `no_agent_writer_reader_or_dreamer_key_reads_a_recall_event_on_any_transport`;
  `query_text_is_null_unless_the_tenant_opted_in`;
  `no_row_older_than_30_days_is_read_and_an_overdue_partition_is_emptied`;
  `an_erased_version_leaves_no_usage_and_no_event_names_it_after_the_job`;
  `every_quality_metric_label_is_from_its_declared_set`;
  `a_replay_answer_holds_no_text_and_no_id`; and
  `the_replay_never_reads_a_credit_from_the_week_it_judges`. Doctor (admin) names: events
  dropped above 1% of recalls in the last hour, by reason; a day partition missing for today
  or tomorrow, or one past 30 days not yet dropped, naming `oml admin telemetry rotate`; the
  labeller's lag above 10 minutes; `usage_state` rows naming a version that no longer exists;
  the usage pipeline being a default before d10's committed evidence exists; a tenant with
  `telemetry.query_text` `on` whose admin page has not shown the notice; and the interleaving
  share outside `[0, 0.1]`.

## Consequences

- **Pages changed when this record is accepted**: [retrieval](../architecture/retrieval.md)
  (the usage leg, convex fusion, and how a recall event differs from the read log),
  [observability](../operations/observability.md) (the metric families of d1 and the dashboard),
  the PostgreSQL schema page (four tables, their partitions and grants, migration 0023), the
  [configuration](../operations/configuration.md) page (`telemetry.query_text`,
  `telemetry.experiments`, the 30-day retention, the rotation timer), the
  [HTTP API](../api/http-api.md), [MCP plugin](../api/mcp-plugin.md), `mcp-tools.json` and
  `openapi.yaml` (`feedback`, `recall_id`, `matched.usage`, `matched.team`,
  `search.interleaved`, the `/v2/usage/*` routes), the CLI reference (`oml usage`, `oml admin
  telemetry rotate`), the use-case call map (d6's use case), the plugin's skill (send feedback
  on the next read), ADR-0005's inventory, ADR-0049's and ADR-0052's pipeline tables, and the
  Helm chart (the CronJob and the dashboard).
- **Roadmap rows opened**: FR-104 (the event write's cost on a recall, the tables' size and the
  usage leg's p95 at 1,000,000 memories and 100 recalls a second, measured on the scale lane);
  FR-105 (whether d5's windows capture what a reader did, measured on the first opted-in
  tenant); FR-106 (the usage pipeline's default, by d10's three measurements); FR-107 (usage
  colluded by principals one person controls, which the service cannot see).
- **Costs**: four tables per tenant and about 31 day partitions each for the three event tables,
  so the catalog grows with tenants × days (open question 2); a background writer and labeller in
  the lead; an owner job the operator must schedule; usage evidence on every recall of the usage
  pipeline; and query text, where opted in, held for 30 days as personal data.
- **What stays**: the read log's design and its sentence; `bm25-postings` as the default;
  ADR-0042's labels and 30 days; the three tools.
- **Numbering**: ADR-0050 to ADR-0053 are taken on `main` or branches, so this record is ADR-0054;
  its migration is 0023, the next after ADR-0052's 0021 and 0022; its roadmap rows follow FR-103.

### Cost at 1,000,000 memories and 100 recalls a second

Estimated, not measured; FR-104 measures each line.

| Item | Estimate | From |
|---|---|---|
| Events | 8.64 million a day; about 400 bytes each with 10 hits (ids 160, scores 40, fixed columns about 120, tuple header 24, digest 32); about 3.5 GB a day, about 100 GB at 30 days, plus about 25 GB of indexes | arithmetic on d2's columns |
| Query text, where opted in | plus the query's length, about 60 bytes for the eval's queries | the recall eval's query lengths |
| Event writes | 100 rows a second in one batched insert a second; about 40 KB/s of heap and a similar order of WAL | arithmetic |
| Labels | at most a few per recall; one index probe per get or write by the acting key | d5 |
| `usage_state` | at most one row per served version and source workarea: with 1,000,000 versions read from 5 workareas each, 5 million rows, about 80 bytes each, 0.4 GB with its index | arithmetic |
| Usage on a recall | one index read for at most 50 candidates over the readable sources; budget 5 ms p95 | d10 (3) |
| Prometheus | about 300 new series per lead | d1 |
| Partitions | per tenant, 3 event tables × 31 days + `usage_state`, each with its indexes | d2 |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Per-memory or per-query Prometheus labels** | One series per memory is 1,000,000 series at the stated scale, and a query or memory label puts a tenant's words and ids in a store outside row security, with its own retention (ADR-0042 d12). The per-memory answers are PostgreSQL's, to an admin (d1). |
| **Global popularity, counted across workareas or tenants** | A tenant-wide count lets rows a reader cannot read move its ranks, which `a_row_outside_the_scope_never_changes_a_candidates_score` forbids for text (ADR-0049 d7) and d7 forbids for usage; across tenants it would leak one tenant's activity into another's answers. |
| **Counting the author's signals** | An author's gets, citations and feedback would rank its own work up, the act R2 refuses for a verdict; nothing distinguishes an author's honest use from promotion of its own row. |
| **An LLM judge of relevance in the read path** | A model call per recall adds latency and cost the reranker's budget already strains (ADR-0053 d5), sends the query and the memories to wherever the model runs, and grades with no knowledge of what the reader needed. Offline, over opted-in sets, it is not ruled out as an annotator (open question 3). |
| **A new MCP tool for feedback** | ADR-0039 d8: the feedback is a parameter of the read the agent makes next; a tool would cost every client's catalog for a call with no result of its own (d6). |
| **Storing query text by default** | The text is what a person or an agent asked, often the most sensitive thing the service holds about them; the digest serves every metric and every ranking input, and only the eval set needs text, so it is the tenant's choice (d3). |
| **Raw counts, or counts normalised per query, as the usage leg** | Counts rank what was shown most, so the first memory to be shown keeps winning; per-query min-max would stretch a small difference to the full weight. A smoothed rate with a position model and an absolute scale bounds both (d7). |
| **Usage as a retriever that adds candidates** | It would serve memories no query word reaches because others used them, a popularity feed in place of recall. |
| **Inheriting usage across a supersession**, whole or in part | Lets a writer replace a popular claim's content and keep its rank (d7). |
| **Extending `read_events`** | The read log is an audit record, anchored in the journal, under a retention the tenant sets for auditors; labels, scores and opt-in text are telemetry that must be purged at 30 days and never offered as audit. One table would hold two retentions. |
| **Writing events in the recall's transaction, or through the outbox** | A recall commits nothing and runs read-only; a write there adds a commit and its latency to every recall, the cost d2 forbids. |
| **De-duplication by session or correlation id** | The lead issues no session, and a correlation id is a label a caller may change each request (ADR-0042 d5); a principal and a day are what the service can enforce (d8). |
| **Reciprocal rank fusion for the usage leg** | RRF reads ranks, so a leg with nearly equal values would reorder fully by them, unbounded by any weight; and the offline experiment found it below CC in 30 of 32 cells. |

## Open questions

Questions 1 to 5 are settled above (*The split, and what acceptance amended*). Question 6
remains open and holds the ranking half.

1. **A frozen eval set.** d11's set is rolling because text lives 30 days. A set an admin
   confirms, kept longer under a tenant policy, would let runs compare across months; it needs
   the operator's decision on query-text retention beyond (b).
2. **Partition count.** Daily partitions per tenant per event table are 93 tables a tenant;
   with thousands of tenants the catalog grows. Weekly partitions with the serving role's batch
   delete for the tail, or one partition set for all small tenants, are the alternatives;
   decided on FR-104's numbers.
3. **An offline annotator.** Whether a model may grade an opted-in tenant's labelled queries
   offline, inside the deployment, to densify d11's labels, and under which consent.
4. **Usage with the reranker.** Whether usage is fused after the reranker as a third leg, and
   whether that pipeline is worth its latency.
5. **The starting constants** (`α` 10, `p0` 0.1, `L` 2, `H` 30 days, `w` 0.1, the windows of
   d5, the caps of d8): set by d10's replay, confirmed by its interleaving.
6. **A one-person tenant.** With its author excluded, a tenant of one principal produces no
   credit at all, so the usage pipeline ranks there as `bm25-postings`. Whether a development
   workarea's self-signed exception (ADR-0001 d5) should extend to usage is not decided here; this
   record says no.

## Test plan

- **Laws first** (d13), red before the code, on CI's pinned PostgreSQL as the serving role.
- **Neutrality**: the recall eval and the external sets through `bm25-postings-usage` equal
  `bm25-postings`'s committed results to the last digit, with an empty `usage_state`.
- **Telemetry under load**: the scale lane at 100 recalls a second for 10 minutes with the
  writer on and off; the recall's p50 and p99 differ by at most the 50 µs budget; drops counted;
  table and index size per million events recorded (FR-104).
- **Labeller**: a scripted agent session (recall, get, derive, requery, feedback) against a
  throwaway lead; each expected label present once, nothing labelled from another key's actions.
- **Gaming**: one principal with ten keys crediting one version a hundred times a day moves its
  `u` as one credit; an author's hundred gets move nothing; a key past its cap credits nothing.
- **Replay and interleaving**: d10's three measurements on the maintainer's opted-in tenant,
  committed under `eval/bench/results/served-replay/`.
- **Erasure**, with erase's build: an erased version's usage rows go in the erase transaction,
  and no event names it after the job.
