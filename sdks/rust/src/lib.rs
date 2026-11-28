//! `oml-sdk`: a hand-written, typed client of OML's public HTTP API
//! (ADR-0070 d9). It covers the operations its consumers call, one method
//! each, and nothing else; [`OPERATIONS`] names them, and
//! `scripts/check_contract_sync.py` fails when an entry's operation id,
//! method, path or field names differ from `docs/api/openapi.yaml`, so a
//! contract change that forgets this client fails where the generated
//! Python and TypeScript layers already do. Its one consumer today is
//! `oml-web`; the skill set ([`Client::skills`]) and a recall's kinds and
//! document passages ([`RecallQuery::kinds`], [`RecallHit::passage`]) are
//! there for any Rust caller, as the Python and TypeScript SDKs serve them
//! (#427).
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
pub const OPERATIONS: [&str; 52] = [
    "authorizationServerMetadata GET /.well-known/oauth-authorization-server resp:AuthorizationServerMetadata",
    "whoami GET /v2/whoami query:person resp:Whoami",
    "getGuardrails GET /v1/guardrails query:workspace resp:GuardrailSet",
    "getWorkspaceTree GET /v2/workspaces/{workspace}/tree query:limit,cursor resp:WorkspacePage",
    "readyz GET /readyz resp:Ready",
    "getWorkspace GET /v2/workspaces/{workspace} resp:WorkspaceView",
    "getWorkspaceOffers GET /v2/workspaces/{workspace}/offers resp:WorkspaceOffers",
    "proposeTransition POST /v2/workspaces/{workspace}/transitions body:to resp:Transition",
    "ratifyTransition POST /v2/workspaces/{workspace}/transitions/{transition_id}/ratify resp:Transition",
    "setWorkspaceMode POST /v2/workspaces/{workspace}/mode body:mode resp:WorkspaceView",
    "listMemories GET /v1/memories query:with_own,mine,include_text,author,workspace,state,kinds,promoted_since,origin_kinds,limit,cursor resp:MemoryPage",
    "recallMemories POST /v1/memories/recall body:query,k,workspace,kinds,include_text,include_unreviewed,origin_kinds resp:RecallResponse",
    "adminSearchMemories POST /v2/admin/memories/search body:query,workspace,state,origin_kind,author,kinds,limit,cursor resp:AdminMemoryPage",
    "getWorkspaceTree GET /v2/workspaces/{workspace}/tree query:counts,offers resp:WorkspaceCounts",
    "listKeys GET /v2/keys query:user,limit,cursor resp:KeyPage",
    "createWorkspace POST /v2/workspaces body:name,parent resp:Workspace",
    "reviewNext POST /v2/review/next resp:Serve",
    "postVerdict POST /v2/review/verdicts body:serve_id,verdict,reason resp:VerdictResult",
    "reviewHeld GET /v2/review/held query:workspace,author_key,claim_key_prefix,limit,cursor resp:HeldPage",
    "reviewHeld GET /v2/review/held query:ids resp:HeldNamed",
    "postBulkVerdict POST /v2/review/verdicts/bulk body:verdict,artifact_ids,reason resp:BulkVerdictResult",
    "reviewQueue GET /v2/review/queue resp:ReviewQueue",
    "todoCounts GET /v2/todo/counts query:workspace resp:TodoCounts",
    "adminTodoCounts GET /v2/admin/todo/counts resp:AdminTodoCounts",
    "listAdminActs GET /v2/admin/acts query:state resp:AdminActList",
    "confirmAdminAct POST /v2/admin/acts/{act}/confirm resp:AdminAct",
    "declineAdminAct POST /v2/admin/acts/{act}/decline body:reason resp:AdminAct",
    "listNotices GET /v2/web/notices resp:NoticeList",
    "dismissNotice POST /v2/web/notices/{notice}/dismiss resp:Notice",
    "issueKey POST /v2/keys body:name,role,connect_code,expiry resp:KeyIssued",
    "mintConnectCode POST /v2/keys/{key}/connect-codes resp:ConnectCode",
    "addDenial POST /v2/keys/{key}/denials body:workspace resp:Denial",
    "getMemory GET /v1/memories/{artifact_id} resp:Memory",
    "adminReadMemory GET /v2/admin/memories/{artifact_id} resp:AdminRead",
    "submitMemory POST /v1/memories body:verb,claim_key,body,workspace,derives_from resp:Accepted",
    "supersedeMemory POST /v1/memories/{artifact_id}/supersede body:body,expected_version,retirement resp:Accepted",
    "previewMemory POST /v1/memories/preview body:kind,workspace,body,format,skill,document,severity,rule,replaces,expected_version,retirement resp:MemoryPreview",
    "withdrawMemory POST /v1/memories/{artifact_id}/withdraw body:reason resp:Withdrawn",
    "whoami GET /v2/whoami query:start resp:Onboarding",
    "listWebEmails GET /v2/web/emails query:limit,cursor resp:WebEmailPage",
    "allowWebEmail POST /v2/web/emails body:email,user,admin,keep_an_admin resp:WebEmail",
    "removeWebEmail POST /v2/web/emails/remove body:email,keep_an_admin resp:Removed",
    "grantWebCapacities POST /v2/web/users/{user}/capacities body:capacity,workspaces,reason resp:CapacityGranted",
    "listWebUsers GET /v2/web/users resp:WebUsers",
    "setWebUserQuota POST /v2/web/users/{user}/quota body:memories,keys,connections,requests_per_minute resp:WebUser",
    "disableWebUser POST /v2/web/users/{user}/disable resp:Disabled",
    "closeWebDeletion POST /v2/web/users/{user}/deletion/close body:outcome resp:Closed",
    "exportWebPerson GET /v2/web/export resp:Export",
    "requestWebDeletion POST /v2/web/deletion-requests resp:DeletionRequested",
    "whoami GET /v2/whoami resp:Onboarding",
    "listMemories GET /v1/memories query:workspace,kinds,prefix,state,limit,cursor resp:CatalogPage",
    "getSkills GET /v1/skills query:workspace,include_own_held,cursor resp:SkillSet",
];

/// The request id header (ADR-0042 d4).
pub const REQUEST_ID: &str = "x-request-id";
/// The correlation id header (ADR-0042 d4).
pub const CORRELATION_ID: &str = "x-correlation-id";
/// The tenant key a console request acts as, beside a capacity credential
/// (ADR-0085 d16).
pub const ACTING_KEY: &str = "oml-acting-key";
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
    /// The id of a tenant key linked to the session's person, which the
    /// request acts as; sent only beside a capacity credential (ADR-0085
    /// d16).
    pub acting_key: Option<&'a str>,
}

/// No debug output prints the bearer value.
impl std::fmt::Debug for Call<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Call")
            .field("bearer", &"<redacted>")
            .field("request_id", &self.request_id)
            .field("correlation_id", &self.correlation_id)
            .field("acting_key", &self.acting_key)
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
    /// workspace, which refuses it issuing a key, minting a connect code and
    /// cutting off a key for good; absent for every other key (ADR-0070 d2,
    /// d5).
    #[serde(default)]
    pub denied: Option<bool>,
}

/// One verdict on a guardrail.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GuardrailVerdict {
    pub used_by: String,
    pub at: String,
}

/// One guardrail in force (openapi.yaml `ServedGuardrail`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServedGuardrail {
    pub claim_key: String,
    pub version: i64,
    /// `None` where the node's path is not served.
    pub workspace_path: Option<String>,
    pub severity: String,
    pub text: String,
    #[serde(default)]
    pub rule: Option<Value>,
    /// `None` on a held version served as unreviewed (ADR-0080 d10).
    #[serde(default)]
    pub promoted_at: Option<String>,
    /// True for a held version served beside the promoted rules.
    #[serde(default)]
    pub unreviewed: bool,
    /// What a hook does inside: `ask` for a held version (ADR-0080 d10).
    #[serde(default)]
    pub effect_inside: Option<String>,
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

/// One node of a workspace tree page.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspaceNode {
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
    /// The node's `public` flag (ADR-0098 d9); not public when a server
    /// before the flag serves none.
    #[serde(default)]
    pub public: PublicFlag,
}

/// A workspace's `public` flag (openapi.yaml `PublicFlag`; ADR-0098 d9):
/// whether every reading key of the tenant reads the node, since when, by
/// which admin act, whether that act took effect under `sole_admin`, and,
/// to an admin key on the workspace read, a set that waits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PublicFlag {
    #[serde(default)]
    pub value: bool,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub act_id: Option<String>,
    #[serde(default)]
    pub sole_admin: bool,
    #[serde(default)]
    pub pending_act_id: Option<String>,
}

/// `POST /v2/workspaces/{workspace}/public`'s answer: `effective`, or
/// `pending_confirmation` while a set waits on a second admin person
/// (ADR-0098 d9).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PublicOutcome {
    pub workspace: String,
    #[serde(default)]
    pub public: bool,
    pub act_id: String,
    pub state: String,
}

/// `GET /v2/workspaces/{workspace}/tree`: one page of nodes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspacePage {
    pub items: Vec<WorkspaceNode>,
    pub next_cursor: Option<String>,
}

/// One status transition (openapi.yaml `Transition`), the fields read here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Transition {
    pub transition_id: String,
    pub from: String,
    pub to: String,
    pub state: String,
    /// The instant of its `workspace.status_proposed`; absent from an API
    /// before v0.8.5 (ADR-0095 d5 as amended 2026-11-24).
    #[serde(default)]
    pub proposed_at: Option<String>,
    /// The proposing key, served to a key of the proposer's person and null
    /// to every other reader (ADR-0095, amendment of 2026-11-24).
    #[serde(default)]
    pub proposed_by: Option<ProposedBy>,
}

/// The key that made a live proposal, as `proposed_by` serves it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProposedBy {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// A workspace's ratified brief as the calling key reads it (openapi.yaml
/// `Brief`; ADR-0046 d8): its members in ratification order, cut by the
/// budget, and the budget itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Brief {
    pub items: Vec<Memory>,
    #[serde(default)]
    pub budget: Option<BriefBudget>,
}

/// A brief's budget (openapi.yaml `RecallBudget`), the field read here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct BriefBudget {
    #[serde(default)]
    pub max_tokens: Option<u64>,
}

/// One brief edge as `GET /v2/edges?kind=brief` lists it (openapi.yaml
/// `BriefEdge`); a pending one carries `proposed_at` and `proposed_by`
/// from v0.8.5 (ADR-0095 d6 as amended 2026-11-24).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BriefEdge {
    pub edge_id: String,
    pub from_artifact_id: String,
    pub to_workspace_id: String,
    pub state: String,
    pub proposer_key_id: String,
    #[serde(default)]
    pub proposed_at: Option<String>,
    #[serde(default)]
    pub proposed_by: Option<ProposedBy>,
}

