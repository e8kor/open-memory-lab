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
    EventDraft, EventTime, HeldBy, KeySet, MemoryStatus, Role, SignedEnvelope, event_types,
    key_fingerprint, next_envelope, sha256_digest_string, sign, verify_chain,
};
use oml_app::ed25519_dalek::{SigningKey, Verifier as _};
use oml_app::ports::{
    AdmissionEnvelope, Clock, CommitReceipt, CommitRequest, IdGen, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, KeyringStore, MetadataFold, OutboxRow,
    Scanner, Signer, StoreError,
};
use oml_app::types::{
    Crew, DenialRecord, GrantRecord, KeyAccess, KeyRecord, MemoryRecord, WorkareaRecord,
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

/// A held fixture memory with its one-author lineage closure.
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
        claim_key: None,
        body: body.to_owned(),
        body_digest: oml_app::domain::sha256_digest_string(body.as_bytes()),
        status: MemoryStatus::Held,
        author_principal: author_principal.to_owned(),
        author_key_id,
        derives_from: Vec::new(),
        lineage_principals: vec![author_principal.to_owned()],
        lineage_key_ids: vec![author_key_id],
        superseded_by: None,
        scan_finding: None,
        created_at: EventTime::parse("2026-10-29T13:00:00.000000Z").expect("fixture time"),
    }
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

/// Recall returns promoted rows only — R3 at the store (retrieval.md) —
/// searches exactly the readable workarea set it is handed, an empty set
/// included, and honors the `k` bound (ADR-0025 d1: no tenant-wide
/// fallback).
pub async fn journal_recall_promoted_only<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(5);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB2);
    let promoted_id = Uuid::from_u128(0xC2);
    let held_id = Uuid::from_u128(0xC3);
    for (id, n) in [(promoted_id, 1_u8), (held_id, 2)] {
        rig.commit(
            store,
            event_types::SUBMISSION_REMEMBER,
            json!({ "n": n }),
            MetadataFold::MemoryHeld {
                memory: held_memory(
                    rig.tenant_id,
                    wa,
                    id,
                    "wanda",
                    Uuid::from_u128(0xD1),
                    "the canary token",
                ),
                enqueue: true,
            },
            None,
        )
        .await
        .expect("commit");
    }
    rig.commit(
        store,
        event_types::LIFECYCLE_PROMOTE,
        json!({}),
        MetadataFold::MemoryPromoted {
            memory_id: promoted_id,
        },
        None,
    )
    .await
    .expect("promote");

    let hits = store
        .recall_fts(rig.tenant_id, &[wa], "canary", 10)
        .await
        .expect("recall");
    assert_eq!(
        hits.len(),
        1,
        "promoted rows only — the held twin is invisible"
    );
    assert_eq!(hits[0].memory_id, promoted_id);
    let wider = store
        .recall_fts(rig.tenant_id, &[Uuid::from_u128(0xBF), wa], "canary", 10)
        .await
        .expect("recall");
    assert_eq!(wider, hits, "every node of the set is searched");
    let other_wa = store
        .recall_fts(rig.tenant_id, &[Uuid::from_u128(0xBF)], "canary", 10)
        .await
        .expect("recall");
    assert!(other_wa.is_empty(), "the workarea set holds");
    let nothing_readable = store
        .recall_fts(rig.tenant_id, &[], "canary", 10)
        .await
        .expect("recall");
    assert!(
        nothing_readable.is_empty(),
        "an empty readable set searches nothing, never the tenant"
    );
    let bounded = store
        .recall_fts(rig.tenant_id, &[wa], "canary", 0)
        .await
        .expect("recall");
    assert!(bounded.is_empty(), "k bounds the result");
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
    journal_recall_promoted_only(&new_store().await).await;
    journal_review_claim_laws(&new_store().await).await;
    journal_key_lookup_by_hash(&new_store().await).await;
    a_revoked_key_no_longer_resolves_by_its_hash(&new_store().await).await;
    journal_idempotency_roundtrip(&new_store().await).await;
    grants_and_denials_round_trip_through_their_folds_and_key_access(&new_store().await).await;
    a_grant_epoch_is_its_event_seq(&new_store().await).await;
    a_grant_or_denial_the_schema_refuses_commits_nothing(&new_store().await).await;
    workarea_ancestry_walks_parent_ids_to_the_root(&new_store().await).await;
    an_ancestry_that_leaves_the_tenant_is_refused(&new_store().await).await;
    workarea_subtree_lists_every_descendant_once(&new_store().await).await;
    workarea_children_are_immediate_and_ordered_by_name_bytes_then_id(&new_store().await).await;
    crew_round_trips_through_workarea_created(&new_store().await).await;
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
