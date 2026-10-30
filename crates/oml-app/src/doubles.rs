//! In-memory doubles for every port: the contract suites in
//! `oml-port-contracts` run against these first, and a real adapter passes
//! the identical suite (workspace-plan.md M1 build order, step 2).
//!
//! The doubles are deterministic — fixed clock, counting id generator — so
//! every test chain is reproducible byte for byte.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use uuid::Uuid;

use oml_domain::{ChainHead, EventTime, MemoryStatus};

use crate::ports::{
    AdmissionEnvelope, Clock, CommitReceipt, CommitRequest, IdGen, IdempotencyRecord,
    IdempotencyScope, JournalStore, KeyringError, KeyringStore, MetadataFold, OutboxRow,
    ScanOutcome, Scanner, Signer, StoreError,
};
use crate::types::{KeyRecord, MemoryRecord, WorkareaRecord};

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
    queue: Vec<QueueEntry>,
    outbox: Vec<OutboxRow>,
    idempotency: BTreeMap<(Uuid, Uuid, String), IdempotencyRecord>,
}

/// BTreeMap journal-and-metadata store with a real lock per tenant: chain
/// assignment is serialized exactly as the Postgres adapter will serialize
/// it (ADR-0002 one chain head per tenant).
#[derive(Debug, Default)]
pub struct MemStore {
    tenants: Mutex<BTreeMap<Uuid, Arc<Mutex<TenantState>>>>,
}

impl MemStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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

fn apply_fold(state: &mut TenantState, fold: MetadataFold) {
    match fold {
        MetadataFold::TenantGenesis | MetadataFold::VerdictRecorded { .. } => {}
        MetadataFold::WorkareaCreated(workarea) => {
            state.workareas.insert(workarea.workarea_id, workarea);
        }
        MetadataFold::KeyIssued(key) => {
            state.keys_by_hash.insert(key.key_hash.clone(), key);
        }
        MetadataFold::MemoryHeld { memory, enqueue } => {
            if enqueue {
                state.queue.push(QueueEntry {
                    memory_id: memory.memory_id,
                    claimed_by: None,
                });
            }
            state.memories.insert(memory.memory_id, memory);
        }
        MetadataFold::MemoryPromoted { memory_id } => {
            if let Some(memory) = state.memories.get_mut(&memory_id) {
                memory.status = MemoryStatus::Promoted;
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
            if let Some(memory) = state.memories.get_mut(&superseded) {
                memory.superseded_by = Some(successor.memory_id);
            }
            state.queue.push(QueueEntry {
                memory_id: successor.memory_id,
                claimed_by: None,
            });
            state.memories.insert(successor.memory_id, successor);
        }
    }
}

impl JournalStore for MemStore {
    async fn commit(&self, req: CommitRequest) -> Result<CommitReceipt, StoreError> {
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
        apply_fold(&mut state, req.fold);
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
        workarea_id: Option<Uuid>,
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
            .filter(|m| m.status == MemoryStatus::Promoted && m.superseded_by.is_none())
            .filter(|m| workarea_id.is_none_or(|w| m.workarea_id == w))
            .filter(|m| m.body.to_lowercase().contains(&needle))
            .take(k)
            .cloned()
            .collect())
    }

    async fn review_claim_next(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
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
            if let Some(key) = tenant
                .lock()
                .expect("tenant lock")
                .keys_by_hash
                .get(key_hash)
            {
                return Ok(Some(key.clone()));
            }
        }
        Ok(None)
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
