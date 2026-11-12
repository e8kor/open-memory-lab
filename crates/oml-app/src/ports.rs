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
use oml_domain::{
    ChainHead, EventTime, Sha256Hash, SignedEnvelope, WorkareaStatus, key_fingerprint,
};

use crate::types::{
    AdminSearch, AuthorizationRecord, BriefEdgeRecord, ClaimEdgePosition, ClaimEdgeRecord,
    ClaimEdgeSelection, ClaimKeyState, ClientDescription, ConnectCode, ConnectCodeRecord,
    ConnectGrant, ConnectOutcome, ConnectionRecord, CrewRecord, DocumentCaching, HeldFilter,
    HeldRow, KeyAccess, KeyRecord, KeyReferences, ListingFilter, ListingPosition, MemoryRecord,
    OAuthClientRecord, PinOutcome, ProviderAnswer, ProviderFailure, QueueDepth, RefreshFacts,
    RefreshOutcome, SourceCheck, SweepReport, TenantRow, TokenPair, TokenRecord, TransitionRecord,
    UnnamedRows, VersionFinding, WebEntry, WebSessionRecord, WorkareaRecord,
};
use oml_domain::web::Provider;

use crate::search::{
    CandidateScope, PostingsHealth, RetrievalQuery, Retrieved, RetrieverKind, SparsePending,
    SparsePosting, SparseTenant, SparseTerm,
};