/// The most pages of brief edges [`Client::brief_edges`] reads in one call:
/// 2,000 edges, past any brief's budget.
pub const BRIEF_EDGE_PAGES: usize = 10;

/// `GET /v2/edges?kind=brief`: a workspace's brief edges, as one page or as
/// [`Client::brief_edges`] gathers them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BriefEdgePage {
    pub items: Vec<BriefEdge>,
    pub next_cursor: Option<String>,
}

/// `GET /v2/workspaces/{workspace}` (openapi.yaml `WorkspaceView`), the fields
/// read here: the node and its live proposals.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspaceView {
    pub id: String,
    pub name: String,
    pub path: String,
    pub status: String,
    /// `in_progress` where a person put the node in progress, else
    /// `standard` (ADR-0030 d1); `None` from an API that does not serve it.
    #[serde(default)]
    pub mode: Option<String>,
    /// Whether the node is a person's root (ADR-0044 d1).
    #[serde(default)]
    pub user_root: bool,
    /// The verbs the calling key may use here; the builder offers New where
    /// they hold `remember` (ADR-0082 d2, d11).
    #[serde(default)]
    pub my_effective_verbs: Vec<String>,
    #[serde(default)]
    pub pending_transitions: Vec<Transition>,
    /// The review policy in force (ADR-0009 d3); `None` from an API that
    /// does not serve it.
    #[serde(default)]
    pub policy: Option<ReviewPolicy>,
    /// The approved claims on the node alone by kind, only kinds with one or
    /// more; `None` from an API that does not serve it, where the builder's
    /// kind links carry no number (ADR-0082 d8 item 7, d10).
    #[serde(default)]
    pub approved_by_kind: Option<std::collections::BTreeMap<String, u64>>,
    /// The ratified brief as the calling key reads it; `None` from an API
    /// that does not serve it (ADR-0046 d8).
    #[serde(default)]
    pub brief: Option<Brief>,
    /// The node's `public` flag, with a set that waits to an admin key
    /// (ADR-0098 d9); not public from an API before v0.8.6.
    #[serde(default)]
    pub public: PublicFlag,
    /// The node's children the calling key reads; the confirm screen of a
    /// public set counts them, since none of them becomes public (ADR-0098
    /// d5, d9).
    #[serde(default)]
    pub children: Vec<Value>,
}

/// A workspace's review policy (openapi.yaml `ReviewPolicy`): how many
/// approving verdicts promote a version, and from which roles.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReviewPolicy {
    #[serde(default)]
    pub k: Option<u32>,
    #[serde(default)]
    pub required_roles: Vec<String>,
}

/// A transition a workspace's offers name: `propose` the next rung, or
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

/// Putting the workspace in progress, where offered.
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

/// `GET /v2/workspaces/{workspace}/offers` (openapi.yaml `WorkspaceOffers`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspaceOffers {
    pub workspace_id: String,
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
    /// The workspace's id; `None` where the key holds no role on it.
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub workspace_path: Option<String>,
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
    /// A skill's name and description, on kind `skill` alone, except a
    /// retirement (ADR-0080 d1); the console's change compares them
    /// (ADR-0082 d6).
    #[serde(default)]
    pub skill: Option<SkillFields>,
    /// A guardrail's severity, `never`, `must` or `should`, on kind
    /// `guardrail` alone, except a retirement (ADR-0037 d2).
    #[serde(default)]
    pub severity: Option<String>,
    /// A guardrail's `oml-gr/1` rule, or `None` where it has none (ADR-0037
    /// d8).
    #[serde(default)]
    pub rule: Option<Value>,
    /// A document's title, on kind `document` alone, except a retirement
    /// (ADR-0081 d1).
    #[serde(default)]
    pub document: Option<DocumentFields>,
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
    /// author's inside an in-progress workspace (ADR-0030 d3, d4).
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
/// reviewing key's attestation, `agent`, `person` or `service`, and whether
/// that key acts for the author's person.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Approver {
    pub used_by: String,
    pub same_user: bool,
    /// `verdict`, or `admin_fix` for a version served because of an
    /// admin's fix (ADR-0089 d7); absent from a server before v0.8.4.
    #[serde(default)]
    pub act: Option<String>,
    /// Whether a person confirmed the fix; null for a verdict (ADR-0089 d7).
    #[serde(default)]
    pub confirmed: Option<bool>,
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

/// `POST /v1/memories/preview`'s answer (openapi.yaml `MemoryPreview`;
/// ADR-0082 d5, d10): what capture would decide, never a capture.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MemoryPreview {
    pub would_capture: bool,
    /// The refusal capture would answer, in the error object's shape.
    #[serde(default)]
    pub refusal: Option<PreviewRefusal>,
    #[serde(default)]
    pub findings: Vec<ScanFinding>,
    pub needs_person: bool,
    #[serde(default)]
    pub body_bytes: Option<u64>,
    #[serde(default)]
    pub budget_tokens: Option<u64>,
    /// A guardrail's rule in its canonical `oml-gr/1` form.
    #[serde(default)]
    pub rule: Option<Value>,
    /// A skill's prompt name.
    #[serde(default)]
    pub offered_as: Option<String>,
    #[serde(default)]
    pub shadows: Vec<SkillShadow>,
    #[serde(default)]
    pub set_after: Option<SetAfter>,
    /// A document's passages as capture would cut them (ADR-0081 d3);
    /// empty for any other kind and from an API that does not serve them.
    #[serde(default)]
    pub passages: Vec<PreviewPassage>,
}

/// One passage of a previewed document: its fragment, its place and its
/// lines, 1-based and inclusive (ADR-0081 d3; ADR-0082 d5).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PreviewPassage {
    pub fragment: String,
    pub ordinal: u32,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(default)]
    pub heading_path: Vec<String>,
}

/// The refusal a preview answers inside its body (ADR-0082 d10).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PreviewRefusal {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Option<Value>,
}

/// A skill's own fields, as a version carries them (openapi.yaml
/// `SkillFields`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SkillFields {
    pub name: String,
    pub description: String,
}

/// A document's own fields, as a version carries them (openapi.yaml
/// `DocumentFields`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DocumentFields {
    #[serde(default)]
    pub title: Option<String>,
}

/// A same-named skill along a previewed skill's chain (ADR-0080 d5).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SkillShadow {
    pub workspace: String,
    /// `shadows` or `shadowed_by`.
    pub relation: String,
}

/// A guardrail's set size after the previewed version, against the cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct SetAfter {
    pub size: u64,
    pub cap: u64,
}

/// A draft the builder previews: a new item, or with `replaces` an edit or
/// a retirement of the version it names at the version the form rendered
/// (ADR-0082 d4, d5).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft<'a> {
    pub kind: &'a str,
    pub workspace: Option<&'a str>,
    pub body: &'a str,
    pub format: Option<&'a str>,
    /// A skill's name and description.
    pub skill: Option<(&'a str, &'a str)>,
    /// A document's title.
    pub document_title: Option<&'a str>,
    /// A document's claim key, sent on a new item alone (ADR-0081 d2).
    pub claim_key: Option<&'a str>,
    pub severity: Option<&'a str>,
    pub rule: Option<&'a Value>,
    /// The version replaced and the version the form rendered.
    pub replaces: Option<(&'a str, u32)>,
    pub retirement: bool,
}

impl Draft<'_> {
    /// The body submit, supersede and preview share: the kind and its
    /// fields, and for a supersession the version the form rendered and
    /// whether it retires; `replaces` itself is preview's alone.
    fn body(&self) -> Value {
        let mut body = serde_json::json!({ "kind": self.kind, "body": self.body });
        if let Some(workspace) = self.workspace {
            body["workspace"] = Value::from(workspace);
        }
        if let Some(format) = self.format {
            body["format"] = Value::from(format);
        }
        if let Some((name, description)) = self.skill {
            body["skill"] = serde_json::json!({ "name": name, "description": description });
        }
        if let Some(title) = self.document_title {
            body["document"] = serde_json::json!({ "title": title });
        }
        if let Some(claim_key) = self.claim_key {
            body["claim_key"] = Value::from(claim_key);
        }
        if let Some(severity) = self.severity {
            body["severity"] = Value::from(severity);
        }
        if let Some(rule) = self.rule {
            body["rule"] = rule.clone();
        }
        if let Some((_, expected_version)) = self.replaces {
            body["expected_version"] = Value::from(expected_version);
            body["retirement"] = Value::from(self.retirement);
        }
        body
    }
}

/// `GET /v1/memories`: one page of memories.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MemoryPage {
    pub items: Vec<Memory>,
    pub next_cursor: Option<String>,
}

/// One kind's count on the catalog's first page (openapi.yaml
/// `CatalogCount`; ADR-0092 d6): exact up to `catalog.count_cap`, a lower
/// bound above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum CatalogCount {
    Exact(u64),
    AtLeast { at_least: u64 },
}

/// One catalog entry (openapi.yaml `Memory` as the catalog serves it,
/// without text; ADR-0092 d6): the fields every kind carries and the
/// projection of its own kind, the fields the Catalog page reads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CatalogEntry {
    pub artifact_id: String,
    pub claim_key: String,
    pub kind: String,
    pub version: u32,
    pub workspace_path: Option<String>,
    pub state: String,
    #[serde(default)]
    pub unreviewed: bool,
    #[serde(default)]
    pub needs_person: bool,
    #[serde(default)]
    pub retirement: bool,
    pub created_at: String,
    #[serde(default)]
    pub approved_at: Option<String>,
    #[serde(default)]
    pub approver: Option<Approver>,
    /// A skill's name.
    #[serde(default)]
    pub name: Option<String>,
    /// A document's title.
    #[serde(default)]
    pub title: Option<String>,
    /// A skill's description, or the server's cut of a memory's or a
    /// derived fact's text where `preview` is true.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub preview: bool,
    /// The command a skill is offered under at the read's scope node;
    /// `None` where the scope does not offer it.
    #[serde(default)]
    pub prompt: Option<String>,
    /// A guardrail's severity word: `never`, `must` or `should`.
    #[serde(default)]
    pub severity: Option<String>,
    /// A guardrail's `oml-gr/1` rule, or `None` where it has none.
    #[serde(default)]
    pub rule: Option<Value>,
    /// A guardrail: whether it binds at the read's scope node.
    #[serde(default)]
    pub in_force: Option<bool>,
    /// A steering entry: whether it is in force at the read's scope node.
    #[serde(default)]
    pub applies_here: Option<bool>,
    /// A document's body bytes.
    #[serde(default)]
    pub size: Option<u64>,
}

/// `GET /v1/memories` without `own`: one page of the catalog, with each
/// kind's count on the first page only (ADR-0092 d6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CatalogPage {
    pub items: Vec<CatalogEntry>,
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub counts: Option<std::collections::BTreeMap<String, CatalogCount>>,
}

