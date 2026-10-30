//! The driven ports (ADR-0019 d2; workspace-plan.md port table).
//!
//! Async methods use RPITIT — `-> impl Future<Output = …> + Send` in the
//! trait, plain `async fn` in implementations — so the traits stay
//! dependency-free and futures are `Send` for the transports. The cost:
//! the traits are not dyn-compatible; compositions use generics.

use core::future::Future;

use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde_json::Value;
use uuid::Uuid;

use oml_domain::{ChainHead, EventTime, Sha256Hash, SignedEnvelope, key_fingerprint};

use crate::types::{KeyRecord, MemoryRecord, WorkareaRecord};

/// A storage refusal. `SeqConflict` is how the serialized per-tenant chain
/// surfaces to a concurrent writer (ADR-0002: one chain head, one assigner).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("chain head moved: expected seq {expected_seq}, commit carries seq {found_seq}")]
    SeqConflict { expected_seq: u64, found_seq: u64 },
    #[error("prev_hash does not match the stored head at seq {seq}")]
    PrevHashConflict { seq: u64 },
    #[error("storage backend failure: {0}")]
    Backend(String),
}

/// The metadata fold committed atomically with a journal event: what the
/// event does to the read-side rows, applied in the same transaction
/// (ADR-0002's one-transaction commit point).
#[derive(Debug, Clone)]
pub enum MetadataFold {
    TenantGenesis,
    WorkareaCreated(WorkareaRecord),
    KeyIssued(KeyRecord),
    /// A captured submission: the row lands `held` (or `rejected` on a scan
    /// veto, with `enqueue: false` — nothing waits in the queue).
    MemoryHeld {
        memory: MemoryRecord,
        enqueue: bool,
    },
    /// A verdict that does not yet satisfy the policy's `k`: journalled,
    /// status unchanged.
    VerdictRecorded {
        memory_id: Uuid,
        reviewer_key_id: Uuid,
    },
    MemoryPromoted {
        memory_id: Uuid,
    },
    MemoryRejected {
        memory_id: Uuid,
    },
    /// The old tip stays promoted and stops being the tip (its
    /// `superseded_by` names the successor, ADR-0025 d10); the successor
    /// enters held and is queued for review like any write.
    MemorySuperseded {
        superseded: Uuid,
        successor: MemoryRecord,
    },
}

/// The outbox row written in the same transaction as the event; the lake
/// writer drains it asynchronously (storage.md). Digest-only payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    pub event_id: Uuid,
    pub topic: String,
    pub payload: Value,
}

/// The idempotency record scope: (key, resolved workarea, Idempotency-Key)
/// per http-api.md, plus the tenant that owns the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyScope {
    pub tenant_id: Uuid,
    pub key_id: Uuid,
    pub workarea_id: Uuid,
    pub idem_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyRecord {
    /// Digest of the request; a replay with a different digest is refused.
    pub request_digest: String,
    /// The stored response, returned byte for byte on replay.
    pub response: Value,
}

#[derive(Debug, Clone)]
pub struct IdempotencyPut {
    pub scope: IdempotencyScope,
    pub record: IdempotencyRecord,
}

/// One commit, one transaction: the signed event, the metadata fold, the
/// outbox row, and (when the use-case is idempotent) the idempotency record
/// — all or nothing (ADR-0002).
#[derive(Debug, Clone)]
pub struct CommitRequest {
    pub event: SignedEnvelope,
    pub fold: MetadataFold,
    pub outbox: OutboxRow,
    pub idempotency: Option<IdempotencyPut>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitReceipt {
    pub seq: u64,
    pub hash: Sha256Hash,
}

/// The journal-and-metadata store port (workspace-plan.md row 1).
pub trait JournalStore: Send + Sync {
    /// The one-transaction commit point. The store refuses a commit whose
    /// `seq`/`prev_hash` do not extend the current head — chain assignment
    /// is serialized per tenant, and the conflict is typed, never silent.
    fn commit(
        &self,
        req: CommitRequest,
    ) -> impl Future<Output = Result<CommitReceipt, StoreError>> + Send;

    fn chain_head(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<ChainHead>, StoreError>> + Send;

    /// Events with `seq >= from_seq`, ascending, at most `limit`.
    fn events_range(
        &self,
        tenant_id: Uuid,
        from_seq: u64,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<SignedEnvelope>, StoreError>> + Send;

    fn memory(
        &self,
        tenant_id: Uuid,
        memory_id: Uuid,
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send;

    /// Promoted-only full-text recall (retrieval.md: reads return promoted
    /// rows only). `workarea_id: None` searches tenant-wide (PoC).
    fn recall_fts(
        &self,
        tenant_id: Uuid,
        workarea_id: Option<Uuid>,
        query: &str,
        k: usize,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send;

    /// Claim the next reviewable queue entry for this reviewer: entries
    /// claimed by another key are skipped, and so is any row whose lineage
    /// closure contains the reviewer — the store never serves what this key
    /// may not judge (admission.md review; the R2 check proper still runs
    /// at verdict time in the use-case).
    fn review_claim_next(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send;

    /// One equality lookup on the key-value hash (roles-and-keys.md).
    fn key_by_hash(
        &self,
        key_hash: &str,
    ) -> impl Future<Output = Result<Option<KeyRecord>, StoreError>> + Send;

    fn idempotency_get(
        &self,
        scope: &IdempotencyScope,
    ) -> impl Future<Output = Result<Option<IdempotencyRecord>, StoreError>> + Send;
}

/// The signer port — the lead/follower split as a trait: only the lead
/// composition holds one (workspace-plan.md ninth trait; ADR-0006 d5).
pub trait Signer: Send + Sync {
    fn sign(&self, signed_bytes: &[u8]) -> Signature;
    fn verifying_key(&self) -> VerifyingKey;
    fn fingerprint(&self) -> String {
        key_fingerprint(&self.verifying_key())
    }
}

/// Time enters the core as an input (workspace-plan.md clock/id port).
pub trait Clock: Send + Sync {
    fn now(&self) -> EventTime;
}

/// Identity enters the core as an input. `new_secret` is the 256-bit key
/// material source for API-key minting (roles-and-keys.md key format).
pub trait IdGen: Send + Sync {
    fn new_id(&self) -> Uuid;
    fn new_secret(&self) -> [u8; 32];
}

/// What the deterministic scan sees: the normalized claim text, never a
/// model prompt (admission.md, "the scan is rules, not a model").
#[derive(Debug, Clone, Copy)]
pub struct AdmissionEnvelope<'a> {
    pub tenant_id: Uuid,
    pub kind: &'a str,
    pub body: &'a str,
}

/// The scan verdict. No promoting arm exists to return — the plugin law as
/// a type (ADR-0019; workspace-plan.md scanner port).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    Pass,
    Hold(String),
    Veto(String),
}

/// The scanner port: deterministic, synchronous, in-process (admission.md).
pub trait Scanner: Send + Sync {
    fn scan(&self, envelope: &AdmissionEnvelope<'_>) -> ScanOutcome;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyringError {
    #[error("keyring io failure: {0}")]
    Io(String),
    #[error("keyring file permissions are not 0600 (ADR-0023 d4 kms-file shape)")]
    Permissions(String),
    #[error("keyring material is corrupt: {0}")]
    Corrupt(String),
}

/// The keyring port: load the deployment signing key, creating it on first
/// start. The kms-file adapter keeps it in a 0600 file and refuses anything
/// looser (ADR-0023 d4).
pub trait KeyringStore: Send + Sync {
    fn load_or_create(&self) -> impl Future<Output = Result<SigningKey, KeyringError>> + Send;
}