/// A storage refusal. `SeqConflict` is how the serialized per-tenant chain
/// surfaces to a concurrent writer (ADR-0002: one chain head, one assigner).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// An in-progress commit found a held version on its subtree inside
    /// the committing transaction; the commit wrote nothing (ADR-0030 d6).
    #[error("a version in the committed subtree is still held")]
    HeldRemaining,
    /// A promotion or ratification carried no approval to a store whose
    /// schema keeps one (migration 0044): the commit wrote nothing. The
    /// application always sends one; only a store over a schema before 0044
    /// takes `None`, as the migration laws' older databases do (ADR-0077
    /// d6).
    #[error("the promotion of version {memory_id} carries no approval, and this schema keeps one")]
    ApprovalMissing { memory_id: Uuid },
    #[error("chain head moved: expected seq {expected_seq}, commit carries seq {found_seq}")]
    SeqConflict { expected_seq: u64, found_seq: u64 },
    #[error("prev_hash does not match the stored head at seq {seq}")]
    PrevHashConflict { seq: u64 },
    /// A ratification's compare-and-set found `replaced` no longer the
    /// promoted tip under its successor's workarea and `claim_key`: the
    /// commit wrote nothing (ADR-0025 d10).
    #[error("version {replaced} is no longer the served tip its successor replaces")]
    TipMoved { replaced: Uuid },
    /// A ratification's in-transaction re-check found its proposal no
    /// longer live: the row is already ratified, or its `from` is no longer
    /// the node's status. The commit wrote nothing, its event included;
    /// answered `workarea.transition_refused`, `details.reason` `stale`
    /// (errors.md; workareas.md, ratification re-checks; ADR-0009 d3).
    #[error("transition {transition_id} on workarea {workarea_id} is stale")]
    TransitionStale {
        transition_id: Uuid,
        workarea_id: Uuid,
        /// The proposal's `from` and `to`, as the row holds them.
        from: WorkareaStatus,
        to: WorkareaStatus,
    },
    /// A ratification of a step to `validated` found, inside its
    /// transaction and after the chain lock, a direct child of the node
    /// that is not `validated`: a child created, or a subtree reparented
    /// in, since the proposal. The commit wrote nothing; answered
    /// `workarea.transition_refused`, `details.reason`
    /// `children_not_validated` (errors.md; ADR-0009 d4).
    #[error("transition {transition_id}: workarea {workarea_id} has a child not validated")]
    ChildrenNotValidated {
        transition_id: Uuid,
        workarea_id: Uuid,
    },
    /// A ratification's ratifying key is the proposing key, or holds the
    /// proposer's principal, read from the key row inside the transaction.
    /// The commit wrote nothing; answered `review.ratifier_is_proposer`
    /// (errors.md; ADR-0010 R2).
    #[error("transition {transition_id}'s ratifier is its proposer")]
    RatifierIsProposer { transition_id: Uuid },
    /// A claim-edge ratification's compare-and-set found `edge_id` no
    /// longer pending, ratified by a commit before it, or not an edge the
    /// tenant holds: the commit wrote nothing, its event included
    /// (postgres-schema.md `claim_edges`; ADR-0021 d5).
    #[error("claim edge {edge_id} is not a pending edge of this tenant")]
    EdgeNotPending { edge_id: Uuid },
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
    /// A `keep_an_admin` change or removal would leave the tenant no admin
    /// entry; the store read the admin entries under lock in the commit's
    /// transaction, and the commit wrote nothing (ADR-0045 d10; ADR-0070
    /// d5).
    #[error("the entry is the tenant's last admin entry")]
    LastAdminEntry,
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
#[allow(
    clippy::large_enum_variant,
    reason = "MemoryHeld carries the whole record, 8 bytes past the lint's bound since the record names its approval (ADR-0077 d6); a commit builds one or two folds and moves them"
)]
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
    /// `workarea.mode_set`: the node's `mode` is now `in_progress` or
    /// `standard` (ADR-0030 d1); the store refuses a node it does not hold.
    ///
    /// `held_free`, on a commit, names the subtree's nodes: the store refuses
    /// the commit with [`StoreError::HeldRemaining`] when any version on
    /// them is held, tested in the committing transaction (d6).
    WorkareaModeSet {
        workarea_id: Uuid,
        in_progress: bool,
        held_free: Vec<Uuid>,
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
    ///
    /// `findings` is the version's findings list (ADR-0074 d23), stored in
    /// the same commit under this commit's seq, in order; a boxed slice, so
    /// the variant stays within a pointer of the record it carries.
    MemoryHeld {
        memory: MemoryRecord,
        enqueue: bool,
        findings: Box<[VersionFinding]>,
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
    /// (postgres-schema.md `memories_served_tip`; ADR-0025 d10). `approval`
    /// is the approving verdict the event's `ratification_event_id` names,
    /// stored on the row with the promotion (ADR-0077 d6). The application
    /// always sends one. A store whose schema keeps the approval refuses
    /// `None` with [`StoreError::ApprovalMissing`]; one over a schema
    /// before migration 0044 takes it and writes none, as the migration
    /// laws send it.
    MemoryPromoted {
        memory_id: Uuid,
        approval: Option<Box<crate::types::Approval>>,
    },
    MemoryRejected {
        memory_id: Uuid,
    },
    /// `source.checked`: the version's latest source check, set on its row
    /// (ADR-0057 d5, d18). The application admits only a promoted, anchored
    /// served tip; the store writes the check and nothing else.
    SourceChecked {
        memory_id: Uuid,
        check: SourceCheck,
    },
    /// `scan.findings_recorded`: each held version named takes
    /// `scanned_under` as the rule set it was last scanned under, and keeps
    /// any finding it had, else takes the one the rescan found; its findings
    /// list gains each finding the rescan made that it does not hold, by
    /// class, rule and field, under this commit's seq; a version no longer
    /// held is left as it is (ADR-0065 d13, d15; ADR-0074 d23, d28).
    FindingsRecorded {
        scanned_under: Option<String>,
        versions: Vec<RecordedVersion>,
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
        /// The end the ratification gives the replaced version: the
        /// successor's declared start, when it begins inside the replaced
        /// version's interval; `None` changes no end (proposed ADR-0059 d4).
        replaced_valid_until: Option<Box<oml_domain::valid_time::ValidEnd>>,
        /// The approving verdict that ratified the successor, stored on its
        /// row with the promotion; `None` as on [`Self::MemoryPromoted`]
        /// (ADR-0077 d6).
        approval: Option<Box<crate::types::Approval>>,
    },
    /// `web.email_allowed`: the entry's row, with no pinned subject. The
    /// store refuses an email another tenant's entry holds with
    /// [`StoreError::EmailTaken`] (ADR-0045 d2).
    WebEmailAllowed(WebEntry),
    /// `web.email_changed`: the entry's user and admin flag, its pinned
    /// subjects kept, and every session of the entry ended in the same
    /// commit (ADR-0045 d2, d5).
    WebEmailChanged {
        entry: WebEntry,
        /// Refuse, with [`StoreError::LastAdminEntry`], un-flagging the
        /// tenant's last admin entry (ADR-0070 d5).
        keep_an_admin: bool,
    },
    /// `web.email_removed`: the entry's row and every session of it, gone
    /// in the same commit; the store refuses an entry it does not hold
    /// (ADR-0045 d2, d13).
    WebEmailRemoved {
        email_hash: Sha256Hash,
        /// Refuse, with [`StoreError::LastAdminEntry`], removing the
        /// tenant's last admin entry (ADR-0070 d5).
        keep_an_admin: bool,
    },
    /// `memory.admin_read`: an admin key read memories' text; nothing
    /// changes but the journal (ADR-0045 d21).
    AdminRead,
    /// `web.quota_set`: one person's override of one quota, or with `None`
    /// its return to the deployment's default (ADR-0068 d5).
    QuotaSet {
        user_root: Uuid,
        quota: oml_domain::signup::Quota,
        limit: Option<u64>,
    },
    /// `web.user_disabled`: every entry naming `user` is disabled at the
    /// commit's time and their sessions end in the same commit (ADR-0068
    /// d8). The key cut-offs are their own `key.denied` commits.
    UserDisabled {
        user: Name,
    },
    /// `web.deletion_requested`: the person's pending request, at the
    /// commit's time; a second request while one is pending changes nothing
    /// (ADR-0068 d6).
    DeletionRequested {
        user: Name,
    },
    /// `web.deletion_closed`: the pending request closes at the commit's
    /// time; its row is kept (ADR-0068 d6).
    DeletionClosed {
        user: Name,
    },
    /// `workarea.status_proposed`: the proposal's row, `pending` with no
    /// ratifier, its `proposed_seq` this commit's seq. The store refuses a
    /// node or proposing key its tenant does not hold, a proposer principal
    /// other than the one the proposing key's row holds, a step other than
    /// one rung forward, and a record that is not pending (postgres-schema.md
    /// `workarea_transitions`; ADR-0009 d3; ADR-0010 R2). Whether the step
    /// may be proposed is the ladder's, decided before the commit.
    TransitionProposed(TransitionRecord),
    /// `workarea.status_transition`: the pending row `transition_id` on
    /// `workarea_id` becomes `ratified` by `ratifier_key_id`, its
    /// `ratified_seq` this commit's seq, and the node's status becomes `to`
    /// in the same transaction. The store re-checks the proposal inside
    /// that transaction, under the chain lock with the node and the row
    /// locked, so a child created or a subtree reparented in since the
    /// proposal is seen: a row already ratified or whose `from` is no
    /// longer the node's status is [`StoreError::TransitionStale`]; a
    /// ratifier that is the proposing key or holds the proposer's
    /// principal, read from its key row, is
    /// [`StoreError::RatifierIsProposer`]; a step to `validated` with a
    /// direct child not `validated` is [`StoreError::ChildrenNotValidated`].
    /// A row that is not a proposal on this node with this `to`, a ratifier
    /// key the tenant does not hold, and a universe root moved off
    /// `committed` are defects. Every refusal refuses the commit, which
    /// writes nothing, its event included (ADR-0009 d1, d3, d4; ADR-0010
    /// R2; workareas.md, ratification re-checks).
    TransitionRatified {
        transition_id: Uuid,
        workarea_id: Uuid,
        to: WorkareaStatus,
        ratifier_key_id: Uuid,
    },
    /// `claim_edge.proposed`: the edge row, pending with no ratifier. The
    /// store refuses an endpoint or a proposer key its tenant does not hold,
    /// an edge id it holds already, an edge that is not pending, and a
    /// `created_at` that is not the event's `at`.
    ///
    /// A pending edge enters the lineage closure (audit-and-lineage.md, the
    /// derivation graph: pending and ratified alike; ADR-0001 d2), and the
    /// closure is flattened onto `lineage_principals` and `lineage_key_ids`,
    /// which the review queue and the verdict read; so this fold widens them
    /// in the same transaction. The reading: let C be the versions connected
    /// to either endpoint through claim edges, the new one included, since
    /// `vocab_edges::lineage_artifacts` follows a claim edge from either end
    /// and no rejected edge is stored in this build. Every version in C, and
    /// every version that reaches one in C through `derives_from` ancestry
    /// and claim edges, gains the union of C's flattened principals and key
    /// ids. The proposer enters no closure: R2 against the proposer is the
    /// ratification's check (ADR-0012; errors.md
    /// `review.ratifier_is_proposer`).
    ClaimEdgeProposed(ClaimEdgeRecord),
    /// `review.ratify` with `subject_kind: "claim_edge"`: the edge turns
    /// ratified and names `ratifier_key_id` (event-catalog.md, the
    /// claim-edge family). The store compare-and-sets that the edge is
    /// still a pending edge of the commit's tenant; on a lost
    /// compare-and-set it writes nothing, the event included, and refuses
    /// with [`StoreError::EdgeNotPending`], as [`Self::TipSuperseded`]
    /// refuses with [`StoreError::TipMoved`]. It refuses a ratifier key the
    /// tenant does not hold and one equal to the proposer's
    /// (`claim_edges_ratifier_is_not_proposer`); R2 against the proposer's
    /// principal and the endpoints' closures is the use case's (ADR-0012).
    /// The edge is already in every closure it joins, pending, so the fold
    /// widens none (audit-and-lineage.md, pending and ratified alike).
    ClaimEdgeRatified {
        edge_id: Uuid,
        ratifier_key_id: Uuid,
    },
    /// `brief_edge.proposed`: the edge row, pending with no ratifier, its
    /// `created_at` the event's `at`. The store refuses a claim, workarea or
    /// proposer key its tenant does not hold, an id it holds already, and
    /// an edge that is not pending. It widens no lineage closure
    /// (postgres-schema.md `brief_edges`; ADR-0046 d7).
    BriefEdgeProposed(BriefEdgeRecord),
    /// `review.ratify` with `subject_kind: "brief_edge"`: the edge turns
    /// ratified, names `ratifier_key_id` and takes this commit's seq as its
    /// `ratified_seq`. A lost compare-and-set on the pending state writes
    /// nothing, the event included, and is [`StoreError::EdgeNotPending`];
    /// a ratifier equal to the proposer is the store's refusal too.
    BriefEdgeRatified {
        edge_id: Uuid,
        ratifier_key_id: Uuid,
    },
    /// `search.default_set`: the tenant's default search pipeline, by name,
    /// or `None` to follow the deployment's again (ADR-0049 d4).
    SearchDefaultSet {
        pipeline: Option<String>,
    },
    /// `telemetry.settings_set`: the instant the tenant's events began to
    /// keep their query text, or `None` to stop, which nulls the text of the
    /// tenant's stored events in the same commit (ADR-0054 d3).
    TelemetrySettingsSet {
        query_text_since: Option<EventTime>,
    },
    /// `federation.policy_set`: the tenant's federation policy, whole
    /// (ADR-0072 d5).
    FederationPolicySet(crate::federation::FederationPolicy),
    /// `workarea.federation_set`: the node's `accept_federated`; the store
    /// refuses a node it does not hold (ADR-0072 d5).
    WorkareaFederationSet {
        workarea_id: Uuid,
        accept_federated: bool,
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
/// request still commits once; these use-cases never answer
/// `idempotency.in_progress`. The web console's acts, which commit in more
/// than one transaction, claim instead ([`JournalStore::idempotency_claim`]).
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

/// What [`JournalStore::idempotency_claim`] found under a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyClaim {
    /// No record and no claim held the scope; this call holds it now.
    Claimed,
    /// An act's outcome is recorded under the scope.
    Recorded(IdempotencyRecord),
    /// Another attempt claimed the scope and has recorded nothing yet: it
    /// is still acting, or it died between its act and its record.
    InProgress { request_digest: String },
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

    /// The near-copy check's candidates (ADR-0050 d1, d17): the versions
    /// holding any of `keys` among `scope`'s promoted served tips and the
    /// held rows of its own keys, by how many keys each holds, then by id
    /// ascending, at most `limit`. Of the kinds `memory`, `derived_fact` and
    /// `steering` alone, never a retirement version. It ranks and never
    /// decides: the core reads each candidate and admits it as recall does
    /// before it names one.
    fn similar_candidates(
        &self,
        tenant_id: Uuid,
        scope: &CandidateScope,
        keys: &[i64],
        limit: usize,
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send;

    /// The tenant's postings health for doctor (ADR-0052 d12), or `None`
    /// from an adapter that keeps no postings.
    fn postings_health(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<PostingsHealth>, StoreError>> + Send {
        let _ = tenant_id;
        async { Ok(None) }
    }

    /// The tenant's served versions that declare valid time, and those
    /// ended at or before `now_us`, for doctor (proposed ADR-0059 d12);
    /// `None` from an adapter that keeps none.
    fn valid_time_counts(
        &self,
        tenant_id: Uuid,
        now_us: i64,
    ) -> impl Future<Output = Result<Option<crate::search::ValidTimeCounts>, StoreError>> + Send
    {
        let _ = (tenant_id, now_us);
        async { Ok(None) }
    }

    /// The tenants the encoder's pass has work for, in one read: those whose
    /// queue holds an entry due now, and those whose sparse state is not on
    /// yet or whose backfill has not finished (ADR-0052 d15); none from an
    /// adapter that keeps no learned sparse postings.
    fn sparse_tenants(&self) -> impl Future<Output = Result<Vec<SparseTenant>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    /// Turns the tenant's sparse state on, so its writes are queued from
    /// now, and queues up to `limit` more of the versions written before,
    /// from where the last step stopped; returns how many it queued and
    /// whether the backfill is complete (ADR-0052 d15).
    fn sparse_backfill(
        &self,
        tenant_id: Uuid,
        limit: usize,
    ) -> impl Future<Output = Result<(u64, bool), StoreError>> + Send {
        let _ = (tenant_id, limit);
        async { Ok((0, true)) }
    }

    /// Turns every tenant's sparse state off and empties the queues: what a
    /// lead serving no sparse pipeline does at start, so nothing is queued
    /// for an encoder no one runs (ADR-0052 d15). Returns the tenants turned
    /// off.
    fn sparse_off(&self) -> impl Future<Output = Result<u64, StoreError>> + Send {
        async { Ok(0) }
    }

    /// Up to `limit` of the tenant's versions due for the sparse encoder,
    /// oldest first, each with its claim, leased for a while so another
    /// lead's pass skips them (ADR-0052 d15).
    fn sparse_pending(
        &self,
        tenant_id: Uuid,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<SparsePending>, StoreError>> + Send {
        let _ = (tenant_id, limit);
        async { Ok(Vec::new()) }
    }

    /// Writes one version's kind 2 postings with the version's current
    /// state and takes it off the queue, in one transaction; `false`, and
    /// nothing written, when its entry is gone or was enqueued again since
    /// it was read (ADR-0052 d15).
    fn sparse_write(
        &self,
        tenant_id: Uuid,
        pending: &SparsePending,
        postings: &[SparsePosting],
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        let _ = (tenant_id, pending, postings);
        async { Ok(false) }
    }

    /// Keeps a version on the queue after a failed encode, counting the
    /// attempt, naming the reason and putting the next try off; at the
    /// attempts' ceiling it parks the entry, a dead letter kept for doctor
    /// and never taken again. Returns whether it parked (ADR-0052 d15).
    fn sparse_failed(
        &self,
        tenant_id: Uuid,
        pending: &SparsePending,
        reason: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        let _ = (tenant_id, pending, reason);
        async { Ok(false) }
    }

    /// The entries waiting across every tenant, parked ones aside, in one
    /// read: the encoder's backlog gauge (ADR-0052 d15).
    fn sparse_backlog(&self) -> impl Future<Output = Result<u64, StoreError>> + Send {
        async { Ok(0) }
    }
}

/// The journal-and-metadata store port (workspace-plan.md row 1). It is a
/// [`Retriever`] too: the keyword retrievers read the same PostgreSQL rows
/// (ADR-0013 d1), so the core reaches both through one handle; a retriever
/// on another store, as an embedder's index would be, gets its own (ADR-0049
/// d2).
pub trait JournalStore: Retriever + Send + Sync {
    /// What the person whose root is `user_root` holds against their
    /// quotas: distinct memories in the root and below, live bearer and
    /// connector keys naming the root, and open connections of those keys
    /// (ADR-0068 d5).
    fn user_usage(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> impl Future<Output = Result<crate::types::UserUsage, StoreError>> + Send;

    /// The person's quota overrides an admin set, each replacing its
    /// default (ADR-0068 d5).
    fn user_quota_overrides(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> impl Future<Output = Result<Vec<(oml_domain::signup::Quota, u64)>, StoreError>> + Send;

    /// The tenant's journalled default search pipeline, by name, or `None`
    /// when it follows the deployment's (ADR-0049 d4).
    fn tenant_search_default(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

    /// Since when the tenant's recall events keep their query text, folded
    /// from `telemetry.settings_set`; `None` is off (ADR-0054 d3).
    fn tenant_query_text_since(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<EventTime>, StoreError>> + Send;

    /// What a recall naming neither a pipeline nor `sources` reads, in one
    /// read: the tenant's default pipeline and its federation policy
    /// (ADR-0049 d4; ADR-0072 d5, d12). A store may answer it with the two
    /// reads below; the PostgreSQL store reads one row.
    fn recall_defaults(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<
        Output = Result<(Option<String>, crate::federation::FederationPolicy), StoreError>,
    > + Send {
        async move {
            let pipeline = self.tenant_search_default(tenant_id).await?;
            let policy = self.federation_policy(tenant_id).await?;
            Ok((pipeline, policy))
        }
    }

    /// The tenant's federation policy, folded from `federation.policy_set`;
    /// the default, federation off, when none was set (ADR-0072 d5).
    fn federation_policy(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<crate::federation::FederationPolicy, StoreError>> + Send;

    /// Of `nodes`, those whose `accept_federated` is off, folded from
    /// `workarea.federation_set`; every node accepts by default
    /// (ADR-0072 d5).
    fn federation_declining(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send;

    /// The reference set's exact counts (ADR-0072 d4.2, d12): per node of
    /// `scope.nodes`, its served versions of `scope.kinds` and how many hold
    /// each of the query's kept words, less every version whose valid time
    /// ended at or before `scope.expired_at`. Read from the per-workarea
    /// counts the postings keep, never from an unreadable node, since the
    /// core names the nodes.
    fn reference_counts(
        &self,
        tenant_id: Uuid,
        scope: &crate::federation::CountsScope,
        query: &str,
    ) -> impl Future<Output = Result<crate::federation::ReferenceCounts, StoreError>> + Send;

    /// Every form's counts over `scope.nodes`, whatever the query: what
    /// [`Self::reference_counts`] reads for a query's forms, read for all of
    /// them at once, `scope.prunes` aside. The widen hint's signal is built
    /// from it off the request path (ADR-0072 d8, as built by #244). `None`
    /// once more than `max_pairs` (node, form) counts are read: the read
    /// stops there and holds no more.
    fn vocabulary_counts(
        &self,
        tenant_id: Uuid,
        scope: &crate::federation::CountsScope,
        max_pairs: usize,
    ) -> impl Future<Output = Result<Option<crate::federation::VocabularyCounts>, StoreError>> + Send;

    /// What [`Self::reference_counts`] answers over the nodes and the state
    /// `vocabulary` was read from, taken out of it: one read of the query's
    /// forms and none of any count. The pruned words are decided over the
    /// vocabulary's sums when `prunes`, as the retriever decides them.
    fn counts_from_vocabulary(
        &self,
        vocabulary: &crate::federation::VocabularyCounts,
        query: &str,
        prunes: bool,
    ) -> impl Future<Output = Result<crate::federation::ReferenceCounts, StoreError>> + Send;

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

    /// [`Self::steering_in_force`] at the instant `as_of` (ADR-0059 d15,
    /// d18): of kind `steering` on `nodes`, each claim's tip at `as_of`,
    /// the version of greatest `version` among its versions whose
    /// `promoted_at` is at or before it, served only when it is promoted
    /// today and no retirement version; ordered as `steering_in_force`
    /// orders, by node depth, then `promoted_at`, then `artifact_id`.
    fn steering_as_of(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        as_of: &EventTime,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send;

    /// Which of `memory_ids` were their claim's tip at `as_of`: promoted
    /// at or before it, with no later version of the claim (one
    /// `workarea_id` and `claim_key`) promoted at or before it, in any
    /// state today (ADR-0059 d15, d16). The core's re-check of every
    /// promoted row an as-of read serves; an id no row has is absent.
    fn tips_as_of(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
        as_of: &EventTime,
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send;

    /// The guardrail rows on `nodes` (proposed ADR-0037 d3, d4): every
    /// promoted served tip of kind `guardrail` whose workarea is one of them,
    /// never a retirement version, and every held version of kind
    /// `guardrail` on them that `author_key_id` authored, never another
    /// key's. Unordered: the read orders the set (d4). The state and the
    /// author predicate are the store's own, as for the own held rows of
    /// [`Retriever::retrieve`].
    fn guardrail_rows(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        author_key_id: Uuid,
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

    /// How many of the tenant's versions, erased ones aside, were last
    /// scanned under a rule set other than `current`, the held ones and
    /// every one, and how many held versions carry a `scan_finding` with
    /// no row in the findings list. Doctor's `scan_held_unrescanned`,
    /// `scan_rescan_due` and `scan_findings_unlisted` (ADR-0074 d23, d28).
    fn scan_backlog(
        &self,
        tenant_id: Uuid,
        current: Option<&str>,
    ) -> impl Future<Output = Result<ScanBacklog, StoreError>> + Send;

    /// The findings lists of the named versions (ADR-0074 d23), each in the
    /// order recorded: by the recording event's seq, then the order that
    /// event gave. A version with no finding, or not the tenant's, is absent.
    fn scan_findings(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
    ) -> impl Future<Output = Result<Vec<(Uuid, VersionFinding)>, StoreError>> + Send;

    /// How many versions on `nodes` are in state `held`, dead-lettered ones
    /// included: the commit's precondition and doctor's count (ADR-0030 d6,
    /// d14).
    fn held_count(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;

    /// How many promoted served tips on `workarea_id` itself read `stale`:
    /// their latest source check found the source `changed` or `removed`
    /// (ADR-0057 d5, d17). A held version is never counted.
    fn stale_count(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;

    /// The greatest `version` of `claim_key` in the workarea that a key of
    /// `own_key_ids` may read as the claim's: its own held and promoted
    /// versions, and with `reads_promoted` (reader effective access on the
    /// workarea) every promoted one; never another author's held version,
    /// so a supersession's check tells its caller nothing it cannot read
    /// (ADR-0057 d19; R3).
    fn claim_latest_readable(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
        own_key_ids: &[Uuid],
        reads_promoted: bool,
    ) -> impl Future<Output = Result<Option<u32>, StoreError>> + Send;

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

    /// The held listing (ADR-0055 d2): the queue entries
    /// [`JournalStore::review_claim_next`] would serve this reviewer — on
    /// `filter.nodes`, not claimed by another key, outside the reviewer's
    /// exclusion set — that pass the filter's author and `claim_key`
    /// prefix, oldest first (`held_since`, then the memory id), strictly
    /// after `after` in that order, at most `limit`. It claims nothing. An
    /// empty node set serves nothing.
    fn review_held(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        filter: &HeldFilter,
        after: Option<&ListingPosition>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<HeldRow>, StoreError>> + Send;

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

    /// The status-ladder proposal `transition_id`, pending or ratified;
    /// `None` for one this tenant does not hold (ADR-0009 d3).
    fn transition(
        &self,
        tenant_id: Uuid,
        transition_id: Uuid,
    ) -> impl Future<Output = Result<Option<TransitionRecord>, StoreError>> + Send;

    /// Every `pending` proposal on the node, in the order they were
    /// proposed (`proposed_seq`); empty for a node this tenant does not
    /// hold. A proposal another's ratification made stale is still listed
    /// here, as its row is still pending: the status read drops the ones
    /// whose `from` is no longer the node's status (workareas.md, the status
    /// read lists only live proposals).
    fn pending_transitions(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Vec<TransitionRecord>, StoreError>> + Send;

    fn idempotency_get(
        &self,
        scope: &IdempotencyScope,
    ) -> impl Future<Output = Result<Option<IdempotencyRecord>, StoreError>> + Send;

    /// Claims `scope` for an act that commits in more than one transaction,
    /// before its first, in a transaction of its own:
    /// [`IdempotencyClaim::Claimed`] when no record or claim held it, which
    /// this call now holds; otherwise the record or the claim that holds it,
    /// left as it is. Of two concurrent claims under one scope exactly one
    /// is `Claimed`
    /// (http-api.md idempotency; ADR-0070 d3 layer 4, d5, the web
    /// console's acts; #233).
    ///
    /// `claim_id` names this attempt's claim, so only the attempt that took
    /// it settles or releases it: a late release of an attempt whose claim
    /// an operator removed never drops a later attempt's claim.
    fn idempotency_claim(
        &self,
        scope: &IdempotencyScope,
        request_digest: &str,
        claim_id: Uuid,
    ) -> impl Future<Output = Result<IdempotencyClaim, StoreError>> + Send;

    /// Records the act's outcome into the claim `claim_id` that
    /// [`Self::idempotency_claim`] took under `put.scope` and the same
    /// digest: `false`, writing nothing, when that claim is not held
    /// (ADR-0070 d5; #233).
    fn idempotency_settle(
        &self,
        put: IdempotencyPut,
        claim_id: Uuid,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Drops the claim `claim_id` under `scope` of an act that was refused
    /// before anything committed, so a
    /// resubmission acts again, as a refused request records nothing
    /// (http-api.md idempotency); a recorded outcome is never dropped
    /// (ADR-0070 d5; #233).
    fn idempotency_release(
        &self,
        scope: &IdempotencyScope,
        claim_id: Uuid,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// One claim edge of the tenant, pending or ratified; `None` for an edge
    /// it does not hold (postgres-schema.md `claim_edges`).
    fn claim_edge(
        &self,
        tenant_id: Uuid,
        edge_id: Uuid,
    ) -> impl Future<Output = Result<Option<ClaimEdgeRecord>, StoreError>> + Send;

    /// One page of the tenant's claim edges, pending and ratified, that
    /// `selection` selects — `artifact_id` matching either endpoint,
    /// `within` holding either endpoint's workarea, and both endpoints rows
    /// whose workarea is in `readable_nodes` and which are promoted or
    /// authored by `reader_key_id` — newest first, `created_at` descending
    /// then `edge_id` descending, strictly after `after`, at most `limit` of
    /// them (http-api.md pagination and claim edges). An edge with an
    /// endpoint the tenant does not hold is absent.
    fn claim_edges(
        &self,
        tenant_id: Uuid,
        selection: &ClaimEdgeSelection,
        after: Option<&ClaimEdgePosition>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<ClaimEdgeRecord>, StoreError>> + Send;

    /// The tenant's ratified claim edges labelled `label` whose endpoints
    /// are both of kind `term`, each as its (source, target) endpoints'
    /// `claim_key`s, a term's CURIE (ontologies.md), in that pair's byte
    /// order ascending; a pending edge, an edge of another label and an
    /// edge with any other kind at an end are absent. The read behind the
    /// ADR-0021 d5 cycle refusal: the application parses each with
    /// `Curie::parse` and builds the `TermEdge`s it checks the candidate
    /// against.
    ///
    /// The use case reads the chain head before it calls this, and seals its
    /// ratification on that head. A commit that changes these edges between
    /// the read and the ratification's commit moves the head, so the head
    /// compare-and-set refuses the ratification with
    /// [`StoreError::SeqConflict`] and the transport's head retry re-reads
    /// the edges: the cycle check is a transaction-time check with no lock
    /// of its own (ADR-0021 d5; ADR-0002, one chain head).
    /// One brief edge of the tenant by id, pending or ratified.
    fn brief_edge(
        &self,
        tenant_id: Uuid,
        edge_id: Uuid,
    ) -> impl Future<Output = Result<Option<BriefEdgeRecord>, StoreError>> + Send;

    /// Every brief edge into `workarea_id`, pending and ratified, newest
    /// first by `created_at`, then by edge id (ADR-0046 d7). Unpaged: the
    /// count per workarea is unbounded, tracked as #145.
    fn brief_edges(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Vec<BriefEdgeRecord>, StoreError>> + Send;

    fn ratified_term_edges(
        &self,
        tenant_id: Uuid,
        label: &str,
    ) -> impl Future<Output = Result<Vec<(String, String)>, StoreError>> + Send;
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

    /// Writes a session row with the digests of its capacity credentials,
    /// each `(capacity, SHA-256 of the credential)` (ADR-0070 d2), and ends
    /// the row `replaces` names in the same transaction, so a browser that
    /// arrives with an old session leaves with only the new one
    /// (ADR-0045 d5). The credentials end with the session.
    fn web_session_open(
        &self,
        session: WebSessionRecord,
        replaces: Option<Sha256Hash>,
        credentials: Vec<(&'static str, Sha256Hash)>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The live session row a capacity credential's digest names, and the
    /// capacity, before any tenant is known; `None` for a digest no row
    /// holds, which includes every credential of an ended session
    /// (ADR-0070 d2).
    fn web_session_credential(
        &self,
        credential_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<(WebSessionRecord, String)>, StoreError>> + Send;

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

    /// The tenant's pending deletion requests, oldest first (ADR-0068 d6).
    fn web_deletion_requests(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Vec<crate::types::DeletionRequest>, StoreError>> + Send;

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

/// One finding as the scanner reports it (ADR-0065 d13): the class, the
/// rule, whether it vetoes, and where in the scanned text it fired — never
/// what it matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanFinding {
    pub class: String,
    pub rule: String,
    pub veto: bool,
    /// The field and line inside the scanned text, `/body line 3`.
    pub at: String,
    /// Where in the scanned text, for a scanner that says (ADR-0074 d23);
    /// `None` for one that reports only its outcome.
    pub place: Option<FindingPlace>,
}

/// Where a finding fired inside the text the scanner was handed: the line,
/// the view its rule read and the match's byte range in that view
/// (ADR-0074 d6, d23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingPlace {
    pub line: u32,
    pub via: String,
    pub span: (u64, u64),
}

/// One held version a recording rescan read (ADR-0074 d28): the summary
/// its `scan_finding` takes when it has none, and every finding the rescan
/// made, for its findings list (d23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedVersion {
    pub memory_id: Uuid,
    pub summary: Option<String>,
    pub findings: Vec<VersionFinding>,
}

/// The tenant's rescan backlog (ADR-0074 d23, d28): held versions scanned
/// under another rule set, every version so scanned, and held versions
/// whose stored `scan_finding` has no row in the findings list, which an
/// older server wrote while migration 0043 ran.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanBacklog {
    pub held_unrescanned: u64,
    pub rescan_due: u64,
    pub findings_unlisted: u64,
}

/// What a scanner says about itself, for `/readyz` and doctor's scan line
/// (ADR-0065 d11): its version, how many rules it loaded, and whether the
/// personal-data rules are on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannerInfo {
    pub version: &'static str,
    pub rules: usize,
    pub pii: bool,
}

/// The scanner port: deterministic, synchronous, in-process (admission.md).
pub trait Scanner: Send + Sync {
    fn scan(&self, envelope: &AdmissionEnvelope<'_>) -> ScanOutcome;

    /// The rule-set version every verdict is journalled under, the
    /// submission event's `scan.scanner_version` (ADR-0065 d1; ADR-0019 d4);
    /// `None` for a scanner that pins none, as the test doubles do.
    fn version(&self) -> Option<&'static str> {
        None
    }

    /// Every finding in the envelope's text, counting nothing (ADR-0065
    /// d13). A scanner that reports only its outcome answers one finding
    /// whose class and rule are its reason.
    fn findings(&self, envelope: &AdmissionEnvelope<'_>) -> Vec<ScanFinding> {
        outcome_findings(&self.scan(envelope))
    }

    /// Admission's scan (ADR-0074 d23): the verdict, counted as
    /// [`Scanner::scan`] counts it, and every finding behind it, so the
    /// version stores each one rather than the reason's first. A scanner
    /// that reports only its outcome answers one finding whose class and
    /// rule are its reason.
    fn assess(&self, envelope: &AdmissionEnvelope<'_>) -> (ScanOutcome, Vec<ScanFinding>) {
        let outcome = self.scan(envelope);
        let findings = outcome_findings(&outcome);
        (outcome, findings)
    }

    /// The scanner's version, rule count and personal-data setting, or
    /// `None` for one that pins no rule set.
    fn info(&self) -> Option<ScannerInfo> {
        None
    }
}

/// The one finding a scanner that reports only its outcome makes.
fn outcome_findings(outcome: &ScanOutcome) -> Vec<ScanFinding> {
    let (reason, veto) = match outcome {
        ScanOutcome::Pass => return Vec::new(),
        ScanOutcome::Hold(reason) => (reason, false),
        ScanOutcome::Veto(reason) => (reason, true),
    };
    vec![ScanFinding {
        class: reason.clone(),
        rule: reason.clone(),
        veto,
        at: String::new(),
        place: None,
    }]
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

    /// A recall ran `pipeline`, `fallback` says whether `match: all` fell
    /// back to any term, and `access` how its lists were read, `postings` or
    /// `scan` (ADR-0049 d4, d5; ADR-0052 d11). Telling no one is the
    /// default.
    fn search(&self, pipeline: &'static str, fallback: bool, access: &'static str) {
        let _ = (pipeline, fallback, access);
    }

    /// A recall's reranking stage ended `outcome` (`applied`, `skipped`, or
    /// `None` when its pipeline does not rerank), with `reason` when skipped
    /// (ADR-0053 d9). Telling no one is the default.
    fn rerank(&self, outcome: Option<&'static str>, reason: Option<&'static str>) {
        let _ = (outcome, reason);
    }

    /// A recall through `pipeline` answered `result` (`hits`, `empty`, or
    /// `below_floor` when the floor withheld a row) in `seconds`
    /// (ADR-0054 d1). Telling no one is the default.
    fn recall_answered(&self, pipeline: &'static str, result: &'static str, seconds: f64) {
        let _ = (pipeline, result, seconds);
    }

    /// The `origin.kind` of each hit a recall served, `none` for a version
    /// written before kinds (ADR-0063 d13). Telling no one is the default.
    fn recall_hits_by_origin(&self, kinds: &[&'static str]) {
        let _ = kinds;
    }

    /// A write's similar check named rows of `relation` at best (`none`,
    /// `same_text` or `near_copy`) under `on_similar` (`hold` or
    /// `refuse`), in `seconds` (ADR-0050 d1; ADR-0054 d1). Telling no one
    /// is the default.
    fn similar_checked(&self, relation: &'static str, on_similar: &'static str, seconds: f64) {
        let _ = (relation, on_similar, seconds);
    }

    /// A recall through `pipeline` spent `seconds` in `stage`, one of
    /// [`crate::search::Stage`]'s names (ADR-0049 d14). Telling no one is
    /// the default.
    fn recall_stage(&self, pipeline: &'static str, stage: &'static str, seconds: f64) {
        let _ = (pipeline, stage, seconds);
    }

    /// A verdict `approve` or `reject` committed `waited_seconds` after its
    /// memory was submitted (ADR-0054 d1, d12). Telling no one is the
    /// default.
    fn verdict(&self, verdict: &'static str, waited_seconds: f64) {
        let _ = (verdict, waited_seconds);
    }

    /// A recall served `n` hits labelled `unreviewed` (ADR-0030 d5).
    fn unreviewed_hits(&self, n: u64) {
        let _ = n;
    }

    /// A person's key set a workarea's mode (ADR-0030 d5).
    fn mode_set(&self, mode: &'static str) {
        let _ = mode;
    }

    /// A recall that searched beyond its scope, under `sources` mode `wide`
    /// or `list` (ADR-0072 Consequences).
    fn recall_sources(&self, mode: &'static str) {
        let _ = mode;
    }

    /// A recall served `n` hits from federated sources (ADR-0072
    /// Consequences).
    fn federated_hits(&self, n: u64) {
        let _ = n;
    }

    /// A curator key committed an act: `reject`, `retire`, `flag` or
    /// `write` (ADR-0030 d5, d9).
    fn curation(&self, act: &'static str) {
        let _ = act;
    }
}

/// The telemetry writer's counts and gauges, which the metrics listener
/// serves (ADR-0054 d1, as accepted). Every label is from a closed set.
pub trait TelemetryMeter: Send + Sync {
    /// `n` recall events ended `outcome`: `written`, `dropped_queue_full`,
    /// `dropped_store_error` or `dropped_no_partition`.
    fn events(&self, outcome: &'static str, n: u64);
    /// `n` labels `label` were stored.
    fn labels(&self, label: &'static str, n: u64);
    /// `n` labels were dropped for `reason`: `rate_limited`, `queue_full`
    /// or `store_error`.
    fn labels_dropped(&self, reason: &'static str, n: u64);
    /// A remembered memory was (`same_text`) or was not (`none`) an exact
    /// duplicate of a served memory of its workarea.
    fn remembered(&self, relation: &'static str);
    /// The queue's depth now.
    fn queue_depth(&self, depth: i64);
    /// The telemetry's health, refreshed by the labeller's pass: tenants
    /// holding rows past the 30 days and one pass, so the purge is behind.
    fn health(&self, purge_overdue: i64);
    /// Seconds since the labeller's last completed pass.
    fn lag(&self, seconds: f64);
    /// The served memories no recall of the last 30 days returned, by how
    /// long ago they were promoted: under 7 days, 30, 90, and older.
    fn unrecalled(&self, by_age: [i64; 4]);
}

/// The observer of a composition that logs no request.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoObserver;

impl RequestObserver for NoObserver {
    fn credential(&self, _key: Option<&KeyRecord>, _connection: Option<Uuid>) {}

    fn workarea(&self, _workarea_id: Uuid) {}
}

/// Why a rerank did not reorder a recall (ADR-0053 d5): the core then serves
/// the first stage's order and says so, never an error to the caller.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RerankError {
    /// Every slot of the concurrency cap stayed taken until the deadline.
    #[error("busy: every rerank slot stayed taken until the deadline")]
    Busy,
    /// The model did not answer before the deadline.
    #[error("timeout: the model did not answer before the deadline")]
    Timeout,
    /// The model failed: a tokenizer or tensor error, named.
    #[error("failed: {0}")]
    Failed(String),
}

impl RerankError {
    /// The reason `search.rerank_reason`, the request line and the metric
    /// spell: a closed set (ADR-0053 d5, d9).
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Timeout => "timeout",
            Self::Failed(_) => "failed",
        }
    }
}

/// What one rerank call returns: a relevance per passage, in order.
pub type RerankFuture<'a> =
    core::pin::Pin<Box<dyn Future<Output = Result<Vec<f32>, RerankError>> + Send + 'a>>;

/// A cross-encoder that scores a query against passages the core hands it
/// (ADR-0053 d2). It reads only those passages, which the core took from
/// rows it would serve, stores nothing and decides no visibility; a score
/// is a relevance in `[0, 1]`, one per passage in the order given, from the
/// query and the passage alone, so no verdict, reviewer or `promoted_at`
/// reaches it (ADR-0001 d3). An adapter enforces its own concurrency cap
/// and deadline and answers [`RerankError::Busy`] or
/// [`RerankError::Timeout`] past them, so a slow model never holds a
/// recall longer than its deadline.
///
/// Unlike the other ports it is held as `Arc<dyn Reranker>`, as the request
/// observer is: a deployment may run without one, and a boxed future per
/// recall costs nothing beside the model's milliseconds.
pub trait Reranker: Send + Sync {
    /// The model's name, as `oml doctor`, `/readyz` and the metrics spell it.
    fn model(&self) -> &str;

    /// Scores each passage against `query`.
    fn score(&self, query: String, passages: Vec<String>) -> RerankFuture<'_>;
}

/// What one document encode returns: per text, its postings or why that
/// text failed, so one bad claim fails alone; the outer error is a failure
/// of the whole call (ADR-0052 d15).
pub type SparseEncoded = Result<Vec<Result<Vec<SparsePosting>, String>>, String>;

/// The future of one document encode.
pub type SparseFuture<'a> = core::pin::Pin<Box<dyn Future<Output = SparseEncoded> + Send + 'a>>;

/// The learned sparse encoder (ADR-0052 d14): a query is its tokenizer's
/// ids weighted by the model's shipped idf table, with no model run and no
/// corpus statistic; a document is the model's weights, written off the
/// commit path by the lead's encoder task (d15). It stores nothing and
/// decides no visibility. Held as `Arc<dyn SparseEncoder>`, as the
/// reranker is: a deployment may run without one.
pub trait SparseEncoder: Send + Sync {
    /// The model's name, as `oml doctor` and `/readyz` spell it.
    fn model(&self) -> &str;

    /// The query's tokens, each once, with its idf weight and word piece.
    fn encode_query(&self, query: &str) -> Vec<SparseTerm>;

    /// Each text's postings, floored and capped, or why it failed
    /// (ADR-0052 d14, d15).
    fn encode_documents(&self, texts: Vec<String>) -> SparseFuture<'_>;
}

// ---------------------------------------------------------------------------
// The format port (ADR-0056 d3)
// ---------------------------------------------------------------------------

/// A decoded string the scan reads beside the source, with the path it came
/// from: a finding on a view names the path, never the value (ADR-0056 d6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanView {
    pub path: String,
    pub text: String,
}

/// What an adapter derived from a source it accepted, handed back to the
/// adapter's other operations.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    /// The text the lexical, postings, sparse and rerank stages read.
    pub search_text: String,
    /// The decoded strings the scan reads beside the source.
    pub views: Vec<ScanView>,
    /// A finding the format's own rule raises (`secret.keyed_value`, ADR-0056
    /// d6): a veto naming the key path, never the value.
    pub veto: Option<String>,
}

/// A refusal of a source by its adapter: a named reason with the line and
/// column it was found at (`submission.format_invalid`, ADR-0056 d13).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}: {message} (line {line}, column {column})")]
pub struct FormatRefusal {
    pub reason: &'static str,
    pub message: String,
    pub line: u32,
    pub column: u32,
}

impl FormatRefusal {
    #[must_use]
    pub fn new(reason: &'static str, message: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            reason,
            message: message.into(),
            line,
            column,
        }
    }
}

/// What a page and a client may do with a source (ADR-0056 d9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderHint {
    /// The source escaped in a monospaced block, nothing rendered.
    Escaped,
    /// The source, and beside it a drawing in a sandboxed frame.
    Diagram,
}

/// One difference of a structural diff (ADR-0056 d3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatChange {
    pub op: &'static str,
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// A structural comparison of two sources of one format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatDiff {
    pub equivalent: bool,
    pub changes: Vec<FormatChange>,
}

/// One adapter: pure and deterministic, with no I/O, clock or randomness
/// (ADR-0056 d3). The adapters are compiled in; no plugin loads at run time.
pub trait Format: Send + Sync {
    /// The adapter and its version, `mermaid/1`, recorded as `search_text_by`.
    fn id(&self) -> &'static str;
    /// Accepts the source or refuses it by name, before the scan.
    fn validate(&self, source: &str) -> Result<Parsed, FormatRefusal>;
    /// The text the search stages read.
    fn search_text(&self, parsed: &Parsed) -> String;
    /// The decoded strings the scan reads beside the source.
    fn scan_views(&self, parsed: &Parsed) -> Vec<ScanView>;
    fn render_hint(&self) -> RenderHint;
    /// Compares two sources the adapter accepts.
    fn diff(&self, old: &str, new: &str) -> Result<FormatDiff, FormatRefusal>;
}

/// A media type the registry resolved: its canonical spelling and adapter.
#[derive(Clone)]
pub struct ResolvedFormat {
    /// The spelling that is signed and stored (`text/vnd.oml.code; lang=rust`).
    pub media_type: String,
    /// Prose is `text/plain` and `text/markdown`: their search text is the
    /// source, and they are the only formats the other kinds take.
    pub prose: bool,
    pub adapter: std::sync::Arc<dyn Format>,
}

impl std::fmt::Debug for ResolvedFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedFormat")
            .field("media_type", &self.media_type)
            .field("adapter", &self.adapter.id())
            .finish()
    }
}

/// The closed list of formats the deployment serves (ADR-0056 d1).
pub trait Formats: Send + Sync {
    /// `Err` carries the accepted values, so a refusal lists them.
    fn resolve(&self, media_type: &str) -> Result<ResolvedFormat, Vec<String>>;
    fn allowed(&self) -> Vec<String>;
    /// Every adapter id with its version, for doctor and `/readyz`.
    fn adapters(&self) -> Vec<&'static str>;
}

/// The identity adapter of `text/plain` and `text/markdown` (ADR-0056 d4): the
/// search text is the source and nothing is derived.
pub struct IdentityFormat;

impl Format for IdentityFormat {
    fn id(&self) -> &'static str {
        "identity/1"
    }
    fn validate(&self, source: &str) -> Result<Parsed, FormatRefusal> {
        Ok(Parsed {
            search_text: source.to_owned(),
            ..Parsed::default()
        })
    }
    fn search_text(&self, parsed: &Parsed) -> String {
        parsed.search_text.clone()
    }
    fn scan_views(&self, _parsed: &Parsed) -> Vec<ScanView> {
        Vec::new()
    }
    fn render_hint(&self) -> RenderHint {
        RenderHint::Escaped
    }
    fn diff(&self, old: &str, new: &str) -> Result<FormatDiff, FormatRefusal> {
        let same = old == new;
        Ok(FormatDiff {
            equivalent: same,
            changes: if same {
                Vec::new()
            } else {
                vec![FormatChange {
                    op: "replace",
                    path: String::new(),
                    before: Some(old.to_owned()),
                    after: Some(new.to_owned()),
                }]
            },
        })
    }
}

/// The two prose media types.
pub const PROSE_FORMATS: [&str; 2] = ["text/plain", "text/markdown"];

/// The registry an `App` has until the composition gives it the compiled-in
/// one: prose only, so a write that names another format is refused naming
/// the accepted values.
pub struct ProseFormats;

impl Formats for ProseFormats {
    fn resolve(&self, media_type: &str) -> Result<ResolvedFormat, Vec<String>> {
        PROSE_FORMATS
            .iter()
            .find(|m| **m == media_type)
            .map(|m| ResolvedFormat {
                media_type: (*m).to_owned(),
                prose: true,
                adapter: std::sync::Arc::new(IdentityFormat),
            })
            .ok_or_else(|| self.allowed())
    }
    fn allowed(&self) -> Vec<String> {
        PROSE_FORMATS.iter().map(|m| (*m).to_owned()).collect()
    }
    fn adapters(&self) -> Vec<&'static str> {
        vec!["identity/1"]
    }
}
