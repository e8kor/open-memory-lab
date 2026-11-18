//! User roots, user keys, node grants, the two kinds of workspace namespace,
//! curation into `universe` and its served view (ADR-0044), over the
//! in-memory doubles. Each test is named after the sentence of ADR-0044 d15
//! it proves.

use oml_app::App;
use oml_app::app::Capacity;
use oml_app::domain::EventTime;
use oml_app::domain::access::Covers;
use oml_app::domain::name::{Name, NamedKind, Reference};
use oml_app::domain::web::Email;
use oml_app::domain::{MemoryKind, MemoryStatus, Role, UsedBy, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::{
    AppError, Bearer, BootstrapRequest, BootstrapResult, GrantRequest, GuardrailRequest,
    GuardrailSet, IssueKeyRequest, IssuedKey, KeyLimitsChange, RecallRequest, SubmitRequest,
    SupersedeRequest, Verdict, WebEmailRequest, WebSessionRecord,
};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

/// The committed test key seed (docs/data/chain-format.md test vector key).
const TEST_SEED: [u8; 32] = [
    0x09, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

/// The app over a store the test also reads directly.
fn rig() -> (TestApp, MemStore) {
    let store = MemStore::new();
    (app_over(store.clone()), store)
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

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name the creation grammar admits")
}

fn by_name(value: &str) -> Reference {
    Reference::parse(value).expect("a reference")
}

fn fresh_key_name() -> Name {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    name(&format!(
        "user-key-{}",
        ISSUED.fetch_add(1, Ordering::Relaxed)
    ))
}

async fn bootstrap(app: &TestApp) -> BootstrapResult {
    app.bootstrap_tenant(&BootstrapRequest {
        tenant_name: name("acme"),
        admin_key_name: name("root-admin"),
        admin_principal: "acme-admin".to_owned(),
    })
    .await
    .expect("bootstrap")
}

fn grant(node: Reference, role: Role) -> GrantRequest {
    GrantRequest {
        node,
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    }
}

fn node_grant(node: Reference, role: Role) -> GrantRequest {
    GrantRequest {
        covers: Covers::Node,
        ..grant(node, role)
    }
}

/// A reviewer key is a person's, since only a person's verdict counts on a
/// steering or guardrail version (ADR-0080 d16); every other key is an
/// agent's.
fn user_req(role: Role, user: &str, principal: &str, grants: Vec<GrantRequest>) -> IssueKeyRequest {
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
        user: Some(name(user)),
        connect_code: false,
        guardrail_scope: None,
        expiry: None,
        requests_per_minute: None,
    }
}

/// A writer key of `user`, writer and reader on its root: what `oml keys
/// issue-agent --user` sends (ADR-0044 d11).
async fn user_writer(app: &TestApp, admin: &str, user: &str, principal: &str) -> IssuedKey {
    app.issue_key(
        admin,
        &user_req(
            Role::Writer,
            user,
            principal,
            vec![
                grant(by_name(user), Role::Writer),
                grant(by_name(user), Role::Reader),
            ],
        ),
    )
    .await
    .expect("issue a user writer key")
}

async fn user_reviewer(app: &TestApp, admin: &str, user: &str, principal: &str) -> IssuedKey {
    app.issue_key(
        admin,
        &user_req(
            Role::Reviewer,
            user,
            principal,
            vec![grant(by_name(user), Role::Reviewer)],
        ),
    )
    .await
    .expect("issue a user reviewer key")
}

/// A tenant key, no user, with `grants`.
async fn tenant_key(
    app: &TestApp,
    admin: &str,
    role: Role,
    principal: &str,
    grants: Vec<GrantRequest>,
) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            user: None,
            ..user_req(role, "unused", principal, grants)
        },
    )
    .await
    .expect("issue a tenant key")
}

/// A curator: a tenant writer key with a writer node grant on `universe`
/// and reader grants on what it curates (ADR-0044 d7).
async fn curator(app: &TestApp, admin: &str, reads: &[&str]) -> IssuedKey {
    let mut grants = vec![
        node_grant(by_name("universe"), Role::Writer),
        node_grant(by_name("universe"), Role::Reader),
    ];
    grants.extend(reads.iter().map(|r| grant(by_name(r), Role::Reader)));
    tenant_key(app, admin, Role::Writer, "curator", grants).await
}

/// A tenant reviewer on the `universe` node: the second identity curation
/// needs (ADR-0044 d7).
async fn universe_reviewer(app: &TestApp, admin: &str, principal: &str) -> IssuedKey {
    tenant_key(
        app,
        admin,
        Role::Reviewer,
        principal,
        vec![
            node_grant(by_name("universe"), Role::Reviewer),
            node_grant(by_name("universe"), Role::Reader),
        ],
    )
    .await
}

fn submit(idem: &str, body: &str, workspace: Option<&str>) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        anchor: None,
        origin: None,
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workspace: workspace.map(by_name),
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

async fn written(app: &TestApp, key: &str, req: &SubmitRequest) -> Uuid {
    app.submit_memory(key, req)
        .await
        .expect("submit")
        .receipt
        .memory_id
}

async fn approve(app: &TestApp, reviewer: &str, id: Uuid) {
    app.post_verdict(reviewer, id, Verdict::Approve, None)
        .await
        .expect("approve");
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

/// A promoted claim by alice's writer in alice's root, curated into
/// `universe` by a curator and approved there by a tenant reviewer: (alice's
/// writer, the source, the curator, the universe version).
struct Curated {
    alice: IssuedKey,
    source: Uuid,
    curator: IssuedKey,
    placed: Uuid,
}

async fn curated(app: &TestApp, boot: &BootstrapResult) -> Curated {
    let admin = &boot.admin_key.value;
    let alice = user_writer(app, admin, "alice", "alice/laptop").await;
    let alice_review = user_reviewer(app, admin, "alice", "alice/review").await;
    let source = written(
        app,
        &alice.value,
        &submit("src", "quartz resonators drift with heat", None),
    )
    .await;
    approve(app, &alice_review.value, source).await;
    let curator = curator(app, admin, &["alice"]).await;
    let placed = written(
        app,
        &curator.value,
        &SubmitRequest {
            on_similar: Default::default(),
            derives_from: vec![source],
            ..submit(
                "curated",
                "quartz resonators drift with temperature",
                Some("universe"),
            )
        },
    )
    .await;
    let reviewer = universe_reviewer(app, admin, "universe-reviewer").await;
    approve(app, &reviewer.value, placed).await;
    Curated {
        alice,
        source,
        curator,
        placed,
    }
}

// ---------------------------------------------------------------------------
// S1 — d4, d1, d2, d3, d12
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_node_grant_with_a_deny_entry_is_refused() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let crew = app
        .create_workspace(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("crew");
    let err = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                user: None,
                ..user_req(
                    Role::Reader,
                    "unused",
                    "reader",
                    vec![GrantRequest {
                        deny: vec![crew.workspace.workspace_id.into()],
                        ..node_grant(by_name("universe"), Role::Reader)
                    }],
                )
            },
        )
        .await
        .expect_err("a node grant takes no deny entry");
    assert!(
        matches!(err, AppError::NodeGrantDeny { grant_index: 0 }),
        "{err:?}"
    );
}

#[tokio::test]
async fn the_first_key_naming_a_user_creates_its_root_under_universe() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let alice = user_writer(&app, &boot.admin_key.value, "alice", "alice/laptop").await;
    let user = alice.user.clone().expect("a user key names its root");
    assert_eq!(user.name.as_str(), "alice");
    let root = store
        .workspace(boot.tenant_id, user.id)
        .await
        .expect("read")
        .expect("the root exists");
    assert!(root.user_root);
    assert_eq!(root.parent_id, Some(boot.root_workspace_id));
    assert_eq!(root.namespace, None, "its name is in the shared namespace");
    assert_eq!(alice.default_workspace, Some(user.id));
    // The root's workspace.create precedes key.issued, both by the issuing
    // admin, and carries user_root: true; key.issued names the root.
    let events = store
        .events_range(boot.tenant_id, 1, 1000)
        .await
        .expect("journal");
    let create = events
        .iter()
        .position(|e| {
            e.envelope.event_type == event_types::WORKSPACE_CREATE
                && e.envelope.payload["workspace_id"] == user.id.to_string()
        })
        .expect("workspace.create of the root");
    assert_eq!(events[create].envelope.payload["user_root"], true);
    assert_eq!(
        events[create].envelope.api_key_id,
        Some(boot.admin_key.key_id)
    );
    let issued = events
        .iter()
        .position(|e| {
            e.envelope.event_type == event_types::KEY_ISSUED
                && e.envelope.payload["key_id"] == alice.key_id.to_string()
        })
        .expect("key.issued");
    assert!(create < issued);
    assert_eq!(events[issued].envelope.payload["user"], user.id.to_string());
    let stored = store
        .key(boot.tenant_id, alice.key_id)
        .await
        .expect("read")
        .expect("key");
    assert_eq!(stored.user, Some(user.id));
}

#[tokio::test]
async fn a_second_key_naming_the_user_joins_its_root() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let first = user_writer(&app, admin, "alice", "alice/laptop").await;
    let second = user_reviewer(&app, admin, "alice", "alice/review").await;
    assert_eq!(first.user, second.user);
    let creates = store
        .events_range(boot.tenant_id, 1, 1000)
        .await
        .expect("journal")
        .into_iter()
        .filter(|e| {
            e.envelope.event_type == event_types::WORKSPACE_CREATE
                && e.envelope.payload["user_root"] == true
        })
        .count();
    assert_eq!(creates, 1, "one root for one user");
}

#[tokio::test]
async fn a_user_named_like_a_shared_workspace_is_name_taken() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    app.create_workspace(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workspace");
    let before = store
        .chain_head(boot.tenant_id)
        .await
        .expect("head")
        .expect("a head")
        .seq;
    let err = app
        .issue_key(
            admin,
            &user_req(
                Role::Writer,
                "crew",
                "crew/laptop",
                vec![grant(by_name("crew"), Role::Writer)],
            ),
        )
        .await
        .expect_err("a shared name is no user's");
    assert!(
        matches!(err, AppError::NameTaken { kind: NamedKind::Workspace, ref name } if name.as_str() == "crew"),
        "{err:?}"
    );
    let err = app
        .issue_key(admin, &user_req(Role::Writer, "universe", "u", Vec::new()))
        .await
        .expect_err("universe is no user");
    assert!(matches!(err, AppError::UserRefused(_)), "{err:?}");
    let after = store
        .chain_head(boot.tenant_id)
        .await
        .expect("head")
        .expect("a head")
        .seq;
    assert_eq!(before, after, "nothing is journalled");
}

#[tokio::test]
async fn user_on_an_admin_key_is_refused() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let err = app
        .issue_key(admin, &user_req(Role::Admin, "alice", "alice", Vec::new()))
        .await
        .expect_err("an admin key acts for no one's root");
    assert!(matches!(err, AppError::UserRefused(_)), "{err:?}");
    let err = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                used_by: Some(UsedBy::Service),
                ..user_req(Role::Reader, "alice", "svc", Vec::new())
            },
        )
        .await
        .expect_err("a service key acts for a process");
    assert!(matches!(err, AppError::UserRefused(_)), "{err:?}");
    assert_eq!(
        store
            .resolve_name(boot.tenant_id, NamedKind::Workspace, &name("alice"))
            .await
            .expect("resolve"),
        None,
        "no root was created"
    );
}

