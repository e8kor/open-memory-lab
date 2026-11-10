//! The layers of ADR-0070 d3 that this scaffold builds, outermost first.
//! Layer 3 (session load) and layer 6 (the fan-out cap) need API calls and
//! land in phase (b) with the first page; layer 7 is [`crate::outcome`].

use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use serde_json::json;

use crate::HEALTH_PATH;

/// The request id header, honoured or minted, and returned.
pub const REQUEST_ID: &str = "x-request-id";
/// The correlation id header, honoured or defaulted to the request id.
pub const CORRELATION_ID: &str = "x-correlation-id";
/// The `details.reason` of an `Authorization` header on a page route,
/// the lead's own reason for the same refusal (ADR-0045 d7).
pub const HUMAN_ONLY: &str = "web_is_human_only";

/// The pages' policy: nothing loads, forms post only to the page's origin,
/// no frame holds a page (ADR-0070 d3 layer 5). The stylesheet's digest
/// joins `style-src` when the stylesheet moves to `oml-page` with the first
/// page.
pub const CONTENT_SECURITY_POLICY: &str =
    "default-src 'none'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// Where the one line per request goes: standard output in the binary, a
/// buffer in the tests.
pub type LogSink = Arc<dyn Fn(String) + Send + Sync>;

/// What the layers share.
#[derive(Clone)]
pub struct Layers {
    /// The public origin the pages are served under, for the same-origin
    /// check. `None` until it is read from the API's discovery document
    /// (ADR-0070 d4), and while it is `None` every form post is refused.
    pub public_origin: Option<String>,
    pub log: LogSink,
}

/// A request's two ids (ADR-0042 d4), in the request's extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIds {
    pub request_id: String,
    pub correlation_id: String,
}

fn honoured(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
        (Some(v), None) if oml_page::request_id_admitted(v.as_bytes()) => {
            Some(String::from_utf8_lossy(v.as_bytes()).into_owned())
        }
        _ => None,
    }
}

fn mint_request_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source answers");
    let mut out = String::from("req_");
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

impl RequestIds {
    /// The ids `headers` name under the rule, minted or defaulted where they
    /// name none the rule admits.
    #[must_use]
    pub fn of(headers: &HeaderMap) -> Self {
        let request_id = honoured(headers, REQUEST_ID).unwrap_or_else(mint_request_id);
        let correlation_id =
            honoured(headers, CORRELATION_ID).unwrap_or_else(|| request_id.clone());
        Self {
            request_id,
            correlation_id,
        }
    }
}

/// A refusal in the API's envelope, so a client reads a page refusal the
/// way it reads an API one.
fn refusal(
    ids: &RequestIds,
    status: StatusCode,
    code: &str,
    rule: &str,
    message: &str,
    reason: Option<&str>,
) -> Response {
    let mut error = json!({
        "code": code,
        "message": message,
        "rule": rule,
        "retryable": false,
    });
    if let Some(reason) = reason {
        error["details"] = json!({ "reason": reason });
    }
    let body = json!({ "error": error, "request_id": ids.request_id });
    let mut response = (status, axum::Json(body)).into_response();
    security_headers(response.headers_mut());
    response
}

/// Layer 1: the ids, resolved once, and the one line per request, written
/// after every inner layer so a refusal by any of them is still logged. The
/// line names the route template, never the path, and no header value.
pub async fn ids_and_log(State(layers): State<Layers>, mut req: Request, next: Next) -> Response {
    let started = Instant::now();
    let ids = RequestIds::of(req.headers());
    let method = req.method().clone();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_owned();
    req.extensions_mut().insert(ids.clone());
    let mut response = next.run(req).await;
    for (name, value) in [
        (REQUEST_ID, &ids.request_id),
        (CORRELATION_ID, &ids.correlation_id),
    ] {
        if let Ok(value) = HeaderValue::from_str(value) {
            response.headers_mut().insert(name, value);
        }
    }
    let line = json!({
        "msg": "http_request",
        "service": "oml-web",
        "request_id": ids.request_id,
        "correlation_id": ids.correlation_id,
        "method": method.as_str(),
        "route": route,
        "status": response.status().as_u16(),
        "duration_ms": started.elapsed().as_millis(),
    });
    (layers.log)(line.to_string());
    response
}

fn ids_of(req: &Request) -> RequestIds {
    req.extensions()
        .get::<RequestIds>()
        .cloned()
        .unwrap_or_else(|| RequestIds::of(req.headers()))
}

/// Layer 2: a page answers browsers only. An `Authorization` header on any
/// route but the health probe is refused before anything else is read
/// (ADR-0045 d7).
pub async fn human_only(req: Request, next: Next) -> Response {
    if req.uri().path() != HEALTH_PATH && req.headers().contains_key(header::AUTHORIZATION) {
        return refusal(
            &ids_of(&req),
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: the pages are a person's, reached by a signed-in browser and by nothing an agent holds (ADR-0045 d7).",
            "the web pages take no Authorization header",
            Some(HUMAN_ONLY),
        );
    }
    next.run(req).await
}

/// Layer 4: every request that can change state comes from the page's own
/// origin (ADR-0045 d8). The `csrf_token` comparison against the session
/// token joins this layer in phase (b), with the session load it needs.
pub async fn same_origin(State(layers): State<Layers>, req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        let admitted = matches!(
            (origin, layers.public_origin.as_deref()),
            (Some(sent), Some(ours)) if sent == ours
        );
        if !admitted {
            return refusal(
                &ids_of(&req),
                StatusCode::FORBIDDEN,
                "oauth.origin_refused",
                "A page's form is accepted only from the page's own origin (ADR-0045 d8; ADR-0031 d9).",
                "this form was not sent from this server's own page",
                None,
            );
        }
    }
    next.run(req).await
}

/// The headers every answer carries. A refusal by an outer layer sets them
/// itself, since it never reaches layer 5.
fn security_headers(h: &mut HeaderMap) {
    for (name, value) in [
        (header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY),
        (header::CACHE_CONTROL, "no-store"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "same-origin"),
    ] {
        h.insert(name, HeaderValue::from_static(value));
    }
}

/// Layer 5: the headers on every answer that reaches it, a page or the
/// health probe.
pub async fn headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    security_headers(response.headers_mut());
    response
}

/// The body of a response, for tests and for the error mapping.
///
/// # Errors
/// When the body cannot be read.
pub async fn body_bytes(body: Body) -> Result<axum::body::Bytes, axum::Error> {
    axum::body::to_bytes(body, usize::MAX).await
}
