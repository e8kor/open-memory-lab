# Observability

**Status:** design draft for review · decided, not built — every metric name
below is a design commitment; no binary emits anything yet.

This page declares what each workload measures, what `oml doctor` reads and
returns, the structured-logging rules (including the one that is a correctness
rule, not a style rule), and the starting points for alerting. The workloads
themselves are placed in [deployment](deployment.md); the guarantees the
signals watch are declared in [storage](../architecture/storage.md),
[admission](../architecture/admission.md),
[reconciliation](../architecture/reconciliation.md) and
[retrieval](../architecture/retrieval.md).

Two principles shape everything here:

1. **The health signals that matter are lag signals.** OML's write path is a
   pipeline of queues — admission's review queue, the outbox, the lake-writer's
   frontier, the indexer's backlog — and each queue's depth and age say more
   than any pod status. A deployment where every pod is green and the oldest
   held item is older than its expiry is down for its purpose.
2. **The system of record is also the record of health.** Queue depths,
   frontiers, dead-letter counts and the chain head all live in Postgres, so
   the authoritative health readout (`oml doctor`) reads them from there
   through the API. The Prometheus-style metrics are the continuous,
   scrapeable projection of the same facts; when they disagree, the doctor is
   right.

## Metrics per service

Metrics are Prometheus-style, prefixed `oml_`, labelled by `tenant_id` where
the cardinality is per-tenant (quota and ceiling concerns for that labelling
are part of [FR-7](../product/roadmap.md)). Names below are normative for the
implementation; thresholds are deliberately absent — nothing is measured yet
([D17 discipline](../product/roadmap.md)).

### `oml-server`

| Metric | Type | What it watches |
|---|---|---|
| `oml_http_request_duration_seconds` (route, status) | histogram | API latency and typed-error rates per route |
| `oml_chain_append_duration_seconds` (tenant) | histogram | Time to assign and commit one journal event: advisory-lock wait + transaction commit, including synchronous replication in the scaled profile. Per-tenant writes serialize on the chain lock, so this histogram **is** the per-tenant write-throughput ceiling made visible. |
| `oml_review_queue_depth` (tenant, workarea) | gauge | Held items waiting in the review queue |
| `oml_review_queue_oldest_age_seconds` (tenant, workarea) | gauge | Age of the oldest held item — the signal that races `held_item_expiry` ([admission](../architecture/admission.md)) |
| `oml_review_verdicts_total` (tenant, outcome) | counter | Drain rate of the review queue; the measured counterpart [FR-2](../product/roadmap.md) asks for |
| `oml_serves_active` / `oml_serve_invalidations_total` | gauge / counter | Review serves outstanding and serves invalidated by content change ([CLI review ceremony](../cli/reference.md)) |
| `oml_outbox_depth` | gauge | Outbox rows not yet drained — the producer-side view of the lag the lake-writer reports from the consumer side |
| `oml_rls_denied_total` (service_role) | counter | Postgres RLS denials observed by the server's own roles. The expected value is zero: a nonzero count means a service bug or a tenant-isolation probe, and either deserves attention ([security](../architecture/security.md)) |
| `oml_quota_rejections_total` (tenant) | counter | 429s issued by the per-replica token buckets — the enforcement approximation [FR-8](../product/roadmap.md) tracks |
| `oml_idempotency_replays_total` / `oml_idempotency_conflicts_total` | counter | Stored-response replays vs. same-key-different-digest errors ([HTTP API](../api/http-api.md)) |
| `oml_upload_sessions_active` (tenant) | gauge | Resumable upload sessions open |
| `oml_lead_lease_age_seconds` | gauge | Age of the current lead lease since acquisition ([deployment — the lead lease](deployment.md)). Paging-adjacent on the frontier-lag precedent: during a takeover, writes are refused until a new lead holds the lease and passes the fork check, so lease state — not pod status — is what says the write path is up. |
| `oml_lead_takeovers_total` | counter | `system.lead_acquired` events observed — lead takeovers. Nonzero during failover or rolling updates is the lease working; a climbing rate in steady state means lead churn, and every takeover is a write-refusal window ([FR-24](../product/roadmap.md) measures that window). |

### `oml-lakewriter`

