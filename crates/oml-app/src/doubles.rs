//! In-memory doubles for every port: the contract suites in
//! `oml-port-contracts` run against these first, and a real adapter passes
//! the identical suite (workspace-plan.md M1 build order, step 2).
//!
//! The doubles are deterministic — fixed clock, counting id generator — so
//! every test chain is reproducible byte for byte.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use serde_json::Value;
use uuid::Uuid;

use oml_domain::access::{Grant, KeyDenial};
use oml_domain::budget::{hit_order, steering_order};
use oml_domain::name::{Name, NamedKind, Naming, UNIVERSE};
use oml_domain::oauth::{
    CACHED_DOCUMENTS_MAX, ConnectionAct, ConnectionState, KeyState, RefreshDecision, RevokedCause,
    TokenKind, UNUSED_REGISTRATION_SECONDS, UNUSED_REGISTRATIONS_MAX, cache_lifetime,
    connection_transition,
};
use oml_domain::{
    ChainHead, EventTime, HeldBy, MemoryKind, MemoryStatus, Role, Sha256Hash, SignedEnvelope, WorkareaStatus,
    event_types,
    ratifier_is_proposer,
};

use crate::ports::{
    AdmissionEnvelope, ClientDocumentFetcher, Clock, CommitReceipt, CommitRequest, ConnectionStore,
    DocumentFetchFailure, FetchedDocument, HandleSealer, IdGen, IdempotencyRecord,
    IdempotencyScope, JournalStore, KeyringError, KeyringStore, MetadataFold, OutboxRow,
    ScanOutcome, Scanner, Signer, StoreError, SubjectRow, TailEvent, TailScan, claim_subject,
};
use crate::types::{
    AuthorizationRecord, ClaimKeyState, ClientDescription, ClientKind, ConnectCodeRecord,
    ConnectGrant, ConnectOutcome, ConnectionRecord, Credential, Crew, CrewRecord, DenialRecord,
    DocumentCaching, GrantRecord, HIT_KINDS, KeyAccess, KeyRecord, KeyReferences, ListingFilter,
    ListingPosition, ListingScope, MemoryRecord, NamedRef, OAuthClientRecord, PROVIDER_TENANT,
    RecallCandidate, RefreshFacts, RefreshOutcome, SweepReport, TenantRow, TokenPair, TokenRecord,
    UnnamedRows, WorkareaRecord, WorkareaRef,
    TransitionRecord, TransitionState,
};

/// The states whose versions hold their `claim_key` and their ordinal
/// (postgres-schema.md `memories_claim_version`; ADR-0025 d18).
const LIVE: [MemoryStatus; 3] = [
    MemoryStatus::Held,
    MemoryStatus::Promoted,
    MemoryStatus::Demoted,
];

/// Whether recall may serve `m` at all: a promoted served tip that is not a
/// retirement version, so no version of a retired claim qualifies
/// (retrieval.md, the recall-ordering contract; ADR-0025 d8, d11).
fn served_tip(m: &MemoryRecord) -> bool {
    m.status == MemoryStatus::Promoted && m.is_tip && !m.retirement
}

/// The distinct lower-cased alphanumeric words of `text`: this store's
/// stand-in for the `simple` text-search configuration, which stems
/// nothing (retrieval.md, text handling).
fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The rows that match `query`, as candidates in the total order of hits,
/// at most `k`: this store's score is the count of distinct query terms
/// matched (retrieval.md, the recall-ordering contract; ADR-0025 d8).
fn ranked<'a>(
    rows: impl Iterator<Item = &'a MemoryRecord>,
    query: &str,
    k: usize,
) -> Vec<RecallCandidate> {
    let terms = words(query);
    let mut hits: Vec<RecallCandidate> = rows
        .filter_map(|m| {
            let body = words(&m.body);
            let matched = terms.iter().filter(|t| body.contains(*t)).count();
            (matched > 0).then_some(RecallCandidate {
                memory_id: m.memory_id,
                body_bytes: m.body_bytes,
                score: matched as f32,
            })
        })
        .collect();
    hit_order(&mut hits, |c| (c.score, c.memory_id));
    hits.truncate(k);
    hits
}

// ---------------------------------------------------------------------------
// Journal store
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct QueueEntry {
    memory_id: Uuid,
    claimed_by: Option<Uuid>,
}

/// A workarea row migration 0006 left without a name: served to no one
/// until the lead's naming names it (postgres-schema.md names and tenants;
/// ADR-0026 d11).
#[derive(Debug, Clone)]
struct UnnamedWorkarea {
    parent_id: Option<Uuid>,
    /// The status the PoC stored: `committed` on the root, `development`
    /// on every other node, the only statuses its `workarea.create` wrote.
    status: WorkareaStatus,
    crew: Option<Crew>,
}

#[derive(Debug, Default, Clone)]
struct TenantState {
    events: Vec<crate::domain::SignedEnvelope>,
    memories: BTreeMap<Uuid, MemoryRecord>,
    workareas: BTreeMap<Uuid, WorkareaRecord>,
    unnamed_workareas: BTreeMap<Uuid, UnnamedWorkarea>,
    crews: BTreeMap<Uuid, CrewRecord>,
    /// Every key the tenant was issued, by id, revoked ones included, so
    /// their names stay held (ADR-0026 d10); a key `key.revoked` folded
    /// carries its `revoked_at` and resolves by hash no more.
    keys: BTreeMap<Uuid, KeyRecord>,
    /// Connect codes by id (postgres-schema.md `connect_codes`).
    connect_codes: BTreeMap<Uuid, ConnectCodeRecord>,
    /// Connections by id (postgres-schema.md `oauth_connections`).
    connections: BTreeMap<Uuid, ConnectionRecord>,
    /// The provider tenant's client rows by `client_id`
    /// (postgres-schema.md `oauth_clients`).
    oauth_clients: BTreeMap<String, OAuthClientRecord>,
    /// The provider tenant's authorization requests by their handle digest
    /// (postgres-schema.md `oauth_authorizations`).
    authorizations: BTreeMap<[u8; 32], AuthorizationRecord>,
    /// Token rows by their digest (postgres-schema.md `oauth_tokens`).
    tokens: BTreeMap<[u8; 32], TokenRecord>,
    /// Per key id, in commit order — which is epoch order.
    access: BTreeMap<Uuid, KeyAccess>,
    queue: Vec<QueueEntry>,
    /// Each status-ladder proposal with the seq of the event that proposed
    /// it, as `workarea_transitions.proposed_seq` holds it (ADR-0009 d3).
    transitions: BTreeMap<Uuid, (u64, TransitionRecord)>,
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
    /// The deployment's tenant namespace: each tenant's name, as `tenants`
    /// holds it (postgres-schema.md names and tenants). Locked after a
    /// tenant's own lock, never before it.
    tenant_names: Arc<Mutex<BTreeMap<Uuid, Name>>>,
    /// The event type whose next commit is refused, with the refusal, then
    /// forgotten.
    refuse_next: Arc<Mutex<Option<(&'static str, StoreError)>>>,
    /// The next idempotency lookup answers no record, then reads normally.
    miss_next_lookup: Arc<AtomicBool>,
    /// How many names `resolve_name` has looked up, across every handle.
    name_lookups: Arc<AtomicU64>,
    /// The tenants whose partitions `create_tenant_partitions` created, in
    /// creation order: a held memory's row needs its tenant's partition, as
    /// the list-partitioned `memories` refuses a row no partition holds
    /// (postgres-schema.md, the per-tenant partitions; ADR-0034 d11).
    partitions: Arc<Mutex<Vec<Uuid>>>,
    /// The next partition creation's refusal, then forgotten.
    refuse_partition: Arc<Mutex<Option<StoreError>>>,
    /// What runs before the next chain-head read answers, then forgotten.
    before_head_read: Arc<Mutex<Option<Interleaved>>>,
}

/// Work a test lands between a use case's reads and its commit: run once,
/// by [`MemStore::before_next_head_read`].
pub struct Interleaved(std::pin::Pin<Box<dyn Future<Output = ()> + Send>>);

impl std::fmt::Debug for Interleaved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Interleaved")
    }
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
        self.refuse_next_commit_of(
            event_type,
            StoreError::Backend(format!(
                "the double refused this {event_type} commit on request"
            )),
        );
        self
    }

    /// Refuses the next commit carrying an `event_type` event with `error`,
    /// writing none of its events, then commits normally again: a refusal a
    /// use-case cannot provoke through honest stores, such as a lost
    /// compare-and-set, armed on any handle mid-test.
    pub fn refuse_next_commit_of(&self, event_type: &'static str, error: StoreError) {
        *self.refuse_next.lock().expect("refusal lock") = Some((event_type, error));
    }

    /// Runs `work` before the next chain-head read answers, once: a use
    /// case reads the head just before it seals its commit, so `work`
    /// lands after the use case's other reads and before its commit, as a
    /// concurrent request's commit can. `work` may drive another use case
    /// on this store; its own head reads find nothing armed.
    pub fn before_next_head_read(&self, work: impl Future<Output = ()> + Send + 'static) {
        *self.before_head_read.lock().expect("interleave lock") = Some(Interleaved(Box::pin(work)));
    }

    /// Refuses the next `create_tenant_partitions` with `error`, creating
    /// nothing, then creates normally again: a lock timeout or a failed DDL,
    /// which the use-case cannot provoke through an honest store (ADR-0034
    /// d11).
    pub fn refuse_next_partition(&self, error: StoreError) {
        *self
            .refuse_partition
            .lock()
            .expect("partition refusal lock") = Some(error);
    }

    /// Test visibility: the tenants whose partitions were created, in
    /// creation order, a partition a failed genesis commit left included.
    #[must_use]
    pub fn partitions(&self) -> Vec<Uuid> {
        self.partitions.lock().expect("partitions lock").clone()
    }

    /// Test visibility: how many tenant rows the store holds, the rows a
    /// `tenant.genesis` or a naming gave a name.
    #[must_use]
    pub fn tenant_count(&self) -> usize {
        self.tenant_names.lock().expect("tenant names lock").len()
    }

    /// Test visibility: how many authorization-request rows the provider
    /// tenant holds (postgres-schema.md `oauth_authorizations`).
    #[must_use]
    pub fn authorization_count(&self) -> usize {
        self.tenant_existing(PROVIDER_TENANT).map_or(0, |provider| {
            provider.lock().expect("tenant lock").authorizations.len()
        })
    }

    /// Answers the next idempotency lookup with no record, whatever is
    /// stored, then reads normally again: the lookup of an attempt that ran
    /// before a concurrent attempt under the same key committed, armed on
    /// any handle after that commit.
    pub fn miss_next_idempotency_lookup(&self) {
        self.miss_next_lookup.store(true, Ordering::SeqCst);
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

    /// The pre-tenant lookup: the first tenant whose state `find` matches,
    /// with the row. The tenants are listed before any tenant's lock is
    /// taken, since Connect takes the tenants' lock under a tenant's.
    fn find_in_tenants<T>(&self, find: impl Fn(&TenantState) -> Option<T>) -> Option<TenantRow<T>> {
        let tenants: Vec<(Uuid, Arc<Mutex<TenantState>>)> = self
            .tenants
            .lock()
            .expect("tenants lock")
            .iter()
            .map(|(id, state)| (*id, Arc::clone(state)))
            .collect();
        tenants.into_iter().find_map(|(tenant_id, state)| {
            find(&state.lock().expect("tenant lock")).map(|row| TenantRow { tenant_id, row })
        })
    }

    /// Test visibility: how many outbox rows a tenant has accumulated.
    #[must_use]
    pub fn outbox_len(&self, tenant_id: Uuid) -> usize {
        self.tenant_existing(tenant_id)
            .map_or(0, |t| t.lock().expect("tenant lock").outbox.len())
    }

    /// Test visibility: how many names have been looked up through
    /// `resolve_name` on any handle, so a test can show that a refusal came
    /// before any resolution (ADR-0026 d4).
    #[must_use]
    pub fn name_lookups(&self) -> u64 {
        self.name_lookups.load(Ordering::SeqCst)
    }

    /// Holds `events` as one tenant's chain in the state migration 0006
    /// leaves a PoC store in: every event as it was signed, and a row with
    /// no name for each `tenant.genesis`, `workarea.create` and `key.issued`,
    /// with the crew objects, grants, denials and revocations the PoC folded
    /// (postgres-schema.md, "The migration that adds names"; ADR-0026 d11).
    /// A PoC `key.issued` never carried its value or its hash, so such a key
    /// authenticates nothing here. The chain must be the tenant's first.
    pub fn migrated_poc_chain(&self, events: Vec<SignedEnvelope>) -> Result<(), StoreError> {
        let Some(first) = events.first() else {
            return Err(StoreError::Backend(
                "an empty chain holds no row".to_owned(),
            ));
        };
        let tenant_id = first.envelope.tenant_id;
        let tenant = self.tenant(tenant_id);
        let mut state = tenant.lock().expect("tenant lock");
        if !state.events.is_empty() {
            return Err(StoreError::Backend(format!(
                "tenant {tenant_id} already holds a chain"
            )));
        }
        let mut staged = state.clone();
        for event in events {
            fold_poc_event(&mut staged, tenant_id, &event)?;
            staged.events.push(event);
        }
        *state = staged;
        // Migration 0009 gives every tenant it finds its partition
        // (ADR-0034 d11).
        self.partitions
            .lock()
            .expect("partitions lock")
            .push(tenant_id);
        Ok(())
    }
}