#[tokio::test]
async fn a_user_keys_grant_outside_its_root_is_refused_as_issuer_ceiling() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    app.create_workspace(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workspace");
    user_writer(&app, admin, "bob", "bob/laptop").await;
    for outside in ["crew", "universe", "bob"] {
        let err = app
            .issue_key(
                admin,
                &user_req(
                    Role::Writer,
                    "alice",
                    "alice/laptop",
                    vec![
                        grant(by_name("alice"), Role::Writer),
                        // A user key's bare names resolve in its own
                        // namespace, so a shared node is named by id.
                        grant(
                            if outside == "universe" {
                                by_name(outside)
                            } else {
                                Reference::Id(
                                    store
                                        .resolve_name(
                                            boot.tenant_id,
                                            NamedKind::Workspace,
                                            &name(outside),
                                        )
                                        .await
                                        .expect("resolve")
                                        .expect("a shared node"),
                                )
                            },
                            Role::Reader,
                        ),
                    ],
                ),
            )
            .await
            .expect_err("a grant outside alice's root");
        match err {
            AppError::GrantOutsideUserRoot {
                role,
                grant_index,
                ref user,
                admissible,
            } => {
                assert_eq!(role, Role::Reader);
                assert_eq!(grant_index, 1);
                assert_eq!(user.as_str(), "alice");
                assert_eq!(admissible, &[Role::Reader, Role::Writer]);
            }
            other => panic!("{outside}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn issuance_adds_a_reader_node_grant_on_universe_to_every_user_key() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    for key in [
        user_writer(&app, admin, "alice", "alice/laptop").await,
        user_reviewer(&app, admin, "alice", "alice/review").await,
    ] {
        let added: Vec<_> = key
            .access
            .grants
            .iter()
            .filter(|g| g.grant.node == boot.root_workspace_id)
            .collect();
        assert_eq!(added.len(), 1, "one grant on universe");
        assert_eq!(added[0].grant.role, Role::Reader);
        assert_eq!(added[0].grant.covers, Covers::Node);
        assert!(added[0].grant.deny.is_empty());
        let stored = store
            .key_access(boot.tenant_id, key.key_id)
            .await
            .expect("read")
            .expect("access");
        assert_eq!(stored.grants, key.access.grants, "echoed as stored");
    }
    // A tenant key gets exactly the grants it is sent.
    let tenant = tenant_key(
        &app,
        admin,
        Role::Reader,
        "tenant-reader",
        vec![grant(by_name("alice"), Role::Reader)],
    )
    .await;
    assert_eq!(tenant.access.grants.len(), 1);
    assert_eq!(tenant.user, None);
}

// ---------------------------------------------------------------------------
// S2 — d5, d6
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_users_each_create_a_workspace_named_project() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    let a = app
        .create_workspace(&alice.value, &name("project"), None, None)
        .await
        .expect("alice's project");
    let b = app
        .create_workspace(&bob.value, &name("project"), None, None)
        .await
        .expect("bob's project");
    assert_eq!(a.path, "universe.alice.project");
    assert_eq!(b.path, "universe.bob.project");
    assert_ne!(a.workspace.workspace_id, b.workspace.workspace_id);
    // A second one in one user's namespace is taken, and so is the user's
    // own name and universe there.
    for taken in ["project", "alice", "universe"] {
        let err = app
            .create_workspace(&alice.value, &name(taken), Some(by_name("project")), None)
            .await
            .expect_err("taken in alice's namespace");
        assert!(
            matches!(err, AppError::NameTaken { .. }),
            "{taken}: {err:?}"
        );
    }
    // The shared namespace still holds a shared `project` apart.
    app.create_workspace(admin, &name("project"), Some(by_name("universe")), None)
        .await
        .expect("a shared project");
}

#[tokio::test]
async fn a_user_keys_bare_name_resolves_in_its_own_root() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    app.create_workspace(admin, &name("project"), Some(by_name("universe")), None)
        .await
        .expect("a shared project");
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let mine = app
        .create_workspace(&alice.value, &name("project"), None, None)
        .await
        .expect("alice's project");
    let info = app
        .workspace_info(alice.value.as_str(), Some(by_name("project")))
        .await
        .expect("read by bare name");
    assert_eq!(info.workspace.workspace_id, mine.workspace.workspace_id);
    assert_eq!(info.path, "universe.alice.project");
    // Its own root's name and universe resolve too.
    let root = app
        .workspace_info(alice.value.as_str(), Some(by_name("alice")))
        .await
        .expect("its root");
    assert!(root.workspace.user_root);
    app.workspace_info(alice.value.as_str(), Some(by_name("universe")))
        .await
        .expect("universe by its node grant");
    // A tenant key's bare name is the shared one.
    let tenant = tenant_key(
        &app,
        admin,
        Role::Reader,
        "tenant",
        vec![grant(by_name("universe"), Role::Reader)],
    )
    .await;
    let shared = app
        .workspace_info(tenant.value.as_str(), Some(by_name("project")))
        .await
        .expect("the shared project");
    assert_eq!(shared.path, "universe.project");
}

#[tokio::test]
async fn a_qualified_name_resolves_for_an_admin() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    let project = app
        .create_workspace(&alice.value, &name("project"), None, None)
        .await
        .expect("alice's project");
    let info = app
        .workspace_info(admin.as_str(), Some(by_name("alice.project")))
        .await
        .expect("an admin reads it by its qualified name");
    assert_eq!(info.workspace.workspace_id, project.workspace.workspace_id);
    // Its bare name is not in the shared namespace.
    assert!(matches!(
        app.workspace_info(admin.as_str(), Some(by_name("project")))
            .await,
        Err(AppError::NotFound)
    ));
    // Bob's key resolves alice's qualified name and is told nothing more
    // than for an id it cannot see.
    assert!(matches!(
        app.workspace_info(bob.value.as_str(), Some(by_name("alice.project")))
            .await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn a_three_label_value_is_still_a_path_and_refused() {
    for value in ["universe.alice.project", "universe.alice", "alice.a.b"] {
        assert!(Reference::parse(value).is_err(), "{value}");
    }
}

#[tokio::test]
async fn a_user_key_creates_a_workspace_under_its_root_by_default() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let alice = user_writer(&app, &boot.admin_key.value, "alice", "alice/laptop").await;
    let root = alice.user.clone().expect("root").id;
    let created = app
        .create_workspace(&alice.value, &name("project"), None, None)
        .await
        .expect("created under the root");
    assert_eq!(created.workspace.parent_id, Some(root));
    assert_eq!(created.workspace.namespace, Some(root));
    assert!(!created.workspace.user_root);
    let nested = app
        .create_workspace(
            &alice.value,
            &name("experiments"),
            Some(by_name("alice.project")),
            None,
        )
        .await
        .expect("nested by qualified parent");
    assert_eq!(nested.path, "universe.alice.project.experiments");
    let events = store
        .events_range(boot.tenant_id, 1, 1000)
        .await
        .expect("journal");
    let create = events
        .iter()
        .find(|e| {
            e.envelope.event_type == event_types::WORKSPACE_CREATE
                && e.envelope.payload["workspace_id"] == created.workspace.workspace_id.to_string()
        })
        .expect("journalled");
    assert_eq!(create.envelope.api_key_id, Some(alice.key_id));
    assert_eq!(create.envelope.payload["user_root"], false);
}

#[tokio::test]
async fn a_user_key_cannot_create_outside_its_root() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    app.create_workspace(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workspace");
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    // universe: read by the node grant, so grant_refused.
    let err = app
        .create_workspace(&alice.value, &name("x"), Some(by_name("universe")), None)
        .await
        .expect_err("not under universe");
    assert!(
        matches!(
            err,
            AppError::GrantRefused {
                needed_role: Role::Writer,
                ..
            }
        ),
        "{err:?}"
    );
    // A shared workspace, or another user's root: absent.
    let bob_root = bob.user.expect("root").id;
    for parent in [Reference::from(bob_root), by_name("bob")] {
        let err = app
            .create_workspace(&alice.value, &name("x"), Some(parent), None)
            .await
            .expect_err("not under bob");
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    let crew = store
        .resolve_name(boot.tenant_id, NamedKind::Workspace, &name("crew"))
        .await
        .expect("resolve")
        .expect("crew");
    let err = app
        .create_workspace(&alice.value, &name("x"), Some(crew.into()), None)
        .await
        .expect_err("not under a shared workspace");
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    // A reader user key, or a tenant writer key, holds no create verb.
    let reader = app
        .issue_key(
            admin,
            &user_req(
                Role::Reader,
                "alice",
                "alice/reader",
                vec![grant(by_name("alice"), Role::Reader)],
            ),
        )
        .await
        .expect("reader");
    assert!(matches!(
        app.create_workspace(&reader.value, &name("x"), None, None)
            .await,
        Err(AppError::VerbCeiling { .. })
    ));
    let tenant_writer = tenant_key(
        &app,
        admin,
        Role::Writer,
        "tw",
        vec![grant(by_name("crew"), Role::Writer)],
    )
    .await;
    assert!(matches!(
        app.create_workspace(
            &tenant_writer.value,
            &name("x"),
            Some(by_name("crew")),
            None
        )
        .await,
        Err(AppError::VerbCeiling { .. })
    ));
}

// ---------------------------------------------------------------------------
// S3 — d7, d8
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_user_keys_write_into_universe_is_universe_refused() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let alice = user_writer(&app, &boot.admin_key.value, "alice", "alice/laptop").await;
    let err = app
        .submit_memory(
            alice.value.as_str(),
            &submit("u", "a claim for everyone", Some("universe")),
        )
        .await
        .expect_err("users never write universe");
    assert!(matches!(err, AppError::UniverseRefused), "{err:?}");
    // A tenant writer without writer on universe keeps grant_refused.
    let tenant = tenant_key(
        &app,
        &boot.admin_key.value,
        Role::Writer,
        "tw",
        vec![node_grant(by_name("universe"), Role::Reader)],
    )
    .await;
    let err = app
        .submit_memory(
            tenant.value.as_str(),
            &submit("t", "a claim", Some("universe")),
        )
        .await
        .expect_err("no writer on universe");
    assert!(matches!(err, AppError::GrantRefused { .. }), "{err:?}");
}

#[tokio::test]
async fn a_curators_write_into_universe_is_held() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let curator = curator(&app, &boot.admin_key.value, &[]).await;
    let receipt = app
        .submit_memory(
            curator.value.as_str(),
            &submit("c", "a tenant-wide claim", Some("universe")),
        )
        .await
        .expect("a curator writes universe")
        .receipt;
    assert_eq!(receipt.workspace_id, boot.root_workspace_id);
    assert_eq!(receipt.state, "held");
}

#[tokio::test]
async fn a_curated_claim_is_never_promoted_by_the_sources_author() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let alice_review = user_reviewer(&app, admin, "alice", "alice/review").await;
    let source = written(&app, &alice.value, &submit("src", "a finding", None)).await;
    approve(&app, &alice_review.value, source).await;
    let curator = curator(&app, admin, &["alice"]).await;
    let placed = written(
        &app,
        &curator.value,
        &SubmitRequest {
            on_similar: Default::default(),
            derives_from: vec![source],
            ..submit("cur", "a finding, curated", Some("universe"))
        },
    )
    .await;
    // A tenant reviewer on universe that shares the source author's
    // principal is in the lineage closure; so is the curator's principal.
    for principal in ["alice/laptop", "curator"] {
        let excluded = universe_reviewer(&app, admin, principal).await;
        let err = app
            .post_verdict(&excluded.value, placed, Verdict::Approve, None)
            .await
            .expect_err("in the lineage closure");
        assert!(matches!(err, AppError::Verdict(_)), "{principal}: {err:?}");
    }
    // A user reviewer key never reaches universe at all.
    let err = app
        .post_verdict(&alice_review.value, placed, Verdict::Approve, None)
        .await
        .expect_err("alice's reviewer has no reviewer on universe");
    assert!(
        matches!(err, AppError::NotFound | AppError::GrantRefused { .. }),
        "{err:?}"
    );
    let held = store
        .memory(boot.tenant_id, placed)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(held.status, MemoryStatus::Held);
    let reviewer = universe_reviewer(&app, admin, "universe-reviewer").await;
    approve(&app, &reviewer.value, placed).await;
}