| Metric | Type | What it watches |
|---|---|---|
| `oml_outbox_frontier_lag_events` (tenant) | gauge | Outbox head seq minus last applied seq — how far the lake is behind the journal, in events |
| `oml_outbox_frontier_lag_seconds` (tenant) | gauge | Age of the oldest undrained outbox row — the same lag in time, the form humans alert on |
| `oml_lake_batch_duration_seconds` / `oml_lake_batch_events` | histogram | Micro-batch cost and size |
| `oml_delta_commit_duration_seconds` (table) | histogram | Per-table Delta commit latency. No expectation is asserted; no measurement exists yet, and Delta commit cost is known to grow with version-history depth, which is why the lake write path is asynchronous by design. |
| `oml_delta_txn_conflicts_total` (table) | counter | Conditional-put losses and txn-version skips — the second fence firing ([deployment](deployment.md)). Nonzero during failover is the fence working; nonzero in steady state means two writers. |
| `oml_dead_letter_count` (tenant) | gauge | Rows in the dead-letter table awaiting journalled re-enqueue |
| `oml_dead_letter_moves_total` (tenant) | counter | Poison rows moved after the retry budget — each one also journals a materialize-skipped event |
| `oml_leader_lease_transitions_total` | counter | Lease acquisitions/losses; churn here predicts frontier-lag spikes |
| `oml_lake_maintenance_duration_seconds` (op) | histogram | OPTIMIZE / VACUUM / checkpoint runs — VACUUM cadence is an erasure-SLO term ([deployment](deployment.md)) |

### `oml-reconciler`

| Metric | Type | What it watches |
|---|---|---|
| `oml_reconcile_last_success_timestamp_seconds` | gauge | When a pass last completed — the CronJob's own freshness |
| `oml_reconcile_cursor_lag_events` (tenant) | gauge | Journal head minus the reconciler's persisted cursor |
| `oml_reconcile_findings_total` (class) | counter | Findings by the closed vocabulary — `missing`, `orphan`, `duplicate`, `state-mismatch`, `digest-mismatch`, `stale-frontier`, `dead-lettered` ([reconciliation](../architecture/reconciliation.md)) |
| `oml_reconcile_repairs_emitted_total` (class) | counter | Repairs emitted through the outbox via `oml-server` |
| `oml_reconcile_repair_halts` (tenant) | gauge | Artifacts whose finding recurred N times, halting auto-repair and raising an operator condition |
| `oml_reconcile_pass_duration_seconds` | histogram | Pass cost against `activeDeadlineSeconds` ([deployment](deployment.md)) |

### `oml-indexer`

| Metric | Type | What it watches |
|---|---|---|
| `oml_index_backlog_depth` (tenant, kind) | gauge | Promoted rows not yet embedded / not yet in FTS |
| `oml_index_lag_seconds` (tenant) | gauge | Age of the oldest unindexed promotion — the gap between "promoted" and "findable" |
| `oml_embedding_failures_total` (tenant) | counter | Embedding computation failures (the model and its location are open — [RS-1](../product/roadmap.md)) |

### Store-level signals (not emitted by OML binaries)

The scaled profile must also watch, from Postgres and the backup machinery
themselves: synchronous-standby quorum health for the journal domain (losing
quorum stalls or degrades the durability guarantee declared in
[storage](../architecture/storage.md)), WAL archiving success, and backup-job
success — the last of these is an erasure-SLO term, because the backup
retention window bounds how long erased plaintext-derived rows survive
([deployment](deployment.md)).

## The doctor contract

`oml status` is the one-line summary; `oml doctor` is the diagnosis
([CLI reference](../cli/reference.md)). Both read through the HTTP API with an
operator or admin key, from Postgres — not from the metrics pipeline — so they
work on a deployment with no Prometheus at all, and they are authoritative
when the two disagree. `--json` is supported everywhere; the JSON shape below
is the contract.

What `oml doctor` reads, per tenant in scope of the caller's key:

