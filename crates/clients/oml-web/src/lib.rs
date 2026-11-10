//! The web console as its own process: pages over the public HTTP API, and
//! nothing of the application core (ADR-0070 d1; d4 as amended on
//! 2026-11-10, `oml-web` runs only as its own process, with `web.api_url`
//! required). `tests/dependencies.rs` holds the wall.
//!
//! Phase (b) moves the pages here one at a time behind their URLs; the
//! reverse proxy sends each moved page's URL here and every other to the
//! lead. [`PAGES`] is that routing table, and [`serves`] its reading.

pub mod config;
pub mod doctor;
pub mod frame;
pub mod middleware;
pub mod outcome;
pub mod pages;
pub mod session;

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use http::StatusCode;
use serde_json::json;
use tower::ServiceBuilder;

pub use config::Config;
pub use middleware::Layers;

/// The health probe: answers without calling the API, so it says the process
/// is up, not that the API is.
pub const HEALTH_PATH: &str = "/web/healthz";

/// The readiness probe: ready when the API answers its own `/readyz` as
/// ready and names a web client identity, so the probe sees the hop. On
/// `oml-web`'s own listener; the proxy sends the public `/readyz` to the
/// lead (ADR-0070 d4).
pub const READY_PATH: &str = "/readyz";

/// The session cookie the lead's sign-in sets (ADR-0045 d5).
pub const SESSION_COOKIE: &str = "__Host-oml_session";

/// Where a person with no live session is sent: the lead's sign-in page,
/// with the page they asked for as `next` (ADR-0070 d4, d6).
pub use oml_page::SIGN_IN_PATH;

/// The pages this process serves, as `(method, path, view)`: the proxy
/// sends a request here when its method and path match and its query names
/// `view` once with that value; every other `/web` request goes to the lead
/// (ADR-0070 d4, d7 (b)). The Caddyfile in `deploy/` routes the same set.
pub const PAGES: &[(&str, &str, &str)] = &[("GET", "/web", "guardrails")];

/// Whether a request with `method`, `path` and `query` is one of [`PAGES`],
/// or the health probe.
#[must_use]
pub fn serves(method: &str, path: &str, query: Option<&str>) -> bool {
    if path == HEALTH_PATH {
        return true;
    }
    // READY_PATH is the probe of oml-web's own listener; the public
    // /readyz is the lead's, so the proxy never sends it here.
    let views: Vec<&str> = query
        .unwrap_or("")
        .split('&')
        .filter_map(|pair| pair.strip_prefix("view="))
        .collect();
    PAGES
        .iter()
        .any(|(m, p, view)| *m == method && *p == path && views == [*view])
}

/// The capacities a session's credential is derived for (ADR-0070 d2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capacity {
    Web,
    Review,
    Curate,
    Admin,
}

impl Capacity {
    /// The name the credential's derivation is keyed under.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Review => "review",
            Self::Curate => "curate",
            Self::Admin => "admin",
        }
    }
}

/// The API client the pages call: the SDK over one pooled transport, with
/// the web client's identity on every call (ADR-0070 d2, d9).
pub type Api = Arc<oml_sdk::Client<Arc<dyn oml_sdk::Transport>>>;

/// What the router shares: the layers' settings and the API.
#[derive(Clone)]
pub struct Web {
    pub layers: Layers,
    pub api: Api,
}

/// The API client over `transport`, sending `secret` as `OML-Web-Client`.
///
/// # Errors
/// The secret is not a header value.
pub fn api(transport: Arc<dyn oml_sdk::Transport>, secret: &str) -> Result<Api, String> {
    let value = http::HeaderValue::from_str(secret)
        .map_err(|_| "web.client_secret is not a header value".to_owned())?;
    Ok(Arc::new(oml_sdk::Client::new(transport).with_header(
        http::HeaderName::from_static("oml-web-client"),
        value,
    )))
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "status": "ok",
        "service": "oml-web",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// [`READY_PATH`]: one unauthenticated call to the API's `/readyz`. `200`
/// when it is ready and accepts a web client identity; `503` naming the
/// check that failed otherwise.
async fn ready(axum::extract::State(web): axum::extract::State<Web>) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let (api, web_client) = match web.api.readyz().await {
        Ok(ready) if ready.status == "ok" => {
            let identity = ready.web_client.unwrap_or_else(|| "unknown".to_owned());
            ("ok".to_owned(), identity)
        }
        Ok(ready) => (
            format!("unavailable: the API answered {}", ready.status),
            String::new(),
        ),
        Err(oml_sdk::Error::Transport(e)) => (format!("unreachable: {e}"), String::new()),
        Err(oml_sdk::Error::Api(e)) => (
            format!("unavailable: the API answered {}", e.status),
            String::new(),
        ),
        Err(e) => (format!("unavailable: {e}"), String::new()),
    };
    let web_client = if web_client == "none" {
        "none: the API accepts no capacity credential; set web.client_secret on the lead".to_owned()
    } else {
        web_client
    };
    let ok = api == "ok" && !web_client.starts_with("none");
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let mut checks = serde_json::Map::new();
    checks.insert("api".to_owned(), json!(api));
    if !web_client.is_empty() {
        checks.insert("web_client".to_owned(), json!(web_client));
    }
    (
        status,
        axum::Json(json!({
            "status": if ok { "ok" } else { "unavailable" },
            "service": "oml-web",
            "version": env!("CARGO_PKG_VERSION"),
            "checks": checks,
        })),
    )
        .into_response()
}

/// Every path no page serves; it passes the same layers as a page.
async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// The router with every layer, outermost first: ids and the line, the
/// human-only gate, the same-origin and CSRF check, the headers; then on the
/// page routes the session load.
pub fn router(web: Web) -> Router {
    let pages = Router::new()
        .route("/web", get(pages::user_page))
        .route_layer(axum::middleware::from_fn_with_state(
            web.clone(),
            session::load,
        ))
        .with_state(web.clone());
    Router::new()
        .route(HEALTH_PATH, get(health))
        .route(READY_PATH, get(ready).with_state(web.clone()))
        .merge(pages)
        .fallback(not_found)
        .layer(
            ServiceBuilder::new()
                .layer(axum::middleware::from_fn_with_state(
                    web.layers.clone(),
                    middleware::ids_and_log,
                ))
                .layer(axum::middleware::from_fn(middleware::human_only))
                .layer(axum::middleware::from_fn_with_state(
                    web.layers,
                    middleware::same_origin,
                ))
                .layer(axum::middleware::from_fn(middleware::headers)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proxy_sends_only_the_moved_views_here() {
        assert!(serves("GET", "/web", Some("view=guardrails")));
        assert!(serves("GET", "/web", Some("view=guardrails&workarea=w")));
        assert!(serves("GET", "/web/healthz", None));
        assert!(!serves("GET", "/web", None));
        assert!(!serves("GET", "/web", Some("view=keys")));
        assert!(!serves("POST", "/web", Some("view=guardrails")));
        assert!(!serves("GET", "/web/admin", Some("view=guardrails")));
        assert!(!serves(
            "GET",
            "/web",
            Some("view=guardrails&view=guardrails")
        ));
    }
}
