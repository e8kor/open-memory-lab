//! `GET /v2/whoami`'s `shared` (ADR-0076 d9): whether the server's admins
//! are not the caller's, which `oml sync` asks consent on. Over the
//! in-memory doubles; each test is named after the sentence it proves.

mod support;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt as _;

use oml_app::domain::name::Name;
use oml_app::doubles::MemStore;
use oml_app::types::{BootstrapRequest, SignupPolicy};

use support::{TestApp, app, state};

/// The whoami of a freshly bootstrapped admin key on the app `compose`
/// builds over a fresh store.
async fn whoami_of(compose: impl Fn(TestApp) -> TestApp) -> Value {
    let store = MemStore::new();
    let boot = compose(app(&store))
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("oml").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "operator".to_owned(),
        })
        .await
        .expect("bootstrap");
    let router = state(&store, compose(app(&store))).into_router();
    let response = router
        .oneshot(
            Request::get("/v2/whoami")
                .header("authorization", format!("Bearer {}", boot.admin_key.value))
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("the router answers");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    serde_json::from_slice(&body).expect("JSON")
}

fn open_signup() -> SignupPolicy {
    SignupPolicy {
        mode: oml_app::domain::signup::SignupMode::Open,
        tenant_id: uuid::Uuid::nil(),
        issuer: Name::parse("root-admin").expect("a name"),
        terms_version: "1".to_owned(),
        terms_url: "https://oml.example.com/terms".to_owned(),
        privacy_url: "https://oml.example.com/privacy".to_owned(),
        per_ip_per_hour: 10,
        quotas: oml_app::domain::signup::Quotas::default(),
    }
}

#[tokio::test]
async fn whoami_says_shared_when_server_shared_is_set_or_sign_up_is_open() {
    assert_eq!(whoami_of(|app| app).await["shared"], Value::Bool(false));
    assert_eq!(
        whoami_of(|app| app.with_signup(open_signup())).await["shared"],
        Value::Bool(true),
        "open sign-up admits people who are not the operator's"
    );
    assert_eq!(
        whoami_of(|app| app.with_shared(true)).await["shared"],
        Value::Bool(true),
        "server.shared"
    );
}
