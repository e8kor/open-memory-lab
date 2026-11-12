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
use oml_transport_http::web::{SESSION_COOKIE, SIGN_IN_COOKIE, USER_ACTIONS, WEB_ROUTES, WebPages};

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
        assert!(
            status != StatusCode::NOT_FOUND || body.contains("access.not_found"),
            "{method} {path}"
        );
    }
    assert_eq!(
        WEB_ROUTES.len(),
        6,
        "a new route is a new decision amending ADR-0045; ADR-0068 d10 added seven, ADR-0070 d4 GET /web/sign-in, ADR-0070 d7 (b) moved the two JSON routes, the onboarding page with its poll and export, and the users page to oml-web, and v0.7.1 deleted GET and POST /web/admin (ADR-0070, \"The old URLs deleted\")"
    );
    assert_eq!(USER_ACTIONS, ["sign_out"]);
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
    // The session stands: a signed-in GET /web goes on to the Keys view.
    let (status, headers, _) = send(&rig.router, get("/web", Some(&cookie))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/web/keys", "the session stands");
}

#[tokio::test]
async fn a_post_from_another_origin_is_refused_before_the_session_is_read() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    for (name, value) in [("sec-fetch-site", "cross-site"), ("origin", "null")] {
        for path in ["/web", "/web/sign-in"] {
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
async fn the_old_page_urls_and_acts_answer_not_found_or_refused_and_run_nothing() {
    // The 303s from the page URLs and acts of v0.6 are deleted in v0.7.1
    // (ADR-0070, "The old URLs deleted"): the old admin page URL is an
    // unknown path, an old `?view=` is a view the lead does not have, and an
    // old act is no action of `POST /web`, signed in or not, admin or not.
    let rig = rig().await;
    rig.allow("boss@example.com", "boss", true).await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let admin = signed_in(&rig, "sub-boss", "boss@example.com").await;
    for who in [Some(cookie.as_str()), Some(admin.as_str()), None] {
        for path in ["/web/admin", "/web/admin?view=people"] {
            let (status, headers, _) = send(&rig.router, get(path, who)).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "GET {path}");
            assert!(headers.get(header::LOCATION).is_none(), "GET {path}");
        }
    }
    let head = rig.head().await;
    let csrf = csrf_for(&admin);
    let request = Request::post("/web/admin")
        .header(header::CONTENT_TYPE, FORM)
        .header(header::COOKIE, &admin)
        .body(Body::from(form(&[
            ("action", "allow_email"),
            ("csrf_token", &csrf),
            ("email", "carol@example.com"),
        ])))
        .expect("a request");
    let (status, headers, _) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "POST /web/admin");
    assert!(headers.get(header::LOCATION).is_none());
    for view in [
        "guardrails",
        "setup",
        "workarea",
        "memories",
        "review",
        "keys",
    ] {
        let path = format!("/web?view={view}");
        let (status, headers, body) = send(&rig.router, get(&path, Some(&admin))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        assert!(body.contains("access.not_found"), "{path}: {body}");
        assert!(headers.get(header::LOCATION).is_none(), "{path}");
    }
    for action in [
        "issue_key",
        "connect_key",
        "connect_code",
        "cut_off",
        "create_workarea",
        "workarea_mode",
        "review_next",
        "verdict",
    ] {
        let (status, headers, body) = act(&rig, &admin, "/web", &[("action", action)]).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{action}: {body}");
        assert!(
            body.contains("request.validation") && body.contains("action"),
            "{action}: {body}"
        );
        assert!(headers.get(header::LOCATION).is_none(), "{action}");
    }
    assert_eq!(rig.head().await, head, "nothing journalled");
    // The onboarding and users pages are not the lead's at all.
    for path in [
        "/web/start",
        "/web/start/status",
        "/web/start/export",
        "/web/admin/users",
    ] {
        let (status, _, _) = send(&rig.router, get(path, Some(&admin))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
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

/// A signed-in person with a writer agent key; the cookie and the value.
async fn person_with_agent(rig: &Rig, subject: &str, email: &str) -> (String, String) {
    let cookie = signed_in(rig, subject, email).await;
    let page = issued(rig, &cookie, "laptop").await;
    (cookie, value_of(&page))
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

/// An admin key of the tenant whose row names alice's root, as only a
/// direct row edit makes one (no verb issues an admin key under a user
/// root): the fixture of #277.
async fn stray_admin_under_alice(rig: &Rig) -> uuid::Uuid {
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
    let root = key_named(rig, "alice-web")
        .await
        .user
        .expect("alice's root");
    rig.store
        .set_key_user(rig.boot.tenant_id, stray.key_id, Some(root));
    stray.key_id
}

/// Whether `key_id` carries a denial on any node.
async fn denied(rig: &Rig, key_id: uuid::Uuid) -> bool {
    rig.store
        .key_access(rig.boot.tenant_id, key_id)
        .await
        .expect("the key's access")
        .is_some_and(|a| !a.denials.is_empty())
}

/// An admin key whose row names the person's root is a corrupt row, not a
/// key the person holds, and no denial can cut it off: the disable refuses
/// before its first commit with `workarea.admin_key_under_root`, naming the
/// key, journals nothing and denies no key, the stray one included; once
/// the row is repaired the disable cuts off the person's keys (ADR-0068 d8,
/// d11; #277).
#[tokio::test]
async fn a_disable_is_refused_by_name_while_an_admin_key_names_the_persons_root_and_journals_nothing()
 {
    use oml_app::domain::event_types::{KEY_DENIED, WEB_USER_DISABLED};
    let rig = rig().await;
    let _ = alice_with_two_keys(&rig).await;
    let stray = stray_admin_under_alice(&rig).await;
    let (status, refused) = as_admin(
        &rig,
        "/v2/web/users/alice/disable",
        "disable-stray",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(
        refused["error"]["code"], "workarea.admin_key_under_root",
        "{refused}"
    );
    assert_eq!(
        refused["error"]["details"]["key"], "stray-admin",
        "{refused}"
    );
    assert_eq!(refused["error"]["details"]["user"], "alice", "{refused}");
    assert!(journalled(&rig, WEB_USER_DISABLED).await.is_empty());
    assert!(journalled(&rig, KEY_DENIED).await.is_empty());
    assert!(!denied(&rig, stray).await, "the stray key is not denied");
    rig.store.set_key_user(rig.boot.tenant_id, stray, None);
    let (status, done) = as_admin(
        &rig,
        "/v2/web/users/alice/disable",
        "disable-repaired",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["keys_cut_off"], 2, "{done}");
    assert_eq!(journalled(&rig, WEB_USER_DISABLED).await.len(), 1);
    assert_eq!(journalled(&rig, KEY_DENIED).await.len(), 2);
    assert!(
        !denied(&rig, stray).await,
        "the repaired key is not alice's"
    );
}

/// A deletion close disables the person first, so it is refused the same
/// way and closes nothing while an admin key names the root (ADR-0068 d6,
/// d8; #277).
#[tokio::test]
async fn a_deletion_close_is_refused_by_name_while_an_admin_key_names_the_persons_root() {
    use oml_app::domain::event_types::{WEB_DELETION_CLOSED, WEB_USER_DISABLED};
    let rig = rig().await;
    let cookie = alice_with_two_keys(&rig).await;
    let (status, asked) =
        as_web(&rig, &cookie, "web", "/v2/web/deletion-requests", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{asked}");
    let _ = stray_admin_under_alice(&rig).await;
    let (status, refused) = as_admin(
        &rig,
        "/v2/web/users/alice/deletion/close",
        "close-stray",
        json!({ "outcome": "deleted" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(
        refused["error"]["code"], "workarea.admin_key_under_root",
        "{refused}"
    );
    assert!(journalled(&rig, WEB_USER_DISABLED).await.is_empty());
    assert!(journalled(&rig, WEB_DELETION_CLOSED).await.is_empty());
}

/// A key gone between the disable's listing and its denial (`NotFound`)
/// is skipped: every other key is cut off, `web.user_disabled` is
/// journalled once, the answer counts only the keys cut off, and a
/// resubmission under the same key replays it (ADR-0068 d8; #266, #277).
#[tokio::test]
async fn a_disable_skips_a_key_gone_since_the_listing_and_cuts_off_the_rest() {
    use oml_app::domain::event_types::{KEY_DENIED, WEB_USER_DISABLED};
    let rig = rig().await;
    let _ = alice_with_two_keys(&rig).await;
    let gone = key_named(&rig, "alice-laptop").await.key_id;
    let kept = key_named(&rig, "alice-phone").await.key_id;
    rig.store.vanish_key(gone);
    let (status, done) = as_admin(
        &rig,
        "/v2/web/users/alice/disable",
        "disable-gone",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["keys_cut_off"], 1, "{done}");
    assert_eq!(journalled(&rig, WEB_USER_DISABLED).await.len(), 1);
    assert_eq!(journalled(&rig, KEY_DENIED).await.len(), 1);
    assert!(denied(&rig, kept).await, "the listed key is cut off");
    assert!(!denied(&rig, gone).await, "the gone key is skipped");
    let (status, again) = as_admin(
        &rig,
        "/v2/web/users/alice/disable",
        "disable-gone",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, done, "the resubmission replays");
    assert_eq!(journalled(&rig, WEB_USER_DISABLED).await.len(), 1);
}

/// `GET /v2/web/users` names, per person, each live admin key whose row
/// names their root, which `oml doctor` warns about (ADR-0068 d11; #277).
#[tokio::test]
async fn the_users_listing_names_an_admin_key_whose_row_names_a_persons_root() {
    let rig = rig().await;
    let _ = alice_with_two_keys(&rig).await;
    let listed = || async {
        let request = Request::get("/v2/web/users")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .body(Body::empty())
            .expect("a request");
        let (status, _, body) = send(&rig.router, request).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let v: Value = serde_json::from_str(&body).expect("JSON");
        v["users"]
            .as_array()
            .expect("users")
            .iter()
            .find(|u| u["user"] == "alice")
            .expect("alice")
            .clone()
    };
    assert_eq!(listed().await["admin_keys"], json!([]));
    let _ = stray_admin_under_alice(&rig).await;
    assert_eq!(listed().await["admin_keys"], json!(["stray-admin"]));
}

// ---------------------------------------------------------------------------
// #325: a session key an admin denied runs none of its key acts
// ---------------------------------------------------------------------------

/// alice signed in, with a writer key and a connector key of her own and
/// the workarea `notes` in her root, and then denied: the admin denied
/// `alice-web` on `workarea`, `notes` standing for that workarea's id.
async fn alice_denied_on(rig: &Rig, workarea: &str) -> String {
    let cookie = signed_in(rig, "sub-1", EMAIL).await;
    let _ = issued(rig, &cookie, "laptop").await;
    let (status, notes) = as_web(
        rig,
        &cookie,
        "web",
        "/v2/workareas",
        json!({"name": "notes"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{notes}");
    let workarea = match workarea {
        "notes" => notes["id"].as_str().expect("an id"),
        named => named,
    };
    let (status, phone) = as_web(
        rig,
        &cookie,
        "web",
        "/v2/keys",
        json!({"name": "alice-phone", "role": "writer", "connect_code": true}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{phone}");
    let (status, denial) = as_admin(
        rig,
        "/v2/keys/alice-web/denials",
        "deny-alice-web",
        json!({"workarea": workarea}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{denial}");
    cookie
}

/// The refusal a denied session key's key act answers: `access.verb_ceiling`
/// naming the verb, `details.reason: session_key_denied`.
fn assert_session_key_denied(status: StatusCode, answer: &Value, verb: &str) {
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(answer["error"]["code"], "access.verb_ceiling", "{answer}");
    assert_eq!(
        answer["error"]["details"]["reason"], "session_key_denied",
        "{answer}"
    );
    assert_eq!(answer["error"]["details"]["verb"], verb, "{answer}");
}

/// A `<user>-web` key an admin cut off issues no key: the answer names the
/// denial, nothing is journalled and no key exists; a denial on the person's
/// root, or on a workarea inside it, refuses the same way, since the key it
/// would issue holds the whole root (ADR-0070 d2; ADR-0045 d16; #325).
#[tokio::test]
async fn a_cut_off_session_key_issues_no_key() {
    for workarea in ["universe", "alice", "notes"] {
        let rig = rig().await;
        let cookie = alice_denied_on(&rig, workarea).await;
        let head = rig.head().await;
        for (name, connect_code) in [("alice-tablet", false), ("alice-watch", true)] {
            let (status, answer) = as_web(
                &rig,
                &cookie,
                "web",
                "/v2/keys",
                json!({"name": name, "role": "writer", "connect_code": connect_code}),
            )
            .await;
            assert_session_key_denied(status, &answer, "issue_key");
            let named = rig
                .store
                .resolve_name(
                    rig.boot.tenant_id,
                    oml_app::domain::name::NamedKind::Key,
                    &Name::parse(name).expect("a name"),
                )
                .await
                .expect("a lookup");
            assert_eq!(named, None, "{workarea}: no key {name}");
        }
        assert_eq!(rig.head().await, head, "{workarea}: nothing journalled");
    }
}

/// A `<user>-web` key an admin cut off mints no connect code for the
/// person's connector key (ADR-0070 d2; #325).
#[tokio::test]
async fn a_cut_off_session_key_mints_no_connect_code() {
    let rig = rig().await;
    let cookie = alice_denied_on(&rig, "universe").await;
    let head = rig.head().await;
    let (status, answer) = as_web(
        &rig,
        &cookie,
        "web",
        "/v2/keys/alice-phone/connect-codes",
        json!({}),
    )
    .await;
    assert_session_key_denied(status, &answer, "mint_connect_code");
    assert_eq!(rig.head().await, head, "nothing journalled");
}

/// A `<user>-web` key an admin cut off cuts off none of the person's keys
/// (ADR-0070 d2; #325).
#[tokio::test]
async fn a_cut_off_session_key_cuts_off_no_key() {
    let rig = rig().await;
    let cookie = alice_denied_on(&rig, "universe").await;
    let head = rig.head().await;
    let (status, answer) = as_web(
        &rig,
        &cookie,
        "web",
        "/v2/keys/alice-laptop/denials",
        json!({"workarea": "universe"}),
    )
    .await;
    assert_session_key_denied(status, &answer, "deny_key");
    let laptop = key_named(&rig, "alice-laptop").await;
    assert!(!denied(&rig, laptop.key_id).await, "the key is not cut off");
    assert_eq!(rig.head().await, head, "nothing journalled");
}

/// A cut-off session key still ends its session, and every credential of
/// the session ends with it: refusing the sign-out would keep the session
/// of a key an admin cut off (ADR-0070 d2, d5 #5; #325).
#[tokio::test]
async fn a_cut_off_session_key_still_ends_its_session() {
    let rig = rig().await;
    let cookie = alice_denied_on(&rig, "universe").await;
    let whoami = || async {
        let token = cookie.split_once('=').expect("a session cookie").1;
        let credential = oml_app::domain::web::capacity_credential(token, "web");
        let request = Request::get("/v2/whoami")
            .header(header::AUTHORIZATION, format!("Bearer {credential}"))
            .header("oml-web-client", WEB_CLIENT_SECRET)
            .body(Body::empty())
            .expect("a request");
        send(&rig.router, request).await.0
    };
    assert_eq!(whoami().await, StatusCode::OK, "the session is live");
    let (status, answer) = as_web(&rig, &cookie, "web", "/v2/web/session/end", json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{answer}");
    assert_eq!(
        whoami().await,
        StatusCode::UNAUTHORIZED,
        "the session ended"
    );
}
