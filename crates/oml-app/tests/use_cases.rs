//! Use-case tests over the in-memory doubles: the full happy path, the R2
//! refusals, the role ceilings, effective access through grants and denials
//! (ADR-0025 d1, d3), issuance in the issuance order (ADR-0025 d2), the
//! idempotent replay, and names: resolution after the verb ceiling, crews,
//! names at issuance, the naming of rows that predate names and the admin
//! structure reads (ADR-0026 d4–d6, d8, d9, d11, d15).

use oml_app::App;
use oml_app::cursor::CursorError;
use oml_app::domain::access::{Covers, admissible_grant_roles, issuable_key_roles};
use oml_app::domain::budget::{Budget, budget_tokens};
use oml_app::domain::claim_edges::{EdgeField, LabelRefusal};
use oml_app::domain::name::{Name, NamedKind, Reference};
use oml_app::domain::oauth::{
    AuthorizationCodeInvalid, AuthorizationExpired, AuthorizationRequest, CLAUDE_CALLBACK,
    ConnectCode, ConnectRefused, ConnectionState, Issuer, RefreshInvalid, Refusal, RevokedCause,
    Token, Validation, open_request,
};
use oml_app::domain::vocab::OmlLabel;
use oml_app::domain::vocab_edges::EdgeState;
use oml_app::domain::{
    EventDraft, EventTime, FrozenOp, HeldBy, KeySet, LadderRefusal, MemoryKind, MemoryStatus, Role,
    Sha256Hash, SignedEnvelope, WorkareaStatus, canonical_bytes, derive_claim_key, event_types,
    next_envelope, sha256_digest_string, sign, verify_chain,
};
use oml_app::doubles::{
    FixedClock, MarkerScanner, MemDocuments, MemSigner, MemStore, PlainSealer, SeqIdGen,
};
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{
    CommitRequest, ConnectionStore as _, HandleSealer as _, JournalStore as _, MetadataFold,
    OutboxRow, StoreError,
};
use oml_app::types::{
    AppError, AuthorizationRecord, AuthorizeOutcome, AuthorizeParams, BootstrapRequest,
    BootstrapResult, ClaimEdgePage, ClaimEdgeQuery, ClaimEdgeRecord, ClientAuthentication,
    ClientKind, CodeEntryOutcome, ConfirmAction, ConfirmOutcome, ConfirmationView, ConnectGrant,
    ConnectOutcome, ConnectionRecord, Credential, CrewCreateRequest, CrewUpdate, ExchangeForm,
    GrantRequest, IssueKeyRequest, IssuedKey, KeyRecord, ListRequest, MintedToken, NamingRefusal,
    ProposeClaimEdge, RecallRequest, RecallResult, RefreshForm, SubmitRequest, SubmitVerb,
    SupersedeRequest, TokenPair, TransitionState, TransitionView, Verdict, VerdictRequest,
    VersionRef,
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
        covers: Covers::Subtree,
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
        connect_code: false,
        user: None,
        guardrail_scope: None,
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
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workarea: None,
        derives_from: Vec::new(),
        severity: None,
        rule: None,
    }
}

fn submit_into(idem: &str, body: &str, workarea_id: Uuid) -> SubmitRequest {
    SubmitRequest {
        workarea: Some(workarea_id.into()),
        ..submit_req(idem, body)
    }
}

fn recall_req(query: &str) -> RecallRequest {
    RecallRequest {
        query: query.to_owned(),
        k: 10,
        workarea: None,
        kinds: Vec::new(),
        include_text: true,
        include_unreviewed: false,
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
        workarea: None,
        refuse_vocabulary_target: false,
        severity: None,
        rule: None,
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
        claim_key: Some("fleet-order".to_owned()),
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
        claim_key: Some("fleet-order".to_owned()),
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
                    workarea: Some(node.into()),
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
                workarea: Some(quarantine.into()),
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
                covers: Covers::Subtree,
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
        claim_key: Some(format!("derived-{idem}")),
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
            claim_key: Some("fleet-order".to_owned()),
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
        workarea: Some(scope.into()),
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
        covers: Covers::Subtree,
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
            covers: Covers::Subtree,
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
                        covers: Covers::Subtree,
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

/// `key.issued` journals the key's credential, so a re-fold or a follower
/// rebuilds `api_keys.credential` from the journal alone (event-catalog.md
/// `key.issued`; ADR-0031 d7, d12).
#[tokio::test]
async fn key_issued_journals_the_keys_credential() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let issued = issue_with(
        &app,
        &boot.admin_key.value,
        Role::Writer,
        "crew/agent-credential",
        vec![grant(crew, Role::Reader)],
    )
    .await;
    let tail = journal(&store, boot.tenant_id).await;
    let key_issued = &tail[usize::try_from(issued.access.issued_seq - 1).expect("seq")].envelope;
    assert_eq!(key_issued.event_type, event_types::KEY_ISSUED);
    assert_eq!(key_issued.payload["credential"], "bearer");
    let admin_issued = tail
        .iter()
        .map(|e| &e.envelope)
        .find(|e| {
            e.event_type == event_types::KEY_ISSUED
                && e.payload["key_id"] == boot.admin_key.key_id.to_string()
        })
        .expect("the admin key's key.issued");
    assert_eq!(admin_issued.payload["credential"], "bearer");
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
                covers: Covers::Subtree,
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
            "credential": "bearer",
            // A tenant key names no user (ADR-0044 d2).
            "user": null,
            // Sent none, the issuance default (proposed ADR-0037 d4).
            "guardrail_scope": "ancestors",
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
                "covers": "subtree",
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

/// A key whose issuance failed after its `key.issued` keeps its name, so the
/// key read answers it by that name as history, with the `at` of the
/// `key.revoked` that followed as its revocation time (roles-and-keys.md
/// issuance order; ADR-0026 d10).
#[tokio::test]
async fn a_key_whose_issuance_failed_part_way_reads_back_by_name_revoked_at_its_key_revoked() {
    let store = MemStore::new().refusing_next_commit_of(event_types::GRANT_ISSUED);
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let req = issue_req(
        Role::Writer,
        "crew/agent-7",
        vec![grant(crew, Role::Writer)],
    );
    app.issue_key(admin, &req)
        .await
        .expect_err("the grant's commit is refused");
    let tail = journal(&store, boot.tenant_id).await;
    let revocation = tail.last().expect("a chain");
    assert_eq!(revocation.envelope.event_type, event_types::KEY_REVOKED);

    let detail = app
        .key(admin, Reference::Name(req.name.clone()))
        .await
        .expect("a revoked key's name still resolves");
    assert_eq!(detail.key.name, Some(req.name.clone()));
    assert_eq!(
        detail.key.revoked_at,
        Some(revocation.envelope.at.clone()),
        "the key.revoked's at, not the key.issued's"
    );
    assert_ne!(revocation.envelope.at, tail[tail.len() - 2].envelope.at);
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
            covers: Covers::Subtree,
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
            .map(|c| (c.workarea_id, c.name.as_str(), c.status.as_str()))
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

/// The status is one stored column: `workarea.create` stores `development`
/// on a new node and the tenant's bootstrap stores `committed` on the
/// universe root, the read serves what the row stores, and each create
/// event's payload spells the status it stored (ADR-0009 d1).
#[tokio::test]
async fn a_created_workarea_is_development_and_the_universe_root_is_committed_as_stored() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let t = boot.tenant_id;

    let root = store
        .workarea(t, boot.root_workarea_id)
        .await
        .expect("read")
        .expect("the root");
    assert_eq!(root.status, WorkareaStatus::Committed);
    let created = store
        .workarea(t, crew)
        .await
        .expect("read")
        .expect("the crew node");
    assert_eq!(created.status, WorkareaStatus::Development);

    let stored: Vec<(String, String)> = journal(&store, t)
        .await
        .iter()
        .filter(|e| e.envelope.event_type == event_types::WORKAREA_CREATE)
        .map(|e| {
            (
                e.envelope.payload["workarea_id"]
                    .as_str()
                    .expect("id")
                    .to_owned(),
                e.envelope.payload["status"]
                    .as_str()
                    .expect("status")
                    .to_owned(),
            )
        })
        .collect();
    assert_eq!(
        stored,
        vec![
            (boot.root_workarea_id.to_string(), "committed".to_owned()),
            (crew.to_string(), "development".to_owned()),
        ]
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
    assert_eq!(root.workarea.status, WorkareaStatus::Committed);

    let leaf = app
        .workarea_info(&reader.value, Some(quarter.into()))
        .await
        .expect("the leaf");
    assert_eq!(leaf.path, "universe.crew.q1-2027");
    assert_eq!(leaf.workarea.parent_id, Some(crew));
    assert_eq!(leaf.workarea.status, WorkareaStatus::Development);
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
        app.crews(admin, None, 50).await.expect("list").items,
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
    let err = app.crews(&writer.value, None, 50).await.unwrap_err();
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
                workarea: Some(boot.root_workarea_id.into()),
                ..supersede_req("sup-1", first, "revised")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::WorkareaNotTarget { .. }), "{err:?}");
    app.supersede(
        &writer.value,
        &SupersedeRequest {
            workarea: Some(crew.into()),
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
        include_unreviewed: true,
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
                workarea: Some(quotes.into()),
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
                    workarea: Some(scope.into()),
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
        include_text: false,
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
            workarea: Some(crew.into()),
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
            include_text: false,
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
        workarea: Some(target),
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
                workarea: Some(metals.into()),
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
                    workarea: Some(target),
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

/// The ceremony's request, under the tenant name `acme`.
fn acme() -> BootstrapRequest {
    BootstrapRequest {
        tenant_name: name("acme"),
        admin_key_name: name("root-admin"),
        admin_principal: "alice-admin".to_owned(),
    }
}

/// Tenant creation asks the store for the tenant's partitions before it
/// seals `tenant.genesis`; a creation that fails fails the act before any
/// commit: no tenant row, no event, and the name stays free, so a retry,
/// which mints a fresh id, takes it (postgres-schema.md, the per-tenant
/// partitions; ADR-0034 d11, d10 step 4).
#[tokio::test]
async fn a_failed_partition_creation_commits_no_tenant_and_journals_nothing() {
    for refusal in [
        StoreError::PartitionLockTimeout,
        StoreError::Partition("the double refused this partition on request".to_owned()),
    ] {
        let store = MemStore::new();
        store.refuse_next_partition(refusal.clone());
        let app = app_over(store.clone());
        let err = app
            .bootstrap_tenant(&acme())
            .await
            .expect_err("the partition was refused");
        assert!(
            matches!(&err, AppError::Store(e) if *e == refusal),
            "{err:?}"
        );
        assert_eq!(store.partitions(), Vec::<Uuid>::new(), "no partition kept");
        assert_eq!(
            store.tenant_count(),
            0,
            "no tenant row, so no event under any tenant"
        );

        let boot = app
            .bootstrap_tenant(&acme())
            .await
            .expect("the retry takes the name the refused act never committed");
        assert_eq!(store.partitions(), vec![boot.tenant_id]);
        assert_eq!(journal(&store, boot.tenant_id).await.len(), 3);
    }
}

/// A partition whose DDL committed is kept when the genesis commit then
/// fails: the act fails, journals nothing, and the retry mints a fresh id
/// with a partition of its own (ADR-0034 d11).
#[tokio::test]
async fn a_partition_left_by_a_failed_genesis_commit_is_kept_and_the_retry_mints_a_fresh_id() {
    let store = MemStore::new().refusing_next_commit_of(event_types::TENANT_GENESIS);
    let app = app_over(store.clone());
    app.bootstrap_tenant(&acme())
        .await
        .expect_err("the genesis commit was refused");
    let orphan = store.partitions();
    assert_eq!(orphan.len(), 1, "the committed partition is kept");
    assert_eq!(store.tenant_count(), 0, "no tenant row committed");
    assert_eq!(
        store.chain_head(orphan[0]).await.expect("head"),
        None,
        "nothing journalled under the orphan's id"
    );

    let boot = app.bootstrap_tenant(&acme()).await.expect("the retry");
    assert_ne!(boot.tenant_id, orphan[0], "the retry mints a fresh id");
    assert_eq!(store.partitions(), vec![orphan[0], boot.tenant_id]);
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

/// The naming of rows that predate names, which the lead runs at its first
/// start on the names schema, names every such row under rules 1–4 and
/// journals the result as one `system.names_assigned` on the chain, whose
/// fold alone gives those rows their names; every earlier event stays as it
/// was signed, and a second call finds nothing to name and journals nothing
/// (http-api.md rows created before names; ADR-0026 d11).
#[tokio::test]
async fn naming_rows_that_predate_names_journals_one_event_per_chain_and_a_second_call_journals_nothing()
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
        .expect("the first call names the chain");
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

    // A second call finds no row without a name, and journals nothing.
    let again_assigned = app
        .name_rows_that_predate_names(&[tenant])
        .await
        .expect("the second call");
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
/// role, with no grant: every node of the subtree in path order, a node
/// before its children and siblings by name, each with its derived path; a
/// reader key reads only what its grants reach (roles-and-keys.md
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
            app.workarea_tree(&key, node, None, 50).await.map(|page| {
                assert_eq!(page.next_cursor, None, "one page holds the tree");
                page.items
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
            (review, "universe.metals.q1-2027.lit-review".to_owned()),
            (q2, "universe.metals.q2-2027".to_owned()),
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
                workarea: Some(by_name("crew")),
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
                    workarea: scope,
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
/// on an admin key is refused, `request.validation` naming `key`, and
/// journals nothing: no structure read would honour it (http-api.md keys,
/// grants, denials; ADR-0026 d15).
#[tokio::test]
async fn a_global_denial_on_an_admin_key_is_refused_naming_key() {
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

/// `whoami` answers the calling key's own record and nothing of any other
/// key: its id, name, role, principal and attestation, its tenant, its
/// default workarea with the derived path and its crew record, each as an
/// id and a name, and its stored grants. It reads no claim, so it journals
/// nothing, and a value that names no live key is refused as every request
/// is (ADR-0039 d15).
#[tokio::test]
async fn whoami_returns_the_calling_keys_own_record_and_no_other() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let metals = metals_crew(&app, admin).await;
    let quarter = workarea(&app, admin, "q1-2027", crew).await;
    let agent = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                name: name("claude-laptop"),
                crew: Some(by_name("metals-crew")),
                default_workarea: Some(by_name("q1-2027")),
                ..issue_req(
                    Role::Writer,
                    "claude/laptop",
                    vec![grant(crew, Role::Writer), grant(crew, Role::Reader)],
                )
            },
        )
        .await
        .expect("issue the agent key");
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    let head = journal(&store, boot.tenant_id).await.len();

    let me = app.whoami(&agent.value).await.expect("whoami");
    assert_eq!(me.key.key_id, agent.key_id);
    assert_eq!(me.key.name, Some(name("claude-laptop")));
    assert_eq!(me.key.role, Role::Writer);
    assert_eq!(me.key.principal, "claude/laptop");
    assert_eq!(me.key.held_by, HeldBy::Agent);
    let tenant = me.tenant.expect("a tenant key names its tenant");
    assert_eq!((tenant.id, tenant.name), (boot.tenant_id, name("acme")));
    let default = me.default_workarea.expect("the journalled default");
    assert_eq!(
        (default.id, default.name, default.path.as_str()),
        (quarter, name("q1-2027"), "universe.crew.q1-2027")
    );
    let named_crew = me.crew.expect("the crew record named at issuance");
    assert_eq!(
        (named_crew.id, named_crew.name),
        (metals.crew_id, name("metals-crew"))
    );
    assert_eq!(
        me.grants
            .iter()
            .map(|g| (g.grant.node, g.grant.role))
            .collect::<Vec<_>>(),
        vec![(crew, Role::Writer), (crew, Role::Reader)]
    );

    // Another key answers its own record, never the first key's.
    let other = app.whoami(&reader.value).await.expect("whoami");
    assert_eq!(other.key.key_id, reader.key_id);
    assert_eq!(other.key.role, Role::Reader);
    assert_eq!(other.crew, None);
    assert_eq!(
        other.default_workarea.map(|w| (w.id, w.path)),
        Some((crew, "universe.crew".to_owned()))
    );
    assert_eq!(other.grants.len(), 1);
    // The admin key holds no grant and reads itself the same way.
    let root = app.whoami(admin).await.expect("whoami");
    assert_eq!(root.key.key_id, boot.admin_key.key_id);
    assert_eq!(root.key.role, Role::Admin);

    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
    let err = app.whoami("oml_not_a_real_key").await.unwrap_err();
    assert!(matches!(err, AppError::Unauthorized), "{err:?}");
}

#[tokio::test]
async fn a_submission_without_claim_key_is_held_under_the_key_derived_from_its_author_and_text() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let first = agent(&app, admin, "crew/agent-7", crew).await;
    let second = agent(&app, admin, "crew/agent-7", crew).await;
    let text = "the canary drains first";
    let derived = |idem: &str| SubmitRequest {
        claim_key: None,
        ..submit_req(idem, text)
    };

    let receipt = app
        .submit_memory(&first.value, &derived("d1"))
        .await
        .expect("submit")
        .receipt;
    assert_eq!(receipt.claim_key, derive_claim_key(first.key_id, text));
    // Another key's derived key differs, so its claim is a second claim and
    // no conflict tells it the first key's held text (ADR-0039 d6).
    let other = app
        .submit_memory(&second.value, &derived("d2"))
        .await
        .expect("a second claim")
        .receipt;
    assert_eq!(other.claim_key, derive_claim_key(second.key_id, text));
    assert_ne!(other.claim_key, receipt.claim_key);
    let err = app
        .submit_memory(&first.value, &derived("d3"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ClaimKeyConflict), "{err:?}");
    let events = journal(&store, boot.tenant_id).await;
    let capture = &events
        .iter()
        .find(|e| e.envelope.payload["artifact_id"] == receipt.memory_id.to_string())
        .expect("the capture")
        .envelope;
    assert_eq!(capture.event_type, event_types::SUBMISSION_REMEMBER);
    assert_eq!(capture.payload["claim_key"], receipt.claim_key);
}

#[tokio::test]
async fn an_explicit_claim_key_starting_auto_is_refused_naming_claim_key() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let writer = agent(&app, &boot.admin_key.value, "crew/agent-7", crew).await;
    let err = app
        .submit_memory(
            &writer.value,
            &SubmitRequest {
                claim_key: Some("auto-0123456789abcdef".to_owned()),
                ..submit_req("a1", "a fact")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::DerivedClaimKeySent), "{err:?}");
}

#[tokio::test]
async fn a_submission_with_derives_from_and_no_verb_journals_learn_and_an_explicit_verb_keeps_its_event()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let writer = agent(&app, &boot.admin_key.value, "crew/agent-7", crew).await;
    let ancestor = app
        .submit_memory(&writer.value, &submit_req("a0", "an ancestor"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    for (idem, verb, expected) in [
        ("v1", None, event_types::SUBMISSION_LEARN),
        (
            "v2",
            Some(SubmitVerb::Remember),
            event_types::SUBMISSION_REMEMBER,
        ),
        ("v3", Some(SubmitVerb::Learn), event_types::SUBMISSION_LEARN),
    ] {
        app.submit_memory(
            &writer.value,
            &SubmitRequest {
                verb,
                derives_from: vec![ancestor],
                ..submit_req(idem, &format!("derived {idem}"))
            },
        )
        .await
        .expect("submit");
        let events = journal(&store, boot.tenant_id).await;
        let capture = &events.last().expect("an event").envelope;
        assert_eq!(capture.event_type, expected, "{idem}");
    }
}

// ---------------------------------------------------------------------------
// The default serve: a verdict without serve_id (ADR-0039 d10)
// ---------------------------------------------------------------------------

/// A verdict sent without `serve_id`, under Idempotency-Key `idem`.
fn default_verdict(verdict: Verdict, reason: Option<&str>, idem: &str) -> VerdictRequest {
    VerdictRequest {
        serve: None,
        verdict,
        reason: reason.map(str::to_owned),
        idempotency_key: idem.to_owned(),
    }
}

#[tokio::test]
async fn the_default_serve_id_is_the_reviewers_own_claimed_item() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (writer, _reader, dana) = crew_keys(&app, admin, crew).await;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let idle = issue(&app, admin, Role::Dreamer, "ida", crew).await;
    let first = held(
        &app,
        &writer.value,
        &submit_req("d1", "the first held claim"),
    )
    .await;
    let second = held(
        &app,
        &writer.value,
        &submit_req("d2", "the second held claim"),
    )
    .await;

    // Each reviewer claims one item: dana the oldest, dora the next, since
    // a row another key claimed is skipped (admission.md review).
    let served = app
        .review_next(&dana.value)
        .await
        .expect("next")
        .expect("item");
    assert_eq!(served.memory.memory_id, first);
    let served = app
        .review_next(&dora.value)
        .await
        .expect("next")
        .expect("item");
    assert_eq!(served.memory.memory_id, second);

    // dora's default is dora's claim, never dana's older one.
    let outcome = app
        .review_verdict(&dora.value, &default_verdict(Verdict::Approve, None, "dv1"))
        .await
        .expect("verdict");
    assert_eq!(outcome.memory_id, second);
    assert_eq!(outcome.state, MemoryStatus::Promoted);
    let first_row = app
        .get_memory(&writer.value, first)
        .await
        .expect("own read");
    assert_eq!(first_row.memory.status, MemoryStatus::Held);

    // A key holding no claim is refused, naming serve_id, and nothing moves.
    let err = app
        .review_verdict(&idle.value, &default_verdict(Verdict::Approve, None, "dv2"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::ServeIdUnresolved { several: false }),
        "{err:?}"
    );
    let first_row = app
        .get_memory(&writer.value, first)
        .await
        .expect("own read");
    assert_eq!(first_row.memory.status, MemoryStatus::Held);

    // dana's default is dana's own claim.
    let outcome = app
        .review_verdict(
            &dana.value,
            &default_verdict(Verdict::Reject, Some("superseded by the second"), "dv3"),
        )
        .await
        .expect("verdict");
    assert_eq!(outcome.memory_id, first);
    assert_eq!(outcome.state, MemoryStatus::Rejected);
}

// ---------------------------------------------------------------------------
// Connector keys, connect codes and connections (ADR-0031 d1, d7, d8, d11)
// ---------------------------------------------------------------------------

/// The fixture deployment's `oauth.issuer`.
const ISSUER: &str = "https://oml.example.com";

/// An app over `store` whose deployment sets `oauth.issuer` (ADR-0031 d1).
fn oauth_app_over(store: MemStore) -> TestApp {
    app_over(store).with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"))
}

/// A writer connector key's issuance: `connect_code: true` (ADR-0031 d7).
fn connector_req(principal: &str, grants: Vec<GrantRequest>) -> IssueKeyRequest {
    IssueKeyRequest {
        connect_code: true,
        ..issue_req(Role::Writer, principal, grants)
    }
}

/// The `details.reason` of an `oauth.connect_refused`, else `None`.
fn connect_refused(err: &AppError) -> Option<ConnectRefused> {
    match err {
        AppError::OAuth(Refusal::ConnectRefused(reason)) => Some(*reason),
        _ => None,
    }
}

fn at(value: &str) -> EventTime {
    EventTime::parse(value).expect("a fixed-width fixture time")
}

static FIXTURE_EVENT: AtomicU64 = AtomicU64::new(0);

/// Seals one event on `tenant`'s head with the test key and commits it with
/// `fold`, as the lead would: for the acts this suite drives beneath the use
/// cases (an operator key, a key's revocation, a token exchange).
async fn seal_onto(
    store: &MemStore,
    tenant: Uuid,
    event_type: &str,
    when: &str,
    payload: Value,
    fold: MetadataFold,
) -> u64 {
    let events = journal(store, tenant).await;
    let draft = EventDraft {
        tenant_id: tenant,
        event_id: Uuid::from_u128(
            0xF1C5_0000 + u128::from(FIXTURE_EVENT.fetch_add(1, Ordering::Relaxed)),
        ),
        event_type: event_type.to_owned(),
        principal: "fixture".to_owned(),
        api_key_id: None,
        at: at(when),
        payload,
    };
    let envelope = next_envelope(events.last(), draft).expect("a lawful draft");
    let event = sign(envelope, &SigningKey::from_bytes(&TEST_SEED)).expect("signing");
    let seq = event.envelope.seq;
    store
        .commit(CommitRequest {
            outbox: OutboxRow {
                event_id: event.envelope.event_id,
                topic: event_type.to_owned(),
                payload: json!({ "seq": seq }),
            },
            event,
            fold,
            idempotency: None,
        })
        .await
        .expect("the fixture commit");
    seq
}

/// An operator key in `tenant`, which no use case issues there: its value.
async fn operator_key(store: &MemStore, tenant: Uuid) -> String {
    let value = "oml_operator-fixture-value".to_owned();
    let key_id = Uuid::from_u128(0x0BE7_0001);
    let record = KeyRecord {
        key_id,
        tenant_id: tenant,
        role: Role::Operator,
        principal: "operator".to_owned(),
        held_by: HeldBy::Human,
        default_workarea: None,
        issued_by: None,
        credential: Credential::Bearer,
        key_hash: Some(sha256_digest_string(value.as_bytes())),
        name: Some(name("operator-fixture")),
        crew_id: None,
        revoked_at: None,
        user: None,
        session_user: None,
        guardrail_scope: GuardrailScope::Ancestors,
    };
    seal_onto(
        store,
        tenant,
        event_types::KEY_ISSUED,
        "2026-10-29T12:00:30.000000Z",
        json!({ "key_id": key_id.to_string(), "role": "operator" }),
        MetadataFold::KeyIssued(record),
    )
    .await;
    value
}

/// The fixture authorization code's digest for `connection`.
fn auth_code(connection: Uuid) -> Sha256Hash {
    Sha256Hash::digest(&[b"auth-code:".as_slice(), connection.as_bytes()].concat())
}

/// Enters the key's code on the authorization page and confirms Connect,
/// on the store, as the page does: `connection` is created `authorized`
/// (roles-and-keys.md connections lifecycle; ADR-0031 d9).
async fn authorize(store: &MemStore, tenant: Uuid, key_id: Uuid, code_id: Uuid, connection: Uuid) {
    assert!(
        store
            .spend_code(tenant, code_id, &at("2026-10-29T12:05:00.000000Z"))
            .await
            .expect("code entry"),
        "the code's first correct entry spends it"
    );
    let handle = Sha256Hash::digest(&[b"handle:".as_slice(), connection.as_bytes()].concat());
    store
        .authorization_insert(AuthorizationRecord {
            authorization_id: Uuid::from_u128(connection.as_u128() ^ 0xA000_0000),
            handle_hash: handle,
            client_id: "https://claude.ai/oauth/mcp-oauth-client-metadata".to_owned(),
            client_kind: ClientKind::Cimd,
            metadata_digest: Sha256Hash::digest(b"client metadata"),
            redirect_uri: "https://claude.ai/api/mcp/auth_callback".to_owned(),
            redirect_uri_sent: true,
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_owned(),
            state: None,
            expires_at: at("2026-10-29T12:09:00.000000Z"),
            code_id,
            code_tenant_id: tenant,
            key_id,
        })
        .await
        .expect("the authorization request");
    let outcome = store
        .connection_connect(
            &handle,
            ConnectGrant {
                connection_id: connection,
                resource: format!("{ISSUER}/mcp"),
                auth_code_hash: auth_code(connection),
                auth_code_expires_at: at("2026-10-29T12:07:00.000000Z"),
                at: at("2026-10-29T12:05:30.000000Z"),
            },
        )
        .await
        .expect("Connect");
    assert!(
        matches!(outcome, ConnectOutcome::Authorized { .. }),
        "{outcome:?}"
    );
}

/// The token exchange's `connection.opened` for an `authorized`
/// connection, replacing none (event-catalog.md `connection.opened`).
async fn open(store: &MemStore, tenant: Uuid, connection: Uuid) {
    let token = |kind: &[u8]| MintedToken {
        token_hash: Sha256Hash::digest(&[kind, connection.as_bytes()].concat()),
        expires_at: at("2026-11-28T12:06:00.000000Z"),
    };
    seal_onto(
        store,
        tenant,
        event_types::CONNECTION_OPENED,
        "2026-10-29T12:06:00.000000Z",
        json!({ "connection_id": connection.to_string(), "replaces_connection_id": Value::Null }),
        MetadataFold::ConnectionOpened {
            connection_id: connection,
            replaces: None,
            auth_code_hash: auth_code(connection),
            tokens: TokenPair {
                access: token(b"access:"),
                refresh: token(b"refresh:"),
            },
        },
    )
    .await;
}

/// `POST /v2/keys` with `connect_code: true` mints no value: the key's
/// credential is `connection` and it has no hash, `key.issued`, every
/// `grant.issued` and `connection.code_issued` commit in that order, and the
/// response carries the code, its id, its expiry ten minutes on and the
/// connector URL `<issuer>/mcp` in the value's place (roles-and-keys.md
/// issuance order, step 7; event-catalog.md `connection.code_issued`;
/// ADR-0031 d7, d8).
#[tokio::test]
async fn a_connector_key_is_issued_with_a_connect_code_and_no_value() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key;
    let head = journal(&store, boot.tenant_id).await.len();

    let issued = app
        .issue_key(
            &admin.value,
            &connector_req(
                "crew/claude-mobile",
                vec![grant(crew, Role::Writer), grant(crew, Role::Reader)],
            ),
        )
        .await
        .expect("a connector key");

    assert!(issued.value.is_empty(), "a connector key has no value");
    let code = issued.connect_code.as_ref().expect("the connect code");
    assert_eq!(code.connector_url, format!("{ISSUER}/mcp"));
    let symbols = code
        .code
        .as_deref()
        .expect("a fresh answer carries the code");
    let parsed = ConnectCode::parse(symbols).expect("twelve Crockford symbols");
    assert_eq!(parsed.grouped(), symbols, "shown as three groups of four");

    let events = journal(&store, boot.tenant_id).await;
    let types: Vec<&str> = events[head..]
        .iter()
        .map(|e| e.envelope.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        [
            event_types::KEY_ISSUED,
            event_types::GRANT_ISSUED,
            event_types::GRANT_ISSUED,
            event_types::CONNECTION_CODE_ISSUED,
        ]
    );
    let key_issued = &events[head].envelope;
    assert_eq!(key_issued.payload["credential"], "connection");
    assert_eq!(issued.access.issued_seq, key_issued.seq);
    let grant_seqs: Vec<u64> = issued.access.grants.iter().map(|g| g.epoch).collect();
    assert_eq!(
        grant_seqs,
        [events[head + 1].envelope.seq, events[head + 2].envelope.seq]
    );
    let code_issued = &events[head + 3].envelope;
    assert_eq!(
        code_issued.payload,
        json!({
            "code_id": code.code_id.to_string(),
            "key_id": issued.key_id.to_string(),
            "issued_by_key_id": admin.key_id.to_string(),
            "expires_at": code.expires_at.as_str(),
            "voids_code_id": Value::Null,
        })
    );
    let ttl = code.expires_at.unix_micros() - code_issued.at.unix_micros();
    assert!(
        (599_000_000..=600_000_000).contains(&ttl),
        "the code lives oauth.connect_code_ttl's shipped ten minutes: {ttl} µs"
    );

    let key = store
        .key(boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("the key's row");
    assert_eq!(key.credential, Credential::Connection);
    assert_eq!(key.key_hash, None);
    let codes = store
        .connect_codes(boot.tenant_id, issued.key_id)
        .await
        .expect("codes");
    assert_eq!(codes.len(), 1);
    assert_eq!(codes[0].code_id, code.code_id);
    assert_eq!(codes[0].code_hash, Sha256Hash(parsed.digest()));
    assert_eq!(codes[0].issued_seq, code_issued.seq);
    assert_eq!(codes[0].expires_at, code.expires_at);
}

/// Step 7 refuses a connect code for a key role of `admin` or `operator`
/// with `oauth.connect_refused` and `details.reason: role`, journalling
/// nothing (roles-and-keys.md issuance order, step 7; ADR-0031 d7).
#[tokio::test]
async fn an_admin_or_operator_role_is_refused_a_connect_code_by_its_reason() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, _) = tenant(&app).await;
    let operator = operator_key(&store, boot.tenant_id).await;
    let head = journal(&store, boot.tenant_id).await.len();

    let err = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                connect_code: true,
                ..issue_req(Role::Admin, "second-admin", Vec::new())
            },
        )
        .await
        .unwrap_err();
    assert_eq!(connect_refused(&err), Some(ConnectRefused::Role), "{err:?}");
    let err = app
        .issue_key(
            &operator,
            &IssueKeyRequest {
                connect_code: true,
                ..issue_req(Role::Operator, "second-operator", Vec::new())
            },
        )
        .await
        .unwrap_err();
    assert_eq!(connect_refused(&err), Some(ConnectRefused::Role), "{err:?}");
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// Step 7 refuses a connect code for a key attested `held_by: service`
/// with `held_by_service`, journalling nothing (ADR-0031 d7).
#[tokio::test]
async fn a_service_held_key_is_refused_a_connect_code() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let head = journal(&store, boot.tenant_id).await.len();
    let err = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                held_by: Some(HeldBy::Service),
                ..connector_req("reconciler", vec![grant(crew, Role::Writer)])
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        connect_refused(&err),
        Some(ConnectRefused::HeldByService),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// A deployment that has not named its public origin cannot advertise
