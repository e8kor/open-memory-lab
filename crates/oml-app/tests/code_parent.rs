//! The code parent (ADR-0064 d3), over the in-memory doubles: a person's
//! reviewer key marks a parent so that agent writer keys create in-progress
//! children under it, and nowhere else.

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::in_progress::WorkareaMode;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{HeldBy, Role, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, IssuedKey,
};
use std::sync::atomic::{AtomicU64, Ordering};

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

fn by_name(value: &str) -> Reference {
    Reference::parse(value).expect("a reference")
}

fn fresh_key_name() -> Name {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    name(&format!("cp-key-{}", ISSUED.fetch_add(1, Ordering::Relaxed)))
}

struct World {
    app: TestApp,
    store: MemStore,
    boot: BootstrapResult,
    /// The person's reviewer key, a dreamer held by a human, on `pat`.
    person: IssuedKey,
    /// The person's agent writer key on `pat`.
    agent: IssuedKey,
}

async fn issue(app: &TestApp, admin: &str, role: Role, held_by: HeldBy, principal: &str) -> IssuedKey {
    let node = by_name("pat");
    let grants = if role == Role::Dreamer {
        vec![GrantRequest {
            node: node.clone(),
            role: Role::Dreamer,
            deny: Vec::new(),
            covers: Covers::Subtree,
        }]
    } else {
        vec![
            GrantRequest {
                node: node.clone(),
                role: Role::Writer,
                deny: Vec::new(),
                covers: Covers::Subtree,
            },
            GrantRequest {
                node,
                role: Role::Reader,
                deny: Vec::new(),
                covers: Covers::Subtree,
            },
        ]
    };
    app.issue_key(
        admin,
        &IssueKeyRequest {
            profile: None,
            role,
            name: fresh_key_name(),
            principal: principal.to_owned(),
            held_by: Some(held_by),
            default_workarea: None,
            crew: None,
            grants,
            user: Some(name("pat")),
            connect_code: false,
            guardrail_scope: None,
        },
    )
    .await
    .expect("issue a user key")
}

async fn world() -> World {
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new([9; 32]),
        FixedClock::new(),
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
    let person = issue(&app, &admin, Role::Dreamer, HeldBy::Human, "pat/reviewer").await;
    let agent = issue(&app, &admin, Role::Writer, HeldBy::Agent, "pat/agent").await;
    World {
        app,
        store,
        boot,
        person,
        agent,
    }
}

impl World {
    /// `pat.code`, created by the agent key as ADR-0044 d6 has it.
    async fn code(&self) -> Reference {
        self.app
            .create_workarea(&self.agent.value, &name("code"), None, None)
            .await
            .expect("pat.code");
        by_name("pat.code")
    }
}

#[tokio::test]
async fn only_a_human_reviewer_key_marks_a_code_parent() {
    let w = world().await;
    let code = w.code().await;
    for refused in [w.agent.value.as_str(), w.boot.admin_key.value.as_str()] {
        let err = w
            .app
            .set_code_parent(refused, code.clone(), true)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::ModeHumanOnly { .. }), "{err:?}");
    }
    let marked = w
        .app
        .set_code_parent(&w.person.value, code.clone(), true)
        .await
        .expect("the person marks it");
    assert!(marked.children_agent);
    assert!(!marked.in_progress, "a code parent is itself standard");
    let events = w.store.events_range(w.boot.tenant_id, 1, 1000).await.expect("journal");
    let marks: Vec<_> = events
        .iter()
        .filter(|e| e.envelope.event_type == event_types::WORKAREA_MODE_SET)
        .collect();
    assert_eq!(marks.len(), 1, "the mark is one journalled act");
    assert_eq!(marks[0].envelope.payload["children"], "agent");
}

#[tokio::test]
async fn a_writer_creates_an_in_progress_child_only_under_a_code_parent() {
    let w = world().await;
    let code = w.code().await;
    // Before the mark, the writer's create body may not name a mode.
    let err = w
        .app
        .create_workarea_in(
            &w.agent.value,
            &name("repo-a"),
            Some(code.clone()),
            None,
            Some(WorkareaMode::InProgress),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ModeNotAllowedHere), "{err:?}");
    w.app
        .set_code_parent(&w.person.value, code.clone(), true)
        .await
        .expect("mark");
    let child = w
        .app
        .create_workarea_in(
            &w.agent.value,
            &name("repo-a"),
            Some(code.clone()),
            None,
            Some(WorkareaMode::InProgress),
        )
        .await
        .expect("an in-progress child under the code parent");
    assert!(child.workarea.in_progress);
    let events = w.store.events_range(w.boot.tenant_id, 1, 1000).await.expect("journal");
    let create = events
        .iter()
        .find(|e| {
            e.envelope.event_type == event_types::WORKAREA_CREATE
                && e.envelope.payload["workarea_id"] == child.workarea.workarea_id.to_string()
        })
        .expect("journalled as one workarea.create");
    assert_eq!(create.envelope.payload["mode"], "in_progress");
}

