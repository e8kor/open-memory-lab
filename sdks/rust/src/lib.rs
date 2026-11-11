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

use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER};
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
pub const OPERATIONS: [&str; 20] = [
    "authorizationServerMetadata GET /.well-known/oauth-authorization-server resp:AuthorizationServerMetadata",
    "whoami GET /v2/whoami resp:Whoami",
    "getGuardrails GET /v1/guardrails query:workarea resp:GuardrailSet",
    "getWorkareaTree GET /v2/workareas/{workarea}/tree query:limit,cursor resp:WorkareaPage",
    "readyz GET /readyz resp:Ready",
    "getWorkarea GET /v2/workareas/{workarea} resp:WorkareaView",
    "getWorkareaOffers GET /v2/workareas/{workarea}/offers resp:WorkareaOffers",
    "proposeTransition POST /v2/workareas/{workarea}/transitions body:to resp:Transition",
    "ratifyTransition POST /v2/workareas/{workarea}/transitions/{transition_id}/ratify resp:Transition",
    "setWorkareaMode POST /v2/workareas/{workarea}/mode body:mode resp:WorkareaView",
    "reviewNext POST /v2/review/next resp:Serve",
    "postVerdict POST /v2/review/verdicts body:serve_id,verdict,reason resp:VerdictResult",
    "reviewHeld GET /v2/review/held query:workarea,author_key,claim_key_prefix,limit,cursor resp:HeldPage",
    "reviewHeld GET /v2/review/held query:ids resp:HeldNamed",
    "postBulkVerdict POST /v2/review/verdicts/bulk body:verdict,artifact_ids,reason resp:BulkVerdictResult",
    "reviewQueue GET /v2/review/queue resp:ReviewQueue",
    "listKeys GET /v2/keys query:user,limit,cursor resp:KeyPage",
    "issueKey POST /v2/keys body:name,role,connect_code resp:KeyIssued",
    "mintConnectCode POST /v2/keys/{key}/connect-codes resp:ConnectCode",
    "addDenial POST /v2/keys/{key}/denials body:workarea resp:Denial",
];

/// The request id header (ADR-0042 d4).
pub const REQUEST_ID: &str = "x-request-id";
/// The correlation id header (ADR-0042 d4).
pub const CORRELATION_ID: &str = "x-correlation-id";
/// The idempotency key every state-creating `POST` carries.
pub const IDEMPOTENCY_KEY: &str = "idempotency-key";

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

/// One status transition (openapi.yaml `Transition`), the fields read here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Transition {
    pub transition_id: String,
    pub from: String,
    pub to: String,
    pub state: String,
}

/// `GET /v2/workareas/{workarea}` (openapi.yaml `WorkareaView`), the fields
/// read here: the node and its live proposals.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkareaView {
    pub id: String,
    pub name: String,
    pub path: String,
    pub status: String,
    #[serde(default)]
    pub pending_transitions: Vec<Transition>,
}

/// A transition a workarea's offers name: `propose` the next rung, or
/// `ratify` a live proposal another key made.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OfferedTransition {
    pub op: String,
    pub to: String,
    /// The live proposal a `ratify` names.
    #[serde(default)]
    pub transition_id: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
}

/// Putting the workarea in progress, where offered.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModeChange {
    pub to: String,
}

/// Committing an in-progress root: offered now, or withheld by `held` rows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CommitOffer {
    pub offered: bool,
    pub held: Option<u64>,
}

/// An act the offers do not name, with the refusal its verb would answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RefusedAct {
    /// `propose` or `mode`.
    pub op: String,
    /// The rung, or the mode (`in_progress`, `standard`).
    pub to: String,
    pub code: String,
    pub message: String,
}

/// `GET /v2/workareas/{workarea}/offers` (openapi.yaml `WorkareaOffers`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkareaOffers {
    pub workarea_id: String,
    pub status: String,
    pub mode: String,
    pub transitions: Vec<OfferedTransition>,
    pub mode_change: Option<ModeChange>,
    pub commit: CommitOffer,
    /// Absent from an API before v0.6.4.
    #[serde(default)]
    pub refused: Vec<RefusedAct>,
}

/// The version a supersession replaces, as a memory names it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Replaced {
    pub artifact_id: String,
    pub version: i64,
}

