//! The one seam between the typed client and the wire (ADR-0070 d4, d9): a
//! [`Transport`] takes a serialized `http::Request` whose URI is the path and
//! query alone and answers an `http::Response`. [`HttpTransport`] is the
//! one implementation the binaries use; a test may supply its own.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use http::{Request, Response};

/// What a transport answers before the API does: the call never reached
/// the API, or no answer came back within the call's timeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// No answer within the per-call timeout (ADR-0070 d8).
    Timeout,
    /// The connection could not be made or broke, and why.
    Unreachable(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => f.write_str("the API did not answer within the call's timeout"),
            Self::Unreachable(why) => write!(f, "the API is unreachable: {why}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// A future a transport answers with.
pub type Sending<'a> =
    Pin<Box<dyn Future<Output = Result<Response<Vec<u8>>, TransportError>> + Send + 'a>>;

/// Sends one serialized request. The request's URI carries the path and
/// query only; where the API lives is the transport's own setting.
pub trait Transport: Send + Sync {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_>;
}

impl<T: Transport + ?Sized> Transport for std::sync::Arc<T> {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
        (**self).send(request)
    }
}

/// The pooled keep-alive HTTP client to one API base URL (ADR-0070 d8):
/// one connection pool per process, bounded idle connections per host,
/// HTTP/1.1 keep-alive and HTTP/2 where the hop is TLS, and a timeout on
/// every call.
#[derive(Clone)]
pub struct HttpTransport {
    client: reqwest::Client,
    base: String,
}

/// How many idle connections the pool keeps to the API. A constant of the
/// build, unmeasured (#170).
pub const POOL_IDLE_MAX: usize = 32;

impl HttpTransport {
    /// A transport to `base_url` (`scheme://host[:port]`, no path), each call
    /// under `timeout`.
    ///
    /// # Errors
    /// The client cannot be built.
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self, String> {
        // The provider the lead's own clients install; a second install is
        // a no-op.
        let _already = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .pool_max_idle_per_host(POOL_IDLE_MAX)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("the API client: {e}"))?;
        Ok(Self {
            client,
            base: base_url.trim_end_matches('/').to_owned(),
        })
    }
}

impl Transport for HttpTransport {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let path = parts.uri.path_and_query().map_or("/", |p| p.as_str());
            let url = format!("{}{path}", self.base);
            let mut builder = self.client.request(parts.method, url).body(body);
            for (name, value) in &parts.headers {
                builder = builder.header(name, value);
            }
            let answer = builder.send().await.map_err(|e| {
                if e.is_timeout() {
                    TransportError::Timeout
                } else {
                    TransportError::Unreachable(e.to_string())
                }
            })?;
            let mut response = Response::builder().status(answer.status());
            for (name, value) in answer.headers() {
                response = response.header(name, value);
            }
            let bytes = answer.bytes().await.map_err(|e| {
                if e.is_timeout() {
                    TransportError::Timeout
                } else {
                    TransportError::Unreachable(e.to_string())
                }
            })?;
            response
                .body(bytes.to_vec())
                .map_err(|e| TransportError::Unreachable(e.to_string()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_url_loses_a_trailing_slash() {
        let t = HttpTransport::new("http://127.0.0.1:8090/", Duration::from_secs(1)).unwrap();
        assert_eq!(t.base, "http://127.0.0.1:8090");
    }
}
