//! The web console as its own process: pages over the public HTTP API, and
//! nothing of the application core (ADR-0070 d1; d4 as amended on
//! 2026-11-10, `oml-web` runs only as its own process, with `web.api_url`
//! required). `tests/dependencies.rs` holds the wall.
//!
//! Phase (b) moves the pages here one at a time, each to a path of its own; the
//! reverse proxy sends each moved page's URL here and every other to the
//! lead. [`PAGES`] is that routing table, and [`serves`] its reading.

mod acts;
pub mod admin;
pub mod atlas;
pub mod brief;
pub mod builder;
mod card;
pub mod catalog;
pub mod config;
mod diff;
pub mod doctor;
pub mod fix;
pub mod imports;
pub mod investigation;
pub mod keys;
pub mod memories;
pub mod memory;
pub mod middleware;
pub mod once;
pub mod outcome;
pub mod pages;
pub mod picker;
pub mod render;
pub mod session;
pub mod start;
pub mod todo;
mod usage;
pub mod workspace;
pub mod workspaces;

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

/// The pages this process serves, as `(method, path)`: each moved page has
/// its own path under `/web/`, so every proxy and Ingress routes by path
/// alone (ADR-0070 d4, d7 (b) as amended). `deploy/caddy/Caddyfile`, both compose
/// files and the chart's `web.ingressPaths` route the same set
/// (`tests/routes.rs`).
pub const PAGES: &[(&str, &str)] = &[
    ("GET", GUARDRAILS_PATH),
    ("GET", workspace::WORKSPACE_PATH),
    ("POST", workspace::WORKSPACE_PATH),
    ("GET", workspace::NEW_PATH),
    ("POST", workspace::NEW_PATH),
    ("GET", workspace::EDIT_PATH),
    ("POST", workspace::EDIT_PATH),
    ("GET", workspace::RETIRE_PATH),
    ("POST", workspace::RETIRE_PATH),
    ("GET", workspace::ADMIN_WORKSPACE_PATH),
    ("POST", fix::FIXES_PATH),
    ("GET", USER_WORKSPACES_PATH),
    ("POST", USER_WORKSPACES_PATH),
    ("GET", imports::IMPORT_PATH),
    ("GET", ADMIN_WORKSPACES_PATH),
    ("POST", ADMIN_WORKSPACES_PATH),
    ("GET", USER_MEMORIES_PATH),
    ("GET", USER_CATALOG_PATH),
    ("GET", USER_ATLAS_PATH),
    ("GET", ADMIN_MEMORIES_PATH),
    ("GET", ADMIN_ATLAS_PATH),
    ("GET", memories::USER_DATA_PATH),
    ("GET", memories::ADMIN_DATA_PATH),
    ("GET", USER_REVIEW_PATH),
    ("GET", ADMIN_REVIEW_PATH),
    ("GET", TODO_PATH),
    ("POST", TODO_PATH),
    ("GET", TODO_JUDGE_PATH),
    ("POST", TODO_JUDGE_PATH),
    ("GET", todo::TODO_FIX_PATH),
    ("POST", todo::TODO_FIX_PATH),
    ("GET", ADMIN_TODO_PATH),
    ("GET", USER_KEYS_PATH),
    ("POST", USER_KEYS_PATH),
    ("GET", keys::NEW_KEY_PATH),
    ("GET", ADMIN_KEYS_PATH),
    ("POST", ADMIN_KEYS_PATH),
    ("GET", USER_MEMORY_PATH),
    ("POST", USER_MEMORY_PATH),
    ("GET", ADMIN_MEMORY_PATH),
    ("POST", ADMIN_MEMORY_PATH),
    ("GET", ADMIN_CURATE_PATH),
    ("POST", ADMIN_CURATE_PATH),
    ("POST", investigation::INVESTIGATION_PATH),
    ("GET", start::START_PATH),
    ("POST", start::START_PATH),
    ("GET", start::STATUS_PATH),
    ("GET", start::EXPORT_PATH),
    ("GET", start::SETUP_PATH),
    ("GET", admin::PEOPLE_PATH),
    ("POST", admin::PEOPLE_PATH),
    ("GET", admin::USERS_PATH),
    ("POST", admin::USERS_PATH),
];

/// The path prefixes this process serves for `GET` alone, each a fixed
/// path below which one segment names a token or a file: the Mermaid frame's
/// document and its pinned bundle (ADR-0056 d9). Every proxy and Ingress
/// routes each as a prefix, by path alone.
pub const PREFIXES: &[&str] = &[render::RENDER_PREFIX, render::ASSETS_PREFIX];

