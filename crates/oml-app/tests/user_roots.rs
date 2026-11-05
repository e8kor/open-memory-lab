//! User roots, user keys, node grants, the two kinds of workarea namespace,
//! curation into `universe` and its served view (ADR-0044), over the
//! in-memory doubles. Each test is named after the sentence of ADR-0044 d15
//! it proves.

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::name::{Name, NamedKind, Reference};
use oml_app::domain::{HeldBy, MemoryKind, MemoryStatus, Role, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, GrantRequest, GuardrailRequest, GuardrailSet,
    IssueKeyRequest, IssuedKey, RecallRequest, SubmitRequest, Verdict,
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

fn user_req(role: Role, user: &str, principal: &str, grants: Vec<GrantRequest>) -> IssueKeyRequest {
    IssueKeyRequest {
        role,
        name: fresh_key_name(),
        principal: principal.to_owned(),
        held_by: Some(HeldBy::Agent),
        default_workarea: None,
        crew: None,
        grants,
        user: Some(name(user)),
        connect_code: false,
        guardrail_scope: None,
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

async fn user_dreamer(app: &TestApp, admin: &str, user: &str, principal: &str) -> IssuedKey {
    app.issue_key(
        admin,
        &user_req(
            Role::Dreamer,
            user,
            principal,
            vec![grant(by_name(user), Role::Dreamer)],
        ),
    )
    .await
    .expect("issue a user dreamer key")
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

/// A tenant dreamer on the `universe` node: the second identity curation
/// needs (ADR-0044 d7).
async fn universe_reviewer(app: &TestApp, admin: &str, principal: &str) -> IssuedKey {
    tenant_key(
        app,
        admin,
        Role::Dreamer,
        principal,
        vec![
            node_grant(by_name("universe"), Role::Dreamer),
            node_grant(by_name("universe"), Role::Reader),
        ],
    )
    .await
}

fn submit(idem: &str, body: &str, workarea: Option<&str>) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workarea: workarea.map(by_name),
        derives_from: Vec::new(),
        severity: None,
        rule: None,
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

fn recall(query: &str, workarea: Option<&str>) -> RecallRequest {
    RecallRequest {
        timings: false,
        query: query.to_owned(),
        k: 10,
        workarea: workarea.map(by_name),
        kinds: Vec::new(),
        include_text: true,
        include_unreviewed: false,
        max_tokens: None,
        search: None,
        match_mode: oml_app::search::MatchMode::Any,
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
    let alice_review = user_dreamer(app, admin, "alice", "alice/review").await;
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
        .create_workarea(admin, &name("crew"), Some(by_name("universe")), None)
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
                        deny: vec![crew.workarea.workarea_id.into()],
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
        .workarea(boot.tenant_id, user.id)
        .await
        .expect("read")
        .expect("the root exists");
    assert!(root.user_root);
    assert_eq!(root.parent_id, Some(boot.root_workarea_id));
    assert_eq!(root.namespace, None, "its name is in the shared namespace");
    assert_eq!(alice.default_workarea, Some(user.id));
    // The root's workarea.create precedes key.issued, both by the issuing
    // admin, and carries user_root: true; key.issued names the root.
    let events = store
        .events_range(boot.tenant_id, 1, 1000)
        .await
        .expect("journal");
    let create = events
        .iter()
        .position(|e| {
            e.envelope.event_type == event_types::WORKAREA_CREATE
                && e.envelope.payload["workarea_id"] == user.id.to_string()
        })
        .expect("workarea.create of the root");
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
    let second = user_dreamer(&app, admin, "alice", "alice/review").await;
    assert_eq!(first.user, second.user);
    let creates = store
        .events_range(boot.tenant_id, 1, 1000)
        .await
        .expect("journal")
        .into_iter()
        .filter(|e| {
            e.envelope.event_type == event_types::WORKAREA_CREATE
                && e.envelope.payload["user_root"] == true
        })
        .count();
    assert_eq!(creates, 1, "one root for one user");
}

#[tokio::test]
async fn a_user_named_like_a_shared_workarea_is_name_taken() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    app.create_workarea(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workarea");
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
        matches!(err, AppError::NameTaken { kind: NamedKind::Workarea, ref name } if name.as_str() == "crew"),
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
                held_by: Some(HeldBy::Service),
                ..user_req(Role::Reader, "alice", "svc", Vec::new())
            },
        )
        .await
        .expect_err("a service key acts for a process");
    assert!(matches!(err, AppError::UserRefused(_)), "{err:?}");
    assert_eq!(
        store
            .resolve_name(boot.tenant_id, NamedKind::Workarea, &name("alice"))
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
    app.create_workarea(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workarea");
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
                                            NamedKind::Workarea,
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
        user_dreamer(&app, admin, "alice", "alice/review").await,
    ] {
        let added: Vec<_> = key
            .access
            .grants
            .iter()
            .filter(|g| g.grant.node == boot.root_workarea_id)
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
async fn two_users_each_create_a_workarea_named_project() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    let a = app
        .create_workarea(&alice.value, &name("project"), None, None)
        .await
        .expect("alice's project");
    let b = app
        .create_workarea(&bob.value, &name("project"), None, None)
        .await
        .expect("bob's project");
    assert_eq!(a.path, "universe.alice.project");
    assert_eq!(b.path, "universe.bob.project");
    assert_ne!(a.workarea.workarea_id, b.workarea.workarea_id);
    // A second one in one user's namespace is taken, and so is the user's
    // own name and universe there.
    for taken in ["project", "alice", "universe"] {
        let err = app
            .create_workarea(&alice.value, &name(taken), Some(by_name("project")), None)
            .await
            .expect_err("taken in alice's namespace");
        assert!(
            matches!(err, AppError::NameTaken { .. }),
            "{taken}: {err:?}"
        );
    }
    // The shared namespace still holds a shared `project` apart.
    app.create_workarea(admin, &name("project"), Some(by_name("universe")), None)
        .await
        .expect("a shared project");
}

#[tokio::test]
async fn a_user_keys_bare_name_resolves_in_its_own_root() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    app.create_workarea(admin, &name("project"), Some(by_name("universe")), None)
        .await
        .expect("a shared project");
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let mine = app
        .create_workarea(&alice.value, &name("project"), None, None)
        .await
        .expect("alice's project");
    let info = app
        .workarea_info(alice.value.as_str(), Some(by_name("project")))
        .await
        .expect("read by bare name");
    assert_eq!(info.workarea.workarea_id, mine.workarea.workarea_id);
    assert_eq!(info.path, "universe.alice.project");
    // Its own root's name and universe resolve too.
    let root = app
        .workarea_info(alice.value.as_str(), Some(by_name("alice")))
        .await
        .expect("its root");
    assert!(root.workarea.user_root);
    app.workarea_info(alice.value.as_str(), Some(by_name("universe")))
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
        .workarea_info(tenant.value.as_str(), Some(by_name("project")))
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
        .create_workarea(&alice.value, &name("project"), None, None)
        .await
        .expect("alice's project");
    let info = app
        .workarea_info(admin.as_str(), Some(by_name("alice.project")))
        .await
        .expect("an admin reads it by its qualified name");
    assert_eq!(info.workarea.workarea_id, project.workarea.workarea_id);
    // Its bare name is not in the shared namespace.
    assert!(matches!(
        app.workarea_info(admin.as_str(), Some(by_name("project")))
            .await,
        Err(AppError::NotFound)
    ));
    // Bob's key resolves alice's qualified name and is told nothing more
    // than for an id it cannot see.
    assert!(matches!(
        app.workarea_info(bob.value.as_str(), Some(by_name("alice.project")))
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
async fn a_user_key_creates_a_workarea_under_its_root_by_default() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let alice = user_writer(&app, &boot.admin_key.value, "alice", "alice/laptop").await;
    let root = alice.user.clone().expect("root").id;
    let created = app
        .create_workarea(&alice.value, &name("project"), None, None)
        .await
        .expect("created under the root");
    assert_eq!(created.workarea.parent_id, Some(root));
    assert_eq!(created.workarea.namespace, Some(root));
    assert!(!created.workarea.user_root);
    let nested = app
        .create_workarea(
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
            e.envelope.event_type == event_types::WORKAREA_CREATE
                && e.envelope.payload["workarea_id"] == created.workarea.workarea_id.to_string()
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
    app.create_workarea(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workarea");
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    // universe: read by the node grant, so grant_refused.
    let err = app
        .create_workarea(&alice.value, &name("x"), Some(by_name("universe")), None)
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
    // A shared workarea, or another user's root: absent.
    let bob_root = bob.user.expect("root").id;
    for parent in [Reference::from(bob_root), by_name("bob")] {
        let err = app
            .create_workarea(&alice.value, &name("x"), Some(parent), None)
            .await
            .expect_err("not under bob");
        assert!(matches!(err, AppError::NotFound), "{err:?}");
    }
    let crew = store
        .resolve_name(boot.tenant_id, NamedKind::Workarea, &name("crew"))
        .await
        .expect("resolve")
        .expect("crew");
    let err = app
        .create_workarea(&alice.value, &name("x"), Some(crew.into()), None)
        .await
        .expect_err("not under a shared workarea");
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
        app.create_workarea(&reader.value, &name("x"), None, None)
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
        app.create_workarea(
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
    assert_eq!(receipt.workarea_id, boot.root_workarea_id);
    assert_eq!(receipt.state, "held");
}

#[tokio::test]
async fn a_curated_claim_is_never_promoted_by_the_sources_author() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let alice_review = user_dreamer(&app, admin, "alice", "alice/review").await;
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
    // A tenant dreamer on universe that shares the source author's
    // principal is in the lineage closure; so is the curator's principal.
    for principal in ["alice/laptop", "curator"] {
        let excluded = universe_reviewer(&app, admin, principal).await;
        let err = app
            .post_verdict(&excluded.value, placed, Verdict::Approve, None)
            .await
            .expect_err("in the lineage closure");
        assert!(matches!(err, AppError::Verdict(_)), "{principal}: {err:?}");
    }
    // A user dreamer key never reaches universe at all.
    let err = app
        .post_verdict(&alice_review.value, placed, Verdict::Approve, None)
        .await
        .expect_err("alice's dreamer has no dreamer on universe");
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
async fn a_users_own_dreamer_key_may_review_its_agents_writes() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let alice_review = user_dreamer(&app, admin, "alice", "alice/review").await;
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
    assert_eq!(got.workarea_path.as_deref(), Some("universe"));
    assert_eq!(got.memory.workarea_id, boot.root_workarea_id);
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
    let alice_review = user_dreamer(&app, admin, "alice", "alice/review2").await;
    let project = app
        .create_workarea(&c.alice.value, &name("project"), None, None)
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
    // A tenant key scoped to a shared workarea gets no universe join.
    let crew = app
        .create_workarea(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("crew");
    let tenant = tenant_key(
        &app,
        admin,
        Role::Reader,
        "tr",
        vec![
            grant(crew.workarea.workarea_id.into(), Role::Reader),
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
    app.create_workarea(&bob.value, &name("secret"), None, None)
        .await
        .expect("bob's workarea");
    app.create_workarea(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workarea");
    let tree = app
        .workarea_tree(&alice.value, by_name("universe"), None, 100)
        .await
        .expect("alice reads the tree from universe");
    let paths: Vec<&str> = tree.items.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(paths, vec!["universe", "universe.alice"]);
    let children = app
        .workarea_info(alice.value.as_str(), Some(by_name("universe")))
        .await
        .expect("universe")
        .children;
    assert_eq!(
        children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["alice"]
    );
    // The admin sees every user root by name.
    let all = app
        .workarea_tree(admin, by_name("universe"), None, 100)
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
    store.set_key_user(boot.tenant_id, alice.key_id, Some(boot.root_workarea_id));
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
    app.create_workarea(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("a shared workarea");
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
    assert_eq!(Some(root.workarea_id), alice.user.map(|u| u.id));
    assert!(matches!(
        app.user_root(&alice.value, &name("alice")).await,
        Err(AppError::VerbCeiling { .. })
    ));
}

// ---------------------------------------------------------------------------
// Guardrails under user roots (proposed ADR-0037 d16, build step S6)
// ---------------------------------------------------------------------------

use oml_app::domain::guardrail::{GuardrailScope, Severity};

fn guardrail(idem: &str, body: &str, workarea: Option<&str>) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        kind: MemoryKind::Guardrail,
        severity: Some(Severity::Must),
        ..submit(idem, body, workarea)
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
    let alice_review = user_dreamer(&app, admin, "alice", "alice/review").await;
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
        .map(|g| g.workarea_path.as_deref())
        .collect();
    assert_eq!(paths, [Some("universe"), Some("universe.alice")]);
}

#[tokio::test]
async fn a_user_key_with_scope_workarea_loads_no_universe_guardrail() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    universe_guardrail(&app, admin, "Never push to main.").await;
    let narrow = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                guardrail_scope: Some(GuardrailScope::Workarea),
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
    assert_eq!(set.guardrail_scope, GuardrailScope::Workarea);
    assert!(set.guardrails.is_empty(), "{:?}", texts(&set));
}

#[tokio::test]
async fn no_guardrail_of_another_users_root_is_ever_served() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    let bob_review = user_dreamer(&app, admin, "bob", "bob/review").await;
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
                workarea: Some(Reference::Name(name("bob"))),
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
    assert_eq!(g.workarea_path.as_deref(), Some("universe"));
    assert_eq!(g.depth, 0);
    // Its verdict's attestation and time, never a key (d18 Q6).
    assert_eq!(g.verdicts.len(), 1);
    assert_eq!(g.verdicts[0].held_by, HeldBy::Agent);
}

#[tokio::test]
async fn the_users_reviewer_key_ratifies_a_guardrail_its_agents_propose_in_its_root() {
    let (app, _) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let alice = user_writer(&app, admin, "alice", "alice/laptop").await;
    let alice_review = user_dreamer(&app, admin, "alice", "alice/review").await;
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

/// A dreamer key of `user` that also reads its root: it proposes, ratifies
/// and reads the claims it links.
async fn user_curator(app: &TestApp, admin: &str, user: &str, principal: &str) -> IssuedKey {
    app.issue_key(
        admin,
        &user_req(
            Role::Dreamer,
            user,
            principal,
            vec![
                grant(by_name(user), Role::Dreamer),
                grant(by_name(user), Role::Reader),
            ],
        ),
    )
    .await
    .expect("issue a user dreamer key that reads its root")
}

#[tokio::test]
async fn a_user_root_climbs_to_committed_and_validated_is_refused_on_it_as_user_root() {
    let (app, _store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let first = user_curator(&app, admin, "alice", "alice/first").await;
    let second = user_curator(&app, admin, "alice", "alice/second").await;
    let committed = app
        .propose_transition(
            &first.value,
            by_name("alice"),
            oml_app::domain::WorkareaStatus::Committed,
        )
        .await
        .expect("a user root climbs to committed");
    app.ratify_transition(&second.value, by_name("alice"), committed.transition_id)
        .await
        .expect("another key of alice ratifies");
    let err = app
        .propose_transition(
            &first.value,
            by_name("alice"),
            oml_app::domain::WorkareaStatus::Validated,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::TransitionRefused {
                reason: oml_app::domain::LadderRefusal::UserRoot,
                ..
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_ratification_by_another_key_of_the_same_user_is_journalled_same_user() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let admin = &boot.admin_key.value;
    let first = user_curator(&app, admin, "alice", "alice/first").await;
    let second = user_curator(&app, admin, "alice", "alice/second").await;
    let project = app
        .create_workarea(
            user_writer(&app, admin, "alice", "alice/laptop")
                .await
                .value
                .as_str(),
            &name("project"),
            None,
            None,
        )
        .await
        .expect("alice creates a workarea")
        .workarea
        .workarea_id;
    let proposal = app
        .propose_transition(
            &first.value,
            project.into(),
            oml_app::domain::WorkareaStatus::Committed,
        )
        .await
        .expect("a proposal");
    app.ratify_transition(&second.value, project.into(), proposal.transition_id)
        .await
        .expect("a ratification");
    let events = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    let ratified = events
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::WORKAREA_STATUS_TRANSITION)
        .expect("the ratification");
    assert_eq!(ratified.envelope.payload["same_user"], true);

    // A tenant key's ratification of another proposal is not same_user.
    let bob = user_curator(&app, admin, "bob", "bob/first").await;
    let tenant_reviewer = tenant_key(
        &app,
        admin,
        Role::Dreamer,
        "tenant-reviewer",
        vec![
            grant(by_name("bob"), Role::Dreamer),
            grant(by_name("bob"), Role::Reader),
        ],
    )
    .await;
    let proposal = app
        .propose_transition(
            &bob.value,
            by_name("bob"),
            oml_app::domain::WorkareaStatus::Committed,
        )
        .await
        .expect("bob proposes");
    app.ratify_transition(
        &tenant_reviewer.value,
        by_name("bob"),
        proposal.transition_id,
    )
    .await
    .expect("a tenant key ratifies");
    let events = store
        .events_range(boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    let ratified = events
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::WORKAREA_STATUS_TRANSITION)
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
                    needed_role: Role::Dreamer,
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
                workarea: by_name("alice"),
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
            workarea: by_name("alice"),
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
/// names no person (ADR-0046 d2, ADR-0055 d7).
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
        Role::Dreamer,
        "crew/reviewer",
        vec![
            grant(by_name("alice"), Role::Dreamer),
            grant(by_name("alice"), Role::Reader),
        ],
    )
    .await;
    let _bob = user_writer(&app, admin, "bob", "bob/laptop").await;
    let bob_review = user_curator(&app, admin, "bob", "bob/review").await;

    // Claims: the person's own agent and reviewer; a crew author; a crew
    // reviewer.
    let cases = [
        (&alice_writer, &alice_review, true, "own"),
        (&crew_writer, &alice_review, false, "crew-author"),
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
}
