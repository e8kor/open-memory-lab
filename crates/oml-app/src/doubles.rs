//! In-memory doubles for every port: the contract suites in
//! `oml-port-contracts` run against these first, and a real adapter passes
//! the identical suite (workspace-plan.md M1 build order, step 2).
//!
//! The doubles are deterministic — fixed clock, counting id generator — so
//! every test chain is reproducible byte for byte.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use uuid::Uuid;

use oml_domain::{ChainHead, EventTime, MemoryStatus, Role};

use crate::ports::{
    AdmissionEnvelope, Clock, CommitReceipt, CommitRequest, IdGen, IdempotencyRecord,
    IdempotencyScope, JournalStore, KeyringError, KeyringStore, MetadataFold, OutboxRow,
    ScanOutcome, Scanner, Signer, StoreError,
};
use crate::types::{
    ClaimKeyState, DenialRecord, GrantRecord, KeyAccess, KeyRecord, ListingFilter, ListingPosition,
    ListingScope, MemoryRecord, WorkareaRecord,
};

/// The states whose versions hold their `claim_key` and their ordinal
/// (postgres-schema.md `memories_claim_version`; ADR-0025 d18).
const LIVE: [MemoryStatus; 3] = [
    MemoryStatus::Held,
    MemoryStatus::Promoted,
    MemoryStatus::Demoted,
];

// ---------------------------------------------------------------------------
// Journal store
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct QueueEntry {
    memory_id: Uuid,
    claimed_by: Option<Uuid>,
}

#[derive(Debug, Default)]
struct TenantState {
    events: Vec<crate::domain::SignedEnvelope>,
    memories: BTreeMap<Uuid, MemoryRecord>,
    workareas: BTreeMap<Uuid, WorkareaRecord>,
    keys_by_hash: BTreeMap<String, KeyRecord>,
    /// Per key id, in commit order — which is epoch order.
    access: BTreeMap<Uuid, KeyAccess>,
    /// Keys `key.revoked` folded: they resolve by hash no more.
    revoked: BTreeSet<Uuid>,
    queue: Vec<QueueEntry>,
    outbox: Vec<OutboxRow>,
    idempotency: BTreeMap<(Uuid, Uuid, String), IdempotencyRecord>,
}

/// BTreeMap journal-and-metadata store with a real lock per tenant: chain
/// assignment is serialized exactly as the Postgres adapter will serialize
/// it (ADR-0002 one chain head per tenant). A clone is another handle on the
/// same state, as a second `PgStore` handle is on the same database, so a
/// test can read through the port what the use-case it drives committed.
#[derive(Debug, Default, Clone)]
pub struct MemStore {
    tenants: Arc<Mutex<BTreeMap<Uuid, Arc<Mutex<TenantState>>>>>,
    /// The event type whose next commit is refused, then forgotten.
    refuse_next: Arc<Mutex<Option<&'static str>>>,
}

impl MemStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Refuses the next commit of an `event_type` event with
    /// `StoreError::Backend`, changing nothing, then commits normally again:
    /// a storage failure in the middle of a multi-commit use-case, on demand.
    #[must_use]
    pub fn refusing_next_commit_of(self, event_type: &'static str) -> Self {
        *self.refuse_next.lock().expect("refusal lock") = Some(event_type);
        self
    }

    fn tenant(&self, tenant_id: Uuid) -> Arc<Mutex<TenantState>> {
        Arc::clone(
            self.tenants
                .lock()
                .expect("tenants lock")
                .entry(tenant_id)
                .or_default(),
        )
    }

    fn tenant_existing(&self, tenant_id: Uuid) -> Option<Arc<Mutex<TenantState>>> {
        self.tenants
            .lock()
            .expect("tenants lock")
            .get(&tenant_id)
            .cloned()
    }

    /// Test visibility: how many outbox rows a tenant has accumulated.
    #[must_use]
    pub fn outbox_len(&self, tenant_id: Uuid) -> usize {
        self.tenant_existing(tenant_id)
            .map_or(0, |t| t.lock().expect("tenant lock").outbox.len())
    }
}

/// The rows a grant or denial names must be held by its own tenant, as the
/// schema's tenant-scoped foreign keys require (postgres-schema.md `grants`,
/// `key_denials`).
fn require_held(state: &TenantState, key_id: Uuid, node: Uuid) -> Result<(), StoreError> {
    if !state.access.contains_key(&key_id) {
        return Err(StoreError::Backend(format!(
            "key {key_id} is not held by this tenant"
        )));
    }
    if !state.workareas.contains_key(&node) {
        return Err(StoreError::Backend(format!(
            "workarea {node} is not held by this tenant"
        )));
    }
    Ok(())
}

