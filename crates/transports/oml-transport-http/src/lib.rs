//! `oml-transport-http` — the two-ring HTTP transport (workspace-plan.md:
//! transports depend on `oml-app` only). Paths, field spellings and status
//! codes follow docs/api/openapi.yaml; every refusal is a typed code from
//! docs/api/errors.md, attached here at the edge and nowhere else
//! (errors.md "Implementation mapping").
//!
//! The PoC slice: submit / list / get / recall / supersede on `/v1`; review next,
//! verdicts, key issuance, the key read, global per-key denials, workarea
//! creation, the workarea read, a workarea's crew, the crew records, the
//! events tail and the bootstrap claim on `/v2`; `/healthz` and `/readyz`
//! unversioned and unauthenticated (http-api.md "Health"); `/mcp`, the agent
//! plugin's stateless facade over those rings ([`mcp`]).
//!
//! Every value that names a workarea, crew or key — a header, a path or
//! query parameter, a body field — is classified here, at parse, as an id or
//! a name, and refused when it is neither; the app resolves a name after the
//! verb ceiling (http-api.md "Names and ids"; ADR-0026 d3, d4).

#![forbid(unsafe_code)]

mod console;
pub mod error;
mod federation;
pub mod mcp;
pub mod oauth;
mod rescan;
mod search;
pub mod session;
pub mod time;
pub mod traffic;
mod usage;
pub mod web;

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::{MethodRouter, get, post};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::App;
use oml_app::app::Caller;
use oml_app::domain::EventTime;
use oml_app::domain::access::Covers;
use oml_app::domain::anchor::{self as anchor_grammar, Anchor, CheckResult};
use oml_app::domain::budget::{Budget, COUNTER_ID, budget_tokens};
use oml_app::domain::guardrail::{GuardrailScope, Severity};
use oml_app::domain::in_progress::{KeyProfile, WorkareaMode};
use oml_app::domain::jcs::MAX_CHAIN_INTEGER;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::oauth::ConnectionState;
use oml_app::domain::origin::{self as origin_grammar, OriginKind};
use oml_app::domain::valid_time::{ValidEnd, fixed_width};
use oml_app::domain::{HeldBy, MemoryKind, MemoryStatus, Role, WorkareaStatus, nfc};
use oml_app::ports::{
    ClientDocumentFetcher, Clock, ConnectionStore, HandleSealer, IdGen, JournalStore, Scanner,
    Signer, StoreError, WebStore,
};
use oml_app::search::MatchMode;
use oml_app::types::{
    AnchorFilter, AppError, Bearer, BootstrapRequest, BriefEdgeView, ClaimEdgeProposal,
    ClaimEdgeQuery, ConnectCodeIssued, ConnectionRecord, CrewCreateRequest, CrewRecord, CrewUpdate,
    EdgeRatification, GrantRecord, GrantRequest, GuardrailRequest, GuardrailSet, IssueKeyRequest,
    IssuedKey, KeyDetail, ListRequest, MemoryRecord, NamedServe, OnSimilar, ProposeBriefEdge,
    ProposeClaimEdge, RecallRequest, ServedMemory, SimilarItem, SourceCheck, SourceCheckRequest,
    SubmitRequest, SubmitVerb, SupersedeRequest, TailedEvent, Verdict, VerdictRequest, Whoami,
    WorkareaInfo, WorkareaRecord,
};
use oml_app::types::{
    BulkItem, BulkVerdictRequest, HELD_PAGE_DEFAULT, HeldListingRequest, HeldPage,
};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::time::{parse_date_time, system_event_time, system_event_time_plus};
use crate::traffic::Rid;

/// How many times a write retries after a chain-head conflict before giving
/// up — the concurrent-submitter shape the oml-pg phase prescribed: re-read
/// the head (the app does, per call) and try again.
pub const CHAIN_RETRIES: usize = 16;

/// Every HTTP operation the router serves, as `(METHOD, path)` in
/// docs/api/openapi.yaml's spelling: the served contract declares exactly
/// these (ADR-0039 d9), and [`ApiState::into_router`] routes exactly these.
/// `/mcp`, whose contract is docs/api/mcp-tools.json, is not among them.
pub const ROUTES: &[(&str, &str)] = &[
    ("GET", "/healthz"),
    ("GET", "/readyz"),
    ("POST", "/v1/memories"),
    ("GET", "/v1/memories"),
    ("POST", "/v1/memories/recall"),
    ("GET", "/v1/memories/{artifact_id}"),
    ("POST", "/v1/memories/{artifact_id}/supersede"),
    ("POST", "/v1/memories/{artifact_id}/source-check"),
    ("GET", "/v1/guardrails"),
    ("POST", "/v2/review/next"),
    ("POST", "/v2/review/verdicts"),
    ("GET", "/v2/review/held"),
    ("POST", "/v2/review/verdicts/bulk"),
    ("GET", "/v2/review/queue"),
    ("GET", "/v2/keys"),
    ("POST", "/v2/keys"),
    ("GET", "/v2/keys/{key}"),
    ("POST", "/v2/keys/{key}/denials"),
    ("POST", "/v2/keys/{key}/connect-codes"),
    ("POST", "/v2/keys/{key}/connections/{connection_id}/revoke"),
    ("GET", "/v2/whoami"),
    ("POST", "/v2/crews"),
    ("GET", "/v2/crews"),
    ("GET", "/v2/crews/{crew}"),
    ("POST", "/v2/crews/{crew}/update"),
    ("POST", "/v2/workareas"),
    ("GET", "/v2/workareas/{workarea}"),
    ("GET", "/v2/workareas/{workarea}/tree"),
    ("GET", "/v2/workareas/{workarea}/offers"),
    ("POST", "/v2/workareas/{workarea}/crew"),
    ("POST", "/v2/workareas/{workarea}/mode"),
    ("POST", "/v2/workareas/{workarea}/federation"),
    ("POST", "/v2/workareas/{workarea}/transitions"),
    (
        "POST",
        "/v2/workareas/{workarea}/transitions/{transition_id}/ratify",
    ),
    ("GET", "/v2/edges"),
    ("POST", "/v2/edges"),
    ("POST", "/v2/edges/{edge_id}/ratify"),
    ("GET", "/v2/events"),
    ("POST", "/v2/bootstrap/claim"),
    ("GET", "/v2/web/emails"),
    ("GET", "/v2/web/users"),
    ("POST", "/v2/web/emails"),
    ("POST", "/v2/web/emails/remove"),
    ("POST", "/v2/web/users/{user}/capacities"),
    ("POST", "/v2/web/users/{user}/quota"),
    ("POST", "/v2/web/users/{user}/disable"),
    ("POST", "/v2/web/users/{user}/deletion/close"),
    ("POST", "/v2/web/session/end"),
    ("GET", "/v2/web/export"),
    ("POST", "/v2/web/deletion-requests"),
    ("POST", "/v2/admin/memories/search"),
    ("GET", "/v2/admin/memories/{artifact_id}"),
    ("GET", "/v2/admin/rescan"),
    ("POST", "/v2/admin/rescan"),
    ("GET", "/v2/search"),
    ("POST", "/v2/search/default"),
    ("GET", "/v2/federation"),
    ("POST", "/v2/federation"),
    ("GET", "/v2/usage/settings"),
    ("POST", "/v2/usage/settings"),
    ("GET", "/v2/usage/events"),
    ("POST", "/v2/usage/eval"),
];

/// The unversioned probes among [`ROUTES`], served even while names are not
/// assigned (http-api.md "Health").
pub(crate) const PROBES: [&str; 2] = ["/healthz", "/readyz"];

/// The unversioned OAuth operations outside both rings, as `(METHOD, path)`
/// in docs/api/openapi.yaml's spelling, which [`ApiState::into_router`]
/// routes exactly where the deployment sets `oauth.issuer`, and not at all
/// otherwise (ADR-0031 d1, d3, d4, d5, d9; ADR-0039 d9). The served
/// contract declares these beside [`ROUTES`].
pub const OAUTH_ROUTES: &[(&str, &str)] = &[
    ("GET", "/.well-known/oauth-protected-resource/mcp"),
    ("GET", "/.well-known/oauth-authorization-server"),
    ("GET", "/oauth/authorize"),
    ("POST", "/oauth/authorize"),
    ("POST", "/oauth/register"),
    ("POST", "/oauth/token"),
];

const PAGE_LIMIT_MAX: usize = 200; // api.page_limit_max shipped default
const PAGE_LIMIT_DEFAULT: usize = 50;
const MAX_BODY_BYTES: usize = 1_048_576; // submission.max_body_size shipped default
const SERVE_TTL_SECS: u64 = 900;

/// A `/readyz` database probe the composition supplies (the transport knows
/// no sqlx — ADR-0019: adapters stay behind the ports).
pub type PingFuture = Pin<Box<dyn Future<Output = bool> + Send>>;
pub type DbPing = Arc<dyn Fn() -> PingFuture + Send + Sync>;

/// The build `/readyz` names (http-api.md "Health"). The composition supplies
/// it; the transport knows no build system. A field nothing supplied is
/// `unknown`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: String,
    pub commit: String,
    pub commit_time: String,
    pub build_time: String,
    pub rustc: String,
    pub target: String,
    pub profile: String,
}

impl BuildInfo {
    /// What a composition that supplies no build reports.
    #[must_use]
    pub fn unknown() -> Self {
        let unknown = || "unknown".to_owned();
        Self {
            version: unknown(),
            commit: unknown(),
            commit_time: unknown(),
            build_time: unknown(),
            rustc: unknown(),
            target: unknown(),
            profile: unknown(),
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "version": self.version,
            "commit": self.commit,
            "commit_time": self.commit_time,
            "build_time": self.build_time,
            "rustc": self.rustc,
            "target": self.target,
            "profile": self.profile,
        })
    }
}

/// Everything the handlers share. `store` is a second handle on the journal
/// port for the reads the use-case surface does not front (the events-tail
/// `head_seq`); every invariant-bearing verb still goes through [`App`].
pub struct ApiState<J, S, C, I, Sc> {
    app: Arc<App<J, S, C, I, Sc>>,
    store: J,
    db_ping: DbPing,
    fork_check_ok: Arc<AtomicBool>,
    names_assigned: bool,
    enrollment: Arc<Mutex<Option<String>>>,
    build: Arc<BuildInfo>,
    /// The authorization request's sealer: one key per process, held in
    /// memory only (ADR-0031 d9 step 1).
    sealer: Arc<dyn HandleSealer>,
    /// The authorize page's client document fetcher (ADR-0031 d5).
    documents: oauth::Documents,
    /// The traffic layer's configuration (ADR-0042).
    traffic: traffic::Traffic,
    /// The web pages, where the composition set providers (ADR-0045 d17).
    web: Option<Arc<web::WebPages>>,
    /// The identity `oml-web` proves to present a capacity credential
    /// (ADR-0070 d2).
    web_client: session::WebClient,
}

impl<J: Clone, S, C, I, Sc> Clone for ApiState<J, S, C, I, Sc> {
    fn clone(&self) -> Self {
        Self {
            app: Arc::clone(&self.app),
            store: self.store.clone(),
            db_ping: Arc::clone(&self.db_ping),
            fork_check_ok: Arc::clone(&self.fork_check_ok),
            names_assigned: self.names_assigned,
            enrollment: Arc::clone(&self.enrollment),
            build: Arc::clone(&self.build),
            sealer: Arc::clone(&self.sealer),
            documents: self.documents.clone(),
            traffic: self.traffic.clone(),
            web: self.web.clone(),
            web_client: self.web_client.clone(),
        }
    }
}

