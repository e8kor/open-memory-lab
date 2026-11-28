//! Public workspaces (ADR-0098): a protected or frozen workspace outside
//! every user root marked public is read by every reading key of the
//! tenant without a grant and searched by every default recall; marking it
//! is an admin person's widening act, clearing it is narrowing.

mod counted;

use oml_app::App;
use oml_app::app::{Caller, Capacity};
use oml_app::domain::access::Covers;
use oml_app::domain::admin_act::AdminActState;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::web::Email;
use oml_app::domain::{MemoryKind, Role, UsedBy, WorkspaceStatus, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::federation::{FederationPolicyChange, Sources, Via};
use oml_app::ports::JournalStore as _;
use oml_app::types::{
    AppError, Bearer, BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, IssuedKey,
    ListRequest, PublicRefusal, RecallRequest, SubmitRequest, Verdict, WebEmailRequest,
};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

const TEST_SEED: [u8; 32] = [
    0x09, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

const DAY: u64 = 86_400;

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

fn by_name(value: &str) -> Reference {
    Reference::parse(value).expect("a reference")
}

fn fresh_key_name() -> Name {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    name(&format!(
        "public-key-{}",
        ISSUED.fetch_add(1, Ordering::Relaxed)
    ))
}

fn grant(node: &str, role: Role) -> GrantRequest {
    GrantRequest {
        node: by_name(node),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
        create: false,
    }
}

fn key_req(role: Role, principal: &str, grants: Vec<GrantRequest>) -> IssueKeyRequest {
    IssueKeyRequest {
        profile: None,
        role,
        name: fresh_key_name(),
        principal: principal.to_owned(),
        used_by: Some(if role == Role::Reviewer {
            UsedBy::Person
        } else {
            UsedBy::Agent
        }),
        default_workspace: None,
        crew: None,
        grants,
        user: None,
        connect_code: false,
        guardrail_scope: None,
        expiry: None,
        requests_per_minute: None,
        person: None,
        create_workspaces: Vec::new(),
    }
}

fn submit(idem: &str, body: &str, workspace: &str, kind: MemoryKind) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        anchor: None,
        origin: None,
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind,
        body: body.to_owned(),
        workspace: Some(by_name(workspace)),
        derives_from: Vec::new(),
        severity: None,
        rule: None,
        kind_fields: None,
        valid_from: None,
        valid_until: None,
        format: None,
        title: None,
    }
}

fn recall(query: &str, workspace: Option<&str>) -> RecallRequest {
    RecallRequest {
        sources: None,
        timings: false,
        anchor: oml_app::types::AnchorFilter::default(),
        query: query.to_owned(),
        k: 10,
        workspace: workspace.map(by_name),
        kinds: Vec::new(),
        include_text: true,
        include_unreviewed: Some(false),
        max_tokens: None,
        search: None,
        match_mode: oml_app::search::MatchMode::Any,
        include_expired: false,
        now: None,
        as_of: None,
    }
}

fn list(workspace: Option<&str>) -> ListRequest {
    ListRequest {
        anchor: oml_app::types::AnchorFilter::default(),
        own: false,
        workspace: workspace.map(by_name),
        states: Vec::new(),
        kinds: Vec::new(),
        prefix: None,
        counts: false,
        claim_key: None,
        since: None,
        until: None,
        limit: 50,
        cursor: None,
        author: None,
        as_of: None,
    }
}

/// A tenant `acme` with `team`, a development workspace an agent reads by
/// its grant, and `shared` (with a child `shared-child`), protected, which
/// holds one approved memory, "the shared glossary names every term", and
/// one held one; `alice` is the tenant's one admin person.
struct Rig {
    app: TestApp,
    store: MemStore,
    clock: FixedClock,
    boot: BootstrapResult,
    /// A person's reviewer key on `shared`, which ratified its status.
    reviewer: IssuedKey,
    /// The contributor that wrote in `shared`.
    writer: IssuedKey,
    /// A reader key granted `team` alone.
    reader: IssuedKey,
    approved: Uuid,
    held: Uuid,
}

impl Rig {
    fn admin(&self) -> &str {
        self.boot.admin_key.value.as_str()
    }

