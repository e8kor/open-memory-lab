//! Use-case tests over the in-memory doubles: the full happy path, the R2
//! refusals, the role ceilings, effective access through grants and denials
//! (ADR-0025 d1, d3), issuance in the issuance order (ADR-0025 d2), the
//! idempotent replay, and names: resolution after the verb ceiling, crews,
//! names at issuance, the naming of rows that predate names and the admin
//! structure reads (ADR-0026 d4–d6, d8, d9, d11, d15).

use oml_app::App;
use oml_app::domain::access::{admissible_grant_roles, issuable_key_roles};
use oml_app::domain::budget::{Budget, budget_tokens};
use oml_app::domain::name::{Name, NamedKind, Reference};
use oml_app::domain::{
    EventDraft, EventTime, HeldBy, KeySet, MemoryKind, MemoryStatus, Role, Sha256Hash,
    SignedEnvelope, event_types, next_envelope, sha256_digest_string, sign, verify_chain,
};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{JournalStore as _, StoreError};
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, CrewCreateRequest, CrewUpdate, GrantRequest,
    IssueKeyRequest, IssuedKey, ListRequest, NamingRefusal, RecallRequest, RecallResult,
    SubmitRequest, SupersedeRequest, Verdict, VersionRef,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

/// The committed test key seed (docs/data/chain-format.md test vector key).
const TEST_SEED: [u8; 32] = [
    0x09, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn app() -> TestApp {
    app_over(MemStore::new())
}

fn app_over(store: MemStore) -> TestApp {
    App::new(
        store,
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
}

/// The whole journal of `tenant`, read through the port: key and grant
/// events belong to the evidence export, never to the content tail
/// (ADR-0025 d15).
async fn journal(store: &MemStore, tenant: Uuid) -> Vec<SignedEnvelope> {
    store.events_range(tenant, 1, 1000).await.expect("journal")
}

/// Each event's hash, in chain order: two equal lists are one journal.
fn hashes(events: &[SignedEnvelope]) -> Vec<Sha256Hash> {
    events.iter().map(|e| e.hash).collect()
}

async fn bootstrap(app: &TestApp) -> BootstrapResult {
    app.bootstrap_tenant(&BootstrapRequest {
        tenant_name: Name::parse("acme").expect("a name"),
        admin_key_name: Name::parse("root-admin").expect("a name"),
        admin_principal: "alice-admin".to_owned(),
    })
    .await
    .expect("bootstrap")
}

/// A workarea under `parent`, created by the admin.
async fn workarea(app: &TestApp, admin: &str, name: &str, parent: Uuid) -> Uuid {
    let name = Name::parse(name).expect("a fixture name the creation grammar admits");
    app.create_workarea(admin, &name, Some(parent.into()), None)
        .await
        .expect("create workarea")
        .workarea
        .workarea_id
}

/// Bootstrap and one crew workarea under the root: (bootstrap, crew).
async fn tenant(app: &TestApp) -> (BootstrapResult, Uuid) {
    let boot = bootstrap(app).await;
    let crew = workarea(app, &boot.admin_key.value, "crew", boot.root_workarea_id).await;
    (boot, crew)
}

fn grant(node: Uuid, role: Role) -> GrantRequest {
    GrantRequest {
        node: node.into(),
        role,
        deny: Vec::new(),
    }
}

/// A name the creation grammar admits.
fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name the creation grammar admits")
}

/// A reference by name, as a request spells it (ADR-0026 d3).
fn by_name(value: &str) -> Reference {
    Reference::parse(value).expect("a name in a reference")
}

/// A key name no other fixture key holds: every issuance takes one
/// (ADR-0026 d9).
fn fresh_key_name() -> Name {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    name(&format!(
        "test-key-{}",
        ISSUED.fetch_add(1, Ordering::Relaxed)
    ))
}

fn issue_req(role: Role, principal: &str, grants: Vec<GrantRequest>) -> IssueKeyRequest {
    IssueKeyRequest {
        role,
        name: fresh_key_name(),
        principal: principal.to_owned(),
        held_by: Some(HeldBy::Agent),
        default_workarea: None,
        crew: None,
        grants,
    }
}

/// Issues a key carrying exactly `grants`; with no default named, its
/// default is the first grant's node (ADR-0025 d2).
async fn issue_with(
    app: &TestApp,
    admin: &str,
    role: Role,
    principal: &str,
    grants: Vec<GrantRequest>,
) -> IssuedKey {
    app.issue_key(admin, &issue_req(role, principal, grants))
        .await
        .expect("issue key")
}

/// The common fixture: a key whose one grant is its own role on `node`.
async fn issue(app: &TestApp, admin: &str, role: Role, principal: &str, node: Uuid) -> IssuedKey {
    issue_with(app, admin, role, principal, vec![grant(node, role)]).await
}

/// A writer-role key holding writer and reader on `node`: the one key that
/// both remembers and recalls (ADR-0025 d1).
async fn agent(app: &TestApp, admin: &str, principal: &str, node: Uuid) -> IssuedKey {
    issue_with(
        app,
        admin,
        Role::Writer,
        principal,
        vec![grant(node, Role::Writer), grant(node, Role::Reader)],
    )
    .await
}

/// A first version of a claim of its own: the claim_key follows the
/// idempotency key, so distinct submissions never collide (ADR-0025 d18).
fn submit_req(idem: &str, body: &str) -> SubmitRequest {
    SubmitRequest {
        idempotency_key: idem.to_owned(),
        claim_key: format!("claim-{idem}"),
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workarea_id: None,
        derives_from: Vec::new(),
    }
}

fn submit_into(idem: &str, body: &str, workarea_id: Uuid) -> SubmitRequest {
    SubmitRequest {
        workarea_id: Some(workarea_id.into()),
        ..submit_req(idem, body)
    }
}

fn recall_req(query: &str) -> RecallRequest {
    RecallRequest {
        query: query.to_owned(),
        k: 10,
        workarea_id: None,
        kinds: Vec::new(),
        include_bodies: true,
        include_own_held: false,
        max_tokens: None,
    }
}

/// A supersession of `target` with `body`, under idempotency key `idem`:
/// no kind sent, no retirement, no ancestor beyond the target.
fn supersede_req(idem: &str, target: Uuid, body: &str) -> SupersedeRequest {
    SupersedeRequest {
        idempotency_key: idem.to_owned(),
        target_id: target,
        body: body.to_owned(),
        kind: None,
        retirement: false,
        derives_from: Vec::new(),
        reason: None,
        workarea_id: None,
        refuse_vocabulary_target: false,
    }
}

/// The retire-proposal of `target`: a supersession carrying `retirement`
/// whose body states why (ADR-0025 d11).
fn retire_req(idem: &str, target: Uuid, why: &str) -> SupersedeRequest {
    SupersedeRequest {
        retirement: true,
        ..supersede_req(idem, target, why)
    }
}

/// The ids of `key`'s recall hits for `query`, in rank order.
async fn hit_ids(app: &TestApp, key: &str, query: &str) -> Vec<Uuid> {
    app.recall(key, &recall_req(query))
        .await
        .expect("recall")
        .hits
        .iter()
        .map(|m| m.memory.memory_id)
        .collect()
}

/// The `derives_from` that get and the recall hit serve `key` for row `id`,
/// asserted equal: the two reads apply one readability test (ADR-0025 d3).
async fn served_lineage(app: &TestApp, key: &str, id: Uuid, query: &str) -> Vec<Uuid> {
    let got = app.get_memory(key, id).await.expect("get");
    let recall = app.recall(key, &recall_req(query)).await.expect("recall");
    let hit = recall
        .hits
        .iter()
        .find(|m| m.memory.memory_id == id)
        .expect("the row is a recall hit");
    assert_eq!(
        got.memory.derives_from, hit.memory.derives_from,
        "get and recall agree"
    );
    got.memory.derives_from
}

/// Submit as `writer` and have `dreamer` approve: the promoted row's id.
async fn promoted(app: &TestApp, writer: &str, dreamer: &str, req: &SubmitRequest) -> Uuid {
    let id = app
        .submit_memory(writer, req)
        .await
        .expect("submit")
        .receipt
        .memory_id;
    app.post_verdict(dreamer, id, Verdict::Approve, None)
        .await
        .expect("approve");
    id
}

#[tokio::test]
async fn happy_path_end_to_end() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda-writer", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana-dreamer", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea-reader", crew).await;

    // Submit: held on arrival, scan pass.
    let submitted = app
        .submit_memory(
            &writer.value,
            &submit_req("idem-1", "The canary stage must drain before promotion."),
        )
        .await
        .expect("submit");
    assert!(!submitted.replayed);
    assert_eq!(submitted.receipt.state, "held");
    assert_eq!(submitted.receipt.workarea_id, crew, "the key's default");
    let memory_id = submitted.receipt.memory_id;

    // Held rows are invisible to everyone but the author (R3).
    let err = app.get_memory(&reader.value, memory_id).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound));
    let own = app
        .get_memory(&writer.value, memory_id)
        .await
        .expect("own-work read");
    assert_eq!(own.memory.status, MemoryStatus::Held);

    // Recall before promotion finds nothing.
    let recall = app
        .recall(&reader.value, &recall_req("canary"))
        .await
        .expect("recall");
    assert!(recall.hits.is_empty());

    // Review: the dreamer is served the held item and approves it.
    let served = app
        .review_next(&dreamer.value)
        .await
        .expect("review next")
        .expect("an item waits");
    assert_eq!(served.memory.memory_id, memory_id);
    let outcome = app
        .post_verdict(&dreamer.value, memory_id, Verdict::Approve, None)
        .await
        .expect("verdict");
    assert_eq!(outcome.state, MemoryStatus::Promoted);

    // Recall now finds it; steering precedes hits (empty segment in PoC).
    let recall = app
        .recall(&reader.value, &recall_req("canary"))
        .await
        .expect("recall");
    assert!(recall.steering.is_empty());
    assert_eq!(recall.hits.len(), 1);
    assert_eq!(recall.hits[0].memory.memory_id, memory_id);

    // Supersede the promoted tip: the successor is held, and the target
    // stays the tip until a non-author ratifies (ADR-0025 d10).
    let superseded = app
        .supersede(
            &writer.value,
            &supersede_req(
                "sup-1",
                memory_id,
                "The canary stage must drain and settle before promotion.",
            ),
        )
        .await
        .expect("supersede");
    assert_eq!(superseded.receipt.replaces.artifact_id, memory_id);
    let old = app
        .get_memory(&reader.value, memory_id)
        .await
        .expect("get old tip");
    assert_eq!(old.memory.status, MemoryStatus::Promoted);
    assert!(
        old.memory.is_tip,
        "the target stays the tip until ratification"
    );
    let successor = app
        .get_memory(&writer.value, superseded.receipt.memory_id)
        .await
        .expect("author reads own held successor");
    assert_eq!(successor.memory.status, MemoryStatus::Held);
    assert_eq!(successor.memory.replaces, Some(memory_id));

    // The whole chain verifies: genesis, root, admin key, crew, three keys
    // of one grant each (a key.issued and a grant.issued apiece), submit,
    // approve, promote, the successor's submission = 14 events.
    let head = app.verify_chain(admin).await.expect("verify chain");
    assert_eq!(head.seq, 14);
    assert_eq!(head.tenant_id, boot.tenant_id);

    // The reader's tail serves only what it may see, ascending, with
    // digest-only payloads: the crew's creation and the promotion, never
    // another key's submission, a verdict or key administration (ADR-0025
    // d15).
    let tail = app
        .tail_events(&reader.value, None, 0, 100)
        .await
        .expect("tail");
    let types: Vec<&str> = tail
        .events
        .iter()
        .map(|e| e.event.envelope.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        [event_types::WORKAREA_CREATE, event_types::LIFECYCLE_PROMOTE]
    );
    assert_eq!((tail.head_seq, tail.next_after_seq), (14, 14));
    let promote = &tail.events[1];
    assert!(!promote.names_actor, "the tail names no reviewer");
    assert!(
        promote.payload.get("body").is_none(),
        "digest-only payloads"
    );
}

#[tokio::test]
async fn a_ratified_successor_is_the_one_hit_and_the_version_it_replaced_stays_promoted() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rita", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let second = app
        .supersede(
            &writer.value,
            &supersede_req("sup-1", first, "the canary drains and settles first"),
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    app.post_verdict(&dreamer.value, second, Verdict::Approve, None)
        .await
        .expect("ratify the successor");

    // A superseded version is never a hit (retrieval.md; ADR-0025 d8).
    let recall = app
        .recall(&reader.value, &recall_req("canary"))
        .await
        .expect("recall");
    let hits: Vec<_> = recall.hits.iter().map(|m| m.memory.memory_id).collect();
    assert_eq!(hits, vec![second]);

    // It stays fetchable by id as history, still promoted (ADR-0025 d10).
    let old = app
        .get_memory(&reader.value, first)
        .await
        .expect("get the replaced version");
    assert_eq!(old.memory.status, MemoryStatus::Promoted);
    assert!(!old.memory.is_tip);
}

#[tokio::test]
async fn r2_author_cannot_verdict_its_own_work() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    // A dreamer key held by the SAME principal as the author.
    let same_principal_dreamer = issue(&app, admin, Role::Dreamer, "wanda", crew).await;
    let other_dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;

    let submitted = app
        .submit_memory(&writer.value, &submit_req("idem-1", "fact one"))
        .await
        .expect("submit");
    let memory_id = submitted.receipt.memory_id;

    // The author's own (writer) key is outside the verdict ceiling entirely.
    let err = app
        .post_verdict(&writer.value, memory_id, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        AppError::VerbCeiling {
            role: Role::Writer,
            ..
        }
    ));

    // The same principal behind a dreamer key is refused by the lineage
    // closure (R2), and the queue never serves it the row at all.
    let served = app
        .review_next(&same_principal_dreamer.value)
        .await
        .expect("review next");
    assert!(served.is_none(), "the queue skips lineage-excluded rows");
    let err = app
        .post_verdict(
            &same_principal_dreamer.value,
            memory_id,
            Verdict::Approve,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        AppError::Verdict(oml_app::domain::VerdictError::PrincipalInLineage(_))
    ));

    // An independent dreamer may judge it.
    let outcome = app
        .post_verdict(&other_dreamer.value, memory_id, Verdict::Approve, None)
        .await
        .expect("independent verdict");
    assert_eq!(outcome.state, MemoryStatus::Promoted);
}

#[tokio::test]
async fn role_ceilings_are_refused_with_typed_errors() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;

    // Reader cannot write (R1).
    let err = app
        .submit_memory(&reader.value, &submit_req("i", "x"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        AppError::VerbCeiling {
            role: Role::Reader,
            ..
        }
    ));
    // Dreamer cannot write.
    let err = app
        .submit_memory(&dreamer.value, &submit_req("i", "x"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        AppError::VerbCeiling {
            role: Role::Dreamer,
            ..
        }
    ));
    // A writer key never reviews (refusal order step 1).
    let err = app.review_next(&writer.value).await.unwrap_err();
    assert!(matches!(
        err,
        AppError::VerbCeiling {
            role: Role::Writer,
            ..
        }
    ));
    // Writer cannot verify the chain (admin verb).
    let err = app.verify_chain(&writer.value).await.unwrap_err();
    assert!(matches!(
        err,
        AppError::VerbCeiling {
            role: Role::Writer,
            ..
        }
    ));
    // Reader cannot issue keys.
    let err = app
        .issue_key(&reader.value, &issue_req(Role::Reader, "p", Vec::new()))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        AppError::VerbCeiling {
            role: Role::Reader,
            ..
        }
    ));
    // Admin cannot mint an operator key (issuer ceiling), and the refusal
    // names the roles it may issue (ADR-0025 d2).
    let err = app
        .issue_key(admin, &issue_req(Role::Operator, "p", Vec::new()))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::IssuerCeiling {
                role: Role::Operator,
                admissible,
                grant_index: None,
            } if admissible == issuable_key_roles(Role::Admin)
        ),
        "{err:?}"
    );
    // An unknown key fails closed.
    let err = app
        .recall(
            "oml_0000000000000000000000000000000000000000000",
            &recall_req("x"),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Unauthorized));
}

#[tokio::test]
async fn idempotent_submit_replays_the_first_result() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda", crew).await;

    let first = app
        .submit_memory(&writer.value, &submit_req("idem-7", "fact body"))
        .await
        .expect("first submit");
    assert!(!first.replayed);

    // Same key, same request: the stored response, byte for byte, no second
    // version (http-api.md idempotency).
    let replay = app
        .submit_memory(&writer.value, &submit_req("idem-7", "fact body"))
        .await
        .expect("replay");
    assert!(replay.replayed);
    assert_eq!(replay.receipt, first.receipt);
    let head = app
        .verify_chain(&boot.admin_key.value)
        .await
        .expect("verify");
    assert_eq!(
        head.seq, 7,
        "replay journalled nothing: genesis, root, admin key, crew, the writer's key and grant, 1 submit"
    );

    // Same key, different request: refused, never silently honored.
    let err = app
        .submit_memory(&writer.value, &submit_req("idem-7", "a different body"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::IdempotencyDigestMismatch));
}

#[tokio::test]
async fn a_scan_veto_journals_nothing_and_leaves_no_row() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, &boot.admin_key.value, Role::Dreamer, "dana", crew).await;
    let before = journal(&store, boot.tenant_id).await.len();
    let outbox = store.outbox_len(boot.tenant_id);

    let err = app
        .submit_memory(
            &writer.value,
            &submit_req("idem-1", "-----BEGIN PRIVATE KEY----- oops"),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ScanVeto(_)));

    // A veto is a refusal like any other: no event, so no fold and no row,
    // and nothing waits in the queue (admission.md; errors.md
    // submission.scan_veto).
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    assert_eq!(store.outbox_len(boot.tenant_id), outbox);
    let served = app.review_next(&dreamer.value).await.expect("review next");
    assert!(served.is_none(), "nothing waits in the queue after a veto");

    // No idempotency record either: the same key with a clean body is new
    // work, not a digest mismatch, and its claim_key is free.
    let clean = app
        .submit_memory(&writer.value, &submit_req("idem-1", "a clean body"))
        .await
        .expect("the retry is a fresh capture");
    assert!(!clean.replayed);
    assert_eq!(clean.receipt.version, 1);
}

