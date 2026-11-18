//! The store port, timed: every call into the Postgres adapter's store
//! ports is one span named by its port method, never by SQL text or bound
//! values, and its wall time and count are summed into the running
//! request's `store_ms` and `store_calls`, pool acquisition and the
//! chain-lock wait included (ADR-0042 d13, d14). Outside a request, the
//! start and the hourly sweep, a call is timed into nothing.
//!
//! Every method forwards to the wrapped store unchanged: the decorator adds
//! an `Instant` and an addition per call and decides nothing.

use std::future::Future;
use std::time::Instant;

use tracing::Instrument as _;
use uuid::Uuid;

use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::oauth::{RefreshDecision, TokenKind};
use oml_app::domain::web::Provider;
use oml_app::domain::{ChainHead, EventTime, Sha256Hash, SignedEnvelope};
use oml_app::ports::{
    CommitReceipt, CommitRequest, ConnectionStore, IdempotencyClaim, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, Retriever, SessionKeyName, StoreError,
    TailScan, WebStore,
};
use oml_app::search::{CandidateScope, RetrievalQuery, Retrieved, RetrieverKind};
use oml_app::types::{
    AdminSearch, AuthorizationRecord, BriefEdgeRecord, ClaimEdgePosition, ClaimEdgeRecord,
    ClaimEdgeSelection, ClaimKeyState, ClientDescription, ConnectCodeRecord, ConnectGrant,
    ConnectOutcome, ConnectionRecord, CrewRecord, DocumentCaching, HeldFilter, HeldRow, KeyAccess,
    KeyRecord, KeyReferences, ListingFilter, ListingPosition, MemoryRecord, OAuthClientRecord,
    PersonReviewer, PinOutcome, QueueDepth, RefreshFacts, RefreshOutcome, ReviewLane, SweepReport,
    TenantRow, TokenPair, TokenRecord, TransitionRecord, UnnamedRows, WebCredentialRows, WebEntry,
    WebSessionRecord, WorkspaceRecord,
};
use oml_transport_http::traffic::store_call;

/// A store whose every port call is timed into the running request.
#[derive(Debug, Clone)]
pub struct Timed<J> {
    inner: J,
}

impl<J> Timed<J> {
    #[must_use]
    pub fn new(inner: J) -> Self {
        Self { inner }
    }

    /// The wrapped store.
    #[must_use]
    pub fn inner(&self) -> &J {
        &self.inner
    }
}

/// `call`, inside a span named by the port method, its wall time summed
/// into the running request (ADR-0042 d13, d14).
fn timed<F: Future>(method: &'static str, call: F) -> impl Future<Output = F::Output> {
    let span = tracing::info_span!("store_port", otel.name = method, oml.store.method = method);
    async move {
        let started = Instant::now();
        let out = call.instrument(span).await;
        store_call(started.elapsed());
        out
    }
}

impl<J: Retriever> Retriever for Timed<J> {
    fn retrievers(&self) -> Vec<RetrieverKind> {
        self.inner.retrievers()
    }

    fn retrieve(
        &self,
        tenant_id: Uuid,
        kind: RetrieverKind,
        scope: &CandidateScope,
        query: RetrievalQuery<'_>,
    ) -> impl Future<Output = Result<Retrieved, StoreError>> + Send {
        timed(
            "retrieve",
            self.inner.retrieve(tenant_id, kind, scope, query),
        )
    }

    fn suggest(
        &self,
        tenant_id: Uuid,
        scope: &CandidateScope,
        words: &[String],
        limit: usize,
    ) -> impl Future<Output = Result<Vec<String>, StoreError>> + Send {
        timed(
            "suggest",
            self.inner.suggest(tenant_id, scope, words, limit),
        )
    }