/// one: step 7 refuses a connector key with `not_configured`, journalling
/// nothing, while a bearer key issues as before (ADR-0031 d1, d7).
#[tokio::test]
async fn a_connector_key_is_refused_when_oauth_issuer_is_unset() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let head = journal(&store, boot.tenant_id).await.len();
    let err = app
        .issue_key(
            &boot.admin_key.value,
            &connector_req("crew/claude-mobile", vec![grant(crew, Role::Writer)]),
        )
        .await
        .unwrap_err();
    assert_eq!(
        connect_refused(&err),
        Some(ConnectRefused::NotConfigured),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
    let bearer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda", crew).await;
    assert!(bearer.value.starts_with("oml_"));
    assert!(bearer.connect_code.is_none());
}

/// The connect-code step is the seventh, after the default workarea and
/// before the name, which ADR-0026 d5 checks after every other check: a
/// taken name with a refused connect code answers the connect code's
/// refusal, an uncovered default answers before step 7, and only a key that
/// can hold a connection reaches `name.taken`, which journals nothing
/// (roles-and-keys.md issuance order, steps 6–8; ADR-0031 d7).
#[tokio::test]
async fn the_name_check_runs_after_the_connect_code_step() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let other = workarea(&app, admin, "other", boot.root_workarea_id).await;
    app.issue_key(
        admin,
        &IssueKeyRequest {
            name: name("claude-mobile"),
            ..issue_req(Role::Writer, "wanda", vec![grant(crew, Role::Writer)])
        },
    )
    .await
    .expect("the name's first key");
    let head = journal(&store, boot.tenant_id).await.len();
    let taken = |req: IssueKeyRequest| IssueKeyRequest {
        name: name("claude-mobile"),
        ..req
    };

    let err = app
        .issue_key(
            admin,
            &taken(IssueKeyRequest {
                held_by: Some(HeldBy::Service),
                ..connector_req("p", vec![grant(crew, Role::Writer)])
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(
        connect_refused(&err),
        Some(ConnectRefused::HeldByService),
        "{err:?}"
    );
    let unset = app_over(store.clone());
    let err = unset
        .issue_key(
            admin,
            &taken(connector_req("p", vec![grant(crew, Role::Writer)])),
        )
        .await
        .unwrap_err();
    assert_eq!(
        connect_refused(&err),
        Some(ConnectRefused::NotConfigured),
        "{err:?}"
    );
    let err = app
        .issue_key(
            admin,
            &taken(IssueKeyRequest {
                held_by: Some(HeldBy::Service),
                default_workarea: Some(other.into()),
                ..connector_req("p", vec![grant(crew, Role::Writer)])
            }),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::DefaultUncovered { .. }),
        "step 6 answers before step 7: {err:?}"
    );
    let err = app
        .issue_key(
            admin,
            &taken(connector_req("p", vec![grant(crew, Role::Writer)])),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AppError::NameTaken { kind: NamedKind::Key, name } if name.as_str() == "claude-mobile"),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// A key is a bearer key or a connector key, never both: a connect code for
/// a key that has a value answers `oauth.connect_refused` (`bearer_key`)
/// and journals nothing (ADR-0031 d7, d8).
#[tokio::test]
async fn minting_a_code_for_a_bearer_key_is_refused() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let bearer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let head = journal(&store, boot.tenant_id).await.len();
    let err = app
        .mint_connect_code(admin, bearer.key_id.into())
        .await
        .unwrap_err();
    assert_eq!(
        connect_refused(&err),
        Some(ConnectRefused::BearerKey),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// A revoked connector key is refused a code with `revoked`, and a
/// deployment without `oauth.issuer` with `not_configured`, each
/// journalling nothing (http-api.md `POST /v2/keys/{key_id}/connect-codes`;
/// ADR-0031 d1, d8).
#[tokio::test]
async fn minting_a_code_for_a_revoked_key_or_without_oauth_issuer_is_refused() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let live = app
        .issue_key(
            admin,
            &connector_req("crew/a", vec![grant(crew, Role::Writer)]),
        )
        .await
        .expect("a connector key");
    let revoked = app
        .issue_key(
            admin,
            &connector_req("crew/b", vec![grant(crew, Role::Writer)]),
        )
        .await
        .expect("a connector key");
    seal_onto(
        &store,
        boot.tenant_id,
        event_types::KEY_REVOKED,
        "2026-10-29T12:01:00.000000Z",
        json!({ "key_id": revoked.key_id.to_string() }),
        MetadataFold::KeyRevoked {
            key_id: revoked.key_id,
        },
    )
    .await;
    let head = journal(&store, boot.tenant_id).await.len();

    let err = app
        .mint_connect_code(admin, revoked.key_id.into())
        .await
        .unwrap_err();
    assert_eq!(
        connect_refused(&err),
        Some(ConnectRefused::Revoked),
        "{err:?}"
    );
    let err = app_over(store.clone())
        .mint_connect_code(admin, live.key_id.into())
        .await
        .unwrap_err();
    assert_eq!(
        connect_refused(&err),
        Some(ConnectRefused::NotConfigured),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
}

/// A key has at most one live code: minting one names the key's unredeemed
/// code in `voids_code_id` and voids it in the same commit, so the first
/// code's `voided_seq` is the second's `issued_seq` (event-catalog.md
/// `connection.code_issued`; ADR-0031 d8).
#[tokio::test]
async fn minting_a_second_code_voids_the_first() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let issued = app
        .issue_key(
            admin,
            &connector_req("crew/claude-mobile", vec![grant(crew, Role::Writer)]),
        )
        .await
        .expect("a connector key");
    let first = issued.connect_code.expect("the first code");

    let second = app
        .mint_connect_code(admin, by_name(issued.name.as_str()))
        .await
        .expect("a second code");
    assert_ne!(second.code_id, first.code_id);
    assert_ne!(second.code, first.code);
    assert_eq!(second.connector_url, format!("{ISSUER}/mcp"));

    let events = journal(&store, boot.tenant_id).await;
    let minted = &events.last().expect("the mint").envelope;
    assert_eq!(minted.event_type, event_types::CONNECTION_CODE_ISSUED);
    assert_eq!(
        minted.payload["voids_code_id"],
        json!(first.code_id.to_string())
    );
    let codes = store
        .connect_codes(boot.tenant_id, issued.key_id)
        .await
        .expect("codes");
    assert_eq!(codes.len(), 2);
    assert_eq!(codes[0].code_id, first.code_id);
    assert_eq!(codes[0].voided_seq, Some(minted.seq));
    assert_eq!(codes[1].code_id, second.code_id);
    assert_eq!(codes[1].voided_seq, None);
}

/// An `authorized` connection is not journalled, not listed and cannot be
/// named: revoking it answers `access.not_found`, as an unknown id does,
/// and journals nothing (http-api.md
/// `POST /v2/keys/{key_id}/connections/{connection_id}/revoke`; ADR-0031
/// d11).
#[tokio::test]
async fn revoking_an_authorized_connection_answers_not_found() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let issued = app
        .issue_key(
            admin,
            &connector_req("crew/claude-mobile", vec![grant(crew, Role::Writer)]),
        )
        .await
        .expect("a connector key");
    let code = issued.connect_code.expect("the code");
    let connection = Uuid::from_u128(0xC0_0001);
    authorize(
        &store,
        boot.tenant_id,
        issued.key_id,
        code.code_id,
        connection,
    )
    .await;
    let head = journal(&store, boot.tenant_id).await.len();

    for named in [connection, Uuid::from_u128(0xC0_DEAD)] {
        let err = app
            .revoke_connection(admin, issued.key_id.into(), named, None)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    assert!(
        app.list_connections(admin, issued.key_id.into())
            .await
            .expect("the listing")
            .is_empty(),
        "an authorized connection is not listed"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), head);
    let row = store
        .connection(boot.tenant_id, connection)
        .await
        .expect("read")
        .expect("the row");
    assert_eq!(row.state, ConnectionState::Authorized);
}

/// Listing a key's connections and revoking one are served whether or not
/// `oauth.issuer` is set, so an admin can clean up while the surface is
/// off: the open connection is listed, its revocation journals
/// `connection.revoked` with the cause `admin` and the digest of the reason
/// given, and the listing then still holds the connection as revoked by an
/// admin at that event's seq: a key's open and revoked connections are
/// listed, an `authorized` one is not (ADR-0031 d1, d11, d12).
#[tokio::test]
async fn connections_are_listed_and_revoked_with_oauth_issuer_unset() {
    let store = MemStore::new();
    let (boot, crew) = tenant(&oauth_app_over(store.clone())).await;
    let admin = &boot.admin_key;
    let issued = oauth_app_over(store.clone())
        .issue_key(
            &admin.value,
            &connector_req("crew/claude-mobile", vec![grant(crew, Role::Writer)]),
        )
        .await
        .expect("a connector key");
    let code = issued.connect_code.expect("the code");
    let connection = Uuid::from_u128(0xC0_0002);
    authorize(
        &store,
        boot.tenant_id,
        issued.key_id,
        code.code_id,
        connection,
    )
    .await;
    open(&store, boot.tenant_id, connection).await;

    let app = app_over(store.clone());
    let listed = app
        .list_connections(&admin.value, by_name(issued.name.as_str()))
        .await
        .expect("the listing");
    let ids: Vec<Uuid> = listed.iter().map(|c| c.connection_id).collect();
    assert_eq!(ids, [connection]);
    assert_eq!(listed[0].state, ConnectionState::Open);

    let revoked = app
        .revoke_connection(
            &admin.value,
            issued.key_id.into(),
            connection,
            Some("lost phone"),
        )
        .await
        .expect("the revocation");
    assert_eq!(revoked.state, ConnectionState::Revoked(RevokedCause::Admin));
    let events = journal(&store, boot.tenant_id).await;
    let event = &events.last().expect("the revocation").envelope;
    assert_eq!(event.event_type, event_types::CONNECTION_REVOKED);
    assert_eq!(revoked.revoked_seq, Some(event.seq));
    assert_eq!(event.api_key_id, Some(admin.key_id));
    assert_eq!(
        event.payload,
        json!({
            "connection_id": connection.to_string(),
            "key_id": issued.key_id.to_string(),
            "revoked_by_key_id": admin.key_id.to_string(),
            "cause": "admin",
            "reason_digest": sha256_digest_string(b"lost phone"),
        })
    );
    let listed = app
        .list_connections(&admin.value, issued.key_id.into())
        .await
        .expect("the listing");
    let ids: Vec<Uuid> = listed.iter().map(|c| c.connection_id).collect();
    assert_eq!(ids, [connection], "a revoked connection is still listed");
    assert_eq!(
        listed[0].state,
        ConnectionState::Revoked(RevokedCause::Admin)
    );
    assert_eq!(listed[0].revoked_seq, Some(event.seq));
    let err = app
        .revoke_connection(&admin.value, issued.key_id.into(), connection, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::NotFound),
        "a revoked connection is not open: {err:?}"
    );
}

/// The code appears in the issuance's answer and nowhere else: no payload
/// of the issuance's or a mint's events carries its symbols, its grouped
/// form or its digest in any spelling (event-catalog.md
/// `connection.code_issued`; ADR-0031 d8).
#[tokio::test]
async fn no_journal_payload_of_the_issuance_carries_the_code_or_its_digest() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let head = journal(&store, boot.tenant_id).await.len();
    let issued = app
        .issue_key(
            admin,
            &connector_req("crew/claude-mobile", vec![grant(crew, Role::Writer)]),
        )
        .await
        .expect("a connector key");
    let minted = app
        .mint_connect_code(admin, issued.key_id.into())
        .await
        .expect("a second code");
    let mut secrets = Vec::new();
    for shown in [issued.connect_code.expect("the code"), minted] {
        let grouped = shown.code.expect("a fresh answer carries the code");
        let code = ConnectCode::parse(&grouped).expect("a code");
        let digest = Sha256Hash(code.digest());
        let hex: String = code.digest().iter().map(|b| format!("{b:02x}")).collect();
        secrets.extend([
            grouped,
            code.symbols().to_owned(),
            digest.to_string(),
            hex.clone(),
            hex.to_uppercase(),
        ]);
    }
    for event in &journal(&store, boot.tenant_id).await[head..] {
        let payload = event.envelope.payload.to_string();
        for secret in &secrets {
            assert!(
                !payload.contains(secret.as_str()),
                "{} carries {secret}",
                event.envelope.event_type
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The authorization page: code entry, Connect and Cancel (ADR-0031 d8, d9)
// ---------------------------------------------------------------------------

/// The fixture client's metadata document URL.
const DOC_URL: &str = "https://client.example.com/oauth/metadata.json";

/// The fixture authorize request's `state`.
const STATE: &str = "opaque state";

/// The fixture client's document, served for an hour.
fn client_documents() -> MemDocuments {
    let docs = MemDocuments::new();
    let body = format!(
        r#"{{"client_id":"{DOC_URL}","client_name":"Claude","redirect_uris":["{CLAUDE_CALLBACK}"]}}"#
    );
    docs.serve(DOC_URL, Ok((body.into_bytes(), 3_600)));
    docs
}

/// A valid authorize request's sealed handle: the page that asks for the
/// code (ADR-0031 d9 step 1).
async fn code_page(app: &TestApp) -> String {
    let params = AuthorizeParams {
        client_id: Some(DOC_URL.to_owned()),
        redirect_uri: Some(CLAUDE_CALLBACK.to_owned()),
        response_type: Some("code".to_owned()),
        code_challenge: Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_owned()),
        code_challenge_method: Some("S256".to_owned()),
        state: Some(STATE.to_owned()),
        resource: Some(format!("{ISSUER}/mcp")),
        scope: Some("mcp".to_owned()),
        repeated: false,
    };
    match app
        .authorize_request(params, &client_documents(), &PlainSealer)
        .await
        .expect("an authorize request is answered")
    {
        AuthorizeOutcome::Page(page) => page.handle,
        other => panic!("a valid request is a page: {other:?}"),
    }
}

/// A writer connector key on `grants` and its connect code's symbols.
async fn connector_with_code(
    app: &TestApp,
    admin: &str,
    grants: Vec<GrantRequest>,
) -> (IssuedKey, String) {
    let issued = app
        .issue_key(admin, &connector_req("crew/claude-mobile", grants))
        .await
        .expect("a connector key");
    let code = issued
        .connect_code
        .as_ref()
        .and_then(|c| c.code.clone())
        .expect("a fresh answer carries the code");
    (issued, code)
}

/// The confirmation a correct code answers.
async fn entered(app: &TestApp, handle: &str, code: &str) -> ConfirmationView {
    match app
        .enter_code(handle, code, &PlainSealer)
        .await
        .expect("code entry is answered")
    {
        CodeEntryOutcome::Confirmation(view) => *view,
        other => panic!("a correct code shows the confirmation: {other:?}"),
    }
}

/// The attempts a sealed handle's request has left.
fn attempts_left(handle: &str) -> u8 {
    let now = at("2026-10-29T12:00:01.000000Z");
    open_request(PlainSealer.open(handle).as_deref(), &now)
        .expect("a live request")
        .attempts_left
}

/// The redirect a refusal of a known request answers: `access_denied` to
/// the fixture request's redirect URI with its `state` and `iss` (errors.md
/// `oauth.authorization_expired`; ADR-0031 d9).
fn denied_redirect(reason: AuthorizationExpired) -> ConfirmOutcome {
    ConfirmOutcome::Denied {
        error: Refusal::AuthorizationExpired(reason),
        redirect_uri: CLAUDE_CALLBACK.to_owned(),
        state: Some(STATE.to_owned()),
        iss: ISSUER.to_owned(),
    }
}

fn authorization_expired(err: &AppError) -> Option<AuthorizationExpired> {
    match err {
        AppError::OAuth(Refusal::AuthorizationExpired(reason)) => Some(*reason),
        _ => None,
    }
}

/// After a correct code the confirmation shows the tenant by its name, the
/// key's name, principal and role, every granted workarea by name with the
/// grant's role, the workareas each verb-kind sentence names, and no
/// self-signed workarea; the code is spent and the request's row is written
/// under the SHA-256 of the confirmation's handle (http-api.md
/// `POST /oauth/authorize`; ADR-0031 d8, d9 step 2).
#[tokio::test]
async fn a_correct_code_shows_the_keys_tenant_name_role_and_granted_workareas() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let notes = workarea(&app, admin, "notes", boot.root_workarea_id).await;
    let (issued, code) = connector_with_code(
        &app,
        admin,
        vec![grant(crew, Role::Writer), grant(notes, Role::Reader)],
    )
    .await;
    let handle = code_page(&app).await;

    let view = entered(&app, &handle, &code.to_lowercase()).await;

    assert_eq!(view.tenant_name, name("acme"));
    assert_eq!(view.key_name, Some(issued.name.clone()));
    assert_eq!(view.principal, "crew/claude-mobile");
    assert_eq!(view.role, Role::Writer);
    let grants: Vec<(Uuid, Name, Role)> = view
        .grants
        .iter()
        .map(|g| (g.workarea_id, g.workarea.clone(), g.role))
        .collect();
    assert_eq!(
        grants,
        [
            (crew, name("crew"), Role::Writer),
            (notes, name("notes"), Role::Reader)
        ]
    );
    assert_eq!(view.writes_in, [name("crew")]);
    assert_eq!(view.reads_in, [name("notes")]);
    assert!(view.proposes_in.is_empty());
    assert!(view.self_signed_in.is_empty());

    let codes = store
        .connect_codes(boot.tenant_id, issued.key_id)
        .await
        .expect("codes");
    assert!(codes[0].redeemed_at.is_some(), "the correct entry spent it");
    let row = store
        .authorization(&Sha256Hash::digest(view.handle.as_bytes()))
        .await
        .expect("read")
        .expect("the request's row, by the handle's digest");
    assert_eq!(row.code_id, codes[0].code_id);
    assert_eq!(row.code_tenant_id, boot.tenant_id);
    assert_eq!(row.key_id, issued.key_id);
    assert_eq!(row.client_id, DOC_URL);
    assert_eq!(row.client_kind, ClientKind::Cimd);
    assert_eq!(row.redirect_uri, CLAUDE_CALLBACK);
    assert_eq!(row.state.as_deref(), Some(STATE));
    assert_eq!(view.handle.len(), 43, "a 256-bit handle in base62");
    assert_eq!(app.connect_code_failures(), 0);
}

/// An unknown, malformed, expired, voided or spent code answers the one
/// refusal, `oauth.connect_code_invalid`, with one attempt fewer under a
/// new sealed handle, writes no request row and counts one deployment-wide
/// failure for each (http-api.md `POST /oauth/authorize`; ADR-0031 d8, d9
/// step 2, d14).
#[tokio::test]
async fn a_wrong_code_of_any_kind_answers_the_same_refusal_with_one_attempt_fewer() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    // Voided: a newer code replaced it.
    let (voided_key, voided) =
        connector_with_code(&app, admin, vec![grant(crew, Role::Writer)]).await;
    app.mint_connect_code(admin, voided_key.key_id.into())
        .await
        .expect("a newer code");
    // Spent: entered correctly on another page.
    let (_, spent) = connector_with_code(&app, admin, vec![grant(crew, Role::Writer)]).await;
    entered(&app, &code_page(&app).await, &spent).await;
    // Expired: a code whose ten minutes are over.
    let (expired_key, _) = connector_with_code(&app, admin, vec![grant(crew, Role::Writer)]).await;
    let expired = "EXPD-EXPD-EXPD";
    let live = store
        .connect_codes(boot.tenant_id, expired_key.key_id)
        .await
        .expect("codes")[0]
        .code_id;
    seal_onto(
        &store,
        boot.tenant_id,
        event_types::CONNECTION_CODE_ISSUED,
        "2026-10-29T11:50:00.000000Z",
        json!({ "code_id": Uuid::from_u128(0xE5_0001).to_string() }),
        MetadataFold::ConnectCodeIssued {
            code: oml_app::types::ConnectCode {
                code_id: Uuid::from_u128(0xE5_0001),
                key_id: expired_key.key_id,
                code_hash: Sha256Hash(ConnectCode::parse(expired).expect("a code").digest()),
                expires_at: at("2026-10-29T11:59:00.000000Z"),
            },
            voids: Some(live),
        },
    )
    .await;
    let rows = store.authorization_count();

    for (kind, entry) in [
        ("unknown", "ZZZZ-ZZZZ-ZZZZ"),
        ("malformed", "not a code"),
        ("expired", expired),
        ("voided", voided.as_str()),
        ("spent", spent.as_str()),
    ] {
        let handle = code_page(&app).await;
        let outcome = app
            .enter_code(&handle, entry, &PlainSealer)
            .await
            .expect("code entry is answered");
        let CodeEntryOutcome::WrongCode {
            handle: next,
            attempts_left: left,
        } = outcome
        else {
            panic!("a {kind} code answers connect_code_invalid: {outcome:?}");
        };
        assert_eq!(left, 4, "a {kind} code costs one attempt");
        assert_ne!(next, handle, "a {kind} code answers a new handle");
        assert_eq!(attempts_left(&next), 4, "the new handle carries it");
    }
    assert_eq!(
        store.authorization_count(),
        rows,
        "no wrong code writes a row"
    );
    assert_eq!(app.connect_code_failures(), 5);
}

/// The fifth wrong entry on one page ends the request: a redirect with
/// `access_denied` (`oauth.authorization_expired`), the `state` and `iss`
/// (http-api.md `POST /oauth/authorize`; ADR-0031 d9 step 2).
#[tokio::test]
async fn the_fifth_wrong_entry_ends_the_request_with_access_denied() {
    let app = oauth_app_over(MemStore::new());
    tenant(&app).await;
    let mut handle = code_page(&app).await;
    for left in (1..=4).rev() {
        match app
            .enter_code(&handle, "ZZZZ-ZZZZ-ZZZZ", &PlainSealer)
            .await
            .expect("code entry is answered")
        {
            CodeEntryOutcome::WrongCode {
                handle: next,
                attempts_left,
            } => {
                assert_eq!(attempts_left, left);
                handle = next;
            }
            other => panic!("a wrong entry with attempts left is the form: {other:?}"),
        }
    }
    let outcome = app
        .enter_code(&handle, "ZZZZ-ZZZZ-ZZZZ", &PlainSealer)
        .await
        .expect("code entry is answered");
    let CodeEntryOutcome::Denied {
        error,
        redirect_uri,
        state,
        iss,
    } = outcome
    else {
        panic!("the fifth wrong entry is a redirect: {outcome:?}");
    };
    assert_eq!(
        error,
        Refusal::AuthorizationExpired(AuthorizationExpired::AttemptsSpent)
    );
    assert_eq!(error.authorize_error(), "access_denied");
    assert_eq!(redirect_uri, CLAUDE_CALLBACK);
    assert_eq!(state.as_deref(), Some(STATE));
    assert_eq!(iss, ISSUER);
    assert_eq!(app.connect_code_failures(), 5);
}

