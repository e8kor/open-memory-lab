//! A handler that panics answers the request with a named 500 and writes one
//! log line and one count; the worker lives and the next request is served.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use serde_json::Value;
use tower::ServiceExt as _;

use oml_transport_http::traffic::{self, Traffic, TrustedProxies};

fn router(traffic: &Traffic) -> Router {
    let routes = Router::new()
        .route("/boom", get(|| async { panic!("a handler defect") }))
        .route("/fine", get(|| async { "fine" }));
    traffic::guard(routes, traffic.clone())
}

async fn send(router: &Router, path: &str) -> (StatusCode, Option<String>, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).expect("a request"))
        .await
        .expect("the router answers");
    let status = response.status();
    let id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body")
        .to_vec();
    (status, id, body)
}

#[tokio::test]
async fn a_handler_panic_is_a_named_500_with_the_request_id_and_the_worker_serves_the_next() {
    let traffic = Traffic::new(TrustedProxies::default());
    let router = router(&traffic);
    let (status, id, body) = send(&router, "/boom").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let envelope: Value = serde_json::from_slice(&body).expect("the error envelope");
    assert_eq!(envelope["error"]["code"], "system.internal");
    assert_eq!(envelope["error"]["details"]["reason"], "handler_panic");
    assert_eq!(
        Some(envelope["request_id"].as_str().expect("an id").to_owned()),
        id,
        "the body's id is the header's"
    );
    assert!(
        !String::from_utf8_lossy(&body).contains("a handler defect"),
        "the panic's text stays in the log: {envelope}"
    );
    let (status, _, body) = send(&router, "/fine").await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, &b"fine"[..]));
    let scrape = traffic.metrics().encode();
    assert!(
        scrape.contains("oml_handler_panics_total 1"),
        "one panic counted: {scrape}"
    );
    let _ = header::CONTENT_TYPE;
}
