//! Use-case tests over the in-memory doubles: the full happy path, the R2
//! refusals, the role ceilings, and the idempotent replay.

use oml_app::App;
use oml_app::domain::{HeldBy, MemoryStatus, Role, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, IssueKeyRequest, IssuedKey, RecallRequest,
    SubmitRequest, SupersedeRequest, Verdict,
};

/// The committed test key seed (docs/data/chain-format.md test vector key).
const TEST_SEED: [u8; 32] = [
    0x09, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn app() -> TestApp {
    App::new(
        MemStore::new(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
}

async fn bootstrap(app: &TestApp) -> BootstrapResult {
    app.bootstrap_tenant(&BootstrapRequest {
        tenant_name: "acme".to_owned(),
        admin_principal: "alice-admin".to_owned(),
    })
    .await
    .expect("bootstrap")
}

async fn issue(app: &TestApp, admin: &str, role: Role, principal: &str) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            role,
            principal: principal.to_owned(),
            held_by: HeldBy::Agent,
            default_workarea: None,
        },
    )
    .await
    .expect("issue key")
}

fn submit_req(idem: &str, body: &str) -> SubmitRequest {
    SubmitRequest {
        idempotency_key: idem.to_owned(),
        claim_key: Some("deploy-runbook".to_owned()),
        body: body.to_owned(),
        workarea_id: None,
        derives_from: Vec::new(),
    }
}

#[tokio::test]
async fn happy_path_end_to_end() {
    let app = app();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda-writer").await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana-dreamer").await;
    let reader = issue(&app, admin, Role::Reader, "rhea-reader").await;

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
    let memory_id = submitted.receipt.memory_id;

    // Held rows are invisible to everyone but the author (R3).
    let err = app.get_memory(&reader.value, memory_id).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound));
    let own = app
        .get_memory(&writer.value, memory_id)
        .await
        .expect("own-work read");
    assert_eq!(own.status, MemoryStatus::Held);

    // Recall before promotion finds nothing.
    let recall = app
        .recall(
            &reader.value,
            &RecallRequest {
                query: "canary".to_owned(),
                k: 10,
                workarea_id: None,
            },
        )
        .await
        .expect("recall");
    assert!(recall.hits.is_empty());

    // Review: the dreamer is served the held item and approves it.
    let served = app
        .review_next(&dreamer.value)
        .await
        .expect("review next")
        .expect("an item waits");
    assert_eq!(served.memory_id, memory_id);
    let outcome = app
        .post_verdict(&dreamer.value, memory_id, Verdict::Approve, None)
        .await
        .expect("verdict");
    assert_eq!(outcome.state, MemoryStatus::Promoted);

    // Recall now finds it; steering precedes hits (empty segment in PoC).
    let recall = app
        .recall(
            &reader.value,
            &RecallRequest {
                query: "canary".to_owned(),
                k: 10,
                workarea_id: None,
            },
        )
        .await
        .expect("recall");
    assert!(recall.steering.is_empty());
    assert_eq!(recall.hits.len(), 1);
    assert_eq!(recall.hits[0].memory_id, memory_id);

    // Supersede the promoted tip.
    let superseded = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                target_id: memory_id,
                body: "The canary stage must drain and settle before promotion.".to_owned(),
                claim_key: None,
            },
        )
        .await
        .expect("supersede");
    assert_eq!(superseded.superseded_id, memory_id);
    let old = app
        .get_memory(&reader.value, memory_id)
        .await
        .expect("get old tip");
    // Supersession adds no state: the replaced version stays promoted and
    // only loses the tip (ADR-0025 d10).
    assert_eq!(old.status, MemoryStatus::Promoted);
    assert_eq!(old.superseded_by, Some(superseded.successor_id));
    let successor = app
        .get_memory(&writer.value, superseded.successor_id)
        .await
        .expect("author reads own held successor");
    assert_eq!(successor.status, MemoryStatus::Held);

    // The whole chain verifies: genesis, workarea, 4 keys, submit, approve,
    // promote, supersede = 10 events.
    let head = app.verify_chain(admin).await.expect("verify chain");
    assert_eq!(head.seq, 10);
    assert_eq!(head.tenant_id, boot.tenant_id);

    // The tail serves the journal ascending with digest-only payloads.
    let tail = app.tail_events(&reader.value, 0, 100).await.expect("tail");
    assert_eq!(tail.len(), 10);
    assert_eq!(tail[0].envelope.event_type, event_types::TENANT_GENESIS);
    assert_eq!(
        tail[9].envelope.event_type,
        event_types::LIFECYCLE_SUPERSEDE
    );
    let submit_event = &tail[6].envelope;
    assert_eq!(submit_event.event_type, event_types::SUBMISSION_REMEMBER);
    assert!(
        submit_event.payload.get("body").is_none(),
        "digest-only payloads"
    );
    assert!(submit_event.payload.get("body_digest").is_some());
}

