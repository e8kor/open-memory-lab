//! The request metrics v0.4.1 builds, served on a listener of their own
//! (ADR-0042 d11, d12). Every label is from a closed set and none carries
//! a tenant, a name, a principal or an address.

use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use axum::routing::get;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{Histogram, exponential_buckets};
use prometheus_client::registry::Registry;

use std::sync::Arc;

/// The histograms' bounds: 1 ms doubling to 32.768 s, sixteen buckets and
/// `+Inf`. The instrument's resolution, not a target (ADR-0042 d12).
fn histogram() -> Histogram {
    Histogram::new(exponential_buckets(0.001, 2.0, 16))
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct HttpLabels {
    route: String,
    status: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct McpLabels {
    tool: String,
    mode: String,
    outcome: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct RecallLabels {
    pipeline: String,
    fallback: String,
    access: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct RerankLabels {
    outcome: String,
    reason: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct SparseLabels {
    outcome: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct TokenLabels {
    grant_type: String,
    outcome: String,
}

/// The four families (ADR-0042 d12).
#[derive(Debug)]
pub struct Metrics {
    registry: Registry,
    http: Family<HttpLabels, Histogram, fn() -> Histogram>,
    mcp: Family<McpLabels, Histogram, fn() -> Histogram>,
    replays: Counter,
    conflicts: Counter,
    oauth_tokens: Family<TokenLabels, Counter>,
    recalls: Family<RecallLabels, Counter>,
    reranks: Family<RerankLabels, Counter>,
    sparse_encoded: Family<SparseLabels, Counter>,
    sparse_backlog: Gauge,
}

impl Default for Metrics {
    fn default() -> Self {
        let mut registry = Registry::default();
        let http = Family::<HttpLabels, Histogram, fn() -> Histogram>::new_with_constructor(
            histogram as fn() -> Histogram,
        );
        let mcp = Family::<McpLabels, Histogram, fn() -> Histogram>::new_with_constructor(
            histogram as fn() -> Histogram,
        );
        let replays = Counter::default();
        let conflicts = Counter::default();
        let oauth_tokens = Family::<TokenLabels, Counter>::default();
        let recalls = Family::<RecallLabels, Counter>::default();
        let reranks = Family::<RerankLabels, Counter>::default();
        let sparse_encoded = Family::<SparseLabels, Counter>::default();
        let sparse_backlog = Gauge::default();
        registry.register(
            "oml_http_request_duration_seconds",
            "API-listener request duration by route template and status; probes answered 200 are not counted",
            http.clone(),
        );
        registry.register(
            "oml_mcp_call_duration_seconds",
            "/mcp call duration inside the facade by tool, mode and outcome",
            mcp.clone(),
        );
        // The client appends `_total` to a counter's name.
        registry.register(
            "oml_idempotency_replays",
            "responses carrying Idempotency-Replay: true",
            replays.clone(),
        );
        registry.register(
            "oml_idempotency_conflicts",
            "requests refused in the idempotency family",
            conflicts.clone(),
        );
        registry.register(
            "oml_oauth_token_requests",
            "POST /oauth/token by grant_type and RFC outcome",
            oauth_tokens.clone(),
        );
        // The pipeline label is from the build's closed set of pipeline
        // names (ADR-0049 d4).
        registry.register(
            "oml_recalls",
            "recalls by the search pipeline that ranked them, whether match all fell back, and whether postings or a scan answered",
            recalls.clone(),
        );
        // Both labels are closed sets: applied or skipped, and none, busy,
        // timeout or failed (ADR-0053 d9).
        registry.register(
            "oml_reranks",
            "reranking stages by outcome, and why a skipped one was skipped",
            reranks.clone(),
        );
        // The learned sparse encoder's outbox (ADR-0052 d15): versions
        // written or failed, a closed set, and the entries waiting, summed
        // over every tenant with no tenant label.
        registry.register(
            "oml_sparse_encoded",
            "versions the learned sparse encoder wrote postings for, or failed to",
            sparse_encoded.clone(),
        );
        registry.register(
            "oml_sparse_backlog",
            "versions waiting for the learned sparse encoder, every tenant's summed",
            sparse_backlog.clone(),
        );
        Self {
            registry,
            http,
            mcp,
            replays,
            conflicts,
            oauth_tokens,
            recalls,
            reranks,
            sparse_encoded,
            sparse_backlog,
        }
    }
}

/// `POST /oauth/token`'s `grant_type` as the label takes it.
#[must_use]
pub fn grant_type_label(sent: Option<&str>) -> &'static str {
    match sent {
        Some("authorization_code") => "authorization_code",
        Some("refresh_token") => "refresh_token",
        _ => "other",
    }
}

impl Metrics {
    /// One API-listener request, as the request line saw it.
    pub(crate) fn request(
        &self,
        route: &str,
        status: u16,
        duration: Duration,
        replay: bool,
        error_code: Option<&str>,
        grant_type: Option<&'static str>,
    ) {
        self.http
            .get_or_create(&HttpLabels {
                route: route.to_owned(),
                status: status.to_string(),
            })
            .observe(duration.as_secs_f64());
        if replay {
            self.replays.inc();
        }
        if error_code.is_some_and(|c| c.starts_with("idempotency.")) {
            self.conflicts.inc();
        }
        if route == "/oauth/token" {
            self.oauth_tokens
                .get_or_create(&TokenLabels {
                    grant_type: grant_type.unwrap_or("other").to_owned(),
                    outcome: error_code.unwrap_or("ok").to_owned(),
                })
                .inc();
        }
    }

    /// One recall, by the pipeline that ranked it and how its lists were
    /// read (ADR-0049 d4; ADR-0052 d6).
    pub(crate) fn recall(&self, pipeline: &'static str, fallback: bool, access: &'static str) {
        self.recalls
            .get_or_create(&RecallLabels {
                pipeline: pipeline.to_owned(),
                fallback: fallback.to_string(),
                access: access.to_owned(),
            })
            .inc();
    }

    /// One recall's reranking stage: `applied`, or `skipped` with its
    /// reason (ADR-0053 d9).
    pub(crate) fn rerank(&self, outcome: &'static str, reason: Option<&'static str>) {
        self.reranks
            .get_or_create(&RerankLabels {
                outcome: outcome.to_owned(),
                reason: reason.unwrap_or("none").to_owned(),
            })
            .inc();
    }

    /// One encoder pass's versions: `written` and `failed` (ADR-0052 d15).
    pub fn sparse_pass(&self, written: u64, failed: u64) {
        for (outcome, n) in [("written", written), ("failed", failed)] {
            if n > 0 {
                self.sparse_encoded
                    .get_or_create(&SparseLabels {
                        outcome: outcome.to_owned(),
                    })
                    .inc_by(n);
            }
        }
    }

    /// The entries waiting for the encoder (ADR-0052 d15).
    pub fn sparse_backlog(&self, waiting: u64) {
        self.sparse_backlog
            .set(i64::try_from(waiting).unwrap_or(i64::MAX));
    }

    /// One `/mcp` call, by the closed values of its `mcp_call` line.
    pub(crate) fn mcp_call(&self, tool: &str, mode: &str, outcome: &str, seconds: f64) {
        self.mcp
            .get_or_create(&McpLabels {
                tool: tool.to_owned(),
                mode: mode.to_owned(),
                outcome: outcome.to_owned(),
            })
            .observe(seconds);
    }

    /// The exposition: OpenMetrics text, which Prometheus scrapes.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut out = String::new();
        // Writing into a String cannot fail.
        let _ = prometheus_client::encoding::text::encode(&mut out, &self.registry);
        out
    }
}

/// The metrics listener's router: `GET /metrics`, and `404` on every other
/// path. No credential, no request line and no histogram entry: its
/// protection is the address it binds (ADR-0042 d11).
pub fn router(metrics: Arc<Metrics>) -> Router {
    Router::new()
        .route("/metrics", get(scrape))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(metrics)
}

async fn scrape(State(metrics): State<Arc<Metrics>>) -> Response {
    let mut response = metrics.encode().into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/openmetrics-text; version=1.0.0; charset=utf-8"),
    );
    response
}
