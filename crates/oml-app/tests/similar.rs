//! The write-time similar check (ADR-0050 d1, d17): a write names the rows
//! its key may read that hold the same text or a near copy, holds by
//! default, refuses on `on_similar: refuse` journalling nothing, never names
//! a row the key may not read, and replays what it first answered.

use std::sync::atomic::{AtomicU64, Ordering};

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::{HeldBy, MemoryKind, MemoryStatus, Role};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::types::{
    AppError, BootstrapRequest, GrantRequest, IssueKeyRequest, OnSimilar, SimilarItem,
    SubmitRequest, SupersedeRequest, Verdict,
};
use uuid::Uuid;

const TEST_SEED: [u8; 32] = [7; 32];

const LONG: &str = "Production deploys run only on Tuesday and Thursday between 10:00 and \
                    12:00 CET; outside that window a deploy needs the on-call lead's approval.";

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

fn grant(node: Uuid, role: Role) -> GrantRequest {
    GrantRequest {
        node: node.into(),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    }
}

async fn issue(app: &TestApp, admin: &str, role: Role, grants: Vec<GrantRequest>) -> String {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    let n = ISSUED.fetch_add(1, Ordering::Relaxed);
    app.issue_key(
        admin,
        &IssueKeyRequest {
            profile: None,
            role,
            name: name(&format!("similar-key-{n}")),
            principal: format!("principal-{n}"),
            held_by: Some(HeldBy::Agent),
            default_workarea: None,
            crew: None,
            grants,
            connect_code: false,
            user: None,
            guardrail_scope: None,
        },
    )
    .await
    .expect("issue key")
    .value
}

/// Two sibling workareas: `alice` writes and reads both, `bob` writes and
/// reads `crew` alone, and `judge` reviews both.
struct World {
    app: TestApp,
    store: MemStore,
    tenant: Uuid,
    admin: String,
    alice: String,
    bob: String,
    judge: String,
    crew: Uuid,
    other: Uuid,
}

async fn world() -> World {
    world_on(MemStore::new()).await
}

