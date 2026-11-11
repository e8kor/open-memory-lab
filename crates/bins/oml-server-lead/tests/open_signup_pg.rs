//! Open sign-up over the real lead and a migrated Postgres store under its
//! role pair (ADR-0068): the terms page, the private root, isolation from a
//! second person on every surface that exists, review by the person's own
//! reviewer key marked `same_user`, the quotas and the request rate, the
//! deletion request and the disable switch, and the per-address limit. The
//! sign-in provider is the in-memory double. Each test is named after the
//! sentence it proves.

use std::sync::atomic::Ordering;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::oauth::Issuer;
use oml_app::domain::signup::{Quotas, SignupMode};
use oml_app::domain::web::{GitHubEmail, Provider};
use oml_app::domain::web::{capacity_credential, csrf_token};
use oml_app::doubles::MemSignIn;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{IdGen as _, JournalStore as _};
use oml_app::types::{BootstrapRequest, BootstrapResult, ProviderAnswer, SignupPolicy};
use oml_fetch::PinnedFetcher;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_app, start_lead_with, start_lead_with_models};
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use oml_transport_http::session::WebClient;
use oml_transport_http::traffic::Traffic;
use oml_transport_http::web::start::SIGN_UP_COOKIE;
use oml_transport_http::web::{SESSION_COOKIE, SIGN_IN_COOKIE, WebPages};

const ISSUER: &str = "https://oml.example.com";
/// `oml-web`'s client identity, which the lead accepts a capacity
/// credential from (ADR-0070 d2).
const WEB_CLIENT_SECRET: &str = "signup-web-client-secret-0123456789";

