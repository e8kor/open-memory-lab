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

use core::future::Future;

use oml_app::domain::access::{Grant, KeyDenial};
use oml_app::domain::{
    EventDraft, EventTime, HeldBy, KeySet, MemoryKind, MemoryStatus, Role, SignedEnvelope,
    event_types, key_fingerprint, next_envelope, sha256_digest_string, sign, verify_chain,
};
use oml_app::ed25519_dalek::{SigningKey, Verifier as _};
use oml_app::ports::{
    AdmissionEnvelope, Clock, CommitReceipt, CommitRequest, IdGen, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, KeyringStore, MetadataFold, OutboxRow,
    Scanner, Signer, StoreError,
};
use oml_app::types::{
    ClaimKeyState, Crew, DenialRecord, GrantRecord, KeyAccess, KeyRecord, ListingFilter,
    ListingPosition, ListingScope, MemoryRecord, WorkareaRecord,
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
    key: SigningKey,
    head: Option<SignedEnvelope>,
    n: u64,
}

impl ChainRig {
    #[must_use]
    pub fn new(tenant_seed: u64) -> Self {
        Self {
            tenant_id: Uuid::from_u128(0xA000_0000 + u128::from(tenant_seed)),
            key: test_signing_key(),
            head: None,
            n: 0,
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

    /// Seal and commit one event with its fold; advance the rig head.
    pub async fn commit<S: JournalStore>(
        &mut self,
        store: &S,
        event_type: &str,
        payload: Value,
        fold: MetadataFold,
        idempotency: Option<IdempotencyPut>,
    ) -> Result<CommitReceipt, StoreError> {
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

    /// Commit one `workarea.create` folding `workarea`.
    pub async fn create_workarea<S: JournalStore>(
        &mut self,
        store: &S,
        workarea: WorkareaRecord,
    ) -> CommitReceipt {
        self.commit(
            store,
            event_types::WORKAREA_CREATE,
            json!({ "workarea_id": workarea.workarea_id.to_string(), "name": workarea.name }),
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

    /// Commit one event whose fold changes no row, to move the head.
    pub async fn filler<S: JournalStore>(&mut self, store: &S) -> CommitReceipt {
        self.commit(
            store,
            event_types::SUBMISSION_REMEMBER,
            json!({ "body_digest": "sha256:00" }),
            MetadataFold::TenantGenesis,
            None,
        )
        .await
        .expect("filler commit")
    }

    /// Genesis plus `extra` empty-payload events.
    pub async fn seed_chain<S: JournalStore>(&mut self, store: &S, extra: u64) {
        self.commit(
            store,
            event_types::TENANT_GENESIS,
            json!({ "tenant_name": "contract" }),
            MetadataFold::TenantGenesis,
            None,
        )
        .await
        .expect("genesis commit");
        for _ in 0..extra {
            self.commit(
                store,
                event_types::SUBMISSION_REMEMBER,
                json!({ "body_digest": "sha256:00" }),
                MetadataFold::TenantGenesis,
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
    }
}

/// Commits `memory` held through the rig, enqueued for review.
async fn hold<S: JournalStore>(
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
        },
        None,
    )
    .await
}

/// Commits the rejection of `memory_id` through the rig.
async fn reject<S: JournalStore>(
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

/// A fixture workarea node with no crew.
#[must_use]
pub fn workarea_record(
    tenant_id: Uuid,
    workarea_id: Uuid,
    parent_id: Option<Uuid>,
    name: &str,
) -> WorkareaRecord {
    WorkareaRecord {
        workarea_id,
        tenant_id,
        parent_id,
        name: name.to_owned(),
        crew: None,
    }
}

/// A fixture key row; its value hash is derived from its id, so every
/// fixture key resolves on its own.
#[must_use]
pub fn key_record(tenant_id: Uuid, key_id: Uuid, role: Role, issued_by: Option<Uuid>) -> KeyRecord {
    KeyRecord {
        key_id,
        tenant_id,
        role,
        principal: format!("fixture/{key_id}"),
        held_by: HeldBy::Agent,
        default_workarea: None,
        issued_by,
        key_hash: sha256_digest_string(key_id.as_bytes()),
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
            fold: MetadataFold::TenantGenesis,
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
async fn hold_and_promote<S: JournalStore>(rig: &mut ChainRig, store: &S, memory: MemoryRecord) {
    let id = memory.memory_id;
    hold(rig, store, memory).await.expect("hold");
    promote(rig, store, id).await.expect("promote");
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

/// The ids of recall hits, in the order served.
fn hit_ids(hits: &[(MemoryRecord, f32)]) -> Vec<Uuid> {
    hits.iter().map(|(m, _)| m.memory_id).collect()
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
            let ((a, a_score), (b, b_score)) = (&w[0], &w[1]);
            a_score > b_score || (a_score == b_score && a.memory_id < b.memory_id)
        }),
        "score descending, then artifact_id ascending: {hits:?}"
    );
    let tied: Vec<&(MemoryRecord, f32)> = hits
        .iter()
        .filter(|(m, _)| ties.contains(&m.memory_id))
        .collect();
    assert_eq!(
        tied.iter().map(|(m, _)| m.memory_id).collect::<Vec<_>>(),
        [0xC41_u128, 0xC42, 0xC43].map(Uuid::from_u128),
        "equal texts tie, and the tie breaks by artifact_id"
    );
    assert!(tied.iter().all(|(_, score)| *score == tied[0].1));
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
}

/// Key rows fold in with their event and resolve by value hash — one
/// lookup, never by value (roles-and-keys.md hash at rest).
pub async fn journal_key_lookup_by_hash<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(7);
    rig.seed_chain(store, 0).await;
    let record = KeyRecord {
        key_id: Uuid::from_u128(0xD9),
        tenant_id: rig.tenant_id,
        role: oml_app::domain::Role::Writer,
        principal: "wanda".to_owned(),
        held_by: oml_app::domain::HeldBy::Agent,
        default_workarea: None,
        issued_by: None,
        key_hash: "sha256:feed".to_owned(),
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
        store.key_by_hash(&key.key_hash).await.expect("lookup"),
        Some(key.clone())
    );

    rig.revoke_key(store, key.key_id).await.expect("revoke");
    assert_eq!(
        store.key_by_hash(&key.key_hash).await.expect("lookup"),
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
    rig.commit(
        store,
        event_types::SUBMISSION_REMEMBER,
        json!({}),
        MetadataFold::TenantGenesis,
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
    };
    let reader_grant = Grant {
        grant_id: Uuid::from_u128(0xF10),
        node: crew_a,
        role: Role::Reader,
        deny: Vec::new(),
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
        }),
    );
    assert_eq!(
        store.key_access(t, admin.key_id).await.expect("read"),
        Some(KeyAccess {
            issued_seq: admin_issued.seq,
            grants: Vec::new(),
            denials: Vec::new(),
        }),
        "a key with no grant reads back empty lists, never nothing"
    );
    assert_eq!(
        store.key_by_hash(&agent.key_hash).await.expect("lookup"),
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

/// An ancestry that leaves the tenant before reaching a root — a node whose
/// parent id names another tenant's node — is refused, never served short:
/// a truncated walk would drop the ancestors' grants and denials
/// (workareas.md effective access).
pub async fn an_ancestry_that_leaves_the_tenant_is_refused<S: JournalStore>(store: &S) {
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
    let stray = Uuid::from_u128(0xB81);
    rig.create_workarea(
        store,
        workarea_record(rig.tenant_id, stray, Some(foreign_root), "stray"),
    )
    .await;

    let walk = store.workarea_ancestry(rig.tenant_id, stray).await;
    assert!(
        matches!(walk, Err(StoreError::Backend(_))),
        "a walk out of the tenant is refused, got {walk:?}"
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
/// in byte order, then id — the order the workarea read serves them in
/// (ADR-0025 d14; openapi.yaml `WorkareaView.children`).
pub async fn workarea_children_are_immediate_and_ordered_by_name_bytes_then_id<S: JournalStore>(
    store: &S,
) {
    let mut rig = ChainRig::new(16);
    rig.seed_chain(store, 0).await;
    let t = rig.tenant_id;
    let parent = Uuid::from_u128(0xB69);
    let id = |n: u128| Uuid::from_u128(0xB60 + n);
    rig.create_workarea(store, workarea_record(t, parent, None, "universe"))
        .await;
    // Byte order puts "Zeta" before "alpha" and "étude" after "beta",
    // where a linguistic collation would not; two "alpha" tie on id.
    for (n, name) in [
        (1, "alpha"),
        (2, "Zeta"),
        (3, "étude"),
        (4, "beta"),
        (0, "alpha"),
    ] {
        rig.create_workarea(store, workarea_record(t, id(n), Some(parent), name))
            .await;
    }
    rig.create_workarea(store, workarea_record(t, id(5), Some(id(1)), "aaa"))
        .await;

    let children = store.workarea_children(t, parent).await.expect("children");
    assert_eq!(
        children
            .iter()
            .map(|w| (w.name.as_str(), w.workarea_id))
            .collect::<Vec<_>>(),
        vec![
            ("Zeta", id(2)),
            ("alpha", id(0)),
            ("alpha", id(1)),
            ("beta", id(4)),
            ("étude", id(3)),
        ],
        "immediate children only, by name bytes then id"
    );
    assert!(
        store
            .workarea_children(t, id(3))
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
    let request = |event: SignedEnvelope| CommitRequest {
        outbox: OutboxRow {
            event_id: event.envelope.event_id,
            topic: event.envelope.event_type.clone(),
            payload: json!({ "seq": event.envelope.seq }),
        },
        event,
        fold: MetadataFold::TenantGenesis,
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
            subtree: Some(vec![denied]),
            ..own(Vec::new())
        })
        .await,
        vec![masked.memory_id],
        "the subtree narrows"
    );
    assert_eq!(
        list(ListingFilter {
            state: Some(MemoryStatus::Held),
            ..own(vec![denied])
        })
        .await,
        vec![held.memory_id],
        "the state narrows"
    );
    assert_eq!(
        list(ListingFilter {
            kind: Some(MemoryKind::Steering),
            ..own(Vec::new())
        })
        .await,
        vec![rejected.memory_id],
        "the kind narrows"
    );
    assert_eq!(
        list(ListingFilter {
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
    for id in [replaced.memory_id, tip.memory_id, off_set.memory_id] {
        promote(&mut rig, store, id).await.expect("promote");
    }
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
    let tips = |nodes: Vec<Uuid>| listing(ListingScope::Tips { nodes });

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

/// Every journal-store law in one call, each against a fresh store.
pub async fn run_journal_store_suite<S, F, Fut>(new_store: F)
where
    S: JournalStore,
    F: Fn() -> Fut,
    Fut: Future<Output = S>,
{
    journal_commit_and_head(&new_store().await).await;
    journal_commit_refuses_conflicts(&new_store().await).await;
    journal_events_range_pages(&new_store().await).await;
    journal_memory_fold_and_lookup(&new_store().await).await;
    recall_serves_promoted_tips_only_within_the_readable_set(&new_store().await).await;
    hits_that_tie_on_score_are_ordered_by_artifact_id(&new_store().await).await;
    steering_is_never_returned_as_a_hit(&new_store().await).await;
    retired_claims_are_absent_from_recall_and_steering(&new_store().await).await;
    steering_in_force_is_ordered_by_depth_then_promoted_at_then_artifact_id(&new_store().await)
        .await;
    journal_review_claim_laws(&new_store().await).await;
    journal_key_lookup_by_hash(&new_store().await).await;
    a_revoked_key_no_longer_resolves_by_its_hash(&new_store().await).await;
    journal_idempotency_roundtrip(&new_store().await).await;
    a_second_record_under_one_idempotency_scope_commits_nothing(&new_store().await).await;
    grants_and_denials_round_trip_through_their_folds_and_key_access(&new_store().await).await;
    a_grant_epoch_is_its_event_seq(&new_store().await).await;
    a_grant_or_denial_the_schema_refuses_commits_nothing(&new_store().await).await;
    workarea_ancestry_walks_parent_ids_to_the_root(&new_store().await).await;
    an_ancestry_that_leaves_the_tenant_is_refused(&new_store().await).await;
    workarea_subtree_lists_every_descendant_once(&new_store().await).await;
    workarea_children_are_immediate_and_ordered_by_name_bytes_then_id(&new_store().await).await;
    crew_round_trips_through_workarea_created(&new_store().await).await;
    kind_version_and_body_bytes_round_trip_through_memory_held(&new_store().await).await;
    promotion_makes_the_version_the_tip_and_stamps_promoted_at_from_its_event(&new_store().await)
        .await;
    a_second_served_tip_of_one_claim_commits_nothing(&new_store().await).await;
    tip_superseded_refuses_when_the_target_is_no_longer_tip(&new_store().await).await;
    a_chain_whose_ratification_loses_the_compare_and_set_journals_no_verdict(&new_store().await)
        .await;
    a_second_live_version_with_one_ordinal_commits_nothing(&new_store().await).await;
    claim_key_state_counts_every_version_and_only_live_ones_hold_the_key(&new_store().await).await;
    an_own_listing_holds_every_state_of_the_key_sets_rows_off_masked_nodes(&new_store().await)
        .await;
    a_tips_listing_holds_only_the_promoted_tips_on_its_nodes(&new_store().await).await;
    a_listing_pages_newest_first_and_resumes_strictly_after_its_position(&new_store().await).await;
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
