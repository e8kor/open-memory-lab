//! The traffic layer: one tower layer outside every route of
//! [`crate::ApiState::into_router`], the probes, `/mcp` and the OAuth routes
//! included, and the routes no handler finishes. It resolves the request's
//! ids (ADR-0042 d4–d6), installs the task-local [`Scope`] the app's
//! observer and the store port's timing write into (d14), and writes one
//! `http_request` line when the inner service returns the response head
//! (d1–d3, d7, d9).

pub mod ids;
pub mod metrics;
pub mod proxies;

use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes, HttpBody};
use axum::extract::{ConnectInfo, MatchedPath, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use http_body::{Frame, SizeHint};
use tracing::Instrument as _;
use uuid::Uuid;

use oml_app::ports::RequestObserver;
use oml_app::types::KeyRecord;

pub use ids::{CORRELATION_ID, REQUEST_ID, RequestIds, Rid};
pub use metrics::Metrics;
pub use proxies::{ClientIpSource, TrustedProxies};

use crate::PROBES;

/// The stable `event` of the request line (observability.md, the request
/// line; ADR-0042 d1).
pub const EVENT_HTTP_REQUEST: &str = "http_request";

/// The `route` of a request no route matched: a closed value, so a scanner
/// probing random paths writes no string of its choosing (ADR-0042 d3).
pub const UNMATCHED: &str = "unmatched";

/// The longest `User-Agent` the line keeps, in bytes (ADR-0042 d9).
pub const USER_AGENT_MAX: usize = 200;

tokio::task_local! {
    static SCOPE: Arc<Scope>;
}

/// What a request resolved and spent, filled while it runs: the app's
/// observer writes the key and the workarea, the store port's timing its
/// calls (ADR-0042 d2, d14). Work spawned onto another task is not seen.
#[derive(Debug)]
pub struct Scope {
    started: Instant,
    /// The request span's trace id, while trace export is on (ADR-0042 d13).
    trace_id: Option<String>,
    observed: Mutex<Observed>,
}

#[derive(Debug, Default, Clone)]
struct Observed {
    auth: Option<Duration>,
    identity: Option<Identity>,
    workarea_id: Option<Uuid>,
    store: Duration,
    store_calls: u64,
    /// What a recall's reranking stage did, and why when it was skipped
    /// (ADR-0053 d9).
    rerank: Option<(&'static str, Option<&'static str>)>,
    /// The search pipeline a recall ran, whether `match: all` fell back,
    /// and whether postings or a scan answered (ADR-0049 d4; ADR-0052 d11).
    search: Option<(&'static str, bool, &'static str)>,
    /// A recall's answer shape and its seconds, by pipeline (ADR-0054 d1).
    answered: Option<(&'static str, &'static str, f64)>,
    /// Each served hit's origin kind (ADR-0063 d13).
    origins: Vec<&'static str>,
    /// A verdict and the seconds its memory waited for it (ADR-0054 d1).
    verdict: Option<(&'static str, f64)>,
    /// Hits served labelled `unreviewed`, a mode set and a curator act
    /// (ADR-0030 d5).
    unreviewed_hits: u64,
    mode_set: Option<&'static str>,
    curation: Option<&'static str>,
    /// A write's similar check: the best relation named, `on_similar`, and
    /// its seconds (ADR-0050 d1; ADR-0054 d1).
    similar: Option<(&'static str, &'static str, f64)>,
    /// A recall's stages: pipeline, stage and seconds (ADR-0049 d14).
    stages: Vec<(&'static str, &'static str, f64)>,
}

impl Scope {
    fn new(started: Instant, trace_id: Option<String>) -> Self {
        Self {
            started,
            trace_id,
            observed: Mutex::new(Observed::default()),
        }
    }

    fn observed(&self) -> Observed {
        self.observed.lock().map(|o| o.clone()).unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut Observed, Duration)) {
        if let Ok(mut observed) = self.observed.lock() {
            f(&mut observed, self.started.elapsed());
        }
    }
}

/// The running request's trace id, while trace export is on: every line
/// about the request carries it (ADR-0042 d13).
#[must_use]
pub fn trace_id() -> Option<String> {
    SCOPE.try_with(|s| s.trace_id.clone()).ok().flatten()
}

fn in_scope(f: impl FnOnce(&Scope)) {
    let _outside_a_request = SCOPE.try_with(|scope| f(scope));
}

/// The resolved key's fields the request line carries; never its value or
/// its hash (ADR-0042 d2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub tenant_id: Uuid,
    pub key_id: Uuid,
    pub key_name: Option<String>,
    pub principal: String,
    pub role: &'static str,
    pub credential: &'static str,
    pub connection_id: Option<Uuid>,
}