/// One payload field naming a row, as the PoC journalled it.
fn poc_id(payload: &Value, field: &str) -> Option<Uuid> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
}

fn poc_required_id(payload: &Value, field: &str) -> Result<Uuid, StoreError> {
    poc_id(payload, field).ok_or_else(|| {
        StoreError::Backend(format!(
            "a PoC event carries no {field} the double can fold"
        ))
    })
}

fn role_from_str(role: &str) -> Result<Role, StoreError> {
    [
        Role::Reader,
        Role::Writer,
        Role::Dreamer,
        Role::Admin,
        Role::Operator,
    ]
    .into_iter()
    .find(|r| r.as_str() == role)
    .ok_or_else(|| StoreError::Backend(format!("{role:?} is not a key role")))
}

/// What the PoC folded from one event, every row it creates left without a
/// name, as 0006 leaves it.
fn fold_poc_event(
    state: &mut TenantState,
    tenant_id: Uuid,
    event: &SignedEnvelope,
) -> Result<(), StoreError> {
    let env = &event.envelope;
    let payload = &env.payload;
    match env.event_type.as_str() {
        event_types::WORKAREA_CREATE => {
            let crew = payload
                .get("crew")
                .filter(|c| !c.is_null())
                .map(|c| serde_json::from_value(c.clone()))
                .transpose()
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            state.unnamed_workareas.insert(
                poc_required_id(payload, "workarea_id")?,
                UnnamedWorkarea {
                    parent_id: poc_id(payload, "parent_id"),
                    status: if poc_id(payload, "parent_id").is_none() {
                        WorkareaStatus::Committed
                    } else {
                        WorkareaStatus::Development
                    },
                    crew,
                },
            );
        }
        event_types::KEY_ISSUED => {
            let key_id = poc_required_id(payload, "key_id")?;
            let role = role_from_str(payload.get("role").and_then(Value::as_str).unwrap_or(""))?;
            state.access.insert(
                key_id,
                KeyAccess {
                    issued_seq: env.seq,
                    grants: Vec::new(),
                    denials: Vec::new(),
                    lift_epochs: Vec::new(),
                },
            );
            state.keys.insert(
                key_id,
                KeyRecord {
                    key_id,
                    tenant_id,
                    role,
                    principal: payload
                        .get("principal")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    held_by: HeldBy::Agent,
                    default_workarea: poc_id(payload, "default_workarea_id"),
                    issued_by: poc_id(payload, "issued_by_key_id"),
                    credential: Credential::Bearer,
                    key_hash: None,
                    name: None,
                    crew_id: None,
                    revoked_at: None,
                },
            );
        }
        event_types::GRANT_ISSUED => {
            let key_id = poc_required_id(payload, "key_id")?;
            let grant = Grant {
                grant_id: poc_required_id(payload, "grant_id")?,
                node: poc_required_id(payload, "workarea_node_id")?,
                role: role_from_str(payload.get("role").and_then(Value::as_str).unwrap_or(""))?,
                deny: Vec::new(),
            };
            if let Some(access) = state.access.get_mut(&key_id) {
                access.grants.push(GrantRecord {
                    grant,
                    epoch: env.seq,
                });
            }
        }
        event_types::KEY_DENIED => {
            let key_id = poc_required_id(payload, "key_id")?;
            let denial = KeyDenial {
                denial_id: poc_required_id(payload, "denial_id")?,
                node: poc_required_id(payload, "workarea_node_id")?,
            };
            if let Some(access) = state.access.get_mut(&key_id) {
                access.denials.push(DenialRecord {
                    denial,
                    created_by: env.api_key_id.unwrap_or_default(),
                    epoch: env.seq,
                });
            }
        }
        event_types::KEY_REVOKED => {
            let key_id = poc_required_id(payload, "key_id")?;
            if let Some(key) = state.keys.get_mut(&key_id) {
                key.revoked_at = Some(env.at.clone());
            }
        }
        _ => {}
    }
    Ok(())
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

/// A name its kind already holds in the namespace is refused, as the
/// unique indexes refuse it (postgres-schema.md names and tenants; ADR-0026
/// d5).
fn require_free_name<'a>(
    mut held: impl Iterator<Item = &'a Name>,
    kind: NamedKind,
    name: &Name,
) -> Result<(), StoreError> {
    if held.any(|h| h == name) {
        return Err(StoreError::NameTaken {
            kind,
            name: name.clone(),
        });
    }
    Ok(())
}

/// The root, and only the root, is named `universe`, as
/// `workareas_universe_is_root` refuses otherwise: a non-root `universe` is
/// refused with `NameTaken`, as errors.md answers it, and a root of another
/// name as the backend's (postgres-schema.md `workareas`; ADR-0026 d6).
/// The root is stored `committed` or refused as the backend's, as
/// `workareas_universe_committed` refuses it (ADR-0009 d1).
fn require_universe_root(workarea: &WorkareaRecord) -> Result<(), StoreError> {
    match (workarea.parent_id, workarea.name.as_str() == UNIVERSE) {
        (Some(_), true) => Err(StoreError::NameTaken {
            kind: NamedKind::Workarea,
            name: workarea.name.clone(),
        }),
        (None, false) => Err(StoreError::Backend(format!(
            "the root workarea {} is named universe, not {:?}",
            workarea.workarea_id,
            workarea.name.as_str()
        ))),
        // The root is permanently committed, as the schema's
        // workareas_universe_committed CHECK holds (ADR-0009 d1).
        (None, true) if workarea.status != WorkareaStatus::Committed => {
            Err(StoreError::Backend(format!(
                "the universe root {} is permanently committed, not {}",
                workarea.workarea_id,
                workarea.status.as_str()
            )))
        }
        _ => Ok(()),
    }
}

/// A workarea that has no name yet is served to no one, as the Postgres
/// store refuses it (postgres-schema.md names and tenants; ADR-0026 d11).
fn require_named(state: &TenantState, workarea_id: Uuid) -> Result<(), StoreError> {
    if state.unnamed_workareas.contains_key(&workarea_id) {
        return Err(StoreError::Backend(format!(
            "workarea {workarea_id} has no name: the lead names every row that predates names \
             before it serves (ADR-0026 d11)"
        )));
    }
    Ok(())
}