#[tokio::test]
async fn supersede_is_author_only_and_tip_only() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let other_writer = agent(&app, admin, "walt", crew).await;
    let writer_only = issue(&app, admin, Role::Writer, "wren", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;

    let submitted = app
        .submit_memory(&writer.value, &submit_req("i1", "original fact"))
        .await
        .expect("submit");
    let memory_id = submitted.receipt.memory_id;
    let revise = |body: &str| supersede_req(&format!("sup-{body}"), memory_id, body);

    // A held row has no promoted tip to supersede.
    let err = app
        .supersede(&writer.value, &revise("v2"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::TipConflict { .. }), "{err:?}");

    app.post_verdict(&dreamer.value, memory_id, Verdict::Approve, None)
        .await
        .expect("promote");

    // Another author that can read the target is refused
    // (access.author_only); one that cannot read it is told nothing exists
    // (ADR-0025 d1, d10).
    let err = app
        .supersede(&other_writer.value, &revise("v2"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AuthorOnly), "{err:?}");
    let err = app
        .supersede(&writer_only.value, &revise("v2"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");

    // Held successors of one tip coexist, each at its own ordinal, and
    // none blocks another (ADR-0025 d10).
    let v2 = app
        .supersede(&writer.value, &revise("v2"))
        .await
        .expect("supersede");
    let v3 = app
        .supersede(&writer.value, &revise("v3"))
        .await
        .expect("a second held successor of the same tip");
    assert_eq!(
        (v2.receipt.version, v3.receipt.version),
        (2, 3),
        "1 + the greatest version of the claim_key, whatever its state"
    );
}

// ---------------------------------------------------------------------------
// Claim conformance: kind, version, NFC, the claim_key (ADR-0025 d6, d10,
// d18)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_second_version_one_on_a_live_claim_key_is_claim_key_conflict() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let library = workarea(&app, admin, "library", boot.root_workarea_id).await;
    let writer = issue_with(
        &app,
        admin,
        Role::Writer,
        "wanda",
        vec![grant(crew, Role::Writer), grant(library, Role::Writer)],
    )
    .await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let fleet = |idem: &str, workarea: Uuid| SubmitRequest {
        claim_key: "fleet-order".to_owned(),
        ..submit_into(idem, "the fleet follows the canary", workarea)
    };

    let first = app
        .submit_memory(&writer.value, &fleet("i1", crew))
        .await
        .expect("a new claim")
        .receipt;
    assert_eq!(first.version, 1);

    // A held version holds the key: another version 1 collides, and the
    // refusal journals nothing.
    let head = journal(&store, boot.tenant_id).await.len();
    let err = app
        .submit_memory(&writer.value, &fleet("i2", crew))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ClaimKeyConflict), "{err:?}");
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);

    // So does a promoted one.
    app.post_verdict(&dreamer.value, first.memory_id, Verdict::Approve, None)
        .await
        .expect("approve");
    let err = app
        .submit_memory(&writer.value, &fleet("i3", crew))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ClaimKeyConflict), "{err:?}");

    // A claim_key is namespaced by workarea.
    let elsewhere = app
        .submit_memory(&writer.value, &fleet("i4", library))
        .await
        .expect("another workarea's claim")
        .receipt;
    assert_eq!(elsewhere.version, 1);
}

#[tokio::test]
async fn a_rejected_first_version_releases_its_claim_key() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let fleet = |idem: &str| SubmitRequest {
        claim_key: "fleet-order".to_owned(),
        ..submit_req(idem, "the fleet follows the canary")
    };

    let rejected = app
        .submit_memory(&writer.value, &fleet("i1"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    app.post_verdict(
        &dreamer.value,
        rejected,
        Verdict::Reject,
        Some("the canary is gone"),
    )
    .await
    .expect("reject");

    // Never promoted, the rejection releases the key for a fresh version 1
    // (audit-and-lineage.md; ADR-0025 d18).
    let fresh = app
        .submit_memory(&writer.value, &fleet("i2"))
        .await
        .expect("the released key takes a fresh claim")
        .receipt;
    assert_ne!(fresh.memory_id, rejected);
    assert_eq!(fresh.version, 1);
    let old = app
        .get_memory(&writer.value, rejected)
        .await
        .expect("the author still reads its rejected row");
    assert_eq!(old.memory.status, MemoryStatus::Rejected);
    assert_eq!(old.memory.claim_key, "fleet-order");
    assert_eq!(old.memory.version, 1);
}

#[tokio::test]
async fn submission_digests_nfc_text() {
    let decomposed = "the cafe\u{301} quote holds";
    let composed = "the caf\u{e9} quote holds";
    assert_ne!(decomposed, composed, "two spellings of one text");

    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda", crew).await;

    let first = app
        .submit_memory(
            &writer.value,
            &SubmitRequest {
                kind: MemoryKind::DerivedFact,
                ..submit_req("i1", decomposed)
            },
        )
        .await
        .expect("submit")
        .receipt;
    let second = app
        .submit_memory(&writer.value, &submit_req("i2", composed))
        .await
        .expect("submit")
        .receipt;

    // Capture normalizes before the digest: both spellings digest as the
    // NFC text (admission.md capture; ADR-0025 d6).
    let nfc_digest = sha256_digest_string(composed.as_bytes());
    assert_eq!(first.body_digest, nfc_digest);
    assert_eq!(second.body_digest, nfc_digest);
    let stored = app
        .get_memory(&writer.value, first.memory_id)
        .await
        .expect("own read")
        .memory;
    assert_eq!(stored.body, composed, "the NFC text is what is stored");
    assert_eq!(stored.body_bytes, composed.len() as u64);
    assert_eq!(stored.kind, MemoryKind::DerivedFact);
    assert_eq!(stored.version, 1);

    // The submission event carries the catalog payload: the NFC digest and
    // byte count, the version and the kind (event-catalog.md).
    let event = journal(&store, boot.tenant_id)
        .await
        .into_iter()
        .find(|e| e.envelope.event_type == event_types::SUBMISSION_REMEMBER)
        .expect("the submission is journalled")
        .envelope;
    assert_eq!(event.payload["plaintext_digest"], nfc_digest.as_str());
    assert_eq!(event.payload["body_bytes"], composed.len());
    assert_eq!(event.payload["version"], 1);
    assert_eq!(event.payload["kind"], "derived_fact");
    assert_eq!(event.payload["retirement"], false);
    assert!(event.payload["replaces"].is_null());
    assert!(event.payload.get("body").is_none(), "digest-only payloads");

    // The scan sees the NFC text too: a rule on the composed spelling
    // vetoes the decomposed one.
    let scanning = App::new(
        MemStore::new(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner {
            veto_marker: "caf\u{e9}".to_owned(),
            hold_marker: "[needs-human]".to_owned(),
        },
    );
    let (boot, crew) = tenant(&scanning).await;
    let writer = issue(
        &scanning,
        &boot.admin_key.value,
        Role::Writer,
        "wanda",
        crew,
    )
    .await;
    let err = scanning
        .submit_memory(&writer.value, &submit_req("i1", decomposed))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ScanVeto(_)), "{err:?}");
}

#[tokio::test]
async fn a_successor_takes_the_next_version_and_the_targets_kind() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &SubmitRequest {
            kind: MemoryKind::DerivedFact,
            ..submit_req("i1", "the canary drains first")
        },
    )
    .await;
    let successor = app
        .supersede(
            &writer.value,
            &supersede_req("sup-1", first, "the canary drains and settles first"),
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    let read = app
        .get_memory(&writer.value, successor)
        .await
        .expect("own read")
        .memory;
    assert_eq!(read.version, 2, "1 + the greatest version of the claim_key");
    assert_eq!(
        read.kind,
        MemoryKind::DerivedFact,
        "an omitted kind is the target's"
    );
    assert_eq!(read.claim_key, "claim-i1");
    assert_eq!(read.replaces, Some(first));
    assert!(!read.is_tip, "a held successor is not the tip");
}

#[tokio::test]
async fn replaces_names_the_replaced_version_only_to_a_key_that_can_read_it() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let approver = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    // Dreamer only: it reviews on crew but reads nothing there.
    let blind = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let sighted = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dina",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;

    // Two claims, each with a held successor: the queue serves them in
    // capture order, one to each reviewer.
    let mut pairs = Vec::new();
    for idem in ["i1", "i2"] {
        let first = promoted(
            &app,
            &writer.value,
            &approver.value,
            &submit_req(idem, "the canary drains first"),
        )
        .await;
        let successor = app
            .supersede(
                &writer.value,
                &supersede_req(
                    &format!("sup-{idem}"),
                    first,
                    "the canary drains and settles first",
                ),
            )
            .await
            .expect("supersede")
            .receipt
            .memory_id;
        pairs.push((first, successor));
    }
    let replaced = |first| {
        Some(VersionRef {
            artifact_id: first,
            version: 1,
        })
    };

    // The author reads its own target: replaces names it with its version.
    let (first, successor) = pairs[0];
    let own = app
        .get_memory(&writer.value, successor)
        .await
        .expect("own read");
    assert_eq!(own.replaces, replaced(first));

    // A reviewer that cannot read the target is served the successor with
    // replaces null, as its derives_from hides the target too (ADR-0025 d3).
    let served = app
        .review_next(&blind.value)
        .await
        .expect("review next")
        .expect("a successor waits");
    assert_eq!(served.memory.memory_id, successor);
    assert_eq!(served.replaces, None);
    assert!(served.memory.derives_from.is_empty());

    // One that can read it is served replaces with the version.
    let (first, successor) = pairs[1];
    let served = app
        .review_next(&sighted.value)
        .await
        .expect("review next")
        .expect("a successor waits");
    assert_eq!(served.memory.memory_id, successor);
    assert_eq!(served.replaces, replaced(first));
    assert_eq!(served.memory.derives_from, vec![first]);
}

// ---------------------------------------------------------------------------
// Effective access on every verb (ADR-0025 d1, d3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn one_writer_key_with_writer_and_reader_grants_remembers_and_recalls() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let agent = agent(&app, admin, "crew/agent-7", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "crew/reviewer-3", crew).await;

    let id = promoted(
        &app,
        &agent.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let recall = app
        .recall(&agent.value, &recall_req("canary"))
        .await
        .expect("the one key recalls what it remembered");
    assert_eq!(
        recall
            .hits
            .iter()
            .map(|m| m.memory.memory_id)
            .collect::<Vec<_>>(),
        vec![id]
    );
}

#[tokio::test]
async fn a_writer_without_a_reader_grant_is_told_not_found_on_recall() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda", crew).await;

    // Reader is admissible under the writer role, so step 1 passes; the
    // scope holds writer only, so the node is not readable (refusal order
    // step 4).
    let err = app
        .recall(&writer.value, &recall_req("x"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
}

#[tokio::test]
async fn a_submit_on_a_node_the_key_only_reads_is_grant_refused() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let library = workarea(&app, admin, "library", boot.root_workarea_id).await;
    let writer = issue_with(
        &app,
        admin,
        Role::Writer,
        "wanda",
        vec![grant(crew, Role::Writer), grant(library, Role::Reader)],
    )
    .await;

    let err = app
        .submit_memory(&writer.value, &submit_into("i1", "x", library))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::GrantRefused {
                workarea_id,
                needed_role: Role::Writer,
            } if workarea_id == library
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn an_x_oml_workarea_outside_the_grants_is_not_found() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let sibling = workarea(&app, admin, "sibling", boot.root_workarea_id).await;
    let agent = agent(&app, admin, "crew/agent-7", crew).await;

    // A sibling, an ancestor of the granted node, and a node no tenant holds
    // all answer as absence: a grant covers its subtree only.
    for node in [sibling, boot.root_workarea_id, Uuid::from_u128(0xDEAD)] {
        let err = app
            .submit_memory(&agent.value, &submit_into("i1", "x", node))
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::NotFound),
            "submit on {node}: {err:?}"
        );
        let err = app
            .recall(
                &agent.value,
                &RecallRequest {
                    workarea_id: Some(node.into()),
                    ..recall_req("x")
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::NotFound),
            "recall on {node}: {err:?}"
        );
    }
}

