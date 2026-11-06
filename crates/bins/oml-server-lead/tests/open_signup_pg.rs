//! Open sign-up over the real lead and a migrated Postgres store under its
//! role pair (ADR-0068): the terms page, the private root, isolation from a
//! second person on every surface that exists, review by the person's own
//! reviewer key marked `same_user`, the quotas and the request rate, the
//! deletion request and the disable switch, and the per-address limit. The
//! sign-in provider is the in-memory double. Each test is named after the
//! sentence it proves.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;

use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::oauth::Issuer;
use oml_app::domain::signup::{Quotas, SignupMode};
use oml_app::domain::web::csrf_token;
use oml_app::domain::web::{GitHubEmail, Provider};
use oml_app::doubles::MemSignIn;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{IdGen as _, JournalStore as _};
use oml_app::types::{BootstrapRequest, BootstrapResult, ProviderAnswer, SignupPolicy};
use oml_fetch::PinnedFetcher;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_app, start_lead_with};
use oml_transport_http::traffic::Traffic;
use oml_transport_http::web::start::SIGN_UP_COOKIE;
use oml_transport_http::web::{SESSION_COOKIE, SIGN_IN_COOKIE, WebPages};

const ISSUER: &str = "https://oml.example.com";

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL names a throwaway PostgreSQL cluster; no suite defaults to one, so none reaches a live server")
}

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
const PAIR_PASSWORD: &str = "oml_test_pair";

/// `db` at `DATABASE_URL`'s host, logged in as `role`.
fn role_url(db: &str, role: &str) -> String {
    let base = base_url();
    let (scheme, rest) = base
        .split_once("://")
        .expect("DATABASE_URL is a postgres:// URL");
    let (_, host_path) = rest.rsplit_once('@').expect("DATABASE_URL names its user");
    let (host, _) = host_path
        .split_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{scheme}://{role}:{PAIR_PASSWORD}@{host}/{db}")
}

/// `db` through the owner login of its role pair (ADR-0034 d2, d8).
fn owner_url(db: &str) -> String {
    role_url(db, &format!("{db}_owner"))
}

/// `db` through the serving login of its role pair (ADR-0034 d2, d8).
fn serving_url(db: &str) -> String {
    role_url(db, &format!("{db}_serving"))
}

/// Eight hex digits of the clock, so a name a crashed earlier run left
/// behind on the cluster under the same pid and counter is not reused.
fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

fn fresh_name() -> String {
    format!(
        "oml_test_signup_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

/// Mints `db`'s owner/serving role pair, `LOGIN` and no other attribute,
/// and creates `db` owned by the owner (ADR-0034 d2, d8); `false`, with
/// `did not run: <role> cannot create roles` printed, when `DATABASE_URL`'s
/// role is not the superuser minting takes, as the maintainer's local `oml`
/// (CREATEDB alone) is not. Under `OML_TEST_REQUIRE_SPLIT=1`, which CI sets,
/// the same condition fails the test, so no run passes vacuously.
async fn mint_database(db: &str) -> bool {
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    let (role, mints): (String, bool) = sqlx::query_as(
        "SELECT current_user::text, rolsuper FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut admin)
    .await
    .expect("role attributes");
    if !mints {
        assert!(
            std::env::var("OML_TEST_REQUIRE_SPLIT").as_deref() != Ok("1"),
            "{role} cannot create roles, and OML_TEST_REQUIRE_SPLIT=1 requires the role pair (ADR-0034 d8)"
        );
        println!("did not run: {role} cannot create roles");
        return false;
    }
    for role in ["owner", "serving"] {
        admin
            .execute(format!("CREATE ROLE {db}_{role} LOGIN PASSWORD '{PAIR_PASSWORD}'").as_str())
            .await
            .expect("mint the role pair");
    }
    admin
        .execute(format!("CREATE DATABASE {db} OWNER {db}_owner").as_str())
        .await
        .expect("create throwaway database");
    true
}

/// A store over `db`'s serving login, migrated through its owner login with
/// `oml.serving_role` set (ADR-0034 d4, d8).
async fn serving_store(db: &str) -> PgStore {
    let store = PgStore::connect(&serving_url(db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(db));
    store
        .migrate(&owner_url(db))
        .await
        .expect("embedded migrations apply");
    store
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
        for role in ["serving", "owner"] {
            let _ = admin
                .execute(format!("DROP ROLE IF EXISTS {name}_{role}").as_str())
                .await;
        }
    }
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a readable body")
        .to_vec();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

fn set_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| {
            v[name.len() + 1..]
                .split(';')
                .next()
                .unwrap_or("")
                .to_owned()
        })
}

fn state_of(location: &str) -> String {
    location
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("state="))
        .expect("a state")
        .to_owned()
}

async fn tenant(store: &PgStore, key: &SigningKey, name: &str) -> BootstrapResult {
    lead_app(store, key.clone())
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse(name).expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: format!("{name}-admin"),
        })
        .await
        .expect("bootstrap")
}

