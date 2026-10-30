//! Application-layer records and use-case request/response shapes.

use oml_domain::access::{Grant, KeyDenial};
use oml_domain::{
    ChainError, EventTime, HeldBy, JcsError, LineageClosure, MemoryStatus, Role, TransitionError,
    VerdictError, VerifyError,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ports::StoreError;

/// A stored API key row: everything about a key is looked up by the value's
/// hash; the value itself is never stored (roles-and-keys.md key format).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRecord {
    pub key_id: Uuid,
    pub tenant_id: Uuid,
    pub role: Role,
    pub principal: String,
    pub held_by: HeldBy,
    pub default_workarea: Option<Uuid>,
    /// The key that issued this one; `None` for the bootstrap ceremony's
    /// first admin key, which no key issues (postgres-schema.md
    /// `api_keys.issued_by`).
    pub issued_by: Option<Uuid>,
    /// `sha256:<hex>` of the full key string, prefix included.
    pub key_hash: String,
}

/// A stored grant row: the domain grant and its epoch, the journal seq of
/// the `grant.issued` that wrote it (postgres-schema.md `grants`;
/// workareas.md permission epoch; ADR-0025 d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRecord {
    pub grant: Grant,
    pub epoch: u64,
}

/// A stored global per-key denial: the domain denial, the key that
/// journalled it, and its epoch, the journal seq of its `key.denied`
/// (postgres-schema.md `key_denials`; ADR-0025 d2, d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenialRecord {
    pub denial: KeyDenial,
    pub created_by: Uuid,
    pub epoch: u64,
}

/// What effective access is computed from for one key, read in one
/// snapshot: the floor of its permission epoch, its grants and its global
/// denials, each list in epoch order (workareas.md effective access and
/// permission epoch; ADR-0025 d1, d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAccess {
    /// The journal seq of the key's `key.issued`.
    pub issued_seq: u64,
    pub grants: Vec<GrantRecord>,
    pub denials: Vec<DenialRecord>,
}

/// A workarea's crew: recorded, not operated — OML never contacts the
/// external manager (openapi.yaml `CrewRecord`; event-catalog.md
/// `workarea.create`). An absent member is stored and served absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crew {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charter_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_manager: Option<String>,
}

/// A workarea node (the PoC slice: id, parent, name and crew — no status
/// ladder yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkareaRecord {
    pub workarea_id: Uuid,
    pub tenant_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    /// `None` when the create named no crew.
    pub crew: Option<Crew>,
}

/// The metadata row of one memory version. The body lives here, never in the
/// journal — journal payloads are digest-only (ADR-0002 d1, admission.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecord {
    pub memory_id: Uuid,
    pub tenant_id: Uuid,
    pub workarea_id: Uuid,
    pub claim_key: Option<String>,
    pub body: String,
    pub body_digest: String,
    pub status: MemoryStatus,
    pub author_principal: String,
    pub author_key_id: Uuid,
    pub derives_from: Vec<Uuid>,
    /// The row's lineage closure, flattened: the author plus every declared
    /// ancestor's closure (roles-and-keys.md lineage-closure exclusion).
    pub lineage_principals: Vec<String>,
    pub lineage_key_ids: Vec<Uuid>,
    pub superseded_by: Option<Uuid>,
    pub scan_finding: Option<String>,
    pub created_at: EventTime,
}

impl MemoryRecord {
    /// The domain closure used by `check_reviewer` (ADR-0001 R2).
    #[must_use]
    pub fn lineage_closure(&self) -> LineageClosure {
        let mut closure = LineageClosure::new();
        for principal in &self.lineage_principals {
            closure.add_principal(principal.clone());
        }
        for key_id in &self.lineage_key_ids {
            closure.add_key_id(*key_id);
        }
        closure
    }
}

// ---------------------------------------------------------------------------
// Use-case requests and responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BootstrapRequest {
    pub tenant_name: String,
    pub admin_principal: String,
}