/// The router needs the connection store as well, for the OAuth routes; a
/// composition's store is both ports.
impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// The two rings plus the unversioned probes, as one axum router, built
    /// from [`ROUTES`] so that it routes exactly what that table names, and
    /// `/mcp` beside them; where `oauth.issuer` is set, the [`OAUTH_ROUTES`]
    /// outside both rings ([`oauth`]); only the probes while names are not
    /// assigned.
    pub fn into_router(self) -> Router {
        let traffic = self.traffic.clone();
        let mut probes = Router::new();
        let mut rings = Router::new();
        for &(method, path) in ROUTES {
            let handler = Self::handler(method, path);
            if PROBES.contains(&path) {
                probes = probes.route(path, handler);
            } else {
                rings = rings.route(path, handler);
            }
        }
        let router = if self.names_assigned {
            let router = probes
                .merge(rings)
                .route("/mcp", post(Self::mcp).get(mcp::mcp_get));
            let router = self.with_oauth_routes(router);
            // A capacity credential is resolved, or refused, before any
            // handler reads `Authorization` (ADR-0070 d2).
            self.with_web_routes(router)
                .layer(axum::middleware::from_fn_with_state(
                    self.clone(),
                    Self::session_gate,
                ))
                .with_state(self)
        } else {
            probes.fallback(names_unassigned).with_state(self)
        };
        // Outside every route, the fallback and axum's own 404 and 405
        // included (ADR-0042 d1, d4); the panic guard just inside it, so a
        // handler's panic is a named 500, never a dropped connection.
        traffic::guard(router, traffic)
    }

    /// The handler of one [`ROUTES`] entry. A table entry with no handler
    /// here panics when the router is built, which every router test does.
    fn handler(method: &str, path: &str) -> MethodRouter<Self> {
        match (method, path) {
            ("GET", "/healthz") => get(healthz),
            ("GET", "/readyz") => get(Self::readyz),
            ("POST", "/v1/memories") => post(Self::submit),
            ("GET", "/v1/memories") => get(Self::list_memories),
            ("POST", "/v1/memories/recall") => post(Self::recall),
            ("GET", "/v1/memories/{artifact_id}") => get(Self::get_memory),
            ("POST", "/v1/memories/{artifact_id}/supersede") => post(Self::supersede),
            ("POST", "/v1/memories/{artifact_id}/source-check") => post(Self::source_check),
            ("GET", "/v1/guardrails") => get(Self::guardrails),
            ("POST", "/v2/review/next") => post(Self::review_next),
            ("POST", "/v2/review/verdicts") => post(Self::review_verdicts),
            ("GET", "/v2/review/held") => get(Self::review_held),
            ("POST", "/v2/review/verdicts/bulk") => post(Self::review_bulk),
            ("GET", "/v2/review/queue") => get(Self::review_queue),
            ("GET", "/v2/keys") => get(Self::list_keys),
            ("POST", "/v2/keys") => post(Self::issue_key),
            ("GET", "/v2/keys/{key}") => get(Self::get_key),
            ("POST", "/v2/keys/{key}/denials") => post(Self::deny_key),
            ("POST", "/v2/keys/{key}/connect-codes") => post(Self::mint_connect_code),
            ("POST", "/v2/keys/{key}/connections/{connection_id}/revoke") => {
                post(Self::revoke_connection)
            }
            ("GET", "/v2/whoami") => get(Self::whoami),
            ("POST", "/v2/crews") => post(Self::create_crew),
            ("GET", "/v2/crews") => get(Self::list_crews),
            ("GET", "/v2/crews/{crew}") => get(Self::get_crew),
            ("POST", "/v2/crews/{crew}/update") => post(Self::update_crew),
            ("POST", "/v2/workareas") => post(Self::create_workarea),
            ("GET", "/v2/workareas/{workarea}") => get(Self::get_workarea),
            ("GET", "/v2/workareas/{workarea}/tree") => get(Self::workarea_tree),
            ("GET", "/v2/workareas/{workarea}/offers") => get(Self::workarea_offers),
            ("POST", "/v2/workareas/{workarea}/crew") => post(Self::set_workarea_crew),
            ("POST", "/v2/workareas/{workarea}/mode") => post(Self::set_workarea_mode),
            ("POST", "/v2/workareas/{workarea}/federation") => post(Self::set_workarea_federation),
            ("POST", "/v2/workareas/{workarea}/transitions") => post(Self::propose_transition),
            ("POST", "/v2/workareas/{workarea}/transitions/{transition_id}/ratify") => {
                post(Self::ratify_transition)
            }
            ("GET", "/v2/edges") => get(Self::list_edges),
            ("POST", "/v2/edges") => post(Self::propose_edge),
            ("POST", "/v2/edges/{edge_id}/ratify") => post(Self::ratify_edge),
            ("GET", "/v2/events") => get(Self::events_tail),
            ("POST", "/v2/bootstrap/claim") => post(Self::bootstrap_claim),
            ("GET", "/v2/web/emails") => get(Self::list_web_emails),
            ("GET", "/v2/web/users") => get(Self::list_web_users),
            ("POST", "/v2/web/emails") => post(Self::allow_web_email),
            ("POST", "/v2/web/emails/remove") => post(Self::remove_web_email),
            ("POST", "/v2/web/users/{user}/capacities") => post(Self::grant_capacities),
            ("POST", "/v2/web/users/{user}/quota") => post(Self::set_quota),
            ("POST", "/v2/web/users/{user}/disable") => post(Self::disable_user),
            ("POST", "/v2/web/users/{user}/deletion/close") => post(Self::close_deletion),
            ("POST", "/v2/web/session/end") => post(Self::end_session),
            ("GET", "/v2/web/export") => get(Self::export_web_person),
            ("POST", "/v2/web/deletion-requests") => post(Self::request_deletion),
            ("POST", "/v2/admin/memories/search") => post(Self::admin_search_route),
            ("GET", "/v2/admin/memories/{artifact_id}") => get(Self::admin_read_route),
            ("GET", "/v2/admin/rescan") => get(Self::rescan_route),
            ("POST", "/v2/admin/rescan") => post(Self::rescan_record_route),
            ("GET", "/v2/search") => get(Self::search_settings),
            ("POST", "/v2/search/default") => post(Self::set_search_default),
            ("GET", "/v2/federation") => get(Self::federation_settings),
            ("POST", "/v2/federation") => post(Self::set_federation_policy),
            ("GET", "/v2/usage/settings") => get(Self::usage_settings),
            ("POST", "/v2/usage/settings") => post(Self::set_usage_settings),
            ("GET", "/v2/usage/events") => get(Self::usage_events),
            ("POST", "/v2/usage/eval") => post(Self::usage_eval),
            _ => panic!("{method} {path} is in ROUTES with no handler"),
        }
    }
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `fork_check_ok: false` means the startup fork check (ADR-0002 d7) did
    /// not verify: `/readyz` answers 503 and every write answers
    /// `system.unavailable` (http-api.md "Health").
    pub fn new(
        app: Arc<App<J, S, C, I, Sc>>,
        store: J,
        db_ping: DbPing,
        fork_check_ok: bool,
        enrollment_token: Option<String>,
    ) -> Self {
        Self {
            app,
            store,
            db_ping,
            fork_check_ok: Arc::new(AtomicBool::new(fork_check_ok)),
            names_assigned: true,
            enrollment: Arc::new(Mutex::new(enrollment_token)),
            build: Arc::new(BuildInfo::unknown()),
            sealer: Arc::new(oauth::seal::AeadSealer::generate()),
            documents: oauth::Documents::default(),
            traffic: traffic::Traffic::default(),
            web: None,
            web_client: session::WebClient::default(),
        }
    }

    /// The fetcher of client ID metadata documents the authorize page
    /// reads through (ADR-0031 d5); without one every document fetch
    /// fails, so a document client is `oauth.client_invalid` and the page
    /// makes no outbound call.
    #[must_use]
    pub fn with_document_fetcher<F: ClientDocumentFetcher + 'static>(mut self, fetcher: F) -> Self {
        self.documents = oauth::Documents::new(fetcher);
        self
    }

    /// The traffic layer's configuration: the trusted proxies the request
    /// line reads the client address through (ADR-0042 d7). Without it no
    /// proxy is trusted.
    #[must_use]
    pub fn with_traffic(mut self, traffic: traffic::Traffic) -> Self {
        self.traffic = traffic;
        self
    }

    /// The web pages over their providers (ADR-0045 d1, d17): routed where
    /// `oauth.issuer` is also set. Without it no web route exists.
    #[must_use]
    pub fn with_web(mut self, pages: web::WebPages) -> Self {
        self.web = Some(Arc::new(pages));
        self
    }

    /// The identity `oml-web` proves when it presents a capacity
    /// credential: `web.client_secret`, `web.client_subject`, or both
    /// (ADR-0070 d2). Without it every capacity credential is refused.
    #[must_use]
    pub fn with_web_client(mut self, web_client: session::WebClient) -> Self {
        self.web_client = web_client;
        self
    }

    /// The build `/readyz` reports; without it every build field is `unknown`.
    #[must_use]
    pub fn with_build(mut self, build: BuildInfo) -> Self {
        self.build = Arc::new(build);
        self
    }

    /// The composition's startup names check. `false` means the lead's
    /// start, which names every row that predates names before it serves
    /// (ADR-0026 d11; http-api.md "Rows created before names"), left a row
    /// with no name: `/readyz` reports `names_assigned` not ready and every
    /// route but the two probes answers `system.unavailable` with that
    /// reason, so no request reaches a row with no name. Without the call
    /// every row is taken as named.
    #[must_use]
    pub fn with_names_assigned(mut self, names_assigned: bool) -> Self {
        self.names_assigned = names_assigned;
        self
    }

    /// Writes are refused while the fork check has not verified the stored
    /// head (errors.md `system.unavailable`).
    fn require_writes_open(&self) -> Result<(), ApiError> {
        if self.fork_check_ok.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(ApiError::unavailable("fork_check_failed"))
        }
    }

    // -- handlers -------------------------------------------------------------

    async fn readyz(State(st): State<Self>, rid: Rid) -> Response {
        let postgres_ok = (st.db_ping)().await;
        let fork_ok = st.fork_check_ok.load(Ordering::SeqCst);
        let ready = postgres_ok && fork_ok && st.names_assigned;
        let body = json!({
            "status": if ready { "ok" } else { "unavailable" },
            "checks": {
                "postgres": if postgres_ok { "ok" } else { "unreachable" },
                "fork_check": if fork_ok { "ok" } else { "failed" },
                "names_assigned": if st.names_assigned { "ok" } else { "unassigned" },
            },
            // Ready or not, the probe names the build it belongs to.
            "build": st.build.to_json(),
            // The lead's oauth.issuer, or null: doctor reads whether
            // connections are on here, off the route it checks (ADR-0031 d14).
            "oauth_issuer": st.app.oauth_issuer().map(|issuer| issuer.as_str()),
            // The web pages' providers, or null where the pages are off:
            // doctor's web line reads it (ADR-0045 d18); their failed
            // exchanges are the metrics listener's (ADR-0054 d1).
            "web": st.web.as_ref().filter(|_| st.app.oauth_issuer().is_some()).map(|w| w.readyz()),
            // Which client identity oml-web proves to present a capacity
            // credential, `none` where neither is set: doctor warns then,
            // and every capacity credential is refused (ADR-0070 d2).
            "web_client": st.web_client.identity(),
            // The claim formats this build serves and the adapters that
            // derive their search text: doctor's formats line reads them
            // (ADR-0056 d14).
            "formats": {
                "enabled": st.app.formats().allowed(),
                "adapters": st.app.formats().adapters(),
            },
            // The scanner's rule set, how many rules it loaded and whether
            // the personal-data rules are on: doctor's scan line reads them
            // (ADR-0065 d11). Its findings are the metrics listener's.
            "scan": st.app.scanner_info().map(|i| json!({
                "version": i.version,
                "rules": i.rules,
                "pii": i.pii,
            })),
            // The served search pipelines and the deployment's default:
            // doctor's search line reads them (ADR-0049 d8).
            "search": {
                "enabled": st.app.search_config().enabled().iter().map(|p| p.as_str()).collect::<Vec<_>>(),
                "default": st.app.search_config().default_pipeline().as_str(),
                // The reranker the reranking pipelines call and its policy:
                // doctor's rerank line reads them (ADR-0053 d9). The stages
                // it skipped are counted on the metrics listener,
                // oml_reranks_total, never here (ADR-0054 d1).
                "rerank": st.app.reranker_model().map(|model| {
                    let policy = st.app.search_config().rerank();
                    json!({
                        "model": model,
                        "depth": policy.depth,
                        "floor": policy.floor.map(relevance),
                    })
                }),
                // The learned sparse encoder the sparse pipelines read, or
                // null: doctor's search_sparse_model line reads it
                // (ADR-0052 d16). Its backlog is per tenant, read by an
                // admin through GET /v2/search, and summed on the metrics
                // listener, never here.
                "sparse": st.app.sparse_model().map(|model| json!({ "model": model })),
                // No count of any kind: recalls, skipped reranks and failed
                // sign-ins are every tenant's traffic, and this probe
                // answers anyone; they are the metrics listener's
                // (ADR-0042 d11; ADR-0052 d12; ADR-0054 d1).
            },
        });
        let status = if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        };
        finish(&rid, Ok(json_response(status, &body)))
    }

    async fn submit(State(st): State<Self>, rid: Rid, headers: HeaderMap, body: Bytes) -> Response {
        finish(&rid, Self::submit_inner(&st, &headers, &body, None).await)
    }

    async fn submit_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
        connection: Option<&Caller>,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, connection)?;
        refuse_workarea_header(headers, "/workarea")?;
        let idem_key = idempotency_key(headers)?;
        let req: SubmitBody = parse_json(headers, body)?;
        // kind defaults to memory, and an omitted verb is the app's to
        // choose from derives_from (ADR-0039 d3).
        let kind = parse_kind(req.kind.as_deref().unwrap_or("memory"))?;
        let verb = match req.verb.as_deref() {
            None => None,
            Some("remember") => Some(SubmitVerb::Remember),
            // learn asserts a derived claim: lineage is required, never
            // optional (errors.md submission.lineage_required).
            Some("learn") if req.derives_from.is_empty() => {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "submission.lineage_required",
                    "learn asserts a derived claim, so its lineage is required; a derived conclusion without nameable lineage is remember with kind derived_fact.",
                    "verb learn with an empty or omitted derives_from",
                ));
            }
            Some("learn") => Some(SubmitVerb::Learn),
            Some(other) => {
                return Err(ApiError::validation(format!(
                    "verb must be remember or learn, not {other:?}"
                )));
            }
        };
        // The limit counts the NFC body, the form capture stores
        // (http-api.md limits; ADR-0025 d6).
        let text = nfc(&req.body);
        if text.len() > MAX_BODY_BYTES {
            return Err(ApiError::payload_too_large("the claim body"));
        }
        refuse_attachments(&req.attachments)?;
        refuse_simulation(req.simulation_id.as_ref())?;
        let workarea = optional_reference("/workarea", req.workarea.as_deref())?;

        let app_req = SubmitRequest {
            on_similar: parse_on_similar(req.on_similar.as_deref())?,
            idempotency_key: idem_key,
            claim_key: req.claim_key,
            verb,
            kind,
            body: text,
            workarea,
            derives_from: req.derives_from,
            severity: parse_severity(req.severity.as_deref())?,
            rule: req.rule,
            valid_from: req.valid_from,
            valid_until: req.valid_until,
            anchor: parse_anchor(req.anchor.as_ref())?,
            format: req.format,
            title: req.title,
            origin: parse_origin(req.origin.as_ref())?,
        };
        let outcome = retry_chain(|| st.app.submit_memory(acting.bearer(), &app_req))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let r = &outcome.receipt;
        let mut response = json_response(
            StatusCode::ACCEPTED,
            // SubmitAccepted: a submit captures a new claim, never a
            // supersession, so it replaces nothing and retires nothing.
            &json!({
                "artifact_id": r.memory_id,
                "claim_key": r.claim_key,
                "version": r.version,
                "state": r.state,
                "plaintext_digest": r.body_digest,
                "journal_seq": r.journal_seq,
                "workarea_id": r.workarea_id,
                "retirement": false,
                "replaces": Value::Null,
                "similar": similar_json(&r.similar),
            }),
        );
        if outcome.replayed {
            // The stored response, flagged (http-api.md idempotency table).
            response
                .headers_mut()
                .insert("idempotency-replay", HeaderValue::from_static("true"));
        }
        Ok(response)
    }

    async fn list_memories(
        State(st): State<Self>,
        rid: Rid,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => {
                Self::list_memories_inner(&st, &params, &headers, false, None).await
            }
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    /// `GET /v1/memories`: the own-work listing with `own=true`, else the
    /// scoped listing, as memory summaries without text, newest first
    /// (http-api.md search; ADR-0025 d13). The query's `workarea` is the
    /// subtree either reads (ADR-0039 d4, d7). The `include_text` argument
    /// is the agent plugin's: `oml_read`'s listing mode returns each item
    /// with its text, as every `oml_read` mode does, from the same rows
    /// and access the listing reads (ADR-0039 d4). On the HTTP route the
    /// scoped listing serves summaries; the query's `include_text` is
    /// taken only beside `with_own` or `author`, the web console's list
    /// (ADR-0070 d5), and is an unknown parameter without them.
    async fn list_memories_inner(
        st: &Self,
        params: &[(String, String)],
        headers: &HeaderMap,
        include_text: bool,
        connection: Option<&Caller>,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, connection)?;
        refuse_workarea_header(headers, "workarea")?;
        if params
            .iter()
            .any(|(n, v)| (n == "with_own" && v == "true") || n == "author")
        {
            // as_of reads the scoped listing alone (ADR-0059 d19).
            if params.iter().any(|(n, _)| n == "as_of") {
                return Err(ApiError::validation(
                    "as_of reads the scoped listing: it does not combine with own, with_own, author or tip",
                )
                .with_details(json!({ "field": "as_of" })));
            }
            return Self::list_with_own_inner(st, params, &acting, include_text).await;
        }
        let req = list_request(params)?;
        let page = st
            .app
            .list_memories(acting.bearer(), &req)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "items": page.items.iter().map(|m| memory_json(m, include_text)).collect::<Vec<_>>(),
                "next_cursor": page.next_cursor,
            }),
        ))
    }

    /// `GET /v1/memories` with `with_own=true` or `author`: the scoped
    /// listing at `universe` (or at `workarea`) and the caller's own rows,
    /// every key naming its user's for a person's web session key, merged
    /// newest first under one cursor, a row in both once; `author`, one of
    /// those keys by name, drops the scoped part, and a key that is not the
    /// caller's is `access.not_found` (ADR-0045 d22, d23; ADR-0070 d5).
    async fn list_with_own_inner(
        st: &Self,
        params: &[(String, String)],
        acting: &Acting<'_>,
        mut include_text: bool,
    ) -> Result<Response, ApiError> {
        let mut req = oml_app::app::WebListRequest {
            limit: PAGE_LIMIT_DEFAULT,
            ..oml_app::app::WebListRequest::default()
        };
        let mut seen = HashSet::new();
        for (name, value) in params {
            if !seen.insert(name.as_str()) {
                return Err(ApiError::validation(format!(
                    "the query parameter {name} is given more than once"
                )));
            }
            match name.as_str() {
                "with_own" if value == "true" || value == "false" => {}
                "with_own" => return Err(ApiError::validation("with_own must be true or false")),
                // Each item with its text, as the console's list shows it
                // (ADR-0070 d5).
                "include_text" if value == "true" => include_text = true,
                "include_text" if value == "false" => {}
                "include_text" => {
                    return Err(ApiError::validation("include_text must be true or false")
                        .with_details(json!({ "field": "include_text" })));
                }
                "author" => {
                    req.author = Some(Name::parse(value).map_err(|e| {
                        ApiError::validation(format!("author {value:?} is not a key's name: {e}"))
                            .with_details(json!({ "field": "author" }))
                    })?);
                }
                "workarea" => req.workarea = Some(reference("workarea", value)?),
                "state" => {
                    req.state =
                        Some(oml_app::types::AdminState::parse(value).ok_or_else(|| {
                            ApiError::validation(
                                "state is held, promoted, rejected, superseded or demoted",
                            )
                            .with_details(json!({ "field": "state" }))
                        })?);
                }
                "origin_kinds" => {
                    let kinds = origin_grammar::parse_kinds(value.split(',')).map_err(|why| {
                        ApiError::validation(format!("origin_kinds: {why}"))
                            .with_details(json!({ "field": "origin_kinds" }))
                    })?;
                    if kinds.len() != 1 {
                        return Err(ApiError::validation(
                            "origin_kinds names one kind with with_own or author",
                        )
                        .with_details(json!({ "field": "origin_kinds" })));
                    }
                    req.origin = kinds.first().copied();
                }
                "limit" => req.limit = page_limit(value)?,
                "cursor" => req.cursor = Some(value.clone()),
                other => {
                    return Err(ApiError::validation(format!(
                        "the query parameter {other:?} does not combine with with_own or author"
                    )));
                }
            }
        }
        let caller = st
            .app
            .caller(acting.bearer())
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let page = st
            .app
            .list_memories_with_own(&caller, &req)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "items": page.items.iter().map(|m| memory_json(m, include_text)).collect::<Vec<_>>(),
                "next_cursor": page.next_cursor,
            }),
        ))
    }

    async fn get_memory(
        State(st): State<Self>,
        rid: Rid,
        Path(artifact_id): Path<String>,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => match get_as_of(&params) {
                Ok(as_of) => {
                    Self::get_memory_inner(&st, &artifact_id, as_of.as_ref(), &headers, None).await
                }
                Err(e) => Err(e),
            },
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    /// `GET /v1/memories/{artifact_id}` (http-api.md get). Get resolves no
    /// workarea, so an `X-OML-Workarea` sent with it is refused, not
    /// ignored, and names no successor (ADR-0025 d13, ADR-0039 d7).
    /// A read's `feedback` member, checked and queued (ADR-0054 d6); the
    /// read it rides on runs only when this answers.
    pub(crate) async fn feedback_inner(
        st: &Self,
        headers: &HeaderMap,
        connection: Option<&Caller>,
        feedback: &Value,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, connection)?;
        let feedback = usage::parse_feedback(feedback)?;
        st.app
            .accept_feedback(acting.bearer(), &feedback)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &json!({})))
    }

    async fn get_memory_inner(
        st: &Self,
        artifact_id: &str,
        as_of: Option<&EventTime>,
        headers: &HeaderMap,
        connection: Option<&Caller>,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, connection)?;
        if headers.contains_key(WORKAREA_HEADER) {
            return Err(ApiError::validation(
                "X-OML-Workarea is refused on get: get resolves no workarea",
            )
            .with_details(json!({ "field": "X-OML-Workarea" })));
        }
        // Ids are opaque; one that can name no row is absent (access.not_found).
        let id = Uuid::parse_str(artifact_id).map_err(|_| ApiError::not_found())?;
        let memory = st
            .app
            .get_memory_as_of(acting.bearer(), id, as_of)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &memory_json(&memory, true)))
    }

    async fn recall(State(st): State<Self>, rid: Rid, headers: HeaderMap, body: Bytes) -> Response {
        finish(&rid, Self::recall_inner(&st, &headers, &body, None).await)
    }

    async fn recall_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
        connection: Option<&Caller>,
    ) -> Result<Response, ApiError> {
        // Read-shaped POST: no Idempotency-Key (http-api.md idempotency).
        let acting = acting(headers, connection)?;
        refuse_workarea_header(headers, "/workarea")?;
        // The body is closed: a field the server does not serve is refused,
        // never dropped (ADR-0025 d16), and a v0.3.0 spelling names its
        // successor (ADR-0039 d7).
        let req: RecallBody = parse_json_renamed(headers, body, |body| {
            refuse_renamed(
                body,
                "",
                &[
                    ("include_bodies", "include_text"),
                    ("include_own_held", "include_unreviewed"),
                ],
            )
        })?;
        let k = req.k.unwrap_or(8);
        if !(1..=100).contains(&k) {
            return Err(ApiError::validation("k must be between 1 and 100"));
        }
        // The vocabulary filter is unbuilt: refused by name, never answered
        // unfiltered (retrieval.md; ADR-0025 d16).
        if req.terms.is_some() {
            return Err(ApiError::validation(
                "terms on recall is not built: the vocabulary filter is refused, never ignored",
            ));
        }
        if req.expand_terms.is_some() {
            return Err(ApiError::validation(
                "expand_terms on recall is not built: the vocabulary filter is refused, never ignored",
            ));
        }
        let kinds = parse_hit_kinds(req.kinds.as_deref())?;
        // A budget is an integer ≥ 1, and one RFC 8785 digests exactly: an
        // integer above 2^53 − 1 has no exact I-JSON form (retrieval.md, the
        // read log; ADR-0025 d7, d9).
        if req
            .max_tokens
            .is_some_and(|m| !(1..=MAX_CHAIN_INTEGER).contains(&m))
        {
            return Err(ApiError::validation(
                "max_tokens must be an integer between 1 and 2^53 - 1",
            ));
        }
        let include_text = req.include_text.unwrap_or(true);
        let match_mode = match req.match_mode.as_deref() {
            None | Some("any") => MatchMode::Any,
            Some("all") => MatchMode::All,
            Some(other) => {
                return Err(ApiError::validation(format!(
                    "match must be any or all, not {other:?}"
                ))
                .with_details(json!({ "field": "/match" })));
            }
        };
        let workarea = optional_reference("/workarea", req.workarea.as_deref())?;
        let sources = parse_sources(req.sources.as_ref())?;
        // Feedback on the key's last recall rides this one: refused by its
        // field before the recall runs, never changing what it answers
        // (ADR-0054 d6).
        if let Some(feedback) = &req.feedback {
            Self::feedback_inner(st, headers, connection, feedback).await?;
        }
        let result = st
            .app
            .recall(
                acting.bearer(),
                &RecallRequest {
                    timings: req.timings.unwrap_or(false),
                    query: req.query.clone(),
                    k,
                    workarea,
                    kinds,
                    include_text,
                    // Absent, the scope decides: true inside an in-progress
                    // subtree, false elsewhere (ADR-0030 d4).
                    include_unreviewed: req.include_unreviewed,
                    max_tokens: req.max_tokens,
                    search: req.search.clone(),
                    match_mode,
                    include_expired: req.include_expired.unwrap_or(false),
                    now: req.now.clone(),
                    anchor: anchor_filter(
                        req.anchor_repo.as_deref(),
                        req.anchor_path_prefix.as_deref(),
                        req.origin_kinds.as_deref(),
                        "/",
                    )?,
                    sources,
                    as_of: parse_as_of(req.as_of.as_deref(), "/as_of")?,
                },
            )
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        // Steering items are unranked and carry no score; each hit carries
        // the store's (retrieval.md, the recall-ordering contract).
        let steering: Vec<Value> = result
            .steering
            .iter()
            .map(|m| memory_json(m, include_text))
            .collect();
        let hits: Vec<Value> = result
            .hits
            .iter()
            .map(|hit| {
                let mut value = json!({
                    "memory": memory_json(&hit.served, include_text),
                    // A reranked hit's score is its relevance, printed as set
                    // (ADR-0053 d7).
                    "score": if hit.rerank.is_some() { relevance(hit.score) } else { f64::from(hit.score) },
                    "matched": {
                        "terms": hit.matched,
                        "of": result.terms.len(),
                        "via": hit.via.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
                        "rerank": hit.rerank.map(relevance),
                        "rank_before": hit.rank_before,
                        // The learned sparse tokens that scored the hit,
                        // each with its contribution and whether the
                        // encoder added it (ADR-0052 d17).
                        "tokens": hit.tokens.iter().map(|t| json!({
                            "token": t.token,
                            "weight": f64::from(t.weight),
                            "expanded": t.expanded,
                        })).collect::<Vec<_>>(),
                        "expanded": hit.tokens.iter().filter(|t| t.expanded).map(|t| t.token.as_str()).collect::<Vec<_>>(),
                    },
                });
                // A federated answer's hit says whether its workarea is the
                // scope's or another team's (ADR-0072 d8).
                if let (Some(via), Some(obj)) = (hit.source, value.as_object_mut()) {
                    obj.insert("via".to_owned(), Value::from(via.as_str()));
                }
                value
            })
            .collect();
        // What ran and what the query was read as (ADR-0049 d5, d6).
        let mut response = json!({
            "recall_id": result.recall_id,
            "steering": steering,
            "hits": hits,
            "query_digest": result.query_digest,
            "search": {
                "pipeline": result.pipeline.as_str(),
                "match": if match_mode == MatchMode::All { "all" } else { "any" },
                "terms": result.terms,
                // The tenant's default the deployment does not serve names the
                // fallback when both apply: each hit's `matched.of` still
                // shows an any-term match (ADR-0049 d5, d15).
                "fallback": if result.tenant_default_unserved.is_some() {
                    Some("tenant_default_unserved")
                } else {
                    result.fallback.then_some("any_term")
                },
                "did_you_mean": result.did_you_mean,
                "rerank": result.rerank.as_str(),
                "rerank_reason": result.rerank.reason(),
                "floor": result.floor.map(relevance),
                "below_floor": result.below_floor,
                "access": result.access.as_str(),
                "pruned": result.pruned,
                // The date window the query named (proposed ADR-0059 d6).
                "window": result.window.as_ref().map(|w| json!({
                    "from": w.from_us.map(fixed_width),
                    "until": w.until_us.map(fixed_width),
                    "phrase": w.phrase,
                })),
            },
        });
        // `search.sources` under wide or a list, and `search.widen` on a
        // scope answer whose precomputed signal finds a richer source
        // (ADR-0072 d8; #244).
        if let Some(search) = response.get_mut("search").and_then(Value::as_object_mut) {
            if let Some(report) = &result.sources {
                search.insert("sources".to_owned(), sources_json(report));
            }
            if let Some(widen) = &result.widen {
                search.insert(
                    "widen".to_owned(),
                    json!({
                        "sources": "wide",
                        "candidates": widen.candidates,
                        "matches_at_least": widen.matches_at_least,
                    }),
                );
            }
        }
        // A signal this recall asked for is built off the request, once
        // its answer is composed (ADR-0072 d12, as built by #244).
        if st.app.widen_signals_wanted() {
            let app = Arc::clone(&st.app);
            // One refresh at a time in the process, one build at a time,
            // tenants in turn; a build past its timeout is stopped, counted
            // and marked, and the next goes on.
            tokio::spawn(async move {
                app.refresh_widen_signals_with(tokio::time::sleep).await;
            });
        }
        // The budget object is present if and only if max_tokens was sent
        // (retrieval.md token budgets; ADR-0025 d7).
        if let (Some(budget), Some(obj)) = (&result.budget, response.as_object_mut()) {
            obj.insert("budget".to_owned(), budget_json(budget));
        }
        // The timings block is present if and only if timings was sent
        // true, which only a deployment serving them admits (ADR-0049 d14).
        if req.timings == Some(true)
            && let Some(obj) = response.as_object_mut()
        {
            obj.insert("timings".to_owned(), timings_json(&result.timings));
        }
        Ok(json_response(StatusCode::OK, &response))
    }

    /// `GET /v1/guardrails`: the applicable guardrail set at the scope node,
    /// the query's `workarea` or the key's default, under the key's reader
    /// access and the `guardrail_scope` its issuance set (proposed ADR-0037
    /// d4, d5, d7). The query takes `workarea` alone: a scope, or any other
    /// parameter, is refused naming it, so nothing the client sends changes
    /// the scope (d4, "never on the client").
    async fn guardrails(
        State(st): State<Self>,
        rid: Rid,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => Self::guardrails_inner(&st, &params, &headers, None).await,
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    pub(crate) async fn guardrails_inner(
        st: &Self,
        params: &[(String, String)],
        headers: &HeaderMap,
        connection: Option<&Caller>,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, connection)?;
        refuse_workarea_header(headers, "workarea")?;
        let mut workarea = None;
        for (name, value) in params {
            match name.as_str() {
                "workarea" if workarea.is_none() => workarea = Some(reference("workarea", value)?),
                "workarea" => {
                    return Err(ApiError::validation(
                        "the query parameter workarea is given more than once",
                    ));
                }
                other => {
                    return Err(ApiError::validation(format!(
                        "{other} is not a parameter of this read: it takes workarea alone, and the guardrail scope is the key's, set at issuance"
                    ))
                    .with_details(json!({ "field": other })));
                }
            }
        }
        let set = st
            .app
            .guardrails(acting.bearer(), &GuardrailRequest { workarea })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &guardrail_set_json(&set)))
    }

    async fn source_check(
        State(st): State<Self>,
        rid: Rid,
        Path(artifact_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::source_check_inner(&st, &artifact_id, &headers, &body).await,
        )
    }

    /// `POST /v1/memories/{artifact_id}/source-check`: a journalled
    /// statement that a promoted version's anchored span was compared with
    /// its source at a commit; it answers the version's new `stale` (ADR-0057
    /// d5, d18). The body is closed.
    async fn source_check_inner(
        st: &Self,
        artifact_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let idem_key = idempotency_key(headers)?;
        let memory_id = Uuid::parse_str(artifact_id).map_err(|_| ApiError::not_found())?;
        let req: SourceCheckBody = parse_json(headers, body)?;
        let result = CheckResult::parse(&req.result).ok_or_else(|| {
            ApiError::validation("result must be fresh, equivalent, moved, changed or removed")
                .with_details(json!({ "field": "/result" }))
        })?;
        let app_req = SourceCheckRequest {
            idempotency_key: idem_key,
            memory_id,
            rev: req.rev,
            result,
            hash: req.hash,
            path: req.path,
            fragment: req.fragment,
        };
        let (receipt, replayed) = retry_chain(|| st.app.source_check(acting.bearer(), &app_req))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let check = json!({
            "rev": receipt.rev,
            "at": receipt.at,
            "result": receipt.result,
            "moved_to": receipt.moved_path.as_ref().map(|path| json!({
                "path": path,
                "fragment": receipt.moved_fragment,
            })),
        });
        if !replayed {
            st.traffic.metrics().source_check(result.as_str());
        }
        let stale = CheckResult::parse(&receipt.result).and_then(CheckResult::stale);
        let mut response = json_response(
            StatusCode::OK,
            &json!({
                "artifact_id": receipt.memory_id,
                "stale": stale,
                "source_checked": check,
                "journal_seq": receipt.journal_seq,
            }),
        );
        if replayed {
            response
                .headers_mut()
                .insert("idempotency-replay", HeaderValue::from_static("true"));
        }
        Ok(response)
    }

    async fn supersede(
        State(st): State<Self>,
        rid: Rid,
        Path(artifact_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            // The ring supersedes a target of any kind (ADR-0025 d12).
            Self::supersede_inner(&st, &artifact_id, &headers, &body, false, None).await,
        )
    }

    /// `POST /v1/memories/{artifact_id}/supersede`: a held successor that
    /// names what it replaces; the target stays the tip until ratification
    /// (http-api.md supersede; ADR-0025 d10, d11). The body is closed: a
    /// field the server does not serve is refused, never dropped (d16).
    /// `refuse_vocabulary_target` is set by `oml_write`, whose input
    /// carries no vocabulary fields (d12).
    async fn supersede_inner(
        st: &Self,
        artifact_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
        refuse_vocabulary_target: bool,
        connection: Option<&Caller>,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, connection)?;
        refuse_workarea_header(headers, "/workarea")?;
        let idem_key = idempotency_key(headers)?;
        let target_id = Uuid::parse_str(artifact_id).map_err(|_| ApiError::not_found())?;
        let req: SupersedeBody = parse_json(headers, body)?;
        let text = nfc(&req.body);
        if text.len() > MAX_BODY_BYTES {
            return Err(ApiError::payload_too_large("the claim body"));
        }
        let kind = req.kind.as_deref().map(parse_kind).transpose()?;
        refuse_attachments(&req.attachments)?;
        refuse_simulation(req.simulation_id.as_ref())?;
        // Vocabulary successors are unbuilt: their fields are parsed so the
        // closed body can name them, and refused by name (ADR-0025 d16).
        for (field, sent) in [
            ("mapping", req.mapping.is_some()),
            ("terms", req.terms.is_some()),
            ("term_status", req.term_status.is_some()),
            ("successors", req.successors.is_some()),
        ] {
            if sent {
                return Err(ApiError::validation(format!(
                    "{field} on supersede is not built: vocabulary successors are outside this slice, so the field is refused, never dropped"
                )));
            }
        }
        let app_req = SupersedeRequest {
            on_similar: parse_on_similar(req.on_similar.as_deref())?,
            idempotency_key: idem_key,
            target_id,
            body: text,
            kind,
            retirement: req.retirement,
            derives_from: req.derives_from,
            reason: req.reason,
            workarea: optional_reference("/workarea", req.workarea.as_deref())?,
            refuse_vocabulary_target,
            severity: parse_severity(req.severity.as_deref())?,
            rule: req.rule,
            valid_from: req.valid_from,
            valid_until: req.valid_until,
            anchor: parse_anchor(req.anchor.as_ref())?,
            origin: parse_origin(req.origin.as_ref())?,
            expected_version: parse_expected_version(req.expected_version)?,
            format: req.format,
            title: req.title,
        };
        let outcome = retry_chain(|| st.app.supersede(acting.bearer(), &app_req))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let r = &outcome.receipt;
        let mut response = json_response(
            StatusCode::ACCEPTED,
            // SubmitAccepted for a supersession: the successor, the version
            // it replaces, and whether it proposes retirement.
            &json!({
                "artifact_id": r.memory_id,
                "claim_key": r.claim_key,
                "version": r.version,
                "state": r.state,
                "workarea_id": r.workarea_id,
                "retirement": r.retirement,
                "replaces": {
                    "artifact_id": r.replaces.artifact_id,
                    "version": r.replaces.version,
                },
                "plaintext_digest": r.body_digest,
                "journal_seq": r.journal_seq,
                "similar": similar_json(&r.similar),
            }),
        );
        if outcome.replayed {
            response
                .headers_mut()
                .insert("idempotency-replay", HeaderValue::from_static("true"));
        }
        Ok(response)
    }

    async fn review_next(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(&rid, Self::review_next_inner(&st, &headers, &body).await)
    }

    async fn review_next_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?; // claiming a serve is a side effect
        let acting = acting(headers, None)?;
        let _idem = idempotency_key(headers)?;
        if !body.is_empty() {
            // The subtree filter is not built: the PoC serves the whole
            // tenant queue, so a `workarea` is refused by name, never
            // ignored, however it is spelled (errors.md `request.validation`;
            // ADR-0026 d4).
            let filter: ReviewNextBody = parse_json(headers, body)?;
            if filter.workarea.is_some() {
                return Err(ApiError::validation(
                    "workarea on POST /v2/review/next is not built: the queue is served whole, \
                     so the filter is refused, never ignored",
                )
                .with_details(json!({ "field": "/workarea" })));
            }
        }
        let served = st
            .app
            .review_next(acting.bearer())
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Review))?;
        match served {
            None => {
                // 204: nothing in scope waits (openapi.yaml reviewNext).
                let mut response = Response::new(Body::empty());
                *response.status_mut() = StatusCode::NO_CONTENT;
                Ok(response)
            }
            Some(memory) => {
                // A supersession is served with the version it replaces
                // whenever the reviewer can read it — the item's `replaces`
                // says so, under the readability test every read applies —
                // and the serve records that version's digest (ADR-0012 d5;
                // ADR-0025 d3, d10).
                let replaced = match memory.replaces {
                    None => None,
                    Some(r) => match st.app.get_memory(acting.bearer(), r.artifact_id).await {
                        Ok(replaced) => Some(replaced),
                        // Unreadable since the item was served: shown as
                        // the item's own read would show it, not at all.
                        Err(AppError::NotFound) => None,
                        Err(e) => return Err(map_app_error(e, ErrorContext::Review)),
                    },
                };
                let mut serve = json!({
                    // The PoC serve binding is the claimed row itself
                    // (review_queue.held_by_key); the id round-trips the
                    // memory id rather than naming a serve table row.
                    "serve_id": format!("srv_{}", memory.memory.memory_id.simple()),
                    "expires_at": system_event_time_plus(SERVE_TTL_SECS).as_str(),
                    "item": memory_json(&memory, true),
                    "served_digests": {
                        "body": memory.memory.body_digest,
                        "attachments": [],
                    },
                });
                if let (Some(replaced), Some(obj)) = (&replaced, serve.as_object_mut()) {
                    obj.insert("replaces".to_owned(), memory_json(replaced, true));
                    obj["served_digests"]["replaces"] = json!(replaced.memory.body_digest);
                }
                Ok(json_response(StatusCode::OK, &serve))
            }
        }
    }

    async fn review_verdicts(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::review_verdicts_inner(&st, &headers, &body).await,
        )
    }

    async fn review_verdicts_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let idem_key = idempotency_key(headers)?;
        let req: VerdictBody = parse_json(headers, body)?;
        let verdict = match req.verdict.as_str() {
            "approve" => Verdict::Approve,
            "reject" => Verdict::Reject,
            other => {
                return Err(ApiError::validation(format!(
                    "verdict must be approve or reject, not {other:?}"
                )));
            }
        };
        if verdict == Verdict::Reject && req.reason.as_deref().unwrap_or("").is_empty() {
            return Err(ApiError::validation("reason is required to reject"));
        }
        // Omitted, the serve is the reviewer key's one claimed item, resolved
        // only after the idempotency record is looked up (ADR-0039 d10).
        let serve = match req.serve_id.as_deref() {
            None => None,
            Some(serve_id) => Some(NamedServe {
                serve_id: serve_id.to_owned(),
                memory_id: serve_id
                    .strip_prefix("srv_")
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| ApiError::serve_invalid("unknown"))?,
            }),
        };
        let request = VerdictRequest {
            serve,
            verdict,
            reason: req.reason.clone(),
            idempotency_key: idem_key,
        };
        let outcome = retry_chain(|| st.app.review_verdict(acting.bearer(), &request))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Review))?;
        let mut response = json_response(
            StatusCode::OK,
            &json!({
                "artifact_id": outcome.memory_id,
                "version": outcome.version,
                "verdict": req.verdict,
                "state": outcome.state.as_str(),
                "ratification_event_id": Value::Null,
                "journal_seq": outcome.journal_seq,
            }),
        );
        if outcome.replayed {
            response
                .headers_mut()
                .insert("idempotency-replay", HeaderValue::from_static("true"));
        }
        Ok(response)
    }

    /// `GET /v2/review/held`: the held rows this person's reviewer key may
    /// judge, with their text, a page at a time (ADR-0055 d2).
    async fn review_held(
        State(st): State<Self>,
        rid: Rid,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => Self::review_held_inner(&st, &params, &headers).await,
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    async fn review_held_inner(
        st: &Self,
        params: &[(String, String)],
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        if let Some((_, ids)) = params.iter().find(|(n, _)| n == "ids") {
            if params.len() > 1 {
                return Err(ApiError::validation(
                    "ids names its rows, so it takes no other query parameter",
                ));
            }
            return Self::review_named_inner(st, ids, headers).await;
        }
        let acting = acting(headers, None)?;
        let mut seen = HashSet::new();
        let mut req = HeldListingRequest {
            limit: HELD_PAGE_DEFAULT,
            ..HeldListingRequest::default()
        };
        for (name, value) in params {
            if !seen.insert(name.as_str()) {
                return Err(ApiError::validation(format!(
                    "the query parameter {name} is given more than once"
                )));
            }
            match name.as_str() {
                "workarea" => req.workarea = Some(reference("workarea", value)?),
                "author_key" => req.author_key = Some(reference("author_key", value)?),
                "claim_key_prefix" => req.claim_key_prefix = Some(value.clone()),
                "limit" => req.limit = page_limit(value)?,
                "cursor" => req.cursor = Some(value.clone()),
                other => {
                    return Err(ApiError::validation(format!(
                        "unknown query parameter {other:?}"
                    )));
                }
            }
        }
        let page = st
            .app
            .review_held(acting.bearer(), &req)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Review))?;
        Ok(json_response(StatusCode::OK, &held_page_json(&page)))
    }

    /// `GET /v2/review/held?ids=`: each named row as the held listing
    /// serves it while it is still held and still the caller's to judge,
    /// else `null`, in the order named; the bulk confirmation reads it
    /// (ADR-0055 d10; ADR-0070 d5). A row the caller cannot judge is
    /// `null`, never named otherwise.
    async fn review_named_inner(
        st: &Self,
        ids: &str,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, None)?;
        let ids: Vec<Uuid> = ids
            .split(',')
            .map(|id| Uuid::parse_str(id.trim()))
            .collect::<Result<_, _>>()
            .map_err(|_| {
                ApiError::validation("ids is a comma-separated list of memory ids")
                    .with_details(json!({ "field": "ids" }))
            })?;
        if ids.is_empty() || ids.len() > oml_app::types::BULK_VERDICT_MAX {
            return Err(ApiError::validation(format!(
                "ids names 1 to {} rows, as one bulk verdict judges",
                oml_app::types::BULK_VERDICT_MAX
            ))
            .with_details(json!({ "field": "ids" })));
        }
        let named = st
            .app
            .review_named(acting.bearer(), &ids)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Review))?;
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "items": named.iter().map(|(id, served)| json!({
                    "id": id,
                    "item": served.as_ref().map(|m| memory_json(m, true)),
                })).collect::<Vec<_>>(),
            }),
        ))
    }

    /// `POST /v2/review/verdicts/bulk`: one verdict per named id, each
    /// judged and journalled as a single verdict, answered per item
    /// (ADR-0055 d3, d4).
    async fn review_bulk(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(&rid, Self::review_bulk_inner(&st, &headers, &body).await)
    }

    async fn review_bulk_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let idem_key = idempotency_key(headers)?;
        let req: BulkBody = parse_json(headers, body)?;
        let verdict = match req.verdict.as_str() {
            "approve" => Verdict::Approve,
            "reject" => Verdict::Reject,
            other => {
                return Err(ApiError::validation(format!(
                    "verdict must be approve or reject, not {other:?}"
                ))
                .with_details(json!({ "field": "/verdict" })));
            }
        };
        let artifact_ids = req
            .artifact_ids
            .iter()
            .map(|id| {
                Uuid::parse_str(id).map_err(|_| {
                    ApiError::validation(format!("artifact_ids holds {id:?}, which is not an id"))
                        .with_details(json!({ "field": "/artifact_ids" }))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let request = BulkVerdictRequest {
            verdict,
            artifact_ids,
            reason: req.reason.clone(),
            idempotency_key: idem_key,
        };
        let items = st
            .app
            .review_bulk(acting.bearer(), &request)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Review))?;
        count_bulk(st.traffic.metrics(), &items);
        Ok(json_response(StatusCode::OK, &bulk_json(items)))
    }

    async fn issue_key(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(&rid, Self::issue_key_inner(&st, &headers, &body).await)
    }

    async fn issue_key_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        acting(headers, None)?;
        let idem = idempotency_key(headers)?;
        // The body is closed, nested objects included: a field the server
        // does not serve is refused, never dropped (ADR-0025 d16).
        let req: KeyIssueBody = parse_json_renamed(headers, body, key_issue_renamed)?;
        // Every issuance names its key, under the creation grammar, checked
        // with the rest of the body before step 0 of the issuance order
        // (roles-and-keys.md issuance order; ADR-0026 d5, d9).
        let name = required_creation_name("/name", req.name.as_deref())?;
        // A session key issues only as the person's own issuance, bounded
        // to their root (ADR-0070 d2, d5; ADR-0045 d9, d16).
        if let Some(caller) = session::session_caller() {
            return Self::issue_key_as_session(st, caller, &req, &name, &idem, body).await;
        }
        // Issued keys do not expire yet: a requested expiry is refused by
        // name, since minting the key without it would leave the key its
        // access past the issuer's date (ADR-0025 d2, d16).
        if req.expiry.is_some() {
            return Err(ApiError::validation(
                "expiry on key issuance is not built: an issued key does not expire, so the field is refused, never dropped",
            ));
        }
        if req.tenant.is_some() {
            return Err(ApiError::validation(
                "tenant on key issuance (operator first-admin-key path) is outside the PoC slice",
            ));
        }
        let role = parse_role(&req.role)?;
        let held_by = match req.held_by.as_deref() {
            None => None,
            Some("agent") => Some(HeldBy::Agent),
            Some("human") => Some(HeldBy::Human),
            Some("service") => Some(HeldBy::Service),
            Some(other) => {
                return Err(ApiError::validation(format!(
                    "held_by must be agent, human or service, not {other:?}"
                )));
            }
        };
        // Every workarea and crew reference is an id or a name, each
        // refusal naming its JSON Pointer (errors.md; ADR-0026 d3).
        let grants = req
            .grants
            .iter()
            .enumerate()
            .map(|(i, g)| {
                Ok(GrantRequest {
                    node: reference(&format!("/grants/{i}/workarea"), &g.workarea)?,
                    role: parse_grant_role(&g.role)?,
                    covers: parse_covers(i, g.covers.as_deref())?,
                    deny: g
                        .deny
                        .iter()
                        .enumerate()
                        .map(|(j, d)| {
                            reference(&format!("/grants/{i}/deny/{j}/workarea"), &d.workarea)
                        })
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        let default_workarea =
            optional_reference("/default_workarea", req.default_workarea.as_deref())?;
        let crew = optional_reference("/crew", req.crew.as_deref())?;
        // A user's name, under the creation grammar (ADR-0044 d2).
        let user = req
            .user
            .as_deref()
            .map(|user| creation_name("/user", user))
            .transpose()?;
        let principal = req
            .principal
            .clone()
            .or_else(|| req.label.clone())
            .unwrap_or_else(|| "unnamed".to_owned());
        let issue = IssueKeyRequest {
            role,
            name,
            principal,
            held_by,
            default_workarea,
            crew,
            grants,
            connect_code: req.connect_code,
            user,
            guardrail_scope: parse_guardrail_scope(req.guardrail_scope.as_deref())?,
            profile: parse_profile(req.profile.as_deref())?,
        };
        // A resubmission under the same key and body replays the answer
        // without the value or the connect code, which are shown once, and
        // issues nothing again; the key is claimed before the issuance, so
        // a process stopping between the two never lets the resubmission
        // issue a second key (ADR-0031 d7; #127, #233).
        // The act runs as the caller this request just resolved, so the
        // request costs one of its key's rate, not two (#253).
        let caller = Arc::new(st.console_caller(headers).await?);
        let acting: Acting<'static> = Acting::Session(caller.clone());
        let digest = console::act_digest("", body);
        let claim = match st.claim(&caller, "keys.issue", &idem, &digest).await? {
            console::Claimed::Replayed(replayed) => return Ok(replayed),
            console::Claimed::Act(claim) => claim,
        };
        let label = req.label.clone();
        let st = st.clone();
        claim
            .run(async move {
                let issued = retry_chain(|| st.app.issue_key(acting.bearer(), &issue))
                    .await
                    .map_err(|e| {
                        console::ActFailure::first(map_app_error(e, ErrorContext::Write))
                    })?;
                let answer = key_issued_json(&issued, label.as_deref());
                let kept = console::without_secrets(&answer);
                Ok(console::Acted {
                    status: StatusCode::CREATED,
                    answer,
                    kept,
                })
            })
            .await
    }

    async fn deny_key(
        State(st): State<Self>,
        rid: Rid,
        Path(key_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::deny_key_inner(&st, &key_id, &headers, &body).await,
        )
    }

    /// `POST /v2/keys/{key}/denials`: one journalled `key.denied`, its
    /// epoch its own seq, the key and the node each by id or name
    /// (http-api.md keys, grants, denials; ADR-0025 d2; ADR-0026 d4).
    async fn deny_key_inner(
        st: &Self,
        key_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let idem = idempotency_key(headers)?;
        let key = reference("key", key_id)?;
        let req: DenialBody = parse_json_renamed(headers, body, |body| {
            refuse_renamed(body, "", &[("workarea_node_id", "workarea")])
        })?;
        let node = reference("/workarea", &req.workarea)?;
        // A session key cuts off one key on universe, as the pages do
        // (ADR-0070 d5; ADR-0045 d9, d13).
        if let Some(caller) = session::session_caller() {
            return Self::cut_off_as_session(st, caller, &key, &node, &idem, key_id, body).await;
        }
        let denial = retry_chain(|| st.app.deny_key(acting.bearer(), key.clone(), node.clone()))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::CREATED, &denial_json(&denial)))
    }

    async fn create_workarea(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::create_workarea_inner(&st, &headers, &body).await,
        )
    }

    async fn create_workarea_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        acting(headers, None)?;
        let idem = idempotency_key(headers)?;
        let req: WorkareaCreateBody = parse_json_renamed(headers, body, |body| {
            refuse_renamed(body, "", &[("parent_id", "parent")])
        })?;
        // A resubmission under the same key answers the recorded creation
        // and creates nothing again (ADR-0070 d3 layer 4).
        // The act runs as the caller this request just resolved, so the
        // request costs one of its key's rate, not two (#253).
        let caller = Arc::new(st.console_caller(headers).await?);
        let acting: Acting<'static> = Acting::Session(caller.clone());
        let digest = console::act_digest("", body);
        let claim = match st.claim(&caller, "workarea.create", &idem, &digest).await? {
            console::Claimed::Replayed(replayed) => return Ok(replayed),
            console::Claimed::Act(claim) => claim,
        };
        if req.prefix.is_some() {
            return Err(ApiError::validation(
                "prefix (vocabulary workareas, ADR-0021) is outside the PoC slice",
            ));
        }
        let parent = optional_reference("/parent", req.parent.as_deref())?;
        let name = creation_name("/name", &req.name)?;
        // A crew record's id or name (ADR-0026 d8).
        let crew = optional_reference("/crew", req.crew.as_deref())?;
        let st = st.clone();
        claim
            .run(async move {
                let created = retry_chain(|| {
                    st.app
                        .create_workarea(acting.bearer(), &name, parent.clone(), crew.clone())
                })
                .await
                .map_err(|e| console::ActFailure::first(map_app_error(e, ErrorContext::Write)))?;
                // The path derived from the parent names and the crew record
                // as stored, as the workarea read serves them (ADR-0025 d14;
                // ADR-0026 d8).
                let mut response =
                    workarea_json(&created.workarea, &created.path, created.crew.as_ref());
                if let Some(obj) = response.as_object_mut() {
                    obj.insert("created_at".to_owned(), json!(system_event_time().as_str()));
                }
                Ok(console::Acted::new(StatusCode::CREATED, response))
            })
            .await
    }

    async fn get_workarea(
        State(st): State<Self>,
        rid: Rid,
        Path(workarea_id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        finish(
            &rid,
            Self::get_workarea_inner(&st, Some(&workarea_id), &headers, None).await,
        )
    }

    /// `GET /v2/workareas/{workarea}`: the node and the calling key's
    /// view of it in one call, at reader effective access, or for an admin
    /// key its structure by its key role, and otherwise `access.not_found`
    /// (http-api.md workareas; ADR-0025 d14; ADR-0026 d15). `None` is
    /// the MCP facade's omitted workarea: the key's default, which the app
    /// resolves after step 1 of the refusal order (mcp-plugin.md
    /// `oml_workarea`).
    async fn get_workarea_inner(
        st: &Self,
        workarea_id: Option<&str>,
        headers: &HeaderMap,
        connection: Option<&Caller>,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, connection)?;
        let node = optional_reference("workarea", workarea_id)?;
        let info = st
            .app
            .workarea_info(acting.bearer(), node)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &workarea_view_json(&info)))
    }

    async fn propose_transition(
        State(st): State<Self>,
        rid: Rid,
        Path(workarea): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::propose_transition_inner(&st, &workarea, &headers, &body).await,
        )
    }

    /// `POST /v2/workareas/{workarea}/transitions`: a forward step proposed,
    /// `202` and pending until a second identity ratifies it; `to` is
    /// `committed` or `validated`, anything else `request.validation`
    /// naming `/to` (http-api.md workareas; ADR-0009 d3; ADR-0046 d3).
    async fn propose_transition_inner(
        st: &Self,
        workarea: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        acting(headers, None)?;
        let idem = idempotency_key(headers)?;
        let node = reference("workarea", workarea)?;
        let req: TransitionBody = parse_json(headers, body)?;
        // A resubmission under the same key answers the recorded proposal
        // and proposes nothing again (ADR-0070 d3 layer 4).
        // The act runs as the caller this request just resolved, so the
        // request costs one of its key's rate, not two (#253).
        let caller = Arc::new(st.console_caller(headers).await?);
        let acting: Acting<'static> = Acting::Session(caller.clone());
        let digest = console::act_digest(workarea, body);
        let claim = match st
            .claim(&caller, "workarea.transition", &idem, &digest)
            .await?
        {
            console::Claimed::Replayed(replayed) => return Ok(replayed),
            console::Claimed::Act(claim) => claim,
        };
        let to = match req.to.as_str() {
            "committed" => WorkareaStatus::Committed,
            "validated" => WorkareaStatus::Validated,
            other => {
                return Err(ApiError::validation(format!(
                    "to must be committed or validated, not {other:?}: the ladder moves forward only"
                ))
                .with_details(json!({ "field": "/to" })));
            }
        };
        let st = st.clone();
        claim
            .run(async move {
                let proposal =
                    retry_chain(|| st.app.propose_transition(acting.bearer(), node.clone(), to))
                        .await
                        .map_err(|e| {
                            console::ActFailure::first(map_app_error(e, ErrorContext::Write))
                        })?;
                Ok(console::Acted::new(
                    StatusCode::ACCEPTED,
                    json!({
                        "transition_id": proposal.transition_id,
                        "workarea_id": proposal.workarea_id,
                        "from": proposal.from.as_str(),
                        "to": proposal.to.as_str(),
                        "state": proposal.state.as_str(),
                        "proposed_by_key_id": proposal.proposer_key_id,
                        "ratified_by_key_id": Value::Null,
                    }),
                ))
            })
            .await
    }

    async fn ratify_transition(
        State(st): State<Self>,
        rid: Rid,
        Path((workarea, transition_id)): Path<(String, String)>,
        headers: HeaderMap,
    ) -> Response {
        finish(
            &rid,
            Self::ratify_transition_inner(&st, &workarea, &transition_id, &headers).await,
        )
    }

    /// `POST /v2/workareas/{workarea}/transitions/{transition_id}/ratify`: a
    /// second, non-proposing dreamer or admin ratifies, and the node takes
    /// the proposal's `to` (http-api.md workareas; ADR-0009 d3).
    async fn ratify_transition_inner(
        st: &Self,
        workarea: &str,
        transition_id: &str,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        acting(headers, None)?;
        let idem = idempotency_key(headers)?;
        let node = reference("workarea", workarea)?;
        let transition_id = Uuid::parse_str(transition_id).map_err(|_| {
            ApiError::validation(format!("transition_id {transition_id:?} is not an id"))
                .with_details(json!({ "field": "transition_id" }))
        })?;
        // The act runs as the caller this request just resolved, so the
        // request costs one of its key's rate, not two (#253).
        let caller = Arc::new(st.console_caller(headers).await?);
        let acting: Acting<'static> = Acting::Session(caller.clone());
        let digest = console::act_digest(workarea, &Bytes::from(transition_id.to_string()));
        let claim = match st.claim(&caller, "workarea.ratify", &idem, &digest).await? {
            console::Claimed::Replayed(replayed) => return Ok(replayed),
            console::Claimed::Act(claim) => claim,
        };
        let st = st.clone();
        claim
            .run(async move {
                let ratified = retry_chain(|| {
                    st.app
                        .ratify_transition(acting.bearer(), node.clone(), transition_id)
                })
                .await
                .map_err(|e| console::ActFailure::first(map_app_error(e, ErrorContext::Review)))?;
                let from = match ratified.status {
                    WorkareaStatus::Validated => WorkareaStatus::Committed,
                    _ => WorkareaStatus::Development,
                };
                Ok(console::Acted::new(
                    StatusCode::OK,
                    json!({
                        "transition_id": transition_id,
                        "workarea_id": ratified.workarea_id,
                        "from": from.as_str(),
                        "to": ratified.status.as_str(),
                        "state": "ratified",
                        "proposed_by_key_id": Value::Null,
                        "ratified_by_key_id": Value::Null,
                    }),
                ))
            })
            .await
    }

    async fn list_edges(
        State(st): State<Self>,
        rid: Rid,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => Self::list_edges_inner(&st, &params, &headers).await,
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    /// `GET /v2/edges?kind=…`: one page of one kind's edges, pending and
    /// ratified, newest first. `kind=claim` takes `workarea_id` as a subtree
    /// filter and `artifact_id`; `kind=brief` takes `workarea_id`, required,
    /// the workarea whose brief edges are read (http-api.md edges; ADR-0046
    /// d4, d7). `kind=workarea` is designed and refused.
    async fn list_edges_inner(
        st: &Self,
        params: &[(String, String)],
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let token = bearer(headers)?;
        let mut seen = HashSet::new();
        let (mut kind, mut workarea, mut artifact, mut cursor) = (None, None, None, None);
        let mut limit = PAGE_LIMIT_DEFAULT;
        for (name, value) in params {
            if !seen.insert(name.as_str()) {
                return Err(ApiError::validation(format!(
                    "the query parameter {name} is given more than once"
                )));
            }
            match name.as_str() {
                "kind" => kind = Some(value.as_str()),
                "workarea_id" => workarea = Some(reference("workarea_id", value)?),
                "artifact_id" => {
                    artifact = Some(Uuid::parse_str(value).map_err(|_| {
                        ApiError::validation(format!("artifact_id {value:?} is not an id"))
                            .with_details(json!({ "field": "artifact_id" }))
                    })?);
                }
                "limit" => limit = page_limit(value)?,
                "cursor" => cursor = Some(value.as_str()),
                other => {
                    return Err(ApiError::validation(format!(
                        "unknown query parameter {other:?}"
                    )));
                }
            }
        }
        let body = match kind {
            Some("claim") => {
                let page = st
                    .app
                    .claim_edges(
                        &token,
                        ClaimEdgeQuery {
                            workarea,
                            artifact_id: artifact,
                        },
                        cursor,
                        limit,
                    )
                    .await
                    .map_err(|e| map_app_error(e, ErrorContext::Write))?;
                json!({
                    "items": page.items.iter().map(claim_edge_json).collect::<Vec<_>>(),
                    "next_cursor": page.next_cursor,
                })
            }
            Some("brief") => {
                if artifact.is_some() {
                    return Err(ApiError::validation(
                        "artifact_id narrows claim edges only; kind=brief reads one workarea",
                    )
                    .with_details(json!({ "field": "artifact_id" })));
                }
                let workarea = workarea.ok_or_else(|| {
                    ApiError::validation("kind=brief reads one workarea: workarea_id is required")
                        .with_details(json!({ "field": "workarea_id" }))
                })?;
                let page = st
                    .app
                    .brief_edges(&token, workarea, cursor, limit)
                    .await
                    .map_err(|e| map_app_error(e, ErrorContext::Write))?;
                json!({
                    "items": page.items.iter().map(brief_edge_json).collect::<Vec<_>>(),
                    "next_cursor": page.next_cursor,
                })
            }
            other => return Err(edge_kind_refused(other)),
        };
        Ok(json_response(StatusCode::OK, &body))
    }

    async fn propose_edge(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(&rid, Self::propose_edge_inner(&st, &headers, &body).await)
    }

    /// `POST /v2/edges`: `kind: claim` `{from_artifact_id, to_artifact_id,
    /// label}` or `kind: brief` `{from_artifact_id, to_workarea_id}`, each
    /// `202` and pending (http-api.md edges; ADR-0046 d4, d7).
    async fn propose_edge_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let idem = idempotency_key(headers)?;
        // The kind first, so a designed kind's members are refused as that
        // kind rather than as unknown fields.
        let value: Value = parse_json(headers, body)?;
        match value.get("kind").and_then(Value::as_str) {
            Some("claim" | "brief") => {}
            other => return Err(edge_kind_refused(other)),
        }
        let req: EdgeBody = serde_json::from_value(value).map_err(not_the_shape)?;
        let artifact = |field: &str, value: Option<&str>| {
            value.and_then(|v| Uuid::parse_str(v).ok()).ok_or_else(|| {
                ApiError::validation(format!("{field} is required and must be an id"))
                    .with_details(json!({ "field": field }))
            })
        };
        let from = artifact("/from_artifact_id", req.from_artifact_id.as_deref())?;
        let (status, body) = match req.kind.as_str() {
            "claim" => {
                let to = artifact("/to_artifact_id", req.to_artifact_id.as_deref())?;
                let label = req.label.clone().ok_or_else(|| {
                    ApiError::validation("label is required on a claim edge")
                        .with_details(json!({ "field": "/label" }))
                })?;
                if req.to_workarea_id.is_some() {
                    return Err(ApiError::validation("to_workarea_id belongs to kind brief")
                        .with_details(json!({ "field": "/to_workarea_id" })));
                }
                let request = ProposeClaimEdge {
                    from_artifact_id: from,
                    to_artifact_id: to,
                    label,
                };
                let outcome =
                    retry_chain(|| st.app.propose_claim_edge(&token, &request, Some(&idem)))
                        .await
                        .map_err(|e| map_app_error(e, ErrorContext::Write))?;
                (StatusCode::ACCEPTED, claim_edge_json(&outcome.proposal))
            }
            "brief" => {
                if req.to_artifact_id.is_some() || req.label.is_some() {
                    return Err(ApiError::validation(
                        "a brief edge takes from_artifact_id and to_workarea_id alone; its label is brief",
                    )
                    .with_details(json!({
                        "field": if req.label.is_some() { "/label" } else { "/to_artifact_id" }
                    })));
                }
                let workarea = req.to_workarea_id.as_deref().ok_or_else(|| {
                    ApiError::validation("to_workarea_id is required on a brief edge")
                        .with_details(json!({ "field": "/to_workarea_id" }))
                })?;
                let request = ProposeBriefEdge {
                    artifact_id: from,
                    workarea: reference("/to_workarea_id", workarea)?,
                };
                let outcome =
                    retry_chain(|| st.app.propose_brief_edge(&token, &request, Some(&idem)))
                        .await
                        .map_err(|e| map_app_error(e, ErrorContext::Write))?;
                (StatusCode::ACCEPTED, brief_edge_json(&outcome.proposal))
            }
            other => return Err(edge_kind_refused(Some(other))),
        };
        Ok(json_response(status, &body))
    }

    async fn ratify_edge(
        State(st): State<Self>,
        rid: Rid,
        Path(edge_id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::ratify_edge_inner(&st, &edge_id, &headers).await)
    }

    /// `POST /v2/edges/{edge_id}/ratify`: either kind, the kind following
    /// from the id (http-api.md edges; ADR-0039 d9).
    async fn ratify_edge_inner(
        st: &Self,
        edge_id: &str,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let idem = idempotency_key(headers)?;
        let edge_id = Uuid::parse_str(edge_id).map_err(|_| {
            ApiError::validation(format!("edge_id {edge_id:?} is not an id"))
                .with_details(json!({ "field": "edge_id" }))
        })?;
        let ratified = retry_chain(|| st.app.ratify_edge(&token, edge_id, Some(&idem)))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Review))?;
        let body = match ratified {
            EdgeRatification::Claim(r) => claim_edge_json(&r.edge),
            EdgeRatification::Brief(r) => brief_edge_json(&r.edge),
        };
        Ok(json_response(StatusCode::OK, &body))
    }

    async fn workarea_tree(
        State(st): State<Self>,
        rid: Rid,
        Path(workarea_id): Path<String>,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => {
                Self::workarea_tree_inner(&st, &workarea_id, &params, &headers).await
            }
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    /// `GET /v2/workareas/{workarea}/tree`: one page of the subtree at a
    /// workarea, by id or name, in path order, each node with the `Workarea`
    /// fields the workarea read serves, under a cursor bound to the key: at
    /// reader effective access with every node outside it omitted, or for an
    /// admin key by its key role, structure only, and otherwise
    /// `access.not_found` (http-api.md workareas and pagination; ADR-0026
    /// d4, d15). Each query parameter at most once, and an unknown one
    /// refused rather than ignored.
    async fn workarea_tree_inner(
        st: &Self,
        workarea_id: &str,
        params: &[(String, String)],
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, None)?;
        let node = reference("workarea", workarea_id)?;
        match params.iter().find(|(n, _)| n == "counts") {
            Some((_, v)) if v == "true" => {
                if params.len() > 1 {
                    return Err(ApiError::validation(
                        "counts=true serves the whole subtree, so it takes no limit or cursor",
                    ));
                }
                return Self::tree_counts(st, node, &acting).await;
            }
            Some((_, v)) if v == "false" => {}
            Some(_) => return Err(ApiError::validation("counts must be true or false")),
            None => {}
        }
        let params: Vec<(String, String)> = params
            .iter()
            .filter(|(n, _)| n != "counts")
            .cloned()
            .collect();
        let (limit, cursor) = page_params(&params)?;
        let page = st
            .app
            .workarea_tree(acting.bearer(), node, cursor, limit)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let items: Vec<Value> = page
            .items
            .iter()
            .map(|n| workarea_json(&n.workarea, &n.path, n.crew.as_ref()))
            .collect();
        Ok(json_response(
            StatusCode::OK,
            &json!({ "items": items, "next_cursor": page.next_cursor }),
        ))
    }

    /// `GET /v2/workareas/{workarea}/tree?counts=true`: the subtree, up to
    /// `REGION_COUNT_MAX` nodes, each with its rows by state as the
    /// caller's merged listing would show them (`with_own`), counted up to
    /// `REGION_COUNT_MAX` rows, and `truncated` when either stopped there;
    /// for the admin key role every row is counted as structure, and no
    /// content is served (ADR-0045 d30; ADR-0070 d5).
    async fn tree_counts(
        st: &Self,
        node: Reference,
        acting: &Acting<'_>,
    ) -> Result<Response, ApiError> {
        let caller = st
            .app
            .caller(acting.bearer())
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let regions = if caller.key().role == Role::Admin {
            st.app.admin_regions(&caller, Some(node)).await
        } else {
            st.app.user_regions(&caller, Some(node)).await
        }
        .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let items: Vec<Value> = regions
            .regions
            .iter()
            .map(|r| {
                let w = &r.workarea;
                json!({
                    "id": w.workarea_id,
                    "name": w.name.as_str(),
                    "parent_id": w.parent_id,
                    "path": r.path,
                    "status": w.status.as_str(),
                    "user_root": w.user_root,
                    "mode": if w.in_progress { "in_progress" } else { "standard" },
                    "counts": {
                        "promoted": r.approved,
                        "held": r.held,
                        "in_progress": r.in_progress,
                        "other": r.other,
                    },
                })
            })
            .collect();
        Ok(json_response(
            StatusCode::OK,
            &json!({ "items": items, "next_cursor": null, "truncated": regions.partial }),
        ))
    }

    async fn events_tail(
        State(st): State<Self>,
        rid: Rid,
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::events_tail_inner(&st, &params, &headers).await)
    }

    async fn events_tail_inner(
        st: &Self,
        params: &HashMap<String, String>,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, None)?;
        refuse_workarea_header(headers, "workarea")?;
        if params.contains_key("workarea_id") {
            return Err(ApiError::renamed("workarea_id", "workarea"));
        }
        let after_seq: u64 = match params.get("after_seq") {
            None => 0,
            Some(s) => s
                .parse()
                .map_err(|_| ApiError::validation("after_seq must be a non-negative integer"))?,
        };
        let limit: usize = match params.get("limit") {
            None => PAGE_LIMIT_DEFAULT,
            Some(s) => s
                .parse()
                .ok()
                .filter(|l| (1..=PAGE_LIMIT_MAX).contains(l))
                .ok_or_else(|| {
                    ApiError::validation(format!("limit must be between 1 and {PAGE_LIMIT_MAX}"))
                })?,
        };
        let workarea = optional_reference("workarea", params.get("workarea").map(String::as_str))?;
        let page = st
            .app
            .tail_events(acting.bearer(), workarea, after_seq, limit)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        // head_seq for measuring lag, next_after_seq to resume from, both
        // read in the scan's snapshot (http-api.md events tail; ADR-0025 d15).
        let items: Vec<Value> = page.events.iter().map(event_json).collect();
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "items": items,
                "head_seq": page.head_seq,
                "next_after_seq": page.next_after_seq,
            }),
        ))
    }

    async fn bootstrap_claim(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::bootstrap_claim_inner(&st, &headers, &body).await,
        )
    }

    /// The one endpoint authenticated by something other than an API key:
    /// the single-use enrollment token the lead minted on first boot against
    /// an empty database (http-api.md bootstrap; ADR-0023 bootstrap ceremony).
    async fn bootstrap_claim_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let req: BootstrapClaimBody = parse_json(headers, body)?;
        // The token first (`auth.invalid`), the names after, so only the
        // holder of the valid token learns anything about a name (ADR-0026
        // d5; http-api.md bootstrap). The names are checked while the lock
        // still holds the token: a refused name consumes no token and
        // journals nothing, and no concurrent claim sees it consumed.
        let (taken, admin_key_name, tenant_name) = {
            let mut guard = st
                .enrollment
                .lock()
                .map_err(|_| ApiError::internal("enrollment token lock poisoned"))?;
            match guard.as_deref() {
                Some(t) if t == req.enrollment_token => {}
                Some(_) => return Err(ApiError::auth_invalid("unknown")),
                None => return Err(ApiError::auth_invalid("consumed")),
            }
            // The claim names the key it mints (ADR-0026 d9).
            let admin_key_name = required_creation_name("/name", req.name.as_deref())?;
            let tenant_name = creation_name(
                "/tenant_name",
                req.tenant_name.as_deref().unwrap_or("default"),
            )?;
            (guard.take(), admin_key_name, tenant_name)
        };
        let boot = BootstrapRequest {
            tenant_name,
            admin_key_name,
            admin_principal: req
                .admin_principal
                .unwrap_or_else(|| "bootstrap/admin".to_owned()),
        };
        match st.app.bootstrap_tenant(&boot).await {
            Err(e) => {
                // The ceremony did not journal; the token stays claimable.
                if let Ok(mut guard) = st.enrollment.lock() {
                    *guard = taken;
                }
                Err(map_app_error(e, ErrorContext::Write))
            }
            Ok(result) => {
                // The PoC ceremony mints the tenant's first ADMIN key
                // directly (the spec's operator tier is out of slice).
                let mut body = key_issued_json(&result.admin_key, Some("root admin key"));
                if let Some(obj) = body.as_object_mut() {
                    obj.insert("tenant_id".to_owned(), json!(result.tenant_id));
                    obj.insert(
                        "root_workarea_id".to_owned(),
                        json!(result.root_workarea_id),
                    );
                }
                Ok(json_response(StatusCode::CREATED, &body))
            }
        }
    }

    async fn get_key(
        State(st): State<Self>,
        rid: Rid,
        Path(key_id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::get_key_inner(&st, &key_id, &headers).await)
    }

    /// `GET /v2/keys/{key}`: one key, by id or name, admin by key role,
    /// a revoked key's included; never its value (http-api.md keys, grants,
    /// denials; ADR-0026 d4, d9, d10).
    async fn get_key_inner(
        st: &Self,
        key_id: &str,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let token = bearer(headers)?;
        let key = reference("key", key_id)?;
        let detail = st
            .app
            .key(&token, key)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        // A connector key's open and revoked connections; a bearer key has
        // none (ADR-0031 d11).
        let connections = st
            .app
            .list_connections(&token, Reference::Id(detail.key.key_id))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        // The key's live connect code, named but never shown (ADR-0067 d3).
        let live_code = st
            .app
            .live_connect_code(&token, Reference::Id(detail.key.key_id))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let mut view = key_json(&detail, &connections);
        view["live_connect_code"] = live_code.map_or(Value::Null, |c| {
            json!({
                "code_id": c.code_id.to_string(),
                "expires_at": c.expires_at.as_str(),
                "extended": c.extended,
            })
        });
        Ok(json_response(StatusCode::OK, &view))
    }

    async fn mint_connect_code(
        State(st): State<Self>,
        rid: Rid,
        Path(key_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::mint_connect_code_inner(&st, &key_id, &headers, &body).await,
        )
    }

    /// `POST /v2/keys/{key}/connect-codes`: a fresh connect code for a
    /// connector key, by id or name, shown once; its commit voids the key's
    /// earlier codes (http-api.md keys; ADR-0031 d7, d8). An optional body
    /// `{"ttl_seconds": n}` asks for a lifetime past the ten-minute default,
    /// for a key confined to one workarea (ADR-0067 d3).
    async fn mint_connect_code_inner(
        st: &Self,
        key_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let idem = idempotency_key(headers)?;
        let key = reference("key", key_id)?;
        let req: ConnectCodeBody = if body.is_empty() {
            ConnectCodeBody::default()
        } else {
            parse_json(headers, body)?
        };
        // A person's web session key mints a code for one of their own
        // connector keys (ADR-0070 d5; ADR-0045 d9).
        if let Some(caller) = session::session_caller() {
            return Self::connect_code_as_session(
                st,
                caller,
                &key,
                req.ttl_seconds,
                &idem,
                key_id,
                body,
            )
            .await;
        }
        // A resubmission replays the answer without the code, which is
        // shown once, and mints none: a second code would void the first
        // (ADR-0031 d7; #233).
        let caller = st.console_caller(headers).await?;
        let digest = console::act_digest(key_id, body);
        let claim = match st
            .claim(&caller, "keys.connect_code", &idem, &digest)
            .await?
        {
            console::Claimed::Replayed(replayed) => return Ok(replayed),
            console::Claimed::Act(claim) => claim,
        };
        let ttl_seconds = req.ttl_seconds;
        let st = st.clone();
        claim
            .run(async move {
                let code = retry_chain(|| {
                    st.app
                        .mint_connect_code_with_ttl(&token, key.clone(), ttl_seconds)
                })
                .await
                .map_err(|e| console::ActFailure::first(map_app_error(e, ErrorContext::Write)))?;
                let answer = connect_code_json(&code);
                let kept = console::without_secrets(&answer);
                Ok(console::Acted {
                    status: StatusCode::CREATED,
                    answer,
                    kept,
                })
            })
            .await
    }

    async fn revoke_connection(
        State(st): State<Self>,
        rid: Rid,
        Path((key_id, connection_id)): Path<(String, String)>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::revoke_connection_inner(&st, &key_id, &connection_id, &headers, &body).await,
        )
    }

    /// `POST /v2/keys/{key}/connections/{connection_id}/revoke`: one
    /// journalled `connection.revoked`, the key by id or name and the
    /// connection by its id; served whether or not `oauth.issuer` is set,
    /// and an `authorized` connection, like an unknown id, is
    /// `access.not_found` (http-api.md keys; ADR-0031 d1, d11, d12).
    async fn revoke_connection_inner(
        st: &Self,
        key_id: &str,
        connection_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        let key = reference("key", key_id)?;
        // A connection has no name; anything but its id names none.
        let connection_id = Uuid::parse_str(connection_id).map_err(|_| ApiError::not_found())?;
        let req: ConnectionRevokeBody = if body.is_empty() {
            ConnectionRevokeBody::default()
        } else {
            parse_json(headers, body)?
        };
        let revoked = retry_chain(|| {
            st.app
                .revoke_connection(&token, key.clone(), connection_id, req.reason.as_deref())
        })
        .await
        .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &connection_json(&revoked)))
    }

    async fn whoami(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        uri: axum::http::Uri,
    ) -> Response {
        let start = uri
            .query()
            .and_then(|q| crate::oauth::form_pairs(q.as_bytes()).ok())
            .is_some_and(|pairs| pairs.iter().any(|(n, v)| n == "start" && v == "true"));
        finish(&rid, Self::whoami_inner(&st, &headers, start).await)
    }

    /// `GET /v2/whoami`: the calling key's own record, for any live key;
    /// never its denials, and no read is journalled (http-api.md whoami;
    /// ADR-0039 d15).
    async fn whoami_inner(
        st: &Self,
        headers: &HeaderMap,
        start: bool,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, None)?;
        let whoami = st
            .app
            .whoami_of(acting.bearer())
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let person = st
            .app
            .whoami_person(acting.bearer(), start)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let person = person.as_ref().map_or(Value::Null, person_json);
        let mut body = whoami_json(&whoami);
        if let Some(obj) = body.as_object_mut() {
            obj.insert("person".to_owned(), person);
        }
        Ok(json_response(StatusCode::OK, &body))
    }

    async fn create_crew(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(&rid, Self::create_crew_inner(&st, &headers, &body).await)
    }

    /// `POST /v2/crews`: a named crew record, admin by key role, journalled
    /// `crew.created`; a name the tenant's crews already hold is
    /// `name.taken`, checked last, and journals nothing (http-api.md crews;
    /// ADR-0026 d5, d8).
    async fn create_crew_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        let req: CrewCreateBody = parse_json(headers, body)?;
        let create = CrewCreateRequest {
            name: required_creation_name("/name", req.name.as_deref())?,
            charter_uri: req.charter_uri,
            external_manager: req.external_manager,
        };
        let crew = retry_chain(|| st.app.create_crew(&token, &create))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::CREATED, &crew_json(&crew)))
    }

    async fn list_crews(
        State(st): State<Self>,
        rid: Rid,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => Self::list_crews_inner(&st, &params, &headers).await,
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    /// `GET /v2/crews`: one page of the tenant's crew records in name
    /// order, admin by key role, under a cursor bound to the key
    /// (http-api.md crews and pagination; ADR-0026 d8). Each query parameter
    /// at most once, and an unknown one refused rather than ignored.
    async fn list_crews_inner(
        st: &Self,
        params: &[(String, String)],
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let token = bearer(headers)?;
        let (limit, cursor) = page_params(params)?;
        let page = st
            .app
            .crews(&token, cursor, limit)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "items": page.items.iter().map(crew_json).collect::<Vec<_>>(),
                "next_cursor": page.next_cursor,
            }),
        ))
    }

    async fn get_crew(
        State(st): State<Self>,
        rid: Rid,
        Path(crew_id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::get_crew_inner(&st, &crew_id, &headers).await)
    }

    /// `GET /v2/crews/{crew}`: one crew record, by id or name, admin by
    /// key role; one the tenant does not hold is absent (http-api.md crews;
    /// ADR-0026 d4, d8).
    async fn get_crew_inner(
        st: &Self,
        crew_id: &str,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let token = bearer(headers)?;
        let crew = reference("crew", crew_id)?;
        let record = st
            .app
            .crew(&token, crew)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &crew_json(&record)))
    }

    async fn update_crew(
        State(st): State<Self>,
        rid: Rid,
        Path(crew_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::update_crew_inner(&st, &crew_id, &headers, &body).await,
        )
    }

    /// `POST /v2/crews/{crew}/update`: the record's charter URI or
    /// external manager, at least one, journalled `crew.updated`. The body
    /// is closed, so a `name` is refused, never dropped: a crew's name
    /// never changes (http-api.md crews; ADR-0026 d1, d8).
    async fn update_crew_inner(
        st: &Self,
        crew_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        let crew = reference("crew", crew_id)?;
        let req: CrewUpdateBody = parse_json(headers, body)?;
        let update = CrewUpdate {
            charter_uri: req.charter_uri,
            external_manager: req.external_manager,
        };
        let updated = retry_chain(|| st.app.update_crew(&token, crew.clone(), &update))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &crew_json(&updated)))
    }

    async fn set_workarea_mode(
        State(st): State<Self>,
        rid: Rid,
        Path(workarea_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::set_workarea_mode_inner(&st, &workarea_id, &headers, &body).await,
        )
    }

    /// `POST /v2/workareas/{workarea}/mode`: a person's dreamer key sets the
    /// workarea's mode, `in_progress` or `standard`; `standard` on an
    /// in-progress root is its commit, refused while anything in its subtree
    /// is held; journalled `workarea.mode_set` (ADR-0030 d1, d6, d13). The
    /// answer is the node as the key's read serves it.
    async fn set_workarea_mode_inner(
        st: &Self,
        workarea_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        acting(headers, None)?;
        let idem = idempotency_key(headers)?;
        let workarea = reference("workarea", workarea_id)?;
        let req: WorkareaModeBody = parse_json(headers, body)?;
        let mode = WorkareaMode::parse(&req.mode).ok_or_else(|| {
            ApiError::validation(format!(
                "mode must be in_progress or standard, not {:?}",
                req.mode
            ))
            .with_details(json!({ "field": "/mode" }))
        })?;
        // The act runs as the caller this request just resolved, so the
        // request costs one of its key's rate, not two (#253).
        let caller = Arc::new(st.console_caller(headers).await?);
        let acting: Acting<'static> = Acting::Session(caller.clone());
        let digest = console::act_digest(workarea_id, body);
        let claim = match st.claim(&caller, "workarea.mode", &idem, &digest).await? {
            console::Claimed::Replayed(replayed) => return Ok(replayed),
            console::Claimed::Act(claim) => claim,
        };
        let st = st.clone();
        claim
            .run(async move {
                let updated = retry_chain(|| {
                    st.app
                        .set_workarea_mode(acting.bearer(), workarea.clone(), mode)
                })
                .await
                .map_err(|e| console::ActFailure::first(map_app_error(e, ErrorContext::Write)))?;
                let info = st
                    .app
                    .workarea_info(acting.bearer(), Some(Reference::Id(updated.workarea_id)))
                    .await
                    .map_err(|e| {
                        console::ActFailure::after(map_app_error(e, ErrorContext::Write))
                    })?;
                Ok(console::Acted::new(
                    StatusCode::OK,
                    workarea_view_json(&info),
                ))
            })
            .await
    }

    async fn set_workarea_crew(
        State(st): State<Self>,
        rid: Rid,
        Path(workarea_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::set_workarea_crew_inner(&st, &workarea_id, &headers, &body).await,
        )
    }

    /// `POST /v2/workareas/{workarea}/crew`: points the workarea at a
    /// crew record, each by id or name, admin by key role, journalled
    /// `workarea.crew_updated`; a workarea the tenant does not hold is
    /// absent, and so, checked after it, is a crew (http-api.md workareas;
    /// ADR-0026 d8). The answer is the node as the admin key's structure read
    /// serves it once the update has committed (ADR-0026 d15).
    async fn set_workarea_crew_inner(
        st: &Self,
        workarea_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        let workarea = reference("workarea", workarea_id)?;
        let req: WorkareaCrewBody = parse_json(headers, body)?;
        let crew = reference("/crew", &req.crew)?;
        let updated = retry_chain(|| {
            st.app
                .set_workarea_crew(&token, workarea.clone(), crew.clone())
        })
        .await
        .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let info = st
            .app
            .workarea_info(&token, Some(Reference::Id(updated.workarea_id)))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::OK,
            &workarea_json(&info.workarea, &info.path, info.crew.as_ref()),
        ))
    }
}

