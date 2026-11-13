//! The reads the web console's pages open with, counted in store calls on
//! the lead's request line (ADR-0042 d14), and what they serve to whom
//! (ADR-0070 d5, d8; #286): `GET /v2/whoami` resolves its credential once
//! and reads the person block only when asked, and the Atlas's counted tree
//! serves the scope's offers beside its counts, as the offers operation
//! serves them to the same caller. Over the in-memory store, so it runs
//! without Postgres.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt as _;

use oml_app::App;
use oml_app::domain::name::Name;
use oml_app::domain::web::{Email, capacity_credential};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::types::{BootstrapRequest, WebEmailRequest};
use oml_server_lead::log_subscriber;
use oml_server_lead::timed::Timed;
use oml_transport_http::ApiState;
use oml_transport_http::session::WebClient;
use oml_transport_http::traffic::ScopeObserver;

/// The web-client secret the lead and these calls share.
const SECRET: &str = "oml-web-client-secret-for-the-tests-0001";

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

type TestApp = App<Timed<MemStore>, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn app(store: &Timed<MemStore>) -> TestApp {
    App::new(
        store.clone(),
        MemSigner::new([3; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_observer(Arc::new(ScopeObserver))
}

/// A lead that accepts oml-web's capacity credentials, its tenant's admin
/// key value, and the session token of `alice`, an admin entry, and of
/// `bob`, a person who is no admin.
struct Rig {
    router: Router,
    admin: String,
    alice: String,
    bob: String,
}

async fn rig() -> Rig {
    let store = Timed::new(MemStore::new());
    let boot = app(&store)
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap, outside any request");
    let sessions = app(&store);
    let mut tokens = Vec::new();
    for (email, user, admin) in [
        ("alice@example.com", "alice", true),
        ("bob@example.com", "bob", false),
    ] {
        let (entry, _) = sessions
            .allow_web_email(
                boot.admin_key.value.as_str(),
                &WebEmailRequest {
                    email: Email::parse(email).expect("an email"),
                    user: Name::parse(user).expect("a user"),
                    admin,
                },
            )
            .await
            .expect("the entry is added");
        sessions
            .web_ensure_keys(&entry)
            .await
            .expect("the session keys");
        tokens.push(
            sessions
                .web_session_open(&entry, 3600, None)
                .await
                .expect("the session opens")
                .token,
        );
    }
    let router = ApiState::new(
        Arc::new(app(&store)),
        store,
        Arc::new(|| Box::pin(async { true })),
        true,
        None,
    )
    .with_web_client(WebClient::new(Some(SECRET.to_owned()), None))
    .into_router();
    let bob = tokens.pop().expect("bob's token");
    let alice = tokens.pop().expect("alice's token");
    Rig {
        router,
        admin: boot.admin_key.value,
        alice,
        bob,
    }
}

/// One answer: its status, its body and its request line.
struct Answer {
    status: StatusCode,
    body: Value,
    line: Value,
}

impl Answer {
    fn store_calls(&self) -> u64 {
        self.line["store_calls"].as_u64().expect("a count")
    }
}

/// `GET path` with `bearer`, and the web client's secret where `web`.
async fn get(router: &Router, path: &str, bearer: &str, web: bool) -> Answer {
    send(router, Request::get(path), "", bearer, web).await
}

/// `request` with `body`, `bearer`, and the web client's secret where
/// `web`.
async fn send(
    router: &Router,
    request: axum::http::request::Builder,
    body: &'static str,
    bearer: &str,
    web: bool,
) -> Answer {
    let mut request = request
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json");
    if web {
        request = request.header("oml-web-client", SECRET);
    }
    let buf = Buf::default();
    let sink = buf.clone();
    let guard = tracing::subscriber::set_default(log_subscriber(move || sink.clone()));
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body)).expect("a request"))
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    drop(guard);
    let text = String::from_utf8(buf.0.lock().expect("unpoisoned").clone()).expect("UTF-8");
    let line = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).expect("a JSON line"))
        .find(|l| l["event"] == "http_request")
        .expect("the request line");
    Answer {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        line,
    }
}

/// `POST path` with an empty JSON object, as [`get`] sends a `GET`.
async fn post(router: &Router, path: &str, bearer: &str, web: bool) -> Answer {
    send(router, Request::post(path), "{}", bearer, web).await
}

fn web(token: &str, capacity: &str) -> String {
    capacity_credential(token, capacity)
}

// ---------------------------------------------------------------------------
// GET /v2/whoami
// ---------------------------------------------------------------------------

