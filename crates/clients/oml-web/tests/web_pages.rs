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
use oml_app::domain::{HeldBy, Role};
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
    store: MemStore,
    app: Arc<TestApp>,
    /// The path router in front of the lead and `oml-web`.
    router: Router,
    boot: BootstrapResult,
    providers: MemSignIn,
    /// Every line `oml-web` wrote.
    lines: Arc<std::sync::Mutex<Vec<String>>>,
}

fn test_app(store: &MemStore) -> TestApp {
    App::new(
        store.clone(),
        MemSigner::new([7; 32]),
        FixedClock::new(),
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
    let store = MemStore::new();
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
    let lead = ApiState::new(Arc::new(test_app(&store)), store.clone(), ping, true, None)
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
    let rig = Rig {
        app: Arc::new(test_app(&store)),
        store,
        router: front(lead, web),
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

/// A signed-in browser: its cookie header.
async fn signed_in(rig: &Rig, subject: &str, email: &str) -> String {
    let (status, token, body) = sign_in_google(rig, subject, email).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    format!("{SESSION_COOKIE}={}", token.expect("a session"))
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
                workarea: None,
                derives_from: Vec::new(),
                severity: Some(oml_app::domain::guardrail::Severity::Must),
                rule: None,
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
            ("role", "writer"),
        ],
    )
    .await;
    value_of(&page)
}

// -- the Guardrails view (ADR-0037 d17), moved to oml-web ------------------

#[tokio::test]
async fn the_user_page_shows_guardrail_scope_and_selects_nested_workareas() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_guardrail(&rig, &value, "Run the tests before every commit.").await;
    let (_, _, page) = send(&rig.router, get("/web/guardrails", Some(&cookie))).await;
    assert!(page.contains("Guardrails"), "{page}");
    assert!(
        page.contains("Rules in force at <code>universe.alice</code>"),
        "{page}"
    );
    assert!(
        page.contains("This workarea and its readable parent workareas."),
        "{page}"
    );
    assert!(page.contains("<select name=\"workarea\">"), "{page}");
    // Held, it is no rule yet: the set holds none (d3); the memories view
    // lists the agents' held rows.
    assert!(page.contains("No guardrail applies here."), "{page}");
    // Approved by the person's reviewer key, it is in force.
    act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    let serve_id = format!("srv_{}", id.simple());
    let (status, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &serve_id),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let (_, _, page) = send(&rig.router, get("/web/guardrails", Some(&cookie))).await;
    assert!(
        page.contains("[must] Run the tests before every commit."),
        "{page}"
    );
    assert!(page.contains("universe.alice"), "{page}");
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[("action", "create_workarea"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[
            ("action", "create_workarea"),
            ("name", "docs"),
            ("parent", "alice.proj"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let proj = rig
        .store
        .workarea_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "proj")
        .expect("proj")
        .workarea_id;
    let docs = rig
        .store
        .workarea_children(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "docs")
        .expect("docs")
        .workarea_id;
    let (status, _, nested) = send(
        &rig.router,
        get(&format!("/web/guardrails?workarea={docs}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{nested}");
    assert!(
        nested.contains("Rules in force at <code>universe.alice.proj.docs</code>"),
        "{nested}"
    );
    assert!(
        nested.contains(&format!("value=\"{docs}\" selected")),
        "{nested}"
    );
    assert!(
        nested.contains("[must] Run the tests before every commit."),
        "the parent's root rule applies to the nested workarea: {nested}"
    );
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
async fn the_guardrail_selector_includes_workareas_after_the_first_tree_page() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for n in 0..200 {
        let name = format!("w{n:03}");
        let (status, _, body) = act(
            &rig,
            &cookie,
            "/web/workareas",
            &[("action", "create_workarea"), ("name", &name)],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{name}: {body}");
    }
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let last = rig
        .store
        .workarea_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "w199")
        .expect("the child after the first 200-node page")
        .workarea_id;
    let (status, _, page) = send(
        &rig.router,
        get(&format!("/web/guardrails?workarea={last}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(&format!("value=\"{last}\" selected")),
        "the selected workarea is present beyond the first page: {page}"
    );
}

// -- the Workareas view (ADR-0045 d28), moved to oml-web ---------------------

#[tokio::test]
async fn a_workarea_act_is_confirmed_before_it_runs() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[("action", "create_workarea"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let proj = rig
        .store
        .workarea_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "proj")
        .expect("proj")
        .workarea_id;
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[
            ("action", "create_workarea"),
            ("name", "child"),
            ("parent", "alice.proj"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send(&rig.router, get("/web/workareas", Some(&cookie))).await;
    assert!(page.contains("universe.alice.proj.child"), "{page}");
    assert!(page.contains(">alice.proj.child<"), "{page}");
    assert!(!page.contains(">alice.child<"), "{page}");
    assert!(page.contains("<select name=\"op\""), "{page}");
    assert!(page.contains("value=\"propose:committed\""), "{page}");
    assert!(page.contains("value=\"mode:in_progress\""), "{page}");
    assert!(!page.contains("value=\"propose:validated\""), "{page}");
    assert!(!page.contains("value=\"mode:standard\""), "{page}");
    // Apply is a GET that changes nothing and asks again.
    let (status, _, confirm) = send(
        &rig.router,
        get(
            &format!("/web/workareas?workarea={proj}&op=mode:in_progress"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    let node = || async {
        rig.store
            .workarea(rig.boot.tenant_id, proj)
            .await
            .unwrap()
            .unwrap()
    };
    assert!(
        !node().await.in_progress,
        "nothing ran before the confirmation"
    );
    assert!(confirm.contains("value=\"workarea_mode\""), "{confirm}");
    assert!(confirm.contains("name=\"csrf_token\""), "{confirm}");
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[
            ("action", "workarea_mode"),
            ("workarea", &proj.to_string()),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(node().await.in_progress);
    // An act the key may not take now is refused by name at the
    // confirmation, with no form to run it.
    let (status, _, refused) = send(
        &rig.router,
        get(
            &format!("/web/workareas?workarea={proj}&op=propose:committed"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{refused}");
    assert!(
        refused.contains("workarea.in_progress_refused"),
        "{refused}"
    );
    assert!(
        !refused.contains("value=\"workarea_transition\""),
        "{refused}"
    );
    // Commit, then propose committed through the dropdown's action.
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[
            ("action", "workarea_mode"),
            ("workarea", &proj.to_string()),
            ("mode", "standard"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[
            ("action", "workarea_transition"),
            ("workarea", &proj.to_string()),
            ("to", "committed"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    // The proposer is never offered the ratification; the admin is.
    let (_, _, page) = send(&rig.router, get("/web/workareas", Some(&cookie))).await;
    assert!(!page.contains("value=\"ratify:"), "{page}");
    assert!(page.contains("proposed: committed"), "{page}");
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(&rig.router, get("/web/admin/workareas", Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let marker = "value=\"ratify:";
    let at = page
        .find(marker)
        .expect("the admin is offered the ratification")
        + marker.len();
    let transition = page[at..at + 36].to_owned();
    let (status, _, page) = act(
        &rig,
        &boss,
        "/web/admin/workareas",
        &[
            ("action", "workarea_ratify"),
            ("workarea", &proj.to_string()),
            ("transition", &transition),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    assert_eq!(node().await.status.as_str(), "committed");
}

async fn agent_writes_in(rig: &Rig, value: &str, body: &str, workarea: &str) -> uuid::Uuid {
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
                workarea: Some(
                    oml_app::domain::name::Reference::parse(workarea).expect("a reference"),
                ),
                derives_from: Vec::new(),
                severity: None,
                rule: None,
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

async fn person_with_agent(rig: &Rig, subject: &str, email: &str) -> (String, String) {
    let cookie = signed_in(rig, subject, email).await;
    let (_, _, page) = act_and_follow(
        rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "writer"),
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
                workarea: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
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
            "/web/workareas",
            &[("action", "create_workarea"), ("name", name)],
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
    let _ = act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    let _ = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &format!("srv_{}", first.simple())),
            ("verdict", "approve"),
        ],
    )
    .await;
    for (body, workarea) in [
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
        agent_writes_in(&rig, &agent, body, workarea).await;
    }
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[
            ("action", "workarea_mode"),
            ("workarea", "alice.research"),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let value = |c: &str| c.split_once('=').expect("a cookie").1.to_owned();
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
        &value(&cookie),
        "--admin-cookie",
        &value(&boss),
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
                assert!(at("cards").as_u64().unwrap() >= 8, "{tag}: {report}");
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
                        .is_some_and(|u| u.starts_with(ISSUER) && u.contains("workarea=")),
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
        "/web/workareas",
        &[("action", "create_workarea"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let root = key_named(rig, "alice-web").await.user.expect("a root");
    rig.store
        .workarea_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "proj")
        .expect("proj")
        .workarea_id
}

#[tokio::test]
async fn a_resubmitted_confirmation_replays_its_act_and_acts_once() {
    // ADR-0070 d3 layer 4: the confirmation's form carries a nonce, and the
    // act's Idempotency-Key is its digest with the session token, so the
    // same form sent again replays at the API instead of acting again.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let proj = alice_proj(&rig, &cookie).await;
    let (status, _, confirm) = send(
        &rig.router,
        get(
            &format!("/web/workareas?workarea={proj}&op=propose:committed"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    let fields = [
        ("action", "workarea_transition".to_owned()),
        ("csrf_token", field_of(&confirm, "csrf_token")),
        ("workarea", field_of(&confirm, "workarea")),
        ("to", field_of(&confirm, "to")),
        ("nonce", field_of(&confirm, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..2 {
        let (status, headers, page) =
            send(&rig.router, post("/web/workareas", &fields, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        assert_eq!(headers[header::LOCATION], "/web/workareas?done=proposed");
    }
    let pending = rig
        .store
        .pending_transitions(rig.boot.tenant_id, proj)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1, "one proposal for one form: {pending:?}");
    // A fresh confirmation carries a fresh nonce.
    let (_, _, again) = send(
        &rig.router,
        get(
            &format!("/web/workareas?workarea={proj}&op=mode:in_progress"),
            Some(&cookie),
        ),
    )
    .await;
    assert_ne!(field_of(&again, "nonce"), field_of(&confirm, "nonce"));
    // The admin's ratification, sent twice, ratifies once and lands on the
    // view both times: without the replay the second would find the
    // proposal no longer live.
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let transition = pending[0].transition_id;
    let (status, _, confirm) = send(
        &rig.router,
        get(
            &format!("/web/admin/workareas?workarea={proj}&op=ratify:{transition}"),
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    let fields = [
        ("action", "workarea_ratify".to_owned()),
        ("csrf_token", field_of(&confirm, "csrf_token")),
        ("workarea", proj.to_string()),
        ("transition", field_of(&confirm, "transition")),
        ("nonce", field_of(&confirm, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..2 {
        let (status, headers, page) = send(
            &rig.router,
            post("/web/admin/workareas", &fields, Some(&boss)),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        assert_eq!(
            headers[header::LOCATION],
            "/web/admin/workareas?done=ratified"
        );
    }
    let node = rig
        .store
        .workarea(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(node.status.as_str(), "committed");
}

#[tokio::test]
async fn a_confirmation_sent_twice_at_once_acts_once() {
    // A double click sends the same form twice before either answers; the
    // second waits for the first and replays it (ADR-0070 d3 layer 4). Two
    // processes of oml-web racing the same key remain #233's case.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let proj = alice_proj(&rig, &cookie).await;
    let (_, _, confirm) = send(
        &rig.router,
        get(
            &format!("/web/workareas?workarea={proj}&op=propose:committed"),
            Some(&cookie),
        ),
    )
    .await;
    let fields = [
        ("action", "workarea_transition".to_owned()),
        ("csrf_token", field_of(&confirm, "csrf_token")),
        ("workarea", proj.to_string()),
        ("to", "committed".to_owned()),
        ("nonce", field_of(&confirm, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..5 {
        let (a, b) = tokio::join!(
            send(&rig.router, post("/web/workareas", &fields, Some(&cookie))),
            send(&rig.router, post("/web/workareas", &fields, Some(&cookie))),
        );
        for (status, headers, page) in [a, b] {
            assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
            assert_eq!(headers[header::LOCATION], "/web/workareas?done=proposed");
        }
    }
    let pending = rig
        .store
        .pending_transitions(rig.boot.tenant_id, proj)
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
            "/web/workareas",
            &[
                ("action", "workarea_mode"),
                ("workarea", &proj.to_string()),
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
        .workarea(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert!(!node.in_progress, "nothing ran");
}

#[tokio::test]
async fn the_workareas_view_shows_only_the_notices_its_acts_name() {
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
            get(&format!("/web/workareas?done={done}"), Some(&cookie)),
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
            "/web/workareas",
            &[
                ("action", "workarea_mode"),
                ("workarea", &proj.to_string()),
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
        .workarea(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert!(!node.in_progress, "nothing ran");
}

#[tokio::test]
async fn the_admin_workareas_page_is_not_found_for_a_person_who_is_not_an_admin() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = send(&rig.router, get("/web/admin/workareas", Some(&cookie))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/admin/workareas",
        &[
            ("action", "workarea_mode"),
            ("workarea", "alice"),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{page}");
}

#[tokio::test]
async fn the_admin_workareas_page_refuses_a_create_as_no_action_of_its_own_and_creates_nothing() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = act(
        &rig,
        &admin,
        "/web/admin/workareas",
        &[("action", "create_workarea"), ("name", "shadow")],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{page}");
    assert!(page.contains("request.validation"), "{page}");
    assert!(
        page.contains("action is not one of this page&#39;s actions")
            || page.contains("action is not one of this page's actions"),
        "{page}"
    );
    let root = key_named(&rig, "boss-web").await.user.expect("a root");
    let children = rig
        .store
        .workarea_children(rig.boot.tenant_id, root)
        .await
        .unwrap();
    assert!(
        children.iter().all(|w| w.name.as_str() != "shadow"),
        "nothing was created"
    );
}

#[tokio::test]
async fn the_old_workareas_urls_are_not_found_and_their_acts_run_nothing() {
    // The lead's 303s from the old URLs are deleted in v0.7.1 (ADR-0070,
    // "The old URLs deleted"): an old URL is not found, an old act is
    // refused and runs nothing, and the moved page answers at its own path.
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let proj = alice_proj(&rig, &cookie).await;
    for (old, who, new) in [
        ("/web?view=workarea", &cookie, "/web/workareas"),
        ("/web/admin?view=workarea", &boss, "/web/admin/workareas"),
    ] {
        let (status, headers, _) = send(&rig.router, get(old, Some(who))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
        let (status, _, page) = send(&rig.router, get(new, Some(who))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.contains("<h1>Workareas</h1>"), "{page}");
    }
    // An old form's act is no action of POST /web and runs nothing.
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web",
        &[
            ("action", "workarea_mode"),
            ("workarea", &proj.to_string()),
            ("mode", "in_progress"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(headers.get(header::LOCATION).is_none());
    let node = rig
        .store
        .workarea(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert!(!node.in_progress);
    // An old form's create runs nothing either: creating a workarea is the
    // view's own act through the API.
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web",
        &[("action", "create_workarea"), ("name", "docs")],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(headers.get(header::LOCATION).is_none());
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let docs = || async {
        rig.store
            .workarea_children(rig.boot.tenant_id, root)
            .await
            .unwrap()
            .into_iter()
            .any(|w| w.name.as_str() == "docs")
    };
    assert!(!docs().await, "the lead created nothing");
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web/workareas",
        &[("action", "create_workarea"), ("name", "docs")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/workareas?done=created");
    assert!(docs().await);
}

#[tokio::test]
async fn the_workareas_pages_draw_the_header_and_tabs_the_leads_pages_draw() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    for (subject, email) in [("sub-1", EMAIL), ("sub-2", "bob@example.com")] {
        let cookie = signed_in(&rig, subject, email).await;
        let (_, _, lead) = send(&rig.router, get("/web/setup", Some(&cookie))).await;
        let (status, _, moved) = send(&rig.router, get("/web/workareas", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(header_of(&moved), header_of(&lead), "{email}");
        assert!(
            moved.contains(r#"<a href="/web/workareas" class="on" aria-current="page">"#),
            "{moved}"
        );
    }
    let boss = signed_in(&rig, "sub-2", "bob@example.com").await;
    let (_, _, lead) = send(&rig.router, get("/web/admin/people", Some(&boss))).await;
    let (status, _, moved) = send(&rig.router, get("/web/admin/workareas", Some(&boss))).await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(header_of(&moved), header_of(&lead));
    assert!(
        moved.contains(r#"<a href="/web/admin/workareas" class="on" aria-current="page">"#),
        "{moved}"
    );
}

// -- the hop itself ------------------------------------------------------------

/// The page header with the active tab's marker removed.
fn header_of(page: &str) -> String {
    let start = page.find(r#"<header class="top">"#).expect("a header");
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
            send(&rig.router, get("/web/guardrails", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(header_of(&moved), header_of(&lead), "{email}");
        assert!(
            moved.contains(r#"<a href="/web/guardrails" class="on" aria-current="page">"#),
            "{moved}"
        );
        // The page came over the hop: oml-web's own policy, no provider.
        let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(!csp.contains("accounts.google.com"), "{csp}");
    }
}

#[tokio::test]
async fn the_old_guardrails_url_is_not_found_and_the_view_answers_at_its_own_path() {
    // ADR-0070 d7 (b) as amended: each moved page has its own path under
    // /web/; the lead's 303 from the old ?view= form is deleted in v0.7.1
    // (ADR-0070, "The old URLs deleted").
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for old in ["/web?view=guardrails", "/web?view=guardrails&workarea=x"] {
        let (status, headers, _) = send(&rig.router, get(old, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
    }
    let (status, _, page) = send(&rig.router, get("/web/guardrails", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Rules in force at"), "{page}");
}

#[tokio::test]
async fn a_moved_page_without_a_session_goes_to_the_leads_sign_in() {
    let rig = rig().await;
    let (status, headers, _) = send(&rig.router, get("/web/guardrails", None)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert_eq!(location, "/web/sign-in?next=%2Fweb%2Fguardrails");
    let (status, _, page) = send(&rig.router, get(&location, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Sign in with Google"), "{page}");
    // The lead's sign-in carries the page on, to come back to it.
    assert!(
        page.contains(r#"name="next" value="/web/guardrails""#),
        "{page}"
    );
}

#[tokio::test]
async fn a_signed_out_session_cookie_goes_to_sign_in_from_a_moved_page() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, _) = act(&rig, &cookie, "/web", &[("action", "sign_out")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, headers, _) = send(&rig.router, get("/web/guardrails", Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION],
        "/web/sign-in?next=%2Fweb%2Fguardrails"
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
                workarea: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
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
            ("role", "writer"),
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
    let token_value = cookie.split_once('=').map_or("", |(_, v)| v);
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
    let (status, _, _) = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &format!("srv_{}", alices.simple())),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (action, workarea) in [
        ("grant_curation", "alice"),
        ("grant_review", "universe"),
        ("grant_review", "alice"),
    ] {
        let (status, _, body) = act(
            &rig,
            &admin,
            "/web/admin/people",
            &[("action", action), ("user", "boss"), ("workarea", workarea)],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}: {body}");
    }
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
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
        )
        .await
        .expect("the admin reads")
        .items[0]
        .memory
        .memory_id;
    // boss-review is outside the curated claim's lineage and approves it.
    let (status, _, _) = act_and_follow(
        &rig,
        &admin,
        "/web/admin/review",
        &[
            ("action", "verdict"),
            ("serve_id", &format!("srv_{}", curated.simple())),
            ("verdict", "approve"),
        ],
    )
    .await;
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
            ("role", "writer"),
        ],
    )
    .await;
    let value = value_of(&issued);
    let held = agent_writes(&rig, &value, "Kilimanjaro budget held for review").await;
    let promoted = agent_writes(&rig, &value, "Tanganyika vendor shortlist").await;
    let (status, _, _) = act_and_follow(
        &rig,
        &ada,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &format!("srv_{}", promoted.simple())),
            ("verdict", "approve"),
        ],
    )
    .await;
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
    let (status, _, review) =
        act_and_follow(&rig, &bob, "/web/review", &[("action", "review_next")]).await;
    assert_eq!(status, StatusCode::OK, "{review}");
    for secret in ["Kilimanjaro", "Tanganyika", "flowchart"] {
        assert!(!review.contains(secret), "{review}");
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
    let (status, _, _) = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &format!("srv_{}", id.simple())),
            ("verdict", "approve"),
        ],
    )
    .await;
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
            ("workarea", "alice"),
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
        page.contains("<script>") && page.contains("Issue my credential"),
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
    let token = cookie.split_once('=').map_or("", |(_, v)| v);
    let form_key = oml_web_credential::act_idempotency_key(token, nonce);
    let attempt: String =
        ring::digest::digest(&ring::digest::SHA256, format!("{form_key}\n2").as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
    let body = json!({"name": "alice-claude-code-2", "role": "writer", "connect_code": false});
    let mut bytes = b"\n".to_vec();
    bytes.extend_from_slice(body.to_string().as_bytes());
    let web_key = key_named(&rig, "alice-web").await;
    let scope = oml_app::ports::IdempotencyScope {
        tenant_id: rig.boot.tenant_id,
        key_id: web_key.key_id,
        workarea_id: uuid::Uuid::nil(),
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
        page.contains("Your deletion request is with the administrator"),
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
            ("role", "writer"),
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
    for (name, role) in [("review", "writer"), ("phone", "dreamer")] {
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
            ("role", "writer"),
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
            ("role", "writer"),
        ],
    )
    .await;
    let value = value_of(&page);
    let memory = agent_writes(&rig, &value, "the deploy window is tuesday").await;
    let (_, _, page) = send(&rig.router, get("/web/memories", Some(&cookie))).await;
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
    // Review on the user page acts as alice-review, which is not the author.
    let (status, _, page) =
        act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("the deploy window is tuesday"), "{page}");
    let serve_id = field_of(&page, "serve_id");
    let (status, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &serve_id),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("is now promoted"), "{page}");
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
            ("role", "writer"),
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
async fn review_on_the_admin_page_acts_as_the_persons_reviewer_key_and_the_admin_key_is_never_offered_a_verdict()
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
            ("role", "writer"),
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
            ("workarea", "alice"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _, page) = act_and_follow(
        &rig,
        &admin,
        "/web/admin/review",
        &[("action", "review_next")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("boss reviews this"), "{page}");
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
            ("role", "writer"),
        ],
    )
    .await;
    let value = value_of(&page);
    let source = agent_writes(&rig, &value, "rotate keys every ninety days").await;
    // Only an approved memory is readable beyond its author: alice approves
    // her agent's write first.
    let serve = format!("srv_{}", source.simple());
    let (status, _, _) = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &serve),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (action, workarea) in [("grant_curation", "alice"), ("grant_review", "universe")] {
        let (status, _, body) = act(
            &rig,
            &admin,
            "/web/admin/people",
            &[("action", action), ("user", "boss"), ("workarea", workarea)],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}: {body}");
    }
    // A curator designation takes effect at the next sign-in: the session's
    // capacity credentials are those its entry reached when it opened
    // (ADR-0070 d2 as built).
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
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
        )
        .await
        .expect("the admin reads");
    let curated = &page.items[0];
    assert_eq!(curated.state.as_str(), "held", "held until reviewed");
    assert_eq!(curated.workarea_path, "universe");
    assert_eq!(curated.memory.author_key_id, curate.key_id);
    // boss-review is outside the curated claim's lineage, so it is served.
    let (_, _, page) = act_and_follow(
        &rig,
        &admin,
        "/web/admin/review",
        &[("action", "review_next")],
    )
    .await;
    assert!(page.contains("Keys rotate quarterly."), "{page}");
    let serve_id = format!("srv_{}", curated.memory.memory_id.simple());
    let (status, _, page) = act_and_follow(
        &rig,
        &admin,
        "/web/admin/review",
        &[
            ("action", "verdict"),
            ("serve_id", &serve_id),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    // The admin read shows its source and author; alice sees it anonymous.
    let read = rig
        .app
        .admin_read(rig.boot.admin_key.value.as_str(), curated.memory.memory_id)
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
            ("role", "writer"),
        ],
    )
    .await;
    let value = value_of(&page);
    let source = agent_writes(&rig, &value, "rotate keys every ninety days").await;
    // Only an approved memory is readable beyond its author: alice approves
    // her agent's write first.
    let serve = format!("srv_{}", source.simple());
    let (status, _, _) = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &serve),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (action, workarea) in [("grant_curation", "alice"), ("grant_review", "universe")] {
        let (status, _, body) = act(
            &rig,
            &admin,
            "/web/admin/people",
            &[("action", action), ("user", "boss"), ("workarea", workarea)],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}: {body}");
    }
    // A curator designation takes effect at the next sign-in: the session's
    // capacity credentials are those its entry reached when it opened
    // (ADR-0070 d2 as built).
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, body) = act(
        &rig,
        &admin,
        "/web/admin/curate",
        &[
            ("action", "curate"),
            ("source", &source.to_string()),
            ("text", "Keys rotate quarterly. [needs-human]"),
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
        )
        .await
        .expect("the admin reads");
    let curated = &page.items[0];
    assert_eq!(curated.state.as_str(), "held", "held until reviewed");
    assert_eq!(curated.workarea_path, "universe");
    assert_eq!(curated.memory.author_key_id, curate.key_id);
    // boss-review is outside the curated claim's lineage, so it is served.
    let (_, _, page) = act_and_follow(
        &rig,
        &admin,
        "/web/admin/review",
        &[("action", "review_next")],
    )
    .await;
    assert!(page.contains("Keys rotate quarterly."), "{page}");
    let serve_id = format!("srv_{}", curated.memory.memory_id.simple());
    let (status, _, page) = act_and_follow(
        &rig,
        &admin,
        "/web/admin/review",
        &[
            ("action", "verdict"),
            ("serve_id", &serve_id),
            ("verdict", "approve"),
        ],
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
            ("role", "writer"),
            ("guardrail_scope", "workarea"),
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
async fn the_review_page_shows_a_held_guardrails_load_warning() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    agent_guardrail(&rig, &value, "Run the tests before every commit.").await;
    let (status, _, page) =
        act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains("Run the tests before every commit."),
        "{page}"
    );
    assert!(
        page.contains("every key that reads this workarea will load this rule"),
        "{page}"
    );
}

/// A reviewer sees what a verdict makes true: the held version's own valid
/// time and, on a supersession, the end approving it gives the version it
/// replaces, on the review card and in the bulk table (ADR-0059
/// d2, d4).
#[tokio::test]
async fn a_reviewer_sees_the_valid_time_and_the_end_an_approval_gives_on_the_card_and_the_bulk_table()
 {
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
                workarea: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
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
    let (_, _, page) =
        act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    assert!(page.contains("valid from 2025, open"), "{page}");
    act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &format!("srv_{}", v1.simple())),
            ("verdict", "approve"),
        ],
    )
    .await;
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
                workarea: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
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
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    act(
        &rig,
        &admin,
        "/web/admin/people",
        &[
            ("action", "grant_review"),
            ("user", "boss"),
            ("workarea", "alice"),
        ],
    )
    .await;
    let (_, _, page) = send(&rig.router, get("/web/admin/review", Some(&admin))).await;
    assert!(page.contains("the api is at version 4"), "{page}");
    assert!(page.contains("<th>Valid</th>"), "{page}");
    assert!(
        page.contains(
            "from 2026-06-01, open; approving ends version 1 at 2026-06-01T00:00:00.000000Z"
        ),
        "{page}"
    );
    let (_, _, page) =
        act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    assert!(page.contains("the api is at version 4"), "{page}");
    assert!(page.contains("valid from 2026-06-01, open"), "{page}");
    assert!(
        page.contains("approving ends version 1 at 2026-06-01T00:00:00.000000Z"),
        "{page}"
    );
}

/// The review card names every finding stored on the version, one line
/// each with its verdict, its class and rule and where it fired, and the
/// bulk table counts them by verdict (ADR-0074 d17, d18 phase 1, d23). The
/// double holds on its marker in the body and in the title alike, so the
/// version carries two findings where the single stored `scan_finding`
/// named one.
#[tokio::test]
async fn the_review_card_names_every_scan_finding() {
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
                body: "the deploy note [needs-human] in the body".to_owned(),
                workarea: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                valid_from: None,
                valid_until: None,
                format: None,
                title: Some("[needs-human] in the title".to_owned()),
                on_similar: Default::default(),
            },
        )
        .await
        .expect("the agent writes");
    // The bulk table first: a serve claims the item for its reviewer.
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    act(
        &rig,
        &admin,
        "/web/admin/people",
        &[
            ("action", "grant_review"),
            ("user", "boss"),
            ("workarea", "alice"),
        ],
    )
    .await;
    let (_, _, page) = send(&rig.router, get("/web/admin/review", Some(&admin))).await;
    assert!(page.contains("<th>Findings</th>"), "{page}");
    assert!(
        page.contains(r#"<td class="mono findings">2 hold</td>"#),
        "{page}"
    );
    let (status, _, page) =
        act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
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
async fn the_review_card_shows_the_source_the_format_and_what_it_will_be_found_by() {
    let rig = rig().await;
    let (cookie, memory) = person_with_a_diagram(&rig).await;
    let (_, _, page) =
        act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
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
            ("role", "writer"),
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

#[tokio::test]
async fn an_issue_sent_twice_at_once_issues_one_key_and_the_surviving_cookie_opens_to_its_value() {
    // A double click sends the same form twice before either answers: the
    // second waits for the first and replays it without the value, so it
    // sets no cookie over the first's (ADR-0070 d3 layer 4).
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    let fields = [
        ("action", "issue_key".to_owned()),
        ("csrf_token", csrf_of(&page)),
        ("name", "laptop".to_owned()),
        ("role", "writer".to_owned()),
        ("nonce", field_of(&page, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
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
            ("role", "writer"),
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
            ("role", "writer"),
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
async fn a_bulk_confirmation_records_nothing_and_its_verdict_lands_on_the_view_and_replays() {
    // Bulk review is the admin page's (ADR-0055 d1): an admin reviews their
    // own agent's writes, marked as by the same user.
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let value = laptop_key(&rig, &cookie).await;
    let a = agent_writes(&rig, &value, "Decision one.").await;
    let b = agent_writes(&rig, &value, "Decision two.").await;
    let (status, _, page) = send(&rig.router, get("/web/admin/review", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(&format!(r#"name="pick_000" value="{a}">"#)),
        "{page}"
    );
    assert!(page.contains("(you)"), "{page}");
    let (status, _, confirm) = act(
        &rig,
        &cookie,
        "/web/admin/review",
        &[
            ("action", "bulk_confirm"),
            ("verdict", "approve"),
            ("reason", ""),
            ("pick_000", &a.to_string()),
            ("pick_001", &b.to_string()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    assert!(confirm.contains("Approve 2 memories?"), "{confirm}");
    assert!(confirm.contains("Decision two."), "{confirm}");
    let held = rig
        .store
        .memory(rig.boot.tenant_id, a)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        held.status.as_str(),
        "held",
        "the confirmation records nothing"
    );
    let form = [
        ("action", "bulk_verdict"),
        ("verdict", "approve"),
        ("reason", ""),
        ("artifact_ids", &field_of(&confirm, "artifact_ids")),
        ("nonce", &field_of(&confirm, "nonce")),
    ];
    let (status, headers, _) = act(&rig, &cookie, "/web/admin/review", &form).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert!(
        location.starts_with("/web/admin/review?done=bulk"),
        "{location}"
    );
    let (_, _, page) = send(&rig.router, get(&location, Some(&cookie))).await;
    assert!(
        page.contains("2 approved, 0 rejected, 0 skipped."),
        "{page}"
    );
    assert!(page.contains("same user"), "{page}");
    let (_, again, _) = act(&rig, &cookie, "/web/admin/review", &form).await;
    let location = again[header::LOCATION].to_str().unwrap().to_owned();
    let (_, _, page) = send(&rig.router, get(&location, Some(&cookie))).await;
    assert!(
        page.contains("2 replayed"),
        "a resubmitted form replays: {page}"
    );
    let approvals = rig
        .store
        .events_range(rig.boot.tenant_id, 1, 1_000)
        .await
        .unwrap()
        .iter()
        .filter(|e| e.envelope.event_type == "review.approve")
        .count();
    assert_eq!(approvals, 2, "acted once");
}

#[tokio::test]
async fn the_persons_review_page_holds_no_bulk_review_and_refuses_its_acts() {
    // Bulk review is the admin page's only (ADR-0055 d1); the person's page
    // holds review next (ADR-0070, Phase (b) as built).
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "Decision one.").await;
    let (status, _, page) = send(&rig.router, get("/web/review", Some(&cookie))).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Review next"), "{page}");
    assert!(!page.contains("Bulk review"), "{page}");
    assert!(!page.contains("pick_000"), "{page}");
    let id = memory.to_string();
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
        let (status, _, page) = act(&rig, &cookie, "/web/review", &fields).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{page}");
        assert!(page.contains("not one of this page"), "{page}");
    }
    let held = rig
        .store
        .memory(rig.boot.tenant_id, memory)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.status.as_str(), "held");
}

#[tokio::test]
async fn the_served_card_is_read_by_the_memory_the_redirect_names_never_by_the_serve_id() {
    // The serve id is opaque in the contract (openapi.yaml `serve_id`): the
    // redirect names the served memory beside it, and the view reads the
    // card by that alone.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let memory = agent_writes(&rig, &value, "the deploy window is tuesday").await;
    let (status, headers, _) =
        act(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert_eq!(
        query_param(&location, "memory").as_deref(),
        Some(memory.to_string().as_str()),
        "{location}"
    );
    let serve_id = query_param(&location, "serve").expect("the serve id");
    let (status, _, page) = send(
        &rig.router,
        get(
            &format!("/web/review?serve=an-opaque-serve&memory={memory}"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("the deploy window is tuesday"), "{page}");
    assert_eq!(field_of(&page, "serve_id"), "an-opaque-serve");
    let (status, _, page) = send(
        &rig.router,
        get(&format!("/web/review?serve={serve_id}"), Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no memory named: {page}");
}

#[tokio::test]
async fn a_resubmitted_verdict_replays_its_claim_of_the_next_item_too() {
    // The claim after a verdict runs under a key derived from the verdict's
    // nonce, so a double submit lands on the same next item.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    for body in ["Decision one.", "Decision two.", "Decision three."] {
        agent_writes(&rig, &value, body).await;
    }
    let (_, _, page) =
        act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    let fields = [
        ("action", "verdict".to_owned()),
        ("csrf_token", csrf_of(&page)),
        ("serve_id", field_of(&page, "serve_id")),
        ("verdict", "approve".to_owned()),
        ("nonce", field_of(&page, "nonce")),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let mut locations = Vec::new();
    for _ in 0..2 {
        let (status, headers, page) =
            send(&rig.router, post("/web/review", &fields, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        locations.push(headers[header::LOCATION].to_str().unwrap().to_owned());
    }
    assert!(locations[0].contains("&done=verdict"), "{locations:?}");
    assert_eq!(locations[0], locations[1]);
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
            ("workarea", "alice"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, headers, page) = act_and_follow(
        &rig,
        &admin,
        "/web/admin/review",
        &[("action", "review_next")],
    )
    .await;
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
    let memory = agent_writes(&rig, &value, "Not served to anyone yet.").await;
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            (
                "serve_id",
                &format!(
                    "srv_{}",
                    uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef).simple()
                ),
            ),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert_ne!(status, StatusCode::SEE_OTHER, "{page}");
    assert!(page.contains("access.not_found"), "{page}");
    let held = rig
        .store
        .memory(rig.boot.tenant_id, memory)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.status.as_str(), "held");
}

#[tokio::test]
async fn the_admin_review_and_keys_pages_are_not_found_for_a_person_who_is_not_an_admin() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for path in ["/web/admin/review", "/web/admin/keys"] {
        let (status, _, page) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {page}");
        assert!(page.contains("access.not_found"), "{page}");
        let (status, _, _) = act(&rig, &cookie, path, &[("action", "cut_off"), ("key", "x")]).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
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

#[tokio::test]
async fn the_old_review_and_keys_urls_are_not_found_and_their_acts_run_nothing() {
    // The lead's 303s from the old URLs are deleted in v0.7.1 (ADR-0070,
    // "The old URLs deleted"): an old URL is not found, an old act is
    // refused and runs nothing, and the moved page answers at its own path.
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
        ("/web?view=review", "/web/review"),
        ("/web/admin?view=review", "/web/admin/review"),
        ("/web/admin?view=held&select=all", "/web/admin/review"),
        ("/web/admin?view=queue", "/web/admin/review"),
    ] {
        let (status, headers, _) = send(&rig.router, get(old, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{old}");
        assert!(headers.get(header::LOCATION).is_none(), "{old}");
        let (status, _, page) = send(&rig.router, get(new, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{new}: {page}");
    }
    for (page, action, refused) in [
        ("/web", "issue_key", StatusCode::BAD_REQUEST),
        ("/web", "review_next", StatusCode::BAD_REQUEST),
        ("/web/admin", "bulk_verdict", StatusCode::NOT_FOUND),
        ("/web/admin", "cut_off", StatusCode::NOT_FOUND),
    ] {
        let (status, headers, _) = act(
            &rig,
            &cookie,
            page,
            &[("action", action), ("name", "laptop"), ("role", "writer")],
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
}

#[tokio::test]
async fn the_review_and_keys_pages_draw_the_header_and_tabs_the_leads_pages_draw() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    let cookie = signed_in(&rig, "sub-2", "bob@example.com").await;
    let (_, _, lead) = send(&rig.router, get("/web/setup", Some(&cookie))).await;
    for path in ["/web/review", "/web/keys"] {
        let (status, _, moved) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(header_of(&moved), header_of(&lead), "{path}");
    }
    let (_, _, keys) = send(&rig.router, get("/web/keys", Some(&cookie))).await;
    assert!(
        keys.contains("bob-web") && keys.contains("bob-review"),
        "the person's session keys are listed: {keys}"
    );
    let (_, _, lead) = send(&rig.router, get("/web/admin/people", Some(&cookie))).await;
    for path in ["/web/admin/review", "/web/admin/keys"] {
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
}

// -- the Memories list and the Atlas (ADR-0045 d22-d32), moved to oml-web ----

// ---------------------------------------------------------------------------
// The Memories list, the Atlas view and the workarea dropdowns (ADR-0045
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
    let mut path = "/web/memories?limit=3".to_owned();
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
    let (_, _, page) = send(&rig.router, get("/web/memories?limit=3", Some(&cookie))).await;
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
        .create_workarea(
            &boot.admin_key.value,
            &Name::parse("team").expect("a name"),
            Some(boot.root_workarea_id.into()),
            None,
        )
        .await
        .expect("a workarea")
        .workarea
        .workarea_id;
    let key = rig
        .app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Writer,
                name: Name::parse("rival-agent").expect("a name"),
                principal: "rival-agent".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: Some(team.into()),
                crew: None,
                grants: vec![GrantRequest {
                    node: team.into(),
                    role: Role::Writer,
                    deny: Vec::new(),
                    covers: oml_app::domain::access::Covers::Subtree,
                }],
                connect_code: false,
                user: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
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
    // A tenant agent an admin granted into alice's root writes there; its
    // held row is another author's.
    let helper = rig
        .app
        .issue_key(
            &rig.boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Writer,
                name: Name::parse("helper").expect("a name"),
                principal: "helper".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: None,
                crew: None,
                grants: vec![GrantRequest {
                    node: Name::parse("alice").expect("a name").into(),
                    role: Role::Writer,
                    deny: Vec::new(),
                    covers: oml_app::domain::access::Covers::Subtree,
                }],
                connect_code: false,
                user: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
            },
        )
        .await
        .expect("a tenant agent on alice's root");
    agent_writes(&rig, &helper.value, "helper held outside").await;
    for path in ["/web/memories", "/web/memories?state=held"] {
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
    // Inside an in-progress workarea every author's held row is served.
    let (status, _, page) = act(
        &rig,
        &alice,
        "/web/workareas",
        &[("action", "create_workarea"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    put_in_progress(&rig, &alice, "alice.proj").await;
    rig.app
        .submit_memory(
            helper.value.as_str(),
            &oml_app::types::SubmitRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                idempotency_key: "k-inside".to_owned(),
                claim_key: None,
                verb: Some(oml_app::types::SubmitVerb::Remember),
                kind: oml_app::domain::MemoryKind::Memory,
                body: "helper held inside".to_owned(),
                workarea: Some(
                    oml_app::domain::name::Reference::parse("alice.proj").expect("a reference"),
                ),
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                valid_from: None,
                valid_until: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("the helper writes inside");
    let (_, _, page) = send(
        &rig.router,
        get("/web/memories?workarea=alice.proj", Some(&alice)),
    )
    .await;
    assert!(page.contains("helper held inside"), "{page}");
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
    for (cookie, page_path, data_path) in [
        (&alice, "/web/memories", "/web/data/memories"),
        (&boss, "/web/admin/memories", "/web/admin/data/memories"),
    ] {
        let mut query = "limit=2&state=held".to_owned();
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
                            "workarea",
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
    assert!(body.contains("web_is_human_only"), "{body}");
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
    let (_, _, page) = send(&rig.router, get("/web/admin/memories?limit=2", Some(&boss))).await;
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
        "/web/workareas",
        &[("action", "create_workarea"), ("name", "proj")],
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
        .workarea_children(
            rig.boot.tenant_id,
            key_named(&rig, "alice-web").await.user.unwrap(),
        )
        .await
        .unwrap()
        .into_iter()
        .find(|w| w.name.as_str() == "proj")
        .unwrap()
        .workarea_id;
    assert!(
        page.contains(&format!(
            "<a href=\"/web/atlas?workarea={proj}\" aria-label=\"proj, 1 readable: 1 held\"><title>proj, 1 readable: 1 held</title>"
        )),
        "{page}"
    );
    assert!(page.contains("id=\"atlas-table\""), "{page}");
    assert!(page.contains("<td class=\"num\">1</td>"), "{page}");
    assert!(page.contains("aria-label=\"Breadcrumb\""), "{page}");
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
    // Workareas view's.
    let (_, _, zoomed) = send(
        &rig.router,
        get(&format!("/web/atlas?workarea={proj}"), Some(&cookie)),
    )
    .await;
    assert!(
        zoomed.contains("<span aria-current=\"page\">proj</span>"),
        "{zoomed}"
    );
    assert!(zoomed.contains("<select name=\"op\""), "{zoomed}");
    // Apply asks the Workareas view, which oml-web serves at its own path
    // (ADR-0070 d7 (b)).
    assert!(
        zoomed.contains(r#"<form class="row act" method="get" action="/web/workareas">"#),
        "{zoomed}"
    );
    // The list links to the Atlas of the same page, filters kept.
    let (_, _, list) = send(&rig.router, get("/web/memories?state=held", Some(&cookie))).await;
    assert!(list.contains("href=\"/web/atlas?state=held\""), "{list}");
}

#[tokio::test]
async fn the_filters_narrow_the_list() {
    let rig = rig().await;
    let (cookie, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let held = agent_writes(&rig, &agent, "a held kestrel").await;
    let approved = agent_writes(&rig, &agent, "an approved kestrel").await;
    // Review serves the oldest first, and a verdict claims the next item.
    let (_, _, page) =
        act_and_follow(&rig, &cookie, "/web/review", &[("action", "review_next")]).await;
    assert!(page.contains("a held kestrel"), "{page}");
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &field_of(&page, "serve_id")),
            ("verdict", "reject"),
            ("reason", "not this one"),
        ],
    )
    .await;
    assert!(page.contains("an approved kestrel"), "{page}");
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/review",
        &[
            ("action", "verdict"),
            ("serve_id", &field_of(&page, "serve_id")),
            ("verdict", "approve"),
        ],
    )
    .await;
    assert!(page.contains("is now promoted"), "{page}");
    let third = agent_writes(&rig, &agent, "still held kestrel").await;
    // A second agent of alice's, to filter by author.
    let (_, _, page) = act_and_follow(
        &rig,
        &cookie,
        "/web/keys",
        &[
            ("action", "issue_key"),
            ("name", "phone"),
            ("role", "writer"),
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
    for (query, expected) in [
        ("state=held", set(&[third, by_phone])),
        ("state=promoted", set(&[approved])),
        ("state=rejected", set(&[held])),
        ("author=alice-phone", set(&[by_phone])),
        ("author=alice-laptop&state=held", set(&[third])),
        ("origin=chat", std::collections::BTreeSet::new()),
        ("origin=manual&state=held", set(&[third, by_phone])),
        ("q=kestrel", set(&[approved, third])),
        ("q=kestrel&state=held", set(&[third])),
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
        get("/web/admin/memories?author=alice-phone", Some(&boss)),
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
            "/web/admin/memories?origin=manual&author=alice-phone",
            Some(&boss),
        ),
    )
    .await;
    assert_eq!(ids(&page), set(&[by_phone]));
}

/// Puts `workarea` in progress as the signed-in person's reviewer key,
/// through the app: the act is the Workareas view's, which `oml-web` runs
/// now (ADR-0070 d7 (b)); these tests read what it changes.
async fn put_in_progress(rig: &Rig, cookie: &str, workarea: &str) {
    let token = cookie.split_once('=').expect("a cookie").1;
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
        .set_workarea_mode(
            oml_app::types::Bearer::Session(&review),
            oml_app::domain::name::Reference::parse(workarea).expect("a reference"),
            oml_app::domain::in_progress::WorkareaMode::InProgress,
        )
        .await
        .expect("the reviewer key puts it in progress");
}

fn region<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    value["workareas"]["items"]
        .as_array()
        .expect("workareas")
        .iter()
        .find(|r| r["path"] == path)
}

#[tokio::test]
async fn the_atlas_draws_and_counts_only_the_workareas_and_rows_the_viewer_may_read() {
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
        "/web/workareas",
        &[("action", "create_workarea"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let helper = rig
        .app
        .issue_key(
            &rig.boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Writer,
                name: Name::parse("helper").expect("a name"),
                principal: "helper".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: None,
                crew: None,
                grants: vec![GrantRequest {
                    node: Name::parse("alice").expect("a name").into(),
                    role: Role::Writer,
                    deny: Vec::new(),
                    covers: oml_app::domain::access::Covers::Subtree,
                }],
                connect_code: false,
                user: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
            },
        )
        .await
        .expect("a tenant agent on alice's root");
    agent_writes(&rig, &helper.value, "helper held outside").await;
    put_in_progress(&rig, &alice, "alice.proj").await;
    agent_writes_in(&rig, &helper.value, "helper held inside", "alice.proj").await;

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
    assert_eq!(root["counts"]["total"], 2);
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
            && value["workareas"]["items"]
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
        &format!("/web/data/memories?workarea={proj_id}"),
        &alice,
    )
    .await;
    let paths: Vec<&str> = zoomed["workareas"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["universe.alice.proj"]);
    // The zoomed node's dropdown is the Workareas view's: the commit waits
    // on the held row and says so.
    assert_eq!(zoomed["workareas"]["items"][0]["commit_withheld"], 1);
    assert!(
        zoomed["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["workarea"] == "universe.alice.proj"),
        "{zoomed}"
    );
    // A workarea the person may not read is not found, never drawn empty.
    let bob_root = key_named(&rig, "bob-web").await.user.unwrap();
    let (status, _, _) = json_of(
        &rig,
        &format!("/web/data/memories?workarea={bob_root}"),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // The page draws what the route counts: no region of bob's.
    let (_, _, atlas) = send(&rig.router, get("/web/atlas", Some(&alice))).await;
    assert!(!atlas.contains("bob"), "{atlas}");
    assert!(atlas.contains("aria-label=\"proj, 1 readable"), "{atlas}");
    let (status, _, _) = send(
        &rig.router,
        get(&format!("/web/atlas?workarea={bob_root}"), Some(&alice)),
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
        assert!(page.contains("<h1>Memories</h1>"), "{new}: {page}");
    }
    // The list's rows link to one memory's own page; the old link to it is
    // not found.
    let (_, _, list) = send(&rig.router, get("/web/memories", Some(&cookie))).await;
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
            moved.contains(r#"<a href="/web/admin/memories" class="on" aria-current="page">"#),
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
async fn a_resubmitted_create_form_creates_its_workarea_once() {
    // ADR-0070 d3 layer 4: the create form carries a nonce, and its
    // Idempotency-Key is the nonce's digest with the session token, so the
    // same form sent again replays at the API instead of answering
    // name.taken.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, view) = send(&rig.router, get("/web/workareas", Some(&cookie))).await;
    let nonce = field_of(&view, "nonce");
    let fields = [
        ("csrf_token", csrf_of(&view)),
        ("action", "create_workarea".to_owned()),
        ("name", "once".to_owned()),
        ("nonce", nonce),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..2 {
        let (status, headers, page) =
            send(&rig.router, post("/web/workareas", &fields, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
        assert_eq!(headers[header::LOCATION], "/web/workareas?done=created");
    }
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let created = rig
        .store
        .workarea_children(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .into_iter()
        .filter(|w| w.name.as_str() == "once")
        .count();
    assert_eq!(created, 1);
    // A name already taken by another form is refused by name.
    let (_, _, view) = send(&rig.router, get("/web/workareas", Some(&cookie))).await;
    let (status, _, page) = send(
        &rig.router,
        post(
            "/web/workareas",
            &[
                ("csrf_token", &csrf_of(&view)),
                ("action", "create_workarea"),
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