#[tokio::test]
async fn a_users_own_reviewer_key_may_review_its_agents_writes() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let alice_review = user_reviewer(&app, admin, "alice", "alice/review").await;
    let id = written(&app, &alice.value, &submit("w", "an agent's write", None)).await;
    let outcome = app
        .post_verdict(&alice_review.value, id, Verdict::Approve, None)
        .await
        .expect("the exclusion is per key and principal, not per user");
    assert_eq!(outcome.state, MemoryStatus::Promoted);
}

// ---------------------------------------------------------------------------
// S4 — d9, d10, d14
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_universe_claim_serves_no_principal_and_no_derives_from_to_a_reader() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let c = curated(&app, &boot).await;
    let bob = user_writer(&app, &boot.admin_key.value, "bob", "bob/laptop").await;
    let got = app
        .get_memory(bob.value.as_str(), c.placed)
        .await
        .expect("bob reads universe's claim");
    assert!(got.anonymous);
    assert_eq!(got.workspace_path.as_deref(), Some("universe"));
    assert_eq!(got.memory.workspace_id, boot.root_workspace_id);
    assert!(got.memory.author_principal.is_empty());
    assert_eq!(got.memory.author_key_id, Uuid::nil());
    assert!(got.memory.derives_from.is_empty());
    assert!(got.memory.lineage_key_ids.is_empty());
    assert!(got.memory.lineage_principals.is_empty());
    assert_eq!(got.replaces, None);
    // The same view in recall, at bob's root.
    let hits = app
        .recall(bob.value.as_str(), &recall("quartz", None))
        .await
        .expect("recall")
        .hits;
    let hit = hits
        .iter()
        .find(|h| h.memory.memory_id == c.placed)
        .expect("a universe hit");
    assert!(hit.anonymous);
    assert!(hit.memory.derives_from.is_empty());
    // The journal keeps every attribution field.
    let row = store
        .memory(boot.tenant_id, c.placed)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.derives_from, vec![c.source]);
    assert_eq!(row.author_key_id, c.curator.key_id);
}

#[tokio::test]
async fn the_sources_author_is_served_the_same_universe_view() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let c = curated(&app, &boot).await;
    let got = app
        .get_memory(c.alice.value.as_str(), c.placed)
        .await
        .expect("alice reads universe's claim");
    assert!(got.anonymous);
    assert!(got.memory.derives_from.is_empty(), "no link to her source");
    assert!(got.memory.author_principal.is_empty());
    // Her own source is unchanged and names nothing of the copy.
    let own = app
        .get_memory(c.alice.value.as_str(), c.source)
        .await
        .expect("her source");
    assert!(!own.anonymous);
    assert_eq!(own.memory.author_principal, "alice/laptop");
}

#[tokio::test]
async fn the_curator_reads_its_own_universe_row_in_full() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let c = curated(&app, &boot).await;
    let got = app
        .get_memory(c.curator.value.as_str(), c.placed)
        .await
        .expect("the curator reads its own row");
    assert!(!got.anonymous);
    assert_eq!(got.memory.author_principal, "curator");
    assert_eq!(got.memory.derives_from, vec![c.source]);
}

#[tokio::test]
async fn recall_inside_a_user_root_ranks_universe_hits_with_its_own() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let c = curated(&app, &boot).await;
    let alice_review = user_reviewer(&app, admin, "alice", "alice/review2").await;
    let project = app
        .create_workspace(&c.alice.value, &name("project"), None, None)
        .await
        .expect("project");
    let own = written(
        &app,
        &c.alice.value,
        &submit("own", "quartz crystal cuts", Some("project")),
    )
    .await;
    approve(&app, &alice_review.value, own).await;
    let hits: Vec<Uuid> = app
        .recall(c.alice.value.as_str(), &recall("quartz", Some("project")))
        .await
        .expect("recall at a scope in alice's root")
        .hits
        .iter()
        .map(|h| h.memory.memory_id)
        .collect();
    assert!(hits.contains(&own), "the scope's own hit");
    assert!(hits.contains(&c.placed), "universe's own claim");
    assert!(
        !hits.contains(&c.source),
        "the source lies outside the scope"
    );
    assert_eq!(project.path, "universe.alice.project");
    // A tenant key scoped to a shared workspace gets no universe join.
    let crew = app
        .create_workspace(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("crew");
    let tenant = tenant_key(
        &app,
        admin,
        Role::Reader,
        "tr",
        vec![
            grant(crew.workspace.workspace_id.into(), Role::Reader),
            node_grant(by_name("universe"), Role::Reader),
        ],
    )
    .await;
    let hits: Vec<Uuid> = app
        .recall(tenant.value.as_str(), &recall("quartz", Some("crew")))
        .await
        .expect("recall")
        .hits
        .iter()
        .map(|h| h.memory.memory_id)
        .collect();
    assert!(!hits.contains(&c.placed));
}

#[tokio::test]
async fn universe_steering_applies_in_every_user_root() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let curator = curator(&app, admin, &[]).await;
    let steering = written(
        &app,
        &curator.value,
        &SubmitRequest {
            on_similar: Default::default(),
            kind: MemoryKind::Steering,
            ..submit("steer", "cite every source", Some("universe"))
        },
    )
    .await;
    let reviewer = universe_reviewer(&app, admin, "universe-reviewer").await;
    approve(&app, &reviewer.value, steering).await;
    for user in ["alice", "bob"] {
        let key = user_writer(&app, admin, user, &format!("{user}/laptop")).await;
        let result = app
            .recall(key.value.as_str(), &recall("anything", None))
            .await
            .expect("recall at the user's root");
        let ids: Vec<Uuid> = result.steering.iter().map(|m| m.memory.memory_id).collect();
        assert_eq!(ids, vec![steering], "{user}");
        assert!(result.steering[0].anonymous);
    }
}

#[tokio::test]
async fn a_user_key_never_sees_another_users_root_in_the_tree() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    app.create_workspace(&bob.value, &name("secret"), None, None)
        .await
        .expect("bob's workspace");
    app.create_workspace(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workspace");
    let tree = app
        .workspace_tree(&alice.value, by_name("universe"), None, 100)
        .await
        .expect("alice reads the tree from universe");
    let paths: Vec<&str> = tree.items.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(paths, vec!["universe", "universe.alice"]);
    let children = app
        .workspace_info(alice.value.as_str(), Some(by_name("universe")))
        .await
        .expect("universe")
        .children;
    assert_eq!(
        children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["alice"]
    );
    // The admin sees every user root by name.
    let all = app
        .workspace_tree(admin, by_name("universe"), None, 100)
        .await
        .expect("admin");
    assert!(all.items.iter().any(|n| n.path == "universe.bob.secret"));
}

#[tokio::test]
async fn a_key_naming_a_root_that_is_not_a_user_root_is_served_nothing() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let alice = user_writer(&app, &boot.admin_key.value, "alice", "alice/laptop").await;
    // A store row that names a shared node as its user: forged here, since
    // no verb writes one.
    store.set_key_user(boot.tenant_id, alice.key_id, Some(boot.root_workspace_id));
    let err = app
        .whoami(&alice.value)
        .await
        .expect_err("a user key whose root is no user root");
    assert!(matches!(err, AppError::UserRootInvalid), "{err:?}");
}

#[tokio::test]
async fn whoami_and_the_key_read_serve_the_users_root() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let alice = user_writer(&app, &boot.admin_key.value, "alice", "alice/laptop").await;
    let me = app.whoami(&alice.value).await.expect("whoami");
    let user = me.user.expect("a user key");
    assert_eq!(user.name.as_str(), "alice");
    assert_eq!(Some(user.clone()), alice.user);
    let detail = app
        .key(&boot.admin_key.value, alice.key_id.into())
        .await
        .expect("key read");
    assert_eq!(detail.user, Some(user));
    let admin = app.whoami(&boot.admin_key.value).await.expect("whoami");
    assert_eq!(admin.user, None);
}

#[tokio::test]
async fn an_admin_reads_a_users_root_by_the_users_name_and_creates_none() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    assert_eq!(
        app.user_root(admin, &name("alice")).await.expect("read"),
        None
    );
    app.create_workspace(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workspace");
    assert!(matches!(
        app.user_root(admin, &name("crew")).await,
        Err(AppError::NameTaken { .. })
    ));
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let root = app
        .user_root(admin, &name("alice"))
        .await
        .expect("read")
        .expect("the root the first key created");
    assert_eq!(Some(root.workspace_id), alice.user.map(|u| u.id));
    assert!(matches!(
        app.user_root(&alice.value, &name("alice")).await,
        Err(AppError::VerbCeiling { .. })
    ));
}

// ---------------------------------------------------------------------------
// Guardrails under user roots (ADR-0037 d16, build step S6)
// ---------------------------------------------------------------------------

use oml_app::domain::guardrail::{GuardrailScope, Severity};

fn guardrail(idem: &str, body: &str, workspace: Option<&str>) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        kind: MemoryKind::Guardrail,
        severity: Some(Severity::Must),
        ..submit(idem, body, workspace)
    }
}

async fn set_of(app: &TestApp, key: &str) -> GuardrailSet {
    app.guardrails(key, &GuardrailRequest::default())
        .await
        .expect("the guardrail read")
}

fn texts(set: &GuardrailSet) -> Vec<&str> {
    set.guardrails.iter().map(|g| g.body.as_str()).collect()
}

/// A guardrail on `universe`, written by a curator and approved by a tenant
/// reviewer on `universe` (ADR-0044 d7).
async fn universe_guardrail(app: &TestApp, admin: &str, body: &str) -> Uuid {
    let curator = curator(app, admin, &[]).await;
    let id = written(
        app,
        &curator.value,
        &guardrail("u-g", body, Some("universe")),
    )
    .await;
    let reviewer = universe_reviewer(app, admin, "universe-reviewer").await;
    approve(app, &reviewer.value, id).await;
    id
}

#[tokio::test]
async fn a_user_key_with_scope_ancestors_loads_universes_guardrails_through_its_node_grant() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let alice_review = user_reviewer(&app, admin, "alice", "alice/review").await;
    let own = written(
        &app,
        &alice.value,
        &guardrail("a-g", "Run the tests first.", None),
    )
    .await;
    approve(&app, &alice_review.value, own).await;
    universe_guardrail(&app, admin, "Never push to main.").await;

    let set = set_of(&app, &alice.value).await;
    assert_eq!(set.guardrail_scope, GuardrailScope::Ancestors);
    assert_eq!(set.scope_path, "universe.alice");
    assert_eq!(texts(&set), ["Never push to main.", "Run the tests first."]);
    let paths: Vec<Option<&str>> = set
        .guardrails
        .iter()
        .map(|g| g.workspace_path.as_deref())
        .collect();
    assert_eq!(paths, [Some("universe"), Some("universe.alice")]);
}

#[tokio::test]
async fn a_user_key_with_scope_workspace_loads_no_universe_guardrail() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    universe_guardrail(&app, admin, "Never push to main.").await;
    let narrow = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                guardrail_scope: Some(GuardrailScope::Workspace),
                ..user_req(
                    Role::Reader,
                    "alice",
                    "alice/narrow",
                    vec![grant(by_name("alice"), Role::Reader)],
                )
            },
        )
        .await
        .expect("issue a narrow user key");
    let set = set_of(&app, &narrow.value).await;
    assert_eq!(set.guardrail_scope, GuardrailScope::Workspace);
    assert!(set.guardrails.is_empty(), "{:?}", texts(&set));
}

