//! What crosses the hop for one page (ADR-0070 d2, d3, d8), against an API
//! double that records every call: the session's derived credential and
//! the web client's identity on each call, the page's request id as every
//! call's correlation, a numbered request id per call, the page's line
//! naming them, and the page's outcome when a call fails.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use http::{HeaderMap, Request, Response, StatusCode, header};
use oml_sdk::{Sending, Transport};
use oml_web::middleware::body_bytes;
use oml_web::{Layers, SESSION_COOKIE, Web, api, router};
use oml_web_credential::capacity_credential;
use serde_json::{Value, json};
use tower::ServiceExt as _;

const SECRET: &str = "0123456789abcdef0123456789abcdef";
const TOKEN: &str = "the-session-token";
const ROOT: &str = "6f1c2d4e-0000-4000-8000-000000000001";

/// The answers the double gives, by path; `None` answers a 500 envelope.
#[derive(Clone)]
struct Answers {
    whoami: Option<Value>,
    guardrails: Option<Value>,
    tree: Option<Value>,
}

#[derive(Clone)]
struct Double {
    answers: Answers,
    seen: Arc<Mutex<Vec<(String, HeaderMap)>>>,
}

fn json_response(status: u16, body: &Value) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .body(body.to_string().into_bytes())
        .unwrap()
}

fn envelope(code: &str) -> Value {
    json!({"error": {"code": code, "message": format!("{code} here")}, "request_id": "req_api"})
}

impl Transport for Double {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
        let path = request.uri().to_string();
        self.seen
            .lock()
            .unwrap()
            .push((path.clone(), request.headers().clone()));
        let bearer = request.headers()[header::AUTHORIZATION]
            .to_str()
            .unwrap()
            .to_owned();
        let web = format!("Bearer {}", capacity_credential(TOKEN, "web"));
        let answer = if path == "/v2/whoami?person=false" {
            match (&self.answers.whoami, bearer == web) {
                (Some(me), true) => json_response(200, me),
                _ => json_response(401, &envelope("auth.invalid")),
            }
        } else if path.starts_with("/v1/guardrails") {
            self.answers.guardrails.as_ref().map_or_else(
                || json_response(500, &envelope("internal.error")),
                |set| json_response(200, set),
            )
        } else {
            self.answers.tree.as_ref().map_or_else(
                || json_response(503, &envelope("unavailable.read_only")),
                |page| json_response(200, page),
            )
        };
        Box::pin(async move { Ok(answer) })
    }
}

fn whoami() -> Value {
    json!({
        "key_id": "k1", "name": "alice-web", "role": "user", "principal": "alice",
        "held_by": "human", "tenant": {"id": "t1", "name": "oml"},
        "default_workarea": null, "crew": null,
        "user": {"id": ROOT, "name": "alice"}, "grants": [],
    })
}

fn set() -> Value {
    json!({
        "scope_node": ROOT, "scope_path": "universe.alice", "guardrail_scope": "ancestors",
        "set_digest": "d", "over_cap": false, "guardrails": [], "own_held": [],
    })
}

fn tree() -> Value {
    json!({"items": [{"id": ROOT, "name": "alice", "path": "universe.alice"}], "next_cursor": null})
}

type Lines = Arc<Mutex<Vec<String>>>;

