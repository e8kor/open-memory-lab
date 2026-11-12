//! Who starts a memory in a user root (ADR-0044 d16, the amendment of
//! 2026-11-12): a new memory in `universe.<user>` or below it is that
//! user's keys' alone, and so is the successor of a ratified retirement
//! there, which would put the claim back in service. Any other key that
//! reads the workarea or the target is refused
//! `submission.user_root_refused`, and one that reads neither keeps
//! `access.not_found`. A successor or a retirement of a served memory
//! there is admitted as before, from its author or from a reviewer granted
//! there; reads, recall and review are what they were, and a row a tenant
//! key wrote there before the rule stays as it is. Stated over the
//! application and run identically over every store: the in-memory double
//! and PostgreSQL. Each law takes a fresh store, builds its own tenant
//! through the app's verbs and panics on violation.

use std::sync::atomic::{AtomicU64, Ordering};

use oml_app::App;
use oml_app::domain::access::{Covers, Grant};
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{EventTime, HeldBy, MemoryKind, MemoryStatus, Role};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, SeqIdGen};
use oml_app::ports::JournalStore;
use oml_app::types::{
    AnchorFilter, AppError, BootstrapRequest, GrantRequest, IssueKeyRequest, IssuedKey,
    RecallRequest, SubmitRequest, SupersedeRequest, Verdict,
};
use uuid::Uuid;

use crate::{ChainRig, TEST_SEED, held_memory, hold};

/// The app every law runs.
pub type RootsApp<S> = App<S, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

static NAMES: AtomicU64 = AtomicU64::new(0);

fn fresh(prefix: &str) -> Name {
    Name::parse(&format!(
        "{prefix}-{}",
        NAMES.fetch_add(1, Ordering::Relaxed)
    ))
    .expect("a fixture name")
}

fn grant(node: Uuid, role: Role) -> GrantRequest {
    GrantRequest {
        node: node.into(),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    }
}

/// `universe` with ada's root, `project` beneath it and the shared
/// workarea `shared`. Ada holds a writer key and a reviewer key in her
/// root. Three tenant keys: `tenant_writer` writes and reads the whole
/// tree by a subtree grant on `universe` (the key of #329),
/// `tenant_reviewer` reviews and reads it, and `stranger` writes and reads
/// `shared` alone.
pub struct RootsWorld<S: JournalStore> {
    pub app: RootsApp<S>,
    pub store: S,
    pub tenant: Uuid,
    pub admin: String,
    pub root: Uuid,
    pub project: Uuid,
    pub shared: Uuid,
    pub ada: IssuedKey,
    pub ada_reviewer: IssuedKey,
    pub tenant_writer: IssuedKey,
    pub tenant_reviewer: IssuedKey,
    pub stranger: IssuedKey,
    idem: AtomicU64,
}