/// A second live version of `memory`'s ordinal under its `claim_key` in its
/// workarea is refused, as `memories_claim_version` refuses it.
fn require_free_ordinal(state: &TenantState, memory: &MemoryRecord) -> Result<(), StoreError> {
    if state.memories.values().any(|m| {
        m.workarea_id == memory.workarea_id
            && m.claim_key == memory.claim_key
            && m.version == memory.version
            && LIVE.contains(&m.status)
    }) {
        return Err(StoreError::Backend(format!(
            "version {} of claim_key {:?} is already live in this workarea",
            memory.version, memory.claim_key
        )));
    }
    Ok(())
}

/// Applies one fold, or refuses it before changing anything — what the
/// schema refuses, the double refuses, so the commit stays all or nothing.
/// `seq` and `at` are the commit's event seq and time: a key's
/// `issued_seq`, a grant's or denial's epoch and a memory's `promoted_at`.
fn apply_fold(
    state: &mut TenantState,
    fold: MetadataFold,
    seq: u64,
    at: &EventTime,
) -> Result<(), StoreError> {
    match fold {
        MetadataFold::TenantGenesis | MetadataFold::VerdictRecorded { .. } => {}
        MetadataFold::WorkareaCreated(workarea) => {
            state.workareas.insert(workarea.workarea_id, workarea);
        }
        MetadataFold::KeyIssued(key) => {
            state.access.insert(
                key.key_id,
                KeyAccess {
                    issued_seq: seq,
                    grants: Vec::new(),
                    denials: Vec::new(),
                },
            );
            state.keys_by_hash.insert(key.key_hash.clone(), key);
        }
        MetadataFold::GrantIssued { key_id, grant } => {
            require_held(state, key_id, grant.node)?;
            if !matches!(grant.role, Role::Reader | Role::Writer | Role::Dreamer) {
                return Err(StoreError::Backend(format!(
                    "a grant role is reader, writer or dreamer, never {}",
                    grant.role.as_str()
                )));
            }
            if let Some(access) = state.access.get_mut(&key_id) {
                access.grants.push(GrantRecord { grant, epoch: seq });
            }
        }
        MetadataFold::KeyDenied {
            key_id,
            denial,
            created_by,
        } => {
            require_held(state, key_id, denial.node)?;
            if let Some(access) = state.access.get_mut(&key_id) {
                access.denials.push(DenialRecord {
                    denial,
                    created_by,
                    epoch: seq,
                });
            }
        }
        MetadataFold::KeyRevoked { key_id } => {
            if !state.access.contains_key(&key_id) || state.revoked.contains(&key_id) {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a live key of this tenant"
                )));
            }
            state.revoked.insert(key_id);
        }
        MetadataFold::MemoryHeld { memory, enqueue } => {
            require_free_ordinal(state, &memory)?;
            if enqueue {
                state.queue.push(QueueEntry {
                    memory_id: memory.memory_id,
                    claimed_by: None,
                });
            }
            state.memories.insert(memory.memory_id, memory);
        }
        MetadataFold::MemoryPromoted { memory_id } => {
            if let Some(promoted) = state.memories.get(&memory_id).cloned() {
                // One served tip per claim: the replaced version gives the
                // tip up, and any other tip refuses the commit, as
                // `memories_served_tip` does (ADR-0025 d10).
                if state.memories.values().any(|m| {
                    m.memory_id != memory_id
                        && Some(m.memory_id) != promoted.replaces
                        && m.workarea_id == promoted.workarea_id
                        && m.claim_key == promoted.claim_key
                        && m.is_tip
                        && m.status == MemoryStatus::Promoted
                }) {
                    return Err(StoreError::Backend(format!(
                        "claim_key {:?} already has a served tip in this workarea",
                        promoted.claim_key
                    )));
                }
                if let Some(replaced) = promoted.replaces.and_then(|id| state.memories.get_mut(&id))
                {
                    replaced.is_tip = false;
                }
                if let Some(memory) = state.memories.get_mut(&memory_id) {
                    memory.status = MemoryStatus::Promoted;
                    memory.is_tip = true;
                    memory.promoted_at = Some(at.clone());
                }
            }
            state.queue.retain(|e| e.memory_id != memory_id);
        }
        MetadataFold::MemoryRejected { memory_id } => {
            if let Some(memory) = state.memories.get_mut(&memory_id) {
                memory.status = MemoryStatus::Rejected;
            }
            state.queue.retain(|e| e.memory_id != memory_id);
        }
        MetadataFold::MemorySuperseded {
            superseded,
            successor,
        } => {
            // The replaced version stays promoted; only the tip moves
            // (ADR-0025 d10).
            require_free_ordinal(state, &successor)?;
            if let Some(memory) = state.memories.get_mut(&superseded) {
                memory.is_tip = false;
            }
            state.queue.push(QueueEntry {
                memory_id: successor.memory_id,
                claimed_by: None,
            });
            state.memories.insert(successor.memory_id, successor);
        }
    }
    Ok(())
}

