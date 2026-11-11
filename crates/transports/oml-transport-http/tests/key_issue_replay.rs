//! A bearer key's `POST /v2/keys` and `POST /v2/keys/{key}/connect-codes`
//! claim their `Idempotency-Key` before they act, as the web console's acts
//! do: a resubmission under the same key and body replays the first answer
//! without the key's value or the connect code, which are shown once, and
//! acts nothing again, even when the first request was dropped after its
//! commit (ADR-0031 d7; #127, #233). Over the in-memory doubles. Each test
//! is named after the sentence it proves.

mod support;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::oauth::Issuer;
use oml_app::doubles::MemStore;
use oml_app::ports::JournalStore as _;
use oml_app::types::BootstrapRequest;

use support::{app, state};

async fn send(
    router: &Router,
    path: &str,
    bearer: &str,
    key: &str,
    body: &Value,
) -> (StatusCode, HeaderMap, Value) {
    let request = Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header("idempotency-key", key)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
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
    tenant: uuid::Uuid,
    admin: String,
}

async fn world() -> World {
    let store = MemStore::new();
    let app = app(&store)
        .with_oauth_issuer(Issuer::parse("https://oml.example.com").expect("a public origin"));
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("acme-admin").expect("a name"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    World {
        router: state(&store, app).into_router(),
        tenant: boot.tenant_id,
        admin: boot.admin_key.value,
        store,
    }
}

fn agent(name: &str, connect_code: bool) -> Value {
    json!({
        "role": "writer",
        "name": name,
        "principal": format!("crew/{name}"),
        "grants": [{"workarea": "universe", "role": "writer"}],
        "connect_code": connect_code,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_issue_resubmitted_replays_without_the_value_and_issues_nothing() {
    let w = world().await;
    let body = agent("agent-1", false);
    let (status, headers, first) = send(&w.router, "/v2/keys", &w.admin, "k-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert!(!replayed(&headers));
    let value = first["value"].as_str().expect("the value, once").to_owned();
    let (status, headers, again) = send(&w.router, "/v2/keys", &w.admin, "k-1", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(replayed(&headers), "the resubmission replays: {again}");
    assert!(
        !again.to_string().contains(&value),
        "a replay holds no value"
    );
    let mut kept = first.clone();
    kept.as_object_mut().unwrap().remove("value");
    assert_eq!(again, kept, "both answers agree but for the value");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_issue_dropped_after_its_commit_never_lets_the_resubmission_issue_again() {
    let w = world().await;
    let body = agent("agent-2", false);
    let park = w.store.park_after_next_commit();
    let first = {
        let (router, admin, body) = (w.router.clone(), w.admin.clone(), body.clone());
        tokio::spawn(async move { send(&router, "/v2/keys", &admin, "k-2", &body).await })
    };
    while !park.reached() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    first.abort();
    assert!(first.await.is_err(), "the request was dropped");
    park.open();
    let (status, headers, again) = send(&w.router, "/v2/keys", &w.admin, "k-2", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(
        replayed(&headers),
        "the resubmission replays the issuance: {again}"
    );
    assert!(again.get("value").is_none(), "{again}");
    let issued = w
        .store
        .resolve_name(w.tenant, NamedKind::Key, &Name::parse("agent-2").unwrap())
        .await
        .unwrap()
        .expect("issued once");
    assert_eq!(again["key_id"], issued.to_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connect_code_resubmitted_replays_without_the_code_and_mints_none() {
    let w = world().await;
    let (status, _, issued) = send(
        &w.router,
        "/v2/keys",
        &w.admin,
        "k-3",
        &agent("agent-3", true),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    assert!(issued["connect_code"]["code"].is_string(), "{issued}");
    let path = "/v2/keys/agent-3/connect-codes";
    let (status, headers, first) = send(&w.router, path, &w.admin, "cc-1", &json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert!(!replayed(&headers));
    let code = first["code"].as_str().expect("the code, once").to_owned();
    let (status, headers, again) = send(&w.router, path, &w.admin, "cc-1", &json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(replayed(&headers), "the resubmission replays: {again}");
    assert!(!again.to_string().contains(&code), "a replay holds no code");
    assert_eq!(
        again["code_id"], first["code_id"],
        "no second code was minted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_used_key_sent_with_another_request_is_digest_mismatch_and_acts_nothing() {
    let w = world().await;
    let (status, _, _) = send(
        &w.router,
        "/v2/keys",
        &w.admin,
        "k-5",
        &agent("agent-5", true),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, refused) = send(
        &w.router,
        "/v2/keys",
        &w.admin,
        "k-5",
        &agent("agent-6", false),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
    assert!(
        w.store
            .resolve_name(w.tenant, NamedKind::Key, &Name::parse("agent-6").unwrap())
            .await
            .unwrap()
            .is_none(),
        "nothing was issued"
    );
    let path = "/v2/keys/agent-5/connect-codes";
    let (status, _, first) = send(&w.router, path, &w.admin, "cc-5", &json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let (status, _, refused) = send(
        &w.router,
        path,
        &w.admin,
        "cc-5",
        &json!({"ttl_seconds": 600}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_admins_under_one_idempotency_key_each_issue_their_own_key() {
    let w = world().await;
    let (status, _, second) = send(
        &w.router,
        "/v2/keys",
        &w.admin,
        "k-admin",
        &json!({"role": "admin", "name": "second-admin", "principal": "bob"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    let second = second["value"].as_str().expect("the value").to_owned();
    let (s1, h1, a1) = send(
        &w.router,
        "/v2/keys",
        &w.admin,
        "same",
        &agent("agent-7", false),
    )
    .await;
    let (s2, h2, a2) = send(
        &w.router,
        "/v2/keys",
        &second,
        "same",
        &agent("agent-8", false),
    )
    .await;
    assert_eq!(
        (s1, s2),
        (StatusCode::CREATED, StatusCode::CREATED),
        "{a1} {a2}"
    );
    assert!(
        !replayed(&h1) && !replayed(&h2),
        "each key's claim is its own"
    );
    assert!(a1["value"].is_string() && a2["value"].is_string());
    assert_ne!(a1["key_id"], a2["key_id"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connector_key_issue_resubmitted_replays_without_its_connect_code() {
    let w = world().await;
    let body = agent("agent-9", true);
    let (status, _, first) = send(&w.router, "/v2/keys", &w.admin, "k-9", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let code = first["connect_code"]["code"]
        .as_str()
        .expect("the code, once")
        .to_owned();
    let (status, headers, again) = send(&w.router, "/v2/keys", &w.admin, "k-9", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(replayed(&headers), "{again}");
    assert!(again["connect_code"].get("code").is_none(), "{again}");
    assert!(!again.to_string().contains(&code), "a replay holds no code");
    assert_eq!(
        again["connect_code"]["code_id"], first["connect_code"]["code_id"],
        "the same code, not a second one"
    );
}
