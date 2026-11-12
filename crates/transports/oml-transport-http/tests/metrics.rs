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

use oml_app::domain::access::Covers;
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
                profile: None,
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
                    covers: Covers::Subtree,
                }],
                connect_code: false,
                user: None,
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

/// Recall counts the meter's queue dropped or could not write are a metric
/// as well as a log event, so an alert can read them (ADR-0071 d8).
#[tokio::test]
async fn recall_counts_that_were_lost_are_counted_on_the_metrics_listener() {
    let traffic = Traffic::new(TrustedProxies::default());
    let scrape = metrics::router(Arc::clone(traffic.metrics()));
    let (_, text) = get(&scrape, "/metrics").await;
    assert!(
        text.contains("oml_usage_recall_counts_lost_total 0"),
        "{text}"
    );
    traffic.metrics().recall_counts_lost(3);
    traffic.metrics().recall_counts_lost(2);
    let (_, text) = get(&scrape, "/metrics").await;
    assert!(
        text.contains("oml_usage_recall_counts_lost_total 5"),
        "{text}"
    );
}

/// A web console act that committed and whose outcome was not recorded is
/// answered and counted in `oml_idempotency_unsettled_total`, the count
/// doctor's `handlers` line warns on (#233).
#[tokio::test]
async fn an_act_whose_outcome_is_not_recorded_is_counted_unsettled() {
    let store = MemStore::new();
    let app = app(&store);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let traffic = Traffic::new(TrustedProxies::default());
    let scrape = metrics::router(Arc::clone(traffic.metrics()));
    let router = state(&store, app).with_traffic(traffic).into_router();
    store.fail_next_idempotency_settle();
    let response = router
        .clone()
        .oneshot(
            Request::post("/v2/workareas")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", boot.admin_key.value),
                )
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "c-1")
                .body(Body::from(
                    json!({"name": "lost", "parent": "universe"}).to_string(),
                ))
                .expect("a request"),
        )
        .await
        .expect("the router answers");
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "the act is answered"
    );
    let (_, text) = get(&scrape, "/metrics").await;
    assert!(text.contains("oml_idempotency_unsettled_total 1"), "{text}");
}

/// A mode change made over HTTP, which the console's act runs in a task of
/// its own, is counted in `oml_workarea_mode_changes_total`, and its request
/// line names the workarea the act resolved (ADR-0042 d2; ADR-0030 d13;
/// #233).
#[tokio::test]
async fn a_mode_change_over_http_is_counted_and_its_request_line_names_the_workarea() {
    let store = MemStore::new();
    let app = app(&store).with_observer(Arc::new(ScopeObserver));
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let draft = app
        .create_workarea(
            &boot.admin_key.value,
            &Name::parse("draft").expect("a name"),
            Some(boot.root_workarea_id.into()),
            None,
        )
        .await
        .expect("a workarea")
        .workarea
        .workarea_id;
    let ann = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Dreamer,
                name: Name::parse("ann-review").expect("a name"),
                principal: "ann".to_owned(),
                held_by: Some(HeldBy::Human),
                default_workarea: None,
                crew: None,
                grants: [Role::Dreamer, Role::Reader]
                    .map(|role| GrantRequest {
                        node: draft.into(),
                        role,
                        deny: Vec::new(),
                        covers: Covers::Subtree,
                    })
                    .to_vec(),
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("ann's key")
        .value;
    let traffic = Traffic::new(TrustedProxies::default());
    let scrape = metrics::router(Arc::clone(traffic.metrics()));
    let router = state(&store, app).with_traffic(traffic).into_router();
    let (guard, captured) = support::Captured::start();
    let response = router
        .clone()
        .oneshot(
            Request::post(format!("/v2/workareas/{draft}/mode"))
                .header(header::AUTHORIZATION, format!("Bearer {ann}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "m-1")
                .body(Body::from(json!({"mode": "in_progress"}).to_string()))
                .expect("a request"),
        )
        .await
        .expect("the router answers");
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    drop(guard);
    let (_, text) = get(&scrape, "/metrics").await;
    assert!(
        text.contains(r#"oml_workarea_mode_changes_total{mode="in_progress"} 1"#),
        "{text}"
    );
    let lines = captured.events("http_request");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["workarea_id"], draft.to_string(), "{lines:?}");
}
