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

    // The deterministic scan can veto; the rejection is journalled.
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
    assert_eq!(body["tip"], true);
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

    // Recall: reader ceiling, promoted rows only, steering first and empty.
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
    assert!(hits.iter().all(|h| h["score"].is_number()));
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

    // Events tail: seq ascending from after_seq, head_seq for lag.
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
        seqs.windows(2).all(|w| w[1] == w[0] + 1),
        "seq ascending, gapless: {seqs:?}"
    );
    assert_eq!(seqs[0], 1);
    assert_eq!(events["head_seq"].as_u64(), seqs.last().copied());
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

    let (status, _, successor) = send(
        router,
        "POST",
        &format!("/v1/memories/{artifact}/supersede"),
        Call::auth(&writer).idem("sup-1").body(json!({
            "body": "Supplier Accra Metals revised the quote to 408 USD/t.",
            "reason": "supplier issued a revised quote",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{successor}");
    assert_eq!(successor["state"], "held");
    assert_eq!(successor["claim_key"], "supplier-quote-am-7741");
    assert_ne!(successor["artifact_id"], accepted["artifact_id"]);

    // The old tip flipped in the same journalled act; supersession adds no
    // state, so the replaced version reads promoted (ADR-0025 d10).
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

    // A second supersession of the same target: not the current tip.
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
    // is never a hit (retrieval.md; ADR-0025 d8).
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
