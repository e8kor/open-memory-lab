//! Who writes in a user root (ADR-0044 d16, the amendment of 2026-11-12):
//! a claim write into `universe.<user>` or below it is that user's keys'
//! alone. A tenant key that reads the node or the target is refused
//! `submission.user_root_refused`, and one that reads neither keeps
//! `access.not_found`; reads, recall and review there are what they were,
//! and a row a tenant key wrote there before the rule stays as it is.
//! Stated over the application and run identically over every store: the
//! in-memory double and PostgreSQL. Each law takes a fresh store, builds
//! its own tenant through the app's verbs and panics on violation.

use std::sync::atomic::{AtomicU64, Ordering};

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{HeldBy, MemoryKind, MemoryStatus, Role};
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

    /// `key` submits a new memory on `workarea`.
    pub async fn submit(
        &self,
        key: &IssuedKey,
        workarea: impl Into<Reference>,
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
                    claim_key: None,
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
/// in a workarea beneath it; so is a tenant key that only reads the root,
/// in place of `access.grant_refused`. Nothing is journalled.
pub async fn a_tenant_keys_new_memory_in_a_users_root_or_below_it_is_refused_and_journals_nothing<
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
    let head = w.head().await;
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
        w.submit(&reads_only, w.project, "a fact for ada").await,
        "a key that reads the root and holds no writer grant in it",
    );
    assert_eq!(w.head().await, head, "a refused write journals nothing");
}

/// A key that reads nothing of a user's root is answered for a write there
/// exactly as for a workarea that does not exist, by id and by qualified
/// name, and for a successor of a row there as for a row that does not
/// exist: the refusal confirms no root.
pub async fn a_key_that_reads_nothing_of_a_users_root_is_told_not_found_as_for_no_workarea<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let row = w.adas_promoted(w.project, "the relay closes at noon").await;
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
        w.supersede(&w.stranger, Uuid::from_u128(0x0329_beef), 1, false)
            .await,
        "a successor of a row that does not exist",
    );
    assert_eq!(w.head().await, head);
}

/// The user's own keys write as before: a new memory in the root and
/// beneath it, a successor of their own row and its retirement, each held
/// and each promoted by the user's reviewer key.
pub async fn a_users_own_keys_still_write_supersede_and_retire_in_their_root<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let in_root = w.adas_promoted(w.root, "the relay closes at noon").await;
    w.adas_promoted(w.project, "the pump runs at night").await;
    let successor = w
        .supersede(&w.ada, in_root, 1, false)
        .await
        .expect("ada supersedes her own row");
    assert_eq!(
        w.approve(&w.ada_reviewer, successor).await,
        MemoryStatus::Promoted
    );
    // Ada's reviewer key edits by proposal in her root, as any dreamer
    // there did.
    let retirement = w
        .supersede(&w.ada_reviewer, successor, 2, true)
        .await
        .expect("ada's reviewer key proposes the retirement");
    let held = w
        .app
        .get_memory(w.ada_reviewer.value.as_str(), retirement)
        .await
        .expect("its author reads the held retirement");
    assert_eq!(held.memory.status, MemoryStatus::Held);
    assert!(held.memory.retirement);
}

/// A tenant key's successor of a memory in a user's root, and its
/// retirement, is refused: a tenant reviewer's edit by proposal, and a
/// tenant writer that reads the row, ahead of `access.author_only`.
/// Nothing is journalled and the row stays the served tip.
pub async fn a_tenant_keys_successor_or_retirement_of_a_memory_in_a_users_root_is_refused<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = RootsWorld::new(store).await;
    let row = w.adas_promoted(w.project, "the relay closes at noon").await;
    let head = w.head().await;
    refused(
        w.supersede(&w.tenant_reviewer, row, 1, false).await,
        "a tenant reviewer's edit by proposal",
    );
    refused(
        w.supersede(&w.tenant_reviewer, row, 1, true).await,
        "a tenant reviewer's retirement proposal",
    );
    refused(
        w.supersede(&w.tenant_writer, row, 1, false).await,
        "a tenant writer that reads the row and did not write it",
    );
    assert_eq!(w.head().await, head, "a refused write journals nothing");
    let served = w
        .app
        .get_memory(w.ada.value.as_str(), row)
        .await
        .expect("ada reads her row");
    assert_eq!(served.memory.status, MemoryStatus::Promoted);
    assert!(served.memory.is_tip);
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
/// demotes or removes it. Its author is refused a successor of it, as of
/// any row there.
pub async fn a_memory_a_tenant_key_wrote_in_a_users_root_before_the_rule_stays_served_and_takes_no_successor_from_it<
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
    let head = w.head().await;
    refused(
        w.supersede(&w.tenant_writer, earlier, 1, false).await,
        "the author's successor of its own earlier row",
    );
    refused(
        w.supersede(&w.tenant_writer, earlier, 1, true).await,
        "the author's retirement of its own earlier row",
    );
    assert_eq!(w.head().await, head);
    let kept = w
        .app
        .get_memory(w.ada.value.as_str(), earlier)
        .await
        .expect("ada reads it");
    assert_eq!(kept.memory.status, MemoryStatus::Promoted);
    assert!(kept.memory.is_tip);
    assert_eq!(kept.memory.author_key_id, w.tenant_writer.key_id);
}

/// Every law of who writes in a user root.
pub async fn run_user_root_write_suite<S, F, Fut>(make: F)
where
    S: JournalStore + Clone,
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = S>,
{
    a_tenant_keys_new_memory_in_a_users_root_or_below_it_is_refused_and_journals_nothing(
        make().await,
    )
    .await;
    a_key_that_reads_nothing_of_a_users_root_is_told_not_found_as_for_no_workarea(make().await)
        .await;
    a_users_own_keys_still_write_supersede_and_retire_in_their_root(make().await).await;
    a_tenant_keys_successor_or_retirement_of_a_memory_in_a_users_root_is_refused(make().await)
        .await;
    a_tenant_key_reads_recalls_and_reviews_in_a_users_root_as_before(make().await).await;
    a_memory_a_tenant_key_wrote_in_a_users_root_before_the_rule_stays_served_and_takes_no_successor_from_it(
        make().await,
    )
    .await;
}
