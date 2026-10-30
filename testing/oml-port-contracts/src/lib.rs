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

use oml_app::domain::{
    EventDraft, EventTime, KeySet, MemoryStatus, SignedEnvelope, event_types, key_fingerprint,
    next_envelope, sign, verify_chain,
};
use oml_app::ed25519_dalek::{SigningKey, Verifier as _};
use oml_app::ports::{
    AdmissionEnvelope, Clock, CommitReceipt, CommitRequest, IdGen, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, KeyringStore, MetadataFold, OutboxRow,
    Scanner, Signer, StoreError,
};
use oml_app::types::{KeyRecord, MemoryRecord};
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
            event_id: Uuid::from_u128(u128::from(self.n)),
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
/// honors the workarea filter and the `k` bound.
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
        .recall_fts(rig.tenant_id, Some(wa), "canary", 10)
        .await
        .expect("recall");
    assert_eq!(
        hits.len(),
        1,
        "promoted rows only — the held twin is invisible"
    );
    assert_eq!(hits[0].memory_id, promoted_id);
    let other_wa = store
        .recall_fts(rig.tenant_id, Some(Uuid::from_u128(0xBF)), "canary", 10)
        .await
        .expect("recall");
    assert!(other_wa.is_empty(), "the workarea filter holds");
    let bounded = store
        .recall_fts(rig.tenant_id, None, "canary", 0)
        .await
        .expect("recall");
    assert!(bounded.is_empty(), "k bounds the result");
}

/// The review queue serves each item to one reviewer, skips rows the
/// reviewer's lineage excludes, and re-serves a reviewer its own claim.
pub async fn journal_review_claim_laws<S: JournalStore>(store: &S) {
    let mut rig = ChainRig::new(6);
    rig.seed_chain(store, 0).await;
    let wa = Uuid::from_u128(0xB3);
    let m1 = Uuid::from_u128(0xC4);
    let m2 = Uuid::from_u128(0xC5);
    let author1_key = Uuid::from_u128(0xD2);
    for (id, author, key) in [
        (m1, "alice", author1_key),
        (m2, "bob", Uuid::from_u128(0xD3)),
    ] {
        rig.commit(
            store,
            event_types::SUBMISSION_REMEMBER,
            json!({ "artifact_id": id.to_string() }),
            MetadataFold::MemoryHeld {
                memory: held_memory(rig.tenant_id, wa, id, author, key, "body"),
                enqueue: true,
            },
            None,
        )
        .await
        .expect("commit");
    }

    // alice (in m1's lineage) is served m2, never her own row.
    let alice_key = Uuid::from_u128(0xE1);
    let served = store
        .review_claim_next(rig.tenant_id, "alice", alice_key)
        .await
        .expect("claim")
        .expect("m2 waits");
    assert_eq!(served.memory_id, m2, "lineage-excluded rows are skipped");
    // A re-claim by the same key returns the same item.
    let again = store
        .review_claim_next(rig.tenant_id, "alice", alice_key)
        .await
        .expect("claim")
        .expect("still hers");
    assert_eq!(again.memory_id, m2);
    // An independent reviewer gets the remaining item, not alice's claim.
    let served = store
        .review_claim_next(rig.tenant_id, "carol", Uuid::from_u128(0xE2))
        .await
        .expect("claim")
        .expect("m1 waits");
    assert_eq!(served.memory_id, m1);
    // Everything is claimed: a third reviewer gets nothing.
    let none = store
        .review_claim_next(rig.tenant_id, "dave", Uuid::from_u128(0xE3))
        .await
        .expect("claim");
    assert!(none.is_none());
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
    journal_idempotency_roundtrip(&new_store().await).await;
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
