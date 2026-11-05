//! The laws of in-progress workareas and the curator profile (ADR-0030 r2
//! d15), over the in-memory doubles: unreviewed rows are served inside an
//! in-progress scope, labelled, and nowhere else; only a person's key sets
//! the mode; the commit needs zero held; no ranking reads a verdict; R3
//! holds everywhere else; a curator narrows inside and never widens.

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::in_progress::{CuratorRefusal, InProgressReason, KeyProfile, WorkareaMode};
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{HeldBy, MemoryKind, MemoryStatus, Role, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, IssuedKey,
    ListRequest, ProposeClaimEdge, RecallRequest, SubmitRequest, SupersedeRequest, Verdict,
};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

const TEST_SEED: [u8; 32] = [7; 32];

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

fn fresh_key_name() -> Name {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    name(&format!(
        "ip-key-{}",
        ISSUED.fetch_add(1, Ordering::Relaxed)
    ))
}

fn grant(node: Uuid, role: Role) -> GrantRequest {
    GrantRequest {
        node: node.into(),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    }
}

async fn bootstrap(app: &TestApp, tenant: &str) -> BootstrapResult {
    app.bootstrap_tenant(&BootstrapRequest {
        tenant_name: name(tenant),
        admin_key_name: name(&format!("{tenant}-admin")),
        admin_principal: format!("{tenant}-admin"),
    })
    .await
    .expect("bootstrap")
}

async fn workarea(app: &TestApp, admin: &str, name_: &str, parent: Uuid) -> Uuid {
    app.create_workarea(admin, &name(name_), Some(parent.into()), None)
        .await
        .expect("create workarea")
        .workarea
        .workarea_id
}

async fn key(
    app: &TestApp,
    admin: &str,
    role: Role,
    held_by: HeldBy,
    profile: Option<KeyProfile>,
    principal: &str,
    grants: Vec<GrantRequest>,
) -> Result<IssuedKey, AppError> {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            profile,
            role,
            name: fresh_key_name(),
            principal: principal.to_owned(),
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
}

/// The tree `universe` → `crew` → {`proj`, `side`}, with two writer agents,
/// a person's reviewer key and a curator agent on `crew`'s subtree.
struct World {
    app: TestApp,
    store: MemStore,
    boot: BootstrapResult,
    crew: Uuid,
    proj: Uuid,
    side: Uuid,
    ann: IssuedKey,
    bob: IssuedKey,
    person: IssuedKey,
    curator: IssuedKey,
}

async fn world() -> World {
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = bootstrap(&app, "acme").await;
    let admin = boot.admin_key.value.clone();
    let crew = workarea(&app, &admin, "crew", boot.root_workarea_id).await;
    let proj = workarea(&app, &admin, "proj", crew).await;
    let side = workarea(&app, &admin, "side", crew).await;
    let agent = |p: &'static str| {
        key(
            &app,
            &admin,
            Role::Writer,
            HeldBy::Agent,
            None,
            p,
            vec![grant(crew, Role::Writer), grant(crew, Role::Reader)],
        )
    };
    let ann = agent("ann").await.expect("ann");
    let bob = agent("bob").await.expect("bob");
    let person = key(
        &app,
        &admin,
        Role::Dreamer,
        HeldBy::Human,
        None,
        "pat",
        vec![grant(crew, Role::Dreamer), grant(crew, Role::Reader)],
    )
    .await
    .expect("person");
    let curator = key(
        &app,
        &admin,
        Role::Dreamer,
        HeldBy::Agent,
        Some(KeyProfile::Curator),
        "cora",
        vec![
            grant(crew, Role::Reader),
            grant(crew, Role::Writer),
            grant(crew, Role::Dreamer),
        ],
    )
    .await
    .expect("curator");
    World {
        app,
        store,
        boot,
        crew,
        proj,
        side,
        ann,
        bob,
        person,
        curator,
    }
}

impl World {
    async fn mark(&self, node: Uuid) {
        self.app
            .set_workarea_mode(
                self.person.value.as_str(),
                Reference::Id(node),
                WorkareaMode::InProgress,
            )
            .await
            .expect("mark in progress");
    }

