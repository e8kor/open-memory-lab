//! `oml-web`'s readiness (ADR-0070 d4): `GET /readyz` is ready when the API
//! it is a client of answers its own `/readyz` as ready and names a web
//! client identity, so a probe, a compose check or a Kubernetes readiness
//! probe sees the hop and not only the process.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use http::{Request, Response, StatusCode, header};
use oml_sdk::{Sending, Transport, TransportError};
use oml_web::middleware::body_bytes;
use oml_web::{Layers, READY_PATH, Web, api, router};
use serde_json::{Value, json};
use tower::ServiceExt as _;

const SECRET: &str = "0123456789abcdef0123456789abcdef";

type Seen = Arc<Mutex<Vec<Request<Vec<u8>>>>>;

/// The API's `/readyz` answer: a status and a body, or no answer at all.
struct Ready(Option<(u16, Value)>, Seen);

impl Transport for Ready {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
        self.1.lock().unwrap().push(request);
        let answer = self.0.clone();
        Box::pin(async move {
            match answer {
                Some((status, body)) => Ok(Response::builder()
                    .status(status)
                    .body(body.to_string().into_bytes())
                    .unwrap()),
                None => Err(TransportError::Unreachable("connection refused".to_owned())),
            }
        })
    }
}

fn lead(status: &str, web_client: &str) -> Value {
    json!({
        "status": status,
        "checks": {"postgres": "ok", "fork_check": "ok"},
        "oauth_issuer": null,
        "web_client": web_client,
        "build": {"version": "0.6.3"},
    })
}

async fn probe(answer: Option<(u16, Value)>) -> (StatusCode, Value, Vec<Request<Vec<u8>>>) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let app = router(Web {
        layers: Layers {
            public_origin: Some("https://memory.example".to_owned()),
            log: Arc::new(|_| {}),
        },
        api: api(Arc::new(Ready(answer, seen.clone())), SECRET).unwrap(),
    });
    let response = app
        .oneshot(Request::get(READY_PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = serde_json::from_slice(&body_bytes(response.into_body()).await.unwrap()).unwrap();
    let calls = seen.lock().unwrap().drain(..).collect();
    (status, body, calls)
}

#[tokio::test]
async fn readyz_is_ready_when_the_api_is_ready_and_names_a_web_client_identity() {
    let (status, body, calls) = probe(Some((200, lead("ok", "secret")))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "oml-web");
    assert_eq!(body["checks"]["api"], "ok");
    assert_eq!(body["checks"]["web_client"], "secret");
    // One unauthenticated call: no session, no credential crosses for it.
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].uri(), "/readyz");
    assert!(!calls[0].headers().contains_key(header::AUTHORIZATION));
}

#[tokio::test]
async fn readyz_is_unavailable_when_the_api_is_unreachable_unready_or_has_no_web_client_identity() {
    for (answer, check, why) in [
        (None, "api", "unreachable"),
        (Some((503, lead("unavailable", "secret"))), "api", "unavailable"),
        (Some((200, lead("ok", "none"))), "web_client", "none"),
    ] {
        let (status, body, _) = probe(answer).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["status"], "unavailable", "{body}");
        assert!(
            body["checks"][check].as_str().unwrap_or("").contains(why),
            "{check}: {body}"
        );
    }
}
