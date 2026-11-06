//! The port-law contract suites (workspace-plan.md M1 step 2): every
//! function here states a law a port implementation must obey, written and
//! proven against the in-memory doubles first, then run IDENTICALLY by each
//! real adapter's test — the first adapter has something to pass.
//!
//! Usage from an adapter test: build a fresh implementation per contract
//! function and call it, or hand an async factory to
//! [`run_journal_store_suite`]. Contract functions panic on violation,
//! like any test assertion.

#![forbid(unsafe_code)]

use core::future::{Future, poll_fn};
use core::pin::pin;
use core::task::Poll;

use oml_app::domain::Sha256Hash;
use oml_app::domain::access::{Covers, Grant, KeyDenial};
use oml_app::domain::guardrail::{GuardrailScope, Severity};
use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::oauth::{
    CACHED_DOCUMENTS_MAX, ConnectionState, DOCUMENT_KEPT_SECONDS, Issuer, KeyState,
    RefreshDecision, RefreshInvalid, RevokedCause, TokenKind, UNUSED_REGISTRATIONS_MAX,
    decide_refresh,
};
use oml_app::domain::vocab_edges::{ClaimEdge, DerivesFrom, EdgeState, lineage_artifacts};
use oml_app::domain::{
    ChainHead, EventDraft, EventTime, HeldBy, KeySet, MemoryKind, MemoryStatus, Role,
    SignedEnvelope, WorkareaStatus, event_types, key_fingerprint, next_envelope,
    sha256_digest_string, sign, verify_chain,
};
use oml_app::doubles::FakeSparseEncoder;
use oml_app::ed25519_dalek::{SigningKey, Verifier as _};
use oml_app::ports::{
    AdmissionEnvelope, Clock, CommitReceipt, CommitRequest, ConnectionStore, IdGen, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, KeyringStore, MetadataFold, OutboxRow,
    Reranker, Retriever, Scanner, Signer, StoreError, SubjectRow,
};
use oml_app::search::{
    Access, Candidate, CandidateScope, MatchMode, OwnHeldScope, PostingsHealth, RetrievalQuery,
    Retrieved, RetrieverKind, SparsePending,
};
use oml_app::types::{
    AuthorizationRecord, BriefEdgeRecord, ClaimEdgePosition, ClaimEdgeRecord, ClaimEdgeSelection,
    ClaimKeyState, ClientDescription, ClientKind, ConnectCode, ConnectGrant, ConnectOutcome,
    ConnectionRecord, Credential, Crew, CrewRecord, DenialRecord, DocumentCaching, GrantRecord,
    GuardrailFields, HeldFilter, HeldRow, KeyAccess, KeyRecord, ListingFilter, ListingPosition,
    ListingScope, MemoryRecord, MintedToken, OAuthClientRecord, RefreshFacts, RefreshOutcome,
    SweepReport, TenantRow, TokenPair, TokenRecord, TransitionRecord, TransitionState, UnnamedRows,
    WorkareaRecord,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// The committed test key seed — the hex of the doc PEM's last 32 bytes
/// (docs/data/chain-format.md test vectors; oml-domain/tests/vectors.rs).
pub const TEST_SEED: [u8; 32] = [
    0x09, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

#[must_use]
pub fn test_signing_key() -> SigningKey {
    SigningKey::from_bytes(&TEST_SEED)
}

/// A deterministic chain builder for fixtures: assigns ids and fixed-width
/// timestamps, seals through the domain, remembers the head.
pub struct ChainRig {
    pub tenant_id: Uuid,
    /// The name the rig's genesis gives its tenant, unique per seed, so
    /// rigs share one store as tenants share one deployment (ADR-0026 d1).
    pub tenant_name: Name,
    key: SigningKey,
    head: Option<SignedEnvelope>,
    n: u64,
    /// Whether the store created this rig's tenant partitions.
    partitioned: bool,
}

impl ChainRig {
    #[must_use]
    pub fn new(tenant_seed: u64) -> Self {
        Self {
            tenant_id: Uuid::from_u128(0xA000_0000 + u128::from(tenant_seed)),
            tenant_name: name(&format!("rig-{tenant_seed}")),
            key: test_signing_key(),
            head: None,
            n: 0,
            partitioned: false,
        }
    }

    fn at(n: u64) -> EventTime {
        let micros = n % 1_000_000;
        let secs = (n / 1_000_000) % 60;
        EventTime::parse(&format!("2026-10-29T13:00:{secs:02}.{micros:06}Z"))
            .expect("fixed-width fixture time")
    }

    /// Seal the next event on this rig's chain (genesis when none exists).
    /// Does not advance the head — [`Self::commit`] does, on success.
    #[must_use]
    pub fn next_event(&mut self, event_type: &str, payload: Value) -> SignedEnvelope {
        self.n += 1;
        let draft = EventDraft {
            tenant_id: self.tenant_id,
            // Tenant in the high bits: event ids are unique store-wide
            // (journal.event_id), so two rigs on one store never collide.
            event_id: Uuid::from_u128((self.tenant_id.as_u128() << 64) | u128::from(self.n)),
            event_type: event_type.to_owned(),
            principal: "rig".to_owned(),
            api_key_id: None,
            at: Self::at(self.n),
            payload,
        };
        let envelope = next_envelope(self.head.as_ref(), draft).expect("rig draft is lawful");
        sign(envelope, &self.key).expect("rig signing")
    }

    /// Seal and commit one event with its fold; advance the rig head. The
    /// rig's first `tenant.genesis` asks the store for the tenant's
    /// partitions first, as tenant creation does; partitions a refused
    /// genesis leaves are kept, so a retry does not ask again (ADR-0034
    /// d11).
    pub async fn commit<S: JournalStore>(
        &mut self,
        store: &S,
        event_type: &str,
        payload: Value,
        fold: MetadataFold,
        idempotency: Option<IdempotencyPut>,
    ) -> Result<CommitReceipt, StoreError> {
        if !self.partitioned && event_type == event_types::TENANT_GENESIS {
            store.create_tenant_partitions(self.tenant_id).await?;
            self.partitioned = true;
        }
        let event = self.next_event(event_type, payload);
        let receipt = store
            .commit(CommitRequest {
                event: event.clone(),
                fold,
                outbox: OutboxRow {
                    event_id: event.envelope.event_id,
                    topic: event.envelope.event_type.clone(),
                    payload: json!({ "seq": event.envelope.seq }),
                },
                idempotency,
            })
            .await;
        match receipt {
            Ok(r) => {
                self.head = Some(event);
                Ok(r)
            }
            Err(e) => {
                self.n -= 1;
                Err(e)
            }
        }
    }

    /// Seal `links` one on the next and commit them as one chain; advance
    /// the rig head past the last on success, leave it where it was on a
    /// refusal.
    pub async fn commit_chain<S: JournalStore>(
        &mut self,
        store: &S,
        links: Vec<(&str, Value, MetadataFold)>,
    ) -> Result<CommitReceipt, StoreError> {
        let (head, n) = (self.head.clone(), self.n);
        let mut reqs = Vec::new();
        for (event_type, payload, fold) in links {
            let event = self.next_event(event_type, payload);
            self.head = Some(event.clone());
            reqs.push(CommitRequest {
                outbox: OutboxRow {
                    event_id: event.envelope.event_id,
                    topic: event.envelope.event_type.clone(),
                    payload: json!({ "seq": event.envelope.seq }),
                },
                event,
                fold,
                idempotency: None,
            });
        }
        let receipt = store.commit_chain(reqs).await;
        if receipt.is_err() {
            (self.head, self.n) = (head, n);
        }
        receipt
    }

    /// Commit one `workarea.status_proposed` folding `proposal`, its
    /// payload the event catalog's.
    pub async fn propose_transition<S: JournalStore>(
        &mut self,
        store: &S,
        proposal: TransitionRecord,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::WORKAREA_STATUS_PROPOSED,
            json!({
                "workarea_id": proposal.workarea_id.to_string(),
                "from": proposal.from.as_str(),
                "to": proposal.to.as_str(),
                "transition_id": proposal.transition_id.to_string(),
                "proposed_by_key_id": proposal.proposer_key_id.to_string(),
            }),
            MetadataFold::TransitionProposed(proposal),
            None,
        )
        .await
    }

    /// The `workarea.status_transition` link that ratifies `transition_id`
    /// on `workarea_id` to `to` by `ratifier_key_id`, for [`Self::commit`]
    /// or [`Self::commit_chain`].
    #[must_use]
    pub fn ratification(
        transition_id: Uuid,
        workarea_id: Uuid,
        to: WorkareaStatus,
        ratifier_key_id: Uuid,
    ) -> (&'static str, Value, MetadataFold) {
        (
            event_types::WORKAREA_STATUS_TRANSITION,
            json!({
                "workarea_id": workarea_id.to_string(),
                "to": to.as_str(),
                "transition_id": transition_id.to_string(),
                "ratified_by_key_id": ratifier_key_id.to_string(),
            }),
            MetadataFold::TransitionRatified {
                transition_id,
                workarea_id,
                to,
                ratifier_key_id,
            },
        )
    }

    /// Commit one `workarea.status_transition` ratifying `transition_id`.
    pub async fn ratify_transition<S: JournalStore>(
        &mut self,
        store: &S,
        transition_id: Uuid,
        workarea_id: Uuid,
        to: WorkareaStatus,
        ratifier_key_id: Uuid,
    ) -> Result<CommitReceipt, StoreError> {
        let (event_type, payload, fold) =
            Self::ratification(transition_id, workarea_id, to, ratifier_key_id);
        self.commit(store, event_type, payload, fold, None).await
    }

    /// Commit one `workarea.create` folding `workarea`.
    pub async fn create_workarea<S: JournalStore>(
        &mut self,
        store: &S,
        workarea: WorkareaRecord,
    ) -> CommitReceipt {
        self.commit(
            store,
            event_types::WORKAREA_CREATE,
            json!({
                "workarea_id": workarea.workarea_id.to_string(),
                "name": workarea.name.as_str(),
            }),
            MetadataFold::WorkareaCreated(workarea),
            None,
        )
        .await
        .expect("workarea commit")
    }

    /// Commit one `key.issued` folding `key`.
    pub async fn issue_key<S: JournalStore>(&mut self, store: &S, key: KeyRecord) -> CommitReceipt {
        self.commit(
            store,
            event_types::KEY_ISSUED,
            json!({ "key_id": key.key_id.to_string(), "value_disclosed": false }),
            MetadataFold::KeyIssued(key),
            None,
        )
        .await
        .expect("key commit")
    }

    /// Commit one `grant.issued` folding `grant` onto `key_id`.
    pub async fn issue_grant<S: JournalStore>(
        &mut self,
        store: &S,
        key_id: Uuid,
        grant: Grant,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::GRANT_ISSUED,
            json!({ "grant_id": grant.grant_id.to_string(), "key_id": key_id.to_string() }),
            MetadataFold::GrantIssued { key_id, grant },
            None,
        )
        .await
    }

    /// Commit one `key.denied` folding `denial` onto `key_id`.
    pub async fn deny_key<S: JournalStore>(
        &mut self,
        store: &S,
        key_id: Uuid,
        denial: KeyDenial,
        created_by: Uuid,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::KEY_DENIED,
            json!({ "denial_id": denial.denial_id.to_string(), "key_id": key_id.to_string() }),
            MetadataFold::KeyDenied {
                key_id,
                denial,
                created_by,
            },
            None,
        )
        .await
    }

    /// Commit one `key.denial_lifted` folding the lift of `denial_id` on
    /// `key_id`.
    pub async fn lift_denial<S: JournalStore>(
        &mut self,
        store: &S,
        key_id: Uuid,
        denial_id: Uuid,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::KEY_DENIAL_LIFTED,
            json!({ "denial_id": denial_id.to_string(), "key_id": key_id.to_string() }),
            MetadataFold::KeyDenialLifted { key_id, denial_id },
            None,
        )
        .await
    }

    /// Commit one `key.revoked` folding the revocation of `key_id`.
    pub async fn revoke_key<S: JournalStore>(
        &mut self,
        store: &S,
        key_id: Uuid,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::KEY_REVOKED,
            json!({ "key_id": key_id.to_string() }),
            MetadataFold::KeyRevoked { key_id },
            None,
        )
        .await
    }

    /// Commit one event whose fold changes no row, to move the head: the
    /// genesis fold of a tenant that already has its row.
    pub async fn filler<S: JournalStore>(&mut self, store: &S) -> CommitReceipt {
        let fold = self.genesis_fold();
        self.commit(
            store,
            event_types::SUBMISSION_REMEMBER,
            json!({ "body_digest": "sha256:00" }),
            fold,
            None,
        )
        .await
        .expect("filler commit")
    }

    /// The genesis fold of this rig's tenant, under its name.
    #[must_use]
    pub fn genesis_fold(&self) -> MetadataFold {
        MetadataFold::TenantGenesis {
            name: self.tenant_name.clone(),
        }
    }

    /// Commit one `crew.created` folding `crew`.
    pub async fn create_crew<S: JournalStore>(
        &mut self,
        store: &S,
        crew: CrewRecord,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::CREW_CREATED,
            json!({ "crew_id": crew.crew_id.to_string(), "name": crew.name.as_str() }),
            MetadataFold::CrewCreated(crew),
            None,
        )
        .await
    }

    /// Genesis plus `extra` empty-payload events.
    pub async fn seed_chain<S: JournalStore>(&mut self, store: &S, extra: u64) {
        let fold = self.genesis_fold();
        let payload = json!({ "tenant_name": self.tenant_name.as_str() });
        self.commit(store, event_types::TENANT_GENESIS, payload, fold, None)
            .await
            .expect("genesis commit");
        for _ in 0..extra {
            let fold = self.genesis_fold();
            self.commit(
                store,
                event_types::SUBMISSION_REMEMBER,
                json!({ "body_digest": "sha256:00" }),
                fold,
                None,
            )
            .await
            .expect("seed commit");
        }
    }
}

/// A held fixture memory with its one-author lineage closure: the first
/// version of a claim of its own, kind `memory`.
#[must_use]
pub fn held_memory(
    tenant_id: Uuid,
    workarea_id: Uuid,
    memory_id: Uuid,
    author_principal: &str,
    author_key_id: Uuid,
    body: &str,
) -> MemoryRecord {
    MemoryRecord {
        memory_id,
        tenant_id,
        workarea_id,
        claim_key: format!("claim-{}", memory_id.simple()),
        version: 1,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        body_digest: oml_app::domain::sha256_digest_string(body.as_bytes()),
        body_bytes: body.len() as u64,
        status: MemoryStatus::Held,
        is_tip: false,
        promoted_at: None,
        retirement: false,
        replaces: None,
        author_principal: author_principal.to_owned(),
        author_key_id,
        derives_from: Vec::new(),
        lineage_principals: vec![author_principal.to_owned()],
        lineage_key_ids: vec![author_key_id],
        scan_finding: None,
        created_at: EventTime::parse("2026-10-29T13:00:00.000000Z").expect("fixture time"),
        guardrail: None,
        valid: Default::default(),
        anchor: None,
        origin_kind: None,
        source_check: None,
        format: Box::default(),
    }
}

/// Commits `memory` held through the rig, enqueued for review.
pub async fn hold<S: JournalStore>(
    rig: &mut ChainRig,
    store: &S,
    memory: MemoryRecord,
) -> Result<CommitReceipt, StoreError> {
    rig.commit(
        store,
        event_types::SUBMISSION_REMEMBER,
        json!({ "artifact_id": memory.memory_id.to_string() }),
        MetadataFold::MemoryHeld {
            memory,
            enqueue: true,
        },
        None,
    )
    .await
}

/// Commits the promotion of `memory_id` through the rig.
async fn promote<S: JournalStore>(
    rig: &mut ChainRig,
    store: &S,
    memory_id: Uuid,
) -> Result<CommitReceipt, StoreError> {
    rig.commit(
        store,
        event_types::LIFECYCLE_PROMOTE,
        json!({ "artifact_id": memory_id.to_string() }),
        MetadataFold::MemoryPromoted { memory_id },
        None,
    )
    .await
}

/// Commits the ratification of `successor`, which takes the tip from
/// `replaced`, through the rig.
async fn supersede_tip<S: JournalStore>(
    rig: &mut ChainRig,
    store: &S,
    successor: Uuid,
    replaced: Uuid,
) -> Result<CommitReceipt, StoreError> {
    rig.commit(
        store,
        event_types::LIFECYCLE_SUPERSEDE,
        json!({
            "new_artifact_id": successor.to_string(),
            "replaces_artifact_id": replaced.to_string(),
        }),
        MetadataFold::TipSuperseded {
            successor,
            replaced,
            replaced_valid_until: None,
        },
        None,
    )
    .await
}

/// Commits the rejection of `memory_id` through the rig.
pub async fn reject<S: JournalStore>(
    rig: &mut ChainRig,
    store: &S,
    memory_id: Uuid,
) -> Result<CommitReceipt, StoreError> {
    rig.commit(
        store,
        event_types::REVIEW_REJECT,
        json!({ "artifact_id": memory_id.to_string() }),
        MetadataFold::MemoryRejected { memory_id },
        None,
    )
    .await
}

/// A fixture name; one the creation grammar refuses fails the fixture
/// (ADR-0026 d2).
#[must_use]
pub fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name the creation grammar admits")
}

/// A fixture workarea node with no crew, at the status `workarea.create`
/// stores: `committed` for a root, `development` for every other node.
#[must_use]
pub fn workarea_record(
    tenant_id: Uuid,
    workarea_id: Uuid,
    parent_id: Option<Uuid>,
    workarea_name: &str,
) -> WorkareaRecord {
    WorkareaRecord {
        in_progress: false,
        workarea_id,
        tenant_id,
        parent_id,
        name: name(workarea_name),
        status: if parent_id.is_none() {
            WorkareaStatus::Committed
        } else {
            WorkareaStatus::Development
        },
        crew: None,
        crew_id: None,
        user_root: false,
        namespace: None,
    }
}

/// A fixture crew record with no charter and no external manager.
#[must_use]
pub fn crew_record(tenant_id: Uuid, crew_id: Uuid, crew_name: &str) -> CrewRecord {
    CrewRecord {
        crew_id,
        tenant_id,
        name: name(crew_name),
        charter_uri: None,
        external_manager: None,
    }
}

/// A fixture key row; its value hash is derived from its id, so every
/// fixture key resolves on its own.
#[must_use]
pub fn key_record(tenant_id: Uuid, key_id: Uuid, role: Role, issued_by: Option<Uuid>) -> KeyRecord {
    KeyRecord {
        profile: None,
        key_id,
        tenant_id,
        role,
        principal: format!("fixture/{key_id}"),
        held_by: HeldBy::Agent,
        default_workarea: None,
        issued_by,
        credential: Credential::Bearer,
        key_hash: Some(sha256_digest_string(key_id.as_bytes())),
        name: None,
        crew_id: None,
        revoked_at: None,
        user: None,
        session_user: None,
        guardrail_scope: GuardrailScope::Ancestors,
    }
}

// ---------------------------------------------------------------------------
// JournalStore contracts
// ---------------------------------------------------------------------------

/// Commits extend the head in order; the head and range reads agree; the
/// committed chain verifies under the domain procedure.
pub async fn journal_commit_and_head<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(1);
    assert_eq!(
        store.chain_head(rig.tenant_id).await.expect("head"),
        None,
        "an unknown tenant has no head"
    );
    rig.seed_chain(store, 3).await;

    let head = store
        .chain_head(rig.tenant_id)
        .await
        .expect("head")
        .expect("a head after commits");
    assert_eq!(head.seq, 4);

    let events = store
        .events_range(rig.tenant_id, 1, 100)
        .await
        .expect("range");
    assert_eq!(events.len(), 4);
    let keys = KeySet::new(vec![test_signing_key().verifying_key()]);
    let verified = verify_chain(&events, &keys, Some(&head)).expect("the committed chain verifies");
    assert_eq!(verified, head);
}

async fn commit_raw<S: JournalStore>(
    store: &S,
    event: SignedEnvelope,
) -> Result<CommitReceipt, StoreError> {
    store
        .commit(CommitRequest {
            event,
            fold: MetadataFold::TenantGenesis {
                name: name("conflict"),
            },
            outbox: OutboxRow {
                event_id: Uuid::from_u128(999),
                topic: "x".to_owned(),
                payload: json!({}),
            },
            idempotency: None,
        })
        .await
}

/// The store refuses a commit that does not extend the current head: stale
/// seq and mismatched prev_hash are both typed conflicts, never silent —
/// the serialized chain assignment of ADR-0002 as a port law.
pub async fn journal_commit_refuses_conflicts<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(2);
    rig.seed_chain(store, 1).await; // head is seq 2

    let drafted = rig.next_event(event_types::SUBMISSION_REMEMBER, json!({}));

    // Stale seq: a second seq-2 event while the head is already 2.
    let mut stale_envelope = drafted.envelope.clone();
    stale_envelope.seq = 2;
    let stale = sign(stale_envelope, &test_signing_key()).expect("re-sign stale");
    let err = commit_raw(store, stale)
        .await
        .expect_err("a stale commit is refused");
    assert!(matches!(err, StoreError::SeqConflict { .. }), "got {err:?}");

    // Fork: the correct next seq, but a prev_hash that is not the head's.
    let mut fork_envelope = drafted.envelope;
    fork_envelope.prev_hash = oml_app::domain::Sha256Hash::ZERO;
    let forked = sign(fork_envelope, &test_signing_key()).expect("re-sign fork");
    let err = commit_raw(store, forked)
        .await
        .expect_err("a forked commit is refused");
    assert!(
        matches!(err, StoreError::PrevHashConflict { .. }),
        "got {err:?}"
    );
}

/// `events_range` pages by seq ascending and respects `from_seq`/`limit`.
pub async fn journal_events_range_pages<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(3);
    rig.seed_chain(store, 4).await; // seq 1..=5
    let page = store
        .events_range(rig.tenant_id, 2, 2)
        .await
        .expect("range");
    assert_eq!(
        page.iter().map(|e| e.envelope.seq).collect::<Vec<_>>(),
        vec![2, 3]
    );
    let empty = store
        .events_range(rig.tenant_id, 6, 10)
        .await
        .expect("range");
    assert!(empty.is_empty());
}

/// A tail scan reads at most `limit` raw events from `from_seq`, seq
/// ascending, joins each event that names a claim version — the version a
/// submission or promotion names, the successor a supersession ratifies —
/// to that row's workarea and authoring key as data, and reads `head_seq`
/// with it (http-api.md events tail; ADR-0025 d15).
pub async fn a_tail_scan_joins_each_claim_subject_to_its_row_and_reads_the_head_with_it<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(70);
    let tenant = rig.tenant_id;
    let unknown = store
        .events_with_subjects(tenant, 1, 10)
        .await
        .expect("scan");
    assert!(unknown.events.is_empty(), "a tenant with no chain");
    assert_eq!(unknown.head_seq, 0, "a tenant with no chain");

    rig.seed_chain(store, 0).await; // seq 1
    let workarea = Uuid::from_u128(0x7001);
    rig.create_workarea(store, workarea_record(tenant, workarea, None, "universe"))
        .await; // seq 2
    let author = Uuid::from_u128(0x7002);
    let first = held_memory(
        tenant,
        workarea,
        Uuid::from_u128(0x7003),
        "wanda",
        author,
        "drain the canary",
    );
    hold_and_promote(&mut rig, store, first.clone()).await; // seq 3, 4
    let successor = MemoryRecord {
        author_key_id: Uuid::from_u128(0x7004),
        ..successor_of(&first, Uuid::from_u128(0x7005), "drain it twice", false)
    };
    hold_and_ratify(&mut rig, store, successor.clone()).await; // seq 5, 6
    rig.filler(store).await; // seq 7: a submission naming no artifact

    let row = |m: &MemoryRecord| {
        Some(SubjectRow {
            artifact_id: m.memory_id,
            workarea_id: m.workarea_id,
            author_key_id: m.author_key_id,
        })
    };
    let scan = store
        .events_with_subjects(tenant, 1, 100)
        .await
        .expect("scan");
    assert_eq!(scan.head_seq, 7);
    assert_eq!(
        scan.events
            .iter()
            .map(|e| (e.event.envelope.seq, e.subject))
            .collect::<Vec<_>>(),
        vec![
            (1, None),
            (2, None),
            (3, row(&first)),
            (4, row(&first)),
            (5, row(&successor)),
            (6, row(&successor)),
            (7, None),
        ],
        "each claim subject joins its own row"
    );

    let page = store
        .events_with_subjects(tenant, 3, 2)
        .await
        .expect("scan");
    assert_eq!(
        page.events
            .iter()
            .map(|e| e.event.envelope.seq)
            .collect::<Vec<_>>(),
        vec![3, 4],
        "limit bounds the scan"
    );
    assert_eq!(page.head_seq, 7, "the head is read whatever the page holds");
    let past = store
        .events_with_subjects(tenant, 8, 10)
        .await
        .expect("scan");
    assert!(past.events.is_empty());
    assert_eq!(past.head_seq, 7);
}

/// The metadata fold is applied atomically with the event: a held row lands
/// readable, a promote fold transitions it.
pub async fn journal_memory_fold_and_lookup<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(4);
    rig.seed_chain(store, 0).await;
    let workarea = Uuid::from_u128(0xB1);
    let memory_id = Uuid::from_u128(0xC1);
    let memory = held_memory(
        rig.tenant_id,
        workarea,
        memory_id,
        "wanda",
        Uuid::from_u128(0xD1),
        "held body",
    );
    rig.commit(
        store,
        event_types::SUBMISSION_REMEMBER,
        json!({ "artifact_id": memory_id.to_string() }),
        MetadataFold::MemoryHeld {
            memory,
            enqueue: true,
        },
        None,
    )
    .await
    .expect("commit held");
    let read = store
        .memory(rig.tenant_id, memory_id)
        .await
        .expect("memory")
        .expect("the fold landed");
    assert_eq!(read.status, MemoryStatus::Held);

    rig.commit(
        store,
        event_types::LIFECYCLE_PROMOTE,
        json!({ "artifact_id": memory_id.to_string() }),
        MetadataFold::MemoryPromoted { memory_id },
        None,
    )
    .await
    .expect("commit promote");
    let read = store
        .memory(rig.tenant_id, memory_id)
        .await
        .expect("memory")
        .expect("still there");
    assert_eq!(read.status, MemoryStatus::Promoted);
    assert_eq!(
        store
            .memory(rig.tenant_id, Uuid::from_u128(0xFF))
            .await
            .expect("memory"),
        None,
        "unknown ids are absent, not errors"
    );
}

/// Holds `memory`, a claim's first version, and promotes it through the
/// rig: the claim's served tip until a ratified successor takes it.
pub async fn hold_and_promote<S: JournalStore>(
    rig: &mut ChainRig,
    store: &S,
    memory: MemoryRecord,
) {
    let id = memory.memory_id;
    hold(rig, store, memory).await.expect("hold");
    promote(rig, store, id).await.expect("promote");
}

/// Holds the next version of `target`'s claim, `memory_id` saying `body`,
/// and ratifies it, so it takes the tip; `retirement` marks a retirement
/// version (ADR-0025 d10, d11).
pub async fn supersede<S: JournalStore>(
    rig: &mut ChainRig,
    store: &S,
    target: &MemoryRecord,
    memory_id: Uuid,
    body: &str,
    retirement: bool,
) {
    hold_and_ratify(
        rig,
        store,
        successor_of(target, memory_id, body, retirement),
    )
    .await;
}

/// Holds `successor` and ratifies it through the rig, so it takes the tip
/// from the version it replaces (ADR-0025 d10).
async fn hold_and_ratify<S: JournalStore>(rig: &mut ChainRig, store: &S, successor: MemoryRecord) {
    let id = successor.memory_id;
    let replaced = successor.replaces.expect("a successor replaces a version");
    hold(rig, store, successor).await.expect("hold");
    supersede_tip(rig, store, id, replaced)
        .await
        .expect("ratify");
}

/// The next version of `target`'s claim, held: `retirement` marks a
/// retirement version (ADR-0025 d10, d11).
fn successor_of(
    target: &MemoryRecord,
    memory_id: Uuid,
    body: &str,
    retirement: bool,
) -> MemoryRecord {
    MemoryRecord {
        memory_id,
        version: target.version + 1,
        body: body.to_owned(),
        body_digest: sha256_digest_string(body.as_bytes()),
        body_bytes: body.len() as u64,
        retirement,
        replaces: Some(target.memory_id),
        derives_from: vec![target.memory_id],
        ..target.clone()
    }
}

/// The v0.5.0 recall reads, as the `keyword-all` retriever serves them: the
/// promoted list over `readable`, and the own held list of `author_key_ids`
/// over `readable` (ADR-0049 d7). The laws written against them hold of
/// every adapter's `keyword-all`.
pub trait KeywordAllReads: Retriever {
    fn recall_fts(
        &self,
        tenant_id: Uuid,
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> impl Future<Output = Result<Vec<Candidate>, StoreError>> + Send {
        let scope = CandidateScope {
            anchor: oml_app::types::AnchorFilter::default(),
            promoted_nodes: readable.to_vec(),
            kinds: kinds.to_vec(),
            own_held: None,
            expired_at: None,
        };
        let query = query.to_owned();
        async move {
            self.retrieve(
                tenant_id,
                RetrieverKind::KeywordAll,
                &scope,
                RetrievalQuery {
                    query: &query,
                    match_mode: MatchMode::All,
                    k,
                    sparse: &[],
                    window: None,
                    within: &[],
                },
            )
            .await
            .map(|r| r.candidates)
        }
    }

    fn recall_own_held(
        &self,
        tenant_id: Uuid,
        author_key_ids: &[Uuid],
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> impl Future<Output = Result<Vec<Candidate>, StoreError>> + Send {
        let scope = CandidateScope {
            anchor: oml_app::types::AnchorFilter::default(),
            promoted_nodes: Vec::new(),
            kinds: kinds.to_vec(),
            own_held: Some(OwnHeldScope {
                any_author: false,
                author_key_ids: author_key_ids.to_vec(),
                nodes: readable.to_vec(),
            }),
            expired_at: None,
        };
        let query = query.to_owned();
        async move {
            self.retrieve(
                tenant_id,
                RetrieverKind::KeywordAll,
                &scope,
                RetrievalQuery {
                    query: &query,
                    match_mode: MatchMode::All,
                    k,
                    sparse: &[],
                    window: None,
                    within: &[],
                },
            )
            .await
            .map(|r| r.candidates)
        }
    }
}

impl<R: Retriever + ?Sized> KeywordAllReads for R {}

/// The ids of recall candidates, in the order served.
fn hit_ids(hits: &[Candidate]) -> Vec<Uuid> {
    hits.iter().map(|c| c.memory_id).collect()
}

/// The hit kinds recall admits by default (retrieval.md: vocabulary kinds
/// only by a workarea's opt-in, which is unbuilt).
const HIT_KINDS: [MemoryKind; 2] = [MemoryKind::Memory, MemoryKind::DerivedFact];

/// Recall serves promoted served tips only — R3 at the store (retrieval.md)
/// — never a held row or a replaced version, searches exactly the readable
/// workarea set it is handed, an empty set included, admits exactly the
/// kinds it is handed, and honours the `k` bound (ADR-0025 d1: no
/// tenant-wide fallback; d8).
pub async fn recall_serves_promoted_tips_only_within_the_readable_set<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(5);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB2);
    let fixture = |id: u128, body: &str| {
        held_memory(
            rig_tenant(5),
            wa,
            Uuid::from_u128(id),
            "wanda",
            Uuid::from_u128(0xD1),
            body,
        )
    };
    let promoted = fixture(0xC2, "the canary token");
    let held = fixture(0xC3, "the canary token");
    let replaced = fixture(0xC4, "the canary token, first cut");
    let tip = successor_of(
        &replaced,
        Uuid::from_u128(0xC5),
        "the canary token, second cut",
        false,
    );
    let derived = MemoryRecord {
        kind: MemoryKind::DerivedFact,
        ..fixture(0xC6, "the canary token, derived")
    };
    hold_and_promote(&mut rig, store, promoted.clone()).await;
    hold(&mut rig, store, held).await.expect("hold the twin");
    hold_and_promote(&mut rig, store, replaced).await;
    hold_and_ratify(&mut rig, store, tip.clone()).await;
    hold_and_promote(&mut rig, store, derived.clone()).await;

    let recall = |readable: Vec<Uuid>, kinds: Vec<MemoryKind>, k: usize| async move {
        store
            .recall_fts(rig_tenant(5), &readable, "canary", &kinds, k)
            .await
            .expect("recall")
    };
    let hits = recall(vec![wa], HIT_KINDS.to_vec(), 10).await;
    let mut served = hit_ids(&hits);
    served.sort();
    assert_eq!(
        served,
        vec![promoted.memory_id, tip.memory_id, derived.memory_id],
        "served tips only — neither the held twin nor the replaced version"
    );
    let wider = recall(vec![Uuid::from_u128(0xBF), wa], HIT_KINDS.to_vec(), 10).await;
    assert_eq!(wider, hits, "every node of the set is searched");
    assert!(
        recall(vec![Uuid::from_u128(0xBF)], HIT_KINDS.to_vec(), 10)
            .await
            .is_empty(),
        "the workarea set holds"
    );
    assert!(
        recall(Vec::new(), HIT_KINDS.to_vec(), 10).await.is_empty(),
        "an empty readable set searches nothing, never the tenant"
    );
    assert_eq!(
        hit_ids(&recall(vec![wa], vec![MemoryKind::DerivedFact], 10).await),
        vec![derived.memory_id],
        "kinds narrows the hits"
    );
    assert!(
        recall(vec![wa], Vec::new(), 10).await.is_empty(),
        "an empty kinds set admits nothing"
    );
    assert!(
        recall(vec![wa], HIT_KINDS.to_vec(), 0).await.is_empty(),
        "k bounds the result"
    );
    assert_eq!(
        recall(vec![wa], HIT_KINDS.to_vec(), 1).await,
        hits[..1],
        "k keeps the first of the order"
    );
}

/// Hits come in the total order of retrieval.md — score descending, then
/// `artifact_id` ascending in byte order — so rows that tie on score, here
/// identical texts, come back in id order whatever order they were written
/// in, and `k` keeps a prefix of that order (ADR-0025 d8). Scores are each
/// store's own and are not compared across stores.
pub async fn hits_that_tie_on_score_are_ordered_by_artifact_id<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(40);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB40);
    let ties = [0xC43_u128, 0xC41, 0xC42].map(Uuid::from_u128);
    for id in ties {
        let memory = held_memory(
            rig.tenant_id,
            wa,
            id,
            "wanda",
            Uuid::from_u128(0xD40),
            "drain the canary",
        );
        hold_and_promote(&mut rig, store, memory).await;
    }
    let louder = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC40),
        "wanda",
        Uuid::from_u128(0xD40),
        "drain the canary, the canary, the canary",
    );
    hold_and_promote(&mut rig, store, louder).await;

    let hits = store
        .recall_fts(rig.tenant_id, &[wa], "canary", &HIT_KINDS, 10)
        .await
        .expect("recall");
    assert_eq!(hits.len(), 4, "{hits:?}");
    assert!(
        hits.windows(2).all(|w| {
            let (a, b) = (&w[0], &w[1]);
            a.score > b.score || (a.score == b.score && a.memory_id < b.memory_id)
        }),
        "score descending, then artifact_id ascending: {hits:?}"
    );
    let tied: Vec<&Candidate> = hits
        .iter()
        .filter(|c| ties.contains(&c.memory_id))
        .collect();
    assert_eq!(
        tied.iter().map(|c| c.memory_id).collect::<Vec<_>>(),
        [0xC41_u128, 0xC42, 0xC43].map(Uuid::from_u128),
        "equal texts tie, and the tie breaks by artifact_id"
    );
    assert!(tied.iter().all(|c| c.score == tied[0].score));
    let first_two = store
        .recall_fts(rig.tenant_id, &[wa], "canary", &HIT_KINDS, 2)
        .await
        .expect("recall");
    assert_eq!(first_two, hits[..2], "k keeps the prefix of the order");
}

/// Steering is never a hit, whatever the kinds it is handed name and
/// however well it matches the query; it is served as applicable steering
/// (retrieval.md, the recall-ordering contract; ADR-0025 d8).
pub async fn steering_is_never_returned_as_a_hit<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(41);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB41);
    let fixture = |id: u128, body: &str| {
        held_memory(
            rig_tenant(41),
            wa,
            Uuid::from_u128(id),
            "wanda",
            Uuid::from_u128(0xD41),
            body,
        )
    };
    let steering = MemoryRecord {
        kind: MemoryKind::Steering,
        ..fixture(0xC410, "canary canary: drain the canary first")
    };
    let memory = fixture(0xC411, "the canary drained");
    hold_and_promote(&mut rig, store, steering.clone()).await;
    hold_and_promote(&mut rig, store, memory.clone()).await;

    let every_kind = [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Steering,
    ];
    let hits = store
        .recall_fts(rig.tenant_id, &[wa], "canary", &every_kind, 10)
        .await
        .expect("recall");
    assert_eq!(hit_ids(&hits), vec![memory.memory_id], "{hits:?}");
    let in_force = store
        .steering_in_force(rig.tenant_id, &[wa])
        .await
        .expect("steering");
    assert_eq!(
        in_force.iter().map(|m| m.memory_id).collect::<Vec<_>>(),
        vec![steering.memory_id]
    );
}

/// A retired claim, one whose served tip is a retirement version, serves
/// nothing: neither the retirement version nor any version it replaced is a
/// hit or applicable steering, while a claim whose retirement is still held
/// keeps serving its tip (ADR-0025 d8, d11).
pub async fn retired_claims_are_absent_from_recall_and_steering<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(42);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB42);
    let fixture = |id: u128, kind: MemoryKind, body: &str| MemoryRecord {
        kind,
        ..held_memory(
            rig_tenant(42),
            wa,
            Uuid::from_u128(id),
            "wanda",
            Uuid::from_u128(0xD42),
            body,
        )
    };
    let retire = "retired: the canary lapsed";
    let mut kept = Vec::new();
    for (base, kind) in [
        (0xC420_u128, MemoryKind::Memory),
        (0xC430, MemoryKind::Steering),
    ] {
        let retired = fixture(base, kind, "the canary claim");
        let retirement = successor_of(&retired, Uuid::from_u128(base + 1), retire, true);
        hold_and_promote(&mut rig, store, retired.clone()).await;
        hold_and_ratify(&mut rig, store, retirement).await;

        let pending = fixture(base + 2, kind, "the canary claim still served");
        let held_retirement = successor_of(&pending, Uuid::from_u128(base + 3), retire, true);
        hold_and_promote(&mut rig, store, pending.clone()).await;
        hold(&mut rig, store, held_retirement)
            .await
            .expect("hold the retirement");
        kept.push(pending.memory_id);
    }

    let every_kind = [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Steering,
    ];
    for query in ["canary", "retired"] {
        let hits = store
            .recall_fts(rig.tenant_id, &[wa], query, &every_kind, 10)
            .await
            .expect("recall");
        let expected = if query == "canary" {
            vec![kept[0]]
        } else {
            Vec::new()
        };
        assert_eq!(hit_ids(&hits), expected, "query {query:?}: {hits:?}");
    }
    let in_force = store
        .steering_in_force(rig.tenant_id, &[wa])
        .await
        .expect("steering");
    assert_eq!(
        in_force.iter().map(|m| m.memory_id).collect::<Vec<_>>(),
        vec![kept[1]],
        "a held retirement changes nothing until it is ratified"
    );
}

/// Applicable steering is served over exactly the nodes named, ordered by
/// its node's position among them — the depth from the root — then by
/// `promoted_at`, then by `artifact_id`, never in the order rows were
/// written; a held steering row and a promoted memory are not steering in
/// force (retrieval.md, the recall-ordering contract; ADR-0025 d8).
pub async fn steering_in_force_is_ordered_by_depth_then_promoted_at_then_artifact_id<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(43);
    rig.seed_chain(store, 0).await;
    let [root, child, grandchild, elsewhere] =
        [0xB430_u128, 0xB431, 0xB432, 0xB433].map(Uuid::from_u128);
    let fixture = |wa: Uuid, id: u128, kind: MemoryKind| MemoryRecord {
        kind,
        ..held_memory(
            rig_tenant(43),
            wa,
            Uuid::from_u128(id),
            "wanda",
            Uuid::from_u128(0xD43),
            "drain the canary first",
        )
    };
    // Promoted in this order: the deepest first, then the root's
    // higher id before its lower one.
    let deep = fixture(grandchild, 0xC431, MemoryKind::Steering);
    let root_first = fixture(root, 0xC439, MemoryKind::Steering);
    let root_second = fixture(root, 0xC432, MemoryKind::Steering);
    let mid = fixture(child, 0xC433, MemoryKind::Steering);
    let outside = fixture(elsewhere, 0xC434, MemoryKind::Steering);
    let memory = fixture(root, 0xC430, MemoryKind::Memory);
    for row in [&deep, &root_first, &root_second, &mid, &outside, &memory] {
        hold_and_promote(&mut rig, store, row.clone()).await;
    }
    hold(&mut rig, store, fixture(root, 0xC435, MemoryKind::Steering))
        .await
        .expect("hold");

    let in_force = |nodes: Vec<Uuid>| async move {
        store
            .steering_in_force(rig_tenant(43), &nodes)
            .await
            .expect("steering")
            .iter()
            .map(|m| m.memory_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        in_force(vec![root, child, grandchild]).await,
        vec![
            root_first.memory_id,
            root_second.memory_id,
            mid.memory_id,
            deep.memory_id
        ],
        "depth, then promoted_at, then artifact_id"
    );
    assert_eq!(in_force(vec![child]).await, vec![mid.memory_id]);
    assert!(
        in_force(Vec::new()).await.is_empty(),
        "no node serves nothing"
    );
}

/// Own held recall reads only the held rows of the author key set it is
/// handed, whatever the query matches: never another key's held row, never
/// a promoted, rejected, steering or retirement row of its own, and never a
/// row off the readable nodes; it admits the kinds it is handed, ranks in
/// the total order of hits and honours `k` (retrieval.md, own held rows in
/// recall and held text containment; ADR-0025 d4, d5).
pub async fn recall_own_held_never_returns_another_keys_row_whatever_the_query<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(44);
    rig.seed_chain(store, 0).await;
    let [wa, elsewhere] = [0xB440_u128, 0xB441].map(Uuid::from_u128);
    let [own, other] = [0xD440_u128, 0xD441].map(Uuid::from_u128);
    let row = |wa: Uuid, id: u128, author: Uuid, kind: MemoryKind, body: &str| MemoryRecord {
        kind,
        ..held_memory(
            rig_tenant(44),
            wa,
            Uuid::from_u128(id),
            "wanda",
            author,
            body,
        )
    };
    let held = row(wa, 0xC440, own, MemoryKind::Memory, "the canary held");
    let derived = row(
        wa,
        0xC441,
        own,
        MemoryKind::DerivedFact,
        "the canary derived and held",
    );
    let steering = row(
        wa,
        0xC442,
        own,
        MemoryKind::Steering,
        "canary steering, held",
    );
    let target = row(wa, 0xC443, own, MemoryKind::Memory, "the canary promoted");
    let retirement = successor_of(
        &target,
        Uuid::from_u128(0xC444),
        "retired: the canary lapsed",
        true,
    );
    let rejected = row(wa, 0xC445, own, MemoryKind::Memory, "the canary rejected");
    let away = row(
        elsewhere,
        0xC446,
        own,
        MemoryKind::Memory,
        "the canary away",
    );
    let theirs = row(
        wa,
        0xC447,
        other,
        MemoryKind::Memory,
        "the canary held by another key: canary, canary",
    );
    let their_own_words = row(wa, 0xC448, other, MemoryKind::Memory, "zebra quartz");
    for memory in [&held, &derived, &steering, &away, &theirs, &their_own_words] {
        hold(&mut rig, store, memory.clone()).await.expect("hold");
    }
    hold_and_promote(&mut rig, store, target).await;
    hold(&mut rig, store, retirement).await.expect("hold");
    hold(&mut rig, store, rejected.clone()).await.expect("hold");
    reject(&mut rig, store, rejected.memory_id)
        .await
        .expect("reject");

    let every_kind = [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Steering,
    ];
    let recall = |keys: Vec<Uuid>, readable: Vec<Uuid>, query: &'static str| async move {
        store
            .recall_own_held(rig_tenant(44), &keys, &readable, query, &every_kind, 10)
            .await
            .expect("own held recall")
    };
    let mine = [held.memory_id, derived.memory_id];
    for query in [
        "canary",
        "zebra quartz",
        "canary another key held derived steering promoted retired rejected away zebra",
        "",
    ] {
        let hits = recall(vec![own], vec![wa], query).await;
        assert!(
            hits.iter().all(|c| mine.contains(&c.memory_id)),
            "query {query:?} reached a row that is not the key's own held: {hits:?}"
        );
        assert!(
            hits.windows(2).all(|w| {
                let (a, b) = (&w[0], &w[1]);
                a.score > b.score || (a.score == b.score && a.memory_id < b.memory_id)
            }),
            "score descending, then artifact_id ascending: {hits:?}"
        );
    }
    let mut served = hit_ids(&recall(vec![own], vec![wa], "canary").await);
    served.sort();
    assert_eq!(served, mine, "the key's own held memory and derived fact");
    assert!(
        recall(vec![own], vec![wa], "zebra quartz").await.is_empty(),
        "another key's text matches nothing for this key"
    );
    assert_eq!(
        hit_ids(&recall(vec![other], vec![wa], "canary").await),
        vec![theirs.memory_id],
        "each key set reads its own"
    );
    assert!(
        recall(Vec::new(), vec![wa], "canary").await.is_empty(),
        "an empty key set reads nothing"
    );
    assert_eq!(
        hit_ids(&recall(vec![own], vec![elsewhere], "canary").await),
        vec![away.memory_id],
        "exactly the readable nodes are searched"
    );
    assert!(
        recall(vec![own], Vec::new(), "canary").await.is_empty(),
        "an empty readable set searches nothing"
    );

    let narrowed = store
        .recall_own_held(
            rig_tenant(44),
            &[own],
            &[wa],
            "canary",
            &[MemoryKind::DerivedFact],
            10,
        )
        .await
        .expect("own held recall");
    assert_eq!(hit_ids(&narrowed), vec![derived.memory_id], "kinds narrows");
    assert_eq!(
        narrowed[0].body_bytes, derived.body_bytes,
        "a candidate carries its body_bytes"
    );
    let all = recall(vec![own], vec![wa], "canary").await;
    let first = store
        .recall_own_held(rig_tenant(44), &[own], &[wa], "canary", &every_kind, 1)
        .await
        .expect("own held recall");
    assert_eq!(first, all[..1], "k keeps the prefix of the order");
}

/// Inside an in-progress scope the core hands the retrievers every author's
/// held rows on the served nodes (ADR-0030 d3): each retriever the store
/// serves reads exactly the held memories and derived facts on the nodes
/// named, of any author, never a node outside them, never a rejected,
/// steering or retirement version, and never another tenant's row, row
/// security beneath it (P-R3-BOUNDARY at the store).
pub async fn held_rows_of_every_author_are_read_on_the_named_nodes_alone<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(46);
    rig.seed_chain(store, 0).await;
    let mut other_tenant = ChainRig::new(47);
    other_tenant.seed_chain(store, 0).await;
    let [inside, outside] = [0xB460_u128, 0xB461].map(Uuid::from_u128);
    let [ann, bob] = [0xD460_u128, 0xD461].map(Uuid::from_u128);
    let row = |tenant: u64, wa: Uuid, id: u128, author: Uuid, kind: MemoryKind, body: &str| {
        MemoryRecord {
            kind,
            ..held_memory(
                rig_tenant(tenant),
                wa,
                Uuid::from_u128(id),
                "wanda",
                author,
                body,
            )
        }
    };
    let anns = row(
        46,
        inside,
        0xC460,
        ann,
        MemoryKind::Memory,
        "the canary drains",
    );
    let bobs = row(
        46,
        inside,
        0xC461,
        bob,
        MemoryKind::DerivedFact,
        "canary weights logged",
    );
    let steering = row(
        46,
        inside,
        0xC462,
        bob,
        MemoryKind::Steering,
        "canary steering",
    );
    let away = row(
        46,
        outside,
        0xC463,
        bob,
        MemoryKind::Memory,
        "the canary outside",
    );
    let rejected = row(
        46,
        inside,
        0xC464,
        bob,
        MemoryKind::Memory,
        "the canary rejected",
    );
    let foreign = row(
        47,
        inside,
        0xC465,
        bob,
        MemoryKind::Memory,
        "the canary foreign",
    );
    for memory in [&anns, &bobs, &steering, &away] {
        hold(&mut rig, store, memory.clone()).await.expect("hold");
    }
    hold(&mut rig, store, rejected.clone()).await.expect("hold");
    reject(&mut rig, store, rejected.memory_id)
        .await
        .expect("reject");
    hold(&mut other_tenant, store, foreign).await.expect("hold");

    let scope = |nodes: Vec<Uuid>| CandidateScope {
        expired_at: None,
        anchor: oml_app::types::AnchorFilter::default(),
        promoted_nodes: Vec::new(),
        kinds: vec![
            MemoryKind::Memory,
            MemoryKind::DerivedFact,
            MemoryKind::Steering,
        ],
        own_held: Some(OwnHeldScope {
            author_key_ids: Vec::new(),
            nodes,
            any_author: true,
        }),
    };
    for kind in store.retrievers() {
        // The time leg reads only a query's date window; its own laws hold it.
        if kind == RetrieverKind::Sparse || kind == RetrieverKind::Time {
            continue;
        }
        let read = |nodes: Vec<Uuid>| {
            let scope = scope(nodes);
            async move {
                let mut ids: Vec<Uuid> = store
                    .retrieve(
                        rig_tenant(46),
                        kind,
                        &scope,
                        RetrievalQuery {
                            window: None,
                            within: &[],
                            query: "canary",
                            match_mode: MatchMode::Any,
                            k: 20,
                            sparse: &[],
                        },
                    )
                    .await
                    .expect("retrieve")
                    .candidates
                    .iter()
                    .map(|c| c.memory_id)
                    .collect();
                ids.sort();
                ids
            }
        };
        let mut expected = vec![anns.memory_id, bobs.memory_id];
        expected.sort();
        assert_eq!(
            read(vec![inside]).await,
            expected,
            "{kind:?}: every author's held memory and derived fact on the node"
        );
        assert_eq!(
            read(vec![outside]).await,
            vec![away.memory_id],
            "{kind:?}: exactly the nodes named"
        );
        assert!(
            read(Vec::new()).await.is_empty(),
            "{kind:?}: no node, no row"
        );
    }
}

/// The set-wise read returns the rows named, in the order named; an id no
/// row of this tenant has is absent, not an error (ADR-0024 d4, ADR-0025
/// d7).
pub async fn memories_reads_the_named_rows_in_the_order_named<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(45);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB45);
    let [a, b, c] = [0xC451_u128, 0xC452, 0xC453].map(|id| {
        held_memory(
            rig_tenant(45),
            wa,
            Uuid::from_u128(id),
            "wanda",
            Uuid::from_u128(0xD45),
            "drain the canary",
        )
    });
    for memory in [&a, &b, &c] {
        hold(&mut rig, store, memory.clone()).await.expect("hold");
    }
    let read = store
        .memories(
            rig.tenant_id,
            &[c.memory_id, a.memory_id, Uuid::from_u128(0xFF), b.memory_id],
        )
        .await
        .expect("memories");
    assert_eq!(
        read.iter().map(|m| m.memory_id).collect::<Vec<_>>(),
        vec![c.memory_id, a.memory_id, b.memory_id]
    );
    for m in &read {
        let one = store
            .memory(rig.tenant_id, m.memory_id)
            .await
            .expect("memory")
            .expect("the row");
        assert_eq!(*m, one, "the set-wise read and the single read agree");
    }
    assert!(
        store
            .memories(rig_tenant(46), &[a.memory_id])
            .await
            .expect("memories")
            .is_empty(),
        "another tenant's rows are absent"
    );
    assert!(
        store
            .memories(rig.tenant_id, &[])
            .await
            .expect("memories")
            .is_empty()
    );
}

/// The review queue serves each item to one reviewer, only from the
/// workarea set it is handed — the reviewer's dreamer effective access
/// (ADR-0025 d1) — skips rows the reviewer's lineage excludes, and
/// re-serves a reviewer its own claim.
pub async fn journal_review_claim_laws<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(6);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB3);
    let elsewhere = Uuid::from_u128(0xB4);
    let m1 = Uuid::from_u128(0xC4);
    let m2 = Uuid::from_u128(0xC5);
    let m3 = Uuid::from_u128(0xC6);
    let author1_key = Uuid::from_u128(0xD2);
    for (id, workarea, author, key) in [
        (m1, wa, "alice", author1_key),
        (m2, wa, "bob", Uuid::from_u128(0xD3)),
        (m3, elsewhere, "bob", Uuid::from_u128(0xD3)),
    ] {
        rig.commit(
            store,
            event_types::SUBMISSION_REMEMBER,
            json!({ "artifact_id": id.to_string() }),
            MetadataFold::MemoryHeld {
                memory: held_memory(rig.tenant_id, workarea, id, author, key, "body"),
                enqueue: true,
            },
            None,
        )
        .await
        .expect("commit");
    }

    // A reviewer with no dreamer node is served nothing, never the tenant.
    let none = store
        .review_claim_next(rig.tenant_id, "erin", Uuid::from_u128(0xE4), &[])
        .await
        .expect("claim");
    assert!(none.is_none(), "an empty workarea set serves nothing");
    // alice (in m1's lineage) is served m2, never her own row.
    let alice_key = Uuid::from_u128(0xE1);
    let served = store
        .review_claim_next(rig.tenant_id, "alice", alice_key, &[wa])
        .await
        .expect("claim")
        .expect("m2 waits");
    assert_eq!(served.memory_id, m2, "lineage-excluded rows are skipped");
    // A re-claim by the same key returns the same item.
    let again = store
        .review_claim_next(rig.tenant_id, "alice", alice_key, &[wa])
        .await
        .expect("claim")
        .expect("still hers");
    assert_eq!(again.memory_id, m2);
    // An independent reviewer gets the remaining item, not alice's claim.
    let served = store
        .review_claim_next(rig.tenant_id, "carol", Uuid::from_u128(0xE2), &[wa])
        .await
        .expect("claim")
        .expect("m1 waits");
    assert_eq!(served.memory_id, m1);
    // Everything in the set is claimed: m3 waits outside it and is never
    // served from it.
    let none = store
        .review_claim_next(rig.tenant_id, "dave", Uuid::from_u128(0xE3), &[wa])
        .await
        .expect("claim");
    assert!(none.is_none(), "a row outside the set is never served");
    let served = store
        .review_claim_next(rig.tenant_id, "dave", Uuid::from_u128(0xE3), &[elsewhere])
        .await
        .expect("claim")
        .expect("m3 waits");
    assert_eq!(served.memory_id, m3);
    // Each key's claimed rows are its own: the default of a verdict sent
    // without serve_id (ADR-0039 d10).
    for (key, claimed) in [
        (alice_key, vec![m2]),
        (Uuid::from_u128(0xE2), vec![m1]),
        (Uuid::from_u128(0xE3), vec![m3]),
        (Uuid::from_u128(0xE4), vec![]),
    ] {
        assert_eq!(
            store
                .review_claimed(rig.tenant_id, key)
                .await
                .expect("claimed"),
            claimed,
            "the rows key {key} claimed"
        );
    }
}

/// The held listing serves what the queue would serve the reviewer and
/// claims nothing: only rows on the nodes it is handed, never a row whose
/// lineage holds the reviewer's key or principal, never another key's
/// claim, narrowed by author and `claim_key` prefix, oldest first and
/// resumed strictly after a position (ADR-0055 d2).
pub async fn journal_review_held_laws<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(6);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB3);
    let elsewhere = Uuid::from_u128(0xB4);
    let loader = Uuid::from_u128(0xD2);
    let other = Uuid::from_u128(0xD3);
    let reviewer = Uuid::from_u128(0xE1);
    let rows = [
        (Uuid::from_u128(0xC1), wa, "loader", loader, "adr-0001-d1"),
        (Uuid::from_u128(0xC2), wa, "loader", loader, "adr-0001-d2"),
        (Uuid::from_u128(0xC3), wa, "other", other, "note-1"),
        (Uuid::from_u128(0xC4), wa, "eugene", reviewer, "adr-0002-d1"),
        (
            Uuid::from_u128(0xC5),
            elsewhere,
            "loader",
            loader,
            "adr-0003-d1",
        ),
        (Uuid::from_u128(0xC6), wa, "loader", loader, "adr-0004-d1"),
    ];
    for (id, workarea, author, key, claim_key) in rows {
        let mut memory = held_memory(rig.tenant_id, workarea, id, author, key, "body");
        memory.claim_key = claim_key.to_owned();
        rig.commit(
            store,
            event_types::SUBMISSION_REMEMBER,
            json!({ "artifact_id": id.to_string() }),
            MetadataFold::MemoryHeld {
                memory,
                enqueue: true,
            },
            None,
        )
        .await
        .expect("commit");
    }
    // Another key claims C6 through review next; the listing then skips it.
    let claimed = store
        .review_claim_next(rig.tenant_id, "carol", Uuid::from_u128(0xE2), &[wa])
        .await
        .expect("claim")
        .expect("a row waits");
    let ids = |rows: Vec<HeldRow>| rows.iter().map(|r| r.memory.memory_id).collect::<Vec<_>>();
    let all = HeldFilter {
        nodes: vec![wa],
        author_key_id: None,
        claim_key_prefix: None,
    };
    let listed = ids(store
        .review_held(rig.tenant_id, "eugene", reviewer, &all, None, 10)
        .await
        .expect("listing"));
    let mut expected: Vec<Uuid> = [0xC1, 0xC2, 0xC3, 0xC6]
        .into_iter()
        .map(Uuid::from_u128)
        .filter(|id| *id != claimed.memory_id)
        .collect();
    expected.sort_by_key(|id| rows.iter().position(|r| r.0 == *id));
    assert_eq!(
        listed, expected,
        "the reviewer's own row, another node's row and another key's claim are never listed"
    );
    // Listing claims nothing: the queue still serves a fresh key every
    // listed row.
    let again = ids(store
        .review_held(rig.tenant_id, "eugene", reviewer, &all, None, 10)
        .await
        .expect("listing"));
    assert_eq!(again, expected, "a listing claims nothing");
    assert!(
        store
            .review_claimed(rig.tenant_id, reviewer)
            .await
            .expect("claimed")
            .is_empty(),
        "the listing key holds no claim"
    );
    // An empty node set lists nothing.
    let none = HeldFilter {
        nodes: Vec::new(),
        ..all.clone()
    };
    assert!(
        store
            .review_held(rig.tenant_id, "eugene", reviewer, &none, None, 10)
            .await
            .expect("listing")
            .is_empty()
    );
    // Author and prefix narrow together.
    let narrowed = HeldFilter {
        nodes: vec![wa],
        author_key_id: Some(loader),
        claim_key_prefix: Some("adr-0001-".to_owned()),
    };
    assert_eq!(
        ids(store
            .review_held(rig.tenant_id, "eugene", reviewer, &narrowed, None, 10)
            .await
            .expect("listing")),
        [0xC1, 0xC2]
            .into_iter()
            .map(Uuid::from_u128)
            .filter(|id| *id != claimed.memory_id)
            .collect::<Vec<_>>()
    );
    // A prefix is literal: `_` and `%` match only themselves.
    let literal = HeldFilter {
        claim_key_prefix: Some("adr_".to_owned()),
        ..all.clone()
    };
    assert!(
        store
            .review_held(rig.tenant_id, "eugene", reviewer, &literal, None, 10)
            .await
            .expect("listing")
            .is_empty(),
        "a prefix is not a pattern"
    );
    // Paging resumes strictly after the last row served.
    let first = store
        .review_held(rig.tenant_id, "eugene", reviewer, &all, None, 1)
        .await
        .expect("listing");
    let after = ListingPosition {
        created_at: first[0].held_since.clone(),
        artifact_id: first[0].memory.memory_id,
    };
    let rest = ids(store
        .review_held(rig.tenant_id, "eugene", reviewer, &all, Some(&after), 10)
        .await
        .expect("listing"));
    assert_eq!(
        rest,
        expected[1..].to_vec(),
        "the next page starts after the last"
    );
}

/// Key rows fold in with their event and resolve by value hash — one
/// lookup, never by value — name included (roles-and-keys.md hash at rest;
/// ADR-0026 d9).
pub async fn journal_key_lookup_by_hash<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(7);
    rig.seed_chain(store, 0).await;
    let record = KeyRecord {
        profile: None,
        key_id: Uuid::from_u128(0xD9),
        tenant_id: rig.tenant_id,
        role: oml_app::domain::Role::Writer,
        principal: "wanda".to_owned(),
        held_by: oml_app::domain::HeldBy::Agent,
        default_workarea: None,
        issued_by: None,
        credential: Credential::Bearer,
        key_hash: Some("sha256:feed".to_owned()),
        name: Some(name("wanda-laptop")),
        crew_id: None,
        revoked_at: None,
        user: None,
        session_user: None,
        guardrail_scope: GuardrailScope::Ancestors,
    };
    rig.commit(
        store,
        event_types::KEY_ISSUED,
        json!({ "key_id": record.key_id.to_string(), "value_disclosed": false }),
        MetadataFold::KeyIssued(record.clone()),
        None,
    )
    .await
    .expect("commit key");
    let found = store
        .key_by_hash("sha256:feed")
        .await
        .expect("lookup")
        .expect("the key resolves");
    assert_eq!(found, record);
    assert_eq!(
        store.key_by_hash("sha256:beef").await.expect("lookup"),
        None
    );
}

/// A revoked key resolves by its hash no more, from the commit that
/// journalled `key.revoked` on, while its row stays as history; revoking a
/// key the tenant does not hold, or one already revoked, is refused and
/// commits nothing (roles-and-keys.md key lifecycle; ADR-0025 d2, the
/// `issuance_incomplete` revocation).
pub async fn a_revoked_key_no_longer_resolves_by_its_hash<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(21);
    rig.seed_chain(store, 0).await;
    let key = key_record(rig.tenant_id, Uuid::from_u128(0xDA), Role::Writer, None);
    rig.issue_key(store, key.clone()).await;
    assert_eq!(
        store
            .key_by_hash(key.key_hash.as_deref().expect("a bearer key"))
            .await
            .expect("lookup"),
        Some(key.clone())
    );

    rig.revoke_key(store, key.key_id).await.expect("revoke");
    assert_eq!(
        store
            .key_by_hash(key.key_hash.as_deref().expect("a bearer key"))
            .await
            .expect("lookup"),
        None,
        "a revoked key authenticates nothing"
    );
    assert!(
        store
            .key_access(rig.tenant_id, key.key_id)
            .await
            .expect("read")
            .is_some(),
        "the revoked key's row stays as history"
    );

    let head = store
        .chain_head(rig.tenant_id)
        .await
        .expect("head")
        .expect("chain");
    for key_id in [Uuid::from_u128(0xDB), key.key_id] {
        let refused = rig.revoke_key(store, key_id).await;
        assert!(
            matches!(refused, Err(StoreError::Backend(_))),
            "{refused:?}"
        );
    }
    assert_eq!(
        store.chain_head(rig.tenant_id).await.expect("head"),
        Some(head),
        "a refused revocation commits nothing"
    );
}

/// The idempotency record rides the commit and reads back whole; an absent
/// scope is absent (http-api.md idempotency record).
pub async fn journal_idempotency_roundtrip<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(8);
    rig.seed_chain(store, 0).await;
    let scope = IdempotencyScope {
        tenant_id: rig.tenant_id,
        key_id: Uuid::from_u128(0xD9),
        workarea_id: Uuid::from_u128(0xB9),
        idem_key: "idem-1".to_owned(),
    };
    assert_eq!(
        store.idempotency_get(&scope).await.expect("get"),
        None,
        "nothing before the commit"
    );
    let record = IdempotencyRecord {
        request_digest: "sha256:aa".to_owned(),
        response: json!({ "state": "held" }),
    };
    let fold = rig.genesis_fold();
    rig.commit(
        store,
        event_types::SUBMISSION_REMEMBER,
        json!({}),
        fold,
        Some(IdempotencyPut {
            scope: scope.clone(),
            record: record.clone(),
        }),
    )
    .await
    .expect("commit with idempotency");
    assert_eq!(
        store.idempotency_get(&scope).await.expect("get"),
        Some(record)
    );
    let other = IdempotencyScope {
        idem_key: "idem-2".to_owned(),
        ..scope
    };
    assert_eq!(store.idempotency_get(&other).await.expect("get"), None);
}

/// One scope holds one record: a commit carrying a second record under a
/// recorded scope is refused whole, so an attempt that looked the key up
/// before a concurrent one committed journals nothing, and the first record
/// stands (http-api.md idempotency; ADR-0025 d10).
pub async fn a_second_record_under_one_idempotency_scope_commits_nothing<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(26);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB26);
    let scope = IdempotencyScope {
        tenant_id: rig.tenant_id,
        key_id: Uuid::from_u128(0xD26),
        workarea_id: Uuid::nil(),
        idem_key: "sup-1".to_owned(),
    };
    let put = |memory: &MemoryRecord| IdempotencyPut {
        scope: scope.clone(),
        record: IdempotencyRecord {
            request_digest: "sha256:aa".to_owned(),
            response: json!({ "artifact_id": memory.memory_id.to_string() }),
        },
    };
    let winner = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC26),
        "wanda",
        scope.key_id,
        "the canary drains first",
    );
    let loser = MemoryRecord {
        memory_id: Uuid::from_u128(0xC27),
        claim_key: "another-claim".to_owned(),
        ..winner.clone()
    };
    let held = |memory: &MemoryRecord| MetadataFold::MemoryHeld {
        memory: memory.clone(),
        enqueue: true,
    };
    rig.commit(
        store,
        event_types::SUBMISSION_REMEMBER,
        json!({}),
        held(&winner),
        Some(put(&winner)),
    )
    .await
    .expect("the first record");
    let head = store.chain_head(rig.tenant_id).await.expect("head");

    let err = rig
        .commit(
            store,
            event_types::SUBMISSION_REMEMBER,
            json!({}),
            held(&loser),
            Some(put(&loser)),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::IdempotencyRecorded), "{err:?}");
    assert_eq!(
        store.chain_head(rig.tenant_id).await.expect("head"),
        head,
        "nothing is journalled"
    );
    assert_eq!(
        store
            .memory(rig.tenant_id, loser.memory_id)
            .await
            .expect("memory"),
        None,
        "nothing is folded"
    );
    assert_eq!(
        store.idempotency_get(&scope).await.expect("get"),
        Some(put(&winner).record),
        "the first record stands"
    );
}

// ---------------------------------------------------------------------------
// Grants, denials and the workarea tree (ADR-0025 d1, d2, d14)
// ---------------------------------------------------------------------------

/// Grant and denial rows fold in with their events and read back whole
/// through `key_access`, in epoch order, deny lists in their given order,
/// and the key row carries its issuer; a key the tenant does not hold has no
/// access state (postgres-schema.md grants, key_denials; workareas.md).
pub async fn grants_and_denials_round_trip_through_their_folds_and_key_access<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(9);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let universe = Uuid::from_u128(0xB10);
    let crew_a = Uuid::from_u128(0xB11);
    let masked = Uuid::from_u128(0xB12);
    let masked_too = Uuid::from_u128(0xB0E);
    let crew_b = Uuid::from_u128(0xB13);
    for w in [
        workarea_record(t, universe, None, "universe"),
        workarea_record(t, crew_a, Some(universe), "crew-a"),
        workarea_record(t, masked, Some(crew_a), "masked"),
        workarea_record(t, masked_too, Some(crew_a), "masked-too"),
        workarea_record(t, crew_b, Some(universe), "crew-b"),
    ] {
        rig.create_workarea(store, w).await;
    }
    let admin = key_record(t, Uuid::from_u128(0xD10), Role::Admin, None);
    let agent = key_record(t, Uuid::from_u128(0xD11), Role::Writer, Some(admin.key_id));
    let admin_issued = rig.issue_key(store, admin.clone()).await;
    let agent_issued = rig.issue_key(store, agent.clone()).await;

    // Grant ids run against epoch order, and the deny list against id
    // order: the read must keep the journal's order, not sort by id.
    let writer_grant = Grant {
        grant_id: Uuid::from_u128(0xF11),
        node: crew_a,
        role: Role::Writer,
        deny: vec![masked, masked_too],
        covers: Covers::Subtree,
    };
    let reader_grant = Grant {
        grant_id: Uuid::from_u128(0xF10),
        node: crew_a,
        role: Role::Reader,
        deny: Vec::new(),
        covers: Covers::Subtree,
    };
    let first = rig
        .issue_grant(store, agent.key_id, writer_grant.clone())
        .await
        .expect("writer grant");
    let second = rig
        .issue_grant(store, agent.key_id, reader_grant.clone())
        .await
        .expect("reader grant");
    let denial = KeyDenial {
        denial_id: Uuid::from_u128(0xF20),
        node: crew_b,
    };
    let denied = rig
        .deny_key(store, agent.key_id, denial.clone(), admin.key_id)
        .await
        .expect("denial");

    assert_eq!(
        store.key_access(t, agent.key_id).await.expect("read"),
        Some(KeyAccess {
            issued_seq: agent_issued.seq,
            grants: vec![
                GrantRecord {
                    grant: writer_grant,
                    epoch: first.seq,
                },
                GrantRecord {
                    grant: reader_grant,
                    epoch: second.seq,
                },
            ],
            denials: vec![DenialRecord {
                denial,
                created_by: admin.key_id,
                epoch: denied.seq,
            }],
            lift_epochs: Vec::new(),
        }),
    );
    assert_eq!(
        store.key_access(t, admin.key_id).await.expect("read"),
        Some(KeyAccess {
            issued_seq: admin_issued.seq,
            grants: Vec::new(),
            denials: Vec::new(),
            lift_epochs: Vec::new(),
        }),
        "a key with no grant reads back empty lists, never nothing"
    );
    assert_eq!(
        store
            .key_by_hash(agent.key_hash.as_deref().expect("a bearer key"))
            .await
            .expect("lookup"),
        Some(agent.clone()),
        "the key row carries the key that issued it"
    );
    assert_eq!(
        store
            .key_access(t, Uuid::from_u128(0xDFF))
            .await
            .expect("read"),
        None,
        "an unknown key has no access state"
    );
    let mut other = ChainRig::new(10);
    other.seed_chain(store, 0).await;
    assert_eq!(
        store
            .key_access(other.tenant_id, agent.key_id)
            .await
            .expect("read"),
        None,
        "another tenant does not hold the key"
    );
}

/// A grant's and a denial's epoch, and a key's `issued_seq`, are the journal
/// seq of the event that wrote the row — never a count of grants, never 1
/// (workareas.md permission epoch; event-catalog.md `grant.issued`).
pub async fn a_grant_epoch_is_its_event_seq<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(11);
    rig.seed_chain(store, 2).await; // seq 1..=3
    let t = rig.tenant_id;
    let node = Uuid::from_u128(0xB20);
    rig.create_workarea(store, workarea_record(t, node, None, "universe"))
        .await; // seq 4
    let key = key_record(t, Uuid::from_u128(0xD20), Role::Writer, None);
    let issued = rig.issue_key(store, key.clone()).await; // seq 5
    rig.filler(store).await; // seq 6
    rig.filler(store).await; // seq 7
    let granted = rig
        .issue_grant(
            store,
            key.key_id,
            Grant {
                grant_id: Uuid::from_u128(0xF21),
                node,
                role: Role::Reader,
                deny: Vec::new(),
                covers: Covers::Subtree,
            },
        )
        .await
        .expect("grant"); // seq 8
    rig.filler(store).await; // seq 9
    let denied = rig
        .deny_key(
            store,
            key.key_id,
            KeyDenial {
                denial_id: Uuid::from_u128(0xF22),
                node,
            },
            Uuid::from_u128(0xD2F),
        )
        .await
        .expect("denial"); // seq 10
    assert_eq!(
        (issued.seq, granted.seq, denied.seq),
        (5, 8, 10),
        "the fixture's own seqs"
    );

    let access = store
        .key_access(t, key.key_id)
        .await
        .expect("read")
        .expect("the key is held");
    assert_eq!(
        (
            access.issued_seq,
            access.grants.iter().map(|g| g.epoch).collect::<Vec<_>>(),
            access.denials.iter().map(|d| d.epoch).collect::<Vec<_>>(),
        ),
        (5, vec![8], vec![10]),
        "each row's epoch is its own event's seq"
    );
}

/// A grant or denial the schema refuses — an unknown key or node, a key or
/// node another tenant holds, or a grant role outside reader, writer and
/// dreamer — fails its commit whole: no event, no row (postgres-schema.md
/// grants CHECK and tenant-scoped foreign keys; ADR-0002 one-transaction
/// commit). Each case carries its own id, so one wrongly accepted row cannot
/// make a later case fail on a duplicate id instead.
pub async fn a_grant_or_denial_the_schema_refuses_commits_nothing<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(12);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let node = Uuid::from_u128(0xB30);
    rig.create_workarea(store, workarea_record(t, node, None, "universe"))
        .await;
    let key = key_record(t, Uuid::from_u128(0xD30), Role::Writer, None);
    rig.issue_key(store, key.clone()).await;
    let mut other = ChainRig::new(20);
    other.seed_chain(store, 0).await;
    let foreign_node = Uuid::from_u128(0xB31);
    other
        .create_workarea(
            store,
            workarea_record(other.tenant_id, foreign_node, None, "universe"),
        )
        .await;
    let foreign_key = key_record(other.tenant_id, Uuid::from_u128(0xD31), Role::Writer, None);
    other.issue_key(store, foreign_key.clone()).await;
    let head = store.chain_head(t).await.expect("head");
    let unknown = Uuid::from_u128(0xBFF);
    let grant = |id, node, role| Grant {
        grant_id: Uuid::from_u128(id),
        node,
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    };
    let denial = |id, node| KeyDenial {
        denial_id: Uuid::from_u128(id),
        node,
    };

    let refusals = [
        (
            "a grant on an unknown key",
            rig.issue_grant(store, unknown, grant(0xF30, node, Role::Reader))
                .await,
        ),
        (
            "a grant on an unknown node",
            rig.issue_grant(store, key.key_id, grant(0xF31, unknown, Role::Reader))
                .await,
        ),
        (
            "a grant on another tenant's key",
            rig.issue_grant(store, foreign_key.key_id, grant(0xF32, node, Role::Reader))
                .await,
        ),
        (
            "a grant on another tenant's node",
            rig.issue_grant(store, key.key_id, grant(0xF33, foreign_node, Role::Reader))
                .await,
        ),
        (
            "an admin grant",
            rig.issue_grant(store, key.key_id, grant(0xF34, node, Role::Admin))
                .await,
        ),
        (
            "an operator grant",
            rig.issue_grant(store, key.key_id, grant(0xF35, node, Role::Operator))
                .await,
        ),
        (
            "a denial on an unknown key",
            rig.deny_key(store, unknown, denial(0xF36, node), key.key_id)
                .await,
        ),
        (
            "a denial on an unknown node",
            rig.deny_key(store, key.key_id, denial(0xF37, unknown), key.key_id)
                .await,
        ),
        (
            "a denial on another tenant's key",
            rig.deny_key(store, foreign_key.key_id, denial(0xF38, node), key.key_id)
                .await,
        ),
        (
            "a denial on another tenant's node",
            rig.deny_key(store, key.key_id, denial(0xF39, foreign_node), key.key_id)
                .await,
        ),
    ];
    for (case, result) in refusals {
        assert!(
            matches!(result, Err(StoreError::Backend(_))),
            "{case} is refused, got {result:?}"
        );
    }
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "a refused fold journals nothing"
    );
    let untouched = Some(KeyAccess {
        issued_seq: 3,
        grants: Vec::new(),
        denials: Vec::new(),
        lift_epochs: Vec::new(),
    });
    assert_eq!(
        store.key_access(t, key.key_id).await.expect("read"),
        untouched,
        "a refused fold writes no row"
    );
    assert_eq!(
        store
            .key_access(other.tenant_id, foreign_key.key_id)
            .await
            .expect("read"),
        untouched,
        "nor any row on the other tenant's key"
    );
}

/// A lift folds with its `key.denial_lifted`: from that commit on the
/// denial is in force no more, so `key_access` lists it among the key's
/// denials no longer and lists the lift's seq among its lift epochs, which
/// the permission epoch counts, while the key's other denials stay in force.
/// A lift of a denial the tenant does not hold, of one held on another key,
/// or of one already lifted is refused and commits nothing
/// (postgres-schema.md `key_denials`; workareas.md permission epoch;
/// event-catalog.md `key.denial_lifted`; ADR-0026 d11, rule 5).
pub async fn a_lifted_denial_masks_nothing_and_its_lift_moves_the_permission_epoch<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(46);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let (universe, team) = (Uuid::from_u128(0xB460), Uuid::from_u128(0xB461));
    rig.create_workarea(store, workarea_record(t, universe, None, "universe"))
        .await;
    rig.create_workarea(store, workarea_record(t, team, Some(universe), "team"))
        .await;
    let key = key_record(t, Uuid::from_u128(0xD460), Role::Writer, None);
    let other_key = key_record(t, Uuid::from_u128(0xD461), Role::Writer, None);
    let issued = rig.issue_key(store, key.clone()).await;
    rig.issue_key(store, other_key.clone()).await;
    let denial = |id: u128, node| KeyDenial {
        denial_id: Uuid::from_u128(id),
        node,
    };
    let (lifted, kept) = (denial(0xF460, team), denial(0xF461, universe));
    rig.deny_key(store, key.key_id, lifted.clone(), other_key.key_id)
        .await
        .expect("the denial to lift");
    let kept_at = rig
        .deny_key(store, key.key_id, kept.clone(), other_key.key_id)
        .await
        .expect("the denial that stays");
    let lift = rig
        .lift_denial(store, key.key_id, lifted.denial_id)
        .await
        .expect("the lift folds");

    let access = store
        .key_access(t, key.key_id)
        .await
        .expect("read")
        .expect("the key is held");
    assert_eq!(
        access,
        KeyAccess {
            issued_seq: issued.seq,
            grants: Vec::new(),
            denials: vec![DenialRecord {
                denial: kept,
                created_by: other_key.key_id,
                epoch: kept_at.seq,
            }],
            lift_epochs: vec![lift.seq],
        },
        "the lifted denial masks nothing; the other stays in force"
    );
    assert_eq!(
        access.permission_epoch(),
        lift.seq,
        "the lift is the latest"
    );

    let head = store.chain_head(t).await.expect("head");
    let refusals = [
        (
            "a lift of a denial already lifted",
            rig.lift_denial(store, key.key_id, lifted.denial_id).await,
        ),
        (
            "a lift of a denial no row holds",
            rig.lift_denial(store, key.key_id, Uuid::from_u128(0xF46F))
                .await,
        ),
        (
            "a lift of a denial held on another key",
            rig.lift_denial(store, other_key.key_id, Uuid::from_u128(0xF461))
                .await,
        ),
    ];
    for (case, result) in refusals {
        assert!(
            matches!(result, Err(StoreError::Backend(_))),
            "{case} is refused, got {result:?}"
        );
    }
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "a refused lift journals nothing"
    );
    assert_eq!(
        store.key_access(t, key.key_id).await.expect("read"),
        Some(access),
        "a refused lift changes no row"
    );
}

/// The ancestry of a node is the node and each parent in turn, up to the
/// root, whatever order the ids run in; an unknown node has none
/// (workareas.md effective access, one ancestry walk per request).
pub async fn workarea_ancestry_walks_parent_ids_to_the_root<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(13);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let universe = workarea_record(t, Uuid::from_u128(0xB49), None, "universe");
    let research = workarea_record(
        t,
        Uuid::from_u128(0xB41),
        Some(universe.workarea_id),
        "research",
    );
    let experiments = workarea_record(
        t,
        Uuid::from_u128(0xB45),
        Some(research.workarea_id),
        "experiments",
    );
    let run = workarea_record(
        t,
        Uuid::from_u128(0xB42),
        Some(experiments.workarea_id),
        "run-7",
    );
    let sibling = workarea_record(t, Uuid::from_u128(0xB40), Some(universe.workarea_id), "ops");
    for w in [&universe, &research, &experiments, &run, &sibling] {
        rig.create_workarea(store, w.clone()).await;
    }

    assert_eq!(
        store
            .workarea_ancestry(t, run.workarea_id)
            .await
            .expect("walk"),
        Some(vec![
            run.clone(),
            experiments.clone(),
            research.clone(),
            universe.clone()
        ]),
        "node first, root last, by parent_id — not by id"
    );
    assert_eq!(
        store
            .workarea_ancestry(t, universe.workarea_id)
            .await
            .expect("walk"),
        Some(vec![universe.clone()]),
        "the root's ancestry is itself"
    );
    assert_eq!(
        store
            .workarea_ancestry(t, Uuid::from_u128(0xBFF))
            .await
            .expect("walk"),
        None,
        "an unknown node has no ancestry"
    );
    assert_eq!(
        store
            .workarea(t, experiments.workarea_id)
            .await
            .expect("read"),
        Some(experiments)
    );
    assert_eq!(
        store
            .workarea(t, Uuid::from_u128(0xBFF))
            .await
            .expect("read"),
        None,
        "unknown ids are absent, not errors"
    );
    let mut other = ChainRig::new(14);
    other.seed_chain(store, 0).await;
    assert_eq!(
        store
            .workarea_ancestry(other.tenant_id, run.workarea_id)
            .await
            .expect("walk"),
        None,
        "another tenant does not hold the node"
    );
}

/// A workarea whose parent is another tenant's node is refused at its
/// commit as one whose parent is no node at all: no event, no row, so no
/// ancestry can leave its tenant. PostgreSQL runs a foreign-key check
/// without row-level security, so the parent reference carries the
/// tenant, `(tenant_id, parent_id)` (postgres-schema.md `workareas`;
/// ADR-0034 d12).
pub async fn a_workarea_under_another_tenants_node_is_refused_as_one_under_no_node<
    S: JournalStore,
>(
    store: &S,
) {
    let mut home = ChainRig::new(18);
    home.seed_chain(store, 0).await;
    let foreign_root = Uuid::from_u128(0xB89);
    home.create_workarea(
        store,
        workarea_record(home.tenant_id, foreign_root, None, "universe"),
    )
    .await;
    let mut rig = ChainRig::new(19);
    rig.seed_chain(store, 0).await;
    let root = Uuid::from_u128(0xB80);
    rig.create_workarea(
        store,
        workarea_record(rig.tenant_id, root, None, "universe"),
    )
    .await;
    let head = store.chain_head(rig.tenant_id).await.expect("head");

    for (case, stray, parent) in [
        ("another tenant's node", 0xB81, foreign_root),
        ("no node", 0xB82, Uuid::from_u128(0xBFF)),
    ] {
        let stray = Uuid::from_u128(stray);
        let refused = rig
            .commit(
                store,
                event_types::WORKAREA_CREATE,
                json!({ "workarea_id": stray.to_string(), "name": "stray" }),
                MetadataFold::WorkareaCreated(workarea_record(
                    rig.tenant_id,
                    stray,
                    Some(parent),
                    "stray",
                )),
                None,
            )
            .await;
        assert!(
            matches!(refused, Err(StoreError::Backend(_))),
            "a workarea under {case} is refused, got {refused:?}"
        );
        assert_eq!(
            store.workarea(rig.tenant_id, stray).await.expect("read"),
            None,
            "a workarea under {case} writes no row"
        );
    }
    assert_eq!(
        store.chain_head(rig.tenant_id).await.expect("head"),
        head,
        "a refused fold journals nothing"
    );
}

/// The subtree of a node lists the node and every descendant exactly once,
/// the node first and then each depth in id order, so a parent always
/// precedes its children; an unknown node's subtree is empty
/// (ADR-0025 d1: recall searches subtree(scope) ∩ the reader set).
pub async fn workarea_subtree_lists_every_descendant_once<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(15);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let id = |n: u128| Uuid::from_u128(0xB50 + n);
    // universe(9) ─┬─ a(1) ─┬─ a1(6) ── a1x(0)
    //              │        └─ a2(3)
    //              └─ b(2) ─── b1(4)
    let tree = [
        workarea_record(t, id(9), None, "universe"),
        workarea_record(t, id(1), Some(id(9)), "a"),
        workarea_record(t, id(6), Some(id(1)), "a1"),
        workarea_record(t, id(0), Some(id(6)), "a1x"),
        workarea_record(t, id(3), Some(id(1)), "a2"),
        workarea_record(t, id(2), Some(id(9)), "b"),
        workarea_record(t, id(4), Some(id(2)), "b1"),
    ];
    for w in &tree {
        rig.create_workarea(store, w.clone()).await;
    }
    let ids = |rows: Vec<WorkareaRecord>| rows.iter().map(|w| w.workarea_id).collect::<Vec<_>>();

    assert_eq!(
        ids(store.workarea_subtree(t, id(1)).await.expect("subtree")),
        vec![id(1), id(3), id(6), id(0)],
        "a's subtree: a, then depth 1 by id, then depth 2 — no sibling, no ancestor"
    );
    let whole = store.workarea_subtree(t, id(9)).await.expect("subtree");
    assert_eq!(
        ids(whole.clone()),
        vec![id(9), id(1), id(2), id(3), id(4), id(6), id(0)],
        "the root's subtree is every node, each once"
    );
    for row in &whole {
        assert!(tree.contains(row), "rows read back whole: {row:?}");
    }
    assert_eq!(
        ids(store.workarea_subtree(t, id(0)).await.expect("subtree")),
        vec![id(0)],
        "a leaf's subtree is itself"
    );
    assert!(
        store
            .workarea_subtree(t, Uuid::from_u128(0xBFF))
            .await
            .expect("subtree")
            .is_empty(),
        "an unknown node's subtree is empty"
    );
}

/// The children of a node are its immediate children only, ordered by name
/// in byte order — the order the workarea read serves them in (ADR-0025
/// d14; openapi.yaml `WorkareaView.children`). Names are unique in the
/// tenant, so no two children tie (ADR-0026 d1).
pub async fn workarea_children_are_immediate_and_ordered_by_name_bytes<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(16);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let parent = Uuid::from_u128(0xB69);
    let id = |n: u128| Uuid::from_u128(0xB60 + n);
    rig.create_workarea(store, workarea_record(t, parent, None, "universe"))
        .await;
    // Byte order puts "a-z" before "ab", where a linguistic collation that
    // skips the hyphen would put it after; the ids run against the names.
    for (n, name) in [(4, "a-z"), (3, "ab"), (2, "beta"), (1, "q9"), (0, "qa")] {
        rig.create_workarea(store, workarea_record(t, id(n), Some(parent), name))
            .await;
    }
    rig.create_workarea(store, workarea_record(t, id(5), Some(id(3)), "aaa"))
        .await;

    let children = store.workarea_children(t, parent).await.expect("children");
    assert_eq!(
        children
            .iter()
            .map(|w| (w.name.as_str(), w.workarea_id))
            .collect::<Vec<_>>(),
        vec![
            ("a-z", id(4)),
            ("ab", id(3)),
            ("beta", id(2)),
            ("q9", id(1)),
            ("qa", id(0)),
        ],
        "immediate children only, by name bytes"
    );
    assert!(
        store
            .workarea_children(t, id(2))
            .await
            .expect("children")
            .is_empty(),
        "a leaf has no children"
    );
    assert!(
        store
            .workarea_children(t, Uuid::from_u128(0xBFF))
            .await
            .expect("children")
            .is_empty(),
        "an unknown node has no children"
    );
}

/// The status is one stored column, not a value derived from the node's
/// place in the tree: every workarea read serves the status its row
/// stores, a child stored `committed` or `validated` among them
/// (ADR-0009 d1; postgres-schema.md `workareas.status`).
pub async fn a_workarea_read_serves_the_status_its_row_stores<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(47);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let universe = workarea_record(t, Uuid::from_u128(0xB410), None, "universe");
    let committed = WorkareaRecord {
        status: WorkareaStatus::Committed,
        ..workarea_record(
            t,
            Uuid::from_u128(0xB411),
            Some(universe.workarea_id),
            "procurement",
        )
    };
    let validated = WorkareaRecord {
        status: WorkareaStatus::Validated,
        ..workarea_record(
            t,
            Uuid::from_u128(0xB412),
            Some(committed.workarea_id),
            "metals",
        )
    };
    let development = workarea_record(
        t,
        Uuid::from_u128(0xB413),
        Some(committed.workarea_id),
        "redline",
    );
    for w in [&universe, &committed, &validated, &development] {
        rig.create_workarea(store, w.clone()).await;
    }
    let statuses = |rows: Vec<WorkareaRecord>| -> Vec<(Uuid, WorkareaStatus)> {
        rows.into_iter()
            .map(|w| (w.workarea_id, w.status))
            .collect()
    };

    for w in [&universe, &committed, &validated, &development] {
        assert_eq!(
            store
                .workarea(t, w.workarea_id)
                .await
                .expect("read")
                .as_ref(),
            Some(w),
            "the read serves {:?} as stored",
            w.status
        );
    }
    assert_eq!(
        statuses(
            store
                .workarea_ancestry(t, validated.workarea_id)
                .await
                .expect("ancestry")
                .expect("held")
        ),
        vec![
            (validated.workarea_id, WorkareaStatus::Validated),
            (committed.workarea_id, WorkareaStatus::Committed),
            (universe.workarea_id, WorkareaStatus::Committed),
        ]
    );
    assert_eq!(
        statuses(
            store
                .workarea_children(t, committed.workarea_id)
                .await
                .expect("children")
        ),
        vec![
            (validated.workarea_id, WorkareaStatus::Validated),
            (development.workarea_id, WorkareaStatus::Development),
        ]
    );
    assert_eq!(
        statuses(
            store
                .workarea_subtree(t, committed.workarea_id)
                .await
                .expect("subtree")
        ),
        vec![
            (committed.workarea_id, WorkareaStatus::Committed),
            (validated.workarea_id, WorkareaStatus::Validated),
            (development.workarea_id, WorkareaStatus::Development),
        ]
    );
}

/// The universe root is permanently `committed`: a root created at any
/// other status is refused at its commit, no event and no row, so no later
/// write can leave a tenant whose root sits on another rung (ADR-0009 d1;
/// postgres-schema.md `workareas_universe_committed`).
pub async fn a_universe_root_created_at_any_status_but_committed_is_refused<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(48);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let head = store.chain_head(t).await.expect("head");

    for (case, raw, status) in [
        ("development", 0xB420, WorkareaStatus::Development),
        ("validated", 0xB421, WorkareaStatus::Validated),
    ] {
        let root = WorkareaRecord {
            status,
            ..workarea_record(t, Uuid::from_u128(raw), None, "universe")
        };
        let refused = rig
            .commit(
                store,
                event_types::WORKAREA_CREATE,
                json!({ "workarea_id": root.workarea_id.to_string(), "name": "universe" }),
                MetadataFold::WorkareaCreated(root.clone()),
                None,
            )
            .await;
        assert!(
            matches!(refused, Err(StoreError::Backend(_))),
            "a universe root stored {case} is refused, got {refused:?}"
        );
        assert_eq!(
            store.workarea(t, root.workarea_id).await.expect("read"),
            None,
            "a universe root stored {case} writes no row"
        );
    }
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "a refused root journals nothing"
    );
}

/// A fixture pending proposal of one rung forward from `from` on
/// `workarea_id` by `proposer_key_id`, under the principal [`key_record`]
/// gives that key.
#[must_use]
pub fn pending_transition(
    tenant_id: Uuid,
    transition_id: Uuid,
    workarea_id: Uuid,
    from: WorkareaStatus,
    proposer_key_id: Uuid,
) -> TransitionRecord {
    TransitionRecord {
        transition_id,
        tenant_id,
        workarea_id,
        from,
        to: match from {
            WorkareaStatus::Development => WorkareaStatus::Committed,
            WorkareaStatus::Committed | WorkareaStatus::Validated => WorkareaStatus::Validated,
        },
        state: TransitionState::Pending,
        proposer_key_id,
        proposer_principal: format!("fixture/{proposer_key_id}"),
        ratifier_key_id: None,
    }
}

/// A rig with a universe root, `node` under it in `development`, and two
/// admin keys, `proposer` and `ratifier`: the ground every transition law
/// stands on.
async fn transition_ground<S: JournalStore>(
    store: &S,
    seed: u64,
    node: Uuid,
    keys: (Uuid, Uuid),
) -> ChainRig {
    let mut rig = ChainRig::new(seed);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let root = Uuid::from_u128(node.as_u128() + 0x100);
    rig.create_workarea(store, workarea_record(t, root, None, "universe"))
        .await;
    rig.create_workarea(
        store,
        workarea_record(t, node, Some(root), &format!("area-{seed}")),
    )
    .await;
    for key in [keys.0, keys.1] {
        rig.issue_key(store, key_record(t, key, Role::Admin, None))
            .await;
    }
    rig
}

/// A proposal is a pending row the store serves back whole, by its id and
/// in its node's pending list, which lists the node's proposals in the
/// order they were proposed, not in id order, and no other node's
/// (ADR-0009 d3; workareas.md, several proposals may wait on one node).
pub async fn a_proposed_transition_is_pending_and_listed_by_its_workarea_in_proposal_order<
    S: JournalStore,
>(
    store: &S,
) {
    let node = Uuid::from_u128(0xB490);
    let (proposer, ratifier) = (Uuid::from_u128(0xD490), Uuid::from_u128(0xD491));
    let mut rig = transition_ground(store, 49, node, (proposer, ratifier)).await;
    let t = rig.tenant_id;
    let other = Uuid::from_u128(0xB491);
    rig.create_workarea(
        store,
        workarea_record(t, other, Some(Uuid::from_u128(0xB590)), "area-49-other"),
    )
    .await;

    // The later proposal on `node` carries the smaller id.
    let first = pending_transition(
        t,
        Uuid::from_u128(0xE492),
        node,
        WorkareaStatus::Development,
        proposer,
    );
    let elsewhere = pending_transition(
        t,
        Uuid::from_u128(0xE493),
        other,
        WorkareaStatus::Development,
        ratifier,
    );
    let second = pending_transition(
        t,
        Uuid::from_u128(0xE491),
        node,
        WorkareaStatus::Development,
        ratifier,
    );
    for proposal in [&first, &elsewhere, &second] {
        rig.propose_transition(store, proposal.clone())
            .await
            .expect("a pending proposal one rung forward commits");
    }

    assert_eq!(
        store
            .transition(t, first.transition_id)
            .await
            .expect("read"),
        Some(first.clone()),
        "the proposal reads back whole and pending"
    );
    assert_eq!(
        store.pending_transitions(t, node).await.expect("pending"),
        vec![first.clone(), second.clone()],
        "the node's proposals, in proposal order"
    );
    assert_eq!(
        store.pending_transitions(t, other).await.expect("pending"),
        vec![elsewhere],
        "another node's list holds only its own"
    );
    assert_eq!(
        store
            .transition(t, Uuid::from_u128(0xE49F))
            .await
            .expect("read"),
        None,
        "an id no proposal carries reads as none"
    );
    assert_eq!(
        store
            .pending_transitions(t, Uuid::from_u128(0xB49F))
            .await
            .expect("pending"),
        Vec::<TransitionRecord>::new(),
        "a node the tenant does not hold lists none"
    );
    let stranger = ChainRig::new(52).tenant_id;
    assert_eq!(
        store
            .transition(stranger, first.transition_id)
            .await
            .expect("read"),
        None,
        "another tenant reads none of this tenant's proposals"
    );
    assert_eq!(
        store
            .pending_transitions(stranger, node)
            .await
            .expect("pending"),
        Vec::<TransitionRecord>::new(),
        "another tenant lists none of this tenant's proposals"
    );
}

/// A ratification is one commit: the row turns `ratified` with its
/// ratifier and the node takes the row's `to`, both with the
/// `workarea.status_transition` event at the head; a chain whose later link
/// is refused leaves the row pending and the node where it was. A
/// proposal another ratification overtook stays pending as history
/// (ADR-0002 d1; ADR-0009 d3; workareas.md, ratification re-checks).
pub async fn ratifying_a_transition_moves_the_workareas_status_in_the_same_commit_as_its_event<
    S: JournalStore,
>(
    store: &S,
) {
    let node = Uuid::from_u128(0xB500);
    let (proposer, ratifier) = (Uuid::from_u128(0xD500), Uuid::from_u128(0xD501));
    let mut rig = transition_ground(store, 50, node, (proposer, ratifier)).await;
    let t = rig.tenant_id;
    let proposal = pending_transition(
        t,
        Uuid::from_u128(0xE500),
        node,
        WorkareaStatus::Development,
        proposer,
    );
    let overtaken = pending_transition(
        t,
        Uuid::from_u128(0xE501),
        node,
        WorkareaStatus::Development,
        proposer,
    );
    for p in [&proposal, &overtaken] {
        rig.propose_transition(store, p.clone())
            .await
            .expect("proposal");
    }

    // The ratification and a link the store refuses, as one chain.
    let head = store.chain_head(t).await.expect("head");
    let refused = rig
        .commit_chain(
            store,
            vec![
                ChainRig::ratification(
                    proposal.transition_id,
                    node,
                    WorkareaStatus::Committed,
                    ratifier,
                ),
                (
                    event_types::KEY_REVOKED,
                    json!({ "key_id": Uuid::from_u128(0xD50F).to_string() }),
                    MetadataFold::KeyRevoked {
                        key_id: Uuid::from_u128(0xD50F),
                    },
                ),
            ],
        )
        .await;
    assert!(
        refused.is_err(),
        "a chain with a refused link commits nothing"
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(
        store
            .workarea(t, node)
            .await
            .expect("read")
            .map(|w| w.status),
        Some(WorkareaStatus::Development),
        "the refused chain moved no status"
    );
    assert_eq!(
        store
            .transition(t, proposal.transition_id)
            .await
            .expect("read"),
        Some(proposal.clone()),
        "the refused chain ratified nothing"
    );

    let receipt = rig
        .ratify_transition(
            store,
            proposal.transition_id,
            node,
            WorkareaStatus::Committed,
            ratifier,
        )
        .await
        .expect("a pending proposal whose from is the node's status ratifies");
    let head = store.chain_head(t).await.expect("head").expect("a head");
    assert_eq!(head.seq, receipt.seq, "the ratification is the head");
    let event = store
        .events_range(t, receipt.seq, 1)
        .await
        .expect("range")
        .pop()
        .expect("the ratification's event");
    assert_eq!(
        event.envelope.event_type,
        event_types::WORKAREA_STATUS_TRANSITION
    );
    assert_eq!(
        store
            .workarea(t, node)
            .await
            .expect("read")
            .map(|w| w.status),
        Some(WorkareaStatus::Committed),
        "the node stands on the row's to"
    );
    assert_eq!(
        store
            .transition(t, proposal.transition_id)
            .await
            .expect("read"),
        Some(TransitionRecord {
            state: TransitionState::Ratified,
            ratifier_key_id: Some(ratifier),
            ..proposal.clone()
        }),
        "the row is ratified by its ratifier"
    );
    assert_eq!(
        store.pending_transitions(t, node).await.expect("pending"),
        vec![overtaken],
        "the overtaken proposal stays pending as history; the ratified one is no longer listed"
    );
}

/// Every ratification whose guard matches no row is refused at its
/// commit: one whose `from` is no longer the node's status and one already
/// ratified as [`StoreError::TransitionStale`], one by the proposing key as
/// [`StoreError::RatifierIsProposer`], and one naming another node, another
/// `to`, no proposal at all or a key the tenant does not hold as a defect.
/// None journals its event, moves the node, or changes the row (ADR-0009
/// d3; ADR-0010 R2; workareas.md, ratification re-checks).
pub async fn a_ratification_fold_whose_status_guard_fails_journals_nothing_and_moves_nothing<
    S: JournalStore,
>(
    store: &S,
) {
    let node = Uuid::from_u128(0xB510);
    let (proposer, ratifier) = (Uuid::from_u128(0xD510), Uuid::from_u128(0xD511));
    let mut rig = transition_ground(store, 51, node, (proposer, ratifier)).await;
    let t = rig.tenant_id;
    let sibling = Uuid::from_u128(0xB511);
    rig.create_workarea(
        store,
        workarea_record(t, sibling, Some(Uuid::from_u128(0xB610)), "area-51-sibling"),
    )
    .await;
    let ratified = pending_transition(
        t,
        Uuid::from_u128(0xE510),
        node,
        WorkareaStatus::Development,
        proposer,
    );
    let stale = pending_transition(
        t,
        Uuid::from_u128(0xE511),
        node,
        WorkareaStatus::Development,
        proposer,
    );
    let live = pending_transition(
        t,
        Uuid::from_u128(0xE512),
        sibling,
        WorkareaStatus::Development,
        proposer,
    );
    for p in [&ratified, &stale, &live] {
        rig.propose_transition(store, p.clone())
            .await
            .expect("proposal");
    }
    rig.ratify_transition(
        store,
        ratified.transition_id,
        node,
        WorkareaStatus::Committed,
        ratifier,
    )
    .await
    .expect("the first ratification");
    let head = store.chain_head(t).await.expect("head");

    let stale_as = |row: &TransitionRecord| {
        Some(StoreError::TransitionStale {
            transition_id: row.transition_id,
            workarea_id: row.workarea_id,
            from: row.from,
            to: row.to,
        })
    };
    // `None` expects the defect path, `StoreError::Backend`.
    let cases = [
        (
            "a proposal whose from is no longer the node's status",
            stale.transition_id,
            node,
            WorkareaStatus::Committed,
            ratifier,
            stale_as(&stale),
        ),
        (
            "a proposal already ratified",
            ratified.transition_id,
            node,
            WorkareaStatus::Committed,
            ratifier,
            stale_as(&ratified),
        ),
        (
            "a ratification naming another node",
            live.transition_id,
            node,
            WorkareaStatus::Committed,
            ratifier,
            None,
        ),
        (
            "a ratification naming another to",
            live.transition_id,
            sibling,
            WorkareaStatus::Validated,
            ratifier,
            None,
        ),
        (
            "a ratification naming no proposal",
            Uuid::from_u128(0xE51F),
            sibling,
            WorkareaStatus::Committed,
            ratifier,
            None,
        ),
        (
            "a ratification by the proposing key",
            live.transition_id,
            sibling,
            WorkareaStatus::Committed,
            proposer,
            Some(StoreError::RatifierIsProposer {
                transition_id: live.transition_id,
            }),
        ),
        (
            "a ratification by a key the tenant does not hold",
            live.transition_id,
            sibling,
            WorkareaStatus::Committed,
            Uuid::from_u128(0xD51F),
            None,
        ),
    ];
    for (case, transition_id, workarea_id, to, by, expected) in cases {
        let refused = rig
            .ratify_transition(store, transition_id, workarea_id, to, by)
            .await;
        match expected {
            Some(expected) => assert_eq!(refused, Err(expected), "{case} is refused by name"),
            None => assert!(
                matches!(refused, Err(StoreError::Backend(_))),
                "{case} is refused as a defect, got {refused:?}"
            ),
        }
        assert_eq!(
            store.chain_head(t).await.expect("head"),
            head,
            "{case} journals nothing"
        );
    }
    let statuses = [
        (node, WorkareaStatus::Committed),
        (sibling, WorkareaStatus::Development),
    ];
    for (id, status) in statuses {
        assert_eq!(
            store.workarea(t, id).await.expect("read").map(|w| w.status),
            Some(status),
            "no refused ratification moved {id}"
        );
    }
    for (row, state, by) in [
        (&ratified, TransitionState::Ratified, Some(ratifier)),
        (&stale, TransitionState::Pending, None),
        (&live, TransitionState::Pending, None),
    ] {
        assert_eq!(
            store.transition(t, row.transition_id).await.expect("read"),
            Some(TransitionRecord {
                state,
                ratifier_key_id: by,
                ..row.clone()
            }),
            "no refused ratification changed row {}",
            row.transition_id
        );
    }
}

/// Propose and ratify one rung of `node` from `from` in two commits, by
/// two keys, for the laws that need a node already moved.
async fn step_once<S: JournalStore>(
    rig: &mut ChainRig,
    store: &S,
    transition_id: Uuid,
    node: Uuid,
    from: WorkareaStatus,
    (proposer, ratifier): (Uuid, Uuid),
) {
    let proposal = pending_transition(rig.tenant_id, transition_id, node, from, proposer);
    let to = proposal.to;
    rig.propose_transition(store, proposal)
        .await
        .expect("proposal");
    rig.ratify_transition(store, transition_id, node, to, ratifier)
        .await
        .expect("ratification");
}

/// A step to `validated` is re-checked against the node's children inside
/// the ratifying commit: a child created after the proposal and still
/// short of `validated` refuses the ratification as
/// [`StoreError::ChildrenNotValidated`], journalling nothing, moving
/// nothing and leaving the row pending; once that child is `validated` the
/// same proposal ratifies (ADR-0009 d4; workareas.md, ratification
/// re-checks).
pub async fn a_child_created_between_proposal_and_ratification_refuses_the_step_to_validated<
    S: JournalStore,
>(
    store: &S,
) {
    let node = Uuid::from_u128(0xB530);
    let keys = (Uuid::from_u128(0xD530), Uuid::from_u128(0xD531));
    let mut rig = transition_ground(store, 53, node, keys).await;
    let t = rig.tenant_id;
    step_once(
        &mut rig,
        store,
        Uuid::from_u128(0xE530),
        node,
        WorkareaStatus::Development,
        keys,
    )
    .await;
    let proposal = pending_transition(
        t,
        Uuid::from_u128(0xE531),
        node,
        WorkareaStatus::Committed,
        keys.0,
    );
    rig.propose_transition(store, proposal.clone())
        .await
        .expect("a step to validated with no child is proposed");

    let child = Uuid::from_u128(0xB532);
    rig.create_workarea(
        store,
        workarea_record(t, child, Some(node), "area-53-child"),
    )
    .await;
    let head = store.chain_head(t).await.expect("head");
    let refused = rig
        .ratify_transition(
            store,
            proposal.transition_id,
            node,
            WorkareaStatus::Validated,
            keys.1,
        )
        .await;
    assert_eq!(
        refused,
        Err(StoreError::ChildrenNotValidated {
            transition_id: proposal.transition_id,
            workarea_id: node,
        }),
        "a child created after the proposal refuses the step by name"
    );
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "the refused ratification journals nothing"
    );
    assert_eq!(
        store
            .workarea(t, node)
            .await
            .expect("read")
            .map(|w| w.status),
        Some(WorkareaStatus::Committed),
        "the refused ratification moves nothing"
    );
    assert_eq!(
        store
            .transition(t, proposal.transition_id)
            .await
            .expect("read"),
        Some(proposal.clone()),
        "the refused ratification leaves the row pending"
    );

    step_once(
        &mut rig,
        store,
        Uuid::from_u128(0xE532),
        child,
        WorkareaStatus::Development,
        keys,
    )
    .await;
    step_once(
        &mut rig,
        store,
        Uuid::from_u128(0xE533),
        child,
        WorkareaStatus::Committed,
        keys,
    )
    .await;
    rig.ratify_transition(
        store,
        proposal.transition_id,
        node,
        WorkareaStatus::Validated,
        keys.1,
    )
    .await
    .expect("with every child validated the same proposal ratifies");
    assert_eq!(
        store
            .workarea(t, node)
            .await
            .expect("read")
            .map(|w| w.status),
        Some(WorkareaStatus::Validated)
    );
}

/// R2 holds on the principal, not only the key: a ratification by another
/// key of the proposer's principal is refused in the ratifying commit as
/// [`StoreError::RatifierIsProposer`], journalling nothing and moving
/// nothing (ADR-0010 R2; errors.md `review.ratifier_is_proposer`).
pub async fn a_ratification_by_another_key_of_the_proposers_principal_is_refused<
    S: JournalStore,
>(
    store: &S,
) {
    let node = Uuid::from_u128(0xB540);
    let (proposer, ratifier) = (Uuid::from_u128(0xD540), Uuid::from_u128(0xD541));
    let mut rig = transition_ground(store, 54, node, (proposer, ratifier)).await;
    let t = rig.tenant_id;
    let twin = Uuid::from_u128(0xD542);
    rig.issue_key(
        store,
        KeyRecord {
            principal: key_record(t, proposer, Role::Admin, None).principal,
            ..key_record(t, twin, Role::Admin, None)
        },
    )
    .await;
    let proposal = pending_transition(
        t,
        Uuid::from_u128(0xE540),
        node,
        WorkareaStatus::Development,
        proposer,
    );
    rig.propose_transition(store, proposal.clone())
        .await
        .expect("proposal");
    let head = store.chain_head(t).await.expect("head");

    let refused = rig
        .ratify_transition(
            store,
            proposal.transition_id,
            node,
            WorkareaStatus::Committed,
            twin,
        )
        .await;
    assert_eq!(
        refused,
        Err(StoreError::RatifierIsProposer {
            transition_id: proposal.transition_id,
        }),
        "the proposer's principal under another key is the proposer"
    );
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "the refused ratification journals nothing"
    );
    assert_eq!(
        store
            .workarea(t, node)
            .await
            .expect("read")
            .map(|w| w.status),
        Some(WorkareaStatus::Development),
        "the refused ratification moves nothing"
    );
    assert_eq!(
        store
            .transition(t, proposal.transition_id)
            .await
            .expect("read"),
        Some(proposal),
        "the refused ratification leaves the row pending"
    );
}

/// A proposal carries its proposing key's own principal, the one the key
/// row holds, or the store refuses it as a defect: a principal the key
/// does not hold would defeat the ratifying commit's R2 check, so no row
/// is written and nothing is journalled (ADR-0010 R2; ADR-0009 d3).
pub async fn a_proposal_whose_principal_is_not_its_proposing_keys_is_refused<S: JournalStore>(
    store: &S,
) {
    let node = Uuid::from_u128(0xB550);
    let (proposer, ratifier) = (Uuid::from_u128(0xD550), Uuid::from_u128(0xD551));
    let mut rig = transition_ground(store, 55, node, (proposer, ratifier)).await;
    let t = rig.tenant_id;
    let proposal = TransitionRecord {
        proposer_principal: key_record(t, ratifier, Role::Admin, None).principal,
        ..pending_transition(
            t,
            Uuid::from_u128(0xE550),
            node,
            WorkareaStatus::Development,
            proposer,
        )
    };
    let head = store.chain_head(t).await.expect("head");
    let refused = rig.propose_transition(store, proposal.clone()).await;
    assert!(
        matches!(refused, Err(StoreError::Backend(_))),
        "a principal the proposing key does not hold is refused, got {refused:?}"
    );
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "the refused proposal journals nothing"
    );
    assert_eq!(
        store
            .transition(t, proposal.transition_id)
            .await
            .expect("read"),
        None,
        "the refused proposal writes no row"
    );
}

/// A crew given at create is stored with the node and read back whole by
/// every workarea read; a node created without one reads back without one
/// (event-catalog.md `workarea.create`; postgres-schema.md `workareas.crew`).
pub async fn crew_round_trips_through_workarea_created<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(17);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let universe = workarea_record(t, Uuid::from_u128(0xB79), None, "universe");
    let metals = WorkareaRecord {
        crew: Some(Crew {
            name: Some("metals-crew".to_owned()),
            charter_uri: Some("https://crews.example.com/metals/charter.md".to_owned()),
            external_manager: Some("crewd.example.com".to_owned()),
        }),
        ..workarea_record(
            t,
            Uuid::from_u128(0xB71),
            Some(universe.workarea_id),
            "metals-crew",
        )
    };
    let named_only = WorkareaRecord {
        crew: Some(Crew {
            name: Some("redline".to_owned()),
            charter_uri: None,
            external_manager: None,
        }),
        ..workarea_record(
            t,
            Uuid::from_u128(0xB72),
            Some(universe.workarea_id),
            "redline",
        )
    };
    for w in [&universe, &metals, &named_only] {
        rig.create_workarea(store, w.clone()).await;
    }

    for w in [&universe, &metals, &named_only] {
        assert_eq!(
            store
                .workarea(t, w.workarea_id)
                .await
                .expect("read")
                .as_ref(),
            Some(w),
            "the crew reads back as it was created"
        );
    }
    assert_eq!(
        store
            .workarea_children(t, universe.workarea_id)
            .await
            .expect("children"),
        vec![metals.clone(), named_only.clone()],
        "the children carry their crews"
    );
    assert_eq!(
        store
            .workarea_ancestry(t, metals.workarea_id)
            .await
            .expect("walk"),
        Some(vec![metals, universe]),
        "the ancestry carries the crews"
    );
}

/// Every claim field a capture stores — kind, version, the NFC byte count,
/// the retirement mark and the replaced version — reads back as committed
/// (postgres-schema.md `memories`; ADR-0025 d6, d10, d11).
pub async fn kind_version_and_body_bytes_round_trip_through_memory_held<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(20);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB20);
    let first = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC20),
        "wanda",
        Uuid::from_u128(0xD20),
        "drain the canary",
    );
    let successor = MemoryRecord {
        memory_id: Uuid::from_u128(0xC21),
        claim_key: first.claim_key.clone(),
        version: 7,
        kind: MemoryKind::Steering,
        body: "retire: the canary is gone".to_owned(),
        body_bytes: 4_000_000,
        retirement: true,
        replaces: Some(first.memory_id),
        derives_from: vec![first.memory_id],
        scan_finding: Some("marker finding".to_owned()),
        ..first.clone()
    };
    for memory in [first, successor] {
        hold(&mut rig, store, memory.clone())
            .await
            .expect("commit held");
        assert_eq!(
            store
                .memory(rig.tenant_id, memory.memory_id)
                .await
                .expect("memory"),
            Some(memory),
            "every stored field reads back"
        );
    }
}

/// A promotion makes the version its claim's tip, stamps `promoted_at` with
/// the promoting event's `at`, and takes the tip off the version it
/// replaces, which stays promoted (ADR-0025 d10).
pub async fn promotion_makes_the_version_the_tip_and_stamps_promoted_at_from_its_event<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(21);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB21);
    let v1 = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC22),
        "wanda",
        Uuid::from_u128(0xD21),
        "v1",
    );
    let v2 = MemoryRecord {
        memory_id: Uuid::from_u128(0xC23),
        version: 2,
        replaces: Some(v1.memory_id),
        body: "v2".to_owned(),
        ..v1.clone()
    };
    hold(&mut rig, store, v1.clone()).await.expect("hold v1");
    let read = |id| async move {
        store
            .memory(rig_tenant(21), id)
            .await
            .expect("memory")
            .expect("the row exists")
    };
    let held = read(v1.memory_id).await;
    assert!(!held.is_tip, "a held version is never the tip");
    assert_eq!(held.promoted_at, None);

    let receipt = promote(&mut rig, store, v1.memory_id)
        .await
        .expect("promote v1");
    let at = store
        .events_range(rig.tenant_id, receipt.seq, 1)
        .await
        .expect("range")
        .remove(0)
        .envelope
        .at;
    let tip = read(v1.memory_id).await;
    assert_eq!(tip.status, MemoryStatus::Promoted);
    assert!(tip.is_tip);
    assert_eq!(tip.promoted_at, Some(at), "the promoting event's at");

    hold(&mut rig, store, v2.clone()).await.expect("hold v2");
    assert!(
        read(v1.memory_id).await.is_tip,
        "a held successor moves nothing"
    );
    let receipt = supersede_tip(&mut rig, store, v2.memory_id, v1.memory_id)
        .await
        .expect("ratify v2");
    let at = store
        .events_range(rig.tenant_id, receipt.seq, 1)
        .await
        .expect("range")
        .remove(0)
        .envelope
        .at;
    let replaced = read(v1.memory_id).await;
    assert_eq!(
        replaced.status,
        MemoryStatus::Promoted,
        "no superseded state"
    );
    assert!(!replaced.is_tip, "the replaced version is history");
    let tip = read(v2.memory_id).await;
    assert!(tip.is_tip);
    assert_eq!(tip.status, MemoryStatus::Promoted);
    assert_eq!(tip.promoted_at, Some(at), "the ratifying event's at");
    assert_eq!(
        store
            .claim_tip(rig.tenant_id, wa, &v1.claim_key)
            .await
            .expect("claim_tip")
            .map(|m| m.memory_id),
        Some(v2.memory_id),
        "the claim's one served tip"
    );
}

/// The compare-and-set of ratification: a successor takes the tip only
/// while the version it replaces is still the promoted tip under the
/// successor's own workarea and claim_key; a lost compare-and-set commits
/// nothing, journal row included, and leaves the successor held
/// (audit-and-lineage.md supersession chains; ADR-0025 d10).
pub async fn tip_superseded_refuses_when_the_target_is_no_longer_tip<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(25);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB27);
    let v1 = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC2B),
        "wanda",
        Uuid::from_u128(0xD25),
        "v1",
    );
    let successor = |id: u128, version: u32| MemoryRecord {
        memory_id: Uuid::from_u128(id),
        version,
        replaces: Some(v1.memory_id),
        body: format!("v{version}"),
        ..v1.clone()
    };
    let (first, rival) = (successor(0xC2C, 2), successor(0xC2D, 3));
    let rekeyed = MemoryRecord {
        claim_key: "another-claim".to_owned(),
        ..successor(0xC2E, 1)
    };
    hold(&mut rig, store, v1.clone()).await.expect("hold v1");
    promote(&mut rig, store, v1.memory_id)
        .await
        .expect("promote v1");
    for m in [&first, &rival, &rekeyed] {
        hold(&mut rig, store, m.clone()).await.expect("hold");
    }
    let tenant = rig.tenant_id;
    let read = |id| async move {
        store
            .memory(tenant, id)
            .await
            .expect("memory")
            .expect("the row exists")
    };
    let mut held = store
        .held_successors(tenant, v1.memory_id)
        .await
        .expect("held_successors")
        .into_iter()
        .map(|m| m.memory_id)
        .collect::<Vec<_>>();
    held.sort();
    let mut expected = vec![first.memory_id, rival.memory_id, rekeyed.memory_id];
    expected.sort();
    assert_eq!(held, expected, "every held successor of the tip");

    // A successor re-keyed away from its target fails the compare-and-set.
    let head = store.chain_head(tenant).await.expect("head");
    assert!(matches!(
        supersede_tip(&mut rig, store, rekeyed.memory_id, v1.memory_id).await,
        Err(StoreError::TipMoved { .. })
    ));
    assert_eq!(store.chain_head(tenant).await.expect("head"), head);
    assert!(read(v1.memory_id).await.is_tip);

    // The first ratified takes the tip; its rival then commits nothing.
    supersede_tip(&mut rig, store, first.memory_id, v1.memory_id)
        .await
        .expect("ratify the first successor");
    let head = store.chain_head(tenant).await.expect("head");
    assert!(matches!(
        supersede_tip(&mut rig, store, rival.memory_id, v1.memory_id).await,
        Err(StoreError::TipMoved { .. })
    ));
    assert_eq!(
        store.chain_head(tenant).await.expect("head"),
        head,
        "a lost compare-and-set journals nothing"
    );
    let stale = read(rival.memory_id).await;
    assert_eq!(stale.status, MemoryStatus::Held, "it stays held");
    assert!(!stale.is_tip);
    assert!(read(first.memory_id).await.is_tip);
    assert!(!read(v1.memory_id).await.is_tip);
    assert_eq!(
        store
            .held_successors(tenant, v1.memory_id)
            .await
            .expect("held_successors")
            .len(),
        2,
        "the ratified successor is no longer held"
    );
}

/// An approving verdict and the ratification it triggers commit as one
/// chain, each link extending the one before; a chain whose ratification
/// loses the compare-and-set writes neither event, nor does a chain whose
/// second link does not extend the first (ADR-0002 d1; ADR-0025 d10).
pub async fn a_chain_whose_ratification_loses_the_compare_and_set_journals_no_verdict<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(60);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let v1 = held_memory(
        tenant,
        Uuid::from_u128(0xB60),
        Uuid::from_u128(0xC60),
        "wanda",
        Uuid::from_u128(0xD60),
        "v1",
    );
    let successor = |id: u128, version: u32| MemoryRecord {
        memory_id: Uuid::from_u128(id),
        version,
        replaces: Some(v1.memory_id),
        body: format!("v{version}"),
        ..v1.clone()
    };
    let (first, rival) = (successor(0xC61, 2), successor(0xC62, 3));
    hold(&mut rig, store, v1.clone()).await.expect("hold v1");
    promote(&mut rig, store, v1.memory_id)
        .await
        .expect("promote v1");
    for m in [&first, &rival] {
        hold(&mut rig, store, m.clone()).await.expect("hold");
    }
    let ratify = |m: &MemoryRecord| {
        vec![
            (
                event_types::REVIEW_APPROVE,
                json!({ "artifact_id": m.memory_id.to_string() }),
                MetadataFold::VerdictRecorded {
                    memory_id: m.memory_id,
                    reviewer_key_id: Uuid::from_u128(0xD61),
                },
            ),
            (
                event_types::LIFECYCLE_SUPERSEDE,
                json!({ "new_artifact_id": m.memory_id.to_string() }),
                MetadataFold::TipSuperseded {
                    successor: m.memory_id,
                    replaced: v1.memory_id,
                    replaced_valid_until: None,
                },
            ),
        ]
    };
    let types_after = |seq: u64| async move {
        store
            .events_range(tenant, seq + 1, 10)
            .await
            .expect("range")
            .into_iter()
            .map(|e| e.envelope.event_type)
            .collect::<Vec<_>>()
    };

    let before = store
        .chain_head(tenant)
        .await
        .expect("head")
        .expect("a head");
    let receipt = rig
        .commit_chain(store, ratify(&first))
        .await
        .expect("the first ratified takes the tip");
    assert_eq!(
        receipt.seq,
        before.seq + 2,
        "the receipt names the last link"
    );
    assert_eq!(
        types_after(before.seq).await,
        vec![
            event_types::REVIEW_APPROVE,
            event_types::LIFECYCLE_SUPERSEDE
        ]
    );
    let head = store.chain_head(tenant).await.expect("head");
    assert_eq!(
        head.as_ref().map(|h| (h.seq, h.hash)),
        Some((receipt.seq, receipt.hash))
    );
    let read = |id| async move {
        store
            .memory(tenant, id)
            .await
            .expect("memory")
            .expect("the row exists")
    };
    assert!(read(first.memory_id).await.is_tip);

    // The rival's verdict lands with its ratification or not at all.
    assert!(matches!(
        rig.commit_chain(store, ratify(&rival)).await,
        Err(StoreError::TipMoved { .. })
    ));
    assert_eq!(store.chain_head(tenant).await.expect("head"), head);
    assert!(
        types_after(receipt.seq).await.is_empty(),
        "a lost compare-and-set journals no verdict"
    );
    assert_eq!(read(rival.memory_id).await.status, MemoryStatus::Held);

    // A second link that does not extend the first refuses the first too.
    let filler_fold = rig.genesis_fold();
    let request = |event: SignedEnvelope| CommitRequest {
        outbox: OutboxRow {
            event_id: event.envelope.event_id,
            topic: event.envelope.event_type.clone(),
            payload: json!({ "seq": event.envelope.seq }),
        },
        event,
        fold: filler_fold.clone(),
        idempotency: None,
    };
    let filler = || json!({ "body_digest": "sha256:00" });
    let (a, b) = (
        rig.next_event(event_types::SUBMISSION_REMEMBER, filler()),
        rig.next_event(event_types::SUBMISSION_REMEMBER, filler()),
    );
    assert!(matches!(
        store.commit_chain(vec![request(a), request(b)]).await,
        Err(StoreError::SeqConflict { .. })
    ));
    assert_eq!(store.chain_head(tenant).await.expect("head"), head);
    assert!(
        store.commit_chain(Vec::new()).await.is_err(),
        "an empty chain is refused"
    );
}

/// The tenant a rig built with `seed` writes under.
fn rig_tenant(seed: u64) -> Uuid {
    ChainRig::new(seed).tenant_id
}

/// One claim has one served tip: promoting a second version that replaces
/// nothing while the first is the tip is refused, and the refused commit
/// changes nothing (postgres-schema.md `memories_served_tip`).
pub async fn a_second_served_tip_of_one_claim_commits_nothing<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(22);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB22);
    let first = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC24),
        "wanda",
        Uuid::from_u128(0xD22),
        "first",
    );
    let rival = MemoryRecord {
        memory_id: Uuid::from_u128(0xC25),
        version: 2,
        body: "rival".to_owned(),
        ..first.clone()
    };
    hold(&mut rig, store, first.clone()).await.expect("hold");
    promote(&mut rig, store, first.memory_id)
        .await
        .expect("promote");
    hold(&mut rig, store, rival.clone())
        .await
        .expect("hold rival");
    let head = store.chain_head(rig.tenant_id).await.expect("head");

    assert!(
        promote(&mut rig, store, rival.memory_id).await.is_err(),
        "a second tip of one claim is refused"
    );
    assert_eq!(store.chain_head(rig.tenant_id).await.expect("head"), head);
    let still = store
        .memory(rig.tenant_id, rival.memory_id)
        .await
        .expect("memory")
        .expect("row");
    assert_eq!(still.status, MemoryStatus::Held);
    assert!(!still.is_tip);
    assert!(
        store
            .memory(rig.tenant_id, first.memory_id)
            .await
            .expect("memory")
            .expect("row")
            .is_tip
    );
}

/// No two held, promoted or demoted versions of one `claim_key` in one
/// workarea share an ordinal; a rejected version leaves its ordinal free
/// (postgres-schema.md `memories_claim_version`; ADR-0025 d18).
pub async fn a_second_live_version_with_one_ordinal_commits_nothing<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(23);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB23);
    let first = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC26),
        "wanda",
        Uuid::from_u128(0xD23),
        "first",
    );
    let twin = MemoryRecord {
        memory_id: Uuid::from_u128(0xC27),
        body: "twin".to_owned(),
        ..first.clone()
    };
    hold(&mut rig, store, first.clone()).await.expect("hold");
    let head = store.chain_head(rig.tenant_id).await.expect("head");
    assert!(
        hold(&mut rig, store, twin.clone()).await.is_err(),
        "a second live version 1 is refused"
    );
    assert_eq!(store.chain_head(rig.tenant_id).await.expect("head"), head);
    assert_eq!(
        store
            .memory(rig.tenant_id, twin.memory_id)
            .await
            .expect("memory"),
        None,
        "the refused commit stored no row"
    );

    // Elsewhere the ordinal is free: another workarea, another claim_key.
    let elsewhere = MemoryRecord {
        memory_id: Uuid::from_u128(0xC28),
        workarea_id: Uuid::from_u128(0xB24),
        ..twin.clone()
    };
    hold(&mut rig, store, elsewhere)
        .await
        .expect("another workarea's claim_key is another claim");

    // A rejected version 1 releases its ordinal.
    reject(&mut rig, store, first.memory_id)
        .await
        .expect("reject");
    hold(&mut rig, store, twin)
        .await
        .expect("a fresh version 1 after the rejection");
}

/// A capture reads the greatest version of a `claim_key` in any state, and
/// whether a held, promoted or demoted version holds it, within one
/// workarea (audit-and-lineage.md supersession chains; ADR-0025 d10, d18).
pub async fn claim_key_state_counts_every_version_and_only_live_ones_hold_the_key<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(24);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB25);
    let v1 = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC29),
        "wanda",
        Uuid::from_u128(0xD24),
        "v1",
    );
    let key = v1.claim_key.clone();
    let state = |workarea| {
        let key = key.clone();
        async move {
            store
                .claim_key_state(rig_tenant(24), workarea, &key)
                .await
                .expect("claim_key_state")
        }
    };
    assert_eq!(state(wa).await, ClaimKeyState::default(), "an unused key");

    hold(&mut rig, store, v1.clone()).await.expect("hold v1");
    assert_eq!(
        state(wa).await,
        ClaimKeyState {
            greatest_version: Some(1),
            live: true,
        },
        "a held version holds the key"
    );
    assert_eq!(
        state(Uuid::from_u128(0xB26)).await,
        ClaimKeyState::default(),
        "claim_key is namespaced by workarea"
    );

    reject(&mut rig, store, v1.memory_id)
        .await
        .expect("reject v1");
    assert_eq!(
        state(wa).await,
        ClaimKeyState {
            greatest_version: Some(1),
            live: false,
        },
        "a rejected version is counted and releases the key"
    );

    let v3 = MemoryRecord {
        memory_id: Uuid::from_u128(0xC2A),
        version: 3,
        body: "v3".to_owned(),
        ..v1.clone()
    };
    hold(&mut rig, store, v3.clone()).await.expect("hold v3");
    promote(&mut rig, store, v3.memory_id)
        .await
        .expect("promote v3");
    assert_eq!(
        state(wa).await,
        ClaimKeyState {
            greatest_version: Some(3),
            live: true,
        },
        "a promoted version holds the key"
    );
}

/// The ids of a listing page, in page order.
fn page_ids(page: &[MemoryRecord]) -> Vec<Uuid> {
    page.iter().map(|m| m.memory_id).collect()
}

/// A listing filter that narrows nothing beyond its scope.
fn listing(scope: ListingScope) -> ListingFilter {
    ListingFilter {
        anchor: oml_app::types::AnchorFilter::default(),
        scope,
        subtree: None,
        state: None,
        kind: None,
        claim_key: None,
        since: None,
        until: None,
    }
}

/// A held fixture row of the listing laws, written at second `second`.
fn listed_row(seed: u64, n: u128, workarea: Uuid, author_key_id: Uuid, second: u8) -> MemoryRecord {
    MemoryRecord {
        created_at: EventTime::parse(&format!("2026-10-29T13:00:{second:02}.000000Z"))
            .expect("fixture time"),
        ..held_memory(
            rig_tenant(seed),
            workarea,
            Uuid::from_u128(n),
            "wanda",
            author_key_id,
            "a claim",
        )
    }
}

/// The own-work scope lists the key set's rows in every state and no other
/// key's, minus every row on a masked node; the subtree, state, kind and
/// claim_key predicates narrow it (roles-and-keys.md#own; ADR-0025 d13).
pub async fn an_own_listing_holds_every_state_of_the_key_sets_rows_off_masked_nodes<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(31);
    rig.seed_chain(store, 0).await;
    let (wa, denied) = (Uuid::from_u128(0xB31), Uuid::from_u128(0xB32));
    let (author, other) = (Uuid::from_u128(0xD31), Uuid::from_u128(0xD32));
    let held = listed_row(31, 0xC31, wa, author, 1);
    let promoted = listed_row(31, 0xC32, wa, author, 2);
    let rejected = MemoryRecord {
        kind: MemoryKind::Steering,
        ..listed_row(31, 0xC33, wa, author, 3)
    };
    let masked = listed_row(31, 0xC34, denied, author, 4);
    let foreign = listed_row(31, 0xC35, wa, other, 5);
    for m in [&held, &promoted, &rejected, &masked, &foreign] {
        hold(&mut rig, store, m.clone()).await.expect("hold");
    }
    promote(&mut rig, store, promoted.memory_id)
        .await
        .expect("promote");
    reject(&mut rig, store, rejected.memory_id)
        .await
        .expect("reject");
    let list = |filter: ListingFilter| async move {
        page_ids(
            &store
                .list_memories(rig_tenant(31), &filter, None, 50)
                .await
                .expect("list"),
        )
    };
    let own = |masked: Vec<Uuid>| {
        listing(ListingScope::Own {
            key_ids: vec![author],
            masked,
        })
    };

    assert_eq!(
        list(own(Vec::new())).await,
        vec![
            masked.memory_id,
            rejected.memory_id,
            promoted.memory_id,
            held.memory_id
        ],
        "every state of the key's own rows, newest first, and no other key's"
    );
    assert_eq!(
        list(own(vec![denied])).await,
        vec![rejected.memory_id, promoted.memory_id, held.memory_id],
        "a masked node's rows are absent"
    );
    assert_eq!(
        list(listing(ListingScope::Own {
            key_ids: vec![other],
            masked: Vec::new(),
        }))
        .await,
        vec![foreign.memory_id]
    );
    assert!(
        list(listing(ListingScope::Own {
            key_ids: Vec::new(),
            masked: Vec::new(),
        }))
        .await
        .is_empty(),
        "an empty key set lists nothing"
    );
    assert_eq!(
        list(ListingFilter {
            anchor: oml_app::types::AnchorFilter::default(),
            subtree: Some(vec![denied]),
            ..own(Vec::new())
        })
        .await,
        vec![masked.memory_id],
        "the subtree narrows"
    );
    assert_eq!(
        list(ListingFilter {
            anchor: oml_app::types::AnchorFilter::default(),
            state: Some(MemoryStatus::Held),
            ..own(vec![denied])
        })
        .await,
        vec![held.memory_id],
        "the state narrows"
    );
    assert_eq!(
        list(ListingFilter {
            anchor: oml_app::types::AnchorFilter::default(),
            kind: Some(MemoryKind::Steering),
            ..own(Vec::new())
        })
        .await,
        vec![rejected.memory_id],
        "the kind narrows"
    );
    assert_eq!(
        list(ListingFilter {
            anchor: oml_app::types::AnchorFilter::default(),
            claim_key: Some(promoted.claim_key.clone()),
            ..own(Vec::new())
        })
        .await,
        vec![promoted.memory_id],
        "the claim_key narrows"
    );
}

/// The tips scope lists the promoted served tips on exactly its nodes:
/// never a held, rejected or replaced version, never a row off the set, and
/// nothing for an empty set (R3; ADR-0025 d1, d13).
pub async fn a_tips_listing_holds_only_the_promoted_tips_on_its_nodes<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(32);
    rig.seed_chain(store, 0).await;
    let (wa, elsewhere) = (Uuid::from_u128(0xB33), Uuid::from_u128(0xB34));
    let author = Uuid::from_u128(0xD33);
    let replaced = listed_row(32, 0xC36, wa, author, 1);
    let tip = MemoryRecord {
        version: 2,
        replaces: Some(replaced.memory_id),
        claim_key: replaced.claim_key.clone(),
        ..listed_row(32, 0xC37, wa, author, 2)
    };
    let held = listed_row(32, 0xC38, wa, author, 3);
    let rejected = listed_row(32, 0xC39, wa, author, 4);
    let off_set = listed_row(32, 0xC3A, elsewhere, author, 5);
    for m in [&replaced, &tip, &held, &rejected, &off_set] {
        hold(&mut rig, store, m.clone()).await.expect("hold");
    }
    for id in [replaced.memory_id, off_set.memory_id] {
        promote(&mut rig, store, id).await.expect("promote");
    }
    // The successor takes the tip at its ratification (ADR-0025 d10).
    supersede_tip(&mut rig, store, tip.memory_id, replaced.memory_id)
        .await
        .expect("ratify");
    reject(&mut rig, store, rejected.memory_id)
        .await
        .expect("reject");
    let list = |filter: ListingFilter| async move {
        page_ids(
            &store
                .list_memories(rig_tenant(32), &filter, None, 50)
                .await
                .expect("list"),
        )
    };
    let tips = |nodes: Vec<Uuid>| {
        listing(ListingScope::Tips {
            nodes,
            held: Vec::new(),
        })
    };

    assert_eq!(list(tips(vec![wa])).await, vec![tip.memory_id]);
    assert_eq!(
        list(tips(vec![elsewhere, wa])).await,
        vec![off_set.memory_id, tip.memory_id],
        "every node of the set is listed"
    );
    assert!(
        list(tips(Vec::new())).await.is_empty(),
        "an empty set lists nothing, never the tenant"
    );
    assert!(
        list(ListingFilter {
            anchor: oml_app::types::AnchorFilter::default(),
            state: Some(MemoryStatus::Held),
            ..tips(vec![wa])
        })
        .await
        .is_empty(),
        "no filter reaches a held row through the tips scope"
    );
}

/// A listing orders `created_at` descending, then `artifact_id`
/// descending; a page resumes strictly after its position, `limit` bounds
/// it, and `since` (inclusive) and `until` (exclusive) bound `created_at`
/// (http-api.md search and pagination).
pub async fn a_listing_pages_newest_first_and_resumes_strictly_after_its_position<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(33);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB35);
    let author = Uuid::from_u128(0xD34);
    // Two rows share an instant: the greater id comes first.
    for (n, second) in [(0xC3B, 1), (0xC3C, 2), (0xC3E, 2), (0xC3D, 3)] {
        hold(&mut rig, store, listed_row(33, n, wa, author, second))
            .await
            .expect("hold");
    }
    let own = listing(ListingScope::Own {
        key_ids: vec![author],
        masked: Vec::new(),
    });
    let at = |second: u8| {
        EventTime::parse(&format!("2026-10-29T13:00:{second:02}.000000Z")).expect("fixture time")
    };
    let page = |filter: ListingFilter, after: Option<(u128, u8)>, limit: usize| async move {
        let after = after.map(|(id, second)| ListingPosition {
            created_at: at(second),
            artifact_id: Uuid::from_u128(id),
        });
        page_ids(
            &store
                .list_memories(rig_tenant(33), &filter, after.as_ref(), limit)
                .await
                .expect("list"),
        )
    };
    let order: Vec<Uuid> = [0xC3D, 0xC3E, 0xC3C, 0xC3B]
        .into_iter()
        .map(Uuid::from_u128)
        .collect();

    assert_eq!(page(own.clone(), None, 50).await, order);
    assert_eq!(page(own.clone(), None, 2).await, order[..2]);
    assert_eq!(
        page(own.clone(), Some((0xC3E, 2)), 50).await,
        order[2..],
        "strictly after the position, the tie broken by id"
    );
    assert!(page(own.clone(), Some((0xC3B, 1)), 50).await.is_empty());
    assert_eq!(
        page(
            ListingFilter {
                anchor: oml_app::types::AnchorFilter::default(),
                since: Some(at(2)),
                until: Some(at(3)),
                ..own.clone()
            },
            None,
            50
        )
        .await,
        order[1..3],
        "since is inclusive and until exclusive"
    );
    assert!(page(own, None, 0).await.is_empty(), "limit bounds the page");
}

// ---------------------------------------------------------------------------
// Names and crew records (ADR-0026)
// ---------------------------------------------------------------------------

/// What `resolve_name` answers for `value` of `kind` in `tenant_id`.
async fn resolved<S: JournalStore>(
    store: &S,
    tenant_id: Uuid,
    kind: NamedKind,
    value: &str,
) -> Option<Uuid> {
    store
        .resolve_name(tenant_id, kind, &name(value))
        .await
        .expect("resolve")
}

/// ADR-0044 d1, d2, d4, d5: a workarea's name is unique in its namespace —
/// the shared one, or the namespace of the user root it lies beneath — so
/// two user roots each hold a `project` beside a shared one; the shared
/// lookup reads the shared namespace alone and the user lookup one user
/// root's; a user root, a node's namespace, a key's user and a node grant's
/// coverage round-trip through their folds.
pub async fn workarea_names_are_unique_per_namespace_and_resolve_in_their_own<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(31);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let universe = workarea_record(t, Uuid::from_u128(0xB310), None, "universe");
    let root = |id: u128, user: &str| WorkareaRecord {
        user_root: true,
        ..workarea_record(t, Uuid::from_u128(id), Some(universe.workarea_id), user)
    };
    let (alice, bob) = (root(0xB311, "alice"), root(0xB312, "bob"));
    let shared = workarea_record(
        t,
        Uuid::from_u128(0xB313),
        Some(universe.workarea_id),
        "project",
    );
    let under = |id: u128, root: &WorkareaRecord| WorkareaRecord {
        namespace: Some(root.workarea_id),
        ..workarea_record(t, Uuid::from_u128(id), Some(root.workarea_id), "project")
    };
    let (alice_project, bob_project) = (under(0xB314, &alice), under(0xB315, &bob));
    for w in [
        universe.clone(),
        alice.clone(),
        bob.clone(),
        shared.clone(),
        alice_project.clone(),
        bob_project.clone(),
    ] {
        rig.create_workarea(store, w).await;
    }
    let head = store.chain_head(t).await.expect("head");
    let twin = WorkareaRecord {
        parent_id: Some(alice_project.workarea_id),
        ..under(0xB316, &alice)
    };
    assert_eq!(
        rig.commit(
            store,
            event_types::WORKAREA_CREATE,
            json!({ "workarea_id": twin.workarea_id.to_string(), "name": "project" }),
            MetadataFold::WorkareaCreated(twin.clone()),
            None,
        )
        .await,
        Err(StoreError::NameTaken {
            kind: NamedKind::Workarea,
            name: name("project"),
        }),
        "one name once in one user's namespace"
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(
        store
            .resolve_name(t, NamedKind::Workarea, &name("project"))
            .await
            .expect("resolve"),
        Some(shared.workarea_id),
        "the shared lookup reads the shared namespace alone"
    );
    assert_eq!(
        store
            .resolve_name(t, NamedKind::Workarea, &name("alice"))
            .await
            .expect("resolve"),
        Some(alice.workarea_id),
        "a user root's name is shared"
    );
    for (root, project) in [(&alice, &alice_project), (&bob, &bob_project)] {
        assert_eq!(
            store
                .resolve_user_workarea(t, root.workarea_id, &name("project"))
                .await
                .expect("resolve"),
            Some(project.workarea_id)
        );
    }
    assert_eq!(
        store
            .resolve_user_workarea(t, shared.workarea_id, &name("project"))
            .await
            .expect("resolve"),
        None,
        "a shared node's namespace holds nothing"
    );
    assert_eq!(
        store
            .workarea(t, alice.workarea_id)
            .await
            .expect("read")
            .as_ref(),
        Some(&alice)
    );
    assert_eq!(
        store
            .workarea(t, alice_project.workarea_id)
            .await
            .expect("read")
            .as_ref(),
        Some(&alice_project)
    );

    let key = KeyRecord {
        user: Some(alice.workarea_id),
        name: Some(name("alice-laptop")),
        ..key_record(t, Uuid::from_u128(0xD310), Role::Writer, None)
    };
    rig.issue_key(store, key.clone()).await;
    assert_eq!(
        store
            .key(t, key.key_id)
            .await
            .expect("read")
            .map(|k| k.user),
        Some(Some(alice.workarea_id))
    );
    let node = Grant {
        grant_id: Uuid::from_u128(0xF310),
        node: universe.workarea_id,
        role: Role::Reader,
        deny: Vec::new(),
        covers: Covers::Node,
    };
    rig.issue_grant(store, key.key_id, node.clone())
        .await
        .expect("a node grant");
    let access = store
        .key_access(t, key.key_id)
        .await
        .expect("read")
        .expect("access");
    assert_eq!(
        access.grants.iter().map(|g| &g.grant).collect::<Vec<_>>(),
        vec![&node]
    );
}

/// A workarea's name is unique in its tenant across the whole tree: a
/// second node of that name, under any parent, is refused with `NameTaken`
/// and commits nothing, its event included, while another tenant's node
/// takes the same name (postgres-schema.md `workareas`; ADR-0026 d1, d5).
pub async fn two_workareas_in_one_tenant_cannot_share_a_name<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(30);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let universe = workarea_record(t, Uuid::from_u128(0xB300), None, "universe");
    let metals = workarea_record(
        t,
        Uuid::from_u128(0xB301),
        Some(universe.workarea_id),
        "metals-crew",
    );
    rig.create_workarea(store, universe.clone()).await;
    rig.create_workarea(store, metals.clone()).await;
    let head = store.chain_head(t).await.expect("head");

    let twin = Uuid::from_u128(0xB302);
    for parent in [universe.workarea_id, metals.workarea_id] {
        let refused = rig
            .commit(
                store,
                event_types::WORKAREA_CREATE,
                json!({ "workarea_id": twin.to_string(), "name": "metals-crew" }),
                MetadataFold::WorkareaCreated(workarea_record(
                    t,
                    twin,
                    Some(parent),
                    "metals-crew",
                )),
                None,
            )
            .await;
        assert_eq!(
            refused,
            Err(StoreError::NameTaken {
                kind: NamedKind::Workarea,
                name: name("metals-crew"),
            }),
            "one shared namespace across the tree outside user roots"
        );
    }
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "a refused creation journals nothing"
    );
    assert_eq!(store.workarea(t, twin).await.expect("read"), None);

    let mut other = ChainRig::new(31);
    other.seed_chain(store, 0).await;
    let o = other.tenant_id;
    let root = workarea_record(o, Uuid::from_u128(0xB310), None, "universe");
    let theirs = workarea_record(
        o,
        Uuid::from_u128(0xB311),
        Some(root.workarea_id),
        "metals-crew",
    );
    other.create_workarea(store, root).await;
    other.create_workarea(store, theirs.clone()).await;
    assert_eq!(
        store.workarea(o, theirs.workarea_id).await.expect("read"),
        Some(theirs),
        "another tenant's namespace is its own"
    );
}

/// The root, and only the root, is named `universe`, as the CHECK on
/// `workareas` holds: a root of another name is refused, and a non-root
/// `universe` is refused with `NameTaken`, as errors.md answers it; neither
/// commits anything, its event included (postgres-schema.md `workareas`;
/// ADR-0026 d5, d6).
pub async fn the_root_and_only_the_root_workarea_is_named_universe<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(39);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let head = store.chain_head(t).await.expect("head");
    let stray = Uuid::from_u128(0xB390);
    let refused = rig
        .commit(
            store,
            event_types::WORKAREA_CREATE,
            json!({ "workarea_id": stray.to_string(), "name": "procurement" }),
            MetadataFold::WorkareaCreated(workarea_record(t, stray, None, "procurement")),
            None,
        )
        .await;
    assert!(
        matches!(refused, Err(StoreError::Backend(_))),
        "{refused:?}"
    );
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "a root of another name commits nothing"
    );
    assert_eq!(store.workarea(t, stray).await.expect("read"), None);

    let universe = workarea_record(t, Uuid::from_u128(0xB391), None, "universe");
    rig.create_workarea(store, universe.clone()).await;
    let head = store.chain_head(t).await.expect("head");
    let impostor = Uuid::from_u128(0xB392);
    let refused = rig
        .commit(
            store,
            event_types::WORKAREA_CREATE,
            json!({ "workarea_id": impostor.to_string(), "name": "universe" }),
            MetadataFold::WorkareaCreated(workarea_record(
                t,
                impostor,
                Some(universe.workarea_id),
                "universe",
            )),
            None,
        )
        .await;
    assert_eq!(
        refused,
        Err(StoreError::NameTaken {
            kind: NamedKind::Workarea,
            name: name("universe"),
        })
    );
    assert_eq!(
        store.chain_head(t).await.expect("head"),
        head,
        "a non-root universe commits nothing"
    );
    assert_eq!(store.workarea(t, impostor).await.expect("read"), None);
}

/// A tenant's name is unique in the deployment: a second tenant's genesis
/// under it is refused with `NameTaken` and commits nothing, and the genesis
/// fold of a tenant that has its row changes nothing (postgres-schema.md
/// `tenants`; ADR-0026 d1).
pub async fn a_second_tenant_cannot_take_a_tenants_name<S: JournalStore>(store: &S) {
    let mut first = ChainRig::new(37);
    first.seed_chain(store, 1).await;
    let mut second = ChainRig::new(38);
    let refused = second
        .commit(
            store,
            event_types::TENANT_GENESIS,
            json!({ "tenant_name": first.tenant_name.as_str() }),
            first.genesis_fold(),
            None,
        )
        .await;
    assert_eq!(
        refused,
        Err(StoreError::NameTaken {
            kind: NamedKind::Tenant,
            name: first.tenant_name.clone(),
        })
    );
    assert_eq!(
        store.chain_head(second.tenant_id).await.expect("head"),
        None,
        "the refused genesis journals nothing"
    );
    second.seed_chain(store, 0).await;
}

/// A name resolves to the id of the row of its kind that holds it in the
/// caller's tenant — a workarea, a crew, a key — and to nothing in another
/// tenant, for another kind, or where no row holds it; a tenant's own name
/// is in the deployment's namespace and resolves in no tenant's
/// (http-api.md names and ids; ADR-0026 d1, d4).
pub async fn a_name_resolves_to_its_rows_id_in_the_callers_tenant_and_to_nothing_in_another_tenant<
    S: JournalStore,
>(
    store: &S,
) {
    let mut a = ChainRig::new(32);
    a.seed_chain(store, 0).await;
    let mut b = ChainRig::new(33);
    b.seed_chain(store, 0).await;
    let (ta, tb) = (a.tenant_id, b.tenant_id);
    let universe = workarea_record(ta, Uuid::from_u128(0xB320), None, "universe");
    let metals = workarea_record(
        ta,
        Uuid::from_u128(0xB321),
        Some(universe.workarea_id),
        "metals",
    );
    a.create_workarea(store, universe.clone()).await;
    a.create_workarea(store, metals.clone()).await;
    let redline = crew_record(ta, Uuid::from_u128(0xC320), "redline");
    a.create_crew(store, redline.clone()).await.expect("crew");
    let laptop = KeyRecord {
        name: Some(name("claude-laptop")),
        ..key_record(ta, Uuid::from_u128(0xD320), Role::Writer, None)
    };
    a.issue_key(store, laptop.clone()).await;
    let their_root = Uuid::from_u128(0xB330);
    b.create_workarea(store, workarea_record(tb, their_root, None, "universe"))
        .await;

    for (kind, value, id) in [
        (NamedKind::Workarea, "universe", universe.workarea_id),
        (NamedKind::Workarea, "metals", metals.workarea_id),
        (NamedKind::Crew, "redline", redline.crew_id),
        (NamedKind::Key, "claude-laptop", laptop.key_id),
    ] {
        assert_eq!(
            resolved(store, ta, kind, value).await,
            Some(id),
            "{value} resolves to its row's id"
        );
    }
    assert_eq!(
        resolved(store, tb, NamedKind::Workarea, "universe").await,
        Some(their_root),
        "each tenant resolves its own row of a name both hold"
    );
    for (kind, value) in [
        (NamedKind::Workarea, "metals"),
        (NamedKind::Crew, "redline"),
        (NamedKind::Key, "claude-laptop"),
    ] {
        assert_eq!(
            resolved(store, tb, kind, value).await,
            None,
            "{value} resolves to nothing in another tenant"
        );
    }
    for (kind, value) in [
        (NamedKind::Crew, "metals"),
        (NamedKind::Key, "redline"),
        (NamedKind::Workarea, "claude-laptop"),
        (NamedKind::Workarea, "nowhere"),
    ] {
        assert_eq!(
            resolved(store, ta, kind, value).await,
            None,
            "{value} names no {} of the tenant",
            kind.as_str()
        );
    }
    assert_eq!(
        store
            .resolve_name(ta, NamedKind::Tenant, &a.tenant_name)
            .await
            .expect("resolve"),
        None,
        "a tenant name resolves in no tenant's namespace"
    );
}

/// A revoked key keeps its name: the name still resolves to the key's id,
/// and no other key of the tenant may take it, since a name is never
/// released; a reissued key takes a new name (roles-and-keys.md key names;
/// ADR-0026 d9, d10).
pub async fn a_revoked_keys_name_still_resolves<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(34);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let laptop = KeyRecord {
        name: Some(name("claude-laptop")),
        ..key_record(t, Uuid::from_u128(0xD340), Role::Writer, None)
    };
    rig.issue_key(store, laptop.clone()).await;
    rig.revoke_key(store, laptop.key_id).await.expect("revoke");
    assert_eq!(
        store
            .key_by_hash(laptop.key_hash.as_deref().expect("a bearer key"))
            .await
            .expect("lookup"),
        None,
        "a revoked key authenticates nothing"
    );
    assert_eq!(
        resolved(store, t, NamedKind::Key, "claude-laptop").await,
        Some(laptop.key_id),
        "its name still addresses it"
    );

    let head = store.chain_head(t).await.expect("head");
    let reissued = KeyRecord {
        name: Some(name("claude-laptop")),
        ..key_record(t, Uuid::from_u128(0xD341), Role::Writer, None)
    };
    let refused = rig
        .commit(
            store,
            event_types::KEY_ISSUED,
            json!({ "key_id": reissued.key_id.to_string(), "value_disclosed": false }),
            MetadataFold::KeyIssued(reissued.clone()),
            None,
        )
        .await;
    assert_eq!(
        refused,
        Err(StoreError::NameTaken {
            kind: NamedKind::Key,
            name: name("claude-laptop"),
        }),
        "the name is never released"
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(
        store
            .key_by_hash(reissued.key_hash.as_deref().expect("a bearer key"))
            .await
            .expect("lookup"),
        None
    );

    let renamed = KeyRecord {
        name: Some(name("claude-laptop-2")),
        ..reissued
    };
    rig.issue_key(store, renamed.clone()).await;
    assert_eq!(
        store
            .key_by_hash(renamed.key_hash.as_deref().expect("a bearer key"))
            .await
            .expect("lookup"),
        Some(renamed.clone())
    );
    assert_eq!(
        resolved(store, t, NamedKind::Key, "claude-laptop-2").await,
        Some(renamed.key_id)
    );
}

/// A crew record folds in with `crew.created` and reads back whole, its
/// name unique in its tenant; a workarea or a key that names it reads back
/// with its `crew_id`; and a row naming another tenant's crew, or a crew no
/// tenant holds, is refused and commits nothing — the reference names the
/// tenant with the id, since PostgreSQL runs a foreign-key check without
/// row-level security (postgres-schema.md `crews`; ADR-0026 d8).
pub async fn a_crew_record_round_trips_and_only_its_own_tenants_rows_name_it<S: JournalStore>(
    store: &S,
) {
    let mut a = ChainRig::new(35);
    a.seed_chain(store, 0).await;
    let mut b = ChainRig::new(36);
    b.seed_chain(store, 0).await;
    let (ta, tb) = (a.tenant_id, b.tenant_id);
    let redline = CrewRecord {
        charter_uri: Some("https://crews.example.com/redline/charter.md".to_owned()),
        external_manager: Some("crewd.example.com".to_owned()),
        ..crew_record(ta, Uuid::from_u128(0xC350), "redline")
    };
    a.create_crew(store, redline.clone()).await.expect("crew");
    assert_eq!(
        store.crew(ta, redline.crew_id).await.expect("read"),
        Some(redline.clone())
    );
    assert_eq!(
        store.crew(tb, redline.crew_id).await.expect("read"),
        None,
        "another tenant does not hold it"
    );
    assert_eq!(
        store.crew(ta, Uuid::from_u128(0xC3FF)).await.expect("read"),
        None
    );

    let head = store.chain_head(ta).await.expect("head");
    assert_eq!(
        a.create_crew(store, crew_record(ta, Uuid::from_u128(0xC351), "redline"))
            .await,
        Err(StoreError::NameTaken {
            kind: NamedKind::Crew,
            name: name("redline"),
        })
    );
    assert_eq!(store.chain_head(ta).await.expect("head"), head);
    let theirs = crew_record(tb, Uuid::from_u128(0xC360), "redline");
    b.create_crew(store, theirs.clone())
        .await
        .expect("another tenant's crew takes the name");

    let universe = workarea_record(ta, Uuid::from_u128(0xB350), None, "universe");
    a.create_workarea(store, universe.clone()).await;
    let procurement = WorkareaRecord {
        crew_id: Some(redline.crew_id),
        ..workarea_record(
            ta,
            Uuid::from_u128(0xB351),
            Some(universe.workarea_id),
            "procurement",
        )
    };
    a.create_workarea(store, procurement.clone()).await;
    assert_eq!(
        store
            .workarea(ta, procurement.workarea_id)
            .await
            .expect("read"),
        Some(procurement)
    );
    let member = KeyRecord {
        crew_id: Some(redline.crew_id),
        ..key_record(ta, Uuid::from_u128(0xD350), Role::Writer, None)
    };
    a.issue_key(store, member.clone()).await;
    assert_eq!(
        store
            .key_by_hash(member.key_hash.as_deref().expect("a bearer key"))
            .await
            .expect("lookup"),
        Some(member)
    );

    let head = store.chain_head(ta).await.expect("head");
    for crew_id in [theirs.crew_id, Uuid::from_u128(0xC3FF)] {
        let stray = WorkareaRecord {
            crew_id: Some(crew_id),
            ..workarea_record(
                ta,
                Uuid::from_u128(0xB352),
                Some(universe.workarea_id),
                "stray",
            )
        };
        let refused = a
            .commit(
                store,
                event_types::WORKAREA_CREATE,
                json!({ "workarea_id": stray.workarea_id.to_string() }),
                MetadataFold::WorkareaCreated(stray),
                None,
            )
            .await;
        assert!(
            matches!(refused, Err(StoreError::Backend(_))),
            "{refused:?}"
        );
        let stray_key = KeyRecord {
            crew_id: Some(crew_id),
            ..key_record(ta, Uuid::from_u128(0xD351), Role::Writer, None)
        };
        let refused = a
            .commit(
                store,
                event_types::KEY_ISSUED,
                json!({ "key_id": stray_key.key_id.to_string() }),
                MetadataFold::KeyIssued(stray_key),
                None,
            )
            .await;
        assert!(
            matches!(refused, Err(StoreError::Backend(_))),
            "{refused:?}"
        );
    }
    assert_eq!(
        store.chain_head(ta).await.expect("head"),
        head,
        "a row naming a crew its tenant does not hold commits nothing"
    );
}

/// `crew.updated` replaces a crew record's charter URI and external manager
/// with the full new values and never its name: an update that carries
/// another name, or names a crew its tenant does not hold, is refused and
/// commits nothing (event-catalog.md `crew.updated`; ADR-0026 d8).
pub async fn a_crew_update_replaces_its_charter_and_manager_and_never_its_name<S: JournalStore>(
    store: &S,
) {
    let mut a = ChainRig::new(37);
    a.seed_chain(store, 0).await;
    let mut b = ChainRig::new(38);
    b.seed_chain(store, 0).await;
    let ta = a.tenant_id;
    let redline = crew_record(ta, Uuid::from_u128(0xC370), "redline");
    a.create_crew(store, redline.clone()).await.expect("crew");
    let updated = CrewRecord {
        charter_uri: Some("https://crews.example.com/redline/charter.md".to_owned()),
        external_manager: Some("crewd.example.com".to_owned()),
        ..redline.clone()
    };
    let crew_updated = |crew: &CrewRecord| {
        (
            event_types::CREW_UPDATED,
            json!({ "crew_id": crew.crew_id.to_string() }),
            MetadataFold::CrewUpdated(crew.clone()),
        )
    };
    let (event_type, payload, fold) = crew_updated(&updated);
    a.commit(store, event_type, payload, fold, None)
        .await
        .expect("update");
    assert_eq!(
        store.crew(ta, redline.crew_id).await.expect("read"),
        Some(updated.clone())
    );

    let head = store.chain_head(ta).await.expect("head");
    let renamed = CrewRecord {
        name: name("blueline"),
        ..updated.clone()
    };
    let (event_type, payload, fold) = crew_updated(&renamed);
    let refused = a.commit(store, event_type, payload, fold, None).await;
    assert!(
        matches!(refused, Err(StoreError::Backend(_))),
        "{refused:?}"
    );
    // Another tenant's update names a crew it does not hold.
    let (event_type, payload, fold) = crew_updated(&CrewRecord {
        tenant_id: b.tenant_id,
        ..updated.clone()
    });
    let b_head = store.chain_head(b.tenant_id).await.expect("head");
    let refused = b.commit(store, event_type, payload, fold, None).await;
    assert!(
        matches!(refused, Err(StoreError::Backend(_))),
        "{refused:?}"
    );
    assert_eq!(store.chain_head(ta).await.expect("head"), head);
    assert_eq!(store.chain_head(b.tenant_id).await.expect("head"), b_head);
    assert_eq!(
        store.crew(ta, redline.crew_id).await.expect("read"),
        Some(updated),
        "the refused updates changed nothing"
    );
}

/// The tenant's crew records list in name byte order, and only its own
/// (http-api.md crews; ADR-0026 d8).
pub async fn crews_lists_the_tenants_crew_records_in_name_byte_order<S: JournalStore>(store: &S) {
    let mut a = ChainRig::new(39);
    a.seed_chain(store, 0).await;
    let mut b = ChainRig::new(40);
    b.seed_chain(store, 0).await;
    let ta = a.tenant_id;
    let mut created = Vec::new();
    for (n, crew_name) in [
        (0, "redline"),
        (1, "alpha"),
        (2, "metals-crew"),
        (3, "alpha-2"),
    ] {
        let crew = crew_record(ta, Uuid::from_u128(0xC390 + n), crew_name);
        a.create_crew(store, crew.clone()).await.expect("crew");
        created.push(crew);
    }
    b.create_crew(
        store,
        crew_record(b.tenant_id, Uuid::from_u128(0xC3A0), "zulu"),
    )
    .await
    .expect("another tenant's crew");
    let names: Vec<String> = store
        .crews(ta)
        .await
        .expect("list")
        .iter()
        .map(|c| c.name.as_str().to_owned())
        .collect();
    assert_eq!(names, vec!["alpha", "alpha-2", "metals-crew", "redline"]);
    assert_eq!(
        store.crews(ta).await.expect("list")[0],
        created[1],
        "each record whole"
    );
    assert_eq!(
        store.crews(rig_tenant(99)).await.expect("list"),
        Vec::<CrewRecord>::new(),
        "a tenant with no crew lists none"
    );
}

/// `workarea.crew_updated` points a workarea at a crew record of its own
/// tenant; one naming another tenant's crew, or a workarea the tenant does
/// not hold, is refused and commits nothing (postgres-schema.md `crews`;
/// ADR-0026 d8).
pub async fn a_workarea_crew_update_points_the_node_at_a_crew_record_of_its_own_tenant<
    S: JournalStore,
>(
    store: &S,
) {
    let mut a = ChainRig::new(41);
    a.seed_chain(store, 0).await;
    let mut b = ChainRig::new(42);
    b.seed_chain(store, 0).await;
    let ta = a.tenant_id;
    let universe = workarea_record(ta, Uuid::from_u128(0xB410), None, "universe");
    a.create_workarea(store, universe.clone()).await;
    let redline = crew_record(ta, Uuid::from_u128(0xC410), "redline");
    a.create_crew(store, redline.clone()).await.expect("crew");
    let theirs = crew_record(b.tenant_id, Uuid::from_u128(0xC420), "redline");
    b.create_crew(store, theirs.clone()).await.expect("crew");

    let point = |workarea_id: Uuid, crew_id: Uuid| {
        (
            event_types::WORKAREA_CREW_UPDATED,
            json!({ "workarea_id": workarea_id.to_string(), "crew_id": crew_id.to_string() }),
            MetadataFold::WorkareaCrewUpdated {
                workarea_id,
                crew_id,
            },
        )
    };
    let (event_type, payload, fold) = point(universe.workarea_id, redline.crew_id);
    a.commit(store, event_type, payload, fold, None)
        .await
        .expect("point");
    assert_eq!(
        store
            .workarea(ta, universe.workarea_id)
            .await
            .expect("read")
            .expect("node")
            .crew_id,
        Some(redline.crew_id)
    );

    let head = store.chain_head(ta).await.expect("head");
    for (workarea_id, crew_id) in [
        (universe.workarea_id, theirs.crew_id),
        (universe.workarea_id, Uuid::from_u128(0xC4FF)),
        (Uuid::from_u128(0xB4FF), redline.crew_id),
    ] {
        let (event_type, payload, fold) = point(workarea_id, crew_id);
        let refused = a.commit(store, event_type, payload, fold, None).await;
        assert!(
            matches!(refused, Err(StoreError::Backend(_))),
            "{refused:?}"
        );
    }
    assert_eq!(store.chain_head(ta).await.expect("head"), head);
    assert_eq!(
        store
            .workarea(ta, universe.workarea_id)
            .await
            .expect("read")
            .expect("node")
            .crew_id,
        Some(redline.crew_id)
    );
}

/// A key reads back by its id in its own tenant, a live one with no
/// revocation time and a revoked one with the `at` of its `key.revoked`,
/// since its name and role stay history; another tenant reads nothing
/// (roles-and-keys.md key lifecycle; ADR-0026 d10).
pub async fn a_key_reads_by_its_id_in_its_tenant_a_revoked_one_with_the_at_of_its_key_revoked<
    S: JournalStore,
>(
    store: &S,
) {
    let mut a = ChainRig::new(43);
    a.seed_chain(store, 0).await;
    let mut b = ChainRig::new(44);
    b.seed_chain(store, 0).await;
    let ta = a.tenant_id;
    let key = KeyRecord {
        name: Some(name("claude-laptop")),
        ..key_record(ta, Uuid::from_u128(0xD430), Role::Admin, None)
    };
    a.issue_key(store, key.clone()).await;
    assert_eq!(
        store.key(ta, key.key_id).await.expect("read"),
        Some(key.clone())
    );
    let receipt = a.revoke_key(store, key.key_id).await.expect("revoke");
    let at = store
        .events_range(ta, receipt.seq, 1)
        .await
        .expect("range")
        .remove(0)
        .envelope
        .at;
    assert_eq!(
        store.key(ta, key.key_id).await.expect("read"),
        Some(KeyRecord {
            revoked_at: Some(at),
            ..key.clone()
        }),
        "a revoked key's row stays as history, stamped with its key.revoked's at"
    );
    assert_eq!(
        store.key(b.tenant_id, key.key_id).await.expect("read"),
        None
    );
    assert_eq!(
        store.key(ta, Uuid::from_u128(0xD4FF)).await.expect("read"),
        None
    );
}

/// Every row a creation folds is named, so a store this build writes lists
/// no tenant and no workarea without a name; a key folded with none, as a
/// key that predates names is, is listed (ADR-0026 d9, d11).
pub async fn every_row_a_creation_folds_is_named_and_a_key_with_no_name_is_listed<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(45);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let universe = workarea_record(t, Uuid::from_u128(0xB450), None, "universe");
    rig.create_workarea(store, universe).await;
    let named = KeyRecord {
        name: Some(name("named-key")),
        ..key_record(t, Uuid::from_u128(0xD450), Role::Writer, None)
    };
    rig.issue_key(store, named).await;
    let unnamed = key_record(t, Uuid::from_u128(0xD451), Role::Writer, None);
    rig.issue_key(store, unnamed.clone()).await;
    assert_eq!(
        store.unnamed_rows(t).await.expect("read"),
        UnnamedRows {
            tenant: false,
            workareas: Vec::new(),
            keys: vec![unnamed.key_id],
        }
    );
    assert!(
        store
            .unnamed_rows(rig_tenant(98))
            .await
            .expect("read")
            .is_empty(),
        "a tenant with no rows lists none"
    );
}

/// The guardrail rows over the nodes named: every promoted served tip of
/// kind guardrail, never a retirement version nor a version it replaced,
/// and the held guardrails the named key authored, never another key's; no
/// row of another kind and none off the named nodes (proposed ADR-0037 d3,
/// d4).
pub async fn guardrail_rows_are_the_tips_in_force_and_the_named_keys_own_held_rows<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(47);
    rig.seed_chain(store, 0).await;
    let [wa, elsewhere] = [0xB470_u128, 0xB471].map(Uuid::from_u128);
    let (author, other) = (Uuid::from_u128(0xD470), Uuid::from_u128(0xD471));
    let rail = |wa: Uuid, id: u128, key: Uuid| MemoryRecord {
        kind: MemoryKind::Guardrail,
        guardrail: Some(Box::new(GuardrailFields {
            severity: Severity::Must,
            rule: Some(json!({
                "rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"],
                "effect": "deny",
            })),
        })),
        ..held_memory(
            rig_tenant(47),
            wa,
            Uuid::from_u128(id),
            "wanda",
            key,
            "run the tests first",
        )
    };
    let in_force = rail(wa, 0xC470, other);
    let retired = rail(wa, 0xC471, other);
    let retirement = MemoryRecord {
        guardrail: None,
        ..successor_of(&retired, Uuid::from_u128(0xC472), "retired", true)
    };
    let own_held = rail(wa, 0xC473, author);
    let others_held = rail(wa, 0xC474, other);
    let off_node = rail(elsewhere, 0xC475, other);
    let memory = MemoryRecord {
        kind: MemoryKind::Memory,
        guardrail: None,
        ..rail(wa, 0xC476, other)
    };
    for row in [&in_force, &retired, &off_node, &memory] {
        hold_and_promote(&mut rig, store, row.clone()).await;
    }
    hold_and_ratify(&mut rig, store, retirement).await;
    for row in [&own_held, &others_held] {
        hold(&mut rig, store, row.clone()).await.expect("hold");
    }

    let mut rows = store
        .guardrail_rows(rig.tenant_id, &[wa], author)
        .await
        .expect("guardrail rows");
    rows.sort_by_key(|m| m.memory_id);
    assert_eq!(
        rows.iter().map(|m| m.memory_id).collect::<Vec<_>>(),
        vec![in_force.memory_id, own_held.memory_id]
    );
    assert_eq!(
        rows[0].guardrail, in_force.guardrail,
        "the fields round-trip"
    );
    assert!(
        store
            .guardrail_rows(rig.tenant_id, &[], author)
            .await
            .expect("guardrail rows")
            .is_empty(),
        "no node, no row"
    );
}

// ---------------------------------------------------------------------------
// Claim edges
// ---------------------------------------------------------------------------

/// A tenant with its root workarea and a dreamer key that proposes edges.
struct EdgeWorld {
    rig: ChainRig,
    root: Uuid,
    proposer: KeyRecord,
}

impl EdgeWorld {
    async fn new<S: JournalStore>(store: &S, seed: u64) -> Self {
        let mut rig = ChainRig::new(seed);
        rig.seed_chain(store, 0).await;
        let root = Uuid::from_u128((u128::from(seed) << 32) | 0x01);
        rig.create_workarea(
            store,
            workarea_record(rig.tenant_id, root, None, "universe"),
        )
        .await;
        let proposer = key_record(
            rig.tenant_id,
            Uuid::from_u128((u128::from(seed) << 32) | 0x02),
            Role::Dreamer,
            None,
        );
        rig.issue_key(store, proposer.clone()).await;
        Self {
            rig,
            root,
            proposer,
        }
    }

    /// Holds a version by `author` (key derived from the name) on `node`,
    /// deriving from `parents`, its closure the author plus each parent's
    /// stored closure, as capture computes it.
    async fn hold<S: JournalStore>(
        &mut self,
        store: &S,
        node: Uuid,
        memory_id: Uuid,
        author: &str,
        parents: &[Uuid],
    ) -> MemoryRecord {
        let mut memory = held_memory(
            self.rig.tenant_id,
            node,
            memory_id,
            author,
            author_key(author),
            "edge body",
        );
        let mut principals: std::collections::BTreeSet<String> =
            memory.lineage_principals.iter().cloned().collect();
        let mut key_ids: std::collections::BTreeSet<Uuid> =
            memory.lineage_key_ids.iter().copied().collect();
        for parent in parents {
            let p = store
                .memory(self.rig.tenant_id, *parent)
                .await
                .expect("read")
                .expect("parent held");
            principals.extend(p.lineage_principals);
            key_ids.extend(p.lineage_key_ids);
        }
        memory.derives_from = parents.to_vec();
        memory.lineage_principals = principals.into_iter().collect();
        memory.lineage_key_ids = key_ids.into_iter().collect();
        hold(&mut self.rig, store, memory.clone())
            .await
            .expect("hold");
        memory
    }

    /// A pending edge `src -> dst` by this world's proposer, its
    /// `created_at` the `at` of the rig's next event, which
    /// [`Self::propose`] stamps again when it commits it.
    fn edge(&self, edge_id: Uuid, src: Uuid, dst: Uuid) -> ClaimEdgeRecord {
        ClaimEdgeRecord {
            edge_id,
            tenant_id: self.rig.tenant_id,
            src_artifact_id: src,
            dst_artifact_id: dst,
            label: "acme:supports".to_owned(),
            state: EdgeState::Pending,
            proposer_key_id: self.proposer.key_id,
            proposer_principal: self.proposer.principal.clone(),
            ratifier_key_id: None,
            created_at: ChainRig::at(self.rig.n + 1),
        }
    }

    /// Commits `claim_edge.proposed` folding `edge`, its `created_at` the
    /// event's `at`; the edge as committed.
    async fn propose<S: JournalStore>(
        &mut self,
        store: &S,
        edge: ClaimEdgeRecord,
    ) -> Result<ClaimEdgeRecord, StoreError> {
        let edge = ClaimEdgeRecord {
            created_at: ChainRig::at(self.rig.n + 1),
            ..edge
        };
        self.propose_as_given(store, edge.clone()).await?;
        Ok(edge)
    }

    /// Commits `claim_edge.proposed` folding `edge` exactly as given.
    async fn propose_as_given<S: JournalStore>(
        &mut self,
        store: &S,
        edge: ClaimEdgeRecord,
    ) -> Result<CommitReceipt, StoreError> {
        self.rig
            .commit(
                store,
                event_types::CLAIM_EDGE_PROPOSED,
                json!({
                    "edge_id": edge.edge_id.to_string(),
                    "src_artifact_id": edge.src_artifact_id.to_string(),
                    "dst_artifact_id": edge.dst_artifact_id.to_string(),
                    "label": edge.label,
                    "proposer_key_id": edge.proposer_key_id.to_string(),
                }),
                MetadataFold::ClaimEdgeProposed(edge),
                None,
            )
            .await
    }

    /// Holds a term on `node` by `author`: kind `term`, its `claim_key` the
    /// CURIE `curie` (ontologies.md, a term's `claim_key` is its CURIE).
    async fn hold_term<S: JournalStore>(
        &mut self,
        store: &S,
        node: Uuid,
        memory_id: Uuid,
        author: &str,
        curie: &str,
    ) -> MemoryRecord {
        let memory = MemoryRecord {
            kind: MemoryKind::Term,
            claim_key: curie.to_owned(),
            ..held_memory(
                self.rig.tenant_id,
                node,
                memory_id,
                author,
                author_key(author),
                "term body",
            )
        };
        hold(&mut self.rig, store, memory.clone())
            .await
            .expect("hold term");
        memory
    }

    /// Issues a dreamer key that is not the proposer, to ratify with.
    async fn ratifier<S: JournalStore>(&mut self, store: &S, key_id: Uuid) -> KeyRecord {
        let key = key_record(self.rig.tenant_id, key_id, Role::Dreamer, None);
        self.rig.issue_key(store, key.clone()).await;
        key
    }

    /// Commits `review.ratify` for the edge `edge_id` by `ratifier`,
    /// folding [`MetadataFold::ClaimEdgeRatified`] (event-catalog.md,
    /// `subject_kind: "claim_edge"`).
    async fn ratify<S: JournalStore>(
        &mut self,
        store: &S,
        edge_id: Uuid,
        ratifier: Uuid,
    ) -> Result<CommitReceipt, StoreError> {
        self.rig
            .commit(
                store,
                event_types::REVIEW_RATIFY,
                json!({
                    "subject_kind": "claim_edge",
                    "edge_id": edge_id.to_string(),
                    "ratifier_key_id": ratifier.to_string(),
                }),
                MetadataFold::ClaimEdgeRatified {
                    edge_id,
                    ratifier_key_id: ratifier,
                },
                None,
            )
            .await
    }

    /// Proposes `src -> dst` labelled `label`; the edge as committed.
    async fn propose_labelled<S: JournalStore>(
        &mut self,
        store: &S,
        edge_id: Uuid,
        src: Uuid,
        dst: Uuid,
        label: &str,
    ) -> ClaimEdgeRecord {
        let edge = ClaimEdgeRecord {
            label: label.to_owned(),
            ..self.edge(edge_id, src, dst)
        };
        self.propose(store, edge).await.expect("propose")
    }

    /// The stored flattened closure of `memory_id`, as sets.
    async fn closure<S: JournalStore>(
        &self,
        store: &S,
        memory_id: Uuid,
    ) -> (
        std::collections::BTreeSet<String>,
        std::collections::BTreeSet<Uuid>,
    ) {
        let m = store
            .memory(self.rig.tenant_id, memory_id)
            .await
            .expect("read")
            .expect("held");
        (
            m.lineage_principals.into_iter().collect(),
            m.lineage_key_ids.into_iter().collect(),
        )
    }
}

/// The fixture key id of an author, derived from its principal.
fn author_key(author: &str) -> Uuid {
    let digest = sha256_digest_string(author.as_bytes());
    let hex = digest.trim_start_matches("sha256:");
    Uuid::from_u128(u128::from_str_radix(&hex[..32], 16).expect("hex digest"))
}

/// A proposed edge reads back by its id exactly as proposed, pending with
/// its proposer's key and principal and no ratifier; another tenant and an
/// unknown id read nothing (postgres-schema.md `claim_edges`; event-catalog.md
/// `claim_edge.proposed`).
pub async fn a_proposed_claim_edge_reads_back_pending_with_its_proposer<S: JournalStore>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 301).await;
    let root = world.root;
    let a = world
        .hold(store, root, Uuid::from_u128(0x301A), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(0x301B), "bob", &[])
        .await;
    let edge = world
        .propose(
            store,
            world.edge(Uuid::from_u128(0x301E), a.memory_id, b.memory_id),
        )
        .await
        .expect("propose");

    let read = store
        .claim_edge(world.rig.tenant_id, edge.edge_id)
        .await
        .expect("read")
        .expect("the edge is held");
    assert_eq!(read, edge, "the edge reads back as proposed");
    assert_eq!(read.state, EdgeState::Pending);
    assert_eq!(read.proposer_key_id, world.proposer.key_id);
    assert_eq!(read.proposer_principal, world.proposer.principal);
    assert_eq!(read.ratifier_key_id, None);

    let other = EdgeWorld::new(store, 302).await;
    assert_eq!(
        store
            .claim_edge(other.rig.tenant_id, edge.edge_id)
            .await
            .expect("read"),
        None,
        "another tenant reads no edge of this one"
    );
    assert_eq!(
        store
            .claim_edge(world.rig.tenant_id, Uuid::from_u128(0x301F))
            .await
            .expect("read"),
        None,
        "an unknown id reads nothing"
    );
}

/// A proposal whose `created_at` is not its event's `at` commits nothing:
/// the edge's listing time is its proposal's, never a caller's
/// (http-api.md pagination; event-catalog.md `claim_edge.proposed`).
pub async fn a_claim_edge_whose_created_at_is_not_its_events_at_commits_nothing<S: JournalStore>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 306).await;
    let root = world.root;
    let a = world
        .hold(store, root, Uuid::from_u128(0x306A), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(0x306B), "bob", &[])
        .await;
    let head = store
        .chain_head(world.rig.tenant_id)
        .await
        .expect("head")
        .expect("a chain");
    let edge = ClaimEdgeRecord {
        created_at: ChainRig::at(0),
        ..world.edge(Uuid::from_u128(0x306E), a.memory_id, b.memory_id)
    };
    assert!(
        world.propose_as_given(store, edge.clone()).await.is_err(),
        "a created_at the event does not carry is refused"
    );
    assert_eq!(
        store.chain_head(world.rig.tenant_id).await.expect("head"),
        Some(head),
        "nothing is journalled"
    );
    assert_eq!(
        store
            .claim_edge(world.rig.tenant_id, edge.edge_id)
            .await
            .expect("read"),
        None,
        "no edge is stored"
    );
    let (principals, _) = world.closure(store, a.memory_id).await;
    assert!(!principals.contains("bob"), "no closure is widened");
}

/// A pending edge puts each endpoint's authors, principal and key id, in
/// the other's stored closure, and its proposer in neither
/// (audit-and-lineage.md, pending and ratified alike; ADR-0001 d2; ADR-0012).
pub async fn a_pending_claim_edge_puts_each_endpoints_authors_in_the_others_lineage_closure<
    S: JournalStore,
>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 303).await;
    let root = world.root;
    let a = world
        .hold(store, root, Uuid::from_u128(0x303A), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(0x303B), "bob", &[])
        .await;
    world
        .propose(
            store,
            world.edge(Uuid::from_u128(0x303E), a.memory_id, b.memory_id),
        )
        .await
        .expect("propose");

    for (row, other) in [(&a, "bob"), (&b, "alice")] {
        let (principals, key_ids) = world.closure(store, row.memory_id).await;
        assert!(
            principals.contains(other) && key_ids.contains(&author_key(other)),
            "{} holds {other}, the other endpoint's author",
            row.author_principal
        );
        assert!(
            principals.contains(&row.author_principal),
            "a row keeps its own author"
        );
        assert!(
            !principals.contains(&world.proposer.principal)
                && !key_ids.contains(&world.proposer.key_id),
            "the proposer enters no closure"
        );
    }
}

/// A held row deriving from an endpoint gains the other endpoint's authors
/// when the edge is proposed after it, and the other endpoint does not gain
/// the descendant's, as `derives_from` is followed toward ancestors only
/// (audit-and-lineage.md, the derivation graph; ADR-0001 d2).
pub async fn a_pending_claim_edge_reaches_the_closure_of_a_held_row_deriving_from_an_endpoint<
    S: JournalStore,
>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 304).await;
    let root = world.root;
    let a = world
        .hold(store, root, Uuid::from_u128(0x304A), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(0x304B), "bob", &[])
        .await;
    let d = world
        .hold(
            store,
            root,
            Uuid::from_u128(0x304D),
            "carol",
            &[a.memory_id],
        )
        .await;
    world
        .propose(
            store,
            world.edge(Uuid::from_u128(0x304E), a.memory_id, b.memory_id),
        )
        .await
        .expect("propose");

    let (principals, key_ids) = world.closure(store, d.memory_id).await;
    assert_eq!(
        principals,
        ["alice", "bob", "carol"].map(str::to_owned).into(),
        "the descendant holds both endpoints' authors and its own"
    );
    assert_eq!(
        key_ids,
        ["alice", "bob", "carol"].map(author_key).into(),
        "and their key ids"
    );
    let (principals, _) = world.closure(store, b.memory_id).await;
    assert!(
        !principals.contains("carol"),
        "the other endpoint does not gain its descendant's author"
    );
}

/// A second edge from the far end of a first carries its far endpoint's
/// authors to the first edge's near endpoint: a claim edge is followed
/// transitively (audit-and-lineage.md, the transitive closure).
pub async fn a_chain_of_two_edges_carries_the_far_endpoints_authors_to_the_near_one<
    S: JournalStore,
>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 305).await;
    let root = world.root;
    let a = world
        .hold(store, root, Uuid::from_u128(0x305A), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(0x305B), "bob", &[])
        .await;
    let c = world
        .hold(store, root, Uuid::from_u128(0x305C), "carol", &[])
        .await;
    world
        .propose(
            store,
            world.edge(Uuid::from_u128(0x305E1), a.memory_id, b.memory_id),
        )
        .await
        .expect("first edge");
    let (principals, _) = world.closure(store, a.memory_id).await;
    assert!(!principals.contains("carol"), "no edge reaches carol yet");
    world
        .propose(
            store,
            world.edge(Uuid::from_u128(0x305E2), c.memory_id, b.memory_id),
        )
        .await
        .expect("second edge");

    for row in [&a, &b, &c] {
        let (principals, key_ids) = world.closure(store, row.memory_id).await;
        assert_eq!(
            principals,
            ["alice", "bob", "carol"].map(str::to_owned).into(),
            "{} holds every author of the chain",
            row.author_principal
        );
        assert_eq!(key_ids, ["alice", "bob", "carol"].map(author_key).into());
    }
}

/// A small deterministic generator, so the random graph is reproducible.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// Two paths that must agree: over a random interleaving of held rows,
/// each deriving from earlier ones, and pending edges between rows, every
/// row's stored closure equals the authors of the domain's
/// `lineage_artifacts` over the same `derives_from` and claim edges
/// (audit-and-lineage.md; ADR-0001 d2).
pub async fn the_stores_widened_closure_equals_the_authors_of_the_domains_lineage_artifacts<
    S: JournalStore,
>(
    store: &S,
) {
    const AUTHORS: [&str; 5] = ["alice", "bob", "carol", "dave", "erin"];
    for seed in 0..6_u64 {
        let mut world = EdgeWorld::new(store, 310 + seed).await;
        let root = world.root;
        let mut rng = Lcg(seed.wrapping_mul(0x9E37_79B9) + 7);
        let mut rows: Vec<MemoryRecord> = Vec::new();
        let mut derives = Vec::new();
        let mut edges = Vec::new();
        for step in 0..24_u64 {
            let id_base = (u128::from(310 + seed) << 32) | (u128::from(step) << 8);
            if rows.len() < 2 || rng.below(3) != 0 {
                let mut parents = Vec::new();
                for row in &rows {
                    if rng.below(4) == 0 {
                        parents.push(row.memory_id);
                    }
                }
                let author = AUTHORS[rng.below(AUTHORS.len() as u64) as usize];
                let row = world
                    .hold(
                        store,
                        root,
                        Uuid::from_u128(id_base | 0xA),
                        author,
                        &parents,
                    )
                    .await;
                for parent in &parents {
                    derives.push(DerivesFrom {
                        child: row.memory_id,
                        parent: *parent,
                    });
                }
                rows.push(row);
            } else {
                let n = rows.len() as u64;
                let src = rows[rng.below(n) as usize].memory_id;
                let dst = rows[rng.below(n) as usize].memory_id;
                let edge = world.edge(Uuid::from_u128(id_base | 0xE), src, dst);
                edges.push(ClaimEdge {
                    edge_id: edge.edge_id,
                    from: src,
                    to: dst,
                    label: edge.label.clone(),
                    term_curie: None,
                    state: EdgeState::Pending,
                });
                world.propose(store, edge).await.expect("propose");
            }
        }
        let author_of: std::collections::BTreeMap<Uuid, &str> = rows
            .iter()
            .map(|r| (r.memory_id, r.author_principal.as_str()))
            .collect();
        for row in &rows {
            let expected: std::collections::BTreeSet<String> =
                lineage_artifacts(row.memory_id, &derives, &edges)
                    .iter()
                    .map(|id| author_of[id].to_owned())
                    .collect();
            let expected_keys: std::collections::BTreeSet<Uuid> =
                expected.iter().map(|a| author_key(a)).collect();
            let (principals, key_ids) = world.closure(store, row.memory_id).await;
            assert_eq!(
                principals, expected,
                "seed {seed}: row {}'s stored principals are the domain closure's authors",
                row.memory_id
            );
            assert_eq!(
                key_ids, expected_keys,
                "seed {seed}: row {}'s stored key ids are the domain closure's",
                row.memory_id
            );
        }
    }
}

/// A claim-edge page is newest first, `created_at` then edge id
/// descending, resumes strictly after its cursor, and its artifact filter
/// matches either endpoint, its `within` nodes either endpoint's node;
/// another tenant lists nothing (http-api.md pagination and claim edges).
pub async fn a_claim_edges_page_is_newest_first_resumes_after_its_cursor_and_filters_by_either_endpoint<
    S: JournalStore,
>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 320).await;
    let root = world.root;
    let tenant = world.rig.tenant_id;
    let side = Uuid::from_u128((320_u128 << 32) | 0x03);
    world
        .rig
        .create_workarea(store, workarea_record(tenant, side, Some(root), "side"))
        .await;
    let a = world
        .hold(store, root, Uuid::from_u128(0x320A), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(0x320B), "bob", &[])
        .await;
    let c = world
        .hold(store, side, Uuid::from_u128(0x320C), "carol", &[])
        .await;
    for row in [&a, &b, &c] {
        promote(&mut world.rig, store, row.memory_id)
            .await
            .expect("promote");
    }
    // Proposed out of id order, so the page order is the proposals' time,
    // newest first, and never the ids'.
    let mut proposed = Vec::new();
    for (id, src, dst) in [
        (0x320E3, b.memory_id, c.memory_id),
        (0x320E1, a.memory_id, b.memory_id),
        (0x320E4, c.memory_id, a.memory_id),
        (0x320E2, a.memory_id, a.memory_id),
    ] {
        let edge = world.edge(Uuid::from_u128(id), src, dst);
        proposed.push(world.propose(store, edge).await.expect("propose"));
    }
    let [e3, e1, e4, e2] = <[ClaimEdgeRecord; 4]>::try_from(proposed).expect("four edges");
    // Every endpoint is promoted on a node the selection reads, so its read
    // scope keeps every edge and the page laws alone decide.
    let select = |artifact_id: Option<Uuid>, within: Option<Vec<Uuid>>| ClaimEdgeSelection {
        artifact_id,
        within,
        held_nodes: Vec::new(),
        readable_nodes: vec![root, side],
        reader_key_id: world.proposer.key_id,
    };
    let all = select(None, None);
    let at = |e: &ClaimEdgeRecord| ClaimEdgePosition {
        created_at: e.created_at.clone(),
        edge_id: e.edge_id,
    };

    let first = store
        .claim_edges(tenant, &all, None, 2)
        .await
        .expect("page");
    assert_eq!(
        first,
        vec![e2.clone(), e4.clone()],
        "the first page, newest first"
    );
    let second = store
        .claim_edges(tenant, &all, Some(&at(&first[1])), 2)
        .await
        .expect("page");
    assert_eq!(
        second,
        vec![e1.clone(), e3.clone()],
        "strictly after its cursor"
    );
    assert!(
        store
            .claim_edges(tenant, &all, Some(&at(&e3)), 2)
            .await
            .expect("page")
            .is_empty(),
        "nothing after the oldest edge"
    );

    let on_b = select(Some(b.memory_id), None);
    assert_eq!(
        store
            .claim_edges(tenant, &on_b, None, 10)
            .await
            .expect("page"),
        vec![e1.clone(), e3.clone()],
        "b is e1's target and e3's source"
    );
    let on_c = select(Some(c.memory_id), None);
    assert_eq!(
        store
            .claim_edges(tenant, &on_c, Some(&at(&e4)), 10)
            .await
            .expect("page"),
        vec![e3.clone()],
        "the filter and the cursor both hold"
    );
    let on_side = select(None, Some(vec![side]));
    assert_eq!(
        store
            .claim_edges(tenant, &on_side, None, 10)
            .await
            .expect("page"),
        vec![e4.clone(), e3.clone()],
        "the within nodes match either endpoint's node"
    );
    let a_on_side = select(Some(a.memory_id), Some(vec![side]));
    assert_eq!(
        store
            .claim_edges(tenant, &a_on_side, None, 10)
            .await
            .expect("page"),
        vec![e4.clone()],
        "two predicates both hold"
    );

    let other = EdgeWorld::new(store, 321).await;
    assert!(
        store
            .claim_edges(other.rig.tenant_id, &all, None, 10)
            .await
            .expect("page")
            .is_empty(),
        "another tenant lists none of these edges"
    );
}

/// A claim-edge page holds only the edges whose two endpoints are each a
/// row on a node of `readable_nodes` that is promoted or authored by
/// `reader_key_id`, and its limit counts only those, so a page is one
/// bounded read whatever else the tenant holds (http-api.md claim edges;
/// roles-and-keys.md footnote 5).
pub async fn a_claim_edges_page_holds_only_edges_whose_endpoints_are_promoted_or_the_readers_own_on_its_readable_nodes<
    S: JournalStore,
>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 322).await;
    let root = world.root;
    let tenant = world.rig.tenant_id;
    let side = Uuid::from_u128((322_u128 << 32) | 0x03);
    world
        .rig
        .create_workarea(store, workarea_record(tenant, side, Some(root), "side"))
        .await;
    let p1 = world
        .hold(store, root, Uuid::from_u128(0x322A), "alice", &[])
        .await;
    let p2 = world
        .hold(store, root, Uuid::from_u128(0x322B), "bob", &[])
        .await;
    let held = world
        .hold(store, root, Uuid::from_u128(0x322C), "alice", &[])
        .await;
    let far = world
        .hold(store, side, Uuid::from_u128(0x322D), "carol", &[])
        .await;
    for row in [&p1, &p2, &far] {
        promote(&mut world.rig, store, row.memory_id)
            .await
            .expect("promote");
    }
    let mut proposed = Vec::new();
    for (id, src, dst) in [
        (0x322E1, p1.memory_id, p2.memory_id),
        (0x322E2, p1.memory_id, held.memory_id),
        (0x322E3, far.memory_id, p2.memory_id),
    ] {
        let edge = world.edge(Uuid::from_u128(id), src, dst);
        proposed.push(world.propose(store, edge).await.expect("propose"));
    }
    let [e1, e2, e3] = <[ClaimEdgeRecord; 3]>::try_from(proposed).expect("three edges");
    let stranger = Uuid::from_u128(0x322F);
    let select = |nodes: Vec<Uuid>, reader: Uuid, within: Option<Vec<Uuid>>| ClaimEdgeSelection {
        artifact_id: None,
        within,
        held_nodes: Vec::new(),
        readable_nodes: nodes,
        reader_key_id: reader,
    };
    let page = |selection: ClaimEdgeSelection, limit: usize| async move {
        store
            .claim_edges(tenant, &selection, None, limit)
            .await
            .expect("page")
    };

    assert_eq!(
        page(select(vec![root], stranger, None), 10).await,
        vec![e1.clone()],
        "another author's held row and a row off the readable nodes are out of scope"
    );
    assert_eq!(
        page(select(vec![root], stranger, None), 1).await,
        vec![e1.clone()],
        "the limit counts only the edges in scope"
    );
    assert_eq!(
        page(select(vec![root], author_key("alice"), None), 10).await,
        vec![e2.clone(), e1.clone()],
        "the reader's own held row is in its scope"
    );
    assert_eq!(
        page(select(vec![root, side], stranger, None), 10).await,
        vec![e3.clone(), e1.clone()],
        "both endpoints on readable nodes"
    );
    assert_eq!(
        page(select(vec![root, side], stranger, Some(vec![side])), 10).await,
        vec![e3.clone()],
        "within narrows to either endpoint's node, after the scope holds"
    );
    assert!(
        page(select(Vec::new(), author_key("alice"), None), 10)
            .await
            .is_empty(),
        "no readable node reads no edge"
    );
}

/// Ratifying a pending edge stores it ratified, naming its ratifier, and
/// changes nothing else of it (postgres-schema.md `claim_edges`;
/// event-catalog.md `review.ratify` with `subject_kind: "claim_edge"`).
pub async fn ratifying_a_pending_edge_records_its_ratifier_and_state<S: JournalStore>(store: &S) {
    let mut world = EdgeWorld::new(store, 330).await;
    let root = world.root;
    let a = world
        .hold(store, root, Uuid::from_u128(0x330A), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(0x330B), "bob", &[])
        .await;
    let edge = world
        .propose(
            store,
            world.edge(Uuid::from_u128(0x330E), a.memory_id, b.memory_id),
        )
        .await
        .expect("propose");
    let ratifier = world.ratifier(store, Uuid::from_u128(0x330D)).await;

    world
        .ratify(store, edge.edge_id, ratifier.key_id)
        .await
        .expect("a pending edge ratifies");

    let read = store
        .claim_edge(world.rig.tenant_id, edge.edge_id)
        .await
        .expect("read")
        .expect("the edge is held");
    assert_eq!(
        read,
        ClaimEdgeRecord {
            state: EdgeState::Ratified,
            ratifier_key_id: Some(ratifier.key_id),
            ..edge
        },
        "the edge is ratified by its ratifier, everything else as proposed"
    );
}

/// A second ratification of an edge finds it no longer pending: the commit
/// writes nothing, its event included, and is refused by name, as a lost
/// tip compare-and-set is (ADR-0025 d10's shape; ADR-0021 d5).
pub async fn ratifying_an_edge_already_ratified_writes_nothing_and_is_refused_edge_not_pending<
    S: JournalStore,
>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 331).await;
    let root = world.root;
    let a = world
        .hold(store, root, Uuid::from_u128(0x331A), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(0x331B), "bob", &[])
        .await;
    let edge = world
        .propose(
            store,
            world.edge(Uuid::from_u128(0x331E), a.memory_id, b.memory_id),
        )
        .await
        .expect("propose");
    let first = world.ratifier(store, Uuid::from_u128(0x331D)).await;
    let second = world.ratifier(store, Uuid::from_u128(0x331C)).await;
    world
        .ratify(store, edge.edge_id, first.key_id)
        .await
        .expect("the first ratification");
    let head = store
        .chain_head(world.rig.tenant_id)
        .await
        .expect("head")
        .expect("a chain");

    assert_eq!(
        world.ratify(store, edge.edge_id, second.key_id).await,
        Err(StoreError::EdgeNotPending {
            edge_id: edge.edge_id
        }),
        "an edge already ratified is refused by name"
    );
    assert_eq!(
        store.chain_head(world.rig.tenant_id).await.expect("head"),
        Some(head),
        "the journal head is unchanged"
    );
    let read = store
        .claim_edge(world.rig.tenant_id, edge.edge_id)
        .await
        .expect("read")
        .expect("the edge is held");
    assert_eq!(
        read.ratifier_key_id,
        Some(first.key_id),
        "the first ratifier stands"
    );
}

/// A brief edge folds pending with no ratifier and widens no closure, its
/// ratification records the ratifier and the commit's seq as the brief's
/// order key, and a second ratification writes nothing and is refused
/// `EdgeNotPending` (postgres-schema.md `brief_edges`; ADR-0046 d7, d8).
pub async fn a_brief_edge_folds_pending_and_its_ratification_records_the_ratifier_and_its_seq<
    S: JournalStore,
>(
    store: &S,
) {
    let mut world = EdgeWorld::new(store, 391).await;
    let root = world.root;
    let claim = world
        .hold(store, root, Uuid::from_u128(0x391A), "alice", &[])
        .await;
    let edge = BriefEdgeRecord {
        edge_id: Uuid::from_u128(0x391E),
        tenant_id: world.rig.tenant_id,
        artifact_id: claim.memory_id,
        workarea_id: root,
        state: EdgeState::Pending,
        proposer_key_id: world.proposer.key_id,
        proposer_principal: world.proposer.principal.clone(),
        ratifier_key_id: None,
        created_at: ChainRig::at(world.rig.n + 1),
        ratified_seq: None,
    };
    world
        .rig
        .commit(
            store,
            event_types::BRIEF_EDGE_PROPOSED,
            json!({ "edge_id": edge.edge_id.to_string() }),
            MetadataFold::BriefEdgeProposed(edge.clone()),
            None,
        )
        .await
        .expect("propose");
    let tenant = world.rig.tenant_id;
    assert_eq!(
        store.brief_edge(tenant, edge.edge_id).await.expect("read"),
        Some(edge.clone()),
        "the edge reads back pending as proposed"
    );
    let unwidened = store
        .memory(tenant, claim.memory_id)
        .await
        .expect("read")
        .expect("the claim");
    assert_eq!(unwidened.lineage_principals, claim.lineage_principals);

    let ratifier = world.ratifier(store, Uuid::from_u128(0x391D)).await;
    let receipt = world
        .rig
        .commit(
            store,
            event_types::REVIEW_RATIFY,
            json!({ "subject_kind": "brief_edge", "edge_id": edge.edge_id.to_string() }),
            MetadataFold::BriefEdgeRatified {
                edge_id: edge.edge_id,
                ratifier_key_id: ratifier.key_id,
            },
            None,
        )
        .await
        .expect("ratify");
    let ratified = store
        .brief_edge(tenant, edge.edge_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(ratified.state, EdgeState::Ratified);
    assert_eq!(ratified.ratifier_key_id, Some(ratifier.key_id));
    assert_eq!(ratified.ratified_seq, Some(receipt.seq));
    assert_eq!(
        store.brief_edges(tenant, root).await.expect("read"),
        vec![ratified.clone()],
        "the workarea's brief edges"
    );

    let head = store.chain_head(tenant).await.expect("head");
    let second = world.ratifier(store, Uuid::from_u128(0x391C)).await;
    let head_after_issue = store.chain_head(tenant).await.expect("head");
    assert_ne!(head, head_after_issue);
    assert_eq!(
        world
            .rig
            .commit(
                store,
                event_types::REVIEW_RATIFY,
                json!({ "subject_kind": "brief_edge", "edge_id": edge.edge_id.to_string() }),
                MetadataFold::BriefEdgeRatified {
                    edge_id: edge.edge_id,
                    ratifier_key_id: second.key_id,
                },
                None,
            )
            .await,
        Err(StoreError::EdgeNotPending {
            edge_id: edge.edge_id
        })
    );
    assert_eq!(
        store.chain_head(tenant).await.expect("head"),
        head_after_issue
    );
}

/// A tenant's pending edge `a -> b` between two held versions, and the
/// journal head after its proposal.
async fn pending_edge_world<S: JournalStore>(
    store: &S,
    seed: u64,
) -> (EdgeWorld, ClaimEdgeRecord, ChainHead) {
    let mut world = EdgeWorld::new(store, seed).await;
    let root = world.root;
    let base = u128::from(seed) << 16;
    let a = world
        .hold(store, root, Uuid::from_u128(base | 0xA), "alice", &[])
        .await;
    let b = world
        .hold(store, root, Uuid::from_u128(base | 0xB), "bob", &[])
        .await;
    let edge = world
        .propose(
            store,
            world.edge(Uuid::from_u128(base | 0xE), a.memory_id, b.memory_id),
        )
        .await
        .expect("propose");
    let head = store
        .chain_head(world.rig.tenant_id)
        .await
        .expect("head")
        .expect("a chain");
    (world, edge, head)
}

/// Asserts a refused ratification wrote nothing: `world`'s journal head is
/// still `head` and `edge` reads back exactly as proposed, pending.
async fn assert_ratification_wrote_nothing<S: JournalStore>(
    store: &S,
    world: &EdgeWorld,
    edge: &ClaimEdgeRecord,
    head: ChainHead,
) {
    assert_eq!(
        store.chain_head(world.rig.tenant_id).await.expect("head"),
        Some(head),
        "the journal head is unchanged"
    );
    assert_eq!(
        store
            .claim_edge(world.rig.tenant_id, edge.edge_id)
            .await
            .expect("read"),
        Some(edge.clone()),
        "the edge is still pending, as proposed"
    );
}

/// Ratifying an edge id the tenant does not hold, another tenant's or one
/// no tenant holds, finds no pending edge of the commit's tenant: the
/// commit writes nothing, its event included, and is refused by name, and
/// the other tenant's edge stays pending (postgres-schema.md `claim_edges`,
/// tenant-keyed; ADR-0021 d5).
pub async fn ratifying_an_edge_the_tenant_does_not_hold_writes_nothing_and_is_refused_edge_not_pending<
    S: JournalStore,
>(
    store: &S,
) {
    let (mut owner, edge, owner_head) = pending_edge_world(store, 340).await;
    let mut other = EdgeWorld::new(store, 341).await;
    let other_ratifier = other.ratifier(store, Uuid::from_u128(0x341D)).await;
    let other_head = store
        .chain_head(other.rig.tenant_id)
        .await
        .expect("head")
        .expect("a chain");

    assert_eq!(
        other
            .ratify(store, edge.edge_id, other_ratifier.key_id)
            .await,
        Err(StoreError::EdgeNotPending {
            edge_id: edge.edge_id
        }),
        "another tenant's edge is refused by name"
    );
    assert_eq!(
        store.chain_head(other.rig.tenant_id).await.expect("head"),
        Some(other_head),
        "the ratifying tenant's journal head is unchanged"
    );
    assert_ratification_wrote_nothing(store, &owner, &edge, owner_head).await;

    let owner_ratifier = owner.ratifier(store, Uuid::from_u128(0x340D)).await;
    let owner_head = store
        .chain_head(owner.rig.tenant_id)
        .await
        .expect("head")
        .expect("a chain");
    let unknown = Uuid::from_u128(0x340F);
    assert_eq!(
        owner.ratify(store, unknown, owner_ratifier.key_id).await,
        Err(StoreError::EdgeNotPending { edge_id: unknown }),
        "an edge id no tenant holds is refused by name"
    );
    assert_ratification_wrote_nothing(store, &owner, &edge, owner_head).await;
}

/// Ratifying an edge with its proposer's own key is refused by the store
/// and writes nothing, its event included: the proposer never ratifies its
/// own edge (ADR-0012 R2's backstop, postgres-schema.md
/// `claim_edges_ratifier_is_not_proposer`).
pub async fn ratifying_an_edge_with_its_proposers_key_writes_nothing_and_is_refused<
    S: JournalStore,
>(
    store: &S,
) {
    let (mut world, edge, head) = pending_edge_world(store, 342).await;

    let refused = world
        .ratify(store, edge.edge_id, edge.proposer_key_id)
        .await;
    assert!(
        matches!(refused, Err(StoreError::Backend(_))),
        "the proposer's key is refused by the store, got {refused:?}"
    );
    assert_ratification_wrote_nothing(store, &world, &edge, head).await;
}

/// Ratifying with a key another tenant holds is refused by the store and
/// writes nothing, its event included: a ratifier is a key of the edge's
/// own tenant (postgres-schema.md `claim_edges`, the tenant-keyed foreign
/// key into `api_keys`).
pub async fn ratifying_with_another_tenants_key_writes_nothing_and_is_refused<S: JournalStore>(
    store: &S,
) {
    let (mut world, edge, _) = pending_edge_world(store, 343).await;
    let mut other = EdgeWorld::new(store, 344).await;
    let foreign = other.ratifier(store, Uuid::from_u128(0x344D)).await;
    let head = store
        .chain_head(world.rig.tenant_id)
        .await
        .expect("head")
        .expect("a chain");

    let refused = world.ratify(store, edge.edge_id, foreign.key_id).await;
    assert!(
        matches!(refused, Err(StoreError::Backend(_))),
        "another tenant's key is refused by the store, got {refused:?}"
    );
    assert_ratification_wrote_nothing(store, &world, &edge, head).await;
}

/// A tenant with four terms on its root, and a ratifier.
async fn term_world<S: JournalStore>(
    store: &S,
    seed: u64,
) -> (EdgeWorld, [MemoryRecord; 4], KeyRecord) {
    let mut world = EdgeWorld::new(store, seed).await;
    let root = world.root;
    let base = u128::from(seed) << 16;
    let mut terms = Vec::new();
    for (i, curie) in ["acme:Animal", "acme:Dog", "acme:Puppy", "acme:Cat"]
        .into_iter()
        .enumerate()
    {
        let author = format!("author-{i}");
        terms.push(
            world
                .hold_term(
                    store,
                    root,
                    Uuid::from_u128(base | (0xA0 + i as u128)),
                    &author,
                    curie,
                )
                .await,
        );
    }
    let ratifier = world.ratifier(store, Uuid::from_u128(base | 0xD0)).await;
    let terms: [MemoryRecord; 4] = terms.try_into().expect("four terms");
    (world, terms, ratifier)
}

/// The ratified term edges of a label are each listed once by its
/// endpoints' `claim_key`s, a term's CURIE (ontologies.md; ADR-0021 d5's
/// cycle read), in the order (source, target) ascending.
pub async fn the_ratified_term_edges_of_a_label_list_each_by_its_endpoints_claim_keys<
    S: JournalStore,
>(
    store: &S,
) {
    let (mut world, [animal, dog, puppy, cat], ratifier) = term_world(store, 332).await;
    let tenant = world.rig.tenant_id;
    for (low, src, dst) in [
        (0xE1, &dog, &animal),
        (0xE2, &puppy, &dog),
        (0xE3, &cat, &animal),
    ] {
        let edge = world
            .propose_labelled(
                store,
                Uuid::from_u128((332_u128 << 16) | low),
                src.memory_id,
                dst.memory_id,
                "oml:is_a",
            )
            .await;
        world
            .ratify(store, edge.edge_id, ratifier.key_id)
            .await
            .expect("ratify");
    }

    let pair = |s: &str, d: &str| (s.to_owned(), d.to_owned());
    assert_eq!(
        store
            .ratified_term_edges(tenant, "oml:is_a")
            .await
            .expect("read"),
        vec![
            pair("acme:Cat", "acme:Animal"),
            pair("acme:Dog", "acme:Animal"),
            pair("acme:Puppy", "acme:Dog"),
        ],
        "each ratified is_a edge by its endpoints' CURIEs"
    );
    let other = EdgeWorld::new(store, 333).await;
    assert!(
        store
            .ratified_term_edges(other.rig.tenant_id, "oml:is_a")
            .await
            .expect("read")
            .is_empty(),
        "another tenant reads none of these edges"
    );
}

/// A pending edge and a ratified edge of another label are absent from a
/// label's ratified term edges: the cycle check sees ratified edges of its
/// own label alone (ADR-0021 d5).
pub async fn a_pending_or_other_label_edge_is_absent_from_the_ratified_term_edges<
    S: JournalStore,
>(
    store: &S,
) {
    let (mut world, [animal, dog, puppy, cat], ratifier) = term_world(store, 334).await;
    let tenant = world.rig.tenant_id;
    let ratified = world
        .propose_labelled(
            store,
            Uuid::from_u128((334_u128 << 16) | 0xE1),
            dog.memory_id,
            animal.memory_id,
            "oml:is_a",
        )
        .await;
    world
        .ratify(store, ratified.edge_id, ratifier.key_id)
        .await
        .expect("ratify");
    world
        .propose_labelled(
            store,
            Uuid::from_u128((334_u128 << 16) | 0xE2),
            puppy.memory_id,
            dog.memory_id,
            "oml:is_a",
        )
        .await;
    let part_of = world
        .propose_labelled(
            store,
            Uuid::from_u128((334_u128 << 16) | 0xE3),
            cat.memory_id,
            animal.memory_id,
            "oml:part_of",
        )
        .await;
    world
        .ratify(store, part_of.edge_id, ratifier.key_id)
        .await
        .expect("ratify");

    assert_eq!(
        store
            .ratified_term_edges(tenant, "oml:is_a")
            .await
            .expect("read"),
        vec![("acme:Dog".to_owned(), "acme:Animal".to_owned())],
        "only the ratified is_a edge"
    );
    assert_eq!(
        store
            .ratified_term_edges(tenant, "oml:part_of")
            .await
            .expect("read"),
        vec![("acme:Cat".to_owned(), "acme:Animal".to_owned())],
        "only the ratified part_of edge"
    );
}

/// A ratified edge with a memory at either end is absent from the ratified
/// term edges, whatever its label: the read covers edges between two terms
/// (ADR-0021 d5).
pub async fn a_ratified_edge_between_a_memory_and_a_term_is_absent_from_the_ratified_term_edges<
    S: JournalStore,
>(
    store: &S,
) {
    let (mut world, [animal, dog, ..], ratifier) = term_world(store, 335).await;
    let tenant = world.rig.tenant_id;
    let root = world.root;
    let note = world
        .hold(store, root, Uuid::from_u128(0x335F), "alice", &[])
        .await;
    for (low, src, dst) in [
        (0xE1, note.memory_id, animal.memory_id),
        (0xE2, dog.memory_id, note.memory_id),
    ] {
        let edge = world
            .propose_labelled(
                store,
                Uuid::from_u128((335_u128 << 16) | low),
                src,
                dst,
                "oml:is_a",
            )
            .await;
        world
            .ratify(store, edge.edge_id, ratifier.key_id)
            .await
            .expect("ratify");
    }

    assert!(
        store
            .ratified_term_edges(tenant, "oml:is_a")
            .await
            .expect("read")
            .is_empty(),
        "an edge with a memory endpoint is no term edge"
    );
}

/// Every journal-store law in one call, each against a fresh store.
/// A ratified supersession that gives the replaced version an end writes it
/// in the same commit that moves the tip, readable back exactly; an end the
/// replaced version's own start forbids is refused and the commit changes
/// nothing, its tip included (proposed ADR-0059 d4).
pub async fn a_ratified_end_is_written_with_the_tip_move_or_not_at_all<S: JournalStore>(store: &S) {
    use oml_app::domain::valid_time::{ValidEnd, ValidTime};
    let mut rig = ChainRig::new(433);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let key = Uuid::from_u128(0xE5);
    let mut v1 = held_memory(
        tenant,
        wa,
        Uuid::from_u128(0x7201),
        "rita",
        key,
        "the api is at v3",
    );
    *v1.valid = ValidTime::parse(Some("2025"), None).expect("ends");
    hold(&mut rig, store, v1.clone()).await.expect("hold v1");
    promote(&mut rig, store, v1.memory_id)
        .await
        .expect("promote v1");
    let mut v2 = successor_of(&v1, Uuid::from_u128(0x7202), "the api is at v4", false);
    *v2.valid = ValidTime::parse(Some("2026-06-01"), None).expect("ends");
    hold(&mut rig, store, v2.clone()).await.expect("hold v2");
    // An end before v1's own start: refused, nothing moves.
    let before = ValidEnd::from_stored(
        "2024-01-01T00:00:00.000000Z".to_owned(),
        1_704_067_200_000_000,
    );
    let refused = rig
        .commit(
            store,
            event_types::LIFECYCLE_SUPERSEDE,
            json!({ "new_artifact_id": v2.memory_id.to_string() }),
            MetadataFold::TipSuperseded {
                successor: v2.memory_id,
                replaced: v1.memory_id,
                replaced_valid_until: Some(Box::new(before)),
            },
            None,
        )
        .await;
    assert!(refused.is_err(), "an end before the start is refused");
    let still = store
        .memory(tenant, v1.memory_id)
        .await
        .expect("read")
        .expect("v1");
    assert!(
        still.is_tip && still.valid.until.is_none(),
        "the refused commit changed nothing"
    );
    let end = ValidEnd::from_stored(
        "2026-06-01T00:00:00.000000Z".to_owned(),
        1_780_272_000_000_000,
    );
    rig.commit(
        store,
        event_types::LIFECYCLE_SUPERSEDE,
        json!({ "new_artifact_id": v2.memory_id.to_string() }),
        MetadataFold::TipSuperseded {
            successor: v2.memory_id,
            replaced: v1.memory_id,
            replaced_valid_until: Some(Box::new(end.clone())),
        },
        None,
    )
    .await
    .expect("the ratification commits");
    let ended = store
        .memory(tenant, v1.memory_id)
        .await
        .expect("read")
        .expect("v1");
    assert!(!ended.is_tip, "the tip moved");
    assert_eq!(
        ended.valid.until.as_ref(),
        Some(&end),
        "with the end it gave"
    );
    assert_eq!(
        ended.valid.from.as_ref().map(ValidEnd::as_str),
        Some("2025")
    );
    let tip = store
        .memory(tenant, v2.memory_id)
        .await
        .expect("read")
        .expect("v2");
    assert!(tip.is_tip);
}

pub async fn run_journal_store_suite<S, F, Fut>(new_store: F)
where
    S: JournalStore,
    F: Fn() -> Fut,
    Fut: Future<Output = S>,
{
    // Each law runs boxed, so the suite's future holds a pointer per law
    // rather than the largest law's state inline, and an unoptimized test
    // build stays inside a test thread's default stack.
    Box::pin(journal_commit_and_head(&new_store().await)).await;
    Box::pin(journal_commit_refuses_conflicts(&new_store().await)).await;
    Box::pin(journal_events_range_pages(&new_store().await)).await;
    Box::pin(
        a_tail_scan_joins_each_claim_subject_to_its_row_and_reads_the_head_with_it(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(journal_memory_fold_and_lookup(&new_store().await)).await;
    Box::pin(a_ratified_end_is_written_with_the_tip_move_or_not_at_all(
        &new_store().await,
    ))
    .await;
    Box::pin(recall_serves_promoted_tips_only_within_the_readable_set(
        &new_store().await,
    ))
    .await;
    Box::pin(hits_that_tie_on_score_are_ordered_by_artifact_id(
        &new_store().await,
    ))
    .await;
    Box::pin(steering_is_never_returned_as_a_hit(&new_store().await)).await;
    Box::pin(retired_claims_are_absent_from_recall_and_steering(
        &new_store().await,
    ))
    .await;
    Box::pin(
        steering_in_force_is_ordered_by_depth_then_promoted_at_then_artifact_id(&new_store().await),
    )
    .await;
    Box::pin(
        guardrail_rows_are_the_tips_in_force_and_the_named_keys_own_held_rows(&new_store().await),
    )
    .await;
    Box::pin(recall_own_held_never_returns_another_keys_row_whatever_the_query(&new_store().await))
        .await;
    Box::pin(held_rows_of_every_author_are_read_on_the_named_nodes_alone(
        &new_store().await,
    ))
    .await;
    Box::pin(memories_reads_the_named_rows_in_the_order_named(
        &new_store().await,
    ))
    .await;
    Box::pin(journal_review_claim_laws(&new_store().await)).await;
    Box::pin(journal_review_held_laws(&new_store().await)).await;
    Box::pin(journal_key_lookup_by_hash(&new_store().await)).await;
    Box::pin(a_revoked_key_no_longer_resolves_by_its_hash(
        &new_store().await,
    ))
    .await;
    Box::pin(journal_idempotency_roundtrip(&new_store().await)).await;
    Box::pin(a_second_record_under_one_idempotency_scope_commits_nothing(
        &new_store().await,
    ))
    .await;
    Box::pin(grants_and_denials_round_trip_through_their_folds_and_key_access(&new_store().await))
        .await;
    Box::pin(a_grant_epoch_is_its_event_seq(&new_store().await)).await;
    Box::pin(a_grant_or_denial_the_schema_refuses_commits_nothing(
        &new_store().await,
    ))
    .await;
    Box::pin(
        a_lifted_denial_masks_nothing_and_its_lift_moves_the_permission_epoch(&new_store().await),
    )
    .await;
    Box::pin(workarea_ancestry_walks_parent_ids_to_the_root(
        &new_store().await,
    ))
    .await;
    Box::pin(
        a_workarea_under_another_tenants_node_is_refused_as_one_under_no_node(&new_store().await),
    )
    .await;
    Box::pin(workarea_subtree_lists_every_descendant_once(
        &new_store().await,
    ))
    .await;
    Box::pin(workarea_children_are_immediate_and_ordered_by_name_bytes(
        &new_store().await,
    ))
    .await;
    Box::pin(crew_round_trips_through_workarea_created(
        &new_store().await,
    ))
    .await;
    Box::pin(a_workarea_read_serves_the_status_its_row_stores(
        &new_store().await,
    ))
    .await;
    Box::pin(a_universe_root_created_at_any_status_but_committed_is_refused(&new_store().await))
        .await;
    Box::pin(
        a_proposed_transition_is_pending_and_listed_by_its_workarea_in_proposal_order(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(
        ratifying_a_transition_moves_the_workareas_status_in_the_same_commit_as_its_event(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(
        a_ratification_fold_whose_status_guard_fails_journals_nothing_and_moves_nothing(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(
        a_child_created_between_proposal_and_ratification_refuses_the_step_to_validated(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(
        a_ratification_by_another_key_of_the_proposers_principal_is_refused(&new_store().await),
    )
    .await;
    Box::pin(a_proposal_whose_principal_is_not_its_proposing_keys_is_refused(&new_store().await))
        .await;
    Box::pin(kind_version_and_body_bytes_round_trip_through_memory_held(
        &new_store().await,
    ))
    .await;
    Box::pin(
        promotion_makes_the_version_the_tip_and_stamps_promoted_at_from_its_event(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(a_second_served_tip_of_one_claim_commits_nothing(
        &new_store().await,
    ))
    .await;
    Box::pin(tip_superseded_refuses_when_the_target_is_no_longer_tip(
        &new_store().await,
    ))
    .await;
    Box::pin(
        a_chain_whose_ratification_loses_the_compare_and_set_journals_no_verdict(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(a_second_live_version_with_one_ordinal_commits_nothing(
        &new_store().await,
    ))
    .await;
    Box::pin(
        claim_key_state_counts_every_version_and_only_live_ones_hold_the_key(&new_store().await),
    )
    .await;
    Box::pin(
        an_own_listing_holds_every_state_of_the_key_sets_rows_off_masked_nodes(&new_store().await),
    )
    .await;
    Box::pin(a_tips_listing_holds_only_the_promoted_tips_on_its_nodes(
        &new_store().await,
    ))
    .await;
    Box::pin(
        a_listing_pages_newest_first_and_resumes_strictly_after_its_position(&new_store().await),
    )
    .await;
    Box::pin(two_workareas_in_one_tenant_cannot_share_a_name(
        &new_store().await,
    ))
    .await;
    Box::pin(workarea_names_are_unique_per_namespace_and_resolve_in_their_own(&new_store().await))
        .await;
    Box::pin(the_root_and_only_the_root_workarea_is_named_universe(
        &new_store().await,
    ))
    .await;
    Box::pin(a_second_tenant_cannot_take_a_tenants_name(
        &new_store().await,
    ))
    .await;
    Box::pin(
        a_name_resolves_to_its_rows_id_in_the_callers_tenant_and_to_nothing_in_another_tenant(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(a_revoked_keys_name_still_resolves(&new_store().await)).await;
    Box::pin(a_crew_record_round_trips_and_only_its_own_tenants_rows_name_it(&new_store().await))
        .await;
    Box::pin(a_crew_update_replaces_its_charter_and_manager_and_never_its_name(&new_store().await))
        .await;
    Box::pin(crews_lists_the_tenants_crew_records_in_name_byte_order(
        &new_store().await,
    ))
    .await;
    Box::pin(
        a_workarea_crew_update_points_the_node_at_a_crew_record_of_its_own_tenant(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(
        a_key_reads_by_its_id_in_its_tenant_a_revoked_one_with_the_at_of_its_key_revoked(
            &new_store().await,
        ),
    )
    .await;
    Box::pin(
        every_row_a_creation_folds_is_named_and_a_key_with_no_name_is_listed(&new_store().await),
    )
    .await;
    Box::pin(run_claim_edge_suite(&new_store)).await;
}

/// Every claim-edge law in one call, each against a fresh store: the edge
/// row, the closure widening its proposal folds, the ratification's
/// compare-and-set and the graph reads (postgres-schema.md `claim_edges`;
/// ADR-0001 d2; ADR-0008 d5; ADR-0021). Part of
/// [`run_journal_store_suite`], so every store that runs it runs these.
pub async fn run_claim_edge_suite<S, F, Fut>(new_store: F)
where
    S: JournalStore,
    F: Fn() -> Fut,
    Fut: Future<Output = S>,
{
    a_proposed_claim_edge_reads_back_pending_with_its_proposer(&new_store().await).await;
    a_pending_claim_edge_puts_each_endpoints_authors_in_the_others_lineage_closure(
        &new_store().await,
    )
    .await;
    a_pending_claim_edge_reaches_the_closure_of_a_held_row_deriving_from_an_endpoint(
        &new_store().await,
    )
    .await;
    a_chain_of_two_edges_carries_the_far_endpoints_authors_to_the_near_one(&new_store().await)
        .await;
    the_stores_widened_closure_equals_the_authors_of_the_domains_lineage_artifacts(
        &new_store().await,
    )
    .await;
    a_claim_edges_page_is_newest_first_resumes_after_its_cursor_and_filters_by_either_endpoint(
        &new_store().await,
    )
    .await;
    a_claim_edges_page_holds_only_edges_whose_endpoints_are_promoted_or_the_readers_own_on_its_readable_nodes(
        &new_store().await,
    )
    .await;
    a_claim_edge_whose_created_at_is_not_its_events_at_commits_nothing(&new_store().await).await;
    ratifying_a_pending_edge_records_its_ratifier_and_state(&new_store().await).await;
    a_brief_edge_folds_pending_and_its_ratification_records_the_ratifier_and_its_seq(
        &new_store().await,
    )
    .await;
    ratifying_an_edge_already_ratified_writes_nothing_and_is_refused_edge_not_pending(
        &new_store().await,
    )
    .await;
    ratifying_an_edge_the_tenant_does_not_hold_writes_nothing_and_is_refused_edge_not_pending(
        &new_store().await,
    )
    .await;
    ratifying_an_edge_with_its_proposers_key_writes_nothing_and_is_refused(&new_store().await)
        .await;
    ratifying_with_another_tenants_key_writes_nothing_and_is_refused(&new_store().await).await;
    the_ratified_term_edges_of_a_label_list_each_by_its_endpoints_claim_keys(&new_store().await)
        .await;
    a_pending_or_other_label_edge_is_absent_from_the_ratified_term_edges(&new_store().await).await;
    a_ratified_edge_between_a_memory_and_a_term_is_absent_from_the_ratified_term_edges(
        &new_store().await,
    )
    .await;
}

// ---------------------------------------------------------------------------
// ConnectionStore contracts (ADR-0031 d7, d8, d12)
// ---------------------------------------------------------------------------

/// A fixture connector key: credential `connection` and no hash, since the
/// server mints no value for it (roles-and-keys.md connections; ADR-0031
/// d7).
#[must_use]
pub fn connector_key(tenant_id: Uuid, key_id: Uuid) -> KeyRecord {
    KeyRecord {
        credential: Credential::Connection,
        key_hash: None,
        ..key_record(tenant_id, key_id, Role::Writer, None)
    }
}

/// A fixture connect code of `key_id`, its digest derived from its id.
#[must_use]
pub fn connect_code(code_id: Uuid, key_id: Uuid) -> ConnectCode {
    ConnectCode {
        code_id,
        key_id,
        code_hash: Sha256Hash::digest(code_id.as_bytes()),
        expires_at: fixture_time("2026-10-29T13:10:00.000000Z"),
    }
}

/// A fixture `authorized` connection of `key_id` from `code_id`, as Connect
/// creates it (roles-and-keys.md connections lifecycle).
#[must_use]
pub fn authorized_connection(connection_id: Uuid, key_id: Uuid, code_id: Uuid) -> ConnectionRecord {
    ConnectionRecord {
        connection_id,
        key_id,
        code_id,
        client_id: "https://claude.ai/oauth/mcp-oauth-client-metadata".to_owned(),
        client_kind: ClientKind::Cimd,
        client_metadata_digest: Sha256Hash::digest(b"client metadata"),
        redirect_uri: "https://claude.ai/api/mcp/auth_callback".to_owned(),
        redirect_uri_sent: true,
        resource: "https://oml.example.com/mcp".to_owned(),
        code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_owned(),
        auth_code_hash: Some(Sha256Hash::digest(connection_id.as_bytes())),
        auth_code_expires_at: Some(fixture_time("2026-10-29T13:01:00.000000Z")),
        auth_code_redeemed_at: None,
        state: ConnectionState::Authorized,
        opened_seq: None,
        revoked_seq: None,
        created_at: fixture_time("2026-10-29T13:00:30.000000Z"),
    }
}

/// The SHA-256 of a fixture confirmation handle, derived from the
/// connection its Connect creates.
#[must_use]
pub fn handle_hash(connection_id: Uuid) -> Sha256Hash {
    Sha256Hash::digest(&[b"handle:".as_slice(), connection_id.as_bytes()].concat())
}

/// A fixture authorization request of `key_id`'s code `code_id` under
/// `code_tenant_id`, its handle the one [`handle_hash`] derives from
/// `connection_id`, its client and redirect those of
/// [`authorized_connection`] (postgres-schema.md `oauth_authorizations`).
#[must_use]
pub fn authorization_request(
    code_tenant_id: Uuid,
    key_id: Uuid,
    code_id: Uuid,
    connection_id: Uuid,
) -> AuthorizationRecord {
    let connection = authorized_connection(connection_id, key_id, code_id);
    AuthorizationRecord {
        authorization_id: Uuid::from_u128(connection_id.as_u128() ^ 0xA000_0000),
        handle_hash: handle_hash(connection_id),
        client_id: connection.client_id,
        client_kind: connection.client_kind,
        metadata_digest: connection.client_metadata_digest,
        redirect_uri: connection.redirect_uri,
        redirect_uri_sent: connection.redirect_uri_sent,
        code_challenge: connection.code_challenge,
        state: Some("af0ifjsldkj".to_owned()),
        expires_at: fixture_time("2026-10-29T13:10:00.000000Z"),
        code_id,
        code_tenant_id,
        key_id,
    }
}

/// What a fixture Connect adds for `connection_id`, so that it creates
/// exactly [`authorized_connection`] from [`authorization_request`].
#[must_use]
pub fn connect_grant(connection_id: Uuid) -> ConnectGrant {
    let connection = authorized_connection(connection_id, Uuid::nil(), Uuid::nil());
    ConnectGrant {
        connection_id,
        resource: connection.resource,
        auth_code_hash: connection.auth_code_hash.expect("the fixture has a code"),
        auth_code_expires_at: connection
            .auth_code_expires_at
            .expect("the fixture's code expires"),
        at: connection.created_at,
    }
}

/// A fixture client row of `kind` for `client_id`, never connected.
#[must_use]
pub fn oauth_client(client_id: &str, kind: ClientKind) -> OAuthClientRecord {
    OAuthClientRecord {
        client_id: client_id.to_owned(),
        kind,
        client_name: "Claude".to_owned(),
        redirect_uris: vec!["https://claude.ai/api/mcp/auth_callback".to_owned()],
        metadata_digest: Sha256Hash::digest(b"client metadata"),
        fetched_at: (kind == ClientKind::Cimd).then(|| fixture_time("2026-10-29T12:59:00.000000Z")),
        max_age: (kind == ClientKind::Cimd).then_some(3_600),
        last_used_at: None,
        connected_at: None,
        created_at: fixture_time("2026-10-29T12:59:00.000000Z"),
    }
}

fn fixture_time(at: &str) -> EventTime {
    EventTime::parse(at).expect("fixed-width fixture time")
}

/// The SHA-256 of a fixture connection's authorization code, the one
/// [`authorized_connection`] stores.
#[must_use]
pub fn auth_code_hash(connection_id: Uuid) -> Sha256Hash {
    Sha256Hash::digest(connection_id.as_bytes())
}

/// A fixture token pair, its digests derived from `seed`: an access token
/// that lives to 14:00 and a refresh token to 2026-11-28 (ADR-0031 d10).
#[must_use]
pub fn token_pair(seed: Uuid) -> TokenPair {
    TokenPair {
        access: MintedToken {
            token_hash: Sha256Hash::digest(&[b"access:".as_slice(), seed.as_bytes()].concat()),
            expires_at: fixture_time("2026-10-29T14:00:00.000000Z"),
        },
        refresh: MintedToken {
            token_hash: Sha256Hash::digest(&[b"refresh:".as_slice(), seed.as_bytes()].concat()),
            expires_at: fixture_time("2026-11-28T13:00:00.000000Z"),
        },
    }
}

/// The refresh rule of the token endpoint, for a request from the fixture
/// client to the fixture issuer: the domain's own decision on the facts the
/// store read under the lock (roles-and-keys.md connections; ADR-0031 d10,
/// d11).
fn fixture_refresh_rule(facts: Option<&RefreshFacts>) -> RefreshDecision {
    let issuer = Issuer::parse("https://oml.example.com").expect("fixture issuer");
    let client = authorized_connection(Uuid::nil(), Uuid::nil(), Uuid::nil()).client_id;
    decide_refresh(facts.map(RefreshFacts::row).as_ref(), &client, &issuer)
}

/// Both futures, polled in turn until each is ready: two requests in
/// flight at once, without a runtime dependency.
async fn both<A: Future, B: Future>(a: A, b: B) -> (A::Output, B::Output) {
    let (mut a, mut b) = (pin!(a), pin!(b));
    let (mut done_a, mut done_b) = (None, None);
    poll_fn(|cx| {
        if done_a.is_none()
            && let Poll::Ready(out) = a.as_mut().poll(cx)
        {
            done_a = Some(out);
        }
        if done_b.is_none()
            && let Poll::Ready(out) = b.as_mut().poll(cx)
        {
            done_b = Some(out);
        }
        match (done_a.take(), done_b.take()) {
            (Some(x), Some(y)) => Poll::Ready((x, y)),
            (x, y) => {
                done_a = x;
                done_b = y;
                Poll::Pending
            }
        }
    })
    .await
}

impl ChainRig {
    /// Commit one `connection.code_issued` folding `code`, naming `voids`
    /// as the unredeemed code it replaces. Its payload carries ids and the
    /// expiry only, never the code's digest (event-catalog.md; ADR-0031 d8).
    pub async fn mint_code<S: JournalStore>(
        &mut self,
        store: &S,
        code: ConnectCode,
        voids: Option<Uuid>,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::CONNECTION_CODE_ISSUED,
            json!({
                "code_id": code.code_id.to_string(),
                "key_id": code.key_id.to_string(),
                "issued_by_key_id": Value::Null,
                "expires_at": code.expires_at.as_str(),
                "voids_code_id": voids.map(|v| v.to_string()),
            }),
            MetadataFold::ConnectCodeIssued { code, voids },
            None,
        )
        .await
    }

    /// Commit one `connection.opened` of `connection_id`, naming `replaces`
    /// as the key's earlier open connection, presenting the fixture
    /// authorization code's digest and minting [`token_pair`] of the
    /// connection (ADR-0031 d10, d12).
    pub async fn open_connection<S: JournalStore>(
        &mut self,
        store: &S,
        connection_id: Uuid,
        replaces: Option<Uuid>,
    ) -> Result<CommitReceipt, StoreError> {
        self.open_connection_with(
            store,
            connection_id,
            replaces,
            auth_code_hash(connection_id),
            token_pair(connection_id),
        )
        .await
    }

    /// Commit one `connection.opened` of `connection_id` presenting
    /// `presented`, the digest of an authorization code, and minting
    /// `tokens`. Its payload carries neither digest (ADR-0031 d10, d12).
    pub async fn open_connection_with<S: JournalStore>(
        &mut self,
        store: &S,
        connection_id: Uuid,
        replaces: Option<Uuid>,
        presented: Sha256Hash,
        tokens: TokenPair,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::CONNECTION_OPENED,
            json!({
                "connection_id": connection_id.to_string(),
                "replaces_connection_id": replaces.map(|r| r.to_string()),
            }),
            MetadataFold::ConnectionOpened {
                connection_id,
                replaces,
                auth_code_hash: presented,
                tokens,
            },
            None,
        )
        .await
    }

    /// Commit one `connection.revoked` of `connection_id` with `cause`.
    pub async fn revoke_connection<S: JournalStore>(
        &mut self,
        store: &S,
        connection_id: Uuid,
        cause: RevokedCause,
    ) -> Result<CommitReceipt, StoreError> {
        self.commit(
            store,
            event_types::CONNECTION_REVOKED,
            json!({
                "connection_id": connection_id.to_string(),
                "cause": cause.as_str(),
            }),
            MetadataFold::ConnectionRevoked {
                connection_id,
                cause,
            },
            None,
        )
        .await
    }

    /// The `at` of the rig's last committed event.
    fn head_at(&self) -> EventTime {
        self.head
            .as_ref()
            .expect("the rig has committed")
            .envelope
            .at
            .clone()
    }

    /// Mint `code_id` for `key_id`, spend it, create its `authorized`
    /// connection and open it: the key's open connection from that code.
    async fn opened_connection<S: ConnectionStore>(
        &mut self,
        store: &S,
        key_id: Uuid,
        code_id: Uuid,
        connection_id: Uuid,
        voids: Option<Uuid>,
        replaces: Option<Uuid>,
    ) -> CommitReceipt {
        self.authorized_connection(store, key_id, code_id, connection_id, voids)
            .await;
        self.open_connection(store, connection_id, replaces)
            .await
            .expect("open")
    }

    /// Mint `code_id` for `key_id`, spend it and create its `authorized`
    /// connection, as code entry and Connect do.
    async fn authorized_connection<S: ConnectionStore>(
        &mut self,
        store: &S,
        key_id: Uuid,
        code_id: Uuid,
        connection_id: Uuid,
        voids: Option<Uuid>,
    ) {
        self.mint_code(store, connect_code(code_id, key_id), voids)
            .await
            .expect("mint");
        assert!(
            store
                .spend_code(
                    self.tenant_id,
                    code_id,
                    &fixture_time("2026-10-29T13:00:20.000000Z")
                )
                .await
                .expect("redeem"),
            "a live code is spent by its first correct entry"
        );
        store
            .authorization_insert(authorization_request(
                self.tenant_id,
                key_id,
                code_id,
                connection_id,
            ))
            .await
            .expect("the request's row at a correct code");
        assert_eq!(
            store
                .connection_connect(&handle_hash(connection_id), connect_grant(connection_id))
                .await
                .expect("connect"),
            ConnectOutcome::Authorized {
                tenant_id: self.tenant_id,
                connection: Box::new(authorized_connection(connection_id, key_id, code_id)),
            },
            "Connect creates the authorized connection under the code's tenant"
        );
    }
}

/// The key's codes keyed by id, each as its `voided_seq`.
async fn voided_seqs<S: ConnectionStore>(
    store: &S,
    tenant_id: Uuid,
    key_id: Uuid,
) -> Vec<(Uuid, Option<u64>)> {
    store
        .connect_codes(tenant_id, key_id)
        .await
        .expect("codes")
        .into_iter()
        .map(|c| (c.code_id, c.voided_seq))
        .collect()
}

/// `key.issued` with credential `connection` stores no hash: the key reads
/// back by its id as a connector key with none, so `key_by_hash` resolves
/// it by no value, and a connector key carrying a hash is refused and
/// commits nothing (roles-and-keys.md connections; postgres-schema.md
/// `api_keys_connector_has_no_hash`; ADR-0031 d7).
pub async fn key_issued_with_credential_connection_stores_no_hash<S: ConnectionStore>(store: &S) {
    let mut rig = ChainRig::new(310);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3100));
    rig.issue_key(store, key.clone()).await;
    let read = store.key(t, key.key_id).await.expect("read");
    assert_eq!(read, Some(key.clone()));
    let read = read.expect("the connector key reads back");
    assert_eq!(read.credential, Credential::Connection);
    assert_eq!(read.key_hash, None, "a connector key stores no hash");
    assert_eq!(
        store
            .key_by_hash(&sha256_digest_string(key.key_id.as_bytes()))
            .await
            .expect("lookup"),
        None,
        "no value resolves a connector key"
    );

    let head = store.chain_head(t).await.expect("head");
    let hashed = KeyRecord {
        key_hash: Some(sha256_digest_string(b"a connector value")),
        ..connector_key(t, Uuid::from_u128(0xD3101))
    };
    rig.commit(
        store,
        event_types::KEY_ISSUED,
        json!({ "key_id": hashed.key_id.to_string(), "credential": "connection" }),
        MetadataFold::KeyIssued(hashed.clone()),
        None,
    )
    .await
    .expect_err("a connector key with a hash is refused");
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(store.key(t, hashed.key_id).await.expect("read"), None);
    assert_eq!(
        store
            .key_by_hash(&sha256_digest_string(b"a connector value"))
            .await
            .expect("lookup"),
        None
    );
}

/// Minting a code voids the key's earlier unredeemed code in the same
/// commit, its `voided_seq` that commit's seq; a spent code from which no
/// connection opened is voided with it; and a mint whose `voids` does not
/// name the key's unredeemed code is refused and commits nothing
/// (roles-and-keys.md connect codes; event-catalog.md
/// `connection.code_issued`; ADR-0031 d8).
pub async fn minting_a_code_voids_the_keys_earlier_unredeemed_code_in_the_same_commit<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(311);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3110));
    rig.issue_key(store, key.clone()).await;
    let (c1, c2, c3) = (
        Uuid::from_u128(0xC3111),
        Uuid::from_u128(0xC3112),
        Uuid::from_u128(0xC3113),
    );

    let r1 = rig
        .mint_code(store, connect_code(c1, key.key_id), None)
        .await
        .expect("first mint");
    let codes = store.connect_codes(t, key.key_id).await.expect("codes");
    assert_eq!(codes.len(), 1);
    assert_eq!(
        codes[0].issued_seq, r1.seq,
        "a code's issued_seq is its event's seq"
    );
    assert_eq!(codes[0].code_hash, connect_code(c1, key.key_id).code_hash);
    assert_eq!(codes[0].redeemed_at, None);
    assert_eq!(codes[0].voided_seq, None);

    let r2 = rig
        .mint_code(store, connect_code(c2, key.key_id), Some(c1))
        .await
        .expect("second mint");
    assert_eq!(
        voided_seqs(store, t, key.key_id).await,
        vec![(c1, Some(r2.seq)), (c2, None)],
        "the newer code voids the unredeemed one in its own commit"
    );

    let head = store.chain_head(t).await.expect("head");
    assert_eq!(
        rig.mint_code(store, connect_code(c3, key.key_id), None)
            .await
            .map(|r| r.seq),
        Err(StoreError::ConnectCodeMoved { key_id: key.key_id }),
        "a mint that does not name the unredeemed code it voids is refused by name"
    );
    assert_eq!(
        rig.mint_code(store, connect_code(c3, key.key_id), Some(c1))
            .await
            .map(|r| r.seq),
        Err(StoreError::ConnectCodeMoved { key_id: key.key_id }),
        "a mint naming a code already voided is refused by name"
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(
        voided_seqs(store, t, key.key_id).await,
        vec![(c1, Some(r2.seq)), (c2, None)],
        "a refused mint commits nothing"
    );

    assert!(
        store
            .spend_code(t, c2, &fixture_time("2026-10-29T13:00:20.000000Z"))
            .await
            .expect("redeem")
    );
    assert!(
        !store
            .spend_code(t, c2, &fixture_time("2026-10-29T13:00:21.000000Z"))
            .await
            .expect("redeem"),
        "a spent code is spent once"
    );
    let r3 = rig
        .mint_code(store, connect_code(c3, key.key_id), None)
        .await
        .expect("third mint");
    assert_eq!(
        voided_seqs(store, t, key.key_id).await,
        vec![(c1, Some(r2.seq)), (c2, Some(r3.seq)), (c3, None)],
        "a spent code from which no connection opened is voided too"
    );
}

/// `connection.revoked` voids every code of the key from which no
/// connection opened, spent or not, and leaves the code of an opened
/// connection alone; it revokes an open connection only, never an
/// `authorized` one, and never with the cause `replaced`, which only a
/// newer `connection.opened` gives (roles-and-keys.md connections;
/// event-catalog.md `connection.revoked`; ADR-0031 d8, d12).
pub async fn connection_revoked_voids_every_code_of_the_key_from_which_no_connection_opened<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(312);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3120));
    rig.issue_key(store, key.clone()).await;
    let (c1, c2) = (Uuid::from_u128(0xC3121), Uuid::from_u128(0xC3122));
    let (open, pending) = (Uuid::from_u128(0xE3121), Uuid::from_u128(0xE3122));
    rig.opened_connection(store, key.key_id, c1, open, None, None)
        .await;
    rig.authorized_connection(store, key.key_id, c2, pending, None)
        .await;
    let head = store.chain_head(t).await.expect("head");

    rig.revoke_connection(store, pending, RevokedCause::Admin)
        .await
        .expect_err("an authorized connection is never revoked by name");
    rig.revoke_connection(store, open, RevokedCause::Replaced)
        .await
        .expect_err("only a newer connection.opened replaces a connection");
    assert_eq!(store.chain_head(t).await.expect("head"), head);

    let r = rig
        .revoke_connection(store, open, RevokedCause::Admin)
        .await
        .expect("revoke");
    let revoked = store
        .connection(t, open)
        .await
        .expect("read")
        .expect("the row stays");
    assert_eq!(revoked.state, ConnectionState::Revoked(RevokedCause::Admin));
    assert_eq!(revoked.revoked_seq, Some(r.seq));
    assert_eq!(
        voided_seqs(store, t, key.key_id).await,
        vec![(c1, None), (c2, Some(r.seq))],
        "the opened connection's code is kept; the spent code with no opened connection is voided"
    );
    assert_eq!(
        store
            .connection(t, pending)
            .await
            .expect("read")
            .expect("the authorized row stays")
            .state,
        ConnectionState::Authorized,
        "its voided code ends it at the exchange; the revocation names it nowhere"
    );
    rig.revoke_connection(store, open, RevokedCause::Admin)
        .await
        .expect_err("a connection is revoked once");
}

/// `key.revoked` voids the key's unredeemed codes in its own commit, and
/// leaves a spent code alone (roles-and-keys.md connect codes; ADR-0031
/// d8).
pub async fn key_revoked_voids_the_keys_unredeemed_codes<S: ConnectionStore>(store: &S) {
    let mut rig = ChainRig::new(313);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3130));
    rig.issue_key(store, key.clone()).await;
    let (c1, c2) = (Uuid::from_u128(0xC3131), Uuid::from_u128(0xC3132));
    rig.opened_connection(store, key.key_id, c1, Uuid::from_u128(0xE3131), None, None)
        .await;
    rig.mint_code(store, connect_code(c2, key.key_id), None)
        .await
        .expect("mint");
    let r = rig.revoke_key(store, key.key_id).await.expect("revoke");
    assert_eq!(
        voided_seqs(store, t, key.key_id).await,
        vec![(c1, None), (c2, Some(r.seq))],
        "the unredeemed code is voided by the key's revocation; the spent one is kept"
    );
}

/// A tenant's name reads back as its genesis named it, under that tenant,
/// and a tenant the store does not hold reads no name (http-api.md "The
/// authorization page"; ADR-0031 d9 step 2; ADR-0026 d1).
pub async fn a_tenants_name_reads_back_under_that_tenant<S: ConnectionStore>(store: &S) {
    let mut rig = ChainRig::new(349);
    rig.seed_chain(store, 0).await;
    assert_eq!(
        store.tenant_name(rig.tenant_id).await,
        Ok(Some(rig.tenant_name.clone()))
    );
    assert_eq!(
        store.tenant_name(Uuid::from_u128(0xBAD)).await,
        Ok(None),
        "a tenant the store does not hold has no name"
    );
}

/// A key's connections are listed in every state, by creation and ties by
/// id, and no other key's connection and nothing of another tenant is
/// listed (roles-and-keys.md connections; ADR-0031 d11).
pub async fn a_keys_connections_are_listed_in_every_state_and_no_other_keys<S: ConnectionStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(345);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3180));
    let other = connector_key(t, Uuid::from_u128(0xD3181));
    rig.issue_key(store, key.clone()).await;
    rig.issue_key(store, other.clone()).await;
    let (c1, c2, c3) = (
        Uuid::from_u128(0xC3181),
        Uuid::from_u128(0xC3182),
        Uuid::from_u128(0xC3183),
    );
    let (open, authorized, others) = (
        Uuid::from_u128(0xE3181),
        Uuid::from_u128(0xE3182),
        Uuid::from_u128(0xE3183),
    );
    rig.opened_connection(store, key.key_id, c1, open, None, None)
        .await;
    rig.authorized_connection(store, key.key_id, c2, authorized, None)
        .await;
    rig.authorized_connection(store, other.key_id, c3, others, None)
        .await;
    let listed = store.connections(t, key.key_id).await.expect("listing");
    let states: Vec<(Uuid, ConnectionState)> =
        listed.iter().map(|c| (c.connection_id, c.state)).collect();
    assert_eq!(
        states,
        [
            (open, ConnectionState::Open),
            (authorized, ConnectionState::Authorized)
        ],
        "both of the key's connections, by creation and ties by id, and not the other key's"
    );
    assert_eq!(
        store.connections(Uuid::from_u128(0xBAD), key.key_id).await,
        Ok(Vec::new()),
        "another tenant lists nothing of this one's"
    );
}

/// `connection.opened` opens its `authorized` connection, its `opened_seq`
/// the commit's seq and its `auth_code_redeemed_at` the commit's `at`, and
/// revokes the key's earlier open connection as `replaced` with its
/// `revoked_seq` that seq; one
/// whose `replaces` does not name the key's open connection is refused and
/// commits nothing (roles-and-keys.md connections; event-catalog.md
/// `connection.opened`; ADR-0031 d7, d12).
pub async fn connection_opened_revokes_the_keys_earlier_open_connection_as_replaced<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(314);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3140));
    rig.issue_key(store, key.clone()).await;
    let (c1, c2) = (Uuid::from_u128(0xC3141), Uuid::from_u128(0xC3142));
    let (first, second) = (Uuid::from_u128(0xE3141), Uuid::from_u128(0xE3142));
    let r1 = rig
        .opened_connection(store, key.key_id, c1, first, None, None)
        .await;
    let first_at = rig.head_at();
    let opened = store
        .connection(t, first)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        opened,
        ConnectionRecord {
            state: ConnectionState::Open,
            opened_seq: Some(r1.seq),
            auth_code_redeemed_at: Some(first_at.clone()),
            ..authorized_connection(first, key.key_id, c1)
        },
        "the exchange opens the row, stamps its opened_seq and redeems its authorization code"
    );

    rig.authorized_connection(store, key.key_id, c2, second, None)
        .await;
    let head = store.chain_head(t).await.expect("head");
    assert_eq!(
        rig.open_connection(store, second, None)
            .await
            .map(|r| r.seq),
        Err(StoreError::ConnectionNotOpenable {
            connection_id: second
        }),
        "an opening that names no replaced connection while one is open is refused by name"
    );
    assert_eq!(
        rig.open_connection(store, second, Some(second))
            .await
            .map(|r| r.seq),
        Err(StoreError::ConnectionNotOpenable {
            connection_id: second
        }),
        "an opening that names another connection than the open one is refused by name"
    );
    assert_eq!(
        rig.open_connection(store, first, None).await.map(|r| r.seq),
        Err(StoreError::ConnectionNotOpenable {
            connection_id: first
        }),
        "an open connection opens once"
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);

    let r2 = rig
        .open_connection(store, second, Some(first))
        .await
        .expect("open the second");
    let second_at = rig.head_at();
    assert_eq!(
        store.connection(t, first).await.expect("read"),
        Some(ConnectionRecord {
            state: ConnectionState::Revoked(RevokedCause::Replaced),
            opened_seq: Some(r1.seq),
            revoked_seq: Some(r2.seq),
            auth_code_redeemed_at: Some(first_at),
            ..authorized_connection(first, key.key_id, c1)
        }),
        "the earlier open connection is revoked as replaced in the same commit"
    );
    assert_eq!(
        store.connection(t, second).await.expect("read"),
        Some(ConnectionRecord {
            state: ConnectionState::Open,
            opened_seq: Some(r2.seq),
            auth_code_redeemed_at: Some(second_at),
            ..authorized_connection(second, key.key_id, c2)
        })
    );
    assert_eq!(
        voided_seqs(store, t, key.key_id).await,
        vec![(c1, None), (c2, None)],
        "an opening voids no code"
    );
}

/// `connection.opened` for a revoked key is refused and commits nothing:
/// the fold reads the key's state in the transaction that holds the chain
/// lock, so no connection opens after its key's `key.revoked`
/// (roles-and-keys.md connections; ADR-0031 d11).
pub async fn connection_opened_for_a_revoked_key_is_refused_and_commits_nothing<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(315);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3150));
    rig.issue_key(store, key.clone()).await;
    let (code, connection) = (Uuid::from_u128(0xC3151), Uuid::from_u128(0xE3151));
    rig.authorized_connection(store, key.key_id, code, connection, None)
        .await;
    rig.revoke_key(store, key.key_id).await.expect("revoke");
    let head = store.chain_head(t).await.expect("head");
    assert_eq!(
        rig.open_connection(store, connection, None)
            .await
            .map(|r| r.seq),
        Err(StoreError::ConnectionNotOpenable {
            connection_id: connection
        }),
        "no connection opens for a revoked key"
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(
        store.connection(t, connection).await.expect("read"),
        Some(authorized_connection(connection, key.key_id, code)),
        "the refused opening stamps nothing"
    );
}

/// `connection.opened` for a connection whose connect code a newer mint
/// voided is refused and commits nothing: Connect takes no chain lock, so
/// the fold reads the code's voiding in the transaction that holds it, and
/// a page holding a code, or the authorization code it produced, opens no
/// connection once the admin mints a new code (postgres-schema.md
/// `oauth_connections`; ADR-0031 d8).
pub async fn connection_opened_for_a_voided_code_is_refused_and_commits_nothing<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(316);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3160));
    rig.issue_key(store, key.clone()).await;
    let (spent, newer) = (Uuid::from_u128(0xC3161), Uuid::from_u128(0xC3162));
    let connection = Uuid::from_u128(0xE3161);
    rig.authorized_connection(store, key.key_id, spent, connection, None)
        .await;
    // The spent code is not the key's unredeemed one, so the mint names
    // none; it voids the spent code all the same.
    let r = rig
        .mint_code(store, connect_code(newer, key.key_id), None)
        .await
        .expect("mint");
    assert_eq!(
        voided_seqs(store, t, key.key_id).await,
        vec![(spent, Some(r.seq)), (newer, None)]
    );
    let head = store.chain_head(t).await.expect("head");
    assert_eq!(
        rig.open_connection(store, connection, None)
            .await
            .map(|r| r.seq),
        Err(StoreError::ConnectionNotOpenable {
            connection_id: connection
        }),
        "no connection opens from a voided code"
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(
        store.connection(t, connection).await.expect("read"),
        Some(authorized_connection(connection, key.key_id, spent)),
        "the refused opening stamps nothing"
    );
}

/// `connection.opened` for an authorization code expired at the commit's
/// `at` is refused and commits nothing, as the exchange's conditional
/// update matches no row past `auth_code_expires_at` (postgres-schema.md
/// `oauth_connections`; ADR-0031 d10).
pub async fn connection_opened_for_an_expired_authorization_code_is_refused_and_commits_nothing<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(317);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3170));
    rig.issue_key(store, key.clone()).await;
    let (code, connection) = (Uuid::from_u128(0xC3171), Uuid::from_u128(0xE3171));
    rig.mint_code(store, connect_code(code, key.key_id), None)
        .await
        .expect("mint");
    assert!(
        store
            .spend_code(t, code, &fixture_time("2026-10-29T12:59:50.000000Z"))
            .await
            .expect("redeem")
    );
    // Every rig commit lands after 13:00:00, so this code has expired.
    let expired = ConnectionRecord {
        auth_code_expires_at: Some(fixture_time("2026-10-29T13:00:00.000000Z")),
        ..authorized_connection(connection, key.key_id, code)
    };
    store
        .authorization_insert(authorization_request(t, key.key_id, code, connection))
        .await
        .expect("request");
    store
        .connection_connect(
            &handle_hash(connection),
            ConnectGrant {
                auth_code_expires_at: fixture_time("2026-10-29T13:00:00.000000Z"),
                ..connect_grant(connection)
            },
        )
        .await
        .expect("connect");
    let head = store.chain_head(t).await.expect("head");
    assert_eq!(
        rig.open_connection(store, connection, None)
            .await
            .map(|r| r.seq),
        Err(StoreError::ConnectionNotOpenable {
            connection_id: connection
        }),
        "an expired authorization code opens no connection"
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(
        store.connection(t, connection).await.expect("read"),
        Some(expired),
        "the refused opening stamps nothing"
    );
}

/// A tenant's codes and connections are invisible and unwritable from
/// another tenant: its reads find nothing, its redemption spends nothing,
/// and its Connect and its three folds naming the first tenant's key, code
/// or connection are refused and commit nothing (postgres-schema.md
/// `connect_codes`, `oauth_connections` RLS; ADR-0031 d7, d8, d12;
/// ADR-0034 d12).
pub async fn a_tenants_codes_and_connections_are_invisible_and_unwritable_from_another_tenant<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut a = ChainRig::new(318);
    a.seed_chain(store, 0).await;
    let mut b = ChainRig::new(319);
    b.seed_chain(store, 0).await;
    let (ta, tb) = (a.tenant_id, b.tenant_id);
    let key = connector_key(ta, Uuid::from_u128(0xD3180));
    a.issue_key(store, key.clone()).await;
    let (opened_code, live_code, foreign_code) = (
        Uuid::from_u128(0xC3181),
        Uuid::from_u128(0xC3182),
        Uuid::from_u128(0xC3183),
    );
    let (open, foreign) = (Uuid::from_u128(0xE3181), Uuid::from_u128(0xE3182));
    a.opened_connection(store, key.key_id, opened_code, open, None, None)
        .await;
    a.mint_code(store, connect_code(live_code, key.key_id), None)
        .await
        .expect("mint");
    let codes = store.connect_codes(ta, key.key_id).await.expect("codes");
    let connection = store.connection(ta, open).await.expect("read");
    assert!(connection.is_some());
    let head_a = store.chain_head(ta).await.expect("head");
    let head_b = store.chain_head(tb).await.expect("head");

    assert_eq!(
        store.connect_codes(tb, key.key_id).await.expect("codes"),
        Vec::new(),
        "another tenant's key has no codes here"
    );
    assert_eq!(
        store.connection(tb, open).await.expect("read"),
        None,
        "another tenant's connection is not found here"
    );
    assert!(
        !store
            .spend_code(tb, live_code, &fixture_time("2026-10-29T13:00:20.000000Z"))
            .await
            .expect("redeem"),
        "another tenant's code is not spent from here"
    );
    let foreign_request = authorization_request(tb, key.key_id, live_code, foreign);
    store
        .authorization_insert(foreign_request.clone())
        .await
        .expect("request");
    store
        .connection_connect(&handle_hash(foreign), connect_grant(foreign))
        .await
        .expect_err("Connect naming another tenant's key and code is refused");
    assert_eq!(
        store
            .authorization(&handle_hash(foreign))
            .await
            .expect("read"),
        Some(foreign_request),
        "the refused Connect keeps the request's row"
    );
    b.mint_code(store, connect_code(foreign_code, key.key_id), None)
        .await
        .expect_err("a code for another tenant's key is refused");
    assert_eq!(
        b.open_connection(store, open, None).await.map(|r| r.seq),
        Err(StoreError::ConnectionNotOpenable {
            connection_id: open
        }),
        "another tenant's connection is not opened from here"
    );
    b.revoke_connection(store, open, RevokedCause::Admin)
        .await
        .expect_err("another tenant's connection is not revoked from here");

    assert_eq!(store.chain_head(tb).await.expect("head"), head_b);
    assert_eq!(store.chain_head(ta).await.expect("head"), head_a);
    assert_eq!(
        store.connect_codes(ta, key.key_id).await.expect("codes"),
        codes,
        "the first tenant's codes are untouched, its live code unspent"
    );
    assert_eq!(store.connection(ta, open).await.expect("read"), connection);
    assert_eq!(store.connection(tb, foreign).await.expect("read"), None);
}

/// Connect deletes the authorization request's row by its handle digest,
/// stamps its client's `connected_at` where it is null and creates the
/// `authorized` connection under the code's tenant, in one transaction; the
/// same handle then finds no request, and a later Connect naming the client
/// leaves its stamp alone (postgres-schema.md "connections (ADR-0031)";
/// roles-and-keys.md connections; ADR-0031 d5, d9).
pub async fn connect_takes_the_request_stamps_its_client_and_creates_the_authorized_connection<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(320);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3200));
    rig.issue_key(store, key.clone()).await;
    let client = authorized_connection(Uuid::nil(), Uuid::nil(), Uuid::nil()).client_id;
    store
        .oauth_client_insert(oauth_client(&client, ClientKind::Cimd))
        .await
        .expect("client");
    let (first, second) = (Uuid::from_u128(0xE3201), Uuid::from_u128(0xE3202));
    let (c1, c2) = (Uuid::from_u128(0xC3201), Uuid::from_u128(0xC3202));
    rig.authorized_connection(store, key.key_id, c1, first, None)
        .await;
    assert_eq!(
        store
            .authorization(&handle_hash(first))
            .await
            .expect("read"),
        None,
        "Connect deletes the request's row"
    );
    let stamped = store
        .oauth_client(&client)
        .await
        .expect("read")
        .expect("the client stays");
    assert_eq!(
        stamped,
        OAuthClientRecord {
            connected_at: Some(connect_grant(first).at),
            ..oauth_client(&client, ClientKind::Cimd)
        },
        "the first Connect naming the client stamps connected_at"
    );
    assert_eq!(
        store
            .connection_connect(&handle_hash(first), connect_grant(first))
            .await
            .expect("connect"),
        ConnectOutcome::NoRequest,
        "a used handle finds no request"
    );
    assert_eq!(
        store.connection(t, first).await.expect("read"),
        Some(authorized_connection(first, key.key_id, c1)),
        "the used handle creates nothing more"
    );

    // The authorized connection's code is voided by this mint; the
    // second code's Connect, later, leaves the client's stamp alone.
    rig.mint_code(store, connect_code(c2, key.key_id), None)
        .await
        .expect("mint");
    assert!(
        store
            .spend_code(t, c2, &fixture_time("2026-10-29T13:00:40.000000Z"))
            .await
            .expect("redeem")
    );
    store
        .authorization_insert(authorization_request(t, key.key_id, c2, second))
        .await
        .expect("request");
    let later = fixture_time("2026-10-29T13:00:50.000000Z");
    let outcome = store
        .connection_connect(
            &handle_hash(second),
            ConnectGrant {
                at: later.clone(),
                ..connect_grant(second)
            },
        )
        .await
        .expect("connect");
    assert_eq!(
        outcome,
        ConnectOutcome::Authorized {
            tenant_id: t,
            connection: Box::new(ConnectionRecord {
                created_at: later,
                ..authorized_connection(second, key.key_id, c2)
            }),
        }
    );
    assert_eq!(
        store.oauth_client(&client).await.expect("read"),
        Some(stamped),
        "connected_at is stamped where it is null only"
    );
}

/// A Connect whose insert is refused rolls back as a whole: the request's
/// row stays, its client's `connected_at` stays null and no connection is
/// created; and a Connect whose code was voided since its entry changes
/// nothing either (postgres-schema.md "connections (ADR-0031)", the stamp
/// commits only with the connection row; roles-and-keys.md connect codes;
/// ADR-0031 d5, d8, d9).
pub async fn a_refused_connect_leaves_the_authorization_row_and_connected_at_unchanged<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(321);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3210));
    rig.issue_key(store, key.clone()).await;
    let (first, second) = (Uuid::from_u128(0xE3211), Uuid::from_u128(0xE3212));
    let code = Uuid::from_u128(0xC3211);
    rig.authorized_connection(store, key.key_id, code, first, None)
        .await;

    // A second request naming the same code, by another client: its
    // connection is a second one of one code, which the store refuses.
    let other = "https://other.example/oauth/client-metadata";
    store
        .oauth_client_insert(oauth_client(other, ClientKind::Cimd))
        .await
        .expect("client");
    let request = AuthorizationRecord {
        client_id: other.to_owned(),
        ..authorization_request(t, key.key_id, code, second)
    };
    store
        .authorization_insert(request.clone())
        .await
        .expect("request");
    store
        .connection_connect(&handle_hash(second), connect_grant(second))
        .await
        .expect_err("a second connection of one code is refused");
    assert_eq!(
        store
            .authorization(&handle_hash(second))
            .await
            .expect("read"),
        Some(request.clone()),
        "the refused Connect keeps the request's row"
    );
    assert_eq!(
        store.oauth_client(other).await.expect("read"),
        Some(oauth_client(other, ClientKind::Cimd)),
        "the stamp commits only with the connection row"
    );
    assert_eq!(store.connection(t, second).await.expect("read"), None);

    // A newer code voids the code from which no connection opened.
    rig.mint_code(
        store,
        connect_code(Uuid::from_u128(0xC3212), key.key_id),
        None,
    )
    .await
    .expect("mint");
    assert_eq!(
        store
            .connection_connect(&handle_hash(second), connect_grant(second))
            .await
            .expect("connect"),
        ConnectOutcome::CodeVoided,
        "a request whose code was voided creates no connection"
    );
    assert_eq!(
        store
            .authorization(&handle_hash(second))
            .await
            .expect("read"),
        Some(request),
        "a voided code's Connect changes nothing"
    );
    assert_eq!(
        store.oauth_client(other).await.expect("read"),
        Some(oauth_client(other, ClientKind::Cimd))
    );
    assert_eq!(store.connection(t, second).await.expect("read"), None);
}

/// A connect code is found by its digest before its tenant is known, with
/// its tenant, its key and its state; spending it is one conditional
/// update, so of two racing correct entries exactly one spends it and the
/// other changes nothing (postgres-schema.md "connections (ADR-0031)";
/// roles-and-keys.md connect codes; ADR-0031 d8, d9).
pub async fn a_code_is_spent_by_exactly_one_of_two_racing_correct_entries<S: ConnectionStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(340);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3400));
    rig.issue_key(store, key.clone()).await;
    let code = connect_code(Uuid::from_u128(0xC3401), key.key_id);
    let minted = rig
        .mint_code(store, code.clone(), None)
        .await
        .expect("mint");
    let found = store
        .code_by_hash(&code.code_hash)
        .await
        .expect("lookup")
        .expect("the code's digest finds its row");
    assert_eq!(found.tenant_id, t, "the lookup names the code's tenant");
    assert_eq!(
        (found.row.code_id, found.row.key_id, found.row.issued_seq),
        (code.code_id, key.key_id, minted.seq)
    );
    assert_eq!((found.row.redeemed_at, found.row.voided_seq), (None, None));

    let at = fixture_time("2026-10-29T13:00:20.000000Z");
    let (first, second) = both(
        store.spend_code(t, code.code_id, &at),
        store.spend_code(t, code.code_id, &at),
    )
    .await;
    let spent = [first.expect("spend"), second.expect("spend")];
    assert_eq!(
        spent.iter().filter(|s| **s).count(),
        1,
        "exactly one racing entry spends the code: {spent:?}"
    );
    assert_eq!(
        store
            .code_by_hash(&code.code_hash)
            .await
            .expect("lookup")
            .map(|c| c.row.redeemed_at),
        Some(Some(at)),
        "the code is spent once, at its first correct entry"
    );
}

/// An unknown digest finds no code, and a code spends only while it is
/// unexpired, unvoided and in the tenant that names it: past its expiry,
/// from another tenant, or once voided by a newer code, the update matches
/// nothing and the code is unchanged (ADR-0031 d8).
pub async fn an_expired_or_voided_code_is_not_spent<S: ConnectionStore>(store: &S) {
    let mut rig = ChainRig::new(341);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3410));
    rig.issue_key(store, key.clone()).await;
    let code = connect_code(Uuid::from_u128(0xC3411), key.key_id);
    rig.mint_code(store, code.clone(), None)
        .await
        .expect("mint");
    assert_eq!(
        store
            .code_by_hash(&Sha256Hash::digest(b"no such code"))
            .await
            .expect("lookup"),
        None
    );
    // Its expiry is 13:10:00; at that instant it is expired.
    assert!(
        !store
            .spend_code(
                t,
                code.code_id,
                &fixture_time("2026-10-29T13:10:00.000000Z")
            )
            .await
            .expect("spend"),
        "an expired code is not spent"
    );
    assert!(
        !store
            .spend_code(
                Uuid::from_u128(0xA000_0000 + 342),
                code.code_id,
                &fixture_time("2026-10-29T13:00:20.000000Z")
            )
            .await
            .expect("spend"),
        "another tenant spends nothing"
    );
    let newer = connect_code(Uuid::from_u128(0xC3412), key.key_id);
    rig.mint_code(store, newer, Some(code.code_id))
        .await
        .expect("mint");
    assert!(
        !store
            .spend_code(
                t,
                code.code_id,
                &fixture_time("2026-10-29T13:00:20.000000Z")
            )
            .await
            .expect("spend"),
        "a voided code is not spent"
    );
    assert_eq!(
        store
            .code_by_hash(&code.code_hash)
            .await
            .expect("lookup")
            .map(|c| c.row.redeemed_at),
        Some(None),
        "a refused entry leaves the code unspent"
    );
}

/// The exchange's `connection.opened` redeems by the presented code's
/// digest and inserts the pair it minted in the same commit: a digest that
/// is not the connection's is refused and inserts nothing, and after the
/// opening the code's digest still finds the connection, redeemed, and
/// each token's digest its row; a pair for a connection that is not open
/// is refused (postgres-schema.md, redemption and rotation are conditional
/// updates; ADR-0031 d10, d12).
pub async fn connection_opened_redeems_by_the_code_digest_and_inserts_the_minted_pair<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(342);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3420));
    rig.issue_key(store, key.clone()).await;
    let (code, connection) = (Uuid::from_u128(0xC3421), Uuid::from_u128(0xE3421));
    rig.authorized_connection(store, key.key_id, code, connection, None)
        .await;
    let early = token_pair(Uuid::from_u128(0xF3420));
    store
        .insert_token_pair(
            t,
            connection,
            early.clone(),
            &fixture_time("2026-10-29T13:00:40.000000Z"),
        )
        .await
        .expect_err("an authorized connection holds no tokens");
    assert_eq!(
        store
            .token_by_hash(&early.access.token_hash, TokenKind::Access)
            .await
            .expect("lookup"),
        None
    );

    let head = store.chain_head(t).await.expect("head");
    assert_eq!(
        rig.open_connection_with(
            store,
            connection,
            None,
            Sha256Hash::digest(b"another code"),
            token_pair(connection),
        )
        .await
        .expect_err("a digest that is not the connection's"),
        StoreError::ConnectionNotOpenable {
            connection_id: connection
        }
    );
    assert_eq!(store.chain_head(t).await.expect("head"), head);
    assert_eq!(
        store
            .token_by_hash(&token_pair(connection).access.token_hash, TokenKind::Access)
            .await
            .expect("lookup"),
        None,
        "a refused opening inserts no token"
    );

    let opened = rig
        .open_connection(store, connection, None)
        .await
        .expect("open");
    let at = rig.head_at();
    let pair = token_pair(connection);
    for (minted, kind) in [
        (&pair.access, TokenKind::Access),
        (&pair.refresh, TokenKind::Refresh),
    ] {
        assert_eq!(
            store
                .token_by_hash(&minted.token_hash, kind)
                .await
                .expect("lookup"),
            Some(TenantRow {
                tenant_id: t,
                row: TokenRecord {
                    token_hash: minted.token_hash,
                    connection_id: connection,
                    kind,
                    expires_at: minted.expires_at.clone(),
                    rotated_at: None,
                    created_at: at.clone(),
                },
            }),
            "the opening's commit inserts the minted {kind:?} token"
        );
    }
    let found = store
        .connection_by_auth_code(&auth_code_hash(connection))
        .await
        .expect("lookup")
        .expect("the redeemed code's digest still finds its connection");
    assert_eq!(found.tenant_id, t);
    assert_eq!(found.row.state, ConnectionState::Open);
    assert_eq!(found.row.opened_seq, Some(opened.seq));
    assert_eq!(found.row.auth_code_redeemed_at, Some(at));
    assert_eq!(
        store
            .connection_by_auth_code(&Sha256Hash::digest(b"another code"))
            .await
            .expect("lookup"),
        None
    );
}

/// The fixture opened connection of a fresh rig: the key, its code and the
/// connection opened from it with [`token_pair`] of the connection.
async fn opened_fixture<S: ConnectionStore>(store: &S, seed: u64) -> (ChainRig, Uuid) {
    let mut rig = ChainRig::new(seed);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let n = u128::from(seed) << 4;
    let key = connector_key(t, Uuid::from_u128(0xD0000 + n));
    rig.issue_key(store, key.clone()).await;
    let connection = Uuid::from_u128(0xE0000 + n);
    rig.opened_connection(
        store,
        key.key_id,
        Uuid::from_u128(0xC0000 + n),
        connection,
        None,
        None,
    )
    .await;
    (rig, connection)
}

/// Two racing refreshes presenting one refresh token mint exactly one new
/// pair: the second reads the first's rotation under the connection's row
/// lock and decides reuse, writing nothing, and the presented token is
/// rotated once (postgres-schema.md, redemption and rotation are
/// conditional updates; ADR-0031 d10).
pub async fn two_racing_refreshes_with_one_token_mint_exactly_one_new_pair<S: ConnectionStore>(
    store: &S,
) {
    let (rig, connection) = opened_fixture(store, 343).await;
    let t = rig.tenant_id;
    let presented = token_pair(connection).refresh.token_hash;
    let (next_a, next_b) = (
        token_pair(Uuid::from_u128(0xF343A)),
        token_pair(Uuid::from_u128(0xF343B)),
    );
    let at = fixture_time("2026-10-29T13:01:00.000000Z");
    let (a, b) = both(
        store.rotate_refresh(t, &presented, next_a.clone(), &at, fixture_refresh_rule),
        store.rotate_refresh(t, &presented, next_b.clone(), &at, fixture_refresh_rule),
    )
    .await;
    let decisions = [a.expect("refresh").decision, b.expect("refresh").decision];
    assert_eq!(
        decisions
            .iter()
            .filter(|d| **d == RefreshDecision::Rotate)
            .count(),
        1,
        "exactly one refresh rotates: {decisions:?}"
    );
    assert!(
        decisions.contains(&RefreshDecision::RevokeForReuse),
        "the other reads the rotation and decides reuse: {decisions:?}"
    );
    let mut minted = 0;
    for next in [&next_a, &next_b] {
        let access = store
            .token_by_hash(&next.access.token_hash, TokenKind::Access)
            .await
            .expect("lookup");
        let refresh = store
            .token_by_hash(&next.refresh.token_hash, TokenKind::Refresh)
            .await
            .expect("lookup");
        assert_eq!(
            access.is_some(),
            refresh.is_some(),
            "a pair is inserted whole or not at all"
        );
        minted += usize::from(access.is_some());
    }
    assert_eq!(minted, 1, "exactly one new pair is minted");
    assert_eq!(
        store
            .token_by_hash(&presented, TokenKind::Refresh)
            .await
            .expect("lookup")
            .and_then(|r| r.row.rotated_at),
        Some(at),
        "the presented token is rotated once"
    );
}

/// A rotated refresh token keeps its row until its own expiry, so its
/// reuse inside that window is recognized: presented again it is decided
/// reuse and mints nothing, the sweep before its expiry keeps it, and the
/// sweep at its expiry removes it (postgres-schema.md, the removal
/// paragraph; ADR-0031 d10).
pub async fn a_rotated_refresh_token_keeps_its_row_until_its_own_expiry<S: ConnectionStore>(
    store: &S,
) {
    let (rig, connection) = opened_fixture(store, 344).await;
    let t = rig.tenant_id;
    let presented = token_pair(connection).refresh.token_hash;
    let next = token_pair(Uuid::from_u128(0xF3441));
    let rotated = store
        .rotate_refresh(
            t,
            &presented,
            next,
            &fixture_time("2026-10-29T13:01:00.000000Z"),
            fixture_refresh_rule,
        )
        .await
        .expect("refresh");
    assert_eq!(rotated.decision, RefreshDecision::Rotate);

    // Both access tokens lapse at 14:00, both refresh tokens on 11-28.
    assert_eq!(
        store
            .sweep_expired(t, &fixture_time("2026-11-28T12:59:59.999999Z"))
            .await
            .expect("sweep"),
        SweepReport {
            tokens_removed: 2,
            ..SweepReport::default()
        }
    );
    let kept = store
        .token_by_hash(&presented, TokenKind::Refresh)
        .await
        .expect("lookup")
        .expect("a rotated refresh token keeps its row");
    assert!(kept.row.rotated_at.is_some());
    let again = token_pair(Uuid::from_u128(0xF3442));
    let reused = store
        .rotate_refresh(
            t,
            &presented,
            again.clone(),
            &fixture_time("2026-11-28T12:59:59.999999Z"),
            fixture_refresh_rule,
        )
        .await
        .expect("refresh");
    assert_eq!(
        reused.decision,
        RefreshDecision::RevokeForReuse,
        "reuse inside the window is recognized"
    );
    assert_eq!(
        store
            .token_by_hash(&again.refresh.token_hash, TokenKind::Refresh)
            .await
            .expect("lookup"),
        None,
        "a reuse mints nothing"
    );
    assert_eq!(
        store
            .sweep_expired(t, &fixture_time("2026-11-28T13:00:00.000000Z"))
            .await
            .expect("sweep"),
        SweepReport {
            tokens_removed: 2,
            ..SweepReport::default()
        }
    );
    assert_eq!(
        store
            .token_by_hash(&presented, TokenKind::Refresh)
            .await
            .expect("lookup"),
        None,
        "the row goes at its own expiry"
    );
}

/// An expired refresh token is not rotated and is not reuse: the rule
/// answers `oauth.refresh_invalid` (expired), the token's row is left
/// unrotated, no pair is minted and the connection stays open
/// (postgres-schema.md, redemption and rotation are conditional updates;
/// ADR-0031 d10).
pub async fn an_expired_refresh_token_is_not_rotated_and_is_not_reuse<S: ConnectionStore>(
    store: &S,
) {
    let (rig, connection) = opened_fixture(store, 345).await;
    let t = rig.tenant_id;
    let short = TokenPair {
        refresh: MintedToken {
            expires_at: fixture_time("2026-10-29T13:00:30.000000Z"),
            ..token_pair(Uuid::from_u128(0xF3451)).refresh
        },
        ..token_pair(Uuid::from_u128(0xF3451))
    };
    store
        .insert_token_pair(
            t,
            connection,
            short.clone(),
            &fixture_time("2026-10-29T13:00:10.000000Z"),
        )
        .await
        .expect("an open connection's pair");
    let next = token_pair(Uuid::from_u128(0xF3452));
    let outcome: RefreshOutcome = store
        .rotate_refresh(
            t,
            &short.refresh.token_hash,
            next.clone(),
            &fixture_time("2026-10-29T13:00:40.000000Z"),
            fixture_refresh_rule,
        )
        .await
        .expect("refresh");
    assert_eq!(
        outcome.decision,
        RefreshDecision::Refuse(RefreshInvalid::Expired),
        "an expired token is refused, not reuse"
    );
    let facts = outcome.facts.expect("the row was read");
    assert_eq!(
        (facts.expired, facts.rotated, facts.key),
        (true, false, KeyState::Active)
    );
    assert_eq!(
        store
            .token_by_hash(&short.refresh.token_hash, TokenKind::Refresh)
            .await
            .expect("lookup")
            .map(|r| r.row.rotated_at),
        Some(None),
        "the expired token is not rotated"
    );
    assert_eq!(
        store
            .token_by_hash(&next.refresh.token_hash, TokenKind::Refresh)
            .await
            .expect("lookup"),
        None,
        "no pair is minted"
    );
    assert_eq!(
        store
            .connection(t, connection)
            .await
            .expect("read")
            .map(|c| c.state),
        Some(ConnectionState::Open),
        "nothing is revoked"
    );
}

/// At `/mcp`-kind lookup a refresh token's digest finds nothing, and at a
/// refresh an access token's digest finds nothing: each digest finds its
/// row only under its own kind (roles-and-keys.md connections; ADR-0031
/// d11).
pub async fn at_mcp_kind_lookup_a_refresh_tokens_digest_finds_nothing<S: ConnectionStore>(
    store: &S,
) {
    let (rig, connection) = opened_fixture(store, 346).await;
    let pair = token_pair(connection);
    let lookup = |hash: Sha256Hash, kind: TokenKind| async move {
        store
            .token_by_hash(&hash, kind)
            .await
            .expect("lookup")
            .map(|r| (r.tenant_id, r.row.kind))
    };
    assert_eq!(
        lookup(pair.refresh.token_hash, TokenKind::Access).await,
        None,
        "a refresh token is no bearer credential at /mcp"
    );
    assert_eq!(
        lookup(pair.access.token_hash, TokenKind::Refresh).await,
        None,
        "an access token is no refresh token"
    );
    assert_eq!(
        lookup(pair.access.token_hash, TokenKind::Access).await,
        Some((rig.tenant_id, TokenKind::Access))
    );
    assert_eq!(
        lookup(pair.refresh.token_hash, TokenKind::Refresh).await,
        Some((rig.tenant_id, TokenKind::Refresh))
    );
}

/// Connect after Cancel finds no authorization row: Cancel deletes the
/// request's row by its handle digest and returns it, a second Cancel
/// finds nothing, and Connect answers no request and creates no
/// connection (postgres-schema.md "connections (ADR-0031)"; ADR-0031 d9).
pub async fn connect_after_cancel_finds_no_authorization_row<S: ConnectionStore>(store: &S) {
    let mut rig = ChainRig::new(347);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3470));
    rig.issue_key(store, key.clone()).await;
    let (code, connection) = (Uuid::from_u128(0xC3471), Uuid::from_u128(0xE3471));
    rig.mint_code(store, connect_code(code, key.key_id), None)
        .await
        .expect("mint");
    assert!(
        store
            .spend_code(t, code, &fixture_time("2026-10-29T13:00:20.000000Z"))
            .await
            .expect("spend")
    );
    let request = authorization_request(t, key.key_id, code, connection);
    store
        .authorization_insert(request.clone())
        .await
        .expect("request");
    assert_eq!(
        store
            .take_authorization_by_handle(&handle_hash(connection))
            .await
            .expect("cancel"),
        Some(request),
        "Cancel takes the request's row"
    );
    assert_eq!(
        store
            .take_authorization_by_handle(&handle_hash(connection))
            .await
            .expect("cancel"),
        None
    );
    assert_eq!(
        store
            .connection_connect(&handle_hash(connection), connect_grant(connection))
            .await
            .expect("connect"),
        ConnectOutcome::NoRequest,
        "Connect after Cancel finds no row"
    );
    assert_eq!(store.connection(t, connection).await.expect("read"), None);
}

/// The sweep removes an `authorized` connection once its authorization
/// code lapsed unexchanged, and an authorization request 24 hours after its
/// expiry, and nothing before either instant (postgres-schema.md, the
/// removal paragraph; ADR-0031 d9, d10).
pub async fn the_sweep_removes_a_lapsed_authorized_connection_and_a_request_a_day_after_it_lapses<
    S: ConnectionStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(348);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3480));
    rig.issue_key(store, key.clone()).await;
    let connection = Uuid::from_u128(0xE3481);
    rig.authorized_connection(
        store,
        key.key_id,
        Uuid::from_u128(0xC3481),
        connection,
        None,
    )
    .await;
    // A request whose page was abandoned: it lapses at 13:10:00.
    let abandoned = Uuid::from_u128(0xE3482);
    store
        .authorization_insert(authorization_request(
            t,
            key.key_id,
            Uuid::from_u128(0xC3482),
            abandoned,
        ))
        .await
        .expect("request");

    // The authorization code lapses at 13:01:00.
    assert_eq!(
        store
            .sweep_expired(t, &fixture_time("2026-10-29T13:00:59.999999Z"))
            .await
            .expect("sweep"),
        SweepReport::default()
    );
    assert_eq!(
        store
            .sweep_expired(t, &fixture_time("2026-10-29T13:01:00.000000Z"))
            .await
            .expect("sweep"),
        SweepReport {
            connections_removed: 1,
            ..SweepReport::default()
        }
    );
    assert_eq!(store.connection(t, connection).await.expect("read"), None);
    assert_eq!(
        store
            .connection_by_auth_code(&auth_code_hash(connection))
            .await
            .expect("lookup"),
        None
    );
    assert_eq!(
        store
            .sweep_expired(t, &fixture_time("2026-10-30T13:09:59.999999Z"))
            .await
            .expect("sweep"),
        SweepReport::default(),
        "a lapsed request stays a day"
    );
    assert_eq!(
        store
            .sweep_expired(t, &fixture_time("2026-10-30T13:10:00.000000Z"))
            .await
            .expect("sweep"),
        SweepReport {
            authorizations_removed: 1,
            ..SweepReport::default()
        }
    );
    assert_eq!(
        store
            .authorization(&handle_hash(abandoned))
            .await
            .expect("read"),
        None
    );
}

/// Once a connection is revoked, the sweep clears its authorization code's
/// digest and removes its token rows, whatever their expiry, while before
/// the sweep the redeemed code's digest still finds the revoked connection
/// (postgres-schema.md, the removal paragraph; ADR-0031 d10).
pub async fn revocation_clears_the_auth_code_digest_and_the_connections_tokens_at_the_sweep<
    S: ConnectionStore,
>(
    store: &S,
) {
    let (mut rig, connection) = opened_fixture(store, 349).await;
    let t = rig.tenant_id;
    rig.revoke_connection(store, connection, RevokedCause::Admin)
        .await
        .expect("revoke");
    assert_eq!(
        store
            .connection_by_auth_code(&auth_code_hash(connection))
            .await
            .expect("lookup")
            .map(|c| c.row.state),
        Some(ConnectionState::Revoked(RevokedCause::Admin))
    );
    assert_eq!(
        store
            .sweep_expired(t, &fixture_time("2026-10-29T13:00:59.999999Z"))
            .await
            .expect("sweep"),
        SweepReport {
            tokens_removed: 2,
            codes_cleared: 1,
            ..SweepReport::default()
        }
    );
    assert_eq!(
        store
            .connection_by_auth_code(&auth_code_hash(connection))
            .await
            .expect("lookup"),
        None
    );
    assert_eq!(
        store
            .connection(t, connection)
            .await
            .expect("read")
            .map(|c| c.auth_code_hash),
        Some(None),
        "the revoked connection's row stays, its digest cleared"
    );
    let pair = token_pair(connection);
    assert_eq!(
        store
            .token_by_hash(&pair.access.token_hash, TokenKind::Access)
            .await
            .expect("lookup"),
        None
    );
    assert_eq!(
        store
            .token_by_hash(&pair.refresh.token_hash, TokenKind::Refresh)
            .await
            .expect("lookup"),
        None
    );
}

// ---------------------------------------------------------------------------
// The client store: the document cache and dynamic registrations (ADR-0031 d5)
// ---------------------------------------------------------------------------

/// The fixture document of a metadata-document client `n`, as fetched.
fn fetched_document(n: u32, digest: &[u8]) -> ClientDescription {
    ClientDescription {
        client_id: format!("https://client-{n}.example.com/oauth/client.json"),
        client_name: format!("Client {n}"),
        redirect_uris: vec!["https://claude.ai/api/mcp/auth_callback".to_owned()],
        metadata_digest: Sha256Hash::digest(digest),
    }
}

/// The fixture registration `n`, as admitted.
fn registration(n: u32) -> ClientDescription {
    ClientDescription {
        client_id: format!("dcr-{n:08}"),
        client_name: format!("Registered {n}"),
        redirect_uris: vec!["https://claude.ai/api/mcp/auth_callback".to_owned()],
        metadata_digest: Sha256Hash::digest(&n.to_be_bytes()),
    }
}

/// A cached row of document `n`, last used at `last_used`, connected at
/// `connected` when it is set.
fn cached_row(n: u32, last_used: &str, connected: Option<&str>) -> OAuthClientRecord {
    let document = fetched_document(n, b"cached");
    OAuthClientRecord {
        client_id: document.client_id,
        client_name: document.client_name,
        redirect_uris: document.redirect_uris,
        metadata_digest: document.metadata_digest,
        last_used_at: Some(fixture_time(last_used)),
        connected_at: connected.map(fixture_time),
        ..oauth_client("", ClientKind::Cimd)
    }
}

/// A registration's row `n`, created at `created`, connected at
/// `connected` when it is set.
fn registered_row(n: u32, created: &str, connected: Option<&str>) -> OAuthClientRecord {
    let registration = registration(n);
    OAuthClientRecord {
        client_id: registration.client_id,
        client_name: registration.client_name,
        redirect_uris: registration.redirect_uris,
        metadata_digest: registration.metadata_digest,
        connected_at: connected.map(fixture_time),
        created_at: fixture_time(created),
        ..oauth_client("", ClientKind::Dcr)
    }
}

/// A full document cache evicts the least recently used document whose
/// client has not connected, and never refuses the fetched document: a
/// connected client's row stays however long unused, and a touch moves a
/// document out of the eviction's way (postgres-schema.md `oauth_clients`;
/// http-api.md client identity; ADR-0031 d5).
pub async fn a_full_document_cache_evicts_the_least_recently_used_unconnected_client_and_never_refuses<
    S: ConnectionStore,
>(
    store: &S,
) {
    let connected = CACHED_DOCUMENTS_MAX - 2;
    for n in 0..connected {
        store
            .oauth_client_insert(cached_row(
                n,
                "2026-10-29T10:00:00.000000Z",
                Some("2026-10-29T10:00:01.000000Z"),
            ))
            .await
            .expect("connected row");
    }
    let (older, newer) = (connected, connected + 1);
    store
        .oauth_client_insert(cached_row(older, "2026-10-29T11:00:00.000000Z", None))
        .await
        .expect("older unconnected row");
    store
        .oauth_client_insert(cached_row(newer, "2026-10-29T12:00:00.000000Z", None))
        .await
        .expect("newer unconnected row");
    let older_id = fetched_document(older, b"").client_id;
    let newer_id = fetched_document(newer, b"").client_id;
    assert!(
        store
            .touch_cimd(&older_id, &fixture_time("2026-10-29T12:30:00.000000Z"))
            .await
            .expect("touch"),
        "a cached document's use is recorded"
    );
    assert!(
        !store
            .touch_cimd(
                "https://absent.example.com/client.json",
                &fixture_time("2026-10-29T12:30:00.000000Z")
            )
            .await
            .expect("touch"),
        "a touch of a document no row holds touches nothing"
    );

    let fetched_at = fixture_time("2026-10-29T13:00:00.000000Z");
    let fresh = fetched_document(CACHED_DOCUMENTS_MAX, b"fresh");
    assert_eq!(
        store
            .upsert_cimd(fresh.clone(), &fetched_at, 600)
            .await
            .expect("a full cache never refuses"),
        DocumentCaching::Cached {
            evicted: Some(newer_id.clone())
        },
        "the least recently used unconnected document is evicted"
    );
    assert_eq!(
        store.oauth_client(&newer_id).await.expect("read"),
        None,
        "the evicted row is gone"
    );
    assert!(
        store.oauth_client(&older_id).await.expect("read").is_some(),
        "the touched document stays"
    );
    assert!(
        store
            .oauth_client(&fetched_document(0, b"").client_id)
            .await
            .expect("read")
            .is_some(),
        "a connected client's row stays, used least recently or not"
    );
    assert_eq!(
        store.oauth_client(&fresh.client_id).await.expect("read"),
        Some(OAuthClientRecord {
            client_id: fresh.client_id.clone(),
            kind: ClientKind::Cimd,
            client_name: fresh.client_name.clone(),
            redirect_uris: fresh.redirect_uris.clone(),
            metadata_digest: fresh.metadata_digest,
            fetched_at: Some(fetched_at.clone()),
            max_age: Some(600),
            last_used_at: Some(fetched_at.clone()),
            connected_at: None,
            created_at: fetched_at,
        }),
        "the fetched document is cached for its Cache-Control lifetime"
    );
}

/// When every cached document's client has connected, a full cache writes
/// nothing and the fetched document serves its request uncached; a cached
/// client's refetch still refreshes its row (ADR-0031 d5).
pub async fn when_every_cached_client_has_connected_the_fetched_document_is_served_uncached<
    S: ConnectionStore,
>(
    store: &S,
) {
    for n in 0..CACHED_DOCUMENTS_MAX {
        store
            .oauth_client_insert(cached_row(
                n,
                "2026-10-29T10:00:00.000000Z",
                Some("2026-10-29T10:00:01.000000Z"),
            ))
            .await
            .expect("connected row");
    }
    let fetched_at = fixture_time("2026-10-29T13:00:00.000000Z");
    let fresh = fetched_document(CACHED_DOCUMENTS_MAX, b"fresh");
    assert_eq!(
        store
            .upsert_cimd(fresh.clone(), &fetched_at, 600)
            .await
            .expect("a full cache never refuses"),
        DocumentCaching::NotCached
    );
    assert_eq!(
        store.oauth_client(&fresh.client_id).await.expect("read"),
        None,
        "an uncached document writes no row"
    );
    for n in 0..CACHED_DOCUMENTS_MAX {
        assert!(
            store
                .oauth_client(&fetched_document(n, b"").client_id)
                .await
                .expect("read")
                .is_some(),
            "no connected client's row is evicted"
        );
    }
    let refetched = fetched_document(0, b"refetched");
    assert_eq!(
        store
            .upsert_cimd(refetched.clone(), &fetched_at, 600)
            .await
            .expect("refetch"),
        DocumentCaching::Cached { evicted: None },
        "a cached client's refetch refreshes its own row"
    );
    assert_eq!(
        store
            .oauth_client(&refetched.client_id)
            .await
            .expect("read")
            .map(|row| row.metadata_digest),
        Some(refetched.metadata_digest)
    );
}

/// A refetch refreshes a cached document's row in place: its name, redirect
/// URIs, digest, fetch, lifetime and use are the new fetch's, and its
/// `connected_at` and `created_at` are kept (postgres-schema.md
/// `oauth_clients.connected_at`, "a refetch keeps it"; ADR-0031 d5).
pub async fn a_refetch_keeps_a_clients_connected_at<S: ConnectionStore>(store: &S) {
    let stamped = cached_row(
        2,
        "2026-10-29T12:00:00.000000Z",
        Some("2026-10-29T12:10:00.000000Z"),
    );
    store
        .oauth_client_insert(stamped.clone())
        .await
        .expect("a connected client's row");
    let refetched_at = fixture_time("2026-10-29T14:00:00.000000Z");
    let refetched = ClientDescription {
        client_id: stamped.client_id.clone(),
        client_name: "Client 2, renamed".to_owned(),
        redirect_uris: vec![
            "https://claude.ai/api/mcp/auth_callback".to_owned(),
            "http://127.0.0.1/callback".to_owned(),
        ],
        metadata_digest: Sha256Hash::digest(b"refetched"),
    };
    assert_eq!(
        store
            .upsert_cimd(refetched.clone(), &refetched_at, 7 * 86_400)
            .await
            .expect("refetch"),
        DocumentCaching::Cached { evicted: None }
    );
    assert_eq!(
        store.oauth_client(&stamped.client_id).await.expect("read"),
        Some(OAuthClientRecord {
            client_id: refetched.client_id,
            kind: ClientKind::Cimd,
            client_name: refetched.client_name,
            redirect_uris: refetched.redirect_uris,
            metadata_digest: refetched.metadata_digest,
            fetched_at: Some(refetched_at.clone()),
            max_age: Some(DOCUMENT_KEPT_SECONDS),
            last_used_at: Some(refetched_at),
            connected_at: stamped.connected_at,
            created_at: stamped.created_at,
        }),
        "a refetch keeps connected_at and created_at, and a lifetime past 24 hours is kept as 24 hours"
    );
}

/// A registration is refused with [`StoreError::CapReached`], writing
/// nothing, once 10 000 registered clients exist that have not connected
/// and are younger than 24 hours; a connected registration, one 24 hours
/// old and a cached document are not counted (http-api.md
/// `POST /oauth/register`; ADR-0031 d5).
pub async fn registration_past_10_000_unused_clients_is_refused<S: ConnectionStore>(store: &S) {
    let at = fixture_time("2026-10-30T12:00:00.000000Z");
    store
        .oauth_client_insert(registered_row(
            UNUSED_REGISTRATIONS_MAX + 1,
            "2026-10-30T11:00:00.000000Z",
            Some("2026-10-30T11:05:00.000000Z"),
        ))
        .await
        .expect("connected registration");
    store
        .oauth_client_insert(registered_row(
            UNUSED_REGISTRATIONS_MAX + 2,
            "2026-10-29T12:00:00.000000Z",
            None,
        ))
        .await
        .expect("a registration 24 hours old");
    store
        .oauth_client_insert(cached_row(1, "2026-10-30T11:00:00.000000Z", None))
        .await
        .expect("cached document");
    for n in 0..UNUSED_REGISTRATIONS_MAX - 1 {
        store
            .oauth_client_insert(registered_row(n, "2026-10-30T11:00:00.000000Z", None))
            .await
            .expect("unused registration");
    }
    let last = registration(UNUSED_REGISTRATIONS_MAX - 1);
    store
        .insert_dcr(last.clone(), &at)
        .await
        .expect("the 10 000th unused registration is admitted");
    assert_eq!(
        store.oauth_client(&last.client_id).await.expect("read"),
        Some(OAuthClientRecord {
            client_id: last.client_id,
            kind: ClientKind::Dcr,
            client_name: last.client_name,
            redirect_uris: last.redirect_uris,
            metadata_digest: last.metadata_digest,
            fetched_at: None,
            max_age: None,
            last_used_at: None,
            connected_at: None,
            created_at: at.clone(),
        })
    );
    let past = registration(UNUSED_REGISTRATIONS_MAX + 3);
    assert_eq!(
        store.insert_dcr(past.clone(), &at).await,
        Err(StoreError::CapReached),
        "registration past 10 000 unused clients is refused"
    );
    assert_eq!(
        store.oauth_client(&past.client_id).await.expect("read"),
        None,
        "a refused registration writes nothing"
    );
    store
        .insert_dcr(past, &fixture_time("2026-10-31T11:00:00.000000Z"))
        .await
        .expect("registration is admitted again once the unused ones are 24 hours old");
}

/// The removal of unused registrations takes every registration that has
/// not connected and was created 24 hours or more before now, and nothing
/// else: a younger or a connected registration and a cached document stay
/// (postgres-schema.md `oauth_clients`; ADR-0031 d5).
pub async fn a_registered_client_unconnected_after_24_hours_is_removed<S: ConnectionStore>(
    store: &S,
) {
    store
        .insert_dcr(
            registration(1),
            &fixture_time("2026-10-29T12:00:00.000000Z"),
        )
        .await
        .expect("lapsing registration");
    store
        .insert_dcr(
            registration(2),
            &fixture_time("2026-10-29T13:00:00.000000Z"),
        )
        .await
        .expect("younger registration");
    store
        .oauth_client_insert(registered_row(
            3,
            "2026-10-29T11:00:00.000000Z",
            Some("2026-10-29T11:30:00.000000Z"),
        ))
        .await
        .expect("connected registration");
    store
        .oauth_client_insert(cached_row(4, "2026-10-29T11:00:00.000000Z", None))
        .await
        .expect("cached document");
    assert_eq!(
        store
            .remove_unused_dcr(&fixture_time("2026-10-29T12:00:00.000000Z"))
            .await
            .expect("remove"),
        1,
        "the registration 24 hours unconnected is removed"
    );
    assert_eq!(
        store
            .oauth_client(&registration(1).client_id)
            .await
            .expect("read"),
        None
    );
    for client_id in [
        registration(2).client_id,
        registration(3).client_id,
        fetched_document(4, b"").client_id,
    ] {
        assert!(
            store
                .oauth_client(&client_id)
                .await
                .expect("read")
                .is_some(),
            "{client_id} stays"
        );
    }
}

/// Every connection-store law in one call, each against a fresh store.
pub async fn run_connection_store_suite<S, F, Fut>(new_store: F)
where
    S: ConnectionStore,
    F: Fn() -> Fut,
    Fut: Future<Output = S>,
{
    key_issued_with_credential_connection_stores_no_hash(&new_store().await).await;
    minting_a_code_voids_the_keys_earlier_unredeemed_code_in_the_same_commit(&new_store().await)
        .await;
    connection_revoked_voids_every_code_of_the_key_from_which_no_connection_opened(
        &new_store().await,
    )
    .await;
    key_revoked_voids_the_keys_unredeemed_codes(&new_store().await).await;
    connection_opened_revokes_the_keys_earlier_open_connection_as_replaced(&new_store().await)
        .await;
    a_keys_connections_are_listed_in_every_state_and_no_other_keys(&new_store().await).await;
    a_tenants_name_reads_back_under_that_tenant(&new_store().await).await;
    connection_opened_for_a_revoked_key_is_refused_and_commits_nothing(&new_store().await).await;
    connection_opened_for_a_voided_code_is_refused_and_commits_nothing(&new_store().await).await;
    connection_opened_for_an_expired_authorization_code_is_refused_and_commits_nothing(
        &new_store().await,
    )
    .await;
    a_tenants_codes_and_connections_are_invisible_and_unwritable_from_another_tenant(
        &new_store().await,
    )
    .await;
    connect_takes_the_request_stamps_its_client_and_creates_the_authorized_connection(
        &new_store().await,
    )
    .await;
    a_refused_connect_leaves_the_authorization_row_and_connected_at_unchanged(&new_store().await)
        .await;
    a_code_is_spent_by_exactly_one_of_two_racing_correct_entries(&new_store().await).await;
    an_expired_or_voided_code_is_not_spent(&new_store().await).await;
    connection_opened_redeems_by_the_code_digest_and_inserts_the_minted_pair(&new_store().await)
        .await;
    two_racing_refreshes_with_one_token_mint_exactly_one_new_pair(&new_store().await).await;
    a_rotated_refresh_token_keeps_its_row_until_its_own_expiry(&new_store().await).await;
    an_expired_refresh_token_is_not_rotated_and_is_not_reuse(&new_store().await).await;
    at_mcp_kind_lookup_a_refresh_tokens_digest_finds_nothing(&new_store().await).await;
    connect_after_cancel_finds_no_authorization_row(&new_store().await).await;
    the_sweep_removes_a_lapsed_authorized_connection_and_a_request_a_day_after_it_lapses(
        &new_store().await,
    )
    .await;
    revocation_clears_the_auth_code_digest_and_the_connections_tokens_at_the_sweep(
        &new_store().await,
    )
    .await;
    a_full_document_cache_evicts_the_least_recently_used_unconnected_client_and_never_refuses(
        &new_store().await,
    )
    .await;
    when_every_cached_client_has_connected_the_fetched_document_is_served_uncached(
        &new_store().await,
    )
    .await;
    a_refetch_keeps_a_clients_connected_at(&new_store().await).await;
    registration_past_10_000_unused_clients_is_refused(&new_store().await).await;
    a_registered_client_unconnected_after_24_hours_is_removed(&new_store().await).await;
}

// ---------------------------------------------------------------------------
// Other port contracts
// ---------------------------------------------------------------------------

/// A signature from the port verifies under its own verifying key, and the
/// fingerprint is the domain fingerprint of that key.
pub fn signer_contract<S: Signer>(signer: &S) {
    let msg = b"oml-port-contract signer message";
    let sig = signer.sign(msg);
    let vk = signer.verifying_key();
    vk.verify(msg, &sig)
        .expect("the port's signature verifies under its verifying key");
    assert_eq!(signer.fingerprint(), key_fingerprint(&vk));
}

/// Load-or-create is stable: two loads return the same key.
pub async fn keyring_contract<K: KeyringStore>(keyring: &K) {
    let first = keyring.load_or_create().await.expect("first load");
    let second = keyring.load_or_create().await.expect("second load");
    assert_eq!(
        first.verifying_key(),
        second.verifying_key(),
        "load_or_create returns one stable key"
    );
}

/// The scan is deterministic: same input, same verdict (admission.md).
pub fn scanner_deterministic<Sc: Scanner>(scanner: &Sc, bodies: &[&str]) {
    let tenant_id = Uuid::from_u128(1);
    for body in bodies {
        let envelope = AdmissionEnvelope {
            tenant_id,
            kind: "memory",
            body,
        };
        assert_eq!(
            scanner.scan(&envelope),
            scanner.scan(&envelope),
            "same input, same verdict, for {body:?}"
        );
    }
}

/// The clock emits the fixed-width signed form (type-guaranteed) and the id
/// generator never repeats an id or a secret.
pub fn clock_and_idgen_contract<C: Clock, I: IdGen>(clock: &C, ids: &I) {
    let _ = clock.now(); // EventTime construction is itself the validity law
    let mut seen_ids = std::collections::BTreeSet::new();
    let mut seen_secrets = std::collections::BTreeSet::new();
    for _ in 0..100 {
        assert!(seen_ids.insert(ids.new_id()), "ids never repeat");
        assert!(
            seen_secrets.insert(ids.new_secret()),
            "secrets never repeat"
        );
    }
}

// ---------------------------------------------------------------------------
// The retriever laws (ADR-0049 d7): run against every adapter, for every
// retriever it serves, so a pipeline means the same thing on every store.
// ---------------------------------------------------------------------------

/// The fixture the retriever laws share: plain lower-case words only, so
/// the in-memory store's tokenizer and PostgreSQL's parser read them alike.
/// Returns (readable promoted ids, own held ids, the readable node, the
/// own key).
async fn retriever_fixture<S: JournalStore>(
    rig: &mut ChainRig,
    store: &S,
    with_outsiders: bool,
) -> (
    Vec<(Uuid, &'static str)>,
    Vec<(Uuid, &'static str)>,
    Uuid,
    Uuid,
) {
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let elsewhere = Uuid::from_u128(0xE3);
    let own = Uuid::from_u128(0xE4);
    let other = Uuid::from_u128(0xE5);
    let row = |id: u128, node: Uuid, key: Uuid, body: &str| {
        held_memory(tenant, node, Uuid::from_u128(id), "rita", key, body)
    };
    let promoted: Vec<(Uuid, &'static str)> = vec![
        (
            Uuid::from_u128(0xF01),
            "deploy the billing service on friday",
        ),
        (Uuid::from_u128(0xF02), "the billing service owns invoices"),
        (Uuid::from_u128(0xF03), "never deploy on friday evenings"),
        (Uuid::from_u128(0xF04), "invoices are numbered per tenant"),
        (
            Uuid::from_u128(0xF05),
            "rollback the release with one command",
        ),
    ];
    for (id, body) in &promoted {
        hold_and_promote(rig, store, row(id.as_u128(), wa, other, body)).await;
    }
    let held: Vec<(Uuid, &'static str)> =
        vec![(Uuid::from_u128(0xF11), "billing deploy notes held")];
    for (id, body) in &held {
        hold(rig, store, row(id.as_u128(), wa, own, body))
            .await
            .expect("hold own");
    }
    hold(
        rig,
        store,
        row(0xF12, wa, other, "billing deploy held by another key"),
    )
    .await
    .expect("hold another's");
    hold_and_promote(
        rig,
        store,
        MemoryRecord {
            kind: MemoryKind::Steering,
            ..row(0xF13, wa, other, "billing deploy steering")
        },
    )
    .await;
    if with_outsiders {
        for (i, body) in [
            "billing billing billing deploy deploy",
            "friday friday friday invoices",
            "service service rollback release",
            "deploy billing",
        ]
        .iter()
        .enumerate()
        {
            hold_and_promote(rig, store, row(0xF20 + i as u128, elsewhere, other, body)).await;
        }
    }
    (promoted, held, wa, own)
}

fn scope_of(wa: Uuid, own: Option<Uuid>) -> CandidateScope {
    CandidateScope {
        anchor: oml_app::types::AnchorFilter::default(),
        promoted_nodes: vec![wa],
        kinds: HIT_KINDS.to_vec(),
        own_held: own.map(|key| OwnHeldScope {
            any_author: false,
            author_key_ids: vec![key],
            nodes: vec![wa],
        }),
        expired_at: None,
    }
}

async fn run<S: Retriever>(
    store: &S,
    tenant: Uuid,
    kind: RetrieverKind,
    scope: &CandidateScope,
    query: &str,
    match_mode: MatchMode,
) -> Retrieved {
    // A sparse list reads what the encoder wrote, so every waiting version
    // is encoded first, by the deterministic double (ADR-0052 d16).
    let sparse = if kind == RetrieverKind::Sparse {
        encode_sparse_backlog(store).await;
        oml_app::ports::SparseEncoder::encode_query(&FakeSparseEncoder::new(), query)
    } else {
        Vec::new()
    };
    store
        .retrieve(
            tenant,
            kind,
            scope,
            RetrievalQuery {
                query,
                match_mode,
                k: 50,
                sparse: &sparse,
                window: None,
                within: &[],
            },
        )
        .await
        .expect("retrieve")
}

/// Turns every tenant's sparse queue on and writes every waiting
/// version's kind 2 postings with [`FakeSparseEncoder`], through the
/// encoder's own pass, as the lead's task does; returns how many it wrote
/// (ADR-0052 d15).
pub async fn encode_sparse_backlog<S: Retriever>(store: &S) -> u64 {
    let mut written = 0;
    loop {
        let pass = oml_app::app::encode_sparse_pass(store, &FakeSparseEncoder::new(), 256)
            .await
            .expect("sparse pass");
        written += pass.written;
        if !pass.busy() {
            return written;
        }
    }
}

const LAW_QUERIES: [&str; 6] = [
    "billing deploy",
    "friday",
    "invoices tenant",
    "rollback zebra",
    "zebra quartz",
    "deploy the billing service on friday",
];

/// No retriever returns a row outside the scope the core hands it: never
/// another node's row, another key's held row, a steering row, or a held
/// row when the scope names no own held keys, and an empty scope ranks
/// nothing (ADR-0049 d2; ADR-0025 d1, d4, d8).
pub async fn a_retriever_never_returns_a_row_outside_the_scope<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(401);
    rig.seed_chain(store, 0).await;
    let (promoted, held, wa, own) = retriever_fixture(&mut rig, store, true).await;
    let promoted_ids: Vec<Uuid> = promoted.iter().map(|(id, _)| *id).collect();
    let held_ids: Vec<Uuid> = held.iter().map(|(id, _)| *id).collect();
    for kind in store.retrievers() {
        for query in LAW_QUERIES {
            for mode in [MatchMode::Any, MatchMode::All] {
                let without =
                    run(store, rig.tenant_id, kind, &scope_of(wa, None), query, mode).await;
                for c in &without.candidates {
                    assert!(
                        promoted_ids.contains(&c.memory_id),
                        "{kind:?} {query:?}: {} is outside the promoted scope",
                        c.memory_id
                    );
                }
                let with = run(
                    store,
                    rig.tenant_id,
                    kind,
                    &scope_of(wa, Some(own)),
                    query,
                    mode,
                )
                .await;
                for c in &with.candidates {
                    assert!(
                        promoted_ids.contains(&c.memory_id) || held_ids.contains(&c.memory_id),
                        "{kind:?} {query:?}: {} is outside the scope with own held rows",
                        c.memory_id
                    );
                }
                let empty = CandidateScope {
                    anchor: oml_app::types::AnchorFilter::default(),
                    promoted_nodes: Vec::new(),
                    kinds: HIT_KINDS.to_vec(),
                    own_held: None,
                    expired_at: None,
                };
                assert!(
                    run(store, rig.tenant_id, kind, &empty, query, mode)
                        .await
                        .candidates
                        .is_empty(),
                    "{kind:?}: an empty scope ranks nothing"
                );
            }
        }
    }
}

/// Every retriever answers equal input with equal output, in the total
/// order of hits: score descending, then id ascending (ADR-0049 d7).
pub async fn a_retriever_orders_deterministically_by_score_then_id<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(402);
    rig.seed_chain(store, 0).await;
    let (_, _, wa, own) = retriever_fixture(&mut rig, store, false).await;
    for kind in store.retrievers() {
        for query in LAW_QUERIES {
            let scope = scope_of(wa, Some(own));
            let first = run(store, rig.tenant_id, kind, &scope, query, MatchMode::Any).await;
            let again = run(store, rig.tenant_id, kind, &scope, query, MatchMode::Any).await;
            assert_eq!(
                first, again,
                "{kind:?} {query:?}: equal input, equal output"
            );
            for pair in first.candidates.windows(2) {
                let ordered = pair[0].score > pair[1].score
                    || (pair[0].score.total_cmp(&pair[1].score).is_eq()
                        && pair[0].memory_id < pair[1].memory_id);
                assert!(ordered, "{kind:?} {query:?}: {pair:?} out of order");
            }
        }
    }
}

/// No row outside the scope changes a candidate's score or the list: the
/// crowded store adds, beside the same scope rows, every class of row a
/// statistic could leak from: another key's held row, the calling key's own
/// held row when the scope names no own keys, a rejected version, a
/// replaced version, a retired claim's versions, a steering row, rows on
/// another node, and another tenant's rows on the same node id in the same
/// database. Each class
/// carries the queries' words, so a document frequency, a row count or an
/// average length taken over it moves a score (ADR-0048 d3; ADR-0049 d2,
/// d12).
pub async fn a_row_outside_the_scope_never_changes_a_candidates_score<S, F, Fut>(new_store: F)
where
    S: JournalStore,
    F: Fn() -> Fut,
    Fut: Future<Output = S>,
{
    let alone = new_store().await;
    let mut rig = ChainRig::new(403);
    rig.seed_chain(&alone, 0).await;
    let (_, _, wa, own) = retriever_fixture(&mut rig, &alone, false).await;
    let tenant = rig.tenant_id;
    // The successor that is a served tip in every store.
    hold_and_promote(
        &mut rig,
        &alone,
        held_memory(
            tenant,
            wa,
            Uuid::from_u128(0xF83),
            "rita",
            Uuid::from_u128(0xE5),
            SUCCESSOR,
        ),
    )
    .await;
    // With no own keys named, the crowded store also holds the key's own
    // held row; with them named, that row would be in scope, so it is left
    // out and another key's held row must stay out instead.
    for (own_held, own_extra) in [(None, true), (Some(own), false)] {
        let crowded = new_store().await;
        let t = crowd(&crowded, wa, own, own_extra).await;
        let scope = scope_of(wa, own_held);
        for kind in alone.retrievers() {
            for query in LAW_QUERIES {
                let a = run(&alone, tenant, kind, &scope, query, MatchMode::Any).await;
                let b = run(&crowded, t, kind, &scope, query, MatchMode::Any).await;
                assert_eq!(
                    a, b,
                    "{kind:?} {query:?} own {own_held:?}: rows outside the scope moved a score"
                );
            }
        }
    }
}

/// The words every leak class of [`crowd`] says: those of the law queries.
const LEAKY: &str = "billing deploy friday invoices tenant service rollback release";
/// The text of the served tip that replaces [`crowd`]'s replaced version.
const SUCCESSOR: &str = "rollback the release on monday";

/// The retriever fixture with every class of row outside `scope_of(wa, _)`
/// added, each saying [`LEAKY`]; the tenant of its rows.
async fn crowd<S: JournalStore>(store: &S, wa: Uuid, own: Uuid, own_extra: bool) -> Uuid {
    let mut rig = ChainRig::new(403);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let _ = retriever_fixture(&mut rig, store, true).await;
    let other = Uuid::from_u128(0xE5);
    let row = |tenant: Uuid, id: u128, key: Uuid, body: &str| {
        held_memory(tenant, wa, Uuid::from_u128(id), "rita", key, body)
    };
    hold(&mut rig, store, row(t, 0xF81, other, LEAKY))
        .await
        .expect("another key's held row");
    if own_extra {
        hold(&mut rig, store, row(t, 0xF87, own, LEAKY))
            .await
            .expect("the key's own held row");
    }
    let rejected = row(t, 0xF88, other, LEAKY);
    hold(&mut rig, store, rejected)
        .await
        .expect("hold to reject");
    reject(&mut rig, store, Uuid::from_u128(0xF88))
        .await
        .expect("a rejected row");
    let replaced = row(t, 0xF82, other, LEAKY);
    hold_and_promote(&mut rig, store, replaced.clone()).await;
    hold_and_ratify(
        &mut rig,
        store,
        successor_of(&replaced, Uuid::from_u128(0xF83), SUCCESSOR, false),
    )
    .await;
    let retired = row(t, 0xF84, other, LEAKY);
    hold_and_promote(&mut rig, store, retired.clone()).await;
    hold_and_ratify(
        &mut rig,
        store,
        successor_of(&retired, Uuid::from_u128(0xF85), LEAKY, true),
    )
    .await;
    hold_and_promote(
        &mut rig,
        store,
        MemoryRecord {
            kind: MemoryKind::Steering,
            ..row(t, 0xF86, other, LEAKY)
        },
    )
    .await;
    let mut neighbour = ChainRig::new(412);
    neighbour.seed_chain(store, 0).await;
    let theirs = neighbour.tenant_id;
    for i in 0..4_u128 {
        hold_and_promote(&mut neighbour, store, row(theirs, 0xF90 + i, other, LEAKY)).await;
    }
    t
}

/// The keyword retrievers mean the same on every store (ADR-0049 d7):
/// `keyword_all` keeps a row holding every query word, `keyword_any` a row
/// holding any, `match: all` narrows `keyword_any` to every word, and
/// `matched` names the words a row holds. Independent expected sets, so
/// two stores passing this law agree with each other.
pub async fn the_keyword_retrievers_match_all_or_any_word_on_every_store<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(405);
    rig.seed_chain(store, 0).await;
    let (_, _, wa, own) = retriever_fixture(&mut rig, store, false).await;
    let ids = |list: &Retrieved| {
        let mut v: Vec<u128> = list
            .candidates
            .iter()
            .map(|c| c.memory_id.as_u128())
            .collect();
        v.sort_unstable();
        v
    };
    let scope = scope_of(wa, Some(own));
    let cases: [(&str, Vec<u128>, Vec<u128>); 5] = [
        (
            "billing deploy",
            vec![0xF01, 0xF11],
            vec![0xF01, 0xF02, 0xF03, 0xF11],
        ),
        ("friday", vec![0xF01, 0xF03], vec![0xF01, 0xF03]),
        ("invoices tenant", vec![0xF04], vec![0xF02, 0xF04]),
        ("rollback zebra", vec![], vec![0xF05]),
        ("zebra quartz", vec![], vec![]),
    ];
    for (query, all, any) in cases {
        let got_all = run(
            store,
            rig.tenant_id,
            RetrieverKind::KeywordAll,
            &scope,
            query,
            MatchMode::Any,
        )
        .await;
        assert_eq!(ids(&got_all), all, "keyword_all {query:?}");
        let got_any = run(
            store,
            rig.tenant_id,
            RetrieverKind::KeywordAny,
            &scope,
            query,
            MatchMode::Any,
        )
        .await;
        assert_eq!(ids(&got_any), any, "keyword_any {query:?}");
        let narrowed = run(
            store,
            rig.tenant_id,
            RetrieverKind::KeywordAny,
            &scope,
            query,
            MatchMode::All,
        )
        .await;
        assert_eq!(ids(&narrowed), all, "keyword_any under match all {query:?}");
        let mut terms: Vec<String> = query.split(' ').map(ToOwned::to_owned).collect();
        terms.sort();
        assert_eq!(got_any.terms, terms, "terms of {query:?}");
        for c in &got_any.candidates {
            assert!(
                !c.matched.is_empty(),
                "an any-word hit names a matched word"
            );
            for word in &c.matched {
                assert!(terms.contains(word), "{word} is a query term");
            }
        }
    }
    // A row holding more of the words ranks above one holding fewer.
    let ranked = run(
        store,
        rig.tenant_id,
        RetrieverKind::KeywordAny,
        &scope,
        "deploy billing friday",
        MatchMode::Any,
    )
    .await;
    assert_eq!(
        ranked.candidates.first().map(|c| c.memory_id.as_u128()),
        Some(0xF01),
        "the row with all three words ranks first"
    );
}

/// The bm25 retriever finds an identifier by its parts and a word by its
/// stem, where the store serves it (ADR-0048 d3; ADR-0049 d7).
pub async fn bm25_finds_an_identifier_by_its_parts_and_a_word_by_its_stem<S: JournalStore>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Bm25) {
        return;
    }
    let mut rig = ChainRig::new(406);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xE2);
    let row = |id: u128, body: &str| {
        held_memory(
            rig_tenant(406),
            wa,
            Uuid::from_u128(id),
            "rita",
            Uuid::from_u128(0xE5),
            body,
        )
    };
    hold_and_promote(
        &mut rig,
        store,
        row(0xF31, "the hook lives in useInvoiceFilters.ts"),
    )
    .await;
    hold_and_promote(&mut rig, store, row(0xF32, "we deployed twice on monday")).await;
    hold_and_promote(&mut rig, store, row(0xF33, "unrelated note about lunch")).await;
    let scope = scope_of(wa, None);
    for (query, expected) in [("invoice filters", 0xF31_u128), ("deployments", 0xF32)] {
        let got = run(
            store,
            rig.tenant_id,
            RetrieverKind::Bm25,
            &scope,
            query,
            MatchMode::Any,
        )
        .await;
        assert_eq!(
            got.candidates.first().map(|c| c.memory_id.as_u128()),
            Some(expected),
            "bm25 {query:?}: {got:?}"
        );
    }
}

/// The trigram retriever finds a misspelled word's memory, where the store
/// serves it, and returns nothing under `match: all` (ADR-0048 d4).
pub async fn trigram_finds_a_misspelled_words_memory<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Trigram) {
        return;
    }
    let mut rig = ChainRig::new(407);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xE2);
    let row = |id: u128, body: &str| {
        held_memory(
            rig_tenant(407),
            wa,
            Uuid::from_u128(id),
            "rita",
            Uuid::from_u128(0xE5),
            body,
        )
    };
    hold_and_promote(
        &mut rig,
        store,
        row(0xF41, "pgbouncer runs in transaction mode"),
    )
    .await;
    hold_and_promote(&mut rig, store, row(0xF42, "unrelated note about lunch")).await;
    let scope = scope_of(wa, None);
    let got = run(
        store,
        rig.tenant_id,
        RetrieverKind::Trigram,
        &scope,
        "pgbouncr",
        MatchMode::Any,
    )
    .await;
    assert_eq!(
        got.candidates.first().map(|c| c.memory_id.as_u128()),
        Some(0xF41),
        "trigram: {got:?}"
    );
    let all = run(
        store,
        rig.tenant_id,
        RetrieverKind::Trigram,
        &scope,
        "pgbouncr",
        MatchMode::All,
    )
    .await;
    assert!(
        all.candidates.is_empty(),
        "trigram serves no list under match all"
    );
    let suggested = store
        .suggest(rig.tenant_id, &scope, &["pgbouncr".to_owned()], 3)
        .await
        .expect("suggest");
    assert_eq!(suggested, vec!["pgbouncer".to_owned()], "did you mean");
    let outside = store
        .suggest(
            rig.tenant_id,
            &scope_of(Uuid::from_u128(0xE3), None),
            &["pgbouncr".to_owned()],
            3,
        )
        .await
        .expect("suggest");
    assert!(
        outside.is_empty(),
        "did you mean offers only words of rows in the scope"
    );
}

/// Every retriever law, against a fresh store per law.
/// BM25 reads a query without its English stop words when it has other
/// words, and with them when it has none: `the billing` ranks as `billing`
/// and reports the one term, while `the` alone still finds the rows that
/// say it (ADR-0049 d12).
pub async fn bm25_reads_a_query_without_its_english_stop_words_unless_it_has_no_other<
    S: JournalStore,
>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Bm25) {
        return;
    }
    let mut rig = ChainRig::new(408);
    rig.seed_chain(store, 0).await;
    let (promoted, _, wa, _) = retriever_fixture(&mut rig, store, false).await;
    let scope = scope_of(wa, None);
    let ids = |r: &Retrieved| -> Vec<(Uuid, f32)> {
        r.candidates
            .iter()
            .map(|c| (c.memory_id, c.score))
            .collect()
    };
    let bare = run(
        store,
        rig.tenant_id,
        RetrieverKind::Bm25,
        &scope,
        "billing",
        MatchMode::Any,
    )
    .await;
    let with_stop = run(
        store,
        rig.tenant_id,
        RetrieverKind::Bm25,
        &scope,
        "the billing",
        MatchMode::Any,
    )
    .await;
    assert_eq!(ids(&with_stop), ids(&bare), "a stop word moves no rank");
    assert_eq!(with_stop.terms, vec!["billing".to_owned()], "{with_stop:?}");
    let all = run(
        store,
        rig.tenant_id,
        RetrieverKind::Bm25,
        &scope,
        "the billing",
        MatchMode::All,
    )
    .await;
    assert_eq!(ids(&all), ids(&bare), "every word but the stop words");
    let only_stop = run(
        store,
        rig.tenant_id,
        RetrieverKind::Bm25,
        &scope,
        "the",
        MatchMode::Any,
    )
    .await;
    let saying_the: Vec<Uuid> = promoted
        .iter()
        .filter(|(_, body)| body.split(' ').any(|w| w == "the"))
        .map(|(id, _)| *id)
        .collect();
    let mut found: Vec<Uuid> = only_stop.candidates.iter().map(|c| c.memory_id).collect();
    found.sort();
    assert_eq!(found, saying_the, "a query of stop words alone keeps them");
}

/// BM25 counts what a row says once: a lone row that says a word once, in
/// a form whose English stem is itself, scores that word at exactly its
/// BM25 weight with k1 1.2 and b 0.75, `ln(1 + (1 - 1 + 0.5) / (1 + 0.5))`,
/// however the text was normalised for identifiers and stems (ADR-0049 d12;
/// eval/bench/REPORT.md).
pub async fn bm25_scores_a_word_a_lone_row_says_once_at_its_weight<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Bm25) {
        return;
    }
    let mut rig = ChainRig::new(409);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xE2);
    hold_and_promote(
        &mut rig,
        store,
        held_memory(
            rig_tenant(409),
            wa,
            Uuid::from_u128(0xF51),
            "rita",
            Uuid::from_u128(0xE5),
            "deploy canary",
        ),
    )
    .await;
    let got = run(
        store,
        rig.tenant_id,
        RetrieverKind::Bm25,
        &scope_of(wa, None),
        "deploy",
        MatchMode::Any,
    )
    .await;
    let weight = (1.0_f64 + 0.5 / 1.5).ln();
    assert_eq!(got.candidates.len(), 1, "{got:?}");
    assert!(
        (f64::from(got.candidates[0].score) - weight).abs() < 1e-4,
        "score {} against {weight}",
        got.candidates[0].score
    );
}

/// BM25 counts an inflected form once, as it counts the exact word: a lone
/// row that says `deploying` once scores `deploy` at exactly its BM25
/// weight, and of two rows that say `runs` and `run`, the query `run` ranks
/// the exact word's row first, never the inflected one above it (ADR-0049
/// d12).
pub async fn bm25_counts_an_inflected_form_once_as_it_counts_the_exact_word<S: JournalStore>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Bm25) {
        return;
    }
    let mut rig = ChainRig::new(411);
    rig.seed_chain(store, 0).await;
    let (lone, pair) = (Uuid::from_u128(0xE2), Uuid::from_u128(0xE3));
    let row = |id: u128, node: Uuid, body: &str| {
        held_memory(
            rig_tenant(411),
            node,
            Uuid::from_u128(id),
            "rita",
            Uuid::from_u128(0xE5),
            body,
        )
    };
    hold_and_promote(&mut rig, store, row(0xF71, lone, "deploying canary")).await;
    let got = run(
        store,
        rig.tenant_id,
        RetrieverKind::Bm25,
        &scope_of(lone, None),
        "deploy",
        MatchMode::Any,
    )
    .await;
    let weight = (1.0_f64 + 0.5 / 1.5).ln();
    assert_eq!(got.candidates.len(), 1, "{got:?}");
    assert!(
        (f64::from(got.candidates[0].score) - weight).abs() < 1e-4,
        "deploying scores deploy at {} against {weight}",
        got.candidates[0].score
    );
    hold_and_promote(&mut rig, store, row(0xF72, pair, "the dog runs fast")).await;
    hold_and_promote(&mut rig, store, row(0xF73, pair, "the dog run fast")).await;
    let got = run(
        store,
        rig.tenant_id,
        RetrieverKind::Bm25,
        &scope_of(pair, None),
        "run",
        MatchMode::Any,
    )
    .await;
    assert_eq!(
        got.candidates
            .iter()
            .map(|c| c.memory_id.as_u128())
            .collect::<Vec<_>>(),
        vec![0xF73, 0xF72],
        "the exact word's row first: {got:?}"
    );
}

/// `did_you_mean` draws only on the words of promoted served tips of the
/// kinds recall serves, in the scope: never a held row's, another node's, a
/// steering row's, another tenant's on the same node id, a replaced
/// version's or a retired claim's, and a word returns once a served tip says
/// it again
/// (ADR-0049 d6, d12).
pub async fn did_you_mean_draws_only_on_the_words_of_promoted_served_tips_in_the_scope<
    S: JournalStore,
>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Trigram) {
        return;
    }
    let mut rig = ChainRig::new(410);
    rig.seed_chain(store, 0).await;
    let (wa, elsewhere) = (Uuid::from_u128(0xE2), Uuid::from_u128(0xE3));
    let row = |id: u128, node: Uuid, body: &str| {
        held_memory(
            rig_tenant(410),
            node,
            Uuid::from_u128(id),
            "rita",
            Uuid::from_u128(0xE5),
            body,
        )
    };
    hold_and_promote(
        &mut rig,
        store,
        row(0xF61, wa, "pgbouncer pools connections"),
    )
    .await;
    hold(&mut rig, store, row(0xF62, wa, "kubernetes ingress"))
        .await
        .expect("hold");
    hold_and_promote(&mut rig, store, row(0xF63, elsewhere, "zookeeper quorum")).await;
    let cache = row(0xF64, wa, "memcached fronts the database");
    hold_and_promote(&mut rig, store, cache.clone()).await;
    let retired = row(0xF65, wa, "terraform plans nightly");
    hold_and_promote(&mut rig, store, retired.clone()).await;
    let scope = scope_of(wa, None);
    let tenant = rig.tenant_id;
    let suggest = |words: &[&str]| {
        let words: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
        let scope = scope.clone();
        async move {
            store
                .suggest(tenant, &scope, &words, 3)
                .await
                .expect("suggest")
        }
    };
    assert_eq!(suggest(&["pgbouncr"]).await, vec!["pgbouncer".to_owned()]);
    assert_eq!(suggest(&["memcachd"]).await, vec!["memcached".to_owned()]);
    assert_eq!(suggest(&["terraforn"]).await, vec!["terraform".to_owned()]);
    assert!(
        suggest(&["kubernets"]).await.is_empty(),
        "a held row's word"
    );
    assert!(
        suggest(&["zookeper"]).await.is_empty(),
        "another node's word"
    );
    hold_and_promote(
        &mut rig,
        store,
        MemoryRecord {
            kind: MemoryKind::Steering,
            ..row(0xF69, wa, "prometheus scrapes every minute")
        },
    )
    .await;
    assert!(
        suggest(&["prometeus"]).await.is_empty(),
        "a kind recall never serves"
    );
    let mut neighbour = ChainRig::new(413);
    neighbour.seed_chain(store, 0).await;
    let theirs = neighbour.tenant_id;
    hold_and_promote(
        &mut neighbour,
        store,
        held_memory(
            theirs,
            wa,
            Uuid::from_u128(0xF6A),
            "rita",
            Uuid::from_u128(0xE5),
            "cassandra replicates across racks",
        ),
    )
    .await;
    assert!(
        suggest(&["casandra"]).await.is_empty(),
        "another tenant's word on the same node id"
    );

    hold_and_ratify(
        &mut rig,
        store,
        successor_of(
            &cache,
            Uuid::from_u128(0xF66),
            "valkey fronts the database",
            false,
        ),
    )
    .await;
    hold_and_ratify(
        &mut rig,
        store,
        successor_of(
            &retired,
            Uuid::from_u128(0xF67),
            "terraform plans nightly",
            true,
        ),
    )
    .await;
    assert!(
        suggest(&["memcachd"]).await.is_empty(),
        "a replaced version's word"
    );
    assert_eq!(suggest(&["valkee"]).await, vec!["valkey".to_owned()]);
    assert!(
        suggest(&["terraforn"]).await.is_empty(),
        "a retired claim's word"
    );
    hold_and_promote(
        &mut rig,
        store,
        row(0xF68, wa, "memcached again, for sessions"),
    )
    .await;
    assert_eq!(
        suggest(&["memcachd"]).await,
        vec!["memcached".to_owned()],
        "a word returns with a served tip that says it"
    );
}

// ---------------------------------------------------------------------------
// The postings laws (ADR-0052 d12): `bm25_postings` ranks as `bm25` does, and
// what it prunes depends on the readable rows alone. Each runs where the
// store serves `bm25_postings`; the caller hands a store pruning by the
// limits the law's doc names.
// ---------------------------------------------------------------------------

/// The postings fixture on node 0xE2: ten promoted memories in which
/// `billing` (5 rows) and `deploy` with its inflections (4 rows) are
/// frequent at a share of 0.3, `friday` (2 rows) is not, a promoted derived
/// fact, and an own held row of key 0xE4; returns (node, own key).
async fn postings_fixture<S: JournalStore>(rig: &mut ChainRig, store: &S) -> (Uuid, Uuid) {
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let own = Uuid::from_u128(0xE4);
    let other = Uuid::from_u128(0xE5);
    let row = |id: u128, key: Uuid, body: &str| {
        held_memory(tenant, wa, Uuid::from_u128(id), "rita", key, body)
    };
    for (id, body) in [
        (0xF01, "deploy the billing service on friday"),
        (0xF02, "the billing service owns invoices"),
        (0xF03, "never deploy on friday evenings"),
        (0xF04, "invoices are numbered per tenant"),
        (0xF05, "rollback the release with one command"),
        (0xF06, "billing runs nightly and billing retries twice"),
        (0xF07, "the billing ledger closes at midnight"),
        (0xF08, "deploys are frozen during the billing audit"),
        (0xF09, "useInvoiceFilters reads the invoice filters"),
        (0xF0A, "a canary deploy precedes the rollout"),
    ] {
        hold_and_promote(rig, store, row(id, other, body)).await;
    }
    hold_and_promote(
        rig,
        store,
        MemoryRecord {
            kind: MemoryKind::DerivedFact,
            ..row(
                0xF0B,
                other,
                "a derived fact: the billing audit runs on friday",
            )
        },
    )
    .await;
    hold(
        rig,
        store,
        row(0xF11, own, "billing deploy notes held friday"),
    )
    .await
    .expect("hold own");
    (wa, own)
}

/// The queries the postings laws ask: frequent words alone, frequent words
/// beside rare ones, inflections, identifiers, stop words and absent words.
const POSTINGS_QUERIES: [&str; 13] = [
    "billing",
    "invoices tenant",
    "billing friday",
    "deploy billing",
    "deploying on friday",
    "friday",
    "invoice filters",
    "useInvoiceFilters",
    "the",
    "zebra",
    "billing zebra",
    "rollback the release on friday",
    "canary deploys during the audit",
];

fn same_list(a: &Retrieved, b: &Retrieved, what: &str) {
    assert_eq!(
        a.candidates
            .iter()
            .map(|c| (c.memory_id, c.matched.clone()))
            .collect::<Vec<_>>(),
        b.candidates
            .iter()
            .map(|c| (c.memory_id, c.matched.clone()))
            .collect::<Vec<_>>(),
        "{what}: the same rows in the same order, matching the same words"
    );
    for (x, y) in a.candidates.iter().zip(&b.candidates) {
        assert!(
            (x.score - y.score).abs() <= 1e-5 * y.score.abs().max(1e-6),
            "{what}: {} scores {} against {}",
            x.memory_id,
            x.score,
            y.score
        );
    }
}

/// With nothing pruned, `bm25_postings` returns `bm25`'s list: the same ids
/// in the same order, scores equal to 1e-5 relative, the same matched
/// words, for every query, every `k`, either `match`, every set of hit
/// kinds and with and without own held rows, read from postings (ADR-0052
/// d12). The store prunes
/// nothing (`PruneLimits::NONE` on PostgreSQL).
pub async fn bm25_postings_ranks_as_bm25_does_when_nothing_is_pruned<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Bm25Postings) {
        return;
    }
    let mut rig = ChainRig::new(421);
    rig.seed_chain(store, 0).await;
    let (wa, own) = postings_fixture(&mut rig, store).await;
    let kind_sets = [
        HIT_KINDS.to_vec(),
        vec![MemoryKind::Memory],
        vec![MemoryKind::DerivedFact],
    ];
    for query in POSTINGS_QUERIES {
        for own_held in [None, Some(own)] {
            for (mode, kinds) in [MatchMode::Any, MatchMode::All]
                .into_iter()
                .flat_map(|m| kind_sets.iter().map(move |k| (m, k)))
            {
                for k in 1..=12 {
                    let scope = CandidateScope {
                        anchor: oml_app::types::AnchorFilter::default(),
                        kinds: kinds.clone(),
                        ..scope_of(wa, own_held)
                    };
                    let ask = |kind| {
                        store.retrieve(
                            rig.tenant_id,
                            kind,
                            &scope,
                            RetrievalQuery {
                                query,
                                match_mode: mode,
                                k,
                                sparse: &[],
                                window: None,
                                within: &[],
                            },
                        )
                    };
                    let scan = ask(RetrieverKind::Bm25).await.expect("bm25");
                    let postings = ask(RetrieverKind::Bm25Postings).await.expect("postings");
                    let what = format!("{query:?} own {own_held:?} {mode:?} {kinds:?} k {k}");
                    same_list(&postings, &scan, &what);
                    assert_eq!(postings.terms, scan.terms, "{what}: the same terms");
                    assert_eq!(
                        postings.access,
                        Access::Postings,
                        "{what}: read from postings"
                    );
                    assert!(postings.pruned.is_empty(), "{what}: nothing pruned");
                }
            }
        }
    }
}

/// An expired version counts in no statistic: `bm25` and `bm25-postings`
/// rank the rows a scope keeps over the rows it keeps alone, so with
/// readable versions ended both pipelines answer the same list, and the same
/// list as a store in which those versions were never written; with
/// `include_expired` both count them again (proposed ADR-0059 d3).
pub async fn bm25_postings_ranks_as_bm25_does_when_a_readable_row_has_expired<S: JournalStore>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Bm25Postings) {
        return;
    }
    let mut rig = ChainRig::new(435);
    rig.seed_chain(store, 0).await;
    let (wa, own) = postings_fixture(&mut rig, store).await;
    let tenant = rig.tenant_id;
    let other = Uuid::from_u128(0xE5);
    for (id, body) in [
        (0xF31_u128, "billing billing billing friday deploy"),
        (0xF32, "invoices billing ledger reconciled monthly"),
    ] {
        let mut m = held_memory(tenant, wa, Uuid::from_u128(id), "rita", other, body);
        *m.valid = oml_app::domain::valid_time::ValidTime::parse(None, Some("2020")).expect("ends");
        hold_and_promote(&mut rig, store, m).await;
    }
    let now = oml_app::domain::valid_time::parse_now("2026-11-05").expect("now");
    for query in POSTINGS_QUERIES {
        for (own_held, expired_at) in [(None, Some(now)), (Some(own), Some(now)), (None, None)] {
            let scope = CandidateScope {
                expired_at,
                ..scope_of(wa, own_held)
            };
            let ask = |kind| {
                store.retrieve(
                    tenant,
                    kind,
                    &scope,
                    RetrievalQuery {
                        query,
                        match_mode: MatchMode::Any,
                        k: 12,
                        sparse: &[],
                        window: None,
                        within: &[],
                    },
                )
            };
            let scan = ask(RetrieverKind::Bm25).await.expect("bm25");
            let postings = ask(RetrieverKind::Bm25Postings).await.expect("postings");
            same_list(
                &postings,
                &scan,
                &format!("{query:?} own {own_held:?} expired_at {expired_at:?}"),
            );
        }
    }
}

/// A pruned list scores each of its rows as `bm25` does: a frequent word is
/// left out of the postings read, yet every row the others reach is scored
/// with it, so each returned row's score is its full `bm25` score; the
/// answer names the pruned word, and a row holding only it is not a
/// candidate (ADR-0052 d4, d6, d12). The store prunes forms above a share of
/// 0.3 of the readable rows once the query's readable postings pass a budget
/// of 5.
pub async fn a_pruned_list_scores_each_of_its_rows_as_bm25_does<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Bm25Postings) {
        return;
    }
    let mut rig = ChainRig::new(422);
    rig.seed_chain(store, 0).await;
    let (wa, _) = postings_fixture(&mut rig, store).await;
    let scope = scope_of(wa, None);
    let mut pruned_any = false;
    for query in POSTINGS_QUERIES {
        let full = run(
            store,
            rig.tenant_id,
            RetrieverKind::Bm25,
            &scope,
            query,
            MatchMode::Any,
        )
        .await;
        let got = run(
            store,
            rig.tenant_id,
            RetrieverKind::Bm25Postings,
            &scope,
            query,
            MatchMode::Any,
        )
        .await;
        pruned_any |= !got.pruned.is_empty();
        for c in &got.candidates {
            let whole = full
                .candidates
                .iter()
                .find(|f| f.memory_id == c.memory_id)
                .unwrap_or_else(|| panic!("{query:?}: {} is no bm25 candidate", c.memory_id));
            assert!(
                (c.score - whole.score).abs() <= 1e-5 * whole.score.abs().max(1e-6),
                "{query:?}: {} scores {} where bm25 scores it {}",
                c.memory_id,
                c.score,
                whole.score
            );
            assert_eq!(c.matched, whole.matched, "{query:?}: the words it holds");
        }
    }
    assert!(pruned_any, "the fixture prunes some word");
    let got = run(
        store,
        rig.tenant_id,
        RetrieverKind::Bm25Postings,
        &scope,
        "billing friday",
        MatchMode::Any,
    )
    .await;
    assert_eq!(got.access, Access::Postings);
    assert_eq!(got.pruned, vec!["billing".to_owned()], "billing is pruned");
    let ids: Vec<u128> = got
        .candidates
        .iter()
        .map(|c| c.memory_id.as_u128())
        .collect();
    let mut reached = ids.clone();
    reached.sort_unstable();
    assert_eq!(
        reached,
        vec![0xF01, 0xF03, 0xF0B],
        "the rows friday reaches; a row saying only billing is no candidate"
    );
    assert_eq!(
        ids.last(),
        Some(&0xF03),
        "the row without billing ranks last: billing is scored though pruned"
    );
}

/// Which words are pruned depends on the readable rows alone: rows the key
/// cannot read (another node's, another key's held row, a rejected one,
/// another tenant's) that make a word frequent tenant-wide change neither
/// the pruned words nor the list (ADR-0052 d6, d12). The store prunes forms
/// above a share of 0.3 of the readable rows once the query's readable
/// postings pass a budget of 5.
pub async fn which_words_are_pruned_depends_on_the_readable_rows_alone<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Bm25Postings) {
        return;
    }
    let mut rig = ChainRig::new(423);
    rig.seed_chain(store, 0).await;
    let (wa, own) = postings_fixture(&mut rig, store).await;
    let tenant = rig.tenant_id;
    let ask = |query: &'static str, own_held: Option<Uuid>| {
        let scope = scope_of(wa, own_held);
        async move {
            run(
                store,
                tenant,
                RetrieverKind::Bm25Postings,
                &scope,
                query,
                MatchMode::Any,
            )
            .await
        }
    };
    let queries = [
        "friday",
        "invoices tenant",
        "friday rollback",
        "billing friday",
    ];
    let mut before = Vec::new();
    for query in queries {
        for own_held in [None, Some(own)] {
            before.push(ask(query, own_held).await);
        }
    }
    let elsewhere = Uuid::from_u128(0xE3);
    let other = Uuid::from_u128(0xE5);
    let loud = "friday invoices tenant rollback friday";
    for i in 0..12_u128 {
        hold_and_promote(
            &mut rig,
            store,
            held_memory(
                tenant,
                elsewhere,
                Uuid::from_u128(0xFA0 + i),
                "rita",
                other,
                loud,
            ),
        )
        .await;
    }
    for i in 0..6_u128 {
        hold(
            &mut rig,
            store,
            held_memory(tenant, wa, Uuid::from_u128(0xFC0 + i), "rita", other, loud),
        )
        .await
        .expect("another key's held row");
    }
    hold(
        &mut rig,
        store,
        held_memory(tenant, wa, Uuid::from_u128(0xFD0), "rita", other, loud),
    )
    .await
    .expect("hold to reject");
    reject(&mut rig, store, Uuid::from_u128(0xFD0))
        .await
        .expect("reject");
    let mut neighbour = ChainRig::new(424);
    neighbour.seed_chain(store, 0).await;
    let theirs = neighbour.tenant_id;
    for i in 0..12_u128 {
        hold_and_promote(
            &mut neighbour,
            store,
            held_memory(theirs, wa, Uuid::from_u128(0xFE0 + i), "rita", other, loud),
        )
        .await;
    }
    let mut after = Vec::new();
    for query in queries {
        for own_held in [None, Some(own)] {
            after.push(ask(query, own_held).await);
        }
    }
    assert_eq!(
        before, after,
        "rows the key cannot read decide neither what is pruned nor how rows rank"
    );
}

/// A query whose every word is frequent among the readable rows is answered
/// by `bm25`'s scan, ranked as `bm25` ranks it, and says so with access
/// `scan` (ADR-0052 d6, d12). The store prunes forms above a share of 0.3 of
/// the readable rows once the query's readable postings pass a budget of 5.
pub async fn a_query_of_only_frequent_words_is_answered_by_the_scan_and_says_so<S: JournalStore>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Bm25Postings) {
        return;
    }
    let mut rig = ChainRig::new(425);
    rig.seed_chain(store, 0).await;
    let (wa, own) = postings_fixture(&mut rig, store).await;
    for query in ["billing", "deploy billing", "billing and the deploy"] {
        for own_held in [None, Some(own)] {
            let scope = scope_of(wa, own_held);
            let scan = run(
                store,
                rig.tenant_id,
                RetrieverKind::Bm25,
                &scope,
                query,
                MatchMode::Any,
            )
            .await;
            let got = run(
                store,
                rig.tenant_id,
                RetrieverKind::Bm25Postings,
                &scope,
                query,
                MatchMode::Any,
            )
            .await;
            assert_eq!(got.access, Access::Scan, "{query:?}: the scan answered");
            assert!(got.pruned.is_empty(), "{query:?}: the scan loses no word");
            assert!(!got.candidates.is_empty(), "{query:?}: rows hold it");
            same_list(&got, &scan, query);
        }
    }
}

/// A scope whose readable postings of every query form fit the budget is
/// read whole: nothing is pruned, however frequent a word, so a row holding
/// only frequent words is found and the list is `bm25`'s; frequent words
/// are left out only when reading them all would pass the budget (ADR-0052
/// d6, amended 2026-11-05). The store prunes forms above a share of 0.3 of
/// the readable rows, with a budget the fixture's postings fit.
pub async fn a_scope_whose_postings_fit_the_budget_is_read_whole<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Bm25Postings) {
        return;
    }
    let mut rig = ChainRig::new(427);
    rig.seed_chain(store, 0).await;
    let (wa, own) = postings_fixture(&mut rig, store).await;
    for own_held in [None, Some(own)] {
        let scope = scope_of(wa, own_held);
        for query in POSTINGS_QUERIES {
            let scan = run(
                store,
                rig.tenant_id,
                RetrieverKind::Bm25,
                &scope,
                query,
                MatchMode::Any,
            )
            .await;
            let got = run(
                store,
                rig.tenant_id,
                RetrieverKind::Bm25Postings,
                &scope,
                query,
                MatchMode::Any,
            )
            .await;
            assert_eq!(
                got.access,
                Access::Postings,
                "{query:?}: read from postings"
            );
            assert!(
                got.pruned.is_empty(),
                "{query:?}: nothing pruned: {:?}",
                got.pruned
            );
            same_list(&got, &scan, query);
        }
        let got = run(
            store,
            rig.tenant_id,
            RetrieverKind::Bm25Postings,
            &scope,
            "billing friday",
            MatchMode::Any,
        )
        .await;
        assert!(
            got.candidates
                .iter()
                .any(|c| c.memory_id.as_u128() == 0xF06),
            "a row saying only the frequent billing is found: {got:?}"
        );
    }
}

/// With pruning on, the list is `bm25`'s over the rows a word it read
/// reaches: every row `bm25` ranks that holds a word the answer does not
/// name in `pruned`, in `bm25`'s order with `bm25`'s scores, and only a row
/// holding nothing but named words is missing. So `pruned: []` means the
/// list is `bm25`'s (ADR-0052 d6, d11). The store prunes forms above a share
/// of 0.3 of the readable rows once the query's readable postings pass a
/// budget of 5.
pub async fn a_pruned_list_is_bm25s_over_the_rows_its_unpruned_words_reach<S: JournalStore>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Bm25Postings) {
        return;
    }
    let mut rig = ChainRig::new(428);
    rig.seed_chain(store, 0).await;
    let (wa, own) = postings_fixture(&mut rig, store).await;
    for own_held in [None, Some(own)] {
        let scope = scope_of(wa, own_held);
        for query in POSTINGS_QUERIES {
            for k in [1, 3, 10] {
                let ask = |kind, k| {
                    store.retrieve(
                        rig.tenant_id,
                        kind,
                        &scope,
                        RetrievalQuery {
                            query,
                            match_mode: MatchMode::Any,
                            k,
                            sparse: &[],
                            window: None,
                            within: &[],
                        },
                    )
                };
                let got = ask(RetrieverKind::Bm25Postings, k).await.expect("postings");
                let full = ask(RetrieverKind::Bm25, 1_000).await.expect("bm25");
                let reached: Vec<Candidate> = full
                    .candidates
                    .into_iter()
                    .filter(|c| c.matched.iter().any(|w| !got.pruned.contains(w)))
                    .take(k)
                    .collect();
                let want = Retrieved {
                    candidates: reached,
                    ..Retrieved::default()
                };
                same_list(
                    &got,
                    &want,
                    &format!("{query:?} own {own_held:?} k {k} pruned {:?}", got.pruned),
                );
            }
        }
    }
}

/// A word only unreadable rows hold is answered as a word no row holds: the
/// same list, the same access and nothing pruned, with no posting of it
/// read (ADR-0052 d2, d12). Any pruning limits.
pub async fn a_word_only_unreadable_rows_hold_is_answered_as_a_word_no_row_holds<
    S: JournalStore,
>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Bm25Postings) {
        return;
    }
    let mut rig = ChainRig::new(426);
    rig.seed_chain(store, 0).await;
    let (wa, own) = postings_fixture(&mut rig, store).await;
    let tenant = rig.tenant_id;
    let other = Uuid::from_u128(0xE5);
    hold_and_promote(
        &mut rig,
        store,
        held_memory(
            tenant,
            Uuid::from_u128(0xE3),
            Uuid::from_u128(0xFB0),
            "rita",
            other,
            "quartz quartz deploy",
        ),
    )
    .await;
    hold(
        &mut rig,
        store,
        held_memory(
            tenant,
            wa,
            Uuid::from_u128(0xFB1),
            "rita",
            other,
            "quartz notes",
        ),
    )
    .await
    .expect("another key's held row");
    for own_held in [None, Some(own)] {
        let scope = scope_of(wa, own_held);
        for (unreadable, absent) in [
            ("quartz", "xylophone"),
            ("friday quartz", "friday xylophone"),
        ] {
            let a = run(
                store,
                tenant,
                RetrieverKind::Bm25Postings,
                &scope,
                unreadable,
                MatchMode::Any,
            )
            .await;
            let b = run(
                store,
                tenant,
                RetrieverKind::Bm25Postings,
                &scope,
                absent,
                MatchMode::Any,
            )
            .await;
            same_list(&a, &b, unreadable);
            assert_eq!(
                (a.access, &a.pruned),
                (b.access, &b.pruned),
                "{unreadable:?}"
            );
        }
    }
}

/// A fixture row of the valid-time laws with its declared ends.
fn dated(mut m: MemoryRecord, from: Option<&str>, until: Option<&str>) -> MemoryRecord {
    *m.valid = oml_app::domain::valid_time::ValidTime::parse(from, until).expect("fixture ends");
    m
}

/// The `time` leg never returns a row outside the scope the core hands it:
/// another node's dated row, another key's held dated row, a dated steering
/// row, an own held row when no own keys are named; a row with no
/// `valid_from` never matches a window; an empty scope ranks nothing; and
/// its order is the rows that start inside the window first, then those
/// that started before it, nearest first, ties by id (proposed ADR-0059 d7).
pub async fn the_time_leg_never_returns_a_row_outside_the_scope<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(431);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let (wa, elsewhere) = (Uuid::from_u128(0xE2), Uuid::from_u128(0xE3));
    let (own, other) = (Uuid::from_u128(0xE4), Uuid::from_u128(0xE5));
    let row = |id: u128, node: Uuid, key: Uuid, body: &str| {
        held_memory(tenant, node, Uuid::from_u128(id), "rita", key, body)
    };
    let in_2015 = Uuid::from_u128(0x7001);
    let spans_2015 = Uuid::from_u128(0x7002);
    hold_and_promote(
        &mut rig,
        store,
        dated(
            row(0x7001, wa, other, "moved the office"),
            Some("2015-06"),
            Some("2015-06"),
        ),
    )
    .await;
    hold_and_promote(
        &mut rig,
        store,
        dated(
            row(0x7002, wa, other, "the office lease"),
            Some("2012"),
            Some("2019"),
        ),
    )
    .await;
    hold_and_promote(
        &mut rig,
        store,
        dated(row(0x7003, wa, other, "a later office"), Some("2017"), None),
    )
    .await;
    hold_and_promote(&mut rig, store, row(0x7004, wa, other, "an undated office")).await;
    hold_and_promote(
        &mut rig,
        store,
        dated(
            row(0x7005, elsewhere, other, "elsewhere office"),
            Some("2015"),
            None,
        ),
    )
    .await;
    hold_and_promote(
        &mut rig,
        store,
        MemoryRecord {
            kind: MemoryKind::Steering,
            ..dated(
                row(0x7006, wa, other, "steering office"),
                Some("2015"),
                None,
            )
        },
    )
    .await;
    let own_held = Uuid::from_u128(0x7007);
    hold(
        &mut rig,
        store,
        dated(row(0x7007, wa, own, "own held office"), Some("2015"), None),
    )
    .await
    .expect("hold own");
    hold(
        &mut rig,
        store,
        dated(
            row(0x7008, wa, other, "another key's held office"),
            Some("2015"),
            None,
        ),
    )
    .await
    .expect("hold another's");
    let window = oml_app::domain::valid_time::parse_window("in 2015", 0).expect("a window");
    // As if the text list had found every fixture row.
    let within: Vec<Uuid> = (0x7001..=0x7008_u128).map(Uuid::from_u128).collect();
    let leg = |scope: CandidateScope| {
        let window = window.clone();
        let within = within.clone();
        async move {
            store
                .retrieve(
                    tenant,
                    RetrieverKind::Time,
                    &scope,
                    RetrievalQuery {
                        query: "in 2015",
                        match_mode: MatchMode::Any,
                        k: 50,
                        sparse: &[],
                        window: Some(&window),
                        within: &within,
                    },
                )
                .await
                .expect("retrieve")
                .candidates
                .iter()
                .map(|c| c.memory_id)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        leg(scope_of(wa, None)).await,
        vec![in_2015, spans_2015],
        "the window's rows on the readable node, those starting inside it first"
    );
    assert_eq!(
        leg(scope_of(wa, Some(own))).await,
        vec![in_2015, own_held, spans_2015],
        "the own held row joins only when its key is named"
    );
    let empty = CandidateScope {
        promoted_nodes: Vec::new(),
        kinds: HIT_KINDS.to_vec(),
        own_held: None,
        expired_at: None,
        anchor: oml_app::types::AnchorFilter::default(),
    };
    assert!(leg(empty).await.is_empty(), "an empty scope ranks nothing");
}

/// A version whose valid time ended at or before the scope's `expired_at`
/// is outside every retriever's scope but `keyword_all`'s, whose v0.5.0
/// statement the core re-checks after; with no `expired_at` it is served
/// like any other (proposed ADR-0059 d3).
pub async fn an_expired_memory_is_outside_every_retrievers_scope<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(432);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let other = Uuid::from_u128(0xE5);
    let row =
        |id: u128, body: &str| held_memory(tenant, wa, Uuid::from_u128(id), "rita", other, body);
    let ended = Uuid::from_u128(0x7101);
    let current = Uuid::from_u128(0x7102);
    hold_and_promote(
        &mut rig,
        store,
        dated(
            row(0x7101, "billing deploy freeze"),
            Some("2014"),
            Some("2015"),
        ),
    )
    .await;
    hold_and_promote(
        &mut rig,
        store,
        dated(row(0x7102, "billing deploy window"), Some("2014"), None),
    )
    .await;
    let now = oml_app::domain::valid_time::parse_now("2026-11-05").expect("now");
    let window = oml_app::domain::valid_time::parse_window("in 2015", now).expect("a window");
    for kind in store.retrievers() {
        if kind == RetrieverKind::KeywordAll {
            continue;
        }
        for expired_at in [Some(now), None] {
            let scope = CandidateScope {
                expired_at,
                ..scope_of(wa, None)
            };
            if kind == RetrieverKind::Sparse {
                encode_sparse_backlog(store).await;
            }
            let sparse = if kind == RetrieverKind::Sparse {
                oml_app::ports::SparseEncoder::encode_query(
                    &FakeSparseEncoder::new(),
                    "billing deploy",
                )
            } else {
                Vec::new()
            };
            let got: Vec<Uuid> = store
                .retrieve(
                    tenant,
                    kind,
                    &scope,
                    RetrievalQuery {
                        query: "billing deploy",
                        match_mode: MatchMode::Any,
                        k: 50,
                        sparse: &sparse,
                        window: Some(&window),
                        within: &[ended, current],
                    },
                )
                .await
                .expect("retrieve")
                .candidates
                .iter()
                .map(|c| c.memory_id)
                .collect();
            assert!(
                got.contains(&current),
                "{kind:?}: the current row is served"
            );
            assert_eq!(
                got.contains(&ended),
                expired_at.is_none(),
                "{kind:?} {expired_at:?}: the ended row is hidden exactly when the scope says"
            );
        }
    }
}

/// The `time` leg reads only the rows the text list found, named by id, so
/// its cost is bounded by that list and a window holding more rows than any
/// read cap still answers every one of them it holds; ties break by id,
/// so equal input gives equal output (proposed ADR-0059 d7).
pub async fn the_time_leg_reads_only_the_text_lists_rows_however_many_the_window_holds<
    S: JournalStore,
>(
    store: &S,
) {
    let mut rig = ChainRig::new(434);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let other = Uuid::from_u128(0xE5);
    let mut ids = Vec::new();
    for i in 0..205_u128 {
        let mut m = held_memory(
            tenant,
            wa,
            Uuid::from_u128(0x7300 + i),
            "rita",
            other,
            "a dated note",
        );
        *m.valid =
            oml_app::domain::valid_time::ValidTime::parse(Some("2015-06"), None).expect("ends");
        hold_and_promote(&mut rig, store, m).await;
        ids.push(Uuid::from_u128(0x7300 + i));
    }
    let window = oml_app::domain::valid_time::parse_window("in 2015", 0).expect("a window");
    // The text list found the last row and one outside the window's rows.
    let within = vec![ids[204], ids[3], Uuid::from_u128(0x7FFF)];
    let read = || async {
        store
            .retrieve(
                tenant,
                RetrieverKind::Time,
                &scope_of(wa, None),
                RetrievalQuery {
                    query: "in 2015",
                    match_mode: MatchMode::Any,
                    k: 50,
                    sparse: &[],
                    window: Some(&window),
                    within: &within,
                },
            )
            .await
            .expect("retrieve")
            .candidates
            .iter()
            .map(|c| c.memory_id)
            .collect::<Vec<_>>()
    };
    let first = read().await;
    assert_eq!(
        first,
        vec![ids[3], ids[204]],
        "the text list's rows the window holds, by id"
    );
    assert_eq!(read().await, first, "equal input, equal output");
}

pub async fn run_retriever_suite<S, F, Fut>(new_store: F)
where
    S: JournalStore,
    F: Fn() -> Fut,
    Fut: Future<Output = S>,
{
    a_retriever_never_returns_a_row_outside_the_scope(&new_store().await).await;
    a_retriever_orders_deterministically_by_score_then_id(&new_store().await).await;
    a_row_outside_the_scope_never_changes_a_candidates_score(&new_store).await;
    the_keyword_retrievers_match_all_or_any_word_on_every_store(&new_store().await).await;
    bm25_finds_an_identifier_by_its_parts_and_a_word_by_its_stem(&new_store().await).await;
    bm25_reads_a_query_without_its_english_stop_words_unless_it_has_no_other(&new_store().await)
        .await;
    bm25_scores_a_word_a_lone_row_says_once_at_its_weight(&new_store().await).await;
    bm25_counts_an_inflected_form_once_as_it_counts_the_exact_word(&new_store().await).await;
    trigram_finds_a_misspelled_words_memory(&new_store().await).await;
    did_you_mean_draws_only_on_the_words_of_promoted_served_tips_in_the_scope(&new_store().await)
        .await;
    similar_candidates_are_the_scopes_served_tips_and_own_held_rows_holding_a_key(
        &new_store().await,
    )
    .await;
    the_time_leg_never_returns_a_row_outside_the_scope(&new_store().await).await;
    an_expired_memory_is_outside_every_retrievers_scope(&new_store().await).await;
    the_time_leg_reads_only_the_text_lists_rows_however_many_the_window_holds(&new_store().await)
        .await;
}

/// The similar check's candidates (ADR-0050 d1, d17): of the versions
/// holding one of the keys, the promoted served tips on the scope's nodes
/// and the held rows of its own keys on theirs, of the kinds memory,
/// derived fact and steering; never another key's held row, a replaced,
/// rejected or retirement version, or a row outside the nodes; by keys
/// held, then id; at most the limit.
pub async fn similar_candidates_are_the_scopes_served_tips_and_own_held_rows_holding_a_key<
    S: JournalStore,
>(
    store: &S,
) {
    use oml_app::domain::similar::Fingerprint;
    const TEXT: &str = "Production deploys run only on Tuesday and Thursday between 10:00 \
                        and 12:00 CET; outside that window a deploy needs the lead's approval.";
    let mut rig = ChainRig::new(45);
    rig.seed_chain(store, 0).await;
    let tenant = rig_tenant(45);
    let [wa, elsewhere] = [0xB450_u128, 0xB451].map(Uuid::from_u128);
    let [own, other] = [0xD450_u128, 0xD451].map(Uuid::from_u128);
    let row = |wa: Uuid, id: u128, author: Uuid, kind: MemoryKind, body: &str| MemoryRecord {
        kind,
        ..held_memory(tenant, wa, Uuid::from_u128(id), "wanda", author, body)
    };
    let tip = row(wa, 0xC450, own, MemoryKind::Memory, TEXT);
    let steering = row(
        wa,
        0xC451,
        other,
        MemoryKind::Steering,
        &format!("Note: {TEXT}"),
    );
    let held = row(wa, 0xC452, own, MemoryKind::DerivedFact, TEXT);
    let theirs = row(wa, 0xC453, other, MemoryKind::Memory, TEXT);
    let rejected = row(wa, 0xC454, own, MemoryKind::Memory, TEXT);
    let away = row(elsewhere, 0xC455, own, MemoryKind::Memory, TEXT);
    let replaced = row(wa, 0xC456, own, MemoryKind::Memory, TEXT);
    let retired = row(
        wa,
        0xC458,
        own,
        MemoryKind::Memory,
        "the canary lapsed in May",
    );
    let unrelated = row(wa, 0xC45A, own, MemoryKind::Memory, "zebra quartz");
    for m in [&tip, &steering, &away, &replaced, &retired, &unrelated] {
        hold_and_promote(&mut rig, store, m.clone()).await;
    }
    for m in [&held, &theirs, &rejected] {
        hold(&mut rig, store, m.clone()).await.expect("hold");
    }
    reject(&mut rig, store, rejected.memory_id)
        .await
        .expect("reject");
    supersede(
        &mut rig,
        store,
        &replaced,
        Uuid::from_u128(0xC457),
        "The deploy window moved to Monday mornings.",
        false,
    )
    .await;
    supersede(
        &mut rig,
        store,
        &retired,
        Uuid::from_u128(0xC459),
        TEXT,
        true,
    )
    .await;

    let keys: Vec<i64> = Fingerprint::of(TEXT)
        .query_keys()
        .into_iter()
        .map(|(_, k)| k)
        .collect();
    let scope = |nodes: Vec<Uuid>, keys_: Vec<Uuid>| CandidateScope {
        promoted_nodes: nodes.clone(),
        kinds: vec![
            MemoryKind::Memory,
            MemoryKind::DerivedFact,
            MemoryKind::Steering,
        ],
        own_held: Some(OwnHeldScope {
            any_author: false,
            author_key_ids: keys_,
            nodes,
        }),
        expired_at: None,
        anchor: Default::default(),
    };
    let candidates = |scope: CandidateScope, keys: Vec<i64>, limit: usize| async move {
        store
            .similar_candidates(tenant, &scope, &keys, limit)
            .await
            .expect("similar candidates")
    };
    assert_eq!(
        candidates(scope(vec![wa], vec![own]), keys.clone(), 10).await,
        vec![tip.memory_id, held.memory_id, steering.memory_id],
        "the same text's two rows first, by id, then the near copy, which holds fewer keys"
    );
    assert_eq!(
        candidates(scope(vec![wa], vec![other]), keys.clone(), 10).await,
        vec![tip.memory_id, theirs.memory_id, steering.memory_id],
        "each key set reads its own held rows alone"
    );
    assert_eq!(
        candidates(scope(vec![elsewhere], vec![own]), keys.clone(), 10).await,
        vec![away.memory_id],
        "exactly the scope's nodes"
    );
    assert_eq!(
        candidates(scope(vec![wa], vec![own]), keys.clone(), 1).await,
        vec![tip.memory_id],
        "the limit keeps the prefix of the order"
    );
    assert!(
        candidates(scope(vec![wa], vec![own]), Vec::new(), 10)
            .await
            .is_empty(),
        "no key, no candidate"
    );
    assert!(
        candidates(scope(Vec::new(), Vec::new()), keys, 10)
            .await
            .is_empty(),
        "an empty scope, no candidate"
    );
}

// ---------------------------------------------------------------------------
// Reranker laws (ADR-0053 d10): the double and every model adapter pass them.
// ---------------------------------------------------------------------------

/// How far two scores of one passage may differ across calls and batches:
/// a padded batch runs other kernels than a lone passage, so a model's float
/// result moves in the last places, never by a rank's worth.
pub const RERANK_TOLERANCE: f32 = 1e-3;

const QUERY: &str = "which port does postgres listen on";
const ANSWER: &str = "PostgreSQL listens on port 5432 by default; the eval cluster uses 55437.";
const UNRELATED: &str = "The cafeteria serves lunch from noon until two on weekdays.";
const NEAR: &str = "Postgres connections go through pgbouncer in transaction mode.";

async fn scores<R: Reranker + ?Sized>(r: &R, query: &str, passages: &[&str]) -> Vec<f32> {
    r.score(
        query.to_owned(),
        passages.iter().map(|p| (*p).to_owned()).collect(),
    )
    .await
    .expect("a reranker inside its cap and deadline answers")
}

/// One score per passage, in the order given: reversing the passages
/// reverses the scores.
pub async fn a_reranker_scores_each_passage_once_in_order<R: Reranker + ?Sized>(r: &R) {
    let forward = scores(r, QUERY, &[ANSWER, UNRELATED, NEAR]).await;
    assert_eq!(forward.len(), 3, "one score per passage");
    let backward = scores(r, QUERY, &[NEAR, UNRELATED, ANSWER]).await;
    for (a, b) in forward.iter().zip(backward.iter().rev()) {
        assert!(
            (a - b).abs() <= RERANK_TOLERANCE,
            "{forward:?} against {backward:?}"
        );
    }
    assert!(
        scores(r, QUERY, &[]).await.is_empty(),
        "no passage, no score"
    );
}

/// Every score is a relevance in `[0, 1]`, so a floor reads the same way on
/// every query (ADR-0053 d6).
pub async fn a_reranker_scores_lie_in_the_unit_interval<R: Reranker + ?Sized>(r: &R) {
    for s in scores(r, QUERY, &[ANSWER, UNRELATED, NEAR, ""]).await {
        assert!((0.0..=1.0).contains(&s), "{s} is outside [0, 1]");
    }
}

/// A passage's score depends on the query and that passage alone: the
/// passages beside it in a call, and a call before it, never move it, so no
/// other row, readable or not, reaches a candidate's relevance (ADR-0048 d3).
pub async fn a_reranker_scores_a_passage_alone_as_among_others<R: Reranker + ?Sized>(r: &R) {
    let alone = scores(r, QUERY, &[ANSWER]).await[0];
    let among = scores(r, QUERY, &[UNRELATED, ANSWER, NEAR]).await[1];
    let again = scores(r, QUERY, &[ANSWER]).await[0];
    assert!(
        (alone - among).abs() <= RERANK_TOLERANCE,
        "{alone} alone, {among} among others"
    );
    assert!(
        (alone - again).abs() <= RERANK_TOLERANCE,
        "{alone}, then {again}"
    );
}

/// The passage that answers the query outranks one that shares no word
/// with it.
pub async fn a_reranker_ranks_an_answer_above_an_unrelated_passage<R: Reranker + ?Sized>(r: &R) {
    let s = scores(r, QUERY, &[UNRELATED, ANSWER]).await;
    assert!(
        s[1] > s[0],
        "the answer scored {} and the unrelated {}",
        s[1],
        s[0]
    );
}

/// Every reranker law, run against `r`.
pub async fn run_reranker_suite<R: Reranker + ?Sized>(r: &R) {
    a_reranker_scores_each_passage_once_in_order(r).await;
    a_reranker_scores_lie_in_the_unit_interval(r).await;
    a_reranker_scores_a_passage_alone_as_among_others(r).await;
    a_reranker_ranks_an_answer_above_an_unrelated_passage(r).await;
}

// ---------------------------------------------------------------------------
// Learned sparse laws (ADR-0052 d15, d16, d17), for a store serving the
// `sparse` retriever, with the deterministic encoder double.
// ---------------------------------------------------------------------------

fn sparse_ids(list: &Retrieved) -> Vec<u128> {
    list.candidates
        .iter()
        .map(|c| c.memory_id.as_u128())
        .collect()
}

/// Each `sparse` candidate's score is the sum over the query's tokens of
/// its idf weight times the row's own weight, and nothing else; its
/// `tokens` name each contribution, largest first, and mark the encoder's
/// expansions, and `matched` names only the pieces its text holds
/// (ADR-0052 d16, d17).
pub async fn sparse_scores_each_row_by_its_own_postings_alone<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Sparse) {
        return;
    }
    let mut rig = ChainRig::new(431);
    rig.seed_chain(store, 0).await;
    let (promoted, _, wa, own) = retriever_fixture(&mut rig, store, true).await;
    let encoder = FakeSparseEncoder::new();
    for query in ["billing invoice", "deploy friday", "rollback zebra"] {
        let list = run(
            store,
            rig.tenant_id,
            RetrieverKind::Sparse,
            &scope_of(wa, Some(own)),
            query,
            MatchMode::Any,
        )
        .await;
        assert_eq!(list.access, Access::Postings, "{query}");
        let terms = oml_app::ports::SparseEncoder::encode_query(&encoder, query);
        for c in &list.candidates {
            let body = promoted
                .iter()
                .find(|(id, _)| *id == c.memory_id)
                .map(|(_, b)| *b)
                .unwrap_or("billing deploy notes held");
            let doc = FakeSparseEncoder::document(body);
            let mut want = 0.0_f64;
            for t in &terms {
                if let Some(p) = doc.iter().find(|p| p.token_id == t.token_id) {
                    want += f64::from(t.weight) * f64::from(p.weight);
                }
            }
            #[allow(clippy::cast_possible_truncation, reason = "the score is a float4")]
            let want = want as f32;
            assert!(
                (c.score - want).abs() <= 1e-5 * want.max(1.0),
                "{query}: {} scored {} where its own postings give {want}",
                c.memory_id,
                c.score
            );
            for pair in c.tokens.windows(2) {
                assert!(
                    pair[0].weight >= pair[1].weight,
                    "{query}: tokens largest first"
                );
            }
            for t in &c.tokens {
                let in_text = body.split(' ').any(|w| w == t.token);
                assert_eq!(t.expanded, !in_text, "{query}: {} expanded", t.token);
            }
            assert!(c.matched.iter().all(|m| body.split(' ').any(|w| w == m)));
        }
    }
    // "invoice" is said by no row; "invoices" expands to it, so the rows
    // saying "invoices" are found and the match is named an expansion.
    let expanded = run(
        store,
        rig.tenant_id,
        RetrieverKind::Sparse,
        &scope_of(wa, None),
        "invoice",
        MatchMode::Any,
    )
    .await;
    let mut ids = sparse_ids(&expanded);
    ids.sort_unstable();
    assert_eq!(ids, vec![0xF02, 0xF04]);
    for c in &expanded.candidates {
        assert!(c.matched.is_empty(), "the text does not say invoice");
        assert!(c.tokens.iter().all(|t| t.expanded && t.token == "invoice"));
    }
}

/// A version is enqueued for the encoder in its own write's transaction,
/// leaves the queue when its postings are written, and its kind 2
/// postings follow its state: found while it is the served tip, not after
/// its supersession or retirement, and a rejected held version's never
/// (ADR-0052 d7, d15).
pub async fn an_encoded_version_follows_promotion_supersession_and_retirement<S: JournalStore>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Sparse) {
        return;
    }
    let mut rig = ChainRig::new(432);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let key = Uuid::from_u128(0xE5);
    let first = held_memory(
        tenant,
        wa,
        Uuid::from_u128(0xA01),
        "rita",
        key,
        "quokka habitat notes",
    );
    hold(&mut rig, store, first.clone()).await.expect("hold");
    assert_eq!(encode_sparse_backlog(store).await, 1, "one version encoded");
    let find = |rig_tenant: Uuid| async move {
        sparse_ids(
            &store
                .retrieve(
                    rig_tenant,
                    RetrieverKind::Sparse,
                    &scope_of(wa, None),
                    RetrievalQuery {
                        query: "quokka",
                        match_mode: MatchMode::Any,
                        k: 10,
                        sparse: &oml_app::ports::SparseEncoder::encode_query(
                            &FakeSparseEncoder::new(),
                            "quokka",
                        ),
                        window: None,
                        within: &[],
                    },
                )
                .await
                .expect("retrieve"),
        )
    };
    assert!(
        find(tenant).await.is_empty(),
        "a held version is no promoted hit"
    );
    promote(&mut rig, store, first.memory_id)
        .await
        .expect("promote");
    assert_eq!(
        find(tenant).await,
        vec![0xA01],
        "promotion serves its postings"
    );
    supersede(
        &mut rig,
        store,
        &first,
        Uuid::from_u128(0xA02),
        "quokka burrow notes",
        false,
    )
    .await;
    encode_sparse_backlog(store).await;
    assert_eq!(
        find(tenant).await,
        vec![0xA02],
        "the successor is served and the replaced version is not"
    );
    let second = successor_of(&first, Uuid::from_u128(0xA02), "quokka burrow notes", false);
    supersede(
        &mut rig,
        store,
        &second,
        Uuid::from_u128(0xA03),
        "quokka",
        true,
    )
    .await;
    encode_sparse_backlog(store).await;
    assert!(find(tenant).await.is_empty(), "a retired claim is no hit");
    let rejected = held_memory(
        tenant,
        wa,
        Uuid::from_u128(0xA04),
        "rita",
        key,
        "quokka rejected",
    );
    hold(&mut rig, store, rejected).await.expect("hold");
    encode_sparse_backlog(store).await;
    reject(&mut rig, store, Uuid::from_u128(0xA04))
        .await
        .expect("reject");
    assert!(
        find(tenant).await.is_empty(),
        "a rejected version is no hit"
    );
}

/// A failed encode keeps its version queued, counts the attempt and puts
/// the next try off, so the next pass does not take it at once; a version
/// enqueued again after it was read is not written with the older claim
/// (ADR-0052 d15).
pub async fn a_failed_encode_stays_queued_and_a_stale_read_writes_nothing<S: JournalStore>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Sparse) {
        return;
    }
    let mut rig = ChainRig::new(433);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let key = Uuid::from_u128(0xE5);
    let row = held_memory(
        tenant,
        wa,
        Uuid::from_u128(0xA11),
        "rita",
        key,
        "wombat notes",
    );
    encode_sparse_backlog(store).await;
    hold(&mut rig, store, row).await.expect("hold");
    let pass = oml_app::app::encode_sparse_pass(store, &FakeSparseEncoder::failing("no model"), 16)
        .await
        .expect("pass");
    assert_eq!((pass.written, pass.failed, pass.parked), (0, 1, 0));
    assert!(
        store
            .sparse_pending(tenant, 10)
            .await
            .expect("pending")
            .is_empty(),
        "a failed version waits before its next try"
    );
    let h = store
        .postings_health(tenant)
        .await
        .expect("health")
        .expect("a store serving sparse keeps postings");
    assert_eq!(
        (h.sparse_backlog, h.sparse_failing, h.sparse_parked),
        (1, 1, 0),
        "{h:?}"
    );
    assert!(h.sparse_enabled, "{h:?}");
    let stale = SparsePending {
        memory_id: Uuid::from_u128(0xA11),
        claim: "wombat notes".to_owned(),
        enqueued_at_us: 0,
    };
    assert!(
        !store
            .sparse_write(tenant, &stale, &FakeSparseEncoder::document("wombat notes"))
            .await
            .expect("write"),
        "an entry read before its re-enqueue writes nothing"
    );
    // Failed again and again, it is parked at the attempts' ceiling: a
    // dead letter, counted apart, never taken again.
    let entry = SparsePending {
        enqueued_at_us: 1,
        ..stale
    };
    let mut parked_after = None;
    for attempt in 2..=20 {
        if store
            .sparse_failed(tenant, &entry, "still failing")
            .await
            .expect("fail")
        {
            parked_after = Some(attempt);
            break;
        }
    }
    assert!(
        parked_after.is_some(),
        "an entry is parked within 20 attempts"
    );
    let h = store
        .postings_health(tenant)
        .await
        .expect("health")
        .expect("postings");
    assert_eq!((h.sparse_backlog, h.sparse_parked), (0, 1), "{h:?}");
    assert!(
        store
            .sparse_pending(tenant, 10)
            .await
            .expect("pending")
            .is_empty()
    );
}

/// A version is enqueued with its own write once the tenant's sparse
/// queue is on, and leaves the queue when its postings are written
/// (ADR-0052 d15, d16).
pub async fn a_version_is_enqueued_with_its_write_and_leaves_the_queue_with_its_postings<
    S: JournalStore,
>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Sparse) {
        return;
    }
    let mut rig = ChainRig::new(438);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    encode_sparse_backlog(store).await;
    let backlog = |h: Option<PostingsHealth>| h.expect("postings").sparse_backlog;
    assert_eq!(
        backlog(store.postings_health(tenant).await.expect("health")),
        0
    );
    hold(
        &mut rig,
        store,
        held_memory(
            tenant,
            wa,
            Uuid::from_u128(0xA51),
            "rita",
            Uuid::from_u128(0xE5),
            "potoroo",
        ),
    )
    .await
    .expect("hold");
    assert_eq!(
        backlog(store.postings_health(tenant).await.expect("health")),
        1,
        "the write enqueued its version"
    );
    assert_eq!(encode_sparse_backlog(store).await, 1);
    assert_eq!(
        backlog(store.postings_health(tenant).await.expect("health")),
        0,
        "it left the queue with its postings"
    );
}

/// One claim the model fails on fails alone: the other versions of its
/// batch are written in the same pass, and only it stays queued with the
/// reason (ADR-0052 d15).
pub async fn a_claim_the_model_fails_on_fails_alone_and_its_batch_is_written<S: JournalStore>(
    store: &S,
) {
    if !store.retrievers().contains(&RetrieverKind::Sparse) {
        return;
    }
    let mut rig = ChainRig::new(434);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let key = Uuid::from_u128(0xE5);
    for (id, body) in [
        (0xA21, "numbat notes"),
        (0xA22, "poison claim here"),
        (0xA23, "bilby notes"),
    ] {
        hold(
            &mut rig,
            store,
            held_memory(tenant, wa, Uuid::from_u128(id), "rita", key, body),
        )
        .await
        .expect("hold");
    }
    let mut written = 0;
    let mut failed = 0;
    for _ in 0..4 {
        let pass =
            oml_app::app::encode_sparse_pass(store, &FakeSparseEncoder::failing_on("poison"), 16)
                .await
                .expect("pass");
        written += pass.written;
        failed += pass.failed;
    }
    assert_eq!(written, 2, "the two good claims are written");
    assert_eq!(failed, 1, "only the bad claim fails, once, then waits");
    let health = store
        .postings_health(tenant)
        .await
        .expect("health")
        .expect("a store serving sparse keeps postings");
    assert_eq!(health.sparse_failing, 1, "{health:?}");
}

/// Two tenants' queues never meet: one tenant's pending versions are its
/// own, and a pass writes each tenant's postings under its own row
/// security (ADR-0052 d15).
pub async fn two_tenants_sparse_queues_never_meet<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Sparse) {
        return;
    }
    let wa = Uuid::from_u128(0xE2);
    let key = Uuid::from_u128(0xE5);
    let mut a = ChainRig::new(435);
    a.seed_chain(store, 0).await;
    let mut b = ChainRig::new(436);
    b.seed_chain(store, 0).await;
    let (ta, tb) = (a.tenant_id, b.tenant_id);
    hold_and_promote(
        &mut a,
        store,
        held_memory(ta, wa, Uuid::from_u128(0xA31), "rita", key, "quoll den"),
    )
    .await;
    hold_and_promote(
        &mut b,
        store,
        held_memory(tb, wa, Uuid::from_u128(0xA32), "rita", key, "quoll den"),
    )
    .await;
    encode_sparse_backlog(store).await;
    for (rig, mine, theirs) in [(&a, 0xA31_u128, 0xA32_u128), (&b, 0xA32, 0xA31)] {
        let list = run(
            store,
            rig.tenant_id,
            RetrieverKind::Sparse,
            &scope_of(wa, None),
            "quoll",
            MatchMode::Any,
        )
        .await;
        assert_eq!(sparse_ids(&list), vec![mine], "its own version alone");
        assert!(!sparse_ids(&list).contains(&theirs));
    }
}

/// A sparse list never holds a version its scope excludes, however the
/// version was encoded: another workarea's served version, another key's
/// held version and a held version when no own keys are named are absent
/// (ADR-0049 d2; ADR-0052 d16).
pub async fn a_sparse_list_never_holds_a_version_its_scope_excludes<S: JournalStore>(store: &S) {
    if !store.retrievers().contains(&RetrieverKind::Sparse) {
        return;
    }
    let mut rig = ChainRig::new(437);
    rig.seed_chain(store, 0).await;
    let tenant = rig.tenant_id;
    let (wa, elsewhere) = (Uuid::from_u128(0xE2), Uuid::from_u128(0xE3));
    let (own, other) = (Uuid::from_u128(0xE4), Uuid::from_u128(0xE5));
    let row = |id: u128, node: Uuid, key: Uuid| {
        held_memory(
            tenant,
            node,
            Uuid::from_u128(id),
            "rita",
            key,
            "dunnart burrow",
        )
    };
    hold_and_promote(&mut rig, store, row(0xA41, wa, other)).await;
    hold_and_promote(&mut rig, store, row(0xA42, elsewhere, other)).await;
    hold(&mut rig, store, row(0xA43, wa, own))
        .await
        .expect("own held");
    hold(&mut rig, store, row(0xA44, wa, other))
        .await
        .expect("another's held");
    encode_sparse_backlog(store).await;
    let without = run(
        store,
        tenant,
        RetrieverKind::Sparse,
        &scope_of(wa, None),
        "dunnart",
        MatchMode::Any,
    )
    .await;
    assert_eq!(sparse_ids(&without), vec![0xA41]);
    let mut with = sparse_ids(
        &run(
            store,
            tenant,
            RetrieverKind::Sparse,
            &scope_of(wa, Some(own)),
            "dunnart",
            MatchMode::Any,
        )
        .await,
    );
    with.sort_unstable();
    assert_eq!(
        with,
        vec![0xA41, 0xA43],
        "the own held version joins, no other"
    );
}

/// Every learned sparse law, for a store serving the `sparse` retriever.
pub async fn run_sparse_suite<S, F, Fut>(new_store: F)
where
    S: JournalStore,
    F: Fn() -> Fut,
    Fut: Future<Output = S>,
{
    sparse_scores_each_row_by_its_own_postings_alone(&new_store().await).await;
    an_encoded_version_follows_promotion_supersession_and_retirement(&new_store().await).await;
    a_failed_encode_stays_queued_and_a_stale_read_writes_nothing(&new_store().await).await;
    a_version_is_enqueued_with_its_write_and_leaves_the_queue_with_its_postings(&new_store().await)
        .await;
    a_claim_the_model_fails_on_fails_alone_and_its_batch_is_written(&new_store().await).await;
    two_tenants_sparse_queues_never_meet(&new_store().await).await;
    a_sparse_list_never_holds_a_version_its_scope_excludes(&new_store().await).await;
}