    async fn write(&self, key: &IssuedKey, idem: &str, body: &str, node: Uuid) -> Uuid {
        self.app
            .submit_memory(
                key.value.as_str(),
                &SubmitRequest {
                    anchor: None,
                    idempotency_key: idem.to_owned(),
                    claim_key: Some(format!("claim-{idem}")),
                    verb: None,
                    kind: MemoryKind::Memory,
                    body: body.to_owned(),
                    workarea: Some(node.into()),
                    derives_from: Vec::new(),
                    severity: None,
                    rule: None,
                },
            )
            .await
            .expect("submit")
            .receipt
            .memory_id
    }

    async fn recall(
        &self,
        key: &IssuedKey,
        query: &str,
        scope: Uuid,
    ) -> Vec<(Uuid, MemoryStatus, f32)> {
        self.recall_with(key, query, scope, None).await
    }

    async fn recall_with(
        &self,
        key: &IssuedKey,
        query: &str,
        scope: Uuid,
        include_unreviewed: Option<bool>,
    ) -> Vec<(Uuid, MemoryStatus, f32)> {
        self.app
            .recall(
                key.value.as_str(),
                &RecallRequest {
                    anchor: oml_app::types::AnchorFilter::default(),
                    query: query.to_owned(),
                    k: 10,
                    workarea: Some(scope.into()),
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed,
                    max_tokens: None,
                    search: None,
                    match_mode: oml_app::search::MatchMode::Any,
                },
            )
            .await
            .expect("recall")
            .hits
            .iter()
            .map(|h| (h.served.memory.memory_id, h.served.memory.status, h.score))
            .collect()
    }

    async fn listed(&self, key: &IssuedKey, scope: Uuid) -> Vec<Uuid> {
        self.app
            .list_memories(
                key.value.as_str(),
                &ListRequest {
                    own: false,
                    workarea: Some(scope.into()),
                    claim_key: None,
                    state: None,
                    kind: None,
                    since: None,
                    until: None,
                    limit: 50,
                    cursor: None,
                    anchor: oml_app::types::AnchorFilter::default(),
                },
            )
            .await
            .expect("list")
            .items
            .iter()
            .map(|m| m.memory.memory_id)
            .collect()
    }

    fn ids(hits: &[(Uuid, MemoryStatus, f32)]) -> Vec<Uuid> {
        hits.iter().map(|h| h.0).collect()
    }
}

/// Law 1: inside, another author's held row is served to a reader, labelled
/// held; from the parent, from a sibling scope and to another tenant it is
/// not (P-R3-BOUNDARY).
#[tokio::test]
async fn unreviewed_rows_are_served_inside_an_in_progress_scope_and_invisible_from_outside_it() {
    let w = world().await;
    w.mark(w.proj).await;
    let id = w
        .write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;

    let inside = w.recall(&w.bob, "canary", w.proj).await;
    assert_eq!(World::ids(&inside), vec![id], "served inside by default");
    assert_eq!(inside[0].1, MemoryStatus::Held, "labelled unreviewed");
    assert_eq!(
        w.listed(&w.bob, w.proj).await,
        vec![id],
        "the scoped listing inside"
    );
    let got = w
        .app
        .get_memory(w.bob.value.as_str(), id)
        .await
        .expect("get inside");
    assert_eq!(got.memory.status, MemoryStatus::Held);

    // `false` narrows inside.
    assert!(
        w.recall_with(&w.bob, "canary", w.proj, Some(false))
            .await
            .is_empty()
    );
    // From the parent and from a sibling: nothing, even asking for it.
    assert!(
        w.recall_with(&w.bob, "canary", w.crew, Some(true))
            .await
            .is_empty()
    );
    assert!(
        w.recall_with(&w.bob, "canary", w.side, Some(true))
            .await
            .is_empty()
    );
    assert!(w.listed(&w.bob, w.crew).await.is_empty());

    // Another tenant reads nothing of it.
    let other = bootstrap(&w.app, "globex").await;
    let stranger = key(
        &w.app,
        &other.admin_key.value,
        Role::Reader,
        HeldBy::Agent,
        None,
        "sam",
        vec![grant(other.root_workarea_id, Role::Reader)],
    )
    .await
    .expect("stranger");
    assert!(matches!(
        w.app.get_memory(stranger.value.as_str(), id).await,
        Err(AppError::NotFound)
    ));
    assert!(
        w.app
            .recall(
                stranger.value.as_str(),
                &RecallRequest {
                    anchor: oml_app::types::AnchorFilter::default(),
                    query: "canary".to_owned(),
                    k: 10,
                    workarea: None,
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed: Some(true),
                    max_tokens: None,
                    search: None,
                    match_mode: oml_app::search::MatchMode::Any,
                },
            )
            .await
            .expect("stranger recall")
            .hits
            .is_empty()
    );
}

