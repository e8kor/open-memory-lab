//! The use-cases: every verb the transports will front, enforcing the
//! constitution here — verb ceilings (R1), reviewer-never-author (R2),
//! promoted-only reads (R3) — never in an adapter (ADR-0019 d1). Every
//! workarea-scoped verb resolves effective access from the key's grants and
//! denials on each request, in the one refusal order of roles-and-keys.md
//! (ADR-0025 d1).

mod federation;
mod pages;
mod rescan;
mod search;
mod signup;
mod sparse;
mod usage;
mod web;
pub use federation::WideKey;
pub use pages::{
    AdminMemory, AdminPage, AdminSearchRequest, Capacity, CapacityGranted, OwnKey, QueueRow,
    REGION_COUNT_MAX, Region, Regions, VerdictTrail, WebListRequest, WorkareaOffer, WorkareaOffers,
};
pub use rescan::{RESCAN_MAX_EVENTS, RescanHit, RescanReport};
pub use search::SearchSettings;
pub use signup::{DRAWS_MAX, Person, SignupState, StartStatus, UserDisabled, UserRow};
pub use sparse::{SPARSE_BACKFILL_STEP, SparsePass, encode_sparse_pass};
pub use usage::UsageSettings;
pub use web::{Callback, CallbackRefusal, WEB_SESSION_TTL_SECS};

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::access::{
    AccessRefusal, Covers, Grant, IssuanceRefusal, KeyDenial, LabelledGrant, TailCaller,
    TailSubject, Verb, VerbRefusal, admissible_grant_roles, check_issuance, effective_access,
    effective_roles, effective_verbs, resolve, resolve_verb, tail_visible,
};
use oml_domain::anchor::{self as anchor_grammar, Anchor, SourceCheckBody};
use oml_domain::brief::{brief_cut, brief_order};
use oml_domain::budget::{budget_tokens, cut};
use oml_domain::claim_edges::{
    LabelRefusal, Ratifier, RatifyRefusal, check_label, check_ratifier, check_term_curies,
};
use oml_domain::guardrail::{
    self, GuardrailScope, LOAD_WARNING, OWN_HELD_MAX_LINES, SET_CAP, Severity, conflicts,
    guardrail_order, set_digest,
};
use oml_domain::in_progress::{
    self as in_progress, CuratorAct, CuratorRefusal, InProgressReason, KeyProfile, WorkareaMode,
    admissible_roles, crosses_into, curator_scope, curator_verb, may_set_mode, profile_admits,
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
use oml_domain::origin::OriginKind;
use oml_domain::similar::{
    Fingerprint, MAX_CANDIDATES as SIMILAR_MAX_CANDIDATES, MAX_LISTED as SIMILAR_MAX_LISTED,
    Relation, relate,
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
    JournalStore, MetadataFold, NoObserver, OutboxRow, Parsed, RequestObserver, Reranker,
    ScanOutcome, Scanner, Signer, SparseEncoder, StoreError, SubjectRow, TailEvent,
};
use crate::telemetry::{HITS_STORED, Label, RecallEvent, Remembered, Telemetry, TelemetryItem};
use crate::types::{
    AppError, AssignedNames, AuthorizationRecord, AuthorizeOutcome, AuthorizePage, AuthorizeParams,
    Bearer, BootstrapRequest, BootstrapResult, Brief, BriefEdgePage, BriefEdgeProposalOutcome,
    BriefEdgeRatification, BriefEdgeRecord, BriefEdgeView, ClaimEdgeFilter, ClaimEdgePage,
    ClaimEdgePosition, ClaimEdgeProposal, ClaimEdgeProposalOutcome, ClaimEdgeQuery,
    ClaimEdgeRatification, ClaimEdgeRecord, ClaimEdgeSelection, ClientAuthentication,
    ClientDescription, ClientKind, CodeEntryOutcome, ConfirmAction, ConfirmOutcome,
    ConfirmationGrant, ConfirmationView, ConnectCode, ConnectCodeIssued, ConnectCodeRecord,
    ConnectGrant, ConnectOutcome, ConnectionRecord, CreatedWorkarea, Credential, CrewCreateRequest,
    CrewPage, CrewRecord, CrewUpdate, DenialRecord, EdgeRatification, ExchangeForm, FormatFields,
    GrantRecord, GuardrailFields, GuardrailRequest, GuardrailSet, GuardrailVerdict, HIT_KINDS,
    IssueKeyRequest, IssuedKey, IssuedTokens, KeyAccess, KeyDetail, KeyRecord, ListRequest,
    ListingFilter, ListingPosition, ListingScope, LiveConnectCode, MemoryPage, MemoryRecord,
    MintedToken, NamedRef, NamingRefusal, OAuthClientRecord, OnSimilar, OwnHeldGuardrail,
    PROVIDER_TENANT, ProposeBriefEdge, ProposeClaimEdge, RecallHit, RecallRequest, RecallResult,
    RefreshFacts, RefreshForm, RegisteredClient, RerankOutcome, SIMILAR_KINDS, ServedGuardrail,
    ServedMemory, SimilarItem, SourceCheck, SourceCheckReceipt, SourceCheckRequest, SubmitOutcome,
    SubmitReceipt, SubmitRequest, SubmitVerb, SupersedeOutcome, SupersedeReceipt, SupersedeRequest,
    TailPage, TailedEvent, TenantRow, TokenPair, TransitionRecord, TransitionState, TransitionView,
    TreeNode, TreePage, UnnamedRows, Verdict, VerdictOutcome, VerdictRequest, VersionRef, Whoami,
    WorkareaInfo, WorkareaRecord, WorkareaStatusView,
};
use crate::types::{
    BULK_VERDICT_MAX, BulkItem, BulkVerdictRequest, HELD_PAGE_MAX, HeldFilter, HeldItem,
    HeldListingRequest, HeldPage,
};

use crate::search::{
    Access, CandidateScope, DID_YOU_MEAN_BELOW, Fused, Fusion, MatchMode, OwnHeldScope, Pipeline,
    RetrievalQuery, RetrieverKind, SearchConfig, Stage, TIME_DEPTH, Timings, apply_floor,
    fuse_capped, order_time_list, pipeline_access, pruned_words, query_terms, rerank_order,
};
use oml_domain::valid_time::{ValidTime, Window, parse_now, parse_window, ratified_end};

/// One pipeline's answer before the cuts: the fused candidates, the query's
/// terms, whether `match: all` fell back to any word, how the lists were
/// read and the words pruned (ADR-0049 d5, d6; ADR-0052 d11).
#[derive(Debug, Default)]
struct PipelineRun {
    fused: Vec<Fused>,
    terms: Vec<String>,
    fallback: bool,
    access: Access,
    pruned: Vec<String>,
}

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
/// (configuration.md; ADR-0031 d8). #117: until journalled settings are
/// built, this code constant is the lifetime of every connect code this
/// build mints, as [`CURSOR_TTL_SECS`](crate::cursor::CURSOR_TTL_SECS) is a
/// cursor's.
pub const CONNECT_CODE_TTL_SECS: i64 = 600;

/// The longest an admin may ask a connect code to live, 30 days, in seconds:
/// the reviewer code of a directory listing, minted only for a key confined
/// to one workarea (ADR-0067 d3).
pub const CONNECT_CODE_TTL_MAX_SECS: i64 = 30 * 24 * 3600;

/// `oauth.access_token_ttl`'s shipped default, 1 hour, in seconds: the
/// lifetime of every access token this build mints until settings are
/// built (configuration.md; #117; ADR-0031 d10).
pub const ACCESS_TOKEN_TTL_SECS: i64 = 3_600;

/// `oauth.refresh_token_ttl`'s shipped default, 30 days, in seconds: the
/// lifetime of every refresh token this build mints until settings are
/// built (configuration.md; #117; ADR-0031 d10).
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
    /// (ADR-0046 d8; #145).
    brief_max_tokens: u64,
    /// The pipelines this deployment serves and its default (ADR-0049 d4).
    search: SearchConfig,
    /// The cross-encoder the reranking pipelines call; `None` where the
    /// deployment loads none, and then no reranking pipeline is served
    /// (ADR-0053 d2, d8).
    reranker: Option<Arc<dyn Reranker>>,
    /// The recall telemetry's writer; `None` where the deployment records
    /// no recall (ADR-0054 d2).
    telemetry: Option<Arc<dyn Telemetry>>,
    /// Each key's feedback members checked today, by UTC day: the per-key
    /// cap, held before any lookup (ADR-0054 d6, as amended).
    feedback_counts: std::sync::Mutex<BTreeMap<Uuid, (i64, u64)>>,
    /// The learned sparse encoder the sparse pipelines read the query with
    /// and the encoder task writes kind 2 with; `None` where the deployment
    /// serves no sparse pipeline (ADR-0052 d14, d16).
    sparse: Option<Arc<dyn SparseEncoder>>,
    /// The claim formats this deployment serves (ADR-0056 d1, d3): prose
    /// alone until the composition gives it the compiled-in adapters.
    formats: Arc<dyn crate::ports::Formats>,
    /// Open sign-up and its counters; `None` where the deployment has no
    /// sign-up tenant, and then no quotas (ADR-0068 d1, d5).
    signup: Option<Arc<signup::SignupState>>,
    /// Each key whose recall met more than `CANDIDATES_DOCTOR` candidate
    /// sources since the lead started, by tenant, with the most it met: the
    /// doctor line a tenant admin reads (ADR-0072 d12). It decides nothing.
    wide_keys: std::sync::Mutex<BTreeMap<(Uuid, Uuid), usize>>,
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

    /// Whether `author_key_id` belongs to this caller's own-key set, without
    /// cloning that set for each row.
    fn owns(&self, author_key_id: Uuid) -> bool {
        if self.own_keys.is_empty() {
            self.key.key_id == author_key_id
        } else {
            self.own_keys.contains(&author_key_id)
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

/// The served node set of a scoped read (ADR-0030 d2), the nodes of it
/// inside the scope's in-progress root, where every author's held memories
/// are served (d3), and that root.
struct ServedNodes {
    nodes: Vec<Uuid>,
    inside: Vec<Uuid>,
    scope_root: Option<Uuid>,
}

/// One text a version's scan reads: the envelope kind it is scanned as, the
/// text, and where a finding on it is placed (`None` for the body).
pub(crate) struct ScanTarget<'a> {
    pub(crate) kind: &'static str,
    pub(crate) text: &'a str,
    pub(crate) at: Option<&'a str>,
}

/// Every text beside the anchor that a version's scan reads, in order: the
/// body, the title, then each view its format decoded (ADR-0056 d6). The
/// admission scan and the recording rescan both read this one list, so a
/// version stamped `scanned_under` was read whole under that rule set
/// (ADR-0065 d15; ADR-0074 d28).
pub(crate) fn scan_targets<'a>(
    kind: MemoryKind,
    body: &'a str,
    fields: &'a FormatFields,
    parsed: &'a Parsed,
) -> Vec<ScanTarget<'a>> {
    let mut out = vec![ScanTarget {
        kind: kind.as_str(),
        text: body,
        at: None,
    }];
    if let Some(title) = &fields.title {
        out.push(ScanTarget {
            kind: "title",
            text: title,
            at: Some("title"),
        });
    }
    out.extend(parsed.views.iter().map(|view| ScanTarget {
        kind: kind.as_str(),
        text: &view.text,
        at: Some(&view.path),
    }));
    out
}

/// Whether a held version may be served unreviewed inside an in-progress
/// subtree: a memory or derived fact, never a retirement version, never one
/// the scan held, which stays its author's until a reviewer reads it, and
/// only one last scanned under `current`, the server's rule set: a version
/// an older rule set passed stays its author's until a rescan re-evaluates
/// it (ADR-0030 d3; ADR-0065 d15).
fn served_unreviewed(m: &MemoryRecord, current: Option<&str>) -> bool {
    m.status == MemoryStatus::Held
        && !m.retirement
        && m.scan_finding.is_none()
        && m.scanned_under.as_deref() == current
        && matches!(m.kind, MemoryKind::Memory | MemoryKind::DerivedFact)
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
    // A curator key reaches only the verbs that read or narrow (ADR-0030
    // d9), before any lookup.
    if key.profile == Some(KeyProfile::Curator) {
        curator_verb(verb).map_err(|refusal| AppError::Curator { verb, refusal })?;
    }
    if admissible_roles(key.role, key.profile).contains(&needed) {
        Ok(())
    } else {
        Err(AppError::VerbCeiling {
            role: key.role,
            verb,
        })
    }
}

/// The role the refusal order reads for `needed`: a curator key's writer
/// grants confer the writer verbs, which a dreamer's ceiling would refuse
/// (ADR-0030 d8).
fn ceiling_role(key: &KeyRecord, needed: Role) -> Role {
    if key.profile == Some(KeyProfile::Curator) && needed == Role::Writer {
        Role::Writer
    } else {
        key.role
    }
}

/// How many times one item of a bulk verdict retries after a chain-head
/// conflict: the single verdict route's retry count (ADR-0055 d4).
const BULK_CHAIN_RETRIES: usize = 16;

/// The listing and the bulk verdict admit a dreamer key a person holds,
/// and refuse every other key before any lookup (ADR-0055 d1): a writer or
/// reader key (R1), a service key (ADR-0025 d1) and a dreamer key held by
/// an agent, whose crew reviews one serve at a time.
fn require_person_reviewer(key: &KeyRecord) -> Result<(), AppError> {
    if key.role == Role::Dreamer && key.held_by == HeldBy::Human {
        Ok(())
    } else {
        Err(AppError::BulkReviewHumanOnly {
            role: key.role,
            held_by: key.held_by,
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
    let mut doc = with_guardrail_fields(doc, req.severity, req.rule.as_ref());
    if let Some(members) = doc.as_object_mut() {
        for (name, value) in [
            ("valid_from", &req.valid_from),
            ("valid_until", &req.valid_until),
        ] {
            if let Some(value) = value {
                members.insert(name.to_owned(), Value::from(value.as_str()));
            }
        }
    }
    let doc = with_anchor(doc, req.anchor.as_ref());
    let doc = with_format(doc, req.format.as_deref(), req.title.as_deref());
    let doc = with_origin(doc, req.origin);
    sha256_digest_string(doc.to_string().as_bytes())
}

/// A request digest's document with the format and the title when sent, and
/// unchanged otherwise, so a request that sends neither digests as it did
/// before formats (ADR-0056 d13).
fn with_format(mut doc: Value, format: Option<&str>, title: Option<&str>) -> Value {
    if let Some(members) = doc.as_object_mut() {
        if let Some(format) = format {
            members.insert("format".to_owned(), Value::from(format));
        }
        if let Some(title) = title {
            members.insert("title".to_owned(), Value::from(title));
        }
    }
    doc
}

/// The name of a person's chat workarea under their user root (ADR-0063
/// d8).
pub const CHAT_WORKAREA: &str = "chat";

/// A request digest's document with `origin` when sent, and unchanged
/// otherwise, so a request without one digests as it did before
/// ADR-0063.
fn with_origin(mut doc: Value, origin: Option<OriginKind>) -> Value {
    if let (Some(kind), Some(members)) = (origin, doc.as_object_mut()) {
        members.insert("origin".to_owned(), kind.to_json());
    }
    doc
}

/// The kind a submission records: as declared, `manual` when none was
/// sent, after the two compositions with the anchor (ADR-0063 d2).
fn recorded_origin(
    origin: Option<OriginKind>,
    anchor: Option<&Anchor>,
) -> Result<OriginKind, AppError> {
    let kind = origin.unwrap_or(OriginKind::Manual);
    kind.check_with(anchor)
        .map_err(|e| AppError::OriginInvalid {
            field: e.field,
            reason: e.reason,
        })?;
    Ok(kind)
}

/// A request digest's document with the anchor when sent, and unchanged
/// otherwise, so an unanchored request digests as it did before anchors
/// (ADR-0057 d2).
fn with_anchor(mut doc: Value, anchor: Option<&Anchor>) -> Value {
    if let (Some(anchor), Some(members)) = (anchor, doc.as_object_mut()) {
        members.insert("anchor".to_owned(), anchor.to_json());
    }
    doc
}

/// The submission payload's `anchor`: the object as declared, or `null`
/// (ADR-0057 d2; event-catalog.md).
fn anchor_payload(anchor: Option<&Anchor>) -> Value {
    anchor.map_or(Value::Null, Anchor::to_json)
}

/// A request digest's document with a guardrail's `severity` and `rule`
/// when sent, and unchanged otherwise, so the digest of every request of
/// another kind is the one it was before guardrails (proposed ADR-0037
/// d10).
fn with_guardrail_fields(
    mut doc: Value,
    severity: Option<Severity>,
    rule: Option<&Value>,
) -> Value {
    if let Some(members) = doc.as_object_mut() {
        if let Some(severity) = severity {
            members.insert("severity".to_owned(), Value::from(severity.as_str()));
        }
        if let Some(rule) = rule {
            members.insert("rule".to_owned(), rule.clone());
        }
    }
    doc
}

/// A version's guardrail fields, checked as proposed ADR-0037 d10 states:
/// `severity` and `rule` are refused on every other kind and on a
/// retirement, `severity` is required on every other guardrail version
/// (`request.validation`), and then the scan's two guardrail classes run
/// over the NFC body and the rule (`submission.scan_veto`, its finding
/// `guardrail.too_long` or `guardrail.rule_unparsable`). `None` for a
/// version that carries neither.
fn guardrail_fields(
    kind: MemoryKind,
    retirement: bool,
    severity: Option<Severity>,
    rule: Option<&Value>,
    body: &str,
) -> Result<Option<Box<GuardrailFields>>, AppError> {
    if kind != MemoryKind::Guardrail || retirement {
        if severity.is_some() {
            return Err(AppError::GuardrailFieldRefused { field: "severity" });
        }
        if rule.is_some() {
            return Err(AppError::GuardrailFieldRefused { field: "rule" });
        }
        return Ok(None);
    }
    let severity = severity.ok_or(AppError::GuardrailSeverityRequired)?;
    guardrail::admit(severity, body, rule)
        .map_err(|refusal| AppError::ScanVeto(refusal.code().to_owned()))?;
    Ok(Some(Box::new(GuardrailFields {
        severity,
        rule: rule.cloned(),
    })))
}

/// The submission payload's guardrail members: the severity in clear and
/// the rule's digest, never the rule, which the row holds (proposed
/// ADR-0037 d14). A version with no guardrail fields journals neither.
fn guardrail_payload(
    payload: &mut Value,
    fields: Option<&GuardrailFields>,
) -> Result<(), AppError> {
    let (Some(fields), Some(members)) = (fields, payload.as_object_mut()) else {
        return Ok(());
    };
    let rule_digest = match &fields.rule {
        Some(rule) => Value::from(sha256_digest_string(&canonical_bytes(rule)?)),
        None => Value::Null,
    };
    members.insert("severity".to_owned(), Value::from(fields.severity.as_str()));
    members.insert("rule_digest".to_owned(), rule_digest);
    Ok(())
}

/// A version's valid time as sent, parsed and checked against its kind
/// (proposed ADR-0059 d2): `request.validation` naming the field for an end
/// in no accepted form, an empty interval, or valid time on a kind or a
/// retirement that takes none.
fn valid_time(
    from: Option<&str>,
    until: Option<&str>,
    kind: MemoryKind,
    retirement: bool,
) -> Result<ValidTime, AppError> {
    let valid = ValidTime::parse(from, until).map_err(AppError::ValidTime)?;
    valid
        .check_kind(kind, retirement)
        .map_err(AppError::ValidTime)?;
    Ok(valid)
}

/// The submission payload's and the request digest's valid-time members:
/// each end's normalized form, only when declared, so a version that
/// declares none signs and digests as it did before (proposed ADR-0059 d2).
fn with_valid_time(doc: &mut Value, valid: &ValidTime) {
    if let Some(members) = doc.as_object_mut() {
        if let Some(from) = &valid.from {
            members.insert("valid_from".to_owned(), Value::from(from.as_str()));
        }
        if let Some(until) = &valid.until {
            members.insert("valid_until".to_owned(), Value::from(until.as_str()));
        }
    }
}

/// The submission payload's `format`, always, and its `title` when the
/// version has one: both are signed with the version, and an event without
/// them reads as `text/plain` with no title (ADR-0056 d2).
fn with_format_fields(payload: &mut Value, fields: &FormatFields) {
    if let Some(members) = payload.as_object_mut() {
        members.insert("format".to_owned(), Value::from(fields.media_type.as_str()));
        if let Some(title) = &fields.title {
            members.insert("title".to_owned(), Value::from(title.as_str()));
        }
    }
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
            "expected_version": req.expected_version,
        },
    });
    let mut doc = doc;
    if let Some(body) = doc.get_mut("body") {
        *body = with_guardrail_fields(body.take(), req.severity, req.rule.as_ref());
        *body = with_anchor(body.take(), req.anchor.as_ref());
        *body = with_format(body.take(), req.format.as_deref(), req.title.as_deref());
        *body = with_origin(body.take(), req.origin);
        if let Some(members) = body.as_object_mut() {
            for (name, value) in [
                ("valid_from", &req.valid_from),
                ("valid_until", &req.valid_until),
            ] {
                if let Some(value) = value {
                    members.insert(name.to_owned(), Value::from(value.as_str()));
                }
            }
        }
    }
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
            same_user: outcome.same_user,
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
    /// Absent from a record a v0.5.6 server stored, which journalled no
    /// mark (ADR-0055 d7).
    #[serde(default)]
    same_user: bool,
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
            same_user: self.same_user,
        })
    }
}

