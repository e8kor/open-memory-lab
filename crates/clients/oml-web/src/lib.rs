//! The web console as its own process: pages over the public HTTP API, and
//! nothing of the application core (ADR-0070 d1; d4 as amended on
//! 2026-11-10, `oml-web` runs only as its own process, with `web.api_url`
//! required). `tests/dependencies.rs` holds the wall.
//!
//! This is the scaffold of phase (b): the configuration, the health probe and
//! the middleware stack. No page has moved yet; every page route still lives
//! in the lead until it moves here behind its parity test.

pub mod config;
pub mod middleware;
pub mod outcome;

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

/// The capacities a session's credential is derived for (ADR-0070 d2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capacity {
    Web,
    Review,
    Curate,
    Admin,
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "status": "ok",
        "service": "oml-web",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Every path no page serves yet; it passes the same layers as a page.
async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// The router with every layer of the scaffold, outermost first: ids and
/// the line, the human-only gate, the same-origin check, the headers.
pub fn router(layers: Layers) -> Router {
    Router::new()
        .route(HEALTH_PATH, get(health))
        .fallback(not_found)
        .layer(
            ServiceBuilder::new()
                .layer(axum::middleware::from_fn_with_state(
                    layers.clone(),
                    middleware::ids_and_log,
                ))
                .layer(axum::middleware::from_fn(middleware::human_only))
                .layer(axum::middleware::from_fn_with_state(
                    layers,
                    middleware::same_origin,
                ))
                .layer(axum::middleware::from_fn(middleware::headers)),
        )
}
