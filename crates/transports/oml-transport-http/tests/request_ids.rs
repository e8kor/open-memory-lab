//! Every response returns the request's id under one rule, and the error
//! envelope's `request_id` is that id: an inbound `X-Request-Id` the rule
//! admits comes back unchanged, any other is replaced, and the routes no
//! handler finishes carry it too (ADR-0042 d4–d6).

mod support;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt as _;

use support::router;

async fn send(req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = router()
        .await
        .oneshot(req)
        .await
        .expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, body)
}

fn header<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .unwrap_or_else(|| panic!("{name} is on every response"))
        .to_str()
        .expect("ASCII")
}

#[tokio::test]
async fn an_id_inside_the_rule_comes_back_unchanged_in_the_header_and_the_envelope() {
    let (status, headers, body) = send(
        Request::get("/v2/whoami")
            .header("x-request-id", "01HZX3K9V7Q2M4N6P8R0S2T4V6")
            .header("x-correlation-id", "task:42")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        header(&headers, "x-request-id"),
        "01HZX3K9V7Q2M4N6P8R0S2T4V6"
    );
    assert_eq!(body["request_id"], "01HZX3K9V7Q2M4N6P8R0S2T4V6");
    assert_eq!(header(&headers, "x-correlation-id"), "task:42");
}

#[tokio::test]
async fn an_id_outside_the_rule_is_replaced_by_a_minted_one_the_envelope_repeats() {
    let (_, headers, body) = send(
        Request::get("/v2/whoami")
            .header("x-request-id", "not an id")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    let id = header(&headers, "x-request-id");
    assert!(id.starts_with("req_") && id.len() == 36, "minted: {id}");
    assert_eq!(body["request_id"], id);
    assert_eq!(header(&headers, "x-correlation-id"), id);
}

#[tokio::test]
async fn axum_own_404_and_405_carry_the_request_and_correlation_ids() {
    for req in [
        Request::get("/no/such/path"),
        Request::delete("/v1/memories"),
    ] {
        let (status, headers, _) = send(
            req.header("x-request-id", "probe-1")
                .body(Body::empty())
                .expect("a request"),
        )
        .await;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
            "{status}"
        );
        assert_eq!(header(&headers, "x-request-id"), "probe-1");
        assert_eq!(header(&headers, "x-correlation-id"), "probe-1");
    }
}

#[tokio::test]
async fn the_envelope_request_id_equals_the_header_on_every_route() {
    for &(method, path) in oml_transport_http::ROUTES {
        let path = path
            .replace("{artifact_id}", "0b5f6c3e-5d0b-4d8f-9a5e-2f1d7c9e8a10")
            .replace("{key}", "agent-7")
            .replace("{connection_id}", "0b5f6c3e-5d0b-4d8f-9a5e-2f1d7c9e8a11")
            .replace("{crew}", "crew-a")
            .replace("{workarea}", "area-a");
        let (status, headers, body) = send(
            Request::builder()
                .method(method)
                .uri(&path)
                .body(Body::empty())
                .expect("a request"),
        )
        .await;
        let id = header(&headers, "x-request-id");
        if status.is_client_error() || status.is_server_error() {
            assert_eq!(body["request_id"], id, "{method} {path}");
        }
    }
}