/// The session check's `whoami`, without the person block: the record a
/// session key's caller reads, in the session gate's one call, which
/// reads the session, the key, its access and for `web` its root and the
/// person's own keys, and one more, the record's references. The gate
/// made six calls for `web` and four for `admin` before, and `whoami`
/// read the access and the root again (#286, #297).
#[tokio::test]
async fn whoami_without_the_person_block_reads_the_session_keys_access_once() {
    let rig = rig().await;
    for (capacity, gate, own) in [("web", 1, 1), ("admin", 1, 1)] {
        let me = get(
            &rig.router,
            "/v2/whoami?person=false",
            &web(&rig.alice, capacity),
            true,
        )
        .await;
        assert_eq!(me.status, StatusCode::OK, "{}", me.body);
        assert_eq!(me.body["name"], format!("alice-{capacity}"), "{}", me.body);
        assert!(me.body.get("person").is_none(), "{}", me.body);
        assert_eq!(me.store_calls(), gate + own, "{capacity}: {}", me.line);
    }
}

/// A session key's read without a body costs the gate's one store call
/// and the verb's own: its first resolution of the caller decides on the
/// access the gate read. Where one request resolves the caller twice, as
/// the counted tree with `offers=true` does, the second reads the access
/// again, and a request with a body reads it when its verb runs (#297).
#[tokio::test]
async fn a_session_keys_read_resolves_its_caller_on_the_gates_access_once_and_a_request_with_a_body_reads_it_again()
 {
    let rig = rig().await;
    let admin = web(&rig.alice, "admin");
    let tree = get(
        &rig.router,
        "/v2/workareas/universe/tree?counts=true",
        &admin,
        true,
    )
    .await;
    let offers = get(&rig.router, "/v2/workareas/universe/offers", &admin, true).await;
    let both = get(
        &rig.router,
        "/v2/workareas/universe/tree?counts=true&offers=true",
        &admin,
        true,
    )
    .await;
    assert_eq!(tree.status, StatusCode::OK, "{}", tree.body);
    assert_eq!(offers.status, StatusCode::OK, "{}", offers.body);
    assert_eq!(both.status, StatusCode::OK, "{}", both.body);
    // One gate call each; the tree and the offers read apart resolve the
    // caller once each on the gate's read, and together one of them reads
    // the admin key's access again: one call more than their own work.
    let own = (tree.store_calls() - 1) + (offers.store_calls() - 1);
    assert_eq!(both.store_calls(), 1 + own + 1, "{}", both.line);

    // The same search with and without the gate's read to decide on: the
    // key value resolves and reads its access (two calls), the session key
    // on a POST is resolved by the gate (one) and reads its access when
    // the verb runs (one more).
    let by_value = post(&rig.router, "/v2/admin/memories/search", &rig.admin, false).await;
    let by_session = post(&rig.router, "/v2/admin/memories/search", &admin, true).await;
    assert_eq!(by_value.status, StatusCode::OK, "{}", by_value.body);
    assert_eq!(by_session.status, StatusCode::OK, "{}", by_session.body);
    assert_eq!(
        by_session.store_calls(),
        by_value.store_calls(),
        "{} / {}",
        by_session.line,
        by_value.line
    );
}

/// A `<user>-web` session key's `whoami` says whether the key carries a denial, from the
/// access the gate already read: the calls the Keys page and the Start page
/// make cost the store calls they cost before the field, and a denial on
/// the key changes none of them (ADR-0070 d2, d8; #343).
#[tokio::test]
async fn whoami_names_a_denied_session_key_in_the_store_calls_it_already_makes() {
    let rig = rig().await;
    let calls = [
        ("/v2/whoami?person=false", "web"),
        ("/v2/whoami?person=false", "admin"),
        ("/v2/keys", "web"),
        ("/v2/whoami", "web"),
        ("/v2/whoami?start=true", "web"),
    ];
    let mut before = Vec::new();
    for (path, capacity) in calls {
        let answer = get(&rig.router, path, &web(&rig.alice, capacity), true).await;
        assert_eq!(answer.status, StatusCode::OK, "{path}: {}", answer.body);
        if path.starts_with("/v2/whoami") {
            // `alice-web` alone carries the member: `alice-admin` has none.
            let told = (capacity == "web").then_some(false);
            assert_eq!(
                answer.body.get("denied").and_then(Value::as_bool),
                told,
                "{path} as {capacity}: {}",
                answer.body
            );
        }
        before.push(answer.store_calls());
    }
    let denial = send(
        &rig.router,
        Request::post("/v2/keys/alice-web/denials").header("idempotency-key", "deny-alice-web"),
        r#"{"workarea":"universe"}"#,
        &rig.admin,
        false,
    )
    .await;
    assert_eq!(denial.status, StatusCode::CREATED, "{}", denial.body);
    let mut after = Vec::new();
    for (path, capacity) in calls {
        let answer = get(&rig.router, path, &web(&rig.alice, capacity), true).await;
        assert_eq!(answer.status, StatusCode::OK, "{path}: {}", answer.body);
        if path.starts_with("/v2/whoami") {
            let told = (capacity == "web").then_some(true);
            assert_eq!(
                answer.body.get("denied").and_then(Value::as_bool),
                told,
                "{path} as {capacity}: {}",
                answer.body
            );
        }
        after.push(answer.store_calls());
    }
    println!("store calls per call {calls:?}: before {before:?}, after {after:?}");
    assert_eq!(after, before, "a denial adds no store call to any read");
}