    async fn admin_person(&self, user: &str) {
        let (entry, _) = self
            .app
            .allow_web_email(
                self.admin(),
                &WebEmailRequest {
                    email: Email::parse(&format!("{user}@example.com")).expect("an email"),
                    user: name(user),
                    admin: true,
                },
            )
            .await
            .expect("the admin entry");
        self.app
            .web_ensure_keys(&entry)
            .await
            .expect("session keys");
    }

    /// `<user>-admin`, from a session opened now.
    async fn admin_session(&self, user: &str) -> Caller {
        let entry = self
            .app
            .list_web_emails(self.admin(), None, 100)
            .await
            .expect("entries")
            .into_iter()
            .find(|e| e.user.as_str() == user)
            .expect("the entry");
        let token = self
            .app
            .web_session_open(&entry, 3600, None)
            .await
            .expect("a session")
            .token;
        let session = self
            .app
            .web_session(&token)
            .await
            .expect("a read")
            .expect("a live session");
        self.app
            .web_caller(&session, Capacity::Admin)
            .await
            .expect("the admin session key")
    }

    async fn key(&self, role: Role, principal: &str, grants: Vec<GrantRequest>) -> IssuedKey {
        let issued = self
            .app
            .issue_key_with_reason(
                self.admin(),
                &key_req(role, principal, grants),
                Some("a key"),
            )
            .await
            .expect("issued");
        // A reviewer counts as the person its principal names (ADR-0097
        // d12, d17).
        if role == Role::Reviewer {
            counted::link_to(&self.app, self.admin(), issued.key_id, principal).await;
        }
        issued
    }

    async fn workspace(&self, node: &str, parent: &str) {
        self.app
            .create_workspace(self.admin(), &name(node), Some(by_name(parent)), None)
            .await
            .expect("a workspace");
    }

    /// `node` moved one rung up: the admin proposes, the reviewer ratifies.
    async fn climb(&self, node: &str, to: WorkspaceStatus) {
        let proposal = self
            .app
            .propose_transition(self.admin(), by_name(node), to)
            .await
            .expect("a proposal");
        self.app
            .ratify_transition(
                self.reviewer.value.as_str(),
                by_name(node),
                proposal.transition_id,
            )
            .await
            .expect("ratified");
    }

    async fn written(&self, idem: &str, body: &str, node: &str, kind: MemoryKind) -> Uuid {
        self.app
            .submit_memory(self.writer.value.as_str(), &submit(idem, body, node, kind))
            .await
            .expect("submit")
            .receipt
            .memory_id
    }

    async fn approved(&self, idem: &str, body: &str, node: &str, kind: MemoryKind) -> Uuid {
        let id = self.written(idem, body, node, kind).await;
        self.app
            .post_verdict(self.reviewer.value.as_str(), id, Verdict::Approve, None)
            .await
            .expect("approve");
        id
    }

    async fn mark(
        &self,
        by: &Caller,
        node: &str,
        public: bool,
    ) -> Result<oml_app::types::PublicOutcome, AppError> {
        self.app
            .set_workspace_public(
                Bearer::Session(by),
                by_name(node),
                public,
                Some("every agent of the tenant reads the glossary"),
            )
            .await
    }

    async fn alice_marks(&self, node: &str) {
        let alice = self.admin_session("alice").await;
        let outcome = self.mark(&alice, node, true).await.expect("marked");
        assert_eq!(outcome.state, AdminActState::Effective);
    }

    async fn recalled(&self, key: &str, req: &RecallRequest) -> Vec<Uuid> {
        self.app
            .recall(key, req)
            .await
            .expect("recall")
            .hits
            .iter()
            .map(|h| h.served.memory.memory_id)
            .collect()
    }
}

