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
pub const OPERATIONS: [&str; 41] = [
    "authorizationServerMetadata GET /.well-known/oauth-authorization-server resp:AuthorizationServerMetadata",
    "whoami GET /v2/whoami query:person resp:Whoami",
    "getGuardrails GET /v1/guardrails query:workarea resp:GuardrailSet",
    "getWorkareaTree GET /v2/workareas/{workarea}/tree query:limit,cursor resp:WorkareaPage",
    "readyz GET /readyz resp:Ready",
    "getWorkarea GET /v2/workareas/{workarea} resp:WorkareaView",
    "getWorkareaOffers GET /v2/workareas/{workarea}/offers resp:WorkareaOffers",
    "proposeTransition POST /v2/workareas/{workarea}/transitions body:to resp:Transition",
    "ratifyTransition POST /v2/workareas/{workarea}/transitions/{transition_id}/ratify resp:Transition",
    "setWorkareaMode POST /v2/workareas/{workarea}/mode body:mode resp:WorkareaView",
    "listMemories GET /v1/memories query:with_own,include_text,author,workarea,state,origin_kinds,limit,cursor resp:MemoryPage",
    "recallMemories POST /v1/memories/recall body:query,k,workarea,kinds,include_text,include_unreviewed,origin_kinds resp:RecallResponse",
    "adminSearchMemories POST /v2/admin/memories/search body:query,workarea,state,origin_kind,author,limit,cursor resp:AdminMemoryPage",
    "getWorkareaTree GET /v2/workareas/{workarea}/tree query:counts,offers resp:WorkareaCounts",
    "listKeys GET /v2/keys query:user,limit,cursor resp:KeyPage",
    "createWorkarea POST /v2/workareas body:name,parent resp:Workarea",
    "reviewNext POST /v2/review/next resp:Serve",
    "postVerdict POST /v2/review/verdicts body:serve_id,verdict,reason resp:VerdictResult",
    "reviewHeld GET /v2/review/held query:workarea,author_key,claim_key_prefix,limit,cursor resp:HeldPage",
    "reviewHeld GET /v2/review/held query:ids resp:HeldNamed",
    "postBulkVerdict POST /v2/review/verdicts/bulk body:verdict,artifact_ids,reason resp:BulkVerdictResult",
    "reviewQueue GET /v2/review/queue resp:ReviewQueue",
    "issueKey POST /v2/keys body:name,role,connect_code,expiry resp:KeyIssued",
    "mintConnectCode POST /v2/keys/{key}/connect-codes resp:ConnectCode",
    "addDenial POST /v2/keys/{key}/denials body:workarea resp:Denial",
    "getMemory GET /v1/memories/{artifact_id} resp:Memory",
    "adminReadMemory GET /v2/admin/memories/{artifact_id} resp:AdminRead",
    "submitMemory POST /v1/memories body:verb,claim_key,body,workarea,derives_from resp:Accepted",
    "supersedeMemory POST /v1/memories/{artifact_id}/supersede body:body,expected_version,retirement resp:Accepted",
    "whoami GET /v2/whoami query:start resp:Onboarding",
    "listWebEmails GET /v2/web/emails query:limit,cursor resp:WebEmailPage",
    "allowWebEmail POST /v2/web/emails body:email,user,admin,keep_an_admin resp:WebEmail",
    "removeWebEmail POST /v2/web/emails/remove body:email,keep_an_admin resp:Removed",
    "grantWebCapacities POST /v2/web/users/{user}/capacities body:capacity,workareas resp:CapacityGranted",
    "listWebUsers GET /v2/web/users resp:WebUsers",
    "setWebUserQuota POST /v2/web/users/{user}/quota body:memories,keys,connections,requests_per_minute resp:WebUser",
    "disableWebUser POST /v2/web/users/{user}/disable resp:Disabled",
    "closeWebDeletion POST /v2/web/users/{user}/deletion/close body:outcome resp:Closed",
    "exportWebPerson GET /v2/web/export resp:Export",
    "requestWebDeletion POST /v2/web/deletion-requests resp:DeletionRequested",
    "whoami GET /v2/whoami resp:Onboarding",
];

/// The request id header (ADR-0042 d4).
pub const REQUEST_ID: &str = "x-request-id";
/// The correlation id header (ADR-0042 d4).
pub const CORRELATION_ID: &str = "x-correlation-id";
/// The idempotency key every state-creating `POST` carries.
pub const IDEMPOTENCY_KEY: &str = "idempotency-key";
/// The header the API sets on an answer it replayed from a recorded
/// outcome (http-api.md idempotency).
pub const IDEMPOTENCY_REPLAY: &str = "idempotency-replay";

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
    /// Whether a person's `<user>-web` session key carries a denial on any
    /// workarea, which refuses it issuing a key, minting a connect code and
    /// cutting off a key for good; absent for every other key (ADR-0070 d2,
    /// d5).
    #[serde(default)]
    pub denied: Option<bool>,
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
    /// The node it sits in; `None` for the tree's own root.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Whether the node is a person's root (ADR-0044 d1), which makes the
    /// names beneath it `<user>.<name>` (d5).
    #[serde(default)]
    pub user_root: bool,
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
    /// The review policy in force (ADR-0009 d3); `None` from an API that
    /// does not serve it.
    #[serde(default)]
    pub policy: Option<ReviewPolicy>,
}

