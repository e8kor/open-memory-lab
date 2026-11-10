//! Claim formats over the lead composition against a throwaway database
//! (ADR-0056): the format is part of the signed submission and fixed for the
//! version; the signature covers the source as stored; a format changes only
//! by supersession; a refused Mermaid construct, an invalid fragment and a
//! secret in a decoded value are refused by name before anything is stored;
//! a non-prose claim carries a title; search reads the derived search text;
//! and a claim written without a format is stored and indexed exactly as it
//! was before the field existed.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::Connection;
use sqlx::postgres::PgConnection;
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::IdGen as _;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_state, pg_ping};
use oml_test_pg::{DB_N, base_url, drop_db, mint_database, owner_url, run_tag, serving_url};

const ENROLLMENT: &str = "enr_test_fixture_token";

// ---------------------------------------------------------------------------
// Throwaway database + rig (the oml-pg test pattern)
// ---------------------------------------------------------------------------

/// `db` through `DATABASE_URL`'s own login, the superuser the role pair
/// was minted by: the journal reads below see every tenant's rows, which
/// row security hides from the owner (ADR-0034 d8).
fn super_url(db: &str) -> String {
    let base = base_url();
    let (head, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{head}/{db}")
}

struct Rig {
    router: Router,
    store: PgStore,
    db_name: String,
}

async fn fresh_rig() -> Option<Rig> {
    let db_name = format!(
        "oml_test_formats_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db_name).await {
        return None;
    }
    let store = PgStore::connect(&serving_url(&db_name))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db_name));
    store
        .migrate(&owner_url(&db_name))
        .await
        .expect("embedded migrations apply");
    let signing = SigningKey::from_bytes(&OsIds.new_secret());
    let router = lead_state(
        &store,
        signing,
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    Some(Rig {
        router,
        store,
        db_name,
    })
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    idem: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(idem) = idem {
        builder = builder.header("idempotency-key", idem);
    }
    let body = match body {
        None => Body::empty(),
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
    };
    let response = router
        .clone()
        .oneshot(builder.body(body).expect("request builds"))
        .await
        .expect("infallible router");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 24)
        .await
        .expect("body collects");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("responses are JSON")
    };
    (status, value)
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("<no code>")
}

