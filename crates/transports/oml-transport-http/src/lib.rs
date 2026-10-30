//! `oml-transport-http` — the two-ring HTTP transport (workspace-plan.md:
//! transports depend on `oml-app` only). Paths, field spellings and status
//! codes follow docs/api/openapi.yaml; every refusal is a typed code from
//! docs/api/errors.md, attached here at the edge and nowhere else
//! (errors.md "Implementation mapping").
//!
//! The PoC slice: submit / get / recall / supersede on `/v1`; review next,
//! verdicts, key issuance, global per-key denials, workarea creation, the
//! events tail and the bootstrap claim on `/v2`; `/healthz` and `/readyz`
//! unversioned and unauthenticated (http-api.md "Health"); `/mcp`, the agent
//! plugin's stateless facade over those rings ([`mcp`]).

#![forbid(unsafe_code)]

pub mod error;
pub mod mcp;
pub mod time;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::App;
use oml_app::domain::budget::budget_tokens;
use oml_app::domain::{HeldBy, MemoryKind, Role, SignedEnvelope, nfc, sha256_digest_string};
use oml_app::ports::{Clock, IdGen, JournalStore, Scanner, Signer, StoreError};
use oml_app::types::{
    AppError, BootstrapRequest, GrantRequest, IssueKeyRequest, IssuedKey, RecallRequest,
    ServedMemory, SubmitRequest, SupersedeRequest, Verdict,
};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::time::{system_event_time, system_event_time_plus};

/// How many times a write retries after a chain-head conflict before giving
/// up — the concurrent-submitter shape the oml-pg phase prescribed: re-read
/// the head (the app does, per call) and try again.
pub const CHAIN_RETRIES: usize = 16;

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
    enrollment: Arc<Mutex<Option<String>>>,
    build: Arc<BuildInfo>,
}