/// A key value's `whoami` without the person block: the key's lookup, its
/// access, and the record's references, one read each (#286).
#[tokio::test]
async fn whoami_of_a_key_value_without_the_person_block_reads_each_row_once() {
    let rig = rig().await;
    let me = get(&rig.router, "/v2/whoami?person=false", &rig.admin, false).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);
    assert_eq!(me.body["name"], "root-admin", "{}", me.body);
    assert!(me.body.get("person").is_none(), "{}", me.body);
    assert_eq!(me.store_calls(), 3, "{}", me.line);
}

/// Without `person=false`, `whoami` serves the person block as it did, in
/// one resolution of the credential: the record's reads and the person's
/// own (their deletion request, and their account's profile, which alone
/// puts an account outside the sign-up tenant under limits; ADR-0071 d5).
#[tokio::test]
async fn whoami_serves_the_person_block_unless_asked_not_to_in_one_resolution() {
    let rig = rig().await;
    let me = get(&rig.router, "/v2/whoami", &web(&rig.alice, "web"), true).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);
    assert_eq!(me.body["person"]["user"], "alice", "{}", me.body);
    assert_eq!(me.body["person"]["under_quotas"], false, "{}", me.body);
    assert_eq!(me.body["person"]["deletion_requested_at"], Value::Null);
    assert!(me.body["person"].get("start").is_none(), "{}", me.body);
    assert_eq!(me.store_calls(), 1 + 1 + 2, "{}", me.line);
    let asked = get(
        &rig.router,
        "/v2/whoami?person=true",
        &web(&rig.alice, "web"),
        true,
    )
    .await;
    assert_eq!(asked.body, me.body);
    let started = get(
        &rig.router,
        "/v2/whoami?start=true",
        &web(&rig.alice, "web"),
        true,
    )
    .await;
    assert_eq!(started.status, StatusCode::OK, "{}", started.body);
    assert_eq!(
        started.body["person"]["start"]["connected"], false,
        "{}",
        started.body
    );
    // A tenant key names no person.
    let admin = get(&rig.router, "/v2/whoami", &rig.admin, false).await;
    assert_eq!(admin.body["person"], Value::Null, "{}", admin.body);
}

/// The person block goes to no one `whoami` refused before: a capacity
/// credential outside oml-web, a capacity the entry does not reach and an
/// unknown key are each `auth.invalid` whether or not the block is asked
/// for, and a person's credential reads only their own (#286: no
/// authority change).
#[tokio::test]
async fn the_person_block_is_refused_to_every_caller_whoami_refused_before() {
    let rig = rig().await;
    for query in ["", "?person=true", "?person=false", "?start=true"] {
        let path = format!("/v2/whoami{query}");
        for (bearer, web_client) in [
            (web(&rig.alice, "web"), false),
            (web(&rig.bob, "admin"), true),
            ("oml_not_a_real_key".to_owned(), false),
        ] {
            let refused = get(&rig.router, &path, &bearer, web_client).await;
            assert_eq!(
                refused.status,
                StatusCode::UNAUTHORIZED,
                "{path}: {}",
                refused.body
            );
            assert_eq!(
                refused.body["error"]["code"], "auth.invalid",
                "{}",
                refused.body
            );
            assert!(refused.body.get("person").is_none(), "{}", refused.body);
        }
        let bob = get(&rig.router, &path, &web(&rig.bob, "web"), true).await;
        assert_eq!(bob.status, StatusCode::OK, "{path}: {}", bob.body);
        assert_eq!(bob.body["name"], "bob-web", "{}", bob.body);
        if query != "?person=false" {
            assert_eq!(bob.body["person"]["user"], "bob", "{}", bob.body);
        }
    }
}