/// Show-once: the value is returned exactly here and never stored
/// (roles-and-keys.md key lifecycle).
#[derive(Debug, Clone)]
pub struct IssuedKey {
    pub key_id: Uuid,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct BootstrapResult {
    pub tenant_id: Uuid,
    pub root_workarea_id: Uuid,
    pub admin_key: IssuedKey,
}

#[derive(Debug, Clone)]
pub struct IssueKeyRequest {
    pub role: Role,
    pub principal: String,
    pub held_by: HeldBy,
    /// `None` inherits the issuer's default workarea.
    pub default_workarea: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct SubmitRequest {
    pub idempotency_key: String,
    pub claim_key: Option<String>,
    pub body: String,
    /// `None` resolves to the key's default workarea
    /// (http-api.md, ADR-0022 d4).
    pub workarea_id: Option<Uuid>,
    pub derives_from: Vec<Uuid>,
}

/// The stored idempotent response: a replayed submit returns this byte for
/// byte (http-api.md idempotency record).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitReceipt {
    pub memory_id: Uuid,
    pub claim_key: Option<String>,
    pub workarea_id: Uuid,
    pub state: String,
    pub journal_seq: u64,
    pub body_digest: String,
}

#[derive(Debug, Clone)]
pub struct SubmitOutcome {
    pub receipt: SubmitReceipt,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Reject,
}

#[derive(Debug, Clone)]
pub struct VerdictOutcome {
    pub memory_id: Uuid,
    pub state: MemoryStatus,
    pub journal_seq: u64,
}

#[derive(Debug, Clone)]
pub struct RecallRequest {
    pub query: String,
    pub k: usize,
    /// `None` resolves to the key's default workarea; a key with no default
    /// searches tenant-wide (PoC simplification of the subtree scope).
    pub workarea_id: Option<Uuid>,
}

/// Steering rows precede hits and are never ranked (retrieval.md recall-
/// ordering contract). The PoC indexes no steering kind, so the segment is
/// always empty here; the shape is the contract.
#[derive(Debug, Clone)]
pub struct RecallResult {
    pub steering: Vec<MemoryRecord>,
    pub hits: Vec<MemoryRecord>,
}

#[derive(Debug, Clone)]
pub struct SupersedeRequest {
    pub target_id: Uuid,
    pub body: String,
    pub claim_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SupersedeOutcome {
    pub superseded_id: Uuid,
    pub successor_id: Uuid,
    pub journal_seq: u64,
}

/// A typed application refusal; every variant names the rule it enforces.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unknown api key (reads fail closed — http-api.md authentication)")]
    Unauthorized,
    #[error(
        "{verb} is outside the {role:?} verb ceiling (roles-and-keys.md permission matrix, R1)"
    )]
    VerbCeiling { role: Role, verb: &'static str },
    #[error("a {issuer:?} key cannot issue a {requested:?} key (roles-and-keys.md issuer ceiling)")]
    IssuerCeiling { issuer: Role, requested: Role },
    #[error(
        "no workarea: the key has no default and the request names none (http-api.md request.workarea_unresolved)"
    )]
    WorkareaUnresolved,
    #[error(
        "not found — absence and no-access are indistinguishable (http-api.md access.not_found)"
    )]
    NotFound,
    #[error("a writer may supersede only its own claims (http-api.md access.author_only)")]
    AuthorOnly,
    #[error(
        "the supersede target is not the current promoted tip (http-api.md submission.tip_conflict)"
    )]
    TipConflict,
    #[error("deterministic scan veto: {0} (admission.md — the rejection is journalled)")]
    ScanVeto(String),
    #[error("derives_from names unknown artifact {0} (fail closed)")]
    LineageUnknown(Uuid),
    #[error(
        "idempotency key reused with a different request (http-api.md idempotency.digest_mismatch)"
    )]
    IdempotencyDigestMismatch,
    #[error("stored idempotent response is unreadable: {0}")]
    StoredResponse(String),
    #[error(transparent)]
    Verdict(#[from] VerdictError),
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error(transparent)]
    Chain(#[from] ChainError),
    #[error(transparent)]
    ChainVerify(#[from] VerifyError),
    #[error(transparent)]
    Jcs(#[from] JcsError),
    #[error(transparent)]
    Store(#[from] StoreError),
}
