//! The use-cases: every verb the transports will front, enforcing the
//! constitution here — verb ceilings (R1), reviewer-never-author (R2),
//! promoted-only reads (R3) — never in an adapter (ADR-0019 d1). Every
//! workarea-scoped verb resolves effective access from the key's grants and
//! denials on each request, in the one refusal order of roles-and-keys.md
//! (ADR-0025 d1).

mod pages;
mod web;
pub use pages::{AdminMemory, AdminPage, Capacity, OwnKey, QueueRow, VerdictTrail};
pub use web::{Callback, CallbackRefusal, WEB_SESSION_TTL_SECS};

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::access::{
    AccessRefusal, Covers, Grant, IssuanceRefusal, KeyDenial, TailCaller, TailSubject,
    admissible_grant_roles, check_issuance, effective_roles, effective_verbs, resolve,
    tail_visible,
    LabelledGrant, Verb, VerbRefusal, effective_access, resolve_verb,
};
use oml_domain::brief::{brief_cut, brief_order};
use oml_domain::budget::{budget_tokens, cut, hit_order};
use oml_domain::claim_edges::{
    LabelRefusal, Ratifier, RatifyRefusal, check_label, check_ratifier, check_term_curies,
};
use oml_domain::name::{
    Assignment, CrewObject, Name, NamedKind, Namespace, Naming, Reference, UNIVERSE, UnnamedRow,
    assign_names, unresolved_qualified, unresolved_value,
};
use oml_domain::oauth::{
    ATTEMPTS_PER_PAGE, AUTHORIZATION_REQUEST_SECONDS, AuthorizationCodeInvalid,
    AuthorizationExpired, AuthorizationRequest, AuthorizeBounds, ClientId, ClientInvalid, CodeRow,
    ConnectCode as CodeSymbols, ConnectRefused, ConnectionState, DOCUMENT_KEPT_SECONDS,
    ExchangeDecision, ExchangeRequest, ExchangeRow, Issuer, KeyState, PkceRequired,
    RefreshDecision, RefreshInvalid, Refusal, RequestClientKind, RevokedCause, SCOPE, Token,
    TokenKind, Validation, after_wrong_entry, check_authorize_bounds, check_code_entry,
    check_confirmation, check_pkce_method, check_resource, classify_client_id, decide_exchange,
    decide_refresh, open_request, refresh_token_digest, select_redirect, token_kind,
    validate_document, validate_registration,
};
use oml_domain::vocab::{Curie, OmlLabel};
use oml_domain::vocab_edges::{EdgeState, TermEdge, check_acyclic, edge_transition};
use oml_domain::{
    ChainHead, DERIVED_CLAIM_KEY_PREFIX, Envelope, EventDraft, EventTime, FrozenOp, HeldBy, KeySet,
    LadderNode, LadderRefusal, LineageClosure, MemoryKind, MemoryStatus, Role, Sha256Hash,
    SignedEnvelope, WorkareaStatus, canonical_bytes, check_reviewer, derive_claim_key, event_types,
    frozen_refuses, next_envelope, nfc, propose_check, ratifier_is_proposer, ratify_check,
    same_user, sha256_digest_string, transition,
};

use crate::cursor::{ClaimEdgeCursor, ListingCursor, NameCursor, PathCursor};
use crate::ports::{
    AdmissionEnvelope, ClientDocumentFetcher, Clock, CommitReceipt, CommitRequest, ConnectionStore,
    DocumentFetchFailure, HandleSealer, IdGen, IdempotencyPut, IdempotencyRecord, IdempotencyScope,
    JournalStore, MetadataFold, NoObserver, OutboxRow, RequestObserver, ScanOutcome, Scanner,
    Signer, StoreError, SubjectRow, TailEvent,
};
use crate::types::{
    AppError, AssignedNames, AuthorizationRecord, AuthorizeOutcome, AuthorizePage, AuthorizeParams,
    Bearer, BootstrapRequest, BootstrapResult, ClientAuthentication, ClientDescription, ClientKind,
    ClaimEdgeProposal, ClaimEdgeProposalOutcome, ClaimEdgeRatification, ClaimEdgeRecord,
    ClaimEdgeFilter, ClaimEdgePage, ClaimEdgePosition, ClaimEdgeQuery, ClaimEdgeSelection,
    CodeEntryOutcome, ConfirmAction, ConfirmOutcome, ConfirmationGrant, ConfirmationView,
    ConnectCode, ConnectCodeIssued, ConnectCodeRecord, ConnectGrant, ConnectOutcome,
    ConnectionRecord, CreatedWorkarea, Credential, CrewCreateRequest, CrewPage, CrewRecord,
    CrewUpdate, DenialRecord, ExchangeForm, GrantRecord, HIT_KINDS, IssueKeyRequest, IssuedKey,
    IssuedTokens, KeyAccess, KeyDetail, KeyRecord, ListRequest, ListingFilter, ListingPosition,
    ListingScope, MemoryPage, MemoryRecord, MintedToken, NamedRef, NamingRefusal,
    OAuthClientRecord, PROVIDER_TENANT, RecallHit, RecallRequest, RecallResult, RefreshFacts,
    RefreshForm, RegisteredClient, ServedMemory, SubmitOutcome, SubmitReceipt, SubmitRequest,
    SubmitVerb, SupersedeOutcome, SupersedeReceipt, SupersedeRequest, TailPage, TailedEvent,
    TenantRow, TokenPair, TreeNode, TreePage, UnnamedRows, Verdict, VerdictOutcome, VerdictRequest,
    VersionRef, Whoami, WorkareaInfo, WorkareaRecord,
    TransitionRecord, TransitionState,
    TransitionView,
    ClaimEdgeProposal, ClaimEdgeProposalOutcome, ClaimEdgeRecord, ProposeClaimEdge,
    ClaimEdgeFilter, ClaimEdgePage, ClaimEdgePosition, ClaimEdgeQuery,
    Brief, BriefEdgePage, BriefEdgeProposalOutcome, BriefEdgeRatification, BriefEdgeRecord, BriefEdgeView, EdgeRatification, ProposeBriefEdge,
    WorkareaStatusView,
};

const EVENTS_PAGE: usize = 512;

/// The review policy in force: the one PoC constant `{k: 1,
/// required_roles: ["dreamer"]}`. A verdict's ratification and a status
/// transition's `policy_snapshot` both read it here, so the two cannot
/// disagree (admission.md review and ratification; workareas.md, the policy
/// snapshot is the policy in force plus the precondition; ADR-0009 d3).
fn review_policy() -> Value {
    json!({ "k": 1, "required_roles": ["dreamer"] })
}

/// The fixed reason a failed issuance's `key.revoked` digests
/// (roles-and-keys.md issuance order; ADR-0025 d2).
const ISSUANCE_INCOMPLETE: &str = "issuance_incomplete";

/// The rule `system.names_assigned` applies, as its payload names it
/// (event-catalog.md `system.names_assigned`).
const NAMES_RULE: &str = "ADR-0026 d11";

/// The fixed reason the lead's lift of a denial on an admin key digests, in
/// the commit that names the chain (event-catalog.md `key.denial_lifted`;
/// ADR-0026 d11, rule 5).
const ADMIN_DENIAL_LIFT_REASON: &str = "adr-0026-d15";

/// `oauth.connect_code_ttl`'s shipped default, 10 minutes, in seconds
/// (configuration.md; ADR-0031 d8). FR-22: until journalled settings are
/// built, this code constant is the lifetime of every connect code this
/// build mints, as [`CURSOR_TTL_SECS`](crate::cursor::CURSOR_TTL_SECS) is a
/// cursor's.
pub const CONNECT_CODE_TTL_SECS: i64 = 600;

/// `oauth.access_token_ttl`'s shipped default, 1 hour, in seconds: the
/// lifetime of every access token this build mints until settings are
/// built (configuration.md; FR-22; ADR-0031 d10).
pub const ACCESS_TOKEN_TTL_SECS: i64 = 3_600;

/// `oauth.refresh_token_ttl`'s shipped default, 30 days, in seconds: the
/// lifetime of every refresh token this build mints until settings are
/// built (configuration.md; FR-22; ADR-0031 d10).
pub const REFRESH_TOKEN_TTL_SECS: i64 = 30 * 86_400;

/// An authorization code lives 60 seconds from Connect, a design constant
/// and not a setting (ADR-0031 d9 step 3).
pub const AUTHORIZATION_CODE_TTL_SECS: i64 = 60;

/// The application, generic over its driven ports.
pub struct App<J, S, C, I, Sc> {
    journal: J,
    signer: S,
    clock: C,
    ids: I,
    scanner: Sc,
    /// `oauth.issuer`, the public origin; `None` where the deployment sets
    /// none, and then no connect code is minted (ADR-0031 d1).
    oauth_issuer: Option<Issuer>,
    /// Wrong connect-code entries since the process started, deployment
    /// wide and never per tenant, for doctor (ADR-0031 d8, d14).
    connect_code_failures: AtomicU64,
    /// Told what each request resolved, for its log line; it decides
    /// nothing (ADR-0042 d2, d14).
    observer: Arc<dyn RequestObserver>,
    /// `brief.max_tokens`: the budget a workarea's brief is cut to
    /// (ADR-0046 d8; FR-77).
    brief_max_tokens: u64,
}

/// The authenticated key and what its effective access is computed from,
/// read once per request and never cached (workareas.md effective access;
/// ADR-0025 d1, d14). A key value and an access token of a live connection
/// each resolve to one, the same for one key (ADR-0031 d11).
#[derive(Debug)]
pub struct Caller {
    key: KeyRecord,
    grants: Vec<Grant>,
    denials: Vec<KeyDenial>,
    /// The epoch of the access read above (workareas.md permission epoch).
    permission_epoch: u64,
    /// The own key set when it is wider than the key itself: a person's
    /// `<user>-web` session key owns every key naming its user (ADR-0045
    /// d16, amending ADR-0025 d3). Empty means the key alone.
    own_keys: Vec<Uuid>,
}

impl Caller {
    /// The key the request acts as.
    #[must_use]
    pub fn key(&self) -> &KeyRecord {
        &self.key
    }

    /// The key's grants as read on this request.
    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// The key's global denials as read on this request.
    #[must_use]
    pub fn denials(&self) -> &[KeyDenial] {
        &self.denials
    }

    /// The epoch of the access read (workareas.md permission epoch).
    #[must_use]
    pub fn permission_epoch(&self) -> u64 {
        self.permission_epoch
    }

    /// The own key set: the key, or every key naming a session user key's
    /// user (ADR-0045 d16).
    fn own_key_ids(&self) -> Vec<Uuid> {
        if self.own_keys.is_empty() {
            vec![self.key.key_id]
        } else {
            self.own_keys.clone()
        }
    }

    /// `effective_roles` at the node whose ancestry, node to root, this is.
    fn effective(&self, ancestry: &[Uuid]) -> BTreeSet<Role> {
        effective_roles(&self.grants, &self.denials, ancestry)
    }

    /// Whether a global per-key denial covers the node: deny-wins, own rows
    /// included (roles-and-keys.md#own).
    fn denied(&self, ancestry: &[Uuid]) -> bool {
        self.denials.iter().any(|d| ancestry.contains(&d.node))
    }
}

/// How an issuance runs once its issuer is known (ADR-0045 d6, d16).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Issuance<'a> {
    /// An admin or operator key's issuance of a bearer or connector key.
    Plain,
    /// A session key for the allowlist user named: no value.
    Session(&'a Name),
    /// A person's `<user>-web` issuing a key in its own root.
    PersonsOwn,
}

/// The user root an issuance names: its id, found or minted, its name, the
/// tenant's `universe` it sits under, and whether this issuance creates it
/// (ADR-0044 d2).
struct UserRoot {
    id: Uuid,
    name: Name,
    universe: Uuid,
    created: bool,
}

/// An admin verb: checked on the key's own role, never on grants
/// (roles-and-keys.md permission matrix; http-api.md authorization).
fn require_role(key: &KeyRecord, role: Role, verb: &'static str) -> Result<(), AppError> {
    if key.role == role {
        Ok(())
    } else {
        Err(AppError::VerbCeiling {
            role: key.role,
            verb,
        })
    }
}

/// Step 1 of the refusal order: the key's role must admit the grant role the
/// verb needs. It reads no node, so it runs before the workarea resolves
/// (roles-and-keys.md grant admissibility and refusal order; ADR-0025 d1).
fn require_admissible(key: &KeyRecord, needed: Role, verb: &'static str) -> Result<(), AppError> {
    if admissible_grant_roles(key.role).contains(&needed) {
        Ok(())
    } else {
        Err(AppError::VerbCeiling {
            role: key.role,
            verb,
        })
    }
}

/// A `held_by: service` key is refused every review-class verb before any
/// lookup: its dreamer grant confers them like any other, and this rule, not
/// the grant, keeps it out of review (roles-and-keys.md; ADR-0025 d1).
fn refuse_service(key: &KeyRecord, verb: &'static str) -> Result<(), AppError> {
    if key.held_by == HeldBy::Service {
        Err(AppError::VerbCeiling {
            role: key.role,
            verb,
        })
    } else {
        Ok(())
    }
}

/// A refusal-order answer as the typed refusal; `node` is the workarea
/// `access.grant_refused` names.
fn access_refusal(refusal: AccessRefusal, node: Uuid, verb: &'static str) -> AppError {
    match refusal {
        AccessRefusal::VerbCeiling { role, .. } => AppError::VerbCeiling { role, verb },
        AccessRefusal::GrantRefused { needed_role } => AppError::GrantRefused {
            workarea_id: node,
            needed_role,
        },
        AccessRefusal::NotFound => AppError::NotFound,
    }
}

/// Issuance steps 1–3 as the typed refusal.
/// `oauth.connect_refused` with its `details.reason` (errors.md; ADR-0031
/// d7, d8).
fn connect_refused(reason: ConnectRefused) -> AppError {
    AppError::OAuth(Refusal::ConnectRefused(reason))
}

/// The catalog payload of `connection.code_issued`: the code's id and
/// expiry and nothing derived from the code, not even its digest
/// (event-catalog.md `connection.code_issued`; ADR-0031 d8).
fn code_issued_payload(code: &ConnectCode, issued_by: Uuid, voids: Option<Uuid>) -> Value {
    json!({
        "code_id": code.code_id.to_string(),
        "key_id": code.key_id.to_string(),
        "issued_by_key_id": issued_by.to_string(),
        "expires_at": code.expires_at.as_str(),
        "voids_code_id": voids.map(|c| c.to_string()),
    })
}