/// A workarea's review policy (openapi.yaml `ReviewPolicy`): how many
/// approving verdicts promote a version, and from which roles.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReviewPolicy {
    #[serde(default)]
    pub k: Option<u32>,
    #[serde(default)]
    pub required_roles: Vec<String>,
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

/// An anchor as the API serves it (openapi.yaml `Anchor`): a repository
/// with its commit and path, or a URI; each with an optional fragment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Anchor {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub rev: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub fragment: Option<String>,
}

/// The version a supersession replaces, as a memory names it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Replaced {
    pub artifact_id: String,
    pub version: i64,
}

/// A memory as the listings and recall serve it (openapi.yaml `Memory`),
/// the fields the Memories list reads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Memory {
    pub artifact_id: String,
    pub claim_key: String,
    pub version: u32,
    pub tip: bool,
    /// The workarea's id; `None` where the key holds no role on it.
    #[serde(default)]
    pub workarea_id: Option<String>,
    pub workarea_path: Option<String>,
    pub state: String,
    pub principal: Option<String>,
    /// Absent from an API before v0.7.0.
    #[serde(default)]
    pub author_key_id: Option<String>,
    pub created_at: String,
    pub promoted_at: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// What a derived format's claim will be found by (ADR-0056 d9).
    #[serde(default)]
    pub search_text: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub anchor: Option<Anchor>,
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
    /// Absent from an API before v0.7.0.
    #[serde(default)]
    pub scan_finding: Option<String>,
    /// Every finding stored on the version, on the review serve, the held
    /// listing and the named read; `None` elsewhere and from an API that
    /// predates the findings list (ADR-0074 d17, d23).
    #[serde(default)]
    pub scan_findings: Option<Vec<ScanFinding>>,
    /// When the approving verdict was given; `None` on a version that is
    /// not promoted, on an anonymous `universe` row and from an API before
    /// v0.7.2 (ADR-0077 d1, d4).
    #[serde(default)]
    pub approved_at: Option<String>,
    /// What kind of reviewer approved it, never who; `None` wherever
    /// `approved_at` is (ADR-0077 d2).
    #[serde(default)]
    pub approver: Option<Approver>,
    /// `memory`, `derived_fact`, `steering`, `guardrail` or a vocabulary
    /// kind; `None` from an API that does not name it.
    #[serde(default)]
    pub kind: Option<String>,
    /// Who wrote it, by kind: `agent`, `human`, `connector` or
    /// `simulation`.
    #[serde(default)]
    pub provenance: Option<String>,
    /// What kind of source the text came from, as signed; `None` on a
    /// version written before ADR-0063.
    #[serde(default)]
    pub origin: Option<Origin>,
    /// Whether this version is a retirement: approving it stops serving
    /// the claim.
    #[serde(default)]
    pub retirement: bool,
    /// `changed` or `removed` from the served version's latest source
    /// check; `None` otherwise, and always on a held version (ADR-0057 d5).
    #[serde(default)]
    pub stale: Option<String>,
    /// The version's latest source check; `None` on a held version.
    #[serde(default)]
    pub source_checked: Option<SourceChecked>,
    /// The ancestors the caller can read (ADR-0025 d3).
    #[serde(default)]
    pub derives_from: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// Budget tokens of the body (counter `oml-tc/1`).
    #[serde(default)]
    pub tokens: Option<u64>,
    /// Whether the served version is held: the caller's own, or another
    /// author's inside an in-progress workarea (ADR-0030 d3, d4).
    #[serde(default)]
    pub unreviewed: bool,
    /// When held-item expiry dead-letters a held row; `None` elsewhere.
    #[serde(default)]
    pub held_expires_at: Option<String>,
    /// The SHA-256 of the plaintext, `sha256:<hex>`.
    #[serde(default)]
    pub plaintext_digest: Option<String>,
}

/// What kind of source a version's text came from (openapi.yaml `Origin`;
/// ADR-0063 d1).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Origin {
    /// `repo`, `doc`, `code`, `lake`, `chat`, `web` or `manual`.
    pub kind: String,
}

/// A version's latest source check (openapi.yaml `SourceChecked`;
/// ADR-0057 d5).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SourceChecked {
    pub rev: String,
    pub at: String,
    /// `fresh`, `equivalent`, `moved`, `changed` or `removed`.
    pub result: String,
    #[serde(default)]
    pub moved_to: Option<MovedTo>,
}

/// Where a `moved` source check found the span (ADR-0057 d18).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MovedTo {
    pub path: String,
    #[serde(default)]
    pub fragment: Option<String>,
}

/// An attachment as a memory names it (openapi.yaml `AttachmentDescriptor`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// The kind of reviewer that approved a version (ADR-0077 d2): the
/// reviewing key's attestation, `agent`, `human` or `service`, and whether
/// that key acts for the author's person.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Approver {
    pub held_by: String,
    pub same_user: bool,
}