/// openapi.yaml `RecallTimings`: each stage's milliseconds, in the order
/// the stages ran, and their sum (ADR-0049 d14).
fn timings_json(timings: &oml_app::search::Timings) -> Value {
    let ms = |seconds: f64| (seconds * 1_000_000.0).round() / 1000.0;
    let stages: serde_json::Map<String, Value> = timings
        .stages
        .iter()
        .map(|(stage, seconds)| (stage.as_str().to_owned(), json!(ms(*seconds))))
        .collect();
    let total: f64 = timings.stages.iter().map(|(_, s)| s).sum();
    json!({ "stages_ms": stages, "total_ms": ms(total) })
}

/// `on_similar` as sent: `hold`, the default, or `refuse` (ADR-0050 d1).
fn parse_on_similar(sent: Option<&str>) -> Result<OnSimilar, ApiError> {
    match sent {
        None | Some("hold") => Ok(OnSimilar::Hold),
        Some("refuse") => Ok(OnSimilar::Refuse),
        Some(other) => Err(ApiError::validation(format!(
            "on_similar must be hold or refuse, not {other:?}"
        ))
        .with_details(json!({ "field": "/on_similar" }))),
    }
}

/// openapi.yaml `SimilarItem`: a row the similar check named, as a write's
/// answer and `submission.similar_exists` serve it (ADR-0050 d1).
pub(crate) fn similar_json(similar: &[SimilarItem]) -> Value {
    Value::Array(
        similar
            .iter()
            .map(|s| {
                json!({
                    "artifact_id": s.artifact_id,
                    "claim_key": s.claim_key,
                    "version": s.version,
                    "kind": s.kind,
                    "workarea_id": s.workarea_id,
                    "state": s.state,
                    "relation": s.relation,
                    "jaccard": f64::from(s.jaccard_percent) / 100.0,
                    "own": s.own,
                })
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Request bodies (field spellings per openapi.yaml)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SubmitBody {
    #[serde(default)]
    verb: Option<String>,
    #[serde(default)]
    claim_key: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    body: String,
    /// The workarea written in, by id or name; omitted, the key's default
    /// (ADR-0039 d4, d7).
    #[serde(default)]
    workarea: Option<String>,
    #[serde(default)]
    attachments: Vec<String>,
    #[serde(default)]
    derives_from: Vec<Uuid>,
    #[serde(default)]
    simulation_id: Option<String>,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "declared data, accepted; the PoC journals real time only"
    )]
    simulated_time: Option<String>,
    /// A guardrail's severity (proposed ADR-0037 d2, d10).
    #[serde(default)]
    severity: Option<String>,
    /// A guardrail's optional `oml-gr/1` rule (proposed ADR-0037 d8, d10).
    #[serde(default)]
    rule: Option<Value>,
    /// `hold` (the default) or `refuse` (ADR-0050 d1).
    #[serde(default)]
    on_similar: Option<String>,
    /// When the memory holds, each end a period or an instant (proposed
    /// ADR-0059 d2).
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
    /// The origin the author declares (ADR-0057 d1-d3).
    #[serde(default)]
    anchor: Option<Value>,
    /// The media type of `body`, `text/plain` when omitted (ADR-0056 d1).
    #[serde(default)]
    format: Option<String>,
    /// One line, at most 100 characters; required of a non-prose claim
    /// (ADR-0056 d10).
    #[serde(default)]
    title: Option<String>,
    /// `origin: {kind}` (ADR-0063 d1, d2).
    #[serde(default)]
    origin: Option<Value>,
}

/// openapi.yaml `RecallRequest`, closed (ADR-0025 d16).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecallBody {
    query: String,
    /// The subtree recalled, by id or name; omitted, the key's default
    /// workarea (ADR-0039 d4).
    workarea: Option<String>,
    k: Option<usize>,
    kinds: Option<Vec<String>>,
    include_text: Option<bool>,
    include_unreviewed: Option<bool>,
    max_tokens: Option<u64>,
    /// Parsed only to be refused while the vocabulary filter is unbuilt
    /// (ADR-0025 d16).
    terms: Option<Vec<String>>,
    /// Parsed only to be refused while the vocabulary filter is unbuilt.
    expand_terms: Option<String>,
    /// The search pipeline by name; omitted, the tenant's default or the
    /// deployment's (ADR-0049 d4).
    search: Option<String>,
    /// `any` (the default) or `all` (ADR-0049 d5).
    #[serde(rename = "match")]
    match_mode: Option<String>,
    /// What the key's last recall served that helped, and what did not;
    /// checked before the recall runs (ADR-0054 d6).
    feedback: Option<Value>,
    /// The answer's per-stage `timings`, where the deployment serves them
    /// (`search.timings`; ADR-0049 d14).
    timings: Option<bool>,
    /// Serve versions whose valid time has ended (proposed ADR-0059 d3).
    include_expired: Option<bool>,
    /// The recall's now: a period or an instant (proposed ADR-0059 d5).
    now: Option<String>,
    /// The origin filters (ADR-0057 d4; ADR-0063 d3).
    anchor_repo: Option<String>,
    anchor_path_prefix: Option<String>,
    origin_kinds: Option<Vec<String>>,
    /// Where to search: `scope`, `wide` or a list of workareas (ADR-0072
    /// d5); parsed by [`parse_sources`].
    sources: Option<Value>,
    /// The transaction-time instant to read at (ADR-0059 d14).
    as_of: Option<String>,
}

/// `sources` on recall (ADR-0072 d5): a string is a mode, `scope` or
/// `wide`, and an array a fixed set of workarea names or ids; any other
/// shape, and a non-string element, is `request.validation` at `/sources`.
fn parse_sources(value: Option<&Value>) -> Result<Option<oml_app::federation::Sources>, ApiError> {
    use oml_app::federation::Sources;
    let refused = |message: &str| {
        ApiError::validation(message.to_owned()).with_details(json!({ "field": "/sources" }))
    };
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(mode)) => match mode.as_str() {
            "scope" => Ok(Some(Sources::Scope)),
            "wide" => Ok(Some(Sources::Wide)),
            other => Err(refused(&format!(
                "sources is scope, wide or a list of workareas, not {other:?}"
            ))),
        },
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_owned).ok_or_else(|| {
                    refused("every element of a sources list is a workarea name or id")
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|names| Some(Sources::List(names))),
        Some(_) => Err(refused("sources is scope, wide or a list of workareas")),
    }
}

