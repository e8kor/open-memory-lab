//! The laws of in-progress workareas and the curator profile (ADR-0030 r2
//! d15), over the in-memory doubles: unreviewed rows are served inside an
//! in-progress scope, labelled, and nowhere else; only a person's key sets
//! the mode; the commit needs zero held; no ranking reads a verdict; R3
//! holds everywhere else; a curator narrows inside and never widens.

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::access::Grant;
use oml_app::domain::in_progress::{CuratorRefusal, InProgressReason, KeyProfile, WorkareaMode};
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{EventDraft, HeldBy, MemoryKind, MemoryStatus, Role, event_types};
use oml_app::domain::{WorkareaStatus, next_envelope, sign};
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
                    on_similar: Default::default(),
                    valid_from: None,
                    valid_until: None,
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
                    timings: false,
                    include_expired: false,
                    now: None,
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
                    timings: false,
                    include_expired: false,
                    now: None,
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
                on_similar: Default::default(),
                valid_from: None,
                valid_until: None,
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
                workarea: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
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
            Role::Dreamer,
            HeldBy::Human,
            None,
            "quinn",
            vec![
                grant(self.crew, Role::Dreamer),
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
        workarea: Option<Uuid>,
        artifact: Option<Uuid>,
    ) -> usize {
        self.app
            .claim_edges(
                key.value.as_str(),
                ClaimEdgeQuery {
                    workarea: workarea.map(Into::into),
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
/// R3 holds in every `committed` and `validated` workarea: another author's
/// held row there is served neither by get nor by recall.
#[tokio::test]
async fn marking_is_refused_over_a_committed_node_and_r3_holds_there() {
    let w = world().await;
    let kid = workarea(&w.app, &w.boot.admin_key.value, "kid", w.proj).await;
    let quinn = w.second_person().await;
    let proposal = w
        .app
        .propose_transition(
            w.person.value.as_str(),
            Reference::Id(kid),
            WorkareaStatus::Committed,
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
        .set_workarea_mode(
            w.person.value.as_str(),
            Reference::Id(w.proj),
            WorkareaMode::InProgress,
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

/// H1, defence in depth: a mark over a committed node, forced through the
/// store, still relaxes R3 on `development` nodes only, every node from the
/// row's up to the mark included.
#[tokio::test]
async fn a_committed_node_under_a_mark_forced_through_the_store_keeps_r3() {
    let w = world().await;
    let kid = workarea(&w.app, &w.boot.admin_key.value, "kid", w.proj).await;
    let grandkid = workarea(&w.app, &w.boot.admin_key.value, "grandkid", kid).await;
    let quinn = w.second_person().await;
    let proposal = w
        .app
        .propose_transition(
            w.person.value.as_str(),
            Reference::Id(kid),
            WorkareaStatus::Committed,
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
        event_types::WORKAREA_MODE_SET,
        json!({ "workarea_id": w.proj.to_string(), "mode": "in_progress", "from": "standard" }),
        MetadataFold::WorkareaModeSet {
            workarea_id: w.proj,
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
/// subtree at a node outside it, in the brief read or the workarea read.
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
                workarea: w.side.into(),
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
        .workarea_info(w.bob.value.as_str(), Some(Reference::Id(w.side)))
        .await
        .expect("workarea read");
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
                workarea: w.side.into(),
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

/// M4: an edge read with no scope and no default workarea leaves every
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
        HeldBy::Agent,
        None,
        "nemo",
        Vec::new(),
    )
    .await
    .expect("a zero-grant key, no default workarea");
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
        HeldBy::Agent,
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
                on_similar: Default::default(),
                valid_from: None,
                valid_until: None,
                anchor: None,
                idempotency_key: "a1".to_owned(),
                claim_key: Some("claim-a1".to_owned()),
                verb: None,
                kind: MemoryKind::Memory,
                body: "the canary drains, per the summary".to_owned(),
                workarea: Some(w.proj.into()),
                derives_from: vec![own],
                severity: None,
                rule: None,
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
        event_type: event_types::WORKAREA_MODE_SET.to_owned(),
        principal: "fixture".to_owned(),
        api_key_id: None,
        at: last.envelope.at.clone(),
        payload: json!({ "workarea_id": w.proj.to_string(), "mode": "standard" }),
    };
    let envelope = next_envelope(Some(last), draft).expect("a lawful draft");
    let event = sign(envelope, &SigningKey::from_bytes(&TEST_SEED)).expect("signing");
    let refused = w
        .store
        .commit(CommitRequest {
            outbox: OutboxRow {
                event_id: event.envelope.event_id,
                topic: event_types::WORKAREA_MODE_SET.to_owned(),
                payload: json!({}),
            },
            event,
            fold: MetadataFold::WorkareaModeSet {
                workarea_id: w.proj,
                in_progress: false,
                held_free: vec![w.proj],
            },
            idempotency: None,
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
            },
        )
        .await;
        let err = w.flag(&w.curator, memory_id, other).await.unwrap_err();
        assert!(matches!(err, AppError::AuthorInLineage), "{case}: {err:?}");
    }
}
