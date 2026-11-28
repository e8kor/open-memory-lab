//! The use-cases: every verb the transports will front, enforcing the
//! constitution here — verb ceilings (R1), reviewer-never-author (R2),
//! promoted-only reads (R3) — never in an adapter (ADR-0019 d1). Every
//! workspace-scoped verb resolves effective access from the key's grants and
//! denials on each request, in the one refusal order of roles-and-keys.md
//! (ADR-0025 d1).

mod admin_acts;
mod against_admin;
mod auth;
pub mod busy;
pub use against_admin::{Freeze, OPERATOR_PRINCIPAL, OPERATOR_SETTLE_AFTER_US};
mod emails;
mod exports;
mod federation;
mod fixes;
mod imports;
mod investigations;
mod key_limits;
mod linked_keys;
mod maintenance;
mod metering;
mod notices;
mod pages;
mod public;
mod rescan;
mod revocation;
mod rotation;
mod search;
mod signup;
mod sparse;
mod uploads;
mod usage;
mod widen;
pub use auth::{Callback, CallbackRefusal, ContestGrant, WEB_SESSION_TTL_SECS};
pub use exports::{ExportCounts, ExportRequest, Exported};
pub use federation::WideKey;
pub use fixes::{
    FIX_REASON_MAX_BYTES, FixOutcome, FixRequest, FixStats, ReviewedFix, SERVER_ISSUER_PRINCIPAL,
    SERVER_KEY_PRINCIPAL, server_issuer_id,
};
pub use imports::{
    ImportAnswer, ImportFinished, ImportJob, ImportPlan, ImportRequest, ImportStart,
};
pub use investigations::{INVESTIGATION_OPEN_LONG_SECONDS, INVESTIGATION_REASON_MAX_BYTES};
pub use metering::{AccountLimits, UsageDriftReport};
pub use pages::{
    ActClaim, ActTicket, AdminMemory, AdminPage, AdminSearchRequest, Capacity, CapacityGranted,
    OwnKey, OwnKeyShape, QueueRow, REGION_COUNT_MAX, Region, Regions, VerdictTrail, WebListRequest,
    WorkspaceOffer, WorkspaceOffers,
};
pub use public::{PublicNodeStats, PublicStats};
pub use rescan::{RESCAN_MAX_EVENTS, RescanHit, RescanReport};
pub use search::SearchSettings;
pub use signup::{
    DRAWS_MAX, PendingEmail, Person, PersonAsk, SignupState, StartStatus, UserDisabled, UserRow,
};
pub use sparse::{SPARSE_BACKFILL_STEP, SparsePass, encode_sparse_pass};
pub use uploads::{UploadConfig, UploadSweep};
pub use usage::UsageSettings;
pub use widen::{
    WIDEN_SIGNAL_BUILD_SECS, WIDEN_SIGNAL_MAX_AGE_SECS, WIDEN_SIGNAL_PAIRS_HELD,
    WIDEN_SIGNAL_REFRESH_SECS, WIDEN_SIGNAL_STATEMENT_SECS, WIDEN_SIGNAL_TENANT_PAIRS, WidenBuild,
    WidenLimits, WidenRefresh, drops_widen_signals,
};

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::access::{
    AccessRefusal, Covers, Grant, IssuanceRefusal, KeyDenial, LabelledGrant, TailCaller,
    TailSubject, Verb, VerbRefusal, admissible_grant_roles, check_contributor_grants,
    check_issuance, contributor_without_recall, effective_access, effective_roles, effective_verbs,
    recall_ancestries_needed, resolve, resolve_verb, tail_visible,
};
use oml_domain::anchor::{self as anchor_grammar, Anchor, SourceCheckBody};
use oml_domain::brief::{brief_cut, brief_order};
use oml_domain::budget::{budget_tokens, cut};
use oml_domain::claim_edges::{
    LabelRefusal, Ratifier, RatifyRefusal, check_label, check_ratifier, check_term_curies,
};
use oml_domain::guardrail::{
    self, Effect, GuardrailScope, LOAD_WARNING, OWN_HELD_MAX_LINES, SET_CAP, Severity, conflicts,
    guardrail_order, set_digest,
};
use oml_domain::in_progress::{
    self as in_progress, CuratorAct, CuratorRefusal, InProgressReason, KeyProfile, WorkspaceMode,
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
    ChainHead, DERIVED_CLAIM_KEY_PREFIX, Envelope, EventDraft, EventTime, FrozenOp, KeySet,
    LadderNode, LadderRefusal, LineageClosure, MemoryKind, MemoryStatus, Role, Sha256Hash,
    SignedEnvelope, UsedBy, Withdrawer, WorkspaceStatus, canonical_bytes, check_approval_class,
    check_reviewer, derive_claim_key, event_types, frozen_refuses, is_person_reviewer,
    next_envelope, nfc, propose_check, ratifier_is_proposer, ratify_check, same_user,
    sha256_digest_string, transition, withdraw,
};

use crate::cursor::SkillCursor;
use crate::cursor::{ClaimEdgeCursor, ListingCursor, NameCursor, PathCursor};
use crate::ports::{
    AdmissionEnvelope, ClientDocumentFetcher, Clock, CommitReceipt, CommitRequest, ConnectionStore,
    DocumentFetchFailure, HandleSealer, IdGen, IdempotencyPut, IdempotencyRecord, IdempotencyScope,
    JournalStore, MetadataFold, NoObserver, OutboxRow, Parsed, RequestObserver, Reranker,
    ScanFinding, ScanOutcome, Scanner, Signer, SparseEncoder, StoreError, SubjectRow, TailEvent,
};
use crate::telemetry::{HITS_STORED, Label, RecallEvent, Remembered, Telemetry, TelemetryItem};
use crate::types::AdminActRefusal;
use crate::types::{
    AdminTodoCounts, BULK_VERDICT_MAX, BriefProposal, BriefProposalPage, BulkItem,
    BulkVerdictRequest, FindingVerdict, FoundBy, HELD_PAGE_MAX, HeldFilter, HeldItem,
    HeldListingRequest, HeldPage, ReviewLane, TODO_COUNT_CAP, TODO_SCAN_ROWS_MAX, TodoCounts,
    VersionFinding,
};
use crate::types::{
    AppError, Approval, AssignedNames, AuthorizationRecord, AuthorizeOutcome, AuthorizePage,
    AuthorizeParams, Bearer, BootstrapPerson, BootstrapRequest, BootstrapResult, Brief,
    BriefEdgeListed, BriefEdgePage, BriefEdgeProposalOutcome, BriefEdgeRatification,
    BriefEdgeRecord, BriefEdgeView, ClaimEdgeFilter, ClaimEdgePage, ClaimEdgePosition,
    ClaimEdgeProposal, ClaimEdgeProposalOutcome, ClaimEdgeQuery, ClaimEdgeRatification,
    ClaimEdgeRecord, ClaimEdgeSelection, ClientAuthentication, ClientDescription, ClientKind,
    CodeEntryOutcome, ConfirmAction, ConfirmOutcome, ConfirmationGrant, ConfirmationView,
    ConnectCode, ConnectCodeIssued, ConnectCodeRecord, ConnectGrant, ConnectOutcome,
    ConnectionRecord, CreatedWorkspace, Credential, CrewCreateRequest, CrewPage, CrewRecord,
    CrewUpdate, DEFAULT_HIT_KINDS, DenialRecord, EdgeRatification, ExchangeForm, FormatFields,
    GrantRecord, GuardrailFields, GuardrailRequest, GuardrailSet, GuardrailVerdict, HIT_KINDS,
    HitPassage, INSTRUCTION_HIT_KINDS, IssueKeyRequest, IssuedKey, IssuedTokens, KeyAccess,
    KeyDetail, KeyRecord, KindFields, ListRequest, ListingFilter, ListingPosition, ListingScope,
    LiveConnectCode, MemoryPage, MemoryRecord, MintedToken, NamedRef, NamingRefusal,
    OAuthClientRecord, OnSimilar, OwnHeldGuardrail, PROVIDER_TENANT, ProposeBriefEdge,
    ProposeClaimEdge, ProposedBy, RecallHit, RecallRequest, RecallResult, RefreshFacts,
    RefreshForm, RegisteredClient, RerankOutcome, SIMILAR_KINDS, ServedGuardrail, ServedMemory,
    SimilarItem, SkillHit, SourceCheck, SourceCheckReceipt, SourceCheckRequest, StateClause,
    SubmitOutcome, SubmitReceipt, SubmitRequest, SubmitVerb, SupersedeOutcome, SupersedeReceipt,
    SupersedeRequest, TailPage, TailedEvent, TenantRow, TokenPair, TransitionRecord,
    TransitionState, TransitionView, TreeNode, TreePage, UnnamedRows, Verdict, VerdictOutcome,
    VerdictRequest, VersionRef, WebEntry, Whoami, WithdrawReceipt, WithdrawRequest, WorkspaceInfo,
    WorkspaceRecord, WorkspaceStatusView,
};
use crate::types::{CatalogMark, KindCount, TimeboxRefusal};
use crate::types::{Preview, PreviewFacts, PreviewRequest, SkillShadow};
use crate::types::{ServedSkill, SkillBody, SkillRead, SkillRequest, SkillSet};
use oml_domain::skills::{SKILL_PAGE, SkillCandidate, offer, skill_uri};

use crate::search::{
    Access, CandidateScope, DID_YOU_MEAN_BELOW, Fused, Fusion, MatchMode, OwnHeldScope, Pipeline,
    RetrievalQuery, RetrieverKind, SearchConfig, Stage, TIME_DEPTH, Timings, apply_floor,
    collapse_passages, fuse_capped, order_time_list, pipeline_access, pruned_words, query_terms,
    rerank_order,
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

/// The instructions a recall ranks at its scope node (ADR-0092 d3): the
/// offered skills and the applicable guardrails by version id and kind, in
/// the states their own reads serve them, and each skill hit's command and
/// resource.
#[derive(Debug, Default)]
struct RecallInstructions {
    ids: Vec<(Uuid, MemoryKind)>,
    skills: BTreeMap<Uuid, SkillHit>,
}

const EVENTS_PAGE: usize = 512;

/// The longest `issued_by` chain [`App::counted_person_of`] walks from a
/// user key before it counts the key as no one: issuance runs from a
/// session or tenant key down a few user keys at most, so a longer chain
/// fails closed rather than walking on.
const COUNTED_PERSON_DEPTH: usize = 32;

/// The review policy in force: the one PoC constant `{k: 1,
/// required_roles: ["reviewer"]}`. A verdict's ratification and a status
/// transition's `policy_snapshot` both read it here, so the two cannot
/// disagree (admission.md review and ratification; workspaces.md, the policy
/// snapshot is the policy in force plus the precondition; ADR-0009 d3).
fn review_policy() -> Value {
    json!({ "k": 1, "required_roles": ["reviewer"] })
}

/// The policy a claim's ratification snapshots: [`review_policy`] with the
/// class whose rule the verdict met and whether only a person's verdict
/// counts on it (ADR-0080 d16). A successor that replaces a version of
/// another kind meets the stricter of the two classes.
fn claim_review_policy(kind: MemoryKind, replaced: Option<MemoryKind>) -> Value {
    let class = match replaced.map(MemoryKind::class) {
        Some(other) if other.needs_person() => other,
        _ => kind.class(),
    };
    let mut policy = review_policy();
    policy["class"] = json!(class.as_str());
    policy["human_required"] = json!(class.needs_person());
    policy
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
/// to one workspace (ADR-0067 d3).
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
    /// `server.shared`: the operator says the server's admins are not its
    /// callers', which whoami reports so `oml sync` asks consent
    /// (ADR-0076 d9).
    shared: bool,
    /// Wrong connect-code entries since the process started, deployment
    /// wide and never per tenant, for doctor (ADR-0031 d8, d14).
    connect_code_failures: AtomicU64,
    /// Told what each request resolved, for its log line; it decides
    /// nothing (ADR-0042 d2, d14).
    observer: Arc<dyn RequestObserver>,
    /// `brief.max_tokens`: the budget a workspace's brief is cut to
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
    /// The recall meter's queue; `None` where nothing writes one, and then
    /// a recall counts itself through the store before it answers, as the
    /// in-memory store's cost nothing (ADR-0071 d2, d8).
    recall_meter: Option<Arc<dyn crate::ports::RecallMeter>>,
    /// Each key's feedback members checked today, by UTC day: the per-key
    /// cap, held before any lookup (ADR-0054 d6, as amended).
    feedback_counts: std::sync::Mutex<BTreeMap<Uuid, (i64, u64)>>,
    /// Each key's requests in its UTC minute under the key's own rate, for
    /// the keys that carry one: counted in this process alone, forgotten at
    /// a restart (ADR-0079 d4, d6).
    key_windows: std::sync::Mutex<BTreeMap<Uuid, (i64, u64)>>,
    /// Each key's UTC day number this process last wrote its
    /// authentication row for: a key's row is written at most once a day,
    /// that day's later authentications coalesced here (ADR-0097, answer
    /// 24). Forgotten at a restart, which writes the day's row once more.
    authenticated_days: std::sync::Mutex<BTreeMap<Uuid, i64>>,
    /// Authentication rows this process could not write since it started:
    /// doctor's `auth.log_write_failed` (answer 24). The key's next
    /// authentication writes the row again.
    authentication_write_failures: AtomicU64,
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
    /// The widen hint's signals, built off the request path, and the ones
    /// recalls asked for (ADR-0072 d8, as built by #244).
    widen: widen::WidenSignals,
    /// The deployment's quota profiles (ADR-0071 d5).
    profiles: Arc<crate::types::QuotaProfiles>,
    /// The upload sessions' object store and limits (ADR-0004 d3, as
    /// amended 2026-11-17); no store until the composition gives one, and
    /// then every upload answers `upload.storage_failed`.
    uploads: uploads::UploadConfig,
    /// One import or dry run per tenant and two in the process (ADR-0084
    /// I13).
    imports_busy: busy::BusyGate,
    /// One export per tenant and two in the process (ADR-0084 I13).
    exports_busy: busy::BusyGate,
}

/// The authenticated key and what its effective access is computed from,
/// read once per request and never cached (workspaces.md effective access;
/// ADR-0025 d1, d14). A key value and an access token of a live connection
/// each resolve to one, the same for one key (ADR-0031 d11).
#[derive(Debug)]
pub struct Caller {
    key: KeyRecord,
    grants: Vec<Grant>,
    denials: Vec<KeyDenial>,
    /// The key's grants and, when its role reads, a reader grant on each
    /// public node alone that the key does not hold: what every effective
    /// access check reads (ADR-0098 d1). `grants` alone is what the key
    /// holds and is served.
    reach: Vec<Grant>,
    /// The tenant's public nodes this key's role reads, before any global
    /// deny; empty for a key whose role does not read (ADR-0098 d2).
    public: Vec<Uuid>,
    /// The epoch of the access read above (workspaces.md permission epoch).
    permission_epoch: u64,
    /// The own key set when it is wider than the key itself: a person's
    /// `<user>-web` session key owns every key naming its user (ADR-0045
    /// d16, amending ADR-0025 d3). Empty means the key alone.
    own_keys: Vec<Uuid>,
    /// The access the session gate read for this request, held for the
    /// request's first resolution of the caller, which takes it in place
    /// of reading the same rows again; `None` on every other caller and
    /// once taken, so a later resolution reads them now (ADR-0070 d8,
    /// #297).
    gate_read: std::sync::Mutex<Option<KeyAccess>>,
    /// Whether the value that authenticated this caller is its key's
    /// previous one, inside a rotation's grace: such a caller reads and
    /// writes as the key does and runs none of the key acts (ADR-0096 d13,
    /// the operator's decision of 2026-11-25).
    previous_value: bool,
    /// For a session key's caller, the session's opening and its entry's
    /// email digest: an acceptance records the provider subject and the
    /// session's age from them (ADR-0097 d7).
    session: Option<(EventTime, Sha256Hash)>,
}

impl Caller {
    /// The caller `key` acts as under `access` as stored, `None` for a
    /// provider-level key. Such a key carries no grant and no denial,
    /// since no tenant's row can name it (postgres-schema.md): it holds no
    /// access, so every verb refuses it by its role (ADR-0025 d13).
    fn of(key: KeyRecord, access: Option<KeyAccess>) -> Self {
        let Some(access) = access else {
            return Caller {
                key,
                grants: Vec::new(),
                denials: Vec::new(),
                reach: Vec::new(),
                public: Vec::new(),
                permission_epoch: 0,
                own_keys: Vec::new(),
                gate_read: std::sync::Mutex::new(None),
                previous_value: false,
                session: None,
            };
        };
        let permission_epoch = access.permission_epoch();
        let grants: Vec<Grant> = access.grants.into_iter().map(|g| g.grant).collect();
        // The public reach, for a key whose role reads (ADR-0098 d1, d2).
        let public = if oml_domain::access::reads_public(key.role) {
            access.public_nodes
        } else {
            Vec::new()
        };
        let reach = grants
            .iter()
            .cloned()
            .chain(
                public
                    .iter()
                    .map(|n| oml_domain::access::public_reach_grant(*n)),
            )
            .collect();
        Caller {
            permission_epoch,
            grants,
            reach,
            public,
            key,
            denials: access.denials.into_iter().map(|d| d.denial).collect(),
            own_keys: Vec::new(),
            gate_read: std::sync::Mutex::new(None),
            previous_value: false,
            session: None,
        }
    }

    /// The access the session gate read for this request, once: the first
    /// resolution of the caller takes it, and every later one finds none.
    fn take_gate_read(&self) -> Option<KeyAccess> {
        self.gate_read.lock().expect("unpoisoned").take()
    }

    /// The key the request acts as.
    #[must_use]
    pub fn key(&self) -> &KeyRecord {
        &self.key
    }

    /// Refuses an act that is not a memory read or write to a caller that
    /// authenticated with its key's previous value: the grace keeps a
    /// deployed value working and lets no one holding it rotate the key
    /// past the grace, run an admin verb or change who holds what
    /// ([`AppError::PreviousKeyValue`]; ADR-0096 d13).
    ///
    /// # Errors
    /// [`AppError::PreviousKeyValue`] naming `verb`.
    pub fn require_current_value(&self, verb: &'static str) -> Result<(), AppError> {
        if self.previous_value {
            Err(AppError::PreviousKeyValue { verb })
        } else {
            Ok(())
        }
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

    /// The epoch of the access read (workspaces.md permission epoch).
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

    /// `effective_roles` at the node whose ancestry, node to root, this is,
    /// over the key's grants and its public reach (ADR-0098 d1).
    fn effective(&self, ancestry: &[Uuid]) -> BTreeSet<Role> {
        effective_roles(&self.reach, &self.denials, ancestry)
    }

    /// Whether a global per-key denial covers the node: deny-wins, own rows
    /// included (roles-and-keys.md#own).
    fn denied(&self, ancestry: &[Uuid]) -> bool {
        self.denials.iter().any(|d| ancestry.contains(&d.node))
    }
}

/// The served node set of a scoped read (ADR-0030 d2), and the nodes of it
/// inside an in-progress subtree, the scope's own or one below it, where
/// every author's held rows are served (d3; ADR-0080 d16).
struct ServedNodes {
    nodes: Vec<Uuid>,
    inside: Vec<Uuid>,
}

/// The scope of an offered skill set: the node, its path, the key's
/// guardrail scope, and the name of each workspace a candidate lives in,
/// which a resource URI carries (ADR-0081 d6).
struct SkillScope {
    scope_node: Uuid,
    scope_path: String,
    guardrail_scope: GuardrailScope,
    names: BTreeMap<Uuid, String>,
}

/// An offered command as the read serves it, without its body (ADR-0080
/// d11); `None` for a row with no skill fields, which no candidate has.
fn served_skill(
    scope: &SkillScope,
    offered: &oml_domain::skills::OfferedSkill,
    row: &MemoryRecord,
) -> Option<ServedSkill> {
    let Some(KindFields::Skill { name, description }) = row.kind_fields.as_deref() else {
        return None;
    };
    let workspace_name = scope.names.get(&row.workspace_id)?.clone();
    let held = offered.candidate.held;
    Some(ServedSkill {
        artifact_id: row.memory_id,
        claim_key: row.claim_key.clone(),
        name: offered.command.clone(),
        skill_name: name.clone(),
        description: description.clone(),
        version: row.version,
        workspace_id: row.workspace_id,
        uri: skill_uri(&workspace_name, &offered.command),
        workspace_name,
        workspace_path: offered.candidate.path.clone(),
        depth: offered.candidate.distance,
        overrides: offered.overrides.clone(),
        approved_at: if held {
            None
        } else {
            row.approval.as_ref().map(|a| a.at.clone())
        },
        approver: if held {
            None
        } else {
            row.approval.as_deref().cloned()
        },
        unreviewed: held,
        body_bytes: row.body_bytes,
    })
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

impl ScanTarget<'_> {
    /// The field a finding on this text is stored under (ADR-0074 d23):
    /// `/body`, `/title`, or `/body/` and a decoded view's path, its dots
    /// as slashes.
    pub(crate) fn field(&self) -> String {
        match self.at {
            None => "/body".to_owned(),
            Some("title") => "/title".to_owned(),
            Some(path) => format!("/body/{}", path.replace('.', "/")),
        }
    }
}

/// A scanner's finding as the findings list stores it (ADR-0074 d23).
pub(crate) fn version_finding(
    f: &ScanFinding,
    field: String,
    rule_set: Option<&str>,
    found_by: FoundBy,
) -> VersionFinding {
    VersionFinding {
        class: f.class.clone(),
        rule: f.rule.clone(),
        verdict: if f.veto {
            FindingVerdict::Refuse
        } else {
            FindingVerdict::Hold
        },
        rule_set: rule_set.map(str::to_owned),
        field: Some(field),
        line: f.place.as_ref().map(|p| p.line),
        via: f.place.as_ref().map(|p| p.via.clone()),
        span: f.place.as_ref().map(|p| p.span),
        found_by,
        event_seq: None,
        migrated_from: None,
    }
}

/// The passage `ordinal` of a document version, cut again by the one rule
/// (ADR-0081 d3, d4); `None` for a whole-version unit, ordinal -1, or an
/// ordinal the body no longer cuts.
fn passage_unit(m: &MemoryRecord, ordinal: i32) -> Option<crate::types::PassageUnit> {
    let ordinal = usize::try_from(ordinal).ok()?;
    m.passage_units()
        .into_iter()
        .find(|u| u.passage.ordinal == ordinal)
}

/// Whether a held version may be served unreviewed inside an in-progress
/// subtree: a version of any kind (ADR-0080 d16, amending ADR-0030 d3),
/// never a retirement version, never one the scan held, which stays its
/// author's until a reviewer reads it, and only one last scanned under
/// `current`, the server's rule set: a version an older rule set passed
/// stays its author's until a rescan re-evaluates it (ADR-0030 d3; ADR-0065
/// d15; ADR-0074 d20).
fn served_unreviewed(m: &MemoryRecord, current: Option<&str>) -> bool {
    m.status == MemoryStatus::Held
        && !m.retirement
        && m.scan_finding.is_none()
        && m.scanned_under.as_deref() == current
}

/// Whether an offered instruction's row, as the recall hydrated it, is still
/// served: a promoted tip, or a held version its read serves (unscanned
/// rows and scan holds never). `recall_instructions` admitted the offered
/// set by state, and a verdict, a withdrawal or a supersession can land
/// before the pipeline reads the row again, so the state is read again
/// here (ADR-0092 d3; ADR-0080 d10, d14).
fn offered_instruction_still_served(m: &MemoryRecord, current_scanner: Option<&str>) -> bool {
    match m.status {
        MemoryStatus::Promoted => m.is_tip,
        MemoryStatus::Held => served_unreviewed(m, current_scanner),
        _ => false,
    }
}

/// The most held steering versions a recall's steering list carries after
/// its promoted rows (ADR-0080 d10, d14): a bound on the two listing reads
/// that find them, beside the steering budget that cuts the list.
const HELD_STEERING_MAX: usize = 50;

/// The most held guardrail versions the guardrail read serves beside the
/// promoted rules, dropped before any promoted rule is (ADR-0080 d10).
const HELD_GUARDRAILS_MAX: usize = 50;

/// How an issuance runs once its issuer is known (ADR-0045 d6, d16).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Issuance<'a> {
    /// An admin or operator key's issuance of a bearer or connector key,
    /// with the admin's reason: an admin's is a widening admin act
    /// (ADR-0097 d5); with a time box, taken at once (d13).
    Plain(Option<&'a str>, Option<&'a Timebox>),
    /// A session key for the allowlist user named: no value.
    Session(&'a Name),
    /// A person's `<user>-web` issuing a key in its own root, sealed on the
    /// chain head read before that key's access ([`Head::BeforeAccess`]).
    PersonsOwn(&'a Option<SignedEnvelope>),
}

/// A time box an issuance carries (ADR-0097 d13): the workspaces its
/// grants name, and whether it renews an earlier box there.
#[derive(Debug)]
pub(crate) struct Timebox {
    pub nodes: Vec<String>,
    pub renewal: bool,
}

/// The chain head an act seals its commit on.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Head<'a> {
    /// Read where the act seals, after its lookups.
    AtSeal,
    /// Read by the caller before it read the acting key's access: a denial
    /// of that key committed after the read moves the head, so the commit
    /// conflicts and the retry reads the denial (ADR-0070 d2, d8; #325).
    BeforeAccess(&'a Option<SignedEnvelope>),
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

/// The grants a key of `role` takes by default on a workspace its issuance
/// creates, covering the subtree: a contributor reader and submitter, a
/// submitter submitter, a reader reader, a reviewer reviewer; none for an
/// admin or operator key, which carries no grant (ADR-0097 d30).
fn default_grant_roles(role: Role) -> &'static [Role] {
    match role {
        Role::Contributor => &[Role::Reader, Role::Submitter],
        Role::Submitter => &[Role::Submitter],
        Role::Reader => &[Role::Reader],
        Role::Reviewer => &[Role::Reviewer],
        _ => &[],
    }
}

/// An admin verb: checked on the key's own role, never on grants
/// (roles-and-keys.md permission matrix; http-api.md authorization), and
/// refused to a caller that authenticated with its key's previous value.
/// This is the one place that rule lives for every admin verb: a previous
/// value inside its grace reads and writes memories as the key does and
/// runs no other act (ADR-0096 d13, its review round's amendment of
/// 2026-11-25).
/// A `maintenance` key reaches only the counts its verbs list (ADR-0097
/// d18): anywhere else it is refused `access.verb_ceiling`, verb
/// `maintenance_counts_only`.
fn refuse_maintenance(key: &KeyRecord, admitted: bool) -> Result<(), AppError> {
    if key.role == Role::Maintenance && !admitted {
        return Err(AppError::VerbCeiling {
            role: Role::Maintenance,
            verb: "maintenance_counts_only",
        });
    }
    Ok(())
}

fn require_role(caller: &Caller, role: Role, verb: &'static str) -> Result<(), AppError> {
    if caller.key.role != role {
        return Err(AppError::VerbCeiling {
            role: caller.key.role,
            verb,
        });
    }
    caller.require_current_value(verb)
}

/// Step 1 of the refusal order: the key's role must admit the grant role the
/// verb needs. It reads no node, so it runs before the workspace resolves
/// (roles-and-keys.md grant admissibility and refusal order; ADR-0025 d1).
/// The verbs a key lists on a node: the verbs table over its effective
/// roles, and `remember` for a person's session reviewer key where it
/// holds reviewer and reader, the grants under which its submit is held,
/// outside its person's own root (ADR-0082 d10, d11, and d10's note of
/// 2026-11-19), so the verbs it lists are the verbs it may use. Two of
/// `oml_write`'s modes are listed where their checks admit the key
/// (ADR-0095 d8): `create_workspace` for a contributor key naming a user,
/// on a node inside that user's root where it holds submitter (d3), and
/// `propose_brief` for a contributor key holding submitter or a reviewer
/// key holding reviewer (d6). `in_key_root` is whether the node lies in the
/// key's own user root.
fn listed_verbs(
    key: &KeyRecord,
    effective: &BTreeSet<Role>,
    in_own_root: bool,
    in_key_root: bool,
) -> Vec<&'static str> {
    let mut verbs = effective_verbs(effective, key.used_by);
    if session_reviewer_starts_claims(key)
        && !in_own_root
        && effective.contains(&Role::Reviewer)
        && effective.contains(&Role::Reader)
        && !verbs.contains(&"remember")
    {
        verbs.push("remember");
    }
    let submitter = effective.contains(&Role::Submitter);
    if key.role == Role::Contributor && key.user.is_some() && in_key_root && submitter {
        verbs.push("create_workspace");
    }
    if (key.role == Role::Contributor && submitter)
        || (key.role == Role::Reviewer && effective.contains(&Role::Reviewer))
    {
        verbs.push("propose_brief");
    }
    verbs
}

/// Whether `key` is a person's session key whose role is reviewer, which
/// starts a held claim where it holds reviewer and reader (ADR-0082 d10).
fn session_reviewer_starts_claims(key: &KeyRecord) -> bool {
    key.credential == Credential::Session && key.role == Role::Reviewer
}

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

/// The role the refusal order reads for `needed`: a curator key's submitter
/// grants confer the submitter verbs, which a reviewer's ceiling would refuse
/// (ADR-0030 d8; ADR-0093 d6).
fn ceiling_role(key: &KeyRecord, needed: Role) -> Role {
    if key.profile == Some(KeyProfile::Curator) && needed == Role::Submitter {
        Role::Submitter
    } else {
        key.role
    }
}

/// How many times one item of a bulk verdict retries after a chain-head
/// conflict: the single verdict route's retry count (ADR-0055 d4).
const BULK_CHAIN_RETRIES: usize = 16;

/// The listing and the bulk verdict admit a reviewer key a person holds,
/// and refuse every other key before any lookup (ADR-0055 d1): a contributor, submitter or
/// reader key (R1), a service key (ADR-0025 d1) and a reviewer key used by
/// an agent, whose crew reviews one serve at a time.
fn require_person_reviewer(key: &KeyRecord) -> Result<(), AppError> {
    if key.role == Role::Reviewer && key.used_by == UsedBy::Person {
        Ok(())
    } else {
        Err(AppError::BulkReviewHumanOnly {
            role: key.role,
            used_by: key.used_by,
        })
    }
}

/// The rows a review read serves `key` under `needs`: a key that cannot
/// approve an instruction (until #214, one that is not a reviewer key
/// `used_by: person` with no crew and no profile) reads only the rows any
/// reviewer may approve, and asking for the person lane it reads none,
/// `None`; any other key reads what it asks for, every row when it asks
/// nothing (ADR-0074 d19; ADR-0080 d16).
fn review_lane(key: &KeyRecord, needs: Option<ReviewLane>) -> Option<ReviewLane> {
    let person = is_person_reviewer(
        key.role,
        key.used_by,
        key.crew_id.is_some(),
        key.profile.is_some(),
    );
    match (needs, person) {
        (Some(ReviewLane::Person), false) => None,
        (_, false) => Some(ReviewLane::Reviewer),
        (needs, true) => Some(needs.unwrap_or_default()),
    }
}

/// A `used_by: service` key is refused every review-class verb before any
/// lookup: its reviewer grant confers them like any other, and this rule, not
/// the grant, keeps it out of review (roles-and-keys.md; ADR-0025 d1).
fn refuse_service(key: &KeyRecord, verb: &'static str) -> Result<(), AppError> {
    if key.used_by == UsedBy::Service {
        Err(AppError::VerbCeiling {
            role: key.role,
            verb,
        })
    } else {
        Ok(())
    }
}

/// A refusal-order answer as the typed refusal; `node` is the workspace
/// `access.grant_refused` names.
fn access_refusal(refusal: AccessRefusal, node: Uuid, verb: &'static str) -> AppError {
    match refusal {
        AccessRefusal::VerbCeiling { role, .. } => AppError::VerbCeiling { role, verb },
        AccessRefusal::GrantRefused { needed_role } => AppError::GrantRefused {
            workspace_id: node,
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
        IssuanceRefusal::ContributorGrants => AppError::ContributorGrants,
    }
}

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 256 bits as exactly 43 base62 characters, zero-padded (roles-and-keys.md
/// key format: `oml_` + 43 × `[A-Za-z0-9]`).
pub(crate) fn base62_43(bytes: &[u8; 32]) -> String {
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

/// The ancestry of `node` as `workspace_ancestry` serves it, the node then
/// each parent in turn to the root, from records already read; `None` where
/// they do not reach the root, and the caller reads it instead.
fn ancestry_of(
    walked: &BTreeMap<Uuid, WorkspaceRecord>,
    node: Uuid,
) -> Option<Vec<WorkspaceRecord>> {
    let mut ancestry = Vec::new();
    let mut at = Some(node);
    while let Some(id) = at {
        let record = walked.get(&id)?;
        // A cycle is no ancestry; the store's walk decides it.
        if ancestry.len() > walked.len() {
            return None;
        }
        at = record.parent_id;
        ancestry.push(record.clone());
    }
    Some(ancestry)
}

/// A node's derived path from its ancestry, listed node to root: the names
/// from `universe` down to the node, joined with `.` (ADR-0025 d14). Every
/// path the app serves is this derivation.
fn derived_path(ancestry: &[WorkspaceRecord]) -> String {
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
    workspace_id: Uuid,
) -> String {
    let doc = json!({
        "body": req.body,
        "claim_key": claim_key,
        "verb": verb.as_str(),
        "kind": req.kind.as_str(),
        "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "workspace_id": workspace_id.to_string(),
    });
    let doc = with_guardrail_fields(doc, req.severity, req.rule.as_ref());
    let mut doc = with_kind_fields(doc, req.kind_fields.as_ref());
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

/// The name of a person's chat workspace under their user root (ADR-0063
/// d8).
pub const CHAT_WORKSPACE: &str = "chat";

/// A `maintenance` key's request rate when its issuance sends none, and the
/// rate a converted admin key takes: 60 a minute, a placeholder, chosen,
/// not measured (ADR-0097 d18).
pub const MAINTENANCE_REQUESTS_PER_MINUTE: u32 = 60;

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

/// The media type a kind takes when a request names none: `text/markdown`
/// for a skill and a document (ADR-0080 d1; ADR-0081 d1), `text/plain`
/// for every other kind (ADR-0056 d1).
fn default_format(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Skill | MemoryKind::Document => "text/markdown",
        _ => "text/plain",
    }
}

/// A skill's, a document's or a test case's fields as admitted, and the
/// claim key the kind derives, checked as ADR-0080 d1, d9, ADR-0081 d1, d2
/// and ADR-0086 d1 state: the object is refused on any other kind and on a
/// retirement, and required where the kind requires it; a skill's key is
/// `skill/<name>` and a successor keeps its name; a document's is sent
/// under `doc/`'s grammar or derived from its title; a test case's is sent
/// as `test/<name>`. `claim_key` is the key a submission sent, or a
/// successor's target's. `(None, None)` for a version of any other kind.
fn kind_fields(
    kind: MemoryKind,
    retirement: bool,
    successor: bool,
    sent: Option<&KindFields>,
    claim_key: Option<&str>,
    body: &str,
    media_type: &str,
) -> Result<(Option<KindFields>, Option<String>), AppError> {
    use oml_domain::claim_kinds::{self as ck, FieldRefusal};
    let refuse = |field: &'static str, reason: &'static str| {
        AppError::KindField(FieldRefusal { field, reason })
    };
    let field = |e: FieldRefusal| AppError::KindField(e);
    if let Some(sent) = sent
        && (sent.kind() != kind || retirement)
    {
        return Err(refuse(
            match sent {
                KindFields::Skill { .. } => "/skill",
                KindFields::Document { .. } => "/document",
                KindFields::Test { .. } => "/test",
            },
            "this object is its kind's field, refused on any other kind and on a retirement",
        ));
    }
    if retirement {
        return Ok((None, None));
    }
    match kind {
        MemoryKind::Skill => {
            let Some(KindFields::Skill { name, description }) = sent else {
                return Err(refuse(
                    "/skill",
                    "a skill carries skill {name, description}",
                ));
            };
            ck::check_skill_name(name).map_err(field)?;
            ck::check_skill_description(description).map_err(field)?;
            if media_type != "text/markdown" {
                return Err(refuse("/format", "a skill is text/markdown"));
            }
            ck::check_skill_body(body).map_err(field)?;
            if body.len() > ck::SKILL_BODY_MAX_BYTES {
                return Err(AppError::SkillBodyTooLarge);
            }
            let derived = ck::skill_claim_key(name);
            match claim_key {
                Some(sent) if sent != derived && successor => {
                    return Err(refuse(
                        "/skill/name",
                        "a skill successor keeps its name: a renamed skill retires the old claim and starts a new one",
                    ));
                }
                Some(sent) if sent != derived => {
                    return Err(refuse(
                        "/claim_key",
                        "a skill's claim key is skill/<name>, derived and never chosen",
                    ));
                }
                _ => {}
            }
            Ok((sent.cloned(), Some(derived)))
        }
        MemoryKind::Document => {
            if !matches!(media_type, "text/markdown" | "text/plain") {
                return Err(refuse(
                    "/format",
                    "a document is text/markdown or text/plain",
                ));
            }
            // The passage rule cuts at capture; a text past the cap is
            // refused before anything is written (ADR-0081 d3).
            let cut = if media_type == "text/markdown" {
                oml_passage::Format::Markdown
            } else {
                oml_passage::Format::Plain
            };
            oml_passage::passages(body, cut)
                .map_err(|e| AppError::TooManyPassages { count: e.count })?;
            let sent_title = match sent {
                Some(KindFields::Document { title: Some(t) }) => {
                    ck::check_document_title(t).map_err(field)?;
                    Some(t.clone())
                }
                _ => None,
            };
            let heading = (media_type == "text/markdown")
                .then(|| ck::markdown_title(body))
                .flatten();
            let key = match claim_key {
                Some(key) => {
                    ck::check_document_key(key).map_err(field)?;
                    key.to_owned()
                }
                None => {
                    let Some(from) = sent_title.as_deref().or(heading.as_deref()) else {
                        return Err(refuse(
                            "/claim_key",
                            "a document with no claim_key takes its key from its title: send document.title, a level-1 heading or a doc/ key",
                        ));
                    };
                    let key = ck::document_key_from_title(from);
                    ck::check_document_key(&key).map_err(field)?;
                    key
                }
            };
            let title = sent_title
                .or(heading)
                .unwrap_or_else(|| key.rsplit('/').next().unwrap_or_default().to_owned());
            Ok((Some(KindFields::Document { title: Some(title) }), Some(key)))
        }
        MemoryKind::Test => {
            let Some(KindFields::Test { .. }) = sent else {
                return Err(refuse("/test", "a test case carries test {shape}"));
            };
            ck::check_test_key(claim_key.unwrap_or_default()).map_err(field)?;
            Ok((sent.cloned(), claim_key.map(str::to_owned)))
        }
        _ => Ok((None, None)),
    }
}

/// A request digest's or a submission payload's document with the kind's
/// object under its member name, and unchanged otherwise, so every other
/// request digests as it did before the kinds (ADR-0080 d1).
fn with_kind_fields(mut doc: Value, fields: Option<&KindFields>) -> Value {
    if let (Some(fields), Some(members)) = (fields, doc.as_object_mut()) {
        members.insert(fields.member().to_owned(), fields.to_json());
    }
    doc
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
        *body = with_kind_fields(body.take(), req.kind_fields.as_ref());
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

/// What `set_workspace_mode` admits (ADR-0030 d1, d6): the node already in
/// the mode, which journals nothing, or a change with the mode it leaves
/// and, for a commit, the nodes it frees.
pub(crate) enum ModeAdmitted {
    Already(WorkspaceRecord),
    Change {
        node: WorkspaceRecord,
        current: WorkspaceMode,
        held_free: Vec<Uuid>,
    },
}

/// What a node is to the ladder, from its ancestry, the node first: the
/// universe root; a user root or any node under one, which stays
/// `development` (ADR-0044 d1; ADR-0085 d17); or an ordinary node.
fn ladder_node(ancestry: &[WorkspaceRecord]) -> LadderNode {
    match ancestry.first() {
        Some(node) if node.parent_id.is_none() => LadderNode::Universe,
        _ if ancestry.iter().any(|w| w.user_root) => LadderNode::UserRoot,
        _ => LadderNode::Ordinary,
    }
}

/// The in-progress root of a node from its ancestry, the node first
/// (ADR-0030 d1).
fn in_progress_root_in(ancestry: &[WorkspaceRecord]) -> Option<Uuid> {
    let marks: Vec<(Uuid, bool, bool)> = ancestry
        .iter()
        .map(|w| {
            (
                w.workspace_id,
                w.in_progress,
                w.status == WorkspaceStatus::Development,
            )
        })
        .collect();
    in_progress::in_progress_root_of(&marks)
}

/// The request digest of a claim-edge proposal: SHA-256 over the RFC 8785
/// JSON of the method, the route template, no path parameters and the body
/// as sent, `kind: claim` included, since ADR-0039 d9 merges the claim-edge
/// and workspace-edge families into `/v2/edges` (http-api.md idempotency).
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
/// `kind: brief`, the workspace as sent (http-api.md idempotency).
fn brief_edge_request_digest(req: &ProposeBriefEdge) -> Result<String, AppError> {
    let mut body = json!({
        "kind": "brief",
        "from_artifact_id": req.artifact_id.to_string(),
        "to_workspace_id": req.workspace.to_string(),
    });
    if let (Some(reason), Some(obj)) = (&req.reason, body.as_object_mut()) {
        obj.insert("reason".to_owned(), json!(reason));
    }
    let doc = json!({
        "method": "POST",
        "route": "/v2/edges",
        "path_params": {},
        "body": body,
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// A structure act's `reason` (ADR-0095 d1): scanned as a verdict's reason
/// is, a veto refusing the act before anything is signed, then journalled
/// in `payload` as `reason`. An act sent with none journals none, so the
/// payloads of acts that never carried one are unchanged.
fn with_reason<J, S, C, I, Sc>(
    app: &App<J, S, C, I, Sc>,
    tenant_id: Uuid,
    payload: &mut Value,
    reason: Option<&str>,
) -> Result<(), AppError>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    let Some(reason) = reason else {
        return Ok(());
    };
    app.scan_veto_only(tenant_id, "reason", reason)?;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("reason".to_owned(), json!(reason));
    }
    Ok(())
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
    scope_ancestry: &[WorkspaceRecord],
) -> Vec<Uuid> {
    // A scope inside a user root also searches `universe`'s own claims
    // where the key reads that node, ranked with the scope's own; own held
    // rows never include them (ADR-0044 d10).
    let mut hit_nodes = readable.to_vec();
    if scope_ancestry.iter().any(|w| w.user_root)
        && let Some(universe) = scope_ancestry.last().map(|w| w.workspace_id)
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
        // the list's workspace ids, so a `scope` recall digests as it did
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

/// An event as its readers read it (ADR-0091 d4): its type and payload
/// through the former-name map, every other signed field as stored. The
/// stored envelope is never replaced: verification reads the stored bytes,
/// and only interpretation reads this.
pub(crate) fn read_as_current(envelope: &Envelope) -> Result<Envelope, StoreError> {
    let read = oml_domain::former_names::read_event(&envelope.event_type, &envelope.payload)
        .map_err(|e| StoreError::Backend(format!("seq {}: {e}", envelope.seq)))?;
    Ok(Envelope {
        event_type: read.event_type,
        payload: read.payload,
        ..envelope.clone()
    })
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
        ceiling: None,
        written: None,
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

/// A crew object as a PoC `workspace.create` or `workspace.crew_updated`
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
            "workspace_ids": c.workspace_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// One chain's rows that predate names, read in journal order of creation
/// from the chain itself (http-api.md rows created before names).
struct PredatingChain {
    tenant_id: Uuid,
    /// The tenant's own row, named in the deployment's namespace.
    tenant: Option<UnnamedRow>,
    /// The root workspace, which rule 1 names `universe`.
    root: Option<Uuid>,
    /// The chain's workspaces and keys.
    rows: Vec<UnnamedRow>,
    /// Each workspace's current crew object, in journal order.
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
        let read = read_as_current(&event.envelope)?;
        let envelope = &read;
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
            event_types::WORKSPACE_CREATE => {
                if let Some(id) = payload_id(payload, "workspace_id")
                    && unnamed.workspaces.contains(&id)
                {
                    if payload.get("parent_id").is_none_or(Value::is_null) {
                        root = Some(id);
                    }
                    rows.push(UnnamedRow {
                        kind: NamedKind::Workspace,
                        id,
                        prior_name: payload_str(payload, "name"),
                    });
                    crews.push((id, crew_object(payload.get("crew"))));
                }
            }
            // The latest crew object of a workspace is its current one.
            event_types::WORKSPACE_CREW_UPDATED => {
                if let Some(id) = payload_id(payload, "workspace_id")
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
                .workspaces
                .iter()
                .find(|w| !created(NamedKind::Workspace, **w))
                .map(|w| (NamedKind::Workspace, *w))
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
            shared: false,
            connect_code_failures: AtomicU64::new(0),
            observer: Arc::new(NoObserver),
            brief_max_tokens: oml_domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
            search: SearchConfig::default(),
            reranker: None,
            telemetry: None,
            recall_meter: None,
            feedback_counts: std::sync::Mutex::default(),
            key_windows: std::sync::Mutex::default(),
            authenticated_days: std::sync::Mutex::default(),
            authentication_write_failures: AtomicU64::new(0),
            sparse: None,
            formats: Arc::new(crate::ports::ProseFormats),
            signup: None,
            wide_keys: std::sync::Mutex::default(),
            widen: widen::WidenSignals::default(),
            profiles: Arc::new(crate::types::QuotaProfiles::default()),
            uploads: uploads::UploadConfig::default(),
            imports_busy: busy::BusyGate::new(busy::IMPORTS_AT_ONCE),
            exports_busy: busy::BusyGate::new(busy::EXPORTS_AT_ONCE),
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

    /// The app whose workspace briefs are cut to `max_tokens`, the
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
    /// its credential named and the workspace its verb targets, which the
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

    /// The app for a deployment that sets `server.shared` (ADR-0076 d9).
    #[must_use]
    pub fn with_shared(self, shared: bool) -> Self {
        Self { shared, ..self }
    }

    /// Whether the server's admins are not its callers': `server.shared`, or
    /// open sign-up, which admits people who are not the operator's
    /// (ADR-0076 d9; ADR-0068 d1). Whoami reports it as `shared`.
    #[must_use]
    pub fn shared(&self) -> bool {
        self.shared
            || self
                .signup_policy()
                .is_some_and(|p| p.mode == oml_domain::signup::SignupMode::Open)
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
    /// access: one hash lookup and one access read per request (workspaces.md
    /// effective access; ADR-0025 d1).
    async fn authenticate(&self, api_key: &str) -> Result<Caller, AppError> {
        self.authenticate_admitting(api_key, false).await
    }

    /// [`Self::authenticate`], admitting a `maintenance` key only where
    /// `maintenance` says the verb is one of the counts it reads (ADR-0097
    /// d18): every other verb refuses it `access.verb_ceiling`, so a new
    /// verb is closed to it until it is listed.
    async fn authenticate_admitting(
        &self,
        api_key: &str,
        maintenance: bool,
    ) -> Result<Caller, AppError> {
        let (key, previous_value) = self.key_of_value_marked(api_key).await?;
        refuse_maintenance(&key, maintenance)?;
        // A person of the sign-up tenant's key under its request rate
        // (ADR-0068 d5).
        self.admit_request(&key).await?;
        let mut caller = self.caller_of(key).await?;
        caller.previous_value = previous_value;
        Ok(caller)
    }

    /// The caller a key act runs as: [`Self::authenticate_bearer`], refused
    /// where the value was the key's previous one (ADR-0096 d13).
    async fn key_act_caller(
        &self,
        bearer: Bearer<'_>,
        verb: &'static str,
    ) -> Result<Caller, AppError> {
        let caller = self.authenticate_bearer(bearer).await?;
        caller.require_current_value(verb)?;
        Ok(caller)
    }

    /// The caller an admin verb runs as: [`Self::authenticate_bearer`],
    /// refused by [`require_role`] unless the key is an admin key presented
    /// by its current value (ADR-0096 d13).
    async fn admin_caller(
        &self,
        bearer: impl Into<Bearer<'_>>,
        verb: &'static str,
    ) -> Result<Caller, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller, Role::Admin, verb)?;
        Ok(caller)
    }

    /// The caller a key value acts as, or the caller a connection's access
    /// token resolved to, its grants and denials read again here: one key,
    /// one caller, whichever credential presented it (ADR-0031 d11).
    async fn authenticate_bearer(&self, bearer: Bearer<'_>) -> Result<Caller, AppError> {
        self.authenticate_bearer_admitting(bearer, false).await
    }

    /// The caller a counts read runs as (ADR-0097 d18): an admin key, or a
    /// `maintenance` key, presented by its current value.
    pub async fn counts_caller(
        &self,
        bearer: impl Into<Bearer<'_>>,
        verb: &'static str,
    ) -> Result<Caller, AppError> {
        let caller = self
            .authenticate_bearer_admitting(bearer.into(), true)
            .await?;
        if !matches!(caller.key.role, Role::Admin | Role::Maintenance) {
            return Err(AppError::VerbCeiling {
                role: caller.key.role,
                verb,
            });
        }
        caller.require_current_value(verb)?;
        Ok(caller)
    }

    /// [`Self::authenticate_bearer`], a `maintenance` key admitted where
    /// `maintenance` is set ([`Self::authenticate_admitting`]).
    async fn authenticate_bearer_admitting(
        &self,
        bearer: Bearer<'_>,
        maintenance: bool,
    ) -> Result<Caller, AppError> {
        match bearer {
            Bearer::Key(value) => self.authenticate_admitting(value, maintenance).await,
            Bearer::Connection(caller) => {
                refuse_maintenance(&caller.key, maintenance)?;
                self.admit_request(&caller.key).await?;
                let mut fresh = self.caller_of(caller.key.clone()).await?;
                fresh.previous_value = caller.previous_value;
                Ok(fresh)
            }
            Bearer::Session(caller) => {
                // The gate read this key's access at the head of this
                // request: its first resolution decides on those rows, and
                // any later one reads them now (ADR-0070 d8, #297).
                let mut fresh = match caller.take_gate_read() {
                    Some(access) => Caller::of(caller.key.clone(), Some(access)),
                    None => self.caller_of(caller.key.clone()).await?,
                };
                fresh.own_keys.clone_from(&caller.own_keys);
                fresh.previous_value = caller.previous_value;
                fresh.session.clone_from(&caller.session);
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
        Ok(Caller::of(key, access))
    }

    /// The live key the value names and its access as stored, `None` for a
    /// provider-level key, which no tenant's row can name; a value that
    /// names no live key is `Unauthorized`.
    async fn key_and_access(
        &self,
        api_key: &str,
    ) -> Result<(KeyRecord, Option<KeyAccess>), AppError> {
        // `/v2/whoami`'s read: an admin key suspended as unlinked, or
        // waiting on its link, still reaches it (ADR-0097 d21, the
        // production migration).
        let (key, _) = self.key_of_value_admitting(api_key, true).await?;
        let access = self.access_of(&key).await?;
        Ok((key, access))
    }

    /// The live key the value names; a value that names no live key is
    /// `Unauthorized`, and one that names a key past its expiry is told so
    /// (ADR-0079 d2).
    async fn key_of_value(&self, api_key: &str) -> Result<KeyRecord, AppError> {
        Ok(self.key_of_value_marked(api_key).await?.0)
    }

    /// [`Self::key_of_value`], and whether the value is the key's previous
    /// one, inside a rotation's grace (ADR-0096 d13).
    async fn key_of_value_marked(&self, api_key: &str) -> Result<(KeyRecord, bool), AppError> {
        self.key_of_value_admitting(api_key, false).await
    }

    /// [`Self::key_of_value_marked`], admitting with `unlinked` an admin key
    /// suspended as unlinked at the upgrade, or waiting on its person's
    /// acceptance of its link: the one admission `/v2/whoami` and `POST
    /// /v2/keys/self/link` make (ADR-0097 d21, d22, the production
    /// migration).
    pub(crate) async fn key_of_value_admitting(
        &self,
        api_key: &str,
        unlinked: bool,
    ) -> Result<(KeyRecord, bool), AppError> {
        let hash = sha256_digest_string(api_key.as_bytes());
        let mut key = self.journal.key_by_hash(&hash).await;
        let mut previous_value = false;
        // A rotated key's previous value, until its grace ends (ADR-0096
        // d13); past it, the value names no key.
        if matches!(key, Ok(None)) {
            previous_value = true;
            let now = self.clock.now();
            key = self.journal.key_by_previous_hash(&hash).await.map(|found| {
                found
                    .filter(|(_, until)| until.unix_micros() > now.unix_micros())
                    .map(|(key, _)| key)
            });
        }
        if let Ok(found) = &key {
            self.observer.credential(found.as_ref(), None);
        }
        let key = key?.ok_or(AppError::Unauthorized)?;
        self.check_key(&key, unlinked)?;
        self.note_authenticated(&key).await;
        Ok((key, previous_value))
    }

    /// Writes `key`'s row of the server authentication table for today,
    /// the first time this process sees the key authenticate today
    /// (ADR-0097 d19, answer 24; ADR-0042 d8 as amended): one upsert per
    /// key per day, the day's later authentications coalesced in memory. A
    /// write that fails is counted for doctor (`auth.log_write_failed`) and
    /// left unmarked, so the key's next authentication writes it again; the
    /// request it rode on goes on. A provider-level key has no tenant's row.
    pub(crate) async fn note_authenticated(&self, key: &KeyRecord) {
        if key.tenant_id == PROVIDER_TENANT {
            return;
        }
        let now = self.clock.now();
        let day = now.unix_micros().div_euclid(crate::types::DAY_US);
        {
            let mut days = self.authenticated_days.lock().expect("unpoisoned");
            if days.get(&key.key_id) == Some(&day) {
                return;
            }
            days.insert(key.key_id, day);
        }
        let written = self
            .journal
            .record_authentication(
                key.tenant_id,
                key.key_id,
                oml_domain::usage::day_of(&now),
                1,
            )
            .await;
        if written.is_err() {
            self.authentication_write_failures
                .fetch_add(1, Ordering::Relaxed);
            let mut days = self.authenticated_days.lock().expect("unpoisoned");
            if days.get(&key.key_id) == Some(&day) {
                days.remove(&key.key_id);
            }
        }
    }

    /// The authentication rows this process could not write since it
    /// started (doctor's `auth.log_write_failed`, answer 24).
    #[must_use]
    pub fn authentication_write_failures(&self) -> u64 {
        self.authentication_write_failures.load(Ordering::Relaxed)
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
                .workspace(key.tenant_id, root)
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

    /// [`Self::resolve`] with the workspace namespace of `user`, a user root,
    /// or the shared one for `None`: a user key's bare workspace name
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
                    NamedKind::Workspace => match self
                        .journal
                        .resolve_name(tenant_id, NamedKind::Workspace, user)
                        .await?
                    {
                        Some(root) => {
                            self.journal
                                .resolve_user_workspace(tenant_id, root, name)
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
            (NamedKind::Workspace, Some(root)) => {
                match self
                    .journal
                    .resolve_user_workspace(tenant_id, root, name)
                    .await?
                {
                    Some(id) => Some(id),
                    // Of the shared namespace a user key resolves only
                    // `universe`, its own root (ADR-0044 d5) and a public
                    // node (ADR-0098 d2, the operator's decision of
                    // 2026-11-27); a private shared name stays unresolved,
                    // so it is not disclosed.
                    None => match self.journal.resolve_name(tenant_id, kind, name).await? {
                        Some(id) if id == root || name.as_str() == UNIVERSE => Some(id),
                        Some(id) => self
                            .journal
                            .workspace(tenant_id, id)
                            .await?
                            .filter(|w| w.public)
                            .map(|w| w.workspace_id),
                        None => None,
                    },
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

    /// A workspace the request names, else the key's journalled default,
    /// else `request.workspace_unresolved`; called only after step 1 of the
    /// refusal order (ADR-0025 d1; ADR-0026 d4).
    async fn target_workspace(
        &self,
        key: &KeyRecord,
        named: Option<&Reference>,
    ) -> Result<Uuid, AppError> {
        let target = self
            .resolve_opt(key, NamedKind::Workspace, named)
            .await?
            .or(key.default_workspace)
            .ok_or(AppError::WorkspaceUnresolved)?;
        self.observer.workspace(target);
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
            .workspace_ancestry(tenant_id, node)
            .await?
            .map(|ancestry| ancestry.into_iter().map(|w| w.workspace_id).collect())
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

    /// Freeze as admission policy: `workspace.frozen` when the stored status
    /// of `node` refuses `op` (ADR-0009 d5). It reads the node's status and
    /// nothing else, never a grant, so freezing moves no permission epoch
    /// (ADR-0009 d2). Every caller reads the chain head before calling it,
    /// so a ratification that lands between this read and the commit moves
    /// the head and the commit conflicts rather than writing into an area
    /// frozen since. Reassign, re-parent, demote and erasure are not
    /// built; their checks join here when they are.
    async fn refuse_frozen(
        &self,
        tenant_id: Uuid,
        node: Uuid,
        op: FrozenOp,
    ) -> Result<(), AppError> {
        match self.journal.workspace(tenant_id, node).await? {
            Some(w) if frozen_refuses(w.status, op) => Err(AppError::WorkspaceFrozen {
                workspace_id: node,
                op,
            }),
            _ => Ok(()),
        }
    }

    /// A memory is started in a user root by its own user's keys only
    /// (ADR-0044 d16): `submission.user_root_refused` when `node` is a user
    /// root or lies beneath one and `key` does not act for that root's
    /// user, so a tenant key and another person's key are refused alike,
    /// whatever submitter or reviewer grant reaches the node. The person a key
    /// acts for is read as `same_user` reads it, by [`Self::person_of`]: a
    /// user key's root, or a session key's `session_user`, which is how
    /// `<user>-review` names its person (ADR-0045 d6). The two acts that
    /// start a memory call it: a new memory, and a successor of a ratified
    /// retirement. It reads the node's ancestry and nothing of the key's
    /// grants. A caller runs it only once the key is known to read the
    /// node or the row it writes against, so the answer confirms no root
    /// to a key that reads nothing of it; that key keeps
    /// `access.not_found`.
    async fn refuse_another_users_root(&self, key: &KeyRecord, node: Uuid) -> Result<(), AppError> {
        let root = self
            .journal
            .workspace_ancestry(key.tenant_id, node)
            .await?
            .unwrap_or_default()
            .into_iter()
            .find(|w| w.user_root);
        let Some(root) = root else { return Ok(()) };
        if same_user(self.person_of(key).await?.as_ref(), Some(&root.name)) {
            Ok(())
        } else {
            Err(AppError::UserRootRefused)
        }
    }

    /// Whether the caller can read `memory`: its own row in any state,
    /// unless a global denial masks the row's workspace, or a promoted version
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
            .ancestry(caller.key.tenant_id, memory.workspace_id)
            .await?;
        if caller.owns(memory.author_key_id) {
            return Ok(!caller.denied(&ancestry));
        }
        // A submitter reads only the rows it wrote, whatever its grants
        // (ADR-0093 d2, as read on 2026-11-20).
        if caller.key.role == Role::Submitter {
            return Ok(false);
        }
        let reader = caller.effective(&ancestry).contains(&Role::Reader);
        if memory.status == MemoryStatus::Held && reader {
            // Inside an in-progress subtree a reader reads another author's
            // held version of any kind, never a retirement version (ADR-0030
            // d3; ADR-0080 d16).
            return Ok(served_unreviewed(memory, self.scanner.version())
                && self
                    .in_progress_root(caller.key.tenant_id, memory.workspace_id)
                    .await?
                    .is_some());
        }
        Ok(memory.status == MemoryStatus::Promoted && reader)
    }

    /// The served tip of `claim_key` in the workspace as a refusal may name it:
    /// only when the caller can read it (errors.md `submission.tip_conflict`
    /// details; ADR-0025 d10, d16).
    async fn readable_tip(
        &self,
        caller: &Caller,
        workspace_id: Uuid,
        claim_key: &str,
    ) -> Result<Option<VersionRef>, AppError> {
        match self
            .journal
            .claim_tip(caller.key.tenant_id, workspace_id, claim_key)
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
    /// path once per workspace ([`Self::served_in`]). Only the rows
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

    /// [`Self::served`] with the workspace ancestries already read, by node:
    /// a use case serving many rows reads each workspace's ancestry once
    /// (ADR-0024 d4; ADR-0049 d12).
    async fn served_in(
        &self,
        caller: &Caller,
        mut memory: MemoryRecord,
        ancestries: &mut BTreeMap<Uuid, Option<Vec<WorkspaceRecord>>>,
    ) -> Result<ServedMemory, AppError> {
        let tenant_id = caller.key.tenant_id;
        let ancestry = match ancestries.get(&memory.workspace_id) {
            Some(read) => read.clone(),
            None => {
                let read = self
                    .journal
                    .workspace_ancestry(tenant_id, memory.workspace_id)
                    .await?;
                ancestries.insert(memory.workspace_id, read.clone());
                read
            }
        };
        // `universe` is the one node with no parent.
        let anonymous = ancestry.as_ref().is_some_and(|a| a.len() == 1)
            && memory.author_key_id != caller.key.key_id;
        let workspace_path = ancestry.map(|ancestry| derived_path(&ancestry));
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
        let replaced = match memory.replaces {
            Some(id) => self.journal.memory(tenant_id, id).await?,
            None => None,
        };
        // Whether approving this successor needs a person depends on the
        // replaced version's class, which the verdict's refusal names
        // whether the caller reads that version or not (ADR-0080 d16); the
        // version itself is named only where it is readable.
        let replaces_kind = replaced.as_ref().map(|m| m.kind);
        if let Some(m) = replaced
            && (!anonymous || m.workspace_id == memory.workspace_id)
            && self.readable(caller, &m).await?
        {
            replaces = Some(VersionRef {
                artifact_id: m.memory_id,
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
            // Nothing of the review either: what stays on the anonymous
            // view is ADR-0044 d9's list, which names no approval
            // (ADR-0077 d4).
            memory.approval = None;
        }
        let load_warning = match (memory.kind, memory.status) {
            (MemoryKind::Guardrail, MemoryStatus::Held) => Some(LOAD_WARNING),
            (MemoryKind::Skill, MemoryStatus::Held) => Some(oml_domain::skills::SKILL_LOAD_WARNING),
            _ => None,
        };
        Ok(ServedMemory {
            memory,
            replaces,
            replaces_kind,
            replaces_valid_until,
            workspace_path,
            anonymous,
            load_warning,
            scan_findings: None,
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
        self.nodes_walked(caller, root, role, None).await
    }

    /// [`Self::nodes_with`], keeping in `walked` every workspace record the
    /// walk read: the root's ancestry and its subtree, from which
    /// [`ancestry_of`] derives any of their ancestries without another read.
    async fn nodes_walked(
        &self,
        caller: &Caller,
        root: Uuid,
        role: Role,
        mut walked: Option<&mut BTreeMap<Uuid, WorkspaceRecord>>,
    ) -> Result<Vec<Uuid>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let above_records = self
            .journal
            .workspace_ancestry(tenant_id, root)
            .await?
            .unwrap_or_default();
        let above: Vec<Uuid> = above_records.iter().map(|w| w.workspace_id).collect();
        if above.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(walked) = walked.as_deref_mut() {
            walked.extend(above_records.into_iter().map(|w| (w.workspace_id, w)));
        }
        let mut ancestries: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::from([(root, above)]);
        let mut nodes = Vec::new();
        // The subtree lists a parent before its children, so each node's
        // ancestry is its parent's with the node in front.
        for node in self.journal.workspace_subtree(tenant_id, root).await? {
            let id = node.workspace_id;
            if let Some(walked) = walked.as_deref_mut() {
                walked.insert(id, node.clone());
            }
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
    /// none, crosses the boundary (ADR-0030 d7): `workspace.in_progress_refused`,
    /// reason `edge_crosses`, naming the first endpoint's workspace.
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
                workspace_id: src,
                reason: InProgressReason::EdgeCrosses,
            });
        }
        Ok(())
    }

    /// What the node is to the ladder, from the one ancestry read a
    /// transition makes; first, a status transition on a node inside an
    /// in-progress subtree waits for its commit (ADR-0030 d6):
    /// `workspace.in_progress_refused`, reason `commit_first`.
    async fn ladder_node_outside(
        &self,
        tenant_id: Uuid,
        node: Uuid,
    ) -> Result<LadderNode, AppError> {
        let ancestry = self
            .journal
            .workspace_ancestry(tenant_id, node)
            .await?
            .unwrap_or_default();
        if in_progress_root_in(&ancestry).is_some() {
            return Err(AppError::InProgressRefused {
                workspace_id: node,
                reason: InProgressReason::CommitFirst,
            });
        }
        Ok(ladder_node(&ancestry))
    }

    /// The node's in-progress root, or `None` (ADR-0030 d1).
    async fn in_progress_root(
        &self,
        tenant_id: Uuid,
        node: Uuid,
    ) -> Result<Option<Uuid>, AppError> {
        let ancestry = self
            .journal
            .workspace_ancestry(tenant_id, node)
            .await?
            .unwrap_or_default();
        Ok(in_progress_root_in(&ancestry))
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
        for node in self.journal.workspace_subtree(tenant_id, top).await? {
            // A node that is not `development` is never inside, nor is
            // anything beneath it (ADR-0030 d3).
            let root = if node.status != WorkspaceStatus::Development {
                None
            } else if node.in_progress {
                Some(node.workspace_id)
            } else if node.workspace_id == top {
                top_root
            } else {
                node.parent_id
                    .and_then(|p| roots.get(&p).copied().flatten())
            };
            roots.insert(node.workspace_id, root);
        }
        Ok(roots)
    }

    /// The served node set of a read scoped at `scope` (ADR-0030 d2, as
    /// ADR-0080 d16 amends it): the nodes of subtree(scope) where the caller
    /// reads, and of them the nodes inside an in-progress subtree, the
    /// scope's own or one whose root lies below the scope, whose held rows
    /// every reader there is served. A sibling's or another branch's
    /// subtree is never under the scope, so never served.
    async fn served_nodes(&self, caller: &Caller, scope: Uuid) -> Result<ServedNodes, AppError> {
        let tenant_id = caller.key.tenant_id;
        let nodes = self.nodes_with(caller, scope, Role::Reader).await?;
        let roots = self.subtree_roots(tenant_id, scope).await?;
        let mut served = ServedNodes {
            nodes: Vec::with_capacity(nodes.len()),
            inside: Vec::new(),
        };
        for node in nodes {
            // Inside a root, every node of which is development; a
            // protected or frozen node, and anything under one, never is
            // (d3).
            if roots.get(&node).copied().flatten().is_some() {
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

    /// Every node where reviewer is effective for the caller: the union over
    /// its reviewer grants' subtrees, a row outside which is never served
    /// (ADR-0025 d1).
    async fn reviewer_nodes(&self, caller: &Caller) -> Result<Vec<Uuid>, AppError> {
        self.nodes_holding(caller, Role::Reviewer).await
    }

    /// [`Self::reviewer_nodes`] with the workspace records the walk read, so
    /// a listing serving rows across many of those nodes derives each
    /// row's workspace path from them instead of reading an ancestry per
    /// workspace (ADR-0070 d8, the held listing of 2026-11-13).
    async fn reviewer_nodes_walked(
        &self,
        caller: &Caller,
    ) -> Result<(Vec<Uuid>, BTreeMap<Uuid, WorkspaceRecord>), AppError> {
        let roots: BTreeSet<Uuid> = caller
            .grants
            .iter()
            .filter(|g| g.role == Role::Reviewer)
            .map(|g| g.node)
            .collect();
        let mut walked = BTreeMap::new();
        let mut nodes = BTreeSet::new();
        for root in roots {
            nodes.extend(
                self.nodes_walked(caller, root, Role::Reviewer, Some(&mut walked))
                    .await?,
            );
        }
        Ok((nodes.into_iter().collect(), walked))
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

    /// The head an act seals on: the one its caller read, else the head now.
    async fn head_at(
        &self,
        tenant_id: Uuid,
        head: Head<'_>,
    ) -> Result<Option<SignedEnvelope>, AppError> {
        match head {
            Head::AtSeal => self.head_event(tenant_id).await,
            Head::BeforeAccess(read) => Ok(read.clone()),
        }
    }

    /// The public nodes `caller` reads that `hit_nodes` does not hold yet,
    /// each added to it: a recall's served tips there rank with the
    /// scope's own, and a node already in the scope counts once (ADR-0098
    /// d3). A global deny naming the node or an ancestor removes it (d7);
    /// a key with no global deny reads no ancestry for this.
    async fn public_hit_nodes(
        &self,
        caller: &Caller,
        hit_nodes: &mut Vec<Uuid>,
    ) -> Result<BTreeSet<Uuid>, AppError> {
        let mut added = BTreeSet::new();
        for &node in &caller.public {
            if hit_nodes.contains(&node) {
                continue;
            }
            if !caller.denials.is_empty() {
                let ancestry = self.ancestry(caller.key.tenant_id, node).await?;
                if !caller.effective(&ancestry).contains(&Role::Reader) {
                    continue;
                }
            }
            hit_nodes.push(node);
            added.insert(node);
        }
        Ok(added)
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
        let at = self.clock.now();
        self.seal_at(
            prev, tenant_id, event_type, principal, api_key_id, payload, at,
        )
    }

    /// [`Self::seal`] at `at`, read off the clock once by a caller whose
    /// payload names an instant derived from the envelope's own, as a
    /// rotation's grace end does (ADR-0096 d13).
    #[allow(clippy::too_many_arguments)]
    fn seal_at(
        &self,
        prev: Option<&SignedEnvelope>,
        tenant_id: Uuid,
        event_type: &str,
        principal: &str,
        api_key_id: Option<Uuid>,
        payload: Value,
        at: EventTime,
    ) -> Result<SignedEnvelope, AppError> {
        let draft = EventDraft {
            tenant_id,
            event_id: self.ids.new_id(),
            event_type: event_type.to_owned(),
            principal: principal.to_owned(),
            api_key_id,
            at,
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
            .commit_requests(vec![commit_request(event, fold, idempotency)])
            .await?)
    }

    /// Commits chain-consecutive events as one transaction: a refusal of
    /// any of them writes none (ADR-0025 d10).
    async fn commit_chain(
        &self,
        links: Vec<(SignedEnvelope, MetadataFold)>,
    ) -> Result<CommitReceipt, AppError> {
        Ok(self
            .commit_requests(
                links
                    .into_iter()
                    .map(|(event, fold)| commit_request(event, fold, None))
                    .collect(),
            )
            .await?)
    }

    /// The one path every use case commits through: one request alone, or
    /// several as one transaction. An erasure, a demotion, or a change to
    /// what is trusted or federated drops its tenant's widen signals once
    /// it commits (#244). A commit that fails after the database committed
    /// drops nothing; the signal's 300 s bound covers it.
    async fn commit_requests(
        &self,
        mut requests: Vec<CommitRequest>,
    ) -> Result<CommitReceipt, StoreError> {
        let drops: BTreeSet<Uuid> = requests
            .iter()
            .filter(|r| widen::drops_widen_signals(&r.event.envelope.event_type))
            .map(|r| r.event.envelope.tenant_id)
            .collect();
        let receipt = if requests.len() == 1 {
            self.journal
                .commit(requests.pop().expect("one request"))
                .await?
        } else {
            self.journal.commit_chain(requests).await?
        };
        for tenant_id in drops {
            self.widen.drop_tenant(tenant_id);
        }
        Ok(receipt)
    }

    fn mint_key(
        &self,
        tenant_id: Uuid,
        req: &IssueKeyRequest,
        default_workspace: Option<Uuid>,
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
            // (postgres-schema.md api_keys.used_by); the journal says null.
            used_by: req.used_by.unwrap_or(UsedBy::Agent),
            default_workspace,
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
            expiry: req.expiry.clone(),
            requests_per_minute: req.requests_per_minute,
            linked_user: None,
            link_pending: None,
            pending_act: None,
            suspended: None,
            issuer_person: None,
        };
        (record, value)
    }

    /// The catalog payload of `key.issued` (event-catalog.md): never the
    /// value (roles-and-keys.md show-once rule), and `used_by` as sent.
    fn key_issued_payload(record: &KeyRecord, used_by: Option<UsedBy>) -> Value {
        let mut payload = json!({
            "key_id": record.key_id.to_string(),
            "role": record.role.as_str(),
            "principal": record.principal,
            "issued_by_key_id": record.issued_by.map(|k| k.to_string()),
            "expiry": record.expiry.as_ref().map(EventTime::as_str),
            "value_disclosed": false,
            "used_by": used_by.map(UsedBy::as_str),
            "default_workspace_id": record.default_workspace.map(|w| w.to_string()),
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
        // The key's own rate, absent for a key with none (ADR-0079 d4).
        if let (Some(rate), Some(obj)) = (record.requests_per_minute, payload.as_object_mut()) {
            obj.insert("requests_per_minute".to_owned(), json!(rate));
        }
        // The issuer's person at issuance, absent for an issuer that counts
        // as no person (ADR-0097, amendment of 2026-11-27, answer 17).
        if let (Some(person), Some(obj)) = (&record.issuer_person, payload.as_object_mut()) {
            obj.insert("issuer_person".to_owned(), json!(person.as_str()));
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
                "workspace_node_id": grant.node.to_string(),
                "role": grant.role.as_str(),
                "deny": grant
                    .deny
                    .iter()
                    .map(|d| json!({ "subtree_node_id": d.to_string() }))
                    .collect::<Vec<_>>(),
                "covers": grant.covers.as_str(),
                "epoch": epoch,
            })
            .as_object()
            .cloned()
            .map(|mut obj| {
                // A flagged grant says so; an unflagged one journals as it
                // did before the flag (ADR-0097 d31).
                if grant.create {
                    obj.insert("create".to_owned(), json!(true));
                }
                Value::Object(obj)
            })
            .unwrap_or_default(),
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

    /// The person an issuance's admin key belongs to (ADR-0097 d21): an
    /// admin key an admin key issues requires `person` and `used_by:
    /// person` (`admin_key_requires_person`), naming a user root of the
    /// tenant (`user_unknown`, as a link's); `person` on any other role is
    /// `person_admin_only`. A
    /// session key's `<user>-admin` belongs to its user already. Nothing is
    /// journalled here.
    async fn admin_key_person(
        &self,
        issuer: &KeyRecord,
        req: &IssueKeyRequest,
        how: &Issuance<'_>,
    ) -> Result<Option<NamedRef>, AppError> {
        if req.role != Role::Admin {
            return match req.person {
                Some(_) => Err(AppError::KeyPerson("person_admin_only")),
                None => Ok(None),
            };
        }
        if !matches!(how, Issuance::Plain(..)) {
            return Ok(None);
        }
        let Some(person) = req
            .person
            .as_ref()
            .filter(|_| req.used_by == Some(UsedBy::Person))
        else {
            return Err(AppError::KeyPerson("admin_key_requires_person"));
        };
        let tenant_id = issuer.tenant_id;
        let root = match self
            .journal
            .resolve_name(tenant_id, NamedKind::Workspace, person)
            .await?
        {
            Some(id) => self.journal.workspace(tenant_id, id).await?,
            None => None,
        }
        .filter(|w| w.user_root)
        .ok_or(AppError::KeyLinkRefused {
            reason: "user_unknown",
        })?;
        // The person must hold a live admin flag (d21, the operator's
        // decision of 2026-11-27).
        if !self.is_admin_person(tenant_id, Some(person)).await? {
            return Err(AppError::KeyPerson("admin_key_requires_admin_person"));
        }
        // A person who cannot sign in never accepts the link, so the key
        // never authenticates (d7).
        Ok(Some(NamedRef {
            id: root.workspace_id,
            name: person.clone(),
        }))
    }

    /// An admin key's issuance as one widening admin act (ADR-0097 d5,
    /// d10, d23): `admin.act`, `key.issued` naming the act, and, when the
    /// act takes effect at once under `sole_admin`, every `grant.issued` and
    /// the connector key's `connection.code_issued`, in one chain, so a
    /// refusal of any link journals none. A pending act journals the key
    /// with its `pending_act`, and keeps the grants and the code's digest in
    /// the act's effect for the confirmation to apply; the code is shown
    /// once, now, and its ten minutes run from the confirmation (d6).
    #[allow(clippy::too_many_arguments)]
    async fn issue_as_admin_act(
        &self,
        issuer: &KeyRecord,
        req: &IssueKeyRequest,
        mut record: KeyRecord,
        grants: Vec<Grant>,
        creations: &[WorkspaceRecord],
        oauth: Option<&Issuer>,
        prev: Option<&SignedEnvelope>,
        (reason, timebox): (Option<&str>, Option<&Timebox>),
        user: Option<&NamedRef>,
        person: Option<&NamedRef>,
    ) -> Result<IssuedKey, AppError> {
        let tenant_id = issuer.tenant_id;
        let key_id = record.key_id;
        // The confirm screen flags a principal spelled as a person's name:
        // a user root holds it (ADR-0097 d23).
        let principal_names_person = match Name::parse_reference(&req.principal) {
            Ok(name) => match self
                .journal
                .resolve_name(tenant_id, NamedKind::Workspace, &name)
                .await?
            {
                Some(id) => self
                    .journal
                    .workspace(tenant_id, id)
                    .await?
                    .is_some_and(|w| w.user_root),
                None => false,
            },
            Err(_) => false,
        };
        let target = json!({
            "key_id": key_id.to_string(),
            "name": req.name.as_str(),
            "role": req.role.as_str(),
            "principal": req.principal,
            "used_by": req.used_by.map(UsedBy::as_str),
            "user": user.map(|u| u.name.as_str()),
            // The root's person the confirm screen names (ADR-0097 d6).
            "root_person": user.map(|u| u.name.as_str()),
            "person": person.map(|p| p.name.as_str()),
            "principal_names_person": principal_names_person,
            // A time box, taken at once until its expiry (ADR-0097 d13).
            "timebox": timebox.is_some(),
            "until": timebox.and(record.expiry.as_ref()).map(EventTime::as_str),
            "timebox_nodes": timebox.map(|t| t.nodes.clone()),
            "renewal": timebox.is_some_and(|t| t.renewal),
            // The workspaces the issuance creates, which the confirm screen
            // names (ADR-0097 d30).
            "create_workspaces": creations
                .iter()
                .map(|w| json!({
                    "name": w.name.as_str(),
                    "parent_id": w.parent_id.map(|p| p.to_string()),
                }))
                .collect::<Vec<_>>(),
        });
        let code = oauth.map(|oauth| self.new_connect_code(oauth, key_id, CONNECT_CODE_TTL_SECS));
        // A default the issuance creates: a pending issuance names none
        // until its confirmation sets it with `key.default_set` (ADR-0097
        // d30, the operator's decision of 2026-11-27).
        let created_default = record
            .default_workspace
            .filter(|d| creations.iter().any(|w| w.workspace_id == *d));
        let effect = json!({
            "key_id": key_id.to_string(),
            "default_workspace_id": created_default.map(|d| d.to_string()),
            "grants": grants.iter().map(admin_acts::grant_json).collect::<Vec<_>>(),
            "workspaces": creations.iter().map(|w| json!({
                "workspace_id": w.workspace_id.to_string(),
                "name": w.name.as_str(),
                "parent_id": w.parent_id.map(|p| p.to_string()),
            })).collect::<Vec<_>>(),
            "code": code.as_ref().map(|(code, _)| json!({
                "code_id": code.code_id.to_string(),
                "code_hash": code.code_hash.to_prefixed(),
                "ttl_secs": CONNECT_CODE_TTL_SECS,
            })),
        });
        let act = self
            .begin_admin_act_boxed(
                issuer,
                oml_domain::admin_act::AdminActKind::IssueKey,
                target,
                reason,
                effect,
                prev,
                timebox.is_some(),
            )
            .await?;
        let pending = act.pending();
        if pending {
            record.pending_act = Some(act.act_id());
            // A pending issuance creates nothing, so its key names no
            // default it creates until the confirmation sets it (ADR-0097
            // d30, the operator's decision of 2026-11-27).
            if created_default.is_some() {
                record.default_workspace = None;
            }
        }
        let mut payload =
            admin_acts::with_act_id(Self::key_issued_payload(&record, req.used_by), act.act_id());
        if pending && let Some(obj) = payload.as_object_mut() {
            obj.insert("pending_act_id".to_owned(), json!(act.act_id().to_string()));
        }
        // At once, the creations lead the effect's chain after the act, each
        // naming it and charged as a create is (ADR-0097 d30).
        let mut created = Vec::new();
        let mut created_last = act.event.clone();
        if !pending {
            for workspace in creations {
                let event = self.seal(
                    Some(&created_last),
                    tenant_id,
                    event_types::WORKSPACE_CREATE,
                    &issuer.principal,
                    Some(issuer.key_id),
                    admin_acts::with_act_id(
                        Self::workspace_create_payload(workspace),
                        act.act_id(),
                    ),
                )?;
                created_last = event.clone();
                created.push((
                    event,
                    MetadataFold::WorkspaceCreated(workspace.clone()),
                    self.ceiling_for(tenant_id, workspace.namespace).await?,
                ));
            }
        }
        let event = self.seal(
            Some(&created_last),
            tenant_id,
            event_types::KEY_ISSUED,
            &issuer.principal,
            Some(issuer.key_id),
            payload,
        )?;
        let charged = record
            .user
            .filter(|_| record.credential != Credential::Session);
        let ceiling = self.ceiling_for(tenant_id, charged).await?;
        let issued_seq = event.envelope.seq;
        let mut last = event.clone();
        let mut links = vec![act.link()];
        let mut ceilings = vec![None];
        for (event, fold, ceiling) in created {
            links.push((event, fold));
            ceilings.push(ceiling);
        }
        links.push((event, MetadataFold::KeyIssued(record)));
        ceilings.push(ceiling);
        // An admin key's link to its person, pending until that person
        // accepts it, in the issuance's chain (ADR-0097 d21, d7).
        if let Some(person) = person {
            let event = self.seal(
                Some(&last),
                tenant_id,
                event_types::KEY_LINKED_PERSON,
                &issuer.principal,
                Some(issuer.key_id),
                admin_acts::with_act_id(
                    json!({
                        "key_id": key_id.to_string(),
                        "user": person.name.as_str(),
                        "user_root_id": person.id.to_string(),
                        "linked_by_key_id": issuer.key_id.to_string(),
                        "accepted": false,
                    }),
                    act.act_id(),
                ),
            )?;
            last = event.clone();
            links.push((
                event,
                MetadataFold::KeyLinked {
                    key_id,
                    user_root: Some(person.id),
                    accepted: false,
                },
            ));
        }
        let mut stored = Vec::new();
        if !pending {
            for grant in grants {
                let event = self.grant_event(&last, issuer, key_id, &grant)?;
                let event = self.seal(
                    Some(&last),
                    tenant_id,
                    &event.envelope.event_type,
                    &event.envelope.principal,
                    event.envelope.api_key_id,
                    admin_acts::with_act_id(event.envelope.payload, act.act_id()),
                )?;
                stored.push(GrantRecord {
                    grant: grant.clone(),
                    epoch: event.envelope.seq,
                });
                last = event.clone();
                links.push((event, MetadataFold::GrantIssued { key_id, grant }));
            }
            if let Some((code, _)) = &code {
                let event = self.seal(
                    Some(&last),
                    tenant_id,
                    event_types::CONNECTION_CODE_ISSUED,
                    &issuer.principal,
                    Some(issuer.key_id),
                    admin_acts::with_act_id(
                        code_issued_payload(code, issuer.key_id, None),
                        act.act_id(),
                    ),
                )?;
                links.push((
                    event,
                    MetadataFold::ConnectCodeIssued {
                        code: code.clone(),
                        voids: None,
                    },
                ));
            }
        }
        let mut reqs: Vec<CommitRequest> = links
            .into_iter()
            .map(|(event, fold)| commit_request(event, fold, None))
            .collect();
        for (req, ceiling) in reqs.iter_mut().zip(ceilings) {
            req.ceiling = ceiling;
        }
        // A key issued at once under `sole_admin` into another person's
        // root notifies that person (ADR-0097 d6, d20).
        self.with_notice(&act.record, &mut reqs).await?;
        self.commit_requests(reqs).await?;
        Ok(IssuedKey {
            key_id,
            value: String::new(),
            role: req.role,
            principal: req.principal.clone(),
            used_by: req.used_by,
            default_workspace: None,
            name: req.name.clone(),
            crew_id: None,
            user: None,
            guardrail_scope: oml_domain::guardrail::GuardrailScope::ISSUANCE_DEFAULT,
            expiry: req.expiry.clone(),
            requests_per_minute: req.requests_per_minute,
            access: KeyAccess {
                issued_seq,
                grants: stored,
                denials: Vec::new(),
                lift_epochs: Vec::new(),
                public_nodes: Vec::new(),
            },
            connect_code: code.map(|(_, shown)| shown),
            pending_act: pending.then(|| act.act_id()),
            act_id: Some(act.act_id()),
        })
    }

    /// The user root an issuance names in `user`: `user` is admitted on a
    /// reader, contributor, submitter or reviewer key used by an agent or a person, and never
    /// names `universe`; a user root holding the name is joined, a shared
    /// workspace holding it is `name.taken`, and otherwise the root is to be
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
        if req.used_by == Some(UsedBy::Service) {
            return Err(AppError::UserRefused(
                "user is refused on a used_by service key, which acts for a process and not a person",
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
            .resolve_name(tenant_id, NamedKind::Workspace, user)
            .await?;
        let (id, created) = match held {
            None => (self.ids.new_id(), true),
            Some(id) => {
                let root = self.journal.workspace(tenant_id, id).await?;
                if !root.is_some_and(|w| w.user_root) {
                    return Err(AppError::NameTaken {
                        kind: NamedKind::Workspace,
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

    /// A workspace reference of an issuance, in the namespace of the user
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
                self.resolve_as(issuer, Some(u.id), NamedKind::Workspace, reference)
                    .await
            }
            (None, _) => self.resolve(issuer, NamedKind::Workspace, reference).await,
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

    /// Journal `workspace.create` of a user root under `universe`, the
    /// issuing admin as actor, `user_root: true` in its payload (ADR-0044
    /// d1, d2).
    async fn create_user_root(&self, issuer: &KeyRecord, user: &UserRoot) -> Result<(), AppError> {
        let workspace = WorkspaceRecord {
            in_progress: false,
            public: false,
            public_act_id: None,
            workspace_id: user.id,
            tenant_id: issuer.tenant_id,
            parent_id: Some(user.universe),
            name: user.name.clone(),
            status: WorkspaceStatus::Development,
            crew: None,
            crew_id: None,
            user_root: true,
            namespace: None,
        };
        let prev = self.head_event(issuer.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            issuer.tenant_id,
            event_types::WORKSPACE_CREATE,
            &issuer.principal,
            Some(issuer.key_id),
            Self::workspace_create_payload(&workspace),
        )?;
        self.commit(event, MetadataFold::WorkspaceCreated(workspace), None)
            .await?;
        Ok(())
    }

    /// The catalog payload of `workspace.create` (event-catalog.md), with
    /// `user_root` (ADR-0044 d1).
    fn workspace_create_payload(workspace: &WorkspaceRecord) -> Value {
        json!({
            "workspace_id": workspace.workspace_id.to_string(),
            "parent_id": workspace.parent_id.map(|p| p.to_string()),
            "name": workspace.name.as_str(),
            "status": workspace.status.as_str(),
            "crew_id": workspace.crew_id.map(|c| c.to_string()),
            "prefix": Value::Null,
            "user_root": workspace.user_root,
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
        let workspace = self
            .journal
            .workspace(tenant_id, root)
            .await?
            .ok_or_else(|| StoreError::Backend(format!("user root {root} is not held")))?;
        Ok(Some(NamedRef {
            id: root,
            name: workspace.name,
        }))
    }

    /// The tenant's `universe` root (ADR-0026 d6).
    async fn universe(&self, tenant_id: Uuid) -> Result<Uuid, AppError> {
        let universe_name = Name::parse(UNIVERSE).expect("universe is a name (ADR-0026 d6)");
        Ok(self
            .journal
            .resolve_name(tenant_id, NamedKind::Workspace, &universe_name)
            .await?
            .ok_or_else(|| StoreError::Backend(format!("tenant {tenant_id} has no universe")))?)
    }

    // -- use-cases ----------------------------------------------------------

    /// The operator ceremony: genesis event, the `universe` root workspace,
    /// first admin key (roles-and-keys.md bootstrap; workspaces.md, one
    /// universe per tenant), committed as one chain, so a refused ceremony
    /// leaves no tenant (ADR-0025 d10). The root's name is the first segment of every
    /// served `workspace_path` (ADR-0025 d14). Unauthenticated by design — the
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
        self.bootstrap_tenant_for(req, None).await
    }

    /// [`Self::bootstrap_tenant`], the first admin key linked to `person`
    /// where the claim names one, and that person's sign-in email recorded
    /// as accepted (ADR-0085 d16; ADR-0089, the operator's decision of
    /// 2026-11-19; ADR-0045 d2's note, the operator's decision of
    /// 2026-11-20).
    ///
    /// # Errors
    /// As [`Self::bootstrap_tenant`]; a person whose name is not a user's
    /// (`universe` included), before anything is journalled.
    pub async fn bootstrap_tenant_for(
        &self,
        req: &BootstrapRequest,
        person: Option<&BootstrapPerson>,
    ) -> Result<BootstrapResult, AppError> {
        if let Some(p) = person {
            if p.user.as_str() == UNIVERSE {
                return Err(AppError::UserRefused(
                    "user may not be universe, the tenant's root",
                ));
            }
            oml_domain::web::check_user(&p.user)?;
        }
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
        // The whole ceremony commits as one chain: a refusal of any link,
        // the person's email included, journals nothing and leaves no
        // tenant, so the token the claim restores claims again cleanly
        // (ADR-0025 d10; the reading of 2026-11-20 after the fifth review
        // of #498).
        let mut links = vec![commit_request(genesis, fold, None)];

        let root_workspace_id = self.ids.new_id();
        let workspace = WorkspaceRecord {
            in_progress: false,
            public: false,
            public_act_id: None,
            workspace_id: root_workspace_id,
            tenant_id,
            parent_id: None,
            name: Name::parse(UNIVERSE).expect("universe is a name (ADR-0026 d6)"),
            // The universe root is permanently protected (ADR-0009 d1).
            status: WorkspaceStatus::Protected,
            crew: None,
            crew_id: None,
            user_root: false,
            namespace: None,
        };
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::WORKSPACE_CREATE,
            "operator",
            None,
            json!({
                "workspace_id": root_workspace_id.to_string(),
                "parent_id": Value::Null,
                "name": workspace.name.as_str(),
                "status": workspace.status.as_str(),
                "crew_id": Value::Null,
                "prefix": Value::Null,
            }),
        )?;
        prev = event.clone();
        links.push(commit_request(
            event,
            MetadataFold::WorkspaceCreated(workspace),
            None,
        ));

        // The ceremony's admin key takes the name the claim gives it
        // (ADR-0026 d9).
        let issue = IssueKeyRequest {
            profile: None,
            role: Role::Admin,
            name: req.admin_key_name.clone(),
            principal: req.admin_principal.clone(),
            used_by: Some(UsedBy::Person),
            default_workspace: Some(root_workspace_id.into()),
            crew: None,
            grants: Vec::new(),
            user: None,
            connect_code: false,
            guardrail_scope: None,
            expiry: None,
            requests_per_minute: None,
            person: None,
            create_workspaces: Vec::new(),
        };
        let (record, value) =
            self.mint_key(tenant_id, &issue, Some(root_workspace_id), None, None, None);
        let key_id = record.key_id;
        let guardrail_scope = record.guardrail_scope;
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::KEY_ISSUED,
            "operator",
            None,
            Self::key_issued_payload(&record, issue.used_by),
        )?;
        let prev = event.clone();
        links.push(commit_request(event, MetadataFold::KeyIssued(record), None));
        let key_link = links.len();
        if let Some(person) = person {
            links.extend(self.link_bootstrap_admin(
                tenant_id,
                root_workspace_id,
                key_id,
                person,
                prev,
            )?);
        }
        let after_key = u64::try_from(links.len() - key_link).unwrap_or(0);
        let receipt = self.commit_requests(links).await?;

        Ok(BootstrapResult {
            tenant_id,
            root_workspace_id,
            admin_key: IssuedKey {
                act_id: None,
                pending_act: None,
                key_id,
                value,
                role: issue.role,
                principal: issue.principal,
                used_by: issue.used_by,
                default_workspace: Some(root_workspace_id),
                name: issue.name,
                crew_id: None,
                user: None,
                guardrail_scope,
                expiry: None,
                requests_per_minute: None,
                access: KeyAccess {
                    issued_seq: receipt.seq - after_key,
                    grants: Vec::new(),
                    denials: Vec::new(),
                    lift_epochs: Vec::new(),
                    public_nodes: Vec::new(),
                },
                connect_code: None,
            },
        })
    }

    /// The ceremony's link of its admin key to `person` (ADR-0085 d16;
    /// ADR-0089, the operator's decision of 2026-11-19): that person's user
    /// root under `universe`, then `key.linked_person`, then the person's
    /// sign-in entry as `web.email_allowed`, all by the operator, chained
    /// after the key's issuance, for the ceremony to commit with it. The person runs the
    /// ceremony themselves, so the link is accepted at its creation and
    /// needs no `key.link_accepted`, and the entry, an admin's, is not
    /// pending (the operator's decisions of 2026-11-19 on key-to-person
    /// links and of 2026-11-20 on the bootstrap's email). An email another
    /// tenant's entry holds refuses the ceremony's commit, `web.email_taken`.
    fn link_bootstrap_admin(
        &self,
        tenant_id: Uuid,
        universe: Uuid,
        key_id: Uuid,
        person: &BootstrapPerson,
        prev: SignedEnvelope,
    ) -> Result<Vec<CommitRequest>, AppError> {
        let root = WorkspaceRecord {
            in_progress: false,
            public: false,
            public_act_id: None,
            workspace_id: self.ids.new_id(),
            tenant_id,
            parent_id: Some(universe),
            name: person.user.clone(),
            status: WorkspaceStatus::Development,
            crew: None,
            crew_id: None,
            user_root: true,
            namespace: None,
        };
        let root_id = root.workspace_id;
        let created = self.seal(
            Some(&prev),
            tenant_id,
            event_types::WORKSPACE_CREATE,
            "operator",
            None,
            Self::workspace_create_payload(&root),
        )?;
        let linked = self.seal(
            Some(&created),
            tenant_id,
            event_types::KEY_LINKED_PERSON,
            "operator",
            None,
            json!({
                "key_id": key_id.to_string(),
                "user": person.user.as_str(),
                "user_root_id": root_id.to_string(),
                "linked_by_key_id": Value::Null,
                "accepted": true,
            }),
        )?;
        let entry = WebEntry {
            tenant_id,
            email: person.email.clone(),
            user: person.user.clone(),
            admin: true,
            google_subject: None,
            github_subject: None,
            added_by: key_id,
            terms_version: None,
            disabled_at: None,
            pending: false,
        };
        let allowed = self.seal(
            Some(&linked),
            tenant_id,
            event_types::WEB_EMAIL_ALLOWED,
            "operator",
            None,
            json!({
                "email_digest": person.email.digest().to_prefixed(),
                "user": person.user.as_str(),
                "admin": true,
                "by_key_id": key_id.to_string(),
                "pending": false,
            }),
        )?;
        Ok(vec![
            commit_request(created, MetadataFold::WorkspaceCreated(root), None),
            commit_request(
                linked,
                MetadataFold::KeyLinked {
                    key_id,
                    user_root: Some(root_id),
                    accepted: true,
                },
                None,
            ),
            commit_request(allowed, MetadataFold::WebEmailAllowed(entry), None),
        ])
    }

    /// Issue a key with its grants, validated in the issuance order of
    /// roles-and-keys.md (ADR-0025 d2): the two-key rule, the issuer ceiling,
    /// grant admissibility, node and crew existence, deny containment, the
    /// default's coverage, with `connect_code` whether the key can hold a
    /// connection, and last the key's name. Every workspace and crew
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
    /// `operator`, `used_by: service` and a deployment without `oauth.issuer`
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
        let issuer = self.key_act_caller(bearer.into(), "issue_key").await?.key;
        if issuer.role != Role::Admin && issuer.role != Role::Operator {
            return Err(AppError::VerbCeiling {
                role: issuer.role,
                verb: "issue_key",
            });
        }
        self.issue_key_by(&issuer, req, Issuance::Plain(None, None))
            .await
    }

    /// [`App::issue_key`] with the admin's reason, scanned and journalled in
    /// the issuance's `admin.act` (ADR-0097 d26). An admin's issuance is
    /// widening (d5): in a tenant that never had two live admin persons it
    /// takes effect at once, marked `sole_admin` (d10); otherwise the key's
    /// value is shown once, the key authenticates nothing, and its grants
    /// and connect code wait in the act, until a second admin person
    /// confirms it (d23). A pending issuance requires the reason.
    ///
    /// # Errors
    /// As [`App::issue_key`], and [`AppError::AdminActReasonRequired`].
    pub async fn issue_key_with_reason(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &IssueKeyRequest,
        reason: Option<&str>,
    ) -> Result<IssuedKey, AppError> {
        let issuer = self.key_act_caller(bearer.into(), "issue_key").await?.key;
        if issuer.role != Role::Admin && issuer.role != Role::Operator {
            return Err(AppError::VerbCeiling {
                role: issuer.role,
                verb: "issue_key",
            });
        }
        self.issue_key_by(&issuer, req, Issuance::Plain(reason, None))
            .await
    }

    /// A time box (ADR-0097 d13): an admin person issues, at once and in any
    /// tenant, a reader, submitter or contributor key whose grants are
    /// reader and submitter grants in shared workspaces, never into a user
    /// root or an in-progress subtree, with an expiry at most 24 hours away
    /// that everything issued under it inherits; the key issues nothing. It
    /// is a widening `issue_key` act marked `timebox`, effective at once and
    /// settled afterwards by a second admin person who counts (d11): a
    /// confirmation settles it, a decline revokes the key. One open box per
    /// actor person per workspace, and one renewal, after which a box on the
    /// workspace is refused until a second admin person settles the first.
    /// Refused while an act against an admin is open and during a freeze.
    /// It requires a reason (d26).
    ///
    /// # Errors
    /// [`AppError::Timebox`] naming the rule, [`AppError::AdminActReasonRequired`],
    /// and [`App::issue_key`]'s.
    pub async fn issue_timeboxed_key(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &IssueKeyRequest,
        reason: Option<&str>,
    ) -> Result<IssuedKey, AppError> {
        use oml_domain::admin_act::{AdminActKind, AdminActState, TIMEBOX_MAX_SECS};
        let issuer = self.key_act_caller(bearer.into(), "issue_key").await?.key;
        if issuer.role != Role::Admin {
            return Err(AppError::VerbCeiling {
                role: issuer.role,
                verb: "issue_key",
            });
        }
        let tenant_id = issuer.tenant_id;
        let refused = |r| Err(AppError::Timebox(r));
        let Some(person) = self.counted_person_of(&issuer).await? else {
            return refused(TimeboxRefusal::NotAdminPerson);
        };
        if !self.is_admin_person(tenant_id, Some(&person)).await? {
            return refused(TimeboxRefusal::NotAdminPerson);
        }
        if !matches!(req.role, Role::Reader | Role::Submitter | Role::Contributor)
            || req.grants.is_empty()
            || req
                .grants
                .iter()
                .any(|g| !matches!(g.role, Role::Reader | Role::Submitter) || g.create)
            || req.connect_code
            || req.user.is_some()
            || req.person.is_some()
            || !req.create_workspaces.is_empty()
        {
            return refused(TimeboxRefusal::Shape);
        }
        let now = self.clock.now();
        match &req.expiry {
            Some(until)
                if until.unix_micros() > now.unix_micros()
                    && until.unix_micros() - now.unix_micros() <= TIMEBOX_MAX_SECS * 1_000_000 => {}
            _ => return refused(TimeboxRefusal::Until),
        }
        if reason.is_none() {
            return Err(AppError::AdminActReasonRequired);
        }
        if self.act_against_admin_open(tenant_id).await? || self.freeze(tenant_id).await?.is_some()
        {
            return refused(TimeboxRefusal::AgainstAdminOpen);
        }
        // Shared workspaces only: no user root, no in-progress subtree, and
        // under a subtree grant no in-progress node below.
        let mut nodes = BTreeSet::new();
        for g in &req.grants {
            let node = self.resolve(&issuer, NamedKind::Workspace, &g.node).await?;
            if self.root_person_of(tenant_id, node).await?.is_some()
                || self
                    .journal
                    .workspace(tenant_id, node)
                    .await?
                    .is_some_and(|w| w.user_root)
                || self.in_progress_root(tenant_id, node).await?.is_some()
                || (g.covers == oml_domain::access::Covers::Subtree
                    && self
                        .subtree_roots(tenant_id, node)
                        .await?
                        .values()
                        .any(Option::is_some))
            {
                return refused(TimeboxRefusal::NotShared);
            }
            nodes.insert(node.to_string());
        }
        // One open box per actor person per workspace, and one renewal
        // while the first is unsettled.
        let mut renewal = false;
        for act in self
            .journal
            .admin_acts(tenant_id, Some(AdminActState::Effective))
            .await?
            .iter()
            .filter(|a| {
                a.act == AdminActKind::IssueKey
                    && a.timebox()
                    && a.actor_person.as_ref() == Some(&person)
            })
        {
            let boxed: BTreeSet<String> = act
                .target
                .get("timebox_nodes")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            if boxed.is_disjoint(&nodes) {
                continue;
            }
            let open = act
                .target
                .get("until")
                .and_then(Value::as_str)
                .and_then(|u| EventTime::parse(u).ok())
                .is_some_and(|u| u.unix_micros() > now.unix_micros());
            if open {
                return refused(TimeboxRefusal::Open);
            }
            // A first box and its one renewal wait unsettled at most.
            if renewal {
                return refused(TimeboxRefusal::RenewalsExhausted);
            }
            renewal = true;
        }
        let timebox = Timebox {
            nodes: nodes.into_iter().collect(),
            renewal,
        };
        self.issue_key_by(&issuer, req, Issuance::Plain(reason, Some(&timebox)))
            .await
    }

    /// [`App::issue_key`] once the issuer is known and its role checked for
    /// `how`: a plain issuance by an admin or operator key; a session key
    /// for `session_user`, with no value (ADR-0045 d6); or a person's
    /// `<user>-web` issuing a key in its own root, which the web use case
    /// has bounded to reader and contributor keys naming its user (ADR-0045 d9,
    /// d16), the issuer ceiling read as an admin's for those.
    pub(crate) async fn issue_key_by(
        &self,
        issuer: &KeyRecord,
        req: &IssueKeyRequest,
        how: Issuance<'_>,
    ) -> Result<IssuedKey, AppError> {
        // No limit for an admin key; for any other, an expiry later than
        // now and a rate within its bound, checked with the rest of the
        // body, before any lookup (ADR-0079 d1, d4).
        self.check_issued_limits(req)?;
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
                create: g.create,
            });
        }
        // The workspaces this issuance creates in its own act, each with the
        // grants the key's role takes by default over its subtree: an
        // admin's anywhere, a person's own inside the own root, which the
        // user-root check below holds; a session key's or an operator's
        // issuance creates none (ADR-0097 d30).
        let mut ancestries: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
        let mut creations: Vec<WorkspaceRecord> = Vec::with_capacity(req.create_workspaces.len());
        if !req.create_workspaces.is_empty() {
            let admitted = match how {
                Issuance::PersonsOwn(_) => true,
                Issuance::Plain(..) => issuer.role == Role::Admin,
                Issuance::Session(_) => false,
            };
            if !admitted {
                return Err(AppError::VerbCeiling {
                    role: issuer.role,
                    verb: "create_workspace",
                });
            }
            for new in &req.create_workspaces {
                let parent_id = self
                    .resolve_for(&issuer, user.as_ref(), &new.parent)
                    .await?;
                let ancestors = self
                    .journal
                    .workspace_ancestry(tenant_id, parent_id)
                    .await?
                    .ok_or(AppError::NotFound)?;
                let child =
                    self.child_record(&issuer, &new.name, Some(parent_id), None, &ancestors)?;
                let ancestry: Vec<Uuid> = std::iter::once(child.workspace_id)
                    .chain(ancestors.iter().map(|w| w.workspace_id))
                    .collect();
                ancestries.insert(child.workspace_id, ancestry);
                for role in default_grant_roles(req.role) {
                    grants.push(Grant {
                        grant_id: self.ids.new_id(),
                        node: child.workspace_id,
                        role: *role,
                        deny: Vec::new(),
                        covers: Covers::Subtree,
                        create: false,
                    });
                }
                creations.push(child);
            }
        }
        let default_workspace = match &req.default_workspace {
            Some(sent) => Some(self.resolve_for(&issuer, user.as_ref(), sent).await?),
            None => None,
        };
        let crew_id = self
            .resolve_opt(&issuer, NamedKind::Crew, req.crew.as_ref())
            .await?;
        // Steps 1–3: the two-key rule, the issuer ceiling, admissibility.
        // The server issuer issues the four session-key shapes alone, which
        // its caller fixes, read as an admin's ceiling (ADR-0097 d18).
        let ceiling = match how {
            Issuance::PersonsOwn(_) | Issuance::Session(_) => Role::Admin,
            Issuance::Plain(..) => issuer.role,
        };
        // A curator is a reviewer key used by an agent, whose submitter grants
        // the reviewer's admissible set is read as if absent (ADR-0030 d8).
        if let Some(profile) = req.profile
            && !profile_admits(profile, req.role, req.used_by.unwrap_or(UsedBy::Agent))
        {
            return Err(AppError::ProfileMismatch {
                profile: profile.as_str(),
            });
        }
        let checked: Vec<Grant> = if req.profile == Some(KeyProfile::Curator) {
            grants
                .iter()
                .filter(|g| g.role != Role::Submitter)
                .cloned()
                .collect()
        } else {
            grants.clone()
        };
        check_issuance(ceiling, req.role, &checked).map_err(issuance_refusal)?;
        // Step 4: every grant node and deny node is the tenant's, and so is
        // the crew record. A user root this issuance is to create sits
        // directly under `universe` (ADR-0044 d1).
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
        // on that node alone, which the request did not send (ADR-0044 d3);
        // a submitter key recalls nothing and gets none (ADR-0093 d2, as
        // read on 2026-11-20).
        if let Some(u) = &user
            && req.role != Role::Submitter
        {
            grants.push(Grant {
                grant_id: self.ids.new_id(),
                node: u.universe,
                role: Role::Reader,
                deny: Vec::new(),
                covers: Covers::Node,
                create: false,
            });
        }
        // Step 6: a sent default lies in some grant's reach and outside
        // that grant's denies; absent, it is the first grant's node, and
        // never the issuer's (workspaces.md per-grant attenuation).
        let default_workspace = match (default_workspace, &req.default_workspace) {
            (Some(workspace_id), Some(sent)) => {
                let ancestry = self
                    .issuance_ancestry(tenant_id, user.as_ref(), workspace_id)
                    .await?;
                let covered = grants
                    .iter()
                    .any(|g| g.reaches(&ancestry) && !g.deny.iter().any(|d| ancestry.contains(d)));
                if !covered {
                    return Err(AppError::DefaultUncovered {
                        workspace: sent.clone(),
                    });
                }
                Some(workspace_id)
            }
            _ => grants.first().map(|g| g.node),
        };

        // Then a contributor key's shape, over the grants the request sent: a
        // reader and a submitter grant, which the server never adds (ADR-0093
        // d2; request.validation, reason contributor_grants).
        let sent_roles: Vec<Role> = req
            .grants
            .iter()
            .map(|g| g.role)
            .chain(
                creations
                    .iter()
                    .flat_map(|_| default_grant_roles(req.role).iter().copied()),
            )
            .collect();
        check_contributor_grants(req.role, &sent_roles).map_err(issuance_refusal)?;

        // Every admin key belongs to a person, after the issuance's own
        // refusals (ADR-0097 d21).
        let admin_person = self.admin_key_person(&issuer, req, &how).await?;

        // Step 7: with `connect_code`, the key can hold a connection — a
        // role other than admin or operator, `used_by` other than service,
        // and a deployment that sets `oauth.issuer` (ADR-0031 d7).
        let oauth = if req.connect_code {
            if matches!(req.role, Role::Admin | Role::Operator | Role::Maintenance) {
                return Err(connect_refused(ConnectRefused::Role));
            }
            if req.used_by == Some(UsedBy::Service) {
                return Err(connect_refused(ConnectRefused::UsedByService));
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
            default_workspace,
            crew_id,
            user.as_ref().map(|u| u.id),
            Some(issuer.key_id),
        );
        // The person the issuer counts as now, kept for the key's life, so a
        // relink of the issuer later moves nothing a fix's settlement reads
        // (ADR-0097, amendment of 2026-11-27, answer 17; #508 N1).
        record.issuer_person = self.counted_person_of(&issuer).await?;
        // A maintenance key carries a request rate, its role's when the
        // issuance sends none (ADR-0097 d18).
        if record.role == Role::Maintenance && record.requests_per_minute.is_none() {
            record.requests_per_minute = Some(MAINTENANCE_REQUESTS_PER_MINUTE);
        }
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
        let prev = match how {
            Issuance::PersonsOwn(head) => head.clone(),
            Issuance::Plain(..) | Issuance::Session(_) => self.head_event(tenant_id).await?,
        };
        // A key that could read, write, approve or ratify, by an admin key:
        // a widening act (ADR-0097 d5).
        if let Issuance::Plain(reason, timebox) = how
            && issuer.role == Role::Admin
        {
            let issued = self
                .issue_as_admin_act(
                    &issuer,
                    req,
                    record,
                    grants,
                    &creations,
                    oauth,
                    prev.as_ref(),
                    (reason, timebox),
                    user_ref.as_ref(),
                    admin_person.as_ref(),
                )
                .await?;
            return Ok(IssuedKey {
                value,
                default_workspace,
                crew_id,
                user: user_ref,
                guardrail_scope,
                ..issued
            });
        }
        // The workspaces the issuance creates lead its chain, so the key's
        // default and grants name nodes that exist (ADR-0097 d30).
        let mut created = Vec::with_capacity(creations.len());
        let mut created_last = prev.clone();
        for workspace in &creations {
            let event = self.seal(
                created_last.as_ref(),
                tenant_id,
                event_types::WORKSPACE_CREATE,
                &issuer.principal,
                Some(issuer.key_id),
                Self::workspace_create_payload(workspace),
            )?;
            created_last = Some(event.clone());
            created.push((
                event,
                MetadataFold::WorkspaceCreated(workspace.clone()),
                self.ceiling_for(tenant_id, workspace.namespace).await?,
            ));
        }
        let event = self.seal(
            created_last.as_ref(),
            tenant_id,
            event_types::KEY_ISSUED,
            &issuer.principal,
            Some(issuer.key_id),
            Self::key_issued_payload(&record, req.used_by),
        )?;
        let mut last = event.clone();
        // A key of an account counts on its `keys` meter, a session key
        // never; the commit holds the account's ceiling (ADR-0071 d2, d3).
        let charged = record
            .user
            .filter(|_| record.credential != Credential::Session);
        let ceiling = self.ceiling_for(tenant_id, charged).await?;
        if oauth.is_some() || !creations.is_empty() {
            // One chain: a refusal of any link journals none, so no key
            // outlives a failed issuance and none needs revoking; the
            // workspaces it creates commit with the key and its grants, or
            // not at all (ADR-0097 d30).
            let issued_seq = event.envelope.seq;
            let mut links = Vec::with_capacity(created.len() + grants.len() + 2);
            let mut ceilings = Vec::with_capacity(created.len() + 1);
            for (event, fold, ceiling) in created {
                links.push((event, fold));
                ceilings.push(ceiling);
            }
            links.push((event, MetadataFold::KeyIssued(record)));
            ceilings.push(ceiling);
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
            let shown = match oauth {
                Some(oauth) => {
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
                    Some(shown)
                }
                None => None,
            };
            let mut reqs: Vec<CommitRequest> = links
                .into_iter()
                .map(|(event, fold)| commit_request(event, fold, None))
                .collect();
            for (req, ceiling) in reqs.iter_mut().zip(ceilings) {
                req.ceiling = ceiling;
            }
            self.commit_requests(reqs).await?;
            return Ok(IssuedKey {
                act_id: None,
                pending_act: None,
                key_id,
                value,
                role: req.role,
                principal: req.principal.clone(),
                used_by: req.used_by,
                default_workspace,
                name: req.name.clone(),
                crew_id,
                user: user_ref.clone(),
                guardrail_scope,
                expiry: req.expiry.clone(),
                requests_per_minute: req.requests_per_minute,
                access: KeyAccess {
                    issued_seq,
                    grants: stored,
                    denials: Vec::new(),
                    lift_epochs: Vec::new(),
                    public_nodes: Vec::new(),
                },
                connect_code: shown,
            });
        }
        let receipt = self
            .commit_charged(event, MetadataFold::KeyIssued(record), None, ceiling)
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
            act_id: None,
            pending_act: None,
            key_id,
            value,
            role: req.role,
            principal: req.principal.clone(),
            used_by: req.used_by,
            default_workspace,
            name: req.name.clone(),
            crew_id,
            user: user_ref.clone(),
            guardrail_scope,
            expiry: req.expiry.clone(),
            requests_per_minute: req.requests_per_minute,
            access: KeyAccess {
                issued_seq: receipt.seq,
                grants: stored,
                denials: Vec::new(),
                lift_epochs: Vec::new(),
                public_nodes: Vec::new(),
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
        self.deny_key_with_reason(bearer, key, node, None).await
    }

    /// [`App::deny_key`] with the admin's reason, scanned and journalled in
    /// `key.denied` when sent, in the field shape of ADR-0095 d1's
    /// structure acts (ADR-0097 d27 item 2, completing ADR-0096 d4).
    ///
    /// # Errors
    /// As [`App::deny_key`], and a reason the scan vetoes.
    pub async fn deny_key_with_reason(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
        node: Reference,
        reason: Option<&str>,
    ) -> Result<DenialRecord, AppError> {
        let admin = self.key_act_caller(bearer.into(), "deny_key").await?;
        require_role(&admin, Role::Admin, "deny")?;
        let admin = admin.key;
        self.deny_key_by(&admin, key, node, Head::AtSeal, reason)
            .await
    }

    /// [`App::deny_key`] once the issuer is known: an admin key, or a
    /// person's `<user>-web` session key cutting off a key naming its user,
    /// which the web use case has checked (ADR-0045 d9, d13, d16).
    pub(crate) async fn deny_key_by(
        &self,
        admin: &KeyRecord,
        key: Reference,
        node: Reference,
        head: Head<'_>,
        reason: Option<&str>,
    ) -> Result<DenialRecord, AppError> {
        let admin = admin.clone();
        let tenant_id = admin.tenant_id;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        let node = self.resolve(&admin, NamedKind::Workspace, &node).await?;
        let Some(target) = self.journal.key(tenant_id, key_id).await? else {
            return Err(AppError::NotFound);
        };
        if self.journal.workspace(tenant_id, node).await?.is_none() {
            return Err(AppError::NotFound);
        }
        if target.role == Role::Admin {
            return Err(AppError::DenialOnAdminKey);
        }
        let denial = KeyDenial {
            denial_id: self.ids.new_id(),
            node,
        };
        let mut payload = json!({
            "denial_id": denial.denial_id.to_string(),
            "key_id": key_id.to_string(),
            "workspace_node_id": node.to_string(),
        });
        with_reason(self, tenant_id, &mut payload, reason)?;
        let prev = self.head_at(tenant_id, head).await?;
        // An admin's denial is a narrowing admin act (ADR-0097 d3); a
        // person's own cut-off of a key naming them is not.
        let act = self
            .admin_act_of(
                &admin,
                oml_domain::admin_act::AdminActKind::DenyKey,
                json!({
                    "key_id": key_id.to_string(),
                    "workspace_node_id": node.to_string(),
                    // Denying a reviewer person's key is offensive
                    // narrowing, loud at once (ADR-0097 d24).
                    "offensive": target.role == Role::Reviewer
                        && target.used_by == UsedBy::Person,
                }),
                reason,
                prev.as_ref(),
            )
            .await?;
        let prev = act.as_ref().map(|a| a.event.clone()).or(prev);
        let epoch = prev.as_ref().map_or(1, |p| p.envelope.seq + 1);
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("epoch".to_owned(), json!(epoch));
        }
        let payload = admin_acts::named(payload, act.as_ref());
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_DENIED,
            &admin.principal,
            Some(admin.key_id),
            payload,
        )?;
        let receipt = self
            .commit_with_act(
                act,
                vec![(
                    event,
                    MetadataFold::KeyDenied {
                        key_id,
                        denial: denial.clone(),
                        created_by: admin.key_id,
                    },
                )],
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
    /// `workspace.create` journals its `crew_id` and the fold stores it, so
    /// the workspace read serves the record from the store (event-catalog.md
    /// `workspace.create`; ADR-0025 d14). The answer carries the node's path
    /// derived from the parent's stored ancestry, as the read derives it. A
    /// name the namespace already holds, `universe` included, is checked
    /// last, by the store's unique index: `name.taken`, and nothing is
    /// journalled (ADR-0026 d5, d6). A contributor user key creates too, under a
    /// parent inside its own root where its effective access confers
    /// submitter, the root itself when `parent` is omitted, by the refusal
    /// order on the parent; the node's name lands in that root's namespace
    /// (ADR-0044 d5, d6). A parent that is `frozen` refuses the child as
    /// `workspace.frozen` after both lookups and before the name, so a frozen
    /// node never acquires an unfrozen descendant (ADR-0009 d4, d5).
    pub async fn create_workspace(
        &self,
        bearer: impl Into<Bearer<'_>>,
        name: &Name,
        parent: Option<Reference>,
        crew: Option<Reference>,
    ) -> Result<CreatedWorkspace, AppError> {
        self.create_workspace_with_reason(bearer, name, parent, crew, None)
            .await
    }

    /// [`Self::create_workspace`] with the caller's `reason`, scanned as a
    /// verdict's reason is and journalled with `workspace.create` (ADR-0095
    /// d1).
    pub async fn create_workspace_with_reason(
        &self,
        bearer: impl Into<Bearer<'_>>,
        name: &Name,
        parent: Option<Reference>,
        crew: Option<Reference>,
        reason: Option<&str>,
    ) -> Result<CreatedWorkspace, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        self.create_workspace_as(&caller, name, parent, crew, false, reason)
            .await
    }

    /// `oml_write`'s `create_workspace` mode (ADR-0095 d3): the create HTTP
    /// admits a contributor user key, and nothing else. Every other key, an
    /// admin key, a tenant key that names no user, a submitter, reviewer or
    /// reader key, is refused before any lookup as `access.verb_ceiling`,
    /// `create_is_own_root_only`; the admitted key then creates exactly as
    /// [`Self::create_workspace`] does, crew refused as there.
    pub async fn create_own_workspace(
        &self,
        bearer: impl Into<Bearer<'_>>,
        name: &Name,
        parent: Option<Reference>,
        reason: Option<&str>,
    ) -> Result<CreatedWorkspace, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        // Beside the own-root rule, a grant carrying `create` reaches a
        // shared parent (ADR-0097 d31).
        if (caller.key.role != Role::Contributor || caller.key.user.is_none())
            && self
                .create_flagged(&caller, parent.as_ref())
                .await?
                .is_none()
        {
            return Err(AppError::CreateIsOwnRootOnly {
                role: caller.key.role,
            });
        }
        self.create_workspace_as(&caller, name, parent, None, false, reason)
            .await
    }

    /// [`Self::create_workspace`] for an authenticated caller. With
    /// `in_progress`, a workspace no ancestor of which is in progress is
    /// marked `in_progress` by the caller's key in the commit that creates
    /// it: an import's new target (ADR-0030 d1, amended 2026-11-18;
    /// ADR-0084 I16).
    pub(crate) async fn create_workspace_as(
        &self,
        caller: &Caller,
        name: &Name,
        parent: Option<Reference>,
        crew: Option<Reference>,
        in_progress: bool,
        reason: Option<&str>,
    ) -> Result<CreatedWorkspace, AppError> {
        // A new workspace is not a memory read or write, so a previous value
        // creates none, an import's new target included (ADR-0096 d13).
        caller.require_current_value("create_workspace")?;
        let key = &caller.key;
        // A key whose effective access at the parent comes from a grant
        // carrying `create` creates a child there, in a shared tree too;
        // under a node grant the child gets the same grant (ADR-0097 d31).
        let flagged = self.create_flagged(caller, parent.as_ref()).await?;
        if flagged.is_some() && crew.is_some() {
            return Err(AppError::VerbCeiling {
                role: key.role,
                verb: "set_workspace_crew",
            });
        }
        // A user key with the contributor role creates inside its own root,
        // by its effective access on the parent, since it acts on a tree it
        // must read; every other key by the admin key role (ADR-0044 d6;
        // ADR-0093 d6).
        let parent_id = if let Some((parent_id, _)) = &flagged {
            Some(*parent_id)
        } else {
            match key.user {
                Some(root) if key.role == Role::Contributor => {
                    // Crew metadata stays a governed admin act (ADR-0044 d6).
                    if crew.is_some() {
                        return Err(AppError::VerbCeiling {
                            role: key.role,
                            verb: "set_workspace_crew",
                        });
                    }
                    let parent_id = match &parent {
                        Some(parent) => self.resolve(key, NamedKind::Workspace, parent).await?,
                        None => root,
                    };
                    self.require_access(caller, parent_id, Role::Submitter, "create_workspace")
                        .await?;
                    Some(parent_id)
                }
                _ => {
                    require_role(caller, Role::Admin, "create_workspace")?;
                    self.resolve_opt(key, NamedKind::Workspace, parent.as_ref())
                        .await?
                }
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
                .workspace_ancestry(key.tenant_id, parent)
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
        // A flagged create under a frozen parent is refused by the flag's
        // own reason (ADR-0097 d31).
        if flagged.is_some()
            && let Some(parent) = ancestors.first()
            && frozen_refuses(parent.status, FrozenOp::CreateChild)
        {
            return Err(AppError::ParentFrozen {
                workspace_id: parent.workspace_id,
            });
        }
        let workspace = self.child_record(key, name, parent_id, crew_id, &ancestors)?;
        let namespace = workspace.namespace;
        // A node below a user root counts on that account's `workspaces`
        // meter, whoever creates it, and the commit holds the account's
        // ceiling: past its limit the create is `limit.quota` and nothing is
        // journalled (ADR-0095 d4; ADR-0071 d3).
        let ceiling = self.ceiling_for(key.tenant_id, namespace).await?;
        let mut payload = Self::workspace_create_payload(&workspace);
        with_reason(self, key.tenant_id, &mut payload, reason)?;
        // A flagged child in the shared tree is charged to no account, so
        // its event names the grant that admitted it, which doctor's
        // `workspace.flagged_creates` counts (ADR-0097 d31, the operator's
        // decision of 2026-11-27).
        if let Some((_, grant)) = &flagged
            && namespace.is_none()
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert(
                "create_grant_id".to_owned(),
                json!(grant.grant_id.to_string()),
            );
        }
        // An admin's create is a neutral admin act (ADR-0097 d9).
        let act = if key.role == Role::Admin {
            self.admin_act_of(
                key,
                oml_domain::admin_act::AdminActKind::CreateWorkspace,
                json!({
                    "workspace_id": workspace.workspace_id.to_string(),
                    "name": workspace.name.as_str(),
                    "parent_id": workspace.parent_id.map(|p| p.to_string()),
                }),
                reason,
                prev.as_ref(),
            )
            .await?
        } else {
            None
        };
        let payload = admin_acts::named(payload, act.as_ref());
        let event = self.seal(
            act.as_ref().map(|a| &a.event).or(prev.as_ref()),
            key.tenant_id,
            event_types::WORKSPACE_CREATE,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let mut links = Vec::with_capacity(3);
        let mark = in_progress && in_progress_root_in(&ancestors).is_none();
        let mode_event = if mark {
            Some(self.seal(
                Some(&event),
                key.tenant_id,
                event_types::WORKSPACE_MODE_SET,
                &key.principal,
                Some(key.key_id),
                json!({
                    "workspace_id": workspace.workspace_id.to_string(),
                    "mode": "in_progress",
                    "from": "standard",
                }),
            )?)
        } else {
            None
        };
        let tail = mode_event.clone().unwrap_or_else(|| event.clone());
        links.push((event, MetadataFold::WorkspaceCreated(workspace.clone())));
        if let Some(mode_event) = mode_event {
            links.push((
                mode_event,
                MetadataFold::WorkspaceModeSet {
                    workspace_id: workspace.workspace_id,
                    in_progress: true,
                    held_free: Vec::new(),
                },
            ));
        }
        // Under a node grant the child carries the same grant, `create`
        // included, in the create's commit (ADR-0097 d31).
        if let Some((_, grant)) = &flagged
            && grant.covers == Covers::Node
        {
            let child = Grant {
                grant_id: self.ids.new_id(),
                node: workspace.workspace_id,
                role: grant.role,
                deny: Vec::new(),
                covers: Covers::Node,
                create: true,
            };
            let event = self.grant_event(&tail, key, key.key_id, &child)?;
            links.push((
                event,
                MetadataFold::GrantIssued {
                    key_id: key.key_id,
                    grant: child,
                },
            ));
        }
        let mut requests: Vec<CommitRequest> = act
            .iter()
            .map(admin_acts::BegunAct::link)
            .chain(links)
            .map(|(event, fold)| commit_request(event, fold, None))
            .collect();
        let created = usize::from(act.is_some());
        requests[created].ceiling = ceiling;
        self.commit_requests(requests).await?;
        let mut workspace = workspace;
        workspace.in_progress = mark;
        let ancestry: Vec<WorkspaceRecord> = std::iter::once(workspace.clone())
            .chain(ancestors)
            .collect();
        Ok(CreatedWorkspace {
            path: derived_path(&ancestry),
            workspace,
            crew,
        })
    }

    /// The parent and the grant carrying `create` that admit `caller`'s
    /// create under `parent`, the key's own root when omitted: `None` for an
    /// admin key, which keeps its own create (ADR-0097 d29), for a key that
    /// holds no flagged grant, and where no flagged grant survives at the
    /// parent, a global denial included (d31).
    async fn create_flagged(
        &self,
        caller: &Caller,
        parent: Option<&Reference>,
    ) -> Result<Option<(Uuid, Grant)>, AppError> {
        let key = &caller.key;
        if key.role == Role::Admin || !caller.grants().iter().any(|g| g.create) {
            return Ok(None);
        }
        let parent_id = match (parent, key.user) {
            (Some(parent), _) => self.resolve(key, NamedKind::Workspace, parent).await?,
            (None, Some(root)) => root,
            (None, None) => return Ok(None),
        };
        let ancestry: Vec<Uuid> = self
            .journal
            .workspace_ancestry(key.tenant_id, parent_id)
            .await?
            .unwrap_or_default()
            .iter()
            .map(|w| w.workspace_id)
            .collect();
        Ok(
            oml_domain::access::create_grant(caller.grants(), caller.denials(), &ancestry)
                .cloned()
                .map(|grant| (parent_id, grant)),
        )
    }

    /// The record of a new child `name` under the parent whose ancestry,
    /// parent first, is `ancestors` (empty for a root), checked as every
    /// create checks it before anything is journalled: `chat` reserved for
    /// a key a person holds in a user root's namespace (ADR-0063 d8); a
    /// `frozen` parent refuses the child (ADR-0009 d4, d5); a name equal to
    /// its namespace's root is taken (ADR-0044 d5). The child starts in
    /// `development`, mode `standard` (ADR-0095 d3).
    fn child_record(
        &self,
        key: &KeyRecord,
        name: &Name,
        parent_id: Option<Uuid>,
        crew_id: Option<Uuid>,
        ancestors: &[WorkspaceRecord],
    ) -> Result<WorkspaceRecord, AppError> {
        // `chat` is reserved in every user root's namespace for a key a
        // person holds, at any depth, so an agent never takes the name
        // before the person opens their chat workspace; refused before
        // anything is journalled (ADR-0063 d8).
        if let Some(parent) = ancestors.first()
            && parent.user().is_some()
            && name.as_str() == CHAT_WORKSPACE
            && key.used_by != UsedBy::Person
        {
            return Err(AppError::ChatScopeHumanOnly {
                used_by: key.used_by,
            });
        }
        if let Some(parent) = ancestors.first()
            && frozen_refuses(parent.status, FrozenOp::CreateChild)
        {
            return Err(AppError::WorkspaceFrozen {
                workspace_id: parent.workspace_id,
                op: FrozenOp::CreateChild,
            });
        }
        // A node beneath a user root takes its name in that root's
        // namespace, where `universe` and the user's own name are reserved
        // (ADR-0044 d5).
        let namespace = ancestors.first().and_then(WorkspaceRecord::user);
        if let Some(root) = namespace
            && ancestors
                .iter()
                .any(|w| w.workspace_id == root && &w.name == name)
        {
            return Err(AppError::NameTaken {
                kind: NamedKind::Workspace,
                name: name.clone(),
            });
        }
        let workspace = WorkspaceRecord {
            in_progress: false,
            public: false,
            public_act_id: None,
            workspace_id: self.ids.new_id(),
            tenant_id: key.tenant_id,
            parent_id,
            name: name.clone(),
            status: WorkspaceStatus::Development,
            crew: None,
            crew_id,
            user_root: false,
            namespace,
        };
        Ok(workspace)
    }

    /// One workspace as the calling key may see it, at reader effective
    /// access by the refusal order and otherwise absent (http-api.md
    /// workspaces; ADR-0025 d14): the node and its derived path, the crew
    /// record it names, the immediate children the key can read, the verbs
    /// its effective roles on the node confer, and the permission epoch of
    /// the access this request read. An admin key reads it by its key role,
    /// with no grant and no denial entering: every child, no effective verb,
    /// and nothing of the node's claims (roles-and-keys.md permission matrix,
    /// footnote 6; ADR-0026 d15). The node, by id or name, or else the key's
    /// journalled default, is resolved only after step 1 of the refusal
    /// order, as recall resolves it (roles-and-keys.md refusal order;
    /// mcp-plugin.md `oml_workspace`; ADR-0026 d4).
    pub async fn workspace_info(
        &self,
        bearer: impl Into<Bearer<'_>>,
        node: Option<Reference>,
    ) -> Result<WorkspaceInfo, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let structure = caller.key.role == Role::Admin;
        if !structure {
            require_admissible(&caller.key, Role::Reader, "workspace_info")?;
        }
        let node = self.target_workspace(&caller.key, node.as_ref()).await?;
        let tenant_id = caller.key.tenant_id;
        let ancestry = self
            .journal
            .workspace_ancestry(tenant_id, node)
            .await?
            .unwrap_or_default();
        let ids: Vec<Uuid> = ancestry.iter().map(|w| w.workspace_id).collect();
        let effective = caller.effective(&ids);
        if !structure {
            resolve(caller.key.role, &effective, Role::Reader)
                .map_err(|refusal| access_refusal(refusal, node, "workspace_info"))?;
        }
        let workspace = ancestry.first().cloned().ok_or(AppError::NotFound)?;
        let mut children = Vec::new();
        // A child's ancestry is the node's with the child in front.
        for child in self.journal.workspace_children(tenant_id, node).await? {
            let child_ancestry: Vec<Uuid> = std::iter::once(child.workspace_id)
                .chain(ids.iter().copied())
                .collect();
            if structure || caller.effective(&child_ancestry).contains(&Role::Reader) {
                children.push(child);
            }
        }
        let crew = match workspace.crew_id {
            Some(crew_id) => self.journal.crew(tenant_id, crew_id).await?,
            None => None,
        };
        let pending_transitions = self.live_transitions(&caller.key, &workspace).await?;
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
        let approved_by_kind = self.journal.approved_by_kind(tenant_id, node).await?;
        // The node's in-progress root, read off the ancestry (ADR-0030 d13).
        let in_progress_root = ancestry.iter().find(|w| w.in_progress).cloned();
        // The node's federation field, served beside the policy (ADR-0072
        // d5).
        let federation = crate::federation::WorkspaceFederation {
            accept_federated: self
                .journal
                .federation_declining(tenant_id, &[node])
                .await?
                .is_empty(),
        };
        // The node's `public` flag, and to an admin key the set that waits
        // on a second admin person (ADR-0098 d9).
        let mut public = self
            .public_flags(tenant_id, &[&workspace])
            .await?
            .remove(&node)
            .unwrap_or_default();
        if structure {
            public.pending_act_id = self
                .pending_public_set(tenant_id, node)
                .await?
                .map(|a| a.act_id);
        }
        Ok(WorkspaceInfo {
            public,
            in_progress_root,
            stale_versions,
            approved_by_kind,
            path: derived_path(&ancestry),
            policy: review_policy(),
            federation,
            pending_transitions,
            brief,
            workspace,
            crew,
            children,
            // The field lists grant-role verbs, and an admin key holds none.
            my_effective_verbs: if structure {
                Vec::new()
            } else {
                let in_own_root = session_reviewer_starts_claims(&caller.key)
                    && self.in_own_root(&caller.key, node).await?;
                let in_key_root = caller.key.user.is_some_and(|root| ids.contains(&root));
                let mut verbs = listed_verbs(&caller.key, &effective, in_own_root, in_key_root);
                // A grant carrying `create` that survives here lists the
                // create its key may make (ADR-0097 d31).
                if !verbs.contains(&"create_workspace")
                    && oml_domain::access::create_grant(caller.grants(), caller.denials(), &ids)
                        .is_some()
                {
                    verbs.push("create_workspace");
                }
                verbs
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
    /// or foreign one is refused (http-api.md workspaces and pagination). The
    /// read is one store walk and one crew read per crew the page names.
    pub async fn workspace_tree(
        &self,
        bearer: impl Into<Bearer<'_>>,
        node: Reference,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<TreePage, AppError> {
        // A maintenance key reads the tree's structure as an admin key does
        // (ADR-0097 d18): node names and counts, no claim.
        let caller = self
            .authenticate_bearer_admitting(bearer.into(), true)
            .await?;
        let structure = matches!(caller.key.role, Role::Admin | Role::Maintenance);
        if !structure {
            require_admissible(&caller.key, Role::Reader, "workspace_tree")?;
        }
        let now = self.clock.now();
        let after = cursor
            .map(|cursor| PathCursor::resume(cursor, caller.key.key_id, &now))
            .transpose()?;
        let tenant_id = caller.key.tenant_id;
        let node = self
            .resolve(&caller.key, NamedKind::Workspace, &node)
            .await?;
        self.observer.workspace(node);
        let readable: Option<BTreeSet<Uuid>> = if structure {
            None
        } else {
            self.require_access(&caller, node, Role::Reader, "workspace_tree")
                .await?;
            Some(
                self.nodes_with(&caller, node, Role::Reader)
                    .await?
                    .into_iter()
                    .collect(),
            )
        };
        let Some(ancestry) = self.journal.workspace_ancestry(tenant_id, node).await? else {
            return Err(AppError::NotFound);
        };
        // The subtree lists a parent before its children, so each node's
        // names, from `universe` down, are its parent's and its own.
        let mut names: BTreeMap<Uuid, Vec<Name>> = BTreeMap::from([(
            node,
            ancestry.iter().rev().map(|w| w.name.clone()).collect(),
        )]);
        let mut tree: Vec<(Vec<Name>, WorkspaceRecord)> = Vec::new();
        for workspace in self.journal.workspace_subtree(tenant_id, node).await? {
            let id = workspace.workspace_id;
            if id != node {
                let mut own = workspace
                    .parent_id
                    .and_then(|p| names.get(&p))
                    .cloned()
                    .ok_or_else(|| {
                        StoreError::Backend(format!(
                            "the subtree of {node} lists {id} before its parent"
                        ))
                    })?;
                own.push(workspace.name.clone());
                names.insert(id, own);
            }
            if readable.as_ref().is_none_or(|nodes| nodes.contains(&id)) {
                tree.push((names[&id].clone(), workspace));
            }
        }
        // A node's names are a prefix of its children's, and names are
        // unique in the tenant, so no two nodes tie.
        tree.sort_by(|(a, _), (b, _)| a.cmp(b));
        // One node past the page says whether another page exists.
        let mut page: Vec<(Vec<Name>, WorkspaceRecord)> = tree
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
        // Many nodes may name one crew, which is read once; each public
        // node's act is read for its flag (ADR-0098 d9).
        let mut crews: BTreeMap<Uuid, Option<CrewRecord>> = BTreeMap::new();
        let mut flags = self
            .public_flags(tenant_id, &page.iter().map(|(_, w)| w).collect::<Vec<_>>())
            .await?;
        let mut items = Vec::with_capacity(page.len());
        for (names, workspace) in page {
            let path = names.iter().map(Name::as_str).collect::<Vec<_>>().join(".");
            let crew = match workspace.crew_id {
                None => None,
                Some(crew_id) => match crews.entry(crew_id) {
                    Entry::Occupied(read) => read.get().clone(),
                    Entry::Vacant(unread) => unread
                        .insert(self.journal.crew(tenant_id, crew_id).await?)
                        .clone(),
                },
            };
            items.push(TreeNode {
                public: flags.remove(&workspace.workspace_id).unwrap_or_default(),
                workspace,
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
        let key = self
            .admin_caller(Bearer::Key(api_key), "create_crew")
            .await?
            .key;
        let crew = CrewRecord {
            crew_id: self.ids.new_id(),
            tenant_id: key.tenant_id,
            name: req.name.clone(),
            charter_uri: req.charter_uri.clone(),
            external_manager: req.external_manager.clone(),
        };
        // Creating a crew is a neutral admin act (ADR-0097 d9).
        self.commit_as_act(
            &key,
            oml_domain::admin_act::AdminActKind::CreateCrew,
            json!({ "crew_id": crew.crew_id.to_string(), "name": crew.name.as_str() }),
            None,
            event_types::CREW_CREATED,
            json!({
                "crew_id": crew.crew_id.to_string(),
                "name": crew.name.as_str(),
                "charter_uri": crew.charter_uri,
                "external_manager": crew.external_manager,
                "created_by_key_id": key.key_id.to_string(),
            }),
            MetadataFold::CrewCreated(crew.clone()),
        )
        .await?;
        Ok(crew)
    }

    /// One crew record, by id or name, admin by key role; one the tenant
    /// does not hold is absent (http-api.md crews; ADR-0026 d8).
    pub async fn crew(&self, api_key: &str, crew: Reference) -> Result<CrewRecord, AppError> {
        let key = self.admin_caller(Bearer::Key(api_key), "crew").await?.key;
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
        let key = self.admin_caller(Bearer::Key(api_key), "crews").await?.key;
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
        let admin = self.admin_caller(bearer.into(), "key").await?.key;
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
        // A key issued before v0.8.0 journals its holder and scope in the
        // former names; it is read in the current ones (ADR-0091 d4).
        let issued = read_as_current(&issued.envelope)?;
        let used_by = match payload_str(&issued.payload, "used_by").as_deref() {
            None => None,
            Some("agent") => Some(UsedBy::Agent),
            Some("person") => Some(UsedBy::Person),
            Some("service") => Some(UsedBy::Service),
            Some(other) => {
                return Err(StoreError::Backend(format!(
                    "key {key_id}'s key.issued journals used_by {other:?}"
                ))
                .into());
            }
        };
        let user = self.user_ref(tenant_id, record.user).await?;
        let guardrail_scope = GuardrailScope::of_issuance(issued.payload.get("guardrail_scope"))
            .ok_or_else(|| {
                StoreError::Backend(format!(
                    "key {key_id}'s key.issued journals guardrail_scope {}",
                    issued.payload["guardrail_scope"]
                ))
            })?;
        let public_reach = self.public_reach_of(&record, &access).await?;
        Ok(KeyDetail {
            key: record,
            user,
            used_by,
            guardrail_scope,
            access,
            public_reach,
        })
    }

    /// The user root of `user`, admin by key role: the root's record when a
    /// user root holds the name, `None` when no workspace of the shared
    /// namespace does. A shared workspace holding the name is `name.taken`,
    /// as an issuance naming that user would be. It journals nothing; only
    /// the issuance of the first key naming a user creates its root
    /// (ADR-0044 d1, d2).
    pub async fn user_root(
        &self,
        api_key: &str,
        user: &Name,
    ) -> Result<Option<WorkspaceRecord>, AppError> {
        let admin = self
            .admin_caller(Bearer::Key(api_key), "user_root")
            .await?
            .key;
        let Some(id) = self
            .journal
            .resolve_name(admin.tenant_id, NamedKind::Workspace, user)
            .await?
        else {
            return Ok(None);
        };
        match self.journal.workspace(admin.tenant_id, id).await? {
            Some(root) if root.user_root => Ok(Some(root)),
            _ => Err(AppError::NameTaken {
                kind: NamedKind::Workspace,
                name: user.clone(),
            }),
        }
    }

    /// The calling key's own record, which any live key reads, whatever its
    /// role: its row, its tenant, its default workspace with its derived path
    /// and its crew record, each by id and name, and its stored grants. It
    /// omits the key's denials, which name subtrees the key may not read,
    /// and journals nothing, since it reads no claim. The references are one
    /// store read after the two every request makes (ADR-0039 d15).
    pub async fn whoami(&self, api_key: &str) -> Result<Whoami, AppError> {
        let (key, access) = self.key_and_access(api_key).await?;
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
                default_workspace: None,
                crew: None,
                user: None,
                grants: Vec::new(),
                denied: None,
                without_recall: false,
                rotation_grace_until: None,
            });
        };
        // The person's `<user>-web` alone is told, and only whether: the
        // key acts it is refused name the same fact (ADR-0039 d15 as
        // amended by ADR-0070; #343).
        let denied = auth::is_web_session_key(&key).then_some(!access.denials.is_empty());
        // The key read above holds the row: a tenant whose references read
        // nothing for it has lost it since, so fail closed.
        let references = self
            .journal
            .key_references(key.tenant_id, key.key_id)
            .await?
            .ok_or(AppError::Unauthorized)?;
        let without_recall = self.without_recall(&key, Some(&access)).await?;
        // Only a bearer key has a value to rotate, so only its whoami reads
        // the grace (ADR-0096 d13).
        let rotation_grace_until = if key.credential == Credential::Bearer {
            self.rotation_grace(&key).await?
        } else {
            None
        };
        Ok(Whoami {
            key,
            tenant: Some(references.tenant),
            default_workspace: references.default_workspace,
            crew: references.crew,
            user: references.user,
            grants: access.grants,
            denied,
            without_recall,
            rotation_grace_until,
        })
    }

    /// Whether `key`, holding `access`, is a contributor without recall: a
    /// submitter grant on a node no reader grant of it reaches (ADR-0093 d3,
    /// as read on 2026-11-20), decided by the domain's one definition. A
    /// session key never is: the server issues a person's `<user>-web`,
    /// `-review`, `-curate` and `-admin`, their grants follow ADR-0044 d7 by
    /// design and no person reissues one (ADR-0093 d3, the reading of
    /// 2026-11-20 on `<user>-curate`). A submitter grant's ancestry is read
    /// only where no reader grant sits on its own node, so a key with a
    /// reader beside each submitter grant, every session key and every key
    /// of another role, costs no read.
    pub(crate) async fn without_recall(
        &self,
        key: &KeyRecord,
        access: Option<&KeyAccess>,
    ) -> Result<bool, AppError> {
        let Some(access) = access else {
            return Ok(false);
        };
        if key.credential == Credential::Session {
            return Ok(false);
        }
        let grants: Vec<Grant> = access.grants.iter().map(|g| g.grant.clone()).collect();
        let mut ancestries = BTreeMap::new();
        for node in recall_ancestries_needed(key.role, &grants) {
            ancestries.insert(node, self.ancestry(key.tenant_id, node).await?);
        }
        Ok(contributor_without_recall(key.role, &grants, &ancestries))
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
        let key = self
            .admin_caller(Bearer::Key(api_key), "update_crew")
            .await?
            .key;
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
        // Crew metadata is a neutral admin act, the manager included while
        // no served check reads it (ADR-0097 d9).
        self.commit_as_act(
            &key,
            oml_domain::admin_act::AdminActKind::UpdateCrew,
            json!({ "crew_id": updated.crew_id.to_string(), "name": updated.name.as_str() }),
            None,
            event_types::CREW_UPDATED,
            json!({
                "crew_id": updated.crew_id.to_string(),
                "charter_uri": updated.charter_uri,
                "external_manager": updated.external_manager,
                "updated_by_key_id": key.key_id.to_string(),
            }),
            MetadataFold::CrewUpdated(updated.clone()),
        )
        .await?;
        Ok(updated)
    }

    /// Set a workspace's mode (ADR-0030 d1, d6): a reviewer key a person holds,
    /// with reviewer effective access on the node, and no other key, refused
    /// before any lookup. `in_progress` is refused on `universe`, a node not
    /// `development`, or one with a marked ancestor or descendant;
    /// `standard` on an in-progress root is its commit, refused while any
    /// version in its subtree is held. The chain head is read before the
    /// held test, so a capture landing between the test and the commit moves
    /// the head and the commit conflicts. Setting the mode a node already
    /// holds journals nothing. Journalled as `workspace.mode_set`.
    pub async fn set_workspace_mode(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        mode: WorkspaceMode,
    ) -> Result<WorkspaceRecord, AppError> {
        let caller = self
            .key_act_caller(bearer.into(), "set_workspace_mode")
            .await?;
        let key = &caller.key;
        if !may_set_mode(key.role, key.used_by, key.profile) {
            return Err(AppError::ModeHumanOnly {
                role: key.role,
                used_by: key.used_by,
            });
        }
        let tenant_id = key.tenant_id;
        let workspace_id = self.resolve(key, NamedKind::Workspace, &workspace).await?;
        self.observer.workspace(workspace_id);
        let prev = self.head_event(tenant_id).await?;
        let (node, current, held_free) =
            match self.mode_admitted(&caller, workspace_id, mode).await? {
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
            event_types::WORKSPACE_MODE_SET,
            &key.principal,
            Some(key.key_id),
            json!({
                "workspace_id": workspace_id.to_string(),
                "mode": mode.as_str(),
                "from": current.as_str(),
            }),
        )?;
        // The commit's zero-held test runs again inside the committing
        // transaction, so no held row created without a chain event slips
        // past it (ADR-0030 d6).
        self.commit(
            event,
            MetadataFold::WorkspaceModeSet {
                workspace_id,
                in_progress: mode == WorkspaceMode::InProgress,
                held_free,
            },
            None,
        )
        .await
        .map_err(|e| match e {
            AppError::Store(StoreError::HeldRemaining) => AppError::HeldRemaining { workspace_id },
            other => other,
        })?;
        self.observer.mode_set(mode.as_str());
        Ok(WorkspaceRecord {
            in_progress: mode == WorkspaceMode::InProgress,
            public: false,
            public_act_id: None,
            ..node
        })
    }

    /// Whether `set_workspace_mode` admits `mode` on the node for the
    /// caller now, every check but the commit's, in its order: the key's
    /// shape, reviewer access, then the mark's or the commit's test. The
    /// verb and the pages' offer call this one function (ADR-0045 d28).
    pub(crate) async fn mode_admitted(
        &self,
        caller: &Caller,
        workspace_id: Uuid,
        mode: WorkspaceMode,
    ) -> Result<ModeAdmitted, AppError> {
        const VERB: &str = "workspace_mode";
        let key = &caller.key;
        if !may_set_mode(key.role, key.used_by, key.profile) {
            return Err(AppError::ModeHumanOnly {
                role: key.role,
                used_by: key.used_by,
            });
        }
        let tenant_id = key.tenant_id;
        self.require_access(caller, workspace_id, Role::Reviewer, VERB)
            .await?;
        let ancestry = self
            .journal
            .workspace_ancestry(tenant_id, workspace_id)
            .await?
            .unwrap_or_default();
        let node = ancestry.first().cloned().ok_or(AppError::NotFound)?;
        let current = if node.in_progress {
            WorkspaceMode::InProgress
        } else {
            WorkspaceMode::Standard
        };
        if current == mode {
            return Ok(ModeAdmitted::Already(node));
        }
        let subtree = self
            .journal
            .workspace_subtree(tenant_id, workspace_id)
            .await?;
        let mut held_free = Vec::new();
        match mode {
            WorkspaceMode::InProgress => {
                let marks: Vec<(Uuid, bool)> = ancestry
                    .iter()
                    .map(|w| (w.workspace_id, w.in_progress))
                    .collect();
                let below = subtree
                    .iter()
                    .any(|w| w.workspace_id != workspace_id && w.in_progress);
                let development = subtree
                    .iter()
                    .all(|w| w.status == WorkspaceStatus::Development);
                in_progress::check_mark(&marks, below, development).map_err(|reason| {
                    AppError::InProgressRefused {
                        workspace_id,
                        reason,
                    }
                })?;
            }
            WorkspaceMode::Standard => {
                // The commit: nothing in the subtree may still be held,
                // dead-lettered versions included (ADR-0030 d6).
                let nodes: Vec<Uuid> = subtree.iter().map(|w| w.workspace_id).collect();
                if self.journal.held_count(tenant_id, &nodes).await? > 0 {
                    return Err(AppError::HeldRemaining { workspace_id });
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

    /// Point a workspace at a crew record, each by id or name, admin by key
    /// role: the journalled crew-metadata update `workspace.crew_updated`,
    /// carrying the record's `crew_id`. A workspace the tenant does not hold
    /// is absent, and so, checked after it, is a crew (http-api.md
    /// workspaces; ADR-0026 d8). A `frozen` workspace admits it: the
    /// crew-metadata update is one of the three mutations freeze allows
    /// (ADR-0009 d5; workspaces.md refusal matrix).
    pub async fn set_workspace_crew(
        &self,
        api_key: &str,
        workspace: Reference,
        crew: Reference,
    ) -> Result<WorkspaceRecord, AppError> {
        let key = self
            .admin_caller(Bearer::Key(api_key), "set_workspace_crew")
            .await?
            .key;
        let workspace_id = self.resolve(&key, NamedKind::Workspace, &workspace).await?;
        self.observer.workspace(workspace_id);
        let crew_id = self.resolve(&key, NamedKind::Crew, &crew).await?;
        let workspace = self
            .journal
            .workspace(key.tenant_id, workspace_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if self.journal.crew(key.tenant_id, crew_id).await?.is_none() {
            return Err(AppError::NotFound);
        }
        // Setting a workspace's crew is a neutral admin act (ADR-0097 d9).
        self.commit_as_act(
            &key,
            oml_domain::admin_act::AdminActKind::SetWorkspaceCrew,
            json!({ "workspace_id": workspace_id.to_string(), "crew_id": crew_id.to_string() }),
            None,
            event_types::WORKSPACE_CREW_UPDATED,
            json!({
                "workspace_id": workspace_id.to_string(),
                "crew_id": crew_id.to_string(),
            }),
            MetadataFold::WorkspaceCrewUpdated {
                workspace_id,
                crew_id,
            },
        )
        .await?;
        Ok(WorkspaceRecord {
            crew_id: Some(crew_id),
            ..workspace
        })
    }

    /// The person a key acts for, by name: a session key's
    /// `session_user`, else the name of a user key's root; `None` for a
    /// tenant key (ADR-0044 d2, ADR-0045 d6).
    async fn person_of(&self, key: &KeyRecord) -> Result<Option<Name>, AppError> {
        if let Some(user) = &key.session_user {
            return Ok(Some(user.clone()));
        }
        // A tenant key linked to a person acts for them as a user key does
        // (ADR-0085 d16; ADR-0082 d7).
        match key.user.or(key.linked_user) {
            Some(root) => Ok(self
                .journal
                .workspace(key.tenant_id, root)
                .await?
                .map(|w| w.name)),
            None => Ok(None),
        }
    }

    /// The person `key` counts as where authority is weighed, an admin
    /// fix's person, a fix's settlement and a write's recorded
    /// `author_person`: a session key's `session_user`, a linked key's
    /// person, and for a user key the person its issuer counts as, walked
    /// up `issued_by`. A user key counts as its root's person only when a
    /// key that already counts as that person issued it, the person's own
    /// session key or a key one issued; one an admin, or another person's
    /// key, issued for someone's root counts as its issuer's person, and
    /// as no one where the issuer counts as no one. A chain longer than
    /// [`COUNTED_PERSON_DEPTH`], or an issuer the tenant does not hold,
    /// counts as no one, which settles nothing (ADR-0089 and ADR-0085 d16,
    /// the operator's decision of 2026-11-20: "counts as the issuer").
    pub(crate) async fn counted_person_of(
        &self,
        key: &KeyRecord,
    ) -> Result<Option<Name>, AppError> {
        match self.counted_walk(key).await? {
            Some(walk) => match walk.last() {
                Some(counted) => self.person_of(counted).await,
                None => Ok(None),
            },
            None => Ok(None),
        }
    }

    /// The keys [`Self::counted_person_of`] walks from `key`: `key` first,
    /// then up `issued_by` while a key is a user key and no session key,
    /// ending at the key whose person `key` counts as. `None` where the walk
    /// counts the key as no one: a user key with no issuer, an issuer the
    /// tenant does not hold, or a chain longer than [`COUNTED_PERSON_DEPTH`]
    /// (ADR-0089 and ADR-0085 d16, the operator's decision of 2026-11-20).
    pub(crate) async fn counted_walk(
        &self,
        key: &KeyRecord,
    ) -> Result<Option<Vec<KeyRecord>>, AppError> {
        let mut walk = vec![key.clone()];
        for _ in 0..COUNTED_PERSON_DEPTH {
            let Some(key) = walk.last() else {
                return Ok(None);
            };
            if key.session_user.is_some() || key.user.is_none() {
                return Ok(Some(walk));
            }
            let Some(issuer) = key.issued_by else {
                return Ok(None);
            };
            match self.journal.key(key.tenant_id, issuer).await? {
                Some(next) => walk.push(next),
                None => return Ok(None),
            }
        }
        Ok(None)
    }

    /// Whether the walk [`Self::counted_person_of`] takes from `key` passes
    /// through the key `through`, `key` itself included: a user key issued by
    /// `through`, directly or by a user key it issued, counts as whomever
    /// `through`'s link names, so a relink of `through` moves such a key's
    /// person with it. A walk that counts the key as no one reads as passing
    /// through, which fails closed (ADR-0089, the reading of 2026-11-20).
    pub(crate) async fn counted_through(
        &self,
        key: &KeyRecord,
        through: Uuid,
    ) -> Result<bool, AppError> {
        Ok(self
            .counted_walk(key)
            .await?
            .is_none_or(|walk| walk.iter().any(|k| k.key_id == through)))
    }

    /// The name of the user root `key` was issued in, if any (ADR-0097 d6).
    pub(crate) async fn person_root_name(&self, key: &KeyRecord) -> Result<Option<Name>, AppError> {
        match key.user {
            Some(root) => Ok(self
                .journal
                .workspace(key.tenant_id, root)
                .await?
                .filter(|w| w.user_root)
                .map(|w| w.name)),
            None => Ok(None),
        }
    }

    /// The person whose own root holds `workspace`, or `None` for a node
    /// of the shared tree (ADR-0097 d16).
    pub(crate) async fn root_person_of(
        &self,
        tenant_id: Uuid,
        workspace: Uuid,
    ) -> Result<Option<Name>, AppError> {
        let Some(root) = self
            .journal
            .workspace(tenant_id, workspace)
            .await?
            .and_then(|w| w.user())
        else {
            return Ok(None);
        };
        Ok(self
            .journal
            .workspace(tenant_id, root)
            .await?
            .filter(|w| w.user_root)
            .map(|w| w.name))
    }

    /// Whether `workspace` sits inside the user root named `person`
    /// (ADR-0082 d7): the root a settlement compares against the person a
    /// key counts as, never the root the key was issued in (the operator's
    /// decision of 2026-11-20).
    pub(crate) async fn in_root_of_person(
        &self,
        tenant_id: Uuid,
        person: &Name,
        workspace: Uuid,
    ) -> Result<bool, AppError> {
        let Some(root) = self
            .journal
            .workspace(tenant_id, workspace)
            .await?
            .and_then(|w| w.user())
        else {
            return Ok(false);
        };
        Ok(self
            .journal
            .workspace(tenant_id, root)
            .await?
            .is_some_and(|w| w.user_root && &w.name == person))
    }

    /// Whether `author` acts for `person` on either reading: as the person
    /// it counts as ([`Self::counted_person_of`]) or as the person whose
    /// root it was issued in ([`Self::person_of`]). R2 by person takes the
    /// union, so neither reading makes a reviewer less the author (ADR-0001
    /// d5; the operator's decision of 2026-11-20).
    pub(crate) async fn acts_for(
        &self,
        author: &KeyRecord,
        person: &Name,
    ) -> Result<bool, AppError> {
        Ok(self.person_of(author).await?.as_ref() == Some(person)
            || self.counted_person_of(author).await?.as_ref() == Some(person))
    }

    /// Whether `author`, a version's author key, acts for `person` beside the
    /// person its write recorded (`author_person`): as the person whose user
    /// root it was issued in, which is fixed for the key's life, never its
    /// link as it stands, so a relink after the write moves nothing
    /// (ADR-0085 d16, amended 2026-11-18; the operator's decision of
    /// 2026-11-20).
    pub(crate) async fn issued_in_root_of(
        &self,
        author: &KeyRecord,
        person: &Name,
    ) -> Result<bool, AppError> {
        let Some(root) = author.user else {
            return Ok(false);
        };
        Ok(self
            .journal
            .workspace(author.tenant_id, root)
            .await?
            .is_some_and(|w| &w.name == person))
    }

    /// The user root of the person `key` acts for, read as
    /// [`Self::person_of`] reads the person: a user key's root, a linked
    /// tenant key's, or the root named as a session key's person; `None`
    /// for a key that acts for no person (ADR-0082 d7; ADR-0085 d16).
    async fn person_root_of(&self, key: &KeyRecord) -> Result<Option<Uuid>, AppError> {
        if let Some(root) = key.user.or(key.linked_user) {
            return Ok(Some(root));
        }
        let Some(user) = &key.session_user else {
            return Ok(None);
        };
        let Some(id) = self
            .journal
            .resolve_name(key.tenant_id, NamedKind::Workspace, user)
            .await?
        else {
            return Ok(None);
        };
        Ok(self
            .journal
            .workspace(key.tenant_id, id)
            .await?
            .filter(|w| w.user_root)
            .map(|w| w.workspace_id))
    }

    /// The root whose person's keys are `key`'s own set in an export
    /// (ADR-0084 I6, a linked key): a user key's root or a session key's
    /// person's; never a linked tenant key's, which owns itself alone.
    pub(crate) async fn own_set_root_of(&self, key: &KeyRecord) -> Result<Option<Uuid>, AppError> {
        if let Some(root) = key.user {
            return Ok(Some(root));
        }
        if key.session_user.is_none() {
            return Ok(None);
        }
        let unlinked = KeyRecord {
            linked_user: None,
            link_pending: None,
            pending_act: None,
            ..key.clone()
        };
        self.person_root_of(&unlinked).await
    }

    /// The account whose limits a write by `key` is held to: a user key's
    /// own, and for a person's `<user>-review`, a session key of no
    /// account, its person's; `None` for a tenant key, a linked one
    /// included, and for `<user>-curate` and `<user>-admin`: curation is
    /// not charged to the person (ADR-0071 d1, d3, the operator's decision
    /// of 2026-11-19; ADR-0082 d10).
    async fn limits_root_of(&self, key: &KeyRecord) -> Result<Option<Uuid>, AppError> {
        if key.user.is_some() || !auth::is_review_session_key(key) {
            return Ok(key.user);
        }
        self.own_set_root_of(key).await
    }

    /// Whether `workspace` sits inside the user root of the person `key`
    /// acts for; false for a key that acts for no person (ADR-0082 d7).
    async fn in_own_root(&self, key: &KeyRecord, workspace: Uuid) -> Result<bool, AppError> {
        let workspace_root = self
            .journal
            .workspace(key.tenant_id, workspace)
            .await?
            .and_then(|w| w.user());
        Ok(workspace_root.is_some() && workspace_root == self.person_root_of(key).await?)
    }

    /// R2 by person: whether `memory`'s author key acts for the person `key`
    /// acts for and the row sits outside that person's root, so `key`
    /// judges it not (ADR-0001 d5; ADR-0082 d7).
    async fn same_person_outside_root(
        &self,
        key: &KeyRecord,
        memory: &MemoryRecord,
    ) -> Result<bool, AppError> {
        Ok(self.same_person_as_author(memory, key).await?
            && !self.in_own_root(key, memory.workspace_id).await?)
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
        // The author read on either reading, the person whose root it was
        // issued in and the person it counts as, so a key an admin minted
        // in a root is no less its root's person here (ADR-0001 d5; the
        // operator's decision of 2026-11-20).
        match self.person_of(reviewer).await? {
            Some(r) => self.acts_for(&author, &r).await,
            None => Ok(false),
        }
    }

    /// Whether `memory`'s author and `reviewer` act for one person, the
    /// author read as recorded at the write (`author_person`), so an unlink
    /// of the author key between the write and this act clears neither the
    /// `same_user` mark nor R2 by person (ADR-0085 d16, amended 2026-11-18;
    /// #448). The reviewer is read as it stands, as [`Self::same_person`]
    /// reads it.
    async fn same_person_as_author(
        &self,
        memory: &MemoryRecord,
        reviewer: &KeyRecord,
    ) -> Result<bool, AppError> {
        let Some(r) = self.person_of(reviewer).await? else {
            return Ok(false);
        };
        if memory.author_person.as_ref() == Some(&r) {
            return Ok(true);
        }
        // `author_person` records the person the author key counts as; the
        // person whose root it was issued in is read off the key, so R2 by
        // person takes both (the operator's decision of 2026-11-20).
        match self
            .journal
            .key(memory.tenant_id, memory.author_key_id)
            .await?
        {
            Some(author) => self.issued_in_root_of(&author, &r).await,
            None => Ok(false),
        }
    }

    /// A user key's edge with an endpoint in `universe` is refused
    /// `access.grant_refused` naming that endpoint's workspace, before
    /// anything is journalled: users neither write nor propose to
    /// `universe` (ADR-0046 d5; ADR-0044 Context, decision 3). A tenant key
    /// passes.
    async fn refuse_user_edge_into_universe(
        &self,
        key: &KeyRecord,
        workspaces: &[Uuid],
    ) -> Result<(), AppError> {
        if key.user.is_none() {
            return Ok(());
        }
        for workspace_id in workspaces {
            let node = self.journal.workspace(key.tenant_id, *workspace_id).await?;
            if node.is_some_and(|n| n.parent_id.is_none()) {
                return Err(AppError::GrantRefused {
                    workspace_id: *workspace_id,
                    needed_role: Role::Reviewer,
                });
            }
        }
        Ok(())
    }

    /// The live proposals on `node` as `key` may see them: the pending rows
    /// whose `from` is the node's status, in proposal order, each naming its
    /// proposing key only to that key (workspaces.md, the status read lists
    /// only live proposals; ADR-0025 d15).
    async fn live_transitions(
        &self,
        key: &KeyRecord,
        node: &WorkspaceRecord,
    ) -> Result<Vec<TransitionView>, AppError> {
        let mut live = Vec::new();
        for t in self
            .journal
            .pending_transitions(key.tenant_id, node.workspace_id)
            .await?
            .into_iter()
            .filter(|t| t.from == node.status)
        {
            // The own key set is the calling key alone until rotation
            // is built (roles-and-keys.md#own), as on the tail.
            let own = t.proposer_key_id == key.key_id;
            live.push(TransitionView {
                transition_id: t.transition_id,
                workspace_id: t.workspace_id,
                from: t.from,
                to: t.to,
                state: t.state,
                proposed_by_key_id: own.then_some(t.proposer_key_id),
                proposed_by: self.proposed_by(key, t.proposer_key_id).await?,
                proposed_at: t.proposed_at,
            });
        }
        Ok(live)
    }

    /// The proposing key as a reader is told it (ADR-0095, amendment of
    /// 2026-11-24): its id and name when it is the reader's own key or
    /// another key of the reader's person, read as R2 reads one person
    /// ([`Self::same_person`]); `None` to every other reader, an admin or a
    /// tenant key that acts for no one included. ADR-0025 d15 is not
    /// widened: `proposed_by_key_id` keeps its own-key rule.
    async fn proposed_by(
        &self,
        reader: &KeyRecord,
        proposer_key_id: Uuid,
    ) -> Result<Option<ProposedBy>, AppError> {
        if proposer_key_id == reader.key_id {
            return Ok(Some(ProposedBy {
                key_id: reader.key_id,
                name: reader.name.clone(),
            }));
        }
        let Some(person) = self.person_of(reader).await? else {
            return Ok(None);
        };
        let Some(proposer) = self.journal.key(reader.tenant_id, proposer_key_id).await? else {
            return Ok(None);
        };
        Ok(self
            .acts_for(&proposer, &person)
            .await?
            .then(|| ProposedBy {
                key_id: proposer.key_id,
                name: proposer.name.clone(),
            }))
    }

    /// The brief of `workspace` as `caller` reads it: its ratified brief
    /// edges, each claim once, in ratification order, kept only while the
    /// claim is a promoted served tip the caller reads, and cut by
    /// `brief.max_tokens` under recall's prefix rule (ADR-0046 d8, d9). A
    /// superseded or retired member drops out by itself.
    async fn brief(&self, caller: &Caller, workspace: Uuid) -> Result<Brief, AppError> {
        let mut ratified: Vec<(u64, Uuid)> = self
            .journal
            .brief_edges(caller.key.tenant_id, workspace)
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
                    self.in_progress_root(caller.key.tenant_id, workspace)
                        .await?,
                    self.in_progress_root(caller.key.tenant_id, memory.workspace_id)
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
    /// pending rows whose `from` is the node's status (workspaces.md, the
    /// status read lists only live proposals; ADR-0009 d3), each naming its
    /// proposing key only to that key and never its principal (ADR-0025
    /// d15). The refusal
    /// order: (1) the key role, an admin key passing on its role and any
    /// other key needing reader among its admissible grant roles; (2) the
    /// node resolves; (3) a non-admin key needs reader effective access on
    /// it, `access.not_found` outside it. An admin key reads the pending
    /// transitions as structure, tenant-wide and with no grant
    /// (roles-and-keys.md permission matrix, footnote 6; ADR-0026 d15).
    pub async fn workspace_status(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
    ) -> Result<WorkspaceStatusView, AppError> {
        const VERB: &str = "workspace_status";
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        let structure = key.role == Role::Admin;
        if !structure {
            require_admissible(key, Role::Reader, VERB)?;
        }
        let workspace_id = self.resolve(key, NamedKind::Workspace, &workspace).await?;
        if !structure {
            self.require_access(&caller, workspace_id, Role::Reader, VERB)
                .await?;
        }
        let node = self
            .journal
            .workspace(key.tenant_id, workspace_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let pending = self.live_transitions(key, &node).await?;
        Ok(WorkspaceStatusView {
            workspace_id,
            status: node.status,
            policy: review_policy(),
            pending,
        })
    }

    /// Proposes a forward status transition of the node, by id or name, to
    /// `to`: the proposal is a pending `workspace_transitions` row, and the
    /// node keeps its status until a second identity ratifies it (ADR-0009
    /// d3). The refusal order: (1) the key role, before any lookup, an admin
    /// key passing on its role and any other key needing reviewer among its
    /// admissible grant roles, so a reader, contributor, submitter or operator key is
    /// `access.verb_ceiling`; (2) the node resolves; (3) a reviewer key needs
    /// reviewer effective access on it, `access.not_found` outside read access
    /// and `access.grant_refused` within it; (4) the ladder, on the node and
    /// its immediate children as read now: `workspace.transition_refused` with
    /// its reason, nothing journalled (ADR-0009 d1, d3, d4); (5) one
    /// `workspace.status_proposed` is sealed and committed with the row. A
    /// `used_by: service` key may propose: roles-and-keys.md bars it from
    /// ratification alone (ADR-0010). The ratifying transaction re-checks the
    /// node, so a proposal admitted here may still be refused there
    /// (workspaces.md, ratification re-checks).
    pub async fn propose_transition(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        to: WorkspaceStatus,
    ) -> Result<TransitionRecord, AppError> {
        self.propose_transition_with_reason(bearer, workspace, to, None)
            .await
    }

    /// [`Self::propose_transition`] with the proposer's `reason`, scanned
    /// as a verdict's reason is and journalled with
    /// `workspace.status_proposed` (ADR-0095 d1).
    pub async fn propose_transition_with_reason(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        to: WorkspaceStatus,
        reason: Option<&str>,
    ) -> Result<TransitionRecord, AppError> {
        const VERB: &str = "propose_transition";
        let caller = self.key_act_caller(bearer.into(), VERB).await?;
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Reviewer, VERB)?;
        }
        let workspace_id = self.resolve(key, NamedKind::Workspace, &workspace).await?;
        let node = self.propose_admitted(&caller, workspace_id, to).await?;
        // One live proposal per workspace and target status, whoever made
        // it: a second is `proposal.pending` naming the live one. Read before
        // the head, so a proposal committed in between moves the head and
        // this commit conflicts and runs again (ADR-0095 d5; #515).
        if let Some(live) = self
            .journal
            .pending_transitions(key.tenant_id, workspace_id)
            .await?
            .into_iter()
            .find(|t| t.from == node.status && t.to == to)
        {
            return Err(AppError::TransitionPending {
                transition_id: live.transition_id,
            });
        }
        let transition_id = self.ids.new_id();
        let prev = self.head_event(key.tenant_id).await?;
        let mut payload = json!({
            "workspace_id": workspace_id.to_string(),
            "from": node.status.as_str(),
            "to": to.as_str(),
            "transition_id": transition_id.to_string(),
            "proposed_by_key_id": key.key_id.to_string(),
        });
        with_reason(self, key.tenant_id, &mut payload, reason)?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::WORKSPACE_STATUS_PROPOSED,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let proposal = TransitionRecord {
            transition_id,
            tenant_id: key.tenant_id,
            workspace_id,
            from: node.status,
            to,
            state: TransitionState::Pending,
            proposer_key_id: key.key_id,
            proposer_principal: key.principal.clone(),
            ratifier_key_id: None,
            proposed_at: event.envelope.at.clone(),
        };
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
        workspace_id: Uuid,
        to: WorkspaceStatus,
    ) -> Result<WorkspaceRecord, AppError> {
        const VERB: &str = "propose_transition";
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Reviewer, VERB)?;
            self.require_access(caller, workspace_id, Role::Reviewer, VERB)
                .await?;
        }
        let node = self
            .journal
            .workspace(key.tenant_id, workspace_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let ladder = self
            .ladder_node_outside(key.tenant_id, workspace_id)
            .await?;
        let children: Vec<WorkspaceStatus> = self
            .journal
            .workspace_children(key.tenant_id, workspace_id)
            .await?
            .iter()
            .map(|child| child.status)
            .collect();
        // "No simulation is fenced on the subtree" holds vacuously while
        // simulations are unbuilt, and lands in the ladder's preconditions
        // with them (workspaces.md, readings the build takes).
        propose_check(ladder, node.status, to, &children).map_err(|reason| {
            AppError::TransitionRefused {
                workspace_id,
                from: node.status,
                to,
                reason,
            }
        })?;
        Ok(node)
    }

    /// Ratifies the pending proposal `transition_id` on the node, by id or
    /// name, as a second identity: the node's status becomes the proposal's
    /// `to`, journalled as one `workspace.status_transition` signed by the
    /// ratifier's principal and key, with the policy snapshot (ADR-0009 d3,
    /// d4). The refusal order: (1) the key role, before any lookup, an admin
    /// key passing on its role and any other key needing reviewer among its
    /// admissible grant roles, `access.verb_ceiling`; (2) a `used_by:
    /// service` key, also `access.verb_ceiling` before any lookup
    /// (roles-and-keys.md; ADR-0010); (3) the node resolves; (4) a reviewer
    /// key needs reviewer effective access on it; (5) the transition: one
    /// this tenant does not hold, or one on another workspace, is
    /// `access.not_found`; (6) R2, a ratifier holding the proposer's key or
    /// principal, `review.ratifier_is_proposer`; (7) the ladder against the
    /// node and its children as read now, a proposal already ratified or
    /// whose `from` is no longer the node's status being
    /// `workspace.transition_refused` with reason `stale`. Every read happens
    /// after the chain head is read and before a commit that compare-and-sets
    /// it, so a child created between the check and the commit fails the
    /// commit with `PrevHashConflict`, retryable, and is never journalled
    /// over a stale check; the store re-checks under the chain lock besides
    /// (workspaces.md, ratification re-checks).
    pub async fn ratify_transition(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        transition_id: Uuid,
    ) -> Result<WorkspaceRecord, AppError> {
        const VERB: &str = "ratify_transition";
        let caller = self.key_act_caller(bearer.into(), VERB).await?;
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Reviewer, VERB)?;
        }
        refuse_service(key, VERB)?;
        let workspace_id = self.resolve(key, NamedKind::Workspace, &workspace).await?;
        let prev = self.head_event(key.tenant_id).await?;
        let (proposal, node) = self
            .ratify_admitted(&caller, workspace_id, transition_id)
            .await?;
        let mut policy_snapshot = review_policy();
        if proposal.to == WorkspaceStatus::Frozen {
            // A false one was refused above and is never journalled.
            policy_snapshot["children_all_frozen"] = Value::Bool(true);
        }
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::WORKSPACE_STATUS_TRANSITION,
            &key.principal,
            Some(key.key_id),
            json!({
                "workspace_id": workspace_id.to_string(),
                "from": proposal.from.as_str(),
                "to": proposal.to.as_str(),
                "transition_id": transition_id.to_string(),
                "proposed_by_key_id": proposal.proposer_key_id.to_string(),
                "ratified_by_key_id": key.key_id.to_string(),
                "policy_snapshot": policy_snapshot,
                "same_user": self.same_person(key.tenant_id, proposal.proposer_key_id, key).await?,
            }),
        )?;
        // A `sole_admin` ratification by the proposer's own person is
        // marked on the transition (ADR-0097 d17).
        let event = if self.ratifier_person_rule(key, &proposal).await? {
            let env = event.envelope;
            let mut payload = env.payload;
            payload["sole_admin"] = Value::Bool(true);
            self.seal(
                prev.as_ref(),
                env.tenant_id,
                &env.event_type,
                &env.principal,
                env.api_key_id,
                payload,
            )?
        } else {
            event
        };
        self.commit(
            event,
            MetadataFold::TransitionRatified {
                transition_id,
                workspace_id,
                to: proposal.to,
                ratifier_key_id: key.key_id,
            },
            None,
        )
        .await?;
        Ok(WorkspaceRecord {
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
        workspace_id: Uuid,
        transition_id: Uuid,
    ) -> Result<(TransitionRecord, WorkspaceRecord), AppError> {
        const VERB: &str = "ratify_transition";
        let key = &caller.key;
        if key.role != Role::Admin {
            require_admissible(key, Role::Reviewer, VERB)?;
        }
        refuse_service(key, VERB)?;
        if key.role != Role::Admin {
            self.require_access(caller, workspace_id, Role::Reviewer, VERB)
                .await?;
        }
        let ladder = self
            .ladder_node_outside(key.tenant_id, workspace_id)
            .await?;
        let proposal = self
            .journal
            .transition(key.tenant_id, transition_id)
            .await?
            .filter(|t| t.workspace_id == workspace_id)
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
            .workspace(key.tenant_id, workspace_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let children: Vec<WorkspaceStatus> = self
            .journal
            .workspace_children(key.tenant_id, workspace_id)
            .await?
            .iter()
            .map(|child| child.status)
            .collect();
        let refused = |reason| AppError::TransitionRefused {
            workspace_id,
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
        // while simulations are unbuilt (workspaces.md, readings the build
        // takes).
        ratify_check(proposal.from, ladder, node.status, proposal.to, &children)
            .map_err(refused)?;
        self.ratifier_person_rule(key, &proposal).await?;
        Ok((proposal, node))
    }

    /// The ladder's two identities as two persons (ADR-0097 d12, d17): the
    /// ratifier counts as a person, that person is not the proposer's, and,
    /// where the proposing key counts as no person, not the person of the
    /// admin who issued it, so one admin cannot propose through an agent
    /// key they issued and ratify through their own session. Under
    /// `sole_admin` a ratification by the proposer's own person, or by the
    /// issuing admin's, is admitted, and the answer is `true`: the
    /// transition is marked `sole_admin`. Refused: a ratifier that counts as
    /// no person ([`AppError::CountsAsNoPerson`]) and the same person
    /// ([`AppError::RatifierSamePerson`]).
    pub(crate) async fn ratifier_person_rule(
        &self,
        key: &KeyRecord,
        proposal: &TransitionRecord,
    ) -> Result<bool, AppError> {
        let Some(ratifier) = self.counted_person_of(key).await? else {
            return Err(AppError::CountsAsNoPerson {
                act: "ladder ratification",
            });
        };
        let proposer = self
            .journal
            .key(key.tenant_id, proposal.proposer_key_id)
            .await?;
        let proposer_person = match &proposer {
            Some(p) => self.counted_person_of(p).await?,
            None => None,
        };
        let same = if proposer_person.as_ref() == Some(&ratifier) {
            Some(false)
        } else if proposer_person.is_none() {
            // The admin act that issued the proposing key names its person;
            // a key issued before admin acts were journalled is read off
            // its issuer as it counts now.
            let issuing = match proposer.as_ref() {
                Some(p) => self.issuing_person(p).await?,
                None => None,
            };
            (issuing.as_ref() == Some(&ratifier)).then_some(true)
        } else {
            None
        };
        match same {
            None => Ok(false),
            Some(_) if self.journal.admin_standing(key.tenant_id).await?.sole_admin => Ok(true),
            Some(issued_the_proposer) => Err(AppError::RatifierSamePerson {
                issued_the_proposer,
            }),
        }
    }

    /// The person of the admin who issued `key`: the `actor_person` of the
    /// `issue_key` act that names it, and for a key issued before admin
    /// acts were journalled, the person its issuer counts as (ADR-0097 d17).
    async fn issuing_person(&self, key: &KeyRecord) -> Result<Option<Name>, AppError> {
        let id = key.key_id.to_string();
        if let Some(act) = self
            .journal
            .admin_acts(key.tenant_id, None)
            .await?
            .into_iter()
            .find(|a| {
                a.act == oml_domain::admin_act::AdminActKind::IssueKey
                    && (a.effect.get("key_id").and_then(Value::as_str) == Some(id.as_str())
                        || a.target.get("key_id").and_then(Value::as_str) == Some(id.as_str()))
            })
        {
            return Ok(act.actor_person);
        }
        match key.issued_by {
            Some(issuer) => match self.journal.key(key.tenant_id, issuer).await? {
                Some(issuer) => self.counted_person_of(&issuer).await,
                None => Ok(None),
            },
            None => Ok(None),
        }
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
    /// `workspace_id` filter naming a node where the key holds some role.
    /// Without `own`, the scoped listing: reader on the scope node by the
    /// refusal order, and the promoted served tips of subtree(scope) ∩ the
    /// nodes where reader is effective, narrowed to a `workspace_id` subtree
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
            if req.own
                || req.author.is_some()
                || req
                    .states
                    .iter()
                    .any(|c| c.tip.is_some() || c.promoted_since.is_some())
            {
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
                    .workspace_subtree(tenant_id, node)
                    .await?
                    .into_iter()
                    .map(|w| w.workspace_id)
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
            // The workspace narrows the key's rows to a subtree, and names
            // only a node the naming rule names to the key; omitted, every
            // workspace the key wrote in (ADR-0039 d4).
            let filter = self
                .resolve_opt(key, NamedKind::Workspace, req.workspace.as_ref())
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
            // One workspace, the scope, defaulting to the key's own
            // (ADR-0039 d4).
            let scope = self.target_workspace(key, req.workspace.as_ref()).await?;
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
        let scoped_node = match &scope {
            ListingScope::Tips { .. } => Some(()),
            ListingScope::Own { .. } => None,
        };
        let filter = ListingFilter {
            scope,
            subtree,
            states: req.states.clone(),
            kinds: req.kinds.clone(),
            claim_key: req.claim_key.clone(),
            prefix: req.prefix.clone(),
            since: req.since.clone(),
            until: req.until.clone(),
            anchor: req.anchor.clone(),
            author: req.author,
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
                    key_ids.contains(&m.author_key_id) && !masked.contains(&m.workspace_id)
                }
                ListingScope::Tips { nodes, held, .. } => {
                    nodes.contains(&m.workspace_id)
                        && (match &filter.as_of {
                            None => m.status == MemoryStatus::Promoted && m.is_tip,
                            Some(_) => {
                                m.status == MemoryStatus::Promoted
                                    && tips_then.contains(&m.memory_id)
                            }
                        } || (held.contains(&m.workspace_id)
                            && filter
                                .as_of
                                .as_ref()
                                .is_none_or(|at| m.created_at.as_str() <= at.as_str())
                            && served_unreviewed(&m, self.scanner.version())))
                }
            };
            // The kinds and the prefix re-checked, as the scope is (R3).
            let filtered = (filter.kinds.is_empty() || filter.kinds.contains(&m.kind))
                && filter
                    .prefix
                    .as_ref()
                    .is_none_or(|p| m.claim_key.as_bytes().starts_with(p.as_bytes()));
            if admitted && filtered {
                items.push(self.served(&caller, m).await?);
            }
        }
        // The catalog's counts, on a scoped read's first page only, capped;
        // over exactly the rows the page's predicate admits, every filter
        // but `kinds` applied (ADR-0092 d6).
        let cap = self.search.catalog_count_cap();
        let counts = match (req.counts, scoped_node, &req.cursor) {
            (true, Some(()), None) => Some(
                self.journal
                    .count_memories(tenant_id, &filter, &MemoryKind::ALL, cap)
                    .await?
                    .into_iter()
                    .map(|(kind, n)| {
                        (
                            kind,
                            if n > cap {
                                KindCount::AtLeast(cap)
                            } else {
                                KindCount::Exact(n)
                            },
                        )
                    })
                    .collect(),
            ),
            _ => None,
        };
        // Whether each instruction or steering item applies at the scope
        // node, read only when the page holds one (ADR-0092 d6).
        let marks = if scoped_node.is_some() {
            self.catalog_marks(&caller, req.workspace.as_ref(), &items)
                .await?
        } else {
            BTreeMap::new()
        };
        Ok(MemoryPage {
            items,
            next_cursor,
            counts,
            marks,
        })
    }

    /// Per skill, guardrail and steering item of a catalog page, whether it
    /// applies at the read's scope node (ADR-0092 d6): a skill's offered
    /// command there, a guardrail's `in_force` under the key's
    /// `guardrail_scope`, a steering row's place in the steering list
    /// there. The sets are read only for the kinds the page holds.
    async fn catalog_marks(
        &self,
        caller: &Caller,
        workspace: Option<&Reference>,
        items: &[ServedMemory],
    ) -> Result<BTreeMap<Uuid, CatalogMark>, AppError> {
        let has = |k: MemoryKind| items.iter().any(|i| i.memory.kind == k);
        let mut marks = BTreeMap::new();
        if !has(MemoryKind::Skill) && !has(MemoryKind::Guardrail) && !has(MemoryKind::Steering) {
            return Ok(marks);
        }
        let key = &caller.key;
        let scope = self.target_workspace(key, workspace).await?;
        let chain = self
            .journal
            .workspace_ancestry(key.tenant_id, scope)
            .await?
            .unwrap_or_default();
        if has(MemoryKind::Skill) || has(MemoryKind::Guardrail) {
            let offered = self
                .recall_instructions(caller, workspace, &chain, false, true, None)
                .await?;
            let applies: BTreeSet<Uuid> = offered.ids.iter().map(|(id, _)| *id).collect();
            for item in items {
                let id = item.memory.memory_id;
                match item.memory.kind {
                    MemoryKind::Skill => {
                        marks.insert(
                            id,
                            CatalogMark::Skill {
                                prompt: offered.skills.get(&id).map(|s| s.prompt.clone()),
                            },
                        );
                    }
                    MemoryKind::Guardrail => {
                        marks.insert(
                            id,
                            CatalogMark::Guardrail {
                                in_force: applies.contains(&id),
                            },
                        );
                    }
                    _ => {}
                }
            }
        }
        if has(MemoryKind::Steering) {
            let ids: Vec<Uuid> = chain.iter().map(|w| w.workspace_id).collect();
            let applicable: Vec<Uuid> = (0..ids.len())
                .rev()
                .filter(|&i| caller.effective(&ids[i..]).contains(&Role::Reader))
                .map(|i| ids[i])
                .collect();
            let in_force: BTreeSet<Uuid> = self
                .journal
                .steering_in_force(key.tenant_id, &applicable)
                .await?
                .into_iter()
                .filter(|m| m.is_tip && !m.retirement && m.status == MemoryStatus::Promoted)
                .map(|m| m.memory_id)
                .collect();
            for item in items {
                if item.memory.kind == MemoryKind::Steering {
                    marks.insert(
                        item.memory.memory_id,
                        CatalogMark::Steering {
                            applies_here: in_force.contains(&item.memory.memory_id),
                        },
                    );
                }
            }
        }
        Ok(marks)
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
    /// `workspace`'s recall scope — the served tips recall would read there,
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
        workspace: Uuid,
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
            .workspace_ancestry(key.tenant_id, workspace)
            .await?
            .unwrap_or_default();
        let readable = self.nodes_with(caller, workspace, Role::Reader).await?;
        let mut hit_nodes = with_universe(caller, &readable, &scope_ancestry);
        // Recall's scope there holds every public node the key reads
        // (ADR-0098 d3).
        self.public_hit_nodes(caller, &mut hit_nodes).await?;
        let scope = CandidateScope {
            reference: None,
            promoted_nodes: hit_nodes.clone(),
            kinds: SIMILAR_KINDS.to_vec(),
            instructions: Vec::new(),
            counted_kinds: SIMILAR_KINDS.to_vec(),
            own_held: Some(OwnHeldScope {
                author_key_ids: vec![key.key_id],
                nodes: readable.clone(),
                any_author_nodes: Vec::new(),
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
                    MemoryStatus::Promoted => m.is_tip && hit_nodes.contains(&m.workspace_id),
                    MemoryStatus::Held => {
                        m.author_key_id == key.key_id && readable.contains(&m.workspace_id)
                    }
                    _ => false,
                };
                admitted
                    && !m.retirement
                    && SIMILAR_KINDS.contains(&m.kind)
                    && !(exclude == Some(m.claim_key.as_str()) && m.workspace_id == workspace)
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
                workspace_id: m.workspace_id,
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
        workspace: Uuid,
        kind: MemoryKind,
        body: &str,
        exclude: Option<&str>,
        on_similar: OnSimilar,
    ) -> Result<Vec<SimilarItem>, AppError> {
        if !SIMILAR_KINDS.contains(&kind) {
            return Ok(Vec::new());
        }
        let started = std::time::Instant::now();
        let similar = self.similar_to(caller, workspace, body, exclude).await?;
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

    /// Submit a memory: submitter on the resolved node, which must exist, by
    /// the refusal order (ADR-0025 d1), and in a user root the key that
    /// user's own, else `submission.user_root_refused` where the key reads
    /// the node (ADR-0044 d16); every `derives_from` target readable
    /// (ADR-0025 d3); the claim text normalized to NFC before the digest,
    /// the scan and the byte count (admission.md capture; ADR-0025 d6);
    /// same-call deterministic scan, whose veto is a refusal that journals
    /// nothing; a new claim's `claim_key` free in the workspace (ADR-0025
    /// d18), derived from the author and the text when omitted, and a
    /// verb defaulted from `derives_from` (ADR-0039 d3, d6); held on arrival
    /// as the claim's version 1, idempotency record honored (admission.md,
    /// http-api.md). A `frozen` workspace refuses the write as
    /// `workspace.frozen` after effective access, so a key without access gets
    /// its access refusal first, and after the idempotency replay, before the
    /// scan (ADR-0009 d5; workspaces.md refusal matrix).
    pub async fn submit_memory(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &SubmitRequest,
    ) -> Result<SubmitOutcome, AppError> {
        self.submit_memory_as(bearer.into(), req, None).await
    }

    /// [`Self::submit_memory`], where `imported` says an import writes the
    /// version: its claim key is the source's, verbatim, an `auto-` key
    /// included, so a re-import plans against the same key (ADR-0084 d8).
    pub(crate) async fn submit_memory_as(
        &self,
        bearer: Bearer<'_>,
        req: &SubmitRequest,
        imported: Option<&crate::types::ImportedWrite>,
    ) -> Result<SubmitOutcome, AppError> {
        self.submit_pipeline(bearer, req, imported, None)
            .await?
            .ok_or_else(|| {
                StoreError::Backend("a capture stopped at its decision".to_owned()).into()
            })
    }

    /// The submit pipeline: every check capture makes, then the capture.
    /// With `preview` it takes no idempotency record and replays none, sets
    /// what it decides on `preview` as it goes, and stops before anything is
    /// sealed, committed or metered, answering `None` (ADR-0082 d5, d10).
    async fn submit_pipeline(
        &self,
        bearer: Bearer<'_>,
        req: &SubmitRequest,
        imported: Option<&crate::types::ImportedWrite>,
        mut preview: Option<&mut PreviewFacts>,
    ) -> Result<Option<SubmitOutcome>, AppError> {
        let caller = self.authenticate_bearer(bearer).await?;
        let key = &caller.key;
        // A person's reviewer session key starts a claim where it holds
        // reviewer and reader, the grants a supersession's step 4 asks; the
        // version is held and its lineage excludes the key. Every other
        // reviewer key keeps the verb ceiling (ADR-0082 d2, d8 item 5, d10).
        let session_reviewer = session_reviewer_starts_claims(key);
        if !session_reviewer {
            require_admissible(key, Role::Submitter, "submit")?;
        }
        // The body's workspace, by id or name, resolves after step 1 and
        // before the record is looked up, so either spelling is one request
        // (ADR-0026 d4).
        let workspace_id = self.target_workspace(key, req.workspace.as_ref()).await?;
        let access = if session_reviewer {
            match self
                .require_access(&caller, workspace_id, Role::Reviewer, "submit")
                .await
            {
                Ok(()) => {
                    self.require_access(&caller, workspace_id, Role::Reader, "submit")
                        .await
                }
                refused => refused,
            }
        } else {
            self.require_access(&caller, workspace_id, Role::Submitter, "submit")
                .await
        };
        // A memory is started in a user root by its own user's keys only.
        // The rule is answered where step 2 allows or step 3 would answer
        // `access.grant_refused`, which a key that reads the node gets; a
        // key that reads nothing of it keeps step 4's `access.not_found`
        // (ADR-0044 d16).
        if matches!(access, Ok(()) | Err(AppError::GrantRefused { .. })) {
            self.refuse_another_users_root(key, workspace_id).await?;
        }
        // Inside the person's own root their web key starts a claim, so
        // every write there counts on their account; the review key starts
        // one only outside it (ADR-0082 d2; d10, note of 2026-11-19).
        if session_reviewer && access.is_ok() && self.in_own_root(key, workspace_id).await? {
            return Err(AppError::ReviewKeyInOwnRoot);
        }
        if let Err(refusal) = access {
            // A user key reads `universe` and never writes it: it is told
            // universe is curated, `submission.universe_refused`, in place
            // of step 3's `access.grant_refused` (ADR-0044 d7, d14).
            if matches!(refusal, AppError::GrantRefused { .. })
                && key.user.is_some()
                && workspace_id == self.universe(key.tenant_id).await?
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
        if imported.is_none()
            && req
                .claim_key
                .as_deref()
                .is_some_and(|sent| sent.starts_with(DERIVED_CLAIM_KEY_PREFIX))
        {
            return Err(AppError::DerivedClaimKeySent);
        }
        // A skill's, a document's or a test case's fields and the key the
        // kind derives, before the digest, so an omitted key and its
        // derived spelling are one request (ADR-0080 d1; ADR-0081 d2).
        let requested_format = req
            .format
            .clone()
            .unwrap_or_else(|| default_format(req.kind).to_owned());
        let (admitted_fields, kind_key) = kind_fields(
            req.kind,
            false,
            false,
            req.kind_fields.as_ref(),
            req.claim_key.as_deref(),
            &nfc(&req.body),
            &requested_format,
        )?;
        let claim_key = match (&req.claim_key, kind_key) {
            (_, Some(derived)) => derived,
            (Some(sent), None) => sent.clone(),
            (None, None) => derive_claim_key(key.key_id, &req.body),
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
            workspace_id,
            idem_key: req.idempotency_key.clone(),
        };
        if let Some(facts) = preview.as_deref_mut() {
            facts.workspace_id = Some(workspace_id);
            facts.kind = Some(req.kind);
        }
        let request_digest = submit_request_digest(req, &claim_key, verb, workspace_id);
        // A preview takes no record and replays none (ADR-0082 d10).
        if preview.is_none()
            && let Some(record) = self.journal.idempotency_get(&scope).await?
        {
            if record.request_digest != request_digest {
                return Err(AppError::IdempotencyDigestMismatch);
            }
            let mut receipt: SubmitReceipt = serde_json::from_value(record.response)
                .map_err(|e| AppError::StoredResponse(e.to_string()))?;
            receipt.similar = self
                .still_readable(&caller, std::mem::take(&mut receipt.similar))
                .await?;
            return Ok(Some(SubmitOutcome {
                receipt,
                replayed: true,
            }));
        }

        // The account's limits, read once and before the head: the check
        // below and the commit's ceiling hold to the same ones, and nothing
        // is looked up in the window a conflicting commit reopens
        // (ADR-0071 d3). A person's session key is held to its person's
        // (ADR-0082 d10, note of 2026-11-19).
        let ceiling = self
            .ceiling_for(key.tenant_id, self.limits_root_of(key).await?)
            .await?;

        // The head first, then the node's status, the ancestors' closures,
        // which a claim edge's proposal widens, and the claim_key read: a
        // ratification, an edge proposal or a capture that commits in between
        // moves the head, so this commit conflicts and its retry reads them
        // all again (ADR-0001 d2; ADR-0009 d5; ADR-0025 d10, ordinals; d18).
        let prev = self.head_event(key.tenant_id).await?;
        self.refuse_frozen(key.tenant_id, workspace_id, FrozenOp::Write)
            .await?;

        // Capture normalizes first: the digest, the scan and the byte count
        // all see the NFC text (admission.md capture; ADR-0025 d6).
        let body = nfc(&req.body);
        let (format, parsed) = self.capture_format(
            req.kind,
            Some(&requested_format),
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
        self.scan_kind_fields(key.tenant_id, admitted_fields.as_ref())?;
        let origin = recorded_origin(req.origin, req.anchor.as_ref())?;

        // Deterministic scan: same call, before the commit transaction. A
        // veto is a refusal like any other: it journals nothing and stores
        // no row (admission.md; errors.md submission.scan_veto).
        let (outcome, found) = self.scanner.assess(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: req.kind.as_str(),
            body: &body,
        });
        let scan_finding = match outcome {
            ScanOutcome::Pass => None,
            ScanOutcome::Hold(reason) => Some(reason),
            ScanOutcome::Veto(reason) => return Err(AppError::ScanVeto(reason)),
        };
        let mut scan_finding = scan_finding;
        // Every finding behind the verdict, in the order scanned: the
        // body's, then the title's and each decoded view's (ADR-0074 d23).
        let mut findings = self.admitted(&found, "/body".to_owned());
        self.scan_derived(
            key.tenant_id,
            req.kind,
            &format,
            &parsed,
            &mut scan_finding,
            &mut findings,
        )?;
        if let Some(facts) = preview.as_deref_mut() {
            facts.findings.clone_from(&findings);
        }

        // The lineage closure: the author plus every declared ancestor's
        // closure — R2 runs over the whole of it (roles-and-keys.md). An
        // ancestor the key cannot read is indistinguishable from none. A
        // target inside an in-progress subtree the write is not inside is
        // unreadable at the write's node (ADR-0030 d7). A curator writes
        // inside an in-progress subtree only (d9).
        self.curator_may_act(key, "submit", workspace_id, None)
            .await?;
        let write_root = self.in_progress_root(key.tenant_id, workspace_id).await?;
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
                    self.in_progress_root(key.tenant_id, m.workspace_id).await?,
                )
            {
                return Err(AppError::LineageUnknown(*ancestor));
            }
            principals.extend(m.lineage_principals);
            key_ids.extend(m.lineage_key_ids);
        }

        let claim_state = self
            .journal
            .claim_key_state(key.tenant_id, workspace_id, &claim_key)
            .await?;
        if claim_state.live {
            return Err(AppError::ClaimKeyConflict);
        }
        // A key whose approved versions were all demoted is written again
        // continuing its numbers, the chain staying the one the meter
        // counts; any other released key starts at version 1 (ADR-0089
        // d16, O8 of 2026-11-19; ADR-0025 d18).
        let version = match claim_state.greatest_version {
            Some(greatest) if claim_state.demoted => greatest.checked_add(1).ok_or_else(|| {
                StoreError::Backend(format!("claim_key {claim_key:?} has no next ordinal"))
            })?,
            _ => 1,
        };

        // A person of the sign-up tenant writes at most their memories
        // quota, and an account within its bytes and monthly writes;
        // nothing is journalled past them. The check follows the
        // idempotency replay and the live claim_key refusal, so a retry
        // gets its stored receipt and a write under a live claim is the
        // conflict it was; a new version goes through `supersede`, which
        // adds no memory. This check spares the signature; the commit's
        // ceiling is the one that holds (ADR-0068 d5; ADR-0071 d3).
        self.require_within(
            key.tenant_id,
            ceiling.as_ref(),
            &[
                // A continued chain is a memory the meter already counts.
                (
                    oml_domain::signup::Quota::Memories,
                    u64::from(!claim_state.demoted),
                ),
                (oml_domain::signup::Quota::TextBytes, body.len() as u64),
                (oml_domain::signup::Quota::WritesPerMonth, 1),
            ],
        )
        .await?;

        // The similar check, after every other refusal and before the
        // commit: a refusal here journals nothing and claims no idempotency
        // record, so the same key may be resent with `hold` (ADR-0050 d1).
        let similar = self
            .similar_gate(
                &caller,
                workspace_id,
                req.kind,
                format.search_text.as_deref().unwrap_or(&body),
                None,
                req.on_similar,
            )
            .await?;

        // A preview stops here: nothing is sealed, committed or metered.
        if preview.is_some() {
            return Ok(None);
        }
        let memory_id = self.ids.new_id();
        let body_digest = sha256_digest_string(body.as_bytes());
        let body_bytes = body.len() as u64;
        let memory = MemoryRecord {
            memory_id,
            tenant_id: key.tenant_id,
            workspace_id,
            claim_key: claim_key.clone(),
            version,
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
            author_person: self.counted_person_of(key).await?,
            derives_from: req.derives_from.clone(),
            lineage_principals: principals.into_iter().collect(),
            lineage_key_ids: key_ids.into_iter().collect(),
            scan_finding: scan_finding.clone(),
            scanned_under: self.scanner.version().map(str::to_owned),
            created_at: self.clock.now(),
            guardrail,
            kind_fields: admitted_fields.map(Box::new),
            valid: Box::new(valid),
            anchor: req.anchor.clone().map(Box::new),
            origin_kind: Some(origin),
            source_check: None,
            approval: None,
            format: Box::new(format),
        };

        // The catalog payload of submission.remember: digest-only, body
        // plaintext never rides the journal (ADR-0002 d1; event-catalog.md).
        // The PoC stores the body inline and unencrypted, so there is no
        // ciphertext digest; the scanner names the rule set its verdict was
        // taken under, or none (admission.md rule-set registry; ADR-0065 d1).
        let mut payload = json!({
            "artifact_id": memory_id.to_string(),
            "workspace_id": workspace_id.to_string(),
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
        let mut payload = with_kind_fields(payload, memory.kind_fields.as_deref());
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
            workspace_id,
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
        self.commit_written(
            event,
            MetadataFold::MemoryHeld {
                imported: imported.cloned().map(Box::new),
                memory,
                enqueue: true,
                findings: findings.into(),
            },
            idempotency,
            key,
            // No earlier row carries the claim_key: this version starts
            // its chain. A key released by a rejection does not.
            claim_state.greatest_version.is_none(),
            ceiling,
        )
        .await?;
        // After the commit, off the write's path: the remember side's
        // duplicate count, and a citation's label on the key's recall
        // (ADR-0054 d5, d12).
        if let Some(telemetry) = &self.telemetry {
            telemetry.offer(TelemetryItem::Remembered(Remembered {
                tenant_id: key.tenant_id,
                workspace_id,
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
        Ok(Some(SubmitOutcome {
            receipt,
            replayed: false,
        }))
    }

    /// Serve the next reviewable held item to this reviewer, claiming it,
    /// from the rows within its reviewer effective access only; a
    /// `used_by: service` key is refused before any lookup (ADR-0025 d1).
    /// A key that cannot approve an instruction is never served one, nor a
    /// successor that replaces one (ADR-0074 d19's lane, accepted for the
    /// instruction class by ADR-0080 d16).
    pub async fn review_next(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<Option<ServedMemory>, AppError> {
        self.review_next_needing(bearer, None).await
    }

    /// [`App::review_next`] narrowed by `needs`: `person` serves the person
    /// lane alone, round-robin across author keys, each author's oldest
    /// first, and serves a key that cannot approve it
    /// nothing; `reviewer` serves the rows any reviewer may approve
    /// (ADR-0074 d19; ADR-0080 d16).
    pub async fn review_next_needing(
        &self,
        bearer: impl Into<Bearer<'_>>,
        needs: Option<ReviewLane>,
    ) -> Result<Option<ServedMemory>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reviewer, "review")?;
        refuse_service(key, "review")?;
        let Some(lane) = review_lane(key, needs) else {
            return Ok(None);
        };
        let workspaces = self.reviewer_nodes(&caller).await?;
        match self
            .journal
            .review_claim_next(key.tenant_id, &key.principal, key.key_id, &workspaces, lane)
            .await?
        {
            None => Ok(None),
            Some(memory) => {
                let mut served = self.served(&caller, memory).await?;
                self.with_findings(key.tenant_id, &mut [&mut served])
                    .await?;
                Ok(Some(served))
            }
        }
    }

    /// Record a verdict. Reviewer on the row's workspace by the refusal order,
    /// with no exception for review, and never from a `used_by: service` key
    /// (ADR-0025 d1); R2 runs here against the row's reviewer-exclusion set
    /// — its lineage closure plus, for a held successor, the authors of the
    /// held rivals of its target (`check_reviewer`; ADR-0025 d10). With the
    /// PoC policy k = 1 an approval ratifies immediately, as a second
    /// journalled event carrying the policy snapshot, committed in the
    /// verdict's own transaction (admission.md review and
    /// ratification): `lifecycle.promote` for a first version, and for a
    /// successor `lifecycle.supersede`, which compare-and-sets that its
    /// target is still the served tip of the successor's workspace and
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
        require_admissible(&caller.key, Role::Reviewer, "verdict")?;
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
        require_admissible(key, Role::Reviewer, "verdict")?;
        refuse_service(key, "verdict")?;
        let recording = VerdictRecording {
            scope: IdempotencyScope {
                tenant_id: key.tenant_id,
                key_id: key.key_id,
                workspace_id: Uuid::nil(),
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
                let lane = review_lane(key, None).unwrap_or(ReviewLane::Reviewer);
                let claimed = self
                    .journal
                    .review_claimed(key.tenant_id, key.key_id, lane)
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
    /// journals nothing. Only a reviewer key a person holds is served it
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
        let (mut nodes, walked) = self.reviewer_nodes_walked(&caller).await?;
        if let Some(workspace) = &req.workspace {
            let root = self.resolve(key, NamedKind::Workspace, workspace).await?;
            let subtree: BTreeSet<Uuid> = self
                .journal
                .workspace_subtree(tenant_id, root)
                .await?
                .into_iter()
                .map(|w| w.workspace_id)
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
        // A key that cannot approve an instruction is listed none, as review
        // next serves it none (ADR-0074 d19; ADR-0080 d16).
        let Some(lane) = review_lane(key, req.needs) else {
            return Ok(HeldPage {
                items: Vec::new(),
                next_cursor: None,
                lane_authors: Some(Vec::new()),
            });
        };
        let filter = HeldFilter {
            nodes,
            author_key_id,
            claim_key_prefix: req.claim_key_prefix.clone(),
            lane,
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
        let reviewer_person = self.person_of(key).await?;
        let mut items = Vec::with_capacity(rows.len());
        let mut issued_here = BTreeMap::new();
        for row in rows {
            let m = &row.memory;
            let Some(same) = self
                .held_admits(
                    key,
                    &filter,
                    &walked,
                    reviewer_person.as_ref(),
                    m,
                    Some(&mut issued_here),
                )
                .await?
            else {
                continue;
            };
            if let Entry::Vacant(slot) = ancestries.entry(m.workspace_id)
                && let Some(ancestry) = ancestry_of(&walked, m.workspace_id)
            {
                slot.insert(Some(ancestry));
            }
            items.push(HeldItem {
                same_user: same,
                held_since: row.held_since,
                served: self.served_in(&caller, row.memory, &mut ancestries).await?,
            });
        }
        let mut served: Vec<&mut ServedMemory> = items.iter_mut().map(|i| &mut i.served).collect();
        self.with_findings(tenant_id, &mut served).await?;
        // The lane's count per author key, beside its page (ADR-0074 d19).
        let lane_authors = if lane == ReviewLane::Person {
            Some(
                self.journal
                    .review_held_authors(tenant_id, &key.principal, key.key_id, &filter)
                    .await?,
            )
        } else {
            None
        };
        Ok(HeldPage {
            items,
            next_cursor,
            lane_authors,
        })
    }

    /// Whether the held listing serves `m` to `key` under `filter`, and if
    /// so whether the key names the author's person (`same_user`): the
    /// application layer's half of the listing's rule, which every read
    /// that lists or counts held rows applies to what the store returned
    /// (ADR-0055 d2; ADR-0083 d11). `reviewer_person` is the key's person,
    /// read once per read. `issued_here`, a listing's cache by author key of
    /// whether that key was issued in the reviewer's person's root, is read
    /// for the `same_user` mark of a row in that root; a count, which serves
    /// such a row either way, passes `None` and reads no key.
    async fn held_admits(
        &self,
        key: &KeyRecord,
        filter: &HeldFilter,
        walked: &BTreeMap<Uuid, WorkspaceRecord>,
        reviewer_person: Option<&Name>,
        m: &MemoryRecord,
        issued_here: Option<&mut BTreeMap<Uuid, bool>>,
    ) -> Result<Option<bool>, AppError> {
        let tenant_id = key.tenant_id;
        // R2 and R3 in depth: nothing the queue would not serve this key
        // leaves the application layer, whatever the store returned.
        let excluded = m.lineage_key_ids.contains(&key.key_id)
            || m.lineage_principals.contains(&key.principal);
        if m.status != MemoryStatus::Held || excluded || !filter.nodes.contains(&m.workspace_id) {
            return Ok(None);
        }
        // The author's person as recorded at the write (ADR-0085 d16,
        // amended 2026-11-18; #448).
        let mut same = same_user(m.author_person.as_ref(), reviewer_person);
        // `author_person` records the person the author key counts as. A
        // row in the reviewer's own person's root by a key issued in that
        // root is that person's too: the root from the walk, and the author
        // key read once per listing (the operator's decision of 2026-11-20).
        if !same
            && let (Some(person), Some(cache)) = (reviewer_person, issued_here)
            && let Some(root) = walked.get(&m.workspace_id).and_then(WorkspaceRecord::user)
            && walked.get(&root).is_some_and(|w| &w.name == person)
        {
            same = match cache.get(&m.author_key_id) {
                Some(&issued) => issued,
                None => {
                    let issued = match self.journal.key(tenant_id, m.author_key_id).await? {
                        Some(author) => author.user == Some(root),
                        None => false,
                    };
                    cache.insert(m.author_key_id, issued);
                    issued
                }
            };
        }
        // R2 by person: a row the reviewer's own person wrote outside
        // that person's root is not theirs to judge, so it is not
        // listed (ADR-0001 d5; ADR-0082 d7).
        if same {
            let in_root = match walked.get(&m.workspace_id) {
                Some(w) => w.user(),
                None => self
                    .journal
                    .workspace(tenant_id, m.workspace_id)
                    .await?
                    .and_then(|w| w.user()),
            };
            // Whether the row's root is the reviewer's own person's,
            // from the key row and the walk, without a read where the
            // walk holds the root: a session key names its person by
            // the root's name.
            let own = match (key.user.or(key.linked_user), &key.session_user, in_root) {
                (_, _, None) => false,
                (Some(root), _, Some(r)) => root == r,
                (None, Some(user), Some(r)) => match walked.get(&r) {
                    Some(w) => &w.name == user,
                    None => self.person_root_of(key).await? == Some(r),
                },
                (None, None, Some(_)) => false,
            };
            if !own {
                return Ok(None);
            }
        }
        Ok(Some(same))
    }

    /// The live session key of `capacity` that serves the same person as
    /// the session key `key`, as its caller: the row holding the capacity's
    /// name, a session key serving that person, not revoked and not
    /// expired (ADR-0045 d6). `None` where the tenant holds no such key.
    async fn sibling_session_caller(
        &self,
        key: &KeyRecord,
        capacity: Capacity,
    ) -> Result<Option<Caller>, AppError> {
        let Some(user) = &key.session_user else {
            return Ok(None);
        };
        let Ok(name) = pages::key_name(user, capacity.suffix()) else {
            return Ok(None);
        };
        let Some(id) = self
            .journal
            .resolve_name(key.tenant_id, NamedKind::Key, &name)
            .await?
        else {
            return Ok(None);
        };
        let Some(found) = self.journal.key(key.tenant_id, id).await? else {
            return Ok(None);
        };
        let now = self.clock.now();
        let serves = found.name.as_ref() == Some(&name)
            && found.credential == Credential::Session
            && found.session_user.as_ref() == Some(user)
            && found.revoked_at.is_none()
            && !found.expired_at(&now);
        if !serves {
            return Ok(None);
        }
        Ok(Some(self.caller_of(found).await?))
    }

    /// The To do counts read (ADR-0083 d11, d14): for a person's web
    /// session key, `review`, the number of rows the held listing would
    /// serve the person's reviewer key, `<user>-review`, at `workspace` and
    /// below it or across every workspace without one, counted under the
    /// listing's own store read and the listing's own check (`held_admits`), so it counts no
    /// row that listing would not serve; up to [`TODO_COUNT_CAP`], with
    /// `truncated` set past it. It reads no text out and journals nothing.
    /// A person with no live reviewer key, no reviewer grant or a
    /// `workspace` its reviewer key cannot read counts 0, the workspace not
    /// named. Groups whose feature is not built are not counted (d14).
    pub async fn todo_counts(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Option<&Reference>,
    ) -> Result<TodoCounts, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        if !auth::is_web_session_key(&caller.key) {
            return Err(AppError::VerbCeiling {
                role: caller.key.role,
                verb: "todo_counts",
            });
        }
        // The Notices group: the person's notices not dismissed, whatever
        // the workspace named (ADR-0097 d20).
        let notices = match &caller.key.session_user {
            Some(person) => self
                .journal
                .notices(caller.key.tenant_id, person, false)
                .await?
                .len() as u64,
            None => 0,
        };
        let mut counts = self.todo_review_counts(&caller, workspace).await?;
        counts.notices = notices.min(TODO_COUNT_CAP);
        counts.truncated |= notices > TODO_COUNT_CAP;
        Ok(counts)
    }

    /// [`Self::todo_counts`]'s To review and Brief proposals groups.
    async fn todo_review_counts(
        &self,
        caller: &Caller,
        workspace: Option<&Reference>,
    ) -> Result<TodoCounts, AppError> {
        let none = TodoCounts::default();
        let Some(reviewer) = self
            .sibling_session_caller(&caller.key, Capacity::Review)
            .await?
        else {
            return Ok(none);
        };
        let key = &reviewer.key;
        if require_person_reviewer(key).is_err() {
            return Ok(none);
        }
        let Some(lane) = review_lane(key, None) else {
            return Ok(none);
        };
        let tenant_id = key.tenant_id;
        let (mut nodes, walked) = self.reviewer_nodes_walked(&reviewer).await?;
        if let Some(workspace) = workspace {
            let root = match self.resolve(key, NamedKind::Workspace, workspace).await {
                Ok(root) => root,
                Err(AppError::NotFound) => return Ok(none),
                Err(e) => return Err(e),
            };
            let subtree: BTreeSet<Uuid> = self
                .journal
                .workspace_subtree(tenant_id, root)
                .await?
                .into_iter()
                .map(|w| w.workspace_id)
                .collect();
            nodes.retain(|n| subtree.contains(n));
        }
        if nodes.is_empty() {
            return Ok(none);
        }
        // The brief group's count, under the same reviewer nodes and in one
        // read whatever their number (ADR-0096 d7).
        let pending = self
            .journal
            .pending_brief_edges(
                tenant_id,
                &nodes,
                (key.key_id, key.principal.as_str()),
                None,
                usize::try_from(TODO_COUNT_CAP).unwrap_or(usize::MAX) + 1,
            )
            .await?;
        let briefs_truncated = pending.len() as u64 > TODO_COUNT_CAP;
        let briefs = (pending.len() as u64).min(TODO_COUNT_CAP);
        let filter = HeldFilter {
            nodes,
            author_key_id: None,
            claim_key_prefix: None,
            lane,
        };
        let reviewer_person = self.person_of(key).await?;
        let mut after: Option<ListingPosition> = None;
        let mut review = 0u64;
        let mut scanned = 0usize;
        loop {
            let rows = self
                .journal
                .review_held(
                    tenant_id,
                    &key.principal,
                    key.key_id,
                    &filter,
                    after.as_ref(),
                    HELD_PAGE_MAX,
                )
                .await?;
            for row in &rows {
                // The scan is bounded as the count is: rows the person may
                // not judge are read and never counted, so past
                // `TODO_SCAN_ROWS_MAX` read the count stops and says so
                // (ADR-0083 d11; the review of PR #494, L4).
                if scanned == TODO_SCAN_ROWS_MAX {
                    return Ok(TodoCounts {
                        review,
                        briefs,
                        notices: 0,
                        truncated: true,
                    });
                }
                scanned += 1;
                if self
                    .held_admits(
                        key,
                        &filter,
                        &walked,
                        reviewer_person.as_ref(),
                        &row.memory,
                        None,
                    )
                    .await?
                    .is_some()
                {
                    review += 1;
                    if review > TODO_COUNT_CAP {
                        return Ok(TodoCounts {
                            review: TODO_COUNT_CAP,
                            briefs,
                            notices: 0,
                            truncated: true,
                        });
                    }
                }
            }
            match rows.last() {
                Some(last) if rows.len() == HELD_PAGE_MAX => {
                    after = Some(ListingPosition {
                        created_at: last.held_since.clone(),
                        artifact_id: last.memory.memory_id,
                    });
                }
                _ => break,
            }
        }
        Ok(TodoCounts {
            review,
            briefs,
            notices: 0,
            truncated: briefs_truncated,
        })
    }

    /// The administration To do counts read (ADR-0083 d11, d14): a
    /// person's `<user>-admin` session key's, and no group of it is built yet, so it answers `truncated`
    /// alone, false; each group joins with its feature (ADR-0089).
    pub async fn admin_todo_counts(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<AdminTodoCounts, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        // The administration capacity's session key alone: `<user>-curate`
        // and a tenant admin key share the role, not the capacity (ADR-0083
        // d11; the review of PR #494, L5).
        if !auth::is_admin_session_key(&caller.key) {
            return Err(AppError::VerbCeiling {
                role: caller.key.role,
                verb: "admin_todo_counts",
            });
        }
        // The alerts group: the tenant's unconfirmed fixes that loosen a
        // guardrail, from the rows `GET /v2/admin/fixes` serves (ADR-0096
        // d7).
        let alerts = self
            .journal
            .fixes(caller.key.tenant_id, true)
            .await?
            .iter()
            .filter(|f| f.loosens_guardrail)
            .count() as u64;
        let acts = self.acts_to_confirm(&caller.key).await?;
        Ok(AdminTodoCounts {
            alerts: alerts.min(TODO_COUNT_CAP),
            acts_to_confirm: acts.min(TODO_COUNT_CAP),
            truncated: alerts > TODO_COUNT_CAP || acts > TODO_COUNT_CAP,
        })
    }

    /// "Admin acts to confirm" for `admin`'s person (ADR-0097 d2, d10,
    /// d11), read by `(tenant, state)` within the counts budget (the
    /// bottleneck register): two indexed reads, pending and effective, and
    /// the tenant's admin standing.
    async fn acts_to_confirm(&self, admin: &KeyRecord) -> Result<u64, AppError> {
        use oml_domain::admin_act::AdminActState;
        let Some(view) = self.confirmer_view(admin).await? else {
            return Ok(0);
        };
        let now = self.clock.now();
        let mut count = 0_u64;
        for state in [AdminActState::PendingConfirmation, AdminActState::Effective] {
            // An effective act is one to review only once the person's flag
            // is seven days old: the read is skipped before then (d11).
            if state == AdminActState::Effective
                && !oml_domain::admin_act::counts_for_act(view.since, now.unix_micros())
            {
                continue;
            }
            count += self
                .journal
                .admin_acts(admin.tenant_id, Some(state))
                .await?
                .iter()
                .filter(|a| admin_acts::to_confirm(&view, a, &now))
                .count() as u64;
        }
        Ok(count)
    }

    /// The live admin person `admin` counts as, and when their admin flag
    /// was accepted: what judges which acts they may confirm (ADR-0097
    /// d11); `None` for a key that counts as no live admin person.
    pub(crate) async fn confirmer_view(
        &self,
        admin: &KeyRecord,
    ) -> Result<Option<admin_acts::ConfirmerView>, AppError> {
        let Some(person) = self.counted_person_of(admin).await? else {
            return Ok(None);
        };
        let standing = self.journal.admin_standing(admin.tenant_id).await?;
        Ok(standing
            .persons
            .iter()
            .find(|p| p.user == person && !p.disabled)
            .map(|me| admin_acts::ConfirmerView {
                since: me.admin_since.unix_micros(),
                person,
            }))
    }

    /// `GET /v2/review/briefs` (ADR-0096 d7): the pending brief edges on
    /// workspaces where a person's `<user>-review` holds reviewer effective
    /// access, at `workspace` and below where one is named, oldest first,
    /// a page of at most `limit` strictly after `after`. An edge the key
    /// proposed, or one proposed under its principal, is not listed, since
    /// ratification refuses those (R2). Any other caller is
    /// `access.verb_ceiling`. It journals nothing.
    pub async fn brief_proposals(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Option<&Reference>,
        after: Option<(EventTime, Uuid)>,
        limit: usize,
    ) -> Result<BriefProposalPage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        if !auth::is_review_session_key(&caller.key) {
            return Err(AppError::VerbCeiling {
                role: caller.key.role,
                verb: "list_brief_proposals",
            });
        }
        let key = &caller.key;
        let tenant_id = key.tenant_id;
        let (mut nodes, _) = self.reviewer_nodes_walked(&caller).await?;
        if let Some(workspace) = workspace {
            let root = self.resolve(key, NamedKind::Workspace, workspace).await?;
            let subtree: BTreeSet<Uuid> = self
                .journal
                .workspace_subtree(tenant_id, root)
                .await?
                .into_iter()
                .map(|w| w.workspace_id)
                .collect();
            nodes.retain(|n| subtree.contains(n));
        }
        if nodes.is_empty() {
            return Ok(BriefProposalPage {
                items: Vec::new(),
                next: None,
            });
        }
        let mut edges = self
            .journal
            .pending_brief_edges(
                tenant_id,
                &nodes,
                (key.key_id, key.principal.as_str()),
                after.as_ref().map(|(at, id)| (at, *id)),
                limit + 1,
            )
            .await?;
        let more = edges.len() > limit;
        edges.truncate(limit);
        let next = more
            .then(|| edges.last().map(|e| (e.created_at.clone(), e.edge_id)))
            .flatten();
        let mut items = Vec::with_capacity(edges.len());
        for edge in edges {
            let claim = self.journal.memory(tenant_id, edge.artifact_id).await?;
            // The proposing key's name only where `proposed_by` names it:
            // the reader's own key or another key of the reader's person
            // (ADR-0097 build notes; ADR-0095, amendment of 2026-11-24).
            let proposer = self
                .proposed_by(key, edge.proposer_key_id)
                .await?
                .and_then(|p| p.name);
            let workspace = self
                .journal
                .workspace(tenant_id, edge.workspace_id)
                .await?
                .map(|w| w.name);
            let same_user = self
                .same_person(tenant_id, edge.proposer_key_id, key)
                .await?;
            items.push(BriefProposal {
                claim_key: claim.as_ref().map(|m| m.claim_key.clone()),
                title: claim.and_then(|m| m.format.title.clone()),
                workspace,
                proposer,
                same_user,
                edge,
            });
        }
        Ok(BriefProposalPage { items, next })
    }

    /// The named held rows as the listing would show them to this key, for
    /// the admin page's confirmation (ADR-0055 d10): each id with its row
    /// when it is still held, in the key's reviewer access and outside its
    /// exclusion, else `None`. It claims and journals nothing.
    pub async fn review_named(
        &self,
        bearer: impl Into<Bearer<'_>>,
        ids: &[Uuid],
    ) -> Result<Vec<(Uuid, Option<ServedMemory>)>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_person_reviewer(key)?;
        let (nodes, walked) = self.reviewer_nodes_walked(&caller).await?;
        let nodes: BTreeSet<Uuid> = nodes.into_iter().collect();
        let mut rows: BTreeMap<Uuid, MemoryRecord> = BTreeMap::new();
        for m in self.journal.memories(key.tenant_id, ids).await? {
            if m.status != MemoryStatus::Held
                || !nodes.contains(&m.workspace_id)
                || m.lineage_key_ids.contains(&key.key_id)
                || m.lineage_principals.contains(&key.principal)
            {
                continue;
            }
            // R2 by person: the confirmation shows no row the verdict would
            // refuse `same_person` (ADR-0001 d5; ADR-0082 d7).
            if self.same_person_outside_root(key, &m).await? {
                continue;
            }
            rows.insert(m.memory_id, m);
        }
        let mut ancestries = BTreeMap::new();
        let mut named = Vec::with_capacity(ids.len());
        for id in ids {
            let served = match rows.remove(id) {
                Some(m) => {
                    if let Entry::Vacant(slot) = ancestries.entry(m.workspace_id)
                        && let Some(ancestry) = ancestry_of(&walked, m.workspace_id)
                    {
                        slot.insert(Some(ancestry));
                    }
                    Some(self.served_in(&caller, m, &mut ancestries).await?)
                }
                None => None,
            };
            named.push((*id, served));
        }
        let mut served: Vec<&mut ServedMemory> =
            named.iter_mut().filter_map(|(_, s)| s.as_mut()).collect();
        self.with_findings(key.tenant_id, &mut served).await?;
        Ok(named)
    }

    /// A bulk verdict (ADR-0055 d3, d4): each named id judged in turn by
    /// the single verdict's use case, `verdict_on` — access, R2
    /// against the row's exclusion set, the transition, the frozen refusal,
    /// the compare-and-set, its own signed verdict and ratification in its
    /// own transaction — and answered with its outcome or the refusal a
    /// single verdict naming it would have answered. Each item records under
    /// the request's idempotency key and its own id, so a retry replays what
    /// was decided and judges the rest (d12). Only a reviewer key a person
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
                    workspace_id: Uuid::nil(),
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
        self.require_access(caller, memory.workspace_id, Role::Reviewer, "verdict")
            .await?;
        // A curator rejects inside an in-progress subtree only (ADR-0030
        // d9); R2 below refuses it what it authored or derived from.
        self.curator_may_act(key, "verdict", memory.workspace_id, None)
            .await?;
        let mut exclusion = memory.lineage_closure();
        if let Some(target) = memory.replaces
            && memory.status == MemoryStatus::Held
        {
            for rival in self.journal.held_successors(key.tenant_id, target).await? {
                if rival.memory_id != memory.memory_id
                    && rival.workspace_id == memory.workspace_id
                    && rival.claim_key == memory.claim_key
                {
                    exclusion.add_principal(rival.author_principal);
                    exclusion.add_key_id(rival.author_key_id);
                }
            }
        }
        check_reviewer(key.role, &key.principal, key.key_id, &exclusion)?;
        // Only a person's reviewer key approves an instruction-class
        // version; a reject promotes nothing and any reviewer may give one
        // (ADR-0080 d3, d16). The version stays held.
        if verdict == Verdict::Approve {
            check_approval_class(
                memory.kind,
                key.role,
                key.used_by,
                key.crew_id.is_some(),
                key.profile.is_some(),
            )?;
        }
        // A claim's ratification is marked as a transition's and an edge's
        // are: whether the reviewing key and the authoring key name one
        // user (ADR-0046 d2, ADR-0055 d7).
        let same_user = self.same_person_as_author(&memory, key).await?;
        // R2 by person: outside the author's person's own root, a key acting
        // for that person counts no verdict, approve or reject (ADR-0001 d5;
        // ADR-0082 d7). Inside the root the verdict stands, marked
        // `same_user`.
        if same_user && !self.in_own_root(key, memory.workspace_id).await? {
            return Err(AppError::SamePerson);
        }

        let (verdict_type, target_status) = match verdict {
            Verdict::Approve => (event_types::REVIEW_APPROVE, MemoryStatus::Promoted),
            Verdict::Reject => (event_types::REVIEW_REJECT, MemoryStatus::Rejected),
        };
        // The pure transition function is the arbiter: anything but a held
        // row refuses here (admission.md lifecycle).
        let new_status = transition(memory.status, target_status)?;

        // Nothing new promotes into a `frozen` area: an approval is
        // refused before its verdict is sealed, so nothing is journalled and
        // the row stays held (ADR-0009 d5). Whether a reject is refused too,
        // and whether review_next serves such a row, is open in workspaces.md
        // and waits on the operator.
        if verdict == Verdict::Approve {
            self.refuse_frozen(key.tenant_id, memory.workspace_id, FrozenOp::Promote)
                .await?;
        }
        let replaced = match (verdict, memory.replaces) {
            (Verdict::Approve, Some(target_id)) => {
                let target = self.journal.memory(key.tenant_id, target_id).await?;
                match target {
                    Some(t)
                        if t.status == MemoryStatus::Promoted
                            && t.is_tip
                            && t.workspace_id == memory.workspace_id
                            && t.claim_key == memory.claim_key =>
                    {
                        Some(t)
                    }
                    _ => {
                        return Err(AppError::TipConflict {
                            current_tip: self
                                .readable_tip(caller, memory.workspace_id, &memory.claim_key)
                                .await?,
                        });
                    }
                }
            }
            _ => None,
        };
        // A successor that changes kind inside the supersession class can
        // retire an instruction: approving it then needs a person as well
        // (ADR-0080 d16), and the policy names the stricter class.
        let replaced_kind = replaced.as_ref().map(|t| t.kind);
        if let Some(kind) = replaced_kind {
            check_approval_class(
                kind,
                key.role,
                key.used_by,
                key.crew_id.is_some(),
                key.profile.is_some(),
            )?;
        }
        // An instruction verdict needs a key that counts as a person, not
        // only a person's reviewer key: a minted `used_by: person` key that
        // counts as no one approves no instruction (ADR-0097 d12). A
        // knowledge verdict keeps key-level R2.
        if verdict == Verdict::Approve
            && (memory.kind.class().needs_person()
                || replaced_kind.is_some_and(|k| k.class().needs_person()))
            && self.counted_person_of(key).await?.is_none()
        {
            return Err(AppError::CountsAsNoPerson {
                act: "instruction verdict",
            });
        }

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
                let policy = claim_review_policy(memory.kind, replaced_kind);
                // What the promoted row keeps of this verdict: the reviewing
                // key's attestation, the same-person mark and the verdict's
                // own time, never the key (ADR-0077 d3, d6).
                let approval = Some(Box::new(Approval {
                    used_by: key.used_by,
                    same_user: Some(same_user),
                    at: verdict_event.envelope.at.clone(),
                    fix: None,
                }));
                let (event_type, payload, fold) = match replaced {
                    None => (
                        event_types::LIFECYCLE_PROMOTE,
                        json!({
                            "artifact_id": memory_id.to_string(),
                            "version": memory.version,
                            "workspace_id": memory.workspace_id.to_string(),
                            "claim_key": memory.claim_key,
                            "ratification_event_id": verdict_event_id.to_string(),
                            "previous_tip_version": Value::Null,
                            "self_signed": false,
                            "same_user": same_user,
                            "policy": policy,
                        }),
                        MetadataFold::MemoryPromoted {
                            memory_id,
                            approval,
                        },
                    ),
                    Some(target) => (
                        event_types::LIFECYCLE_SUPERSEDE,
                        json!({
                            "claim_key": memory.claim_key,
                            "workspace_id": memory.workspace_id.to_string(),
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
                            approval,
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
                let committed = match self.commit_requests(links).await {
                    Err(StoreError::TipMoved { .. }) => {
                        return Err(AppError::TipConflict {
                            current_tip: self
                                .readable_tip(caller, memory.workspace_id, &memory.claim_key)
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
        // One recall on the account's meter, and never what was asked,
        // handed to the meter without waiting. A count that is dropped or
        // fails to write leaves the recall served: reading never blocks
        // (ADR-0071 d2, d5, d7, d8).
        if let Some(root) = caller.key.user {
            self.count_recall(caller.key.tenant_id, root).await;
        }
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
                scope_workspace_id: result.scope_workspace_id,
                source_workspace_id: caller.key.default_workspace,
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
        let scope = self.target_workspace(key, req.workspace.as_ref()).await?;
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
            .workspace_ancestry(key.tenant_id, scope)
            .await?
            .unwrap_or_default();
        let ancestry: Vec<Uuid> = scope_ancestry.iter().map(|w| w.workspace_id).collect();
        let applicable: Vec<Uuid> = (0..ancestry.len())
            .rev()
            .filter(|&i| caller.effective(&ancestry[i..]).contains(&Role::Reader))
            .map(|i| ancestry[i])
            .collect();
        // R3 defense in depth, as for the hits below: only a readable served
        // tip in force, never a retirement version (ADR-0025 d8, d11).
        // At an instant, the steering tips then, promoted today (ADR-0059
        // d15, d18).
        // `kinds` decides whether the steering list is served: absent, or
        // naming `steering`; and whether hits are: absent, or naming any
        // other kind (ADR-0092 d2).
        let steering_requested = req.kinds.is_empty() || req.kinds.contains(&MemoryKind::Steering);
        let hits_requested =
            req.kinds.is_empty() || req.kinds.iter().any(|k| *k != MemoryKind::Steering);
        let steering_rows = match &req.as_of {
            _ if !steering_requested => Vec::new(),
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
        let mut steering: Vec<MemoryRecord> = steering_rows
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
                    && applicable.contains(&m.workspace_id)
            })
            .collect();

        // The served node set holds the scope's in-progress subtree and
        // every one below it; where it holds one, `include_unreviewed`
        // defaults to true and admits every author's held rows on the nodes
        // inside. The key's own held rows come from those nodes alone unless
        // the request asks `include_unreviewed: true`, which adds them on
        // every served node (ADR-0030 d2, d3, d4, as ADR-0080 d16 amends
        // them).
        let own_everywhere = req.include_unreviewed == Some(true);
        let ServedNodes {
            nodes: readable,
            inside,
        } = self.served_nodes(caller, scope).await?;
        let include_unreviewed = req.include_unreviewed.unwrap_or(!inside.is_empty());
        if steering_requested {
            steering.extend(
                self.held_steering(
                    caller,
                    &scope_ancestry,
                    &applicable,
                    include_unreviewed,
                    own_everywhere,
                    req,
                )
                .await?,
            );
        }
        // A scope inside a user root also searches `universe`'s own claims
        // where the key reads that node, ranked with the scope's own; own
        // held rows never include them (ADR-0044 d10). Every default recall
        // also searches every public node the key reads, ranked in the one
        // order, counted once (ADR-0098 d3).
        let mut hit_nodes = with_universe(caller, &readable, &scope_ancestry);
        let public = self.public_hit_nodes(caller, &mut hit_nodes).await?;
        // The hit kinds: those named, or with none named every hit kind but
        // the vocabulary kinds, which join only by a workspace's opt-in,
        // unbuilt (ADR-0092 d1, d12 reading 2). The statistics count the
        // knowledge kinds and any vocabulary kind named, whatever else
        // `kinds` names, and the offered instructions by id (d4).
        let kinds: Vec<MemoryKind> = if req.kinds.is_empty() {
            DEFAULT_HIT_KINDS.to_vec()
        } else {
            HIT_KINDS
                .into_iter()
                .filter(|kind| req.kinds.contains(kind))
                .collect()
        };
        let counted_kinds: Vec<MemoryKind> = HIT_KINDS
            .into_iter()
            .filter(|kind| {
                !INSTRUCTION_HIT_KINDS.contains(kind)
                    && (DEFAULT_HIT_KINDS.contains(kind) || kinds.contains(kind))
            })
            .collect();
        // The offered skills and the applicable guardrails at the scope
        // node, the only rows of those kinds that are candidates; read
        // whatever `kinds` names, since they count in the statistics
        // (ADR-0092 d3, d4). An as-of recall reads its scope's knowledge
        // alone: the offered set is today's.
        let offered = if hits_requested && req.as_of.is_none() {
            self.recall_instructions(
                caller,
                req.workspace.as_ref(),
                &scope_ancestry,
                own_everywhere,
                include_unreviewed,
                (!req.include_expired).then_some(now_us),
            )
            .await?
        } else {
            RecallInstructions::default()
        };
        // A recall naming no `sources` reads the tenant's default with its
        // default pipeline, off one row; one naming them reads only the
        // pipeline's default, as before federation (ADR-0072 d12).
        let ((pipeline, tenant_default_unserved), policy) = if req.sources.is_none() {
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
                caller,
                req,
                policy,
                scope,
                &hit_nodes,
                &public,
                &counted_kinds,
                pipeline,
                &query,
                expired_at,
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
        let own_nodes = if own_everywhere {
            readable.clone()
        } else {
            inside.clone()
        };
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
            instructions: offered.ids.clone(),
            counted_kinds: counted_kinds.clone(),
            own_held: include_unreviewed.then(|| OwnHeldScope {
                author_key_ids: caller.own_key_ids(),
                // The key's own held rows on every served node, and every
                // author's on the nodes inside (ADR-0030 d3; ADR-0080 d16).
                nodes: own_nodes.clone(),
                any_author_nodes: inside.clone(),
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
        let ran = if hits_requested {
            self.run_pipeline(
                key.tenant_id,
                pipeline,
                &retrieval_scope,
                &query,
                window.as_ref(),
                req,
                &mut timings,
            )
            .await?
        } else {
            // `kinds: ["steering"]` ranks nothing (ADR-0092 d2).
            PipelineRun::default()
        };
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
        // The request line's `search_fallback` and the recall metric's
        // `fallback` label count either fallback (ADR-0049 d5, d8, d15).
        self.observer.search(
            pipeline.as_str(),
            fallback || tenant_default_unserved.is_some(),
            ran.access.as_str(),
        );
        // R3 defense in depth: the gate changed visibility. Nothing
        // unpromoted or unreadable leaves the application layer except the
        // caller's own held rows when it asked for them; a replaced version
        // or any version of a retired claim is promoted history, never a
        // hit; a retirement version is never in force (ADR-0025 d4, d8).
        // The reranker is handed only rows this admits (ADR-0053 d2).
        let own = caller.own_key_ids();
        let instruction_ids: BTreeSet<Uuid> = offered.ids.iter().map(|(id, _)| *id).collect();
        let admits = |m: &MemoryRecord| {
            // An instruction is a hit only as one of the offered set, which
            // `recall_instructions` admitted by state; a verdict landing
            // before the pipeline hydrates the row is read again here
            // (ADR-0092 d3).
            if INSTRUCTION_HIT_KINDS.contains(&m.kind) {
                return instruction_ids.contains(&m.memory_id)
                    && offered_instruction_still_served(m, self.scanner.version())
                    && !m.retirement
                    && kinds.contains(&m.kind)
                    && (req.include_expired || !m.valid.expired_at(now_us));
            }
            let admitted = match m.status {
                MemoryStatus::Promoted => {
                    (if req.as_of.is_some() {
                        tips_then.contains(&m.memory_id)
                    } else {
                        m.is_tip
                    }) && promoted.contains(&m.workspace_id)
                }
                MemoryStatus::Held => {
                    include_unreviewed
                        && recorded(m)
                        && ((own.contains(&m.author_key_id) && own_nodes.contains(&m.workspace_id))
                            || (served_unreviewed(m, self.scanner.version())
                                && inside.contains(&m.workspace_id)))
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
        // Per kind, the candidates of the fused list before the `k` cut,
        // counted after a reference set's list is restricted to the searched
        // nodes, so no row outside them is counted (ADR-0092 d4).
        let matched_by_kind: Vec<(MemoryKind, u64)> = HIT_KINDS
            .into_iter()
            .map(|k| (k, candidates.iter().filter(|c| c.kind == k).count() as u64))
            .filter(|(_, n)| *n > 0)
            .collect();
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
            // A document's candidate is scored on its passage's search text
            // (ADR-0081 d4).
            let passages: Vec<String> = head
                .iter()
                .filter_map(|c| {
                    rows.get(&c.memory_id)
                        .map(|m| match passage_unit(m, c.ordinal) {
                            Some(unit) => unit.search_text,
                            None => m.searched().to_owned(),
                        })
                })
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
        // The widen hint, on a `scope` answer at the server's now: read off
        // the key's precomputed signal, never off the candidates or their
        // counts, and absent where no usable signal is held (ADR-0072 d8,
        // d12, as built by #244).
        let widen = if plan.report.is_none()
            && req.as_of.is_none()
            && req.now.is_none()
            && policy.is_none_or(|p| p.enabled)
        {
            let prunes = pipeline.retrievers().contains(&RetrieverKind::Bm25Postings)
                && req.match_mode == MatchMode::Any;
            self.widen_hint(
                caller,
                scope,
                &hit_nodes,
                &counted_kinds,
                req.include_expired,
                prunes,
                &query,
            )
            .await
        } else {
            None
        };
        // Spellings only for an empty or thin result, fewer hits than
        // `DID_YOU_MEAN_BELOW` (or than `k` when it asks fewer) before the
        // budget cut, for the
        // words no hit holds, from the vocabulary of the readable nodes
        // alone, under any pipeline but v0.5.0's (ADR-0049 d6, d12).
        // An answer the floor thinned matched its words; its rows were
        // withheld, so it offers no spellings (ADR-0053 d6).
        lap = timings.lap(Stage::Cut, lap);
        // None at an instant: the vocabulary is today's, and a word only a
        // version recorded later holds would name it (ADR-0059 d18).
        let did_you_mean = if pipeline.suggests()
            && req.as_of.is_none()
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
                let home = m.workspace_id;
                // A document's hit is its best passage: its text, bytes
                // and place (ADR-0081 d4).
                let skill = offered.skills.get(&m.memory_id).cloned();
                let unit = passage_unit(&m, candidate.ordinal);
                let mut served = self.served_in(caller, m, &mut ancestries).await?;
                let passage = unit.map(|unit| {
                    let text =
                        oml_passage::passage_text(&served.memory.body, &unit.passage).to_owned();
                    served.memory.body = text;
                    served.memory.body_bytes = unit.body_bytes;
                    HitPassage {
                        fragment: unit.passage.fragment,
                        ordinal: unit.passage.ordinal,
                        start_line: unit.passage.start_line,
                        end_line: unit.passage.end_line,
                        heading_path: unit.passage.heading_path,
                    }
                });
                // A held version's hit names its version, which a read
                // honours for a reader allowed to see it (R10); the
                // fragment is URI text (R11).
                let uri = passage.as_ref().and_then(|p| {
                    let name = served.workspace_path.as_deref()?.rsplit('.').next()?;
                    let held = served.memory.status == oml_domain::MemoryStatus::Held;
                    Some(oml_domain::skills::document_uri(
                        name,
                        &served.memory.claim_key,
                        Some(&oml_passage::uri_fragment(
                            &p.fragment,
                            p.start_line,
                            p.end_line,
                        )),
                        held.then_some(u64::from(served.memory.version)),
                    ))
                });
                hits.push(RecallHit {
                    skill,
                    served,
                    passage,
                    uri,
                    more_passages: candidate.more_passages,
                    score: candidate.score,
                    matched: candidate.matched.clone(),
                    via: candidate.via.clone(),
                    tokens: candidate.tokens.clone(),
                    rerank: candidate.rerank,
                    rank_before: candidate.rank_before,
                    // Each hit of a federated answer says whether its
                    // workspace is the scope's or another team's (ADR-0072
                    // d8).
                    source: plan.report.as_ref().map(|_| {
                        if plan.public.contains(&home) {
                            // A public node outside the scope (ADR-0098 d3).
                            crate::federation::Via::Public
                        } else if plan.federated.contains(&home) {
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
            steering_requested,
            hits_requested,
            matched_by_kind,
            recall_id: self.ids.new_id(),
            scope_workspace_id: scope,
            steering: served_steering,
            hits,
            budget,
            served_ids,
            query_digest: recall_query_digest(&query, req, scope, pipeline, plan.record.as_ref())?,
            pipeline,
            terms,
            fallback,
            tenant_default_unserved,
            did_you_mean,
            rerank,
            floor,
            below_floor,
            access: ran.access,
            pruned: ran.pruned,
            timings,
            window,
            sources: plan.report,
            widen,
            federation: plan.record,
        })
    }

    /// The guardrail read (proposed ADR-0037 d3, d4, d5, d7): reader on the
    /// scope node by the refusal order, as recall is, the node the key's
    /// default or the request's workspace within its grants. The set is
    /// d4's: the promoted served guardrail tips, never a retirement version,
    /// on the scope node and, under the key's `guardrail_scope`
    /// `ancestors`, on each ancestor of it, each only where the key's
    /// reader effective access covers that node; `workspace` keeps the scope
    /// node alone. The scope is the key's own, set at issuance; nothing in
    /// the request changes it. The set is ordered by severity, depth,
    /// `promoted_at` and `artifact_id`, never by a query; rows of one
    /// `claim_key` with different severities each name the others in
    /// `conflict`; over [`SET_CAP`] the set is served whole with
    /// `over_cap`. A held guardrail is served as a line of its own, marked
    /// unreviewed and asking, to its author key under `include_own_held` and
    /// inside an in-progress root (ADR-0080 d10); the caller's own held ones
    /// in the same chain that are not served are listed by `claim_key` and
    /// version alone, at most [`OWN_HELD_MAX_LINES`] (d3). Each served row carries
    /// its approving verdicts' `used_by` and `at`, never a reviewer's key
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
        let scope_node = self.target_workspace(key, req.workspace.as_ref()).await?;
        self.require_access(&caller, scope_node, Role::Reader, "guardrails")
            .await?;
        let tenant_id = key.tenant_id;
        let guardrail_scope = key.guardrail_scope;

        // The chain runs node to root, so each node's own ancestry is its
        // suffix and its depth from the root is the suffix's length less
        // one; the scope bounds it before access does.
        let chain = self
            .journal
            .workspace_ancestry(tenant_id, scope_node)
            .await?
            .unwrap_or_default();
        let ids: Vec<Uuid> = chain.iter().map(|w| w.workspace_id).collect();
        let scope_path = derived_path(&chain);
        let bound = match guardrail_scope {
            GuardrailScope::Workspace => ids.len().min(1),
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
        // The chain's nodes inside an in-progress subtree, where every
        // reader is served every author's held guardrail (ADR-0080 d10,
        // d16).
        let inside: Vec<Uuid> = (0..bound)
            .filter(|&i| nodes.get(&ids[i]).is_some_and(|n| n.1))
            .filter(|&i| in_progress_root_in(&chain[i..]).is_some())
            .map(|i| ids[i])
            .collect();
        let rows = self
            .journal
            .instruction_rows(
                tenant_id,
                MemoryKind::Guardrail,
                &listed,
                key.key_id,
                &inside,
                true,
            )
            .await?;

        // R3 defense in depth: a served rule is a readable promoted tip in
        // force with its fields. A held version is listed to its author by
        // key and version (d3); it is served as a rule only to its author
        // key under `include_own_held` and to the in-progress audience, a
        // row of its own beside any promoted tip, marked and asking; never
        // a retirement, never one the scan held or has not scanned
        // (ADR-0080 d10, d14, d16).
        let mut in_force = Vec::new();
        let mut own_held = Vec::new();
        for m in rows {
            if m.kind != MemoryKind::Guardrail {
                continue;
            }
            let Some(&(depth, reads, _)) = nodes.get(&m.workspace_id) else {
                continue;
            };
            match m.status {
                MemoryStatus::Promoted if m.is_tip && !m.retirement && reads => {
                    if let Some(fields) = m.guardrail.clone()
                        && m.promoted_at.is_some()
                    {
                        in_force.push((depth, *fields, m));
                    }
                }
                MemoryStatus::Held => {
                    let own = m.author_key_id == key.key_id;
                    let served = reads
                        && served_unreviewed(&m, self.scanner.version())
                        && ((own && req.include_own_held) || inside.contains(&m.workspace_id));
                    // A version served as a line is not also listed as held
                    // and not in force.
                    match m.guardrail.clone() {
                        Some(fields) if served => in_force.push((depth, *fields, m)),
                        _ if own => own_held.push(OwnHeldGuardrail {
                            claim_key: m.claim_key.clone(),
                            version: m.version,
                        }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        guardrail_order(&mut in_force, |(depth, fields, m)| {
            (
                fields.severity,
                *depth,
                m.promoted_at.as_ref().unwrap_or(&m.created_at),
                m.memory_id,
            )
        });
        // A held version follows the promoted ones of its severity and
        // depth, so it never takes a promoted rule's place (ADR-0080 d10).
        in_force.sort_by_key(|(depth, fields, m)| {
            (fields.severity, *depth, m.status == MemoryStatus::Held)
        });
        // Held versions are capped on their own, as held steering is, and
        // dropped before any promoted rule is (ADR-0080 d10).
        let mut held_seen = 0;
        in_force.retain(|(_, _, m)| {
            if m.status != MemoryStatus::Held {
                return true;
            }
            held_seen += 1;
            held_seen <= HELD_GUARDRAILS_MAX
        });
        // A conflict is between promoted rules; a held version adds a line
        // and names none, nor is it named.
        let held: BTreeSet<Uuid> = in_force
            .iter()
            .filter(|(_, _, m)| m.status == MemoryStatus::Held)
            .map(|(_, _, m)| m.memory_id)
            .collect();
        let conflict: Vec<Vec<Uuid>> = conflicts(&in_force, |(_, fields, m)| {
            (m.claim_key.as_str(), fields.severity, m.memory_id)
        })
        .into_iter()
        .zip(&in_force)
        .map(|(others, (_, _, m))| {
            if held.contains(&m.memory_id) {
                Vec::new()
            } else {
                others.into_iter().filter(|id| !held.contains(id)).collect()
            }
        })
        .collect();
        own_held.sort_by(|a, b| {
            a.claim_key
                .cmp(&b.claim_key)
                .then_with(|| a.version.cmp(&b.version))
        });
        own_held.truncate(OWN_HELD_MAX_LINES);

        let verdicts = self
            .approvals(tenant_id, in_force.iter().map(|(_, _, m)| m.memory_id))
            .await?;
        let served_ids: Vec<Uuid> = in_force.iter().map(|(_, _, m)| m.memory_id).collect();
        let versions: Vec<(Uuid, u32)> = in_force
            .iter()
            .map(|(_, _, m)| (m.memory_id, m.version))
            .collect();
        // The cap counts the promoted rules alone; held versions are listed
        // beside them and capped on their own, so they never hide an
        // approved rule (ADR-0080 d10).
        let over_cap = in_force
            .iter()
            .filter(|(_, _, m)| m.status != MemoryStatus::Held)
            .count()
            > SET_CAP;
        let guardrails = in_force
            .into_iter()
            .zip(conflict)
            .map(|((depth, fields, m), conflict)| ServedGuardrail {
                artifact_id: m.memory_id,
                workspace_path: nodes.get(&m.workspace_id).map(|(_, _, path)| path.clone()),
                verdicts: verdicts.get(&m.memory_id).cloned().unwrap_or_default(),
                tokens: budget_tokens(m.body_bytes),
                claim_key: m.claim_key,
                version: m.version,
                workspace_id: m.workspace_id,
                depth,
                severity: fields.severity,
                body: m.body,
                rule: fields.rule,
                unreviewed: m.status == MemoryStatus::Held,
                effect_inside: (m.status == MemoryStatus::Held).then_some(Effect::Ask),
                fix: m.approval.as_ref().and_then(|a| a.fix),
                promoted_at: m.promoted_at,
                conflict,
            })
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

    /// The instructions a recall at `workspace` ranks (ADR-0092 d3): the
    /// skills ADR-0080 d4 and d5 offer there, and the guardrails ADR-0037 d4
    /// applies there under the key's `guardrail_scope`, each a promoted tip
    /// in force or a held version its own read serves: to its author key
    /// under `include_unreviewed: true` (`own_held`) and inside an
    /// in-progress subtree when `include_unreviewed` is not false
    /// (`audience`), never one the scan held or has not scanned (ADR-0080
    /// d10, d14, d16). A version whose valid time ended at `expired_at` is
    /// left out. No descendant outside an in-progress root, wide source or
    /// federation source contributes (d11 answer 5).
    async fn recall_instructions(
        &self,
        caller: &Caller,
        workspace: Option<&Reference>,
        chain: &[WorkspaceRecord],
        own_held: bool,
        audience: bool,
        expired_at: Option<i64>,
    ) -> Result<RecallInstructions, AppError> {
        let key = &caller.key;
        let live = |m: &MemoryRecord| !expired_at.is_some_and(|now| m.valid.expired_at(now));
        let mut found = RecallInstructions::default();
        let (scope, offer, rows) = self.offered_skills(caller, workspace, own_held).await?;
        for o in &offer.offered {
            let Some(row) = rows.get(&o.candidate.artifact_id) else {
                continue;
            };
            if (o.candidate.held && !(audience || own_held)) || !live(row) {
                continue;
            }
            let Some(name) = scope.names.get(&row.workspace_id) else {
                continue;
            };
            found.ids.push((row.memory_id, MemoryKind::Skill));
            found.skills.insert(
                row.memory_id,
                SkillHit {
                    prompt: o.command.clone(),
                    uri: skill_uri(name, &o.command),
                },
            );
        }
        // The guardrails: the chain node to root, bounded by the key's
        // scope, on each node the key reads (ADR-0037 d4).
        let ids: Vec<Uuid> = chain.iter().map(|w| w.workspace_id).collect();
        let bound = match key.guardrail_scope {
            GuardrailScope::Workspace => ids.len().min(1),
            GuardrailScope::Ancestors => ids.len(),
        };
        let listed: Vec<Uuid> = (0..bound)
            .filter(|&i| caller.effective(&ids[i..]).contains(&Role::Reader))
            .map(|i| ids[i])
            .collect();
        if listed.is_empty() {
            return Ok(found);
        }
        let inside: Vec<Uuid> = (0..bound)
            .filter(|&i| listed.contains(&ids[i]))
            .filter(|&i| in_progress_root_in(&chain[i..]).is_some())
            .map(|i| ids[i])
            .collect();
        let rows = self
            .journal
            .instruction_rows(
                key.tenant_id,
                MemoryKind::Guardrail,
                &listed,
                key.key_id,
                &inside,
                true,
            )
            .await?;
        for m in rows {
            if m.kind != MemoryKind::Guardrail
                || m.retirement
                || m.guardrail.is_none()
                || !listed.contains(&m.workspace_id)
                || !live(&m)
            {
                continue;
            }
            let served = match m.status {
                MemoryStatus::Promoted => m.is_tip && m.promoted_at.is_some(),
                MemoryStatus::Held => {
                    served_unreviewed(&m, self.scanner.version())
                        && ((m.author_key_id == key.key_id && own_held)
                            || (audience && inside.contains(&m.workspace_id)))
                }
                _ => false,
            };
            if served {
                found.ids.push((m.memory_id, MemoryKind::Guardrail));
            }
        }
        Ok(found)
    }

    /// The offered skill set at the scope node, before paging (ADR-0080 d4,
    /// d5, d10, d14, d16): reader on the scope node by the refusal order, as
    /// the guardrail read is. Its candidates are the skill tips on the scope
    /// node and, under the key's `guardrail_scope` `ancestors`, on each
    /// ancestor it reads and on each node below it that lies inside an
    /// in-progress subtree, where reach goes down as well as up; `workspace`
    /// keeps the scope node alone, in both directions. A held version is a
    /// candidate for its author key under `include_own_held`, and for every
    /// reader on a node inside an in-progress subtree; never a retirement,
    /// never one the scan held or has not scanned (ADR-0074 d20). Each
    /// candidate keeps its row, which a prompt or a resource read serves.
    async fn offered_skills(
        &self,
        caller: &Caller,
        workspace: Option<&Reference>,
        include_own_held: bool,
    ) -> Result<
        (
            SkillScope,
            oml_domain::skills::Offer,
            BTreeMap<Uuid, MemoryRecord>,
        ),
        AppError,
    > {
        let key = &caller.key;
        require_admissible(key, Role::Reader, "skills")?;
        let scope_node = self.target_workspace(key, workspace).await?;
        self.require_access(caller, scope_node, Role::Reader, "skills")
            .await?;
        let tenant_id = key.tenant_id;
        let guardrail_scope = key.guardrail_scope;
        let chain = self
            .journal
            .workspace_ancestry(tenant_id, scope_node)
            .await?
            .unwrap_or_default();
        let ids: Vec<Uuid> = chain.iter().map(|w| w.workspace_id).collect();
        let scope_path = derived_path(&chain);
        let bound = match guardrail_scope {
            GuardrailScope::Workspace => ids.len().min(1),
            GuardrailScope::Ancestors => ids.len(),
        };
        // node -> (distance, ancestor, name, path); `inside` holds the
        // nodes of an in-progress subtree, where every reader is offered
        // every author's held skill (ADR-0080 d10, d16).
        let mut nodes: BTreeMap<Uuid, (u32, bool, String, String)> = BTreeMap::new();
        let mut inside: Vec<Uuid> = Vec::new();
        for i in 0..bound {
            if !caller.effective(&ids[i..]).contains(&Role::Reader) {
                continue;
            }
            let distance = u32::try_from(i).unwrap_or(u32::MAX);
            nodes.insert(
                ids[i],
                (
                    distance,
                    true,
                    chain[i].name.as_str().to_owned(),
                    derived_path(&chain[i..]),
                ),
            );
            if in_progress_root_in(&chain[i..]).is_some() {
                inside.push(ids[i]);
            }
        }
        if guardrail_scope == GuardrailScope::Ancestors {
            let mut walked = BTreeMap::new();
            let below = self
                .nodes_walked(caller, scope_node, Role::Reader, Some(&mut walked))
                .await?;
            let roots = self.subtree_roots(tenant_id, scope_node).await?;
            for node in below {
                if node == scope_node || roots.get(&node).copied().flatten().is_none() {
                    continue;
                }
                let Some(ancestry) = ancestry_of(&walked, node) else {
                    continue;
                };
                let Some(distance) = ancestry.iter().position(|w| w.workspace_id == scope_node)
                else {
                    continue;
                };
                nodes.insert(
                    node,
                    (
                        u32::try_from(distance).unwrap_or(u32::MAX),
                        false,
                        ancestry[0].name.as_str().to_owned(),
                        derived_path(&ancestry),
                    ),
                );
                inside.push(node);
            }
        }
        let listed: Vec<Uuid> = nodes.keys().copied().collect();
        let rows = self
            .journal
            .instruction_rows(
                tenant_id,
                MemoryKind::Skill,
                &listed,
                key.key_id,
                &inside,
                false,
            )
            .await?;
        let mut candidates = Vec::new();
        let mut kept = BTreeMap::new();
        let mut names = BTreeMap::new();
        for m in rows {
            let Some(&(distance, ancestor, ref name, ref path)) = nodes.get(&m.workspace_id) else {
                continue;
            };
            let Some(KindFields::Skill {
                name: skill_name, ..
            }) = m.kind_fields.as_deref()
            else {
                continue;
            };
            let held = match m.status {
                MemoryStatus::Promoted if m.is_tip && !m.retirement => false,
                MemoryStatus::Held
                    if served_unreviewed(&m, self.scanner.version())
                        && ((m.author_key_id == key.key_id && include_own_held)
                            || inside.contains(&m.workspace_id)) =>
                {
                    true
                }
                _ => continue,
            };
            candidates.push(SkillCandidate {
                artifact_id: m.memory_id,
                name: skill_name.clone(),
                node: m.workspace_id,
                path: path.clone(),
                distance,
                ancestor,
                held,
                version: m.version,
                created_at: m.created_at.as_str().to_owned(),
            });
            names.insert(m.workspace_id, name.clone());
            kept.insert(m.memory_id, m);
        }
        Ok((
            SkillScope {
                scope_node,
                scope_path,
                guardrail_scope,
                names,
            },
            offer(candidates),
            kept,
        ))
    }

    /// The skills offered at the scope node, one page of them without
    /// bodies (ADR-0080 d11): the set `offered_skills` computes, in
    /// d5's order, 50 a page after the cursor's position, with the digest
    /// of the whole set. Serving one is a read, not a recall: nothing here
    /// counts in `recalls` (d12).
    pub async fn skills(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &SkillRequest,
    ) -> Result<SkillSet, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let now = self.clock.now();
        let after = req
            .cursor
            .as_deref()
            .map(|cursor| SkillCursor::resume(cursor, caller.key.key_id, &now))
            .transpose()?;
        let (scope, offer, rows) = self
            .offered_skills(&caller, req.workspace.as_ref(), req.include_own_held)
            .await?;
        let versions: Vec<(Uuid, u32)> = offer
            .offered
            .iter()
            .map(|o| (o.candidate.artifact_id, o.candidate.version))
            .collect();
        let mut page: Vec<ServedSkill> = offer
            .offered
            .iter()
            .filter(|o| {
                after
                    .as_ref()
                    .is_none_or(|a| (o.candidate.distance, o.command.clone()) > *a)
            })
            .filter_map(|o| served_skill(&scope, o, rows.get(&o.candidate.artifact_id)?))
            .take(SKILL_PAGE.saturating_add(1))
            .collect();
        let more = page.len() > SKILL_PAGE;
        page.truncate(SKILL_PAGE);
        let next_cursor = more.then(|| {
            let last = page.last().expect("a page past the cap holds rows");
            SkillCursor {
                key_id: caller.key.key_id,
                after: (last.depth, last.name.clone()),
                issued_at: now.clone(),
            }
            .encode()
        });
        Ok(SkillSet {
            scope_node: scope.scope_node,
            scope_path: scope.scope_path,
            guardrail_scope: scope.guardrail_scope,
            set_digest: oml_domain::guardrail::set_digest(&versions),
            served_ids: page.iter().map(|s| s.artifact_id).collect(),
            skills: page,
            next_cursor,
        })
    }

    /// One offered skill with its body, as a prompt or a resource serves it
    /// whole (ADR-0080 d6, d7, d9): a prompt names a command offered at the
    /// scope node; a resource names a workspace and a command of that
    /// workspace's own skills the scope offers, a shadowed one included.
    /// `None` for anything else: an unknown name, one not offered here, a
    /// rejected or retired skill, a held one outside its audience, so the
    /// caller answers every case with one error and discloses no row (d6).
    pub async fn skill(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Option<&Reference>,
        req: &SkillRead,
    ) -> Result<Option<SkillBody>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let (scope, offer, mut rows) = self
            .offered_skills(&caller, workspace, req.include_own_held)
            .await?;
        let found = match &req.workspace_name {
            None => offer.offered.iter().find(|o| o.command == req.command),
            Some(name) => offer.all.iter().find(|o| {
                o.command == req.command
                    && scope
                        .names
                        .get(&o.candidate.node)
                        .is_some_and(|n| n == name)
            }),
        };
        let Some(found) = found else {
            return Ok(None);
        };
        // The winner's own entry carries its overrides; a shadowed one, read
        // by URI, carries none.
        let offered = offer
            .offered
            .iter()
            .find(|o| o.candidate.artifact_id == found.candidate.artifact_id)
            .unwrap_or(found);
        let Some(row) = rows.remove(&found.candidate.artifact_id) else {
            return Ok(None);
        };
        let Some(skill) = served_skill(&scope, offered, &row) else {
            return Ok(None);
        };
        // The set was read without bodies; the one served is read here
        // (ADR-0080 d11). A row gone since, or one whose state, tip or
        // version moved between the two reads, is not served: the header
        // would describe a version other than the body's.
        let Some(full) = self
            .journal
            .memory(caller.key.tenant_id, row.memory_id)
            .await?
            .filter(|f| {
                f.status == row.status && f.is_tip == row.is_tip && f.version == row.version
            })
        else {
            return Ok(None);
        };
        Ok(Some(SkillBody {
            skill,
            body: full.body,
        }))
    }

    /// The held steering a recall's steering list carries after the
    /// promoted rows, marked held (ADR-0080 d10, d14, d16): the key's own on
    /// the applicable nodes under `include_unreviewed`, and every author's
    /// on an applicable node inside an in-progress subtree; never a
    /// retirement, never one the scan held or has not scanned, and never at
    /// an instant. Nearest the root first, then oldest first, as the
    /// promoted rows. At most two listing reads, each only when it can
    /// serve a row.
    async fn held_steering(
        &self,
        caller: &Caller,
        scope_ancestry: &[WorkspaceRecord],
        applicable: &[Uuid],
        include_unreviewed: bool,
        own_everywhere: bool,
        req: &RecallRequest,
    ) -> Result<Vec<MemoryRecord>, AppError> {
        if req.as_of.is_some() {
            return Ok(Vec::new());
        }
        let tenant_id = caller.key.tenant_id;
        let inside: Vec<Uuid> = (0..scope_ancestry.len())
            .filter(|&i| applicable.contains(&scope_ancestry[i].workspace_id))
            .filter(|&i| in_progress_root_in(&scope_ancestry[i..]).is_some())
            .map(|i| scope_ancestry[i].workspace_id)
            .collect();
        let filter = |scope: ListingScope| ListingFilter {
            scope,
            subtree: Some(applicable.to_vec()),
            states: vec![StateClause::status(MemoryStatus::Held)],
            kinds: vec![MemoryKind::Steering],
            claim_key: None,
            prefix: None,
            since: None,
            until: None,
            anchor: crate::types::AnchorFilter::default(),
            author: None,
            as_of: None,
        };
        let current = self.scanner.version().map(str::to_owned);
        let mut rows = Vec::new();
        // `include_unreviewed: false` narrows to promoted rows (ADR-0030 d4).
        if include_unreviewed && !inside.is_empty() {
            rows.extend(
                self.journal
                    .list_memories(
                        tenant_id,
                        &filter(ListingScope::Tips {
                            nodes: inside.clone(),
                            held: inside,
                            current_scanner: current,
                        }),
                        None,
                        HELD_STEERING_MAX,
                    )
                    .await?,
            );
        }
        // The key's own held steering outside an in-progress subtree needs
        // the request's own `true`; the calling key alone, never another key
        // of its person (ADR-0080 d14).
        if include_unreviewed && own_everywhere {
            rows.extend(
                self.journal
                    .list_memories(
                        tenant_id,
                        &filter(ListingScope::Own {
                            key_ids: vec![caller.key.key_id],
                            masked: Vec::new(),
                        }),
                        None,
                        HELD_STEERING_MAX,
                    )
                    .await?,
            );
        }
        let mut seen = BTreeSet::new();
        let mut held: Vec<MemoryRecord> = rows
            .into_iter()
            .filter(|m| {
                m.kind == MemoryKind::Steering
                    && served_unreviewed(m, self.scanner.version())
                    && applicable.contains(&m.workspace_id)
                    && seen.insert(m.memory_id)
            })
            .collect();
        held.sort_by(|a, b| {
            let depth = |m: &MemoryRecord| applicable.iter().position(|n| *n == m.workspace_id);
            depth(a)
                .cmp(&depth(b))
                .then_with(|| a.created_at.as_str().cmp(b.created_at.as_str()))
                .then_with(|| a.memory_id.cmp(&b.memory_id))
        });
        held.truncate(HELD_STEERING_MAX);
        Ok(held)
    }

    /// The approving verdicts on each of `memory_ids`, in journal order: the
    /// reviewing key's `used_by` and the verdict's `at` (proposed ADR-0037
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
        let mut used_by: BTreeMap<Uuid, UsedBy> = BTreeMap::new();
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
            let attested = match used_by.get(&reviewer) {
                Some(attested) => *attested,
                None => {
                    let record = self.journal.key(tenant_id, reviewer).await?.ok_or_else(|| {
                        StoreError::Backend(format!(
                            "review.approve at seq {} names key {reviewer}, which the tenant does not hold",
                            envelope.seq
                        ))
                    })?;
                    used_by.insert(reviewer, record.used_by);
                    record.used_by
                }
            };
            out.entry(memory_id).or_default().push(GuardrailVerdict {
                used_by: attested,
                at: envelope.at,
            });
        }
        Ok(out)
    }

    /// The pipeline a recall runs, and the tenant's default when the
    /// deployment's ran in its place (ADR-0049 d4, d15), reading the
    /// tenant's default itself; see [`Self::pipeline_from`].
    async fn pipeline_for(
        &self,
        tenant_id: Uuid,
        requested: Option<&str>,
    ) -> Result<(Pipeline, Option<String>), AppError> {
        let tenant_default = match requested {
            Some(_) => None,
            None => self.journal.tenant_search_default(tenant_id).await?,
        };
        self.pipeline_from(requested, tenant_default)
    }

    /// The pipeline a recall runs: the request's `search`, which the
    /// deployment must serve, else the tenant's default, else the
    /// deployment's (ADR-0049 d4). A name the request sends that the
    /// deployment does not serve is refused naming it. A tenant default the
    /// deployment does not serve is not refused: the deployment's default
    /// runs, and the tenant's name comes back beside it so the answer says
    /// so with `search.fallback: "tenant_default_unserved"` (ADR-0049 d15).
    fn pipeline_from(
        &self,
        requested: Option<&str>,
        tenant_default: Option<String>,
    ) -> Result<(Pipeline, Option<String>), AppError> {
        if let Some(name) = requested {
            return Pipeline::parse(name)
                .filter(|p| self.search.serves(*p))
                .map(|p| (p, None))
                .ok_or_else(|| AppError::SearchUnavailable {
                    name: name.to_owned(),
                    served: self.served_pipelines(),
                });
        }
        let deployment = self.search.default_pipeline();
        Ok(match tenant_default {
            None => (deployment, None),
            Some(name) => match Pipeline::parse(&name).filter(|p| self.search.serves(*p)) {
                Some(p) => (p, None),
                None => (deployment, Some(name)),
            },
        })
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
            // A document contributes its best passage once (ADR-0081 d4).
            let fused = collapse_passages(fuse_capped(lists, pipeline.fusion(), depth));
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
    /// effective access on the row's workspace; anything else is absence,
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
        // A submitter key recalls nothing but reads its own rows in every
        // state (ADR-0025 d3; ADR-0093 d2), so its role passes step 1 here
        // and `readable` admits its own rows alone, whatever its grants: any
        // other id is `access.not_found`.
        if caller.key.role != Role::Submitter {
            require_admissible(&caller.key, Role::Reader, "get")?;
        }
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
        findings: &mut Vec<VersionFinding>,
    ) -> Result<(), AppError> {
        if let Some(finding) = &parsed.veto {
            return Err(AppError::ScanVeto(finding.clone()));
        }
        // The body is scanned by the caller; the rest of the one list.
        for target in scan_targets(kind, "", fields, parsed).into_iter().skip(1) {
            let at = target.at.unwrap_or("/body");
            let (outcome, found) = self.scanner.assess(&AdmissionEnvelope {
                tenant_id,
                kind: target.kind,
                body: target.text,
            });
            match outcome {
                ScanOutcome::Pass => {}
                ScanOutcome::Hold(reason) => {
                    hold.get_or_insert(format!("{reason} at {at}"));
                    findings.extend(self.admitted(&found, target.field()));
                }
                ScanOutcome::Veto(reason) => {
                    return Err(AppError::ScanVeto(format!("{reason} at {at}")));
                }
            }
        }
        Ok(())
    }

    /// Admission's findings on one scanned text, as the findings list stores
    /// them (ADR-0074 d23): each hold finding, under `field`. A veto refuses
    /// the write before anything is stored, so none is kept.
    fn admitted(&self, found: &[ScanFinding], field: String) -> Vec<VersionFinding> {
        found
            .iter()
            .filter(|f| !f.veto)
            .map(|f| version_finding(f, field.clone(), self.scanner.version(), FoundBy::Admission))
            .collect()
    }

    /// The findings lists of `served`, read in one call and set on each, for
    /// the surfaces a reviewer judges from (ADR-0074 d17, d23).
    async fn with_findings(
        &self,
        tenant_id: Uuid,
        served: &mut [&mut ServedMemory],
    ) -> Result<(), AppError> {
        let ids: Vec<Uuid> = served.iter().map(|s| s.memory.memory_id).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let mut lists: BTreeMap<Uuid, Vec<VersionFinding>> = BTreeMap::new();
        for (id, f) in self.journal.scan_findings(tenant_id, &ids).await? {
            lists.entry(id).or_default().push(f);
        }
        for s in served.iter_mut() {
            s.scan_findings = Some(lists.remove(&s.memory.memory_id).unwrap_or_default());
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

    /// A skill's name and description and a document's title reach the
    /// journal in clear in the signed submission, so a veto in them refuses
    /// as one in the body does (ADR-0065 d7; ADR-0080 d1; ADR-0081 d1).
    fn scan_kind_fields(
        &self,
        tenant_id: Uuid,
        fields: Option<&KindFields>,
    ) -> Result<(), AppError> {
        match fields {
            Some(KindFields::Skill { name, description }) => {
                self.scan_veto_only(tenant_id, "skill.name", name)?;
                self.scan_veto_only(tenant_id, "skill.description", description)
            }
            Some(KindFields::Document { title: Some(title) }) => {
                self.scan_veto_only(tenant_id, "document.title", title)
            }
            _ => Ok(()),
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
    /// d5, d18): from its author key (a contributor), or a reviewer with reviewer
    /// access on its workspace; a contributor that can read it and did not author
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
        let reviewer = admissible.contains(&Role::Reviewer);
        if !reviewer && !admissible.contains(&Role::Submitter) {
            return Err(AppError::VerbCeiling {
                role: key.role,
                verb: "source_check",
            });
        }
        let scope = IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workspace_id: Uuid::nil(),
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
        if reviewer {
            self.require_access(&caller, target.workspace_id, Role::Reviewer, "source_check")
                .await?;
        } else {
            if target.author_key_id != key.key_id {
                return Err(AppError::AuthorOnly);
            }
            self.require_access(
                &caller,
                target.workspace_id,
                Role::Submitter,
                "source_check",
            )
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
            "workspace_id": target.workspace_id.to_string(),
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

    /// Withdraw a held version (ADR-0083 d13): the author key on its own
    /// held version, an agent's included, or a person's session key on a
    /// held version any key of that person wrote, read the one way
    /// `same_user` reads it; any workspace, frozen ones included. The
    /// idempotency record, scoped to the key with a nil workspace, is
    /// looked up first and replayed whatever the version's state now; then a
    /// version the key cannot read is told nothing, another key is
    /// `access.author_only`, and a version not held is `memory.not_held`.
    /// The reason is scanned as a verdict's is. Journals `memory.withdrawn
    /// {artifact_id, reason}` under the withdrawing key and folds the state,
    /// leaving every review queue, in one commit with the idempotency
    /// record. No verdict is recorded: R1 is untouched, since nothing is
    /// promoted, signed, demoted or deleted.
    pub async fn withdraw(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &WithdrawRequest,
    ) -> Result<(WithdrawReceipt, bool), AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        let reason = req.reason.as_deref().filter(|r| !r.is_empty());
        let scope = IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workspace_id: Uuid::nil(),
            idem_key: req.idempotency_key.clone(),
        };
        let request_digest = sha256_digest_string(&canonical_bytes(&json!({
            "method": "POST",
            "route": "/v1/memories/{artifact_id}/withdraw",
            "path_params": { "artifact_id": req.memory_id.to_string() },
            "body": { "reason": reason },
        }))?);
        if let Some(record) = self.journal.idempotency_get(&scope).await? {
            if record.request_digest != request_digest {
                return Err(AppError::IdempotencyDigestMismatch);
            }
            let receipt: WithdrawReceipt = serde_json::from_value(record.response)
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
        let by = if target.author_key_id == key.key_id {
            Withdrawer::AuthorKey
        } else if key.credential == Credential::Session
            && self.same_person_as_author(&target, key).await?
        {
            Withdrawer::PersonSession
        } else {
            Withdrawer::Other
        };
        let state = withdraw(target.status, by).map_err(AppError::Withdraw)?;
        if let Some(reason) = reason {
            self.scan_veto_only(key.tenant_id, "reason", reason)?;
        }
        let at = self.clock.now();
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::MEMORY_WITHDRAWN,
            &key.principal,
            Some(key.key_id),
            json!({
                "artifact_id": target.memory_id.to_string(),
                "workspace_id": target.workspace_id.to_string(),
                "reason": reason,
            }),
        )?;
        let receipt = WithdrawReceipt {
            memory_id: target.memory_id,
            state: state.as_str().to_owned(),
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
            MetadataFold::MemoryWithdrawn {
                memory_id: target.memory_id,
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
    /// runs in its order: a key role admitting submitter or reviewer (step 1);
    /// the idempotency record, scoped to the key with a nil workspace, looked
    /// up next and replayed whatever the target's state now; the target
    /// readable (step 2), and, where it is a ratified retirement in a user
    /// root, the key that user's own, else `submission.user_root_refused`
    /// (ADR-0044 d16); a submitting key's own target and submitter on its workspace
    /// (step 3), or a reviewer's reviewer and reader there, whoever authored
    /// it (step 4, edit-as-proposal); then the target-dependent refusals —
    /// `X-OML-Workarea`, the served tip, the kind and retirement checks,
    /// and from `oml_write` the vocabulary-kind check, and last the
    /// workspace not `frozen`, else `workspace.frozen` (step 5; ADR-0009
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
        self.supersede_as(bearer.into(), req, None).await
    }

    /// `POST /v1/memories/preview` (ADR-0082 d5, d10): what capture would
    /// decide for a submit's body, or a supersession's, deciding it through
    /// the same pipeline and stopping before anything is sealed, committed,
    /// held, replayed or metered (ADR-0071 d2). The key is authenticated
    /// once, so the request rate is charged once (ADR-0079). Every refusal
    /// the pipeline makes is the answer's `refusal`, beside what it read
    /// before it; which of them a transport answers as its error is the
    /// transport's (d10). On a version capture would take, a skill answers
    /// its prompt name and the same-named skills along its chain, and a
    /// guardrail its canonical rule and the set size it leaves.
    pub async fn preview(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &PreviewRequest,
    ) -> Result<Preview, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let mut facts = PreviewFacts::default();
        let (decided, body, rule, kind_fields) = match req {
            PreviewRequest::Submit(r) => (
                self.submit_pipeline(Bearer::Session(&caller), r, None, Some(&mut facts))
                    .await
                    .map(|_| ()),
                &r.body,
                r.rule.as_ref(),
                r.kind_fields.as_ref(),
            ),
            PreviewRequest::Supersede(r) => (
                self.supersede_pipeline(Bearer::Session(&caller), r, None, Some(&mut facts))
                    .await
                    .map(|_| ()),
                &r.body,
                r.rule.as_ref(),
                r.kind_fields.as_ref(),
            ),
        };
        let body_bytes = nfc(body).len() as u64;
        let kind = facts.kind.unwrap_or(match req {
            PreviewRequest::Submit(r) => r.kind,
            PreviewRequest::Supersede(r) => r.kind.unwrap_or(MemoryKind::Memory),
        });
        let retirement = matches!(req, PreviewRequest::Supersede(r) if r.retirement);
        let needs_person = kind.class().needs_person()
            || facts
                .replaces
                .as_ref()
                .is_some_and(|t| t.kind.class().needs_person());
        let mut preview = Preview {
            refusal: decided.err(),
            findings: std::mem::take(&mut facts.findings),
            needs_person,
            body_bytes,
            budget_tokens: budget_tokens(body_bytes),
            rule: None,
            offered_as: None,
            shadows: Vec::new(),
            set_after: None,
            passages: None,
        };
        let Some(workspace_id) = facts.workspace_id.filter(|_| preview.refusal.is_none()) else {
            return Ok(preview);
        };
        match kind {
            MemoryKind::Skill if !retirement => {
                let name = match (kind_fields, facts.replaces.as_ref()) {
                    (Some(KindFields::Skill { name, .. }), _) => Some(name.clone()),
                    (_, Some(target)) => match target.kind_fields.as_deref() {
                        Some(KindFields::Skill { name, .. }) => Some(name.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(name) = name {
                    preview.shadows = self.skill_shadows(&caller, workspace_id, &name).await?;
                    preview.offered_as = Some(name);
                }
            }
            // A document's passages, cut by the rule capture cuts with, by
            // fragment and lines (ADR-0081 d3; ADR-0082 d5).
            MemoryKind::Document if !retirement => {
                let format = match req {
                    PreviewRequest::Submit(r) => r.format.as_deref(),
                    PreviewRequest::Supersede(r) => r.format.as_deref(),
                }
                .unwrap_or_else(|| default_format(MemoryKind::Document));
                let cut = if format == "text/markdown" {
                    oml_passage::Format::Markdown
                } else {
                    oml_passage::Format::Plain
                };
                preview.passages = oml_passage::passages(&nfc(body), cut).ok().map(|cut| {
                    cut.into_iter()
                        .map(|p| HitPassage {
                            fragment: p.fragment,
                            ordinal: p.ordinal,
                            start_line: p.start_line,
                            end_line: p.end_line,
                            heading_path: p.heading_path,
                        })
                        .collect()
                });
            }
            MemoryKind::Guardrail => {
                preview.rule = rule
                    .and_then(|r| guardrail::Rule::parse(r).ok())
                    .map(|r| r.to_value());
                preview.set_after = self
                    .guardrail_set_after(&caller, workspace_id, facts.replaces.as_ref(), retirement)
                    .await?;
            }
            _ => {}
        }
        Ok(preview)
    }

    /// The same-named promoted skills along `workspace_id`'s chain that the
    /// caller reads: an ancestor's, which a skill there shadows, and a
    /// descendant's, which shadows it below (ADR-0080 d5). A workspace the
    /// caller cannot read names nothing.
    async fn skill_shadows(
        &self,
        caller: &Caller,
        workspace_id: Uuid,
        name: &str,
    ) -> Result<Vec<SkillShadow>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let chain = self
            .journal
            .workspace_ancestry(tenant_id, workspace_id)
            .await?
            .unwrap_or_default();
        let ids: Vec<Uuid> = chain.iter().map(|w| w.workspace_id).collect();
        let mut paths: BTreeMap<Uuid, (String, bool)> = BTreeMap::new();
        for i in 1..ids.len() {
            if caller.effective(&ids[i..]).contains(&Role::Reader) {
                paths.insert(ids[i], (derived_path(&chain[i..]), false));
            }
        }
        let mut walked = BTreeMap::new();
        for node in self
            .nodes_walked(caller, workspace_id, Role::Reader, Some(&mut walked))
            .await?
        {
            if node == workspace_id {
                continue;
            }
            if let Some(ancestry) = ancestry_of(&walked, node) {
                paths.insert(node, (derived_path(&ancestry), true));
            }
        }
        let nodes: Vec<Uuid> = paths.keys().copied().collect();
        let rows = self
            .journal
            .instruction_rows(
                tenant_id,
                MemoryKind::Skill,
                &nodes,
                caller.key.key_id,
                &[],
                false,
            )
            .await?;
        let mut shadows: Vec<SkillShadow> = rows
            .iter()
            .filter(|m| m.status == MemoryStatus::Promoted && m.is_tip && !m.retirement)
            .filter(|m| {
                matches!(m.kind_fields.as_deref(), Some(KindFields::Skill { name: n, .. }) if n == name)
            })
            .filter_map(|m| {
                paths.get(&m.workspace_id).map(|(path, below)| SkillShadow {
                    workspace_path: path.clone(),
                    shadowed_by: *below,
                })
            })
            .collect();
        shadows.sort_by(|a, b| {
            (a.shadowed_by, &a.workspace_path).cmp(&(b.shadowed_by, &b.workspace_path))
        });
        shadows.dedup();
        Ok(shadows)
    }

    /// The promoted guardrails an agent at `workspace_id` would load after
    /// the previewed version is approved: the set in force under the
    /// caller's own guardrail scope, one more for a new rule, one fewer for
    /// the retirement of a rule in it (ADR-0037 d7; ADR-0082 d5); `None`
    /// for a caller that does not read the workspace.
    async fn guardrail_set_after(
        &self,
        caller: &Caller,
        workspace_id: Uuid,
        replaces: Option<&MemoryRecord>,
        retirement: bool,
    ) -> Result<Option<usize>, AppError> {
        // A key that writes the workspace but does not read it is told no
        // set it may not read, and the preview still answers what capture
        // would decide (the review of PR #494, L3).
        let ancestry = self.ancestry(caller.key.tenant_id, workspace_id).await?;
        if caller.denied(&ancestry) || !caller.effective(&ancestry).contains(&Role::Reader) {
            return Ok(None);
        }
        let set = self
            .guardrails(
                Bearer::Session(caller),
                &GuardrailRequest {
                    workspace: Some(Reference::Id(workspace_id)),
                    include_own_held: false,
                },
            )
            .await?;
        let in_force = set.guardrails.iter().filter(|g| !g.unreviewed).count();
        let replaced_in_force = replaces.is_some_and(|t| set.served_ids.contains(&t.memory_id));
        Ok(Some(match (replaced_in_force, retirement) {
            (true, true) => in_force.saturating_sub(1),
            (true, false) | (false, true) => in_force,
            (false, false) => in_force + 1,
        }))
    }

    /// [`Self::supersede`], with an import's record of the write stored in
    /// its own commit (ADR-0084 I14).
    pub(crate) async fn supersede_as(
        &self,
        bearer: Bearer<'_>,
        req: &SupersedeRequest,
        imported: Option<&crate::types::ImportedWrite>,
    ) -> Result<SupersedeOutcome, AppError> {
        self.supersede_pipeline(bearer, req, imported, None)
            .await?
            .ok_or_else(|| {
                StoreError::Backend("a capture stopped at its decision".to_owned()).into()
            })
    }

    /// The supersede pipeline: every check a supersession makes, then the
    /// capture. With `preview` it takes no idempotency record and replays
    /// none, sets what it decides on `preview` as it goes, and stops before
    /// anything is sealed, committed or metered, answering `None` (ADR-0082
    /// d5, d10).
    async fn supersede_pipeline(
        &self,
        bearer: Bearer<'_>,
        req: &SupersedeRequest,
        imported: Option<&crate::types::ImportedWrite>,
        mut preview: Option<&mut PreviewFacts>,
    ) -> Result<Option<SupersedeOutcome>, AppError> {
        let caller = self.authenticate_bearer(bearer).await?;
        let key = &caller.key;
        let admissible = admissible_grant_roles(key.role);
        let reviewer = admissible.contains(&Role::Reviewer);
        if !reviewer && !admissible.contains(&Role::Submitter) {
            return Err(AppError::VerbCeiling {
                role: key.role,
                verb: "supersede",
            });
        }
        // The body's workspace, by id or name, resolves after step 1
        // (ADR-0026 d4).
        let header = self
            .resolve_opt(key, NamedKind::Workspace, req.workspace.as_ref())
            .await?;

        // The record is looked up between steps 1 and 2 and replays its
        // stored response whatever the target or the key's access now: the
        // response names only the caller's own successor (http-api.md
        // idempotency). The scope's workspace is nil, so no target's
        // workspace ever enters a record.
        let scope = IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workspace_id: Uuid::nil(),
            idem_key: req.idempotency_key.clone(),
        };
        let request_digest = supersede_request_digest(req)?;
        // A preview takes no record and replays none (ADR-0082 d10).
        if preview.is_none()
            && let Some(receipt) = self.recorded(&scope, &request_digest).await?
        {
            return Ok(Some(SupersedeOutcome {
                receipt: self.replayed_supersede(&caller, receipt).await?,
                replayed: true,
            }));
        }
        // The account's limits, read once and before the head, as a
        // submit's are, a session key's its person's (ADR-0071 d3;
        // ADR-0082 d10, note of 2026-11-19).
        let ceiling = self
            .ceiling_for(key.tenant_id, self.limits_root_of(key).await?)
            .await?;

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
        // A successor of a ratified retirement puts the claim back in
        // service, and on its account's `memories`, when it is ratified: in
        // a user root that is a memory started, which is that user's keys'
        // alone, whatever grant the key holds there. The key reads the
        // target, so the rule is named ahead of steps 3 and 4. Any other
        // successor there is governed by those steps, as before (ADR-0044
        // d16; ADR-0071 d2).
        if target.retirement && target.status == MemoryStatus::Promoted && target.is_tip {
            self.refuse_another_users_root(key, target.workspace_id)
                .await?;
        }
        // A curator retires or rewrites inside an in-progress subtree only,
        // and never what it authored or derived from (ADR-0030 d9).
        self.curator_may_act(key, "supersede", target.workspace_id, Some(&target))
            .await?;
        if reviewer {
            // Step 4: edit-as-proposal needs reviewer and reader on the
            // target's workspace, whoever authored it.
            self.require_access(&caller, target.workspace_id, Role::Reviewer, "supersede")
                .await?;
            self.require_access(&caller, target.workspace_id, Role::Reader, "supersede")
                .await?;
        } else {
            // Step 3: own is the target version's authoring key, never the
            // claim's first version (roles-and-keys.md#own); for a person's
            // session key, any key of its person, read the one way
            // `same_user` reads it (ADR-0082 d2, d8 item 2).
            let own = target.author_key_id == key.key_id
                || (key.credential == Credential::Session
                    && self
                        .same_person(key.tenant_id, target.author_key_id, key)
                        .await?);
            if !own {
                return Err(AppError::AuthorOnly);
            }
            self.require_access(&caller, target.workspace_id, Role::Submitter, "supersede")
                .await?;
        }
        // Step 5, in its order: the scope is the target's workspace, the
        // target is the served tip, the kind stays in its class and a
        // retirement keeps the target's kind and never retires a term, and
        // on oml_write the target is of no vocabulary kind (ADR-0025 d12).
        if let (Some(workspace_id), Some(sent)) = (header, &req.workspace)
            && workspace_id != target.workspace_id
        {
            return Err(AppError::WorkspaceNotTarget {
                workspace: sent.clone(),
            });
        }
        // The claim's latest version the caller may read (promoted, or held
        // by its own keys), read after the head: a capture that commits in
        // between moves the head and the retry reads it again. Anything but
        // the version the caller names is refused before the tip check,
        // nothing captured; another author's held version never changes the
        // answer (ADR-0057 d19; R3).
        // A preview knows the version it replaces before any version check
        // refuses, so its kind decides `needs_person` beside that refusal
        // (ADR-0082 d5; the review of PR #494, L2).
        if let Some(facts) = preview.as_deref_mut() {
            facts.workspace_id = Some(target.workspace_id);
            facts.kind =
                Some(successor_kind(target.kind, req.kind, req.retirement).unwrap_or(target.kind));
            facts.replaces = Some(target.clone());
        }
        let claim_state = self
            .journal
            .claim_key_state(key.tenant_id, target.workspace_id, &target.claim_key)
            .await?;
        let reads_promoted = self
            .require_access(&caller, target.workspace_id, Role::Reader, "supersede")
            .await
            .is_ok();
        let latest = self
            .journal
            .claim_latest_readable(
                key.tenant_id,
                target.workspace_id,
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
            if preview.is_none()
                && let Some(receipt) = self.recorded(&scope, &request_digest).await?
            {
                return Ok(Some(SupersedeOutcome {
                    receipt: self.replayed_supersede(&caller, receipt).await?,
                    replayed: true,
                }));
            }
            return Err(AppError::VersionConflict { current: latest });
        }
        if target.status != MemoryStatus::Promoted || !target.is_tip {
            return Err(AppError::TipConflict {
                current_tip: self
                    .readable_tip(&caller, target.workspace_id, &target.claim_key)
                    .await?,
            });
        }
        let kind = successor_kind(target.kind, req.kind, req.retirement)?;
        if let Some(facts) = preview.as_deref_mut() {
            facts.workspace_id = Some(target.workspace_id);
            facts.kind = Some(kind);
            facts.replaces = Some(target.clone());
        }
        if req.refuse_vocabulary_target
            && matches!(
                target.kind,
                MemoryKind::Term | MemoryKind::TermMapping | MemoryKind::VocabRelease
            )
        {
            return Err(AppError::VocabularyTarget(target.kind));
        }
        // A guardrail is superseded over `oml_write` too, with its
        // `guardrail` object, and only into kind guardrail (ADR-0095 d2;
        // `successor_kind`).
        // Nor a document or a test case, which are written on the HTTP API
        // with their objects until their serving lands (#405, #419): a
        // successor of one takes its kind. A skill is superseded over
        // `oml_write` with its object (ADR-0080 d3).
        if req.refuse_vocabulary_target
            && matches!(target.kind, MemoryKind::Document | MemoryKind::Test)
        {
            return Err(AppError::KindField(oml_domain::claim_kinds::FieldRefusal {
                field: "/kind",
                reason: "a document or a test case is superseded and retired on the HTTP API in this release, with its object",
            }));
        }
        // Step 5's last check, after the head read above: a ratification
        // that validates the area in between moves the head and the commit
        // conflicts (roles-and-keys.md supersede; ADR-0009 d5).
        self.refuse_frozen(key.tenant_id, target.workspace_id, FrozenOp::Write)
            .await?;

        // The checks every submit runs: NFC first, then the scan, whose veto
        // journals nothing, then every declared ancestor readable
        // (admission.md capture; ADR-0025 d3, d6). A guardrail successor
        // re-sends its fields, which run their checks first (proposed
        // ADR-0037 d10).
        let body = nfc(&req.body);
        let requested_format = req
            .format
            .clone()
            .unwrap_or_else(|| default_format(kind).to_owned());
        let (format, parsed) = self.capture_format(
            kind,
            Some(&requested_format),
            req.title.as_deref(),
            &body,
            req.retirement,
        )?;
        let guardrail =
            guardrail_fields(kind, req.retirement, req.severity, req.rule.as_ref(), &body)?;
        let (admitted_fields, _) = kind_fields(
            kind,
            req.retirement,
            true,
            req.kind_fields.as_ref(),
            Some(&target.claim_key),
            &body,
            &requested_format,
        )?;
        self.scan_kind_fields(key.tenant_id, admitted_fields.as_ref())?;
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
        let (outcome, found) = self.scanner.assess(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: kind.as_str(),
            body: &body,
        });
        let scan_finding = match outcome {
            ScanOutcome::Pass => None,
            ScanOutcome::Hold(reason) => Some(reason),
            ScanOutcome::Veto(reason) => return Err(AppError::ScanVeto(reason)),
        };
        let mut scan_finding = scan_finding;
        let mut findings = self.admitted(&found, "/body".to_owned());
        self.scan_derived(
            key.tenant_id,
            kind,
            &format,
            &parsed,
            &mut scan_finding,
            &mut findings,
        )?;
        if let Some(facts) = preview.as_deref_mut() {
            facts.findings.clone_from(&findings);
        }
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
        // version of the claim_key in the workspace, whatever its state; a
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

        // A new version charges the account's bytes and monthly writes and
        // never its memories; nothing is journalled past them. As a
        // submit's, the check follows every other refusal and the replay
        // step 5 makes when an attempt under the same key committed since
        // the lookup, so a retry at the limit gets its stored receipt and a
        // revision refused for its target is the refusal it was. This check
        // spares the signature; the commit's ceiling is the one that holds
        // (ADR-0071 d2, d3).
        self.require_within(
            key.tenant_id,
            ceiling.as_ref(),
            &[
                (oml_domain::signup::Quota::TextBytes, body.len() as u64),
                (oml_domain::signup::Quota::WritesPerMonth, 1),
            ],
        )
        .await?;

        // The similar check, as a submit's, never naming a version of the
        // claim this replaces; a retirement states why and is not checked
        // (ADR-0050 d1).
        let similar = if req.retirement {
            Vec::new()
        } else {
            self.similar_gate(
                &caller,
                target.workspace_id,
                kind,
                format.search_text.as_deref().unwrap_or(&body),
                Some(&target.claim_key),
                req.on_similar,
            )
            .await?
        };

        // A preview stops here: nothing is sealed, committed or metered.
        if preview.is_some() {
            return Ok(None);
        }
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
            workspace_id: target.workspace_id,
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
            author_person: self.counted_person_of(key).await?,
            derives_from: derives_from.clone(),
            lineage_principals: principals.into_iter().collect(),
            lineage_key_ids: key_ids.into_iter().collect(),
            scan_finding: scan_finding.clone(),
            scanned_under: self.scanner.version().map(str::to_owned),
            created_at: self.clock.now(),
            guardrail,
            kind_fields: admitted_fields.map(Box::new),
            valid: Box::new(valid),
            anchor: req.anchor.clone().map(Box::new),
            origin_kind: Some(origin),
            source_check: None,
            approval: None,
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
            "workspace_id": target.workspace_id.to_string(),
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
        let mut payload = with_kind_fields(payload, successor.kind_fields.as_deref());
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
            workspace_id: target.workspace_id,
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
            .commit_written(
                event,
                MetadataFold::MemoryHeld {
                    imported: imported.cloned().map(Box::new),
                    memory: successor,
                    enqueue: true,
                    findings: findings.into(),
                },
                idempotency,
                key,
                false,
                ceiling,
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
            return Ok(Some(SupersedeOutcome {
                receipt: self.replayed_supersede(&caller, receipt).await?,
                replayed: true,
            }));
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
        Ok(Some(SupersedeOutcome {
            receipt,
            replayed: false,
        }))
    }

    /// Propose a claim edge `from_artifact_id -> to_artifact_id` carrying
    /// `label`: journalled `claim_edge.proposed`, stored pending, and in its
    /// own commit widening the lineage closure of every version it connects,
    /// so R2 counts the edge while it waits (ADR-0008 d5; ADR-0012;
    /// audit-and-lineage.md, pending and ratified alike). The refusal order:
    /// the verb ceiling, the idempotency record, each endpoint present and in
    /// read scope, reviewer on at least one endpoint's workspace, then the
    /// label (workspaces.md, edge authority; ADR-0021 d5). A `used_by:
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
        require_admissible(key, Role::Reviewer, VERB)?;

        // A claim-edge POST names no workspace, so the scope's workspace is
        // nil (http-api.md idempotency); the record replays whatever the
        // endpoints or the key's access now, as it names only the caller's
        // own edge.
        let request_digest = claim_edge_request_digest(req)?;
        let scope = idempotency_key.map(|idem_key| IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workspace_id: Uuid::nil(),
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

        // An edge endpoint on a public node counts as read (ADR-0098 d1).
        let grants: Vec<LabelledGrant> = caller
            .reach
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
            let ancestry = self.ancestry(key.tenant_id, memory.workspace_id).await?;
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
            // `access.grant_refused` naming its workspace.
            return Err(match refusal {
                VerbRefusal::VerbCeiling { role, .. } => AppError::VerbCeiling {
                    role: *role,
                    verb: VERB,
                },
                VerbRefusal::GrantRefused { needed_role, .. } => AppError::GrantRefused {
                    workspace_id: endpoints[0].workspace_id,
                    needed_role: *needed_role,
                },
                VerbRefusal::NotFound => AppError::NotFound,
            });
        }
        let (src, dst) = (&endpoints[0], &endpoints[1]);
        self.refuse_user_edge_into_universe(key, &[src.workspace_id, dst.workspace_id])
            .await?;
        self.refuse_crossing_edge(key.tenant_id, src.workspace_id, dst.workspace_id)
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
            self.curator_may_act(key, VERB, src.workspace_id, Some(src))
                .await?;
            self.curator_may_act(key, VERB, dst.workspace_id, Some(dst))
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

    /// Propose that the promoted claim `artifact_id` join `workspace`'s
    /// brief: journalled `brief_edge.proposed` and stored pending (ADR-0046
    /// d7, d8). The refusal order: the verb ceiling, a key of role
    /// contributor or reviewer (ADR-0095 d6), the idempotency record, the
    /// workspace resolved and submitter effective access on it for a
    /// contributor key, reviewer for a reviewer key (`access.not_found`
    /// outside read access, `access.grant_refused` within it), the claim
    /// present and readable (`access.not_found` otherwise), the claim
    /// promoted and its claim's served tip (`request.validation` naming
    /// `from_artifact_id`), then the claim neither a live proposal nor a
    /// ratified member of the same brief (`proposal.pending` naming the
    /// edge; ADR-0095 d6, #515). A `used_by: service` key may propose; only
    /// ratification is review-class.
    pub async fn propose_brief_edge(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &ProposeBriefEdge,
        idempotency_key: Option<&str>,
    ) -> Result<BriefEdgeProposalOutcome, AppError> {
        const VERB: &str = "brief_edge.propose";
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        // The proposer floor is a contributor or a reviewer key: every
        // member is already approved, and its place is approved again by a
        // reviewer who did not propose it (ADR-0095 d6).
        let needed = match key.role {
            Role::Contributor => Role::Submitter,
            Role::Reviewer => Role::Reviewer,
            role => return Err(AppError::VerbCeiling { role, verb: VERB }),
        };
        let request_digest = brief_edge_request_digest(req)?;
        let scope = idempotency_key.map(|idem_key| IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workspace_id: Uuid::nil(),
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
        let workspace_id = self
            .resolve(key, NamedKind::Workspace, &req.workspace)
            .await?;
        self.require_access(&caller, workspace_id, needed, VERB)
            .await?;
        let memory = self
            .journal
            .memory(key.tenant_id, req.artifact_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if !self.readable(&caller, &memory).await? {
            return Err(AppError::NotFound);
        }
        self.refuse_user_edge_into_universe(key, &[memory.workspace_id, workspace_id])
            .await?;
        self.refuse_crossing_edge(key.tenant_id, memory.workspace_id, workspace_id)
            .await?;
        if memory.status != MemoryStatus::Promoted || !memory.is_tip {
            return Err(AppError::BriefMemberNotServed {
                artifact_id: memory.memory_id,
            });
        }
        // One proposal per claim and brief: a claim already proposed, or
        // already a member, is `proposal.pending` naming that edge. Read
        // after the head, as every check above, so an edge committed in
        // between moves the head and this commit conflicts and runs again
        // (ADR-0095 d6; #515).
        if let Some(existing) = self
            .journal
            .brief_edges(key.tenant_id, workspace_id)
            .await?
            .into_iter()
            .find(|e| e.artifact_id == memory.memory_id)
        {
            return Err(AppError::BriefEdgePending {
                edge_id: existing.edge_id,
            });
        }
        let edge_id = self.ids.new_id();
        let mut payload = json!({
            "edge_id": edge_id.to_string(),
            "artifact_id": memory.memory_id.to_string(),
            "workspace_id": workspace_id.to_string(),
            "proposer_key_id": key.key_id.to_string(),
        });
        with_reason(self, key.tenant_id, &mut payload, req.reason.as_deref())?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::BRIEF_EDGE_PROPOSED,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let edge = BriefEdgeRecord {
            edge_id,
            tenant_id: key.tenant_id,
            artifact_id: memory.memory_id,
            workspace_id,
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
    /// ceiling, a `used_by: service` key, the idempotency record, the edge
    /// present and its workspace within the key's reviewer effective access
    /// (`access.not_found` outside read access), R2 against the proposer's
    /// key and principal (`review.ratifier_is_proposer`), then the edge's
    /// state (`review.serve_invalid`, `consumed`), then R2 by person: a key
    /// acting for the proposer's person outside that person's own root is
    /// `review.author_in_lineage` with `same_person`, save under
    /// `sole_admin`, where the ratification is admitted and marked
    /// (ADR-0097 build notes, d17).
    pub async fn ratify_brief_edge(
        &self,
        bearer: impl Into<Bearer<'_>>,
        edge_id: Uuid,
        idempotency_key: Option<&str>,
    ) -> Result<BriefEdgeRatification, AppError> {
        let bearer: Bearer<'_> = bearer.into();
        const VERB: &str = "brief_edge.ratify";
        let caller = self.authenticate_bearer(bearer).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reviewer, VERB)?;
        refuse_service(key, VERB)?;
        let request_digest = claim_edge_ratify_request_digest(edge_id)?;
        let scope = idempotency_key.map(|idem_key| IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workspace_id: Uuid::nil(),
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
        self.require_access(&caller, edge.workspace_id, Role::Reviewer, VERB)
            .await?;
        // A pending edge across the boundary is not ratified after a mark
        // (ADR-0030 d7).
        if let Some(claim) = self.journal.memory(key.tenant_id, edge.artifact_id).await? {
            self.refuse_crossing_edge(key.tenant_id, claim.workspace_id, edge.workspace_id)
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
        // R2 by person, as a verdict reads it: outside the person's own
        // root, a key acting for the proposer's person ratifies nothing,
        // save in a `sole_admin` tenant, where it is admitted and marked
        // (ADR-0097 build notes and d17; ADR-0001 d5; ADR-0082 d7).
        let same_user = self
            .same_person(key.tenant_id, edge.proposer_key_id, key)
            .await?;
        let sole_admin = if same_user && !self.in_own_root(key, edge.workspace_id).await? {
            if !self.journal.admin_standing(key.tenant_id).await?.sole_admin {
                return Err(AppError::SamePerson);
            }
            true
        } else {
            false
        };
        let mut payload = json!({
            "subject_kind": "brief_edge",
            "artifact_id": edge.artifact_id.to_string(),
            "version": Value::Null,
            "edge_id": edge_id.to_string(),
            "workspace_id": edge.workspace_id.to_string(),
            "verdict_event_ids": [],
            "ratifier_key_id": key.key_id.to_string(),
            "same_user": same_user,
            "policy_snapshot": review_policy(),
        });
        if sole_admin && let Some(obj) = payload.as_object_mut() {
            obj.insert("sole_admin".to_owned(), json!(true));
        }
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::REVIEW_RATIFY,
            &key.principal,
            Some(key.key_id),
            payload,
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
        bearer: impl Into<Bearer<'_>>,
        edge_id: Uuid,
        idempotency_key: Option<&str>,
    ) -> Result<EdgeRatification, AppError> {
        let bearer: Bearer<'_> = bearer.into();
        let caller = self.authenticate_bearer(bearer).await?;
        let is_claim = self
            .journal
            .claim_edge(caller.key.tenant_id, edge_id)
            .await?
            .is_some();
        if is_claim {
            Ok(EdgeRatification::Claim(
                self.ratify_claim_edge(bearer, edge_id, idempotency_key)
                    .await?,
            ))
        } else {
            Ok(EdgeRatification::Brief(
                self.ratify_brief_edge(bearer, edge_id, idempotency_key)
                    .await?,
            ))
        }
    }

    /// `GET /v2/edges?kind=brief`: one page of `workspace`'s brief edges,
    /// pending and ratified, newest first, each served only when the key
    /// reads its claim, at reader effective access on the workspace
    /// (`access.not_found` outside it) (ADR-0046 d7). The cursor is the
    /// claim-edge listing's, bound to the key and the workspace.
    pub async fn brief_edges(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<BriefEdgePage, AppError> {
        const VERB: &str = "brief_edge.list";
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reader, VERB)?;
        let workspace_id = self.resolve(key, NamedKind::Workspace, &workspace).await?;
        self.require_access(&caller, workspace_id, Role::Reader, VERB)
            .await?;
        let filter = ClaimEdgeFilter {
            artifact_id: None,
            workspace_id: Some(workspace_id),
        };
        let now = self.clock.now();
        let after = cursor
            .map(|cursor| ClaimEdgeCursor::resume(cursor, key.key_id, &filter, &now))
            .transpose()?;
        let position = |e: &BriefEdgeRecord| (e.created_at.as_str().to_owned(), e.edge_id);
        let scope_root = self.in_progress_root(key.tenant_id, workspace_id).await?;
        let mut served = Vec::new();
        for edge in self
            .journal
            .brief_edges(key.tenant_id, workspace_id)
            .await?
        {
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
                    self.in_progress_root(key.tenant_id, memory.workspace_id)
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
        // A pending edge serves when it was proposed and, to a key of the
        // proposer's person, which key proposed it (ADR-0095 d6 as amended
        // 2026-11-24).
        let mut items = Vec::with_capacity(served.len());
        for edge in &served {
            let pending = edge.state == EdgeState::Pending;
            items.push(BriefEdgeListed {
                edge: BriefEdgeView::from(edge),
                proposed_at: pending.then(|| edge.created_at.clone()),
                proposed_by: if pending {
                    self.proposed_by(key, edge.proposer_key_id).await?
                } else {
                    None
                },
            });
        }
        Ok(BriefEdgePage { items, next_cursor })
    }

    /// Ratify the pending claim edge `edge_id`: journalled `review.ratify`
    /// with `subject_kind: "claim_edge"`, the edge's one review act at k = 1,
    /// and stored ratified naming the ratifier (event-catalog.md; ADR-0008
    /// d5). The refusal order: the verb ceiling, a `used_by: service` key
    /// before any lookup, the idempotency record, the edge and both its
    /// endpoints present and in read scope, reviewer on both endpoints'
    /// workspaces, the proposal's label check, R2 — the proposer's key or
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
    // ceiling, and edge authority's reviewer on both ends, which also reads
    // ADR-0021 d6's both-vocabularies rule for a term mapping.
    pub async fn ratify_claim_edge(
        &self,
        bearer: impl Into<Bearer<'_>>,
        edge_id: Uuid,
        idempotency_key: Option<&str>,
    ) -> Result<ClaimEdgeRatification, AppError> {
        let bearer: Bearer<'_> = bearer.into();
        const VERB: &str = "claim_edge.ratify";
        let caller = self.authenticate_bearer(bearer).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reviewer, VERB)?;
        refuse_service(key, VERB)?;

        // The route names no workspace, so the scope's workspace is nil
        // (http-api.md idempotency), as for the proposal.
        let request_digest = claim_edge_ratify_request_digest(edge_id)?;
        let scope = idempotency_key.map(|idem_key| IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workspace_id: Uuid::nil(),
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
            let ancestry = self.ancestry(key.tenant_id, memory.workspace_id).await?;
            if !(caller.effective(&ancestry).contains(&Role::Reader)
                && self.readable(&caller, &memory).await?)
            {
                return Err(AppError::NotFound);
            }
            endpoints.push(memory);
        }
        for memory in &endpoints {
            self.require_access(&caller, memory.workspace_id, Role::Reviewer, VERB)
                .await?;
        }
        let (src, dst) = (&endpoints[0], &endpoints[1]);
        // A pending edge across the boundary is not ratified after a mark
        // (ADR-0030 d7).
        self.refuse_crossing_edge(key.tenant_id, src.workspace_id, dst.workspace_id)
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
            RatifyRefusal::NotReviewer(role) => AppError::VerbCeiling { role, verb: VERB },
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
                "required_roles": ["reviewer"],
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
    /// ratified, newest first, narrowed to `query`'s workspace, by id or name
    /// (ADR-0026), whose subtree holds either endpoint, and its version at
    /// either endpoint. A reader key is needed: an admin key answers
    /// `access.verb_ceiling`, since an edge joins content and an admin
    /// structure read never includes it (ADR-0026 d15). A named workspace
    /// asks reader effective there by the refusal order. An edge is served
    /// only when both its endpoints are in the key's read scope, a row in a
    /// node where reader is effective that is promoted or the key's own
    /// (`readable`), so the read never names an unreadable version's id.
    /// The store selects by that scope, so one keyset page of at most
    /// `limit + 1` edges answers the call. The cursor is bound to the key,
    /// to `api.cursor_ttl` and to the filter, its workspace resolved, so
    /// either spelling of the workspace resumes it.
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
        let workspace_id = self
            .resolve_opt(key, NamedKind::Workspace, query.workspace.as_ref())
            .await?;
        if let Some(node) = workspace_id {
            self.require_access(&caller, node, Role::Reader, VERB)
                .await?;
        }
        let filter = ClaimEdgeFilter {
            artifact_id: query.artifact_id,
            workspace_id,
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
        // array that grows with the tenant's workspaces, unmeasured (#99,
        // #150).
        let readable_nodes = self.nodes_holding(&caller, Role::Reader).await?;
        // A scoped read: at `workspace`, else the key's default workspace, an
        // edge with an endpoint inside an in-progress subtree the scope is
        // not inside is not served (ADR-0030 d2).
        // With neither, the scope has no root, so every in-progress subtree
        // is left out (fail closed). Inside the scope's root, an endpoint
        // held by any author is served, as recall serves it (d3).
        let scope_root = match workspace_id.or(key.default_workspace) {
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
        let within = match workspace_id {
            None => None,
            Some(node) => {
                let subtree: BTreeSet<Uuid> = self
                    .journal
                    .workspace_subtree(tenant_id, node)
                    .await?
                    .into_iter()
                    .map(|w| w.workspace_id)
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
        let key = self
            .admin_caller(Bearer::Key(api_key), "verify_chain")
            .await?
            .key;
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
    /// node the query's `workspace` names, else the key's default, else
    /// `request.workspace_unresolved` — then one bounded scan of at most
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
        workspace: Option<Reference>,
        after_seq: u64,
        limit: usize,
    ) -> Result<TailPage, AppError> {
        let caller = self
            .authenticate_bearer_admitting(bearer.into(), true)
            .await?;
        let key = &caller.key;
        // A maintenance key reads the head alone, never an event (ADR-0097
        // d18).
        if key.role == Role::Maintenance {
            let scan = self
                .journal
                .events_with_subjects(key.tenant_id, 1, 0)
                .await?;
            return Ok(TailPage {
                events: Vec::new(),
                head_seq: scan.head_seq,
                next_after_seq: after_seq.min(scan.head_seq),
            });
        }
        require_admissible(key, Role::Reader, "events_tail")?;
        let scope = self.target_workspace(key, workspace.as_ref()).await?;
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
            // A public node in the scope's subtree is read (ADR-0098 d1).
            grants: &caller.reach,
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
            // Judged in the current names; served as stored, so the served
            // fields still verify against the stored signature (ADR-0091 d4).
            let read = read_as_current(&event.envelope)?;
            let envelope = &read;
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
                workspaces: std::slice::from_ref(ancestry),
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
            let payload = served_payload(&event.envelope, judged.claim, names_actor, names_node);
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
    /// A key past its expiry is refused as `expired` (ADR-0079 d2).
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
    /// minted only for a key confined to one workspace (`oauth.connect_refused`,
    /// `not_confined`; ADR-0067 d3).
    pub async fn mint_connect_code_with_ttl(
        &self,
        api_key: impl Into<Bearer<'_>>,
        key: Reference,
        ttl_secs: Option<i64>,
    ) -> Result<ConnectCodeIssued, AppError> {
        // A caller the request already resolved is not charged again
        // against its rate (#253, #281).
        let admin = self
            .key_act_caller(api_key.into(), "mint_connect_code")
            .await?;
        require_role(&admin, Role::Admin, "mint_connect_code")?;
        let admin = admin.key;
        self.mint_connect_code_by_with_ttl(&admin, key, ttl_secs, Head::AtSeal)
            .await
    }

    /// [`App::mint_connect_code`] once the issuer is known: an admin key, or
    /// a person's `<user>-web` session key for a key naming its user, which
    /// the web use case has checked (ADR-0045 d9, d16).
    pub(crate) async fn mint_connect_code_by(
        &self,
        admin: &KeyRecord,
        key: Reference,
        head: Head<'_>,
    ) -> Result<ConnectCodeIssued, AppError> {
        self.mint_connect_code_by_with_ttl(admin, key, None, head)
            .await
    }

    async fn mint_connect_code_by_with_ttl(
        &self,
        admin: &KeyRecord,
        key: Reference,
        ttl_secs: Option<i64>,
        head: Head<'_>,
    ) -> Result<ConnectCodeIssued, AppError> {
        self.mint_connect_code_by_with_reason(admin, key, ttl_secs, head, None)
            .await
    }

    /// [`App::mint_connect_code_with_ttl`] with the admin's reason, which a
    /// mint that waits on a second admin person requires (ADR-0097 d26).
    ///
    /// # Errors
    /// As [`App::mint_connect_code_with_ttl`], and a pending mint sent no
    /// reason.
    pub async fn mint_connect_code_with_reason(
        &self,
        api_key: impl Into<Bearer<'_>>,
        key: Reference,
        ttl_secs: Option<i64>,
        reason: Option<&str>,
    ) -> Result<ConnectCodeIssued, AppError> {
        let admin = self
            .key_act_caller(api_key.into(), "mint_connect_code")
            .await?;
        require_role(&admin, Role::Admin, "mint_connect_code")?;
        self.mint_connect_code_by_with_reason(&admin.key, key, ttl_secs, Head::AtSeal, reason)
            .await
    }

    async fn mint_connect_code_by_with_reason(
        &self,
        admin: &KeyRecord,
        key: Reference,
        ttl_secs: Option<i64>,
        head: Head<'_>,
        reason: Option<&str>,
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
        if target.expired_at(&self.clock.now()) {
            return Err(connect_refused(ConnectRefused::Expired));
        }
        // A cut-off key is refused as an expired one is: its connection
        // would read and write nothing (#353).
        if self.cut_off(&target).await? {
            return Err(connect_refused(ConnectRefused::CutOff));
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
        // A pending key takes no connect code (ADR-0097 d23).
        if target.pending_act.is_some() {
            return Err(AppError::AdminAct(AdminActRefusal::KeyPending));
        }
        let (code, shown) = self.new_connect_code(oauth, key_id, ttl_secs);
        let prev = self.head_at(tenant_id, head).await?;
        // An admin's mint is widening (ADR-0097 d5, d6): pending, the code
        // is shown once and voids, opens and lives nothing until a second
        // admin person confirms it, its life running from then.
        let act = if admin.role == Role::Admin {
            let act = self
                .begin_admin_act(
                    &admin,
                    oml_domain::admin_act::AdminActKind::MintConnectCode,
                    json!({
                        "key_id": key_id.to_string(),
                        "name": target.name.as_ref().map(Name::as_str),
                        "role": target.role.as_str(),
                        "principal": target.principal,
                        // The root's person the confirm screen names, and
                        // whom a mint at once under `sole_admin` notifies
                        // (ADR-0097 d6).
                        "root_person": self.person_root_name(&target).await?.as_ref().map(Name::as_str),
                    }),
                    reason,
                    json!({
                        "key_id": key_id.to_string(),
                        "code": {
                            "code_id": code.code_id.to_string(),
                            "code_hash": code.code_hash.to_prefixed(),
                            "ttl_secs": ttl_secs,
                        },
                    }),
                    prev.as_ref(),
                )
                .await?;
            if act.pending() {
                self.commit_with_act(Some(act), Vec::new()).await?;
                return Ok(shown);
            }
            Some(act)
        } else {
            None
        };
        let event = self.seal(
            act.as_ref().map(|a| &a.event).or(prev.as_ref()),
            tenant_id,
            event_types::CONNECTION_CODE_ISSUED,
            &admin.principal,
            Some(admin.key_id),
            admin_acts::named(
                code_issued_payload(&code, admin.key_id, voids),
                act.as_ref(),
            ),
        )?;
        self.commit_with_act(
            act,
            vec![(event, MetadataFold::ConnectCodeIssued { code, voids })],
        )
        .await?;
        Ok(shown)
    }

    /// Whether `target` is confined to one workspace: no user root, and every
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
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
    ) -> Result<Option<LiveConnectCode>, AppError> {
        let admin = self
            .admin_caller(bearer.into(), "live_connect_code")
            .await?
            .key;
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
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
    ) -> Result<Vec<ConnectionRecord>, AppError> {
        let admin = self
            .admin_caller(bearer.into(), "list_connections")
            .await?
            .key;
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
        let admin = self
            .admin_caller(Bearer::Key(api_key), "revoke_connection")
            .await?
            .key;
        let tenant_id = admin.tenant_id;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        let connection = self
            .journal
            .connection(tenant_id, connection_id)
            .await?
            .filter(|c| c.key_id == key_id && c.state == ConnectionState::Open)
            .ok_or(AppError::NotFound)?;
        // Revoking a connection is a narrowing admin act (ADR-0097 d3).
        let receipt = self
            .commit_as_act(
                &admin,
                oml_domain::admin_act::AdminActKind::RevokeConnection,
                json!({
                    "connection_id": connection_id.to_string(),
                    "key_id": key_id.to_string(),
                }),
                reason,
                event_types::CONNECTION_REVOKED,
                json!({
                    "connection_id": connection_id.to_string(),
                    "key_id": key_id.to_string(),
                    "revoked_by_key_id": admin.key_id.to_string(),
                    "cause": RevokedCause::Admin.as_str(),
                    "reason_digest": reason.map(|r| sha256_digest_string(r.as_bytes())),
                }),
                MetadataFold::ConnectionRevoked {
                    connection_id,
                    cause: RevokedCause::Admin,
                },
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
    /// entry spent first, or an expired key's (ADR-0031 d8; ADR-0079 d2).
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
        let key = self.journal.key(found.tenant_id, found.row.key_id).await?;
        let row = CodeRow {
            redeemed: found.row.redeemed_at.is_some(),
            voided: found.row.voided_seq.is_some(),
            expired: now.unix_micros() >= found.row.expires_at.unix_micros(),
            // A key its tenant no longer holds reads as revoked.
            key: key
                .as_ref()
                .map_or(KeyState::Revoked, |key| key.state_at(now)),
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
    /// name, principal and role, each granted workspace with the grant's
    /// role, and for each the roles effective there, which name the
    /// verb-kind sentences' workspaces.
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
                .workspace_ancestry(tenant_id, grant.node)
                .await?
                .filter(|ancestry| !ancestry.is_empty())
                .ok_or_else(|| inconsistent(format!("granted node {} is not held", grant.node)))?;
            let workspace = ancestry[0].name.clone();
            let ids: Vec<Uuid> = ancestry.iter().map(|w| w.workspace_id).collect();
            let effective = effective_roles(&grants, &denials, &ids);
            for (role, names) in [
                (Role::Submitter, &mut view.writes_in),
                (Role::Reader, &mut view.reads_in),
                (Role::Reviewer, &mut view.proposes_in),
            ] {
                if effective.contains(&role) && !names.contains(&workspace) {
                    names.push(workspace.clone());
                }
            }
            view.grants.push(ConfirmationGrant {
                workspace_id: grant.node,
                workspace,
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
    /// ADR-0031 d10, d11). A key past its expiry is
    /// [`AppError::KeyExpired`] (ADR-0079 d2); anything else is
    /// [`AppError::Unauthorized`].
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
        let key = key.ok_or(AppError::Unauthorized)?;
        // The token is the key's own credential, so its holder is told the
        // key expired (ADR-0079 d2).
        self.require_unexpired(&key)?;
        self.note_authenticated(&key).await;
        self.caller_of(key).await
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
            key: key
                .as_ref()
                .map_or(KeyState::Revoked, |key| key.state_at(now)),
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
        let ceiling = self.ceiling_for(key.tenant_id, key.user).await?;
        self.commit_charged(
            event,
            MetadataFold::ConnectionOpened {
                connection_id: connection.connection_id,
                replaces,
                auth_code_hash,
                tokens,
            },
            None,
            ceiling,
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
    /// Whether the subject is a claim version, not a workspace node.
    claim: bool,
    /// The subject's workspace: the payload's `workspace_id`, else the
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
/// `workspace.relate_*` name two workspaces, are never journalled by this
/// build, and are not placed: they stay hidden until they are.
fn judged_subject(envelope: &Envelope, subject: Option<&SubjectRow>) -> Option<JudgedSubject> {
    let event_type = envelope.event_type.as_str();
    let payload = &envelope.payload;
    if event_type == "lifecycle.reassign" || event_type.starts_with("workspace.relate_") {
        return None;
    }
    let named = payload
        .get("workspace_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok());
    if event_type.starts_with("workspace.") {
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
        node: named.or(subject.map(|s| s.workspace_id))?,
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
/// `*_by_key_id` — is null, and a claim subject's `workspace_id` on a node
/// where the key holds no role is null (roles-and-keys.md#own, naming a
/// node).
fn served_payload(envelope: &Envelope, claim: bool, names_actor: bool, names_node: bool) -> Value {
    let mut payload = envelope.payload.clone();
    if let Some(fields) = payload.as_object_mut() {
        for (field, value) in fields.iter_mut() {
            let names_key = field == "proposer_key_id"
                || field == "ratifier_key_id"
                || field.ends_with("_by_key_id");
            let withheld_node =
                claim && oml_domain::former_names::read_key(field) == "workspace_id" && !names_node;
            if (names_key && !names_actor) || withheld_node {
                *value = Value::Null;
            }
        }
    }
    payload
}

#[cfg(test)]
mod offered_instruction_tests {
    use super::*;

    fn skill(status: MemoryStatus, is_tip: bool) -> MemoryRecord {
        MemoryRecord {
            memory_id: Uuid::from_u128(1),
            tenant_id: Uuid::from_u128(2),
            workspace_id: Uuid::from_u128(3),
            claim_key: "skill/deploy".to_owned(),
            version: 1,
            kind: MemoryKind::Skill,
            body: "deploy".to_owned(),
            body_digest: String::new(),
            body_bytes: 6,
            status,
            is_tip,
            promoted_at: None,
            retirement: false,
            replaces: None,
            author_principal: "acme/agent".to_owned(),
            author_key_id: Uuid::from_u128(4),
            author_person: None,
            derives_from: Vec::new(),
            lineage_principals: Vec::new(),
            lineage_key_ids: Vec::new(),
            scan_finding: None,
            scanned_under: Some("rules-1".to_owned()),
            created_at: EventTime::parse("2026-11-19T10:00:00.000000Z").expect("time"),
            guardrail: None,
            kind_fields: None,
            valid: Box::default(),
            anchor: None,
            origin_kind: None,
            source_check: None,
            approval: None,
            format: Box::default(),
        }
    }

    /// A held skill offered at the instructions read and rejected, withdrawn
    /// or replaced before the pipeline hydrates it is no hit; a promoted tip
    /// and a scanned held version still are.
    #[test]
    fn an_offered_instruction_decided_before_the_pipeline_reads_it_is_no_hit() {
        let scanner = Some("rules-1");
        for (status, is_tip, served) in [
            (MemoryStatus::Promoted, true, true),
            (MemoryStatus::Held, false, true),
            (MemoryStatus::Rejected, false, false),
            (MemoryStatus::Withdrawn, false, false),
            (MemoryStatus::Demoted, false, false),
            (MemoryStatus::Promoted, false, false),
        ] {
            assert_eq!(
                offered_instruction_still_served(&skill(status, is_tip), scanner),
                served,
                "{status:?} tip {is_tip}"
            );
        }
        let held_unscanned = MemoryRecord {
            scanned_under: None,
            ..skill(MemoryStatus::Held, false)
        };
        assert!(!offered_instruction_still_served(&held_unscanned, scanner));
    }
}
