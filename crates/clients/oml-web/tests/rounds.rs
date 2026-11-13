//! The sequential rounds a page spends at the API (ADR-0070 d8; #286),
//! against an API double that holds a call's answer until the call it must
//! run beside has arrived: a page that sends the two in turn waits for an
//! answer that never comes, and the double answers it `504` after
//! [`PARTNER_WAIT`]. Which calls each page sends, and with what query, is
//! read off the double's record.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use http::{Request, Response, StatusCode, header};
use oml_sdk::{Sending, Transport};
use oml_web::middleware::body_bytes;
use oml_web::{Layers, SESSION_COOKIE, Web, api, router};
use oml_web_credential::capacity_credential;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt as _;

const SECRET: &str = "0123456789abcdef0123456789abcdef";
const TOKEN: &str = "the-session-token";
const ROOT: &str = "6f1c2d4e-0000-4000-8000-000000000001";
const UNIVERSE: &str = "6f1c2d4e-0000-4000-8000-000000000000";

/// How long a held answer waits for its partner call.
const PARTNER_WAIT: Duration = Duration::from_secs(2);

/// A call as the double saw it: `METHOD path capacity`.
type Seen = Vec<String>;

/// An answer, and the call (a substring of its record) it waits for.
type Script = fn(&str) -> (u16, Value, Option<&'static str>);

struct Double {
    script: Script,
    seen: Arc<Mutex<Seen>>,
    arrived: watch::Sender<Seen>,
}

fn envelope(code: &str) -> Value {
    json!({"error": {"code": code, "message": format!("{code} here")}, "request_id": "req_api"})
}

/// The capacity a bearer is the session's credential of.
fn capacity(bearer: &str) -> &'static str {
    ["web", "admin", "review", "curate"]
        .into_iter()
        .find(|c| bearer == format!("Bearer {}", capacity_credential(TOKEN, c)))
        .unwrap_or("none")
}