async fn rig_over(store: MemStore) -> Rig {
    let clock = FixedClock::new();
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("root-admin"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    for (node, parent) in [
        ("team", "universe"),
        ("shared", "universe"),
        ("shared-child", "shared"),
    ] {
        app.create_workspace(admin.as_str(), &name(node), Some(by_name(parent)), None)
            .await
            .expect("a workspace");
    }
    let issue = |role: Role, principal: &'static str, grants: Vec<GrantRequest>| {
        let app = &app;
        let admin = admin.clone();
        async move {
            app.issue_key_with_reason(
                admin.as_str(),
                &key_req(role, principal, grants),
                Some("a key"),
            )
            .await
            .expect("issued")
        }
    };
    let reviewer = issue(
        Role::Reviewer,
        "rhea",
        vec![
            grant("shared", Role::Reviewer),
            grant("shared", Role::Reader),
        ],
    )
    .await;
    let writer = issue(
        Role::Contributor,
        "will",
        vec![
            grant("shared", Role::Reader),
            grant("shared", Role::Submitter),
        ],
    )
    .await;
    let reader = issue(Role::Reader, "ruth", vec![grant("team", Role::Reader)]).await;
    // The reviewer counts as the person rhea, linked by alice, the one
    // admin person: an instruction verdict and a ladder ratification need
    // a counted person (ADR-0097 d12, d17).
    counted::entry_of(&app, admin.as_str(), "alice", true).await;
    counted::link_to(&app, admin.as_str(), reviewer.key_id, "rhea").await;
    let mut r = Rig {
        app,
        store,
        clock,
        boot,
        reviewer,
        writer,
        reader,
        approved: Uuid::nil(),
        held: Uuid::nil(),
    };
    r.approved = r
        .approved(
            "gloss",
            "the shared glossary names every term",
            "shared",
            MemoryKind::Memory,
        )
        .await;
    r.held = r
        .written(
            "draft",
            "the shared glossary draft names a term",
            "shared",
            MemoryKind::Memory,
        )
        .await;
    r.climb("shared", WorkspaceStatus::Protected).await;
    r
}

/// A tenant that stays `sole_admin` with alice its one admin person.
async fn rig() -> Rig {
    rig_over(MemStore::new()).await
}

/// A tenant with two admin persons, alice and bob, accepted eight days ago.
async fn two_admins() -> Rig {
    let r = rig().await;
    r.admin_person("bob").await;
    r.clock.advance(8 * DAY);
    r
}

#[tokio::test]
async fn a_reader_key_reads_the_approved_items_of_a_public_workspace_it_holds_no_grant_on() {
    let r = rig().await;
    let before = r
        .app
        .get_memory(r.reader.value.as_str(), r.approved)
        .await
        .unwrap_err();
    assert!(matches!(before, AppError::NotFound), "{before:?}");
    r.alice_marks("shared").await;
    let served = r
        .app
        .get_memory(r.reader.value.as_str(), r.approved)
        .await
        .expect("read through the flag");
    assert_eq!(served.memory.memory_id, r.approved);
    let listed = r
        .app
        .list_memories(r.reader.value.as_str(), &list(Some("shared")))
        .await
        .expect("a listing named at the public workspace");
    assert_eq!(
        listed
            .items
            .iter()
            .map(|m| m.memory.memory_id)
            .collect::<Vec<_>>(),
        vec![r.approved]
    );
}

#[tokio::test]
async fn every_default_recall_searches_every_public_workspace_the_key_reads() {
    let r = rig().await;
    assert!(
        r.recalled(r.reader.value.as_str(), &recall("glossary", Some("team")))
            .await
            .is_empty()
    );
    r.alice_marks("shared").await;
    let hits = r
        .recalled(r.reader.value.as_str(), &recall("glossary", Some("team")))
        .await;
    assert_eq!(hits, vec![r.approved]);
}

#[tokio::test]
async fn a_hit_from_a_public_workspace_outside_the_scope_carries_via_public() {
    let r = rig().await;
    r.alice_marks("shared").await;
    let result = r
        .app
        .recall(r.reader.value.as_str(), &recall("glossary", Some("team")))
        .await
        .expect("recall");
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].source, Some(Via::Public));
    assert_eq!(
        result.hits[0].served.workspace_path.as_deref(),
        Some("universe.shared")
    );
    let sources = result
        .sources
        .expect("search.sources lists the public node");
    assert_eq!(sources.mode, "scope");
    assert_eq!(sources.searched.len(), 1);
    assert_eq!(sources.searched[0].via, Via::Public);
    assert_eq!(sources.searched[0].workspace_path, "universe.shared");

    // Scoped at the public node itself, it is the scope: counted once and
    // no source report.
    let at_node = r
        .app
        .recall(r.reader.value.as_str(), &recall("glossary", Some("shared")))
        .await
        .expect("recall at the node");
    assert_eq!(at_node.hits.len(), 1);
    assert_eq!(at_node.hits[0].source, None);
    assert!(at_node.sources.is_none());
}