impl<S: JournalStore + Clone> RootsWorld<S> {
    pub async fn new(store: S) -> Self {
        let app = App::new(
            store.clone(),
            MemSigner::new(TEST_SEED),
            FixedClock::new(),
            SeqIdGen::new(),
            MarkerScanner::default(),
        );
        let boot = app
            .bootstrap_tenant(&BootstrapRequest {
                tenant_name: fresh("roots"),
                admin_key_name: fresh("roots-admin"),
                admin_principal: "roots-admin".to_owned(),
            })
            .await
            .expect("bootstrap");
        let admin = boot.admin_key.value.clone();
        let universe = boot.root_workarea_id;
        let user = Name::parse("ada").expect("a user name");
        let ada_root = Reference::parse("ada").expect("a reference");
        let user_key = |role: Role, held_by: HeldBy, roles: &[Role]| IssueKeyRequest {
            profile: None,
            role,
            name: fresh("roots-key"),
            principal: format!("ada/{}", NAMES.fetch_add(1, Ordering::Relaxed)),
            held_by: Some(held_by),
            default_workarea: None,
            crew: None,
            grants: roles
                .iter()
                .map(|&role| GrantRequest {
                    node: ada_root.clone(),
                    role,
                    deny: Vec::new(),
                    covers: Covers::Subtree,
                })
                .collect(),
            connect_code: false,
            user: Some(user.clone()),
            guardrail_scope: None,
            expiry: None,
            requests_per_minute: None,
        };
        let ada = app
            .issue_key(
                admin.as_str(),
                &user_key(Role::Writer, HeldBy::Agent, &[Role::Writer, Role::Reader]),
            )
            .await
            .expect("ada's writer key");
        let ada_reviewer = app
            .issue_key(
                admin.as_str(),
                &user_key(Role::Dreamer, HeldBy::Human, &[Role::Dreamer, Role::Reader]),
            )
            .await
            .expect("ada's reviewer key");
        let root = ada.user.as_ref().expect("a user key names its root").id;
        let project = app
            .create_workarea(
                ada.value.as_str(),
                &Name::parse("project").expect("a name"),
                None,
                None,
            )
            .await
            .expect("ada creates a workarea in her root")
            .workarea
            .workarea_id;
        let shared = app
            .create_workarea(
                admin.as_str(),
                &fresh("shared"),
                Some(universe.into()),
                None,
            )
            .await
            .expect("a shared workarea")
            .workarea
            .workarea_id;
        let tenant_writer = tenant_key(
            &app,
            &admin,
            Role::Writer,
            vec![grant(universe, Role::Writer), grant(universe, Role::Reader)],
        )
        .await;
        let tenant_reviewer = tenant_key(
            &app,
            &admin,
            Role::Dreamer,
            vec![
                grant(universe, Role::Dreamer),
                grant(universe, Role::Reader),
            ],
        )
        .await;
        let stranger = tenant_key(
            &app,
            &admin,
            Role::Writer,
            vec![grant(shared, Role::Writer), grant(shared, Role::Reader)],
        )
        .await;
        RootsWorld {
            app,
            store,
            tenant: boot.tenant_id,
            admin,
            root,
            project,
            shared,
            ada,
            ada_reviewer,
            tenant_writer,
            tenant_reviewer,
            stranger,
            idem: AtomicU64::new(0),
        }
    }

    /// A tenant key under `role` with `grants`.
    pub async fn tenant_key(&self, role: Role, grants: Vec<GrantRequest>) -> IssuedKey {
        tenant_key(&self.app, &self.admin, role, grants).await
    }

    fn next_idem(&self) -> String {
        format!("roots-{}", self.idem.fetch_add(1, Ordering::Relaxed))
    }

    /// `key` submits a new memory on `workarea`, its `claim_key` derived.
    pub async fn submit(
        &self,
        key: &IssuedKey,
        workarea: impl Into<Reference>,
        body: &str,
    ) -> Result<Uuid, AppError> {
        self.submit_claim(key, workarea, None, body).await
    }

    /// `key` submits a new memory on `workarea` under `claim_key`.
    pub async fn submit_claim(
        &self,
        key: &IssuedKey,
        workarea: impl Into<Reference>,
        claim_key: Option<&str>,
        body: &str,
    ) -> Result<Uuid, AppError> {
        self.app
            .submit_memory(
                key.value.as_str(),
                &SubmitRequest {
                    origin: None,
                    on_similar: Default::default(),
                    valid_from: None,
                    valid_until: None,
                    anchor: None,
                    idempotency_key: self.next_idem(),
                    claim_key: claim_key.map(ToOwned::to_owned),
                    verb: None,
                    kind: MemoryKind::Memory,
                    body: body.to_owned(),
                    workarea: Some(workarea.into()),
                    derives_from: Vec::new(),
                    severity: None,
                    rule: None,
                    format: None,
                    title: None,
                },
            )
            .await
            .map(|outcome| outcome.receipt.memory_id)
    }