/// One finding of a version's findings list (openapi.yaml `ScanFinding`;
/// ADR-0074 d23): where the scan fired and why, never the matched text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScanFinding {
    pub class: String,
    pub rule: String,
    /// `refuse`, `hold` or `annotate`.
    pub verdict: String,
    #[serde(default)]
    pub rule_set: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub via: Option<String>,
    /// `admission` or `rescan`.
    pub found_by: String,
    #[serde(default)]
    pub migrated_from: Option<String>,
}

/// `GET /v1/memories`: one page of memories.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MemoryPage {
    pub items: Vec<Memory>,
    pub next_cursor: Option<String>,
}

/// One ranked hit of a recall.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecallHit {
    pub memory: Memory,
}

/// `POST /v1/memories/recall`: steering first, then the ranked hits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecallResponse {
    pub steering: Vec<Memory>,
    pub hits: Vec<RecallHit>,
}

/// A memory as the admin read serves it (openapi.yaml `AdminMemory`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminMemory {
    pub artifact_id: String,
    pub claim_key: String,
    pub version: u32,
    pub state: String,
    pub workarea_path: String,
    pub text: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    pub key: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub anchor: Option<Anchor>,
    /// Absent, with the four fields after it, from an API before v0.7.0.
    #[serde(default)]
    pub held_by: Option<String>,
    #[serde(default)]
    pub promoted_at: Option<String>,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub scan_finding: Option<String>,
}

/// One verdict on a memory, as the admin read lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminVerdict {
    pub verdict: String,
    pub key: Option<String>,
    pub principal: Option<String>,
    pub reason: Option<String>,
    pub at: String,
}

/// `GET /v2/admin/memories/{artifact_id}`: one memory with its full
/// attribution (ADR-0045 d21).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminRead {
    pub artifact_id: String,
    pub claim_key: String,
    pub version: u32,
    pub state: String,
    pub workarea_path: String,
    pub text: String,
    /// The kind the version was written as; absent from an API before
    /// v0.7.0.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    pub key: Option<String>,
    /// The authoring key's id; absent from an API before v0.7.0.
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default)]
    pub principal: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub anchor: Option<Anchor>,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub derives_from: Vec<String>,
    #[serde(default)]
    pub replaces: Option<String>,
    #[serde(default)]
    pub plaintext_digest: Option<String>,
    #[serde(default)]
    pub verdicts: Vec<AdminVerdict>,
    /// The authoring key's holder, `agent`, `human` or `service`; absent,
    /// with the two fields after it, from an API before v0.7.0.
    #[serde(default)]
    pub held_by: Option<String>,
    #[serde(default)]
    pub promoted_at: Option<String>,
    /// Admission's scan finding, `class: rule at place`, never the matched
    /// text (ADR-0065 d8).
    #[serde(default)]
    pub scan_finding: Option<String>,
}

/// `POST /v1/memories` and `POST /v1/memories/{artifact_id}/supersede`:
/// the version written, held.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Accepted {
    pub artifact_id: String,
    pub claim_key: String,
    pub version: u32,
    pub state: String,
}

/// What [`Client::learn`] submits: the text, the workarea, the memories
/// it derives from and an optional claim key.
#[derive(Debug, Clone, Copy)]
pub struct Learn<'a> {
    pub body: &'a str,
    pub workarea: &'a str,
    pub derives_from: &'a [&'a str],
    pub claim_key: Option<&'a str>,
}

/// A person's usage, as `GET /v2/whoami` and `GET /v2/web/users` serve it:
/// the stocks, and the versions submitted this UTC month (ADR-0071 d2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Usage {
    pub memories: u64,
    /// Absent, with `writes`, from an API before the usage meters.
    #[serde(default)]
    pub text_bytes: Option<u64>,
    #[serde(default)]
    pub writes: Option<u64>,
    pub keys: u64,
    pub connections: u64,
}

/// A person's limits, each `None` where the quota is unlimited (the API
/// serves an unlimited quota as null, ADR-0071 d5); `text_bytes` and
/// `writes_per_month` are also `None` from an API before the usage meters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Limits {
    pub memories: Option<u64>,
    pub keys: Option<u64>,
    pub connections: Option<u64>,
    pub requests_per_minute: Option<u64>,
    #[serde(default)]
    pub text_bytes: Option<u64>,
    #[serde(default)]
    pub writes_per_month: Option<u64>,
}

/// What the onboarding page waits for (ADR-0068 d10): whether an agent
/// key connected, and the state of the newest memory of the person's root.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StartStatus {
    pub connected: bool,
    pub first_memory: Option<String>,
}

/// The person a session key names, with the onboarding status.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Person {
    pub user: String,
    pub under_quotas: bool,
    pub usage: Option<Usage>,
    pub quotas: Option<Limits>,
    pub deletion_requested_at: Option<String>,
    #[serde(default)]
    pub start: Option<StartStatus>,
}

/// `GET /v2/whoami` with its person block: the key's person, and with
/// `start=true` the onboarding status on it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Onboarding {
    pub person: Option<Person>,
}

/// One allowlist entry (ADR-0045 d2).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WebEmail {
    pub email: String,
    pub user: String,
    pub admin: bool,
}

