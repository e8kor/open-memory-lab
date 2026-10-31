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

use oml_domain::access::{Grant, KeyDenial};
use oml_domain::{ChainHead, EventTime, MemoryKind, Sha256Hash, SignedEnvelope, key_fingerprint};

use crate::types::{
    ClaimKeyState, KeyAccess, KeyRecord, ListingFilter, ListingPosition, MemoryRecord,
    RecallCandidate, WorkareaRecord,
};

/// A storage refusal. `SeqConflict` is how the serialized per-tenant chain
/// surfaces to a concurrent writer (ADR-0002: one chain head, one assigner).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("chain head moved: expected seq {expected_seq}, commit carries seq {found_seq}")]
    SeqConflict { expected_seq: u64, found_seq: u64 },
    #[error("prev_hash does not match the stored head at seq {seq}")]
    PrevHashConflict { seq: u64 },
    /// A ratification's compare-and-set found `replaced` no longer the
    /// promoted tip under its successor's workarea and `claim_key`: the
    /// commit wrote nothing (ADR-0025 d10).
    #[error("version {replaced} is no longer the served tip its successor replaces")]
    TipMoved { replaced: Uuid },
    /// A commit's idempotency record found its scope already recorded: a
    /// concurrent attempt under the same Idempotency-Key committed first,
    /// and this commit wrote nothing (http-api.md idempotency).
    #[error("an idempotency record already holds this scope")]
    IdempotencyRecorded,
    #[error("storage backend failure: {0}")]
    Backend(String),
}

/// The metadata fold committed atomically with a journal event: what the
/// event does to the read-side rows, applied in the same transaction
/// (ADR-0002's one-transaction commit point).
///
/// A key row's `issued_seq`, a grant or denial row's epoch and a memory's
/// `promoted_at` are never carried here: the store writes the seq or the
/// `at` of the event this fold commits with, so the row cannot disagree with
/// its event (workareas.md permission epoch; ADR-0025 d10, d14).
#[derive(Debug, Clone)]
pub enum MetadataFold {
    TenantGenesis,
    WorkareaCreated(WorkareaRecord),
    /// `key.issued`: the key row, its `issued_seq` this commit's seq.
    KeyIssued(KeyRecord),
    /// `grant.issued`: one grant row on `key_id`, its epoch this commit's
    /// seq. The store refuses a grant on a key or node it does not hold, and
    /// a grant role other than reader, writer or dreamer (postgres-schema.md
    /// `grants`).
    GrantIssued {
        key_id: Uuid,
        grant: Grant,
    },
    /// `key.denied`: one global per-key denial row on `key_id`, journalled
    /// by `created_by`, its epoch this commit's seq. The store refuses a
    /// denial on a key or node it does not hold (postgres-schema.md
    /// `key_denials`).
    KeyDenied {
        key_id: Uuid,
        denial: KeyDenial,
        created_by: Uuid,
    },
    /// `key.revoked`: the key resolves by its hash no more; its row, grants
    /// and denials stay as history. The store refuses a revocation of a key
    /// it does not hold (roles-and-keys.md key lifecycle; ADR-0025 d2).
    KeyRevoked {
        key_id: Uuid,
    },
    /// A captured submission: the row lands `held` and, with `enqueue`,
    /// waits in the review queue. The store refuses a second held, promoted
    /// or demoted version of one ordinal under one `claim_key` in one
    /// workarea (postgres-schema.md `memories_claim_version`).
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
    /// Ratification of a claim's first version: it is promoted and becomes
    /// the tip, and `promoted_at` is the event's `at`. The store refuses a
    /// second promoted tip of one `claim_key` in one workarea
    /// (postgres-schema.md `memories_served_tip`; ADR-0025 d10).
    MemoryPromoted {
        memory_id: Uuid,
    },
    MemoryRejected {
        memory_id: Uuid,
    },
    /// `lifecycle.supersede`: the ratified successor takes the tip from the
    /// version it replaces, which stays promoted. The store compare-and-sets
    /// that `replaced` is still the promoted tip under the successor's own
    /// workarea and `claim_key` and that the successor replaces it; on a
    /// lost compare-and-set it writes nothing and refuses with
    /// [`StoreError::TipMoved`]. `promoted_at` is the event's `at`
    /// (audit-and-lineage.md supersession chains; ADR-0025 d10).
    TipSuperseded {
        successor: Uuid,
        replaced: Uuid,
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

/// The record a commit writes inside its own transaction. The slice records
/// at commit instead of claiming an `in_progress` record in a transaction of
/// its own before any side effect (http-api.md idempotency): no use-case has
/// a side effect before its commit, and a second record under one scope
/// refuses the whole commit ([`StoreError::IdempotencyRecorded`]), so a
/// request still commits once. `idempotency.in_progress` is never answered.
/// A supersede that lost the race to another under the same key answers the
/// winner's response as a replay: directly when its commit finds the record,
/// or through the transport's retry when its commit was sealed on the head
/// the winner moved. A submit does the same unless it read the `claim_key`
/// after the winner committed; then it answers
/// `submission.claim_key_conflict`, because the winner's version holds it.
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

/// The claim version an event names as its subject, by the subject table of
/// the events tail (http-api.md#events-tail; ADR-0025 d15): the new version
/// of a `lifecycle.supersede`, the version that becomes the tip of a
/// `lifecycle.tip_override`, and the payload's `artifact_id` for every other
/// `lifecycle.*`, `submission.*` and `simulation.write` event. `None` for
/// every other family, and for a payload that names no version.
#[must_use]
pub fn claim_subject(event_type: &str, payload: &Value) -> Option<Uuid> {
    let field = match event_type {
        "lifecycle.supersede" => "new_artifact_id",
        "lifecycle.tip_override" => "to_artifact_id",
        t if t.starts_with("lifecycle.")
            || t.starts_with("submission.")
            || t == "simulation.write" =>
        {
            "artifact_id"
        }
        _ => return None,
    };
    payload
        .get(field)
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
}

/// The row of a tail event's claim subject, joined as data: the filter that
/// reads it is the use-case's (ADR-0025 d15; ADR-0019 d1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubjectRow {
    pub artifact_id: Uuid,
    pub workarea_id: Uuid,
    /// `memories.author_key_id`: the envelope `api_key_id` of the version's
    /// submission event.
    pub author_key_id: Uuid,
}

/// One raw event of a tail scan, with the row its [`claim_subject`] names
/// when the store holds one.
#[derive(Debug, Clone)]
pub struct TailEvent {
    pub event: SignedEnvelope,
    pub subject: Option<SubjectRow>,
}

/// One bounded tail scan and the head read in the snapshot it reads (0 for
/// a tenant with no chain), so `next_after_seq` and `head_seq` label one
/// journalled state (http-api.md#events-tail).
#[derive(Debug, Clone, Default)]
pub struct TailScan {
    pub head_seq: u64,
    pub events: Vec<TailEvent>,
}

/// The journal-and-metadata store port (workspace-plan.md row 1).
pub trait JournalStore: Send + Sync {
    /// The one-transaction commit point. The store refuses a commit whose
    /// `seq`/`prev_hash` do not extend the current head — chain assignment
    /// is serialized per tenant, and the conflict is typed, never silent.
    fn commit(
        &self,
        req: CommitRequest,
    ) -> impl Future<Output = Result<CommitReceipt, StoreError>> + Send {
        self.commit_chain(vec![req])
    }