/// One memory as the API serves it (openapi.yaml `Memory`), the fields a
/// reviewer's card reads.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Memory {
    pub artifact_id: String,
    pub claim_key: String,
    pub version: i64,
    pub tip: bool,
    #[serde(default)]
    pub workarea_path: Option<String>,
    pub state: String,
    /// `None` for a universe claim, which names no author (ADR-0044 d9).
    #[serde(default)]
    pub principal: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// What a derived format's claim will be found by (ADR-0056 d9).
    #[serde(default)]
    pub search_text: Option<String>,
    #[serde(default)]
    pub anchor: Option<Value>,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub replaces: Option<Replaced>,
    #[serde(default)]
    pub replaces_valid_until: Option<String>,
    /// On a held guardrail, what approving it does (ADR-0037 d10).
    #[serde(default)]
    pub load_warning: Option<String>,
}

/// `POST /v2/review/next`: one served item, claimed by the reviewer key.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Serve {
    pub serve_id: String,
    pub expires_at: String,
    pub item: Memory,
}

/// `POST /v2/review/verdicts`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VerdictResult {
    pub artifact_id: String,
    pub version: i64,
    pub verdict: String,
    pub state: String,
}

/// One row of the held listing: the memory, when it entered the queue, and
/// whether its author names the reviewer's user (ADR-0055 d7).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HeldItem {
    // Not `pub`, so the contract check reads `Memory` once, through
    // `reviewNext`, where the schema names it.
    #[serde(flatten)]
    memory: Memory,
    pub held_since: String,
    pub same_user: bool,
}

impl HeldItem {
    /// The row itself.
    #[must_use]
    pub fn memory(&self) -> &Memory {
        &self.memory
    }
}

/// `GET /v2/review/held`: one page of the rows the key may judge.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HeldPage {
    pub items: Vec<HeldItem>,
    pub next_cursor: Option<String>,
}

/// One id named to `GET /v2/review/held?ids=`, with its row, or `None` when
/// it is no longer held or no longer the key's to judge.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NamedHeld {
    pub id: String,
    pub item: Option<Memory>,
}

/// `GET /v2/review/held?ids=` (openapi.yaml `HeldNamed`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HeldNamed {
    pub items: Vec<NamedHeld>,
}

/// One item of a bulk verdict's answer.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BulkItem {
    pub artifact_id: String,
    /// `approved`, `rejected` or `skipped`.
    pub outcome: String,
    #[serde(default)]
    pub same_user: Option<bool>,
    #[serde(default)]
    pub replayed: Option<bool>,
    #[serde(default)]
    pub code: Option<String>,
}

/// The bulk verdict's counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct BulkSummary {
    pub approved: u64,
    pub rejected: u64,
    pub skipped: u64,
}

/// `POST /v2/review/verdicts/bulk` (openapi.yaml `BulkVerdictResult`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BulkVerdictResult {
    pub items: Vec<BulkItem>,
    pub summary: BulkSummary,
}

/// One workarea holding held rows, as the admin's queue names it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct QueueItem {
    pub workarea_id: String,
    pub path: String,
    pub held: u64,
    pub oldest_at: String,
}

/// `GET /v2/review/queue`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReviewQueue {
    pub items: Vec<QueueItem>,
}

/// A grant as a listed key carries it: the role and the workarea's path.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListedGrant {
    pub role: String,
    pub workarea_id: String,
    pub path: String,
}

/// One key of `GET /v2/keys` (openapi.yaml `ListedKey`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListedKey {
    pub id: String,
    pub name: Option<String>,
    pub role: String,
    pub held_by: String,
    pub credential: String,
    pub revoked: bool,
    pub cut_off: bool,
    pub grants: Vec<ListedGrant>,
}

/// `GET /v2/keys`: one page of keys, in name order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KeyPage {
    pub items: Vec<ListedKey>,
    pub next_cursor: Option<String>,
}

/// A connect code, shown once (openapi.yaml `ConnectCode`).
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct ConnectCode {
    pub code_id: String,
    #[serde(default)]
    pub code: Option<String>,
    pub expires_at: String,
    pub connector_url: String,
}

/// No debug output prints the code.
impl std::fmt::Debug for ConnectCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectCode")
            .field("code_id", &self.code_id)
            .field("code", &self.code.as_ref().map(|_| "<redacted>"))
            .field("expires_at", &self.expires_at)
            .field("connector_url", &self.connector_url)
            .finish()
    }
}