#[tokio::test]
async fn no_guardrail_of_another_users_root_is_ever_served() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    let bob_review = user_reviewer(&app, admin, "bob", "bob/review").await;
    let bobs = written(&app, &bob.value, &guardrail("b-g", "Bob's own rule.", None)).await;
    approve(&app, &bob_review.value, bobs).await;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let set = set_of(&app, &alice.value).await;
    assert!(set.guardrails.is_empty(), "{:?}", texts(&set));
    assert!(set.own_held.is_empty());
    // Named directly, bob's root is outside alice's grants.
    let err = app
        .guardrails(
            alice.value.as_str(),
            &GuardrailRequest {
                workspace: Some(Reference::Name(name("bob"))),
                include_own_held: false,
            },
        )
        .await
        .expect_err("another user's root");
    assert!(
        matches!(err, AppError::NotFound | AppError::GrantRefused { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_user_keys_guardrail_write_into_universe_is_refused_as_universe_curated() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let alice = user_writer(&app, &boot.admin_key.value, "alice", "alice/laptop").await;
    let err = app
        .submit_memory(
            alice.value.as_str(),
            &guardrail("u", "A rule for everyone.", Some("universe")),
        )
        .await
        .expect_err("universe is curated");
    assert!(matches!(err, AppError::UniverseRefused), "{err:?}");
}

#[tokio::test]
async fn a_universe_guardrail_is_served_with_origin_universe_its_verdicts_and_no_source() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    universe_guardrail(&app, admin, "Never push to main.").await;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let set = set_of(&app, &alice.value).await;
    let g = set.guardrails.first().expect("the universe guardrail");
    assert_eq!(g.workspace_path.as_deref(), Some("universe"));
    assert_eq!(g.depth, 0);
    // Its verdict's attestation and time, never a key (d18 Q6).
    assert_eq!(g.verdicts.len(), 1);
    assert_eq!(g.verdicts[0].used_by, UsedBy::Person);
}

#[tokio::test]
async fn the_users_reviewer_key_ratifies_a_guardrail_its_agents_propose_in_its_root() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let alice_review = user_reviewer(&app, admin, "alice", "alice/review").await;
    let id = written(
        &app,
        &alice.value,
        &guardrail("a-g", "Run the tests first.", None),
    )
    .await;
    assert_eq!(set_of(&app, &alice.value).await.own_held.len(), 1);
    let outcome = app
        .post_verdict(&alice_review.value, id, Verdict::Approve, None)
        .await
        .expect("the person's reviewer key is not the author");
    assert_eq!(outcome.state, MemoryStatus::Promoted);
    assert_eq!(
        texts(&set_of(&app, &alice.value).await),
        ["Run the tests first."]
    );
}

// ---------------------------------------------------------------------------
// The lifecycle and edges inside user roots (ADR-0046 d1, d2, d5)
// ---------------------------------------------------------------------------

/// A reviewer key of `user` that also reads its root: it proposes, ratifies
/// and reads the claims it links.
async fn user_curator(app: &TestApp, admin: &str, user: &str, principal: &str) -> IssuedKey {
    app.issue_key(
        admin,
        &user_req(
            Role::Reviewer,
            user,
            principal,
            vec![
                grant(by_name(user), Role::Reviewer),
                grant(by_name(user), Role::Reader),
            ],
        ),
    )
    .await
    .expect("issue a user reviewer key that reads its root")
}

/// Whether `err` is the ladder's `user_root` refusal.
fn refused_as_user_root(err: &AppError) -> bool {
    matches!(
        err,
        AppError::TransitionRefused {
            reason: oml_app::domain::LadderRefusal::UserRoot,
            ..
        }
    )
}

#[tokio::test]
async fn a_user_root_is_refused_protected_as_user_root() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let first = user_curator(&app, admin, "alice", "alice/first").await;
    let before = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    let err = app
        .propose_transition(
            &first.value,
            by_name("alice"),
            oml_app::domain::WorkspaceStatus::Protected,
        )
        .await
        .unwrap_err();
    assert!(refused_as_user_root(&err), "{err:?}");
    // An admin key passes on its role and meets the same ceiling.
    let err = app
        .propose_transition(
            admin,
            by_name("alice"),
            oml_app::domain::WorkspaceStatus::Protected,
        )
        .await
        .unwrap_err();
    assert!(refused_as_user_root(&err), "{err:?}");
    let after = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    assert_eq!(before, after, "a refused proposal journals nothing");
}

#[tokio::test]
async fn a_workspace_under_a_user_root_is_refused_protected() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let writer = user_writer(&app, admin, "alice", "alice/laptop").await;
    let first = user_curator(&app, admin, "alice", "alice/first").await;
    let project = app
        .create_workspace(writer.value.as_str(), &name("project"), None, None)
        .await
        .expect("alice creates a workspace")
        .workspace
        .workspace_id;
    let nested = app
        .create_workspace(
            writer.value.as_str(),
            &name("notes"),
            Some(project.into()),
            None,
        )
        .await
        .expect("alice creates a workspace under it")
        .workspace
        .workspace_id;
    for node in [project, nested] {
        let err = app
            .propose_transition(
                &first.value,
                node.into(),
                oml_app::domain::WorkspaceStatus::Protected,
            )
            .await
            .unwrap_err();
        assert!(refused_as_user_root(&err), "{err:?}");
    }
    // A shared workspace still climbs (ADR-0085 d17: production work
    // happens in shared workspaces).
    let shared = app
        .create_workspace(
            admin.as_str(),
            &name("team"),
            Some(boot.root_workspace_id.into()),
            None,
        )
        .await
        .expect("an admin creates a shared workspace")
        .workspace
        .workspace_id;
    let reviewer = tenant_key(
        &app,
        admin,
        Role::Reviewer,
        "team-reviewer",
        vec![
            grant(by_name("team"), Role::Reviewer),
            grant(by_name("team"), Role::Reader),
        ],
    )
    .await;
    app.propose_transition(
        &reviewer.value,
        shared.into(),
        oml_app::domain::WorkspaceStatus::Protected,
    )
    .await
    .expect("a shared workspace climbs to protected");
}

#[tokio::test]
async fn a_workspace_under_a_user_root_is_offered_no_ladder_move_and_names_user_root() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let writer = user_writer(&app, admin, "alice", "alice/laptop").await;
    let first = user_curator(&app, admin, "alice", "alice/first").await;
    let project = app
        .create_workspace(writer.value.as_str(), &name("project"), None, None)
        .await
        .expect("alice creates a workspace")
        .workspace
        .workspace_id;
    let (offers, refused) = app
        .workspace_offers_explained_for(first.value.as_str(), project.into())
        .await
        .expect("the offers");
    assert!(
        !offers
            .offers
            .iter()
            .any(|o| matches!(o, oml_app::app::WorkspaceOffer::Propose(_))),
        "{:?}",
        offers.offers
    );
    let protected = refused
        .iter()
        .find(|(act, _)| {
            *act == oml_app::app::WorkspaceOffer::Propose(
                oml_app::domain::WorkspaceStatus::Protected,
            )
        })
        .expect("proposing protected is refused");
    assert!(refused_as_user_root(&protected.1), "{:?}", protected.1);
}

#[tokio::test]
async fn a_pending_protected_proposal_in_a_user_root_is_refused_at_ratification() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let writer = user_writer(&app, admin, "alice", "alice/laptop").await;
    let first = user_curator(&app, admin, "alice", "alice/first").await;
    let second = user_curator(&app, admin, "alice", "alice/second").await;
    let project = app
        .create_workspace(writer.value.as_str(), &name("project"), None, None)
        .await
        .expect("alice creates a workspace")
        .workspace
        .workspace_id;
    // A proposal a build before the cap left pending there.
    let transition_id = Uuid::from_u128(0x393);
    store
        .seed_pending_transition(oml_app::types::TransitionRecord {
            transition_id,
            tenant_id: boot.tenant_id,
            workspace_id: project,
            from: oml_app::domain::WorkspaceStatus::Development,
            to: oml_app::domain::WorkspaceStatus::Protected,
            state: oml_app::types::TransitionState::Pending,
            proposer_key_id: first.key_id,
            proposer_principal: "alice/first".to_owned(),
            ratifier_key_id: None,
        })
        .expect("a pending proposal");
    let before = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    let err = app
        .ratify_transition(&second.value, project.into(), transition_id)
        .await
        .unwrap_err();
    assert!(refused_as_user_root(&err), "{err:?}");
    let after = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    assert_eq!(before, after, "a refused ratification journals nothing");
    let node = store
        .workspace(boot.tenant_id, project)
        .await
        .expect("read")
        .expect("the node");
    assert_eq!(node.status, oml_app::domain::WorkspaceStatus::Development);
}

/// Two session keys of one person act in a shared workspace: `<user>-admin`
/// proposes and `<user>-review`, designated there, ratifies; the
/// transition is journalled `same_user: true` (ADR-0055 d7; ADR-0046 d2),
/// the one place left where a ladder move can be one person's.
#[tokio::test]
async fn two_session_keys_of_one_person_move_a_shared_workspace_marked_same_user() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    app.create_workspace(
        admin,
        &name("team"),
        Some(boot.root_workspace_id.into()),
        None,
    )
    .await
    .expect("an admin creates a shared workspace");
    let alice = signed_in(&app, admin, "alice", true).await;
    let alice_admin = app
        .web_caller(&alice, Capacity::Admin)
        .await
        .expect("alice-admin");
    app.grant_review_capacity(&alice_admin, &name("alice"), by_name("team"))
        .await
        .expect("the review designation");
    let alice_review = app
        .web_caller(&alice, Capacity::Review)
        .await
        .expect("alice-review");
    let proposal = app
        .propose_transition(
            Bearer::Session(&alice_admin),
            by_name("team"),
            oml_app::domain::WorkspaceStatus::Protected,
        )
        .await
        .expect("alice-admin proposes");
    app.ratify_transition(
        Bearer::Session(&alice_review),
        by_name("team"),
        proposal.transition_id,
    )
    .await
    .expect("alice-review ratifies");
    let events = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    let ratified = events
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::WORKSPACE_STATUS_TRANSITION)
        .expect("the ratification");
    assert_eq!(ratified.envelope.payload["same_user"], true);
}

#[tokio::test]
async fn a_ratification_by_a_tenant_key_in_a_shared_workspace_is_journalled_not_same_user() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    app.create_workspace(
        admin.as_str(),
        &name("team"),
        Some(boot.root_workspace_id.into()),
        None,
    )
    .await
    .expect("an admin creates a shared workspace");
    let grants = || {
        vec![
            grant(by_name("team"), Role::Reviewer),
            grant(by_name("team"), Role::Reader),
        ]
    };
    let proposer = tenant_key(&app, admin, Role::Reviewer, "team-first", grants()).await;
    let ratifier = tenant_key(&app, admin, Role::Reviewer, "team-second", grants()).await;
    let proposal = app
        .propose_transition(
            &proposer.value,
            by_name("team"),
            oml_app::domain::WorkspaceStatus::Protected,
        )
        .await
        .expect("a proposal");
    app.ratify_transition(&ratifier.value, by_name("team"), proposal.transition_id)
        .await
        .expect("a ratification");
    let events = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    let ratified = events
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::WORKSPACE_STATUS_TRANSITION)
        .expect("the ratification");
    assert_eq!(ratified.envelope.payload["same_user"], false);
}

#[tokio::test]
async fn a_user_keys_claim_or_brief_edge_with_an_endpoint_in_universe_is_refused_grant_refused() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let c = curated(&app, &boot).await;
    let alice = user_curator(&app, admin, "alice", "alice/curator").await;
    let before = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();

    let refused = |err: AppError| {
        assert!(
            matches!(
                err,
                AppError::GrantRefused {
                    needed_role: Role::Reviewer,
                    ..
                }
            ),
            "{err:?}"
        );
    };
    refused(
        app.propose_claim_edge(
            &alice.value,
            &oml_app::types::ProposeClaimEdge {
                from_artifact_id: c.source,
                to_artifact_id: c.placed,
                label: "supports".to_owned(),
            },
            None,
        )
        .await
        .unwrap_err(),
    );
    refused(
        app.propose_brief_edge(
            &alice.value,
            &oml_app::types::ProposeBriefEdge {
                artifact_id: c.placed,
                workspace: by_name("alice"),
            },
            None,
        )
        .await
        .unwrap_err(),
    );
    assert_eq!(
        store
            .events_range(boot.tenant_id, 1, 10_000)
            .await
            .expect("journal")
            .len(),
        before,
        "nothing is journalled"
    );
    // Inside its own root, the same key proposes freely.
    app.propose_brief_edge(
        &alice.value,
        &oml_app::types::ProposeBriefEdge {
            artifact_id: c.source,
            workspace: by_name("alice"),
        },
        None,
    )
    .await
    .expect("a brief edge inside alice's root");
    let _ = (c.alice, c.curator);
}