    /// Several events of one tenant as one transaction: the first extends
    /// the current head and each next one extends the one before it. Any
    /// refusal — a conflict on any link, or a fold the store refuses, a lost
    /// compare-and-set included — writes none of them. An approving verdict
    /// and the ratification it triggers commit this way, so a verdict that
    /// loses the compare-and-set is never journalled (ADR-0002 d1; ADR-0025
    /// d10). Returns the receipt of the last event, the new head; an empty
    /// chain is refused.
    fn commit_chain(
        &self,
        reqs: Vec<CommitRequest>,
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

    /// The events tail's bounded scan: events with `seq >= from_seq`,
    /// ascending, at most `limit`, each joined to the row its
    /// [`claim_subject`] names, with the chain head read in the same
    /// snapshot (http-api.md#events-tail; ADR-0025 d15).
    fn events_with_subjects(
        &self,
        tenant_id: Uuid,
        from_seq: u64,
        limit: usize,
    ) -> impl Future<Output = Result<TailScan, StoreError>> + Send;

    fn memory(
        &self,
        tenant_id: Uuid,
        memory_id: Uuid,
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send;

    /// The rows of `memory_ids` this tenant holds, read set-wise in one
    /// statement, in the order named; an id no row has is absent. Recall
    /// hydrates exactly the rows it serves this way, after the `k` and
    /// budget cuts, never one read per hit (retrieval.md token budgets;
    /// ADR-0024 d4, ADR-0025 d7).
    fn memories(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send;

    /// Full-text recall: each matched hit as a candidate with its score, at
    /// most `k`, in the total order of hits — score descending, then
    /// `artifact_id` ascending (retrieval.md, the recall-ordering contract;
    /// ADR-0025 d8). Hits are served tips (tip and state promoted) of kind
    /// `memory` or `derived_fact` that `kinds` names, never retirement
    /// versions, so neither a replaced version nor any version of a retired
    /// claim is a hit; steering never is, whatever `kinds` names. The search
    /// covers exactly the `readable` workarea nodes — subtree(scope) ∩ the
    /// nodes where reader is effective, computed by the use-case. An empty
    /// set searches nothing: there is no tenant-wide fallback (ADR-0025 d1).
    fn recall_fts(
        &self,
        tenant_id: Uuid,
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> impl Future<Output = Result<Vec<RecallCandidate>, StoreError>> + Send;

    /// Own held recall: the held rows whose author is one of
    /// `author_key_ids`, of kind `memory` or `derived_fact` that `kinds`
    /// names, never retirement versions and never steering, on exactly the
    /// `readable` nodes, matched by the query and scored as
    /// [`recall_fts`](Self::recall_fts) scores, at most `k` in the same
    /// total order (retrieval.md, own held rows in recall; ADR-0025 d4). The
    /// state and the author predicate are the store's own, never a filter
    /// the caller may forget, so no other key's held text is ever matched.
    /// The designed form reads a `held_fts` table fenced by an author policy
    /// instead; the PoC stores bodies inline and reads them here (retrieval.md,
    /// held text containment; ADR-0025 d5).
    fn recall_own_held(
        &self,
        tenant_id: Uuid,
        author_key_ids: &[Uuid],
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> impl Future<Output = Result<Vec<RecallCandidate>, StoreError>> + Send;

    /// Applicable steering over `nodes`, the scope's ancestors the key reads,
    /// listed from the `universe` root down: every promoted served tip of
    /// kind `steering` whose workarea is one of them, never a retirement
    /// version, so a retired claim serves none. It is ordered structurally,
    /// never by a query: by its node's position in `nodes` — its depth —
    /// then `promoted_at` ascending, then `artifact_id` ascending
    /// (retrieval.md, the recall-ordering contract; ADR-0025 d8).
    fn steering_in_force(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send;

    /// One listing page (http-api.md search; ADR-0025 d13): the rows the
    /// filter's scope admits — the own key set's rows in every state off the
    /// masked nodes, or the promoted served tips on the given nodes — that
    /// pass every other predicate, ordered `created_at` descending, then
    /// `artifact_id` descending, strictly after `after` in that order, at
    /// most `limit` of them.
    fn list_memories(
        &self,
        tenant_id: Uuid,
        filter: &ListingFilter,
        after: Option<&ListingPosition>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send;

    /// The greatest version of `claim_key` in the workarea in any state, and
    /// whether a held, promoted or demoted version holds it: a capture's
    /// pre-`BEGIN` read (audit-and-lineage.md supersession chains; ADR-0025
    /// d10, d18).
    fn claim_key_state(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> impl Future<Output = Result<ClaimKeyState, StoreError>> + Send;

    /// The promoted served tip of `claim_key` in the workarea, if any
    /// (postgres-schema.md `memories_served_tip`; ADR-0025 d10).
    fn claim_tip(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send;

    /// Every held version whose `replaces` names `replaced`, in capture
    /// order: the rival successors of one tip (roles-and-keys.md rival
    /// successors; ADR-0025 d10).
    fn held_successors(
        &self,
        tenant_id: Uuid,
        replaced: Uuid,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send;

    /// Claim the next reviewable queue entry for this reviewer among the rows
    /// whose workarea is in `workareas`, the nodes where the reviewer's
    /// dreamer access is effective (ADR-0025 d1): entries claimed by another
    /// key are skipped, and so is any row whose reviewer-exclusion set holds
    /// the reviewer — its lineage closure, and for a held successor the
    /// authors of the held rivals of its target (ADR-0025 d10) — so the
    /// store never serves what this key may not judge (admission.md review;
    /// the R2 check proper still runs at verdict time in the use-case). An
    /// empty set serves nothing.
    fn review_claim_next(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        workareas: &[Uuid],
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send;

    /// One equality lookup on the key-value hash (roles-and-keys.md); a
    /// revoked key resolves to nothing.
    fn key_by_hash(
        &self,
        key_hash: &str,
    ) -> impl Future<Output = Result<Option<KeyRecord>, StoreError>> + Send;

    /// The key's `issued_seq`, grants and global denials, read in one
    /// snapshot so their epochs label one journalled state; `None` for a key
    /// this tenant does not hold. This is the one grants read effective
    /// access costs per request (workareas.md effective access; ADR-0025 d1,
    /// d14).
    fn key_access(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Option<KeyAccess>, StoreError>> + Send;

    fn workarea(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkareaRecord>, StoreError>> + Send;

    /// The node and each parent in turn, from the node to the root, by
    /// `parent_id`; `None` for a node this tenant does not hold. This is the
    /// one ancestry walk effective access costs per request (workareas.md;
    /// ADR-0025 d1).
    fn workarea_ancestry(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Option<Vec<WorkareaRecord>>, StoreError>> + Send;

    /// The node and every descendant, each once: the node first, then each
    /// depth in id order, so a parent always precedes its children; empty for
    /// a node this tenant does not hold (ADR-0025 d1, subtree(scope)).
    fn workarea_subtree(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkareaRecord>, StoreError>> + Send;

    /// The node's immediate children, ordered by name in byte order, then
    /// id: the order the workarea read serves them in (ADR-0025 d14).
    fn workarea_children(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkareaRecord>, StoreError>> + Send;

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