#[tokio::test]
async fn mode_in_a_create_body_elsewhere_is_request_validation() {
    let w = world().await;
    let code = w.code().await;
    w.app
        .set_code_parent(&w.person.value, code.clone(), true)
        .await
        .expect("mark");
    // A sibling of the code parent, under the root: not under the mark.
    let err = w
        .app
        .create_workarea_in(
            &w.agent.value,
            &name("elsewhere"),
            None,
            None,
            Some(WorkareaMode::InProgress),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ModeNotAllowedHere), "{err:?}");
    // A grandchild is not a child of the mark either.
    let child = w
        .app
        .create_workarea_in(&w.agent.value, &name("plain"), Some(code.clone()), None, None)
        .await
        .expect("a plain child");
    let err = w
        .app
        .create_workarea_in(
            &w.agent.value,
            &name("deeper"),
            Some(Reference::Id(child.workarea.workarea_id)),
            None,
            Some(WorkareaMode::InProgress),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ModeNotAllowedHere), "{err:?}");
    // The admin key's creation names no mode, whatever the parent.
    let err = w
        .app
        .create_workarea_in(
            &w.boot.admin_key.value,
            &name("adm"),
            Some(code),
            None,
            Some(WorkareaMode::InProgress),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::ModeNotAllowedHere), "{err:?}");
    // `standard` names nothing the default does not.
    w.app
        .create_workarea_in(
            &w.agent.value,
            &name("std"),
            None,
            None,
            Some(WorkareaMode::Standard),
        )
        .await
        .expect("standard is the default");
}

#[tokio::test]
async fn a_code_parent_is_standard_and_its_children_may_each_be_in_progress() {
    let w = world().await;
    let code = w.code().await;
    w.app
        .set_code_parent(&w.person.value, code.clone(), true)
        .await
        .expect("mark");
    for repo in ["repo-a", "repo-b"] {
        let child = w
            .app
            .create_workarea_in(
                &w.agent.value,
                &name(repo),
                Some(code.clone()),
                None,
                Some(WorkareaMode::InProgress),
            )
            .await
            .expect("each child in progress");
        assert!(child.workarea.in_progress);
    }
    // A code parent that is in progress cannot be a code parent (nesting).
    let err = w
        .app
        .set_workarea_mode(&w.person.value, code.clone(), WorkareaMode::InProgress)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::InProgressRefused { .. }), "{err:?}");
    // Unmarking leaves the children as they are.
    let unmarked = w
        .app
        .set_code_parent(&w.person.value, code.clone(), false)
        .await
        .expect("unmark");
    assert!(!unmarked.children_agent);
    let kept = w
        .app
        .workarea_info(&w.person.value, Some(by_name("pat.code.repo-a")))
        .await
        .expect("child");
    assert!(kept.workarea.in_progress, "children keep their mode");
}

#[tokio::test]
async fn an_agent_key_cannot_create_a_child_under_a_code_parent_it_cannot_write() {
    let w = world().await;
    let code = w.code().await;
    w.app
        .set_code_parent(&w.person.value, code.clone(), true)
        .await
        .expect("mark");
    // Another user's writer key: the parent is not its own.
    let admin = w.boot.admin_key.value.clone();
    let other = w
        .app
        .issue_key(
            &admin,
            &IssueKeyRequest {
                profile: None,
                role: Role::Writer,
                name: fresh_key_name(),
                principal: "bo/agent".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: None,
                crew: None,
                grants: vec![GrantRequest {
                    node: by_name("bo"),
                    role: Role::Writer,
                    deny: Vec::new(),
                    covers: Covers::Subtree,
                }],
                user: Some(name("bo")),
                connect_code: false,
                guardrail_scope: None,
            },
        )
        .await
        .expect("bo's key");
    let err = w
        .app
        .create_workarea_in(
            &other.value,
            &name("x"),
            Some(code),
            None,
            Some(WorkareaMode::InProgress),
        )
        .await
        .unwrap_err();
    assert!(
        !matches!(err, AppError::Store(_)),
        "refused by the access rules, not the store: {err:?}"
    );
}