fn issuance_refusal(refusal: IssuanceRefusal) -> AppError {
    match refusal {
        IssuanceRefusal::RoleExclusive => AppError::RoleExclusive,
        IssuanceRefusal::IssuerCeiling {
            role,
            admissible,
            grant_index,
        } => AppError::IssuerCeiling {
            role,
            admissible,
            grant_index,
        },
    }
}

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 256 bits as exactly 43 base62 characters, zero-padded (roles-and-keys.md
/// key format: `oml_` + 43 × `[A-Za-z0-9]`).
fn base62_43(bytes: &[u8; 32]) -> String {
    let mut num = bytes.to_vec();
    let mut digits: Vec<u8> = Vec::with_capacity(43);
    while num.iter().any(|&b| b != 0) {
        let mut rem: u32 = 0;
        for b in &mut num {
            let acc = (rem << 8) | u32::from(*b);
            *b = u8::try_from(acc / 62).unwrap_or(u8::MAX);
            rem = acc % 62;
        }
        digits.push(BASE62[rem as usize]);
    }
    while digits.len() < 43 {
        digits.push(b'0');
    }
    digits.reverse();
    String::from_utf8(digits).expect("base62 alphabet is ASCII")
}

/// A node's derived path from its ancestry, listed node to root: the names
/// from `universe` down to the node, joined with `.` (ADR-0025 d14). Every
/// path the app serves is this derivation.
fn derived_path(ancestry: &[WorkareaRecord]) -> String {
    ancestry
        .iter()
        .rev()
        .map(|w| w.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

/// The request digest of a submission, over the claim_key and the verb as
/// resolved, so an omitted key and its derived spelling are one request
/// (ADR-0039 d3, d6).
fn submit_request_digest(
    req: &SubmitRequest,
    claim_key: &str,
    verb: SubmitVerb,
    workarea_id: Uuid,
) -> String {
    let doc = json!({
        "body": req.body,
        "claim_key": claim_key,
        "verb": verb.as_str(),
        "kind": req.kind.as_str(),
        "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "workarea_id": workarea_id.to_string(),
    });
    sha256_digest_string(doc.to_string().as_bytes())
}

/// The request digest of a supersession: SHA-256 over the RFC 8785 JSON of
/// the method, the route template, the path parameters and the parsed body,
/// so one Idempotency-Key reused for two targets never replays the first
/// (http-api.md idempotency; ADR-0025 d10).
fn supersede_request_digest(req: &SupersedeRequest) -> Result<String, AppError> {
    let doc = json!({
        "method": "POST",
        "route": "/v1/memories/{artifact_id}/supersede",
        "path_params": { "artifact_id": req.target_id.to_string() },
        "body": {
            "body": req.body,
            "kind": req.kind.map(MemoryKind::as_str),
            "retirement": req.retirement,
            "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "reason": req.reason,
        },
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// The request digest of a review verdict: SHA-256 over the RFC 8785 JSON
/// of the method, the route template, no path parameters and the parsed
/// body, `serve_id` as sent and `null` when omitted, so a verdict naming a
/// serve and the same verdict by default are two requests (http-api.md
/// idempotency; ADR-0039 d10).
fn verdict_request_digest(req: &VerdictRequest) -> Result<String, AppError> {
    let doc = json!({
        "method": "POST",
        "route": "/v2/review/verdicts",
        "path_params": {},
        "body": {
            "serve_id": req.serve.as_ref().map(|serve| serve.serve_id.as_str()),
            "verdict": match req.verdict { Verdict::Approve => "approve", Verdict::Reject => "reject" },
            "reason": req.reason,
        },
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// A verdict's idempotency record as [`App::review_verdict`] looked it up:
/// the scope and the request digest its commit records under.
struct VerdictRecording {
    scope: IdempotencyScope,
    request_digest: String,
}

impl VerdictRecording {
    /// The record committed with the verdict, storing `outcome`.
    fn put(&self, outcome: &VerdictOutcome) -> Result<IdempotencyPut, AppError> {
        let stored = RecordedVerdict {
            memory_id: outcome.memory_id,
            version: outcome.version,
            state: outcome.state.as_str().to_owned(),
            journal_seq: outcome.journal_seq,
        };
        Ok(IdempotencyPut {
            scope: self.scope.clone(),
            record: IdempotencyRecord {
                request_digest: self.request_digest.clone(),
                response: serde_json::to_value(&stored)
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?,
            },
        })
    }
}

/// The stored response of a committed verdict, which a replay answers
/// (http-api.md idempotency).
#[derive(serde::Serialize, serde::Deserialize)]
struct RecordedVerdict {
    memory_id: Uuid,
    version: u32,
    state: String,
    journal_seq: u64,
}

impl RecordedVerdict {
    fn replay(self) -> Result<VerdictOutcome, AppError> {
        // A verdict leaves its row promoted or rejected (admission.md).
        let state = match self.state.as_str() {
            "promoted" => MemoryStatus::Promoted,
            "rejected" => MemoryStatus::Rejected,
            other => {
                return Err(AppError::StoredResponse(format!(
                    "a verdict's stored state is {other:?}"
                )));
            }
        };
        Ok(VerdictOutcome {
            memory_id: self.memory_id,
            version: self.version,
            state,
            journal_seq: self.journal_seq,
            replayed: true,
        })
    }
}

/// What `node` is to the ladder (ADR-0046 d1): the universe root, or an
/// ordinary node. A user root is `LadderNode::UserRoot` once the workarea
/// row carries ADR-0044's `user_root`; until that build no node is one.
fn ladder_node(node: &WorkareaRecord) -> LadderNode {
    if node.parent_id.is_none() {
        LadderNode::Universe
    } else {
        LadderNode::Ordinary
    }
}

/// The `user` a key names (ADR-0044 d2), which `same_user` compares
/// (ADR-0046 d2); every key is a tenant key until ADR-0044's build adds the
/// field, so this is `None`.
fn key_user(_key: &KeyRecord) -> Option<Uuid> {
    None
}

/// The request digest of a claim-edge proposal: SHA-256 over the RFC 8785
/// JSON of the method, the route template, no path parameters and the body
/// as sent, `kind: claim` included, since ADR-0039 d9 merges the claim-edge
/// and workarea-edge families into `/v2/edges` (http-api.md idempotency).
fn claim_edge_request_digest(req: &ProposeClaimEdge) -> Result<String, AppError> {
    let doc = json!({
        "method": "POST",
        "route": "/v2/edges",
        "path_params": {},
        "body": {
            "kind": "claim",
            "from_artifact_id": req.from_artifact_id.to_string(),
            "to_artifact_id": req.to_artifact_id.to_string(),
            "label": req.label,
        },
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// The request digest of a brief-edge proposal: the `/v2/edges` family's,
/// `kind: brief`, the workarea as sent (http-api.md idempotency).
fn brief_edge_request_digest(req: &ProposeBriefEdge) -> Result<String, AppError> {
    let doc = json!({
        "method": "POST",
        "route": "/v2/edges",
        "path_params": {},
        "body": {
            "kind": "brief",
            "from_artifact_id": req.artifact_id.to_string(),
            "to_workarea_id": req.workarea.to_string(),
        },
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// The idempotency record a commit writes beside its event, storing
/// `response`, when the request named a scope.
fn idempotency_put<T: serde::Serialize>(
    scope: Option<&IdempotencyScope>,
    request_digest: &str,
    response: &T,
) -> Result<Option<IdempotencyPut>, AppError> {
    scope
        .map(|scope| {
            Ok(IdempotencyPut {
                scope: scope.clone(),
                record: IdempotencyRecord {
                    request_digest: request_digest.to_owned(),
                    response: serde_json::to_value(response)
                        .map_err(|e| AppError::StoredResponse(e.to_string()))?,
                },
            })
        })
        .transpose()
}

/// The request digest of a claim-edge ratification: SHA-256 over the RFC
/// 8785 JSON of the method, the route template, the edge id as its path
/// parameter and a `null` body, as the route takes none (http-api.md
/// idempotency, claim edges).
fn claim_edge_ratify_request_digest(edge_id: Uuid) -> Result<String, AppError> {
    let doc = json!({
        "method": "POST",
        "route": "/v2/edges/{edge_id}/ratify",
        "path_params": { "edge_id": edge_id.to_string() },
        "body": Value::Null,
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// A claim edge's label against its endpoints: [`check_label`] on their
/// kinds, then [`check_term_curies`] on their `claim_key`s, each refusal
/// naming its body field (errors.md `request.validation`; ADR-0021 d2, d5).
fn check_edge_label(label: &str, src: &MemoryRecord, dst: &MemoryRecord) -> Result<(), AppError> {
    let refused = |reason: LabelRefusal| AppError::ClaimEdgeLabel {
        field: reason.field(),
        reason,
    };
    check_label(label, src.kind, dst.kind).map_err(refused)?;
    check_term_curies(label, &src.claim_key, &dst.claim_key).map_err(refused)
}

/// A term endpoint's CURIE, its `claim_key` (ADR-0021 d2); anything else
/// fails the cycle check closed. The label check at proposal and at
/// ratification refuses a non-CURIE endpoint first, so this answers only a
/// stored row that defies it.
fn term_curie(kind: MemoryKind, claim_key: &str) -> Result<Curie, AppError> {
    let refused = || AppError::TermEndpoint {
        claim_key: claim_key.to_owned(),
    };
    if kind != MemoryKind::Term {
        return Err(refused());
    }
    Curie::parse(claim_key).map_err(|_| refused())
}

/// A recall's `query_digest`: SHA-256 over the RFC 8785 JSON of an object
/// whose members are always present, defaults applied — `kinds` sorted by
/// code point with duplicates removed, `max_tokens` null when absent, and
/// `scope` the resolved node. `terms` and `expand_terms` digest as absent:
/// the vocabulary filter is unbuilt, and a request that sends either is
/// refused before it reaches a recall (retrieval.md, the read log; ADR-0025
/// d9, d16).
fn recall_query_digest(query: &str, req: &RecallRequest, scope: Uuid) -> Result<String, AppError> {
    let mut kinds: Vec<&str> = req.kinds.iter().map(|kind| kind.as_str()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    let doc = json!({
        "query": query,
        "k": req.k,
        "kinds": kinds,
        "include_bodies": req.include_text,
        "include_own_held": req.include_unreviewed,
        "max_tokens": req.max_tokens,
        "terms": [],
        "expand_terms": "none",
        "scope": scope.to_string(),
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// The successor's kind: an omitted one is the target's; a `memory`,
/// `derived_fact` or `steering` target admits any of those three, a
/// vocabulary-kind target keeps its kind; a retirement keeps the target's
/// kind and never retires a term, which deprecates instead
/// (audit-and-lineage.md supersession chains; ADR-0025 d11).
fn successor_kind(
    target: MemoryKind,
    sent: Option<MemoryKind>,
    retirement: bool,
) -> Result<MemoryKind, AppError> {
    const CLASS: [MemoryKind; 3] = [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Steering,
    ];
    if retirement && target == MemoryKind::Term {
        return Err(AppError::TermRetirement);
    }
    let Some(sent) = sent else {
        return Ok(target);
    };
    let admitted = if retirement || !CLASS.contains(&target) {
        sent == target
    } else {
        CLASS.contains(&sent)
    };
    if admitted {
        Ok(sent)
    } else {
        Err(AppError::KindChangeRefused { target, sent })
    }
}

/// One event's commit request, with its outbox row.
fn commit_request(
    event: SignedEnvelope,
    fold: MetadataFold,
    idempotency: Option<IdempotencyPut>,
) -> CommitRequest {
    let outbox = OutboxRow {
        event_id: event.envelope.event_id,
        topic: event.envelope.event_type.clone(),
        // Digest-only, like the journal: no body plaintext rides the
        // outbox either (storage.md bronze mirror).
        payload: json!({
            "tenant_id": event.envelope.tenant_id.to_string(),
            "seq": event.envelope.seq,
            "event_type": event.envelope.event_type,
        }),
    };
    CommitRequest {
        event,
        fold,
        outbox,
        idempotency,
    }
}

/// A string field of a payload as journalled; `None` when absent or not a
/// string.
fn payload_str(payload: &Value, field: &str) -> Option<String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// A field of a payload naming a row by its id.
fn payload_id(payload: &Value, field: &str) -> Option<Uuid> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
}

/// A crew object as a PoC `workarea.create` or `workarea.crew_updated`
/// carried it; `None` for null or absent (ADR-0026 d11, rule 4).
fn crew_object(value: Option<&Value>) -> Option<CrewObject> {
    let object = value?.as_object()?;
    let member = |field: &str| object.get(field).and_then(Value::as_str).map(str::to_owned);
    Some(CrewObject {
        name: member("name"),
        charter_uri: member("charter_uri"),
        external_manager: member("external_manager"),
    })
}

/// The payload of `system.names_assigned` (event-catalog.md): every row
/// named, kept names included, and every crew record made.
fn names_assigned_payload(naming: &Naming) -> Value {
    json!({
        "rule": NAMES_RULE,
        "names": naming.names.iter().map(|a| json!({
            "kind": a.kind.as_str(),
            "id": a.id.to_string(),
            "name": a.name.as_str(),
            "prior_name": a.prior_name,
        })).collect::<Vec<_>>(),
        "crews": naming.crews.iter().map(|c| json!({
            "crew_id": c.crew_id.to_string(),
            "name": c.name.as_str(),
            "charter_uri": c.charter_uri,
            "external_manager": c.external_manager,
            "workarea_ids": c.workarea_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// One chain's rows that predate names, read in journal order of creation
/// from the chain itself (http-api.md rows created before names).
struct PredatingChain {
    tenant_id: Uuid,
    /// The tenant's own row, named in the deployment's namespace.
    tenant: Option<UnnamedRow>,
    /// The root workarea, which rule 1 names `universe`.
    root: Option<Uuid>,
    /// The chain's workareas and keys.
    rows: Vec<UnnamedRow>,
    /// Each workarea's current crew object, in journal order.
    crews: Vec<(Uuid, CrewObject)>,
    /// Every global denial on an admin key not yet lifted, as `(denial_id,
    /// key_id)` in journal order: rule 5 lifts each in the commit that names
    /// the chain.
    lifts: Vec<(Uuid, Uuid)>,
}

/// The rows of `unnamed` as the chain's creation events carry them, and the
/// denials on admin keys rule 5 lifts, or the refusal that keeps the chain
/// unnamed: a `system.names_assigned` already journalled, or a row with no
/// creation event (ADR-0026 d11).
fn predating_chain(
    tenant_id: Uuid,
    unnamed: &UnnamedRows,
    events: &[SignedEnvelope],
) -> Result<PredatingChain, AppError> {
    let refuse = |refusal| AppError::NamingRefused { tenant_id, refusal };
    let (mut tenant, mut root, mut rows) = (None, None, Vec::new());
    let mut crews: Vec<(Uuid, Option<CrewObject>)> = Vec::new();
    let (mut admin_keys, mut denials) = (BTreeSet::new(), Vec::new());
    for event in events {
        let envelope = &event.envelope;
        let payload = &envelope.payload;
        match envelope.event_type.as_str() {
            event_types::SYSTEM_NAMES_ASSIGNED => {
                return Err(refuse(NamingRefusal::AlreadyAssigned { seq: envelope.seq }));
            }
            // The PoC journalled the tenant's name as `tenant_name`.
            event_types::TENANT_GENESIS if unnamed.tenant => {
                tenant = Some(UnnamedRow {
                    kind: NamedKind::Tenant,
                    id: tenant_id,
                    prior_name: payload_str(payload, "name")
                        .or_else(|| payload_str(payload, "tenant_name")),
                });
            }
            event_types::WORKAREA_CREATE => {
                if let Some(id) = payload_id(payload, "workarea_id")
                    && unnamed.workareas.contains(&id)
                {
                    if payload.get("parent_id").is_none_or(Value::is_null) {
                        root = Some(id);
                    }
                    rows.push(UnnamedRow {
                        kind: NamedKind::Workarea,
                        id,
                        prior_name: payload_str(payload, "name"),
                    });
                    crews.push((id, crew_object(payload.get("crew"))));
                }
            }
            // The latest crew object of a workarea is its current one.
            event_types::WORKAREA_CREW_UPDATED => {
                if let Some(id) = payload_id(payload, "workarea_id")
                    && let Some(object) = crew_object(payload.get("crew"))
                    && let Some(slot) = crews.iter_mut().find(|(w, _)| *w == id)
                {
                    slot.1 = Some(object);
                }
            }
            event_types::KEY_ISSUED => {
                if let Some(id) = payload_id(payload, "key_id") {
                    if payload.get("role").and_then(Value::as_str) == Some(Role::Admin.as_str()) {
                        admin_keys.insert(id);
                    }
                    if unnamed.keys.contains(&id) {
                        rows.push(UnnamedRow {
                            kind: NamedKind::Key,
                            id,
                            prior_name: payload_str(payload, "name"),
                        });
                    }
                }
            }
            event_types::KEY_DENIED => {
                if let (Some(denial), Some(key)) = (
                    payload_id(payload, "denial_id"),
                    payload_id(payload, "key_id"),
                ) {
                    denials.push((denial, key));
                }
            }
            event_types::KEY_DENIAL_LIFTED => {
                let lifted = payload_id(payload, "denial_id");
                denials.retain(|(denial, _)| Some(*denial) != lifted);
            }
            _ => {}
        }
    }
    // Rule 5: every denial on an admin key still in force is lifted.
    let lifts = denials
        .into_iter()
        .filter(|(_, key)| admin_keys.contains(key))
        .collect();
    let created = |kind: NamedKind, id: Uuid| rows.iter().any(|r| r.kind == kind && r.id == id);
    let missing = (unnamed.tenant && tenant.is_none())
        .then_some((NamedKind::Tenant, tenant_id))
        .or_else(|| {
            unnamed
                .workareas
                .iter()
                .find(|w| !created(NamedKind::Workarea, **w))
                .map(|w| (NamedKind::Workarea, *w))
        })
        .or_else(|| {
            unnamed
                .keys
                .iter()
                .find(|k| !created(NamedKind::Key, **k))
                .map(|k| (NamedKind::Key, *k))
        });
    if let Some((kind, id)) = missing {
        return Err(refuse(NamingRefusal::NoCreationEvent { kind, id }));
    }
    Ok(PredatingChain {
        tenant_id,
        tenant,
        root,
        rows,
        crews: crews
            .into_iter()
            .filter_map(|(w, object)| object.map(|o| (w, o)))
            .collect(),
        lifts,
    })
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    pub fn new(journal: J, signer: S, clock: C, ids: I, scanner: Sc) -> Self {
        Self {
            journal,
            signer,
            clock,
            ids,
            scanner,
            oauth_issuer: None,
            connect_code_failures: AtomicU64::new(0),
            observer: Arc::new(NoObserver),
            brief_max_tokens: oml_domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        }
    }

    /// The app whose workarea briefs are cut to `max_tokens`, the
    /// deployment's `brief.max_tokens` (ADR-0046 d8; FR-77).
    #[must_use]
    pub fn with_brief_max_tokens(self, max_tokens: u64) -> Self {
        Self {
            brief_max_tokens: max_tokens,
            ..self
        }
    }

    /// The app that tells `observer` what each request resolved: the key
    /// its credential named and the workarea its verb targets, which the
    /// transport's request line logs by id (ADR-0042 d2). Without it the
    /// app tells no one.
    #[must_use]
    pub fn with_observer(self, observer: Arc<dyn RequestObserver>) -> Self {
        Self { observer, ..self }
    }

    /// The app for a deployment that sets `oauth.issuer`: connect codes are
    /// minted, and their connector URL is `<issuer>/mcp` (ADR-0031 d1).
    #[must_use]
    pub fn with_oauth_issuer(self, issuer: Issuer) -> Self {
        Self {
            oauth_issuer: Some(issuer),
            ..self
        }
    }

    /// The deployment's `oauth.issuer`, `None` where it is unset: the one
    /// value the discovery documents and the OAuth endpoints are built from,
    /// never a request's `Host` (ADR-0031 d1, d3).
    #[must_use]
    pub fn oauth_issuer(&self) -> Option<&Issuer> {
        self.oauth_issuer.as_ref()
    }

    /// Wrong connect-code entries on the authorization page since this
    /// process started, across every tenant: the count doctor shows, since
    /// the guessing bound is the code's entropy and not a throttle
    /// (ADR-0031 d8, d14).
    #[must_use]
    pub fn connect_code_failures(&self) -> u64 {
        self.connect_code_failures.load(Ordering::Relaxed)
    }

    // -- internals ----------------------------------------------------------

    /// The key the value names and the grants and denials of its effective
    /// access: one hash lookup and one access read per request (workareas.md
    /// effective access; ADR-0025 d1).
    async fn authenticate(&self, api_key: &str) -> Result<Caller, AppError> {
        let key = self.key_of_value(api_key).await?;
        self.caller_of(key).await
    }

    /// The caller a key value acts as, or the caller a connection's access
    /// token resolved to, its grants and denials read again here: one key,
    /// one caller, whichever credential presented it (ADR-0031 d11).
    async fn authenticate_bearer(&self, bearer: Bearer<'_>) -> Result<Caller, AppError> {
        match bearer {
            Bearer::Key(value) => self.authenticate(value).await,
            Bearer::Connection(caller) => self.caller_of(caller.key.clone()).await,
            Bearer::Session(caller) => {
                let mut fresh = self.caller_of(caller.key.clone()).await?;
                fresh.own_keys.clone_from(&caller.own_keys);
                Ok(fresh)
            }
        }
    }

    /// The caller `key` acts as: its grants and denials, read now.
    async fn caller_of(&self, key: KeyRecord) -> Result<Caller, AppError> {
        let access = self.access_of(&key).await?;
        // A provider-level key carries no grant and no denial, since no
        // tenant's row can name it (postgres-schema.md): it holds no access,
        // so every verb refuses it by its role (ADR-0025 d13).
        let Some(access) = access else {
            return Ok(Caller {
                key,
                grants: Vec::new(),
                denials: Vec::new(),
                permission_epoch: 0,
                own_keys: Vec::new(),
            });
        };
        Ok(Caller {
            key,
            permission_epoch: access.permission_epoch(),
            grants: access.grants.into_iter().map(|g| g.grant).collect(),
            denials: access.denials.into_iter().map(|d| d.denial).collect(),
            own_keys: Vec::new(),
        })
    }

    /// The live key the value names and its access as stored, `None` for a
    /// provider-level key, which no tenant's row can name; a value that
    /// names no live key is `Unauthorized`.
    async fn key_and_access(
        &self,
        api_key: &str,
    ) -> Result<(KeyRecord, Option<KeyAccess>), AppError> {
        let key = self.key_of_value(api_key).await?;
        let access = self.access_of(&key).await?;
        Ok((key, access))
    }

    /// The live key the value names; a value that names no live key is
    /// `Unauthorized`.
    async fn key_of_value(&self, api_key: &str) -> Result<KeyRecord, AppError> {
        let hash = sha256_digest_string(api_key.as_bytes());
        let key = self.journal.key_by_hash(&hash).await;
        if let Ok(found) = &key {
            self.observer.credential(found.as_ref(), None);
        }
        key?.ok_or(AppError::Unauthorized)
    }

    /// `key`'s access as stored, `None` for a provider-level key.
    async fn access_of(&self, key: &KeyRecord) -> Result<Option<KeyAccess>, AppError> {
        if key.tenant_id == PROVIDER_TENANT {
            return Ok(None);
        }
        // The access read answers nothing only for a key its own tenant
        // does not hold: fail closed.
        let access = self
            .journal
            .key_access(key.tenant_id, key.key_id)
            .await?
            .ok_or(AppError::Unauthorized)?;
        // A user key whose row names a node that is not a user root is
        // served nothing (ADR-0044 d14).
        if let Some(root) = key.user {
            let is_root = self
                .journal
                .workarea(key.tenant_id, root)
                .await?
                .is_some_and(|w| w.user_root);
            if !is_root {
                return Err(AppError::UserRootInvalid);
            }
        }
        Ok(Some(access))
    }

    /// The id `reference` names for `key`: an id as sent; a name replaced by
    /// the id of the row of `kind` that holds it in the caller's namespace,
    /// or by its unresolved value when no row holds it. The request then
    /// runs exactly as the same request by id runs, so an unknown name and a
    /// name outside the key's access answer what the id would. Every
    /// use-case resolves after the verb ceiling, which needs no row, and
    /// before every check that reads one (http-api.md names and ids;
    /// ADR-0026 d4).
    async fn resolve(
        &self,
        key: &KeyRecord,
        kind: NamedKind,
        reference: &Reference,
    ) -> Result<Uuid, AppError> {
        self.resolve_as(key, key.user, kind, reference).await
    }

    /// [`Self::resolve`] with the workarea namespace of `user`, a user root,
    /// or the shared one for `None`: a user key's bare workarea name
    /// resolves in its own user namespace, and otherwise only as `universe`
    /// or its own root's name; a tenant key's in the shared namespace; and a
    /// qualified name `<user>.<name>` in the named user root's namespace,
    /// for every key, so resolution still confirms nothing (ADR-0044 d5).
    async fn resolve_as(
        &self,
        key: &KeyRecord,
        user: Option<Uuid>,
        kind: NamedKind,
        reference: &Reference,
    ) -> Result<Uuid, AppError> {
        // A tenant's key resolves every name in its tenant; an operator
        // key, a tenant name in the deployment and any other in the
        // provider's namespace, which no tenant read consults.
        let namespace = match (key.tenant_id == PROVIDER_TENANT, kind) {
            (false, _) => Namespace::Tenant(key.tenant_id),
            (true, NamedKind::Tenant) => Namespace::Deployment,
            (true, _) => Namespace::Provider,
        };
        let tenant_id = key.tenant_id;
        let name = match reference {
            Reference::Id(id) => return Ok(*id),
            Reference::Qualified { user, name } => {
                let resolved = match kind {
                    NamedKind::Workarea => match self
                        .journal
                        .resolve_name(tenant_id, NamedKind::Workarea, user)
                        .await?
                    {
                        Some(root) => {
                            self.journal
                                .resolve_user_workarea(tenant_id, root, name)
                                .await?
                        }
                        None => None,
                    },
                    _ => None,
                };
                return Ok(
                    resolved.unwrap_or_else(|| unresolved_qualified(namespace, kind, user, name))
                );
            }
            Reference::Name(name) => name,
        };
        let found = match (kind, user) {
            (NamedKind::Workarea, Some(root)) => {
                match self
                    .journal
                    .resolve_user_workarea(tenant_id, root, name)
                    .await?
                {
                    Some(id) => Some(id),
                    // Of the shared namespace a user key resolves only
                    // `universe` and its own root (ADR-0044 d5).
                    None => self
                        .journal
                        .resolve_name(tenant_id, kind, name)
                        .await?
                        .filter(|&id| id == root || name.as_str() == UNIVERSE),
                }
            }
            _ => self.journal.resolve_name(tenant_id, kind, name).await?,
        };
        Ok(found.unwrap_or_else(|| unresolved_value(namespace, kind, name)))
    }

    /// [`Self::resolve`] over an optional reference.
    async fn resolve_opt(
        &self,
        key: &KeyRecord,
        kind: NamedKind,
        reference: Option<&Reference>,
    ) -> Result<Option<Uuid>, AppError> {
        match reference {
            Some(reference) => Ok(Some(self.resolve(key, kind, reference).await?)),
            None => Ok(None),
        }
    }

    /// A workarea the request names, else the key's journalled default,
    /// else `request.workarea_unresolved`; called only after step 1 of the
    /// refusal order (ADR-0025 d1; ADR-0026 d4).
    async fn target_workarea(
        &self,
        key: &KeyRecord,
        named: Option<&Reference>,
    ) -> Result<Uuid, AppError> {
        let target = self
            .resolve_opt(key, NamedKind::Workarea, named)
            .await?
            .or(key.default_workarea)
            .ok_or(AppError::WorkareaUnresolved)?;
        self.observer.workarea(target);
        Ok(target)
    }

    /// Every event of the tenant's chain, in seq order.
    async fn whole_chain(&self, tenant_id: Uuid) -> Result<Vec<SignedEnvelope>, AppError> {
        let mut events = Vec::new();
        let mut from = 1_u64;
        loop {
            let page = self
                .journal
                .events_range(tenant_id, from, EVENTS_PAGE)
                .await?;
            let n = page.len();
            events.extend(page);
            if n < EVENTS_PAGE {
                return Ok(events);
            }
            from += n as u64;
        }
    }

    /// The node's ancestry as ids, node to root; empty for a node the tenant
    /// does not hold, which confers nothing.
    async fn ancestry(&self, tenant_id: Uuid, node: Uuid) -> Result<Vec<Uuid>, AppError> {
        Ok(self
            .journal
            .workarea_ancestry(tenant_id, node)
            .await?
            .map(|ancestry| ancestry.into_iter().map(|w| w.workarea_id).collect())
            .unwrap_or_default())
    }

    /// The refusal order for a verb needing grant role `needed` on `node`
    /// (roles-and-keys.md; ADR-0025 d1): verb ceiling, allowed, grant
    /// refused, not found.
    async fn require_access(
        &self,
        caller: &Caller,
        node: Uuid,
        needed: Role,
        verb: &'static str,
    ) -> Result<(), AppError> {
        let ancestry = self.ancestry(caller.key.tenant_id, node).await?;
        resolve(caller.key.role, &caller.effective(&ancestry), needed)
            .map_err(|refusal| access_refusal(refusal, node, verb))
    }

    /// Freeze as admission policy: `workarea.frozen` when the stored status
    /// of `node` refuses `op` (ADR-0009 d5). It reads the node's status and
    /// nothing else, never a grant, so freezing moves no permission epoch
    /// (ADR-0009 d2). Every caller reads the chain head before calling it,
    /// so a ratification that lands between this read and the commit moves
    /// the head and the commit conflicts rather than writing into an area
    /// validated since. Reassign, re-parent, demote and erasure are not
    /// built; their checks join here when they are.
    async fn refuse_frozen(
        &self,
        tenant_id: Uuid,
        node: Uuid,
        op: FrozenOp,
    ) -> Result<(), AppError> {
        match self.journal.workarea(tenant_id, node).await? {
            Some(w) if frozen_refuses(w.status, op) => Err(AppError::WorkareaFrozen {
                workarea_id: node,
                op,
            }),
            _ => Ok(()),
        }
    }

    /// Whether the caller can read `memory`: its own row in any state,
    /// unless a global denial masks the row's workarea, or a promoted version
    /// within its reader effective access (roles-and-keys.md#own; ADR-0025
    /// d3). Own is the authoring key, never the principal.
    async fn readable(&self, caller: &Caller, memory: &MemoryRecord) -> Result<bool, AppError> {
        let ancestry = self
            .ancestry(caller.key.tenant_id, memory.workarea_id)
            .await?;
        if memory.author_key_id == caller.key.key_id {
            return Ok(!caller.denied(&ancestry));
        }
        Ok(memory.status == MemoryStatus::Promoted
            && caller.effective(&ancestry).contains(&Role::Reader))
    }

    /// The served tip of `claim_key` in the workarea as a refusal may name it:
    /// only when the caller can read it (errors.md `submission.tip_conflict`
    /// details; ADR-0025 d10, d16).
    async fn readable_tip(
        &self,
        caller: &Caller,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> Result<Option<VersionRef>, AppError> {
        match self
            .journal
            .claim_tip(caller.key.tenant_id, workarea_id, claim_key)
            .await?
        {
            Some(tip) if self.readable(caller, &tip).await? => Ok(Some(VersionRef {
                artifact_id: tip.memory_id,
                version: tip.version,
            })),
            _ => Ok(None),
        }
    }

    /// `memory` as served to the caller: `derives_from` keeps only the
    /// ancestors `readable` accepts, so a promoted row derived from its
    /// author's held row names that row to no other key, and `replaces`
    /// obeys the same test, so one item never names in `replaces` what its
    /// `derives_from` hides (http-api.md get; ADR-0025 d3). The path is the
    /// ancestry's names from the root, joined with `.` (ADR-0025 d14).
    ///
    /// Not set-wise yet: each ancestor, the replaced version and the path
    /// are read for this one row, so recall, get and every listing issue
    /// those reads once per served row, against ADR-0024 d4 ("never N+1").
    /// Only the rows themselves hydrate set-wise (`JournalStore::memories`).
    /// The cost is unmeasured (FR-7, FR-25).
    ///
    /// A version in `universe` is served to every key outside its authoring
    /// key set without its source: no principal, no `derives_from`, its
    /// author and lineage fields blanked, and `replaces` only where it names
    /// a version in `universe` (ADR-0044 d9). The authoring key set is the
    /// calling key alone until rotation is built (roles-and-keys.md#own).
    async fn served(
        &self,
        caller: &Caller,
        mut memory: MemoryRecord,
    ) -> Result<ServedMemory, AppError> {
        let tenant_id = caller.key.tenant_id;
        let ancestry = self
            .journal
            .workarea_ancestry(tenant_id, memory.workarea_id)
            .await?;
        // `universe` is the one node with no parent.
        let anonymous = ancestry.as_ref().is_some_and(|a| a.len() == 1)
            && memory.author_key_id != caller.key.key_id;
        let workarea_path = ancestry.map(|ancestry| derived_path(&ancestry));
        let mut derives_from = Vec::with_capacity(memory.derives_from.len());
        if !anonymous {
            for ancestor in &memory.derives_from {
                if let Some(m) = self.journal.memory(tenant_id, *ancestor).await?
                    && self.readable(caller, &m).await?
                {
                    derives_from.push(*ancestor);
                }
            }
        }
        memory.derives_from = derives_from;
        let mut replaces = None;
        if let Some(id) = memory.replaces
            && let Some(m) = self.journal.memory(tenant_id, id).await?
            && (!anonymous || m.workarea_id == memory.workarea_id)
            && self.readable(caller, &m).await?
        {
            replaces = Some(VersionRef {
                artifact_id: id,
                version: m.version,
            });
        }
        if anonymous {
            memory.author_principal = String::new();
            memory.author_key_id = Uuid::nil();
            memory.lineage_principals.clear();
            memory.lineage_key_ids.clear();
        }
        Ok(ServedMemory {
            memory,
            replaces,
            workarea_path,
            anonymous,
        })
    }

    /// The nodes of the subtree at `root` where `role` is effective for the
    /// caller, parent before child: subtree(root) ∩ effective access, each
    /// node judged by the whole formula (ADR-0025 d1).
    async fn nodes_with(
        &self,
        caller: &Caller,
        root: Uuid,
        role: Role,
    ) -> Result<Vec<Uuid>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let above = self.ancestry(tenant_id, root).await?;
        if above.is_empty() {
            return Ok(Vec::new());
        }
        let mut ancestries: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::from([(root, above)]);
        let mut nodes = Vec::new();
        // The subtree lists a parent before its children, so each node's
        // ancestry is its parent's with the node in front.
        for node in self.journal.workarea_subtree(tenant_id, root).await? {
            let id = node.workarea_id;
            if id != root {
                let parent = node
                    .parent_id
                    .and_then(|p| ancestries.get(&p))
                    .ok_or_else(|| {
                        StoreError::Backend(format!(
                            "the subtree of {root} lists {id} before its parent"
                        ))
                    })?;
                let ancestry = std::iter::once(id).chain(parent.iter().copied()).collect();
                ancestries.insert(id, ancestry);
            }
            if caller.effective(&ancestries[&id]).contains(&role) {
                nodes.push(id);
            }
        }
        Ok(nodes)
    }

    /// Every node where dreamer is effective for the caller: the union over
    /// its dreamer grants' subtrees, a row outside which is never served
    /// (ADR-0025 d1).
    async fn dreamer_nodes(&self, caller: &Caller) -> Result<Vec<Uuid>, AppError> {
        self.nodes_holding(caller, Role::Dreamer).await
    }

    /// Every node where `role` is effective for the caller: the union over
    /// its `role` grants' subtrees, each node judged by [`Self::nodes_with`].
    async fn nodes_holding(&self, caller: &Caller, role: Role) -> Result<Vec<Uuid>, AppError> {
        let roots: BTreeSet<Uuid> = caller
            .grants
            .iter()
            .filter(|g| g.role == role)
            .map(|g| g.node)
            .collect();
        let mut nodes = BTreeSet::new();
        for root in roots {
            nodes.extend(self.nodes_with(caller, root, role).await?);
        }
        Ok(nodes.into_iter().collect())
    }

    async fn head_event(&self, tenant_id: Uuid) -> Result<Option<SignedEnvelope>, AppError> {
        match self.journal.chain_head(tenant_id).await? {
            None => Ok(None),
            Some(head) => Ok(self
                .journal
                .events_range(tenant_id, head.seq, 1)
                .await?
                .into_iter()
                .next()),
        }
    }

    /// `next_envelope` + signing through the signer port. The envelope is
    /// sealed over the full canonical bytes (chain-format.md).
    fn seal(
        &self,
        prev: Option<&SignedEnvelope>,
        tenant_id: Uuid,
        event_type: &str,
        principal: &str,
        api_key_id: Option<Uuid>,
        payload: Value,
    ) -> Result<SignedEnvelope, AppError> {
        let draft = EventDraft {
            tenant_id,
            event_id: self.ids.new_id(),
            event_type: event_type.to_owned(),
            principal: principal.to_owned(),
            api_key_id,
            at: self.clock.now(),
            payload,
        };
        let envelope = next_envelope(prev, draft)?;
        let signed_bytes = envelope.signed_bytes()?;
        let hash = Sha256Hash::digest(&signed_bytes);
        let sig = self.signer.sign(&signed_bytes);
        Ok(SignedEnvelope {
            envelope,
            signed_bytes,
            hash,
            sig,
        })
    }

    /// The response recorded under `scope` for this request, if any; a
    /// record of another request is `idempotency.digest_mismatch`
    /// (http-api.md idempotency).
    async fn recorded<T: DeserializeOwned>(
        &self,
        scope: &IdempotencyScope,
        request_digest: &str,
    ) -> Result<Option<T>, AppError> {
        let Some(record) = self.journal.idempotency_get(scope).await? else {
            return Ok(None);
        };
        if record.request_digest != request_digest {
            return Err(AppError::IdempotencyDigestMismatch);
        }
        serde_json::from_value(record.response)
            .map(Some)
            .map_err(|e| AppError::StoredResponse(e.to_string()))
    }

    async fn commit(
        &self,
        event: SignedEnvelope,
        fold: MetadataFold,
        idempotency: Option<IdempotencyPut>,
    ) -> Result<CommitReceipt, AppError> {
        Ok(self
            .journal
            .commit(commit_request(event, fold, idempotency))
            .await?)
    }

    /// Commits chain-consecutive events as one transaction: a refusal of
    /// any of them writes none (ADR-0025 d10).
    async fn commit_chain(
        &self,
        links: Vec<(SignedEnvelope, MetadataFold)>,
    ) -> Result<CommitReceipt, AppError> {
        Ok(self
            .journal
            .commit_chain(
                links
                    .into_iter()
                    .map(|(event, fold)| commit_request(event, fold, None))
                    .collect(),
            )
            .await?)
    }

    fn mint_key(
        &self,
        tenant_id: Uuid,
        req: &IssueKeyRequest,
        default_workarea: Option<Uuid>,
        crew_id: Option<Uuid>,
        user: Option<Uuid>,
        issued_by: Option<Uuid>,
    ) -> (KeyRecord, String) {
        // A connector key has no value and so no hash (ADR-0031 d7).
        let (credential, value) = if req.connect_code {
            (Credential::Connection, String::new())
        } else {
            let secret = self.ids.new_secret();
            (Credential::Bearer, format!("oml_{}", base62_43(&secret)))
        };
        let record = KeyRecord {
            key_id: self.ids.new_id(),
            tenant_id,
            role: req.role,
            principal: req.principal.clone(),
            // The schema default when no attestation was sent
            // (postgres-schema.md api_keys.held_by); the journal says null.
            held_by: req.held_by.unwrap_or(HeldBy::Agent),
            default_workarea,
            issued_by,
            credential,
            key_hash: (credential == Credential::Bearer)
                .then(|| sha256_digest_string(value.as_bytes())),
            name: Some(req.name.clone()),
            crew_id,
            user,
            revoked_at: None,
            session_user: None,
        };
        (record, value)
    }

    /// The catalog payload of `key.issued` (event-catalog.md): never the
    /// value (roles-and-keys.md show-once rule), and `held_by` as sent.
    fn key_issued_payload(record: &KeyRecord, held_by: Option<HeldBy>) -> Value {
        let mut payload = json!({
            "key_id": record.key_id.to_string(),
            "role": record.role.as_str(),
            "principal": record.principal,
            "issued_by_key_id": record.issued_by.map(|k| k.to_string()),
            "expiry": Value::Null,
            "value_disclosed": false,
            "held_by": held_by.map(HeldBy::as_str),
            "default_workarea_id": record.default_workarea.map(|w| w.to_string()),
            "name": record.name.as_ref().map(Name::as_str),
            "crew_id": record.crew_id.map(|c| c.to_string()),
            "credential": record.credential.as_str(),
            // The user root's id, null for a tenant key (ADR-0044 d2).
            "user": record.user.map(|u| u.to_string()),
        });
        // The person a session key serves, absent for every other key
        // (ADR-0045 d6).
        if let (Some(user), Some(obj)) = (&record.session_user, payload.as_object_mut()) {
            obj.insert("session_user".to_owned(), json!(user.as_str()));
        }
        payload
    }

    /// One `grant.issued` on `key_id`, sealed on `prev`: its payload's epoch
    /// is its own seq (event-catalog.md `grant.issued`; ADR-0025 d2).
    async fn commit_grant(
        &self,
        prev: &SignedEnvelope,
        issuer: &KeyRecord,
        key_id: Uuid,
        grant: Grant,
    ) -> Result<(SignedEnvelope, GrantRecord), AppError> {
        let event = self.grant_event(prev, issuer, key_id, &grant)?;
        let receipt = self
            .commit(
                event.clone(),
                MetadataFold::GrantIssued {
                    key_id,
                    grant: grant.clone(),
                },
                None,
            )
            .await?;
        Ok((
            event,
            GrantRecord {
                grant,
                epoch: receipt.seq,
            },
        ))
    }

    /// The sealed `grant.issued` of `grant` on `key_id`, on `prev`.
    fn grant_event(
        &self,
        prev: &SignedEnvelope,
        issuer: &KeyRecord,
        key_id: Uuid,
        grant: &Grant,
    ) -> Result<SignedEnvelope, AppError> {
        let epoch = prev.envelope.seq + 1;
        self.seal(
            Some(prev),
            issuer.tenant_id,
            event_types::GRANT_ISSUED,
            &issuer.principal,
            Some(issuer.key_id),
            json!({
                "grant_id": grant.grant_id.to_string(),
                "key_id": key_id.to_string(),
                "workarea_node_id": grant.node.to_string(),
                "role": grant.role.as_str(),
                "deny": grant
                    .deny
                    .iter()
                    .map(|d| json!({ "subtree_node_id": d.to_string() }))
                    .collect::<Vec<_>>(),
                "covers": grant.covers.as_str(),
                "epoch": epoch,
            }),
        )
    }

    /// A fresh connect code for `key_id`: its row as `connection.code_issued`
    /// folds it, holding only the code's digest, and the answer that shows
    /// the code once, with its connector URL `<issuer>/mcp`. The code is 60
    /// bits of the id generator's secret, and lives
    /// [`CONNECT_CODE_TTL_SECS`] from now (ADR-0031 d8).
    fn new_connect_code(&self, oauth: &Issuer, key_id: Uuid) -> (ConnectCode, ConnectCodeIssued) {
        let secret = self.ids.new_secret();
        let mut random = [0_u8; 8];
        random.copy_from_slice(&secret[24..]);
        let symbols = CodeSymbols::from_random(random);
        let expires_at = EventTime::from_unix_micros(
            self.clock.now().unix_micros() + CONNECT_CODE_TTL_SECS * 1_000_000,
        )
        .expect("ten minutes after a clock reading is a time the fixed-width form writes");
        let code = ConnectCode {
            code_id: self.ids.new_id(),
            key_id,
            code_hash: Sha256Hash(symbols.digest()),
            expires_at,
        };
        let shown = ConnectCodeIssued {
            code_id: code.code_id,
            code: Some(symbols.grouped()),
            expires_at: code.expires_at.clone(),
            connector_url: oauth.resource().to_owned(),
        };
        (code, shown)
    }

    /// The `key.revoked` that follows an issuance which failed after its
    /// `key.issued`, in a commit of its own, over the fixed reason
    /// (roles-and-keys.md issuance order; ADR-0025 d2).
    async fn revoke_incomplete(
        &self,
        issuer: &KeyRecord,
        key_id: Uuid,
    ) -> Result<CommitReceipt, AppError> {
        let prev = self.head_event(issuer.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            issuer.tenant_id,
            event_types::KEY_REVOKED,
            &issuer.principal,
            Some(issuer.key_id),
            json!({
                "key_id": key_id.to_string(),
                "revoked_by_key_id": issuer.key_id.to_string(),
                "reason_digest": sha256_digest_string(ISSUANCE_INCOMPLETE.as_bytes()),
            }),
        )?;
        self.commit(event, MetadataFold::KeyRevoked { key_id }, None)
            .await
    }

    /// The user root an issuance names in `user`: `user` is admitted on a
    /// reader, writer or dreamer key held by an agent or a human, and never
    /// names `universe`; a user root holding the name is joined, a shared
    /// workarea holding it is `name.taken`, and otherwise the root is to be
    /// created, under a fresh id (ADR-0044 d2, d14). Nothing is journalled
    /// here.
    async fn user_root_for(
        &self,
        issuer: &KeyRecord,
        req: &IssueKeyRequest,
        user: &Name,
    ) -> Result<UserRoot, AppError> {
        if matches!(req.role, Role::Admin | Role::Operator) {
            return Err(AppError::UserRefused(
                "user is refused on an admin or operator key, whose authority sits in no one's root",
            ));
        }
        if req.held_by == Some(HeldBy::Service) {
            return Err(AppError::UserRefused(
                "user is refused on a held_by service key, which acts for a process and not a person",
            ));
        }
        if user.as_str() == UNIVERSE {
            return Err(AppError::UserRefused(
                "user may not be universe, the tenant's root",
            ));
        }
        let tenant_id = issuer.tenant_id;
        let universe = self.universe(tenant_id).await?;
        let held = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workarea, user)
            .await?;
        let (id, created) = match held {
            None => (self.ids.new_id(), true),
            Some(id) => {
                let root = self.journal.workarea(tenant_id, id).await?;
                if !root.is_some_and(|w| w.user_root) {
                    return Err(AppError::NameTaken {
                        kind: NamedKind::Workarea,
                        name: user.clone(),
                    });
                }
                (id, false)
            }
        };
        Ok(UserRoot {
            id,
            name: user.clone(),
            universe,
            created,
        })
    }

    /// A workarea reference of an issuance, in the namespace of the user
    /// it names, the user's own name naming its root even before the
    /// issuance creates it (ADR-0044 d5, d11); with no user, the issuer's.
    async fn resolve_for(
        &self,
        issuer: &KeyRecord,
        user: Option<&UserRoot>,
        reference: &Reference,
    ) -> Result<Uuid, AppError> {
        match (user, reference) {
            (Some(u), Reference::Name(name)) if *name == u.name => Ok(u.id),
            (Some(u), _) => {
                self.resolve_as(issuer, Some(u.id), NamedKind::Workarea, reference)
                    .await
            }
            (None, _) => self.resolve(issuer, NamedKind::Workarea, reference).await,
        }
    }

    /// A node's ancestry as an issuance sees it: a user root it is to
    /// create sits directly under `universe` (ADR-0044 d1).
    async fn issuance_ancestry(
        &self,
        tenant_id: Uuid,
        user: Option<&UserRoot>,
        node: Uuid,
    ) -> Result<Vec<Uuid>, AppError> {
        match user {
            Some(u) if u.created && node == u.id => Ok(vec![u.id, u.universe]),
            _ => self.ancestry(tenant_id, node).await,
        }
    }

    /// Journal `workarea.create` of a user root under `universe`, the
    /// issuing admin as actor, `user_root: true` in its payload (ADR-0044
    /// d1, d2).
    async fn create_user_root(&self, issuer: &KeyRecord, user: &UserRoot) -> Result<(), AppError> {
        let workarea = WorkareaRecord {
            workarea_id: user.id,
            tenant_id: issuer.tenant_id,
            parent_id: Some(user.universe),
            name: user.name.clone(),
            crew: None,
            crew_id: None,
            user_root: true,
            namespace: None,
        };
        let prev = self.head_event(issuer.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            issuer.tenant_id,
            event_types::WORKAREA_CREATE,
            &issuer.principal,
            Some(issuer.key_id),
            Self::workarea_create_payload(&workarea),
        )?;
        self.commit(event, MetadataFold::WorkareaCreated(workarea), None)
            .await?;
        Ok(())
    }

    /// The catalog payload of `workarea.create` (event-catalog.md), with
    /// `user_root` (ADR-0044 d1).
    fn workarea_create_payload(workarea: &WorkareaRecord) -> Value {
        json!({
            "workarea_id": workarea.workarea_id.to_string(),
            "parent_id": workarea.parent_id.map(|p| p.to_string()),
            "name": workarea.name.as_str(),
            "status": workarea.status(),
            "crew_id": workarea.crew_id.map(|c| c.to_string()),
            "prefix": Value::Null,
            "user_root": workarea.user_root,
        })
    }

    /// A key's user root by id and name; `None` for a tenant key (ADR-0044
    /// d11).
    async fn user_ref(
        &self,
        tenant_id: Uuid,
        user: Option<Uuid>,
    ) -> Result<Option<NamedRef>, AppError> {
        let Some(root) = user else {
            return Ok(None);
        };
        let workarea = self
            .journal
            .workarea(tenant_id, root)
            .await?
            .ok_or_else(|| StoreError::Backend(format!("user root {root} is not held")))?;
        Ok(Some(NamedRef {
            id: root,
            name: workarea.name,
        }))
    }

    /// The tenant's `universe` root (ADR-0026 d6).
    async fn universe(&self, tenant_id: Uuid) -> Result<Uuid, AppError> {
        let universe_name = Name::parse(UNIVERSE).expect("universe is a name (ADR-0026 d6)");
        Ok(self
            .journal
            .resolve_name(tenant_id, NamedKind::Workarea, &universe_name)
            .await?
            .ok_or_else(|| StoreError::Backend(format!("tenant {tenant_id} has no universe")))?)
    }

    // -- use-cases ----------------------------------------------------------

    /// The operator ceremony: genesis event, the `universe` root workarea,
    /// first admin key (roles-and-keys.md bootstrap; workareas.md, one
    /// universe per tenant). The root's name is the first segment of every
    /// served `workarea_path` (ADR-0025 d14). Unauthenticated by design — the
    /// composition decides who may run it.
    ///
    /// The tenant's partitions come first, created by the store before
    /// `tenant.genesis` is sealed, with no commit of this act in flight: a
    /// creation that fails fails the act, journals nothing and creates no
    /// tenant row; partitions a failed genesis commit leaves are kept, and a
    /// retry mints a fresh id (postgres-schema.md, the per-tenant
    /// partitions; ADR-0034 d11).
    pub async fn bootstrap_tenant(
        &self,
        req: &BootstrapRequest,
    ) -> Result<BootstrapResult, AppError> {
        let tenant_id = self.ids.new_id();
        self.journal.create_tenant_partitions(tenant_id).await?;
        let genesis = self.seal(
            None,
            tenant_id,
            event_types::TENANT_GENESIS,
            "operator",
            None,
            json!({ "tenant_id": tenant_id.to_string(), "name": req.tenant_name.as_str() }),
        )?;
        let mut prev = genesis.clone();
        let fold = MetadataFold::TenantGenesis {
            name: req.tenant_name.clone(),
        };
        self.commit(genesis, fold, None).await?;

        let root_workarea_id = self.ids.new_id();
        let workarea = WorkareaRecord {
            workarea_id: root_workarea_id,
            tenant_id,
            parent_id: None,
            name: Name::parse(UNIVERSE).expect("universe is a name (ADR-0026 d6)"),
            // The universe root is permanently committed (ADR-0009 d1).
            status: WorkareaStatus::Committed,
            crew: None,
            crew_id: None,
            user_root: false,
            namespace: None,
        };
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::WORKAREA_CREATE,
            "operator",
            None,
            json!({
                "workarea_id": root_workarea_id.to_string(),
                "parent_id": Value::Null,
                "name": workarea.name.as_str(),
                "status": workarea.status.as_str(),
                "crew_id": Value::Null,
                "prefix": Value::Null,
            }),
        )?;
        prev = event.clone();
        self.commit(event, MetadataFold::WorkareaCreated(workarea), None)
            .await?;

        // The ceremony's admin key takes the name the claim gives it
        // (ADR-0026 d9).
        let issue = IssueKeyRequest {
            role: Role::Admin,
            name: req.admin_key_name.clone(),
            principal: req.admin_principal.clone(),
            held_by: Some(HeldBy::Human),
            default_workarea: Some(root_workarea_id.into()),
            crew: None,
            grants: Vec::new(),
            user: None,
            connect_code: false,
        };
        let (record, value) =
            self.mint_key(tenant_id, &issue, Some(root_workarea_id), None, None, None);
        let key_id = record.key_id;
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::KEY_ISSUED,
            "operator",
            None,
            Self::key_issued_payload(&record, issue.held_by),
        )?;
        let receipt = self
            .commit(event, MetadataFold::KeyIssued(record), None)
            .await?;

        Ok(BootstrapResult {
            tenant_id,
            root_workarea_id,
            admin_key: IssuedKey {
                key_id,
                value,
                role: issue.role,
                principal: issue.principal,
                held_by: issue.held_by,
                default_workarea: Some(root_workarea_id),
                name: issue.name,
                crew_id: None,
                user: None,
                access: KeyAccess {
                    issued_seq: receipt.seq,
                    grants: Vec::new(),
                    denials: Vec::new(),
                    lift_epochs: Vec::new(),
                },
                connect_code: None,
            },
        })
    }

    /// Issue a key with its grants, validated in the issuance order of
    /// roles-and-keys.md (ADR-0025 d2): the two-key rule, the issuer ceiling,
    /// grant admissibility, node and crew existence, deny containment, the
    /// default's coverage, with `connect_code` whether the key can hold a
    /// connection, and last the key's name. Every workarea and crew
    /// reference is resolved, in the issuer's tenant, once the issuer's role
    /// admits the verb and before step 1, so an unknown name fails step 4 as
    /// an unknown id does (ADR-0026 d4, d8). `key.issued`, journalling the
    /// key's name and crew, commits first, and a name the tenant already
    /// holds refuses that commit with `name.taken`, journalling nothing
    /// (ADR-0026 d5, d9); one `grant.issued` per grant follows, and the value
    /// is returned only after the last of them commits. An issuance that
    /// fails after `key.issued` journals `key.revoked` over the fixed reason
    /// and answers the failure: the value was never disclosed, so the key
    /// authenticates nothing either way.
    ///
    /// With `connect_code`, step 7 refuses a key role of `admin` or
    /// `operator`, `held_by: service` and a deployment without `oauth.issuer`
    /// with `oauth.connect_refused`, by that reason. The key then has no
    /// value: `key.issued` journals its credential `connection`, and it,
    /// every `grant.issued` and `connection.code_issued` commit as one
    /// chain, so a refusal of any of them, `name.taken` included, journals
    /// none. The code is returned last, in the value's place
    /// (roles-and-keys.md issuance order, steps 7 and 8; ADR-0031 d7, d8).
    pub async fn issue_key(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &IssueKeyRequest,
    ) -> Result<IssuedKey, AppError> {
        let issuer = self.authenticate_bearer(bearer.into()).await?.key;
        if issuer.role != Role::Admin && issuer.role != Role::Operator {
            return Err(AppError::VerbCeiling {
                role: issuer.role,
                verb: "issue_key",
            });
        }
        self.issue_key_by(&issuer, req, Issuance::Plain).await
    }

    /// [`App::issue_key`] once the issuer is known and its role checked for
    /// `how`: a plain issuance by an admin or operator key; a session key
    /// for `session_user`, with no value (ADR-0045 d6); or a person's
    /// `<user>-web` issuing a key in its own root, which the web use case
    /// has bounded to reader and writer keys naming its user (ADR-0045 d9,
    /// d16), the issuer ceiling read as an admin's for those.
    pub(crate) async fn issue_key_by(
        &self,
        issuer: &KeyRecord,
        req: &IssueKeyRequest,
        how: Issuance<'_>,
    ) -> Result<IssuedKey, AppError> {
        let issuer = issuer.clone();
        let tenant_id = issuer.tenant_id;
        // The person the key acts for, and the root its references resolve
        // in, found or to be created (ADR-0044 d2, d5).
        let user = match &req.user {
            Some(user) => Some(self.user_root_for(&issuer, req, user).await?),
            None => None,
        };
        let mut grants = Vec::with_capacity(req.grants.len() + 1);
        for g in &req.grants {
            let node = self.resolve_for(&issuer, user.as_ref(), &g.node).await?;
            let mut deny = Vec::with_capacity(g.deny.len());
            for d in &g.deny {
                deny.push(self.resolve_for(&issuer, user.as_ref(), d).await?);
            }
            grants.push(Grant {
                grant_id: self.ids.new_id(),
                node,
                role: g.role,
                deny,
                covers: g.covers,
            });
        }
        let default_workarea = match &req.default_workarea {
            Some(sent) => Some(self.resolve_for(&issuer, user.as_ref(), sent).await?),
            None => None,
        };
        let crew_id = self
            .resolve_opt(&issuer, NamedKind::Crew, req.crew.as_ref())
            .await?;
        // Steps 1–3: the two-key rule, the issuer ceiling, admissibility.
        let ceiling = match how {
            Issuance::PersonsOwn => Role::Admin,
            Issuance::Plain | Issuance::Session(_) => issuer.role,
        };
        check_issuance(ceiling, req.role, &grants).map_err(issuance_refusal)?;
        // Step 4: every grant node and deny node is the tenant's, and so is
        // the crew record. A user root this issuance is to create sits
        // directly under `universe` (ADR-0044 d1).
        let mut ancestries: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
        for node in grants
            .iter()
            .flat_map(|g| std::iter::once(g.node).chain(g.deny.iter().copied()))
        {
            if let Entry::Vacant(slot) = ancestries.entry(node) {
                let ancestry = self
                    .issuance_ancestry(tenant_id, user.as_ref(), node)
                    .await?;
                if ancestry.is_empty() {
                    return Err(AppError::NotFound);
                }
                slot.insert(ancestry);
            }
        }
        if let Some(crew_id) = crew_id
            && self.journal.crew(tenant_id, crew_id).await?.is_none()
        {
            return Err(AppError::NotFound);
        }
        // A user key's every grant lies in its user's root (ADR-0044 d3).
        if let Some(u) = &user
            && let Some((grant_index, outside)) = grants
                .iter()
                .enumerate()
                .find(|(_, g)| !ancestries[&g.node].contains(&u.id))
        {
            return Err(AppError::GrantOutsideUserRoot {
                role: outside.role,
                admissible: admissible_grant_roles(req.role),
                grant_index,
                user: u.name.clone(),
            });
        }
        // Step 5: each deny node lies inside its own grant's subtree, and a
        // node grant takes none (ADR-0044 d4); the refusal echoes the deny
        // as the request spelled it.
        for (grant_index, grant) in grants.iter().enumerate() {
            if grant.covers == Covers::Node && !grant.deny.is_empty() {
                return Err(AppError::NodeGrantDeny { grant_index });
            }
            if let Some(outside) = grant
                .deny
                .iter()
                .position(|d| !ancestries[d].contains(&grant.node))
            {
                return Err(AppError::DenyOutsideGrant {
                    grant_index,
                    subtree_node: req.grants[grant_index].deny[outside].clone(),
                });
            }
        }
        // A user key reads `universe`'s own claims through one reader grant
        // on that node alone, which the request did not send (ADR-0044 d3).
        if let Some(u) = &user {
            grants.push(Grant {
                grant_id: self.ids.new_id(),
                node: u.universe,
                role: Role::Reader,
                deny: Vec::new(),
                covers: Covers::Node,
            });
        }
        // Step 6: a sent default lies in some grant's reach and outside
        // that grant's denies; absent, it is the first grant's node, and
        // never the issuer's (workareas.md per-grant attenuation).
        let default_workarea = match (default_workarea, &req.default_workarea) {
            (Some(workarea_id), Some(sent)) => {
                let ancestry = self
                    .issuance_ancestry(tenant_id, user.as_ref(), workarea_id)
                    .await?;
                let covered = grants
                    .iter()
                    .any(|g| g.reaches(&ancestry) && !g.deny.iter().any(|d| ancestry.contains(d)));
                if !covered {
                    return Err(AppError::DefaultUncovered {
                        workarea: sent.clone(),
                    });
                }
                Some(workarea_id)
            }
            _ => grants.first().map(|g| g.node),
        };

        // Step 7: with `connect_code`, the key can hold a connection — a
        // role other than admin or operator, `held_by` other than service,
        // and a deployment that sets `oauth.issuer` (ADR-0031 d7).
        let oauth = if req.connect_code {
            if matches!(req.role, Role::Admin | Role::Operator) {
                return Err(connect_refused(ConnectRefused::Role));
            }
            if req.held_by == Some(HeldBy::Service) {
                return Err(connect_refused(ConnectRefused::HeldByService));
            }
            Some(
                self.oauth_issuer
                    .as_ref()
                    .ok_or_else(|| connect_refused(ConnectRefused::NotConfigured))?,
            )
        } else {
            None
        };

        // The first key naming a user creates its root, in a commit of its
        // own before `key.issued`; an issuance refused after it leaves the
        // root, empty, for the next one to join (ADR-0044 d2; R4).
        if let Some(u) = &user
            && u.created
        {
            self.create_user_root(&issuer, u).await?;
        }
        let user_ref = user.as_ref().map(|u| NamedRef {
            id: u.id,
            name: u.name.clone(),
        });

        // Step 8, the name, is the unique index's, at the commit of
        // `key.issued` below.
        let (mut record, mut value) = self.mint_key(
            tenant_id,
            req,
            default_workarea,
            crew_id,
            user.as_ref().map(|u| u.id),
            Some(issuer.key_id),
        );
        // A session key has no value and so no hash: nothing can present it
        // (ADR-0045 d6).
        if let Issuance::Session(session_user) = how {
            record.credential = Credential::Session;
            record.key_hash = None;
            record.session_user = Some(session_user.clone());
            value = String::new();
        }
        let key_id = record.key_id;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_ISSUED,
            &issuer.principal,
            Some(issuer.key_id),
            Self::key_issued_payload(&record, req.held_by),
        )?;
        let mut last = event.clone();
        if let Some(oauth) = oauth {
            // One chain: a refusal of any link journals none, so no key
            // outlives a failed issuance and none needs revoking.
            let issued_seq = event.envelope.seq;
            let mut links = vec![(event, MetadataFold::KeyIssued(record))];
            let mut stored = Vec::with_capacity(grants.len());
            for grant in grants {
                let event = self.grant_event(&last, &issuer, key_id, &grant)?;
                stored.push(GrantRecord {
                    grant: grant.clone(),
                    epoch: event.envelope.seq,
                });
                last = event.clone();
                links.push((event, MetadataFold::GrantIssued { key_id, grant }));
            }
            let (code, shown) = self.new_connect_code(oauth, key_id);
            let event = self.seal(
                Some(&last),
                tenant_id,
                event_types::CONNECTION_CODE_ISSUED,
                &issuer.principal,
                Some(issuer.key_id),
                code_issued_payload(&code, issuer.key_id, None),
            )?;
            links.push((event, MetadataFold::ConnectCodeIssued { code, voids: None }));
            self.commit_chain(links).await?;
            return Ok(IssuedKey {
                key_id,
                value,
                role: req.role,
                principal: req.principal.clone(),
                held_by: req.held_by,
                default_workarea,
                name: req.name.clone(),
                crew_id,
                user: user_ref.clone(),
                access: KeyAccess {
                    issued_seq,
                    grants: stored,
                    denials: Vec::new(),
                    lift_epochs: Vec::new(),
                },
                connect_code: Some(shown),
            });
        }
        let receipt = self
            .commit(event, MetadataFold::KeyIssued(record), None)
            .await?;
        let mut stored = Vec::with_capacity(grants.len());
        for grant in grants {
            match self.commit_grant(&last, &issuer, key_id, grant).await {
                Ok((event, record)) => {
                    last = event;
                    stored.push(record);
                }
                Err(failure) => {
                    // The failure answers whether or not the revocation
                    // commits: one that cannot leaves a key whose value
                    // nobody holds, which authenticates nothing.
                    let _revocation = self.revoke_incomplete(&issuer, key_id).await;
                    return Err(failure);
                }
            }
        }
        Ok(IssuedKey {
            key_id,
            value,
            role: req.role,
            principal: req.principal.clone(),
            held_by: req.held_by,
            default_workarea,
            name: req.name.clone(),
            crew_id,
            user: user_ref.clone(),
            access: KeyAccess {
                issued_seq: receipt.seq,
                grants: stored,
                denials: Vec::new(),
                lift_epochs: Vec::new(),
            },
            connect_code: None,
        })
    }

    /// Journal a global per-key denial on `key` over the subtree at `node`,
    /// each by id or name: deny-wins across all of the key's grants, its
    /// epoch its own `key.denied` seq. Admin key role only; a key or node the
    /// tenant does not hold is absent (http-api.md keys, grants, denials;
    /// ADR-0025 d2), and a denial on an admin key, which carries no grant to
    /// attenuate, is refused (ADR-0026 d15).
    pub async fn deny_key(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
        node: Reference,
    ) -> Result<DenialRecord, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "deny")?;
        self.deny_key_by(&admin, key, node).await
    }

    /// [`App::deny_key`] once the issuer is known: an admin key, or a
    /// person's `<user>-web` session key cutting off a key naming its user,
    /// which the web use case has checked (ADR-0045 d9, d13, d16).
    pub(crate) async fn deny_key_by(
        &self,
        admin: &KeyRecord,
        key: Reference,
        node: Reference,
    ) -> Result<DenialRecord, AppError> {
        let admin = admin.clone();
        let tenant_id = admin.tenant_id;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        let node = self.resolve(&admin, NamedKind::Workarea, &node).await?;
        let Some(target) = self.journal.key(tenant_id, key_id).await? else {
            return Err(AppError::NotFound);
        };
        if self.journal.workarea(tenant_id, node).await?.is_none() {
            return Err(AppError::NotFound);
        }
        if target.role == Role::Admin {
            return Err(AppError::DenialOnAdminKey);
        }
        let denial = KeyDenial {
            denial_id: self.ids.new_id(),
            node,
        };
        let prev = self.head_event(tenant_id).await?;
        let epoch = prev.as_ref().map_or(1, |p| p.envelope.seq + 1);
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_DENIED,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "denial_id": denial.denial_id.to_string(),
                "key_id": key_id.to_string(),
                "workarea_node_id": node.to_string(),
                "epoch": epoch,
            }),
        )?;
        let receipt = self
            .commit(
                event,
                MetadataFold::KeyDenied {
                    key_id,
                    denial: denial.clone(),
                    created_by: admin.key_id,
                },
                None,
            )
            .await?;
        Ok(DenialRecord {
            denial,
            created_by: admin.key_id,
            epoch: receipt.seq,
        })
    }

    /// Create a node under `parent`, admin by key role, the parent and the
    /// crew each by id or name. A parent the tenant does not hold is absent,
    /// and so, checked after it, is a `crew` that names no crew record of
    /// the tenant (ADR-0026 d8). The crew is recorded, not operated:
    /// `workarea.create` journals its `crew_id` and the fold stores it, so
    /// the workarea read serves the record from the store (event-catalog.md
    /// `workarea.create`; ADR-0025 d14). The answer carries the node's path
    /// derived from the parent's stored ancestry, as the read derives it. A
    /// name the namespace already holds, `universe` included, is checked
    /// last, by the store's unique index: `name.taken`, and nothing is
    /// journalled (ADR-0026 d5, d6). A writer user key creates too, under a
    /// parent inside its own root where its effective access confers
    /// writer, the root itself when `parent` is omitted, by the refusal
    /// order on the parent; the node's name lands in that root's namespace
    /// (ADR-0044 d5, d6). A parent that is `validated` refuses the child as
    /// `workarea.frozen` after both lookups and before the name, so a frozen
    /// node never acquires an unfrozen descendant (ADR-0009 d4, d5).
    pub async fn create_workarea(
        &self,
        bearer: impl Into<Bearer<'_>>,
        name: &Name,
        parent: Option<Reference>,
        crew: Option<Reference>,
    ) -> Result<CreatedWorkarea, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        // A user key with the writer role creates inside its own root, by
        // its effective access on the parent; every other key by the admin
        // key role (ADR-0044 d6).
        let parent_id = match key.user {
            Some(root) if key.role == Role::Writer => {
                // Crew metadata stays a governed admin act (ADR-0044 d6).
                if crew.is_some() {
                    return Err(AppError::VerbCeiling {
                        role: key.role,
                        verb: "set_workarea_crew",
                    });
                }
                let parent_id = match &parent {
                    Some(parent) => self.resolve(key, NamedKind::Workarea, parent).await?,
                    None => root,
                };
                self.require_access(&caller, parent_id, Role::Writer, "create_workarea")
                    .await?;
                Some(parent_id)
            }
            _ => {
                require_role(key, Role::Admin, "create_workarea")?;
                self.resolve_opt(key, NamedKind::Workarea, parent.as_ref())
                    .await?
            }
        };
        let crew_id = self
            .resolve_opt(key, NamedKind::Crew, crew.as_ref())
            .await?;
        // The head before the parent's status: a ratification landing in
        // between moves it, and this commit conflicts (ADR-0009 d4).
        let prev = self.head_event(key.tenant_id).await?;
        let ancestors = match parent_id {
            Some(parent) => self
                .journal
                .workarea_ancestry(key.tenant_id, parent)
                .await?
                .ok_or(AppError::NotFound)?,
            None => Vec::new(),
        };
        let crew = match crew_id {
            Some(crew_id) => Some(
                self.journal
                    .crew(key.tenant_id, crew_id)
                    .await?
                    .ok_or(AppError::NotFound)?,
            ),
            None => None,
        };
        if let Some(parent) = ancestors.first()
            && frozen_refuses(parent.status, FrozenOp::CreateChild)
        {
            return Err(AppError::WorkareaFrozen {
                workarea_id: parent.workarea_id,
                op: FrozenOp::CreateChild,
            });
        }
        // A node beneath a user root takes its name in that root's
        // namespace, where `universe` and the user's own name are reserved
        // (ADR-0044 d5).
        let namespace = ancestors.first().and_then(WorkareaRecord::user);
        if let Some(root) = namespace
            && ancestors
                .iter()
                .any(|w| w.workarea_id == root && &w.name == name)
        {
            return Err(AppError::NameTaken {
                kind: NamedKind::Workarea,
                name: name.clone(),
            });
        }
        let workarea = WorkareaRecord {
            workarea_id: self.ids.new_id(),
            tenant_id: key.tenant_id,
            parent_id,
            name: name.clone(),
            status: WorkareaStatus::Development,
            crew: None,
            crew_id,
            user_root: false,
            namespace,
        };
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::WORKAREA_CREATE,
            &key.principal,
            Some(key.key_id),
            Self::workarea_create_payload(&workarea),
        )?;
        self.commit(event, MetadataFold::WorkareaCreated(workarea.clone()), None)
            .await?;
        let ancestry: Vec<WorkareaRecord> =
            std::iter::once(workarea.clone()).chain(ancestors).collect();
        Ok(CreatedWorkarea {
            path: derived_path(&ancestry),
            workarea,
            crew,
        })
    }

    /// One workarea as the calling key may see it, at reader effective
    /// access by the refusal order and otherwise absent (http-api.md
    /// workareas; ADR-0025 d14): the node and its derived path, the crew
    /// record it names, the immediate children the key can read, the verbs
    /// its effective roles on the node confer, and the permission epoch of
    /// the access this request read. An admin key reads it by its key role,
    /// with no grant and no denial entering: every child, no effective verb,
    /// and nothing of the node's claims (roles-and-keys.md permission matrix,
    /// footnote 6; ADR-0026 d15). The node, by id or name, or else the key's
    /// journalled default, is resolved only after step 1 of the refusal
    /// order, as recall resolves it (roles-and-keys.md refusal order;
    /// mcp-plugin.md `oml_workarea_info`; ADR-0026 d4).
    pub async fn workarea_info(
        &self,
        bearer: impl Into<Bearer<'_>>,
        node: Option<Reference>,
    ) -> Result<WorkareaInfo, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let structure = caller.key.role == Role::Admin;
        if !structure {
            require_admissible(&caller.key, Role::Reader, "workarea_info")?;
        }
        let node = self.target_workarea(&caller.key, node.as_ref()).await?;
        let tenant_id = caller.key.tenant_id;
        let ancestry = self
            .journal
            .workarea_ancestry(tenant_id, node)
            .await?
            .unwrap_or_default();
        let ids: Vec<Uuid> = ancestry.iter().map(|w| w.workarea_id).collect();
        let effective = caller.effective(&ids);
        if !structure {
            resolve(caller.key.role, &effective, Role::Reader)
                .map_err(|refusal| access_refusal(refusal, node, "workarea_info"))?;
        }
        let workarea = ancestry.first().cloned().ok_or(AppError::NotFound)?;
        let mut children = Vec::new();
        // A child's ancestry is the node's with the child in front.
        for child in self.journal.workarea_children(tenant_id, node).await? {
            let child_ancestry: Vec<Uuid> = std::iter::once(child.workarea_id)
                .chain(ids.iter().copied())
                .collect();
            if structure || caller.effective(&child_ancestry).contains(&Role::Reader) {
                children.push(child);
            }
        }
        let crew = match workarea.crew_id {
            Some(crew_id) => self.journal.crew(tenant_id, crew_id).await?,
            None => None,
        };
        let pending_transitions = self.live_transitions(&caller.key, &workarea).await?;
        // An admin key reads structure and never claim content, so its
        // brief is empty (roles-and-keys.md permission matrix, footnote 6).
        let brief = if structure {
            Brief {
                items: Vec::new(),
                budget: brief_cut(&[], self.brief_max_tokens),
            }
        } else {
            self.brief(&caller, node).await?
        };
        Ok(WorkareaInfo {
            path: derived_path(&ancestry),
            policy: review_policy(),
            pending_transitions,
            brief,
            workarea,
            crew,
            children,
            // The field lists grant-role verbs, and an admin key holds none.
            my_effective_verbs: if structure {
                Vec::new()
            } else {
                effective_verbs(&effective, caller.key.held_by)
            },
            permission_epoch: caller.permission_epoch,
        })
    }

    /// One page of the subtree at `node`, by id or name, in path order: a
    /// node before its children, and siblings by name in byte order, so the
    /// derived paths' names, compared one by one, order the walk. Each node
    /// carries its derived path and the crew record it names. A key past
    /// step 1 needs reader effective access on `node` by the refusal order,
    /// and the nodes outside its read access are omitted, a node below one
    /// it cannot read included when a grant reaches it; an admin key reads
    /// every node by its key role (roles-and-keys.md permission matrix,
    /// footnote 6; ADR-0026 d15). A page holds at most `limit` nodes, after
    /// the path `cursor` resumes at, and the cursor of the next page, bound
    /// to the key and to `api.cursor_ttl` like every listing cursor; a stale
    /// or foreign one is refused (http-api.md workareas and pagination). The
    /// read is one store walk and one crew read per crew the page names.
    pub async fn workarea_tree(
        &self,
        bearer: impl Into<Bearer<'_>>,
        node: Reference,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<TreePage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let structure = caller.key.role == Role::Admin;
        if !structure {
            require_admissible(&caller.key, Role::Reader, "workarea_tree")?;
        }
        let now = self.clock.now();
        let after = cursor
            .map(|cursor| PathCursor::resume(cursor, caller.key.key_id, &now))
            .transpose()?;
        let tenant_id = caller.key.tenant_id;
        let node = self
            .resolve(&caller.key, NamedKind::Workarea, &node)
            .await?;
        self.observer.workarea(node);
        let readable: Option<BTreeSet<Uuid>> = if structure {
            None
        } else {
            self.require_access(&caller, node, Role::Reader, "workarea_tree")
                .await?;
            Some(
                self.nodes_with(&caller, node, Role::Reader)
                    .await?
                    .into_iter()
                    .collect(),
            )
        };
        let Some(ancestry) = self.journal.workarea_ancestry(tenant_id, node).await? else {
            return Err(AppError::NotFound);
        };
        // The subtree lists a parent before its children, so each node's
        // names, from `universe` down, are its parent's and its own.
        let mut names: BTreeMap<Uuid, Vec<Name>> = BTreeMap::from([(
            node,
            ancestry.iter().rev().map(|w| w.name.clone()).collect(),
        )]);
        let mut tree: Vec<(Vec<Name>, WorkareaRecord)> = Vec::new();
        for workarea in self.journal.workarea_subtree(tenant_id, node).await? {
            let id = workarea.workarea_id;
            if id != node {
                let mut own = workarea
                    .parent_id
                    .and_then(|p| names.get(&p))
                    .cloned()
                    .ok_or_else(|| {
                        StoreError::Backend(format!(
                            "the subtree of {node} lists {id} before its parent"
                        ))
                    })?;
                own.push(workarea.name.clone());
                names.insert(id, own);
            }
            if readable.as_ref().is_none_or(|nodes| nodes.contains(&id)) {
                tree.push((names[&id].clone(), workarea));
            }
        }
        // A node's names are a prefix of its children's, and names are
        // unique in the tenant, so no two nodes tie.
        tree.sort_by(|(a, _), (b, _)| a.cmp(b));
        // One node past the page says whether another page exists.
        let mut page: Vec<(Vec<Name>, WorkareaRecord)> = tree
            .into_iter()
            .filter(|(names, _)| after.as_ref().is_none_or(|after| names > after))
            .take(limit.saturating_add(1))
            .collect();
        let more = page.len() > limit;
        page.truncate(limit);
        let next_cursor = page.last().filter(|_| more).map(|(names, _)| {
            PathCursor {
                key_id: caller.key.key_id,
                after: names.clone(),
                issued_at: now.clone(),
            }
            .encode()
        });
        // Many nodes may name one crew, which is read once.
        let mut crews: BTreeMap<Uuid, Option<CrewRecord>> = BTreeMap::new();
        let mut items = Vec::with_capacity(page.len());
        for (names, workarea) in page {
            let path = names.iter().map(Name::as_str).collect::<Vec<_>>().join(".");
            let crew = match workarea.crew_id {
                None => None,
                Some(crew_id) => match crews.entry(crew_id) {
                    Entry::Occupied(read) => read.get().clone(),
                    Entry::Vacant(unread) => unread
                        .insert(self.journal.crew(tenant_id, crew_id).await?)
                        .clone(),
                },
            };
            items.push(TreeNode {
                workarea,
                path,
                crew,
            });
        }
        Ok(TreePage { items, next_cursor })
    }

    /// Create a crew record, admin by key role (http-api.md crews;
    /// ADR-0026 d8), journalled as `crew.created` with its name; a name the
    /// tenant already holds is `name.taken`, and nothing is journalled.
    pub async fn create_crew(
        &self,
        api_key: &str,
        req: &CrewCreateRequest,
    ) -> Result<CrewRecord, AppError> {
        let key = self.authenticate(api_key).await?.key;
        require_role(&key, Role::Admin, "create_crew")?;
        let crew = CrewRecord {
            crew_id: self.ids.new_id(),
            tenant_id: key.tenant_id,
            name: req.name.clone(),
            charter_uri: req.charter_uri.clone(),
            external_manager: req.external_manager.clone(),
        };
        let prev = self.head_event(key.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::CREW_CREATED,
            &key.principal,
            Some(key.key_id),
            json!({
                "crew_id": crew.crew_id.to_string(),
                "name": crew.name.as_str(),
                "charter_uri": crew.charter_uri,
                "external_manager": crew.external_manager,
                "created_by_key_id": key.key_id.to_string(),
            }),
        )?;
        self.commit(event, MetadataFold::CrewCreated(crew.clone()), None)
            .await?;
        Ok(crew)
    }

    /// One crew record, by id or name, admin by key role; one the tenant
    /// does not hold is absent (http-api.md crews; ADR-0026 d8).
    pub async fn crew(&self, api_key: &str, crew: Reference) -> Result<CrewRecord, AppError> {
        let key = self.authenticate(api_key).await?.key;
        require_role(&key, Role::Admin, "crew")?;
        let crew_id = self.resolve(&key, NamedKind::Crew, &crew).await?;
        self.journal
            .crew(key.tenant_id, crew_id)
            .await?
            .ok_or(AppError::NotFound)
    }

    /// One page of the tenant's crew records in name order, admin by key
    /// role: at most `limit` of them, after the name `cursor` resumes at,
    /// and the cursor of the next page, bound to the key and to
    /// `api.cursor_ttl` like every listing cursor; a stale or foreign one
    /// is refused (http-api.md crews and pagination; ADR-0026 d8). The read
    /// is one store query.
    pub async fn crews(
        &self,
        api_key: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<CrewPage, AppError> {
        let key = self.authenticate(api_key).await?.key;
        require_role(&key, Role::Admin, "crews")?;
        let now = self.clock.now();
        let after = cursor
            .map(|cursor| NameCursor::resume(cursor, key.key_id, &now))
            .transpose()?;
        // One row past the page says whether another page exists.
        let mut items: Vec<CrewRecord> = self
            .journal
            .crews(key.tenant_id)
            .await?
            .into_iter()
            .filter(|crew| after.as_ref().is_none_or(|after| crew.name > *after))
            .take(limit.saturating_add(1))
            .collect();
        let more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = items.last().filter(|_| more).map(|last| {
            NameCursor {
                key_id: key.key_id,
                after: last.name.clone(),
                issued_at: now.clone(),
            }
            .encode()
        });
        Ok(CrewPage { items, next_cursor })
    }

    /// One key of the tenant, by id or name, admin by key role: a revoked
    /// key's included, since its name still resolves and its row stays as
    /// history; one the tenant does not hold is absent (http-api.md keys,
    /// grants, denials; ADR-0026 d4, d9, d10). The attestation is read from
    /// the key's `key.issued`, which journals it as sent, where the row
    /// holds the schema default; never the value, which no store holds.
    pub async fn key(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
    ) -> Result<KeyDetail, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "key")?;
        let tenant_id = admin.tenant_id;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        let Some(record) = self.journal.key(tenant_id, key_id).await? else {
            return Err(AppError::NotFound);
        };
        let access = self
            .journal
            .key_access(tenant_id, key_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let issued = self
            .journal
            .events_range(tenant_id, access.issued_seq, 1)
            .await?
            .into_iter()
            .next()
            .filter(|e| {
                e.envelope.event_type == event_types::KEY_ISSUED
                    && payload_id(&e.envelope.payload, "key_id") == Some(key_id)
            })
            .ok_or_else(|| {
                StoreError::Backend(format!(
                    "key {key_id}'s issued_seq {} names no key.issued of it",
                    access.issued_seq
                ))
            })?;
        let held_by = match payload_str(&issued.envelope.payload, "held_by").as_deref() {
            None => None,
            Some("agent") => Some(HeldBy::Agent),
            Some("human") => Some(HeldBy::Human),
            Some("service") => Some(HeldBy::Service),
            Some(other) => {
                return Err(StoreError::Backend(format!(
                    "key {key_id}'s key.issued journals held_by {other:?}"
                ))
                .into());
            }
        };
        let user = self.user_ref(tenant_id, record.user).await?;
        Ok(KeyDetail {
            key: record,
            user,
            held_by,
            access,
        })
    }

    /// The user root of `user`, admin by key role: the root's record when a
    /// user root holds the name, `None` when no workarea of the shared
    /// namespace does. A shared workarea holding the name is `name.taken`,
    /// as an issuance naming that user would be. It journals nothing; only
    /// the issuance of the first key naming a user creates its root
    /// (ADR-0044 d1, d2).
    pub async fn user_root(
        &self,
        api_key: &str,
        user: &Name,
    ) -> Result<Option<WorkareaRecord>, AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "user_root")?;
        let Some(id) = self
            .journal
            .resolve_name(admin.tenant_id, NamedKind::Workarea, user)
            .await?
        else {
            return Ok(None);
        };
        match self.journal.workarea(admin.tenant_id, id).await? {
            Some(root) if root.user_root => Ok(Some(root)),
            _ => Err(AppError::NameTaken {
                kind: NamedKind::Workarea,
                name: user.clone(),
            }),
        }
    }

    /// The calling key's own record, which any live key reads, whatever its
    /// role: its row, its tenant, its default workarea with its derived path
    /// and its crew record, each by id and name, and its stored grants. It
    /// omits the key's denials, which name subtrees the key may not read,
    /// and journals nothing, since it reads no claim. The references are one
    /// store read after the two every request makes (ADR-0039 d15).
    pub async fn whoami(&self, api_key: &str) -> Result<Whoami, AppError> {
        let (key, access) = self.key_and_access(api_key).await?;
        let Some(access) = access else {
            return Ok(Whoami {
                key,
                tenant: None,
                default_workarea: None,
                crew: None,
                user: None,
                grants: Vec::new(),
            });
        };
        // The key read above holds the row: a tenant whose references read
        // nothing for it has lost it since, so fail closed.
        let references = self
            .journal
            .key_references(key.tenant_id, key.key_id)
            .await?
            .ok_or(AppError::Unauthorized)?;
        Ok(Whoami {
            key,
            tenant: Some(references.tenant),
            default_workarea: references.default_workarea,
            crew: references.crew,
            user: references.user,
            grants: access.grants,
        })
    }

    /// Change a crew record's charter URI or external manager, never its
    /// name, admin by key role: at least one field is sent, a field not
    /// sent keeps its value, and `crew.updated` journals the full new
    /// values, so its fold needs no earlier event (http-api.md crews;
    /// event-catalog.md `crew.updated`; ADR-0026 d8).
    pub async fn update_crew(
        &self,
        api_key: &str,
        crew: Reference,
        update: &CrewUpdate,
    ) -> Result<CrewRecord, AppError> {
        let key = self.authenticate(api_key).await?.key;
        require_role(&key, Role::Admin, "update_crew")?;
        if update.charter_uri.is_none() && update.external_manager.is_none() {
            return Err(AppError::EmptyCrewUpdate);
        }
        let crew_id = self.resolve(&key, NamedKind::Crew, &crew).await?;
        let current = self
            .journal
            .crew(key.tenant_id, crew_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let updated = CrewRecord {
            charter_uri: update.charter_uri.clone().or(current.charter_uri),
            external_manager: update.external_manager.clone().or(current.external_manager),
            ..current
        };
        let prev = self.head_event(key.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::CREW_UPDATED,
            &key.principal,
            Some(key.key_id),
            json!({
                "crew_id": updated.crew_id.to_string(),
                "charter_uri": updated.charter_uri,
                "external_manager": updated.external_manager,
                "updated_by_key_id": key.key_id.to_string(),
            }),
        )?;
        self.commit(event, MetadataFold::CrewUpdated(updated.clone()), None)
            .await?;
        Ok(updated)
    }

    /// Point a workarea at a crew record, each by id or name, admin by key
    /// role: the journalled crew-metadata update `workarea.crew_updated`,
    /// carrying the record's `crew_id`. A workarea the tenant does not hold
    /// is absent, and so, checked after it, is a crew (http-api.md
    /// workareas; ADR-0026 d8). A `validated` workarea admits it: the
    /// crew-metadata update is one of the three mutations freeze allows
    /// (ADR-0009 d5; workareas.md refusal matrix).
    pub async fn set_workarea_crew(
        &self,
        api_key: &str,
        workarea: Reference,
        crew: Reference,
    ) -> Result<WorkareaRecord, AppError> {
        let key = self.authenticate(api_key).await?.key;
        require_role(&key, Role::Admin, "set_workarea_crew")?;
        let workarea_id = self.resolve(&key, NamedKind::Workarea, &workarea).await?;
        self.observer.workarea(workarea_id);
        let crew_id = self.resolve(&key, NamedKind::Crew, &crew).await?;
        let workarea = self
            .journal
            .workarea(key.tenant_id, workarea_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if self.journal.crew(key.tenant_id, crew_id).await?.is_none() {
            return Err(AppError::NotFound);
        }
        let prev = self.head_event(key.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::WORKAREA_CREW_UPDATED,
            &key.principal,
            Some(key.key_id),
            json!({
                "workarea_id": workarea_id.to_string(),
                "crew_id": crew_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::WorkareaCrewUpdated {
                workarea_id,
                crew_id,
            },
            None,
        )
        .await?;
        Ok(WorkareaRecord {
            crew_id: Some(crew_id),
            ..workarea
        })
    }

    /// The live proposals on `node` as `key` may see them: the pending rows
    /// whose `from` is the node's status, in proposal order, each naming its
    /// proposing key only to that key (workareas.md, the status read lists
    /// only live proposals; ADR-0025 d15).
    async fn live_transitions(
        &self,
        key: &KeyRecord,
        node: &WorkareaRecord,
    ) -> Result<Vec<TransitionView>, AppError> {
        Ok(self
            .journal
            .pending_transitions(key.tenant_id, node.workarea_id)
            .await?
            .into_iter()
            .filter(|t| t.from == node.status)
            .map(|t| {
                // The own key set is the calling key alone until rotation
                // is built (roles-and-keys.md#own), as on the tail.
                let own = t.proposer_key_id == key.key_id;
                TransitionView {
                    transition_id: t.transition_id,
                    workarea_id: t.workarea_id,
                    from: t.from,
                    to: t.to,
                    state: t.state,
                    proposed_by_key_id: own.then_some(t.proposer_key_id),
                }
            })
            .collect())
    }

    /// The brief of `workarea` as `caller` reads it: its ratified brief
    /// edges, each claim once, in ratification order, kept only while the
    /// claim is a promoted served tip the caller reads, and cut by
    /// `brief.max_tokens` under recall's prefix rule (ADR-0046 d8, d9). A
    /// superseded or retired member drops out by itself.
    async fn brief(&self, caller: &Caller, workarea: Uuid) -> Result<Brief, AppError> {
        let mut ratified: Vec<(u64, Uuid)> = self
            .journal
            .brief_edges(caller.key.tenant_id, workarea)
            .await?
            .into_iter()
            .filter(|e| e.state == EdgeState::Ratified)
            .filter_map(|e| e.ratified_seq.map(|seq| (seq, e.artifact_id)))
            .collect();
        brief_order(&mut ratified, |r| *r);
        let mut seen = BTreeSet::new();
        let mut members = Vec::new();
        for (_, artifact_id) in ratified {
            if !seen.insert(artifact_id) {
                continue;
            }
            let Some(memory) = self
                .journal
                .memory(caller.key.tenant_id, artifact_id)
                .await?
            else {
                continue;
            };
            if memory.status == MemoryStatus::Promoted
                && memory.is_tip
                && self.readable(caller, &memory).await?
            {
                members.push(memory);
            }
        }
        let tokens: Vec<u64> = members
            .iter()
            .map(|m| budget_tokens(m.body_bytes))
            .collect();
        let budget = brief_cut(&tokens, self.brief_max_tokens);
        let mut items = Vec::with_capacity(budget.hits_served);
        for memory in members.into_iter().take(budget.hits_served) {
            items.push(self.served(caller, memory).await?);
        }
        Ok(Brief { items, budget })
    }

    /// The status read of the node, by id or name: its stored rung, the
    /// review policy in force and its live proposals in proposal order, the
    /// pending rows whose `from` is the node's status (workareas.md, the
    /// status read lists only live proposals; ADR-0009 d3), each naming its
    /// proposing key only to that key and never its principal (ADR-0025
    /// d15). The refusal
    /// order: (1) the key role, an admin key passing on its role and any
    /// other key needing reader among its admissible grant roles; (2) the
    /// node resolves; (3) a non-admin key needs reader effective access on
    /// it, `access.not_found` outside it. An admin key reads the pending
    /// transitions as structure, tenant-wide and with no grant
    /// (roles-and-keys.md permission matrix, footnote 6; ADR-0026 d15).
    pub async fn workarea_status(
        &self,
        api_key: &str,
        workarea: Reference,
    ) -> Result<WorkareaStatusView, AppError> {
        const VERB: &str = "workarea_status";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        let structure = key.role == Role::Admin;
        if !structure {
            require_admissible(key, Role::Reader, VERB)?;
        }
        let workarea_id = self.resolve(key, NamedKind::Workarea, &workarea).await?;
        if !structure {
            self.require_access(&caller, workarea_id, Role::Reader, VERB)
                .await?;
        }
        let node = self
            .journal
            .workarea(key.tenant_id, workarea_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let pending = self.live_transitions(key, &node).await?;
        Ok(WorkareaStatusView {
            workarea_id,
            status: node.status,
            policy: review_policy(),
            pending,
        })
    }

    /// Proposes a forward status transition of the node, by id or name, to
    /// `to`: the proposal is a pending `workarea_transitions` row, and the
    /// node keeps its status until a second identity ratifies it (ADR-0009
    /// d3). The refusal order: (1) the key role, before any lookup, an admin
    /// key passing on its role and any other key needing dreamer among its
    /// admissible grant roles, so a reader, writer or operator key is
    /// `access.verb_ceiling`; (2) the node resolves; (3) a dreamer key needs
    /// dreamer effective access on it, `access.not_found` outside read access
    /// and `access.grant_refused` within it; (4) the ladder, on the node and
    /// its immediate children as read now: `workarea.transition_refused` with
    /// its reason, nothing journalled (ADR-0009 d1, d3, d4); (5) one
    /// `workarea.status_proposed` is sealed and committed with the row. A
    /// `held_by: service` key may propose: roles-and-keys.md bars it from
    /// ratification alone (ADR-0010). The ratifying transaction re-checks the
    /// node, so a proposal admitted here may still be refused there
    /// (workareas.md, ratification re-checks).
    pub async fn propose_transition(
        &self,
        api_key: &str,
        workarea: Reference,
        to: WorkareaStatus,
    ) -> Result<TransitionRecord, AppError> {
        const VERB: &str = "propose_transition";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Dreamer, VERB)?;
        }
        let workarea_id = self.resolve(key, NamedKind::Workarea, &workarea).await?;
        if key.role != Role::Admin {
            self.require_access(&caller, workarea_id, Role::Dreamer, VERB)
                .await?;
        }
        let node = self
            .journal
            .workarea(key.tenant_id, workarea_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let children: Vec<WorkareaStatus> = self
            .journal
            .workarea_children(key.tenant_id, workarea_id)
            .await?
            .iter()
            .map(|child| child.status)
            .collect();
        // "No simulation is fenced on the subtree" holds vacuously while
        // simulations are unbuilt, and lands in the ladder's preconditions
        // with them (workareas.md, readings the build takes).
        propose_check(ladder_node(&node), node.status, to, &children).map_err(|reason| {
            AppError::TransitionRefused {
                workarea_id,
                from: node.status,
                to,
                reason,
            }
        })?;
        let proposal = TransitionRecord {
            transition_id: self.ids.new_id(),
            tenant_id: key.tenant_id,
            workarea_id,
            from: node.status,
            to,
            state: TransitionState::Pending,
            proposer_key_id: key.key_id,
            proposer_principal: key.principal.clone(),
            ratifier_key_id: None,
        };
        let prev = self.head_event(key.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::WORKAREA_STATUS_PROPOSED,
            &key.principal,
            Some(key.key_id),
            json!({
                "workarea_id": workarea_id.to_string(),
                "from": proposal.from.as_str(),
                "to": proposal.to.as_str(),
                "transition_id": proposal.transition_id.to_string(),
                "proposed_by_key_id": key.key_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::TransitionProposed(proposal.clone()),
            None,
        )
        .await?;
        Ok(proposal)
    }

    /// Ratifies the pending proposal `transition_id` on the node, by id or
    /// name, as a second identity: the node's status becomes the proposal's
    /// `to`, journalled as one `workarea.status_transition` signed by the
    /// ratifier's principal and key, with the policy snapshot (ADR-0009 d3,
    /// d4). The refusal order: (1) the key role, before any lookup, an admin
    /// key passing on its role and any other key needing dreamer among its
    /// admissible grant roles, `access.verb_ceiling`; (2) a `held_by:
    /// service` key, also `access.verb_ceiling` before any lookup
    /// (roles-and-keys.md; ADR-0010); (3) the node resolves; (4) a dreamer
    /// key needs dreamer effective access on it; (5) the transition: one
    /// this tenant does not hold, or one on another workarea, is
    /// `access.not_found`; (6) R2, a ratifier holding the proposer's key or
    /// principal, `review.ratifier_is_proposer`; (7) the ladder against the
    /// node and its children as read now, a proposal already ratified or
    /// whose `from` is no longer the node's status being
    /// `workarea.transition_refused` with reason `stale`. Every read happens
    /// after the chain head is read and before a commit that compare-and-sets
    /// it, so a child created between the check and the commit fails the
    /// commit with `PrevHashConflict`, retryable, and is never journalled
    /// over a stale check; the store re-checks under the chain lock besides
    /// (workareas.md, ratification re-checks).
    pub async fn ratify_transition(
        &self,
        api_key: &str,
        workarea: Reference,
        transition_id: Uuid,
    ) -> Result<WorkareaRecord, AppError> {
        const VERB: &str = "ratify_transition";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Dreamer, VERB)?;
        }
        refuse_service(key, VERB)?;
        let workarea_id = self.resolve(key, NamedKind::Workarea, &workarea).await?;
        if key.role != Role::Admin {
            self.require_access(&caller, workarea_id, Role::Dreamer, VERB)
                .await?;
        }
        let prev = self.head_event(key.tenant_id).await?;
        let proposal = self
            .journal
            .transition(key.tenant_id, transition_id)
            .await?
            .filter(|t| t.workarea_id == workarea_id)
            .ok_or(AppError::NotFound)?;
        if ratifier_is_proposer(
            proposal.proposer_key_id,
            &proposal.proposer_principal,
            key.key_id,
            &key.principal,
        ) {
            return Err(AppError::RatifierIsProposer);
        }
        let node = self
            .journal
            .workarea(key.tenant_id, workarea_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let children: Vec<WorkareaStatus> = self
            .journal
            .workarea_children(key.tenant_id, workarea_id)
            .await?
            .iter()
            .map(|child| child.status)
            .collect();
        let refused = |reason| AppError::TransitionRefused {
            workarea_id,
            from: proposal.from,
            to: proposal.to,
            reason,
        };
        if proposal.state == TransitionState::Ratified {
            return Err(refused(LadderRefusal::Stale));
        }
        // ADR-0030 d12 (proposed) joins its held_remaining test, no
        // unreviewed memory left in the subtree, to transition_preconditions
        // on the commit step, which ratify_check runs. "No simulation is
        // fenced on the subtree" would sit there too and holds vacuously
        // while simulations are unbuilt (workareas.md, readings the build
        // takes).
        ratify_check(
            proposal.from,
            ladder_node(&node),
            node.status,
            proposal.to,
            &children,
        )
        .map_err(refused)?;
        let mut policy_snapshot = review_policy();
        if proposal.to == WorkareaStatus::Validated {
            // A false one was refused above and is never journalled.
            policy_snapshot["children_all_validated"] = Value::Bool(true);
        }
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::WORKAREA_STATUS_TRANSITION,
            &key.principal,
            Some(key.key_id),
            json!({
                "workarea_id": workarea_id.to_string(),
                "from": proposal.from.as_str(),
                "to": proposal.to.as_str(),
                "transition_id": transition_id.to_string(),
                "proposed_by_key_id": proposal.proposer_key_id.to_string(),
                "ratified_by_key_id": key.key_id.to_string(),
                "policy_snapshot": policy_snapshot,
                // The proposer's user is read from its key row once keys
                // carry ADR-0044's `user`; until then no key names one.
                "same_user": same_user(None, key_user(key)),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::TransitionRatified {
                transition_id,
                workarea_id,
                to: proposal.to,
                ratifier_key_id: key.key_id,
            },
            None,
        )
        .await?;
        Ok(WorkareaRecord {
            status: proposal.to,
            ..node
        })
    }

    /// The lead's start on the names schema, before it serves: each chain of
    /// `tenants` that holds rows with no name gets one `system.names_assigned`
    /// naming them under rules 1–4 of http-api.md "Rows created before
    /// names", signed by the server, no key acting; its fold alone gives the
    /// rows their names, and every earlier event stays as it was signed. In
    /// the same commit, after it and in journal order, rule 5 lifts every
    /// global denial on an admin key still in force, each as one
    /// `key.denial_lifted` over the fixed reason `adr-0026-d15`, its epoch
    /// its own seq, so the names and the lifts commit together or not at
    /// all. A chain with no row to name journals nothing, so a second start
    /// journals nothing (ADR-0026 d11, d15). Tenant names are the deployment's,
    /// so rule 2 holds them across the chains, in the order `tenants` lists
    /// them; the PoC has no provider chain, so each tenant's name is
    /// journalled in its own chain's event, as the PoC journals the tenant's
    /// creation there. Every chain is read before any is named, so a chain
    /// that cannot be named answers [`AppError::NamingRefused`] with nothing
    /// journalled anywhere; a commit refused later answers its refusal and
    /// leaves named the chains it follows.
    pub async fn name_rows_that_predate_names(
        &self,
        tenants: &[Uuid],
    ) -> Result<Vec<AssignedNames>, AppError> {
        let mut chains = Vec::new();
        for &tenant_id in tenants {
            let unnamed = self.journal.unnamed_rows(tenant_id).await?;
            if unnamed.is_empty() {
                continue;
            }
            let events = self.whole_chain(tenant_id).await?;
            chains.push(predating_chain(tenant_id, &unnamed, &events)?);
        }
        let tenant_rows: Vec<UnnamedRow> = chains.iter().filter_map(|c| c.tenant.clone()).collect();
        let tenant_names: BTreeMap<Uuid, Assignment> =
            assign_names(None, &tenant_rows, &[], Uuid::nil)
                .names
                .into_iter()
                .map(|a| (a.id, a))
                .collect();
        let mut assigned = Vec::with_capacity(chains.len());
        for chain in chains {
            let tenant_id = chain.tenant_id;
            let rows = assign_names(chain.root, &chain.rows, &chain.crews, || self.ids.new_id());
            let naming = Naming {
                names: tenant_names
                    .get(&tenant_id)
                    .cloned()
                    .into_iter()
                    .chain(rows.names)
                    .collect(),
                crews: rows.crews,
            };
            let prev = self.head_event(tenant_id).await?;
            let names = self.seal(
                prev.as_ref(),
                tenant_id,
                event_types::SYSTEM_NAMES_ASSIGNED,
                "oml-server",
                None,
                names_assigned_payload(&naming),
            )?;
            let journal_seq = names.envelope.seq;
            let mut last = names.clone();
            let mut links = vec![(names, MetadataFold::NamesAssigned(naming.clone()))];
            for &(denial_id, key_id) in &chain.lifts {
                let lift = self.seal(
                    Some(&last),
                    tenant_id,
                    event_types::KEY_DENIAL_LIFTED,
                    "oml-server",
                    None,
                    json!({
                        "denial_id": denial_id.to_string(),
                        "key_id": key_id.to_string(),
                        "lifted_by_key_id": Value::Null,
                        "reason_digest": sha256_digest_string(ADMIN_DENIAL_LIFT_REASON.as_bytes()),
                        "epoch": last.envelope.seq + 1,
                    }),
                )?;
                last = lift.clone();
                links.push((lift, MetadataFold::KeyDenialLifted { key_id, denial_id }));
            }
            self.commit_chain(links).await?;
            assigned.push(AssignedNames {
                tenant_id,
                journal_seq,
                naming,
                lifted_denials: chain.lifts.iter().map(|(denial, _)| *denial).collect(),
            });
        }
        Ok(assigned)
    }

    /// `GET /v1/memories` (http-api.md search; ADR-0025 d3, d13). With
    /// `own`, the own-work listing: every row the calling key authored, in
    /// every state, across every node minus the subtrees its global
    /// denials cover; any tenant role but operator, no grant needed, and a
    /// `workarea_id` filter naming a node where the key holds some role.
    /// Without `own`, the scoped listing: reader on the scope node by the
    /// refusal order, and the promoted served tips of subtree(scope) ∩ the
    /// nodes where reader is effective, narrowed to a `workarea_id` subtree
    /// where reader is effective. Newest first; the cursor is bound to the
    /// key and to `api.cursor_ttl`, and every page re-applies the filter.
    pub async fn list_memories(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &ListRequest,
    ) -> Result<MemoryPage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        let tenant_id = key.tenant_id;
        let subtree_ids = |node: Uuid| async move {
            Ok::<BTreeSet<Uuid>, AppError>(
                self.journal
                    .workarea_subtree(tenant_id, node)
                    .await?
                    .into_iter()
                    .map(|w| w.workarea_id)
                    .collect(),
            )
        };
        let (scope, subtree) = if req.own {
            // The operator never reads content (roles-and-keys.md#own).
            if key.role == Role::Operator {
                return Err(AppError::VerbCeiling {
                    role: key.role,
                    verb: "list",
                });
            }
            // The workarea narrows the key's rows to a subtree, and names
            // only a node the naming rule names to the key; omitted, every
            // workarea the key wrote in (ADR-0039 d4).
            let filter = self
                .resolve_opt(key, NamedKind::Workarea, req.workarea.as_ref())
                .await?;
            let subtree = match filter {
                None => None,
                Some(node) => {
                    let ancestry = self.ancestry(tenant_id, node).await?;
                    if caller.effective(&ancestry).is_empty() {
                        return Err(AppError::NotFound);
                    }
                    Some(subtree_ids(node).await?.into_iter().collect())
                }
            };
            let mut masked = BTreeSet::new();
            for denial in &caller.denials {
                masked.extend(subtree_ids(denial.node).await?);
            }
            let scope = ListingScope::Own {
                key_ids: caller.own_key_ids(),
                masked: masked.into_iter().collect(),
            };
            (scope, subtree)
        } else {
            require_admissible(key, Role::Reader, "list")?;
            // One workarea, the scope, defaulting to the key's own
            // (ADR-0039 d4).
            let scope = self.target_workarea(key, req.workarea.as_ref()).await?;
            self.require_access(&caller, scope, Role::Reader, "list")
                .await?;
            let nodes = self.nodes_with(&caller, scope, Role::Reader).await?;
            (ListingScope::Tips { nodes }, None)
        };
        let now = self.clock.now();
        let after = req
            .cursor
            .as_deref()
            .map(|cursor| ListingCursor::resume(cursor, key.key_id, &now))
            .transpose()?;
        let filter = ListingFilter {
            scope,
            subtree,
            state: req.state,
            kind: req.kind,
            claim_key: req.claim_key.clone(),
            since: req.since.clone(),
            until: req.until.clone(),
        };
        // One row past the page says whether another page exists.
        let mut rows = self
            .journal
            .list_memories(
                tenant_id,
                &filter,
                after.as_ref(),
                req.limit.saturating_add(1),
            )
            .await?;
        let more = rows.len() > req.limit;
        rows.truncate(req.limit);
        let next_cursor = rows.last().filter(|_| more).map(|last| {
            ListingCursor {
                key_id: key.key_id,
                position: ListingPosition {
                    created_at: last.created_at.clone(),
                    artifact_id: last.memory_id,
                },
                issued_at: now.clone(),
            }
            .encode()
        });
        let mut items = Vec::with_capacity(rows.len());
        for m in rows {
            // R3 defense in depth: nothing outside the scope's predicate
            // leaves the application layer, whatever the store returned.
            let admitted = match &filter.scope {
                ListingScope::Own { key_ids, masked } => {
                    key_ids.contains(&m.author_key_id) && !masked.contains(&m.workarea_id)
                }
                ListingScope::Tips { nodes } => {
                    m.status == MemoryStatus::Promoted && m.is_tip && nodes.contains(&m.workarea_id)
                }
            };
            if admitted {
                items.push(self.served(&caller, m).await?);
            }
        }
        Ok(MemoryPage { items, next_cursor })
    }

    /// Submit a memory: writer on the resolved node, which must exist, by
    /// the refusal order (ADR-0025 d1); every `derives_from` target readable
    /// (ADR-0025 d3); the claim text normalized to NFC before the digest,
    /// the scan and the byte count (admission.md capture; ADR-0025 d6);
    /// same-call deterministic scan, whose veto is a refusal that journals
    /// nothing; a new claim's `claim_key` free in the workarea (ADR-0025
    /// d18), derived from the author and the text when omitted, and a
    /// verb defaulted from `derives_from` (ADR-0039 d3, d6); held on arrival
    /// as the claim's version 1, idempotency record honored (admission.md,
    /// http-api.md). A `validated` workarea refuses the write as
    /// `workarea.frozen` after effective access, so a key without access gets
    /// its access refusal first, and after the idempotency replay, before the
    /// scan (ADR-0009 d5; workareas.md refusal matrix).
    pub async fn submit_memory(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &SubmitRequest,
    ) -> Result<SubmitOutcome, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Writer, "submit")?;
        // The body's workarea, by id or name, resolves after step 1 and
        // before the record is looked up, so either spelling is one request
        // (ADR-0026 d4).
        let workarea_id = self.target_workarea(key, req.workarea.as_ref()).await?;
        if let Err(refusal) = self
            .require_access(&caller, workarea_id, Role::Writer, "submit")
            .await
        {
            // A user key reads `universe` and never writes it: it is told
            // universe is curated, `submission.universe_refused`, in place
            // of step 3's `access.grant_refused` (ADR-0044 d7, d14).
            if matches!(refusal, AppError::GrantRefused { .. })
                && key.user.is_some()
                && workarea_id == self.universe(key.tenant_id).await?
            {
                return Err(AppError::UniverseRefused);
            }
            return Err(refusal);
        }

        // An omitted claim_key is derived from the author and the text; the
        // `auto-` prefix is the server's alone (ADR-0039 d6). The author id
        // is the key's own id while rotation is unbuilt; once it is, d6 names
        // the root of the key's `rotated_from` chain, so a rotated key keeps
        // deriving the keys its predecessor did.
        let claim_key = match &req.claim_key {
            Some(sent) if sent.starts_with(DERIVED_CLAIM_KEY_PREFIX) => {
                return Err(AppError::DerivedClaimKeySent);
            }
            Some(sent) => sent.clone(),
            None => derive_claim_key(key.key_id, &req.body),
        };
        // An omitted verb is learn when lineage is declared, remember
        // otherwise; a sent verb is honoured (ADR-0039 d3).
        let verb = req.verb.unwrap_or(if req.derives_from.is_empty() {
            SubmitVerb::Remember
        } else {
            SubmitVerb::Learn
        });

        let scope = IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id,
            idem_key: req.idempotency_key.clone(),
        };
        let request_digest = submit_request_digest(req, &claim_key, verb, workarea_id);
        if let Some(record) = self.journal.idempotency_get(&scope).await? {
            if record.request_digest != request_digest {
                return Err(AppError::IdempotencyDigestMismatch);
            }
            let receipt: SubmitReceipt = serde_json::from_value(record.response)
                .map_err(|e| AppError::StoredResponse(e.to_string()))?;
            return Ok(SubmitOutcome {
                receipt,
                replayed: true,
            });
        }

        // The head first, then the node's status, the ancestors' closures,
        // which a claim edge's proposal widens, and the claim_key read: a
        // ratification, an edge proposal or a capture that commits in between
        // moves the head, so this commit conflicts and its retry reads them
        // all again (ADR-0001 d2; ADR-0009 d5; ADR-0025 d10, ordinals; d18).
        let prev = self.head_event(key.tenant_id).await?;
        self.refuse_frozen(key.tenant_id, workarea_id, FrozenOp::Write)
            .await?;

        // Capture normalizes first: the digest, the scan and the byte count
        // all see the NFC text (admission.md capture; ADR-0025 d6).
        let body = nfc(&req.body);

        // Deterministic scan: same call, before the commit transaction. A
        // veto is a refusal like any other: it journals nothing and stores
        // no row (admission.md; errors.md submission.scan_veto).
        let scan_finding = match self.scanner.scan(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: req.kind.as_str(),
            body: &body,
        }) {
            ScanOutcome::Pass => None,
            ScanOutcome::Hold(reason) => Some(reason),
            ScanOutcome::Veto(reason) => return Err(AppError::ScanVeto(reason)),
        };

        // The lineage closure: the author plus every declared ancestor's
        // closure — R2 runs over the whole of it (roles-and-keys.md). An
        // ancestor the key cannot read is indistinguishable from none.
        let mut principals = BTreeSet::from([key.principal.clone()]);
        let mut key_ids = BTreeSet::from([key.key_id]);
        for ancestor in &req.derives_from {
            let m = self
                .journal
                .memory(key.tenant_id, *ancestor)
                .await?
                .ok_or(AppError::LineageUnknown(*ancestor))?;
            if !self.readable(&caller, &m).await? {
                return Err(AppError::LineageUnknown(*ancestor));
            }
            principals.extend(m.lineage_principals);
            key_ids.extend(m.lineage_key_ids);
        }

        if self
            .journal
            .claim_key_state(key.tenant_id, workarea_id, &claim_key)
            .await?
            .live
        {
            return Err(AppError::ClaimKeyConflict);
        }

        let memory_id = self.ids.new_id();
        let body_digest = sha256_digest_string(body.as_bytes());
        let body_bytes = body.len() as u64;
        let memory = MemoryRecord {
            memory_id,
            tenant_id: key.tenant_id,
            workarea_id,
            claim_key: claim_key.clone(),
            version: 1,
            kind: req.kind,
            body,
            body_digest: body_digest.clone(),
            body_bytes,
            status: MemoryStatus::Held,
            is_tip: false,
            promoted_at: None,
            retirement: false,
            replaces: None,
            author_principal: key.principal.clone(),
            author_key_id: key.key_id,
            derives_from: req.derives_from.clone(),
            lineage_principals: principals.into_iter().collect(),
            lineage_key_ids: key_ids.into_iter().collect(),
            scan_finding: scan_finding.clone(),
            created_at: self.clock.now(),
        };

        // The catalog payload of submission.remember: digest-only, body
        // plaintext never rides the journal (ADR-0002 d1; event-catalog.md).
        // The PoC stores the body inline and unencrypted, so there is no
        // ciphertext digest, and it pins no scan rule set, so there is no
        // scanner version (admission.md rule-set registry).
        let payload = json!({
            "artifact_id": memory_id.to_string(),
            "workarea_id": workarea_id.to_string(),
            "claim_key": claim_key,
            "version": memory.version,
            "kind": req.kind.as_str(),
            "plaintext_digest": body_digest,
            "body_bytes": body_bytes,
            "ciphertext_digest": Value::Null,
            "attachments": [],
            "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "provenance": "agent",
            "simulation_id": Value::Null,
            "replaces": Value::Null,
            "retirement": false,
            "scan": {
                "result": if scan_finding.is_some() { "hold" } else { "pass" },
                "scanner_version": Value::Null,
                "finding": scan_finding,
            },
        });
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            match verb {
                SubmitVerb::Remember => event_types::SUBMISSION_REMEMBER,
                SubmitVerb::Learn => event_types::SUBMISSION_LEARN,
            },
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let seq = event.envelope.seq;

        let receipt = SubmitReceipt {
            memory_id,
            claim_key: claim_key.clone(),
            version: memory.version,
            workarea_id,
            state: MemoryStatus::Held.as_str().to_owned(),
            journal_seq: seq,
            body_digest,
        };
        let idempotency = Some(IdempotencyPut {
            scope,
            record: IdempotencyRecord {
                request_digest,
                response: serde_json::to_value(&receipt)
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?,
            },
        });
        self.commit(
            event,
            MetadataFold::MemoryHeld {
                memory,
                enqueue: true,
            },
            idempotency,
        )
        .await?;
        Ok(SubmitOutcome {
            receipt,
            replayed: false,
        })
    }

    /// Serve the next reviewable held item to this dreamer, claiming it,
    /// from the rows within its dreamer effective access only; a
    /// `held_by: service` key is refused before any lookup (ADR-0025 d1).
    pub async fn review_next(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<Option<ServedMemory>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Dreamer, "review")?;
        refuse_service(key, "review")?;
        let workareas = self.dreamer_nodes(&caller).await?;
        match self
            .journal
            .review_claim_next(key.tenant_id, &key.principal, key.key_id, &workareas)
            .await?
        {
            None => Ok(None),
            Some(memory) => Ok(Some(self.served(&caller, memory).await?)),
        }
    }

    /// Record a verdict. Dreamer on the row's workarea by the refusal order,
    /// with no exception for review, and never from a `held_by: service` key
    /// (ADR-0025 d1); R2 runs here against the row's reviewer-exclusion set
    /// — its lineage closure plus, for a held successor, the authors of the
    /// held rivals of its target (`check_reviewer`; ADR-0025 d10). With the
    /// PoC policy k = 1 an approval ratifies immediately, as a second
    /// journalled event carrying the policy snapshot, committed in the
    /// verdict's own transaction (admission.md review and
    /// ratification): `lifecycle.promote` for a first version, and for a
    /// successor `lifecycle.supersede`, which compare-and-sets that its
    /// target is still the served tip of the successor's workarea and
    /// `claim_key`. An approval whose target lost the tip is
    /// `submission.tip_conflict`, journals nothing and leaves the row held
    /// and queued (ADR-0025 d10).
    pub async fn post_verdict(
        &self,
        bearer: impl Into<Bearer<'_>>,
        memory_id: Uuid,
        verdict: Verdict,
        reason: Option<&str>,
    ) -> Result<VerdictOutcome, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_admissible(&caller.key, Role::Dreamer, "verdict")?;
        refuse_service(&caller.key, "verdict")?;
        self.verdict_on(&caller, memory_id, verdict, reason, None)
            .await
    }

    /// Record a verdict as `POST /v2/review/verdicts` received it, under
    /// its idempotency record. The record is looked up after the verb
    /// ceiling and before anything else: a retry of a committed verdict
    /// replays its stored response whatever the key holds now, so a
    /// verdict sent without `serve_id` never resolves a new target on
    /// retry, and the same `Idempotency-Key` with another request is
    /// [`AppError::IdempotencyDigestMismatch`]. With no record a verdict
    /// without `serve_id` applies to the one queue entry this reviewer key
    /// has claimed, and with none or several it is
    /// [`AppError::ServeIdUnresolved`], nothing journalled (http-api.md
    /// idempotency; ADR-0039 d10). Everything else is
    /// [`App::post_verdict`]'s.
    pub async fn review_verdict(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &VerdictRequest,
    ) -> Result<VerdictOutcome, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Dreamer, "verdict")?;
        refuse_service(key, "verdict")?;
        let recording = VerdictRecording {
            scope: IdempotencyScope {
                tenant_id: key.tenant_id,
                key_id: key.key_id,
                workarea_id: Uuid::nil(),
                idem_key: req.idempotency_key.clone(),
            },
            request_digest: verdict_request_digest(req)?,
        };
        if let Some(recorded) = self
            .recorded::<RecordedVerdict>(&recording.scope, &recording.request_digest)
            .await?
        {
            return recorded.replay();
        }
        let memory_id = match &req.serve {
            Some(serve) => serve.memory_id,
            None => {
                // The default rests on the claimed queue row (review_queue's
                // held_by_key), not on a serve record: the serve binding
                // stays FR-66 (ADR-0039 Consequences, "The serve binding is
                // not fixed here").
                let claimed = self
                    .journal
                    .review_claimed(key.tenant_id, key.key_id)
                    .await?;
                let [memory_id] = claimed[..] else {
                    return Err(AppError::ServeIdUnresolved {
                        several: !claimed.is_empty(),
                    });
                };
                memory_id
            }
        };
        self.verdict_on(
            &caller,
            memory_id,
            req.verdict,
            req.reason.as_deref(),
            Some(recording),
        )
        .await
    }

    /// [`App::post_verdict`] after the verb ceiling, for an authenticated
    /// caller, committing `recording`'s idempotency record with the verdict
    /// when there is one.
    async fn verdict_on(
        &self,
        caller: &Caller,
        memory_id: Uuid,
        verdict: Verdict,
        reason: Option<&str>,
        recording: Option<VerdictRecording>,
    ) -> Result<VerdictOutcome, AppError> {
        let key = &caller.key;
        // The head first, then every read R2 and the compare-and-set judge:
        // the row's closure, which a claim edge's proposal widens while the
        // row is held, its rivals and its target. A commit landing after the
        // head moves it, so the verdict conflicts and its retry reads them
        // again (ADR-0001 d2; ADR-0025 d10).
        let prev = self.head_event(key.tenant_id).await?;
        let memory = self
            .journal
            .memory(key.tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        self.require_access(caller, memory.workarea_id, Role::Dreamer, "verdict")
            .await?;
        let mut exclusion = memory.lineage_closure();
        if let Some(target) = memory.replaces
            && memory.status == MemoryStatus::Held
        {
            for rival in self.journal.held_successors(key.tenant_id, target).await? {
                if rival.memory_id != memory.memory_id
                    && rival.workarea_id == memory.workarea_id
                    && rival.claim_key == memory.claim_key
                {
                    exclusion.add_principal(rival.author_principal);
                    exclusion.add_key_id(rival.author_key_id);
                }
            }
        }
        check_reviewer(key.role, &key.principal, key.key_id, &exclusion)?;

        let (verdict_type, target_status) = match verdict {
            Verdict::Approve => (event_types::REVIEW_APPROVE, MemoryStatus::Promoted),
            Verdict::Reject => (event_types::REVIEW_REJECT, MemoryStatus::Rejected),
        };
        // The pure transition function is the arbiter: anything but a held
        // row refuses here (admission.md lifecycle).
        let new_status = transition(memory.status, target_status)?;

        // Nothing new promotes into a `validated` area: an approval is
        // refused before its verdict is sealed, so nothing is journalled and
        // the row stays held (ADR-0009 d5). Whether a reject is refused too,
        // and whether review_next serves such a row, is open in workareas.md
        // and waits on the operator.
        if verdict == Verdict::Approve {
            self.refuse_frozen(key.tenant_id, memory.workarea_id, FrozenOp::Promote)
                .await?;
        }
        let replaced = match (verdict, memory.replaces) {
            (Verdict::Approve, Some(target_id)) => {
                let target = self.journal.memory(key.tenant_id, target_id).await?;
                match target {
                    Some(t)
                        if t.status == MemoryStatus::Promoted
                            && t.is_tip
                            && t.workarea_id == memory.workarea_id
                            && t.claim_key == memory.claim_key =>
                    {
                        Some(t)
                    }
                    _ => {
                        return Err(AppError::TipConflict {
                            current_tip: self
                                .readable_tip(caller, memory.workarea_id, &memory.claim_key)
                                .await?,
                        });
                    }
                }
            }
            _ => None,
        };

        let verdict_event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            verdict_type,
            &key.principal,
            Some(key.key_id),
            json!({
                "artifact_id": memory_id.to_string(),
                "verdict": match verdict { Verdict::Approve => "approve", Verdict::Reject => "reject" },
                "reason": reason,
                "served_body_digest": memory.body_digest,
            }),
        )?;
        let verdict_event_id = verdict_event.envelope.event_id;

        match verdict {
            Verdict::Reject => {
                let outcome = VerdictOutcome {
                    memory_id,
                    version: memory.version,
                    state: new_status,
                    journal_seq: verdict_event.envelope.seq,
                    replayed: false,
                };
                let idempotency = recording.as_ref().map(|r| r.put(&outcome)).transpose()?;
                let committed = self
                    .commit(
                        verdict_event,
                        MetadataFold::MemoryRejected { memory_id },
                        idempotency,
                    )
                    .await;
                self.verdict_committed(committed, recording.as_ref(), outcome)
                    .await
            }
            Verdict::Approve => {
                // Ratification snapshots the policy it was decided under
                // (admission.md). The PoC journals no review.ratify: at k = 1
                // the approving verdict is the ratifying act, so
                // ratification_event_id names it. Nothing is self-signed in
                // the PoC (ADR-0025 d17).
                let policy = review_policy();
                let (event_type, payload, fold) = match replaced {
                    None => (
                        event_types::LIFECYCLE_PROMOTE,
                        json!({
                            "artifact_id": memory_id.to_string(),
                            "version": memory.version,
                            "workarea_id": memory.workarea_id.to_string(),
                            "claim_key": memory.claim_key,
                            "ratification_event_id": verdict_event_id.to_string(),
                            "previous_tip_version": Value::Null,
                            "self_signed": false,
                            "policy": policy,
                        }),
                        MetadataFold::MemoryPromoted { memory_id },
                    ),
                    Some(target) => (
                        event_types::LIFECYCLE_SUPERSEDE,
                        json!({
                            "claim_key": memory.claim_key,
                            "workarea_id": memory.workarea_id.to_string(),
                            "new_artifact_id": memory_id.to_string(),
                            "new_version": memory.version,
                            "replaces_artifact_id": target.memory_id.to_string(),
                            "replaces_version": target.version,
                            "retirement": memory.retirement,
                            "ratification_event_id": verdict_event_id.to_string(),
                            "self_signed": false,
                            "policy": policy,
                        }),
                        MetadataFold::TipSuperseded {
                            successor: memory_id,
                            replaced: target.memory_id,
                        },
                    ),
                };
                let ratification = self.seal(
                    Some(&verdict_event),
                    key.tenant_id,
                    event_type,
                    "oml-server",
                    None,
                    payload,
                )?;
                // The verdict and its ratification commit as one transaction,
                // so the compare-and-set conditions the verdict too: a lost
                // one journals neither, and a chain-head conflict leaves no
                // verdict for the retry to journal twice (ADR-0025 d10).
                // The record rides the verdict's link, so a lost
                // compare-and-set records nothing either.
                let outcome = VerdictOutcome {
                    memory_id,
                    version: memory.version,
                    state: new_status,
                    journal_seq: ratification.envelope.seq,
                    replayed: false,
                };
                let idempotency = recording.as_ref().map(|r| r.put(&outcome)).transpose()?;
                let links = vec![
                    commit_request(
                        verdict_event,
                        MetadataFold::VerdictRecorded {
                            memory_id,
                            reviewer_key_id: key.key_id,
                        },
                        idempotency,
                    ),
                    commit_request(ratification, fold, None),
                ];
                let committed = match self.journal.commit_chain(links).await {
                    Err(StoreError::TipMoved { .. }) => {
                        return Err(AppError::TipConflict {
                            current_tip: self
                                .readable_tip(caller, memory.workarea_id, &memory.claim_key)
                                .await?,
                        });
                    }
                    other => other.map_err(AppError::from),
                };
                self.verdict_committed(committed, recording.as_ref(), outcome)
                    .await
            }
        }
    }

    /// A verdict's commit result as its caller is answered: `outcome` once
    /// committed, and when another attempt under the same idempotency
    /// record committed first, which wrote nothing here, that attempt's
    /// stored response as a replay (http-api.md idempotency).
    async fn verdict_committed(
        &self,
        committed: Result<CommitReceipt, AppError>,
        recording: Option<&VerdictRecording>,
        outcome: VerdictOutcome,
    ) -> Result<VerdictOutcome, AppError> {
        match (committed, recording) {
            (Err(AppError::Store(StoreError::IdempotencyRecorded)), Some(recording)) => self
                .recorded::<RecordedVerdict>(&recording.scope, &recording.request_digest)
                .await?
                .ok_or(AppError::Store(StoreError::IdempotencyRecorded))?
                .replay(),
            (committed, _) => committed.map(|_| outcome),
        }
    }

    /// Recall: reader on the scope node by the refusal order, and no
    /// tenant-wide fallback (ADR-0025 d1). Segment 1 is the applicable
    /// steering — the steering in force on the scope and each ancestor the
    /// key reads, ordered structurally and never by the query; the hits
    /// follow, over subtree(scope) ∩ the nodes where reader is effective, of
    /// the kinds the request admits, score descending then `artifact_id`
    /// ascending (retrieval.md, the recall-ordering contract; ADR-0025 d8).
    /// With `include_unreviewed` the calling key's own held rows join the hit
    /// candidates under that one order, flagged by state (d4). The merged
    /// candidates are cut to `k`, then by `max_tokens` — steering never cut
    /// and spent first, the hits the longest rank prefix that fits (d7) —
    /// and only the rows served are hydrated, in one set-wise read. The
    /// result names the served ids and the digest of the whole request, the
    /// read log's two inputs (d9).
    pub async fn recall(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &RecallRequest,
    ) -> Result<RecallResult, AppError> {
        // A budget counts bodies, so it is refused with bodies off before
        // anything is read, as the request's other shape refusals are
        // (retrieval.md token budgets).
        if req.max_tokens.is_some() && !req.include_text {
            return Err(AppError::BudgetWithoutBodies);
        }
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reader, "recall")?;
        let scope = self.target_workarea(key, req.workarea.as_ref()).await?;
        self.require_access(&caller, scope, Role::Reader, "recall")
            .await?;
        // The query is its NFC text, searched and digested alike (ADR-0025
        // d9).
        let query = nfc(&req.query);

        // The scope's ancestry runs node to root, so each ancestor's own
        // ancestry is its suffix; the store is handed the readable ones root
        // first, the order of their depth.
        let scope_ancestry = self
            .journal
            .workarea_ancestry(key.tenant_id, scope)
            .await?
            .unwrap_or_default();
        let ancestry: Vec<Uuid> = scope_ancestry.iter().map(|w| w.workarea_id).collect();
        let applicable: Vec<Uuid> = (0..ancestry.len())
            .rev()
            .filter(|&i| caller.effective(&ancestry[i..]).contains(&Role::Reader))
            .map(|i| ancestry[i])
            .collect();
        // R3 defense in depth, as for the hits below: only a readable served
        // tip in force, never a retirement version (ADR-0025 d8, d11).
        let steering: Vec<MemoryRecord> = self
            .journal
            .steering_in_force(key.tenant_id, &applicable)
            .await?
            .into_iter()
            .filter(|m| {
                m.kind == MemoryKind::Steering
                    && m.status == MemoryStatus::Promoted
                    && m.is_tip
                    && !m.retirement
                    && applicable.contains(&m.workarea_id)
            })
            .collect();

        let readable = self.nodes_with(&caller, scope, Role::Reader).await?;
        // A scope inside a user root also searches `universe`'s own claims
        // where the key reads that node, ranked with the scope's own; own
        // held rows never include them (ADR-0044 d10).
        let mut hit_nodes = readable.clone();
        if scope_ancestry.iter().any(|w| w.user_root)
            && let Some(&universe) = ancestry.last()
            && caller.effective(&[universe]).contains(&Role::Reader)
            && !hit_nodes.contains(&universe)
        {
            hit_nodes.push(universe);
        }
        let kinds: Vec<MemoryKind> = HIT_KINDS
            .into_iter()
            .filter(|kind| req.kinds.is_empty() || req.kinds.contains(kind))
            .collect();
        let mut candidates = self
            .journal
            .recall_fts(key.tenant_id, &hit_nodes, &query, &kinds, req.k)
            .await?;
        if req.include_unreviewed {
            // The own key set is the calling key alone until rotation is
            // built (roles-and-keys.md#own); the readable set already
            // excludes the key's global denials.
            candidates.extend(
                self.journal
                    .recall_own_held(
                        key.tenant_id,
                        &[key.key_id],
                        &readable,
                        &query,
                        &kinds,
                        req.k,
                    )
                    .await?,
            );
        }
        // One total order over both lists, then the k cut, then the budget
        // cut over the counts `body_bytes` gives without any body read.
        hit_order(&mut candidates, |c| (c.score, c.memory_id));
        candidates.truncate(req.k);
        let steering_counts: Vec<u64> = steering
            .iter()
            .map(|m| budget_tokens(m.body_bytes))
            .collect();
        let hit_counts: Vec<u64> = candidates
            .iter()
            .map(|c| budget_tokens(c.body_bytes))
            .collect();
        let budget = cut(&steering_counts, &hit_counts, req.max_tokens);
        if let Some(budget) = &budget {
            candidates.truncate(budget.hits_served);
        }

        let ids: Vec<Uuid> = candidates.iter().map(|c| c.memory_id).collect();
        let mut rows: BTreeMap<Uuid, MemoryRecord> = self
            .journal
            .memories(key.tenant_id, &ids)
            .await?
            .into_iter()
            .map(|m| (m.memory_id, m))
            .collect();
        let mut served_ids: Vec<Uuid> = steering.iter().map(|m| m.memory_id).collect();
        let mut served_steering = Vec::with_capacity(steering.len());
        for m in steering {
            served_steering.push(self.served(&caller, m).await?);
        }
        let mut hits = Vec::with_capacity(candidates.len());
        for candidate in &candidates {
            let Some(m) = rows.remove(&candidate.memory_id) else {
                continue;
            };
            // R3 defense in depth: the gate changed visibility. Nothing
            // unpromoted or unreadable leaves the application layer except
            // the caller's own held rows when it asked for them; a replaced
            // version or any version of a retired claim is promoted history,
            // never a hit; a retirement version is never in force (ADR-0025
            // d4, d8). Each hit names only the ancestors the caller can read
            // (d3).
            let admitted = match m.status {
                MemoryStatus::Promoted => m.is_tip && hit_nodes.contains(&m.workarea_id),
                MemoryStatus::Held => {
                    req.include_unreviewed
                        && m.author_key_id == key.key_id
                        && readable.contains(&m.workarea_id)
                }
                _ => false,
            };
            if admitted && !m.retirement && kinds.contains(&m.kind) {
                served_ids.push(m.memory_id);
                hits.push(RecallHit {
                    served: self.served(&caller, m).await?,
                    score: candidate.score,
                });
            }
        }
        Ok(RecallResult {
            steering: served_steering,
            hits,
            budget,
            served_ids,
            query_digest: recall_query_digest(&query, req, scope)?,
        })
    }

    /// Get one memory: the authoring key sees its own rows in every state,
    /// with no grant needed and global denials still masking them; everyone
    /// else sees promoted rows, replaced versions included, with reader
    /// effective access on the row's workarea; anything else is absence,
    /// indistinguishable from no row. `derives_from` names only the
    /// ancestors the caller can read (http-api.md; ADR-0025 d3). Get needs
    /// reader, so an admin or operator key, which no reader grant can be
    /// admitted on, never reads content and is refused by its role before
    /// any lookup (step 1 of the refusal order; ADR-0025 d1; ADR-0026 d15).
    pub async fn get_memory(
        &self,
        bearer: impl Into<Bearer<'_>>,
        memory_id: Uuid,
    ) -> Result<ServedMemory, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_admissible(&caller.key, Role::Reader, "get")?;
        let memory = self
            .journal
            .memory(caller.key.tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if self.readable(&caller, &memory).await? {
            self.served(&caller, memory).await
        } else {
            Err(AppError::NotFound)
        }
    }

    /// Propose a successor of the target's claim, held like any write: the
    /// target stays the served tip, and stays in everyone's recall, until a
    /// non-author ratifies the successor (ADR-0025 d10). With `retirement`
    /// the successor proposes retiring the claim (d11). The supersede rule
    /// runs in its order: a key role admitting writer or dreamer (step 1);
    /// the idempotency record, scoped to the key with a nil workarea, looked
    /// up next and replayed whatever the target's state now; the target
    /// readable (step 2); a writer's own target and writer on its workarea
    /// (step 3), or a dreamer's dreamer and reader there, whoever authored
    /// it (step 4, edit-as-proposal); then the target-dependent refusals —
    /// `X-OML-Workarea`, the served tip, the kind and retirement checks,
    /// from `oml_supersede` the vocabulary-kind check, and last the
    /// workarea not `validated`, else `workarea.frozen` (step 5; ADR-0009
    /// d5); then the checks every submit runs. The successor takes the next
    /// ordinal of the claim_key and journals `submission.remember`
    /// (`submission.learn` for a `derived_fact`) naming what it replaces,
    /// with the idempotency record in the same transaction; a commit that finds the record already
    /// written by a concurrent attempt under the same key writes nothing and
    /// answers that attempt's receipt as a replay ([`IdempotencyPut`] says
    /// what the slice does not claim).
    pub async fn supersede(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &SupersedeRequest,
    ) -> Result<SupersedeOutcome, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        let admissible = admissible_grant_roles(key.role);
        let dreamer = admissible.contains(&Role::Dreamer);
        if !dreamer && !admissible.contains(&Role::Writer) {
            return Err(AppError::VerbCeiling {
                role: key.role,
                verb: "supersede",
            });
        }
        // The body's workarea, by id or name, resolves after step 1
        // (ADR-0026 d4).
        let header = self
            .resolve_opt(key, NamedKind::Workarea, req.workarea.as_ref())
            .await?;

        // The record is looked up between steps 1 and 2 and replays its
        // stored response whatever the target or the key's access now: the
        // response names only the caller's own successor (http-api.md
        // idempotency). The scope's workarea is nil, so no target's
        // workarea ever enters a record.
        let scope = IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id: Uuid::nil(),
            idem_key: req.idempotency_key.clone(),
        };
        let request_digest = supersede_request_digest(req)?;
        if let Some(receipt) = self.recorded(&scope, &request_digest).await? {
            return Ok(SupersedeOutcome {
                receipt,
                replayed: true,
            });
        }

        // The head first, then the target, whose closure a claim edge's
        // proposal widens, the ancestors and the ordinal: a commit landing in
        // between moves the head, and the retry reads them all again
        // (ADR-0001 d2; ADR-0025 d10, ordinals).
        let prev = self.head_event(key.tenant_id).await?;

        // Step 2: an unreadable target is indistinguishable from none.
        let target = self
            .journal
            .memory(key.tenant_id, req.target_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if !self.readable(&caller, &target).await? {
            return Err(AppError::NotFound);
        }
        if dreamer {
            // Step 4: edit-as-proposal needs dreamer and reader on the
            // target's workarea, whoever authored it.
            self.require_access(&caller, target.workarea_id, Role::Dreamer, "supersede")
                .await?;
            self.require_access(&caller, target.workarea_id, Role::Reader, "supersede")
                .await?;
        } else {
            // Step 3: own is the target version's authoring key, never the
            // claim's first version (roles-and-keys.md#own).
            if target.author_key_id != key.key_id {
                return Err(AppError::AuthorOnly);
            }
            self.require_access(&caller, target.workarea_id, Role::Writer, "supersede")
                .await?;
        }

        // Step 5, in its order: the scope is the target's workarea, the
        // target is the served tip, the kind stays in its class and a
        // retirement keeps the target's kind and never retires a term, and
        // on oml_supersede the target is of no vocabulary kind (ADR-0025 d12).
        if let (Some(workarea_id), Some(sent)) = (header, &req.workarea)
            && workarea_id != target.workarea_id
        {
            return Err(AppError::WorkareaNotTarget {
                workarea: sent.clone(),
            });
        }
        if target.status != MemoryStatus::Promoted || !target.is_tip {
            return Err(AppError::TipConflict {
                current_tip: self
                    .readable_tip(&caller, target.workarea_id, &target.claim_key)
                    .await?,
            });
        }
        let kind = successor_kind(target.kind, req.kind, req.retirement)?;
        if req.refuse_vocabulary_target
            && matches!(
                target.kind,
                MemoryKind::Term | MemoryKind::TermMapping | MemoryKind::VocabRelease
            )
        {
            return Err(AppError::VocabularyTarget(target.kind));
        }
        // Step 5's last check, after the head read above: a ratification
        // that validates the area in between moves the head and the commit
        // conflicts (roles-and-keys.md supersede; ADR-0009 d5).
        self.refuse_frozen(key.tenant_id, target.workarea_id, FrozenOp::Write)
            .await?;

        // The checks every submit runs: NFC first, then the scan, whose veto
        // journals nothing, then every declared ancestor readable
        // (admission.md capture; ADR-0025 d3, d6).
        let body = nfc(&req.body);
        let scan_finding = match self.scanner.scan(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: kind.as_str(),
            body: &body,
        }) {
            ScanOutcome::Pass => None,
            ScanOutcome::Hold(reason) => Some(reason),
            ScanOutcome::Veto(reason) => return Err(AppError::ScanVeto(reason)),
        };
        // derives_from ⊇ {target}; the lineage closure is the proposer plus
        // every ancestor's closure, the target's included (roles-and-keys.md).
        let mut derives_from = vec![target.memory_id];
        for ancestor in &req.derives_from {
            if !derives_from.contains(ancestor) {
                derives_from.push(*ancestor);
            }
        }
        let mut principals = BTreeSet::from([key.principal.clone()]);
        let mut key_ids = BTreeSet::from([key.key_id]);
        principals.extend(target.lineage_principals.iter().cloned());
        key_ids.extend(target.lineage_key_ids.iter().copied());
        for ancestor in &derives_from[1..] {
            let m = self
                .journal
                .memory(key.tenant_id, *ancestor)
                .await?
                .ok_or(AppError::LineageUnknown(*ancestor))?;
            if !self.readable(&caller, &m).await? {
                return Err(AppError::LineageUnknown(*ancestor));
            }
            principals.extend(m.lineage_principals);
            key_ids.extend(m.lineage_key_ids);
        }

        // The head was read above, then the ordinal: 1 + the greatest
        // version of the claim_key in the workarea, whatever its state; a
        // capture that commits in between moves the head, and the retry
        // reads both again (ADR-0025 d10, ordinals).
        let version = self
            .journal
            .claim_key_state(key.tenant_id, target.workarea_id, &target.claim_key)
            .await?
            .greatest_version
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                StoreError::Backend(format!(
                    "claim_key {:?} has no next ordinal",
                    target.claim_key
                ))
            })?;

        let successor_id = self.ids.new_id();
        let body_digest = sha256_digest_string(body.as_bytes());
        let body_bytes = body.len() as u64;
        let replaces = VersionRef {
            artifact_id: target.memory_id,
            version: target.version,
        };
        let successor = MemoryRecord {
            memory_id: successor_id,
            tenant_id: key.tenant_id,
            workarea_id: target.workarea_id,
            claim_key: target.claim_key.clone(),
            version,
            kind,
            body,
            body_digest: body_digest.clone(),
            body_bytes,
            status: MemoryStatus::Held,
            is_tip: false,
            promoted_at: None,
            retirement: req.retirement,
            replaces: Some(target.memory_id),
            author_principal: key.principal.clone(),
            author_key_id: key.key_id,
            derives_from: derives_from.clone(),
            lineage_principals: principals.into_iter().collect(),
            lineage_key_ids: key_ids.into_iter().collect(),
            scan_finding: scan_finding.clone(),
            created_at: self.clock.now(),
        };

        // The catalog payload of the submission, digest-only, plus what it
        // replaces and whether it retires (event-catalog.md
        // submission.remember; ADR-0025 d10, d11).
        let event_type = if kind == MemoryKind::DerivedFact {
            event_types::SUBMISSION_LEARN
        } else {
            event_types::SUBMISSION_REMEMBER
        };
        let payload = json!({
            "artifact_id": successor_id.to_string(),
            "workarea_id": target.workarea_id.to_string(),
            "claim_key": target.claim_key,
            "version": version,
            "kind": kind.as_str(),
            "plaintext_digest": body_digest,
            "body_bytes": body_bytes,
            "ciphertext_digest": Value::Null,
            "attachments": [],
            "derives_from": derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "provenance": "agent",
            "simulation_id": Value::Null,
            "replaces": {
                "artifact_id": target.memory_id.to_string(),
                "version": target.version,
            },
            "retirement": req.retirement,
            "scan": {
                "result": if scan_finding.is_some() { "hold" } else { "pass" },
                "scanner_version": Value::Null,
                "finding": scan_finding,
            },
        });
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_type,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let receipt = SupersedeReceipt {
            memory_id: successor_id,
            claim_key: target.claim_key.clone(),
            version,
            workarea_id: target.workarea_id,
            state: MemoryStatus::Held.as_str().to_owned(),
            retirement: req.retirement,
            replaces,
            journal_seq: event.envelope.seq,
            body_digest,
        };
        let idempotency = Some(IdempotencyPut {
            scope: scope.clone(),
            record: IdempotencyRecord {
                request_digest: request_digest.clone(),
                response: serde_json::to_value(&receipt)
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?,
            },
        });
        let committed = self
            .commit(
                event,
                MetadataFold::MemoryHeld {
                    memory: successor,
                    enqueue: true,
                },
                idempotency,
            )
            .await;
        if let Err(AppError::Store(StoreError::IdempotencyRecorded)) = committed {
            // An attempt under the same key committed between this one's
            // lookup and its commit, which wrote nothing: the record that
            // attempt committed answers this one, as it answers any replay
            // (http-api.md idempotency).
            let receipt = self
                .recorded(&scope, &request_digest)
                .await?
                .ok_or(AppError::Store(StoreError::IdempotencyRecorded))?;
            return Ok(SupersedeOutcome {
                receipt,
                replayed: true,
            });
        }
        committed?;
        Ok(SupersedeOutcome {
            receipt,
            replayed: false,
        })
    }

    /// Propose a claim edge `from_artifact_id -> to_artifact_id` carrying
    /// `label`: journalled `claim_edge.proposed`, stored pending, and in its
    /// own commit widening the lineage closure of every version it connects,
    /// so R2 counts the edge while it waits (ADR-0008 d5; ADR-0012;
    /// audit-and-lineage.md, pending and ratified alike). The refusal order:
    /// the verb ceiling, the idempotency record, each endpoint present and in
    /// read scope, dreamer on at least one endpoint's workarea, then the
    /// label (workareas.md, edge authority; ADR-0021 d5). A `held_by:
    /// service` key may propose: only ratification is review-class
    /// (roles-and-keys.md).
    pub async fn propose_claim_edge(
        &self,
        api_key: &str,
        req: &ProposeClaimEdge,
        idempotency_key: Option<&str>,
    ) -> Result<ClaimEdgeProposalOutcome, AppError> {
        const VERB: &str = "claim_edge.propose";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Dreamer, VERB)?;

        // A claim-edge POST names no workarea, so the scope's workarea is
        // nil (http-api.md idempotency); the record replays whatever the
        // endpoints or the key's access now, as it names only the caller's
        // own edge.
        let request_digest = claim_edge_request_digest(req)?;
        let scope = idempotency_key.map(|idem_key| IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id: Uuid::nil(),
            idem_key: idem_key.to_owned(),
        });
        if let Some(scope) = &scope
            && let Some(proposal) = self.recorded(scope, &request_digest).await?
        {
            return Ok(ClaimEdgeProposalOutcome {
                proposal,
                replayed: true,
            });
        }

        // The head before the endpoints: a commit landing in between moves
        // it, and the retry reads them again (ADR-0001 d2).
        let prev = self.head_event(key.tenant_id).await?;

        let grants: Vec<LabelledGrant> = caller
            .grants
            .iter()
            .cloned()
            .map(LabelledGrant::from)
            .collect();
        let mut endpoints = Vec::with_capacity(2);
        for id in [req.from_artifact_id, req.to_artifact_id] {
            let memory = self
                .journal
                .memory(key.tenant_id, id)
                .await?
                .ok_or(AppError::NotFound)?;
            endpoints.push(memory);
        }
        let mut proposable = Vec::with_capacity(2);
        for memory in &endpoints {
            let ancestry = self.ancestry(key.tenant_id, memory.workarea_id).await?;
            let access = effective_access(&grants, &caller.denials, &ancestry);
            // Edge authority asks reader on both ends, and a claim endpoint
            // is a row, so the row must also be in the key's read scope: its
            // own in any state, or promoted (roles-and-keys.md footnote 5;
            // `readable`). Another author's held, rejected, demoted or
            // erased row is outside it and indistinguishable from absent.
            let in_scope =
                access.roles.contains(&Role::Reader) && self.readable(&caller, memory).await?;
            if !in_scope {
                return Err(AppError::NotFound);
            }
            proposable.push(resolve_verb(key.role, &access, Verb::ClaimEdge(&req.label)));
        }
        if let [Err(refusal), Err(_)] = proposable.as_slice() {
            // Both ends are readable, so the first end's refusal is
            // `access.grant_refused` naming its workarea.
            return Err(match refusal {
                VerbRefusal::VerbCeiling { role, .. } => AppError::VerbCeiling {
                    role: *role,
                    verb: VERB,
                },
                VerbRefusal::GrantRefused { needed_role, .. } => AppError::GrantRefused {
                    workarea_id: endpoints[0].workarea_id,
                    needed_role: *needed_role,
                },
                VerbRefusal::NotFound => AppError::NotFound,
            });
        }
        let (src, dst) = (&endpoints[0], &endpoints[1]);
        check_edge_label(&req.label, src, dst)?;

        let edge_id = self.ids.new_id();
        let payload = json!({
            "edge_id": edge_id.to_string(),
            "src_artifact_id": src.memory_id.to_string(),
            "dst_artifact_id": dst.memory_id.to_string(),
            "label": req.label,
            "proposer_key_id": key.key_id.to_string(),
        });
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::CLAIM_EDGE_PROPOSED,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let edge = ClaimEdgeRecord {
            edge_id,
            tenant_id: key.tenant_id,
            src_artifact_id: src.memory_id,
            dst_artifact_id: dst.memory_id,
            label: req.label.clone(),
            state: EdgeState::Pending,
            proposer_key_id: key.key_id,
            proposer_principal: key.principal.clone(),
            ratifier_key_id: None,
            created_at: event.envelope.at.clone(),
        };
        let proposal = ClaimEdgeProposal::from(&edge);
        let idempotency = match &scope {
            Some(scope) => Some(IdempotencyPut {
                scope: scope.clone(),
                record: IdempotencyRecord {
                    request_digest: request_digest.clone(),
                    response: serde_json::to_value(&proposal)
                        .map_err(|e| AppError::StoredResponse(e.to_string()))?,
                },
            }),
            None => None,
        };
        let committed = self
            .commit(event, MetadataFold::ClaimEdgeProposed(edge), idempotency)
            .await;
        if let (Err(AppError::Store(StoreError::IdempotencyRecorded)), Some(scope)) =
            (&committed, &scope)
        {
            // A concurrent attempt under the same key committed first and
            // this one wrote nothing: its record answers (http-api.md
            // idempotency).
            let proposal = self
                .recorded(scope, &request_digest)
                .await?
                .ok_or(AppError::Store(StoreError::IdempotencyRecorded))?;
            return Ok(ClaimEdgeProposalOutcome {
                proposal,
                replayed: true,
            });
        }
        committed?;
        Ok(ClaimEdgeProposalOutcome {
            proposal,
            replayed: false,
        })
    }

    /// Propose that the promoted claim `artifact_id` join `workarea`'s
    /// brief: journalled `brief_edge.proposed` and stored pending (ADR-0046
    /// d7, d8). The refusal order: the verb ceiling, the idempotency record,
    /// the workarea resolved and dreamer effective access on it
    /// (`access.not_found` outside read access, `access.grant_refused`
    /// within it), the claim present and readable (`access.not_found`
    /// otherwise), then the claim promoted and its claim's served tip
    /// (`request.validation` naming `from_artifact_id`). A `held_by: service`
    /// key may propose; only ratification is review-class.
    pub async fn propose_brief_edge(
        &self,
        api_key: &str,
        req: &ProposeBriefEdge,
        idempotency_key: Option<&str>,
    ) -> Result<BriefEdgeProposalOutcome, AppError> {
        const VERB: &str = "brief_edge.propose";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Dreamer, VERB)?;
        let request_digest = brief_edge_request_digest(req)?;
        let scope = idempotency_key.map(|idem_key| IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id: Uuid::nil(),
            idem_key: idem_key.to_owned(),
        });
        if let Some(scope) = &scope
            && let Some(proposal) = self.recorded(scope, &request_digest).await?
        {
            return Ok(BriefEdgeProposalOutcome {
                proposal,
                replayed: true,
            });
        }
        let prev = self.head_event(key.tenant_id).await?;
        let workarea_id = self
            .resolve(key, NamedKind::Workarea, &req.workarea)
            .await?;
        self.require_access(&caller, workarea_id, Role::Dreamer, VERB)
            .await?;
        let memory = self
            .journal
            .memory(key.tenant_id, req.artifact_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if !self.readable(&caller, &memory).await? {
            return Err(AppError::NotFound);
        }
        if memory.status != MemoryStatus::Promoted || !memory.is_tip {
            return Err(AppError::BriefMemberNotServed {
                artifact_id: memory.memory_id,
            });
        }
        let edge_id = self.ids.new_id();
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::BRIEF_EDGE_PROPOSED,
            &key.principal,
            Some(key.key_id),
            json!({
                "edge_id": edge_id.to_string(),
                "artifact_id": memory.memory_id.to_string(),
                "workarea_id": workarea_id.to_string(),
                "proposer_key_id": key.key_id.to_string(),
            }),
        )?;
        let edge = BriefEdgeRecord {
            edge_id,
            tenant_id: key.tenant_id,
            artifact_id: memory.memory_id,
            workarea_id,
            state: EdgeState::Pending,
            proposer_key_id: key.key_id,
            proposer_principal: key.principal.clone(),
            ratifier_key_id: None,
            created_at: event.envelope.at.clone(),
            ratified_seq: None,
        };
        let proposal = BriefEdgeView::from(&edge);
        let idempotency = idempotency_put(scope.as_ref(), &request_digest, &proposal)?;
        let committed = self
            .commit(event, MetadataFold::BriefEdgeProposed(edge), idempotency)
            .await;
        if let (Err(AppError::Store(StoreError::IdempotencyRecorded)), Some(scope)) =
            (&committed, &scope)
        {
            let proposal = self
                .recorded(scope, &request_digest)
                .await?
                .ok_or(AppError::Store(StoreError::IdempotencyRecorded))?;
            return Ok(BriefEdgeProposalOutcome {
                proposal,
                replayed: true,
            });
        }
        committed?;
        Ok(BriefEdgeProposalOutcome {
            proposal,
            replayed: false,
        })
    }

    /// Ratify the pending brief edge `edge_id`: journalled `review.ratify`
    /// with `subject_kind: "brief_edge"` and stored ratified at that event's
    /// seq, the brief's order (ADR-0046 d8). The refusal order: the verb
    /// ceiling, a `held_by: service` key, the idempotency record, the edge
    /// present and its workarea within the key's dreamer effective access
    /// (`access.not_found` outside read access), R2 against the proposer's
    /// key and principal (`review.ratifier_is_proposer`), then the edge's
    /// state (`review.serve_invalid`, `consumed`).
    pub async fn ratify_brief_edge(
        &self,
        api_key: &str,
        edge_id: Uuid,
        idempotency_key: Option<&str>,
    ) -> Result<BriefEdgeRatification, AppError> {
        const VERB: &str = "brief_edge.ratify";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Dreamer, VERB)?;
        refuse_service(key, VERB)?;
        let request_digest = claim_edge_ratify_request_digest(edge_id)?;
        let scope = idempotency_key.map(|idem_key| IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id: Uuid::nil(),
            idem_key: idem_key.to_owned(),
        });
        if let Some(scope) = &scope
            && let Some(edge) = self.recorded(scope, &request_digest).await?
        {
            return Ok(BriefEdgeRatification {
                edge,
                journal_seq: None,
                replayed: true,
            });
        }
        let prev = self.head_event(key.tenant_id).await?;
        let edge = self
            .journal
            .brief_edge(key.tenant_id, edge_id)
            .await?
            .ok_or(AppError::NotFound)?;
        self.require_access(&caller, edge.workarea_id, Role::Dreamer, VERB)
            .await?;
        if ratifier_is_proposer(
            edge.proposer_key_id,
            &edge.proposer_principal,
            key.key_id,
            &key.principal,
        ) {
            return Err(AppError::RatifierIsProposer);
        }
        if edge.state != EdgeState::Pending {
            return Err(AppError::EdgeConsumed { edge_id });
        }
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::REVIEW_RATIFY,
            &key.principal,
            Some(key.key_id),
            json!({
                "subject_kind": "brief_edge",
                "artifact_id": edge.artifact_id.to_string(),
                "version": Value::Null,
                "edge_id": edge_id.to_string(),
                "workarea_id": edge.workarea_id.to_string(),
                "verdict_event_ids": [],
                "ratifier_key_id": key.key_id.to_string(),
                "same_user": same_user(None, key_user(key)),
                "policy_snapshot": review_policy(),
            }),
        )?;
        let seq = event.envelope.seq;
        let ratified = BriefEdgeView::from(&BriefEdgeRecord {
            state: EdgeState::Ratified,
            ratifier_key_id: Some(key.key_id),
            ratified_seq: Some(seq),
            ..edge
        });
        let idempotency = idempotency_put(scope.as_ref(), &request_digest, &ratified)?;
        let committed = self
            .commit(
                event,
                MetadataFold::BriefEdgeRatified {
                    edge_id,
                    ratifier_key_id: key.key_id,
                },
                idempotency,
            )
            .await;
        match (committed, &scope) {
            (Err(AppError::Store(StoreError::IdempotencyRecorded)), Some(scope)) => {
                let edge = self
                    .recorded(scope, &request_digest)
                    .await?
                    .ok_or(AppError::Store(StoreError::IdempotencyRecorded))?;
                Ok(BriefEdgeRatification {
                    edge,
                    journal_seq: None,
                    replayed: true,
                })
            }
            (Err(AppError::Store(StoreError::EdgeNotPending { edge_id })), _) => {
                Err(AppError::EdgeConsumed { edge_id })
            }
            (Err(e), _) => Err(e),
            (Ok(_), _) => Ok(BriefEdgeRatification {
                edge: ratified,
                journal_seq: Some(seq),
                replayed: false,
            }),
        }
    }

    /// `POST /v2/edges/{edge_id}/ratify` for either kind: the edge's kind
    /// follows from its id (ADR-0039 d9). An id the tenant holds as a claim
    /// edge is ratified as one; any other id is ratified as a brief edge,
    /// which answers `access.not_found` when the tenant holds none.
    pub async fn ratify_edge(
        &self,
        api_key: &str,
        edge_id: Uuid,
        idempotency_key: Option<&str>,
    ) -> Result<EdgeRatification, AppError> {
        let caller = self.authenticate(api_key).await?;
        let is_claim = self
            .journal
            .claim_edge(caller.key.tenant_id, edge_id)
            .await?
            .is_some();
        if is_claim {
            Ok(EdgeRatification::Claim(
                self.ratify_claim_edge(api_key, edge_id, idempotency_key)
                    .await?,
            ))
        } else {
            Ok(EdgeRatification::Brief(
                self.ratify_brief_edge(api_key, edge_id, idempotency_key)
                    .await?,
            ))
        }
    }

    /// `GET /v2/edges?kind=brief`: one page of `workarea`'s brief edges,
    /// pending and ratified, newest first, each served only when the key
    /// reads its claim, at reader effective access on the workarea
    /// (`access.not_found` outside it) (ADR-0046 d7). The cursor is the
    /// claim-edge listing's, bound to the key and the workarea.
    pub async fn brief_edges(
        &self,
        api_key: &str,
        workarea: Reference,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<BriefEdgePage, AppError> {
        const VERB: &str = "brief_edge.list";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reader, VERB)?;
        let workarea_id = self.resolve(key, NamedKind::Workarea, &workarea).await?;
        self.require_access(&caller, workarea_id, Role::Reader, VERB)
            .await?;
        let filter = ClaimEdgeFilter {
            artifact_id: None,
            workarea_id: Some(workarea_id),
        };
        let now = self.clock.now();
        let after = cursor
            .map(|cursor| ClaimEdgeCursor::resume(cursor, key.key_id, &filter, &now))
            .transpose()?;
        let position = |e: &BriefEdgeRecord| (e.created_at.as_str().to_owned(), e.edge_id);
        let mut served = Vec::new();
        for edge in self.journal.brief_edges(key.tenant_id, workarea_id).await? {
            if let Some(after) = &after
                && position(&edge) >= (after.created_at.as_str().to_owned(), after.edge_id)
            {
                continue;
            }
            let Some(memory) = self.journal.memory(key.tenant_id, edge.artifact_id).await? else {
                continue;
            };
            if self.readable(&caller, &memory).await? {
                served.push(edge);
            }
            if served.len() > limit {
                break;
            }
        }
        let more = served.len() > limit;
        served.truncate(limit);
        let next_cursor = served.last().filter(|_| more).map(|last| {
            ClaimEdgeCursor {
                key_id: key.key_id,
                position: ClaimEdgePosition {
                    created_at: last.created_at.clone(),
                    edge_id: last.edge_id,
                },
                filter,
                issued_at: now.clone(),
            }
            .encode()
        });
        Ok(BriefEdgePage {
            items: served.iter().map(BriefEdgeView::from).collect(),
            next_cursor,
        })
    }

    /// Ratify the pending claim edge `edge_id`: journalled `review.ratify`
    /// with `subject_kind: "claim_edge"`, the edge's one review act at k = 1,
    /// and stored ratified naming the ratifier (event-catalog.md; ADR-0008
    /// d5). The refusal order: the verb ceiling, a `held_by: service` key
    /// before any lookup, the idempotency record, the edge and both its
    /// endpoints present and in read scope, dreamer on both endpoints'
    /// workareas, the proposal's label check, R2 — the proposer's key or
    /// principal, then either endpoint's lineage closure — then the edge's
    /// state, then for `oml:is_a` the cycle invariant over that label's
    /// ratified edges (roles-and-keys.md; ADR-0012; ADR-0021 d5). The
    /// `oml:replaced_by` cycle check belongs to the deprecation verdict that
    /// carries the pointer (ADR-0021 d3).
    /// The head is read before the edges the cycle check reads, so a
    /// ratification committing between them conflicts this one and the
    /// transport's head retry reads them again (ADR-0002).
    // reading: the docs state each refusal but not their order; this is
    // propose_claim_edge's order with review's service refusal after the
    // ceiling, and edge authority's dreamer on both ends, which also reads
    // ADR-0021 d6's both-vocabularies rule for a term mapping.
    pub async fn ratify_claim_edge(
        &self,
        api_key: &str,
        edge_id: Uuid,
        idempotency_key: Option<&str>,
    ) -> Result<ClaimEdgeRatification, AppError> {
        const VERB: &str = "claim_edge.ratify";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Dreamer, VERB)?;
        refuse_service(key, VERB)?;

        // The route names no workarea, so the scope's workarea is nil
        // (http-api.md idempotency), as for the proposal.
        let request_digest = claim_edge_ratify_request_digest(edge_id)?;
        let scope = idempotency_key.map(|idem_key| IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id: Uuid::nil(),
            idem_key: idem_key.to_owned(),
        });
        if let Some(scope) = &scope
            && let Some(edge) = self.recorded(scope, &request_digest).await?
        {
            return Ok(ClaimEdgeRatification {
                edge,
                journal_seq: None,
                replayed: true,
            });
        }

        let prev = self.head_event(key.tenant_id).await?;

        let edge = self
            .journal
            .claim_edge(key.tenant_id, edge_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let mut endpoints = Vec::with_capacity(2);
        for id in [edge.src_artifact_id, edge.dst_artifact_id] {
            let memory = self
                .journal
                .memory(key.tenant_id, id)
                .await?
                .ok_or(AppError::NotFound)?;
            // An endpoint outside read scope answers as an absent edge
            // (roles-and-keys.md footnote 5; `readable`).
            let ancestry = self.ancestry(key.tenant_id, memory.workarea_id).await?;
            if !(caller.effective(&ancestry).contains(&Role::Reader)
                && self.readable(&caller, &memory).await?)
            {
                return Err(AppError::NotFound);
            }
            endpoints.push(memory);
        }
        for memory in &endpoints {
            self.require_access(&caller, memory.workarea_id, Role::Dreamer, VERB)
                .await?;
        }
        let (src, dst) = (&endpoints[0], &endpoints[1]);
        // The proposal's label check again: an `oml:replaced_by` or
        // `oml:consider` edge is ratified only inside its deprecation's
        // verdict, and `oml:about` only once classification is built, so a
        // pending one of either is refused as its proposal is (ADR-0021 d3).
        check_edge_label(&edge.label, src, dst)?;

        // R2 against both endpoints' flattened closures plus their authors,
        // pending and ratified edges alike (http-api.md, claim edges).
        let mut principals = BTreeSet::new();
        let mut key_ids = BTreeSet::new();
        for memory in &endpoints {
            principals.insert(memory.author_principal.clone());
            principals.extend(memory.lineage_principals.iter().cloned());
            key_ids.insert(memory.author_key_id);
            key_ids.extend(memory.lineage_key_ids.iter().copied());
        }
        let mut closure = LineageClosure::new();
        for principal in &principals {
            closure.add_principal(principal.clone());
        }
        for key_id in &key_ids {
            closure.add_key_id(*key_id);
        }
        check_ratifier(
            &Ratifier {
                role: key.role,
                principal: &key.principal,
                key_id: key.key_id,
            },
            edge.proposer_key_id,
            &edge.proposer_principal,
            &closure,
        )
        .map_err(|refusal| match refusal {
            RatifyRefusal::NotDreamer(role) => AppError::VerbCeiling { role, verb: VERB },
            RatifyRefusal::RatifierIsProposer => AppError::RatifierIsProposer,
            RatifyRefusal::AuthorInLineage => AppError::AuthorInLineage,
        })?;

        let state = edge_transition(&edge.label, edge.state, EdgeState::Ratified)
            .map_err(|_| AppError::EdgeConsumed { edge_id })?;

        if let Some(label @ OmlLabel::IsA) = OmlLabel::parse(&edge.label) {
            let candidate = TermEdge {
                label,
                from: term_curie(src.kind, &src.claim_key)?,
                to: term_curie(dst.kind, &dst.claim_key)?,
            };
            let mut ratified = Vec::new();
            for (from, to) in self
                .journal
                .ratified_term_edges(key.tenant_id, &edge.label)
                .await?
            {
                ratified.push(TermEdge {
                    label,
                    from: term_curie(MemoryKind::Term, &from)?,
                    to: term_curie(MemoryKind::Term, &to)?,
                });
            }
            check_acyclic(&ratified, &candidate)?;
        }

        let closure_doc = json!({
            "principals": principals.iter().collect::<Vec<_>>(),
            "key_ids": key_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
        });
        let payload = json!({
            "subject_kind": "claim_edge",
            "artifact_id": Value::Null,
            "version": Value::Null,
            "edge_id": edge_id.to_string(),
            // At k = 1 the ratify call is the edge's one review act, so no
            // verdict precedes it.
            "verdict_event_ids": [],
            "ratifier_key_id": key.key_id.to_string(),
            "same_user": same_user(None, key_user(key)),
            "policy_snapshot": {
                "k": 1,
                "required_roles": ["dreamer"],
                "independence_axis": "principal",
                "human_required": false,
            },
            "lineage_closure_digest": sha256_digest_string(&canonical_bytes(&closure_doc)?),
        });
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::REVIEW_RATIFY,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let seq = event.envelope.seq;
        let ratified_edge = ClaimEdgeProposal::from(&ClaimEdgeRecord {
            state,
            ratifier_key_id: Some(key.key_id),
            ..edge
        });
        let idempotency = match &scope {
            Some(scope) => Some(IdempotencyPut {
                scope: scope.clone(),
                record: IdempotencyRecord {
                    request_digest: request_digest.clone(),
                    response: serde_json::to_value(&ratified_edge)
                        .map_err(|e| AppError::StoredResponse(e.to_string()))?,
                },
            }),
            None => None,
        };
        let committed = self
            .commit(
                event,
                MetadataFold::ClaimEdgeRatified {
                    edge_id,
                    ratifier_key_id: key.key_id,
                },
                idempotency,
            )
            .await;
        match (committed, &scope) {
            (Err(AppError::Store(StoreError::IdempotencyRecorded)), Some(scope)) => {
                // A concurrent attempt under the same key committed first
                // and this one wrote nothing: its record answers.
                let edge = self
                    .recorded(scope, &request_digest)
                    .await?
                    .ok_or(AppError::Store(StoreError::IdempotencyRecorded))?;
                Ok(ClaimEdgeRatification {
                    edge,
                    journal_seq: None,
                    replayed: true,
                })
            }
            // A commit before this one decided the edge: nothing was
            // written, this event included.
            (Err(AppError::Store(StoreError::EdgeNotPending { .. })), _) => {
                Err(AppError::EdgeConsumed { edge_id })
            }
            (committed, _) => {
                committed?;
                Ok(ClaimEdgeRatification {
                    edge: ratified_edge,
                    journal_seq: Some(seq),
                    replayed: false,
                })
            }
        }
    }

    /// `GET /v2/edges?kind=claim` (http-api.md edges and pagination;
    /// ADR-0008 d5): one page of the tenant's claim edges, pending and
    /// ratified, newest first, narrowed to `query`'s workarea, by id or name
    /// (ADR-0026), whose subtree holds either endpoint, and its version at
    /// either endpoint. A reader key is needed: an admin key answers
    /// `access.verb_ceiling`, since an edge joins content and an admin
    /// structure read never includes it (ADR-0026 d15). A named workarea
    /// asks reader effective there by the refusal order. An edge is served
    /// only when both its endpoints are in the key's read scope, a row in a
    /// node where reader is effective that is promoted or the key's own
    /// (`readable`), so the read never names an unreadable version's id.
    /// The store selects by that scope, so one keyset page of at most
    /// `limit + 1` edges answers the call. The cursor is bound to the key,
    /// to `api.cursor_ttl` and to the filter, its workarea resolved, so
    /// either spelling of the workarea resumes it.
    pub async fn claim_edges(
        &self,
        api_key: &str,
        query: ClaimEdgeQuery,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ClaimEdgePage, AppError> {
        const VERB: &str = "claim_edge.list";
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        let tenant_id = key.tenant_id;
        require_admissible(key, Role::Reader, VERB)?;
        let workarea_id = self
            .resolve_opt(key, NamedKind::Workarea, query.workarea.as_ref())
            .await?;
        if let Some(node) = workarea_id {
            self.require_access(&caller, node, Role::Reader, VERB)
                .await?;
        }
        let filter = ClaimEdgeFilter {
            artifact_id: query.artifact_id,
            workarea_id,
        };
        let now = self.clock.now();
        let after = cursor
            .map(|cursor| ClaimEdgeCursor::resume(cursor, key.key_id, &filter, &now))
            .transpose()?;

        // reading: the read scope is pushed into the store's query, not
        // filtered after it, so the keyset LIMIT counts only servable edges
        // and a page is one bounded store read whatever the tenant holds.
        // The cost left is the scope itself: every reader node of the key,
        // one subtree read per reader grant, bound into the query as an
        // array that grows with the tenant's workareas, unmeasured (FR-7,
        // FR-25).
        let readable_nodes = self.nodes_holding(&caller, Role::Reader).await?;
        let within = match workarea_id {
            None => None,
            Some(node) => {
                let subtree: BTreeSet<Uuid> = self
                    .journal
                    .workarea_subtree(tenant_id, node)
                    .await?
                    .into_iter()
                    .map(|w| w.workarea_id)
                    .collect();
                Some(
                    readable_nodes
                        .iter()
                        .copied()
                        .filter(|n| subtree.contains(n))
                        .collect(),
                )
            }
        };
        let selection = ClaimEdgeSelection {
            artifact_id: filter.artifact_id,
            within,
            readable_nodes,
            reader_key_id: key.key_id,
        };
        // One edge past the page says whether another page exists.
        let mut served = self
            .journal
            .claim_edges(
                tenant_id,
                &selection,
                after.as_ref(),
                limit.saturating_add(1),
            )
            .await?;
        let more = served.len() > limit;
        served.truncate(limit);
        let next_cursor = served.last().filter(|_| more).map(|last| {
            ClaimEdgeCursor {
                key_id: key.key_id,
                position: ClaimEdgePosition {
                    created_at: last.created_at.clone(),
                    edge_id: last.edge_id,
                },
                filter,
                issued_at: now.clone(),
            }
            .encode()
        });
        Ok(ClaimEdgePage {
            items: served.iter().map(ClaimEdgeProposal::from).collect(),
            next_cursor,
        })
    }

    /// Verify the tenant chain end to end with the domain procedure
    /// (chain-format.md steps 1–7 and 9). Admin ceiling (roles-and-keys.md:
    /// chain verification is an admin verb).
    pub async fn verify_chain(&self, api_key: &str) -> Result<ChainHead, AppError> {
        let key = self.authenticate(api_key).await?.key;
        require_role(&key, Role::Admin, "verify_chain")?;
        let events = self.whole_chain(key.tenant_id).await?;
        let stated_head = self.journal.chain_head(key.tenant_id).await?;
        let keys = KeySet::new(vec![self.signer.verifying_key()]);
        Ok(oml_domain::verify_chain(
            &events,
            &keys,
            stated_head.as_ref(),
        )?)
    }

    /// The events tail (http-api.md#events-tail, the single declaration
    /// site; ADR-0025 d15): reader on the scope by the refusal order — the
    /// node the query's `workarea` names, else the key's default, else
    /// `request.workarea_unresolved` — then one bounded scan of at most
    /// `limit` raw events from `after_seq + 1`. An event stays only where
    /// the per-family rule ([`tail_visible`]) shows it to this key within
    /// subtree(scope), judged on the event's own payload and its subject's
    /// authoring key and never on later row state, and it is served with
    /// only the names the key may learn. `next_after_seq` is the last raw
    /// seq scanned, or `after_seq` when no raw event follows it, and
    /// `head_seq` is read in the scan's snapshot; an `after_seq` beyond it is
    /// refused.
    pub async fn tail_events(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workarea: Option<Reference>,
        after_seq: u64,
        limit: usize,
    ) -> Result<TailPage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reader, "events_tail")?;
        let scope = self.target_workarea(key, workarea.as_ref()).await?;
        self.require_access(&caller, scope, Role::Reader, "events_tail")
            .await?;
        let scan = self
            .journal
            .events_with_subjects(key.tenant_id, after_seq.saturating_add(1), limit)
            .await?;
        if after_seq > scan.head_seq {
            return Err(AppError::AfterSeqBeyondHead {
                after_seq,
                head_seq: scan.head_seq,
            });
        }
        let next_after_seq = scan
            .events
            .last()
            .map_or(after_seq, |e| e.event.envelope.seq);

        // The own key set is the calling key alone until rotation is built
        // (roles-and-keys.md#own).
        let own_keys = caller.own_key_ids();
        let tail_caller = TailCaller {
            own_keys: &own_keys,
            grants: &caller.grants,
            denials: &caller.denials,
            scope,
        };
        let mut ancestries: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
        let mut events = Vec::new();
        for TailEvent { event, subject } in scan.events {
            let envelope = &event.envelope;
            let Some(judged) = judged_subject(envelope, subject.as_ref()) else {
                continue;
            };
            let ancestry: &Vec<Uuid> = match ancestries.entry(judged.node) {
                Entry::Occupied(known) => known.into_mut(),
                Entry::Vacant(new) => new.insert(self.ancestry(key.tenant_id, judged.node).await?),
            };
            let tail_subject = TailSubject {
                workareas: std::slice::from_ref(ancestry),
                author_key: judged.author_key,
                promoted: judged.promoted,
                self_signed: judged.self_signed,
                outside_audience: judged.outside_audience,
            };
            if !tail_visible(&envelope.event_type, &tail_subject, &tail_caller) {
                continue;
            }
            let names_actor = envelope.api_key_id.is_some_and(|k| own_keys.contains(&k));
            let names_node = !caller.effective(ancestry).is_empty();
            let payload = served_payload(envelope, judged.claim, names_actor, names_node);
            events.push(TailedEvent {
                event,
                names_actor,
                payload,
            });
        }
        Ok(TailPage {
            events,
            head_seq: scan.head_seq,
            next_after_seq,
        })
    }
}