/// One memory opened by its id, on the user page and on the admin page, and
/// the admin page's Curate view (ADR-0045 d10, d12, d21), moved from
/// `GET /web?view=memories&memory=`, `GET /web/admin?view=memories&memory=`
/// and `GET /web/admin?view=curate` with the admin page's `supersede` and
/// `curate` acts.
pub use oml_page::web::{ADMIN_CURATE_PATH, ADMIN_MEMORY_PATH, USER_MEMORY_PATH};

/// The Memories list and the Atlas of each page (ADR-0045 d22-d32), moved
/// from `GET /web?view=memories` and `GET /web/admin?view=memories`, the
/// Atlas from their `display=atlas`.
pub use oml_page::web::{
    ADMIN_ATLAS_PATH, ADMIN_MEMORIES_PATH, USER_ATLAS_PATH, USER_MEMORIES_PATH,
};

/// The Review views' old paths: a `GET` answers `308` to To do, keeping
/// the workspace, and on the admin side to Memories, Held, for the release
/// that builds To do (ADR-0083 d6). A `POST` is no longer routed here, so
/// the lead answers it not found: the forms that posted it are drawn
/// nowhere, and a `308` would carry a verdict to a page without that act.
pub use oml_page::web::{ADMIN_REVIEW_PATH, USER_REVIEW_PATH};

/// The Personal side's Catalog page (ADR-0092 d7).
pub use oml_page::web::USER_CATALOG_PATH;

/// The To do page of each side and the Judge page (ADR-0083 d2, d6, d11).
pub use oml_page::web::{ADMIN_TODO_PATH, TODO_JUDGE_PATH, TODO_PATH};

/// The user page's Keys view and its acts (ADR-0045 d9), moved from
/// `GET /web` and the key acts of `POST /web`.
pub use oml_page::web::USER_KEYS_PATH;

/// The admin page's Keys view, one person's keys and the cut-off, moved
/// from the admin page's `cut_off` act (ADR-0045 d13).
pub use oml_page::web::ADMIN_KEYS_PATH;

/// The user page's Workspaces view and its acts (ADR-0045 d28), moved from
/// `GET /web?view=workspace` and the user page's workspace actions.
pub use oml_page::web::USER_WORKSPACES_PATH;

/// The admin page's Workspaces view and its acts, moved from
/// `GET /web/admin?view=workspace` and the admin page's workspace actions.
pub use oml_page::web::ADMIN_WORKSPACES_PATH;

/// The Guardrails view's former path: a `GET` answers `308` to the
/// workspace page's Guardrails kind, whose In force here panel replaced it
/// (ADR-0083 d12).
pub const GUARDRAILS_PATH: &str = "/web/guardrails";

/// Whether a request with `method` and `path` is one of [`PAGES`], or the
/// health probe. The query plays no part.
#[must_use]
pub fn serves(method: &str, path: &str) -> bool {
    if path == HEALTH_PATH {
        return true;
    }
    // READY_PATH is the probe of oml-web's own listener; the public
    // /readyz is the lead's, so the proxy never sends it here.
    PAGES.iter().any(|(m, p)| *m == method && *p == path)
        || (method == "GET"
            && PREFIXES.iter().any(|prefix| {
                path.strip_prefix(prefix)
                    .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
            }))
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
            // A lead that names no identity proves none: fail closed.
            let identity = ready
                .web_client
                .unwrap_or_else(|| "unknown: the API names no web client identity".to_owned());
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
    let ok = api == "ok" && !web_client.starts_with("none") && !web_client.starts_with("unknown");
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

/// The fields of `uri`'s query that `names` names, each `name=value`
/// encoded; the other fields are dropped.
pub(crate) fn kept_query(uri: &http::Uri, names: &[&str]) -> Vec<String> {
    use axum::extract::Query;
    // A query that does not decode keeps nothing: the redirect still lands.
    let pairs = Query::<Vec<(String, String)>>::try_from_uri(uri)
        .map(|Query(pairs)| pairs)
        .unwrap_or_default();
    pairs
        .into_iter()
        .filter(|(k, _)| names.contains(&k.as_str()))
        .map(|(k, v)| format!("{k}={}", oml_sdk::encode(&v)))
        .collect()
}

/// A `308` to `location`.
pub(crate) fn permanent(location: &str) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    (
        StatusCode::PERMANENT_REDIRECT,
        [(http::header::LOCATION, location.to_owned())],
    )
        .into_response()
}

