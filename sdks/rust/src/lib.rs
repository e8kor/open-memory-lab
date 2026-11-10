//! `oml-sdk`: a hand-written, typed client of OML's public HTTP API
//! (ADR-0070 d9). It covers the operations its consumers call, one method
//! each, and nothing else; [`OPERATIONS`] names them, and
//! `scripts/check_contract_sync.py` fails when an entry's operation id,
//! method, path or field names differ from `docs/api/openapi.yaml`, so a
//! contract change that forgets this client fails where the generated
//! Python and TypeScript layers already do. Its one consumer today is
//! `oml-web`.
//!
//! Every call names its credential and the ids it is made under
//! ([`Call`]); a refusal is the API's envelope as [`ApiError`]; a field the
//! API adds is ignored, so a client one release behind still reads the
//! answer (ADR-0070 d4).

#![forbid(unsafe_code)]

pub mod transport;

use http::header::{ACCEPT, AUTHORIZATION, RETRY_AFTER};
use http::{HeaderMap, HeaderName, HeaderValue, Method, Request};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub use transport::{HttpTransport, Sending, Transport, TransportError};

/// The operations this client calls, one entry each:
/// `<operationId> <METHOD> <path>`, then `query:` the query parameters it
/// may send and `body:` the request fields it may send, each
/// comma-separated, and `resp:` the struct its answer decodes into. The
/// contract check reads this table and holds every field of that struct,
/// and of each struct it holds, to the operation's 2xx schema;
/// `every_method_sends_the_method_path_and_type_its_table_entry_names`
/// holds the methods to it.
pub const OPERATIONS: [&str; 5] = [
    "authorizationServerMetadata GET /.well-known/oauth-authorization-server resp:AuthorizationServerMetadata",
    "whoami GET /v2/whoami resp:Whoami",
    "getGuardrails GET /v1/guardrails query:workarea resp:GuardrailSet",
    "getWorkareaTree GET /v2/workareas/{workarea}/tree query:limit,cursor resp:WorkareaPage",
    "readyz GET /readyz resp:Ready",
];

/// The request id header (ADR-0042 d4).
pub const REQUEST_ID: &str = "x-request-id";
/// The correlation id header (ADR-0042 d4).
pub const CORRELATION_ID: &str = "x-correlation-id";

/// One call's credential and ids: the bearer value it presents, and the
/// request and correlation ids the API's line will carry.
#[derive(Clone, Copy)]
pub struct Call<'a> {
    pub bearer: &'a str,
    pub request_id: Option<&'a str>,
    pub correlation_id: Option<&'a str>,
}

/// No debug output prints the bearer value.
impl std::fmt::Debug for Call<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Call")
            .field("bearer", &"<redacted>")
            .field("request_id", &self.request_id)
            .field("correlation_id", &self.correlation_id)
            .finish()
    }
}

/// The API's refusal envelope (docs/api/errors.md), with the status and the
/// `Retry-After` it came with.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub details: Value,
    pub request_id: Option<String>,
    pub retry_after: Option<String>,
}

/// Why a call has no answer this client can use.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The API refused, in its envelope; boxed, so a `Result` stays small.
    Api(Box<ApiError>),
    /// The call never had an answer: timeout or no connection.
    Transport(TransportError),
    /// An answer that is not the operation's shape: a status without an
    /// envelope, or a body that does not decode.
    Decode { status: u16, why: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Api(e) => write!(f, "{}: {}", e.code, e.message),
            Self::Transport(e) => e.fmt(f),
            Self::Decode { status, why } => {
                write!(
                    f,
                    "the API answered {status} in a shape this client does not read: {why}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

/// `GET /.well-known/oauth-authorization-server`, the field read here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
}

/// A named reference, `{id, name}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NamedRef {
    pub id: String,
    pub name: String,
}

/// `GET /readyz`: whether the API can serve, and the web client identity it
/// accepts, the fields read here (ADR-0070 d2, d4).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ready {
    /// `ok`, or `unavailable` (with a `503`).
    pub status: String,
    /// `none` when the API accepts no capacity credential from `oml-web`;
    /// absent from an API before ADR-0070.
    pub web_client: Option<String>,
}