/// The code of a revoked key answers what any code that is not valid
/// answers, so the page never shows a dead key's grants (ADR-0031 d8).
#[tokio::test]
async fn a_revoked_keys_code_answers_connect_code_invalid() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) =
        connector_with_code(&app, &boot.admin_key.value, vec![grant(crew, Role::Writer)]).await;
    seal_onto(
        &store,
        boot.tenant_id,
        event_types::KEY_REVOKED,
        "2026-10-29T12:01:00.000000Z",
        json!({ "key_id": issued.key_id.to_string() }),
        MetadataFold::KeyRevoked {
            key_id: issued.key_id,
        },
    )
    .await;
    let rows = store.authorization_count();

    let outcome = app
        .enter_code(&code_page(&app).await, &code, &PlainSealer)
        .await
        .expect("code entry is answered");

    assert!(
        matches!(
            outcome,
            CodeEntryOutcome::WrongCode {
                attempts_left: 4,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(store.authorization_count(), rows);
    let codes = store
        .connect_codes(boot.tenant_id, issued.key_id)
        .await
        .expect("codes");
    assert!(codes[0].redeemed_at.is_none(), "the code is not spent");
    assert_eq!(
        app.connect_code_failures(),
        1,
        "the entry counts one failure"
    );
}

/// Connect deletes the request's row in its own commit, so a second
/// Connect with the same handle names no row, answers
/// `oauth.authorization_expired`, and one connect code yields one
/// connection (ADR-0031 d9 step 3).
#[tokio::test]
async fn connect_twice_with_one_handle_opens_at_most_one_connection() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) =
        connector_with_code(&app, &boot.admin_key.value, vec![grant(crew, Role::Writer)]).await;
    let view = entered(&app, &code_page(&app).await, &code).await;

    let first = app
        .confirm(&view.handle, ConfirmAction::Connect)
        .await
        .expect("Connect");
    assert!(
        matches!(first, ConfirmOutcome::Connected { .. }),
        "{first:?}"
    );
    let err = app
        .confirm(&view.handle, ConfirmAction::Connect)
        .await
        .unwrap_err();

    assert_eq!(
        authorization_expired(&err),
        Some(AuthorizationExpired::NoRequest),
        "{err:?}"
    );
    let connections = store
        .connections(boot.tenant_id, issued.key_id)
        .await
        .expect("connections");
    assert_eq!(connections.len(), 1);
}

/// A newer code voids the key's spent code, so a Connect on the page that
/// verified it answers `oauth.authorization_expired` as `access_denied` on
/// the request's redirect, with its `state` and `iss`, since the handle
/// names a row, and opens nothing (errors.md; ADR-0031 d8, d9 step 3).
#[tokio::test]
async fn connect_after_a_newer_code_was_minted_answers_authorization_expired() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (issued, code) = connector_with_code(&app, admin, vec![grant(crew, Role::Writer)]).await;
    let view = entered(&app, &code_page(&app).await, &code).await;
    app.mint_connect_code(admin, issued.key_id.into())
        .await
        .expect("a newer code");

    let outcome = app
        .confirm(&view.handle, ConfirmAction::Connect)
        .await
        .expect("a known handle is answered on its redirect");

    assert_eq!(
        outcome,
        denied_redirect(AuthorizationExpired::CodeVoided),
        "{outcome:?}"
    );
    assert!(
        store
            .connections(boot.tenant_id, issued.key_id)
            .await
            .expect("connections")
            .is_empty()
    );
}

/// Cancel redirects with `access_denied`, the `state` and `iss`, and takes
/// the request's row; the code stays spent, so it neither verifies again
/// nor lets the handle Connect (ADR-0031 d9 step 3).
#[tokio::test]
async fn the_connect_code_stays_spent_after_cancel() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) =
        connector_with_code(&app, &boot.admin_key.value, vec![grant(crew, Role::Writer)]).await;
    let view = entered(&app, &code_page(&app).await, &code).await;

    let cancelled = app
        .confirm(&view.handle, ConfirmAction::Cancel)
        .await
        .expect("Cancel");

    assert_eq!(
        cancelled,
        ConfirmOutcome::Cancelled {
            redirect_uri: CLAUDE_CALLBACK.to_owned(),
            state: Some(STATE.to_owned()),
            iss: ISSUER.to_owned(),
        }
    );
    let codes = store
        .connect_codes(boot.tenant_id, issued.key_id)
        .await
        .expect("codes");
    assert!(codes[0].redeemed_at.is_some(), "the code stays spent");
    let err = app
        .confirm(&view.handle, ConfirmAction::Connect)
        .await
        .unwrap_err();
    assert_eq!(
        authorization_expired(&err),
        Some(AuthorizationExpired::NoRequest)
    );
    let again = app
        .enter_code(&code_page(&app).await, &code, &PlainSealer)
        .await
        .expect("code entry is answered");
    assert!(
        matches!(again, CodeEntryOutcome::WrongCode { .. }),
        "{again:?}"
    );
    assert!(
        store
            .connections(boot.tenant_id, issued.key_id)
            .await
            .expect("connections")
            .is_empty()
    );
}

/// Connect stamps the client's `connected_at`, creates the connection
/// `authorized` with the digest of a 256-bit authorization code alive 60
/// seconds, and redirects with that code, the `state` and `iss`
/// (postgres-schema.md `oauth_clients`; ADR-0031 d5, d9 step 3).
#[tokio::test]
async fn connect_stamps_the_clients_connected_at() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) =
        connector_with_code(&app, &boot.admin_key.value, vec![grant(crew, Role::Writer)]).await;
    let view = entered(&app, &code_page(&app).await, &code).await;
    let client = store.oauth_client(DOC_URL).await.expect("read");
    assert_eq!(client.expect("the cached document").connected_at, None);

    let outcome = app
        .confirm(&view.handle, ConfirmAction::Connect)
        .await
        .expect("Connect");

    let ConfirmOutcome::Connected {
        redirect_uri,
        code: auth_code,
        state,
        iss,
    } = outcome
    else {
        panic!("Connect redirects with a code: {outcome:?}");
    };
    assert_eq!(redirect_uri, CLAUDE_CALLBACK);
    assert_eq!(state.as_deref(), Some(STATE));
    assert_eq!(iss, ISSUER);
    assert_eq!(auth_code.len(), 43, "a 256-bit code in base62");
    let client = store
        .oauth_client(DOC_URL)
        .await
        .expect("read")
        .expect("the cached document");
    assert!(client.connected_at.is_some(), "Connect stamps connected_at");
    let connections = store
        .connections(boot.tenant_id, issued.key_id)
        .await
        .expect("connections");
    let [connection] = connections.as_slice() else {
        panic!("one connection: {connections:?}");
    };
    assert_eq!(connection.state, ConnectionState::Authorized);
    assert_eq!(connection.resource, format!("{ISSUER}/mcp"));
    assert_eq!(
        connection.auth_code_hash,
        Some(Sha256Hash::digest(auth_code.as_bytes()))
    );
    let lifetime = connection
        .auth_code_expires_at
        .as_ref()
        .expect("an expiry")
        .unix_micros()
        - connection.created_at.unix_micros();
    assert_eq!(lifetime, 60_000_000, "the authorization code lives 60 s");
}

/// A confirmation row whose request outlived its ten minutes, copied from
/// the row `view` wrote under a new handle: the handle it answers to.
async fn lapsed_confirmation(store: &MemStore, view: &ConfirmationView) -> String {
    let handle = "a-confirmation-past-its-ten-minutes";
    let row = store
        .authorization(&Sha256Hash::digest(view.handle.as_bytes()))
        .await
        .expect("read")
        .expect("the request's row");
    store
        .authorization_insert(AuthorizationRecord {
            authorization_id: Uuid::from_u128(0x1A_75ED),
            handle_hash: Sha256Hash::digest(handle.as_bytes()),
            expires_at: at("2026-10-29T11:59:59.000000Z"),
            ..row
        })
        .await
        .expect("a lapsed row");
    handle.to_owned()
}

/// A Connect or a Cancel after the request's ten minutes answers
/// `oauth.authorization_expired` as `access_denied` on the request's
/// redirect, with its `state` and `iss`, and opens nothing (errors.md;
/// ADR-0031 d9 step 3).
#[tokio::test]
async fn connect_or_cancel_after_ten_minutes_answers_authorization_expired() {
    for action in [ConfirmAction::Connect, ConfirmAction::Cancel] {
        let store = MemStore::new();
        let app = oauth_app_over(store.clone());
        let (boot, crew) = tenant(&app).await;
        let (issued, code) =
            connector_with_code(&app, &boot.admin_key.value, vec![grant(crew, Role::Writer)]).await;
        let view = entered(&app, &code_page(&app).await, &code).await;
        let handle = lapsed_confirmation(&store, &view).await;

        let outcome = app
            .confirm(&handle, action)
            .await
            .expect("a known handle is answered on its redirect");

        assert_eq!(
            outcome,
            denied_redirect(AuthorizationExpired::Lapsed),
            "{action:?}"
        );
        assert!(
            store
                .connections(boot.tenant_id, issued.key_id)
                .await
                .expect("connections")
                .is_empty(),
            "{action:?} opens nothing"
        );
    }
}

/// A Cancel on a confirmation whose code a newer code voided answers
/// `oauth.authorization_expired` as `access_denied` on the request's
/// redirect, with its `state` and `iss` (errors.md; ADR-0031 d8, d9 step 3).
#[tokio::test]
async fn cancel_after_a_newer_code_answers_authorization_expired() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (issued, code) = connector_with_code(&app, admin, vec![grant(crew, Role::Writer)]).await;
    let view = entered(&app, &code_page(&app).await, &code).await;
    app.mint_connect_code(admin, issued.key_id.into())
        .await
        .expect("a newer code");

    let outcome = app
        .confirm(&view.handle, ConfirmAction::Cancel)
        .await
        .expect("a known handle is answered on its redirect");

    assert_eq!(outcome, denied_redirect(AuthorizationExpired::CodeVoided));
}

/// A code entered on a handle whose request outlived its ten minutes ends
/// the request as `access_denied` on its redirect, with its `state` and
/// `iss`, since the handle opens and so is known; one that does not open
/// answers `oauth.authorization_expired` on the page. Either is checked
/// before the code: neither spends it, writes a row nor counts a failure
/// (errors.md; ADR-0031 d8, d9 steps 1 and 2).
#[tokio::test]
async fn a_code_entered_on_a_lapsed_or_unopened_handle_spends_nothing_and_counts_no_failure() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) =
        connector_with_code(&app, &boot.admin_key.value, vec![grant(crew, Role::Writer)]).await;
    let live = code_page(&app).await;
    let request = open_request(
        PlainSealer.open(&live).as_deref(),
        &at("2026-10-29T12:00:01.000000Z"),
    )
    .expect("a live request");
    let lapsed = PlainSealer.seal(
        &AuthorizationRequest {
            expires_at: at("2026-10-29T11:59:59.000000Z"),
            ..request
        }
        .encode(),
    );
    let rows = store.authorization_count();

    let outcome = app
        .enter_code(&lapsed, &code, &PlainSealer)
        .await
        .expect("a handle that opens is answered on its redirect");
    let CodeEntryOutcome::Denied {
        error,
        redirect_uri,
        state,
        iss,
    } = outcome
    else {
        panic!("a lapsed handle is a redirect: {outcome:?}");
    };
    assert_eq!(
        error,
        Refusal::AuthorizationExpired(AuthorizationExpired::Lapsed)
    );
    assert_eq!(error.authorize_error(), "access_denied");
    assert_eq!(redirect_uri, CLAUDE_CALLBACK);
    assert_eq!(state.as_deref(), Some(STATE));
    assert_eq!(iss, ISSUER);

    let err = app
        .enter_code("not a sealed handle", &code, &PlainSealer)
        .await
        .unwrap_err();
    assert_eq!(
        authorization_expired(&err),
        Some(AuthorizationExpired::Unopened),
        "{err:?}"
    );

    let codes = store
        .connect_codes(boot.tenant_id, issued.key_id)
        .await
        .expect("codes");
    assert!(codes[0].redeemed_at.is_none(), "the code is not spent");
    assert_eq!(store.authorization_count(), rows, "no row is written");
    assert_eq!(app.connect_code_failures(), 0, "no failure is counted");
    entered(&app, &live, &code).await;
}

/// Without `oauth.issuer` the authorization page does not exist: a code
/// entry and a confirmation answer `access.not_found` (http-api.md;
/// ADR-0031 d1).
#[tokio::test]
async fn the_page_does_not_exist_without_an_issuer() {
    let store = MemStore::new();
    let oauth = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&oauth).await;
    let (_, code) = connector_with_code(
        &oauth,
        &boot.admin_key.value,
        vec![grant(crew, Role::Writer)],
    )
    .await;
    let handle = code_page(&oauth).await;
    let view = entered(&oauth, &code_page(&oauth).await, &code).await;
    let app = app_over(store);

    let entry = app.enter_code(&handle, &code, &PlainSealer).await;
    assert!(matches!(entry, Err(AppError::NotFound)), "{entry:?}");
    for action in [ConfirmAction::Connect, ConfirmAction::Cancel] {
        let outcome = app.confirm(&view.handle, action).await;
        assert!(
            matches!(outcome, Err(AppError::NotFound)),
            "{action:?}: {outcome:?}"
        );
    }
}

/// A dreamer connector key's confirmation names its granted workareas in
/// the propose sentence and in no write or read sentence (http-api.md
/// "The authorization page"; ADR-0031 d9 step 2).
#[tokio::test]
async fn a_dreamer_key_is_shown_as_proposing() {
    let app = oauth_app_over(MemStore::new());
    let (boot, crew) = tenant(&app).await;
    let issued = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                connect_code: true,
                ..issue_req(
                    Role::Dreamer,
                    "crew/claude-mobile",
                    vec![grant(crew, Role::Dreamer)],
                )
            },
        )
        .await
        .expect("a dreamer connector key");
    let code = issued
        .connect_code
        .and_then(|c| c.code)
        .expect("a fresh answer carries the code");

    let view = entered(&app, &code_page(&app).await, &code).await;

    assert_eq!(view.role, Role::Dreamer);
    assert_eq!(view.proposes_in, [name("crew")]);
    assert!(view.writes_in.is_empty(), "{view:?}");
    assert!(view.reads_in.is_empty(), "{view:?}");
}

/// A workarea a grant's own deny or a global denial masks is named in no
/// verb sentence that grant or the denied grant would give, while every
/// grant is still listed (http-api.md
/// "The authorization page"; roles-and-keys.md denials; ADR-0031 d9
/// step 2).
#[tokio::test]
async fn a_denied_workarea_is_named_in_no_sentence() {
    let app = oauth_app_over(MemStore::new());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let notes = workarea(&app, admin, "notes", crew).await;
    let archive = workarea(&app, admin, "archive", boot.root_workarea_id).await;
    let (issued, code) = connector_with_code(
        &app,
        admin,
        vec![
            GrantRequest {
                deny: vec![notes.into()],
                ..grant(crew, Role::Writer)
            },
            grant(notes, Role::Reader),
            grant(archive, Role::Reader),
        ],
    )
    .await;
    app.deny_key(admin, issued.key_id.into(), archive.into())
        .await
        .expect("a global denial");

    let view = entered(&app, &code_page(&app).await, &code).await;

    let listed: Vec<Name> = view.grants.iter().map(|g| g.workarea.clone()).collect();
    assert_eq!(listed, [name("crew"), name("notes"), name("archive")]);
    assert_eq!(view.writes_in, [name("crew")], "crew's deny masks notes");
    assert_eq!(
        view.reads_in,
        [name("notes")],
        "the global denial masks archive"
    );
}

/// A code of one tenant shows that tenant's name and that key's grants and
/// nothing of another tenant's, and its row is written under the code's
/// tenant (ADR-0031 d8, d9 step 2, d10).
#[tokio::test]
async fn a_code_of_one_tenant_shows_only_that_tenants_name_and_grants() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (acme, acme_crew) = tenant(&app).await;
    connector_with_code(
        &app,
        &acme.admin_key.value,
        vec![grant(acme_crew, Role::Writer)],
    )
    .await;
    let globex = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("globex"),
            admin_key_name: name("globex-admin"),
            admin_principal: "gina-admin".to_owned(),
        })
        .await
        .expect("a second tenant");
    let admin = &globex.admin_key.value;
    let lab = workarea(&app, admin, "lab", globex.root_workarea_id).await;
    let (issued, code) = connector_with_code(&app, admin, vec![grant(lab, Role::Reader)]).await;

    let view = entered(&app, &code_page(&app).await, &code).await;

    assert_eq!(view.tenant_name, name("globex"));
    let grants: Vec<(Uuid, Name)> = view
        .grants
        .iter()
        .map(|g| (g.workarea_id, g.workarea.clone()))
        .collect();
    assert_eq!(grants, [(lab, name("lab"))]);
    assert_eq!(view.reads_in, [name("lab")]);
    assert!(view.writes_in.is_empty(), "{view:?}");
    let row = store
        .authorization(&Sha256Hash::digest(view.handle.as_bytes()))
        .await
        .expect("read")
        .expect("the request's row");
    assert_eq!(row.code_tenant_id, globex.tenant_id);
    assert_eq!(row.key_id, issued.key_id);
}

// ---------------------------------------------------------------------------
// The token endpoint and dynamic registration (ADR-0031 d5, d10, d11, d12)
// ---------------------------------------------------------------------------

/// The fixture request's S256 verifier: `code_page`'s challenge is its
/// digest (RFC 7636 appendix B).
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

/// A writer connector key whose code was entered and connected on the page,
/// and the authorization code Connect answered (ADR-0031 d9 step 3).
async fn connected(app: &TestApp, admin: &str, crew: Uuid) -> (IssuedKey, String) {
    let (issued, code) = connector_with_code(app, admin, vec![grant(crew, Role::Writer)]).await;
    (issued, connect_with(app, &code).await)
}

/// Enters `code` on a fresh page and confirms Connect: the authorization
/// code.
async fn connect_with(app: &TestApp, code: &str) -> String {
    let view = entered(app, &code_page(app).await, code).await;
    match app
        .confirm(&view.handle, ConfirmAction::Connect)
        .await
        .expect("Connect")
    {
        ConfirmOutcome::Connected { code, .. } => code,
        other => panic!("Connect answers an authorization code: {other:?}"),
    }
}

/// The fixture client's exchange of `code` with its verifier.
fn exchange_form(code: &str) -> ExchangeForm {
    ExchangeForm {
        code: code.to_owned(),
        client_id: DOC_URL.to_owned(),
        redirect_uri: Some(CLAUDE_CALLBACK.to_owned()),
        code_verifier: VERIFIER.to_owned(),
        resource: Some(format!("{ISSUER}/mcp")),
        client_authentication: None,
    }
}

/// The fixture client's refresh with `token`.
fn refresh_form(token: &Token) -> RefreshForm {
    RefreshForm {
        refresh_token: token.as_str().to_owned(),
        client_id: DOC_URL.to_owned(),
        resource: None,
        client_authentication: None,
    }
}

fn code_invalid(err: &AppError) -> Option<AuthorizationCodeInvalid> {
    match err {
        AppError::OAuth(Refusal::AuthorizationCodeInvalid(reason)) => Some(*reason),
        _ => None,
    }
}

fn refresh_invalid(err: &AppError) -> Option<RefreshInvalid> {
    match err {
        AppError::OAuth(Refusal::RefreshInvalid(reason)) => Some(*reason),
        _ => None,
    }
}

/// The key's one connection in any state.
async fn only_connection(store: &MemStore, tenant: Uuid, key_id: Uuid) -> ConnectionRecord {
    let connections = store.connections(tenant, key_id).await.expect("read");
    assert_eq!(connections.len(), 1, "{connections:?}");
    connections.into_iter().next().expect("one connection")
}

/// The first exchange redeems the code, opens the connection and answers a
/// Bearer pair whose access token lives an hour; `connection.opened`
/// names the connector key on its envelope and the client, its kind and
/// digest, the redirect URI and the resource in its payload (event-catalog.md
/// `connection.opened`; ADR-0031 d10, d12).
#[tokio::test]
async fn the_first_exchange_opens_the_connection_and_journals_connection_opened_with_the_client_and_resource()
 {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;

    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");

    assert!(tokens.access_token.as_str().starts_with("oml_at_"));
    assert!(tokens.refresh_token.as_str().starts_with("oml_rt_"));
    assert_eq!(tokens.expires_in, 3_600);
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(connection.state, ConnectionState::Open);
    let events = journal(&store, boot.tenant_id).await;
    let opened = &events.last().expect("an event").envelope;
    assert_eq!(opened.event_type, event_types::CONNECTION_OPENED);
    assert_eq!(opened.api_key_id, Some(issued.key_id));
    assert_eq!(opened.principal, "crew/claude-mobile");
    assert_eq!(connection.opened_seq, Some(opened.seq));
    assert_eq!(
        opened.payload,
        json!({
            "connection_id": connection.connection_id.to_string(),
            "key_id": issued.key_id.to_string(),
            "code_id": connection.code_id.to_string(),
            "client_id": DOC_URL,
            "client_kind": "cimd",
            "client_metadata_digest": connection.client_metadata_digest.to_prefixed(),
            "redirect_uri": CLAUDE_CALLBACK,
            "resource": format!("{ISSUER}/mcp"),
            "replaces_connection_id": Value::Null,
        })
    );
}

/// A key holds at most one open connection: a newer code's exchange names
/// the open one in `replaces_connection_id`, and that one is revoked
/// `replaced` at the newer `connection.opened`'s seq, with no event of its
/// own (roles-and-keys.md connections; ADR-0031 d7, d12).
#[tokio::test]
async fn opening_a_second_connection_for_a_key_revokes_the_first_as_replaced() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (issued, code) = connected(&app, admin, crew).await;
    app.exchange(exchange_form(&code)).await.expect("first");
    let first = only_connection(&store, boot.tenant_id, issued.key_id).await;
    let newer = app
        .mint_connect_code(admin, Reference::Id(issued.key_id))
        .await
        .expect("a newer code")
        .code
        .expect("shown once");

    let second_code = connect_with(&app, &newer).await;
    app.exchange(exchange_form(&second_code))
        .await
        .expect("second");

    let events = journal(&store, boot.tenant_id).await;
    let opened = &events.last().expect("an event").envelope;
    assert_eq!(opened.event_type, event_types::CONNECTION_OPENED);
    assert_eq!(
        opened.payload["replaces_connection_id"],
        json!(first.connection_id.to_string())
    );
    let connections = store
        .connections(boot.tenant_id, issued.key_id)
        .await
        .expect("read");
    let states: Vec<_> = connections
        .iter()
        .map(|c| (c.state, c.revoked_seq))
        .collect();
    assert_eq!(
        states,
        [
            (
                ConnectionState::Revoked(RevokedCause::Replaced),
                Some(opened.seq)
            ),
            (ConnectionState::Open, None),
        ]
    );
}

/// A redeemed code presented again by the connection's client with the
/// verifier is reuse: `invalid_grant`, and `connection.revoked` with the
/// cause `code_reuse`, `revoked_by_key_id` null and the connector key on
/// the envelope, after which the connection's access token resolves to
/// nothing (roles-and-keys.md connections; event-catalog.md
/// `connection.revoked`; ADR-0031 d10, d12).
#[tokio::test]
async fn a_code_redeemed_twice_by_its_verifiers_holder_revokes_the_connection() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("first");

    let err = app.exchange(exchange_form(&code)).await.unwrap_err();

    assert_eq!(
        code_invalid(&err),
        Some(AuthorizationCodeInvalid::Reused),
        "{err:?}"
    );
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(
        connection.state,
        ConnectionState::Revoked(RevokedCause::CodeReuse)
    );
    let events = journal(&store, boot.tenant_id).await;
    let revoked = &events.last().expect("an event").envelope;
    assert_eq!(revoked.event_type, event_types::CONNECTION_REVOKED);
    assert_eq!(revoked.api_key_id, Some(issued.key_id));
    assert_eq!(
        revoked.payload,
        json!({
            "connection_id": connection.connection_id.to_string(),
            "key_id": issued.key_id.to_string(),
            "revoked_by_key_id": Value::Null,
            "cause": "code_reuse",
            "reason_digest": Value::Null,
        })
    );
    let resolved = app.resolve_access_token(tokens.access_token.as_str()).await;
    assert!(
        matches!(resolved, Err(AppError::Unauthorized)),
        "{resolved:?}"
    );
}

/// A redeemed code presented without the verifier, or by another client,
/// is `invalid_grant` and revokes nothing: only the verifier's holder can
/// have redeemed first (roles-and-keys.md connections; ADR-0031 d10).
#[tokio::test]
async fn a_redeemed_code_presented_without_the_verifier_revokes_nothing() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    app.exchange(exchange_form(&code)).await.expect("first");
    let before = journal(&store, boot.tenant_id).await.len();

    let wrong_verifier = app
        .exchange(ExchangeForm {
            code_verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXl".to_owned(),
            ..exchange_form(&code)
        })
        .await
        .unwrap_err();
    let other_client = app
        .exchange(ExchangeForm {
            client_id: "https://other.example.com/client.json".to_owned(),
            ..exchange_form(&code)
        })
        .await
        .unwrap_err();

    for err in [&wrong_verifier, &other_client] {
        assert_eq!(
            code_invalid(err),
            Some(AuthorizationCodeInvalid::Redeemed),
            "{err:?}"
        );
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(connection.state, ConnectionState::Open);
}

/// A failed exchange consumes nothing: after a wrong verifier, a wrong
/// client and a wrong `redirect_uri`, the right presentation still opens
/// the connection (roles-and-keys.md connections; ADR-0031 d10).
#[tokio::test]
async fn a_wrong_verifier_does_not_consume_the_code() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let failures = [
        (
            ExchangeForm {
                code_verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXl".to_owned(),
                ..exchange_form(&code)
            },
            AuthorizationCodeInvalid::VerifierMismatch,
        ),
        (
            ExchangeForm {
                client_id: "https://other.example.com/client.json".to_owned(),
                ..exchange_form(&code)
            },
            AuthorizationCodeInvalid::ClientMismatch,
        ),
        (
            ExchangeForm {
                redirect_uri: None,
                ..exchange_form(&code)
            },
            AuthorizationCodeInvalid::RedirectMismatch,
        ),
    ];

    for (form, reason) in failures {
        let err = app.exchange(form).await.unwrap_err();
        assert_eq!(code_invalid(&err), Some(reason), "{err:?}");
    }
    let before = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(before.state, ConnectionState::Authorized);
    assert_eq!(before.auth_code_redeemed_at, None);
    app.exchange(exchange_form(&code))
        .await
        .expect("the code is still redeemable");
    let after = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(after.state, ConnectionState::Open);
}

/// A refresh answers a new pair and marks the presented token rotated; the
/// rotated token presented again is `invalid_grant` and revokes the
/// connection with `refresh_reuse` in a `connection.revoked` of its own,
/// after which the newer refresh token refreshes nothing (roles-and-keys.md
/// connections; ADR-0031 d10, d11, d12).
#[tokio::test]
async fn a_refresh_rotates_and_the_old_refresh_token_presented_again_revokes_the_connection() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let first = app.exchange(exchange_form(&code)).await.expect("exchange");

    let second = app
        .refresh(refresh_form(&first.refresh_token))
        .await
        .expect("a refresh rotates");
    assert_ne!(second.refresh_token, first.refresh_token);
    assert_ne!(second.access_token, first.access_token);
    let err = app
        .refresh(refresh_form(&first.refresh_token))
        .await
        .unwrap_err();

    assert_eq!(
        refresh_invalid(&err),
        Some(RefreshInvalid::Reused),
        "{err:?}"
    );
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(
        connection.state,
        ConnectionState::Revoked(RevokedCause::RefreshReuse)
    );
    let events = journal(&store, boot.tenant_id).await;
    let revoked = &events.last().expect("an event").envelope;
    assert_eq!(revoked.event_type, event_types::CONNECTION_REVOKED);
    assert_eq!(revoked.api_key_id, Some(issued.key_id));
    assert_eq!(revoked.payload["cause"], json!("refresh_reuse"));
    assert_eq!(revoked.payload["revoked_by_key_id"], Value::Null);
    assert_eq!(connection.revoked_seq, Some(revoked.seq));
    let after = app
        .refresh(refresh_form(&second.refresh_token))
        .await
        .unwrap_err();
    assert_eq!(
        refresh_invalid(&after),
        Some(RefreshInvalid::ConnectionNotOpen),
        "{after:?}"
    );
}