#[tokio::test]
async fn recall_with_neither_default_nor_override_is_workarea_unresolved() {
    let app = app();
    let (boot, _crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    // A zero-grant key has no default (ADR-0025 d2).
    let reader = issue_with(&app, admin, Role::Reader, "rhea", Vec::new()).await;
    assert_eq!(reader.default_workarea, None);

    let err = app
        .recall(&reader.value, &recall_req("x"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::WorkareaUnresolved), "{err:?}");
    // Step 1 runs before the workarea resolves: a reader submitting is the
    // verb ceiling whether or not it has a default.
    let err = app
        .submit_memory(&reader.value, &submit_req("i1", "x"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::VerbCeiling {
                role: Role::Reader,
                ..
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_global_denial_takes_a_granted_subtree_out_of_recall() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quarantine = workarea(&app, admin, "quarantine", crew).await;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;

    let kept = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let masked = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_into("i2", "the canary in quarantine", quarantine),
    )
    .await;
    let before = app
        .recall(&reader.value, &recall_req("canary"))
        .await
        .expect("recall");
    assert_eq!(before.hits.len(), 2, "the subtree is searched");

    let denial = app
        .deny_key(admin, reader.key_id.into(), quarantine.into())
        .await
        .expect("deny");
    assert_eq!(denial.denial.node, quarantine);
    assert!(
        denial.epoch > reader.access.issued_seq,
        "the denial's epoch is its own key.denied seq"
    );

    let after = app
        .recall(&reader.value, &recall_req("canary"))
        .await
        .expect("recall");
    assert_eq!(
        after
            .hits
            .iter()
            .map(|m| m.memory.memory_id)
            .collect::<Vec<_>>(),
        vec![kept],
        "deny-wins over the reader grant"
    );
    let err = app
        .recall(
            &reader.value,
            &RecallRequest {
                workarea_id: Some(quarantine.into()),
                ..recall_req("canary")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let err = app.get_memory(&reader.value, masked).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");

    // Global denials mask the key's own rows too (roles-and-keys.md#own).
    app.deny_key(admin, writer.key_id.into(), quarantine.into())
        .await
        .expect("deny the author");
    let err = app.get_memory(&writer.value, masked).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    app.get_memory(&writer.value, kept)
        .await
        .expect("an own row outside the denial");

    // Only an admin key journals a denial; an unknown key or node is absent.
    let err = app
        .deny_key(&writer.value, reader.key_id.into(), crew.into())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::VerbCeiling {
                role: Role::Writer,
                ..
            }
        ),
        "{err:?}"
    );
    for (key_id, node) in [
        (Uuid::from_u128(0xBEEF), crew),
        (reader.key_id, Uuid::from_u128(0xBEEF)),
    ] {
        let err = app
            .deny_key(admin, key_id.into(), node.into())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
}

#[tokio::test]
async fn a_dreamer_is_served_only_held_rows_inside_its_dreamer_grants() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let other = workarea(&app, admin, "other-crew", boot.root_workarea_id).await;
    let masked = workarea(&app, admin, "masked", crew).await;
    let writer = issue_with(
        &app,
        admin,
        Role::Writer,
        "wanda",
        vec![grant(crew, Role::Writer), grant(other, Role::Writer)],
    )
    .await;
    let dreamer = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dana",
        vec![
            GrantRequest {
                node: crew.into(),
                role: Role::Dreamer,
                deny: vec![masked.into()],
            },
            grant(other, Role::Reader),
        ],
    )
    .await;

    let inside = app
        .submit_memory(&writer.value, &submit_req("i1", "inside"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    for (idem, node) in [("i2", other), ("i3", masked)] {
        app.submit_memory(&writer.value, &submit_into(idem, "outside", node))
            .await
            .expect("submit");
    }

    // The other crew's row is readable only, and the masked row lies under
    // the grant's own deny: neither is ever served.
    let served = app
        .review_next(&dreamer.value)
        .await
        .expect("review next")
        .expect("the row inside waits");
    assert_eq!(served.memory.memory_id, inside);
    app.post_verdict(&dreamer.value, inside, Verdict::Approve, None)
        .await
        .expect("approve");
    let served = app.review_next(&dreamer.value).await.expect("review next");
    assert!(served.is_none(), "{served:?}");
}

#[tokio::test]
async fn a_verdict_on_a_row_outside_dreamer_access_is_not_found() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let other = workarea(&app, admin, "other-crew", boot.root_workarea_id).await;
    let writer = issue(&app, admin, Role::Writer, "wanda", other).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let reading_dreamer = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dora",
        vec![grant(crew, Role::Dreamer), grant(other, Role::Reader)],
    )
    .await;

    let row = app
        .submit_memory(&writer.value, &submit_req("i1", "elsewhere"))
        .await
        .expect("submit")
        .receipt
        .memory_id;

    let err = app
        .post_verdict(&dreamer.value, row, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    // Where reader is effective, the refusal names the missing role
    // (refusal order step 3), with no exception for review.
    let err = app
        .post_verdict(&reading_dreamer.value, row, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::GrantRefused {
                workarea_id,
                needed_role: Role::Dreamer,
            } if workarea_id == other
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn r2_refuses_the_author_whatever_grants_its_dreamer_key_holds() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let author_dreamer = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "wanda",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;

    let row = app
        .submit_memory(&writer.value, &submit_req("i1", "fact one"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    let served = app
        .review_next(&author_dreamer.value)
        .await
        .expect("review next");
    assert!(served.is_none(), "the queue skips lineage-excluded rows");
    let err = app
        .post_verdict(&author_dreamer.value, row, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Verdict(oml_app::domain::VerdictError::PrincipalInLineage(_))
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_service_key_is_refused_review_before_any_lookup() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let repair = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                held_by: Some(HeldBy::Service),
                ..issue_req(
                    Role::Dreamer,
                    "reconciler",
                    vec![grant(crew, Role::Dreamer)],
                )
            },
        )
        .await
        .expect("issue a service key");
    app.submit_memory(&writer.value, &submit_req("i1", "fact one"))
        .await
        .expect("submit");

    // Its dreamer grant confers review like any other; the attestation, not
    // the grant, keeps it out (ADR-0025 d1).
    let err = app.review_next(&repair.value).await.unwrap_err();
    assert!(
        matches!(
            err,
            AppError::VerbCeiling {
                role: Role::Dreamer,
                ..
            }
        ),
        "{err:?}"
    );
    let err = app
        .post_verdict(
            &repair.value,
            Uuid::from_u128(0xDEAD),
            Verdict::Approve,
            None,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::VerbCeiling {
                role: Role::Dreamer,
                ..
            }
        ),
        "refused before the row is looked up: {err:?}"
    );
}

#[tokio::test]
async fn another_key_of_the_same_principal_cannot_get_a_held_row() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let first = agent(&app, admin, "crew/agent-7", crew).await;
    let second = agent(&app, admin, "crew/agent-7", crew).await;

    let row = app
        .submit_memory(&first.value, &submit_req("i1", "a held fact"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    app.get_memory(&first.value, row)
        .await
        .expect("the authoring key reads its own held row");
    // Own is the authoring key, never the principal (ADR-0025 d3).
    let err = app.get_memory(&second.value, row).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
}

#[tokio::test]
async fn an_unreadable_derives_from_target_is_not_found() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let first = agent(&app, admin, "crew/agent-7", crew).await;
    let second = agent(&app, admin, "crew/agent-8", crew).await;
    let writer_only = issue(&app, admin, Role::Writer, "wren", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;

    let held = app
        .submit_memory(&first.value, &submit_req("i1", "a held fact"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    let derive = |idem: &str| SubmitRequest {
        claim_key: format!("derived-{idem}"),
        derives_from: vec![held],
        ..submit_req(idem, "a derived fact")
    };

    // Another key's held row is indistinguishable from no row at all.
    let err = app
        .submit_memory(&second.value, &derive("d1"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::LineageUnknown(id) if id == held),
        "{err:?}"
    );
    // The author derives from its own row in any state.
    app.submit_memory(&first.value, &derive("d2"))
        .await
        .expect("own held ancestor");

    // Once promoted, a reader of its workarea may derive from it; a key
    // with no reader grant there still may not.
    app.post_verdict(&dreamer.value, held, Verdict::Approve, None)
        .await
        .expect("approve");
    app.submit_memory(&second.value, &derive("d3"))
        .await
        .expect("promoted ancestor within reader access");
    let err = app
        .submit_memory(&writer_only.value, &derive("d4"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::LineageUnknown(id) if id == held),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_promoted_row_derived_from_its_authors_held_row_names_it_to_no_other_key() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let author = agent(&app, admin, "crew/agent-7", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;

    let held = app
        .submit_memory(&author.value, &submit_req("i1", "the canary drains first"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    let derived = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &SubmitRequest {
            claim_key: "fleet-order".to_owned(),
            derives_from: vec![held],
            ..submit_req("i2", "the fleet follows the canary")
        },
    )
    .await;

    // The reader cannot read the held ancestor, so neither get nor recall
    // names it (ADR-0025 d3); its author still sees its own row there.
    let err = app.get_memory(&reader.value, held).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    assert_eq!(
        served_lineage(&app, &reader.value, derived, "fleet").await,
        Vec::<Uuid>::new()
    );
    assert_eq!(
        served_lineage(&app, &author.value, derived, "fleet").await,
        vec![held]
    );

    // Once the ancestor is promoted the reader can read it, and it is named.
    app.post_verdict(&dreamer.value, held, Verdict::Approve, None)
        .await
        .expect("approve the ancestor");
    assert_eq!(
        served_lineage(&app, &reader.value, derived, "fleet").await,
        vec![held]
    );
}

// ---------------------------------------------------------------------------
// Recall: the steering segment and the hit order (ADR-0025 d8)
// ---------------------------------------------------------------------------

/// A first version of `kind` in `workarea_id`.
fn submit_kind(idem: &str, kind: MemoryKind, body: &str, workarea_id: Uuid) -> SubmitRequest {
    SubmitRequest {
        kind,
        ..submit_into(idem, body, workarea_id)
    }
}

/// The steering ids a recall served, in order.
fn steering_ids(recall: &RecallResult) -> Vec<Uuid> {
    recall.steering.iter().map(|m| m.memory.memory_id).collect()
}

/// The hit ids a recall served, in order.
fn hit_ids_of(recall: &RecallResult) -> Vec<Uuid> {
    recall.hits.iter().map(|h| h.memory.memory_id).collect()
}

/// A recall of `query` at `scope`.
fn recall_at(query: &str, scope: Uuid) -> RecallRequest {
    RecallRequest {
        workarea_id: Some(scope.into()),
        ..recall_req(query)
    }
}

#[tokio::test]
async fn a_promoted_steering_row_precedes_hits_unranked() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    let promote = |req: SubmitRequest| {
        let (writer, dreamer) = (writer.value.clone(), dreamer.value.clone());
        let app = &app;
        async move { promoted(app, &writer, &dreamer, &req).await }
    };

    // One steering row misses the query and the other matches it twice:
    // steering applies by scope and is ordered by promotion, never by the
    // query (retrieval.md, the recall-ordering contract).
    let quiet = promote(submit_kind(
        "s1",
        MemoryKind::Steering,
        "Quote validity dates are mandatory.",
        crew,
    ))
    .await;
    let loud = promote(submit_kind(
        "s2",
        MemoryKind::Steering,
        "canary drain: drain the canary first",
        crew,
    ))
    .await;
    let weaker = promote(submit_req("m1", "the canary drained")).await;
    let stronger = promote(submit_req("m2", "canary drain, then settle")).await;
    let equal = promote(submit_req("m3", "drain the canary slowly")).await;

    let recall = app
        .recall(&reader.value, &recall_req("canary drain"))
        .await
        .expect("recall");
    assert_eq!(steering_ids(&recall), vec![quiet, loud]);
    assert!(
        recall
            .steering
            .iter()
            .all(|m| m.memory.kind == MemoryKind::Steering),
        "steering is never a hit"
    );
    assert_eq!(
        hit_ids_of(&recall),
        vec![stronger, equal, weaker],
        "score descending, then artifact_id ascending"
    );
    let scores: Vec<f32> = recall.hits.iter().map(|h| h.score).collect();
    assert!(
        scores[0] > scores[2],
        "the score is the store's rank: {scores:?}"
    );
    assert!(
        (scores[0] - scores[1]).abs() < f32::EPSILON,
        "two terms each: {scores:?}"
    );
}

#[tokio::test]
async fn ancestor_steering_applies_to_a_child_scope() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quotes = workarea(&app, admin, "quotes", crew).await;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;

    // The child's row is promoted first: depth from the root orders the
    // segment before promotion time does.
    let local = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_kind("s1", MemoryKind::Steering, "Quotes are in USD/t.", quotes),
    )
    .await;
    let standing = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_kind(
            "s2",
            MemoryKind::Steering,
            "Treat undated quotes as expired.",
            crew,
        ),
    )
    .await;

    let recall = app
        .recall(&reader.value, &recall_at("anything", quotes))
        .await
        .expect("recall");
    assert_eq!(steering_ids(&recall), vec![standing, local]);
    assert!(recall.hits.is_empty(), "steering is never a hit");
}

#[tokio::test]
async fn child_steering_does_not_apply_to_the_parent() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quotes = workarea(&app, admin, "quotes", crew).await;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    for (idem, kind, body) in [
        ("s1", MemoryKind::Steering, "Quote the canary in USD/t."),
        ("m1", MemoryKind::Memory, "the canary quote is 412 USD/t"),
    ] {
        promoted(
            &app,
            &writer.value,
            &dreamer.value,
            &submit_kind(idem, kind, body, quotes),
        )
        .await;
    }

    // The child is searched for hits at the parent; its steering belongs to
    // another crew and does not apply there.
    let recall = app
        .recall(&reader.value, &recall_req("canary"))
        .await
        .expect("recall");
    assert!(recall.steering.is_empty(), "{:?}", steering_ids(&recall));
    assert_eq!(recall.hits.len(), 1, "the subtree is searched");
}

#[tokio::test]
async fn steering_on_an_unreadable_ancestor_is_not_served() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quotes = workarea(&app, admin, "quotes", crew).await;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let standing = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_kind(
            "s1",
            MemoryKind::Steering,
            "Treat undated quotes as expired.",
            crew,
        ),
    )
    .await;

    // Steering is bounded by read access: a key that reads only the child
    // is served none of the parent's, and a key that reads the parent is.
    let narrow = issue(&app, admin, Role::Reader, "rhea", quotes).await;
    let recall = app
        .recall(&narrow.value, &recall_req("anything"))
        .await
        .expect("recall");
    assert!(recall.steering.is_empty(), "{:?}", steering_ids(&recall));
    let wide = issue(&app, admin, Role::Reader, "rhea-wide", crew).await;
    let recall = app
        .recall(&wide.value, &recall_at("anything", quotes))
        .await
        .expect("recall");
    assert_eq!(steering_ids(&recall), vec![standing]);
}

#[tokio::test]
async fn kinds_filters_hits_only() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    let mut ids = Vec::new();
    for (idem, kind, body) in [
        (
            "s1",
            MemoryKind::Steering,
            "Drain the canary before any quote.",
        ),
        ("m1", MemoryKind::Memory, "the canary drained at noon"),
        (
            "d1",
            MemoryKind::DerivedFact,
            "the canary drains in an hour",
        ),
    ] {
        ids.push(
            promoted(
                &app,
                &writer.value,
                &dreamer.value,
                &submit_kind(idem, kind, body, crew),
            )
            .await,
        );
    }
    let [steering, memory, derived] = ids.try_into().expect("three rows");

    for (kinds, hits) in [
        (vec![MemoryKind::DerivedFact], vec![derived]),
        (vec![MemoryKind::Memory], vec![memory]),
        (Vec::new(), vec![memory, derived]),
    ] {
        let recall = app
            .recall(
                &reader.value,
                &RecallRequest {
                    kinds: kinds.clone(),
                    ..recall_req("canary")
                },
            )
            .await
            .expect("recall");
        assert_eq!(steering_ids(&recall), vec![steering], "kinds {kinds:?}");
        assert_eq!(hit_ids_of(&recall), hits, "kinds {kinds:?}");
    }
}

// ---------------------------------------------------------------------------
// Issuance (ADR-0025 d2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn issuance_never_inherits_the_issuers_default() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let sub = workarea(&app, admin, "sub", crew).await;

    // The issuer's default is the root; no issued key ever takes it.
    let zero = issue_with(&app, admin, Role::Writer, "w0", Vec::new()).await;
    assert_eq!(zero.default_workarea, None, "a zero-grant key has none");
    let first = issue_with(
        &app,
        admin,
        Role::Writer,
        "w1",
        vec![grant(crew, Role::Writer), grant(sub, Role::Reader)],
    )
    .await;
    assert_eq!(first.default_workarea, Some(crew), "the first grant's node");
    let named = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                default_workarea: Some(sub.into()),
                ..issue_req(Role::Writer, "w2", vec![grant(crew, Role::Writer)])
            },
        )
        .await
        .expect("issue");
    assert_eq!(
        named.default_workarea,
        Some(sub),
        "a covered default is kept"
    );

    // The journal says the same: key.issued carries the default it chose.
    let journalled: Vec<_> = journal(&store, boot.tenant_id)
        .await
        .iter()
        .filter(|e| e.envelope.event_type == event_types::KEY_ISSUED)
        .filter(|e| {
            [zero.key_id, first.key_id, named.key_id]
                .iter()
                .any(|k| e.envelope.payload["key_id"] == k.to_string())
        })
        .map(|e| e.envelope.payload["default_workarea_id"].clone())
        .collect();
    assert_eq!(
        journalled,
        vec![
            serde_json::Value::Null,
            serde_json::json!(crew.to_string()),
            serde_json::json!(sub.to_string()),
        ]
    );
}

#[tokio::test]
async fn a_default_no_grant_covers_is_refused() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let sibling = workarea(&app, admin, "sibling", boot.root_workarea_id).await;
    let masked = workarea(&app, admin, "masked", crew).await;
    let head = app.verify_chain(admin).await.expect("verify").seq;

    let masking = GrantRequest {
        node: crew.into(),
        role: Role::Writer,
        deny: vec![masked.into()],
    };
    for default in [sibling, masked, boot.root_workarea_id] {
        let err = app
            .issue_key(
                admin,
                &IssueKeyRequest {
                    default_workarea: Some(default.into()),
                    ..issue_req(Role::Writer, "w", vec![masking.clone()])
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&err, AppError::DefaultUncovered { workarea } if *workarea == Reference::Id(default)),
            "{err:?}"
        );
    }
    assert_eq!(
        app.verify_chain(admin).await.expect("verify").seq,
        head,
        "a refused issuance journals nothing"
    );
}

#[tokio::test]
async fn issuance_refuses_in_the_issuance_order_and_journals_nothing() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let sibling = workarea(&app, admin, "sibling", boot.root_workarea_id).await;
    let head = app.verify_chain(admin).await.expect("verify").seq;

    // 1. The two-key rule runs first, even where admissibility also fails.
    let err = app
        .issue_key(
            admin,
            &issue_req(Role::Admin, "a", vec![grant(crew, Role::Dreamer)]),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::RoleExclusive), "{err:?}");
    // 3. The first inadmissible grant, by its 0-based index.
    let err = app
        .issue_key(
            admin,
            &issue_req(
                Role::Writer,
                "w",
                vec![grant(crew, Role::Reader), grant(crew, Role::Dreamer)],
            ),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::IssuerCeiling {
                role: Role::Dreamer,
                admissible,
                grant_index: Some(1),
            } if admissible == admissible_grant_roles(Role::Writer)
        ),
        "{err:?}"
    );
    // 4. A grant node or deny node the tenant does not hold.
    for g in [
        grant(Uuid::from_u128(0xDEAD), Role::Writer),
        GrantRequest {
            node: crew.into(),
            role: Role::Writer,
            deny: vec![Uuid::from_u128(0xDEAD).into()],
        },
    ] {
        let err = app
            .issue_key(admin, &issue_req(Role::Writer, "w", vec![g]))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    // 5. A deny outside its own grant's subtree.
    let err = app
        .issue_key(
            admin,
            &issue_req(
                Role::Writer,
                "w",
                vec![
                    grant(sibling, Role::Reader),
                    GrantRequest {
                        node: crew.into(),
                        role: Role::Writer,
                        deny: vec![sibling.into()],
                    },
                ],
            ),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            &err,
            AppError::DenyOutsideGrant {
                grant_index: 1,
                subtree_node,
            } if *subtree_node == Reference::Id(sibling)
        ),
        "{err:?}"
    );
    assert_eq!(app.verify_chain(admin).await.expect("verify").seq, head);
}

#[tokio::test]
async fn issuance_journals_the_key_then_each_grant_with_its_own_seq_as_its_epoch() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let sub = workarea(&app, admin, "sub", crew).await;

    let issued = issue_with(
        &app,
        admin,
        Role::Writer,
        "crew/agent-7",
        vec![
            GrantRequest {
                node: crew.into(),
                role: Role::Writer,
                deny: vec![sub.into()],
            },
            grant(crew, Role::Reader),
        ],
    )
    .await;
    let epochs: Vec<u64> = issued.access.grants.iter().map(|g| g.epoch).collect();
    assert_eq!(
        epochs,
        vec![issued.access.issued_seq + 1, issued.access.issued_seq + 2]
    );
    assert_eq!(
        issued.access.permission_epoch(),
        issued.access.issued_seq + 2
    );
    assert_eq!(
        issued.access.grants[0].grant.deny,
        vec![sub],
        "never dropped"
    );

    let tail = journal(&store, boot.tenant_id).await;
    let at = |seq: u64| &tail[usize::try_from(seq - 1).expect("seq")].envelope;
    let key_issued = at(issued.access.issued_seq);
    assert_eq!(key_issued.event_type, event_types::KEY_ISSUED);
    assert_eq!(
        key_issued.payload,
        serde_json::json!({
            "key_id": issued.key_id.to_string(),
            "role": "writer",
            "principal": "crew/agent-7",
            "issued_by_key_id": boot.admin_key.key_id.to_string(),
            "expiry": null,
            "value_disclosed": false,
            "held_by": "agent",
            "default_workarea_id": crew.to_string(),
            // The name the issuance gave, and, sent no crew, a null crew
            // (event-catalog.md `key.issued`; ADR-0026 d8, d9).
            "name": issued.name.as_str(),
            "crew_id": null,
        })
    );
    for record in &issued.access.grants {
        let event = at(record.epoch);
        assert_eq!(event.event_type, event_types::GRANT_ISSUED);
        assert_eq!(
            event.payload,
            serde_json::json!({
                "grant_id": record.grant.grant_id.to_string(),
                "key_id": issued.key_id.to_string(),
                "workarea_node_id": record.grant.node.to_string(),
                "role": record.grant.role.as_str(),
                "deny": record
                    .grant
                    .deny
                    .iter()
                    .map(|d| serde_json::json!({ "subtree_node_id": d.to_string() }))
                    .collect::<Vec<_>>(),
                "epoch": record.epoch,
            })
        );
    }
    assert!(
        tail.iter()
            .all(|e| !e.envelope.payload.to_string().contains(&issued.value)),
        "the value is never journalled"
    );
}

#[tokio::test]
async fn a_failed_grant_commit_discloses_no_key_value() {
    let store = MemStore::new().refusing_next_commit_of(event_types::GRANT_ISSUED);
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;

    // The caller gets the store's refusal and no value; the value exists
    // nowhere else (ADR-0025 d2).
    let err = app
        .issue_key(
            admin,
            &issue_req(
                Role::Writer,
                "crew/agent-7",
                vec![grant(crew, Role::Writer), grant(crew, Role::Reader)],
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Store(_)), "{err:?}");

    // The key.issued that committed is followed by its revocation over the
    // fixed reason, and by no grant.
    let tail = journal(&store, boot.tenant_id).await;
    let types: Vec<&str> = tail
        .iter()
        .skip(4)
        .map(|e| e.envelope.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        vec![event_types::KEY_ISSUED, event_types::KEY_REVOKED],
        "genesis, root, admin key and crew, then the failed issuance"
    );
    let failed_key = tail[4].envelope.payload["key_id"].clone();
    assert_eq!(
        tail[5].envelope.payload,
        serde_json::json!({
            "key_id": failed_key,
            "revoked_by_key_id": boot.admin_key.key_id.to_string(),
            "reason_digest": sha256_digest_string(b"issuance_incomplete"),
        })
    );
    let key_id = Uuid::parse_str(failed_key.as_str().expect("key id")).expect("uuid");
    let access = store
        .key_access(boot.tenant_id, key_id)
        .await
        .expect("read")
        .expect("the revoked key stays as history");
    assert!(access.grants.is_empty(), "no grant was stored");

    // The refusal was one-shot: the next issuance completes.
    issue(&app, admin, Role::Reader, "rhea", crew).await;
}

