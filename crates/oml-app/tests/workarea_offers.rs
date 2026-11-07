//! The workarea dropdown's offer (ADR-0045 d28), over the in-memory
//! doubles: what the pages offer a key on a workarea is exactly what the
//! verbs would do for it now, for an agent key, a person's key, the
//! proposer against another person, and an in-progress workarea with held
//! rows. Each test is named after the sentence it proves.

use oml_app::App;
use oml_app::app::{WorkareaOffer, WorkareaOffers};
use oml_app::domain::WorkareaStatus;
use oml_app::domain::access::Covers;
use oml_app::domain::in_progress::WorkareaMode;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{HeldBy, MemoryKind, Role};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, IssuedKey,
    SubmitRequest, SubmitVerb,
};
use uuid::Uuid;

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

struct World {
    app: TestApp,
    boot: BootstrapResult,
    proj: Uuid,
    agent: IssuedKey,
    agent_dreamer: IssuedKey,
    ann: IssuedKey,
    bob: IssuedKey,
}

async fn key(
    app: &TestApp,
    admin: &str,
    key_name: &str,
    role: Role,
    held_by: HeldBy,
    grants: Vec<GrantRequest>,
) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            profile: None,
            role,
            name: name(key_name),
            principal: key_name.to_owned(),
            held_by: Some(held_by),
            default_workarea: None,
            crew: None,
            grants,
            connect_code: false,
            user: None,
            guardrail_scope: None,
        },
    )
    .await
    .expect("the admin issues the key")
}

