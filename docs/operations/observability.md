<!--
id: operations/observability
tier: engineering
status: design draft for review · decided, not built
answers: which metrics and structured-log fields each workload commits to emit, what oml doctor reads, and the starting points for alerting
-->

# Observability

**Status:** design draft for review · the request log, the MCP call line, four request-metric
families and trace export built; every other metric name below is a design commitment no binary
emits yet.

**Release:** `oml doctor` since v0.1.0; the request log, the four request-metric families, `/metrics` and trace export since v0.4.1; the other metrics and the SLOs not scheduled ([releases](../product/releases.md)).

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

*Available since v0.4.1, the rows marked so · [releases](../product/releases.md)*

The lead serves these on a listener of its own, `GET /metrics` at `server.metrics_listen`, off
unless that setting names an address; the API listener never routes `/metrics`, so a proxy that
forwards the whole origin exposes none of it ([configuration](configuration.md),
[ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d11). v0.4.1 builds the four families marked below, labelled from closed sets
and without `tenant_id`; every other row stays designed (ADR-0042 d12). The histograms' bucket bounds
start at 1 ms and double sixteen times, to 32.768 s: the instrument's resolution, not a target.
No label value is ever a name, a principal or an address. The exposition is the OpenMetrics text
format, `application/openmetrics-text; version=1.0.0`, which Prometheus scrapes; a labelled family
shows its first series once a request has produced one, and `mode` is empty for a call whose tool
has no modes.

| Metric | Type | What it watches |
|---|---|---|
| `oml_http_request_duration_seconds` (route, status) | histogram | API latency and typed-error rates per route. `route` is the matched route template, or `unmatched`; probes answered `200` are not counted. **Since v0.4.1.** |
| `oml_mcp_call_duration_seconds` (tool, mode, outcome) | histogram | Per-tool latency and outcome on `/mcp`, which the route label alone shows as one route: `tool` a served, alias or designed name or `unknown`, `mode` the tool's mode, `outcome` `result`, `tool_error` or `rpc_error` ([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d10). **Since v0.4.1.** |
| `oml_chain_append_duration_seconds` (tenant) | histogram | Time to assign and commit one journal event: advisory-lock wait + transaction commit, including synchronous replication in the scaled profile. Per-tenant writes serialize on the chain lock, so this histogram **is** the per-tenant write-throughput ceiling made visible. |
| `oml_review_queue_depth` (tenant, workarea) | gauge | Held items waiting in the review queue |
| `oml_review_queue_oldest_age_seconds` (tenant, workarea) | gauge | Age of the oldest held item — the signal that races `held_item_expiry` ([admission](../architecture/admission.md)) |
| `oml_review_verdicts_total` (tenant, outcome) | counter | Drain rate of the review queue; the measured counterpart [FR-2](../product/roadmap.md) asks for |
| `oml_serves_active` / `oml_serve_invalidations_total` | gauge / counter | Review serves outstanding and serves invalidated by content change ([CLI review ceremony](../cli/reference.md)) |
| `oml_outbox_depth` | gauge | Outbox rows not yet drained — the producer-side view of the lag the lake-writer reports from the consumer side |
| `oml_rls_denied_total` (service_role) | counter | Postgres RLS denials observed by the server's own roles. The expected value is zero: a nonzero count means a service bug or a tenant-isolation probe, and either deserves attention ([security](../architecture/security.md)) |
| `oml_quota_rejections_total` (tenant) | counter | 429s issued by the per-replica token buckets — the enforcement approximation [FR-8](../product/roadmap.md) tracks |
| `oml_idempotency_replays_total` / `oml_idempotency_conflicts_total` | counter | Stored-response replays vs. same-key-different-digest errors ([HTTP API](../api/http-api.md)). **Since v0.4.1.** |
| `oml_upload_sessions_active` (tenant) | gauge | Resumable upload sessions open |
| `oml_lead_lease_age_seconds` | gauge | Age of the current lead lease since acquisition ([deployment — the lead lease](deployment.md)). Paging-adjacent on the frontier-lag precedent: during a takeover, writes are refused until a new lead holds the lease and passes the fork check, so lease state — not pod status — is what says the write path is up. |
| `oml_oauth_token_requests_total` (grant_type, outcome) | counter | Token exchanges and refreshes on the connection surface by RFC outcome; a rise in `invalid_grant` on `refresh_token` is lost refresh answers or stolen tokens ([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d10). **Since v0.4.1.** |
| `oml_recalls_total` (pipeline, fallback) | counter | Recalls by the search pipeline that ranked them, from the build's closed set, and whether `match: all` fell back to any word; a tenant moving between pipelines shows here ([ADR-0049](../adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md) d8). **Since v0.5.5.** |
| `oml_reranks_total` (outcome, reason) | counter | Reranking stages, `applied` or `skipped` with `busy`, `timeout` or `failed`; doctor's rerank lines read it, no longer `/readyz` ([ADR-0053](../adr/ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md) d9). **Since v0.5.6.** |
| `oml_web_exchanges_failed_total` (provider, reason) | counter | Sign-in exchanges with Google or GitHub that failed, by the closed reason; doctor's web lines read it, no longer `/readyz` ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d18). **Since v0.5.7.** |
| `oml_recall_answers_total` (pipeline, result) | counter | Recalls by pipeline and shape: `hits`, `empty`, or `below_floor` when the relevance floor withheld a row; the zero-hit share ([ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) d1). **Since v0.5.7.** |
| `oml_recall_duration_seconds` (pipeline) | histogram | Time the app spent answering a recall, by pipeline, on the bounds above. **Since v0.5.7.** |
| `oml_recall_stage_duration_seconds` (pipeline, stage) | histogram | Time a recall spent in each stage, on the bounds above: `scope`, `retrieve_<retriever>` per list (`keyword_all`, `keyword_any`, `bm25`, `trigram`, `bm25_postings`, `sparse`, the sparse query's encoding included), `fuse`, `rerank`, `suggest`, `hydrate`, `cut`. Series exist only for the stages a served pipeline runs: the default `bm25-postings` uses five ([ADR-0049](../adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md) d14). **Since v0.5.9.** |
| `oml_recall_outcomes_total` (label) | counter | Labels of what a recall's reader did next: `get`, `derived`, `superseded`, `feedback_useful`, `feedback_not_useful`, `requery`, `no_use`; over `oml_recall_events_total{outcome="written"}` it is the follow-up rate. **Since v0.5.7.** |
| `oml_recall_events_total` (outcome) | counter | Recall events `written`, or `dropped_queue_full`, `dropped_store_error`, `dropped_no_partition`; a drop never changes a recall's answer. **Since v0.5.7.** |
| `oml_recall_labels_dropped_total` (reason) | counter | Labels not stored: `rate_limited` (a key past 1,000 inferred labels in a UTC day), `queue_full`, `store_error`. **Since v0.5.7.** |
| `oml_telemetry_queue_depth`, `oml_labeller_lag_seconds`, `oml_telemetry_purge_overdue` | gauge | The recall telemetry's health: items waiting for the writer (bound 10,000), seconds since the labeller's last pass, and tenants whose oldest event, found at the start of a pass, is past the 30 days and a minute, so the purge is behind. **Since v0.5.7.** |
| `oml_memories_unrecalled` (promoted_for) | gauge | Served memories no recall of the last 30 days returned, by how long ago they were promoted (`7d`, `30d`, `90d`, `older`), refreshed every 10 minutes. **Since v0.5.7.** |
| `oml_remember_duplicates_total` (relation) | counter | Writes whose text a served memory of the same workarea already holds (`same_text`) or not (`none`); a write that repeats a `claim_key` is refused as `submission.claim_key_conflict` and counted by the request histogram's `409`. **Since v0.5.7.** |
| `oml_remember_similar_total` (relation, on_similar), `oml_remember_similar_duration_seconds` | counter, histogram | Writes the similar check ran for, by the best relation it named (`none`, `same_text`, `near_copy`) and `on_similar` (`hold`, `refuse`), and the check's time on the bounds above ([ADR-0050](../adr/ADR-0050-remembering-checks-what-is-already-held-replaces-instead-of-repeating-and-says-how-the-memory-will-be-found.md) d1, d17). **Since v0.5.9.** |
| `oml_review_wait_seconds` (verdict) | histogram | Time from a memory's submission to its verdict, by `approve` or `reject`, on bounds from one minute doubling to about 23 days; its count by verdict is the rejection rate, deployment-wide. **Since v0.5.7.** |
| `oml_oauth_connect_code_failures_total` | counter | Wrong connect-code entries on the authorization page; a sustained rate is someone guessing ([security](../architecture/security.md#connections-the-oauth-surface)) |
| `oml_oauth_client_metadata_fetch_total` (outcome) | counter | Client metadata document fetches by outcome — fetched, cached, a stale copy served, refused address, timeout, oversize, invalid, busy; a deployment with no outbound HTTPS shows only failures |
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

### The `oml-quality` dashboard

*Since v0.5.7 · [ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) d1*

[`dashboards/oml-quality.json`](dashboards/oml-quality.json) is a Grafana dashboard over the
families above, to import against the Prometheus that scrapes `server.metrics_listen`: the
zero-hit and below-floor share and the latency per pipeline, the rerank skipped share, the
scan-fallback share of `bm25-postings`, the follow-up rate by label, duplicate writes, the time
to review by verdict, the rejection rate, the memories never recalled in 30 days by age, and the
telemetry's own health. Every label is from a closed set and none names a tenant, a key or a
query: the families add at most 300 series to a lead. What one key or one memory did is the
tenant admin's to read, with `oml usage events`, never a label.

## The doctor contract

`oml status` is the one-line summary; `oml doctor` is the diagnosis
([CLI reference](../cli/reference.md)). Both read through the HTTP API with an
operator or admin key, from Postgres — not from the metrics pipeline — so they
work on a deployment with no Prometheus at all, and they are authoritative
when the two disagree. One field is not read from Postgres: `connect_code_failures`
is the lead's own count, read through the same API. `--json` is supported everywhere; the JSON shape
below
is the contract.

What `oml doctor` reads, per tenant in scope of the caller's key, and, for an operator key, the
deployment-level fields marked so:

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
| `names_assigned` | journal + fold tables | Per chain, whether its `system.names_assigned` is journalled and folded ([ADR-0026](../adr/ADR-0026-names-are-keys.md) d11). A follower on the schema version that adds names waits for it, not ready and serving nothing; a lead that finds a NULL `name` in a chain whose event is folded refuses to start, and this field names the table ([rows created before names](../api/http-api.md#rows-created-before-names)). |
| `oauth_discovery` | the two discovery documents, fetched by the CLI from `<issuer>` over the public network, and the issuer the lead recorded in `lead_lease` | Present only when `oauth.issuer` is set. `failing` when either document is unreachable, when `resource` is not exactly `<issuer>/mcp`, or when `issuer` is not the issuer the lead recorded — the signature of a proxy that does not route the paths or rewrites them, or of a node serving another issuer than the lead's; the output names the URL and the field. The CLI reaches whichever node the proxy routes it to, so it sees a node serving another issuer only when routed there. As built, with no follower and no issuer in `lead_lease`, the issuer compared is the lead's `oauth.issuer` as `/readyz` names it at the CLI's configured URL, a route that does not depend on the discovery paths; `off` only where `/readyz` names none, and where it names one, any status but `200` at `<issuer>/.well-known/*`, a `404` from a proxy that does not pass the paths included, is `failing` with that URL ([HTTP API](../api/http-api.md#connections-oauth-for-hosted-mcp-clients), [configuration](configuration.md)) |
| `connect_codes` | `connect_codes` | Per tenant: live codes, and codes that lapsed unredeemed in the window ([roles and keys](../architecture/roles-and-keys.md#connections)) |
| `connect_code_failures` | the lead's in-memory count | **Deployment-level, operator key only.** Wrong connect-code entries since the lead started, deployment-wide: a wrong entry matches no code, so it names no key and no tenant, and it writes no row, since an authorization request has none until its code verifies ([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d9); the count starts again at a restart or takeover, and no tenant admin's doctor reads it. `oml_oauth_connect_code_failures_total` is the metrics view of the same count. `attention` when wrong entries exceed the threshold — someone is guessing, and the bound is L·G / 2⁶⁰ |
| `connections` | `oauth_connections` + `oauth_tokens` + `api_keys` | Per key: live connections, as [roles and keys](../architecture/roles-and-keys.md#connections) defines them, and, apart, open connections that are not live, by why. Connections revoked for `refresh_reuse` or `code_reuse` in the window; any reuse revocation is `attention`: a stolen token, or a client that lost a refresh answer, and either way someone needs a new connect code |
| `search` (rerank) and `web` (providers) | `oml_reranks_total{outcome="skipped"}` and `oml_web_exchanges_failed_total` on the metrics listener, read where `OML_METRICS_URL` names it | *Since v0.5.7 ([ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) d1).* `/readyz` names the reranker and the providers and counts nothing. `warn` when reranks were skipped busy or late, or failed, since the lead started, and when a provider's exchanges failed, with the reasons; without `OML_METRICS_URL` doctor says where the counts are |
| `telemetry` | the metrics listener: `oml_recall_events_total`, `oml_telemetry_queue_depth`, `oml_telemetry_purge_overdue`, `oml_labeller_lag_seconds` | *Since v0.5.7 ([ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) d13).* `warn` when more than 1% of recall events were dropped since start, by reason; when more than 5,000 wait for the writer; when a tenant's events past the 30 days were found at a pass's start, so the purge is behind; and when the labeller's last pass is more than 10 minutes old. `off` when the server records no recall |
| `search` (postings) | `oml_recalls_total{pipeline="bm25-postings", access}` on the metrics listener, read where `OML_METRICS_URL` names it (never `/readyz`, which answers anyone), and, for an admin key, `GET /v2/search` `postings` | *Unreleased ([ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) d12).* `warn` when the scan answered more than 5% of `bm25-postings`'s recalls since the lead started, over at least 20; and, per tenant: no postings partition (every memory write for the tenant fails until the lead's next start creates and fills it); the fill incomplete (memory versions with no postings); served memories with no served postings; per-workarea counts or sampled word counts that disagree with the postings. Each fault line names the repair, the owner's `oml_search_postings_refill('<tenant id>')` ([schema](../data/postgres-schema.md#search-postings-adr-0052)); a clean read prints `postings complete; counts agree (<n> sampled word counts)` |

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
| all profiles | Logs are kept at most 30 days where they land — journald, the container log driver or the cluster's log pipeline, and the reverse proxy's access log ([log retention](#log-retention)): stated, not checkable — the lead holds no log file, and doctor's output says so rather than reporting `ok` ([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d8). |
| any profile with `oauth.issuer` set | A reverse proxy that writes access logs records neither the `Location` header of `POST /oauth/authorize` nor the request bodies of `POST /oauth/authorize` and `POST /oauth/token` ([HTTP API — behind a proxy](../api/http-api.md#connections-oauth-for-hosted-mcp-clients)): stated, not checkable — doctor cannot read a proxy's configuration, and its output says so rather than reporting `ok`. |

## Structured logging

*Available since v0.4.1 · [releases](../product/releases.md)*

Every binary logs JSON lines to **stderr**; stdout is reserved for the show-once enrollment
token. Each line is `tracing-subscriber`'s JSON format with the event's fields at the top level:
`timestamp` (RFC 3339, UTC), `level`, `target`, `event` (a stable snake_case name, not prose),
then the event's fields ([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d1). Before v0.4.1 the lead wrote the same events
as text `key=value` lines. A field whose value is `null` is left out of the line, which `jq`
reads as `null`; `idempotency_replay` is the one field that is `true` or absent. Correlation: a
request's log lines carry its `request_id` and `correlation_id`; lines about a journalled act
carry the journal `event_id`, so a log line can always be joined to the chain.

### The request line

One `http_request` line per HTTP request, written by one layer outside every route, at `info`,
or `error` for a 5xx. A probe answered `200` writes none ([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d1–d3):

| Field | Value |
|---|---|
| `request_id`, `request_id_source` | the inbound `X-Request-Id` when it passes the rule on the [HTTP API](../api/http-api.md#request-ids-correlation-and-trace-context), else one the lead mints; `caller`, `minted`, or `replaced` when one was sent and failed the rule, whose value is never logged. An honoured id is the caller's label: forgeable and not unique, so a reader joins on it with `key_id` and `timestamp` (ADR-0042 d4, d5) |
| `correlation_id`, `correlation_id_source` | the inbound `X-Correlation-Id` under the same rule, else the request id; `caller`, `request_id` or `replaced` (ADR-0042 d6) |
| `trace_id` | only while trace export is on (ADR-0042 d13) |
| `method`, `route`, `status` | the method, the matched route template (`/v2/keys/{key}`, never the raw path; `unmatched` where none matched), the status |
| `error_code` | the envelope's `error.code`, the RFC `error` on an OAuth endpoint, or `null` |
| `duration_ms` | request seen to response head, milliseconds to three decimals |
| `auth_ms`, `store_ms`, `store_calls` | time until the credential resolved or was refused; wall time and count of the request's store-port calls, pool acquisition and lock waits included (ADR-0042 d14). The other stages are not timed ([FR-70](../product/roadmap.md)) |
| `bytes_in`, `bytes_out` | body bytes read; response body length when exact-sized |
| `tenant_id`, `key_id`, `key_name`, `principal`, `role` | the resolved key's, `null` when none resolved |
| `credential`, `connection_id` | `bearer` or `connection`, and the connection's id; *since v0.5.0 ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d14):* `session` on the web pages, the key fields naming the session key the act ran as, and `connection_id` null |
| `web_action`, `web_provider` | *since v0.5.0 (ADR-0045 d14):* on a web route, the form's `action` from its page's closed set, and `google` or `github` on the sign-in and callback routes; absent elsewhere. A callback also writes one `web_sign_in` line with `provider`, `outcome`, `tenant_id` and the `user` when admitted, never the email, the provider subject, the code or `state`; its `outcome` is `signed_in`, a refusal's `details.reason`, `limit_rate` or `unavailable` |
| `workarea_id` | the resolved target workarea, by id |
| `search`, `search_fallback` | *since v0.5.5 (ADR-0049 d8):* on a recall, over HTTP or through `oml_read`, the search pipeline that ranked it, and `true` when `match: all` fell back to any word; absent elsewhere. Never the query |
| `idempotency_replay` | `true` on a replay, absent otherwise |
| `client_ip`, `client_ip_source` | the socket peer (`peer`), or, when the peer is in `server.trusted_proxies`, the first address from the right of `X-Forwarded-For` outside that list (`forwarded`) ([configuration](configuration.md), ADR-0042 d7) |
| `user_agent` | the `User-Agent` header, cut to 200 bytes, non-printable bytes replaced by `?`: `oml-cli/<version>`, `oml-cli/<version> hook/<name>`, `oml-sdk-python/<version>`, `oml-sdk-typescript/<version>`, or what another client sends (ADR-0042 d9) |

What the request line costs per request, and how many bytes it writes, is unmeasured
([FR-71](../product/roadmap.md)).

### The MCP call line

One `mcp_call` line per `/mcp` POST that carries a JSON-RPC message, written before that
request's `http_request` line and under its `request_id`, `correlation_id` and `trace_id`, with
`tenant_id`, `key_id`, `rpc_method`, `tool` (an unrecognized name logs `unknown`, a v0.3.0
tool name among them since v0.5.1), `mode`, `outcome`, `error_code`, `duration_ms` and, on `initialize` only, `client_name` and
`client_version` from `clientInfo`, cut and replaced as `user_agent` is. `rpc_method` is
`initialize`, `ping`, `tools/list`, `tools/call`, one of the four MCP notifications, or `unknown`.
A POST refused before its message is read, for its credential or its `MCP-Protocol-Version`,
writes its `http_request` line alone. Neither the JSON-RPC `id` nor any argument is logged
([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d10).

### Log retention

The request and MCP lines hold personal data: `client_ip`, `principal`, `key_name`,
`user_agent` and a caller's `request_id`. **Logs are kept at most 30 days.** The lead only
emits: it holds no log file and deletes nothing, so the bound is enforced where the lines land,
and the reverse proxy's access log falls under the same 30 days ([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d8):

- **systemd:** journald's `MaxRetentionSec=30day`, with `MaxFileSec=1day` so that journal files
  rotate daily and the bound is reached within a day. Both are host-wide journald settings.
- **Docker:** the default `json-file` driver rotates by size, never by age. Use the `journald`
  log driver, or a shipper with a 30-day policy.
- **Kubernetes:** the cluster's log pipeline; the kubelet's own rotation is by size.

Doctor cannot read any of these and states the rule as not checkable
([per-profile checks](#per-profile-checks)).

Built: the lead's hourly removal of the connections' lapsed rows (its start, then every hour;
[Postgres schema](../data/postgres-schema.md#connections-adr-0031)) writes `event`
`connection_sweep` with the count of each kind of row it removed, `registrations_removed`,
`connections_removed`, `tokens_removed`, `codes_cleared` and `authorizations_removed`, and the
`tenants` it swept; a sweep that fails writes `connection_sweep_failed` with its `reason`, stops
neither the lead nor the next sweep, and, repeated, means unused registrations accumulate toward
the 10 000 at which `POST /oauth/register` answers `429 limit.rate`
([HTTP API](../api/http-api.md#connections-oauth-for-hosted-mcp-clients);
[ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d5).

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
- **Attribution** — `tenant_id`, `key_id`, workarea node ids, and, on the
  request line, the key's name and principal and the client's address
  ([ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) d8 amends this bullet, which said never principal values,
  so a request line reads without a second lookup). These survive erasure by
  the stated boundary ([FR-1](../product/roadmap.md)), and the log keeps them
  at most 30 days ([log retention](#log-retention)); when FR-1 closes with a
  pseudonymization act, the log joins that act's inventory with that bound.
- **Never**: API key values (key ids only — the stored form is already a
  hash), DEK/KEK material or wrapped keys, serve payloads, scan excerpts that
  quote the body. A scanner finding is logged as rule id + offset range +
  digest, not as the matched text.
- **Never, for connections**: access and refresh tokens, authorization codes,
  PKCE code verifiers, authorization-request handles and connect codes, nor a
  digest of any of them — including the bodies of `POST /oauth/authorize` and
  `POST /oauth/token` and the `Location` header of the authorize redirect, whose
  query carries the authorization code. Ids only: `key_id`, `connection_id`,
  `code_id`, the authorization request's row id. The rule binds `oml-server`'s
  own log; a reverse proxy's access log is bound by the proxy requirement on
  the [HTTP API](../api/http-api.md#connections-oauth-for-hosted-mcp-clients)
  ([roles and keys](../architecture/roles-and-keys.md#connections),
  [ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d8, d10).

Under this rule logs contain no erasable content — that is the property the
rule buys. They do contain attribution, which is personal data, and so they
are kept at most 30 days ([log retention](#log-retention)); the content rule
is what lets that bound be a retention rule rather than an erasure act. The
cost is paid in debuggability: an engineer reproducing a scan or review issue
works from digests and the journalled record, never from a quoted body.

The read-event log is not logging. Who-read-what lives in `read_events`,
retention-bounded and digest-anchored to the journal
([retrieval](../architecture/retrieval.md)); application logs are operational
exhaust and carry no authority. An auditor gets the evidence export, never a
log archive.

## SLOs

SLIs are defined here over the exported metric names; **target percentages
are deployment policy and do not appear on this page** — a target is a choice
made against measured baselines, and no measurement of OML exists
([FR-25](../product/roadmap.md), [FR-2](../product/roadmap.md),
[FR-7](../product/roadmap.md)). What this section fixes is the formula: which
requests count, which count against, and where the latency targets come from.

- **Write availability.** Good events are write-route requests answered
  without a 5xx; the denominator is all write-route requests. Two refusal
  windows **count against this SLI, by design**: the fork-check hold-down
  (the deployment refusing writes because the stored chain head did not
  verify) and the lead-takeover window (writes refused until a new lead
  holds the lease and passes the fork check) — both surface as
  `system.unavailable` (503, [errors](../api/errors.md)). Excusing them
  would hide the availability-for-integrity trade
  [commitments](../product/commitments.md) §2 makes on purpose; counting
  them is that trade made visible in the one number operators watch. The
  takeover window's size is what [FR-24](../product/roadmap.md) measures.
- **Read availability.** The same formula over read routes. Followers keep
  reads flowing through a lead takeover ([deployment — the lead
  lease](deployment.md)), so in a deployment with followers a takeover
  should not dent this SLI — if it does, the single client-facing Service is
  not routing reads around the lead, which is itself a finding.
- **Latency.** Per-verb SLIs — submit, get, recall, supersede, attachment IO
  — as the fraction of requests completing under the verb's target, computed
  from the `oml_http_request_duration_seconds` histogram. The targets are
  [FR-25](../product/roadmap.md)'s declared per-ring targets
  ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d7), reused rather
  than restated: no latency number appears here, and no target may be met by
  weakening durability — the quorum floor is outside the optimizable budget.

Example expressions, Prometheus shape — `$WINDOW`, the route sets and every
`$…` value are deployment policy:

```
# write availability: non-5xx fraction on write routes
  sum(rate(oml_http_request_duration_seconds_count{route=~"$WRITE_ROUTES", status!~"5.."}[$WINDOW]))
/
  sum(rate(oml_http_request_duration_seconds_count{route=~"$WRITE_ROUTES"}[$WINDOW]))

# recall latency: fraction under the FR-25 recall target
  sum(rate(oml_http_request_duration_seconds_bucket{route="recall", le="$FR25_RECALL_TARGET"}[$WINDOW]))
/
  sum(rate(oml_http_request_duration_seconds_count{route="recall"}[$WINDOW]))
```

One honesty note the formulas need: a deployment that is fully down exports
nothing, so a metrics-only SLI overstates availability during total outages.
A blackbox probe against `/readyz` is the standard complement; its
configuration is a deployment-site choice like the rest of the scrape stack.

## Alerting starting points

Starting points, not a tuned alert book: the structural alerts (zero/nonzero,
boolean) are meaningful on day one; every threshold alert needs numbers this
design does not have yet and must be set against measured baselines
([FR-2](../product/roadmap.md), [FR-7](../product/roadmap.md)). Each
condition links its runbook — one anchored section per condition on the one
[runbooks](runbooks.md) page.

| Condition | Signal | Why it pages (or tickets) |
|---|---|---|
| [Fork check failing](runbooks.md#fork-check-failing) | `/readyz` red on all `oml-server` replicas after restart; `chain_head` failing in doctor | The deployment is refusing writes because the chain head did not verify — by design it stays down until an operator resolves it ([deployment](deployment.md)). Page. |
| [Review unavailable](runbooks.md#review-unavailable) | `review_availability: false` for a workarea | Admission fails closed; every write to that workarea will hold until expiry. Page if the workarea is in active use. |
| [Oldest held age approaching expiry](runbooks.md#oldest-held-approaching-expiry) | `oml_review_queue_oldest_age_seconds` vs. the policy's `held_item_expiry` | Items are about to dead-letter not because they are poison but because review capacity lagged — the failure mode [FR-2](../product/roadmap.md) exists to quantify. Alert ahead of the expiry, at a margin set locally. |
| [Dead letters present](runbooks.md#dead-letters-present) | `oml_dead_letter_count > 0`, or doctor's `dead_letter_count` | Every dead-lettered row is a journalled skip awaiting a human decision (re-enqueue or not). Ticket on first occurrence, page on growth. |
| [Frontier lag growing monotonically](runbooks.md#frontier-lag-growing) | `oml_outbox_frontier_lag_seconds` trend | Brief lag is normal (micro-batching, lease failover); monotonic growth means the lake-writer is down, conflicted, or poisoned. Alert on sustained growth, not on a fixed depth. |
| [Txn conflicts in steady state](runbooks.md#txn-conflicts-in-steady-state) | `oml_delta_txn_conflicts_total` increasing outside failover windows | Two writers are alive; the second fence is absorbing what the lease should have prevented. Ticket: investigate lease churn. |
| [Lease churn](runbooks.md#lease-churn) | `oml_leader_lease_transitions_total` rate | Predicts frontier-lag spikes; usually node or kubelet trouble. Ticket. |
| [Lead takeovers in steady state](runbooks.md#lead-churn) | `oml_lead_takeovers_total` rate outside deploy windows | Every takeover is a write-refusal window ([deployment — the lead lease](deployment.md)); churn means the lead keeps losing its lease. Ticket, page if writes are failing. |
| [RLS denials](runbooks.md#rls-denials) | `oml_rls_denied_total > 0` | Expected zero. A service bug or an isolation probe — either way, investigate ([security](../architecture/security.md)). Ticket, page on spike. |
| [Repair halted](runbooks.md#repair-halted) | `oml_reconcile_repair_halts > 0` | A finding recurred past its attempt budget; auto-repair stopped on purpose and raised an operator condition ([reconciliation](../architecture/reconciliation.md)). Ticket, with the artifact id from doctor. |
| [Reconciler stale](runbooks.md#reconciler-stale) | `oml_reconcile_last_success_timestamp_seconds` older than a few schedules | `concurrencyPolicy: Forbid` means slow passes queue up silently; staleness is how that surfaces. Ticket. |
| [Backup job failed](runbooks.md#backup-failed) | store-level backup success signal | A missed backup stretches no erasure window, but a *changed retention configuration* or failed expiry does — and a failed backup chain breaks the restore ceremony ([restore](restore.md)). Ticket. |
| [Sustained 429s for one tenant](runbooks.md#sustained-429s) | `oml_quota_rejections_total` rate | Either the tenant outgrew its journalled quota grant or the per-replica approximation is misbehaving under replica churn ([FR-8](../product/roadmap.md)). Ticket. |

What this page does not decide: the scrape/alert stack (Prometheus, the
managed equivalent, or none locally), dashboard layouts, and every numeric
threshold. Those are deployment-site choices and, for the thresholds,
measurement work the [roadmap](../product/roadmap.md) tracks.