// ---------------------------------------------------------------------------
// The workarea read: the caller's children, verbs and epoch (ADR-0025 d14)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn workarea_info_names_the_verbs_this_keys_grants_confer() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let agent = agent(&app, admin, "crew/agent-7", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    let dreamer = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dana",
        vec![grant(crew, Role::Reader), grant(crew, Role::Dreamer)],
    )
    .await;
    let service = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                held_by: Some(HeldBy::Service),
                ..issue_req(
                    Role::Dreamer,
                    "reconciler",
                    vec![grant(crew, Role::Reader), grant(crew, Role::Dreamer)],
                )
            },
        )
        .await
        .expect("issue a service key");

    // The union over the effective grant roles, in table order without
    // duplicates (roles-and-keys.md effective verbs).
    for (key, verbs) in [
        (
            &agent,
            vec![
                "recall",
                "read",
                "graph_read",
                "events_tail",
                "remember",
                "learn",
                "attach",
                "supersede",
            ],
        ),
        (&reader, vec!["recall", "read", "graph_read", "events_tail"]),
        (
            &dreamer,
            vec![
                "recall",
                "read",
                "graph_read",
                "events_tail",
                "propose",
                "review",
                "supersede",
            ],
        ),
        // A service key is refused review whatever its grants (ADR-0025 d1).
        (
            &service,
            vec![
                "recall",
                "read",
                "graph_read",
                "events_tail",
                "propose",
                "supersede",
            ],
        ),
    ] {
        let info = app
            .workarea_info(&key.value, Some(crew.into()))
            .await
            .expect("a readable node");
        assert_eq!(info.my_effective_verbs, verbs, "{}", key.principal);
    }
}

#[tokio::test]
async fn workarea_info_lists_only_readable_children() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let beta = workarea(&app, admin, "beta", crew).await;
    let sealed = workarea(&app, admin, "sealed", crew).await;
    let a_zulu = workarea(&app, admin, "a-zulu", crew).await;
    let alpha = workarea(&app, admin, "alpha", crew).await;
    let quarantine = workarea(&app, admin, "quarantine", crew).await;
    let reader = issue_with(
        &app,
        admin,
        Role::Reader,
        "rhea",
        vec![GrantRequest {
            node: crew.into(),
            role: Role::Reader,
            deny: vec![sealed.into()],
        }],
    )
    .await;
    app.deny_key(admin, reader.key_id.into(), quarantine.into())
        .await
        .expect("deny");

    // A child its grant's own deny masks and a child a global denial covers
    // are absent, not marked; the rest are ordered by path_leaf in byte
    // order, so "a-zulu" sorts before "alpha", where a collation that skips
    // the hyphen would put it after.
    let info = app
        .workarea_info(&reader.value, Some(crew.into()))
        .await
        .expect("a readable node");
    assert_eq!(
        info.children
            .iter()
            .map(|c| (c.workarea_id, c.name.as_str(), c.status()))
            .collect::<Vec<_>>(),
        vec![
            (a_zulu, "a-zulu", "development"),
            (alpha, "alpha", "development"),
            (beta, "beta", "development"),
        ]
    );
}

#[tokio::test]
async fn an_unreadable_workarea_is_not_found() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let sibling = workarea(&app, admin, "sibling", boot.root_workarea_id).await;
    let quarantine = workarea(&app, admin, "quarantine", crew).await;
    let agent = agent(&app, admin, "crew/agent-7", crew).await;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    app.deny_key(admin, agent.key_id.into(), quarantine.into())
        .await
        .expect("deny");

    // A sibling, an ancestor of the granted node, a node no tenant holds and
    // a globally denied child all answer as absence.
    for node in [
        sibling,
        boot.root_workarea_id,
        Uuid::from_u128(0xDEAD),
        quarantine,
    ] {
        let err = app
            .workarea_info(&agent.value, Some(node.into()))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{node}: {err:?}");
    }
    // Writer alone does not read the node (refusal order step 4).
    let err = app
        .workarea_info(&writer.value, Some(crew.into()))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    // An admin key reads structure by its key role, the denied child
    // included, and a node no tenant holds is absent to it too (ADR-0026
    // d15).
    for node in [crew, quarantine] {
        app.workarea_info(admin, Some(node.into()))
            .await
            .expect("the admin reads the structure");
    }
    let err = app
        .workarea_info(admin, Some(Uuid::from_u128(0xDEAD).into()))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
}

#[tokio::test]
async fn an_omitted_workarea_resolves_to_the_default_only_after_the_verb_ceiling() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let second_admin = issue_with(&app, admin, Role::Admin, "bob-admin", vec![]).await;
    assert_eq!(second_admin.default_workarea, None);
    let agent = agent(&app, admin, "crew/agent-7", crew).await;
    let unscoped = issue_with(&app, admin, Role::Reader, "rhea", vec![]).await;

    // An admin key reads structure by its key role (ADR-0026 d15), so the
    // default resolves for it as for any key past step 1: the bootstrap
    // admin reads the root it defaults to, and a zero-grant admin with no
    // default is refused by name.
    let root = app.workarea_info(admin, None).await.expect("the default");
    assert_eq!(root.workarea.workarea_id, boot.root_workarea_id);
    let err = app
        .workarea_info(&second_admin.value, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::WorkareaUnresolved), "{err:?}");
    // Past step 1, a key with no default is refused by name, and a key with
    // one reads it as if it had named it.
    let err = app.workarea_info(&unscoped.value, None).await.unwrap_err();
    assert!(matches!(err, AppError::WorkareaUnresolved), "{err:?}");
    assert_eq!(
        app.workarea_info(&agent.value, None)
            .await
            .expect("default"),
        app.workarea_info(&agent.value, Some(crew.into()))
            .await
            .expect("named")
    );
}

#[tokio::test]
async fn the_derived_path_follows_parent_ids_from_universe() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quarter = workarea(&app, admin, "q1-2027", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rhea", boot.root_workarea_id).await;

    let root = app
        .workarea_info(&reader.value, Some(boot.root_workarea_id.into()))
        .await
        .expect("the root");
    assert_eq!(root.path, "universe");
    assert_eq!(root.workarea.parent_id, None);
    // The universe root is permanently committed (workareas.md).
    assert_eq!(root.workarea.status(), "committed");

    let leaf = app
        .workarea_info(&reader.value, Some(quarter.into()))
        .await
        .expect("the leaf");
    assert_eq!(leaf.path, "universe.crew.q1-2027");
    assert_eq!(leaf.workarea.parent_id, Some(crew));
    assert_eq!(leaf.workarea.status(), "development");
    let middle = app
        .workarea_info(&reader.value, Some(crew.into()))
        .await
        .expect("the crew");
    assert_eq!(middle.path, "universe.crew");
    assert_eq!(
        middle
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["q1-2027"]
    );
}

#[tokio::test]
async fn the_permission_epoch_moves_when_a_global_denial_is_journalled() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quarantine = workarea(&app, admin, "quarantine", crew).await;
    let agent = agent(&app, admin, "crew/agent-7", crew).await;
    let epoch = || async {
        app.workarea_info(&agent.value, Some(crew.into()))
            .await
            .expect("a readable node")
            .permission_epoch
    };

    // The latest of the key's key.issued and grant.issued seqs.
    let issued = epoch().await;
    assert_eq!(issued, agent.access.permission_epoch());
    assert_eq!(issued, agent.access.grants[1].epoch);

    // Journal events about other things leave it where it is.
    app.submit_memory(&agent.value, &submit_req("i1", "the canary drains first"))
        .await
        .expect("submit");
    issue(&app, admin, Role::Reader, "rhea", crew).await;
    assert_eq!(epoch().await, issued);

    // A denial on another subtree keeps the node readable and moves the
    // epoch to the denial's own key.denied seq.
    let denial = app
        .deny_key(admin, agent.key_id.into(), quarantine.into())
        .await
        .expect("deny");
    assert!(denial.epoch > issued);
    assert_eq!(epoch().await, denial.epoch);
}

/// A crew record named `metals-crew`, created by the admin.
async fn metals_crew(app: &TestApp, admin: &str) -> oml_app::types::CrewRecord {
    app.create_crew(
        admin,
        &CrewCreateRequest {
            name: name("metals-crew"),
            charter_uri: Some("https://crews.example.com/metals/charter.md".to_owned()),
            external_manager: None,
        },
    )
    .await
    .expect("create crew")
}

/// `crew` on workarea create is a crew record's id or name; the workarea
/// read serves the record from the store, and `workarea.create` journals
/// its `crew_id` in place of the crew object the PoC copied in (ADR-0026
/// d8; event-catalog.md `workarea.create`).
#[tokio::test]
async fn a_workarea_names_its_crew_record_by_name_and_the_read_serves_the_record() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let crew = metals_crew(&app, admin).await;
    let created = app
        .create_workarea(
            admin,
            &name("metals"),
            Some(boot.root_workarea_id.into()),
            Some(by_name("metals-crew")),
        )
        .await
        .expect("create");
    let (path, created) = (created.path, created.workarea);
    assert_eq!(created.crew_id, Some(crew.crew_id));
    let bare = workarea(&app, admin, "bare", boot.root_workarea_id).await;
    let reader = issue_with(
        &app,
        admin,
        Role::Reader,
        "rhea",
        vec![
            grant(created.workarea_id, Role::Reader),
            grant(bare, Role::Reader),
        ],
    )
    .await;

    let info = app
        .workarea_info(&reader.value, Some(created.workarea_id.into()))
        .await
        .expect("read");
    assert_eq!(info.crew, Some(crew.clone()), "the record, read back");
    // The create answers with the path the read derives.
    assert_eq!(path, "universe.metals");
    assert_eq!(info.path, path);
    let info = app
        .workarea_info(&reader.value, Some(bare.into()))
        .await
        .expect("read");
    assert_eq!(info.crew, None);

    // The journal carries the crew record's id, so the fold is
    // reproducible from it.
    let tail = journal(&store, boot.tenant_id).await;
    let create = tail
        .iter()
        .find(|e| {
            e.envelope.event_type == event_types::WORKAREA_CREATE
                && e.envelope.payload["workarea_id"] == created.workarea_id.to_string()
        })
        .expect("the create is journalled");
    assert_eq!(create.envelope.payload["crew_id"], crew.crew_id.to_string());
    assert!(create.envelope.payload.get("crew").is_none());
    assert_eq!(create.envelope.payload["status"], "development");

    // A crew that names no crew record of the tenant is absent, checked
    // after the parent and before the name: nothing is journalled.
    let head = journal(&store, boot.tenant_id).await.len();
    for crew_ref in [by_name("nowhere-crew"), Uuid::from_u128(0xDEAD).into()] {
        let err = app
            .create_workarea(
                admin,
                &name("orphan"),
                Some(boot.root_workarea_id.into()),
                Some(crew_ref),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);

    // The crew-metadata update points a workarea at a crew record, by name,
    // journalled as `workarea.crew_updated` with its `crew_id`.
    let pointed = app
        .set_workarea_crew(admin, by_name("bare"), by_name("metals-crew"))
        .await
        .expect("point bare at the crew");
    assert_eq!(pointed.crew_id, Some(crew.crew_id));
    let info = app
        .workarea_info(&reader.value, Some(bare.into()))
        .await
        .expect("read");
    assert_eq!(info.crew, Some(crew.clone()));
    let tail = journal(&store, boot.tenant_id).await;
    let updated = tail.last().expect("the update is journalled");
    assert_eq!(
        updated.envelope.event_type,
        event_types::WORKAREA_CREW_UPDATED
    );
    assert_eq!(
        updated.envelope.payload,
        json!({ "workarea_id": bare.to_string(), "crew_id": crew.crew_id.to_string() })
    );
}

/// A crew record's charter URI and external manager change by
/// `crew.updated`, which carries the full new values; its name never
/// changes. Crew verbs are the admin key role's (ADR-0026 d8; http-api.md
/// crews).
#[tokio::test]
async fn a_crew_update_changes_its_charter_and_manager_and_never_its_name() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew_wa) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let metals = metals_crew(&app, admin).await;
    let redline = app
        .create_crew(
            admin,
            &CrewCreateRequest {
                name: name("redline"),
                charter_uri: None,
                external_manager: Some("crewd.example.com".to_owned()),
            },
        )
        .await
        .expect("create crew");
    let created = journal(&store, boot.tenant_id)
        .await
        .into_iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::CREW_CREATED)
        .expect("crew.created");
    assert_eq!(
        created.envelope.payload,
        json!({
            "crew_id": redline.crew_id.to_string(),
            "name": "redline",
            "charter_uri": Value::Null,
            "external_manager": "crewd.example.com",
            "created_by_key_id": boot.admin_key.key_id.to_string(),
        })
    );

    let updated = app
        .update_crew(
            admin,
            by_name("metals-crew"),
            &CrewUpdate {
                charter_uri: None,
                external_manager: Some("crewd.example.com".to_owned()),
            },
        )
        .await
        .expect("update");
    assert_eq!(updated.name, metals.name, "the name never changes");
    assert_eq!(
        updated.charter_uri, metals.charter_uri,
        "kept when not sent"
    );
    assert_eq!(
        updated.external_manager.as_deref(),
        Some("crewd.example.com")
    );
    let event = journal(&store, boot.tenant_id)
        .await
        .pop()
        .expect("crew.updated");
    assert_eq!(event.envelope.event_type, event_types::CREW_UPDATED);
    assert_eq!(
        event.envelope.payload,
        json!({
            "crew_id": metals.crew_id.to_string(),
            "charter_uri": "https://crews.example.com/metals/charter.md",
            "external_manager": "crewd.example.com",
            "updated_by_key_id": boot.admin_key.key_id.to_string(),
        }),
        "the full new values"
    );
    assert_eq!(
        app.crew(admin, metals.crew_id.into()).await.expect("show"),
        updated
    );
    assert_eq!(
        app.crews(admin).await.expect("list"),
        vec![updated.clone(), redline.clone()],
        "in name order"
    );

    // An update that changes nothing is refused, and so is an unknown crew.
    let head = journal(&store, boot.tenant_id).await.len();
    let err = app
        .update_crew(admin, by_name("redline"), &CrewUpdate::default())
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::EmptyCrewUpdate), "{err:?}");
    let err = app
        .update_crew(
            admin,
            by_name("nowhere"),
            &CrewUpdate {
                charter_uri: Some("https://crews.example.com/x.md".to_owned()),
                external_manager: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let err = app.crew(admin, by_name("nowhere")).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    // A second crew under a taken name.
    let err = app
        .create_crew(
            admin,
            &CrewCreateRequest {
                name: name("redline"),
                charter_uri: None,
                external_manager: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AppError::NameTaken { kind: NamedKind::Crew, name } if name.as_str() == "redline"),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);

    // No agent role reaches a crew verb (R1).
    let writer = agent(&app, admin, "wanda", crew_wa).await;
    let err = app.crews(&writer.value).await.unwrap_err();
    assert!(
        matches!(
            err,
            AppError::VerbCeiling {
                role: Role::Writer,
                ..
            }
        ),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------------
// Supersession: the tip moves only at ratification, by compare-and-set;
// retirement is a supersession; edit-as-proposal (ADR-0025 d10, d11)
// ---------------------------------------------------------------------------

/// A writer key on `crew` (writer and reader), a reader key, and a dreamer
/// key with dreamer and reader, each under its own principal.
async fn crew_keys(app: &TestApp, admin: &str, crew: Uuid) -> (IssuedKey, IssuedKey, IssuedKey) {
    let writer = agent(app, admin, "wanda", crew).await;
    let reader = issue(app, admin, Role::Reader, "rhea", crew).await;
    let dreamer = issue_with(
        app,
        admin,
        Role::Dreamer,
        "dana",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;
    (writer, reader, dreamer)
}

#[tokio::test]
async fn a_supersession_leaves_the_old_tip_served_to_others_until_a_non_author_ratifies() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, reader, dreamer) = crew_keys(&app, admin, crew).await;
    let authors_dreamer = issue(&app, admin, Role::Dreamer, "wanda", crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let successor = app
        .supersede(
            &writer.value,
            &supersede_req("sup-1", first, "the canary drains and settles first"),
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id;

    // Other keys keep the old tip: it reads promoted and tip, it is their
    // one hit, and the held successor does not exist for them (R3).
    let old = app.get_memory(&reader.value, first).await.expect("get");
    assert_eq!(old.memory.status, MemoryStatus::Promoted);
    assert!(old.memory.is_tip);
    assert_eq!(hit_ids(&app, &reader.value, "canary").await, vec![first]);
    let err = app.get_memory(&reader.value, successor).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");

    // The author's principal cannot ratify, under any key (R2).
    let err = app
        .post_verdict(&authors_dreamer.value, successor, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Verdict(_)), "{err:?}");
    assert_eq!(hit_ids(&app, &reader.value, "canary").await, vec![first]);

    // A non-author ratifies: only now does the successor replace the tip.
    app.post_verdict(&dreamer.value, successor, Verdict::Approve, None)
        .await
        .expect("ratify");
    assert_eq!(
        hit_ids(&app, &reader.value, "canary").await,
        vec![successor]
    );
    let old = app.get_memory(&reader.value, first).await.expect("get");
    assert_eq!(old.memory.status, MemoryStatus::Promoted);
    assert!(
        !old.memory.is_tip,
        "a superseded version is promoted, tip false"
    );
}

#[tokio::test]
async fn ratifying_a_supersession_journals_one_lifecycle_supersede_and_flips_the_tip_once() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let successor = app
        .supersede(
            &writer.value,
            &supersede_req("sup-1", first, "the canary drains and settles first"),
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    let before = journal(&store, boot.tenant_id).await.len();
    app.post_verdict(&dreamer.value, successor, Verdict::Approve, None)
        .await
        .expect("ratify");

    let events = journal(&store, boot.tenant_id).await;
    let ratified: Vec<_> = events[before..].iter().map(|e| &e.envelope).collect();
    let types: Vec<&str> = ratified.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(
        types,
        vec![
            event_types::REVIEW_APPROVE,
            event_types::LIFECYCLE_SUPERSEDE
        ],
        "one event flips the tip, never a promote beside it"
    );
    let supersede = &ratified[1].payload;
    assert_eq!(supersede["claim_key"], "claim-i1");
    assert_eq!(supersede["workarea_id"], crew.to_string());
    assert_eq!(supersede["new_artifact_id"], successor.to_string());
    assert_eq!(supersede["new_version"], 2);
    assert_eq!(supersede["replaces_artifact_id"], first.to_string());
    assert_eq!(supersede["replaces_version"], 1);
    assert_eq!(supersede["retirement"], false);
    assert_eq!(supersede["self_signed"], false);
    assert_eq!(
        supersede["ratification_event_id"],
        ratified[0].event_id.to_string()
    );

    // The first version's promotion was a lifecycle.promote with no
    // previous tip; it is the claim's only one.
    let promotes: Vec<_> = events
        .iter()
        .filter(|e| e.envelope.event_type == event_types::LIFECYCLE_PROMOTE)
        .collect();
    assert_eq!(promotes.len(), 1);
    let promote = &promotes[0].envelope.payload;
    assert_eq!(promote["artifact_id"], first.to_string());
    assert_eq!(promote["version"], 1);
    assert_eq!(promote["previous_tip_version"], serde_json::Value::Null);

    // One served tip: the successor's.
    let tip = store
        .claim_tip(boot.tenant_id, crew, "claim-i1")
        .await
        .expect("tip")
        .expect("the claim has a tip");
    assert_eq!(tip.memory_id, successor);
    assert!(tip.promoted_at.is_some());
    let old = store
        .memory(boot.tenant_id, first)
        .await
        .expect("memory")
        .expect("row");
    assert_eq!(old.status, MemoryStatus::Promoted);
    assert!(!old.is_tip);
}

#[tokio::test]
async fn a_supersession_whose_target_lost_the_tip_is_tip_conflict_at_verdict_and_journals_nothing()
{
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    let editor = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "eddie",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    // Two held successors of one tip: the author's revision and a
    // dreamer's edit-as-proposal.
    let revision = app
        .supersede(&writer.value, &supersede_req("sup-w", first, "revision"))
        .await
        .expect("revision")
        .receipt;
    let edit = app
        .supersede(&editor.value, &supersede_req("sup-e", first, "edit"))
        .await
        .expect("edit")
        .receipt;
    assert_eq!((revision.version, edit.version), (2, 3));

    app.post_verdict(&dreamer.value, revision.memory_id, Verdict::Approve, None)
        .await
        .expect("the first ratified takes the tip");
    let head = journal(&store, boot.tenant_id).await.len();
    let err = app
        .post_verdict(&dreamer.value, edit.memory_id, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::TipConflict {
                current_tip: Some(VersionRef {
                    artifact_id,
                    version: 2,
                })
            } if artifact_id == revision.memory_id
        ),
        "{err:?}"
    );
    assert_eq!(
        journal(&store, boot.tenant_id).await.len(),
        head,
        "a lost compare-and-set journals nothing"
    );
    let stale = store
        .memory(boot.tenant_id, edit.memory_id)
        .await
        .expect("memory")
        .expect("row");
    assert_eq!(stale.status, MemoryStatus::Held, "it stays held");
    assert!(!stale.is_tip);

    // It stays held until a reviewer rejects it.
    let rejected = app
        .post_verdict(
            &dreamer.value,
            edit.memory_id,
            Verdict::Reject,
            Some("stale"),
        )
        .await
        .expect("reject the stale successor");
    assert_eq!(rejected.state, MemoryStatus::Rejected);
}

/// A promoted first version and one held successor of it: the tenant, the
/// dreamer that may ratify, the target and the successor.
async fn a_held_successor(app: &TestApp) -> (BootstrapResult, IssuedKey, Uuid, Uuid) {
    let (boot, crew) = tenant(app).await;
    let (writer, _reader, dreamer) = crew_keys(app, &boot.admin_key.value, crew).await;
    let first = promoted(
        app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let successor = app
        .supersede(
            &writer.value,
            &supersede_req("sup-1", first, "the canary drains and settles first"),
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    (boot, dreamer, first, successor)
}

/// The types of the events journalled from `from` on.
async fn types_since(store: &MemStore, tenant: Uuid, from: usize) -> Vec<String> {
    journal(store, tenant).await[from..]
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect()
}

#[tokio::test]
async fn a_verdict_whose_ratification_loses_the_compare_and_set_in_the_store_journals_no_verdict() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, dreamer, first, successor) = a_held_successor(&app).await;
    let served = app
        .review_next(&dreamer.value)
        .await
        .expect("review next")
        .expect("the successor is served");
    assert_eq!(served.memory.memory_id, successor);
    let before = journal(&store, boot.tenant_id).await.len();

    // The store's compare-and-set refuses the ratification; the app's
    // pre-check read cannot see a race it lost after reading.
    store.refuse_next_commit_of(
        event_types::LIFECYCLE_SUPERSEDE,
        StoreError::TipMoved { replaced: first },
    );
    let err = app
        .post_verdict(&dreamer.value, successor, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::TipConflict {
                current_tip: Some(VersionRef {
                    artifact_id,
                    version: 1,
                })
            } if artifact_id == first
        ),
        "{err:?}"
    );
    assert_eq!(
        types_since(&store, boot.tenant_id, before).await,
        Vec::<String>::new(),
        "nothing is journalled, the verdict included"
    );
    let row = store
        .memory(boot.tenant_id, successor)
        .await
        .expect("memory")
        .expect("row");
    assert_eq!(row.status, MemoryStatus::Held, "it stays held");
    assert!(!row.is_tip);
    let again = app
        .review_next(&dreamer.value)
        .await
        .expect("review next")
        .expect("the serve is not consumed");
    assert_eq!(again.memory.memory_id, successor);
}

#[tokio::test]
async fn a_ratification_that_loses_the_chain_head_journals_its_verdict_once_on_retry() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, dreamer, _first, successor) = a_held_successor(&app).await;
    let before = journal(&store, boot.tenant_id).await.len();

    // A commit landed between the verdict's head read and its ratification.
    let seq = before as u64 + 2;
    store.refuse_next_commit_of(
        event_types::LIFECYCLE_SUPERSEDE,
        StoreError::SeqConflict {
            expected_seq: seq + 1,
            found_seq: seq,
        },
    );
    let err = app
        .post_verdict(&dreamer.value, successor, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::Store(StoreError::SeqConflict { .. })),
        "{err:?}"
    );
    assert_eq!(
        types_since(&store, boot.tenant_id, before).await,
        Vec::<String>::new(),
        "the conflicted verdict is not journalled"
    );

    // The transport retries the whole verdict on a chain-head conflict.
    app.post_verdict(&dreamer.value, successor, Verdict::Approve, None)
        .await
        .expect("the retry ratifies");
    assert_eq!(
        types_since(&store, boot.tenant_id, before).await,
        vec![
            event_types::REVIEW_APPROVE,
            event_types::LIFECYCLE_SUPERSEDE
        ],
        "one verdict and one ratification"
    );
}

