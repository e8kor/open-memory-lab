---
status: Stable
verified: 2026-11-06
answers: "which Prometheus metrics the OML server and the lake writer emit, their types, labels and what each counts"
---

# Metrics

The lead serves these metrics on a second listener, and the lake writer serves its own. To turn them on, see [Monitor an OML server](../operations/observability.md).

Every name starts with `oml_`. Counters end in `_total` in the scrape. Every label value comes from a closed set. No label carries a tenant, a key, a principal, an address or a query.

Histograms use bucket bounds that start at 1 ms and double sixteen times, to 32.768 s. `oml_review_wait_seconds` starts at one minute and doubles to about 23 days.

## Lead (`server.metrics_listen`)

### Requests

| Metric | Type | Labels | Counts |
|---|---|---|---|
| `oml_handler_panics_total` | counter | none | Requests whose handler panicked and were answered `500 system.internal` with `details.reason` `handler_panic` by the panic guard inside the traffic layer; the lead's log carries one `handler_panic` line per panic with its request id and text, and the worker serves the next request. Every one is a defect; doctor's `handlers` line warns on any. |
| `oml_http_request_duration_seconds` | histogram | `route`, `status` | API requests. `route` is the matched route template or `unmatched`. Probes answered `200` are not counted. |
| `oml_mcp_call_duration_seconds` | histogram | `tool`, `mode`, `outcome` | Calls on `/mcp`. `outcome` is `result`, `tool_error` or `rpc_error`. `mode` is empty for a tool without modes. |
| `oml_idempotency_replays_total` | counter | none | Responses that carried `Idempotency-Replay: true`. |
| `oml_idempotency_conflicts_total` | counter | none | Requests refused in the idempotency error family. |
| `oml_idempotency_unsettled_total` | counter | none | Web console acts and key issues (`POST /v2/keys`, `POST /v2/keys/{key}/connect-codes`) that left their `Idempotency-Key` claimed and unrecorded: the act committed and its outcome could not be written, a step after its commit failed, it stopped mid-act, or a refused act's claim could not be released. A resubmission under that key answers `409 idempotency.in_progress` and acts nothing. The lead's log carries one `idempotency_unsettled` line per act, naming the act, the key id and the claim id; doctor's `handlers` line warns on any. |
| `oml_oauth_token_requests_total` | counter | `grant_type`, `outcome` | `POST /oauth/token` calls by RFC outcome. A rise in `invalid_grant` on `refresh_token` means lost refresh answers or stolen tokens. |
| `oml_web_exchanges_failed_total` | counter | `provider`, `reason` | Failed Google or GitHub sign-in exchanges. |

### Review and workareas

| Metric | Type | Labels | Counts |
|---|---|---|---|
| `oml_review_wait_seconds` | histogram | `verdict` | Time from a memory's submission to its verdict. The count by `verdict` gives the rejection rate. |
| `oml_review_bulk_items_total` | counter | `outcome` | Items in bulk verdicts: `approved`, `rejected` or `skipped`. |
| `oml_recall_unreviewed_hits_total` | counter | none | Recall hits labeled `unreviewed`. |
| `oml_workarea_mode_changes_total` | counter | `mode` | Workarea modes set by a person's key. `standard` on an in-progress root is its commit. |
| `oml_recall_sources_total` | counter | `mode` | Recalls that searched beyond their scope, by `sources` mode: `wide` or `list` ([ADR-0072](../adr/ADR-0072-recall-searches-the-trusted-workareas-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md)). |
| `oml_recall_federated_hits_total` | counter | none | Recall hits served from a federated source, outside the recall's scope. |
| `oml_curation_acts_total` | counter | `act` | Acts by curator keys inside in-progress workareas: `reject`, `retire`, `flag` or `write`. |
| `oml_source_checks_total` | counter | `outcome` | Source checks recorded: `fresh`, `equivalent`, `moved`, `changed` or `removed`. |
| `oml_scan_findings_total` | counter | `class`, `rule`, `action` | Scan findings since start. `action` is `veto` or `hold`. Counts only, never a matched value. |

### Writes