#[tokio::test]
async fn a_tenant_without_public_workspaces_recalls_exactly_as_before() {
    let r = rig().await;
    let result = r
        .app
        .recall(r.writer.value.as_str(), &recall("glossary", Some("shared")))
        .await
        .expect("recall");
    assert!(result.sources.is_none());
    assert!(result.hits.iter().all(|h| h.source.is_none()));
    // Marked and cleared, the answer is as before again.
    r.alice_marks("shared").await;
    let alice = r.admin_session("alice").await;
    r.mark(&alice, "shared", false).await.expect("cleared");
    let again = r
        .app
        .recall(r.reader.value.as_str(), &recall("glossary", Some("team")))
        .await
        .expect("recall");
    assert!(again.hits.is_empty());
    assert!(again.sources.is_none());
}

#[tokio::test]
async fn a_public_workspaces_steering_and_guardrails_do_not_apply_at_another_scope() {
    let r = rig().await;
    let steering = r
        .approved(
            "steer",
            "always cite the shared glossary",
            "shared",
            MemoryKind::Steering,
        )
        .await;
    r.alice_marks("shared").await;
    let result = r
        .app
        .recall(r.reader.value.as_str(), &recall("glossary", Some("team")))
        .await
        .expect("recall");
    assert!(
        result
            .steering
            .iter()
            .all(|s| s.memory.memory_id != steering),
        "a public node's steering never steers a recall elsewhere"
    );
    assert!(
        result
            .hits
            .iter()
            .all(|h| h.served.memory.memory_id != steering)
    );
    assert!(
        result
            .hits
            .iter()
            .all(|h| h.served.memory.kind != MemoryKind::Guardrail)
    );
}

#[tokio::test]
async fn a_sources_list_searches_a_public_workspace_only_when_it_names_it() {
    let r = rig().await;
    r.app
        .set_federation_policy(
            r.admin(),
            FederationPolicyChange {
                enabled: Some(true),
                default_sources: None,
                max_sources: None,
            },
        )
        .await
        .expect("federation on");
    // `docs`, protected, a trusted source the reader reads by its grant.
    r.workspace("docs", "universe").await;
    let docs_reviewer = r
        .key(
            Role::Reviewer,
            "rhea-docs",
            vec![grant("docs", Role::Reviewer)],
        )
        .await;
    let proposal = r
        .app
        .propose_transition(r.admin(), by_name("docs"), WorkspaceStatus::Protected)
        .await
        .expect("a proposal");
    r.app
        .ratify_transition(
            docs_reviewer.value.as_str(),
            by_name("docs"),
            proposal.transition_id,
        )
        .await
        .expect("ratified");
    let reader = r
        .key(
            Role::Reader,
            "rita",
            vec![grant("team", Role::Reader), grant("docs", Role::Reader)],
        )
        .await;
    r.alice_marks("shared").await;
    let mut req = recall("glossary", Some("team"));
    req.sources = Some(Sources::List(vec!["docs".to_owned()]));
    assert!(r.recalled(reader.value.as_str(), &req).await.is_empty());
    req.sources = Some(Sources::List(vec!["shared".to_owned()]));
    let result = r
        .app
        .recall(reader.value.as_str(), &req)
        .await
        .expect("a list naming it");
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].source, Some(Via::Public));
}

#[tokio::test]
async fn the_listing_without_a_workspace_does_not_list_public_items() {
    let r = rig().await;
    let mut req = key_req(Role::Reader, "dana", vec![grant("team", Role::Reader)]);
    req.default_workspace = Some(by_name("team"));
    let dana = r
        .app
        .issue_key_with_reason(r.admin(), &req, Some("a key"))
        .await
        .expect("issued");
    r.alice_marks("shared").await;
    let listed = r
        .app
        .list_memories(dana.value.as_str(), &list(None))
        .await
        .expect("the default workspace's listing");
    assert!(
        listed
            .items
            .iter()
            .all(|m| m.memory.memory_id != r.approved)
    );
}