/// `GET /v2/whoami`: the calling key's own record, the fields read here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Whoami {
    pub key_id: String,
    pub name: Option<String>,
    pub role: String,
    pub tenant: Option<NamedRef>,
    /// The key's user root; `None` for a tenant key.
    pub user: Option<NamedRef>,
}

/// One verdict on a guardrail.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GuardrailVerdict {
    pub held_by: String,
    pub at: String,
}

/// One guardrail in force (openapi.yaml `ServedGuardrail`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServedGuardrail {
    pub claim_key: String,
    pub version: i64,
    /// `None` where the node's path is not served.
    pub workarea_path: Option<String>,
    pub severity: String,
    pub text: String,
    #[serde(default)]
    pub rule: Option<Value>,
    pub promoted_at: String,
    #[serde(default)]
    pub verdicts: Vec<GuardrailVerdict>,
    #[serde(default)]
    pub conflict: Option<Vec<String>>,
}

/// One of the caller's own held guardrails, by key and version.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OwnHeld {
    pub claim_key: String,
    pub version: i64,
}

/// `GET /v1/guardrails` (openapi.yaml `GuardrailSet`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GuardrailSet {
    pub scope_node: String,
    pub scope_path: String,
    pub guardrail_scope: String,
    pub over_cap: bool,
    pub guardrails: Vec<ServedGuardrail>,
    #[serde(default)]
    pub own_held: Vec<OwnHeld>,
}

/// One node of a workarea tree page.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkareaNode {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// `GET /v2/workareas/{workarea}/tree`: one page of nodes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkareaPage {
    pub items: Vec<WorkareaNode>,
    pub next_cursor: Option<String>,
}

/// The typed client over a [`Transport`], with the headers every call
/// carries (for `oml-web`, its client identity).
pub struct Client<T> {
    transport: T,
    fixed: HeaderMap,
}

/// `value` percent-encoded for a query string or a path segment: every
/// byte but the unreserved ones (RFC 3986 §2.3).
#[must_use]
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The envelope of a refusal body, `{"error": {…}, "request_id"}`.
fn api_error(status: u16, headers: &HeaderMap, body: &[u8]) -> Error {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Error::Decode {
            status,
            why: "a refusal without the error envelope".to_owned(),
        };
    };
    let Some(code) = value["error"]["code"].as_str() else {
        return Error::Decode {
            status,
            why: "a refusal without the error envelope".to_owned(),
        };
    };
    Error::Api(Box::new(ApiError {
        status,
        code: code.to_owned(),
        message: value["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        details: value["error"]["details"].clone(),
        request_id: value["request_id"].as_str().map(str::to_owned),
        retry_after: headers
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    }))
}