/// One ranked hit of a recall.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecallHit {
    pub memory: Memory,
    /// A document's hit only: the passage it serves, whose lines are the
    /// memory's text (ADR-0081 d4).
    #[serde(default)]
    pub passage: Option<RecallPassage>,
    /// A document's hit only: how many of its other passages were in the
    /// fused list before the `k` cut, a lower bound (ADR-0081 d4).
    #[serde(default)]
    pub more_passages: Option<u32>,
    /// A document's hit only: the passage's MCP resource,
    /// `oml://workspace/<workspace>/doc/<path>?passage=<fragment>`
    /// (ADR-0081 d6).
    #[serde(default)]
    pub uri: Option<String>,
}

/// The passage a document's recall hit serves (openapi.yaml
/// `RecallPassage`; ADR-0081 d3, d4).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecallPassage {
    /// The heading's slug for a whole section, else `L<n>-L<m>`.
    pub fragment: String,
    /// Its place in the document, from 0.
    pub ordinal: u32,
    pub start_line: u32,
    pub end_line: u32,
    /// The headings of level 1 to 4 above it, outermost first.
    pub heading_path: Vec<String>,
}

/// `GET /v1/skills`: one page of the skills offered at the scope node,
/// without bodies (openapi.yaml `SkillSet`; ADR-0080 d11).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SkillSet {
    pub scope_node: String,
    pub scope_path: String,
    pub guardrail_scope: String,
    /// Opaque, the same on every page of one set: a caller only compares it.
    pub set_digest: String,
    /// Ordered by depth from the scope node, then name.
    pub skills: Vec<ServedSkill>,
    pub next_cursor: Option<String>,
}

/// One offered skill (openapi.yaml `ServedSkill`): its body is
/// [`Client::get_memory`] of its `artifact_id`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServedSkill {
    pub artifact_id: String,
    pub claim_key: String,
    /// The command: the skill's name, or `<name>-unreviewed` for a held edit
    /// of an approved skill (ADR-0080 d10).
    pub name: String,
    pub description: String,
    pub version: u32,
    pub workspace_path: String,
    /// The tree distance from the scope node, up or down.
    pub depth: u32,
    /// The farther workspaces' paths whose skill of this name it shadows,
    /// nearest first.
    pub overrides: Vec<String>,
    /// `None` for a held version.
    pub approved_at: Option<String>,
    pub approver: Option<Approver>,
    /// The skill's MCP resource, `oml://workspace/<workspace>/skill/<name>`.
    pub uri: String,
    /// A held version, offered to its author key or inside an in-progress
    /// workspace (ADR-0080 d10, d14).
    pub unreviewed: bool,
}

/// `POST /v1/memories/recall`: steering first, then the ranked hits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecallResponse {
    /// The steering list, `Some` and possibly empty, "none in force"; `None`
    /// when the recall's `kinds` does not name steering, which the server
    /// answers as `"not_requested"`, so an unasked list is never read as
    /// none in force (ADR-0092 d2).
    #[serde(deserialize_with = "steering_list")]
    pub steering: Option<Vec<Memory>>,
    pub hits: Vec<RecallHit>,
}

/// A recall's `steering`: the list, or `None` for `"not_requested"`.
fn steering_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<Memory>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Steering {
        List(Vec<Memory>),
        NotRequested(String),
    }
    match Steering::deserialize(d)? {
        Steering::List(items) => Ok(Some(items)),
        Steering::NotRequested(s) if s == "not_requested" => Ok(None),
        Steering::NotRequested(other) => Err(serde::de::Error::custom(format!(
            "steering is a list or \"not_requested\", not {other:?}"
        ))),
    }
}

/// A memory as the admin read serves it (openapi.yaml `AdminMemory`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminMemory {
    pub artifact_id: String,
    pub claim_key: String,
    pub version: u32,
    pub state: String,
    pub workspace_path: String,
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
    pub used_by: Option<String>,
    #[serde(default)]
    pub promoted_at: Option<String>,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub scan_finding: Option<String>,
}

/// An admin key's investigation (ADR-0089 d10, d15).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Investigation {
    pub investigation_id: String,
    pub reason: String,
    pub opened_at: String,
    pub reads: u64,
    pub age_seconds: i64,
    pub state: String,
    #[serde(default)]
    pub closed_at: Option<String>,
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
    pub workspace_path: String,
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
    /// The authoring key's holder, `agent`, `person` or `service`; absent,
    /// with the two fields after it, from an API before v0.7.0.
    #[serde(default)]
    pub used_by: Option<String>,
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

/// `POST /v1/memories/{artifact_id}/withdraw`: the held version its
/// author withdrew (ADR-0083 d13).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Withdrawn {
    pub artifact_id: String,
    pub state: String,
    pub at: String,
    pub journal_seq: u64,
}

/// What [`Client::learn`] submits: the text, the workspace, the memories
/// it derives from and an optional claim key.
#[derive(Debug, Clone, Copy)]
pub struct Learn<'a> {
    pub body: &'a str,
    pub workspace: &'a str,
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
    /// This month's recalls, a count with no limit; absent from an API
    /// before v0.8.5 (ADR-0096 d9).
    #[serde(default)]
    pub recalls: Option<u64>,
    pub keys: u64,
    pub connections: u64,
    /// Absent from an API before the `workspaces` meter (ADR-0095 d4).
    #[serde(default)]
    pub workspaces: Option<u64>,
}

/// A person's limits, each `None` where the quota is unlimited (the API
/// serves an unlimited quota as null, ADR-0071 d5); `text_bytes` and
/// `writes_per_month` are also `None` from an API before the usage meters,
/// and `workspaces` from one before its meter (ADR-0095 d4).
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
    #[serde(default)]
    pub workspaces: Option<u64>,
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
    /// The account's quota profile, `None` for none or from an API before
    /// v0.8.5 (ADR-0096 d9).
    #[serde(default)]
    pub profile: Option<String>,
    /// The UTC month the usage counts, `YYYY-MM`; absent from an API
    /// before v0.8.5 (ADR-0096 d9).
    #[serde(default)]
    pub period: Option<String>,
    pub deletion_requested_at: Option<String>,
    #[serde(default)]
    pub start: Option<StartStatus>,
    /// The live tenant keys an admin linked to the person; empty from an
    /// API before v0.8.0 (ADR-0085 d16).
    #[serde(default)]
    pub linked_keys: Vec<LinkedKey>,
    /// The live tenant keys an admin linked to the person that wait on
    /// their acceptance; empty from an API before v0.8.4 (ADR-0085 d16,
    /// the operator's decision of 2026-11-19).
    #[serde(default)]
    pub pending_links: Vec<LinkedKey>,
    /// The sign-in emails an admin added for the person that wait on their
    /// acceptance, each with its admin flag and the key that added it;
    /// empty from an API before v0.8.4 (ADR-0045 d2, the operator's
    /// decisions of 2026-11-19 and 2026-11-20).
    #[serde(default)]
    pub pending_emails: Vec<PendingEmail>,
}

/// A sign-in email that waits on its person: the address, whether it signs
/// in as an admin once accepted, and the admin key that added it, whose
/// name is `None` for a key issued before names (ADR-0045 d2, the
/// operator's decision of 2026-11-20).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PendingEmail {
    pub email: String,
    pub admin: bool,
    pub added_by: AddedBy,
}

/// The admin key that added a pending sign-in email, by id and name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AddedBy {
    pub id: String,
    pub name: Option<String>,
}

/// `POST /v2/keys/{key}/link`, `/unlink` and `/accept_link`: the key and
/// the user its link names, `None` once unlinked, and whether that user
/// has yet to accept it (ADR-0085 d16, the operator's decision of
/// 2026-11-19).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KeyLink {
    pub key_id: String,
    pub name: Option<String>,
    pub linked_user: Option<String>,
    #[serde(default)]
    pub pending: bool,
}

/// A tenant key linked to a person, by id and name (ADR-0085 d16).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LinkedKey {
    pub id: String,
    pub name: String,
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
    /// Whether the entry waits on its person's acceptance; false from an
    /// API before v0.8.4 (ADR-0045 d2, the operator's decision of
    /// 2026-11-19).
    #[serde(default)]
    pub pending: bool,
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
    /// The quota profile and the UTC month; absent from an API before
    /// v0.8.5 (ADR-0096 d9).
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub period: Option<String>,
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
    /// Whether every reading key of the tenant reads the node (ADR-0098 d9).
    #[serde(default)]
    pub public: bool,
}

/// `GET /v2/workspaces/{workspace}/tree?counts=true`: the subtree, a parent
/// before its children, with each node's counts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspaceCounts {
    pub items: Vec<CountedNode>,
    pub truncated: bool,
    /// The scope's offers, asked with `offers=true` (#286); absent when not
    /// asked. An API before v0.7.1 refuses `offers=true` as
    /// `request.validation`, so a caller that must work one release behind
    /// reads the offers apart on that refusal (ADR-0070 d4).
    #[serde(default)]
    pub offers: Option<WorkspaceOffers>,
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
    /// Whether only a person's verdict counts on this version, an
    /// instruction's (ADR-0080 d16).
    pub needs_person: bool,
    /// `instruction_needs_person` when `needs_person`; `None` otherwise.
    pub needs_person_reason: Option<String>,
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
    /// With `needs=person`, the lane's rows per author key (ADR-0074 d19);
    /// absent from an API before v0.8.0 and `None` otherwise.
    #[serde(default)]
    pub lane_authors: Option<Vec<LaneAuthor>>,
}

/// One author key's rows in the person lane (ADR-0074 d19).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LaneAuthor {
    pub author_key_id: String,
    pub held: u64,
}

/// One id named to `GET /v2/review/held?ids=`, with its row, or `None` when
/// it is no longer held or no longer the key's to judge.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NamedHeld {
    pub id: String,
    pub item: Option<Memory>,
    /// Whether only a person's verdict counts on the row: it, or the
    /// version it replaces, is an instruction (ADR-0080 d16); false where
    /// `item` is null.
    pub needs_person: bool,
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

/// One workspace holding held rows, as the admin's queue names it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct QueueItem {
    pub workspace_id: String,
    pub path: String,
    pub held: u64,
    pub oldest_at: String,
}

/// `GET /v2/review/queue`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReviewQueue {
    pub items: Vec<QueueItem>,
}