    /// `key` proposes a successor of `target`, whose latest version is
    /// `expected`; with `retirement`, the claim's retirement.
    pub async fn supersede(
        &self,
        key: &IssuedKey,
        target: Uuid,
        expected: u32,
        retirement: bool,
    ) -> Result<Uuid, AppError> {
        self.app
            .supersede(
                key.value.as_str(),
                &SupersedeRequest {
                    idempotency_key: self.next_idem(),
                    on_similar: Default::default(),
                    target_id: target,
                    body: if retirement {
                        "no longer true".to_owned()
                    } else {
                        "the same fact, said again".to_owned()
                    },
                    kind: None,
                    retirement,
                    derives_from: Vec::new(),
                    reason: None,
                    workarea: None,
                    refuse_vocabulary_target: false,
                    severity: None,
                    rule: None,
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
            .map(|outcome| outcome.receipt.memory_id)
    }

    /// `reviewer` approves `id`; the state the verdict left it in.
    pub async fn approve(&self, reviewer: &IssuedKey, id: Uuid) -> MemoryStatus {
        self.app
            .post_verdict(reviewer.value.as_str(), id, Verdict::Approve, None)
            .await
            .expect("approve")
            .state
    }

    /// A memory ada wrote on `workarea` and her reviewer key approved.
    pub async fn adas_promoted(&self, workarea: Uuid, body: &str) -> Uuid {
        let id = self.submit(&self.ada, workarea, body).await.expect("held");
        assert_eq!(
            self.approve(&self.ada_reviewer, id).await,
            MemoryStatus::Promoted
        );
        id
    }

    /// A key of bo, another person, under `role`, holding `roles` on ada's
    /// root: what an admin's review designation gives a person's review key
    /// there (ADR-0045 d10). Issuance refuses a user key a grant outside
    /// its root (ADR-0044 d3), so the key is issued in bo's root and the
    /// store takes the grants on ada's as the designation's `grant.issued`
    /// folds them.
    pub async fn bos_key_granted_on_adas_root(&self, role: Role, roles: &[Role]) -> IssuedKey {
        let n = NAMES.fetch_add(1, Ordering::Relaxed);
        let bo = Reference::parse("bo").expect("a reference");
        let key = self
            .app
            .issue_key(
                self.admin.as_str(),
                &IssueKeyRequest {
                    profile: None,
                    role,
                    name: fresh("roots-key"),
                    principal: format!("bo/{n}"),
                    held_by: Some(if role == Role::Dreamer {
                        HeldBy::Human
                    } else {
                        HeldBy::Agent
                    }),
                    default_workarea: None,
                    crew: None,
                    grants: vec![GrantRequest {
                        node: bo,
                        role,
                        deny: Vec::new(),
                        covers: Covers::Subtree,
                    }],
                    connect_code: false,
                    user: Some(Name::parse("bo").expect("a user name")),
                    guardrail_scope: None,
                    expiry: None,
                    requests_per_minute: None,
                },
            )
            .await
            .expect("bo's key");
        let mut rig = ChainRig::new(0);
        rig.tenant_id = self.tenant;
        let mut rig = rig.lane(330 + n);
        rig.follow(&self.store).await;
        for (i, &role) in roles.iter().enumerate() {
            rig.issue_grant(
                &self.store,
                key.key_id,
                Grant {
                    grant_id: Uuid::from_u128(0x0329_1000 + u128::from(n) * 16 + i as u128),
                    node: self.root,
                    role,
                    deny: Vec::new(),
                    covers: Covers::Subtree,
                },
            )
            .await
            .expect("the store folds the grant");
        }
        key
    }

    /// Ada's `memories` meter: the live chains her keys started (ADR-0071
    /// d2).
    pub async fn adas_memories(&self) -> u64 {
        let at = EventTime::parse("2026-10-29T12:30:00.000000Z").expect("a time");
        self.store
            .user_usage(self.tenant, self.root, &at)
            .await
            .expect("usage")
            .memories
    }

    /// The tenant chain's head seq: what a refusal that journals nothing
    /// leaves where it was.
    pub async fn head(&self) -> u64 {
        self.store
            .chain_head(self.tenant)
            .await
            .expect("head")
            .expect("the tenant has a chain")
            .seq
    }

    /// The ids `key`'s recall of `query` at `workarea` served as hits.
    pub async fn hits(&self, key: &IssuedKey, workarea: Uuid, query: &str) -> Vec<Uuid> {
        self.app
            .recall(
                key.value.as_str(),
                &RecallRequest {
                    query: query.to_owned(),
                    k: 10,
                    workarea: Some(workarea.into()),
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed: Some(false),
                    max_tokens: None,
                    search: None,
                    match_mode: oml_app::search::MatchMode::Any,
                    timings: false,
                    include_expired: false,
                    now: None,
                    anchor: AnchorFilter::default(),
                    sources: None,
                    as_of: None,
                },
            )
            .await
            .expect("recall")
            .hits
            .iter()
            .map(|h| h.served.memory.memory_id)
            .collect()
    }
}

async fn tenant_key<S: JournalStore + Clone>(
    app: &RootsApp<S>,
    admin: &str,
    role: Role,
    grants: Vec<GrantRequest>,
) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            profile: None,
            role,
            name: fresh("roots-key"),
            principal: format!("crew/{}", NAMES.fetch_add(1, Ordering::Relaxed)),
            held_by: Some(if role == Role::Dreamer {
                HeldBy::Human
            } else {
                HeldBy::Agent
            }),
            default_workarea: None,
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
    .expect("issue a tenant key")
}

fn refused(result: Result<Uuid, AppError>, what: &str) {
    match result {
        Err(AppError::UserRootRefused) => {}
        other => panic!("{what}: expected submission.user_root_refused, got {other:?}"),
    }
}

fn not_found(result: Result<Uuid, AppError>, what: &str) {
    match result {
        Err(AppError::NotFound) => {}
        other => panic!("{what}: expected access.not_found, got {other:?}"),
    }
}

/// A tenant writer key that holds a subtree grant on `universe` writes a
/// new memory in a shared workarea and is refused one in a user's root and
/// in a workarea beneath it, with a `claim_key` of its own, with none, and
/// under a `claim_key` the user's rejected write released; so is a tenant
/// key that only reads the root, in place of `access.grant_refused`, and a
/// key of another person holding writer there. Nothing is journalled and
/// the user's `memories` does not move.
pub async fn a_key_that_is_not_the_users_starts_no_memory_in_a_users_root_or_below_it<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    w.submit(&w.tenant_writer, w.shared, "a shared fact")
        .await
        .expect("a tenant key writes in a shared workarea");
    let reads_only = w
        .tenant_key(
            Role::Writer,
            vec![grant(w.root, Role::Reader), grant(w.shared, Role::Writer)],
        )
        .await;
    let bos_writer = w
        .bos_key_granted_on_adas_root(Role::Writer, &[Role::Writer, Role::Reader])
        .await;
    // A claim_key every version of which was rejected is released
    // (ADR-0025 d18): the next claim under it is a version 1 again.
    let released = w
        .submit_claim(&w.ada, w.root, Some("released"), "a first try")
        .await
        .expect("held");
    w.app
        .post_verdict(
            w.ada_reviewer.value.as_str(),
            released,
            Verdict::Reject,
            Some("not so"),
        )
        .await
        .expect("rejected");
    let (head, memories) = (w.head().await, w.adas_memories().await);
    refused(
        w.submit(&w.tenant_writer, w.root, "a fact for ada").await,
        "the root, by a subtree writer grant on universe",
    );
    refused(
        w.submit(&w.tenant_writer, w.project, "a fact for ada")
            .await,
        "a workarea beneath the root",
    );
    refused(
        w.submit(
            &w.tenant_writer,
            Reference::parse("ada.project").expect("a qualified name"),
            "a fact for ada",
        )
        .await,
        "a workarea beneath the root, by its qualified name",
    );
    refused(
        w.submit_claim(&w.tenant_writer, w.root, Some("named"), "a fact for ada")
            .await,
        "a new memory under a claim_key of the key's choosing",
    );
    refused(
        w.submit_claim(&w.tenant_writer, w.root, Some("released"), "a second try")
            .await,
        "a new memory under a claim_key ada's rejected write released",
    );
    refused(
        w.submit(&reads_only, w.project, "a fact for ada").await,
        "a key that reads the root and holds no writer grant in it",
    );
    refused(
        w.submit(&bos_writer, w.project, "a fact for ada").await,
        "another person's key holding writer in the root",
    );
    assert_eq!(w.head().await, head, "a refused write journals nothing");
    assert_eq!(w.adas_memories().await, memories);
}