impl<J: Clone, S, C, I, Sc> Clone for ApiState<J, S, C, I, Sc> {
    fn clone(&self) -> Self {
        Self {
            app: Arc::clone(&self.app),
            store: self.store.clone(),
            db_ping: Arc::clone(&self.db_ping),
            fork_check_ok: Arc::clone(&self.fork_check_ok),
            enrollment: Arc::clone(&self.enrollment),
            build: Arc::clone(&self.build),
        }
    }
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + Clone + 'static,
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
            enrollment: Arc::new(Mutex::new(enrollment_token)),
            build: Arc::new(BuildInfo::unknown()),
        }
    }

    /// The build `/readyz` reports; without it every build field is `unknown`.
    #[must_use]
    pub fn with_build(mut self, build: BuildInfo) -> Self {
        self.build = Arc::new(build);
        self
    }

    /// The two rings plus the unversioned probes, as one axum router.
    pub fn into_router(self) -> Router {
        Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(Self::readyz))
            .route("/v1/memories", post(Self::submit))
            .route("/v1/memories/recall", post(Self::recall))
            .route("/v1/memories/{artifact_id}", get(Self::get_memory))
            .route(
                "/v1/memories/{artifact_id}/supersede",
                post(Self::supersede),
            )
            .route("/v2/review/next", post(Self::review_next))
            .route("/v2/review/verdicts", post(Self::review_verdicts))
            .route("/v2/keys", post(Self::issue_key))
            .route("/v2/keys/{key_id}/denials", post(Self::deny_key))
            .route("/v2/workareas", post(Self::create_workarea))
            .route("/v2/events", get(Self::events_tail))
            .route("/v2/bootstrap/claim", post(Self::bootstrap_claim))
            .route("/mcp", post(Self::mcp).get(mcp::mcp_get))
            .with_state(self)
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

    async fn readyz(State(st): State<Self>) -> Response {
        let rid = new_request_id();
        let postgres_ok = (st.db_ping)().await;
        let fork_ok = st.fork_check_ok.load(Ordering::SeqCst);
        let body = json!({
            "status": if postgres_ok && fork_ok { "ok" } else { "unavailable" },
            "checks": {
                "postgres": if postgres_ok { "ok" } else { "unreachable" },
                "fork_check": if fork_ok { "ok" } else { "failed" },
            },
            // Ready or not, the probe names the build it belongs to.
            "build": st.build.to_json(),
        });
        let status = if postgres_ok && fork_ok {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        };
        finish(&rid, Ok(json_response(status, &body)))
    }

    async fn submit(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
        finish(&rid, Self::submit_inner(&st, &headers, &body).await)
    }

    async fn submit_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let idem_key = idempotency_key(headers)?;
        let req: SubmitBody = parse_json(headers, body)?;
        let kind = parse_kind(&req.kind)?;
        match req.verb.as_str() {
            "remember" => {}
            // learn asserts a derived claim: lineage is required, never
            // optional (errors.md submission.lineage_required).
            "learn" if req.derives_from.is_empty() => {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "submission.lineage_required",
                    "learn asserts a derived claim, so its lineage is required; a derived conclusion without nameable lineage is remember with kind derived_fact.",
                    "verb learn with an empty or omitted derives_from",
                ));
            }
            "learn" => {}
            other => {
                return Err(ApiError::validation(format!(
                    "verb must be remember or learn, not {other:?}"
                )));
            }
        }
        // The limit counts the NFC body, the form capture stores
        // (http-api.md limits; ADR-0025 d6).
        let text = nfc(&req.body);
        if text.len() > MAX_BODY_BYTES {
            return Err(ApiError::payload_too_large("the claim body"));
        }
        refuse_attachments(&req.attachments)?;
        refuse_simulation(req.simulation_id.as_ref())?;
        let workarea_id = workarea_override(headers)?;

        let app_req = SubmitRequest {
            idempotency_key: idem_key,
            claim_key: req.claim_key,
            kind,
            body: text,
            workarea_id,
            derives_from: req.derives_from,
        };
        let outcome = retry_chain(|| st.app.submit_memory(&token, &app_req))
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

    async fn get_memory(
        State(st): State<Self>,
        Path(artifact_id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let rid = new_request_id();
        finish(
            &rid,
            Self::get_memory_inner(&st, &artifact_id, &headers).await,
        )
    }

    async fn get_memory_inner(
        st: &Self,
        artifact_id: &str,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let token = bearer(headers)?;
        // Ids are opaque; one that can name no row is absent (access.not_found).
        let id = Uuid::parse_str(artifact_id).map_err(|_| ApiError::not_found())?;
        let memory = st
            .app
            .get_memory(&token, id)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &memory_json(&memory, true)))
    }

    async fn recall(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
        finish(&rid, Self::recall_inner(&st, &headers, &body).await)
    }

    async fn recall_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        // Read-shaped POST: no Idempotency-Key (http-api.md idempotency).
        let token = bearer(headers)?;
        let req: RecallBody = parse_json(headers, body)?;
        let k = req.k.unwrap_or(8);
        if !(1..=100).contains(&k) {
            return Err(ApiError::validation("k must be between 1 and 100"));
        }
        let include_bodies = req.include_bodies.unwrap_or(true);
        let workarea_id = workarea_override(headers)?;
        let result = st
            .app
            .recall(
                &token,
                &RecallRequest {
                    query: req.query.clone(),
                    k,
                    workarea_id,
                },
            )
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let hits: Vec<Value> = result
            .hits
            .iter()
            .map(|m| {
                json!({
                    "memory": memory_json(m, include_bodies),
                    // The PoC store ranks (ts_rank order) but does not expose
                    // the rank value; the score field is shape, not signal.
                    "score": 0.0,
                })
            })
            .collect();
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "steering": [],
                "hits": hits,
                "query_digest": sha256_digest_string(req.query.as_bytes()),
            }),
        ))
    }

    async fn supersede(
        State(st): State<Self>,
        Path(artifact_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let rid = new_request_id();
        finish(
            &rid,
            Self::supersede_inner(&st, &artifact_id, &headers, &body).await,
        )
    }

    async fn supersede_inner(
        st: &Self,
        artifact_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        let target_id = Uuid::parse_str(artifact_id).map_err(|_| ApiError::not_found())?;
        let req: SupersedeBody = parse_json(headers, body)?;
        if nfc(&req.body).len() > MAX_BODY_BYTES {
            return Err(ApiError::payload_too_large("the claim body"));
        }
        refuse_attachments(&req.attachments)?;
        let app_req = SupersedeRequest {
            target_id,
            body: req.body,
            claim_key: None, // the successor keeps the claim's key
        };
        let outcome = retry_chain(|| st.app.supersede(&token, &app_req))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        // The successor is the caller's own held row, so the author read is
        // always admitted — it supplies the claim_key/digest the receipt
        // shape (SubmitAccepted) carries.
        let successor = st
            .app
            .get_memory(&token, outcome.successor_id)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let m = &successor.memory;
        Ok(json_response(
            StatusCode::ACCEPTED,
            &json!({
                "artifact_id": outcome.successor_id,
                "claim_key": m.claim_key,
                "version": m.version,
                "state": "held",
                "plaintext_digest": m.body_digest,
                "journal_seq": outcome.journal_seq,
            }),
        ))
    }

    async fn review_next(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
        finish(&rid, Self::review_next_inner(&st, &headers, &body).await)
    }

    async fn review_next_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?; // claiming a serve is a side effect
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        if !body.is_empty() {
            // The optional subtree filter is accepted but the PoC serves the
            // whole tenant queue.
            let _filter: ReviewNextBody = parse_json(headers, body)?;
        }
        let served = st
            .app
            .review_next(&token)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Review))?;
        match served {
            None => {
                // 204: nothing in scope waits (openapi.yaml reviewNext).
                let mut response = Response::new(Body::empty());
                *response.status_mut() = StatusCode::NO_CONTENT;
                Ok(response)
            }
            Some(memory) => Ok(json_response(
                StatusCode::OK,
                &json!({
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
                }),
            )),
        }
    }

    async fn review_verdicts(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
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
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
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
        let memory_id = req
            .serve_id
            .strip_prefix("srv_")
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| ApiError::serve_invalid("unknown"))?;
        let outcome = retry_chain(|| {
            st.app
                .post_verdict(&token, memory_id, verdict, req.reason.as_deref())
        })
        .await
        .map_err(|e| map_app_error(e, ErrorContext::Review))?;
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "artifact_id": outcome.memory_id,
                "version": outcome.version,
                "verdict": req.verdict,
                "state": outcome.state.as_str(),
                "ratification_event_id": Value::Null,
                "journal_seq": outcome.journal_seq,
            }),
        ))
    }

    async fn issue_key(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
        finish(&rid, Self::issue_key_inner(&st, &headers, &body).await)
    }

    async fn issue_key_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        // The body is closed, nested objects included: a field the server
        // does not serve is refused, never dropped (ADR-0025 d16).
        let req: KeyIssueBody = parse_json(headers, body)?;
        // Issued keys do not expire yet: a requested expiry is refused by
        // name, since minting the key without it would leave the key its
        // access past the issuer's date (ADR-0025 d2, d16).
        if req.expiry.is_some() {
            return Err(ApiError::validation(
                "expiry on key issuance is not built: an issued key does not expire, so the field is refused, never dropped",
            ));
        }
        if req.tenant_id.is_some() {
            return Err(ApiError::validation(
                "tenant_id on key issuance (operator first-admin-key path) is outside the PoC slice",
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
        let grants = req
            .grants
            .iter()
            .map(|g| {
                Ok(GrantRequest {
                    node: g.workarea_node_id,
                    role: parse_grant_role(&g.role)?,
                    deny: g.deny.iter().map(|d| d.subtree_node_id).collect(),
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        let principal = req
            .principal
            .clone()
            .or_else(|| req.label.clone())
            .unwrap_or_else(|| "unnamed".to_owned());
        let issue = IssueKeyRequest {
            role,
            principal,
            held_by,
            default_workarea: req.default_workarea_id,
            grants,
        };
        let issued = retry_chain(|| st.app.issue_key(&token, &issue))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::CREATED,
            &key_issued_json(&issued, req.label.as_deref()),
        ))
    }

    async fn deny_key(
        State(st): State<Self>,
        Path(key_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let rid = new_request_id();
        finish(
            &rid,
            Self::deny_key_inner(&st, &key_id, &headers, &body).await,
        )
    }

    /// `POST /v2/keys/{key_id}/denials`: one journalled `key.denied`, its
    /// epoch its own seq (http-api.md keys, grants, denials; ADR-0025 d2).
    async fn deny_key_inner(
        st: &Self,
        key_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        // Ids are opaque; one that can name no key is absent.
        let key_id = Uuid::parse_str(key_id).map_err(|_| ApiError::not_found())?;
        let req: DenialBody = parse_json(headers, body)?;
        let denial = retry_chain(|| st.app.deny_key(&token, key_id, req.workarea_node_id))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::CREATED,
            &json!({
                "denial_id": denial.denial.denial_id,
                "workarea_node_id": denial.denial.node,
                "epoch": denial.epoch,
                // Lifting is not built (ADR-0025 d2).
                "lifted": false,
            }),
        ))
    }

    async fn create_workarea(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
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
        let token = bearer(headers)?;
        let _idem = idempotency_key(headers)?;
        let req: WorkareaCreateBody = parse_json(headers, body)?;
        if req.prefix.is_some() {
            return Err(ApiError::validation(
                "prefix (vocabulary workareas, ADR-0021) is outside the PoC slice",
            ));
        }
        let workarea = retry_chain(|| {
            st.app
                .create_workarea(&token, &req.name, Some(req.parent_id))
        })
        .await
        .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::CREATED,
            &json!({
                "id": workarea.workarea_id,
                "parent_id": workarea.parent_id,
                // The PoC derives no ltree; the node's own name stands in.
                "path": workarea.name,
                "status": "development",
                "crew": req.crew,
                "created_at": system_event_time().as_str(),
            }),
        ))
    }

    async fn events_tail(
        State(st): State<Self>,
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        let rid = new_request_id();
        finish(&rid, Self::events_tail_inner(&st, &params, &headers).await)
    }

    async fn events_tail_inner(
        st: &Self,
        params: &HashMap<String, String>,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let token = bearer(headers)?;
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
        let events = st
            .app
            .tail_events(&token, after_seq, limit)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        // head_seq, for measuring lag (http-api.md events tail). The key was
        // just authenticated by the app; the port lookup names its tenant.
        let key = st
            .store
            .key_by_hash(&sha256_digest_string(token.as_bytes()))
            .await
            .map_err(|e| map_app_error(AppError::Store(e), ErrorContext::Write))?
            .ok_or_else(|| ApiError::auth_invalid("unknown"))?;
        let head_seq = st
            .store
            .chain_head(key.tenant_id)
            .await
            .map_err(|e| map_app_error(AppError::Store(e), ErrorContext::Write))?
            .map_or(0, |h| h.seq);
        let items: Vec<Value> = events.iter().map(event_json).collect();
        Ok(json_response(
            StatusCode::OK,
            &json!({ "items": items, "head_seq": head_seq }),
        ))
    }

    async fn bootstrap_claim(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
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
        let taken = {
            let mut guard = st
                .enrollment
                .lock()
                .map_err(|_| ApiError::internal("enrollment token lock poisoned"))?;
            match guard.as_deref() {
                Some(t) if t == req.enrollment_token => guard.take(),
                Some(_) => return Err(ApiError::auth_invalid("unknown")),
                None => return Err(ApiError::auth_invalid("consumed")),
            }
        };
        let boot = BootstrapRequest {
            tenant_name: req.tenant_name.unwrap_or_else(|| "default".to_owned()),
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
}

// ---------------------------------------------------------------------------
// Request bodies (field spellings per openapi.yaml)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SubmitBody {
    verb: String,
    claim_key: String,
    kind: String,
    body: String,
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
}

#[derive(Deserialize)]
struct RecallBody {
    query: String,
    k: Option<usize>,
    #[serde(default)]
    #[allow(dead_code, reason = "kind filtering is outside the PoC recall slice")]
    kinds: Vec<String>,
    include_bodies: Option<bool>,
}

#[derive(Deserialize)]
struct SupersedeBody {
    body: String,
    #[serde(default)]
    attachments: Vec<String>,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "recorded reason text is outside the PoC journal payload"
    )]
    reason: Option<String>,
}