impl Transport for Double {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
        let path = request.uri().to_string();
        let bearer = request.headers()[header::AUTHORIZATION]
            .to_str()
            .unwrap()
            .to_owned();
        let record = format!("{} {path} {}", request.method(), capacity(&bearer));
        self.seen.lock().unwrap().push(record.clone());
        self.arrived.send_modify(|all| all.push(record.clone()));
        let (status, body, partner) = (self.script)(&record);
        let mut arrived = self.arrived.subscribe();
        Box::pin(async move {
            let (status, body) = match partner {
                None => (status, body),
                Some(partner) => {
                    let came = tokio::time::timeout(
                        PARTNER_WAIT,
                        arrived.wait_for(|all| all.iter().any(|r| r.contains(partner))),
                    )
                    .await;
                    match came {
                        Ok(Ok(_)) => (status, body),
                        _ => (504, envelope("test.sent_in_turn")),
                    }
                }
            };
            Ok(Response::builder()
                .status(status)
                .body(body.to_string().into_bytes())
                .unwrap())
        })
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

fn offers(workarea: &str) -> Value {
    json!({
        "workarea_id": workarea, "status": "development", "mode": "standard",
        "transitions": [{"op": "propose", "to": "committed"}],
        "mode_change": {"to": "in_progress"},
        "commit": {"offered": false, "held": null}, "refused": [],
    })
}

fn counts(top: &str, path: &str) -> Value {
    json!({
        "items": [{
            "id": top, "name": "top", "parent_id": null, "path": path,
            "status": "development", "mode": "standard",
            "counts": {"promoted": 2, "held": 1, "in_progress": 0, "other": 0},
        }],
        "next_cursor": null,
        "truncated": false,
    })
}

/// A page's answer and every call it made.
struct Answered {
    status: StatusCode,
    body: String,
    calls: Seen,
}

async fn page(script: Script, path: &str) -> Answered {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = router(Web {
        layers: Layers {
            public_origin: Some("https://memory.example".to_owned()),
            log: Arc::new(|_| {}),
        },
        api: api(
            Arc::new(Double {
                script,
                seen: seen.clone(),
                arrived: watch::channel(Vec::new()).0,
            }),
            SECRET,
        )
        .unwrap(),
    });
    let response = app
        .oneshot(
            Request::get(path)
                .header("x-request-id", "page-9")
                .header(header::COOKIE, format!("{SESSION_COOKIE}={TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = body_bytes(response.into_body()).await.unwrap();
    let calls = seen.lock().unwrap().clone();
    Answered {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
        calls,
    }
}

/// The session check, as every page opens: both credentials' `whoami`, the
/// `admin` one resolving for an admin's entry.
fn session(record: &str) -> Option<(u16, Value, Option<&'static str>)> {
    if !record.starts_with("GET /v2/whoami") {
        return None;
    }
    Some(if record.ends_with(" web") || record.ends_with(" admin") {
        (200, whoami(), None)
    } else {
        (401, envelope("auth.invalid"), None)
    })
}

/// The text search reads the person's keys beside the recall, so the page
/// spends two rounds, the session check and then both, where it spent
/// three (#286).
#[tokio::test]
async fn the_text_search_reads_the_persons_keys_beside_the_recall() {
    let script: Script = |record| {
        if let Some(answer) = session(record) {
            return answer;
        }
        if record.starts_with("GET /v2/keys") {
            let page = json!({"items": [{
                "id": "k9", "name": "alice-laptop", "role": "writer", "held_by": "agent",
                "credential": "bearer", "revoked": false, "cut_off": false, "grants": [],
            }], "next_cursor": null});
            (200, page, Some("POST /v1/memories/recall"))
        } else if record.starts_with("POST /v1/memories/recall") {
            (
                200,
                json!({"steering": [], "hits": []}),
                Some("GET /v2/keys"),
            )
        } else {
            (500, envelope("internal.error"), None)
        }
    };
    let answered = page(script, "/web/memories?q=deploy").await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    assert!(
        answered.body.contains("No memory matches."),
        "{}",
        answered.body
    );
    // A named author is still refused by name when no key of the person's
    // holds it, after both reads.
    let unknown = page(script, "/web/memories?q=deploy&author=someone-else").await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{}", unknown.body);
}

/// The Keys page reads the person's usage beside the key list, so the card
/// adds no round: each call waits for the other, and the page answers with
/// both after the session check (ADR-0070 d8).
#[tokio::test]
async fn the_keys_page_reads_the_persons_usage_beside_the_key_list() {
    let script: Script = |record| {
        if record == "GET /v2/whoami web" {
            let mut person = whoami();
            person["person"] = json!({
                "user": "alice", "under_quotas": true,
                "usage": {"memories": 4, "text_bytes": 100, "writes": 2, "keys": 1, "connections": 0},
                "quotas": {
                    "memories": 1000, "text_bytes": null, "writes_per_month": null,
                    "keys": 5, "connections": 5, "requests_per_minute": 60,
                },
                "deletion_requested_at": null,
            });
            return (200, person, Some("GET /v2/keys"));
        }
        if let Some(answer) = session(record) {
            return answer;
        }
        if record.starts_with("GET /v2/keys") {
            (
                200,
                json!({"items": [], "next_cursor": null}),
                Some("GET /v2/whoami web"),
            )
        } else {
            (500, envelope("internal.error"), None)
        }
    };
    let answered = page(script, "/web/keys").await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    assert!(
        answered.body.contains("<h2>Your usage</h2>") && answered.body.contains("4 / 1000"),
        "{}",
        answered.body
    );
    assert_eq!(
        answered
            .calls
            .iter()
            .filter(|c| !c.starts_with("GET /v2/whoami?person=false"))
            .count(),
        2,
        "{:?}",
        answered.calls
    );
}

/// The admin Atlas reads the scope's transitions offers with its counts, in
/// the admin key's one counted tree, beside the reviewer key's offers for
/// the mode: two rounds where it spent three, and no offers call of the
/// admin key's own (#286).
#[tokio::test]
async fn the_admin_atlas_reads_the_scopes_offers_with_its_counts() {
    let script: Script = |record| {
        if let Some(answer) = session(record) {
            return answer;
        }
        if record == "GET /v2/workareas/universe/tree?counts=true&offers=true admin" {
            let mut both = counts(UNIVERSE, "universe");
            both["offers"] = offers(UNIVERSE);
            (200, both, Some("/offers review"))
        } else if record == "GET /v2/workareas/universe/offers review" {
            (
                200,
                offers(UNIVERSE),
                Some("tree?counts=true&offers=true admin"),
            )
        } else {
            (500, envelope("internal.error"), None)
        }
    };
    let answered = page(script, "/web/admin/atlas").await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    assert!(
        !answered.body.contains("The acts are unavailable"),
        "{}",
        answered.body
    );
    assert!(
        answered.body.contains("propose committed"),
        "{}",
        answered.body
    );
    assert_eq!(
        answered
            .calls
            .iter()
            .filter(|c| !c.starts_with("GET /v2/whoami"))
            .count(),
        2,
        "{:?}",
        answered.calls
    );
}

/// The session check asks `whoami` for no person block: no page reads it
/// from the check, so neither credential's call pays for it (#286).
#[tokio::test]
async fn the_session_check_asks_whoami_for_no_person_block() {
    let script: Script = |record| {
        session(record).unwrap_or_else(|| (200, json!({"items": [], "next_cursor": null}), None))
    };
    let answered = page(script, "/web/review").await;
    let checks: Vec<&String> = answered
        .calls
        .iter()
        .filter(|c| c.starts_with("GET /v2/whoami"))
        .collect();
    assert_eq!(checks.len(), 2, "{:?}", answered.calls);
    for check in checks {
        assert!(check.starts_with("GET /v2/whoami?person=false "), "{check}");
    }
}

/// Against an API one release behind, which refuses `offers` on the
/// counted tree as a parameter it does not know, the admin Atlas reads the
/// counts and the transitions' offers apart, as it did before #286, and
/// still draws both (ADR-0070 d4).
#[tokio::test]
async fn the_admin_atlas_reads_its_offers_apart_from_an_api_that_refuses_the_parameter() {
    let script: Script = |record| {
        if let Some(answer) = session(record) {
            return answer;
        }
        match record {
            "GET /v2/workareas/universe/tree?counts=true&offers=true admin" => {
                (400, envelope("request.validation"), None)
            }
            "GET /v2/workareas/universe/tree?counts=true admin" => {
                (200, counts(UNIVERSE, "universe"), None)
            }
            "GET /v2/workareas/universe/offers admin"
            | "GET /v2/workareas/universe/offers review" => (200, offers(UNIVERSE), None),
            "POST /v2/admin/memories/search admin" => {
                (200, json!({"items": [], "next_cursor": null}), None)
            }
            _ => (500, envelope("internal.error"), None),
        }
    };
    let answered = page(script, "/web/admin/atlas").await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    assert!(answered.body.contains("universe"), "{}", answered.body);
    assert!(
        !answered.body.contains("The acts are unavailable"),
        "{}",
        answered.body
    );
    assert!(
        answered.body.contains("propose committed"),
        "{}",
        answered.body
    );
    assert!(
        answered
            .calls
            .contains(&"GET /v2/workareas/universe/offers admin".to_owned()),
        "{:?}",
        answered.calls
    );
    // The JSON route reads the regions the same way.
    let json = page(script, "/web/admin/data/memories").await;
    assert_eq!(json.status, StatusCode::OK, "{}", json.body);
    let value: Value = serde_json::from_str(&json.body).unwrap();
    let scope = &value["workareas"]["items"][0];
    assert_eq!(scope["path"], "universe", "{value}");
    assert!(scope.get("offers_unavailable").is_none(), "{value}");
}

/// The person's names under `universe`: the tree read the picker's hints
/// come from.
fn names() -> Value {
    json!({"items": [
        {"id": UNIVERSE, "name": "universe", "path": "universe", "parent_id": null, "user_root": false},
        {"id": ROOT, "name": "alice", "path": "universe.alice", "parent_id": UNIVERSE, "user_root": true},
        {"id": "6f1c2d4e-0000-4000-8000-000000000002", "name": "proj", "path": "universe.alice.proj", "parent_id": ROOT, "user_root": false},
    ], "next_cursor": null})
}

/// The person's Memories with each read the page makes after the session
/// check held until its partner arrives: the listing, the person's keys
/// and the picker's names go together in one round.
fn memories_script(record: &str) -> (u16, Value, Option<&'static str>) {
    if let Some(answer) = session(record) {
        return answer;
    }
    if record.starts_with("GET /v2/keys") {
        (
            200,
            json!({"items": [], "next_cursor": null}),
            Some("/tree?limit=200"),
        )
    } else if record.starts_with("GET /v2/workareas/universe/tree?limit=200 web") {
        (200, names(), Some("GET /v1/memories"))
    } else if record.starts_with("GET /v1/memories") {
        (
            200,
            json!({"items": [], "next_cursor": null}),
            Some("/tree?limit=200"),
        )
    } else if record.starts_with("GET /v2/workareas/universe/tree?counts=true web") {
        (200, counts(UNIVERSE, "universe"), None)
    } else {
        (500, envelope("internal.error"), None)
    }
}

/// The picker's hints are names only, from the uncounted tree read made
/// beside the listing, so Memories keeps two rounds; with Browse closed
/// the page makes no counted read, and Browse asked open makes exactly one
/// (the operator's decision on the design notes' row 14).
#[tokio::test]
async fn the_memories_page_reads_the_pickers_names_beside_its_listing_and_counts_only_for_an_open_browse()
 {
    let closed = page(memories_script, "/web/memories").await;
    assert_eq!(closed.status, StatusCode::OK, "{}", closed.body);
    assert!(
        closed
            .body
            .contains(r#"<option value="alice.proj"></option>"#),
        "{}",
        closed.body
    );
    let counted = |calls: &Seen| calls.iter().filter(|c| c.contains("counts=true")).count();
    let named = |calls: &Seen| {
        calls
            .iter()
            .filter(|c| c.contains("tree?limit=200"))
            .count()
    };
    assert_eq!(counted(&closed.calls), 0, "{:?}", closed.calls);
    assert_eq!(named(&closed.calls), 1, "{:?}", closed.calls);
    assert!(
        closed
            .body
            .contains(r#"href="/web/memories?browse=1">Browse</a>"#),
        "{}",
        closed.body
    );
    let open = page(memories_script, "/web/memories?browse=1").await;
    assert_eq!(open.status, StatusCode::OK, "{}", open.body);
    assert_eq!(counted(&open.calls), 1, "{:?}", open.calls);
    assert_eq!(named(&open.calls), 1, "{:?}", open.calls);
    assert!(
        open.body
            .contains(r#"<details class="oml-wa-browse" open>"#),
        "{}",
        open.body
    );
    assert!(open.body.contains("3 readable"), "{}", open.body);
}

/// A path typed where a name goes is answered by the console with the
/// API's code for it, and the listing is never asked (ADR-0026 d7).
#[tokio::test]
async fn a_path_typed_in_the_field_is_refused_without_a_listing_call() {
    // No listing comes, so the names' read waits for no partner.
    let script: Script = |record| match memories_script(record) {
        (status, body, Some("GET /v1/memories")) => (status, body, None),
        other => other,
    };
    let answered = page(script, "/web/memories?workarea=Alice.Proj.Docs").await;
    assert_eq!(
        answered.status,
        StatusCode::BAD_REQUEST,
        "{}",
        answered.body
    );
    assert!(
        !answered
            .calls
            .iter()
            .any(|c| c.starts_with("GET /v1/memories")),
        "{:?}",
        answered.calls
    );
    assert!(
        answered.body.contains("<code>request.validation</code>"),
        "{}",
        answered.body
    );
    assert!(
        answered
            .body
            .contains("<code>alice.proj.docs</code> is a path, not a name."),
        "{}",
        answered.body
    );
    assert!(
        answered.body.contains(r#"aria-invalid="true""#),
        "{}",
        answered.body
    );
    assert!(
        answered
            .body
            .contains(r#"<a href="/web/memories?workarea=alice.proj">alice.proj</a>"#),
        "the closest names come from the names read: {}",
        answered.body
    );
}