| Field | Source | Meaning |
|---|---|---|
| `queue_depth_per_step` | fold tables | Items at each admission step: captured, scanned, under review, awaiting ratification ([admission](../architecture/admission.md)) |
| `review_availability` | review policy objects + key/grant state | Per workarea: whether the configured policy is currently satisfiable — enough non-author dreamer-class keys with review capability exist and are unexpired. `false` means writes will hold until expiry: admission fails closed. |
| `oldest_held_age` / `held_item_expiry` | fold tables + policy | The race the review queue is losing or winning |
| `outbox_frontier_lag` | outbox head vs. frontier cache | Lag in events and seconds; the Delta txn version is the recovery authority, the cached frontier is what doctor can read cheaply — a `stale-frontier` reconciliation finding is the correction path |
| `dead_letter_count` | dead-letter table | Rows awaiting journalled re-enqueue, with age of the oldest |
| `chain_head` | journal | Per-tenant head seq and whether the last startup fork check passed |
| `lease_holder` | coordination lease | Which pod holds the lake-writer lease, and lease age |
| `lead_lease` | lead-lease row in Postgres | The current lead's holder id, address, epoch and lease age; **exactly one lead lease held** is a doctor check in every profile ([deployment — the lead lease](deployment.md)). A companion check verifies **no follower holds the signing key**: checkable in Kubernetes from the pod spec (no signing-key Secret mounted in `oml-server-follower`); in docker and standalone it is stated, not checkable, beyond the absence of the key file on the follower's mounts — the honesty is the check's output, not a gap it hides. |
| `reconciler` | journalled cursor + findings | Cursor lag, last pass time, open findings by class, repair halts |
| `settings_drift` | configured vs. effective | Delegated to `oml settings show`; doctor flags that drift exists |

Doctor renders each field with a verdict (`ok` / `attention` / `failing`) and
the rule it applied, in the same fail-closed, name-your-rule style as the
[error taxonomy](../api/errors.md). The verdict thresholds ship as defaults
and are unmeasured, like every number in this design.

### Per-profile checks

The install profiles ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md))
each weaken different guarantees, so `oml doctor` carries a short per-profile
check list on top of the common fields above:

| Profile | Doctor checks |
|---|---|
| standalone | Data directory is a local POSIX filesystem (the conditional-put scratch-key self-test; NFS/SMB refused). Host Postgres: pinned major version, pgvector present, the C6 owner/service role split, `FORCE ROW LEVEL SECURITY`. |
| docker | MinIO image matches the one declared pin the chart and compose share. The C6 role split in the containerized Postgres. The bootstrap one-shot completed and its enrollment secret was consumed. |
| kubernetes (`local`) | The MinIO pin (same fact as docker); the single-node durability reduction is reported, not silent. |
| kubernetes (`scaled`) | Quorum-synchronous settings on the journal domain (`synchronous_commit`, quorum standbys), as [deployment](deployment.md) requires. |
| all profiles, cluster mode | Exactly one lead lease held; no follower holds the signing key (pod-spec-checkable in Kubernetes; stated, not checkable, in docker and standalone — see `lead_lease` above). |

## Structured logging

All binaries log JSON lines to stdout: `ts`, `level`, `service`, `tenant_id`,
`request_id`, `event` (a stable snake_case name, not prose), plus
event-specific fields. Correlation: a request's log lines carry its
`request_id`; lines about a journalled act carry the journal `event_id`, so a
log line can always be joined to the chain.

### The plaintext rule (a correctness rule)

**No memory body, attachment content, extracted text, or recall query text
ever appears in a log line — at any level, including debug.** This is not
hygiene; it is the erasure design. The
[erasure inventory](../architecture/audit-and-lineage.md) enumerates every
copy class a body can inhabit and states each one's disposition at erase time.
A log stream is not in that inventory and must never need to be: logs are
shipped, indexed, and retained by machinery outside OML's control, and a body
that leaked into them would be an unenumerated, unerasable copy —
a copy-class violation of D4's boundary. The same rule covers recall query
text, because queries carry content; the read-event log records a **query
digest** for exactly this reason ([retrieval](../architecture/retrieval.md)),
and application logs follow suit.

What logs may carry instead:

- **Digests and ids** — `artifact_id`, `claim_key`, plaintext/ciphertext
  digests, journal seq, workarea node ids. The journal itself is digest-only
  ([storage](../architecture/storage.md)); logs get no more than the journal.
- **Attribution** — `tenant_id`, `key_id`, principal, workarea. These survive
  erasure by the stated boundary ([FR-1](../product/roadmap.md)); when FR-1
  closes with a pseudonymization act, log retention joins that act's
  inventory.