/// `POST /v2/keys` (openapi.yaml `KeyIssued`): the key, and exactly one of
/// its value and its connect code, each shown once.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct KeyIssued {
    pub key_id: String,
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub connect_code: Option<ConnectCode>,
}

/// No debug output prints the value or the code.
impl std::fmt::Debug for KeyIssued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyIssued")
            .field("key_id", &self.key_id)
            .field("name", &self.name)
            .field("value", &self.value.as_ref().map(|_| "<redacted>"))
            .field("connect_code", &self.connect_code)
            .finish()
    }
}

/// `POST /v2/keys/{key}/denials`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Denial {
    pub denial_id: String,
    pub epoch: i64,
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
        self.send(Method::GET, call, path, None, None).await
    }

    /// A `POST` of `body` as JSON under `idempotency_key`, so a
    /// resubmission under the same key is answered as recorded and acts
    /// nothing again (docs/api/http-api.md, idempotency).
    async fn post<R: DeserializeOwned>(
        &self,
        call: &Call<'_>,
        path: String,
        idempotency_key: &str,
        body: &Value,
    ) -> Result<R, Error> {
        self.send(
            Method::POST,
            Some(call),
            path,
            Some(idempotency_key),
            Some(body),
        )
        .await
    }

    async fn send<R: DeserializeOwned>(
        &self,
        method: Method,
        call: Option<&Call<'_>>,
        path: String,
        idempotency_key: Option<&str>,
        body: Option<&Value>,
    ) -> Result<R, Error> {
        let bytes = body.map_or_else(Vec::new, |body| body.to_string().into_bytes());
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .body(bytes)
            .map_err(|e| Error::Decode {
                status: 0,
                why: format!("the request could not be built: {e}"),
            })?;
        let headers = request.headers_mut();
        headers.extend(self.fixed.clone());
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if body.is_some() {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
        if let Some(key) = idempotency_key {
            let key = HeaderValue::from_str(key).map_err(|_| Error::Decode {
                status: 0,
                why: "the idempotency key is not a header value".to_owned(),
            })?;
            headers.insert(IDEMPOTENCY_KEY, key);
        }
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
        // `204 No Content` decodes as JSON `null`, so an `Option` answer
        // reads it as `None` and any other type refuses it.
        let body: &[u8] = if status == 204 {
            b"null"
        } else {
            response.body()
        };
        serde_json::from_slice(body).map_err(|e| Error::Decode {
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

    /// `GET /v2/workareas/{workarea}`: the node and its live proposals.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn workarea(&self, call: &Call<'_>, workarea: &str) -> Result<WorkareaView, Error> {
        self.get(Some(call), format!("/v2/workareas/{}", encode(workarea)))
            .await
    }

    /// `GET /v2/workareas/{workarea}/offers`: the acts the caller's verbs
    /// admit there now, and each refused act's refusal.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn workarea_offers(
        &self,
        call: &Call<'_>,
        workarea: &str,
    ) -> Result<WorkareaOffers, Error> {
        self.get(
            Some(call),
            format!("/v2/workareas/{}/offers", encode(workarea)),
        )
        .await
    }

    /// `POST /v2/workareas/{workarea}/transitions`: proposes `to`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn propose_transition(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        workarea: &str,
        to: &str,
    ) -> Result<Transition, Error> {
        self.post(
            call,
            format!("/v2/workareas/{}/transitions", encode(workarea)),
            idempotency_key,
            &serde_json::json!({ "to": to }),
        )
        .await
    }

    /// `POST /v2/workareas/{workarea}/transitions/{transition_id}/ratify`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn ratify_transition(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        workarea: &str,
        transition_id: &str,
    ) -> Result<Transition, Error> {
        self.send(
            Method::POST,
            Some(call),
            format!(
                "/v2/workareas/{}/transitions/{}/ratify",
                encode(workarea),
                encode(transition_id)
            ),
            Some(idempotency_key),
            None,
        )
        .await
    }

    /// `POST /v2/workareas/{workarea}/mode`: `in_progress`, or `standard`,
    /// which on an in-progress root is its commit.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn set_workarea_mode(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        workarea: &str,
        mode: &str,
    ) -> Result<WorkareaView, Error> {
        self.post(
            call,
            format!("/v2/workareas/{}/mode", encode(workarea)),
            idempotency_key,
            &serde_json::json!({ "mode": mode }),
        )
        .await
    }

    /// `POST /v2/review/next`: the next held item, claimed by the key, or
    /// `None` when nothing in scope waits (`204`).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn review_next(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
    ) -> Result<Option<Serve>, Error> {
        self.send(
            Method::POST,
            Some(call),
            "/v2/review/next".to_owned(),
            Some(idempotency_key),
            None,
        )
        .await
    }

    /// `POST /v2/review/verdicts`: `verdict` (`approve` or `reject`) on the
    /// item `serve_id` served, with `reason`, required to reject.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn post_verdict(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        serve_id: &str,
        verdict: &str,
        reason: Option<&str>,
    ) -> Result<VerdictResult, Error> {
        let mut body = serde_json::json!({ "serve_id": serve_id, "verdict": verdict });
        if let Some(reason) = reason {
            body["reason"] = Value::from(reason);
        }
        self.post(
            call,
            "/v2/review/verdicts".to_owned(),
            idempotency_key,
            &body,
        )
        .await
    }

    /// `GET /v2/review/held`: one page of `limit` rows the key may judge,
    /// narrowed by each filter given, after `cursor`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn review_held(
        &self,
        call: &Call<'_>,
        filters: &[(&str, &str)],
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<HeldPage, Error> {
        let mut path = format!("/v2/review/held?limit={limit}");
        for (name, value) in filters
            .iter()
            .filter(|(n, _)| matches!(*n, "workarea" | "author_key" | "claim_key_prefix"))
        {
            path.push_str(&format!("&{name}={}", encode(value)));
        }
        if let Some(cursor) = cursor {
            path.push_str("&cursor=");
            path.push_str(&encode(cursor));
        }
        self.get(Some(call), path).await
    }

    /// `GET /v2/review/held?ids=`: each id named, with its row while it is
    /// still held and the key's to judge. Claims nothing.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn review_named(&self, call: &Call<'_>, ids: &[&str]) -> Result<HeldNamed, Error> {
        let ids: Vec<String> = ids.iter().map(|id| encode(id)).collect();
        self.get(Some(call), format!("/v2/review/held?ids={}", ids.join(",")))
            .await
    }

    /// `POST /v2/review/verdicts/bulk`: one `verdict` on each of
    /// `artifact_ids`, with `reason`, required to reject.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn post_bulk_verdict(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        verdict: &str,
        artifact_ids: &[&str],
        reason: Option<&str>,
    ) -> Result<BulkVerdictResult, Error> {
        let mut body = serde_json::json!({ "verdict": verdict, "artifact_ids": artifact_ids });
        if let Some(reason) = reason {
            body["reason"] = Value::from(reason);
        }
        self.post(
            call,
            "/v2/review/verdicts/bulk".to_owned(),
            idempotency_key,
            &body,
        )
        .await
    }

    /// `GET /v2/review/queue`: each workarea holding held rows, with its
    /// count and its oldest row's time. An admin key's.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn review_queue(&self, call: &Call<'_>) -> Result<ReviewQueue, Error> {
        self.get(Some(call), "/v2/review/queue".to_owned()).await
    }

    /// `GET /v2/keys`: one page of `limit` keys after `cursor`; `user`
    /// names the person, which an admin key must.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn list_keys(
        &self,
        call: &Call<'_>,
        user: Option<&str>,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<KeyPage, Error> {
        let mut path = format!("/v2/keys?limit={limit}");
        if let Some(user) = user {
            path.push_str("&user=");
            path.push_str(&encode(user));
        }
        if let Some(cursor) = cursor {
            path.push_str("&cursor=");
            path.push_str(&encode(cursor));
        }
        self.get(Some(call), path).await
    }

    /// `POST /v2/keys` as a person's web session key: their own key `name`
    /// with `role`, a connector key where `connect_code` is set.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn issue_key(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        name: &str,
        role: &str,
        connect_code: bool,
    ) -> Result<KeyIssued, Error> {
        self.post(
            call,
            "/v2/keys".to_owned(),
            idempotency_key,
            &serde_json::json!({ "name": name, "role": role, "connect_code": connect_code }),
        )
        .await
    }

    /// `POST /v2/keys/{key}/connect-codes`: a new code for a connector key,
    /// voiding its earlier ones.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn mint_connect_code(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        key: &str,
    ) -> Result<ConnectCode, Error> {
        self.send(
            Method::POST,
            Some(call),
            format!("/v2/keys/{}/connect-codes", encode(key)),
            Some(idempotency_key),
            None,
        )
        .await
    }

    /// `POST /v2/keys/{key}/denials` on `workarea`: on `universe`, the key
    /// is cut off.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn add_denial(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        key: &str,
        workarea: &str,
    ) -> Result<Denial, Error> {
        self.post(
            call,
            format!("/v2/keys/{}/denials", encode(key)),
            idempotency_key,
            &serde_json::json!({ "workarea": workarea }),
        )
        .await
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
            decodes(&client.workarea(&CALL, "alice.proj").await),
            decodes(&client.workarea_offers(&CALL, "alice.proj").await),
            decodes(
                &client
                    .propose_transition(&CALL, "idem-1", "alice.proj", "committed")
                    .await,
            ),
            decodes(
                &client
                    .ratify_transition(&CALL, "idem-2", "alice.proj", "t-1")
                    .await,
            ),
            decodes(
                &client
                    .set_workarea_mode(&CALL, "idem-3", "alice.proj", "in_progress")
                    .await,
            ),
            {
                // An `Option`: `None` is the `204` of nothing waiting.
                let _ = client.review_next(&CALL, "idem-0").await;
                "Serve"
            },
            decodes(
                &client
                    .post_verdict(&CALL, "idem-4", "srv_1", "approve", None)
                    .await,
            ),
            decodes(
                &client
                    .review_held(&CALL, &[("workarea", "alice")], 50, None)
                    .await,
            ),
            decodes(&client.review_named(&CALL, &["a", "b"]).await),
            decodes(
                &client
                    .post_bulk_verdict(&CALL, "idem-5", "approve", &["a"], None)
                    .await,
            ),
            decodes(&client.review_queue(&CALL).await),
            decodes(&client.list_keys(&CALL, Some("alice"), 200, None).await),
            decodes(
                &client
                    .issue_key(&CALL, "idem-6", "alice-laptop", "writer", false)
                    .await,
            ),
            decodes(
                &client
                    .mint_connect_code(&CALL, "idem-7", "alice-phone")
                    .await,
            ),
            decodes(
                &client
                    .add_denial(&CALL, "idem-8", "alice-phone", "universe")
                    .await,
            ),
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
        ); // Each POST carries its idempotency key; one with a body sends it
        // as JSON, and the ratify sends none.
        for (i, key, body) in [
            (7, "idem-1", Some(r#"{"to":"committed"}"#)),
            (8, "idem-2", None),
            (9, "idem-3", Some(r#"{"mode":"in_progress"}"#)),
        ] {
            let h = seen[i].headers();
            assert_eq!(h[IDEMPOTENCY_KEY], key);
            assert_eq!(h.get(CONTENT_TYPE).is_some(), body.is_some());
            assert_eq!(
                std::str::from_utf8(seen[i].body()).unwrap(),
                body.unwrap_or("")
            );
        }
        assert!(!seen[6].headers().contains_key(IDEMPOTENCY_KEY));
        assert_eq!(seen[12].uri(), "/v2/review/held?limit=50&workarea=alice");
        assert_eq!(seen[13].uri(), "/v2/review/held?ids=a,b");
        assert_eq!(seen[16].uri(), "/v2/keys?limit=200&user=alice");
        assert_eq!(
            std::str::from_utf8(seen[17].body()).unwrap(),
            r#"{"connect_code":false,"name":"alice-laptop","role":"writer"}"#
        );
    }

    #[tokio::test]
    async fn nothing_waiting_for_review_is_none() {
        let (client, _) = recording(204, "");
        assert_eq!(client.review_next(&CALL, "idem").await, Ok(None));
    }

    #[test]
    fn no_debug_output_prints_a_shown_once_secret() {
        let issued = KeyIssued {
            key_id: "k".to_owned(),
            name: "alice-laptop".to_owned(),
            value: Some("oml_the-secret-value".to_owned()),
            connect_code: Some(ConnectCode {
                code_id: "c".to_owned(),
                code: Some("ABCD-EFGH-JKMN".to_owned()),
                expires_at: "2026-11-11T00:00:00Z".to_owned(),
                connector_url: "https://oml.example.com/mcp".to_owned(),
            }),
        };
        let shown = format!("{issued:?}");
        assert!(!shown.contains("oml_the-secret-value"), "{shown}");
        assert!(!shown.contains("ABCD-EFGH-JKMN"), "{shown}");
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