/// `search.sources` (ADR-0072 d8): the mode, the candidates considered, the
/// sources searched with why, and the tail cut.
fn sources_json(report: &oml_app::federation::SourcesReport) -> Value {
    json!({
        "mode": report.mode,
        "considered": report.considered,
        "searched": report.searched.iter().map(|s| json!({
            "workarea_id": s.workarea_id,
            "workarea_path": s.workarea_path,
            "via": s.via.as_str(),
            "estimate": s.estimate,
            "matched": { "words": s.matched_words },
        })).collect::<Vec<_>>(),
        "cut": {
            "count": report.cut.count,
            "matches_at_least": report.cut.matches_at_least,
        },
    })
}

/// openapi.yaml `SourceCheckRequest`, closed (ADR-0057 d5).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceCheckBody {
    rev: String,
    result: String,
    hash: Option<String>,
    path: Option<String>,
    fragment: Option<String>,
}

/// openapi.yaml `SupersedeRequest`, closed: it declares every submission
/// field a successor can need beside its own (ADR-0025 d16).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupersedeBody {
    body: String,
    /// The claim's latest live version as last read; required (ADR-0057
    /// d19), parsed as optional so its absence names the field.
    #[serde(default)]
    expected_version: Option<Value>,
    /// The successor's own anchor (ADR-0057 d2).
    #[serde(default)]
    anchor: Option<Value>,
    /// The successor's own `origin` (ADR-0063 d2).
    #[serde(default)]
    origin: Option<Value>,
    /// When sent, the target's workarea, by id or name (ADR-0039 d7).
    workarea: Option<String>,
    #[serde(default)]
    retirement: bool,
    kind: Option<String>,
    #[serde(default)]
    attachments: Vec<String>,
    #[serde(default)]
    derives_from: Vec<Uuid>,
    reason: Option<String>,
    #[serde(default)]
    simulation_id: Option<String>,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "declared data, accepted; the PoC journals real time only"
    )]
    simulated_time: Option<String>,
    #[serde(default)]
    mapping: Option<Value>,
    #[serde(default)]
    terms: Option<Value>,
    #[serde(default)]
    term_status: Option<Value>,
    #[serde(default)]
    successors: Option<Value>,
    /// A guardrail successor's severity (proposed ADR-0037 d10).
    #[serde(default)]
    severity: Option<String>,
    /// A guardrail successor's optional rule (proposed ADR-0037 d10).
    #[serde(default)]
    rule: Option<Value>,
    /// `hold` (the default) or `refuse` (ADR-0050 d1).
    #[serde(default)]
    on_similar: Option<String>,
    /// The successor's own valid time; it inherits none (proposed
    /// ADR-0059 d2).
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
    /// The successor's own format and title; nothing is inherited, so a
    /// format changes by supersession alone (ADR-0056 d2).
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Deserialize)]
struct ReviewNextBody {
    /// The subtree filter, by id or name; parsed only to be refused while
    /// the filter is unbuilt. Any JSON value is the field's presence.
    #[serde(default)]
    workarea: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransitionBody {
    to: String,
}

/// `POST /v2/edges`'s body; which members a kind takes is checked after
/// the parse, so each refusal names its JSON Pointer.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeBody {
    kind: String,
    from_artifact_id: Option<String>,
    to_artifact_id: Option<String>,
    to_workarea_id: Option<String>,
    label: Option<String>,
}

