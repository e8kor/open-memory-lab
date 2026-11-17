//! The laws of in-progress workspaces and the curator profile (ADR-0030 r2
//! d15), over the in-memory doubles: unreviewed rows are served inside an
//! in-progress scope, labelled, and nowhere else; only a person's key sets
//! the mode; the commit needs zero held; no ranking reads a verdict; R3
//! holds everywhere else; a curator narrows inside and never widens.

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::access::Grant;
use oml_app::domain::in_progress::{CuratorRefusal, InProgressReason, KeyProfile, WorkspaceMode};
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{EventDraft, MemoryKind, MemoryStatus, Role, UsedBy, event_types};
use oml_app::domain::{WorkspaceStatus, next_envelope, sign};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{CommitRequest, JournalStore as _, MetadataFold, OutboxRow};
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, IssuedKey,
    ListRequest, ProposeClaimEdge, RecallRequest, SubmitRequest, SupersedeRequest, Verdict,
};
use oml_app::types::{ClaimEdgeQuery, ProposeBriefEdge};
use serde_json::{Value, json};
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

async fn workspace(app: &TestApp, admin: &str, name_: &str, parent: Uuid) -> Uuid {
    app.create_workspace(admin, &name(name_), Some(parent.into()), None)
        .await
        .expect("create workspace")
        .workspace
        .workspace_id
}

