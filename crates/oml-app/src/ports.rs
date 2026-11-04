//! The driven ports (ADR-0019 d2; workspace-plan.md port table).
//!
//! Async methods use RPITIT — `-> impl Future<Output = …> + Send` in the
//! trait, plain `async fn` in implementations — so the traits stay
//! dependency-free and futures are `Send` for the transports. The cost:
//! the traits are not dyn-compatible; compositions use generics.

use core::future::Future;
use std::net::IpAddr;

use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde_json::Value;
use uuid::Uuid;

use oml_domain::access::{Grant, KeyDenial};
use oml_domain::name::{Name, NamedKind, Naming};
use oml_domain::oauth::{RefreshDecision, RevokedCause, TokenKind};
use oml_domain::{ChainHead, EventTime, Sha256Hash, SignedEnvelope, key_fingerprint};

use crate::types::{AdminSearch, PinOutcome, ProviderAnswer, ProviderFailure, QueueDepth};
use crate::types::{
    AuthorizationRecord, ClaimKeyState, ClientDescription, ConnectCode, ConnectCodeRecord,
    ConnectGrant, ConnectOutcome, ConnectionRecord, CrewRecord, DocumentCaching, KeyAccess,
    KeyRecord, KeyReferences, ListingFilter, ListingPosition, MemoryRecord, OAuthClientRecord,
    RefreshFacts, RefreshOutcome, SweepReport, TenantRow, TokenPair, TokenRecord, UnnamedRows,
    WebEntry, WebSessionRecord, WorkareaRecord,
};
use oml_domain::web::Provider;

