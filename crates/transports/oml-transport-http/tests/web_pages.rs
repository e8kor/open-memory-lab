//! The web pages and the allowlist's routes through the router, over the
//! in-memory doubles and a sign-in provider double: no Postgres, and no
//! call ever reaches Google or GitHub (ADR-0045 d1–d8, d14, d15, d18).
//! Each test is named after the sentence it proves.

mod support;

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::{Email, GitHubEmail, Provider};
use oml_app::domain::{HeldBy, Role};
use oml_app::doubles::{MemSignIn, MemStore};
use oml_app::ports::JournalStore as _;
use oml_app::types::{
    BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, ProviderAnswer,
    ProviderFailure, WebEmailRequest,
};
use oml_transport_http::ROUTES;
use oml_transport_http::session::WebClient;
use oml_transport_http::traffic::{ScopeObserver, Traffic};
use oml_transport_http::web::{
    ADMIN_ACTIONS, SESSION_COOKIE, SIGN_IN_COOKIE, USER_ACTIONS, WEB_ROUTES, WebPages,
};

use support::{Captured, TestApp, app, state};

const ISSUER: &str = "https://oml.example.com";
const CLIENT_GOOGLE: &str = "google-client.apps.googleusercontent.com";
const CLIENT_GITHUB: &str = "github-client";
const FORM: &str = "application/x-www-form-urlencoded";
const EMAIL: &str = "alice@example.com";

struct Rig {
    store: MemStore,
    app: Arc<TestApp>,
    router: Router,
    boot: BootstrapResult,
    providers: MemSignIn,
    traffic: Traffic,
}

fn test_app(store: &MemStore) -> TestApp {
    app(store)
        .with_observer(Arc::new(ScopeObserver))
        .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"))
        .with_formats(oml_format::compiled())
}