#[tokio::test]
async fn a_replayed_supersede_returns_the_stored_receipt_and_writes_once() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let other = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i2", "the canary drains last"),
    )
    .await;
    let req = supersede_req("sup-1", first, "the canary drains and settles first");
    let original = app.supersede(&writer.value, &req).await.expect("supersede");
    assert!(!original.replayed);
    let head = journal(&store, boot.tenant_id).await.len();

    let replay = app.supersede(&writer.value, &req).await.expect("replay");
    assert!(replay.replayed);
    assert_eq!(replay.receipt, original.receipt, "the stored receipt");
    assert_eq!(
        journal(&store, boot.tenant_id).await.len(),
        head,
        "writes once"
    );

    // The key names one request: another body, or the same body against
    // another target, is a digest mismatch, never a replay.
    let err = app
        .supersede(
            &writer.value,
            &supersede_req("sup-1", first, "another body"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::IdempotencyDigestMismatch),
        "{err:?}"
    );
    let err = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                target_id: other,
                ..req.clone()
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::IdempotencyDigestMismatch),
        "{err:?}"
    );

    // A committed supersession replays even after the key lost its access
    // to the target's workarea (http-api.md idempotency).
    app.deny_key(admin, writer.key_id.into(), crew.into())
        .await
        .expect("deny");
    let late = app
        .supersede(&writer.value, &req)
        .await
        .expect("late replay");
    assert!(late.replayed);
    assert_eq!(late.receipt, original.receipt);
}

#[tokio::test]
async fn a_supersede_that_loses_the_idempotency_race_replays_the_winner_and_writes_once() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let req = supersede_req("sup-1", first, "the canary drains and settles first");
    let winner = app.supersede(&writer.value, &req).await.expect("winner");
    let head = journal(&store, boot.tenant_id).await.len();

    // The loser looked the key up before the winner committed, so it found
    // no record, and it reaches its own commit after the winner's: the
    // target is still the tip, so nothing refuses it before the commit.
    store.miss_next_idempotency_lookup();
    let loser = app.supersede(&writer.value, &req).await.expect("loser");
    assert!(loser.replayed, "the winner's stored receipt, as a replay");
    assert_eq!(loser.receipt, winner.receipt);
    assert_eq!(
        journal(&store, boot.tenant_id).await.len(),
        head,
        "writes once"
    );
}

#[tokio::test]
async fn tip_conflict_names_the_current_tip_when_readable() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    // Writer only: it reads its own rows and nothing else on crew.
    let blind = issue(&app, admin, Role::Writer, "wren", crew).await;
    let approver = issue(&app, admin, Role::Dreamer, "dora", crew).await;

    // The reading writer's claim moves to a version it can read.
    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let second = app
        .supersede(&writer.value, &supersede_req("sup-1", first, "v2"))
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    app.post_verdict(&dreamer.value, second, Verdict::Approve, None)
        .await
        .expect("ratify");
    let err = app
        .supersede(&writer.value, &supersede_req("sup-2", first, "v3"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::TipConflict {
                current_tip: Some(VersionRef { artifact_id, version: 2 })
            } if artifact_id == second
        ),
        "{err:?}"
    );

    // The blind writer's claim moves to a dreamer's edit it cannot read:
    // the refusal names no tip.
    let blind_first = promoted(
        &app,
        &blind.value,
        &approver.value,
        &submit_req("i2", "the canary drains last"),
    )
    .await;
    let edit = app
        .supersede(&dreamer.value, &supersede_req("sup-e", blind_first, "edit"))
        .await
        .expect("edit-as-proposal")
        .receipt
        .memory_id;
    app.post_verdict(&approver.value, edit, Verdict::Approve, None)
        .await
        .expect("ratify the edit");
    let err = app
        .supersede(&blind.value, &supersede_req("sup-b", blind_first, "v3"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::TipConflict { current_tip: None }),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_ratified_retirement_takes_the_claim_out_of_recall_and_keeps_every_version_fetchable() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, reader, dreamer) = crew_keys(&app, admin, crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let proposal = app
        .supersede(
            &writer.value,
            &retire_req("ret-1", first, "the canary runbook replaced this"),
        )
        .await
        .expect("retire-proposal")
        .receipt;
    assert!(proposal.retirement);
    let before = journal(&store, boot.tenant_id).await.len();
    app.post_verdict(&dreamer.value, proposal.memory_id, Verdict::Approve, None)
        .await
        .expect("ratify the retirement");

    // Recall serves nothing of a retired claim, whatever matches.
    assert!(hit_ids(&app, &reader.value, "canary").await.is_empty());
    let recall = app
        .recall(&reader.value, &recall_req("runbook"))
        .await
        .expect("recall");
    assert!(recall.hits.is_empty() && recall.steering.is_empty());

    // Every version stays fetchable as history; nothing is demoted.
    let old = app.get_memory(&reader.value, first).await.expect("get");
    assert_eq!(old.memory.status, MemoryStatus::Promoted);
    assert!(!old.memory.is_tip);
    let tip = app
        .get_memory(&reader.value, proposal.memory_id)
        .await
        .expect("get the retirement version");
    assert_eq!(tip.memory.status, MemoryStatus::Promoted);
    assert!(tip.memory.is_tip);
    assert!(tip.memory.retirement);

    // The flip is journalled with the retirement named.
    let events = journal(&store, boot.tenant_id).await;
    let flip = &events[before + 1].envelope;
    assert_eq!(flip.event_type, event_types::LIFECYCLE_SUPERSEDE);
    assert_eq!(flip.payload["retirement"], true);
}

#[tokio::test]
async fn a_held_retirement_changes_no_recall() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, reader, dreamer) = crew_keys(&app, admin, crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    app.supersede(&writer.value, &retire_req("ret-1", first, "stop serving"))
        .await
        .expect("retire-proposal");
    assert_eq!(hit_ids(&app, &reader.value, "canary").await, vec![first]);
    assert!(
        app.get_memory(&reader.value, first)
            .await
            .expect("get")
            .memory
            .is_tip
    );
}

#[tokio::test]
async fn superseding_a_retirement_version_returns_the_claim_to_recall() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, reader, dreamer) = crew_keys(&app, admin, crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let retirement = app
        .supersede(&writer.value, &retire_req("ret-1", first, "stop serving"))
        .await
        .expect("retire-proposal")
        .receipt
        .memory_id;
    app.post_verdict(&dreamer.value, retirement, Verdict::Approve, None)
        .await
        .expect("ratify the retirement");
    assert!(hit_ids(&app, &reader.value, "canary").await.is_empty());

    let revived = app
        .supersede(
            &writer.value,
            &supersede_req("sup-1", retirement, "the canary drains again"),
        )
        .await
        .expect("supersede the retirement version")
        .receipt;
    assert_eq!(revived.version, 3);
    assert!(!revived.retirement);
    app.post_verdict(&dreamer.value, revived.memory_id, Verdict::Approve, None)
        .await
        .expect("ratify");
    assert_eq!(
        hit_ids(&app, &reader.value, "canary").await,
        vec![revived.memory_id]
    );
}

#[tokio::test]
async fn a_term_cannot_retire_it_deprecates() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;

    let term = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &SubmitRequest {
            kind: MemoryKind::Term,
            ..submit_req("i1", "canary: a staged rollout slice")
        },
    )
    .await;
    let err = app
        .supersede(&writer.value, &retire_req("ret-1", term, "stop serving"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::TermRetirement), "{err:?}");
}

#[tokio::test]
async fn a_successor_kind_changes_only_within_its_class() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;

    let memory = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let term = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &SubmitRequest {
            kind: MemoryKind::Term,
            ..submit_req("i2", "canary: a staged rollout slice")
        },
    )
    .await;
    let with_kind = |idem: &str, target, kind, retirement| SupersedeRequest {
        kind: Some(kind),
        retirement,
        ..supersede_req(idem, target, "revised")
    };

    // A memory may become a standing instruction under review.
    let steering = app
        .supersede(
            &writer.value,
            &with_kind("s-1", memory, MemoryKind::Steering, false),
        )
        .await
        .expect("memory to steering");
    let read = app
        .get_memory(&writer.value, steering.receipt.memory_id)
        .await
        .expect("own read");
    assert_eq!(read.memory.kind, MemoryKind::Steering);

    for (idem, target, kind, retirement) in [
        ("s-2", memory, MemoryKind::Term, false),
        ("s-3", term, MemoryKind::Memory, false),
        ("s-4", memory, MemoryKind::DerivedFact, true),
    ] {
        let err = app
            .supersede(&writer.value, &with_kind(idem, target, kind, retirement))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::KindChangeRefused { .. }), "{err:?}");
    }
}

#[tokio::test]
async fn a_derived_fact_successor_journals_submission_learn() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    app.supersede(
        &writer.value,
        &SupersedeRequest {
            kind: Some(MemoryKind::DerivedFact),
            ..supersede_req("sup-1", first, "the canary always drains first")
        },
    )
    .await
    .expect("supersede");
    let events = journal(&store, boot.tenant_id).await;
    let capture = &events.last().expect("an event").envelope;
    assert_eq!(capture.event_type, event_types::SUBMISSION_LEARN);
    assert_eq!(capture.payload["kind"], "derived_fact");
    assert_eq!(
        capture.payload["replaces"],
        serde_json::json!({ "artifact_id": first.to_string(), "version": 1 })
    );
    assert_eq!(
        capture.payload["derives_from"],
        serde_json::json!([first.to_string()])
    );
    assert_eq!(capture.payload["retirement"], false);
}

#[tokio::test]
async fn an_x_oml_workarea_other_than_the_targets_is_request_validation() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let err = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                workarea_id: Some(boot.root_workarea_id.into()),
                ..supersede_req("sup-1", first, "revised")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::WorkareaNotTarget { .. }), "{err:?}");
    app.supersede(
        &writer.value,
        &SupersedeRequest {
            workarea_id: Some(crew.into()),
            ..supersede_req("sup-2", first, "revised")
        },
    )
    .await
    .expect("the target's own workarea");
}

#[tokio::test]
async fn a_writer_cannot_supersede_another_keys_claim() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    let other = agent(&app, admin, "walt", crew).await;
    let approver = issue(&app, admin, Role::Dreamer, "dora", crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let err = app
        .supersede(&other.value, &supersede_req("sup-o", first, "mine now"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AuthorOnly), "{err:?}");

    // Once a dreamer's ratified edit holds the tip, the claim is the
    // dreamer's: own is read on the tip, never on the first version.
    let edit = app
        .supersede(&dreamer.value, &supersede_req("sup-e", first, "edit"))
        .await
        .expect("edit-as-proposal")
        .receipt
        .memory_id;
    app.post_verdict(&approver.value, edit, Verdict::Approve, None)
        .await
        .expect("ratify the edit");
    let err = app
        .supersede(&writer.value, &supersede_req("sup-w", edit, "revised"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AuthorOnly), "{err:?}");
}

#[tokio::test]
async fn a_dreamer_proposes_an_edit_to_another_authors_claim_and_cannot_review_it() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, reader, dreamer) = crew_keys(&app, admin, crew).await;
    let approver = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let reviewer_only = issue(&app, admin, Role::Dreamer, "dina", crew).await;
    let reads_only = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dale",
        vec![grant(crew, Role::Reader)],
    )
    .await;

    let first = promoted(
        &app,
        &writer.value,
        &approver.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;

    // Edit-as-proposal needs dreamer and reader on the target's workarea,
    // resolved by the refusal order.
    let err = app
        .supersede(&reviewer_only.value, &supersede_req("e-0", first, "edit"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let err = app
        .supersede(&reads_only.value, &supersede_req("e-1", first, "edit"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::GrantRefused {
                needed_role: Role::Dreamer,
                ..
            }
        ),
        "{err:?}"
    );

    let edit = app
        .supersede(
            &dreamer.value,
            &supersede_req("e-2", first, "the canary drains and settles first"),
        )
        .await
        .expect("edit-as-proposal")
        .receipt;
    assert_eq!(edit.version, 2);
    assert_eq!(hit_ids(&app, &reader.value, "canary").await, vec![first]);

    // Its proposer is in its lineage: never served it, refused its verdict.
    assert!(
        app.review_next(&dreamer.value)
            .await
            .expect("review next")
            .is_none()
    );
    let err = app
        .post_verdict(&dreamer.value, edit.memory_id, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Verdict(_)), "{err:?}");

    // Another dreamer ratifies it.
    app.post_verdict(&approver.value, edit.memory_id, Verdict::Approve, None)
        .await
        .expect("ratify");
    assert_eq!(
        hit_ids(&app, &reader.value, "canary").await,
        vec![edit.memory_id]
    );
}

#[tokio::test]
async fn the_author_cannot_ratify_its_own_retirement() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, reader, dreamer) = crew_keys(&app, admin, crew).await;
    let authors_dreamer = issue(&app, admin, Role::Dreamer, "wanda", crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    let retirement = app
        .supersede(&writer.value, &retire_req("ret-1", first, "stop serving"))
        .await
        .expect("retire-proposal")
        .receipt
        .memory_id;
    assert!(
        app.review_next(&authors_dreamer.value)
            .await
            .expect("review next")
            .is_none()
    );
    let err = app
        .post_verdict(&authors_dreamer.value, retirement, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Verdict(oml_app::domain::VerdictError::PrincipalInLineage(_))
        ),
        "{err:?}"
    );
    assert_eq!(hit_ids(&app, &reader.value, "canary").await, vec![first]);
}