/// A key that reads nothing of a user's root is answered for a write there
/// exactly as for a workarea that does not exist, by id and by qualified
/// name, and for a successor of a row there, a ratified retirement
/// included, as for a row that does not exist: the refusal confirms no
/// root.
pub async fn a_key_that_reads_nothing_of_a_users_root_is_told_not_found_as_for_no_workarea<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let row = w.adas_promoted(w.project, "the relay closes at noon").await;
    let retired = w.adas_promoted(w.project, "the pump runs at night").await;
    let retirement = w
        .supersede(&w.ada, retired, 1, true)
        .await
        .expect("ada retires her claim");
    assert_eq!(
        w.approve(&w.ada_reviewer, retirement).await,
        MemoryStatus::Promoted
    );
    let head = w.head().await;
    not_found(w.submit(&w.stranger, w.root, "x").await, "the root by id");
    not_found(
        w.submit(&w.stranger, Uuid::from_u128(0x0329_dead), "x")
            .await,
        "an id no workarea holds",
    );
    not_found(
        w.submit(
            &w.stranger,
            Reference::parse("ada.project").expect("a qualified name"),
            "x",
        )
        .await,
        "a workarea in the root by qualified name",
    );
    not_found(
        w.submit(
            &w.stranger,
            Reference::parse("nobody.project").expect("a qualified name"),
            "x",
        )
        .await,
        "a qualified name under no user",
    );
    not_found(
        w.supersede(&w.stranger, row, 1, false).await,
        "a successor of a row in the root",
    );
    not_found(
        w.supersede(&w.stranger, retirement, 2, false).await,
        "a successor of a ratified retirement in the root",
    );
    not_found(
        w.supersede(&w.stranger, Uuid::from_u128(0x0329_beef), 1, false)
            .await,
        "a successor of a row that does not exist",
    );
    assert_eq!(w.head().await, head);
}

