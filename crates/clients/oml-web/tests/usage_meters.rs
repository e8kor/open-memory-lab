//! The usage meters the pages draw from the fields `GET /v2/whoami`'s
//! `person` and each row of `GET /v2/web/users` carry (ADR-0071 d6): the
//! "Your usage" card of the Start and Keys pages, and the meter columns of
//! the users page, each meter in both of its states and for a person no
//! limit is read for. Against an API double that answers the person as a
//! test says.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use http::{Request, Response, StatusCode, header};
use oml_sdk::{Sending, Transport};
use oml_web::middleware::body_bytes;
use oml_web::{Layers, SESSION_COOKIE, Web, api, router};
use serde_json::{Value, json};
use tower::ServiceExt as _;

const SECRET: &str = "0123456789abcdef0123456789abcdef";
const TOKEN: &str = "the-session-token";
const ORIGIN: &str = "https://memory.example";
const ROOT: &str = "6f1c2d4e-0000-4000-8000-000000000001";

/// The double's answers past the session check, by path with query.
type Script = Arc<dyn Fn(&str) -> (u16, Value) + Send + Sync>;

struct Double {
    script: Script,
    seen: Arc<Mutex<Vec<String>>>,
}

fn whoami() -> Value {
    json!({
        "key_id": "k1", "name": "alice-web", "role": "user", "principal": "alice",
        "held_by": "human", "tenant": {"id": "t1", "name": "oml"},
        "default_workarea": null, "crew": null,
        "user": {"id": ROOT, "name": "alice"}, "grants": [],
    })
}

/// `whoami` with the person block a test names, as the Start and Keys
/// pages ask for it.
fn whoami_of(person: Value) -> Value {
    let mut value = whoami();
    value["person"] = person;
    value
}

impl Transport for Double {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
        let path = request.uri().to_string();
        self.seen
            .lock()
            .unwrap()
            .push(format!("{} {path}", request.method()));
        let (status, body) = if path == "/v2/whoami?person=false" {
            (200, whoami())
        } else {
            (self.script)(&path)
        };
        let response = Response::builder()
            .status(status)
            .body(body.to_string().into_bytes())
            .unwrap();
        Box::pin(async move { Ok(response) })
    }
}

struct Answered {
    status: StatusCode,
    body: String,
    calls: Vec<String>,
}