impl Identity {
    fn of(key: &KeyRecord, connection_id: Option<Uuid>) -> Self {
        Self {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            key_name: key.name.as_ref().map(|n| n.as_str().to_owned()),
            principal: key.principal.clone(),
            role: key.role.as_str(),
            credential: key.credential.as_str(),
            connection_id,
        }
    }
}

/// A credential resolved to `key`, or, with `None`, to no key: the first
/// resolution of the request sets `auth_ms` and the identity fields
/// (ADR-0042 d2, d14).
pub fn credential(key: Option<&KeyRecord>, connection_id: Option<Uuid>) {
    in_scope(|scope| {
        scope.update(|o, at| {
            if o.auth.is_none() {
                o.auth = Some(at);
                o.identity = key.map(|k| Identity::of(k, connection_id));
            }
        });
    });
}

/// The request's verb targets `workarea_id`; the first one set is logged.
pub fn workarea(workarea_id: Uuid) {
    in_scope(|scope| scope.update(|o, _| _ = o.workarea_id.get_or_insert(workarea_id)));
}

/// The request's recall ran `pipeline`; `fallback` when `match: all` fell
/// back to any term; `access`, `postings` or `scan` (ADR-0049 d4, d5;
/// ADR-0052 d11).
pub fn search(pipeline: &'static str, fallback: bool, access: &'static str) {
    in_scope(|scope| scope.update(|o, _| o.search = Some((pipeline, fallback, access))));
}

/// The request's recall reranked (`applied`) or served the first stage's
/// order (`skipped`, with the reason) (ADR-0053 d9).
pub fn rerank(outcome: &'static str, reason: Option<&'static str>) {
    in_scope(|scope| scope.update(|o, _| o.rerank = Some((outcome, reason))));
}

/// The request's recall through `pipeline` answered `result` in `seconds`
/// (ADR-0054 d1).
pub fn recall_answered(pipeline: &'static str, result: &'static str, seconds: f64) {
    in_scope(|scope| scope.update(|o, _| o.answered = Some((pipeline, result, seconds))));
}

/// The request's recall served hits of these origin kinds (ADR-0063 d13).
pub fn recall_hits_by_origin(kinds: &[&'static str]) {
    in_scope(|scope| scope.update(|o, _| o.origins.extend_from_slice(kinds)));
}

/// The request's write ran the similar check (ADR-0050 d1).
pub fn similar_checked(relation: &'static str, on_similar: &'static str, seconds: f64) {
    in_scope(|scope| scope.update(|o, _| o.similar = Some((relation, on_similar, seconds))));
}

/// The request's recall spent `seconds` in `stage` (ADR-0049 d14).
pub fn recall_stage(pipeline: &'static str, stage: &'static str, seconds: f64) {
    in_scope(|scope| scope.update(|o, _| o.stages.push((pipeline, stage, seconds))));
}

/// The request's verdict committed `waited` seconds after its memory was
/// submitted (ADR-0054 d1).
pub fn verdict(verdict: &'static str, waited: f64) {
    in_scope(|scope| scope.update(|o, _| o.verdict = Some((verdict, waited))));
}

/// One call into the store port took `elapsed`: summed into the request's
/// `store_ms` and counted in `store_calls` (ADR-0042 d14). Outside a
/// request, as the startup and the hourly sweep are, nothing is recorded.
pub fn store_call(elapsed: Duration) {
    in_scope(|scope| {
        scope.update(|o, _| {
            o.store += elapsed;
            o.store_calls += 1;
        });
    });
}

/// The app's [`RequestObserver`] that writes into the running request's
/// [`Scope`]; a composition hands it to `App::with_observer`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScopeObserver;

impl RequestObserver for ScopeObserver {
    fn credential(&self, key: Option<&KeyRecord>, connection: Option<Uuid>) {
        credential(key, connection);
    }

    fn workarea(&self, workarea_id: Uuid) {
        workarea(workarea_id);
    }

    fn search(&self, pipeline: &'static str, fallback: bool, access: &'static str) {
        search(pipeline, fallback, access);
    }

    fn rerank(&self, outcome: Option<&'static str>, reason: Option<&'static str>) {
        if let Some(outcome) = outcome {
            rerank(outcome, reason);
        }
    }

    fn recall_answered(&self, pipeline: &'static str, result: &'static str, seconds: f64) {
        recall_answered(pipeline, result, seconds);
    }

    fn recall_hits_by_origin(&self, kinds: &[&'static str]) {
        recall_hits_by_origin(kinds);
    }

    fn verdict(&self, verdict: &'static str, waited_seconds: f64) {
        self::verdict(verdict, waited_seconds);
    }

    fn unreviewed_hits(&self, n: u64) {
        in_scope(|scope| scope.update(|o, _| o.unreviewed_hits += n));
    }

    fn mode_set(&self, mode: &'static str) {
        in_scope(|scope| scope.update(|o, _| o.mode_set = Some(mode)));
    }

    fn curation(&self, act: &'static str) {
        in_scope(|scope| scope.update(|o, _| o.curation = Some(act)));
    }

    fn similar_checked(&self, relation: &'static str, on_similar: &'static str, seconds: f64) {
        similar_checked(relation, on_similar, seconds);
    }

    fn recall_stage(&self, pipeline: &'static str, stage: &'static str, seconds: f64) {
        recall_stage(pipeline, stage, seconds);
    }
}

/// What a handler resolved, attached to its response as an extension for
/// the layer to read: the key, the target workarea and the refusal's code.
/// A handler that attaches nothing leaves those fields out of the line,
/// which a reader sees as missing, never as wrong (ADR-0042 d2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    pub identity: Option<Identity>,
    pub workarea_id: Option<Uuid>,
    /// The envelope's `error.code`, or the RFC `error` on an OAuth endpoint.
    pub error_code: Option<String>,
    /// `POST /oauth/token`'s `grant_type`, as the token metric labels it.
    pub grant_type: Option<&'static str>,
    /// A web route's session, action and provider (ADR-0045 d14).
    pub web: Option<WebLine>,
}

/// What a web route adds to the request line (ADR-0045 d14): whether a
/// session resolved, which makes `credential` `session`, its tenant, the
/// form's `action` from the page's closed set, and the provider on the
/// sign-in and callback routes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebLine {
    pub session: bool,
    pub tenant_id: Option<Uuid>,
    pub action: Option<&'static str>,
    pub provider: Option<&'static str>,
}

