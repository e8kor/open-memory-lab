//! The traffic layer: one tower layer outside every route of
//! [`crate::ApiState::into_router`], the probes, `/mcp` and the OAuth routes
//! included, and the routes no handler finishes (ADR-0042 d1, d4).

pub mod ids;

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

pub use ids::{CORRELATION_ID, REQUEST_ID, RequestIds, Rid};

/// Resolves the request's ids once, keeps them in its extensions for the
/// handler, and returns both on the response, axum's own `404` and `405`
/// included (ADR-0042 d4, d6).
pub(crate) async fn layer(mut req: Request, next: Next) -> Response {
    let ids = RequestIds::of(req.headers());
    req.extensions_mut().insert(ids.clone());
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    for (name, value) in [
        (REQUEST_ID, &ids.request_id),
        (CORRELATION_ID, &ids.correlation_id),
    ] {
        // An id is ASCII under the rule or minted, so it is a header value.
        if let Ok(value) = HeaderValue::from_str(value) {
            headers.insert(name, value);
        }
    }
    response
}