impl JournalStore for MemStore {
    async fn commit(&self, req: CommitRequest) -> Result<CommitReceipt, StoreError> {
        {
            let mut refuse = self.refuse_next.lock().expect("refusal lock");
            if *refuse == Some(req.event.envelope.event_type.as_str()) {
                *refuse = None;
                return Err(StoreError::Backend(format!(
                    "the double refused this {} commit on request",
                    req.event.envelope.event_type
                )));
            }
        }
        let tenant = self.tenant(req.event.envelope.tenant_id);
        // The per-tenant lock IS the serialization point the real adapter
        // provides with a row lock on the chain head (ADR-0002).
        let mut state = tenant.lock().expect("tenant lock");
        let expected_seq = state.events.len() as u64 + 1;
        if req.event.envelope.seq != expected_seq {
            return Err(StoreError::SeqConflict {
                expected_seq,
                found_seq: req.event.envelope.seq,
            });
        }
        if let Some(last) = state.events.last()
            && req.event.envelope.prev_hash != last.hash
        {
            return Err(StoreError::PrevHashConflict { seq: expected_seq });
        }
        apply_fold(&mut state, req.fold, expected_seq, &req.event.envelope.at)?;
        if let Some(put) = req.idempotency {
            state.idempotency.insert(
                (put.scope.key_id, put.scope.workarea_id, put.scope.idem_key),
                put.record,
            );
        }
        state.outbox.push(req.outbox);
        let receipt = CommitReceipt {
            seq: req.event.envelope.seq,
            hash: req.event.hash,
        };
        state.events.push(req.event);
        Ok(receipt)
    }