/// The last event of `event_type` in the tenant's journal.
async fn last_event(store: &MemStore, tenant: Uuid, event_type: &str) -> serde_json::Value {
    store
        .events_range(tenant, 1, 10_000)
        .await
        .expect("journal")
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_type)
        .expect("the event")
        .envelope
        .payload
        .clone()
}

/// One definition of `same_user` for a claim's approval, a transition's
/// ratification and a claim edge's: true when both keys act for one person,
/// false for two people, and false whenever either key is a tenant key that
/// names no person (ADR-0046 d2, ADR-0055 d7). A tenant key starts no memory
/// in a user's root (ADR-0044 d16), so the tenant key is the reviewer of
/// alice's claim, and as an author it is a reviewer granted there whose
/// successor of her claim her own review key approves; a claim a tenant key
/// wrote there before that rule is approved with `same_user` false in the
/// port contracts' user-root suite.
#[tokio::test]
async fn same_user_is_one_rule_for_claim_approvals_transitions_and_edges() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let (admin, tenant) = (&boot.admin_key.value, boot.tenant_id);
    let alice_writer = user_writer(&app, admin, "alice", "alice/laptop").await;
    let alice_review = user_curator(&app, admin, "alice", "alice/review").await;
    let alice_other = user_curator(&app, admin, "alice", "alice/other").await;
    let crew_writer = tenant_key(
        &app,
        admin,
        Role::Writer,
        "crew/loader",
        vec![
            grant(by_name("alice"), Role::Writer),
            grant(by_name("alice"), Role::Reader),
        ],
    )
    .await;
    let crew_review = tenant_key(
        &app,
        admin,
        Role::Reviewer,
        "crew/reviewer",
        vec![
            grant(by_name("alice"), Role::Reviewer),
            grant(by_name("alice"), Role::Reader),
        ],
    )
    .await;
    let _bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    let bob_review = user_curator(&app, admin, "bob", "bob/review").await;

    // A crew key holding writer on alice's root starts no memory there.
    let refused = app
        .submit_memory(
            crew_writer.value.as_str(),
            &submit("crew-author", "Decision crew-author.", Some("alice")),
        )
        .await
        .expect_err("a memory is started in a user's root by that user's keys only");
    assert!(matches!(refused, AppError::UserRootRefused), "{refused:?}");

    // Claims: the person's own agent and reviewer, twice; a crew reviewer.
    let cases = [
        (&alice_writer, &alice_review, true, "own"),
        (&alice_writer, &alice_review, true, "own-again"),
        (&alice_writer, &crew_review, false, "crew-reviewer"),
    ];
    let mut promoted = Vec::new();
    for (author, reviewer, expected, tag) in cases {
        let id = written(
            &app,
            &author.value,
            &submit(tag, &format!("Decision {tag}."), Some("alice")),
        )
        .await;
        approve(&app, &reviewer.value, id).await;
        assert_eq!(
            last_event(&store, tenant, event_types::LIFECYCLE_PROMOTE).await["same_user"],
            expected,
            "claim {tag}"
        );
        promoted.push(id);
    }
    // Bob's reviewer key reaches nothing of alice's: two people are never
    // one, whatever the grants.
    assert!(
        app.post_verdict(&bob_review.value, promoted[0], Verdict::Approve, None)
            .await
            .is_err()
    );

    // Claim edges: proposed by one of alice's keys, ratified by another of
    // hers, then by a crew key.
    for (ratifier, expected, (from, to)) in [
        (&alice_other, true, (promoted[0], promoted[1])),
        (&crew_review, false, (promoted[0], promoted[2])),
    ] {
        let edge = app
            .propose_claim_edge(
                &alice_review.value,
                &oml_app::types::ProposeClaimEdge {
                    from_artifact_id: from,
                    to_artifact_id: to,
                    label: "supports".to_owned(),
                },
                None,
            )
            .await
            .expect("a proposal");
        app.ratify_claim_edge(&ratifier.value, edge.proposal.edge_id, None)
            .await
            .expect("a ratification");
        assert_eq!(
            last_event(&store, tenant, event_types::REVIEW_RATIFY).await["same_user"],
            expected,
            "edge ratified by {}",
            if expected { "alice" } else { "the crew" }
        );
    }

    // A tenant key as the author: the crew reviewer edits alice's claim by
    // proposal, which the rule still allows it, and alice's review key
    // approves the successor. Its author names no person.
    let successor = app
        .supersede(
            crew_review.value.as_str(),
            &SupersedeRequest {
                idempotency_key: "crew-successor".to_owned(),
                on_similar: Default::default(),
                target_id: promoted[1],
                body: "Decision own-again, as the crew reworded it.".to_owned(),
                kind: None,
                retirement: false,
                derives_from: Vec::new(),
                reason: None,
                workspace: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                anchor: None,
                format: None,
                title: None,
                origin: None,
                expected_version: 1,
            },
        )
        .await
        .expect("a tenant reviewer granted in alice's root edits by proposal")
        .receipt
        .memory_id;
    let verdict = app
        .post_verdict(&alice_review.value, successor, Verdict::Approve, None)
        .await
        .expect("alice's review key approves the crew's successor");
    assert!(!verdict.same_user, "a tenant author is no one's person");
    let journalled = last_event(&store, tenant, event_types::LIFECYCLE_SUPERSEDE).await;
    assert_eq!(journalled["new_artifact_id"], successor.to_string());
    assert_eq!(journalled["same_user"], false, "claim crew-author");
}

/// A person signed in once on the web: the session keys the first sign-in
/// ensures (ADR-0045 d6) and a live session of theirs.
async fn signed_in(app: &TestApp, admin: &str, user: &str, is_admin: bool) -> WebSessionRecord {
    let (entry, _) = app
        .allow_web_email(
            admin,
            &WebEmailRequest {
                email: Email::parse(&format!("{user}@example.com")).expect("an email"),
                user: name(user),
                admin: is_admin,
            },
        )
        .await
        .expect("the entry is added");
    app.web_ensure_keys(&entry).await.expect("the session keys");
    let token = app
        .web_session_open(&entry, 3600, None)
        .await
        .expect("the session opens")
        .token;
    app.web_session(&token)
        .await
        .expect("a read")
        .expect("a live session")
}

/// Each session key the web console gives a person is read in a user root
/// as that person's key and no one else's (ADR-0044 d16; ADR-0045 d6, d10).
/// In the person's own root the user-root rule refuses none of them: what
/// answers is the key's role or its grants. `<user>-web`, the one user key
/// among them, starts a memory in its own root and reads nothing of
/// another person's. `<user>-curate`, a tenant writer key with writer on
/// the `universe` node alone, writes there, is told `access.grant_refused`
/// in its person's root, and `submission.user_root_refused` in another
/// person's root it reads. `<user>-review` and `<user>-admin` are refused
/// a new memory by their role before any node is read, wherever it is
/// addressed; the review key reaches the rule through a supersession, which
/// the port contracts' user-root suite proves.
#[tokio::test]
async fn each_session_key_of_a_person_is_read_in_a_user_root_as_that_persons_key_and_no_one_elses()
{
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let alice = signed_in(&app, admin, "alice", true).await;
    signed_in(&app, admin, "bob", false).await;
    // Alice, an admin, designates herself: curation over her root and
    // bob's, and review on bob's (ADR-0045 d10).
    let alice_admin = app
        .web_caller(&alice, Capacity::Admin)
        .await
        .expect("alice-admin");
    for root in ["alice", "bob"] {
        app.grant_curation_capacity(&alice_admin, &name("alice"), by_name(root))
            .await
            .expect("the curation designation");
    }
    app.grant_review_capacity(&alice_admin, &name("alice"), by_name("bob"))
        .await
        .expect("the review designation");

    #[derive(Debug, PartialEq)]
    enum Answer {
        Held,
        NotFound,
        GrantRefused,
        UserRootRefused,
        VerbCeiling,
    }
    let cases = [
        (Capacity::Web, "alice", Answer::Held),
        (Capacity::Web, "bob", Answer::NotFound),
        (Capacity::Curate, "universe", Answer::Held),
        (Capacity::Curate, "alice", Answer::GrantRefused),
        (Capacity::Curate, "bob", Answer::UserRootRefused),
        (Capacity::Review, "alice", Answer::VerbCeiling),
        (Capacity::Review, "bob", Answer::VerbCeiling),
        (Capacity::Admin, "alice", Answer::VerbCeiling),
        (Capacity::Admin, "bob", Answer::VerbCeiling),
    ];
    for (i, (capacity, workspace, expected)) in cases.into_iter().enumerate() {
        let key = app
            .web_caller(&alice, capacity)
            .await
            .expect("alice's session key");
        let head = store
            .events_range(boot.tenant_id, 1, 10_000)
            .await
            .expect("journal")
            .len();
        let outcome = app
            .submit_memory(
                Bearer::Session(&key),
                &submit(
                    &format!("capacity-{i}"),
                    &format!("Decision capacity-{i}."),
                    Some(workspace),
                ),
            )
            .await;
        let answer = match &outcome {
            Ok(written) => {
                assert_eq!(written.receipt.state, "held");
                Answer::Held
            }
            Err(AppError::NotFound) => Answer::NotFound,
            Err(AppError::GrantRefused {
                needed_role: Role::Writer,
                ..
            }) => Answer::GrantRefused,
            Err(AppError::UserRootRefused) => Answer::UserRootRefused,
            Err(AppError::VerbCeiling { verb: "submit", .. }) => Answer::VerbCeiling,
            Err(other) => panic!("alice-{} in {workspace}: {other:?}", capacity.suffix()),
        };
        assert_eq!(
            answer,
            expected,
            "alice-{} in {workspace}",
            capacity.suffix()
        );
        let journalled = store
            .events_range(boot.tenant_id, 1, 10_000)
            .await
            .expect("journal")
            .len();
        assert_eq!(
            journalled - head,
            usize::from(expected == Answer::Held),
            "a refusal journals nothing"
        );
    }
}

// -- linked tenant keys (ADR-0085 d16, amended 2026-11-17; ADR-0082 d7) -----

/// A tenant reviewer key a person holds, with `grants`.
async fn person_tenant_key(
    app: &TestApp,
    admin: &str,
    principal: &str,
    grants: Vec<GrantRequest>,
) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            user: None,
            used_by: Some(UsedBy::Person),
            ..user_req(Role::Reviewer, "unused", principal, grants)
        },
    )
    .await
    .expect("issue a person's tenant key")
}

/// A console admin session of the tenant, `<user>-admin` of the admin
/// entry `user`: the one caller that links and unlinks a key (the
/// operator, 2026-11-17: "Console only"; ADR-0085 d16).
async fn console_admin(app: &TestApp, admin: &str, user: &str) -> oml_app::app::Caller {
    let session = signed_in(app, admin, user, true).await;
    app.web_caller(&session, Capacity::Admin)
        .await
        .expect("the console admin session")
}

fn link_refused(err: &AppError, reason: &str) -> bool {
    matches!(err, AppError::KeyLinkRefused { reason: r } if *r == reason)
}

