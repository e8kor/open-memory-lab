//! The route wall of ADR-0070 d7 (d): every page is `oml-web`'s, and the
//! lead serves under `/web` only what the auth context keeps (sign-in, its
//! callback, sign-up and its terms page, sign-out). The `303`s from the old
//! page URLs are deleted in v0.7.1 (ADR-0070, "The old URLs deleted"), so
//! the old admin page URL is an unknown path. A page route added to the
//! lead, in its web table, in its API table or at one of `oml-web`'s paths,
//! fails this test.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt as _;

use oml_app::App;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::Provider;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSignIn, MemSigner, MemStore, SeqIdGen};
use oml_transport_http::web::{WEB_ROUTES, WebPages};
use oml_transport_http::{ApiState, DbPing, OAUTH_ROUTES, ROUTES};

/// What one of the lead's web routes is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Serves {
    /// The auth context's (ADR-0070 d6): sign-in, its callback, sign-up and
    /// sign-out. `GET /web` without a session is the sign-in page, and the
    /// callback draws the terms page for an address not yet signed up
    /// (ADR-0068 d2); `POST /web` runs `sign_out` alone.
    Auth,
}

/// The lead's web routes, each named: exactly [`WEB_ROUTES`].
const LEAD_WEB_ROUTES: &[(&str, &str, Serves)] = &[
    ("GET", "/web", Serves::Auth),
    ("POST", "/web", Serves::Auth),
    ("GET", "/web/sign-in", Serves::Auth),
    ("POST", "/web/sign-in", Serves::Auth),
    ("GET", "/web/callback/{provider}", Serves::Auth),
    ("POST", "/web/sign-up", Serves::Auth),
];

/// The lead over the in-memory doubles with `oauth.issuer` and both sign-in
/// providers set, so it routes every table it has.
fn lead() -> Router {
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new([7; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_oauth_issuer(Issuer::parse("https://oml.example.com").expect("a public origin"));
    let ping: DbPing = Arc::new(|| Box::pin(async { true }));
    ApiState::new(Arc::new(app), store, ping, true, None)
        .with_web(WebPages::new(
            vec![
                (Provider::Google, "google-client".to_owned()),
                (Provider::GitHub, "github-client".to_owned()),
            ],
            MemSignIn::new(),
        ))
        .into_router()
}

async fn answer(router: &Router, method: &str, path: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request builds");
    let response = router.clone().oneshot(request).await.expect("infallible");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1 << 22).await.expect("body");
    (status, body.to_vec())
}

#[tokio::test]
async fn the_lead_serves_no_page_route_outside_oml_web_and_auth() {
    // Its web table is exactly the named routes, in order.
    let named: Vec<(&str, &str)> = LEAD_WEB_ROUTES.iter().map(|&(m, p, _)| (m, p)).collect();
    assert_eq!(
        WEB_ROUTES, named,
        "the lead's web routes are the auth context's alone"
    );

    // Its other tables hold no page: the API, the probes, and the OAuth
    // endpoints with the connect page, which are the auth context's.
    for &(method, path) in ROUTES {
        assert!(
            path.starts_with("/v1/")
                || path.starts_with("/v2/")
                || ["/healthz", "/readyz"].contains(&path),
            "{method} {path}: the lead's API table holds a route outside the API"
        );
    }
    for &(method, path) in OAUTH_ROUTES {
        assert!(
            path.starts_with("/oauth/") || path.starts_with("/.well-known/"),
            "{method} {path}: the lead's OAuth table holds a route outside OAuth"
        );
    }

    // Every path oml-web serves is, on the lead, an unknown path.
    let router = lead();
    let unknown = answer(&router, "GET", "/web/no-such-page").await;
    assert_eq!(unknown.0, StatusCode::NOT_FOUND, "an unknown path");

    // The old admin page URL, whose `303`s v0.7.1 deletes, is one too.
    for method in ["GET", "POST"] {
        assert_eq!(
            answer(&router, method, "/web/admin").await,
            unknown,
            "{method} /web/admin is answered as a route of the lead's"
        );
    }
    let prefixed: Vec<(&str, String)> = oml_web::PREFIXES
        .iter()
        .map(|prefix| ("GET", format!("{prefix}x")))
        .collect();
    let pages = oml_web::PAGES
        .iter()
        .map(|&(method, path)| (method, path.to_owned()))
        .chain(prefixed);
    for (method, path) in pages {
        assert!(
            !WEB_ROUTES.contains(&(method, path.as_str())),
            "{method} {path} is oml-web's and in the lead's web table"
        );
        assert_eq!(
            answer(&router, method, &path).await,
            unknown,
            "{method} {path} is oml-web's, and the lead answers it as a route of its own"
        );
    }

    // The walk is not vacuous: each named route is answered by a handler.
    for &(method, path, _) in LEAD_WEB_ROUTES {
        let path = path.replace("{provider}", "github");
        let (status, body) = answer(&router, method, &path).await;
        assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
        assert!(
            status != StatusCode::NOT_FOUND || !body.is_empty(),
            "{method} {path} is not routed"
        );
    }
}