    async fn chain_head(&self, tenant_id: Uuid) -> Result<Option<ChainHead>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state.events.last().map(|e| ChainHead {
            tenant_id,
            seq: e.envelope.seq,
            hash: e.hash,
        }))
    }

    async fn events_range(
        &self,
        tenant_id: Uuid,
        from_seq: u64,
        limit: usize,
    ) -> Result<Vec<crate::domain::SignedEnvelope>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state
            .events
            .iter()
            .filter(|e| e.envelope.seq >= from_seq)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn memory(
        &self,
        tenant_id: Uuid,
        memory_id: Uuid,
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state.memories.get(&memory_id).cloned())
    }

    async fn recall_fts(
        &self,
        tenant_id: Uuid,
        readable: &[Uuid],
        query: &str,
        k: usize,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let needle = query.to_lowercase();
        Ok(state
            .memories
            .values()
            .filter(|m| m.status == MemoryStatus::Promoted && m.is_tip)
            .filter(|m| readable.contains(&m.workarea_id))
            .filter(|m| m.body.to_lowercase().contains(&needle))
            .take(k)
            .cloned()
            .collect())
    }

    async fn list_memories(
        &self,
        tenant_id: Uuid,
        filter: &ListingFilter,
        after: Option<&ListingPosition>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let in_scope = |m: &MemoryRecord| match &filter.scope {
            ListingScope::Own { key_ids, masked } => {
                key_ids.contains(&m.author_key_id) && !masked.contains(&m.workarea_id)
            }
            ListingScope::Tips { nodes } => {
                m.status == MemoryStatus::Promoted && m.is_tip && nodes.contains(&m.workarea_id)
            }
        };
        let position = |m: &MemoryRecord| (m.created_at.as_str().to_owned(), m.memory_id);
        let mut rows: Vec<MemoryRecord> = state
            .memories
            .values()
            .filter(|m| in_scope(m))
            .filter(|m| {
                filter
                    .subtree
                    .as_ref()
                    .is_none_or(|nodes| nodes.contains(&m.workarea_id))
            })
            .filter(|m| filter.state.is_none_or(|s| m.status == s))
            .filter(|m| filter.kind.is_none_or(|k| m.kind == k))
            .filter(|m| filter.claim_key.as_ref().is_none_or(|c| &m.claim_key == c))
            .filter(|m| {
                filter
                    .since
                    .as_ref()
                    .is_none_or(|t| m.created_at.as_str() >= t.as_str())
            })
            .filter(|m| {
                filter
                    .until
                    .as_ref()
                    .is_none_or(|t| m.created_at.as_str() < t.as_str())
            })
            .filter(|m| {
                after
                    .is_none_or(|p| position(m) < (p.created_at.as_str().to_owned(), p.artifact_id))
            })
            .cloned()
            .collect();
        // The fixed-width timestamp orders as its string does; newest first.
        rows.sort_by_key(|m| std::cmp::Reverse(position(m)));
        rows.truncate(limit);
        Ok(rows)
    }

    async fn claim_key_state(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> Result<ClaimKeyState, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(ClaimKeyState::default());
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state
            .memories
            .values()
            .filter(|m| m.workarea_id == workarea_id && m.claim_key == claim_key)
            .fold(ClaimKeyState::default(), |acc, m| ClaimKeyState {
                greatest_version: acc.greatest_version.max(Some(m.version)),
                live: acc.live || LIVE.contains(&m.status),
            }))
    }

    async fn review_claim_next(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        workareas: &[Uuid],
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let mut state = tenant.lock().expect("tenant lock");
        let TenantState {
            queue, memories, ..
        } = &mut *state;
        for entry in queue.iter_mut() {
            if entry.claimed_by.is_some_and(|k| k != reviewer_key_id) {
                continue;
            }
            let Some(memory) = memories.get(&entry.memory_id) else {
                continue;
            };
            if !workareas.contains(&memory.workarea_id) {
                continue;
            }
            // Never serve a row the reviewer's lineage excludes
            // (admission.md review; R2 is re-checked at verdict time).
            if memory
                .lineage_principals
                .iter()
                .any(|p| p == reviewer_principal)
                || memory.lineage_key_ids.contains(&reviewer_key_id)
            {
                continue;
            }
            entry.claimed_by = Some(reviewer_key_id);
            return Ok(Some(memory.clone()));
        }
        Ok(None)
    }

    async fn key_by_hash(&self, key_hash: &str) -> Result<Option<KeyRecord>, StoreError> {
        let tenants: Vec<_> = self
            .tenants
            .lock()
            .expect("tenants lock")
            .values()
            .cloned()
            .collect();
        for tenant in tenants {
            let state = tenant.lock().expect("tenant lock");
            if let Some(key) = state.keys_by_hash.get(key_hash) {
                if state.revoked.contains(&key.key_id) {
                    return Ok(None);
                }
                return Ok(Some(key.clone()));
            }
        }
        Ok(None)
    }

    async fn key_access(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Option<KeyAccess>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state.access.get(&key_id).cloned())
    }

    async fn workarea(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Option<WorkareaRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state.workareas.get(&workarea_id).cloned())
    }

    async fn workarea_ancestry(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Option<Vec<WorkareaRecord>>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        let Some(node) = state.workareas.get(&workarea_id) else {
            return Ok(None);
        };
        let mut ancestry = vec![node.clone()];
        let mut parent = node.parent_id;
        while let Some(parent_id) = parent {
            // A walk that leaves the tenant before the root would drop
            // ancestors' grants and denials; refuse it instead.
            let Some(next) = state.workareas.get(&parent_id) else {
                return Err(StoreError::Backend(format!(
                    "the ancestry of {workarea_id} names parent {parent_id}, \
                     which this tenant does not hold"
                )));
            };
            ancestry.push(next.clone());
            parent = next.parent_id;
        }
        Ok(Some(ancestry))
    }

    async fn workarea_subtree(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Vec<WorkareaRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let Some(node) = state.workareas.get(&workarea_id) else {
            return Ok(Vec::new());
        };
        let mut subtree = vec![node.clone()];
        let mut depth = vec![workarea_id];
        while !depth.is_empty() {
            // The map iterates in id order, so each depth comes out in id
            // order.
            let next: Vec<WorkareaRecord> = state
                .workareas
                .values()
                .filter(|w| w.parent_id.is_some_and(|p| depth.contains(&p)))
                .cloned()
                .collect();
            depth = next.iter().map(|w| w.workarea_id).collect();
            subtree.extend(next);
        }
        Ok(subtree)
    }

    async fn workarea_children(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Vec<WorkareaRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let mut children: Vec<WorkareaRecord> = state
            .workareas
            .values()
            .filter(|w| w.parent_id == Some(workarea_id))
            .cloned()
            .collect();
        // `str` orders by its bytes.
        children.sort_by(|a, b| {
            (a.name.as_str(), a.workarea_id).cmp(&(b.name.as_str(), b.workarea_id))
        });
        Ok(children)
    }

    async fn idempotency_get(
        &self,
        scope: &IdempotencyScope,
    ) -> Result<Option<IdempotencyRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(scope.tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state
            .idempotency
            .get(&(scope.key_id, scope.workarea_id, scope.idem_key.clone()))
            .cloned())
    }
}