/// The user's own keys write as before: a new memory in the root and
/// beneath it, a successor of their own row, its retirement, and a
/// successor of that retirement once it is ratified, which puts the claim
/// back in service and on the user's `memories`.
pub async fn a_users_own_keys_still_write_supersede_retire_and_put_a_retired_claim_back<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let in_root = w.adas_promoted(w.root, "the relay closes at noon").await;
    w.adas_promoted(w.project, "the pump runs at night").await;
    assert_eq!(w.adas_memories().await, 2);
    let successor = w
        .supersede(&w.ada, in_root, 1, false)
        .await
        .expect("ada supersedes her own row");
    assert_eq!(
        w.approve(&w.ada_reviewer, successor).await,
        MemoryStatus::Promoted
    );
    let retirement = w
        .supersede(&w.ada, successor, 2, true)
        .await
        .expect("ada proposes the retirement");
    assert_eq!(
        w.approve(&w.ada_reviewer, retirement).await,
        MemoryStatus::Promoted
    );
    assert_eq!(w.adas_memories().await, 1, "a retired claim is not counted");
    let back = w
        .supersede(&w.ada, retirement, 3, false)
        .await
        .expect("ada's own key proposes a successor of her ratified retirement");
    assert_eq!(
        w.approve(&w.ada_reviewer, back).await,
        MemoryStatus::Promoted
    );
    assert_eq!(w.adas_memories().await, 2, "the claim is back in service");
    // Ada's reviewer key edits by proposal in her root, as any dreamer
    // there does.
    let edit = w
        .supersede(&w.ada_reviewer, back, 4, false)
        .await
        .expect("ada's reviewer key edits by proposal");
    let held = w
        .app
        .get_memory(w.ada_reviewer.value.as_str(), edit)
        .await
        .expect("its author reads the held successor");
    assert_eq!(held.memory.status, MemoryStatus::Held);
}

