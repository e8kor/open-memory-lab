//! The three workarea acts the web console confirms, over the in-memory
//! doubles: proposing a transition, ratifying one and setting the mode each
//! replay the recorded outcome under the same `Idempotency-Key` and request,
//! with `Idempotency-Replay: true` and nothing acted again, and refuse the
//! same key on another request with `idempotency.digest_mismatch`
//! (ADR-0070 d3 layer 4, d5). Each test is named after the sentence it
//! proves.

mod support;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::{HeldBy, Role};
use oml_app::doubles::MemStore;
use oml_app::ports::JournalStore as _;
use oml_app::types::{BootstrapRequest, GrantRequest, IssueKeyRequest};

use support::{app, state};

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

async fn send(
    router: &Router,
    path: &str,
    bearer: &str,
    key: &str,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let builder = Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header("idempotency-key", key);
    let request = match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("a request");
    let response = router.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn replayed(headers: &HeaderMap) -> bool {
    headers
        .get("idempotency-replay")
        .is_some_and(|v| v == "true")
}

struct World {
    store: MemStore,
    router: Router,
    tenant: Uuid,
    admin: String,
    ann: String,
    proj: Uuid,
    draft: Uuid,
}

/// `universe` → `proj` and `draft`; the admin key, and ann, a person's
/// dreamer key on both.
async fn world() -> World {
    let store = MemStore::new();
    let app = app(&store);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("acme-admin"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let mut nodes = Vec::new();
    for n in ["proj", "draft"] {
        nodes.push(
            app.create_workarea(&admin, &name(n), Some(boot.root_workarea_id.into()), None)
                .await
                .expect("a workarea")
                .workarea
                .workarea_id,
        );
    }
    let grants = nodes
        .iter()
        .flat_map(|node| {
            [Role::Dreamer, Role::Reader].map(|role| GrantRequest {
                node: (*node).into(),
                role,
                deny: Vec::new(),
                covers: Covers::Subtree,
            })
        })
        .collect();
    let ann = app
        .issue_key(
            &admin,
            &IssueKeyRequest {
                profile: None,
                role: Role::Dreamer,
                name: name("ann-review"),
                principal: "ann".to_owned(),
                held_by: Some(HeldBy::Human),
                default_workarea: None,
                crew: None,
                grants,
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("ann's key")
        .value;
    World {
        router: state(&store, app).into_router(),
        tenant: boot.tenant_id,
        store,
        admin,
        ann,
        proj: nodes[0],
        draft: nodes[1],
    }
}

#[tokio::test]
async fn a_proposal_replays_under_the_same_key_and_request_and_refuses_the_key_on_another() {
    let w = world().await;
    let path = format!("/v2/workareas/{}/transitions", w.proj);
    let body = json!({"to": "committed"});
    let (status, headers, first) = send(&w.router, &path, &w.ann, "p-1", Some(body.clone())).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert!(!replayed(&headers));
    let (status, headers, again) = send(&w.router, &path, &w.ann, "p-1", Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert!(replayed(&headers), "the resubmission is a replay");
    assert_eq!(again, first);
    let pending = w.store.pending_transitions(w.tenant, w.proj).await.unwrap();
    assert_eq!(pending.len(), 1, "proposed once: {pending:?}");
    let (status, _, refused) = send(
        &w.router,
        &path,
        &w.ann,
        "p-1",
        Some(json!({"to": "validated"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
}

#[tokio::test]
async fn a_ratification_replays_under_the_same_key_and_refuses_the_key_on_another_transition() {
    let w = world().await;
    let propose = format!("/v2/workareas/{}/transitions", w.proj);
    let (status, _, proposal) = send(
        &w.router,
        &propose,
        &w.ann,
        "p-1",
        Some(json!({"to": "committed"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{proposal}");
    let ratify = |t: &str| format!("/v2/workareas/{}/transitions/{t}/ratify", w.proj);
    let first_id = proposal["transition_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let (status, headers, first) = send(&w.router, &ratify(&first_id), &w.admin, "r-1", None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(!replayed(&headers));
    // Without the replay a second ratification fails as no longer live;
    // with it, the person sees the act that already succeeded.
    let (status, headers, again) = send(&w.router, &ratify(&first_id), &w.admin, "r-1", None).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert!(replayed(&headers), "the resubmission is a replay");
    assert_eq!(again, first);
    let node = w.store.workarea(w.tenant, w.proj).await.unwrap().unwrap();
    assert_eq!(node.status.as_str(), "committed");
    // The same key on another transition is refused by name.
    let (status, _, next) = send(
        &w.router,
        &propose,
        &w.ann,
        "p-2",
        Some(json!({"to": "validated"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{next}");
    let next_id = next["transition_id"].as_str().expect("an id").to_owned();
    let (status, _, refused) = send(&w.router, &ratify(&next_id), &w.admin, "r-1", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
    let node = w.store.workarea(w.tenant, w.proj).await.unwrap().unwrap();
    assert_eq!(node.status.as_str(), "committed", "nothing ratified");
}

#[tokio::test]
async fn a_mode_change_replays_under_the_same_key_and_request_and_refuses_the_key_on_another() {
    let w = world().await;
    let path = format!("/v2/workareas/{}/mode", w.draft);
    let body = json!({"mode": "in_progress"});
    let (status, headers, first) = send(&w.router, &path, &w.ann, "m-1", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(!replayed(&headers));
    let (status, headers, again) = send(&w.router, &path, &w.ann, "m-1", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert!(replayed(&headers), "the resubmission is a replay");
    assert_eq!(again, first);
    let (status, _, refused) = send(
        &w.router,
        &path,
        &w.ann,
        "m-1",
        Some(json!({"mode": "standard"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
    let node = w.store.workarea(w.tenant, w.draft).await.unwrap().unwrap();
    assert!(node.in_progress, "the refused request committed nothing");
}