async fn get(script: Script, path: &str) -> Answered {
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
    let request = Request::get(path)
        .header("x-request-id", "page-7")
        .header(header::COOKIE, format!("{SESSION_COOKIE}={TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = body_bytes(response.into_body()).await.unwrap();
    let calls = seen.lock().unwrap().clone();
    Answered {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
        calls,
    }
}

/// The card's row for the meter `name`, its `<tr>` whole.
fn row(page: &str, name: &str) -> String {
    let start = page
        .find(&format!("<tr><td>{name}</td>"))
        .unwrap_or_else(|| panic!("no {name} row in {page}"));
    let end = page[start..].find("</tr>").unwrap() + start;
    page[start..end].to_owned()
}

/// A person under limits: memories at 80% of a thousand, text stored at
/// its limit, writes with no limit, keys and connections well under theirs.
fn person_under_limits() -> Value {
    json!({
        "user": "alice", "under_quotas": true,
        "usage": {
            "memories": 800, "text_bytes": 2_000_000, "writes": 120, "keys": 2,
            "connections": 0,
        },
        "quotas": {
            "memories": 1000, "text_bytes": 2_000_000, "writes_per_month": null,
            "keys": 5, "connections": 5, "requests_per_minute": 60,
        },
        "deletion_requested_at": null,
        "start": {"connected": false, "first_memory": null},
    })
}

/// A person whose account no limit is read for: outside the sign-up tenant
/// and under no profile, so the API serves them no counts.
fn person_with_no_profile() -> Value {
    json!({
        "user": "alice", "under_quotas": false, "usage": null, "quotas": null,
        "deletion_requested_at": null,
        "start": {"connected": false, "first_memory": null},
    })
}

fn start_script(person: Value) -> Script {
    Arc::new(move |path| {
        if path == "/v2/whoami?start=true" {
            (200, whoami_of(person.clone()))
        } else {
            // The page's address lookup finds none and falls back to the
            // public origin.
            (
                404,
                json!({"error": {"code": "access.not_found", "message": path}}),
            )
        }
    })
}

fn keys_script(person: Result<Value, ()>) -> Script {
    Arc::new(move |path| {
        if path == "/v2/whoami" {
            match &person {
                Ok(person) => (200, whoami_of(person.clone())),
                Err(()) => (
                    503,
                    json!({"error": {"code": "unavailable", "message": "later"}, "request_id": "req_api"}),
                ),
            }
        } else if path.starts_with("/v2/keys") {
            let key = json!({
                "id": "k-agent", "name": "alice-agent", "role": "writer", "held_by": "agent",
                "credential": "bearer", "revoked": false, "cut_off": false, "grants": [],
            });
            (200, json!({"items": [key], "next_cursor": null}))
        } else {
            (
                500,
                json!({"error": {"code": "internal.error", "message": path}}),
            )
        }
    })
}

#[tokio::test]
async fn the_start_page_draws_each_meter_against_its_limit_with_a_mark_at_eighty_percent_and_at_the_limit()
 {
    let page = get(start_script(person_under_limits()), "/web/start").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("<h2>Your usage</h2>"), "{}", page.body);

    let memories = row(&page.body, "Memories");
    assert!(memories.contains("800 / 1000"), "{memories}");
    assert!(memories.contains(r#"is-held">80%</span>"#), "{memories}");
    assert!(!memories.contains("is-rejected"), "{memories}");
    assert!(memories.contains(r#"max="1000""#), "{memories}");
    assert!(memories.contains(r#"value="800""#), "{memories}");

    let text = row(&page.body, "Text stored");
    assert!(text.contains("2.0 MB / 2.0 MB"), "{text}");
    assert!(text.contains(r#"is-rejected">limit</span>"#), "{text}");
    assert!(!text.contains("is-held"), "{text}");

    // Under 80%: no mark.
    for name in ["Keys", "Connections"] {
        let under = row(&page.body, name);
        assert!(!under.contains("oml-chip"), "{under}");
    }
    assert!(row(&page.body, "Keys").contains("2 / 5"));

    // A meter with no limit shows its count alone, no mark and no bar.
    let writes = row(&page.body, "Writes this month");
    assert!(
        writes.contains(r#"<span class="mono">120</span>"#),
        "{writes}"
    );
    assert!(
        !writes.contains("oml-chip") && !writes.contains("<meter"),
        "{writes}"
    );

    assert!(page.body.contains("60 / min"), "{}", page.body);
    // The card spends no call of its own: the person the page already read
    // carries it.
    assert_eq!(
        page.calls
            .iter()
            .filter(|c| c.starts_with("GET /v2/whoami") && !c.contains("person=false"))
            .count(),
        1,
        "{:?}",
        page.calls
    );
}

#[tokio::test]
async fn a_person_no_limit_is_read_for_gets_no_usage_card_and_no_zeros() {
    for path in ["/web/start", "/web/keys"] {
        let script: Script = if path == "/web/start" {
            start_script(person_with_no_profile())
        } else {
            keys_script(Ok(person_with_no_profile()))
        };
        let page = get(script, path).await;
        assert_eq!(page.status, StatusCode::OK, "{path}: {}", page.body);
        assert!(!page.body.contains("Your usage"), "{path}: {}", page.body);
        assert!(
            !page.body.contains(r#"id="usage""#),
            "{path}: {}",
            page.body
        );
        assert!(!page.body.contains(" / min"), "{path}: {}", page.body);
    }
}

#[tokio::test]
async fn a_meter_an_older_api_does_not_serve_is_left_out_and_never_shown_as_zero() {
    let mut person = person_under_limits();
    for field in ["text_bytes", "writes"] {
        person["usage"].as_object_mut().unwrap().remove(field);
    }
    for field in ["text_bytes", "writes_per_month"] {
        person["quotas"].as_object_mut().unwrap().remove(field);
    }
    let page = get(start_script(person), "/web/start").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("800 / 1000"), "{}", page.body);
    assert!(!page.body.contains("Text stored"), "{}", page.body);
    assert!(!page.body.contains("Writes this month"), "{}", page.body);
}

#[tokio::test]
async fn the_keys_page_draws_the_card_above_the_keys_and_marks_it_unavailable_when_the_person_cannot_be_read()
 {
    let page = get(keys_script(Ok(person_under_limits())), "/web/keys").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    let card = page.body.find("<h2>Your usage</h2>").expect("the card");
    let keys = page.body.find("alice-agent").expect("the key");
    assert!(card < keys, "{}", page.body);
    assert!(row(&page.body, "Memories").contains("800 / 1000"));
    assert!(row(&page.body, "Text stored").contains("limit</span>"));

    // The person call is the secondary one: its failure leaves the keys and
    // names the request that failed.
    let page = get(keys_script(Err(())), "/web/keys").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("alice-agent"), "{}", page.body);
    assert!(
        page.body
            .contains("Your usage is unavailable (request <code>page-7."),
        "{}",
        page.body
    );
    assert!(!page.body.contains("800 / 1000"), "{}", page.body);
}

#[tokio::test]
async fn the_card_and_the_users_page_say_which_limits_the_quota_form_raises_and_which_the_profile_sets()
 {
    // The quota form posts to a route that takes memories, keys, connections
    // and requests per minute; text stored and writes are the profile's.
    let page = get(start_script(person_under_limits()), "/web/start").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(
        page.body.contains(
            "An administrator can raise a limit for you. Text stored and writes follow the profile of your account, which an administrator changes with an admin key."
        ),
        "{}",
        page.body
    );

    let page = get(users_script(json!([])), "/web/admin/users").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    for quota in ["memories", "keys", "connections", "requests_per_minute"] {
        assert!(
            page.body.contains(&format!(r#"<option value="{quota}""#)),
            "{quota}: {}",
            page.body
        );
    }
    for quota in ["text_bytes", "writes_per_month"] {
        assert!(
            !page.body.contains(&format!(r#"<option value="{quota}""#)),
            "{quota}: {}",
            page.body
        );
    }
    assert!(
        page.body.contains(
            "Text stored and writes are not set here: they follow the profile of the account, which an admin key sets with <code>PUT /v2/admin/accounts/{account}/profile</code>."
        ),
        "{}",
        page.body
    );
}

fn users_script(users: Value) -> Script {
    Arc::new(move |path| {
        if path == "/v2/web/users" {
            (200, json!({"users": users.clone(), "deletions": []}))
        } else {
            (
                500,
                json!({"error": {"code": "internal.error", "message": path}}),
            )
        }
    })
}

#[tokio::test]
async fn the_users_page_shows_each_persons_meters_with_their_marks_and_a_dash_for_one_not_served() {
    let users = json!([
        {
            "user": "alice", "signed_up": true, "disabled": false, "under_quotas": true,
            "usage": {
                "memories": 1000, "text_bytes": 900, "writes": 7, "keys": 1, "connections": 0,
            },
            "quotas": {
                "memories": 1000, "text_bytes": 1000, "writes_per_month": 100,
                "keys": 5, "connections": 5, "requests_per_minute": 60,
            },
            "admin_keys": [],
        },
        {
            "user": "bob", "signed_up": true, "disabled": false, "under_quotas": true,
            "usage": {"memories": 3, "keys": 1, "connections": 0},
            "quotas": {
                "memories": 1000, "keys": 5, "connections": 5, "requests_per_minute": 60,
            },
            "admin_keys": [],
        },
    ]);
    let page = get(users_script(users), "/web/admin/users").await;
    // The admin credential resolves for the page's own calls only where the
    // session check does; the double answers whoami for any capacity.
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    for heading in ["text stored", "writes"] {
        assert!(
            page.body.contains(&format!("<th>{heading}</th>")),
            "{heading}: {}",
            page.body
        );
    }
    let alice = row_of_user(&page.body, "alice");
    assert!(
        alice.contains(r#"mono">1000 / 1000</span> <span class="oml-chip is-rejected">limit"#),
        "{alice}"
    );
    assert!(
        alice.contains(r#"mono">900 B / 1.0 kB</span> <span class="oml-chip is-held">80%"#),
        "{alice}"
    );
    // Writes: 7 of 100, under both marks.
    assert!(alice.contains(r#"mono">7 / 100</span></td>"#), "{alice}");
    let bob = row_of_user(&page.body, "bob");
    assert!(bob.contains(r#"mono">3 / 1000</span></td>"#), "{bob}");
    assert_eq!(bob.matches("<td>—</td>").count(), 2, "{bob}");
    assert!(!bob.contains("oml-chip"), "{bob}");
}

#[tokio::test]
async fn the_users_page_marks_no_meter_of_a_person_whose_limits_do_not_hold() {
    let row = |user: &str, under_quotas: bool| {
        json!({
            "user": user, "signed_up": false, "disabled": false, "under_quotas": under_quotas,
            "usage": {
                "memories": 800, "text_bytes": 1000, "writes": 0, "keys": 7, "connections": 0,
            },
            "quotas": {
                "memories": 1000, "text_bytes": 1000, "writes_per_month": 100,
                "keys": 5, "connections": 5, "requests_per_minute": 60,
            },
            "admin_keys": [],
        })
    };
    let users = json!([row("held", true), row("free", false)]);
    let page = get(users_script(users), "/web/admin/users").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);

    // Under the limits: seven keys of five is at the limit, and marked.
    let held = row_of_user(&page.body, "held");
    assert!(
        held.contains(r#"mono">7 / 5</span> <span class="oml-chip is-rejected">limit"#),
        "{held}"
    );
    assert!(
        held.contains(r#"mono">800 / 1000</span> <span class="oml-chip is-held">80%"#),
        "{held}"
    );

    // Outside them: the same counts and limits, and no mark, since a
    // seventh key was accepted and an eighth will be.
    let free = row_of_user(&page.body, "free");
    for cell in ["7 / 5", "800 / 1000", "1.0 kB / 1.0 kB"] {
        assert!(free.contains(cell), "{cell}: {free}");
    }
    assert!(!free.contains("oml-chip"), "{free}");
    assert!(
        page.body.contains("for a person the limits hold for"),
        "{}",
        page.body
    );
}

/// The users table's row of `user`.
fn row_of_user(page: &str, user: &str) -> String {
    let start = page
        .find(&format!(r#"<tr><td class="mono">{user}"#))
        .unwrap_or_else(|| panic!("no row of {user} in {page}"));
    let end = page[start..].find("</tr>").unwrap() + start;
    page[start..end].to_owned()
}

#[tokio::test]
async fn a_quota_the_api_serves_as_null_is_unlimited_and_draws_no_bar_and_no_mark() {
    let mut person = person_under_limits();
    for quota in ["memories", "keys", "connections", "requests_per_minute"] {
        person["quotas"][quota] = Value::Null;
    }
    let page = get(start_script(person), "/web/start").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    let memories = row(&page.body, "Memories");
    assert!(
        memories.contains(r#"<span class="mono">800</span>"#),
        "{memories}"
    );
    assert!(
        !memories.contains("<meter") && !memories.contains("oml-chip"),
        "{memories}"
    );
    assert!(row(&page.body, "Keys").contains(r#"<span class="mono">2</span>"#));
    let rate = row(&page.body, "Requests, per key");
    assert!(!rate.contains("/ min") && rate.contains("—"), "{rate}");

    let users = json!([{
        "user": "alice", "signed_up": true, "disabled": false, "under_quotas": true,
        "usage": {"memories": 3, "text_bytes": 0, "writes": 0, "keys": 1, "connections": 0},
        "quotas": {
            "memories": null, "keys": null, "connections": null,
            "requests_per_minute": null, "text_bytes": null, "writes_per_month": null,
        },
        "admin_keys": [],
    }]);
    let page = get(users_script(users), "/web/admin/users").await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    let alice = row_of_user(&page.body, "alice");
    assert!(
        alice.contains(r#"<td><span class="mono">3</span></td>"#),
        "{alice}"
    );
    assert!(!alice.contains("oml-chip"), "{alice}");
    assert!(alice.contains("<td>—</td>"), "{alice}");
}