async fn page(
    answers: Answers,
) -> (
    StatusCode,
    HeaderMap,
    String,
    Vec<(String, HeaderMap)>,
    Lines,
) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let lines: Lines = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    let double = Double {
        answers,
        seen: seen.clone(),
    };
    let app = router(Web {
        layers: Layers {
            public_origin: Some("https://memory.example".to_owned()),
            log: Arc::new(move |line| sink.lock().unwrap().push(line)),
        },
        api: api(Arc::new(double), SECRET).unwrap(),
    });
    let response = app
        .oneshot(
            Request::get("/web/guardrails")
                .header("x-request-id", "page-42")
                .header(header::COOKIE, format!("{SESSION_COOKIE}={TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = body_bytes(response.into_body()).await.unwrap();
    let calls = seen.lock().unwrap().clone();
    (
        status,
        headers,
        String::from_utf8_lossy(&body).into_owned(),
        calls,
        lines,
    )
}

fn all() -> Answers {
    Answers {
        whoami: Some(whoami()),
        guardrails: Some(set()),
        tree: Some(tree()),
    }
}

#[tokio::test]
async fn each_api_call_of_a_page_carries_the_pages_id_as_its_correlation_and_a_numbered_request_id()
{
    let (status, _, page, calls, lines) = page(all()).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("Rules in force at <code>universe.alice</code>"),
        "{page}"
    );
    assert!(page.contains("alice · oml"), "{page}");
    // whoami as web and as admin, then the set and the tree: four calls.
    assert_eq!(calls.len(), 4, "{calls:?}");
    let mut ids: Vec<String> = calls
        .iter()
        .map(|(_, h)| h["x-request-id"].to_str().unwrap().to_owned())
        .collect();
    ids.sort();
    assert_eq!(ids, ["page-42.1", "page-42.2", "page-42.3", "page-42.4"]);
    for (path, h) in &calls {
        assert_eq!(h["x-correlation-id"], "page-42", "{path}");
        assert_eq!(h["oml-web-client"], SECRET, "{path}");
        let bearer = h[header::AUTHORIZATION].to_str().unwrap();
        assert!(bearer.starts_with("Bearer omls_"), "{path}");
        assert!(!bearer.contains(TOKEN), "the cookie never crosses: {path}");
    }
    let line: Value = serde_json::from_str(&lines.lock().unwrap()[0]).unwrap();
    assert_eq!(line["request_id"], "page-42");
    assert_eq!(line["credential"], "session");
    assert_eq!(line["key"], "alice-web");
    assert_eq!(line["api_request_ids"].as_array().unwrap().len(), 4);
    assert!(!lines.lock().unwrap()[0].contains(TOKEN));
}

#[tokio::test]
async fn the_admin_tab_shows_only_where_the_admin_credential_resolves() {
    // The double resolves only the web credential, as for a non-admin.
    let (_, _, page, _, _) = page(all()).await;
    assert!(!page.contains("Administration"), "{page}");
}

#[tokio::test]
async fn a_failed_tree_renders_the_guardrails_without_the_selector_naming_the_calls_request_id() {
    let (status, _, page, _, _) = page(Answers {
        tree: None,
        ..all()
    })
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("No guardrail applies here."), "{page}");
    assert!(!page.contains("<select name=\"workarea\">"), "{page}");
    assert!(
        page.contains("The workarea list is unavailable (request <code>page-42."),
        "{page}"
    );
}

#[tokio::test]
async fn a_failed_set_is_the_error_page_naming_the_pages_request_id() {
    let (status, _, page, _, _) = page(Answers {
        guardrails: None,
        ..all()
    })
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{page}");
    assert!(page.contains("internal.error"), "{page}");
    assert!(page.contains("Request <code>page-42</code>"), "{page}");
}

#[tokio::test]
async fn auth_invalid_from_whoami_sends_the_person_to_sign_in() {
    let (status, headers, _, calls, _) = page(Answers {
        whoami: None,
        ..all()
    })
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION],
        "/web/sign-in?next=%2Fweb%2Fguardrails"
    );
    assert!(
        calls.iter().all(|(p, _)| p == "/v2/whoami?person=false"),
        "{calls:?}"
    );
}

#[tokio::test]
async fn a_tree_cursor_the_api_repeats_ends_the_listing_instead_of_looping() {
    let looping = json!({
        "items": [{"id": ROOT, "name": "alice", "path": "universe.alice"}],
        "next_cursor": "same",
    });
    let (status, _, page, calls, _) = page(Answers {
        tree: Some(looping),
        ..all()
    })
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(!page.contains("<select name=\"workarea\">"), "{page}");
    assert!(page.contains("The workarea list is unavailable"), "{page}");
    let tree_calls = calls.iter().filter(|(p, _)| p.contains("/tree")).count();
    assert_eq!(
        tree_calls, 2,
        "the second page repeats the first's cursor: {calls:?}"
    );
}