#[tokio::test]
async fn a_public_workspace_cannot_be_a_keys_default_workspace() {
    let r = rig().await;
    r.alice_marks("shared").await;
    let mut req = key_req(Role::Reader, "dana", vec![grant("team", Role::Reader)]);
    req.default_workspace = Some(by_name("shared"));
    let err = r
        .app
        .issue_key_with_reason(r.admin(), &req, Some("a key"))
        .await
        .unwrap_err();
    assert!(
        !matches!(err, AppError::Store(_)),
        "a default the grants do not cover is refused: {err:?}"
    );
}

#[tokio::test]
async fn a_submitter_key_recalls_nothing_from_a_public_workspace() {
    let r = rig().await;
    let submitter = r
        .key(Role::Submitter, "sam", vec![grant("team", Role::Submitter)])
        .await;
    r.alice_marks("shared").await;
    let err = r
        .app
        .get_memory(submitter.value.as_str(), r.approved)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::NotFound | AppError::VerbCeiling { .. }),
        "{err:?}"
    );
    let recalled = r
        .app
        .recall(submitter.value.as_str(), &recall("glossary", Some("team")))
        .await;
    assert!(
        recalled.is_err() || recalled.expect("recall").hits.is_empty(),
        "a submitter recalls nothing"
    );
}

#[tokio::test]
async fn an_admin_key_reads_no_content_of_a_public_workspace() {
    let r = rig().await;
    r.alice_marks("shared").await;
    let err = r
        .app
        .recall(r.admin(), &recall("glossary", Some("shared")))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::VerbCeiling { .. }), "{err:?}");
}

#[tokio::test]
async fn a_public_workspace_never_serves_a_held_version_to_a_key_that_did_not_write_it() {
    let r = rig().await;
    r.alice_marks("shared").await;
    let err = r
        .app
        .get_memory(r.reader.value.as_str(), r.held)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let mut req = recall("draft", Some("team"));
    req.include_unreviewed = Some(true);
    assert!(
        r.recalled(r.reader.value.as_str(), &req)
            .await
            .iter()
            .all(|id| *id != r.held)
    );
}

#[tokio::test]
async fn a_public_workspace_serves_principal_and_readable_lineage() {
    let r = rig().await;
    r.alice_marks("shared").await;
    let served = r
        .app
        .get_memory(r.reader.value.as_str(), r.approved)
        .await
        .expect("read");
    assert!(!served.anonymous, "a public node's rows keep their author");
    assert_eq!(served.memory.author_principal, "will");
}

#[tokio::test]
async fn a_child_of_a_public_workspace_stays_private() {
    let r = rig().await;
    let child = r
        .approved(
            "child",
            "the child glossary names a private term",
            "shared-child",
            MemoryKind::Memory,
        )
        .await;
    r.alice_marks("shared").await;
    let err = r
        .app
        .get_memory(r.reader.value.as_str(), child)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    assert!(
        r.recalled(r.reader.value.as_str(), &recall("glossary", Some("shared")))
            .await
            .iter()
            .all(|id| *id != child)
    );
}