/// `GET /v2/todo/counts`: the To do page's built groups, each counted up
/// to 1,000 (ADR-0083 d11, d14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct TodoCounts {
    /// The To review group: the held rows the person's reviewer key may
    /// judge.
    pub review: u64,
    /// The Brief proposals group; 0 from an API before v0.8.5 (ADR-0096
    /// d7).
    #[serde(default)]
    pub briefs: u64,
    /// The Notices group: the person's notices not dismissed (ADR-0097
    /// d20); 0 from an API before v0.8.6.
    #[serde(default)]
    pub notices: u64,
    /// Whether a count passed 1,000.
    pub truncated: bool,
}

/// `GET /v2/web/notices`'s answer: the person's notices not dismissed,
/// oldest first (ADR-0097 d20).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NoticeList {
    pub items: Vec<Notice>,
}

/// One in-product notice to one person (openapi.yaml `Notice`; ADR-0097
/// d20): what it tells about, the act, the act's detail and when.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Notice {
    pub notice_id: String,
    pub kind: String,
    #[serde(default)]
    pub act_id: Option<String>,
    pub detail: Value,
    pub at: String,
    #[serde(default)]
    pub dismissed_at: Option<String>,
}

/// `GET /v2/admin/todo/counts`: the guardrail-loosening alerts, 0 from an
/// API before v0.8.5 (ADR-0096 d7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct AdminTodoCounts {
    #[serde(default)]
    pub alerts: u64,
    /// The admin acts the calling admin person may confirm (ADR-0097 d2,
    /// d10, d11); 0 from a server before v0.8.6.
    #[serde(default)]
    pub acts_to_confirm: u64,
    pub truncated: bool,
}

/// `GET /v2/admin/acts`'s answer.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AdminActList {
    pub items: Vec<AdminAct>,
}

/// One admin act (openapi.yaml `AdminAct`; ADR-0097 d2, d23): what the
/// confirm screen reads, the target's role, principal, `used_by` and the
/// root's person, and what a confirmation grants.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AdminAct {
    pub act_id: String,
    pub act: String,
    pub class: String,
    pub state: String,
    pub target: Value,
    #[serde(default)]
    pub grants: Vec<Value>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub actor_person: Option<String>,
    #[serde(default)]
    pub confirmed_by_person: Option<String>,
    /// The reason the confirmation gave (ADR-0097 d26).
    #[serde(default)]
    pub confirm_reason: Option<String>,
    pub sole_admin: bool,
    #[serde(default)]
    pub cause: Option<String>,
    pub at: String,
    #[serde(default)]
    pub settled_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    /// Whether the calling admin person may confirm or review the act, as
    /// the To do count judges it (ADR-0097 d11); served by the list alone.
    #[serde(default)]
    pub to_confirm: bool,
}

/// A grant as a listed key carries it: the role, the workspace's path and
/// its coverage, read as `subtree` from an API before v0.8.5 (ADR-0096 d8).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListedGrant {
    pub role: String,
    pub workspace_id: String,
    pub path: String,
    #[serde(default = "subtree")]
    pub covers: String,
}

fn subtree() -> String {
    "subtree".to_owned()
}

/// One grant of a key's full record (openapi.yaml `Grant`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KeyGrant {
    pub workspace_node_id: String,
    pub role: String,
    #[serde(default = "subtree")]
    pub covers: String,
}

/// One denial in force on a key (openapi.yaml `Denial`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KeyDenial {
    pub denial_id: String,
    pub workspace_node_id: String,
}

/// One connection of a connector key, as the one-key read lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KeyConnection {
    pub state: String,
    #[serde(default)]
    pub client_name: Option<String>,
}

/// `GET /v2/keys/{key}`: one key's full record, never its value (ADR-0096
/// d4).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KeyRecord {
    pub key_id: String,
    pub credential: String,
    pub name: Option<String>,
    pub role: String,
    pub principal: String,
    #[serde(default)]
    pub crew_id: Option<String>,
    #[serde(default)]
    pub user: Option<MaybeNamed>,
    #[serde(default)]
    pub default_workspace_id: Option<String>,
    #[serde(default)]
    pub used_by: Option<String>,
    /// The label sent at issuance; `None` where the read serves none.
    #[serde(default)]
    pub label: Option<String>,
    pub guardrail_scope: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub expiry: Option<String>,
    #[serde(default)]
    pub expired: bool,
    #[serde(default)]
    pub requests_per_minute: Option<u32>,
    #[serde(default)]
    pub revoked_at: Option<String>,
    pub grants: Vec<KeyGrant>,
    #[serde(default)]
    pub denials: Vec<KeyDenial>,
    #[serde(default)]
    pub connections: Vec<KeyConnection>,
}

/// `POST /v2/keys/{key}/limits`: the key's limits as they stand.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KeyLimits {
    pub key_id: String,
    pub name: Option<String>,
    pub expiry: Option<String>,
    pub expired: bool,
    pub requests_per_minute: Option<u32>,
}

/// An `{id, name}` reference whose name may be null: a key that predates
/// names.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MaybeNamed {
    pub id: String,
    pub name: Option<String>,
}

/// One import (openapi.yaml `Import`), with the importing key where a
/// listing serves it (ADR-0096 d6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Import {
    pub import_id: String,
    pub state: String,
    pub target_workspace_id: String,
    #[serde(default)]
    pub position: u64,
    #[serde(default)]
    pub counts: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    pub refusals: Vec<serde_json::Value>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub key: Option<MaybeNamed>,
}

#[derive(Deserialize)]
struct ImportAnswer {
    import: Import,
}

/// `GET /v2/imports`: a page of imports, newest first (ADR-0096 d6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImportPage {
    pub items: Vec<Import>,
    pub next_cursor: Option<String>,
}

/// One pending brief edge a person's reviewer key may ratify (ADR-0096 d7).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BriefProposal {
    pub edge_id: String,
    pub workspace: MaybeNamed,
    pub artifact_id: String,
    pub claim_key: Option<String>,
    pub title: Option<String>,
    pub proposed_by: MaybeNamed,
    pub same_user: bool,
    pub proposed_at: String,
}

/// `GET /v2/review/briefs`: a page of brief proposals, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BriefProposalPage {
    pub items: Vec<BriefProposal>,
    pub next_cursor: Option<String>,
}

/// `POST /v2/keys/{key}/rotate`'s answer (ADR-0096 d13): the value is
/// `None` on a replay, which never shows it again.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RotatedKey {
    pub key_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    pub rotated_at: String,
    #[serde(default)]
    pub previous_valid_until: Option<String>,
}

/// One admin fix as a person's review read serves it (openapi.yaml `Fix`;
/// ADR-0089 d15): never the admin's key or person, and the acts the
/// reading key may take.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Fix {
    pub fix_id: String,
    pub act: String,
    pub target_artifact_id: String,
    pub workspace_id: String,
    pub claim_key: String,
    #[serde(default)]
    pub replaced_artifact_id: Option<String>,
    #[serde(default)]
    pub served_artifact_id: Option<String>,
    pub reason: String,
    pub loosens_guardrail: bool,
    pub state: String,
    pub fixed_at: String,
    #[serde(default)]
    pub same_user: Option<bool>,
    #[serde(default)]
    pub restored_artifact_id: Option<String>,
    /// `confirm` and `revert`, `confirm` alone for an erasure, or none
    /// where this key acts for the admin's own person outside its root.
    #[serde(default)]
    pub acts: Vec<String>,
}

/// One admin fix as the admin's own listing serves it (ADR-0089 d15).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdminFix {
    pub fix_id: String,
    pub act: String,
    pub target_artifact_id: String,
    pub workspace_id: String,
    pub claim_key: String,
    pub reason: String,
    pub loosens_guardrail: bool,
    pub state: String,
    pub fixed_at: String,
    #[serde(default)]
    pub admin_person: Option<String>,
}

#[derive(Deserialize)]
struct Items<T> {
    items: Vec<T>,
}

/// One key of `GET /v2/keys` (openapi.yaml `ListedKey`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListedKey {
    pub id: String,
    pub name: Option<String>,
    pub role: String,
    pub used_by: String,
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
    /// Whether the key is a contributor without recall: a submitter grant on
    /// a node no reader grant of it reaches (ADR-0093 d3).
    #[serde(default)]
    pub without_recall: bool,
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
    /// The admin act an admin's issuance is, and its state:
    /// `pending_confirmation` until a second admin person confirms it,
    /// `effective` where it took effect at once (ADR-0097 d5, d10, d23).
    #[serde(default)]
    pub admin_act: Option<IssuedAct>,
    /// Whether the answer was a recorded outcome replayed, which carries no
    /// value or code; read off `Idempotency-Replay`, not the body.
    #[serde(skip)]
    replayed: bool,
}

/// The admin act of an admin's issuance (ADR-0097 d2).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct IssuedAct {
    pub act_id: String,
    pub state: String,
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
            admin_act: None,
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

/// `POST /v2/workspaces`: the new node, the fields read here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// The Memories list's filters for `GET /v1/memories` with `with_own`, or
/// with `mine` the caller's own key set alone (ADR-0083 d3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryQuery<'a> {
    /// `mine=true`: the caller's own rows, with no scoped part.
    pub mine: bool,
    pub workspace: Option<&'a str>,
    /// A comma list of states.
    pub state: Option<&'a str>,
    /// A comma list of kinds (ADR-0082 d8 item 3; ADR-0092 d6).
    pub kinds: Option<&'a str>,
    /// Only the promoted rows approved at or after this RFC 3339 instant.
    pub promoted_since: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub author: Option<&'a str>,
    pub limit: u32,
    pub cursor: Option<&'a str>,
}

/// The catalog's filters for `GET /v1/memories` without `own` (ADR-0092
/// d6): `kinds` comma-separated, none for every kind; never `include_text`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogQuery<'a> {
    pub workspace: Option<&'a str>,
    pub kinds: Option<&'a str>,
    pub prefix: Option<&'a str>,
    pub state: Option<&'a str>,
    pub limit: u32,
    pub cursor: Option<&'a str>,
}

/// A text search of the caller's readable memories, its own held ones
/// among them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecallQuery<'a> {
    pub query: &'a str,
    pub k: u32,
    /// A comma list of kinds, sent as the body's `kinds` array; `None`
    /// sends no `kinds`, so the server serves every hit kind but steering,
    /// test and the vocabulary kinds, with the steering list (ADR-0092 d1).
    /// `document` serves each document's best passage (ADR-0081 d4;
    /// ADR-0092 d6).
    pub kinds: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub origin: Option<&'a str>,
    /// Sent as `include_unreviewed`, always, so an in-progress scope's
    /// default of `true` never applies unasked (ADR-0030 d4; ADR-0083 d1).
    pub include_unreviewed: bool,
}