// ---------------------------------------------------------------------------
// Signer
// ---------------------------------------------------------------------------

/// A signer over an in-memory Ed25519 key — the lead composition's port
/// double.
#[derive(Debug)]
pub struct MemSigner {
    key: SigningKey,
}

impl MemSigner {
    #[must_use]
    pub fn new(seed: [u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(&seed),
        }
    }
}

impl Signer for MemSigner {
    fn sign(&self, signed_bytes: &[u8]) -> Signature {
        self.key.sign(signed_bytes)
    }

    fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }
}

// ---------------------------------------------------------------------------
// Keyring
// ---------------------------------------------------------------------------

/// Keyring double: "creates" a key from a fixed seed on first load and
/// returns the same key ever after — the load-or-create law.
#[derive(Debug)]
pub struct MemKeyring {
    create_seed: [u8; 32],
    stored: Mutex<Option<[u8; 32]>>,
}

impl MemKeyring {
    #[must_use]
    pub fn new(create_seed: [u8; 32]) -> Self {
        Self {
            create_seed,
            stored: Mutex::new(None),
        }
    }
}

impl KeyringStore for MemKeyring {
    async fn load_or_create(&self) -> Result<SigningKey, KeyringError> {
        let mut stored = self.stored.lock().expect("keyring lock");
        let seed = *stored.get_or_insert(self.create_seed);
        Ok(SigningKey::from_bytes(&seed))
    }
}

// ---------------------------------------------------------------------------
// Clock and id generator
// ---------------------------------------------------------------------------

/// Deterministic clock: a fixed date, one microsecond per call.
#[derive(Debug, Default)]
pub struct FixedClock {
    ticks: AtomicU64,
}

impl FixedClock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clock for FixedClock {
    fn now(&self) -> EventTime {
        let n = self.ticks.fetch_add(1, Ordering::SeqCst);
        let micros = n % 1_000_000;
        let secs_total = n / 1_000_000;
        let secs = secs_total % 60;
        let mins = (secs_total / 60) % 60;
        EventTime::parse(&format!("2026-10-29T12:{mins:02}:{secs:02}.{micros:06}Z"))
            .expect("the fixed clock emits the fixed-width form")
    }
}

/// Deterministic id generator: counting UUIDs and counting 256-bit secrets.
#[derive(Debug, Default)]
pub struct SeqIdGen {
    next: AtomicU64,
}

impl SeqIdGen {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl IdGen for SeqIdGen {
    fn new_id(&self) -> Uuid {
        Uuid::from_u128(u128::from(self.next.fetch_add(1, Ordering::SeqCst)) + 1)
    }

    fn new_secret(&self) -> [u8; 32] {
        let n = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let mut secret = [0_u8; 32];
        secret[24..].copy_from_slice(&n.to_be_bytes());
        secret
    }
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

/// Deterministic marker scanner: substring rules standing in for the rule
/// set (admission.md: the scan is rules, not a model). No promoting arm
/// exists — the type refuses one.
#[derive(Debug, Clone)]
pub struct MarkerScanner {
    pub veto_marker: String,
    pub hold_marker: String,
}

impl Default for MarkerScanner {
    fn default() -> Self {
        Self {
            veto_marker: "-----BEGIN".to_owned(),
            hold_marker: "[needs-human]".to_owned(),
        }
    }
}

impl Scanner for MarkerScanner {
    fn scan(&self, envelope: &AdmissionEnvelope<'_>) -> ScanOutcome {
        if envelope.body.contains(&self.veto_marker) {
            ScanOutcome::Veto(format!(
                "secret.private_key: body matches {:?}",
                self.veto_marker
            ))
        } else if envelope.body.contains(&self.hold_marker) {
            ScanOutcome::Hold(format!(
                "marker finding: body matches {:?}",
                self.hold_marker
            ))
        } else {
            ScanOutcome::Pass
        }
    }
}
