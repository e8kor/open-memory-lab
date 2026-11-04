//! The request line's `store_ms` and `store_calls` are the wall time and
//! count of the request's calls into the store ports, timed at the port
//! boundary the lead composes (ADR-0042 d14). Over the in-memory store, so
//! it runs without Postgres.

use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::App;
use oml_app::domain::name::Name;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::types::BootstrapRequest;
use oml_server_lead::log_subscriber;
use oml_server_lead::timed::Timed;
use oml_transport_http::ApiState;
use oml_transport_http::traffic::ScopeObserver;

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("unpoisoned").extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_request_line_sums_the_store_port_calls_it_made() {
    let store = Timed::new(MemStore::new());
    let app = App::new(
        store.clone(),
        MemSigner::new([3; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_observer(Arc::new(ScopeObserver));
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap, outside any request");
    let router = ApiState::new(
        Arc::new(app),
        store,
        Arc::new(|| Box::pin(async { true })),
        true,
        None,
    )
    .into_router();

    let buf = Buf::default();
    let sink = buf.clone();
    let guard = tracing::subscriber::set_default(log_subscriber(move || sink.clone()));
    let response = router
        .oneshot(
            Request::post("/v1/memories")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", boot.admin_key.value),
                )
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "idem-1")
                .body(Body::from(json!({"body": "a fact"}).to_string()))
                .expect("a request"),
        )
        .await
        .expect("the router answers");
    let status = response.status();
    to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    drop(guard);
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an admin key does not submit"
    );

    let text = String::from_utf8(buf.0.lock().expect("unpoisoned").clone()).expect("UTF-8");
    let line: Value = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).expect("a JSON line"))
        .find(|l| l["event"] == "http_request")
        .expect("the request line");
    let calls = line["store_calls"].as_u64().expect("a count");
    assert!(calls >= 1, "the key lookup at least: {line}");
    assert!(
        line["store_ms"].as_f64().is_some_and(|ms| ms >= 0.0),
        "{line}"
    );
    assert!(line["auth_ms"].is_number(), "{line}");
}