/// A refresh token presented with another `client_id` is `invalid_grant`
/// and rotates nothing, so its own client still refreshes with it
/// (http-api.md `POST /oauth/token`; ADR-0031 d5, d10).
#[tokio::test]
async fn a_refresh_token_of_another_client_is_invalid_grant() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (_, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");

    let err = app
        .refresh(RefreshForm {
            client_id: "https://other.example.com/client.json".to_owned(),
            ..refresh_form(&tokens.refresh_token)
        })
        .await
        .unwrap_err();

    assert_eq!(
        refresh_invalid(&err),
        Some(RefreshInvalid::ClientMismatch),
        "{err:?}"
    );
    app.refresh(refresh_form(&tokens.refresh_token))
        .await
        .expect("its own client still refreshes");
}

/// A connection is bound to `<issuer>/mcp` at Connect: under another
/// `oauth.issuer` its refresh is `invalid_grant` and rotates nothing, so
/// with the origin set back the same token refreshes (roles-and-keys.md
/// connections, live; ADR-0031 d1, d10).
#[tokio::test]
async fn a_refresh_after_the_issuer_changed_is_invalid_grant_and_rotates_nothing() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (_, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");
    let moved = app_over(store.clone())
        .with_oauth_issuer(Issuer::parse("https://memory.example.org").expect("an origin"));

    let err = moved
        .refresh(refresh_form(&tokens.refresh_token))
        .await
        .unwrap_err();

    assert_eq!(
        refresh_invalid(&err),
        Some(RefreshInvalid::ResourceNotCurrent),
        "{err:?}"
    );
    app.refresh(refresh_form(&tokens.refresh_token))
        .await
        .expect("nothing rotated");
}

/// A rotation journals nothing: it is no actor's act on the connection
/// (event-catalog.md `connection.*`; ADR-0031 d12).
#[tokio::test]
async fn a_refresh_is_not_journalled() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (_, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");
    let before = hashes(&journal(&store, boot.tenant_id).await);

    app.refresh(refresh_form(&tokens.refresh_token))
        .await
        .expect("refresh");

    assert_eq!(hashes(&journal(&store, boot.tenant_id).await), before);
}

/// An access token of an open connection, on the current resource, of a
/// live key resolves to the caller that key's own authentication builds:
/// the key's row and its grants and denials, read on the request
/// (roles-and-keys.md connections; ADR-0031 d11).
#[tokio::test]
async fn an_access_token_resolves_to_exactly_its_keys_caller() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");

    let caller = app
        .resolve_access_token(tokens.access_token.as_str())
        .await
        .expect("an open connection's access token resolves");

    let key = store
        .key(boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("the connector key");
    assert_eq!(caller.key(), &key);
    let access = store
        .key_access(boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("its access");
    let grants: Vec<_> = access.grants.iter().map(|g| g.grant.clone()).collect();
    assert_eq!(caller.grants(), grants.as_slice());
    assert!(caller.denials().is_empty());
    assert_eq!(caller.permission_epoch(), access.permission_epoch());
}

/// A refresh token's digest finds no access row, so it never authenticates
/// a request, nor does a key value (roles-and-keys.md connections;
/// ADR-0031 d11).
#[tokio::test]
async fn a_refresh_token_never_resolves_as_an_access_credential() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (_, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");

    for presented in [tokens.refresh_token.as_str(), boot.admin_key.value.as_str()] {
        let resolved = app.resolve_access_token(presented).await;
        assert!(
            matches!(resolved, Err(AppError::Unauthorized)),
            "{resolved:?}"
        );
    }
}

/// An app over `store` whose deployment sets `oauth.issuer`, and a handle
/// on its clock that moves its time forward.
fn oauth_app_with_clock(store: MemStore) -> (TestApp, FixedClock) {
    let clock = FixedClock::new();
    let app = App::new(
        store,
        MemSigner::new(TEST_SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"));
    (app, clock)
}

/// `key.revoked` of `key_id`, sealed onto `tenant`'s chain as a fixture.
async fn revoke_key(store: &MemStore, tenant: Uuid, key_id: Uuid) {
    seal_onto(
        store,
        tenant,
        event_types::KEY_REVOKED,
        "2026-10-29T12:01:00.000000Z",
        json!({ "key_id": key_id.to_string() }),
        MetadataFold::KeyRevoked { key_id },
    )
    .await;
}

/// Whether `token` resolves to no caller.
async fn unauthorized(app: &TestApp, token: &Token) -> bool {
    matches!(
        app.resolve_access_token(token.as_str()).await,
        Err(AppError::Unauthorized)
    )
}

/// Each event's type, in chain order from `from`.
fn types_from(events: &[SignedEnvelope], from: usize) -> Vec<&str> {
    events[from..]
        .iter()
        .map(|e| e.envelope.event_type.as_str())
        .collect()
}

/// An access token lives its hour: the last microsecond of it resolves,
/// and the token presented at its expiry is refused (roles-and-keys.md
/// connections; ADR-0031 d11).
#[tokio::test]
async fn an_access_token_presented_after_its_hour_is_unauthorized() {
    let store = MemStore::new();
    let (app, clock) = oauth_app_with_clock(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (_, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");

    clock.advance(tokens.expires_in - 1);
    app.resolve_access_token(tokens.access_token.as_str())
        .await
        .expect("within its hour the token resolves");
    clock.advance(1);

    assert!(unauthorized(&app, &tokens.access_token).await);
}

/// A connection's access token resolves to nothing once its key's
/// `key.revoked` commits, the connection's own state untouched
/// (roles-and-keys.md connections; ADR-0031 d11).
#[tokio::test]
async fn after_its_keys_key_revoked_a_connections_access_token_is_unauthorized() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");

    revoke_key(&store, boot.tenant_id, issued.key_id).await;

    assert!(unauthorized(&app, &tokens.access_token).await);
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(connection.state, ConnectionState::Open);
}

/// A connection is bound to `<issuer>/mcp` at Connect: under another
/// `oauth.issuer` its access token resolves to nothing, and with the
/// origin set back the same token resolves again (roles-and-keys.md
/// connections, live; ADR-0031 d1, d11).
#[tokio::test]
async fn an_access_token_under_a_moved_issuer_is_unauthorized_and_resolves_once_it_is_set_back() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (_, code) = connected(&app, &boot.admin_key.value, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");
    let moved = app_over(store.clone())
        .with_oauth_issuer(Issuer::parse("https://memory.example.org").expect("an origin"));

    assert!(unauthorized(&moved, &tokens.access_token).await);

    app.resolve_access_token(tokens.access_token.as_str())
        .await
        .expect("the origin set back resolves the token");
}

/// An admin's revocation of the connection ends its access token at once
/// (roles-and-keys.md connections; ADR-0031 d11).
#[tokio::test]
async fn after_an_admin_revokes_the_connection_its_access_token_is_unauthorized() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (issued, code) = connected(&app, admin, crew).await;
    let tokens = app.exchange(exchange_form(&code)).await.expect("exchange");
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;

    app.revoke_connection(admin, issued.key_id.into(), connection.connection_id, None)
        .await
        .expect("an admin revokes the open connection");

    assert!(unauthorized(&app, &tokens.access_token).await);
}

/// Two tenants' connections each resolve to their own tenant's connector
/// key, whatever tenant the token's digest is found under
/// (roles-and-keys.md connections; ADR-0031 d10, d11).
#[tokio::test]
async fn each_tenants_access_token_resolves_to_its_own_tenants_key() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (acme, acme_crew) = tenant(&app).await;
    let (acme_key, acme_code) = connected(&app, &acme.admin_key.value, acme_crew).await;
    let acme_tokens = app
        .exchange(exchange_form(&acme_code))
        .await
        .expect("acme's exchange");
    let globex = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("globex"),
            admin_key_name: name("globex-admin"),
            admin_principal: "gina-admin".to_owned(),
        })
        .await
        .expect("a second tenant");
    let lab = workarea(
        &app,
        &globex.admin_key.value,
        "lab",
        globex.root_workarea_id,
    )
    .await;
    let (globex_key, globex_code) = connected(&app, &globex.admin_key.value, lab).await;
    let globex_tokens = app
        .exchange(exchange_form(&globex_code))
        .await
        .expect("globex's exchange");

    for (tokens, tenant_id, key_id) in [
        (&acme_tokens, acme.tenant_id, acme_key.key_id),
        (&globex_tokens, globex.tenant_id, globex_key.key_id),
    ] {
        let caller = app
            .resolve_access_token(tokens.access_token.as_str())
            .await
            .expect("each token resolves");
        assert_eq!(
            (caller.key().tenant_id, caller.key().key_id),
            (tenant_id, key_id)
        );
    }
}

/// A key revoked after Connect ends its authorized connection at the
/// exchange: `invalid_grant` with the reason `key_ended`, no
/// `connection.opened`, and the code stays unredeemed (roles-and-keys.md
/// connections; ADR-0031 d10, d11).
#[tokio::test]
async fn a_code_whose_key_was_revoked_after_connect_is_key_ended_and_opens_nothing() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    revoke_key(&store, boot.tenant_id, issued.key_id).await;
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app.exchange(exchange_form(&code)).await.unwrap_err();

    assert_eq!(
        code_invalid(&err),
        Some(AuthorizationCodeInvalid::KeyEnded),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(connection.state, ConnectionState::Authorized);
    assert_eq!(connection.auth_code_redeemed_at, None);
}

/// A newer connect code minted after Connect voids the earlier code, so
/// the earlier authorization code is `invalid_grant` with the reason
/// `code_voided` and opens nothing (roles-and-keys.md connections;
/// ADR-0031 d8, d10).
#[tokio::test]
async fn a_newer_code_minted_after_connect_makes_the_earlier_authorization_code_code_voided() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (issued, code) = connected(&app, admin, crew).await;
    app.mint_connect_code(admin, issued.key_id.into())
        .await
        .expect("a newer code");
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app.exchange(exchange_form(&code)).await.unwrap_err();

    assert_eq!(
        code_invalid(&err),
        Some(AuthorizationCodeInvalid::CodeVoided),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(connection.state, ConnectionState::Authorized);
    assert_eq!(connection.auth_code_redeemed_at, None);
}

/// An authorization code lives 60 seconds from Connect: presented at its
/// expiry it is `invalid_grant` with the reason `expired`, and it stays
/// unredeemed (roles-and-keys.md connections; ADR-0031 d9, d10).
#[tokio::test]
async fn an_authorization_code_presented_sixty_seconds_after_connect_is_expired_and_stays_unredeemed()
 {
    let store = MemStore::new();
    let (app, clock) = oauth_app_with_clock(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();
    clock.advance(60);

    let err = app.exchange(exchange_form(&code)).await.unwrap_err();

    assert_eq!(
        code_invalid(&err),
        Some(AuthorizationCodeInvalid::Expired),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(connection.state, ConnectionState::Authorized);
    assert_eq!(connection.auth_code_redeemed_at, None);
}

/// A `key.revoked` that commits after the exchange read the key live and
/// before its `connection.opened` is refused by the opening's fold under
/// the chain lock; the exchange judges the code once more on a fresh read
/// and answers `invalid_grant` with the reason `key_ended`, so the journal
/// gains the `key.revoked` alone (roles-and-keys.md connections; ADR-0031
/// d10, d11).
#[tokio::test]
async fn a_key_revoked_between_the_exchanges_read_and_its_commit_is_key_ended_and_opens_nothing() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();
    let racing = store.clone();
    let tenant_id = boot.tenant_id;
    let key_id = issued.key_id;
    store.before_next_head_read(async move { revoke_key(&racing, tenant_id, key_id).await });

    let err = app.exchange(exchange_form(&code)).await.unwrap_err();

    assert_eq!(
        code_invalid(&err),
        Some(AuthorizationCodeInvalid::KeyEnded),
        "{err:?}"
    );
    let events = journal(&store, boot.tenant_id).await;
    assert_eq!(types_from(&events, before), [event_types::KEY_REVOKED]);
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(connection.state, ConnectionState::Authorized);
    assert_eq!(connection.auth_code_redeemed_at, None);
}

/// Two presentations of one code by its verifier's holder race: the one
/// whose `connection.opened` commits first opens the connection, and the
/// one the fold then refuses is judged by the reuse rule on its fresh
/// read, `invalid_grant` with the reason `reused`, revoking the
/// connection with `code_reuse` (roles-and-keys.md connections; ADR-0031
/// d10, d12).
#[tokio::test]
async fn an_exchange_that_loses_the_race_for_its_code_is_judged_reused_and_revokes_the_connection()
{
    let store = MemStore::new();
    let app = std::sync::Arc::new(oauth_app_over(store.clone()));
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();
    let winner = std::sync::Arc::clone(&app);
    let form = exchange_form(&code);
    store.before_next_head_read(async move {
        winner.exchange(form).await.expect("the winning exchange");
    });

    let err = app.exchange(exchange_form(&code)).await.unwrap_err();

    assert_eq!(
        code_invalid(&err),
        Some(AuthorizationCodeInvalid::Reused),
        "{err:?}"
    );
    let events = journal(&store, boot.tenant_id).await;
    assert_eq!(
        types_from(&events, before),
        [
            event_types::CONNECTION_OPENED,
            event_types::CONNECTION_REVOKED
        ]
    );
    assert_eq!(
        events.last().expect("an event").envelope.payload["cause"],
        json!("code_reuse")
    );
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(
        connection.state,
        ConnectionState::Revoked(RevokedCause::CodeReuse)
    );
}

/// A code whose 60 seconds end after the exchange read it and before its
/// commit is refused by the fold at the commit's time, and the fresh read
/// answers `invalid_grant` with the reason `expired`, opening nothing
/// (roles-and-keys.md connections; ADR-0031 d10).
#[tokio::test]
async fn a_code_that_expires_between_the_exchanges_read_and_its_commit_is_expired() {
    let store = MemStore::new();
    let (app, clock) = oauth_app_with_clock(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();
    let later = clock.clone();
    store.before_next_head_read(async move { later.advance(60) });

    let err = app.exchange(exchange_form(&code)).await.unwrap_err();

    assert_eq!(
        code_invalid(&err),
        Some(AuthorizationCodeInvalid::Expired),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
    assert_eq!(connection.auth_code_redeemed_at, None);
}

/// Refuses the next `times` openings of `connection_id`, each armed at the
/// head read before its commit.
fn refuse_openings(store: &MemStore, connection_id: Uuid, times: u32) {
    if times == 0 {
        return;
    }
    let handle = store.clone();
    store.before_next_head_read(async move {
        handle.refuse_next_commit_of(
            event_types::CONNECTION_OPENED,
            StoreError::ConnectionNotOpenable { connection_id },
        );
        refuse_openings(&handle, connection_id, times - 1);
    });
}

/// An opening the fold refuses once is judged again on a fresh read and,
/// still redeemable, opens; one refused on that second try too answers
/// `invalid_grant` with the reason `unknown` and opens nothing
/// (roles-and-keys.md connections; ADR-0031 d10).
#[tokio::test]
async fn an_opening_refused_once_opens_on_its_fresh_read_and_one_refused_twice_is_unknown() {
    for (times, opens) in [(1, true), (2, false)] {
        let store = MemStore::new();
        let app = oauth_app_over(store.clone());
        let (boot, crew) = tenant(&app).await;
        let (issued, code) = connected(&app, &boot.admin_key.value, crew).await;
        let authorized = only_connection(&store, boot.tenant_id, issued.key_id).await;
        let before = journal(&store, boot.tenant_id).await.len();
        refuse_openings(&store, authorized.connection_id, times);

        let answer = app.exchange(exchange_form(&code)).await;

        let connection = only_connection(&store, boot.tenant_id, issued.key_id).await;
        let events = journal(&store, boot.tenant_id).await;
        if opens {
            answer.expect("the second try opens");
            assert_eq!(connection.state, ConnectionState::Open);
            assert_eq!(
                types_from(&events, before),
                [event_types::CONNECTION_OPENED]
            );
        } else {
            let err = answer.unwrap_err();
            assert_eq!(
                code_invalid(&err),
                Some(AuthorizationCodeInvalid::Unknown),
                "{err:?}"
            );
            assert_eq!(connection.state, ConnectionState::Authorized);
            assert_eq!(events.len(), before);
        }
    }
}

/// A token request that sends client authentication is
/// `oauth.client_invalid`, by the scheme it used, before any lookup
/// (http-api.md `POST /oauth/token`; ADR-0031 d5).
#[tokio::test]
async fn a_token_request_that_sends_client_authentication_is_client_invalid_with_its_scheme() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let (_, code) = connected(&app, &boot.admin_key.value, crew).await;

    let err = app
        .exchange(ExchangeForm {
            client_authentication: Some(ClientAuthentication::Header {
                scheme: "Basic".to_owned(),
            }),
            ..exchange_form(&code)
        })
        .await
        .unwrap_err();

    assert!(
        matches!(&err, AppError::ClientAuthentication { scheme: Some(s) } if s == "Basic"),
        "{err:?}"
    );
    app.exchange(exchange_form(&code))
        .await
        .expect("the refused request consumed nothing");
}

/// The page shows `client_name`, so a registration without it is
/// `request.validation`, carried as `invalid_client_metadata`, and writes
/// no client (http-api.md `POST /oauth/register`; ADR-0031 d5).
#[tokio::test]
async fn a_registration_without_client_name_is_invalid_client_metadata() {
    let app = oauth_app_over(MemStore::new());
    let body = format!(r#"{{"redirect_uris":["{CLAUDE_CALLBACK}"]}}"#);

    let err = app.register(body.as_bytes()).await.unwrap_err();

    assert!(
        matches!(
            err,
            AppError::OAuth(Refusal::RequestValidation(Validation::ClientNameMissing))
        ),
        "{err:?}"
    );
}

/// An absent `token_endpoint_auth_method` is registered and echoed as
/// `none`, under a server-minted `client_id` and with no secret, and the
/// registration's row is a `dcr` client (http-api.md `POST /oauth/register`;
/// ADR-0031 d5).
#[tokio::test]
async fn an_absent_auth_method_registers_as_none() {
    let store = MemStore::new();
    let app = oauth_app_over(store.clone());
    let body = format!(r#"{{"client_name":"Desk","redirect_uris":["{CLAUDE_CALLBACK}"]}}"#);

    let registered = app.register(body.as_bytes()).await.expect("registered");

    assert_eq!(registered.token_endpoint_auth_method, "none");
    assert_eq!(registered.client_name, "Desk");
    assert_eq!(registered.redirect_uris, [CLAUDE_CALLBACK]);
    let row = store
        .oauth_client(&registered.client_id)
        .await
        .expect("read")
        .expect("the registration's row");
    assert_eq!(row.kind, ClientKind::Dcr);
    assert_eq!(row.metadata_digest, Sha256Hash::digest(body.as_bytes()));
}

/// What a [`oml_app::ports::RequestObserver`] was told, in order.
#[derive(Default)]
struct Told(std::sync::Mutex<Vec<String>>);

impl oml_app::ports::RequestObserver for Told {
    fn credential(&self, key: Option<&KeyRecord>, connection: Option<Uuid>) {
        let told = match (key, connection) {
            (Some(key), None) => format!("key {}", key.key_id),
            (Some(key), Some(c)) => format!("key {} via {c}", key.key_id),
            (None, _) => "no key".to_owned(),
        };
        self.0.lock().expect("unpoisoned").push(told);
    }

    fn workarea(&self, workarea_id: Uuid) {
        self.0
            .lock()
            .expect("unpoisoned")
            .push(format!("workarea {workarea_id}"));
    }
}

/// The request line's identity fields come from the app: it tells its
/// observer the key a credential resolved to, or that it resolved to none,
/// and the workarea the verb targets, by id (ADR-0042 d2).
#[tokio::test]
async fn the_app_tells_its_observer_the_key_a_credential_named_and_the_workarea_a_submit_targets() {
    let told = std::sync::Arc::new(Told::default());
    let app = app().with_observer(told.clone());
    let (boot, crew) = tenant(&app).await;
    let writer = agent(&app, &boot.admin_key.value, "agent-a", crew).await;
    told.0.lock().expect("unpoisoned").clear();
    app.submit_memory(&writer.value, &submit_into("idem-told", "a fact", crew))
        .await
        .expect("submit");
    let refused = app
        .submit_memory("oml_sk_no-such-key", &submit_into("idem-x", "a fact", crew))
        .await;
    assert!(matches!(refused, Err(AppError::Unauthorized)));
    assert_eq!(
        *told.0.lock().expect("unpoisoned"),
        vec![
            format!("key {}", writer.key_id),
            format!("workarea {crew}"),
            "no key".to_owned(),
        ]
    );
}

// ---------------------------------------------------------------------------
// Guardrails (proposed ADR-0037): the kind, its admission and review (S1),
// and its read under the key's guardrail scope (S2)
// ---------------------------------------------------------------------------

use oml_app::domain::guardrail::{GuardrailScope, LOAD_WARNING, SET_CAP, Severity};
use oml_app::types::{GuardrailRequest, GuardrailSet};

fn deny_push() -> Value {
    json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"], "effect": "deny"})
}

/// A guardrail of its own `claim_key` on `workarea_id` with `severity` and
/// no rule.
fn guardrail_req(idem: &str, severity: Severity, body: &str, workarea_id: Uuid) -> SubmitRequest {
    SubmitRequest {
        severity: Some(severity),
        ..submit_kind(idem, MemoryKind::Guardrail, body, workarea_id)
    }
}

/// The set the key loads at its default workarea.
async fn guardrail_set(app: &TestApp, key: &str) -> GuardrailSet {
    app.guardrails(key, &GuardrailRequest::default())
        .await
        .expect("the guardrail read")
}

fn served_guardrail_ids(set: &GuardrailSet) -> Vec<Uuid> {
    set.guardrails.iter().map(|g| g.artifact_id).collect()
}

/// A reader key on `node` with `scope`.
async fn reader_scoped(
    app: &TestApp,
    admin: &str,
    principal: &str,
    node: Uuid,
    scope: Option<GuardrailScope>,
) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            guardrail_scope: scope,
            ..issue_req(Role::Reader, principal, vec![grant(node, Role::Reader)])
        },
    )
    .await
    .expect("issue key")
}

#[tokio::test]
async fn a_guardrail_without_a_severity_is_refused() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda", crew).await;
    let err = app
        .submit_memory(
            &writer.value,
            &submit_kind("g1", MemoryKind::Guardrail, "Never push to main.", crew),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::GuardrailSeverityRequired),
        "{err:?}"
    );

    // Both fields are a guardrail's own: refused on any other kind.
    let err = app
        .submit_memory(
            &writer.value,
            &SubmitRequest {
                severity: Some(Severity::Must),
                ..submit_into("m1", "a fact", crew)
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::GuardrailFieldRefused { field: "severity" }),
        "{err:?}"
    );
    let err = app
        .submit_memory(
            &writer.value,
            &SubmitRequest {
                rule: Some(deny_push()),
                ..submit_into("m2", "a fact", crew)
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::GuardrailFieldRefused { field: "rule" }),
        "{err:?}"
    );
    assert!(
        journal(&store, boot.tenant_id)
            .await
            .iter()
            .all(|e| e.envelope.event_type != event_types::SUBMISSION_REMEMBER),
        "a refusal journals nothing"
    );
}

#[tokio::test]
async fn a_guardrail_target_admits_only_kind_guardrail() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let rail = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g1", Severity::Must, "Run the tests before a commit.", crew),
    )
    .await;
    let fact = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &submit_into("m1", "a fact", crew),
    )
    .await;

    for sent in [
        MemoryKind::Memory,
        MemoryKind::Steering,
        MemoryKind::DerivedFact,
    ] {
        let err = app
            .supersede(
                &writer.value,
                &SupersedeRequest {
                    kind: Some(sent),
                    ..supersede_req(&format!("s-{}", sent.as_str()), rail, "now a fact")
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::KindChangeRefused { .. }), "{err:?}");
    }
    let err = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                kind: Some(MemoryKind::Guardrail),
                severity: Some(Severity::Must),
                ..supersede_req("s-fact", fact, "now a rule")
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::KindChangeRefused {
                target: MemoryKind::Memory,
                sent: MemoryKind::Guardrail
            }
        ),
        "no other target admits it: {err:?}"
    );

    // A guardrail successor re-sends its severity; a retirement sends none.
    let err = app
        .supersede(&writer.value, &supersede_req("s-bare", rail, "Run them."))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::GuardrailSeverityRequired),
        "{err:?}"
    );
    let err = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                severity: Some(Severity::Must),
                ..retire_req("s-retire-sev", rail, "no longer needed")
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::GuardrailFieldRefused { field: "severity" }),
        "{err:?}"
    );
    let successor = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                severity: Some(Severity::Should),
                ..supersede_req("s-ok", rail, "Run the tests before a push.")
            },
        )
        .await
        .expect("a guardrail successor");
    let row = app
        .get_memory(&writer.value, successor.receipt.memory_id)
        .await
        .expect("get");
    assert_eq!(row.memory.kind, MemoryKind::Guardrail);
    assert_eq!(
        row.memory.guardrail.map(|g| g.severity),
        Some(Severity::Should)
    );
}

#[tokio::test]
async fn a_rule_outside_oml_gr_1_or_with_allow_is_vetoed_as_guardrail_rule_unparsable() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda", crew).await;
    let refused = [
        (
            Severity::Must,
            json!({"rule": "oml-gr/2", "tool": "shell", "command": ["git"], "effect": "deny"}),
        ),
        (
            Severity::Must,
            json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git"], "effect": "allow"}),
        ),
        (Severity::Should, deny_push()),
    ];
    for (i, (severity, rule)) in refused.into_iter().enumerate() {
        let err = app
            .submit_memory(
                &writer.value,
                &SubmitRequest {
                    rule: Some(rule.clone()),
                    ..guardrail_req(&format!("g{i}"), severity, "Never push.", crew)
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&err, AppError::ScanVeto(finding) if finding == "guardrail.rule_unparsable"),
            "{rule}: {err:?}"
        );
    }
    let held = app
        .submit_memory(
            &writer.value,
            &SubmitRequest {
                rule: Some(deny_push()),
                ..guardrail_req("ok", Severity::Never, "Never push.", crew)
            },
        )
        .await
        .expect("an oml-gr/1 deny under never");
    assert_eq!(held.receipt.state, "held");
}

#[tokio::test]
async fn a_body_over_300_bytes_is_vetoed_as_guardrail_too_long() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let writer = issue(&app, &boot.admin_key.value, Role::Writer, "wanda", crew).await;
    let err = app
        .submit_memory(
            &writer.value,
            &guardrail_req("g1", Severity::Must, &"x".repeat(301), crew),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AppError::ScanVeto(finding) if finding == "guardrail.too_long"),
        "{err:?}"
    );
    app.submit_memory(
        &writer.value,
        &guardrail_req("g2", Severity::Must, &"x".repeat(300), crew),
    )
    .await
    .expect("300 bytes is under the cap");
}

#[tokio::test]
async fn a_guardrail_promotes_under_its_workarea_policy_and_no_author_verdict_counts() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let author_dreamer = issue(&app, admin, Role::Dreamer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let id = app
        .submit_memory(
            &writer.value,
            &guardrail_req("g1", Severity::Never, "Never push to main.", crew),
        )
        .await
        .expect("submit")
        .receipt
        .memory_id;
    let err = app
        .post_verdict(&author_dreamer.value, id, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Verdict(oml_app::domain::VerdictError::PrincipalInLineage(_))
        ),
        "{err:?}"
    );
    assert_eq!(
        app.get_memory(&writer.value, id)
            .await
            .expect("get")
            .memory
            .status,
        MemoryStatus::Held
    );
    let outcome = app
        .post_verdict(&dreamer.value, id, Verdict::Approve, None)
        .await
        .expect("a non-author verdict");
    assert_eq!(outcome.state, MemoryStatus::Promoted);
}