const TERMS: &str = "2026-11-06";

fn policy(tenant_id: uuid::Uuid) -> SignupPolicy {
    SignupPolicy {
        mode: SignupMode::Open,
        tenant_id,
        issuer: Name::parse("root-admin").expect("a name"),
        terms_version: TERMS.to_owned(),
        terms_url: "https://example.com/terms".to_owned(),
        privacy_url: "https://example.com/privacy".to_owned(),
        per_ip_per_hour: 100,
        quotas: Quotas::default(),
    }
}

struct Rig {
    db: String,
    store: PgStore,
    boot: BootstrapResult,
    providers: MemSignIn,
    traffic: Traffic,
    router: Router,
}

/// A migrated store, the tenant `oml`, and the lead with GitHub sign-in
/// and, unless `open` is `None`, open sign-up into that tenant under the
/// policy `open` makes of the default one.
async fn rig(open: Option<fn(SignupPolicy) -> SignupPolicy>) -> Option<Rig> {
    let db = fresh_name();
    if !mint_database(&db).await {
        return None;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key, "oml").await;
    let providers = MemSignIn::new();
    let traffic = Traffic::default();
    let mut pages = WebPages::new(
        vec![(Provider::GitHub, "github-client".to_owned())],
        providers.clone(),
    );
    if let Some(open) = open {
        pages = pages.with_signup(open(policy(boot.tenant_id)));
    }
    let router = start_lead_with(
        &store,
        key.clone(),
        Some(Issuer::parse(ISSUER).expect("a public origin")),
        PinnedFetcher,
        traffic.clone(),
        Some(pages),
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
    )
    .await
    .expect("the lead starts")
    .router;
    Some(Rig {
        db,
        store,
        boot,
        providers,
        traffic,
        router,
    })
}

fn form(path: &str, cookie: Option<&str>, body: &str) -> Request<Body> {
    let mut b = Request::post(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin");
    if let Some(cookie) = cookie {
        b = b.header(header::COOKIE, cookie);
    }
    b.body(Body::from(body.to_owned())).expect("a request")
}

fn page(path: &str, cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::get(path);
    if let Some(cookie) = cookie {
        b = b.header(header::COOKIE, cookie);
    }
    b.body(Body::empty()).expect("a request")
}

fn api(method: &str, path: &str, key: &str, body: Option<Value>) -> Request<Body> {
    let b = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header("idempotency-key", format!("k-{}", fresh_name()));
    match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string())),
        None => b.body(Body::empty()),
    }
    .expect("a request")
}