/// A read that would list a child with no name yet fails closed, as
/// [`require_named`] does for the child itself.
fn require_no_unnamed_child(state: &TenantState, parents: &[Uuid]) -> Result<(), StoreError> {
    match state
        .unnamed_workareas
        .iter()
        .find(|(_, w)| w.parent_id.is_some_and(|p| parents.contains(&p)))
    {
        Some((child, _)) => require_named(state, *child),
        None => Ok(()),
    }
}

/// A crew a row names must be its own tenant's, as the tenant-scoped
/// foreign keys onto `crews` require (postgres-schema.md `crews`).
fn require_crew(state: &TenantState, crew_id: Option<Uuid>) -> Result<(), StoreError> {
    match crew_id {
        Some(crew_id) if !state.crews.contains_key(&crew_id) => Err(StoreError::Backend(format!(
            "crew {crew_id} is not held by this tenant"
        ))),
        _ => Ok(()),
    }
}

/// A workarea's parent must be its own tenant's node, as the tenant-scoped
/// foreign key `(tenant_id, parent_id)` requires: another tenant's node is
/// refused as no node is (postgres-schema.md `workareas`; ADR-0034 d12).
fn require_parent(state: &TenantState, parent_id: Option<Uuid>) -> Result<(), StoreError> {
    match parent_id {
        Some(parent_id) if !state.workareas.contains_key(&parent_id) => Err(StoreError::Backend(
            format!("parent workarea {parent_id} is not held by this tenant"),
        )),
        _ => Ok(()),
    }
}

/// The one step forward from each rung, as `workarea_transitions_one_rung`
/// admits it (ADR-0009 d1).
fn one_rung(from: WorkareaStatus, to: WorkareaStatus) -> bool {
    matches!(
        (from, to),
        (WorkareaStatus::Development, WorkareaStatus::Committed)
            | (WorkareaStatus::Committed, WorkareaStatus::Validated)
    )
}

/// `workarea.status_proposed`'s fold: what the store refuses, the double
/// refuses — a node or proposing key the tenant does not hold, a proposer
/// principal other than the one that key's row holds, a step other than
/// one rung forward, a record that is not pending, and an id already held
/// (0016_workarea_transitions.sql; ADR-0010 R2).
fn propose_transition_fold(
    state: &mut TenantState,
    seq: u64,
    proposal: TransitionRecord,
) -> Result<(), StoreError> {
    require_held(state, proposal.proposer_key_id, proposal.workarea_id)?;
    if key_principal(state, proposal.proposer_key_id)? != proposal.proposer_principal {
        return Err(StoreError::Backend(format!(
            "transition {}'s proposer principal is not its proposing key's",
            proposal.transition_id
        )));
    }
    if !one_rung(proposal.from, proposal.to) {
        return Err(StoreError::Backend(format!(
            "a transition is one rung forward, never {} to {}",
            proposal.from.as_str(),
            proposal.to.as_str()
        )));
    }
    if proposal.state != TransitionState::Pending || proposal.ratifier_key_id.is_some() {
        return Err(StoreError::Backend(format!(
            "transition {} is proposed pending, with no ratifier",
            proposal.transition_id
        )));
    }
    if state.transitions.contains_key(&proposal.transition_id) {
        return Err(StoreError::Backend(format!(
            "transition {} is already held",
            proposal.transition_id
        )));
    }
    state
        .transitions
        .insert(proposal.transition_id, (seq, proposal));
    Ok(())
}

/// `workarea.status_transition`'s fold, as the ratifying transaction
/// applies it once the chain lock is held: a row that is not a proposal on
/// this node with this `to`, a ratifier key the tenant does not hold, and a
/// universe root moved off `committed` are defects; a row already ratified
/// or whose `from` is no longer the node's status is
/// [`StoreError::TransitionStale`]; a ratifier that is the proposing key or
/// holds the proposer's principal is [`StoreError::RatifierIsProposer`];
/// and a step to `validated` with a direct child not `validated` is
/// [`StoreError::ChildrenNotValidated`]. Any refusal refuses the commit,
/// which writes nothing (ADR-0009 d1, d3, d4; ADR-0010 R2; workareas.md,
/// ratification re-checks).
fn ratify_transition_fold(
    state: &mut TenantState,
    transition_id: Uuid,
    workarea_id: Uuid,
    to: WorkareaStatus,
    ratifier_key_id: Uuid,
) -> Result<(), StoreError> {
    let defect = || {
        StoreError::Backend(format!(
            "transition {transition_id} is not a proposal on workarea {workarea_id} to {}",
            to.as_str()
        ))
    };
    let Some((_, row)) = state.transitions.get(&transition_id) else {
        return Err(defect());
    };
    let Some(node) = state.workareas.get(&workarea_id) else {
        return Err(defect());
    };
    if row.workarea_id != workarea_id || row.to != to {
        return Err(defect());
    }
    if row.state != TransitionState::Pending || node.status != row.from {
        return Err(StoreError::TransitionStale {
            transition_id,
            workarea_id,
            from: row.from,
            to: row.to,
        });
    }
    require_held(state, ratifier_key_id, workarea_id)?;
    let ratifier_principal = key_principal(state, ratifier_key_id)?;
    if ratifier_is_proposer(
        row.proposer_key_id,
        &row.proposer_principal,
        ratifier_key_id,
        ratifier_principal,
    ) {
        return Err(StoreError::RatifierIsProposer { transition_id });
    }
    if to == WorkareaStatus::Validated && child_not_validated(state, workarea_id) {
        return Err(StoreError::ChildrenNotValidated {
            transition_id,
            workarea_id,
        });
    }
    if node.parent_id.is_none() && to != WorkareaStatus::Committed {
        return Err(StoreError::Backend(
            "the universe root is permanently committed".to_owned(),
        ));
    }
    if let Some(node) = state.workareas.get_mut(&workarea_id) {
        node.status = to;
    }
    if let Some((_, row)) = state.transitions.get_mut(&transition_id) {
        row.state = TransitionState::Ratified;
        row.ratifier_key_id = Some(ratifier_key_id);
    }
    Ok(())
}

/// The principal the tenant's key row `key_id` holds, revoked or not.
fn key_principal(state: &TenantState, key_id: Uuid) -> Result<&str, StoreError> {
    state
        .keys_by_hash
        .values()
        .find(|k| k.key_id == key_id)
        .map(|k| k.principal.as_str())
        .ok_or_else(|| StoreError::Backend(format!("key {key_id} is not held by this tenant")))
}

/// Whether any direct child of `workarea_id`, named or not, stands on a
/// rung other than `validated` (ADR-0009 d4).
fn child_not_validated(state: &TenantState, workarea_id: Uuid) -> bool {
    state
        .workareas
        .values()
        .filter(|w| w.parent_id == Some(workarea_id))
        .map(|w| w.status)
        .chain(
            state
                .unnamed_workareas
                .values()
                .filter(|w| w.parent_id == Some(workarea_id))
                .map(|w| w.status),
        )
        .any(|status| status != WorkareaStatus::Validated)
}

/// Names the tenant at its genesis, as the `tenants` row does: a tenant
/// that has its row keeps it, name included, and a name another tenant
/// holds is refused (postgres-schema.md names and tenants; ADR-0026 d1).
fn name_tenant(
    names: &mut BTreeMap<Uuid, Name>,
    tenant_id: Uuid,
    name: &Name,
) -> Result<(), StoreError> {
    if names.contains_key(&tenant_id) {
        return Ok(());
    }
    require_free_name(names.values(), NamedKind::Tenant, name)?;
    names.insert(tenant_id, name.clone());
    Ok(())
}