/// `GET /web/review`: `308` to To do, keeping the workspace and the linked
/// key the page judged as (ADR-0083 d6; ADR-0085 d16).
async fn moved_review(uri: http::Uri) -> axum::response::Response {
    let kept = kept_query(&uri, &["workspace", "as"]);
    if kept.is_empty() {
        permanent(TODO_PATH)
    } else {
        permanent(&format!("{TODO_PATH}?{}", kept.join("&")))
    }
}

/// `GET /web/admin/review`: `308` to the admin Memories page's Held view,
/// keeping the workspace (ADR-0083 d6).
async fn moved_admin_review(uri: http::Uri) -> axum::response::Response {
    let mut kept = vec!["view=held".to_owned()];
    kept.extend(kept_query(&uri, &["workspace"]));
    permanent(&format!("{ADMIN_MEMORIES_PATH}?{}", kept.join("&")))
}

/// Every path no page serves; it passes the same layers as a page.
async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// The router with every layer, outermost first: ids and the line, the
/// person-only gate, the same-origin and CSRF check, the headers; then on the
/// page routes the session load.
pub fn router(web: Web) -> Router {
    let pages = Router::new()
        .route(
            workspace::WORKSPACE_PATH,
            get(workspace::user_page).post(brief::act),
        )
        .route(
            workspace::NEW_PATH,
            get(workspace::new_page).post(workspace::new_act),
        )
        .route(
            workspace::EDIT_PATH,
            get(workspace::edit_page).post(workspace::edit_act),
        )
        .route(
            workspace::RETIRE_PATH,
            get(workspace::retire_page).post(workspace::retire_act),
        )
        .route(workspace::ADMIN_WORKSPACE_PATH, get(workspace::admin_page))
        .route(fix::FIXES_PATH, axum::routing::post(fix::act))
        .route(
            USER_WORKSPACES_PATH,
            get(workspaces::user_page).post(workspaces::user_act),
        )
        .route(
            ADMIN_WORKSPACES_PATH,
            get(workspaces::admin_page).post(workspaces::admin_act),
        )
        .route(USER_MEMORIES_PATH, get(memories::user_list))
        .route(USER_CATALOG_PATH, get(catalog::page))
        .route(imports::IMPORT_PATH, get(imports::page))
        .route(ADMIN_MEMORIES_PATH, get(memories::admin_list))
        .route(USER_ATLAS_PATH, get(memories::user_atlas))
        .route(ADMIN_ATLAS_PATH, get(memories::admin_atlas))
        .route(TODO_PATH, get(todo::user_page).post(todo::user_act))
        .route(TODO_JUDGE_PATH, get(todo::judge_page).post(todo::judge_act))
        .route(todo::TODO_FIX_PATH, get(todo::fix_page).post(todo::fix_act))
        .route(ADMIN_TODO_PATH, get(todo::admin_page))
        .route(USER_KEYS_PATH, get(keys::user_page).post(keys::user_act))
        .route(keys::NEW_KEY_PATH, get(builder::page))
        .route(ADMIN_KEYS_PATH, get(keys::admin_page).post(keys::admin_act))
        .route(
            USER_MEMORY_PATH,
            get(memory::user_page).post(memory::user_act),
        )
        .route(
            ADMIN_MEMORY_PATH,
            get(memory::admin_page).post(memory::admin_act),
        )
        .route(
            investigation::INVESTIGATION_PATH,
            axum::routing::post(investigation::act),
        )
        .route(
            ADMIN_CURATE_PATH,
            get(memory::curate_page).post(memory::curate_act),
        )
        .route(start::START_PATH, get(start::page).post(start::act))
        .route(start::SETUP_PATH, get(start::setup))
        .route(
            admin::PEOPLE_PATH,
            get(admin::people_page).post(admin::people_act),
        )
        .route(
            admin::USERS_PATH,
            get(admin::users_page).post(admin::users_act),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            web.clone(),
            session::load,
        ))
        // The form extractor reads what the same-origin layer admitted,
        // whose cap is per path (`middleware::form_max_bytes`).
        .layer(axum::extract::DefaultBodyLimit::max(
            middleware::BUILDER_FORM_MAX_BYTES,
        ))
        .with_state(web.clone());
    // The old Review paths answer their 308 before any session is read: a
    // redirect needs none (ADR-0083 d6).
    let moved = Router::new()
        .route(GUARDRAILS_PATH, get(workspace::moved_guardrails))
        // A post to an old Review path is not found, never a 405: the
        // proxies route every method of these paths here, and a 308 would
        // carry the verdict form to a page with no such act (ADR-0083 d16
        // T3; the review of PR #494, 12).
        .route(USER_REVIEW_PATH, get(moved_review).post(not_found))
        .route(ADMIN_REVIEW_PATH, get(moved_admin_review).post(not_found));
    // The JSON routes load their own session: no session is not found as
    // data, never a redirect to sign in (ADR-0045 d25).
    let data = Router::new()
        .route(memories::USER_DATA_PATH, get(memories::user_data))
        .route(memories::ADMIN_DATA_PATH, get(memories::admin_data))
        .route(start::STATUS_PATH, get(start::status))
        .route(start::EXPORT_PATH, get(start::export))
        .with_state(web.clone());
    // The frame resolves its own session: a frame that does not open is not
    // found, never a redirect inside the frame (ADR-0056 d9).
    let frames = Router::new()
        .route("/web/render/{token}", get(render::frame))
        .route("/web/assets/{file}", get(render::asset))
        .with_state(web.clone());
    Router::new()
        .route(HEALTH_PATH, get(health))
        .route(READY_PATH, get(ready).with_state(web.clone()))
        .merge(pages)
        .merge(moved)
        .merge(data)
        .merge(frames)
        .fallback(not_found)
        .layer(
            ServiceBuilder::new()
                .layer(axum::middleware::from_fn_with_state(
                    web.layers.clone(),
                    middleware::ids_and_log,
                ))
                .layer(axum::middleware::from_fn(middleware::person_only))
                .layer(axum::middleware::from_fn_with_state(
                    web,
                    middleware::same_origin,
                ))
                .layer(axum::middleware::from_fn(middleware::headers)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proxy_sends_only_the_moved_pages_paths_here() {
        // Route-by-path (ADR-0070 d7 (b) as amended): the query plays no part.
        // The Guardrails view's path answers its 308 (ADR-0083 d12); the
        // workspace page and its forms are this process's (ADR-0082 d1).
        assert!(serves("GET", "/web/guardrails"));
        // The workspace page posts its status and brief acts to itself
        // (ADR-0096 P7, P9).
        assert!(serves("GET", "/web/workspace") && serves("POST", "/web/workspace"));
        assert!(serves("GET", "/web/todo/fix") && serves("POST", "/web/todo/fix"));
        for path in [
            "/web/workspace/new",
            "/web/workspace/edit",
            "/web/workspace/retire",
        ] {
            assert!(serves("GET", path) && serves("POST", path), "{path}");
        }
        assert!(serves("GET", "/web/admin/workspace") && !serves("POST", "/web/admin/workspace"));
        assert!(serves("GET", "/web/workspaces"));
        assert!(serves("POST", "/web/workspaces"));
        assert!(serves("GET", "/web/admin/workspaces"));
        assert!(serves("POST", "/web/admin/workspaces"));
        for path in [
            "/web/todo",
            "/web/todo/judge",
            "/web/keys",
            "/web/admin/keys",
        ] {
            assert!(serves("GET", path) && serves("POST", path), "{path}");
        }
        // The old Review paths answer their 308 to a GET for one release;
        // a POST goes to the lead, which answers not found (ADR-0083 d6).
        for path in ["/web/review", "/web/admin/review", "/web/admin/todo"] {
            assert!(serves("GET", path) && !serves("POST", path), "{path}");
        }
        for path in [
            "/web/memories",
            "/web/atlas",
            "/web/admin/memories",
            "/web/admin/atlas",
            "/web/data/memories",
            "/web/admin/data/memories",
        ] {
            assert!(serves("GET", path), "{path}");
            assert!(!serves("POST", path), "{path}");
        }
        // The Catalog page reads only (ADR-0092 d7).
        assert!(serves("GET", "/web/catalog") && !serves("POST", "/web/catalog"));
        for path in ["/web/memory", "/web/admin/memory", "/web/admin/curate"] {
            assert!(serves("GET", path), "{path}");
        }
        assert!(serves("POST", "/web/admin/memory") && serves("POST", "/web/admin/curate"));
        // The user memory page's one act, Withdraw (ADR-0083 d13).
        assert!(serves("POST", "/web/memory"));
        assert!(serves("GET", "/web/render/0a1b") && serves("GET", "/web/assets/mermaid-0.js"));
        assert!(!serves("GET", "/web/render/") && !serves("GET", "/web/render/a/b"));
        assert!(!serves("POST", "/web/render/0a1b"));
        assert!(serves("GET", "/web/healthz"));
        assert!(!serves("GET", "/web"));
        assert!(!serves("POST", "/web"));
        assert!(!serves("POST", "/web/guardrails"));
        assert!(!serves("GET", "/web/guardrails/x"));
        assert!(!serves("GET", "/web/admin"));
        assert!(PAGES.iter().all(|(_, path)| path.starts_with("/web/")));
    }
}