#[tokio::test]
async fn a_rival_successors_author_is_kept_off_the_others_review() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    let approver = issue(&app, admin, Role::Dreamer, "dora", crew).await;

    let first = promoted(
        &app,
        &writer.value,
        &approver.value,
        &submit_req("i1", "the canary drains first"),
    )
    .await;
    // The dreamer's edit and the author's revision compete for one tip.
    let edit = app
        .supersede(&dreamer.value, &supersede_req("e-1", first, "edit"))
        .await
        .expect("edit")
        .receipt
        .memory_id;
    let revision = app
        .supersede(&writer.value, &supersede_req("w-1", first, "revision"))
        .await
        .expect("revision")
        .receipt
        .memory_id;

    // The edit's author may not judge its rival: never served, refused.
    assert!(
        app.review_next(&dreamer.value)
            .await
            .expect("review next")
            .is_none()
    );
    let err = app
        .post_verdict(&dreamer.value, revision, Verdict::Reject, Some("mine wins"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Verdict(_)), "{err:?}");

    // Once its own successor is no longer held, the exclusion lifts.
    app.post_verdict(&approver.value, edit, Verdict::Reject, Some("superfluous"))
        .await
        .expect("reject the edit");
    let served = app
        .review_next(&dreamer.value)
        .await
        .expect("review next")
        .expect("the revision waits");
    assert_eq!(served.memory.memory_id, revision);
}

// ---------------------------------------------------------------------------
// Own held rows in recall and the max_tokens budget (ADR-0025 d4, d5, d6,
// d7, d9)
// ---------------------------------------------------------------------------

/// Budget tokens of `text`, an NFC literal: ⌈UTF-8 bytes ÷ 4⌉ under counter
/// `oml-tc/1` (retrieval.md token budgets).
fn tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

/// A recall of `query` that takes the calling key's own held rows too.
fn own_held(query: &str) -> RecallRequest {
    RecallRequest {
        include_own_held: true,
        ..recall_req(query)
    }
}

/// A recall of `query` under a budget of `max_tokens`.
fn budgeted(query: &str, max_tokens: u64) -> RecallRequest {
    RecallRequest {
        max_tokens: Some(max_tokens),
        ..recall_req(query)
    }
}

/// Submits `req` as `key`, leaving it held: its id.
async fn held(app: &TestApp, key: &str, req: &SubmitRequest) -> Uuid {
    app.submit_memory(key, req)
        .await
        .expect("submit")
        .receipt
        .memory_id
}

/// Everything a recall answered, comparable: steering, each hit with its
/// score's bits, the budget, the served ids and the digest.
fn answer(recall: &RecallResult) -> impl PartialEq + std::fmt::Debug {
    (
        recall.steering.clone(),
        recall
            .hits
            .iter()
            .map(|h| (h.served.clone(), h.score.to_bits()))
            .collect::<Vec<_>>(),
        recall.budget,
        recall.served_ids.clone(),
        recall.query_digest.clone(),
    )
}

#[tokio::test]
async fn include_own_held_shows_the_authors_held_row_to_that_key_only() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let author = agent(&app, admin, "wanda", crew).await;
    let row = held(&app, &author.value, &submit_req("h1", "the canary held")).await;

    let recall = app
        .recall(&author.value, &own_held("canary"))
        .await
        .expect("recall");
    assert_eq!(hit_ids_of(&recall), vec![row]);
    let hit = &recall.hits[0];
    assert_eq!(hit.memory.status, MemoryStatus::Held);
    assert!(!hit.memory.is_tip);
    assert!(hit.score > 0.0, "a held row is scored like any hit");
    let off = app
        .recall(&author.value, &recall_req("canary"))
        .await
        .expect("recall");
    assert!(off.hits.is_empty(), "the flag defaults off");

    // Own is the authoring key, never the principal: another reader key and
    // a dreamer key of the same principal see nothing of it
    // (roles-and-keys.md#own; ADR-0025 d4).
    let same_reader = issue(&app, admin, Role::Reader, "wanda", crew).await;
    let same_dreamer = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "wanda",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;
    let stranger = issue(&app, admin, Role::Reader, "rhea", crew).await;
    for key in [&same_reader, &same_dreamer, &stranger] {
        let recall = app
            .recall(&key.value, &own_held("canary"))
            .await
            .expect("recall");
        assert!(
            recall.hits.is_empty(),
            "{:?} key of {}: {:?}",
            key.role,
            key.principal,
            hit_ids_of(&recall)
        );
    }
}

#[tokio::test]
async fn own_held_rows_are_ranked_among_promoted_hits_with_state_held() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (author, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    // This store scores by distinct query terms matched: 3, 2 and 1.
    let best = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &submit_req("p1", "canary drain settle"),
    )
    .await;
    let row = held(&app, &author.value, &submit_req("h1", "canary drain")).await;
    let worst = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &submit_req("p2", "canary"),
    )
    .await;

    let recall = app
        .recall(&author.value, &own_held("canary drain settle"))
        .await
        .expect("recall");
    assert_eq!(
        hit_ids_of(&recall),
        vec![best, row, worst],
        "one total order across states"
    );
    let flagged: Vec<(MemoryStatus, bool)> = recall
        .hits
        .iter()
        .map(|h| (h.memory.status, h.memory.is_tip))
        .collect();
    assert_eq!(
        flagged,
        vec![
            (MemoryStatus::Promoted, true),
            (MemoryStatus::Held, false),
            (MemoryStatus::Promoted, true),
        ]
    );
    assert!(
        recall.hits.windows(2).all(|w| w[0].score > w[1].score),
        "score descending"
    );
}

#[tokio::test]
async fn own_held_rows_count_against_k_and_the_budget() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (author, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    let (two_text, held_text) = ("canary drain", "canary drain settle");
    let two = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &submit_req("p1", two_text),
    )
    .await;
    let one = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &submit_req("p2", "canary"),
    )
    .await;
    let row = held(&app, &author.value, &submit_req("h1", held_text)).await;
    let query = "canary drain settle";

    let k2 = |req: RecallRequest| RecallRequest { k: 2, ..req };
    let with = app
        .recall(&author.value, &k2(own_held(query)))
        .await
        .expect("recall");
    assert_eq!(
        hit_ids_of(&with),
        vec![row, two],
        "the held row takes a k slot"
    );
    let without = app
        .recall(&author.value, &k2(recall_req(query)))
        .await
        .expect("recall");
    assert_eq!(hit_ids_of(&without), vec![two, one]);

    let filled = app
        .recall(
            &author.value,
            &RecallRequest {
                max_tokens: Some(tokens(held_text)),
                ..own_held(query)
            },
        )
        .await
        .expect("recall");
    assert_eq!(
        hit_ids_of(&filled),
        vec![row],
        "the held row spends the budget"
    );
    assert_eq!(
        filled.budget,
        Some(Budget {
            max_tokens: tokens(held_text),
            steering_tokens: 0,
            used_tokens: tokens(held_text),
            truncated: true,
            hits_served: 1,
            hits_omitted: 2,
            next_hit_tokens: Some(tokens(two_text)),
        })
    );
}

#[tokio::test]
async fn own_held_steering_and_retirement_rows_never_enter_recall() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (author, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    let steering = held(
        &app,
        &author.value,
        &submit_kind("s1", MemoryKind::Steering, "canary steering held", crew),
    )
    .await;
    let target = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &submit_req("m1", "the canary claim"),
    )
    .await;
    let retirement = app
        .supersede(
            &author.value,
            &retire_req("r1", target, "retired: the canary lapsed"),
        )
        .await
        .expect("retire-proposal")
        .receipt
        .memory_id;

    // Neither is in force, so neither is recalled; the promoted tip stays.
    let recall = app
        .recall(
            &author.value,
            &own_held("canary steering held retired lapsed claim"),
        )
        .await
        .expect("recall");
    assert_eq!(hit_ids_of(&recall), vec![target]);
    assert!(recall.steering.is_empty(), "{:?}", steering_ids(&recall));
    // Get reaches both: they are the key's own (ADR-0025 d3).
    for id in [steering, retirement] {
        let got = app.get_memory(&author.value, id).await.expect("get");
        assert_eq!(got.memory.status, MemoryStatus::Held);
    }
}

#[tokio::test]
async fn include_own_held_never_widens_the_workareas_served() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quotes = workarea(&app, admin, "quotes", crew).await;
    let freight = workarea(&app, admin, "freight", crew).await;
    let quarantine = workarea(&app, admin, "quarantine", quotes).await;
    // The key writes the whole crew but reads only quotes, and a global
    // denial masks quarantine beneath it.
    let author = issue_with(
        &app,
        admin,
        Role::Writer,
        "wanda",
        vec![grant(crew, Role::Writer), grant(quotes, Role::Reader)],
    )
    .await;
    let mut rows = Vec::new();
    for (idem, wa) in [
        ("q", quotes),
        ("f", freight),
        ("c", crew),
        ("x", quarantine),
    ] {
        rows.push(
            held(
                &app,
                &author.value,
                &submit_into(idem, "the canary held", wa),
            )
            .await,
        );
    }
    app.deny_key(admin, author.key_id.into(), quarantine.into())
        .await
        .expect("deny");

    let recall = app
        .recall(
            &author.value,
            &RecallRequest {
                workarea_id: Some(quotes.into()),
                ..own_held("canary")
            },
        )
        .await
        .expect("recall");
    assert_eq!(
        hit_ids_of(&recall),
        vec![rows[0]],
        "subtree(scope) within reader access, minus global denials"
    );
    // The recall ceiling is unchanged: reader on the scope, own rows or not.
    for scope in [crew, freight] {
        let err = app
            .recall(
                &author.value,
                &RecallRequest {
                    workarea_id: Some(scope.into()),
                    ..own_held("canary")
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
}

#[tokio::test]
async fn a_held_successor_appears_with_replaces_while_its_target_stays_a_hit() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (author, reader, dreamer) = crew_keys(&app, admin, crew).await;
    let target = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &submit_req("q1", "the canary quote is 412"),
    )
    .await;
    let successor = app
        .supersede(
            &author.value,
            &supersede_req("q2", target, "the canary quote is 409"),
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id;

    let recall = app
        .recall(&author.value, &own_held("canary quote"))
        .await
        .expect("recall");
    let mut served = hit_ids_of(&recall);
    served.sort();
    let mut both = vec![target, successor];
    both.sort();
    assert_eq!(served, both, "the two are never deduplicated");
    let hit = |id: Uuid| {
        recall
            .hits
            .iter()
            .find(|h| h.memory.memory_id == id)
            .expect("served")
    };
    let proposal = hit(successor);
    assert_eq!(proposal.memory.status, MemoryStatus::Held);
    assert!(!proposal.memory.is_tip);
    assert_eq!(
        proposal.replaces,
        Some(VersionRef {
            artifact_id: target,
            version: 1
        })
    );
    let tip = hit(target);
    assert_eq!(tip.memory.status, MemoryStatus::Promoted);
    assert!(tip.memory.is_tip);

    // Every other key recalls the target alone until ratification.
    let other = app
        .recall(&reader.value, &own_held("canary quote"))
        .await
        .expect("recall");
    assert_eq!(hit_ids_of(&other), vec![target]);
}

/// A crew with one promoted steering row and three promoted memories that
/// match `canary drain settle` by 3, 2 and 1 terms, the second the largest:
/// (app, reader, steering id, hit ids in rank order, steering tokens, hit
/// tokens in rank order).
async fn budget_fixture() -> (TestApp, IssuedKey, Uuid, Vec<Uuid>, u64, Vec<u64>) {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, reader, dreamer) = crew_keys(&app, admin, crew).await;
    let steering_text = "Quote every price in USD/t.";
    let texts = [
        "canary drain settle",
        "canary drain, then wait out the whole quarter before anyone settles it",
        "canary",
    ];
    let steering = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_kind("s1", MemoryKind::Steering, steering_text, crew),
    )
    .await;
    let mut hits = Vec::new();
    for (n, text) in texts.iter().enumerate() {
        hits.push(
            promoted(
                &app,
                &writer.value,
                &dreamer.value,
                &submit_req(&format!("m{n}"), text),
            )
            .await,
        );
    }
    let counts = texts.iter().map(|t| tokens(t)).collect();
    (app, reader, steering, hits, tokens(steering_text), counts)
}

#[tokio::test]
async fn max_tokens_keeps_the_longest_rank_prefix_and_reports_the_budget() {
    let (app, reader, steering, hits, s, t) = budget_fixture().await;
    assert!(t[1] > t[2], "the third hit fits where the second does not");

    // After steering and the first hit, what remains fits the third hit but
    // not the second: the first that does not fit ends the list.
    let max = s + t[0] + t[2];
    let recall = app
        .recall(&reader.value, &budgeted("canary drain settle", max))
        .await
        .expect("recall");
    assert_eq!(steering_ids(&recall), vec![steering]);
    assert_eq!(hit_ids_of(&recall), vec![hits[0]], "no skip-ahead");
    assert_eq!(
        recall.budget,
        Some(Budget {
            max_tokens: max,
            steering_tokens: s,
            used_tokens: s + t[0],
            truncated: true,
            hits_served: 1,
            hits_omitted: 2,
            next_hit_tokens: Some(t[1]),
        })
    );
    assert_eq!(
        recall.served_ids,
        vec![steering, hits[0]],
        "the ids served after the cuts, never the candidates"
    );

    // A budget every candidate fits cuts nothing.
    let all = s + t.iter().sum::<u64>();
    let recall = app
        .recall(&reader.value, &budgeted("canary drain settle", all))
        .await
        .expect("recall");
    assert_eq!(hit_ids_of(&recall), hits);
    assert_eq!(
        recall.budget,
        Some(Budget {
            max_tokens: all,
            steering_tokens: s,
            used_tokens: all,
            truncated: false,
            hits_served: 3,
            hits_omitted: 0,
            next_hit_tokens: None,
        })
    );
}

#[tokio::test]
async fn steering_past_the_budget_is_served_whole_with_no_hits_and_truncated() {
    let (app, reader, steering, _hits, s, t) = budget_fixture().await;
    let recall = app
        .recall(&reader.value, &budgeted("canary drain settle", s - 1))
        .await
        .expect("recall");
    assert_eq!(
        steering_ids(&recall),
        vec![steering],
        "steering is never cut"
    );
    assert!(recall.hits.is_empty());
    let budget = recall.budget.expect("a budget object");
    assert_eq!(
        budget,
        Budget {
            max_tokens: s - 1,
            steering_tokens: s,
            used_tokens: s,
            truncated: true,
            hits_served: 0,
            hits_omitted: 3,
            next_hit_tokens: Some(t[0]),
        }
    );
    assert!(
        budget.used_tokens > budget.max_tokens,
        "the overrun is visible"
    );
}

#[tokio::test]
async fn every_steering_item_and_hit_carries_its_tokens() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (author, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    // Each text is sent decomposed; its count is its NFC text's, which here
    // is one budget token fewer (retrieval.md token budgets; ADR-0025 d6).
    let [steering_text, hit_text, held_text] =
        ["café canary", "the café canary", "own café canary"];
    let decomposed = |text: &str| text.replace('é', "e\u{301}");
    for text in [steering_text, hit_text, held_text] {
        assert_ne!(tokens(text), tokens(&decomposed(text)), "{text}");
    }
    let steering = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &submit_kind("s1", MemoryKind::Steering, &decomposed(steering_text), crew),
    )
    .await;
    let hit = promoted(
        &app,
        &author.value,
        &dreamer.value,
        &submit_req("m1", &decomposed(hit_text)),
    )
    .await;
    let row = held(
        &app,
        &author.value,
        &submit_req("h1", &decomposed(held_text)),
    )
    .await;

    let recall = app
        .recall(&author.value, &own_held("canary"))
        .await
        .expect("recall");
    let expected = [(steering, steering_text), (hit, hit_text), (row, held_text)];
    let served: Vec<(Uuid, u64)> = recall
        .steering
        .iter()
        .chain(recall.hits.iter().map(|h| &h.served))
        .map(|m| (m.memory.memory_id, budget_tokens(m.memory.body_bytes)))
        .collect();
    assert_eq!(served.len(), 3, "{served:?}");
    for (id, text) in expected {
        assert!(served.contains(&(id, tokens(text))), "{text}: {served:?}");
    }
}

#[tokio::test]
async fn the_same_budgeted_recall_answers_identically_twice() {
    let (app, reader, _steering, hits, s, t) = budget_fixture().await;
    let req = budgeted("canary drain settle", s + t[0] + t[2]);
    let first = app.recall(&reader.value, &req).await.expect("recall");
    let second = app.recall(&reader.value, &req).await.expect("recall");
    assert_eq!(hit_ids_of(&first), vec![hits[0]]);
    assert!(first.budget.is_some_and(|b| b.truncated));
    assert_eq!(answer(&first), answer(&second));
}

#[tokio::test]
async fn a_recall_without_max_tokens_carries_no_budget_object() {
    let (app, reader, steering, hits, _s, _t) = budget_fixture().await;
    let recall = app
        .recall(&reader.value, &recall_req("canary drain settle"))
        .await
        .expect("recall");
    assert_eq!(recall.budget, None);
    assert_eq!(hit_ids_of(&recall), hits, "nothing is cut");
    let mut served = vec![steering];
    served.extend(&hits);
    assert_eq!(recall.served_ids, served);
}

#[tokio::test]
async fn max_tokens_with_include_bodies_false_is_request_validation() {
    let (app, reader, _steering, _hits, _s, _t) = budget_fixture().await;
    let bodiless = RecallRequest {
        include_bodies: false,
        ..recall_req("canary")
    };
    let err = app
        .recall(
            &reader.value,
            &RecallRequest {
                max_tokens: Some(500),
                ..bodiless.clone()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BudgetWithoutBodies), "{err:?}");
    // Either alone is a recall.
    app.recall(&reader.value, &bodiless)
        .await
        .expect("no bodies, no budget");
    app.recall(&reader.value, &budgeted("canary", 500))
        .await
        .expect("a budget over bodies");
}

