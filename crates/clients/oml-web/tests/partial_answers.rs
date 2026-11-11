//! What a page answers when one of its API calls fails or answers past a
//! bound (ADR-0070 d8), against an API double that answers each path as a
//! test says: the Atlas and its JSON when the offers fail, the JSON route
//! when the listing fails, a verdict whose next claim fails, the list past
//! the pages of keys it reads, and a key issue whose answer carries no
//! value (#253).

use std::sync::{Arc, Mutex};

use axum::body::Body;
use http::{HeaderMap, Method, Request, Response, StatusCode, header};
use oml_sdk::{Sending, Transport};
use oml_web::middleware::body_bytes;
use oml_web::{Layers, SESSION_COOKIE, Web, api, router};
use oml_web_credential::{capacity_credential, csrf_token};
use serde_json::{Value, json};
use tower::ServiceExt as _;

const SECRET: &str = "0123456789abcdef0123456789abcdef";
const TOKEN: &str = "the-session-token";
const ORIGIN: &str = "https://memory.example";
const ROOT: &str = "6f1c2d4e-0000-4000-8000-000000000001";

/// One answer: the status, the body and any extra headers.
type Answer = (u16, Value, Vec<(&'static str, &'static str)>);

/// The double's answers past whoami, by method and path with query.
type Script = Arc<dyn Fn(&Method, &str) -> Answer + Send + Sync>;

struct Double {
    script: Script,
    seen: Arc<Mutex<Vec<String>>>,
}

fn envelope(code: &str) -> Value {
    json!({"error": {"code": code, "message": format!("{code} here")}, "request_id": "req_api"})
}

fn whoami() -> Value {
    json!({
        "key_id": "k1", "name": "alice-web", "role": "user", "principal": "alice",
        "held_by": "human", "tenant": {"id": "t1", "name": "oml"},
        "default_workarea": null, "crew": null,
        "user": {"id": ROOT, "name": "alice"}, "grants": [],
    })
}

impl Transport for Double {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
        let path = request.uri().to_string();
        self.seen
            .lock()
            .unwrap()
            .push(format!("{} {path}", request.method()));
        let bearer = request.headers()[header::AUTHORIZATION]
            .to_str()
            .unwrap()
            .to_owned();
        // Only the web credential resolves, as for a person who is no admin.
        let (status, body, extra) = if path.starts_with("/v2/whoami") {
            if bearer == format!("Bearer {}", capacity_credential(TOKEN, "web")) {
                (200, whoami(), Vec::new())
            } else {
                (401, envelope("auth.invalid"), Vec::new())
            }
        } else {
            (self.script)(request.method(), &path)
        };
        let mut response = Response::builder().status(status);
        for (name, value) in extra {
            response = response.header(name, value);
        }
        let response = response.body(body.to_string().into_bytes()).unwrap();
        Box::pin(async move { Ok(response) })
    }
}

/// A page's answer and every call it made.
struct Answered {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
    calls: Vec<String>,
}

async fn send(script: Script, request: Request<Body>) -> Answered {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = router(Web {
        layers: Layers {
            public_origin: Some(ORIGIN.to_owned()),
            log: Arc::new(|_| {}),
        },
        api: api(
            Arc::new(Double {
                script,
                seen: seen.clone(),
            }),
            SECRET,
        )
        .unwrap(),
    });
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = body_bytes(response.into_body()).await.unwrap();
    let calls = seen.lock().unwrap().clone();
    Answered {
        status,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
        calls,
    }
}

fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header("x-request-id", "page-7")
        .header(header::COOKIE, format!("{SESSION_COOKIE}={TOKEN}"))
        .body(Body::empty())
        .unwrap()
}

fn post(path: &str, fields: &str) -> Request<Body> {
    Request::post(path)
        .header("x-request-id", "page-7")
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, format!("{SESSION_COOKIE}={TOKEN}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "csrf_token={}&{fields}",
            csrf_token(TOKEN)
        )))
        .unwrap()
}

fn empty_page() -> Value {
    json!({"items": [], "next_cursor": null})
}

fn counts() -> Value {
    json!({
        "items": [{
            "id": ROOT, "name": "alice", "parent_id": null, "path": "universe.alice",
            "status": "development", "mode": "standard",
            "counts": {"promoted": 2, "held": 1, "in_progress": 0, "other": 0},
        }],
        "truncated": false,
    })
}

/// The list's and the Atlas's reads, the offers refused with a 503.
fn offers_fail() -> Script {
    Arc::new(|_, path| {
        if path.contains("/offers") {
            (503, envelope("system.unavailable"), Vec::new())
        } else if path.contains("tree?counts=true") {
            (200, counts(), Vec::new())
        } else {
            (200, empty_page(), Vec::new())
        }
    })
}

#[tokio::test]
async fn the_atlas_draws_its_regions_and_marks_the_panel_unavailable_when_the_offers_fail() {
    let page = send(offers_fail(), get("/web/atlas")).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("universe.alice"), "{}", page.body);
    assert!(
        page.body
            .contains("The acts are unavailable (request <code>page-7."),
        "{}",
        page.body
    );
}

