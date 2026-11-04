//! `/metrics` is served on a listener of its own and never on the API
//! listener; v0.4.1's four families carry closed labels and no tenant, and
//! the histograms' buckets run from 1 ms to 32.768 s (ADR-0042 d11, d12).

mod support;

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::json;
use tower::ServiceExt as _;

use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::{HeldBy, Role};
use oml_app::doubles::MemStore;
use oml_app::types::{BootstrapRequest, GrantRequest, IssueKeyRequest};
use oml_transport_http::traffic::{ScopeObserver, Traffic, TrustedProxies, metrics};

use support::{app, state};

async fn get(router: &Router, path: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).expect("a request"))
        .await
        .expect("the router answers");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("UTF-8"))
}

#[tokio::test]
async fn metrics_answers_404_on_the_api_listener() {
    let store = MemStore::new();
    let router = state(&store, app(&store)).into_router();
    let (status, body) = get(&router, "/metrics").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        !body.contains("oml_http_request_duration_seconds"),
        "{body}"
    );
}

#[tokio::test]
async fn the_metrics_listener_serves_the_four_families_with_closed_labels_and_no_tenant() {
    let store = MemStore::new();
    let app = app(&store)
        .with_observer(Arc::new(ScopeObserver))
        .with_oauth_issuer(Issuer::parse("https://oml.example.com").expect("an origin"));
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let writer = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                role: Role::Writer,
                name: Name::parse("agent-7").expect("a name"),
                principal: "crew/agent-7".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: Some(boot.root_workarea_id.into()),
                crew: None,
                grants: vec![GrantRequest {
                    node: boot.root_workarea_id.into(),
                    role: Role::Writer,
                    deny: Vec::new(),
                }],
                connect_code: false,
                guardrail_scope: None,
            },
        )
        .await
        .expect("a key");
    let traffic = Traffic::new(TrustedProxies::default());
    let scrape = metrics::router(Arc::clone(traffic.metrics()));
    let router = state(&store, app).with_traffic(traffic).into_router();

    let submit = |idem: &str, body: &str| {
        Request::post("/v1/memories")
            .header(header::AUTHORIZATION, format!("Bearer {}", writer.value))
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", idem)
            .body(Body::from(json!({ "body": body }).to_string()))
            .expect("a request")
    };
    for request in [
        submit("idem-a", "one fact"),
        submit("idem-a", "one fact"),
        submit("idem-a", "another fact"),
        Request::get("/healthz")
            .body(Body::empty())
            .expect("a request"),
        Request::post("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "grant_type=refresh_token&refresh_token=x&client_id=y",
            ))
            .expect("a request"),
        Request::post("/mcp")
            .header(header::AUTHORIZATION, format!("Bearer {}", writer.value))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": {"name": "oml_read", "arguments": {"query": "fact"}}})
                .to_string(),
            ))
            .expect("a request"),
    ] {
        router
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
    }

    let (status, text) = get(&scrape, "/metrics").await;
    assert_eq!(status, StatusCode::OK);
    for expected in [
        r#"oml_http_request_duration_seconds_bucket{le="0.001",route="/v1/memories",status="202"}"#,
        r#"{le="32.768",route="/v1/memories",status="202"}"#,
        r#"{le="+Inf",route="/v1/memories",status="202"}"#,
        r#"oml_mcp_call_duration_seconds_count{tool="oml_read",mode="recall",outcome="tool_error"} 1"#,
        "oml_idempotency_replays_total 1",
        "oml_idempotency_conflicts_total 1",
        r#"oml_oauth_token_requests_total{grant_type="refresh_token",outcome="invalid_grant"} 1"#,
    ] {
        assert!(text.contains(expected), "no {expected} in:\n{text}");
    }
    assert!(
        !text.contains("/healthz"),
        "a probe answered 200 is not counted"
    );
    for leak in ["tenant", &writer.key_id.to_string(), "agent-7", "crew/"] {
        assert!(!text.contains(leak), "{leak} in:\n{text}");
    }
    let bounds = text
        .lines()
        .filter(|l| {
            l.starts_with("oml_http_request_duration_seconds_bucket{")
                && l.contains(r#"route="/v1/memories",status="202"}"#)
        })
        .count();
    assert_eq!(bounds, 17, "sixteen doubling bounds and +Inf");
    let (status, _) = get(&scrape, "/other").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