#[tokio::test]
async fn a_ratified_successor_is_the_one_hit_and_the_version_it_replaced_stays_promoted() {
    let app = app();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda").await;
    let reader = issue(&app, admin, Role::Reader, "rita").await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana").await;

    let first = app
        .submit_memory(&writer.value, &submit_req("i1", "the canary drains first"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    app.post_verdict(&dreamer.value, first, Verdict::Approve, None)
        .await
        .expect("promote");
    let second = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                target_id: first,
                body: "the canary drains and settles first".to_owned(),
                claim_key: None,
            },
        )
        .await
        .expect("supersede")
        .successor_id;
    app.post_verdict(&dreamer.value, second, Verdict::Approve, None)
        .await
        .expect("ratify the successor");

    // A superseded version is never a hit (retrieval.md; ADR-0025 d8).
    let recall = app
        .recall(
            &reader.value,
            &RecallRequest {
                query: "canary".to_owned(),
                k: 10,
                workarea_id: None,
            },
        )
        .await
        .expect("recall");
    let hits: Vec<_> = recall.hits.iter().map(|m| m.memory_id).collect();
    assert_eq!(hits, vec![second]);

    // It stays fetchable by id as history, still promoted (ADR-0025 d10).
    let old = app
        .get_memory(&reader.value, first)
        .await
        .expect("get the replaced version");
    assert_eq!(old.status, MemoryStatus::Promoted);
    assert_eq!(old.superseded_by, Some(second));
}

#[tokio::test]
async fn r2_author_cannot_verdict_its_own_work() {
    let app = app();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda").await;
    // A dreamer key held by the SAME principal as the author.
    let same_principal_dreamer = issue(&app, admin, Role::Dreamer, "wanda").await;
    let other_dreamer = issue(&app, admin, Role::Dreamer, "dana").await;

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
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda").await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana").await;
    let reader = issue(&app, admin, Role::Reader, "rhea").await;

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
    // Writer cannot recall (roles do not nest — roles-and-keys.md).
    let err = app
        .recall(
            &writer.value,
            &RecallRequest {
                query: "x".to_owned(),
                k: 1,
                workarea_id: None,
            },
        )
        .await
        .unwrap_err();
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
        .issue_key(
            &reader.value,
            &IssueKeyRequest {
                role: Role::Reader,
                principal: "p".to_owned(),
                held_by: HeldBy::Agent,
                default_workarea: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        AppError::VerbCeiling {
            role: Role::Reader,
            ..
        }
    ));
    // Admin cannot mint an operator key (issuer ceiling).
    let err = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                role: Role::Operator,
                principal: "p".to_owned(),
                held_by: HeldBy::Service,
                default_workarea: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::IssuerCeiling { .. }));
    // An unknown key fails closed.
    let err = app
        .recall(
            "oml_0000000000000000000000000000000000000000000",
            &RecallRequest {
                query: "x".to_owned(),
                k: 1,
                workarea_id: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Unauthorized));
}

#[tokio::test]
async fn idempotent_submit_replays_the_first_result() {
    let app = app();
    let boot = bootstrap(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda").await;

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
        head.seq, 5,
        "replay journalled nothing: genesis, workarea, 2 keys, 1 submit"
    );

    // Same key, different request: refused, never silently honored.
    let err = app
        .submit_memory(&writer.value, &submit_req("idem-7", "a different body"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::IdempotencyDigestMismatch));
}

#[tokio::test]
async fn scan_veto_is_a_journalled_rejection() {
    let app = app();
    let boot = bootstrap(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda").await;
    let dreamer = issue(&app, &boot.admin_key.value, Role::Dreamer, "dana").await;

    let err = app
        .submit_memory(
            &writer.value,
            &submit_req("idem-1", "-----BEGIN PRIVATE KEY----- oops"),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ScanVeto(_)));

    // The rejection was journalled (admission.md: a veto is a journalled
    // rejection, not a dropped request) and nothing waits in the queue.
    let head = app
        .verify_chain(&boot.admin_key.value)
        .await
        .expect("verify");
    assert_eq!(head.seq, 6, "the vetoed submission is event 6");
    let served = app.review_next(&dreamer.value).await.expect("review next");
    assert!(served.is_none(), "nothing waits in the queue after a veto");
}

#[tokio::test]
async fn supersede_is_author_only_and_tip_only() {
    let app = app();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda").await;
    let other_writer = issue(&app, admin, Role::Writer, "walt").await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana").await;

    let submitted = app
        .submit_memory(&writer.value, &submit_req("i1", "original fact"))
        .await
        .expect("submit");
    let memory_id = submitted.receipt.memory_id;

    // A held row has no promoted tip to supersede.
    let err = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                target_id: memory_id,
                body: "v2".to_owned(),
                claim_key: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::TipConflict));

    app.post_verdict(&dreamer.value, memory_id, Verdict::Approve, None)
        .await
        .expect("promote");

    // Another author is refused (access.author_only).
    let err = app
        .supersede(
            &other_writer.value,
            &SupersedeRequest {
                target_id: memory_id,
                body: "v2".to_owned(),
                claim_key: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AuthorOnly));

    // The author supersedes once; the second attempt hits a stale tip.
    app.supersede(
        &writer.value,
        &SupersedeRequest {
            target_id: memory_id,
            body: "v2".to_owned(),
            claim_key: None,
        },
    )
    .await
    .expect("supersede");
    let err = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                target_id: memory_id,
                body: "v3".to_owned(),
                claim_key: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::TipConflict));
}