/// The connection verbs that read the connection rows: a store that serves
/// connector keys (postgres-schema.md "connections (ADR-0031)").
impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// Mint a fresh connect code for a connector key by id or name: admin
    /// key role only. A bearer key, a revoked key and a deployment without
    /// `oauth.issuer` are refused with `oauth.connect_refused`, by that
    /// reason, journalling nothing. `connection.code_issued` names the key's
    /// unredeemed code in `voids_code_id` and its commit voids every code of
    /// the key from which no connection has opened, so a key has at most
    /// one live code; the code is returned once, after that commit (http-api.md
    /// `POST /v2/keys/{key_id}/connect-codes`; ADR-0031 d7, d8).
    ///
    /// Keys carry no expiry in this build (`key.issued.expiry` is null), so
    /// `expired` is never the answer here.
    pub async fn mint_connect_code(
        &self,
        api_key: &str,
        key: Reference,
    ) -> Result<ConnectCodeIssued, AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "mint_connect_code")?;
        self.mint_connect_code_by(&admin, key).await
    }

    /// [`App::mint_connect_code`] once the issuer is known: an admin key, or
    /// a person's `<user>-web` session key for a key naming its user, which
    /// the web use case has checked (ADR-0045 d9, d16).
    pub(crate) async fn mint_connect_code_by(
        &self,
        admin: &KeyRecord,
        key: Reference,
    ) -> Result<ConnectCodeIssued, AppError> {
        let admin = admin.clone();
        let tenant_id = admin.tenant_id;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        let target = self
            .journal
            .key(tenant_id, key_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if target.credential == Credential::Bearer {
            return Err(connect_refused(ConnectRefused::BearerKey));
        }
        if target.credential == Credential::Session {
            return Err(connect_refused(ConnectRefused::SessionKey));
        }
        if target.revoked_at.is_some() {
            return Err(connect_refused(ConnectRefused::Revoked));
        }
        let oauth = self
            .oauth_issuer
            .as_ref()
            .ok_or_else(|| connect_refused(ConnectRefused::NotConfigured))?;
        let voids = self
            .journal
            .connect_codes(tenant_id, key_id)
            .await?
            .into_iter()
            .find(|c| c.redeemed_at.is_none() && c.voided_seq.is_none())
            .map(|c| c.code_id);
        let (code, shown) = self.new_connect_code(oauth, key_id);
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::CONNECTION_CODE_ISSUED,
            &admin.principal,
            Some(admin.key_id),
            code_issued_payload(&code, admin.key_id, voids),
        )?;
        self.commit(event, MetadataFold::ConnectCodeIssued { code, voids }, None)
            .await?;
        Ok(shown)
    }

    /// A key's open and revoked connections in creation order, the key by
    /// id or name: admin key role only, and served whether or not
    /// `oauth.issuer` is set. An `authorized` connection is not journalled
    /// and is not listed; a key the tenant does not hold is absent
    /// (http-api.md, cli/reference.md `oml keys show`; ADR-0031 d1, d11).
    pub async fn list_connections(
        &self,
        api_key: &str,
        key: Reference,
    ) -> Result<Vec<ConnectionRecord>, AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "list_connections")?;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        if self.journal.key(admin.tenant_id, key_id).await?.is_none() {
            return Err(AppError::NotFound);
        }
        let mut connections = self.journal.connections(admin.tenant_id, key_id).await?;
        connections.retain(|c| c.state != ConnectionState::Authorized);
        Ok(connections)
    }

    /// End an open connection of a key, the key by id or name: admin key
    /// role only, and served whether or not `oauth.issuer` is set. It
    /// journals `connection.revoked` with the cause `admin` and
    /// `reason_digest` over the reason given, else null, and its commit
    /// voids every code of the key from which no connection has opened. A
    /// connection that is not the key's or not open, an `authorized` one
    /// included, answers `access.not_found` as an unknown id does
    /// (http-api.md `POST /v2/keys/{key_id}/connections/{connection_id}/revoke`;
    /// event-catalog.md `connection.revoked`; ADR-0031 d1, d8, d11, d12).
    pub async fn revoke_connection(
        &self,
        api_key: &str,
        key: Reference,
        connection_id: Uuid,
        reason: Option<&str>,
    ) -> Result<ConnectionRecord, AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "revoke_connection")?;
        let tenant_id = admin.tenant_id;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        let connection = self
            .journal
            .connection(tenant_id, connection_id)
            .await?
            .filter(|c| c.key_id == key_id && c.state == ConnectionState::Open)
            .ok_or(AppError::NotFound)?;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::CONNECTION_REVOKED,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "connection_id": connection_id.to_string(),
                "key_id": key_id.to_string(),
                "revoked_by_key_id": admin.key_id.to_string(),
                "cause": RevokedCause::Admin.as_str(),
                "reason_digest": reason.map(|r| sha256_digest_string(r.as_bytes())),
            }),
        )?;
        let receipt = self
            .commit(
                event,
                MetadataFold::ConnectionRevoked {
                    connection_id,
                    cause: RevokedCause::Admin,
                },
                None,
            )
            .await?;
        Ok(ConnectionRecord {
            state: ConnectionState::Revoked(RevokedCause::Admin),
            revoked_seq: Some(receipt.seq),
            ..connection
        })
    }

    /// `GET /oauth/authorize` (http-api.md; ADR-0031 d4, d9 step 1). It runs
    /// one order: the parameter bounds, a repeated parameter among them,
    /// before anything is fetched; then the client — a document client from
    /// a fresh cached copy, else by `client_lookup`, else from a copy kept
    /// within its 24 hours when the fetch fails or a cap is full, and a
    /// registered client from its row; then the redirect URI; each of those
    /// refusals is a page and never a redirect. Then `response_type=code`,
    /// S256 PKCE, `resource` and `scope`, each refusal redirected to the
    /// accepted redirect URI with the `state` and `iss`. A valid request is
    /// sealed by `sealer` into the handle the page's form carries, alive 10
    /// minutes with five attempts. It writes no row but the document cache
    /// and journals nothing. Without `oauth.issuer` the endpoint does not
    /// exist (`access.not_found`).
    pub async fn authorize_request<F>(
        &self,
        params: AuthorizeParams,
        client_lookup: &F,
        sealer: &dyn HandleSealer,
    ) -> Result<AuthorizeOutcome, AppError>
    where
        F: ClientDocumentFetcher,
    {
        let issuer = self.oauth_issuer.as_ref().ok_or(AppError::NotFound)?;
        let now = self.clock.now();
        let bounds = AuthorizeBounds {
            state: params.state.as_deref(),
            client_id: params.client_id.as_deref(),
            redirect_uri: params.redirect_uri.as_deref(),
            code_challenge: params.code_challenge.as_deref(),
        };
        let page = |refusal| Ok(AuthorizeOutcome::PageRefusal(refusal));
        if params.repeated {
            return page(Refusal::RequestValidation(Validation::ParameterRepeated));
        }
        if let Err(refusal) = check_authorize_bounds(&bounds) {
            return page(refusal);
        }
        let Some(client_id) = params.client_id.as_deref() else {
            return page(Refusal::RequestValidation(Validation::ClientIdAbsent));
        };
        let client = match classify_client_id(client_id) {
            Ok(client) => client,
            Err(refusal) => return page(refusal),
        };
        let client = match self.authorize_client(client, client_lookup, &now).await? {
            Ok(client) => client,
            Err(refusal) => return page(refusal),
        };
        let redirect = match select_redirect(&client.redirect_uris, params.redirect_uri.as_deref())
        {
            Ok(redirect) => redirect,
            Err(refusal) => return page(refusal),
        };
        let code_challenge = match authorize_later_checks(&params, issuer) {
            Ok(code_challenge) => code_challenge.to_owned(),
            Err(error) => {
                return Ok(AuthorizeOutcome::RedirectRefusal {
                    error,
                    redirect_uri: redirect.uri.to_owned(),
                    state: params.state,
                    iss: issuer.as_str().to_owned(),
                });
            }
        };
        let expires_at = EventTime::from_unix_micros(
            now.unix_micros() + AUTHORIZATION_REQUEST_SECONDS * 1_000_000,
        )
        .expect("ten minutes after a clock reading is a time the fixed-width form writes");
        let request = AuthorizationRequest {
            client_id: client_id.to_owned(),
            kind: client.kind,
            client_name: client.client_name,
            document_host: client.document_host,
            metadata_digest: client.metadata_digest,
            redirect_uri: redirect.uri.to_owned(),
            redirect_uri_sent: redirect.sent,
            code_challenge,
            state: params.state.clone(),
            resource: issuer.resource().to_owned(),
            expires_at,
            attempts_left: ATTEMPTS_PER_PAGE,
        };
        Ok(AuthorizeOutcome::Page(AuthorizePage {
            handle: sealer.seal(&request.encode()),
            redirect_host: redirect_host(&request.redirect_uri).to_owned(),
            verified_by: request.document_host,
            client_name: request.client_name,
            attempts_left: request.attempts_left,
            expires_at: request.expires_at,
        }))
    }

    /// `POST /oauth/authorize` with `handle` and `connect_code` (http-api.md;
    /// ADR-0031 d8, d9 step 2). The sealed handle must name a live request,
    /// before the code is read: one that does not open is unknown and
    /// answers `oauth.authorization_expired` on the page; one that opens to
    /// a request past its ten minutes is known and ends it as
    /// `access_denied` on its redirect (errors.md). Neither spends the code
    /// nor counts a failure. The entry is decoded
    /// by Crockford's rule, found by its digest before any tenant is known,
    /// checked against its key's state as `/mcp` reads it, and spent by one
    /// conditional update. A correct code writes the request's row under a
    /// fresh 256-bit handle, of which the store keeps only the SHA-256, and
    /// answers the confirmation. Every other entry answers one refusal, with
    /// one attempt fewer under a new sealed handle, and the fifth ends the
    /// request with `access_denied`; each counts one deployment-wide failure.
    /// It journals nothing. Without `oauth.issuer` the endpoint does not
    /// exist (`access.not_found`).
    pub async fn enter_code(
        &self,
        handle: &str,
        entry: &str,
        sealer: &dyn HandleSealer,
    ) -> Result<CodeEntryOutcome, AppError> {
        let issuer = self.oauth_issuer.as_ref().ok_or(AppError::NotFound)?;
        let now = self.clock.now();
        let opened = sealer.open(handle);
        let request = match open_request(opened.as_deref(), &now) {
            Ok(request) => request,
            Err(error @ Refusal::AuthorizationExpired(AuthorizationExpired::Lapsed)) => {
                let request = opened
                    .as_deref()
                    .and_then(AuthorizationRequest::decode)
                    .expect("a lapsed request is one that opened and decoded");
                return Ok(CodeEntryOutcome::Denied {
                    error,
                    redirect_uri: request.redirect_uri,
                    state: request.state,
                    iss: issuer.as_str().to_owned(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        let Some((tenant_id, code, key)) = self.spend_entry(entry, &now).await? else {
            self.connect_code_failures.fetch_add(1, Ordering::Relaxed);
            return Ok(match after_wrong_entry(request.attempts_left) {
                Ok(attempts_left) => CodeEntryOutcome::WrongCode {
                    handle: sealer.seal(
                        &AuthorizationRequest {
                            attempts_left,
                            ..request
                        }
                        .encode(),
                    ),
                    attempts_left,
                },
                Err(error) => CodeEntryOutcome::Denied {
                    error,
                    redirect_uri: request.redirect_uri,
                    state: request.state,
                    iss: issuer.as_str().to_owned(),
                },
            });
        };
        let confirmation = base62_43(&self.ids.new_secret());
        let view = self
            .confirmation_view(tenant_id, &key, confirmation.clone())
            .await?;
        self.journal
            .authorization_insert(AuthorizationRecord {
                authorization_id: self.ids.new_id(),
                handle_hash: Sha256Hash::digest(confirmation.as_bytes()),
                client_id: request.client_id,
                client_kind: match request.kind {
                    RequestClientKind::Document => ClientKind::Cimd,
                    RequestClientKind::Registered => ClientKind::Dcr,
                },
                metadata_digest: request.metadata_digest,
                redirect_uri: request.redirect_uri,
                redirect_uri_sent: request.redirect_uri_sent,
                code_challenge: request.code_challenge,
                state: request.state,
                expires_at: request.expires_at,
                code_id: code.code_id,
                code_tenant_id: tenant_id,
                key_id: key.key_id,
            })
            .await?;
        Ok(CodeEntryOutcome::Confirmation(Box::new(view)))
    }

    /// A code entry spent: the code's tenant, row and key, or `None` for
    /// every entry that is not a correct code of a live key — malformed,
    /// unknown, expired, voided, spent, a revoked key's, or one a racing
    /// entry spent first (ADR-0031 d8). Keys carry no expiry in this build
    /// (`key.issued.expiry` is null), so a key is active or revoked.
    async fn spend_entry(
        &self,
        entry: &str,
        now: &EventTime,
    ) -> Result<Option<(Uuid, ConnectCodeRecord, KeyRecord)>, AppError> {
        let Ok(symbols) = CodeSymbols::parse(entry) else {
            return Ok(None);
        };
        let Some(found) = self
            .journal
            .code_by_hash(&Sha256Hash(symbols.digest()))
            .await?
        else {
            return Ok(None);
        };
        let key = self
            .journal
            .key(found.tenant_id, found.row.key_id)
            .await?
            .filter(|key| key.revoked_at.is_none());
        let row = CodeRow {
            redeemed: found.row.redeemed_at.is_some(),
            voided: found.row.voided_seq.is_some(),
            expired: now.unix_micros() >= found.row.expires_at.unix_micros(),
            key: if key.is_some() {
                KeyState::Active
            } else {
                KeyState::Revoked
            },
        };
        let Some(key) = key.filter(|_| check_code_entry(Some(&row)).is_ok()) else {
            return Ok(None);
        };
        let spent = self
            .journal
            .spend_code(found.tenant_id, found.row.code_id, now)
            .await?;
        Ok(spent.then_some((found.tenant_id, found.row, key)))
    }

    /// What the confirmation shows for `key` (http-api.md "The
    /// authorization page"; ADR-0031 d9 step 2): its tenant's name, its
    /// name, principal and role, each granted workarea with the grant's
    /// role, and for each the roles effective there, which name the
    /// verb-kind sentences' workareas.
    async fn confirmation_view(
        &self,
        tenant_id: Uuid,
        key: &KeyRecord,
        handle: String,
    ) -> Result<ConfirmationView, AppError> {
        let inconsistent = |what: String| AppError::Store(StoreError::Backend(what));
        let tenant_name = self.journal.tenant_name(tenant_id).await?.ok_or_else(|| {
            inconsistent(format!(
                "tenant {tenant_id} holds a connect code but no name"
            ))
        })?;
        let access = self
            .journal
            .key_access(tenant_id, key.key_id)
            .await?
            .ok_or_else(|| inconsistent(format!("key {} has no access row", key.key_id)))?;
        let grants: Vec<Grant> = access.grants.into_iter().map(|g| g.grant).collect();
        let denials: Vec<KeyDenial> = access.denials.into_iter().map(|d| d.denial).collect();
        let mut view = ConfirmationView {
            handle,
            tenant_name,
            key_name: key.name.clone(),
            principal: key.principal.clone(),
            role: key.role,
            grants: Vec::new(),
            writes_in: Vec::new(),
            reads_in: Vec::new(),
            proposes_in: Vec::new(),
            self_signed_in: Vec::new(),
        };
        for grant in &grants {
            let ancestry = self
                .journal
                .workarea_ancestry(tenant_id, grant.node)
                .await?
                .filter(|ancestry| !ancestry.is_empty())
                .ok_or_else(|| inconsistent(format!("granted node {} is not held", grant.node)))?;
            let workarea = ancestry[0].name.clone();
            let ids: Vec<Uuid> = ancestry.iter().map(|w| w.workarea_id).collect();
            let effective = effective_roles(&grants, &denials, &ids);
            for (role, names) in [
                (Role::Writer, &mut view.writes_in),
                (Role::Reader, &mut view.reads_in),
                (Role::Dreamer, &mut view.proposes_in),
            ] {
                if effective.contains(&role) && !names.contains(&workarea) {
                    names.push(workarea.clone());
                }
            }
            view.grants.push(ConfirmationGrant {
                workarea_id: grant.node,
                workarea,
                role: grant.role,
            });
        }
        Ok(view)
    }

    /// `POST /oauth/authorize` with `handle` and `action` (http-api.md;
    /// ADR-0031 d8, d9 step 3). The confirmation's handle must name a
    /// request row a verified code wrote, else it is unknown and answers
    /// `oauth.authorization_expired` on the page. A row past the request's
    /// ten minutes, or whose code is voided, is known: either action
    /// answers `oauth.authorization_expired` as `access_denied` on the
    /// row's redirect with its `state` and `iss` (errors.md). Connect, in the store's one transaction, takes the row,
    /// stamps the client's `connected_at` and creates the connection
    /// `authorized` with the digest of a 256-bit authorization code alive 60
    /// seconds, and answers the redirect with that code, the `state` and
    /// `iss`. Cancel takes the row and answers the redirect with
    /// `access_denied`. Either way the connect code stays spent, and a
    /// later confirmation with the handle names no row. It journals nothing.
    pub async fn confirm(
        &self,
        handle: &str,
        action: ConfirmAction,
    ) -> Result<ConfirmOutcome, AppError> {
        let issuer = self.oauth_issuer.as_ref().ok_or(AppError::NotFound)?;
        let now = self.clock.now();
        let expired = |reason| AppError::OAuth(Refusal::AuthorizationExpired(reason));
        let handle_hash = Sha256Hash::digest(handle.as_bytes());
        let iss = issuer.as_str().to_owned();
        let lapsed = |row: &AuthorizationRecord| now.unix_micros() >= row.expires_at.unix_micros();
        let denied = |reason, row: AuthorizationRecord| ConfirmOutcome::Denied {
            error: Refusal::AuthorizationExpired(reason),
            redirect_uri: row.redirect_uri,
            state: row.state,
            iss: issuer.as_str().to_owned(),
        };
        match action {
            ConfirmAction::Cancel => {
                let row = self
                    .journal
                    .take_authorization_by_handle(&handle_hash)
                    .await?
                    .ok_or(expired(AuthorizationExpired::NoRequest))?;
                if lapsed(&row) {
                    return Ok(denied(AuthorizationExpired::Lapsed, row));
                }
                let voided = self
                    .journal
                    .connect_codes(row.code_tenant_id, row.key_id)
                    .await?
                    .iter()
                    .find(|c| c.code_id == row.code_id)
                    .is_none_or(|c| c.voided_seq.is_some());
                if let Err(Refusal::AuthorizationExpired(reason)) = check_confirmation(voided) {
                    return Ok(denied(reason, row));
                }
                Ok(ConfirmOutcome::Cancelled {
                    redirect_uri: row.redirect_uri,
                    state: row.state,
                    iss,
                })
            }
            ConfirmAction::Connect => {
                let row = self
                    .journal
                    .authorization(&handle_hash)
                    .await?
                    .ok_or(expired(AuthorizationExpired::NoRequest))?;
                if lapsed(&row) {
                    return Ok(denied(AuthorizationExpired::Lapsed, row));
                }
                let code = base62_43(&self.ids.new_secret());
                let auth_code_expires_at = EventTime::from_unix_micros(
                    now.unix_micros() + AUTHORIZATION_CODE_TTL_SECS * 1_000_000,
                )
                .expect("a minute after a clock reading is a time the fixed-width form writes");
                let grant = ConnectGrant {
                    connection_id: self.ids.new_id(),
                    resource: issuer.resource().to_owned(),
                    auth_code_hash: Sha256Hash::digest(code.as_bytes()),
                    auth_code_expires_at,
                    at: now,
                };
                match self.journal.connection_connect(&handle_hash, grant).await? {
                    ConnectOutcome::Authorized { .. } => Ok(ConfirmOutcome::Connected {
                        redirect_uri: row.redirect_uri,
                        code,
                        state: row.state,
                        iss,
                    }),
                    ConnectOutcome::NoRequest => Err(expired(AuthorizationExpired::NoRequest)),
                    ConnectOutcome::CodeVoided => Ok(denied(AuthorizationExpired::CodeVoided, row)),
                }
            }
        }
    }

    /// `POST /oauth/token` with `grant_type=authorization_code` (http-api.md;
    /// ADR-0031 d5, d10, d12). Client authentication is refused first, then
    /// a `resource` other than `<issuer>/mcp`; then the connection the
    /// code's digest finds, whatever its tenant, is judged by
    /// [`decide_exchange`] with the client stored on it, never the client
    /// tables. A pass commits `connection.opened` under the chain lock,
    /// naming the key's open connection it replaces, and its fold redeems
    /// the code by a conditional update and inserts the minted pair in that
    /// transaction. A redeemed code presented by its client with the
    /// verifier revokes the connection (`code_reuse`); every other refusal
    /// consumes and revokes nothing. An opening the fold refuses — a race
    /// for the code, or a mint, revocation or `key.revoked` landed since the
    /// read — is judged once more on a fresh read. Without `oauth.issuer`
    /// the endpoint does not exist (`access.not_found`).
    pub async fn exchange(&self, form: ExchangeForm) -> Result<IssuedTokens, AppError> {
        let issuer = self.oauth_issuer.as_ref().ok_or(AppError::NotFound)?;
        refuse_client_authentication(form.client_authentication.as_ref())?;
        check_resource(form.resource.as_deref(), issuer)?;
        let code_hash = Sha256Hash::digest(form.code.as_bytes());
        let request = ExchangeRequest {
            client_id: &form.client_id,
            code_verifier: &form.code_verifier,
            redirect_uri: form.redirect_uri.as_deref(),
        };
        let mut retried = false;
        loop {
            let now = self.clock.now();
            let found = self.journal.connection_by_auth_code(&code_hash).await?;
            let judged = match &found {
                Some(found) => Some(self.exchange_row(found, &now).await?),
                None => None,
            };
            let decision = decide_exchange(
                judged.as_ref().map(|(facts, _)| facts.row()).as_ref(),
                &request,
                issuer,
            );
            let (Some(found), Some((_, key))) = (found, judged) else {
                return Err(exchange_refusal(decision));
            };
            match decision {
                ExchangeDecision::Redeem => {}
                ExchangeDecision::Refuse(_) => return Err(exchange_refusal(decision)),
                ExchangeDecision::RevokeForReuse => {
                    self.revoke_for_reuse(
                        found.tenant_id,
                        &found.row,
                        key.as_ref(),
                        RevokedCause::CodeReuse,
                    )
                    .await?;
                    return Err(exchange_refusal(decision));
                }
            }
            let Some(key) = key else {
                return Err(exchange_refusal(ExchangeDecision::Refuse(
                    AuthorizationCodeInvalid::KeyEnded,
                )));
            };
            match self.open_connection(found, &key, code_hash, &now).await {
                Err(AppError::Store(StoreError::ConnectionNotOpenable { .. })) if !retried => {
                    retried = true;
                }
                Err(AppError::Store(StoreError::ConnectionNotOpenable { .. })) => {
                    return Err(exchange_refusal(ExchangeDecision::Refuse(
                        AuthorizationCodeInvalid::Unknown,
                    )));
                }
                other => return other,
            }
        }
    }

    /// `POST /oauth/token` with `grant_type=refresh_token` (http-api.md;
    /// ADR-0031 d10, d11, d12). Client authentication is refused first,
    /// then a `resource` other than `<issuer>/mcp`, then a string not in the
    /// refresh-token format, before any lookup. The token's digest finds
    /// its tenant, and the store's one transaction judges it by
    /// [`decide_refresh`] under its connection's row lock and rotates it
    /// only on a pass, journalling nothing. A rotated token presented again
    /// revokes the connection (`refresh_reuse`) in a `connection.revoked`
    /// of its own under the chain lock; expiry, another client or a
    /// resource that is not current rotates and revokes nothing. Without
    /// `oauth.issuer` the endpoint does not exist (`access.not_found`).
    pub async fn refresh(&self, form: RefreshForm) -> Result<IssuedTokens, AppError> {
        let issuer = self.oauth_issuer.as_ref().ok_or(AppError::NotFound)?;
        refuse_client_authentication(form.client_authentication.as_ref())?;
        check_resource(form.resource.as_deref(), issuer)?;
        let token_hash = Sha256Hash(refresh_token_digest(&form.refresh_token)?);
        let unknown = Refusal::RefreshInvalid(RefreshInvalid::Unknown);
        let found = self
            .journal
            .token_by_hash(&token_hash, TokenKind::Refresh)
            .await?
            .ok_or(unknown)?;
        let now = self.clock.now();
        let (issued, next) = self.mint_token_pair(&now);
        let client_id = form.client_id.as_str();
        let outcome = self
            .journal
            .rotate_refresh(found.tenant_id, &token_hash, next, &now, |facts| {
                decide_refresh(facts.map(RefreshFacts::row).as_ref(), client_id, issuer)
            })
            .await?;
        match (outcome.decision, outcome.facts) {
            (RefreshDecision::Rotate, _) => Ok(issued),
            (RefreshDecision::RevokeForReuse, Some(facts)) => {
                let connection = self
                    .journal
                    .connection(found.tenant_id, facts.connection_id)
                    .await?
                    .ok_or(unknown)?;
                let key = self.journal.key(found.tenant_id, connection.key_id).await?;
                self.revoke_for_reuse(
                    found.tenant_id,
                    &connection,
                    key.as_ref(),
                    RevokedCause::RefreshReuse,
                )
                .await?;
                Err(Refusal::RefreshInvalid(RefreshInvalid::Reused).into())
            }
            (decision, _) => Err(decision.refusal().unwrap_or(unknown).into()),
        }
    }

    /// The caller an access token acts as at `/mcp`: the token's digest
    /// finds an access row and never a refresh row, the token unexpired,
    /// its connection open and bound to the current `<issuer>/mcp`, and its
    /// key neither revoked nor expired; the caller is then exactly the one
    /// the key's own authentication builds (roles-and-keys.md connections;
    /// ADR-0031 d10, d11). Anything else is [`AppError::Unauthorized`].
    pub async fn resolve_access_token(&self, token: &str) -> Result<Caller, AppError> {
        let issuer = self.oauth_issuer.as_ref().ok_or(AppError::Unauthorized)?;
        if token_kind(token) != Some(TokenKind::Access) {
            return Err(AppError::Unauthorized);
        }
        let found = self
            .journal
            .token_by_hash(&Sha256Hash::digest(token.as_bytes()), TokenKind::Access)
            .await?
            .ok_or(AppError::Unauthorized)?;
        if self.clock.now().unix_micros() >= found.row.expires_at.unix_micros() {
            return Err(AppError::Unauthorized);
        }
        let connection = self
            .journal
            .connection(found.tenant_id, found.row.connection_id)
            .await?
            .filter(|c| c.state == ConnectionState::Open && c.resource == issuer.resource())
            .ok_or(AppError::Unauthorized)?;
        let key = self
            .journal
            .key(found.tenant_id, connection.key_id)
            .await?
            .filter(|k| k.revoked_at.is_none());
        self.observer
            .credential(key.as_ref(), Some(connection.connection_id));
        self.caller_of(key.ok_or(AppError::Unauthorized)?).await
    }

    /// `POST /oauth/register` (http-api.md; RFC 7591; ADR-0031 d5): the body
    /// is validated, a `client_id` minted and the registration stored under
    /// the provider tenant with the digest of the body as received, and the
    /// answer carries no secret, its `token_endpoint_auth_method` `none`
    /// whether sent or absent. Past the unused registrations' cap the store
    /// writes nothing and answers [`StoreError::CapReached`], `limit.rate`.
    /// It journals nothing. Without `oauth.issuer` the endpoint does not
    /// exist (`access.not_found`).
    pub async fn register(&self, body: &[u8]) -> Result<RegisteredClient, AppError> {
        if self.oauth_issuer.is_none() {
            return Err(AppError::NotFound);
        }
        let metadata = validate_registration(body)?;
        let registration = ClientDescription {
            client_id: self.ids.new_id().to_string(),
            client_name: metadata.client_name,
            redirect_uris: metadata.redirect_uris,
            metadata_digest: Sha256Hash::digest(body),
        };
        let issued_at = self.clock.now();
        self.journal
            .insert_dcr(registration.clone(), &issued_at)
            .await?;
        Ok(RegisteredClient {
            client_id: registration.client_id,
            issued_at,
            client_name: registration.client_name,
            redirect_uris: registration.redirect_uris,
            token_endpoint_auth_method: "none",
        })
    }

    /// The exchange's facts about the connection a code's digest found, at
    /// `now`: the code's expiry and voiding and the key's state, beside the
    /// key's row, `None` for a key its tenant does not hold.
    async fn exchange_row(
        &self,
        found: &TenantRow<ConnectionRecord>,
        now: &EventTime,
    ) -> Result<(ExchangeFacts, Option<KeyRecord>), AppError> {
        let connection = &found.row;
        let key = self.journal.key(found.tenant_id, connection.key_id).await?;
        let code_voided = self
            .journal
            .connect_codes(found.tenant_id, connection.key_id)
            .await?
            .iter()
            .find(|c| c.code_id == connection.code_id)
            .is_none_or(|c| c.voided_seq.is_some());
        let facts = ExchangeFacts {
            connection: connection.clone(),
            key: match &key {
                Some(k) if k.revoked_at.is_none() => KeyState::Active,
                _ => KeyState::Revoked,
            },
            code_expired: connection
                .auth_code_expires_at
                .as_ref()
                .is_none_or(|at| now.unix_micros() >= at.unix_micros()),
            code_voided,
        };
        Ok((facts, key))
    }

    /// The exchange's `connection.opened`, as the connector key, replacing
    /// the key's open connection, with the minted pair in its fold: the
    /// pair is answered only once the commit holds.
    async fn open_connection(
        &self,
        found: TenantRow<ConnectionRecord>,
        key: &KeyRecord,
        auth_code_hash: Sha256Hash,
        now: &EventTime,
    ) -> Result<IssuedTokens, AppError> {
        let tenant_id = found.tenant_id;
        let connection = found.row;
        let replaces = self
            .journal
            .connections(tenant_id, key.key_id)
            .await?
            .into_iter()
            .find(|c| c.state == ConnectionState::Open)
            .map(|c| c.connection_id);
        let (issued, tokens) = self.mint_token_pair(now);
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::CONNECTION_OPENED,
            &key.principal,
            Some(key.key_id),
            json!({
                "connection_id": connection.connection_id.to_string(),
                "key_id": key.key_id.to_string(),
                "code_id": connection.code_id.to_string(),
                "client_id": connection.client_id,
                "client_kind": connection.client_kind.as_str(),
                "client_metadata_digest": connection.client_metadata_digest.to_prefixed(),
                "redirect_uri": connection.redirect_uri,
                "resource": connection.resource,
                "replaces_connection_id": replaces.map(|c| c.to_string()),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::ConnectionOpened {
                connection_id: connection.connection_id,
                replaces,
                auth_code_hash,
                tokens,
            },
            None,
        )
        .await?;
        Ok(issued)
    }

    /// A reuse's `connection.revoked`, as the connector key whose past
    /// credential triggered it, `revoked_by_key_id` and `reason_digest`
    /// null, under the chain lock (event-catalog.md `connection.revoked`;
    /// ADR-0031 d10, d12). A connection another request revoked first is
    /// revoked once: that commit's refusal is not this request's failure.
    async fn revoke_for_reuse(
        &self,
        tenant_id: Uuid,
        connection: &ConnectionRecord,
        key: Option<&KeyRecord>,
        cause: RevokedCause,
    ) -> Result<(), AppError> {
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::CONNECTION_REVOKED,
            key.map_or("", |k| k.principal.as_str()),
            Some(connection.key_id),
            json!({
                "connection_id": connection.connection_id.to_string(),
                "key_id": connection.key_id.to_string(),
                "revoked_by_key_id": Value::Null,
                "cause": cause.as_str(),
                "reason_digest": Value::Null,
            }),
        )?;
        let fold = MetadataFold::ConnectionRevoked {
            connection_id: connection.connection_id,
            cause,
        };
        match self.commit(event, fold, None).await {
            Ok(_) => Ok(()),
            Err(err) => {
                let revoked = self
                    .journal
                    .connection(tenant_id, connection.connection_id)
                    .await?
                    .is_some_and(|c| matches!(c.state, ConnectionState::Revoked(_)));
                if revoked { Ok(()) } else { Err(err) }
            }
        }
    }

    /// A fresh access and refresh token, each from the id generator's
    /// secret, living [`ACCESS_TOKEN_TTL_SECS`] and
    /// [`REFRESH_TOKEN_TTL_SECS`] from `now`: the answer and what the store
    /// keeps of it, the digests (ADR-0031 d10).
    fn mint_token_pair(&self, now: &EventTime) -> (IssuedTokens, TokenPair) {
        let access = Token::mint(TokenKind::Access, self.ids.new_secret());
        let refresh = Token::mint(TokenKind::Refresh, self.ids.new_secret());
        let minted = |token: &Token, ttl: i64| MintedToken {
            token_hash: Sha256Hash(token.digest()),
            expires_at: EventTime::from_unix_micros(now.unix_micros() + ttl * 1_000_000).expect(
                "a token's lifetime after a clock reading is a time the fixed-width form writes",
            ),
        };
        let pair = TokenPair {
            access: minted(&access, ACCESS_TOKEN_TTL_SECS),
            refresh: minted(&refresh, REFRESH_TOKEN_TTL_SECS),
        };
        let issued = IssuedTokens {
            access_token: access,
            refresh_token: refresh,
            expires_in: ACCESS_TOKEN_TTL_SECS.unsigned_abs(),
        };
        (issued, pair)
    }

    /// The authorize request's client (ADR-0031 d5): a document client's
    /// metadata from a cached copy within its `Cache-Control` lifetime,
    /// else a fresh fetch, validated and cached, else — the fetch failed or
    /// a cap is full — a copy kept within 24 hours of its fetch; a
    /// registered client's from its registration. The inner `Err` is the
    /// refusal the page answers.
    async fn authorize_client<F>(
        &self,
        client: ClientId<'_>,
        client_lookup: &F,
        now: &EventTime,
    ) -> Result<Result<AcceptedClient, Refusal>, AppError>
    where
        F: ClientDocumentFetcher,
    {
        let url = match client {
            ClientId::Registered(client_id) => {
                let row = self.journal.oauth_client(client_id).await?;
                return Ok(row
                    .filter(|row| row.kind == ClientKind::Dcr)
                    .map(|row| AcceptedClient::from_row(row, None))
                    .ok_or(Refusal::ClientInvalid(ClientInvalid::Unknown)));
            }
            ClientId::Document(url) => url,
        };
        let host = Some(url.host().to_owned());
        let age = |row: &OAuthClientRecord| {
            row.fetched_at
                .as_ref()
                .map_or(i64::MAX, |at| now.unix_micros() - at.unix_micros())
        };
        let kept = self
            .journal
            .oauth_client(url.as_str())
            .await?
            .filter(|row| row.kind == ClientKind::Cimd)
            .filter(|row| age(row) < i64::from(DOCUMENT_KEPT_SECONDS) * 1_000_000);
        if let Some(row) = kept
            .as_ref()
            .filter(|row| age(row) < i64::from(row.max_age.unwrap_or(0)) * 1_000_000)
        {
            self.journal.touch_cimd(url.as_str(), now).await?;
            return Ok(Ok(AcceptedClient::from_row(row.clone(), host)));
        }
        match client_lookup.fetch(url.as_str()).await {
            Ok(fetched) => {
                let metadata = match validate_document(&url, &fetched.body) {
                    Ok(metadata) => metadata,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                let document = ClientDescription {
                    client_id: url.as_str().to_owned(),
                    client_name: metadata.client_name,
                    redirect_uris: metadata.redirect_uris,
                    metadata_digest: Sha256Hash::digest(&fetched.body),
                };
                self.journal
                    .upsert_cimd(document.clone(), now, fetched.max_age)
                    .await?;
                Ok(Ok(AcceptedClient {
                    kind: RequestClientKind::Document,
                    client_name: document.client_name,
                    redirect_uris: document.redirect_uris,
                    metadata_digest: document.metadata_digest,
                    document_host: host,
                }))
            }
            Err(failure) => match kept {
                Some(row) => {
                    self.journal.touch_cimd(url.as_str(), now).await?;
                    Ok(Ok(AcceptedClient::from_row(row, host)))
                }
                None => Ok(Err(Refusal::ClientInvalid(match failure {
                    DocumentFetchFailure::Failed => ClientInvalid::Fetch,
                    DocumentFetchFailure::Busy => ClientInvalid::FetchBusy,
                }))),
            },
        }
    }
}

/// What the token exchange judges a connection by: the row the code's
/// digest found and what was read beside it.
struct ExchangeFacts {
    connection: ConnectionRecord,
    key: KeyState,
    code_expired: bool,
    code_voided: bool,
}

impl ExchangeFacts {
    fn row(&self) -> ExchangeRow<'_> {
        let c = &self.connection;
        ExchangeRow {
            state: c.state,
            client_id: &c.client_id,
            code_challenge: &c.code_challenge,
            redirect_uri: &c.redirect_uri,
            redirect_uri_sent: c.redirect_uri_sent,
            resource: &c.resource,
            key: self.key,
            code_redeemed: c.auth_code_redeemed_at.is_some(),
            code_expired: self.code_expired,
            code_voided: self.code_voided,
        }
    }
}

/// The exchange's refusal, `oauth.authorization_code_invalid` whatever its
/// reason (`invalid_grant`).
fn exchange_refusal(decision: ExchangeDecision) -> AppError {
    decision
        .refusal()
        .unwrap_or(Refusal::AuthorizationCodeInvalid(
            AuthorizationCodeInvalid::Unknown,
        ))
        .into()
}

/// Every client is public, so client authentication on a token request is
/// `oauth.client_invalid`, by the scheme of its header, before any lookup
/// (http-api.md `POST /oauth/token`; ADR-0031 d5).
fn refuse_client_authentication(sent: Option<&ClientAuthentication>) -> Result<(), AppError> {
    match sent {
        None => Ok(()),
        Some(ClientAuthentication::Header { scheme }) => Err(AppError::ClientAuthentication {
            scheme: Some(scheme.clone()),
        }),
        Some(ClientAuthentication::BodySecret) => {
            Err(AppError::ClientAuthentication { scheme: None })
        }
    }
}

/// An authorize request's accepted client: what the sealed handle and the
/// page carry of it (ADR-0031 d9 step 1).
struct AcceptedClient {
    kind: RequestClientKind,
    client_name: String,
    redirect_uris: Vec<String>,
    metadata_digest: Sha256Hash,
    document_host: Option<String>,
}

impl AcceptedClient {
    /// A stored client: a registration with no host, or a cached document
    /// with its URL's host.
    fn from_row(row: OAuthClientRecord, document_host: Option<String>) -> Self {
        Self {
            kind: match row.kind {
                ClientKind::Cimd => RequestClientKind::Document,
                ClientKind::Dcr => RequestClientKind::Registered,
            },
            client_name: row.client_name,
            redirect_uris: row.redirect_uris,
            metadata_digest: row.metadata_digest,
            document_host,
        }
    }
}

/// The authorize checks after the client and the redirect URI are
/// accepted, in their order (ADR-0031 d4): `response_type` exactly `code`,
/// S256 PKCE, `resource` absent or `<issuer>/mcp`, and `scope` absent or
/// naming `mcp` alone. Each refusal is redirected; a request that passes
/// answers its S256 `code_challenge`.
fn authorize_later_checks<'p>(
    params: &'p AuthorizeParams,
    issuer: &Issuer,
) -> Result<&'p str, Refusal> {
    if params.response_type.as_deref() != Some("code") {
        return Err(Refusal::RequestValidation(
            Validation::ResponseTypeUnsupported,
        ));
    }
    check_pkce_method(
        params.code_challenge.as_deref(),
        params.code_challenge_method.as_deref(),
    )?;
    let code_challenge = params
        .code_challenge
        .as_deref()
        .ok_or(Refusal::PkceRequired(PkceRequired::ChallengeAbsent))?;
    check_resource(params.resource.as_deref(), issuer)?;
    if params
        .scope
        .as_deref()
        .is_some_and(|scope| !scope.split(' ').all(|token| token == SCOPE))
    {
        return Err(Refusal::RequestValidation(Validation::ScopeUnsupported));
    }
    Ok(code_challenge)
}

/// The host a redirect URI delivers to, as the page shows it: the
/// authority after `://` without its port, a bracketed IPv6 literal kept
/// whole. Every URI here is inside the allowlist (ADR-0031 d6, d9).
fn redirect_host(uri: &str) -> &str {
    let authority = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let authority = authority.split(['/', '?']).next().unwrap_or(authority);
    match authority.find(']') {
        Some(end) if authority.starts_with('[') => &authority[..=end],
        _ => authority.split(':').next().unwrap_or(authority),
    }
}

/// What the content tail judges one event by, assembled from the event's
/// own payload and its subject's authoring row (http-api.md#events-tail,
/// the subject table; ADR-0025 d15).
struct JudgedSubject {
    /// Whether the subject is a claim version, not a workarea node.
    claim: bool,
    /// The subject's workarea: the payload's `workarea_id`, else the
    /// subject row's.
    node: Uuid,
    author_key: Option<Uuid>,
    promoted: bool,
    self_signed: bool,
    outside_audience: bool,
}

/// The subject of `envelope` as the tail judges it, or `None` for an event
/// it cannot place, which the tail never shows. The author is the envelope
/// key of a `submission.*` or `simulation.write` event and the subject
/// row's authoring key for a `lifecycle.*` one. *Promoted* holds for a
/// `lifecycle.promote` or `lifecycle.supersede`, the promotion of the
/// version it names; every other `lifecycle.*` event fails it, so only its
/// author sees it — the journal read that judges the latest transition
/// lands with the write paths of those types, none of which this build
/// journals. A subject is `self_signed` unless the promoting payload says
/// `false`, and outside every audience when its submission names a
/// simulation, whose audience is unbuilt. `lifecycle.reassign` and
/// `workarea.relate_*` name two workareas, are never journalled by this
/// build, and are not placed: they stay hidden until they are.
fn judged_subject(envelope: &Envelope, subject: Option<&SubjectRow>) -> Option<JudgedSubject> {
    let event_type = envelope.event_type.as_str();
    let payload = &envelope.payload;
    if event_type == "lifecycle.reassign" || event_type.starts_with("workarea.relate_") {
        return None;
    }
    let named = payload
        .get("workarea_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok());
    if event_type.starts_with("workarea.") {
        return Some(JudgedSubject {
            claim: false,
            node: named?,
            author_key: None,
            promoted: false,
            self_signed: false,
            outside_audience: false,
        });
    }
    let submission = event_type.starts_with("submission.") || event_type == "simulation.write";
    if !submission && !event_type.starts_with("lifecycle.") {
        return None;
    }
    let promoted = matches!(
        event_type,
        event_types::LIFECYCLE_PROMOTE | event_types::LIFECYCLE_SUPERSEDE
    );
    Some(JudgedSubject {
        claim: true,
        node: named.or(subject.map(|s| s.workarea_id))?,
        author_key: if submission {
            envelope.api_key_id
        } else {
            subject.map(|s| s.author_key_id)
        },
        promoted,
        self_signed: payload.get("self_signed") != Some(&Value::Bool(false)),
        outside_audience: payload.get("simulation_id").is_some_and(|s| !s.is_null())
            || payload.get("provenance").and_then(Value::as_str) == Some("simulation"),
    })
}

/// The payload as the tail serves it (http-api.md#events-tail, what a
/// served event names): to a key outside the actor's own key set every
/// field naming a key — `proposer_key_id`, `ratifier_key_id` and each
/// `*_by_key_id` — is null, and a claim subject's `workarea_id` on a node
/// where the key holds no role is null (roles-and-keys.md#own, naming a
/// node).
fn served_payload(envelope: &Envelope, claim: bool, names_actor: bool, names_node: bool) -> Value {
    let mut payload = envelope.payload.clone();
    if let Some(fields) = payload.as_object_mut() {
        for (field, value) in fields.iter_mut() {
            let names_key = field == "proposer_key_id"
                || field == "ratifier_key_id"
                || field.ends_with("_by_key_id");
            let withheld_node = claim && field == "workarea_id" && !names_node;
            if (names_key && !names_actor) || withheld_node {
                *value = Value::Null;
            }
        }
    }
    payload
}