/// `GET /v2/web/emails`: one page of the allowlist.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WebEmailPage {
    pub items: Vec<WebEmail>,
    pub next_cursor: Option<String>,
}

/// `POST /v2/web/emails/remove`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Removed {
    pub journal_seq: i64,
}

/// The key a capacity grant reached.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GrantedKey {
    pub id: String,
    pub name: Option<String>,
}

/// `POST /v2/web/users/{user}/capacities`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CapacityGranted {
    pub key: GrantedKey,
    pub journal_seq: Option<i64>,
}

/// One person as `GET /v2/web/users` lists them (openapi.yaml `WebUser`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WebUser {
    pub user: String,
    pub signed_up: bool,
    pub disabled: bool,
    pub under_quotas: bool,
    pub usage: Usage,
    pub quotas: Limits,
    /// Each live admin key whose row names the person's root, a row no verb
    /// makes, which refuses their disable (ADR-0068 d8, d11); empty from a
    /// server before v0.7.1.
    #[serde(default)]
    pub admin_keys: Vec<String>,
}

/// A pending deletion request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeletionRequest {
    pub user: String,
    pub requested_at: String,
}

/// `GET /v2/web/users`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WebUsers {
    pub users: Vec<WebUser>,
    pub deletions: Vec<DeletionRequest>,
}

/// `POST /v2/web/users/{user}/disable`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Disabled {
    pub keys_cut_off: u64,
    pub journal_seq: i64,
}

/// `POST /v2/web/users/{user}/deletion/close`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Closed {
    pub journal_seq: i64,
}

/// `GET /v2/web/export`: everything the person holds; the memories and
/// keys as the API wrote them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Export {
    pub format: String,
    pub user: Option<String>,
    pub exported_at: String,
    pub memories: Vec<Value>,
    pub keys: Vec<Value>,
}

/// `POST /v2/web/deletion-requests`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeletionRequested {
    pub requested_at: Option<String>,
    pub journal_seq: i64,
}

/// `POST /v2/admin/memories/search`: one page.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminMemoryPage {
    pub items: Vec<AdminMemory>,
    pub next_cursor: Option<String>,
}

/// One node's rows by state, as the caller's merged listing shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct NodeCounts {
    pub promoted: u64,
    pub held: u64,
    pub in_progress: u64,
    pub other: u64,
}

/// One node of a counted subtree.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CountedNode {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub path: String,
    pub status: String,
    pub mode: String,
    pub counts: NodeCounts,
    /// Whether the node is a person's root (ADR-0044 d1).
    #[serde(default)]
    pub user_root: bool,
}

/// `GET /v2/workareas/{workarea}/tree?counts=true`: the subtree, a parent
/// before its children, with each node's counts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkareaCounts {
    pub items: Vec<CountedNode>,
    pub truncated: bool,
    /// The scope's offers, asked with `offers=true` (#286); absent when not
    /// asked. An API before v0.7.1 refuses `offers=true` as
    /// `request.validation`, so a caller that must work one release behind
    /// reads the offers apart on that refusal (ADR-0070 d4).
    #[serde(default)]
    pub offers: Option<WorkareaOffers>,
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
    /// A skipped item's refusal message, as the single verdict answers it.
    #[serde(default)]
    pub message: Option<String>,
    /// A decided item's state, `promoted` or `rejected`, and its version.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub version: Option<i64>,
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
    /// The instant from which the key authenticates nothing; `None` for a
    /// key that does not expire (ADR-0079 d3).
    #[serde(default)]
    pub expiry: Option<String>,
    /// Whether the expiry has come on the server's clock (ADR-0079 d3).
    #[serde(default)]
    pub expired: bool,
    /// The key's own requests per UTC minute; `None` for none of its own
    /// (ADR-0079 d4).
    #[serde(default)]
    pub requests_per_minute: Option<u32>,
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
    /// Whether the answer was a recorded outcome replayed, which carries no
    /// code; read off `Idempotency-Replay`, not the body.
    #[serde(skip)]
    replayed: bool,
}

impl ConnectCode {
    /// A connect code as a first answer, not a replay: what a fake transport
    /// or a caller's test builds, since the replay mark is the client's to
    /// set (#281).
    #[must_use]
    pub fn new(
        code_id: String,
        code: Option<String>,
        expires_at: String,
        connector_url: String,
    ) -> Self {
        Self {
            code_id,
            code,
            expires_at,
            connector_url,
            replayed: false,
        }
    }

    /// Whether the API answered `Idempotency-Replay: true`: a replay of a
    /// recorded outcome, which shows no code.
    #[must_use]
    pub fn replayed(&self) -> bool {
        self.replayed
    }
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
    /// Whether the answer was a recorded outcome replayed, which carries no
    /// value or code; read off `Idempotency-Replay`, not the body.
    #[serde(skip)]
    replayed: bool,
}

impl KeyIssued {
    /// A key issue as a first answer, not a replay: what a fake transport
    /// or a caller's test builds, since the replay mark is the client's to
    /// set (#281).
    #[must_use]
    pub fn new(
        key_id: String,
        name: String,
        value: Option<String>,
        connect_code: Option<ConnectCode>,
    ) -> Self {
        Self {
            key_id,
            name,
            value,
            connect_code,
            replayed: false,
        }
    }

