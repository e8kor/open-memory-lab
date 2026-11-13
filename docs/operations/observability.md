---
status: Stable
verified: 2026-11-06
answers: "how to turn on, scrape and read an OML server's metrics, request log and traces, what oml doctor checks, and which alerts to set"
---

# Monitor an OML server

Use this page to watch a running server. It covers the metrics listener, the request log, trace export, `oml doctor` and three alerts. For the metric names and labels, see [Metrics](../reference/metrics.md). For every setting, see [configuration](configuration.md).

## Before starting

- A running server with an operator or admin key.
- A Prometheus-compatible scraper that can reach the host.

## Turn on the metrics listener

The metrics listener is off until it is given an address. It serves `GET /metrics` and nothing else, without authentication. The API listener never routes `/metrics`.

1. Set `server.metrics_listen` (environment `OML_SERVER__METRICS_LISTEN`) to an address only the scraper reaches, for example `127.0.0.1:9464`.
2. Restart the server.
3. Fetch the listener to check it.

```bash
curl -s http://127.0.0.1:9464/metrics | head
```

4. Add the address as a scrape target. The format is OpenMetrics text, `application/openmetrics-text; version=1.0.0`, which Prometheus reads.

The lake writer has a listener of its own. It defaults to `127.0.0.1:9466` and is set by `OML_LAKE_METRICS_ADDR`. See [the lake](lake.md).

To let `oml doctor` read rerank, scan and recall-telemetry counts, set `OML_METRICS_URL` to the lead's listener when it runs.

```bash
OML_METRICS_URL=http://127.0.0.1:9464 oml doctor
```

## Read the request log

Every OML binary writes JSON lines to stderr. Each line has `timestamp`, `level`, `target`, `event` and the event's fields. Stdout is reserved for the show-once enrollment token.

The server writes one `http_request` line per request, except probes answered `200`. The fields read most often:

| Field | Meaning |
|---|---|
| `request_id`, `correlation_id` | The inbound `X-Request-Id` and `X-Correlation-Id` when they pass the rule on the [HTTP API](../api/http-api.md), else ones the server mints. |
| `method`, `route`, `status` | The method, the matched route template and the status. |
| `error_code` | The envelope's `error.code`, or `null`. |
| `duration_ms` | Time from request to response head. |
| `tenant_id`, `key_id`, `key_name`, `role` | The resolved key's, or `null` when none resolved. |
| `workarea_id` | The target workarea, by id. |
| `client_ip`, `client_ip_source` | The socket peer, or the first address from the right of `X-Forwarded-For` outside `server.trusted_proxies`. |
| `trace_id` | Present only while trace export is on. |

A `/mcp` POST that carries a JSON-RPC message also writes an `mcp_call` line, with `rpc_method`, `tool`, `mode`, `outcome` and `duration_ms`. It comes before the `http_request` line and shares its `request_id`.

Find the slow or failing calls with `jq`:

```bash
journalctl -u oml-server -o cat | jq -c 'select(.event=="http_request" and .status>=500)'
```

### Read the usage rollup

The lead's hourly usage rollup (at its start, then every hour) writes `event` `usage_rollup` with the `tenants` it rolled up and the ledger `rows` it appended. A pass that fails writes `usage_rollup_failed` with its `reason`, stops neither the lead nor the next pass, and the next pass appends the closed days it missed, up to 62 days back ([ADR-0071](../adr/ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d4). No account is ever a metric label: the per-account record is the ledger.

A recall hands its count to a queue and never waits for the write. Counts the queue dropped because it was full, or that the store refused, write `event` `usage_recall_counts_lost` with their `count` and the `reason`, and add to `oml_usage_recall_counts_lost_total` on the metrics listener. The month's `recalls` meter then reads low by that count, and nothing recomputes it (ADR-0071 d8). A graceful stop (`SIGTERM`, `SIGINT`) writes the counts the queue still holds after the last request is answered, waiting up to five seconds for the writer; counts it has to leave are logged and counted the same way, those a full queue had dropped and those the store refused earlier in the pass in flight included, so the counts told as lost over the run are at least what was accepted and never written: a writer still mid-write at the stop can commit rows the stop has already told as lost, so the total can read high by those and never low. A killed process loses what its queue held, and logs nothing.