#[tokio::test]
async fn an_admin_links_a_person_tenant_key_and_the_link_is_journalled() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    signed_in(&app, admin, "alice", false).await;
    let key = person_tenant_key(&app, admin, "alice/dev-review", Vec::new()).await;
    let linked = app
        .link_key_to_person(Bearer::Session(&ops), key.key_id.into(), &name("alice"))
        .await
        .expect("linked");
    let root = app
        .user_root(admin, &name("alice"))
        .await
        .expect("a read")
        .expect("alice's root")
        .workspace_id;
    assert_eq!(linked.linked_user, Some(root));
    let events = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    let event = events
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::KEY_LINKED_PERSON)
        .expect("key.linked_person");
    assert_eq!(event.envelope.payload["key_id"], key.key_id.to_string());
    assert_eq!(event.envelope.payload["user"], "alice");
    // Unlinked, journalled, and the key acts for no one again.
    let unlinked = app
        .unlink_key_from_person(Bearer::Session(&ops), key.key_id.into())
        .await
        .expect("unlinked");
    assert_eq!(unlinked.linked_user, None);
    let events = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    assert_eq!(
        events.last().expect("an event").envelope.event_type,
        event_types::KEY_UNLINKED_PERSON
    );
}

#[tokio::test]
async fn linking_an_agent_held_key_is_refused() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    signed_in(&app, admin, "alice", false).await;
    let agent = tenant_key(&app, admin, Role::Writer, "alice/agent", Vec::new()).await;
    let user_key = user_writer(&app, admin, "alice", "alice/laptop").await;
    let head = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    for (key, reason) in [
        (agent.key_id, "not_used_by_person"),
        (user_key.key_id, "not_a_tenant_key"),
        (boot.admin_key.key_id, "admin_key"),
    ] {
        let err = app
            .link_key_to_person(Bearer::Session(&ops), key.into(), &name("alice"))
            .await
            .unwrap_err();
        assert!(link_refused(&err, reason), "{reason}: {err:?}");
    }
    let person = person_tenant_key(&app, admin, "alice/dev-review", Vec::new()).await;
    let err = app
        .link_key_to_person(Bearer::Session(&ops), person.key_id.into(), &name("nobody"))
        .await
        .unwrap_err();
    assert!(link_refused(&err, "user_unknown"), "{err:?}");
    // Only an admin key links.
    let err = app
        .link_key_to_person(person.value.as_str(), person.key_id.into(), &name("alice"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::VerbCeiling { .. }), "{err:?}");
    let after = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    assert_eq!(
        head + 1,
        after,
        "only the person key's issuance; no refusal journals"
    );
}

#[tokio::test]
async fn a_linked_keys_approval_of_its_persons_agent_write_is_marked_same_user() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    signed_in(&app, admin, "alice", false).await;
    let writer = user_writer(&app, admin, "alice", "alice/laptop").await;
    let reviewer = person_tenant_key(
        &app,
        admin,
        "alice/dev-review",
        vec![
            grant(by_name("alice"), Role::Reviewer),
            grant(by_name("alice"), Role::Reader),
        ],
    )
    .await;
    app.link_key_to_person(
        Bearer::Session(&ops),
        reviewer.key_id.into(),
        &name("alice"),
    )
    .await
    .expect("linked");
    let id = written(&app, &writer.value, &submit("l1", "A linked review.", None)).await;
    let outcome = app
        .post_verdict(&reviewer.value, id, Verdict::Approve, None)
        .await
        .expect("approve");
    assert!(outcome.same_user, "the linked key acts for alice");
}

#[tokio::test]
async fn the_acting_key_resolves_only_for_a_live_key_linked_to_the_sessions_own_user() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    let alice = signed_in(&app, admin, "alice", false).await;
    let bob = signed_in(&app, admin, "bob", false).await;
    let review = app
        .web_caller(&alice, Capacity::Review)
        .await
        .expect("alice-review");
    let alices = person_tenant_key(&app, admin, "alice/dev-review", Vec::new()).await;
    let bobs = person_tenant_key(&app, admin, "bob/dev-review", Vec::new()).await;
    let unlinked = person_tenant_key(&app, admin, "alice/spare", Vec::new()).await;
    app.link_key_to_person(Bearer::Session(&ops), alices.key_id.into(), &name("alice"))
        .await
        .expect("linked");
    app.link_key_to_person(Bearer::Session(&ops), bobs.key_id.into(), &name("bob"))
        .await
        .expect("linked");
    let acting = app
        .acting_caller(&review, &alice, alices.key_id)
        .await
        .expect("alice's linked key");
    assert_eq!(acting.key().key_id, alices.key_id);
    for key in [bobs.key_id, unlinked.key_id, Uuid::from_u128(0xDEAD)] {
        let err = app.acting_caller(&review, &alice, key).await.unwrap_err();
        assert!(matches!(err, AppError::ActingKeyNotLinked), "{err:?}");
    }
    // Unlinked, the key no longer acts, for the person it acted for.
    let bob_review = app
        .web_caller(&bob, Capacity::Review)
        .await
        .expect("bob-review");
    app.acting_caller(&bob_review, &bob, bobs.key_id)
        .await
        .expect("bob's linked key");
    app.unlink_key_from_person(Bearer::Session(&ops), bobs.key_id.into())
        .await
        .expect("unlinked");
    let err = app
        .acting_caller(&bob_review, &bob, bobs.key_id)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ActingKeyNotLinked), "{err:?}");
}

#[tokio::test]
async fn a_linked_key_past_its_expiry_no_longer_acts_for_its_person() {
    let store = MemStore::new();
    let clock = FixedClock::new();
    let app = App::new(
        store,
        MemSigner::new(TEST_SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    let alice = signed_in(&app, admin, "alice", false).await;
    let review = app
        .web_caller(&alice, Capacity::Review)
        .await
        .expect("alice-review");
    let key = person_tenant_key(&app, admin, "alice/dev-review", Vec::new()).await;
    app.link_key_to_person(Bearer::Session(&ops), key.key_id.into(), &name("alice"))
        .await
        .expect("linked");
    app.set_key_limits(
        admin,
        key.key_id.into(),
        KeyLimitsChange {
            expiry: Some(Some(
                EventTime::parse("2026-10-29T12:10:00.000000Z").expect("an instant"),
            )),
            requests_per_minute: None,
        },
    )
    .await
    .expect("an expiry");
    app.acting_caller(&review, &alice, key.key_id)
        .await
        .expect("live until its expiry");
    clock.advance(3_600);
    let err = app
        .acting_caller(&review, &alice, key.key_id)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ActingKeyNotLinked), "{err:?}");
    // An expired key is not linked again.
    let err = app
        .link_key_to_person(Bearer::Session(&ops), key.key_id.into(), &name("alice"))
        .await
        .unwrap_err();
    assert!(link_refused(&err, "revoked"), "{err:?}");
}

/// The review's refusals of a link: a disabled person, a session key, a
/// service key, an already linked key, the admin's own person, and an
/// unlink by a key that is not an admin key (ADR-0085 d16, amended
/// 2026-11-17).
#[tokio::test]
async fn a_link_is_refused_for_a_disabled_person_a_session_key_a_service_key_a_linked_key_and_the_admins_own_person()
 {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    let alice = signed_in(&app, admin, "alice", true).await;
    signed_in(&app, admin, "bob", false).await;
    signed_in(&app, admin, "carol", false).await;
    let alice_admin = app
        .web_caller(&alice, Capacity::Admin)
        .await
        .expect("alice-admin");
    app.disable_web_user(&alice_admin, &name("carol"))
        .await
        .expect("carol is disabled");
    let key = person_tenant_key(&app, admin, "bob/dev-review", Vec::new()).await;
    let head = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    let err = app
        .link_key_to_person(Bearer::Session(&ops), key.key_id.into(), &name("carol"))
        .await
        .unwrap_err();
    assert!(link_refused(&err, "user_disabled"), "{err:?}");
    let review = app
        .web_caller(&alice, Capacity::Review)
        .await
        .expect("alice-review");
    let err = app
        .link_key_to_person(
            Bearer::Session(&ops),
            review.key().key_id.into(),
            &name("alice"),
        )
        .await
        .unwrap_err();
    assert!(link_refused(&err, "not_a_tenant_key"), "{err:?}");
    let service = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                user: None,
                used_by: Some(UsedBy::Service),
                ..user_req(Role::Reader, "unused", "svc/indexer", Vec::new())
            },
        )
        .await
        .expect("a service key");
    let head = head + 1;
    let err = app
        .link_key_to_person(Bearer::Session(&ops), service.key_id.into(), &name("bob"))
        .await
        .unwrap_err();
    assert!(link_refused(&err, "not_used_by_person"), "{err:?}");
    // alice-admin links bob's key to bob, never to alice herself.
    let err = app
        .link_key_to_person(
            Bearer::Session(&alice_admin),
            key.key_id.into(),
            &name("alice"),
        )
        .await
        .unwrap_err();
    assert!(link_refused(&err, "self_link"), "{err:?}");
    app.link_key_to_person(
        Bearer::Session(&alice_admin),
        key.key_id.into(),
        &name("bob"),
    )
    .await
    .expect("another person's key");
    let err = app
        .link_key_to_person(Bearer::Session(&ops), key.key_id.into(), &name("alice"))
        .await
        .unwrap_err();
    assert!(link_refused(&err, "already_linked"), "{err:?}");
    let err = app
        .unlink_key_from_person(key.value.as_str(), key.key_id.into())
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::VerbCeiling { .. }), "{err:?}");
    let after = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    assert_eq!(after, head + 1, "only the link journals");
}

/// A linked key revoked after its link acts for no one, and is unlinked
/// all the same; a key of another tenant is no key of this session's
/// (ADR-0085 d16, amended 2026-11-17).
#[tokio::test]
async fn a_revoked_or_foreign_linked_key_acts_for_no_one_and_a_revoked_one_is_still_unlinked() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    let alice = signed_in(&app, admin, "alice", false).await;
    let review = app
        .web_caller(&alice, Capacity::Review)
        .await
        .expect("alice-review");
    let key = person_tenant_key(&app, admin, "alice/dev-review", Vec::new()).await;
    app.link_key_to_person(Bearer::Session(&ops), key.key_id.into(), &name("alice"))
        .await
        .expect("linked");
    store.set_key_revoked(
        boot.tenant_id,
        key.key_id,
        EventTime::parse("2026-10-29T12:00:00.000000Z").expect("an instant"),
    );
    let err = app
        .acting_caller(&review, &alice, key.key_id)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ActingKeyNotLinked), "{err:?}");
    app.unlink_key_from_person(Bearer::Session(&ops), key.key_id.into())
        .await
        .expect("a revoked key is unlinked");

    // A second tenant on the same store, with a key linked to its person.
    let other = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("globex"),
            admin_key_name: name("globex-admin"),
            admin_principal: "globex-admin".to_owned(),
        })
        .await
        .expect("a second tenant");
    let other_admin = other.admin_key.value.as_str();
    let other_ops = console_admin(&app, other_admin, "globex-ops").await;
    signed_in(&app, other_admin, "dave", false).await;
    let foreign = person_tenant_key(&app, other_admin, "dave/globex", Vec::new()).await;
    app.link_key_to_person(
        Bearer::Session(&other_ops),
        foreign.key_id.into(),
        &name("dave"),
    )
    .await
    .expect("linked in its own tenant");
    let err = app
        .acting_caller(&review, &alice, foreign.key_id)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ActingKeyNotLinked), "{err:?}");
}

/// A linked tenant key writes into its person's root wherever its grants
/// allow, as a user key does; unlinked, the same key is refused there
/// (the operator, 2026-11-17: "Yes, allow it"; ADR-0044 d16).
#[tokio::test]
async fn a_linked_key_writes_into_its_persons_root_and_an_unlinked_one_is_refused() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    signed_in(&app, admin, "alice", false).await;
    let writer = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                user: None,
                used_by: Some(UsedBy::Person),
                ..user_req(
                    Role::Writer,
                    "unused",
                    "alice/desk",
                    vec![grant(by_name("alice"), Role::Writer)],
                )
            },
        )
        .await
        .expect("a tenant writer granted into alice's root");
    let err = app
        .submit_memory(&writer.value, &submit("w0", "Not hers yet.", Some("alice")))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::UserRootRefused), "{err:?}");
    app.link_key_to_person(Bearer::Session(&ops), writer.key_id.into(), &name("alice"))
        .await
        .expect("linked");
    written(
        &app,
        &writer.value,
        &submit("w1", "Hers now.", Some("alice")),
    )
    .await;
    app.unlink_key_from_person(Bearer::Session(&ops), writer.key_id.into())
        .await
        .expect("unlinked");
    let err = app
        .submit_memory(
            &writer.value,
            &submit("w2", "Not hers again.", Some("alice")),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::UserRootRefused), "{err:?}");
}

