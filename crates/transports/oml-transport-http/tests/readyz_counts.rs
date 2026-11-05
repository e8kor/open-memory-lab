//! `/readyz` answers anyone, so it carries health and configuration and no
//! count of anyone's traffic: the reranking stages it skipped and the
//! sign-in exchanges that failed are the metrics listener's, as the recall
//! counts are (ADR-0042 d11; ADR-0052 d12; ADR-0054 d1).

mod support;

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::Provider;
use oml_app::domain::{HeldBy, Role};
use oml_app::doubles::{FakeReranker, MemSignIn, MemStore};
use oml_app::ports::RerankError;
use oml_app::search::{Pipeline, SearchConfig};
use oml_app::types::{BootstrapRequest, GrantRequest, IssueKeyRequest};
use oml_transport_http::traffic::{ScopeObserver, Traffic, TrustedProxies, metrics};
use oml_transport_http::web::WebPages;

use support::{app, state};

async fn call(router: &Router, request: Request<Body>) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("UTF-8"))
}

/// Every numeric leaf of `value`, by its dotted path.
fn numbers(value: &Value, path: &str, out: &mut Vec<String>) {
    match value {
        Value::Number(_) => out.push(path.to_owned()),
        Value::Object(map) => {
            for (k, v) in map {
                let next = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                numbers(v, &next, out);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                numbers(v, &format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn unauthenticated_readyz_carries_no_per_pipeline_or_per_tenant_count() {
    let store = MemStore::new().answering_bm25_with_any_word();
    let search = SearchConfig::new(
        &[Pipeline::KeywordAny, Pipeline::Bm25Rerank],
        Pipeline::Bm25Rerank,
    )
    .expect("a served default");
    let app = app(&store)
        .with_observer(Arc::new(ScopeObserver))
        .with_oauth_issuer(Issuer::parse("https://oml.example.com").expect("an origin"))
        .with_search(search)
        .with_reranker(Arc::new(FakeReranker::failing(RerankError::Busy)));
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let grant = |role| GrantRequest {
        node: boot.root_workarea_id.into(),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    };
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
                grants: vec![grant(Role::Writer), grant(Role::Reader)],
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("a key");
    let traffic = Traffic::new(TrustedProxies::default());
    let scrape = metrics::router(Arc::clone(traffic.metrics()));
    let router = state(&store, app)
        .with_traffic(traffic)
        .with_web(WebPages::new(
            vec![(Provider::Google, "google-client".to_owned())],
            MemSignIn::new(),
        ))
        .into_router();

    let post = |path: &str, body: Value, idem: Option<&str>| {
        let mut request = Request::post(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", writer.value))
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(idem) = idem {
            request = request.header("idempotency-key", idem);
        }
        request
            .body(Body::from(body.to_string()))
            .expect("a request")
    };
    let (status, _) = call(
        &router,
        post(
            "/v1/memories",
            json!({"body": "the deploy runs on fridays"}),
            Some("i-1"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    // The own held row is a candidate, so the busy reranker skips a stage.
    let (status, body) = call(
        &router,
        post(
            "/v1/memories/recall",
            json!({"query": "deploy fridays", "include_unreviewed": true}),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let recalled: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(recalled["search"]["rerank_reason"], "busy", "{body}");

    let (status, body) = call(
        &router,
        Request::get("/readyz")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ready: Value = serde_json::from_str(&body).expect("JSON");
    // Configuration a doctor needs is a number too: the reranker's depth
    // and floor. Nothing else is.
    let mut found = Vec::new();
    numbers(&ready, "", &mut found);
    assert_eq!(
        found,
        vec!["search.rerank.depth".to_owned()],
        "readyz carries a count:\n{body}"
    );
    for word in ["skipped", "failed_exchanges", "last_failure", "recalls"] {
        assert!(!body.contains(word), "readyz names {word}:\n{body}");
    }
    assert_eq!(ready["web"]["providers"], json!(["google"]), "{body}");

    // The skipped stage is counted where only the operator reads it.
    let (_, text) = call(
        &scrape,
        Request::get("/metrics")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert!(
        text.contains(r#"oml_reranks_total{outcome="skipped",reason="busy"} 1"#),
        "{text}"
    );
}