fn fresh_name() -> String {
    format!(
        "oml_test_signup_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
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
    let router = start_lead_with_models(
        &store,
        key.clone(),
        Some(Issuer::parse(ISSUER).expect("a public origin")),
        PinnedFetcher,
        traffic.clone(),
        Some(pages),
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
        None,
        WebClient::new(Some(WEB_CLIENT_SECRET.to_owned()), None),
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

/// A call as `oml-web` makes it for the session `cookie` names: the
/// capacity's credential and the web client's identity (ADR-0070 d2).
fn as_web(
    cookie: &str,
    capacity: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Request<Body> {
    let credential = capacity_credential(cookie.split_once('=').expect("a cookie").1, capacity);
    let mut request = api(method, path, &credential, body);
    request.headers_mut().insert(
        "oml-web-client",
        WEB_CLIENT_SECRET.parse().expect("a header value"),
    );
    request
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

/// A remember under the caller's own idempotency key, so the same call can
/// be sent again.
async fn remember_keyed(
    rig: &Rig,
    key: &str,
    idem: &str,
    claim_key: &str,
    text: &str,
) -> (StatusCode, HeaderMap, Value) {
    let request = Request::post("/v1/memories")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header("idempotency-key", idem)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"verb": "remember", "claim_key": claim_key, "kind": "memory", "body": text})
                .to_string(),
        ))
        .expect("a request");
    json(rig, request).await
}

/// The sign-up POST of a pending cookie: its status, headers and body.
async fn post_sign_up(rig: &Rig, pending: &str) -> (StatusCode, HeaderMap, String) {
    send(
        &rig.router,
        form(
            "/web/sign-up",
            Some(&format!("{SIGN_UP_COOKIE}={pending}")),
            &format!("accept_terms=yes&terms_version={TERMS}"),
        ),
    )
    .await
}

/// The person approves the item their reviewer key is served, as the
/// Review page, which `oml-web` serves, does through the API (ADR-0070 d7
/// (b)).
async fn approve_next(rig: &Rig, cookie: &str) {
    let (status, _, served) = json(
        rig,
        as_web(cookie, "review", "POST", "/v2/review/next", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let serve_id = served["serve_id"].as_str().expect("an item is served");
    let (status, _, recorded) = json(
        rig,
        as_web(
            cookie,
            "review",
            "POST",
            "/v2/review/verdicts",
            Some(json!({"serve_id": serve_id, "verdict": "approve", "reason": "mine"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recorded}");
    assert_eq!(recorded["state"], "promoted", "{recorded}");
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
    assert!(html.contains("0 / 1000"), "the limits: {html}");
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

    // Ada's agent then writes two more memories that nobody approves: one
    // held in her root, and one held in a workarea of her own that she
    // puts in progress, where she can recall what is unreviewed (ADR-0030).
    let held_text = "The Kilimanjaro budget is held for review and no other person may see it.";
    let (status, held) = remember(&rig, &ada_key, "kilimanjaro-budget", held_text).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{held}");
    let ada_user = rig_user(&rig, 21).await;
    let scratch = format!("{ada_user}.scratch");
    // Creating the workarea and its mode are the Workareas view's acts,
    // which oml-web runs as the person's web and reviewer keys through the
    // API (ADR-0070 d7 (b)).
    let web = capacity_credential(ada.split_once('=').expect("a cookie").1, "web");
    let mut request = api(
        "POST",
        "/v2/workareas",
        &web,
        Some(json!({"name": "scratch"})),
    );
    request.headers_mut().insert(
        "oml-web-client",
        WEB_CLIENT_SECRET.parse().expect("a header value"),
    );
    let (status, _, html) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::CREATED, "{html}");
    let review = capacity_credential(ada.split_once('=').expect("a cookie").1, "review");
    let mut request = api(
        "POST",
        &format!("/v2/workareas/{scratch}/mode"),
        &review,
        Some(json!({"mode": "in_progress"})),
    );
    request.headers_mut().insert(
        "oml-web-client",
        WEB_CLIENT_SECRET.parse().expect("a header value"),
    );
    let (status, _, html) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let (status, in_progress) =
        {
            let (status, _, body) = json(
            &rig,
            api(
                "POST",
                "/v1/memories",
                &ada_key,
                Some(json!({"verb": "remember", "claim_key": "tanganyika-draft", "kind": "memory",
                    "workarea": scratch,
                    "body": "The Tanganyika draft stays in progress until Ada commits it."})),
            ),
        )
        .await;
            (status, body)
        };
    assert_eq!(status, StatusCode::ACCEPTED, "{in_progress}");
    // Positive controls: Ada's reviewer key is served a held memory, and
    // her agent recalls the in-progress one inside its workarea.
    let (status, _, served) = json(
        &rig,
        as_web(&ada, "review", "POST", "/v2/review/next", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    assert!(served["serve_id"].is_string(), "{served}");
    let (status, _, hits) = json(
        &rig,
        api(
            "POST",
            "/v1/memories/recall",
            &ada_key,
            Some(json!({"query": "Tanganyika draft", "workarea": scratch})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hits}");
    assert!(hits.to_string().contains("Tanganyika"), "{hits}");

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
    // The Memories list is oml-web's (ADR-0070 d7 (b)): the lead answers
    // its old URL with a redirect that holds no page, and oml-web's own
    // isolation case is `a_person_reaches_nothing_of_another_persons_root_on_oml_webs_pages`.
    let (status, headers, _) = send(&rig.router, page("/web?view=memories", Some(&bob))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/memories");
    for path in ["/web/start"] {
        let (_, _, html) = send(&rig.router, page(path, Some(&bob))).await;
        assert!(!html.contains("Zanzibar"), "{path}: {html}");
        assert!(!html.contains(&ada_user), "{path}: {html}");
    }
    // The held rows of Ada's root and of her in-progress workarea are in no
    // queue, listing, event tail or recall of Bob's (ADR-0068 d3).
    let (status, _, served) = json(
        &rig,
        as_web(&bob, "review", "POST", "/v2/review/next", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "nothing to serve: {served}");
    let (_, _, bob_held) = json(&rig, as_web(&bob, "review", "GET", "/v2/review/held", None)).await;
    for secret in ["Kilimanjaro", "Tanganyika"] {
        assert!(!bob_held.to_string().contains(secret), "{bob_held}");
        // Neither old URL is a page on the lead any more: curate was never
        // a user view, and the list is oml-web's.
        for (path, want) in [
            ("/web?view=curate", StatusCode::NOT_FOUND),
            ("/web?view=memories", StatusCode::SEE_OTHER),
        ] {
            let (status, _, html) = send(&rig.router, page(path, Some(&bob))).await;
            assert_eq!(status, want, "{path}");
            assert!(!html.contains(secret), "{path}: {html}");
        }
        for path in ["/v2/review/held", "/v2/events", "/v1/memories"] {
            let (_, _, body) = json(&rig, api("GET", path, &bob_key, None)).await;
            assert!(!body.to_string().contains(secret), "{path}: {body}");
        }
        for query in [secret, "budget held for review", "draft stays in progress"] {
            let (_, _, hits) = json(
                &rig,
                api(
                    "POST",
                    "/v1/memories/recall",
                    &bob_key,
                    Some(json!({"query": query})),
                ),
            )
            .await;
            assert!(!hits.to_string().contains(secret), "{query}: {hits}");
        }
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
    // Her newer, still held writes are what the page shows now.
    assert_eq!(
        ada_status,
        json!({"connected": true, "first_memory": "held"})
    );
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sign_up_post_for_an_address_an_entry_holds_never_opens_a_session() {
    let Some(rig) = rig(Some(|p| p)).await else {
        return;
    };
    // An attacker's provider account 99 has the victim's address verified,
    // and while the address is unlisted it reaches the terms page.
    let (status, headers, body) = callback(&rig, 99, "victim@example.com").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let stale = set_cookie(&headers, SIGN_UP_COOKIE).expect("the sign-up cookie");
    // The victim signs up first, with account 41, and the address is pinned to it.
    sign_up(&rig, 41, "victim@example.com").await;
    let sessions_before = events(&rig).await.len();
    // The attacker's post is refused: no session, no cookie, no new event.
    let (status, headers, body) = post_sign_up(&rig, &stale).await;
    assert_ne!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(body.contains("auth."), "{body}");
    assert_eq!(
        set_cookie(&headers, SIGN_UP_COOKIE).as_deref(),
        Some(""),
        "the spent cookie is cleared: {headers:?}"
    );
    assert!(
        set_cookie(&headers, SESSION_COOKIE).is_none(),
        "{headers:?}"
    );
    assert_eq!(events(&rig).await.len(), sessions_before);
    // Through the normal path the other subject is refused by the pin.
    let (status, headers, body) = callback(&rig, 99, "victim@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(set_cookie(&headers, SESSION_COOKIE).is_none());
    // An admin allowlisting the address after a cookie was issued gets no
    // session for the cookie's holder either, whatever the entry's flag.
    let (status, headers, body) = callback(&rig, 77, "late@example.com").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let late = set_cookie(&headers, SIGN_UP_COOKIE).expect("the sign-up cookie");
    let (status, _, body) = send(
        &rig.router,
        Request::post("/v2/web/emails")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "late")
            .body(Body::from(
                json!({"email": "late@example.com", "user": "late", "admin": true}).to_string(),
            ))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, headers, body) = post_sign_up(&rig, &late).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(
        set_cookie(&headers, SESSION_COOKIE).is_none(),
        "{headers:?}"
    );
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn at_the_memories_quota_a_replay_returns_its_receipt_and_a_new_version_is_admitted() {
    let Some(rig) = rig(Some(|mut p| {
        p.quotas.memories = 1;
        p
    }))
    .await
    else {
        return;
    };
    let ada = sign_up(&rig, 61, "ada@example.com").await;
    let key = claude_code_key(&rig, &ada).await;
    let (status, _, first) = remember_keyed(&rig, &key, "retry-1", "one", "The first fact.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    // The person is at 1 of 1. The same call again is the stored receipt.
    let (status, headers, again) =
        remember_keyed(&rig, &key, "retry-1", "one", "The first fact.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(headers["idempotency-replay"], "true");
    assert_eq!(again["artifact_id"], first["artifact_id"], "{again}");
    // A different write is past the quota.
    let (status, _, body) = remember_keyed(&rig, &key, "new-1", "two", "The second fact.").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"]["code"], "limit.quota");
    // A write under the live claim key is the conflict it always was.
    let (status, _, body) = remember_keyed(&rig, &key, "new-2", "one", "Another text.").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    // A new version of the memory adds no memory: after the person approves
    // the first, the agent supersedes it at the quota.
    approve_next(&rig, &ada).await;
    let artifact = first["artifact_id"].as_str().expect("an id");
    let (status, _, body) = json(
        &rig,
        Request::post(format!("/v1/memories/{artifact}/supersede"))
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .header("idempotency-key", "sup-1")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"expected_version": 1, "body": "The first fact, revised."}).to_string(),
            ))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
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
async fn an_admin_disables_only_a_person_who_signed_up_and_never_an_admin_or_a_stranger() {
    let Some(rig) = rig(Some(|p| p)).await else {
        return;
    };
    let ada = sign_up(&rig, 81, "ada@example.com").await;
    let op = operator_session(&rig).await;
    let disable = |user: &str| {
        form(
            "/web/admin/users",
            Some(&op),
            &format!("action=disable_user&csrf_token={}&user={user}", csrf(&op)),
        )
    };
    // A name no entry holds, and the admin's own entry, are refused and journal nothing.
    for user in ["unknownperson", "op"] {
        let (status, _, html) = send(&rig.router, disable(user)).await;
        assert_ne!(status, StatusCode::SEE_OTHER, "{user}: {html}");
    }
    assert!(
        !events(&rig)
            .await
            .iter()
            .any(|(t, _)| t == "web.user_disabled"),
        "nothing was disabled"
    );
    // The person who signed up is disabled, and their session ends.
    let ada_user = rig_user(&rig, 81).await;
    let (status, _, html) = send(&rig.router, disable(&ada_user)).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{html}");
    let (_, _, html) = send(&rig.router, page("/web/start", Some(&ada))).await;
    assert!(html.contains("Sign in with GitHub"), "{html}");
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
    let (_, headers, _) = send(&rig.router, page("/web?view=setup", Some(&ada))).await;
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
    // Open sign-up with no sign-in provider would serve no pages and no
    // quotas: the start stops, naming the web settings (ADR-0068 d1).
    let error = base(&full).unwrap_err().to_string();
    assert!(
        error.contains("web.google") && error.contains("web.github"),
        "{error}"
    );
    let mut with_provider = full.to_vec();
    with_provider.extend([
        ("OML_WEB__GITHUB__CLIENT_ID", "github-client"),
        ("OML_WEB__GITHUB__CLIENT_SECRET", "github-secret"),
    ]);
    let full = with_provider;
    let open = base(&full).expect("open").signup.expect("a policy");
    assert_eq!(open.quotas.memories, 50);
    assert_eq!(open.quotas.keys, 5);
    assert_eq!(open.per_ip_per_hour, 5);
    let mut bad = full.to_vec();
    bad.push(("OML_SIGNUP__MODE", "wide"));
    assert!(base(&bad).is_err());
}