impl<T: Transport> Client<T> {
    /// A client over `transport`.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            fixed: HeaderMap::new(),
        }
    }

    /// The client with `name: value` on every call, marked sensitive so no
    /// debug output prints it.
    #[must_use]
    pub fn with_header(mut self, name: HeaderName, mut value: HeaderValue) -> Self {
        value.set_sensitive(true);
        self.fixed.insert(name, value);
        self
    }

    async fn get<R: DeserializeOwned>(
        &self,
        call: Option<&Call<'_>>,
        path: String,
    ) -> Result<R, Error> {
        let mut request = Request::builder()
            .method(Method::GET)
            .uri(path)
            .body(Vec::new())
            .map_err(|e| Error::Decode {
                status: 0,
                why: format!("the request could not be built: {e}"),
            })?;
        let headers = request.headers_mut();
        headers.extend(self.fixed.clone());
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Some(call) = call {
            let mut bearer =
                HeaderValue::from_str(&format!("Bearer {}", call.bearer)).map_err(|_| {
                    Error::Decode {
                        status: 0,
                        why: "the credential is not a header value".to_owned(),
                    }
                })?;
            bearer.set_sensitive(true);
            headers.insert(AUTHORIZATION, bearer);
        }
        for (name, value) in [
            (REQUEST_ID, call.and_then(|c| c.request_id)),
            (CORRELATION_ID, call.and_then(|c| c.correlation_id)),
        ] {
            if let Some(value) = value.and_then(|v| HeaderValue::from_str(v).ok()) {
                headers.insert(name, value);
            }
        }
        let response = self
            .transport
            .send(request)
            .await
            .map_err(Error::Transport)?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(api_error(status, response.headers(), response.body()));
        }
        serde_json::from_slice(response.body()).map_err(|e| Error::Decode {
            status,
            why: e.to_string(),
        })
    }

    /// `GET /.well-known/oauth-authorization-server`. No credential.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn authorization_server_metadata(
        &self,
    ) -> Result<AuthorizationServerMetadata, Error> {
        self.get(None, "/.well-known/oauth-authorization-server".to_owned())
            .await
    }

    /// The server's public origin, the metadata's `issuer`, which a client
    /// takes as its own default (ADR-0070 d4).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn issuer(&self) -> Result<String, Error> {
        Ok(self.authorization_server_metadata().await?.issuer)
    }

    /// `GET /readyz`. No credential: the probe answers anyone.
    ///
    /// # Errors
    /// The API's refusal (a `503` while it cannot serve), or no usable
    /// answer.
    pub async fn readyz(&self) -> Result<Ready, Error> {
        self.get(None, "/readyz".to_owned()).await
    }

    /// `GET /v2/whoami`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn whoami(&self, call: &Call<'_>) -> Result<Whoami, Error> {
        self.get(Some(call), "/v2/whoami".to_owned()).await
    }

    /// `GET /v1/guardrails`, at `workarea` (an id or a name) or at the key's
    /// default workarea.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn guardrails(
        &self,
        call: &Call<'_>,
        workarea: Option<&str>,
    ) -> Result<GuardrailSet, Error> {
        let path = match workarea {
            Some(w) => format!("/v1/guardrails?workarea={}", encode(w)),
            None => "/v1/guardrails".to_owned(),
        };
        self.get(Some(call), path).await
    }

    /// `GET /v2/workareas/{workarea}/tree`: one page of `limit` nodes
    /// after `cursor`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn workarea_tree(
        &self,
        call: &Call<'_>,
        workarea: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<WorkareaPage, Error> {
        let mut path = format!("/v2/workareas/{}/tree?limit={limit}", encode(workarea));
        if let Some(cursor) = cursor {
            path.push_str("&cursor=");
            path.push_str(&encode(cursor));
        }
        self.get(Some(call), path).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use http::Response;

    use super::*;

    type Seen = Arc<Mutex<Vec<Request<Vec<u8>>>>>;

    /// Records each request and answers `body` with `status`.
    #[derive(Clone)]
    struct Recording {
        seen: Seen,
        status: u16,
        body: &'static str,
    }

    impl Transport for Recording {
        fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
            self.seen.lock().unwrap().push(request);
            let response = Response::builder()
                .status(self.status)
                .header("retry-after", "7")
                .body(self.body.as_bytes().to_vec())
                .unwrap();
            Box::pin(async move { Ok(response) })
        }
    }

    fn recording(status: u16, body: &'static str) -> (Client<Recording>, Seen) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = Client::new(Recording {
            seen: seen.clone(),
            status,
            body,
        });
        (client, seen)
    }

    const CALL: Call<'static> = Call {
        bearer: "omls_x",
        request_id: Some("page-1.1"),
        correlation_id: Some("page-1"),
    };

    /// The method and path template of each table entry, by operation id.
    /// Each entry's operation id, method, path template and `resp:` type.
    fn table() -> Vec<[&'static str; 4]> {
        OPERATIONS
            .iter()
            .map(|entry| {
                let mut words = entry.split(' ');
                let (id, method, path) = (
                    words.next().unwrap(),
                    words.next().unwrap(),
                    words.next().unwrap(),
                );
                let resp = words
                    .find_map(|w| w.strip_prefix("resp:"))
                    .expect("every entry names its resp: type");
                [id, method, path, resp]
            })
            .collect()
    }

    /// The name of the type an answer decodes into.
    fn decodes<T>(_: &Result<T, Error>) -> &'static str {
        std::any::type_name::<T>().rsplit("::").next().unwrap()
    }

    fn matches_template(path: &str, template: &str) -> bool {
        let path = path.split('?').next().unwrap();
        let (a, b): (Vec<&str>, Vec<&str>) =
            (path.split('/').collect(), template.split('/').collect());
        a.len() == b.len()
            && a.iter()
                .zip(&b)
                .all(|(p, t)| (t.starts_with('{') && t.ends_with('}')) || p == t)
    }

    #[tokio::test]
    async fn every_method_sends_the_method_path_and_type_its_table_entry_names() {
        let (client, seen) = recording(500, "{}");
        let types = [
            decodes(&client.authorization_server_metadata().await),
            decodes(&client.whoami(&CALL).await),
            decodes(&client.guardrails(&CALL, Some("alice.proj")).await),
            decodes(&client.workarea_tree(&CALL, "w/1", 200, Some("c 1")).await),
            decodes(&client.readyz().await),
        ];
        let seen = seen.lock().unwrap();
        let table = table();
        assert_eq!(seen.len(), table.len(), "one call per table entry");
        for ((request, [id, method, template, resp]), ty) in seen.iter().zip(&table).zip(types) {
            assert_eq!(
                ty, *resp,
                "{id}: the method decodes into the type its entry names"
            );
            assert_eq!(request.method().as_str(), *method, "{id}");
            let path = request.uri().to_string();
            assert!(
                matches_template(&path, template),
                "{id}: {path} vs {template}"
            );
        }
        assert!(!seen[0].headers().contains_key(AUTHORIZATION));
        assert!(!seen[4].headers().contains_key(AUTHORIZATION));
        assert_eq!(seen[2].uri(), "/v1/guardrails?workarea=alice.proj");
        assert_eq!(
            seen[3].uri(),
            "/v2/workareas/w%2F1/tree?limit=200&cursor=c%201"
        );
    }

    #[tokio::test]
    async fn a_call_carries_its_bearer_its_ids_and_the_fixed_headers() {
        let (client, seen) = recording(
            200,
            r#"{"key_id":"k","name":"alice-web","role":"user","tenant":null,"user":null,"extra":1}"#,
        );
        let client = client.with_header(
            HeaderName::from_static("oml-web-client"),
            HeaderValue::from_static("s3cret"),
        );
        let whoami = client.whoami(&CALL).await.unwrap();
        assert_eq!(whoami.name.as_deref(), Some("alice-web"));
        let seen = seen.lock().unwrap();
        let h = seen[0].headers();
        assert_eq!(h[AUTHORIZATION], "Bearer omls_x");
        assert!(h[AUTHORIZATION].is_sensitive());
        assert_eq!(h[REQUEST_ID], "page-1.1");
        assert_eq!(h[CORRELATION_ID], "page-1");
        assert_eq!(h["oml-web-client"], "s3cret");
        assert!(h["oml-web-client"].is_sensitive());
    }

    #[test]
    fn a_calls_debug_output_never_prints_its_bearer() {
        let shown = format!("{CALL:?}");
        assert!(!shown.contains("omls_x"), "{shown}");
        assert!(shown.contains("page-1.1"), "{shown}");
    }

    #[tokio::test]
    async fn a_refusal_is_the_apis_envelope_with_its_retry_after() {
        let (client, _) = recording(
            429,
            r#"{"error":{"code":"limit.rate","message":"slow down","details":{"limit":"rpm"}},"request_id":"req_9"}"#,
        );
        let Err(Error::Api(e)) = client.whoami(&CALL).await else {
            panic!("an API refusal");
        };
        assert_eq!(e.status, 429);
        assert_eq!(e.code, "limit.rate");
        assert_eq!(e.details["limit"], "rpm");
        assert_eq!(e.request_id.as_deref(), Some("req_9"));
        assert_eq!(e.retry_after.as_deref(), Some("7"));
        let (client, _) = recording(502, "<html>bad gateway</html>");
        assert!(matches!(
            client.whoami(&CALL).await,
            Err(Error::Decode { status: 502, .. })
        ));
    }
}