#[tokio::test]
async fn query_digest_changes_when_max_tokens_or_include_own_held_change() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    let digest = |req: RecallRequest| {
        let (app, key) = (&app, reader.value.clone());
        async move { app.recall(&key, &req).await.expect("recall").query_digest }
    };

    // Every member is present with its default applied, the scope the
    // resolved node (retrieval.md, the read log; ADR-0025 d9).
    let base = digest(recall_req("canary")).await;
    let canonical = format!(
        "{{\"expand_terms\":\"none\",\"include_bodies\":true,\"include_own_held\":false,\
         \"k\":10,\"kinds\":[],\"max_tokens\":null,\"query\":\"canary\",\
         \"scope\":\"{crew}\",\"terms\":[]}}"
    );
    assert_eq!(base, sha256_digest_string(canonical.as_bytes()));
    assert_eq!(
        digest(recall_req("canary")).await,
        base,
        "one request, one digest"
    );
    assert_eq!(
        digest(RecallRequest {
            workarea_id: Some(crew.into()),
            ..recall_req("canary")
        })
        .await,
        base,
        "the default named is the same scope"
    );

    let budget = digest(budgeted("canary", 500)).await;
    assert_ne!(budget, base, "max_tokens is digested");
    assert_ne!(digest(budgeted("canary", 501)).await, budget);
    assert_ne!(
        digest(own_held("canary")).await,
        base,
        "include_own_held is digested"
    );
    assert_ne!(
        digest(RecallRequest {
            include_bodies: false,
            ..recall_req("canary")
        })
        .await,
        base
    );

    // Only the order and repetition of kinds entries never matter, and the
    // query digests as its NFC text.
    let kinds = |kinds: Vec<MemoryKind>| RecallRequest {
        kinds,
        ..recall_req("canary")
    };
    assert_eq!(
        digest(kinds(vec![MemoryKind::Memory, MemoryKind::DerivedFact])).await,
        digest(kinds(vec![
            MemoryKind::DerivedFact,
            MemoryKind::Memory,
            MemoryKind::Memory
        ]))
        .await
    );
    assert_ne!(digest(kinds(vec![MemoryKind::Memory])).await, base);
    assert_eq!(
        digest(recall_req("cafe\u{301}")).await,
        digest(recall_req("café")).await
    );
}

// ---------------------------------------------------------------------------
// Names: resolution after the verb ceiling, name.taken, the universe root,
// names and crews at issuance, rows that predate names, and the admin
// structure reads (ADR-0026 d4, d5, d6, d8, d9, d11, d15)
// ---------------------------------------------------------------------------

/// A first version of a claim into the workarea `target` names.
fn submit_to(idem: &str, body: &str, target: Reference) -> SubmitRequest {
    SubmitRequest {
        workarea_id: Some(target),
        ..submit_req(idem, body)
    }
}

/// A refusal as its two renderings: the message and the value. An
/// application refusal is the transport's whole error body, so two equal
/// pairs answer one body (errors.md, implementation mapping).
fn rendered(err: &AppError) -> (String, String) {
    (err.to_string(), format!("{err:?}"))
}

/// `X-OML-Workarea` names the node by name; the server replaces the name by
/// the node's id after the verb ceiling, and the request runs as the same
/// request by id runs — which is the same request, so it replays
/// (http-api.md names and ids; ADR-0026 d4).
#[tokio::test]
async fn a_writer_key_submitting_with_its_workareas_name_lands_in_that_workarea() {
    let app = app();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let other = workarea(&app, admin, "other", boot.root_workarea_id).await;
    let metals = workarea(&app, admin, "metals", boot.root_workarea_id).await;
    // The key's default is `other`, so only the name can land the write in
    // `metals`.
    let writer = issue_with(
        &app,
        admin,
        Role::Writer,
        "wanda",
        vec![
            grant(other, Role::Writer),
            grant(metals, Role::Writer),
            grant(metals, Role::Reader),
        ],
    )
    .await;
    assert_eq!(writer.default_workarea, Some(other));

    let req = submit_to("s-1", "copper rose in march", by_name("metals"));
    let first = app
        .submit_memory(&writer.value, &req)
        .await
        .expect("submit by name");
    assert!(!first.replayed);
    assert_eq!(first.receipt.workarea_id, metals);
    let stored = app
        .get_memory(&writer.value, first.receipt.memory_id)
        .await
        .expect("get its own row");
    assert_eq!(stored.memory.workarea_id, metals);
    assert_eq!(stored.workarea_path.as_deref(), Some("universe.metals"));

    // The header is resolved before the record is looked up, so the same
    // request with the id in the header replays (http-api.md idempotency).
    let by_id = app
        .submit_memory(
            &writer.value,
            &SubmitRequest {
                workarea_id: Some(metals.into()),
                ..req.clone()
            },
        )
        .await
        .expect("the same request by id");
    assert!(by_id.replayed);
    assert_eq!(by_id.receipt, first.receipt);
}

/// An unknown name and a name outside the key's access answer
/// `access.not_found` at the same step, with the same body, as the id would:
/// names add no refusal path (http-api.md names and ids; ADR-0026 d4).
#[tokio::test]
async fn a_name_the_key_holds_no_grant_on_answers_not_found_with_the_same_body_as_its_id() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let metals = workarea(&app, admin, "metals", boot.root_workarea_id).await;
    let hidden = workarea(&app, admin, "hidden", boot.root_workarea_id).await;
    let agent = agent(&app, admin, "wanda", metals).await;
    let head = journal(&store, boot.tenant_id).await.len();

    let submit = |target: Reference| {
        let (app, key) = (&app, &agent.value);
        async move {
            app.submit_memory(key, &submit_to("s-1", "copper", target))
                .await
                .expect_err("refused")
        }
    };
    let by_id = submit(hidden.into()).await;
    assert!(matches!(by_id, AppError::NotFound), "{by_id:?}");
    for target in [by_name("hidden"), by_name("nowhere")] {
        assert_eq!(rendered(&submit(target).await), rendered(&by_id));
    }

    let recall = |target: Reference| {
        let (app, key) = (&app, &agent.value);
        async move {
            app.recall(
                key,
                &RecallRequest {
                    workarea_id: Some(target),
                    ..recall_req("copper")
                },
            )
            .await
            .expect_err("refused")
        }
    };
    let by_id = recall(hidden.into()).await;
    assert!(matches!(by_id, AppError::NotFound), "{by_id:?}");
    for target in [by_name("hidden"), by_name("nowhere")] {
        assert_eq!(rendered(&recall(target).await), rendered(&by_id));
    }

    let info = |target: Reference| {
        let (app, key) = (&app, &agent.value);
        async move {
            app.workarea_info(key, Some(target))
                .await
                .expect_err("refused")
        }
    };
    let by_id = info(hidden.into()).await;
    assert!(matches!(by_id, AppError::NotFound), "{by_id:?}");
    for target in [by_name("hidden"), by_name("nowhere")] {
        assert_eq!(rendered(&info(target).await), rendered(&by_id));
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// Step 1 of the refusal order needs no row, so a reader key's submit is
/// `access.verb_ceiling` whether its header names a node that exists or one
/// that does not, and no name is looked up first (roles-and-keys.md refusal
/// order; ADR-0025 d1; ADR-0026 d4).
#[tokio::test]
async fn a_reader_key_submitting_by_workarea_name_is_refused_at_the_verb_ceiling_before_resolution()
{
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let reader = issue(&app, &boot.admin_key.value, Role::Reader, "rita", crew).await;
    let lookups = store.name_lookups();
    for target in [by_name("crew"), by_name("nowhere")] {
        let err = app
            .submit_memory(&reader.value, &submit_to("s-1", "copper", target))
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                AppError::VerbCeiling {
                    role: Role::Reader,
                    verb: "submit"
                }
            ),
            "{err:?}"
        );
    }
    assert_eq!(
        store.name_lookups(),
        lookups,
        "the ceiling answered before any name was resolved"
    );
}

/// Names are unique in the tenant across the whole tree, not among
/// siblings: a second workarea of a taken name answers `name.taken` under
/// any parent, and journals nothing (ADR-0026 d1, d5).
#[tokio::test]
async fn a_second_workarea_with_a_taken_name_answers_name_taken_and_journals_nothing() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let first = workarea(&app, admin, "procurement", boot.root_workarea_id).await;
    let team = workarea(&app, admin, "team", boot.root_workarea_id).await;
    let head = hashes(&journal(&store, boot.tenant_id).await);
    for parent in [boot.root_workarea_id, team] {
        let err = app
            .create_workarea(admin, &name("procurement"), Some(parent.into()), None)
            .await
            .unwrap_err();
        assert!(
            matches!(
                &err,
                AppError::NameTaken { kind: NamedKind::Workarea, name } if name.as_str() == "procurement"
            ),
            "{err:?}"
        );
    }
    assert_eq!(hashes(&journal(&store, boot.tenant_id).await), head);
    assert_eq!(
        store
            .resolve_name(boot.tenant_id, NamedKind::Workarea, &name("procurement"))
            .await
            .expect("resolve"),
        Some(first),
        "the name still means the first row"
    );
}

/// Every tenant's root is `universe` from its creation, so a workarea create
/// naming `universe` answers `name.taken`, with no special case (ADR-0026
/// d5, d6).
#[tokio::test]
async fn creating_a_workarea_named_universe_answers_name_taken() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let head = journal(&store, boot.tenant_id).await.len();
    for parent in [boot.root_workarea_id, crew] {
        let err = app
            .create_workarea(admin, &name("universe"), Some(parent.into()), None)
            .await
            .unwrap_err();
        assert!(
            matches!(
                &err,
                AppError::NameTaken { kind: NamedKind::Workarea, name } if name.as_str() == "universe"
            ),
            "{err:?}"
        );
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// Tenant creation creates the root named `universe` and journals the name
/// in the root's `workarea.create`; `tenant.genesis` records the tenant's
/// name (ADR-0026 d1, d6; event-catalog.md).
#[tokio::test]
async fn bootstrap_names_the_root_universe() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let boot = bootstrap(&app).await;
    let root = store
        .workarea(boot.tenant_id, boot.root_workarea_id)
        .await
        .expect("read")
        .expect("the root is stored");
    assert_eq!(root.name.as_str(), "universe");
    assert_eq!(root.parent_id, None);
    assert_eq!(
        store
            .resolve_name(boot.tenant_id, NamedKind::Workarea, &name("universe"))
            .await
            .expect("resolve"),
        Some(boot.root_workarea_id)
    );

    let events = journal(&store, boot.tenant_id).await;
    assert_eq!(events[0].envelope.event_type, event_types::TENANT_GENESIS);
    assert_eq!(
        events[0].envelope.payload,
        json!({ "tenant_id": boot.tenant_id.to_string(), "name": "acme" })
    );
    assert_eq!(events[1].envelope.event_type, event_types::WORKAREA_CREATE);
    assert_eq!(
        events[1].envelope.payload,
        json!({
            "workarea_id": boot.root_workarea_id.to_string(),
            "parent_id": Value::Null,
            "name": "universe",
            "status": "committed",
            "crew_id": Value::Null,
            "prefix": Value::Null,
        })
    );

    // The bootstrap admin key reads the root by its name.
    let info = app
        .workarea_info(&boot.admin_key.value, Some(by_name("universe")))
        .await
        .expect("the admin reads the root");
    assert_eq!(info.workarea.workarea_id, boot.root_workarea_id);
    assert_eq!(info.path, "universe");
}

