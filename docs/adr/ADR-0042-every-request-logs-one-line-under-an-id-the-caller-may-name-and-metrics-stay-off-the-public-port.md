<!--
id: adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port
tier: engineering
status: proposed · 2026-11-04
answers: what the lead logs for each HTTP request and each MCP call, in which format and with which fields; which inbound X-Request-Id, X-Correlation-Id and traceparent it accepts, what it returns, and what an honoured id may never be used for; where the client IP comes from behind a proxy; which fields are personal data, how long logs are kept and who enforces it; how /metrics is served without reaching the public internet and which metrics v0.4.1 builds; how OTLP trace export is turned on and what it carries; which per-stage timings a log line records and which stay unmeasured
-->

# ADR-0042 — Every request logs one line under an id the caller may name, metrics stay off the public port, and trace export is opt-in

**Status:** proposed · 2026-11-04

**Release:** planned for v0.4.1 ([releases](../product/releases.md#decisions))

## Context

The lead answers every request with an `X-Request-Id` it mints itself: each handler in
`crates/transports/oml-transport-http/src/lib.rs` calls `new_request_id()`, which returns
`req_` and 32 hex digits, and `finish()` sets the header and the error envelope's `request_id`
([errors](../api/errors.md)). Nothing reads an inbound id, and no request writes a log line. The
only per-request lines are `oauth_authorize` and `mcp_alias_called`; the hourly
`connection_sweep` is not a request. The process logs through `tracing_subscriber::fmt()` to
stderr as text, with ANSI colour only on a terminal (`crates/bins/oml-server-lead/src/main.rs`),
while [observability](../operations/observability.md#structured-logging) promises JSON lines to
stdout. It declares the `oml-server` metric names, and the binary emits none. The documented
proxy setup forwards the whole origin to the lead at `127.0.0.1:8080`
([deployment](../operations/deployment.md#connections-behind-a-reverse-proxy)), so every socket
peer the lead sees there is the proxy, and any route the lead serves is public.

Operators running v0.4.0 cannot answer three questions: which key or client sent the request
that failed, how long each route takes, and which caller's retries fill the log. On 2026-11-04 the
operator chose the sinks, chose to honour a caller's request id over minting one, named the fields
of origin and set log retention at 30 days. This record writes those choices down as decisions
and settles what they left open: the metrics listener, the validation rule, the proxy trust rule
and what the trace export carries. [ADR-0024](ADR-0024-performance-foundation.md) and
[D17](index.md#the-d-register-which-page-owns-which-decision) bind every number: nothing here is
measured, and no threshold is stated.

Decided, not built.

## Decision

- **d1 — The lead logs JSON lines on stderr, the whole process, with one `http_request` line per
  HTTP request.** One tower layer outside every route of
  [`into_router`](../api/http-api.md#two-rings), the probes, `/mcp` and the OAuth routes
  included, writes the line when the inner service returns the response head. Every line the
  process writes becomes `tracing-subscriber`'s JSON format with the event's fields flattened to
  the top level: `timestamp` (RFC 3339, UTC), `level`, `target`, then the fields. stdout stays
  reserved for the show-once enrollment token. The line is `info`, or `error` for a 5xx. A probe
  (`/healthz`, `/readyz`) answered `200` writes no line, so a kubelet polling every few seconds
  does not fill the log, and a probe answered `503` writes its line. This changes the format of
  every existing line: `mcp_alias_called`, `oauth_authorize` and `connection_sweep` keep their
  names and fields and become JSON, so a `grep` for the event name still finds them. The cost is
  that a person reading a terminal reads JSON, and `jq` becomes the reading tool. The
  `tracing-subscriber` `json` feature is pure Rust and adds `tracing-serde`; `serde_json` is
  already in the graph.

- **d2 — The `http_request` line carries the request's origin and outcome, and nothing a caller
  sends as content.** Its fields:

    | Field | Value |
    |---|---|
    | `event` | `http_request` |
    | `request_id`, `request_id_source` | d4 |
    | `correlation_id`, `correlation_id_source` | d6 |
    | `trace_id` | the span's trace id, only while trace export is on (d13) |
    | `method` | the HTTP method |
    | `route` | the route template (d3) |
    | `status` | the HTTP status |
    | `error_code` | the envelope's `error.code` ([errors](../api/errors.md)), the RFC `error` on an OAuth endpoint, or `null` |
    | `duration_ms` | from the layer seeing the request to the response head, in milliseconds to three decimals |
    | `auth_ms`, `store_ms`, `store_calls` | d14 |
    | `bytes_in` | request body bytes the server read, or `null` when it read none |
    | `bytes_out` | response body length when the body is exact-sized, as every response the lead serves today is, else `null` |
    | `tenant_id`, `key_id`, `key_name`, `principal`, `role` | the resolved key's; `null` when no key resolved, as on `auth.missing` |
    | `credential`, `connection_id` | `bearer` or `connection` ([ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d7), and the connection's id for a connection |
    | `workarea_id` | the workarea the handler resolved as its target, by id, never by name; `null` where none is resolved |
    | `idempotency_replay` | `true` on a stored-response replay, absent otherwise |
    | `client_ip`, `client_ip_source` | d7 |
    | `user_agent` | d9 |

    Handlers do not write the line. Each one attaches what it resolved, the key and the
    workarea, and the refusal's code, to the response as an extension, and the layer reads it.
    A handler that attaches nothing leaves those fields `null`, which a reader sees as missing,
    never as wrong; a router test asserts that every route of `ROUTES` attaches them. The line
    never carries a request or response body, a query string, recall text, a key value or its
    hash, a token, the `Authorization` or `Cookie` header, the `Location` header of
    `POST /oauth/authorize`, nor any other header than d9's (the plaintext rule of
    [observability](../operations/observability.md#the-plaintext-rule-a-correctness-rule),
    [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d8, d10).

- **d3 — `route` is the matched route template, never the raw path.** It is the template the
  router matched, `/v2/keys/{key}` for `GET /v2/keys/agent-7`, because a path segment can hold
  a name, and names are tenant data ([ADR-0026](ADR-0026-names-are-keys.md)). It is also a
  closed set, which the metric label needs (d12). A path no route matches logs `unmatched`, so a
  scanner probing random paths adds no label values and writes no attacker-chosen string into
  the log. The cost: the log cannot say which artifact or key a request named. The journal and
  the response say that, and the request id joins them.

- **d4 — An inbound `X-Request-Id` is honoured when it is valid; otherwise the lead mints one, and
  every response returns the id.** Valid means one header instance whose value is 1 to 128
  characters from `A`–`Z`, `a`–`z`, `0`–`9`, `-`, `_`, `.` and `:`. The rule admits UUIDs,
  ULIDs, `req_`-style ids and the hex ids proxies generate, and refuses spaces, quotes and
  control characters, so an honoured id cannot break a JSON line or forge a second one. A
  missing header, a second instance, or a value outside the rule is not refused: the lead mints
  `req_<32 hex>`, as today. The id is resolved once, in the layer, and stored in the request's
  extensions. Handlers read it there in place of calling `new_request_id()`, so the
  `X-Request-Id` header, the error envelope's `request_id`, the `request_id` an MCP tool
  refusal carries ([errors](../api/errors.md)) and every log line about the request are one
  value. The layer also sets the header on the responses no handler finishes, axum's own `404`
  for an unrouted path and `405` for a wrong method among them. `request_id_source` tells the
  cases apart: `caller` for an honoured id, `minted` when none was sent, `replaced` when one was
  sent and failed the rule. A replaced value is never logged.

- **d5 — An honoured id is a label the caller chose, never an authority.** Honouring it lets a
  client join its own logs to the lead's with no lookup, which is why the operator chose it. The
  cost is carried in the open. A caller can send another caller's id, so its lines look related.
  Two callers, or one careless client, can send the same id, so `request_id` is not unique in
  the log: a reader joins on `request_id` with `key_id` and `timestamp`, and
  `request_id_source` says when the id was the caller's. No decision reads the id. It is not an
  idempotency key, which stays `Idempotency-Key` scoped to the key
  ([HTTP API](../api/http-api.md#idempotency)), not a rate-limit or authorization input, not
  journalled and not stored in any table. The read log, not built, records its own server-side
  id when it is.

- **d6 — `X-Correlation-Id` follows the same rule, and defaults to the request id.** A client
  that groups several requests under one task id sends it as `X-Correlation-Id`, validated as
  in d4. Valid, it is honoured. Absent, or outside the rule, the correlation id is the request
  id. Every response returns `X-Correlation-Id`. `correlation_id_source` is `caller`,
  `request_id` or `replaced`. It carries d5's cost and d5's limits: it is a label, never an
  input to a decision.

- **d7 — The client IP comes from `X-Forwarded-For` only when the socket peer is a trusted
  proxy.** `server.trusted_proxies` lists addresses and CIDR ranges, empty by default
  ([configuration](../operations/configuration.md)). The lead serves with the socket's peer
  address (axum's `ConnectInfo`). When the peer is not in the list, `client_ip` is the peer and
  `client_ip_source` is `peer`; `X-Forwarded-For` is ignored, so a client that sets the header
  directly gains nothing. When the peer is in the list, the lead reads `X-Forwarded-For` right to
  left, skips every address in the list, and logs the first address outside it, with
  `client_ip_source` `forwarded`; a header that does not parse leaves the peer. The empty default
  costs a deployment behind a same-host proxy every client address, since its peer is always the
  proxy on `127.0.0.1`, until the operator sets `server.trusted_proxies = ["127.0.0.1/32", "::1/128"]`. Trusting
  loopback by default would let any local process on a shared host write the address it likes.
  An entry in the list that does not parse stops the process at start. Only `X-Forwarded-For` is read, because Caddy, nginx, Traefik and the Kubernetes ingress
  controllers set it; RFC 7239 `Forwarded` is not.

- **d8 — The log holds personal data, logs are kept at most 30 days, and the operator's log
  machinery enforces it.** `client_ip`, `principal`, `key_name`, `user_agent` and a caller's
  `request_id` can identify a person. This reverses one line of the attribution rule on
  [observability](../operations/observability.md#the-plaintext-rule-a-correctness-rule), which
  said a log carries key ids and never principal values: the operator wants a line read without
  a second lookup, and the trade is that the principal now sits in the log for the retention
  window. The plaintext rule does not move: no body, attachment, extracted text, query text or
  secret enters a log. The lead only emits. It holds no log file and deletes nothing, so the
  30 days is enforced where the lines land. Under systemd, journald's `MaxRetentionSec=30day`,
  with `MaxFileSec=1day` so that journal files rotate daily and the bound is reached within a
  day; the setting is journald's, for the whole host. Under Docker, the default `json-file`
  driver rotates by size and never by age, so an age bound needs the `journald` log driver or a
  shipper with a 30-day policy. In Kubernetes, the cluster's log pipeline. A reverse proxy's
  access log carries the same addresses and falls under the same 30 days. Doctor cannot read
  any of it and states the rule as not checkable, as it does the proxy rule of
  [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md). When
  [FR-1](../product/roadmap.md)'s pseudonymization act closes, the log joins its inventory with a
  30-day bound.

- **d9 — The client is named by its `User-Agent`, and OML's own clients send one.** The line's
  `user_agent` is the header's value, cut to 200 bytes, with every byte outside printable ASCII
  replaced by `?`. It is a label the caller chooses, like d4's id, and never an input to a
  decision. In v0.4.1 the CLI sends `oml-cli/<version>`, and `oml-cli/<version> hook/<name>`
  when it runs as one of the plugin's hooks (`session-start`, `tool-failure`, `session-end`),
  instead of ureq's default; the Python and TypeScript SDKs send `oml-sdk-python/<version>` and
  `oml-sdk-typescript/<version>`. The plugin's MCP entry is Claude Code's HTTP client, and what
  it sends is unobserved; the record names no value for it. Neither
  [ADR-0038](ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md),
  [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) nor
  [ADR-0040](ADR-0040-plugin-setup-in-claude-code.md) defines a harness name, and this record
  does not add a header for one: the User-Agent already travels on every request.

- **d10 — Each `/mcp` POST writes one `mcp_call` line under the HTTP request's ids, before its
  `http_request` line.** The facade takes one JSON-RPC message per POST and refuses batches
  (`crates/transports/oml-transport-http/src/mcp.rs`), so one call is one HTTP request. The
  `mcp_call` line carries `request_id`, `correlation_id` and `trace_id` as d4, d6 and d13 set
  them, `tenant_id` and `key_id` for filtering, and:

    | Field | Value |
    |---|---|
    | `rpc_method` | `initialize`, `ping`, `tools/list`, `tools/call`, a notification's method, or `unknown` for one the facade does not serve |
    | `tool` | for `tools/call`, the served name (`oml_write`, `oml_read`, `oml_workarea`), a v0.3.0 alias name, a designed tool's name, or `unknown`; an unrecognized caller-chosen name is never logged |
    | `alias_of` | the successor, for an alias; `mcp_alias_called` still logs as [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d12 says |
    | `mode` | `oml_write`'s `submit` or `supersede`, `oml_read`'s `get`, `recall` or `listing`; `null` for a tool without modes |
    | `outcome` | `result`, `tool_error` (`isError` set) or `rpc_error` |
    | `error_code` | the tool refusal envelope's `error.code`, or the JSON-RPC error code as a string |
    | `duration_ms` | the call's time inside the facade |
    | `client_name`, `client_version` | on `initialize` only, from `clientInfo`, cut and replaced as d9 does |

    The JSON-RPC `id` and the tool's arguments are never logged. The facade is stateless, so
    `clientInfo` from `initialize` cannot be joined to later calls; `user_agent` on the
    `http_request` line names the client on every call.

- **d11 — `/metrics` is served on a listener of its own, off unless `server.metrics_listen`
  names an address.** The API listener never routes `/metrics`, so a proxy that forwards the
  whole origin, as the documented Caddy site does, exposes nothing. Set, `server.metrics_listen`
  binds a second listener that serves `GET /metrics` in the Prometheus text exposition format,
  answers `404` on every other path, takes no credential and writes no request log line. Its
  traffic is not counted in d12's histogram either. Its protection is the address it binds: the
  documented values are `127.0.0.1:<port>` on a host where Prometheus runs locally, an
  unpublished container port in Docker, and the pod address in Kubernetes behind a
  NetworkPolicy that admits the scraper alone. A value equal to `server.listen`, or one that
  does not parse as an address, stops the process at start. The cost is a second port for the
  operator to firewall, and a deployment that binds it publicly publishes its per-tenant
  gauges, once built, to anyone who scrapes. The record accepts that, because the alternatives
  below each put the metrics on the public port.

- **d12 — v0.4.1 builds the request metrics the transport can count, and labels never carry a
  name, a principal or an address.** Built in v0.4.1, through the `prometheus-client` crate
  (the Prometheus project's Rust client):

    | Metric | Labels | Source |
    |---|---|---|
    | `oml_http_request_duration_seconds` | `route` (d3's template), `status` | every API-listener request but the probes answered `200`, which d1 also leaves out of the log |
    | `oml_mcp_call_duration_seconds` | `tool`, `mode`, `outcome` (d10's closed values) | every `/mcp` call |
    | `oml_idempotency_replays_total` / `oml_idempotency_conflicts_total` | none | a response carrying `Idempotency-Replay: true` / one refused in the `idempotency` family ([errors](../api/errors.md)) |
    | `oml_oauth_token_requests_total` | `grant_type`, `outcome` | `POST /oauth/token`, by RFC outcome |

    `oml_mcp_call_duration_seconds` is a new name, declared on
    [observability](../operations/observability.md#oml-server) in the same change. Every other
    declared metric stays designed, because each reads a queue, a lease or a component that
    either is not built or needs a Postgres query per scrape: the review-queue gauges, the chain
    append histogram (d14), the outbox, lease, quota, upload and RLS-denial series, the
    connect-code failure count, whose in-memory count doctor's `connect_code_failures` also
    waits for, and the client-metadata fetch counter. Every series of the four in v0.4.1 has
    labels from closed sets and no `tenant_id`: a tenant label waits for
    [FR-7](../product/roadmap.md)'s cardinality work. The histograms' bucket bounds start at
    1 ms and double sixteen times, to 32.768 s, plus `+Inf`. They are the instrument's
    resolution, not a target, and are revisited when [FR-25](../product/roadmap.md) produces the
    first measurements.

- **d13 — OTLP trace export is in v0.4.1 and off unless `server.otlp_endpoint` names a
  collector.** Set to an `http` or `https` base URL, the lead exports spans as OTLP over HTTP
  with protobuf to `<endpoint>/v1/traces`, through the `opentelemetry`, `opentelemetry_sdk`,
  `opentelemetry-otlp` and `tracing-opentelemetry` crates, all pure Rust and compiled into
  every release binary. A value that is not such a URL stops the process at start. Only OML's
  setting turns it on: the exporter is configured explicitly from the setting, the `OTEL_*`
  environment variables are not read, and a test asserts that `OTEL_EXPORTER_OTLP_ENDPOINT`
  alone exports nothing, since a second configuration surface is what
  [configuration](../operations/configuration.md) rules out. No authentication header is sent:
  the supported shape is a collector beside the lead, which forwards with its own credentials.
  - **Trace context.** A valid W3C `traceparent` (version `00`, a 32-hex trace id and a 16-hex
    parent id that are not all zeros, two hex flag digits) makes the request's span its child
    and honours its sampled flag. An invalid one starts a new trace, as Trace Context requires.
    `tracestate` is not read, and no response carries `traceparent`. With export off,
    `traceparent` is ignored and `trace_id` is absent from every line.
  - **Spans.** One server span per HTTP request, named by method and route template; one per
    MCP call; one per store-port call, named by the port method and never by SQL text or bound
    values. Span attributes are `http.request.method`, `http.route`,
    `http.response.status_code`, `oml.request_id`, `oml.correlation_id`, `oml.tenant_id`,
    `oml.key_id`, `oml.error_code`, `oml.mcp.tool` and `oml.mcp.mode`. The principal, key name,
    client IP and user agent are never exported: they leave the deployment only through the
    log, under d8's 30 days, and a collector's retention is outside it.
  - **Sampling and failure.** Parent-based: a root span is always sampled, and a child follows
    its parent's flag. A caller can therefore force or suppress sampling of its own requests,
    d5's cost again. Spans leave through the SDK's batch processor with a bounded queue. A full
    queue drops spans, and a request never waits on export. An export failure is logged as
    `otlp_export_failed` with the reason. What export costs per request is unmeasured
    ([FR-71](../product/roadmap.md)).

- **d14 — A log line records the time spent authenticating and in the store, and every other stage
  stays unmeasured.** `auth_ms` is the time from the request's arrival to the credential resolved
  or refused. `store_ms` and `store_calls` are the wall time and count of the request's calls
  into the Postgres adapter's store ports, summed in a task-local the layer installs. They
  include pool acquisition and, on a write, the chain-lock wait, because the adapter cannot
  separate them without timing inside each statement. Work a handler spawns onto another task
  is not counted. Both are an `Instant` and an addition per call, which is why they ship now.
  Deferred: the chain-lock wait apart from the insert, signing, the scan, pool acquisition apart
  from the query, response serialization, and the declared
  `oml_chain_append_duration_seconds` histogram. Each needs timing inside the domain or the
  adapter's transaction, and none of them is measured. They are tracked as
  [FR-70](../product/roadmap.md). No page states a threshold for any of these fields
  ([D17](index.md#the-d-register-which-page-owns-which-decision)).

- **d15 — The headers enter openapi.yaml in the change that serves them.**
  [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d9 keeps
  [openapi.yaml](../api/openapi.yaml) to what the lead routes. Until the layer is built, the
  inbound `X-Request-Id`, `X-Correlation-Id` and `traceparent` and the response
  `X-Correlation-Id` are declared on the [HTTP API](../api/http-api.md#request-ids-correlation-and-trace-context)
  page, marked planned. The building change adds them to `components.headers` and
  `components.parameters` and attaches them to every operation, in one commit with the code.

## Consequences

- **Pages:** [observability](../operations/observability.md) declares d1's format, d2's and d10's
  fields, d8's retention and the amended attribution rule, and marks which metrics v0.4.1
  builds. [Configuration](../operations/configuration.md) declares `server.trusted_proxies`,
  `server.metrics_listen` and `server.otlp_endpoint`. The [HTTP API](../api/http-api.md)
  declares the three inbound headers and the two it returns. [Releases](../product/releases.md)
  carries v0.4.1 as planned.
- **Build:** the layer, the request-id extension read by every handler, the response extension
  every handler attaches, `ConnectInfo` on the serve call, the JSON subscriber, the metrics
  listener, the exporter, the store-port timing, and the CLI and SDK `User-Agent` values. Tests,
  each named after the sentence it proves: an id inside the rule comes back unchanged; one
  outside it is replaced and not logged; a header from an untrusted peer leaves `client_ip` the
  peer; `/metrics` answers `404` on the API listener; `OTEL_EXPORTER_OTLP_ENDPOINT` alone
  exports nothing; no log line contains a request body, the bearer value or the authorize
  `Location`.
- **Dependencies:** `tracing-subscriber`'s `json` feature, `prometheus-client`, and the four
  OpenTelemetry crates join the graph and pass `deny.toml` before the build lands. The
  OpenTelemetry crates release together and `tracing-opentelemetry` trails them, so an upgrade
  moves all of them at once.
- **Operators:** behind a proxy, `server.trusted_proxies` must be set, or every line logs the
  proxy's address. The log's retention is set where it lands (d8). Upgrading to v0.4.1 changes
  the log format to JSON.
- **Left open:** how the Helm chart ([ADR-0033](ADR-0033-interim-helm-chart.md)) exposes the
  metrics port, a scrape annotation or a ServiceMonitor, is decided in the change that wires it.
  Axum's `404` for an unrouted path is not the error envelope [errors](../api/errors.md)
  requires; d4 gives it the header, and the body is not decided here.
- **Opened:** [FR-70](../product/roadmap.md) (the stages d14 does not time, and the chain append
  histogram) and [FR-71](../product/roadmap.md) (what the request log, the metrics and trace
  export cost per request, and how many bytes the log writes per request, which is what 30 days
  of it occupies).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Mint every request id on the server, ignoring the caller's** | It was the operator's other option. It makes the id unforgeable and unique, but a client must then read the response header and log it to join its own records, and a proxy's id is lost. The operator chose the join. d5 bounds what an honoured id can do. |
| **Refuse a request whose `X-Request-Id` fails the rule** | A tracing header would then be able to fail a write. Replacing the id keeps the request and tells the log (`replaced`). |
| **Serve `/metrics` on the API port, behind an operator key** | The scrape configuration then holds a long-lived operator key, and the endpoint is reachable from the internet through any proxy that forwards the whole origin. A bug in its check would publish every tenant's gauges. |
| **Serve `/metrics` on the API port, only to loopback peers** | Behind a proxy on the same host every request comes from loopback, so the check admits the internet. That is the documented Caddy shape. |
| **Push metrics through OTLP and serve no `/metrics`** | It makes a collector mandatory for the one signal the SLO formulas on [observability](../operations/observability.md#slos) are written over, and Prometheus scrapes without one. |
| **Log the raw path and query** | Paths carry key and workarea names and queries carry cursors and filters: tenant data, and an unbounded label set. |
| **Log the workarea and key by name only** | A name is readable but is tenant data, and an id joins to the journal. The key name is logged beside its id because the operator asked for it; the workarea is logged by id. |
| **Trust loopback, or every private range, as a proxy by default** | Any local process, or any host on the private network, could then write the client address it likes into a log that holds personal data. |
| **Read RFC 7239 `Forwarded` as well** | The proxies OML documents set `X-Forwarded-For`. A second header doubles the parsing and the precedence rules for no deployment that needs it. |
| **Export the principal, key name and client IP on spans** | A collector's retention and location are outside d8's 30 days and outside the deployment. The span carries ids, and the log holds the personal fields. |
| **Read the `OTEL_*` environment variables** | They would be a second configuration surface, which [configuration](../operations/configuration.md) refuses, and a stray variable in a container image could start exporting. |
| **A cargo feature that leaves OpenTelemetry out of the default build** | The operator asked for export in the release. A binary built without it cannot turn it on, and a second binary doubles the release matrix. |
| **A setting for the log format, text or JSON** | Two formats are two sets of tests and two parsers downstream. The process has one consumer shape: a pipe into journald, Docker or a collector. |
| **Time every stage now: chain lock, signing, scan, serialization** | Each needs instrumentation inside the domain or inside the adapter's transaction, and none has a measured cost to justify it yet. FR-70 holds the work. |