#[derive(Deserialize)]
struct ReviewNextBody {
    #[serde(default)]
    #[allow(dead_code, reason = "the PoC serves the whole tenant queue")]
    workarea: Option<String>,
}

#[derive(Deserialize)]
struct VerdictBody {
    serve_id: String,
    verdict: String,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DenyBody {
    subtree_node_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantBody {
    workarea_node_id: Uuid,
    role: String,
    #[serde(default)]
    deny: Vec<DenyBody>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyIssueBody {
    role: String,
    #[serde(default)]
    grants: Vec<GrantBody>,
    label: Option<String>,
    principal: Option<String>,
    /// Parsed only to be refused while key expiry is unbuilt (ADR-0025 d16).
    #[serde(default)]
    expiry: Option<String>,
    held_by: Option<String>,
    default_workarea_id: Option<Uuid>,
    tenant_id: Option<String>,
}

/// openapi.yaml `DenialRequest`, which the contract leaves open.
#[derive(Deserialize)]
struct DenialBody {
    workarea_node_id: Uuid,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "key.denied journals no reason (event-catalog.md); the text is accepted, not stored"
    )]
    reason: Option<String>,
}

#[derive(Deserialize)]
struct WorkareaCreateBody {
    parent_id: Uuid,
    name: String,
    #[serde(default)]
    crew: Option<Value>,
    #[serde(default)]
    prefix: Option<String>,
}

#[derive(Deserialize)]
struct BootstrapClaimBody {
    enrollment_token: String,
    // PoC extensions: the standalone ceremony names its tenant here.
    #[serde(default)]
    tenant_name: Option<String>,
    #[serde(default)]
    admin_principal: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn healthz() -> Response {
    let rid = new_request_id();
    finish(
        &rid,
        Ok(json_response(StatusCode::OK, &json!({"status": "ok"}))),
    )
}

fn new_uuid() -> Uuid {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source is available");
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

fn new_request_id() -> String {
    format!("req_{}", new_uuid().simple())
}

/// Every response carries `X-Request-Id`; error bodies already repeat it as
/// `request_id` (http-api.md conventions).
fn finish(request_id: &str, result: Result<Response, ApiError>) -> Response {
    let mut response = match result {
        Ok(response) => response,
        Err(e) => e.into_response(request_id),
    };
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
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

/// `X-OML-Workarea`: the provider ring's explicit per-request target override
/// (ADR-0022 d4); absent, the key's journalled default resolves in the app.
fn workarea_override(headers: &HeaderMap) -> Result<Option<Uuid>, ApiError> {
    match headers.get("x-oml-workarea") {
        None => Ok(None),
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(Some)
            .ok_or_else(|| ApiError::validation("X-OML-Workarea must be a workarea node id")),
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<T, ApiError> {
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
    serde_json::from_slice(body).map_err(|e| {
        ApiError::validation(format!(
            "the request body is not this operation's shape: {e}"
        ))
    })
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
    let grants: Vec<Value> = issued
        .access
        .grants
        .iter()
        .map(|g| {
            json!({
                "grant_id": g.grant.grant_id,
                "workarea_node_id": g.grant.node,
                "role": g.grant.role.as_str(),
                "deny": g
                    .grant
                    .deny
                    .iter()
                    .map(|d| json!({ "subtree_node_id": d }))
                    .collect::<Vec<_>>(),
                "epoch": g.epoch,
            })
        })
        .collect();
    json!({
        "key_id": issued.key_id,
        "role": issued.role.as_str(),
        "principal": issued.principal,
        "default_workarea_id": issued.default_workarea,
        "held_by": issued.held_by.map(HeldBy::as_str),
        "label": label,
        "expiry": Value::Null,
        "revoked_at": Value::Null,
        "rotated_from": Value::Null,
        "grants": grants,
        "denials": [],
        "epoch": issued.access.permission_epoch(),
        // Show-once: the value appears exactly here, journalled as
        // issued-without-value (D14).
        "value": issued.value,
    })
}

/// The submit body's kind, stored as sent (http-api.md submit; ADR-0025
/// d8); an unknown one is refused, never coerced to `memory`.
fn parse_kind(kind: &str) -> Result<MemoryKind, ApiError> {
    MemoryKind::parse(kind).ok_or_else(|| ApiError::validation(format!("unknown kind {kind:?}")))
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
async fn retry_chain<T, Fut, F>(mut call: F) -> Result<T, AppError>
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
pub fn memory_json(served: &ServedMemory, include_body: bool) -> Value {
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
        "principal": m.author_principal,
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
    });
    if include_body && let Some(obj) = value.as_object_mut() {
        obj.insert("body".to_owned(), Value::String(m.body.clone()));
    }
    value
}

/// The event-catalog envelope as the tail serves it (digest-only payloads by
/// construction — the journal never stored plaintext).
#[must_use]
pub fn event_json(e: &SignedEnvelope) -> Value {
    json!({
        "seq": e.envelope.seq,
        "event_id": e.envelope.event_id,
        "event_type": e.envelope.event_type,
        "prev_hash": e.envelope.prev_hash.to_prefixed(),
        "hash": e.hash.to_prefixed(),
        "sig": hex::encode(e.sig.to_bytes()),
        "principal": e.envelope.principal,
        "api_key_id": e.envelope.api_key_id,
        "at": e.envelope.at.as_str(),
        "payload": e.envelope.payload,
    })
}