impl Resolved {
    /// What the running request has resolved so far, with `error_code`.
    pub(crate) fn now(error_code: Option<&str>) -> Self {
        let observed = SCOPE.try_with(|s| s.observed()).unwrap_or_default();
        Self {
            identity: observed.identity,
            workarea_id: observed.workarea_id,
            error_code: error_code.map(ToOwned::to_owned),
            grant_type: None,
            web: None,
        }
    }
}

/// The layer's configuration, built once by the composition.
#[derive(Debug, Clone, Default)]
pub struct Traffic {
    trusted: Arc<TrustedProxies>,
    metrics: Arc<Metrics>,
    tracer: Option<Arc<dyn RequestTracer>>,
}

/// Trace export's hook into the layer, set only while the composition
/// exports traces: it makes the request's span the child of a valid W3C
/// `traceparent` and names the span's trace id, which every line about the
/// request then carries (ADR-0042 d13). The transport knows no
/// OpenTelemetry; without a tracer `traceparent` is never read.
pub trait RequestTracer: Send + Sync + std::fmt::Debug {
    /// Adopts `traceparent`'s trace for `span` when it is valid, and
    /// returns the span's trace id.
    fn adopt(&self, span: &tracing::Span, traceparent: Option<&HeaderValue>) -> Option<String>;
}

impl Traffic {
    /// The layer that reads the client address through `trusted`.
    #[must_use]
    pub fn new(trusted: TrustedProxies) -> Self {
        Self {
            trusted: Arc::new(trusted),
            metrics: Arc::default(),
            tracer: None,
        }
    }

    /// The layer while trace export is on (ADR-0042 d13).
    #[must_use]
    pub fn with_tracer(mut self, tracer: Arc<dyn RequestTracer>) -> Self {
        self.tracer = Some(tracer);
        self
    }

    /// The metrics the layer and the MCP facade record, which the
    /// composition serves on `server.metrics_listen` (ADR-0042 d11).
    #[must_use]
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }
}

/// The request body, counting the bytes the server read of it.
struct Counted {
    inner: Body,
    read: Arc<AtomicU64>,
}

