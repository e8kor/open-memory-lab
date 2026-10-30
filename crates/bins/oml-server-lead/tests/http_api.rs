//! Axum integration tests over the lead composition against a throwaway
//! database: auth refusals, the full happy path over HTTP, the
//! workarea_unresolved 422, idempotent replay, and `/readyz` states — the
//! phase-4 gate list. Requests go through `tower::ServiceExt::oneshot`
//! against the real router; every byte on the wire is the spec's shape
//! (docs/api/openapi.yaml, docs/api/errors.md).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::domain::sha256_digest_string;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{IdGen as _, JournalStore};
use oml_pg::PgStore;
use oml_server_lead::{OsIds, enumerate_tenants, lead_state, pg_ping, startup_fork_check};
use oml_transport_http::DbPing;

const ENROLLMENT: &str = "enr_test_fixture_token";

// ---------------------------------------------------------------------------
// Throwaway database + rig (the oml-pg test pattern)
// ---------------------------------------------------------------------------

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

fn db_url(name: &str) -> String {
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{prefix}/{name}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

struct Rig {
    router: Router,
    store: PgStore,
    signing: SigningKey,
    db_name: String,
}

async fn fresh_rig() -> Rig {
    let db_name = format!(
        "oml_test_http_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst)
    );
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("CREATE DATABASE {db_name}").as_str())
        .await
        .expect("create throwaway database");
    let store = PgStore::connect(&db_url(&db_name)).await.expect("connect");
    store.migrate().await.expect("embedded migrations apply");
    let signing = SigningKey::from_bytes(&OsIds.new_secret());
    let router = lead_state(
        &store,
        signing.clone(),
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    Rig {
        router,
        store,
        signing,
        db_name,
    }
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
    }
}

// ---------------------------------------------------------------------------
// Request helper
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct Call {
    token: Option<String>,
    idem: Option<String>,
    workarea: Option<String>,
    content_type: Option<String>,
    body: Option<Value>,
}

impl Call {
    fn auth(token: &str) -> Self {
        Self {
            token: Some(token.to_owned()),
            ..Self::default()
        }
    }
    fn idem(mut self, key: &str) -> Self {
        self.idem = Some(key.to_owned());
        self
    }
    fn workarea(mut self, id: &str) -> Self {
        self.workarea = Some(id.to_owned());
        self
    }
    fn body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }
    fn content_type(mut self, ct: &str) -> Self {
        self.content_type = Some(ct.to_owned());
        self
    }
}

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    call: Call,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = &call.token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(idem) = &call.idem {
        builder = builder.header("idempotency-key", idem);
    }
    if let Some(workarea) = &call.workarea {
        builder = builder.header("x-oml-workarea", workarea);
    }
    let body = match &call.body {
        None => Body::empty(),
        Some(v) => {
            let ct = call
                .content_type
                .clone()
                .unwrap_or_else(|| "application/json".to_owned());
            builder = builder.header("content-type", ct);
            Body::from(v.to_string())
        }
    };
    let request = builder.body(body).expect("request builds");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("infallible router");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("body collects");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("responses are JSON")
    };
    (status, headers, value)
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("<no code>")
}

fn assert_envelope(body: &Value, code: &str) {
    assert_eq!(error_code(body), code, "envelope: {body}");
    assert!(body["error"]["message"].is_string(), "envelope: {body}");
    assert!(body["error"]["rule"].is_string(), "envelope: {body}");
    assert!(body["error"]["retryable"].is_boolean(), "envelope: {body}");
    assert!(body["request_id"].is_string(), "envelope: {body}");
}