async fn world_on(store: MemStore) -> World {
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("root-admin"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let mut nodes = Vec::new();
    for n in ["crew", "other"] {
        nodes.push(
            app.create_workarea(&admin, &name(n), Some(boot.root_workarea_id.into()), None)
                .await
                .expect("workarea")
                .workarea
                .workarea_id,
        );
    }
    let (crew, other) = (nodes[0], nodes[1]);
    let both = |role| vec![grant(crew, role), grant(other, role)];
    let mut alice_grants = both(Role::Writer);
    alice_grants.extend(both(Role::Reader));
    let alice = issue(&app, &admin, Role::Writer, alice_grants).await;
    let bob = issue(
        &app,
        &admin,
        Role::Writer,
        vec![grant(crew, Role::Writer), grant(crew, Role::Reader)],
    )
    .await;
    let judge = issue(&app, &admin, Role::Dreamer, both(Role::Dreamer)).await;
    World {
        app,
        store,
        tenant: boot.tenant_id,
        admin,
        alice,
        bob,
        judge,
        crew,
        other,
    }
}

fn submit(idem: &str, body: &str, workarea: Uuid, on_similar: OnSimilar) -> SubmitRequest {
    SubmitRequest {
        origin: None,
        on_similar,
        idempotency_key: idem.to_owned(),
        claim_key: None,
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workarea: Some(workarea.into()),
        derives_from: Vec::new(),
        severity: None,
        rule: None,
        anchor: None,
        valid_from: None,
        valid_until: None,
    }
}

impl World {
    async fn write(
        &self,
        key: &str,
        idem: &str,
        body: &str,
        workarea: Uuid,
    ) -> (Uuid, Vec<SimilarItem>) {
        let receipt = self
            .app
            .submit_memory(key, &submit(idem, body, workarea, OnSimilar::Hold))
            .await
            .expect("submit")
            .receipt;
        (receipt.memory_id, receipt.similar)
    }

    async fn promoted(&self, key: &str, idem: &str, body: &str, workarea: Uuid) -> Uuid {
        let (id, _) = self.write(key, idem, body, workarea).await;
        self.app
            .post_verdict(&self.judge, id, Verdict::Approve, None)
            .await
            .expect("approve");
        id
    }

    async fn head(&self) -> u64 {
        self.app.verify_chain(&self.admin).await.expect("chain").seq
    }
}

#[tokio::test]
async fn a_write_lists_a_same_text_row_the_writer_may_read() {
    let w = world().await;
    let original = w.promoted(&w.alice, "a", LONG, w.crew).await;
    let (_, similar) = w
        .write(&w.bob, "b", &LONG.to_uppercase().replace(';', " -"), w.crew)
        .await;
    assert_eq!(similar.len(), 1, "{similar:?}");
    assert_eq!(similar[0].artifact_id, original);
    assert_eq!(similar[0].relation, "same_text");
    assert_eq!(similar[0].jaccard_percent, 100);
    assert_eq!(similar[0].state, "promoted");
    assert!(!similar[0].own, "alice wrote it, not bob");
}

#[tokio::test]
async fn a_write_lists_a_near_copy_at_jaccard_0_9_and_not_below() {
    let w = world().await;
    let original = w.promoted(&w.alice, "a", LONG, w.crew).await;
    let (_, near) = w.write(&w.bob, "b", &format!("Note: {LONG}"), w.crew).await;
    assert_eq!(near.len(), 1, "{near:?}");
    assert_eq!(near[0].artifact_id, original);
    assert_eq!(near[0].relation, "near_copy");
    assert!((90..100).contains(&near[0].jaccard_percent), "{near:?}");
    // LONG holds 24 distinct words: one added is 24 / 25 = 0.96, five
    // added is 24 / 29 = 0.83.
    let (_, below) = w
        .write(
            &w.bob,
            "c",
            &format!("{LONG} Ask in the release channel first."),
            w.crew,
        )
        .await;
    assert!(below.is_empty(), "{below:?}");
    let (_, paraphrase) = w
        .write(
            &w.bob,
            "d",
            "Deploys to production need the on-call lead to approve them unless they land on a \
             Tuesday or a Thursday morning in the agreed window.",
            w.crew,
        )
        .await;
    assert!(paraphrase.is_empty(), "{paraphrase:?}");
}

#[tokio::test]
async fn a_short_text_is_never_listed_as_a_near_copy() {
    let w = world().await;
    w.promoted(&w.alice, "a", "Port 8080, not 8081.", w.crew)
        .await;
    w.promoted(&w.alice, "b", "Use port 8080 for the api.", w.crew)
        .await;
    let (_, swapped) = w.write(&w.bob, "c", "Port 8081, not 8080.", w.crew).await;
    assert!(swapped.is_empty(), "{swapped:?}");
    let (_, value) = w
        .write(&w.bob, "d", "Use port 8081 for the api.", w.crew)
        .await;
    assert!(value.is_empty(), "{value:?}");
    let (_, same) = w.write(&w.bob, "e", "port 8080 - not 8081", w.crew).await;
    assert_eq!(same.len(), 1, "a short same text is still named");
    assert_eq!(same[0].relation, "same_text");
}

#[tokio::test]
async fn a_write_never_lists_a_row_the_writer_may_not_read() {
    let w = world().await;
    // Alice's held row in crew, and her promoted row in other, which bob
    // cannot read.
    let held = w.write(&w.alice, "a", LONG, w.crew).await.0;
    w.promoted(&w.alice, "b", &format!("Note: {LONG}"), w.other)
        .await;
    let (_, bobs) = w.write(&w.bob, "c", LONG, w.crew).await;
    assert!(
        bobs.is_empty(),
        "another key's held row and an unreadable row: {bobs:?}"
    );
    // Alice reads her own held row and the promoted one in other only from
    // other's scope.
    let (_, own) = w
        .write(&w.alice, "d", &format!("{LONG} Really."), w.crew)
        .await;
    assert_eq!(own.len(), 1, "{own:?}");
    assert_eq!(own[0].artifact_id, held);
    assert!(own[0].own);
    assert_eq!(own[0].state, "held");
}

#[tokio::test]
async fn on_similar_refuse_journals_nothing_and_claims_no_idempotency_record() {
    let w = world().await;
    let original = w.promoted(&w.alice, "a", LONG, w.crew).await;
    let before = w.head().await;
    let refused = w
        .app
        .submit_memory(&w.bob, &submit("b", LONG, w.crew, OnSimilar::Refuse))
        .await
        .expect_err("refused");
    let AppError::SimilarExists(similar) = refused else {
        panic!("{refused:?}");
    };
    assert_eq!(similar[0].artifact_id, original);
    assert_eq!(w.head().await, before, "nothing journalled");
    // The same idempotency key, resent with hold, is a new write.
    let outcome = w
        .app
        .submit_memory(&w.bob, &submit("b", LONG, w.crew, OnSimilar::Hold))
        .await
        .expect("held");
    assert!(!outcome.replayed);
    assert_eq!(outcome.receipt.similar.len(), 1);
    assert_eq!(w.head().await, before + 1);
    // Refuse with nothing similar writes as hold does.
    let fresh = w
        .app
        .submit_memory(
            &w.bob,
            &submit(
                "c",
                "The billing cron runs at 02:00 UTC.",
                w.crew,
                OnSimilar::Refuse,
            ),
        )
        .await
        .expect("nothing similar");
    assert!(fresh.receipt.similar.is_empty());
}

#[tokio::test]
async fn a_replayed_write_answers_the_similar_list_first_answered() {
    let w = world().await;
    let alices = w.write(&w.alice, "a", LONG, w.crew).await.0;
    let first = w
        .app
        .submit_memory(&w.bob, &submit("b", LONG, w.crew, OnSimilar::Hold))
        .await
        .expect("held");
    assert!(first.receipt.similar.is_empty(), "alice's row is held");
    w.app
        .post_verdict(&w.judge, alices, Verdict::Approve, None)
        .await
        .expect("approve");
    let replay = w
        .app
        .submit_memory(&w.bob, &submit("b", LONG, w.crew, OnSimilar::Hold))
        .await
        .expect("replayed");
    assert!(replay.replayed);
    assert_eq!(replay.receipt, first.receipt, "as first answered");
}

#[tokio::test]
async fn a_supersession_never_lists_its_own_claim() {
    let w = world().await;
    let original = w.promoted(&w.alice, "a", LONG, w.crew).await;
    let other_copy = w
        .promoted(&w.bob, "b", &format!("{LONG} Really."), w.crew)
        .await;
    let outcome = w
        .app
        .supersede(
            &w.alice,
            &SupersedeRequest {
                origin: None,
                on_similar: OnSimilar::Hold,
                idempotency_key: "c".to_owned(),
                target_id: original,
                body: format!("Note: {LONG}"),
                kind: None,
                retirement: false,
                derives_from: Vec::new(),
                reason: None,
                workarea: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
                anchor: None,
                valid_from: None,
                valid_until: None,
                // The replace outcome: the listed row's version (ADR-0057 d19).
                expected_version: 1,
            },
        )
        .await
        .expect("supersede");
    let named: Vec<Uuid> = outcome
        .receipt
        .similar
        .iter()
        .map(|s| s.artifact_id)
        .collect();
    assert_eq!(
        named,
        vec![other_copy],
        "bob's copy, never alice's own claim"
    );
}

#[tokio::test]
async fn on_similar_refuse_fires_on_a_near_copy_as_on_the_same_text() {
    let w = world().await;
    let original = w.promoted(&w.alice, "a", LONG, w.crew).await;
    let refused = w
        .app
        .submit_memory(
            &w.bob,
            &submit("b", &format!("Note: {LONG}"), w.crew, OnSimilar::Refuse),
        )
        .await
        .expect_err("a near copy is refused");
    let AppError::SimilarExists(similar) = refused else {
        panic!("{refused:?}");
    };
    assert_eq!(similar[0].artifact_id, original);
    assert_eq!(similar[0].relation, "near_copy");
}

#[tokio::test]
async fn the_core_admits_no_candidate_the_writer_may_not_read_even_from_a_store_that_names_it() {
    let store = MemStore::new();
    store.widen_similar_candidates();
    let w = world_on(store).await;
    // Alice's held row in crew, and her promoted row in other, which bob
    // cannot read: a store that ignores the scope hands both to the core.
    w.write(&w.alice, "a", LONG, w.crew).await;
    w.promoted(&w.alice, "b", &format!("Note: {LONG}"), w.other)
        .await;
    let refused = w
        .app
        .submit_memory(&w.bob, &submit("c", LONG, w.crew, OnSimilar::Refuse))
        .await
        .expect("nothing readable is named, so nothing is refused");
    assert!(refused.receipt.similar.is_empty(), "{refused:?}");
}

#[tokio::test]
async fn a_replay_drops_a_row_the_writer_can_no_longer_read() {
    let w = world().await;
    let original = w.promoted(&w.alice, "a", LONG, w.crew).await;
    let first = w
        .app
        .submit_memory(&w.bob, &submit("b", LONG, w.crew, OnSimilar::Hold))
        .await
        .expect("held");
    assert_eq!(first.receipt.similar.len(), 1);
    w.store
        .set_status(w.tenant, original, MemoryStatus::Demoted);
    let replay = w
        .app
        .submit_memory(&w.bob, &submit("b", LONG, w.crew, OnSimilar::Hold))
        .await
        .expect("replayed");
    assert!(replay.replayed);
    assert!(replay.receipt.similar.is_empty(), "{:?}", replay.receipt);
    assert_eq!(replay.receipt.memory_id, first.receipt.memory_id);
}