    fn similar_candidates(
        &self,
        tenant_id: Uuid,
        scope: &CandidateScope,
        keys: &[i64],
        limit: usize,
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send {
        timed(
            "similar_candidates",
            self.inner.similar_candidates(tenant_id, scope, keys, limit),
        )
    }
}

impl<J: JournalStore> JournalStore for Timed<J> {
    fn upload_open(
        &self,
        session: &oml_app::types::UploadSession,
        per_key: u64,
        tenant_max_bytes: u64,
    ) -> impl Future<Output = Result<Result<(), oml_app::domain::upload::UploadRefusal>, StoreError>>
    + Send {
        timed(
            "upload_open",
            self.inner.upload_open(session, per_key, tenant_max_bytes),
        )
    }

    fn upload_session(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
    ) -> impl Future<Output = Result<Option<oml_app::types::UploadSession>, StoreError>> + Send
    {
        timed(
            "upload_session",
            self.inner.upload_session(tenant_id, upload_id),
        )
    }

    fn upload_part_received(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
        n: u64,
        at: &EventTime,
        expires_at: &EventTime,
    ) -> impl Future<Output = Result<Option<oml_app::types::UploadSession>, StoreError>> + Send
    {
        timed(
            "upload_part_received",
            self.inner
                .upload_part_received(tenant_id, upload_id, n, at, expires_at),
        )
    }

    fn upload_transition(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
        from: &[oml_app::domain::upload::UploadState],
        to: oml_app::domain::upload::UploadState,
        at: &EventTime,
        expires_at: Option<&EventTime>,
        shred: bool,
    ) -> impl Future<Output = Result<Option<oml_app::types::UploadSession>, StoreError>> + Send
    {
        timed(
            "upload_transition",
            self.inner
                .upload_transition(tenant_id, upload_id, from, to, at, expires_at, shred),
        )
    }

    fn uploads_expire(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        timed("uploads_expire", self.inner.uploads_expire(tenant_id, at))
    }

    fn uploads_unswept(
        &self,
        tenant_id: Uuid,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send {
        timed(
            "uploads_unswept",
            self.inner.uploads_unswept(tenant_id, limit),
        )
    }

    fn upload_objects_removed(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "upload_objects_removed",
            self.inner.upload_objects_removed(tenant_id, upload_id, at),
        )
    }

    fn upload_stats(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<oml_app::types::UploadStats, StoreError>> + Send {
        timed("upload_stats", self.inner.upload_stats(tenant_id))
    }

    fn import_target_view(
        &self,
        tenant_id: Uuid,
        target_workspace_id: Uuid,
        source_server_id: &str,
        source_tenant_id: &str,
    ) -> impl Future<Output = Result<oml_app::types::ImportTargetView, StoreError>> + Send {
        timed(
            "import_target_view",
            self.inner.import_target_view(
                tenant_id,
                target_workspace_id,
                source_server_id,
                source_tenant_id,
            ),
        )
    }

    fn import_begin(
        &self,
        import: &oml_app::types::ImportRecord,
        at: &EventTime,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        timed("import_begin", self.inner.import_begin(import, at))
    }

    fn import_version_recorded(
        &self,
        tenant_id: Uuid,
        import_id: Uuid,
        version: &oml_app::types::ImportedVersion,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "import_version_recorded",
            self.inner
                .import_version_recorded(tenant_id, import_id, version),
        )
    }

    fn import_failed(
        &self,
        tenant_id: Uuid,
        import_id: Uuid,
        at: &EventTime,
        counts: &std::collections::BTreeMap<String, u64>,
        refusals: &[oml_app::types::ImportRefusal],
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "import_failed",
            self.inner
                .import_failed(tenant_id, import_id, at, counts, refusals),
        )
    }

    fn import_record(
        &self,
        tenant_id: Uuid,
        import_id: Uuid,
    ) -> impl Future<Output = Result<Option<oml_app::types::ImportRecord>, StoreError>> + Send {
        timed(
            "import_record",
            self.inner.import_record(tenant_id, import_id),
        )
    }

