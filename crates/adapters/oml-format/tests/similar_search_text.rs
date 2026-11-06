//! The near-copy check reads the text search reads, not the source (ADR-0056
//! d4; ADR-0050 d1; review of #90, L4): two diagrams that say the same thing
//! in different source are the same claim, and the check names the first.

use oml_app::App;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::domain::name::Name;
use oml_app::domain::{MemoryKind, Role};
use oml_app::domain::access::Covers;
use oml_app::domain::HeldBy;
use oml_app::types::{
    AppError, BootstrapRequest, GrantRequest, IssueKeyRequest, OnSimilar, SubmitRequest,
};

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

const SEED: [u8; 32] = [7; 32];

fn app() -> TestApp {
    app_over(MemStore::new())
}

fn app_over(store: MemStore) -> TestApp {
    App::new(
        store,
        MemSigner::new(SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_formats(oml_format::compiled())
}

fn request(idem: &str, body: &str, format: Option<&str>) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        anchor: None,
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workarea: None,
        derives_from: Vec::new(),
        severity: None,
        rule: None,
        valid_from: None,
        valid_until: None,
        format: format.map(str::to_owned),
        title: format.map(|_| "a title".to_owned()),
    }
}

async fn writer(app: &TestApp) -> String {
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("name"),
            admin_key_name: Name::parse("root-admin").expect("name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let grant = |role| GrantRequest {
        node: boot.root_workarea_id.into(),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    };
    let issued = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Writer,
                name: Name::parse("w-key").expect("name"),
                principal: "writer".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: None,
                crew: None,
                grants: vec![grant(Role::Writer), grant(Role::Reader)],
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("issue");
    issued.value
}


#[tokio::test]
async fn two_diagrams_with_one_search_text_are_named_as_the_same_claim_whatever_their_source() {
    let app = app();
    let key = writer(&app).await;
    let first = request("one", "flowchart LR\n  A --> B", Some("text/vnd.oml.mermaid"));
    app.submit_memory(&key, &first).await.expect("first");
    let second = SubmitRequest {
        on_similar: OnSimilar::Refuse,
        ..request("two", "graph TD\n  A-->B", Some("text/vnd.oml.mermaid"))
    };
    match app.submit_memory(&key, &second).await {
        Err(AppError::SimilarExists(named)) => assert_eq!(named.len(), 1),
        other => panic!("the check read the source, not the search text: {other:?}"),
    }
}

#[tokio::test]
async fn the_verdict_event_covers_the_format_the_version_was_signed_with() {
    use oml_app::domain::{Role as R, event_types};
    use oml_app::ports::JournalStore as _;
    use oml_app::types::Verdict;
    let store = MemStore::new();
    let app = app_over(store.clone());
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("name"),
            admin_key_name: Name::parse("root-admin").expect("name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let grant = |role| GrantRequest {
        node: boot.root_workarea_id.into(),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    };
    let issue = |name: &'static str, role, grants| IssueKeyRequest {
        profile: None,
        role,
        name: Name::parse(name).expect("name"),
        principal: name.to_owned(),
        held_by: Some(HeldBy::Agent),
        default_workarea: None,
        crew: None,
        grants,
        connect_code: false,
        user: None,
        guardrail_scope: None,
    };
    let admin = &boot.admin_key.value;
    let w = app
        .issue_key(admin, &issue("w-key", R::Writer, vec![grant(R::Writer), grant(R::Reader)]))
        .await
        .expect("writer");
    let d = app
        .issue_key(admin, &issue("d-key", R::Dreamer, vec![grant(R::Dreamer), grant(R::Reader)]))
        .await
        .expect("dreamer");
    let sent = app
        .submit_memory(
            &w.value,
            &request("one", "flowchart LR\n  A --> B", Some("text/vnd.oml.mermaid")),
        )
        .await
        .expect("submit");
    app.post_verdict(&d.value, sent.receipt.memory_id, Verdict::Approve, None)
        .await
        .expect("verdict");
    let events = store.events_range(boot.tenant_id, 1, 1000).await.expect("journal");
    let verdict = events
        .iter()
        .find(|e| e.envelope.event_type == event_types::REVIEW_APPROVE)
        .expect("an approval");
    assert_eq!(verdict.envelope.payload["format"], "text/vnd.oml.mermaid", "{:?}", verdict.envelope.payload);
}