/// A reviewer granted in a user's root, a tenant key or another person's,
/// proposes a successor and a retirement of a served memory there, as it
/// did before the rule, and each is held and ratified like any write. A
/// successor moves no meter and a ratified retirement takes the claim off
/// the user's `memories`. A tenant writer that reads a row there and did
/// not write it keeps `access.author_only`.
pub async fn a_reviewer_granted_in_a_users_root_proposes_a_successor_and_a_retirement_there<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let bos_reviewer = w
        .bos_key_granted_on_adas_root(Role::Dreamer, &[Role::Dreamer, Role::Reader])
        .await;
    let mut rows = Vec::new();
    for body in [
        "the relay closes at noon",
        "the pump runs at night",
        "the valve is green",
        "the gauge reads low in winter",
    ] {
        rows.push(w.adas_promoted(w.project, body).await);
    }
    assert_eq!(w.adas_memories().await, 4);
    for (reviewer, edited, retired, who) in [
        (&w.tenant_reviewer, rows[0], rows[1], "a tenant reviewer"),
        (&bos_reviewer, rows[2], rows[3], "another person's reviewer"),
    ] {
        let successor = w
            .supersede(reviewer, edited, 1, false)
            .await
            .unwrap_or_else(|e| panic!("{who} edits by proposal: {e:?}"));
        assert_eq!(
            w.approve(&w.ada_reviewer, successor).await,
            MemoryStatus::Promoted,
            "{who}'s successor is ratified like any write"
        );
        let retirement = w
            .supersede(reviewer, retired, 1, true)
            .await
            .unwrap_or_else(|e| panic!("{who} proposes a retirement: {e:?}"));
        assert_eq!(
            w.approve(&w.ada_reviewer, retirement).await,
            MemoryStatus::Promoted,
            "{who}'s retirement is ratified like any write"
        );
    }
    assert_eq!(
        w.adas_memories().await,
        2,
        "the two edited claims stay ada's and the two retired ones are not counted"
    );
    match w.supersede(&w.tenant_writer, rows[0], 2, false).await {
        Err(AppError::AuthorOnly) => {}
        other => panic!("a writer supersedes its own rows only, got {other:?}"),
    }
}

/// A successor of a ratified retirement in a user's root would put the
/// claim back in service and on the user's `memories`, so it is that
/// user's keys' alone: a tenant reviewer and another person's reviewer,
/// each granted there, are refused `submission.user_root_refused`, nothing
/// is journalled and the meter stays. The user's own key proposes it as
/// before, and a granted reviewer's verdict still ratifies it.
pub async fn a_successor_of_a_ratified_retirement_in_a_users_root_is_the_users_keys_alone<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let bos_reviewer = w
        .bos_key_granted_on_adas_root(Role::Dreamer, &[Role::Dreamer, Role::Reader])
        .await;
    let row = w.adas_promoted(w.project, "the relay closes at noon").await;
    let retirement = w
        .supersede(&w.tenant_reviewer, row, 1, true)
        .await
        .expect("a tenant reviewer proposes the retirement");
    assert_eq!(
        w.approve(&w.ada_reviewer, retirement).await,
        MemoryStatus::Promoted
    );
    assert_eq!(w.adas_memories().await, 0);
    let head = w.head().await;
    for (reviewer, who) in [
        (&w.tenant_reviewer, "a tenant reviewer"),
        (&bos_reviewer, "another person's reviewer"),
    ] {
        refused(
            w.supersede(reviewer, retirement, 2, false).await,
            &format!("{who}'s successor of the ratified retirement"),
        );
        refused(
            w.supersede(reviewer, retirement, 2, true).await,
            &format!("{who}'s retirement of the ratified retirement"),
        );
    }
    assert_eq!(w.head().await, head, "a refused write journals nothing");
    assert_eq!(w.adas_memories().await, 0);
    let back = w
        .supersede(&w.ada_reviewer, retirement, 2, false)
        .await
        .expect("ada's own key proposes the successor of the retirement");
    assert_eq!(
        w.approve(&bos_reviewer, back).await,
        MemoryStatus::Promoted,
        "a granted reviewer's verdict ratifies what ada's key proposed"
    );
    assert_eq!(w.adas_memories().await, 1);
}

