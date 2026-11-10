//! The pooled HTTP transport against a real listener on an ephemeral port
//! (ADR-0070 d8): calls reuse one kept-alive connection, and a call past its
//! timeout fails as a timeout rather than waiting.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use axum::serve::ListenerExt as _;
use oml_sdk::{Call, Client, Error, HttpTransport, TransportError};

const CALL: Call<'static> = Call {
    bearer: "omls_x",
    request_id: None,
    correlation_id: None,
};

/// A server answering `/v2/whoami` at once and `/v1/guardrails` after two
/// seconds, and the number of connections it has accepted.
async fn server() -> (String, Arc<AtomicUsize>) {
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = accepted.clone();
    let app = Router::new()
        .route(
            "/v2/whoami",
            get(|| async {
                axum::Json(serde_json::json!({
                    "key_id": "k", "name": "alice-web", "role": "user",
                    "tenant": {"id": "t", "name": "oml"}, "user": null,
                }))
            }),
        )
        .route(
            "/v1/guardrails",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                "late"
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let listener = listener.tap_io(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    tokio::spawn(async move { axum::serve(listener, app).await });
    (format!("http://{address}"), accepted)
}

#[tokio::test]
async fn sequential_calls_reuse_one_kept_alive_connection() {
    let (base, accepted) = server().await;
    let client = Client::new(HttpTransport::new(&base, Duration::from_secs(5)).unwrap());
    for _ in 0..5 {
        let whoami = client.whoami(&CALL).await.unwrap();
        assert_eq!(whoami.tenant.unwrap().name, "oml");
    }
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_call_past_its_timeout_fails_as_a_timeout() {
    let (base, _) = server().await;
    let client = Client::new(HttpTransport::new(&base, Duration::from_millis(200)).unwrap());
    let started = std::time::Instant::now();
    let answer = client.guardrails(&CALL, None).await;
    assert_eq!(answer, Err(Error::Transport(TransportError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn an_unreachable_api_is_a_transport_failure() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let client = Client::new(
        HttpTransport::new(&format!("http://{address}"), Duration::from_secs(2)).unwrap(),
    );
    assert!(matches!(
        client.whoami(&CALL).await,
        Err(Error::Transport(TransportError::Unreachable(_)))
    ));
}