| Metric | Type | Labels | Counts |
|---|---|---|---|
| `oml_remember_duplicates_total` | counter | `relation` | Writes whose text a served memory in the workarea already holds (`same_text`) or does not (`none`). |
| `oml_remember_similar_total` | counter | `relation`, `on_similar` | Writes the similar check ran for. `relation` is `none`, `same_text` or `near_copy`. `on_similar` is `hold` or `refuse`. |
| `oml_remember_similar_duration_seconds` | histogram | none | Time of the similar check. |

### Recall

| Metric | Type | Labels | Counts |
|---|---|---|---|
| `oml_recalls_total` | counter | `pipeline`, `fallback`, `access` | Recalls by the search pipeline that ranked them. `fallback` marks a `match=all` recall that fell back to any word. `access` says whether postings or a scan answered. |
| `oml_recall_answers_total` | counter | `pipeline`, `result` | Recall answers: `hits`, `empty`, or `below_floor` when the relevance floor withheld a row. |
| `oml_recall_duration_seconds` | histogram | `pipeline` | Time the app spent answering a recall. |
| `oml_recall_stage_duration_seconds` | histogram | `pipeline`, `stage` | Time in each stage: `scope`, `retrieve_<retriever>`, `fuse`, `rerank`, `suggest`, `hydrate`, `cut`. Only the stages a served pipeline runs have series. |
| `oml_recall_hits_by_origin_total` | counter | `origin_kind` | Recall hits by the origin kind of the version. |
| `oml_reranks_total` | counter | `outcome`, `reason` | Reranking stages: `applied` or `skipped`, with the reason `none`, `busy`, `timeout` or `failed`. |
| `oml_sparse_encoded_total` | counter | `outcome` | Versions the learned sparse encoder wrote postings for, failed on, or parked as dead letters. |
| `oml_sparse_backlog` | gauge | none | Versions waiting for the sparse encoder, across the deployment. |
| `oml_memories_unrecalled` | gauge | `promoted_for` | Served memories no recall in the last 30 days returned, by time since approval (`7d`, `30d`, `90d`, `older`). Refreshed every 10 minutes. |

### Recall telemetry

| Metric | Type | Labels | Counts |
|---|---|---|---|
| `oml_recall_outcomes_total` | counter | `label` | What a recall's reader did next: `get`, `derived`, `superseded`, `feedback_useful`, `feedback_not_useful`, `requery` or `no_use`. |
| `oml_recall_events_total` | counter | `outcome` | Recall events `written`, or dropped as `dropped_queue_full`, `dropped_store_error` or `dropped_no_partition`. A drop never changes a recall's answer. |
| `oml_recall_labels_dropped_total` | counter | `reason` | Labels not stored: `rate_limited`, `queue_full` or `store_error`. |
| `oml_telemetry_queue_depth` | gauge | none | Items waiting for the telemetry writer. |
| `oml_labeller_lag_seconds` | gauge | none | Seconds since the labeller's last completed pass. |
| `oml_telemetry_purge_overdue` | gauge | none | Tenants holding telemetry rows older than 30 days and a minute. |

## Lake writer (`OML_LAKE_METRICS_ADDR`)

The writer listens on `127.0.0.1:9466` by default. See [the lake](../operations/lake.md).

| Metric | Type | Labels | Counts |
|---|---|---|---|
| `oml_lake_frontier_seq` | gauge | none | The outbox sequence the lake has applied through. |
| `oml_lake_horizon_seq` | gauge | none | The outbox sequence of the stable prefix at the last cut. |
| `oml_lake_lag_rows` | gauge | none | Outbox sequences past the frontier. |
| `oml_lake_lag_seconds` | gauge | none | Age of the oldest outbox row past the frontier. |
| `oml_lake_batches_total` | counter | none | Batches committed. |
| `oml_lake_rows_total` | counter | `table` | Rows committed. |
| `oml_lake_commit_failures_total` | counter | `table` | Failed lake commits. |
| `oml_lake_dead_letters_total` | counter | none | Outbox rows a table skipped. |
| `oml_lake_maintenance_runs_total` | counter | `job`, `result` | Maintenance runs. |
| `oml_lake_maintenance_files_total` | counter | `kind` | Files compacted, written and vacuumed. |
| `oml_lake_batch_millis` | gauge | none | Wall time of the last batch, in milliseconds. |
