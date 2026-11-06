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
use std::sync::atomic::AtomicU64;

use oml_app::ports::TelemetryMeter;
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

/// The review wait's bounds: one minute doubling fifteen times, to about
/// 22.8 days, and `+Inf` (ADR-0054 d1).
fn review_histogram() -> Histogram {
    Histogram::new(exponential_buckets(60.0, 2.0, 16))
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct OriginLabels {
    origin_kind: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct AnswerLabels {
    pipeline: String,
    result: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct PipelineLabels {
    pipeline: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct StageLabels {
    pipeline: String,
    stage: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct SimilarLabels {
    relation: String,
    on_similar: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct LabelLabels {
    label: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct OutcomeLabels {
    outcome: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ReasonLabels {
    reason: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct RelationLabels {
    relation: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ModeLabels {
    mode: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ActLabels {
    act: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct VerdictLabels {
    verdict: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct AgeLabels {
    promoted_for: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct QuotaLabels {
    quota: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct WebFailureLabels {
    provider: String,
    reason: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct SparseLabels {
    outcome: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct BulkLabels {
    outcome: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct TokenLabels {
    grant_type: String,
    outcome: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ScanLabels {
    class: String,
    rule: String,
    action: String,
}

/// Where the scan findings come from: the composition's rule set, read at
/// each scrape (ADR-0065 d11). Counts by class, rule and action, nothing
/// else.
pub type ScanCounts = fn() -> Vec<(&'static str, &'static str, &'static str, u64)>;

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
    bulk_items: Family<BulkLabels, Counter>,
    unreviewed_hits: Counter,
    handler_panics: Counter,
    mode_changes: Family<ModeLabels, Counter>,
    curation_acts: Family<ActLabels, Counter>,
    source_checks: Family<BulkLabels, Counter>,
    web_failures: Family<WebFailureLabels, Counter>,
    quota_refusals: Family<QuotaLabels, Counter>,
    quality: Quality,
    sparse_encoded: Family<SparseLabels, Counter>,
    sparse_backlog: Gauge,
    scan_findings: Family<ScanLabels, Counter>,
    scan_counts: std::sync::OnceLock<ScanCounts>,
    scan_refresh: std::sync::Mutex<()>,
}

/// The recall and remember quality families (ADR-0054 d1, as accepted):
/// aggregates only, every label from a closed set, no tenant.
#[derive(Debug)]
struct Quality {
    answers: Family<AnswerLabels, Counter>,
    durations: Family<PipelineLabels, Histogram, fn() -> Histogram>,
    outcomes: Family<LabelLabels, Counter>,
    events: Family<OutcomeLabels, Counter>,
    labels_dropped: Family<ReasonLabels, Counter>,
    duplicates: Family<RelationLabels, Counter>,
    review_wait: Family<VerdictLabels, Histogram, fn() -> Histogram>,
    queue_depth: Gauge,
    lag: Gauge<f64, AtomicU64>,
    purge_overdue: Gauge,
    unrecalled: Family<AgeLabels, Gauge>,
    origins: Family<OriginLabels, Counter>,
    stages: Family<StageLabels, Histogram, fn() -> Histogram>,
    similar: Family<SimilarLabels, Counter>,
    similar_duration: Histogram,
}

impl Quality {
    fn register(registry: &mut Registry) -> Self {
        let q = Self {
            answers: Family::default(),
            durations: Family::new_with_constructor(histogram as fn() -> Histogram),
            outcomes: Family::default(),
            events: Family::default(),
            labels_dropped: Family::default(),
            duplicates: Family::default(),
            review_wait: Family::new_with_constructor(review_histogram as fn() -> Histogram),
            queue_depth: Gauge::default(),
            lag: Gauge::default(),
            purge_overdue: Gauge::default(),
            unrecalled: Family::default(),
            origins: Family::default(),
            stages: Family::new_with_constructor(histogram as fn() -> Histogram),
            similar: Family::default(),
            similar_duration: histogram(),
        };
        registry.register(
            "oml_recall_stage_duration_seconds",
            "time a recall spent in each stage, by pipeline and stage: scope, retrieve_<retriever>, fuse, rerank, suggest, hydrate, cut",
            q.stages.clone(),
        );
        registry.register(
            "oml_remember_similar",
            "writes the similar check ran for, by the best relation it named (none, same_text, near_copy) and on_similar (hold, refuse)",
            q.similar.clone(),
        );
        registry.register(
            "oml_remember_similar_duration_seconds",
            "time a write's similar check took",
            q.similar_duration.clone(),
        );
        registry.register(
            "oml_recall_answers",
            "recall answers by pipeline and shape: hits, empty, or below_floor when the relevance floor withheld a row",
            q.answers.clone(),
        );
        registry.register(
            "oml_recall_duration_seconds",
            "time the app spent answering a recall, by pipeline",
            q.durations.clone(),
        );
        registry.register(
            "oml_recall_outcomes",
            "labels of what a recall's reader did next, inferred or sent as feedback",
            q.outcomes.clone(),
        );
        registry.register(
            "oml_recall_events",
            "recall events by outcome: written, or dropped with the reason",
            q.events.clone(),
        );
        registry.register(
            "oml_recall_labels_dropped",
            "labels not stored, by reason",
            q.labels_dropped.clone(),
        );
        registry.register(
            "oml_remember_duplicates",
            "memories written whose text a served memory of the same workarea already holds (same_text) or not (none)",
            q.duplicates.clone(),
        );
        registry.register(
            "oml_review_wait_seconds",
            "time from a memory's submission to its verdict, by verdict; the count by verdict is the rejection rate",
            q.review_wait.clone(),
        );
        registry.register(
            "oml_telemetry_queue_depth",
            "recall telemetry items waiting for the writer",
            q.queue_depth.clone(),
        );
        registry.register(
            "oml_labeller_lag_seconds",
            "seconds since the labeller's last completed pass",
            q.lag.clone(),
        );
        registry.register(
            "oml_telemetry_purge_overdue",
            "organisations holding recall telemetry rows past the 30 days and a minute: the purge is behind",
            q.purge_overdue.clone(),
        );
        registry.register(
            "oml_recall_hits_by_origin",
            "recall hits served, by the origin kind of the version, none for one written before kinds (ADR-0063 d13)",
            q.origins.clone(),
        );
        registry.register(
            "oml_memories_unrecalled",
            "served memories no recall of the last 30 days returned, by how long ago they were promoted",
            q.unrecalled.clone(),
        );
        q
    }
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
        let bulk_items = Family::<BulkLabels, Counter>::default();
        let unreviewed_hits = Counter::default();
        let handler_panics = Counter::default();
        let mode_changes = Family::<ModeLabels, Counter>::default();
        let curation_acts = Family::<ActLabels, Counter>::default();
        let source_checks = Family::<BulkLabels, Counter>::default();
        let web_failures = Family::<WebFailureLabels, Counter>::default();
        let quota_refusals = Family::<QuotaLabels, Counter>::default();
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
        // The outcome is a closed set: approved, rejected or skipped
        // (ADR-0055 d13).
        registry.register(
            "oml_review_bulk_items",
            "items of bulk verdicts by outcome",
            bulk_items.clone(),
        );
        // No label: hits served labelled unreviewed inside an in-progress
        // scope, or a key's own held rows (ADR-0030 d5).
        registry.register(
            "oml_recall_unreviewed_hits",
            "recall hits served labelled unreviewed",
            unreviewed_hits.clone(),
        );
        // The mode is a closed set: in_progress or standard (ADR-0030 d5).
        registry.register(
            "oml_workarea_mode_changes",
            "workarea modes set by a person's key, by the mode set; standard on an in-progress root is its commit",
            mode_changes.clone(),
        );
        // The act is a closed set: reject, retire, flag or write (ADR-0030
        // d5, d9).
        registry.register(
            "oml_curation_acts",
            "acts committed by curator keys inside in-progress workareas, by act",
            curation_acts.clone(),
        );
        // The outcome is a closed set: fresh, equivalent, moved, changed or
        // removed (ADR-0057 d17).
        registry.register(
            "oml_source_checks",
            "source checks recorded, by result",
            source_checks.clone(),
        );
        // Both labels are closed sets: the provider, and the exchange's
        // failure reason (ADR-0045 d18), moved off /readyz (ADR-0054 d1).
        registry.register(
            "oml_web_exchanges_failed",
            "sign-in provider exchanges that failed, by provider and reason",
            web_failures.clone(),
        );
        // One label, the closed set of the four quotas (ADR-0068 d5).
        registry.register(
            "oml_quota_refused",
            "requests refused by a person's quota or a key's request rate, by quota",
            quota_refusals.clone(),
        );
        let quality = Quality::register(&mut registry);
        // The learned sparse encoder's outbox (ADR-0052 d15): versions
        // written or failed, a closed set, and the entries waiting, summed
        // over every tenant with no tenant label.
        registry.register(
            "oml_sparse_encoded",
            "versions the learned sparse encoder wrote postings for, failed to, or parked as dead letters",
            sparse_encoded.clone(),
        );
        registry.register(
            "oml_sparse_backlog",
            "versions waiting for the learned sparse encoder across the deployment",
            sparse_backlog.clone(),
        );
        registry.register(
            "oml_handler_panics",
            "requests whose handler panicked and were answered system.internal by the panic guard; every one is a defect, named in the lead's log under event handler_panic",
            handler_panics.clone(),
        );
        // Every label is from the rule set's closed vocabulary: the finding
        // class, the rule id and veto or hold — never what a rule matched
        // (ADR-0065 d8, d11).
        let scan_findings = Family::<ScanLabels, Counter>::default();
        registry.register(
            "oml_scan_findings",
            "deterministic scan findings since start, by finding class, rule and action",
            scan_findings.clone(),
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
            bulk_items,
            unreviewed_hits,
            handler_panics,
            mode_changes,
            curation_acts,
            source_checks,
            web_failures,
            quota_refusals,
            quality,
            sparse_encoded,
            sparse_backlog,
            scan_findings,
            scan_counts: std::sync::OnceLock::new(),
            scan_refresh: std::sync::Mutex::new(()),
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

    /// One item of a bulk verdict: `approved`, `rejected` or `skipped`
    /// (ADR-0055 d13).
    pub(crate) fn bulk_item(&self, outcome: &'static str) {
        self.bulk_items
            .get_or_create(&BulkLabels {
                outcome: outcome.to_owned(),
            })
            .inc();
    }

    /// `n` recall hits served labelled `unreviewed` (ADR-0030 d5).
    /// One handler panic the panic guard answered.
    pub(crate) fn handler_panicked(&self) {
        self.handler_panics.inc();
    }

    pub(crate) fn unreviewed_hits(&self, n: u64) {
        self.unreviewed_hits.inc_by(n);
    }

    /// One workarea mode set by a person's key (ADR-0030 d5).
    pub(crate) fn mode_set(&self, mode: &'static str) {
        self.mode_changes
            .get_or_create(&ModeLabels {
                mode: mode.to_owned(),
            })
            .inc();
    }

    /// One curator act (ADR-0030 d5, d9).
    pub(crate) fn curation(&self, act: &'static str) {
        self.curation_acts
            .get_or_create(&ActLabels {
                act: act.to_owned(),
            })
            .inc();
    }

    /// One recorded source check, by its result (ADR-0057 d17).
    pub(crate) fn source_check(&self, result: &'static str) {
        self.source_checks
            .get_or_create(&BulkLabels {
                outcome: result.to_owned(),
            })
            .inc();
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

    /// One request refused by `quota` (ADR-0068 d5).
    pub fn quota_refused(&self, quota: &'static str) {
        self.quota_refusals
            .get_or_create(&QuotaLabels {
                quota: quota.to_owned(),
            })
            .inc();
    }

    /// One sign-in exchange with `provider` failed for `reason` (ADR-0045
    /// d18).
    pub fn web_failed(&self, provider: &'static str, reason: &'static str) {
        self.web_failures
            .get_or_create(&WebFailureLabels {
                provider: provider.to_owned(),
                reason: reason.to_owned(),
            })
            .inc();
    }

    /// One recall answered `result` in `seconds` through `pipeline`
    /// (ADR-0054 d1).
    pub(crate) fn recall_answered(
        &self,
        pipeline: &'static str,
        result: &'static str,
        seconds: f64,
    ) {
        self.quality
            .answers
            .get_or_create(&AnswerLabels {
                pipeline: pipeline.to_owned(),
                result: result.to_owned(),
            })
            .inc();
        self.quality
            .durations
            .get_or_create(&PipelineLabels {
                pipeline: pipeline.to_owned(),
            })
            .observe(seconds);
    }

    /// One recall hit of `origin_kind`, one of eight closed values.
    pub(crate) fn recall_hit_origin(&self, origin_kind: &str) {
        self.quality
            .origins
            .get_or_create(&OriginLabels {
                origin_kind: origin_kind.to_owned(),
            })
            .inc();
    }

    /// One recall stage's seconds (ADR-0049 d14).
    pub(crate) fn recall_stage(&self, pipeline: &'static str, stage: &'static str, seconds: f64) {
        self.quality
            .stages
            .get_or_create(&StageLabels {
                pipeline: pipeline.to_owned(),
                stage: stage.to_owned(),
            })
            .observe(seconds);
    }

    /// One write's similar check (ADR-0050 d1; ADR-0054 d1).
    pub(crate) fn similar_checked(
        &self,
        relation: &'static str,
        on_similar: &'static str,
        seconds: f64,
    ) {
        self.quality
            .similar
            .get_or_create(&SimilarLabels {
                relation: relation.to_owned(),
                on_similar: on_similar.to_owned(),
            })
            .inc();
        self.quality.similar_duration.observe(seconds);
    }

    /// One verdict, `waited` seconds after its memory's submission.
    pub(crate) fn verdict(&self, verdict: &'static str, waited: f64) {
        self.quality
            .review_wait
            .get_or_create(&VerdictLabels {
                verdict: verdict.to_owned(),
            })
            .observe(waited);
    }

    /// One encoder pass's versions: `written`, `failed`, and of those
    /// `parked` at the attempts' ceiling (ADR-0052 d15).
    pub fn sparse_pass(&self, written: u64, failed: u64, parked: u64) {
        for (outcome, n) in [("written", written), ("failed", failed), ("parked", parked)] {
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

    /// Read the scan findings from `counts` at every scrape (ADR-0065 d11).
    /// The first source set stays.
    pub fn with_scan_counts(&self, counts: ScanCounts) {
        let _ = self.scan_counts.set(counts);
    }

    /// Bring the findings counters up to the rule set's counts. One refresh
    /// at a time, so two scrapes never add one delta twice.
    fn refresh_scan(&self) {
        let Some(counts) = self.scan_counts.get() else {
            return;
        };
        let Ok(_one) = self.scan_refresh.lock() else {
            return;
        };
        for (class, rule, action, n) in counts() {
            let counter = self.scan_findings.get_or_create(&ScanLabels {
                class: class.to_owned(),
                rule: rule.to_owned(),
                action: action.to_owned(),
            });
            let seen = counter.get();
            if n > seen {
                counter.inc_by(n - seen);
            }
        }
    }

    /// The exposition: OpenMetrics text, which Prometheus scrapes.
    #[must_use]
    pub fn encode(&self) -> String {
        self.refresh_scan();
        let mut out = String::new();
        // Writing into a String cannot fail.
        let _ = prometheus_client::encoding::text::encode(&mut out, &self.registry);
        out
    }
}

/// The recall telemetry writer's counts and gauges (ADR-0054 d1).
impl TelemetryMeter for Metrics {
    fn events(&self, outcome: &'static str, n: u64) {
        self.quality
            .events
            .get_or_create(&OutcomeLabels {
                outcome: outcome.to_owned(),
            })
            .inc_by(n);
    }

    fn labels(&self, label: &'static str, n: u64) {
        self.quality
            .outcomes
            .get_or_create(&LabelLabels {
                label: label.to_owned(),
            })
            .inc_by(n);
    }

    fn labels_dropped(&self, reason: &'static str, n: u64) {
        self.quality
            .labels_dropped
            .get_or_create(&ReasonLabels {
                reason: reason.to_owned(),
            })
            .inc_by(n);
    }

    fn remembered(&self, relation: &'static str) {
        self.quality
            .duplicates
            .get_or_create(&RelationLabels {
                relation: relation.to_owned(),
            })
            .inc();
    }

    fn queue_depth(&self, depth: i64) {
        self.quality.queue_depth.set(depth);
    }

    fn health(&self, purge_overdue: i64) {
        self.quality.purge_overdue.set(purge_overdue);
    }

    fn lag(&self, seconds: f64) {
        self.quality.lag.set(seconds);
    }

    fn unrecalled(&self, by_age: [i64; 4]) {
        for (age, n) in ["7d", "30d", "90d", "older"].into_iter().zip(by_age) {
            self.quality
                .unrecalled
                .get_or_create(&AgeLabels {
                    promoted_for: age.to_owned(),
                })
                .set(n);
        }
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
