//! The request's ids: an inbound `X-Request-Id` and `X-Correlation-Id`
//! honoured when they pass one rule, minted or defaulted otherwise, and
//! never refused (ADR-0042 d4–d6; http-api.md request ids).

use std::convert::Infallible;
use std::ops::Deref;

use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::request::Parts;

use crate::new_request_id;

/// The request id header, read and returned on every response.
pub const REQUEST_ID: &str = "x-request-id";
/// The correlation id header, read and returned on every response.
pub const CORRELATION_ID: &str = "x-correlation-id";

/// Where the request id came from, the request line's `request_id_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestIdSource {
    /// The caller's, honoured under the rule.
    Caller,
    /// None was sent, so the lead minted one.
    Minted,
    /// One was sent and failed the rule; the lead minted one, and the sent
    /// value is never logged.
    Replaced,
}

impl RequestIdSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Caller => "caller",
            Self::Minted => "minted",
            Self::Replaced => "replaced",
        }
    }
}

/// Where the correlation id came from, the request line's
/// `correlation_id_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrelationIdSource {
    /// The caller's, honoured under the rule.
    Caller,
    /// None was sent, so it is the request id.
    RequestId,
    /// One was sent and failed the rule, so it is the request id.
    Replaced,
}

impl CorrelationIdSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Caller => "caller",
            Self::RequestId => "request_id",
            Self::Replaced => "replaced",
        }
    }
}

/// A request's two ids, resolved once by the traffic layer and kept in the
/// request's extensions, where every handler reads them (ADR-0042 d4).
/// Each is a label the caller may have chosen: no decision reads it (d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIds {
    pub request_id: String,
    pub request_id_source: RequestIdSource,
    pub correlation_id: String,
    pub correlation_id_source: CorrelationIdSource,
}

/// An id the rule admits: one to 128 characters from `A`–`Z`, `a`–`z`,
/// `0`–`9`, `-`, `_`, `.` and `:`, so an honoured id can neither break a
/// JSON line nor forge a second one (ADR-0042 d4).
#[must_use]
pub fn admitted(value: &[u8]) -> bool {
    (1..=128).contains(&value.len())
        && value
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

/// The header's one instance, `Err(())` for two or more, `Ok(None)` for
/// none.
fn single<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a [u8]>, ()> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
        (None, _) => Ok(None),
        (Some(value), None) => Ok(Some(value.as_bytes())),
        (Some(_), Some(_)) => Err(()),
    }
}

/// The admitted value of `name`, or why there is none: `Ok(None)` when it
/// was not sent, `Err(())` when it was sent and the rule refused it.
fn honoured(headers: &HeaderMap, name: &str) -> Result<Option<String>, ()> {
    match single(headers, name)? {
        None => Ok(None),
        Some(value) if admitted(value) => Ok(Some(String::from_utf8_lossy(value).into_owned())),
        Some(_) => Err(()),
    }
}

impl RequestIds {
    /// The ids `headers` name under the rule, minted or defaulted where
    /// they name none the rule admits (ADR-0042 d4, d6).
    #[must_use]
    pub fn of(headers: &HeaderMap) -> Self {
        let (request_id, request_id_source) = match honoured(headers, REQUEST_ID) {
            Ok(Some(id)) => (id, RequestIdSource::Caller),
            Ok(None) => (new_request_id(), RequestIdSource::Minted),
            Err(()) => (new_request_id(), RequestIdSource::Replaced),
        };
        let (correlation_id, correlation_id_source) = match honoured(headers, CORRELATION_ID) {
            Ok(Some(id)) => (id, CorrelationIdSource::Caller),
            Ok(None) => (request_id.clone(), CorrelationIdSource::RequestId),
            Err(()) => (request_id.clone(), CorrelationIdSource::Replaced),
        };
        Self {
            request_id,
            request_id_source,
            correlation_id,
            correlation_id_source,
        }
    }
}

/// A handler's ids, read from the extensions the traffic layer filled, so
/// the `X-Request-Id` header, the envelope's `request_id` and every log
/// line about the request are one value (ADR-0042 d4). It reads as the
/// request id. Outside the layer, as a handler called alone is, the ids
/// are resolved here.
#[derive(Debug, Clone)]
pub struct Rid(pub RequestIds);

impl Deref for Rid {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0.request_id
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Rid {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        let ids = match parts.extensions.get::<RequestIds>() {
            Some(ids) => ids.clone(),
            None => RequestIds::of(&parts.headers),
        };
        Ok(Self(ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn with(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(*name, HeaderValue::from_str(value).expect("a header value"));
        }
        headers
    }

    #[test]
    fn the_rule_admits_uuids_ulids_req_ids_and_proxy_hex_and_refuses_spaces_quotes_and_long_values()
    {
        for id in [
            "0b5f6c3e-5d0b-4d8f-9a5e-2f1d7c9e8a10",
            "01HZX3K9V7Q2M4N6P8R0S2T4V6",
            "req_0123456789abcdef0123456789abcdef",
            "a3f9c2e1d0b8",
            "trace:span.1",
            &"x".repeat(128),
        ] {
            assert!(admitted(id.as_bytes()), "{id} is admitted");
        }
        for id in [
            "",
            "has space",
            "quo\"te",
            "new\nline",
            "ü",
            &"x".repeat(129),
        ] {
            assert!(!admitted(id.as_bytes()), "{id:?} is refused");
        }
    }

    #[test]
    fn a_second_x_request_id_instance_is_replaced_and_a_missing_one_minted() {
        let twice = RequestIds::of(&with(&[(REQUEST_ID, "a"), (REQUEST_ID, "b")]));
        assert_eq!(twice.request_id_source, RequestIdSource::Replaced);
        assert!(twice.request_id.starts_with("req_"));
        let none = RequestIds::of(&HeaderMap::new());
        assert_eq!(none.request_id_source, RequestIdSource::Minted);
        assert_eq!(none.correlation_id, none.request_id);
        assert_eq!(none.correlation_id_source, CorrelationIdSource::RequestId);
    }

    #[test]
    fn a_correlation_id_outside_the_rule_is_the_request_id_marked_replaced() {
        let ids = RequestIds::of(&with(&[(REQUEST_ID, "job-7"), (CORRELATION_ID, "a b")]));
        assert_eq!(ids.request_id, "job-7");
        assert_eq!(ids.request_id_source, RequestIdSource::Caller);
        assert_eq!(ids.correlation_id, "job-7");
        assert_eq!(ids.correlation_id_source, CorrelationIdSource::Replaced);
    }
}