impl HttpBody for Counted {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let polled = Pin::new(&mut self.inner).poll_frame(cx);
        if let Poll::Ready(Some(Ok(frame))) = &polled
            && let Some(data) = frame.data_ref()
        {
            self.read.fetch_add(data.len() as u64, Ordering::Relaxed);
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

/// The `User-Agent` as the line keeps it: cut to [`USER_AGENT_MAX`] bytes,
/// every byte outside printable ASCII replaced by `?` (ADR-0042 d9).
#[must_use]
pub fn user_agent(value: &[u8]) -> String {
    value
        .iter()
        .take(USER_AGENT_MAX)
        .map(|&b| {
            if (0x20..=0x7e).contains(&b) {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect()
}

/// Milliseconds to three decimals (ADR-0042 d2).
fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

/// The layer itself (ADR-0042 d1–d7, d9, d14).
pub(crate) async fn layer(
    State(traffic): State<Traffic>,
    mut req: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let ids = RequestIds::of(req.headers());
    req.extensions_mut().insert(ids.clone());
    let method = req.method().clone();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| UNMATCHED.to_owned(), |p| p.as_str().to_owned());
    let peer: Option<IpAddr> = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    let client = traffic.trusted.client_ip(peer, req.headers());
    let agent = req
        .headers()
        .get(header::USER_AGENT)
        .map(|v| user_agent(v.as_bytes()));
    let read = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&read);
    let req = req.map(|inner| {
        Body::new(Counted {
            inner,
            read: counter,
        })
    });
    // One server span per request, named by method and route template; the
    // personal fields never enter it (ADR-0042 d13).
    let span = tracing::info_span!(
        "http_request",
        otel.name = %format_args!("{method} {route}"),
        otel.kind = "server",
        http.request.method = %method,
        http.route = %route,
        http.response.status_code = tracing::field::Empty,
        oml.request_id = %ids.request_id,
        oml.correlation_id = %ids.correlation_id,
        oml.tenant_id = tracing::field::Empty,
        oml.key_id = tracing::field::Empty,
        oml.error_code = tracing::field::Empty,
    );
    let trace_id = traffic
        .tracer
        .as_ref()
        .and_then(|t| t.adopt(&span, req.headers().get("traceparent")));
    let scope = Arc::new(Scope::new(started, trace_id));
    let mut response = SCOPE
        .scope(Arc::clone(&scope), next.run(req))
        .instrument(span.clone())
        .await;
    let duration = started.elapsed();

    let headers = response.headers_mut();
    for (name, value) in [
        (REQUEST_ID, &ids.request_id),
        (CORRELATION_ID, &ids.correlation_id),
    ] {
        // An id is ASCII under the rule or minted, so it is a header value.
        if let Ok(value) = HeaderValue::from_str(value) {
            headers.insert(name, value);
        }
    }

    let status = response.status();
    span.record("http.response.status_code", status.as_u16());
    // A probe answered 200 writes no line, so a kubelet polling every few
    // seconds does not fill the log (ADR-0042 d1).
    if PROBES.contains(&route.as_str()) && status == StatusCode::OK {
        return response;
    }
    let resolved = response
        .extensions()
        .get::<Resolved>()
        .cloned()
        .unwrap_or_default();
    if let Some(identity) = &resolved.identity {
        span.record("oml.tenant_id", tracing::field::display(identity.tenant_id));
        span.record("oml.key_id", tracing::field::display(identity.key_id));
    }
    if let Some(code) = &resolved.error_code {
        span.record("oml.error_code", code.as_str());
    }
    let observed = scope.observed();
    let replay = response
        .headers()
        .get("idempotency-replay")
        .is_some_and(|v| v.as_bytes() == b"true");
    let line = Line {
        ids: &ids,
        trace_id: scope.trace_id.as_deref(),
        method: method.as_str(),
        route: &route,
        status: status.as_u16(),
        resolved: &resolved,
        duration_ms: ms(duration),
        auth_ms: observed.auth.map(ms),
        store_ms: ms(observed.store),
        store_calls: observed.store_calls,
        bytes_in: Some(read.load(Ordering::Relaxed)).filter(|n| *n > 0),
        bytes_out: response.body().size_hint().exact(),
        idempotency_replay: replay.then_some(true),
        client,
        user_agent: agent.as_deref(),
        search: observed.search.map(|(pipeline, _, _)| pipeline),
        search_fallback: observed
            .search
            .and_then(|(_, fallback, _)| fallback.then_some(true)),
        search_access: observed.search.map(|(_, _, access)| access),
        search_rerank: observed.rerank.map(|(outcome, _)| outcome),
        search_rerank_reason: observed.rerank.and_then(|(_, reason)| reason),
    };
    if status.is_server_error() {
        line.write_error();
    } else {
        line.write_info();
    }
    traffic.metrics.request(
        &route,
        status.as_u16(),
        duration,
        replay,
        resolved.error_code.as_deref(),
        resolved.grant_type,
    );
    if let Some((pipeline, fallback, access)) = observed.search {
        traffic.metrics.recall(pipeline, fallback, access);
    }
    if let Some((outcome, reason)) = observed.rerank {
        traffic.metrics.rerank(outcome, reason);
    }
    if let Some((pipeline, result, seconds)) = observed.answered {
        traffic.metrics.recall_answered(pipeline, result, seconds);
    }
    if let Some((relation, on_similar, seconds)) = observed.similar {
        traffic
            .metrics
            .similar_checked(relation, on_similar, seconds);
    }
    for kind in &observed.origins {
        traffic.metrics.recall_hit_origin(kind);
    }
    for &(pipeline, stage, seconds) in &observed.stages {
        traffic.metrics.recall_stage(pipeline, stage, seconds);
    }
    if let Some((verdict, waited)) = observed.verdict {
        traffic.metrics.verdict(verdict, waited);
    }
    if observed.unreviewed_hits > 0 {
        traffic.metrics.unreviewed_hits(observed.unreviewed_hits);
    }
    if let Some(mode) = observed.mode_set {
        traffic.metrics.mode_set(mode);
    }
    if let Some(act) = observed.curation {
        traffic.metrics.curation(act);
    }
    response
}

/// One `http_request` line's values (ADR-0042 d2).
struct Line<'a> {
    ids: &'a RequestIds,
    trace_id: Option<&'a str>,
    method: &'a str,
    route: &'a str,
    status: u16,
    resolved: &'a Resolved,
    duration_ms: f64,
    auth_ms: Option<f64>,
    store_ms: f64,
    store_calls: u64,
    bytes_in: Option<u64>,
    bytes_out: Option<u64>,
    idempotency_replay: Option<bool>,
    client: Option<(IpAddr, ClientIpSource)>,
    user_agent: Option<&'a str>,
    /// The search pipeline a recall ran (ADR-0049 d4).
    search: Option<&'static str>,
    search_fallback: Option<bool>,
    /// The reranking stage's outcome and its reason (ADR-0053 d9).
    search_rerank: Option<&'static str>,
    search_rerank_reason: Option<&'static str>,
    /// `postings` or `scan` (ADR-0052 d11).
    search_access: Option<&'static str>,
}

/// The line at one level: the event macros take their level as a constant.
macro_rules! write_line {
    ($level:ident, $line:expr) => {{
        let line = $line;
        let identity = line.resolved.identity.as_ref();
        let web = line.resolved.web.as_ref();
        let session = web.is_some_and(|w| w.session);
        tracing::$level!(
            event = EVENT_HTTP_REQUEST,
            request_id = %line.ids.request_id,
            request_id_source = line.ids.request_id_source.as_str(),
            correlation_id = %line.ids.correlation_id,
            correlation_id_source = line.ids.correlation_id_source.as_str(),
            trace_id = line.trace_id,
            method = line.method,
            route = line.route,
            status = line.status,
            error_code = line.resolved.error_code.as_deref(),
            duration_ms = line.duration_ms,
            auth_ms = line.auth_ms,
            store_ms = line.store_ms,
            store_calls = line.store_calls,
            bytes_in = line.bytes_in,
            bytes_out = line.bytes_out,
            tenant_id = identity
                .map(|i| i.tenant_id)
                .or_else(|| web.and_then(|w| w.tenant_id))
                .map(tracing::field::display),
            key_id = identity.map(|i| tracing::field::display(i.key_id)),
            key_name = identity.and_then(|i| i.key_name.as_deref()),
            principal = identity.map(|i| i.principal.as_str()),
            role = identity.map(|i| i.role),
            credential = identity
                .map(|i| i.credential)
                .or(session.then_some("session")),
            connection_id = identity
                .and_then(|i| i.connection_id)
                .map(tracing::field::display),
            workarea_id = line.resolved.workarea_id.map(tracing::field::display),
            idempotency_replay = line.idempotency_replay,
            client_ip = line.client.map(|(ip, _)| tracing::field::display(ip)),
            client_ip_source = line.client.map(|(_, source)| source.as_str()),
            user_agent = line.user_agent,
            web_action = web.and_then(|w| w.action),
            web_provider = web.and_then(|w| w.provider),
            search = line.search,
            search_fallback = line.search_fallback,
            search_rerank = line.search_rerank,
            search_rerank_reason = line.search_rerank_reason,
            search_access = line.search_access,
        );
    }};
}

impl Line<'_> {
    fn write_info(&self) {
        write_line!(info, self);
    }

    fn write_error(&self) {
        write_line!(error, self);
    }
}