/// The ceremony's admin key takes the name the claim gives it: `key.issued`
/// journals it, the name resolves to the key, and a tenant this build
/// bootstrapped and issues keys in holds no row without a name, so the
/// naming of rows that predate names finds nothing there and journals
/// nothing (ADR-0026 d9, d11; event-catalog.md `key.issued`).
#[tokio::test]
async fn bootstrap_names_its_admin_key_and_no_key_this_build_issues_is_left_for_the_naming() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    assert_eq!(boot.admin_key.name, name("root-admin"));
    let events = journal(&store, boot.tenant_id).await;
    let key_issued = events
        .iter()
        .find(|e| e.envelope.event_type == event_types::KEY_ISSUED)
        .expect("the ceremony's key.issued");
    assert_eq!(key_issued.envelope.payload["name"], "root-admin");
    assert_eq!(
        store
            .resolve_name(boot.tenant_id, NamedKind::Key, &name("root-admin"))
            .await
            .expect("resolve"),
        Some(boot.admin_key.key_id)
    );
    let writer = issue(
        &app,
        &boot.admin_key.value,
        Role::Writer,
        "crew/agent-7",
        crew,
    )
    .await;
    let stored = store
        .key(boot.tenant_id, writer.key_id)
        .await
        .expect("read")
        .expect("the key row");
    assert_eq!(stored.name, Some(writer.name));

    assert!(
        store
            .unnamed_rows(boot.tenant_id)
            .await
            .expect("read")
            .is_empty()
    );
    let head = journal(&store, boot.tenant_id).await.len();
    assert!(
        app.name_rows_that_predate_names(&[boot.tenant_id])
            .await
            .expect("the naming reads the chain")
            .is_empty()
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// Key issuance takes the key's name and an optional crew, by id or name;
/// `key.issued` journals the name and the crew record's id, never the name
/// it was sent as, and the key's crew is fixed for its life (ADR-0026 d8,
/// d9; event-catalog.md `key.issued`).
#[tokio::test]
async fn issuing_a_key_with_a_crew_name_journals_the_crews_id() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew_wa) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let crew = metals_crew(&app, admin).await;
    let issued = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                name: name("claude-laptop"),
                crew: Some(by_name("metals-crew")),
                ..issue_req(
                    Role::Writer,
                    "claude/laptop",
                    vec![grant(crew_wa, Role::Writer), grant(crew_wa, Role::Reader)],
                )
            },
        )
        .await
        .expect("issue");
    assert_eq!(issued.crew_id, Some(crew.crew_id));
    assert_eq!(issued.name, name("claude-laptop"));

    let events = journal(&store, boot.tenant_id).await;
    let key_issued = events
        .iter()
        .find(|e| {
            e.envelope.event_type == event_types::KEY_ISSUED
                && e.envelope.payload["key_id"] == issued.key_id.to_string()
        })
        .expect("key.issued");
    assert_eq!(
        key_issued.envelope.payload["crew_id"],
        crew.crew_id.to_string()
    );
    assert_eq!(key_issued.envelope.payload["name"], "claude-laptop");
    let stored = store
        .key(boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("the key row");
    assert_eq!(stored.crew_id, Some(crew.crew_id));
    assert_eq!(
        store
            .resolve_name(boot.tenant_id, NamedKind::Key, &name("claude-laptop"))
            .await
            .expect("resolve"),
        Some(issued.key_id)
    );

    // A crew that names no crew record of the tenant is absent at step 4,
    // as an unknown id is, and nothing is journalled.
    let head = journal(&store, boot.tenant_id).await.len();
    for crew_ref in [by_name("nowhere-crew"), Uuid::from_u128(0xDEAD).into()] {
        let err = app
            .issue_key(
                admin,
                &IssueKeyRequest {
                    name: name("claude-desktop"),
                    crew: Some(crew_ref),
                    ..issue_req(
                        Role::Writer,
                        "claude/desktop",
                        vec![grant(crew_wa, Role::Writer)],
                    )
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    // A taken name is checked last, after every other check, and journals
    // nothing: no key.issued commits.
    let err = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                name: name("claude-laptop"),
                ..issue_req(
                    Role::Writer,
                    "claude/other",
                    vec![grant(crew_wa, Role::Writer)],
                )
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AppError::NameTaken { kind: NamedKind::Key, name } if name.as_str() == "claude-laptop"),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// One tenant's chain in the shapes the PoC journalled, signed with the
/// test key, as migration 0006 leaves it (ADR-0026 d11).
struct PocChain {
    tenant_id: Uuid,
    key: SigningKey,
    events: Vec<SignedEnvelope>,
}

impl PocChain {
    fn new(tenant_id: Uuid) -> Self {
        Self {
            tenant_id,
            key: SigningKey::from_bytes(&TEST_SEED),
            events: Vec::new(),
        }
    }

    fn push(&mut self, event_type: &str, payload: Value) -> &mut Self {
        let n = u64::try_from(self.events.len()).expect("a short chain");
        let draft = EventDraft {
            tenant_id: self.tenant_id,
            event_id: Uuid::from_u128(0xE000 + u128::from(n)),
            event_type: event_type.to_owned(),
            principal: "oml/admin".to_owned(),
            api_key_id: None,
            at: EventTime::parse(&format!("2026-10-01T09:00:{n:02}.000000Z")).expect("time"),
            payload,
        };
        let envelope = next_envelope(self.events.last(), draft).expect("a lawful draft");
        self.events
            .push(sign(envelope, &self.key).expect("signing"));
        self
    }

    /// A PoC `workarea.create`: the name as the PoC took it, and the crew
    /// object it copied in.
    fn workarea(
        &mut self,
        id: Uuid,
        parent: Option<Uuid>,
        wa_name: &str,
        crew: Value,
    ) -> &mut Self {
        self.push(
            event_types::WORKAREA_CREATE,
            json!({
                "workarea_id": id.to_string(),
                "parent_id": parent.map(|p| p.to_string()),
                "name": wa_name,
                "status": if parent.is_none() { "committed" } else { "development" },
                "crew": crew,
                "prefix": Value::Null,
            }),
        )
    }

    /// A PoC `key.issued`, which carried no name.
    fn key(&mut self, id: Uuid, role: &str, principal: &str) -> &mut Self {
        self.push(
            event_types::KEY_ISSUED,
            json!({
                "key_id": id.to_string(),
                "role": role,
                "principal": principal,
                "issued_by_key_id": Value::Null,
                "expiry": Value::Null,
                "value_disclosed": false,
                "held_by": Value::Null,
                "default_workarea_id": Value::Null,
            }),
        )
    }
}

fn derived(kind: NamedKind, id: Uuid) -> String {
    Name::derived(kind, id).as_str().to_owned()
}

/// At its first start on the names schema the lead names every row that
/// predates names under rules 1–4 and journals the result as one
/// `system.names_assigned` on the chain, whose fold alone gives those rows
/// their names; every earlier event stays as it was signed, and a second
/// start finds nothing to name and journals nothing (http-api.md rows
/// created before names; ADR-0026 d11).
#[tokio::test]
async fn the_lead_names_legacy_rows_once_one_system_names_assigned_per_chain_and_a_second_start_journals_nothing()
 {
    let tenant = Uuid::from_u128(0xA600);
    let id = |n: u128| Uuid::from_u128(0xB600 + n);
    let (root, procurement, team, again, impostor, review) =
        (id(0), id(1), id(2), id(3), id(4), id(5));
    let (admin, writer) = (Uuid::from_u128(0xD600), Uuid::from_u128(0xD601));
    let procurement_crew = json!({ "name": "Procurement Crew" });
    let mut poc = PocChain::new(tenant);
    poc.push(event_types::TENANT_GENESIS, json!({ "tenant_name": "oml" }))
        .workarea(root, None, "root", Value::Null)
        .workarea(
            procurement,
            Some(root),
            "procurement",
            procurement_crew.clone(),
        )
        .workarea(team, Some(root), "Procurement Team", procurement_crew)
        .workarea(again, Some(team), "procurement", Value::Null)
        .workarea(impostor, Some(team), "universe", Value::Null)
        .workarea(
            review,
            Some(root),
            "lit-review",
            json!({
                "name": "redline",
                "charter_uri": "https://crews.example.com/redline/charter.md",
                "external_manager": "crewd.example.com",
            }),
        )
        .key(admin, "admin", "oml/admin")
        .key(writer, "writer", "crew/agent-7")
        .push(
            event_types::GRANT_ISSUED,
            json!({
                "grant_id": id(10).to_string(), "key_id": writer.to_string(),
                "workarea_node_id": procurement.to_string(), "role": "writer",
                "deny": [], "epoch": 10,
            }),
        )
        .push(
            event_types::KEY_REVOKED,
            json!({ "key_id": writer.to_string(), "revoked_by_key_id": admin.to_string() }),
        );
    let store = MemStore::new();
    store
        .migrated_poc_chain(poc.events.clone())
        .expect("the PoC chain as 0006 leaves it");
    assert!(!store.unnamed_rows(tenant).await.expect("read").is_empty());
    let app = app_over(store.clone());

    let assigned = app
        .name_rows_that_predate_names(&[tenant])
        .await
        .expect("the first start names the chain");
    let events = journal(&store, tenant).await;
    assert_eq!(events.len(), poc.events.len() + 1, "one event per chain");
    assert_eq!(
        hashes(&events[..poc.events.len()]),
        hashes(&poc.events),
        "every earlier event stays as it was signed"
    );
    let naming = &events[poc.events.len()].envelope;
    assert_eq!(naming.event_type, event_types::SYSTEM_NAMES_ASSIGNED);
    assert_eq!(naming.principal, "oml-server");
    assert_eq!(naming.api_key_id, None);
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0].tenant_id, tenant);
    assert_eq!(assigned[0].journal_seq, naming.seq);

    let crews = naming.payload["crews"].as_array().expect("crews");
    let crew_id =
        |i: usize| Uuid::parse_str(crews[i]["crew_id"].as_str().expect("crew_id")).expect("an id");
    let entry = |kind: &str, row: Uuid, row_name: String, prior: Value| json!({ "kind": kind, "id": row.to_string(), "name": row_name, "prior_name": prior });
    assert_eq!(
        naming.payload,
        json!({
            "rule": "ADR-0026 d11",
            "names": [
                entry("tenant", tenant, "oml".to_owned(), json!("oml")),
                // Rule 1: the root is universe, whatever its create said.
                entry("workarea", root, "universe".to_owned(), json!("root")),
                // Rule 2: a usable name no earlier row holds is kept.
                entry("workarea", procurement, "procurement".to_owned(), json!("procurement")),
                // Rule 3: ungrammatical, held by an earlier row, or a
                // non-root universe — derived from the id.
                entry("workarea", team, derived(NamedKind::Workarea, team), json!("Procurement Team")),
                entry("workarea", again, derived(NamedKind::Workarea, again), json!("procurement")),
                entry("workarea", impostor, derived(NamedKind::Workarea, impostor), json!("universe")),
                entry("workarea", review, "lit-review".to_owned(), json!("lit-review")),
                // Every key before names had none.
                entry("key", admin, derived(NamedKind::Key, admin), Value::Null),
                entry("key", writer, derived(NamedKind::Key, writer), Value::Null),
            ],
            // Rule 4: one record per distinct crew object, in order of first
            // appearance, named by rules 2 and 3.
            "crews": [
                {
                    "crew_id": crew_id(0).to_string(),
                    "name": derived(NamedKind::Crew, crew_id(0)),
                    "charter_uri": Value::Null,
                    "external_manager": Value::Null,
                    "workarea_ids": [procurement.to_string(), team.to_string()],
                },
                {
                    "crew_id": crew_id(1).to_string(),
                    "name": "redline",
                    "charter_uri": "https://crews.example.com/redline/charter.md",
                    "external_manager": "crewd.example.com",
                    "workarea_ids": [review.to_string()],
                },
            ],
        })
    );

    // The fold gave every row the name the event assigns, and the store
    // serves them: paths derive from the names, the crew objects are
    // records, and a revoked key's derived name still resolves.
    assert!(store.unnamed_rows(tenant).await.expect("read").is_empty());
    assert_eq!(
        store
            .resolve_name(tenant, NamedKind::Workarea, &name("universe"))
            .await
            .expect("resolve"),
        Some(root)
    );
    let ancestry: Vec<String> = store
        .workarea_ancestry(tenant, again)
        .await
        .expect("read")
        .expect("the node")
        .iter()
        .rev()
        .map(|w| w.name.as_str().to_owned())
        .collect();
    assert_eq!(
        ancestry,
        vec![
            "universe".to_owned(),
            derived(NamedKind::Workarea, team),
            derived(NamedKind::Workarea, again),
        ]
    );
    for (node, crew) in [
        (procurement, crew_id(0)),
        (team, crew_id(0)),
        (review, crew_id(1)),
    ] {
        let stored = store
            .workarea(tenant, node)
            .await
            .expect("read")
            .expect("node");
        assert_eq!(stored.crew_id, Some(crew), "{node}");
    }
    let redline = store
        .crew(tenant, crew_id(1))
        .await
        .expect("read")
        .expect("the crew record");
    assert_eq!(redline.name.as_str(), "redline");
    let writer_name = Name::parse_reference(&derived(NamedKind::Key, writer)).expect("a name");
    assert_eq!(
        store
            .resolve_name(tenant, NamedKind::Key, &writer_name)
            .await
            .expect("resolve"),
        Some(writer)
    );
    let keys = KeySet::new(vec![SigningKey::from_bytes(&TEST_SEED).verifying_key()]);
    verify_chain(&events, &keys, None).expect("the named chain verifies");

    // A second start finds no row without a name, and journals nothing.
    let again_assigned = app
        .name_rows_that_predate_names(&[tenant])
        .await
        .expect("the second start");
    assert!(again_assigned.is_empty());
    assert_eq!(hashes(&journal(&store, tenant).await), hashes(&events));
}

/// Rule 5: every global denial on an admin key that is not yet lifted is
/// lifted in the commit that names the chain, after its
/// `system.names_assigned` and in the journal order of the denials, each as
/// one `key.denial_lifted` the server signs with no lifting key, its
/// `reason_digest` over the fixed reason `adr-0026-d15` and its epoch its own
/// seq. The denial then masks nothing and its lift is the key's permission
/// epoch, a denial on a writer key stays in force, and a commit refused on a
/// lift journals neither the lifts nor the names (http-api.md rows created
/// before names, rule 5; event-catalog.md `key.denial_lifted`; ADR-0026 d11,
/// d15).
#[tokio::test]
async fn the_naming_lifts_every_denial_on_an_admin_key_in_the_commit_that_names_the_chain() {
    let tenant = Uuid::from_u128(0xA700);
    let (root, team) = (Uuid::from_u128(0xB700), Uuid::from_u128(0xB701));
    let (admin, steward, writer) = (
        Uuid::from_u128(0xD700),
        Uuid::from_u128(0xD701),
        Uuid::from_u128(0xD702),
    );
    let (on_admin, on_writer, on_steward) = (
        Uuid::from_u128(0xF700),
        Uuid::from_u128(0xF701),
        Uuid::from_u128(0xF702),
    );
    let denied = |denial: Uuid, key: Uuid, node: Uuid| {
        json!({
            "denial_id": denial.to_string(), "key_id": key.to_string(),
            "workarea_node_id": node.to_string(),
        })
    };
    let mut poc = PocChain::new(tenant);
    poc.push(event_types::TENANT_GENESIS, json!({ "tenant_name": "oml" }))
        .workarea(root, None, "root", Value::Null)
        .workarea(team, Some(root), "team", Value::Null)
        .key(admin, "admin", "oml/admin")
        .key(steward, "admin", "oml/steward")
        .key(writer, "writer", "crew/agent-7")
        .push(event_types::KEY_DENIED, denied(on_admin, admin, team))
        .push(event_types::KEY_DENIED, denied(on_writer, writer, team))
        .push(event_types::KEY_DENIED, denied(on_steward, steward, root));
    let refused = MemStore::new().refusing_next_commit_of(event_types::KEY_DENIAL_LIFTED);
    refused
        .migrated_poc_chain(poc.events.clone())
        .expect("the PoC chain");
    let err = app_over(refused.clone())
        .name_rows_that_predate_names(&[tenant])
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Store(_)), "{err:?}");
    assert_eq!(
        hashes(&journal(&refused, tenant).await),
        hashes(&poc.events),
        "a refused lift journals neither the lifts nor the names"
    );
    assert!(!refused.unnamed_rows(tenant).await.expect("read").is_empty());

    let store = MemStore::new();
    store
        .migrated_poc_chain(poc.events.clone())
        .expect("the PoC chain");
    let assigned = app_over(store.clone())
        .name_rows_that_predate_names(&[tenant])
        .await
        .expect("the start names the chain and lifts its admin-key denials");
    let events = journal(&store, tenant).await;
    let n = poc.events.len();
    assert_eq!(events.len(), n + 3, "the names, then one lift per denial");
    assert_eq!(
        events[n].envelope.event_type,
        event_types::SYSTEM_NAMES_ASSIGNED
    );
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0].journal_seq, events[n].envelope.seq);
    assert_eq!(assigned[0].lifted_denials, vec![on_admin, on_steward]);
    let reason = sha256_digest_string(b"adr-0026-d15");
    for (lift, (denial, key)) in events[n + 1..]
        .iter()
        .zip([(on_admin, admin), (on_steward, steward)])
    {
        let env = &lift.envelope;
        assert_eq!(env.event_type, event_types::KEY_DENIAL_LIFTED);
        assert_eq!(env.principal, "oml-server");
        assert_eq!(env.api_key_id, None);
        assert_eq!(
            env.payload,
            json!({
                "denial_id": denial.to_string(),
                "key_id": key.to_string(),
                "lifted_by_key_id": Value::Null,
                "reason_digest": reason,
                "epoch": env.seq,
            })
        );
        let access = store
            .key_access(tenant, key)
            .await
            .expect("read")
            .expect("the key is held");
        assert!(access.denials.is_empty(), "{key}: the denial masks nothing");
        assert_eq!(access.permission_epoch(), env.seq, "{key}");
    }
    let writer_access = store
        .key_access(tenant, writer)
        .await
        .expect("read")
        .expect("the key is held");
    assert_eq!(
        writer_access
            .denials
            .iter()
            .map(|d| d.denial.denial_id)
            .collect::<Vec<_>>(),
        vec![on_writer],
        "a denial on a writer key stays in force"
    );
    assert!(store.unnamed_rows(tenant).await.expect("read").is_empty());
    let keys = KeySet::new(vec![SigningKey::from_bytes(&TEST_SEED).verifying_key()]);
    verify_chain(&events, &keys, None).expect("the named chain verifies");
}

/// A chain whose `system.names_assigned` is journalled holds no row without
/// a name; finding one, the start refuses by name rather than journal a
/// second assignment (postgres-schema.md names and tenants; ADR-0026 d11).
#[tokio::test]
async fn a_row_with_no_name_in_a_chain_already_named_refuses_the_naming() {
    let tenant = Uuid::from_u128(0xA800);
    let root = Uuid::from_u128(0xB800);
    let mut poc = PocChain::new(tenant);
    poc.push(event_types::TENANT_GENESIS, json!({ "tenant_name": "oml" }))
        .workarea(root, None, "root", Value::Null)
        .push(
            event_types::SYSTEM_NAMES_ASSIGNED,
            json!({ "rule": "ADR-0026 d11", "names": [], "crews": [] }),
        );
    let store = MemStore::new();
    store
        .migrated_poc_chain(poc.events.clone())
        .expect("the chain");
    let app = app_over(store.clone());
    let err = app
        .name_rows_that_predate_names(&[tenant])
        .await
        .unwrap_err();
    assert!(
        matches!(
            &err,
            AppError::NamingRefused { tenant_id, refusal: NamingRefusal::AlreadyAssigned { seq: 3 } }
                if *tenant_id == tenant
        ),
        "{err:?}"
    );
    assert_eq!(hashes(&journal(&store, tenant).await), hashes(&poc.events));
}

/// An admin key reads the tree of any workarea in its tenant by its key
/// role, with no grant: every node of the subtree, each with its derived
/// path; a reader key reads only what its grants reach (roles-and-keys.md
/// permission matrix, footnote 6; ADR-0026 d15).
#[tokio::test]
async fn an_admin_key_reads_the_workarea_tree_by_name() {
    let app = app();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let metals = workarea(&app, admin, "metals", boot.root_workarea_id).await;
    let q1 = workarea(&app, admin, "q1-2027", metals).await;
    let q2 = workarea(&app, admin, "q2-2027", metals).await;
    let review = workarea(&app, admin, "lit-review", q1).await;
    let _other = workarea(&app, admin, "other", boot.root_workarea_id).await;

    let tree = |key: &str, node: Reference| {
        let (app, key) = (&app, key.to_owned());
        async move {
            app.workarea_tree(&key, node).await.map(|nodes| {
                nodes
                    .into_iter()
                    .map(|n| (n.workarea.workarea_id, n.path))
                    .collect::<Vec<_>>()
            })
        }
    };
    assert_eq!(
        tree(admin, by_name("metals"))
            .await
            .expect("the admin's tree"),
        vec![
            (metals, "universe.metals".to_owned()),
            (q1, "universe.metals.q1-2027".to_owned()),
            (q2, "universe.metals.q2-2027".to_owned()),
            (review, "universe.metals.q1-2027.lit-review".to_owned()),
        ]
    );
    // An unknown name answers what an unknown id does.
    for unknown in [by_name("nowhere"), Uuid::from_u128(0xDEAD).into()] {
        let err = tree(admin, unknown).await.unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }

    // A reader holding q1 only: metals is outside its access, and the
    // tree at q1 omits nothing it can read.
    let reader = issue(&app, admin, Role::Reader, "rita", q1).await;
    let err = tree(&reader.value, by_name("metals")).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    assert_eq!(
        tree(&reader.value, by_name("q1-2027"))
            .await
            .expect("its tree"),
        vec![
            (q1, "universe.metals.q1-2027".to_owned()),
            (review, "universe.metals.q1-2027.lit-review".to_owned()),
        ]
    );
}

/// The admin key's workarea read is structure: every child, no effective
/// verb, its own permission epoch, and nothing of the claims the node
/// holds; the content verbs still answer `access.verb_ceiling` at step 1
/// (roles-and-keys.md permission matrix, footnote 6; ADR-0026 d15).
#[tokio::test]
async fn an_admin_key_reading_a_workarea_sees_no_claim_content() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let child = workarea(&app, admin, "child", crew).await;
    let sealed = workarea(&app, admin, "sealed", crew).await;
    let (writer, _reader, dreamer) = crew_keys(&app, admin, crew).await;
    let body = "the copper contract closes at 4182";
    let memory = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_into("s-1", body, crew),
    )
    .await;

    let info = app
        .workarea_info(admin, Some(by_name("crew")))
        .await
        .expect("the admin reads the structure");
    assert_eq!(info.workarea.workarea_id, crew);
    assert_eq!(info.path, "universe.crew");
    assert_eq!(
        info.children
            .iter()
            .map(|c| c.workarea_id)
            .collect::<Vec<_>>(),
        vec![child, sealed],
        "every child, in name order"
    );
    assert!(info.my_effective_verbs.is_empty());
    assert_eq!(
        info.permission_epoch,
        boot.admin_key.access.permission_epoch()
    );
    let shown = format!("{info:?}");
    assert!(!shown.contains(body), "{shown}");
    assert!(!shown.contains(&memory.to_string()), "{shown}");

    let refused = |err: AppError, verb: &str| {
        assert!(
            matches!(&err, AppError::VerbCeiling { role: Role::Admin, verb: v } if *v == verb),
            "{err:?}"
        );
    };
    refused(app.get_memory(admin, memory).await.unwrap_err(), "get");
    refused(
        app.list_memories(
            admin,
            &ListRequest {
                own: false,
                scope: Some(by_name("crew")),
                workarea_id: None,
                state: None,
                kind: None,
                claim_key: None,
                since: None,
                until: None,
                limit: 10,
                cursor: None,
            },
        )
        .await
        .unwrap_err(),
        "list",
    );
    refused(
        app.tail_events(admin, Some(by_name("crew")), 0, 100)
            .await
            .unwrap_err(),
        "events_tail",
    );
}

/// No admin key reaches recall: reader is never admissible on an admin key,
/// so recall answers `access.verb_ceiling` at step 1, by name or by default
/// (roles-and-keys.md refusal order; ADR-0026 d15).
#[tokio::test]
async fn an_admin_keys_recall_is_refused() {
    let app = app();
    let boot = bootstrap(&app).await;
    for scope in [Some(by_name("universe")), None] {
        let err = app
            .recall(
                &boot.admin_key.value,
                &RecallRequest {
                    workarea_id: scope,
                    ..recall_req("anything")
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                AppError::VerbCeiling {
                    role: Role::Admin,
                    verb: "recall"
                }
            ),
            "{err:?}"
        );
    }
}

/// Denials attenuate grants and an admin key holds none, so a global denial
/// on an admin key is refused, `request.validation` naming `key_id`, and
/// journals nothing: no structure read would honour it (http-api.md keys,
/// grants, denials; ADR-0026 d15).
#[tokio::test]
async fn a_global_denial_on_an_admin_key_is_refused_naming_key_id() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let second = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                name: name("second-admin"),
                ..issue_req(Role::Admin, "bob-admin", Vec::new())
            },
        )
        .await
        .expect("a second admin key");
    let head = journal(&store, boot.tenant_id).await.len();
    for key in [second.key_id.into(), by_name("second-admin")] {
        let err = app.deny_key(admin, key, by_name("crew")).await.unwrap_err();
        assert!(matches!(err, AppError::DenialOnAdminKey), "{err:?}");
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
    // A writer key's denial by names still journals.
    let writer = agent(&app, admin, "wanda", crew).await;
    app.deny_key(admin, writer.key_id.into(), by_name("crew"))
        .await
        .expect("a denial on a writer key");
}