/// Law 2: only a dreamer key a person holds sets the mode.
#[tokio::test]
async fn only_a_persons_key_sets_the_mode() {
    let w = world().await;
    let admin = w.boot.admin_key.value.clone();
    let agent_dreamer = key(
        &w.app,
        &admin,
        Role::Dreamer,
        HeldBy::Agent,
        None,
        "dax",
        vec![grant(w.crew, Role::Dreamer)],
    )
    .await
    .expect("agent dreamer");
    for refused in [
        agent_dreamer.value.as_str(),
        w.ann.value.as_str(),
        w.curator.value.as_str(),
        admin.as_str(),
    ] {
        let err = w
            .app
            .set_workarea_mode(refused, Reference::Id(w.proj), WorkareaMode::InProgress)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::ModeHumanOnly { .. }), "{err:?}");
    }
    w.mark(w.proj).await;
    // Nesting is refused, both ways.
    let err = w
        .app
        .set_workarea_mode(
            w.person.value.as_str(),
            Reference::Id(w.crew),
            WorkareaMode::InProgress,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::InProgressRefused {
                reason: InProgressReason::Nested,
                ..
            }
        ),
        "{err:?}"
    );
    let journal = w.app_journal().await;
    assert_eq!(
        journal
            .iter()
            .filter(|e| e.envelope.event_type == event_types::WORKAREA_MODE_SET)
            .count(),
        1,
        "the one mark is journalled"
    );
}

impl World {
    async fn app_journal(&self) -> Vec<oml_app::domain::SignedEnvelope> {
        self.store
            .events_range(self.boot.tenant_id, 1, 1000)
            .await
            .expect("journal")
    }
}

/// Law 3: the commit is refused while anything inside is held, and from it
/// the promoted rows reach the parent; before it, a promoted row inside is
/// not served outside either.
#[tokio::test]
async fn the_commit_is_refused_while_unreviewed_rows_remain_and_then_releases_the_subtree() {
    let w = world().await;
    w.mark(w.proj).await;
    let id = w
        .write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    let held = w
        .app
        .set_workarea_mode(
            w.person.value.as_str(),
            Reference::Id(w.proj),
            WorkareaMode::Standard,
        )
        .await
        .unwrap_err();
    assert!(matches!(held, AppError::HeldRemaining { .. }), "{held:?}");

    w.app
        .post_verdict(w.person.value.as_str(), id, Verdict::Approve, None)
        .await
        .expect("approve");
    assert!(
        w.recall_with(&w.bob, "canary", w.crew, Some(false))
            .await
            .is_empty(),
        "a promoted row inside stays inside until the commit"
    );
    // A transition inside waits for the commit.
    let err = w
        .app
        .propose_transition(
            w.person.value.as_str(),
            Reference::Id(w.proj),
            oml_app::domain::WorkareaStatus::Committed,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::InProgressRefused {
                reason: InProgressReason::CommitFirst,
                ..
            }
        ),
        "{err:?}"
    );

    w.app
        .set_workarea_mode(
            w.person.value.as_str(),
            Reference::Id(w.proj),
            WorkareaMode::Standard,
        )
        .await
        .expect("commit at zero held");
    assert_eq!(
        World::ids(&w.recall_with(&w.bob, "canary", w.crew, Some(false)).await),
        vec![id],
        "released to the parent"
    );
}