use crate::search::{CandidateScope, RetrievalQuery, Retrieved, RetrieverKind};

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
    /// A `connection.opened` fold found the connection not openable under
    /// the chain lock: not held by the tenant, not `authorized`, its
    /// authorization code redeemed already or expired at the commit's `at`,
    /// its connect code voided, its key revoked, or the key's open
    /// connection not the one the opening names as replaced. The commit
    /// wrote nothing. A token exchange that read the row before its fold,
    /// and lost a race for the code or saw a mint, a connection revocation
    /// or a `key.revoked` land since, judges the code by the reuse rule and
    /// otherwise answers `invalid_grant` (`oauth.authorization_code_invalid`)
    /// (postgres-schema.md `oauth_connections`; roles-and-keys.md
    /// connections; errors.md; ADR-0031 d8, d10, d11, d12).
    #[error("connection {connection_id} cannot open at this commit")]
    ConnectionNotOpenable { connection_id: Uuid },
    /// A `connection.code_issued` fold found that the code the mint names
    /// as the one it voids is not the key's one unredeemed, unvoided code:
    /// a code entry spent it, or another commit voided or minted one, after
    /// the mint read the key's codes. The commit wrote nothing; it is
    /// contention, retryable as a head conflict is, and answered
    /// `system.unavailable` (errors.md; event-catalog.md
    /// `connection.code_issued`; ADR-0031 d8).
    #[error("key {key_id}'s unredeemed connect code moved before the mint committed")]
    ConnectCodeMoved { key_id: Uuid },
    /// A creation named a name its kind already holds in the namespace, a
    /// revoked key's included: the unique index refused it, and the commit
    /// wrote nothing, its event included (postgres-schema.md names and
    /// tenants; errors.md `name.taken`; ADR-0026 d5, d10).
    #[error("the {} name {} is already taken in its namespace", .kind.as_str(), .name.as_str())]
    NameTaken { kind: NamedKind, name: Name },
    /// A tenant's partition DDL lost a lock contest, waiting past its lock
    /// timeout (`55P03`) or aborted by the deadlock detector (`40P01`), and
    /// committed nothing: retryable, answered `system.unavailable` with
    /// `details.reason` `partition_lock_timeout` (postgres-schema.md, the
    /// per-tenant partitions; ADR-0034 d11).
    #[error("the tenant's partition DDL lost a lock contest")]
    PartitionLockTimeout,
    /// A tenant's partition DDL failed otherwise and committed nothing: a
    /// defect, answered `system.internal` (ADR-0034 d11).
    #[error("the tenant's partition DDL failed: {0}")]
    Partition(String),
    /// A registration found 10 000 registered clients that have not
    /// connected and are younger than 24 hours: it wrote nothing, and is
    /// answered `limit.rate` until some lapse (http-api.md
    /// `POST /oauth/register`; errors.md; ADR-0031 d5).
    #[error("the unused client registrations are at their cap")]
    CapReached,
    /// An allowlist entry's email that another tenant's entry holds: the
    /// deployment-wide unique index refused it and the commit wrote
    /// nothing, its event included (ADR-0045 d2).
    #[error("the email is held by another tenant's allowlist")]
    EmailTaken,
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
///
/// Every fold that creates a named row writes its name, and refuses a name
/// its kind already holds in the namespace with [`StoreError::NameTaken`]
/// (ADR-0026 d1, d5); a row that names a crew names one of its own tenant's
/// crew records, or the store refuses it (postgres-schema.md `crews`).
#[derive(Debug, Clone)]
pub enum MetadataFold {
    /// `tenant.genesis`: the tenant's row, named, unique in the deployment.
    /// The PoC creates its tenant here, since the provider chain's
    /// `tenant.created` is not built. A tenant that already has its row
    /// keeps it, name included, so the fold changes nothing then.
    TenantGenesis {
        name: Name,
    },
    /// `workarea.create`: the node, its path derived from its parent's path
    /// and its name (postgres-schema.md `workareas`). Its name is unique in
    /// its namespace, the shared one or its user root's, and refused with
    /// [`StoreError::NameTaken`] otherwise (ADR-0044 d5).
    WorkareaCreated(WorkareaRecord),
    /// `key.issued`: the key row, its `issued_seq` this commit's seq, with
    /// its credential. A connector key (`Credential::Connection`) carries no
    /// hash, or the store refuses it, so it never resolves through
    /// [`JournalStore::key_by_hash`] (postgres-schema.md
    /// `api_keys_connector_has_no_hash`; ADR-0031 d7).
    KeyIssued(KeyRecord),
    /// `crew.created`: one crew record (ADR-0026 d8).
    CrewCreated(CrewRecord),
    /// `crew.updated`: the record's charter URI and external manager, the
    /// full new values; the store refuses a crew the tenant does not hold,
    /// and never changes a name (ADR-0026 d8).
    CrewUpdated(CrewRecord),
    /// `workarea.crew_updated`: the node now names `crew_id`, a crew record
    /// of its own tenant, or the store refuses it (ADR-0026 d8).
    WorkareaCrewUpdated {
        workarea_id: Uuid,
        crew_id: Uuid,
    },
    /// `system.names_assigned`: each listed row of the commit's tenant, and
    /// the tenant itself, takes the name the event assigns, every workarea's
    /// path derives from those names, and each crew record is created and
    /// named by the workareas listed under it. Every listed row must hold
    /// no name yet, or the store refuses the commit, which writes nothing; a
    /// name its kind already holds is [`StoreError::NameTaken`]
    /// (postgres-schema.md folds; ADR-0026 d11).
    NamesAssigned(Naming),
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
    /// `key.denial_lifted`: the denial `denial_id` on `key_id` masks nothing
    /// from this commit on, its row staying as history, and this commit's
    /// seq is the lift's epoch. The store refuses a lift of a denial it does
    /// not hold on that key, and of one already lifted (postgres-schema.md
    /// `key_denials`; event-catalog.md `key.denial_lifted`; ADR-0026 d11,
    /// rule 5).
    KeyDenialLifted {
        key_id: Uuid,
        denial_id: Uuid,
    },
    /// `key.revoked`: the key resolves by its hash no more; its row, grants
    /// and denials stay as history. The store refuses a revocation of a key
    /// it does not hold (roles-and-keys.md key lifecycle; ADR-0025 d2). It
    /// voids the key's unredeemed codes, their `voided_seq` this commit's
    /// seq (roles-and-keys.md connect codes; ADR-0031 d8).
    KeyRevoked {
        key_id: Uuid,
    },
    /// `connection.code_issued`: the code's row, its `issued_seq` this
    /// commit's seq. The commit voids every code of the key from which no
    /// connection has opened, spent or not, their `voided_seq` this seq.
    /// `voids` is the key's unredeemed code the event names as replaced,
    /// else `None`; the store refuses a commit whose `voids` is not the
    /// key's one unredeemed, unvoided code with
    /// [`StoreError::ConnectCodeMoved`], and a code of a key it does not
    /// hold (event-catalog.md `connection.code_issued`; ADR-0031 d8).
    ConnectCodeIssued {
        code: ConnectCode,
        voids: Option<Uuid>,
    },
    /// `connection.opened`: the `authorized` connection opens, its
    /// `opened_seq` this commit's seq and its `auth_code_redeemed_at` this
    /// commit's `at`, and the key's earlier open connection, which
    /// `replaces` names, is revoked with the cause `replaced` and its
    /// `revoked_seq` this seq. The store refuses a connection that is not
    /// `authorized`, one whose authorization code is redeemed already or
    /// expired at this `at`, one whose connect code is voided, a `replaces`
    /// that is not the key's open connection, and a key that is revoked, so
    /// no connection opens after its key's `key.revoked` or after a mint or
    /// revocation voided its code since Connect. Each refusal is
    /// [`StoreError::ConnectionNotOpenable`] (postgres-schema.md
    /// `oauth_connections`; event-catalog.md `connection.opened`; ADR-0031
    /// d7, d8, d10, d11, d12).
    ///
    /// The exchange's conditional update matches the row by
    /// `auth_code_hash`, the presented code's digest, so a digest that is
    /// not the connection's is refused the same way, and on its match the
    /// fold inserts `tokens`, the pair the exchange minted, created at the
    /// commit's `at`: redemption, the pair and the event are one transaction
    /// under the chain lock. No journal payload carries either digest
    /// (postgres-schema.md, redemption and rotation are conditional updates;
    /// ADR-0031 d10, d12).
    ConnectionOpened {
        connection_id: Uuid,
        replaces: Option<Uuid>,
        auth_code_hash: Sha256Hash,
        tokens: TokenPair,
    },
    /// `connection.revoked`: the open connection is revoked with `cause`,
    /// its `revoked_seq` this commit's seq, and every code of its key from
    /// which no connection has opened, spent or not, is voided, their
    /// `voided_seq` this seq. The store refuses a connection that is not
    /// open and the cause `replaced`, which only a newer `connection.opened`
    /// gives (event-catalog.md `connection.revoked`; ADR-0031 d8, d12).
    ConnectionRevoked {
        connection_id: Uuid,
        cause: RevokedCause,
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
    /// `web.email_allowed`: the entry's row, with no pinned subject. The
    /// store refuses an email another tenant's entry holds with
    /// [`StoreError::EmailTaken`] (ADR-0045 d2).
    WebEmailAllowed(WebEntry),
    /// `web.email_changed`: the entry's user and admin flag, its pinned
    /// subjects kept, and every session of the entry ended in the same
    /// commit (ADR-0045 d2, d5).
    WebEmailChanged(WebEntry),
    /// `web.email_removed`: the entry's row and every session of it, gone
    /// in the same commit; the store refuses an entry it does not hold
    /// (ADR-0045 d2, d13).
    WebEmailRemoved {
        email_hash: Sha256Hash,
    },
    /// `memory.admin_read`: an admin key read memories' text; nothing
    /// changes but the journal (ADR-0045 d21).
    AdminRead,
    /// `search.default_set`: the tenant's default search pipeline, by name,
    /// or `None` to follow the deployment's again (ADR-0049 d4).
    SearchDefaultSet {
        pipeline: Option<String>,
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

/// The retriever port (ADR-0049 d2): ranked candidate lists over a scope
/// the core computes. An adapter ranks and never decides visibility — the
/// core hands it the readable nodes, the kinds and the own held keys, an SQL
/// adapter additionally runs under row security, and the core re-checks
/// every row it serves (R3). No ranking input reads a verdict, a reviewer or
/// `promoted_at` (ADR-0001 d3). The laws every adapter passes are in
/// `oml-port-contracts` (`retriever_*`).
pub trait Retriever: Send + Sync {
    /// The retrievers this adapter serves; a deployment enabling a pipeline
    /// that needs another is refused at start (ADR-0049 d4).
    fn retrievers(&self) -> Vec<RetrieverKind>;

    /// One ranked list: at most `query.k` candidates of `scope`, promoted
    /// served tips and the own held rows alike under one score, in the
    /// total order of hits — score descending, then `artifact_id`
    /// ascending. Never a steering row, a vocabulary kind or a retirement
    /// version; an empty scope ranks nothing (ADR-0025 d1, d4, d8). With
    /// `MatchMode::All` only rows holding every term are candidates. A kind
    /// this adapter does not serve answers [`StoreError::Backend`].
    fn retrieve(
        &self,
        tenant_id: Uuid,
        kind: RetrieverKind,
        scope: &CandidateScope,
        query: RetrievalQuery<'_>,
    ) -> impl Future<Output = Result<Retrieved, StoreError>> + Send;

    /// Spellings for `words` taken from the words of rows in `scope` alone,
    /// at most `limit`, for `explain.did_you_mean` (ADR-0049 d6); an
    /// adapter with no trigram list answers none.
    fn suggest(
        &self,
        tenant_id: Uuid,
        scope: &CandidateScope,
        words: &[String],
        limit: usize,
    ) -> impl Future<Output = Result<Vec<String>, StoreError>> + Send;
}

/// The journal-and-metadata store port (workspace-plan.md row 1). It is a
/// [`Retriever`] too: the keyword retrievers read the same PostgreSQL rows
/// (ADR-0013 d1), so the core reaches both through one handle; a retriever
/// on another store, as an embedder's index would be, gets its own (ADR-0049
/// d2).
pub trait JournalStore: Retriever + Send + Sync {
    /// The tenant's journalled default search pipeline, by name, or `None`
    /// when it follows the deployment's (ADR-0049 d4).
    fn tenant_search_default(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

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

    /// Creates `tenant_id`'s fenced partitions, before the commit whose fold
    /// creates its row (`tenant.genesis` today), in a transaction of their
    /// own that the store's serving connections never hold open: the
    /// Postgres store runs the DDL as the owner, on a connection opened for
    /// this call alone (postgres-schema.md, the per-tenant partitions;
    /// ADR-0034 d4, d11). A refusal commits nothing:
    /// [`StoreError::PartitionLockTimeout`] when it lost a lock contest, a
    /// timeout or a deadlock, [`StoreError::Partition`] otherwise. Partitions that
    /// committed are kept when the genesis commit then fails; the retry
    /// mints a fresh id.
    fn create_tenant_partitions(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

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

    /// The memory ids of the queue entries this reviewer key has claimed,
    /// oldest first and at most two, enough to tell one open item from
    /// several: the default of a verdict sent without `serve_id`
    /// (ADR-0039 d10).
    fn review_claimed(
        &self,
        tenant_id: Uuid,
        reviewer_key_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send;

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

    /// The id of the row of `kind` that holds `name` in the tenant's
    /// namespace, a revoked or expired key's included, since a name is never
    /// released; `None` when no row of the tenant holds it. One lookup on
    /// the unique index that also refuses a taken name (http-api.md names
    /// and ids; ADR-0026 d1, d4, d10). Tenant names live
    /// in the deployment's namespace and operator-key names in the
    /// provider's, which the operator tier reads and which this read never
    /// consults: a [`NamedKind::Tenant`] resolves nothing here, and no tenant
    /// holds the provider tenant's nil id, so nothing resolves under it. A
    /// [`NamedKind::Workarea`] resolves in the shared namespace alone:
    /// `universe`, each user root and each shared workarea (ADR-0044 d5).
    fn resolve_name(
        &self,
        tenant_id: Uuid,
        kind: NamedKind,
        name: &Name,
    ) -> impl Future<Output = Result<Option<Uuid>, StoreError>> + Send;

    /// The id of the workarea that holds `name` in the namespace of the
    /// user root `user_root`: the names strictly beneath that root; `None`
    /// when none does, and for a `user_root` that is no user root, whose
    /// namespace holds nothing (ADR-0044 d5). [`JournalStore::resolve_name`]
    /// of a workarea reads the shared namespace alone.
    fn resolve_user_workarea(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        name: &Name,
    ) -> impl Future<Output = Result<Option<Uuid>, StoreError>> + Send;

    /// One crew record of the tenant; `None` for a crew it does not hold
    /// (postgres-schema.md `crews`; ADR-0026 d8).
    fn crew(
        &self,
        tenant_id: Uuid,
        crew_id: Uuid,
    ) -> impl Future<Output = Result<Option<CrewRecord>, StoreError>> + Send;

    /// Every crew record of the tenant, ordered by name in byte order
    /// (http-api.md crews; ADR-0026 d8).
    fn crews(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Vec<CrewRecord>, StoreError>> + Send;

    /// One key row of the tenant by its id, a revoked key's included, since
    /// its name and role stay history; `None` for a key the tenant does not
    /// hold (roles-and-keys.md key lifecycle; ADR-0026 d10).
    fn key(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Option<KeyRecord>, StoreError>> + Send;

    /// The rows a key of the tenant references, read in one query under the
    /// tenant's fence: the tenant's id and name, the key's default workarea
    /// with the path its ancestry derives, and its crew record; `None` for a
    /// key the tenant does not hold (ADR-0039 d15).
    fn key_references(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Option<KeyReferences>, StoreError>> + Send;

    /// The tenant's rows that have no name yet: its own row, its workareas
    /// and its keys, which the lead names before it serves (http-api.md
    /// rows created before names; ADR-0026 d11).
    fn unnamed_rows(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<UnnamedRows, StoreError>> + Send;

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

    /// The node's immediate children, ordered by name in byte order: the
    /// order the workarea read serves them in (ADR-0025 d14). Names are
    /// unique in the tenant, so no two children tie (ADR-0026 d1).
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

/// The web pages' rows (ADR-0045 d2, d4, d5; postgres-schema.md "web"):
/// the reads of the allowlist, the pre-tenant lookups of an entry by its
/// email's digest and of a session by its token's digest, the pin of a
/// provider subject, and the session rows, which are operational state and
/// journal nothing. The allowlist acts are folds of [`MetadataFold`].
pub trait WebStore: JournalStore {
    /// The entry an email's digest names, in whichever tenant holds it,
    /// before any tenant is known; `None` for a digest no entry holds.
    fn web_entry_by_email(
        &self,
        email_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<WebEntry>, StoreError>> + Send;

    /// The tenant's entries in the order of their email's digest, after
    /// the digest `after`, at most `limit`; a cursor that is a digest keeps
    /// every address out of the listing's query string (ADR-0045 d2).
    fn web_entries(
        &self,
        tenant_id: Uuid,
        after: Option<&Sha256Hash>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<WebEntry>, StoreError>> + Send;

    /// Pins `subject` for `provider` on the entry when none is pinned,
    /// and otherwise compares it, in one conditional update (ADR-0045 d4).
    /// An entry the tenant does not hold is [`StoreError::Backend`].
    fn web_pin(
        &self,
        tenant_id: Uuid,
        email_hash: &Sha256Hash,
        provider: Provider,
        subject: &str,
    ) -> impl Future<Output = Result<PinOutcome, StoreError>> + Send;

    /// Writes a session row, and ends the row `replaces` names in the same
    /// transaction, so a browser that arrives with an old session leaves
    /// with only the new one (ADR-0045 d5).
    fn web_session_open(
        &self,
        session: WebSessionRecord,
        replaces: Option<Sha256Hash>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The session a token's digest names, before any tenant is known;
    /// `None` for a digest no row holds.
    fn web_session(
        &self,
        token_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<WebSessionRecord>, StoreError>> + Send;

    /// Ends one session: its row is gone.
    fn web_session_end(
        &self,
        tenant_id: Uuid,
        token_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The tenant's keys whose user root is `user_root`, revoked ones
    /// included, in name order (ADR-0045 d9).
    fn web_user_keys(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> impl Future<Output = Result<Vec<KeyRecord>, StoreError>> + Send;

    /// The tenant's session keys serving `session_user` (ADR-0045 d6).
    fn web_session_keys(
        &self,
        tenant_id: Uuid,
        session_user: &Name,
    ) -> impl Future<Output = Result<Vec<KeyRecord>, StoreError>> + Send;

    /// The tenant's held versions per workarea: count and oldest
    /// (ADR-0045 d10).
    fn web_queue(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Vec<QueueDepth>, StoreError>> + Send;

    /// Every memory of the tenant the search names, any state, newest
    /// first (ADR-0045 d21).
    fn web_admin_search(
        &self,
        tenant_id: Uuid,
        search: &AdminSearch,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send;

    /// Removes the tenant's session rows that expired before `at`; the
    /// count removed.
    fn web_sessions_sweep(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;
}

/// What a sign-in provider's endpoints answer for one code (ADR-0045 d3,
/// d4): the adapter calls the provider's token endpoint with the code, the
/// PKCE verifier and the client secret it holds, and for GitHub its two
/// API calls, then discards the provider's access token. It judges
/// nothing; the application admits or refuses what it answers. Tests use a
/// double and never reach a provider.
pub trait SignInExchange: Send + Sync {
    fn redeem(
        &self,
        provider: Provider,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> impl Future<Output = Result<ProviderAnswer, ProviderFailure>> + Send;
}

/// The connection rows of a store that serves connector keys
/// (postgres-schema.md "connections (ADR-0031)"): the operational writes
/// that journal nothing — code entry spending a code and writing its
/// authorization request, Connect or Cancel taking that request, Connect
/// creating an `authorized` connection, a client's row, a refresh's
/// rotation and the sweep — and the reads, the pre-tenant digest lookups
/// among them. Every argument that names a secret is its digest, never the
/// secret (ADR-0031 d10). The journalled acts are folds of
/// [`MetadataFold`] (ADR-0031 d5, d8, d9, d12; event-catalog.md,
/// operational state).
pub trait ConnectionStore: JournalStore {
    /// The pre-tenant lookup of a connect code by the SHA-256 of its twelve
    /// decoded symbols, as `api_keys.hash` is found: the code's row, which
    /// names its key and its state, and the tenant that then sets
    /// `oml.tenant_id`; `None` for a digest no row holds (postgres-schema.md
    /// "connections (ADR-0031)"; ADR-0031 d8).
    fn code_by_hash(
        &self,
        code_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<TenantRow<ConnectCodeRecord>>, StoreError>> + Send;

    /// The tenant's name as the operator gave it at `POST /v2/tenants`,
    /// which no tenant admin can set, under that tenant: what the
    /// authorization page's confirmation names the key's tenant by; `None`
    /// for a tenant it does not hold or one not yet named (http-api.md "The
    /// authorization page"; ADR-0031 d9 step 2; ADR-0026 d1).
    fn tenant_name(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<Name>, StoreError>> + Send;

    /// Spends the code by its first correct entry, `redeemed_at` = `at`, in
    /// one conditional update that matches only a code still unredeemed,
    /// unvoided and unexpired at `at`, so of two racing correct entries
    /// exactly one spends it: `true` when this call spent it, `false`
    /// otherwise, the code unchanged. The key's state is the caller's check
    /// (ADR-0031 d8, d9).
    fn spend_code(
        &self,
        tenant_id: Uuid,
        code_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Cancel: deletes the authorization request's row by `handle_hash`
    /// under the provider tenant and returns it, `None` when Connect or
    /// Cancel took it already or it was removed, so a later confirmation
    /// with that handle finds no row (postgres-schema.md "connections
    /// (ADR-0031)"; ADR-0031 d9).
    fn take_authorization_by_handle(
        &self,
        handle_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<AuthorizationRecord>, StoreError>> + Send;

    /// The pre-tenant lookup of a connection by its authorization code's
    /// digest, redeemed or not, so a second redemption is recognized while
    /// the connection stands; `None` for a digest no row holds
    /// (ADR-0031 d10).
    fn connection_by_auth_code(
        &self,
        auth_code_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<TenantRow<ConnectionRecord>>, StoreError>> + Send;

    /// The pre-tenant lookup of a token by its digest, matching only a row
    /// of `kind`: at `/mcp` the kind is `access` and at a refresh `refresh`,
    /// so a refresh token's digest finds nothing at `/mcp`
    /// (roles-and-keys.md connections; ADR-0031 d11).
    fn token_by_hash(
        &self,
        token_hash: &Sha256Hash,
        kind: TokenKind,
    ) -> impl Future<Output = Result<Option<TenantRow<TokenRecord>>, StoreError>> + Send;

    /// Inserts an open connection's access and refresh rows, created at
    /// `at`; a connection that is not open, or a digest held already, is
    /// refused and inserts neither (ADR-0031 d10).
    fn insert_token_pair(
        &self,
        tenant_id: Uuid,
        connection_id: Uuid,
        tokens: TokenPair,
        at: &EventTime,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// A refresh, in one transaction that takes no chain lock: it finds the
    /// presented refresh token's row, locks its connection's row, reads
    /// [`RefreshFacts`] at `at` and asks `decide`; only on
    /// [`RefreshDecision::Rotate`] it runs the conditional update that marks
    /// the token rotated where it is still unrotated and unexpired, and on
    /// that match inserts `next`. Of two racing refreshes with one token,
    /// the second reads the first's rotation under the lock, so exactly one
    /// mints. A reuse decision writes nothing here: its `connection.revoked`
    /// is a commit of its own under the chain lock (postgres-schema.md,
    /// redemption and rotation are conditional updates; ADR-0031 d10, d11).
    fn rotate_refresh<F>(
        &self,
        tenant_id: Uuid,
        token_hash: &Sha256Hash,
        next: TokenPair,
        at: &EventTime,
        decide: F,
    ) -> impl Future<Output = Result<RefreshOutcome, StoreError>> + Send
    where
        F: FnOnce(Option<&RefreshFacts>) -> RefreshDecision + Send;

    /// The operational removal at `at` (postgres-schema.md, the removal
    /// paragraph; ADR-0031 d9, d10): under `tenant_id`, every `authorized`
    /// connection whose authorization code lapsed unexchanged, every token
    /// row past its own expiry and every token row of a revoked connection,
    /// and a revoked connection's `auth_code_hash`; under the provider
    /// tenant, every authorization request 24 hours past its expiry. A
    /// rotated refresh token keeps its row until its own expiry. Journals
    /// nothing (ADR-0031 d12).
    fn sweep_expired(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<SweepReport, StoreError>> + Send;

    /// Connect, in one transaction (postgres-schema.md "connections
    /// (ADR-0031)"): it deletes the authorization request's row by
    /// `handle_hash` under the provider tenant, stamps the row's client's
    /// `connected_at` at `grant.at` where it is null, then, under the
    /// row's code tenant, reads the code's voiding and creates the
    /// `authorized` connection from the row and `grant`. A voided code
    /// changes nothing ([`ConnectOutcome::CodeVoided`]); a refused insert, a
    /// key or code the code tenant does not hold or a second connection of
    /// one code (`oauth_connections_tenant_id_code_id`), rolls the whole
    /// transaction back, so the request's row stays and the stamp commits
    /// only with the connection row (ADR-0031 d5, d8, d9; ADR-0034 d12).
    fn connection_connect(
        &self,
        handle_hash: &Sha256Hash,
        grant: ConnectGrant,
    ) -> impl Future<Output = Result<ConnectOutcome, StoreError>> + Send;

    /// The authorization request's row at a correct code, under the
    /// provider tenant; a second row with one handle digest is refused
    /// (postgres-schema.md `oauth_authorizations`; ADR-0031 d9).
    fn authorization_insert(
        &self,
        record: AuthorizationRecord,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The authorization request's row by its handle digest; `None` once
    /// Connect or Cancel took it.
    fn authorization(
        &self,
        handle_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<AuthorizationRecord>, StoreError>> + Send;

    /// A client's row, a cached document or a registration, under the
    /// provider tenant; a second row for one `client_id` is refused
    /// (postgres-schema.md `oauth_clients`; ADR-0031 d5).
    fn oauth_client_insert(
        &self,
        record: OAuthClientRecord,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// A client's row by its `client_id`; `None` for one it does not hold.
    fn oauth_client(
        &self,
        client_id: &str,
    ) -> impl Future<Output = Result<Option<OAuthClientRecord>, StoreError>> + Send;

    /// Caches a fetched client ID metadata document, under the provider
    /// tenant, for [`cache_lifetime`](oml_domain::oauth::cache_lifetime) of
    /// `max_age` from `fetched_at`, its `last_used_at` = `fetched_at`. A
    /// cached client's row is refreshed in place and keeps its
    /// `connected_at` and `created_at`. A new row in a cache holding
    /// [`CACHED_DOCUMENTS_MAX`](oml_domain::oauth::CACHED_DOCUMENTS_MAX)
    /// documents evicts the least recently used one whose client has not
    /// connected; when every cached client has connected it writes nothing
    /// and answers [`DocumentCaching::NotCached`], never a refusal
    /// (postgres-schema.md `oauth_clients`; ADR-0031 d5).
    fn upsert_cimd(
        &self,
        document: ClientDescription,
        fetched_at: &EventTime,
        max_age: u64,
    ) -> impl Future<Output = Result<DocumentCaching, StoreError>> + Send;

    /// A cached document's use at `at`, its `last_used_at`, the cache's
    /// eviction order: `true` when a cached document's row was touched,
    /// `false` for a `client_id` no cached document holds (ADR-0031 d5).
    fn touch_cimd(
        &self,
        client_id: &str,
        at: &EventTime,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// A dynamic registration's row, created at `at`, under the provider
    /// tenant. Refused with [`StoreError::CapReached`], writing nothing,
    /// when
    /// [`UNUSED_REGISTRATIONS_MAX`](oml_domain::oauth::UNUSED_REGISTRATIONS_MAX)
    /// registrations that have not connected were created within
    /// [`UNUSED_REGISTRATION_SECONDS`](oml_domain::oauth::UNUSED_REGISTRATION_SECONDS)
    /// before `at`; a second row for one `client_id` is refused
    /// (http-api.md `POST /oauth/register`; ADR-0031 d5).
    fn insert_dcr(
        &self,
        registration: ClientDescription,
        at: &EventTime,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Removes every registration that has not connected and was created at
    /// or before `registered_by`, which the caller sets 24 hours before
    /// now; cached documents are untouched. Answers how many rows it
    /// removed, and journals nothing (ADR-0031 d5, d12).
    fn remove_unused_dcr(
        &self,
        registered_by: &EventTime,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;

    /// The key's codes in the order they were issued, voided and spent ones
    /// included; empty for a key the tenant does not hold.
    fn connect_codes(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConnectCodeRecord>, StoreError>> + Send;

    /// The key's connections in any state, in the order they were created,
    /// ties by id; empty for a key the tenant does not hold
    /// (roles-and-keys.md connections; ADR-0031 d11).
    fn connections(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConnectionRecord>, StoreError>> + Send;

    /// One connection of the tenant, in any state; `None` for one it does
    /// not hold.
    fn connection(
        &self,
        tenant_id: Uuid,
        connection_id: Uuid,
    ) -> impl Future<Output = Result<Option<ConnectionRecord>, StoreError>> + Send;
}

/// The sealing port of an authorization request's handle (ADR-0031 d9 step
/// 1): an AEAD under a key the lead generates when it starts and holds only
/// in memory, so a handle is the request itself and no row is written for
/// it. `open` answers the sealed bytes only for a handle this sealer's key
/// sealed and nobody altered; any other string — another process's handle,
/// a tampered one, or garbage — opens to `None`.
pub trait HandleSealer: Send + Sync {
    fn seal(&self, plaintext: &[u8]) -> String;
    fn open(&self, handle: &str) -> Option<Vec<u8>>;
}

/// A client ID metadata document as fetched: its body as received, within
/// the 16 KiB bound, and its `Cache-Control` `max-age` in seconds, `0`
/// where it allows no caching (ADR-0031 d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedDocument {
    pub body: Vec<u8>,
    pub max_age: u64,
}

/// Why a document was not fetched (http-api.md "Client identity"): any
/// failure within the fetch bounds, or a full fetch cap. Either serves a
/// cached copy kept within its 24 hours; without one it is
/// `oauth.client_invalid` (ADR-0031 d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFetchFailure {
    /// An answer outside the address allow-list, TLS, a redirect, the
    /// 5-second or 16 KiB cut, or any other failed fetch.
    Failed,
    /// At most 16 fetches at once, one per host name and one per resolved
    /// address; past any cap (`reason: fetch_busy`).
    Busy,
}

/// The outbound fetch of a client ID metadata document at its URL, under
/// the bounds of http-api.md "Client identity" (ADR-0031 d5). The use case
/// decides what the cache serves; the fetcher only fetches.
pub trait ClientDocumentFetcher: Send + Sync {
    fn fetch(
        &self,
        url: &str,
    ) -> impl Future<Output = Result<FetchedDocument, DocumentFetchFailure>> + Send;
}

/// The fetch-cap slots one piece of outbound work holds (ADR-0031 d5):
/// they are given back when the last guard holding them drops. An adapter
/// moves the guard it is handed into whatever runs the I/O — the
/// `spawn_blocking` closure, not the future awaiting it — so a dropped
/// request or a timeout that fires while a resolver or a connection still
/// runs leaves its slots taken until that work ends. Only
/// [`crate::oauth_fetch::CappedFetcher`] mints one.
#[must_use = "the slots are given back when the guard drops"]
pub struct FetchGuard {
    _slots: Vec<std::sync::Arc<crate::oauth_fetch::Slot>>,
}

impl FetchGuard {
    pub(crate) fn new(slots: Vec<std::sync::Arc<crate::oauth_fetch::Slot>>) -> Self {
        Self { _slots: slots }
    }
}

/// The outbound I/O under a document fetch, split at the one point the fetch
/// caps need between its halves (ADR-0031 d5; http-api.md "Client
/// identity"): resolve the host, then fetch from one address the use case
/// checked. [`crate::oauth_fetch::CappedFetcher`] holds the caps and the
/// address allow-list over it, so an adapter cannot widen either. Each half
/// is handed the [`FetchGuard`] of the slots it runs under and keeps it
/// until its I/O ends, whether or not anything still awaits it.
pub trait DocumentFetcher: Send + Sync {
    /// Every A and AAAA answer for `host`, unfiltered; a resolver failure
    /// or timeout is [`DocumentFetchFailure::Failed`]. `guard` is held
    /// until the resolver itself returns.
    fn resolve(
        &self,
        host: &str,
        guard: FetchGuard,
    ) -> impl Future<Output = Result<Vec<IpAddr>, DocumentFetchFailure>> + Send;

    /// `GET url` over a connection to `address` with no second resolution,
    /// no proxy and no redirect, `Accept-Encoding: identity`, cut at 5
    /// seconds and at 16 KiB received; TLS verifies the URL's host, sent
    /// as SNI. Any answer but `200` is [`DocumentFetchFailure::Failed`].
    /// `guard` is held until the connection is done with.
    fn fetch(
        &self,
        url: &str,
        address: IpAddr,
        guard: FetchGuard,
    ) -> impl Future<Output = Result<FetchedDocument, DocumentFetchFailure>> + Send;
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

/// What a request resolved, told as it resolves to the transport that logs
/// the request: the key a credential named, or that none did, and the
/// workarea a verb targets, by id (ADR-0042 d2, d14). It decides nothing,
/// and nothing it is told is a name or a credential's value; the app works
/// the same with [`NoObserver`], which tells no one.
pub trait RequestObserver: Send + Sync {
    /// A credential resolved to `key`, through `connection` when it was a
    /// connection's access token, or, with `None`, resolved to no key.
    fn credential(&self, key: Option<&KeyRecord>, connection: Option<Uuid>);

    /// The request's verb targets `workarea_id`.
    fn workarea(&self, workarea_id: Uuid);

    /// A recall ran `pipeline`, and `fallback` says whether `match: all`
    /// fell back to any term (ADR-0049 d4, d5). Telling no one is the
    /// default.
    fn search(&self, pipeline: &'static str, fallback: bool) {
        let _ = (pipeline, fallback);
    }
}

/// The observer of a composition that logs no request.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoObserver;

impl RequestObserver for NoObserver {
    fn credential(&self, _key: Option<&KeyRecord>, _connection: Option<Uuid>) {}

    fn workarea(&self, _workarea_id: Uuid) {}
}