/// The bootstrap ceremony; returns (admin token, root workarea id).
async fn bootstrap(router: &Router) -> (String, Uuid) {
    let (status, body) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(json!({
            "enrollment_token": ENROLLMENT,
            "name": "root-admin",
            "tenant_name": "test-tenant",
            "admin_principal": "platform/admin",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "bootstrap claim: {body}");
    let token = body["value"].as_str().expect("show-once value").to_owned();
    let root = Uuid::parse_str(body["root_workarea_id"].as_str().expect("root id")).expect("uuid");
    (token, root)
}

/// Issues a key named `name` under the admin token; returns the 201 body.
async fn issue(
    router: &Router,
    admin: &str,
    name: &str,
    role: &str,
    principal: &str,
    held_by: &str,
    grants: Value,
) -> Value {
    let (status, body) = send(
        router,
        "POST",
        "/v2/keys",
        Some(admin),
        Some(&format!("issue-{name}")),
        Some(json!({
            "role": role, "name": name, "principal": principal,
            "held_by": held_by, "grants": grants,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "issue {name}: {body}");
    body
}

fn value_of(issued: &Value) -> String {
    issued["value"]
        .as_str()
        .expect("show-once value")
        .to_owned()
}

async fn create_workarea(router: &Router, admin: &str, parent: Uuid, name: &str) -> Uuid {
    let (status, body) = send(
        router,
        "POST",
        "/v2/workareas",
        Some(admin),
        Some(&format!("wa-{name}")),
        Some(json!({ "parent": parent, "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Uuid::parse_str(body["id"].as_str().expect("workarea id")).expect("uuid")
}

/// A fresh Idempotency-Key per request a test means as new.
static IDEM: AtomicU64 = AtomicU64::new(0);

const MERMAID: &str = "text/vnd.oml.mermaid";
const DIAGRAM: &str = "flowchart LR\n  A[Checkout] -->|calls| B[Payments]\n  B --> C[Ledger]\n";

struct World {
    rig: Rig,
    node: Uuid,
    writer: String,
    reviewer: String,
}

async fn world() -> Option<World> {
    let rig = fresh_rig().await?;
    let (admin, root) = bootstrap(&rig.router).await;
    let node = create_workarea(&rig.router, &admin, root, "docs").await;
    let on = |roles: &[&str]| {
        Value::from(
            roles
                .iter()
                .map(|r| json!({ "workarea": node, "role": r }))
                .collect::<Vec<_>>(),
        )
    };
    let writer = value_of(
        &issue(
            &rig.router,
            &admin,
            "scribe",
            "writer",
            "crew/scribe",
            "agent",
            on(&["writer", "reader"]),
        )
        .await,
    );
    let reviewer = value_of(
        &issue(
            &rig.router,
            &admin,
            "person-review",
            "dreamer",
            "person-review",
            "human",
            on(&["dreamer", "reader"]),
        )
        .await,
    );
    Some(World {
        rig,
        node,
        writer,
        reviewer,
    })
}

impl World {
    async fn write(&self, key: &str, body: Value) -> (StatusCode, Value) {
        let mut request = json!({ "claim_key": key, "kind": "memory", "workarea": self.node });
        for (k, v) in body.as_object().expect("object") {
            request[k] = v.clone();
        }
        send(
            &self.rig.router,
            "POST",
            "/v1/memories",
            Some(&self.writer),
            Some(&format!("w-{key}-{}", IDEM.fetch_add(1, Ordering::SeqCst))),
            Some(request),
        )
        .await
    }

    async fn written(&self, key: &str, body: Value) -> String {
        let (status, response) = self.write(key, body).await;
        assert_eq!(status, StatusCode::ACCEPTED, "write {key}: {response}");
        response["artifact_id"].as_str().expect("id").to_owned()
    }

    async fn approve(&self, id: &str) {
        let (status, body) = send(
            &self.rig.router,
            "POST",
            "/v2/review/verdicts/bulk",
            Some(&self.reviewer),
            Some(&format!("approve-{}", IDEM.fetch_add(1, Ordering::SeqCst))),
            Some(json!({ "verdict": "approve", "artifact_ids": [id] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    async fn get(&self, id: &str) -> Value {
        let (status, body) = send(
            &self.rig.router,
            "GET",
            &format!("/v1/memories/{id}"),
            Some(&self.writer),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn submission_event(&self, id: &str) -> Value {
        let mut admin = PgConnection::connect(&super_url(&self.rig.db_name))
            .await
            .expect("connect");
        let payload: Value = sqlx::query_scalar(
            "SELECT payload FROM journal WHERE event_type LIKE 'submission.%' AND payload->>'artifact_id' = $1",
        )
        .bind(id)
        .fetch_one(&mut admin)
        .await
        .expect("the submission event");
        payload
    }

    async fn done(self) {
        drop(self.rig.store);
        drop_db(&self.rig.db_name).await;
    }
}

#[tokio::test]
async fn omitting_format_stores_text_plain_and_indexes_exactly_as_today() {
    let Some(w) = world().await else { return };
    let id = w
        .written(
            "plain",
            json!({ "body": "Retries stop after five attempts" }),
        )
        .await;
    assert_eq!(w.get(&id).await["format"], "text/plain");
    assert_eq!(w.submission_event(&id).await["format"], "text/plain");
    let mut admin = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("connect");
    let (format, search_text, same): (String, Option<String>, bool) = sqlx::query_as(
        "SELECT format, search_text, search_vector = oml_search_vector(claim) FROM memories WHERE id = $1",
    )
    .bind(Uuid::parse_str(&id).expect("uuid"))
    .fetch_one(&mut admin)
    .await
    .expect("row");
    assert_eq!(format, "text/plain");
    assert_eq!(search_text, None, "a prose row stores no derived text");
    assert!(same, "its lexemes are the lexemes of its claim");
    drop(admin);
    w.done().await;
}

#[tokio::test]
async fn an_event_without_a_format_reads_as_text_plain_and_keeps_its_digest() {
    let Some(w) = world().await else { return };
    let id = w
        .written("same", json!({ "body": "one", "format": "text/plain" }))
        .await;
    let explicit = w.get(&id).await["plaintext_digest"].clone();
    let other = w.written("same-two", json!({ "body": "one" })).await;
    assert_eq!(
        w.get(&other).await["plaintext_digest"],
        explicit,
        "the body digest ignores the format"
    );
    w.done().await;
}

#[tokio::test]
async fn the_signature_covers_the_format_and_the_stored_source_bytes() {
    let Some(w) = world().await else { return };
    let json_text = "{\"a\": 1}";
    let as_json = w
        .written(
            "as-json",
            json!({ "body": json_text, "format": "application/json", "title": "A fragment" }),
        )
        .await;
    let as_plain = w
        .written(
            "as-plain",
            json!({ "body": json_text, "title": "A fragment" }),
        )
        .await;
    let signed_json = w.submission_event(&as_json).await;
    let signed_plain = w.submission_event(&as_plain).await;
    assert_eq!(signed_json["format"], "application/json");
    assert_eq!(signed_plain["format"], "text/plain");
    assert_eq!(
        signed_json["plaintext_digest"], signed_plain["plaintext_digest"],
        "the digest is of the source as stored, whatever its format"
    );
    // The source is stored as sent: no re-indentation, no key sorting.
    let spaced = "{\n   \"b\": 2,\n   \"a\": 1\n}";
    let id = w
        .written(
            "spaced",
            json!({ "body": spaced, "format": "application/json", "title": "Spaced" }),
        )
        .await;
    assert_eq!(w.get(&id).await["text"], spaced);
    assert_eq!(w.get(&id).await["format"], "application/json");
    w.done().await;
}

#[tokio::test]
async fn a_format_changes_only_by_supersession() {
    let Some(w) = world().await else { return };
    let v1 = w
        .written(
            "flow",
            json!({ "body": "Checkout calls Payments.", "title": "The payment flow" }),
        )
        .await;
    w.approve(&v1).await;
    let (status, v2) = send(
        &w.rig.router,
        "POST",
        &format!("/v1/memories/{v1}/supersede"),
        Some(&w.writer),
        Some("flow-v2"),
        Some(json!({ "body": DIAGRAM, "format": MERMAID, "title": "The payment flow", "expected_version": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{v2}");
    let v2 = v2["artifact_id"].as_str().expect("id").to_owned();
    assert_eq!(w.get(&v2).await["format"], MERMAID);
    assert_eq!(
        w.get(&v1).await["format"],
        "text/plain",
        "the target is unchanged"
    );
    w.done().await;
}

#[tokio::test]
async fn a_mermaid_directive_click_or_style_is_refused_by_name_and_stores_nothing() {
    let Some(w) = world().await else { return };
    for (source, reason, line) in [
        (
            "%%{init: {\"securityLevel\": \"loose\"}}%%\nflowchart LR\n A-->B",
            "mermaid.directive_refused",
            1,
        ),
        (
            "flowchart LR\n A-->B\n click A callback \"tip\"",
            "mermaid.interaction_refused",
            3,
        ),
        (
            "flowchart LR\n A-->B\n style A fill:#fff",
            "mermaid.styling_refused",
            3,
        ),
        ("gantt\n title x", "mermaid.unsupported_diagram", 1),
    ] {
        let (status, body) = w
            .write(
                "refused",
                json!({ "body": source, "format": MERMAID, "title": "t" }),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(error_code(&body), "submission.format_invalid", "{body}");
        assert_eq!(body["error"]["details"]["reason"], reason, "{body}");
        assert_eq!(body["error"]["details"]["line"], line, "{body}");
    }
    let mut admin = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("connect");
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM memories WHERE claim_key = 'refused'")
        .fetch_one(&mut admin)
        .await
        .expect("count");
    assert_eq!(held, 0, "a refusal journals nothing and stores no row");
    drop(admin);
    w.done().await;
}

#[tokio::test]
async fn an_unknown_format_is_request_validation_listing_the_accepted_values() {
    let Some(w) = world().await else { return };
    let (status, body) = w
        .write("x", json!({ "body": "a", "format": "mermaid" }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "request.validation");
    assert_eq!(body["error"]["details"]["field"], "format");
    assert!(
        body["error"]["details"]["allowed"]
            .as_array()
            .expect("list")
            .iter()
            .any(|v| v == MERMAID)
    );
    // Steering reads prose: another format is refused naming `format`.
    let (status, body) = send(
        &w.rig.router, "POST", "/v1/memories", Some(&w.writer), Some("steer-fmt"),
        Some(json!({ "kind": "steering", "body": "{\"a\":1}", "workarea": w.node, "format": "application/json", "title": "t" })),
    ).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["details"]["field"], "format");
    w.done().await;
}

#[tokio::test]
async fn a_non_prose_claim_without_a_title_is_refused_naming_title() {
    let Some(w) = world().await else { return };
    let (status, body) = w
        .write("untitled", json!({ "body": DIAGRAM, "format": MERMAID }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "request.validation");
    assert_eq!(body["error"]["details"]["field"], "title");
    let (status, body) = w
        .write(
            "long",
            json!({ "body": DIAGRAM, "format": MERMAID, "title": "x".repeat(101) }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["details"]["field"], "title");
    let (status, body) = w
        .write(
            "breaks",
            json!({ "body": DIAGRAM, "format": MERMAID, "title": "a\nb" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    // Prose needs none, and a prose claim may carry one.
    let id = w
        .written(
            "prose-titled",
            json!({ "body": "plain text", "title": "A title" }),
        )
        .await;
    assert_eq!(w.get(&id).await["title"], "A title");
    w.done().await;
}

#[tokio::test]
async fn a_secret_in_a_json_value_is_vetoed_naming_the_path_never_the_value() {
    let Some(w) = world().await else { return };
    // The source alone shows no marker: the scalar decodes to one.
    let hidden = "{\"note\": \"\\u005boml:veto]\"}";
    assert!(!hidden.contains("[oml:veto]"));
    let (status, body) = w
        .write(
            "hidden",
            json!({ "body": hidden, "format": "application/json", "title": "t" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "submission.scan_veto", "{body}");
    assert!(
        body["error"]["details"]["finding"]
            .as_str()
            .expect("finding")
            .contains("note"),
        "{body}"
    );
    // A value under a secret-shaped key is vetoed by the format's own rule.
    let (status, body) = w.write("keyed", json!({ "body": "{\"db\": {\"password\": \"hunter2hunter2\"}}", "format": "application/json", "title": "t" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "submission.scan_veto");
    let finding = body["error"]["details"]["finding"]
        .as_str()
        .expect("finding");
    assert!(
        finding.contains("secret.keyed_value") && finding.contains("db.password"),
        "{finding}"
    );
    assert!(
        !body.to_string().contains("hunter2"),
        "never the value: {body}"
    );
    // The same bytes as prose still pass today's scan: a format only adds findings.
    let id = w
        .written(
            "as-prose",
            json!({ "body": "{\"db\": {\"password\": \"hunter2hunter2\"}}" }),
        )
        .await;
    assert!(!id.is_empty());
    w.done().await;
}

#[tokio::test]
async fn search_reads_the_derived_search_text_of_a_diagram() {
    let Some(w) = world().await else { return };
    let id = w.written("flow", json!({ "body": DIAGRAM, "format": MERMAID, "title": "The payment flow from checkout to settlement" })).await;
    let mut admin = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("connect");
    let (text, by, covered): (String, String, bool) = sqlx::query_as(
        "SELECT search_text, search_text_by, search_vector = oml_search_vector(search_text) FROM memories WHERE id = $1",
    )
    .bind(Uuid::parse_str(&id).expect("uuid"))
    .fetch_one(&mut admin)
    .await
    .expect("row");
    assert_eq!(
        text,
        "The payment flow from checkout to settlement\nCheckout (A) calls Payments (B).\nPayments (B) leads to Ledger (C)."
    );
    assert_eq!(by, "mermaid/1");
    assert!(
        covered,
        "the stored lexemes are the search text's, never the arrow syntax's"
    );
    drop(admin);
    w.approve(&id).await;
    let (status, hits) = send(
        &w.rig.router,
        "POST",
        "/v1/memories/recall",
        Some(&w.writer),
        None,
        Some(json!({ "query": "which service calls Payments", "workarea": w.node })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hits}");
    let items = hits["hits"].as_array().expect("hits");
    assert!(
        items
            .iter()
            .any(|i| i["memory"]["artifact_id"] == id.as_str() || i["artifact_id"] == id.as_str()),
        "{hits}"
    );
    let item = &items[0];
    let item = if item["memory"].is_object() {
        &item["memory"]
    } else {
        item
    };
    assert_eq!(item["format"], MERMAID, "a hit carries its format");
    assert_eq!(
        item["text"], DIAGRAM,
        "agents get the source, never the search text"
    );
    w.done().await;
}

#[tokio::test]
async fn the_near_copy_keys_of_a_diagram_are_the_keys_of_its_search_text() {
    let Some(w) = world().await else { return };
    let id = w.written("keys", json!({ "body": DIAGRAM, "format": MERMAID, "title": "The payment flow from checkout to settlement" })).await;
    let mut admin = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("connect");
    let (held, same): (i64, bool) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM similar_keys s WHERE s.memory_id = m.id), \
                ARRAY(SELECT s.key FROM similar_keys s WHERE s.memory_id = m.id ORDER BY 1) \
                  = ARRAY(SELECT k.key FROM oml_similar_keys(m.search_text) k ORDER BY 1) \
           FROM memories m WHERE m.id = $1",
    )
    .bind(Uuid::parse_str(&id).expect("uuid"))
    .fetch_one(&mut admin)
    .await
    .expect("row");
    assert!(held > 0, "a held diagram holds keys");
    assert!(
        same,
        "the keys are those of the search text, never of the arrow syntax"
    );
    drop(admin);
    w.done().await;
}