/// Names a tenant that predates names, as the naming's fold of its row
/// does: one that already has its name is refused, and so is a name another
/// tenant holds (ADR-0026 d11).
fn assign_tenant_name(
    names: &mut BTreeMap<Uuid, Name>,
    tenant_id: Uuid,
    name: &Name,
) -> Result<(), StoreError> {
    if names.contains_key(&tenant_id) {
        return Err(StoreError::Backend(format!(
            "tenant {tenant_id} is not a row without a name"
        )));
    }
    require_free_name(names.values(), NamedKind::Tenant, name)?;
    names.insert(tenant_id, name.clone());
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

/// Voids, in the commit of `seq`, every code of `key_id` from which no
/// connection has opened, spent or not, and not voided already
/// (roles-and-keys.md connect codes; ADR-0031 d8).
fn void_unopened_codes(state: &mut TenantState, key_id: Uuid, seq: u64) {
    let opened: BTreeSet<Uuid> = state
        .connections
        .values()
        .filter(|c| c.key_id == key_id && c.opened_seq.is_some())
        .map(|c| c.code_id)
        .collect();
    for code in state
        .connect_codes
        .values_mut()
        .filter(|c| c.key_id == key_id && c.voided_seq.is_none() && !opened.contains(&c.code_id))
    {
        code.voided_seq = Some(seq);
    }
}

/// Refuses a pair either of whose digests a token row holds already, as
/// the `oauth_tokens` primary key does (ADR-0031 d10).
fn require_free_tokens(state: &TenantState, tokens: &TokenPair) -> Result<(), StoreError> {
    if tokens.access.token_hash == tokens.refresh.token_hash
        || state.tokens.contains_key(&tokens.access.token_hash.0)
        || state.tokens.contains_key(&tokens.refresh.token_hash.0)
    {
        return Err(StoreError::Backend(
            "a token digest is held already".to_owned(),
        ));
    }
    Ok(())
}

/// Inserts a checked pair's two rows of `connection_id`, created at `at`.
fn insert_tokens(state: &mut TenantState, connection_id: Uuid, tokens: TokenPair, at: &EventTime) {
    for (minted, kind) in [
        (tokens.access, TokenKind::Access),
        (tokens.refresh, TokenKind::Refresh),
    ] {
        state.tokens.insert(
            minted.token_hash.0,
            TokenRecord {
                token_hash: minted.token_hash,
                connection_id,
                kind,
                expires_at: minted.expires_at,
                rotated_at: None,
                created_at: at.clone(),
            },
        );
    }
}

/// Whether `expires_at` has passed at `at`: a row lives while its expiry is
/// later than the instant, as the store's `expires_at > $at` reads it.
fn lapsed(expires_at: &EventTime, at: &EventTime) -> bool {
    expires_at.unix_micros() <= at.unix_micros()
}

/// Applies one fold, or refuses it before changing anything — what the
/// schema refuses, the double refuses, so the commit stays all or nothing.
/// `seq` and `at` are the commit's event seq and time: a key's
/// `issued_seq` and `revoked_at`, a grant's or denial's epoch and a
/// memory's `promoted_at`.
fn apply_fold(
    state: &mut TenantState,
    tenant_id: Uuid,
    fold: MetadataFold,
    seq: u64,
    at: &EventTime,
) -> Result<(), StoreError> {
    match fold {
        // The tenant's name is the deployment's, applied by the commit.
        MetadataFold::TenantGenesis { .. } | MetadataFold::VerdictRecorded { .. } => {}
        MetadataFold::WorkareaCreated(workarea) => {
            require_universe_root(&workarea)?;
            require_free_name(
                state.workareas.values().map(|w| &w.name),
                NamedKind::Workarea,
                &workarea.name,
            )?;
            require_crew(state, workarea.crew_id)?;
            require_parent(state, workarea.parent_id)?;
            state.workareas.insert(workarea.workarea_id, workarea);
        }
        MetadataFold::CrewCreated(crew) => {
            require_free_name(
                state.crews.values().map(|c| &c.name),
                NamedKind::Crew,
                &crew.name,
            )?;
            state.crews.insert(crew.crew_id, crew);
        }
        MetadataFold::CrewUpdated(crew) => {
            let Some(held) = state.crews.get_mut(&crew.crew_id) else {
                return Err(StoreError::Backend(format!(
                    "crew {} is not held by this tenant",
                    crew.crew_id
                )));
            };
            if held.name != crew.name {
                return Err(StoreError::Backend(format!(
                    "crew {}'s name never changes",
                    crew.crew_id
                )));
            }
            held.charter_uri = crew.charter_uri;
            held.external_manager = crew.external_manager;
        }
        MetadataFold::WorkareaCrewUpdated {
            workarea_id,
            crew_id,
        } => {
            require_crew(state, Some(crew_id))?;
            let Some(workarea) = state.workareas.get_mut(&workarea_id) else {
                return Err(StoreError::Backend(format!(
                    "workarea {workarea_id} is not held by this tenant"
                )));
            };
            workarea.crew_id = Some(crew_id);
        }
        MetadataFold::NamesAssigned(naming) => {
            assign_names_fold(state, tenant_id, &naming)?;
        }
        MetadataFold::KeyIssued(key) => {
            // api_keys_connector_has_no_hash (ADR-0031 d7).
            if key.credential == Credential::Connection && key.key_hash.is_some() {
                return Err(StoreError::Backend(format!(
                    "connector key {} carries a hash; a connector key has no value",
                    key.key_id
                )));
            }
            if let Some(name) = &key.name {
                require_free_name(
                    state.keys.values().filter_map(|k| k.name.as_ref()),
                    NamedKind::Key,
                    name,
                )?;
            }
            require_crew(state, key.crew_id)?;
            state.access.insert(
                key.key_id,
                KeyAccess {
                    issued_seq: seq,
                    grants: Vec::new(),
                    denials: Vec::new(),
                    lift_epochs: Vec::new(),
                },
            );
            state.keys.insert(key.key_id, key);
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
        // The lifted denial leaves the denials in force; a denial the key
        // does not hold in force, an already lifted one included, is
        // refused, as the store's guarded update refuses it.
        MetadataFold::KeyDenialLifted { key_id, denial_id } => {
            let held = state.access.get_mut(&key_id).and_then(|access| {
                let at = access
                    .denials
                    .iter()
                    .position(|d| d.denial.denial_id == denial_id)?;
                Some((access, at))
            });
            let Some((access, at)) = held else {
                return Err(StoreError::Backend(format!(
                    "denial {denial_id} is not a denial in force on key {key_id} of this tenant"
                )));
            };
            access.denials.remove(at);
            access.lift_epochs.push(seq);
        }
        // The row stays as history, stamped with the event's `at`, as the
        // store's guarded update stamps it; a key the tenant does not hold,
        // or one already revoked, is refused.
        MetadataFold::KeyRevoked { key_id } => {
            let Some(key) = state
                .keys
                .get_mut(&key_id)
                .filter(|k| k.revoked_at.is_none())
            else {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a live key of this tenant"
                )));
            };
            key.revoked_at = Some(at.clone());
            // Its unredeemed codes are voided in this commit (ADR-0031 d8).
            for code in state
                .connect_codes
                .values_mut()
                .filter(|c| c.key_id == key_id && c.redeemed_at.is_none() && c.voided_seq.is_none())
            {
                code.voided_seq = Some(seq);
            }
        }
        MetadataFold::ConnectCodeIssued { code, voids } => {
            if !state.keys.contains_key(&code.key_id) {
                return Err(StoreError::Backend(format!(
                    "key {} is not held by this tenant",
                    code.key_id
                )));
            }
            if state.connect_codes.contains_key(&code.code_id)
                || state
                    .connect_codes
                    .values()
                    .any(|c| c.code_hash == code.code_hash)
            {
                return Err(StoreError::Backend(format!(
                    "connect code {} is held already",
                    code.code_id
                )));
            }
            let unredeemed = state
                .connect_codes
                .values()
                .find(|c| {
                    c.key_id == code.key_id && c.redeemed_at.is_none() && c.voided_seq.is_none()
                })
                .map(|c| c.code_id);
            if unredeemed != voids {
                return Err(StoreError::ConnectCodeMoved {
                    key_id: code.key_id,
                });
            }
            void_unopened_codes(state, code.key_id, seq);
            state.connect_codes.insert(
                code.code_id,
                ConnectCodeRecord {
                    code_id: code.code_id,
                    key_id: code.key_id,
                    code_hash: code.code_hash,
                    expires_at: code.expires_at,
                    issued_seq: seq,
                    redeemed_at: None,
                    voided_seq: None,
                },
            );
        }
        MetadataFold::ConnectionOpened {
            connection_id,
            replaces,
            auth_code_hash,
            tokens,
        } => {
            // Every refusal of an opening is one typed refusal, as the
            // store's conditional update is one statement (ADR-0031 d10).
            let not_openable = StoreError::ConnectionNotOpenable { connection_id };
            let Some(connection) = state.connections.get(&connection_id) else {
                return Err(not_openable);
            };
            let (key_id, code_id) = (connection.key_id, connection.code_id);
            if connection_transition(connection.state, ConnectionAct::Exchange).is_err() {
                return Err(not_openable);
            }
            // The exchange's update matches the row by the presented code's
            // digest (postgres-schema.md `oauth_connections`; ADR-0031 d10).
            if connection.auth_code_hash != Some(auth_code_hash) {
                return Err(not_openable);
            }
            // The exchange redeems the authorization code once and before
            // its expiry, as the store's conditional update does
            // (postgres-schema.md `oauth_connections`; ADR-0031 d10).
            if connection.auth_code_redeemed_at.is_some()
                || connection
                    .auth_code_expires_at
                    .as_ref()
                    .is_none_or(|expires| expires.as_str() <= at.as_str())
            {
                return Err(not_openable);
            }
            // Connect takes no chain lock, so the code's voiding is read
            // here, under it: a mint or revocation since Connect ends the
            // connection at the exchange (ADR-0031 d8).
            if state
                .connect_codes
                .get(&code_id)
                .is_none_or(|c| c.voided_seq.is_some())
            {
                return Err(not_openable);
            }
            // The key's state is read under the chain lock, so no
            // connection opens after its key's key.revoked (ADR-0031 d11).
            if !state
                .keys
                .get(&key_id)
                .is_some_and(|k| k.revoked_at.is_none())
            {
                return Err(not_openable);
            }
            let open = state
                .connections
                .values()
                .find(|c| c.key_id == key_id && c.state == ConnectionState::Open)
                .map(|c| c.connection_id);
            if open != replaces {
                return Err(not_openable);
            }
            require_free_tokens(state, &tokens)?;
            if let Some(earlier) = open.and_then(|id| state.connections.get_mut(&id)) {
                earlier.state = ConnectionState::Revoked(RevokedCause::Replaced);
                earlier.revoked_seq = Some(seq);
            }
            if let Some(connection) = state.connections.get_mut(&connection_id) {
                connection.state = ConnectionState::Open;
                connection.opened_seq = Some(seq);
                connection.auth_code_redeemed_at = Some(at.clone());
            }
            insert_tokens(state, connection_id, tokens, at);
        }
        MetadataFold::ConnectionRevoked {
            connection_id,
            cause,
        } => {
            let act = match cause {
                RevokedCause::Admin => ConnectionAct::AdminRevoke,
                RevokedCause::RefreshReuse => ConnectionAct::RefreshReuse,
                RevokedCause::CodeReuse => ConnectionAct::CodeReuse,
                RevokedCause::Replaced => {
                    return Err(StoreError::Backend(
                        "only a newer connection.opened replaces a connection".to_owned(),
                    ));
                }
            };
            let Some(connection) = state.connections.get_mut(&connection_id) else {
                return Err(StoreError::Backend(format!(
                    "connection {connection_id} is not held by this tenant"
                )));
            };
            connection.state = connection_transition(connection.state, act)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            connection.revoked_seq = Some(seq);
            let key_id = connection.key_id;
            void_unopened_codes(state, key_id, seq);
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
                // One served tip per claim: any other tip refuses the
                // commit, as `memories_served_tip` does (ADR-0025 d10).
                if state.memories.values().any(|m| {
                    m.memory_id != memory_id
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
        MetadataFold::TipSuperseded {
            successor,
            replaced,
        } => {
            // The compare-and-set: the successor is held and replaces
            // `replaced`, which is still the promoted tip under the
            // successor's own workarea and claim_key; anything else writes
            // nothing (ADR-0025 d10).
            let cas = match (
                state.memories.get(&successor),
                state.memories.get(&replaced),
            ) {
                (Some(s), Some(t)) => {
                    s.status == MemoryStatus::Held
                        && s.replaces == Some(replaced)
                        && t.workarea_id == s.workarea_id
                        && t.claim_key == s.claim_key
                        && t.is_tip
                        && t.status == MemoryStatus::Promoted
                }
                _ => false,
            };
            if !cas {
                return Err(StoreError::TipMoved { replaced });
            }
            if let Some(target) = state.memories.get_mut(&replaced) {
                target.is_tip = false;
            }
            if let Some(memory) = state.memories.get_mut(&successor) {
                memory.status = MemoryStatus::Promoted;
                memory.is_tip = true;
                memory.promoted_at = Some(at.clone());
            }
            state.queue.retain(|e| e.memory_id != successor);
        }
        MetadataFold::TransitionProposed(proposal) => {
            propose_transition_fold(state, seq, proposal)?;
        }
        MetadataFold::TransitionRatified {
            transition_id,
            workarea_id,
            to,
            ratifier_key_id,
        } => {
            ratify_transition_fold(state, transition_id, workarea_id, to, ratifier_key_id)?;
        }
    }
    Ok(())
}

/// The fold of `system.names_assigned` within the tenant: each crew record,
/// then each listed workarea and key, which must hold no name yet, named as
/// the event assigns, each workarea pointing at the crew it is listed under.
/// The tenant's own name is the deployment's, applied by the commit
/// (postgres-schema.md folds; ADR-0026 d11).
fn assign_names_fold(
    state: &mut TenantState,
    tenant_id: Uuid,
    naming: &Naming,
) -> Result<(), StoreError> {
    let mut crew_of = BTreeMap::new();
    for crew in &naming.crews {
        require_free_name(
            state.crews.values().map(|c| &c.name),
            NamedKind::Crew,
            &crew.name,
        )?;
        state.crews.insert(
            crew.crew_id,
            CrewRecord {
                crew_id: crew.crew_id,
                tenant_id,
                name: crew.name.clone(),
                charter_uri: crew.charter_uri.clone(),
                external_manager: crew.external_manager.clone(),
            },
        );
        for workarea_id in &crew.workarea_ids {
            crew_of.insert(*workarea_id, crew.crew_id);
        }
    }
    for assignment in &naming.names {
        let id = assignment.id;
        match assignment.kind {
            NamedKind::Tenant if id == tenant_id => {}
            NamedKind::Workarea => {
                let Some(unnamed) = state.unnamed_workareas.remove(&id) else {
                    return Err(StoreError::Backend(format!(
                        "workarea {id} is not a row without a name"
                    )));
                };
                let workarea = WorkareaRecord {
                    workarea_id: id,
                    tenant_id,
                    parent_id: unnamed.parent_id,
                    name: assignment.name.clone(),
                    status: unnamed.status,
                    crew: unnamed.crew,
                    crew_id: crew_of.remove(&id),
                };
                require_universe_root(&workarea)?;
                require_free_name(
                    state.workareas.values().map(|w| &w.name),
                    NamedKind::Workarea,
                    &workarea.name,
                )?;
                state.workareas.insert(id, workarea);
            }
            NamedKind::Key => {
                require_free_name(
                    state.keys.values().filter_map(|k| k.name.as_ref()),
                    NamedKind::Key,
                    &assignment.name,
                )?;
                let Some(key) = state.keys.get_mut(&id).filter(|k| k.name.is_none()) else {
                    return Err(StoreError::Backend(format!(
                        "key {id} is not a row without a name"
                    )));
                };
                key.name = Some(assignment.name.clone());
            }
            kind => {
                return Err(StoreError::Backend(format!(
                    "{} {id} is not a row of this tenant's chain to name",
                    kind.as_str()
                )));
            }
        }
    }
    if let Some((workarea_id, _)) = crew_of.pop_first() {
        return Err(StoreError::Backend(format!(
            "a crew record names workarea {workarea_id}, which the event does not name"
        )));
    }
    Ok(())
}

/// Appends one event to `state` if it extends the head, with its fold,
/// idempotency record and outbox row, or refuses it.
fn append(state: &mut TenantState, req: CommitRequest) -> Result<CommitReceipt, StoreError> {
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
    apply_fold(
        state,
        req.event.envelope.tenant_id,
        req.fold,
        expected_seq,
        &req.event.envelope.at,
    )?;
    if let Some(put) = req.idempotency {
        // One scope holds one record; the caller drops the staged state on
        // a refusal, so the fold above is undone with it.
        let scope = (put.scope.key_id, put.scope.workarea_id, put.scope.idem_key);
        if state.idempotency.contains_key(&scope) {
            return Err(StoreError::IdempotencyRecorded);
        }
        state.idempotency.insert(scope, put.record);
    }
    state.outbox.push(req.outbox);
    let receipt = CommitReceipt {
        seq: req.event.envelope.seq,
        hash: req.event.hash,
    };
    state.events.push(req.event);
    Ok(receipt)
}

impl JournalStore for MemStore {
    async fn commit_chain(&self, reqs: Vec<CommitRequest>) -> Result<CommitReceipt, StoreError> {
        {
            let mut refuse = self.refuse_next.lock().expect("refusal lock");
            if refuse
                .as_ref()
                .is_some_and(|(t, _)| reqs.iter().any(|r| r.event.envelope.event_type == *t))
                && let Some((_, error)) = refuse.take()
            {
                return Err(error);
            }
        }
        // A held memory's row needs its tenant's partition (ADR-0034 d11).
        if let Some(req) = reqs.iter().find(|r| {
            matches!(r.fold, MetadataFold::MemoryHeld { .. })
                && !self
                    .partitions
                    .lock()
                    .expect("partitions lock")
                    .contains(&r.event.envelope.tenant_id)
        }) {
            return Err(StoreError::Backend(format!(
                "no partition of memories holds tenant {}",
                req.event.envelope.tenant_id
            )));
        }
        // The deployment's tenant names, which a genesis and a naming give.
        let tenant_names: Vec<(bool, Name)> = reqs
            .iter()
            .filter_map(|r| match &r.fold {
                MetadataFold::TenantGenesis { name } => Some((true, name.clone())),
                MetadataFold::NamesAssigned(naming) => naming
                    .names
                    .iter()
                    .find(|a| a.kind == NamedKind::Tenant)
                    .map(|a| (false, a.name.clone())),
                _ => None,
            })
            .collect();
        let mut reqs = reqs.into_iter();
        let Some(first) = reqs.next() else {
            return Err(StoreError::Backend(
                "an empty chain commits nothing".to_owned(),
            ));
        };
        let tenant_id = first.event.envelope.tenant_id;
        let tenant = self.tenant(tenant_id);
        // The per-tenant lock IS the serialization point the real adapter
        // provides with a row lock on the chain head (ADR-0002). The links
        // apply to a copy that replaces the state only once every one has
        // applied, so a refused link leaves the earlier ones unwritten.
        let mut state = tenant.lock().expect("tenant lock");
        let mut staged = state.clone();
        let mut receipt = append(&mut staged, first)?;
        for req in reqs {
            if req.event.envelope.tenant_id != tenant_id {
                return Err(StoreError::Backend(
                    "a chain commit names one tenant".to_owned(),
                ));
            }
            receipt = append(&mut staged, req)?;
        }
        if !tenant_names.is_empty() {
            let mut names = self.tenant_names.lock().expect("tenant names lock");
            let mut staged_names = names.clone();
            for (genesis, name) in &tenant_names {
                if *genesis {
                    name_tenant(&mut staged_names, tenant_id, name)?;
                } else {
                    assign_tenant_name(&mut staged_names, tenant_id, name)?;
                }
            }
            *names = staged_names;
        }
        *state = staged;
        Ok(receipt)
    }

    async fn create_tenant_partitions(&self, tenant_id: Uuid) -> Result<(), StoreError> {
        if let Some(error) = self
            .refuse_partition
            .lock()
            .expect("partition refusal lock")
            .take()
        {
            return Err(error);
        }
        let mut partitions = self.partitions.lock().expect("partitions lock");
        if partitions.contains(&tenant_id) {
            // The relation exists already, as CREATE TABLE refuses it.
            return Err(StoreError::Partition(format!(
                "tenant {tenant_id} already has its partition"
            )));
        }
        partitions.push(tenant_id);
        Ok(())
    }

    async fn chain_head(&self, tenant_id: Uuid) -> Result<Option<ChainHead>, StoreError> {
        let interleaved = self
            .before_head_read
            .lock()
            .expect("interleave lock")
            .take();
        if let Some(Interleaved(work)) = interleaved {
            work.await;
        }
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

    async fn events_with_subjects(
        &self,
        tenant_id: Uuid,
        from_seq: u64,
        limit: usize,
    ) -> Result<TailScan, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(TailScan::default());
        };
        // One lock is one snapshot: the head and the scan agree.
        let state = tenant.lock().expect("tenant lock");
        let events = state
            .events
            .iter()
            .filter(|e| e.envelope.seq >= from_seq)
            .take(limit)
            .map(|e| TailEvent {
                subject: claim_subject(&e.envelope.event_type, &e.envelope.payload)
                    .and_then(|id| state.memories.get(&id))
                    .map(|m| SubjectRow {
                        artifact_id: m.memory_id,
                        workarea_id: m.workarea_id,
                        author_key_id: m.author_key_id,
                    }),
                event: e.clone(),
            })
            .collect();
        Ok(TailScan {
            head_seq: state.events.last().map_or(0, |e| e.envelope.seq),
            events,
        })
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

    async fn memories(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(memory_ids
            .iter()
            .filter_map(|id| state.memories.get(id).cloned())
            .collect())
    }

    async fn recall_fts(
        &self,
        tenant_id: Uuid,
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> Result<Vec<RecallCandidate>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(ranked(
            state
                .memories
                .values()
                .filter(|m| served_tip(m) && HIT_KINDS.contains(&m.kind) && kinds.contains(&m.kind))
                .filter(|m| readable.contains(&m.workarea_id)),
            query,
            k,
        ))
    }

    async fn recall_own_held(
        &self,
        tenant_id: Uuid,
        author_key_ids: &[Uuid],
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> Result<Vec<RecallCandidate>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        // The state and the author predicate are this read's own, never the
        // caller's to forget (ADR-0025 d5).
        Ok(ranked(
            state
                .memories
                .values()
                .filter(|m| {
                    m.status == MemoryStatus::Held && author_key_ids.contains(&m.author_key_id)
                })
                .filter(|m| !m.retirement && HIT_KINDS.contains(&m.kind) && kinds.contains(&m.kind))
                .filter(|m| readable.contains(&m.workarea_id)),
            query,
            k,
        ))
    }

    async fn steering_in_force(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        // A promoted version always carries the `promoted_at` its fold
        // stamped, so no row in force is passed over here.
        let mut rows: Vec<(u32, EventTime, MemoryRecord)> = state
            .memories
            .values()
            .filter(|m| served_tip(m) && m.kind == MemoryKind::Steering)
            .filter_map(|m| {
                let depth = nodes.iter().position(|n| *n == m.workarea_id)?;
                Some((
                    u32::try_from(depth).unwrap_or(u32::MAX),
                    m.promoted_at.clone()?,
                    m.clone(),
                ))
            })
            .collect();
        steering_order(&mut rows, |(depth, at, m)| (*depth, at, m.memory_id));
        Ok(rows.into_iter().map(|(_, _, m)| m).collect())
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

    async fn claim_tip(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state
            .memories
            .values()
            .find(|m| {
                m.workarea_id == workarea_id
                    && m.claim_key == claim_key
                    && m.is_tip
                    && m.status == MemoryStatus::Promoted
            })
            .cloned())
    }

    async fn held_successors(
        &self,
        tenant_id: Uuid,
        replaced: Uuid,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let mut held: Vec<MemoryRecord> = state
            .memories
            .values()
            .filter(|m| m.status == MemoryStatus::Held && m.replaces == Some(replaced))
            .cloned()
            .collect();
        held.sort_by(|a, b| {
            (a.created_at.as_str(), a.memory_id).cmp(&(b.created_at.as_str(), b.memory_id))
        });
        Ok(held)
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
            // Nor a held successor whose target has a held rival this
            // reviewer authored (roles-and-keys.md rival successors;
            // ADR-0025 d10).
            if memory.replaces.is_some()
                && memories.values().any(|r| {
                    r.memory_id != memory.memory_id
                        && r.status == MemoryStatus::Held
                        && r.replaces == memory.replaces
                        && r.workarea_id == memory.workarea_id
                        && r.claim_key == memory.claim_key
                        && (r.author_key_id == reviewer_key_id
                            || r.author_principal == reviewer_principal)
                })
            {
                continue;
            }
            entry.claimed_by = Some(reviewer_key_id);
            return Ok(Some(memory.clone()));
        }
        Ok(None)
    }

    async fn review_claimed(
        &self,
        tenant_id: Uuid,
        reviewer_key_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state
            .queue
            .iter()
            .filter(|e| e.claimed_by == Some(reviewer_key_id))
            .map(|e| e.memory_id)
            .take(2)
            .collect())
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
            // One equality lookup on a hash: a connector key has none.
            if let Some(key) = state
                .keys
                .values()
                .find(|k| k.key_hash.as_deref() == Some(key_hash))
            {
                if key.revoked_at.is_some() {
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

    async fn resolve_name(
        &self,
        tenant_id: Uuid,
        kind: NamedKind,
        name: &Name,
    ) -> Result<Option<Uuid>, StoreError> {
        self.name_lookups.fetch_add(1, Ordering::SeqCst);
        // This store keeps operator keys under the provider tenant; their
        // names are the provider's namespace, which no tenant read consults
        // (ADR-0026 d4).
        if tenant_id == PROVIDER_TENANT {
            return Ok(None);
        }
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(match kind {
            NamedKind::Workarea => state
                .workareas
                .values()
                .find(|w| &w.name == name)
                .map(|w| w.workarea_id),
            NamedKind::Crew => state
                .crews
                .values()
                .find(|c| &c.name == name)
                .map(|c| c.crew_id),
            NamedKind::Key => state
                .keys
                .values()
                .find(|k| k.name.as_ref() == Some(name))
                .map(|k| k.key_id),
            // A tenant's name is in the deployment's namespace.
            NamedKind::Tenant => None,
        })
    }

    async fn crew(&self, tenant_id: Uuid, crew_id: Uuid) -> Result<Option<CrewRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state.crews.get(&crew_id).cloned())
    }

    async fn crews(&self, tenant_id: Uuid) -> Result<Vec<CrewRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let mut crews: Vec<CrewRecord> = state.crews.values().cloned().collect();
        // `str` orders by its bytes.
        crews.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        Ok(crews)
    }

    async fn key(&self, tenant_id: Uuid, key_id: Uuid) -> Result<Option<KeyRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state.keys.get(&key_id).cloned())
    }

    async fn key_references(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Option<KeyReferences>, StoreError> {
        let Some(key) = self.key(tenant_id, key_id).await? else {
            return Ok(None);
        };
        let tenant_name = self
            .tenant_names
            .lock()
            .expect("tenant names lock")
            .get(&tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::Backend(format!("tenant {tenant_id} has no name")))?;
        // The walk and the crew read are the port's own: the double answers
        // what they answer, where the adapter reads all three in one query.
        let default_workarea = match key.default_workarea {
            Some(workarea_id) => {
                let ancestry = self
                    .workarea_ancestry(tenant_id, workarea_id)
                    .await?
                    .unwrap_or_default();
                let path = ancestry
                    .iter()
                    .rev()
                    .map(|w| w.name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                // A default the tenant does not hold fails closed, as the
                // adapter's read does.
                let node = ancestry.into_iter().next().ok_or_else(|| {
                    StoreError::Backend(format!(
                        "key {key_id}'s default workarea {workarea_id} is not this tenant's"
                    ))
                })?;
                Some(WorkareaRef {
                    id: node.workarea_id,
                    name: node.name,
                    path,
                })
            }
            None => None,
        };
        let crew = match key.crew_id {
            Some(crew_id) => self.crew(tenant_id, crew_id).await?.map(|c| NamedRef {
                id: c.crew_id,
                name: c.name,
            }),
            None => None,
        };
        Ok(Some(KeyReferences {
            tenant: NamedRef {
                id: tenant_id,
                name: tenant_name,
            },
            default_workarea,
            crew,
        }))
    }

    async fn unnamed_rows(&self, tenant_id: Uuid) -> Result<UnnamedRows, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(UnnamedRows::default());
        };
        let state = tenant.lock().expect("tenant lock");
        let mut keys: Vec<Uuid> = state
            .keys
            .values()
            .filter(|k| k.name.is_none())
            .map(|k| k.key_id)
            .collect();
        keys.sort_unstable();
        Ok(UnnamedRows {
            tenant: !state.events.is_empty()
                && !self
                    .tenant_names
                    .lock()
                    .expect("tenant names lock")
                    .contains_key(&tenant_id),
            workareas: state.unnamed_workareas.keys().copied().collect(),
            keys,
        })
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
        require_named(&state, workarea_id)?;
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
        require_named(&state, workarea_id)?;
        let Some(node) = state.workareas.get(&workarea_id) else {
            return Ok(None);
        };
        let mut ancestry = vec![node.clone()];
        let mut parent = node.parent_id;
        while let Some(parent_id) = parent {
            require_named(&state, parent_id)?;
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
        require_named(&state, workarea_id)?;
        let Some(node) = state.workareas.get(&workarea_id) else {
            return Ok(Vec::new());
        };
        let mut subtree = vec![node.clone()];
        let mut depth = vec![workarea_id];
        while !depth.is_empty() {
            require_no_unnamed_child(&state, &depth)?;
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
        require_no_unnamed_child(&state, &[workarea_id])?;
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

    async fn transition(
        &self,
        tenant_id: Uuid,
        transition_id: Uuid,
    ) -> Result<Option<TransitionRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state
            .transitions
            .get(&transition_id)
            .map(|(_, row)| row.clone()))
    }

    async fn pending_transitions(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Vec<TransitionRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let mut pending: Vec<&(u64, TransitionRecord)> = state
            .transitions
            .values()
            .filter(|(_, row)| {
                row.workarea_id == workarea_id && row.state == TransitionState::Pending
            })
            .collect();
        pending.sort_by_key(|(seq, _)| *seq);
        Ok(pending.into_iter().map(|(_, row)| row.clone()).collect())
    }

    async fn idempotency_get(
        &self,
        scope: &IdempotencyScope,
    ) -> Result<Option<IdempotencyRecord>, StoreError> {
        if self.miss_next_lookup.swap(false, Ordering::SeqCst) {
            return Ok(None);
        }
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

impl ConnectionStore for MemStore {
    async fn code_by_hash(
        &self,
        code_hash: &Sha256Hash,
    ) -> Result<Option<TenantRow<ConnectCodeRecord>>, StoreError> {
        Ok(self.find_in_tenants(|state| {
            state
                .connect_codes
                .values()
                .find(|c| c.code_hash == *code_hash)
                .cloned()
        }))
    }

    async fn take_authorization_by_handle(
        &self,
        handle_hash: &Sha256Hash,
    ) -> Result<Option<AuthorizationRecord>, StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        Ok(provider.authorizations.remove(&handle_hash.0))
    }

    async fn connection_by_auth_code(
        &self,
        auth_code_hash: &Sha256Hash,
    ) -> Result<Option<TenantRow<ConnectionRecord>>, StoreError> {
        Ok(self.find_in_tenants(|state| {
            state
                .connections
                .values()
                .find(|c| c.auth_code_hash == Some(*auth_code_hash))
                .cloned()
        }))
    }

    async fn token_by_hash(
        &self,
        token_hash: &Sha256Hash,
        kind: TokenKind,
    ) -> Result<Option<TenantRow<TokenRecord>>, StoreError> {
        Ok(self.find_in_tenants(|state| {
            state
                .tokens
                .get(&token_hash.0)
                .filter(|t| t.kind == kind)
                .cloned()
        }))
    }

    async fn insert_token_pair(
        &self,
        tenant_id: Uuid,
        connection_id: Uuid,
        tokens: TokenPair,
        at: &EventTime,
    ) -> Result<(), StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Err(StoreError::Backend(format!(
                "connection {connection_id} is not an open connection of this tenant"
            )));
        };
        let mut state = tenant.lock().expect("tenant lock");
        if state
            .connections
            .get(&connection_id)
            .is_none_or(|c| c.state != ConnectionState::Open)
        {
            return Err(StoreError::Backend(format!(
                "connection {connection_id} is not an open connection of this tenant"
            )));
        }
        require_free_tokens(&state, &tokens)?;
        insert_tokens(&mut state, connection_id, tokens, at);
        Ok(())
    }

    async fn rotate_refresh<F>(
        &self,
        tenant_id: Uuid,
        token_hash: &Sha256Hash,
        next: TokenPair,
        at: &EventTime,
        decide: F,
    ) -> Result<RefreshOutcome, StoreError>
    where
        F: FnOnce(Option<&RefreshFacts>) -> RefreshDecision + Send,
    {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(RefreshOutcome {
                decision: decide(None),
                facts: None,
            });
        };
        // The tenant's lock stands for the connection's row lock: the
        // facts, the decision and the conditional update are one step.
        let mut state = tenant.lock().expect("tenant lock");
        let facts = state
            .tokens
            .get(&token_hash.0)
            .filter(|t| t.kind == TokenKind::Refresh)
            .and_then(|token| {
                let connection = state.connections.get(&token.connection_id)?;
                let key = match state.keys.get(&connection.key_id) {
                    Some(k) if k.revoked_at.is_none() => KeyState::Active,
                    _ => KeyState::Revoked,
                };
                Some(RefreshFacts {
                    connection_id: connection.connection_id,
                    connection: connection.state,
                    client_id: connection.client_id.clone(),
                    resource: connection.resource.clone(),
                    key,
                    rotated: token.rotated_at.is_some(),
                    expired: lapsed(&token.expires_at, at),
                })
            });
        let decision = decide(facts.as_ref());
        if decision == RefreshDecision::Rotate {
            let Some(facts) = facts.as_ref() else {
                return Err(StoreError::Backend(
                    "a refresh decided to rotate a token no row holds".to_owned(),
                ));
            };
            // The conditional update: unrotated and unexpired at `at`.
            let matched = state
                .tokens
                .get(&token_hash.0)
                .is_some_and(|t| t.rotated_at.is_none() && !lapsed(&t.expires_at, at));
            if !matched {
                return Err(StoreError::Backend(
                    "the refresh token moved under its connection's lock".to_owned(),
                ));
            }
            require_free_tokens(&state, &next)?;
            if let Some(token) = state.tokens.get_mut(&token_hash.0) {
                token.rotated_at = Some(at.clone());
            }
            insert_tokens(&mut state, facts.connection_id, next, at);
        }
        Ok(RefreshOutcome { decision, facts })
    }

    async fn sweep_expired(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
    ) -> Result<SweepReport, StoreError> {
        const DAY_MICROS: i64 = 24 * 3_600 * 1_000_000;
        let mut report = SweepReport::default();
        if let Some(tenant) = self.tenant_existing(tenant_id) {
            let mut state = tenant.lock().expect("tenant lock");
            let before = state.connections.len();
            state.connections.retain(|_, c| {
                c.state != ConnectionState::Authorized
                    || c.auth_code_expires_at
                        .as_ref()
                        .is_none_or(|expires| !lapsed(expires, at))
            });
            report.connections_removed = (before - state.connections.len()) as u64;
            let revoked: BTreeSet<Uuid> = state
                .connections
                .values()
                .filter(|c| matches!(c.state, ConnectionState::Revoked(_)))
                .map(|c| c.connection_id)
                .collect();
            let before = state.tokens.len();
            state
                .tokens
                .retain(|_, t| !lapsed(&t.expires_at, at) && !revoked.contains(&t.connection_id));
            report.tokens_removed = (before - state.tokens.len()) as u64;
            for connection in state.connections.values_mut().filter(|c| {
                matches!(c.state, ConnectionState::Revoked(_)) && c.auth_code_hash.is_some()
            }) {
                connection.auth_code_hash = None;
                report.codes_cleared += 1;
            }
        }
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        let before = provider.authorizations.len();
        provider
            .authorizations
            .retain(|_, a| a.expires_at.unix_micros() + DAY_MICROS > at.unix_micros());
        report.authorizations_removed = (before - provider.authorizations.len()) as u64;
        Ok(report)
    }

    async fn tenant_name(&self, tenant_id: Uuid) -> Result<Option<Name>, StoreError> {
        Ok(self
            .tenant_names
            .lock()
            .expect("tenant names lock")
            .get(&tenant_id)
            .cloned())
    }

    async fn spend_code(
        &self,
        tenant_id: Uuid,
        code_id: Uuid,
        at: &EventTime,
    ) -> Result<bool, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(false);
        };
        let mut state = tenant.lock().expect("tenant lock");
        // The one conditional update two racing entries cannot both pass.
        match state.connect_codes.get_mut(&code_id).filter(|c| {
            c.redeemed_at.is_none() && c.voided_seq.is_none() && !lapsed(&c.expires_at, at)
        }) {
            Some(code) => {
                code.redeemed_at = Some(at.clone());
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn connection_connect(
        &self,
        handle_hash: &Sha256Hash,
        grant: ConnectGrant,
    ) -> Result<ConnectOutcome, StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        let Some(request) = provider.authorizations.get(&handle_hash.0).cloned() else {
            return Ok(ConnectOutcome::NoRequest);
        };
        // The code tenant's lock is taken after the provider tenant's, and
        // no other path holds both; a code under the provider tenant is
        // one the code tenant does not hold.
        let tenant_id = request.code_tenant_id;
        let refused = |why: &str| {
            Err(StoreError::Backend(format!(
                "connection {} {why}",
                grant.connection_id
            )))
        };
        if tenant_id == PROVIDER_TENANT {
            return refused("names a code under the provider tenant");
        }
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return refused("names a key or code its tenant does not hold");
        };
        let mut state = tenant.lock().expect("tenant lock");
        // Every check before any write, so a refusal changes nothing, as
        // the store's rolled-back transaction does.
        match state.connect_codes.get(&request.code_id) {
            None => return refused("names a key or code its tenant does not hold"),
            Some(code) if code.voided_seq.is_some() => return Ok(ConnectOutcome::CodeVoided),
            Some(_) => {}
        }
        if !state.keys.contains_key(&request.key_id) {
            return refused("names a key or code its tenant does not hold");
        }
        if state.connections.contains_key(&grant.connection_id)
            || state
                .connections
                .values()
                .any(|c| c.code_id == request.code_id)
        {
            return refused("is a second connection of its code");
        }
        let connection = ConnectionRecord {
            connection_id: grant.connection_id,
            key_id: request.key_id,
            code_id: request.code_id,
            client_id: request.client_id.clone(),
            client_kind: request.client_kind,
            client_metadata_digest: request.metadata_digest,
            redirect_uri: request.redirect_uri,
            redirect_uri_sent: request.redirect_uri_sent,
            resource: grant.resource,
            code_challenge: request.code_challenge,
            auth_code_hash: Some(grant.auth_code_hash),
            auth_code_expires_at: Some(grant.auth_code_expires_at),
            auth_code_redeemed_at: None,
            state: ConnectionState::Authorized,
            opened_seq: None,
            revoked_seq: None,
            created_at: grant.at.clone(),
        };
        provider.authorizations.remove(&handle_hash.0);
        if let Some(client) = provider
            .oauth_clients
            .get_mut(&request.client_id)
            .filter(|c| c.connected_at.is_none())
        {
            client.connected_at = Some(grant.at);
        }
        state
            .connections
            .insert(connection.connection_id, connection.clone());
        Ok(ConnectOutcome::Authorized {
            tenant_id,
            connection: Box::new(connection),
        })
    }

    async fn authorization_insert(&self, record: AuthorizationRecord) -> Result<(), StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        if provider.authorizations.contains_key(&record.handle_hash.0)
            || provider
                .authorizations
                .values()
                .any(|a| a.authorization_id == record.authorization_id)
        {
            return Err(StoreError::Backend(format!(
                "authorization request {} is held already",
                record.authorization_id
            )));
        }
        provider.authorizations.insert(record.handle_hash.0, record);
        Ok(())
    }

    async fn authorization(
        &self,
        handle_hash: &Sha256Hash,
    ) -> Result<Option<AuthorizationRecord>, StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let provider = provider.lock().expect("tenant lock");
        Ok(provider.authorizations.get(&handle_hash.0).cloned())
    }

    async fn oauth_client_insert(&self, record: OAuthClientRecord) -> Result<(), StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        if provider.oauth_clients.contains_key(&record.client_id) {
            return Err(StoreError::Backend(format!(
                "client {} is held already",
                record.client_id
            )));
        }
        provider
            .oauth_clients
            .insert(record.client_id.clone(), record);
        Ok(())
    }

    async fn oauth_client(&self, client_id: &str) -> Result<Option<OAuthClientRecord>, StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let provider = provider.lock().expect("tenant lock");
        Ok(provider.oauth_clients.get(client_id).cloned())
    }

    async fn upsert_cimd(
        &self,
        document: ClientDescription,
        fetched_at: &EventTime,
        max_age: u64,
    ) -> Result<DocumentCaching, StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        let clients = &mut provider.oauth_clients;
        let refreshed = |prior: Option<&OAuthClientRecord>| OAuthClientRecord {
            client_id: document.client_id.clone(),
            kind: ClientKind::Cimd,
            client_name: document.client_name.clone(),
            redirect_uris: document.redirect_uris.clone(),
            metadata_digest: document.metadata_digest,
            fetched_at: Some(fetched_at.clone()),
            max_age: Some(cache_lifetime(max_age)),
            last_used_at: Some(fetched_at.clone()),
            connected_at: prior.and_then(|row| row.connected_at.clone()),
            created_at: prior.map_or_else(|| fetched_at.clone(), |row| row.created_at.clone()),
        };
        if let Some(prior) = clients.get(&document.client_id) {
            if prior.kind != ClientKind::Cimd {
                return Err(StoreError::Backend(format!(
                    "client {} is a registration",
                    document.client_id
                )));
            }
            let row = refreshed(Some(prior));
            clients.insert(row.client_id.clone(), row);
            return Ok(DocumentCaching::Cached { evicted: None });
        }
        let cached = clients
            .values()
            .filter(|row| row.kind == ClientKind::Cimd)
            .count();
        let mut evicted = None;
        if cached >= CACHED_DOCUMENTS_MAX as usize {
            // The least recently used unconnected document; a row never
            // used sorts first, as `NULLS FIRST` does, and ties fall to the
            // client id's bytes.
            let Some(victim) = clients
                .values()
                .filter(|row| row.kind == ClientKind::Cimd && row.connected_at.is_none())
                .min_by_key(|row| {
                    (
                        row.last_used_at.as_ref().map(EventTime::unix_micros),
                        row.client_id.clone(),
                    )
                })
                .map(|row| row.client_id.clone())
            else {
                return Ok(DocumentCaching::NotCached);
            };
            clients.remove(&victim);
            evicted = Some(victim);
        }
        let row = refreshed(None);
        clients.insert(row.client_id.clone(), row);
        Ok(DocumentCaching::Cached { evicted })
    }

    async fn touch_cimd(&self, client_id: &str, at: &EventTime) -> Result<bool, StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        match provider.oauth_clients.get_mut(client_id) {
            Some(row) if row.kind == ClientKind::Cimd => {
                row.last_used_at = Some(at.clone());
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn insert_dcr(
        &self,
        registration: ClientDescription,
        at: &EventTime,
    ) -> Result<(), StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        let clients = &mut provider.oauth_clients;
        if clients.contains_key(&registration.client_id) {
            return Err(StoreError::Backend(format!(
                "client {} is held already",
                registration.client_id
            )));
        }
        let window_start = at.unix_micros() - i64::from(UNUSED_REGISTRATION_SECONDS) * 1_000_000;
        let unused = clients
            .values()
            .filter(|row| {
                row.kind == ClientKind::Dcr
                    && row.connected_at.is_none()
                    && row.created_at.unix_micros() > window_start
            })
            .count();
        if unused >= UNUSED_REGISTRATIONS_MAX as usize {
            return Err(StoreError::CapReached);
        }
        clients.insert(
            registration.client_id.clone(),
            OAuthClientRecord {
                client_id: registration.client_id,
                kind: ClientKind::Dcr,
                client_name: registration.client_name,
                redirect_uris: registration.redirect_uris,
                metadata_digest: registration.metadata_digest,
                fetched_at: None,
                max_age: None,
                last_used_at: None,
                connected_at: None,
                created_at: at.clone(),
            },
        );
        Ok(())
    }

    async fn remove_unused_dcr(&self, registered_by: &EventTime) -> Result<u64, StoreError> {
        let provider = self.tenant(PROVIDER_TENANT);
        let mut provider = provider.lock().expect("tenant lock");
        let before = provider.oauth_clients.len();
        provider.oauth_clients.retain(|_, row| {
            row.kind != ClientKind::Dcr
                || row.connected_at.is_some()
                || row.created_at.unix_micros() > registered_by.unix_micros()
        });
        Ok((before - provider.oauth_clients.len()) as u64)
    }

    async fn connect_codes(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Vec<ConnectCodeRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let mut codes: Vec<ConnectCodeRecord> = state
            .connect_codes
            .values()
            .filter(|c| c.key_id == key_id)
            .cloned()
            .collect();
        codes.sort_by_key(|c| c.issued_seq);
        Ok(codes)
    }

    async fn connections(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Vec<ConnectionRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(Vec::new());
        };
        let state = tenant.lock().expect("tenant lock");
        let mut connections: Vec<ConnectionRecord> = state
            .connections
            .values()
            .filter(|c| c.key_id == key_id)
            .cloned()
            .collect();
        connections.sort_by(|a, b| {
            (a.created_at.unix_micros(), a.connection_id)
                .cmp(&(b.created_at.unix_micros(), b.connection_id))
        });
        Ok(connections)
    }

    async fn connection(
        &self,
        tenant_id: Uuid,
        connection_id: Uuid,
    ) -> Result<Option<ConnectionRecord>, StoreError> {
        let Some(tenant) = self.tenant_existing(tenant_id) else {
            return Ok(None);
        };
        let state = tenant.lock().expect("tenant lock");
        Ok(state.connections.get(&connection_id).cloned())
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
// Client documents and handle sealing (ADR-0031 d5, d9)
// ---------------------------------------------------------------------------

/// What a document fetch answers for one URL: its body and `max-age`, or
/// the failure.
type Served = Result<(Vec<u8>, u64), DocumentFetchFailure>;

/// Client ID metadata documents served from memory by URL, counting every
/// fetch; a URL it does not serve fails as an unreachable host would. A
/// clone is another handle on the same documents.
#[derive(Debug, Default, Clone)]
pub struct MemDocuments {
    served: Arc<Mutex<BTreeMap<String, Served>>>,
    fetches: Arc<AtomicUsize>,
}

impl MemDocuments {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers every later fetch of `url` with `answer`.
    pub fn serve(&self, url: &str, answer: Served) {
        self.served
            .lock()
            .expect("documents lock")
            .insert(url.to_owned(), answer);
    }

    /// How many fetches were made, of any URL.
    #[must_use]
    pub fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

impl ClientDocumentFetcher for MemDocuments {
    async fn fetch(&self, url: &str) -> Result<FetchedDocument, DocumentFetchFailure> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        let served = self
            .served
            .lock()
            .expect("documents lock")
            .get(url)
            .cloned();
        let (body, max_age) = served.unwrap_or(Err(DocumentFetchFailure::Failed))?;
        Ok(FetchedDocument { body, max_age })
    }
}

/// A sealer that seals nothing: the handle is the plaintext in lower-case
/// hex, so a use-case test can read what a handle carries. It is a test
/// double only: the lead's sealer is an AEAD (ADR-0031 d9).
#[derive(Debug, Default, Clone, Copy)]
pub struct PlainSealer;

impl HandleSealer for PlainSealer {
    fn seal(&self, plaintext: &[u8]) -> String {
        plaintext.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn open(&self, handle: &str) -> Option<Vec<u8>> {
        if !handle.len().is_multiple_of(2) || !handle.is_ascii() {
            return None;
        }
        (0..handle.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&handle[i..i + 2], 16).ok())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Clock and id generator
// ---------------------------------------------------------------------------

/// Deterministic clock: a fixed date from noon, one microsecond per call.
/// A clone is another handle on the same clock, so a test that hands one
/// to an `App` can move that app's time forward with the other.
#[derive(Debug, Default, Clone)]
pub struct FixedClock {
    ticks: Arc<AtomicU64>,
}

impl FixedClock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the clock `secs` seconds forward, on every handle: an expiry
    /// reached without waiting for it. The clock reads within the fixed
    /// day, so it stops short of its midnight.
    pub fn advance(&self, secs: u64) {
        self.ticks.fetch_add(secs * 1_000_000, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> EventTime {
        let n = self.ticks.fetch_add(1, Ordering::SeqCst);
        let micros = n % 1_000_000;
        let secs_total = n / 1_000_000;
        let secs = secs_total % 60;
        let mins = (secs_total / 60) % 60;
        let hours = 12 + secs_total / 3_600;
        EventTime::parse(&format!(
            "2026-10-29T{hours:02}:{mins:02}:{secs:02}.{micros:06}Z"
        ))
        .expect("the fixed clock emits the fixed-width form within its day")
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