/// The bootstrap ceremony over HTTP; returns (admin token, tenant id, root
/// workarea id).
async fn bootstrap(router: &Router) -> (String, Uuid, Uuid) {
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        Call::default().body(json!({
            "enrollment_token": ENROLLMENT,
            "tenant_name": "test-tenant",
            "admin_principal": "platform/admin",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "bootstrap claim: {body}");
    let token = body["value"].as_str().expect("show-once value").to_owned();
    assert!(token.starts_with("oml_"), "key wire format: {token}");
    assert_eq!(body["role"], "admin");
    let tenant = Uuid::parse_str(body["tenant_id"].as_str().expect("tenant id")).expect("uuid");
    let root = Uuid::parse_str(body["root_workarea_id"].as_str().expect("root id")).expect("uuid");
    (token, tenant, root)
}

/// Issues a key over HTTP under the admin token, its one grant its own role
/// on `wa`; returns the show-once value.
async fn issue_key(router: &Router, admin: &str, role: &str, principal: &str, wa: Uuid) -> String {
    issue_key_with(
        router,
        admin,
        role,
        principal,
        json!([{ "workarea_node_id": wa, "role": role }]),
    )
    .await["value"]
        .as_str()
        .expect("show-once value")
        .to_owned()
}

/// Issues a key over HTTP carrying exactly `grants`; returns the 201 body.
async fn issue_key_with(
    router: &Router,
    admin: &str,
    role: &str,
    principal: &str,
    grants: Value,
) -> Value {
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/keys",
        Call::auth(admin)
            .idem(&format!("issue-{role}-{principal}"))
            .body(json!({ "role": role, "principal": principal, "grants": grants })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "issue {role}: {body}");
    assert_eq!(body["role"], role);
    body
}

/// Creates a workarea under `parent` over HTTP; returns its id.
async fn create_workarea(router: &Router, admin: &str, parent: Uuid, name: &str) -> Uuid {
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/workareas",
        Call::auth(admin)
            .idem(&format!("wa-{name}"))
            .body(json!({ "parent_id": parent, "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Uuid::parse_str(body["id"].as_str().expect("workarea id")).expect("uuid")
}

// ---------------------------------------------------------------------------
// Probes and refusal states
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_readyz_and_fork_refusal() {
    let rig = fresh_rig().await;

    let (status, headers, body) = send(&rig.router, "GET", "/healthz", Call::default()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"status": "ok"}));
    assert!(
        headers.contains_key("x-request-id"),
        "every response carries X-Request-Id"
    );

    let (status, _, body) = send(&rig.router, "GET", "/readyz", Call::default()).await;
    assert_eq!(status, StatusCode::OK, "readyz: {body}");
    assert_eq!(body["checks"]["postgres"], "ok");
    assert_eq!(body["checks"]["fork_check"], "ok");

    // Postgres unreachable: readyz 503 with per-check detail.
    let dead_ping: DbPing = Arc::new(|| Box::pin(async { false }));
    let dead_router =
        lead_state(&rig.store, rig.signing.clone(), dead_ping, true, None).into_router();
    let (status, _, body) = send(&dead_router, "GET", "/readyz", Call::default()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "unavailable");
    assert_eq!(body["checks"]["postgres"], "unreachable");

    // Failed fork check: readyz 503 and writes answer system.unavailable
    // (http-api.md "Health"; ADR-0002 d7).
    let forked_router = lead_state(
        &rig.store,
        rig.signing.clone(),
        pg_ping(rig.store.pool().clone()),
        false,
        None,
    )
    .into_router();
    let (status, _, body) = send(&forked_router, "GET", "/readyz", Call::default()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["checks"]["fork_check"], "failed");
    let (status, headers, body) = send(
        &forked_router,
        "POST",
        "/v1/memories",
        Call::auth("oml_whatever").idem("k").body(json!({
            "verb": "remember", "claim_key": "x", "kind": "memory", "body": "x",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_envelope(&body, "system.unavailable");
    assert_eq!(body["error"]["retryable"], true);
    assert!(
        headers.contains_key("retry-after"),
        "retryable refusals carry Retry-After"
    );

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn auth_and_media_refusals() {
    let rig = fresh_rig().await;
    let id = Uuid::from_u128(1);

    let (status, _, body) = send(
        &rig.router,
        "GET",
        &format!("/v1/memories/{id}"),
        Call::default(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_envelope(&body, "auth.missing");

    let (status, _, body) = send(
        &rig.router,
        "GET",
        &format!("/v1/memories/{id}"),
        Call::auth("oml_not_a_real_key"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_envelope(&body, "auth.invalid");

    // A request body in any other media type is refused before parsing.
    let (status, _, body) = send(
        &rig.router,
        "POST",
        "/v1/memories",
        Call::auth("oml_whatever")
            .idem("k1")
            .content_type("text/plain")
            .body(json!({"verb": "remember"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_envelope(&body, "request.unsupported_media");

    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// The full happy path over HTTP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_happy_path_over_http() {
    let rig = fresh_rig().await;
    let router = &rig.router;

    // Wrong token first: the ceremony refuses without consuming.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        Call::default().body(json!({"enrollment_token": "enr_wrong"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_envelope(&body, "auth.invalid");

    let (admin, _tenant, root) = bootstrap(router).await;

    // The token is single-use: a replay is consumed.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        Call::default().body(json!({"enrollment_token": ENROLLMENT})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_envelope(&body, "auth.invalid");
    assert_eq!(body["error"]["details"]["reason"], "consumed");

    // State-creating POST without Idempotency-Key.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/workareas",
        Call::auth(&admin).body(json!({"parent_id": root, "name": "metals-crew"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_envelope(&body, "request.idempotency_key_required");

    // Create the crew workarea.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/workareas",
        Call::auth(&admin)
            .idem("wa-1")
            .body(json!({"parent_id": root, "name": "metals-crew"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["status"], "development");
    assert_eq!(body["parent_id"].as_str(), Some(root.to_string().as_str()));
    let crew = Uuid::parse_str(body["id"].as_str().expect("workarea id")).expect("uuid");

    // The two-key rule: admin + dreamer-class grant is refused outright.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/keys",
        Call::auth(&admin).idem("bad-key").body(json!({
            "role": "admin",
            "grants": [{"workarea_node_id": crew, "role": "dreamer"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_envelope(&body, "access.role_exclusive");

    let quotes = create_workarea(router, &admin, crew, "quotes").await;
    let writer = issue_key(router, &admin, "writer", "crew/agent-7", crew).await;
    let dreamer = issue_key(router, &admin, "dreamer", "crew/reviewer-3", crew).await;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;

    // R1: a reader key cannot submit.
    let submit_body = json!({
        "verb": "remember",
        "claim_key": "supplier-quote-am-7741",
        "kind": "memory",
        "body": "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery.",
    });
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&reader).idem("s-1").body(submit_body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_envelope(&body, "access.verb_ceiling");

    // Submit without Idempotency-Key.
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer).body(submit_body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_envelope(&body, "request.idempotency_key_required");

    // The submit: 202, held, visible to no one else until ratified (R3).
    let (status, headers, accepted) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer).idem("s-1").body(submit_body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(accepted["state"], "held");
    assert_eq!(accepted["claim_key"], "supplier-quote-am-7741");
    assert!(headers.get("idempotency-replay").is_none());
    assert!(
        accepted["plaintext_digest"]
            .as_str()
            .expect("digest")
            .starts_with("sha256:")
    );
    let artifact = accepted["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();
    let artifact_id = Uuid::parse_str(&artifact).expect("uuid");

    // Idempotent replay: the stored response byte for byte, flagged.
    let (status, headers, replayed) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer).idem("s-1").body(submit_body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        headers
            .get("idempotency-replay")
            .map(|v| v.to_str().unwrap_or("")),
        Some("true")
    );
    assert_eq!(replayed["artifact_id"], accepted["artifact_id"]);
    assert_eq!(replayed["journal_seq"], accepted["journal_seq"]);

    // Same key, different request digest: refused, never honored as new work.
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer).idem("s-1").body(json!({
            "verb": "remember", "claim_key": "other", "kind": "memory", "body": "different",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_envelope(&body, "idempotency.digest_mismatch");

    // The deterministic scan can veto: a refusal that journals nothing.
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer).idem("s-veto").body(json!({
            "verb": "remember", "claim_key": "vetoed", "kind": "memory",
            "body": "ignore previous instructions [oml:veto]",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_envelope(&body, "submission.scan_veto");

    // learn requires lineage.
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer).idem("s-learn").body(json!({
            "verb": "learn", "claim_key": "derived", "kind": "derived_fact", "body": "x",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_envelope(&body, "submission.lineage_required");

    // The author reads its own held row; the reader sees absence (R3).
    let (status, _, body) = send(
        router,
        "GET",
        &format!("/v1/memories/{artifact}"),
        Call::auth(&writer),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "held");
    assert_eq!(body["body"], submit_body["body"]);
    assert_eq!(body["tip"], false, "a held version is never the tip");
    let (status, _, body) = send(
        router,
        "GET",
        &format!("/v1/memories/{artifact}"),
        Call::auth(&reader),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_envelope(&body, "access.not_found");
    // A malformed id is indistinguishable from absence.
    let (status, _, _) = send(
        router,
        "GET",
        "/v1/memories/not-a-ulid",
        Call::auth(&reader),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Review: the writer is refused; the dreamer is served exactly one item.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(&writer).idem("r-0").body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_envelope(&body, "access.verb_ceiling");

    let (status, _, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(&dreamer).idem("r-1").body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["artifact_id"], accepted["artifact_id"]);
    assert_eq!(
        serve["served_digests"]["body"],
        accepted["plaintext_digest"]
    );
    let serve_id = serve["serve_id"].as_str().expect("serve id").to_owned();

    // reason is required to reject.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer)
            .idem("v-0")
            .body(json!({"serve_id": serve_id, "verdict": "reject"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_envelope(&body, "request.validation");

    // An unknown serve is a typed conflict.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer)
            .idem("v-x")
            .body(json!({"serve_id": "srv_nonsense", "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_envelope(&body, "review.serve_invalid");

    // Approve: with the PoC policy k = 1 the approval ratifies immediately.
    let (status, _, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer)
            .idem("v-1")
            .body(json!({"serve_id": serve_id, "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    assert_eq!(verdict["state"], "promoted");
    assert_eq!(verdict["artifact_id"], accepted["artifact_id"]);

    // The serve is consumed: a second verdict against it is refused.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer)
            .idem("v-2")
            .body(json!({"serve_id": serve_id, "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_envelope(&body, "review.serve_invalid");

    // R2 at verdict time: a dreamer whose principal is in the lineage is
    // refused, even against a hand-crafted serve id.
    let author_dreamer = issue_key(router, &admin, "dreamer", "crew/agent-7", crew).await;
    let (status, _, held2) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer).idem("s-2").body(json!({
            "verb": "remember", "claim_key": "second-claim", "kind": "memory",
            "body": "a second held claim",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let held2_id = held2["artifact_id"].as_str().expect("id").replace('-', "");
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&author_dreamer)
            .idem("v-3")
            .body(json!({"serve_id": format!("srv_{held2_id}"), "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_envelope(&body, "review.author_in_lineage");

    // The independent dreamer rejects that second claim (and empties the
    // queue): a rejection is a journalled verdict with a required reason.
    let (status, _, serve2) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(&dreamer).idem("r-held2").body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve2}");
    assert_eq!(serve2["item"]["artifact_id"], held2["artifact_id"]);
    let (status, _, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer).idem("v-held2").body(json!({
            "serve_id": serve2["serve_id"],
            "verdict": "reject",
            "reason": "fixture row, rejected to drain the queue",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    assert_eq!(verdict["state"], "rejected");

    // Nothing in scope waits: 204.
    let (status, _, _) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(&dreamer).idem("r-empty").body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Promoted rows are readable by the reader now.
    let (status, _, body) = send(
        router,
        "GET",
        &format!("/v1/memories/{artifact}"),
        Call::auth(&reader),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "promoted");

    // Recall: reader ceiling, promoted rows only; no steering is promoted,
    // so the segment is empty.
    let (status, _, recall) = send(
        router,
        "POST",
        "/v1/memories/recall",
        // The 'simple' FTS configuration stems nothing (configuration.md
        // search.fts_config), so the query uses exact token forms.
        Call::auth(&reader).body(json!({"query": "Accra Metals", "k": 8})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    assert_eq!(recall["steering"], json!([]));
    let hits = recall["hits"].as_array().expect("hits");
    assert!(
        hits.iter()
            .any(|h| h["memory"]["artifact_id"] == accepted["artifact_id"]),
        "recall finds the promoted row: {recall}"
    );
    assert!(hits.iter().all(|h| h["memory"]["state"] == "promoted"));
    // The score is the store's ts_rank, never a fixed 0.0 (ADR-0025 d8).
    assert!(
        hits.iter()
            .all(|h| h["score"].as_f64().is_some_and(|s| s > 0.0)),
        "{recall}"
    );
    assert!(
        recall["query_digest"]
            .as_str()
            .expect("query digest")
            .starts_with("sha256:")
    );

    // Roles do not nest: the writer key holds no reader grant, so its scope
    // is not readable and recall answers as absence (ADR-0025 d1).
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories/recall",
        Call::auth(&writer).body(json!({"query": "quote"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope(&body, "access.not_found");

    // Events tail: seq ascending from after_seq, filtered to what the
    // reader may see, so gaps are normal; head_seq for lag and
    // next_after_seq to resume from (ADR-0025 d15).
    let (status, _, events) = send(
        router,
        "GET",
        "/v2/events?after_seq=0&limit=100",
        Call::auth(&reader),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{events}");
    let items = events["items"].as_array().expect("items");
    assert!(!items.is_empty());
    let seqs: Vec<u64> = items
        .iter()
        .map(|e| e["seq"].as_u64().expect("seq"))
        .collect();
    assert!(
        seqs.windows(2).all(|w| w[1] > w[0]),
        "seq ascending: {seqs:?}"
    );
    assert!(
        seqs[0] > 1,
        "the genesis is not on the content tail: {seqs:?}"
    );
    assert_eq!(events["next_after_seq"], events["head_seq"], "{events}");
    assert!(
        items.iter().any(|e| e["event_type"] == "lifecycle.promote"),
        "the ratification is journalled"
    );
    // Digest-only payloads: no event payload carries the body plaintext.
    assert!(
        items
            .iter()
            .all(|e| !e["payload"].to_string().contains("Accra Metals quoted")),
        "no body plaintext rides the journal (D1)"
    );

    // Supersede: writer-own-claims only, held like any write.
    let (status, _, body) = send(
        router,
        "POST",
        &format!("/v1/memories/{artifact}/supersede"),
        Call::auth(&reader)
            .idem("sup-0")
            .body(json!({"body": "revised"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_envelope(&body, "access.verb_ceiling");

    // The body is closed: a field the server does not serve is refused.
    let (status, _, body) = send(
        router,
        "POST",
        &format!("/v1/memories/{artifact}/supersede"),
        Call::auth(&writer)
            .idem("sup-x")
            .body(json!({"body": "revised", "claim_key": "elsewhere"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_envelope(&body, "request.validation");

    let supersede_call = Call::auth(&writer).idem("sup-1").body(json!({
        "body": "Supplier Accra Metals revised the quote to 408 USD/t.",
        "reason": "supplier issued a revised quote",
    }));
    let (status, _, successor) = send(
        router,
        "POST",
        &format!("/v1/memories/{artifact}/supersede"),
        supersede_call.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{successor}");
    assert_eq!(successor["state"], "held");
    assert_eq!(successor["claim_key"], "supplier-quote-am-7741");
    assert_eq!(successor["version"], 2);
    assert_eq!(successor["retirement"], false);
    assert_eq!(
        successor["replaces"],
        json!({ "artifact_id": accepted["artifact_id"], "version": 1 })
    );
    assert_eq!(successor["workarea_id"], json!(crew));
    assert_ne!(successor["artifact_id"], accepted["artifact_id"]);

    // A replay answers the stored response, flagged, and writes nothing.
    let (status, headers, replay) = send(
        router,
        "POST",
        &format!("/v1/memories/{artifact}/supersede"),
        supersede_call,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{replay}");
    assert_eq!(replay, successor);
    assert_eq!(
        headers
            .get("idempotency-replay")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );

    // The old tip reads state promoted and tip true until ratification,
    // and stays the reader's hit (ADR-0025 d10).
    let (status, _, body) = send(
        router,
        "GET",
        &format!("/v1/memories/{artifact}"),
        Call::auth(&reader),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "promoted");
    assert_eq!(body["tip"], true);
    let (status, _, recall) = send(
        router,
        "POST",
        "/v1/memories/recall",
        Call::auth(&reader).body(json!({"query": "Accra Metals", "k": 8})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    let hits: Vec<&Value> = recall["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| &h["memory"]["artifact_id"])
        .collect();
    assert_eq!(hits, vec![&accepted["artifact_id"]], "{recall}");

    // The successor rides the ordinary review pipeline to promotion.
    let (status, _, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(&dreamer).idem("r-2").body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["artifact_id"], successor["artifact_id"]);
    let (status, _, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer).idem("v-4").body(json!({
            "serve_id": serve["serve_id"], "verdict": "approve",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    assert_eq!(verdict["state"], "promoted");

    // The ratified successor is the claim's one hit; the version it replaced
    // is never a hit (retrieval.md; ADR-0025 d8), and reads promoted, tip
    // false.
    let (status, _, recall) = send(
        router,
        "POST",
        "/v1/memories/recall",
        Call::auth(&reader).body(json!({"query": "Accra Metals", "k": 8})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    let hits: Vec<&Value> = recall["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| &h["memory"]["artifact_id"])
        .collect();
    assert_eq!(hits, vec![&successor["artifact_id"]], "{recall}");
    let (status, _, body) = send(
        router,
        "GET",
        &format!("/v1/memories/{artifact}"),
        Call::auth(&reader),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "promoted");
    assert_eq!(body["tip"], false);

    // Superseding the replaced version now: not the current tip, and the
    // refusal names the tip the writer can read (ADR-0025 d10, d16).
    let (status, _, body) = send(
        router,
        "POST",
        &format!("/v1/memories/{artifact}/supersede"),
        Call::auth(&writer)
            .idem("sup-2")
            .body(json!({"body": "again"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_envelope(&body, "submission.tip_conflict");
    assert_eq!(
        body["error"]["details"]["current_tip"],
        json!({ "artifact_id": successor["artifact_id"], "version": 2 })
    );

    // X-OML-Workarea: the explicit per-request override (ADR-0022 d4),
    // honored within the key's grants — a child of the granted node is
    // inside them, the root above it is not (ADR-0025 d1).
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer)
            .idem("s-quotes")
            .workarea(&quotes.to_string())
            .body(json!({
                "verb": "remember", "claim_key": "quotes-note", "kind": "memory",
                "body": "a note written into the quotes workarea",
            })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(
        body["workarea_id"].as_str(),
        Some(quotes.to_string().as_str())
    );
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer)
            .idem("s-root")
            .workarea(&root.to_string())
            .body(json!({
                "verb": "remember", "claim_key": "root-note", "kind": "memory",
                "body": "a note written into the root workarea",
            })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope(&body, "access.not_found");

    // A malformed override is refused as validation, not guessed.
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&writer)
            .idem("s-bad-wa")
            .workarea("not-a-node-id")
            .body(json!({
                "verb": "remember", "claim_key": "x", "kind": "memory", "body": "x",
            })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_envelope(&body, "request.validation");

    // After the whole ceremony the stored chain still verifies under the
    // deployment key — what the next boot's fork check will run.
    let tenants = enumerate_tenants(rig.store.pool())
        .await
        .expect("enumerate");
    assert_eq!(tenants.len(), 1);
    startup_fork_check(&rig.store, rig.signing.verifying_key(), &tenants)
        .await
        .expect("the stored chain verifies end to end");
    // A foreign key refuses: the check binds the chain to this deployment.
    let foreign = SigningKey::from_bytes(&OsIds.new_secret());
    assert!(
        startup_fork_check(&rig.store, foreign.verifying_key(), &tenants)
            .await
            .is_err(),
        "a chain signed by another deployment is refused (ADR-0002 d7)"
    );

    let _ = artifact_id;
    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// request.workarea_unresolved — a key with no default and no override
// ---------------------------------------------------------------------------

#[tokio::test]
async fn workarea_unresolved_is_422() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, root) = bootstrap(router).await;

    // A zero-grant key has no default workarea, and never inherits the
    // issuer's (ADR-0025 d2).
    let issued = issue_key_with(router, &admin, "writer", "crew/no-default", json!([])).await;
    assert_eq!(issued["default_workarea_id"], Value::Null, "{issued}");
    assert_eq!(issued["grants"], json!([]), "{issued}");
    let token = issued["value"].as_str().expect("show-once value");

    // No default, no X-OML-Workarea: refused, never guessed for.
    let submit = json!({
        "verb": "remember", "claim_key": "unresolved", "kind": "memory", "body": "x",
    });
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(token).idem("u-1").body(submit.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_envelope(&body, "request.workarea_unresolved");

    // The override resolves a workarea, which no grant of the key covers.
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(token)
            .idem("u-2")
            .workarea(&root.to_string())
            .body(submit),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope(&body, "access.not_found");

    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// Issuance with grants and denials (ADR-0025 d2, d16)
// ---------------------------------------------------------------------------

/// Bootstrap and one crew workarea: (admin token, tenant, crew).
async fn crew_tenant(router: &Router) -> (String, Uuid, Uuid) {
    let (admin, tenant, root) = bootstrap(router).await;
    let crew = create_workarea(router, &admin, root, "metals-crew").await;
    (admin, tenant, crew)
}

async fn head_seq(rig: &Rig, tenant: Uuid) -> u64 {
    rig.store
        .chain_head(tenant)
        .await
        .expect("head")
        .expect("chain")
        .seq
}

#[tokio::test]
async fn the_issuance_response_echoes_stored_grant_ids_and_epochs() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, tenant, crew) = crew_tenant(router).await;

    let issued = issue_key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        json!([
            { "workarea_node_id": crew, "role": "writer" },
            { "workarea_node_id": crew, "role": "reader" },
        ]),
    )
    .await;
    let key_id = Uuid::parse_str(issued["key_id"].as_str().expect("key id")).expect("uuid");
    let stored = rig
        .store
        .key_access(tenant, key_id)
        .await
        .expect("read")
        .expect("the key is stored");
    let echoed: Vec<(String, u64)> = issued["grants"]
        .as_array()
        .expect("grants")
        .iter()
        .map(|g| {
            (
                g["grant_id"].as_str().expect("grant id").to_owned(),
                g["epoch"].as_u64().expect("epoch"),
            )
        })
        .collect();
    let expected: Vec<(String, u64)> = stored
        .grants
        .iter()
        .map(|g| (g.grant.grant_id.to_string(), g.epoch))
        .collect();
    assert_eq!(echoed, expected, "{issued}");
    // key.issued, then one grant.issued per grant, each its own seq.
    assert_eq!(
        expected.iter().map(|(_, e)| *e).collect::<Vec<_>>(),
        vec![stored.issued_seq + 1, stored.issued_seq + 2]
    );
    assert_eq!(issued["epoch"].as_u64(), Some(stored.issued_seq + 2));
    assert_eq!(head_seq(&rig, tenant).await, stored.issued_seq + 2);
    assert_eq!(issued["grants"][0]["role"], "writer");
    assert_eq!(issued["grants"][1]["role"], "reader");
    assert_eq!(issued["grants"][0]["workarea_node_id"], crew.to_string());
    // The default the server chose and the attestation as journalled: none
    // was sent, so the first grant's node and null.
    assert_eq!(issued["default_workarea_id"], crew.to_string());
    assert_eq!(issued["held_by"], Value::Null);
    assert_eq!(issued["denials"], json!([]));

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn a_deny_mask_on_issuance_is_stored_never_dropped() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, tenant, crew) = crew_tenant(router).await;
    let sealed = create_workarea(router, &admin, crew, "sealed").await;

    let issued = issue_key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        json!([
            { "workarea_node_id": crew, "role": "writer", "deny": [{ "subtree_node_id": sealed }] },
            { "workarea_node_id": crew, "role": "reader", "deny": [{ "subtree_node_id": sealed }] },
        ]),
    )
    .await;
    let mask = json!([{ "subtree_node_id": sealed.to_string() }]);
    assert_eq!(issued["grants"][0]["deny"], mask, "{issued}");
    assert_eq!(issued["grants"][1]["deny"], mask, "{issued}");
    let key_id = Uuid::parse_str(issued["key_id"].as_str().expect("key id")).expect("uuid");
    let stored = rig
        .store
        .key_access(tenant, key_id)
        .await
        .expect("read")
        .expect("stored");
    assert!(stored.grants.iter().all(|g| g.grant.deny == vec![sealed]));

    // The mask holds: the key writes on the crew and is told nothing exists
    // under the sealed subtree.
    let token = issued["value"].as_str().expect("value");
    let note = |claim_key: &str| json!({ "verb": "remember", "claim_key": claim_key, "kind": "memory", "body": "x" });
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(token).idem("m-1").body(note("open")),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(token)
            .idem("m-2")
            .workarea(&sealed.to_string())
            .body(note("sealed")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope(&body, "access.not_found");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn an_unknown_issuance_field_is_request_validation() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, tenant, crew) = crew_tenant(router).await;
    let head = head_seq(&rig, tenant).await;

    // Unknown at the top, in a grant, and in a deny entry: refused, never
    // ignored, so no field the issuer meant is silently dropped (ADR-0025
    // d16).
    for (n, body) in [
        json!({ "role": "writer", "scopes": ["all"] }),
        json!({ "role": "writer", "grants": [{ "workarea_node_id": crew, "role": "writer", "expires": "never" }] }),
        json!({ "role": "writer", "grants": [{ "workarea_node_id": crew, "role": "writer", "deny": [{ "node": crew }] }] }),
    ]
    .into_iter()
    .enumerate()
    {
        let (status, _, refused) = send(
            router,
            "POST",
            "/v2/keys",
            Call::auth(&admin).idem(&format!("bad-{n}")).body(body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_envelope(&refused, "request.validation");
    }
    assert_eq!(head_seq(&rig, tenant).await, head, "nothing journalled");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn an_issuance_expiry_is_refused_by_name_while_unbuilt() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, tenant, crew) = crew_tenant(router).await;
    let head = head_seq(&rig, tenant).await;

    // Issued keys do not expire yet, so a requested expiry is refused by
    // name: a key minted without the expiry its issuer set would hold its
    // access longer than meant (ADR-0025 d2, d16).
    let (status, _, refused) = send(
        router,
        "POST",
        "/v2/keys",
        Call::auth(&admin).idem("expiring").body(json!({
            "role": "writer",
            "grants": [{ "workarea_node_id": crew, "role": "writer" }],
            "expiry": "2027-01-01T00:00:00Z",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_envelope(&refused, "request.validation");
    let message = refused["error"]["message"].as_str().expect("message");
    assert!(message.contains("expiry"), "{refused}");
    assert_eq!(head_seq(&rig, tenant).await, head, "nothing journalled");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn access_refusals_carry_their_recovery_details() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let quotes = create_workarea(router, &admin, crew, "quotes").await;

    // An inadmissible grant: the grant's role, the key role's admissible set
    // and the grant's index.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/keys",
        Call::auth(&admin).idem("k-1").body(json!({
            "role": "writer",
            "grants": [
                { "workarea_node_id": crew, "role": "reader" },
                { "workarea_node_id": crew, "role": "dreamer" },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_envelope(&body, "access.issuer_ceiling");
    assert_eq!(
        body["error"]["details"],
        json!({ "role": "dreamer", "admissible": ["reader", "writer"], "grant_index": 1 })
    );
    // A key role above the issuer's: the requested role and the issuable set.
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/keys",
        Call::auth(&admin)
            .idem("k-2")
            .body(json!({ "role": "operator" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_envelope(&body, "access.issuer_ceiling");
    assert_eq!(
        body["error"]["details"],
        json!({ "role": "operator", "admissible": ["reader", "writer", "dreamer", "admin"] })
    );
    // A deny outside its grant, and a default no grant covers.
    for (idem, body) in [
        (
            "k-3",
            json!({ "role": "writer", "grants": [{ "workarea_node_id": quotes, "role": "writer", "deny": [{ "subtree_node_id": crew }] }] }),
        ),
        (
            "k-4",
            json!({ "role": "writer", "grants": [{ "workarea_node_id": quotes, "role": "writer" }], "default_workarea_id": crew }),
        ),
    ] {
        let (status, _, refused) = send(
            router,
            "POST",
            "/v2/keys",
            Call::auth(&admin).idem(idem).body(body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_envelope(&refused, "request.validation");
    }

    // Readable but not writable: the node and the missing role.
    let issued = issue_key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        json!([
            { "workarea_node_id": crew, "role": "reader" },
            { "workarea_node_id": quotes, "role": "writer" },
        ]),
    )
    .await;
    let (status, _, body) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(issued["value"].as_str().expect("value"))
            .idem("s-1")
            .body(json!({ "verb": "remember", "claim_key": "k", "kind": "memory", "body": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_envelope(&body, "access.grant_refused");
    assert_eq!(
        body["error"]["details"],
        json!({ "workarea_id": crew.to_string(), "needed_role": "writer" })
    );

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn a_denial_is_journalled_with_its_own_seq_as_its_epoch_and_takes_the_subtree_away() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, tenant, crew) = crew_tenant(router).await;
    let issued = issue_key_with(
        router,
        &admin,
        "reader",
        "crew/agent-recall",
        json!([{ "workarea_node_id": crew, "role": "reader" }]),
    )
    .await;
    let reader = issued["value"].as_str().expect("value");
    let key_id = issued["key_id"].as_str().expect("key id");
    let recall = || {
        send(
            router,
            "POST",
            "/v1/memories/recall",
            Call::auth(reader).body(json!({ "query": "anything" })),
        )
    };
    let (status, _, body) = recall().await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Only the admin key role journals a denial.
    let path = format!("/v2/keys/{key_id}/denials");
    let (status, _, body) = send(
        router,
        "POST",
        &path,
        Call::auth(reader)
            .idem("d-0")
            .body(json!({ "workarea_node_id": crew })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_envelope(&body, "access.verb_ceiling");
    let (status, _, body) = send(
        router,
        "POST",
        &path,
        Call::auth(&admin).body(json!({ "workarea_node_id": crew })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_envelope(&body, "request.idempotency_key_required");
    let (status, _, body) = send(
        router,
        "POST",
        &format!("/v2/keys/{}/denials", Uuid::from_u128(0xBEEF)),
        Call::auth(&admin)
            .idem("d-1")
            .body(json!({ "workarea_node_id": crew })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope(&body, "access.not_found");

    let (status, _, denial) = send(
        router,
        "POST",
        &path,
        Call::auth(&admin).idem("d-2").body(json!({
            "workarea_node_id": crew,
            "reason": "quarantine the crew during incident 44",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{denial}");
    assert_eq!(denial["workarea_node_id"], crew.to_string());
    assert_eq!(denial["lifted"], false);
    assert!(denial["denial_id"].is_string(), "{denial}");
    assert_eq!(denial["epoch"].as_u64(), Some(head_seq(&rig, tenant).await));

    // Deny-wins: the key's own default scope is no longer readable.
    let (status, _, body) = recall().await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope(&body, "access.not_found");

    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// The events tail: per-family visibility, the subtree filter and the bounded
// scan (http-api.md events tail; ADR-0025 d15)
// ---------------------------------------------------------------------------

/// A key the tail tests read with: its show-once value and its key id.
struct TailKey {
    value: String,
    key_id: String,
}

/// A crew and the keys that tail it: two writer keys holding writer and
/// reader there under one principal, so own is the authoring key and never
/// the principal (roles-and-keys.md#own), a dreamer holding dreamer and
/// reader, and a reader.
struct TailRig {
    rig: Rig,
    admin: String,
    tenant: Uuid,
    root: Uuid,
    crew: Uuid,
    agent: TailKey,
    twin: TailKey,
    dreamer: TailKey,
    reader: TailKey,
}

async fn tail_key(
    router: &Router,
    admin: &str,
    role: &str,
    principal: &str,
    idem: &str,
    grants: Value,
) -> TailKey {
    let (value, key_id) = key_with(router, admin, role, principal, idem, grants).await;
    TailKey { value, key_id }
}

async fn tail_rig() -> TailRig {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, tenant, root) = bootstrap(router).await;
    let crew = create_workarea(router, &admin, root, "metals-crew").await;
    let on_crew = |roles: &[&str]| {
        Value::Array(
            roles
                .iter()
                .map(|role| json!({ "workarea_node_id": crew, "role": role }))
                .collect(),
        )
    };
    let agent = tail_key(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        "issue-agent",
        on_crew(&["writer", "reader"]),
    )
    .await;
    let twin = tail_key(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        "issue-twin",
        on_crew(&["writer", "reader"]),
    )
    .await;
    let dreamer = tail_key(
        router,
        &admin,
        "dreamer",
        "crew/reviewer-3",
        "issue-dreamer",
        on_crew(&["dreamer", "reader"]),
    )
    .await;
    let reader = tail_key(
        router,
        &admin,
        "reader",
        "crew/agent-recall",
        "issue-reader",
        on_crew(&["reader"]),
    )
    .await;
    TailRig {
        rig,
        admin,
        tenant,
        root,
        crew,
        agent,
        twin,
        dreamer,
        reader,
    }
}

/// Every item of `key`'s events tail from the beginning, at `scope` when one
/// is named, following `next_after_seq` until it reaches `head_seq`.
async fn tailed(router: &Router, key: &str, scope: Option<Uuid>) -> Vec<Value> {
    let mut events = Vec::new();
    let mut after_seq = 0;
    for _ in 0..100 {
        let mut path = format!("/v2/events?after_seq={after_seq}&limit=5");
        if let Some(scope) = scope {
            path.push_str(&format!("&workarea_id={scope}"));
        }
        let (status, _, page) = send(router, "GET", &path, Call::auth(key)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        events.extend(page["items"].as_array().expect("items").iter().cloned());
        let next = page["next_after_seq"].as_u64().expect("next_after_seq");
        if Some(next) == page["head_seq"].as_u64() {
            return events;
        }
        assert!(
            next > after_seq,
            "a page short of head_seq advances: {page}"
        );
        after_seq = next;
    }
    panic!("the tail never reached head_seq");
}

/// The claim versions the `event_type` items of a tail name, in order: the
/// new version of a `lifecycle.supersede`, the payload's `artifact_id`
/// otherwise.
fn tail_subjects(events: &[Value], event_type: &str) -> Vec<String> {
    events
        .iter()
        .filter(|e| e["event_type"] == event_type)
        .map(|e| {
            let payload = &e["payload"];
            payload
                .get("new_artifact_id")
                .or_else(|| payload.get("artifact_id"))
                .and_then(Value::as_str)
                .expect("a claim subject")
                .to_owned()
        })
        .collect()
}

/// The events of a tail outside the submission family: what two keys with
/// equal access see alike.
fn shared_events(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter(|e| {
            !e["event_type"]
                .as_str()
                .expect("event_type")
                .starts_with("submission.")
        })
        .cloned()
        .collect()
}

#[tokio::test]
async fn the_tail_never_shows_another_keys_submission() {
    let t = tail_rig().await;
    let router = &t.rig.router;
    let mine = remember(
        router,
        &t.agent.value,
        "am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    let theirs = remember(
        router,
        &t.twin.value,
        "km-2210",
        "Kumasi Metals quoted 405 USD/t.",
        None,
    )
    .await;

    // The two writer keys share a principal: only the authoring key makes a
    // submission its own (roles-and-keys.md#own; ADR-0025 d3, d15).
    let agent = tailed(router, &t.agent.value, None).await;
    assert_eq!(tail_subjects(&agent, "submission.remember"), [mine]);
    let twin = tailed(router, &t.twin.value, None).await;
    assert_eq!(tail_subjects(&twin, "submission.remember"), [theirs]);
    for key in [&t.dreamer.value, &t.reader.value] {
        let events = tailed(router, key, None).await;
        assert!(
            events.iter().all(|e| !e["event_type"]
                .as_str()
                .expect("event_type")
                .starts_with("submission.")),
            "a held submission reaches no other key: {events:?}"
        );
    }

    drop_db(&t.rig.db_name).await;
}

#[tokio::test]
async fn an_author_sees_its_own_submission_in_the_tail() {
    let t = tail_rig().await;
    let router = &t.rig.router;
    let text = "Accra Metals quoted 412 USD/t for Q1-2027 delivery.";
    let mine = remember(router, &t.agent.value, "am-7741", text, None).await;

    let events = tailed(router, &t.agent.value, None).await;
    let own: Vec<&Value> = events
        .iter()
        .filter(|e| e["event_type"] == "submission.remember")
        .collect();
    assert_eq!(own.len(), 1, "{events:?}");
    let event = own[0];
    assert_eq!(event["payload"]["artifact_id"], mine);
    assert_eq!(event["payload"]["workarea_id"], t.crew.to_string());
    // The key is the actor, so the tail names it (http-api.md events tail).
    assert_eq!(event["api_key_id"], t.agent.key_id, "{event}");
    assert_eq!(event["principal"], "crew/agent-7", "{event}");
    assert!(
        !event["payload"].to_string().contains("Accra Metals quoted"),
        "no body plaintext rides the tail (D1): {event}"
    );

    drop_db(&t.rig.db_name).await;
}

#[tokio::test]
async fn lifecycle_promote_is_visible_to_readers_of_the_workarea() {
    let t = tail_rig().await;
    let router = &t.rig.router;
    let ledger = create_workarea(router, &t.admin, t.root, "ledger").await;
    let outsider = tail_key(
        router,
        &t.admin,
        "reader",
        "ledger/agent-recall",
        "issue-outsider",
        json!([{ "workarea_node_id": ledger, "role": "reader" }]),
    )
    .await;
    let mine = remember(
        router,
        &t.agent.value,
        "am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    assert_eq!(review(router, &t.dreamer.value, "approve").await, mine);

    let events = tailed(router, &t.reader.value, None).await;
    assert_eq!(tail_subjects(&events, "lifecycle.promote"), [mine]);
    assert!(
        tail_subjects(&events, "submission.remember").is_empty(),
        "{events:?}"
    );
    let promote = events
        .iter()
        .find(|e| e["event_type"] == "lifecycle.promote")
        .expect("the promote");
    assert_eq!(promote["payload"]["workarea_id"], t.crew.to_string());
    assert_eq!(promote["payload"]["version"], 1);
    // The reader is outside the actor's own key set: the tail names no
    // identity (ADR-0025 d15).
    assert!(promote["principal"].is_null(), "{promote}");
    assert!(promote["api_key_id"].is_null(), "{promote}");

    // A reader of another workarea sees nothing of it.
    let elsewhere = tailed(router, &outsider.value, None).await;
    assert!(
        tail_subjects(&elsewhere, "lifecycle.promote").is_empty(),
        "{elsewhere:?}"
    );

    drop_db(&t.rig.db_name).await;
}

#[tokio::test]
async fn the_tail_never_shows_review_or_key_administration_events() {
    let t = tail_rig().await;
    let router = &t.rig.router;
    remember(
        router,
        &t.agent.value,
        "am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    review(router, &t.dreamer.value, "approve").await;
    remember(
        router,
        &t.agent.value,
        "am-7742",
        "Accra Metals quoted 415 USD/t.",
        None,
    )
    .await;
    review(router, &t.dreamer.value, "reject").await;
    let (status, _, denial) = send(
        router,
        "POST",
        &format!("/v2/keys/{}/denials", t.twin.key_id),
        Call::auth(&t.admin)
            .idem("deny-twin")
            .body(json!({ "workarea_node_id": t.crew })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{denial}");

    // The journal holds them; the content tail omits them for every key.
    let journalled: Vec<String> = t
        .rig
        .store
        .events_range(t.tenant, 1, 1000)
        .await
        .expect("journal")
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect();
    for event_type in [
        "tenant.genesis",
        "key.issued",
        "grant.issued",
        "key.denied",
        "review.approve",
        "review.reject",
    ] {
        assert!(
            journalled.iter().any(|j| j == event_type),
            "{event_type}: {journalled:?}"
        );
    }
    // The dreamer recorded both verdicts and sees neither (ADR-0025 d15).
    for key in [&t.agent.value, &t.dreamer.value, &t.reader.value] {
        for event in tailed(router, key, None).await {
            let event_type = event["event_type"].as_str().expect("event_type");
            assert!(
                ["submission.", "lifecycle.", "workarea."]
                    .iter()
                    .any(|family| event_type.starts_with(family)),
                "{event_type} is not on the content tail: {event}"
            );
        }
    }

    drop_db(&t.rig.db_name).await;
}

#[tokio::test]
async fn two_keys_with_equal_access_tail_the_same_events_in_the_same_order() {
    let t = tail_rig().await;
    let router = &t.rig.router;
    let first = remember(
        router,
        &t.agent.value,
        "am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    review(router, &t.dreamer.value, "approve").await;
    let theirs = remember(
        router,
        &t.twin.value,
        "km-2210",
        "Kumasi Metals quoted 405 USD/t.",
        None,
    )
    .await;
    review(router, &t.dreamer.value, "approve").await;
    let rejected = remember(
        router,
        &t.agent.value,
        "am-7742",
        "Accra Metals quoted 415 USD/t.",
        None,
    )
    .await;
    review(router, &t.dreamer.value, "reject").await;

    let agent = tailed(router, &t.agent.value, None).await;
    let twin = tailed(router, &t.twin.value, None).await;
    let reader = tailed(router, &t.reader.value, None).await;
    assert_eq!(shared_events(&agent), shared_events(&twin));
    assert_eq!(
        shared_events(&reader),
        shared_events(&agent),
        "reader access on the crew is the access that matters"
    );
    assert_eq!(
        tail_subjects(&agent, "lifecycle.promote"),
        [first.clone(), theirs.clone()]
    );
    // Each additionally sees its own submissions, and only those.
    assert_eq!(
        tail_subjects(&agent, "submission.remember"),
        [first, rejected]
    );
    assert_eq!(tail_subjects(&twin, "submission.remember"), [theirs]);

    drop_db(&t.rig.db_name).await;
}

#[tokio::test]
async fn a_filtered_page_advances_next_after_seq() {
    let t = tail_rig().await;
    let router = &t.rig.router;
    let start = head_seq(&t.rig, t.tenant).await;
    // Six events no tail shows: three keys, a key.issued and a
    // grant.issued apiece.
    for n in 0..3 {
        tail_key(
            router,
            &t.admin,
            "reader",
            &format!("crew/extra-{n}"),
            &format!("issue-extra-{n}"),
            json!([{ "workarea_node_id": t.crew, "role": "reader" }]),
        )
        .await;
    }
    let head = head_seq(&t.rig, t.tenant).await;
    assert_eq!(head, start + 6);
    let page = async |after_seq: u64| {
        send(
            router,
            "GET",
            &format!("/v2/events?after_seq={after_seq}&limit=4"),
            Call::auth(&t.reader.value),
        )
        .await
    };

    // One call scans at most limit raw events and resumes after the last.
    let (status, _, first) = page(start).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["items"], json!([]), "{first}");
    assert_eq!(first["next_after_seq"].as_u64(), Some(start + 4), "{first}");
    assert_eq!(first["head_seq"].as_u64(), Some(head), "{first}");
    let (status, _, second) = page(start + 4).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["items"], json!([]), "{second}");
    assert_eq!(second["next_after_seq"].as_u64(), Some(head), "{second}");
    // No raw event follows the head: the resume point stays where it is.
    let (status, _, idle) = page(head).await;
    assert_eq!(status, StatusCode::OK, "{idle}");
    assert_eq!(idle["items"], json!([]), "{idle}");
    assert_eq!(idle["next_after_seq"].as_u64(), Some(head), "{idle}");

    drop_db(&t.rig.db_name).await;
}

#[tokio::test]
async fn an_after_seq_beyond_head_is_request_validation() {
    let t = tail_rig().await;
    let router = &t.rig.router;
    let head = head_seq(&t.rig, t.tenant).await;
    let (status, _, body) = send(
        router,
        "GET",
        &format!("/v2/events?after_seq={}", head + 1),
        Call::auth(&t.reader.value),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_envelope(&body, "request.validation");
    let (status, _, body) = send(
        router,
        "GET",
        &format!("/v2/events?after_seq={head}"),
        Call::auth(&t.reader.value),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["next_after_seq"].as_u64(), Some(head), "{body}");

    drop_db(&t.rig.db_name).await;
}

#[tokio::test]
async fn a_tail_scope_outside_read_access_is_not_found() {
    let t = tail_rig().await;
    let router = &t.rig.router;
    let quotes = create_workarea(router, &t.admin, t.crew, "quotes").await;
    let ledger = create_workarea(router, &t.admin, t.root, "ledger").await;
    for scope in [
        ledger.to_string(),
        t.root.to_string(),
        Uuid::from_u128(0xBEEF).to_string(),
        "not-a-node".to_owned(),
    ] {
        let (status, _, body) = send(
            router,
            "GET",
            &format!("/v2/events?workarea_id={scope}"),
            Call::auth(&t.reader.value),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{scope}: {body}");
        assert_envelope(&body, "access.not_found");
    }
    // An omitted scope is the key's default, and a key with none is
    // refused, never guessed for.
    let unscoped = tail_key(
        router,
        &t.admin,
        "reader",
        "crew/unscoped",
        "issue-unscoped",
        json!([]),
    )
    .await;
    let (status, _, body) = send(router, "GET", "/v2/events", Call::auth(&unscoped.value)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_envelope(&body, "request.workarea_unresolved");

    // Within read access the scope is a subtree filter.
    let in_crew = remember(
        router,
        &t.agent.value,
        "am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    review(router, &t.dreamer.value, "approve").await;
    let in_quotes = remember(
        router,
        &t.agent.value,
        "am-7742",
        "Accra Metals quoted 415 USD/t.",
        Some(quotes),
    )
    .await;
    review(router, &t.dreamer.value, "approve").await;
    let scoped = tailed(router, &t.reader.value, Some(quotes)).await;
    assert_eq!(
        tail_subjects(&scoped, "lifecycle.promote"),
        std::slice::from_ref(&in_quotes)
    );
    let created: Vec<&Value> = scoped
        .iter()
        .filter(|e| e["event_type"] == "workarea.create")
        .map(|e| &e["payload"]["workarea_id"])
        .collect();
    assert_eq!(created, [&json!(quotes.to_string())]);
    let whole = tailed(router, &t.reader.value, None).await;
    assert_eq!(
        tail_subjects(&whole, "lifecycle.promote"),
        [in_crew, in_quotes]
    );
    let created: Vec<&Value> = whole
        .iter()
        .filter(|e| e["event_type"] == "workarea.create")
        .map(|e| &e["payload"]["workarea_id"])
        .collect();
    assert_eq!(
        created,
        [&json!(t.crew.to_string()), &json!(quotes.to_string())]
    );

    drop_db(&t.rig.db_name).await;
}

// ---------------------------------------------------------------------------
// Recall: the steering segment, the hit order and the strict body (ADR-0025
// d8, d16)
// ---------------------------------------------------------------------------

/// Submits a first version into `workarea` as `writer` and has `dreamer`
/// ratify it over the review ring; returns its artifact id.
async fn promote_over_http(
    router: &Router,
    writer: &str,
    dreamer: &str,
    workarea: Uuid,
    (claim_key, kind, body): (&str, &str, &str),
) -> Value {
    let (status, _, accepted) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(writer)
            .idem(&format!("s-{claim_key}"))
            .workarea(&workarea.to_string())
            .body(json!({
                "verb": "remember", "claim_key": claim_key, "kind": kind, "body": body,
            })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    let (status, _, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(dreamer)
            .idem(&format!("r-{claim_key}"))
            .body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["artifact_id"], accepted["artifact_id"]);
    let (status, _, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(dreamer)
            .idem(&format!("v-{claim_key}"))
            .body(json!({"serve_id": serve["serve_id"], "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    accepted["artifact_id"].clone()
}

/// One recall over HTTP, at `scope` when one is named.
async fn recall_over_http(
    router: &Router,
    token: &str,
    scope: Option<Uuid>,
    body: Value,
) -> (StatusCode, Value) {
    let mut call = Call::auth(token).body(body);
    if let Some(scope) = scope {
        call = call.workarea(&scope.to_string());
    }
    let (status, _, value) = send(router, "POST", "/v1/memories/recall", call).await;
    (status, value)
}

/// The artifact ids of a recall's steering items, in order.
fn steering_ids(recall: &Value) -> Vec<Value> {
    recall["steering"]
        .as_array()
        .expect("steering")
        .iter()
        .map(|m| m["artifact_id"].clone())
        .collect()
}

/// The artifact ids of a recall's hits, in order.
fn hit_ids(recall: &Value) -> Vec<Value> {
    recall["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| h["memory"]["artifact_id"].clone())
        .collect()
}

/// A crew, its child `quotes`, and a writer and a dreamer on the crew:
/// (rig, admin, crew, quotes, writer, dreamer).
async fn steering_rig() -> (Rig, String, Uuid, Uuid, String, String) {
    let rig = fresh_rig().await;
    let (admin, _tenant, crew) = crew_tenant(&rig.router).await;
    let quotes = create_workarea(&rig.router, &admin, crew, "quotes").await;
    let writer = issue_key(&rig.router, &admin, "writer", "crew/agent-7", crew).await;
    let dreamer = issue_key(&rig.router, &admin, "dreamer", "crew/reviewer-3", crew).await;
    (rig, admin, crew, quotes, writer, dreamer)
}

#[tokio::test]
async fn a_promoted_steering_row_precedes_hits_unranked() {
    let (rig, admin, crew, _quotes, writer, dreamer) = steering_rig().await;
    let router = &rig.router;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;
    let promote = |row| promote_over_http(router, &writer, &dreamer, crew, row);

    // One steering row misses the query and the other matches it best:
    // steering applies by scope and is ordered by promotion, never by the
    // query (retrieval.md, the recall-ordering contract).
    let quiet = promote((
        "validity",
        "steering",
        "Treat any supplier quote without an explicit validity date as expired.",
    ))
    .await;
    let loud = promote((
        "units",
        "steering",
        "Quote every aluminium price in USD/t; an aluminium price without units is no aluminium price.",
    ))
    .await;
    let accra = promote((
        "accra",
        "memory",
        "Accra Metals aluminium price is 412 USD/t.",
    ))
    .await;
    let tema = promote((
        "tema",
        "memory",
        "Tema Alloys aluminium price is 409 USD/t, the lowest aluminium price this quarter.",
    ))
    .await;

    let (status, recall) =
        recall_over_http(router, &reader, None, json!({"query": "aluminium price"})).await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    assert_eq!(steering_ids(&recall), vec![quiet, loud], "{recall}");
    for item in recall["steering"].as_array().expect("steering") {
        assert_eq!(item["kind"], "steering", "{item}");
        assert!(item.get("score").is_none(), "steering is unranked: {item}");
        assert!(item["body"].is_string(), "{item}");
    }
    let mut served = hit_ids(&recall);
    served.sort_by_key(ToString::to_string);
    let mut expected = vec![accra, tema];
    expected.sort_by_key(ToString::to_string);
    assert_eq!(served, expected, "steering is never a hit: {recall}");
    let ranked: Vec<(f64, Uuid)> = recall["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| {
            let id = h["memory"]["artifact_id"].as_str().expect("id");
            (
                h["score"].as_f64().expect("a numeric score"),
                Uuid::parse_str(id).expect("uuid"),
            )
        })
        .collect();
    assert!(
        ranked.iter().all(|(score, _)| *score > 0.0),
        "the score is the store's ts_rank, never a fixed 0.0: {recall}"
    );
    assert!(
        ranked
            .windows(2)
            .all(|w| w[0].0 > w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1)),
        "score descending, then artifact_id ascending: {recall}"
    );

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn ancestor_steering_applies_to_a_child_scope() {
    let (rig, admin, crew, quotes, writer, dreamer) = steering_rig().await;
    let router = &rig.router;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;

    // The child's row is promoted first: depth from the root orders the
    // segment before promotion time does.
    let local = promote_over_http(
        router,
        &writer,
        &dreamer,
        quotes,
        ("units", "steering", "Quotes are in USD/t."),
    )
    .await;
    let standing = promote_over_http(
        router,
        &writer,
        &dreamer,
        crew,
        ("validity", "steering", "Treat undated quotes as expired."),
    )
    .await;

    let (status, recall) =
        recall_over_http(router, &reader, Some(quotes), json!({"query": "anything"})).await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    assert_eq!(steering_ids(&recall), vec![standing, local], "{recall}");
    assert_eq!(recall["hits"], json!([]));

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn child_steering_does_not_apply_to_the_parent() {
    let (rig, admin, crew, quotes, writer, dreamer) = steering_rig().await;
    let router = &rig.router;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;
    promote_over_http(
        router,
        &writer,
        &dreamer,
        quotes,
        ("units", "steering", "Quote the aluminium price in USD/t."),
    )
    .await;
    let quote = promote_over_http(
        router,
        &writer,
        &dreamer,
        quotes,
        (
            "accra",
            "memory",
            "Accra Metals aluminium price is 412 USD/t.",
        ),
    )
    .await;

    // The child is searched for hits at the parent; its steering belongs to
    // another crew and does not apply there.
    let (status, recall) = recall_over_http(
        router,
        &reader,
        Some(crew),
        json!({"query": "aluminium price"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    assert_eq!(recall["steering"], json!([]), "{recall}");
    assert_eq!(hit_ids(&recall), vec![quote], "the subtree is searched");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn steering_on_an_unreadable_ancestor_is_not_served() {
    let (rig, admin, crew, quotes, writer, dreamer) = steering_rig().await;
    let router = &rig.router;
    let standing = promote_over_http(
        router,
        &writer,
        &dreamer,
        crew,
        ("validity", "steering", "Treat undated quotes as expired."),
    )
    .await;

    // Steering is bounded by read access: a key that reads only the child
    // is served none of the parent's, and a key that reads the parent is.
    let narrow = issue_key(router, &admin, "reader", "crew/agent-narrow", quotes).await;
    let (status, recall) =
        recall_over_http(router, &narrow, None, json!({"query": "anything"})).await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    assert_eq!(recall["steering"], json!([]), "{recall}");
    let wide = issue_key(router, &admin, "reader", "crew/agent-wide", crew).await;
    let (status, recall) =
        recall_over_http(router, &wide, Some(quotes), json!({"query": "anything"})).await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    assert_eq!(steering_ids(&recall), vec![standing], "{recall}");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn kinds_filters_hits_only() {
    let (rig, admin, crew, _quotes, writer, dreamer) = steering_rig().await;
    let router = &rig.router;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;
    let promote = |row| promote_over_http(router, &writer, &dreamer, crew, row);
    let steering = promote((
        "validity",
        "steering",
        "Check the aluminium price validity date first.",
    ))
    .await;
    let memory = promote((
        "accra",
        "memory",
        "Accra Metals aluminium price is 412 USD/t.",
    ))
    .await;
    let derived = promote((
        "anchor",
        "derived_fact",
        "Accra Metals sets the aluminium price anchor.",
    ))
    .await;

    for (kinds, hits) in [
        (json!(["derived_fact"]), vec![derived.clone()]),
        (json!(["memory"]), vec![memory.clone()]),
    ] {
        let (status, recall) = recall_over_http(
            router,
            &reader,
            None,
            json!({"query": "aluminium price", "kinds": kinds}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{recall}");
        assert_eq!(steering_ids(&recall), vec![steering.clone()], "{recall}");
        assert_eq!(hit_ids(&recall), hits, "{recall}");
    }

    // An empty filter, a steering filter and a vocabulary kind are refused,
    // never read as no filter (openapi.yaml RecallRequest.kinds).
    for kinds in [
        json!([]),
        json!(["steering"]),
        json!(["term"]),
        json!(["bogus"]),
    ] {
        let (status, refused) = recall_over_http(
            router,
            &reader,
            None,
            json!({"query": "aluminium price", "kinds": kinds.clone()}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{kinds}: {refused}");
        assert_envelope(&refused, "request.validation");
    }

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn an_unknown_recall_field_is_request_validation() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;

    // A field the body does not declare is refused, never dropped (ADR-0025
    // d16).
    let (status, refused) = recall_over_http(
        router,
        &reader,
        None,
        json!({"query": "aluminium price", "rerank": "by_recency"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_envelope(&refused, "request.validation");
    let message = refused["error"]["message"].as_str().expect("message");
    assert!(message.contains("rerank"), "{refused}");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn terms_on_recall_is_refused_by_name_until_built() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;

    // The vocabulary filter is unbuilt, so it is refused by name rather than
    // answered unfiltered (retrieval.md; ADR-0025 d16).
    for (field, body) in [
        (
            "terms",
            json!({"query": "aluminium price", "terms": ["proc:alloy"]}),
        ),
        ("terms", json!({"query": "aluminium price", "terms": []})),
        (
            "expand_terms",
            json!({"query": "aluminium price", "expand_terms": "descendants"}),
        ),
    ] {
        let (status, refused) = recall_over_http(router, &reader, None, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_envelope(&refused, "request.validation");
        let message = refused["error"]["message"].as_str().expect("message");
        assert!(message.contains(field), "{field}: {refused}");
    }

    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// The claim item's real fields (ADR-0025 d6, d10, d12)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn responses_carry_real_version_kind_tokens_and_workarea_path() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let quotes = create_workarea(router, &admin, crew, "quotes").await;
    let agent = issue_key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        json!([
            { "workarea_node_id": crew, "role": "writer" },
            { "workarea_node_id": crew, "role": "reader" },
        ]),
    )
    .await["value"]
        .as_str()
        .expect("show-once value")
        .to_owned();
    let dreamer = issue_key(router, &admin, "dreamer", "crew/reviewer-3", crew).await;
    // Sent decomposed; counted, digested and stored as its NFC form.
    let decomposed = "Accra Metals quoted 412 USD/t at the cafe\u{301}.";
    let composed = "Accra Metals quoted 412 USD/t at the caf\u{e9}.";
    let tokens = |text: &str| text.len().div_ceil(4);
    let get = |token: String, id: Value| async move {
        let id = id.as_str().expect("artifact id").to_owned();
        let (status, _, body) = send(
            router,
            "GET",
            &format!("/v1/memories/{id}"),
            Call::auth(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    };
    // The path walks parent names from the tenant's `universe` root
    // (ADR-0025 d14; workareas.md, one universe per tenant).
    let assert_path = |item: &Value| {
        assert_eq!(
            item["workarea_path"], "universe.metals-crew.quotes",
            "{item}"
        );
    };

    let (status, _, accepted) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(&agent)
            .idem("s-1")
            .workarea(&quotes.to_string())
            .body(json!({
                "verb": "remember", "claim_key": "quote-am-7741", "kind": "derived_fact",
                "body": decomposed,
            })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(accepted["version"], 1);
    assert_eq!(accepted["retirement"], false);
    assert_eq!(accepted["replaces"], Value::Null);
    assert_eq!(accepted["workarea_id"], json!(quotes));
    let first = accepted["artifact_id"].clone();

    let held = get(agent.clone(), first.clone()).await;
    assert_eq!(held["version"], 1);
    assert_eq!(
        held["kind"], "derived_fact",
        "the kind sent, never 'memory'"
    );
    assert_eq!(held["state"], "held");
    assert_eq!(held["tip"], false);
    assert_eq!(
        held["tokens"],
        tokens(composed),
        "budget tokens of the NFC body"
    );
    assert_eq!(held["body"], composed);
    assert_eq!(held["retirement"], false);
    assert_eq!(held["replaces"], Value::Null);
    assert_eq!(
        held["held_expires_at"],
        Value::Null,
        "held-item expiry is unbuilt"
    );
    assert_eq!(held["promoted_at"], Value::Null);
    assert_eq!(held["workarea_id"], json!(quotes));
    assert_path(&held);

    let (status, _, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(&dreamer).idem("r-1").body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["version"], 1);
    assert_eq!(serve["item"]["kind"], "derived_fact");
    assert_eq!(serve["item"]["tokens"], tokens(composed));
    let (status, _, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer)
            .idem("v-1")
            .body(json!({"serve_id": serve["serve_id"], "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    assert_eq!(verdict["version"], 1);

    let promoted = get(agent.clone(), first.clone()).await;
    assert_eq!(promoted["state"], "promoted");
    assert_eq!(
        promoted["tip"], true,
        "the ratified first version is the tip"
    );
    assert!(promoted["promoted_at"].is_string(), "{promoted}");

    let (status, _, recall) = send(
        router,
        "POST",
        "/v1/memories/recall",
        Call::auth(&agent).body(json!({"query": "Accra Metals"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    let hit = &recall["hits"][0]["memory"];
    assert_eq!(hit["artifact_id"], first);
    assert_eq!(hit["version"], 1);
    assert_eq!(hit["kind"], "derived_fact");
    assert_eq!(hit["tokens"], tokens(composed));
    assert_eq!(hit["tip"], true);
    assert_path(hit);

    let revised = "Accra Metals revised the quote to 408 USD/t.";
    let (status, _, successor) = send(
        router,
        "POST",
        &format!("/v1/memories/{}/supersede", first.as_str().expect("id")),
        Call::auth(&agent)
            .idem("sup-1")
            .body(json!({"body": revised})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{successor}");
    assert_eq!(successor["version"], 2);
    let next = get(agent.clone(), successor["artifact_id"].clone()).await;
    assert_eq!(next["version"], 2, "1 + the claim's greatest version");
    assert_eq!(next["kind"], "derived_fact", "the target's kind");
    assert_eq!(next["claim_key"], "quote-am-7741");
    assert_eq!(
        next["replaces"],
        json!({ "artifact_id": first, "version": 1 })
    );
    assert_eq!(next["tokens"], tokens(revised));
    assert_eq!(next["tip"], false);
    assert_path(&next);
    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn the_body_limit_counts_the_nfc_text() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let writer = issue_key(router, &admin, "writer", "crew/agent-7", crew).await;
    let submit = |idem: &str, claim_key: &str, body: String| {
        let call = Call::auth(&writer).idem(idem).body(json!({
            "verb": "remember", "claim_key": claim_key, "kind": "memory", "body": body,
        }));
        async move { send(router, "POST", "/v1/memories", call).await }
    };

    // 400 000 decomposed é: 1.2 MB as sent, 0.8 MB in NFC, under the
    // shipped 1 MiB submission.max_body_size (http-api.md limits).
    let (status, _, body) = submit("s-1", "decomposed", "e\u{301}".repeat(400_000)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");

    // One byte over 1 MiB in NFC is refused before anything is stored.
    let (status, _, body) = submit("s-2", "over", "a".repeat(1_048_577)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_envelope(&body, "request.payload_too_large");
    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// The workarea read (ADR-0025 d14)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn crew_given_at_create_is_persisted_and_returned() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, root) = bootstrap(router).await;
    let crew_record = json!({
        "name": "metals-crew",
        "charter_uri": "https://crews.example.com/metals/charter.md",
        "external_manager": "crewd.example.com",
    });
    let (status, _, created) = send(
        router,
        "POST",
        "/v2/workareas",
        Call::auth(&admin).idem("wa-crew").body(json!({
            "parent_id": root,
            "name": "metals-crew",
            "crew": crew_record,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["crew"], crew_record);
    // The create serves the same derived path the read serves.
    assert_eq!(created["path"], "universe.metals-crew");
    let crew = Uuid::parse_str(created["id"].as_str().expect("id")).expect("uuid");
    let quarter = create_workarea(router, &admin, crew, "q1-2027").await;
    let agent = issue_key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        json!([
            { "workarea_node_id": crew, "role": "writer" },
            { "workarea_node_id": crew, "role": "reader" },
        ]),
    )
    .await;
    let agent = agent["value"].as_str().expect("value");

    // One call carries the node and the calling key's view of it; the crew
    // is the stored record, read back, not the request echoed.
    let (status, _, view) = send(
        router,
        "GET",
        &format!("/v2/workareas/{crew}"),
        Call::auth(agent),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        view,
        json!({
            "id": crew,
            "parent_id": root,
            "path": "universe.metals-crew",
            "status": "development",
            "crew": crew_record,
            "children": [{ "id": quarter, "path_leaf": "q1-2027", "status": "development" }],
            "edges": [],
            "my_effective_verbs": [
                "recall", "read", "graph_read", "events_tail",
                "remember", "learn", "attach", "supersede",
            ],
            "permission_epoch": view["permission_epoch"],
        })
    );
    assert!(view["permission_epoch"].is_u64(), "{view}");

    // A node created without a crew has none, stored and served absent.
    let (status, _, leaf) = send(
        router,
        "GET",
        &format!("/v2/workareas/{quarter}"),
        Call::auth(agent),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{leaf}");
    assert_eq!(leaf["path"], "universe.metals-crew.q1-2027");
    assert!(leaf.get("crew").is_none(), "{leaf}");
    assert_eq!(leaf["children"], json!([]));

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn an_unreadable_workarea_is_not_found() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let writer = issue_key(router, &admin, "writer", "crew/agent-7", crew).await;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;

    let get = |token: &str, id: String| {
        let call = Call::auth(token);
        async move { send(router, "GET", &format!("/v2/workareas/{id}"), call).await }
    };
    let (status, _, body) = get(&reader, crew.to_string()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // A writer-only key, an unknown node and an id that names no node all
    // answer as absence.
    for (token, id) in [
        (&writer, crew.to_string()),
        (&reader, Uuid::from_u128(0xDEAD).to_string()),
        (&reader, "wa_not_a_node".to_owned()),
    ] {
        let (status, _, body) = get(token, id).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_envelope(&body, "access.not_found");
    }
    let (status, _, body) = send(
        router,
        "GET",
        &format!("/v2/workareas/{crew}"),
        Call::default(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_envelope(&body, "auth.missing");

    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// The own-work and scoped listings (ADR-0025 d3, d13)
// ---------------------------------------------------------------------------

/// Submits a memory under `token`, into `workarea` when one is named, else
/// the key's default; returns its artifact id.
async fn remember(
    router: &Router,
    token: &str,
    claim_key: &str,
    text: &str,
    workarea: Option<Uuid>,
) -> String {
    let mut call = Call::auth(token)
        .idem(&format!("remember-{claim_key}"))
        .body(json!({
            "verb": "remember", "claim_key": claim_key, "kind": "memory", "body": text,
        }));
    if let Some(workarea) = workarea {
        call = call.workarea(&workarea.to_string());
    }
    let (status, _, accepted) = send(router, "POST", "/v1/memories", call).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    accepted["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned()
}

/// Serves the dreamer its next held item and records `verdict` on it;
/// returns the item's artifact id.
async fn review(router: &Router, dreamer: &str, verdict: &str) -> String {
    let (status, _, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(dreamer)
            .idem(&OsIds.new_id().to_string())
            .body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(dreamer)
            .idem(&OsIds.new_id().to_string())
            .body(json!({
                "serve_id": serve["serve_id"],
                "verdict": verdict,
                "reason": "checked against the supplier's quote sheet",
            })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serve["item"]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned()
}

/// `GET /v1/memories?{query}` as `call` sends it.
async fn list(router: &Router, call: Call, query: &str) -> (StatusCode, Value) {
    let (status, _, body) = send(router, "GET", &format!("/v1/memories?{query}"), call).await;
    (status, body)
}

/// The artifact ids of a listing page, in page order.
fn listed(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| {
            item["artifact_id"]
                .as_str()
                .expect("artifact id")
                .to_owned()
        })
        .collect()
}

/// A key issued over HTTP with exactly `grants`; returns (value, key id).
async fn key_with(
    router: &Router,
    admin: &str,
    role: &str,
    principal: &str,
    idem: &str,
    grants: Value,
) -> (String, String) {
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/keys",
        Call::auth(admin)
            .idem(idem)
            .body(json!({ "role": role, "principal": principal, "grants": grants })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (
        body["value"].as_str().expect("show-once value").to_owned(),
        body["key_id"].as_str().expect("key id").to_owned(),
    )
}

#[tokio::test]
async fn own_true_lists_every_state_of_the_keys_own_rows_newest_first() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    // A writer-only key: the own-work listing needs no reader grant.
    let writer = issue_key(router, &admin, "writer", "crew/agent-7", crew).await;
    let dreamer = issue_key(router, &admin, "dreamer", "crew/reviewer-3", crew).await;
    let first = remember(
        router,
        &writer,
        "quote-am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    let second = remember(
        router,
        &writer,
        "quote-am-7742",
        "Accra Metals quoted 415 USD/t.",
        None,
    )
    .await;
    let third = remember(
        router,
        &writer,
        "quote-am-7743",
        "Accra Metals quoted 409 USD/t.",
        None,
    )
    .await;
    assert_eq!(review(router, &dreamer, "approve").await, first);
    assert_eq!(review(router, &dreamer, "reject").await, second);

    let (status, page) = list(router, Call::auth(&writer), "own=true").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed(&page), vec![third.clone(), second.clone(), first]);
    let states: Vec<&Value> = page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| &item["state"])
        .collect();
    assert_eq!(states, vec!["held", "rejected", "promoted"]);
    for item in page["items"].as_array().expect("items") {
        assert!(
            item.get("body").is_none(),
            "summaries carry no body: {item}"
        );
        assert_eq!(item["tokens"], 8, "{item}");
        assert_eq!(item["workarea_id"], json!(crew), "{item}");
        assert_eq!(item["workarea_path"], "universe.metals-crew", "{item}");
        assert_eq!(item["held_expires_at"], Value::Null, "{item}");
    }
    assert_eq!(page["next_cursor"], Value::Null, "the listing is exhausted");

    let (status, page) = list(router, Call::auth(&writer), "own=true&state=rejected").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed(&page), vec![second]);

    // The own-work listing resolves no workarea, so the override is refused.
    let (status, body) = list(
        router,
        Call::auth(&writer).workarea(&crew.to_string()),
        "own=true",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_envelope(&body, "request.validation");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn the_own_listing_covers_every_workarea_the_key_wrote_to() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, root) = bootstrap(router).await;
    let crew = create_workarea(router, &admin, root, "metals-crew").await;
    let alloys = create_workarea(router, &admin, root, "alloys").await;
    let writer = issue_key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        json!([
            { "workarea_node_id": crew, "role": "writer" },
            { "workarea_node_id": alloys, "role": "writer" },
        ]),
    )
    .await["value"]
        .as_str()
        .expect("show-once value")
        .to_owned();
    let quote = remember(
        router,
        &writer,
        "quote-am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    let alloy = remember(
        router,
        &writer,
        "alloy-6061",
        "6061 tempers to T6 at 160 C.",
        Some(alloys),
    )
    .await;

    let (status, page) = list(router, Call::auth(&writer), "own=true").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed(&page), vec![alloy.clone(), quote]);
    assert_eq!(page["items"][0]["workarea_id"], json!(alloys));
    assert_eq!(page["items"][0]["workarea_path"], "universe.alloys");
    assert_eq!(page["items"][1]["workarea_id"], json!(crew));
    assert_eq!(page["items"][1]["workarea_path"], "universe.metals-crew");

    // workarea_id narrows to a subtree the key holds a role on.
    let (status, page) = list(
        router,
        Call::auth(&writer),
        &format!("own=true&workarea_id={alloys}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed(&page), vec![alloy]);
    // A node the key holds no role on, or none at all, is absent.
    for node in [root, Uuid::from_u128(0xDEAD)] {
        let (status, body) = list(
            router,
            Call::auth(&writer),
            &format!("own=true&workarea_id={node}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_envelope(&body, "access.not_found");
    }

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn the_own_listing_never_shows_another_keys_row() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    // Two keys of one principal: principal equality never makes a row own.
    let grants = json!([{ "workarea_node_id": crew, "role": "writer" }]);
    let (one, _) = key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        "k-1",
        grants.clone(),
    )
    .await;
    let (two, _) = key_with(router, &admin, "writer", "crew/agent-7", "k-2", grants).await;
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;
    let mine = remember(
        router,
        &one,
        "quote-am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    let theirs = remember(
        router,
        &two,
        "quote-am-7742",
        "Accra Metals quoted 415 USD/t.",
        None,
    )
    .await;

    for (token, expected) in [
        (&one, vec![mine]),
        (&two, vec![theirs]),
        (&reader, Vec::new()),
    ] {
        let (status, page) = list(router, Call::auth(token), "own=true").await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(listed(&page), expected);
    }

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn a_global_denial_masks_own_rows_in_the_listing() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, root) = bootstrap(router).await;
    let crew = create_workarea(router, &admin, root, "metals-crew").await;
    let alloys = create_workarea(router, &admin, root, "alloys").await;
    let (writer, key_id) = key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        "k-1",
        json!([
            { "workarea_node_id": crew, "role": "writer" },
            { "workarea_node_id": alloys, "role": "writer" },
        ]),
    )
    .await;
    let quote = remember(
        router,
        &writer,
        "quote-am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    remember(
        router,
        &writer,
        "alloy-6061",
        "6061 tempers to T6 at 160 C.",
        Some(alloys),
    )
    .await;
    let (status, _, denial) = send(
        router,
        "POST",
        &format!("/v2/keys/{key_id}/denials"),
        Call::auth(&admin)
            .idem("d-1")
            .body(json!({ "workarea_node_id": alloys })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{denial}");

    // Deny-wins: the key's own row under the denial is masked.
    let (status, page) = list(router, Call::auth(&writer), "own=true").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed(&page), vec![quote]);
    // And the denied node is no longer one the key may name.
    let (status, body) = list(
        router,
        Call::auth(&writer),
        &format!("own=true&workarea_id={alloys}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope(&body, "access.not_found");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn a_listing_without_own_returns_promoted_tips_in_scope_only() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, root) = bootstrap(router).await;
    let crew = create_workarea(router, &admin, root, "metals-crew").await;
    let quotes = create_workarea(router, &admin, crew, "quotes").await;
    let alloys = create_workarea(router, &admin, root, "alloys").await;
    let writer = issue_key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        json!([
            { "workarea_node_id": crew, "role": "writer" },
            { "workarea_node_id": crew, "role": "reader" },
            { "workarea_node_id": alloys, "role": "writer" },
        ]),
    )
    .await["value"]
        .as_str()
        .expect("show-once value")
        .to_owned();
    let dreamer = issue_key_with(
        router,
        &admin,
        "dreamer",
        "crew/reviewer-3",
        json!([
            { "workarea_node_id": crew, "role": "dreamer" },
            { "workarea_node_id": alloys, "role": "dreamer" },
        ]),
    )
    .await["value"]
        .as_str()
        .expect("show-once value")
        .to_owned();
    let reader = issue_key(router, &admin, "reader", "crew/agent-recall", crew).await;

    let anchor = remember(
        router,
        &writer,
        "price-anchor",
        "Accra Metals anchors at 410 USD/t.",
        None,
    )
    .await;
    let quote = remember(
        router,
        &writer,
        "quote-am-7741",
        "Accra Metals quoted 412 USD/t.",
        Some(quotes),
    )
    .await;
    remember(
        router,
        &writer,
        "alloy-6061",
        "6061 tempers to T6 at 160 C.",
        Some(alloys),
    )
    .await;
    let replaced = remember(
        router,
        &writer,
        "lead-time",
        "Lead time is six weeks.",
        None,
    )
    .await;
    for _ in 0..4 {
        review(router, &dreamer, "approve").await;
    }
    let (status, _, successor) = send(
        router,
        "POST",
        &format!("/v1/memories/{replaced}/supersede"),
        Call::auth(&writer)
            .idem("sup-1")
            .body(json!({ "body": "Lead time is eight weeks." })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{successor}");
    let tip = successor["artifact_id"].as_str().expect("id").to_owned();
    assert_eq!(review(router, &dreamer, "approve").await, tip);
    remember(
        router,
        &writer,
        "pending",
        "Accra Metals may raise prices.",
        None,
    )
    .await;

    // The scope is the key's default workarea's subtree: promoted tips only —
    // never a held row, the writer's own included, nor a replaced version, nor
    // a row outside the scope.
    for token in [&reader, &writer] {
        let (status, page) = list(router, Call::auth(token), "").await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(
            listed(&page),
            vec![tip.clone(), quote.clone(), anchor.clone()]
        );
        for item in page["items"].as_array().expect("items") {
            assert_eq!(item["state"], "promoted", "{item}");
            assert_eq!(item["tip"], true, "{item}");
        }
    }
    let (status, page) = list(router, Call::auth(&reader), "state=held").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        listed(&page),
        Vec::<String>::new(),
        "no filter reaches a held row"
    );
    // workarea_id intersects the scope's subtree; X-OML-Workarea moves it.
    let (status, page) = list(
        router,
        Call::auth(&reader),
        &format!("workarea_id={quotes}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed(&page), vec![quote.clone()]);
    let (status, page) = list(
        router,
        Call::auth(&reader).workarea(&quotes.to_string()),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed(&page), vec![quote]);
    // A node without reader effective access is absent.
    let (status, body) = list(
        router,
        Call::auth(&reader),
        &format!("workarea_id={alloys}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_envelope(&body, "access.not_found");
    // Without own, a key with neither a default nor the override is refused.
    let unscoped =
        issue_key_with(router, &admin, "reader", "crew/no-default", json!([])).await["value"]
            .as_str()
            .expect("show-once value")
            .to_owned();
    let (status, body) = list(router, Call::auth(&unscoped), "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_envelope(&body, "request.workarea_unresolved");

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn a_cursor_from_another_key_or_expired_is_request_validation() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let grants = json!([{ "workarea_node_id": crew, "role": "writer" }]);
    let (writer, key_id) = key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        "k-1",
        grants.clone(),
    )
    .await;
    let (other, _) = key_with(router, &admin, "writer", "crew/agent-8", "k-2", grants).await;
    let mut written = Vec::new();
    for n in 0..3 {
        written.push(
            remember(
                router,
                &writer,
                &format!("quote-{n}"),
                "Accra Metals quoted 412 USD/t.",
                None,
            )
            .await,
        );
    }
    written.reverse();

    // A client-held cursor walks the listing, newest first, one position at
    // a time.
    let (status, first) = list(router, Call::auth(&writer), "own=true&limit=2").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(listed(&first), written[..2]);
    let cursor = first["next_cursor"]
        .as_str()
        .expect("a next page")
        .to_owned();
    let (status, second) = list(
        router,
        Call::auth(&writer),
        &format!("own=true&limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(listed(&second), written[2..]);
    assert_eq!(second["next_cursor"], Value::Null);

    // The cursor answers its own key only, and only within api.cursor_ttl.
    let decoded = oml_app::cursor::ListingCursor::decode(&cursor).expect("our cursor");
    assert_eq!(decoded.key_id.to_string(), key_id);
    let stale = oml_app::cursor::ListingCursor {
        issued_at: oml_app::domain::EventTime::parse("2000-01-01T00:00:00.000000Z")
            .expect("fixed width"),
        ..decoded
    }
    .encode();
    for (token, cursor) in [
        (&other, cursor.as_str()),
        (&writer, stale.as_str()),
        (&writer, "not-a-cursor"),
    ] {
        let (status, body) = list(
            router,
            Call::auth(token),
            &format!("own=true&cursor={cursor}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_envelope(&body, "request.validation");
    }
    for query in [
        "own=true&limit=0",
        "own=true&limit=201",
        "own=yes",
        "own=true&colour=red",
    ] {
        let (status, body) = list(router, Call::auth(&writer), query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_envelope(&body, "request.validation");
    }

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn an_operator_key_is_refused_own_work_with_verb_ceiling() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    bootstrap(router).await;
    // An operator key is a provider-level row (tenant_id NULL): no admin
    // issues one, so the fixture writes the row as the table's owner would,
    // with row-level security off for that one insert.
    let operator = format!("oml_{}", "0".repeat(42) + "1");
    let hash = sha256_digest_string(operator.as_bytes());
    let mut conn = rig.store.pool().acquire().await.expect("connection");
    sqlx::query("ALTER TABLE api_keys NO FORCE ROW LEVEL SECURITY")
        .execute(&mut *conn)
        .await
        .expect("lift force");
    sqlx::query(
        "INSERT INTO api_keys (tenant_id, hash, role, principal, held_by, issued_seq) \
         VALUES (NULL, decode($1, 'hex'), 'operator', 'provider/operator-1', 'human', 0)",
    )
    .bind(hash.trim_start_matches("sha256:"))
    .execute(&mut *conn)
    .await
    .expect("provider-level operator key");
    sqlx::query("ALTER TABLE api_keys FORCE ROW LEVEL SECURITY")
        .execute(&mut *conn)
        .await
        .expect("restore force");
    drop(conn);

    // The operator never reads content: refused by its role, before any
    // tenant is consulted, with or without own.
    for query in ["own=true", ""] {
        let (status, body) = list(router, Call::auth(&operator), query).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{query}: {body}");
        assert_envelope(&body, "access.verb_ceiling");
    }

    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn claim_key_filter_lists_one_claims_versions() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let writer = issue_key_with(
        router,
        &admin,
        "writer",
        "crew/agent-7",
        json!([
            { "workarea_node_id": crew, "role": "writer" },
            { "workarea_node_id": crew, "role": "reader" },
        ]),
    )
    .await["value"]
        .as_str()
        .expect("show-once value")
        .to_owned();
    let dreamer = issue_key(router, &admin, "dreamer", "crew/reviewer-3", crew).await;
    let first = remember(
        router,
        &writer,
        "quote-am-7741",
        "Accra Metals quoted 412 USD/t.",
        None,
    )
    .await;
    remember(
        router,
        &writer,
        "alloy-6061",
        "6061 tempers to T6 at 160 C.",
        None,
    )
    .await;
    review(router, &dreamer, "approve").await;
    review(router, &dreamer, "approve").await;
    let (status, _, successor) = send(
        router,
        "POST",
        &format!("/v1/memories/{first}/supersede"),
        Call::auth(&writer)
            .idem("sup-1")
            .body(json!({ "body": "Accra Metals revised the quote to 408 USD/t." })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{successor}");
    let second = successor["artifact_id"].as_str().expect("id").to_owned();

    let (status, page) = list(
        router,
        Call::auth(&writer),
        "own=true&claim_key=quote-am-7741",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed(&page), vec![second, first.clone()]);
    let items = page["items"].as_array().expect("items");
    assert_eq!(items[0]["version"], 2);
    assert_eq!(items[0]["state"], "held");
    assert_eq!(
        items[0]["replaces"],
        json!({ "artifact_id": first, "version": 1 })
    );
    assert_eq!(items[1]["version"], 1);
    assert_eq!(items[1]["state"], "promoted");
    for item in items {
        assert_eq!(item["claim_key"], "quote-am-7741", "{item}");
    }

    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// Supersession over HTTP: the serve, retirement and the verdict-time
// compare-and-set (ADR-0025 d10, d11)
// ---------------------------------------------------------------------------

/// A writer key with writer and reader on `crew`, a dreamer key with
/// dreamer and reader there, and a reader key: (writer, dreamer, reader).
async fn supersession_keys(router: &Router, admin: &str, crew: Uuid) -> (String, String, String) {
    let value = |body: Value| body["value"].as_str().expect("value").to_owned();
    let writer = value(
        issue_key_with(
            router,
            admin,
            "writer",
            "crew/agent-7",
            json!([
                { "workarea_node_id": crew, "role": "writer" },
                { "workarea_node_id": crew, "role": "reader" },
            ]),
        )
        .await,
    );
    let dreamer = value(
        issue_key_with(
            router,
            admin,
            "dreamer",
            "crew/reviewer-3",
            json!([
                { "workarea_node_id": crew, "role": "dreamer" },
                { "workarea_node_id": crew, "role": "reader" },
            ]),
        )
        .await,
    );
    let reader = issue_key(router, admin, "reader", "crew/agent-recall", crew).await;
    (writer, dreamer, reader)
}

/// Submits `body` as `writer` under `claim_key` and has `dreamer` approve it
/// through a serve: the promoted first version's id.
async fn promoted_over_http(
    router: &Router,
    writer: &str,
    dreamer: &str,
    claim_key: &str,
    body: &str,
) -> String {
    let (status, _, accepted) = send(
        router,
        "POST",
        "/v1/memories",
        Call::auth(writer)
            .idem(&format!("s-{claim_key}"))
            .body(json!({
                "verb": "remember", "claim_key": claim_key, "kind": "memory", "body": body,
            })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    let id = accepted["artifact_id"].as_str().expect("id").to_owned();
    approve_over_http(router, dreamer, &id).await;
    id
}

/// `dreamer` takes the next serve, which must be `id`, and approves it.
async fn approve_over_http(router: &Router, dreamer: &str, id: &str) -> Value {
    let (status, _, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(dreamer).idem(&format!("r-{id}")).body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["artifact_id"], id, "{serve}");
    let (status, _, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(dreamer).idem(&format!("v-{id}")).body(json!({
            "serve_id": serve["serve_id"], "verdict": "approve",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    verdict
}

/// The artifact ids `key`'s recall answers for `query`, in rank order.
async fn recalled(router: &Router, key: &str, query: &str) -> Vec<String> {
    let (status, _, recall) = send(
        router,
        "POST",
        "/v1/memories/recall",
        Call::auth(key).body(json!({ "query": query })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    recall["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| h["memory"]["artifact_id"].as_str().expect("id").to_owned())
        .collect()
}

#[tokio::test]
async fn the_review_serve_of_a_supersession_carries_the_replaced_version() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, _tenant, crew) = crew_tenant(router).await;
    let (writer, dreamer, _reader) = supersession_keys(router, &admin, crew).await;

    let first = promoted_over_http(
        router,
        &writer,
        &dreamer,
        "quote",
        "Accra Metals quoted 412 USD/t.",
    )
    .await;
    let (_, _, target) = send(
        router,
        "GET",
        &format!("/v1/memories/{first}"),
        Call::auth(&dreamer),
    )
    .await;
    let (status, _, successor) = send(
        router,
        "POST",
        &format!("/v1/memories/{first}/supersede"),
        Call::auth(&writer)
            .idem("sup-1")
            .body(json!({"body": "Accra Metals revised the quote to 408 USD/t."})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{successor}");

    let (status, _, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(&dreamer).idem("r-sup").body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["artifact_id"], successor["artifact_id"]);
    assert_eq!(
        serve["item"]["replaces"],
        json!({ "artifact_id": first, "version": 1 })
    );
    assert_eq!(serve["replaces"]["artifact_id"], first.as_str());
    assert_eq!(serve["replaces"]["version"], 1);
    assert_eq!(serve["replaces"]["tip"], true, "still the tip while held");
    assert_eq!(serve["replaces"]["body"], "Accra Metals quoted 412 USD/t.");
    assert_eq!(
        serve["served_digests"]["replaces"], target["plaintext_digest"],
        "the serve records the replaced version's digest"
    );
    assert_eq!(
        serve["served_digests"]["body"],
        successor["plaintext_digest"]
    );
    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn a_ratified_retirement_over_http_leaves_recall_and_journals_lifecycle_supersede() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, tenant, crew) = crew_tenant(router).await;
    let (writer, dreamer, reader) = supersession_keys(router, &admin, crew).await;

    let first = promoted_over_http(
        router,
        &writer,
        &dreamer,
        "runbook",
        "Drain the canary before promotion.",
    )
    .await;
    let (status, _, proposal) = send(
        router,
        "POST",
        &format!("/v1/memories/{first}/supersede"),
        Call::auth(&writer).idem("ret-1").body(json!({
            "body": "Superseded by the 2026 canary runbook; stop serving.",
            "retirement": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{proposal}");
    assert_eq!(proposal["retirement"], true);
    assert_eq!(
        recalled(router, &reader, "canary").await,
        vec![first.clone()],
        "a held retirement changes no recall"
    );

    let retirement = proposal["artifact_id"].as_str().expect("id").to_owned();
    approve_over_http(router, &dreamer, &retirement).await;
    assert!(recalled(router, &reader, "canary").await.is_empty());
    for (id, tip) in [(&first, false), (&retirement, true)] {
        let (status, _, body) = send(
            router,
            "GET",
            &format!("/v1/memories/{id}"),
            Call::auth(&reader),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["state"], "promoted");
        assert_eq!(body["tip"], tip, "{body}");
    }

    let head = head_seq(&rig, tenant).await;
    let flip = rig
        .store
        .events_range(tenant, head, 1)
        .await
        .expect("range")
        .remove(0)
        .envelope;
    assert_eq!(flip.event_type, "lifecycle.supersede");
    assert_eq!(flip.payload["retirement"], true);
    assert_eq!(flip.payload["new_artifact_id"], retirement.as_str());
    assert_eq!(flip.payload["replaces_artifact_id"], first.as_str());
    drop_db(&rig.db_name).await;
}

#[tokio::test]
async fn an_approval_whose_target_lost_the_tip_is_tip_conflict_and_journals_nothing() {
    let rig = fresh_rig().await;
    let router = &rig.router;
    let (admin, tenant, crew) = crew_tenant(router).await;
    let (writer, dreamer, _reader) = supersession_keys(router, &admin, crew).await;
    let editor = issue_key_with(
        router,
        &admin,
        "dreamer",
        "crew/editor-5",
        json!([
            { "workarea_node_id": crew, "role": "dreamer" },
            { "workarea_node_id": crew, "role": "reader" },
        ]),
    )
    .await["value"]
        .as_str()
        .expect("value")
        .to_owned();

    let first = promoted_over_http(router, &writer, &dreamer, "quote", "412 USD/t").await;
    let propose = |key: &str, idem: &str, body: &str| {
        let call = Call::auth(key).idem(idem).body(json!({ "body": body }));
        let path = format!("/v1/memories/{first}/supersede");
        async move { send(router, "POST", &path, call).await }
    };
    let (status, _, revision) = propose(&writer, "sup-w", "408 USD/t").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{revision}");
    let (status, _, edit) = propose(&editor, "sup-e", "409 USD/t").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{edit}");
    assert_eq!(
        edit["version"], 3,
        "rival successors take distinct ordinals"
    );

    let revision_id = revision["artifact_id"].as_str().expect("id").to_owned();
    approve_over_http(router, &dreamer, &revision_id).await;

    let (status, _, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Call::auth(&dreamer).idem("r-edit").body(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["artifact_id"], edit["artifact_id"]);
    let head = head_seq(&rig, tenant).await;
    let (status, _, body) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer).idem("v-edit").body(json!({
            "serve_id": serve["serve_id"], "verdict": "approve",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_envelope(&body, "submission.tip_conflict");
    assert_eq!(
        body["error"]["details"]["current_tip"],
        json!({ "artifact_id": revision_id, "version": 2 })
    );
    assert_eq!(head_seq(&rig, tenant).await, head, "nothing is journalled");

    // The serve is not consumed: the stale successor is still the
    // reviewer's to reject.
    let (status, _, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Call::auth(&dreamer).idem("v-edit-2").body(json!({
            "serve_id": serve["serve_id"], "verdict": "reject", "reason": "stale",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    assert_eq!(verdict["state"], "rejected");
    drop_db(&rig.db_name).await;
}