/// `POST /v2/review/verdicts/bulk`'s body (ADR-0055 d3): the ids as
/// strings, each parsed after the body so a refusal names the field.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BulkBody {
    verdict: String,
    artifact_ids: Vec<String>,
    reason: Option<String>,
}

/// One bulk item's outcome label: `approved`, `rejected` or `skipped`
/// (ADR-0055 d4, d13).
#[must_use]
pub fn bulk_outcome(item: &BulkItem) -> &'static str {
    match &item.result {
        Ok(outcome) if outcome.state == oml_app::domain::MemoryStatus::Promoted => "approved",
        Ok(_) => "rejected",
        Err(_) => "skipped",
    }
}

/// Counts a bulk verdict's items in `oml_review_bulk_items_total`, once
/// each: an item that replays its idempotency record was counted when it
/// was decided (ADR-0055 d13). The route and the admin page both call it.
pub(crate) fn count_bulk(metrics: &traffic::metrics::Metrics, items: &[BulkItem]) {
    for item in items {
        if !item.result.as_ref().is_ok_and(|o| o.replayed) {
            metrics.bulk_item(bulk_outcome(item));
        }
    }
}

/// A bulk verdict's answer: one outcome per id in the order named, a
/// skipped one with the refusal a single verdict naming it would have
/// answered, and the counts (ADR-0055 d4).
#[must_use]
pub fn bulk_json(items: Vec<BulkItem>) -> Value {
    let mut counts = [0_u64; 3];
    let rows: Vec<Value> = items
        .into_iter()
        .map(|item| {
            let outcome = bulk_outcome(&item);
            match outcome {
                "approved" => counts[0] += 1,
                "rejected" => counts[1] += 1,
                _ => counts[2] += 1,
            }
            match item.result {
                Ok(o) => json!({
                    "artifact_id": item.memory_id,
                    "outcome": outcome,
                    "state": o.state.as_str(),
                    "version": o.version,
                    "journal_seq": o.journal_seq,
                    "same_user": o.same_user,
                    "replayed": o.replayed,
                }),
                Err(e) => {
                    let refusal = map_app_error(e, ErrorContext::Review);
                    json!({
                        "artifact_id": item.memory_id,
                        "outcome": outcome,
                        "code": refusal.code,
                        "message": refusal.message,
                        "details": refusal.details,
                    })
                }
            }
        })
        .collect();
    json!({
        "items": rows,
        "summary": {
            "approved": counts[0],
            "rejected": counts[1],
            "skipped": counts[2],
        },
    })
}

