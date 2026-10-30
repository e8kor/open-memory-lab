//! Use-case tests over the in-memory doubles: the full happy path, the R2
//! refusals, the role ceilings, effective access through grants and denials
//! (ADR-0025 d1, d3), issuance in the issuance order (ADR-0025 d2), and the
//! idempotent replay.

use oml_app::App;
use oml_app::domain::access::{admissible_grant_roles, issuable_key_roles};
use oml_app::domain::{
    HeldBy, MemoryKind, MemoryStatus, Role, SignedEnvelope, event_types, sha256_digest_string,
};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, IssuedKey,
    RecallRequest, SubmitRequest, SupersedeRequest, Verdict, VersionRef,
};
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

async fn bootstrap(app: &TestApp) -> BootstrapResult {
    app.bootstrap_tenant(&BootstrapRequest {
        tenant_name: "acme".to_owned(),
        admin_principal: "alice-admin".to_owned(),
    })
    .await
    .expect("bootstrap")
}

/// A workarea under `parent`, created by the admin.
async fn workarea(app: &TestApp, admin: &str, name: &str, parent: Uuid) -> Uuid {
    app.create_workarea(admin, name, Some(parent))
        .await
        .expect("create workarea")
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
        node,
        role,
        deny: Vec::new(),
    }
}

fn issue_req(role: Role, principal: &str, grants: Vec<GrantRequest>) -> IssueKeyRequest {
    IssueKeyRequest {
        role,
        principal: principal.to_owned(),
        held_by: Some(HeldBy::Agent),
        default_workarea: None,
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
        workarea_id: Some(workarea_id),
        ..submit_req(idem, body)
    }
}

fn recall_req(query: &str) -> RecallRequest {
    RecallRequest {
        query: query.to_owned(),
        k: 10,
        workarea_id: None,
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

    // The tail serves the journal ascending with digest-only payloads.
    let tail = app.tail_events(&reader.value, 0, 100).await.expect("tail");
    assert_eq!(tail.len(), 14);
    assert_eq!(tail[0].envelope.event_type, event_types::TENANT_GENESIS);
    assert_eq!(
        tail[13].envelope.event_type,
        event_types::SUBMISSION_REMEMBER,
        "a supersession is a submission until it is ratified"
    );
    let submit_event = &tail[10].envelope;
    assert_eq!(submit_event.event_type, event_types::SUBMISSION_REMEMBER);
    assert!(
        submit_event.payload.get("body").is_none(),
        "digest-only payloads"
    );
    assert!(submit_event.payload.get("plaintext_digest").is_some());
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
                    workarea_id: Some(node),
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
        .deny_key(admin, reader.key_id, quarantine)
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
                workarea_id: Some(quarantine),
                ..recall_req("canary")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let err = app.get_memory(&reader.value, masked).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");

    // Global denials mask the key's own rows too (roles-and-keys.md#own).
    app.deny_key(admin, writer.key_id, quarantine)
        .await
        .expect("deny the author");
    let err = app.get_memory(&writer.value, masked).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    app.get_memory(&writer.value, kept)
        .await
        .expect("an own row outside the denial");

    // Only an admin key journals a denial; an unknown key or node is absent.
    let err = app
        .deny_key(&writer.value, reader.key_id, crew)
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
        let err = app.deny_key(admin, key_id, node).await.unwrap_err();
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
                node: crew,
                role: Role::Dreamer,
                deny: vec![masked],
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
                default_workarea: Some(sub),
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
        node: crew,
        role: Role::Writer,
        deny: vec![masked],
    };
    for default in [sibling, masked, boot.root_workarea_id] {
        let err = app
            .issue_key(
                admin,
                &IssueKeyRequest {
                    default_workarea: Some(default),
                    ..issue_req(Role::Writer, "w", vec![masking.clone()])
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::DefaultUncovered { workarea_id } if workarea_id == default),
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
            node: crew,
            role: Role::Writer,
            deny: vec![Uuid::from_u128(0xDEAD)],
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
                        node: crew,
                        role: Role::Writer,
                        deny: vec![sibling],
                    },
                ],
            ),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::DenyOutsideGrant {
                grant_index: 1,
                subtree_node_id,
            } if subtree_node_id == sibling
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
                node: crew,
                role: Role::Writer,
                deny: vec![sub],
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
    app.deny_key(admin, writer.key_id, crew)
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
                workarea_id: Some(boot.root_workarea_id),
                ..supersede_req("sup-1", first, "revised")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::WorkareaNotTarget { .. }), "{err:?}");
    app.supersede(
        &writer.value,
        &SupersedeRequest {
            workarea_id: Some(crew),
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