/// A successor proposed while a retirement of the same memory is still
/// held never puts the claim back: both replace the served tip, the
/// retirement's ratification takes the tip, and the successor's approval
/// is then `submission.tip_conflict`, journals nothing and leaves it held.
/// A successor of the retirement itself is the only way back, and that is
/// the user's keys' alone.
pub async fn a_successor_proposed_while_a_retirement_is_held_is_never_ratified_after_it<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let bos_reviewer = w
        .bos_key_granted_on_adas_root(Role::Dreamer, &[Role::Dreamer, Role::Reader])
        .await;
    let row = w.adas_promoted(w.project, "the relay closes at noon").await;
    let retirement = w
        .supersede(&w.tenant_reviewer, row, 1, true)
        .await
        .expect("a held retirement");
    let rival = w
        .supersede(&bos_reviewer, row, 1, false)
        .await
        .expect("a successor of the served tip, proposed while the retirement is held");
    assert_eq!(
        w.approve(&w.ada_reviewer, retirement).await,
        MemoryStatus::Promoted
    );
    assert_eq!(w.adas_memories().await, 0);
    let head = w.head().await;
    match w
        .app
        .post_verdict(w.ada_reviewer.value.as_str(), rival, Verdict::Approve, None)
        .await
    {
        Err(AppError::TipConflict { .. }) => {}
        other => panic!("the rival's target lost the tip, got {other:?}"),
    }
    assert_eq!(w.head().await, head);
    assert_eq!(w.adas_memories().await, 0, "the claim stays retired");
    let held = w
        .app
        .get_memory(bos_reviewer.value.as_str(), rival)
        .await
        .expect("its author reads the rival");
    assert_eq!(held.memory.status, MemoryStatus::Held);
}

/// Reading, recall and review in a user's root are what they were for a
/// tenant key: it gets and recalls ada's promoted memory under its reader
/// grant, and a tenant reviewer's verdict promotes ada's held memory.
pub async fn a_tenant_key_reads_recalls_and_reviews_in_a_users_root_as_before<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let row = w.adas_promoted(w.project, "the relay closes at noon").await;
    let read = w
        .app
        .get_memory(w.tenant_writer.value.as_str(), row)
        .await
        .expect("a tenant key with reader on the root reads its promoted row");
    assert_eq!(read.memory.memory_id, row);
    assert_eq!(
        w.hits(&w.tenant_writer, w.root, "relay noon").await,
        vec![row]
    );
    let held = w
        .submit(&w.ada, w.project, "the pump runs at night")
        .await
        .expect("held");
    assert_eq!(
        w.approve(&w.tenant_reviewer, held).await,
        MemoryStatus::Promoted,
        "a tenant reviewer's verdict in a user's root counts as it did"
    );
    let rejected = w
        .submit(&w.ada, w.project, "the valve is green")
        .await
        .expect("held");
    let outcome = w
        .app
        .post_verdict(
            w.tenant_reviewer.value.as_str(),
            rejected,
            Verdict::Reject,
            Some("not so"),
        )
        .await
        .expect("a tenant reviewer rejects in a user's root");
    assert_eq!(outcome.state, MemoryStatus::Rejected);
}