#[tokio::test]
async fn the_atlas_json_serves_the_regions_and_marks_the_offers_unavailable_when_they_fail() {
    let answer = send(offers_fail(), get("/web/data/memories")).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let value: Value = serde_json::from_str(&answer.body).unwrap();
    let scope = &value["workareas"]["items"][0];
    assert_eq!(scope["path"], "universe.alice", "{value}");
    assert_eq!(scope["counts"]["approved"], 2, "{value}");
    assert_eq!(scope["offers_unavailable"], true, "{value}");
    assert!(scope.get("offers").is_none(), "{value}");
}

#[tokio::test]
async fn the_json_route_answers_the_apis_refusal_as_json_when_the_listing_fails() {
    let script: Script = Arc::new(|_, path| {
        if path.starts_with("/v1/memories") {
            (503, envelope("system.unavailable"), Vec::new())
        } else if path.contains("tree?counts=true") {
            (200, counts(), Vec::new())
        } else {
            (200, empty_page(), Vec::new())
        }
    });
    let answer = send(script, get("/web/data/memories")).await;
    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        answer.headers[header::CONTENT_TYPE],
        "application/json",
        "{}",
        answer.body
    );
    let value: Value = serde_json::from_str(&answer.body).unwrap();
    assert_eq!(value["error"]["code"], "system.unavailable", "{value}");
    assert_eq!(value["request_id"], "page-7", "{value}");
}

#[tokio::test]
async fn the_memories_list_refuses_by_name_past_ten_pages_of_keys_and_never_shows_a_cut_list() {
    let script: Script = Arc::new(|_, path| {
        if path.starts_with("/v2/keys") {
            // Every page names a fresh next one: past the ten the list reads.
            let n: u32 = path
                .split("cursor=c")
                .nth(1)
                .and_then(|n| n.split('&').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let page = json!({"items": [], "next_cursor": format!("c{}", n + 1)});
            (200, page, Vec::new())
        } else {
            (200, empty_page(), Vec::new())
        }
    });
    let page = send(script, get("/web/memories")).await;
    assert_eq!(
        page.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        page.body
    );
    assert!(page.body.contains("limit.list_too_long"), "{}", page.body);
    let keys = page
        .calls
        .iter()
        .filter(|c| c.starts_with("GET /v2/keys"))
        .count();
    assert_eq!(keys, oml_web::memories::KEY_PAGES_MAX, "{:?}", page.calls);
}

#[tokio::test]
async fn a_key_cursor_the_api_repeats_refuses_the_list_instead_of_cutting_it() {
    let script: Script = Arc::new(|_, path| {
        if path.starts_with("/v2/keys") {
            (200, json!({"items": [], "next_cursor": "same"}), Vec::new())
        } else {
            (200, empty_page(), Vec::new())
        }
    });
    let page = send(script, get("/web/memories")).await;
    assert_eq!(
        page.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        page.body
    );
    assert!(page.body.contains("limit.list_too_long"), "{}", page.body);
}

#[tokio::test]
async fn a_verdict_recorded_before_the_next_claim_fails_lands_on_the_view_with_its_notice() {
    let script: Script = Arc::new(|method, path| match (method.as_str(), path) {
        ("POST", "/v2/review/verdicts") => (
            200,
            json!({"artifact_id": "m1", "version": 3, "verdict": "approve", "state": "promoted"}),
            Vec::new(),
        ),
        ("POST", "/v2/review/next") => (503, envelope("system.unavailable"), Vec::new()),
        _ => (404, envelope("access.not_found"), Vec::new()),
    });
    let answer = send(
        script,
        post(
            "/web/review",
            "action=verdict&serve_id=srv_1&verdict=approve&nonce=0123456789abcdef0123456789abcdef",
        ),
    )
    .await;
    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{}", answer.body);
    assert_eq!(
        answer.headers[header::LOCATION],
        "/web/review?done=verdict&version=3&state=promoted"
    );
    assert!(
        answer.calls.iter().any(|c| c == "POST /v2/review/next"),
        "the claim was tried: {:?}",
        answer.calls
    );
}

/// `POST /v2/keys` answered with a name and no value, with the replay
/// header or without it.
fn issued_without_a_value(replayed: bool) -> Script {
    Arc::new(move |method, path| match (method.as_str(), path) {
        ("POST", "/v2/keys") => (
            201,
            json!({"key_id": "k9", "name": "alice-laptop"}),
            if replayed {
                vec![("idempotency-replay", "true")]
            } else {
                Vec::new()
            },
        ),
        _ => (404, envelope("access.not_found"), Vec::new()),
    })
}

const ISSUE: &str =
    "action=issue_key&name=laptop&role=writer&nonce=0123456789abcdef0123456789abcdef";

#[tokio::test]
async fn a_replayed_issue_redirects_to_the_keys_and_sets_no_cookie_over_the_first_answers() {
    let answer = send(issued_without_a_value(true), post("/web/keys", ISSUE)).await;
    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{}", answer.body);
    assert_eq!(answer.headers[header::LOCATION], "/web/keys");
    assert!(answer.headers.get(header::SET_COOKIE).is_none());
}

#[tokio::test]
async fn an_issue_answered_without_a_value_and_not_as_a_replay_is_refused_by_name() {
    let answer = send(issued_without_a_value(false), post("/web/keys", ISSUE)).await;
    assert_eq!(answer.status, StatusCode::BAD_GATEWAY, "{}", answer.body);
    assert!(answer.body.contains("unavailable"), "{}", answer.body);
    assert!(answer.headers.get(header::SET_COOKIE).is_none());
}