#[tokio::test]
async fn a_guardrail_is_never_self_signed_whatever_the_tenant_policy() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let id = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g1", Severity::Must, "Run the tests first.", crew),
    )
    .await;
    let events = journal(&store, boot.tenant_id).await;
    let promote = events
        .iter()
        .find(|e| {
            e.envelope.event_type == event_types::LIFECYCLE_PROMOTE
                && e.envelope.payload["artifact_id"] == id.to_string()
        })
        .expect("the promotion");
    assert_eq!(promote.envelope.payload["self_signed"], json!(false));
    let ratifying = promote.envelope.payload["ratification_event_id"].clone();
    let verdict = events
        .iter()
        .find(|e| json!(e.envelope.event_id.to_string()) == ratifying)
        .expect("the ratifying verdict");
    assert_eq!(verdict.envelope.event_type, event_types::REVIEW_APPROVE);
    assert_ne!(
        verdict.envelope.principal, "wanda",
        "a non-author ratified it"
    );
}

#[tokio::test]
async fn the_serve_of_a_held_guardrail_carries_the_load_warning() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let rail = app
        .submit_memory(
            &writer.value,
            &guardrail_req("g1", Severity::Must, "Run the tests first.", crew),
        )
        .await
        .expect("submit")
        .receipt
        .memory_id;
    app.submit_memory(&writer.value, &submit_into("m1", "a fact", crew))
        .await
        .expect("submit");
    let own = app.get_memory(&writer.value, rail).await.expect("get");
    assert_eq!(own.load_warning, Some(LOAD_WARNING));
    let mut warned = Vec::new();
    while let Some(served) = app.review_next(&dreamer.value).await.expect("review") {
        warned.push((served.memory.kind, served.load_warning));
        app.post_verdict(
            &dreamer.value,
            served.memory.memory_id,
            Verdict::Approve,
            None,
        )
        .await
        .expect("verdict");
    }
    warned.sort_by_key(|(kind, _)| kind.as_str());
    assert_eq!(
        warned,
        vec![
            (MemoryKind::Guardrail, Some(LOAD_WARNING)),
            (MemoryKind::Memory, None)
        ]
    );
    let promoted_row = app.get_memory(&writer.value, rail).await.expect("get");
    assert_eq!(
        promoted_row.load_warning, None,
        "a promoted rule is in force"
    );
}

#[tokio::test]
async fn oml_write_supersession_refuses_a_guardrail_target() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = issue(&app, admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let rail = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g1", Severity::Must, "Run the tests first.", crew),
    )
    .await;
    let err = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                refuse_vocabulary_target: true,
                severity: Some(Severity::Must),
                ..supersede_req("s1", rail, "Run the tests before a push.")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::GuardrailTarget), "{err:?}");
}

#[tokio::test]
async fn a_key_is_issued_with_guardrail_scope_ancestors_unless_the_admin_names_workarea() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let unnamed = reader_scoped(&app, admin, "rhea", crew, None).await;
    let named = reader_scoped(&app, admin, "rhea-2", crew, Some(GuardrailScope::Workarea)).await;
    assert_eq!(unnamed.guardrail_scope, GuardrailScope::Ancestors);
    assert_eq!(named.guardrail_scope, GuardrailScope::Workarea);
    assert_eq!(boot.admin_key.guardrail_scope, GuardrailScope::Ancestors);
    for (issued, scope) in [
        (&unnamed, GuardrailScope::Ancestors),
        (&named, GuardrailScope::Workarea),
    ] {
        let detail = app
            .key(admin, issued.key_id.into())
            .await
            .expect("the key read");
        assert_eq!(detail.guardrail_scope, scope);
        assert_eq!(detail.key.guardrail_scope, scope);
    }
}

#[tokio::test]
async fn key_issued_journals_the_guardrail_scope() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let unnamed = reader_scoped(&app, admin, "rhea", crew, None).await;
    let named = reader_scoped(&app, admin, "rhea-2", crew, Some(GuardrailScope::Workarea)).await;
    let events = journal(&store, boot.tenant_id).await;
    let journalled = |key_id: Uuid| {
        events
            .iter()
            .find(|e| {
                e.envelope.event_type == event_types::KEY_ISSUED
                    && e.envelope.payload["key_id"] == key_id.to_string()
            })
            .map(|e| e.envelope.payload["guardrail_scope"].clone())
    };
    assert_eq!(journalled(unnamed.key_id), Some(json!("ancestors")));
    assert_eq!(journalled(named.key_id), Some(json!("workarea")));
    assert_eq!(journalled(boot.admin_key.key_id), Some(json!("ancestors")));
}

/// A `key.issued` journalled before the build carries no `guardrail_scope`:
/// its key holds the issuance default, and nothing in the journal is
/// rewritten (proposed ADR-0037 d4).
#[tokio::test]
async fn a_key_issued_without_guardrail_scope_folds_as_ancestors() {
    let tenant = Uuid::from_u128(0xA700);
    let key_id = Uuid::from_u128(0xD700);
    let mut poc = PocChain::new(tenant);
    poc.push(event_types::TENANT_GENESIS, json!({ "tenant_name": "oml" }))
        .workarea(Uuid::from_u128(0xB700), None, "root", Value::Null)
        .key(key_id, "writer", "crew/agent-7");
    assert!(
        poc.events[2]
            .envelope
            .payload
            .get("guardrail_scope")
            .is_none(),
        "the issuance names no scope"
    );
    let store = MemStore::new();
    store
        .migrated_poc_chain(poc.events.clone())
        .expect("the PoC chain as 0006 leaves it");
    let key = store
        .key(tenant, key_id)
        .await
        .expect("read")
        .expect("the key");
    assert_eq!(key.guardrail_scope, GuardrailScope::Ancestors);
    assert_eq!(hashes(&journal(&store, tenant).await), hashes(&poc.events));
}

// --- S2: the read ---------------------------------------------------------

/// A tenant with `crew` and `crew.team`, a writer on both and a dreamer:
/// (boot, crew, team, writer, dreamer).
async fn guardrail_tenant(app: &TestApp) -> (BootstrapResult, Uuid, Uuid, IssuedKey, IssuedKey) {
    let (boot, crew) = tenant(app).await;
    let admin = boot.admin_key.value.clone();
    let team = workarea(app, &admin, "team", crew).await;
    let writer = issue(app, &admin, Role::Writer, "wanda", crew).await;
    let dreamer = issue(app, &admin, Role::Dreamer, "dana", crew).await;
    (boot, crew, team, writer, dreamer)
}

#[tokio::test]
async fn a_held_guardrail_is_never_served_as_a_rule_even_inside_an_in_progress_workarea() {
    let app = app();
    let (boot, crew, _team, writer, _dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    app.submit_memory(
        &writer.value,
        &guardrail_req("g1", Severity::Never, "Never push to main.", crew),
    )
    .await
    .expect("submit");
    // The author holds reader on the node too, so it loads the set there.
    let author = agent(&app, admin, "wanda-agent", crew).await;
    app.submit_memory(
        &author.value,
        &guardrail_req("g2", Severity::Must, "Run the tests first.", crew),
    )
    .await
    .expect("submit");
    let set = guardrail_set(&app, &author.value).await;
    assert!(
        set.guardrails.is_empty(),
        "{:?}",
        served_guardrail_ids(&set)
    );
    assert_eq!(set.own_held.len(), 1, "its own held row, listed");
    assert_eq!(set.own_held[0].claim_key, "claim-g2");
    assert_eq!(set.own_held[0].version, 1);
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    let set = guardrail_set(&app, &reader.value).await;
    assert!(set.guardrails.is_empty() && set.own_held.is_empty());
}

#[tokio::test]
async fn recall_never_serves_kind_guardrail() {
    let app = app();
    let (boot, crew, _team, writer, dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    let rail = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g1", Severity::Must, "Rotate the canary weekly.", crew),
    )
    .await;
    let author = agent(&app, admin, "wanda-agent", crew).await;
    app.submit_memory(
        &author.value,
        &guardrail_req("g2", Severity::Must, "Rotate the canary daily.", crew),
    )
    .await
    .expect("submit");
    let recall = app
        .recall(
            &author.value,
            &RecallRequest {
                include_unreviewed: true,
                ..recall_req("canary")
            },
        )
        .await
        .expect("recall");
    assert!(recall.hits.is_empty(), "{:?}", hit_ids_of(&recall));
    assert!(recall.steering.is_empty());
    assert_eq!(
        served_guardrail_ids(&guardrail_set(&app, &author.value).await),
        vec![rail]
    );
}

#[tokio::test]
async fn an_ancestor_guardrail_is_served_only_to_a_key_whose_reader_access_covers_that_ancestor() {
    let app = app();
    let (boot, crew, team, writer, dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    let parent = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g1", Severity::Must, "Run the tests first.", crew),
    )
    .await;
    let child = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g2", Severity::Must, "Ask before a migration.", team),
    )
    .await;
    let narrow = issue(&app, admin, Role::Reader, "rhea", team).await;
    assert_eq!(
        served_guardrail_ids(&guardrail_set(&app, &narrow.value).await),
        vec![child]
    );
    let wide = issue_with(
        &app,
        admin,
        Role::Reader,
        "rhea-wide",
        vec![grant(team, Role::Reader), grant(crew, Role::Reader)],
    )
    .await;
    assert_eq!(
        served_guardrail_ids(&guardrail_set(&app, &wide.value).await),
        vec![parent, child],
        "depth from the root orders one severity"
    );
}

#[tokio::test]
async fn guardrail_scope_workarea_serves_the_scope_node_alone() {
    let app = app();
    let (boot, crew, team, writer, dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g1", Severity::Never, "Never push to main.", crew),
    )
    .await;
    let child = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g2", Severity::Must, "Ask before a migration.", team),
    )
    .await;
    let key = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                guardrail_scope: Some(GuardrailScope::Workarea),
                default_workarea: Some(team.into()),
                ..issue_req(Role::Reader, "rhea", vec![grant(crew, Role::Reader)])
            },
        )
        .await
        .expect("issue key");
    let set = guardrail_set(&app, &key.value).await;
    assert_eq!(set.scope_node, team);
    assert_eq!(set.guardrail_scope, GuardrailScope::Workarea);
    assert_eq!(served_guardrail_ids(&set), vec![child]);
}

#[tokio::test]
async fn no_parameter_or_header_changes_the_guardrail_scope() {
    let app = app();
    let (boot, crew, team, writer, dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    let parent = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g1", Severity::Never, "Never push to main.", crew),
    )
    .await;
    let child = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g2", Severity::Must, "Ask before a migration.", team),
    )
    .await;
    let narrow = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                guardrail_scope: Some(GuardrailScope::Workarea),
                ..issue_req(Role::Reader, "rhea", vec![grant(crew, Role::Reader)])
            },
        )
        .await
        .expect("issue key");
    // X-OML-Workarea picks the node within the grants, never the scope.
    for node in [crew, team] {
        let set = app
            .guardrails(
                &narrow.value,
                &GuardrailRequest {
                    workarea: Some(node.into()),
                },
            )
            .await
            .expect("read");
        assert_eq!(set.guardrail_scope, GuardrailScope::Workarea);
        assert_eq!(set.scope_node, node);
        let expected = if node == crew { parent } else { child };
        assert_eq!(served_guardrail_ids(&set), vec![expected]);
    }
}

#[tokio::test]
async fn the_set_orders_by_severity_then_depth_then_promoted_at_then_artifact_id() {
    let app = app();
    let (boot, crew, team, writer, dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    let (w, d) = (&writer.value, &dreamer.value);
    let req = |idem, severity, node| guardrail_req(idem, severity, "A rule.", node);
    let should_root = promoted(&app, w, d, &req("g1", Severity::Should, crew)).await;
    let must_child = promoted(&app, w, d, &req("g2", Severity::Must, team)).await;
    let must_root_a = promoted(&app, w, d, &req("g3", Severity::Must, crew)).await;
    let never_child = promoted(&app, w, d, &req("g4", Severity::Never, team)).await;
    let must_root_b = promoted(&app, w, d, &req("g5", Severity::Must, crew)).await;
    let reader = issue_with(
        &app,
        admin,
        Role::Reader,
        "rhea",
        vec![grant(team, Role::Reader), grant(crew, Role::Reader)],
    )
    .await;
    let set = guardrail_set(&app, &reader.value).await;
    assert_eq!(
        served_guardrail_ids(&set),
        vec![
            never_child,
            must_root_a,
            must_root_b,
            must_child,
            should_root
        ],
        "severity, then depth, then promoted_at, then artifact_id"
    );
    let depths: Vec<u32> = set.guardrails.iter().map(|g| g.depth).collect();
    assert_eq!(depths, vec![2, 1, 1, 2, 1]);
    assert_eq!(
        set.guardrails[0].workarea_path.as_deref(),
        Some("universe.crew.team")
    );
    assert_eq!(set.guardrails[0].verdicts.len(), 1);
    assert_eq!(set.guardrails[0].verdicts[0].held_by, HeldBy::Agent);
    assert_eq!(
        set.guardrails[0].tokens,
        budget_tokens("A rule.".len() as u64)
    );
}

#[tokio::test]
async fn rows_of_one_claim_key_with_different_severities_each_name_the_others_in_conflict() {
    let app = app();
    let (boot, crew, team, writer, dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    let topic = |severity, node, idem: &str| SubmitRequest {
        claim_key: Some("push-policy".to_owned()),
        ..guardrail_req(idem, severity, "Pushing to main.", node)
    };
    let root = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &topic(Severity::Must, crew, "g1"),
    )
    .await;
    let child = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &topic(Severity::Never, team, "g2"),
    )
    .await;
    let same = promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &SubmitRequest {
            claim_key: Some("tests".to_owned()),
            ..guardrail_req("g3", Severity::Must, "Run the tests.", crew)
        },
    )
    .await;
    let reader = issue_with(
        &app,
        admin,
        Role::Reader,
        "rhea",
        vec![grant(team, Role::Reader), grant(crew, Role::Reader)],
    )
    .await;
    let set = guardrail_set(&app, &reader.value).await;
    let conflict = |id: Uuid| {
        set.guardrails
            .iter()
            .find(|g| g.artifact_id == id)
            .expect("served")
            .conflict
            .clone()
    };
    assert_eq!(conflict(child), vec![root]);
    assert_eq!(conflict(root), vec![child]);
    assert!(conflict(same).is_empty());
}

#[tokio::test]
async fn a_set_over_twelve_is_served_whole_with_over_cap_true() {
    let app = app();
    let (boot, crew, _team, writer, dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    for i in 0..SET_CAP {
        promoted(
            &app,
            &writer.value,
            &dreamer.value,
            &guardrail_req(&format!("g{i}"), Severity::Must, "A rule.", crew),
        )
        .await;
    }
    let set = guardrail_set(&app, &reader.value).await;
    assert_eq!(set.guardrails.len(), SET_CAP);
    assert!(!set.over_cap, "twelve is the cap, not over it");
    promoted(
        &app,
        &writer.value,
        &dreamer.value,
        &guardrail_req("g-over", Severity::Must, "A rule.", crew),
    )
    .await;
    let set = guardrail_set(&app, &reader.value).await;
    assert_eq!(set.guardrails.len(), SET_CAP + 1, "served whole");
    assert!(set.over_cap);
}

#[tokio::test]
async fn the_read_logs_the_served_ids() {
    let app = app();
    let (boot, crew, team, writer, dreamer) = guardrail_tenant(&app).await;
    let admin = &boot.admin_key.value;
    for (idem, node) in [("g1", crew), ("g2", team)] {
        promoted(
            &app,
            &writer.value,
            &dreamer.value,
            &guardrail_req(idem, Severity::Must, "A rule.", node),
        )
        .await;
    }
    let reader = issue(&app, admin, Role::Reader, "rhea", team).await;
    let set = guardrail_set(&app, &reader.value).await;
    assert_eq!(set.served_ids, served_guardrail_ids(&set));
    assert_eq!(set.served_ids.len(), 1);
    let versions: Vec<(Uuid, u32)> = set
        .guardrails
        .iter()
        .map(|g| (g.artifact_id, g.version))
        .collect();
    assert_eq!(
        set.set_digest,
        oml_app::domain::guardrail::set_digest(&versions)
    );
}

// -- the status ladder: proposing a forward transition (ADR-0009 d3, d4;
// ADR-0010) ----------------------------------------------------------------

/// Ratifies `proposal` by `ratifier` straight through the store, sealing the
/// `workarea.status_transition` on the journal's head with the test key: the
/// ratify use case is a later step, and these tests need a node moved up the
/// ladder to propose on.
async fn ratify_in_store(
    store: &MemStore,
    proposal: &oml_app::types::TransitionRecord,
    ratifier: Uuid,
) {
    let head = journal(store, proposal.tenant_id)
        .await
        .pop()
        .expect("a chain to ratify on");
    let draft = EventDraft {
        tenant_id: proposal.tenant_id,
        event_id: Uuid::from_u128(0xF000_0000 + u128::from(head.envelope.seq)),
        event_type: event_types::WORKAREA_STATUS_TRANSITION.to_owned(),
        principal: "fixture-ratifier".to_owned(),
        api_key_id: Some(ratifier),
        at: head.envelope.at.clone(),
        payload: json!({
            "workarea_id": proposal.workarea_id.to_string(),
            "to": proposal.to.as_str(),
            "transition_id": proposal.transition_id.to_string(),
            "ratified_by_key_id": ratifier.to_string(),
        }),
    };
    let envelope = next_envelope(Some(&head), draft).expect("a lawful draft");
    let event = sign(envelope, &SigningKey::from_bytes(&TEST_SEED)).expect("signing");
    store
        .commit(oml_app::ports::CommitRequest {
            outbox: oml_app::ports::OutboxRow {
                event_id: event.envelope.event_id,
                topic: event.envelope.event_type.clone(),
                payload: json!({ "seq": event.envelope.seq }),
            },
            event,
            fold: oml_app::ports::MetadataFold::TransitionRatified {
                transition_id: proposal.transition_id,
                workarea_id: proposal.workarea_id,
                to: proposal.to,
                ratifier_key_id: ratifier,
            },
            idempotency: None,
        })
        .await
        .expect("the ratification commits");
}

/// Moves `node` of `tenant` from its status to `to` one rung at a time: the
/// admin proposes, a second identity ratifies.
async fn climb(
    app: &TestApp,
    store: &MemStore,
    (tenant, admin, ratifier): (Uuid, &str, Uuid),
    node: Uuid,
    to: WorkareaStatus,
) {
    let current = store
        .workarea(tenant, node)
        .await
        .expect("read")
        .expect("the node")
        .status;
    for rung in [WorkareaStatus::Committed, WorkareaStatus::Validated] {
        if rung <= current || rung > to {
            continue;
        }
        let proposal = app
            .propose_transition(admin, node.into(), rung)
            .await
            .expect("a forward proposal");
        ratify_in_store(store, &proposal, ratifier).await;
    }
}

/// An admin proposes on its key role: the proposal is a pending row naming
/// the admin as proposer, the node keeps its status until a second identity
/// ratifies, and exactly one `workarea.status_proposed` is journalled with
/// the event catalog's payload (ADR-0009 d3; event-catalog.md).
#[tokio::test]
async fn an_admin_proposes_development_to_committed_and_it_stays_pending_with_one_status_proposed_event()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let t = boot.tenant_id;
    let before = journal(&store, t).await.len();

    let proposal = app
        .propose_transition(
            &boot.admin_key.value,
            by_name("crew"),
            WorkareaStatus::Committed,
        )
        .await
        .expect("an admin proposes");
    assert_eq!(
        (
            proposal.workarea_id,
            proposal.from,
            proposal.to,
            proposal.state,
            proposal.proposer_key_id,
            proposal.proposer_principal.as_str(),
            proposal.ratifier_key_id,
        ),
        (
            crew,
            WorkareaStatus::Development,
            WorkareaStatus::Committed,
            oml_app::types::TransitionState::Pending,
            boot.admin_key.key_id,
            "alice-admin",
            None,
        )
    );
    let node = store.workarea(t, crew).await.expect("read").expect("node");
    assert_eq!(node.status, WorkareaStatus::Development, "it stays pending");
    assert_eq!(
        store.pending_transitions(t, crew).await.expect("read"),
        vec![proposal.clone()]
    );

    let events = journal(&store, t).await;
    assert_eq!(events.len(), before + 1);
    let event = events.last().expect("the proposal event");
    assert_eq!(
        event.envelope.event_type,
        event_types::WORKAREA_STATUS_PROPOSED
    );
    assert_eq!(event.envelope.api_key_id, Some(boot.admin_key.key_id));
    assert_eq!(
        event.envelope.payload,
        json!({
            "workarea_id": crew.to_string(),
            "from": "development",
            "to": "committed",
            "transition_id": proposal.transition_id.to_string(),
            "proposed_by_key_id": boot.admin_key.key_id.to_string(),
        })
    );
}

/// A dreamer key passes the verb ceiling and proposes where its effective
/// access holds dreamer, here inherited from a grant on the parent
/// (ADR-0009 d3; ADR-0010; roles-and-keys.md refusal order).
#[tokio::test]
async fn a_dreamer_with_dreamer_access_on_the_node_proposes_a_transition() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quarter = workarea(&app, admin, "q1-2027", crew).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dora", crew).await;

    let proposal = app
        .propose_transition(&dreamer.value, quarter.into(), WorkareaStatus::Committed)
        .await
        .expect("a dreamer proposes");
    assert_eq!(proposal.proposer_key_id, dreamer.key_id);
    assert_eq!(proposal.proposer_principal, "dora");
    assert_eq!(proposal.workarea_id, quarter);
    assert_eq!(
        store
            .pending_transitions(boot.tenant_id, quarter)
            .await
            .expect("read"),
        vec![proposal]
    );
}

/// Step 1 of the refusal order reads no node: a writer or reader key is
/// refused `access.verb_ceiling` whether the node exists or not, before any
/// name resolves, and nothing is journalled (roles-and-keys.md refusal
/// order; ADR-0010).
#[tokio::test]
async fn a_writer_or_reader_key_proposing_a_transition_is_refused_by_the_verb_ceiling_before_any_lookup()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = agent(&app, admin, "wanda", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rita", crew).await;
    let before = journal(&store, boot.tenant_id).await.len();
    let lookups = store.name_lookups();

    for (key, role) in [(&writer, Role::Writer), (&reader, Role::Reader)] {
        for node in [by_name("crew"), by_name("no-such-node")] {
            let err = app
                .propose_transition(&key.value, node, WorkareaStatus::Committed)
                .await
                .unwrap_err();
            assert!(
                matches!(err, AppError::VerbCeiling { role: r, .. } if r == role),
                "{err:?}"
            );
        }
    }
    assert_eq!(store.name_lookups(), lookups, "no name resolved");
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// A dreamer key that reads the node but holds no dreamer role on it is
/// refused `access.grant_refused` naming the node and `dreamer`
/// (roles-and-keys.md refusal order, step 3).
#[tokio::test]
async fn a_dreamer_reading_but_not_dreaming_on_the_node_is_refused_with_needed_role_dreamer() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dreamer = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dora",
        vec![grant(crew, Role::Reader)],
    )
    .await;
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .propose_transition(&dreamer.value, by_name("crew"), WorkareaStatus::Committed)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::GrantRefused { workarea_id, needed_role: Role::Dreamer } if workarea_id == crew
        ),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// Outside its read access a dreamer cannot tell the node from absence: a
/// sibling it holds no grant on, and a name no row holds, both answer
/// `access.not_found` (roles-and-keys.md refusal order, step 2).
#[tokio::test]
async fn a_dreamer_outside_read_access_gets_not_found() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let other = workarea(&app, admin, "other", boot.root_workarea_id).await;
    let dreamer = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let before = journal(&store, boot.tenant_id).await.len();

    for node in [other.into(), by_name("other"), by_name("no-such-node")] {
        let err = app
            .propose_transition(&dreamer.value, node, WorkareaStatus::Committed)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// The ladder refuses before anything is sealed: the same rung, a skipped
/// rung, and any status on the universe root each answer
/// `workarea.transition_refused` with its reason, and the journal is
/// unchanged (ADR-0009 d1, d3; errors.md).
#[tokio::test]
async fn proposing_committed_on_a_committed_node_validated_from_development_or_anything_on_universe_is_transition_refused_and_journals_nothing()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let ratifier = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let fresh = workarea(&app, admin, "fresh", boot.root_workarea_id).await;
    climb(
        &app,
        &store,
        (boot.tenant_id, admin, ratifier.key_id),
        crew,
        WorkareaStatus::Committed,
    )
    .await;
    let before = journal(&store, boot.tenant_id).await.len();

    let root = boot.root_workarea_id;
    let cases = [
        (
            crew,
            WorkareaStatus::Committed,
            WorkareaStatus::Committed,
            LadderRefusal::Same,
        ),
        (
            fresh,
            WorkareaStatus::Development,
            WorkareaStatus::Validated,
            LadderRefusal::Skipped,
        ),
        (
            root,
            WorkareaStatus::Committed,
            WorkareaStatus::Development,
            LadderRefusal::Universe,
        ),
        (
            root,
            WorkareaStatus::Committed,
            WorkareaStatus::Committed,
            LadderRefusal::Universe,
        ),
        (
            root,
            WorkareaStatus::Committed,
            WorkareaStatus::Validated,
            LadderRefusal::Universe,
        ),
    ];
    for (node, from, to, reason) in cases {
        let err = app
            .propose_transition(admin, node.into(), to)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                AppError::TransitionRefused { workarea_id, from: f, to: t, reason: r }
                    if workarea_id == node && f == from && t == to && r == reason
            ),
            "{node} {to:?}: {err:?}"
        );
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// A step to `validated` needs every child `validated`: with a child
/// `committed` the proposal is refused `children_not_validated` and
/// journals nothing (ADR-0009 d4).
#[tokio::test]
async fn validated_is_refused_while_a_child_is_committed() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quarter = workarea(&app, admin, "q1-2027", crew).await;
    let ratifier = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    for node in [crew, quarter] {
        climb(
            &app,
            &store,
            (boot.tenant_id, admin, ratifier.key_id),
            node,
            WorkareaStatus::Committed,
        )
        .await;
    }
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Validated)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::TransitionRefused {
                workarea_id,
                from: WorkareaStatus::Committed,
                to: WorkareaStatus::Validated,
                reason: LadderRefusal::ChildrenNotValidated,
            } if workarea_id == crew
        ),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);

    // With the child validated, the same proposal is admitted.
    climb(
        &app,
        &store,
        (boot.tenant_id, admin, ratifier.key_id),
        quarter,
        WorkareaStatus::Validated,
    )
    .await;
    app.propose_transition(admin, crew.into(), WorkareaStatus::Validated)
        .await
        .expect("every child validated");
}

// -- the status ladder: ratifying a transition by a second identity
// (ADR-0009 d3, d4; ADR-0010 R2) ------------------------------------------

/// A dreamer key, held by a service, on `node`.
async fn service_dreamer(app: &TestApp, admin: &str, principal: &str, node: Uuid) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            held_by: Some(HeldBy::Service),
            ..issue_req(Role::Dreamer, principal, vec![grant(node, Role::Dreamer)])
        },
    )
    .await
    .expect("issue a service key")
}

/// Asserts `err` is `workarea.transition_refused` on `node` with `reason`.
fn assert_refused(err: &AppError, node: Uuid, reason: LadderRefusal) {
    assert!(
        matches!(
            err,
            AppError::TransitionRefused { workarea_id, reason: r, .. }
                if *workarea_id == node && *r == reason
        ),
        "{err:?}"
    );
}