/// `universe` → `crew` → `proj`: a writer agent, a dreamer agent and two
/// people, ann and bob, each with a dreamer key of their own on `crew`.
async fn world() -> World {
    let store = MemStore::new();
    let app = App::new(
        store,
        MemSigner::new([7; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("acme-admin"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let crew = app
        .create_workarea(
            &admin,
            &name("crew"),
            Some(boot.root_workarea_id.into()),
            None,
        )
        .await
        .expect("crew")
        .workarea
        .workarea_id;
    let proj = app
        .create_workarea(&admin, &name("proj"), Some(crew.into()), None)
        .await
        .expect("proj")
        .workarea
        .workarea_id;
    let agent = key(
        &app,
        &admin,
        "writer-agent",
        Role::Writer,
        HeldBy::Agent,
        vec![grant(crew, Role::Writer), grant(crew, Role::Reader)],
    )
    .await;
    let agent_dreamer = key(
        &app,
        &admin,
        "dreamer-agent",
        Role::Dreamer,
        HeldBy::Agent,
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await;
    let person = |n: &'static str| {
        key(
            &app,
            &admin,
            n,
            Role::Dreamer,
            HeldBy::Human,
            vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
        )
    };
    let ann = person("ann-review").await;
    let bob = person("bob-review").await;
    World {
        app,
        boot,
        proj,
        agent,
        agent_dreamer,
        ann,
        bob,
    }
}

async fn offers(w: &World, key: &IssuedKey) -> WorkareaOffers {
    w.app
        .workarea_offers_for(key.value.as_str(), w.proj.into())
        .await
        .expect("the offer reads")
}

/// Every act a dropdown could hold for the node as it is: each rung, each
/// live proposal, and the mode the node is not in (setting the mode it is
/// in changes nothing, so it is no act).
async fn candidates(w: &World) -> Vec<WorkareaOffer> {
    let status = w
        .app
        .workarea_status(w.boot.admin_key.value.as_str(), w.proj.into())
        .await
        .expect("the admin reads the status");
    let in_progress = offers(w, &w.ann).await.in_progress;
    let mut all = vec![
        WorkareaOffer::Propose(WorkareaStatus::Committed),
        WorkareaOffer::Propose(WorkareaStatus::Validated),
        if in_progress {
            WorkareaOffer::Commit
        } else {
            WorkareaOffer::InProgress
        },
    ];
    for t in status.pending {
        all.push(WorkareaOffer::Ratify {
            transition_id: t.transition_id,
            from: t.from,
            to: t.to,
        });
    }
    all
}

/// Runs `offer` as `key`, the verb the page would post.
async fn run(w: &World, key: &IssuedKey, offer: WorkareaOffer) -> Result<(), AppError> {
    let node: Reference = w.proj.into();
    match offer {
        WorkareaOffer::Propose(to) => w
            .app
            .propose_transition(key.value.as_str(), node, to)
            .await
            .map(|_| ()),
        WorkareaOffer::Ratify { transition_id, .. } => w
            .app
            .ratify_transition(key.value.as_str(), node, transition_id)
            .await
            .map(|_| ()),
        WorkareaOffer::InProgress => w
            .app
            .set_workarea_mode(key.value.as_str(), node, WorkareaMode::InProgress)
            .await
            .map(|_| ()),
        WorkareaOffer::Commit => w
            .app
            .set_workarea_mode(key.value.as_str(), node, WorkareaMode::Standard)
            .await
            .map(|_| ()),
    }
}

/// The offer equals the allowed set: every candidate not offered is
/// refused by its verb now (a refusal changes nothing, so each is tried),
/// and the one offered act `apply` names succeeds.
async fn assert_offer_is_the_allowed_set(
    w: &World,
    key: &IssuedKey,
    expected: &[WorkareaOffer],
    apply: Option<WorkareaOffer>,
) {
    let offered = offers(w, key).await.offers;
    assert_eq!(offered, expected, "the offer for {}", key.name.as_str());
    for candidate in candidates(w).await {
        if offered.contains(&candidate) {
            continue;
        }
        let refused = run(w, key, candidate).await;
        assert!(
            refused.is_err(),
            "{candidate:?} was not offered to {} but its verb admits it",
            key.name.as_str()
        );
    }
    if let Some(act) = apply {
        assert!(offered.contains(&act));
        run(w, key, act)
            .await
            .unwrap_or_else(|e| panic!("{act:?} was offered and refused: {e:?}"));
    }
}

#[tokio::test]
async fn the_workarea_options_offered_are_exactly_the_acts_allowed() {
    let w = world().await;

    // A writer agent may do nothing to a workarea.
    assert_offer_is_the_allowed_set(&w, &w.agent, &[], None).await;

    // A dreamer agent may propose, never set the mode: that is a person's.
    assert_offer_is_the_allowed_set(
        &w,
        &w.agent_dreamer,
        &[WorkareaOffer::Propose(WorkareaStatus::Committed)],
        None,
    )
    .await;

    // A person may propose the next rung or put it in progress; ann
    // proposes committed.
    assert_offer_is_the_allowed_set(
        &w,
        &w.ann,
        &[
            WorkareaOffer::Propose(WorkareaStatus::Committed),
            WorkareaOffer::InProgress,
        ],
        Some(WorkareaOffer::Propose(WorkareaStatus::Committed)),
    )
    .await;

    // The proposer is never offered the ratification (R2); another person
    // is, and only that one proposal.
    let pending = w
        .app
        .workarea_status(w.boot.admin_key.value.as_str(), w.proj.into())
        .await
        .expect("a status")
        .pending;
    assert_eq!(pending.len(), 1);
    let ratify = WorkareaOffer::Ratify {
        transition_id: pending[0].transition_id,
        from: WorkareaStatus::Development,
        to: WorkareaStatus::Committed,
    };
    let ann = offers(&w, &w.ann).await.offers;
    assert!(!ann.contains(&ratify), "R2: the proposer never ratifies");
    assert_offer_is_the_allowed_set(
        &w,
        &w.bob,
        &[
            WorkareaOffer::Propose(WorkareaStatus::Committed),
            ratify,
            WorkareaOffer::InProgress,
        ],
        Some(ratify),
    )
    .await;

    // Committed now: the next rung is validated, and in progress is a
    // development node's alone.
    let after = offers(&w, &w.ann).await;
    assert_eq!(after.status, WorkareaStatus::Committed);
    assert_offer_is_the_allowed_set(
        &w,
        &w.ann,
        &[WorkareaOffer::Propose(WorkareaStatus::Validated)],
        None,
    )
    .await;
}

#[tokio::test]
async fn an_in_progress_workarea_with_held_rows_withholds_the_commit_with_their_count() {
    let w = world().await;
    w.app
        .set_workarea_mode(
            w.ann.value.as_str(),
            w.proj.into(),
            WorkareaMode::InProgress,
        )
        .await
        .expect("ann puts proj in progress");
    // Inside, no transition is offered: commit first (ADR-0030).
    assert_offer_is_the_allowed_set(&w, &w.ann, &[WorkareaOffer::Commit], None).await;
    for body in ["first draft", "second draft"] {
        w.app
            .submit_memory(
                w.agent.value.as_str(),
                &SubmitRequest {
                    on_similar: Default::default(),
                    anchor: None,
                    origin: None,
                    idempotency_key: format!("k-{body}"),
                    claim_key: None,
                    verb: Some(SubmitVerb::Remember),
                    kind: MemoryKind::Memory,
                    body: body.to_owned(),
                    workarea: Some(w.proj.into()),
                    derives_from: Vec::new(),
                    severity: None,
                    rule: None,
                    valid_from: None,
                    valid_until: None,
                    format: None,
                    title: None,
                },
            )
            .await
            .expect("the agent writes inside");
    }
    let held = offers(&w, &w.ann).await;
    assert!(held.in_progress);
    assert_eq!(held.commit_withheld, Some(2), "the count is shown");
    assert_offer_is_the_allowed_set(&w, &w.ann, &[], None).await;
    let refused = run(&w, &w.ann, WorkareaOffer::Commit)
        .await
        .expect_err("the commit is refused while rows are held");
    assert!(
        matches!(refused, AppError::HeldRemaining { .. }),
        "{refused:?}"
    );
    // Another person sees the same withheld commit; an agent sees none.
    assert_eq!(offers(&w, &w.bob).await.commit_withheld, Some(2));
    assert_eq!(offers(&w, &w.agent).await.commit_withheld, None);
}