/// The held listing's page as `GET /v2/review/held` serves it.
#[must_use]
pub fn held_page_json(page: &HeldPage) -> Value {
    json!({
        "items": page.items.iter().map(|item| {
            let mut value = memory_json(&item.served, true);
            if let Some(obj) = value.as_object_mut() {
                obj.insert("held_since".to_owned(), json!(item.held_since.as_str()));
                obj.insert("same_user".to_owned(), json!(item.same_user));
            }
            value
        }).collect::<Vec<_>>(),
        "next_cursor": page.next_cursor,
    })
}

#[derive(Deserialize)]
struct VerdictBody {
    /// Omitted, the reviewer key's one claimed item (ADR-0039 d10).
    serve_id: Option<String>,
    verdict: String,
    reason: Option<String>,
}

/// A workarea or crew reference is a string, classified after the body
/// parses so that a refusal names its JSON Pointer (errors.md; ADR-0026 d3).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DenyBody {
    workarea: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantBody {
    workarea: String,
    role: String,
    #[serde(default)]
    deny: Vec<DenyBody>,
    /// `subtree`, the default, or `node` (ADR-0044 d4).
    #[serde(default)]
    covers: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyIssueBody {
    role: String,
    /// Required, and optional here only so that an absent name is refused
    /// with `details.field: name`, which serde's missing-field refusal does
    /// not carry (ADR-0026 d9).
    name: Option<String>,
    #[serde(default)]
    grants: Vec<GrantBody>,
    label: Option<String>,
    principal: Option<String>,
    /// Parsed only to be refused while key expiry is unbuilt (ADR-0025 d16).
    #[serde(default)]
    expiry: Option<String>,
    held_by: Option<String>,
    default_workarea: Option<String>,
    /// The key's crew record, by id or name (ADR-0026 d8).
    crew: Option<String>,
    /// The person the key acts for, by the user's name; the first key
    /// naming a user creates its root (ADR-0044 d2).
    #[serde(default)]
    user: Option<String>,
    /// Parsed only to be refused while the first-admin-key path is unbuilt
    /// (ADR-0039 d9).
    tenant: Option<String>,
    /// A connector key: no value, and a connect code in its place
    /// (ADR-0031 d7).
    #[serde(default)]
    connect_code: bool,
    /// The key's guardrail scope; omitted, the issuance default (proposed
    /// ADR-0037 d4).
    #[serde(default)]
    guardrail_scope: Option<String>,
    /// `curator`, or omitted for a key with no profile (ADR-0030 d8).
    #[serde(default)]
    profile: Option<String>,
}

/// The body of `POST /v2/keys/{key}/connect-codes`, closed; absent, the code
/// lives the ten-minute default (ADR-0067 d3).
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ConnectCodeBody {
    #[serde(default)]
    ttl_seconds: Option<i64>,
}

/// The revoke body of `POST /v2/keys/{key}/connections/{connection_id}/revoke`,
/// closed; absent, no reason is given.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ConnectionRevokeBody {
    reason: Option<String>,
}

/// openapi.yaml `DenialRequest`, closed, so a v0.3.0 spelling is refused
/// naming its successor rather than dropped (ADR-0039 d7).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DenialBody {
    workarea: String,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "key.denied journals no reason (event-catalog.md); the text is accepted, not stored"
    )]
    reason: Option<String>,
}

/// openapi.yaml `WorkareaCreate`, closed, so a v0.3.0 spelling is refused
/// naming its successor rather than dropped (ADR-0039 d7).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkareaCreateBody {
    /// Omitted, a user key's own root (ADR-0044 d6).
    #[serde(default)]
    parent: Option<String>,
    name: String,
    /// A crew record's id or name (ADR-0026 d8).
    #[serde(default)]
    crew: Option<String>,
    #[serde(default)]
    prefix: Option<String>,
}

/// openapi.yaml `WorkareaCrewUpdate`, closed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkareaCrewBody {
    crew: String,
}

/// openapi.yaml `WorkareaModeSet`, closed (ADR-0030 d1).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkareaModeBody {
    mode: String,
}

/// openapi.yaml `CrewCreate`, closed (ADR-0026 d8).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CrewCreateBody {
    /// Required, and optional here only so that an absent name is refused
    /// with `details.field: /name`, as on key issuance.
    name: Option<String>,
    charter_uri: Option<String>,
    external_manager: Option<String>,
}

/// openapi.yaml `CrewUpdate`, closed: the name is not updatable.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CrewUpdateBody {
    charter_uri: Option<String>,
    external_manager: Option<String>,
}