async fn key(
    app: &TestApp,
    admin: &str,
    role: Role,
    used_by: UsedBy,
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
            used_by: Some(used_by),
            default_workspace: None,
            crew: None,
            grants,
            connect_code: false,
            user: None,
            guardrail_scope: None,
            expiry: None,
            requests_per_minute: None,
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
    let crew = workspace(&app, &admin, "crew", boot.root_workspace_id).await;
    let proj = workspace(&app, &admin, "proj", crew).await;
    let side = workspace(&app, &admin, "side", crew).await;
    let agent = |p: &'static str| {
        key(
            &app,
            &admin,
            Role::Writer,
            UsedBy::Agent,
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
        Role::Reviewer,
        UsedBy::Person,
        None,
        "pat",
        vec![grant(crew, Role::Reviewer), grant(crew, Role::Reader)],
    )
    .await
    .expect("person");
    let curator = key(
        &app,
        &admin,
        Role::Reviewer,
        UsedBy::Agent,
        Some(KeyProfile::Curator),
        "cora",
        vec![
            grant(crew, Role::Reader),
            grant(crew, Role::Writer),
            grant(crew, Role::Reviewer),
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
            .set_workspace_mode(
                self.person.value.as_str(),
                Reference::Id(node),
                WorkspaceMode::InProgress,
            )
            .await
            .expect("mark in progress");
    }

    async fn write(&self, key: &IssuedKey, idem: &str, body: &str, node: Uuid) -> Uuid {
        self.app
            .submit_memory(
                key.value.as_str(),
                &SubmitRequest {
                    origin: None,
                    on_similar: Default::default(),
                    valid_from: None,
                    valid_until: None,
                    anchor: None,
                    idempotency_key: idem.to_owned(),
                    claim_key: Some(format!("claim-{idem}")),
                    verb: None,
                    kind: MemoryKind::Memory,
                    body: body.to_owned(),
                    workspace: Some(node.into()),
                    derives_from: Vec::new(),
                    severity: None,
                    rule: None,
                    kind_fields: None,
                    format: None,
                    title: None,
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
                    sources: None,
                    timings: false,
                    include_expired: false,
                    now: None,
                    anchor: oml_app::types::AnchorFilter::default(),
                    query: query.to_owned(),
                    k: 10,
                    workspace: Some(scope.into()),
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed,
                    max_tokens: None,
                    search: None,
                    match_mode: oml_app::search::MatchMode::Any,
                    as_of: None,
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
                    workspace: Some(scope.into()),
                    claim_key: None,
                    state: None,
                    kind: None,
                    since: None,
                    until: None,
                    limit: 50,
                    cursor: None,
                    anchor: oml_app::types::AnchorFilter::default(),
                    author: None,
                    tip: None,
                    as_of: None,
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
/// held; from a sibling scope and to another tenant it is not
/// (P-R3-BOUNDARY). From an ancestor it is, since ADR-0080 d16: see
/// `a_read_at_an_ancestor_serves_a_descendant_in_progress_root_marked_unreviewed`.
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
    // From a sibling: nothing, even asking for it.
    assert!(
        w.recall_with(&w.bob, "canary", w.side, Some(true))
            .await
            .is_empty()
    );
    assert!(w.listed(&w.bob, w.side).await.is_empty());

    // Another tenant reads nothing of it.
    let other = bootstrap(&w.app, "globex").await;
    let stranger = key(
        &w.app,
        &other.admin_key.value,
        Role::Reader,
        UsedBy::Agent,
        None,
        "sam",
        vec![grant(other.root_workspace_id, Role::Reader)],
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
                    sources: None,
                    timings: false,
                    include_expired: false,
                    now: None,
                    anchor: oml_app::types::AnchorFilter::default(),
                    query: "canary".to_owned(),
                    k: 10,
                    workspace: None,
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed: Some(true),
                    max_tokens: None,
                    search: None,
                    match_mode: oml_app::search::MatchMode::Any,
                    as_of: None,
                },
            )
            .await
            .expect("stranger recall")
            .hits
            .is_empty()
    );
}

/// A held row the scan held stays its author's inside an in-progress scope:
/// another reader there neither recalls, lists nor reads it, while the
/// author still does, and a held row the scan passed is served as before
/// (ADR-0030 d3; ADR-0065 d5, amended 2026-11-10). ADR-0074 d18 names it
/// for phase 1.
#[tokio::test]
async fn a_held_version_with_a_hold_finding_is_never_served_unreviewed_in_progress() {
    let w = world().await;
    w.mark(w.proj).await;
    let flagged = w
        .write(
            &w.ann,
            "f1",
            "the canary drains first [needs-person]",
            w.proj,
        )
        .await;
    let clean = w
        .write(&w.ann, "f2", "the canary rolls back on error", w.proj)
        .await;
    let sorted = |mut ids: Vec<Uuid>| {
        ids.sort();
        ids
    };

    // Another reader inside: the clean row alone, by recall, listing and id.
    assert_eq!(
        World::ids(&w.recall_with(&w.bob, "canary", w.proj, Some(true)).await),
        vec![clean]
    );
    assert_eq!(w.listed(&w.bob, w.proj).await, vec![clean]);
    assert!(matches!(
        w.app.get_memory(w.bob.value.as_str(), flagged).await,
        Err(AppError::NotFound)
    ));
    w.app
        .get_memory(w.bob.value.as_str(), clean)
        .await
        .expect("the clean row inside");

    // The author: both rows, by recall, by its own listing and by id.
    assert_eq!(
        sorted(World::ids(&w.recall(&w.ann, "canary", w.proj).await)),
        sorted(vec![flagged, clean])
    );
    let own: Vec<Uuid> = w
        .app
        .list_memories(
            w.ann.value.as_str(),
            &ListRequest {
                own: true,
                workspace: Some(w.proj.into()),
                claim_key: None,
                state: None,
                kind: None,
                since: None,
                until: None,
                limit: 50,
                cursor: None,
                anchor: oml_app::types::AnchorFilter::default(),
                author: None,
                tip: None,
                as_of: None,
            },
        )
        .await
        .expect("own list")
        .items
        .iter()
        .map(|m| m.memory.memory_id)
        .collect();
    assert_eq!(sorted(own), sorted(vec![flagged, clean]));
    let got = w
        .app
        .get_memory(w.ann.value.as_str(), flagged)
        .await
        .expect("the author reads its flagged row");
    assert_eq!(got.memory.status, MemoryStatus::Held);
}

/// Law 2: only a reviewer key a person holds sets the mode.
#[tokio::test]
async fn only_a_persons_key_sets_the_mode() {
    let w = world().await;
    let admin = w.boot.admin_key.value.clone();
    let agent_reviewer = key(
        &w.app,
        &admin,
        Role::Reviewer,
        UsedBy::Agent,
        None,
        "dax",
        vec![grant(w.crew, Role::Reviewer)],
    )
    .await
    .expect("agent reviewer");
    for refused in [
        agent_reviewer.value.as_str(),
        w.ann.value.as_str(),
        w.curator.value.as_str(),
        admin.as_str(),
    ] {
        let err = w
            .app
            .set_workspace_mode(refused, Reference::Id(w.proj), WorkspaceMode::InProgress)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::ModeHumanOnly { .. }), "{err:?}");
    }
    w.mark(w.proj).await;
    // Nesting is refused, both ways.
    let err = w
        .app
        .set_workspace_mode(
            w.person.value.as_str(),
            Reference::Id(w.crew),
            WorkspaceMode::InProgress,
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
            .filter(|e| e.envelope.event_type == event_types::WORKSPACE_MODE_SET)
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

/// Law 3: the commit is refused while anything inside is held; the
/// promoted rows inside reach the parent before it and after it (ADR-0080
/// d16).
#[tokio::test]
async fn the_commit_is_refused_while_unreviewed_rows_remain_and_then_releases_the_subtree() {
    let w = world().await;
    w.mark(w.proj).await;
    let id = w
        .write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    let held = w
        .app
        .set_workspace_mode(
            w.person.value.as_str(),
            Reference::Id(w.proj),
            WorkspaceMode::Standard,
        )
        .await
        .unwrap_err();
    assert!(matches!(held, AppError::HeldRemaining { .. }), "{held:?}");

    w.app
        .post_verdict(w.person.value.as_str(), id, Verdict::Approve, None)
        .await
        .expect("approve");
    // Since ADR-0080 d16 an ancestor's read serves the subtree's promoted
    // rows before the commit; the commit changes nothing for it.
    assert_eq!(
        World::ids(&w.recall_with(&w.bob, "canary", w.crew, Some(false)).await),
        vec![id],
        "a promoted row inside is served to the ancestor as promoted"
    );
    // A transition inside waits for the commit.
    let err = w
        .app
        .propose_transition(
            w.person.value.as_str(),
            Reference::Id(w.proj),
            oml_app::domain::WorkspaceStatus::Protected,
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
        .set_workspace_mode(
            w.person.value.as_str(),
            Reference::Id(w.proj),
            WorkspaceMode::Standard,
        )
        .await
        .expect("commit at zero held");
    assert_eq!(
        World::ids(&w.recall_with(&w.bob, "canary", w.crew, Some(false)).await),
        vec![id],
        "still served to the parent after the commit"
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
            .set_workspace_mode(
                w.curator.value.as_str(),
                Reference::Id(w.proj),
                WorkspaceMode::Standard,
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
                origin: None,
                on_similar: Default::default(),
                valid_from: None,
                valid_until: None,
                anchor: None,
                idempotency_key: "c1".to_owned(),
                claim_key: Some("claim-c1".to_owned()),
                verb: None,
                kind: MemoryKind::Memory,
                body: "summary".to_owned(),
                workspace: Some(w.side.into()),
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: None,
                format: None,
                title: None,
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
                origin: None,
                on_similar: Default::default(),
                valid_from: None,
                valid_until: None,
                anchor: None,
                expected_version: 1,
                idempotency_key: "r1".to_owned(),
                target_id: tip,
                body: "noise".to_owned(),
                kind: None,
                retirement: true,
                derives_from: Vec::new(),
                reason: None,
                workspace: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
                kind_fields: None,
                format: None,
                title: None,
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

/// A curator is a reviewer key used by an agent, and nothing else.
#[tokio::test]
async fn the_curator_profile_needs_a_reviewer_key_used_by_an_agent() {
    let w = world().await;
    let admin = w.boot.admin_key.value.clone();
    for (role, used_by) in [
        (Role::Reviewer, UsedBy::Person),
        (Role::Writer, UsedBy::Agent),
    ] {
        let err = key(
            &w.app,
            &admin,
            role,
            used_by,
            Some(KeyProfile::Curator),
            "x",
            vec![grant(w.crew, Role::Reader)],
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::ProfileMismatch { .. }), "{err:?}");
    }
}

// ---------------------------------------------------------------------------
// Review of #79: the cases a mark must not open (H1, M1–M4) and the curator
// checks the first laws left unkilled.
// ---------------------------------------------------------------------------

/// Seals one event on the tenant's head and commits it with `fold`, as the
/// lead would: the cases built beneath the use cases, through the store.
static FIXTURE: AtomicU64 = AtomicU64::new(0);

async fn seal_onto(w: &World, event_type: &str, payload: Value, fold: MetadataFold) {
    let events = w.app_journal().await;
    let last = events.last().expect("a chain");
    let draft = EventDraft {
        tenant_id: w.boot.tenant_id,
        event_id: Uuid::from_u128(
            0xF1C5_0000 + u128::from(FIXTURE.fetch_add(1, Ordering::Relaxed)),
        ),
        event_type: event_type.to_owned(),
        principal: "fixture".to_owned(),
        api_key_id: None,
        at: last.envelope.at.clone(),
        payload,
    };
    let envelope = next_envelope(Some(last), draft).expect("a lawful draft");
    let event = sign(envelope, &SigningKey::from_bytes(&TEST_SEED)).expect("signing");
    let seq = event.envelope.seq;
    w.store
        .commit(CommitRequest {
            outbox: OutboxRow {
                event_id: event.envelope.event_id,
                topic: event_type.to_owned(),
                payload: json!({ "seq": seq }),
            },
            event,
            fold,
            idempotency: None,
            ceiling: None,
            written: None,
        })
        .await
        .expect("the fixture commit");
}

impl World {
    /// A second person's reviewer key, another principal, for two-identity acts.
    async fn second_person(&self) -> IssuedKey {
        key(
            &self.app,
            &self.boot.admin_key.value,
            Role::Reviewer,
            UsedBy::Person,
            None,
            "quinn",
            vec![
                grant(self.crew, Role::Reviewer),
                grant(self.crew, Role::Reader),
            ],
        )
        .await
        .expect("second person")
    }

    async fn promoted(&self, key: &IssuedKey, idem: &str, body: &str, node: Uuid) -> Uuid {
        let id = self.write(key, idem, body, node).await;
        self.app
            .post_verdict(self.person.value.as_str(), id, Verdict::Approve, None)
            .await
            .expect("approve");
        id
    }

    async fn edges(
        &self,
        key: &IssuedKey,
        workspace: Option<Uuid>,
        artifact: Option<Uuid>,
    ) -> usize {
        self.app
            .claim_edges(
                key.value.as_str(),
                ClaimEdgeQuery {
                    workspace: workspace.map(Into::into),
                    artifact_id: artifact,
                },
                None,
                50,
            )
            .await
            .expect("edge read")
            .items
            .len()
    }

    async fn flag(&self, key: &IssuedKey, from: Uuid, to: Uuid) -> Result<Uuid, AppError> {
        self.app
            .propose_claim_edge(
                key.value.as_str(),
                &ProposeClaimEdge {
                    from_artifact_id: from,
                    to_artifact_id: to,
                    label: "contradicts".to_owned(),
                },
                None,
            )
            .await
            .map(|o| o.proposal.edge_id)
    }
}

/// H1: a mark is refused while any node beneath it is not `development`, so
/// R3 holds in every `protected` and `frozen` workspace: another author's
/// held row there is served neither by get nor by recall.
#[tokio::test]
async fn marking_is_refused_over_a_committed_node_and_r3_holds_there() {
    let w = world().await;
    let kid = workspace(&w.app, &w.boot.admin_key.value, "kid", w.proj).await;
    let quinn = w.second_person().await;
    let proposal = w
        .app
        .propose_transition(
            w.person.value.as_str(),
            Reference::Id(kid),
            WorkspaceStatus::Protected,
        )
        .await
        .expect("propose");
    w.app
        .ratify_transition(
            quinn.value.as_str(),
            Reference::Id(kid),
            proposal.transition_id,
        )
        .await
        .expect("ratify");
    let id = w.write(&w.ann, "k1", "the canary drains in kid", kid).await;
    let err = w
        .app
        .set_workspace_mode(
            w.person.value.as_str(),
            Reference::Id(w.proj),
            WorkspaceMode::InProgress,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::InProgressRefused {
                reason: InProgressReason::Status,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(matches!(
        w.app.get_memory(w.bob.value.as_str(), id).await,
        Err(AppError::NotFound)
    ));
    assert!(
        w.recall_with(&w.bob, "canary", w.proj, Some(true))
            .await
            .is_empty()
    );
}

/// H1, defence in depth: a mark over a protected node, forced through the
/// store, still relaxes R3 on `development` nodes only, every node from the
/// row's up to the mark included.
#[tokio::test]
async fn a_committed_node_under_a_mark_forced_through_the_store_keeps_r3() {
    let w = world().await;
    let kid = workspace(&w.app, &w.boot.admin_key.value, "kid", w.proj).await;
    let grandkid = workspace(&w.app, &w.boot.admin_key.value, "grandkid", kid).await;
    let quinn = w.second_person().await;
    let proposal = w
        .app
        .propose_transition(
            w.person.value.as_str(),
            Reference::Id(kid),
            WorkspaceStatus::Protected,
        )
        .await
        .expect("propose");
    w.app
        .ratify_transition(
            quinn.value.as_str(),
            Reference::Id(kid),
            proposal.transition_id,
        )
        .await
        .expect("ratify");
    let in_kid = w.write(&w.ann, "k1", "the canary drains in kid", kid).await;
    let below = w
        .write(&w.ann, "g1", "the canary drains below kid", grandkid)
        .await;
    let in_proj = w
        .write(&w.ann, "p1", "the canary drains in proj", w.proj)
        .await;
    seal_onto(
        &w,
        event_types::WORKSPACE_MODE_SET,
        json!({ "workspace_id": w.proj.to_string(), "mode": "in_progress", "from": "standard" }),
        MetadataFold::WorkspaceModeSet {
            workspace_id: w.proj,
            in_progress: true,
            held_free: Vec::new(),
        },
    )
    .await;
    for id in [in_kid, below] {
        assert!(matches!(
            w.app.get_memory(w.bob.value.as_str(), id).await,
            Err(AppError::NotFound)
        ));
    }
    assert_eq!(
        World::ids(&w.recall_with(&w.bob, "canary", w.proj, Some(true)).await),
        vec![in_proj],
        "only the development node's held row"
    );
}

/// M1: a brief edge ratified before a mark serves no claim from inside the
/// subtree at a node outside it, in the brief read or the workspace read.
#[tokio::test]
async fn a_brief_edge_made_before_a_mark_serves_no_inside_claim_outside() {
    let w = world().await;
    let quinn = w.second_person().await;
    let claim = w
        .promoted(&w.ann, "a1", "the canary drains first", w.proj)
        .await;
    let edge = w
        .app
        .propose_brief_edge(
            w.person.value.as_str(),
            &ProposeBriefEdge {
                artifact_id: claim,
                workspace: w.side.into(),
            },
            None,
        )
        .await
        .expect("propose brief")
        .proposal
        .edge_id;
    w.app
        .ratify_brief_edge(quinn.value.as_str(), edge, None)
        .await
        .expect("ratify brief");
    w.mark(w.proj).await;
    let page = w
        .app
        .brief_edges(w.bob.value.as_str(), Reference::Id(w.side), None, 50)
        .await
        .expect("brief read");
    assert!(page.items.is_empty(), "{:?}", page.items);
    let info = w
        .app
        .workspace_info(w.bob.value.as_str(), Some(Reference::Id(w.side)))
        .await
        .expect("workspace read");
    assert!(info.brief.items.is_empty());
}

/// M2 and d7: an edge across the boundary is refused when proposed after a
/// mark, and a pending one proposed before it is refused at ratification.
#[tokio::test]
async fn an_edge_across_the_boundary_is_neither_proposed_nor_ratified_after_a_mark() {
    let w = world().await;
    let quinn = w.second_person().await;
    let inside = w
        .promoted(&w.ann, "a1", "the canary drains first", w.proj)
        .await;
    let outside = w
        .promoted(&w.bob, "b1", "the canary never drains", w.side)
        .await;
    let pending = w
        .flag(&w.person, inside, outside)
        .await
        .expect("proposed before");
    let brief = w
        .app
        .propose_brief_edge(
            w.person.value.as_str(),
            &ProposeBriefEdge {
                artifact_id: inside,
                workspace: w.side.into(),
            },
            None,
        )
        .await
        .expect("brief proposed before")
        .proposal
        .edge_id;
    w.mark(w.proj).await;
    let crossing = |err: AppError| {
        assert!(
            matches!(
                err,
                AppError::InProgressRefused {
                    reason: InProgressReason::EdgeCrosses,
                    ..
                }
            ),
            "{err:?}"
        );
    };
    crossing(w.flag(&w.person, inside, outside).await.unwrap_err());
    crossing(
        w.app
            .ratify_claim_edge(quinn.value.as_str(), pending, None)
            .await
            .unwrap_err(),
    );
    crossing(
        w.app
            .ratify_brief_edge(quinn.value.as_str(), brief, None)
            .await
            .unwrap_err(),
    );
}

/// M3: a curator's `contradicts` flag between two held claims is served to
/// the readers inside, the person who commits included, and to no read
/// scoped outside.
#[tokio::test]
async fn a_curator_flag_between_held_claims_is_served_to_readers_inside() {
    let w = world().await;
    w.mark(w.proj).await;
    let a = w
        .write(&w.ann, "a1", "the canary drains first", w.proj)
        .await;
    let b = w
        .write(&w.bob, "b1", "the canary never drains", w.proj)
        .await;
    w.flag(&w.curator, a, b).await.expect("curator flags");
    assert_eq!(
        w.edges(&w.person, Some(w.proj), Some(a)).await,
        1,
        "the person"
    );
    assert_eq!(
        w.edges(&w.bob, Some(w.proj), Some(a)).await,
        1,
        "a reader inside"
    );
    assert_eq!(
        w.edges(&w.person, Some(w.crew), Some(a)).await,
        0,
        "from the parent"
    );
}

/// M4: an edge read with no scope and no default workspace leaves every
/// in-progress subtree out; the key is built through the store, since
/// issuance always gives a granted key a default.
#[tokio::test]
async fn an_unscoped_edge_read_without_a_default_leaves_every_in_progress_subtree_out() {
    let w = world().await;
    let quinn = w.second_person().await;
    w.mark(w.proj).await;
    let a = w
        .promoted(&w.ann, "a1", "the canary drains first", w.proj)
        .await;
    let b = w
        .promoted(&w.bob, "b1", "the canary never drains", w.proj)
        .await;
    let edge = w.flag(&w.person, a, b).await.expect("inside edge");
    w.app
        .ratify_claim_edge(quinn.value.as_str(), edge, None)
        .await
        .expect("ratify inside");
    let bare = key(
        &w.app,
        &w.boot.admin_key.value,
        Role::Reader,
        UsedBy::Agent,
        None,
        "nemo",
        Vec::new(),
    )
    .await
    .expect("a zero-grant key, no default workspace");
    let grant_id = Uuid::from_u128(0x6A47_0001);
    seal_onto(
        &w,
        event_types::GRANT_ISSUED,
        json!({ "key_id": bare.key_id.to_string(), "grant_id": grant_id.to_string() }),
        MetadataFold::GrantIssued {
            key_id: bare.key_id,
            grant: Grant {
                grant_id,
                node: w.crew,
                role: Role::Reader,
                deny: Vec::new(),
                covers: Covers::Subtree,
            },
        },
    )
    .await;
    assert_eq!(
        w.edges(&w.bob, Some(w.proj), None).await,
        1,
        "served inside"
    );
    assert_eq!(w.edges(&bare, None, None).await, 0, "unscoped, no default");
}

/// The curator's R2 by principal: a row another key of the curator's
/// principal wrote is never the curator's to flag.
#[tokio::test]
async fn a_curator_never_flags_a_row_its_principal_wrote_through_another_key() {
    let w = world().await;
    w.mark(w.proj).await;
    let twin = key(
        &w.app,
        &w.boot.admin_key.value,
        Role::Writer,
        UsedBy::Agent,
        None,
        "cora",
        vec![grant(w.crew, Role::Writer), grant(w.crew, Role::Reader)],
    )
    .await
    .expect("same principal, another key");
    let mine = w
        .write(&twin, "t1", "the canary drains first", w.proj)
        .await;
    let other = w
        .write(&w.ann, "a1", "the canary never drains", w.proj)
        .await;
    let err = w.flag(&w.curator, mine, other).await.unwrap_err();
    assert!(matches!(err, AppError::AuthorInLineage), "{err:?}");
}

/// The curator's R2 over the lineage closure: a row derived from the
/// curator's own write is never the curator's to flag.
#[tokio::test]
async fn a_curator_never_flags_a_row_derived_from_its_own_write() {
    let w = world().await;
    w.mark(w.proj).await;
    let own = w
        .write(&w.curator, "c1", "the canary summary", w.proj)
        .await;
    let derived = w
        .app
        .submit_memory(
            w.ann.value.as_str(),
            &SubmitRequest {
                origin: None,
                on_similar: Default::default(),
                valid_from: None,
                valid_until: None,
                anchor: None,
                idempotency_key: "a1".to_owned(),
                claim_key: Some("claim-a1".to_owned()),
                verb: None,
                kind: MemoryKind::Memory,
                body: "the canary drains, per the summary".to_owned(),
                workspace: Some(w.proj.into()),
                derives_from: vec![own],
                severity: None,
                rule: None,
                kind_fields: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("derived")
        .receipt
        .memory_id;
    let other = w
        .write(&w.bob, "b1", "the canary never drains", w.proj)
        .await;
    let err = w.flag(&w.curator, derived, other).await.unwrap_err();
    assert!(matches!(err, AppError::AuthorInLineage), "{err:?}");
}

/// d6, in the committing transaction: the store refuses a commit whose
/// subtree holds a held version, whatever the use case tested before it.
#[tokio::test]
async fn the_store_refuses_a_commit_while_its_subtree_holds_a_held_version() {
    let w = world().await;
    w.mark(w.proj).await;
    w.write(&w.ann, "a1", "the canary drains first", w.proj)
        .await;
    let events = w.app_journal().await;
    let last = events.last().expect("a chain");
    let draft = EventDraft {
        tenant_id: w.boot.tenant_id,
        event_id: Uuid::from_u128(0xF1C5_9999),
        event_type: event_types::WORKSPACE_MODE_SET.to_owned(),
        principal: "fixture".to_owned(),
        api_key_id: None,
        at: last.envelope.at.clone(),
        payload: json!({ "workspace_id": w.proj.to_string(), "mode": "standard" }),
    };
    let envelope = next_envelope(Some(last), draft).expect("a lawful draft");
    let event = sign(envelope, &SigningKey::from_bytes(&TEST_SEED)).expect("signing");
    let refused = w
        .store
        .commit(CommitRequest {
            outbox: OutboxRow {
                event_id: event.envelope.event_id,
                topic: event_types::WORKSPACE_MODE_SET.to_owned(),
                payload: json!({}),
            },
            event,
            fold: MetadataFold::WorkspaceModeSet {
                workspace_id: w.proj,
                in_progress: false,
                held_free: vec![w.proj],
            },
            idempotency: None,
            ceiling: None,
            written: None,
        })
        .await;
    assert!(
        matches!(refused, Err(oml_app::ports::StoreError::HeldRemaining)),
        "{refused:?}"
    );
}

/// One row's place for the curator: its name, authoring key, author
/// principal, lineage keys and lineage principals.
type R2Case = (&'static str, Uuid, String, Vec<Uuid>, Vec<String>);

/// d9's R2 per key and per principal, clause by clause: rows built through
/// the store so that each carries the curator in exactly one place — its
/// authoring key, its author's principal, its lineage keys or its lineage
/// principals — and each is refused to the curator on its own.
#[tokio::test]
async fn each_r2_clause_alone_keeps_a_curator_off_a_row() {
    let w = world().await;
    w.mark(w.proj).await;
    let template_id = w.write(&w.ann, "t0", "the canary template", w.proj).await;
    let template = w
        .store
        .memory(w.boot.tenant_id, template_id)
        .await
        .expect("read")
        .expect("the row");
    let other = w
        .write(&w.bob, "b1", "the canary never drains", w.proj)
        .await;
    let cora = "cora".to_owned();
    let cases: [R2Case; 4] = [
        (
            "authoring key",
            w.curator.key_id,
            "x".to_owned(),
            vec![],
            vec![],
        ),
        (
            "author principal",
            w.ann.key_id,
            cora.clone(),
            vec![],
            vec![],
        ),
        (
            "lineage key",
            w.ann.key_id,
            "ann".to_owned(),
            vec![w.curator.key_id],
            vec![],
        ),
        (
            "lineage principal",
            w.ann.key_id,
            "ann".to_owned(),
            vec![],
            vec![cora],
        ),
    ];
    for (n, (case, author_key_id, author_principal, lineage_key_ids, lineage_principals)) in
        cases.into_iter().enumerate()
    {
        let n = u128::try_from(n).expect("small");
        let memory_id = Uuid::from_u128(0xC0DE_0000 + n);
        let memory = oml_app::types::MemoryRecord {
            memory_id,
            claim_key: format!("r2-clause-{n}"),
            author_key_id,
            author_principal,
            lineage_key_ids,
            lineage_principals,
            ..template.clone()
        };
        seal_onto(
            &w,
            event_types::SUBMISSION_REMEMBER,
            json!({ "artifact_id": memory_id.to_string() }),
            MetadataFold::MemoryHeld {
                memory,
                enqueue: false,
                findings: Box::default(),
            },
        )
        .await;
        let err = w.flag(&w.curator, memory_id, other).await.unwrap_err();
        assert!(matches!(err, AppError::AuthorInLineage), "{case}: {err:?}");
    }
}

/// An edge with an endpoint the scan held is not served to a sibling inside
/// an in-progress scope, while that endpoint's author still reads it
/// (ADR-0030 d3; ADR-0065 d15). The edge is made between clean rows; a
/// recording rescan then finds a hold on one of them.
#[tokio::test]
async fn an_edge_to_a_flagged_held_row_is_not_listed_to_a_sibling_in_progress() {
    let w = world().await;
    w.mark(w.proj).await;
    let a = w
        .write(&w.ann, "e1", "the canary drains first", w.proj)
        .await;
    let b = w
        .write(&w.bob, "e2", "the canary never drains", w.proj)
        .await;
    w.flag(&w.curator, a, b).await.expect("curator flags");
    assert_eq!(w.edges(&w.bob, Some(w.proj), Some(b)).await, 1, "before");
    seal_onto(
        &w,
        event_types::SCAN_FINDINGS_RECORDED,
        json!({ "scanned_under": null, "versions": [
            { "artifact_id": a.to_string(), "findings": [] },
        ] }),
        MetadataFold::FindingsRecorded {
            scanned_under: None,
            versions: vec![oml_app::ports::RecordedVersion {
                memory_id: a,
                summary: Some("test.marker: marker-hold at /body line 1".to_owned()),
                findings: Vec::new(),
            }],
        },
    )
    .await;
    assert_eq!(
        w.edges(&w.bob, Some(w.proj), Some(b)).await,
        0,
        "a sibling no longer reads the edge"
    );
    assert_eq!(
        w.edges(&w.ann, Some(w.proj), Some(b)).await,
        1,
        "the flagged endpoint's author still does"
    );
}

/// A withdrawn version is never served inside an in-progress workspace:
/// another reader there neither recalls, lists nor reads it, as a held one
/// was before the withdrawal (ADR-0083 d13).
#[tokio::test]
async fn a_withdrawn_version_is_never_served_inside_an_in_progress_workspace() {
    let w = world().await;
    w.mark(w.proj).await;
    let id = w
        .write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    assert_eq!(
        World::ids(&w.recall(&w.bob, "canary", w.proj).await),
        vec![id]
    );
    w.app
        .withdraw(
            w.ann.value.as_str(),
            &oml_app::types::WithdrawRequest {
                idempotency_key: "w1".to_owned(),
                memory_id: id,
                reason: Some("written to the wrong workspace".to_owned()),
            },
        )
        .await
        .expect("ann withdraws her held version");
    assert!(w.recall(&w.bob, "canary", w.proj).await.is_empty());
    assert!(w.listed(&w.bob, w.proj).await.is_empty());
    assert!(matches!(
        w.app.get_memory(w.bob.value.as_str(), id).await,
        Err(AppError::NotFound)
    ));
}

// -- The in-progress audience reaches the ancestors (ADR-0080 d16; #402) ----

impl World {
    /// A successor of `target` at version `expected + 1` by `key`, left held;
    /// with `retirement` it retires the claim.
    async fn successor(
        &self,
        key: &IssuedKey,
        idem: &str,
        target: Uuid,
        expected: u32,
        body: &str,
        retirement: bool,
    ) -> Uuid {
        self.app
            .supersede(
                key.value.as_str(),
                &SupersedeRequest {
                    idempotency_key: idem.to_owned(),
                    on_similar: Default::default(),
                    target_id: target,
                    body: body.to_owned(),
                    kind: None,
                    retirement,
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
                    expected_version: expected,
                },
            )
            .await
            .expect("supersede")
            .receipt
            .memory_id
    }
}

/// A read scoped at an ancestor of an in-progress root serves the root's
/// held rows, every author's, marked held, by default and in the scoped
/// listing; beside them the reader's own held rows on the ancestor's other
/// nodes, which are not inside; and `include_unreviewed: false` narrows
/// them away (ADR-0080 d16, amending ADR-0030 d2, d4).
#[tokio::test]
async fn a_read_at_an_ancestor_serves_a_descendant_in_progress_root_marked_unreviewed() {
    let w = world().await;
    w.mark(w.proj).await;
    let inside = w
        .write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    let bobs_own = w
        .write(&w.bob, "b1", "the canary waits for the pager", w.crew)
        .await;
    let anns_outside = w
        .write(&w.ann, "a2", "the canary rotates weekly", w.crew)
        .await;

    let at_crew = w.recall(&w.bob, "canary", w.crew).await;
    let mut ids = World::ids(&at_crew);
    ids.sort();
    let mut expected = vec![inside, bobs_own];
    expected.sort();
    assert_eq!(
        ids, expected,
        "the root's row and bob's own, not ann's outside one"
    );
    assert!(
        at_crew.iter().all(|h| h.1 == MemoryStatus::Held),
        "each marked unreviewed"
    );
    assert!(!ids.contains(&anns_outside));
    assert_eq!(
        w.listed(&w.bob, w.crew).await,
        vec![inside],
        "the scoped listing at the ancestor"
    );
    assert!(
        w.recall_with(&w.bob, "canary", w.crew, Some(false))
            .await
            .is_empty(),
        "false narrows the held rows away"
    );
    // A key that reads only the ancestor's subtree from the universe down.
    let wide = key(
        &w.app,
        &w.boot.admin_key.value,
        Role::Reader,
        UsedBy::Agent,
        None,
        "rhea",
        vec![grant(w.boot.root_workspace_id, Role::Reader)],
    )
    .await
    .expect("a reader at the universe");
    assert_eq!(
        World::ids(&w.recall(&wide, "canary", w.boot.root_workspace_id).await),
        vec![inside],
        "from two levels up as well"
    );
}

/// A read scoped at a sibling of an in-progress root serves nothing of it,
/// asked or not, by recall or the listing (ADR-0080 d16).
#[tokio::test]
async fn a_read_at_a_sibling_serves_nothing_of_an_in_progress_root() {
    let w = world().await;
    w.mark(w.proj).await;
    w.write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    w.promoted(&w.ann, "a2", "the canary rolls back on error", w.proj)
        .await;
    for asked in [None, Some(true)] {
        assert!(
            w.recall_with(&w.bob, "canary", w.side, asked)
                .await
                .is_empty()
        );
    }
    assert!(w.listed(&w.bob, w.side).await.is_empty());
}

/// A held retirement inside an in-progress root is never served to the
/// audience, inside or at an ancestor: the promoted tip it would retire is
/// served until a reviewer approves it (ADR-0080 d16; ADR-0030 d3).
#[tokio::test]
async fn a_held_retirement_is_never_served_to_the_in_progress_audience() {
    let w = world().await;
    w.mark(w.proj).await;
    let tip = w
        .promoted(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    let retirement = w
        .successor(
            &w.ann,
            "a1-r",
            tip,
            1,
            "the canary drains before promotion",
            true,
        )
        .await;
    for scope in [w.proj, w.crew] {
        let hits = World::ids(&w.recall_with(&w.bob, "canary", scope, Some(true)).await);
        assert_eq!(hits, vec![tip], "at {scope}");
        let listed = w.listed(&w.bob, scope).await;
        assert!(listed.contains(&tip) && !listed.contains(&retirement));
    }
}

/// A version the scan held stays its author's inside an in-progress root,
/// and the ancestor's audience does not widen that: from the parent another
/// key neither recalls nor lists it, while its author does (ADR-0080 d16;
/// ADR-0074 d20).
#[tokio::test]
async fn a_scan_held_version_stays_author_only_inside_an_in_progress_root() {
    let w = world().await;
    w.mark(w.proj).await;
    let flagged = w
        .write(
            &w.ann,
            "f1",
            "the canary drains first [needs-person]",
            w.proj,
        )
        .await;
    assert!(
        w.recall_with(&w.bob, "canary", w.crew, Some(true))
            .await
            .is_empty()
    );
    assert!(w.listed(&w.bob, w.crew).await.is_empty());
    assert_eq!(
        World::ids(&w.recall(&w.ann, "canary", w.crew).await),
        vec![flagged],
        "its author still recalls it"
    );
}

/// Federation and a listed source never read an in-progress subtree's held
/// rows: a key scoped at a sibling that names the root, or asks wide, is
/// served none of them (ADR-0080 d16; ADR-0072).
#[tokio::test]
async fn federation_never_reads_an_in_progress_subtree() {
    let w = world().await;
    w.mark(w.proj).await;
    let held = w
        .write(&w.ann, "a1", "the canary drains before promotion", w.proj)
        .await;
    for sources in [
        oml_app::federation::Sources::Wide,
        oml_app::federation::Sources::List(vec![w.proj.to_string()]),
    ] {
        let answer = w
            .app
            .recall(
                w.bob.value.as_str(),
                &RecallRequest {
                    sources: Some(sources.clone()),
                    timings: false,
                    include_expired: false,
                    now: None,
                    anchor: oml_app::types::AnchorFilter::default(),
                    query: "canary".to_owned(),
                    k: 10,
                    workspace: Some(w.side.into()),
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed: Some(true),
                    max_tokens: None,
                    search: None,
                    match_mode: oml_app::search::MatchMode::Any,
                    as_of: None,
                },
            )
            .await;
        let served: Vec<Uuid> = answer
            .map(|r| r.hits.iter().map(|h| h.served.memory.memory_id).collect())
            .unwrap_or_default();
        assert!(!served.contains(&held), "{sources:?}: {served:?}");
    }
}

/// Every held kind inside an in-progress root is listed and read, marked
/// held, by the in-progress audience, a held steering version included;
/// recall's hits stay the knowledge kinds it ranks (ADR-0080 d10, d16,
/// amending ADR-0030 d3).
#[tokio::test]
async fn every_held_kind_inside_is_listed_to_the_in_progress_audience() {
    let w = world().await;
    w.mark(w.proj).await;
    let steering = w
        .app
        .submit_memory(
            w.ann.value.as_str(),
            &SubmitRequest {
                origin: None,
                on_similar: Default::default(),
                valid_from: None,
                valid_until: None,
                anchor: None,
                idempotency_key: "s1".to_owned(),
                claim_key: Some("claim-s1".to_owned()),
                verb: None,
                kind: MemoryKind::Steering,
                body: "Prefer the canary pool for rollouts.".to_owned(),
                workspace: Some(w.proj.into()),
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                kind_fields: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("submit")
        .receipt
        .memory_id;
    for scope in [w.proj, w.crew] {
        assert_eq!(w.listed(&w.bob, scope).await, vec![steering], "at {scope}");
    }
    let got = w
        .app
        .get_memory(w.bob.value.as_str(), steering)
        .await
        .expect("read by id inside the audience");
    assert_eq!(got.memory.status, MemoryStatus::Held);
    assert!(w.listed(&w.bob, w.side).await.is_empty());
}