### What logs never hold

No log line holds a memory body, a recall query, an API key, a token or an authorization code. Logs carry ids and digests. They do hold `client_ip`, `key_name` and `user_agent`, which are personal data.

Keep logs at most 30 days where they land. The server holds no log file and deletes nothing. `oml doctor` cannot check this.

- **systemd:** set journald `MaxRetentionSec=30day` and `MaxFileSec=1day`. Both are host-wide.
- **Docker:** the default `json-file` driver rotates by size, not age. Use the `journald` driver or a shipper with a 30-day policy.
- **Kubernetes:** set the retention in the cluster's log pipeline. The kubelet rotates by size.

Make the reverse proxy omit the `Location` header of `POST /oauth/authorize` and the request bodies of `POST /oauth/authorize` and `POST /oauth/token` from its access log. They carry an authorization code.

## Export traces

Trace export is off until `server.otlp_endpoint` is set (environment `OML_SERVER__OTLP_ENDPOINT`) to the base URL of an OpenTelemetry collector. The server posts spans to `<endpoint>/v1/traces` over OTLP with protobuf. It sends no authentication header. Once export is on, request log lines carry `trace_id`.

## Run `oml doctor`

`oml doctor` prints each health signal with a verdict and exits 1 on any failure. It reads through the HTTP API, so it works with no Prometheus at all. When the metrics and the doctor disagree, trust the doctor.

```bash
oml doctor
oml doctor --json
```

It checks, among other things:

- the key, the URL and the server's readiness;
- whether each workarea has enough reviewer keys to approve what authors write;
- held memories that cannot be reviewed, and in-progress workareas with unreviewed memories;
- search: the pipeline, reranker skips, and postings completeness;
- the scan rule set and findings since start;
- usage, with an admin key: accounts whose counters differ from their rows, and accounts on a profile the configuration does not declare;
- approvals, with an admin key: served versions with no approval (`approval_unmatched`);
- sign-in providers and OAuth discovery when `oauth.issuer` is set;
- recall telemetry: dropped events, queue depth and a purge that is behind.

Run it after every upgrade and when a user reports a problem. For what to do with a failing line, see [troubleshooting the server](../help/troubleshooting-server.md).

## Set three alerts

OML ships no thresholds, because no baseline is measured yet. These three work without one. Replace `$WINDOW` and the values with the deployment's own.

**1. The API returns 5xx.**

```
sum(rate(oml_http_request_duration_seconds_count{status=~"5.."}[$WINDOW]))
/
sum(rate(oml_http_request_duration_seconds_count[$WINDOW]))
```

Page when the ratio stays above the error budget. Two causes are expected: a lead takeover and a failed fork check both refuse writes with `system.unavailable` (503). See [operational alerts](runbooks.md).

**2. The lake falls behind.** Skip this one without a lake writer.

```
oml_lake_lag_seconds > $SECONDS
```

Brief lag is normal while a batch commits. Alert on sustained growth. A rising `oml_lake_commit_failures_total` or `oml_lake_dead_letters_total` says why.

**3. Nobody is reviewing.** Held memories wait for a reviewer who is not their author.

```
histogram_quantile(0.95, sum(rate(oml_review_wait_seconds_bucket[1d])) by (le))
```

Alert when the 95th percentile of the wait passes the time promised to the agents. Then run `oml review list` to see what is held.

A fully down server exports nothing, so a metrics-only alert misses a total outage. Add a blackbox probe against `/readyz`. For service-level targets, see [operating](operating.md).

## First-party web traffic

OML ships `docs/operations/dashboards/oml-web-traffic.json`, a first-party template that imports into Grafana with the Prometheus datasource scraping `server.metrics_listen`.

It uses:

- `oml_http_request_duration_seconds_count` for request rate, status outcomes, and latency;
- `oml_web_exchanges_failed_total` for sign-in provider failures.

Queries filter matched web routes. The metrics exposed use closed labels only: route and status. There are no tenant, account, key, or address labels, so this is aggregate traffic rather than visitor identification.

Example route filter:

`route=~"^/web"`
