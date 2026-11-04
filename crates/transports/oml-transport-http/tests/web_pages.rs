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
use oml_transport_http::traffic::ScopeObserver;
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
}

fn test_app(store: &MemStore) -> TestApp {
    app(store)
        .with_observer(Arc::new(ScopeObserver))
        .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"))
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
    let router = state(&store, test_app(&store))
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

fn csrf_of(page: &str) -> String {
    let marker = r#"name="csrf_token" value=""#;
    let start = page.find(marker).expect("a csrf field") + marker.len();
    page[start..start + page[start..].find('"').expect("a quote")].to_owned()
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
        6,
        "a new route is a new decision amending ADR-0045"
    );
    assert_eq!(
        USER_ACTIONS,
        [
            "issue_key",
            "connect_key",
            "connect_code",
            "cut_off",
            "create_workarea",
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
            "curate",
            "supersede",
            "cut_off"
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
        get("/web", Some(&format!("{SESSION_COOKIE}={token}"))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("alice · oml"),
        "who and the organisation: {page}"
    );
    assert!(
        page.contains("alice-web") && page.contains("alice-review"),
        "{page}"
    );
    assert!(!page.contains(EMAIL), "the page never echoes the email");
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
    let (_, _, page) = send(&rig.router, get("/web", Some(&cookie))).await;
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
    let (_, _, page) = send(&rig.router, get("/web", Some(&cookie))).await;
    let csrf = csrf_of(&page);
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
async fn a_field_the_action_does_not_define_is_refused() {
    let rig = rig().await;
    let (_, token, _) = sign_in_google(&rig, "sub-1", EMAIL).await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    let (_, _, page) = send(&rig.router, get("/web", Some(&cookie))).await;
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
            "/web",
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
async fn the_provider_cap_answers_limit_rate_and_readyz_counts_failed_exchanges() {
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
    let (_, _, body) = send(&rig.router, get("/readyz", None)).await;
    let ready: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(ready["web"]["providers"]["google"]["failed_exchanges"], 2);
    assert_eq!(
        ready["web"]["providers"]["google"]["last_failure"],
        "error_answer"
    );
    assert_eq!(ready["web"]["providers"]["github"]["failed_exchanges"], 0);
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
    let (_, _, page) = send(&rig.router, get("/web", Some(&cookie))).await;
    let csrf = csrf_of(&page);
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
    let (_, _, page) = send(&rig.router, get("/web", Some(cookie))).await;
    let csrf = csrf_of(&page);
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

fn value_of(page: &str) -> String {
    let at = page.find("OML_API_KEY=").expect("a key value") + "OML_API_KEY=".len();
    page[at..at + page[at..].find(['<', '\n']).expect("an end")].to_owned()
}

async fn agent_writes(rig: &Rig, value: &str, body: &str) -> uuid::Uuid {
    rig.app
        .submit_memory(
            value,
            &oml_app::types::SubmitRequest {
                idempotency_key: format!("k-{body}"),
                claim_key: None,
                verb: Some(oml_app::types::SubmitVerb::Remember),
                kind: oml_app::domain::MemoryKind::Memory,
                body: body.to_owned(),
                workarea: None,
                derives_from: Vec::new(),
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
async fn a_user_issues_a_key_only_inside_its_root() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web",
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
        let (status, _, body) = act(
            &rig,
            &cookie,
            "/web",
            &[("action", "issue_key"), ("name", name), ("role", role)],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
    }
    // The keys page lists it; a cut-off denies it on universe.
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web",
        &[("action", "cut_off"), ("key", "alice-laptop")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, page) = send(&rig.router, get("/web", Some(&cookie))).await;
    assert!(page.contains("cut off"), "{page}");
    // Another user's key is not the person's to cut off.
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web",
        &[("action", "cut_off"), ("key", "root-admin")],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_connector_key_shows_its_connect_code_once() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web",
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
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web",
        &[("action", "connect_code"), ("key", "alice-phone")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Connect code"), "{page}");
}

#[tokio::test]
async fn the_user_page_shows_its_agents_held_rows_and_a_person_approves_their_own_agents_write() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act(
        &rig,
        &cookie,
        "/web",
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
    // Review on the user page acts as alice-review, which is not the author.
    let (status, _, page) = act(&rig, &cookie, "/web", &[("action", "review_next")]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("the deploy window is tuesday"), "{page}");
    let serve_id = format!("srv_{}", memory.simple());
    let (status, _, page) = act(
        &rig,
        &cookie,
        "/web",
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
    let (_, _, page) = act(
        &rig,
        &cookie,
        "/web",
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
async fn review_on_the_admin_page_acts_as_the_persons_reviewer_key_and_the_admin_key_is_never_offered_a_verdict()
 {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act(
        &rig,
        &cookie,
        "/web",
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
    let (status, _, page) = act(&rig, &admin, "/web/admin", &[("action", "review_next")]).await;
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
    let (_, _, page) = act(
        &rig,
        &cookie,
        "/web",
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
    let (status, _, _) = act(
        &rig,
        &cookie,
        "/web",
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
    let (_, _, page) = act(&rig, &admin, "/web/admin", &[("action", "review_next")]).await;
    assert!(page.contains("Keys rotate quarterly."), "{page}");
    let serve_id = format!("srv_{}", curated.memory.memory_id.simple());
    let (status, _, page) = act(
        &rig,
        &admin,
        "/web/admin",
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
async fn the_admin_read_serves_text_and_attribution_and_journals_the_read() {
    let rig = rig().await;
    let cookie = signed_in(&rig, "sub-1", EMAIL).await;
    let (_, _, page) = act(
        &rig,
        &cookie,
        "/web",
        &[
            ("action", "issue_key"),
            ("name", "laptop"),
            ("role", "writer"),
        ],
    )
    .await;
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
                query: "zebra".to_owned(),
                k: 5,
                workarea: None,
                kinds: oml_app::types::HIT_KINDS.to_vec(),
                include_text: true,
                include_unreviewed: false,
                max_tokens: None,
            },
        )
        .await
        .expect_err("an admin key still recalls nothing");
    assert!(
        format!("{refused:?}").contains("VerbCeiling"),
        "{refused:?}"
    );
    // The admin page's memories view renders the same read.
    rig.allow("boss@example.com", "boss", true).await;
    let boss = signed_in(&rig, "sub-boss", "boss@example.com").await;
    let (_, _, page) = send(
        &rig.router,
        get(
            &format!("/web/admin?view=memories&memory={memory}"),
            Some(&boss),
        ),
    )
    .await;
    assert!(
        page.contains("zebra crossing secret") && page.contains("alice-laptop"),
        "{page}"
    );
}