/// The admin read's filters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminSearch<'a> {
    pub query: Option<&'a str>,
    pub workspace: Option<&'a str>,
    pub state: Option<&'a str>,
    pub origin_kind: Option<&'a str>,
    pub author: Option<&'a str>,
    /// A comma list of kinds, sent as the body's `kinds` array (ADR-0083
    /// d5; ADR-0092 d6).
    pub kinds: Option<&'a str>,
    pub limit: u32,
    pub cursor: Option<&'a str>,
    /// The admin key's open investigation the read is made under
    /// (ADR-0089 d10).
    pub investigation: Option<&'a str>,
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
            (ACTING_KEY, call.and_then(|c| c.acting_key)),
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

    /// `GET /v1/guardrails`, at `workspace` (an id or a name) or at the key's
    /// default workspace.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn guardrails(
        &self,
        call: &Call<'_>,
        workspace: Option<&str>,
    ) -> Result<GuardrailSet, Error> {
        let path = match workspace {
            Some(w) => format!("/v1/guardrails?workspace={}", encode(w)),
            None => "/v1/guardrails".to_owned(),
        };
        self.get(Some(call), path).await
    }

    /// `GET /v1/skills`: one page of the skills offered at `workspace` (an id
    /// or a name) or at the key's default workspace, the key's own held
    /// skills too where `include_own_held` asks; pass the page's
    /// `next_cursor` back as `cursor` for the next (ADR-0080 d10, d11).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn skills(
        &self,
        call: &Call<'_>,
        workspace: Option<&str>,
        include_own_held: bool,
        cursor: Option<&str>,
    ) -> Result<SkillSet, Error> {
        let mut query = Vec::new();
        if let Some(workspace) = workspace {
            query.push(format!("workspace={}", encode(workspace)));
        }
        if include_own_held {
            query.push("include_own_held=true".to_owned());
        }
        if let Some(cursor) = cursor {
            query.push(format!("cursor={}", encode(cursor)));
        }
        let path = if query.is_empty() {
            "/v1/skills".to_owned()
        } else {
            format!("/v1/skills?{}", query.join("&"))
        };
        self.get(Some(call), path).await
    }

    /// `GET /v2/workspaces/{workspace}/tree`: one page of `limit` nodes
    /// after `cursor`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn workspace_tree(
        &self,
        call: &Call<'_>,
        workspace: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<WorkspacePage, Error> {
        let mut path = format!("/v2/workspaces/{}/tree?limit={limit}", encode(workspace));
        if let Some(cursor) = cursor {
            path.push_str("&cursor=");
            path.push_str(&encode(cursor));
        }
        self.get(Some(call), path).await
    }

    /// `GET /v2/workspaces/{workspace}`: the node and its live proposals.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn workspace(
        &self,
        call: &Call<'_>,
        workspace: &str,
    ) -> Result<WorkspaceView, Error> {
        self.get(Some(call), format!("/v2/workspaces/{}", encode(workspace)))
            .await
    }

    /// `GET /v2/workspaces/{workspace}/offers`: the acts the caller's verbs
    /// admit there now, and each refused act's refusal.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn workspace_offers(
        &self,
        call: &Call<'_>,
        workspace: &str,
    ) -> Result<WorkspaceOffers, Error> {
        self.get(
            Some(call),
            format!("/v2/workspaces/{}/offers", encode(workspace)),
        )
        .await
    }

    /// `POST /v2/workspaces/{workspace}/transitions`: proposes `to`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn propose_transition(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        workspace: &str,
        to: &str,
    ) -> Result<Transition, Error> {
        self.post(
            call,
            format!("/v2/workspaces/{}/transitions", encode(workspace)),
            idempotency_key,
            &serde_json::json!({ "to": to }),
        )
        .await
    }

    /// `POST /v2/workspaces/{workspace}/transitions/{transition_id}/ratify`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn ratify_transition(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        workspace: &str,
        transition_id: &str,
    ) -> Result<Transition, Error> {
        self.send(
            Method::POST,
            Some(call),
            format!(
                "/v2/workspaces/{}/transitions/{}/ratify",
                encode(workspace),
                encode(transition_id)
            ),
            Some(idempotency_key),
            None,
        )
        .await
    }

    /// `POST /v2/workspaces/{workspace}/mode`: `in_progress`, or `standard`,
    /// which on an in-progress root is its commit.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn set_workspace_mode(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        workspace: &str,
        mode: &str,
    ) -> Result<WorkspaceView, Error> {
        self.post(
            call,
            format!("/v2/workspaces/{}/mode", encode(workspace)),
            idempotency_key,
            &serde_json::json!({ "mode": mode }),
        )
        .await
    }

    /// `POST /v2/workspaces/{workspace}/public`: an admin person sets or
    /// clears the node's `public` flag with a reason (ADR-0098 d6, d9).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn set_workspace_public(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        workspace: &str,
        public: bool,
        reason: Option<&str>,
    ) -> Result<PublicOutcome, Error> {
        self.post(
            call,
            format!("/v2/workspaces/{}/public", encode(workspace)),
            idempotency_key,
            &serde_json::json!({ "public": public, "reason": reason }),
        )
        .await
    }

    /// `GET /v1/memories` with `with_own=true`, or `mine=true`, and
    /// `include_text=true`: the caller's merged listing, or its own rows
    /// alone, with each item's text.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn list_memories(
        &self,
        call: &Call<'_>,
        query: &MemoryQuery<'_>,
    ) -> Result<MemoryPage, Error> {
        let mut path = format!(
            "/v1/memories?{}=true&include_text=true&limit={}",
            if query.mine { "mine" } else { "with_own" },
            query.limit
        );
        for (name, value) in [
            ("author", query.author),
            ("workspace", query.workspace),
            ("state", query.state),
            ("kinds", query.kinds),
            ("promoted_since", query.promoted_since),
            ("origin_kinds", query.origin),
            ("cursor", query.cursor),
        ] {
            if let Some(value) = value {
                path.push_str(&format!("&{name}={}", encode(value)));
            }
        }
        self.get(Some(call), path).await
    }

    /// `GET /v1/memories` without `own`: one page of the catalog of what
    /// the caller reads, without text, and on the first page each kind's
    /// count (ADR-0092 d6).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn catalog(
        &self,
        call: &Call<'_>,
        query: &CatalogQuery<'_>,
    ) -> Result<CatalogPage, Error> {
        let mut path = format!("/v1/memories?limit={}", query.limit);
        for (name, value) in [
            ("workspace", query.workspace),
            ("kinds", query.kinds),
            ("prefix", query.prefix),
            ("state", query.state),
            ("cursor", query.cursor),
        ] {
            if let Some(value) = value {
                path.push_str(&format!("&{name}={}", encode(value)));
            }
        }
        self.get(Some(call), path).await
    }

    /// `POST /v1/memories/recall`: the server's default kinds with their
    /// text, or the kinds `kinds` names, held ones only where
    /// `include_unreviewed` asks for them. Read-shaped: no idempotency key.
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
            "include_text": true,
            "include_unreviewed": query.include_unreviewed,
        });
        if let Some(kinds) = query.kinds {
            body["kinds"] = serde_json::json!(kinds.split(',').collect::<Vec<_>>());
        }
        if let Some(workspace) = query.workspace {
            body["workspace"] = serde_json::json!(workspace);
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
            ("workspace", search.workspace),
            ("state", search.state),
            ("origin_kind", search.origin_kind),
            ("author", search.author),
            ("cursor", search.cursor),
            ("investigation", search.investigation),
        ] {
            if let Some(value) = value {
                body[name] = serde_json::json!(value);
            }
        }
        if let Some(kinds) = search.kinds {
            body["kinds"] = serde_json::json!(kinds.split(',').collect::<Vec<_>>());
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

    /// `GET /v2/workspaces/{workspace}/tree?counts=true`: the subtree with
    /// each node's rows counted by state, and with `offers` the scope's
    /// offers to the same caller, read beside them (#286).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn workspace_counts(
        &self,
        call: &Call<'_>,
        workspace: &str,
        offers: bool,
    ) -> Result<WorkspaceCounts, Error> {
        let asked = if offers { "&offers=true" } else { "" };
        self.get(
            Some(call),
            format!(
                "/v2/workspaces/{}/tree?counts=true{asked}",
                encode(workspace)
            ),
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
        for (name, value) in filters.iter().filter(|(n, _)| {
            matches!(
                *n,
                "workspace" | "author_key" | "claim_key_prefix" | "needs"
            )
        }) {
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

    /// `GET /v2/review/queue`: each workspace holding held rows, with its
    /// count and its oldest row's time. An admin key's.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn review_queue(&self, call: &Call<'_>) -> Result<ReviewQueue, Error> {
        self.get(Some(call), "/v2/review/queue".to_owned()).await
    }

    /// `GET /v2/todo/counts`: the To do page's counts, as a person's web
    /// session key reads them, at `workspace` and below or across every
    /// workspace (ADR-0083 d11).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn todo_counts(
        &self,
        call: &Call<'_>,
        workspace: Option<&str>,
    ) -> Result<TodoCounts, Error> {
        let path = match workspace {
            Some(w) => format!("/v2/todo/counts?workspace={}", encode(w)),
            None => "/v2/todo/counts".to_owned(),
        };
        self.get(Some(call), path).await
    }

    /// `GET /v2/web/notices`: the calling person's notices not dismissed,
    /// oldest first (ADR-0097 d20).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn notices(&self, call: &Call<'_>) -> Result<NoticeList, Error> {
        self.get(Some(call), "/v2/web/notices".to_owned()).await
    }

    /// `POST /v2/web/notices/{notice}/dismiss`: the person dismisses a
    /// notice of their own (ADR-0097 d20).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn dismiss_notice(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        notice: &str,
    ) -> Result<Notice, Error> {
        self.post(
            call,
            format!("/v2/web/notices/{}/dismiss", encode(notice)),
            idempotency_key,
            &serde_json::json!({}),
        )
        .await
    }

    /// `GET /v2/admin/todo/counts`: the administration To do page's
    /// counts. An admin key's (ADR-0083 d11).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn admin_todo_counts(&self, call: &Call<'_>) -> Result<AdminTodoCounts, Error> {
        self.get(Some(call), "/v2/admin/todo/counts".to_owned())
            .await
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

    /// `POST /v2/workspaces`: creates `name` under `parent`, or under the
    /// caller's own root when `parent` is `None`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn create_workspace(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        name: &str,
        parent: Option<&str>,
    ) -> Result<Workspace, Error> {
        let mut body = serde_json::json!({ "name": name });
        if let Some(parent) = parent {
            body["parent"] = serde_json::json!(parent);
        }
        self.post(call, "/v2/workspaces".to_owned(), idempotency_key, &body)
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

    /// `POST /v2/keys` with a whole body, as the console's key builder
    /// sends it: grants, default workspace, guardrail scope, expiry and
    /// rate beside the name, role and kind (ADR-0096 d1).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn issue_key_with(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        body: &Value,
    ) -> Result<KeyIssued, Error> {
        let (mut issued, replayed): (KeyIssued, bool) = self
            .exchange(
                Method::POST,
                Some(call),
                "/v2/keys".to_owned(),
                Some(idempotency_key),
                Some(body),
            )
            .await?;
        issued.replayed = replayed;
        Ok(issued)
    }

    /// `GET /v2/keys/{key}`: one key's full record (ADR-0096 d4).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn get_key(&self, call: &Call<'_>, key: &str) -> Result<KeyRecord, Error> {
        self.get(Some(call), format!("/v2/keys/{}", encode(key)))
            .await
    }

    /// `POST /v2/keys/{key}/limits`: `body` sets or clears `expiry` and
    /// `requests_per_minute` (ADR-0079 d5; ADR-0096 d4).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn set_key_limits(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        key: &str,
        body: &Value,
    ) -> Result<KeyLimits, Error> {
        self.post(
            call,
            format!("/v2/keys/{}/limits", encode(key)),
            idempotency_key,
            body,
        )
        .await
    }

    /// `POST /v2/keys/{key}/denials` on `workspace` with a `reason`
    /// (ADR-0096 d4).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn deny_key(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        key: &str,
        workspace: &str,
        reason: &str,
    ) -> Result<Denial, Error> {
        self.post(
            call,
            format!("/v2/keys/{}/denials", encode(key)),
            idempotency_key,
            &serde_json::json!({ "workspace": workspace, "reason": reason }),
        )
        .await
    }

    /// `GET /v2/imports`: a page of imports, newest first; as a person's
    /// web session every key of the person's (ADR-0096 d6).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn list_imports(
        &self,
        call: &Call<'_>,
        state: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<ImportPage, Error> {
        let mut query = Vec::new();
        if let Some(state) = state {
            query.push(format!("state={}", encode(state)));
        }
        if let Some(cursor) = cursor {
            query.push(format!("cursor={}", encode(cursor)));
        }
        let path = if query.is_empty() {
            "/v2/imports".to_owned()
        } else {
            format!("/v2/imports?{}", query.join("&"))
        };
        self.get(Some(call), path).await
    }

    /// `GET /v2/imports/{id}`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn get_import(&self, call: &Call<'_>, id: &str) -> Result<Import, Error> {
        let answer: ImportAnswer = self
            .get(Some(call), format!("/v2/imports/{}", encode(id)))
            .await?;
        Ok(answer.import)
    }

    /// `GET /v2/review/briefs`: the pending brief edges a person's review
    /// session key may ratify (ADR-0096 d7).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn brief_proposals(
        &self,
        call: &Call<'_>,
        workspace: Option<&str>,
    ) -> Result<BriefProposalPage, Error> {
        let path = match workspace {
            Some(w) => format!("/v2/review/briefs?workspace={}", encode(w)),
            None => "/v2/review/briefs".to_owned(),
        };
        self.get(Some(call), path).await
    }

    /// `POST /v2/edges/{id}/ratify`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn ratify_edge(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        edge_id: &str,
    ) -> Result<Value, Error> {
        self.post(
            call,
            format!("/v2/edges/{}/ratify", encode(edge_id)),
            idempotency_key,
            &serde_json::json!({}),
        )
        .await
    }

    /// `POST /v2/edges` with `kind: brief`: proposes `artifact_id` for the
    /// brief of `workspace` (ADR-0046 d7; ADR-0095 d6).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn propose_brief(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        artifact_id: &str,
        workspace: &str,
    ) -> Result<Value, Error> {
        self.post(
            call,
            "/v2/edges".to_owned(),
            idempotency_key,
            &serde_json::json!({
                "kind": "brief",
                "from_artifact_id": artifact_id,
                "to_workspace_id": workspace,
            }),
        )
        .await
    }

    /// `GET /v2/admin/acts?state=`: the tenant's admin acts in `state`, or
    /// `all`, oldest first (ADR-0097 d2).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn admin_acts(&self, call: &Call<'_>, state: &str) -> Result<AdminActList, Error> {
        self.get(
            Some(call),
            format!("/v2/admin/acts?state={}", encode(state)),
        )
        .await
    }

    /// `POST /v2/admin/acts/{act}/confirm {reason}`: a second admin person
    /// confirms a pending act, which takes effect in that commit, with the
    /// reason the act keeps (ADR-0097 d5, d11, d26).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn confirm_admin_act(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        act: &str,
        reason: &str,
    ) -> Result<AdminAct, Error> {
        self.post(
            call,
            format!("/v2/admin/acts/{}/confirm", encode(act)),
            idempotency_key,
            &serde_json::json!({ "reason": reason }),
        )
        .await
    }

    /// `POST /v2/admin/acts/{act}/decline {reason}`: a decline, final, which
    /// undoes a `sole_admin` act it declines (ADR-0097 d3, d10, d23).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn decline_admin_act(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        act: &str,
        reason: &str,
    ) -> Result<AdminAct, Error> {
        self.post(
            call,
            format!("/v2/admin/acts/{}/decline", encode(act)),
            idempotency_key,
            &serde_json::json!({ "reason": reason }),
        )
        .await
    }

    /// `GET /v2/admin/fixes`: the tenant's unconfirmed fixes (ADR-0089 d15).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn admin_fixes(&self, call: &Call<'_>) -> Result<Vec<AdminFix>, Error> {
        let answer: Items<AdminFix> = self.get(Some(call), "/v2/admin/fixes".to_owned()).await?;
        Ok(answer.items)
    }

    /// `POST /v2/keys/{key}/rotate`: a new value for `key`, answered once;
    /// `grace_seconds` keeps the previous value working until
    /// `previous_valid_until` (ADR-0096 d13). A replay answers no value.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn rotate_key(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        key: &str,
        grace_seconds: Option<u32>,
    ) -> Result<RotatedKey, Error> {
        let body = match grace_seconds {
            Some(grace) => serde_json::json!({ "grace_seconds": grace }),
            None => serde_json::json!({}),
        };
        self.post(
            call,
            format!("/v2/keys/{}/rotate", encode(key)),
            idempotency_key,
            &body,
        )
        .await
    }

    /// `GET /v2/review/fixes`: the unconfirmed admin fixes where a person's
    /// reviewer key holds reviewer, each with the acts it may take
    /// (ADR-0089 d15).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn review_fixes(&self, call: &Call<'_>) -> Result<Vec<Fix>, Error> {
        let answer: Items<Fix> = self.get(Some(call), "/v2/review/fixes".to_owned()).await?;
        Ok(answer.items)
    }

    /// `POST /v2/review/fixes/{id}/confirm` or `/revert` (ADR-0089 d8,
    /// d15): `revert` true reverts.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn settle_fix(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        fix_id: &str,
        revert: bool,
    ) -> Result<Value, Error> {
        self.post(
            call,
            format!(
                "/v2/review/fixes/{}/{}",
                encode(fix_id),
                if revert { "revert" } else { "confirm" }
            ),
            idempotency_key,
            &serde_json::json!({}),
        )
        .await
    }

    /// `GET /v2/admin/investigations`: the admin key's own open
    /// investigations (ADR-0089 d10, d15).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn investigations(&self, call: &Call<'_>) -> Result<Vec<Investigation>, Error> {
        let answer: Items<Investigation> = self
            .get(Some(call), "/v2/admin/investigations".to_owned())
            .await?;
        Ok(answer.items)
    }

    /// `GET /v2/edges?kind=brief&workspace_id=`: a workspace's brief
    /// edges, pending and ratified (ADR-0046 d7), each page's
    /// `next_cursor` followed, at most [`BRIEF_EDGE_PAGES`] pages of 200;
    /// past that the answer's `next_cursor` names the page not read.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer, on any page.
    pub async fn brief_edges(
        &self,
        call: &Call<'_>,
        workspace: &str,
    ) -> Result<BriefEdgePage, Error> {
        let first = format!(
            "/v2/edges?kind=brief&limit=200&workspace_id={}",
            encode(workspace)
        );
        let mut all: BriefEdgePage = self.get(Some(call), first.clone()).await?;
        for _ in 1..BRIEF_EDGE_PAGES {
            let Some(cursor) = all.next_cursor.take() else {
                break;
            };
            let page: BriefEdgePage = self
                .get(Some(call), format!("{first}&cursor={}", encode(&cursor)))
                .await?;
            all.items.extend(page.items);
            all.next_cursor = page.next_cursor;
        }
        Ok(all)
    }

    /// `POST /v2/admin/fixes`: `body` is `{act, target, reason, body?}`
    /// (ADR-0089 d15; ADR-0096 d5).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn admin_fix(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        body: &Value,
    ) -> Result<Value, Error> {
        self.post(call, "/v2/admin/fixes".to_owned(), idempotency_key, body)
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

    /// `POST /v2/keys/{key}/denials` on `workspace`: on `universe`, the key
    /// is cut off.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn add_denial(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        key: &str,
        workspace: &str,
    ) -> Result<Denial, Error> {
        self.post(
            call,
            format!("/v2/keys/{}/denials", encode(key)),
            idempotency_key,
            &serde_json::json!({ "workspace": workspace }),
        )
        .await
    }

    /// `POST /v2/keys/{key}/link {user, reason}`: an admin links a person's
    /// tenant key to `user`, a consent act whose reason is required, or
    /// `None`, `POST /v2/keys/{key}/unlink`, ends its link (ADR-0085 d16;
    /// ADR-0097 d7).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn link_key(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        key: &str,
        user: Option<(&str, &str)>,
    ) -> Result<KeyLink, Error> {
        let (verb, body) = match user {
            Some((user, reason)) => (
                "link",
                serde_json::json!({ "user": user, "reason": reason }),
            ),
            None => ("unlink", serde_json::json!({})),
        };
        self.post(
            call,
            format!("/v2/keys/{}/{verb}", encode(key)),
            idempotency_key,
            &body,
        )
        .await
    }

    /// `POST /v2/keys/{key}/accept_link`: the signed-in person accepts the
    /// link an admin made of `key` to them, so it acts for them (ADR-0085
    /// d16, the operator's decision of 2026-11-19).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn accept_key_link(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        key: &str,
    ) -> Result<KeyLink, Error> {
        self.post(
            call,
            format!("/v2/keys/{}/accept_link", encode(key)),
            idempotency_key,
            &serde_json::json!({}),
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
    pub async fn admin_read(
        &self,
        call: &Call<'_>,
        artifact_id: &str,
        investigation: &str,
    ) -> Result<AdminRead, Error> {
        self.get(
            Some(call),
            format!(
                "/v2/admin/memories/{}?investigation={}",
                encode(artifact_id),
                encode(investigation)
            ),
        )
        .await
    }

    /// `POST /v2/admin/investigations {reason}`: opens an investigation of
    /// the admin key, the reason its reads of claim text are made under
    /// (ADR-0089 d10). Not idempotent: each call opens one.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn open_investigation(
        &self,
        call: &Call<'_>,
        reason: &str,
    ) -> Result<Investigation, Error> {
        self.send(
            Method::POST,
            Some(call),
            "/v2/admin/investigations".to_owned(),
            None,
            Some(&serde_json::json!({ "reason": reason })),
        )
        .await
    }

    /// `POST /v2/admin/investigations/{id}/close`: the admin key closes its
    /// own investigation (ADR-0089 d10).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn close_investigation(
        &self,
        call: &Call<'_>,
        investigation: &str,
    ) -> Result<Investigation, Error> {
        self.send(
            Method::POST,
            Some(call),
            format!("/v2/admin/investigations/{}/close", encode(investigation)),
            None,
            Some(&serde_json::json!({})),
        )
        .await
    }

    /// `POST /v1/memories` with `verb: learn`: a new claim deriving from
    /// `derives_from`, in `workspace`, held until reviewed.
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
            "workspace": submission.workspace,
            "derives_from": submission.derives_from,
        });
        if let Some(claim_key) = submission.claim_key {
            body["claim_key"] = Value::from(claim_key);
        }
        self.post(call, "/v1/memories".to_owned(), idempotency_key, &body)
            .await
    }

    /// `POST /v1/memories/preview`: what capture would decide for a draft,
    /// written nowhere and metered nowhere. Read-shaped: no idempotency key.
    ///
    /// # Errors
    /// The API's refusal of authority, or no usable answer; a refusal
    /// capture would make is the answer's `refusal`.
    pub async fn preview(
        &self,
        call: &Call<'_>,
        draft: &Draft<'_>,
    ) -> Result<MemoryPreview, Error> {
        let mut body = draft.body();
        if let Some((artifact_id, _)) = draft.replaces {
            body["replaces"] = Value::from(artifact_id);
        }
        self.send(
            Method::POST,
            Some(call),
            "/v1/memories/preview".to_owned(),
            None,
            Some(&body),
        )
        .await
    }

    /// `POST /v1/memories` with a draft's kind and fields: a new skill,
    /// guardrail or document, held until reviewed (ADR-0082 d2, d4).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn submit(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        draft: &Draft<'_>,
    ) -> Result<Accepted, Error> {
        self.post(
            call,
            "/v1/memories".to_owned(),
            idempotency_key,
            &draft.body(),
        )
        .await
    }

    /// `POST /v1/memories/{artifact_id}/supersede` with a draft's fields,
    /// re-sent on every version: the edit or the retirement of the version
    /// `replaces` names, at the version the form rendered, held until
    /// reviewed (ADR-0082 d2, d3, d4; ADR-0037 d10).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer; a draft that names no
    /// version it replaces is refused before any call.
    pub async fn supersede_draft(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        draft: &Draft<'_>,
    ) -> Result<Accepted, Error> {
        let Some((artifact_id, _)) = draft.replaces else {
            return Err(Error::Decode {
                status: 0,
                why: "a supersession names the version it replaces".to_owned(),
            });
        };
        self.post(
            call,
            format!("/v1/memories/{}/supersede", encode(artifact_id)),
            idempotency_key,
            &draft.body(),
        )
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

    /// `POST /v1/memories/{artifact_id}/withdraw`: the author retracts its
    /// own held version, kept as withdrawn and never served (ADR-0083 d13).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn withdraw(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        artifact_id: &str,
        reason: Option<&str>,
    ) -> Result<Withdrawn, Error> {
        let mut body = serde_json::json!({});
        if let Some(reason) = reason {
            body["reason"] = Value::from(reason);
        }
        self.post(
            call,
            format!("/v1/memories/{}/withdraw", encode(artifact_id)),
            idempotency_key,
            &body,
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
    /// `keep_an_admin` un-flagging the last admin is refused. `reason` is
    /// required for a new admin person outside `sole_admin` and for an act
    /// against another admin person (ADR-0097 d8, d26).
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
        reason: Option<&str>,
    ) -> Result<WebEmail, Error> {
        let mut body = serde_json::json!({
            "email": email, "user": user, "admin": admin, "keep_an_admin": true,
        });
        if let Some(reason) = reason {
            body["reason"] = serde_json::json!(reason);
        }
        self.post(call, "/v2/web/emails".to_owned(), idempotency_key, &body)
            .await
    }

    /// `POST /v2/web/emails/remove`, refusing the last admin entry;
    /// `reason` is required to remove another admin person's entry
    /// (ADR-0097 d8, d26).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn remove_web_email(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        email: &str,
        reason: Option<&str>,
    ) -> Result<Removed, Error> {
        let mut body = serde_json::json!({ "email": email, "keep_an_admin": true });
        if let Some(reason) = reason {
            body["reason"] = serde_json::json!(reason);
        }
        self.post(
            call,
            "/v2/web/emails/remove".to_owned(),
            idempotency_key,
            &body,
        )
        .await
    }

    /// `POST /v2/web/emails/accept`: the signed-in person accepts a sign-in
    /// email an admin added for them, so it signs in as them (ADR-0045 d2,
    /// the operator's decision of 2026-11-19). `admin` is the flag the
    /// person reviewed; an entry whose flag changed since is refused
    /// `changed` (the operator's decision of 2026-11-20).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn accept_web_email(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        email: &str,
        admin: bool,
    ) -> Result<WebEmail, Error> {
        self.post(
            call,
            "/v2/web/emails/accept".to_owned(),
            idempotency_key,
            &serde_json::json!({ "email": email, "admin": admin }),
        )
        .await
    }

    /// `POST /v2/web/emails/decline`: the signed-in person removes a sign-in
    /// email an admin added for them that waits on them (ADR-0045 d2, the
    /// operator's decision of 2026-11-19).
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn decline_web_email(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        email: &str,
    ) -> Result<WebEmail, Error> {
        self.post(
            call,
            "/v2/web/emails/decline".to_owned(),
            idempotency_key,
            &serde_json::json!({ "email": email }),
        )
        .await
    }

    /// `POST /v2/web/users/{user}/capacities`: review or curation on
    /// `workspace`.
    ///
    /// # Errors
    /// The API's refusal, or no usable answer.
    pub async fn grant_web_capacity(
        &self,
        call: &Call<'_>,
        idempotency_key: &str,
        user: &str,
        capacity: &str,
        workspace: &str,
        reason: Option<&str>,
    ) -> Result<CapacityGranted, Error> {
        let mut body = serde_json::json!({ "capacity": capacity, "workspaces": [workspace] });
        // The admin's reason, which a grant waiting on a second admin
        // person requires (ADR-0097 d5, d26).
        if let Some(reason) = reason {
            body["reason"] = serde_json::json!(reason);
        }
        self.post(
            call,
            format!("/v2/web/users/{}/capacities", encode(user)),
            idempotency_key,
            &body,
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
        acting_key: None,
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
                .issue_key(&CALL, "idem", "alice-laptop", "contributor", false, None)
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
                .issue_key(&CALL, "idem", "alice-laptop", "contributor", false, expiry)
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
                serde_json::json!({"name": "alice-laptop", "role": "contributor", "connect_code": false}),
                serde_json::json!({
                    "name": "alice-laptop",
                    "role": "contributor",
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
            decodes(&client.workspace_tree(&CALL, "w/1", 200, Some("c 1")).await),
            decodes(&client.readyz().await),
            decodes(&client.workspace(&CALL, "alice.proj").await),
            decodes(&client.workspace_offers(&CALL, "alice.proj").await),
            decodes(
                &client
                    .propose_transition(&CALL, "idem-1", "alice.proj", "protected")
                    .await,
            ),
            decodes(
                &client
                    .ratify_transition(&CALL, "idem-2", "alice.proj", "t-1")
                    .await,
            ),
            decodes(
                &client
                    .set_workspace_mode(&CALL, "idem-3", "alice.proj", "in_progress")
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
            decodes(&client.workspace_counts(&CALL, "alice", true).await),
            decodes(&client.list_keys(&CALL, Some("alice"), 200, None).await),
            decodes(
                &client
                    .create_workspace(&CALL, "idem-4", "proj", Some("alice"))
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
                    .review_held(&CALL, &[("workspace", "alice")], 50, None)
                    .await,
            ),
            decodes(&client.review_named(&CALL, &["a", "b"]).await),
            decodes(
                &client
                    .post_bulk_verdict(&CALL, "idem-6", "approve", &["a"], None)
                    .await,
            ),
            decodes(&client.review_queue(&CALL).await),
            decodes(&client.todo_counts(&CALL, Some("alice")).await),
            decodes(&client.admin_todo_counts(&CALL).await),
            decodes(&client.admin_acts(&CALL, "all").await),
            decodes(
                &client
                    .confirm_admin_act(&CALL, "idem-a1", "act-1", "checked")
                    .await,
            ),
            decodes(
                &client
                    .decline_admin_act(&CALL, "idem-a2", "act-1", "not this agent")
                    .await,
            ),
            decodes(&client.notices(&CALL).await),
            decodes(&client.dismiss_notice(&CALL, "idem-n1", "n-1").await),
            decodes(
                &client
                    .issue_key(&CALL, "idem-7", "alice-laptop", "contributor", false, None)
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
            decodes(&client.admin_read(&CALL, "m-1", "i-1").await),
            decodes(
                &client
                    .learn(
                        &CALL,
                        "idem-10",
                        &Learn {
                            body: "Keys rotate quarterly.",
                            workspace: "universe",
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
            decodes(
                &client
                    .preview(
                        &CALL,
                        &Draft {
                            kind: "skill",
                            body: "Cite every source.",
                            skill: Some(("cite", "Cite sources.")),
                            replaces: Some(("m-1", 1)),
                            ..Draft::default()
                        },
                    )
                    .await,
            ),
            decodes(
                &client
                    .withdraw(&CALL, "idem-11w", "m-1", Some("by mistake"))
                    .await,
            ),
            decodes(&client.whoami_start(&CALL).await),
            decodes(&client.list_web_emails(&CALL, 200, None).await),
            decodes(
                &client
                    .allow_web_email(&CALL, "idem-12", "a@example.com", "alice", false, None)
                    .await,
            ),
            decodes(
                &client
                    .remove_web_email(&CALL, "idem-13", "a@example.com", None)
                    .await,
            ),
            decodes(
                &client
                    .grant_web_capacity(&CALL, "idem-14", "alice", "review", "alice", None)
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
            decodes(
                &client
                    .catalog(
                        &CALL,
                        &CatalogQuery {
                            kinds: Some("skill"),
                            limit: 50,
                            ..CatalogQuery::default()
                        },
                    )
                    .await,
            ),
            decodes(
                &client
                    .skills(&CALL, Some("alice.proj"), true, Some("c 1"))
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
        // The record alone, for a caller that needs no person block.
        assert_eq!(seen[1].uri(), "/v2/whoami?person=false");
        // The person block is asked for by name, on the one method that reads it.
        assert_eq!(seen[seen.len() - 3].uri(), "/v2/whoami");
        assert_eq!(seen[2].uri(), "/v1/guardrails?workspace=alice.proj");
        assert_eq!(
            seen[3].uri(),
            "/v2/workspaces/w%2F1/tree?limit=200&cursor=c%201"
        ); // Each POST carries its idempotency key; one with a body sends it
        // as JSON, and the ratify sends none.
        for (i, key, body) in [
            (7, "idem-1", Some(r#"{"to":"protected"}"#)),
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
            "/v2/workspaces/alice/tree?counts=true&offers=true"
        );
        assert_eq!(seen[15].headers()[IDEMPOTENCY_KEY], "idem-4");
        assert_eq!(
            std::str::from_utf8(seen[15].body()).unwrap(),
            r#"{"name":"proj","parent":"alice"}"#
        );
        // The admin acts: the list's state in the query, a confirm with an
        // empty body, and a decline's reason (ADR-0097 d23).
        assert_eq!(seen[24].uri(), "/v2/admin/acts?state=all");
        assert_eq!(seen[25].uri(), "/v2/admin/acts/act-1/confirm");
        assert_eq!(
            std::str::from_utf8(seen[26].body()).unwrap(),
            r#"{"reason":"not this agent"}"#
        );
        assert_eq!(seen[14].uri(), "/v2/keys?limit=200&user=alice");
        assert_eq!(seen[18].uri(), "/v2/review/held?limit=50&workspace=alice");
        assert_eq!(seen[19].uri(), "/v2/review/held?ids=a,b");
        assert_eq!(
            std::str::from_utf8(seen[27].body()).unwrap(),
            r#"{"connect_code":false,"name":"alice-laptop","role":"contributor"}"#
        );
    }

    /// The catalog read sends no `own`, no `with_own` and no
    /// `include_text`, each filter percent-encoded, and reads the first
    /// page's counts, exact or capped (ADR-0092 d6).
    #[tokio::test]
    async fn the_catalog_read_sends_its_filters_and_never_asks_for_text_or_own_work() {
        const BODY: &str = r#"{"items":[{"artifact_id":"a1","claim_key":"skill/deploy","kind":"skill","version":2,"workspace_path":"universe.alice","state":"promoted","unreviewed":false,"needs_person":false,"retirement":false,"created_at":"2026-11-15T10:00:00Z","approved_at":"2026-11-15T10:01:00Z","approver":{"used_by":"person","same_user":false},"name":"deploy","description":"Deploy, then check.","prompt":null}],"next_cursor":"c2","counts":{"skill":1,"memory":{"at_least":10000}}}"#;
        let (client, seen) = recording(200, BODY);
        let page = client
            .catalog(
                &CALL,
                &CatalogQuery {
                    workspace: Some("alice.proj"),
                    kinds: Some("term,term_mapping"),
                    prefix: Some("doc/run books/"),
                    state: Some("promoted"),
                    limit: 50,
                    cursor: Some("c 1"),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            seen.lock().unwrap()[0].uri(),
            "/v1/memories?limit=50&workspace=alice.proj&kinds=term%2Cterm_mapping&prefix=doc%2Frun%20books%2F&state=promoted&cursor=c%201"
        );
        let counts = page.counts.expect("the first page's counts");
        assert_eq!(counts["skill"], CatalogCount::Exact(1));
        assert_eq!(counts["memory"], CatalogCount::AtLeast { at_least: 10_000 });
        assert_eq!(page.items[0].name.as_deref(), Some("deploy"));
        assert_eq!(page.items[0].prompt, None);
        assert_eq!(page.next_cursor.as_deref(), Some("c2"));
    }

    /// The skill set is read one page at a time, at the key's default
    /// workspace unless one is named, the key's own held skills only where
    /// asked for, and each skill carries its resource and its approval
    /// (ADR-0080 d10, d11).
    #[tokio::test]
    async fn the_skill_set_is_read_one_page_at_a_time_with_only_the_fields_given() {
        const BODY: &str = r#"{"scope_node":"ws_1","scope_path":"universe.alice","guardrail_scope":"ancestors","set_digest":"sd_1","skills":[{"artifact_id":"a1","claim_key":"skill/deploy","name":"deploy","description":"Deploy, then check.","version":2,"workspace_path":"universe.alice","depth":0,"overrides":["universe"],"approved_at":"2026-11-15T10:01:00Z","approver":{"used_by":"person","same_user":false},"uri":"oml://workspace/universe.alice/skill/deploy","unreviewed":false}],"next_cursor":"c2"}"#;
        let (client, seen) = recording(200, BODY);
        let page = client.skills(&CALL, None, false, None).await.unwrap();
        client
            .skills(&CALL, Some("universe.alice"), true, Some("c 1"))
            .await
            .unwrap();
        let uris: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.uri().to_string())
            .collect();
        assert_eq!(
            uris,
            [
                "/v1/skills",
                "/v1/skills?workspace=universe.alice&include_own_held=true&cursor=c%201"
            ]
        );
        assert_eq!(page.set_digest, "sd_1");
        assert_eq!(page.next_cursor.as_deref(), Some("c2"));
        let deploy = &page.skills[0];
        assert_eq!(
            (deploy.name.as_str(), deploy.depth, deploy.unreviewed),
            ("deploy", 0, false)
        );
        assert_eq!(deploy.overrides, ["universe"]);
        assert_eq!(deploy.uri, "oml://workspace/universe.alice/skill/deploy");
    }

    /// A recall that names no kinds sends no `kinds`, so the server's
    /// default applies: every hit kind but steering, test and the vocabulary
    /// kinds, with the steering list served (ADR-0092 d1).
    #[tokio::test]
    async fn a_recall_that_names_no_kinds_sends_no_kinds() {
        let (client, seen) = recording(200, r#"{"steering":[],"hits":[]}"#);
        client
            .recall(
                &CALL,
                &RecallQuery {
                    query: "restart",
                    k: 5,
                    ..RecallQuery::default()
                },
            )
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(seen.lock().unwrap()[0].body()).unwrap();
        assert_eq!(body.get("kinds"), None, "{body}");
    }

    /// A recall names the kinds it asks for, and a document's hit carries
    /// the passage it serves, how many more matched and the passage's
    /// resource (ADR-0081 d4, d6).
    #[tokio::test]
    async fn a_document_hit_carries_its_passage_and_the_recall_names_its_kinds() {
        const BODY: &str = r#"{"steering":"not_requested","hits":[{"memory":{"artifact_id":"d1","claim_key":"doc/runbook","version":1,"tip":true,"workspace_path":"universe.alice","state":"promoted","principal":null,"created_at":"2026-11-15T10:00:00Z","promoted_at":"2026-11-15T10:01:00Z","kind":"document","text":"Restart the lead."},"score":0.5,"matched":{"terms":["restart"],"of":1,"via":["bm25"]},"passage":{"fragment":"restart","ordinal":1,"start_line":3,"end_line":3,"heading_path":["Runbook","Restart"]},"more_passages":2,"uri":"oml://workspace/universe.alice/doc/runbook?passage=restart"}]}"#;
        let (client, seen) = recording(200, BODY);
        let answer = client
            .recall(
                &CALL,
                &RecallQuery {
                    query: "restart",
                    k: 5,
                    kinds: Some("document"),
                    ..RecallQuery::default()
                },
            )
            .await
            .unwrap();
        let kinds: Vec<Value> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice::<Value>(r.body()).unwrap()["kinds"].clone())
            .collect();
        assert_eq!(kinds, [serde_json::json!(["document"])]);
        let hit = &answer.hits[0];
        let passage = hit.passage.as_ref().expect("a document hit's passage");
        assert_eq!(
            (
                passage.fragment.as_str(),
                passage.ordinal,
                passage.start_line,
                passage.end_line
            ),
            ("restart", 1, 3, 3)
        );
        assert_eq!(passage.heading_path, ["Runbook", "Restart"]);
        assert_eq!(hit.more_passages, Some(2));
        assert_eq!(
            hit.uri.as_deref(),
            Some("oml://workspace/universe.alice/doc/runbook?passage=restart")
        );
    }

    /// Answers each request with the next of `bodies`, `200`.
    struct Paging {
        seen: Seen,
        bodies: Vec<&'static str>,
    }

    impl Transport for Paging {
        fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
            let mut seen = self.seen.lock().unwrap();
            let body = self.bodies[seen.len().min(self.bodies.len() - 1)];
            seen.push(request);
            let response = Response::builder()
                .status(200)
                .body(body.as_bytes().to_vec())
                .unwrap();
            Box::pin(async move { Ok(response) })
        }
    }

    /// `brief_edges` follows `next_cursor` to the last page, so an edge on
    /// a later page is found too (ADR-0046 d7).
    #[tokio::test]
    async fn brief_edges_follows_next_cursor_to_the_last_page() {
        const FIRST: &str = r#"{"items":[{"edge_id":"e1","from_artifact_id":"a1","to_workspace_id":"w","state":"pending","proposer_key_id":"k"}],"next_cursor":"c2"}"#;
        const LAST: &str = r#"{"items":[{"edge_id":"e2","from_artifact_id":"a2","to_workspace_id":"w","state":"ratified","proposer_key_id":"k"}],"next_cursor":null}"#;
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let client = Client::new(Paging {
            seen: seen.clone(),
            bodies: vec![FIRST, LAST],
        });
        let page = client.brief_edges(&CALL, "w").await.unwrap();
        let ids: Vec<&str> = page.items.iter().map(|e| e.edge_id.as_str()).collect();
        assert_eq!(ids, ["e1", "e2"]);
        assert_eq!(page.next_cursor, None);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(
            seen[1].uri().to_string().ends_with("&cursor=c2"),
            "{}",
            seen[1].uri()
        );
    }

    /// The admin search sends its kinds as the body's `kinds` array, as
    /// recall's body takes them, never v0.8.0's draft `kind` (ADR-0083 d5;
    /// ADR-0092 d6).
    #[tokio::test]
    async fn the_admin_search_sends_its_kinds_as_a_kinds_array() {
        let (client, seen) = recording(200, r#"{"items":[],"next_cursor":null}"#);
        client
            .admin_search(
                &CALL,
                &AdminSearch {
                    state: Some("promoted"),
                    kinds: Some("skill,guardrail"),
                    limit: 50,
                    ..AdminSearch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            std::str::from_utf8(seen.lock().unwrap()[0].body()).unwrap(),
            r#"{"kinds":["skill","guardrail"],"limit":50,"state":"promoted"}"#
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
            admin_act: None,
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