- **Never**: API key values (key ids only — the stored form is already a
  hash), DEK/KEK material or wrapped keys, serve payloads, scan excerpts that
  quote the body. A scanner finding is logged as rule id + offset range +
  digest, not as the matched text.

Log retention is a deployment-site choice precisely because, under this rule,
logs contain no erasable content — that is the property the rule buys, and the
cost is paid in debuggability: an engineer reproducing a scan or review issue
works from digests and the journalled record, never from a quoted body.

The read-event log is not logging. Who-read-what lives in `read_events`,
retention-bounded and digest-anchored to the journal
([retrieval](../architecture/retrieval.md)); application logs are operational
exhaust and carry no authority. An auditor gets the evidence export, never a
log archive.

## Alerting starting points

Starting points, not a tuned alert book: the structural alerts (zero/nonzero,
boolean) are meaningful on day one; every threshold alert needs numbers this
design does not have yet and must be set against measured baselines
([FR-2](../product/roadmap.md), [FR-7](../product/roadmap.md)).

| Condition | Signal | Why it pages (or tickets) |
|---|---|---|
| Fork check failing | `/readyz` red on all `oml-server` replicas after restart; `chain_head` failing in doctor | The deployment is refusing writes because the chain head did not verify — by design it stays down until an operator resolves it ([deployment](deployment.md)). Page. |
| Review unavailable | `review_availability: false` for a workarea | Admission fails closed; every write to that workarea will hold until expiry. Page if the workarea is in active use. |
| Oldest held age approaching expiry | `oml_review_queue_oldest_age_seconds` vs. the policy's `held_item_expiry` | Items are about to dead-letter not because they are poison but because review capacity lagged — the failure mode [FR-2](../product/roadmap.md) exists to quantify. Alert ahead of the expiry, at a margin set locally. |
| Dead letters present | `oml_dead_letter_count > 0`, or doctor's `dead_letter_count` | Every dead-lettered row is a journalled skip awaiting a human decision (re-enqueue or not). Ticket on first occurrence, page on growth. |
| Frontier lag growing monotonically | `oml_outbox_frontier_lag_seconds` trend | Brief lag is normal (micro-batching, lease failover); monotonic growth means the lake-writer is down, conflicted, or poisoned. Alert on sustained growth, not on a fixed depth. |
| Txn conflicts in steady state | `oml_delta_txn_conflicts_total` increasing outside failover windows | Two writers are alive; the second fence is absorbing what the lease should have prevented. Ticket: investigate lease churn. |
| Lease churn | `oml_leader_lease_transitions_total` rate | Predicts frontier-lag spikes; usually node or kubelet trouble. Ticket. |
| Lead takeovers in steady state | `oml_lead_takeovers_total` rate outside deploy windows | Every takeover is a write-refusal window ([deployment — the lead lease](deployment.md)); churn means the lead keeps losing its lease. Ticket, page if writes are failing. |
| RLS denials | `oml_rls_denied_total > 0` | Expected zero. A service bug or an isolation probe — either way, investigate ([security](../architecture/security.md)). Ticket, page on spike. |
| Repair halted | `oml_reconcile_repair_halts > 0` | A finding recurred past its attempt budget; auto-repair stopped on purpose and raised an operator condition ([reconciliation](../architecture/reconciliation.md)). Ticket, with the artifact id from doctor. |
| Reconciler stale | `oml_reconcile_last_success_timestamp_seconds` older than a few schedules | `concurrencyPolicy: Forbid` means slow passes queue up silently; staleness is how that surfaces. Ticket. |
| Backup job failed | store-level backup success signal | A missed backup stretches no erasure window, but a *changed retention configuration* or failed expiry does — and a failed backup chain breaks the restore ceremony ([deployment](deployment.md)). Ticket. |
| Sustained 429s for one tenant | `oml_quota_rejections_total` rate | Either the tenant outgrew its journalled quota grant or the per-replica approximation is misbehaving under replica churn ([FR-8](../product/roadmap.md)). Ticket. |

What this page does not decide: the scrape/alert stack (Prometheus, the
managed equivalent, or none locally), dashboard layouts, and every numeric
threshold. Those are deployment-site choices and, for the thresholds,
measurement work the [roadmap](../product/roadmap.md) tracks.
