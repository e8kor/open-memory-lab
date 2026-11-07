//! The web console as a client: presentation and translation of the public
//! HTTP API's responses. It does not reach the application core; the
//! dependency test in `tests/dependencies.rs` enforces the wall.
//!
//! Phase (b) of ADR-0070 d7: this crate exists before any page moves, so
//! the wall can be built and the SDK/portfolio code can be added incrementally.
//! Rendering code is added page by page in later commits.

/// A page that can render an outcome, in the end: `Authorization` is checked
/// at the first point a session is loaded (d3 layer 2).
pub const WEB_ROUTES: &[(&str, &str)] = &[
    ("GET", "/web"),
    ("POST", "/web"),
    ("GET", "/web/admin"),
    ("POST", "web/admin"),
];

/// Capacities that the browser proves through the credential derived from
/// the session token (ADR-0070 d2).
pub enum Capacity {
    Web,
    Review,
    Curate,
    Admin,
}

/// The rendering modes required of the lead's container.
pub enum WebApiClientMode {
    /// A clone of the same router the lead serves.
    InProcessLoopback,
    /// The public HTTP API at `web.api_url`.
    StandaloneHttp,
}

/// Split into two separate real types (local/in_process vs HTTP) once the
/// build reaches phase (a). This placeholder keeps the boot shape explicit.
pub struct WebBootstrap {
    pub mode: WebApiClientMode,
}