#[tokio::test]
async fn a_development_workspace_and_a_user_root_cannot_be_marked_public() {
    let r = rig().await;
    let alice = r.admin_session("alice").await;
    let err = r.mark(&alice, "team", true).await.unwrap_err();
    assert!(
        matches!(
            err,
            AppError::WorkspacePublicRefused {
                reason: PublicRefusal::Status,
                ..
            }
        ),
        "{err:?}"
    );
    // alice's own root exists once her session keys are issued.
    let err = r.mark(&alice, "alice", true).await.unwrap_err();
    assert!(
        matches!(
            err,
            AppError::WorkspacePublicRefused {
                reason: PublicRefusal::UserRoot,
                ..
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_frozen_workspace_can_be_marked_public_and_cleared() {
    let r = rig().await;
    r.workspace("archive", "universe").await;
    let reviewer = r
        .key(
            Role::Reviewer,
            "rhea-archive",
            vec![grant("archive", Role::Reviewer)],
        )
        .await;
    for to in [WorkspaceStatus::Protected, WorkspaceStatus::Frozen] {
        let proposal = r
            .app
            .propose_transition(r.admin(), by_name("archive"), to)
            .await
            .expect("a proposal");
        r.app
            .ratify_transition(
                reviewer.value.as_str(),
                by_name("archive"),
                proposal.transition_id,
            )
            .await
            .expect("ratified");
    }
    let alice = r.admin_session("alice").await;
    let set = r
        .mark(&alice, "archive", true)
        .await
        .expect("set on frozen");
    assert!(set.public);
    let cleared = r
        .mark(&alice, "archive", false)
        .await
        .expect("cleared on frozen");
    assert!(!cleared.public);
    let node = r
        .store
        .workspace(r.boot.tenant_id, set.workspace_id)
        .await
        .expect("read")
        .expect("held");
    assert!(!node.public);
    assert_eq!(node.public_act_id, Some(cleared.act_id));
}

#[tokio::test]
async fn marking_a_workspace_public_waits_on_a_second_admin_person() {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let outcome = r.mark(&alice, "shared", true).await.expect("a pending set");
    assert_eq!(outcome.state, AdminActState::PendingConfirmation);
    assert!(
        r.app
            .get_memory(r.reader.value.as_str(), r.approved)
            .await
            .is_err(),
        "a pending set has no effect"
    );
    let again = r.mark(&alice, "shared", true).await.unwrap_err();
    assert!(
        matches!(
            again,
            AppError::WorkspacePublicRefused {
                reason: PublicRefusal::Pending,
                act_id: Some(id),
                ..
            } if id == outcome.act_id
        ),
        "{again:?}"
    );
    let bob = r.admin_session("bob").await;
    r.app
        .confirm_admin_act(Bearer::Session(&bob), outcome.act_id, Some("confirmed"))
        .await
        .expect("bob confirms");
    r.app
        .get_memory(r.reader.value.as_str(), r.approved)
        .await
        .expect("read once confirmed");
    let events = r
        .store
        .events_range(r.boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    let last = events.last().expect("an event");
    assert_eq!(last.envelope.event_type, event_types::WORKSPACE_PUBLIC_SET);
    assert_eq!(last.envelope.payload["value"], true);
    assert_eq!(last.envelope.payload["from"], false);
    assert_eq!(last.envelope.payload["act_id"], outcome.act_id.to_string());
}

#[tokio::test]
async fn marking_a_workspace_public_takes_effect_at_once_under_sole_admin_marked() {
    let r = rig().await;
    let alice = r.admin_session("alice").await;
    let outcome = r.mark(&alice, "shared", true).await.expect("set");
    assert_eq!(outcome.state, AdminActState::Effective);
    let act = r
        .store
        .admin_act(r.boot.tenant_id, outcome.act_id)
        .await
        .expect("read")
        .expect("held");
    assert!(act.sole_admin);
    let info = r
        .app
        .workspace_info(r.admin(), Some(by_name("shared")))
        .await
        .expect("the workspace read");
    assert!(info.public.value);
    assert!(info.public.sole_admin);
    assert_eq!(info.public.act_id, Some(outcome.act_id));
}

#[tokio::test]
async fn clearing_public_takes_effect_at_once_and_declines_a_pending_set() {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let pending = r.mark(&alice, "shared", true).await.expect("a pending set");
    let cleared = r.mark(&alice, "shared", false).await.expect("a clear");
    assert_eq!(cleared.state, AdminActState::Effective);
    let declined = r
        .store
        .admin_act(r.boot.tenant_id, pending.act_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(declined.state, AdminActState::Declined);
    let bob = r.admin_session("bob").await;
    assert!(
        r.app
            .confirm_admin_act(Bearer::Session(&bob), pending.act_id, Some("confirmed"))
            .await
            .is_err(),
        "a declined set is final"
    );
    let err = r.mark(&alice, "shared", false).await.unwrap_err();
    assert!(
        matches!(
            err,
            AppError::WorkspacePublicRefused {
                reason: PublicRefusal::NotPublic,
                ..
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_reviewer_key_cannot_set_public() {
    let r = rig().await;
    let err = r
        .app
        .set_workspace_public(
            r.reviewer.value.as_str(),
            by_name("shared"),
            true,
            Some("a reviewer tries"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::PublicIsAdminOnly {
                role: Role::Reviewer
            }
        ),
        "{err:?}"
    );
    // An admin key that counts as no person is refused the same way.
    let err = r
        .app
        .set_workspace_public(r.admin(), by_name("shared"), true, Some("unlinked"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::PublicIsAdminOnly { role: Role::Admin }),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_global_deny_removes_a_public_workspace_from_default_recall() {
    let r = rig().await;
    r.alice_marks("shared").await;
    r.app
        .deny_key_with_reason(
            r.admin(),
            Reference::Id(r.reader.key_id),
            by_name("shared"),
            Some("this agent never reads the glossary"),
        )
        .await
        .expect("denied");
    assert!(
        r.recalled(r.reader.value.as_str(), &recall("glossary", Some("team")))
            .await
            .is_empty()
    );
    assert!(
        r.app
            .get_memory(r.reader.value.as_str(), r.approved)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_public_workspace_is_recalled_by_default_with_federation_off() {
    let r = rig().await;
    r.alice_marks("shared").await;
    // Federation is off by default on a new tenant.
    assert_eq!(
        r.recalled(r.reader.value.as_str(), &recall("glossary", Some("team")))
            .await,
        vec![r.approved]
    );
}

#[tokio::test]
async fn accept_federated_off_does_not_remove_a_public_workspace_from_default_recall() {
    let r = rig().await;
    r.alice_marks("shared").await;
    r.app
        .set_workspace_federation(r.admin(), by_name("shared"), false)
        .await
        .expect("declines federated reads");
    assert_eq!(
        r.recalled(r.reader.value.as_str(), &recall("glossary", Some("team")))
            .await,
        vec![r.approved]
    );
}

#[tokio::test]
async fn a_key_of_another_tenant_never_reads_a_public_workspace() {
    let store = MemStore::new();
    let r = rig_over(store.clone()).await;
    r.alice_marks("shared").await;
    let other = r
        .app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("globex"),
            admin_key_name: name("globex-admin"),
            admin_principal: "globex-admin".to_owned(),
        })
        .await
        .expect("a second tenant");
    r.app
        .create_workspace(
            other.admin_key.value.as_str(),
            &name("ops"),
            Some(by_name("universe")),
            None,
        )
        .await
        .expect("a workspace");
    let outsider = r
        .app
        .issue_key(
            other.admin_key.value.as_str(),
            &key_req(Role::Reader, "otto", vec![grant("ops", Role::Reader)]),
        )
        .await
        .expect("issued");
    let err = r
        .app
        .get_memory(outsider.value.as_str(), r.approved)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    assert!(
        r.recalled(outsider.value.as_str(), &recall("glossary", Some("ops")))
            .await
            .is_empty()
    );
}

/// A user contributor key for `user`, reading and writing its own root.
async fn user_writer(r: &Rig, user: &str) -> IssuedKey {
    let mut req = key_req(
        Role::Contributor,
        user,
        vec![grant(user, Role::Submitter), grant(user, Role::Reader)],
    );
    req.user = Some(name(user));
    r.app
        .issue_key_with_reason(r.admin(), &req, Some("a user key"))
        .await
        .expect("a user key")
}

#[tokio::test]
async fn a_user_key_reads_a_public_workspace_outside_its_root_and_writes_nothing_there() {
    let r = rig().await;
    let ula = user_writer(&r, "ula").await;
    r.alice_marks("shared").await;
    r.app
        .get_memory(ula.value.as_str(), r.approved)
        .await
        .expect("a user key reads the public node");
    let err = r
        .app
        .submit_memory(
            ula.value.as_str(),
            &submit(
                "ula-1",
                "ula writes in the shared glossary",
                "shared",
                MemoryKind::Memory,
            ),
        )
        .await
        .unwrap_err();
    assert!(
        !matches!(err, AppError::Store(_)),
        "the flag gives no write: {err:?}"
    );
}

#[tokio::test]
async fn a_user_roots_default_recall_searches_universe_and_every_public_workspace() {
    let r = rig().await;
    let ula = user_writer(&r, "ula").await;
    r.alice_marks("shared").await;
    let result = r
        .app
        .recall(ula.value.as_str(), &recall("glossary", Some("ula")))
        .await
        .expect("a recall at the user root");
    assert_eq!(
        result
            .hits
            .iter()
            .map(|h| (h.served.memory.memory_id, h.source))
            .collect::<Vec<_>>(),
        vec![(r.approved, Some(Via::Public))]
    );
}

#[tokio::test]
async fn a_public_universe_serves_its_claims_without_their_source() {
    let r = rig().await;
    let curator = r
        .key(
            Role::Contributor,
            "cora",
            vec![
                GrantRequest {
                    covers: Covers::Node,
                    ..grant("universe", Role::Submitter)
                },
                GrantRequest {
                    covers: Covers::Node,
                    ..grant("universe", Role::Reader)
                },
            ],
        )
        .await;
    let curator_reviewer = r
        .key(
            Role::Reviewer,
            "uma",
            vec![
                GrantRequest {
                    covers: Covers::Node,
                    ..grant("universe", Role::Reviewer)
                },
                GrantRequest {
                    covers: Covers::Node,
                    ..grant("universe", Role::Reader)
                },
            ],
        )
        .await;
    let placed = r
        .app
        .submit_memory(
            curator.value.as_str(),
            &submit(
                "uni-1",
                "the universe glossary names a curated term",
                "universe",
                MemoryKind::Memory,
            ),
        )
        .await
        .expect("placed")
        .receipt
        .memory_id;
    r.app
        .post_verdict(
            curator_reviewer.value.as_str(),
            placed,
            Verdict::Approve,
            None,
        )
        .await
        .expect("approved");
    r.alice_marks("universe").await;
    let served = r
        .app
        .get_memory(r.reader.value.as_str(), placed)
        .await
        .expect("a tenant key reads a public universe");
    assert!(served.anonymous, "universe keeps its no-author serving");
}

/// A user key resolves a bare name to a shared node when that node is
/// public, so a recall scoped by the name reaches it; a private shared
/// name stays unresolved, `access.not_found`, and is not disclosed
/// (ADR-0098 d2, d9; the operator's decision of 2026-11-27).
#[tokio::test]
async fn a_user_key_resolves_a_public_shared_node_by_its_bare_name() {
    let r = rig().await;
    let ula = user_writer(&r, "ula").await;
    let err = r
        .app
        .recall(ula.value.as_str(), &recall("glossary", Some("shared")))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    r.alice_marks("shared").await;
    let ids = r
        .recalled(ula.value.as_str(), &recall("glossary", Some("shared")))
        .await;
    assert_eq!(ids, vec![r.approved]);
}

/// A public node's path shows each ancestor the reading key cannot read as
/// the placeholder `…`, never its name, on a recall hit, the source report,
/// the workspace read and the tree read; `universe` is named, and an admin
/// key's tree read names every node (ADR-0098, answer 4 of 2026-11-28,
/// D28; review finding surface-10).
#[tokio::test]
async fn a_public_nodes_private_ancestors_are_placeholders_never_names() {
    let r = rig().await;
    let child = r
        .approved(
            "child",
            "the child glossary note names a term",
            "shared-child",
            MemoryKind::Memory,
        )
        .await;
    r.climb("shared-child", WorkspaceStatus::Protected).await;
    r.alice_marks("shared-child").await;
    let masked = "universe.\u{2026}.shared-child";
    let reader = r.reader.value.as_str();
    let result = r
        .app
        .recall(reader, &recall("child", Some("team")))
        .await
        .expect("recall");
    let hit = result
        .hits
        .iter()
        .find(|h| h.served.memory.memory_id == child)
        .expect("the public child's hit");
    assert_eq!(hit.served.workspace_path.as_deref(), Some(masked));
    let sources = result.sources.expect("the public node is a source");
    assert!(
        sources.searched.iter().any(|s| s.workspace_path == masked),
        "{:?}",
        sources.searched
    );
    assert!(
        !format!("{:?}", sources.searched).contains("universe.shared."),
        "the private ancestor is never named"
    );
    let info = r
        .app
        .workspace_info(reader, Some(by_name("shared-child")))
        .await
        .expect("the workspace read");
    assert_eq!(info.path, masked);
    let tree = r
        .app
        .workspace_tree(reader, by_name("shared-child"), None, 50)
        .await
        .expect("the tree read");
    assert_eq!(tree.items[0].path, masked);
    let admin = r
        .app
        .workspace_tree(r.admin(), by_name("shared"), None, 50)
        .await
        .expect("the admin's tree");
    assert!(
        admin.items.iter().any(|n| n.path == "universe.shared.shared-child"),
        "an admin key reads every name"
    );
}