/// `person` is a boolean, and `start=true` reads the person block, so it
/// does not go with `person=false`: both refused by name.
#[tokio::test]
async fn whoami_refuses_a_person_flag_it_cannot_serve() {
    let rig = rig().await;
    for path in [
        "/v2/whoami?person=maybe",
        "/v2/whoami?person=false&start=true",
    ] {
        let refused = get(&rig.router, path, &rig.admin, false).await;
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{path}: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["code"], "request.validation",
            "{}",
            refused.body
        );
    }
}

// ---------------------------------------------------------------------------
// GET /v2/workareas/{workarea}/tree?counts=true&offers=true
// ---------------------------------------------------------------------------

/// The counted tree with `offers=true` serves the scope's offers exactly as
/// `GET /v2/workareas/{workarea}/offers` serves them to the same caller,
/// beside the counts it serves without them, for an admin key value and
/// for each capacity of a session (#286: the admin Atlas's extra round).
#[tokio::test]
async fn the_counted_tree_serves_the_scopes_offers_as_the_offers_operation_does() {
    let rig = rig().await;
    for (bearer, web_client, scope) in [
        (rig.admin.clone(), false, "universe"),
        (web(&rig.alice, "admin"), true, "universe"),
        (web(&rig.alice, "review"), true, "alice"),
        (web(&rig.alice, "web"), true, "alice"),
    ] {
        let counted = get(
            &rig.router,
            &format!("/v2/workareas/{scope}/tree?counts=true"),
            &bearer,
            web_client,
        )
        .await;
        let both = get(
            &rig.router,
            &format!("/v2/workareas/{scope}/tree?counts=true&offers=true"),
            &bearer,
            web_client,
        )
        .await;
        let offers = get(
            &rig.router,
            &format!("/v2/workareas/{scope}/offers"),
            &bearer,
            web_client,
        )
        .await;
        assert_eq!(counted.status, StatusCode::OK, "{}", counted.body);
        assert_eq!(both.status, StatusCode::OK, "{}", both.body);
        assert_eq!(offers.status, StatusCode::OK, "{}", offers.body);
        assert_eq!(both.body["items"], counted.body["items"], "{scope}");
        assert_eq!(both.body["truncated"], counted.body["truncated"]);
        assert_eq!(both.body["offers"], offers.body, "{scope}");
        assert!(counted.body.get("offers").is_none(), "{}", counted.body);
    }
}

/// A scope the caller cannot read answers the counted tree's refusal with
/// or without `offers=true`, and names no offers (#286: no authority
/// change).
#[tokio::test]
async fn the_counted_trees_offers_go_to_no_caller_the_tree_refuses() {
    let rig = rig().await;
    for offers in ["", "&offers=true"] {
        let refused = get(
            &rig.router,
            &format!("/v2/workareas/alice/tree?counts=true{offers}"),
            &web(&rig.bob, "web"),
            true,
        )
        .await;
        assert_eq!(refused.status, StatusCode::NOT_FOUND, "{}", refused.body);
        assert_eq!(
            refused.body["error"]["code"], "access.not_found",
            "{}",
            refused.body
        );
        assert!(refused.body.get("offers").is_none(), "{}", refused.body);
    }
}

/// `offers` is a boolean, served only beside `counts=true`, whose whole
/// subtree has one scope: anything else is refused by name.
#[tokio::test]
async fn the_tree_refuses_offers_it_cannot_serve() {
    let rig = rig().await;
    for path in [
        "/v2/workareas/universe/tree?offers=true",
        "/v2/workareas/universe/tree?counts=false&offers=true",
        "/v2/workareas/universe/tree?counts=true&offers=maybe",
    ] {
        let refused = get(&rig.router, path, &rig.admin, false).await;
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{path}: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["code"], "request.validation",
            "{}",
            refused.body
        );
    }
}