/// A dreamer other than the proposer ratifies: the node's status becomes
/// `to`, and exactly one `workarea.status_transition` is journalled, signed
/// by the ratifier's principal and key, carrying the catalog's payload with
/// the policy in force; a step to `validated` adds the precondition it
/// evaluated, `children_all_validated` (ADR-0009 d3, d4; event-catalog.md).
#[tokio::test]
async fn a_second_dreamer_ratifies_and_the_workarea_becomes_committed_with_the_policy_snapshot_journalled()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let t = boot.tenant_id;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;

    let proposal = app
        .propose_transition(admin, by_name("crew"), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    let before = journal(&store, t).await.len();
    let record = app
        .ratify_transition(&dora.value, by_name("crew"), proposal.transition_id)
        .await
        .expect("a second identity ratifies");
    assert_eq!(
        (record.workarea_id, record.status),
        (crew, WorkareaStatus::Committed)
    );
    let node = store.workarea(t, crew).await.expect("read").expect("node");
    assert_eq!(node.status, WorkareaStatus::Committed);
    assert!(
        store
            .pending_transitions(t, crew)
            .await
            .expect("read")
            .is_empty()
    );

    let events = journal(&store, t).await;
    assert_eq!(events.len(), before + 1);
    let event = events.last().expect("the ratification");
    assert_eq!(
        event.envelope.event_type,
        event_types::WORKAREA_STATUS_TRANSITION
    );
    assert_eq!(event.envelope.principal, "dora");
    assert_eq!(event.envelope.api_key_id, Some(dora.key_id));
    assert_eq!(
        event.envelope.payload,
        json!({
            "workarea_id": crew.to_string(),
            "from": "development",
            "to": "committed",
            "transition_id": proposal.transition_id.to_string(),
            "proposed_by_key_id": boot.admin_key.key_id.to_string(),
            "ratified_by_key_id": dora.key_id.to_string(),
            "policy_snapshot": { "k": 1, "required_roles": ["dreamer"] },
        })
    );

    // The step to validated snapshots the precondition it evaluated.
    let proposal = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Validated)
        .await
        .expect("a proposal");
    app.ratify_transition(&dora.value, crew.into(), proposal.transition_id)
        .await
        .expect("ratified");
    let event = journal(&store, t).await.pop().expect("the ratification");
    assert_eq!(
        event.envelope.payload["policy_snapshot"],
        json!({ "k": 1, "required_roles": ["dreamer"], "children_all_validated": true })
    );
}

/// R2: the key that proposed cannot ratify, and nothing is journalled
/// (ADR-0009 d3; errors.md `review.ratifier_is_proposer`).
#[tokio::test]
async fn the_proposing_key_cannot_ratify_its_own_transition() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let dora = issue(&app, &boot.admin_key.value, Role::Dreamer, "dora", crew).await;
    let proposal = app
        .propose_transition(&dora.value, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .ratify_transition(&dora.value, crew.into(), proposal.transition_id)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::RatifierIsProposer), "{err:?}");
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let node = store.workarea(boot.tenant_id, crew).await.expect("read");
    assert_eq!(node.expect("node").status, WorkareaStatus::Development);
}

/// R2 compares principals too: a second key of the proposing principal is
/// the same identity (ADR-0009 d3; ADR-0010 R2).
#[tokio::test]
async fn a_second_key_of_the_proposing_principal_cannot_ratify() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let dora_again = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let proposal = app
        .propose_transition(&dora.value, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .ratify_transition(&dora_again.value, crew.into(), proposal.transition_id)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::RatifierIsProposer), "{err:?}");
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// A `held_by: service` key, and a reader or writer key, are refused
/// `access.verb_ceiling` before any name resolves or any transition is
/// looked up (roles-and-keys.md refusal order; ADR-0010).
#[tokio::test]
async fn a_service_key_is_refused_ratification_by_the_verb_ceiling_before_any_lookup() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let service = service_dreamer(&app, admin, "reconciler", crew).await;
    let writer = agent(&app, admin, "wanda", crew).await;
    let reader = issue(&app, admin, Role::Reader, "rita", crew).await;
    let proposal = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    let before = journal(&store, boot.tenant_id).await.len();
    let lookups = store.name_lookups();

    for (key, role) in [
        (&service, Role::Dreamer),
        (&writer, Role::Writer),
        (&reader, Role::Reader),
    ] {
        for (node, id) in [
            (by_name("crew"), proposal.transition_id),
            (by_name("no-such-node"), Uuid::from_u128(0xDEAD)),
        ] {
            let err = app
                .ratify_transition(&key.value, node, id)
                .await
                .unwrap_err();
            assert!(
                matches!(err, AppError::VerbCeiling { role: r, .. } if r == role),
                "{err:?}"
            );
        }
    }
    assert_eq!(store.name_lookups(), lookups, "no name resolved");
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// Ratification acts on the node as it is: a child created after a
/// proposal to `validated` makes its ratification `transition_refused`
/// with reason `children_not_validated`, journalling nothing (ADR-0009 d4;
/// workareas.md, ratification re-checks).
#[tokio::test]
async fn a_child_created_after_the_validated_proposal_makes_its_ratification_transition_refused() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    climb(
        &app,
        &store,
        (boot.tenant_id, admin, dora.key_id),
        crew,
        WorkareaStatus::Committed,
    )
    .await;
    let proposal = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Validated)
        .await
        .expect("no child yet");
    workarea(&app, admin, "late-child", crew).await;
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .ratify_transition(&dora.value, crew.into(), proposal.transition_id)
        .await
        .unwrap_err();
    assert_refused(&err, crew, LadderRefusal::ChildrenNotValidated);
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let node = store.workarea(boot.tenant_id, crew).await.expect("read");
    assert_eq!(node.expect("node").status, WorkareaStatus::Committed);
}

/// A ratified proposal is no longer live: a second ratification is
/// `transition_refused` with reason `stale`, and journals nothing
/// (workareas.md, ratification re-checks).
#[tokio::test]
async fn a_transition_ratified_once_is_refused_as_stale_the_second_time() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let dan = issue(&app, admin, Role::Dreamer, "dan", crew).await;
    let proposal = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    app.ratify_transition(&dora.value, crew.into(), proposal.transition_id)
        .await
        .expect("the first ratification");
    let before = journal(&store, boot.tenant_id).await.len();

    for key in [&dora, &dan] {
        let err = app
            .ratify_transition(&key.value, crew.into(), proposal.transition_id)
            .await
            .unwrap_err();
        assert_refused(&err, crew, LadderRefusal::Stale);
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// A transition is addressed under its node: an id that names another
/// workarea's proposal, and an id no row holds, both answer
/// `access.not_found` (roles-and-keys.md refusal order).
#[tokio::test]
async fn a_transition_id_of_another_workarea_is_not_found() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quarter = workarea(&app, admin, "q1-2027", crew).await;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let proposal = app
        .propose_transition(admin, quarter.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal on the child");
    let before = journal(&store, boot.tenant_id).await.len();

    for id in [proposal.transition_id, Uuid::from_u128(0xDEAD)] {
        let err = app
            .ratify_transition(&dora.value, crew.into(), id)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let node = store.workarea(boot.tenant_id, quarter).await.expect("read");
    assert_eq!(node.expect("node").status, WorkareaStatus::Development);
}

/// The ladder climbs development → committed → validated through the two
/// verbs, and from `validated` every proposal is refused, backward or same:
/// no verb moves a workarea down (ADR-0009 d3, d5).
#[tokio::test]
async fn no_verb_moves_a_workarea_down_the_ladder() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    for rung in [WorkareaStatus::Committed, WorkareaStatus::Validated] {
        let proposal = app
            .propose_transition(admin, crew.into(), rung)
            .await
            .expect("a forward proposal");
        let record = app
            .ratify_transition(&dora.value, crew.into(), proposal.transition_id)
            .await
            .expect("ratified");
        assert_eq!(record.status, rung);
    }
    let before = journal(&store, boot.tenant_id).await.len();

    for (to, reason) in [
        (WorkareaStatus::Development, LadderRefusal::Backward),
        (WorkareaStatus::Committed, LadderRefusal::Backward),
        (WorkareaStatus::Validated, LadderRefusal::Same),
    ] {
        for key in [admin.as_str(), dora.value.as_str()] {
            let err = app
                .propose_transition(key, crew.into(), to)
                .await
                .unwrap_err();
            assert_refused(&err, crew, reason);
        }
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let node = store.workarea(boot.tenant_id, crew).await.expect("read");
    assert_eq!(node.expect("node").status, WorkareaStatus::Validated);
}

/// A tenant whose one principal is its admin cannot freeze an area alone:
/// the admin's own key, a second admin key of the same principal, and a
/// dreamer key it issues to itself are each the proposer under R2
/// (ADR-0009 d3; ADR-0010 R2).
#[tokio::test]
async fn a_one_admin_tenant_cannot_freeze_alone() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let second_admin = issue_with(&app, admin, Role::Admin, "alice-admin", Vec::new()).await;
    let own_dreamer = issue(&app, admin, Role::Dreamer, "alice-admin", crew).await;
    let proposal = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    let before = journal(&store, boot.tenant_id).await.len();

    for key in [
        admin.as_str(),
        second_admin.value.as_str(),
        own_dreamer.value.as_str(),
    ] {
        let err = app
            .ratify_transition(key, crew.into(), proposal.transition_id)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::RatifierIsProposer), "{err:?}");
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    let node = store.workarea(boot.tenant_id, crew).await.expect("read");
    assert_eq!(node.expect("node").status, WorkareaStatus::Development);
}

// -- freeze: a validated area refuses by admission policy, never by grant
// resolution (ADR-0009 d2, d4, d5) ----------------------------------------

/// Moves `node` to `validated` through the real verbs: the admin proposes
/// each rung and `ratifier`, a second identity with dreamer access on the
/// node, ratifies it.
async fn freeze(app: &TestApp, admin: &str, ratifier: &str, node: Uuid) {
    for rung in [WorkareaStatus::Committed, WorkareaStatus::Validated] {
        let proposal = app
            .propose_transition(admin, node.into(), rung)
            .await
            .expect("a forward proposal");
        app.ratify_transition(ratifier, node.into(), proposal.transition_id)
            .await
            .expect("a second identity ratifies");
    }
}

/// Asserts `err` is `workarea.frozen` on `node` for `op`.
fn assert_frozen(err: &AppError, node: Uuid, op: FrozenOp) {
    assert!(
        matches!(
            err,
            AppError::WorkareaFrozen { workarea_id, op: o } if *workarea_id == node && *o == op
        ),
        "{err:?}"
    );
}

/// A writer key holding writer and reader on the root reaches the crew area
/// by inheritance, a second writer holds them on the crew itself with the
/// crew as its default, and each submits with and without the header, the
/// scan's veto marker included: every one is `workarea.frozen`, before the
/// scan, and nothing is journalled or queued (ADR-0009 d5;
/// workareas.md refusal matrix, new claim write).
#[tokio::test]
async fn a_submission_into_a_validated_workarea_is_frozen_for_every_holder_of_write_access_and_journals_nothing()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let on_crew = agent(&app, admin, "crew/agent-7", crew).await;
    let on_root = issue_with(
        &app,
        admin,
        Role::Writer,
        "rooted",
        vec![
            grant(boot.root_workarea_id, Role::Writer),
            grant(boot.root_workarea_id, Role::Reader),
        ],
    )
    .await;
    freeze(&app, admin, &dora.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();
    let outbox = store.outbox_len(boot.tenant_id);

    let attempts = [
        (&on_crew, submit_req("f1", "the canary drains first")),
        (
            &on_crew,
            submit_to("f2", "the canary drains first", by_name("crew")),
        ),
        (
            &on_crew,
            submit_into("f3", "-----BEGIN PRIVATE KEY-----", crew),
        ),
        (&on_root, submit_into("f4", "the canary drains first", crew)),
        (
            &on_root,
            submit_to("f5", "-----BEGIN PRIVATE KEY-----", by_name("crew")),
        ),
    ];
    for (key, req) in &attempts {
        let err = app.submit_memory(&key.value, req).await.unwrap_err();
        assert_frozen(&err, crew, FrozenOp::Write);
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    assert_eq!(store.outbox_len(boot.tenant_id), outbox);
    assert!(
        app.review_next(&dora.value)
            .await
            .expect("review")
            .is_none()
    );
}

/// Effective access is consulted before status: a key the area is outside
/// of is `access.not_found`, one that only reads it is
/// `access.grant_refused`, and a reader key is `access.verb_ceiling`, as
/// they would be on an open area (workareas.md refusal matrix preamble).
#[tokio::test]
async fn a_key_without_access_to_a_validated_workarea_still_gets_its_access_refusal_first() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let elsewhere = workarea(&app, admin, "elsewhere", boot.root_workarea_id).await;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let outsider = agent(&app, admin, "outsider", elsewhere).await;
    let only_reads = issue_with(
        &app,
        admin,
        Role::Writer,
        "only-reads",
        vec![grant(elsewhere, Role::Writer), grant(crew, Role::Reader)],
    )
    .await;
    let reader = issue(&app, admin, Role::Reader, "rhea", crew).await;
    freeze(&app, admin, &dora.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .submit_memory(&outsider.value, &submit_into("a1", "x", crew))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let err = app
        .submit_memory(&only_reads.value, &submit_into("a2", "x", crew))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::GrantRefused { workarea_id, needed_role: Role::Writer } if workarea_id == crew
        ),
        "{err:?}"
    );
    let err = app
        .submit_memory(&reader.value, &submit_into("a3", "x", crew))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::VerbCeiling { .. }), "{err:?}");
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// A supersession, a retirement included, of a promoted tip in an area
/// validated since is `workarea.frozen` at step 5's last check, whether the
/// author or a dreamer proposing an edit sends it, and journals nothing;
/// the target-dependent refusals before it still answer first
/// (roles-and-keys.md supersede, step 5; ADR-0009 d5).
#[tokio::test]
async fn a_supersede_whose_target_lies_in_a_validated_workarea_is_frozen() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let open = workarea(&app, admin, "open", boot.root_workarea_id).await;
    let writer = agent(&app, admin, "crew/agent-7", crew).await;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let editor = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "eddie",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;
    let target = promoted(
        &app,
        &writer.value,
        &dora.value,
        &submit_req("s1", "the canary drains first"),
    )
    .await;
    freeze(&app, admin, &dora.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();

    for (key, req) in [
        (
            &writer,
            supersede_req("s2", target, "the canary drains last"),
        ),
        (&writer, retire_req("s3", target, "no canary any more")),
        (
            &editor,
            supersede_req("s4", target, "the canary drains twice"),
        ),
    ] {
        let err = app.supersede(&key.value, &req).await.unwrap_err();
        assert_frozen(&err, crew, FrozenOp::Write);
    }

    // A body `workarea` other than the target's is answered before freeze.
    let err = app
        .supersede(
            &writer.value,
            &SupersedeRequest {
                workarea: Some(open.into()),
                ..supersede_req("s5", target, "x")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::WorkareaNotTarget { .. }), "{err:?}");
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

/// A row held when its area is validated cannot be approved: the approval
/// is `workarea.frozen` before its verdict is sealed, nothing is journalled,
/// and the row stays held, visible to its author (ADR-0009 d5; workareas.md
/// refusal matrix, promotion).
#[tokio::test]
async fn an_approval_of_a_held_row_in_a_workarea_validated_after_submission_is_frozen_and_the_row_stays_held()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = agent(&app, admin, "crew/agent-7", crew).await;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let id = app
        .submit_memory(&writer.value, &submit_req("h1", "the canary drains first"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    freeze(&app, admin, &dora.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();
    let outbox = store.outbox_len(boot.tenant_id);

    let err = app
        .post_verdict(&dora.value, id, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert_frozen(&err, crew, FrozenOp::Promote);
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    assert_eq!(store.outbox_len(boot.tenant_id), outbox);
    let row = store
        .memory(boot.tenant_id, id)
        .await
        .expect("read")
        .expect("the row");
    assert_eq!(row.status, MemoryStatus::Held);
    let own = app.get_memory(&writer.value, id).await.expect("own row");
    assert_eq!(own.memory.status, MemoryStatus::Held);
}

/// No child is created under a validated node, by id or by name, and
/// nothing is journalled; a child under a committed node is still created
/// (ADR-0009 d4; workareas.md child-status rule).
#[tokio::test]
async fn creating_a_child_under_a_validated_workarea_is_frozen() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let committed = workarea(&app, admin, "committed", boot.root_workarea_id).await;
    let proposal = app
        .propose_transition(admin, committed.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    let ratifier = issue(&app, admin, Role::Dreamer, "rita", committed).await;
    app.ratify_transition(&ratifier.value, committed.into(), proposal.transition_id)
        .await
        .expect("ratified");
    freeze(&app, admin, &dora.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();

    for parent in [Reference::from(crew), by_name("crew")] {
        let err = app
            .create_workarea(admin, &name("late-child"), Some(parent), None)
            .await
            .unwrap_err();
        assert_frozen(&err, crew, FrozenOp::CreateChild);
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
    workarea(&app, admin, "child", committed).await;
}

/// The crew-metadata update is one of the three mutations a validated area
/// admits: the workarea is pointed at a crew record, and that record's
/// charter is corrected, each journalled (ADR-0009 d5; workareas.md crew
/// records).
#[tokio::test]
async fn a_validated_workareas_crew_record_can_still_be_updated() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let record = metals_crew(&app, admin).await;
    freeze(&app, admin, &dora.value, crew).await;
    let before = journal(&store, boot.tenant_id).await.len();

    let pointed = app
        .set_workarea_crew(admin, crew.into(), by_name("metals-crew"))
        .await
        .expect("a frozen area's crew is corrected");
    assert_eq!(pointed.crew_id, Some(record.crew_id));
    assert_eq!(pointed.status, WorkareaStatus::Validated);
    let updated = app
        .update_crew(
            admin,
            by_name("metals-crew"),
            &CrewUpdate {
                charter_uri: Some("https://crews.example.com/metals/v2.md".to_owned()),
                external_manager: None,
            },
        )
        .await
        .expect("the record stays correctable");
    let info = app
        .workarea_info(admin, Some(crew.into()))
        .await
        .expect("read");
    assert_eq!(info.crew, Some(updated));
    let types: Vec<String> = journal(&store, boot.tenant_id).await[before..]
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect();
    assert_eq!(
        types,
        [
            event_types::WORKAREA_CREW_UPDATED,
            event_types::CREW_UPDATED
        ]
    );
}

/// Freezing is not archival: the recall and the get of a promoted claim
/// answer the same after the area is validated as before (workareas.md,
/// frozen is not read-only-dead).
#[tokio::test]
async fn a_validated_workarea_still_serves_recall_and_get_of_its_promoted_claims() {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = agent(&app, admin, "crew/agent-7", crew).await;
    let rhea = issue(&app, admin, Role::Reader, "rhea", crew).await;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let id = promoted(
        &app,
        &writer.value,
        &dora.value,
        &submit_req("r1", "the canary drains first"),
    )
    .await;
    let read = || async {
        let mut answers = Vec::new();
        for key in [&writer, &rhea] {
            let got = app.get_memory(&key.value, id).await.expect("get");
            let hits = hit_ids(&app, &key.value, "canary").await;
            answers.push((got.memory, hits));
        }
        answers
    };
    let open = read().await;
    assert!(open.iter().all(|(_, hits)| hits == &vec![id]));

    freeze(&app, admin, &dora.value, crew).await;
    assert_eq!(read().await, open);
}

/// Freezing journals only its transitions: no grant is issued, edited or
/// denied, and every key's permission epoch stays where it was
/// (ADR-0009 d2; workareas.md permission epoch).
#[tokio::test]
async fn freezing_changes_no_grant_and_no_permission_epoch() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let writer = agent(&app, admin, "crew/agent-7", crew).await;
    let dora = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dora",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;
    let access = || async {
        let mut seen = Vec::new();
        for key in [&writer, &dora] {
            let detail = app.key(admin, key.key_id.into()).await.expect("key read");
            let epoch = app
                .workarea_info(&key.value, Some(crew.into()))
                .await
                .expect("a readable node")
                .permission_epoch;
            seen.push((detail.access, epoch));
        }
        seen
    };
    let before = access().await;
    let seq = journal(&store, boot.tenant_id).await.len();

    freeze(&app, admin, &dora.value, crew).await;
    assert_eq!(access().await, before);
    let types: Vec<String> = journal(&store, boot.tenant_id).await[seq..]
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect();
    assert!(
        types
            .iter()
            .all(|t| t == event_types::WORKAREA_STATUS_PROPOSED
                || t == event_types::WORKAREA_STATUS_TRANSITION),
        "{types:?}"
    );
}

// -- the status read, and status events on the tail (ADR-0009 d3; ADR-0025
// d15) --------------------------------------------------------------------

/// A stored proposal as the status read serves it to a key outside the
/// proposer's own key set: the proposing key withheld (ADR-0025 d15).
fn seen_by_another(t: &oml_app::types::TransitionRecord) -> TransitionView {
    TransitionView {
        transition_id: t.transition_id,
        workarea_id: t.workarea_id,
        from: t.from,
        to: t.to,
        state: t.state,
        proposed_by_key_id: None,
    }
}

/// The status read serves the node's stored status, the review policy in
/// force and its live proposals in proposal order; a ratification makes
/// every other proposal from the old rung stale, and the read drops them
/// (http-api.md `GET /v2/workareas/{id}/status`; workareas.md, the status
/// read lists only live proposals; ADR-0009 d3).
#[tokio::test]
async fn the_status_read_lists_pending_transitions_in_proposal_order_and_the_policy_in_force() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let rita = issue(&app, admin, Role::Reader, "rita", crew).await;

    let first = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    let second = app
        .propose_transition(&dora.value, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a second proposal");
    let third = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a third proposal");

    let view = app
        .workarea_status(&rita.value, by_name("crew"))
        .await
        .expect("a reader reads the status");
    assert_eq!(view.workarea_id, crew);
    assert_eq!(view.status, WorkareaStatus::Development);
    assert_eq!(
        view.policy,
        json!({ "k": 1, "required_roles": ["dreamer"] })
    );
    assert_eq!(
        view.pending,
        [&first, &second, &third].map(seen_by_another).to_vec()
    );

    app.ratify_transition(&dora.value, crew.into(), first.transition_id)
        .await
        .expect("a second identity ratifies");
    let view = app
        .workarea_status(&rita.value, crew.into())
        .await
        .expect("a reader reads the status");
    assert_eq!(view.status, WorkareaStatus::Committed);
    assert!(view.pending.is_empty(), "stale proposals are not live");
    let stored = store
        .pending_transitions(boot.tenant_id, crew)
        .await
        .expect("read");
    assert_eq!(stored, vec![second, third], "the stale rows stay pending");

    let next = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Validated)
        .await
        .expect("a proposal from the new rung");
    let view = app
        .workarea_status(&rita.value, crew.into())
        .await
        .expect("a reader reads the status");
    assert_eq!(view.pending, vec![seen_by_another(&next)]);
}

/// An admin key reads a node's pending transitions as structure, on its key
/// role, with no grant anywhere (roles-and-keys.md permission matrix,
/// footnote 6; ADR-0026 d15).
#[tokio::test]
async fn an_admin_reads_a_nodes_pending_transitions_without_any_grant() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let quarter = workarea(&app, admin, "q1-2027", crew).await;
    let dora = issue(&app, admin, Role::Dreamer, "dora", quarter).await;
    let bob = issue_with(&app, admin, Role::Admin, "bob-admin", Vec::new()).await;
    let proposal = app
        .propose_transition(&dora.value, quarter.into(), WorkareaStatus::Committed)
        .await
        .expect("a dreamer proposes");

    let view = app
        .workarea_status(&bob.value, by_name("q1-2027"))
        .await
        .expect("an admin reads structure");
    assert_eq!(
        (view.workarea_id, view.status, view.pending),
        (
            quarter,
            WorkareaStatus::Development,
            vec![seen_by_another(&proposal)]
        )
    );
    let err = app
        .workarea_status(&bob.value, Uuid::from_u128(0xDEAD).into())
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
}

/// A key outside read access on the node is refused by the refusal order
/// as `access.not_found`, by id and by name alike, so the read never tells
/// it the node exists (roles-and-keys.md refusal order).
#[tokio::test]
async fn a_key_without_read_access_gets_not_found_from_the_status_read() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let other = workarea(&app, admin, "other", boot.root_workarea_id).await;
    let outsider = issue(&app, admin, Role::Reader, "otto", other).await;
    app.propose_transition(admin, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");

    for node in [crew.into(), by_name("crew"), by_name("no-such-node")] {
        let err = app
            .workarea_status(&outsider.value, node)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
}

/// A status read serves each live proposal as the served `Transition` shape
/// (transition id, node, step, state, proposing key) and never the
/// proposer's principal or the tenant, and it names the proposing key only
/// to a key in the proposer's own key set, as the tail does: a reader of
/// the node, an admin among them, learns that a proposal waits, never which
/// key made it (workareas.md, the status read names the proposing key only
/// to its own key set; http-api.md#events-tail; ADR-0025 d15).
#[tokio::test]
async fn a_status_read_never_names_the_proposers_principal_and_names_the_proposing_key_only_to_that_key()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dora-the-proposer",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;
    let rita = issue(&app, admin, Role::Reader, "rita", crew).await;
    let proposal = app
        .propose_transition(&dora.value, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a dreamer proposes");
    let served = |proposed_by_key_id| TransitionView {
        transition_id: proposal.transition_id,
        workarea_id: crew,
        from: WorkareaStatus::Development,
        to: WorkareaStatus::Committed,
        state: TransitionState::Pending,
        proposed_by_key_id,
    };

    let own = app
        .workarea_status(&dora.value, crew.into())
        .await
        .expect("the proposer reads the node");
    assert_eq!(own.pending, vec![served(Some(dora.key_id))]);

    for reader in [&rita.value, admin] {
        let view = app
            .workarea_status(reader, crew.into())
            .await
            .expect("a reader of the node");
        assert_eq!(view.pending, vec![served(None)]);
        let shown = format!("{view:?}");
        assert!(!shown.contains("dora-the-proposer"), "{shown}");
        assert!(!shown.contains(&boot.tenant_id.to_string()), "{shown}");
        assert!(!shown.contains(&dora.key_id.to_string()), "{shown}");
    }
}

/// The workarea read serves the status the row stores: `validated` once a
/// second identity ratified the last rung, and the status read agrees
/// (ADR-0009 d1, d3; ADR-0025 d14).
#[tokio::test]
async fn the_workarea_read_serves_validated_after_ratification() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "dora",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;

    freeze(&app, admin, &dora.value, crew).await;
    let info = app
        .workarea_info(&dora.value, Some(crew.into()))
        .await
        .expect("a readable node");
    assert_eq!(info.workarea.status, WorkareaStatus::Validated);
    let view = app
        .workarea_status(&dora.value, crew.into())
        .await
        .expect("a readable node");
    assert_eq!(view.status, info.workarea.status);
    assert!(view.pending.is_empty());
}

/// The tail shows `workarea.status_proposed` and
/// `workarea.status_transition` to a key that reads the node, judged on the
/// payload's `workarea_id`, and neither to a key that does not
/// (http-api.md#events-tail; event-catalog.md; ADR-0025 d15).
#[tokio::test]
async fn a_reader_of_the_node_sees_status_proposed_and_status_transition_on_the_tail_and_a_non_reader_sees_neither()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let other = workarea(&app, admin, "other", boot.root_workarea_id).await;
    let rita = issue(&app, admin, Role::Reader, "rita", crew).await;
    let otto = issue(&app, admin, Role::Reader, "otto", other).await;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;
    let proposal = app
        .propose_transition(admin, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("a proposal");
    app.ratify_transition(&dora.value, crew.into(), proposal.transition_id)
        .await
        .expect("ratified");

    let status_events = |page: oml_app::types::TailPage| -> Vec<(String, Value)> {
        page.events
            .into_iter()
            .filter(|e| e.event.envelope.event_type.starts_with("workarea.status_"))
            .map(|e| (e.event.envelope.event_type.clone(), e.payload))
            .collect()
    };
    let seen = status_events(
        app.tail_events(&rita.value, None, 0, 100)
            .await
            .expect("tail"),
    );
    let types: Vec<&str> = seen.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        types,
        [
            event_types::WORKAREA_STATUS_PROPOSED,
            event_types::WORKAREA_STATUS_TRANSITION
        ]
    );
    for (_, payload) in &seen {
        assert_eq!(payload["workarea_id"], json!(crew.to_string()));
        assert_eq!(
            payload["transition_id"],
            json!(proposal.transition_id.to_string())
        );
    }

    let unseen = status_events(
        app.tail_events(&otto.value, None, 0, 100)
            .await
            .expect("tail"),
    );
    assert!(unseen.is_empty(), "{unseen:?}");
}

/// A chain holding status proposals and their ratifications, both rungs,
/// verifies end to end with the domain procedure (chain-format.md; ADR-0009
/// d3).
#[tokio::test]
async fn the_chain_with_transition_events_verifies() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let dora = issue(&app, admin, Role::Dreamer, "dora", crew).await;

    freeze(&app, admin, &dora.value, crew).await;
    let events = journal(&store, boot.tenant_id).await;
    let count = |t: &str| events.iter().filter(|e| e.envelope.event_type == t).count();
    assert_eq!(count(event_types::WORKAREA_STATUS_PROPOSED), 2);
    assert_eq!(count(event_types::WORKAREA_STATUS_TRANSITION), 2);

    let head = app.verify_chain(admin).await.expect("the chain verifies");
    assert_eq!(head.seq, events.last().expect("a chain").envelope.seq);
    let keys = KeySet::new(vec![SigningKey::from_bytes(&TEST_SEED).verifying_key()]);
    verify_chain(&events, &keys, None).expect("the domain procedure verifies it");
}