/// Law 4: no ranking input reads a verdict: a held row inside scores as it
/// does once promoted.
#[tokio::test]
async fn a_held_row_inside_scores_as_it_does_once_promoted() {
    let w = world().await;
    w.mark(w.proj).await;
    let id = w
        .write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    w.write(&w.ann, "a2", "canary weights are logged", w.proj)
        .await;
    let before = w.recall(&w.bob, "canary drains", w.proj).await;
    w.app
        .post_verdict(w.person.value.as_str(), id, Verdict::Approve, None)
        .await
        .expect("approve");
    let after = w.recall(&w.bob, "canary drains", w.proj).await;
    let score = |hits: &[(Uuid, MemoryStatus, f32)]| hits.iter().find(|h| h.0 == id).map(|h| h.2);
    assert_eq!(score(&before), score(&after), "approval moved no score");
    assert_eq!(
        World::ids(&before),
        World::ids(&after),
        "approval moved no rank"
    );
}

/// Law 5: outside every in-progress subtree, R3 holds: another author's
/// held row is never served, by recall, listing or get.
#[tokio::test]
async fn r3_holds_outside_every_in_progress_subtree() {
    let w = world().await;
    w.mark(w.proj).await;
    let id = w
        .write(&w.ann, "s1", "the canary drains before promotion", w.side)
        .await;
    assert!(
        w.recall_with(&w.bob, "canary", w.side, Some(true))
            .await
            .is_empty()
    );
    assert!(w.listed(&w.bob, w.side).await.is_empty());
    assert!(matches!(
        w.app.get_memory(w.bob.value.as_str(), id).await,
        Err(AppError::NotFound)
    ));
}

/// Law 6a: a curator key never promotes.
#[tokio::test]
async fn a_curator_key_cannot_promote_out() {
    let w = world().await;
    w.mark(w.proj).await;
    let id = w
        .write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    let err = w
        .app
        .post_verdict(w.curator.value.as_str(), id, Verdict::Approve, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Curator {
                refusal: CuratorRefusal::NarrowOnly,
                ..
            }
        ),
        "{err:?}"
    );
    for refused in [
        w.app
            .set_workarea_mode(
                w.curator.value.as_str(),
                Reference::Id(w.proj),
                WorkareaMode::Standard,
            )
            .await
            .unwrap_err(),
        w.app
            .review_next(w.curator.value.as_str())
            .await
            .unwrap_err(),
    ] {
        assert!(
            matches!(
                refused,
                AppError::ModeHumanOnly { .. }
                    | AppError::Curator {
                        refusal: CuratorRefusal::NarrowOnly,
                        ..
                    }
            ),
            "{refused:?}"
        );
    }
}

/// Law 6b: a curator never acts outside an in-progress scope.
#[tokio::test]
async fn a_curator_cannot_act_outside_an_in_progress_scope() {
    let w = world().await;
    w.mark(w.proj).await;
    let id = w
        .write(&w.ann, "s1", "the canary drains before promotion", w.side)
        .await;
    let err = w
        .app
        .post_verdict(w.curator.value.as_str(), id, Verdict::Reject, Some("noise"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Curator {
                refusal: CuratorRefusal::OutsideInProgress,
                ..
            }
        ),
        "{err:?}"
    );
    let err = w
        .app
        .submit_memory(
            w.curator.value.as_str(),
            &SubmitRequest {
                anchor: None,
                idempotency_key: "c1".to_owned(),
                claim_key: Some("claim-c1".to_owned()),
                verb: None,
                kind: MemoryKind::Memory,
                body: "summary".to_owned(),
                workarea: Some(w.side.into()),
                derives_from: Vec::new(),
                severity: None,
                rule: None,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Curator {
                refusal: CuratorRefusal::OutsideInProgress,
                ..
            }
        ),
        "{err:?}"
    );
}