#[derive(Deserialize)]
struct BootstrapClaimBody {
    enrollment_token: String,
    /// The key's name: required, and checked after the token, so optional
    /// here for the same reason as `KeyIssueBody`'s (ADR-0026 d5, d9).
    #[serde(default)]
    name: Option<String>,
    // PoC extensions: the standalone ceremony names its tenant here.
    #[serde(default)]
    tenant_name: Option<String>,
    #[serde(default)]
    admin_principal: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A value that names a workarea, crew, key or tenant, classified at parse
/// as an id or a name, before any check that consults state. One that is
/// neither — a derived path, an uppercase spelling, hex-and-dash text of
/// version 8 — is `request.validation` with `details.field` naming its place
/// as errors.md spells it: the parameter for a path or query parameter, the
/// header's name, an RFC 6901 JSON Pointer for a body member (http-api.md
/// names and ids; ADR-0026 d3). The app resolves a name after the verb
/// ceiling (ADR-0026 d4).
fn reference(field: &str, value: &str) -> Result<Reference, ApiError> {
    Reference::parse(value).map_err(|refusal| {
        ApiError::validation(format!(
            "{field} {value:?} is neither an id nor a name: {refusal}"
        ))
        .with_details(json!({ "field": field }))
    })
}

/// The `limit` and `cursor` of a listing that takes no other query
/// parameter (http-api.md pagination): each at most once, and an unknown
/// one refused rather than ignored, so a mistyped filter never widens a
/// listing.
fn page_params(params: &[(String, String)]) -> Result<(usize, Option<&str>), ApiError> {
    let (mut limit, mut cursor) = (PAGE_LIMIT_DEFAULT, None);
    let mut seen = HashSet::new();
    for (name, value) in params {
        if !seen.insert(name.as_str()) {
            return Err(ApiError::validation(format!(
                "the query parameter {name} is given more than once"
            )));
        }
        match name.as_str() {
            "limit" => limit = page_limit(value)?,
            "cursor" => cursor = Some(value.as_str()),
            other => {
                return Err(ApiError::validation(format!(
                    "unknown query parameter {other:?}"
                )));
            }
        }
    }
    Ok((limit, cursor))
}

/// [`reference`] over an optional value.
fn optional_reference(field: &str, value: Option<&str>) -> Result<Option<Reference>, ApiError> {
    value.map(|value| reference(field, value)).transpose()
}

/// A name a creation sends, under the creation grammar: one it refuses is
/// `request.validation` naming the field by its JSON Pointer, at parse,
/// never coerced into a name it resembles (http-api.md names and ids;
/// errors.md; ADR-0026 d2, d3).
fn creation_name(field: &'static str, value: &str) -> Result<Name, ApiError> {
    Name::parse(value).map_err(|refusal| {
        ApiError::validation(format!("{field} {value:?}: {refusal}"))
            .with_details(json!({ "field": field }))
    })
}

/// A name a creation must send: absent, `request.validation` naming the
/// field; present, [`creation_name`]'s (ADR-0026 d9).
fn required_creation_name(field: &'static str, value: Option<&str>) -> Result<Name, ApiError> {
    let value = value.ok_or_else(|| {
        ApiError::validation(format!(
            "{field} is required: every creation names its row (ADR-0026 d1)"
        ))
        .with_details(json!({ "field": field }))
    })?;
    creation_name(field, value)
}

async fn healthz(rid: Rid) -> Response {
    finish(
        &rid,
        Ok(json_response(StatusCode::OK, &json!({"status": "ok"}))),
    )
}

/// Every route but the probes while the store holds rows that predate
/// names: the replica cannot serve them safely, so it serves nothing
/// (errors.md `system.unavailable`; ADR-0026 d11).
async fn names_unassigned(rid: Rid) -> Response {
    finish(&rid, Err(ApiError::unavailable("names_assigned")))
}

fn new_uuid() -> Uuid {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source is available");
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

pub(crate) fn new_request_id() -> String {
    format!("req_{}", new_uuid().simple())
}

/// Every response carries `X-Request-Id`; error bodies already repeat it as
/// `request_id` (http-api.md conventions).
/// The response carries what the handler resolved, its key, target
/// workarea and refusal code, for the request line (ADR-0042 d2).
fn finish(request_id: &str, result: Result<Response, ApiError>) -> Response {
    let (response, code) = match result {
        Ok(response) => (response, None),
        Err(e) => {
            let code = e.code;
            (e.into_response(request_id), Some(code))
        }
    };
    finish_coded(request_id, response, code)
}

/// [`finish`] for a response the handler already built, refused under
/// `error_code` or not: the OAuth endpoints, whose refusals carry the RFC
/// `error` (ADR-0042 d2).
fn finish_coded(request_id: &str, mut response: Response, error_code: Option<&str>) -> Response {
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
        .extensions_mut()
        .insert(traffic::Resolved::now(error_code));
    response
}

fn json_response(status: StatusCode, body: &Value) -> Response {
    let mut response = Response::new(Body::from(body.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

fn bearer(headers: &HeaderMap) -> Result<String, ApiError> {
    let token = bearer_value(headers);
    if token.is_err() {
        // Refused before any lookup: no key resolved (ADR-0042 d14).
        traffic::credential(None, None);
    }
    token
}

fn bearer_value(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(ApiError::auth_missing)?;
    let token = value
        .to_str()
        .ok()
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(ApiError::auth_missing)?;
    if token.is_empty() {
        return Err(ApiError::auth_missing());
    }
    Ok(token.to_owned())
}

/// Who a handler acts as: the key value in `Authorization` on a ring route,
/// or, where the facade dispatches for a connection, the caller its access
/// token already resolved to at `/mcp`, so a token is never read as a
/// credential on a ring route (ADR-0031 d10, d11).
enum Acting<'a> {
    Value(String),
    Connection(&'a Caller),
    /// A caller this request already resolved: a session key's, from a
    /// capacity credential by the session gate (ADR-0070 d2), or a console
    /// act's, resolved once before it claims its `Idempotency-Key`, so the
    /// act reads the key's grants again but is not charged a second time
    /// against its rate (#253).
    Session(Arc<Caller>),
}

impl Acting<'_> {
    fn bearer(&self) -> Bearer<'_> {
        match self {
            Acting::Value(value) => Bearer::Key(value),
            Acting::Connection(caller) => Bearer::Connection(caller),
            Acting::Session(caller) => Bearer::Session(caller),
        }
    }
}

fn acting<'a>(headers: &HeaderMap, connection: Option<&'a Caller>) -> Result<Acting<'a>, ApiError> {
    match connection {
        Some(caller) => Ok(Acting::Connection(caller)),
        None => match session::session_caller() {
            Some(caller) => Ok(Acting::Session(caller)),
            None => bearer(headers).map(Acting::Value),
        },
    }
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get("idempotency-key")
        .ok_or_else(ApiError::idempotency_key_required)?;
    let key = value
        .to_str()
        .map_err(|_| ApiError::validation("Idempotency-Key must be visible ASCII"))?;
    if key.is_empty() || key.len() > 256 {
        return Err(ApiError::validation(
            "Idempotency-Key must be 1..=256 characters",
        ));
    }
    Ok(key.to_owned())
}

/// ADR-0022 d4's per-request target header, which v0.4.0 removed.
const WORKAREA_HEADER: &str = "x-oml-workarea";

/// v0.3.0's `X-OML-Workarea` is refused, whatever it holds, naming the
/// `workarea` field that replaces it on this route: `/workarea` in a body,
/// `workarea` in a query (ADR-0039 d7, d11).
fn refuse_workarea_header(headers: &HeaderMap, renamed_to: &str) -> Result<(), ApiError> {
    if headers.contains_key(WORKAREA_HEADER) {
        return Err(ApiError::renamed("X-OML-Workarea", renamed_to));
    }
    Ok(())
}

/// A page's `limit`, at most `api.page_limit_max` (openapi.yaml `Limit`).
fn page_limit(value: &str) -> Result<usize, ApiError> {
    value
        .parse()
        .ok()
        .filter(|l| (1..=PAGE_LIMIT_MAX).contains(l))
        .ok_or_else(|| {
            ApiError::validation(format!("limit must be between 1 and {PAGE_LIMIT_MAX}"))
        })
}

fn parse_json<T: serde::de::DeserializeOwned>(
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<T, ApiError> {
    require_json(headers)?;
    serde_json::from_slice(body).map_err(not_the_shape)
}

/// [`parse_json`] for a body v0.4.0 renamed fields of: `renamed` sees the
/// body as JSON before it is read as `T`, so a v0.3.0 spelling is refused
/// naming its successor ([`ApiError::renamed`]) where the closed `T` would
/// refuse it only as an unknown field (ADR-0039 d7, d11).
fn parse_json_renamed<T: serde::de::DeserializeOwned>(
    headers: &HeaderMap,
    body: &Bytes,
    renamed: impl Fn(&Value) -> Result<(), ApiError>,
) -> Result<T, ApiError> {
    require_json(headers)?;
    let value: Value = serde_json::from_slice(body).map_err(not_the_shape)?;
    renamed(&value)?;
    serde_json::from_value(value).map_err(not_the_shape)
}

/// The JSON object at `pointer`, when `body` is one: each v0.3.0 member
/// name in `renames` it carries is refused naming its successor, both as
/// JSON Pointers under `pointer` (ADR-0039 d7).
fn refuse_renamed(body: &Value, pointer: &str, renames: &[(&str, &str)]) -> Result<(), ApiError> {
    let Some(object) = body.as_object() else {
        return Ok(());
    };
    match renames.iter().find(|(old, _)| object.contains_key(*old)) {
        Some((old, new)) => Err(ApiError::renamed(
            &format!("{pointer}/{old}"),
            &format!("{pointer}/{new}"),
        )),
        None => Ok(()),
    }
}

/// `POST /v2/keys`'s v0.3.0 spellings, at the top, in each grant and in
/// each grant's deny entries (ADR-0039 d7).
fn key_issue_renamed(body: &Value) -> Result<(), ApiError> {
    refuse_renamed(
        body,
        "",
        &[
            ("default_workarea_id", "default_workarea"),
            ("tenant_id", "tenant"),
        ],
    )?;
    let grants = body.get("grants").and_then(Value::as_array);
    for (i, grant) in grants.into_iter().flatten().enumerate() {
        let at = format!("/grants/{i}");
        refuse_renamed(grant, &at, &[("workarea_node_id", "workarea")])?;
        let deny = grant.get("deny").and_then(Value::as_array);
        for (j, entry) in deny.into_iter().flatten().enumerate() {
            refuse_renamed(
                entry,
                &format!("{at}/deny/{j}"),
                &[("subtree_node_id", "workarea")],
            )?;
        }
    }
    Ok(())
}

/// A body sent under any media type but `application/json` is refused
/// before it is parsed (errors.md `request.unsupported_media`).
fn require_json(headers: &HeaderMap) -> Result<(), ApiError> {
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        let essence = content_type
            .to_str()
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if essence != "application/json" {
            return Err(ApiError::unsupported_media(&essence));
        }
    }
    Ok(())
}

fn not_the_shape(e: serde_json::Error) -> ApiError {
    ApiError::validation(format!(
        "the request body is not this operation's shape: {e}"
    ))
}

/// A grant's `covers`, `subtree` when omitted; any other value is
/// `request.validation` naming `/grants/<i>/covers` (ADR-0044 d4).
fn parse_covers(index: usize, covers: Option<&str>) -> Result<Covers, ApiError> {
    match covers {
        None => Ok(Covers::Subtree),
        Some(value) => Covers::parse(value).ok_or_else(|| {
            ApiError::validation(format!(
                "covers is subtree or node, not {value:?} (ADR-0044 d4)"
            ))
            .with_details(json!({ "field": format!("/grants/{index}/covers") }))
        }),
    }
}

fn parse_role(role: &str) -> Result<Role, ApiError> {
    match role {
        "reader" => Ok(Role::Reader),
        "writer" => Ok(Role::Writer),
        "dreamer" => Ok(Role::Dreamer),
        "admin" => Ok(Role::Admin),
        "operator" => Ok(Role::Operator),
        other => Err(ApiError::validation(format!(
            "role must be one of reader, writer, dreamer, admin, operator — not {other:?}"
        ))),
    }
}

/// A grant role is reader, writer or dreamer, never admin or operator
/// (openapi.yaml `GrantRequest.role`); whether the key role admits it is the
/// app's issuance check.
fn parse_grant_role(role: &str) -> Result<Role, ApiError> {
    match role {
        "reader" => Ok(Role::Reader),
        "writer" => Ok(Role::Writer),
        "dreamer" => Ok(Role::Dreamer),
        other => Err(ApiError::validation(format!(
            "a grant role is one of reader, writer, dreamer — not {other:?}"
        ))),
    }
}

/// The spec's `KeyIssued` (openapi.yaml): the key as `key.issued` journalled
/// it, each stored grant with its id and epoch, the key's permission epoch
/// and the value, shown once (ADR-0025 d2).
fn key_issued_json(issued: &IssuedKey, label: Option<&str>) -> Value {
    let grants: Vec<Value> = issued.access.grants.iter().map(grant_json).collect();
    let mut body = json!({
        "key_id": issued.key_id,
        "name": issued.name.as_str(),
        "role": issued.role.as_str(),
        "principal": issued.principal,
        "crew_id": issued.crew_id,
        "user": issued.user.as_ref().map(named_ref_json),
        "default_workarea_id": issued.default_workarea,
        "held_by": issued.held_by.map(HeldBy::as_str),
        "guardrail_scope": issued.guardrail_scope.as_str(),
        "label": label,
        "expiry": Value::Null,
        "revoked_at": Value::Null,
        "rotated_from": Value::Null,
        "grants": grants,
        "denials": [],
        "epoch": issued.access.permission_epoch(),
    });
    match &issued.connect_code {
        // A connector key has no value; its code stands in the value's
        // place, shown once (ADR-0031 d7).
        Some(code) => body["connect_code"] = connect_code_json(code),
        // Show-once: the value appears exactly here, journalled as
        // issued-without-value (D14).
        None => body["value"] = json!(issued.value),
    }
    body
}

/// openapi.yaml `Denial`: the denial `POST /v2/keys/{key}/denials` adds.
fn denial_json(denial: &oml_app::types::DenialRecord) -> Value {
    json!({
        "denial_id": denial.denial.denial_id,
        "workarea_node_id": denial.denial.node,
        "epoch": denial.epoch,
        // Lifting is not built (ADR-0025 d2).
        "lifted": false,
    })
}

/// openapi.yaml `ConnectCode`: the code shown once, its expiry and the URL
/// to enter as the custom connector (ADR-0031 d7, d8).
fn connect_code_json(code: &ConnectCodeIssued) -> Value {
    json!({
        "code_id": code.code_id,
        "code": code.code,
        "expires_at": code.expires_at.as_str(),
        "connector_url": code.connector_url,
    })
}

/// openapi.yaml `Connection`: one of a key's open or revoked connections,
/// never a token or a digest of one (ADR-0031 d10, d11).
fn connection_json(c: &ConnectionRecord) -> Value {
    json!({
        "connection_id": c.connection_id,
        "client_id": c.client_id,
        "client_kind": c.client_kind.as_str(),
        "redirect_uri": c.redirect_uri,
        "resource": c.resource,
        "state": c.state.as_str(),
        "revoked_cause": match c.state {
            ConnectionState::Revoked(cause) => Value::from(cause.as_str()),
            _ => Value::Null,
        },
    })
}

/// openapi.yaml `Grant`: a stored grant with its id and its epoch, the seq
/// of its `grant.issued` (ADR-0025 d2).
fn grant_json(g: &GrantRecord) -> Value {
    json!({
        "grant_id": g.grant.grant_id,
        "workarea_node_id": g.grant.node,
        "role": g.grant.role.as_str(),
        "covers": g.grant.covers.as_str(),
        "deny": g
            .grant
            .deny
            .iter()
            .map(|d| json!({ "subtree_node_id": d }))
            .collect::<Vec<_>>(),
        "epoch": g.epoch,
    })
}

/// openapi.yaml `Key` as the key read serves it: the row, its attestation as
/// `key.issued` journalled it, its stored grants, its denials in force and
/// its permission epoch, and never the value (ADR-0025 d2; ADR-0026 d8, d9).
/// `revoked_at` is the row's, the `at` of the key's `key.revoked`, and null
/// for a live key, so a revoked key, whose name still resolves, reads as
/// history and never as live (ADR-0026 d10). Expiry and rotation are
/// unbuilt, so `expiry` and `rotated_from` are null. Nothing stores the
/// label, so the read omits `label` rather than serve a value it has not
/// read.
fn key_json(detail: &KeyDetail, connections: &[ConnectionRecord]) -> Value {
    let key = &detail.key;
    json!({
        "key_id": key.key_id,
        "credential": key.credential.as_str(),
        "connections": connections.iter().map(connection_json).collect::<Vec<_>>(),
        "name": key.name.as_ref().map(Name::as_str),
        "role": key.role.as_str(),
        "principal": key.principal,
        "crew_id": key.crew_id,
        "user": detail.user.as_ref().map(named_ref_json),
        "default_workarea_id": key.default_workarea,
        "held_by": detail.held_by.map(HeldBy::as_str),
        "guardrail_scope": detail.guardrail_scope.as_str(),
        "profile": key.profile.map(KeyProfile::as_str),
        "expiry": Value::Null,
        "revoked_at": key.revoked_at.as_ref().map(|at| at.as_str()),
        "rotated_from": Value::Null,
        "grants": detail.access.grants.iter().map(grant_json).collect::<Vec<_>>(),
        "denials": detail
            .access
            .denials
            .iter()
            .map(|d| json!({
                "denial_id": d.denial.denial_id,
                "workarea_node_id": d.denial.node,
                "epoch": d.epoch,
                // Only the denials in force are listed (ADR-0025 d2).
                "lifted": false,
            }))
            .collect::<Vec<_>>(),
        "epoch": detail.access.permission_epoch(),
    })
}

/// openapi.yaml `Whoami`: the calling key's own record, each reference
/// `{id, name}` and the default workarea with its `path`, its stored grants
/// without their deny masks, and never its denials (ADR-0039 d15).
fn whoami_json(whoami: &Whoami) -> Value {
    let key = &whoami.key;
    let named = |r: &oml_app::types::NamedRef| json!({ "id": r.id, "name": r.name.as_str() });
    json!({
        "key_id": key.key_id,
        "name": key.name.as_ref().map(Name::as_str),
        "role": key.role.as_str(),
        "principal": key.principal,
        "held_by": key.held_by.as_str(),
        "tenant": whoami.tenant.as_ref().map(named),
        "default_workarea": whoami.default_workarea.as_ref().map(|w| json!({
            "id": w.id,
            "name": w.name.as_str(),
            "path": w.path,
        })),
        "crew": whoami.crew.as_ref().map(named),
        "user": whoami.user.as_ref().map(named),
        "grants": whoami.grants.iter().map(whoami_grant_json).collect::<Vec<_>>(),
        // The scope the key's issuance set (proposed ADR-0037 d5, as
        // ADR-0039 d15 reserved).
        "guardrail_scope": key.guardrail_scope.as_str(),
        // The key's profile, null for none (ADR-0030 d8).
        "profile": key.profile.map(KeyProfile::as_str),
    })
}

/// `whoami`'s `person`: the user a key names, whether it is under the
/// sign-up quotas, its usage and limits there, and its pending deletion
/// request (ADR-0070 d5; ADR-0068 d5, d6, d10).
fn person_json(person: &oml_app::app::Person) -> Value {
    let (usage, quotas) = match &person.limits {
        Some((usage, quotas)) => (
            json!({
                "memories": usage.memories,
                "keys": usage.keys,
                "connections": usage.connections,
            }),
            json!({
                "memories": quotas.memories,
                "keys": quotas.keys,
                "connections": quotas.connections,
                "requests_per_minute": quotas.requests_per_minute,
            }),
        ),
        None => (Value::Null, Value::Null),
    };
    let mut out = json!({
        "user": person.user.as_str(),
        "under_quotas": person.limits.is_some(),
        "usage": usage,
        "quotas": quotas,
        "deletion_requested_at": person.deletion_requested_at.as_ref().map(|t| t.as_str()),
    });
    // The onboarding status, only when asked (`start=true`): every page's
    // session check calls whoami (ADR-0070 d5, d8).
    if let (Some(status), Some(obj)) = (&person.start, out.as_object_mut()) {
        obj.insert(
            "start".to_owned(),
            json!({
                "connected": status.connected,
                "first_memory": match status.first_memory {
                    Some(oml_app::domain::MemoryStatus::Held) => Some("held"),
                    Some(oml_app::domain::MemoryStatus::Promoted) => Some("promoted"),
                    _ => None,
                },
            }),
        );
    }
    out
}

/// openapi.yaml `WhoamiGrant`: a stored grant as `whoami` serves it, its
/// id, node, role and epoch and never its per-grant deny masks, which name
/// subtrees the key may not read through it, the reason `whoami` omits the
/// key's denials (ADR-0039 d15).
fn whoami_grant_json(g: &GrantRecord) -> Value {
    json!({
        "grant_id": g.grant.grant_id,
        "workarea_node_id": g.grant.node,
        "role": g.grant.role.as_str(),
        "covers": g.grant.covers.as_str(),
        "epoch": g.epoch,
    })
}

/// A reference `{id, name}`, as `user` is served (ADR-0044 d11).
fn named_ref_json(r: &oml_app::types::NamedRef) -> Value {
    json!({ "id": r.id, "name": r.name.as_str() })
}

/// `as_of`, an RFC 3339 `date-time` with an offset and at most six
/// fractional digits, as the fixed-width instant it names; a date, a period
/// or anything else is `request.validation` naming `field` (ADR-0059 d14).
pub(crate) fn parse_as_of(value: Option<&str>, field: &str) -> Result<Option<EventTime>, ApiError> {
    value
        .map(|v| {
            parse_date_time(v).ok_or_else(|| {
                ApiError::validation(
                    "as_of must be an RFC 3339 date-time with an offset and at most microsecond precision",
                )
                .with_details(json!({ "field": field }))
            })
        })
        .transpose()
}

/// Get's query parameters: `as_of` alone, at most once (ADR-0059 d14,
/// d19); any other is refused, never ignored.
fn get_as_of(params: &[(String, String)]) -> Result<Option<EventTime>, ApiError> {
    match params {
        [] => Ok(None),
        [(name, value)] if name == "as_of" => parse_as_of(Some(value), "as_of"),
        [(name, _)] => Err(ApiError::validation(format!(
            "unknown query parameter {name:?}"
        ))),
        _ => Err(ApiError::validation("get takes one query parameter, as_of")),
    }
}

/// The listing's query parameters (openapi.yaml `searchMemories`): each
/// at most once, and an unknown one refused rather than ignored, so a
/// mistyped filter never widens a listing; v0.3.0's `workarea_id` names
/// its successor, `workarea` (ADR-0039 d7).
fn list_request(params: &[(String, String)]) -> Result<ListRequest, ApiError> {
    let mut req = ListRequest {
        own: false,
        workarea: None,
        state: None,
        kind: None,
        claim_key: None,
        since: None,
        until: None,
        limit: PAGE_LIMIT_DEFAULT,
        cursor: None,
        anchor: AnchorFilter::default(),
        author: None,
        tip: None,
        as_of: None,
    };
    let mut anchor_repo = None;
    let mut anchor_path_prefix = None;
    let mut origin_kinds: Option<Vec<String>> = None;
    let mut seen = HashSet::new();
    for (name, value) in params {
        // origin_kinds may be sent comma-separated or repeated, as a
        // generated client explodes a list (ADR-0063 d3).
        if name == "origin_kinds" {
            origin_kinds
                .get_or_insert_with(Vec::new)
                .extend(value.split(',').map(ToOwned::to_owned));
            continue;
        }
        if !seen.insert(name.as_str()) {
            return Err(ApiError::validation(format!(
                "the query parameter {name} is given more than once"
            )));
        }
        let date_time = || {
            parse_date_time(value).ok_or_else(|| {
                ApiError::validation(format!(
                    "{name} must be an RFC 3339 date-time of at most microsecond precision"
                ))
            })
        };
        match name.as_str() {
            "own" => {
                req.own = match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(ApiError::validation("own must be true or false")),
                };
            }
            "workarea" => req.workarea = Some(reference("workarea", value)?),
            "workarea_id" => return Err(ApiError::renamed("workarea_id", "workarea")),
            "claim_key" => req.claim_key = Some(value.clone()),
            "state" => req.state = Some(parse_state(value)?),
            "kind" => req.kind = Some(parse_kind(value)?),
            "since" => req.since = Some(date_time()?),
            "until" => req.until = Some(date_time()?),
            "as_of" => req.as_of = parse_as_of(Some(value), "as_of")?,
            "limit" => {
                req.limit = value
                    .parse()
                    .ok()
                    .filter(|l| (1..=PAGE_LIMIT_MAX).contains(l))
                    .ok_or_else(|| {
                        ApiError::validation(format!(
                            "limit must be between 1 and {PAGE_LIMIT_MAX}"
                        ))
                    })?;
            }
            "cursor" => req.cursor = Some(value.clone()),
            "anchor_repo" => anchor_repo = Some(value.as_str()),
            "anchor_path_prefix" => anchor_path_prefix = Some(value.as_str()),
            // `with_own=true` is the merged listing's; false is this one.
            "with_own" if value == "false" => {}
            "with_own" => return Err(ApiError::validation("with_own must be true or false")),
            other => {
                return Err(ApiError::validation(format!(
                    "unknown query parameter {other:?}"
                )));
            }
        }
    }
    req.anchor = anchor_filter(anchor_repo, anchor_path_prefix, origin_kinds.as_deref(), "")?;
    Ok(req)
}

/// The origin filters, each checked against its grammar and refused naming
/// it (ADR-0057 d3, d4); `base` is `/` for a body's field and empty for a
/// query parameter.
pub(crate) fn anchor_filter(
    repo: Option<&str>,
    path_prefix: Option<&str>,
    origin_kinds: Option<&[String]>,
    base: &str,
) -> Result<AnchorFilter, ApiError> {
    let origin_kinds = match origin_kinds {
        None => Vec::new(),
        Some(kinds) => {
            origin_grammar::parse_kinds(kinds.iter().map(String::as_str)).map_err(|why| {
                ApiError::validation(format!("origin_kinds: {why}"))
                    .with_details(json!({ "field": format!("{base}origin_kinds") }))
            })?
        }
    };
    if let Some(repo) = repo {
        anchor_grammar::check_repo(repo).map_err(|why| {
            ApiError::validation(format!("anchor_repo: {why}"))
                .with_details(json!({ "field": format!("{base}anchor_repo") }))
        })?;
    }
    if let Some(prefix) = path_prefix {
        anchor_grammar::check_path_prefix(prefix).map_err(|why| {
            ApiError::validation(format!("anchor_path_prefix: {why}"))
                .with_details(json!({ "field": format!("{base}anchor_path_prefix") }))
        })?;
    }
    Ok(AnchorFilter {
        repo: repo.map(ToOwned::to_owned),
        path_prefix: path_prefix.map(ToOwned::to_owned),
        origin_kinds,
    })
}

/// A body's `origin`, a closed object whose `kind` is one of the seven,
/// refused naming the field (ADR-0063 d1).
pub(crate) fn parse_origin(origin: Option<&Value>) -> Result<Option<OriginKind>, ApiError> {
    origin
        .filter(|o| !o.is_null())
        .map(|o| {
            OriginKind::from_json(o, "/origin").map_err(|e| {
                ApiError::validation(format!("origin: {e}"))
                    .with_details(json!({ "field": e.field }))
            })
        })
        .transpose()
}

/// A body's `anchor`, checked against the grammar and refused naming the
/// field, such as `/anchor/path` (ADR-0057 d3).
pub(crate) fn parse_anchor(anchor: Option<&Value>) -> Result<Option<Anchor>, ApiError> {
    anchor
        .filter(|a| !a.is_null())
        .map(|a| {
            Anchor::from_json(a, "/anchor").map_err(|e| {
                ApiError::validation(format!("anchor: {e}"))
                    .with_details(json!({ "field": e.field }))
            })
        })
        .transpose()
}

/// A supersession's `expected_version`: required, an integer of at least 1
/// (ADR-0057 d19).
pub(crate) fn parse_expected_version(sent: Option<Value>) -> Result<u32, ApiError> {
    let refused = |why: &str| {
        ApiError::validation(format!("expected_version: {why}"))
            .with_details(json!({ "field": "/expected_version" }))
    };
    let Some(sent) = sent else {
        return Err(refused(
            "required: the claim's latest version as you last read it",
        ));
    };
    sent.as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v >= 1)
        .ok_or_else(|| refused("an integer of at least 1"))
}

/// A listing's `state` filter (openapi.yaml `searchMemories`).
fn parse_state(state: &str) -> Result<MemoryStatus, ApiError> {
    match state {
        "held" => Ok(MemoryStatus::Held),
        "promoted" => Ok(MemoryStatus::Promoted),
        "demoted" => Ok(MemoryStatus::Demoted),
        "rejected" => Ok(MemoryStatus::Rejected),
        "erased" => Ok(MemoryStatus::Erased),
        other => Err(ApiError::validation(format!("unknown state {other:?}"))),
    }
}