// ---------------------------------------------------------------------------
// A claim edge proposed between a use-case's reads and its commit
// ---------------------------------------------------------------------------

/// A pending edge `src -> dst` in `tenant`, proposed by `proposer`.
fn pending_edge(tenant: Uuid, proposer: &IssuedKey, src: Uuid, dst: Uuid) -> ClaimEdgeRecord {
    ClaimEdgeRecord {
        edge_id: Uuid::from_u128(0xED6E_0000_0000_0000_0000_0000_0000_0001),
        tenant_id: tenant,
        src_artifact_id: src,
        dst_artifact_id: dst,
        label: "acme:supports".to_owned(),
        state: EdgeState::Pending,
        proposer_key_id: proposer.key_id,
        proposer_principal: proposer.principal.clone(),
        ratifier_key_id: None,
        // Stamped with the proposal event's at when it is committed.
        created_at: EventTime::parse("2026-10-29T12:00:00.000000Z").expect("a fixed-width time"),
    }
}

/// Arms `store` to commit `claim_edge.proposed` for `edge` just before the
/// next head read: another writer's proposal landing between a use-case's
/// earlier reads and its head read.
fn propose_before_next_head(store: &MemStore, edge: ClaimEdgeRecord) {
    store.commit_before_next_head(move |head| {
        let head = head.expect("the tenant's chain");
        let edge = ClaimEdgeRecord {
            created_at: head.envelope.at.clone(),
            ..edge
        };
        let draft = EventDraft {
            tenant_id: edge.tenant_id,
            event_id: Uuid::from_u128(0xED6E_0000_0000_0000_0000_0000_0000_00EE),
            event_type: event_types::CLAIM_EDGE_PROPOSED.to_owned(),
            principal: edge.proposer_principal.clone(),
            api_key_id: Some(edge.proposer_key_id),
            at: head.envelope.at.clone(),
            payload: json!({
                "edge_id": edge.edge_id.to_string(),
                "src_artifact_id": edge.src_artifact_id.to_string(),
                "dst_artifact_id": edge.dst_artifact_id.to_string(),
                "label": edge.label,
                "proposer_key_id": edge.proposer_key_id.to_string(),
            }),
        };
        let envelope = next_envelope(Some(head), draft).expect("a lawful draft");
        let event = sign(envelope, &SigningKey::from_bytes(&TEST_SEED)).expect("signing");
        CommitRequest {
            outbox: OutboxRow {
                event_id: event.envelope.event_id,
                topic: event.envelope.event_type.clone(),
                payload: json!({ "seq": event.envelope.seq }),
            },
            event,
            fold: MetadataFold::ClaimEdgeProposed(edge),
            idempotency: None,
        }
    });
}

/// The stored lineage principals of `memory_id`.
async fn stored_principals(store: &MemStore, tenant: Uuid, memory_id: Uuid) -> Vec<String> {
    store
        .memory(tenant, memory_id)
        .await
        .expect("read")
        .expect("the row")
        .lineage_principals
}

#[tokio::test]
async fn a_claim_edge_proposed_between_a_verdicts_reviewer_check_and_its_commit_never_promotes_past_a_reviewer_it_puts_in_the_lineage()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = issue(&app, admin, Role::Writer, "alice", crew).await;
    let bob_writer = issue(&app, admin, Role::Writer, "bob", crew).await;
    let bob_dreamer = issue(&app, admin, Role::Dreamer, "bob", crew).await;
    let pat = issue(&app, admin, Role::Dreamer, "pat", crew).await;
    let a = app
        .submit_memory(&alice.value, &submit_req("edge-a", "alice's fact"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    let b = app
        .submit_memory(&bob_writer.value, &submit_req("edge-b", "bob's fact"))
        .await
        .expect("submit")
        .receipt
        .memory_id;

    // The edge joins bob's row to alice's while bob's verdict is in flight.
    propose_before_next_head(&store, pending_edge(boot.tenant_id, &pat, a, b));
    let err = app
        .post_verdict(&bob_dreamer.value, a, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Verdict(oml_app::domain::VerdictError::PrincipalInLineage(_))
        ),
        "the reviewer the edge put in the lineage is refused: {err:?}"
    );
    let row = store
        .memory(boot.tenant_id, a)
        .await
        .expect("read")
        .expect("the row");
    assert_eq!(row.status, MemoryStatus::Held, "the row is not promoted");
    assert!(row.lineage_principals.contains(&"bob".to_owned()));
}

#[tokio::test]
async fn a_claim_edge_proposed_on_an_ancestor_between_a_captures_ancestor_read_and_its_commit_reaches_the_captured_rows_closure()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = issue(&app, admin, Role::Writer, "alice", crew).await;
    let bob = issue(&app, admin, Role::Writer, "bob", crew).await;
    let carol = agent(&app, admin, "carol", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = issue(&app, admin, Role::Dreamer, "pat", crew).await;
    let a = promoted(
        &app,
        &alice.value,
        &dana.value,
        &submit_req("anc-a", "alice's fact"),
    )
    .await;
    let b = app
        .submit_memory(&bob.value, &submit_req("anc-b", "bob's fact"))
        .await
        .expect("submit")
        .receipt
        .memory_id;

    propose_before_next_head(&store, pending_edge(boot.tenant_id, &pat, a, b));
    let d = app
        .submit_memory(
            &carol.value,
            &SubmitRequest {
                derives_from: vec![a],
                ..submit_req("anc-d", "carol builds on alice")
            },
        )
        .await
        .expect("submit")
        .receipt
        .memory_id;
    let principals = stored_principals(&store, boot.tenant_id, d).await;
    assert!(
        principals.contains(&"bob".to_owned()),
        "the captured row holds the author the edge joined to its ancestor: {principals:?}"
    );
}