/// Linking and unlinking take a console admin session, whose person is
/// known: a bearer admin key, which names no person, is refused
/// `session_admin_only` and journals nothing; a session admin linking a
/// key to its own person is refused `self_link` (the operator,
/// 2026-11-17: "Console only"; ADR-0085 d16).
#[tokio::test]
async fn a_bearer_admin_key_links_nothing_and_a_console_admin_never_links_its_own_person() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    signed_in(&app, admin, "alice", false).await;
    let key = person_tenant_key(&app, admin, "alice/dev-review", Vec::new()).await;
    let head = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    let err = app
        .link_key_to_person(admin, key.key_id.into(), &name("alice"))
        .await
        .unwrap_err();
    assert!(link_refused(&err, "session_admin_only"), "{err:?}");
    let err = app
        .link_key_to_person(Bearer::Session(&ops), key.key_id.into(), &name("ops"))
        .await
        .unwrap_err();
    assert!(link_refused(&err, "self_link"), "{err:?}");
    app.link_key_to_person(Bearer::Session(&ops), key.key_id.into(), &name("alice"))
        .await
        .expect("a console admin links another person's key");
    let err = app
        .unlink_key_from_person(admin, key.key_id.into())
        .await
        .unwrap_err();
    assert!(link_refused(&err, "session_admin_only"), "{err:?}");
    let after = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    assert_eq!(after, head + 1, "only the console admin's link journals");
}

// -- Withdraw (ADR-0083 d13; #401) -------------------------------------------

fn withdraw_req(idem: &str, id: Uuid) -> oml_app::types::WithdrawRequest {
    oml_app::types::WithdrawRequest {
        idempotency_key: idem.to_owned(),
        memory_id: id,
        reason: None,
    }
}

/// A person's session web key withdraws a held version their agent's key
/// wrote; another person's session key, which cannot read it, is told
/// nothing.
#[tokio::test]
async fn a_person_web_key_withdraws_a_version_its_agent_wrote() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let alice = signed_in(&app, admin, "alice", false).await;
    let bob = signed_in(&app, admin, "bob", false).await;
    let agent = user_writer(&app, admin, "alice", "alice/laptop").await;
    let id = written(
        &app,
        &agent.value,
        &submit("w1", "A note to retract.", None),
    )
    .await;
    let bobs = app.web_caller(&bob, Capacity::Web).await.expect("bob-web");
    let err = app
        .withdraw(Bearer::Session(&bobs), &withdraw_req("b1", id))
        .await
        .unwrap_err();
    // bob reads nothing in alice's root, so he is told nothing exists.
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let web = app
        .web_caller(&alice, Capacity::Web)
        .await
        .expect("alice-web");
    let (receipt, replayed) = app
        .withdraw(Bearer::Session(&web), &withdraw_req("a1", id))
        .await
        .expect("alice withdraws her agent's write");
    assert!(!replayed);
    assert_eq!(receipt.state, "withdrawn");
    let row = app
        .get_memory(agent.value.as_str(), id)
        .await
        .expect("the author still reads it");
    assert_eq!(row.memory.status, MemoryStatus::Withdrawn);
}

// -- R2 by person (ADR-0001 d5; ADR-0082 d7; #392) ---------------------------

/// A shared `team` workspace, alice signed in as an admin of it, and her
/// tenant writer key `alice-team` linked to her, writer on `team`.
async fn team_with_alices_linked_writer(
    app: &TestApp,
    admin: &str,
    ops: &oml_app::app::Caller,
) -> (WebSessionRecord, IssuedKey) {
    app.create_workspace(admin, &name("team"), Some(by_name("universe")), None)
        .await
        .expect("an admin creates a shared workspace");
    let alice = signed_in(app, admin, "alice", true).await;
    let writer = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                user: None,
                used_by: Some(UsedBy::Person),
                ..user_req(
                    Role::Writer,
                    "unused",
                    "alice/team",
                    vec![
                        grant(by_name("team"), Role::Writer),
                        grant(by_name("team"), Role::Reader),
                    ],
                )
            },
        )
        .await
        .expect("alice's tenant writer key");
    app.link_key_to_person(Bearer::Session(ops), writer.key_id.into(), &name("alice"))
        .await
        .expect("linked");
    (alice, writer)
}

fn same_person(err: &AppError) -> bool {
    matches!(err, AppError::SamePerson)
}

/// Outside alice's root, alice's session reviewer key judges nothing her
/// linked tenant key wrote, approve or reject; the row is neither listed
/// to it nor served; another person's reviewer approves it.
#[tokio::test]
async fn a_persons_session_reviewer_key_is_refused_a_verdict_on_their_linked_keys_write_outside_their_root()
 {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    let (alice, writer) = team_with_alices_linked_writer(&app, admin, &ops).await;
    let id = written(
        &app,
        &writer.value,
        &submit("t1", "The team ships on Fridays.", Some("team")),
    )
    .await;
    let alice_admin = app
        .web_caller(&alice, Capacity::Admin)
        .await
        .expect("alice-admin");
    app.grant_review_capacity(&alice_admin, &name("alice"), by_name("team"))
        .await
        .expect("the review designation");
    let review = app
        .web_caller(&alice, Capacity::Review)
        .await
        .expect("alice-review");
    for verdict in [Verdict::Approve, Verdict::Reject] {
        let err = app
            .post_verdict(Bearer::Session(&review), id, verdict, Some("no"))
            .await
            .unwrap_err();
        assert!(same_person(&err), "{err:?}");
    }
    let listed = app
        .review_held(
            Bearer::Session(&review),
            &oml_app::types::HeldListingRequest {
                workspace: None,
                author_key: None,
                claim_key_prefix: None,
                cursor: None,
                limit: 50,
                needs: None,
            },
        )
        .await
        .expect("the listing");
    assert!(
        listed.items.iter().all(|i| i.served.memory.memory_id != id),
        "not listed to alice's reviewer"
    );
    assert!(
        app.review_next(Bearer::Session(&review))
            .await
            .expect("serve")
            .is_none(),
        "not served to alice's reviewer"
    );
    let bob = person_tenant_key(
        &app,
        admin,
        "bob/team-review",
        vec![
            grant(by_name("team"), Role::Reviewer),
            grant(by_name("team"), Role::Reader),
        ],
    )
    .await;
    let outcome = app
        .post_verdict(&bob.value, id, Verdict::Approve, None)
        .await
        .expect("another person approves");
    assert!(!outcome.same_user);
}

/// A linked tenant reviewer key judges nothing its person's session key
/// wrote outside their root: `<user>-curate`'s write in `universe`.
#[tokio::test]
async fn a_persons_linked_reviewer_key_is_refused_a_verdict_on_their_session_keys_write() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    let alice = signed_in(&app, admin, "alice", true).await;
    let alice_admin = app
        .web_caller(&alice, Capacity::Admin)
        .await
        .expect("alice-admin");
    app.grant_curation_capacity(&alice_admin, &name("alice"), by_name("alice"))
        .await
        .expect("the curation designation");
    let curate = app
        .web_caller(&alice, Capacity::Curate)
        .await
        .expect("alice-curate");
    let id = app
        .submit_memory(
            Bearer::Session(&curate),
            &submit("u1", "Deploys pause on Fridays.", Some("universe")),
        )
        .await
        .expect("curated")
        .receipt
        .memory_id;
    let reviewer = person_tenant_key(
        &app,
        admin,
        "alice/universe-review",
        vec![
            node_grant(by_name("universe"), Role::Reviewer),
            node_grant(by_name("universe"), Role::Reader),
        ],
    )
    .await;
    app.link_key_to_person(
        Bearer::Session(&ops),
        reviewer.key_id.into(),
        &name("alice"),
    )
    .await
    .expect("linked");
    let err = app
        .post_verdict(&reviewer.value, id, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(same_person(&err), "{err:?}");
    let other = universe_reviewer(&app, admin, "universe-review").await;
    app.post_verdict(&other.value, id, Verdict::Approve, None)
        .await
        .expect("another identity approves");
}

/// Inside alice's root the rule does not apply: her linked tenant
/// reviewer key approves her user key's write, marked same user.
#[tokio::test]
async fn inside_the_persons_root_their_linked_key_approves_their_user_keys_write_marked_same_user()
{
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let ops = console_admin(&app, admin, "ops").await;
    signed_in(&app, admin, "alice", false).await;
    let writer = user_writer(&app, admin, "alice", "alice/laptop").await;
    let reviewer = person_tenant_key(
        &app,
        admin,
        "alice/root-review",
        vec![
            grant(by_name("alice"), Role::Reviewer),
            grant(by_name("alice"), Role::Reader),
        ],
    )
    .await;
    app.link_key_to_person(
        Bearer::Session(&ops),
        reviewer.key_id.into(),
        &name("alice"),
    )
    .await
    .expect("linked");
    let id = written(
        &app,
        &writer.value,
        &submit("r1", "A note in the root.", None),
    )
    .await;
    let outcome = app
        .post_verdict(&reviewer.value, id, Verdict::Approve, None)
        .await
        .expect("approved inside the root");
    assert!(outcome.same_user);
}

/// Alice's session reviewer of `team`, her linked tenant writer, and bob's
/// unlinked tenant writer, all on `team` (ADR-0082 d7; #392).
async fn team_with_alices_reviewer_and_bobs_writer(
    app: &TestApp,
    admin: &str,
) -> (oml_app::app::Caller, IssuedKey, IssuedKey) {
    let ops = console_admin(app, admin, "ops").await;
    let (alice, writer) = team_with_alices_linked_writer(app, admin, &ops).await;
    let alice_admin = app
        .web_caller(&alice, Capacity::Admin)
        .await
        .expect("alice-admin");
    app.grant_review_capacity(&alice_admin, &name("alice"), by_name("team"))
        .await
        .expect("the review designation");
    let review = app
        .web_caller(&alice, Capacity::Review)
        .await
        .expect("alice-review");
    let bob = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                user: None,
                used_by: Some(UsedBy::Person),
                ..user_req(
                    Role::Writer,
                    "unused",
                    "bob/team",
                    vec![
                        grant(by_name("team"), Role::Writer),
                        grant(by_name("team"), Role::Reader),
                    ],
                )
            },
        )
        .await
        .expect("bob's tenant writer key");
    (review, writer, bob)
}

fn held_request(
    limit: usize,
    needs: Option<oml_app::types::ReviewLane>,
) -> oml_app::types::HeldListingRequest {
    oml_app::types::HeldListingRequest {
        workspace: None,
        author_key: None,
        claim_key_prefix: None,
        cursor: None,
        limit,
        needs,
    }
}

/// A page whose first rows are all the reviewer's own person's, outside
/// their root, gives way to the next rows the reviewer may judge: no
/// empty page with a cursor, no short page.
#[tokio::test]
async fn a_held_page_skips_the_reviewers_own_persons_rows_and_serves_the_next_eligible_rows() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let (review, alice, bob) = team_with_alices_reviewer_and_bobs_writer(&app, admin).await;
    for n in 0..3 {
        written(
            &app,
            &alice.value,
            &submit(
                &format!("a{n}"),
                &format!("Alice's note {n}."),
                Some("team"),
            ),
        )
        .await;
    }
    let mut bobs = Vec::new();
    for n in 0..2 {
        bobs.push(
            written(
                &app,
                &bob.value,
                &submit(&format!("b{n}"), &format!("Bob's note {n}."), Some("team")),
            )
            .await,
        );
    }
    let page = app
        .review_held(Bearer::Session(&review), &held_request(2, None))
        .await
        .expect("the listing");
    let listed: Vec<Uuid> = page
        .items
        .iter()
        .map(|i| i.served.memory.memory_id)
        .collect();
    assert_eq!(listed, bobs, "bob's rows fill the page");
    assert!(page.next_cursor.is_none(), "nothing past bob's rows");
}