/// What `node` is to the ladder (ADR-0046 d1): the universe root, a user
/// root (ADR-0044 d1), or an ordinary node.
/// What `set_workarea_mode` admits (ADR-0030 d1, d6): the node already in
/// the mode, which journals nothing, or a change with the mode it leaves
/// and, for a commit, the nodes it frees.
pub(crate) enum ModeAdmitted {
    Already(WorkareaRecord),
    Change {
        node: WorkareaRecord,
        current: WorkareaMode,
        held_free: Vec<Uuid>,
    },
}

fn ladder_node(node: &WorkareaRecord) -> LadderNode {
    if node.parent_id.is_none() {
        LadderNode::Universe
    } else if node.user_root {
        LadderNode::UserRoot
    } else {
        LadderNode::Ordinary
    }
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

/// The nodes a recall at a scope searches for hits: the readable ones and,
/// inside a user root, `universe` where the key reads it (ADR-0044 d10).
fn with_universe(
    caller: &Caller,
    readable: &[Uuid],
    scope_ancestry: &[WorkareaRecord],
) -> Vec<Uuid> {
    // A scope inside a user root also searches `universe`'s own claims
    // where the key reads that node, ranked with the scope's own; own held
    // rows never include them (ADR-0044 d10).
    let mut hit_nodes = readable.to_vec();
    if scope_ancestry.iter().any(|w| w.user_root)
        && let Some(universe) = scope_ancestry.last().map(|w| w.workarea_id)
        && caller.effective(&[universe]).contains(&Role::Reader)
        && !hit_nodes.contains(&universe)
    {
        hit_nodes.push(universe);
    }
    hit_nodes
}

/// A recall's `query_digest`: SHA-256 over the RFC 8785 JSON of an object
/// whose members are always present, defaults applied — `kinds` sorted by
/// code point with duplicates removed, `max_tokens` null when absent, and
/// `scope` the resolved node. `terms` and `expand_terms` digest as absent:
/// the vocabulary filter is unbuilt, and a request that sends either is
/// refused before it reaches a recall (retrieval.md, the read log; ADR-0025
/// d9, d16).
fn recall_query_digest(
    query: &str,
    req: &RecallRequest,
    scope: Uuid,
    pipeline: Pipeline,
    federation: Option<&crate::federation::FederationRecord>,
) -> Result<String, AppError> {
    let mut kinds: Vec<&str> = req.kinds.iter().map(|kind| kind.as_str()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    let doc = json!({
        "query": query,
        "k": req.k,
        "kinds": kinds,
        "include_bodies": req.include_text,
        "include_own_held": req.include_unreviewed.unwrap_or(false),
        "max_tokens": req.max_tokens,
        "terms": [],
        "expand_terms": "none",
        "scope": scope.to_string(),
        "search": pipeline.as_str(),
        "match": match req.match_mode {
            MatchMode::Any => "any",
            MatchMode::All => "all",
        },
    });
    // The two members of proposed ADR-0059 d3 and d5, only when sent, so a
    // recall that sends neither digests as it did before.
    let mut doc = doc;
    if let Some(members) = doc.as_object_mut() {
        if req.include_expired {
            members.insert("include_expired".to_owned(), Value::from(true));
        }
        if let Some(now) = &req.now {
            members.insert("now".to_owned(), Value::from(now.as_str()));
        }
        // `as_of` only when sent, so a recall of today digests as before
        // (ADR-0059 d18).
        if let Some(at) = &req.as_of {
            members.insert("as_of".to_owned(), Value::from(at.as_str()));
        }
        // `sources` only when it searched wider than the scope, `wide` or
        // the list's workarea ids, so a `scope` recall digests as it did
        // before (ADR-0072 d9).
        if let Some(record) = federation
            && record.sources != "scope"
        {
            members.insert("sources".to_owned(), record.sources.clone());
        }
    }
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
            search: SearchConfig::default(),
            reranker: None,
            telemetry: None,
            feedback_counts: std::sync::Mutex::default(),
            sparse: None,
            formats: Arc::new(crate::ports::ProseFormats),
            signup: None,
            wide_keys: std::sync::Mutex::default(),
        }
    }

    /// The app that serves `formats`, the compiled-in adapters of ADR-0056
    /// d3.
    #[must_use]
    pub fn with_formats(self, formats: Arc<dyn crate::ports::Formats>) -> Self {
        Self { formats, ..self }
    }

    /// The formats this app serves, for `/readyz` and doctor (ADR-0056 d14).
    #[must_use]
    pub fn formats(&self) -> &dyn crate::ports::Formats {
        self.formats.as_ref()
    }

    /// The app whose reranking pipelines call `reranker` (ADR-0053 d2). The
    /// composition refuses to serve a reranking pipeline without one
    /// (`SearchConfig::require_reranker`).
    #[must_use]
    pub fn with_reranker(self, reranker: Arc<dyn Reranker>) -> Self {
        Self {
            reranker: Some(reranker),
            ..self
        }
    }

    /// The app whose recalls hand their events, and whose gets, citations
    /// and supersessions hand their acts, to `telemetry` (ADR-0054 d2, d5).
    #[must_use]
    pub fn with_telemetry(self, telemetry: Arc<dyn Telemetry>) -> Self {
        Self {
            telemetry: Some(telemetry),
            ..self
        }
    }

    /// The app whose sparse pipelines read the query with `encoder`, and
    /// whose encoder task writes kind 2 with it (ADR-0052 d14). The
    /// composition refuses to serve a sparse pipeline without one
    /// (`SearchConfig::require_sparse`).
    #[must_use]
    pub fn with_sparse_encoder(self, encoder: Arc<dyn SparseEncoder>) -> Self {
        Self {
            sparse: Some(encoder),
            ..self
        }
    }

    /// The loaded sparse encoder's model name, for doctor and `/readyz`.
    #[must_use]
    pub fn sparse_model(&self) -> Option<&str> {
        self.sparse.as_deref().map(SparseEncoder::model)
    }

    /// The loaded reranker's model name, for doctor and `/readyz`.
    #[must_use]
    pub fn reranker_model(&self) -> Option<&str> {
        self.reranker.as_deref().map(Reranker::model)
    }

    /// The app whose workarea briefs are cut to `max_tokens`, the
    /// deployment's `brief.max_tokens` (ADR-0046 d8; #145).
    #[must_use]
    pub fn with_brief_max_tokens(self, max_tokens: u64) -> Self {
        Self {
            brief_max_tokens: max_tokens,
            ..self
        }
    }

    /// The app serving the search pipelines `search` names (configuration.md
    /// `search.*`; ADR-0049 d4). The composition checks the store serves
    /// their retrievers before it serves (`SearchConfig::require_supported`).
    #[must_use]
    pub fn with_search(self, search: SearchConfig) -> Self {
        Self { search, ..self }
    }

    /// The scanner's version, rule count and personal-data setting, for
    /// `/readyz` and doctor's scan line (ADR-0065 d11).
    #[must_use]
    pub fn scanner_info(&self) -> Option<crate::ports::ScannerInfo> {
        self.scanner.info()
    }

    /// The deployment's search settings.
    #[must_use]
    pub fn search_config(&self) -> &SearchConfig {
        &self.search
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
        // A person of the sign-up tenant's key under its request rate
        // (ADR-0068 d5).
        self.admit_request(&key).await?;
        self.caller_of(key).await
    }

    /// The caller a key value acts as, or the caller a connection's access
    /// token resolved to, its grants and denials read again here: one key,
    /// one caller, whichever credential presented it (ADR-0031 d11).
    async fn authenticate_bearer(&self, bearer: Bearer<'_>) -> Result<Caller, AppError> {
        match bearer {
            Bearer::Key(value) => self.authenticate(value).await,
            Bearer::Connection(caller) => {
                self.admit_request(&caller.key).await?;
                self.caller_of(caller.key.clone()).await
            }
            Bearer::Session(caller) => {
                let mut fresh = self.caller_of(caller.key.clone()).await?;
                fresh.own_keys.clone_from(&caller.own_keys);
                Ok(fresh)
            }
        }
    }

    /// The caller a credential acts as, as every verb resolves it: a key
    /// value, a connection's caller or a session key's, its grants and
    /// denials read now. The operations that serve a page's use case take
    /// it (ADR-0070 d5).
    pub async fn caller(&self, bearer: Bearer<'_>) -> Result<Caller, AppError> {
        self.authenticate_bearer(bearer).await
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
        resolve(
            ceiling_role(&caller.key, needed),
            &caller.effective(&ancestry),
            needed,
        )
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
    /// d3). Own is the authoring key for a bearer caller, and every key in the
    /// session caller's own-key set for the user page (ADR-0045 d16).
    /// An `as_of` after the lead's clock names an instant that has not
    /// happened: refused naming `field` (ADR-0059 d17).
    fn refuse_future(&self, at: &EventTime, field: &'static str) -> Result<(), AppError> {
        if at.as_str() > self.clock.now().as_str() {
            return Err(AppError::AsOfInvalid {
                field,
                reason: "as_of is after the lead's clock: the instant has not happened",
            });
        }
        Ok(())
    }

    async fn readable(&self, caller: &Caller, memory: &MemoryRecord) -> Result<bool, AppError> {
        let ancestry = self
            .ancestry(caller.key.tenant_id, memory.workarea_id)
            .await?;
        if caller.owns(memory.author_key_id) {
            return Ok(!caller.denied(&ancestry));
        }
        let reader = caller.effective(&ancestry).contains(&Role::Reader);
        if memory.status == MemoryStatus::Held && reader {
            // Inside an in-progress subtree a reader reads another author's
            // held memory, never a steering, guardrail or retirement version
            // (ADR-0030 d3).
            return Ok(served_unreviewed(memory, self.scanner.version())
                && self
                    .in_progress_root(caller.key.tenant_id, memory.workarea_id)
                    .await?
                    .is_some());
        }
        Ok(memory.status == MemoryStatus::Promoted && reader)
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
    /// Not set-wise yet: each ancestor and the replaced version are read for
    /// this one row, so recall, get and every listing issue those reads once
    /// per served row, against ADR-0024 d4 ("never N+1"); recall reads the
    /// path once per workarea ([`Self::served_in`]). Only the rows
    /// themselves hydrate set-wise (`JournalStore::memories`). Measured at
    /// `k` 100 in ADR-0049 d12; elsewhere unmeasured (#99, #150).
    ///
    /// A version in `universe` is served to every key outside its authoring
    /// key set without its source: no principal, no `derives_from`, its
    /// author and lineage fields blanked, and `replaces` only where it names
    /// a version in `universe` (ADR-0044 d9). The authoring key set is the
    /// calling key alone until rotation is built (roles-and-keys.md#own).
    async fn served(
        &self,
        caller: &Caller,
        memory: MemoryRecord,
    ) -> Result<ServedMemory, AppError> {
        self.served_in(caller, memory, &mut BTreeMap::new()).await
    }

    /// [`Self::served`] with the workarea ancestries already read, by node:
    /// a use case serving many rows reads each workarea's ancestry once
    /// (ADR-0024 d4; ADR-0049 d12).
    async fn served_in(
        &self,
        caller: &Caller,
        mut memory: MemoryRecord,
        ancestries: &mut BTreeMap<Uuid, Option<Vec<WorkareaRecord>>>,
    ) -> Result<ServedMemory, AppError> {
        let tenant_id = caller.key.tenant_id;
        let ancestry = match ancestries.get(&memory.workarea_id) {
            Some(read) => read.clone(),
            None => {
                let read = self
                    .journal
                    .workarea_ancestry(tenant_id, memory.workarea_id)
                    .await?;
                ancestries.insert(memory.workarea_id, read.clone());
                read
            }
        };
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
        let mut replaces_valid_until = None;
        if let Some(id) = memory.replaces
            && let Some(m) = self.journal.memory(tenant_id, id).await?
            && (!anonymous || m.workarea_id == memory.workarea_id)
            && self.readable(caller, &m).await?
        {
            replaces = Some(VersionRef {
                artifact_id: id,
                version: m.version,
            });
            // What approving a held successor does to the version it
            // replaces, shown before the verdict (proposed ADR-0059 d4).
            if memory.status == MemoryStatus::Held {
                replaces_valid_until = ratified_end(&m.valid, &memory.valid);
            }
        }
        if anonymous {
            memory.author_principal = String::new();
            memory.author_key_id = Uuid::nil();
            memory.lineage_principals.clear();
            memory.lineage_key_ids.clear();
        }
        let load_warning = (memory.kind == MemoryKind::Guardrail
            && memory.status == MemoryStatus::Held)
            .then_some(LOAD_WARNING);
        Ok(ServedMemory {
            memory,
            replaces,
            replaces_valid_until,
            workarea_path,
            anonymous,
            load_warning,
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

    /// An edge whose endpoints have different in-progress roots, or one and
    /// none, crosses the boundary (ADR-0030 d7): `workarea.in_progress_refused`,
    /// reason `edge_crosses`, naming the first endpoint's workarea.
    async fn refuse_crossing_edge(
        &self,
        tenant_id: Uuid,
        src: Uuid,
        dst: Uuid,
    ) -> Result<(), AppError> {
        if self.in_progress_root(tenant_id, src).await?
            != self.in_progress_root(tenant_id, dst).await?
        {
            return Err(AppError::InProgressRefused {
                workarea_id: src,
                reason: InProgressReason::EdgeCrosses,
            });
        }
        Ok(())
    }

    /// A status transition on a node inside an in-progress subtree waits for
    /// its commit (ADR-0030 d6): `workarea.in_progress_refused`, reason
    /// `commit_first`.
    async fn refuse_inside(&self, tenant_id: Uuid, node: Uuid) -> Result<(), AppError> {
        match self.in_progress_root(tenant_id, node).await? {
            Some(_) => Err(AppError::InProgressRefused {
                workarea_id: node,
                reason: InProgressReason::CommitFirst,
            }),
            None => Ok(()),
        }
    }

    /// The node's in-progress root, or `None` (ADR-0030 d1).
    async fn in_progress_root(
        &self,
        tenant_id: Uuid,
        node: Uuid,
    ) -> Result<Option<Uuid>, AppError> {
        let marks: Vec<(Uuid, bool, bool)> = self
            .journal
            .workarea_ancestry(tenant_id, node)
            .await?
            .unwrap_or_default()
            .iter()
            .map(|w| {
                (
                    w.workarea_id,
                    w.in_progress,
                    w.status == WorkareaStatus::Development,
                )
            })
            .collect();
        Ok(in_progress::in_progress_root_of(&marks))
    }

    /// Each node of subtree(`top`) with its in-progress root, parent before
    /// child (ADR-0030 d1).
    async fn subtree_roots(
        &self,
        tenant_id: Uuid,
        top: Uuid,
    ) -> Result<BTreeMap<Uuid, Option<Uuid>>, AppError> {
        let mut roots = BTreeMap::new();
        let top_root = self.in_progress_root(tenant_id, top).await?;
        for node in self.journal.workarea_subtree(tenant_id, top).await? {
            // A node that is not `development` is never inside, nor is
            // anything beneath it (ADR-0030 d3).
            let root = if node.status != WorkareaStatus::Development {
                None
            } else if node.in_progress {
                Some(node.workarea_id)
            } else if node.workarea_id == top {
                top_root
            } else {
                node.parent_id
                    .and_then(|p| roots.get(&p).copied().flatten())
            };
            roots.insert(node.workarea_id, root);
        }
        Ok(roots)
    }

    /// The served node set of a read scoped at `scope` (ADR-0030 d2): the
    /// nodes of subtree(scope) where the caller reads, less every node whose
    /// in-progress root differs from the scope's; with the scope's root.
    async fn served_nodes(&self, caller: &Caller, scope: Uuid) -> Result<ServedNodes, AppError> {
        let tenant_id = caller.key.tenant_id;
        let nodes = self.nodes_with(caller, scope, Role::Reader).await?;
        let roots = self.subtree_roots(tenant_id, scope).await?;
        let scope_root = roots.get(&scope).copied().flatten();
        let mut served = ServedNodes {
            nodes: Vec::with_capacity(nodes.len()),
            inside: Vec::new(),
            scope_root,
        };
        for node in nodes {
            let root = roots.get(&node).copied().flatten();
            if crosses_into(scope_root, root) {
                continue;
            }
            // Inside the scope's root, every node of which is development;
            // a committed or validated node, and anything under one, never
            // is (d3).
            if root.is_some() {
                served.inside.push(node);
            }
            served.nodes.push(node);
        }
        Ok(served)
    }

    /// A curator act on a row or target on `node` (ADR-0030 d9): inside an
    /// in-progress subtree only, and never on what the curator's key or
    /// principal authored or derived from (R2 per key and principal).
    async fn curator_may_act(
        &self,
        key: &KeyRecord,
        verb: &'static str,
        node: Uuid,
        subject: Option<&MemoryRecord>,
    ) -> Result<(), AppError> {
        if key.profile != Some(KeyProfile::Curator) {
            return Ok(());
        }
        curator_scope(self.in_progress_root(key.tenant_id, node).await?)
            .map_err(|refusal| AppError::Curator { verb, refusal })?;
        if let Some(m) = subject
            && (m.author_key_id == key.key_id
                || m.author_principal == key.principal
                || m.lineage_key_ids.contains(&key.key_id)
                || m.lineage_principals.contains(&key.principal))
        {
            return Err(AppError::AuthorInLineage);
        }
        Ok(())
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
            profile: req.profile,
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
            guardrail_scope: req
                .guardrail_scope
                .unwrap_or(GuardrailScope::ISSUANCE_DEFAULT),
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
            "guardrail_scope": record.guardrail_scope.as_str(),
            // The user root's id, null for a tenant key (ADR-0044 d2).
            "user": record.user.map(|u| u.to_string()),
        });
        // The person a session key serves, absent for every other key
        // (ADR-0045 d6).
        if let (Some(user), Some(obj)) = (&record.session_user, payload.as_object_mut()) {
            obj.insert("session_user".to_owned(), json!(user.as_str()));
        }
        // The profile, absent for a key with none (ADR-0030 d8).
        if let (Some(profile), Some(obj)) = (record.profile, payload.as_object_mut()) {
            obj.insert("profile".to_owned(), json!(profile.as_str()));
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
    /// `ttl_secs` from now: [`CONNECT_CODE_TTL_SECS`] unless an admin asked
    /// for longer (ADR-0031 d8, ADR-0067 d3).
    fn new_connect_code(
        &self,
        oauth: &Issuer,
        key_id: Uuid,
        ttl_secs: i64,
    ) -> (ConnectCode, ConnectCodeIssued) {
        let secret = self.ids.new_secret();
        let mut random = [0_u8; 8];
        random.copy_from_slice(&secret[24..]);
        let symbols = CodeSymbols::from_random(random);
        let expires_at = EventTime::from_unix_micros(
            self.clock.now().unix_micros() + ttl_secs * 1_000_000,
        )
        .expect("at most thirty days after a clock reading is a time the fixed-width form writes");
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
            in_progress: false,
            workarea_id: user.id,
            tenant_id: issuer.tenant_id,
            parent_id: Some(user.universe),
            name: user.name.clone(),
            status: WorkareaStatus::Development,
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
            "status": workarea.status.as_str(),
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
            in_progress: false,
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
            profile: None,
            role: Role::Admin,
            name: req.admin_key_name.clone(),
            principal: req.admin_principal.clone(),
            held_by: Some(HeldBy::Human),
            default_workarea: Some(root_workarea_id.into()),
            crew: None,
            grants: Vec::new(),
            user: None,
            connect_code: false,
            guardrail_scope: None,
        };
        let (record, value) =
            self.mint_key(tenant_id, &issue, Some(root_workarea_id), None, None, None);
        let key_id = record.key_id;
        let guardrail_scope = record.guardrail_scope;
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
                guardrail_scope,
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
        // A curator is a dreamer key held by an agent, whose writer grants
        // the dreamer's admissible set is read as if absent (ADR-0030 d8).
        if let Some(profile) = req.profile
            && !profile_admits(profile, req.role, req.held_by.unwrap_or(HeldBy::Agent))
        {
            return Err(AppError::ProfileMismatch {
                profile: profile.as_str(),
            });
        }
        let checked: Vec<Grant> = if req.profile == Some(KeyProfile::Curator) {
            grants
                .iter()
                .filter(|g| g.role != Role::Writer)
                .cloned()
                .collect()
        } else {
            grants.clone()
        };
        check_issuance(ceiling, req.role, &checked).map_err(issuance_refusal)?;
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
        let guardrail_scope = record.guardrail_scope;
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
            let (code, shown) = self.new_connect_code(oauth, key_id, CONNECT_CODE_TTL_SECS);
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
                guardrail_scope,
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
            guardrail_scope,
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
        // `chat` is reserved in every user root's namespace for a key a
        // person holds, at any depth, so an agent never takes the name
        // before the person opens their chat workarea; refused before
        // anything is journalled (ADR-0063 d8).
        if let Some(parent) = ancestors.first()
            && parent.user().is_some()
            && name.as_str() == CHAT_WORKAREA
            && key.held_by != HeldBy::Human
        {
            return Err(AppError::ChatScopeHumanOnly {
                held_by: key.held_by,
            });
        }
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
            in_progress: false,
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
    /// mcp-plugin.md `oml_workarea`; ADR-0026 d4).
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
        let stale_versions = if structure {
            0
        } else {
            self.journal.stale_count(tenant_id, node).await?
        };
        // The node's in-progress root, read off the ancestry (ADR-0030 d13).
        let in_progress_root = ancestry.iter().find(|w| w.in_progress).cloned();
        // The node's federation field, served beside the policy (ADR-0072
        // d5).
        let federation = crate::federation::WorkareaFederation {
            accept_federated: self
                .journal
                .federation_declining(tenant_id, &[node])
                .await?
                .is_empty(),
        };
        Ok(WorkareaInfo {
            in_progress_root,
            stale_versions,
            path: derived_path(&ancestry),
            policy: review_policy(),
            federation,
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
        let guardrail_scope =
            GuardrailScope::of_issuance(issued.envelope.payload.get("guardrail_scope"))
                .ok_or_else(|| {
                    StoreError::Backend(format!(
                        "key {key_id}'s key.issued journals guardrail_scope {}",
                        issued.envelope.payload["guardrail_scope"]
                    ))
                })?;
        Ok(KeyDetail {
            key: record,
            user,
            held_by,
            guardrail_scope,
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
        self.whoami_of_key(key, access).await
    }

    /// [`Self::whoami`] for whichever credential presented the key: a key
    /// value, a connection's caller, or a session key's caller from a
    /// capacity credential (ADR-0070 d5).
    pub async fn whoami_of(&self, bearer: Bearer<'_>) -> Result<Whoami, AppError> {
        let key = match bearer {
            Bearer::Key(value) => return self.whoami(value).await,
            Bearer::Connection(caller) | Bearer::Session(caller) => {
                self.authenticate_bearer(bearer).await?;
                caller.key.clone()
            }
        };
        let access = self.access_of(&key).await?;
        self.whoami_of_key(key, access).await
    }

    async fn whoami_of_key(
        &self,
        key: KeyRecord,
        access: Option<KeyAccess>,
    ) -> Result<Whoami, AppError> {
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

    /// Set a workarea's mode (ADR-0030 d1, d6): a dreamer key a person holds,
    /// with dreamer effective access on the node, and no other key, refused
    /// before any lookup. `in_progress` is refused on `universe`, a node not
    /// `development`, or one with a marked ancestor or descendant;
    /// `standard` on an in-progress root is its commit, refused while any
    /// version in its subtree is held. The chain head is read before the
    /// held test, so a capture landing between the test and the commit moves
    /// the head and the commit conflicts. Setting the mode a node already
    /// holds journals nothing. Journalled as `workarea.mode_set`.
    pub async fn set_workarea_mode(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workarea: Reference,
        mode: WorkareaMode,
    ) -> Result<WorkareaRecord, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        if !may_set_mode(key.role, key.held_by, key.profile) {
            return Err(AppError::ModeHumanOnly {
                role: key.role,
                held_by: key.held_by,
            });
        }
        let tenant_id = key.tenant_id;
        let workarea_id = self.resolve(key, NamedKind::Workarea, &workarea).await?;
        self.observer.workarea(workarea_id);
        let prev = self.head_event(tenant_id).await?;
        let (node, current, held_free) =
            match self.mode_admitted(&caller, workarea_id, mode).await? {
                ModeAdmitted::Already(node) => return Ok(node),
                ModeAdmitted::Change {
                    node,
                    current,
                    held_free,
                } => (node, current, held_free),
            };
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WORKAREA_MODE_SET,
            &key.principal,
            Some(key.key_id),
            json!({
                "workarea_id": workarea_id.to_string(),
                "mode": mode.as_str(),
                "from": current.as_str(),
            }),
        )?;
        // The commit's zero-held test runs again inside the committing
        // transaction, so no held row created without a chain event slips
        // past it (ADR-0030 d6).
        self.commit(
            event,
            MetadataFold::WorkareaModeSet {
                workarea_id,
                in_progress: mode == WorkareaMode::InProgress,
                held_free,
            },
            None,
        )
        .await
        .map_err(|e| match e {
            AppError::Store(StoreError::HeldRemaining) => AppError::HeldRemaining { workarea_id },
            other => other,
        })?;
        self.observer.mode_set(mode.as_str());
        Ok(WorkareaRecord {
            in_progress: mode == WorkareaMode::InProgress,
            ..node
        })
    }

    /// Whether `set_workarea_mode` admits `mode` on the node for the
    /// caller now, every check but the commit's, in its order: the key's
    /// shape, dreamer access, then the mark's or the commit's test. The
    /// verb and the pages' offer call this one function (ADR-0045 d28).
    pub(crate) async fn mode_admitted(
        &self,
        caller: &Caller,
        workarea_id: Uuid,
        mode: WorkareaMode,
    ) -> Result<ModeAdmitted, AppError> {
        const VERB: &str = "workarea_mode";
        let key = &caller.key;
        if !may_set_mode(key.role, key.held_by, key.profile) {
            return Err(AppError::ModeHumanOnly {
                role: key.role,
                held_by: key.held_by,
            });
        }
        let tenant_id = key.tenant_id;
        self.require_access(caller, workarea_id, Role::Dreamer, VERB)
            .await?;
        let ancestry = self
            .journal
            .workarea_ancestry(tenant_id, workarea_id)
            .await?
            .unwrap_or_default();
        let node = ancestry.first().cloned().ok_or(AppError::NotFound)?;
        let current = if node.in_progress {
            WorkareaMode::InProgress
        } else {
            WorkareaMode::Standard
        };
        if current == mode {
            return Ok(ModeAdmitted::Already(node));
        }
        let subtree = self
            .journal
            .workarea_subtree(tenant_id, workarea_id)
            .await?;
        let mut held_free = Vec::new();
        match mode {
            WorkareaMode::InProgress => {
                let marks: Vec<(Uuid, bool)> = ancestry
                    .iter()
                    .map(|w| (w.workarea_id, w.in_progress))
                    .collect();
                let below = subtree
                    .iter()
                    .any(|w| w.workarea_id != workarea_id && w.in_progress);
                let development = subtree
                    .iter()
                    .all(|w| w.status == WorkareaStatus::Development);
                in_progress::check_mark(&marks, below, development).map_err(|reason| {
                    AppError::InProgressRefused {
                        workarea_id,
                        reason,
                    }
                })?;
            }
            WorkareaMode::Standard => {
                // The commit: nothing in the subtree may still be held,
                // dead-lettered versions included (ADR-0030 d6).
                let nodes: Vec<Uuid> = subtree.iter().map(|w| w.workarea_id).collect();
                if self.journal.held_count(tenant_id, &nodes).await? > 0 {
                    return Err(AppError::HeldRemaining { workarea_id });
                }
                held_free = nodes;
            }
        }
        Ok(ModeAdmitted::Change {
            node,
            current,
            held_free,
        })
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

    /// The person a key acts for, by name: a session key's
    /// `session_user`, else the name of a user key's root; `None` for a
    /// tenant key (ADR-0044 d2, ADR-0045 d6).
    async fn person_of(&self, key: &KeyRecord) -> Result<Option<Name>, AppError> {
        if let Some(user) = &key.session_user {
            return Ok(Some(user.clone()));
        }
        match key.user {
            Some(root) => Ok(self
                .journal
                .workarea(key.tenant_id, root)
                .await?
                .map(|w| w.name)),
            None => Ok(None),
        }
    }

    /// Whether the proposing or authoring key and the ratifying key act for
    /// one person: the one `same_user` rule of a claim's approval, a
    /// transition's ratification and an edge's (ADR-0046 d2, ADR-0055 d7).
    /// A person reviews through `<user>-review`, a session key that names
    /// its person by `session_user` rather than by a user root, so both are
    /// read as the person's name; a key the tenant does not hold is no one's.
    async fn same_person(
        &self,
        tenant_id: Uuid,
        author_key_id: Uuid,
        reviewer: &KeyRecord,
    ) -> Result<bool, AppError> {
        let Some(author) = self.journal.key(tenant_id, author_key_id).await? else {
            return Ok(false);
        };
        let (a, r) = (
            self.person_of(&author).await?,
            self.person_of(reviewer).await?,
        );
        Ok(same_user(a.as_ref(), r.as_ref()))
    }

    /// A user key's edge with an endpoint in `universe` is refused
    /// `access.grant_refused` naming that endpoint's workarea, before
    /// anything is journalled: users neither write nor propose to
    /// `universe` (ADR-0046 d5; ADR-0044 Context, decision 3). A tenant key
    /// passes.
    async fn refuse_user_edge_into_universe(
        &self,
        key: &KeyRecord,
        workareas: &[Uuid],
    ) -> Result<(), AppError> {
        if key.user.is_none() {
            return Ok(());
        }
        for workarea_id in workareas {
            let node = self.journal.workarea(key.tenant_id, *workarea_id).await?;
            if node.is_some_and(|n| n.parent_id.is_none()) {
                return Err(AppError::GrantRefused {
                    workarea_id: *workarea_id,
                    needed_role: Role::Dreamer,
                });
            }
        }
        Ok(())
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
            // A claim inside an in-progress subtree the brief's node is not
            // inside is left out, whenever its edge was made (ADR-0030 d2).
            if memory.status == MemoryStatus::Promoted
                && memory.is_tip
                && self.readable(caller, &memory).await?
                && !crosses_into(
                    self.in_progress_root(caller.key.tenant_id, workarea)
                        .await?,
                    self.in_progress_root(caller.key.tenant_id, memory.workarea_id)
                        .await?,
                )
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
        bearer: impl Into<Bearer<'_>>,
        workarea: Reference,
    ) -> Result<WorkareaStatusView, AppError> {
        const VERB: &str = "workarea_status";
        let caller = self.authenticate_bearer(bearer.into()).await?;
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
        bearer: impl Into<Bearer<'_>>,
        workarea: Reference,
        to: WorkareaStatus,
    ) -> Result<TransitionRecord, AppError> {
        const VERB: &str = "propose_transition";
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Dreamer, VERB)?;
        }
        let workarea_id = self.resolve(key, NamedKind::Workarea, &workarea).await?;
        let node = self.propose_admitted(&caller, workarea_id, to).await?;
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

    /// Whether `propose_transition` admits `to` on the node for the caller
    /// now, every check after the name resolves and before the commit, in
    /// its order. The verb and the pages' offer call this one function
    /// (ADR-0045 d28).
    pub(crate) async fn propose_admitted(
        &self,
        caller: &Caller,
        workarea_id: Uuid,
        to: WorkareaStatus,
    ) -> Result<WorkareaRecord, AppError> {
        const VERB: &str = "propose_transition";
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Dreamer, VERB)?;
            self.require_access(caller, workarea_id, Role::Dreamer, VERB)
                .await?;
        }
        let node = self
            .journal
            .workarea(key.tenant_id, workarea_id)
            .await?
            .ok_or(AppError::NotFound)?;
        self.refuse_inside(key.tenant_id, workarea_id).await?;
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
        Ok(node)
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
        bearer: impl Into<Bearer<'_>>,
        workarea: Reference,
        transition_id: Uuid,
    ) -> Result<WorkareaRecord, AppError> {
        const VERB: &str = "ratify_transition";
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Dreamer, VERB)?;
        }
        refuse_service(key, VERB)?;
        let workarea_id = self.resolve(key, NamedKind::Workarea, &workarea).await?;
        let prev = self.head_event(key.tenant_id).await?;
        let (proposal, node) = self
            .ratify_admitted(&caller, workarea_id, transition_id)
            .await?;
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
                "same_user": self.same_person(key.tenant_id, proposal.proposer_key_id, key).await?,
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

    /// Whether `ratify_transition` admits the proposal on the node for the
    /// caller now, every check after the name resolves and before the
    /// commit, in its order; R2 among them, so a proposing key or
    /// principal is never admitted. The verb and the pages' offer call this
    /// one function (ADR-0045 d28).
    pub(crate) async fn ratify_admitted(
        &self,
        caller: &Caller,
        workarea_id: Uuid,
        transition_id: Uuid,
    ) -> Result<(TransitionRecord, WorkareaRecord), AppError> {
        const VERB: &str = "ratify_transition";
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Dreamer, VERB)?;
        }
        refuse_service(key, VERB)?;
        if key.role != Role::Admin {
            self.require_access(caller, workarea_id, Role::Dreamer, VERB)
                .await?;
        }
        self.refuse_inside(key.tenant_id, workarea_id).await?;
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
        Ok((proposal, node))
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
        // `as_of` reads the scoped listing at an instant that has happened
        // (ADR-0059 d17, d19).
        if let Some(at) = &req.as_of {
            if req.own || req.author.is_some() || req.tip.is_some() {
                return Err(AppError::AsOfInvalid {
                    field: "as_of",
                    reason: "as_of reads the scoped listing: it does not combine with own, with_own, author or tip",
                });
            }
            self.refuse_future(at, "as_of")?;
        }
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
            // The served node set, and inside an in-progress scope every
            // author's held rows on it (ADR-0030 d2, d3).
            let served = self.served_nodes(&caller, scope).await?;
            (
                ListingScope::Tips {
                    nodes: served.nodes,
                    held: served.inside,
                    current_scanner: self.scanner.version().map(str::to_owned),
                },
                None,
            )
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
            anchor: req.anchor.clone(),
            author: req.author,
            tip: req.tip,
            as_of: req.as_of.clone(),
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
        // The re-check at an instant: which rows were their claim's tip then
        // (ADR-0059 d15, d16).
        let tips_then: BTreeSet<Uuid> = match &filter.as_of {
            None => BTreeSet::new(),
            Some(at) => {
                let ids: Vec<Uuid> = rows.iter().map(|m| m.memory_id).collect();
                self.journal
                    .tips_as_of(tenant_id, &ids, at)
                    .await?
                    .into_iter()
                    .collect()
            }
        };
        let mut items = Vec::with_capacity(rows.len());
        for m in rows {
            // R3 defense in depth: nothing outside the scope's predicate
            // leaves the application layer, whatever the store returned.
            let admitted = match &filter.scope {
                ListingScope::Own { key_ids, masked } => {
                    key_ids.contains(&m.author_key_id) && !masked.contains(&m.workarea_id)
                }
                ListingScope::Tips { nodes, held, .. } => {
                    nodes.contains(&m.workarea_id)
                        && (match &filter.as_of {
                            None => m.status == MemoryStatus::Promoted && m.is_tip,
                            Some(_) => {
                                m.status == MemoryStatus::Promoted
                                    && tips_then.contains(&m.memory_id)
                            }
                        } || (held.contains(&m.workarea_id)
                            && filter
                                .as_of
                                .as_ref()
                                .is_none_or(|at| m.created_at.as_str() <= at.as_str())
                            && served_unreviewed(&m, self.scanner.version())))
                }
            };
            if admitted {
                items.push(self.served(&caller, m).await?);
            }
        }
        Ok(MemoryPage { items, next_cursor })
    }

    /// The rows of a stored `similar` list the caller can still read: a
    /// replay answers what was first answered, less any row since demoted,
    /// erased or made unreadable by a revoked grant (ADR-0050 d1, R3).
    async fn still_readable(
        &self,
        caller: &Caller,
        similar: Vec<SimilarItem>,
    ) -> Result<Vec<SimilarItem>, AppError> {
        if similar.is_empty() {
            return Ok(similar);
        }
        let ids: Vec<Uuid> = similar.iter().map(|s| s.artifact_id).collect();
        let rows = self.journal.memories(caller.key.tenant_id, &ids).await?;
        let mut kept = Vec::with_capacity(similar.len());
        for item in similar {
            if let Some(m) = rows.iter().find(|m| m.memory_id == item.artifact_id)
                && self.readable(caller, m).await?
            {
                kept.push(item);
            }
        }
        Ok(kept)
    }

    async fn replayed_supersede(
        &self,
        caller: &Caller,
        mut receipt: SupersedeReceipt,
    ) -> Result<SupersedeReceipt, AppError> {
        receipt.similar = self
            .still_readable(caller, std::mem::take(&mut receipt.similar))
            .await?;
        Ok(receipt)
    }

    /// The similar check (ADR-0050 d1, d17): the rows `caller` may read in
    /// `workarea`'s recall scope — the served tips recall would read there,
    /// `universe`'s included inside a user root, and its own held rows —
    /// that hold `body`'s words in the same order (`same_text`) or a word
    /// set at Jaccard 0.9 or more (`near_copy`), at most three, `same_text`
    /// first, then by similarity, then by id. A version of `exclude`, the
    /// claim a supersession replaces, is never named. Another key's held row
    /// never reaches the candidates, and every candidate is read and
    /// admitted by recall's rule before it is compared, so a row the caller
    /// cannot read is never named (R3).
    async fn similar_to(
        &self,
        caller: &Caller,
        workarea: Uuid,
        body: &str,
        exclude: Option<&str>,
    ) -> Result<Vec<SimilarItem>, AppError> {
        let key = &caller.key;
        // Both sides are the text search reads, so two sources that say one
        // thing are one claim (ADR-0056 d4; ADR-0050 d1).
        let written = Fingerprint::of(body);
        let keys: Vec<i64> = written.query_keys().into_iter().map(|(_, k)| k).collect();
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let scope_ancestry = self
            .journal
            .workarea_ancestry(key.tenant_id, workarea)
            .await?
            .unwrap_or_default();
        let readable = self.nodes_with(caller, workarea, Role::Reader).await?;
        let hit_nodes = with_universe(caller, &readable, &scope_ancestry);
        let scope = CandidateScope {
            reference: None,
            promoted_nodes: hit_nodes.clone(),
            kinds: SIMILAR_KINDS.to_vec(),
            own_held: Some(OwnHeldScope {
                author_key_ids: vec![key.key_id],
                nodes: readable.clone(),
                any_author: false,
                current_scanner: self.scanner.version().map(str::to_owned),
            }),
            // A copy is a copy even once its valid time has ended
            // (ADR-0050 d1; proposed ADR-0059 d3).
            expired_at: None,
            // The check reads every origin: a copy is a copy wherever it
            // came from (ADR-0050 d1).
            anchor: Default::default(),
            as_of: None,
        };
        let ids = self
            .journal
            .similar_candidates(key.tenant_id, &scope, &keys, SIMILAR_MAX_CANDIDATES)
            .await?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut named: Vec<(Relation, f64, MemoryRecord)> = self
            .journal
            .memories(key.tenant_id, &ids)
            .await?
            .into_iter()
            .filter(|m| {
                let admitted = match m.status {
                    MemoryStatus::Promoted => m.is_tip && hit_nodes.contains(&m.workarea_id),
                    MemoryStatus::Held => {
                        m.author_key_id == key.key_id && readable.contains(&m.workarea_id)
                    }
                    _ => false,
                };
                admitted
                    && !m.retirement
                    && SIMILAR_KINDS.contains(&m.kind)
                    && !(exclude == Some(m.claim_key.as_str()) && m.workarea_id == workarea)
            })
            .filter_map(|m| {
                relate(&written, &Fingerprint::of(m.searched())).map(|(r, j)| (r, j, m))
            })
            .collect();
        named.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.total_cmp(&a.1))
                .then(a.2.memory_id.cmp(&b.2.memory_id))
        });
        named.truncate(SIMILAR_MAX_LISTED);
        Ok(named
            .into_iter()
            .map(|(relation, jaccard, m)| SimilarItem {
                artifact_id: m.memory_id,
                claim_key: m.claim_key,
                version: m.version,
                kind: m.kind.as_str().to_owned(),
                workarea_id: m.workarea_id,
                state: m.status.as_str().to_owned(),
                relation: relation.as_str().to_owned(),
                // Hundredths, rounded down, so 0.899 never reads as 0.90.
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a ratio in [0, 1] times 100"
                )]
                jaccard_percent: (jaccard * 100.0).floor() as u8,
                own: m.author_key_id == key.key_id,
            })
            .collect())
    }

    /// Runs the similar check for a write of `kind` and applies
    /// `on_similar`: the named rows, or [`AppError::SimilarExists`] when
    /// asked to refuse and a row is named. Kinds the check does not cover
    /// are not checked (ADR-0050 d1).
    async fn similar_gate(
        &self,
        caller: &Caller,
        workarea: Uuid,
        kind: MemoryKind,
        body: &str,
        exclude: Option<&str>,
        on_similar: OnSimilar,
    ) -> Result<Vec<SimilarItem>, AppError> {
        if !SIMILAR_KINDS.contains(&kind) {
            return Ok(Vec::new());
        }
        let started = std::time::Instant::now();
        let similar = self.similar_to(caller, workarea, body, exclude).await?;
        let relation = similar.first().map_or("none", |s| {
            if s.relation == Relation::SameText.as_str() {
                Relation::SameText.as_str()
            } else {
                Relation::NearCopy.as_str()
            }
        });
        self.observer.similar_checked(
            relation,
            on_similar.as_str(),
            started.elapsed().as_secs_f64(),
        );
        if on_similar == OnSimilar::Refuse && !similar.is_empty() {
            return Err(AppError::SimilarExists(similar));
        }
        Ok(similar)
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
            let mut receipt: SubmitReceipt = serde_json::from_value(record.response)
                .map_err(|e| AppError::StoredResponse(e.to_string()))?;
            receipt.similar = self
                .still_readable(&caller, std::mem::take(&mut receipt.similar))
                .await?;
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
        let (format, parsed) = self.capture_format(
            req.kind,
            req.format.as_deref(),
            req.title.as_deref(),
            &body,
            false,
        )?;
        // A guardrail's fields, then its two scan classes, before the
        // scanner port: a veto journals nothing (proposed ADR-0037 d10).
        let guardrail = guardrail_fields(req.kind, false, req.severity, req.rule.as_ref(), &body)?;
        let valid = valid_time(
            req.valid_from.as_deref(),
            req.valid_until.as_deref(),
            req.kind,
            false,
        )?;
        self.scan_anchor(key.tenant_id, req.anchor.as_ref())?;
        // The claim key reaches the journal as plaintext, so a veto in it
        // refuses as one in the body does (ADR-0065 d7).
        if let Some(claim_key) = req.claim_key.as_deref() {
            self.scan_veto_only(key.tenant_id, "claim_key", claim_key)?;
        }
        let origin = recorded_origin(req.origin, req.anchor.as_ref())?;

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
        let mut scan_finding = scan_finding;
        self.scan_derived(key.tenant_id, req.kind, &format, &parsed, &mut scan_finding)?;

        // The lineage closure: the author plus every declared ancestor's
        // closure — R2 runs over the whole of it (roles-and-keys.md). An
        // ancestor the key cannot read is indistinguishable from none. A
        // target inside an in-progress subtree the write is not inside is
        // unreadable at the write's node (ADR-0030 d7). A curator writes
        // inside an in-progress subtree only (d9).
        self.curator_may_act(key, "submit", workarea_id, None)
            .await?;
        let write_root = self.in_progress_root(key.tenant_id, workarea_id).await?;
        let mut principals = BTreeSet::from([key.principal.clone()]);
        let mut key_ids = BTreeSet::from([key.key_id]);
        for ancestor in &req.derives_from {
            let m = self
                .journal
                .memory(key.tenant_id, *ancestor)
                .await?
                .ok_or(AppError::LineageUnknown(*ancestor))?;
            if !self.readable(&caller, &m).await?
                || crosses_into(
                    write_root,
                    self.in_progress_root(key.tenant_id, m.workarea_id).await?,
                )
            {
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

        // A person of the sign-up tenant writes at most their memories
        // quota; nothing is journalled past it. The check follows the
        // idempotency replay and the live claim_key refusal, so a retry
        // gets its stored receipt and a write under a live claim is the
        // conflict it was; a new version goes through `supersede`, which
        // adds no memory (ADR-0068 d5).
        self.require_quota(key, oml_domain::signup::Quota::Memories)
            .await?;

        // The similar check, after every other refusal and before the
        // commit: a refusal here journals nothing and claims no idempotency
        // record, so the same key may be resent with `hold` (ADR-0050 d1).
        let similar = self
            .similar_gate(
                &caller,
                workarea_id,
                req.kind,
                format.search_text.as_deref().unwrap_or(&body),
                None,
                req.on_similar,
            )
            .await?;

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
            scanned_under: self.scanner.version().map(str::to_owned),
            created_at: self.clock.now(),
            guardrail,
            valid: Box::new(valid),
            anchor: req.anchor.clone().map(Box::new),
            origin_kind: Some(origin),
            source_check: None,
            format: Box::new(format),
        };

        // The catalog payload of submission.remember: digest-only, body
        // plaintext never rides the journal (ADR-0002 d1; event-catalog.md).
        // The PoC stores the body inline and unencrypted, so there is no
        // ciphertext digest; the scanner names the rule set its verdict was
        // taken under, or none (admission.md rule-set registry; ADR-0065 d1).
        let mut payload = json!({
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
            "anchor": anchor_payload(req.anchor.as_ref()),
            "origin": origin.to_json(),
            "provenance": "agent",
            "simulation_id": Value::Null,
            "replaces": Value::Null,
            "retirement": false,
            "scan": {
                "result": if scan_finding.is_some() { "hold" } else { "pass" },
                "scanner_version": self.scanner.version(),
                "finding": scan_finding,
            },
        });
        guardrail_payload(&mut payload, memory.guardrail.as_deref())?;
        with_valid_time(&mut payload, &memory.valid);
        with_format_fields(&mut payload, &memory.format);
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
            similar,
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
        // After the commit, off the write's path: the remember side's
        // duplicate count, and a citation's label on the key's recall
        // (ADR-0054 d5, d12).
        if let Some(telemetry) = &self.telemetry {
            telemetry.offer(TelemetryItem::Remembered(Remembered {
                tenant_id: key.tenant_id,
                workarea_id,
                memory_id,
                body_digest: receipt.body_digest.clone(),
            }));
        }
        for cited in &req.derives_from {
            self.offer_act(key.tenant_id, key.key_id, *cited, Label::Derived);
        }
        if key.profile == Some(KeyProfile::Curator) {
            self.observer.curation(CuratorAct::Write.as_str());
        }
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
                // stays #130 (ADR-0039 Consequences, "The serve binding is
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

    /// The held listing (ADR-0055 d2): the rows `review next` would serve
    /// this key, with their text, a page at a time, narrowed by a subtree,
    /// an authoring key and a `claim_key` prefix. It claims nothing and
    /// journals nothing. Only a dreamer key a person holds is served it
    /// (d1).
    pub async fn review_held(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &HeldListingRequest,
    ) -> Result<HeldPage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_person_reviewer(key)?;
        if req.limit == 0 || req.limit > HELD_PAGE_MAX {
            return Err(AppError::BulkShape {
                field: "limit",
                reason: format!("limit is 1 to {HELD_PAGE_MAX}"),
            });
        }
        let tenant_id = key.tenant_id;
        let mut nodes = self.dreamer_nodes(&caller).await?;
        if let Some(workarea) = &req.workarea {
            let root = self.resolve(key, NamedKind::Workarea, workarea).await?;
            let subtree: BTreeSet<Uuid> = self
                .journal
                .workarea_subtree(tenant_id, root)
                .await?
                .into_iter()
                .map(|w| w.workarea_id)
                .collect();
            nodes.retain(|n| subtree.contains(n));
        }
        let author_key_id = match &req.author_key {
            Some(author) => Some(self.resolve(key, NamedKind::Key, author).await?),
            None => None,
        };
        let now = self.clock.now();
        let after = req
            .cursor
            .as_deref()
            .map(|cursor| ListingCursor::resume(cursor, key.key_id, &now))
            .transpose()?;
        let filter = HeldFilter {
            nodes,
            author_key_id,
            claim_key_prefix: req.claim_key_prefix.clone(),
        };
        // One row past the page says whether another page exists.
        let mut rows = self
            .journal
            .review_held(
                tenant_id,
                &key.principal,
                key.key_id,
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
                    created_at: last.held_since.clone(),
                    artifact_id: last.memory.memory_id,
                },
                issued_at: now.clone(),
            }
            .encode()
        });
        let mut ancestries = BTreeMap::new();
        let mut users: BTreeMap<Uuid, bool> = BTreeMap::new();
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let m = &row.memory;
            // R2 and R3 in depth: nothing the queue would not serve this key
            // leaves the application layer, whatever the store returned.
            let excluded = m.lineage_key_ids.contains(&key.key_id)
                || m.lineage_principals.contains(&key.principal);
            if m.status != MemoryStatus::Held || excluded || !filter.nodes.contains(&m.workarea_id)
            {
                continue;
            }
            let same = match users.get(&m.author_key_id) {
                Some(same) => *same,
                None => {
                    let same = self.same_person(tenant_id, m.author_key_id, key).await?;
                    users.insert(m.author_key_id, same);
                    same
                }
            };
            items.push(HeldItem {
                same_user: same,
                held_since: row.held_since,
                served: self.served_in(&caller, row.memory, &mut ancestries).await?,
            });
        }
        Ok(HeldPage { items, next_cursor })
    }

    /// The named held rows as the listing would show them to this key, for
    /// the admin page's confirmation (ADR-0055 d10): each id with its row
    /// when it is still held, in the key's dreamer access and outside its
    /// exclusion, else `None`. It claims and journals nothing.
    pub async fn review_named(
        &self,
        bearer: impl Into<Bearer<'_>>,
        ids: &[Uuid],
    ) -> Result<Vec<(Uuid, Option<ServedMemory>)>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_person_reviewer(key)?;
        let nodes: BTreeSet<Uuid> = self.dreamer_nodes(&caller).await?.into_iter().collect();
        let mut rows: BTreeMap<Uuid, MemoryRecord> = self
            .journal
            .memories(key.tenant_id, ids)
            .await?
            .into_iter()
            .filter(|m| {
                m.status == MemoryStatus::Held
                    && nodes.contains(&m.workarea_id)
                    && !m.lineage_key_ids.contains(&key.key_id)
                    && !m.lineage_principals.contains(&key.principal)
            })
            .map(|m| (m.memory_id, m))
            .collect();
        let mut ancestries = BTreeMap::new();
        let mut named = Vec::with_capacity(ids.len());
        for id in ids {
            let served = match rows.remove(id) {
                Some(m) => Some(self.served_in(&caller, m, &mut ancestries).await?),
                None => None,
            };
            named.push((*id, served));
        }
        Ok(named)
    }

    /// A bulk verdict (ADR-0055 d3, d4): each named id judged in turn by
    /// the single verdict's use case, `verdict_on` — access, R2
    /// against the row's exclusion set, the transition, the frozen refusal,
    /// the compare-and-set, its own signed verdict and ratification in its
    /// own transaction — and answered with its outcome or the refusal a
    /// single verdict naming it would have answered. Each item records under
    /// the request's idempotency key and its own id, so a retry replays what
    /// was decided and judges the rest (d12). Only a dreamer key a person
    /// holds sends one (d1); the ids are 1 to [`BULK_VERDICT_MAX`] distinct
    /// ids, and a reject carries its reason (d3, d8).
    pub async fn review_bulk(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &BulkVerdictRequest,
    ) -> Result<Vec<BulkItem>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_person_reviewer(key)?;
        if let Some(reason) = req.reason.as_deref() {
            self.scan_veto_only(key.tenant_id, "reason", reason)?;
        }
        let ids_refused = |reason: String| AppError::BulkShape {
            field: "/artifact_ids",
            reason,
        };
        if req.artifact_ids.is_empty() || req.artifact_ids.len() > BULK_VERDICT_MAX {
            return Err(ids_refused(format!(
                "artifact_ids names 1 to {BULK_VERDICT_MAX} ids, not {}",
                req.artifact_ids.len()
            )));
        }
        let mut seen = BTreeSet::new();
        if let Some(repeated) = req.artifact_ids.iter().find(|id| !seen.insert(**id)) {
            return Err(ids_refused(format!(
                "artifact_ids names {repeated} more than once"
            )));
        }
        if req.verdict == Verdict::Reject && req.reason.as_deref().unwrap_or("").is_empty() {
            return Err(AppError::BulkShape {
                field: "/reason",
                reason: "reason is required to reject".to_owned(),
            });
        }
        let mut items = Vec::with_capacity(req.artifact_ids.len());
        for &memory_id in &req.artifact_ids {
            let result = self.bulk_item(&caller, req, memory_id).await;
            items.push(BulkItem { memory_id, result });
        }
        Ok(items)
    }

    /// One item of [`Self::review_bulk`]: its record replayed when one
    /// exists, else the single verdict, retried on a chain-head conflict as
    /// the single verdict's route retries it.
    async fn bulk_item(
        &self,
        caller: &Caller,
        req: &BulkVerdictRequest,
        memory_id: Uuid,
    ) -> Result<VerdictOutcome, AppError> {
        let key = &caller.key;
        let doc = json!({
            "method": "POST",
            "route": "/v2/review/verdicts/bulk",
            "item": {
                "artifact_id": memory_id.to_string(),
                "verdict": match req.verdict { Verdict::Approve => "approve", Verdict::Reject => "reject" },
                "reason": req.reason,
            },
        });
        let recording = || -> Result<VerdictRecording, AppError> {
            Ok(VerdictRecording {
                scope: IdempotencyScope {
                    tenant_id: key.tenant_id,
                    key_id: key.key_id,
                    workarea_id: Uuid::nil(),
                    idem_key: format!("{}/{memory_id}", req.idempotency_key),
                },
                request_digest: sha256_digest_string(&canonical_bytes(&doc)?),
            })
        };
        let first = recording()?;
        if let Some(recorded) = self
            .recorded::<RecordedVerdict>(&first.scope, &first.request_digest)
            .await?
        {
            return recorded.replay();
        }
        let mut attempt = 0;
        loop {
            match self
                .verdict_on(
                    caller,
                    memory_id,
                    req.verdict,
                    req.reason.as_deref(),
                    Some(recording()?),
                )
                .await
            {
                Err(AppError::Store(
                    StoreError::SeqConflict { .. } | StoreError::PrevHashConflict { .. },
                )) if attempt + 1 < BULK_CHAIN_RETRIES => attempt += 1,
                other => return other,
            }
        }
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
        let curator = caller.key.profile == Some(KeyProfile::Curator);
        // A curator never approves, inside or out: it narrows and never
        // widens (ADR-0030 d9).
        if curator && verdict == Verdict::Approve {
            return Err(AppError::Curator {
                verb: "verdict",
                refusal: CuratorRefusal::NarrowOnly,
            });
        }
        let outcome = self
            .verdict_judged(caller, memory_id, verdict, reason, recording)
            .await?;
        if curator {
            self.observer.curation(CuratorAct::Reject.as_str());
        }
        Ok(outcome)
    }

    async fn verdict_judged(
        &self,
        caller: &Caller,
        memory_id: Uuid,
        verdict: Verdict,
        reason: Option<&str>,
        recording: Option<VerdictRecording>,
    ) -> Result<VerdictOutcome, AppError> {
        let key = &caller.key;
        if let Some(reason) = reason {
            self.scan_veto_only(key.tenant_id, "reason", reason)?;
        }
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
        // A curator rejects inside an in-progress subtree only (ADR-0030
        // d9); R2 below refuses it what it authored or derived from.
        self.curator_may_act(key, "verdict", memory.workarea_id, None)
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
        // A claim's ratification is marked as a transition's and an edge's
        // are: whether the reviewing key and the authoring key name one
        // user (ADR-0046 d2, ADR-0055 d7).
        let same_user = self
            .same_person(key.tenant_id, memory.author_key_id, key)
            .await?;

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
                "format": memory.format.media_type,
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
                    same_user,
                };
                let idempotency = recording.as_ref().map(|r| r.put(&outcome)).transpose()?;
                let committed = self
                    .commit(
                        verdict_event,
                        MetadataFold::MemoryRejected { memory_id },
                        idempotency,
                    )
                    .await;
                let answered = self
                    .verdict_committed(committed, recording.as_ref(), outcome)
                    .await;
                self.verdict_observed(&answered, "reject", &memory.created_at);
                answered
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
                            "same_user": same_user,
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
                            "same_user": same_user,
                            "policy": policy,
                        }),
                        MetadataFold::TipSuperseded {
                            successor: memory_id,
                            replaced: target.memory_id,
                            replaced_valid_until: ratified_end(&target.valid, &memory.valid)
                                .map(Box::new),
                        },
                    ),
                };
                // The end this ratification gives the replaced version,
                // signed in its event, only when it gives one (proposed
                // ADR-0059 d4).
                let mut payload = payload;
                if let MetadataFold::TipSuperseded {
                    replaced_valid_until: Some(end),
                    ..
                } = &fold
                    && let Some(members) = payload.as_object_mut()
                {
                    members.insert("replaces_valid_until".to_owned(), Value::from(end.as_str()));
                }
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
                    same_user,
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
                let answered = self
                    .verdict_committed(committed, recording.as_ref(), outcome)
                    .await;
                self.verdict_observed(&answered, "approve", &memory.created_at);
                answered
            }
        }
    }

    /// Tells the observer a verdict committed, and how long after its
    /// memory was submitted; a replay committed nothing and tells nothing
    /// (ADR-0054 d1, d12).
    fn verdict_observed(
        &self,
        answered: &Result<VerdictOutcome, AppError>,
        verdict: &'static str,
        submitted: &EventTime,
    ) {
        if let Ok(outcome) = answered
            && !outcome.replayed
        {
            let waited = self.clock.now().unix_micros() - submitted.unix_micros();
            #[allow(clippy::cast_precision_loss, reason = "a wait in microseconds")]
            self.observer
                .verdict(verdict, waited.max(0) as f64 / 1_000_000.0);
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
        let started = std::time::Instant::now();
        let result = self.recall_as(&caller, req).await?;
        // The answer's shape and latency, by pipeline, for the metrics
        // listener; then the event, handed to the writer without waiting,
        // so neither sits on the recall's path (ADR-0054 d1, d2).
        let shape = if result.below_floor > 0 {
            "below_floor"
        } else if result.hits.is_empty() {
            "empty"
        } else {
            "hits"
        };
        self.observer.recall_answered(
            result.pipeline.as_str(),
            shape,
            started.elapsed().as_secs_f64(),
        );
        let kinds: Vec<&'static str> = result
            .hits
            .iter()
            .map(|h| {
                h.served
                    .memory
                    .origin_kind
                    .map_or("none", OriginKind::as_str)
            })
            .collect();
        self.observer.recall_hits_by_origin(&kinds);
        // Hits served labelled `unreviewed` (ADR-0030 d5).
        let unreviewed = result
            .hits
            .iter()
            .filter(|h| h.served.memory.status == MemoryStatus::Held)
            .count() as u64;
        if unreviewed > 0 {
            self.observer.unreviewed_hits(unreviewed);
        }
        for (stage, seconds) in &result.timings.stages {
            self.observer
                .recall_stage(result.pipeline.as_str(), stage.as_str(), *seconds);
        }
        // The mode and the federated hits of a federated tenant's recall
        // (ADR-0072 Consequences).
        if let Some(record) = &result.federation {
            // A `scope` recall records nothing, so the mode is wide or list.
            self.observer.recall_sources(if record.sources.is_string() {
                "wide"
            } else {
                "list"
            });
            let federated = result
                .hits
                .iter()
                .filter(|h| h.source == Some(crate::federation::Via::Federated))
                .count() as u64;
            if federated > 0 {
                self.observer.federated_hits(federated);
            }
        }
        if let Some(telemetry) = &self.telemetry {
            telemetry.offer(TelemetryItem::Recall(Box::new(RecallEvent {
                federation: result.federation.clone(),
                tenant_id: caller.key.tenant_id,
                recall_id: result.recall_id,
                created_at: self.clock.now(),
                key_id: caller.key.key_id,
                principal: caller.key.principal.clone(),
                scope_workarea_id: result.scope_workarea_id,
                source_workarea_id: caller.key.default_workarea,
                pipeline: result.pipeline.as_str(),
                access: result.access.as_str(),
                query_digest: result.query_digest.clone(),
                query_text: nfc(&req.query),
                hits: result
                    .hits
                    .iter()
                    .take(HITS_STORED)
                    .map(|h| (h.served.memory.memory_id, h.score))
                    .collect(),
            })));
        }
        Ok(result)
    }

    /// The recall itself, as `caller`: what [`Self::recall`] answers, and
    /// what the real-usage replay runs under each original key, recording
    /// nothing (ADR-0054 d11).
    async fn recall_as(
        &self,
        caller: &Caller,
        req: &RecallRequest,
    ) -> Result<RecallResult, AppError> {
        let key = &caller.key;
        require_admissible(key, Role::Reader, "recall")?;
        // Timings are served only where the deployment serves them, and a
        // request for them elsewhere is refused by name (ADR-0049 d14).
        if req.timings && !self.search.timings() {
            return Err(AppError::TimingsOff);
        }
        let mut timings = Timings::default();
        let lap = std::time::Instant::now();
        let scope = self.target_workarea(key, req.workarea.as_ref()).await?;
        self.require_access(caller, scope, Role::Reader, "recall")
            .await?;
        // The query is its NFC text, searched and digested alike (ADR-0025
        // d9).
        let query = nfc(&req.query);
        // One now for the recall: the expiry cut and the query's relative
        // dates (proposed ADR-0059 d5); then the window the
        // query names, by rule (d6).
        let now_us = match req.now.as_deref() {
            Some(sent) => parse_now(sent).ok_or(AppError::NowUnparsable)?,
            None => self.clock.now().unix_micros(),
        };
        let window = parse_window(&query, now_us);
        // An as-of recall reads an instant that has happened, in its scope
        // alone (ADR-0059 d17, d18).
        if let Some(at) = &req.as_of {
            self.refuse_future(at, "/as_of")?;
            if req
                .sources
                .as_ref()
                .is_some_and(|s| *s != crate::federation::Sources::Scope)
            {
                return Err(AppError::AsOfInvalid {
                    field: "/sources",
                    reason: "an as-of recall reads its scope alone: sources is scope with as_of",
                });
            }
        }

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
        // At an instant, the steering tips then, promoted today (ADR-0059
        // d15, d18).
        let steering_rows = match &req.as_of {
            None => {
                self.journal
                    .steering_in_force(key.tenant_id, &applicable)
                    .await?
            }
            Some(at) => {
                self.journal
                    .steering_as_of(key.tenant_id, &applicable, at)
                    .await?
            }
        };
        let steering_tips: BTreeSet<Uuid> = match &req.as_of {
            None => BTreeSet::new(),
            Some(at) => {
                let ids: Vec<Uuid> = steering_rows.iter().map(|m| m.memory_id).collect();
                self.journal
                    .tips_as_of(key.tenant_id, &ids, at)
                    .await?
                    .into_iter()
                    .collect()
            }
        };
        let steering: Vec<MemoryRecord> = steering_rows
            .into_iter()
            .filter(|m| {
                m.kind == MemoryKind::Steering
                    && m.status == MemoryStatus::Promoted
                    && (if req.as_of.is_some() {
                        steering_tips.contains(&m.memory_id)
                    } else {
                        m.is_tip
                    })
                    && !m.retirement
                    && applicable.contains(&m.workarea_id)
            })
            .collect();

        // The served node set leaves out every in-progress subtree the scope
        // is not inside; inside one, `include_unreviewed` defaults to true
        // and admits every author's held rows on the served nodes, all of
        // which lie inside the scope's root (ADR-0030 d2, d3, d4).
        let ServedNodes {
            nodes: readable,
            inside,
            scope_root,
        } = self.served_nodes(caller, scope).await?;
        let include_unreviewed = req.include_unreviewed.unwrap_or(scope_root.is_some());
        let any_author = include_unreviewed && scope_root.is_some();
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
        // A recall naming no `sources` reads the tenant's default with its
        // default pipeline, off one row; one naming them reads only the
        // pipeline's default, as before federation (ADR-0072 d12).
        let (pipeline, policy) = if req.sources.is_none() {
            let (tenant_default, policy) = self.journal.recall_defaults(key.tenant_id).await?;
            (
                self.pipeline_from(req.search.as_deref(), tenant_default)?,
                // An as-of recall reads its scope, whatever the tenant's
                // default sources (ADR-0059 d18).
                req.as_of.is_none().then_some(policy),
            )
        } else {
            (
                self.pipeline_for(key.tenant_id, req.search.as_deref())
                    .await?,
                None,
            )
        };
        // Where the recall searches under `sources`: the scope's served
        // nodes, plus the sources `wide` selects, or a list's nodes alone;
        // and the reference set every BM25 statistic is taken over, fixed
        // before any selection (ADR-0072 d4, d5, d7).
        let expired_at = (!req.include_expired).then_some(now_us);
        let plan = self
            .plan_sources(
                caller, req, policy, scope, &hit_nodes, &kinds, pipeline, &query, expired_at,
            )
            .await?;
        let promoted: BTreeSet<Uuid> = plan.promoted.iter().copied().collect();
        // Under a list, own held rows come from a listed node alone.
        let within = |nodes: &[Uuid]| -> Vec<Uuid> {
            match &plan.held_within {
                Some(listed) => nodes
                    .iter()
                    .filter(|n| listed.contains(n))
                    .copied()
                    .collect(),
                None => nodes.to_vec(),
            }
        };
        let readable = within(&readable);
        let inside = within(&inside);
        // The candidate scope is the core's, never an adapter's: the
        // readable hit nodes, the kinds, and the own held rows only when
        // asked — the own key set is the calling key alone until rotation is
        // built (roles-and-keys.md#own), and for a session user key every key
        // naming its user, ADR-0045 d16's reading that the user page's text
        // search uses (d22); on the readable set, which already excludes the
        // key's global denials (ADR-0025 d4; ADR-0049 d2).
        let candidate_scope = CandidateScope {
            reference: plan.reference.clone(),
            promoted_nodes: plan.promoted.clone(),
            kinds: kinds.clone(),
            own_held: include_unreviewed.then(|| OwnHeldScope {
                author_key_ids: caller.own_key_ids(),
                // Every author's held rows on the nodes inside alone; the
                // key's own on every served node otherwise (ADR-0030 d3).
                nodes: if any_author {
                    inside.clone()
                } else {
                    readable.clone()
                },
                any_author,
                current_scanner: self.scanner.version().map(str::to_owned),
            }),
            // An ended version is outside the scope unless asked for: a
            // visibility rule, never a weight (proposed ADR-0059 d3).
            expired_at,
            anchor: req.anchor.clone(),
            as_of: req.as_of.clone(),
        };
        timings.lap(Stage::Scope, lap);
        // A fused pipeline ranks by position, so under a reference set its
        // lists are read over the whole set and restricted to the searched
        // nodes after fusion: a hit's fused score is then the union's, and
        // no selection moves it (ADR-0072 d7, as built).
        let fused_over_reference =
            plan.reference.is_some() && matches!(pipeline.fusion(), Fusion::Rrf { .. });
        let retrieval_scope = match (&plan.reference, fused_over_reference) {
            (Some(reference), true) => CandidateScope {
                promoted_nodes: reference.clone(),
                ..candidate_scope.clone()
            },
            _ => candidate_scope.clone(),
        };
        let ran = self
            .run_pipeline(
                key.tenant_id,
                pipeline,
                &retrieval_scope,
                &query,
                window.as_ref(),
                req,
                &mut timings,
            )
            .await?;
        let mut lap = std::time::Instant::now();
        let (mut candidates, terms, fallback) = (ran.fused, ran.terms, ran.fallback);
        // The core's re-check at an instant: which candidates were their
        // claim's tip then, read once for the list (ADR-0059 d15, d16).
        let tips_then: BTreeSet<Uuid> = match &req.as_of {
            None => BTreeSet::new(),
            Some(at) => {
                let ids: Vec<Uuid> = candidates.iter().map(|c| c.memory_id).collect();
                self.journal
                    .tips_as_of(key.tenant_id, &ids, at)
                    .await?
                    .into_iter()
                    .collect()
            }
        };
        let recorded = |m: &MemoryRecord| {
            req.as_of
                .as_ref()
                .is_none_or(|at| m.created_at.as_str() <= at.as_str())
        };
        self.observer
            .search(pipeline.as_str(), fallback, ran.access.as_str());
        // R3 defense in depth: the gate changed visibility. Nothing
        // unpromoted or unreadable leaves the application layer except the
        // caller's own held rows when it asked for them; a replaced version
        // or any version of a retired claim is promoted history, never a
        // hit; a retirement version is never in force (ADR-0025 d4, d8).
        // The reranker is handed only rows this admits (ADR-0053 d2).
        let own = caller.own_key_ids();
        let admits = |m: &MemoryRecord| {
            let admitted = match m.status {
                MemoryStatus::Promoted => {
                    (if req.as_of.is_some() {
                        tips_then.contains(&m.memory_id)
                    } else {
                        m.is_tip
                    }) && promoted.contains(&m.workarea_id)
                }
                MemoryStatus::Held => {
                    include_unreviewed
                        && recorded(m)
                        && ((own.contains(&m.author_key_id) && readable.contains(&m.workarea_id))
                            || (any_author
                                && served_unreviewed(m, self.scanner.version())
                                && inside.contains(&m.workarea_id)))
                }
                _ => false,
            };
            admitted
                && !m.retirement
                && kinds.contains(&m.kind)
                && (req.include_expired || !m.valid.expired_at(now_us))
        };
        // The reranking stage, after fusion and before the cuts: the head
        // of the first stage's list, hydrated and admitted by the core,
        // reordered by the reranker's relevance; a reranker that is busy,
        // late or failing leaves the first stage's order and says so
        // (ADR-0053 d1, d5).
        let mut rows: BTreeMap<Uuid, MemoryRecord> = BTreeMap::new();
        // The fused list read over the reference set keeps the searched
        // nodes' rows alone, before any cut.
        if fused_over_reference && !candidates.is_empty() {
            let ids: Vec<Uuid> = candidates.iter().map(|c| c.memory_id).collect();
            rows.extend(
                self.journal
                    .memories(key.tenant_id, &ids)
                    .await?
                    .into_iter()
                    .filter(|m| admits(m))
                    .map(|m| (m.memory_id, m)),
            );
            candidates.retain(|c| rows.contains_key(&c.memory_id));
            lap = timings.lap(Stage::Hydrate, lap);
        }
        let mut rerank = RerankOutcome::NotRun;
        if pipeline.reranks()
            && let Some(reranker) = self.reranker.as_deref()
            && !candidates.is_empty()
        {
            let depth = self.search.rerank().depth.min(candidates.len());
            let head_ids: Vec<Uuid> = candidates[..depth].iter().map(|c| c.memory_id).collect();
            rows.extend(
                self.journal
                    .memories(key.tenant_id, &head_ids)
                    .await?
                    .into_iter()
                    .filter(|m| admits(m))
                    .map(|m| (m.memory_id, m)),
            );
            lap = timings.lap(Stage::Hydrate, lap);
            let tail = candidates.split_off(depth);
            let head: Vec<_> = candidates
                .into_iter()
                .filter(|c| rows.contains_key(&c.memory_id))
                .collect();
            let passages: Vec<String> = head
                .iter()
                .filter_map(|c| rows.get(&c.memory_id).map(|m| m.searched().to_owned()))
                .collect();
            let scored = reranker.score(query.clone(), passages).await;
            lap = timings.lap(Stage::Rerank, lap);
            let scored = match scored {
                // A score that is not a finite relevance in [0, 1] would sort
                // first or serialise as null: the list is refused whole.
                Ok(scores)
                    if scores
                        .iter()
                        .any(|s| !s.is_finite() || !(0.0..=1.0).contains(s)) =>
                {
                    Err(crate::ports::RerankError::Failed(
                        "a score is not a finite relevance in [0, 1]".to_owned(),
                    ))
                }
                Ok(scores) if scores.len() == head.len() => Ok(scores),
                Ok(scores) => Err(crate::ports::RerankError::Failed(format!(
                    "{} scores for {} passages",
                    scores.len(),
                    head.len()
                ))),
                Err(e) => Err(e),
            };
            candidates = match scored {
                Ok(scores) => {
                    rerank = RerankOutcome::Applied;
                    rerank_order(head, &scores)
                }
                Err(e) => {
                    rerank = RerankOutcome::Skipped(e.reason());
                    head
                }
            };
            candidates.extend(tail);
        }
        self.observer.rerank(rerank.as_str(), rerank.reason());
        // The k cut, then the relevance floor over what the reranker
        // scored, then the budget cut over the counts `body_bytes` gives
        // without any body read; steering is never cut and spent first
        // (ADR-0025 d7; ADR-0053 d6).
        if pipeline.reranks() {
            lap = timings.lap(Stage::Rerank, lap);
        }
        candidates.truncate(req.k);
        let floor = match rerank {
            RerankOutcome::Applied => self.search.rerank().floor,
            _ => None,
        };
        let mut below_floor = 0;
        if let Some(floor) = floor {
            // Only rows the core admits are counted as withheld: the tail
            // past the depth is hydrated and admitted first (ADR-0053 d6).
            let unread: Vec<Uuid> = candidates
                .iter()
                .map(|c| c.memory_id)
                .filter(|id| !rows.contains_key(id))
                .collect();
            if !unread.is_empty() {
                lap = timings.lap(Stage::Cut, lap);
                rows.extend(
                    self.journal
                        .memories(key.tenant_id, &unread)
                        .await?
                        .into_iter()
                        .map(|m| (m.memory_id, m)),
                );
                lap = timings.lap(Stage::Hydrate, lap);
            }
            candidates.retain(|c| rows.get(&c.memory_id).is_some_and(admits));
            (candidates, below_floor) = apply_floor(candidates, floor);
        }
        // Spellings only for an empty or thin result, fewer hits than
        // `DID_YOU_MEAN_BELOW` (or than `k` when it asks fewer) before the
        // budget cut, for the
        // words no hit holds, from the vocabulary of the readable nodes
        // alone, under any pipeline but v0.5.0's (ADR-0049 d6, d12).
        // An answer the floor thinned matched its words; its rows were
        // withheld, so it offers no spellings (ADR-0053 d6).
        lap = timings.lap(Stage::Cut, lap);
        let did_you_mean = if pipeline.suggests()
            && below_floor == 0
            && candidates.len() < DID_YOU_MEAN_BELOW.min(req.k)
        {
            let unmatched: Vec<String> = terms
                .iter()
                .filter(|t| !candidates.iter().any(|c| c.matched.contains(t)))
                .cloned()
                .collect();
            if unmatched.is_empty() {
                Vec::new()
            } else {
                self.journal
                    .suggest(key.tenant_id, &candidate_scope, &unmatched, 3)
                    .await?
            }
        } else {
            Vec::new()
        };
        lap = timings.lap(Stage::Suggest, lap);

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
        lap = timings.lap(Stage::Cut, lap);
        let ids: Vec<Uuid> = candidates
            .iter()
            .map(|c| c.memory_id)
            .filter(|id| !rows.contains_key(id))
            .collect();
        if !ids.is_empty() {
            rows.extend(
                self.journal
                    .memories(key.tenant_id, &ids)
                    .await?
                    .into_iter()
                    .map(|m| (m.memory_id, m)),
            );
        }
        let mut served_ids: Vec<Uuid> = steering.iter().map(|m| m.memory_id).collect();
        let mut ancestries = BTreeMap::new();
        let mut served_steering = Vec::with_capacity(steering.len());
        for m in steering {
            served_steering.push(self.served_in(caller, m, &mut ancestries).await?);
        }
        let mut hits = Vec::with_capacity(candidates.len());
        for candidate in &candidates {
            let Some(m) = rows.remove(&candidate.memory_id) else {
                continue;
            };
            // R3 defense in depth, `admits` above; each hit names only the
            // ancestors the caller can read (ADR-0025 d3).
            if admits(&m) {
                served_ids.push(m.memory_id);
                let home = m.workarea_id;
                hits.push(RecallHit {
                    served: self.served_in(caller, m, &mut ancestries).await?,
                    score: candidate.score,
                    matched: candidate.matched.clone(),
                    via: candidate.via.clone(),
                    tokens: candidate.tokens.clone(),
                    rerank: candidate.rerank,
                    rank_before: candidate.rank_before,
                    // Each hit of a federated answer says whether its
                    // workarea is the scope's or another team's (ADR-0072
                    // d8).
                    source: plan.report.as_ref().map(|_| {
                        if plan.federated.contains(&home) {
                            crate::federation::Via::Federated
                        } else {
                            crate::federation::Via::Scope
                        }
                    }),
                });
            }
        }
        timings.lap(Stage::Hydrate, lap);
        Ok(RecallResult {
            recall_id: self.ids.new_id(),
            scope_workarea_id: scope,
            steering: served_steering,
            hits,
            budget,
            served_ids,
            query_digest: recall_query_digest(&query, req, scope, pipeline, plan.record.as_ref())?,
            pipeline,
            terms,
            fallback,
            did_you_mean,
            rerank,
            floor,
            below_floor,
            access: ran.access,
            pruned: ran.pruned,
            timings,
            window,
            sources: plan.report,
            federation: plan.record,
        })
    }

    /// The guardrail read (proposed ADR-0037 d3, d4, d5, d7): reader on the
    /// scope node by the refusal order, as recall is, the node the key's
    /// default or the request's workarea within its grants. The set is
    /// d4's: the promoted served guardrail tips, never a retirement version,
    /// on the scope node and, under the key's `guardrail_scope`
    /// `ancestors`, on each ancestor of it, each only where the key's
    /// reader effective access covers that node; `workarea` keeps the scope
    /// node alone. The scope is the key's own, set at issuance; nothing in
    /// the request changes it. The set is ordered by severity, depth,
    /// `promoted_at` and `artifact_id`, never by a query; rows of one
    /// `claim_key` with different severities each name the others in
    /// `conflict`; over [`SET_CAP`] the set is served whole with
    /// `over_cap`. Held guardrails are never served as rules: the caller's
    /// own held ones in the same chain are listed by `claim_key` and version
    /// alone, at most [`OWN_HELD_MAX_LINES`] (d3). Each served row carries
    /// its approving verdicts' `held_by` and `at`, never a reviewer's key
    /// name (d5); they are read from the journal, one pass over the tenant's
    /// chain per read whose cost is unmeasured. The result names the served
    /// ids, the read log's input as recall's does (ADR-0025 d9).
    pub async fn guardrails(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &GuardrailRequest,
    ) -> Result<GuardrailSet, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reader, "guardrails")?;
        let scope_node = self.target_workarea(key, req.workarea.as_ref()).await?;
        self.require_access(&caller, scope_node, Role::Reader, "guardrails")
            .await?;
        let tenant_id = key.tenant_id;
        let guardrail_scope = key.guardrail_scope;

        // The chain runs node to root, so each node's own ancestry is its
        // suffix and its depth from the root is the suffix's length less
        // one; the scope bounds it before access does.
        let chain = self
            .journal
            .workarea_ancestry(tenant_id, scope_node)
            .await?
            .unwrap_or_default();
        let ids: Vec<Uuid> = chain.iter().map(|w| w.workarea_id).collect();
        let scope_path = derived_path(&chain);
        let bound = match guardrail_scope {
            GuardrailScope::Workarea => ids.len().min(1),
            GuardrailScope::Ancestors => ids.len(),
        };
        let mut nodes: BTreeMap<Uuid, (u32, bool, String)> = BTreeMap::new();
        for i in 0..bound {
            let depth = u32::try_from(ids.len() - 1 - i).unwrap_or(u32::MAX);
            let reads = caller.effective(&ids[i..]).contains(&Role::Reader);
            if reads || !caller.denied(&ids[i..]) {
                nodes.insert(ids[i], (depth, reads, derived_path(&chain[i..])));
            }
        }
        let listed: Vec<Uuid> = nodes.keys().copied().collect();
        let rows = self
            .journal
            .guardrail_rows(tenant_id, &listed, key.key_id)
            .await?;

        // R3 defense in depth: a served rule is a readable promoted tip in
        // force with its fields; a held row is listed to its author alone
        // and never served as a rule, inside an in-progress workarea too
        // (d3).
        let mut in_force = Vec::new();
        let mut own_held = Vec::new();
        for m in rows {
            if m.kind != MemoryKind::Guardrail {
                continue;
            }
            let Some(&(depth, reads, _)) = nodes.get(&m.workarea_id) else {
                continue;
            };
            match m.status {
                MemoryStatus::Promoted if m.is_tip && !m.retirement && reads => {
                    if let (Some(fields), Some(at)) = (m.guardrail.clone(), m.promoted_at.clone()) {
                        in_force.push((depth, *fields, at, m));
                    }
                }
                MemoryStatus::Held if m.author_key_id == key.key_id => {
                    own_held.push(OwnHeldGuardrail {
                        claim_key: m.claim_key,
                        version: m.version,
                    });
                }
                _ => {}
            }
        }
        guardrail_order(&mut in_force, |(depth, fields, at, m)| {
            (fields.severity, *depth, at, m.memory_id)
        });
        let conflict = conflicts(&in_force, |(_, fields, _, m)| {
            (m.claim_key.as_str(), fields.severity, m.memory_id)
        });
        own_held.sort_by(|a, b| {
            a.claim_key
                .cmp(&b.claim_key)
                .then_with(|| a.version.cmp(&b.version))
        });
        own_held.truncate(OWN_HELD_MAX_LINES);

        let verdicts = self
            .approvals(tenant_id, in_force.iter().map(|(_, _, _, m)| m.memory_id))
            .await?;
        let served_ids: Vec<Uuid> = in_force.iter().map(|(_, _, _, m)| m.memory_id).collect();
        let versions: Vec<(Uuid, u32)> = in_force
            .iter()
            .map(|(_, _, _, m)| (m.memory_id, m.version))
            .collect();
        let over_cap = in_force.len() > SET_CAP;
        let guardrails = in_force
            .into_iter()
            .zip(conflict)
            .map(
                |((depth, fields, promoted_at, m), conflict)| ServedGuardrail {
                    artifact_id: m.memory_id,
                    workarea_path: nodes.get(&m.workarea_id).map(|(_, _, path)| path.clone()),
                    verdicts: verdicts.get(&m.memory_id).cloned().unwrap_or_default(),
                    tokens: budget_tokens(m.body_bytes),
                    claim_key: m.claim_key,
                    version: m.version,
                    workarea_id: m.workarea_id,
                    depth,
                    severity: fields.severity,
                    body: m.body,
                    rule: fields.rule,
                    promoted_at,
                    conflict,
                },
            )
            .collect();
        Ok(GuardrailSet {
            scope_node,
            scope_path,
            guardrail_scope,
            set_digest: set_digest(&versions),
            over_cap,
            guardrails,
            own_held,
            served_ids,
        })
    }

    /// The approving verdicts on each of `memory_ids`, in journal order: the
    /// reviewing key's `held_by` and the verdict's `at` (proposed ADR-0037
    /// d5). A verdict whose key the tenant does not hold fails closed.
    async fn approvals(
        &self,
        tenant_id: Uuid,
        memory_ids: impl Iterator<Item = Uuid>,
    ) -> Result<BTreeMap<Uuid, Vec<GuardrailVerdict>>, AppError> {
        let wanted: BTreeSet<Uuid> = memory_ids.collect();
        let mut out: BTreeMap<Uuid, Vec<GuardrailVerdict>> = BTreeMap::new();
        if wanted.is_empty() {
            return Ok(out);
        }
        let mut held_by: BTreeMap<Uuid, HeldBy> = BTreeMap::new();
        for event in self.whole_chain(tenant_id).await? {
            let envelope = event.envelope;
            if envelope.event_type != event_types::REVIEW_APPROVE {
                continue;
            }
            let Some(memory_id) = payload_id(&envelope.payload, "artifact_id") else {
                continue;
            };
            if !wanted.contains(&memory_id) {
                continue;
            }
            let reviewer = envelope.api_key_id.ok_or_else(|| {
                StoreError::Backend(format!(
                    "review.approve at seq {} names no key",
                    envelope.seq
                ))
            })?;
            let attested = match held_by.get(&reviewer) {
                Some(attested) => *attested,
                None => {
                    let record = self.journal.key(tenant_id, reviewer).await?.ok_or_else(|| {
                        StoreError::Backend(format!(
                            "review.approve at seq {} names key {reviewer}, which the tenant does not hold",
                            envelope.seq
                        ))
                    })?;
                    held_by.insert(reviewer, record.held_by);
                    record.held_by
                }
            };
            out.entry(memory_id).or_default().push(GuardrailVerdict {
                held_by: attested,
                at: envelope.at,
            });
        }
        Ok(out)
    }

    /// The pipeline a recall runs: the request's `search` when it names a
    /// served one, else the tenant's journalled default while the
    /// deployment still serves it, else the deployment's (ADR-0049 d4). A
    /// name the deployment does not serve, unknown or disabled, is refused
    /// naming the field, never replaced by another pipeline.
    async fn pipeline_for(
        &self,
        tenant_id: Uuid,
        requested: Option<&str>,
    ) -> Result<Pipeline, AppError> {
        if let Some(name) = requested {
            return Pipeline::parse(name)
                .filter(|p| self.search.serves(*p))
                .ok_or_else(|| AppError::SearchUnavailable {
                    name: name.to_owned(),
                    served: self.served_pipelines(),
                });
        }
        let tenant_default = self.journal.tenant_search_default(tenant_id).await?;
        self.pipeline_from(None, tenant_default)
    }

    /// The pipeline a recall runs: the one it names, which the deployment
    /// must serve, else the tenant's default where served, else the
    /// deployment's (ADR-0049 d4).
    fn pipeline_from(
        &self,
        requested: Option<&str>,
        tenant_default: Option<String>,
    ) -> Result<Pipeline, AppError> {
        if let Some(name) = requested {
            return Pipeline::parse(name)
                .filter(|p| self.search.serves(*p))
                .ok_or_else(|| AppError::SearchUnavailable {
                    name: name.to_owned(),
                    served: self.served_pipelines(),
                });
        }
        Ok(tenant_default
            .and_then(|name| Pipeline::parse(&name))
            .filter(|p| self.search.serves(*p))
            .unwrap_or_else(|| self.search.default_pipeline()))
    }

    /// The served pipeline names, comma-separated, for a refusal's text.
    fn served_pipelines(&self) -> String {
        self.search
            .enabled()
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Runs `pipeline` over the scope: each retriever's list, read
    /// `FUSION_DEPTH` deep when the lists are fused, then the core's fusion
    /// (ADR-0049 d3). Under `match: all` every retriever keeps only rows
    /// with every term; when that leaves nothing, the any-term lists are
    /// served and `fallback` says so (d5). The hits come back in the total
    /// order of hits, uncut.
    #[allow(clippy::too_many_arguments)]
    async fn run_pipeline(
        &self,
        tenant_id: Uuid,
        pipeline: Pipeline,
        scope: &CandidateScope,
        query: &str,
        window: Option<&Window>,
        req: &RecallRequest,
        timings: &mut Timings,
    ) -> Result<PipelineRun, AppError> {
        let depth = pipeline.depth(req.k);
        let encoding = std::time::Instant::now();
        // A sparse pipeline reads the query's tokens, weighted by the
        // model's shipped idf, with no model run (ADR-0052 d14, d16).
        let sparse = if pipeline.needs_sparse() {
            let encoder = self.sparse.as_deref().ok_or_else(|| {
                AppError::Store(StoreError::Backend(format!(
                    "{pipeline} reads learned sparse postings and no sparse encoder is loaded"
                )))
            })?;
            let encoded = encoder.encode_query(query);
            timings.lap(Stage::Retrieve(RetrieverKind::Sparse), encoding);
            encoded
        } else {
            Vec::new()
        };
        // A reranking pipeline reads its first stage as deep as the head it
        // reorders (ADR-0053 d4).
        let depth = if pipeline.reranks() {
            depth.max(self.search.rerank().depth)
        } else {
            depth
        };
        let mut modes = vec![req.match_mode];
        if req.match_mode == MatchMode::All && pipeline != Pipeline::KeywordAll {
            modes.push(MatchMode::Any);
        }
        let mut last = PipelineRun::default();
        for (attempt, mode) in modes.into_iter().enumerate() {
            let mut lists = Vec::with_capacity(pipeline.retrievers().len());
            for &kind in pipeline.retrievers() {
                let started = std::time::Instant::now();
                // The time leg reads only when the query names a window, and
                // then its own depth; the core orders it by the text list
                // and caps it (proposed ADR-0059 d7).
                if kind == RetrieverKind::Time {
                    let list = match window {
                        None => crate::search::Retrieved::default(),
                        Some(window) => {
                            // The leg reads only the rows the text list
                            // found, by id: its cost is that list's, and a
                            // window holding more rows loses none of them
                            // (proposed ADR-0059 d7).
                            let found: Vec<Uuid> = lists
                                .first()
                                .map(|(_, text): &(RetrieverKind, crate::search::Retrieved)| {
                                    text.candidates.iter().map(|c| c.memory_id).collect()
                                })
                                .unwrap_or_default();
                            let time = self
                                .journal
                                .retrieve(
                                    tenant_id,
                                    kind,
                                    scope,
                                    RetrievalQuery {
                                        query,
                                        match_mode: mode,
                                        k: TIME_DEPTH,
                                        sparse: &sparse,
                                        window: Some(window),
                                        within: &found,
                                    },
                                )
                                .await?;
                            match lists.first() {
                                Some((_, text)) => order_time_list(time, text),
                                None => time,
                            }
                        }
                    };
                    timings.lap(Stage::Retrieve(kind), started);
                    lists.push((kind, list));
                    continue;
                }
                let list = self
                    .journal
                    .retrieve(
                        tenant_id,
                        kind,
                        scope,
                        RetrievalQuery {
                            query,
                            match_mode: mode,
                            k: depth,
                            sparse: &sparse,
                            window,
                            within: &[],
                        },
                    )
                    .await?;
                timings.lap(Stage::Retrieve(kind), started);
                lists.push((kind, list));
            }
            let terms = query_terms(&lists);
            let access = pipeline_access(&lists);
            let pruned = pruned_words(&lists);
            let started = std::time::Instant::now();
            let fused = fuse_capped(lists, pipeline.fusion(), depth);
            timings.lap(Stage::Fuse, started);
            let run = PipelineRun {
                fused,
                terms,
                fallback: attempt > 0,
                access,
                pruned,
            };
            if !run.fused.is_empty() {
                return Ok(run);
            }
            last = PipelineRun {
                fallback: false,
                ..run
            };
        }
        Ok(last)
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
        self.get_memory_as_of(bearer, memory_id, None).await
    }

    /// [`Self::get_memory`] at an instant (ADR-0059 d15, d19): a version
    /// recorded after `as_of` does not exist then, `access.not_found`, and
    /// any other answers exactly as get answers today; an instant after the
    /// lead's clock is refused (d17).
    pub async fn get_memory_as_of(
        &self,
        bearer: impl Into<Bearer<'_>>,
        memory_id: Uuid,
        as_of: Option<&EventTime>,
    ) -> Result<ServedMemory, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_admissible(&caller.key, Role::Reader, "get")?;
        if let Some(at) = as_of {
            self.refuse_future(at, "as_of")?;
        }
        let memory = self
            .journal
            .memory(caller.key.tenant_id, memory_id)
            .await?
            .filter(|m| as_of.is_none_or(|at| m.created_at.as_str() <= at.as_str()))
            .ok_or(AppError::NotFound)?;
        if self.readable(&caller, &memory).await? {
            // A get by id labels the key's recall that served the version
            // (ADR-0054 d5).
            self.offer_act(
                caller.key.tenant_id,
                caller.key.key_id,
                memory_id,
                Label::Get,
            );
            self.served(&caller, memory).await
        } else {
            Err(AppError::NotFound)
        }
    }

    /// The format checks of a capture (ADR-0056 d1, d10, d13): the media type
    /// resolved against the closed list, the kinds that read prose held to
    /// prose, the title checked, and the source accepted by its adapter
    /// before the scan. Returns the fields to store and what the adapter
    /// parsed.
    fn capture_format(
        &self,
        kind: MemoryKind,
        requested: Option<&str>,
        title: Option<&str>,
        body: &str,
        retirement: bool,
    ) -> Result<(FormatFields, Parsed), AppError> {
        let resolved = self
            .formats
            .resolve(requested.unwrap_or("text/plain"))
            .map_err(|allowed| AppError::FormatUnknown { allowed })?;
        let prose_kind = matches!(kind, MemoryKind::Memory | MemoryKind::DerivedFact);
        if !resolved.prose && (!prose_kind || retirement) {
            return Err(AppError::FormatKindRefused { kind });
        }
        let title = match title {
            None => None,
            Some(raw) => {
                let t = nfc(raw);
                let t = t.trim();
                if t.is_empty() {
                    return Err(AppError::TitleInvalid("a title is not empty"));
                }
                if t.chars()
                    .any(|c| matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))
                {
                    return Err(AppError::TitleInvalid("a title is one line"));
                }
                if t.chars().count() > 100 {
                    return Err(AppError::TitleInvalid("a title is at most 100 characters"));
                }
                Some(t.to_owned())
            }
        };
        if !resolved.prose && title.is_none() {
            return Err(AppError::TitleInvalid(
                "a claim that is not prose carries a title stating the one fact its source shows",
            ));
        }
        let parsed = if retirement {
            Parsed::default()
        } else {
            resolved
                .adapter
                .validate(body)
                .map_err(AppError::FormatInvalid)?
        };
        let derived = resolved.adapter.search_text(&parsed);
        let search_text = match (&title, resolved.prose) {
            (None, true) => None,
            (Some(t), _) => Some(format!("{t}\n{derived}")),
            (None, false) => Some(derived),
        };
        let search_text_by = search_text
            .as_ref()
            .map(|_| resolved.adapter.id().to_owned());
        Ok((
            FormatFields {
                media_type: resolved.media_type,
                title,
                search_text,
                search_text_by,
            },
            parsed,
        ))
    }

    /// The scan over what a format adds to the source: the title, the
    /// format's own finding and every decoded view. A veto names the key
    /// path, never the value; it can add a finding and never remove one
    /// (ADR-0056 d6). A hold on a view is the row's finding when the source
    /// raised none.
    fn scan_derived(
        &self,
        tenant_id: Uuid,
        kind: MemoryKind,
        fields: &FormatFields,
        parsed: &Parsed,
        hold: &mut Option<String>,
    ) -> Result<(), AppError> {
        let mut note = |outcome: ScanOutcome, at: &str| -> Result<(), AppError> {
            match outcome {
                ScanOutcome::Pass => Ok(()),
                ScanOutcome::Hold(reason) => {
                    hold.get_or_insert(format!("{reason} at {at}"));
                    Ok(())
                }
                ScanOutcome::Veto(reason) => Err(AppError::ScanVeto(format!("{reason} at {at}"))),
            }
        };
        if let Some(finding) = &parsed.veto {
            return Err(AppError::ScanVeto(finding.clone()));
        }
        // The body is scanned by the caller; the rest of the one list.
        for target in scan_targets(kind, "", fields, parsed).into_iter().skip(1) {
            note(
                self.scanner.scan(&AdmissionEnvelope {
                    tenant_id,
                    kind: target.kind,
                    body: target.text,
                }),
                target.at.unwrap_or("/body"),
            )?;
        }
        Ok(())
    }

    /// The scan over an anchor's `repo`, `uri`, `path` and `fragment`
    /// (ADR-0057 d2): a path whose file name has a secret's shape, and any
    /// finding of the deterministic scanner, hold or veto alike, refuses the
    /// write naming the field, since a written anchor is never erased.
    /// A field the journal keeps as plaintext beside a body, a claim key or
    /// a verdict's reason: a veto finding refuses, a hold finding passes,
    /// since no review follows (ADR-0065 d7).
    fn scan_veto_only(&self, tenant_id: Uuid, kind: &str, text: &str) -> Result<(), AppError> {
        match self.scanner.scan(&AdmissionEnvelope {
            tenant_id,
            kind,
            body: text,
        }) {
            ScanOutcome::Veto(reason) => Err(AppError::ScanVeto(reason)),
            ScanOutcome::Pass | ScanOutcome::Hold(_) => Ok(()),
        }
    }

    fn scan_anchor(&self, tenant_id: Uuid, anchor: Option<&Anchor>) -> Result<(), AppError> {
        let Some(anchor) = anchor else {
            return Ok(());
        };
        self.scan_fields(tenant_id, &anchor.scanned(), "/anchor/path")
    }

    /// The anchor scan over `fields`: the one named `path_field` vetoed when
    /// its file name has a secret's shape, and every field by the
    /// deterministic scanner, a hold counting as a veto (ADR-0057 d2).
    fn scan_fields(
        &self,
        tenant_id: Uuid,
        fields: &[(&'static str, &str)],
        path_field: &str,
    ) -> Result<(), AppError> {
        for &(field, value) in fields {
            if field == path_field && anchor_grammar::secret_shaped(value) {
                return Err(AppError::AnchorScanVeto {
                    field,
                    finding: "anchor.secret_path".to_owned(),
                });
            }
            match self.scanner.scan(&AdmissionEnvelope {
                tenant_id,
                kind: "anchor",
                body: value,
            }) {
                ScanOutcome::Pass => {}
                ScanOutcome::Hold(finding) | ScanOutcome::Veto(finding) => {
                    return Err(AppError::AnchorScanVeto { field, finding });
                }
            }
        }
        Ok(())
    }

    /// Record a source check on a promoted, anchored served tip (ADR-0057
    /// d5, d18): from its author key (writer), or a dreamer with dreamer
    /// access on its workarea; a writer that can read it and did not author
    /// it is `access.author_only`, and a key that cannot read it is told
    /// nothing. A held, unanchored or superseded version is
    /// `request.validation` naming `/artifact_id`. Journals `source.checked`
    /// and folds it onto the row as the version's latest check, in one
    /// commit with the idempotency record; nothing moves through review and
    /// no text or anchor changes.
    pub async fn source_check(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &SourceCheckRequest,
    ) -> Result<(SourceCheckReceipt, bool), AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        let admissible = admissible_grant_roles(key.role);
        let dreamer = admissible.contains(&Role::Dreamer);
        if !dreamer && !admissible.contains(&Role::Writer) {
            return Err(AppError::VerbCeiling {
                role: key.role,
                verb: "source_check",
            });
        }
        let scope = IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id: Uuid::nil(),
            idem_key: req.idempotency_key.clone(),
        };
        let request_digest = sha256_digest_string(&canonical_bytes(&json!({
            "method": "POST",
            "route": "/v1/memories/{artifact_id}/source-check",
            "path_params": { "artifact_id": req.memory_id.to_string() },
            "body": {
                "rev": req.rev,
                "result": req.result.as_str(),
                "hash": req.hash,
                "path": req.path,
                "fragment": req.fragment,
            },
        }))?);
        if let Some(record) = self.journal.idempotency_get(&scope).await? {
            if record.request_digest != request_digest {
                return Err(AppError::IdempotencyDigestMismatch);
            }
            let receipt: SourceCheckReceipt = serde_json::from_value(record.response)
                .map_err(|e| AppError::StoredResponse(e.to_string()))?;
            return Ok((receipt, true));
        }
        let prev = self.head_event(key.tenant_id).await?;
        let target = self
            .journal
            .memory(key.tenant_id, req.memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if !self.readable(&caller, &target).await? {
            return Err(AppError::NotFound);
        }
        if dreamer {
            self.require_access(&caller, target.workarea_id, Role::Dreamer, "source_check")
                .await?;
        } else {
            if target.author_key_id != key.key_id {
                return Err(AppError::AuthorOnly);
            }
            self.require_access(&caller, target.workarea_id, Role::Writer, "source_check")
                .await?;
        }
        let invalid = |reason: &str| AppError::SourceCheckInvalid {
            field: "/artifact_id".to_owned(),
            reason: reason.to_owned(),
        };
        if target.status != MemoryStatus::Promoted || !target.is_tip {
            return Err(invalid(
                "a source check names a promoted served tip, never a held or superseded version",
            ));
        }
        let Some(anchor) = target.anchor.as_deref() else {
            return Err(invalid("the version has no anchor to check"));
        };
        let body = SourceCheckBody::check(
            anchor,
            &req.rev,
            req.result,
            req.hash.as_deref(),
            req.path.as_deref(),
            req.fragment.as_deref(),
        )
        .map_err(|e| AppError::SourceCheckInvalid {
            field: e.field,
            reason: e.reason,
        })?;
        // Where a span moved is kept for good and served to every reader,
        // so it is scanned as an anchor is (ADR-0057 d2, d18).
        if let Some((path, fragment)) = &body.moved_to {
            let mut fields: Vec<(&'static str, &str)> = vec![("/path", path.as_str())];
            if let Some(fragment) = fragment {
                fields.push(("/fragment", fragment.as_str()));
            }
            self.scan_fields(key.tenant_id, &fields, "/path")?;
        }
        let at = self.clock.now();
        let (moved_path, moved_fragment) = body
            .moved_to
            .clone()
            .map_or((None, None), |(p, f)| (Some(p), f));
        let payload = json!({
            "artifact_id": target.memory_id.to_string(),
            "workarea_id": target.workarea_id.to_string(),
            "rev": body.rev,
            "result": body.result.as_str(),
            "hash": body.hash,
            "path": moved_path,
            "fragment": moved_fragment,
        });
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::SOURCE_CHECKED,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let receipt = SourceCheckReceipt {
            memory_id: target.memory_id,
            rev: body.rev.clone(),
            result: body.result.as_str().to_owned(),
            moved_path,
            moved_fragment,
            at: at.as_str().to_owned(),
            journal_seq: event.envelope.seq,
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
            MetadataFold::SourceChecked {
                memory_id: target.memory_id,
                check: SourceCheck {
                    rev: body.rev,
                    result: body.result,
                    hash: body.hash,
                    moved_to: body.moved_to,
                    at,
                },
            },
            idempotency,
        )
        .await?;
        Ok((receipt, false))
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
    /// and from `oml_write` the vocabulary-kind check, and last the
    /// workarea not `validated`, else `workarea.frozen` (step 5; ADR-0009
    /// d5); then the checks every submit runs. The successor takes the next
    /// ordinal of the claim_key and journals `submission.remember`
    /// (`submission.learn` for a `derived_fact`) naming what it replaces,
    /// with the idempotency record in the same transaction; a commit that
    /// finds the record already
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
                receipt: self.replayed_supersede(&caller, receipt).await?,
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
        // A curator retires or rewrites inside an in-progress subtree only,
        // and never what it authored or derived from (ADR-0030 d9).
        self.curator_may_act(key, "supersede", target.workarea_id, Some(&target))
            .await?;
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
        // on oml_write the target is of no vocabulary kind (ADR-0025 d12).
        if let (Some(workarea_id), Some(sent)) = (header, &req.workarea)
            && workarea_id != target.workarea_id
        {
            return Err(AppError::WorkareaNotTarget {
                workarea: sent.clone(),
            });
        }
        // The claim's latest version the caller may read (promoted, or held
        // by its own keys), read after the head: a capture that commits in
        // between moves the head and the retry reads it again. Anything but
        // the version the caller names is refused before the tip check,
        // nothing captured; another author's held version never changes the
        // answer (ADR-0057 d19; R3).
        let claim_state = self
            .journal
            .claim_key_state(key.tenant_id, target.workarea_id, &target.claim_key)
            .await?;
        let reads_promoted = self
            .require_access(&caller, target.workarea_id, Role::Reader, "supersede")
            .await
            .is_ok();
        let latest = self
            .journal
            .claim_latest_readable(
                key.tenant_id,
                target.workarea_id,
                &target.claim_key,
                &caller.own_key_ids(),
                reads_promoted,
            )
            .await?
            .unwrap_or(target.version);
        if req.expected_version != latest {
            // An attempt under the same key that committed since the lookup
            // above made the claim move on: its record answers this one, as
            // it answers any replay (http-api.md idempotency).
            if let Some(receipt) = self.recorded(&scope, &request_digest).await? {
                return Ok(SupersedeOutcome {
                    receipt: self.replayed_supersede(&caller, receipt).await?,
                    replayed: true,
                });
            }
            return Err(AppError::VersionConflict { current: latest });
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
        // A guardrail is superseded on the HTTP API only (proposed ADR-0037
        // d10).
        if req.refuse_vocabulary_target && target.kind == MemoryKind::Guardrail {
            return Err(AppError::GuardrailTarget);
        }
        // Step 5's last check, after the head read above: a ratification
        // that validates the area in between moves the head and the commit
        // conflicts (roles-and-keys.md supersede; ADR-0009 d5).
        self.refuse_frozen(key.tenant_id, target.workarea_id, FrozenOp::Write)
            .await?;

        // The checks every submit runs: NFC first, then the scan, whose veto
        // journals nothing, then every declared ancestor readable
        // (admission.md capture; ADR-0025 d3, d6). A guardrail successor
        // re-sends its fields, which run their checks first (proposed
        // ADR-0037 d10).
        let body = nfc(&req.body);
        let (format, parsed) = self.capture_format(
            kind,
            req.format.as_deref(),
            req.title.as_deref(),
            &body,
            req.retirement,
        )?;
        let guardrail =
            guardrail_fields(kind, req.retirement, req.severity, req.rule.as_ref(), &body)?;
        // The successor's own valid time: it inherits none (proposed
        // ADR-0059 d2, d4).
        let valid = valid_time(
            req.valid_from.as_deref(),
            req.valid_until.as_deref(),
            kind,
            req.retirement,
        )?;
        self.scan_anchor(key.tenant_id, req.anchor.as_ref())?;
        let origin = recorded_origin(req.origin, req.anchor.as_ref())?;
        let scan_finding = match self.scanner.scan(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: kind.as_str(),
            body: &body,
        }) {
            ScanOutcome::Pass => None,
            ScanOutcome::Hold(reason) => Some(reason),
            ScanOutcome::Veto(reason) => return Err(AppError::ScanVeto(reason)),
        };
        let mut scan_finding = scan_finding;
        self.scan_derived(key.tenant_id, kind, &format, &parsed, &mut scan_finding)?;
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
        let version = claim_state
            .greatest_version
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                StoreError::Backend(format!(
                    "claim_key {:?} has no next ordinal",
                    target.claim_key
                ))
            })?;

        // The similar check, as a submit's, never naming a version of the
        // claim this replaces; a retirement states why and is not checked
        // (ADR-0050 d1).
        let similar = if req.retirement {
            Vec::new()
        } else {
            self.similar_gate(
                &caller,
                target.workarea_id,
                kind,
                format.search_text.as_deref().unwrap_or(&body),
                Some(&target.claim_key),
                req.on_similar,
            )
            .await?
        };

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
            scanned_under: self.scanner.version().map(str::to_owned),
            created_at: self.clock.now(),
            guardrail,
            valid: Box::new(valid),
            anchor: req.anchor.clone().map(Box::new),
            origin_kind: Some(origin),
            source_check: None,
            format: Box::new(format),
        };

        // The catalog payload of the submission, digest-only, plus what it
        // replaces and whether it retires (event-catalog.md
        // submission.remember; ADR-0025 d10, d11).
        let event_type = if kind == MemoryKind::DerivedFact {
            event_types::SUBMISSION_LEARN
        } else {
            event_types::SUBMISSION_REMEMBER
        };
        let mut payload = json!({
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
            "anchor": anchor_payload(req.anchor.as_ref()),
            "origin": origin.to_json(),
            "provenance": "agent",
            "simulation_id": Value::Null,
            "replaces": {
                "artifact_id": target.memory_id.to_string(),
                "version": target.version,
            },
            "retirement": req.retirement,
            "scan": {
                "result": if scan_finding.is_some() { "hold" } else { "pass" },
                "scanner_version": self.scanner.version(),
                "finding": scan_finding,
            },
        });
        guardrail_payload(&mut payload, successor.guardrail.as_deref())?;
        with_valid_time(&mut payload, &successor.valid);
        with_format_fields(&mut payload, &successor.format);
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
            similar,
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
                receipt: self.replayed_supersede(&caller, receipt).await?,
                replayed: true,
            });
        }
        committed?;
        // A supersession labels the key's recall that served the target,
        // and a citation labels the version it cites (ADR-0054 d5).
        self.offer_act(
            key.tenant_id,
            key.key_id,
            receipt.replaces.artifact_id,
            Label::Superseded,
        );
        for cited in &req.derives_from {
            self.offer_act(key.tenant_id, key.key_id, *cited, Label::Derived);
        }
        if key.profile == Some(KeyProfile::Curator) {
            self.observer.curation(if req.retirement {
                CuratorAct::Retire.as_str()
            } else {
                CuratorAct::Write.as_str()
            });
        }
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
        self.refuse_user_edge_into_universe(key, &[src.workarea_id, dst.workarea_id])
            .await?;
        self.refuse_crossing_edge(key.tenant_id, src.workarea_id, dst.workarea_id)
            .await?;
        // A curator flags a contradiction between two claims of one
        // in-progress subtree, neither its own (ADR-0030 d9).
        if key.profile == Some(KeyProfile::Curator) {
            if req.label != "contradicts" {
                return Err(AppError::Curator {
                    verb: VERB,
                    refusal: CuratorRefusal::NarrowOnly,
                });
            }
            self.curator_may_act(key, VERB, src.workarea_id, Some(src))
                .await?;
            self.curator_may_act(key, VERB, dst.workarea_id, Some(dst))
                .await?;
        }
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
        if key.profile == Some(KeyProfile::Curator) {
            self.observer.curation(CuratorAct::Flag.as_str());
        }
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
        self.refuse_user_edge_into_universe(key, &[memory.workarea_id, workarea_id])
            .await?;
        self.refuse_crossing_edge(key.tenant_id, memory.workarea_id, workarea_id)
            .await?;
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
        // A pending edge across the boundary is not ratified after a mark
        // (ADR-0030 d7).
        if let Some(claim) = self.journal.memory(key.tenant_id, edge.artifact_id).await? {
            self.refuse_crossing_edge(key.tenant_id, claim.workarea_id, edge.workarea_id)
                .await?;
        }
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
                "same_user": self.same_person(key.tenant_id, edge.proposer_key_id, key).await?,
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
        let scope_root = self.in_progress_root(key.tenant_id, workarea_id).await?;
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
            // The boundary at the read's node, whenever the edge was made
            // (ADR-0030 d2).
            if self.readable(&caller, &memory).await?
                && !crosses_into(
                    scope_root,
                    self.in_progress_root(key.tenant_id, memory.workarea_id)
                        .await?,
                )
            {
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
        // A pending edge across the boundary is not ratified after a mark
        // (ADR-0030 d7).
        self.refuse_crossing_edge(key.tenant_id, src.workarea_id, dst.workarea_id)
            .await?;
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
            "same_user": self.same_person(key.tenant_id, edge.proposer_key_id, key).await?,
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
        // array that grows with the tenant's workareas, unmeasured (#99,
        // #150).
        let readable_nodes = self.nodes_holding(&caller, Role::Reader).await?;
        // A scoped read: at `workarea`, else the key's default workarea, an
        // edge with an endpoint inside an in-progress subtree the scope is
        // not inside is not served (ADR-0030 d2).
        // With neither, the scope has no root, so every in-progress subtree
        // is left out (fail closed). Inside the scope's root, an endpoint
        // held by any author is served, as recall serves it (d3).
        let scope_root = match workarea_id.or(key.default_workarea) {
            Some(scope) => self.in_progress_root(tenant_id, scope).await?,
            None => None,
        };
        let mut kept = Vec::with_capacity(readable_nodes.len());
        let mut held_nodes = Vec::new();
        for node in readable_nodes {
            let root = self.in_progress_root(tenant_id, node).await?;
            if crosses_into(scope_root, root) {
                continue;
            }
            if root.is_some() {
                held_nodes.push(node);
            }
            kept.push(node);
        }
        let readable_nodes = kept;
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
            held_nodes,
            readable_nodes,
            reader_key_id: key.key_id,
            current_scanner: self.scanner.version().map(str::to_owned),
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
        // The tail is a scoped read: an event whose subject lies inside an
        // in-progress subtree the scope is not inside is left out (ADR-0030
        // d2).
        let scope_root = self.in_progress_root(key.tenant_id, scope).await?;
        let mut roots: BTreeMap<Uuid, Option<Uuid>> = BTreeMap::new();
        let mut events = Vec::new();
        for TailEvent { event, subject } in scan.events {
            let envelope = &event.envelope;
            let Some(judged) = judged_subject(envelope, subject.as_ref()) else {
                continue;
            };
            let root = match roots.get(&judged.node) {
                Some(root) => *root,
                None => {
                    let root = self.in_progress_root(key.tenant_id, judged.node).await?;
                    roots.insert(judged.node, root);
                    root
                }
            };
            if crosses_into(scope_root, root) {
                continue;
            }
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
        self.mint_connect_code_with_ttl(api_key, key, None).await
    }

    /// [`App::mint_connect_code`] with a lifetime: `None` is the ten-minute
    /// default, and `Some(seconds)` is one second to
    /// [`CONNECT_CODE_TTL_MAX_SECS`], outside which it is refused by name
    /// ([`AppError::ConnectCodeLifetime`]). A lifetime past the default is
    /// minted only for a key confined to one workarea (`oauth.connect_refused`,
    /// `not_confined`; ADR-0067 d3).
    pub async fn mint_connect_code_with_ttl(
        &self,
        api_key: &str,
        key: Reference,
        ttl_secs: Option<i64>,
    ) -> Result<ConnectCodeIssued, AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "mint_connect_code")?;
        self.mint_connect_code_by_with_ttl(&admin, key, ttl_secs)
            .await
    }

    /// [`App::mint_connect_code`] once the issuer is known: an admin key, or
    /// a person's `<user>-web` session key for a key naming its user, which
    /// the web use case has checked (ADR-0045 d9, d16).
    pub(crate) async fn mint_connect_code_by(
        &self,
        admin: &KeyRecord,
        key: Reference,
    ) -> Result<ConnectCodeIssued, AppError> {
        self.mint_connect_code_by_with_ttl(admin, key, None).await
    }

    async fn mint_connect_code_by_with_ttl(
        &self,
        admin: &KeyRecord,
        key: Reference,
        ttl_secs: Option<i64>,
    ) -> Result<ConnectCodeIssued, AppError> {
        let ttl_secs = match ttl_secs {
            None => CONNECT_CODE_TTL_SECS,
            Some(seconds) if (1..=CONNECT_CODE_TTL_MAX_SECS).contains(&seconds) => seconds,
            Some(seconds) => {
                return Err(AppError::ConnectCodeLifetime {
                    seconds,
                    max: CONNECT_CODE_TTL_MAX_SECS,
                });
            }
        };
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
        if ttl_secs > CONNECT_CODE_TTL_SECS {
            self.require_confined(&target).await?;
        }
        let voids = self
            .journal
            .connect_codes(tenant_id, key_id)
            .await?
            .into_iter()
            .find(|c| c.redeemed_at.is_none() && c.voided_seq.is_none())
            .map(|c| c.code_id);
        let (code, shown) = self.new_connect_code(oauth, key_id, ttl_secs);
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

    /// Whether `target` is confined to one workarea: no user root, and every
    /// grant on one node that is not the universe. A connect code that lives
    /// longer than ten minutes is a standing credential, so it is minted only
    /// for a key whose reach is that one node's subtree (ADR-0067 d3). The
    /// roles that cannot hold a connection at all, admin and operator, never
    /// get here.
    async fn require_confined(&self, target: &KeyRecord) -> Result<(), AppError> {
        let not_confined = || connect_refused(ConnectRefused::NotConfined);
        if target.user.is_some() {
            return Err(not_confined());
        }
        let access = self
            .journal
            .key_access(target.tenant_id, target.key_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let universe = self.universe(target.tenant_id).await?;
        let mut nodes = access.grants.iter().map(|g| g.grant.node);
        let Some(first) = nodes.next() else {
            return Err(not_confined());
        };
        if first == universe || nodes.any(|node| node != first) {
            return Err(not_confined());
        }
        Ok(())
    }

    /// The one connect code of a key that can still open a connection (not
    /// redeemed, not voided, not lapsed), by id or name: admin key role only.
    /// `extended` is true when it outlives the ten-minute default, so a
    /// reviewer code is named on `oml keys show` (ADR-0067 d3). The code
    /// itself is never returned; none is stored.
    pub async fn live_connect_code(
        &self,
        api_key: &str,
        key: Reference,
    ) -> Result<Option<LiveConnectCode>, AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "live_connect_code")?;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        if self.journal.key(admin.tenant_id, key_id).await?.is_none() {
            return Err(AppError::NotFound);
        }
        let now = self.clock.now().unix_micros();
        let horizon = now + CONNECT_CODE_TTL_SECS * 1_000_000;
        Ok(self
            .journal
            .connect_codes(admin.tenant_id, key_id)
            .await?
            .into_iter()
            .find(|c| {
                c.redeemed_at.is_none()
                    && c.voided_seq.is_none()
                    && c.expires_at.unix_micros() > now
            })
            .map(|c| LiveConnectCode {
                code_id: c.code_id,
                extended: c.expires_at.unix_micros() > horizon,
                expires_at: c.expires_at,
            }))
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
        // A new connection, not a replacement, counts against the person's
        // connections quota (ADR-0068 d5).
        if replaces.is_none() {
            self.require_quota(key, oml_domain::signup::Quota::Connections)
                .await?;
        }
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
