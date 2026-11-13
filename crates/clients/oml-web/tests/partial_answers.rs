//! What a page answers when one of its API calls fails or answers past a
//! bound (ADR-0070 d8), against an API double that answers each path as a
//! test says: the Atlas and its JSON when the offers fail, the JSON route
//! when the listing fails, a verdict whose next claim fails, the list past
//! the pages of keys it reads, a key issue whose answer carries no value
//! (#253), the allowlist at a cursor the API repeats (#293), and the key
//! list at one, in a cycle of them and past its pages (#308).

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
    /// Whether the session's `admin` credential resolves too.
    admin: bool,
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
        // Only the web credential resolves, as for a person who is no admin,
        // unless the double is an admin's.
        let (status, body, extra) = if path.starts_with("/v2/whoami") {
            let resolves =
                |capacity| bearer == format!("Bearer {}", capacity_credential(TOKEN, capacity));
            if resolves("web") || (self.admin && resolves("admin")) {
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
    send_as(script, request, false).await
}

/// As [`send`], by an admin's session where `admin`.
async fn send_as(script: Script, request: Request<Body>, admin: bool) -> Answered {
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
                admin,
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
    assert!(
        page.body
            .contains(r#"<h2 id="sel-h" class="mono">alice</h2>"#),
        "the panel names the scope by its reference: {}",
        page.body
    );
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

/// The memories JSON names no `base`: the lead page it named is gone and
/// nothing read it (#281).
#[tokio::test]
async fn the_memories_json_names_no_base_page() {
    let answer = send(offers_fail(), get("/web/data/memories")).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let value: Value = serde_json::from_str(&answer.body).unwrap();
    assert!(value.get("base").is_none(), "{value}");
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

/// The text search with an author is the one Memories read that walks the
/// key list: the recall filters by the key's id.
#[tokio::test]
async fn the_memories_search_refuses_by_name_past_ten_pages_of_keys_and_never_shows_a_cut_list() {
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
    let page = send(script, get("/web/memories?q=deploy&author=alice-laptop")).await;
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

/// A cursor the API repeats is the API's fault, not the person's: the
/// list is refused as `unavailable` with 502, never as
/// `limit.list_too_long`, which tells them they hold too many keys
/// (#281).
#[tokio::test]
async fn a_key_cursor_the_api_repeats_is_refused_as_an_api_fault_instead_of_cutting_the_list() {
    let script: Script = Arc::new(|_, path| {
        if path.starts_with("/v2/keys") {
            (200, json!({"items": [], "next_cursor": "same"}), Vec::new())
        } else {
            (200, empty_page(), Vec::new())
        }
    });
    let page = send(script, get("/web/memories?q=deploy&author=alice-laptop")).await;
    assert_eq!(page.status, StatusCode::BAD_GATEWAY, "{}", page.body);
    assert!(page.body.contains("unavailable"), "{}", page.body);
    assert!(page.body.contains("repeated"), "{}", page.body);
    assert!(!page.body.contains("limit.list_too_long"), "{}", page.body);
}

/// The allowlist the same way (#293): a cursor the API repeats is refused
/// `unavailable` with 502, on the page and on the act that reads the list
/// to find its entry, and the list is never shown cut at the repeat.
#[tokio::test]
async fn an_allowlist_cursor_the_api_repeats_is_refused_as_an_api_fault_instead_of_cutting_the_list()
 {
    let script: Script = Arc::new(|_, path| {
        if path.starts_with("/v2/web/emails") {
            let entry = json!({"email": "first@example.com", "user": "first", "admin": true});
            (
                200,
                json!({"items": [entry], "next_cursor": "same"}),
                Vec::new(),
            )
        } else {
            (200, empty_page(), Vec::new())
        }
    });
    let page = send_as(script.clone(), get("/web/admin/people"), true).await;
    assert_eq!(page.status, StatusCode::BAD_GATEWAY, "{}", page.body);
    assert!(page.body.contains("unavailable"), "{}", page.body);
    assert!(page.body.contains("repeated"), "{}", page.body);
    assert!(!page.body.contains("first@example.com"), "{}", page.body);
    let listed = |calls: &[String]| {
        calls
            .iter()
            .filter(|c| c.starts_with("GET /v2/web/emails"))
            .count()
    };
    assert_eq!(listed(&page.calls), 2, "{:?}", page.calls);

    let act = send_as(
        script,
        post(
            "/web/admin/people",
            "action=set_admin&email=first%40example.com&nonce=0123456789abcdef0123456789abcdef",
        ),
        true,
    )
    .await;
    assert_eq!(act.status, StatusCode::BAD_GATEWAY, "{}", act.body);
    assert!(act.body.contains("unavailable"), "{}", act.body);
    assert!(
        !act.calls.iter().any(|c| c.starts_with("POST ")),
        "{:?}",
        act.calls
    );
}

/// A cycle of allowlist cursors, A then B then A, is the same fault as a
/// cursor repeated at once: refused `unavailable` at the page that names
/// A again, the third (#308).
#[tokio::test]
async fn an_allowlist_cursor_cycle_is_refused_as_an_api_fault_at_its_first_repeat() {
    let script: Script = Arc::new(|_, path| {
        if path.starts_with("/v2/web/emails") {
            let entry = json!({"email": "first@example.com", "user": "first", "admin": true});
            let next = if path.contains("cursor=a") { "b" } else { "a" };
            (
                200,
                json!({"items": [entry], "next_cursor": next}),
                Vec::new(),
            )
        } else {
            (200, empty_page(), Vec::new())
        }
    });
    let page = send_as(script, get("/web/admin/people"), true).await;
    assert_eq!(page.status, StatusCode::BAD_GATEWAY, "{}", page.body);
    assert!(page.body.contains("unavailable"), "{}", page.body);
    assert!(!page.body.contains("first@example.com"), "{}", page.body);
    assert_eq!(calls_to(&page, "GET /v2/web/emails"), 3, "{:?}", page.calls);
}

/// One agent key of the person `first`, as `GET /v2/keys` lists it.
fn agent_key() -> Value {
    json!({
        "id": "k-agent", "name": "first-agent", "role": "writer", "held_by": "agent",
        "credential": "bearer", "revoked": false, "cut_off": false, "grants": [],
    })
}

/// The calls of `answered` that start with `prefix`.
fn calls_to(answered: &Answered, prefix: &str) -> usize {
    answered
        .calls
        .iter()
        .filter(|c| c.starts_with(prefix))
        .count()
}

/// The allowlist's one entry, `first`'s only address, and the key list as
/// `keys` answers each of its pages by the path's query.
fn one_address_and_keys(keys: fn(&str) -> Value) -> Script {
    Arc::new(move |_, path| {
        if path.starts_with("/v2/web/emails") {
            let entry = json!({"email": "first@example.com", "user": "first", "admin": false});
            (
                200,
                json!({"items": [entry], "next_cursor": null}),
                Vec::new(),
            )
        } else if path.starts_with("/v2/keys") {
            (200, keys(path), Vec::new())
        } else {
            (200, empty_page(), Vec::new())
        }
    })
}

/// The three places that walk a person's key list, each sent to `script`:
/// the Keys view, the admin's Keys view, and the confirmation before a
/// person's last address is removed, which lists the keys to cut off.
async fn key_walks(script: &Script) -> [Answered; 3] {
    [
        send_as(script.clone(), get("/web/keys"), true).await,
        send_as(script.clone(), get("/web/admin/keys?user=first"), true).await,
        send_as(
            script.clone(),
            post(
                "/web/admin/people",
                "action=remove_email&email=first%40example.com",
            ),
            true,
        )
        .await,
    ]
}

/// A key cursor the API repeats is the API's fault on every page that
/// walks the key list (#308): `unavailable` with 502 at the repeat, and
/// no key of the cut list is shown, where the confirmation before a last
/// address is removed would have left the keys past the repeat out.
#[tokio::test]
async fn a_key_list_cursor_the_api_repeats_is_refused_as_an_api_fault_and_no_cut_list_is_shown() {
    let script = one_address_and_keys(|_| json!({"items": [agent_key()], "next_cursor": "same"}));
    for answered in key_walks(&script).await {
        assert_eq!(
            answered.status,
            StatusCode::BAD_GATEWAY,
            "{}",
            answered.body
        );
        assert!(answered.body.contains("unavailable"), "{}", answered.body);
        assert!(answered.body.contains("repeated"), "{}", answered.body);
        assert!(!answered.body.contains("first-agent"), "{}", answered.body);
        assert!(
            !answered.body.contains("limit.list_too_long"),
            "{}",
            answered.body
        );
        assert_eq!(
            calls_to(&answered, "GET /v2/keys"),
            2,
            "{:?}",
            answered.calls
        );
    }
}

/// A cycle of key cursors, A then B then A, is refused at the page that
/// names A again, the third: a walk that compared a cursor with the one
/// before it alone went round to its last page and showed what it had
/// (#308).
#[tokio::test]
async fn a_key_list_cursor_cycle_is_refused_as_an_api_fault_at_its_first_repeat() {
    let script = one_address_and_keys(|path| {
        let next = if path.contains("cursor=a") { "b" } else { "a" };
        json!({"items": [agent_key()], "next_cursor": next})
    });
    for answered in key_walks(&script).await {
        assert_eq!(
            answered.status,
            StatusCode::BAD_GATEWAY,
            "{}",
            answered.body
        );
        assert!(answered.body.contains("unavailable"), "{}", answered.body);
        assert!(!answered.body.contains("first-agent"), "{}", answered.body);
        assert_eq!(
            calls_to(&answered, "GET /v2/keys"),
            3,
            "{:?}",
            answered.calls
        );
    }
}

/// Past the pages a view reads, the key list is refused by name,
/// `limit.list_too_long`, and never shown cut at its last page (#308).
#[tokio::test]
async fn a_key_list_past_the_pages_a_view_reads_is_refused_by_name_and_no_cut_list_is_shown() {
    let script = one_address_and_keys(|path| {
        // Every page names a fresh next one.
        let n: u32 = path
            .split("cursor=c")
            .nth(1)
            .and_then(|n| n.split('&').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        json!({"items": [agent_key()], "next_cursor": format!("c{}", n + 1)})
    });
    for answered in key_walks(&script).await {
        assert_eq!(
            answered.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            answered.body
        );
        assert!(
            answered.body.contains("limit.list_too_long"),
            "{}",
            answered.body
        );
        assert!(!answered.body.contains("first-agent"), "{}", answered.body);
        assert_eq!(
            calls_to(&answered, "GET /v2/keys"),
            oml_web::keys::KEY_PAGES_MAX,
            "{:?}",
            answered.calls
        );
    }
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