/// The submit body's kind, stored as sent (http-api.md submit; ADR-0025
/// d8); an unknown one is refused, never coerced to `memory`. Kind
/// `guardrail` carries its `severity` and `rule` beside it (proposed
/// ADR-0037 d10).
fn parse_kind(kind: &str) -> Result<MemoryKind, ApiError> {
    MemoryKind::parse(kind).ok_or_else(|| ApiError::validation(format!("unknown kind {kind:?}")))
}

/// A guardrail's `severity`, `never`, `must` or `should`; any other word is
/// refused naming the field, never coerced (proposed ADR-0037 d2). Whether
/// the kind admits one is the use case's to decide (d10).
fn parse_severity(severity: Option<&str>) -> Result<Option<Severity>, ApiError> {
    severity
        .map(|word| {
            Severity::parse(word).ok_or_else(|| {
                ApiError::validation(format!(
                    "severity must be never, must or should, not {word:?}"
                ))
                .with_details(json!({ "field": "/severity" }))
            })
        })
        .transpose()
}

/// A key's `guardrail_scope`, `workarea` or `ancestors`; any other word,
/// `off` included, is refused naming the field (proposed ADR-0037 d4).
/// A key's `profile`: `curator` or omitted (ADR-0030 d8).
fn parse_profile(profile: Option<&str>) -> Result<Option<KeyProfile>, ApiError> {
    profile
        .map(|word| {
            KeyProfile::parse(word).ok_or_else(|| {
                ApiError::validation(format!("profile must be curator, not {word:?}"))
                    .with_details(json!({ "field": "/profile" }))
            })
        })
        .transpose()
}

fn parse_guardrail_scope(scope: Option<&str>) -> Result<Option<GuardrailScope>, ApiError> {
    scope
        .map(|word| {
            GuardrailScope::parse(word).ok_or_else(|| {
                ApiError::validation(format!(
                    "guardrail_scope must be workarea or ancestors, not {word:?}: no off value exists, and an admin who wants none issues the key where none applies"
                ))
                .with_details(json!({ "field": "/guardrail_scope" }))
            })
        })
        .transpose()
}

/// The recall `kinds` filter: absent is no filter, and a sent one names at
/// least one of `memory` and `derived_fact`, the only kinds it filters —
/// steering is never ranked and the vocabulary kinds are no filter value
/// (openapi.yaml `RecallRequest.kinds`; ADR-0021 d9, ADR-0025 d16).
fn parse_hit_kinds(kinds: Option<&[String]>) -> Result<Vec<MemoryKind>, ApiError> {
    let Some(kinds) = kinds else {
        return Ok(Vec::new());
    };
    if kinds.is_empty() {
        return Err(ApiError::validation(
            "an empty kinds is refused: omit kinds for no filter",
        ));
    }
    kinds
        .iter()
        .map(|name| match MemoryKind::parse(name) {
            Some(kind @ (MemoryKind::Memory | MemoryKind::DerivedFact)) => Ok(kind),
            // Recall never serves a guardrail; the filter that names one is
            // refused, never answered empty, and points at the one read
            // that serves the set (proposed ADR-0037 d5, d18 Q7).
            Some(MemoryKind::Guardrail) => Err(ApiError::validation(
                "recall never serves kind guardrail: GET /v1/guardrails serves the set in force",
            )
            .with_details(json!({ "field": "/kinds" }))),
            _ => Err(ApiError::validation(format!(
                "kinds names memory or derived_fact, not {name:?}"
            ))),
        })
        .collect()
}

/// No upload sessions exist in the PoC, so every attachment digest fails the
/// completed-session check — fail closed (errors.md submission.attachment_missing).
fn refuse_attachments(attachments: &[String]) -> Result<(), ApiError> {
    if attachments.is_empty() {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "submission.attachment_missing",
            "The claim and its files are one signed unit; the envelope binds only bytes the service already holds and has digest-verified (D3).",
            "no completed upload session owns these digests (attachment uploads are outside the PoC slice)",
        ))
    }
}

/// No simulations exist in the PoC; a named one is absent, and the fence
/// hides rather than advertises (errors.md `simulation` preamble).
fn refuse_simulation(simulation_id: Option<&String>) -> Result<(), ApiError> {
    match simulation_id {
        None => Ok(()),
        Some(_) => Err(ApiError::not_found()),
    }
}

/// The lead write-path shape from the oml-pg phase: a chain-head conflict is
/// a typed refusal, and the writer re-reads the head (each `App` call does)
/// and retries.
pub(crate) async fn retry_chain<T, Fut, F>(mut call: F) -> Result<T, AppError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let mut attempt = 0;
    loop {
        match call().await {
            Err(AppError::Store(
                StoreError::SeqConflict { .. } | StoreError::PrevHashConflict { .. },
            )) if attempt + 1 < CHAIN_RETRIES => attempt += 1,
            other => return other,
        }
    }
}

/// The spec's `Memory` object (openapi.yaml components.schemas.Memory) as
/// the calling key is served it (ADR-0025 d3, d12). Fields the slice does
/// not model — provenance, simulation, self-signing, attachments, held-item
/// expiry — are emitted at their fixed PoC values rather than omitted, so
/// clients see the full shape.
#[must_use]
/// A version's latest source check, served only on a promoted version: a
/// held one carries none, so no mark ever derives from it (ADR-0057 d5).
fn source_check(m: &MemoryRecord) -> Option<&SourceCheck> {
    (m.status == MemoryStatus::Promoted)
        .then_some(m.source_check.as_deref())
        .flatten()
}

/// `ScanFinding` (openapi.yaml; ADR-0074 d23): one finding of a version's
/// list, never the matched text.
pub fn scan_finding_json(f: &oml_app::types::VersionFinding) -> Value {
    json!({
        "class": f.class,
        "rule": f.rule,
        "verdict": f.verdict.as_str(),
        "rule_set": f.rule_set,
        "field": f.field,
        "line": f.line,
        "via": f.via,
        "span": f.span.map(|(a, b)| [a, b]),
        "found_by": f.found_by.as_str(),
        "event_seq": f.event_seq,
        "migrated_from": f.migrated_from,
    })
}

/// `SourceChecked` (openapi.yaml; ADR-0057 d5, d18).
pub fn source_checked_json(check: &SourceCheck) -> Value {
    json!({
        "rev": check.rev,
        "at": check.at.as_str(),
        "result": check.result.as_str(),
        "moved_to": check.moved_to.as_ref().map(|(path, fragment)| json!({
            "path": path,
            "fragment": fragment,
        })),
    })
}

pub fn memory_json(served: &ServedMemory, include_text: bool) -> Value {
    let m = &served.memory;
    let mut value = json!({
        "artifact_id": m.memory_id,
        "claim_key": m.claim_key,
        "version": m.version,
        "tip": m.is_tip,
        "workarea_id": m.workarea_id,
        "workarea_path": served.workarea_path,
        "kind": m.kind.as_str(),
        "state": m.status.as_str(),
        // Exactly when the served version is held (ADR-0030 d4).
        "unreviewed": m.status == MemoryStatus::Held,
        // A universe claim names no author outside its authoring key set
        // (ADR-0044 d9).
        "principal": if served.anonymous {
            Value::Null
        } else {
            Value::from(m.author_principal.clone())
        },
        // The authoring key's id, null wherever principal is (ADR-0044 d9;
        // ADR-0070 d5): the user page names the kind of its person's own
        // keys and filters a search by one of them.
        "author_key_id": if served.anonymous {
            Value::Null
        } else {
            Value::from(m.author_key_id.to_string())
        },
        // Admission's scan finding, `class: rule at place`, never the text
        // (ADR-0065 d8; ADR-0070 d5); null wherever principal is, so an
        // anonymous row carries its text and nothing admission recorded of
        // its writing (ADR-0044 d9; #253).
        "scan_finding": if served.anonymous {
            Value::Null
        } else {
            Value::from(m.scan_finding.clone())
        },
        "provenance": "agent",
        "simulation_id": Value::Null,
        "self_signed": false,
        "plaintext_digest": m.body_digest,
        "derives_from": m.derives_from,
        "attachments": [],
        // Budget tokens of the body under oml-tc/1, whether or not a budget
        // was sent (retrieval.md token budgets; ADR-0025 d6).
        "tokens": budget_tokens(m.body_bytes),
        "retirement": m.retirement,
        "replaces": served.replaces.map(|r| json!({
            "artifact_id": r.artifact_id,
            "version": r.version,
        })),
        // Held-item expiry is unbuilt (ADR-0025 d13).
        "held_expires_at": Value::Null,
        "created_at": m.created_at.as_str(),
        "promoted_at": m.promoted_at.as_ref().map(|at| at.as_str()),
        // When the version says it holds, as signed (proposed ADR-0059 d2).
        "valid_from": m.valid.from.as_ref().map(ValidEnd::as_str),
        "valid_until": m.valid.until.as_ref().map(ValidEnd::as_str),
        // The media type signed with the version and its title (ADR-0056
        // d1, d12; ADR-0050 d5).
        "format": m.format.media_type,
        "title": m.format.title,
        // The author's declared origin, and the served version's latest
        // source check, from the source alone (ADR-0057 d1, d5).
        "anchor": m.anchor.as_deref().map_or(Value::Null, Anchor::to_json),
        // What kind of source the text came from, as signed (ADR-0063 d3).
        "origin": m.origin_kind.map_or(Value::Null, OriginKind::to_json),
        "stale": source_check(m).and_then(SourceCheck::stale),
        "source_checked": source_check(m).map(source_checked_json),
    });
    // The claim's text, `body` in v0.3.0 and still ADR-0004's storage
    // word (ADR-0039 d7).
    if include_text && let Some(obj) = value.as_object_mut() {
        obj.insert("text".to_owned(), Value::String(m.body.clone()));
    }
    // A guardrail version's own fields, and on a held one the sentence
    // every reader of it is told (proposed ADR-0037 d10, d14).
    if let (Some(fields), Some(obj)) = (m.guardrail.as_deref(), value.as_object_mut()) {
        obj.insert("severity".to_owned(), json!(fields.severity.as_str()));
        obj.insert(
            "rule".to_owned(),
            fields.rule.clone().unwrap_or(Value::Null),
        );
    }
    // On a held successor, the end approving it gives the version it
    // replaces, shown to a reviewer before the verdict (proposed ADR-0059
    // d4).
    if let (Some(end), Some(obj)) = (&served.replaces_valid_until, value.as_object_mut()) {
        obj.insert("replaces_valid_until".to_owned(), json!(end.as_str()));
    }
    if let (Some(warning), Some(obj)) = (served.load_warning, value.as_object_mut()) {
        obj.insert("load_warning".to_owned(), json!(warning));
    }
    // Every finding stored on the version, on the surfaces a reviewer
    // judges from; absent elsewhere (ADR-0074 d17, d23).
    if let (Some(list), Some(obj)) = (&served.scan_findings, value.as_object_mut()) {
        obj.insert(
            "scan_findings".to_owned(),
            Value::Array(list.iter().map(scan_finding_json).collect()),
        );
    }
    // What a derived format's claim will be found by, which a reviewer's
    // card shows before the verdict (ADR-0056 d9; ADR-0070 d5).
    if let (Some(found), Some(obj)) = (&m.format.search_text, value.as_object_mut()) {
        obj.insert("search_text".to_owned(), json!(found));
    }
    value
}

/// openapi.yaml `GuardrailSet`: the applicable set at the scope node, its
/// digest and cap flag, each served guardrail with its text, rule, depth,
/// verdicts (attestation and time, never a reviewer key) and conflicts, and
/// the caller's own held guardrails by key and version (proposed ADR-0037
/// d3, d5, d7).
pub(crate) fn guardrail_set_json(set: &GuardrailSet) -> Value {
    json!({
        "scope_node": set.scope_node,
        "scope_path": set.scope_path,
        "guardrail_scope": set.guardrail_scope.as_str(),
        "set_digest": set.set_digest,
        "over_cap": set.over_cap,
        "guardrails": set.guardrails.iter().map(|g| json!({
            "artifact_id": g.artifact_id,
            "claim_key": g.claim_key,
            "version": g.version,
            "workarea_id": g.workarea_id,
            "workarea_path": g.workarea_path,
            "depth": g.depth,
            "severity": g.severity.as_str(),
            "text": g.body,
            "rule": g.rule,
            "tokens": g.tokens,
            "promoted_at": g.promoted_at.as_str(),
            "verdicts": g.verdicts.iter().map(|v| json!({
                "held_by": v.held_by.as_str(),
                "at": v.at.as_str(),
            })).collect::<Vec<_>>(),
            "conflict": g.conflict,
        })).collect::<Vec<_>>(),
        "own_held": set.own_held.iter().map(|h| json!({
            "claim_key": h.claim_key,
            "version": h.version,
        })).collect::<Vec<_>>(),
    })
}

/// openapi.yaml `RecallBudget`: how the `max_tokens` cut was applied
/// (retrieval.md token budgets; ADR-0025 d7).
fn budget_json(budget: &Budget) -> Value {
    json!({
        "max_tokens": budget.max_tokens,
        "counter": COUNTER_ID,
        "steering_tokens": budget.steering_tokens,
        "used_tokens": budget.used_tokens,
        "truncated": budget.truncated,
        "hits_omitted": budget.hits_omitted,
        "next_hit_tokens": budget.next_hit_tokens,
    })
}

/// openapi.yaml `Edge` with `kind: claim` (http-api.md edges).
fn claim_edge_json(e: &ClaimEdgeProposal) -> Value {
    json!({
        "kind": "claim",
        "edge_id": e.edge_id,
        "from_artifact_id": e.from_artifact_id,
        "to_artifact_id": e.to_artifact_id,
        "label": e.label,
        "state": e.state,
        "proposer_key_id": e.proposer_key_id,
        "ratifier_key_id": e.ratifier_key_id,
    })
}

/// openapi.yaml `Edge` with `kind: brief` (ADR-0046 d7).
fn brief_edge_json(e: &BriefEdgeView) -> Value {
    json!({
        "kind": "brief",
        "edge_id": e.edge_id,
        "from_artifact_id": e.from_artifact_id,
        "to_workarea_id": e.to_workarea_id,
        "label": e.label,
        "state": e.state,
        "proposer_key_id": e.proposer_key_id,
        "ratifier_key_id": e.ratifier_key_id,
    })
}

/// A `kind` the served edge family does not take: `workarea` is designed,
/// anything else unknown; either is `request.validation` naming `/kind`.
fn edge_kind_refused(kind: Option<&str>) -> ApiError {
    let message = match kind {
        Some("workarea") => {
            "kind workarea is designed and not served: claim and brief are".to_owned()
        }
        Some(other) => format!("kind must be claim or brief, not {other:?}"),
        None => "kind is required: claim or brief".to_owned(),
    };
    ApiError::validation(message).with_details(json!({ "field": "/kind" }))
}

/// openapi.yaml `Crew`: a crew record, recorded and not operated
/// (ADR-0026 d8).
fn crew_json(crew: &CrewRecord) -> Value {
    json!({
        "crew_id": crew.crew_id,
        "name": crew.name.as_str(),
        "charter_uri": crew.charter_uri,
        "external_manager": crew.external_manager,
    })
}

/// openapi.yaml `Workarea`: the node, its name, its derived path, its
/// status and the crew record it names, or null when it names none
/// (ADR-0025 d14; ADR-0026 d1, d8).
fn workarea_json(workarea: &WorkareaRecord, path: &str, crew: Option<&CrewRecord>) -> Value {
    json!({
        "id": workarea.workarea_id,
        "name": workarea.name.as_str(),
        "parent_id": workarea.parent_id,
        "path": path,
        "status": workarea.status.as_str(),
        "crew": crew.map(crew_json),
        "user_root": workarea.user_root,
        // A person's key puts a workarea in progress (ADR-0030 d1, d13).
        "mode": if workarea.in_progress { "in_progress" } else { "standard" },
    })
}

/// openapi.yaml `WorkareaView`: the `Workarea` fields and the calling key's
/// children, edges, effective verbs and permission epoch (ADR-0025 d14;
/// ADR-0026 d8).
#[must_use]
pub fn workarea_view_json(info: &WorkareaInfo) -> Value {
    let mut value = workarea_json(&info.workarea, &info.path, info.crew.as_ref());
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "children".to_owned(),
            info.children
                .iter()
                .map(|c| {
                    json!({
                        "id": c.workarea_id,
                        "path_leaf": c.name.as_str(),
                        "status": c.status.as_str(),
                    })
                })
                .collect(),
        );
        // No workarea edge can exist in this build (ADR-0025 d14).
        obj.insert("edges".to_owned(), json!([]));
        obj.insert(
            "my_effective_verbs".to_owned(),
            json!(info.my_effective_verbs),
        );
        obj.insert("permission_epoch".to_owned(), json!(info.permission_epoch));
        obj.insert("stale_versions".to_owned(), json!(info.stale_versions));
        // The node's in-progress root, itself for a root, null outside
        // (ADR-0030 d13).
        obj.insert(
            "in_progress_root".to_owned(),
            info.in_progress_root.as_ref().map_or(
                Value::Null,
                |r| json!({ "id": r.workarea_id, "name": r.name.as_str() }),
            ),
        );
        obj.insert("policy".to_owned(), info.policy.clone());
        // The node's federation field, beside the policy (ADR-0072 d5).
        obj.insert(
            "federation".to_owned(),
            json!({ "accept_federated": info.federation.accept_federated }),
        );
        obj.insert(
            "pending_transitions".to_owned(),
            info.pending_transitions
                .iter()
                .map(transition_json)
                .collect(),
        );
        obj.insert(
            "brief".to_owned(),
            json!({
                "items": info
                    .brief
                    .items
                    .iter()
                    .map(|m| memory_json(m, true))
                    .collect::<Vec<_>>(),
                "budget": budget_json(&info.brief.budget),
            }),
        );
    }
    value
}

/// One transition in the openapi `Transition` shape: the proposing key only
/// where the read names it, and a ratified row's ratifier
/// (ADR-0046 d3; workareas.md, the status read).
#[must_use]
pub fn transition_json(t: &oml_app::types::TransitionView) -> Value {
    json!({
        "transition_id": t.transition_id,
        "workarea_id": t.workarea_id,
        "from": t.from.as_str(),
        "to": t.to.as_str(),
        "state": t.state.as_str(),
        "proposed_by_key_id": t.proposed_by_key_id,
        "ratified_by_key_id": Value::Null,
    })
}

/// The event-catalog envelope as the tail serves it (digest-only payloads by
/// construction — the journal never stored plaintext), with the actor named
/// only to a key in its own key set and the payload as the app serves it
/// (http-api.md events tail; ADR-0025 d15).
#[must_use]
pub fn event_json(tailed: &TailedEvent) -> Value {
    let e = &tailed.event;
    let (principal, api_key_id) = if tailed.names_actor {
        (json!(e.envelope.principal), json!(e.envelope.api_key_id))
    } else {
        (Value::Null, Value::Null)
    };
    json!({
        "seq": e.envelope.seq,
        "event_id": e.envelope.event_id,
        "event_type": e.envelope.event_type,
        "prev_hash": e.envelope.prev_hash.to_prefixed(),
        "hash": e.hash.to_prefixed(),
        "sig": hex::encode(e.sig.to_bytes()),
        "principal": principal,
        "api_key_id": api_key_id,
        "at": e.envelope.at.as_str(),
        "payload": tailed.payload,
    })
}

/// A relevance or floor, held as `f32`, printed as the decimal it was set
/// or scored as: six places, where the widening to `f64` would print
/// `0.004999999888241291` for 0.005 (ADR-0053 d7).
fn relevance(value: f32) -> f64 {
    (f64::from(value) * 1e6).round() / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;
    use oml_app::domain::access::Grant;

    /// openapi.yaml `WhoamiGrant`: a grant as `whoami` serves it carries
    /// its id, node, role and epoch and never its per-grant deny masks,
    /// which name subtrees the key may not read through it (ADR-0039 d15).
    #[test]
    fn whoami_serves_a_grant_without_its_deny_masks() {
        let sealed = Uuid::from_u128(7);
        let record = GrantRecord {
            grant: Grant {
                grant_id: Uuid::from_u128(1),
                node: Uuid::from_u128(2),
                role: Role::Writer,
                deny: vec![sealed],
                covers: Covers::Subtree,
            },
            epoch: 41,
        };
        assert_eq!(
            whoami_grant_json(&record),
            json!({
                "grant_id": Uuid::from_u128(1),
                "workarea_node_id": Uuid::from_u128(2),
                "role": "writer",
                "covers": "subtree",
                "epoch": 41,
            })
        );
    }
}