/// Law 6c: a curator never curates its own writes (R2 per key and
/// principal).
#[tokio::test]
async fn a_curator_cannot_curate_its_own_writes() {
    let w = world().await;
    w.mark(w.proj).await;
    let own = w
        .write(&w.curator, "c1", "the canary summary", w.proj)
        .await;
    let err = w
        .app
        .post_verdict(
            w.curator.value.as_str(),
            own,
            Verdict::Reject,
            Some("noise"),
        )
        .await
        .unwrap_err();
    // R2's own refusal, review.author_in_lineage on the wire.
    assert!(matches!(err, AppError::Verdict(_)), "{err:?}");
    let other = w.write(&w.ann, "a1", "the canary drains", w.proj).await;
    let err = w
        .app
        .propose_claim_edge(
            w.curator.value.as_str(),
            &ProposeClaimEdge {
                from_artifact_id: own,
                to_artifact_id: other,
                label: "contradicts".to_owned(),
            },
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AuthorInLineage), "{err:?}");
}

/// Law 6d: every curator act is journalled under its key and reversible:
/// a rejection keeps the bytes and the claim key takes a new write; a
/// retirement is a held proposal a person may reject.
#[tokio::test]
async fn every_curator_act_is_journalled_under_its_key_and_reversible() {
    let w = world().await;
    w.mark(w.proj).await;
    let dup = w
        .write(&w.ann, "a1", "the canary drains first", w.proj)
        .await;
    w.app
        .post_verdict(
            w.curator.value.as_str(),
            dup,
            Verdict::Reject,
            Some("duplicate"),
        )
        .await
        .expect("curator rejects inside");
    let journal = w.app_journal().await;
    let reject = journal
        .iter()
        .rfind(|e| e.envelope.event_type == event_types::REVIEW_REJECT)
        .expect("journalled");
    assert_eq!(reject.envelope.api_key_id, Some(w.curator.key_id));
    let kept = w
        .app
        .get_memory(w.ann.value.as_str(), dup)
        .await
        .expect("bytes kept");
    assert_eq!(kept.memory.status, MemoryStatus::Rejected);
    assert_eq!(kept.memory.body, "the canary drains first");
    assert!(
        w.recall(&w.bob, "canary", w.proj).await.is_empty(),
        "a rejected row is narrowed away inside"
    );
    // Reversed: the claim key takes the text again.
    w.write(&w.ann, "a1-again", "the canary drains first", w.proj)
        .await;

    // A retirement is a held proposal, and the tip stays served until a
    // person decides.
    let tip = w.write(&w.bob, "b1", "weights are logged", w.proj).await;
    w.app
        .post_verdict(w.person.value.as_str(), tip, Verdict::Approve, None)
        .await
        .expect("approve");
    let proposal = w
        .app
        .supersede(
            w.curator.value.as_str(),
            &SupersedeRequest {
                anchor: None,
                expected_version: 1,
                idempotency_key: "r1".to_owned(),
                target_id: tip,
                body: "noise".to_owned(),
                kind: None,
                retirement: true,
                derives_from: Vec::new(),
                reason: None,
                workarea: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
            },
        )
        .await
        .expect("curator proposes a retirement")
        .receipt
        .memory_id;
    assert!(World::ids(&w.recall(&w.bob, "weights", w.proj).await).contains(&tip));
    w.app
        .post_verdict(
            w.person.value.as_str(),
            proposal,
            Verdict::Reject,
            Some("keep it"),
        )
        .await
        .expect("a person reverses the retirement");
    assert!(World::ids(&w.recall(&w.bob, "weights", w.proj).await).contains(&tip));
}

/// A curator is a dreamer key held by an agent, and nothing else.
#[tokio::test]
async fn the_curator_profile_needs_a_dreamer_key_held_by_an_agent() {
    let w = world().await;
    let admin = w.boot.admin_key.value.clone();
    for (role, held_by) in [
        (Role::Dreamer, HeldBy::Human),
        (Role::Writer, HeldBy::Agent),
    ] {
        let err = key(
            &w.app,
            &admin,
            role,
            held_by,
            Some(KeyProfile::Curator),
            "x",
            vec![grant(w.crew, Role::Reader)],
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::ProfileMismatch { .. }), "{err:?}");
    }
}