    /// Whether the API answered `Idempotency-Replay: true`: a replay of a
    /// recorded issue, which shows neither the value nor the code.
    #[must_use]
    pub fn replayed(&self) -> bool {
        self.replayed
    }
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

/// `POST /v2/workareas`: the new node, the fields read here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Workarea {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// The Memories list's filters for `GET /v1/memories` with `with_own`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryQuery<'a> {
    pub workarea: Option<&'a str>,
    pub state: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub author: Option<&'a str>,
    pub limit: u32,
    pub cursor: Option<&'a str>,
}

/// A text search of the caller's readable memories, its own held ones
/// among them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecallQuery<'a> {
    pub query: &'a str,
    pub k: u32,
    pub workarea: Option<&'a str>,
    pub origin: Option<&'a str>,
}

/// The admin read's filters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminSearch<'a> {
    pub query: Option<&'a str>,
    pub workarea: Option<&'a str>,
    pub state: Option<&'a str>,
    pub origin_kind: Option<&'a str>,
    pub author: Option<&'a str>,
    pub limit: u32,
    pub cursor: Option<&'a str>,
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
        self.exchange(method, call, path, idempotency_key, body)
            .await
            .map(|(answer, _)| answer)
    }

    /// [`Self::send`], with whether the API answered a recorded outcome:
    /// `Idempotency-Replay: true` on the answer (http-api.md idempotency).
    async fn exchange<R: DeserializeOwned>(
        &self,
        method: Method,
        call: Option<&Call<'_>>,
        path: String,
        idempotency_key: Option<&str>,
        body: Option<&Value>,
    ) -> Result<(R, bool), Error> {
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
        let replayed = response
            .headers()
            .get(IDEMPOTENCY_REPLAY)
            .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"true"));
        let body: &[u8] = if status == 204 {
            b"null"
        } else {
            response.body()
        };
        serde_json::from_slice(body)
            .map(|answer| (answer, replayed))
            .map_err(|e| Error::Decode {
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

    /// `GET /v2/whoami?person=false`: the key's record without the person
    /// block, which [`Whoami`] does not read, so the API reads none of it
    /// (#286); an API before v0.7.1 ignores the flag and serves it.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn whoami(&self, call: &Call<'_>) -> Result<Whoami, Error> {
        self.get(Some(call), "/v2/whoami?person=false".to_owned())
            .await
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

    /// `GET /v1/memories` with `with_own=true` and `include_text=true`: the
    /// caller's merged listing with each item's text.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn list_memories(
        &self,
        call: &Call<'_>,
        query: &MemoryQuery<'_>,
    ) -> Result<MemoryPage, Error> {
        let mut path = format!(
            "/v1/memories?with_own=true&include_text=true&limit={}",
            query.limit
        );
        for (name, value) in [
            ("author", query.author),
            ("workarea", query.workarea),
            ("state", query.state),
            ("origin_kinds", query.origin),
            ("cursor", query.cursor),
        ] {
            if let Some(value) = value {
                path.push_str(&format!("&{name}={}", encode(value)));
            }
        }
        self.get(Some(call), path).await
    }

    /// `POST /v1/memories/recall`: memories and derived facts with their
    /// text, the caller's own held ones among them. Read-shaped: no
    /// idempotency key.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn recall(
        &self,
        call: &Call<'_>,
        query: &RecallQuery<'_>,
    ) -> Result<RecallResponse, Error> {
        let mut body = serde_json::json!({
            "query": query.query,
            "k": query.k,
            "kinds": ["memory", "derived_fact"],
            "include_text": true,
            "include_unreviewed": true,
        });
        if let Some(workarea) = query.workarea {
            body["workarea"] = serde_json::json!(workarea);
        }
        if let Some(origin) = query.origin {
            body["origin_kinds"] = serde_json::json!([origin]);
        }
        self.send(
            Method::POST,
            Some(call),
            "/v1/memories/recall".to_owned(),
            None,
            Some(&body),
        )
        .await
    }

    /// `POST /v2/admin/memories/search`: one page of the tenant's memories,
    /// journalled as one admin read. Read-shaped: no idempotency key.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn admin_search(
        &self,
        call: &Call<'_>,
        search: &AdminSearch<'_>,
    ) -> Result<AdminMemoryPage, Error> {
        let mut body = serde_json::json!({ "limit": search.limit });
        for (name, value) in [
            ("query", search.query),
            ("workarea", search.workarea),
            ("state", search.state),
            ("origin_kind", search.origin_kind),
            ("author", search.author),
            ("cursor", search.cursor),
        ] {
            if let Some(value) = value {
                body[name] = serde_json::json!(value);
            }
        }
        self.send(
            Method::POST,
            Some(call),
            "/v2/admin/memories/search".to_owned(),
            None,
            Some(&body),
        )
        .await
    }

    /// `GET /v2/workareas/{workarea}/tree?counts=true`: the subtree with
    /// each node's rows counted by state, and with `offers` the scope's
    /// offers to the same caller, read beside them (#286).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn workarea_counts(
        &self,
        call: &Call<'_>,
        workarea: &str,
        offers: bool,
    ) -> Result<WorkareaCounts, Error> {
        let asked = if offers { "&offers=true" } else { "" };
        self.get(
            Some(call),
            format!("/v2/workareas/{}/tree?counts=true{asked}", encode(workarea)),
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

    /// `POST /v2/workareas`: creates `name` under `parent`, or under the
    /// caller's own root when `parent` is `None`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn create_workarea(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        name: &str,
        parent: Option<&str>,
    ) -> Result<Workarea, Error> {
        let mut body = serde_json::json!({ "name": name });
        if let Some(parent) = parent {
            body["parent"] = serde_json::json!(parent);
        }
        self.post(call, "/v2/workareas".to_owned(), idempotency_key, &body)
            .await
    }

    /// `POST /v2/keys` as a person's web session key: their own key `name`
    /// with `role`, a connector key where `connect_code` is set, ending at
    /// `expiry`, an RFC 3339 instant with its offset, where one is given
    /// (ADR-0079 d1).
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
        expiry: Option<&str>,
    ) -> Result<KeyIssued, Error> {
        let mut body =
            serde_json::json!({ "name": name, "role": role, "connect_code": connect_code });
        if let Some(expiry) = expiry {
            body["expiry"] = serde_json::json!(expiry);
        }
        let (mut issued, replayed): (KeyIssued, bool) = self
            .exchange(
                Method::POST,
                Some(call),
                "/v2/keys".to_owned(),
                Some(idempotency_key),
                Some(&body),
            )
            .await?;
        issued.replayed = replayed;
        Ok(issued)
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
        let (mut code, replayed): (ConnectCode, bool) = self
            .exchange(
                Method::POST,
                Some(call),
                format!("/v2/keys/{}/connect-codes", encode(key)),
                Some(idempotency_key),
                None,
            )
            .await?;
        code.replayed = replayed;
        Ok(code)
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

    /// `GET /v1/memories/{artifact_id}`: one memory with its text, as the
    /// caller may read it.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn get_memory(&self, call: &Call<'_>, artifact_id: &str) -> Result<Memory, Error> {
        self.get(Some(call), format!("/v1/memories/{}", encode(artifact_id)))
            .await
    }

    /// `GET /v2/admin/memories/{artifact_id}`: one memory of the tenant
    /// with its attribution, as the admin key reads it; journalled by the
    /// API as an admin read.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn admin_read(&self, call: &Call<'_>, artifact_id: &str) -> Result<AdminRead, Error> {
        self.get(
            Some(call),
            format!("/v2/admin/memories/{}", encode(artifact_id)),
        )
        .await
    }

    /// `POST /v1/memories` with `verb: learn`: a new claim deriving from
    /// `derives_from`, in `workarea`, held until reviewed.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn learn(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        submission: &Learn<'_>,
    ) -> Result<Accepted, Error> {
        let mut body = serde_json::json!({
            "verb": "learn",
            "body": submission.body,
            "workarea": submission.workarea,
            "derives_from": submission.derives_from,
        });
        if let Some(claim_key) = submission.claim_key {
            body["claim_key"] = Value::from(claim_key);
        }
        self.post(call, "/v1/memories".to_owned(), idempotency_key, &body)
            .await
    }

    /// `POST /v1/memories/{artifact_id}/supersede`: a new version of the
    /// claim, or with `retirement` its retirement, held until reviewed.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn supersede(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        artifact_id: &str,
        body: &str,
        expected_version: u32,
        retirement: bool,
    ) -> Result<Accepted, Error> {
        self.post(
            call,
            format!("/v1/memories/{}/supersede", encode(artifact_id)),
            idempotency_key,
            &serde_json::json!({
                "body": body,
                "expected_version": expected_version,
                "retirement": retirement,
            }),
        )
        .await
    }

    /// `GET /v2/whoami`: the key's person block, with its usage and limits
    /// where the person is under limits (ADR-0070 d5). A page that needs the
    /// person asks for it here; [`Client::whoami`] leaves the block out.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn whoami_person(&self, call: &Call<'_>) -> Result<Onboarding, Error> {
        self.get(Some(call), "/v2/whoami".to_owned()).await
    }

    /// `GET /v2/whoami?start=true`: the key's person with its onboarding
    /// status.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn whoami_start(&self, call: &Call<'_>) -> Result<Onboarding, Error> {
        self.get(Some(call), "/v2/whoami?start=true".to_owned())
            .await
    }

    /// `GET /v2/web/emails`: one page of the allowlist.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn list_web_emails(
        &self,
        call: &Call<'_>,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<WebEmailPage, Error> {
        let mut path = format!("/v2/web/emails?limit={limit}");
        if let Some(cursor) = cursor {
            path.push_str("&cursor=");
            path.push_str(&encode(cursor));
        }
        self.get(Some(call), path).await
    }

    /// `POST /v2/web/emails`: adds an entry or changes one; with
    /// `keep_an_admin` un-flagging the last admin is refused.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn allow_web_email(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        email: &str,
        user: &str,
        admin: bool,
    ) -> Result<WebEmail, Error> {
        self.post(
            call,
            "/v2/web/emails".to_owned(),
            idempotency_key,
            &serde_json::json!({
                "email": email, "user": user, "admin": admin, "keep_an_admin": true,
            }),
        )
        .await
    }

    /// `POST /v2/web/emails/remove`, refusing the last admin entry.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn remove_web_email(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        email: &str,
    ) -> Result<Removed, Error> {
        self.post(
            call,
            "/v2/web/emails/remove".to_owned(),
            idempotency_key,
            &serde_json::json!({ "email": email, "keep_an_admin": true }),
        )
        .await
    }

    /// `POST /v2/web/users/{user}/capacities`: review or curation on
    /// `workarea`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn grant_web_capacity(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        user: &str,
        capacity: &str,
        workarea: &str,
    ) -> Result<CapacityGranted, Error> {
        self.post(
            call,
            format!("/v2/web/users/{}/capacities", encode(user)),
            idempotency_key,
            &serde_json::json!({ "capacity": capacity, "workareas": [workarea] }),
        )
        .await
    }

    /// `GET /v2/web/users`: the people with their quotas, and the pending
    /// deletion requests.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn list_web_users(&self, call: &Call<'_>) -> Result<WebUsers, Error> {
        self.get(Some(call), "/v2/web/users".to_owned()).await
    }

    /// `POST /v2/web/users/{user}/quota`: one quota, `None` its default.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn set_web_user_quota(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        user: &str,
        quota: &str,
        limit: Option<u64>,
    ) -> Result<WebUser, Error> {
        let mut body = serde_json::Map::new();
        body.insert(quota.to_owned(), limit.map_or(Value::Null, Value::from));
        self.post(
            call,
            format!("/v2/web/users/{}/quota", encode(user)),
            idempotency_key,
            &Value::Object(body),
        )
        .await
    }

    /// `POST /v2/web/users/{user}/disable`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn disable_web_user(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        user: &str,
    ) -> Result<Disabled, Error> {
        self.post(
            call,
            format!("/v2/web/users/{}/disable", encode(user)),
            idempotency_key,
            &serde_json::json!({}),
        )
        .await
    }

    /// `POST /v2/web/users/{user}/deletion/close` with outcome `deleted`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn close_web_deletion(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        user: &str,
    ) -> Result<Closed, Error> {
        self.post(
            call,
            format!("/v2/web/users/{}/deletion/close", encode(user)),
            idempotency_key,
            &serde_json::json!({ "outcome": "deleted" }),
        )
        .await
    }

    /// `GET /v2/web/export`: everything the person holds.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn export_web_person(&self, call: &Call<'_>) -> Result<Export, Error> {
        self.get(Some(call), "/v2/web/export".to_owned()).await
    }

    /// `POST /v2/web/deletion-requests`: the person asks for deletion.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn request_web_deletion(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
    ) -> Result<DeletionRequested, Error> {
        self.send(
            Method::POST,
            Some(call),
            "/v2/web/deletion-requests".to_owned(),
            Some(idempotency_key),
            None,
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

    /// Answers every request `201` with `body`, and `Idempotency-Replay:
    /// true` when `replay` is set.
    struct Replaying {
        replay: bool,
        body: &'static str,
    }

    impl Transport for Replaying {
        fn send(&self, _: Request<Vec<u8>>) -> Sending<'_> {
            let mut response = Response::builder().status(201);
            if self.replay {
                response = response.header(IDEMPOTENCY_REPLAY, "true");
            }
            let response = response.body(self.body.as_bytes().to_vec()).unwrap();
            Box::pin(async move { Ok(response) })
        }
    }

    #[tokio::test]
    async fn an_issue_or_a_connect_code_is_a_replay_only_where_the_answer_says_so_in_its_header() {
        const BODY: &str = r#"{"key_id":"k9","name":"alice-laptop","code_id":"c1","expires_at":"2026-11-11T10:20:00Z","connector_url":"https://m/mcp"}"#;
        for replay in [true, false] {
            let client = Client::new(Replaying { replay, body: BODY });
            let issued = client
                .issue_key(&CALL, "idem", "alice-laptop", "writer", false, None)
                .await
                .unwrap();
            assert_eq!(issued.replayed(), replay);
            assert!(issued.value.is_none());
            let code = client
                .mint_connect_code(&CALL, "idem", "alice-phone")
                .await
                .unwrap();
            assert_eq!(code.replayed(), replay);
        }
    }

    /// An issue sends `expiry` only where one is given: a key issued
    /// without a date carries no such member, as before (ADR-0079 d1).
    #[tokio::test]
    async fn an_issue_sends_the_expiry_it_is_given_and_none_otherwise() {
        const BODY: &str = r#"{"key_id":"k9","name":"alice-laptop","value":"oml_v"}"#;
        let (client, seen) = recording(201, BODY);
        for expiry in [None, Some("2026-12-01T23:59:59Z")] {
            client
                .issue_key(&CALL, "idem", "alice-laptop", "writer", false, expiry)
                .await
                .unwrap();
        }
        let sent: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|request| serde_json::from_slice(request.body()).unwrap())
            .collect();
        assert_eq!(
            sent,
            [
                serde_json::json!({"name": "alice-laptop", "role": "writer", "connect_code": false}),
                serde_json::json!({
                    "name": "alice-laptop",
                    "role": "writer",
                    "connect_code": false,
                    "expiry": "2026-12-01T23:59:59Z",
                }),
            ]
        );
    }

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
            decodes(
                &client
                    .list_memories(
                        &CALL,
                        &MemoryQuery {
                            state: Some("held"),
                            limit: 50,
                            ..MemoryQuery::default()
                        },
                    )
                    .await,
            ),
            decodes(
                &client
                    .recall(
                        &CALL,
                        &RecallQuery {
                            query: "kestrel",
                            k: 50,
                            ..RecallQuery::default()
                        },
                    )
                    .await,
            ),
            decodes(
                &client
                    .admin_search(
                        &CALL,
                        &AdminSearch {
                            author: Some("alice-phone"),
                            limit: 50,
                            ..AdminSearch::default()
                        },
                    )
                    .await,
            ),
            decodes(&client.workarea_counts(&CALL, "alice", true).await),
            decodes(&client.list_keys(&CALL, Some("alice"), 200, None).await),
            decodes(
                &client
                    .create_workarea(&CALL, "idem-4", "proj", Some("alice"))
                    .await,
            ),
            {
                // An `Option`: `None` is the `204` of nothing waiting.
                let _ = client.review_next(&CALL, "idem-0").await;
                "Serve"
            },
            decodes(
                &client
                    .post_verdict(&CALL, "idem-5", "srv_1", "approve", None)
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
                    .post_bulk_verdict(&CALL, "idem-6", "approve", &["a"], None)
                    .await,
            ),
            decodes(&client.review_queue(&CALL).await),
            decodes(
                &client
                    .issue_key(&CALL, "idem-7", "alice-laptop", "writer", false, None)
                    .await,
            ),
            decodes(
                &client
                    .mint_connect_code(&CALL, "idem-8", "alice-phone")
                    .await,
            ),
            decodes(
                &client
                    .add_denial(&CALL, "idem-9", "alice-phone", "universe")
                    .await,
            ),
            decodes(&client.get_memory(&CALL, "m-1").await),
            decodes(&client.admin_read(&CALL, "m-1").await),
            decodes(
                &client
                    .learn(
                        &CALL,
                        "idem-10",
                        &Learn {
                            body: "Keys rotate quarterly.",
                            workarea: "universe",
                            derives_from: &["m-1"],
                            claim_key: None,
                        },
                    )
                    .await,
            ),
            decodes(
                &client
                    .supersede(&CALL, "idem-11", "m-1", "new text", 1, false)
                    .await,
            ),
            decodes(&client.whoami_start(&CALL).await),
            decodes(&client.list_web_emails(&CALL, 200, None).await),
            decodes(
                &client
                    .allow_web_email(&CALL, "idem-12", "a@example.com", "alice", false)
                    .await,
            ),
            decodes(
                &client
                    .remove_web_email(&CALL, "idem-13", "a@example.com")
                    .await,
            ),
            decodes(
                &client
                    .grant_web_capacity(&CALL, "idem-14", "alice", "review", "alice")
                    .await,
            ),
            decodes(&client.list_web_users(&CALL).await),
            decodes(
                &client
                    .set_web_user_quota(&CALL, "idem-15", "alice", "keys", Some(5))
                    .await,
            ),
            decodes(&client.disable_web_user(&CALL, "idem-16", "alice").await),
            decodes(&client.close_web_deletion(&CALL, "idem-17", "alice").await),
            decodes(&client.export_web_person(&CALL).await),
            decodes(&client.request_web_deletion(&CALL, "idem-18").await),
            decodes(&client.whoami_person(&CALL).await),
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
        // The record alone, for a caller that needs no person block.
        assert_eq!(seen[1].uri(), "/v2/whoami?person=false");
        // The person block is asked for by name, on the one method that reads it.
        assert_eq!(seen[seen.len() - 1].uri(), "/v2/whoami");
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
        assert_eq!(
            seen[10].uri(),
            "/v1/memories?with_own=true&include_text=true&limit=50&state=held"
        );
        // The reads that travel in a body carry no idempotency key.
        for i in [11, 12] {
            assert!(!seen[i].headers().contains_key(IDEMPOTENCY_KEY), "{i}");
        }
        assert_eq!(
            seen[13].uri(),
            "/v2/workareas/alice/tree?counts=true&offers=true"
        );
        assert_eq!(seen[15].headers()[IDEMPOTENCY_KEY], "idem-4");
        assert_eq!(
            std::str::from_utf8(seen[15].body()).unwrap(),
            r#"{"name":"proj","parent":"alice"}"#
        );
        assert_eq!(seen[14].uri(), "/v2/keys?limit=200&user=alice");
        assert_eq!(seen[18].uri(), "/v2/review/held?limit=50&workarea=alice");
        assert_eq!(seen[19].uri(), "/v2/review/held?ids=a,b");
        assert_eq!(
            std::str::from_utf8(seen[22].body()).unwrap(),
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
                replayed: false,
            }),
            replayed: false,
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
