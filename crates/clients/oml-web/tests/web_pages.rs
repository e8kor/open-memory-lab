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
use oml_app::domain::Role;
use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::{Email, Provider};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSignIn, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::{BootstrapRequest, BootstrapResult, ProviderAnswer, WebEmailRequest};
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
        "/web",
        &[("action", "create_workarea"), ("name", "proj")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web",
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
            "/web",
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
        "/web",
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
        "/web",
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
            "/web",
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
        "/web",
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
async fn the_old_workareas_urls_and_acts_go_through_the_lead_to_the_moved_pages_own_paths() {
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
        assert_eq!(status, StatusCode::SEE_OTHER, "{old}");
        assert_eq!(headers[header::LOCATION], new, "{old}");
        let (status, _, page) = send(&rig.router, get(new, Some(who))).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.contains("<h1>Workareas</h1>"), "{page}");
    }
    // An old form's act runs nothing and lands on the view.
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
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/workareas");
    let node = rig
        .store
        .workarea(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert!(!node.in_progress);
    // Creating a workarea is still the lead's act, and lands on the view.
    let (status, headers, _) = act(
        &rig,
        &cookie,
        "/web",
        &[("action", "create_workarea"), ("name", "docs")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/workareas?done=created");
}

#[tokio::test]
async fn the_workareas_pages_draw_the_header_and_tabs_the_leads_pages_draw() {
    let rig = rig().await;
    rig.allow("bob@example.com", "bob", true).await;
    for (subject, email) in [("sub-1", EMAIL), ("sub-2", "bob@example.com")] {
        let cookie = signed_in(&rig, subject, email).await;
        let (_, _, lead) = send(&rig.router, get("/web?view=setup", Some(&cookie))).await;
        let (status, _, moved) = send(&rig.router, get("/web/workareas", Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{moved}");
        assert_eq!(header_of(&moved), header_of(&lead), "{email}");
        assert!(
            moved.contains(r#"<a href="/web/workareas" class="on" aria-current="page">"#),
            "{moved}"
        );
    }
    let boss = signed_in(&rig, "sub-2", "bob@example.com").await;
    let (_, _, lead) = send(&rig.router, get("/web/admin", Some(&boss))).await;
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
        let (_, _, lead) = send(&rig.router, get("/web?view=setup", Some(&cookie))).await;
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
async fn the_old_guardrails_url_goes_through_the_lead_to_the_moved_pages_own_path() {
    // ADR-0070 d7 (b) as amended: each moved page has its own path under
    // /web/; the lead answers the old ?view= form with a 303 to it.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for old in ["/web?view=guardrails", "/web?view=guardrails&workarea=x"] {
        let (status, headers, _) = send(&rig.router, get(old, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{old}");
        assert_eq!(headers[header::LOCATION], "/web/guardrails", "{old}");
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
        &[("action", "cut_off"), ("key", "alice-laptop")],
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
        &[("action", "cut_off"), ("key", "root-admin")],
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
    let (_, _, page) = send(&rig.router, get("/web?view=memories", Some(&cookie))).await;
    assert!(page.contains("the deploy window is tuesday"), "{page}");
    assert!(page.contains(">held<"), "{page}");
    let (status, _, opened) = send(
        &rig.router,
        get(
            &format!("/web?view=memories&memory={memory}"),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert!(opened.contains("the deploy window is tuesday"), "{opened}");
    rig.allow("bob@example.com", "bob", false).await;
    let bob = signed_in(&rig, "sub-bob", "bob@example.com").await;
    let (status, _, hidden) = send(
        &rig.router,
        get(&format!("/web?view=memories&memory={memory}"), Some(&bob)),
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
        "/web/admin",
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
        "/web/admin",
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
        "/web/admin",
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
            "/web/admin",
            &[("action", action), ("user", "boss"), ("workarea", workarea)],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}: {body}");
    }
    let (status, _, body) = act(
        &rig,
        &admin,
        "/web/admin",
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
    let (_, _, mine) = send(&rig.router, get("/web?view=memories", Some(&cookie))).await;
    assert!(
        mine.contains("Keys rotate quarterly.") && mine.contains("the shared area"),
        "{mine}"
    );
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
        "/web/admin",
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
    // The drawing is the lead's render frame, which only the lead mints
    // (ADR-0056 d9): the moved card says where it shows once promoted and
    // links nothing (ADR-0070, Phase (b) as built).
    assert!(
        page.contains("The drawing shows on the memory's own page once it is promoted"),
        "{page}"
    );
    assert!(!page.contains(&format!("memory={memory}")), "{page}");
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
        "Max-Age=300",
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
async fn a_reviewer_who_is_not_the_author_gets_the_diagrams_source_and_no_link_to_a_page_that_would_not_open()
 {
    let rig = rig().await;
    let (_, memory) = person_with_a_diagram(&rig).await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, _) = act(
        &rig,
        &admin,
        "/web/admin",
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
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("flowchart LR"), "the complete source: {page}");
    assert!(
        page.contains("The drawing shows on the memory's own page once it is promoted"),
        "{page}"
    );
    assert!(
        !page.contains(&format!("memory={memory}")),
        "no link to the memory's page: {page}"
    );
    // Why there is no link: the held memory's own page is not the
    // reviewer's to open (#189 moves it with the drawing).
    let (status, _, _) = send(
        &rig.router,
        get(&format!("/web?view=memories&memory={memory}"), Some(&admin)),
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
async fn the_old_review_and_keys_urls_and_acts_go_through_the_lead_to_the_moved_pages_own_paths() {
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (old, new) in [
        ("/web", "/web/keys"),
        ("/web?view=keys", "/web/keys"),
        ("/web?view=review", "/web/review"),
        ("/web/admin?view=review", "/web/admin/review"),
        ("/web/admin?view=held&select=all", "/web/admin/review"),
        ("/web/admin?view=queue", "/web/admin/review"),
    ] {
        let (status, headers, _) = send(&rig.router, get(old, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{old}");
        assert_eq!(headers[header::LOCATION], new, "{old}");
        let (status, _, page) = send(&rig.router, get(new, Some(&cookie))).await;
        assert_eq!(status, StatusCode::OK, "{new}: {page}");
    }
    for (page, action, new) in [
        ("/web", "issue_key", "/web/keys"),
        ("/web", "review_next", "/web/review"),
        ("/web/admin", "bulk_verdict", "/web/admin/review"),
        ("/web/admin", "cut_off", "/web/admin/keys"),
    ] {
        let (status, headers, _) = act(
            &rig,
            &cookie,
            page,
            &[("action", action), ("name", "laptop"), ("role", "writer")],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}");
        assert_eq!(headers[header::LOCATION], new, "{action}");
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
    let (_, _, lead) = send(&rig.router, get("/web?view=setup", Some(&cookie))).await;
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
    let (_, _, lead) = send(&rig.router, get("/web/admin", Some(&cookie))).await;
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
