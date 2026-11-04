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
use oml_app::domain::{ChainHead, EventTime, MemoryKind, Sha256Hash, SignedEnvelope};
use oml_app::ports::{
    CommitReceipt, CommitRequest, ConnectionStore, IdempotencyRecord, IdempotencyScope,
    JournalStore, StoreError, TailScan,
};
use oml_app::types::{
    AuthorizationRecord, ClaimKeyState, ClientDescription, ConnectCodeRecord, ConnectGrant,
    ConnectOutcome, ConnectionRecord, CrewRecord, DocumentCaching, KeyAccess, KeyRecord,
    KeyReferences, ListingFilter, ListingPosition, MemoryRecord, OAuthClientRecord,
    RecallCandidate, RefreshFacts, RefreshOutcome, SweepReport, TenantRow, TokenPair, TokenRecord,
    UnnamedRows, WorkareaRecord,
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

impl<J: JournalStore> JournalStore for Timed<J> {
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

    fn recall_fts(
        &self,
        tenant_id: Uuid,
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> impl Future<Output = Result<Vec<RecallCandidate>, StoreError>> + Send {
        timed(
            "recall_fts",
            self.inner.recall_fts(tenant_id, readable, query, kinds, k),
        )
    }

    fn guardrail_rows(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        author_key_id: Uuid,
    ) -> impl Future<Output = Result<Vec<MemoryRecord>, StoreError>> + Send {
        timed(
            "guardrail_rows",
            self.inner.guardrail_rows(tenant_id, nodes, author_key_id),
        )
    }

    fn recall_own_held(
        &self,
        tenant_id: Uuid,
        author_key_ids: &[Uuid],
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> impl Future<Output = Result<Vec<RecallCandidate>, StoreError>> + Send {
        timed(
            "recall_own_held",
            self.inner
                .recall_own_held(tenant_id, author_key_ids, readable, query, kinds, k),
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
        workarea_id: Uuid,
        claim_key: &str,
    ) -> impl Future<Output = Result<ClaimKeyState, StoreError>> + Send {
        timed(
            "claim_key_state",
            self.inner
                .claim_key_state(tenant_id, workarea_id, claim_key),
        )
    }

    fn claim_tip(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send {
        timed(
            "claim_tip",
            self.inner.claim_tip(tenant_id, workarea_id, claim_key),
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
        workareas: &[Uuid],
    ) -> impl Future<Output = Result<Option<MemoryRecord>, StoreError>> + Send {
        timed(
            "review_claim_next",
            self.inner
                .review_claim_next(tenant_id, reviewer_principal, reviewer_key_id, workareas),
        )
    }

    fn review_claimed(
        &self,
        tenant_id: Uuid,
        reviewer_key_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Uuid>, StoreError>> + Send {
        timed(
            "review_claimed",
            self.inner.review_claimed(tenant_id, reviewer_key_id),
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

    fn workarea(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkareaRecord>, StoreError>> + Send {
        timed("workarea", self.inner.workarea(tenant_id, workarea_id))
    }

    fn workarea_ancestry(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Option<Vec<WorkareaRecord>>, StoreError>> + Send {
        timed(
            "workarea_ancestry",
            self.inner.workarea_ancestry(tenant_id, workarea_id),
        )
    }

    fn workarea_subtree(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkareaRecord>, StoreError>> + Send {
        timed(
            "workarea_subtree",
            self.inner.workarea_subtree(tenant_id, workarea_id),
        )
    }

    fn workarea_children(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkareaRecord>, StoreError>> + Send {
        timed(
            "workarea_children",
            self.inner.workarea_children(tenant_id, workarea_id),
        )
    }

    fn idempotency_get(
        &self,
        scope: &IdempotencyScope,
    ) -> impl Future<Output = Result<Option<IdempotencyRecord>, StoreError>> + Send {
        timed("idempotency_get", self.inner.idempotency_get(scope))
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