/// Across every caller and scope of the rig, the counted tree with
/// `offers=true` serves exactly where both the counted tree and the offers
/// operation serve that caller, and then what each serves; wherever either
/// refuses, it is refused with the refusal of the one that refused (the
/// tree's first) and serves neither counts nor offers. An admin key on a
/// person's root is served by both, the admin key role reading every
/// workarea's structure and offers (#286: the field widens nothing).
#[tokio::test]
async fn the_counted_trees_offers_serve_only_where_the_tree_and_the_offers_operation_both_serve() {
    let rig = rig().await;
    let callers = [
        (rig.admin.clone(), false),
        (web(&rig.alice, "web"), true),
        (web(&rig.alice, "review"), true),
        (web(&rig.alice, "admin"), true),
        (web(&rig.bob, "web"), true),
        (web(&rig.bob, "review"), true),
    ];
    let (mut served, mut refused) = (0, 0);
    for (bearer, web_client) in &callers {
        for scope in ["universe", "alice", "bob", "nowhere"] {
            let at = |q: &str| format!("/v2/workareas/{scope}/{q}");
            let tree = get(&rig.router, &at("tree?counts=true"), bearer, *web_client).await;
            let offers = get(&rig.router, &at("offers"), bearer, *web_client).await;
            let both = get(
                &rig.router,
                &at("tree?counts=true&offers=true"),
                bearer,
                *web_client,
            )
            .await;
            if tree.status == StatusCode::OK && offers.status == StatusCode::OK {
                served += 1;
                assert_eq!(both.status, StatusCode::OK, "{scope}: {}", both.body);
                assert_eq!(both.body["items"], tree.body["items"], "{scope}");
                assert_eq!(both.body["offers"], offers.body, "{scope}");
                continue;
            }
            refused += 1;
            let first = if tree.status == StatusCode::OK {
                &offers
            } else {
                &tree
            };
            assert_eq!(both.status, first.status, "{scope}: {}", both.body);
            assert_eq!(
                both.body["error"]["code"], first.body["error"]["code"],
                "{scope}: {}",
                both.body
            );
            assert!(both.body.get("items").is_none(), "{}", both.body);
            assert!(both.body.get("offers").is_none(), "{}", both.body);
        }
    }
    assert!(
        served > 0 && refused > 0,
        "{served} served, {refused} refused"
    );
    // The case the review named: an admin key on a person's root.
    let admin_on_root = get(
        &rig.router,
        "/v2/workareas/alice/tree?counts=true&offers=true",
        &rig.admin,
        false,
    )
    .await;
    assert_eq!(
        admin_on_root.status,
        StatusCode::OK,
        "{}",
        admin_on_root.body
    );
}

// ---------------------------------------------------------------------------
// The Review pages' reads (bulk D, the review lane)
// ---------------------------------------------------------------------------

/// The reads the person's Review page and its one-item page add (design
/// notes rows 1, 4, 6, 15), each a read the API already serves, so each
/// costs the gate's one store call and the verb's own: the held listing as
/// `<user>-review` at the API's largest page (the list and "Waiting for
/// review" from one read), the person's key list as `<user>-web` (whether
/// an item's author is one of the person's keys), and the replaced
/// version as `<user>-review` (the change at page density). Printed, so
/// the counts the report states are the ones measured.
#[tokio::test]
async fn the_review_pages_reads_cost_the_gate_and_their_own_verbs_store_calls() {
    let rig = rig().await;
    let held = get(
        &rig.router,
        "/v2/review/held?limit=200",
        &web(&rig.alice, "review"),
        true,
    )
    .await;
    assert_eq!(held.status, StatusCode::OK, "{}", held.body);
    let keys = get(
        &rig.router,
        "/v2/keys?limit=200",
        &web(&rig.alice, "web"),
        true,
    )
    .await;
    assert_eq!(keys.status, StatusCode::OK, "{}", keys.body);
    let named = get(
        &rig.router,
        "/v2/review/held?ids=00000000-0000-4000-8000-000000000001",
        &web(&rig.alice, "review"),
        true,
    )
    .await;
    assert_eq!(named.status, StatusCode::OK, "{}", named.body);
    println!(
        "store calls: held listing {}, key list {}, named held {}",
        held.store_calls(),
        keys.store_calls(),
        named.store_calls()
    );
    // Measured 2026-11-13 over the in-memory store: the gate's one call
    // and the listing's three; the key list's eleven, as the Keys page
    // pays them; the named read's three.
    assert_eq!(held.store_calls(), 4, "{}", held.line);
    assert_eq!(keys.store_calls(), 12, "{}", keys.line);
    assert_eq!(named.store_calls(), 4, "{}", named.line);
    // The held listing at 200 a page costs what it costs at 50: the page
    // size changes no store call.
    let fifty = get(
        &rig.router,
        "/v2/review/held?limit=50",
        &web(&rig.alice, "review"),
        true,
    )
    .await;
    assert_eq!(held.store_calls(), fifty.store_calls(), "{}", held.line);
    assert_eq!(
        keys.store_calls(),
        get(&rig.router, "/v2/keys", &web(&rig.alice, "web"), true)
            .await
            .store_calls()
    );
}