    fn import_stats(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<oml_app::types::ImportStats, StoreError>> + Send {
        timed("import_stats", self.inner.import_stats(tenant_id))
    }

    fn export_policy(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<oml_app::domain::export::ExportPolicy, StoreError>> + Send
    {
        timed("export_policy", self.inner.export_policy(tenant_id))
    }

    fn export_allowed(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        timed(
            "export_allowed",
            self.inner.export_allowed(tenant_id, workspace_id, at),
        )
    }

    fn export_versions(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        own_keys: &[Uuid],
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed(
            "export_versions",
            self.inner.export_versions(tenant_id, nodes, own_keys),
        )
    }

    fn exported_bytes(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        timed(
            "exported_bytes",
            self.inner.exported_bytes(tenant_id, user_root, at),
        )
    }

    fn user_usage(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<oml_app::types::UserUsage, StoreError>> + Send {
        timed(
            "user_usage",
            self.inner.user_usage(tenant_id, user_root, at),
        )
    }

    fn account_profile(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        timed(
            "account_profile",
            self.inner.account_profile(tenant_id, user_root),
        )
    }

    fn meter_recalls(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        day: &str,
        count: u64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "meter_recall",
            self.inner.meter_recalls(tenant_id, user_root, day, count),
        )
    }

    fn usage_accounts(
        &self,
        tenant_id: Uuid,
        month: oml_app::domain::usage::Month,
    ) -> impl Future<Output = Result<Vec<oml_app::types::AccountCounters>, StoreError>> + Send {
        timed(
            "usage_accounts",
            self.inner.usage_accounts(tenant_id, month),
        )
    }

    fn usage_ledger(
        &self,
        tenant_id: Uuid,
        month: oml_app::domain::usage::Month,
    ) -> impl Future<Output = Result<Vec<oml_app::types::LedgerRow>, StoreError>> + Send {
        timed("usage_ledger", self.inner.usage_ledger(tenant_id, month))
    }

    fn usage_drift(
        &self,
        tenant_id: Uuid,
        month: oml_app::domain::usage::Month,
    ) -> impl Future<Output = Result<Vec<oml_app::types::MeterDrift>, StoreError>> + Send {
        timed("usage_drift", self.inner.usage_drift(tenant_id, month))
    }

    fn user_quota_overrides(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> impl Future<Output = Result<Vec<(oml_app::domain::signup::Quota, u64)>, StoreError>> + Send
    {
        timed(
            "user_quota_overrides",
            self.inner.user_quota_overrides(tenant_id, user_root),
        )
    }

    fn brief_edge(
        &self,
        tenant_id: Uuid,
        edge_id: Uuid,
    ) -> impl Future<Output = Result<Option<BriefEdgeRecord>, StoreError>> + Send {
        timed("brief_edge", self.inner.brief_edge(tenant_id, edge_id))
    }

    fn brief_edges(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> impl Future<Output = Result<Vec<BriefEdgeRecord>, StoreError>> + Send {
        timed(
            "brief_edges",
            self.inner.brief_edges(tenant_id, workspace_id),
        )
    }

    fn claim_edge(
        &self,
        tenant_id: Uuid,
        edge_id: Uuid,
    ) -> impl Future<Output = Result<Option<ClaimEdgeRecord>, StoreError>> + Send {
        timed("claim_edge", self.inner.claim_edge(tenant_id, edge_id))
    }

    fn claim_edges(
        &self,
        tenant_id: Uuid,
        selection: &ClaimEdgeSelection,
        after: Option<&ClaimEdgePosition>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<ClaimEdgeRecord>, StoreError>> + Send {
        timed(
            "claim_edges",
            self.inner.claim_edges(tenant_id, selection, after, limit),
        )
    }

    fn ratified_term_edges(
        &self,
        tenant_id: Uuid,
        label: &str,
    ) -> impl Future<Output = Result<Vec<(String, String)>, StoreError>> + Send {
        timed(
            "ratified_term_edges",
            self.inner.ratified_term_edges(tenant_id, label),
        )
    }

    fn transition(
        &self,
        tenant_id: Uuid,
        transition_id: Uuid,
    ) -> impl Future<Output = Result<Option<TransitionRecord>, StoreError>> + Send {
        timed(
            "transition",
            self.inner.transition(tenant_id, transition_id),
        )
    }

    fn pending_transitions(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> impl Future<Output = Result<Vec<TransitionRecord>, StoreError>> + Send {
        timed(
            "pending_transitions",
            self.inner.pending_transitions(tenant_id, workspace_id),
        )
    }

    fn commit(
        &self,
        req: CommitRequest,
    ) -> impl Future<Output = Result<CommitReceipt, StoreError>> + Send {
        timed("commit", self.inner.commit(req))
    }

    fn commit_chain(
        &self,
        reqs: Vec<CommitRequest>,
    ) -> impl Future<Output = Result<CommitReceipt, StoreError>> + Send {
        timed("commit_chain", self.inner.commit_chain(reqs))
    }

    fn create_tenant_partitions(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "create_tenant_partitions",
            self.inner.create_tenant_partitions(tenant_id),
        )
    }

    fn chain_head(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<ChainHead>, StoreError>> + Send {
        timed("chain_head", self.inner.chain_head(tenant_id))
    }

    fn events_range(
        &self,
        tenant_id: Uuid,
        from_seq: u64,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<SignedEnvelope>, StoreError>> + Send {
        timed(
            "events_range",
            self.inner.events_range(tenant_id, from_seq, limit),
        )
    }

    fn events_with_subjects(
        &self,
        tenant_id: Uuid,
        from_seq: u64,
        limit: usize,
    ) -> impl Future<Output = Result<TailScan, StoreError>> + Send {
        timed(
            "events_with_subjects",
            self.inner.events_with_subjects(tenant_id, from_seq, limit),
        )
    }

    fn memory(
        &self,
        tenant_id: Uuid,
        memory_id: Uuid,
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send {
        timed("memory", self.inner.memory(tenant_id, memory_id))
    }

    fn memories(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed("memories", self.inner.memories(tenant_id, memory_ids))
    }

    fn tenant_search_default(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        timed(
            "tenant_search_default",
            self.inner.tenant_search_default(tenant_id),
        )
    }

    fn tenant_query_text_since(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<EventTime>, StoreError>> + Send {
        timed(
            "tenant_query_text_since",
            self.inner.tenant_query_text_since(tenant_id),
        )
    }

    fn recall_defaults(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<
        Output = Result<(Option<String>, oml_app::federation::FederationPolicy), StoreError>,
    > + Send {
        timed("recall_defaults", self.inner.recall_defaults(tenant_id))
    }

    fn federation_policy(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<oml_app::federation::FederationPolicy, StoreError>> + Send
    {
        timed("federation_policy", self.inner.federation_policy(tenant_id))
    }

    fn federation_declining(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send {
        timed(
            "federation_declining",
            self.inner.federation_declining(tenant_id, nodes),
        )
    }

    fn reference_counts(
        &self,
        tenant_id: Uuid,
        scope: &oml_app::federation::CountsScope,
        query: &str,
    ) -> impl Future<Output = Result<oml_app::federation::ReferenceCounts, StoreError>> + Send {
        timed(
            "reference_counts",
            self.inner.reference_counts(tenant_id, scope, query),
        )
    }

    fn vocabulary_counts(
        &self,
        tenant_id: Uuid,
        scope: &oml_app::federation::CountsScope,
        max_pairs: usize,
    ) -> impl Future<Output = Result<Option<oml_app::federation::VocabularyCounts>, StoreError>> + Send
    {
        timed(
            "vocabulary_counts",
            self.inner.vocabulary_counts(tenant_id, scope, max_pairs),
        )
    }

    fn counts_from_vocabulary(
        &self,
        vocabulary: &oml_app::federation::VocabularyCounts,
        query: &str,
        prunes: bool,
    ) -> impl Future<Output = Result<oml_app::federation::ReferenceCounts, StoreError>> + Send {
        timed(
            "counts_from_vocabulary",
            self.inner.counts_from_vocabulary(vocabulary, query, prunes),
        )
    }

    fn guardrail_rows(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        author_key_id: Uuid,
        held_nodes: &[Uuid],
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed(
            "guardrail_rows",
            self.inner
                .guardrail_rows(tenant_id, nodes, author_key_id, held_nodes),
        )
    }

    fn steering_in_force(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed(
            "steering_in_force",
            self.inner.steering_in_force(tenant_id, nodes),
        )
    }

    fn steering_as_of(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        as_of: &EventTime,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed(
            "steering_as_of",
            self.inner.steering_as_of(tenant_id, nodes, as_of),
        )
    }

    fn tips_as_of(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
        as_of: &EventTime,
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send {
        timed(
            "tips_as_of",
            self.inner.tips_as_of(tenant_id, memory_ids, as_of),
        )
    }

    fn list_memories(
        &self,
        tenant_id: Uuid,
        filter: &ListingFilter,
        after: Option<&ListingPosition>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed(
            "list_memories",
            self.inner.list_memories(tenant_id, filter, after, limit),
        )
    }

    fn claim_key_state(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        claim_key: &str,
    ) -> impl Future<Output = Result<ClaimKeyState, StoreError>> + Send {
        timed(
            "claim_key_state",
            self.inner
                .claim_key_state(tenant_id, workspace_id, claim_key),
        )
    }

    fn scan_backlog(
        &self,
        tenant_id: Uuid,
        current: Option<&str>,
    ) -> impl Future<Output = Result<oml_app::ports::ScanBacklog, StoreError>> + Send {
        timed("scan_backlog", self.inner.scan_backlog(tenant_id, current))
    }

    fn scan_findings(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
    ) -> impl Future<Output = Result<Vec<(Uuid, oml_app::types::VersionFinding)>, StoreError>> + Send
    {
        timed(
            "scan_findings",
            self.inner.scan_findings(tenant_id, memory_ids),
        )
    }

    fn held_count(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        timed("held_count", self.inner.held_count(tenant_id, nodes))
    }

    fn stale_count(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        timed(
            "stale_count",
            self.inner.stale_count(tenant_id, workspace_id),
        )
    }

    fn claim_latest_readable(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        claim_key: &str,
        own_key_ids: &[Uuid],
        reads_promoted: bool,
    ) -> impl Future<Output = Result<Option<u32>, StoreError>> + Send {
        timed(
            "claim_latest_readable",
            self.inner.claim_latest_readable(
                tenant_id,
                workspace_id,
                claim_key,
                own_key_ids,
                reads_promoted,
            ),
        )
    }

    fn claim_tip(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        claim_key: &str,
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send {
        timed(
            "claim_tip",
            self.inner.claim_tip(tenant_id, workspace_id, claim_key),
        )
    }

    fn held_successors(
        &self,
        tenant_id: Uuid,
        replaced: Uuid,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed(
            "held_successors",
            self.inner.held_successors(tenant_id, replaced),
        )
    }

    fn review_claim_next(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        workspaces: &[Uuid],
        lane: ReviewLane,
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send {
        timed(
            "review_claim_next",
            self.inner.review_claim_next(
                tenant_id,
                reviewer_principal,
                reviewer_key_id,
                workspaces,
                lane,
            ),
        )
    }

    fn review_claimed(
        &self,
        tenant_id: Uuid,
        reviewer_key_id: Uuid,
        lane: ReviewLane,
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send {
        timed(
            "review_claimed",
            self.inner.review_claimed(tenant_id, reviewer_key_id, lane),
        )
    }

    fn review_held(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        filter: &HeldFilter,
        after: Option<&ListingPosition>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<HeldRow>, StoreError>> + Send {
        timed(
            "review_held",
            self.inner.review_held(
                tenant_id,
                reviewer_principal,
                reviewer_key_id,
                filter,
                after,
                limit,
            ),
        )
    }

    fn review_held_authors(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        filter: &HeldFilter,
    ) -> impl Future<Output = Result<Vec<(Uuid, u64)>, StoreError>> + Send {
        timed(
            "review_held_authors",
            self.inner
                .review_held_authors(tenant_id, reviewer_principal, reviewer_key_id, filter),
        )
    }

    fn key_by_hash(
        &self,
        key_hash: &str,
    ) -> impl Future<Output = Result<Option<KeyRecord>, StoreError>> + Send {
        timed("key_by_hash", self.inner.key_by_hash(key_hash))
    }

    fn key_access(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Option<KeyAccess>, StoreError>> + Send {
        timed("key_access", self.inner.key_access(tenant_id, key_id))
    }

    fn resolve_name(
        &self,
        tenant_id: Uuid,
        kind: NamedKind,
        name: &Name,
    ) -> impl Future<Output = Result<Option<Uuid>, StoreError>> + Send {
        timed(
            "resolve_name",
            self.inner.resolve_name(tenant_id, kind, name),
        )
    }

    fn resolve_user_workspace(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        name: &Name,
    ) -> impl Future<Output = Result<Option<Uuid>, StoreError>> + Send {
        timed(
            "resolve_user_workspace",
            self.inner
                .resolve_user_workspace(tenant_id, user_root, name),
        )
    }

    fn crew(
        &self,
        tenant_id: Uuid,
        crew_id: Uuid,
    ) -> impl Future<Output = Result<Option<CrewRecord>, StoreError>> + Send {
        timed("crew", self.inner.crew(tenant_id, crew_id))
    }

    fn crews(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Vec<CrewRecord>, StoreError>> + Send {
        timed("crews", self.inner.crews(tenant_id))
    }

    fn key(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Option<KeyRecord>, StoreError>> + Send {
        timed("key", self.inner.key(tenant_id, key_id))
    }

    fn key_references(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Option<KeyReferences>, StoreError>> + Send {
        timed(
            "key_references",
            self.inner.key_references(tenant_id, key_id),
        )
    }

    fn unnamed_rows(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<UnnamedRows, StoreError>> + Send {
        timed("unnamed_rows", self.inner.unnamed_rows(tenant_id))
    }

    fn workspace(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkspaceRecord>, StoreError>> + Send {
        timed("workspace", self.inner.workspace(tenant_id, workspace_id))
    }

    fn workspace_ancestry(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> impl Future<Output = Result<Option<Vec<WorkspaceRecord>>, StoreError>> + Send {
        timed(
            "workspace_ancestry",
            self.inner.workspace_ancestry(tenant_id, workspace_id),
        )
    }

    fn workspace_subtree(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkspaceRecord>, StoreError>> + Send {
        timed(
            "workspace_subtree",
            self.inner.workspace_subtree(tenant_id, workspace_id),
        )
    }

    fn workspace_children(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkspaceRecord>, StoreError>> + Send {
        timed(
            "workspace_children",
            self.inner.workspace_children(tenant_id, workspace_id),
        )
    }

    fn idempotency_get(
        &self,
        scope: &IdempotencyScope,
    ) -> impl Future<Output = Result<Option<IdempotencyRecord>, StoreError>> + Send {
        timed("idempotency_get", self.inner.idempotency_get(scope))
    }

    fn idempotency_claim(
        &self,
        scope: &IdempotencyScope,
        request_digest: &str,
        claim_id: Uuid,
    ) -> impl Future<Output = Result<IdempotencyClaim, StoreError>> + Send {
        timed(
            "idempotency_claim",
            self.inner
                .idempotency_claim(scope, request_digest, claim_id),
        )
    }

    fn idempotency_settle(
        &self,
        put: IdempotencyPut,
        claim_id: Uuid,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        timed(
            "idempotency_settle",
            self.inner.idempotency_settle(put, claim_id),
        )
    }

    fn idempotency_release(
        &self,
        scope: &IdempotencyScope,
        claim_id: Uuid,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "idempotency_release",
            self.inner.idempotency_release(scope, claim_id),
        )
    }
}

impl<J: ConnectionStore> ConnectionStore for Timed<J> {
    fn code_by_hash(
        &self,
        code_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<TenantRow<ConnectCodeRecord>>, StoreError>> + Send {
        timed("code_by_hash", self.inner.code_by_hash(code_hash))
    }

    fn tenant_name(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Option<Name>, StoreError>> + Send {
        timed("tenant_name", self.inner.tenant_name(tenant_id))
    }

    fn spend_code(
        &self,
        tenant_id: Uuid,
        code_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        timed("spend_code", self.inner.spend_code(tenant_id, code_id, at))
    }

    fn take_authorization_by_handle(
        &self,
        handle_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<AuthorizationRecord>, StoreError>> + Send {
        timed(
            "take_authorization_by_handle",
            self.inner.take_authorization_by_handle(handle_hash),
        )
    }

    fn connection_by_auth_code(
        &self,
        auth_code_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<TenantRow<ConnectionRecord>>, StoreError>> + Send {
        timed(
            "connection_by_auth_code",
            self.inner.connection_by_auth_code(auth_code_hash),
        )
    }

    fn token_by_hash(
        &self,
        token_hash: &Sha256Hash,
        kind: TokenKind,
    ) -> impl Future<Output = Result<Option<TenantRow<TokenRecord>>, StoreError>> + Send {
        timed("token_by_hash", self.inner.token_by_hash(token_hash, kind))
    }

    fn insert_token_pair(
        &self,
        tenant_id: Uuid,
        connection_id: Uuid,
        tokens: TokenPair,
        at: &EventTime,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "insert_token_pair",
            self.inner
                .insert_token_pair(tenant_id, connection_id, tokens, at),
        )
    }

    fn rotate_refresh<F>(
        &self,
        tenant_id: Uuid,
        token_hash: &Sha256Hash,
        next: TokenPair,
        at: &EventTime,
        decide: F,
    ) -> impl Future<Output = Result<RefreshOutcome, StoreError>> + Send
    where
        F: FnOnce(Option<&RefreshFacts>) -> RefreshDecision + Send,
    {
        timed(
            "rotate_refresh",
            self.inner
                .rotate_refresh(tenant_id, token_hash, next, at, decide),
        )
    }

    fn sweep_expired(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<SweepReport, StoreError>> + Send {
        timed("sweep_expired", self.inner.sweep_expired(tenant_id, at))
    }

    fn connection_connect(
        &self,
        handle_hash: &Sha256Hash,
        grant: ConnectGrant,
    ) -> impl Future<Output = Result<ConnectOutcome, StoreError>> + Send {
        timed(
            "connection_connect",
            self.inner.connection_connect(handle_hash, grant),
        )
    }

    fn authorization_insert(
        &self,
        record: AuthorizationRecord,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "authorization_insert",
            self.inner.authorization_insert(record),
        )
    }

    fn authorization(
        &self,
        handle_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<AuthorizationRecord>, StoreError>> + Send {
        timed("authorization", self.inner.authorization(handle_hash))
    }

    fn oauth_client_insert(
        &self,
        record: OAuthClientRecord,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "oauth_client_insert",
            self.inner.oauth_client_insert(record),
        )
    }

    fn oauth_client(
        &self,
        client_id: &str,
    ) -> impl Future<Output = Result<Option<OAuthClientRecord>, StoreError>> + Send {
        timed("oauth_client", self.inner.oauth_client(client_id))
    }

    fn upsert_cimd(
        &self,
        document: ClientDescription,
        fetched_at: &EventTime,
        max_age: u64,
    ) -> impl Future<Output = Result<DocumentCaching, StoreError>> + Send {
        timed(
            "upsert_cimd",
            self.inner.upsert_cimd(document, fetched_at, max_age),
        )
    }

    fn touch_cimd(
        &self,
        client_id: &str,
        at: &EventTime,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        timed("touch_cimd", self.inner.touch_cimd(client_id, at))
    }

    fn insert_dcr(
        &self,
        registration: ClientDescription,
        at: &EventTime,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed("insert_dcr", self.inner.insert_dcr(registration, at))
    }

    fn remove_unused_dcr(
        &self,
        registered_by: &EventTime,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        timed(
            "remove_unused_dcr",
            self.inner.remove_unused_dcr(registered_by),
        )
    }

    fn connect_codes(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConnectCodeRecord>, StoreError>> + Send {
        timed("connect_codes", self.inner.connect_codes(tenant_id, key_id))
    }

    fn connections(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ConnectionRecord>, StoreError>> + Send {
        timed("connections", self.inner.connections(tenant_id, key_id))
    }

    fn connection(
        &self,
        tenant_id: Uuid,
        connection_id: Uuid,
    ) -> impl Future<Output = Result<Option<ConnectionRecord>, StoreError>> + Send {
        timed(
            "connection",
            self.inner.connection(tenant_id, connection_id),
        )
    }
}

impl<J: WebStore> WebStore for Timed<J> {
    fn web_entry_by_email(
        &self,
        email_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<WebEntry>, StoreError>> + Send {
        timed(
            "web_entry_by_email",
            self.inner.web_entry_by_email(email_hash),
        )
    }

    fn web_entries(
        &self,
        tenant_id: Uuid,
        after: Option<&Sha256Hash>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<WebEntry>, StoreError>> + Send {
        timed(
            "web_entries",
            self.inner.web_entries(tenant_id, after, limit),
        )
    }

    fn web_pin(
        &self,
        tenant_id: Uuid,
        email_hash: &Sha256Hash,
        provider: Provider,
        subject: &str,
    ) -> impl Future<Output = Result<PinOutcome, StoreError>> + Send {
        timed(
            "web_pin",
            self.inner.web_pin(tenant_id, email_hash, provider, subject),
        )
    }

    fn web_session_open(
        &self,
        session: WebSessionRecord,
        replaces: Option<Sha256Hash>,
        credentials: Vec<(&'static str, Sha256Hash)>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "web_session_open",
            self.inner.web_session_open(session, replaces, credentials),
        )
    }

    fn web_session_credential(
        &self,
        credential_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<(WebSessionRecord, String)>, StoreError>> + Send {
        timed(
            "web_session_credential",
            self.inner.web_session_credential(credential_hash),
        )
    }

    fn web_credential_rows(
        &self,
        credential_hash: &Sha256Hash,
        key_name: SessionKeyName,
    ) -> impl Future<Output = Result<Option<WebCredentialRows>, StoreError>> + Send {
        timed(
            "web_credential_rows",
            self.inner.web_credential_rows(credential_hash, key_name),
        )
    }

    fn web_session(
        &self,
        token_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<Option<WebSessionRecord>, StoreError>> + Send {
        timed("web_session", self.inner.web_session(token_hash))
    }

    fn web_session_end(
        &self,
        tenant_id: Uuid,
        token_hash: &Sha256Hash,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        timed(
            "web_session_end",
            self.inner.web_session_end(tenant_id, token_hash),
        )
    }

    fn web_linked_keys(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> impl Future<Output = Result<Vec<KeyRecord>, StoreError>> + Send {
        timed(
            "web_linked_keys",
            self.inner.web_linked_keys(tenant_id, user_root),
        )
    }

    fn web_user_keys(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> impl Future<Output = Result<Vec<KeyRecord>, StoreError>> + Send {
        timed(
            "web_user_keys",
            self.inner.web_user_keys(tenant_id, user_root),
        )
    }

    fn web_session_keys(
        &self,
        tenant_id: Uuid,
        session_user: &Name,
    ) -> impl Future<Output = Result<Vec<KeyRecord>, StoreError>> + Send {
        timed(
            "web_session_keys",
            self.inner.web_session_keys(tenant_id, session_user),
        )
    }

    fn web_queue(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Vec<QueueDepth>, StoreError>> + Send {
        timed("web_queue", self.inner.web_queue(tenant_id))
    }

    fn web_person_reviewers(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Vec<PersonReviewer>, StoreError>> + Send {
        timed(
            "web_person_reviewers",
            self.inner.web_person_reviewers(tenant_id),
        )
    }

    fn web_admin_search(
        &self,
        tenant_id: Uuid,
        search: &AdminSearch,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed(
            "web_admin_search",
            self.inner.web_admin_search(tenant_id, search),
        )
    }

    fn web_deletion_requests(
        &self,
        tenant_id: Uuid,
    ) -> impl Future<Output = Result<Vec<oml_app::types::DeletionRequest>, StoreError>> + Send {
        timed(
            "web_deletion_requests",
            self.inner.web_deletion_requests(tenant_id),
        )
    }

    fn web_sessions_sweep(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        timed(
            "web_sessions_sweep",
            self.inner.web_sessions_sweep(tenant_id, at),
        )
    }
}