/// The person lane's per-author counts hold no author key of the
/// reviewer's own person outside their root.
#[tokio::test]
async fn the_person_lanes_counts_omit_the_reviewers_own_persons_author_keys() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let (review, alice, bob) = team_with_alices_reviewer_and_bobs_writer(&app, admin).await;
    for (key, idem) in [(&alice, "sa"), (&bob, "sb")] {
        written(
            &app,
            &key.value,
            &SubmitRequest {
                kind: MemoryKind::Steering,
                ..submit(idem, "Cite every source.", Some("team"))
            },
        )
        .await;
    }
    let page = app
        .review_held(
            Bearer::Session(&review),
            &held_request(10, Some(oml_app::types::ReviewLane::Person)),
        )
        .await
        .expect("the person lane's listing");
    assert_eq!(page.items.len(), 1, "bob's steering alone");
    assert_eq!(page.lane_authors, Some(vec![(bob.key_id, 1)]));
}

/// The bulk confirmation shows none of the reviewer's own person's rows
/// outside their root, since the verdict would refuse them.
#[tokio::test]
async fn the_bulk_confirmation_names_no_row_the_reviewers_own_person_wrote_outside_their_root() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let (review, alice, bob) = team_with_alices_reviewer_and_bobs_writer(&app, admin).await;
    let mine = written(
        &app,
        &alice.value,
        &submit("n1", "Alice's note.", Some("team")),
    )
    .await;
    let theirs = written(&app, &bob.value, &submit("n2", "Bob's note.", Some("team"))).await;
    let named = app
        .review_named(Bearer::Session(&review), &[mine, theirs])
        .await
        .expect("the confirmation");
    assert_eq!(named.len(), 2);
    assert_eq!(named[0].0, mine);
    assert!(named[0].1.is_none(), "alice's own row is not confirmable");
    assert_eq!(named[1].0, theirs);
    assert!(named[1].1.is_some(), "bob's row is");
}

/// Held steering reaches the key that wrote it and no other key of its
/// person: the web key of a person whose curate key wrote the row is not
/// served it, `include_unreviewed: true` or not (ADR-0080 d14).
#[tokio::test]
async fn held_steering_reaches_the_authoring_key_and_no_other_key_of_its_person() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let alice = signed_in(&app, admin, "alice", true).await;
    let alice_admin = app
        .web_caller(&alice, Capacity::Admin)
        .await
        .expect("alice-admin");
    app.grant_curation_capacity(&alice_admin, &name("alice"), by_name("alice"))
        .await
        .expect("the curation designation");
    let curate = app
        .web_caller(&alice, Capacity::Curate)
        .await
        .expect("alice-curate");
    let web = app
        .web_caller(&alice, Capacity::Web)
        .await
        .expect("alice-web");
    let held = app
        .submit_memory(
            Bearer::Session(&curate),
            &SubmitRequest {
                on_similar: Default::default(),
                kind: MemoryKind::Steering,
                ..submit("steer", "cite every source", Some("universe"))
            },
        )
        .await
        .expect("a held steering row")
        .receipt
        .memory_id;
    let own_steering = app
        .submit_memory(
            Bearer::Session(&web),
            &SubmitRequest {
                on_similar: Default::default(),
                kind: MemoryKind::Steering,
                ..submit("own", "cite the source twice", Some("alice"))
            },
        )
        .await
        .expect("the web key's own held steering row")
        .receipt
        .memory_id;
    let asked = RecallRequest {
        include_unreviewed: Some(true),
        ..recall("source", Some("alice"))
    };
    let served = app
        .recall(Bearer::Session(&web), &asked)
        .await
        .expect("the web key's recall");
    let ids: Vec<Uuid> = served.steering.iter().map(|m| m.memory.memory_id).collect();
    assert_eq!(
        ids,
        vec![own_steering],
        "the calling key's own row, not alice-curate's {held}"
    );
}

/// A held skill is offered to the key that wrote it and to no other key of
/// its person, the person's admin capacity included (ADR-0080 d14, d16).
#[tokio::test]
async fn a_held_skill_is_offered_to_its_authoring_key_and_no_other_key_of_its_person() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let alice = signed_in(&app, admin, "alice", true).await;
    let alice_admin = app
        .web_caller(&alice, Capacity::Admin)
        .await
        .expect("alice-admin");
    app.grant_curation_capacity(&alice_admin, &name("alice"), by_name("alice"))
        .await
        .expect("the curation designation");
    let curate = app
        .web_caller(&alice, Capacity::Curate)
        .await
        .expect("alice-curate");
    let web = app
        .web_caller(&alice, Capacity::Web)
        .await
        .expect("alice-web");
    let skill = |idem: &str, name: &str, workspace: &str| SubmitRequest {
        on_similar: Default::default(),
        kind: MemoryKind::Skill,
        claim_key: None,
        format: Some("text/markdown".to_owned()),
        kind_fields: Some(oml_app::types::KindFields::Skill {
            name: name.to_owned(),
            description: "Cite every source.".to_owned(),
        }),
        ..submit(idem, "Cite every source, then check each.", Some(workspace))
    };
    app.submit_memory(Bearer::Session(&curate), &skill("s1", "cite", "universe"))
        .await
        .expect("alice-curate's held skill");
    app.submit_memory(Bearer::Session(&web), &skill("s2", "own", "alice"))
        .await
        .expect("alice-web's own held skill");
    let asked = oml_app::types::SkillRequest {
        workspace: Some(by_name("alice")),
        include_own_held: true,
        cursor: None,
    };
    let offered = app
        .skills(Bearer::Session(&web), &asked)
        .await
        .expect("the web key's skills");
    let names: Vec<&str> = offered.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["own"], "the calling key's own, not alice-curate's");
    match app.skills(Bearer::Session(&alice_admin), &asked).await {
        Ok(set) => assert!(
            set.skills.iter().all(|s| s.skill_name != "cite"),
            "{:?}",
            set.skills
        ),
        Err(e) => assert!(
            matches!(
                e,
                AppError::VerbCeiling { .. } | AppError::SessionKeyDenied { .. }
            ),
            "{e:?}"
        ),
    }
}

// -- mine, state lists and promoted_since (ADR-0083 d3, d14; #409) -----------

fn mine_listing(
    states: Vec<oml_app::types::AdminState>,
    kinds: Vec<MemoryKind>,
    promoted_since: Option<EventTime>,
) -> oml_app::app::WebListRequest {
    oml_app::app::WebListRequest {
        mine: true,
        states,
        kinds,
        promoted_since,
        limit: 50,
        ..oml_app::app::WebListRequest::default()
    }
}

async fn listed_ids(
    app: &TestApp,
    caller: &oml_app::app::Caller,
    req: &oml_app::app::WebListRequest,
) -> std::collections::BTreeSet<Uuid> {
    app.list_memories_with_own(caller, req)
        .await
        .expect("the listing")
        .items
        .iter()
        .map(|m| m.memory.memory_id)
        .collect()
}

/// `mine=true` from a person's session web key lists the rows of every key
/// of the person, in every state, and no scoped part: a `universe` tip the
/// merged listing serves is absent. `state` takes a comma list, `kind` a
/// comma list, `author` narrows to one own key, and `promoted_since`
/// narrows the promoted rows alone, the held ones staying whole (ADR-0083
/// d3, d14; ADR-0082 d8 item 3).
#[tokio::test]
async fn mine_lists_every_key_of_the_person_and_promoted_since_limits_only_promoted_rows() {
    use oml_app::types::AdminState;
    let store = MemStore::new();
    let clock = FixedClock::new();
    let app = App::new(
        store,
        MemSigner::new(TEST_SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let c = curated(&app, &boot).await;
    let alice = signed_in(&app, admin, "alice", false).await;
    let web = app
        .web_caller(&alice, Capacity::Web)
        .await
        .expect("alice-web");
    let review = app
        .web_caller(&alice, Capacity::Review)
        .await
        .expect("alice-review");
    clock.advance(60);
    let cut = oml_app::ports::Clock::now(&clock);
    let late = written(
        &app,
        &c.alice.value,
        &submit("late", "Releases tag from main.", None),
    )
    .await;
    app.post_verdict(Bearer::Session(&review), late, Verdict::Approve, None)
        .await
        .expect("alice's review key approves in her root");
    let held = app
        .submit_memory(
            Bearer::Session(&web),
            &submit("held", "Typed in the console.", None),
        )
        .await
        .expect("the web key writes in its root")
        .receipt
        .memory_id;
    let steer = written(
        &app,
        &c.alice.value,
        &SubmitRequest {
            kind: MemoryKind::Steering,
            ..submit("steer", "Run the tests before a commit.", None)
        },
    )
    .await;

    let all = listed_ids(&app, &web, &mine_listing(Vec::new(), Vec::new(), None)).await;
    let expected: std::collections::BTreeSet<Uuid> =
        [c.source, late, held, steer].into_iter().collect();
    assert_eq!(all, expected, "every key of the person, no scoped part");
    let merged = listed_ids(
        &app,
        &web,
        &oml_app::app::WebListRequest {
            limit: 50,
            ..oml_app::app::WebListRequest::default()
        },
    )
    .await;
    assert!(
        merged.contains(&c.placed),
        "the merged listing has the scoped part"
    );

    let since = listed_ids(
        &app,
        &web,
        &mine_listing(
            vec![AdminState::Held, AdminState::Promoted],
            Vec::new(),
            Some(cut),
        ),
    )
    .await;
    let expected: std::collections::BTreeSet<Uuid> = [late, held, steer].into_iter().collect();
    assert_eq!(
        since, expected,
        "promoted_since drops the promoted row approved before it and leaves the held rows whole"
    );

    let steering = listed_ids(
        &app,
        &web,
        &mine_listing(
            Vec::new(),
            vec![MemoryKind::Steering, MemoryKind::Skill],
            None,
        ),
    )
    .await;
    assert_eq!(steering, [steer].into_iter().collect());

    let by_web = listed_ids(
        &app,
        &web,
        &oml_app::app::WebListRequest {
            author: web.key().name.clone(),
            ..mine_listing(Vec::new(), Vec::new(), None)
        },
    )
    .await;
    assert_eq!(by_web, [held].into_iter().collect(), "author narrows mine");
}

/// `mine=true` from a key no person's session serves lists that key's own
/// rows alone, as the own part of `with_own` does (ADR-0083 d14).
#[tokio::test]
async fn mine_from_a_key_no_session_serves_lists_that_key_alone() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = boot.admin_key.value.as_str();
    let alice = signed_in(&app, admin, "alice", false).await;
    let agent = user_writer(&app, admin, "alice", "alice/laptop").await;
    let other = user_writer(&app, admin, "alice", "alice/desktop").await;
    let mine = written(&app, &agent.value, &submit("a1", "The agent's note.", None)).await;
    written(
        &app,
        &other.value,
        &submit("o1", "Another key's note.", None),
    )
    .await;
    let web = app
        .web_caller(&alice, Capacity::Web)
        .await
        .expect("alice-web");
    app.submit_memory(
        Bearer::Session(&web),
        &submit("w1", "The person's own note.", None),
    )
    .await
    .expect("the web key writes");
    let caller = app
        .caller(Bearer::from(agent.value.as_str()))
        .await
        .expect("the agent's caller");
    let listed = listed_ids(&app, &caller, &mine_listing(Vec::new(), Vec::new(), None)).await;
    assert_eq!(listed, [mine].into_iter().collect());
}