/// A memory a tenant key wrote in a user's root before the rule is kept as
/// it is: its author still reads it, ada's reviewer key still judges it,
/// every reader of the root is served it once it is promoted, and nothing
/// demotes or removes it. Its author still proposes its successor, as a
/// writer does of its own row, and starts no new memory beside it.
pub async fn a_memory_a_tenant_key_wrote_in_a_users_root_before_the_rule_stays_served_and_its_author_still_revises_it<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    // The row as a build before the rule journalled it: held, authored by
    // the tenant key, in ada's root. The application no longer writes one,
    // so the store takes it directly.
    let earlier = Uuid::from_u128(0x0329_0001);
    let mut rig = ChainRig::new(0);
    rig.tenant_id = w.tenant;
    let mut rig = rig.lane(329);
    rig.follow(&w.store).await;
    hold(
        &mut rig,
        &w.store,
        held_memory(
            w.tenant,
            w.project,
            earlier,
            &w.tenant_writer.principal,
            w.tenant_writer.key_id,
            "the gauge reads low in winter",
        ),
    )
    .await
    .expect("the store holds the earlier row");
    let own = w
        .app
        .get_memory(w.tenant_writer.value.as_str(), earlier)
        .await
        .expect("its author reads its own held row");
    assert_eq!(own.memory.status, MemoryStatus::Held);
    let verdict = w
        .app
        .post_verdict(
            w.ada_reviewer.value.as_str(),
            earlier,
            Verdict::Approve,
            None,
        )
        .await
        .expect("review of a row a tenant key wrote there is unchanged");
    assert_eq!(verdict.state, MemoryStatus::Promoted);
    assert!(
        !verdict.same_user,
        "its author names no person, as ADR-0046 d2 marks it"
    );
    assert_eq!(
        w.hits(&w.ada, w.project, "gauge winter").await,
        vec![earlier]
    );
    let kept = w
        .app
        .get_memory(w.ada.value.as_str(), earlier)
        .await
        .expect("ada reads it");
    assert_eq!(kept.memory.status, MemoryStatus::Promoted);
    assert!(kept.memory.is_tip);
    assert_eq!(kept.memory.author_key_id, w.tenant_writer.key_id);
    let successor = w
        .supersede(&w.tenant_writer, earlier, 1, false)
        .await
        .expect("the author's successor of its own earlier row");
    assert_eq!(
        w.approve(&w.ada_reviewer, successor).await,
        MemoryStatus::Promoted
    );
    refused(
        w.submit(
            &w.tenant_writer,
            w.project,
            "the gauge reads high in summer",
        )
        .await,
        "a new memory beside it",
    );
    assert_eq!(
        w.adas_memories().await,
        0,
        "a chain a tenant key started is no account's (ADR-0071 d3)"
    );
}

/// Every law of who starts a memory in a user root.
pub async fn run_user_root_write_suite<S, F, Fut>(make: F)
where
    S: JournalStore + Clone,
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = S>,
{
    a_key_that_is_not_the_users_starts_no_memory_in_a_users_root_or_below_it(make().await).await;
    a_key_that_reads_nothing_of_a_users_root_is_told_not_found_as_for_no_workarea(make().await)
        .await;
    a_users_own_keys_still_write_supersede_retire_and_put_a_retired_claim_back(make().await).await;
    a_reviewer_granted_in_a_users_root_proposes_a_successor_and_a_retirement_there(make().await)
        .await;
    a_successor_of_a_ratified_retirement_in_a_users_root_is_the_users_keys_alone(make().await)
        .await;
    a_successor_proposed_while_a_retirement_is_held_is_never_ratified_after_it(make().await).await;
    a_tenant_key_reads_recalls_and_reviews_in_a_users_root_as_before(make().await).await;
    a_memory_a_tenant_key_wrote_in_a_users_root_before_the_rule_stays_served_and_its_author_still_revises_it(
        make().await,
    )
    .await;
}