/// GitHub's callback for account `id` with the primary verified `email`:
/// its status, headers and body.
async fn callback(rig: &Rig, id: u64, email: &str) -> (StatusCode, HeaderMap, String) {
    let (status, headers, _) =
        send(&rig.router, form("/web/sign-in", None, "provider=github")).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let state = state_of(headers[header::LOCATION].to_str().expect("ASCII"));
    let sealed = set_cookie(&headers, SIGN_IN_COOKIE).expect("the sign-in cookie");
    rig.providers.answer(
        Provider::GitHub,
        Ok(ProviderAnswer::GitHub {
            account_id: id,
            emails: vec![GitHubEmail {
                email: email.to_owned(),
                primary: true,
                verified: true,
            }],
        }),
    );
    send(
        &rig.router,
        page(
            &format!("/web/callback/github?code=c&state={state}"),
            Some(&format!("{SIGN_IN_COOKIE}={sealed}")),
        ),
    )
    .await
}

/// A whole open sign-up: the callback's terms page, then its accepted
/// form; the session cookie, as a `Cookie` header value.
async fn sign_up(rig: &Rig, id: u64, email: &str) -> String {
    let (status, headers, body) = callback(rig, id, email).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let pending = set_cookie(&headers, SIGN_UP_COOKIE).expect("the sign-up cookie");
    let (status, headers, body) = send(
        &rig.router,
        form(
            "/web/sign-up",
            Some(&format!("{SIGN_UP_COOKIE}={pending}")),
            &format!("accept_terms=yes&terms_version={TERMS}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(headers[header::LOCATION], "/web/start");
    format!(
        "{SESSION_COOKIE}={}",
        set_cookie(&headers, SESSION_COOKIE).expect("a session")
    )
}

fn csrf(cookie: &str) -> String {
    csrf_token(cookie.split_once('=').expect("a cookie").1)
}

/// The text of `<pre id="{id}">…</pre>` on a page.
fn pre(html: &str, id: &str) -> String {
    let open = format!(r#"<pre id="{id}">"#);
    let start = html.find(&open).expect("the value is shown") + open.len();
    let end = html[start..].find("</pre>").expect("closed") + start;
    html[start..end].replace("&amp;", "&")
}

/// Issues the Claude Code key from `/web/start`: its value.
async fn claude_code_key(rig: &Rig, cookie: &str) -> String {
    let (status, _, html) = send(
        &rig.router,
        form(
            "/web/start",
            Some(cookie),
            &format!("action=issue_claude_code&csrf_token={}", csrf(cookie)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{html}");
    pre(&html, "cred-key")
}

async fn json(rig: &Rig, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let (status, headers, body) = send(&rig.router, request).await;
    let value = serde_json::from_str(&body).unwrap_or(Value::String(body));
    (status, headers, value)
}

async fn remember(rig: &Rig, key: &str, claim_key: &str, text: &str) -> (StatusCode, Value) {
    let (status, _, body) = json(
        rig,
        api(
            "POST",
            "/v1/memories",
            key,
            Some(
                json!({"verb": "remember", "claim_key": claim_key, "kind": "memory", "body": text}),
            ),
        ),
    )
    .await;
    (status, body)
}

/// The person approves the item their reviewer key is served on `/web`.
async fn approve_next(rig: &Rig, cookie: &str) {
    let (status, _, html) = send(
        &rig.router,
        form(
            "/web",
            Some(cookie),
            &format!("action=review_next&csrf_token={}", csrf(cookie)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let at = html
        .find(r#"name="serve_id" value=""#)
        .expect("an item is served")
        + r#"name="serve_id" value=""#.len();
    let serve_id = &html[at..at + html[at..].find('"').expect("closed")];
    let (status, _, html) = send(
        &rig.router,
        form(
            "/web",
            Some(cookie),
            &format!(
                "action=verdict&csrf_token={}&serve_id={serve_id}&verdict=approve&reason=mine",
                csrf(cookie)
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("is now promoted"), "{html}");
}

async fn events(rig: &Rig) -> Vec<(String, Value)> {
    rig.store
        .events_range(rig.boot.tenant_id, 1, 10_000)
        .await
        .expect("the chain")
        .into_iter()
        .map(|e| (e.envelope.event_type.clone(), e.envelope.payload.clone()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn allowlist_mode_still_refuses_an_email_no_entry_holds() {
    let Some(rig) = rig(None).await else { return };
    let (status, headers, body) = callback(&rig, 1, "stranger@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(body.contains("not on the list"), "{body}");
    assert!(set_cookie(&headers, SIGN_UP_COOKIE).is_none());
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_sign_up_records_the_accepted_terms_and_lands_on_start_with_a_private_root() {
    let Some(rig) = rig(Some(|p| p)).await else {
        return;
    };
    // The callback shows the terms and writes nothing.
    let (status, headers, body) = callback(&rig, 11, "Ada@Example.com").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#"name="accept_terms""#), "{body}");
    assert!(body.contains("https://example.com/terms"), "{body}");
    assert!(!events(&rig).await.iter().any(|(t, _)| t == "web.signed_up"));
    let pending = format!(
        "{SIGN_UP_COOKIE}={}",
        set_cookie(&headers, SIGN_UP_COOKIE).expect("the sign-up cookie")
    );
    // Unaccepted, or a version not shown, is refused and writes nothing.
    let (status, _, body) = send(
        &rig.router,
        form(
            "/web/sign-up",
            Some(&pending),
            &format!("terms_version={TERMS}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _, body) = send(
        &rig.router,
        form(
            "/web/sign-up",
            Some(&pending),
            "accept_terms=yes&terms_version=old",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("terms changed"), "{body}");
    // A form from another origin is refused.
    let (status, _, _) = send(
        &rig.router,
        Request::post("/web/sign-up")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("sec-fetch-site", "cross-site")
            .header(header::COOKIE, &pending)
            .body(Body::from(format!(
                "accept_terms=yes&terms_version={TERMS}"
            )))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!events(&rig).await.iter().any(|(t, _)| t == "web.signed_up"));

    let cookie = sign_up(&rig, 11, "Ada@Example.com").await;
    let journal = events(&rig).await;
    let (_, signed_up) = journal
        .iter()
        .find(|(t, _)| t == "web.signed_up")
        .expect("web.signed_up");
    assert_eq!(signed_up["terms_version"], TERMS);
    assert_eq!(signed_up["self_signup"], true);
    assert_eq!(signed_up["admin"], false);
    assert!(
        !signed_up
            .to_string()
            .to_ascii_lowercase()
            .contains("example.com"),
        "{signed_up}"
    );
    let user = signed_up["user"].as_str().expect("a user").to_owned();
    assert!(user.starts_with('u') && user.len() == 11, "{user}");
    assert!(!user.contains("ada"));
    // The root and the two session keys exist under the drawn user.
    for key in [format!("{user}-web"), format!("{user}-review")] {
        assert!(
            rig.store
                .resolve_name(
                    rig.boot.tenant_id,
                    NamedKind::Key,
                    &Name::parse(&key).unwrap()
                )
                .await
                .unwrap()
                .is_some(),
            "{key}"
        );
    }
    let (status, _, html) = send(&rig.router, page("/web/start", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Waiting for your first connection"), "{html}");
    assert!(html.contains("1. Choose your client"));
    // A second sign-in of the same account is a sign-in, not a sign-up.
    let (status, headers, _) = callback(&rig, 11, "ada@example.com").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/start", "no agent key yet");
    assert_eq!(
        events(&rig)
            .await
            .iter()
            .filter(|(t, _)| t == "web.signed_up")
            .count(),
        1
    );
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_person_cannot_see_count_or_recall_the_first_persons_memories() {
    let Some(rig) = rig(Some(|p| p)).await else {
        return;
    };
    let ada = sign_up(&rig, 21, "ada@example.com").await;
    let bob = sign_up(&rig, 22, "bob@example.com").await;
    let ada_key = claude_code_key(&rig, &ada).await;
    let bob_key = claude_code_key(&rig, &bob).await;
    let secret = "The Zanzibar launch slips to March because the vendor audit failed.";
    let (status, receipt) = remember(&rig, &ada_key, "zanzibar-launch", secret).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    let artifact = receipt["artifact_id"].as_str().expect("an id").to_owned();
    // Ada reviews her agent's write through her reviewer key, a key that
    // is not the author, and the verdict is marked same_user (d4).
    approve_next(&rig, &ada).await;
    let journal = events(&rig).await;
    let (_, promote) = journal
        .iter()
        .rev()
        .find(|(t, _)| t == "lifecycle.promote")
        .expect("the approval promotes");
    assert_eq!(promote["same_user"], true, "{promote}");
    assert_eq!(promote["self_signed"], false, "{promote}");

    // Positive control: Ada's agent recalls it.
    let (status, _, hits) = json(
        &rig,
        api(
            "POST",
            "/v1/memories/recall",
            &ada_key,
            Some(json!({"query": "Zanzibar vendor audit"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hits}");
    assert!(hits.to_string().contains("Zanzibar"), "{hits}");

    // HTTP: Bob recalls nothing of it, and its id is absent.
    let (status, _, hits) = json(
        &rig,
        api(
            "POST",
            "/v1/memories/recall",
            &bob_key,
            Some(json!({"query": "Zanzibar vendor audit"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hits}");
    assert!(!hits.to_string().contains("Zanzibar"), "{hits}");
    let (status, _, body) = json(
        &rig,
        api("GET", &format!("/v1/memories/{artifact}"), &bob_key, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (_, _, listing) = json(&rig, api("GET", "/v1/memories", &bob_key, None)).await;
    assert!(!listing.to_string().contains("Zanzibar"), "{listing}");
    // The tree shows Bob no node of Ada's root.
    let ada_user = rig_user(&rig, 21).await;
    let (_, _, tree) = json(
        &rig,
        api("GET", "/v2/workareas/universe/tree", &bob_key, None),
    )
    .await;
    assert!(!tree.to_string().contains(&ada_user), "{tree}");
    let (status, _, body) = json(
        &rig,
        api(
            "POST",
            "/v1/memories/recall",
            &bob_key,
            Some(json!({"query": "Zanzibar", "workarea": ada_user})),
        ),
    )
    .await;
    assert_ne!(status, StatusCode::OK, "{body}");
    assert!(!body.to_string().contains("vendor audit"), "{body}");
    // MCP: the read tool finds nothing.
    let (status, _, rpc) = json(
        &rig,
        api(
            "POST",
            "/mcp",
            &bob_key,
            Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "oml_read", "arguments": {"query": "Zanzibar vendor audit"}}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rpc}");
    assert!(!rpc.to_string().contains("vendor audit"), "{rpc}");
    // Web: Bob's pages, status, counts and export hold nothing of Ada's.
    for path in ["/web?view=memories", "/web/start", "/web?view=review"] {
        let (_, _, html) = send(&rig.router, page(path, Some(&bob))).await;
        assert!(!html.contains("Zanzibar"), "{path}: {html}");
        assert!(!html.contains(&ada_user), "{path}: {html}");
    }
    let (_, _, status_json) = json(&rig, page("/web/start/status", Some(&bob))).await;
    assert_eq!(status_json["first_memory"], Value::Null, "{status_json}");
    let (_, _, export) = json(&rig, page("/web/start/export", Some(&bob))).await;
    assert_eq!(export["memories"], json!([]), "{export}");
    // And Ada's own export holds it.
    let (status, headers, export) = json(&rig, page("/web/start/export", Some(&ada))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert!(export.to_string().contains("Zanzibar"), "{export}");
    let (_, _, ada_status) = json(&rig, page("/web/start/status", Some(&ada))).await;
    assert_eq!(
        ada_status,
        json!({"connected": true, "first_memory": "promoted"})
    );
    drop_db(&rig.db).await;
}

/// The drawn user of GitHub account `id`, read off the journal.
async fn rig_user(rig: &Rig, _id: u64) -> String {
    events(rig)
        .await
        .iter()
        .find(|(t, _)| t == "web.signed_up")
        .map(|(_, p)| p["user"].as_str().expect("a user").to_owned())
        .expect("a sign-up")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quotas_refuse_past_their_limit_with_limit_quota_and_an_admin_raises_them() {
    let Some(rig) = rig(Some(|mut p| {
        p.quotas.keys = 1;
        p.quotas.memories = 1;
        p.quotas.requests_per_minute = 4;
        p
    }))
    .await
    else {
        return;
    };
    let ada = sign_up(&rig, 31, "ada@example.com").await;
    let key = claude_code_key(&rig, &ada).await;
    // A second key is past the keys quota.
    let (status, _, html) = send(
        &rig.router,
        form(
            "/web/start",
            Some(&ada),
            &format!("action=issue_other&csrf_token={}", csrf(&ada)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{html}");
    assert!(html.contains("limit.quota"), "{html}");
    // A second memory is past the memories quota, named in the refusal.
    let (status, body) = remember(&rig, &key, "one", "The first fact.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, body) = remember(&rig, &key, "two", "The second fact.").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"]["code"], "limit.quota");
    assert_eq!(
        body["error"]["details"],
        json!({"quota": "memories", "limit": 1, "used": 1})
    );
    assert!(
        !events(&rig)
            .await
            .iter()
            .any(|(_, p)| p.to_string().contains("The second fact"))
    );
    // Past four requests in a minute the key is refused; ten requests
    // cross at most one minute boundary, so one of them is refused.
    let mut refused = None;
    for _ in 0..10 {
        let (status, headers, body) = json(&rig, api("GET", "/v1/memories", &key, None)).await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            refused = Some((headers, body));
            break;
        }
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (headers, body) = refused.expect("the rate refuses");
    assert_eq!(body["error"]["code"], "limit.rate");
    assert_eq!(body["error"]["details"]["quota"], "requests_per_minute");
    let retry: u64 = headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&retry), "{retry}");
    let metrics = rig.traffic.metrics().encode();
    for quota in ["keys", "memories", "requests_per_minute"] {
        assert!(
            metrics.contains(&format!(r#"oml_quota_refused_total{{quota="{quota}"}}"#)),
            "{quota}: {metrics}"
        );
    }

    // The operator, an admin entry, raises Ada's keys quota on the users page.
    let op = operator_session(&rig).await;
    let user = rig_user(&rig, 31).await;
    let (status, _, html) = send(&rig.router, page("/web/admin/users", Some(&op))).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains(&user), "{html}");
    assert!(html.contains("1 / 1"), "{html}");
    let (status, headers, html) = send(
        &rig.router,
        form(
            "/web/admin/users",
            Some(&op),
            &format!(
                "action=set_quota&csrf_token={}&user={user}&quota=keys&limit=2",
                csrf(&op)
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{html}");
    assert_eq!(headers[header::LOCATION], "/web/admin/users");
    let (_, set) = events(&rig)
        .await
        .into_iter()
        .find(|(t, _)| t == "web.quota_set")
        .expect("web.quota_set");
    assert_eq!(set["quota"], "keys");
    assert_eq!(set["limit"], 2);
    let (status, _, html) = send(
        &rig.router,
        form(
            "/web/start",
            Some(&ada),
            &format!("action=issue_other&csrf_token={}", csrf(&ada)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("cred-key"));
    // A person's page refuses the users page.
    let (status, _, _) = send(&rig.router, page("/web/admin/users", Some(&ada))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    drop_db(&rig.db).await;
}

/// The operator's admin entry on the sign-up tenant, added by the CLI's
/// route, and its session cookie.
async fn operator_session(rig: &Rig) -> String {
    let (status, _, body) = send(
        &rig.router,
        Request::post("/v2/web/emails")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "op")
            .body(Body::from(
                json!({"email": "op@example.com", "user": "op", "admin": true}).to_string(),
            ))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, headers, body) = callback(rig, 99, "op@example.com").await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    format!(
        "{SESSION_COOKIE}={}",
        set_cookie(&headers, SESSION_COOKIE).expect("a session")
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletion_is_a_journalled_request_the_admin_closes_by_disabling_the_person() {
    let Some(rig) = rig(Some(|p| p)).await else {
        return;
    };
    let ada = sign_up(&rig, 41, "ada@example.com").await;
    let key = claude_code_key(&rig, &ada).await;
    let (status, body) = remember(&rig, &key, "kept", "A fact that outlives the account.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Without the confirmation word, nothing is requested.
    let (status, _, _) = send(
        &rig.router,
        form(
            "/web/start",
            Some(&ada),
            &format!(
                "action=request_deletion&csrf_token={}&confirm=no",
                csrf(&ada)
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = send(
        &rig.router,
        form(
            "/web/start",
            Some(&ada),
            &format!(
                "action=request_deletion&csrf_token={}&confirm=delete",
                csrf(&ada)
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        events(&rig)
            .await
            .iter()
            .any(|(t, _)| t == "web.deletion_requested")
    );
    // The agent keeps working until the admin acts.
    let (status, _, _) = json(&rig, api("GET", "/v1/memories", &key, None)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, html) = send(&rig.router, page("/web/start", Some(&ada))).await;
    assert!(
        html.contains("deletion request is with the administrator"),
        "{html}"
    );

    let op = operator_session(&rig).await;
    let user = rig_user(&rig, 41).await;
    let (_, _, html) = send(&rig.router, page("/web/admin/users", Some(&op))).await;
    assert!(html.contains("Disable and close"), "{html}");
    let (status, _, html) = send(
        &rig.router,
        form(
            "/web/admin/users",
            Some(&op),
            &format!("action=close_deletion&csrf_token={}&user={user}", csrf(&op)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{html}");
    let types: Vec<String> = events(&rig).await.into_iter().map(|(t, _)| t).collect();
    for t in ["web.user_disabled", "key.denied", "web.deletion_closed"] {
        assert!(types.iter().any(|x| x == t), "{t}: {types:?}");
    }
    // The session ended, the agent is cut off, the memory is kept.
    let (_, _, html) = send(&rig.router, page("/web/start", Some(&ada))).await;
    assert!(html.contains("Sign in with GitHub"), "{html}");
    let (status, _, body) = json(
        &rig,
        api(
            "POST",
            "/v1/memories/recall",
            &key,
            Some(json!({"query": "outlives"})),
        ),
    )
    .await;
    assert!(
        !body.to_string().contains("outlives the account") || status != StatusCode::OK,
        "{body}"
    );
    let (_, _, op_read) = json(
        &rig,
        api(
            "POST",
            "/v2/admin/memories/search",
            &rig.boot.admin_key.value,
            Some(json!({"query": "outlives"})),
        ),
    )
    .await;
    assert!(
        op_read.to_string().contains("outlives the account"),
        "kept: {op_read}"
    );
    // A disabled person signs in to nothing and cannot sign up again.
    let (status, headers, body) = callback(&rig, 41, "ada@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(body.contains("disabled"), "{body}");
    assert!(set_cookie(&headers, SIGN_UP_COOKIE).is_none());
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sign_ups_from_one_address_are_limited_per_hour() {
    let Some(rig) = rig(Some(|mut p| {
        p.per_ip_per_hour = 1;
        p
    }))
    .await
    else {
        return;
    };
    sign_up(&rig, 51, "one@example.com").await;
    let (status, headers, _) = callback(&rig, 52, "two@example.com").await;
    assert_eq!(status, StatusCode::OK);
    let pending = set_cookie(&headers, SIGN_UP_COOKIE).expect("the sign-up cookie");
    let (status, headers, body) = send(
        &rig.router,
        form(
            "/web/sign-up",
            Some(&format!("{SIGN_UP_COOKIE}={pending}")),
            &format!("accept_terms=yes&terms_version={TERMS}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(body.contains("limit.rate"), "{body}");
    let _ = headers;
    assert_eq!(
        events(&rig)
            .await
            .iter()
            .filter(|(t, _)| t == "web.signed_up")
            .count(),
        1
    );
    // The first person signs in again: a sign-in takes no slot.
    let (status, _, _) = callback(&rig, 51, "one@example.com").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_start_pages_answer_nothing_without_a_session() {
    let Some(rig) = rig(Some(|p| p)).await else {
        return;
    };
    let (status, _, html) = send(&rig.router, page("/web/start", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Sign in with GitHub"), "{html}");
    for path in ["/web/start/status", "/web/start/export", "/web/admin/users"] {
        let (status, _, body) = send(&rig.router, page(path, None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
    }
    // A bearer key reaches none of them (ADR-0045 d7).
    let (status, _, _) = send(
        &rig.router,
        Request::get("/web/start/status")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The start page carries its one hashed script, and no other page does.
    let ada = sign_up(&rig, 61, "ada@example.com").await;
    let (_, headers, html) = send(&rig.router, page("/web/start", Some(&ada))).await;
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(
        csp.contains("script-src 'sha256-") && csp.contains("connect-src 'self'"),
        "{csp}"
    );
    assert!(html.contains("<script>"));
    let (_, headers, _) = send(&rig.router, page("/web", Some(&ada))).await;
    assert!(
        !headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("script-src")
    );
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signup_issuer_that_is_not_an_admin_key_stops_the_start() {
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key, "oml").await;
    let mut p = policy(boot.tenant_id);
    p.issuer = Name::parse("nobody").unwrap();
    let started = start_lead_with(
        &store,
        key,
        Some(Issuer::parse(ISSUER).unwrap()),
        PinnedFetcher,
        Traffic::default(),
        Some(
            WebPages::new(vec![(Provider::GitHub, "c".to_owned())], MemSignIn::new())
                .with_signup(p),
        ),
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
    )
    .await;
    let error = started.err().expect("the start stops");
    assert!(error.contains("signup.issuer"), "{error}");
    drop_db(&db).await;
}

#[test]
fn open_mode_without_its_settings_stops_the_start() {
    let base = |extra: &[(&str, &str)]| {
        let mut env: std::collections::HashMap<String, String> = [
            ("OML_POSTGRES__URL", "postgres://a@h/d"),
            ("OML_POSTGRES__OWNER_URL", "postgres://b@h/d"),
            ("OML_OAUTH__ISSUER", "https://oml.example.com"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
        for (k, v) in extra {
            env.insert((*k).to_owned(), (*v).to_owned());
        }
        oml_server_lead::Config::from_sources(None, &|k| env.get(k).cloned())
    };
    assert_eq!(base(&[]).expect("allowlist by default").signup, None);
    let error = base(&[("OML_SIGNUP__MODE", "open")])
        .unwrap_err()
        .to_string();
    assert!(error.contains("signup.tenant"), "{error}");
    let full = [
        ("OML_SIGNUP__MODE", "open"),
        ("OML_SIGNUP__TENANT", "7b6f9a8e-0000-4000-8000-000000000001"),
        ("OML_SIGNUP__ISSUER", "root-admin"),
        ("OML_SIGNUP__TERMS_VERSION", "v1"),
        ("OML_SIGNUP__TERMS_URL", "https://example.com/t"),
        ("OML_SIGNUP__PRIVACY_URL", "https://example.com/p"),
        ("OML_QUOTA__MEMORIES", "50"),
    ];
    let open = base(&full).expect("open").signup.expect("a policy");
    assert_eq!(open.quotas.memories, 50);
    assert_eq!(open.quotas.keys, 5);
    assert_eq!(open.per_ip_per_hour, 5);
    let mut bad = full.to_vec();
    bad.push(("OML_SIGNUP__MODE", "wide"));
    assert!(base(&bad).is_err());
}
