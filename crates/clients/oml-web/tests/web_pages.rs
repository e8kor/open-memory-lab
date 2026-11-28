//! The web pages' assertions, moved from
//! `crates/transports/oml-transport-http/tests/web_pages.rs` with each page
//! that moves to `oml-web`, unchanged (ADR-0070 d7, parity). Only the rig
//! differs: it starts the lead on a loopback listener over the in-memory
//! doubles, starts `oml-web` on another with `web.api_url` set to the lead,
//! and puts a path router in front of both that routes as the production
//! proxy does ([`oml_web::serves`]). Every request a test sends goes through
//! that router, so a moved page is answered by `oml-web` over the hop and
//! everything else, the sign-in and the acts, by the lead.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Request as AxumRequest;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::Response;
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::App;
use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::{Email, Provider};
use oml_app::domain::{Role, UsedBy};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSignIn, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::{
    BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, ProviderAnswer,
    WebEmailRequest,
};
use oml_transport_http::session::WebClient;
use oml_transport_http::traffic::ScopeObserver;
use oml_transport_http::web::{SESSION_COOKIE, SIGN_IN_COOKIE, WebPages};
use oml_transport_http::{ApiState, DbPing};
use oml_web::{Layers, Web};

const ISSUER: &str = "https://oml.example.com";
const CLIENT_GOOGLE: &str = "google-client.apps.googleusercontent.com";
const CLIENT_GITHUB: &str = "github-client";
const FORM: &str = "application/x-www-form-urlencoded";
const EMAIL: &str = "alice@example.com";
/// The web client secret both processes are configured with (ADR-0070 d2).
const WEB_CLIENT_SECRET: &str = "parity-web-client-secret-0123456789";

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

struct Rig {
    /// The lead's clock, which a test moves to reach a key's expiry.
    lead_clock: FixedClock,
    store: MemStore,
    app: Arc<TestApp>,
    /// The path router in front of the lead and `oml-web`.
    router: Router,
    /// Every request to `oml-web`, whatever its method, as the Caddyfile's
    /// and compose's path matchers route a moved page's path.
    direct: Router,
    boot: BootstrapResult,
    providers: MemSignIn,
    /// Every line `oml-web` wrote.
    lines: Arc<std::sync::Mutex<Vec<String>>>,
}

fn test_app(store: &MemStore) -> TestApp {
    test_app_on(store, FixedClock::new())
}

fn test_app_on(store: &MemStore, clock: FixedClock) -> TestApp {
    App::new(
        store.clone(),
        MemSigner::new([7; 32]),
        clock,
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_observer(Arc::new(ScopeObserver))
    .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"))
    .with_formats(oml_format::compiled())
}

/// `router` served on a loopback listener on an ephemeral port.
async fn serve(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("an address");
    tokio::spawn(async move { axum::serve(listener, router).await });
    address
}

/// The proxy in front of both processes: each request goes, over HTTP, to
/// `oml-web` when [`oml_web::serves`] names it and to the lead otherwise,
/// and its answer comes back as sent, every `Set-Cookie` included.
fn front(lead: SocketAddr, web: SocketAddr) -> Router {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("a client");
    Router::new().fallback(move |request: AxumRequest| {
        let client = client.clone();
        async move {
            let target = if oml_web::serves(request.method().as_str(), request.uri().path()) {
                web
            } else {
                lead
            };
            let (parts, body) = request.into_parts();
            let body = to_bytes(body, usize::MAX).await.expect("a body");
            let path = parts.uri.path_and_query().map_or("/", |p| p.as_str());
            let mut outbound = client
                .request(parts.method, format!("http://{target}{path}"))
                .body(body.to_vec());
            for (name, value) in &parts.headers {
                outbound = outbound.header(name, value);
            }
            let answer = outbound.send().await.expect("the upstream answers");
            let mut response = Response::builder().status(answer.status());
            for (name, value) in answer.headers() {
                response = response.header(name, value);
            }
            response
                .body(Body::from(answer.bytes().await.expect("a body").to_vec()))
                .expect("a response")
        }
    })
}

/// A tenant whose admin key put `alice@example.com` on the list as `alice`;
/// the lead with both providers on and the web client's secret set, and
/// `oml-web` reaching it over loopback.
async fn rig() -> Rig {
    // The console suite adds several admin entries to reach its persons;
    // the admin act's confirm-first rule has its own suite (ADR-0097 d10).
    rig_on(MemStore::new().keeping_sole_admin(), false).await
}

/// [`rig`] over `store`; with `one_clock` the tests' own app shares the
/// lead's clock, so an act a test takes after moving it is as old as the
/// lead reads it (ADR-0097 d11).
async fn rig_on(store: MemStore, one_clock: bool) -> Rig {
    let providers = MemSignIn::new();
    let boot = test_app(&store)
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("oml").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "operator".to_owned(),
        })
        .await
        .expect("bootstrap");
    let ping: DbPing = Arc::new(|| Box::pin(async { true }));
    let lead_clock = FixedClock::new();
    let lead = ApiState::new(
        Arc::new(test_app_on(&store, lead_clock.clone())),
        store.clone(),
        ping,
        true,
        None,
    )
    .with_web(WebPages::new(
        vec![
            (Provider::Google, CLIENT_GOOGLE.to_owned()),
            (Provider::GitHub, CLIENT_GITHUB.to_owned()),
        ],
        providers.clone(),
    ))
    .with_web_client(WebClient::new(Some(WEB_CLIENT_SECRET.to_owned()), None))
    .into_router();
    let lead = serve(lead).await;
    let transport = oml_sdk::HttpTransport::new(&format!("http://{lead}"), Duration::from_secs(10))
        .expect("a transport");
    let lines = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = lines.clone();
    let web = oml_web::router(Web {
        layers: Layers {
            public_origin: Some(ISSUER.to_owned()),
            log: Arc::new(move |line| sink.lock().expect("unpoisoned").push(line)),
        },
        api: oml_web::api(Arc::new(transport), WEB_CLIENT_SECRET).expect("an API client"),
    });
    let web = serve(web).await;
    let app = if one_clock {
        test_app_on(&store, lead_clock.clone())
    } else {
        test_app(&store)
    };
    let rig = Rig {
        lead_clock,
        app: Arc::new(app),
        store,
        router: front(lead, web),
        direct: front(web, web),
        boot,
        providers,
        lines,
    };
    rig.allow(EMAIL, "alice", false).await;
    rig
}

impl Rig {
    async fn allow(&self, email: &str, user: &str, admin: bool) {
        self.app
            .allow_web_email(
                &self.boot.admin_key.value,
                &WebEmailRequest {
                    email: Email::parse(email).expect("an email"),
                    user: Name::parse(user).expect("a name"),
                    admin,
                },
            )
            .await
            .expect("the admin key adds the entry");
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

fn pct(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn form(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(n, v)| format!("{}={}", pct(n), pct(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn post(path: &str, fields: &[(&str, &str)], cookie: Option<&str>) -> Request<Body> {
    let mut builder = Request::post(path)
        .header(header::CONTENT_TYPE, FORM)
        .header("sec-fetch-site", "same-origin");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder.body(Body::from(form(fields))).expect("a request")
}

fn get(path: &str, cookie: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder.body(Body::empty()).expect("a request")
}

/// The value a `Set-Cookie` header gives `name`, and the whole header.
fn set_cookie(headers: &HeaderMap, name: &str) -> Option<(String, String)> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| {
            let value = v[name.len() + 1..]
                .split(';')
                .next()
                .unwrap_or("")
                .to_owned();
            (value, v.to_owned())
        })
}

fn query_param(location: &str, name: &str) -> Option<String> {
    let query = location.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (n, v) = pair.split_once('=')?;
        (n == name).then(|| {
            let bytes = v.as_bytes();
            let mut out = Vec::new();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%' {
                    out.push(u8::from_str_radix(&v[i + 1..i + 3], 16).expect("hex"));
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            String::from_utf8(out).expect("UTF-8")
        })
    })
}

fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

fn google_token(nonce: &str, subject: &str, email: &str, verified: bool) -> String {
    let claims = json!({
        "iss": "https://accounts.google.com",
        "aud": CLIENT_GOOGLE,
        "exp": 4_102_444_800_i64,
        "nonce": nonce,
        "sub": subject,
        "email": email,
        "email_verified": verified,
    });
    format!(
        "{}.{}.{}",
        b64url(br#"{"alg":"RS256"}"#),
        b64url(claims.to_string().as_bytes()),
        b64url(b"sig")
    )
}

/// A started sign-in: the provider's `Location` and the sealed cookie.
async fn start(rig: &Rig, provider: Provider) -> (String, String) {
    let (status, headers, _) = send(
        &rig.router,
        post("/web/sign-in", &[("provider", provider.as_str())], None),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned();
    let (sealed, whole) = set_cookie(&headers, SIGN_IN_COOKIE).expect("the sign-in cookie");
    for attribute in [
        "Path=/",
        "Secure",
        "HttpOnly",
        "SameSite=Lax",
        "Max-Age=600",
    ] {
        assert!(whole.contains(attribute), "{whole}");
    }
    (location, sealed)
}

/// The provider's redirect back, with the sealed cookie and the state.
async fn callback(
    rig: &Rig,
    provider: Provider,
    code: &str,
    state: &str,
    cookies: &str,
) -> (StatusCode, HeaderMap, String) {
    send(
        &rig.router,
        get(
            &format!(
                "/web/callback/{}?code={}&state={}",
                provider.as_str(),
                pct(code),
                pct(state)
            ),
            Some(cookies),
        ),
    )
    .await
}

/// A whole Google sign-in as `subject` for `email`; the session token.
async fn sign_in_google(
    rig: &Rig,
    subject: &str,
    email: &str,
) -> (StatusCode, Option<String>, String) {
    let (location, sealed) = start(rig, Provider::Google).await;
    let state = query_param(&location, "state").expect("a state");
    let nonce = query_param(&location, "nonce").expect("a nonce");
    rig.providers.answer(
        Provider::Google,
        Ok(ProviderAnswer::GoogleIdToken(google_token(
            &nonce, subject, email, true,
        ))),
    );
    let (status, headers, body) = callback(
        rig,
        Provider::Google,
        "the-code-4711",
        &state,
        &format!("{SIGN_IN_COOKIE}={sealed}"),
    )
    .await;
    (
        status,
        set_cookie(&headers, SESSION_COOKIE).map(|(v, _)| v),
        body,
    )
}

fn csrf_of(page: &str) -> String {
    let marker = r#"name="csrf_token" value=""#;
    let start = page.find(marker).expect("a csrf field") + marker.len();
    page[start..start + page[start..].find('"').expect("a quote")].to_owned()
}

/// A signed-in browser that opened no investigation: its cookie header.
async fn signed_in_without_investigation(rig: &Rig, subject: &str, email: &str) -> String {
    let (status, token, body) = sign_in_google(rig, subject, email).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    format!("{SESSION_COOKIE}={}", token.expect("a session"))
}

/// A signed-in browser: its cookie header. An admin's browser opens an
/// investigation through the console's own form first, as the console asks
/// an admin who reads claim text to (ADR-0089 d10).
async fn signed_in(rig: &Rig, subject: &str, email: &str) -> String {
    let (status, token, body) = sign_in_google(rig, subject, email).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    investigating(rig, &cookie).await
}

/// An open investigation of the tenant's boot admin key, for the tests'
/// own admin reads through the application (ADR-0089 d10).
async fn boot_investigation(rig: &Rig) -> uuid::Uuid {
    // The admin read needs a person-linked admin key (ADR-0097 d16): the
    // boot key links itself to the tenant's admin person, who accepts.
    let admin = rig.boot.admin_key.value.as_str();
    let ops = match rig
        .app
        .list_web_emails(admin, None, 500)
        .await
        .expect("entries")
        .into_iter()
        .find(|e| e.admin && !e.pending)
    {
        Some(entry) => entry,
        None => {
            rig.app
                .allow_web_email(
                    admin,
                    &oml_app::types::WebEmailRequest {
                        email: oml_app::domain::web::Email::parse("ops-boot@example.com")
                            .expect("an email"),
                        user: oml_app::domain::name::Name::parse("ops-boot").expect("a name"),
                        admin: true,
                    },
                )
                .await
                .expect("an admin entry")
                .0
        }
    };
    rig.app.web_ensure_keys(&ops).await.expect("session keys");
    if let Ok(key) = rig
        .app
        .link_self(admin, &ops.user, Some("the person holds this key"))
        .await
    {
        let token = rig
            .app
            .web_session_open(&ops, 3600, None)
            .await
            .expect("a session")
            .token;
        let session = rig
            .app
            .web_session(&token)
            .await
            .expect("a read")
            .expect("a live session");
        let web = rig
            .app
            .web_caller(&session, oml_app::app::Capacity::Web)
            .await
            .expect("the console");
        rig.app
            .accept_key_link(oml_app::types::Bearer::Session(&web), key.key_id.into())
            .await
            .expect("the link is accepted");
    }
    rig.app
        .open_investigation(rig.boot.admin_key.value.as_str(), "the console tests")
        .await
        .expect("an investigation")
        .investigation_id
}

/// The cookie the console's investigation form sets.
const INVESTIGATION_COOKIE: &str = "__Host-oml_investigation";

/// `cookie` with an open investigation of the session's admin key, where
/// the session is an admin's; else `cookie` as it is.
async fn investigating(rig: &Rig, cookie: &str) -> String {
    let (status, _, page) = send(&rig.router, get("/web/admin/memories", Some(cookie))).await;
    if status != StatusCode::FORBIDDEN || !page.contains("Open an investigation") {
        return cookie.to_owned();
    }
    let csrf = csrf_of(&page);
    let (status, headers, body) = send(
        &rig.router,
        post(
            "/web/admin/investigation",
            &[
                ("csrf_token", csrf.as_str()),
                ("action", "open"),
                ("reason", "the console tests"),
                ("next", "/web/admin/memories"),
            ],
            Some(cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (id, _) = set_cookie(&headers, INVESTIGATION_COOKIE).expect("the investigation cookie");
    format!("{cookie}; {INVESTIGATION_COOKIE}={id}")
}

/// A page act with the CSRF token read off the page first.
async fn act(
    rig: &Rig,
    cookie: &str,
    path: &str,
    fields: &[(&str, &str)],
) -> (StatusCode, HeaderMap, String) {
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(cookie))).await;
    let csrf = csrf_of(&page);
    let mut all: Vec<(&str, &str)> = vec![("csrf_token", &csrf)];
    all.extend_from_slice(fields);
    send(&rig.router, post(path, &all, Some(cookie))).await
}

/// The shown-once cookie an act sets for the page it redirects to.
const ONCE_COOKIE: &str = "__Host-oml_once";

/// An act, then the page its `303` names, as a browser follows it, with
/// the cookie the act set for that page (Post/Redirect/Get). An answer
/// that is not a `303` is returned as it is.
async fn act_and_follow(
    rig: &Rig,
    cookie: &str,
    path: &str,
    fields: &[(&str, &str)],
) -> (StatusCode, HeaderMap, String) {
    let (status, headers, body) = act(rig, cookie, path, fields).await;
    if status != StatusCode::SEE_OTHER {
        return (status, headers, body);
    }
    let location = headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned();
    let cookies = match set_cookie(&headers, ONCE_COOKIE) {
        Some((value, _)) => format!("{cookie}; {ONCE_COOKIE}={value}"),
        None => cookie.to_owned(),
    };
    send(&rig.router, get(&location, Some(&cookies))).await
}

async fn key_named(rig: &Rig, name: &str) -> oml_app::types::KeyRecord {
    let id = rig
        .store
        .resolve_name(
            rig.boot.tenant_id,
            oml_app::domain::name::NamedKind::Key,
            &Name::parse(name).expect("a name"),
        )
        .await
        .expect("a lookup")
        .unwrap_or_else(|| panic!("no key {name}"));
    rig.store
        .key(rig.boot.tenant_id, id)
        .await
        .expect("a read")
        .expect("the key")
}

fn value_of(page: &str) -> String {
    let at = page.find("OML_API_KEY=").expect("a key value") + "OML_API_KEY=".len();
    page[at..at + page[at..].find(['<', '\n']).expect("an end")].to_owned()
}

async fn agent_guardrail(rig: &Rig, value: &str, body: &str) -> uuid::Uuid {
    rig.app
        .submit_memory(
            value,
            &oml_app::types::SubmitRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                idempotency_key: format!("g-{body}"),
                claim_key: None,
                verb: None,
                kind: oml_app::domain::MemoryKind::Guardrail,
                body: body.to_owned(),
                workspace: None,
                derives_from: Vec::new(),
                severity: Some(oml_app::domain::guardrail::Severity::Must),
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("the agent proposes a guardrail")
        .receipt
        .memory_id
}

async fn laptop_key(rig: &Rig, cookie: &str) -> String {
    let (_, _, page) = act_and_follow(
        rig,
        cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    value_of(&page)
}

/// Next on To do: the memory id of the Judge page its `303` opens, or
/// `None` where it lands back on To do with "Nothing waits" (ADR-0083 d11,
/// d14).
async fn next_item(rig: &Rig, cookie: &str) -> Option<String> {
    let (status, headers, body) = act(rig, cookie, "/web/todo", &[("action", "next")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let location = headers[header::LOCATION].to_str().expect("ASCII");
    query_param(location, "memory")
}

/// Judge's verdict on `memory`, as a browser posts the Judge page's form
/// and follows its `303` (ADR-0083 d2); `reason` is required to reject.
async fn judge(
    rig: &Rig,
    cookie: &str,
    memory: &str,
    verdict: &str,
    reason: Option<&str>,
) -> (StatusCode, HeaderMap, String) {
    let mut fields = vec![
        ("action", "judge"),
        ("artifact_id", memory),
        ("verdict", verdict),
    ];
    if let Some(reason) = reason {
        fields.push(("reason", reason));
    }
    act_and_follow(rig, cookie, "/web/todo/judge", &fields).await
}

/// Next, then the verdict on the item it opens: the item's id and the page
/// the verdict lands on. Next must open an item.
async fn judge_next(
    rig: &Rig,
    cookie: &str,
    verdict: &str,
    reason: Option<&str>,
) -> (String, (StatusCode, HeaderMap, String)) {
    let memory = next_item(rig, cookie)
        .await
        .expect("Next opens a held item");
    let answer = judge(rig, cookie, &memory, verdict, reason).await;
    (memory, answer)
}

/// The Keys page's issue form offers a contributor, first and so the
/// default, and a reader, never a submitter, which stays an act of the CLI
/// and the admin; a posted `writer` or `submitter` issues nothing, and the
/// key the form issues is a contributor with a submitter and a reader grant
/// (ADR-0093 d6).
#[tokio::test]
async fn the_keys_page_issues_a_contributor_by_default_or_a_reader_and_nothing_else() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    // The Keys page's one way to a new key is the builder, whose role
    // select offers contributor (the default), reader and submitter, and no
    // writer (ADR-0096 d3).
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(
        page.contains(r#"href="/web/keys/new">New key</a>"#),
        "{page}"
    );
    assert!(!page.contains(r#"value="issue_key""#), "{page}");
    let (_, _, form) = send(&rig.router, get("/web/keys/new?shape=root", Some(&cookie))).await;
    for role in ["contributor", "reader", "submitter"] {
        assert!(
            form.contains(&format!(r#"<option value="{role}""#)),
            "{role}: {form}"
        );
    }
    assert!(
        form.contains(r#"<option value="contributor" selected>"#),
        "{form}"
    );
    assert!(!form.contains(r#"<option value="writer""#), "{form}");
    for refused in ["writer", "submitter"] {
        let (_, _, answer) = act_and_follow(
            &rig,
            &cookie,
            "/web/keys",
            &[
                ("action", "issue_key"),
                ("name", refused),
                ("role", refused),
            ],
        )
        .await;
        assert!(
            answer.contains("role is contributor or reader"),
            "{refused}: {answer}"
        );
    }
    laptop_key(&rig, &cookie).await;
    let key = key_named(&rig, "alice-laptop").await;
    assert_eq!(key.role, oml_app::domain::Role::Contributor);
}

// -- the Guardrails view (ADR-0037 d17), moved to oml-web ------------------

#[tokio::test]
async fn todo_marks_a_held_instruction_as_needing_a_person_and_the_judge_page_draws_the_check() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_guardrail(&rig, &value, "Run the tests before every commit.").await;
    let (_, _, page) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    // The row names the fact; there is no lane of its own (ADR-0083 d14).
    assert!(page.contains("<span>needs a person</span>"), "{page}");
    assert!(!page.contains("<h2>Needs a person</h2>"), "{page}");
    // The Judge page draws the check from the row's kind.
    let (_, _, page) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={id}"), Some(&cookie)),
    )
    .await;
    assert!(
        page.contains(
            "<span class=\"k\">Needs a person</span><span>An instruction: only a person's reviewer key approves it; any other key may reject it.</span>"
        ),
        "{page}"
    );
}

#[tokio::test]
async fn todos_second_page_holds_only_the_rows_past_the_first() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    for n in 0..201 {
        agent_writes_in(
            &rig,
            &value,
            &format!("Quote rule {n} holds for supplier {n}."),
            "alice",
        )
        .await;
    }
    let (_, _, first) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    assert_eq!(first.matches(">Judge</a>").count(), 200, "{first}");
    let marker = "/web/todo?cursor=";
    let at = first.rfind(marker).expect("a next page") + marker.len();
    let cursor = &first[at..at + first[at..].find(['"', '&']).expect("an end")];
    let (status, _, second) = send(
        &rig.router,
        get(&format!("/web/todo?cursor={cursor}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    // The 201st row alone, none of the first page's, and no page past it.
    assert_eq!(second.matches(">Judge</a>").count(), 1, "{second}");
    let first_ids: Vec<&str> = first
        .match_indices("/web/todo/judge?memory=")
        .map(|(i, m)| &first[i + m.len()..i + m.len() + 36])
        .collect();
    let (i, m) = second
        .match_indices("/web/todo/judge?memory=")
        .next()
        .expect("the last row");
    assert!(
        !first_ids.contains(&&second[i + m.len()..i + m.len() + 36]),
        "{second}"
    );
    assert!(!second.contains("Next page</a>"), "{second}");
}

#[tokio::test]
async fn the_judge_page_marks_a_memory_successor_of_steering_as_needing_a_person() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let steering = rig
        .app
        .submit_memory(
            value.as_str(),
            &oml_app::types::SubmitRequest {
                kind: oml_app::domain::MemoryKind::Steering,
                severity: None,
                idempotency_key: "s-quotes".to_owned(),
                body: "Quote in USD/t.".to_owned(),
                ..steering_req()
            },
        )
        .await
        .expect("the agent proposes steering")
        .receipt
        .memory_id;
    // The person's reviewer key approves it.
    let (judged, (status, _, page)) = judge_next(&rig, &cookie, "approve", None).await;
    assert_eq!(judged, steering.to_string());
    assert_eq!(status, StatusCode::OK, "{page}");
    // A memory successor would retire it, so approving it needs a person.
    let successor = rig
        .app
        .supersede(
            value.as_str(),
            &oml_app::types::SupersedeRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                expected_version: 1,
                idempotency_key: "s-quotes-memory".to_owned(),
                target_id: steering,
                body: "Quotes are in USD/t.".to_owned(),
                kind: Some(oml_app::domain::MemoryKind::Memory),
                retirement: false,
                derives_from: Vec::new(),
                reason: None,
                workspace: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("the agent proposes a memory successor")
        .receipt
        .memory_id;
    let (_, _, page) = send(
        &rig.router,
        get(
            &format!("/web/todo/judge?memory={successor}"),
            Some(&cookie),
        ),
    )
    .await;
    assert!(
        page.contains("<span class=\"k\">Needs a person</span>"),
        "{page}"
    );
}

/// A steering submission's fields apart from its kind, key and body.
fn steering_req() -> oml_app::types::SubmitRequest {
    oml_app::types::SubmitRequest {
        on_similar: Default::default(),
        anchor: None,
        origin: None,
        idempotency_key: String::new(),
        claim_key: None,
        verb: None,
        kind: oml_app::domain::MemoryKind::Steering,
        body: String::new(),
        workspace: None,
        derives_from: Vec::new(),
        severity: None,
        rule: None,
        kind_fields: None,
        valid_from: None,
        valid_until: None,
        format: None,
        title: None,
    }
}

/// The workspace page's Guardrails kind opens with In force here: what an
/// agent connected to the workspace loads, the inherited first, linking
/// where each comes from, then the workspace's own, with the counts under
/// the key's guardrail scope (ADR-0083 d12).
#[tokio::test]
async fn in_force_here_lists_the_roots_rule_as_here_and_on_a_nested_workspace_as_inherited() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_guardrail(&rig, &value, "Run the tests before every commit.").await;
    let (_, _, page) = send(
        &rig.router,
        get("/web/workspace?kind=guardrail", Some(&cookie)),
    )
    .await;
    assert!(
        page.contains(r#"<h2 id="in-force">In force here</h2>"#),
        "{page}"
    );
    assert!(page.contains(r#"<h1 class="mono">alice</h1>"#), "{page}");
    assert!(
        page.contains("<code>alice-web</code> loads 0 inherited and 0 from here, at most 12: its guardrail scope is <code>ancestors</code>. An agent's key loads by its own scope."),
        "{page}"
    );
    // The picker names the scope in force by its reference (design notes
    // row 20).
    assert!(
        page.contains(r#"name="workspace" type="text" value="alice""#),
        "{page}"
    );
    // Held, it is no rule yet: the set holds none (d3); the memories view
    // lists the agents' held rows.
    assert!(page.contains("No guardrail applies here."), "{page}");
    // The agent's held guardrail is listed for its person's reviewer key to
    // judge, and loads nowhere yet.
    assert!(
        page.contains(&format!(
            r#"href="/web/todo/judge?memory={id}">Judge it</a>"#
        )),
        "{page}"
    );
    // Approved by the person's reviewer key, it is in force.
    let (judged, (status, _, page)) = judge_next(&rig, &cookie, "approve", None).await;
    assert_eq!(judged, id.to_string());
    assert_eq!(status, StatusCode::OK, "{page}");
    let (_, _, page) = send(
        &rig.router,
        get("/web/workspace?kind=guardrail", Some(&cookie)),
    )
    .await;
    assert!(
        page.contains("<strong>must</strong> Run the tests before every commit."),
        "{page}"
    );
    assert!(page.contains(" · here</span></li>"), "{page}");
    assert!(
        page.contains("<code>alice-web</code> loads 0 inherited and 1 from here"),
        "{page}"
    );
    assert!(!page.contains("universe.alice"), "{page}");
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[
            ("action", "create_workspace"),
            ("name", "docs"),
            ("parent", "alice.proj"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let proj = rig
        .store
        .workspace_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "proj")
        .expect("proj")
        .workspace_id;
    let docs = rig
        .store
        .workspace_children(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "docs")
        .expect("docs")
        .workspace_id;
    let (status, _, nested) = send(
        &rig.router,
        get(
            &format!("/web/workspace?workspace={docs}&kind=guardrail"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{nested}");
    assert!(
        nested.contains(r#"<h1 class="mono">alice.docs</h1>"#),
        "{nested}"
    );
    assert!(
        nested.contains(r#"name="workspace" type="text" value="alice.docs""#),
        "{nested}"
    );
    assert!(
        nested.contains("<strong>must</strong> Run the tests before every commit."),
        "the parent's root rule applies to the nested workspace: {nested}"
    );
    assert!(
        nested.contains(r#"<a href="/web/workspace?workspace=alice&amp;kind=guardrail">inherited from alice</a>"#),
        "{nested}"
    );
    assert!(nested.contains("No guardrail in alice.docs."), "{nested}");
    // No page act writes, supersedes or retires a guardrail (d17).
    let (status, _, body) = act(
        &rig,
        &cookie,
        "/web",
        &[("action", "write_guardrail"), ("text", "Push freely.")],
    )
    .await;
    assert!(status.is_client_error(), "{status}: {body}");
}

#[tokio::test]
async fn the_guardrail_selector_includes_workspaces_after_the_first_tree_page() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for n in 0..200 {
        let name = format!("w{n:03}");
        let (status, _, body) = act(
            &rig,
            &cookie,
            "/web/workspaces",
            &[("action", "create_workspace"), ("name", &name)],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{name}: {body}");
    }
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let last = rig
        .store
        .workspace_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "w199")
        .expect("the child after the first 200-node page")
        .workspace_id;
    let (status, _, page) = send(
        &rig.router,
        get(
            &format!("/web/workspace?workspace={last}&kind=guardrail"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(r#"name="workspace" type="text" value="alice.w199""#),
        "the chosen workspace is named beyond the first page: {page}"
    );
    assert!(
        page.contains(r#"<option value="alice.w199"></option>"#),
        "the hints hold the workspace beyond the first page: {page}"
    );
}

// -- the Workspaces view (ADR-0045 d28), moved to oml-web ---------------------

#[tokio::test]
async fn a_workspace_act_is_confirmed_before_it_runs() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let proj = rig
        .store
        .workspace_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "proj")
        .expect("proj")
        .workspace_id;
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[
            ("action", "create_workspace"),
            ("name", "child"),
            ("parent", "alice.proj"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send(&rig.router, get("/web/workspaces", Some(&cookie))).await;
    // Each workspace by the reference every field takes, and the one it
    // sits in (design notes row 20).
    assert!(!page.contains("alice.proj.child"), "{page}");
    assert!(
        page.contains(">alice.child</a></td><td class=\"mono\">alice.proj</td>"),
        "{page}"
    );
    assert!(page.contains("<select name=\"op\""), "{page}");
    // A workspace in a person's root stays development, so no ladder move
    // is offered there (ADR-0085 d17).
    assert!(!page.contains("value=\"propose:protected\""), "{page}");
    assert!(page.contains("value=\"mode:in_progress\""), "{page}");
    assert!(!page.contains("value=\"propose:frozen\""), "{page}");
    assert!(!page.contains("value=\"mode:standard\""), "{page}");
    // Apply is a GET that changes nothing and asks again.
    let (status, _, confirm) = send(
        &rig.router,
        get(
            &format!("/web/workspaces?workspace={proj}&op=mode:in_progress"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    let node = || async {
        rig.store
            .workspace(rig.boot.tenant_id, proj)
            .await
            .unwrap()
            .unwrap()
    };
    assert!(
        !node().await.in_progress,
        "nothing ran before the confirmation"
    );
    assert!(confirm.contains("value=\"workspace_mode\""), "{confirm}");
    assert!(confirm.contains("name=\"csrf_token\""), "{confirm}");
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[
            ("action", "workspace_mode"),
            ("workspace", &proj.to_string()),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(node().await.in_progress);
    // The row names the mode as the drafts-visibility toggle does, the words
    // scripts/check_web_signed_in.sh reads on the router (ADR-0091 d16).
    let (_, _, after) = send(&rig.router, get("/web/workspaces", Some(&cookie))).await;
    assert!(
        after.contains("<td>development</td><td>drafts visible to everyone working here</td>"),
        "{after}"
    );
    // An act the key may not take now is refused by name at the
    // confirmation, with no form to run it.
    let (status, _, refused) = send(
        &rig.router,
        get(
            &format!("/web/workspaces?workspace={proj}&op=propose:protected"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{refused}");
    assert!(
        refused.contains("workspace.in_progress_refused"),
        "{refused}"
    );
    assert!(
        !refused.contains("value=\"workspace_transition\""),
        "{refused}"
    );
    // Commit; inside the root the ladder move is still refused by name at
    // the confirmation, with no form to run it (ADR-0085 d17).
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[
            ("action", "workspace_mode"),
            ("workspace", &proj.to_string()),
            ("mode", "standard"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    let (status, _, refused) = send(
        &rig.router,
        get(
            &format!("/web/workspaces?workspace={proj}&op=propose:protected"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{refused}");
    assert!(
        refused.contains("workspace.transition_refused"),
        "{refused}"
    );
    assert!(
        !refused.contains("value=\"workspace_transition\""),
        "{refused}"
    );
    // In a shared workspace an admin proposes protected through the
    // dropdown's action; the proposer is never offered the ratification,
    // and another admin is.
    let team = shared_team(&rig).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, page) = send(&rig.router, get("/web/admin/workspaces", Some(&boss))).await;
    assert!(page.contains("value=\"propose:protected\""), "{page}");
    let (status, _, page) = act(
        &rig,
        &boss,
        "/web/admin/workspaces",
        &[
            ("action", "workspace_transition"),
            ("workspace", &team.to_string()),
            ("to", "protected"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    let (_, _, page) = send(&rig.router, get("/web/admin/workspaces", Some(&boss))).await;
    assert!(!page.contains("value=\"ratify:"), "{page}");
    assert!(page.contains("proposed: protected"), "{page}");
    rig.allow("chief@example.com", "chief", true).await;
    let chief = signed_in(&rig, "sub-chief", "chief@example.com").await;
    let (status, _, page) = send(&rig.router, get("/web/admin/workspaces", Some(&chief))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let marker = "value=\"ratify:";
    let at = page
        .find(marker)
        .expect("another admin is offered the ratification")
        + marker.len();
    let transition = page[at..at + 36].to_owned();
    let (status, _, page) = act(
        &rig,
        &chief,
        "/web/admin/workspaces",
        &[
            ("action", "workspace_ratify"),
            ("workspace", &team.to_string()),
            ("transition", &transition),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    let team_node = rig
        .store
        .workspace(rig.boot.tenant_id, team)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(team_node.status.as_str(), "protected");
}

/// `team`, a shared workspace under universe, created with the tenant's
/// bootstrap admin key: the ladder moves outside every user root
/// (ADR-0085 d17).
async fn shared_team(rig: &Rig) -> uuid::Uuid {
    issuer_app(rig)
        .create_workspace(
            rig.boot.admin_key.value.as_str(),
            &Name::parse("team").expect("a name"),
            Some(rig.boot.root_workspace_id.into()),
            None,
        )
        .await
        .expect("a shared workspace")
        .workspace
        .workspace_id
}

async fn agent_writes_in(rig: &Rig, value: &str, body: &str, workspace: &str) -> uuid::Uuid {
    rig.app
        .submit_memory(
            value,
            &oml_app::types::SubmitRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                idempotency_key: format!("k-{body}"),
                claim_key: None,
                verb: Some(oml_app::types::SubmitVerb::Remember),
                kind: oml_app::domain::MemoryKind::Memory,
                body: body.to_owned(),
                workspace: Some(
                    oml_app::domain::name::Reference::parse(workspace).expect("a reference"),
                ),
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("the agent writes")
        .receipt
        .memory_id
}

/// A tenant reviewer an admin granted into alice's root, reviewer and
/// reader on its subtree: the key that puts another author's row there. A
/// key that is not alice's starts no memory in her root (ADR-0044 d16), so
/// what it holds there is a successor it proposed.
async fn reviewer_in_alices_root(rig: &Rig) -> oml_app::types::IssuedKey {
    tenant_reviewer_in(rig, "helper", "alice").await
}

/// The tenant reviewer `name`, reviewer and reader on `user`'s root.
async fn tenant_reviewer_in(rig: &Rig, name: &str, user: &str) -> oml_app::types::IssuedKey {
    rig.app
        .issue_key(
            &rig.boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Reviewer,
                name: Name::parse(name).expect("a name"),
                principal: name.to_owned(),
                used_by: Some(UsedBy::Agent),
                default_workspace: None,
                crew: None,
                grants: [Role::Reviewer, Role::Reader]
                    .into_iter()
                    .map(|role| GrantRequest {
                        node: Name::parse(user).expect("a name").into(),
                        role,
                        deny: Vec::new(),
                        covers: oml_app::domain::access::Covers::Subtree,
                        create: false,
                    })
                    .collect(),
                connect_code: false,
                user: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("a tenant reviewer on the root")
}

/// The reviewer approves `target`, a row alice's agent wrote, and proposes
/// `body` as its successor: a held row in the target's workspace that
/// another author wrote.
async fn reviewer_holds_a_successor(rig: &Rig, reviewer: &str, target: uuid::Uuid, body: &str) {
    rig.app
        .post_verdict(reviewer, target, oml_app::types::Verdict::Approve, None)
        .await
        .expect("the reviewer approves alice's row");
    rig.app
        .supersede(
            reviewer,
            &oml_app::types::SupersedeRequest {
                idempotency_key: format!("k-{body}"),
                on_similar: Default::default(),
                target_id: target,
                body: body.to_owned(),
                kind: None,
                retirement: false,
                derives_from: Vec::new(),
                reason: None,
                workspace: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                anchor: None,
                format: None,
                title: None,
                origin: None,
                expected_version: 1,
            },
        )
        .await
        .expect("the reviewer edits by proposal");
}

async fn person_with_agent(rig: &Rig, subject: &str, email: &str) -> (String, String) {
    let cookie = signed_in(rig, subject, email).await;
    let (_, _, page) = act_and_follow(
        rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    (cookie, value_of(&page))
}

async fn agent_writes(rig: &Rig, value: &str, body: &str) -> uuid::Uuid {
    rig.app
        .submit_memory(
            value,
            &oml_app::types::SubmitRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                idempotency_key: format!("k-{body}"),
                claim_key: None,
                verb: Some(oml_app::types::SubmitVerb::Remember),
                kind: oml_app::domain::MemoryKind::Memory,
                body: body.to_owned(),
                workspace: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("the agent writes")
        .receipt
        .memory_id
}

/// `uv run --with playwright`, when uv and a cached Playwright are here.
fn playwright() -> Option<std::process::Command> {
    let probe = std::process::Command::new("uv")
        .args([
            "run",
            "--quiet",
            "--with",
            "playwright",
            "python",
            "-c",
            "import playwright",
        ])
        .output();
    match probe {
        Ok(o) if o.status.success() => {
            let mut command = std::process::Command::new("uv");
            command.args(["run", "--quiet", "--with", "playwright", "python", "-I"]);
            Some(command)
        }
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_atlas_treemap_draws_and_zooms_with_no_script_in_chromium() {
    let Some(mut command) = playwright() else {
        println!("did not run: no Playwright (uv run --with playwright)");
        return;
    };
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let (cookie, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    for name in ["payments", "research"] {
        let (status, _, _) = act(
            &rig,
            &cookie,
            "/web/workspaces",
            &[("action", "create_workspace"), ("name", name)],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
    }
    let first = agent_writes(
        &rig,
        &agent,
        "The deploy window is Tuesday 14:00 to 16:00 CET",
    )
    .await;
    let (judged, _) = judge_next(&rig, &cookie, "approve", None).await;
    assert_eq!(judged, first.to_string());
    for (body, workspace) in [
        (
            "Checkout calls the payments service with an idempotency key per order",
            "alice.payments",
        ),
        (
            "Refunds above 500 EUR need a second approver",
            "alice.payments",
        ),
        (
            "The ledger settles at 02:00 UTC; retries wait for it",
            "alice.payments",
        ),
        (
            "Benchmarks: hybrid recall 0.97 success at 5 on the eval set",
            "alice.research",
        ),
        (
            "The reranker stays off until it fits the latency budget",
            "alice.research",
        ),
        ("Staging mirrors production schema weekly", "alice"),
        ("The on-call rotation changes on Mondays", "alice"),
    ] {
        agent_writes_in(&rig, &agent, body, workspace).await;
    }
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[
            ("action", "workspace_mode"),
            ("workspace", "alice.research"),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free port");
    let port = listener.local_addr().expect("an address").port();
    let router = rig.router.clone();
    tokio::spawn(async move { axum::serve(listener, router).await });
    let script = format!("{}/tests/chromium/atlas.py", env!("CARGO_MANIFEST_DIR"));
    let shots = std::env::var("OML_WEB_SCREENSHOTS").unwrap_or_default();
    command.args([
        script.as_str(),
        "--port",
        &port.to_string(),
        "--issuer",
        ISSUER,
        "--cookie",
        &cookie,
        "--admin-cookie",
        &boss,
        "--shots",
        &shots,
    ]);
    let output = tokio::task::spawn_blocking(move || command.output())
        .await
        .expect("the runner joins")
        .expect("Playwright runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_str(stdout.trim()).expect("one JSON report");
    if let Some(why) = report["did_not_run"].as_str() {
        println!("did not run: {why}");
        return;
    }
    assert_eq!(report["foreign"], json!([]), "no request left the origin");
    for who in ["person", "admin"] {
        for theme in ["dark", "light"] {
            for width in [1280, 375] {
                let tag = format!("{who}-{theme}-{width}");
                let at = |k: &str| report[format!("{tag}_{k}")].clone();
                assert!(at("cards").as_u64().unwrap() >= 7, "{tag}: {report}");
                assert_eq!(at("folded"), json!(true), "{tag}: compact by default");
                assert_eq!(at("scripts"), json!(0), "{tag}: the Atlas runs no script");
                assert!(at("regions").as_u64().unwrap() >= 1, "{tag}: {report}");
                assert!(
                    at("rows").as_u64().unwrap() >= at("regions").as_u64().unwrap(),
                    "{tag}: the table twin holds every drawn region"
                );
                assert_eq!(
                    at("hscroll"),
                    json!(false),
                    "{tag}: no page scroll sideways"
                );
                assert!(
                    at("zoomed_url")
                        .as_str()
                        .is_some_and(|u| u.starts_with(ISSUER) && u.contains("workspace=")),
                    "{tag}: a region zooms by a plain link: {report}"
                );
            }
        }
        assert!(report[format!("{who}-dark-1280_selects")].as_u64().unwrap() >= 1);
    }
    for line in report["console"].as_array().unwrap() {
        let line = line.as_str().unwrap();
        assert!(!line.contains("Content Security Policy"), "{line}");
        assert!(!line.starts_with("pageerror"), "{line}");
    }
}

/// Every hidden field of the form on `page` that posts `action`, the
/// action and the CSRF token among them.
fn hidden_fields(page: &str, action: &str) -> Vec<(String, String)> {
    let marker = format!(r#"name="action" value="{action}""#);
    let at = page.find(&marker).expect("the form");
    let start = page[..at].rfind("<form").expect("a form");
    let end = at + page[at..].find("</form>").expect("its end");
    page[start..end]
        .split(r#"<input type="hidden" name=""#)
        .skip(1)
        .map(|part| {
            let (name, rest) = part.split_once('"').expect("a name");
            let value = rest.split_once(r#"value=""#).expect("a value").1;
            (
                name.to_owned(),
                value[..value.find('"').expect("a quote")].replace("&amp;", "&"),
            )
        })
        .collect()
}

/// The value of the hidden field `name` on a page.
fn field_of(page: &str, name: &str) -> String {
    let marker = format!(r#"name="{name}" value=""#);
    let start = page.find(&marker).expect("the field") + marker.len();
    page[start..start + page[start..].find('"').expect("a quote")].to_owned()
}

/// `proj` under alice's root, created through the lead's act: its id.
async fn alice_proj(rig: &Rig, cookie: &str) -> uuid::Uuid {
    let (status, _, _) = act(
        rig,
        cookie,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let root = key_named(rig, "alice-web").await.user.expect("a root");
    rig.store
        .workspace_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "proj")
        .expect("proj")
        .workspace_id
}

#[tokio::test]
async fn a_resubmitted_confirmation_replays_its_act_and_acts_once() {
    // ADR-0070 d3 layer 4: the confirmation's form carries a nonce, and the
    // act's Idempotency-Key is its digest with the session token, so the
    // same form sent again replays at the API instead of acting again. A
    // ladder move happens in a shared workspace, where an admin proposes
    // and another ratifies (ADR-0085 d17).
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    rig.allow("chief@example.com", "chief", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let team = shared_team(&rig).await;
    let (status, _, confirm) = send(
        &rig.router,
        get(
            &format!("/web/admin/workspaces?workspace={team}&op=propose:protected"),
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    let fields = [
        ("action", "workspace_transition".to_owned()),
        ("csrf_token", field_of(&confirm, "csrf_token")),
        ("workspace", field_of(&confirm, "workspace")),
        ("to", field_of(&confirm, "to")),
        ("nonce", field_of(&confirm, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..2 {
        let (status, headers, page) = send(
            &rig.router,
            post("/web/admin/workspaces", &fields, Some(&boss)),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        assert_eq!(
            headers[header::LOCATION],
            "/web/admin/workspaces?done=proposed"
        );
    }
    let pending = rig
        .store
        .pending_transitions(rig.boot.tenant_id, team)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1, "one proposal for one form: {pending:?}");
    let proposed_nonce = field_of(&confirm, "nonce");
    // Another admin's ratification, sent twice, ratifies once and lands on
    // the view both times: without the replay the second would find the
    // proposal no longer live.
    let chief = signed_in(&rig, "sub-chief", "chief@example.com").await;
    let transition = pending[0].transition_id;
    let (status, _, confirm) = send(
        &rig.router,
        get(
            &format!("/web/admin/workspaces?workspace={team}&op=ratify:{transition}"),
            Some(&chief),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    // A fresh confirmation carries a fresh nonce.
    assert_ne!(field_of(&confirm, "nonce"), proposed_nonce);
    let fields = [
        ("action", "workspace_ratify".to_owned()),
        ("csrf_token", field_of(&confirm, "csrf_token")),
        ("workspace", team.to_string()),
        ("transition", field_of(&confirm, "transition")),
        ("nonce", field_of(&confirm, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..2 {
        let (status, headers, page) = send(
            &rig.router,
            post("/web/admin/workspaces", &fields, Some(&chief)),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        assert_eq!(
            headers[header::LOCATION],
            "/web/admin/workspaces?done=ratified"
        );
    }
    let node = rig
        .store
        .workspace(rig.boot.tenant_id, team)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(node.status.as_str(), "protected");
}

#[tokio::test]
async fn a_confirmation_sent_twice_at_once_acts_once() {
    // A double click sends the same form twice before either answers; the
    // second waits for the first and replays it (ADR-0070 d3 layer 4). Two
    // processes of oml-web racing the same key remain #233's case. The
    // ladder move is an admin's, in a shared workspace (ADR-0085 d17).
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let team = shared_team(&rig).await;
    let (_, _, confirm) = send(
        &rig.router,
        get(
            &format!("/web/admin/workspaces?workspace={team}&op=propose:protected"),
            Some(&boss),
        ),
    )
    .await;
    let fields = [
        ("action", "workspace_transition".to_owned()),
        ("csrf_token", field_of(&confirm, "csrf_token")),
        ("workspace", team.to_string()),
        ("to", "protected".to_owned()),
        ("nonce", field_of(&confirm, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..5 {
        let (a, b) = tokio::join!(
            send(
                &rig.router,
                post("/web/admin/workspaces", &fields, Some(&boss))
            ),
            send(
                &rig.router,
                post("/web/admin/workspaces", &fields, Some(&boss))
            ),
        );
        for (status, headers, page) in [a, b] {
            assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
            assert_eq!(
                headers[header::LOCATION],
                "/web/admin/workspaces?done=proposed"
            );
        }
    }
    let pending = rig
        .store
        .pending_transitions(rig.boot.tenant_id, team)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
}

#[tokio::test]
async fn a_nonce_that_is_not_one_the_page_draws_is_refused_and_nothing_runs() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let proj = alice_proj(&rig, &cookie).await;
    for nonce in ["", "x", "0123456789ABCDEF0123456789ABCDEF", &"0".repeat(64)] {
        let (status, _, page) = act(
            &rig,
            &cookie,
            "/web/workspaces",
            &[
                ("action", "workspace_mode"),
                ("workspace", &proj.to_string()),
                ("mode", "in_progress"),
                ("nonce", nonce),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{nonce:?}: {page}");
        assert!(page.contains("nonce"), "{page}");
    }
    let node = rig
        .store
        .workspace(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert!(!node.in_progress, "nothing ran");
}

#[tokio::test]
async fn the_workspaces_view_shows_only_the_notices_its_acts_name() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for (done, shown) in [
        ("proposed", Some("proposed")),
        ("ratified", Some("ratified")),
        ("mode+set", Some("mode set")),
        ("created", Some("created")),
        ("Your+account+is+suspended", None),
    ] {
        let (status, _, page) = send(
            &rig.router,
            get(&format!("/web/workspaces?done={done}"), Some(&cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        match shown {
            Some(text) => assert!(
                page.contains(&format!(r#"<p class="notice" role="status">{text}</p>"#)),
                "{done}: {page}"
            ),
            None => {
                assert!(!page.contains(r#"class="notice""#), "{done}: {page}");
                assert!(!page.contains("suspended"), "{page}");
            }
        }
    }
}

#[tokio::test]
async fn an_act_without_the_sessions_csrf_token_is_refused_before_any_api_call() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let proj = alice_proj(&rig, &cookie).await;
    let (status, _, body) = send(
        &rig.router,
        post(
            "/web/workspaces",
            &[
                ("action", "workspace_mode"),
                ("workspace", &proj.to_string()),
                ("mode", "in_progress"),
                ("csrf_token", "0000"),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("csrf_token"), "{body}");
    let node = rig
        .store
        .workspace(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert!(!node.in_progress, "nothing ran");
}

#[tokio::test]
async fn the_admin_workspaces_page_is_not_found_for_a_person_who_is_not_an_admin() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(&rig.router, get("/web/admin/workspaces", Some(&cookie))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/admin/workspaces",
        &[
            ("action", "workspace_mode"),
            ("workspace", "alice"),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
}

#[tokio::test]
async fn the_old_workspaces_urls_are_not_found_and_their_acts_run_nothing() {
    // The lead's 303s from the old URLs are deleted in v0.7.1 (ADR-0070,
    // "The old URLs deleted"): an old URL is not found, an old act is
    // refused and runs nothing, and the moved page answers at its own path.
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let proj = alice_proj(&rig, &cookie).await;
    for (old, who, new) in [
        ("/web?view=workspace", &cookie, "/web/workspaces"),
        ("/web/admin?view=workspace", &boss, "/web/admin/workspaces"),
    ] {
        let (status, headers, _) = send(&rig.router, get(old, Some(who))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
        let (status, _, page) = send(&rig.router, get(new, Some(who))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.contains("<h1>Workspaces</h1>"), "{page}");
    }
    // An old form's act is no action of POST /web and runs nothing.
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web",
        &[
            ("action", "workspace_mode"),
            ("workspace", &proj.to_string()),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(headers.get(header::LOCATION).is_none());
    let node = rig
        .store
        .workspace(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert!(!node.in_progress);
    // An old form's create runs nothing either: creating a workspace is the
    // view's own act through the API.
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web",
        &[("action", "create_workspace"), ("name", "docs")],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(headers.get(header::LOCATION).is_none());
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let docs = || async {
        rig.store
            .workspace_children(rig.boot.tenant_id, root)
            .await
            .unwrap()
            .into_iter()
            .any(|w| w.name.as_str() == "docs")
    };
    assert!(!docs().await, "the lead created nothing");
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "docs")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/workspaces?done=created");
    assert!(docs().await);
}

#[tokio::test]
async fn the_workspaces_pages_draw_the_header_and_tabs_the_leads_pages_draw() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    for (subject, email) in [("sub-1", EMAIL), ("sub-2", "bob@example.com")] {
        let cookie = signed_in(&rig, subject, email).await;
        let (_, _, lead) = send(&rig.router, get("/web/setup", Some(&cookie))).await;
        let (status, _, moved) = send(&rig.router, get("/web/workspaces", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(header_of(&moved), header_of(&lead), "{email}");
        assert!(
            moved.contains(r#"<a href="/web/workspaces" class="on" aria-current="page">"#),
            "{moved}"
        );
    }
    let boss = signed_in(&rig, "sub-2", "bob@example.com").await;
    let (_, _, lead) = send(&rig.router, get("/web/admin/people", Some(&boss))).await;
    let (status, _, moved) = send(&rig.router, get("/web/admin/workspaces", Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(header_of(&moved), header_of(&lead));
    assert!(
        moved.contains(r#"<a href="/web/admin/workspaces" class="on" aria-current="page">"#),
        "{moved}"
    );
}

// -- the hop itself ------------------------------------------------------------

/// The page header with the active tab's marker removed.
fn header_of(page: &str) -> String {
    let start = page.find(r#"<header class="top"#).expect("a header");
    let end = page[start..].find("</header>").expect("its end") + start;
    page[start..end].replace(r#" class="on" aria-current="page""#, "")
}

#[tokio::test]
async fn a_moved_page_draws_the_header_and_tabs_the_leads_pages_draw() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    for (subject, email) in [("sub-1", EMAIL), ("sub-2", "bob@example.com")] {
        let cookie = signed_in(&rig, subject, email).await;
        let (_, _, lead) = send(&rig.router, get("/web/setup", Some(&cookie))).await;
        let (status, headers, moved) =
            send(&rig.router, get("/web/workspace", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(header_of(&moved), header_of(&lead), "{email}");
        assert!(
            moved.contains(r#"<a href="/web/workspaces" class="on" aria-current="page">"#),
            "{moved}"
        );
        // The page came over the hop: oml-web's own policy, no provider.
        let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(!csp.contains("accounts.google.com"), "{csp}");
    }
}

/// The Guardrails view's path answers `308` to the workspace page's
/// Guardrails kind, keeping the workspace, before any session is read
/// (ADR-0083 d12).
#[tokio::test]
async fn the_old_guardrails_urls_answer_not_found_or_308_to_the_workspace_page() {
    // ADR-0070 d7 (b) as amended: each moved page has its own path under
    // /web/; the lead's 303 from the old ?view= form is deleted in v0.7.1
    // (ADR-0070, "The old URLs deleted").
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for old in ["/web?view=guardrails", "/web?view=guardrails&workspace=x"] {
        let (status, headers, _) = send(&rig.router, get(old, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
    }
    let (status, headers, _) = send(&rig.router, get("/web/guardrails", Some(&cookie))).await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert_eq!(location, "/web/workspace?kind=guardrail");
    let (status, _, page) = send(&rig.router, get(&location, Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("In force here"), "{page}");
}

#[tokio::test]
async fn a_moved_page_without_a_session_goes_to_the_leads_sign_in() {
    let rig = rig().await;
    let (status, headers, _) = send(&rig.router, get("/web/workspace", None)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert_eq!(location, "/web/sign-in?next=%2Fweb%2Fworkspace");
    let (status, _, page) = send(&rig.router, get(&location, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Sign in with Google"), "{page}");
    // The lead's sign-in carries the page on, to come back to it.
    assert!(
        page.contains(r#"name="next" value="/web/workspace""#),
        "{page}"
    );
}

#[tokio::test]
async fn a_signed_out_session_cookie_goes_to_sign_in_from_a_moved_page() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, _) = act(&rig, &cookie, "/web", &[("action", "sign_out")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, headers, _) = send(&rig.router, get("/web/workspace", Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION],
        "/web/sign-in?next=%2Fweb%2Fworkspace"
    );
}

// -- sign-out (#327): the header's control posts to the lead's POST /web ------

/// The sign-out control in a page's header, as the page draws it.
#[derive(Debug, PartialEq, Eq)]
struct SignOutForm {
    method: String,
    action: String,
    /// Its hidden fields, `(name, value)`, in the order drawn.
    fields: Vec<(String, String)>,
}

/// The sign-out control in a page's header; `None` where the header draws
/// none.
fn sign_out_form(page: &str) -> Option<SignOutForm> {
    let header = header_of(page);
    let start = header.find("<form ")?;
    let form = &header[start..start + header[start..].find("</form>")?];
    let attribute = |tag: &str, name: &str| {
        let marker = format!(r#"{name}=""#);
        let at = tag.find(&marker)? + marker.len();
        Some(tag[at..at + tag[at..].find('"')?].to_owned())
    };
    let fields = form
        .split("<input ")
        .skip(1)
        .filter_map(|input| {
            let input = &input[..input.find('>')?];
            (attribute(input, "type")? == "hidden")
                .then(|| Some((attribute(input, "name")?, attribute(input, "value")?)))?
        })
        .collect();
    Some(SignOutForm {
        method: attribute(form, "method")?,
        action: attribute(form, "action")?,
        fields,
    })
}

/// The sign-out control's post, sent as a browser sends the form the page
/// drew: its own action and its own fields, nothing added.
async fn sign_out_from(rig: &Rig, cookie: &str, page: &str) -> (StatusCode, HeaderMap, String) {
    let form = sign_out_form(page).expect("a sign-out control");
    assert_eq!(form.method, "post", "sign-out is a POST");
    let fields: Vec<(&str, &str)> = form
        .fields
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    send(&rig.router, post(&form.action, &fields, Some(cookie))).await
}

/// The status `GET /v2/whoami` answers the session's `capacity` credential,
/// sent as `oml-web` sends it: whether the credential still names a session.
async fn credential_status(rig: &Rig, cookie: &str, capacity: &str) -> StatusCode {
    let token = cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .expect("a session cookie")
        .1;
    let credential = oml_app::domain::web::capacity_credential(token, capacity);
    let request = Request::get("/v2/whoami")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header("oml-web-client", WEB_CLIENT_SECRET)
        .body(Body::empty())
        .expect("a request");
    send(&rig.router, request).await.0
}

/// Every page `oml-web` draws for a signed-in person carries the sign-out
/// control in its header: a form that posts `sign_out` with the session's
/// CSRF token to the lead's `POST /web`. A page for no one draws none
/// (ADR-0070 d2, d6; ADR-0045 d5, d8; #327).
#[tokio::test]
async fn every_signed_in_page_draws_the_sign_out_control_in_its_header() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    let (cookie, value) = person_with_agent(&rig, "sub-2", "bob@example.com").await;
    let memory = agent_writes(&rig, &value, "Sign-out is drawn on every page.").await;
    let token = cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .expect("a session cookie")
        .1;
    let expected = SignOutForm {
        method: "post".to_owned(),
        action: "/web".to_owned(),
        fields: vec![
            ("action".to_owned(), "sign_out".to_owned()),
            (
                "csrf_token".to_owned(),
                oml_web_credential::csrf_token(token),
            ),
        ],
    };
    // The routes that answer data to a script draw no page.
    let data = [
        oml_web::memories::USER_DATA_PATH,
        oml_web::memories::ADMIN_DATA_PATH,
        oml_web::start::STATUS_PATH,
        oml_web::start::EXPORT_PATH,
    ];
    // The old Review paths, and the Guardrails view's, answer a 308 to the
    // pages that replace them (ADR-0083 d6, d12) and draw nothing of their
    // own.
    let moved = [
        oml_web::USER_REVIEW_PATH,
        oml_web::ADMIN_REVIEW_PATH,
        oml_web::GUARDRAILS_PATH,
    ];
    for path in moved {
        let (status, _, _) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::PERMANENT_REDIRECT, "{path}");
    }
    let mut drawn = 0;
    for (_, path) in oml_web::PAGES
        .iter()
        .filter(|(method, path)| *method == "GET" && !data.contains(path) && !moved.contains(path))
    {
        let url = match *path {
            oml_web::USER_MEMORY_PATH | oml_web::ADMIN_MEMORY_PATH => {
                format!("{path}?id={memory}")
            }
            oml_web::TODO_JUDGE_PATH => format!("{path}?memory={memory}"),
            oml_web::ADMIN_KEYS_PATH => format!("{path}?user=bob"),
            oml_web::workspace::ADMIN_WORKSPACE_PATH => format!("{path}?workspace=bob"),
            // An edit or a retirement opens on a skill, a guardrail or a
            // document; the memory here is none, so its form answers the
            // refusal page, which draws the control too.
            oml_web::workspace::EDIT_PATH | oml_web::workspace::RETIRE_PATH => {
                format!("{path}?memory={memory}")
            }
            _ => (*path).to_owned(),
        };
        let (status, _, page) = send(&rig.router, get(&url, Some(&cookie))).await;
        // The fix page opens on a fix's id, which none here names.
        let refused = [
            oml_web::workspace::EDIT_PATH,
            oml_web::workspace::RETIRE_PATH,
            oml_web::todo::TODO_FIX_PATH,
        ];
        let want = if refused.contains(path) {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        };
        assert_eq!(status, want, "{url}: {page}");
        assert_eq!(sign_out_form(&page).as_ref(), Some(&expected), "{url}");
        assert!(
            header_of(&page).contains(r#"<button type="submit" class="linkish">Sign out</button>"#),
            "{url}"
        );
        drawn += 1;
    }
    assert_eq!(drawn, 29, "every page route of oml_web::PAGES was read");
    // A page an act answers is drawn by the same chrome.
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[("action", "cut_off"), ("key", "bob-laptop")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Cut off bob-laptop?"), "{page}");
    assert_eq!(sign_out_form(&page).as_ref(), Some(&expected));
    // No one is signed in on the lead's sign-in page.
    let (status, _, page) = send(&rig.router, get("/web", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Sign in with Google"), "{page}");
    assert!(!page.contains("sign_out"), "{page}");
}

/// A refusal page drawn for a signed-in person carries the same sign-out
/// control: a page not found, a query or a form refused, a conflict, the
/// refusal of a cut-off key's act and every page a cut-off person is
/// refused, from `oml-web` and from the lead, and the control on it ends
/// the session. A refusal answered before the session is read, or for no
/// one, draws none (ADR-0070 d2; ADR-0045 d5, d8; #325, #327).
#[tokio::test]
async fn every_refusal_page_of_a_signed_in_person_draws_the_sign_out_control_in_its_header() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    laptop_key(&rig, &cookie).await;
    let token = cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .expect("a session cookie")
        .1;
    let csrf = oml_web_credential::csrf_token(token);
    let expected = SignOutForm {
        method: "post".to_owned(),
        action: "/web".to_owned(),
        fields: vec![
            ("action".to_owned(), "sign_out".to_owned()),
            ("csrf_token".to_owned(), csrf.clone()),
        ],
    };
    let drawn =
        |what: &str, want: StatusCode, (status, _, page): (StatusCode, HeaderMap, String)| {
            assert_eq!(status, want, "{what}: {page}");
            assert!(page.contains("This cannot continue"), "{what}: {page}");
            assert_eq!(sign_out_form(&page).as_ref(), Some(&expected), "{what}");
            page
        };
    // Not found: an admin page for a person who is not an admin, on
    // `oml-web`, and an old URL, on the lead.
    for url in ["/web/admin/keys?user=alice", "/web?view=guardrails"] {
        drawn(
            url,
            StatusCode::NOT_FOUND,
            send(&rig.router, get(url, Some(&cookie))).await,
        );
    }
    // Refused as sent: a query that names a field twice, a form naming no
    // action of its page, on `oml-web` and on the lead, and on the lead a
    // form without the session's CSRF token, since the cookie names a live
    // session there.
    drawn(
        "a repeated field",
        StatusCode::BAD_REQUEST,
        send(
            &rig.router,
            get("/web/workspace?workspace=a&workspace=b", Some(&cookie)),
        )
        .await,
    );
    for path in ["/web/keys", "/web"] {
        drawn(
            path,
            StatusCode::BAD_REQUEST,
            act(&rig, &cookie, path, &[("action", "no_such_act")]).await,
        );
    }
    drawn(
        "the lead's form without the token",
        StatusCode::BAD_REQUEST,
        send(
            &rig.router,
            post("/web", &[("action", "sign_out")], Some(&cookie)),
        )
        .await,
    );
    // A conflict: a workspace name the namespace already holds.
    let create = [("action", "create_workspace"), ("name", "once")];
    let (status, _, page) = act(&rig, &cookie, "/web/workspaces", &create).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    drawn(
        "a name taken",
        StatusCode::CONFLICT,
        act(&rig, &cookie, "/web/workspaces", &create).await,
    );
    // Cut off: the key acts are refused on the Keys page, and so is every
    // page that reads under the key's grants.
    let name = |name: &str| oml_app::domain::name::Reference::Name(Name::parse(name).unwrap());
    rig.app
        .deny_key(
            &rig.boot.admin_key.value,
            name("alice-web"),
            name("universe"),
        )
        .await
        .expect("the admin cuts alice-web off");
    let page = drawn(
        "a cut-off key's act",
        StatusCode::FORBIDDEN,
        act(
            &rig,
            &cookie,
            "/web/keys",
            &[
                ("action", "issue_key"),
                ("name", "tablet"),
                ("role", "contributor"),
            ],
        )
        .await,
    );
    assert!(page.contains("signing out still works"), "{page}");
    for path in [
        "/web/workspace",
        "/web/workspaces",
        "/web/memories",
        "/web/atlas",
    ] {
        drawn(
            path,
            StatusCode::NOT_FOUND,
            send(&rig.router, get(path, Some(&cookie))).await,
        );
    }
    // Before the session is read, and for no one, there is no control: the
    // lead's refusal of another origin's form and of an unknown provider,
    // and `oml-web`'s refusal of a form without the token, which is no page.
    let request = Request::post("/web")
        .header(header::CONTENT_TYPE, FORM)
        .header("sec-fetch-site", "cross-site")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::COOKIE, &cookie)
        .body(Body::from(form(&[
            ("action", "sign_out"),
            ("csrf_token", csrf.as_str()),
        ])))
        .expect("a request");
    let (status, _, refused) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(sign_out_form(&refused), None, "{refused}");
    let (status, _, refused) = send(&rig.router, get("/web/callback/nowhere", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    assert_eq!(sign_out_form(&refused), None, "{refused}");
    let (status, _, refused) = send(
        &rig.router,
        post("/web/keys", &[("action", "issue_key")], Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert!(!refused.contains("<header"), "no page: {refused}");
    // The control on a refusal page ends the session.
    let (status, _, body) = sign_out_from(&rig, &cookie, &page).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        credential_status(&rig, &cookie, "web").await,
        StatusCode::UNAUTHORIZED
    );
}

/// The control ends the session on the server: after its post no capacity
/// credential of the session resolves at the API, the cookie opens no page,
/// and a second post finds no session (ADR-0045 d5; ADR-0070 d2; #327).
#[tokio::test]
async fn the_sign_out_control_ends_the_session_and_no_credential_of_it_works_afterwards() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    let cookie = signed_in(&rig, "sub-2", "bob@example.com").await;
    let other = signed_in(&rig, "sub-1", EMAIL).await;
    let capacities = ["web", "review", "admin"];
    for capacity in capacities {
        assert_eq!(
            credential_status(&rig, &cookie, capacity).await,
            StatusCode::OK,
            "{capacity}: live before the sign-out"
        );
    }
    let (_, _, page) = send(&rig.router, get("/web/workspace", Some(&cookie))).await;
    let (status, headers, body) = sign_out_from(&rig, &cookie, &page).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(headers[header::LOCATION], "/web");
    for capacity in capacities {
        assert_eq!(
            credential_status(&rig, &cookie, capacity).await,
            StatusCode::UNAUTHORIZED,
            "{capacity}: the credential ended with the session"
        );
    }
    // The cookie a browser failed to drop opens nothing.
    let (status, headers, _) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/sign-in?next=%2Fweb%2Fkeys");
    let (status, _, body) = sign_out_from(&rig, &cookie, &page).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    // Another person's session is untouched.
    assert_eq!(credential_status(&rig, &other, "web").await, StatusCode::OK);
}

/// The attributes of a `Set-Cookie` header, its name, value and `Max-Age`
/// aside.
fn cookie_attributes(whole: &str) -> Vec<&str> {
    whole
        .split("; ")
        .skip(1)
        .filter(|a| !a.starts_with("Max-Age="))
        .collect()
}

/// Signing out clears the session cookie with the attributes the sign-in
/// set it with, so the browser drops that cookie and no other, and lands on
/// the console's address, which is the sign-in page for no one (ADR-0045 d5;
/// #327).
#[tokio::test]
async fn signing_out_clears_the_cookie_as_it_was_set_and_lands_on_the_sign_in_page() {
    let rig = rig().await;
    let (location, sealed) = start(&rig, Provider::Google).await;
    let state = query_param(&location, "state").expect("a state");
    let nonce = query_param(&location, "nonce").expect("a nonce");
    rig.providers.answer(
        Provider::Google,
        Ok(ProviderAnswer::GoogleIdToken(google_token(
            &nonce, "sub-1", EMAIL, true,
        ))),
    );
    let (status, headers, body) = callback(
        &rig,
        Provider::Google,
        "the-code-4711",
        &state,
        &format!("{SIGN_IN_COOKIE}={sealed}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (token, set) = set_cookie(&headers, SESSION_COOKIE).expect("the session cookie");
    let cookie = format!("{SESSION_COOKIE}={token}");
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    let (status, headers, body) = sign_out_from(&rig, &cookie, &page).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (value, cleared) = set_cookie(&headers, SESSION_COOKIE).expect("the cookie cleared");
    assert_eq!(value, "", "{cleared}");
    assert!(cleared.split("; ").any(|a| a == "Max-Age=0"), "{cleared}");
    assert_eq!(cookie_attributes(&cleared), cookie_attributes(&set));
    assert_eq!(
        cookie_attributes(&set),
        ["Path=/", "Secure", "HttpOnly", "SameSite=Lax"]
    );
    // The browser follows the redirect with no cookie.
    let landing = headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned();
    assert_eq!(landing, "/web");
    let (status, _, page) = send(&rig.router, get(&landing, None)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Sign in with Google"), "{page}");
    assert_eq!(sign_out_form(&page), None, "no one is signed in");
}

/// Sign-out is the form's `POST` alone: a `GET` naming it ends nothing, on
/// the lead's address or on a page's, and neither does a `POST` without the
/// session's CSRF token or from another origin (ADR-0045 d8; #327).
#[tokio::test]
async fn sign_out_is_a_same_origin_post_with_the_sessions_csrf_token_and_never_a_get() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    let SignOutForm { action, fields, .. } = sign_out_form(&page).expect("a sign-out control");
    let query = form(
        &fields
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect::<Vec<_>>(),
    );
    for url in [format!("{action}?{query}"), format!("/web/keys?{query}")] {
        let (_, headers, _) = send(&rig.router, get(&url, Some(&cookie))).await;
        assert!(set_cookie(&headers, SESSION_COOKIE).is_none(), "{url}");
        assert_eq!(
            credential_status(&rig, &cookie, "web").await,
            StatusCode::OK,
            "{url}: the session is live"
        );
    }
    // Without the token, and with another session's.
    let other = signed_in(&rig, "sub-1", EMAIL).await;
    let other_token = other
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .expect("a session cookie")
        .1;
    let other_csrf = oml_web_credential::csrf_token(other_token);
    for fields in [
        vec![("action", "sign_out")],
        vec![("action", "sign_out"), ("csrf_token", other_csrf.as_str())],
    ] {
        let (status, _, body) = send(&rig.router, post(&action, &fields, Some(&cookie))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("csrf_token"), "{body}");
    }
    // From another origin, with the right token.
    let request = Request::post(action.as_str())
        .header(header::CONTENT_TYPE, FORM)
        .header("sec-fetch-site", "cross-site")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::COOKIE, &cookie)
        .body(Body::from(query.clone()))
        .expect("a request");
    let (status, _, body) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        credential_status(&rig, &cookie, "web").await,
        StatusCode::OK,
        "the session is live"
    );
    // The form as drawn, from the page's own origin, ends it.
    let (status, _, body) = sign_out_from(&rig, &cookie, &page).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        credential_status(&rig, &cookie, "web").await,
        StatusCode::UNAUTHORIZED
    );
}

/// A person whose `<user>-web` key an admin cut off still signs out from
/// the page: sign-out reads the session and nothing of the key (#325, #327).
#[tokio::test]
async fn a_cut_off_person_signs_out_from_the_page() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let name = |name: &str| oml_app::domain::name::Reference::Name(Name::parse(name).unwrap());
    rig.app
        .deny_key(
            &rig.boot.admin_key.value,
            name("alice-web"),
            name("universe"),
        )
        .await
        .expect("the admin cuts alice-web off");
    let (status, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let (status, _, body) = sign_out_from(&rig, &cookie, &page).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        credential_status(&rig, &cookie, "web").await,
        StatusCode::UNAUTHORIZED
    );
}

/// The Mermaid source alice's agent writes in the diagram tests.
const DIAGRAM: &str = "flowchart LR\n  A[Checkout] -->|calls| B[Payments]\n";

async fn agent_writes_diagram(rig: &Rig, value: &str) -> uuid::Uuid {
    agent_writes_diagram_of(rig, value, DIAGRAM).await
}

async fn agent_writes_diagram_of(rig: &Rig, value: &str, body: &str) -> uuid::Uuid {
    rig.app
        .submit_memory(
            value,
            &oml_app::types::SubmitRequest {
                origin: None,
                anchor: None,
                idempotency_key: "k-diagram".to_owned(),
                claim_key: None,
                verb: Some(oml_app::types::SubmitVerb::Remember),
                kind: oml_app::domain::MemoryKind::Memory,
                body: body.to_owned(),
                workspace: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                format: Some("text/vnd.oml.mermaid".to_owned()),
                title: Some("The payment flow".to_owned()),
                on_similar: Default::default(),
            },
        )
        .await
        .expect("the agent writes a diagram")
        .receipt
        .memory_id
}

/// A signed-in person whose agent wrote a diagram: the cookie and the id.
async fn person_with_a_diagram(rig: &Rig) -> (String, uuid::Uuid) {
    let cookie = signed_in(rig, "sub-1", EMAIL).await;
    let (_, _, page) = act_and_follow(
        rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    let memory = agent_writes_diagram(rig, &value_of(&page)).await;
    (cookie, memory)
}

fn frame_token(page: &str) -> String {
    let at = page.find("src=\"/web/render/").expect("an iframe") + "src=\"/web/render/".len();
    page[at..at + page[at..].find('"').expect("an end")].to_owned()
}

impl Rig {
    /// The journal's last sequence number.
    async fn head(&self) -> u64 {
        self.store
            .chain_head(self.boot.tenant_id)
            .await
            .expect("a head")
            .expect("a chain")
            .seq
    }
}

// -- the memory's own page, curation and the frame (ADR-0045 d10, d12, d21;
// ADR-0056 d9), moved to oml-web

#[tokio::test]
async fn the_parent_pages_carry_no_script_and_frame_only_the_render_route() {
    let rig = rig().await;
    let (cookie, memory) = person_with_a_diagram(&rig).await;
    let _ = memory;
    let (status, headers, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(!page.contains("<script"), "the parent page has no script");
    assert!(
        page.contains("<iframe sandbox=\"allow-scripts\" src=\"/web/render/"),
        "{page}"
    );
    assert!(
        !page.contains("allow-same-origin"),
        "the frame is an opaque origin"
    );
    assert!(
        page.contains("flowchart LR"),
        "the source is beside the drawing: {page}"
    );
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(
        csp.contains(&format!("frame-src {ISSUER}/web/render/;")),
        "{csp}"
    );
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(
        !csp.contains("script-src"),
        "no page may run a script: {csp}"
    );
}

#[tokio::test]
async fn the_render_frame_answers_under_its_own_policy_with_the_source_escaped() {
    let rig = rig().await;
    let (cookie, memory) = person_with_a_diagram(&rig).await;
    let _ = memory;
    let (_, _, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&cookie)),
    )
    .await;
    let token = frame_token(&page);
    let (status, headers, body) = send(
        &rig.router,
        get(&format!("/web/render/{token}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert_eq!(csp, oml_web::render::frame_policy(ISSUER));
    for want in [
        "sandbox allow-scripts;",
        "default-src 'none'",
        "connect-src 'none'",
        "img-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
        "frame-ancestors https://oml.example.com",
    ] {
        assert!(csp.contains(want), "{want} in {csp}");
    }
    assert!(!csp.contains("unsafe-eval"), "{csp}");
    assert!(!csp.contains("allow-same-origin"), "{csp}");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    // The frame sends no referrer, so the token in its URL goes nowhere
    // (ADR-0070, the sixth change's table, d3 layer 5).
    assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
    assert!(
        body.contains("<pre class=\"mermaid\">flowchart LR"),
        "{body}"
    );
    assert!(
        body.contains("-&gt;") || body.contains("--&gt;") || body.contains("&gt;"),
        "escaped: {body}"
    );
    assert!(body.contains(&oml_web::render::bundle_path()), "{body}");
}

#[tokio::test]
async fn a_render_token_is_bound_to_its_session_and_a_forged_or_altered_one_does_not_open() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", false).await;
    let (cookie, memory) = person_with_a_diagram(&rig).await;
    let _ = memory;
    let (_, _, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&cookie)),
    )
    .await;
    let token = frame_token(&page);
    let bob = signed_in(&rig, "sub-2", "bob@example.com").await;
    for (who, path) in [
        (Some(bob.as_str()), format!("/web/render/{token}")),
        (None, format!("/web/render/{token}")),
        (Some(cookie.as_str()), format!("/web/render/{}0", token)),
        (Some(cookie.as_str()), "/web/render/forged".to_owned()),
    ] {
        let (status, _, body) = send(&rig.router, get(&path, who)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        assert!(!body.contains("flowchart"), "{body}");
    }
    let (status, headers, body) =
        send(&rig.router, get(&oml_web::render::bundle_path(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    assert!(
        headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    assert!(!body.is_empty());
    let (status, _, _) = send(&rig.router, get("/web/assets/mermaid-0000.js", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no other file is served");
}

#[tokio::test]
async fn a_render_token_names_the_diagram_and_carries_no_source_so_a_large_one_draws() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    // About 56 KiB of source: under the 64 KiB cap, and a URL path of that
    // size, once sealed and encoded, is more than a server will accept.
    let mut big = String::from("flowchart LR\n");
    for i in 0..100 {
        big.push_str(&format!("  N{i}[\"{}\"] --> N{}\n", "x".repeat(540), i + 1));
    }
    big.push_str("  N100[\"the last node\"]\n");
    assert!(big.len() > 55_000 && big.len() < 64 * 1024, "{}", big.len());
    let memory = agent_writes_diagram_of(&rig, &value, &big).await;
    let (_, _, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&cookie)),
    )
    .await;
    let token = frame_token(&page);
    assert!(
        token.len() < 1024,
        "a token names the diagram, it does not carry it: {} bytes",
        token.len()
    );
    let (status, _, body) = send(
        &rig.router,
        get(&format!("/web/render/{token}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", &body[..body.len().min(300)]);
    assert!(
        body.contains("the last node"),
        "the whole source is drawn from"
    );
}

#[tokio::test]
async fn an_expired_render_token_does_not_open_through_the_route() {
    let rig = rig().await;
    let (cookie, memory) = person_with_a_diagram(&rig).await;
    let token_value = cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .map_or("", |(_, v)| v);
    let csrf = oml_web_credential::csrf_token(token_value);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let id = memory.to_string();
    let live = oml_web::render::mint(&csrf, &id, DIAGRAM, now);
    let (status, _, _) = send(
        &rig.router,
        get(&format!("/web/render/{live}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a token minted now opens");
    let expired = oml_web::render::mint(
        &csrf,
        &id,
        DIAGRAM,
        now - oml_web::render::RENDER_TTL_SECS - 1,
    );
    let (status, _, body) = send(
        &rig.router,
        get(&format!("/web/render/{expired}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(!body.contains("flowchart"), "{body}");
}

#[tokio::test]
async fn a_memory_whose_text_names_the_render_route_frames_nothing() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_writes(&rig, &value, "the frame lives at /web/render/ on oml-web").await;
    let (status, headers, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={id}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(!page.contains("<iframe"), "{page}");
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(!csp.contains("frame-src"), "{csp}");
}

/// The key a memory found by `text` was written by, as the admin reads it.
async fn author_of(rig: &Rig, text: &str) -> uuid::Uuid {
    let page = rig
        .app
        .admin_search(
            rig.boot.admin_key.value.as_str(),
            Some(text),
            None,
            None,
            None,
            10,
            Some(boot_investigation(rig).await),
        )
        .await
        .expect("the admin reads");
    page.items
        .iter()
        .find(|m| m.memory.body == text)
        .unwrap_or_else(|| panic!("a memory reading {text}"))
        .memory
        .author_key_id
}

#[tokio::test]
async fn a_supersession_runs_as_the_curator_on_its_own_claim_and_as_the_reviewer_on_any_other() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let alices = agent_writes(&rig, &value, "the release train leaves on fridays").await;
    let (status, _, _) = judge(&rig, &cookie, &alices.to_string(), "approve", None).await;
    assert_eq!(status, StatusCode::OK);
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (action, workspace) in [
        ("grant_curation", "alice"),
        ("grant_review", "universe"),
        ("grant_review", "alice"),
    ] {
        let (status, _, body) = act(
            &rig,
            &admin,
            "/web/admin/people",
            &[
                ("action", action),
                ("user", "boss"),
                ("workspace", workspace),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}: {body}");
    }
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    // Another person judges boss's curated claim: outside boss's own
    // root no key of boss counts, boss-review included (ADR-0082 d7).
    let carol = reviewer_on_universe(&rig, &admin).await;
    let (status, _, body) = act(
        &rig,
        &admin,
        "/web/admin/curate",
        &[
            ("action", "curate"),
            ("source", &alices.to_string()),
            ("text", "Releases ship weekly."),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let curate = key_named(&rig, "boss-curate").await.key_id;
    let review = key_named(&rig, "boss-review").await.key_id;
    let admin_key = key_named(&rig, "boss-admin").await.key_id;
    let curated = rig
        .app
        .admin_search(
            rig.boot.admin_key.value.as_str(),
            Some("weekly"),
            None,
            None,
            None,
            10,
            Some(boot_investigation(&rig).await),
        )
        .await
        .expect("the admin reads")
        .items[0]
        .memory
        .memory_id;
    // carol-review is outside the curated claim's lineage and its person, and approves it.
    let (status, _, _) = judge(&rig, &carol, &curated.to_string(), "approve", None).await;
    assert_eq!(status, StatusCode::OK);
    for (target, text, writer) in [
        (curated, "Releases ship every week.", curate),
        (alices, "the release train leaves on thursdays", review),
    ] {
        let target = target.to_string();
        let (status, _, body) = act(
            &rig,
            &admin,
            "/web/admin/memory",
            &[
                ("action", "supersede"),
                ("memory", &target),
                ("expected_version", "1"),
                ("text", text),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{text}: {body}");
        let author = author_of(&rig, text).await;
        assert_eq!(author, writer, "{text}");
        assert_ne!(author, admin_key, "never the admin key");
    }
    assert_ne!(
        author_of(&rig, "the release train leaves on thursdays").await,
        curate
    );
}

/// carol, a second admin designated to review on `universe` by `admin`'s
/// session: her signed-in cookie, a reviewer outside boss's person.
async fn reviewer_on_universe(rig: &Rig, admin: &str) -> String {
    rig.allow("carol@example.com", "carol", true).await;
    signed_in(rig, "sub-carol", "carol@example.com").await;
    let (status, _, body) = act(
        rig,
        admin,
        "/web/admin/people",
        &[
            ("action", "grant_review"),
            ("user", "carol"),
            ("workspace", "universe"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    signed_in(rig, "sub-carol", "carol@example.com").await
}

/// Every memory whose text is `text`, by the admin read.
async fn memories_reading(rig: &Rig, text: &str) -> Vec<uuid::Uuid> {
    rig.app
        .admin_search(
            rig.boot.admin_key.value.as_str(),
            Some(text),
            None,
            None,
            None,
            10,
            Some(boot_investigation(rig).await),
        )
        .await
        .expect("the admin reads")
        .items
        .iter()
        .filter(|m| m.memory.body == text)
        .map(|m| m.memory.memory_id)
        .collect()
}

/// The admin retires `target`, a served version 1, on the admin memory
/// page, and `reviewer` ratifies the retirement: the retirement's id.
async fn retired_on_the_admin_page(
    rig: &Rig,
    admin: &str,
    target: uuid::Uuid,
    text: &str,
    reviewer: &str,
) -> uuid::Uuid {
    let (status, _, body) = act(
        rig,
        admin,
        "/web/admin/memory",
        &[
            ("action", "supersede"),
            ("memory", &target.to_string()),
            ("expected_version", "1"),
            ("text", text),
            ("retirement", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let [retirement] = memories_reading(rig, text).await[..] else {
        panic!("one memory reading {text}");
    };
    let verdict = rig
        .app
        .post_verdict(reviewer, retirement, oml_app::types::Verdict::Approve, None)
        .await
        .expect("a reviewer ratifies the retirement");
    assert_eq!(verdict.state, oml_app::domain::MemoryStatus::Promoted);
    retirement
}

/// An admin retires a memory of his own on the admin memory page, a
/// reviewer ratifies the retirement, and he puts the claim back on the
/// same page. Both acts run as his `<user>-review`, a session key that is
/// his though it carries no user root, so the successor of the ratified
/// retirement in his own root is proposed and the act lands on the memory
/// (ADR-0044 d16; ADR-0045 d6, d12).
#[tokio::test]
async fn an_admin_retires_his_own_memory_and_puts_it_back_on_the_admin_memory_page() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let (boss, agent) = person_with_agent(&rig, "sub-boss", "boss@example.com").await;
    let own = agent_writes(&rig, &agent, "the pump runs at night").await;
    let ratifier = tenant_reviewer_in(&rig, "ratifier", "boss").await;
    rig.app
        .post_verdict(&ratifier.value, own, oml_app::types::Verdict::Approve, None)
        .await
        .expect("a reviewer approves his agent's write");
    let retirement =
        retired_on_the_admin_page(&rig, &boss, own, "the pump was removed", &ratifier.value).await;

    let (status, headers, body) = act(
        &rig,
        &boss,
        "/web/admin/memory",
        &[
            ("action", "supersede"),
            ("memory", &retirement.to_string()),
            ("expected_version", "2"),
            ("text", "the pump runs at night again"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let location = headers[header::LOCATION].to_str().expect("ASCII");
    assert_eq!(
        location,
        format!("/web/admin/memory?id={retirement}&done=proposed")
    );
    let (status, _, _) = send(&rig.router, get(location, Some(&boss))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        author_of(&rig, "the pump runs at night again").await,
        key_named(&rig, "boss-review").await.key_id
    );
}

/// An admin designated for review on another person's root retires a
/// memory of theirs on the admin memory page, and once the retirement is
/// ratified his put-back is refused by its code: his `<user>-review` is his
/// key, never read as theirs, and only their keys put their claim back in
/// service (ADR-0044 d16; ADR-0045 d10). Nothing is written.
#[tokio::test]
async fn the_admin_memory_page_refuses_putting_another_persons_retired_memory_back() {
    let rig = rig().await;
    let (_, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let alices = agent_writes(&rig, &agent, "the relay closes at noon").await;
    let ratifier = tenant_reviewer_in(&rig, "ratifier", "alice").await;
    rig.app
        .post_verdict(
            &ratifier.value,
            alices,
            oml_app::types::Verdict::Approve,
            None,
        )
        .await
        .expect("a reviewer approves alice's agent's write");
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, body) = act(
        &rig,
        &boss,
        "/web/admin/people",
        &[
            ("action", "grant_review"),
            ("user", "boss"),
            ("workspace", "alice"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let retirement = retired_on_the_admin_page(
        &rig,
        &boss,
        alices,
        "the relay was removed",
        &ratifier.value,
    )
    .await;

    let (status, _, body) = act(
        &rig,
        &boss,
        "/web/admin/memory",
        &[
            ("action", "supersede"),
            ("memory", &retirement.to_string()),
            ("expected_version", "2"),
            ("text", "the relay closes at noon again"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("submission.user_root_refused"));
    assert!(
        memories_reading(&rig, "the relay closes at noon again")
            .await
            .is_empty(),
        "a refused put-back writes no version"
    );
}

#[tokio::test]
async fn an_admin_without_a_curator_key_on_the_session_is_told_to_sign_in_again_and_nothing_is_written()
 {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let source = agent_writes(&rig, &value, "a source to curate").await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let head = rig.head().await;
    let (status, _, body) = act(
        &rig,
        &admin,
        "/web/admin/curate",
        &[
            ("action", "curate"),
            ("source", &source.to_string()),
            ("text", "a curated copy"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains("next sign-in"), "{body}");
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .all(|e| !e.envelope.event_type.starts_with("submission."))
    );
}

#[tokio::test]
async fn a_person_who_is_not_an_admin_posts_to_neither_admin_act() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_writes(&rig, &value, "a row a person cannot supersede here").await;
    let head = rig.head().await;
    let target = id.to_string();
    for (path, fields) in [
        (
            "/web/admin/memory",
            vec![
                ("action", "supersede"),
                ("memory", target.as_str()),
                ("expected_version", "1"),
                ("text", "replaced"),
            ],
        ),
        (
            "/web/admin/curate",
            vec![
                ("action", "curate"),
                ("source", target.as_str()),
                ("text", "a copy"),
            ],
        ),
    ] {
        let (status, _, body) = act(&rig, &cookie, path, &fields).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
    }
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .all(|e| !e.envelope.event_type.starts_with("submission."))
    );
}

#[tokio::test]
async fn a_person_reaches_nothing_of_another_persons_root_on_oml_webs_pages() {
    // The web half of open_signup_pg's isolation, where the pages now live.
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", false).await;
    let (ada, memory) = person_with_a_diagram(&rig).await;
    let (_, _, issued) = act_and_follow(
        &rig,
        &ada,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "desk"),
            ("role", "contributor"),
        ],
    )
    .await;
    let value = value_of(&issued);
    let held = agent_writes(&rig, &value, "Kilimanjaro budget held for review").await;
    let promoted = agent_writes(&rig, &value, "Tanganyika vendor shortlist").await;
    let (status, _, _) = judge(&rig, &ada, &promoted.to_string(), "approve", None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&ada)),
    )
    .await;
    let token = frame_token(&page);
    let bob = signed_in(&rig, "sub-2", "bob@example.com").await;
    for path in ["/web/memories", "/web/atlas"] {
        let (status, _, html) = send(&rig.router, get(path, Some(&bob))).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        for secret in ["Kilimanjaro", "Tanganyika", "flowchart"] {
            assert!(!html.contains(secret), "{path}: {html}");
        }
    }
    for id in [held, promoted, memory] {
        let (status, _, html) = send(
            &rig.router,
            get(&format!("/web/memory?id={id}"), Some(&bob)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{id}");
        for secret in ["Kilimanjaro", "Tanganyika", "flowchart"] {
            assert!(!html.contains(secret), "{id}: {html}");
        }
    }
    // Next finds nothing of ada's for bob's reviewer key, and neither To do
    // nor the Judge page of ada's held row shows any of it.
    let (status, _, todo) = act_and_follow(&rig, &bob, "/web/todo", &[("action", "next")]).await;
    assert_eq!(status, StatusCode::OK, "{todo}");
    assert!(todo.contains("Nothing waits for this account."), "{todo}");
    let (_, _, judge_page) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={held}"), Some(&bob)),
    )
    .await;
    for page in [&todo, &judge_page] {
        assert!(!page.contains("/web/todo/judge?memory="), "{page}");
        for secret in ["Kilimanjaro", "Tanganyika", "flowchart"] {
            assert!(!page.contains(secret), "{page}");
        }
    }
    let (status, _, frame) = send(
        &rig.router,
        get(&format!("/web/render/{token}"), Some(&bob)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!frame.contains("flowchart"));
}

#[tokio::test]
async fn the_admin_memory_page_renders_the_admin_read_with_its_attribution() {
    // Moved from the lead's the_admin_read_serves_text_and_attribution_and_journals_the_read,
    // whose API half stays there.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "zebra crossing secret").await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(
        &rig.router,
        get(&format!("/web/admin/memory?id={memory}"), Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("zebra crossing secret") && page.contains("alice-laptop"),
        "{page}"
    );
    assert!(page.contains("No verdict yet."), "{page}");
    assert!(
        page.contains(&format!(r#"href="/web/admin/curate?memory={memory}""#)),
        "{page}"
    );
    // A person who is not an admin, and the person's own page, get no
    // admin read.
    let (status, _, _) = send(
        &rig.router,
        get(&format!("/web/admin/memory?id={memory}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_admin_page_supersedes_and_retires_no_guardrail() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_guardrail(&rig, &value, "Run the tests before every commit.").await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let head = rig.head().await;
    for retirement in [None, Some(("retirement", "yes"))] {
        let target = id.to_string();
        let mut fields = vec![
            ("action", "supersede"),
            ("memory", target.as_str()),
            ("expected_version", "1"),
            ("text", "Run the tests sometimes."),
        ];
        fields.extend(retirement);
        let (status, _, body) = act(&rig, &admin, "/web/admin/memory", &fields).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("guardrail"), "{body}");
    }
    // The admin read itself is journalled; no version was written.
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .all(|e| !e.envelope.event_type.starts_with("submission.")),
        "{:?}",
        events
            .iter()
            .map(|e| &e.envelope.event_type)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_resubmitted_supersede_form_proposes_one_version_and_lands_on_the_memory() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_writes(&rig, &value, "the build takes ten minutes").await;
    // Approved first, so the reviewer key may propose its successor.
    let (status, _, _) = judge(&rig, &cookie, &id.to_string(), "approve", None).await;
    assert_eq!(status, StatusCode::OK);
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, _) = act(
        &rig,
        &admin,
        "/web/admin/people",
        &[
            ("action", "grant_review"),
            ("user", "boss"),
            ("workspace", "alice"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send(
        &rig.router,
        get(&format!("/web/admin/memory?id={id}"), Some(&admin)),
    )
    .await;
    let nonce = field_of(&page, "nonce");
    let target = id.to_string();
    let fields = [
        ("action", "supersede"),
        ("memory", target.as_str()),
        ("expected_version", "1"),
        ("text", "the build takes twelve minutes"),
        ("nonce", nonce.as_str()),
    ];
    let head = rig.head().await;
    for _ in 0..2 {
        let (status, headers, body) = act(&rig, &admin, "/web/admin/memory", &fields).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        assert_eq!(
            headers[header::LOCATION],
            format!("/web/admin/memory?id={id}&done=proposed").as_str()
        );
    }
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    let proposed = events
        .iter()
        .filter(|e| e.envelope.event_type.starts_with("submission."))
        .count();
    assert_eq!(
        proposed, 1,
        "one version proposed, the second send replayed"
    );
    let (_, _, landed) = send(
        &rig.router,
        get(
            &format!("/web/admin/memory?id={id}&done=proposed"),
            Some(&admin),
        ),
    )
    .await;
    assert!(
        landed.contains("Proposed: held until reviewed."),
        "{landed}"
    );
}

#[tokio::test]
async fn the_old_memory_and_curate_urls_are_not_found_and_the_pages_answer_at_their_own_paths() {
    // The lead's 303s from the old URLs are deleted in v0.7.1 (ADR-0070,
    // "The old URLs deleted"): an old URL is not found, an old act is
    // refused and runs nothing, and the moved page answers at its own path.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_writes(&rig, &value, "an old link still opens").await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (old, who, new) in [
        (
            format!("/web?view=memories&memory={id}"),
            cookie.as_str(),
            format!("/web/memory?id={id}"),
        ),
        (
            format!("/web/admin?view=memories&memory={id}"),
            boss.as_str(),
            format!("/web/admin/memory?id={id}"),
        ),
        (
            "/web/admin?view=curate".to_owned(),
            boss.as_str(),
            "/web/admin/curate".to_owned(),
        ),
    ] {
        let (status, headers, _) = send(&rig.router, get(&old, Some(who))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
        let (status, _, page) = send(&rig.router, get(&new, Some(who))).await;
        assert_eq!(status, StatusCode::OK, "{new}: {page}");
    }
    // Each moved page draws the header and tabs, the admin's naming Curate
    // at its own path.
    let (_, _, curate) = send(&rig.router, get("/web/admin/curate", Some(&boss))).await;
    assert!(
        curate.contains(r#"href="/web/admin/curate""#) && curate.contains("<h1>Curate</h1>"),
        "{curate}"
    );
    // Without a session each goes to the lead's sign-in, and the admin
    // pages are not found for a person who is not an admin.
    for path in ["/web/memory", "/web/admin/memory", "/web/admin/curate"] {
        let (status, headers, _) = send(&rig.router, get(path, None)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{path}");
        assert!(
            headers[header::LOCATION]
                .to_str()
                .unwrap()
                .starts_with("/web/sign-in"),
            "{path}"
        );
    }
    for path in [
        format!("/web/admin/memory?id={id}"),
        "/web/admin/curate".to_owned(),
    ] {
        let (status, _, _) = send(&rig.router, get(&path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

// -- Start, setup, the admin overview and the users page (ADR-0068 d5,
// d6, d8, d10; ADR-0045 d10, d13), moved to oml-web

#[tokio::test]
async fn the_setup_page_names_who_and_the_organisation_and_never_the_email() {
    // Moved from the lead's a_listed_verified_google_email_signs_in_and_sees_who_it_is.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", "Alice@Example.com").await;
    let (status, _, page) = send(&rig.router, get("/web/setup", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("alice · oml"),
        "who and the organisation: {page}"
    );
    assert!(!page.contains(EMAIL), "the page never echoes the email");
    assert!(
        page.contains(ISSUER),
        "the server an agent connects to: {page}"
    );
    assert!(
        page.contains("give each agent this URL with its key"),
        "{page}"
    );
    assert!(!page.to_lowercase().contains("default"), "{page}");
    assert!(!page.contains("doublegate"), "{page}");
}

#[tokio::test]
async fn a_non_admin_session_gets_not_found_on_the_overview_and_the_users_page() {
    // Moved from the lead's a_non_admin_session_gets_not_found_on_the_admin_page.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for path in ["/web/admin/people", "/web/admin/users"] {
        let (status, _, body) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert!(body.contains("access.not_found"), "{body}");
        let (status, headers, _) = send(&rig.router, get(path, None)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{path}");
        assert!(
            headers[header::LOCATION]
                .to_str()
                .unwrap()
                .starts_with("/web/sign-in"),
            "{path}"
        );
    }
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(&rig.router, get("/web/admin/people", Some(&admin))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Administration") && page.contains(EMAIL),
        "{page}"
    );
}

#[tokio::test]
async fn the_last_admin_address_is_never_removed_or_unflagged_from_the_page() {
    // Moved from the lead; the refusal is now the API's keep_an_admin.
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for fields in [
        vec![
            ("action", "set_admin"),
            ("email", "boss@example.com"),
            ("admin", "no"),
        ],
        vec![
            ("action", "remove_email"),
            ("email", "boss@example.com"),
            ("confirm", "yes"),
        ],
    ] {
        let (status, _, body) = act(&rig, &admin, "/web/admin/people", &fields).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("admin"), "{body}");
    }
    let (status, _, _) = act(
        &rig,
        &admin,
        "/web/admin/people",
        &[
            ("action", "allow_email"),
            ("email", "carol@example.com"),
            ("user", "carol"),
            ("admin", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _, _) = act(
        &rig,
        &admin,
        "/web/admin/people",
        &[
            ("action", "set_admin"),
            ("email", "carol@example.com"),
            ("admin", "no"),
            // Carol is another admin person: the act carries a reason
            // (ADR-0097 d8, d26).
            ("reason", "carol moves teams"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn a_resubmitted_allowlist_form_acts_once_and_lands_on_the_overview() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let nonce = "0123456789abcdef0123456789abcdef";
    for (fields, done) in [
        (
            vec![
                ("action", "allow_email"),
                ("email", "carol@example.com"),
                ("user", "carol"),
                ("admin", "no"),
                ("nonce", nonce),
            ],
            "/web/admin/people?done=added",
        ),
        (
            vec![
                ("action", "remove_email"),
                ("email", "carol@example.com"),
                ("confirm", "yes"),
                ("nonce", "fedcba9876543210fedcba9876543210"),
            ],
            "/web/admin/people?done=removed",
        ),
    ] {
        let head = rig.head().await;
        for _ in 0..2 {
            let (status, headers, body) = act(&rig, &admin, "/web/admin/people", &fields).await;
            assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
            assert_eq!(headers[header::LOCATION], done);
        }
        let events = rig
            .store
            .events_range(rig.boot.tenant_id, head + 1, 500)
            .await
            .unwrap();
        let acted = events
            .iter()
            .filter(|e| e.envelope.event_type.starts_with("web.email"))
            .count();
        assert_eq!(acted, 1, "{done}: the second send replayed");
    }
}

#[tokio::test]
async fn disabling_a_person_is_confirmed_first_and_nothing_acts_until_it_is() {
    let rig = rig().await;
    let _alice = signed_in(&rig, "sub-1", EMAIL).await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for action in ["disable_user", "close_deletion"] {
        let head = rig.head().await;
        let (status, _, page) = act(
            &rig,
            &admin,
            "/web/admin/users",
            &[("action", action), ("user", "alice")],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{action}: {page}");
        assert!(page.contains(r#"name="confirm" value="yes""#), "{page}");
        assert_eq!(
            rig.head().await,
            head,
            "{action}: the confirmation records nothing"
        );
    }
    // A person who is not an admin reaches neither act.
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/admin/users",
        &[
            ("action", "disable_user"),
            ("user", "alice"),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, headers, body) = act(
        &rig,
        &admin,
        "/web/admin/users",
        &[
            ("action", "disable_user"),
            ("user", "alice"),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(headers[header::LOCATION], "/web/admin/users?done=disabled");
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(!page.contains("<h1>Keys</h1>"), "her session ended: {page}");
}

#[tokio::test]
async fn the_start_page_shows_a_credential_once_and_carries_its_one_script() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, headers, page) = send(
        &rig.router,
        get("/web/start?client=claude-code", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert_eq!(csp, oml_web::start::start_csp());
    assert!(
        page.contains("<script>") && page.contains("Issue credential"),
        "{page}"
    );
    let csrf = csrf_of(&page);
    let nonce = field_of(&page, "nonce");
    let fields = [
        ("action", "issue_claude_code"),
        ("csrf_token", csrf.as_str()),
        ("nonce", nonce.as_str()),
    ];
    let (status, headers, _) = send(&rig.router, post("/web/start", &fields, Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert_eq!(location, "/web/start?client=claude-code");
    assert!(!location.contains("oml_"), "never the value in a URL");
    let (once, _) = set_cookie(&headers, ONCE_COOKIE).expect("the sealed value");
    let both = format!("{cookie}; {ONCE_COOKIE}={once}");
    let (_, headers, shown) = send(&rig.router, get(&location, Some(&both))).await;
    assert!(
        shown.contains("alice-claude-code") || shown.contains("cred-key"),
        "{shown}"
    );
    assert!(shown.contains("Shown once"), "{shown}");
    assert!(
        set_cookie(&headers, ONCE_COOKIE).is_some_and(|(v, _)| v.is_empty()),
        "cleared"
    );
    // The same form again replays: no second key, no cookie over the first.
    let (status, headers, _) = send(&rig.router, post("/web/start", &fields, Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(set_cookie(&headers, ONCE_COOKIE).is_none());
    key_named(&rig, "alice-claude-code").await;
    // A second issue takes the next free name.
    let (_, _, page) = send(
        &rig.router,
        get("/web/start?client=claude-code", Some(&cookie)),
    )
    .await;
    let again = [
        ("action", "issue_claude_code"),
        ("csrf_token", csrf.as_str()),
        ("nonce", &field_of(&page, "nonce")),
    ];
    let (status, _, _) = send(&rig.router, post("/web/start", &again, Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    key_named(&rig, "alice-claude-code-2").await;
}

#[tokio::test]
async fn resubmitting_an_issue_that_landed_on_the_next_free_name_issues_nothing_new() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let csrf = csrf_for_page(&rig, &cookie).await;
    // The first name is taken, so this form's issue lands on -2.
    for nonce in [
        "11111111111111111111111111111111",
        "22222222222222222222222222222222",
    ] {
        let fields = [
            ("action", "issue_claude_code"),
            ("csrf_token", csrf.as_str()),
            ("nonce", nonce),
        ];
        let (status, _, _) = send(&rig.router, post("/web/start", &fields, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER);
    }
    key_named(&rig, "alice-claude-code-2").await;
    let head = rig.head().await;
    let again = [
        ("action", "issue_claude_code"),
        ("csrf_token", csrf.as_str()),
        ("nonce", "22222222222222222222222222222222"),
    ];
    let (status, headers, _) = send(&rig.router, post("/web/start", &again, Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        set_cookie(&headers, ONCE_COOKIE).is_none(),
        "a replay shows nothing"
    );
    assert_eq!(rig.head().await, head, "no key issued, nothing journalled");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_issue_whose_name_a_stopped_attempt_holds_issues_no_next_name() {
    // F2 of #255's review, closed by the key operation's claim-first record
    // (#233, #259): an attempt that committed `-2` and stopped before its
    // outcome was recorded still holds its claim, so the resubmission meets
    // `idempotency.in_progress` on `-2` and never issues `-3`.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let csrf = csrf_for_page(&rig, &cookie).await;
    let first = [
        ("action", "issue_claude_code"),
        ("csrf_token", csrf.as_str()),
        ("nonce", "33333333333333333333333333333333"),
    ];
    let (status, _, _) = send(&rig.router, post("/web/start", &first, Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    // The stopped attempt: the claim on `-2`'s key, under this form's
    // nonce, with the request it made.
    let nonce = "44444444444444444444444444444444";
    let token = cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .map_or("", |(_, v)| v);
    let form_key = oml_web_credential::act_idempotency_key(token, nonce);
    let attempt: String =
        ring::digest::digest(&ring::digest::SHA256, format!("{form_key}\n2").as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
    let body = json!({"name": "alice-claude-code-2", "role": "contributor", "connect_code": false});
    let mut bytes = b"\n".to_vec();
    bytes.extend_from_slice(body.to_string().as_bytes());
    let web_key = key_named(&rig, "alice-web").await;
    let scope = oml_app::ports::IdempotencyScope {
        tenant_id: rig.boot.tenant_id,
        key_id: web_key.key_id,
        workspace_id: uuid::Uuid::nil(),
        idem_key: format!("keys.issue:{attempt}"),
    };
    rig.store
        .idempotency_claim(
            &scope,
            &oml_app::domain::Sha256Hash::digest(&bytes).to_prefixed(),
            uuid::Uuid::from_u128(0xF2),
        )
        .await
        .expect("a claim");
    let again = [
        ("action", "issue_claude_code"),
        ("csrf_token", csrf.as_str()),
        ("nonce", nonce),
    ];
    let (status, _, page) = send(&rig.router, post("/web/start", &again, Some(&cookie))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{page}");
    assert!(page.contains("idempotency.in_progress"), "{page}");
    for name in ["alice-claude-code-2", "alice-claude-code-3"] {
        assert!(
            rig.store
                .resolve_name(
                    rig.boot.tenant_id,
                    oml_app::domain::name::NamedKind::Key,
                    &Name::parse(name).unwrap()
                )
                .await
                .unwrap()
                .is_none(),
            "{name} was not issued"
        );
    }
}

async fn csrf_for_page(rig: &Rig, cookie: &str) -> String {
    let (_, _, page) = send(
        &rig.router,
        get("/web/start?client=claude-code", Some(cookie)),
    )
    .await;
    csrf_of(&page)
}

#[tokio::test]
async fn the_start_routes_are_never_stored_and_the_export_holds_only_the_persons_own() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", false).await;
    let alice = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &alice).await;
    agent_writes(&rig, &value, "alice's private kestrel").await;
    let bob = signed_in(&rig, "sub-2", "bob@example.com").await;
    for path in [
        "/web/start",
        "/web/start/status",
        "/web/start/export",
        "/web/setup",
    ] {
        let (status, headers, _) = send(&rig.router, get(path, Some(&bob))).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{path}");
    }
    let (_, _, export) = send(&rig.router, get("/web/start/export", Some(&bob))).await;
    assert!(
        !export.contains("kestrel"),
        "another person's memory: {export}"
    );
    let (_, _, export) = send(&rig.router, get("/web/start/export", Some(&alice))).await;
    assert!(export.contains("kestrel"), "{export}");
}

#[tokio::test]
async fn a_deletion_request_needs_the_word_delete_and_is_shown_as_pending() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let head = rig.head().await;
    let (status, _, body) = act(
        &rig,
        &cookie,
        "/web/start",
        &[("action", "request_deletion"), ("confirm", "yes")],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(rig.head().await, head, "nothing recorded");
    let (status, headers, body) = act(
        &rig,
        &cookie,
        "/web/start",
        &[("action", "request_deletion"), ("confirm", "delete")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(headers[header::LOCATION], "/web/start?done=requested");
    let (_, _, page) = send(&rig.router, get("/web/start?done=requested", Some(&cookie))).await;
    assert!(
        page.contains("The deletion request is with the administrator"),
        "{page}"
    );
    assert!(!page.contains("Type delete to confirm"), "{page}");
}

#[tokio::test]
async fn the_start_status_and_export_are_data_and_answer_nothing_without_a_session() {
    let rig = rig().await;
    for path in ["/web/start/status", "/web/start/export"] {
        let (status, headers, body) = send(&rig.router, get(path, None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        assert!(
            headers.get(header::LOCATION).is_none(),
            "{path}: no redirect"
        );
    }
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    agent_writes(&rig, &value, "an exported fact").await;
    let (status, _, body) = send(&rig.router, get("/web/start/status", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let status_json: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(status_json["first_memory"], "held", "{body}");
    let (status, headers, body) = send(&rig.router, get("/web/start/export", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment"),
    );
    assert!(body.contains("an exported fact"), "{body}");
    let (status, _, _) = send(
        &rig.router,
        Request::get("/web/start/status")
            .header(header::AUTHORIZATION, format!("Bearer {value}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a bearer key reaches no page"
    );
}

#[tokio::test]
async fn the_old_setup_and_admin_urls_are_not_found_and_the_pages_answer_at_their_own_paths() {
    // The lead's 303s from the old URLs are deleted in v0.7.1 (ADR-0070,
    // "The old URLs deleted"): an old URL is not found, an old act is
    // refused and runs nothing, and the moved page answers at its own path.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (old, who, new) in [
        ("/web?view=setup", cookie.as_str(), "/web/setup"),
        ("/web/admin", boss.as_str(), "/web/admin/people"),
        ("/web/admin?view=people", boss.as_str(), "/web/admin/people"),
    ] {
        let (status, headers, _) = send(&rig.router, get(old, Some(who))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
        let (status, _, page) = send(&rig.router, get(new, Some(who))).await;
        assert_eq!(status, StatusCode::OK, "{new}: {page}");
    }
    for path in ["/web/start", "/web/admin/users"] {
        let (status, _, page) = send(&rig.router, get(path, Some(&boss))).await;
        assert_eq!(status, StatusCode::OK, "{path}: {page}");
    }
}

// -- the Review and Keys views (ADR-0045 d9, d11, d13; ADR-0055 d10), moved to oml-web

#[tokio::test]
async fn a_field_the_action_does_not_define_is_refused() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    let csrf = csrf_of(&page);
    let (status, _, body) = send(
        &rig.router,
        post(
            "/web",
            &[
                ("action", "sign_out"),
                ("csrf_token", &csrf),
                ("user", "bob"),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("request.validation"), "{body}");
    let (status, _, body) = send(
        &rig.router,
        post(
            "/web",
            &[("action", "an_unknown_action"), ("csrf_token", &csrf)],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("not one of this page"), "{body}");
    let (status, _, body) = send(
        &rig.router,
        post(
            "/web/keys",
            &[
                ("action", "issue_key"),
                ("csrf_token", &csrf),
                ("name", "laptop"),
                ("user", "bob"),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("takes no field user"), "{body}");
}

#[tokio::test]
async fn a_user_issues_a_key_only_inside_its_root() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let value = value_of(&page);
    assert!(value.starts_with("oml_"), "shown once: {value}");
    let laptop = key_named(&rig, "alice-laptop").await;
    let root = key_named(&rig, "alice-web").await;
    assert_eq!(laptop.user, root.user, "a key naming the person's user");
    assert_eq!(laptop.issued_by, Some(root.key_id), "issued by alice-web");
    // Its every grant lies in the root, and the universe node read.
    let access = rig
        .store
        .key_access(rig.boot.tenant_id, laptop.key_id)
        .await
        .unwrap()
        .unwrap();
    for g in &access.grants {
        assert!(
            g.grant.node == root.user.unwrap() || g.grant.role == Role::Reader,
            "{g:?}"
        );
    }
    for (name, role) in [("review", "contributor"), ("phone", "reviewer")] {
        let (status, _, body) = act_and_follow(
            &rig,
            &cookie,
            "/web/keys",
            &[("action", "issue_key"), ("name", name), ("role", role)],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
    }
    // The keys page lists it; a cut-off denies it on universe.
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "cut_off"),
            ("key", "alice-laptop"),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(page.contains("cut off"), "{page}");
    // Another user's key is not the person's to cut off.
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "cut_off"),
            ("key", "root-admin"),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_connector_key_shows_its_connect_code_once() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "connect_key"),
            ("name", "phone"),
            ("role", "contributor"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("https://oml.example.com/mcp"), "{page}");
    let phone = key_named(&rig, "alice-phone").await;
    assert_eq!(phone.credential.as_str(), "connection");
    let (status, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[("action", "connect_code"), ("key", "alice-phone")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Connect code"), "{page}");
    // Its expiry reads as the minutes left and the clock time in UTC.
    assert!(page.contains(" UTC)."), "{page}");
}

#[tokio::test]
async fn the_user_page_opens_its_agents_held_rows_and_a_person_approves_their_own_agents_write() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    let value = value_of(&page);
    let memory = agent_writes(&rig, &value, "the deploy window is tuesday").await;
    let (_, _, page) = send(
        &rig.router,
        get("/web/memories?view=submitted", Some(&cookie)),
    )
    .await;
    assert!(page.contains("the deploy window is tuesday"), "{page}");
    assert!(page.contains(">held<"), "{page}");
    let (status, _, opened) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert!(opened.contains("the deploy window is tuesday"), "{opened}");
    rig.allow("bob@example.com", "bob", false).await;
    let bob = signed_in(&rig, "sub-bob", "bob@example.com").await;
    let (status, _, hidden) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&bob)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");
    assert!(!hidden.contains("the deploy window is tuesday"), "{hidden}");
    // Judge on the user page acts as alice-review, which is not the author.
    let (status, _, page) = act_and_follow(&rig, &cookie, "/web/todo", &[("action", "next")]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("<h1>Judge</h1>"), "{page}");
    assert!(page.contains("the deploy window is tuesday"), "{page}");
    let target = memory.to_string();
    let nonce = field_of(&page, "nonce");
    let (status, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/todo/judge",
        &[
            ("action", "judge"),
            ("artifact_id", &target),
            ("verdict", "approve"),
            ("nonce", &nonce),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("is now approved"), "{page}");
    let promoted = rig
        .store
        .memory(rig.boot.tenant_id, memory)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(promoted.status.as_str(), "promoted");
    // The card says when it was approved and by what kind of reviewer, and
    // that a person's review of their own agent's write is a self-review.
    let (_, _, page) = send(&rig.router, get("/web/memories", Some(&cookie))).await;
    assert!(
        page.contains("<span>approved by a person's key on 20")
            && page.contains("self-review: the same person"),
        "{page}"
    );
    assert!(!page.contains("outside its lineage"), "{page}");
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, 1, 500)
        .await
        .unwrap();
    let verdict = events
        .iter()
        .find(|e| e.envelope.event_type == "review.approve")
        .expect("the verdict");
    assert_eq!(
        verdict.envelope.api_key_id,
        Some(key_named(&rig, "alice-review").await.key_id)
    );
}

#[tokio::test]
async fn removing_the_last_email_leaves_the_agents_keys_working() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    let value = value_of(&page);
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    // The confirmation lists alice's agent keys with a cut-off beside each.
    let (status, _, page) = act(
        &rig,
        &admin,
        "/web/admin/people",
        &[("action", "remove_email"), ("email", EMAIL)],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("alice-laptop") && page.contains("Cut off"),
        "{page}"
    );
    assert!(!page.contains("alice-web"), "session keys are not listed");
    let (status, _, _) = act(
        &rig,
        &admin,
        "/web/admin/people",
        &[
            ("action", "remove_email"),
            ("email", EMAIL),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send(&rig.router, get("/web", Some(&cookie))).await;
    assert!(page.contains("Sign in with Google"), "her session ended");
    rig.app
        .whoami(&value)
        .await
        .expect("the agent's key still works");
}

#[tokio::test]
async fn review_acts_as_the_persons_reviewer_key_and_the_admin_key_is_never_offered_a_verdict() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    let value = value_of(&page);
    agent_writes(&rig, &value, "boss reviews this").await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, _) = act(
        &rig,
        &admin,
        "/web/admin/people",
        &[
            ("action", "grant_review"),
            ("user", "boss"),
            ("workspace", "alice"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    // Next on boss's Personal To do opens the item to judge as boss-review.
    let (status, _, page) = act_and_follow(&rig, &admin, "/web/todo", &[("action", "next")]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("boss reviews this"), "{page}");
    assert!(
        page.contains("Recorded under <code>boss-review</code>"),
        "{page}"
    );
    assert!(!page.contains("<code>boss-admin</code>"), "{page}");
    let refused = rig
        .app
        .review_next(rig.boot.admin_key.value.as_str())
        .await
        .expect_err("the admin key never reviews");
    assert!(
        format!("{refused:?}").contains("VerbCeiling"),
        "{refused:?}"
    );
    let boss_admin = key_named(&rig, "boss-admin").await;
    assert_eq!(boss_admin.role, Role::Admin);
}

#[tokio::test]
async fn a_curated_claim_is_held_until_a_key_outside_its_lineage_approves_and_the_admin_read_shows_its_source()
 {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    let value = value_of(&page);
    let source = agent_writes(&rig, &value, "rotate keys every ninety days").await;
    // Only an approved memory is readable beyond its author: alice approves
    // her agent's write first.
    let (status, _, _) = judge(&rig, &cookie, &source.to_string(), "approve", None).await;
    assert_eq!(status, StatusCode::OK);
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (action, workspace) in [("grant_curation", "alice"), ("grant_review", "universe")] {
        let (status, _, body) = act(
            &rig,
            &admin,
            "/web/admin/people",
            &[
                ("action", action),
                ("user", "boss"),
                ("workspace", workspace),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}: {body}");
    }
    // A curator designation takes effect at the next sign-in: the session's
    // capacity credentials are those its entry reached when it opened
    // (ADR-0070 d2 as built).
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    // Another person judges boss's curated claim: outside boss's own
    // root no key of boss counts, boss-review included (ADR-0082 d7).
    let carol = reviewer_on_universe(&rig, &admin).await;
    let (status, _, body) = act(
        &rig,
        &admin,
        "/web/admin/curate",
        &[
            ("action", "curate"),
            ("source", &source.to_string()),
            ("text", "Keys rotate quarterly."),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let curate = key_named(&rig, "boss-curate").await;
    let page = rig
        .app
        .admin_search(
            rig.boot.admin_key.value.as_str(),
            Some("quarterly"),
            None,
            None,
            None,
            10,
            Some(boot_investigation(&rig).await),
        )
        .await
        .expect("the admin reads");
    let curated = &page.items[0];
    assert_eq!(curated.state.as_str(), "held", "held until reviewed");
    assert_eq!(curated.workspace_path, "universe");
    assert_eq!(curated.memory.author_key_id, curate.key_id);
    // boss-review acts for boss, whose key wrote the claim outside boss's
    // root, so it is served nothing of it (ADR-0082 d7).
    let (status, _, page) = act_and_follow(&rig, &admin, "/web/todo", &[("action", "next")]).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Nothing waits for this account."), "{page}");
    assert!(!page.contains("Keys rotate quarterly."), "{page}");
    // carol-review is outside the curated claim's lineage and its person, so Next opens it.
    let (_, _, page) = act_and_follow(&rig, &carol, "/web/todo", &[("action", "next")]).await;
    assert!(page.contains("<h1>Judge</h1>"), "{page}");
    assert!(page.contains("Keys rotate quarterly."), "{page}");
    let (status, _, page) = judge(
        &rig,
        &carol,
        &curated.memory.memory_id.to_string(),
        "approve",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    // The admin read shows its source and author; alice sees it anonymous.
    let read = rig
        .app
        .admin_read(
            rig.boot.admin_key.value.as_str(),
            curated.memory.memory_id,
            Some(boot_investigation(&rig).await),
        )
        .await
        .expect("the admin reads one");
    assert_eq!(read.memory.derives_from, vec![source]);
    assert_eq!(read.verdicts.len(), 1);
    let (_, _, mine) = send(&rig.router, get("/web/memories", Some(&cookie))).await;
    assert!(
        mine.contains("Keys rotate quarterly.") && mine.contains("the shared area"),
        "{mine}"
    );
}

#[tokio::test]
async fn an_anonymous_universe_row_shows_no_scan_finding_to_a_reader_outside_its_authors() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    let value = value_of(&page);
    let source = agent_writes(&rig, &value, "rotate keys every ninety days").await;
    // Only an approved memory is readable beyond its author: alice approves
    // her agent's write first.
    let (status, _, _) = judge(&rig, &cookie, &source.to_string(), "approve", None).await;
    assert_eq!(status, StatusCode::OK);
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (action, workspace) in [("grant_curation", "alice"), ("grant_review", "universe")] {
        let (status, _, body) = act(
            &rig,
            &admin,
            "/web/admin/people",
            &[
                ("action", action),
                ("user", "boss"),
                ("workspace", workspace),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}: {body}");
    }
    // A curator designation takes effect at the next sign-in: the session's
    // capacity credentials are those its entry reached when it opened
    // (ADR-0070 d2 as built).
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    // Another person judges boss's curated claim: outside boss's own
    // root no key of boss counts, boss-review included (ADR-0082 d7).
    let carol = reviewer_on_universe(&rig, &admin).await;
    let (status, _, body) = act(
        &rig,
        &admin,
        "/web/admin/curate",
        &[
            ("action", "curate"),
            ("source", &source.to_string()),
            ("text", "Keys rotate quarterly. [needs-person]"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let curate = key_named(&rig, "boss-curate").await;
    let page = rig
        .app
        .admin_search(
            rig.boot.admin_key.value.as_str(),
            Some("quarterly"),
            None,
            None,
            None,
            10,
            Some(boot_investigation(&rig).await),
        )
        .await
        .expect("the admin reads");
    let curated = &page.items[0];
    assert_eq!(curated.state.as_str(), "held", "held until reviewed");
    assert_eq!(curated.workspace_path, "universe");
    assert_eq!(curated.memory.author_key_id, curate.key_id);
    // carol-review is outside the curated claim's lineage and its person, so it is served.
    let (_, _, page) = act_and_follow(&rig, &carol, "/web/todo", &[("action", "next")]).await;
    assert!(page.contains("Keys rotate quarterly."), "{page}");
    let (status, _, page) = judge(
        &rig,
        &carol,
        &curated.memory.memory_id.to_string(),
        "approve",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    // The admin reads the finding; alice, outside the authoring keys,
    // reads the row anonymous and without it (ADR-0044 d9; #253).
    let page = rig
        .app
        .admin_search(
            rig.boot.admin_key.value.as_str(),
            Some("quarterly"),
            None,
            None,
            None,
            10,
            Some(boot_investigation(&rig).await),
        )
        .await
        .expect("the admin reads");
    assert!(
        page.items[0].memory.scan_finding.is_some(),
        "the curated text was held with a finding"
    );
    let (_, _, mine) = send(&rig.router, get("/web/memories", Some(&cookie))).await;
    assert!(
        mine.contains("Keys rotate quarterly.") && mine.contains("the shared area"),
        "{mine}"
    );
    assert!(!mine.contains("marker finding"), "{mine}");
}

#[tokio::test]
async fn a_key_issued_on_the_user_page_takes_the_default_guardrail_scope_and_the_page_offers_none()
{
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(!page.contains("guardrail_scope"), "no scope field: {page}");
    laptop_key(&rig, &cookie).await;
    let laptop = key_named(&rig, "alice-laptop").await;
    assert_eq!(
        laptop.guardrail_scope,
        oml_app::domain::guardrail::GuardrailScope::Ancestors
    );
    let (status, _, body) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "narrow"),
            ("role", "contributor"),
            ("guardrail_scope", "workspace"),
        ],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a field the action does not define: {body}"
    );
}

#[tokio::test]
async fn the_judge_page_shows_a_held_guardrails_load_warning() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    agent_guardrail(&rig, &value, "Run the tests before every commit.").await;
    let (status, _, page) = act_and_follow(&rig, &cookie, "/web/todo", &[("action", "next")]).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("Run the tests before every commit."),
        "{page}"
    );
    assert!(
        page.contains("every key that reads this workspace will load this rule"),
        "{page}"
    );
}

/// A reviewer sees what a verdict makes true: the held version's own valid
/// time and, on a supersession, the end approving it gives the version it
/// replaces, on the Judge page's card (ADR-0059 d2, d4; ADR-0083 d2).
#[tokio::test]
async fn a_reviewer_sees_the_valid_time_and_the_end_an_approval_gives_on_the_judge_card() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let v1 = rig
        .app
        .submit_memory(
            value.as_str(),
            &oml_app::types::SubmitRequest {
                origin: None,
                anchor: None,
                idempotency_key: "k-api-v3".to_owned(),
                claim_key: Some("api-version".to_owned()),
                verb: Some(oml_app::types::SubmitVerb::Remember),
                kind: oml_app::domain::MemoryKind::Memory,
                body: "the api is at version 3".to_owned(),
                workspace: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: Some("2025".to_owned()),
                valid_until: None,
                format: None,
                title: None,
                on_similar: Default::default(),
            },
        )
        .await
        .expect("the agent writes")
        .receipt
        .memory_id;
    let (_, _, page) = act_and_follow(&rig, &cookie, "/web/todo", &[("action", "next")]).await;
    assert!(
        page.contains(r#"<dt>Valid</dt><dd>from <time datetime="2025">2025</time>, open</dd>"#),
        "{page}"
    );
    judge(&rig, &cookie, &v1.to_string(), "approve", None).await;
    rig.app
        .supersede(
            value.as_str(),
            &oml_app::types::SupersedeRequest {
                origin: None,
                idempotency_key: "k-api-v4".to_owned(),
                target_id: v1,
                body: "the api is at version 4".to_owned(),
                kind: None,
                retirement: false,
                derives_from: Vec::new(),
                reason: None,
                workspace: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: Some("2026-06-01".to_owned()),
                valid_until: None,
                on_similar: Default::default(),
                anchor: None,
                expected_version: 1,
                format: None,
                title: None,
            },
        )
        .await
        .expect("the agent supersedes");
    // The To do row carries no card; the Judge page Next opens draws it.
    let (_, _, page) = act_and_follow(&rig, &cookie, "/web/todo", &[("action", "next")]).await;
    assert!(page.contains("the api is at version 4"), "{page}");
    assert!(
        page.contains(
            r#"<dt>Valid</dt><dd>from <time datetime="2026-06-01">2026-06-01</time>, open"#
        ),
        "{page}"
    );
    assert!(
        page.contains(r#"approving ends version 1 at <time datetime="2026-06-01T00:00:00.000000Z">2026-06-01 00:00:00 UTC</time>"#),
        "{page}"
    );
}

/// The Judge page's card names every finding stored on the version, one
/// line each with its verdict, its class and rule and where it fired
/// (ADR-0074 d17, d18 phase 1, d23; ADR-0083 d2). The double holds on its
/// marker in the body and in the title alike, so the version carries two
/// findings where the single stored `scan_finding` named one.
#[tokio::test]
async fn the_judge_card_names_every_scan_finding() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    rig.app
        .submit_memory(
            value.as_str(),
            &oml_app::types::SubmitRequest {
                origin: None,
                anchor: None,
                idempotency_key: "k-flagged".to_owned(),
                claim_key: Some("flagged".to_owned()),
                verb: Some(oml_app::types::SubmitVerb::Remember),
                kind: oml_app::domain::MemoryKind::Memory,
                body: "the deploy note [needs-person] in the body".to_owned(),
                workspace: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                format: None,
                title: Some("[needs-person] in the title".to_owned()),
                on_similar: Default::default(),
            },
        )
        .await
        .expect("the agent writes");
    let (status, _, page) = act_and_follow(&rig, &cookie, "/web/todo", &[("action", "next")]).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("scan findings · 2"), "{page}");
    let block = page
        .split(r#"<div class="findings">"#)
        .nth(1)
        .and_then(|rest| rest.split("</div>").next())
        .expect("a findings block");
    assert_eq!(
        block
            .matches(r#"<span class="tag oml-chip is-held">hold</span>"#)
            .count(),
        2,
        "{block}"
    );
    for field in [">/body<", ">/title<"] {
        assert!(block.contains(field), "{field}: {block}");
    }
}

#[tokio::test]
async fn the_judge_card_shows_the_source_the_format_and_what_it_will_be_found_by() {
    let rig = rig().await;
    let (cookie, memory) = person_with_a_diagram(&rig).await;
    let (_, _, page) = act_and_follow(&rig, &cookie, "/web/todo", &[("action", "next")]).await;
    assert!(page.contains("flowchart LR"), "the complete source: {page}");
    assert!(page.contains("text/vnd.oml.mermaid"), "{page}");
    assert!(page.contains("The payment flow"), "the title: {page}");
    assert!(
        page.contains("Checkout (A) calls Payments (B)."),
        "what it will be found by: {page}"
    );
    // The drawing beside it, in the sandboxed frame oml-web now mints
    // (ADR-0056 d9), as the lead's card drew it; the recorded exception of
    // the fifth change is closed (#189).
    assert!(
        page.contains("<iframe sandbox=\"allow-scripts\" src=\"/web/render/"),
        "{page}"
    );
    assert!(!page.contains(&format!("id={memory}")), "{page}");
}

// -- the Review and Keys views: what the move adds (ADR-0070 d3 layer 4) ----

#[tokio::test]
async fn a_key_value_is_shown_once_on_the_redirect_target_and_never_in_a_url_or_a_line() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION],
        "/web/keys",
        "no secret in the URL"
    );
    let (sealed, whole) = set_cookie(&headers, ONCE_COOKIE).expect("the sealed secret");
    for attribute in [
        "Path=/",
        "Secure",
        "HttpOnly",
        "SameSite=Strict",
        "Max-Age=60",
    ] {
        assert!(whole.contains(attribute), "{whole}");
    }
    let shown_with = format!("{cookie}; {ONCE_COOKIE}={sealed}");
    let (status, headers, page) = send(&rig.router, get("/web/keys", Some(&shown_with))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let value = value_of(&page);
    assert!(value.starts_with("oml_"), "{value}");
    assert!(
        !sealed.contains(&value),
        "the cookie holds the value sealed"
    );
    let (_, cleared) = set_cookie(&headers, ONCE_COOKIE).expect("the page clears it");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    // Once the browser drops it, the page shows the keys and no value.
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(page.contains("alice-laptop"), "{page}");
    assert!(!page.contains(&value), "{page}");
    // Another session's browser cannot open the seal.
    rig.allow("bob@example.com", "bob", false).await;
    let bob = signed_in(&rig, "sub-bob", "bob@example.com").await;
    let (_, _, page) = send(
        &rig.router,
        get("/web/keys", Some(&format!("{bob}; {ONCE_COOKIE}={sealed}"))),
    )
    .await;
    assert!(!page.contains(&value), "{page}");
    let lines = rig.lines.lock().expect("unpoisoned").join("\n");
    assert!(!lines.is_empty());
    assert!(!lines.contains(&value), "no line holds the value: {lines}");
    assert!(!lines.contains(&sealed), "no line holds the seal: {lines}");
}

/// Issue key refused by an answer only the server decides, here
/// `name.taken`, goes back to the review step with its notice and code, the
/// draft kept, and Issue key offered again (ADR-0096 d1, D12).
#[tokio::test]
async fn a_name_the_server_finds_taken_goes_back_to_the_review_step_with_the_draft() {
    tokio::time::timeout(Duration::from_secs(120), async {
        let rig = rig().await;
        let cookie = signed_in(&rig, "sub-1", EMAIL).await;
        let draft = "/web/keys/new?kind=agent&name=ci&role=reader&n=1&g0w=alice&g0r=reader&g0c=node&dw=&gs=ancestors&exp=&rpm=60&step=review";
        let mut answers = Vec::new();
        for _ in 0..2 {
            let (_, _, review) = send(&rig.router, get(draft, Some(&cookie))).await;
            let fields = hidden_fields(&review, "build_key");
            let fields: Vec<(&str, &str)> = fields
                .iter()
                .map(|(n, v)| (n.as_str(), v.as_str()))
                .collect();
            answers.push(send(&rig.router, post("/web/keys", &fields, Some(&cookie))).await);
        }
        let (status, headers, body) = &answers[1];
        assert_eq!(*status, StatusCode::SEE_OTHER, "{body}");
        let back = location(headers);
        assert!(back.starts_with("/web/keys/new?"), "{back}");
        assert!(back.contains("step=review"), "{back}");
        assert!(back.contains("refused=name.taken"), "{back}");
        let (status, _, page) = send(&rig.router, get(&back, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.contains("<h1>Review key</h1>"), "{page}");
        assert!(page.contains("<code>name.taken</code>"), "{page}");
        assert!(page.contains(r#"role="alert""#), "{page}");
        assert!(
            page.contains(r#"<input type="hidden" name="name" value="ci">"#),
            "{page}"
        );
        assert!(page.contains(r#"value="build_key""#), "{page}");
    })
    .await
    .expect("the builder answers within two minutes");
}

#[tokio::test]
async fn an_issue_sent_twice_at_once_issues_one_key_and_the_surviving_cookie_opens_to_its_value() {
    // A double click sends the same form twice before either answers: the
    // second waits for the first and replays it without the value, so it
    // sets no cookie over the first's (ADR-0070 d3 layer 4).
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    // The builder's review step draws the one post, Issue key, with its
    // nonce (ADR-0096 d1).
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web/keys/new?kind=agent&name=laptop&role=contributor&n=2&g0w=alice&g0r=submitter&g0c=subtree&g1w=alice&g1r=reader&g1c=subtree&dw=&gs=ancestors&exp=&rpm=&step=review",
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("<h1>Review key</h1>"), "{page}");
    let fields = hidden_fields(&page, "build_key");
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let (a, b) = tokio::join!(
        send(&rig.router, post("/web/keys", &fields, Some(&cookie))),
        send(&rig.router, post("/web/keys", &fields, Some(&cookie))),
    );
    let mut sealed = Vec::new();
    for (status, headers, page) in [a, b] {
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        assert_eq!(headers[header::LOCATION], "/web/keys");
        if let Some((value, _)) = set_cookie(&headers, ONCE_COOKIE) {
            sealed.push(value);
        }
    }
    assert_eq!(sealed.len(), 1, "one answer carries the value: {sealed:?}");
    let (_, _, page) = send(
        &rig.router,
        get(
            "/web/keys",
            Some(&format!("{cookie}; {ONCE_COOKIE}={}", sealed[0])),
        ),
    )
    .await;
    let value = value_of(&page);
    let laptop = key_named(&rig, "alice-laptop").await;
    let presented = rig
        .app
        .caller(oml_app::types::Bearer::Key(&value))
        .await
        .expect("the shown value is a live key");
    assert_eq!(presented.key().key_id, laptop.key_id);
    // A new connect code sent twice at once: one answer carries the code.
    act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "connect_key"),
            ("name", "phone"),
            ("role", "contributor"),
        ],
    )
    .await;
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    let fields = [
        ("action", "connect_code".to_owned()),
        ("csrf_token", csrf_of(&page)),
        ("key", "alice-phone".to_owned()),
        ("nonce", field_of(&page, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let (a, b) = tokio::join!(
        send(&rig.router, post("/web/keys", &fields, Some(&cookie))),
        send(&rig.router, post("/web/keys", &fields, Some(&cookie))),
    );
    let mut sealed = 0;
    for (status, headers, page) in [a, b] {
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        assert_eq!(headers[header::LOCATION], "/web/keys");
        sealed += usize::from(set_cookie(&headers, ONCE_COOKIE).is_some());
    }
    assert_eq!(sealed, 1, "one answer carries the code");
}

#[tokio::test]
async fn a_connect_code_is_shown_once_on_the_redirect_target() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "connect_key"),
            ("name", "phone"),
            ("role", "contributor"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/keys");
    let (sealed, _) = set_cookie(&headers, ONCE_COOKIE).expect("the sealed code");
    let (_, _, page) = send(
        &rig.router,
        get(
            "/web/keys",
            Some(&format!("{cookie}; {ONCE_COOKIE}={sealed}")),
        ),
    )
    .await;
    let at = page.find("Connect code</p><pre>").expect("the code") + "Connect code</p><pre>".len();
    let code = &page[at..at + 14];
    assert!(!sealed.contains(code));
    let lines = rig.lines.lock().expect("unpoisoned").join("\n");
    assert!(!lines.contains(code), "{lines}");
}

#[tokio::test]
async fn the_todo_and_judge_pages_hold_no_bulk_review_and_refuse_its_acts() {
    // Bulk review is the CLI's alone (ADR-0083 d7); the person's To do
    // holds Next and Judge (ADR-0083 d2, d14).
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "Decision one.").await;
    let (status, _, page) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains(r#"name="action" value="next""#), "{page}");
    let (status, _, judge_page) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={memory}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{judge_page}");
    for page in [&page, &judge_page] {
        assert!(!page.contains("Bulk review"), "{page}");
        assert!(!page.contains("pick_000"), "{page}");
        assert!(!page.contains("bulk_"), "{page}");
    }
    let id = memory.to_string();
    for path in ["/web/todo", "/web/todo/judge"] {
        for fields in [
            [
                ("action", "bulk_confirm"),
                ("verdict", "approve"),
                ("pick_000", id.as_str()),
            ],
            [
                ("action", "bulk_verdict"),
                ("verdict", "approve"),
                ("artifact_ids", id.as_str()),
            ],
        ] {
            let (status, _, page) = act(&rig, &cookie, path, &fields).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {page}");
            assert!(page.contains("not one of this page"), "{path}: {page}");
        }
    }
    let held = rig
        .store
        .memory(rig.boot.tenant_id, memory)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.status.as_str(), "held");
}

/// The person's To do lists what waits from the held listing their
/// reviewer key reads (ADR-0083 d14); a write by one of their own agents is
/// listed marked same user; Judge draws the item with its verdict form,
/// and the verdict is recorded as a listed-set verdict of one id, under
/// the person's reviewer key, as a single approve journals it (ADR-0055
/// d4; ADR-0083 d2).
#[tokio::test]
async fn a_person_judges_one_listed_item_from_todo_as_a_one_id_verdict() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "Deploys pause on Fridays.").await;
    let (status, _, page) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains(">To review · 1</h2>"), "{page}");
    assert!(
        page.contains(r#"href="/web/todo">To do · 1</a>"#)
            || page.contains(r#"href="/web/todo" aria-current="page">To do · 1</a>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<span class="oml-decision">New memory</span>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<span class="tag oml-chip is-quiet">same user</span>"#),
        "{page}"
    );
    let judge_link = format!(r#"href="/web/todo/judge?memory={memory}">Judge</a>"#);
    assert!(page.contains(&judge_link), "{page}");
    assert!(
        !page.contains("pick_000"),
        "no bulk pick on the person's page: {page}"
    );
    let (status, _, item) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={memory}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{item}");
    assert!(item.contains("Deploys pause on Fridays."), "{item}");
    assert!(
        item.contains(
            r#"action="/web/todo/judge"><input type="hidden" name="action" value="judge">"#
        ),
        "{item}"
    );
    assert_eq!(field_of(&item, "artifact_id"), memory.to_string());
    assert!(item.contains("Record verdict"), "{item}");
    assert!(
        item.contains("approving is a self-review, marked same user"),
        "{item}"
    );
    let head = rig.head().await;
    let form = [
        ("action", "judge"),
        ("artifact_id", &memory.to_string()),
        ("verdict", "approve"),
        ("reason", ""),
        ("nonce", &field_of(&item, "nonce")),
    ];
    let (status, headers, _) = act(&rig, &cookie, "/web/todo/judge", &form).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        "/web/todo?done=verdict&version=1&state=promoted"
    );
    let promoted = rig
        .store
        .memory(rig.boot.tenant_id, memory)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(promoted.status.as_str(), "promoted");
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    let verdicts: Vec<_> = events
        .iter()
        .filter(|e| e.envelope.event_type == "review.approve")
        .collect();
    assert_eq!(verdicts.len(), 1, "one verdict");
    assert_eq!(
        verdicts[0].envelope.api_key_id,
        Some(key_named(&rig, "alice-review").await.key_id)
    );
    let (_, _, page) = send(
        &rig.router,
        get(
            "/web/todo?done=verdict&version=1&state=promoted",
            Some(&cookie),
        ),
    )
    .await;
    assert!(
        page.contains("Recorded: version 1 is now approved."),
        "{page}"
    );
    assert!(
        page.contains(r#"aria-current="page">To do · 0</a>"#),
        "{page}"
    );
    assert!(page.contains("Nothing waits for this account."), "{page}");
    assert!(!page.contains(&judge_link), "{page}");
}

/// R2 on the person's pages: a version the person's reviewer key wrote is
/// never listed to it on To do, Next never opens it, and a judge naming its
/// id is refused with `review.author_in_lineage` and records nothing
/// (ADR-0055 d5; ADR-0083 d2).
#[tokio::test]
async fn a_version_the_reviewer_key_wrote_is_never_listed_and_judging_it_is_refused() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "The backup runs at two.").await;
    let (status, _, _) = judge(&rig, &cookie, &memory.to_string(), "approve", None).await;
    assert_eq!(status, StatusCode::OK);
    // boss, an admin with review on alice's root, proposes its next
    // version: boss-review writes it.
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    act(
        &rig,
        &boss,
        "/web/admin/people",
        &[
            ("action", "grant_review"),
            ("user", "boss"),
            ("workspace", "alice"),
        ],
    )
    .await;
    let (status, _, page) = act(
        &rig,
        &boss,
        "/web/admin/memory",
        &[
            ("action", "supersede"),
            ("memory", &memory.to_string()),
            ("expected_version", "1"),
            ("text", "The backup runs at three."),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    // alice's reviewer key may judge it; boss's never sees it listed.
    let (_, _, alice_list) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    assert!(
        alice_list
            .contains(r#"<span class="oml-decision">Edit <span class="of">of v1</span></span>"#),
        "{alice_list}"
    );
    let marker = "/web/todo/judge?memory=";
    let at = alice_list.find(marker).expect("a row") + marker.len();
    let successor = alice_list[at..at + 36].to_owned();
    let (_, _, alice_judge) = send(
        &rig.router,
        get(
            &format!("/web/todo/judge?memory={successor}"),
            Some(&cookie),
        ),
    )
    .await;
    assert!(
        alice_judge.contains("The backup runs at three."),
        "{alice_judge}"
    );
    let (_, _, listed) = send(&rig.router, get("/web/todo", Some(&boss))).await;
    assert!(!listed.contains(&successor), "{listed}");
    assert!(!listed.contains("/web/todo/judge?memory="), "{listed}");
    assert_eq!(next_item(&rig, &boss).await, None, "Next opens nothing");
    let (_, _, opened) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={successor}"), Some(&boss)),
    )
    .await;
    assert!(!opened.contains("The backup runs at three."), "{opened}");
    assert!(!opened.contains("Record verdict"), "{opened}");
    let head = rig.head().await;
    let (status, _, refused) = act(
        &rig,
        &boss,
        "/web/todo/judge",
        &[
            ("action", "judge"),
            ("artifact_id", &successor),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert!(refused.contains("review.author_in_lineage"), "{refused}");
    let held = rig
        .store
        .memory(rig.boot.tenant_id, successor.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.status.as_str(), "held", "nothing recorded");
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.envelope.event_type.starts_with("review.")),
        "{:?}",
        events
            .iter()
            .map(|e| &e.envelope.event_type)
            .collect::<Vec<_>>()
    );
}

/// The admin's One memory page draws the same card as the person's, with
/// the scan finding the admin read serves, and says why it offers no
/// verdict (MemoryScreen).
#[tokio::test]
async fn the_admin_memory_page_shows_the_scan_finding_the_persons_page_shows() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "Keys rotate quarterly. [needs-person]").await;
    let (_, _, person) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&cookie)),
    )
    .await;
    assert!(person.contains(r#"<div class="findings">"#), "{person}");
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(
        &rig.router,
        get(&format!("/web/admin/memory?id={memory}"), Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains(r#"<div class="findings">"#), "{page}");
    assert!(page.contains("scan findings · 1"), "{page}");
    assert!(
        page.contains("0 of the 1 approval this workspace's review policy asks for."),
        "{page}"
    );
    assert!(
        page.contains("Read here as <code>boss-admin</code>, which never judges. Judge it in To do as <code>boss-review</code>."),
        "{page}"
    );
}

/// Next opens the oldest held item under the filter, then the item after
/// the one the person leaves unjudged, the oldest again past the last, and
/// records nothing (ADR-0083 d2, d11, d14).
#[tokio::test]
async fn next_opens_the_oldest_held_item_passes_over_the_one_left_and_records_nothing() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let first = agent_writes(&rig, &value, "the deploy window is tuesday").await;
    let second = agent_writes(&rig, &value, "the deploy window is thursday").await;
    let third = agent_writes(&rig, &value, "the deploy window is friday").await;
    let head = rig.head().await;
    assert_eq!(
        next_item(&rig, &cookie).await,
        Some(first.to_string()),
        "the oldest first"
    );
    // Next walks the queue: from the second the third, not the first again,
    // and past the last the oldest (the review of PR #494, 6).
    for (left, opens) in [(second, third), (third, first)] {
        let (status, headers, body) = act(
            &rig,
            &cookie,
            "/web/todo",
            &[("action", "next"), ("after", &left.to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        assert_eq!(
            headers[header::LOCATION].to_str().unwrap(),
            format!("/web/todo/judge?memory={opens}")
        );
    }
    let (status, headers, body) = act(
        &rig,
        &cookie,
        "/web/todo",
        &[("action", "next"), ("after", &first.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        format!("/web/todo/judge?memory={second}")
    );
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert!(
        events.is_empty(),
        "Next records nothing: {:?}",
        events
            .iter()
            .map(|e| &e.envelope.event_type)
            .collect::<Vec<_>>()
    );
    for id in [first, second, third] {
        let row = rig
            .store
            .memory(rig.boot.tenant_id, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status.as_str(), "held");
    }
}

#[tokio::test]
async fn a_resubmitted_judge_form_records_one_verdict_and_lands_on_the_same_notice() {
    // The verdict runs under a key derived from the Judge form's nonce, so
    // a double submit replays the first answer and records it once.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    for body in ["Decision one.", "Decision two.", "Decision three."] {
        agent_writes(&rig, &value, body).await;
    }
    let (_, _, page) = act_and_follow(&rig, &cookie, "/web/todo", &[("action", "next")]).await;
    let fields = [
        ("action", "judge".to_owned()),
        ("csrf_token", csrf_of(&page)),
        ("artifact_id", field_of(&page, "artifact_id")),
        ("verdict", "approve".to_owned()),
        ("nonce", field_of(&page, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let head = rig.head().await;
    let mut locations = Vec::new();
    for _ in 0..2 {
        let (status, headers, page) =
            send(&rig.router, post("/web/todo/judge", &fields, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        locations.push(headers[header::LOCATION].to_str().unwrap().to_owned());
    }
    assert_eq!(
        locations[0], "/web/todo?done=verdict&version=1&state=promoted",
        "{locations:?}"
    );
    assert_eq!(locations[0], locations[1]);
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.envelope.event_type == "review.approve")
            .count(),
        1,
        "one verdict recorded"
    );
}

#[tokio::test]
async fn a_reviewer_who_is_not_the_author_sees_the_held_diagram_drawn_on_the_card() {
    // The exit criterion of #189 named in the review of #249: the Review
    // card draws a held Mermaid claim for a reviewer who is neither its
    // author nor the agent's owner, from the source the API served them.
    let rig = rig().await;
    let (_, memory) = person_with_a_diagram(&rig).await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, _) = act(
        &rig,
        &admin,
        "/web/admin/people",
        &[
            ("action", "grant_review"),
            ("user", "boss"),
            ("workspace", "alice"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, headers, page) =
        act_and_follow(&rig, &admin, "/web/todo", &[("action", "next")]).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("flowchart LR"), "the complete source: {page}");
    assert!(
        page.contains("<iframe sandbox=\"allow-scripts\" src=\"/web/render/"),
        "the drawing: {page}"
    );
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(
        csp.contains(&format!("frame-src {ISSUER}/web/render/;")),
        "{csp}"
    );
    let token = frame_token(&page);
    let (status, _, frame) = send(
        &rig.router,
        get(&format!("/web/render/{token}"), Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{frame}");
    assert!(
        frame.contains("<pre class=\"mermaid\">flowchart LR"),
        "{frame}"
    );
    // Still no link to the held memory's own page, which is not the
    // reviewer's to open.
    assert!(!page.contains(&format!("id={memory}")), "{page}");
    let (status, _, _) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_verdict_the_api_refuses_is_shown_by_its_code_and_records_nothing() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "Not judged by anyone yet.").await;
    let head = rig.head().await;
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/todo/judge",
        &[
            ("action", "judge"),
            (
                "artifact_id",
                &uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef).to_string(),
            ),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
    assert!(page.contains("access.not_found"), "{page}");
    let held = rig
        .store
        .memory(rig.boot.tenant_id, memory)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.status.as_str(), "held");
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.envelope.event_type.starts_with("review.")),
        "nothing recorded"
    );
}

#[tokio::test]
async fn the_admin_todo_and_keys_pages_are_not_found_for_a_person_who_is_not_an_admin() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for path in ["/web/admin/todo", "/web/admin/keys"] {
        let (status, _, page) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {page}");
        assert!(page.contains("access.not_found"), "{page}");
        let (status, _, _) = act(&rig, &cookie, path, &[("action", "cut_off"), ("key", "x")]).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    // The old admin Review moves to the admin Held view, which is not
    // found for this person either (ADR-0083 d6).
    let (status, headers, _) = send(&rig.router, get("/web/admin/review", Some(&cookie))).await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert_eq!(location, "/web/admin/memories?view=held");
    let (status, _, page) = send(&rig.router, get(&location, Some(&cookie))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
}

#[tokio::test]
async fn the_admin_keys_page_lists_a_persons_keys_and_cuts_one_off() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    laptop_key(&rig, &cookie).await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) =
        send(&rig.router, get("/web/admin/keys?user=alice", Some(&admin))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("alice-laptop") && page.contains("Cut off"),
        "{page}"
    );
    assert!(!page.contains("alice-web"), "session keys are not listed");
    assert!(
        page.contains(r#"<input type="hidden" name="user" value="alice">"#),
        "the cut-off carries the listed user: {page}"
    );
    let (status, headers, _) = act(
        &rig,
        &admin,
        "/web/admin/keys",
        &[
            ("action", "cut_off"),
            ("key", "alice-laptop"),
            ("user", "alice"),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION],
        "/web/admin/keys?user=alice&done=cut+off",
        "the act lands back on the same list"
    );
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    let (_, _, page) = send(&rig.router, get(&location, Some(&admin))).await;
    assert!(
        page.contains("alice-laptop") && page.contains("cut off"),
        "{page}"
    );
    // The person's own cut-off names no user.
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "cut_off"),
            ("key", "alice-laptop"),
            ("user", "alice"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{page}");
}

#[tokio::test]
async fn a_cut_off_is_confirmed_first_and_nothing_is_cut_off_until_it_is() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    laptop_key(&rig, &cookie).await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (who, path, listed) in [
        (&cookie, "/web/keys", None),
        (&admin, "/web/admin/keys", Some("alice")),
    ] {
        let head = rig.head().await;
        let mut fields = vec![("action", "cut_off"), ("key", "alice-laptop")];
        if let Some(user) = listed {
            fields.push(("user", user));
        }
        let (status, _, page) = act(&rig, who, path, &fields).await;
        assert_eq!(status, StatusCode::OK, "{path}: {page}");
        assert!(page.contains("Cut off alice-laptop?"), "{page}");
        assert!(page.contains(r#"name="confirm" value="yes""#), "{page}");
        assert!(page.contains(r#"name="nonce""#), "{page}");
        if listed.is_some() {
            assert!(
                page.contains(r#"<input type="hidden" name="user" value="alice">"#),
                "the confirmation carries the listed user: {page}"
            );
        }
        assert_eq!(
            rig.head().await,
            head,
            "{path}: the confirmation records nothing"
        );
        // Anything but the confirmation's own field acts nothing either.
        fields.push(("confirm", "no"));
        let (status, _, _) = act(&rig, who, path, &fields).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(rig.head().await, head, "{path}: confirm=no records nothing");
    }
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(!page.contains("cut off"), "still live: {page}");
}

#[tokio::test]
async fn a_cut_off_confirmation_sent_twice_cuts_the_key_off_once() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    laptop_key(&rig, &cookie).await;
    let head = rig.head().await;
    let fields = [
        ("action", "cut_off"),
        ("key", "alice-laptop"),
        ("confirm", "yes"),
        ("nonce", "0f1e2d3c4b5a69788796a5b4c3d2e1f0"),
    ];
    for _ in 0..2 {
        let (status, headers, body) = act(&rig, &cookie, "/web/keys", &fields).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        assert_eq!(headers[header::LOCATION], "/web/keys?done=cut+off");
    }
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    let denied = events
        .iter()
        .filter(|e| e.envelope.event_type == "key.denied")
        .count();
    assert_eq!(denied, 1, "the second send replayed");
}

/// A call to the API as `oml-web` makes it for the session `cookie` names,
/// sent to the lead through the router: the `web` credential, the web
/// client's identity, a fresh key.
async fn api_as_web(rig: &Rig, cookie: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let token = cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .expect("a session cookie")
        .1;
    let credential = oml_app::domain::web::capacity_credential(token, "web");
    let request = Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header("oml-web-client", WEB_CLIENT_SECRET)
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", format!("k-{path}-{body}"))
        .body(Body::from(body.to_string()))
        .expect("a request");
    let (status, _, answer) = send(&rig.router, request).await;
    (status, serde_json::from_str(&answer).unwrap_or(Value::Null))
}

/// A person whose `<user>-web` key an admin cut off issues no key, mints
/// no connect code and cuts off no key from the Keys page or the Start
/// page: each act answers the API's own refusal, its status, its code and
/// its message, and journals nothing (ADR-0070 d2, d3 layer 7; #325).
#[tokio::test]
async fn a_cut_off_persons_key_acts_are_refused_on_the_page_as_the_api_refuses_them() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    laptop_key(&rig, &cookie).await;
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "connect_key"),
            ("name", "phone"),
            ("role", "contributor"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let name = |name: &str| oml_app::domain::name::Reference::Name(Name::parse(name).unwrap());
    rig.app
        .deny_key(
            &rig.boot.admin_key.value,
            name("alice-web"),
            name("universe"),
        )
        .await
        .expect("the admin cuts alice-web off");
    let head = rig.head().await;
    // The page still opens, so the person can read their keys and sign out.
    let (status, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    type Fields<'a> = &'a [(&'a str, &'a str)];
    let cases: [(&str, Value, Fields<'_>); 4] = [
        (
            "/v2/keys",
            json!({"name": "alice-tablet", "role": "contributor", "connect_code": false}),
            &[
                ("action", "issue_key"),
                ("name", "tablet"),
                ("role", "contributor"),
            ],
        ),
        (
            "/v2/keys",
            json!({"name": "alice-watch", "role": "contributor", "connect_code": true}),
            &[
                ("action", "connect_key"),
                ("name", "watch"),
                ("role", "contributor"),
            ],
        ),
        (
            "/v2/keys/alice-phone/connect-codes",
            json!({}),
            &[("action", "connect_code"), ("key", "alice-phone")],
        ),
        (
            "/v2/keys/alice-laptop/denials",
            json!({"workspace": "universe"}),
            &[
                ("action", "cut_off"),
                ("key", "alice-laptop"),
                ("confirm", "yes"),
            ],
        ),
    ];
    for (api_path, body, fields) in cases {
        let (api_status, refusal) = api_as_web(&rig, &cookie, api_path, body).await;
        assert_eq!(api_status, StatusCode::FORBIDDEN, "{api_path}: {refusal}");
        assert_eq!(refusal["error"]["code"], "access.verb_ceiling", "{refusal}");
        assert_eq!(
            refusal["error"]["details"]["reason"], "session_key_denied",
            "{refusal}"
        );
        let message = refusal["error"]["message"].as_str().expect("a message");
        let (status, _, page) = act(&rig, &cookie, "/web/keys", fields).await;
        assert_eq!(status, api_status, "{fields:?}: {page}");
        assert!(
            page.contains(&format!("<code>access.verb_ceiling</code>: {message}")),
            "{fields:?}: the page names the API's code and message: {page}"
        );
    }
    // The Start page issues through the same operation.
    let (_, _, start) = send(
        &rig.router,
        get("/web/start?client=claude-code", Some(&cookie)),
    )
    .await;
    let (csrf, nonce) = (csrf_of(&start), field_of(&start, "nonce"));
    let (status, _, page) = send(
        &rig.router,
        post(
            "/web/start",
            &[
                ("action", "issue_claude_code"),
                ("csrf_token", csrf.as_str()),
                ("nonce", nonce.as_str()),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{page}");
    assert!(
        page.contains("<code>access.verb_ceiling</code>: "),
        "{page}"
    );
    assert_eq!(rig.head().await, head, "nothing journalled");
    // The denial ends no session and refuses no sign-in.
    let again = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(&rig.router, get("/web/keys", Some(&again))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    for key in ["alice-tablet", "alice-watch"] {
        let named = rig
            .store
            .resolve_name(
                rig.boot.tenant_id,
                oml_app::domain::name::NamedKind::Key,
                &Name::parse(key).expect("a name"),
            )
            .await
            .expect("a lookup");
        assert_eq!(named, None, "no key {key}");
    }
}

/// An app over the rig's store for issuing several keys, with its own id
/// generator started past the others': each app counts from one, so a
/// second key issued by the rig's would repeat the secret of an earlier one.
fn issuer_app(rig: &Rig) -> TestApp {
    let ids = SeqIdGen::new();
    for _ in 0..1_000_000 {
        oml_app::ports::IdGen::new_id(&ids);
    }
    App::new(
        rig.store.clone(),
        MemSigner::new([7; 32]),
        FixedClock::new(),
        ids,
        MarkerScanner::default(),
    )
    .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"))
}

/// A key of alice's with the limits it is issued under, by the tenant admin
/// key: a user key on her root, a connector key where `connect`.
async fn alice_key(
    issuer: &TestApp,
    rig: &Rig,
    name: &str,
    role: Role,
    connect: bool,
    expiry: Option<&str>,
    requests_per_minute: Option<u32>,
) {
    // A contributor writes through a submitter grant (ADR-0093 d2).
    let writes = if role == Role::Contributor {
        Role::Submitter
    } else {
        role
    };
    let grants = [writes, Role::Reader]
        .into_iter()
        .map(|role| GrantRequest {
            node: Name::parse("alice").expect("a name").into(),
            role,
            deny: Vec::new(),
            covers: oml_app::domain::access::Covers::Subtree,
            create: false,
        })
        .collect();
    issuer
        .issue_key(
            &rig.boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role,
                name: Name::parse(name).expect("a name"),
                principal: name.to_owned(),
                used_by: Some(UsedBy::Person),
                default_workspace: None,
                crew: None,
                grants,
                connect_code: connect,
                user: Some(Name::parse("alice").expect("a name")),
                guardrail_scope: None,
                expiry: expiry.map(|at| oml_app::domain::EventTime::parse(at).expect("an instant")),
                requests_per_minute,
                person: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
}

/// The keys `GET /v2/keys` serves to the session `cookie` names, under its
/// `capacity` credential, as the lead answers them.
async fn api_keys(rig: &Rig, cookie: &str, capacity: &str, query: &str) -> Vec<Value> {
    let token = cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .expect("a session cookie")
        .1;
    let credential = oml_app::domain::web::capacity_credential(token, capacity);
    let request = Request::get(format!("/v2/keys{query}"))
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header("oml-web-client", WEB_CLIENT_SECRET)
        .body(Body::empty())
        .expect("a request");
    let (status, _, body) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let listed: Value = serde_json::from_str(&body).expect("a JSON listing");
    listed["items"].as_array().expect("items").clone()
}

/// One key's row of a keys table, from its name cell to the row's end.
fn row_of<'a>(page: &'a str, name: &str) -> &'a str {
    let start = page
        .find(&format!(r#"<tr><td class="mono">{name}</td>"#))
        .unwrap_or_else(|| panic!("no row for {name}: {page}"));
    &page[start..start + page[start..].find("</tr>").expect("a row end")]
}

/// Markup as its text alone: every tag dropped, nothing put in its place.
fn plain(markup: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in markup.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// A mark the page draws in the place it marks `revoked` and `cut off`.
fn marked(row: &str, mark: &str) -> bool {
    row.contains(&format!(r#"oml-chip is-quiet">{mark}</span>"#))
}

/// The page holds each key's row equal to the API's fields for it: its
/// expiry to the second in UTC, with the instant as served in the `<time>`
/// element's `datetime`, an expired mark exactly where `expired` is true,
/// its rate in words, and none of the three for a key that carries none.
fn assert_rows_equal_the_api(page: &str, items: &[Value]) {
    for item in items {
        let name = item["name"].as_str().expect("a name");
        let row = row_of(page, name);
        match item["expiry"].as_str() {
            // To the second and in UTC by name, the instant as served in
            // the element's `datetime`.
            Some(expiry) => {
                let shown = format!(
                    r#"<time datetime="{expiry}">{} {} UTC</time>"#,
                    &expiry[..10],
                    &expiry[11..19]
                );
                assert!(row.contains(&shown), "{name}: {shown}: {row}");
                assert_eq!(row.matches(expiry).count(), 1, "{name}: {row}");
            }
            None => assert!(
                !row.contains("expires") && !row.contains("expired"),
                "{name}: no date: {row}"
            ),
        }
        assert_eq!(
            marked(row, "expired"),
            item["expired"] == true,
            "{name}: the expired mark follows the API's `expired`: {row}"
        );
        match item["requests_per_minute"].as_u64() {
            Some(1) => assert!(row.contains("1 request a minute"), "{name}: {row}"),
            Some(n) => assert!(
                row.contains(&format!("{n} requests a minute")),
                "{name}: {row}"
            ),
            None => assert!(!row.contains("a minute"), "{name}: no rate: {row}"),
        }
    }
}

/// An expired key reads as expired, with its date, and a key with a rate
/// shows it; a key with neither shows neither, and an admin key never
/// carries either. The person's page and the admin's page each hold their
/// rows equal to the API's fields, and an expired connector key is offered
/// no new connect code (ADR-0079 d2, d3, d4; #337).
#[tokio::test]
async fn the_keys_pages_show_a_keys_expiry_expired_mark_and_rate_as_the_api_serves_them() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let issuer = issuer_app(&rig);
    let (soon, late) = ("2026-10-29T12:30:00.000000Z", "2026-10-29T23:00:00.000000Z");
    alice_key(
        &issuer,
        &rig,
        "alice-both",
        Role::Contributor,
        false,
        Some(late),
        Some(60),
    )
    .await;
    alice_key(
        &issuer,
        &rig,
        "alice-one",
        Role::Reader,
        false,
        None,
        Some(1),
    )
    .await;
    alice_key(
        &issuer,
        &rig,
        "alice-lapsed",
        Role::Reader,
        false,
        Some(soon),
        None,
    )
    .await;
    alice_key(
        &issuer,
        &rig,
        "alice-phone",
        Role::Contributor,
        true,
        Some(soon),
        None,
    )
    .await;
    alice_key(
        &issuer,
        &rig,
        "alice-open",
        Role::Contributor,
        true,
        Some(late),
        None,
    )
    .await;
    alice_key(
        &issuer,
        &rig,
        "alice-plain",
        Role::Contributor,
        false,
        None,
        None,
    )
    .await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    // The lead's clock reaches the two 12:30 expiries.
    rig.lead_clock.advance(3_600);
    let mut checked = 0;
    for (path, items) in [
        ("/web/keys", api_keys(&rig, &cookie, "web", "").await),
        (
            "/web/admin/keys?user=alice",
            api_keys(&rig, &admin, "admin", "?user=alice").await,
        ),
    ] {
        let cookie = if path == "/web/keys" { &cookie } else { &admin };
        let (status, _, page) = send(&rig.router, get(path, Some(cookie))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let expired: Vec<&str> = items
            .iter()
            .filter(|i| i["expired"] == true)
            .filter_map(|i| i["name"].as_str())
            .collect();
        assert_eq!(
            expired,
            ["alice-lapsed", "alice-phone"],
            "{path}: the API's own answer"
        );
        assert_rows_equal_the_api(&page, &items);
        checked += items.len();
        let phone = row_of(&page, "alice-phone");
        assert!(!phone.contains("connect_code"), "{path}: {phone}");
        if path == "/web/keys" {
            assert!(
                phone.contains("Expired: no new connect code until an admin sets a later date."),
                "{path}: the page says why: {phone}"
            );
            assert!(row_of(&page, "alice-open").contains("New connect code"));
        }
    }
    assert!(checked >= 14, "both pages checked every key: {checked}");
    // A key that is cut off and expired carries two marks, apart in the
    // markup and read as two: "cut off, expired".
    let name = |name: &str| oml_app::domain::name::Reference::Name(Name::parse(name).unwrap());
    rig.app
        .deny_key(
            &rig.boot.admin_key.value,
            name("alice-lapsed"),
            name("universe"),
        )
        .await
        .expect("the admin cuts alice-lapsed off");
    for (path, cookie) in [
        ("/web/keys", &cookie),
        ("/web/admin/keys?user=alice", &admin),
    ] {
        let (_, _, page) = send(&rig.router, get(path, Some(cookie))).await;
        let lapsed = row_of(&page, "alice-lapsed");
        assert!(
            lapsed.contains(
                r#"cut off</span><span class="sr-only">,</span> <span class="tag oml-chip is-quiet">expired</span>"#
            ),
            "{path}: {lapsed}"
        );
        assert!(
            plain(lapsed).contains("cut off, expired"),
            "{path}: read as two marks: {}",
            plain(lapsed)
        );
    }
    // An admin key carries neither limit: the admin's own `boss-admin`, which
    // the API lists on the admin's page of their keys, is drawn with none.
    let items = api_keys(&rig, &admin, "web", "").await;
    let ops: Vec<&Value> = items.iter().filter(|i| i["role"] == "admin").collect();
    assert!(
        !ops.is_empty(),
        "the admin's own admin key is listed: {items:?}"
    );
    for item in ops {
        assert_eq!(item["expiry"], Value::Null, "{item}");
        assert_eq!(item["requests_per_minute"], Value::Null, "{item}");
    }
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&admin))).await;
    assert_rows_equal_the_api(&page, &items);
}

/// The sentence the Keys and Start pages draw for a person whose session
/// key carries a denial, and the muted line beneath it.
const DENIED_SESSION: &str = "This web session is restricted: it cannot issue keys, get connect codes or cut off keys. An administrator can.";
const DENIED_STILL_WORKS: &str =
    "Exporting memories, requesting deletion and signing out still work.";

/// The forms the Keys page and the Start page draw for the three acts a
/// denied session key is refused.
const KEY_FORMS: [&str; 6] = [
    r#"href="/web/keys/new""#,
    r#"value="connect_code""#,
    r#"value="cut_off""#,
    r#"value="issue_claude_ai""#,
    r#"value="issue_claude_code""#,
    r#"value="issue_other""#,
];

/// A person sets a date on a key they issue from the Keys page: each of the
/// two issue forms has one optional date input, sent as the end of that day
/// in UTC, and the key's row then shows the expiry the API serves. A date
/// that has passed and a value that is not `YYYY-MM-DD` are refused on the page as
/// the API refuses them, by its status, code and message, and issue no key.
/// The Start page's one-button issue has no such input (ADR-0079 d1, the
/// operator's answer 3; #352).
#[tokio::test]
async fn a_person_issues_a_key_with_an_expiry_date_from_the_keys_page() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    // The builder's form has the one optional date input, with the line
    // that says how it is sent (ADR-0096 d10).
    let (status, _, page) = send(&rig.router, get("/web/keys/new?shape=root", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        page.matches(r#"type="date" id="ke" name="exp""#).count(),
        1,
        "{page}"
    );
    assert!(
        page.contains("The key works through this date in every time zone."),
        "{page}"
    );
    let start = start_pages(&rig, &cookie).await;
    assert!(!start.contains(r#"name="expires""#), "{start}");

    // Refused as the API refuses the instant the page sends.
    let head = rig.head().await;
    // A date is sent as the next UTC day's end, so a day before the
    // clock's yesterday is past; a value that is no date is the page's own
    // refusal.
    for action in ["issue_key", "connect_key"] {
        let (status, _, page) = act(
            &rig,
            &cookie,
            "/web/keys",
            &[
                ("action", action),
                ("name", "nodate"),
                ("role", "contributor"),
                ("expires", "tomorrow"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{action}: {page}");
        assert!(page.contains("<code>request.validation</code>"), "{page}");
    }
    for (day, sent, name) in [("2026-10-27", "2026-10-28", "past")] {
        let (api_status, refusal) = api_as_web(
            &rig,
            &cookie,
            "/v2/keys",
            json!({
                "name": format!("alice-{name}"),
                "role": "contributor",
                "connect_code": false,
                "expiry": format!("{sent}T23:59:59Z"),
            }),
        )
        .await;
        assert_eq!(api_status, StatusCode::BAD_REQUEST, "{day}: {refusal}");
        assert_eq!(refusal["error"]["code"], "request.validation", "{refusal}");
        assert_eq!(refusal["error"]["details"]["field"], "/expiry", "{refusal}");
        let message = refusal["error"]["message"].as_str().expect("a message");
        for action in ["issue_key", "connect_key"] {
            let (status, _, page) = act(
                &rig,
                &cookie,
                "/web/keys",
                &[
                    ("action", action),
                    ("name", name),
                    ("role", "contributor"),
                    ("expires", day),
                ],
            )
            .await;
            assert_eq!(status, api_status, "{action} {day}: {page}");
            assert!(
                page.contains(&format!("<code>request.validation</code>: {message}")),
                "{action} {day}: the page names the API's code and message: {page}"
            );
        }
    }
    assert_eq!(rig.head().await, head, "nothing journalled");

    // Issued with a date: the row shows the expiry `GET /v2/keys` serves,
    // the end of that day in UTC; without one, no date.
    for (action, name, day) in [
        ("issue_key", "laptop", "2026-10-29"),
        ("connect_key", "phone", "2999-01-01"),
        ("issue_key", "tablet", ""),
    ] {
        let (status, _, body) = act(
            &rig,
            &cookie,
            "/web/keys",
            &[
                ("action", action),
                ("name", name),
                ("role", "contributor"),
                ("expires", day),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action} {name}: {body}");
    }
    let items = api_keys(&rig, &cookie, "web", "").await;
    let expiry_of = |name: &str| {
        items
            .iter()
            .find(|k| k["name"] == name)
            .unwrap_or_else(|| panic!("no key {name}: {items:?}"))["expiry"]
            .clone()
    };
    assert_eq!(expiry_of("alice-laptop"), "2026-10-30T23:59:59.000000Z");
    assert_eq!(expiry_of("alice-phone"), "2999-01-02T23:59:59.000000Z");
    assert_eq!(expiry_of("alice-tablet"), Value::Null);
    let (status, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_rows_equal_the_api(&page, &items);
    for key in ["alice-past", "alice-nodate"] {
        assert!(
            items.iter().all(|k| k["name"] != key),
            "no key {key}: {items:?}"
        );
    }
}

/// The Start page as it is drawn with no client chosen and for each of its
/// three clients, one after another.
async fn start_pages(rig: &Rig, cookie: &str) -> String {
    let mut pages = String::new();
    for query in [
        "",
        "?client=claude-ai",
        "?client=claude-code",
        "?client=other",
    ] {
        let (status, _, page) = send(
            &rig.router,
            get(&format!("/web/start{query}"), Some(cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{query}: {page}");
        pages.push_str(&page);
    }
    pages
}

/// A person whose `<user>-web` key carries a denial, on `universe`, on their
/// root or on a workspace outside it, is told so on the Keys page and the
/// Start page and is drawn none of the forms for the three acts the API
/// refuses that key for good; a person whose key carries none sees the
/// forms. The page's state is the API's answer: each act posted anyway is
/// the 403 `session_key_denied` the page drew no form for, and the export and
/// the deletion request stay drawn (ADR-0070 d2; #343, #335).
#[tokio::test]
async fn a_denied_persons_keys_and_start_pages_state_the_refusal_and_draw_no_key_forms() {
    for target in ["universe", "alice", "team"] {
        let rig = rig().await;
        let cookie = signed_in(&rig, "sub-1", EMAIL).await;
        laptop_key(&rig, &cookie).await;
        act_and_follow(
            &rig,
            &cookie,
            "/web/keys",
            &[
                ("action", "connect_key"),
                ("name", "phone"),
                ("role", "contributor"),
            ],
        )
        .await;
        issuer_app(&rig)
            .create_workspace(
                &rig.boot.admin_key.value,
                &Name::parse("team").expect("a name"),
                Some(rig.boot.root_workspace_id.into()),
                None,
            )
            .await
            .expect("a workspace outside alice's root");
        let (_, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
        let start = start_pages(&rig, &cookie).await;
        for form in KEY_FORMS {
            assert!(
                keys.contains(form) || start.contains(form),
                "{target}: before a denial the pages draw {form}"
            );
        }
        assert!(!keys.contains(DENIED_SESSION) && !start.contains(DENIED_SESSION));
        assert!(DENIED_SESSION.split_whitespace().count() <= 25);
        let web = row_of(&keys, "alice-web");
        assert!(
            marked(web, "live") && !marked(web, "restricted"),
            "{target}: before a denial the session's own key is live: {web}"
        );
        let name = |name: &str| oml_app::domain::name::Reference::Name(Name::parse(name).unwrap());
        rig.app
            .deny_key(&rig.boot.admin_key.value, name("alice-web"), name(target))
            .await
            .expect("the admin denies alice-web");
        let (status, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{target}: {keys}");
        let start = start_pages(&rig, &cookie).await;
        assert_eq!(
            start.matches(DENIED_SESSION).count(),
            4,
            "{target}: the sentence is on the Start page with no client and with each"
        );
        // On the Keys page the sentence comes first, above the table.
        assert!(
            keys.find(DENIED_SESSION) < keys.find("<table"),
            "{target}: {keys}"
        );
        // The session's own key is the one row marked, in the sentence's
        // word, and never `live`; a denial on `universe` is `cut off` too.
        // No other key's row gains a state.
        let web = row_of(&keys, "alice-web");
        assert!(
            marked(web, "restricted") && !marked(web, "live"),
            "{target}: {web}"
        );
        assert_eq!(
            marked(web, "cut off"),
            target == "universe",
            "{target}: {web}"
        );
        assert_eq!(
            keys.matches(">restricted</span>").count(),
            1,
            "{target}: {keys}"
        );
        for other in ["alice-laptop", "alice-phone", "alice-review"] {
            let row = row_of(&keys, other);
            assert!(marked(row, "live"), "{target}: {other}: {row}");
        }
        for (page, body) in [("keys", &keys), ("start", &start)] {
            assert!(body.contains(DENIED_SESSION), "{target}: {page}: {body}");
            assert!(
                body.contains(DENIED_STILL_WORKS),
                "{target}: {page}: {body}"
            );
            for form in KEY_FORMS {
                assert!(
                    !body.contains(form),
                    "{target}: {page} draws {form}: {body}"
                );
            }
        }
        assert!(
            keys.contains("alice-laptop"),
            "{target}: the keys are still listed"
        );
        // The export and the deletion request stay open to the person.
        assert!(
            start.contains(r#"href="/web/start/export""#),
            "{target}: {start}"
        );
        assert!(
            start.contains(r#"value="request_deletion""#),
            "{target}: {start}"
        );
        // The API refuses each act the page no longer draws a form for.
        for (api_path, body, fields) in [
            (
                "/v2/keys",
                json!({"name": "alice-tablet", "role": "contributor", "connect_code": false}),
                vec![
                    ("action", "issue_key"),
                    ("name", "tablet"),
                    ("role", "contributor"),
                ],
            ),
            (
                "/v2/keys/alice-phone/connect-codes",
                json!({}),
                vec![("action", "connect_code"), ("key", "alice-phone")],
            ),
            (
                "/v2/keys/alice-laptop/denials",
                json!({"workspace": "universe"}),
                vec![
                    ("action", "cut_off"),
                    ("key", "alice-laptop"),
                    ("confirm", "yes"),
                ],
            ),
        ] {
            let (status, refusal) = api_as_web(&rig, &cookie, api_path, body).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{target}: {api_path}: {refusal}"
            );
            assert_eq!(
                refusal["error"]["details"]["reason"], "session_key_denied",
                "{target}: {refusal}"
            );
            let (status, _, page) = act(&rig, &cookie, "/web/keys", &fields).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{target}: {fields:?}: {page}"
            );
        }
    }
}

/// A person whose `<user>-web` key carries a denial, on `universe`, on
/// their root or on a workspace outside it, still downloads their export from
/// the Start page and still files a deletion request from its form: the
/// download is what it was before the denial, and `web.deletion_requested`
/// journals under the denied key (ADR-0070 d2, the operator's decision of
/// 2026-11-12; #335).
#[tokio::test]
async fn a_denied_person_still_exports_and_requests_deletion_from_the_start_page() {
    for target in ["universe", "alice", "team"] {
        let rig = rig().await;
        let cookie = signed_in(&rig, "sub-1", EMAIL).await;
        let value = laptop_key(&rig, &cookie).await;
        agent_writes(&rig, &value, "a kestrel the export keeps").await;
        issuer_app(&rig)
            .create_workspace(
                &rig.boot.admin_key.value,
                &Name::parse("team").expect("a name"),
                Some(rig.boot.root_workspace_id.into()),
                None,
            )
            .await
            .expect("a workspace outside alice's root");
        let exported = |body: &str| {
            let mut export: Value = serde_json::from_str(body).expect("an export");
            export
                .as_object_mut()
                .expect("an object")
                .remove("exported_at");
            export
        };
        let (status, _, body) = send(&rig.router, get("/web/start/export", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{target}: {body}");
        let before = exported(&body);
        let name = |name: &str| oml_app::domain::name::Reference::Name(Name::parse(name).unwrap());
        rig.app
            .deny_key(&rig.boot.admin_key.value, name("alice-web"), name(target))
            .await
            .expect("the admin denies alice-web");
        let (status, headers, body) =
            send(&rig.router, get("/web/start/export", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{target}: {body}");
        assert!(
            headers[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .starts_with("attachment"),
        );
        assert!(
            body.contains("a kestrel the export keeps"),
            "{target}: {body}"
        );
        assert_eq!(
            exported(&body),
            before,
            "{target}: the export is what it was"
        );
        let (status, headers, body) = act(
            &rig,
            &cookie,
            "/web/start",
            &[("action", "request_deletion"), ("confirm", "delete")],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{target}: {body}");
        assert_eq!(headers[header::LOCATION], "/web/start?done=requested");
        let web = key_named(&rig, "alice-web").await.key_id;
        let requested: Vec<_> = rig
            .store
            .events_range(rig.boot.tenant_id, 1, 1000)
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.envelope.event_type == "web.deletion_requested")
            .collect();
        assert_eq!(requested.len(), 1, "{target}: journalled once");
        assert_eq!(requested[0].envelope.api_key_id, Some(web), "{target}");
    }
}

#[tokio::test]
async fn the_old_review_and_keys_urls_move_or_are_not_found_and_their_acts_run_nothing() {
    // The lead's 303s from the old URLs are deleted in v0.7.1 (ADR-0070,
    // "The old URLs deleted"): an old URL is not found, an old act is
    // refused and runs nothing, and the moved page answers at its own path.
    // The Review pages of v0.7 move to To do and the admin Held view with a
    // 308 on GET; a POST to them is no page's act (ADR-0083 d6).
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-boss", "boss@example.com").await;
    // A signed-in GET /web naming no view is the console's landing, the
    // Keys view; it is not an old URL.
    let (status, headers, _) = send(&rig.router, get("/web", Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/keys");
    for (old, new) in [
        ("/web?view=keys", "/web/keys"),
        ("/web?view=review", "/web/todo"),
        ("/web/admin?view=review", "/web/admin/todo"),
        ("/web/admin?view=held&select=all", "/web/admin/todo"),
        ("/web/admin?view=queue", "/web/admin/todo"),
    ] {
        let (status, headers, _) = send(&rig.router, get(old, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
        let (status, _, page) = send(&rig.router, get(new, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{new}: {page}");
    }
    let linked = "0b9e4c1a-0000-4000-8000-0000000000a1";
    for (old, new) in [
        ("/web/review", "/web/todo".to_owned()),
        (
            &format!("/web/review?workspace=alice&as={linked}&serve=srv_1&memory={linked}") as &str,
            format!("/web/todo?workspace=alice&as={linked}"),
        ),
        (
            "/web/admin/review",
            "/web/admin/memories?view=held".to_owned(),
        ),
        (
            "/web/admin/review?workspace=alice&memory=x",
            "/web/admin/memories?view=held&workspace=alice".to_owned(),
        ),
        ("/web/memories?view=review", "/web/todo".to_owned()),
        ("/web/memories?view=fixes", "/web/todo".to_owned()),
    ] {
        let (status, headers, _) = send(&rig.router, get(old, Some(&cookie))).await;
        assert_eq!(status, StatusCode::PERMANENT_REDIRECT, "{old}");
        assert_eq!(headers[header::LOCATION], new.as_str(), "{old}");
    }
    for (page, action, refused) in [
        ("/web", "issue_key", StatusCode::BAD_REQUEST),
        ("/web", "review_next", StatusCode::BAD_REQUEST),
        ("/web/admin", "bulk_verdict", StatusCode::NOT_FOUND),
        ("/web/admin", "cut_off", StatusCode::NOT_FOUND),
        ("/web/review", "review_next", StatusCode::NOT_FOUND),
        ("/web/review", "issue_key", StatusCode::NOT_FOUND),
        ("/web/admin/review", "bulk_verdict", StatusCode::NOT_FOUND),
    ] {
        let (status, headers, _) = act(
            &rig,
            &cookie,
            page,
            &[
                ("action", action),
                ("name", "laptop"),
                ("role", "contributor"),
            ],
        )
        .await;
        assert_eq!(status, refused, "{page} {action}");
        assert!(headers.get(header::LOCATION).is_none(), "{action}");
    }
    assert!(
        rig.store
            .resolve_name(
                rig.boot.tenant_id,
                oml_app::domain::name::NamedKind::Key,
                &Name::parse("boss-laptop").unwrap()
            )
            .await
            .unwrap()
            .is_none(),
        "the lead ran none of the old acts"
    );
    // A verdict posted to the old Review paths records nothing.
    let (alice, value) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let memory = agent_writes(&rig, &value, "the old review path judges nothing").await;
    let head = rig.head().await;
    for path in ["/web/review", "/web/admin/review"] {
        let (status, _, _) = act(
            &rig,
            &alice,
            path,
            &[
                ("action", "judge"),
                ("artifact_id", &memory.to_string()),
                ("verdict", "approve"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        // The proxies route every method of the old paths to `oml-web`,
        // which answers the post not found too, never `405` (the review of
        // PR #494, 12).
        let (_, _, page) = send(&rig.router, get("/web/keys", Some(&alice))).await;
        let csrf = csrf_of(&page);
        let (status, _, _) = send(
            &rig.direct,
            post(
                path,
                &[
                    ("csrf_token", csrf.as_str()),
                    ("action", "judge"),
                    ("artifact_id", &memory.to_string()),
                    ("verdict", "approve"),
                ],
                Some(&alice),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} through the proxy");
    }
    assert_eq!(rig.head().await, head, "nothing journalled");
    let held = rig
        .store
        .memory(rig.boot.tenant_id, memory)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.status.as_str(), "held");
}

#[tokio::test]
async fn the_todo_and_keys_pages_draw_the_header_and_tabs_the_leads_pages_draw() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    let cookie = signed_in(&rig, "sub-2", "bob@example.com").await;
    let (_, _, lead) = send(&rig.router, get("/web/setup", Some(&cookie))).await;
    // The To do badge's own mark of the page it opens is the one difference
    // To do's header has (ADR-0083 d6).
    let unmarked = |page: &str| header_of(page).replace(r#" aria-current="page""#, "");
    for path in ["/web/todo", "/web/keys"] {
        let (status, _, moved) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(unmarked(&moved), unmarked(&lead), "{path}");
        assert!(
            moved.contains(r#"<a class="oml-todo-b" href="/web/todo""#),
            "{moved}"
        );
    }
    let (_, _, todo) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    assert!(
        todo.contains(r#"<a class="oml-todo-b" href="/web/todo" aria-current="page">"#),
        "{todo}"
    );
    let (_, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(
        keys.contains("bob-web") && keys.contains("bob-review"),
        "the person's session keys are listed: {keys}"
    );
    let (_, _, lead) = send(&rig.router, get("/web/admin/people", Some(&cookie))).await;
    for path in ["/web/admin/todo", "/web/admin/keys"] {
        let (status, _, moved) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(unmarked(&moved), unmarked(&lead), "{path}");
        assert!(
            moved.contains(r#"<a class="oml-todo-b" href="/web/admin/todo""#),
            "{moved}"
        );
    }
    let (_, _, keys) = send(&rig.router, get("/web/admin/keys", Some(&cookie))).await;
    assert!(
        keys.contains(r#"<a href="/web/admin/keys" class="on" aria-current="page">"#),
        "{keys}"
    );
    let (_, _, todo) = send(&rig.router, get("/web/admin/todo", Some(&cookie))).await;
    assert!(
        todo.contains(r#"<a class="oml-todo-b" href="/web/admin/todo" aria-current="page">"#),
        "{todo}"
    );
}

// -- the Memories list and the Atlas (ADR-0045 d22-d32), moved to oml-web ----

// ---------------------------------------------------------------------------
// The Memories list, the Atlas view and the workspace dropdowns (ADR-0045
// d22-d29)
// ---------------------------------------------------------------------------

/// The artifact ids of the cards on a Memories page, in page order.
fn card_ids(page: &str) -> Vec<String> {
    page.match_indices("id=\"m-")
        .map(|(at, m)| page[at + m.len()..at + m.len() + 36].to_owned())
        .collect()
}

fn unescape(href: &str) -> String {
    href.replace("&amp;", "&")
}

/// The page's next-page link, unescaped.
fn next_link(page: &str) -> Option<String> {
    let marker = "<a rel=\"next\" href=\"";
    let at = page.find(marker)? + marker.len();
    Some(unescape(&page[at..at + page[at..].find('"')?]))
}

async fn json_of(rig: &Rig, path: &str, cookie: &str) -> (StatusCode, HeaderMap, Value) {
    let (status, headers, body) = send(&rig.router, get(path, Some(cookie))).await;
    let value = serde_json::from_str(&body).unwrap_or_else(|_| panic!("JSON: {body}"));
    (status, headers, value)
}

fn json_ids(value: &Value) -> Vec<String> {
    value["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["artifact_id"].as_str().expect("an id").to_owned())
        .collect()
}

#[tokio::test]
async fn the_web_memories_list_pages_by_keyset_with_no_duplicate_or_gap_under_inserts() {
    let rig = rig().await;
    let (cookie, value) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let mut written = Vec::new();
    for i in 0..7 {
        written.push(agent_writes(&rig, &value, &format!("note number {i}")).await);
    }
    let mut seen: Vec<String> = Vec::new();
    let mut path = "/web/memories?view=submitted&limit=3".to_owned();
    let mut late = Vec::new();
    let mut pages = 0;
    loop {
        let (status, _, page) = send(&rig.router, get(&path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let ids = card_ids(&page);
        assert!(ids.len() <= 3, "a page holds at most limit rows");
        seen.extend(ids);
        pages += 1;
        // Another agent write lands while the person pages.
        if late.len() < 2 {
            late.push(agent_writes(&rig, &value, &format!("late note {}", late.len())).await);
        }
        match next_link(&page) {
            Some(next) => path = next,
            None => break,
        }
    }
    assert_eq!(pages, 3);
    let unique: std::collections::BTreeSet<&String> = seen.iter().collect();
    assert_eq!(unique.len(), seen.len(), "no row twice: {seen:?}");
    let expected: std::collections::BTreeSet<String> =
        written.iter().map(ToString::to_string).collect();
    assert_eq!(
        seen.iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        expected,
        "every row once, none of the late ones pushed in"
    );
    // A fresh first page starts with the late writes, newest first.
    let (_, _, page) = send(
        &rig.router,
        get("/web/memories?view=submitted&limit=3", Some(&cookie)),
    )
    .await;
    let first = card_ids(&page);
    assert_eq!(first[0], late[1].to_string());
    assert_eq!(first[1], late[0].to_string());
    // The page size is bounded.
    for bad in ["0", "201", "many"] {
        let (status, _, page) = send(
            &rig.router,
            get(&format!("/web/memories?limit={bad}"), Some(&cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {page}");
        assert!(page.contains("request.validation"), "{page}");
    }
}

async fn other_tenant_writes(rig: &Rig, body: &str) {
    let boot = rig
        .app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("rival").expect("a name"),
            admin_key_name: Name::parse("rival-admin").expect("a name"),
            admin_principal: "rival".to_owned(),
        })
        .await
        .expect("a second tenant");
    let team = rig
        .app
        .create_workspace(
            &boot.admin_key.value,
            &Name::parse("team").expect("a name"),
            Some(boot.root_workspace_id.into()),
            None,
        )
        .await
        .expect("a workspace")
        .workspace
        .workspace_id;
    let key = rig
        .app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Submitter,
                name: Name::parse("rival-agent").expect("a name"),
                principal: "rival-agent".to_owned(),
                used_by: Some(UsedBy::Agent),
                default_workspace: Some(team.into()),
                crew: None,
                grants: vec![GrantRequest {
                    node: team.into(),
                    role: Role::Submitter,
                    deny: Vec::new(),
                    covers: oml_app::domain::access::Covers::Subtree,
                    create: false,
                }],
                connect_code: false,
                user: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("an agent key");
    agent_writes(rig, &key.value, body).await;
}

#[tokio::test]
async fn the_user_page_never_lists_another_authors_held_row_outside_in_progress_or_another_tenants_row()
 {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", false).await;
    let (alice, alice_agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let (_, bob_agent) = person_with_agent(&rig, "sub-bob", "bob@example.com").await;
    agent_writes(&rig, &alice_agent, "alice keeps this note").await;
    agent_writes(&rig, &bob_agent, "bob held secret").await;
    other_tenant_writes(&rig, "rival tenant secret").await;
    // A tenant reviewer an admin granted into alice's root proposes a
    // successor of her agent's served row; the held successor is another
    // author's.
    let helper = reviewer_in_alices_root(&rig).await;
    let settled = agent_writes(&rig, &alice_agent, "alice settled this").await;
    reviewer_holds_a_successor(&rig, &helper.value, settled, "helper held outside").await;
    for path in [
        "/web/memories?view=submitted",
        "/web/memories?view=submitted&state=held",
    ] {
        let (status, _, page) = send(&rig.router, get(path, Some(&alice))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.contains("alice keeps this note"), "{page}");
        for hidden in [
            "bob held secret",
            "rival tenant secret",
            "helper held outside",
        ] {
            assert!(!page.contains(hidden), "{path} shows {hidden}");
        }
        let json_path = path.replace("/web/memories", "/web/data/memories");
        let (status, _, value) = json_of(&rig, &json_path, &alice).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let text = value.to_string();
        for hidden in [
            "bob held secret",
            "rival tenant secret",
            "helper held outside",
        ] {
            assert!(!text.contains(hidden), "{json_path} answers {hidden}");
        }
    }
    // Inside an in-progress workspace every author's held row is served,
    // and still neither is listed under Approved (ADR-0083 d1).
    let (status, _, page) = act(
        &rig,
        &alice,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    let inside = agent_writes_in(&rig, &alice_agent, "alice settled inside", "alice.proj").await;
    put_in_progress(&rig, &alice, "alice.proj").await;
    reviewer_holds_a_successor(&rig, &helper.value, inside, "helper held inside").await;
    let (_, _, page) = send(
        &rig.router,
        get("/web/memories?workspace=alice.proj", Some(&alice)),
    )
    .await;
    assert!(!page.contains("helper held inside"), "{page}");
    assert!(!page.contains("helper held outside"), "{page}");
}

#[tokio::test]
async fn the_json_route_answers_exactly_the_lists_rows() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let (alice, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    for i in 0..5 {
        agent_writes(&rig, &agent, &format!("shared fact {i}")).await;
    }
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (cookie, page_path, data_path, first) in [
        (
            &alice,
            "/web/memories",
            "/web/data/memories",
            "view=submitted&limit=2&state=held",
        ),
        (
            &boss,
            "/web/admin/memories",
            "/web/admin/data/memories",
            "view=held&limit=2",
        ),
    ] {
        let mut query = first.to_owned();
        let mut pages = 0;
        loop {
            let (status, _, page) = send(
                &rig.router,
                get(&format!("{page_path}?{query}"), Some(cookie)),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{page}");
            let (status, headers, value) =
                json_of(&rig, &format!("{data_path}?{query}"), cookie).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(json_ids(&value), card_ids(&page), "{page_path}?{query}");
            assert_eq!(headers[header::CACHE_CONTROL], "no-store");
            assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert!(
                headers[header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .starts_with("application/json")
            );
            assert!(!headers.contains_key("access-control-allow-origin"));
            for item in value["items"].as_array().unwrap() {
                for field in item.as_object().unwrap().keys() {
                    assert!(
                        [
                            "artifact_id",
                            "state",
                            "workspace",
                            "version",
                            "title",
                            "text",
                            "claim_key",
                            "approved_at",
                            "approver",
                            "author",
                            "author_kind",
                            "anchor",
                            "format",
                            "valid"
                        ]
                        .contains(&field.as_str()),
                        "a field the card does not show: {field}"
                    );
                }
            }
            pages += 1;
            match (next_link(&page), value["next_cursor"].as_str()) {
                // Each answer mints its own cursor, bound to its issue time;
                // the next page is the same either way.
                (Some(next), Some(_)) => {
                    assert!(next.contains("cursor="), "{next}");
                    query = next.split_once('?').unwrap().1.to_owned();
                }
                (None, None) => break,
                other => panic!("the list and the route disagree on a next page: {other:?}"),
            }
        }
        assert_eq!(pages, 3, "{page_path}");
    }
    // Nothing an agent holds reaches the route, and no session is not found.
    let (status, _, body) = send(
        &rig.router,
        Request::get("/web/data/memories")
            .header(header::AUTHORIZATION, format!("Bearer {agent}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("web_is_person_only"), "{body}");
    for path in ["/web/data/memories", "/web/admin/data/memories"] {
        let (status, _, body) = send(&rig.router, get(path, None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        assert!(body.contains("access.not_found"), "{body}");
    }
    let (status, _, _) = send(&rig.router, get("/web/admin/data/memories", Some(&alice))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a non-admin session");
}

#[tokio::test]
async fn every_admin_page_of_memories_is_one_admin_read_event() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let (_, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    for i in 0..3 {
        agent_writes(&rig, &agent, &format!("audited row {i}")).await;
    }
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let reads = || async {
        rig.store
            .events_range(rig.boot.tenant_id, 1, 1000)
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.envelope.event_type == "memory.admin_read")
            .map(|e| e.envelope.payload)
            .collect::<Vec<_>>()
    };
    let before = reads().await.len();
    let (_, _, page) = send(
        &rig.router,
        get("/web/admin/memories?view=held&limit=2", Some(&boss)),
    )
    .await;
    assert_eq!(card_ids(&page).len(), 2);
    let after_first = reads().await;
    assert_eq!(after_first.len(), before + 1, "one event for the page");
    assert_eq!(
        after_first.last().unwrap()["artifact_ids"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "naming the page's rows, never one event per row"
    );
    let next = next_link(&page).expect("a second page");
    let _ = send(&rig.router, get(&next, Some(&boss))).await;
    assert_eq!(reads().await.len(), before + 2);
    let (_, _, _) = json_of(&rig, "/web/admin/data/memories?limit=2", &boss).await;
    assert_eq!(reads().await.len(), before + 3, "the JSON page is one read");
    // The Atlas page itself serves no memory and journals nothing.
    let _ = send(&rig.router, get("/web/admin/atlas?limit=2", Some(&boss))).await;
    assert_eq!(reads().await.len(), before + 3);
    // A text search journals its digest, never the text.
    let _ = send(
        &rig.router,
        get("/web/admin/memories?q=audited", Some(&boss)),
    )
    .await;
    let last = reads().await.pop().unwrap();
    assert!(last["query_digest"].is_string());
    assert!(!last.to_string().contains("audited"));
}

#[tokio::test]
async fn the_atlas_view_is_a_treemap_drawn_on_the_server_with_no_script() {
    let rig = rig().await;
    let (cookie, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    agent_writes(&rig, &agent, "drawn as a region").await;
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    agent_writes_in(&rig, &agent, "inside the child", "alice.proj").await;
    let (status, headers, page) = send(&rig.router, get("/web/atlas", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(
        !csp.contains("script-src") && !csp.contains("connect-src"),
        "{csp}"
    );
    assert!(
        !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
        "{csp}"
    );
    assert!(csp.starts_with("default-src 'none'"), "{csp}");
    assert!(!page.contains("<script"), "the Atlas runs no script");
    // The map is SVG on the page, each region a plain link that zooms to it,
    // named in full in its title and label, and the table twin beside it.
    assert!(page.contains("<svg class=\"tm wide\""), "{page}");
    assert!(page.contains("<svg class=\"tm narrow\""), "{page}");
    let proj = rig
        .store
        .workspace_children(
            rig.boot.tenant_id,
            key_named(&rig, "alice-web").await.user.unwrap(),
        )
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "proj")
        .unwrap()
        .workspace_id;
    // Each region links and is named by its reference (design notes row
    // 20), and the scope's panel links up to its parent instead of a
    // breadcrumb (AtlasScreen).
    assert!(
        page.contains(
            "<a href=\"/web/atlas?workspace=alice.proj\" aria-label=\"alice.proj, 1 readable: 1 held\"><title>alice.proj, 1 readable: 1 held</title>"
        ),
        "{page}"
    );
    assert!(page.contains("id=\"atlas-table\""), "{page}");
    assert!(page.contains("<td class=\"num\">1</td>"), "{page}");
    assert!(!page.contains("aria-label=\"Breadcrumb\""), "{page}");
    assert!(
        page.contains(r#"Up to <a href="/web/atlas?workspace=universe">universe</a>"#),
        "{page}"
    );
    // The vendored scripts are gone with the script.
    let (status, _, _) = send(
        &rig.router,
        get(
            "/web/assets/three-218b9f9420af1ee2b773fa066cee0eeb52f7ac76ca2fa11b11b732c1ba0a6e39.min.js",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // A region zooms the Atlas, and the panel's dropdown is the
    // Workspaces view's.
    let (_, _, zoomed) = send(
        &rig.router,
        get(&format!("/web/atlas?workspace={proj}"), Some(&cookie)),
    )
    .await;
    assert!(
        zoomed.contains(r#"<h2 id="sel-h" class="mono">alice.proj</h2><p class="oml-atlas-up">Up to <a href="/web/atlas?workspace=alice">alice</a></p>"#),
        "{zoomed}"
    );
    assert!(zoomed.contains("<select name=\"op\""), "{zoomed}");
    // Apply asks the Workspaces view, which oml-web serves at its own path
    // (ADR-0070 d7 (b)).
    assert!(
        zoomed.contains(r#"<form class="row act" method="get" action="/web/workspaces">"#),
        "{zoomed}"
    );
    // The list links to the Atlas of the same page, filters kept, in one
    // link (design notes row 23).
    let (_, _, list) = send(
        &rig.router,
        get("/web/memories?state=superseded", Some(&cookie)),
    )
    .await;
    assert!(
        list.contains(r#"<a href="/web/atlas?state=superseded">The same filters on the Atlas</a>"#),
        "{list}"
    );
}

#[tokio::test]
async fn the_filters_narrow_the_list() {
    let rig = rig().await;
    let (cookie, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let held = agent_writes(&rig, &agent, "a held kestrel").await;
    let approved = agent_writes(&rig, &agent, "an approved kestrel").await;
    // Next opens the oldest first; a verdict lands back on To do.
    let (judged, (_, _, page)) = judge_next(&rig, &cookie, "reject", Some("not this one")).await;
    assert_eq!(judged, held.to_string());
    assert!(page.contains("is now rejected"), "{page}");
    let (judged, (_, _, page)) = judge_next(&rig, &cookie, "approve", None).await;
    assert_eq!(judged, approved.to_string());
    assert!(page.contains("is now approved"), "{page}");
    let third = agent_writes(&rig, &agent, "still held kestrel").await;
    // A second agent of alice's, to filter by author.
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "phone"),
            ("role", "contributor"),
        ],
    )
    .await;
    let phone = value_of(&page);
    let by_phone = agent_writes(&rig, &phone, "written on the phone").await;
    let ids = |page: &str| {
        card_ids(page)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    };
    let set = |v: &[uuid::Uuid]| v.iter().map(ToString::to_string).collect();
    // Approved lists the promoted tips, Submitted the person's own rows
    // (ADR-0083 d1, d3); the filters narrow each.
    for (query, expected) in [
        ("view=submitted&state=held", set(&[third, by_phone])),
        ("state=promoted", set(&[approved])),
        ("", set(&[approved])),
        ("view=submitted&state=rejected", set(&[held])),
        ("view=submitted&author=alice-phone", set(&[by_phone])),
        (
            "view=submitted&author=alice-laptop&state=held",
            set(&[third]),
        ),
        ("origin=chat", std::collections::BTreeSet::new()),
        (
            "view=submitted&origin=manual&state=held",
            set(&[third, by_phone]),
        ),
        ("q=kestrel", set(&[approved])),
        (
            "q=kestrel&state=superseded",
            std::collections::BTreeSet::new(),
        ),
    ] {
        let (status, _, page) = send(
            &rig.router,
            get(&format!("/web/memories?{query}"), Some(&cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{query}: {page}");
        assert_eq!(ids(&page), expected, "{query}: {page}");
    }
    // The widened own-key set also opens a rejected agent row by id.
    let (status, _, rejected) = send(
        &rig.router,
        get(&format!("/web/memory?id={held}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rejected}");
    assert!(rejected.contains("a held kestrel"), "{rejected}");
    assert!(rejected.contains(">rejected<"), "{rejected}");
    // A key that is not the person's is not found; a value outside its
    // grammar names its field.
    let (status, _, page) = send(
        &rig.router,
        get("/web/memories?author=root-admin", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
    for (query, field) in [("state=lost", "state"), ("origin=dream", "origin")] {
        let (status, _, page) = send(
            &rig.router,
            get(&format!("/web/memories?{query}"), Some(&cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert!(page.contains(field), "{page}");
    }
    // The admin page filters by author and origin as well.
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, page) = send(
        &rig.router,
        get(
            "/web/admin/memories?view=held&author=alice-phone",
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(ids(&page), set(&[by_phone]));
    let (_, _, page) = send(
        &rig.router,
        get("/web/admin/memories?origin=chat", Some(&boss)),
    )
    .await;
    assert_eq!(ids(&page), std::collections::BTreeSet::new());
    let (_, _, page) = send(
        &rig.router,
        get(
            "/web/admin/memories?view=held&origin=manual&author=alice-phone",
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(ids(&page), set(&[by_phone]));
}

/// Puts `workspace` in progress as the signed-in person's reviewer key,
/// through the app: the act is the Workspaces view's, which `oml-web` runs
/// now (ADR-0070 d7 (b)); these tests read what it changes.
async fn put_in_progress(rig: &Rig, cookie: &str, workspace: &str) {
    let token = cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .split_once('=')
        .expect("a cookie")
        .1;
    let session = rig
        .app
        .web_session(token)
        .await
        .expect("a read")
        .expect("a live session");
    let review = rig
        .app
        .web_caller(&session, oml_app::app::Capacity::Review)
        .await
        .expect("the person's reviewer key");
    rig.app
        .set_workspace_mode(
            oml_app::types::Bearer::Session(&review),
            oml_app::domain::name::Reference::parse(workspace).expect("a reference"),
            oml_app::domain::in_progress::WorkspaceMode::InProgress,
        )
        .await
        .expect("the reviewer key puts it in progress");
}

fn region<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    value["workspaces"]["items"]
        .as_array()
        .expect("workspaces")
        .iter()
        .find(|r| r["path"] == path)
}

#[tokio::test]
async fn the_atlas_draws_and_counts_only_the_workspaces_and_rows_the_viewer_may_read() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", false).await;
    rig.allow("boss@example.com", "boss", true).await;
    let (alice, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let (_, bob_agent) = person_with_agent(&rig, "sub-bob", "bob@example.com").await;
    agent_writes(&rig, &bob_agent, "bob held secret").await;
    agent_writes(&rig, &agent, "alice one").await;
    agent_writes(&rig, &agent, "alice two").await;
    let (status, _, _) = act(
        &rig,
        &alice,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    // Another author's rows in alice's root: a tenant reviewer granted
    // there holds a successor of a served row of hers, outside the
    // in-progress child and inside it (ADR-0044 d16).
    let helper = reviewer_in_alices_root(&rig).await;
    let settled = agent_writes(&rig, &agent, "alice settled this").await;
    reviewer_holds_a_successor(&rig, &helper.value, settled, "helper held outside").await;
    let inside = agent_writes_in(&rig, &agent, "alice settled inside", "alice.proj").await;
    put_in_progress(&rig, &alice, "alice.proj").await;
    reviewer_holds_a_successor(&rig, &helper.value, inside, "helper held inside").await;

    let (status, _, value) = json_of(&rig, "/web/data/memories", &alice).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert!(
        region(&value, "universe.bob").is_none(),
        "bob's root is not drawn: {value}"
    );
    let root = region(&value, "universe.alice").expect("alice's root");
    assert_eq!(
        root["counts"]["held"], 2,
        "her agent's two, never the helper's: {root}"
    );
    assert_eq!(
        root["counts"]["total"], 3,
        "and the served row the helper proposed to replace: {root}"
    );
    let proj = region(&value, "universe.alice.proj").expect("the in-progress child");
    assert_eq!(proj["in_progress"], true);
    assert_eq!(
        proj["counts"]["in_progress"], 1,
        "every author's row inside: {proj}"
    );
    assert_eq!(proj["parent"], root["id"], "nested in its parent");
    assert!(!value.to_string().contains("bob held secret"));
    // Offers cost several store reads each, so only the scope node carries
    // them: no other region does, however many the tree holds.
    assert!(
        root.get("offers").is_some(),
        "the scope node's offers: {root}"
    );
    assert!(
        value["items"].is_array()
            && value["workspaces"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r.get("offers").is_some())
                .count()
                == 1,
        "exactly one region carries offers: {value}"
    );
    assert!(proj.get("offers").is_none() && proj.get("commit_withheld").is_none());
    // Zooming to a region narrows the drawing and the page alike.
    let proj_id = proj["id"].as_str().unwrap().to_owned();
    let (_, _, zoomed) = json_of(
        &rig,
        &format!("/web/data/memories?workspace={proj_id}"),
        &alice,
    )
    .await;
    let paths: Vec<&str> = zoomed["workspaces"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["universe.alice.proj"]);
    // The zoomed node's dropdown is the Workspaces view's: the commit waits
    // on the held row and says so.
    assert_eq!(zoomed["workspaces"]["items"][0]["commit_withheld"], 1);
    assert!(
        zoomed["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["workspace"] == "universe.alice.proj"),
        "{zoomed}"
    );
    // A workspace the person may not read is not found, never drawn empty.
    let bob_root = key_named(&rig, "bob-web").await.user.unwrap();
    let (status, _, _) = json_of(
        &rig,
        &format!("/web/data/memories?workspace={bob_root}"),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // The page draws what the route counts: no region of bob's.
    let (_, _, atlas) = send(&rig.router, get("/web/atlas", Some(&alice))).await;
    assert!(!atlas.contains("bob"), "{atlas}");
    assert!(
        atlas.contains("aria-label=\"alice.proj, 2 readable: 1 approved, 1 unreviewed in progress"),
        "the served row and the helper's held successor of it: {atlas}"
    );
    let (status, _, _) = send(
        &rig.router,
        get(&format!("/web/atlas?workspace={bob_root}"), Some(&alice)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // The admin page draws the tenant, the admin read's scope and no wider.
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, admin) = json_of(&rig, "/web/admin/data/memories", &boss).await;
    let bob = region(&admin, "universe.bob").expect("the admin reads every root");
    assert_eq!(bob["counts"]["held"], 1);
}

#[tokio::test]
async fn the_old_memories_urls_are_not_found_and_the_list_and_atlas_answer_at_their_own_paths() {
    // ADR-0070 d7 (b) as amended: the Memories list and the Atlas each have
    // their own path under /web/, and one memory its own page; the lead's
    // 303s from the old ?view= forms are deleted in v0.7.1 (ADR-0070, "The
    // old URLs deleted").
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let (cookie, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let memory = agent_writes(&rig, &agent, "a row the old link opens").await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (old, who, new) in [
        ("/web?view=memories", &cookie, "/web/memories"),
        ("/web?view=memories&state=held", &cookie, "/web/memories"),
        ("/web?view=memories&display=atlas", &cookie, "/web/atlas"),
        ("/web/admin?view=memories", &boss, "/web/admin/memories"),
        (
            "/web/admin?view=memories&display=atlas",
            &boss,
            "/web/admin/atlas",
        ),
    ] {
        let (status, headers, _) = send(&rig.router, get(old, Some(who))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
        let (status, _, page) = send(&rig.router, get(new, Some(who))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let h1 = if new.ends_with("atlas") {
            "<h1>Atlas</h1>"
        } else {
            "<h1>Memories</h1>"
        };
        assert!(page.contains(h1), "{new}: {page}");
    }
    // The list's rows link to one memory's own page; the old link to it is
    // not found.
    let (_, _, list) = send(
        &rig.router,
        get("/web/memories?view=submitted", Some(&cookie)),
    )
    .await;
    assert!(
        list.contains(&format!(
            r#"<a href="/web/memory?id={memory}">this memory</a>"#
        )),
        "{list}"
    );
    let (status, headers, _) = send(
        &rig.router,
        get(
            &format!("/web?view=memories&memory={memory}"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(headers.get(header::LOCATION).is_none());
    let (status, _, opened) = send(
        &rig.router,
        get(&format!("/web/memory?id={memory}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert!(opened.contains("a row the old link opens"), "{opened}");
}

#[tokio::test]
async fn the_memories_pages_draw_the_header_and_tabs_the_leads_pages_draw() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    let cookie = signed_in(&rig, "sub-2", "bob@example.com").await;
    let (_, _, lead) = send(&rig.router, get("/web/setup", Some(&cookie))).await;
    for (path, tab) in [
        ("/web/memories", "/web/memories"),
        ("/web/atlas", "/web/atlas"),
    ] {
        let (status, _, moved) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(header_of(&moved), header_of(&lead), "{path}");
        assert!(
            moved.contains(&format!(
                r#"<a href="{tab}" class="on" aria-current="page">"#
            )),
            "{moved}"
        );
    }
    let (_, _, lead) = send(&rig.router, get("/web/admin/people", Some(&cookie))).await;
    for path in ["/web/admin/memories", "/web/admin/atlas"] {
        let (status, _, moved) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(header_of(&moved), header_of(&lead), "{path}");
        assert!(
            moved.contains(&format!(
                r#"<a href="{path}" class="on" aria-current="page">"#
            )),
            "{moved}"
        );
    }
    // A person who is not an admin finds no admin page here.
    let alice = signed_in(&rig, "sub-1", EMAIL).await;
    for path in ["/web/admin/memories", "/web/admin/atlas"] {
        let (status, _, page) = send(&rig.router, get(path, Some(&alice))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {page}");
    }
}

#[tokio::test]
async fn a_resubmitted_create_form_creates_its_workspace_once() {
    // ADR-0070 d3 layer 4: the create form carries a nonce, and its
    // Idempotency-Key is the nonce's digest with the session token, so the
    // same form sent again replays at the API instead of answering
    // name.taken.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, view) = send(&rig.router, get("/web/workspaces", Some(&cookie))).await;
    let nonce = field_of(&view, "nonce");
    let fields = [
        ("csrf_token", csrf_of(&view)),
        ("action", "create_workspace".to_owned()),
        ("name", "once".to_owned()),
        ("nonce", nonce),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..2 {
        let (status, headers, page) =
            send(&rig.router, post("/web/workspaces", &fields, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        assert_eq!(headers[header::LOCATION], "/web/workspaces?done=created");
    }
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let created = rig
        .store
        .workspace_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .filter(|w| w.name.as_str() == "once")
        .count();
    assert_eq!(created, 1);
    // A name already taken by another form is refused by name.
    let (_, _, view) = send(&rig.router, get("/web/workspaces", Some(&cookie))).await;
    let (status, _, page) = send(
        &rig.router,
        post(
            "/web/workspaces",
            &[
                ("csrf_token", &csrf_of(&view)),
                ("action", "create_workspace"),
                ("name", "once"),
                ("nonce", &field_of(&view, "nonce")),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{page}");
    assert!(page.contains("name.taken"), "{page}");
}

/// Rotation without a refused page, over the real gate (ADR-0070 d2,
/// #230): a lead holding a new secret and the old one until a future
/// instant takes both from oml-web, and the doctor's probe says so; a
/// different secret, or the old one once its window has closed, is
/// refused, and the probe names `web.client_secret`.
#[tokio::test]
async fn a_lead_mid_rotation_accepts_the_old_and_new_secret_and_the_doctor_tells_a_refused_one_apart()
 {
    use oml_web::doctor::{Level, secret_line};
    const OLD: &str = "parity-old-web-client-secret-9876543210";
    let lead = |until_us: i64| {
        let store = MemStore::new();
        let ping: DbPing = Arc::new(|| Box::pin(async { true }));
        ApiState::new(Arc::new(test_app(&store)), store, ping, true, None)
            .with_web_client(
                WebClient::new(Some(WEB_CLIENT_SECRET.to_owned()), None)
                    .with_previous(OLD.to_owned(), until_us),
            )
            .into_router()
    };
    let probe = |lead: SocketAddr, secret: &'static str| async move {
        let url = format!("http://{lead}");
        let transport =
            oml_sdk::HttpTransport::new(&url, Duration::from_secs(10)).expect("a transport");
        let api = oml_web::api(Arc::new(transport), secret).expect("an API client");
        secret_line(&api, &url).await
    };
    let open = serve(lead(i64::MAX)).await;
    for secret in [WEB_CLIENT_SECRET, OLD] {
        let (level, text) = probe(open, secret).await;
        assert_eq!(level, Level::Ok, "{text}");
    }
    let (level, text) = probe(open, "another-web-client-secret-0123456789").await;
    assert_eq!(level, Level::Fail, "{text}");
    assert!(text.contains("web.client_secret"), "{text}");
    let closed = serve(lead(1)).await;
    assert_eq!(probe(closed, WEB_CLIENT_SECRET).await.0, Level::Ok);
    let (level, text) = probe(closed, OLD).await;
    assert_eq!(level, Level::Fail, "the window closed: {text}");
}

// -- the workspace picker and its reference (design notes rows 14, 20, 21) --

/// alice with `proj` and `docs` inside it, a memory in each, and boss, an
/// admin: alice's cookie, boss's cookie.
async fn alice_proj_docs(rig: &Rig) -> (String, String) {
    rig.allow("boss@example.com", "boss", true).await;
    let (alice, agent) = person_with_agent(rig, "sub-1", EMAIL).await;
    alice_proj(rig, &alice).await;
    let (status, _, _) = act(
        rig,
        &alice,
        "/web/workspaces",
        &[
            ("action", "create_workspace"),
            ("name", "docs"),
            ("parent", "alice.proj"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    agent_writes_in(rig, &agent, "inside proj", "alice.proj").await;
    agent_writes_in(rig, &agent, "inside docs", "alice.docs").await;
    let boss = signed_in(rig, "sub-boss", "boss@example.com").await;
    (alice, boss)
}

#[tokio::test]
async fn the_workspaces_pages_name_a_workspace_by_the_reference_every_field_takes() {
    let rig = rig().await;
    let (alice, boss) = alice_proj_docs(&rig).await;
    // Its rows are held: Submitted lists them on the person's side, Held
    // on the admin's (ADR-0083 d3, d5).
    for (cookie, path, memories) in [
        (&alice, "/web/workspaces", "/web/memories?view=submitted&"),
        (
            &boss,
            "/web/admin/workspaces",
            "/web/admin/memories?view=held&",
        ),
    ] {
        let (status, _, page) = send(&rig.router, get(path, Some(cookie))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(
            !page.contains("alice.proj.docs"),
            "{path} prints a path no field takes: {page}"
        );
        assert!(page.contains(">alice.docs<"), "{path}: {page}");
        // The name the page prints is one the Memories field takes.
        let (status, _, listed) = send(
            &rig.router,
            get(&format!("{memories}workspace=alice.docs"), Some(cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{listed}");
        assert!(listed.contains("inside docs"), "{listed}");
    }
}

#[tokio::test]
async fn a_bare_name_resolves_on_the_admin_pages_as_on_the_persons() {
    let rig = rig().await;
    let (alice, boss) = alice_proj_docs(&rig).await;
    for (cookie, path) in [
        (&alice, "/web/memories?workspace=docs"),
        (&boss, "/web/admin/memories?workspace=docs"),
        (&alice, "/web/atlas?workspace=docs"),
        (&boss, "/web/admin/atlas?workspace=docs"),
    ] {
        let (status, _, page) = send(&rig.router, get(path, Some(cookie))).await;
        assert_eq!(status, StatusCode::OK, "{path}: {page}");
        assert!(
            page.contains(r#"name="workspace" type="text" value="alice.docs""#),
            "{path} shows the reference it resolved: {page}"
        );
    }
}

#[tokio::test]
async fn the_admin_atlas_links_up_by_a_reference_that_resolves_below_a_user_root() {
    let rig = rig().await;
    let (_alice, boss) = alice_proj_docs(&rig).await;
    let (status, _, page) = send(
        &rig.router,
        get("/web/admin/atlas?workspace=alice.docs", Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let links: Vec<String> = page
        .match_indices("href=\"/web/admin/atlas?workspace=")
        .map(|(at, m)| {
            let start = at + "href=\"".len();
            page[start..at + m.len() + page[at + m.len()..].find('"').unwrap()]
                .replace("&amp;", "&")
        })
        .collect();
    assert!(
        links
            .iter()
            .any(|l| l == "/web/admin/atlas?workspace=alice.proj"),
        "the scope links up to its parent by reference: {links:?}"
    );
    for link in &links {
        let (status, _, answer) = send(&rig.router, get(link, Some(&boss))).await;
        assert_eq!(status, StatusCode::OK, "{link}: {answer}");
    }
}

#[tokio::test]
async fn an_unknown_and_an_unreadable_workspace_get_one_refusal_naming_only_readable_names() {
    let rig = rig().await;
    let (alice, _boss) = alice_proj_docs(&rig).await;
    rig.allow("bob@example.com", "bob", false).await;
    signed_in(&rig, "sub-bob", "bob@example.com").await;
    for (typed, said) in [("bob", "bob"), ("nosuch", "nosuch"), ("BOB", "bob")] {
        for path in ["/web/memories", "/web/atlas"] {
            let (status, _, page) = send(
                &rig.router,
                get(&format!("{path}?workspace={typed}"), Some(&alice)),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path} {typed}: {page}");
            let sentence = format!("No readable workspace is named <code>{said}</code>.");
            assert!(page.contains(&sentence), "{path} {typed}: {page}");
            assert!(page.contains(r#"aria-invalid="true""#), "{page}");
            assert!(!page.contains("<option value=\"bob"), "{page}");
            assert!(
                !page.contains("oml-mlist\"><li"),
                "nothing is listed: {page}"
            );
            assert!(page.contains("No close name"), "{page}");
        }
    }
    // A name near a readable one is offered by its reference.
    let (status, _, page) = send(
        &rig.router,
        get("/web/memories?workspace=prj", Some(&alice)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
    assert!(
        page.contains(r#"<a href="/web/memories?workspace=alice.proj">alice.proj</a>"#),
        "{page}"
    );
    // A typed name is trimmed and lower-cased.
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web/memories?view=submitted&workspace=%20ALICE.PROJ%20",
            Some(&alice),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains(r#"type="text" value="alice.proj""#), "{page}");
    assert!(page.contains("inside proj"), "{page}");
}

#[tokio::test]
async fn the_workspaces_page_draws_fifty_rows_a_page_with_a_link_to_the_next() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for n in 0..51 {
        let name = format!("w{n:02}");
        let (status, _, body) = act(
            &rig,
            &cookie,
            "/web/workspaces",
            &[("action", "create_workspace"), ("name", &name)],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{name}: {body}");
    }
    // alice's root and its 51 children: 52 rows.
    let (status, _, first) = send(&rig.router, get("/web/workspaces", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        first.matches("<select name=\"op\"").count()
            + first.matches("nothing to do here now").count(),
        50,
        "{first}"
    );
    assert!(
        first.contains(r#"<p class="pager">1–50 of 52 · <a rel="next" href="/web/workspaces?from=50">Next page</a></p>"#),
        "{first}"
    );
    let (status, _, next) = send(&rig.router, get("/web/workspaces?from=50", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{next}");
    assert!(next.contains(">alice.w50<"), "{next}");
    assert!(!next.contains(">alice.w00<"), "{next}");
    assert!(
        next.contains(
            r#"<p class="pager">51–52 of 52 · <a href="/web/workspaces">First page</a></p>"#
        ),
        "{next}"
    );
    let (status, _, _) = send(&rig.router, get("/web/workspaces?from=x", Some(&cookie))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// -- the two sides (the design system's BrandHeader, variant A) --------------

/// A page without its stylesheet, whose comments and selectors are not
/// what the page says.
fn markup(page: &str) -> String {
    page.replace(oml_page::web::STYLESHEET, "")
}

/// The text between the first `open` and the `close` after it.
fn between<'a>(page: &'a str, open: &str, close: &str) -> &'a str {
    let start = page.find(open).map_or(page.len(), |i| i + open.len());
    let end = page[start..].find(close).map_or(page.len(), |i| i + start);
    &page[start..end]
}

/// The side's `<nav>` a page with `current` its current tab draws: the
/// side's pages alone, in the BrandHeader's order.
fn side_nav(label: &str, tabs: &[(&str, &str)], current: &str) -> String {
    let mut nav = format!(r#"<nav aria-label="{label}">"#);
    for (href, word) in tabs {
        let on = if *href == current {
            r#" class="on" aria-current="page""#
        } else {
            ""
        };
        nav.push_str(&format!(r#"<a href="{href}"{on}>{word}</a>"#));
    }
    nav + "</nav>"
}

/// The Personal tabs: no Review tab, since To do is the badge every page's
/// header carries (ADR-0083 d6), and no Guardrails tab, which folded into
/// the workspace page (d12). The Catalog follows Memories (ADR-0092 d7).
const PERSONAL: [(&str, &str); 6] = [
    ("/web/start", "Start"),
    ("/web/memories", "Memories"),
    ("/web/catalog", "Catalog"),
    ("/web/atlas", "Atlas"),
    ("/web/keys", "Keys"),
    ("/web/workspaces", "Workspaces"),
];

/// The Administration tabs, likewise without Review (ADR-0083 d6).
const ADMINISTRATION: [(&str, &str); 7] = [
    ("/web/admin/people", "People"),
    ("/web/admin/users", "Quotas"),
    ("/web/admin/memories", "Memories"),
    ("/web/admin/atlas", "Atlas"),
    ("/web/admin/keys", "Keys"),
    ("/web/admin/workspaces", "Workspaces"),
    ("/web/admin/curate", "Curate"),
];

/// Each tab's page, on each side, draws that side's bar: its nav exactly,
/// the scope word, the To do badge of its side before the account name
/// (ADR-0083 d6), the switch to the other side, the `<title>` and an
/// `<h1>` that is the tab's word. The side's To do page, which no tab
/// names, draws the same bar with no tab marked and its badge marked.
#[tokio::test]
async fn each_side_draws_its_own_nav_and_each_page_is_titled_and_headed_by_its_tab() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    let bob = signed_in(&rig, "sub-2", "bob@example.com").await;
    for (side, label, tabs, title, switch, todo) in [
        (
            "personal",
            "Personal",
            &PERSONAL[..],
            "",
            r#"<a class="switch" href="/web/admin/people">"#,
            "/web/todo",
        ),
        (
            "admin",
            "Administration",
            &ADMINISTRATION[..],
            "Admin · ",
            r#"<a class="switch" href="/web/start">"#,
            "/web/admin/todo",
        ),
    ] {
        for (path, word) in tabs.iter().chain([&(todo, "To do")]) {
            let (status, _, page) = send(&rig.router, get(path, Some(&bob))).await;
            assert_eq!(status, StatusCode::OK, "{path}: {page}");
            let page = markup(&page);
            assert_eq!(
                between(&page, "<title>", "</title>"),
                format!("{title}{word} · OML"),
                "{path}"
            );
            assert_eq!(between(&page, "<h1>", "</h1>"), *word, "{path}");
            let header = between(&page, "<header", "</header>");
            assert!(
                header.starts_with(&format!(r#" class="top {side}">"#)),
                "{path}: {header}"
            );
            assert_eq!(
                format!("<nav{}</nav>", between(header, "<nav", "</nav>")),
                side_nav(label, tabs, path),
                "{path}"
            );
            assert!(
                header.contains(&format!("{label}</span><nav")),
                "{path}: {header}"
            );
            assert_eq!(header.matches(r#"class="switch""#).count(), 1, "{path}");
            assert!(header.contains(switch), "{path}: {header}");
            let current = if *path == todo {
                r#" aria-current="page""#
            } else {
                ""
            };
            assert!(
                header.contains(&format!(
                    r#"<div class="end"><a class="oml-todo-b" href="{todo}"{current}>To do · 0</a><span class="who">"#
                )),
                "{path}: {header}"
            );
            assert!(!header.contains(">Review<"), "{path}: {header}");
            assert!(!page.contains("Your page"), "{path}");
        }
    }
}

/// A person without the admin capacity is shown no word, link or address
/// of the Administration side: no switch, no admin tab, no admin title,
/// on any page of theirs or on the refusal of an admin address, which is
/// drawn in the Personal bar and leads back to Start (ADR-0045 d10).
#[tokio::test]
async fn a_person_without_the_admin_capacity_sees_nothing_of_the_admin_side() {
    let rig = rig().await;
    let alice = signed_in(&rig, "sub-1", EMAIL).await;
    let mut pages = Vec::new();
    for (path, _) in PERSONAL.into_iter().chain([("/web/todo", "To do")]) {
        let (status, _, page) = send(&rig.router, get(path, Some(&alice))).await;
        assert_eq!(status, StatusCode::OK, "{path}: {page}");
        pages.push((path.to_owned(), page));
    }
    for (path, _) in ADMINISTRATION
        .into_iter()
        .chain([("/web/admin/todo", "To do")])
    {
        let (status, _, page) = send(&rig.router, get(path, Some(&alice))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {page}");
        assert!(
            page.contains(r#"<header class="top personal">"#),
            "{path}: {page}"
        );
        assert!(
            page.contains(r#"<p><a href="/web/start">Back to Start</a></p>"#),
            "{path}: {page}"
        );
        pages.push((path.to_owned(), page));
    }
    for (path, page) in pages {
        let page = markup(&page);
        for word in [
            "Administration",
            "/web/admin",
            "Admin ·",
            r#"class="switch""#,
        ] {
            assert!(!page.contains(word), "{path} names {word}: {page}");
        }
    }
}

/// A refusal is drawn inside the bar of the side it happened on, with the
/// sign-out control and one link back to the tab of the page that refused.
#[tokio::test]
async fn a_refusal_page_is_drawn_inside_its_sides_bar_with_a_link_back_to_its_tab() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    let bob = signed_in(&rig, "sub-2", "bob@example.com").await;
    let unknown = "00000000-0000-4000-8000-000000000000";
    for (path, side, back) in [
        (
            format!("/web/admin/memory?id={unknown}"),
            "admin",
            r#"<a href="/web/admin/memories">Back to Memories</a>"#,
        ),
        (
            format!("/web/memory?id={unknown}"),
            "personal",
            r#"<a href="/web/memories">Back to Memories</a>"#,
        ),
        (
            "/web/keys?unknown=1&unknown=2".to_owned(),
            "personal",
            r#"<a href="/web/keys">Back to Keys</a>"#,
        ),
    ] {
        let (status, _, page) = send(&rig.router, get(&path, Some(&bob))).await;
        assert!(status.is_client_error(), "{path}: {status} {page}");
        let page = markup(&page);
        assert!(
            page.contains(&format!(r#"<header class="top {side}">"#)),
            "{path}: {page}"
        );
        assert!(page.contains("<h1>This cannot continue</h1>"), "{path}");
        assert!(page.contains(back), "{path}: {page}");
        assert!(page.contains(r#"value="sign_out""#), "{path}");
        assert!(
            page.contains(r#"<span class="name">bob · "#),
            "{path}: {page}"
        );
    }
}

/// Every page's bar names its side and the person at every width: the
/// scope word, the name and the tenant are in the bar's markup, and no
/// rule of the stylesheet hides them below 600px (`oml-page`'s
/// `the_bar_keeps_the_side_and_the_person_at_every_width`).
#[tokio::test]
async fn every_bar_names_its_side_and_the_person() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    let bob = signed_in(&rig, "sub-2", "bob@example.com").await;
    let (_, _, whoami) = send(&rig.router, get("/web/keys", Some(&bob))).await;
    let name = between(&whoami, r#"<span class="name">"#, "</span>").to_owned();
    assert!(
        name.starts_with("bob · ") && name.len() > "bob · ".len(),
        "{name}"
    );
    for (path, label) in PERSONAL
        .iter()
        .map(|(p, _)| (*p, "Personal"))
        .chain([("/web/todo", "Personal")])
        .chain(ADMINISTRATION.iter().map(|(p, _)| (*p, "Administration")))
        .chain([("/web/admin/todo", "Administration")])
    {
        let (_, _, page) = send(&rig.router, get(path, Some(&bob))).await;
        let header = between(&page, "<header", "</header>");
        assert!(
            header.contains(&format!(r#"<span class="name">{name}</span>"#)),
            "{path}"
        );
        assert!(
            header.contains(&format!("{label}</span><nav")),
            "{path}: {header}"
        );
    }
}

// -- one memory, the same facts on every person surface (bulk D fixes) -----

/// The card of `id` on `page`: its `<article>` whole.
fn card_of<'a>(page: &'a str, id: &str) -> &'a str {
    let at = page
        .find(&format!(r#"id="m-{id}""#))
        .unwrap_or_else(|| panic!("no card m-{id}: {page}"));
    let start = page[..at].rfind("<article").expect("an article");
    &page[start..at + page[at..].find("</article>").expect("its end")]
}

/// The head's facts of a card, without the Memories row's version bar
/// (design notes row 12) and the `same user` mark the Judge page adds.
fn head_facts(card: &str) -> String {
    let start = card.find(r#"<ul class="mc-facts">"#).expect("the facts");
    let rest = &card[start..];
    // The facts list holds the version bar's own `<ol>`, never a `</ul>`
    // before its end.
    let mut facts = rest[..rest.find("</li></ul>").expect("the facts' end")].to_owned();
    if let Some(bar) = facts.find(r#"<span class="vb">"#) {
        let end = bar + facts[bar..].find("</ol></span>").expect("the bar's end");
        facts.replace_range(bar..end + "</ol></span>".len(), "");
    }
    facts.replace(
        r#" <span class="tag oml-chip is-quiet">same user</span>"#,
        "",
    )
}

/// The opened facts of a card, as `(label, value)`, without the way in
/// and the page's ids.
fn body_facts(card: &str) -> Vec<(String, String)> {
    let list = between(card, r#"<dl class="oml-facts">"#, "</dl>");
    list.split("<dt>")
        .skip(1)
        .filter_map(|pair| {
            let (label, value) = pair.split_once("</dt><dd>")?;
            let value = value.trim_end_matches("</dd>");
            (!matches!(
                label,
                "Open" | "Id" | "Kind" | "Text digest" | "Attachments"
            ))
            .then(|| (label.to_owned(), value.to_owned()))
        })
        .collect()
}

#[tokio::test]
async fn one_memory_shows_the_same_facts_in_the_same_words_on_memories_judge_and_its_page() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "Backups run at 02:00 UTC.").await;
    let id = memory.to_string();
    // Held, it is listed under Memories, Submitted (ADR-0083 d3).
    let (_, _, memories) = send(
        &rig.router,
        get("/web/memories?view=submitted", Some(&cookie)),
    )
    .await;
    let (_, _, judge_page) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={id}"), Some(&cookie)),
    )
    .await;
    let (_, _, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={id}"), Some(&cookie)),
    )
    .await;
    let cards = [
        ("Memories", card_of(&memories, &id)),
        ("Judge", card_of(&judge_page, &id)),
        ("its page", card_of(&page, &id)),
    ];
    for (surface, card) in &cards {
        // The author as the read serves it, and no kind the read cannot
        // stand behind: its provenance is `agent` for every key.
        assert!(!card.contains(r#"class="kind""#), "{surface}: {card}");
        assert!(!card.contains("data-author-kind"), "{surface}: {card}");
        assert!(
            card.contains(r#"<span class="who">alice-laptop</span>"#),
            "{surface}: {card}"
        );
    }
    let (_, memories_card) = cards[0];
    for (surface, card) in &cards[1..] {
        assert_eq!(
            head_facts(card),
            head_facts(memories_card),
            "{surface}'s head facts"
        );
        assert_eq!(
            body_facts(card),
            body_facts(memories_card),
            "{surface}'s opened facts"
        );
    }
    let labels: Vec<String> = body_facts(memories_card)
        .into_iter()
        .map(|(l, _)| l)
        .collect();
    assert_eq!(
        labels,
        ["Source", "Valid", "Approval", "Served"],
        "{memories_card}"
    );
}

#[tokio::test]
async fn the_approval_line_has_one_wording_on_both_sides_and_never_names_who() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "Tokens rotate every 90 days.").await;
    let id = memory.to_string();
    let (_, _, item) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={id}"), Some(&cookie)),
    )
    .await;
    let form = [
        ("action", "judge"),
        ("artifact_id", id.as_str()),
        ("verdict", "approve"),
        ("reason", ""),
        ("nonce", &field_of(&item, "nonce")),
    ];
    let (status, _, _) = act(&rig, &cookie, "/web/todo/judge", &form).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, person) = send(
        &rig.router,
        get(&format!("/web/memory?id={id}"), Some(&cookie)),
    )
    .await;
    let (_, _, admin) = send(
        &rig.router,
        get(&format!("/web/admin/memory?id={id}"), Some(&boss)),
    )
    .await;
    let approval = |page: &str| {
        body_facts(card_of(page, &id))
            .into_iter()
            .find(|(l, _)| l == "Approval")
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("no Approval: {page}"))
    };
    let (person, admin) = (approval(&person), approval(&admin));
    // The person's read serves the kind of key; the admin read does not,
    // so its line leaves that slot out and keeps the same words.
    assert!(
        person.starts_with("approved by a person's key on 20"),
        "{person}"
    );
    assert!(admin.starts_with("approved on 20"), "{admin}");
    assert_eq!(
        person
            .replace(" by a person's key", "")
            .split(" · ")
            .next()
            .unwrap_or_default(),
        admin,
        "one wording"
    );
    assert!(!admin.contains("alice-review") && !person.contains("alice-review"));
}

/// `universe.platform.oncall`, a shared workspace nested under another, and
/// a held memory in it that a tenant writer wrote; `alice-review` and
/// `boss-review` may judge it. Alice's cookie and the admin's.
async fn held_in_a_nested_shared_workspace(rig: &Rig) -> (String, String, uuid::Uuid) {
    rig.allow("boss@example.com", "boss", true).await;
    let alice = signed_in(rig, "sub-1", EMAIL).await;
    let boss = signed_in(rig, "sub-boss", "boss@example.com").await;
    let admin = &rig.boot.admin_key.value;
    let name = |n: &str| Name::parse(n).expect("a name");
    // Through the lead's API, whose id generator the rig's own app does
    // not share.
    for (child, parent) in [("platform", "universe"), ("oncall", "platform")] {
        let request = Request::post("/v2/workspaces")
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", format!("wa-{child}"))
            .body(Body::from(
                json!({"name": child, "parent": parent}).to_string(),
            ))
            .expect("a request");
        let (status, _, body) = send(&rig.router, request).await;
        assert_eq!(status, StatusCode::CREATED, "{child}: {body}");
    }
    let pager = rig
        .app
        .issue_key(
            admin,
            &IssueKeyRequest {
                profile: None,
                role: Role::Contributor,
                name: name("pager"),
                principal: "pager".to_owned(),
                used_by: Some(UsedBy::Agent),
                default_workspace: None,
                crew: None,
                grants: [Role::Submitter, Role::Reader]
                    .into_iter()
                    .map(|role| GrantRequest {
                        node: name("oncall").into(),
                        role,
                        deny: Vec::new(),
                        covers: oml_app::domain::access::Covers::Subtree,
                        create: false,
                    })
                    .collect(),
                connect_code: false,
                user: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("a tenant writer on oncall");
    let memory = agent_writes_in(rig, &pager.value, "The pager rotates weekly.", "oncall").await;
    for user in ["alice", "boss"] {
        let (status, _, body) = act(
            rig,
            &boss,
            "/web/admin/people",
            &[
                ("action", "grant_review"),
                ("user", user),
                ("workspace", "oncall"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{user}: {body}");
    }
    (alice, boss, memory)
}

#[tokio::test]
async fn a_nested_shared_workspace_is_named_by_its_bare_name_on_todo_judge_and_the_admin_held_view()
{
    let rig = rig().await;
    let (alice, boss, memory) = held_in_a_nested_shared_workspace(&rig).await;
    let (status, _, page) = send(&rig.router, get("/web/todo", Some(&alice))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(&format!(
            r#"href="/web/todo/judge?memory={memory}">Judge</a>"#
        )),
        "{page}"
    );
    assert!(!page.contains("platform.oncall"), "{page}");
    // The row's "in" names it by its bare name.
    assert!(
        page.contains(r#"<span class="k">in</span> <span class="t">oncall</span>"#),
        "{page}"
    );
    // Its wait is to the second, in UTC, as on every page.
    assert!(
        page.contains(r#"<span class="k">waiting since</span> <time datetime="#),
        "{page}"
    );
    assert!(page.contains(" UTC</time></span>"), "{page}");
    assert!(!page.contains("Z</span>"), "no raw instant: {page}");
    // The name the page prints filters the list.
    let (status, _, filtered) =
        send(&rig.router, get("/web/todo?workspace=oncall", Some(&alice))).await;
    assert_eq!(status, StatusCode::OK, "{filtered}");
    assert!(
        filtered.contains(&format!("/web/todo/judge?memory={memory}")),
        "{filtered}"
    );
    // The Judge page: the card's "in" link and the verdict line.
    let (status, _, item) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={memory}"), Some(&alice)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{item}");
    assert!(item.contains("The pager rotates weekly."), "{item}");
    assert!(!item.contains("platform.oncall"), "{item}");
    assert!(
        item.contains(r#"href="/web/memories?workspace=oncall">oncall</a>"#),
        "{item}"
    );
    assert!(
        item.contains("to every key that reads <code>oncall</code>"),
        "{item}"
    );

    let held = r#"href="/web/admin/memories?view=held&amp;workspace=oncall">oncall</a>"#;
    // The admin Held view's table and rows name it the same way, and the
    // name filters the rows.
    let (status, _, listed) = send(
        &rig.router,
        get(
            "/web/admin/memories?view=held&workspace=oncall",
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(listed.contains("The pager rotates weekly."), "{listed}");
    assert!(listed.contains(held), "{listed}");
    let link = r#"href="/web/admin/memories?workspace=oncall">oncall</a>"#;
    assert!(listed.contains(link), "{listed}");
    assert!(!listed.contains("platform.oncall"), "{listed}");
    // The admin's One memory page reads the policy by the same reference.
    let (status, _, one) = send(
        &rig.router,
        get(&format!("/web/admin/memory?id={memory}"), Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert!(link_free(&one, "platform.oncall"), "{one}");
    assert!(
        one.contains("this workspace's review policy asks for"),
        "{one}"
    );
}

/// Whether `page` prints `text` nowhere.
fn link_free(page: &str, text: &str) -> bool {
    !page.contains(text)
}

/// The admin To do lists nothing yet and draws no Judge, Next or verdict
/// form (ADR-0083 d11, d14); a judge posted to it, or to the old admin
/// Review path, is no page's act and records nothing.
#[tokio::test]
async fn the_admin_todo_page_has_no_judge_and_refuses_the_judge_act() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    // A browser that opened no investigation: an open one is a group.
    let cookie = signed_in_without_investigation(&rig, "sub-boss", "boss@example.com").await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "Releases ship on Thursdays.").await;
    let (status, _, page) = send(&rig.router, get("/web/admin/todo", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Nothing waits for this account."), "{page}");
    assert!(!page.contains("Releases ship on Thursdays."), "{page}");
    assert!(!page.contains(">Judge</a>"), "{page}");
    assert!(!page.contains("?memory="), "{page}");
    assert!(!page.contains(r#"value="judge""#), "{page}");
    assert!(!page.contains(r#"value="next""#), "{page}");
    // The old admin Review path opens the admin Held view, which draws no
    // verdict either.
    let (status, _, opened) = send(
        &rig.router,
        get(&format!("/web/admin/review?memory={memory}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert!(!opened.contains(r#"value="judge""#), "{opened}");
    let (_, _, held) = send(
        &rig.router,
        get("/web/admin/memories?view=held", Some(&cookie)),
    )
    .await;
    assert!(!held.contains(r#"value="judge""#), "{held}");
    let head = rig.head().await;
    for path in ["/web/admin/todo", "/web/admin/review"] {
        let (status, _, body) = act(
            &rig,
            &cookie,
            path,
            &[
                ("action", "judge"),
                ("artifact_id", &memory.to_string()),
                ("verdict", "approve"),
                ("reason", ""),
                ("nonce", "n-judge"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
    }
    assert_eq!(rig.head().await, head, "nothing is journalled");
}

/// The `memory.admin_read` events journalled after `head`.
async fn admin_reads_since(rig: &Rig, head: u64) -> usize {
    rig.store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap()
        .iter()
        .filter(|e| e.envelope.event_type == "memory.admin_read")
        .count()
}

/// The admin's One memory page makes one journalled admin read on load:
/// the change against the replaced version is a second admin read, which
/// the admin chooses through "Show the change" (bulk D fixes).
#[tokio::test]
async fn the_admin_memory_page_reads_the_replaced_version_only_when_the_admin_asks() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let (_, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let target = agent_writes(&rig, &agent, "The relay closes at noon.").await;
    let helper = reviewer_in_alices_root(&rig).await;
    reviewer_holds_a_successor(&rig, &helper.value, target, "The relay closes at one.").await;
    let [successor] = memories_reading(&rig, "The relay closes at one.").await[..] else {
        panic!("one successor");
    };
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let head = rig.head().await;
    let path = format!("/web/admin/memory?id={successor}");
    let (status, _, page) = send(&rig.router, get(&path, Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(admin_reads_since(&rig, head).await, 1, "one admin read");
    assert!(!page.contains("Change against"), "{page}");
    let show =
        format!(r#"<a href="/web/admin/memory?id={successor}&amp;change=1">Show the change</a>"#);
    assert!(page.contains(&show), "{page}");
    let head = rig.head().await;
    let (status, _, page) = send(&rig.router, get(&format!("{path}&change=1"), Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        admin_reads_since(&rig, head).await,
        2,
        "the asked-for second read"
    );
    assert!(page.contains("Change against v1"), "{page}");
    assert!(page.contains("<del>noon.</del>"), "{page}");
    // Replaces names the claim and the version, as the person's page
    // does, and Derives from does not name the same version again.
    let replaces = between(&page, "<dt>Replaces</dt><dd>", "</dd>");
    assert!(replaces.contains(" · v1</a>"), "{replaces}");
    assert!(!page.contains(&format!(">{target}</a>")), "{page}");
}

/// To do's Browse link opens Browse, and so does the admin Held view's,
/// which lists the held rows the admin Review page listed (bulk D fixes;
/// ADR-0083 d5, d14): the link the closed field draws leads to a page that
/// draws it open.
#[tokio::test]
async fn the_todo_and_held_pages_browse_link_opens_browse() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let alice = signed_in(&rig, "sub-1", EMAIL).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (cookie, path) in [
        (&alice, "/web/todo"),
        (&boss, "/web/admin/memories?view=held"),
    ] {
        let (_, _, page) = send(&rig.router, get(path, Some(cookie))).await;
        let marker = r#"<a class="oml-wa-browse oml-wa-ask" href=""#;
        let at = page
            .find(marker)
            .unwrap_or_else(|| panic!("{path}: {page}"))
            + marker.len();
        let link = page[at..at + page[at..].find('"').unwrap()].replace("&amp;", "&");
        assert!(link.contains("browse=1"), "{link}");
        let (status, _, opened) = send(&rig.router, get(&link, Some(cookie))).await;
        assert_eq!(status, StatusCode::OK, "{link}: {opened}");
        assert!(
            opened.contains(r#"<details class="oml-wa-browse" open>"#),
            "{link}: {opened}"
        );
    }
}

/// A `judge` form resent with its nonce and another memory's id conflicts,
/// as every other act's resend with another request does, and judges
/// nothing (bulk D fixes): the API keys a listed verdict per item, so the
/// page binds the form's nonce to the item it drew.
#[tokio::test]
async fn a_judge_form_resent_with_another_id_conflicts_and_judges_nothing() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let first = agent_writes(&rig, &value, "The cache warms at boot.").await;
    let second = agent_writes(&rig, &value, "The cache holds a day.").await;
    let (_, _, item) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={first}"), Some(&cookie)),
    )
    .await;
    let nonce = field_of(&item, "nonce");
    let judge = |id: uuid::Uuid| {
        vec![
            ("action", "judge".to_owned()),
            ("artifact_id", id.to_string()),
            ("verdict", "approve".to_owned()),
            ("reason", String::new()),
            ("nonce", nonce.clone()),
        ]
    };
    let sent = judge(first);
    let fields: Vec<(&str, &str)> = sent.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, _) = act(&rig, &cookie, "/web/todo/judge", &fields).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let head = rig.head().await;
    let resent = judge(second);
    let fields: Vec<(&str, &str)> = resent.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, body) = act(&rig, &cookie, "/web/todo/judge", &fields).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("idempotency.digest_mismatch"), "{body}");
    assert_eq!(rig.head().await, head, "nothing is journalled");
    let held = rig
        .store
        .memory(rig.boot.tenant_id, second)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.status.as_str(), "held");
    // The same form sent again for its own item replays.
    let fields: Vec<(&str, &str)> = sent.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, _) = act(&rig, &cookie, "/web/todo/judge", &fields).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(rig.head().await, head, "a replay journals nothing");
}

/// The redesign's pages, as the fix round left them: the Atlas has its own
/// line under its heading on both sides, the admin side's Memories and
/// Atlas name the key that reads, the three wide tables stack below 600px,
/// and the decision is a chip, not a button (bulk D fixes).
#[tokio::test]
async fn the_atlas_has_its_own_lede_and_the_admin_lists_name_the_key_that_reads() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let alice = signed_in(&rig, "sub-1", EMAIL).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, atlas) = send(&rig.router, get("/web/atlas", Some(&alice))).await;
    let (_, _, list) = send(&rig.router, get("/web/memories", Some(&alice))).await;
    // The line under the heading, after the views where the page draws
    // them (ADR-0083 d6).
    let lede = |page: &str| {
        let head = between(page, "<h1>", "</header>");
        between(head, "<p class=\"desc\">", "</p>").to_owned()
    };
    assert_ne!(lede(&atlas), lede(&list), "{atlas}");
    assert!(lede(&atlas).contains("workspace"), "{atlas}");
    for path in ["/web/admin/memories", "/web/admin/atlas"] {
        let (_, _, page) = send(&rig.router, get(path, Some(&boss))).await;
        assert!(
            lede(&page).starts_with("Read as <code>boss-admin</code>"),
            "{path}: {page}"
        );
    }
    let (_, _, admin_atlas) = send(&rig.router, get("/web/admin/atlas", Some(&boss))).await;
    let (_, _, admin_list) = send(&rig.router, get("/web/admin/memories", Some(&boss))).await;
    assert_ne!(lede(&admin_atlas), lede(&admin_list));
    for (path, class) in [("/web/workspaces", "t-wa"), ("/web/keys", "t-keys")] {
        let (_, _, page) = send(&rig.router, get(path, Some(&alice))).await;
        assert!(
            page.contains(&format!(r#"<table class="oml-stack {class}">"#)),
            "{path}: {page}"
        );
    }
    let sheet = oml_page::web::STYLESHEET;
    assert!(sheet.contains(".oml-stack.t-wa td:nth-child(2)::before{content:\"Inside\"}"));
    let decision = between(sheet, ".oml-decision{", "}");
    assert!(
        decision.contains("border-radius:var(--radius-pill)"),
        "{decision}"
    );
}

/// A key of `role` and `used_by` on the shared workspace `team`, with the
/// grants `roles` there, under the tenant's admin key.
async fn team_key(
    rig: &Rig,
    name: &str,
    role: Role,
    used_by: UsedBy,
    roles: &[Role],
) -> oml_app::types::IssuedKey {
    rig.app
        .issue_key(
            &rig.boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role,
                name: Name::parse(name).expect("a name"),
                principal: name.to_owned(),
                used_by: Some(used_by),
                default_workspace: None,
                crew: None,
                grants: roles
                    .iter()
                    .map(|role| GrantRequest {
                        node: Name::parse("team").expect("a name").into(),
                        role: *role,
                        deny: Vec::new(),
                        covers: oml_app::domain::access::Covers::Subtree,
                        create: false,
                    })
                    .collect(),
                connect_code: false,
                user: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("a key on team")
}

/// An address an admin adds for a person who signs in already waits on
/// that person (the operator's decision of 2026-11-19, ADR-0045 d2's note):
/// it signs in to nothing, the person's Keys page offers it to accept or
/// remove, and once accepted it signs in as them; one the person removes is
/// gone.
#[tokio::test]
async fn the_keys_page_offers_a_pending_sign_in_email_to_accept_or_remove() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    rig.allow("alice@work.example", "alice", false).await;
    rig.allow("someone@example.net", "alice", false).await;
    let (status, token, body) = sign_in_google(&rig, "sub-2", "alice@work.example").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(token, None, "a pending email opens no session");

    let (status, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{keys}");
    assert!(keys.contains("<h2>Sign-in emails to accept</h2>"), "{keys}");
    assert!(keys.contains("<code>alice@work.example</code>"), "{keys}");
    // Each pending email shows whether it signs in as an admin and the
    // admin key that added it, and its accept names the flag back (the
    // operator's decision of 2026-11-20).
    assert!(keys.contains("signs in without admin"), "{keys}");
    assert!(keys.contains("added by <code>"), "{keys}");
    assert!(
        keys.contains(r#"<input type="hidden" name="admin" value="false">"#),
        "{keys}"
    );
    assert!(keys.contains(">Accept sign-in email</button>"), "{keys}");
    assert!(keys.contains(">Remove</button>"), "{keys}");

    let (status, headers, body) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "accept_email"),
            ("email", "alice@work.example"),
            ("admin", "false"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        "/web/keys?done=email_accepted"
    );
    let (status, headers, body) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "decline_email"),
            ("email", "someone@example.net"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        "/web/keys?done=email_removed"
    );
    let (_, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(
        !keys.contains("<h2>Sign-in emails to accept</h2>"),
        "{keys}"
    );
    let (status, token, body) = sign_in_google(&rig, "sub-2", "alice@work.example").await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert!(token.is_some(), "the accepted email opens a session");
    let (status, token, _) = sign_in_google(&rig, "sub-3", "someone@example.net").await;
    assert_ne!(status, StatusCode::SEE_OTHER);
    assert_eq!(token, None, "the removed email is on no list");
}

/// In a shared workspace the person's own reviewer key holds no grant, so
/// the console judges there with the tenant key an admin linked to them:
/// the Keys page links it to Review, Review lists what that key may judge,
/// and the verdict is that key's, journalled under its id; a key not
/// linked to the person is no page (ADR-0085 d16, amended 2026-11-17).
#[tokio::test]
async fn the_console_judges_in_a_shared_workspace_with_the_linked_key() {
    let rig = rig().await;
    shared_team(&rig).await;
    let bot = team_key(
        &rig,
        "team-bot",
        Role::Submitter,
        UsedBy::Agent,
        &[Role::Submitter],
    )
    .await;
    let memory = agent_writes_in(&rig, &bot.value, "The team ships on Fridays.", "team").await;
    let linked = team_key(
        &rig,
        "alice-team",
        Role::Reviewer,
        UsedBy::Person,
        &[Role::Reviewer, Role::Reader],
    )
    .await;
    // Alice's first sign-in makes her root, which the link names; another
    // person, a console admin, links her key on the admin Keys page.
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    rig.allow("ops@example.com", "ops", true).await;
    let ops = signed_in(&rig, "sub-ops", "ops@example.com").await;
    let (status, headers, body) = act(
        &rig,
        &ops,
        "/web/admin/keys",
        &[
            ("action", "link_key"),
            ("key", "alice-team"),
            ("user", "alice"),
            ("reason", "alice works with this key"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        "/web/admin/keys?user=alice&done=linked"
    );
    let as_linked = format!("/web/todo?as={}", linked.key_id);

    // The link waits on alice: her Keys page offers it to accept, and lists
    // it under no linked key until she does (the operator's decision of
    // 2026-11-19).
    let (status, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{keys}");
    assert!(keys.contains("<h2>Links to accept</h2>"), "{keys}");
    assert!(keys.contains(">Accept link</button>"), "{keys}");
    assert!(!keys.contains("<h2>Linked keys</h2>"), "{keys}");
    let (status, _, page) = send(&rig.router, get(&as_linked, Some(&cookie))).await;
    assert_ne!(
        status,
        StatusCode::OK,
        "a pending link acts for no one: {page}"
    );
    let (status, headers, body) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[("action", "accept_link"), ("key", "alice-team")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        "/web/keys?done=accepted"
    );

    let (status, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{keys}");
    assert!(keys.contains("<h2>Linked keys</h2>"), "{keys}");
    assert!(keys.contains("alice-team"), "{keys}");
    assert!(
        keys.contains(&format!(
            r#"href="/web/todo?as={}">Judge as this key in To do</a>"#,
            linked.key_id
        )),
        "{keys}"
    );

    // As alice-review the row is not hers to judge: her own To do lists
    // nothing and her Next opens nothing.
    let (_, _, own) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    assert!(!own.contains(&memory.to_string()), "{own}");
    assert!(own.contains("Nothing waits for this account."), "{own}");
    assert_eq!(next_item(&rig, &cookie).await, None);

    let (status, _, page) = send(&rig.router, get(&as_linked, Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("An admin linked <code>alice-team</code> to this account"),
        "{page}"
    );
    let judge_link = format!(
        r#"href="/web/todo/judge?memory={memory}&amp;as={}">Judge</a>"#,
        linked.key_id
    );
    assert!(page.contains(&judge_link), "{page}");
    // Next on that page acts as the linked key too, and opens its item.
    assert!(
        page.contains(&format!(
            r#"<input type="hidden" name="as" value="{}">"#,
            linked.key_id
        )),
        "{page}"
    );
    let (status, headers, body) = act(
        &rig,
        &cookie,
        "/web/todo",
        &[("action", "next"), ("as", &linked.key_id.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        format!("/web/todo/judge?memory={memory}&as={}", linked.key_id)
    );

    let (status, _, item) = send(
        &rig.router,
        get(
            &format!("/web/todo/judge?memory={memory}&as={}", linked.key_id),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{item}");
    assert!(item.contains("The team ships on Fridays."), "{item}");
    assert!(
        item.contains("Recorded under <code>alice-team</code>"),
        "{item}"
    );
    assert_eq!(field_of(&item, "as"), linked.key_id.to_string());
    let head = rig.head().await;
    let (status, headers, body) = act(
        &rig,
        &cookie,
        "/web/todo/judge",
        &[
            ("action", "judge"),
            ("artifact_id", &memory.to_string()),
            ("verdict", "approve"),
            ("reason", ""),
            ("nonce", &field_of(&item, "nonce")),
            ("as", &linked.key_id.to_string()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        format!(
            "/web/todo?done=verdict&version=1&state=promoted&as={}",
            linked.key_id
        )
    );
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    let verdicts: Vec<_> = events
        .iter()
        .filter(|e| e.envelope.event_type == "review.approve")
        .collect();
    assert_eq!(verdicts.len(), 1, "one verdict");
    assert_eq!(verdicts[0].envelope.api_key_id, Some(linked.key_id));

    // A key not linked to alice is no page, and nothing is judged with it.
    let (status, _, refused) = send(
        &rig.router,
        get(&format!("/web/todo?as={}", bot.key_id), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    // A judge posted with an unlinked key's id, or a forged one, records
    // nothing: the lead refuses the acting key.
    let other = agent_writes_in(&rig, &bot.value, "The team deploys on Mondays.", "team").await;
    let head = rig.head().await;
    for forged in [
        bot.key_id.to_string(),
        uuid::Uuid::from_u128(0xF0F0).to_string(),
    ] {
        let (status, headers, body) = act(
            &rig,
            &cookie,
            "/web/todo/judge",
            &[
                ("action", "judge"),
                ("artifact_id", &other.to_string()),
                ("verdict", "approve"),
                ("reason", ""),
                ("as", &forged),
            ],
        )
        .await;
        let location = headers
            .get(header::LOCATION)
            .and_then(|l| l.to_str().ok())
            .unwrap_or_default();
        assert!(
            !location.contains("done=verdict"),
            "{status} {location} {body}"
        );
    }
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .all(|e| !e.envelope.event_type.starts_with("review.")),
        "no verdict recorded"
    );
}

/// A held row a key of the signed-in person wrote offers Withdraw under
/// Memories, Submitted, and never under Approved; its confirmation withdraws it as the person's web key,
/// and the version is kept as withdrawn (ADR-0083 d3, d13; #410).
#[tokio::test]
async fn a_person_withdraws_their_agents_held_write_from_submitted_after_a_confirmation() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "contributor"),
        ],
    )
    .await;
    let value = value_of(&page);
    let id = agent_writes(&rig, &value, "a note written by mistake").await;
    let link = format!("/web/memory?id={id}&amp;act=withdraw");
    let (status, _, approved) = send(&rig.router, get("/web/memories", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!approved.contains(&link), "{approved}");
    let (status, _, list) = send(
        &rig.router,
        get("/web/memories?view=submitted", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list.contains(&link), "{list}");
    let (status, _, confirm) = send(
        &rig.router,
        get(&format!("/web/memory?id={id}&act=withdraw"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(confirm.contains("<h2>Withdraw it</h2>"), "{confirm}");
    let nonce = field_of(&confirm, "nonce");
    let (status, _, done) = act_and_follow(
        &rig,
        &cookie,
        "/web/memory",
        &[
            ("action", "withdraw"),
            ("memory", &id.to_string()),
            ("reason", "written by mistake"),
            ("nonce", &nonce),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(
        done.contains("Withdrawn: kept with its text, in no review queue and never served."),
        "{done}"
    );
    let row = rig
        .app
        .get_memory(value.as_str(), id)
        .await
        .expect("kept, and read by its author");
    assert_eq!(row.memory.status, oml_app::domain::MemoryStatus::Withdrawn);
    // Withdrawn, the row offers Withdraw no more.
    let (_, _, list) = send(
        &rig.router,
        get("/web/memories?view=submitted", Some(&cookie)),
    )
    .await;
    assert!(!list.contains(&link), "{list}");
}

// -- the Memories cockpit (ADR-0083 d1, d3, d4, d5, d6, d7; #411, #413) -----

/// A person's agent writes, in the person's `proj` workspace, a row the
/// helper approves and then edits by a held successor, and a held row of
/// its own; `proj` is put in progress, where every author's held row is
/// served to its readers. Answers the cookie, the agent's key and the
/// helper's.
async fn in_progress_with_held_rows(rig: &Rig) -> (String, String, String) {
    let (alice, agent) = person_with_agent(rig, "sub-1", EMAIL).await;
    // Issued first, as the other tests issue it: the test app's ids are
    // its own sequence beside the lead's.
    let helper = reviewer_in_alices_root(rig).await;
    let (status, _, page) = act(
        rig,
        &alice,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    let served = agent_writes_in(rig, &agent, "approved inside proj", "alice.proj").await;
    agent_writes_in(rig, &agent, "own held inside proj", "alice.proj").await;
    put_in_progress(rig, &alice, "alice.proj").await;
    reviewer_holds_a_successor(rig, &helper.value, served, "helper held inside proj").await;
    (alice, agent, helper.value)
}

/// Approved reads the promoted tips alone, and its search sends
/// `include_unreviewed: false`, so no held row reaches it: not the
/// person's own and not, inside an in-progress workspace, another
/// author's (ADR-0083 d1).
#[tokio::test]
async fn approved_shows_no_held_row_even_inside_an_in_progress_workspace() {
    let rig = rig().await;
    let (alice, _, _) = in_progress_with_held_rows(&rig).await;
    for path in [
        "/web/memories",
        "/web/memories?workspace=alice.proj",
        "/web/memories?workspace=alice.proj&q=inside",
        "/web/memories?q=inside",
        "/web/data/memories?workspace=alice.proj",
        "/web/data/memories?workspace=alice.proj&q=inside",
    ] {
        let (status, _, page) = send(&rig.router, get(path, Some(&alice))).await;
        assert_eq!(status, StatusCode::OK, "{path}: {page}");
        assert!(page.contains("approved inside proj"), "{path}: {page}");
        for held in ["own held inside proj", "helper held inside proj"] {
            assert!(!page.contains(held), "{path} shows {held}");
        }
    }
}

/// Submitted lists the person's own rows, held, rejected and withdrawn,
/// each led by one plain sentence, a held one offering Withdraw; it has no
/// search box, and another person's row is never in it (ADR-0083 d3, d4,
/// d13).
#[tokio::test]
async fn submitted_leads_each_of_the_persons_rows_with_a_sentence_and_offers_withdraw_on_a_held_row()
 {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", false).await;
    let (alice, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let (_, bob_agent) = person_with_agent(&rig, "sub-bob", "bob@example.com").await;
    agent_writes(&rig, &bob_agent, "bob held secret").await;
    let helper = reviewer_in_alices_root(&rig).await;
    let held = agent_writes(&rig, &agent, "alice waits for review").await;
    let rejected = agent_writes(&rig, &agent, "alice was refused").await;
    rig.app
        .post_verdict(
            helper.value.as_str(),
            rejected,
            oml_app::types::Verdict::Reject,
            Some("no source"),
        )
        .await
        .expect("the helper rejects");
    let withdrawn = agent_writes(&rig, &agent, "alice took this back").await;
    rig.app
        .withdraw(
            agent.as_str(),
            &oml_app::types::WithdrawRequest {
                idempotency_key: "w-alice".to_owned(),
                memory_id: withdrawn,
                reason: None,
            },
        )
        .await
        .expect("the author withdraws");
    let (status, _, page) = send(
        &rig.router,
        get("/web/memories?view=submitted", Some(&alice)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("<title>Submitted · Memories · OML</title>"),
        "{page}"
    );
    assert!(!page.contains("bob held secret"), "{page}");
    assert!(!page.contains(r#"name="q""#), "no search box: {page}");
    let lead = |id: uuid::Uuid| {
        between(
            card_of(&page, &id.to_string()),
            r#"<p class="mc-lead">"#,
            "</p>",
        )
        .to_owned()
    };
    assert_eq!(lead(held), "Waiting for review.");
    assert_eq!(
        lead(rejected),
        "Rejected. The text is kept and never served."
    );
    assert_eq!(
        lead(withdrawn),
        "Withdrawn. The text is kept and never served."
    );
    let withdraw = |id: uuid::Uuid| format!("/web/memory?id={id}&amp;act=withdraw");
    assert!(page.contains(&withdraw(held)), "{page}");
    assert!(!page.contains(&withdraw(rejected)), "{page}");
    assert!(!page.contains(&withdraw(withdrawn)), "{page}");
    // The State filter narrows to one of the four.
    let (status, _, only) = send(
        &rig.router,
        get("/web/memories?view=submitted&state=rejected", Some(&alice)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{only}");
    assert!(only.contains("alice was refused"), "{only}");
    assert!(!only.contains("alice waits for review"), "{only}");
    // Approved lists none of them and offers no Withdraw.
    let (_, _, approved) = send(&rig.router, get("/web/memories", Some(&alice))).await;
    for text in [
        "alice waits for review",
        "alice was refused",
        "alice took this back",
    ] {
        assert!(!approved.contains(text), "{approved}");
    }
    assert!(!approved.contains("act=withdraw"), "{approved}");
}

/// The views are two links in `nav.oml-views`, the current one
/// `aria-current="page"`, each keeping the picked workspace and the query
/// (ADR-0083 d4, d6); a view the page does not draw is refused naming the
/// field.
#[tokio::test]
async fn the_memories_views_are_two_links_that_keep_the_workspace_and_the_query() {
    let rig = rig().await;
    let (alice, _) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let (_, _, approved) = send(
        &rig.router,
        get("/web/memories?workspace=alice&q=deploy", Some(&alice)),
    )
    .await;
    assert!(
        approved.contains(r#"<nav class="oml-views" aria-label="Memories views"><a href="/web/memories?q=deploy&amp;workspace=alice" aria-current="page">Approved</a><a href="/web/memories?view=submitted&amp;q=deploy&amp;workspace=alice">Submitted</a></nav>"#),
        "{approved}"
    );
    let (_, _, submitted) = send(
        &rig.router,
        get(
            "/web/memories?view=submitted&workspace=alice&q=deploy",
            Some(&alice),
        ),
    )
    .await;
    assert!(
        submitted.contains(r#"<nav class="oml-views" aria-label="Memories views"><a href="/web/memories?q=deploy&amp;workspace=alice">Approved</a><a href="/web/memories?view=submitted&amp;q=deploy&amp;workspace=alice" aria-current="page">Submitted</a></nav>"#),
        "{submitted}"
    );
    // The query rides along unseen, so going back to Approved keeps it.
    assert!(
        submitted.contains(r#"<input type="hidden" name="q" value="deploy">"#),
        "{submitted}"
    );
    let (status, _, refused) = send(
        &rig.router,
        get("/web/memories?view=everything", Some(&alice)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert!(refused.contains("view"), "{refused}");
}

/// A held row, a rejected one and a served one, and the admin `boss`
/// signed in: the cookie, the held id and the rejected id.
async fn admin_with_held_and_rejected(rig: &Rig) -> (String, uuid::Uuid, uuid::Uuid) {
    rig.allow("boss@example.com", "boss", true).await;
    let (_, agent) = person_with_agent(rig, "sub-1", EMAIL).await;
    let boss = signed_in(rig, "sub-boss", "boss@example.com").await;
    let helper = reviewer_in_alices_root(rig).await;
    let served = agent_writes(rig, &agent, "served to readers").await;
    rig.app
        .post_verdict(
            helper.value.as_str(),
            served,
            oml_app::types::Verdict::Approve,
            None,
        )
        .await
        .expect("the helper approves");
    let rejected = agent_writes(rig, &agent, "refused by review").await;
    rig.app
        .post_verdict(
            helper.value.as_str(),
            rejected,
            oml_app::types::Verdict::Reject,
            Some("no source"),
        )
        .await
        .expect("the helper rejects");
    let held = agent_writes(rig, &agent, "waiting for a verdict").await;
    (boss, held, rejected)
}

/// No admin page draws Judge, Next, Review next, a pick box or a verdict
/// form, and neither the old admin Review path nor the admin To do runs an
/// act: an admin key gives no verdict, and bulk review is the CLI's
/// (ADR-0083 d5, d7, d14).
#[tokio::test]
async fn no_admin_page_draws_a_verdict_form() {
    let rig = rig().await;
    let (boss, held, rejected) = admin_with_held_and_rejected(&rig).await;
    let mut paths: Vec<String> = oml_page::web::ADMIN_TABS
        .iter()
        .map(|(path, _)| (*path).to_owned())
        .collect();
    paths.extend([
        "/web/admin/memories?view=held".to_owned(),
        "/web/admin/memories?view=rejected".to_owned(),
        "/web/admin/todo".to_owned(),
        format!("/web/admin/memory?id={held}"),
        format!("/web/admin/memory?id={rejected}"),
    ]);
    for path in &paths {
        let (status, _, page) = send(&rig.router, get(path, Some(&boss))).await;
        assert_eq!(status, StatusCode::OK, "{path}: {page}");
        for drawn in [
            r#"name="verdict""#,
            r#"name="pick_"#,
            r#"value="review_next""#,
            r#"value="bulk_confirm""#,
            r#"value="bulk_verdict""#,
            r#"value="judge""#,
            r#"value="next""#,
            "Review next",
            ">Judge</",
        ] {
            assert!(!page.contains(drawn), "{path} draws {drawn}: {page}");
        }
    }
    for (path, action) in ["/web/admin/review", "/web/admin/todo"]
        .into_iter()
        .flat_map(|path| {
            [
                "review_next",
                "bulk_confirm",
                "bulk_verdict",
                "verdict",
                "judge",
                "next",
            ]
            .map(|action| (path, action))
        })
    {
        let (status, _, page) = act(
            &rig,
            &boss,
            path,
            &[
                ("action", action),
                ("verdict", "approve"),
                ("artifact_ids", &held.to_string()),
                ("pick_000", &held.to_string()),
            ],
        )
        .await;
        assert_ne!(status, StatusCode::SEE_OTHER, "{path} {action}: {page}");
    }
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    assert_eq!(
        events
            .iter()
            .filter(|e| e.envelope.event_type.starts_with("review.")
                && e.envelope.payload.to_string().contains(&held.to_string()))
            .count(),
        0,
        "no verdict on the held row"
    );
}

/// The admin Held view lists the tenant's held rows under the "Held per
/// workspace" table, each workspace a link that filters them, carries its
/// count on its tab, says the admin key never judges, and links each held
/// row to Judge on the Personal side as the person's reviewer key
/// (ADR-0083 d5).
#[tokio::test]
async fn admin_held_view_links_judge_on_the_personal_side() {
    let rig = rig().await;
    let (boss, held, rejected) = admin_with_held_and_rejected(&rig).await;
    let (status, _, page) = send(
        &rig.router,
        get("/web/admin/memories?view=held", Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("<title>Admin · Held · Memories · OML</title>"),
        "{page}"
    );
    assert!(
        page.contains(r#"<nav class="oml-views" aria-label="Memories views"><a href="/web/admin/memories">Approved</a><a href="/web/admin/memories?view=held" aria-current="page">Held <span class="n">1</span></a><a href="/web/admin/memories?view=rejected">Rejected</a></nav>"#),
        "{page}"
    );
    assert!(
        page.contains("This key never judges; verdicts are given on the Personal side, as <code>boss-review</code>."),
        "{page}"
    );
    assert!(page.contains("<h2>Held per workspace</h2>"), "{page}");
    assert!(
        page.contains(r#"href="/web/admin/memories?view=held&amp;workspace=alice">alice</a>"#),
        "{page}"
    );
    assert!(page.contains("waiting for a verdict"), "{page}");
    assert!(!page.contains("refused by review"), "{page}");
    assert!(!page.contains("served to readers"), "{page}");
    assert!(
        page.contains(&format!(
            r#"<a href="/web/todo/judge?memory={held}">Judge as <code>boss-review</code></a>"#
        )),
        "{page}"
    );
    // The workspace link filters the rows, the view kept.
    let (status, _, filtered) = send(
        &rig.router,
        get("/web/admin/memories?view=held&workspace=alice", Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{filtered}");
    assert!(filtered.contains("waiting for a verdict"), "{filtered}");
    // Approved and Rejected hold their own rows, with no Judge link and no
    // count on the tab.
    let (_, _, approved) = send(&rig.router, get("/web/admin/memories", Some(&boss))).await;
    assert!(approved.contains("served to readers"), "{approved}");
    assert!(!approved.contains("waiting for a verdict"), "{approved}");
    assert!(!approved.contains("/web/todo/judge"), "{approved}");
    assert!(
        approved.contains(r#"<a href="/web/admin/memories" aria-current="page">Approved</a><a href="/web/admin/memories?view=held">Held</a>"#),
        "{approved}"
    );
    let (_, _, refused) = send(
        &rig.router,
        get("/web/admin/memories?view=rejected", Some(&boss)),
    )
    .await;
    assert!(refused.contains(&rejected.to_string()), "{refused}");
    assert!(!refused.contains("waiting for a verdict"), "{refused}");
    assert!(!refused.contains("/web/todo/judge"), "{refused}");
}

// -- the workspace page and its builder (ADR-0082 d1-d6; ADR-0083 d12) -------

/// An agent's skill `name` in its person's root, held.
async fn agent_skill(rig: &Rig, value: &str, name: &str, body: &str) -> uuid::Uuid {
    rig.app
        .submit_memory(
            value,
            &oml_app::types::SubmitRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                idempotency_key: format!("s-{name}"),
                claim_key: None,
                verb: None,
                kind: oml_app::domain::MemoryKind::Skill,
                body: body.to_owned(),
                workspace: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: Some(oml_app::types::KindFields::Skill {
                    name: name.to_owned(),
                    description: "Walk the release steps.".to_owned(),
                }),
                valid_from: None,
                valid_until: None,
                format: Some("text/markdown".to_owned()),
                title: None,
            },
        )
        .await
        .expect("the agent proposes a skill")
        .receipt
        .memory_id
}

/// An approved skill `release-checklist` in alice's root, written by her
/// agent and approved by her review key: the cookie and its id.
async fn approved_skill(rig: &Rig) -> (String, uuid::Uuid) {
    let cookie = signed_in(rig, "sub-1", EMAIL).await;
    let value = laptop_key(rig, &cookie).await;
    let id = agent_skill(
        rig,
        &value,
        "release-checklist",
        "Tag, build, publish notes.",
    )
    .await;
    let (judged, (status, _, page)) = judge_next(rig, &cookie, "approve", None).await;
    assert_eq!(judged, id.to_string());
    assert_eq!(status, StatusCode::OK, "{page}");
    (cookie, id)
}

/// Every builder page answers without script and says nothing in the
/// second person, and no control saves, publishes or deletes (ADR-0082 d1;
/// the operator's rule of 2026-11-13).
fn assert_builder_copy(page: &str) {
    assert!(!page.contains("<script"), "no script: {page}");
    let lower = page.to_lowercase();
    for word in [" you ", " you.", " your ", " my "] {
        assert!(!lower.contains(word), "{word:?}: {page}");
    }
    for word in [">Save<", ">Publish<", ">Delete<"] {
        assert!(!page.contains(word), "{word}: {page}");
    }
}

/// The skills of a workspace are listed on its page with Edit and Retire on
/// the approved tip, the kinds counted by approved claims, the one New and
/// who approves under the policy (ADR-0082 d1, d10).
#[tokio::test]
async fn the_workspace_page_lists_skills_with_edit_and_retire_on_an_approved_tip() {
    let rig = rig().await;
    let (cookie, id) = approved_skill(&rig).await;
    let (status, _, page) =
        send(&rig.router, get("/web/workspace?kind=skill", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_builder_copy(&page);
    assert!(
        page.contains("<title>alice · Workspaces · OML</title>"),
        "{page}"
    );
    assert!(
        page.contains(r#"<nav class="crumbs" aria-label="Breadcrumb">Personal / <a href="/web/workspaces">Workspaces</a> / alice</nav><h1 class="mono">alice</h1>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<nav class="oml-views" aria-label="Approved contents by kind"><a href="/web/memories?workspace=alice">Memories <span class="n">1</span></a><a href="/web/workspace?workspace=alice&amp;kind=skill" aria-current="page">Skills <span class="n">1</span></a><a href="/web/workspace?workspace=alice&amp;kind=guardrail">Guardrails <span class="n">0</span></a><a href="/web/workspace?workspace=alice&amp;kind=document">Documents <span class="n">0</span></a><a href="/web/workspace?workspace=alice&amp;kind=brief">Brief <span class="n">0</span></a></nav>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<a class="button primary" href="/web/workspace/new?workspace=alice&amp;kind=skill">New skill</a>"#),
        "{page}"
    );
    assert!(
        page.contains("Proposed under <code>alice-web</code>. Held until approved; nothing new is served before then. This workspace needs 1 approval: in this root <code>alice-review</code> may give it, marked same user."),
        "{page}"
    );
    assert!(
        page.contains(&format!(
            r#"<a class="button" href="/web/workspace/edit?memory={id}">Edit</a> <a class="button" href="/web/workspace/retire?memory={id}">Retire</a>"#
        )),
        "{page}"
    );
    assert!(
        page.contains("<code>/mcp__&lt;server&gt;__release-checklist</code>"),
        "{page}"
    );
    // The Workspaces view's row leads here (d1).
    let (_, _, workspaces) = send(&rig.router, get("/web/workspaces", Some(&cookie))).await;
    assert!(
        workspaces.contains(r#"<a href="/web/workspace?workspace=alice">alice</a>"#),
        "{workspaces}"
    );
}

/// The values a form carries: every field the form's page drew as an
/// input, with the hidden fields, as a browser sends them.
fn form_values(page: &str) -> Vec<(String, String)> {
    let form = &page[page.find(r#"<form class="oml-form""#).expect("the form")..];
    let form = &form[..form.find("</form>").expect("its end")];
    let mut out = Vec::new();
    for marker in [r#"<input type="hidden" name=""#, r#"<input name=""#] {
        let mut at = 0;
        while let Some(found) = form[at..].find(marker) {
            let start = at + found + marker.len();
            let name = &form[start..start + form[start..].find('"').unwrap()];
            let value_at = start + form[start..].find(r#"value=""#).unwrap() + 7;
            let value = &form[value_at..value_at + form[value_at..].find('"').unwrap()];
            if name != "csrf_token" {
                out.push((name.to_owned(), value.replace("&amp;", "&")));
            }
            at = value_at;
        }
    }
    let marker = r#"<textarea name=""#;
    let mut at = 0;
    while let Some(found) = form[at..].find(marker) {
        let start = at + found + marker.len();
        let name = &form[start..start + form[start..].find('"').unwrap()];
        let open = start + form[start..].find('>').unwrap() + 1;
        let close = open + form[open..].find("</textarea>").unwrap();
        // An HTML parser drops the one line break that opens a textarea.
        let value = &form[open..close];
        let value = value.strip_prefix('\n').unwrap_or(value);
        out.push((name.to_owned(), value.replace("&amp;", "&")));
        at = close;
    }
    out
}

/// A form post of `values`, with `step` and the fields of `extra` set over
/// them.
async fn post_form(
    rig: &Rig,
    cookie: &str,
    path: &str,
    values: &[(String, String)],
    extra: &[(&str, &str)],
) -> (StatusCode, HeaderMap, String) {
    let mut fields: Vec<(&str, &str)> = values
        .iter()
        .filter(|(n, _)| !extra.iter().any(|(e, _)| e == n))
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    fields.extend_from_slice(extra);
    act(rig, cookie, path, &fields).await
}

/// The held skills alice's keys wrote in her root.
async fn held_skills(rig: &Rig) -> usize {
    rig.store
        .events_range(rig.boot.tenant_id, 1, 100_000)
        .await
        .expect("the journal")
        .iter()
        .filter(|e| {
            format!("{e:?}").contains("submission.remember") && format!("{e:?}").contains("triage")
        })
        .count()
}

/// Before a preview Preview is the one primary and Propose is not drawn;
/// the page a preview answered draws what a session will receive and
/// Propose; Propose with a changed field redraws the preview instead of
/// writing; and with the previewed values it writes once and answers `303`
/// to the workspace page (ADR-0082 d5).
#[tokio::test]
async fn propose_is_drawn_only_after_a_preview_of_the_same_values() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, first) = send(
        &rig.router,
        get("/web/workspace/new?kind=skill", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_builder_copy(&first);
    assert!(first.contains("<h1>New skill</h1>"), "{first}");
    assert!(
        first.contains(r#"<nav class="crumbs" aria-label="Breadcrumb">Personal / <a href="/web/workspaces">Workspaces</a> / <a href="/web/workspace?workspace=alice&amp;kind=skill">alice</a> / New skill</nav>"#),
        "{first}"
    );
    assert!(
        first.contains(r#"<div class="acts"><button type="submit" name="step" value="preview" class="primary">Preview</button></div>"#),
        "{first}"
    );
    assert!(!first.contains("Propose skill"), "{first}");
    let mut values = form_values(&first);
    for (name, value) in &mut values {
        match name.as_str() {
            "name" => *value = "triage".to_owned(),
            "description" => *value = "Sort an incident.".to_owned(),
            "body" => *value = "Read the alert, then page the owner.".to_owned(),
            _ => {}
        }
    }
    let (status, _, previewed) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &values,
        &[("step", "preview")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{previewed}");
    assert_builder_copy(&previewed);
    assert!(
        previewed.contains(r#"<h2 id="preview">Preview</h2>"#),
        "{previewed}"
    );
    assert!(
        previewed
            .contains("A session receives the command <code>/mcp__&lt;server&gt;__triage</code>."),
        "{previewed}"
    );
    assert!(
        previewed.contains("<pre>- triage: Sort an incident.</pre>"),
        "{previewed}"
    );
    assert!(
        previewed.contains(r#"<button type="submit" name="step" value="preview">Preview</button><button type="submit" name="step" value="propose" class="primary">Propose skill</button>"#),
        "{previewed}"
    );
    assert_eq!(held_skills(&rig).await, 0, "a preview writes nothing");
    // A page a post draws carries the badge's number as a GET does (the
    // review of PR #494, 11).
    assert!(previewed.contains(">To do · 0</a>"), "{previewed}");
    // A field changed since the preview: the page previews again.
    let mut changed = form_values(&previewed);
    for (name, value) in &mut changed {
        if name == "description" {
            *value = "Sort an incident by severity.".to_owned();
        }
    }
    let (status, _, again) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &changed,
        &[("step", "propose")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert!(
        again.contains("<pre>- triage: Sort an incident by severity.</pre>"),
        "{again}"
    );
    assert_eq!(
        held_skills(&rig).await,
        0,
        "a changed field is previewed, not written"
    );
    let (status, headers, _) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &form_values(&again),
        &[("step", "propose")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert!(
        location.starts_with("/web/workspace?workspace=alice&kind=skill&done=proposed&held="),
        "{location}"
    );
    assert_eq!(held_skills(&rig).await, 1);
    let (_, _, listed) = send(&rig.router, get(&location, Some(&cookie))).await;
    // The notice names the version held (the review of PR #494, 16).
    assert!(
        listed.contains(r#"<p class="notice" role="status"><b>triage</b> v1. Proposed: the new version is held until it is approved"#),
        "{listed}"
    );
    assert!(
        listed.contains("New skill</td>") || listed.contains(" New skill"),
        "{listed}"
    );
}

/// The same Propose sent twice carries the form's nonce both times, so the
/// API replays the first and one version is captured (ADR-0082 d2).
#[tokio::test]
async fn a_double_submit_of_propose_captures_one_version() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, first) = send(
        &rig.router,
        get("/web/workspace/new?kind=skill", Some(&cookie)),
    )
    .await;
    let mut values = form_values(&first);
    for (name, value) in &mut values {
        match name.as_str() {
            "name" => *value = "triage".to_owned(),
            "description" => *value = "Sort an incident.".to_owned(),
            "body" => *value = "Read the alert.".to_owned(),
            _ => {}
        }
    }
    let (_, _, previewed) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &values,
        &[("step", "preview")],
    )
    .await;
    let ready = form_values(&previewed);
    for _ in 0..2 {
        let (status, _, body) = post_form(
            &rig,
            &cookie,
            "/web/workspace/new",
            &ready,
            &[("step", "propose")],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    }
    assert_eq!(held_skills(&rig).await, 1, "the second send replayed");
}

/// A refusal keeps the form with every value typed and draws the error
/// beside its field, its sentence first and its code last; Propose is not
/// offered (ADR-0082 d5).
#[tokio::test]
async fn a_refused_preview_keeps_every_value_and_names_the_error_beside_its_field() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, first) = send(
        &rig.router,
        get("/web/workspace/new?kind=skill", Some(&cookie)),
    )
    .await;
    let mut values = form_values(&first);
    for (name, value) in &mut values {
        match name.as_str() {
            "name" => *value = "Not A Name".to_owned(),
            "description" => *value = "Sort an incident.".to_owned(),
            "body" => *value = "Read the alert.".to_owned(),
            _ => {}
        }
    }
    let (status, _, page) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &values,
        &[("step", "preview")],
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{page}");
    assert_builder_copy(&page);
    assert!(page.contains(r#"<input name="name" value="Not A Name" required aria-describedby="err-name" aria-invalid="true">"#), "{page}");
    assert!(
        page.contains(r#"<input name="description" value="Sort an incident." required>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<p class="field-error" id="err-name" role="alert">"#),
        "{page}"
    );
    assert!(
        page.contains("<code>request.validation</code></p>"),
        "{page}"
    );
    assert!(!page.contains("Propose skill"), "{page}");
}

/// An edit is previewed with its change against the served tip, name,
/// description and body each in its own block, and proposes the next
/// version; once it is held, the page offers no second edit and links the
/// pending one (ADR-0082 d2, d6).
#[tokio::test]
async fn an_edit_previews_its_change_and_then_waits_for_review() {
    let rig = rig().await;
    let (cookie, id) = approved_skill(&rig).await;
    let (status, _, form) = send(
        &rig.router,
        get(&format!("/web/workspace/edit?memory={id}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{form}");
    assert_builder_copy(&form);
    assert!(form.contains("<h1>Edit release-checklist</h1>"), "{form}");
    assert!(
        form.contains("<code>release-checklist</code> <span class=\"dim\">fixed for every version"),
        "{form}"
    );
    let mut values = form_values(&form);
    for (name, value) in &mut values {
        if name == "body" {
            *value = "Tag, build, sign, publish notes.".to_owned();
        }
    }
    let (status, _, previewed) = post_form(
        &rig,
        &cookie,
        "/web/workspace/edit",
        &values,
        &[("step", "preview")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{previewed}");
    assert!(previewed.contains("Change against v1 <span class=\"n\">baseline: v1, the served tip · 0 words removed, 1 word added</span>"), "{previewed}");
    assert!(
        previewed.contains(r#"<p class="label">Name <span class="n">unchanged</span></p>"#),
        "{previewed}"
    );
    assert!(previewed.contains("<ins>sign,</ins>"), "{previewed}");
    assert!(previewed.contains(">Propose v2</button>"), "{previewed}");
    let (status, _, _) = post_form(
        &rig,
        &cookie,
        "/web/workspace/edit",
        &form_values(&previewed),
        &[("step", "propose")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send(&rig.router, get("/web/workspace?kind=skill", Some(&cookie))).await;
    assert!(
        page.contains("in force · <a href=\"/web/memory?id="),
        "{page}"
    );
    assert!(page.contains("edit v2 waits for review</a>"), "{page}");
    assert!(
        !page.contains(&format!("/web/workspace/edit?memory={id}")),
        "{page}"
    );
    assert!(page.contains("Edit of v1"), "{page}");
    // The person's own held version offers Withdraw, with its confirmation.
    assert!(page.contains("&amp;act=withdraw\">Withdraw</a>"), "{page}");
    // The edit form itself links the pending version instead of a second edit.
    let (_, _, again) = send(
        &rig.router,
        get(&format!("/web/workspace/edit?memory={id}"), Some(&cookie)),
    )
    .await;
    assert!(
        again.contains("edit v2 waits for review</a>: a second proposal would race it"),
        "{again}"
    );
    assert!(!again.contains(r#"<form class="oml-form""#), "{again}");
    // The stale form posted again (a second tab, or Back): the person's own
    // held v2 is no tip to rebase onto, so the post draws the same waiting
    // page and no rebase (the review of PR #494, C-B4).
    for step in ["preview", "rebase"] {
        let (status, _, stale) = post_form(
            &rig,
            &cookie,
            "/web/workspace/edit",
            &values,
            &[("step", step)],
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{step}: {stale}");
        assert!(
            stale.contains("edit v2 waits for review</a>: a second proposal would race it"),
            "{step}: {stale}"
        );
        assert!(
            !stale.contains("is now the latest version"),
            "{step}: {stale}"
        );
        assert!(
            !stale.contains(r#"<form class="oml-form""#),
            "{step}: {stale}"
        );
    }
}

/// When the tip moved after the form was drawn, nothing is proposed, the
/// typed text is kept, and the one primary edits from the new tip with it,
/// previewing the change against that tip (ADR-0082 d5).
#[tokio::test]
async fn a_stale_edit_offers_one_primary_that_edits_from_the_new_tip_with_the_typed_text() {
    let rig = rig().await;
    let (cookie, id) = approved_skill(&rig).await;
    let (_, _, stale) = send(
        &rig.router,
        get(&format!("/web/workspace/edit?memory={id}"), Some(&cookie)),
    )
    .await;
    let mut typed = form_values(&stale);
    for (name, value) in &mut typed {
        if name == "body" {
            *value = "Tag, build, publish notes, announce.".to_owned();
        }
    }
    // Another edit lands and is approved first.
    let (_, _, other) = send(
        &rig.router,
        get(&format!("/web/workspace/edit?memory={id}"), Some(&cookie)),
    )
    .await;
    let mut values = form_values(&other);
    for (name, value) in &mut values {
        if name == "body" {
            *value = "Tag, build, sign, publish notes.".to_owned();
        }
    }
    let (_, _, previewed) = post_form(
        &rig,
        &cookie,
        "/web/workspace/edit",
        &values,
        &[("step", "preview")],
    )
    .await;
    let (status, _, _) = post_form(
        &rig,
        &cookie,
        "/web/workspace/edit",
        &form_values(&previewed),
        &[("step", "propose")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, (status, _, _)) = judge_next(&rig, &cookie, "approve", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, conflict) = post_form(
        &rig,
        &cookie,
        "/web/workspace/edit",
        &typed,
        &[("step", "preview")],
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_builder_copy(&conflict);
    assert!(conflict.contains("v2 is now the latest version; this edit was made against v1. Nothing was proposed; the text typed here is kept. <code>submission.version_conflict</code>"), "{conflict}");
    assert!(conflict.contains(r#"<button type="submit" name="step" value="rebase" class="primary">Edit from v2 with this text</button>"#), "{conflict}");
    assert_eq!(
        conflict.matches("class=\"primary\"").count(),
        1,
        "{conflict}"
    );
    let rebase: Vec<(String, String)> = {
        let form = &conflict[conflict.find(r#"<form class="oml-form""#).unwrap()..];
        let mut out = Vec::new();
        let marker = r#"<input type="hidden" name=""#;
        let mut at = 0;
        while let Some(found) = form[at..].find(marker) {
            let start = at + found + marker.len();
            let name = &form[start..start + form[start..].find('"').unwrap()];
            let v = start + form[start..].find(r#"value=""#).unwrap() + 7;
            let value = &form[v..v + form[v..].find('"').unwrap()];
            if name != "csrf_token" {
                out.push((name.to_owned(), value.replace("&amp;", "&")));
            }
            at = v;
        }
        out
    };
    let (status, _, rebased) = post_form(
        &rig,
        &cookie,
        "/web/workspace/edit",
        &rebase,
        &[("step", "rebase")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rebased}");
    assert!(rebased.contains("Change against v2"), "{rebased}");
    assert!(
        rebased.contains("Tag, build, publish notes, announce.</textarea>"),
        "{rebased}"
    );
    assert!(rebased.contains(">Propose v3</button>"), "{rebased}");
}

/// Retire asks for a reason and names what stops once it is approved; a
/// missing reason is refused beside its field, and the proposal answers
/// `303` with the retirement's notice (ADR-0082 d3).
#[tokio::test]
async fn retire_names_what_stops_and_needs_a_reason() {
    let rig = rig().await;
    let (cookie, id) = approved_skill(&rig).await;
    let (status, _, form) = send(
        &rig.router,
        get(&format!("/web/workspace/retire?memory={id}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{form}");
    assert_builder_copy(&form);
    assert!(form.contains("<h1>Retire release-checklist</h1>"), "{form}");
    assert!(
        form.contains(
            "Once approved, the command <code>/mcp__&lt;server&gt;__release-checklist</code> is no longer offered."
        ),
        "{form}"
    );
    let values = form_values(&form);
    let (status, _, refused) = post_form(
        &rig,
        &cookie,
        "/web/workspace/retire",
        &values,
        &[("step", "preview")],
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert!(refused.contains("A reason is required: it is the retirement version&#39;s text. <code>request.validation</code>"), "{refused}");
    let (status, _, previewed) = post_form(
        &rig,
        &cookie,
        "/web/workspace/retire",
        &values,
        &[
            ("step", "preview"),
            ("reason", "Releases are automated now."),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{previewed}");
    assert!(
        previewed.contains(">Propose retirement</button>"),
        "{previewed}"
    );
    let (status, headers, _) = post_form(
        &rig,
        &cookie,
        "/web/workspace/retire",
        &form_values(&previewed),
        &[("step", "propose")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        headers[header::LOCATION]
            .to_str()
            .unwrap()
            .starts_with("/web/workspace?workspace=alice&kind=skill&done=retirement&held="),
        "{:?}",
        headers[header::LOCATION]
    );
    let (_, _, page) = send(
        &rig.router,
        get("/web/workspace?kind=skill&done=retirement", Some(&cookie)),
    )
    .await;
    assert!(
        page.contains("retirement v2 waits for review</a>"),
        "{page}"
    );
    assert!(page.contains("Retirement of v1"), "{page}");
}

/// A document's preview draws its resource address and its passages, and a
/// guardrail's its session-start line and the set's size against the cap
/// (ADR-0082 d5; ADR-0081 d3).
#[tokio::test]
async fn a_document_and_a_guardrail_preview_draw_what_a_session_receives() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, form) = send(
        &rig.router,
        get("/web/workspace/new?kind=document", Some(&cookie)),
    )
    .await;
    assert!(form.contains("<h1>New document</h1>"), "{form}");
    let mut values = form_values(&form);
    for (name, value) in &mut values {
        match name.as_str() {
            "title" => *value = "Runbook".to_owned(),
            "claim_key" => *value = "doc/runbook".to_owned(),
            "body" => {
                *value = "# Runbook\n\nStart here.\n\n## Deploy\n\nRun the deployer.\n".to_owned()
            }
            _ => {}
        }
    }
    let (status, _, page) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &values,
        &[("step", "preview"), ("format", "text/markdown")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("Its resource address is <code>oml://workspace/alice/doc/runbook</code>."),
        "{page}"
    );
    assert!(
        page.contains("<li><code>#runbook</code> lines 1–3 · Runbook</li>"),
        "{page}"
    );
    assert!(
        page.contains("<li><code>#deploy</code> lines 5–7 · Runbook › Deploy</li>"),
        "{page}"
    );
    assert!(page.contains(">Propose document</button>"), "{page}");

    let (_, _, form) = send(
        &rig.router,
        get("/web/workspace/new?kind=guardrail", Some(&cookie)),
    )
    .await;
    assert!(form.contains("At most 300 bytes (UTF-8)."), "{form}");
    let mut values = form_values(&form);
    for (name, value) in &mut values {
        match name.as_str() {
            "body" => *value = "Never force-push to main.".to_owned(),
            "pattern" => *value = "git push *".to_owned(),
            _ => {}
        }
    }
    let (status, _, page) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &values,
        &[
            ("step", "preview"),
            ("severity", "never"),
            ("tool", "shell"),
            ("effect", "deny"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_builder_copy(&page);
    assert!(
        page.contains("<pre>- [never] Never force-push to main.</pre>"),
        "{page}"
    );
    assert!(
        page.contains("Its rule checks each shell call: command <code>git push *</code> · deny."),
        "{page}"
    );
    assert!(
        page.contains("1 of at most 12 guardrails apply here once it is approved."),
        "{page}"
    );
    assert!(page.contains("25 of 300 bytes (UTF-8)."), "{page}");
}

/// The admin side's workspace page lists the same contents by kind,
/// read-only: no New, Edit, Retire or form, and a person who is no admin
/// is told nothing of it (ADR-0082 d1).
#[tokio::test]
async fn the_admin_workspace_page_has_no_builder_act() {
    let rig = rig().await;
    let (cookie, _) = approved_skill(&rig).await;
    let (status, _, refused) = send(
        &rig.router,
        get(
            "/web/admin/workspace?workspace=alice&kind=skill",
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web/admin/workspace?workspace=alice&kind=skill",
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_builder_copy(&page);
    assert!(
        page.contains("<title>Admin · alice · Workspaces · OML</title>"),
        "{page}"
    );
    assert!(page.contains(r#"<nav class="crumbs" aria-label="Breadcrumb">Administration / <a href="/web/admin/workspaces">Workspaces</a> / alice</nav>"#), "{page}");
    assert!(page.contains(r#"aria-label="Approved contents by kind"><a href="/web/admin/memories?workspace=alice">Memories <span class="n">1</span></a>"#), "{page}");
    assert!(page.contains(">release-checklist</a>"), "{page}");
    assert!(page.contains("first version"), "{page}");
    // The admin search's `promoted` is the served tip: its chip reads
    // approved, never superseded (the review of PR #494, C-B1).
    assert!(page.contains(r#"oml-chip is-promoted"#), "{page}");
    assert!(!page.contains(r#"oml-chip is-superseded"#), "{page}");
    for act in [
        "New skill",
        "/web/workspace/new",
        "/web/workspace/edit",
        "/web/workspace/retire",
        ">Edit<",
        ">Retire<",
        r#"<form class="oml-form""#,
        "Judge it",
        "Withdraw",
    ] {
        assert!(!page.contains(act), "{act}: {page}");
    }
    // The admin Workspaces rows lead here.
    let (_, _, rows) = send(&rig.router, get("/web/admin/workspaces", Some(&boss))).await;
    assert!(
        rows.contains(r#"<a href="/web/admin/workspace?workspace=alice">alice</a>"#),
        "{rows}"
    );
}

/// A builder form carries a body up to the cap the form states, though
/// form encoding triples punctuation and non-ASCII bytes: a 100 KiB
/// document of punctuation and a 25 KB Cyrillic skill each answer their
/// preview, where a 64 KiB form cap answered `413` and lost the typed text
/// (ADR-0082 d4; the review of PR #494, C-B3).
#[tokio::test]
async fn the_builder_previews_a_100_kib_document_and_a_25_kb_cyrillic_skill() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let document = "- [x] #1 (a/b); ".repeat(6_400);
    assert_eq!(document.len(), 102_400);
    let skill = "Проверка ".repeat(1_500);
    assert!(
        skill.len() > 25_000 && skill.len() < 32_768,
        "{}",
        skill.len()
    );
    for (kind, fields) in [
        (
            "document",
            vec![("title", "Runbook"), ("body", document.as_str())],
        ),
        (
            "skill",
            vec![
                ("name", "triage"),
                ("description", "Sort an incident."),
                ("body", skill.as_str()),
            ],
        ),
    ] {
        let (status, _, first) = send(
            &rig.router,
            get(&format!("/web/workspace/new?kind={kind}"), Some(&cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{first}");
        let mut values = form_values(&first);
        for (name, value) in &mut values {
            if let Some((_, v)) = fields.iter().find(|(n, _)| n == name) {
                *value = (*v).to_owned();
            }
        }
        let (status, _, previewed) = post_form(
            &rig,
            &cookie,
            "/web/workspace/new",
            &values,
            &[("step", "preview")],
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{kind}: {}",
            &previewed[..previewed.len().min(400)]
        );
        assert!(
            previewed.contains(r#"<h2 id="preview">Preview</h2>"#),
            "{kind}"
        );
    }
}

/// An edit's prefilled text round-trips: a body with CRLF line breaks and a
/// right-to-left mark, edited only in its description, is proposed with
/// the same bytes, as a browser submits a textarea (every line break as
/// CRLF); nothing becomes U+FFFD (ADR-0082 d4; the review of PR #494, C-B5).
#[tokio::test]
async fn an_edit_keeps_a_bodys_crlf_and_bidi_bytes() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let body = "Tag \u{05E9}\u{05DC}\u{05D5}\u{05DD}\u{200F}.\r\nBuild.\r\nPublish notes.";
    let id = agent_skill(&rig, &value, "release-checklist", body).await;
    let (judged, _) = judge_next(&rig, &cookie, "approve", None).await;
    assert_eq!(judged, id.to_string());
    let (status, _, form) = send(
        &rig.router,
        get(&format!("/web/workspace/edit?memory={id}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{form}");
    assert!(!form.contains('\u{FFFD}'), "{form}");
    // As a browser parses and submits a textarea: CR LF and a lone CR read
    // as LF, and every LF is sent as CRLF.
    let browser = |v: &str| {
        v.replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n")
    };
    let mut values = form_values(&form);
    for (name, value) in &mut values {
        match name.as_str() {
            "description" => *value = "Walk every release step.".to_owned(),
            "body" => *value = browser(value),
            _ => {}
        }
    }
    let (status, _, previewed) = post_form(
        &rig,
        &cookie,
        "/web/workspace/edit",
        &values,
        &[("step", "preview")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{previewed}");
    assert!(
        previewed.contains(r#"<p class="label">Body <span class="n">unchanged</span>"#),
        "{previewed}"
    );
    let mut again = form_values(&previewed);
    for (name, value) in &mut again {
        if name == "body" {
            *value = browser(value);
        }
    }
    let (status, _, _) = post_form(
        &rig,
        &cookie,
        "/web/workspace/edit",
        &again,
        &[("step", "propose")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, waiting) = send(
        &rig.router,
        get(&format!("/web/workspace/edit?memory={id}"), Some(&cookie)),
    )
    .await;
    let at = waiting.find("/web/memory?id=").expect("the held version") + 15;
    let held: uuid::Uuid = waiting[at..at + 36].parse().expect("an id");
    let stored = rig
        .store
        .memory(rig.boot.tenant_id, held)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.body, body, "the edit keeps the body's bytes");
}

/// A shared workspace the person's web key reads and their review key does
/// not, `universe` here, opens read-only instead of being refused: its
/// contents listed, and no New, Edit or Retire (ADR-0082 d12 B4; the
/// review of PR #494, 10).
#[tokio::test]
async fn a_shared_workspace_only_the_web_key_reads_opens_read_only() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web/workspace?workspace=universe&kind=skill",
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_builder_copy(&page);
    assert!(
        page.contains("<h1>universe</h1>") || page.contains(">universe<"),
        "{page}"
    );
    for act in ["New skill", "/web/workspace/new", "/web/workspace/edit"] {
        assert!(!page.contains(act), "{act}: {page}");
    }
}

/// A new guardrail's optional Name becomes its claim key by the slug rule
/// documents use, and with no name the server derives its `auto-…` key
/// (ADR-0082 d4, amended 2026-11-19 by the operator's decision B2).
#[tokio::test]
async fn a_guardrails_name_becomes_its_claim_key_and_none_leaves_it_derived() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for (name, body, key) in [
        (
            "No force push!",
            "Never force-push to main.",
            Some("no-force-push"),
        ),
        ("", "Never delete a release tag.", None),
    ] {
        let (_, _, form) = send(
            &rig.router,
            get("/web/workspace/new?kind=guardrail", Some(&cookie)),
        )
        .await;
        assert!(form.contains(r#"<input name="name""#), "{form}");
        assert!(form.contains("Name (optional)"), "{form}");
        let mut values = form_values(&form);
        for (field, value) in &mut values {
            match field.as_str() {
                "name" => *value = name.to_owned(),
                "body" => *value = body.to_owned(),
                _ => {}
            }
        }
        let extra = [("severity", "never"), ("tool", ""), ("effect", "")];
        let mut with: Vec<(&str, &str)> = extra.to_vec();
        with.push(("step", "preview"));
        let (status, _, previewed) =
            post_form(&rig, &cookie, "/web/workspace/new", &values, &with).await;
        assert_eq!(status, StatusCode::OK, "{previewed}");
        let mut with: Vec<(&str, &str)> = extra.to_vec();
        with.push(("step", "propose"));
        let (status, headers, page) = post_form(
            &rig,
            &cookie,
            "/web/workspace/new",
            &form_values(&previewed),
            &with,
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        let location = headers[header::LOCATION].to_str().unwrap().to_owned();
        let at = location.find("held=").expect("the held id") + 5;
        let held: uuid::Uuid = location[at..].parse().expect("an id");
        let stored = rig
            .store
            .memory(rig.boot.tenant_id, held)
            .await
            .unwrap()
            .unwrap();
        match key {
            Some(key) => assert_eq!(stored.claim_key, key),
            None => assert!(
                stored.claim_key.starts_with("auto-"),
                "{}",
                stored.claim_key
            ),
        }
    }
}

/// A carriage return before a line feed is part of the line break a
/// browser sends for every textarea: a CRLF body shows no `U+000D` marker
/// on its card, on Judge or in the builder's preview, while a lone CR,
/// which no reader sees, is still flagged on each (the review of PR #494,
/// re-verification).
#[tokio::test]
async fn a_crlf_body_shows_no_carriage_return_marker_and_a_lone_cr_is_still_flagged() {
    const CR: &str = "U+000D";
    let rig = rig().await;
    let (alice, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let crlf = agent_writes(
        &rig,
        &agent,
        "Deploys run on Tuesdays.\r\nThe standup is first.",
    )
    .await;
    let (_, _, cards) = send(
        &rig.router,
        get("/web/memories?view=submitted", Some(&alice)),
    )
    .await;
    assert!(cards.contains("Deploys run on Tuesdays."), "{cards}");
    assert!(!cards.contains(CR), "a CRLF card: {cards}");
    let (_, _, judge) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={crlf}"), Some(&alice)),
    )
    .await;
    assert!(judge.contains("Deploys run on Tuesdays."), "{judge}");
    assert!(!judge.contains(CR), "a CRLF body on Judge: {judge}");
    let lone = agent_writes(&rig, &agent, "Rollbacks are manual.\rNo script runs them.").await;
    let (_, _, cards) = send(
        &rig.router,
        get("/web/memories?view=submitted", Some(&alice)),
    )
    .await;
    assert_eq!(cards.matches(CR).count(), 1, "the lone CR's card: {cards}");
    let (_, _, judge) = send(
        &rig.router,
        get(&format!("/web/todo/judge?memory={lone}"), Some(&alice)),
    )
    .await;
    assert!(judge.contains(CR), "a lone CR on Judge: {judge}");
    let (status, _, first) = send(
        &rig.router,
        get("/web/workspace/new?kind=guardrail", Some(&alice)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    for (body, flagged) in [
        ("Never deploy on Fridays.\r\nAsk first.", false),
        ("Never deploy on Fridays.\rAsk first.", true),
    ] {
        let mut values = form_values(&first);
        for (name, value) in &mut values {
            match name.as_str() {
                "name" => *value = "fridays".to_owned(),
                "body" => *value = body.to_owned(),
                _ => {}
            }
        }
        let (status, _, previewed) = post_form(
            &rig,
            &alice,
            "/web/workspace/new",
            &values,
            &[("step", "preview"), ("severity", "never")],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{previewed}");
        assert!(
            previewed.contains(r#"<h2 id="preview">Preview</h2>"#),
            "{previewed}"
        );
        assert_eq!(previewed.contains(CR), flagged, "{body:?}: {previewed}");
    }
}

/// A builder form past a page's 64 KiB is read only under a live session:
/// under a session that is gone, a post past even the builder's cap is
/// sent to sign in unread, never answered `413` after its body was taken
/// in (the review of PR #494, re-verification).
#[tokio::test]
async fn a_builder_post_over_64_kib_under_a_gone_session_goes_to_sign_in_unread() {
    let rig = rig().await;
    let gone = format!("{SESSION_COOKIE}={}", "7".repeat(64));
    let body = format!(
        "csrf_token=x&body={}",
        "a".repeat(oml_web::middleware::BUILDER_FORM_MAX_BYTES)
    );
    let request = Request::post("/web/workspace/new")
        .header(header::CONTENT_TYPE, FORM)
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, &gone)
        .body(Body::from(body))
        .expect("a request");
    let (status, headers, page) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    assert!(
        headers[header::LOCATION]
            .to_str()
            .expect("ASCII")
            .starts_with("/web/sign-in"),
        "{headers:?}"
    );
}

/// The skill `triage` previewed on the builder of `workspace` under the
/// session `cookie`: the values of the form the preview answered.
async fn skill_previewed(rig: &Rig, cookie: &str, workspace: &str) -> Vec<(String, String)> {
    let (status, _, first) = send(
        &rig.router,
        get(
            &format!("/web/workspace/new?kind=skill&workspace={workspace}"),
            Some(cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let mut values = form_values(&first);
    for (name, value) in &mut values {
        match name.as_str() {
            "name" => *value = "triage".to_owned(),
            "description" => *value = "Sort an incident.".to_owned(),
            "body" => *value = "Read the alert.".to_owned(),
            _ => {}
        }
    }
    let (status, _, previewed) = post_form(
        rig,
        cookie,
        "/web/workspace/new",
        &values,
        &[("step", "preview")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{previewed}");
    let values = form_values(&previewed);
    assert!(values.iter().any(|(n, _)| n == "previewed"), "{previewed}");
    values
}

fn form_field(values: &[(String, String)], name: &str) -> String {
    values
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// The preview stamp a builder form carries is keyed apart from the act's
/// `Idempotency-Key`: by the act key of `preview/<nonce>`, never by the key
/// the write is sent under (the review of PR #494, re-verification).
#[tokio::test]
async fn the_preview_stamp_is_keyed_apart_from_the_acts_idempotency_key() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let values = skill_previewed(&rig, &cookie, "alice").await;
    let token = cookie.split_once('=').expect("a cookie").1;
    let nonce = form_field(&values, "nonce");
    let workspace = form_field(&values, "workspace");
    let joined = [
        "skill",
        "new",
        "",
        workspace.as_str(),
        "",
        "triage",
        "Sort an incident.",
        "Read the alert.",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
    ]
    .join("\u{0}");
    let stamp = |key: &str| -> String {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes());
        ring::hmac::sign(&key, joined.as_bytes()).as_ref()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let previewed = form_field(&values, "previewed");
    assert_ne!(
        previewed,
        stamp(&oml_web_credential::act_idempotency_key(token, &nonce)),
        "keyed by the act's Idempotency-Key"
    );
    assert_eq!(
        previewed,
        stamp(&oml_web_credential::act_idempotency_key(
            token,
            &format!("preview/{nonce}")
        ))
    );
}

/// Propose writes only under the stamp its own preview drew: a forged
/// `previewed`, one drawn for another session of the same person, and one
/// sent with another workspace each redraw the preview and write nothing
/// (ADR-0082 d5; the review of PR #494, re-verification).
#[tokio::test]
async fn a_forged_or_borrowed_preview_stamp_re_previews_and_writes_nothing() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let other = signed_in(&rig, "sub-1", EMAIL).await;
    assert_ne!(cookie, other, "two sessions");
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[("action", "create_workspace"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let mine = skill_previewed(&rig, &cookie, "alice").await;
    let theirs = skill_previewed(&rig, &other, "alice").await;
    let set = |values: &[(String, String)], name: &str, to: &str| -> Vec<(String, String)> {
        values
            .iter()
            .map(|(n, v)| (n.clone(), if n == name { to.to_owned() } else { v.clone() }))
            .collect()
    };
    for (why, sent) in [
        ("forged", set(&mine, "previewed", &"0".repeat(32))),
        ("another session's", theirs.clone()),
        ("another workspace's", set(&mine, "workspace", "alice.proj")),
    ] {
        let (status, _, page) = post_form(
            &rig,
            &cookie,
            "/web/workspace/new",
            &sent,
            &[("step", "propose")],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{why}: {page}");
        assert!(
            page.contains(r#"<h2 id="preview">Preview</h2>"#),
            "{why}: {page}"
        );
        assert_eq!(held_skills(&rig).await, 0, "{why}: nothing is written");
    }
    // Its own stamp writes once.
    let (status, _, page) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &mine,
        &[("step", "propose")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    assert_eq!(held_skills(&rig).await, 1);
}

/// A body that starts with a line break keeps it on the builder: the
/// textarea opens with a line break of its own, the one an HTML parser
/// drops, so the body's own is what the next post sends (the review of PR
/// #494, re-verification).
#[tokio::test]
async fn a_body_that_starts_with_a_line_break_keeps_it_in_the_builders_textarea() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, first) = send(
        &rig.router,
        get("/web/workspace/new?kind=skill", Some(&cookie)),
    )
    .await;
    let mut values = form_values(&first);
    for (name, value) in &mut values {
        match name.as_str() {
            "name" => *value = "triage".to_owned(),
            "description" => *value = "Sort an incident.".to_owned(),
            "body" => *value = "\nRead the alert.".to_owned(),
            _ => {}
        }
    }
    let (status, _, previewed) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &values,
        &[("step", "preview")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{previewed}");
    assert!(
        previewed.contains("<textarea name=\"body\" required>\n\nRead the alert.</textarea>"),
        "{previewed}"
    );
    assert_eq!(
        form_field(&form_values(&previewed), "body"),
        "\nRead the alert."
    );
}

/// Next pages on past the first 50 held rows: from the last row of the
/// first page it opens the first of the second, and from a row past the
/// first page the row after it, never the oldest row again (the review of
/// PR #494, re-verification).
#[tokio::test]
async fn next_from_an_item_past_the_first_50_rows_opens_the_row_after_it() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let mut held = Vec::new();
    for n in 0..53 {
        held.push(agent_writes(&rig, &value, &format!("the build number is {n}")).await);
    }
    for (left, opens) in [
        (held[49], held[50]),
        (held[50], held[51]),
        (held[51], held[52]),
    ] {
        let (status, headers, body) = act(
            &rig,
            &cookie,
            "/web/todo",
            &[("action", "next"), ("after", &left.to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        assert_eq!(
            headers[header::LOCATION].to_str().unwrap(),
            format!("/web/todo/judge?memory={opens}"),
            "after {left}"
        );
    }
}

/// A guardrail named "Auto deploy" is proposed under a claim key without
/// the `auto-` lead only the server derives: its preview is no refusal,
/// and Propose writes it under `deploy` (ADR-0039 d6; the review of PR
/// #494, re-verification).
#[tokio::test]
async fn a_guardrail_named_auto_something_is_proposed_without_the_servers_auto_lead() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, first) = send(
        &rig.router,
        get("/web/workspace/new?kind=guardrail", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let mut values = form_values(&first);
    for (name, value) in &mut values {
        match name.as_str() {
            "name" => *value = "Auto deploy".to_owned(),
            "body" => *value = "Never deploy without a ticket.".to_owned(),
            _ => {}
        }
    }
    let (status, _, previewed) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &values,
        &[("step", "preview"), ("severity", "never")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{previewed}");
    assert!(!previewed.contains("auto-"), "{previewed}");
    // The severity is a radio, which `form_values` does not read.
    let (status, _, page) = post_form(
        &rig,
        &cookie,
        "/web/workspace/new",
        &form_values(&previewed),
        &[("step", "propose"), ("severity", "never")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    let written: Vec<String> = rig
        .app
        .admin_search(
            rig.boot.admin_key.value.as_str(),
            Some("Never deploy without a ticket."),
            None,
            None,
            None,
            10,
            Some(boot_investigation(&rig).await),
        )
        .await
        .expect("the admin reads")
        .items
        .iter()
        .map(|m| m.memory.claim_key.clone())
        .collect();
    assert_eq!(written, ["deploy"]);
}

// -- the Catalog page (ADR-0092 d7) ---------------------------------------------

/// A skill `name` the agent key `value` proposes in `workspace`, held until a
/// person's reviewer key approves it.
async fn agent_skill_in(rig: &Rig, value: &str, name: &str, workspace: &str) -> uuid::Uuid {
    rig.app
        .submit_memory(
            value,
            &oml_app::types::SubmitRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                idempotency_key: format!("skill-{name}"),
                claim_key: None,
                verb: None,
                kind: oml_app::domain::MemoryKind::Skill,
                body: format!("Run the {name} script, then check the health page."),
                workspace: Some(
                    oml_app::domain::name::Reference::parse(workspace).expect("a reference"),
                ),
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: Some(oml_app::types::KindFields::Skill {
                    name: name.to_owned(),
                    description: format!("Runs {name} and checks it."),
                }),
                valid_from: None,
                valid_until: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("the agent proposes a skill")
        .receipt
        .memory_id
}

/// The signed-in person's reviewer key approves `id` on Judge, as To do
/// opens it (ADR-0083 d2).
async fn person_approves(rig: &Rig, cookie: &str, id: uuid::Uuid) {
    let (status, _, page) = judge(rig, cookie, &id.to_string(), "approve", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
}

/// `n` memories alice's agent writes in her root, each approved by a
/// tenant reviewer agent key.
async fn approved_memories(rig: &Rig, value: &str, n: usize) {
    let helper = reviewer_in_alices_root(rig).await;
    for i in 0..n {
        let id = agent_writes(
            rig,
            value,
            &format!("The service in area {i} logs one line per request and rotates its log daily at midnight UTC, keeping fourteen days of history on the node before the shipper moves it."),
        )
        .await;
        rig.app
            .post_verdict(&helper.value, id, oml_app::types::Verdict::Approve, None)
            .await
            .expect("the tenant reviewer approves");
    }
}

#[tokio::test]
async fn the_catalog_is_a_personal_tab_after_memories_headed_catalog_under_its_breadcrumb() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(&rig.router, get("/web/catalog", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let page = markup(&page);
    assert!(
        page.contains(r#"<a href="/web/memories">Memories</a><a href="/web/catalog" class="on" aria-current="page">Catalog</a><a href="/web/atlas">Atlas</a>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<p class="oml-crumbs"><a href="/web/start">Personal</a> / Catalog</p>"#),
        "{page}"
    );
    assert_eq!(between(&page, "<h1>", "</h1>"), "Catalog");
    assert!(
        page.contains("Read as <code>alice-web</code>: one row per claim, newest version first."),
        "{page}"
    );
    // A GET form and links only: the page writes nothing and runs no script.
    assert!(!page.contains("<script"), "{page}");
    assert!(
        !page.contains(r#"method="post" action="/web/catalog""#),
        "{page}"
    );
    assert!(
        page.contains(r#"<form class="filters" method="get" action="/web/catalog">"#),
        "{page}"
    );
    assert!(
        page.contains("skill/, doc/runbooks/ or any claim key"),
        "{page}"
    );
}

#[tokio::test]
async fn the_kind_tabs_show_each_kinds_count_and_a_zero_tab_is_greyed_and_still_a_link() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    approved_memories(&rig, &value, 2).await;
    let (status, _, page) = send(&rig.router, get("/web/catalog", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let tabs = between(
        &page,
        r#"<nav class="oml-views kinds" aria-label="Kinds">"#,
        "</nav>",
    );
    assert!(
        tabs.starts_with(r#"<a href="/web/catalog" aria-current="page">All <span class="n">2</span></a><a href="/web/catalog?kinds=memory">Memories <span class="n">2</span></a>"#),
        "{tabs}"
    );
    assert!(
        tabs.contains(
            r#"<a href="/web/catalog?kinds=skill" class="zero">Skills <span class="n">0</span></a>"#
        ),
        "{tabs}"
    );
    assert!(
        tabs.ends_with(r#"<a href="/web/catalog?kinds=term%2Cterm_mapping%2Cvocab_release" class="zero">Terms <span class="n">0</span></a>"#),
        "{tabs}"
    );
    // The zero tab is a link to an empty table, not a dead end.
    let (status, _, empty) =
        send(&rig.router, get("/web/catalog?kinds=skill", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert!(
        empty.contains(r#"<a href="/web/catalog?kinds=skill" class="zero" aria-current="page">Skills <span class="n">0</span></a>"#),
        "{empty}"
    );
}

#[tokio::test]
async fn every_filter_carries_across_the_kind_tabs_and_the_form_keeps_the_tab() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web/catalog?workspace=alice&kinds=skill&prefix=skill%2Fde&state=approved",
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(r#"<a href="/web/catalog?workspace=alice&amp;kinds=guardrail&amp;prefix=skill%2Fde&amp;state=approved" class="zero">Guardrails"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<input type="hidden" name="kinds" value="skill">"#),
        "{page}"
    );
    assert!(page.contains(r#"name="prefix" value="skill/de""#), "{page}");
}

#[tokio::test]
async fn a_memory_row_marks_its_description_as_the_servers_preview_and_names_its_reviewers_kind() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    approved_memories(&rig, &value, 1).await;
    let (_, _, page) = send(&rig.router, get("/web/catalog", Some(&cookie))).await;
    assert!(
        page.contains(r#"<td class="c-desc"><span class="tag oml-chip is-quiet">preview</span>The service in area 0 logs one line per request"#),
        "{page}"
    );
    let state = between(&page, r#"<td class="c-state">"#, "</td>");
    assert!(state.contains("approved"), "{state}");
    assert!(state.contains(" by an agent reviewer</span>"), "{state}");
    assert!(
        page.contains(r#"<td class="c-ver c-meta">v1</td>"#),
        "{page}"
    );
}

#[tokio::test]
async fn a_skill_row_says_offered_here_or_not_offered_at_the_workspace_read() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    alice_proj(&rig, &cookie).await;
    let here = agent_skill_in(&rig, &value, "deploy", "alice").await;
    let below = agent_skill_in(&rig, &value, "bump", "alice.proj").await;
    person_approves(&rig, &cookie, here).await;
    person_approves(&rig, &cookie, below).await;
    let (status, _, page) = send(&rig.router, get("/web/catalog?kinds=skill", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let row_of = |name: &str| {
        let at = page
            .find(&format!(">{name}</a>"))
            .unwrap_or_else(|| panic!("a row named {name}: {page}"));
        let start = page[..at].rfind("<tr>").expect("its row");
        let end = page[at..].find("</tr>").expect("its end") + at;
        page[start..end].to_owned()
    };
    let deploy = row_of("deploy");
    assert!(
        deploy.contains(r#"<span class="key">skill/deploy</span>"#),
        "{deploy}"
    );
    assert!(deploy.contains("offered here</span>"), "{deploy}");
    assert!(deploy.contains("Runs deploy and checks it."), "{deploy}");
    assert!(deploy.contains(" by a person reviewer</span>"), "{deploy}");
    let bump = row_of("bump");
    assert!(
        bump.contains(r#"<span class="mark">not offered at <code>alice</code></span>"#),
        "{bump}"
    );
    assert!(
        bump.contains(r#"<a href="/web/workspaces?workspace=alice.proj">alice.proj</a>"#),
        "{bump}"
    );
}

#[tokio::test]
async fn a_guardrail_row_leads_with_its_severity_word_and_says_in_force_here() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_guardrail(&rig, &value, "Run the tests before every commit.").await;
    person_approves(&rig, &cookie, id).await;
    let (status, _, page) = send(
        &rig.router,
        get("/web/catalog?kinds=guardrail", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(r#"<td class="c-desc"><b class="sev">must</b></td>"#),
        "{page}"
    );
    assert!(page.contains("in force here</span>"), "{page}");
    assert!(!page.contains("not in force at"), "{page}");
}

#[tokio::test]
async fn a_held_skill_in_a_development_workspace_is_served_unreviewed_and_needs_a_person() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    alice_proj(&rig, &cookie).await;
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workspaces",
        &[
            ("action", "workspace_mode"),
            ("workspace", "alice.proj"),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let held = agent_skill_in(&rig, &value, "bump-deps", "alice.proj").await;
    let (status, _, page) = send(&rig.router, get("/web/catalog", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let state = between(&page, r#"<td class="c-state">"#, "</td>");
    assert!(state.contains("unreviewed"), "{state}");
    assert!(
        state.contains(r#"<span class="when">served here, unreviewed</span>"#),
        "{state}"
    );
    assert!(state.contains("needs a person</span>"), "{state}");
    // alice-review may judge it in her own root: Judge it opens the judge
    // page (ADR-0092 d7; ADR-0083 d2).
    assert!(
        state.contains(&format!(
            r#"<a href="/web/todo/judge?memory={held}">Judge it</a>"#
        )),
        "{state}"
    );
    // The read serves held rows here, so State offers Unreviewed.
    assert!(
        page.contains(r#"<option value="unreviewed">Unreviewed</option>"#),
        "{page}"
    );
}

#[tokio::test]
async fn an_empty_tab_says_nothing_of_this_kind_is_read_in_the_workspace_and_offers_no_unreviewed()
{
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(
        &rig.router,
        get("/web/catalog?kinds=guardrail", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(r#"<div class="oml-empty"><p>Nothing of this kind in <code>alice</code> that this key reads.</p></div>"#),
        "{page}"
    );
    assert!(!page.contains("<table"), "{page}");
    assert!(!page.contains(r#"value="unreviewed""#), "{page}");
    assert!(!page.contains("Next page"), "{page}");
}

#[tokio::test]
async fn the_next_page_follows_the_cursor_without_page_numbers_and_counts_ride_the_first_page_only()
{
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    approved_memories(&rig, &value, 51).await;
    let (_, _, first) = send(&rig.router, get("/web/catalog", Some(&cookie))).await;
    assert_eq!(
        first.matches("<tr><td class=\"c-name\">").count(),
        50,
        "{first}"
    );
    assert!(
        first.contains(r#"All <span class="n">51</span>"#),
        "{first}"
    );
    let marker = r#"<a rel="next" href="/web/catalog?cursor="#;
    let at = first.find(marker).expect("a next page") + marker.len();
    let cursor = &first[at..at + first[at..].find('"').expect("a quote")];
    let (status, _, second) = send(
        &rig.router,
        get(&format!("/web/catalog?cursor={cursor}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(
        second.matches("<tr><td class=\"c-name\">").count(),
        1,
        "{second}"
    );
    assert!(
        second.contains(r#"<a href="/web/catalog">First page</a>"#),
        "{second}"
    );
    assert!(!second.contains("Next page"), "{second}");
    // Counts come with the first page only; the tabs stay links.
    assert!(!second.contains(r#"<span class="n">"#), "{second}");
    assert!(
        second.contains(r#"<a href="/web/catalog?kinds=memory">Memories</a>"#),
        "{second}"
    );
}

// ---------------------------------------------------------------------------
// ADR-0096: the v0.8.5 console round's pages
// ---------------------------------------------------------------------------

/// The builder's three steps, with no script: the templates, the form a
/// template opens, the review that restates the access and posts once,
/// and the error summary for a refusal the page decides (ADR-0096 d1, D5,
/// D12). Issue key shows the value once and the row lists the key with
/// its grants' coverage and its rate (d2, d8).
#[tokio::test]
async fn the_key_builder_reviews_a_key_before_it_issues_it_and_shows_the_value_once() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, start) = send(&rig.router, get("/web/keys/new", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{start}");
    for shape in ["root", "one", "read", "write", "connector"] {
        assert!(
            start.contains(&format!(r#"href="/web/keys/new?shape={shape}""#)),
            "{shape}: {start}"
        );
    }
    assert!(!start.contains("<script"), "{start}");
    let (_, _, form) = send(&rig.router, get("/web/keys/new?shape=root", Some(&cookie))).await;
    assert!(form.contains(r#"<span class="p">alice-</span>"#), "{form}");
    assert!(form.contains(" autofocus"), "{form}");
    assert!(form.contains("Add a grant"), "{form}");
    // A reserved name is the page's own refusal, in the summary, linked to
    // the control, which names its error state.
    let reserved = "/web/keys/new?kind=agent&name=review&role=contributor&n=2&g0w=alice&g0r=submitter&g0c=subtree&g1w=alice&g1r=reader&g1c=subtree&dw=&gs=ancestors&exp=&rpm=&step=review";
    let (status, _, page) = send(&rig.router, get(reserved, Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page.matches(r#"role="alert""#).count(), 1, "{page}");
    assert!(page.contains(r##"<a href="#kn">"##), "{page}");
    assert!(
        page.contains(r#"id="kn" name="name" value="review" required aria-invalid="true""#),
        "{page}"
    );
    assert!(page.contains("<code>request.validation</code>"), "{page}");
    // A clean draft reviews, then issues once.
    let draft = "/web/keys/new?kind=agent&name=ci&role=reader&n=1&g0w=alice&g0r=reader&g0c=node&dw=&gs=ancestors&exp=&rpm=60&step=review";
    let (_, _, review) = send(&rig.router, get(draft, Some(&cookie))).await;
    assert!(review.contains("<h1>Review key</h1>"), "{review}");
    assert!(review.contains("Effective access"), "{review}");
    assert!(review.contains("The exact request"), "{review}");
    assert!(
        review.contains("&quot;requests_per_minute&quot;: 60"),
        "{review}"
    );
    let fields = hidden_fields(&review, "build_key");
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let (status, headers, body) =
        send(&rig.router, post("/web/keys", &fields, Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (sealed, _) = set_cookie(&headers, ONCE_COOKIE).expect("the shown-once cookie");
    let (_, _, shown) = send(
        &rig.router,
        get(
            "/web/keys",
            Some(&format!("{cookie}; {ONCE_COOKIE}={sealed}")),
        ),
    )
    .await;
    assert!(shown.contains("OML_API_KEY="), "{shown}");
    let key = key_named(&rig, "alice-ci").await;
    assert_eq!(key.requests_per_minute, Some(60));
    let (_, _, list) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(list.contains("(this workspace only)"), "{list}");
    assert!(list.contains("60 requests a minute"), "{list}");
}

/// The admin's one-key page reads the key through `GET /v2/keys/{key}` as
/// `<user>-admin` and offers Set limits and Deny on a workspace; the limits
/// form sends the date as the next UTC day's end, and the denial is
/// confirmed before it posts (ADR-0096 d4, d10).
#[tokio::test]
async fn the_admin_one_key_page_sets_limits_and_denies_on_a_workspace() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    laptop_key(&rig, &cookie).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, list) = send(&rig.router, get("/web/admin/keys?user=alice", Some(&boss))).await;
    assert!(
        list.contains(r#"href="/web/admin/keys?key=alice-laptop">Open</a>"#),
        "{list}"
    );
    let (status, _, page) = send(
        &rig.router,
        get("/web/admin/keys?key=alice-laptop", Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("read as <code>boss-admin</code>"), "{page}");
    assert!(
        page.contains("Set limits") && page.contains("Deny on a workspace"),
        "{page}"
    );
    // The read journals nothing, and its line says nothing of an audit trail.
    assert!(
        page.contains("read as <code>boss-admin</code>.</p>"),
        "{page}"
    );
    // Set limits is the page's primary act, the record draws no label row
    // (the server stores no label to serve), and a grant's role is one cell
    // that does not wrap.
    assert!(
        page.contains(r#"<a class="button primary" href="/web/admin/keys?key=alice-laptop&amp;op=limits">Set limits</a>"#),
        "{page}"
    );
    assert!(!page.contains(r#"<th scope="row">Label</th>"#), "{page}");
    assert!(page.contains(r#"<td class="c-grant">"#), "{page}");
    // A session key and an admin key take neither limits nor a denial.
    for other in ["alice-web", "root-admin"] {
        let (status, _, page) = send(
            &rig.router,
            get(&format!("/web/admin/keys?key={other}"), Some(&boss)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{other}: {page}");
        assert!(
            !page.contains("Set limits") && !page.contains("Deny on a workspace"),
            "{other}: {page}"
        );
    }
    let (_, _, form) = send(
        &rig.router,
        get("/web/admin/keys?key=alice-laptop&op=limits", Some(&boss)),
    )
    .await;
    let csrf = csrf_of(&form);
    let (status, headers, body) = send(
        &rig.router,
        post(
            "/web/admin/keys",
            &[
                ("csrf_token", &csrf),
                ("action", "set_limits"),
                ("key", "alice-laptop"),
                ("expires", "2999-01-01"),
                ("rate", "30"),
            ],
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(
        headers[header::LOCATION],
        "/web/admin/keys?key=alice-laptop&done=limits"
    );
    let key = key_named(&rig, "alice-laptop").await;
    assert_eq!(
        (
            key.expiry.as_ref().map(|e| e.as_str().to_owned()),
            key.requests_per_minute
        ),
        (Some("2999-01-02T23:59:59.000000Z".to_owned()), Some(30))
    );
    let (_, _, page) = send(
        &rig.router,
        get("/web/admin/keys?key=alice-laptop", Some(&boss)),
    )
    .await;
    assert!(
        page.contains("works through 2999-01-01, every time zone"),
        "{page}"
    );
    let (_, _, review) = send(
        &rig.router,
        get(
            "/web/admin/keys?key=alice-laptop&op=deny&workspace=alice&reason=leaked&step=review",
            Some(&boss),
        ),
    )
    .await;
    assert!(review.contains("Deny alice-laptop on alice?"), "{review}");
    let fields = hidden_fields(&review, "deny_on");
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let (status, _, body) = send(&rig.router, post("/web/admin/keys", &fields, Some(&boss))).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let (_, _, page) = send(
        &rig.router,
        get("/web/admin/keys?key=alice-laptop", Some(&boss)),
    )
    .await;
    assert!(page.contains("denied on <code>"), "{page}");
}

/// The administration To do lists admin work only: with no fix waiting it
/// says nothing waits, and it offers no verdict, confirmation or
/// ratification (ADR-0096 d7; ADR-0083 d11).
#[tokio::test]
async fn the_admin_todo_offers_no_verdict_and_says_when_nothing_waits() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    // A browser that opened no investigation: an open one is a group.
    let boss = signed_in_without_investigation(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(&rig.router, get("/web/admin/todo", Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Nothing waits for this account."), "{page}");
    for act in ["Approve", "Confirm", "Ratify", "Revert"] {
        assert!(!page.contains(&format!(">{act}<")), "{act}: {page}");
    }
}

/// The Fix tab names no target until an investigation's memory page links
/// one, and offers demote and erase alone on a frozen workspace (ADR-0096
/// d5).
#[tokio::test]
async fn the_fix_tab_takes_its_target_from_an_investigation() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web/admin/workspace?workspace=universe&view=fix",
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Unconfirmed here"), "{page}");
    assert!(page.contains("an investigation opens first"), "{page}");
    let target = "00000000-0000-4000-8000-000000000001";
    let (_, _, page) = send(
        &rig.router,
        get(
            &format!("/web/admin/workspace?workspace=universe&view=fix&target={target}"),
            Some(&boss),
        ),
    )
    .await;
    for act in ["demote", "edit", "retire", "erase"] {
        assert!(page.contains(&format!(r#"value="{act}""#)), "{act}: {page}");
    }
    let (_, _, review) = send(
        &rig.router,
        get(
            &format!("/web/admin/workspace?workspace=universe&view=fix&target={target}&act=erase&reason=leak&step=review"),
            Some(&boss),
        ),
    )
    .await;
    assert!(
        review.contains(r#"class="danger">Erase</button>"#),
        "{review}"
    );
    assert!(review.contains("crypto-shredded"), "{review}");
}

/// The Fix tab of a nested workspace carries the workspace's id, never its
/// bare name, which names no workspace on its own: the Memories link, the
/// form, the review step and the act's way back each name the id
/// (ADR-0096 d5).
#[tokio::test]
async fn the_fix_tab_of_a_nested_workspace_carries_its_id_everywhere() {
    tokio::time::timeout(Duration::from_secs(120), async {
        let rig = rig().await;
        rig.allow("boss@example.com", "boss", true).await;
        let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
        // One issuer for both workspaces: two issuers count their ids from
        // the same start, and a second one would give `docs` the id of
        // `team`, its own parent.
        let issuer = issuer_app(&rig);
        let team = issuer
            .create_workspace(
                rig.boot.admin_key.value.as_str(),
                &Name::parse("team").expect("a name"),
                Some(rig.boot.root_workspace_id.into()),
                None,
            )
            .await
            .expect("a shared workspace")
            .workspace
            .workspace_id;
        let docs = issuer
            .create_workspace(
                rig.boot.admin_key.value.as_str(),
                &Name::parse("docs").expect("a name"),
                Some(team.into()),
                None,
            )
            .await
            .expect("a nested workspace")
            .workspace
            .workspace_id
            .to_string();
        assert_ne!(docs, team.to_string());
        let target = "00000000-0000-4000-8000-000000000001";
        let carried = format!(r#"<input type="hidden" name="workspace" value="{docs}">"#);
        let (status, _, form) = send(
            &rig.router,
            get(
                &format!("/web/admin/workspace?workspace={docs}&view=fix&target={target}"),
                Some(&boss),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{form}");
        assert!(form.contains(&carried), "{form}");
        assert!(
            form.contains(&format!(
                r#"href="/web/admin/workspace?workspace={docs}">Memories"#
            )),
            "{form}"
        );
        let (_, _, review) = send(
            &rig.router,
            get(
                &format!("/web/admin/workspace?workspace={docs}&view=fix&target={target}&act=retire&reason=stale&step=review"),
                Some(&boss),
            ),
        )
        .await;
        assert!(review.contains(&carried), "{review}");
        assert!(
            !review.contains(r#"name="workspace" value="docs""#),
            "{review}"
        );
    })
    .await
    .expect("the Fix tab answers within two minutes");
}

/// The import pages: the hand-off states the built rule, and the list of
/// the person's imports reads through `GET /v2/imports` as `<user>-web`
/// (ADR-0096 d6).
#[tokio::test]
async fn the_import_pages_hand_off_to_the_command_line_and_list_the_persons_imports() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(&rig.router, get("/web/workspaces/import", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("import.target_not_in_progress"), "{page}");
    let (_, _, page) = send(
        &rig.router,
        get("/web/workspaces/import?into=docs", Some(&cookie)),
    )
    .await;
    assert!(
        page.contains("oml workspaces import --into alice.docs"),
        "{page}"
    );
    let (status, _, list) = send(
        &rig.router,
        get("/web/workspaces/import?list=1", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert!(list.contains("No import yet."), "{list}");
}

/// To do's brief group follows To review: empty, it is one line, and the
/// Ratify act asks for confirmation before it posts (ADR-0096 d7).
#[tokio::test]
async fn todo_folds_an_empty_brief_group_and_ratify_is_confirmed_first() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Nothing waits for this account."), "{page}");
    let csrf = csrf_of(&page);
    let edge = "00000000-0000-4000-8000-000000000009";
    let (status, _, confirm) = send(
        &rig.router,
        post(
            "/web/todo",
            &[
                ("csrf_token", &csrf),
                ("action", "ratify"),
                ("edge_id", edge),
                ("title", "deploys"),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    assert!(confirm.contains("Ratify the brief proposal?"), "{confirm}");
    assert!(
        confirm.contains(r#"name="confirm" value="yes""#),
        "{confirm}"
    );
}

// -- v0.8.5: the brief, the status proposal, the fix page and the admin's
// investigations (ADR-0095 d5, d6; ADR-0096 P7, P8, P9, P14) ---------------

/// The value of the `Location` an act answered with.
fn location(headers: &HeaderMap) -> String {
    headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned()
}

/// alice signed in with an agent whose row her review key approved through
/// Judge: her cookie and the approved row's id.
async fn alice_with_an_approved_memory(rig: &Rig) -> (String, String) {
    let (cookie, value) = person_with_agent(rig, "sub-1", EMAIL).await;
    let id = agent_writes(rig, &value, "the deploy runs from main")
        .await
        .to_string();
    let (status, _, page) = judge(rig, &cookie, &id, "approve", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    (cookie, id)
}

/// P8 and P7: an approved memory's page offers Propose for brief, which
/// asks first and then proposes once as `alice-web`, landing on the
/// workspace's Brief with the proposal named by its key and marked same
/// user; a second proposal comes back to the memory naming the live one
/// with its code; and `alice-review` ratifies it from the Brief after a
/// confirmation, so it is a member and no longer a proposal.
#[tokio::test]
async fn propose_for_brief_asks_first_names_the_live_proposal_the_second_time_and_the_brief_ratifies_it()
 {
    let rig = rig().await;
    let (cookie, id) = alice_with_an_approved_memory(&rig).await;
    let (status, _, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={id}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Propose for brief"), "{page}");
    let (_, _, confirm) = send(
        &rig.router,
        get(&format!("/web/memory?id={id}&act=brief"), Some(&cookie)),
    )
    .await;
    assert!(
        confirm.contains("Proposed as <code>alice-web</code> for the brief of"),
        "{confirm}"
    );
    let workspace = field_of(&confirm, "workspace");
    let fields = [
        ("action", "propose_brief"),
        ("memory", id.as_str()),
        ("workspace", workspace.as_str()),
    ];
    let (status, _, brief) = act_and_follow(&rig, &cookie, "/web/memory", &fields).await;
    assert_eq!(status, StatusCode::OK, "{brief}");
    assert!(brief.contains("Proposed for the brief"), "{brief}");
    assert!(brief.contains("Proposals · 1"), "{brief}");
    assert!(
        brief.contains("by <code>alice-web</code> · same user"),
        "{brief}"
    );
    assert!(brief.contains("Brief · 0"), "{brief}");
    // The same proposal again, from another form.
    let (status, headers, _) = act(&rig, &cookie, "/web/memory", &fields).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let back = location(&headers);
    assert!(back.contains("act=brief&pending="), "{back}");
    let (_, _, pending) = send(&rig.router, get(&back, Some(&cookie))).await;
    assert!(
        pending.contains("<code>proposal.pending</code>"),
        "{pending}"
    );
    assert!(
        pending.contains("Already proposed for this brief, made"),
        "{pending}"
    );
    assert!(pending.contains("by <code>alice-web</code>"), "{pending}");
    // Ratify from the Brief, confirmed first.
    let edge = field_of(&brief, "edge_id");
    let (status, _, ask) = act(
        &rig,
        &cookie,
        "/web/workspace",
        &[
            ("action", "ratify_brief"),
            ("workspace", "alice"),
            ("edge_id", edge.as_str()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ask}");
    assert!(ask.contains("Ratify the brief proposal?"), "{ask}");
    let (status, _, after) = act_and_follow(
        &rig,
        &cookie,
        "/web/workspace",
        &[
            ("action", "ratify_brief"),
            ("workspace", "alice"),
            ("edge_id", edge.as_str()),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert!(
        after.contains("Ratified: the memory joins the brief."),
        "{after}"
    );
    assert!(after.contains("Brief · 1"), "{after}");
    assert!(after.contains("Proposals · 0"), "{after}");
}

/// P8: a held memory's page offers no Propose for brief, and its brief
/// view says why with no act.
#[tokio::test]
async fn a_held_memory_offers_no_proposal_for_a_brief_and_says_why() {
    let rig = rig().await;
    let (cookie, value) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let id = agent_writes(&rig, &value, "a held row").await;
    let (_, _, page) = send(
        &rig.router,
        get(&format!("/web/memory?id={id}"), Some(&cookie)),
    )
    .await;
    assert!(!page.contains("Propose for brief"), "{page}");
    let (_, _, view) = send(
        &rig.router,
        get(&format!("/web/memory?id={id}&act=brief"), Some(&cookie)),
    )
    .await;
    assert!(
        view.contains("Only an approved memory joins a brief"),
        "{view}"
    );
    assert!(!view.contains(r#"value="propose_brief""#), "{view}");
}

/// P9: a shared workspace where `alice-review` holds reviewer offers its
/// next status, asks before proposing, proposes once, draws the live
/// proposal with its date and key, and names the live one with its code
/// when the same move is proposed again; alice's root offers no status
/// move (ADR-0085 d17).
#[tokio::test]
async fn a_shared_workspace_proposes_its_status_after_asking_and_names_the_live_proposal_the_second_time()
 {
    let rig = rig().await;
    shared_team(&rig).await;
    let _first = signed_in(&rig, "sub-1", EMAIL).await;
    let request = Request::post("/v2/web/users/alice/capacities")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", rig.boot.admin_key.value),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", "review-alice-team")
        .body(Body::from(
            json!({"capacity": "review", "workspaces": ["team"]}).to_string(),
        ))
        .expect("a request");
    let (status, _, granted) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::OK, "{granted}");
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, root) = send(&rig.router, get("/web/workspace", Some(&cookie))).await;
    assert!(!root.contains("Propose protected"), "{root}");
    let (status, _, page) = send(
        &rig.router,
        get("/web/workspace?workspace=team", Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Propose protected"), "{page}");
    let (_, _, ask) = send(
        &rig.router,
        get(
            "/web/workspace?workspace=team&propose=protected",
            Some(&cookie),
        ),
    )
    .await;
    assert!(ask.contains("Propose protected for team?"), "{ask}");
    let fields = [
        ("action", "propose_status"),
        ("workspace", "team"),
        ("to", "protected"),
    ];
    let (status, _, after) = act_and_follow(&rig, &cookie, "/web/workspace", &fields).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert!(after.contains("Proposed: the status changes"), "{after}");
    assert!(
        after.contains("A proposal to protected is live, made"),
        "{after}"
    );
    assert!(after.contains("by <code>alice-review</code>"), "{after}");
    // While the proposal is live no second one is offered.
    assert!(!after.contains("propose=protected"), "{after}");
    let (status, headers, _) = act(&rig, &cookie, "/web/workspace", &fields).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let back = location(&headers);
    assert!(back.contains("pending="), "{back}");
    let (_, _, pending) = send(&rig.router, get(&back, Some(&cookie))).await;
    assert!(
        pending.contains(
            "<code>proposal.pending</code> A proposal to protected is already live, made"
        ),
        "{pending}"
    );
}

/// P14 and the To do group: an admin's fix in alice's root waits in
/// alice's To do under Admin fixes to confirm; its page names the act, the
/// claim and the reason, never the admin, and offers Confirm and Revert,
/// each asked first; Confirm settles it and To do says so.
#[tokio::test]
async fn an_admin_fix_waits_in_to_do_and_its_page_confirms_it_after_asking() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let (cookie, id) = alice_with_an_approved_memory(&rig).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, fixed) = act(
        &rig,
        &boss,
        "/web/admin/fixes",
        &[
            ("action", "fix"),
            ("workspace", "alice"),
            ("act", "demote"),
            ("target", id.as_str()),
            ("reason", "names the wrong branch"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{fixed}");
    let (_, _, todo) = send(&rig.router, get("/web/todo", Some(&cookie))).await;
    assert!(todo.contains("Admin fixes to confirm · 1"), "{todo}");
    assert!(todo.contains("names the wrong branch"), "{todo}");
    let at = todo.find("/web/todo/fix?id=").expect("a fix link") + "/web/todo/fix?id=".len();
    let fix = todo[at..at + 36].to_owned();
    let (status, _, page) = send(
        &rig.router,
        get(&format!("/web/todo/fix?id={fix}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(r#"<span class="oml-chip is-adminfix">unconfirmed</span>"#),
        "{page}"
    );
    assert!(page.contains("names the wrong branch"), "{page}");
    assert!(!page.contains("boss"), "{page}");
    assert!(
        page.contains("act=confirm") && page.contains("act=revert"),
        "{page}"
    );
    // Who settles it is read off the served acts, never computed: the
    // person's page says what this account may do, and the admin's pages
    // name no person it would exclude (ADR-0096 d5).
    assert!(
        page.contains("This account may confirm or revert it."),
        "{page}"
    );
    for admin_page in [
        "/web/admin/todo".to_owned(),
        "/web/admin/workspace?workspace=alice&view=fix".to_owned(),
    ] {
        let (_, _, drawn) = send(&rig.router, get(&admin_page, Some(&boss))).await;
        assert!(
            !drawn.contains("a person other than"),
            "{admin_page}: {drawn}"
        );
        assert!(
            drawn.contains("waits for a person with reviewer on the workspace"),
            "{admin_page}: {drawn}"
        );
    }
    let (_, _, ask) = send(
        &rig.router,
        get(
            &format!("/web/todo/fix?id={fix}&act=confirm"),
            Some(&cookie),
        ),
    )
    .await;
    assert!(ask.contains("Confirm the fix?"), "{ask}");
    let (status, _, after) = act_and_follow(
        &rig,
        &cookie,
        "/web/todo/fix",
        &[("action", "confirm"), ("fix_id", fix.as_str())],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert!(
        after.contains("Confirmed: the version the fix serves"),
        "{after}"
    );
    assert!(!after.contains("Admin fixes to confirm ·"), "{after}");
}

/// An admin's fix inside the admin's own person's root waits in that
/// person's To do with the acts the server serves them: the page offers
/// Confirm, and the confirmation the server records as `same_user` reads
/// "marked same user" (ADR-0096 d5; ADR-0089, the amendment of 2026-11-18).
#[tokio::test]
async fn an_admin_fix_in_the_admins_own_root_is_confirmed_by_that_person_marked_same_user() {
    tokio::time::timeout(Duration::from_secs(120), async {
        let rig = rig().await;
        rig.allow("boss@example.com", "boss", true).await;
        let (boss, value) = person_with_agent(&rig, "sub-boss", "boss@example.com").await;
        let id = agent_writes(&rig, &value, "the boss deploys on mondays")
            .await
            .to_string();
        let (status, _, page) = judge(&rig, &boss, &id, "approve", None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let (status, _, fixed) = act(
            &rig,
            &boss,
            "/web/admin/fixes",
            &[
                ("action", "fix"),
                ("workspace", "boss"),
                ("act", "demote"),
                ("target", id.as_str()),
                ("reason", "says monday, it is tuesday"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{fixed}");
        let (_, _, todo) = send(&rig.router, get("/web/todo", Some(&boss))).await;
        assert!(todo.contains("Admin fixes to confirm · 1"), "{todo}");
        let at = todo.find("/web/todo/fix?id=").expect("a fix link") + "/web/todo/fix?id=".len();
        let fix = todo[at..at + 36].to_owned();
        let (status, _, page) = send(
            &rig.router,
            get(&format!("/web/todo/fix?id={fix}"), Some(&boss)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.contains("act=confirm"), "{page}");
        assert!(
            page.contains("This account may confirm or revert it."),
            "{page}"
        );
        let (status, _, after) = act_and_follow(
            &rig,
            &boss,
            "/web/todo/fix",
            &[("action", "confirm"), ("fix_id", fix.as_str())],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{after}");
        assert!(after.contains("Confirmed, marked same user:"), "{after}");
    })
    .await
    .expect("the pages answer within two minutes");
}

/// To do's workspace filter keeps the admin fixes of the workspace it
/// names by name as by id: a fix in alice's root is listed under `alice`
/// and not under `alice.proj` (ADR-0096 d5; ADR-0083 d16).
#[tokio::test]
async fn the_todo_filter_by_a_workspace_name_keeps_only_that_workspaces_admin_fixes() {
    tokio::time::timeout(Duration::from_secs(120), async {
        let rig = rig().await;
        rig.allow("boss@example.com", "boss", true).await;
        let (cookie, id) = alice_with_an_approved_memory(&rig).await;
        let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
        let (status, _, fixed) = act(
            &rig,
            &boss,
            "/web/admin/fixes",
            &[
                ("action", "fix"),
                ("workspace", "alice"),
                ("act", "demote"),
                ("target", id.as_str()),
                ("reason", "names the wrong branch"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{fixed}");
        alice_proj(&rig, &cookie).await;
        let (status, _, here) =
            send(&rig.router, get("/web/todo?workspace=alice", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{here}");
        assert!(here.contains("Admin fixes to confirm · 1"), "{here}");
        let (status, _, elsewhere) = send(
            &rig.router,
            get("/web/todo?workspace=alice.proj", Some(&cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{elsewhere}");
        assert!(!elsewhere.contains("Admin fixes to confirm"), "{elsewhere}");
    })
    .await
    .expect("To do answers within two minutes");
}

/// The admin To do lists the admin key's open investigations, each with
/// its reason and the reads journalled under it (AdminTodo, group 2).
#[tokio::test]
async fn the_admin_to_do_lists_the_admin_keys_open_investigations() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, todo) = send(&rig.router, get("/web/admin/todo", Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{todo}");
    assert!(todo.contains("Open investigations · 1"), "{todo}");
    assert!(todo.contains("the console tests"), "{todo}");
}

/// The admin workspace page shows "Browse as <user>" only when the admin's
/// own person holds read there: absent on a shared workspace before the
/// People page's Grant read, present after it at the admin's next sign-in,
/// and it opens the Personal Memories view of that workspace, which lists
/// its approved memory (ADR-0096, the operator's decision of 2026-11-25).
#[tokio::test]
async fn the_admin_workspace_page_shows_the_browse_link_only_when_read_is_held() {
    let rig = rig().await;
    let team = shared_team(&rig).await;
    let bot = team_key(
        &rig,
        "team-bot",
        Role::Submitter,
        UsedBy::Agent,
        &[Role::Submitter],
    )
    .await;
    let memory = agent_writes_in(&rig, &bot.value, "The team ships on Fridays.", "team").await;
    let checker = team_key(
        &rig,
        "team-check",
        Role::Reviewer,
        UsedBy::Agent,
        &[Role::Reviewer, Role::Reader],
    )
    .await;
    rig.app
        .post_verdict(
            checker.value.as_str(),
            memory,
            oml_app::types::Verdict::Approve,
            None,
        )
        .await
        .expect("the team's reviewer approves");
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let admin_page = format!("/web/admin/workspace?workspace={team}");
    let (status, _, before) = send(&rig.router, get(&admin_page, Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert!(!before.contains("Browse as boss"), "{before}");
    let (status, _, people) = send(&rig.router, get("/web/admin/people", Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{people}");
    assert!(
        people.contains(r#"value="grant_read""#) && people.contains("Grant read"),
        "{people}"
    );
    let team_id = team.to_string();
    let (status, _, granted) = act(
        &rig,
        &boss,
        "/web/admin/people",
        &[
            ("action", "grant_read"),
            ("user", "boss"),
            ("workspace", team_id.as_str()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{granted}");
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, after) = send(&rig.router, get(&admin_page, Some(&boss))).await;
    let link =
        format!(r#"<a class="button" href="/web/memories?workspace={team}">Browse as boss</a>"#);
    assert!(after.contains(&link), "{after}");
    let (status, _, browsed) = send(
        &rig.router,
        get(&format!("/web/memories?workspace={team}"), Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{browsed}");
    assert!(browsed.contains("The team ships on Fridays."), "{browsed}");
}

/// Rotate on the Personal Keys tab (ADR-0096 d13): beside Cut off on a
/// live bearer key; it asks first, offering no grace, one hour or 24
/// hours; then shows the new value once with when the previous one stops,
/// and the new value authenticates the same key while the old one, in its
/// hour, still does.
#[tokio::test]
async fn the_keys_tab_rotates_a_key_after_asking_and_shows_the_new_value_once() {
    let rig = rig().await;
    let (cookie, old) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let (_, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(
        keys.contains(r#"value="rotate_key""#) && keys.contains(">Rotate<"),
        "{keys}"
    );
    let (status, _, ask) = act(
        &rig,
        &cookie,
        "/web/keys",
        &[("action", "rotate_key"), ("key", "alice-laptop")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ask}");
    assert!(ask.contains("Rotate alice-laptop?"), "{ask}");
    assert!(ask.contains("1 hour") && ask.contains("24 hours"), "{ask}");
    let (status, _, shown) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "rotate_key"),
            ("key", "alice-laptop"),
            ("grace", "3600"),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert!(shown.contains("Key rotated"), "{shown}");
    assert!(shown.contains("The previous value works until"), "{shown}");
    let new = value_of(&shown);
    assert_ne!(new, old);
    let me = rig.app.whoami(&new).await.expect("the new value");
    assert_eq!(me.key.name.as_ref().map(Name::as_str), Some("alice-laptop"));
    rig.app
        .whoami(&old)
        .await
        .expect("the old value in its hour");
    // Shown once: the page again draws the list.
    let (_, _, again) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(!again.contains(&new), "{again}");
}

/// The admin Keys tab offers no Rotate on a person's key, since an admin
/// rotates no key in a person's root, and a rotation posted anyway is
/// refused by name; the one-key page of a tenant key naming no one offers
/// Rotate, shows the value once on the admin page with its way back to that
/// key, and without a grace the old value stops at once (ADR-0096 d13, the
/// operator's decision of 2026-11-25).
#[tokio::test]
async fn the_admin_console_rotates_a_tenant_key_and_offers_no_rotate_on_a_persons_key() {
    let rig = rig().await;
    let (_alice, laptop) = person_with_agent(&rig, "sub-1", EMAIL).await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, listed) = send(&rig.router, get("/web/admin/keys?user=alice", Some(&boss))).await;
    assert!(!listed.contains(">Rotate<"), "{listed}");
    let (_, _, one) = send(
        &rig.router,
        get("/web/admin/keys?key=alice-laptop", Some(&boss)),
    )
    .await;
    assert!(!one.contains(">Rotate<"), "{one}");
    let (_, _, refused) = act_and_follow(
        &rig,
        &boss,
        "/web/admin/keys",
        &[
            ("action", "rotate_key"),
            ("key", "alice-laptop"),
            ("user", "alice"),
            ("grace", "0"),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert!(
        refused.contains("rotated by that person, from their own session"),
        "{refused}"
    );
    rig.app
        .whoami(&laptop)
        .await
        .expect("the person's key keeps its value");
    let old = rig
        .app
        .issue_key(
            &rig.boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Reader,
                name: Name::parse("team-reader").expect("a name"),
                principal: "team-reader".to_owned(),
                used_by: Some(UsedBy::Agent),
                default_workspace: None,
                crew: None,
                grants: vec![GrantRequest {
                    node: rig.boot.root_workspace_id.into(),
                    role: Role::Reader,
                    deny: Vec::new(),
                    covers: oml_app::domain::access::Covers::Subtree,
                    create: false,
                }],
                connect_code: false,
                user: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("a tenant key")
        .value;
    let (_, _, one) = send(
        &rig.router,
        get("/web/admin/keys?key=team-reader", Some(&boss)),
    )
    .await;
    assert!(one.contains(">Rotate<"), "{one}");
    let (status, _, shown) = act_and_follow(
        &rig,
        &boss,
        "/web/admin/keys",
        &[
            ("action", "rotate_key"),
            ("key", "team-reader"),
            ("grace", "0"),
            ("confirm", "yes"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert!(
        shown.contains("The previous value stopped working now."),
        "{shown}"
    );
    assert!(
        shown.contains(r#"href="/web/admin/keys?key=team-reader">Back to Keys"#),
        "{shown}"
    );
    let new = value_of(&shown);
    rig.app.whoami(&new).await.expect("the new value");
    assert!(rig.app.whoami(&old).await.is_err(), "the old value stopped");
}

// ---------------------------------------------------------------------------
// ADR-0097 d29 to d31: the admin console creates workspaces and issues keys
// ---------------------------------------------------------------------------

/// The admin Workspaces page creates a workspace anywhere in the tenant as
/// `<user>-admin`, a blank parent meaning `universe`: the form posts once
/// and answers a `303` back to the view (ADR-0097 d9, d29).
#[tokio::test]
async fn the_admin_workspaces_page_creates_a_workspace_under_universe() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(&rig.router, get("/web/admin/workspaces", Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("<h2>New workspace</h2>"), "{page}");
    assert!(page.contains("inside (blank: universe)"), "{page}");
    assert!(page.contains("as <code>boss-admin</code>"), "{page}");
    let hidden = hidden_fields(&page, "create_workspace");
    let mut fields: Vec<(&str, &str)> = hidden
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    fields.push(("name", "ops-shared"));
    fields.push(("parent", ""));
    let (status, headers, body) = send(
        &rig.router,
        post("/web/admin/workspaces", &fields, Some(&boss)),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(location(&headers), "/web/admin/workspaces?done=created");
    let created = rig
        .store
        .resolve_name(
            rig.boot.tenant_id,
            oml_app::domain::name::NamedKind::Workspace,
            &Name::parse("ops-shared").expect("a name"),
        )
        .await
        .expect("read")
        .expect("created");
    let node = rig
        .store
        .workspace(rig.boot.tenant_id, created)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(node.parent_id, Some(rig.boot.root_workspace_id));
}

/// The admin key builder: the templates, a review that refuses a draft
/// without a reason and restates a new workspace and a grant's create
/// flag in the exact request, and Issue key, which shows the value once on
/// the admin Keys page. A person who is no admin finds no such page
/// (ADR-0097 d26, d29, d30, d31).
#[tokio::test]
async fn the_admin_key_builder_issues_a_key_with_a_new_workspace_and_a_create_grant() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, _) = send(&rig.router, get("/web/admin/keys/new", Some(&cookie))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, start) = send(&rig.router, get("/web/admin/keys/new", Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{start}");
    for shape in ["one", "read", "write", "review", "new", "connector"] {
        assert!(
            start.contains(&format!(r#"href="/web/admin/keys/new?shape={shape}""#)),
            "{shape}: {start}"
        );
    }
    assert!(!start.contains("<script"), "{start}");
    let (_, _, form) = send(
        &rig.router,
        get("/web/admin/keys/new?shape=new", Some(&boss)),
    )
    .await;
    assert!(form.contains("New workspace 1"), "{form}");
    assert!(form.contains("Add a new workspace"), "{form}");
    assert!(form.contains(r#"name="why""#), "{form}");
    let draft = "/web/admin/keys/new?kind=agent&name=ops-bot&role=contributor&n=2&g0w=universe&g0r=submitter&g0c=subtree&g0k=1&g1w=universe&g1r=reader&g1c=subtree&m=1&w0p=universe&w0n=ops-space&dw=&gs=ancestors&exp=&rpm=&kb=agent";
    let (_, _, refused) = send(
        &rig.router,
        get(&format!("{draft}&step=review"), Some(&boss)),
    )
    .await;
    assert!(refused.contains(r##"<a href="#why">"##), "{refused}");
    let (_, _, review) = send(
        &rig.router,
        get(
            &format!("{draft}&why=the+ops+agent&step=review"),
            Some(&boss),
        ),
    )
    .await;
    assert!(review.contains("<h1>Review key</h1>"), "{review}");
    assert!(review.contains("&quot;create_workspaces&quot;"), "{review}");
    assert!(review.contains("&quot;create&quot;: true"), "{review}");
    assert!(review.contains("Waits on another admin"), "{review}");
    assert!(review.contains(r#"action="/web/admin/keys""#), "{review}");
    let fields = hidden_fields(&review, "build_key");
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let (status, headers, body) =
        send(&rig.router, post("/web/admin/keys", &fields, Some(&boss))).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(location(&headers), "/web/admin/keys");
    let (sealed, _) = set_cookie(&headers, ONCE_COOKIE).expect("the shown-once cookie");
    let (_, _, shown) = send(
        &rig.router,
        get(
            "/web/admin/keys",
            Some(&format!("{boss}; {ONCE_COOKIE}={sealed}")),
        ),
    )
    .await;
    assert!(shown.contains("OML_API_KEY="), "{shown}");
    assert!(shown.contains("Shown once"), "{shown}");
    key_named(&rig, "ops-bot").await;
}

// -- admin acts on the console (ADR-0097 d2, d23) ----------------------------

/// A tenant with two admin persons, alice and bob, accepted eight days ago,
/// and an agent key the boot admin key issued since, pending: it waits on a
/// second admin person.
async fn two_admins_and_a_pending_key() -> (Rig, String) {
    let rig = rig_on(MemStore::new(), true).await;
    rig.allow(EMAIL, "alice", true).await;
    rig.allow("bob@example.com", "bob", true).await;
    rig.lead_clock.advance(8 * 86_400);
    let issued = rig
        .app
        .issue_key_with_reason(
            rig.boot.admin_key.value.as_str(),
            &oml_app::types::IssueKeyRequest {
                profile: None,
                role: oml_app::domain::Role::Reader,
                name: Name::parse("acts-agent").expect("a name"),
                principal: "acts-agent".to_owned(),
                used_by: Some(oml_app::domain::UsedBy::Agent),
                default_workspace: None,
                crew: None,
                grants: vec![oml_app::types::GrantRequest {
                    node: oml_app::domain::name::Reference::parse("universe").expect("a name"),
                    role: oml_app::domain::Role::Reader,
                    deny: Vec::new(),
                    covers: oml_app::domain::access::Covers::Subtree,
                    create: false,
                }],
                user: None,
                connect_code: false,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: None,
                create_workspaces: Vec::new(),
            },
            Some("an agent for the nightly digest"),
        )
        .await
        .expect("a pending issuance");
    let act = issued.pending_act.expect("the issuance waits");
    (rig, act.to_string())
}

/// The administration To do lists the act under "Admin acts to confirm",
/// and its confirm screen shows what a confirmation applies (d23): the
/// role, the grants, `used_by`, the principal, who took it and why; the
/// confirmation applies it and the screen says so.
#[tokio::test]
async fn admin_acts_to_confirm_open_a_confirm_screen_whose_confirmation_applies_the_act() {
    let (rig, act) = two_admins_and_a_pending_key().await;
    let cookie = signed_in(&rig, "sub-alice", EMAIL).await;
    let (status, _, todo) = send(&rig.router, get("/web/admin/todo", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{todo}");
    // The pending issuance alone: the two admin flags the boot key set at
    // once under sole admin stay in the acts list and leave To do (D30,
    // decided by the operator, 2026-11-28).
    assert!(todo.contains("Admin acts to confirm · 1"), "{todo}");
    let link = format!("/web/admin/acts?act={act}");
    assert!(todo.contains(&link), "{todo}");
    let (status, _, screen) = send(&rig.router, get(&link, Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{screen}");
    for shown in [
        "issue key",
        "widening",
        "pending confirmation",
        "an agent for the nightly digest",
        "acts-agent",
        "Grants a confirmation applies",
        "reader",
        "agent",
        "Confirm",
        "Decline",
    ] {
        assert!(screen.contains(shown), "{shown}: {screen}");
    }
    for word in [" you ", " your ", " my "] {
        assert!(!screen.to_lowercase().contains(word), "{word}");
    }
    let csrf = csrf_of(&screen);
    // A confirmation carries a reason (ADR-0097 d26; amendment of
    // 2026-11-27, answer 18): one sent without is refused before any call.
    let (status, _, body) = send(
        &rig.router,
        post(
            "/web/admin/acts",
            &[
                ("csrf_token", csrf.as_str()),
                ("action", "confirm"),
                ("act", act.as_str()),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("a confirmation carries a reason"), "{body}");
    let (status, headers, body) = send(
        &rig.router,
        post(
            "/web/admin/acts",
            &[
                ("csrf_token", csrf.as_str()),
                ("action", "confirm"),
                ("act", act.as_str()),
                ("reason", "the digest is wanted"),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let location = headers[http::header::LOCATION]
        .to_str()
        .expect("a location");
    assert_eq!(location, format!("{link}&done=confirmed"));
    let (_, _, after) = send(&rig.router, get(location, Some(&cookie))).await;
    assert!(after.contains("confirmed: the act took effect"), "{after}");
    assert!(after.contains("Confirmed by"), "{after}");
    assert!(after.contains("the digest is wanted"), "{after}");
    assert!(
        !after.contains(r#"value="confirm""#),
        "no form once settled"
    );
}

/// A decline on the confirm screen carries a reason: one sent without is
/// refused before any API call, and one with it is final.
#[tokio::test]
async fn a_decline_on_the_confirm_screen_requires_a_reason_and_is_final() {
    let (rig, act) = two_admins_and_a_pending_key().await;
    let cookie = signed_in(&rig, "sub-bob", "bob@example.com").await;
    let link = format!("/web/admin/acts?act={act}");
    let (_, _, screen) = send(&rig.router, get(&link, Some(&cookie))).await;
    let csrf = csrf_of(&screen);
    let (status, _, body) = send(
        &rig.router,
        post(
            "/web/admin/acts",
            &[
                ("csrf_token", csrf.as_str()),
                ("action", "decline"),
                ("act", act.as_str()),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("a decline carries a reason"), "{body}");
    let (status, headers, body) = send(
        &rig.router,
        post(
            "/web/admin/acts",
            &[
                ("csrf_token", csrf.as_str()),
                ("action", "decline"),
                ("act", act.as_str()),
                ("reason", "the digest runs elsewhere"),
            ],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    let location = headers[http::header::LOCATION]
        .to_str()
        .expect("a location");
    let (_, _, after) = send(&rig.router, get(location, Some(&cookie))).await;
    assert!(
        after.contains("declined: the act never takes effect"),
        "{after}"
    );
    assert!(!after.contains(r#"value="decline""#), "{after}");
}

/// The admin acts list draws every act, newest first, each opening its
/// screen; a person with no admin entry is not found there.
#[tokio::test]
async fn the_admin_acts_list_draws_every_act_and_is_not_found_to_a_non_admin() {
    let (rig, act) = two_admins_and_a_pending_key().await;
    let cookie = signed_in(&rig, "sub-alice", EMAIL).await;
    let (status, _, list) = send(&rig.router, get("/web/admin/acts", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert!(list.contains("<h1>Admin acts</h1>"), "{list}");
    assert!(
        list.contains(&format!("/web/admin/acts?act={act}")),
        "{list}"
    );
    assert!(
        list.contains("allow admin"),
        "the flags are acts too: {list}"
    );
    rig.allow("carol@example.com", "carol", false).await;
    let carol = signed_in(&rig, "sub-carol", "carol@example.com").await;
    let (status, _, _) = send(&rig.router, get("/web/admin/acts", Some(&carol))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