#[tokio::test]
async fn a_claim_edge_proposed_on_a_target_between_a_supersedes_target_read_and_its_commit_reaches_the_successors_closure()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let carol = agent(&app, admin, "carol", crew).await;
    let bob = issue(&app, admin, Role::Writer, "bob", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = issue(&app, admin, Role::Dreamer, "pat", crew).await;
    let target = promoted(
        &app,
        &carol.value,
        &dana.value,
        &submit_req("sup-t", "carol's fact"),
    )
    .await;
    let b = app
        .submit_memory(&bob.value, &submit_req("sup-b", "bob's fact"))
        .await
        .expect("submit")
        .receipt
        .memory_id;

    propose_before_next_head(&store, pending_edge(boot.tenant_id, &pat, target, b));
    let successor = app
        .supersede(
            &carol.value,
            &supersede_req("sup-s", target, "carol's revised fact"),
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    let principals = stored_principals(&store, boot.tenant_id, successor).await;
    assert!(
        principals.contains(&"bob".to_owned()),
        "the successor holds the author the edge joined to its target: {principals:?}"
    );
}

// ---------------------------------------------------------------------------
// Proposing a claim edge (ADR-0008 d5; ADR-0012; ADR-0021 d5)
// ---------------------------------------------------------------------------

/// A dreamer-role key holding dreamer and reader on `node`: the key that
/// proposes an edge between two of the node's versions (workareas.md, edge
/// authority).
async fn curator(app: &TestApp, admin: &str, principal: &str, node: Uuid) -> IssuedKey {
    issue_with(
        app,
        admin,
        Role::Dreamer,
        principal,
        vec![grant(node, Role::Dreamer), grant(node, Role::Reader)],
    )
    .await
}

fn edge_req(from: Uuid, to: Uuid, label: &str) -> ProposeClaimEdge {
    ProposeClaimEdge {
        from_artifact_id: from,
        to_artifact_id: to,
        label: label.to_owned(),
    }
}

#[tokio::test]
async fn a_dreamer_proposes_a_claim_edge_and_it_is_journalled_pending_as_claim_edge_proposed() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let bob = agent(&app, admin, "bob", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let a = promoted(&app, &alice.value, &dana.value, &submit_req("prop-a", "a")).await;
    let b = promoted(&app, &bob.value, &dana.value, &submit_req("prop-b", "b")).await;

    let outcome = app
        .propose_claim_edge(&pat.value, &edge_req(a, b, "acme:supports"), None)
        .await
        .expect("propose");
    assert!(!outcome.replayed);
    let proposal = outcome.proposal;
    assert_eq!(
        serde_json::to_value(&proposal).expect("serializes"),
        json!({
            "edge_id": proposal.edge_id.to_string(),
            "from_artifact_id": a.to_string(),
            "to_artifact_id": b.to_string(),
            "label": "acme:supports",
            "state": "pending",
            "proposer_key_id": pat.key_id.to_string(),
            "ratifier_key_id": null,
        }),
        "the stored response is openapi.yaml's ClaimEdge, nothing more"
    );

    let edge = store
        .claim_edge(boot.tenant_id, proposal.edge_id)
        .await
        .expect("read")
        .expect("the edge is stored");
    assert_eq!(edge.state, EdgeState::Pending);
    assert_eq!((edge.src_artifact_id, edge.dst_artifact_id), (a, b));
    assert_eq!(edge.label, "acme:supports");
    assert_eq!(edge.proposer_key_id, pat.key_id);
    assert_eq!(edge.proposer_principal, "pat");
    assert_eq!(edge.ratifier_key_id, None);

    let events = journal(&store, boot.tenant_id).await;
    let last = &events.last().expect("the chain").envelope;
    assert_eq!(last.event_type, event_types::CLAIM_EDGE_PROPOSED);
    assert_eq!(last.api_key_id, Some(pat.key_id));
    assert_eq!(
        last.payload,
        json!({
            "edge_id": proposal.edge_id.to_string(),
            "src_artifact_id": a.to_string(),
            "dst_artifact_id": b.to_string(),
            "label": "acme:supports",
            "proposer_key_id": pat.key_id.to_string(),
        }),
        "the event-catalog payload, nothing more"
    );
}

#[tokio::test]
async fn a_writer_key_proposing_a_claim_edge_is_refused_access_verb_ceiling() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let a = held(
        &app,
        &alice.value,
        &submit_req("ceil-a", "the fact of ceil-a"),
    )
    .await;
    let b = held(
        &app,
        &alice.value,
        &submit_req("ceil-b", "the fact of ceil-b"),
    )
    .await;
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .propose_claim_edge(&alice.value, &edge_req(a, b, "acme:supports"), None)
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
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn a_dreamer_without_dreamer_on_either_endpoints_workarea_is_refused_access_grant_refused() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let other = workarea(&app, admin, "other", boot.root_workarea_id).await;
    let alice = agent(&app, admin, "alice", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let a = promoted(&app, &alice.value, &dana.value, &submit_req("gr-a", "a")).await;
    let b = promoted(&app, &alice.value, &dana.value, &submit_req("gr-b", "b")).await;
    // Reader on the endpoints' workarea, dreamer only elsewhere.
    let rhea = issue_with(
        &app,
        admin,
        Role::Dreamer,
        "rhea",
        vec![grant(crew, Role::Reader), grant(other, Role::Dreamer)],
    )
    .await;
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .propose_claim_edge(&rhea.value, &edge_req(a, b, "acme:supports"), None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::GrantRefused {
                workarea_id,
                needed_role: Role::Dreamer,
            } if workarea_id == crew
        ),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn an_endpoint_outside_the_keys_read_scope_answers_access_not_found_as_an_absent_one_does() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let elsewhere = workarea(&app, admin, "elsewhere", boot.root_workarea_id).await;
    let alice = agent(&app, admin, "alice", crew).await;
    let erin = agent(&app, admin, "erin", elsewhere).await;
    let eve = issue(&app, admin, Role::Dreamer, "eve", elsewhere).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let a = promoted(&app, &alice.value, &dana.value, &submit_req("scope-a", "a")).await;
    // Promoted, but in a workarea where pat holds no reader.
    let unseen = promoted(&app, &erin.value, &eve.value, &submit_req("scope-u", "u")).await;
    let before = journal(&store, boot.tenant_id).await.len();

    let outside = app
        .propose_claim_edge(&pat.value, &edge_req(a, unseen, "acme:supports"), None)
        .await
        .unwrap_err();
    let absent = app
        .propose_claim_edge(
            &pat.value,
            &edge_req(a, Uuid::from_u128(0xAB5E_0000), "acme:supports"),
            None,
        )
        .await
        .unwrap_err();
    let outside_from = app
        .propose_claim_edge(&pat.value, &edge_req(unseen, a, "acme:supports"), None)
        .await
        .unwrap_err();
    for err in [&outside, &absent, &outside_from] {
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn a_replayed_idempotency_key_answers_the_first_proposal_and_journals_once() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let a = promoted(&app, &alice.value, &dana.value, &submit_req("idem-a", "a")).await;
    let b = promoted(&app, &alice.value, &dana.value, &submit_req("idem-b", "b")).await;
    let req = edge_req(a, b, "acme:supports");

    let first = app
        .propose_claim_edge(&pat.value, &req, Some("edge-1"))
        .await
        .expect("propose");
    let after_first = journal(&store, boot.tenant_id).await.len();
    let replay = app
        .propose_claim_edge(&pat.value, &req, Some("edge-1"))
        .await
        .expect("replay");
    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(replay.proposal, first.proposal);
    assert_eq!(journal(&store, boot.tenant_id).await.len(), after_first);

    let err = app
        .propose_claim_edge(&pat.value, &edge_req(b, a, "acme:supports"), Some("edge-1"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::IdempotencyDigestMismatch),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), after_first);
}

/// The held row derives from `c`, which the edge connects to bob's promoted
/// `b`: the edge widens the closure of every row that reaches `c`, so bob
/// is in the held row's lineage once it is proposed.
#[tokio::test]
async fn a_pending_edge_refuses_a_verdict_on_a_held_row_deriving_from_an_endpoint_by_the_other_endpoints_author_with_review_author_in_lineage()
 {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let carol = agent(&app, admin, "carol", crew).await;
    let bob_writer = agent(&app, admin, "bob", crew).await;
    let bob_dreamer = issue(&app, admin, Role::Dreamer, "bob", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let c = promoted(&app, &carol.value, &dana.value, &submit_req("verd-c", "c")).await;
    let b = promoted(
        &app,
        &bob_writer.value,
        &dana.value,
        &submit_req("verd-b", "b"),
    )
    .await;
    let a = held(
        &app,
        &alice.value,
        &SubmitRequest {
            derives_from: vec![c],
            ..submit_req("verd-a", "the fact of verd-a")
        },
    )
    .await;

    app.propose_claim_edge(&pat.value, &edge_req(b, c, "acme:supports"), None)
        .await
        .expect("propose");
    let err = app
        .post_verdict(&bob_dreamer.value, a, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Verdict(oml_app::domain::VerdictError::PrincipalInLineage(_))
        ),
        "the author the pending edge joined is refused R2: {err:?}"
    );
    let row = store
        .memory(boot.tenant_id, a)
        .await
        .expect("read")
        .expect("the row");
    assert_eq!(row.status, MemoryStatus::Held);
}

#[tokio::test]
async fn review_next_never_serves_a_held_row_deriving_from_an_endpoint_to_the_author_of_the_other_endpoint()
 {
    let app = app();
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let carol = agent(&app, admin, "carol", crew).await;
    let bob_writer = agent(&app, admin, "bob", crew).await;
    let bob_dreamer = issue(&app, admin, Role::Dreamer, "bob", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let c = promoted(&app, &carol.value, &dana.value, &submit_req("queue-c", "c")).await;
    let b = promoted(
        &app,
        &bob_writer.value,
        &dana.value,
        &submit_req("queue-b", "b"),
    )
    .await;
    let a = held(
        &app,
        &alice.value,
        &SubmitRequest {
            derives_from: vec![c],
            ..submit_req("queue-a", "the fact of queue-a")
        },
    )
    .await;

    let served = app
        .review_next(&bob_dreamer.value)
        .await
        .expect("review next")
        .map(|m| m.memory.memory_id);
    assert_eq!(
        served,
        Some(a),
        "before the edge alice's row is bob's to review"
    );

    app.propose_claim_edge(&pat.value, &edge_req(b, c, "acme:supports"), None)
        .await
        .expect("propose");
    let served = app
        .review_next(&bob_dreamer.value)
        .await
        .expect("review next")
        .map(|m| m.memory.memory_id);
    assert_eq!(served, None, "the edge put bob in alice's row's lineage");
}

/// Another author's row is in a key's read scope only once promoted
/// (roles-and-keys.md footnote 5; `App::readable`): a held, rejected,
/// demoted or erased one in a workarea where the key holds dreamer and
/// reader answers as an absent row does, and journals nothing.
#[tokio::test]
async fn another_authors_held_rejected_demoted_or_erased_endpoint_answers_access_not_found() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let seen = promoted(&app, &alice.value, &dana.value, &submit_req("st-p", "p")).await;
    let held_row = held(&app, &alice.value, &submit_req("st-h", "h")).await;
    let rejected = held(&app, &alice.value, &submit_req("st-r", "r")).await;
    app.post_verdict(&dana.value, rejected, Verdict::Reject, Some("no"))
        .await
        .expect("reject");
    // This build journals no demotion or erasure: the store stands them in.
    let demoted = promoted(&app, &alice.value, &dana.value, &submit_req("st-d", "d")).await;
    store.set_status(boot.tenant_id, demoted, MemoryStatus::Demoted);
    let erased = promoted(&app, &alice.value, &dana.value, &submit_req("st-e", "e")).await;
    store.set_status(boot.tenant_id, erased, MemoryStatus::Erased);
    let before = journal(&store, boot.tenant_id).await.len();

    for row in [held_row, rejected, demoted, erased] {
        for req in [
            edge_req(seen, row, "acme:supports"),
            edge_req(row, seen, "acme:supports"),
        ] {
            let err = app
                .propose_claim_edge(&pat.value, &req, None)
                .await
                .unwrap_err();
            assert!(matches!(err, AppError::NotFound), "{row}: {err:?}");
        }
    }
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn an_oml_about_proposal_is_refused_request_validation_and_journals_nothing() {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let a = promoted(&app, &alice.value, &dana.value, &submit_req("about-a", "a")).await;
    let b = promoted(&app, &alice.value, &dana.value, &submit_req("about-b", "b")).await;
    let before = journal(&store, boot.tenant_id).await.len();

    let err = app
        .propose_claim_edge(&pat.value, &edge_req(a, b, "oml:about"), Some("about-1"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::ClaimEdgeLabel {
                field: EdgeField::Label,
                reason: LabelRefusal::AboutNotBuilt,
            }
        ),
        "{err:?}"
    );
    assert_eq!(journal(&store, boot.tenant_id).await.len(), before);
}

// ---------------------------------------------------------------------------
// Claim-edge ratification: R2 and the cycle refusal (ADR-0012, ADR-0021 d5)
// ---------------------------------------------------------------------------

/// A promoted term whose `claim_key` is `curie`, written by `writer` and
/// approved by `dreamer` (ADR-0021 d2: a term's claim_key is its CURIE).
async fn term(app: &TestApp, writer: &str, dreamer: &str, idem: &str, curie: &str) -> Uuid {
    promoted(
        app,
        writer,
        dreamer,
        &SubmitRequest {
            kind: MemoryKind::Term,
            claim_key: curie.to_owned(),
            ..submit_req(idem, &format!("{curie}: a term"))
        },
    )
    .await
}

/// The id `propose_claim_edge` gave a proposal of `src -> dst` under `label`.
async fn proposed(app: &TestApp, proposer: &str, src: Uuid, dst: Uuid, label: &str) -> Uuid {
    app.propose_claim_edge(proposer, &edge_req(src, dst, label), None)
        .await
        .expect("propose")
        .proposal
        .edge_id
}

/// Commits `claim_edge.proposed` for `edge` on the tenant's head, as another
/// writer would: the edge a label no standalone proposal admits needs, such
/// as `oml:replaced_by`, declared only inside a deprecation (ADR-0021 d3).
async fn commit_pending_edge(store: &MemStore, edge: ClaimEdgeRecord, event_id: u128) {
    let head_seq = store
        .chain_head(edge.tenant_id)
        .await
        .expect("head")
        .expect("the tenant's chain")
        .seq;
    let head = store
        .events_range(edge.tenant_id, head_seq, 1)
        .await
        .expect("read")
        .pop()
        .expect("the head event");
    let edge = ClaimEdgeRecord {
        created_at: head.envelope.at.clone(),
        ..edge
    };
    let draft = EventDraft {
        tenant_id: edge.tenant_id,
        event_id: Uuid::from_u128(event_id),
        event_type: event_types::CLAIM_EDGE_PROPOSED.to_owned(),
        principal: edge.proposer_principal.clone(),
        api_key_id: Some(edge.proposer_key_id),
        at: head.envelope.at.clone(),
        payload: json!({
            "edge_id": edge.edge_id.to_string(),
            "src_artifact_id": edge.src_artifact_id.to_string(),
            "dst_artifact_id": edge.dst_artifact_id.to_string(),
            "label": edge.label,
            "proposer_key_id": edge.proposer_key_id.to_string(),
        }),
    };
    let envelope = next_envelope(Some(&head), draft).expect("a lawful draft");
    let event = sign(envelope, &SigningKey::from_bytes(&TEST_SEED)).expect("signing");
    store
        .commit(CommitRequest {
            outbox: OutboxRow {
                event_id: event.envelope.event_id,
                topic: event.envelope.event_type.clone(),
                payload: json!({ "seq": event.envelope.seq }),
            },
            event,
            fold: MetadataFold::ClaimEdgeProposed(edge),
            idempotency: None,
        })
        .await
        .expect("commit the pending edge");
}

/// The tenant's journalled `review.ratify` events.
async fn ratifications(store: &MemStore, tenant: Uuid) -> Vec<SignedEnvelope> {
    journal(store, tenant)
        .await
        .into_iter()
        .filter(|e| e.envelope.event_type == event_types::REVIEW_RATIFY)
        .collect()
}

/// A tenant with two promoted claims by alice and bob, a pending
/// `acme:supports` edge between them proposed by pat, and the keys around
/// it.
struct EdgeFixture {
    store: MemStore,
    app: TestApp,
    boot: BootstrapResult,
    crew: Uuid,
    alice: IssuedKey,
    bob: IssuedKey,
    pat: IssuedKey,
    a: Uuid,
    b: Uuid,
    edge_id: Uuid,
}

async fn edge_fixture() -> EdgeFixture {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let bob = agent(&app, admin, "bob", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let a = promoted(&app, &alice.value, &dana.value, &submit_req("rat-a", "a")).await;
    let b = promoted(&app, &bob.value, &dana.value, &submit_req("rat-b", "b")).await;
    let edge_id = proposed(&app, &pat.value, a, b, "acme:supports").await;
    EdgeFixture {
        store,
        app,
        boot,
        crew,
        alice,
        bob,
        pat,
        a,
        b,
        edge_id,
    }
}

/// The sorted closure a ratification of an edge between `a` and `b` checks,
/// digested as `review.ratify` journals it: SHA-256 over the RFC 8785 JSON
/// of its principals and key ids, each sorted.
fn closure_digest(principals: &[&str], key_ids: &[Uuid]) -> String {
    let mut principals: Vec<String> = principals.iter().map(|p| (*p).to_owned()).collect();
    principals.sort();
    let mut key_ids: Vec<String> = key_ids.iter().map(ToString::to_string).collect();
    key_ids.sort();
    let doc = json!({ "principals": principals, "key_ids": key_ids });
    sha256_digest_string(&canonical_bytes(&doc).expect("jcs"))
}

#[tokio::test]
async fn a_second_dreamer_ratifies_a_pending_edge_and_review_ratify_names_it_with_subject_kind_claim_edge()
 {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let rita = curator(&f.app, admin, "rita", f.crew).await;

    let outcome = f
        .app
        .ratify_claim_edge(&rita.value, f.edge_id, Some("rat-1"))
        .await
        .expect("ratify");
    assert!(!outcome.replayed);
    assert_eq!(
        serde_json::to_value(&outcome.edge).expect("serializes"),
        json!({
            "edge_id": f.edge_id.to_string(),
            "from_artifact_id": f.a.to_string(),
            "to_artifact_id": f.b.to_string(),
            "label": "acme:supports",
            "state": "ratified",
            "proposer_key_id": f.pat.key_id.to_string(),
            "ratifier_key_id": rita.key_id.to_string(),
        }),
        "the response is openapi.yaml's ClaimEdge, ratified"
    );

    let edge = f
        .store
        .claim_edge(f.boot.tenant_id, f.edge_id)
        .await
        .expect("read")
        .expect("the edge");
    assert_eq!(edge.state, EdgeState::Ratified);
    assert_eq!(edge.ratifier_key_id, Some(rita.key_id));

    let events = journal(&f.store, f.boot.tenant_id).await;
    let last = &events.last().expect("the chain").envelope;
    assert_eq!(last.event_type, event_types::REVIEW_RATIFY);
    assert_eq!(outcome.journal_seq, Some(last.seq));
    assert_eq!(last.api_key_id, Some(rita.key_id));
    assert_eq!(last.principal, "rita");
    assert_eq!(
        last.payload,
        json!({
            "subject_kind": "claim_edge",
            "artifact_id": null,
            "version": null,
            "edge_id": f.edge_id.to_string(),
            "verdict_event_ids": [],
            "ratifier_key_id": rita.key_id.to_string(),
            "policy_snapshot": {
                "k": 1,
                "required_roles": ["dreamer"],
                "independence_axis": "principal",
                "human_required": false,
            },
            "lineage_closure_digest": closure_digest(
                &["alice", "bob"],
                &[f.alice.key_id, f.bob.key_id],
            ),
        }),
        "the event-catalog payload, nothing more"
    );

    // The same Idempotency-Key replays the record and journals nothing.
    let replay = f
        .app
        .ratify_claim_edge(&rita.value, f.edge_id, Some("rat-1"))
        .await
        .expect("replay");
    assert!(replay.replayed);
    assert_eq!(replay.edge, outcome.edge);
    assert_eq!(
        replay.journal_seq, None,
        "the record holds the ClaimEdge alone"
    );
    assert_eq!(
        journal(&f.store, f.boot.tenant_id).await.len(),
        events.len()
    );
}

/// `review.*` and `claim_edge.*` have no key on the events tail
/// (http-api.md#events-tail, the per-family table; `tail_visible`): no key
/// is served the proposal or the ratification, the proposer's and the
/// ratifier's own keys included, while the endpoints' promotions stay on
/// the tail.
#[tokio::test]
async fn claim_edge_proposed_and_an_edges_review_ratify_never_appear_on_the_events_tail() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let rita = curator(&f.app, admin, "rita", f.crew).await;
    f.app
        .ratify_claim_edge(&rita.value, f.edge_id, None)
        .await
        .expect("ratify");
    let reader = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;

    // Both events are in the chain, so their absence below is the tail's.
    let journalled: Vec<String> = journal(&f.store, f.boot.tenant_id)
        .await
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect();
    for edge_event in [event_types::CLAIM_EDGE_PROPOSED, event_types::REVIEW_RATIFY] {
        assert!(
            journalled.iter().any(|t| t == edge_event),
            "{edge_event} is journalled"
        );
    }

    for key in [&f.pat, &rita, &reader] {
        let tail = f
            .app
            .tail_events(&key.value, Some(by_name("crew")), 0, 1000)
            .await
            .expect("tail");
        let types: Vec<&str> = tail
            .events
            .iter()
            .map(|e| e.event.envelope.event_type.as_str())
            .collect();
        assert!(
            types.contains(&event_types::LIFECYCLE_PROMOTE),
            "{}: the endpoints' promotions are on the tail: {types:?}",
            key.principal
        );
        assert!(
            !types
                .iter()
                .any(|t| t.starts_with("claim_edge.") || t.starts_with("review.")),
            "{}: no claim_edge.* or review.* event: {types:?}",
            key.principal
        );
        assert_eq!(
            tail.next_after_seq, tail.head_seq,
            "the scan reached the ratification"
        );
    }
}

#[tokio::test]
async fn the_proposers_own_key_is_refused_review_ratifier_is_proposer() {
    let f = edge_fixture().await;
    let before = journal(&f.store, f.boot.tenant_id).await.len();

    let err = f
        .app
        .ratify_claim_edge(&f.pat.value, f.edge_id, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::RatifierIsProposer), "{err:?}");
    assert_eq!(journal(&f.store, f.boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn another_key_of_the_proposers_principal_is_refused_review_ratifier_is_proposer() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let pat_again = curator(&f.app, admin, "pat", f.crew).await;
    let before = journal(&f.store, f.boot.tenant_id).await.len();

    let err = f
        .app
        .ratify_claim_edge(&pat_again.value, f.edge_id, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::RatifierIsProposer), "{err:?}");
    assert_eq!(journal(&f.store, f.boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn a_ratifier_who_authored_either_endpoint_is_refused_review_author_in_lineage() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    // Dreamer keys of each endpoint's author: alice wrote the source, bob
    // the target.
    let alice_dreamer = curator(&f.app, admin, "alice", f.crew).await;
    let bob_dreamer = curator(&f.app, admin, "bob", f.crew).await;
    let before = journal(&f.store, f.boot.tenant_id).await.len();

    for ratifier in [&alice_dreamer, &bob_dreamer] {
        let err = f
            .app
            .ratify_claim_edge(&ratifier.value, f.edge_id, None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::AuthorInLineage),
            "{}: {err:?}",
            ratifier.principal
        );
    }
    assert_eq!(journal(&f.store, f.boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn a_service_key_is_refused_claim_edge_ratify_with_access_verb_ceiling_even_for_an_unknown_edge()
 {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let repair = f
        .app
        .issue_key(
            admin,
            &IssueKeyRequest {
                held_by: Some(HeldBy::Service),
                ..issue_req(
                    Role::Dreamer,
                    "reconciler",
                    vec![grant(f.crew, Role::Dreamer), grant(f.crew, Role::Reader)],
                )
            },
        )
        .await
        .expect("issue a service key");
    let before = journal(&f.store, f.boot.tenant_id).await.len();

    for edge_id in [f.edge_id, Uuid::from_u128(0xDEAD)] {
        let err = f
            .app
            .ratify_claim_edge(&repair.value, edge_id, None)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                AppError::VerbCeiling {
                    role: Role::Dreamer,
                    verb: "claim_edge.ratify",
                }
            ),
            "refused before the edge is looked up: {err:?}"
        );
    }
    assert_eq!(journal(&f.store, f.boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn an_unknown_edge_id_answers_access_not_found() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let rita = curator(&f.app, admin, "rita", f.crew).await;
    // An edge whose endpoints rita cannot read answers as an absent one.
    let elsewhere = workarea(&f.app, admin, "elsewhere", f.boot.root_workarea_id).await;
    let outsider = curator(&f.app, admin, "otto", elsewhere).await;
    let before = journal(&f.store, f.boot.tenant_id).await.len();

    let err = f
        .app
        .ratify_claim_edge(&rita.value, Uuid::from_u128(0xDEAD), None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let err = f
        .app
        .ratify_claim_edge(&outsider.value, f.edge_id, None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    assert_eq!(journal(&f.store, f.boot.tenant_id).await.len(), before);
}

/// A tenant with three promoted terms, `acme:x`, `acme:y` and `acme:z`,
/// a proposer pat and two ratifiers, rita and ruth.
struct TermFixture {
    store: MemStore,
    app: TestApp,
    boot: BootstrapResult,
    pat: IssuedKey,
    rita: IssuedKey,
    ruth: IssuedKey,
    x: Uuid,
    y: Uuid,
    z: Uuid,
}

async fn term_fixture() -> TermFixture {
    let store = MemStore::new();
    let app = app_over(store.clone());
    let (boot, crew) = tenant(&app).await;
    let admin = &boot.admin_key.value;
    let alice = agent(&app, admin, "alice", crew).await;
    let dana = issue(&app, admin, Role::Dreamer, "dana", crew).await;
    let pat = curator(&app, admin, "pat", crew).await;
    let rita = curator(&app, admin, "rita", crew).await;
    let ruth = curator(&app, admin, "ruth", crew).await;
    let x = term(&app, &alice.value, &dana.value, "t-x", "acme:x").await;
    let y = term(&app, &alice.value, &dana.value, "t-y", "acme:y").await;
    let z = term(&app, &alice.value, &dana.value, "t-z", "acme:z").await;
    TermFixture {
        store,
        app,
        boot,
        pat,
        rita,
        ruth,
        x,
        y,
        z,
    }
}

fn is_cycle_refused(err: &AppError, label: OmlLabel) -> bool {
    matches!(err, AppError::CycleRefused(refused) if refused.label == label)
}

#[tokio::test]
async fn ratifying_an_is_a_edge_that_closes_a_cycle_is_refused_vocab_cycle_refused_and_journals_nothing()
 {
    let t = term_fixture().await;
    let xy = proposed(&t.app, &t.pat.value, t.x, t.y, "oml:is_a").await;
    let yz = proposed(&t.app, &t.pat.value, t.y, t.z, "oml:is_a").await;
    let zx = proposed(&t.app, &t.pat.value, t.z, t.x, "oml:is_a").await;
    t.app
        .ratify_claim_edge(&t.rita.value, xy, None)
        .await
        .expect("x is_a y");
    t.app
        .ratify_claim_edge(&t.rita.value, yz, None)
        .await
        .expect("y is_a z");
    let before = journal(&t.store, t.boot.tenant_id).await.len();

    let err = t
        .app
        .ratify_claim_edge(&t.ruth.value, zx, Some("cycle-1"))
        .await
        .unwrap_err();
    assert!(is_cycle_refused(&err, OmlLabel::IsA), "{err:?}");
    assert_eq!(journal(&t.store, t.boot.tenant_id).await.len(), before);
    let edge = t
        .store
        .claim_edge(t.boot.tenant_id, zx)
        .await
        .expect("read")
        .expect("the edge");
    assert_eq!(edge.state, EdgeState::Pending, "the refused edge waits");
}

#[tokio::test]
async fn a_pending_successor_or_about_edge_is_never_ratified_apart_from_its_deprecation_and_journals_nothing()
 {
    let t = term_fixture().await;
    let cases = [
        (
            0xED6E_0000_0000_0000_0000_0000_0000_0101_u128,
            OmlLabel::ReplacedBy,
            LabelRefusal::SuccessorOutsideDeprecation,
        ),
        (
            0xED6E_0000_0000_0000_0000_0000_0000_0102,
            OmlLabel::Consider,
            LabelRefusal::SuccessorOutsideDeprecation,
        ),
        (
            0xED6E_0000_0000_0000_0000_0000_0000_0103,
            OmlLabel::About,
            LabelRefusal::AboutNotBuilt,
        ),
    ];
    for (n, (edge_id, label, _)) in cases.iter().enumerate() {
        let edge = ClaimEdgeRecord {
            edge_id: Uuid::from_u128(*edge_id),
            label: label.as_str().to_owned(),
            ..pending_edge(t.boot.tenant_id, &t.pat, t.x, t.y)
        };
        commit_pending_edge(&t.store, edge, 0xEE01 + n as u128).await;
    }
    let before = journal(&t.store, t.boot.tenant_id).await.len();

    for (edge_id, label, reason) in cases {
        let edge_id = Uuid::from_u128(edge_id);
        let err = t
            .app
            .ratify_claim_edge(&t.rita.value, edge_id, Some(label.as_str()))
            .await
            .unwrap_err();
        assert!(
            matches!(
                &err,
                AppError::ClaimEdgeLabel { field: EdgeField::Label, reason: refused }
                    if *refused == reason
            ),
            "{label:?}: {err:?}"
        );
        let edge = t
            .store
            .claim_edge(t.boot.tenant_id, edge_id)
            .await
            .expect("read")
            .expect("the edge");
        assert_eq!(edge.state, EdgeState::Pending, "{label:?} waits");
    }
    assert_eq!(journal(&t.store, t.boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn an_is_a_proposal_between_two_terms_whose_claim_keys_are_not_curies_is_refused_request_validation_and_journals_nothing()
 {
    let t = term_fixture().await;
    let store = &t.store;
    let admin = &t.boot.admin_key.value;
    let crew = store
        .memory(t.boot.tenant_id, t.x)
        .await
        .expect("read")
        .expect("x")
        .workarea_id;
    let alice = agent(&t.app, admin, "alice-2", crew).await;
    let dana = issue(&t.app, admin, Role::Dreamer, "dana-2", crew).await;
    let plain = term(&t.app, &alice.value, &dana.value, "t-plain", "claim-plain").await;
    let before = journal(store, t.boot.tenant_id).await.len();

    for (from, to, field) in [(plain, t.x, EdgeField::From), (t.x, plain, EdgeField::To)] {
        for label in [OmlLabel::IsA, OmlLabel::PartOf] {
            let err = t
                .app
                .propose_claim_edge(&t.pat.value, &edge_req(from, to, label.as_str()), None)
                .await
                .unwrap_err();
            assert!(
                matches!(
                    &err,
                    AppError::ClaimEdgeLabel { field: f, reason: LabelRefusal::EndpointNotCurie { field: r } }
                        if *f == field && *r == field
                ),
                "{label:?}: {err:?}"
            );
        }
    }
    assert_eq!(journal(store, t.boot.tenant_id).await.len(), before);
}

#[tokio::test]
async fn an_is_a_loop_through_a_pending_edge_is_ratified_since_only_ratified_edges_count() {
    let t = term_fixture().await;
    let _xy_pending = proposed(&t.app, &t.pat.value, t.x, t.y, "oml:is_a").await;
    let yx = proposed(&t.app, &t.pat.value, t.y, t.x, "oml:is_a").await;

    let outcome = t
        .app
        .ratify_claim_edge(&t.rita.value, yx, None)
        .await
        .expect("a loop closed only by a pending edge is no cycle");
    assert_eq!(outcome.edge.state, "ratified");
}

#[tokio::test]
async fn a_relates_to_loop_carries_no_cycle_invariant() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let rita = curator(&f.app, admin, "rita", f.crew).await;
    let back = proposed(&f.app, &f.pat.value, f.b, f.a, "acme:relates_to").await;
    let forth = proposed(&f.app, &f.pat.value, f.a, f.b, "acme:relates_to").await;
    let self_edge = proposed(&f.app, &f.pat.value, f.a, f.a, "acme:relates_to").await;

    for edge_id in [back, forth, self_edge] {
        f.app
            .ratify_claim_edge(&rita.value, edge_id, None)
            .await
            .expect("an open label closes no cycle");
    }
    assert_eq!(ratifications(&f.store, f.boot.tenant_id).await.len(), 3);
}

#[tokio::test]
async fn ratifying_a_ratified_edge_answers_review_serve_invalid_consumed_and_journals_nothing() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let rita = curator(&f.app, admin, "rita", f.crew).await;
    let ruth = curator(&f.app, admin, "ruth", f.crew).await;
    f.app
        .ratify_claim_edge(&rita.value, f.edge_id, None)
        .await
        .expect("ratify");
    let before = journal(&f.store, f.boot.tenant_id).await.len();

    for key in [&rita, &ruth] {
        let err = f
            .app
            .ratify_claim_edge(&key.value, f.edge_id, None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::EdgeConsumed { edge_id } if edge_id == f.edge_id),
            "{err:?}"
        );
    }
    assert_eq!(journal(&f.store, f.boot.tenant_id).await.len(), before);

    // A ratification that lost the store's compare-and-set answers alike.
    let other = proposed(&f.app, &f.pat.value, f.b, f.a, "acme:supports").await;
    f.store.refuse_next_commit_of(
        event_types::REVIEW_RATIFY,
        StoreError::EdgeNotPending { edge_id: other },
    );
    let err = f
        .app
        .ratify_claim_edge(&rita.value, other, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::EdgeConsumed { edge_id } if edge_id == other),
        "{err:?}"
    );
}

#[tokio::test]
async fn of_two_ratifications_racing_to_close_an_is_a_cycle_one_commits_and_the_other_is_refused() {
    let t = term_fixture().await;
    let xy = proposed(&t.app, &t.pat.value, t.x, t.y, "oml:is_a").await;
    let yx = proposed(&t.app, &t.pat.value, t.y, t.x, "oml:is_a").await;
    let head = t
        .store
        .chain_head(t.boot.tenant_id)
        .await
        .expect("head")
        .expect("the chain")
        .seq;

    // Ruth's ratification of y is_a x reads no ratified is_a edge, passes
    // the cycle check, and loses the head to rita's ratification of x is_a
    // y, which commits between ruth's read and ruth's commit.
    t.store.refuse_next_commit_of(
        event_types::REVIEW_RATIFY,
        StoreError::SeqConflict {
            expected_seq: head + 2,
            found_seq: head + 1,
        },
    );
    let lost = t
        .app
        .ratify_claim_edge(&t.ruth.value, yx, None)
        .await
        .unwrap_err();
    assert!(
        matches!(lost, AppError::Store(StoreError::SeqConflict { .. })),
        "{lost:?}"
    );
    t.app
        .ratify_claim_edge(&t.rita.value, xy, None)
        .await
        .expect("rita's ratification commits");

    // The transport's head retry runs ruth's call again: it re-reads the
    // ratified edges and refuses.
    let err = t
        .app
        .ratify_claim_edge(&t.ruth.value, yx, None)
        .await
        .unwrap_err();
    assert!(is_cycle_refused(&err, OmlLabel::IsA), "{err:?}");

    let ratified = ratifications(&t.store, t.boot.tenant_id).await;
    assert_eq!(ratified.len(), 1, "one ratification commits");
    assert_eq!(
        ratified[0].envelope.payload["edge_id"],
        json!(xy.to_string())
    );
    let edge = t
        .store
        .claim_edge(t.boot.tenant_id, yx)
        .await
        .expect("read")
        .expect("the edge");
    assert_eq!(edge.state, EdgeState::Pending);
}

// ---------------------------------------------------------------------------
// The claim-edge read: GET /v2/claim-edges behind its use case (http-api.md,
// claim edges and pagination; ADR-0008 d5)
// ---------------------------------------------------------------------------

fn edges_of(artifact_id: Uuid) -> ClaimEdgeQuery {
    ClaimEdgeQuery {
        workarea: None,
        artifact_id: Some(artifact_id),
    }
}

fn edges_in(workarea: Reference) -> ClaimEdgeQuery {
    ClaimEdgeQuery {
        workarea: Some(workarea),
        artifact_id: None,
    }
}

fn edge_ids(page: &ClaimEdgePage) -> Vec<Uuid> {
    page.items.iter().map(|e| e.edge_id).collect()
}

#[tokio::test]
async fn the_claim_edges_read_lists_pending_and_ratified_edges_of_a_readable_artifact() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let rita = curator(&f.app, admin, "rita", f.crew).await;
    let ratified = proposed(&f.app, &f.pat.value, f.b, f.a, "acme:refutes").await;
    f.app
        .ratify_claim_edge(&rita.value, ratified, None)
        .await
        .expect("ratify");
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;

    let page = f
        .app
        .claim_edges(&rory.value, edges_of(f.a), None, 10)
        .await
        .expect("the read");
    assert_eq!(page.next_cursor, None);
    assert_eq!(
        page.items
            .iter()
            .map(|e| serde_json::to_value(e).expect("serializes"))
            .collect::<Vec<_>>(),
        vec![
            json!({
                "edge_id": ratified.to_string(),
                "from_artifact_id": f.b.to_string(),
                "to_artifact_id": f.a.to_string(),
                "label": "acme:refutes",
                "state": "ratified",
                "proposer_key_id": f.pat.key_id.to_string(),
                "ratifier_key_id": rita.key_id.to_string(),
            }),
            json!({
                "edge_id": f.edge_id.to_string(),
                "from_artifact_id": f.a.to_string(),
                "to_artifact_id": f.b.to_string(),
                "label": "acme:supports",
                "state": "pending",
                "proposer_key_id": f.pat.key_id.to_string(),
                "ratifier_key_id": null,
            }),
        ],
        "newest first, each as openapi.yaml's ClaimEdge"
    );
}

#[tokio::test]
async fn an_edge_with_an_endpoint_outside_read_scope_is_absent_from_the_read() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let elsewhere = workarea(&f.app, admin, "elsewhere", f.boot.root_workarea_id).await;
    let erin = agent(&f.app, admin, "erin", elsewhere).await;
    let eve = issue(&f.app, admin, Role::Dreamer, "eve", elsewhere).await;
    let unseen = promoted(&f.app, &erin.value, &eve.value, &submit_req("far-u", "u")).await;
    // A curator reading both workareas joins a to the row rory cannot read.
    let quinn = issue_with(
        &f.app,
        admin,
        Role::Dreamer,
        "quinn",
        vec![
            grant(f.crew, Role::Dreamer),
            grant(f.crew, Role::Reader),
            grant(elsewhere, Role::Reader),
        ],
    )
    .await;
    let hidden = proposed(&f.app, &quinn.value, f.a, unseen, "acme:supports").await;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;

    let of_a = f
        .app
        .claim_edges(&rory.value, edges_of(f.a), None, 10)
        .await
        .expect("the read");
    assert_eq!(edge_ids(&of_a), vec![f.edge_id]);
    let in_crew = f
        .app
        .claim_edges(&rory.value, edges_in(f.crew.into()), None, 10)
        .await
        .expect("the read");
    assert_eq!(edge_ids(&in_crew), vec![f.edge_id]);
    let of_unseen = f
        .app
        .claim_edges(&rory.value, edges_of(unseen), None, 10)
        .await
        .expect("the read");
    assert!(of_unseen.items.is_empty(), "{:?}", of_unseen.items);

    // The key that reads both ends is served the edge.
    let by_quinn = f
        .app
        .claim_edges(&quinn.value, edges_of(f.a), None, 10)
        .await
        .expect("the read");
    assert_eq!(edge_ids(&by_quinn), vec![hidden, f.edge_id]);
}

#[tokio::test]
async fn an_edge_to_a_version_demoted_after_its_proposal_is_absent_for_another_reader_and_served_to_its_author()
 {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;
    // No use case demotes in this build; the row turns demoted after the
    // edge to it was proposed.
    f.store
        .set_status(f.boot.tenant_id, f.a, MemoryStatus::Demoted);

    for query in [
        edges_of(f.b),
        edges_in(f.crew.into()),
        ClaimEdgeQuery::default(),
    ] {
        let page = f
            .app
            .claim_edges(&rory.value, query, None, 10)
            .await
            .expect("the read");
        assert!(page.items.is_empty(), "{:?}", page.items);
    }
    // Alice wrote a and holds reader on crew: her own row in any state is
    // in her read scope, and b is promoted.
    let by_author = f
        .app
        .claim_edges(&f.alice.value, edges_of(f.b), None, 10)
        .await
        .expect("the read");
    assert_eq!(edge_ids(&by_author), vec![f.edge_id]);
}

#[tokio::test]
async fn an_edge_whose_endpoint_is_another_authors_held_row_in_a_readable_workarea_is_absent() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let held = f
        .app
        .submit_memory(&f.alice.value, &submit_req("held-h", "h"))
        .await
        .expect("submit")
        .receipt
        .memory_id;
    // No proposer could read alice's held row; the edge lands as another
    // writer's commit would.
    let edge = ClaimEdgeRecord {
        edge_id: Uuid::from_u128(0xED6E_0000_0000_0000_0000_0000_0000_00AB),
        ..pending_edge(f.boot.tenant_id, &f.pat, f.b, held)
    };
    commit_pending_edge(&f.store, edge, 0xEE0A).await;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;

    for query in [edges_of(f.b), edges_of(held), ClaimEdgeQuery::default()] {
        let page = f
            .app
            .claim_edges(&rory.value, query, None, 10)
            .await
            .expect("the read");
        assert!(
            page.items.iter().all(|e| e.to_artifact_id != held),
            "{:?}",
            page.items
        );
    }
    assert_eq!(
        edge_ids(
            &f.app
                .claim_edges(&rory.value, edges_of(f.b), None, 10)
                .await
                .expect("the read")
        ),
        vec![f.edge_id]
    );
}

#[tokio::test]
async fn another_tenants_key_reads_an_empty_page_by_this_tenants_artifact_and_its_cursor_answers_foreign()
 {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    proposed(&f.app, &f.pat.value, f.b, f.a, "acme:refutes").await;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;
    let cursor = f
        .app
        .claim_edges(&rory.value, edges_of(f.a), None, 1)
        .await
        .expect("the first page")
        .next_cursor
        .expect("another page");

    let globex = f
        .app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("globex"),
            admin_key_name: name("globex-admin"),
            admin_principal: "gina-admin".to_owned(),
        })
        .await
        .expect("bootstrap another tenant");
    let globex_admin = &globex.admin_key.value;
    let desk = workarea(&f.app, globex_admin, "desk", globex.root_workarea_id).await;
    let gus = issue(&f.app, globex_admin, Role::Reader, "gus", desk).await;

    for query in [edges_of(f.a), edges_of(f.b), ClaimEdgeQuery::default()] {
        let page = f
            .app
            .claim_edges(&gus.value, query, None, 10)
            .await
            .expect("the read");
        assert!(page.items.is_empty(), "{:?}", page.items);
        assert_eq!(page.next_cursor, None);
    }
    let err = f
        .app
        .claim_edges(&gus.value, edges_of(f.a), Some(&cursor), 1)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::Cursor(CursorError::Foreign)),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_named_workarea_the_key_holds_no_reader_on_answers_access_not_found() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let elsewhere = workarea(&f.app, admin, "elsewhere", f.boot.root_workarea_id).await;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;
    // Wes holds writer on crew and no reader anywhere: a node without
    // reader is unreadable, so the refusal order stops at access.not_found
    // before access.grant_refused (roles-and-keys.md refusal order).
    let wes = issue(&f.app, admin, Role::Writer, "wes", f.crew).await;

    for (key, reference) in [
        (&rory, Reference::from(elsewhere)),
        (&rory, by_name("elsewhere")),
        (&wes, Reference::from(f.crew)),
        (&wes, by_name("crew")),
    ] {
        let err = f
            .app
            .claim_edges(&key.value, edges_in(reference), None, 10)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
}

#[tokio::test]
async fn a_workarea_filter_serves_an_edge_between_two_claims_in_its_child_workareas() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let left = workarea(&f.app, admin, "left", f.crew).await;
    let right = workarea(&f.app, admin, "right", f.crew).await;
    let dana = issue(&f.app, admin, Role::Dreamer, "dina", f.crew).await;
    let l = promoted(
        &f.app,
        &f.alice.value,
        &dana.value,
        &submit_into("sub-l", "l", left),
    )
    .await;
    let r = promoted(
        &f.app,
        &f.bob.value,
        &dana.value,
        &submit_into("sub-r", "r", right),
    )
    .await;
    let below = proposed(&f.app, &f.pat.value, l, r, "acme:supports").await;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;

    let in_crew = f
        .app
        .claim_edges(&rory.value, edges_in(f.crew.into()), None, 10)
        .await
        .expect("the read");
    assert_eq!(edge_ids(&in_crew), vec![below, f.edge_id]);
    let in_left = f
        .app
        .claim_edges(&rory.value, edges_in(left.into()), None, 10)
        .await
        .expect("the read");
    assert_eq!(edge_ids(&in_left), vec![below]);
}

#[tokio::test]
async fn a_page_cursor_resumes_after_the_last_edge() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    let elsewhere = workarea(&f.app, admin, "elsewhere", f.boot.root_workarea_id).await;
    let erin = agent(&f.app, admin, "erin", elsewhere).await;
    let eve = issue(&f.app, admin, Role::Dreamer, "eve", elsewhere).await;
    let unseen = promoted(&f.app, &erin.value, &eve.value, &submit_req("pg-u", "u")).await;
    let quinn = issue_with(
        &f.app,
        admin,
        Role::Dreamer,
        "quinn",
        vec![
            grant(f.crew, Role::Dreamer),
            grant(f.crew, Role::Reader),
            grant(elsewhere, Role::Reader),
        ],
    )
    .await;
    // Newest first: fourth, the edge rory cannot read, second, the fixture's.
    let second = proposed(&f.app, &f.pat.value, f.b, f.a, "acme:refutes").await;
    proposed(&f.app, &quinn.value, f.a, unseen, "acme:supports").await;
    let fourth = proposed(&f.app, &f.pat.value, f.a, f.b, "acme:cites").await;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;
    let query = edges_of(f.a);

    let first = f
        .app
        .claim_edges(&rory.value, query.clone(), None, 2)
        .await
        .expect("the first page");
    assert_eq!(
        edge_ids(&first),
        vec![fourth, second],
        "the page reads past the edge it filters out to fill its limit"
    );
    let cursor = first.next_cursor.expect("another page");
    let next = f
        .app
        .claim_edges(&rory.value, query, Some(&cursor), 2)
        .await
        .expect("the next page");
    assert_eq!(edge_ids(&next), vec![f.edge_id]);
    assert_eq!(next.next_cursor, None, "the listing is exhausted");
}

#[tokio::test]
async fn a_cursor_minted_for_another_filter_is_refused_request_validation() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    proposed(&f.app, &f.pat.value, f.b, f.a, "acme:refutes").await;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;
    let cursor = f
        .app
        .claim_edges(&rory.value, edges_of(f.a), None, 1)
        .await
        .expect("the first page")
        .next_cursor
        .expect("another page");

    for other in [
        edges_of(f.b),
        edges_in(f.crew.into()),
        ClaimEdgeQuery {
            workarea: Some(f.crew.into()),
            artifact_id: Some(f.a),
        },
    ] {
        let err = f
            .app
            .claim_edges(&rory.value, other, Some(&cursor), 1)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::Cursor(CursorError::OtherFilter)),
            "{err:?}"
        );
    }
    // Under its own filter it resumes.
    f.app
        .claim_edges(&rory.value, edges_of(f.a), Some(&cursor), 1)
        .await
        .expect("its own filter resumes");
}

#[tokio::test]
async fn an_admin_key_reading_claim_edges_is_refused_access_verb_ceiling() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    for query in [
        edges_of(f.a),
        edges_in(f.crew.into()),
        ClaimEdgeQuery::default(),
    ] {
        let err = f.app.claim_edges(admin, query, None, 10).await.unwrap_err();
        assert!(
            matches!(
                err,
                AppError::VerbCeiling {
                    role: Role::Admin,
                    ..
                }
            ),
            "{err:?}"
        );
    }
}

#[tokio::test]
async fn a_workarea_named_by_its_name_reads_as_by_its_id() {
    let f = edge_fixture().await;
    let admin = &f.boot.admin_key.value;
    proposed(&f.app, &f.pat.value, f.b, f.a, "acme:refutes").await;
    let rory = issue(&f.app, admin, Role::Reader, "rory", f.crew).await;

    let by_id = f
        .app
        .claim_edges(&rory.value, edges_in(f.crew.into()), None, 1)
        .await
        .expect("by id");
    let by_name = f
        .app
        .claim_edges(&rory.value, edges_in(by_name("crew")), None, 1)
        .await
        .expect("by name");
    assert_eq!(edge_ids(&by_name), edge_ids(&by_id));
    assert_eq!(by_id.items.len(), 1);
    // The cursor binds the resolved node, so either spelling resumes it.
    let cursor = by_name.next_cursor.expect("another page");
    let rest = f
        .app
        .claim_edges(&rory.value, edges_in(f.crew.into()), Some(&cursor), 1)
        .await
        .expect("resumes by id");
    assert_eq!(edge_ids(&rest), vec![f.edge_id]);
}