/// A tenant whose admin key put `alice@example.com` on the list as `alice`,
/// and the router with both providers on.
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
    let traffic = Traffic::default();
    let router = state(&store, test_app(&store))
        .with_traffic(traffic.clone())
        .with_web_client(WebClient::new(Some(WEB_CLIENT_SECRET.to_owned()), None))
        .with_web(WebPages::new(
            vec![
                (Provider::Google, CLIENT_GOOGLE.to_owned()),
                (Provider::GitHub, CLIENT_GITHUB.to_owned()),
            ],
            providers.clone(),
        ))
        .into_router();
    let rig = Rig {
        app: Arc::new(test_app(&store)),
        store,
        router,
        boot,
        providers,
        traffic,
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

    async fn head(&self) -> u64 {
        self.store
            .chain_head(self.boot.tenant_id)
            .await
            .expect("a head")
            .expect("a chain")
            .seq
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

/// The CSRF token of the session `cookie` names, as its pages carry it.
fn csrf_for(cookie: &str) -> String {
    let token = cookie.split_once('=').expect("a session cookie").1;
    oml_app::domain::web::csrf_token(token)
}

#[tokio::test]
async fn the_lead_serves_exactly_web_routes() {
    let rig = rig().await;
    for &(method, path) in WEB_ROUTES {
        let path = path.replace("{provider}", "google");
        let request = Request::builder()
            .method(method)
            .uri(&path)
            .body(Body::empty())
            .expect("a request");
        let (status, _, body) = send(&rig.router, request).await;
        assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
        // The admin page answers a signed-out browser access.not_found on
        // a page of its own, never the router's empty 404.
        assert!(
            status != StatusCode::NOT_FOUND || body.contains("access.not_found"),
            "{method} {path}"
        );
    }
    assert_eq!(
        WEB_ROUTES.len(),
        14,
        "a new route is a new decision amending ADR-0045; ADR-0068 d10 added seven, ADR-0070 d4 GET /web/sign-in, and ADR-0070 d7 (b) moved the two JSON routes to oml-web"
    );
    assert_eq!(
        USER_ACTIONS,
        [
            "issue_key",
            "connect_key",
            "connect_code",
            "cut_off",
            "create_workarea",
            "workarea_mode",
            "workarea_transition",
            "workarea_ratify",
            "review_next",
            "verdict",
            "sign_out"
        ]
    );
    assert_eq!(
        ADMIN_ACTIONS,
        [
            "allow_email",
            "remove_email",
            "set_admin",
            "grant_review",
            "grant_curation",
            "review_next",
            "verdict",
            "bulk_confirm",
            "bulk_verdict",
            "curate",
            "supersede",
            "cut_off",
            "workarea_mode",
            "workarea_transition",
            "workarea_ratify"
        ]
    );
    // An unknown provider in the path is an unknown page.
    let (status, _, _) = send(&rig.router, get("/web/callback/facebook", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Without the pages, or without oauth.issuer, every web route is an
    // unknown path.
    let store = MemStore::new();
    let off = state(&store, test_app(&store)).into_router();
    let no_issuer = state(&store, app(&store))
        .with_web(WebPages::new(
            vec![(Provider::GitHub, CLIENT_GITHUB.to_owned())],
            MemSignIn::new(),
        ))
        .into_router();
    for router in [off, no_issuer] {
        for &(method, path) in WEB_ROUTES {
            let request = Request::builder()
                .method(method)
                .uri(path.replace("{provider}", "github"))
                .body(Body::empty())
                .expect("a request");
            let (status, _, _) = send(&router, request).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
        }
    }
}

#[tokio::test]
async fn every_web_answer_carries_the_csp_no_store_and_no_script() {
    let rig = rig().await;
    let (status, headers, body) = send(&rig.router, get("/web", None)).await;
    assert_eq!(status, StatusCode::OK);
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(
        csp.starts_with("default-src 'none'; style-src 'sha256-"),
        "{csp}"
    );
    assert!(
        csp.contains("form-action 'self' https://accounts.google.com https://github.com;"),
        "{csp}"
    );
    assert!(
        csp.contains("frame-ancestors 'none'; base-uri 'none'"),
        "{csp}"
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::REFERRER_POLICY], "same-origin");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(!body.contains("<script"), "no script on a page");
    assert!(body.contains("Sign in with Google") && body.contains("Sign in with GitHub"));
}

#[tokio::test]
async fn the_sign_in_start_redirects_with_s256_pkce_state_and_a_google_nonce_and_writes_no_row() {
    let rig = rig().await;
    let before = rig.head().await;
    let (location, _) = start(&rig, Provider::Google).await;
    assert!(location.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    assert_eq!(
        query_param(&location, "client_id").as_deref(),
        Some(CLIENT_GOOGLE)
    );
    assert_eq!(
        query_param(&location, "redirect_uri").as_deref(),
        Some("https://oml.example.com/web/callback/google")
    );
    assert_eq!(
        query_param(&location, "code_challenge_method").as_deref(),
        Some("S256")
    );
    assert_eq!(
        query_param(&location, "scope").as_deref(),
        Some("openid email")
    );
    assert_eq!(
        query_param(&location, "code_challenge").map(|c| c.len()),
        Some(43)
    );
    assert_eq!(query_param(&location, "state").map(|s| s.len()), Some(64));
    assert!(query_param(&location, "nonce").is_some());
    let (github, _) = start(&rig, Provider::GitHub).await;
    assert!(github.starts_with("https://github.com/login/oauth/authorize?"));
    assert_eq!(query_param(&github, "nonce"), None);
    assert_eq!(
        query_param(&github, "scope").as_deref(),
        Some("read:user user:email")
    );
    assert_eq!(rig.head().await, before, "a sign-in start journals nothing");
    assert_eq!(rig.providers.calls(), 0, "and calls no provider");
}

#[tokio::test]
async fn a_callback_without_the_sealed_state_makes_no_outbound_call() {
    let rig = rig().await;
    let (location, sealed) = start(&rig, Provider::Google).await;
    let state = query_param(&location, "state").expect("a state");
    let cookie = format!("{SIGN_IN_COOKIE}={sealed}");
    for (provider, state, cookies) in [
        // No cookie at all.
        (Provider::Google, state.as_str(), ""),
        // Another state.
        (Provider::Google, "0000", cookie.as_str()),
        // The Google sign-in's cookie on the GitHub callback.
        (Provider::GitHub, state.as_str(), cookie.as_str()),
        // A cookie that does not open.
        (
            Provider::Google,
            state.as_str(),
            "__Host-oml_signin=deadbeef",
        ),
    ] {
        let (status, headers, body) = callback(&rig, provider, "a-code", state, cookies).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        assert!(body.contains("auth.invalid"), "{body}");
        let (_, cleared) = set_cookie(&headers, SIGN_IN_COOKIE).expect("cleared");
        assert!(cleared.contains("Max-Age=0"), "{cleared}");
        assert!(set_cookie(&headers, SESSION_COOKIE).is_none());
    }
    assert_eq!(rig.providers.calls(), 0);
}

#[tokio::test]
async fn a_listed_verified_google_email_signs_in_and_sees_who_it_is() {
    let rig = rig().await;
    let before = rig.head().await;
    let (status, token, _) = sign_in_google(&rig, "sub-1", "Alice@Example.com").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let token = token.expect("a session cookie");
    assert_eq!(token.len(), 64, "a 256-bit token, hex");
    let (provider, code, verifier, redirect) = rig.providers.last().expect("one exchange");
    assert_eq!(provider, Provider::Google);
    assert_eq!(code, "the-code-4711");
    assert_eq!(verifier.len(), 64);
    assert_eq!(redirect, "https://oml.example.com/web/callback/google");
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web?view=setup",
            Some(&format!("{SESSION_COOKIE}={token}")),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("alice · oml"),
        "who and the organisation: {page}"
    );
    assert!(!page.contains(EMAIL), "the page never echoes the email");
    // The landing is the Keys view, which oml-web serves (ADR-0070 d7 (b)):
    // `oml-web/tests/web_pages.rs` holds that it lists the session keys.
    let (status, headers, _) = send(
        &rig.router,
        get("/web", Some(&format!("{SESSION_COOKIE}={token}"))),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/keys");
    // The first sign-in journals the session keys and the root, nothing
    // more; a second sign-in journals nothing.
    let after_first = rig.head().await;
    assert!(after_first > before);
    let (status, _, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        rig.head().await,
        after_first,
        "a later sign-in journals nothing"
    );
}

#[tokio::test]
async fn github_signs_in_only_the_primary_verified_email() {
    let rig = rig().await;
    for (emails, signs_in) in [
        (
            vec![
                GitHubEmail {
                    email: EMAIL.to_owned(),
                    primary: false,
                    verified: true,
                },
                GitHubEmail {
                    email: "other@example.com".to_owned(),
                    primary: true,
                    verified: true,
                },
            ],
            false,
        ),
        (
            vec![GitHubEmail {
                email: EMAIL.to_owned(),
                primary: true,
                verified: true,
            }],
            true,
        ),
    ] {
        let (location, sealed) = start(&rig, Provider::GitHub).await;
        let state = query_param(&location, "state").expect("a state");
        rig.providers.answer(
            Provider::GitHub,
            Ok(ProviderAnswer::GitHub {
                account_id: 77,
                emails,
            }),
        );
        let (status, headers, body) = callback(
            &rig,
            Provider::GitHub,
            "gh-code",
            &state,
            &format!("{SIGN_IN_COOKIE}={sealed}"),
        )
        .await;
        if signs_in {
            assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
            assert!(set_cookie(&headers, SESSION_COOKIE).is_some());
        } else {
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert!(
                body.contains("not on the list"),
                "a secondary address does not sign in: {body}"
            );
        }
    }
}

#[tokio::test]
async fn a_google_token_with_email_verified_false_is_refused() {
    let rig = rig().await;
    let (location, sealed) = start(&rig, Provider::Google).await;
    let state = query_param(&location, "state").expect("a state");
    let nonce = query_param(&location, "nonce").expect("a nonce");
    rig.providers.answer(
        Provider::Google,
        Ok(ProviderAnswer::GoogleIdToken(google_token(
            &nonce, "sub-1", EMAIL, false,
        ))),
    );
    let (status, headers, body) = callback(
        &rig,
        Provider::Google,
        "c",
        &state,
        &format!("{SIGN_IN_COOKIE}={sealed}"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        body.contains("does not call this email address verified"),
        "{body}"
    );
    assert!(set_cookie(&headers, SESSION_COOKIE).is_none());
}

#[tokio::test]
async fn no_session_exists_before_the_callback_admits() {
    let rig = rig().await;
    // An address on no list: the provider answered, nothing is opened.
    let (status, token, body) = sign_in_google(&rig, "sub-9", "mallory@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("not on the list"), "{body}");
    assert_eq!(token, None);
    // A planted session cookie is no session: the sign-in page answers.
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web",
            Some(&format!("{SESSION_COOKIE}={}", "a".repeat(64))),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Sign in with Google"));
    // A browser arriving with an old session leaves with only the new one.
    let (_, first, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let first = first.expect("a session");
    let (location, sealed) = start(&rig, Provider::Google).await;
    let state = query_param(&location, "state").expect("a state");
    let nonce = query_param(&location, "nonce").expect("a nonce");
    rig.providers.answer(
        Provider::Google,
        Ok(ProviderAnswer::GoogleIdToken(google_token(
            &nonce, "sub-1", EMAIL, true,
        ))),
    );
    let (_, headers, _) = callback(
        &rig,
        Provider::Google,
        "c2",
        &state,
        &format!("{SIGN_IN_COOKIE}={sealed}; {SESSION_COOKIE}={first}"),
    )
    .await;
    let (second, _) = set_cookie(&headers, SESSION_COOKIE).expect("a new session");
    assert_ne!(second, first, "a fresh token, never the planted one");
    assert!(
        rig.app.web_session(&first).await.unwrap().is_none(),
        "the old row ended"
    );
    assert!(rig.app.web_session(&second).await.unwrap().is_some());
}

#[tokio::test]
async fn a_moved_account_is_refused_as_identity_changed() {
    let rig = rig().await;
    let (status, _, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, token, body) = sign_in_google(&rig, "sub-2", EMAIL).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("another account"), "{body}");
    assert_eq!(token, None);
    // The pin is per provider: a GitHub sign-in pins its own subject.
    let (location, sealed) = start(&rig, Provider::GitHub).await;
    let state = query_param(&location, "state").expect("a state");
    rig.providers.answer(
        Provider::GitHub,
        Ok(ProviderAnswer::GitHub {
            account_id: 5,
            emails: vec![GitHubEmail {
                email: EMAIL.to_owned(),
                primary: true,
                verified: true,
            }],
        }),
    );
    let (status, _, _) = callback(
        &rig,
        Provider::GitHub,
        "c",
        &state,
        &format!("{SIGN_IN_COOKIE}={sealed}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn removing_an_email_ends_its_sessions() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let token = token.expect("a session");
    let (removed, _, _) = rig
        .app
        .remove_web_email(&rig.boot.admin_key.value, &Email::parse(EMAIL).unwrap())
        .await
        .expect("removed");
    assert_eq!(removed.user.as_str(), "alice");
    let (_, _, page) = send(
        &rig.router,
        get("/web", Some(&format!("{SESSION_COOKIE}={token}"))),
    )
    .await;
    assert!(page.contains("Sign in with Google"), "signed out: {page}");
    // Changing the entry's admin flag ends its sessions too.
    rig.allow(EMAIL, "alice", false).await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let token = token.expect("a session");
    rig.allow(EMAIL, "alice", true).await;
    assert!(rig.app.web_session(&token).await.unwrap().is_none());
}

#[tokio::test]
async fn a_post_without_the_csrf_token_journals_nothing() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    let before = rig.head().await;
    for fields in [
        vec![("action", "sign_out")],
        vec![
            ("action", "sign_out"),
            ("csrf_token", "0".repeat(64).as_str()),
        ],
    ] {
        let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, *v)).collect();
        let (status, _, body) = send(&rig.router, post("/web", &fields, Some(&cookie))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body.contains("request.validation") && body.contains("csrf_token"),
            "{body}"
        );
    }
    assert_eq!(rig.head().await, before);
    let (_, _, page) = send(&rig.router, get("/web?view=setup", Some(&cookie))).await;
    assert!(page.contains("alice · oml"), "the session stands");
}

#[tokio::test]
async fn a_post_from_another_origin_is_refused_before_the_session_is_read() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    for (name, value) in [("sec-fetch-site", "cross-site"), ("origin", "null")] {
        for path in ["/web", "/web/admin", "/web/sign-in"] {
            let request = Request::post(path)
                .header(header::CONTENT_TYPE, FORM)
                .header(name, value)
                .header(header::COOKIE, &cookie)
                .body(Body::from("action=sign_out"))
                .expect("a request");
            let (status, _, body) = send(&rig.router, request).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
            assert!(body.contains("oauth.origin_refused"), "{body}");
        }
    }
}

#[tokio::test]
async fn sign_out_ends_the_session_and_clears_the_cookie() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    let csrf = csrf_for(&cookie);
    let (status, headers, _) = send(
        &rig.router,
        post(
            "/web",
            &[("action", "sign_out"), ("csrf_token", &csrf)],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web");
    let (_, cleared) = set_cookie(&headers, SESSION_COOKIE).expect("cleared");
    assert!(cleared.contains("Max-Age=0"));
    let (_, _, page) = send(&rig.router, get("/web", Some(&cookie))).await;
    assert!(page.contains("Sign in with Google"));
    // A POST after the session ended is session_expired on the sign-in page.
    let (status, _, body) = send(
        &rig.router,
        post(
            "/web",
            &[("action", "sign_out"), ("csrf_token", &csrf)],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("session has ended"), "{body}");
}

#[tokio::test]
async fn a_non_admin_session_gets_not_found_on_the_admin_page() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    let (status, _, body) = send(&rig.router, get("/web/admin", Some(&cookie))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("access.not_found"));
    let (status, _, _) = send(&rig.router, get("/web/admin", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    rig.allow("boss@example.com", "boss", true).await;
    let (_, admin, _) = sign_in_google(&rig, "sub-boss", "boss@example.com").await;
    let (status, _, page) = send(
        &rig.router,
        get(
            "/web/admin",
            Some(&format!("{SESSION_COOKIE}={}", admin.expect("a session"))),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Administration"));
}

#[tokio::test]
async fn an_authorization_header_on_a_web_route_is_verb_ceiling() {
    let rig = rig().await;
    for &(method, path) in WEB_ROUTES {
        let request = Request::builder()
            .method(method)
            .uri(path.replace("{provider}", "google"))
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, FORM)
            .body(Body::from("provider=google"))
            .expect("a request");
        let (status, _, body) = send(&rig.router, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert!(
            body.contains("access.verb_ceiling") && body.contains("Authorization"),
            "{method} {path}: {body}"
        );
    }
    assert_eq!(rig.providers.calls(), 0);
}

#[tokio::test]
async fn a_session_cookie_on_the_api_is_auth_missing() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    let api = ROUTES
        .iter()
        .filter(|(_, path)| !["/healthz", "/readyz", "/v2/bootstrap/claim"].contains(path))
        .chain(&[("POST", "/mcp")]);
    for &(method, path) in api {
        let path = path
            .replace("{artifact_id}", "0b5f6c3e-5d0b-4d8f-9a5e-2f1d7c9e8a10")
            .replace("{key}", "root-admin")
            .replace("{connection_id}", "0b5f6c3e-5d0b-4d8f-9a5e-2f1d7c9e8a11")
            .replace("{crew}", "crew-a")
            .replace("{workarea}", "universe");
        let request = Request::builder()
            .method(method)
            .uri(&path)
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "k-1")
            .body(Body::from("{}"))
            .expect("a request");
        let (status, _, body) = send(&rig.router, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}: {body}");
        if path != "/mcp" {
            assert_eq!(
                serde_json::from_str::<Value>(&body).expect("JSON")["error"]["code"],
                "auth.missing",
                "{method} {path}"
            );
        }
    }
}

#[tokio::test]
async fn the_provider_cap_answers_limit_rate_and_the_metrics_listener_counts_failed_exchanges() {
    let rig = rig().await;
    for failure in [
        ProviderFailure::Busy,
        ProviderFailure::Refused("error_answer"),
    ] {
        let (location, sealed) = start(&rig, Provider::Google).await;
        let state = query_param(&location, "state").expect("a state");
        rig.providers.answer(Provider::Google, Err(failure));
        let (status, _, body) = callback(
            &rig,
            Provider::Google,
            "c",
            &state,
            &format!("{SIGN_IN_COOKIE}={sealed}"),
        )
        .await;
        match failure {
            ProviderFailure::Busy => {
                assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
                assert!(body.contains("limit.rate"));
            }
            ProviderFailure::Refused(_) => {
                assert_eq!(status, StatusCode::UNAUTHORIZED);
                assert!(body.contains("auth.invalid"));
            }
        }
    }
    let text = rig.traffic.metrics().encode();
    for expected in [
        r#"oml_web_exchanges_failed_total{provider="google",reason="busy"} 1"#,
        r#"oml_web_exchanges_failed_total{provider="google",reason="error_answer"} 1"#,
    ] {
        assert!(text.contains(expected), "no {expected} in:\n{text}");
    }
    assert!(!text.contains(r#"provider="github""#), "{text}");
    // /readyz names the providers and counts nothing (ADR-0054 d1).
    let (_, _, body) = send(&rig.router, get("/readyz", None)).await;
    let ready: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(ready["web"]["providers"], json!(["google", "github"]));
}

#[tokio::test]
async fn no_log_line_holds_a_code_state_cookie_or_email() {
    let rig = rig().await;
    let (guard, captured) = Captured::start();
    let (location, sealed) = start(&rig, Provider::Google).await;
    let state = query_param(&location, "state").expect("a state");
    let nonce = query_param(&location, "nonce").expect("a nonce");
    let challenge = query_param(&location, "code_challenge").expect("a challenge");
    rig.providers.answer(
        Provider::Google,
        Ok(ProviderAnswer::GoogleIdToken(google_token(
            &nonce,
            "sub-secret-42",
            EMAIL,
            true,
        ))),
    );
    let (status, headers, _) = callback(
        &rig,
        Provider::Google,
        "codezebra",
        &state,
        &format!("{SIGN_IN_COOKIE}={sealed}"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (token, _) = set_cookie(&headers, SESSION_COOKIE).expect("a session");
    let cookie = format!("{SESSION_COOKIE}={token}");
    let csrf = csrf_for(&cookie);
    let _ = sign_in_google(&rig, "sub-other", "mallory@example.com").await;
    let (status, _, _) = send(
        &rig.router,
        post(
            "/web",
            &[("action", "sign_out"), ("csrf_token", &csrf)],
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    drop(guard);

    let text = captured.text();
    for (what, secret) in [
        ("the code", "codezebra"),
        ("the state", state.as_str()),
        ("the nonce", nonce.as_str()),
        ("the PKCE challenge", challenge.as_str()),
        ("the sealed sign-in cookie", sealed.as_str()),
        ("the session token", token.as_str()),
        ("the CSRF token", csrf.as_str()),
        ("an email", "example.com"),
        ("the provider subject", "sub-secret-42"),
    ] {
        assert!(!text.contains(secret), "{what} is in the log:\n{text}");
    }
    let signed: Vec<Value> = captured.events("web_sign_in");
    let outcomes: Vec<&str> = signed
        .iter()
        .filter_map(|l| l["outcome"].as_str())
        .collect();
    assert!(
        outcomes.contains(&"signed_in") && outcomes.contains(&"not_allowed"),
        "{outcomes:?}"
    );
    let admitted = signed
        .iter()
        .find(|l| l["outcome"] == "signed_in")
        .expect("one");
    assert_eq!(admitted["user"], "alice");
    assert_eq!(admitted["provider"], "google");
    assert!(admitted["request_id"].is_string());
    let lines = captured.events("http_request");
    let routes: Vec<&str> = lines.iter().filter_map(|l| l["route"].as_str()).collect();
    assert!(
        routes.contains(&"/web/callback/{provider}"),
        "the template: {routes:?}"
    );
    let sign_out = lines
        .iter()
        .find(|l| l["web_action"] == "sign_out")
        .expect("the sign-out line");
    assert_eq!(sign_out["credential"], "session");
    assert_eq!(sign_out["tenant_id"], rig.boot.tenant_id.to_string());
    let start_line = lines
        .iter()
        .find(|l| l["route"] == "/web/sign-in")
        .expect("the start line");
    assert_eq!(start_line["web_provider"], "google");
    assert!(
        start_line["credential"].is_null(),
        "no session before the callback"
    );
}

#[tokio::test]
async fn the_allowlist_routes_take_an_admin_key_and_the_email_only_in_a_body() {
    let rig = rig().await;
    let admin = format!("Bearer {}", rig.boot.admin_key.value);
    let writer = rig
        .app
        .issue_key(
            &rig.boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Writer,
                name: Name::parse("agent-7").expect("a name"),
                principal: "crew/agent-7".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: None,
                crew: None,
                grants: vec![GrantRequest {
                    node: rig.boot.root_workarea_id.into(),
                    role: Role::Writer,
                    deny: Vec::new(),
                    covers: Default::default(),
                }],
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("a writer key");
    let json_post = |path: &str, auth: &str, body: Value| {
        Request::post(path)
            .header(header::AUTHORIZATION, auth)
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "k")
            .body(Body::from(body.to_string()))
            .expect("a request")
    };
    let (status, _, body) = send(
        &rig.router,
        json_post(
            "/v2/web/emails",
            &admin,
            json!({"email": "Bob@Example.com", "user": "bob", "admin": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"email": "bob@example.com", "user": "bob", "admin": true})
    );
    let (status, _, body) = send(
        &rig.router,
        json_post(
            "/v2/web/emails",
            &format!("Bearer {}", writer.value),
            json!({"email": "eve@example.com", "user": "eve", "admin": false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("access.verb_ceiling"));
    let (status, _, body) = send(
        &rig.router,
        Request::get("/v2/web/emails?limit=1")
            .header(header::AUTHORIZATION, &admin)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let page: Value = serde_json::from_str(&body).unwrap();
    let cursor = page["next_cursor"]
        .as_str()
        .expect("a second page")
        .to_owned();
    assert!(
        !cursor.contains('@'),
        "the cursor is a digest, never an address"
    );
    let (_, _, body) = send(
        &rig.router,
        Request::get(format!("/v2/web/emails?limit=1&cursor={cursor}"))
            .header(header::AUTHORIZATION, &admin)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let second: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(second["items"].as_array().map(Vec::len), Some(1));
    assert_ne!(second["items"][0], page["items"][0]);
    let (status, _, body) = send(
        &rig.router,
        json_post(
            "/v2/web/emails/remove",
            &admin,
            json!({"email": "nobody@example.com"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _, body) = send(
        &rig.router,
        json_post(
            "/v2/web/emails/remove",
            &admin,
            json!({"email": "BOB@example.com"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(serde_json::from_str::<Value>(&body).unwrap()["journal_seq"].is_u64());
}

#[tokio::test]
async fn the_journal_holds_an_email_digest_and_never_the_address() {
    let rig = rig().await;
    rig.allow(EMAIL, "alice", true).await;
    rig.app
        .remove_web_email(&rig.boot.admin_key.value, &Email::parse(EMAIL).unwrap())
        .await
        .expect("removed");
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, 1, 100)
        .await
        .expect("the chain");
    let digest = Email::parse(EMAIL).unwrap().digest().to_prefixed();
    let web: Vec<_> = events
        .iter()
        .filter(|e| e.envelope.event_type.starts_with("web."))
        .collect();
    let types: Vec<&str> = web.iter().map(|e| e.envelope.event_type.as_str()).collect();
    assert_eq!(
        types,
        [
            "web.email_allowed",
            "web.email_changed",
            "web.email_removed"
        ]
    );
    for event in &web {
        assert_eq!(event.envelope.payload["email_digest"], digest.as_str());
        assert_eq!(event.envelope.payload["user"], "alice");
        assert_eq!(
            event.envelope.api_key_id,
            Some(rig.boot.admin_key.key_id),
            "the admin key acts"
        );
    }
    for event in &events {
        assert!(
            !event.envelope.payload.to_string().contains("example.com"),
            "{}",
            event.envelope.event_type
        );
    }
    // An entry asked again exactly as it stands journals nothing.
    rig.allow(EMAIL, "alice", false).await;
    let head = rig.head().await;
    rig.allow(EMAIL, "alice", false).await;
    assert_eq!(rig.head().await, head);
}

// ---------------------------------------------------------------------------
// W3 and W4: session keys, the user page, the admin page, the admin read
// ---------------------------------------------------------------------------

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
    let csrf = csrf_for(cookie);
    let mut all: Vec<(&str, &str)> = vec![("csrf_token", &csrf)];
    all.extend_from_slice(fields);
    send(&rig.router, post(path, &all, Some(cookie))).await
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

/// The web client secret the lead is configured with, which `oml-web`
/// presents beside a session's capacity credential (ADR-0070 d2).
const WEB_CLIENT_SECRET: &str = "lead-web-client-secret-0123456789";

/// A call to the API as `oml-web` makes it for the session `cookie` names:
/// the capacity's credential, the web client's identity, a fresh key.
async fn as_web(
    rig: &Rig,
    cookie: &str,
    capacity: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let token = cookie.split_once('=').expect("a session cookie").1;
    let credential = oml_app::domain::web::capacity_credential(token, capacity);
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

/// The person's own `<user>-<name>` writer key, issued through the API as
/// the Keys page, which `oml-web` serves, issues it: the card it shows,
/// `OML_API_KEY=<value>`.
async fn issued(rig: &Rig, cookie: &str, name: &str) -> String {
    let token = cookie.split_once('=').expect("a session cookie").1;
    let credential = oml_app::domain::web::capacity_credential(token, "web");
    let whoami = Request::get("/v2/whoami")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .header("oml-web-client", WEB_CLIENT_SECRET)
        .body(Body::empty())
        .expect("a request");
    let (_, _, me) = send(&rig.router, whoami).await;
    let me: Value = serde_json::from_str(&me).expect("JSON");
    let user = me["user"]["name"].as_str().expect("a person").to_owned();
    let (status, key) = as_web(
        rig,
        cookie,
        "web",
        "/v2/keys",
        json!({"name": format!("{user}-{name}"), "role": "writer", "connect_code": false}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    format!("OML_API_KEY={}\n", key["value"].as_str().expect("a value"))
}

fn value_of(page: &str) -> String {
    let at = page.find("OML_API_KEY=").expect("a key value") + "OML_API_KEY=".len();
    page[at..at + page[at..].find(['<', '\n']).expect("an end")].to_owned()
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

#[tokio::test]
async fn the_first_sign_in_creates_the_users_root_and_web_key() {
    let rig = rig().await;
    let _ = signed_in(&rig, "sub-1", EMAIL).await;
    let web = key_named(&rig, "alice-web").await;
    assert_eq!(web.credential.as_str(), "session");
    assert_eq!(web.session_user.as_ref().map(Name::as_str), Some("alice"));
    let root = web.user.expect("a user key");
    let workarea = rig
        .store
        .workarea(rig.boot.tenant_id, root)
        .await
        .unwrap()
        .expect("the root");
    assert!(workarea.user_root);
    assert_eq!(workarea.name.as_str(), "alice");
    assert_eq!(web.held_by, HeldBy::Human);
    assert_eq!(
        web.principal, "alice-web",
        "a session key's principal is its name"
    );
}

#[tokio::test]
async fn the_first_sign_in_issues_a_review_key_on_the_users_root() {
    let rig = rig().await;
    let _ = signed_in(&rig, "sub-1", EMAIL).await;
    let review = key_named(&rig, "alice-review").await;
    assert_eq!(review.role, Role::Dreamer);
    assert_eq!(review.user, None, "a tenant key");
    assert_eq!(review.credential.as_str(), "session");
    let access = rig
        .store
        .key_access(rig.boot.tenant_id, review.key_id)
        .await
        .unwrap()
        .expect("its access");
    let root = key_named(&rig, "alice-web").await.user.unwrap();
    let roles: Vec<Role> = access
        .grants
        .iter()
        .filter(|g| g.grant.node == root)
        .map(|g| g.grant.role)
        .collect();
    assert_eq!(roles, [Role::Dreamer, Role::Reader]);
    assert!(
        rig.store
            .resolve_name(
                rig.boot.tenant_id,
                oml_app::domain::name::NamedKind::Key,
                &Name::parse("alice-admin").unwrap()
            )
            .await
            .unwrap()
            .is_none(),
        "no admin key for a non-admin entry"
    );
}

#[tokio::test]
async fn a_session_key_has_no_value_and_no_connect_code() {
    let rig = rig().await;
    let _ = signed_in(&rig, "sub-1", EMAIL).await;
    let web = key_named(&rig, "alice-web").await;
    assert_eq!(web.key_hash, None, "no value, so no hash");
    let refused = rig
        .app
        .mint_connect_code(
            &rig.boot.admin_key.value,
            oml_app::domain::name::Reference::Name(Name::parse("alice-web").unwrap()),
        )
        .await
        .expect_err("a session key holds no connection");
    assert!(format!("{refused:?}").contains("SessionKey"), "{refused:?}");
}

#[tokio::test]
async fn the_last_admin_address_is_never_removed_or_unflagged_from_the_page() {
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
        let (status, _, body) = act(&rig, &admin, "/web/admin", &fields).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("last admin"), "{body}");
    }
    let (status, _, _) = act(
        &rig,
        &admin,
        "/web/admin",
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
        "/web/admin",
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
async fn the_admin_read_serves_text_and_attribution_and_journals_the_read() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let page = issued(&rig, &cookie, "laptop").await;
    let value = value_of(&page);
    let memory = agent_writes(&rig, &value, "zebra crossing secret").await;
    let admin = format!("Bearer {}", rig.boot.admin_key.value);
    let (status, _, body) = send(
        &rig.router,
        Request::post("/v2/admin/memories/search")
            .header(header::AUTHORIZATION, &admin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"query": "zebra", "state": "held"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let found: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(found["items"][0]["text"], "zebra crossing secret");
    assert_eq!(found["items"][0]["key"], "alice-laptop");
    assert_eq!(found["items"][0]["user"], "alice");
    let (status, _, body) = send(
        &rig.router,
        Request::get(format!("/v2/admin/memories/{memory}"))
            .header(header::AUTHORIZATION, &admin)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let read: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(read["state"], "held");
    assert!(read["plaintext_digest"].is_string() && read["verdicts"].is_array());
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, 1, 500)
        .await
        .unwrap();
    let reads: Vec<_> = events
        .iter()
        .filter(|e| e.envelope.event_type == "memory.admin_read")
        .collect();
    assert_eq!(reads.len(), 2, "one event per answer");
    assert_eq!(
        reads[1].envelope.payload["artifact_ids"],
        json!([memory.to_string()])
    );
    assert!(reads[0].envelope.payload["query_digest"].is_string());
    for e in &reads {
        assert!(
            !e.envelope.payload.to_string().contains("zebra"),
            "never the text or query"
        );
    }
    // Every other role is refused; an admin key still recalls nothing.
    let (status, _, _) = send(
        &rig.router,
        Request::get(format!("/v2/admin/memories/{memory}"))
            .header(header::AUTHORIZATION, format!("Bearer {value}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let refused = rig
        .app
        .recall(
            rig.boot.admin_key.value.as_str(),
            &oml_app::types::RecallRequest {
                sources: None,
                timings: false,
                anchor: oml_app::types::AnchorFilter::default(),
                query: "zebra".to_owned(),
                k: 5,
                workarea: None,
                kinds: oml_app::types::HIT_KINDS.to_vec(),
                include_text: true,
                include_unreviewed: Some(false),
                max_tokens: None,
                search: None,
                match_mode: oml_app::search::MatchMode::Any,
                include_expired: false,
                now: None,
                as_of: None,
            },
        )
        .await
        .expect_err("an admin key still recalls nothing");
    assert!(
        format!("{refused:?}").contains("VerbCeiling"),
        "{refused:?}"
    );
}

// ---------------------------------------------------------------------------
// Guardrails on the pages (ADR-0037 d17, d18 Q5; build step S6)
// ---------------------------------------------------------------------------

async fn laptop_key(rig: &Rig, cookie: &str) -> String {
    let page = issued(rig, cookie, "laptop").await;
    value_of(&page)
}

#[tokio::test]
async fn the_lead_answers_the_old_guardrails_url_with_a_redirect_to_its_own_path() {
    // oml-web serves the view at /web/guardrails (ADR-0070 d7 (b) as
    // amended): the lead sends the old ?view= form there, to a fixed target
    // that carries nothing from the request, signed in or not.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    for (old, cookie) in [
        ("/web?view=guardrails", Some(cookie.as_str())),
        (
            "/web?view=guardrails&workarea=%2F%2Fevil.example",
            Some(cookie.as_str()),
        ),
        ("/web?view=guardrails", None),
    ] {
        let (status, headers, page) = send(&rig.router, get(old, cookie)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{old}: {page}");
        assert_eq!(headers[header::LOCATION], "/web/guardrails", "{old}");
        assert!(!page.contains("Rules in force at"), "{page}");
    }
    // The lead serves no page at the moved path.
    let (status, _, _) = send(&rig.router, get("/web/guardrails", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_lead_answers_the_workareas_view_and_its_acts_with_a_redirect_to_their_own_paths() {
    // oml-web serves the Workareas view and runs its acts at
    // /web/workareas and /web/admin/workareas (ADR-0070 d7 (b) as amended):
    // the lead sends the old ?view= forms there, carrying nothing from the
    // request, and runs none of the old acts.
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (old, cookie, new) in [
        (
            "/web?view=workarea",
            Some(cookie.as_str()),
            "/web/workareas",
        ),
        (
            "/web?view=workarea&workarea=alice&op=mode:in_progress",
            Some(cookie.as_str()),
            "/web/workareas",
        ),
        ("/web?view=workarea", None, "/web/workareas"),
        (
            "/web/admin?view=workarea",
            Some(boss.as_str()),
            "/web/admin/workareas",
        ),
        ("/web/admin?view=workarea", None, "/web/admin/workareas"),
    ] {
        let (status, headers, page) = send(&rig.router, get(old, cookie)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{old}: {page}");
        assert_eq!(headers[header::LOCATION], new, "{old}");
    }
    // Creating a workarea is the view's act too (ADR-0070 d7 (b)); the
    // tenant's admin key makes the one these acts name, through the API.
    let root = key_named(&rig, "alice-web").await.user.expect("a root");
    let (status, _, created) = send(
        &rig.router,
        Request::post("/v2/workareas")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "create-proj")
            .body(Body::from(
                json!({"name": "proj", "parent": root.to_string()}).to_string(),
            ))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let proj: uuid::Uuid = serde_json::from_str::<Value>(&created).expect("JSON")["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid");
    let (status, headers, page) = act(
        &rig,
        &cookie,
        "/web",
        &[("action", "create_workarea"), ("name", "docs")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{page}");
    assert_eq!(headers[header::LOCATION], "/web/workareas");
    assert!(
        rig.store
            .workarea_children(rig.boot.tenant_id, root)
            .await
            .unwrap()
            .iter()
            .all(|w| w.name.as_str() != "docs"),
        "the lead created nothing"
    );
    for (cookie, path, fields, new) in [
        (
            &cookie,
            "/web",
            [
                ("action", "workarea_mode"),
                ("workarea", "alice.proj"),
                ("mode", "in_progress"),
            ],
            "/web/workareas",
        ),
        (
            &cookie,
            "/web",
            [
                ("action", "workarea_transition"),
                ("workarea", "alice.proj"),
                ("to", "committed"),
            ],
            "/web/workareas",
        ),
        (
            &boss,
            "/web/admin",
            [
                ("action", "workarea_transition"),
                ("workarea", "alice.proj"),
                ("to", "committed"),
            ],
            "/web/admin/workareas",
        ),
        (
            &boss,
            "/web/admin",
            [
                ("action", "workarea_mode"),
                ("workarea", "alice.proj"),
                ("mode", "in_progress"),
            ],
            "/web/admin/workareas",
        ),
        (
            &cookie,
            "/web",
            [
                ("action", "workarea_ratify"),
                ("workarea", "alice.proj"),
                ("transition", "00000000-0000-4000-8000-000000000001"),
            ],
            "/web/workareas",
        ),
        (
            &boss,
            "/web/admin",
            [
                ("action", "workarea_ratify"),
                ("workarea", "alice.proj"),
                ("transition", "00000000-0000-4000-8000-000000000001"),
            ],
            "/web/admin/workareas",
        ),
    ] {
        let (status, headers, page) = act(&rig, cookie, path, &fields).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{path}: {page}");
        assert_eq!(headers[header::LOCATION], new, "{path}");
    }
    let node = rig
        .store
        .workarea(rig.boot.tenant_id, proj)
        .await
        .unwrap()
        .unwrap();
    assert!(!node.in_progress, "the lead ran no act");
    assert!(
        rig.store
            .pending_transitions(rig.boot.tenant_id, proj)
            .await
            .unwrap()
            .is_empty(),
        "the lead proposed nothing"
    );
    // The lead serves no page at the moved paths.
    for path in ["/web/workareas", "/web/admin/workareas"] {
        let (status, _, _) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn the_lead_answers_the_memories_views_with_a_redirect_to_their_own_paths() {
    // oml-web serves the Memories list and the Atlas at /web/memories,
    // /web/atlas, /web/admin/memories and /web/admin/atlas, and their JSON
    // at /web/data/memories and /web/admin/data/memories (ADR-0070 d7 (b)
    // as amended): the lead sends the old ?view= forms there, carrying
    // nothing from the request, signed in or not.
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let (cookie, agent) = person_with_agent(&rig, "sub-1", EMAIL).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (old, cookie, new) in [
        ("/web?view=memories", Some(cookie.as_str()), "/web/memories"),
        (
            "/web?view=memories&state=held&cursor=x",
            Some(cookie.as_str()),
            "/web/memories",
        ),
        ("/web?view=memories", None, "/web/memories"),
        (
            "/web?view=memories&display=atlas",
            Some(cookie.as_str()),
            "/web/atlas",
        ),
        (
            "/web?view=memories&display=atlas&workarea=alice",
            None,
            "/web/atlas",
        ),
        (
            "/web/admin?view=memories",
            Some(boss.as_str()),
            "/web/admin/memories",
        ),
        ("/web/admin?view=memories", None, "/web/admin/memories"),
        (
            "/web/admin?view=memories&display=atlas",
            Some(boss.as_str()),
            "/web/admin/atlas",
        ),
    ] {
        let (status, headers, page) = send(&rig.router, get(old, cookie)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{old}: {page}");
        assert_eq!(headers[header::LOCATION], new, "{old}");
    }
    // One memory opened by its id goes to its own page, carrying the id
    // alone, parsed and written anew (ADR-0070 d7 (b), sixth change).
    let memory = agent_writes(&rig, &agent, "opened on oml-web").await;
    for (old, cookie, new) in [
        (
            format!("/web?view=memories&memory={memory}&state=held"),
            Some(cookie.as_str()),
            format!("/web/memory?id={memory}"),
        ),
        (
            format!("/web?view=memories&memory={}", memory.simple()),
            None,
            format!("/web/memory?id={memory}"),
        ),
        (
            format!("/web/admin?view=memories&memory={memory}"),
            Some(boss.as_str()),
            format!("/web/admin/memory?id={memory}"),
        ),
        (
            "/web/admin?view=curate&memory=x".to_owned(),
            None,
            "/web/admin/curate".to_owned(),
        ),
    ] {
        let (status, headers, page) = send(&rig.router, get(&old, cookie)).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{old}: {page}");
        assert_eq!(headers[header::LOCATION], new.as_str(), "{old}");
    }
    // A memory that is not one id is no redirect and no page.
    let (status, headers, _) = send(
        &rig.router,
        get(
            "/web?view=memories&memory=%2F%2Fevil.example",
            Some(&cookie),
        ),
    )
    .await;
    assert_ne!(status, StatusCode::SEE_OTHER);
    assert!(headers.get(header::LOCATION).is_none());
    // The lead serves no page and no JSON at the moved paths.
    for path in [
        "/web/memories",
        "/web/atlas",
        "/web/admin/memories",
        "/web/admin/atlas",
        "/web/data/memories",
        "/web/admin/data/memories",
        "/web/memory",
        "/web/admin/memory",
        "/web/admin/curate",
        "/web/assets/mermaid-0.js",
    ] {
        let (status, _, _) = send(&rig.router, get(path, Some(&boss))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn a_sign_in_form_that_repeats_a_field_is_refused() {
    let rig = rig().await;
    for fields in [
        [("provider", "google"), ("provider", "github")],
        [("next", "/web"), ("next", "/web/admin")],
    ] {
        let mut all = fields.to_vec();
        if all[0].0 == "next" {
            all.push(("provider", "google"));
        }
        let (status, _, page) = send(&rig.router, post("/web/sign-in", &all, None)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{fields:?}: {page}");
    }
}

/// The `next` field the sign-in page's provider forms carry.
fn next_field(page: &str) -> Option<String> {
    let marker = r#"name="next" value=""#;
    let start = page.find(marker)? + marker.len();
    Some(page[start..start + page[start..].find('"')?].replace("&amp;", "&"))
}

#[tokio::test]
async fn a_next_outside_web_or_naming_another_origin_returns_to_web() {
    // ADR-0070 d4: a same-origin relative path under /web, with no `//`,
    // `\`, `@` or scheme, checked again after one more percent-decoding.
    let rig = rig().await;
    for (sent, kept) in [
        ("/web?view=guardrails", "/web?view=guardrails"),
        ("/web/admin", "/web/admin"),
        ("/web", "/web"),
        ("//evil.example/web", "/web"),
        ("https://evil.example/web", "/web"),
        ("/web//evil.example", "/web"),
        ("/web\\evil.example", "/web"),
        ("/web@evil.example", "/web"),
        ("/web/%2F%2Fevil.example", "/web"),
        ("/web/%5Cevil.example", "/web"),
        ("/webfoo", "/web"),
        ("/v2/keys", "/web"),
        ("/web:evil", "/web"),
        ("", "/web"),
    ] {
        let (status, _, page) = send(
            &rig.router,
            get(&format!("/web/sign-in?next={}", pct(sent)), None),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{sent}: {page}");
        assert!(page.contains("Sign in with Google"), "{sent}: {page}");
        assert_eq!(next_field(&page).as_deref(), Some(kept), "{sent}");
    }
    // No next at all is /web.
    let (_, _, page) = send(&rig.router, get("/web/sign-in", None)).await;
    assert_eq!(next_field(&page).as_deref(), Some("/web"));
}

#[tokio::test]
async fn a_sign_in_returns_to_its_next_on_the_public_origin() {
    let rig = rig().await;
    // A person with an agent key, so onboarding does not take the landing.
    let (cookie, _) = person_with_agent(&rig, "sub-1", EMAIL).await;
    // Signed in already: /web/sign-in sends the browser straight on.
    let (status, headers, _) = send(
        &rig.router,
        get(
            &format!("/web/sign-in?next={}", pct("/web?view=guardrails")),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION],
        format!("{ISSUER}/web?view=guardrails").as_str()
    );
    // A whole sign-in started with next lands on it, built from the
    // configured origin and never from a request header.
    let (status, headers, _) = send(
        &rig.router,
        post(
            "/web/sign-in",
            &[("provider", "google"), ("next", "/web?view=guardrails")],
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned();
    let (sealed, _) = set_cookie(&headers, SIGN_IN_COOKIE).expect("the sign-in cookie");
    let state = query_param(&location, "state").expect("a state");
    let nonce = query_param(&location, "nonce").expect("a nonce");
    rig.providers.answer(
        Provider::Google,
        Ok(ProviderAnswer::GoogleIdToken(google_token(
            &nonce, "sub-1", EMAIL, true,
        ))),
    );
    let (status, headers, _) = send(
        &rig.router,
        Request::get(format!("/web/callback/google?code=c&state={}", pct(&state)))
            .header(header::COOKIE, format!("{SIGN_IN_COOKIE}={sealed}"))
            .header(header::HOST, "evil.example")
            .header("x-forwarded-host", "evil.example")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION],
        format!("{ISSUER}/web?view=guardrails").as_str()
    );
    // A next that fails the rule is dropped to /web at the callback too.
    let (status, headers, _) = send(
        &rig.router,
        post(
            "/web/sign-in",
            &[("provider", "google"), ("next", "//evil.example/web")],
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned();
    let (sealed, _) = set_cookie(&headers, SIGN_IN_COOKIE).expect("the sign-in cookie");
    let state = query_param(&location, "state").expect("a state");
    let nonce = query_param(&location, "nonce").expect("a nonce");
    rig.providers.answer(
        Provider::Google,
        Ok(ProviderAnswer::GoogleIdToken(google_token(
            &nonce, "sub-1", EMAIL, true,
        ))),
    );
    let (_, headers, _) = callback(
        &rig,
        Provider::Google,
        "c2",
        &state,
        &format!("{SIGN_IN_COOKIE}={sealed}"),
    )
    .await;
    assert_eq!(headers[header::LOCATION], "/web");
}

#[tokio::test]
async fn the_lead_answers_the_supersede_and_curate_acts_with_a_redirect_and_runs_neither() {
    // The admin page's supersede and curate acts are oml-web's at
    // /web/admin/memory and /web/admin/curate (ADR-0070 d7 (b), sixth
    // change): a cached form posting either to the lead runs nothing and
    // lands on the moved view.
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let value = laptop_key(&rig, &cookie).await;
    let id = agent_writes(&rig, &value, "a row nobody supersedes here").await;
    rig.allow("boss@example.com", "boss", true).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let head = rig.head().await;
    let target = id.to_string();
    for (fields, new) in [
        (
            vec![
                ("action", "supersede"),
                ("memory", target.as_str()),
                ("expected_version", "1"),
                ("text", "replaced"),
            ],
            "/web/admin/memories",
        ),
        (
            vec![
                ("action", "curate"),
                ("source", target.as_str()),
                ("text", "a copy"),
            ],
            "/web/admin/curate",
        ),
    ] {
        let (status, headers, body) = act(&rig, &admin, "/web/admin", &fields).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        assert_eq!(headers[header::LOCATION], new);
    }
    let events = rig
        .store
        .events_range(rig.boot.tenant_id, head + 1, 500)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .all(|e| !e.envelope.event_type.starts_with("submission.")),
        "nothing was written"
    );
}

/// A signed-in person with a writer agent key; the cookie and the value.
async fn person_with_agent(rig: &Rig, subject: &str, email: &str) -> (String, String) {
    let cookie = signed_in(rig, subject, email).await;
    let page = issued(rig, &cookie, "laptop").await;
    (cookie, value_of(&page))
}

#[tokio::test]
async fn the_lead_answers_the_review_and_keys_views_and_their_acts_with_a_redirect_to_their_own_paths()
 {
    // oml-web serves the Review and Keys views and runs their acts at
    // /web/review, /web/keys and their admin paths (ADR-0070 d7 (b) as
    // amended): the lead sends the old forms there, carrying nothing from
    // the request, and runs none of the acts.
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for (old, new) in [
        ("/web", "/web/keys"),
        ("/web?done=cut+off", "/web/keys"),
        ("/web?view=keys", "/web/keys"),
        ("/web?view=review", "/web/review"),
        ("/web/admin?view=review", "/web/admin/review"),
        (
            "/web/admin?view=held&cursor=%2F%2Fevil",
            "/web/admin/review",
        ),
        ("/web/admin?view=queue", "/web/admin/review"),
    ] {
        let (status, headers, page) = send(&rig.router, get(old, Some(&cookie))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{old}: {page}");
        assert_eq!(headers[header::LOCATION], new, "{old}");
    }
    let before = rig.head().await;
    for (page, action, new) in [
        ("/web", "issue_key", "/web/keys"),
        ("/web", "connect_key", "/web/keys"),
        ("/web", "connect_code", "/web/keys"),
        ("/web", "cut_off", "/web/keys"),
        ("/web", "review_next", "/web/review"),
        ("/web", "verdict", "/web/review"),
        ("/web/admin", "review_next", "/web/admin/review"),
        ("/web/admin", "verdict", "/web/admin/review"),
        ("/web/admin", "bulk_confirm", "/web/admin/review"),
        ("/web/admin", "bulk_verdict", "/web/admin/review"),
        ("/web/admin", "cut_off", "/web/admin/keys"),
    ] {
        let (status, headers, body) = act(
            &rig,
            &cookie,
            page,
            &[
                ("action", action),
                ("name", "laptop"),
                ("key", "root-admin"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{page} {action}: {body}");
        assert_eq!(headers[header::LOCATION], new, "{page} {action}");
    }
    assert_eq!(
        rig.head().await,
        before,
        "the lead ran none of the old acts"
    );
    // The lead serves no page at the moved paths.
    for path in [
        "/web/review",
        "/web/keys",
        "/web/admin/review",
        "/web/admin/keys",
    ] {
        let (status, _, _) = send(&rig.router, get(path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

// ---------------------------------------------------------------------------
// #266: a chain conflict after a disable's first commit never journals it twice
// ---------------------------------------------------------------------------

/// A call to the API with the tenant's admin key, as `oml admin` makes it.
async fn as_admin(rig: &Rig, path: &str, idem: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::post(path)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", rig.boot.admin_key.value),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", idem)
        .body(Body::from(body.to_string()))
        .expect("a request");
    let (status, _, answer) = send(&rig.router, request).await;
    (status, serde_json::from_str(&answer).unwrap_or(Value::Null))
}

/// The tenant's events of `event_type`, oldest first.
async fn journalled(rig: &Rig, event_type: &str) -> Vec<u64> {
    rig.store
        .events_range(rig.boot.tenant_id, 1, 1000)
        .await
        .expect("the chain")
        .iter()
        .filter(|e| e.envelope.event_type == event_type)
        .map(|e| e.envelope.seq)
        .collect()
}

/// A chain-head conflict as a concurrent writer causes it.
fn lost_race() -> oml_app::ports::StoreError {
    oml_app::ports::StoreError::SeqConflict {
        expected_seq: 0,
        found_seq: 0,
    }
}

/// alice signed in, with two writer keys of her own to cut off.
async fn alice_with_two_keys(rig: &Rig) -> String {
    let cookie = signed_in(rig, "sub-1", EMAIL).await;
    let _ = issued(rig, &cookie, "laptop").await;
    let _ = issued(rig, &cookie, "phone").await;
    cookie
}

#[tokio::test]
async fn a_disable_whose_key_denial_loses_the_chain_race_journals_web_user_disabled_once() {
    let rig = rig().await;
    let _ = alice_with_two_keys(&rig).await;
    rig.store
        .refuse_next_commit_of(oml_app::domain::event_types::KEY_DENIED, lost_race());
    let (status, done) =
        as_admin(&rig, "/v2/web/users/alice/disable", "disable-1", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let disabled = journalled(&rig, oml_app::domain::event_types::WEB_USER_DISABLED).await;
    assert_eq!(disabled.len(), 1, "journalled once: {disabled:?}");
    assert_eq!(done["journal_seq"], disabled[0], "{done}");
    assert_eq!(done["keys_cut_off"], 2, "{done}");
    assert_eq!(
        journalled(&rig, oml_app::domain::event_types::KEY_DENIED)
            .await
            .len(),
        2,
        "each key denied once"
    );
}

#[tokio::test]
async fn a_close_whose_closing_commit_loses_the_chain_race_journals_each_event_once() {
    let rig = rig().await;
    let cookie = alice_with_two_keys(&rig).await;
    let (status, asked) =
        as_web(&rig, &cookie, "web", "/v2/web/deletion-requests", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{asked}");
    rig.store.refuse_next_commit_of(
        oml_app::domain::event_types::WEB_DELETION_CLOSED,
        lost_race(),
    );
    let (status, done) = as_admin(
        &rig,
        "/v2/web/users/alice/deletion/close",
        "close-1",
        json!({ "outcome": "deleted" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    use oml_app::domain::event_types::{KEY_DENIED, WEB_DELETION_CLOSED, WEB_USER_DISABLED};
    assert_eq!(
        journalled(&rig, WEB_USER_DISABLED).await.len(),
        1,
        "disabled once"
    );
    assert_eq!(
        journalled(&rig, KEY_DENIED).await.len(),
        2,
        "each key denied once"
    );
    let closed = journalled(&rig, WEB_DELETION_CLOSED).await;
    assert_eq!(closed.len(), 1, "closed once");
    assert_eq!(done["journal_seq"], closed[0], "{done}");
}

/// A key under the person's root that no denial can cut off, as an admin
/// key there is (`DenialOnAdminKey`), is skipped: the disable denies every
/// other key, journals `web.user_disabled` once, answers 200, and a
/// resubmission under the same key replays it, so no refusal ever follows
/// the disable's first commit (ADR-0068 d8, #266).
#[tokio::test]
async fn a_disable_skips_a_key_it_cannot_deny_denies_the_rest_and_a_resubmission_replays() {
    use oml_app::domain::event_types::{KEY_DENIED, WEB_USER_DISABLED};
    let rig = rig().await;
    let _ = alice_with_two_keys(&rig).await;
    let stray = rig
        .app
        .issue_key(
            &rig.boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Admin,
                name: Name::parse("stray-admin").expect("a name"),
                principal: "operator-2".to_owned(),
                held_by: Some(HeldBy::Human),
                default_workarea: None,
                crew: None,
                grants: Vec::new(),
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("an admin key");
    let root = key_named(&rig, "alice-web")
        .await
        .user
        .expect("alice's root");
    rig.store
        .set_key_user(rig.boot.tenant_id, stray.key_id, Some(root));
    let request = || {
        Request::post("/v2/web/users/alice/disable")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "disable-stray")
            .body(Body::from("{}"))
            .expect("a request")
    };
    let (status, _, body) = send(&rig.router, request()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let done: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(done["keys_cut_off"], 2, "{done}");
    assert_eq!(journalled(&rig, WEB_USER_DISABLED).await.len(), 1);
    assert_eq!(journalled(&rig, KEY_DENIED).await.len(), 2);
    let (status, headers, again) = send(&rig.router, request()).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(headers["idempotency-replay"], "true", "{again}");
    assert_eq!(
        journalled(&rig, WEB_USER_DISABLED).await.len(),
        1,
        "the resubmission replays"
    );
}
