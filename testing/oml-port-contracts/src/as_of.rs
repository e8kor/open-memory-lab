//! As-of reads' laws (ADR-0059 d14 to d21, the amendment of 2026-11-11):
//! get, the scoped listing and recall at a past instant take two facts from
//! it, a version's existence and its claim's tip, and read everything else
//! today, so an as-of read never serves a row the caller cannot read today,
//! nor an erased one. Stated over the application and run identically over
//! every store: the in-memory double and PostgreSQL. Each law takes a fresh
//! store, builds its own tenant through the app's verbs and panics on
//! violation.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::in_progress::WorkspaceMode;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{EventTime, MemoryKind, Role, UsedBy};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, SeqIdGen};
use oml_app::federation::Sources;
use oml_app::ports::{Clock, JournalStore};
use oml_app::search::{MatchMode, Pipeline, SearchConfig};
use oml_app::types::{
    AnchorFilter, AppError, BootstrapRequest, GrantRequest, IssueKeyRequest, IssuedKey,
    ListRequest, RecallRequest, SubmitRequest, SupersedeRequest, Verdict,
};
use uuid::Uuid;

use crate::TEST_SEED;

/// The app every law runs, its clock shared with the world so a law can
/// read an instant between two acts.
pub type AsOfApp<S> = App<S, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

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

/// `universe` with two children: `home`, which `reader` reads, and `other`,
/// which it does not. The writer and the reviewer, a person, hold
/// `universe`'s subtree.
pub struct AsOfWorld<S: JournalStore> {
    pub app: AsOfApp<S>,
    pub clock: FixedClock,
    pub tenant: Uuid,
    pub admin: String,
    pub writer: IssuedKey,
    pub reviewer: IssuedKey,
    pub reader: IssuedKey,
    pub home: Uuid,
    pub other: Uuid,
    pub searches: Vec<Pipeline>,
    idem: AtomicU64,
}

impl<S: JournalStore + Clone> AsOfWorld<S> {
    /// The tree and the keys, serving `searches`, the first the default.
    pub async fn new(store: S, searches: &[&str]) -> Self {
        let pipelines: Vec<Pipeline> = searches
            .iter()
            .map(|s| Pipeline::parse(s).expect("a pipeline name"))
            .collect();
        let clock = FixedClock::new();
        let app = App::new(
            store,
            MemSigner::new(TEST_SEED),
            clock.clone(),
            SeqIdGen::new(),
            MarkerScanner::default(),
        )
        .with_search(SearchConfig::new(&pipelines, pipelines[0]).expect("a search config"));
        let boot = app
            .bootstrap_tenant(&BootstrapRequest {
                tenant_name: fresh("asof"),
                admin_key_name: fresh("asof-admin"),
                admin_principal: "asof-admin".to_owned(),
            })
            .await
            .expect("bootstrap");
        let admin = boot.admin_key.value.clone();
        let root = boot.root_workspace_id;
        let node = |name: &'static str| {
            let app = &app;
            let admin = admin.clone();
            async move {
                app.create_workspace(admin.as_str(), &fresh(name), Some(root.into()), None)
                    .await
                    .expect("create a workspace")
                    .workspace
                    .workspace_id
            }
        };
        let home = node("home").await;
        let other = node("other").await;
        let writer = issue(
            &app,
            &admin,
            Role::Writer,
            UsedBy::Agent,
            vec![grant(root, Role::Writer), grant(root, Role::Reader)],
        )
        .await;
        let reviewer = issue(
            &app,
            &admin,
            Role::Reviewer,
            UsedBy::Person,
            vec![grant(root, Role::Reviewer), grant(root, Role::Reader)],
        )
        .await;
        let reader = issue(
            &app,
            &admin,
            Role::Reader,
            UsedBy::Agent,
            vec![grant(home, Role::Reader)],
        )
        .await;
        AsOfWorld {
            app,
            clock,
            tenant: boot.tenant_id,
            admin,
            writer,
            reviewer,
            reader,
            home,
            other,
            searches: pipelines,
            idem: AtomicU64::new(0),
        }
    }

    /// A reader key on `node`'s subtree.
    pub async fn reader_of(&self, node: Uuid) -> IssuedKey {
        issue(
            &self.app,
            &self.admin,
            Role::Reader,
            UsedBy::Agent,
            vec![grant(node, Role::Reader)],
        )
        .await
    }

    /// An instant between the acts before and after it.
    #[must_use]
    pub fn instant(&self) -> EventTime {
        self.clock.now()
    }

    /// One claim `claim_key` of `kind` on `node` by the writer, left held.
    pub async fn hold_kind(
        &self,
        node: Uuid,
        claim_key: &str,
        kind: MemoryKind,
        body: &str,
    ) -> Uuid {
        self.hold_by(&self.writer, node, claim_key, kind, body)
            .await
    }

    /// One claim `claim_key` of `kind` on `node` by `key`, left held.
    pub async fn hold_by(
        &self,
        key: &IssuedKey,
        node: Uuid,
        claim_key: &str,
        kind: MemoryKind,
        body: &str,
    ) -> Uuid {
        let n = self.idem.fetch_add(1, Ordering::Relaxed);
        self.app
            .submit_memory(
                key.value.as_str(),
                &SubmitRequest {
                    origin: None,
                    on_similar: Default::default(),
                    valid_from: None,
                    valid_until: None,
                    anchor: None,
                    idempotency_key: format!("asof-{n}"),
                    claim_key: Some(claim_key.to_owned()),
                    verb: None,
                    kind,
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

    /// The reviewer, a person, approves `id`.
    pub async fn approve(&self, id: Uuid) {
        self.app
            .post_verdict(self.reviewer.value.as_str(), id, Verdict::Approve, None)
            .await
            .expect("approve");
    }

    /// One `memory` claim on `node`, approved.
    pub async fn promote(&self, node: Uuid, claim_key: &str, body: &str) -> Uuid {
        let id = self
            .hold_kind(node, claim_key, MemoryKind::Memory, body)
            .await;
        self.approve(id).await;
        id
    }

    /// A successor of `target`, version `expected + 1`, approved; with
    /// `retirement` it retires the claim.
    pub async fn supersede(
        &self,
        target: Uuid,
        expected: u32,
        body: &str,
        retirement: bool,
    ) -> Uuid {
        self.supersede_from(target, expected, body, retirement, None)
            .await
    }

    /// [`Self::supersede`], the successor declaring `valid_from`.
    pub async fn supersede_from(
        &self,
        target: Uuid,
        expected: u32,
        body: &str,
        retirement: bool,
        valid_from: Option<&str>,
    ) -> Uuid {
        let n = self.idem.fetch_add(1, Ordering::Relaxed);
        let id = self
            .app
            .supersede(
                self.writer.value.as_str(),
                &SupersedeRequest {
                    idempotency_key: format!("asof-s-{n}"),
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
                    valid_from: valid_from.map(ToOwned::to_owned),
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
            .memory_id;
        self.approve(id).await;
        id
    }

    /// A recall of `query` at `home` by `key`, at `as_of`.
    pub async fn recall(
        &self,
        key: &IssuedKey,
        query: &str,
        search: Pipeline,
        as_of: Option<&EventTime>,
        held: bool,
    ) -> Result<oml_app::types::RecallResult, AppError> {
        self.recall_at(key, self.home, query, search, as_of, Some(held))
            .await
    }

    /// A recall of `query` at `node` by `key`, at `as_of`, sending
    /// `include_unreviewed` as given.
    pub async fn recall_at(
        &self,
        key: &IssuedKey,
        node: Uuid,
        query: &str,
        search: Pipeline,
        as_of: Option<&EventTime>,
        held: Option<bool>,
    ) -> Result<oml_app::types::RecallResult, AppError> {
        self.app
            .recall(
                key.value.as_str(),
                &RecallRequest {
                    query: query.to_owned(),
                    k: 10,
                    workspace: Some(node.into()),
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed: held,
                    max_tokens: None,
                    search: Some(search.as_str().to_owned()),
                    match_mode: MatchMode::Any,
                    timings: false,
                    include_expired: false,
                    now: None,
                    anchor: AnchorFilter::default(),
                    sources: None,
                    as_of: as_of.cloned(),
                },
            )
            .await
    }

    /// The ids a recall served as hits.
    pub async fn hits(
        &self,
        key: &IssuedKey,
        query: &str,
        search: Pipeline,
        as_of: Option<&EventTime>,
        held: bool,
    ) -> BTreeSet<Uuid> {
        self.recall(key, query, search, as_of, held)
            .await
            .expect("recall")
            .hits
            .iter()
            .map(|h| h.served.memory.memory_id)
            .collect()
    }

    /// The scoped listing at `home` of `claim_key`, at `as_of`.
    pub async fn list(
        &self,
        key: &IssuedKey,
        claim_key: Option<&str>,
        as_of: Option<&EventTime>,
        own: bool,
    ) -> Result<Vec<Uuid>, AppError> {
        self.app
            .list_memories(
                key.value.as_str(),
                &ListRequest {
                    own,
                    workspace: (!own).then(|| Reference::from(self.home)),
                    state: None,
                    kinds: Vec::new(),
                    prefix: None,
                    counts: false,
                    claim_key: claim_key.map(ToOwned::to_owned),
                    since: None,
                    until: None,
                    limit: 50,
                    cursor: None,
                    anchor: AnchorFilter::default(),
                    author: None,
                    tip: None,
                    as_of: as_of.cloned(),
                },
            )
            .await
            .map(|page| page.items.iter().map(|m| m.memory.memory_id).collect())
    }

    /// Get `id` by `key` at `as_of`.
    pub async fn get(
        &self,
        key: &IssuedKey,
        id: Uuid,
        as_of: Option<&EventTime>,
    ) -> Result<Uuid, AppError> {
        self.app
            .get_memory_as_of(key.value.as_str(), id, as_of)
            .await
            .map(|m| m.memory.memory_id)
    }

    /// Every id is one today's get serves `key`: an as-of read never widens
    /// what a key reads (ADR-0059 d15).
    pub async fn assert_gettable_today(&self, key: &IssuedKey, ids: &BTreeSet<Uuid>) {
        for id in ids {
            assert!(
                self.get(key, *id, None).await.is_ok(),
                "an as-of read served {id}, which today's get does not serve the key"
            );
        }
    }
}

/// A key issued by the tenant admin.
async fn issue<S: JournalStore + Clone>(
    app: &AsOfApp<S>,
    admin: &str,
    role: Role,
    used_by: UsedBy,
    grants: Vec<GrantRequest>,
) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            profile: None,
            role,
            name: fresh("asof-key"),
            principal: format!("asof-{}", NAMES.fetch_add(1, Ordering::Relaxed)),
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
    .expect("issue a key")
}

fn one(id: Uuid) -> BTreeSet<Uuid> {
    BTreeSet::from([id])
}

/// `an_as_of_read_serves_the_tip_the_claim_had_then` over every pipeline,
/// with `a_version_recorded_after_the_instant_does_not_exist_at_it` and
/// `every_pipeline_answers_an_as_of_recall_over_the_scope_at_the_instant`:
/// before the first promotion nothing, between the two the first version,
/// after the second the second, in recall, the listing, steering and get.
pub async fn an_as_of_read_serves_the_tip_the_claim_had_then<S: JournalStore + Clone>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store, searches).await;
    let t0 = w.instant();
    let v1 = w
        .promote(w.home, "runbook", "the deploy runbook names alpha")
        .await;
    let s1 = w
        .hold_kind(
            w.home,
            "tone",
            MemoryKind::Steering,
            "answer in plain words",
        )
        .await;
    w.approve(s1).await;
    let t1 = w.instant();
    let v2 = w
        .supersede(v1, 1, "the deploy runbook names beta", false)
        .await;
    let s2 = w
        .supersede(s1, 1, "answer in short plain words", false)
        .await;
    let t2 = w.instant();
    for &search in &w.searches {
        let at = |t| w.hits(&w.reader, "deploy runbook", search, Some(t), false);
        assert!(
            at(&t0).await.is_empty(),
            "{search}: nothing was held before t0"
        );
        assert_eq!(at(&t1).await, one(v1), "{search}: v1 was the tip at t1");
        assert_eq!(at(&t2).await, one(v2), "{search}: v2 is the tip at t2");
        assert_eq!(
            w.hits(&w.reader, "deploy runbook", search, None, false)
                .await,
            one(v2),
            "{search}: today's recall is unchanged"
        );
        w.assert_gettable_today(&w.reader, &at(&t1).await).await;
        let r = w
            .recall(&w.reader, "deploy runbook", search, Some(&t1), false)
            .await
            .expect("recall");
        assert_eq!(
            r.pipeline, search,
            "search.pipeline names the pipeline asked"
        );
        let steering: Vec<Uuid> = r.steering.iter().map(|m| m.memory.memory_id).collect();
        assert_eq!(steering, vec![s1], "{search}: the steering tip at t1");
    }
    let list = |t| w.list(&w.reader, Some("runbook"), Some(t), false);
    assert_eq!(list(&t0).await.expect("list"), Vec::<Uuid>::new());
    assert_eq!(list(&t1).await.expect("list"), vec![v1]);
    assert_eq!(list(&t2).await.expect("list"), vec![v2]);
    let steering = w
        .recall(&w.reader, "deploy", w.searches[0], Some(&t2), false)
        .await
        .expect("recall");
    let steering: Vec<Uuid> = steering
        .steering
        .iter()
        .map(|m| m.memory.memory_id)
        .collect();
    assert_eq!(steering, vec![s2]);
    // Existence: a version recorded after the instant does not exist at it.
    assert!(matches!(
        w.get(&w.reader, v2, Some(&t1)).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.get(&w.reader, v1, Some(&t0)).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(w.get(&w.reader, v1, Some(&t2)).await.expect("get"), v1);
}

/// `an_as_of_read_applies_todays_grants_and_denials`: a reader that read
/// the claim at an instant reads nothing at that instant once denied today,
/// and a key with no grant on the node never reads it at any instant.
pub async fn an_as_of_read_applies_todays_grants_and_denials<S: JournalStore + Clone>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store, searches).await;
    let v1 = w.promote(w.home, "quota", "the export quota is ten").await;
    let elsewhere = w
        .promote(w.other, "quota", "the export quota is nine")
        .await;
    let t1 = w.instant();
    let reader = w.reader_of(w.home).await;
    for &search in &w.searches {
        let hits = w
            .hits(&reader, "export quota", search, Some(&t1), false)
            .await;
        assert_eq!(hits, one(v1), "{search}: never the unreadable node's row");
    }
    assert!(matches!(
        w.get(&reader, elsewhere, Some(&t1)).await,
        Err(AppError::NotFound)
    ));
    w.app
        .deny_key(w.admin.as_str(), reader.key_id.into(), w.home.into())
        .await
        .expect("deny today");
    for &search in &w.searches {
        let answer = w
            .recall(&reader, "export quota", search, Some(&t1), false)
            .await;
        assert!(
            answer.is_err() || answer.expect("recall").hits.is_empty(),
            "{search}: a denial issued today hides the past"
        );
    }
    assert!(w.get(&reader, v1, Some(&t1)).await.is_err());
    let listed = w.list(&reader, None, Some(&t1), false).await;
    assert!(listed.is_err() || listed.expect("list").is_empty());
}

/// `an_as_of_read_never_serves_a_version_erased_today_and_no_earlier_version_stands_in`:
/// `erase` marks a version erased in the store, as the erasure fold will.
pub async fn an_as_of_read_never_serves_a_version_erased_today<S, E, Fut>(
    store: S,
    searches: &[&str],
    erase: E,
) where
    S: JournalStore + Clone,
    E: Fn(Uuid, Uuid) -> Fut,
    Fut: core::future::Future<Output = ()>,
{
    let w = AsOfWorld::new(store, searches).await;
    let v1 = w
        .promote(w.home, "vendor", "the vendor contact is ada")
        .await;
    let t1 = w.instant();
    let v2 = w
        .supersede(v1, 1, "the vendor contact is grace", false)
        .await;
    let t2 = w.instant();
    erase(w.tenant, v2).await;
    for &search in &w.searches {
        let at = |t| w.hits(&w.reader, "vendor contact", search, Some(t), false);
        assert!(
            at(&t2).await.is_empty(),
            "{search}: an erased tip is served at no instant and v1 does not stand in"
        );
        assert_eq!(at(&t1).await, one(v1), "{search}: v1 was the tip at t1");
    }
    assert!(
        w.list(&w.reader, Some("vendor"), Some(&t2), false)
            .await
            .expect("list")
            .is_empty()
    );
    erase(w.tenant, v1).await;
    for &search in &w.searches {
        assert!(
            w.hits(&w.reader, "vendor contact", search, Some(&t1), false)
                .await
                .is_empty(),
            "{search}: an erased row never reappears"
        );
    }
    assert!(w.get(&w.reader, v1, Some(&t1)).await.is_err());
    assert!(
        w.list(&w.reader, Some("vendor"), Some(&t1), false)
            .await
            .expect("list")
            .is_empty()
    );
}

/// `an_as_of_read_skips_a_demoted_version_and_serves_the_version_below_it`
/// (ADR-0059 d22, the operator's decision of 2026-11-17): a demoted
/// version is served by no read at any instant, and the instants it was
/// the tip serve the latest earlier promoted, non-demoted version; a
/// demoted version below the tip leaves the instants it was the tip with
/// none.
pub async fn an_as_of_read_skips_a_demoted_version_and_serves_the_version_below_it<
    S: JournalStore + Clone,
>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store, searches).await;
    let v1 = w
        .promote(w.home, "runbook", "the runbook lives in the wiki")
        .await;
    let t1 = w.instant();
    let v2 = w
        .supersede(v1, 1, "the runbook lives in the repository", false)
        .await;
    let t2 = w.instant();
    let v3 = w
        .supersede(v2, 2, "the runbook lives in the handbook", false)
        .await;
    let t3 = w.instant();
    // A version below the tip goes: the instants it was the tip serve v1,
    // and today's tip stays (the probe, not the short cut, answers t2).
    w.app
        .demote_version(w.admin.as_str(), v2)
        .await
        .expect("demote v2, below the tip");
    for &search in &w.searches {
        let at = |t| w.hits(&w.reader, "runbook lives", search, t, false);
        assert_eq!(
            at(Some(&t1)).await,
            one(v1),
            "{search}: v1 was the tip at t1"
        );
        assert_eq!(
            at(Some(&t2)).await,
            one(v1),
            "{search}: v1 stands in for v2 at t2"
        );
        assert_eq!(
            at(Some(&t3)).await,
            one(v3),
            "{search}: v3 was the tip at t3"
        );
        assert_eq!(at(None).await, one(v3), "{search}: v3 is today's tip");
    }
    assert_eq!(
        w.list(&w.reader, Some("runbook"), Some(&t2), false)
            .await
            .expect("list"),
        vec![v1]
    );
    assert!(w.get(&w.reader, v2, Some(&t2)).await.is_err());
    assert!(w.get(&w.reader, v2, None).await.is_err());
    // The tip goes: v1, the greatest promoted version below it that is not
    // demoted, is the tip again and stands in at every instant.
    let demotion = w
        .app
        .demote_version(w.admin.as_str(), v3)
        .await
        .expect("demote the tip");
    assert_eq!(demotion.restored.map(|r| r.artifact_id), Some(v1));
    for &search in &w.searches {
        let at = |t| w.hits(&w.reader, "runbook lives", search, t, false);
        assert_eq!(
            at(Some(&t3)).await,
            one(v1),
            "{search}: v1 stands in for v3 at t3"
        );
        assert_eq!(at(Some(&t2)).await, one(v1), "{search}: and for v2 at t2");
        assert_eq!(at(None).await, one(v1), "{search}: v1 is today's tip");
    }
    w.app
        .demote_version(w.admin.as_str(), v1)
        .await
        .expect("demote the last");
    for &search in &w.searches {
        let at = |t| w.hits(&w.reader, "runbook lives", search, t, false);
        for t in [&t1, &t2, &t3] {
            assert!(at(Some(t)).await.is_empty(), "{search}: nothing stands in");
        }
    }
    assert!(
        w.list(&w.reader, Some("runbook"), Some(&t1), false)
            .await
            .expect("list")
            .is_empty()
    );
}

/// A demotion restores the end the restored version had before the
/// supersession it undoes (ADR-0089 d16, note of 2026-11-18): `v1` declares
/// no dates, `v2` supersedes it with a `valid_from` in the past, so its
/// ratification ends `v1` there (ADR-0059 d4); demoting `v2` leaves `v1`
/// open again, served today and at an instant before the demotion.
pub async fn a_demotion_restores_the_end_the_version_had_before_its_supersession<
    S: JournalStore + Clone,
>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store.clone(), searches).await;
    let v1 = w
        .promote(w.home, "lease", "the lease renews every month")
        .await;
    let v2 = w
        .supersede_from(
            v1,
            1,
            "the lease renews every quarter",
            false,
            Some("2020-01-01"),
        )
        .await;
    for &search in &w.searches {
        assert_eq!(
            w.hits(&w.reader, "lease renews", search, None, false).await,
            one(v2),
            "{search}: v2 is today's tip and v1 ended in 2020"
        );
    }
    let t = w.instant();
    let demotion = w
        .app
        .demote_version(w.admin.as_str(), v2)
        .await
        .expect("demote the tip");
    assert_eq!(demotion.restored.map(|r| r.artifact_id), Some(v1));
    for &search in &w.searches {
        let at = |t| w.hits(&w.reader, "lease renews", search, t, false);
        assert_eq!(at(None).await, one(v1), "{search}: v1 serves today");
        assert_eq!(
            at(Some(&t)).await,
            one(v1),
            "{search}: and stands in for v2 before the demotion"
        );
    }
    assert_eq!(
        w.list(&w.reader, Some("lease"), None, false)
            .await
            .expect("list"),
        vec![v1]
    );
    assert_eq!(
        w.list(&w.reader, Some("lease"), Some(&t), false)
            .await
            .expect("list"),
        vec![v1]
    );
    let restored = store.memory(w.tenant, v1).await.expect("read").expect("v1");
    assert!(restored.valid.until.is_none(), "v1's end is open again");
}

/// A demotion of a version below the tip restores the end that version's
/// ratification gave the version it replaced (ADR-0089 d16, note of
/// 2026-11-18): `v2` supersedes the open `v1` with a `valid_from` in the
/// past, `v3` supersedes `v2`, and demoting `v2` leaves `v1` open again, so
/// an as-of read in `v2`'s tenure serves `v1` in its place.
pub async fn demoting_a_version_below_the_tip_restores_the_end_its_ratification_gave_the_version_it_replaced<
    S: JournalStore + Clone,
>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store.clone(), searches).await;
    let v1 = w
        .promote(w.home, "rota", "the rota rotates every week")
        .await;
    let v2 = w
        .supersede_from(
            v1,
            1,
            "the rota rotates every fortnight",
            false,
            Some("2020-01-01"),
        )
        .await;
    let t2 = w.instant();
    let v3 = w
        .supersede(v2, 2, "the rota rotates every month", false)
        .await;
    let demotion = w
        .app
        .demote_version(w.admin.as_str(), v2)
        .await
        .expect("demote v2, below the tip");
    assert_eq!(demotion.restored, None, "the tip does not move");
    for &search in &w.searches {
        let at = |t| w.hits(&w.reader, "rota rotates", search, t, false);
        assert_eq!(
            at(Some(&t2)).await,
            one(v1),
            "{search}: v1 stands in for v2 in v2's tenure"
        );
        assert_eq!(at(None).await, one(v3), "{search}: v3 is today's tip");
    }
    let replaced = store.memory(w.tenant, v1).await.expect("read").expect("v1");
    assert!(replaced.valid.until.is_none(), "v1's end is open again");
}

/// Demotes `id` and checks the signed `lifecycle.demote` against the stored
/// rows (#474): the version it names as reopened carries the end the event
/// names, and with none named the version the demoted one replaces keeps
/// the end it had.
async fn demote_and_check_the_event<S: JournalStore + Clone>(
    w: &AsOfWorld<S>,
    store: &S,
    id: Uuid,
) -> (Option<Uuid>, Option<Uuid>) {
    let demoted = store
        .memory(w.tenant, id)
        .await
        .expect("read")
        .expect("row");
    let replaced_before = match demoted.replaces {
        Some(r) => store.memory(w.tenant, r).await.expect("read"),
        None => None,
    };
    let demotion = w
        .app
        .demote_version(w.admin.as_str(), id)
        .await
        .expect("demote");
    let event = store
        .events_range(w.tenant, demotion.journal_seq, 1)
        .await
        .expect("range")
        .remove(0)
        .envelope;
    assert_eq!(event.event_type, "lifecycle.demote");
    let p = &event.payload;
    assert_eq!(p["artifact_id"], id.to_string());
    let restored = demotion.restored.map(|r| r.artifact_id);
    assert_eq!(
        p["restored_artifact_id"].as_str(),
        restored.map(|r| r.to_string()).as_deref(),
        "the event names the version the store made the tip"
    );
    if let Some(r) = restored {
        assert!(
            store
                .memory(w.tenant, r)
                .await
                .expect("read")
                .expect("row")
                .is_tip
        );
    }
    let reopened = p["reopened_artifact_id"]
        .as_str()
        .map(|s| Uuid::parse_str(s).expect("a uuid"));
    match reopened {
        Some(r) => {
            assert_eq!(Some(r), demoted.replaces, "it reopens what it replaces");
            let row = store.memory(w.tenant, r).await.expect("read").expect("row");
            assert_eq!(
                row.valid.until.as_ref().map(|e| e.as_str().to_owned()),
                p["restored_valid_until"].as_str().map(ToOwned::to_owned),
                "the stored end is the end the event names"
            );
            assert_eq!(
                store.valid_until_before(w.tenant, r).await.expect("read"),
                None,
                "the end before the ratification was written back"
            );
        }
        None => {
            assert!(p["restored_valid_until"].is_null());
            if let Some(before) = replaced_before {
                let after = store
                    .memory(w.tenant, before.memory_id)
                    .await
                    .expect("read")
                    .expect("row");
                assert_eq!(after.valid, before.valid, "no end was written back");
            }
        }
    }
    let row = store
        .memory(w.tenant, id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.status, oml_app::domain::MemoryStatus::Demoted);
    (restored, reopened)
}

/// `the_demote_event_names_what_the_store_wrote_whichever_of_two_versions_is_demoted_first`
/// (#474): `v2` ends the open `v1` in 2020 and `v3` ends `v2` in 2021.
/// Demoting `v2` then `v3`, `v2` reopens `v1` and `v3` reopens nothing, `v2`
/// being demoted; demoting `v3` then `v2`, each reopens the version below it
/// and restores it. Each event names what its commit wrote.
pub async fn the_demote_event_names_what_the_store_wrote_in_either_order<S, F, Fut>(
    make: F,
    searches: &[&str],
) where
    S: JournalStore + Clone,
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = S>,
{
    for v2_first in [true, false] {
        let store = make().await;
        let w = AsOfWorld::new(store.clone(), searches).await;
        let v1 = w.promote(w.home, "shift", "the shift starts at six").await;
        let v2 = w
            .supersede_from(
                v1,
                1,
                "the shift starts at seven",
                false,
                Some("2020-01-01"),
            )
            .await;
        let v3 = w
            .supersede_from(
                v2,
                2,
                "the shift starts at eight",
                false,
                Some("2021-01-01"),
            )
            .await;
        if v2_first {
            assert_eq!(
                demote_and_check_the_event(&w, &store, v2).await,
                (None, Some(v1)),
                "v2, below the tip, reopens v1"
            );
            assert_eq!(
                demote_and_check_the_event(&w, &store, v3).await,
                (Some(v1), None),
                "v3 restores v1 and reopens nothing, v2 being demoted"
            );
        } else {
            assert_eq!(
                demote_and_check_the_event(&w, &store, v3).await,
                (Some(v2), Some(v2)),
                "v3 restores and reopens v2"
            );
            assert_eq!(
                demote_and_check_the_event(&w, &store, v2).await,
                (Some(v1), Some(v1)),
                "v2 restores and reopens v1"
            );
        }
        let v1_row = store.memory(w.tenant, v1).await.expect("read").expect("v1");
        assert!(
            v1_row.is_tip && v1_row.valid.until.is_none(),
            "v1 serves, open"
        );
        for &search in &w.searches {
            assert_eq!(
                w.hits(&w.reader, "shift starts", search, None, false).await,
                one(v1),
                "{search}: v1 is today's tip"
            );
        }
    }
}

/// `a_retired_claim_serves_its_earlier_tip_before_the_retirement_and_nothing_after`.
pub async fn a_retired_claim_serves_its_earlier_tip_before_the_retirement<
    S: JournalStore + Clone,
>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store, searches).await;
    let v1 = w
        .promote(w.home, "pager", "the pager rota starts monday")
        .await;
    let t1 = w.instant();
    w.supersede(v1, 1, "the rota moved to the calendar", true)
        .await;
    let t2 = w.instant();
    for &search in &w.searches {
        let at = |t| w.hits(&w.reader, "pager rota", search, t, false);
        assert_eq!(
            at(Some(&t1)).await,
            one(v1),
            "{search}: the tip before the retirement"
        );
        assert!(at(Some(&t2)).await.is_empty(), "{search}: retired by t2");
        assert!(at(None).await.is_empty(), "{search}: retired today");
        w.assert_gettable_today(&w.reader, &at(Some(&t1)).await)
            .await;
    }
}

/// `held_rows_at_an_instant_follow_todays_rules`: the caller's own held row
/// recorded by the instant is served with `include_unreviewed`, one recorded
/// after it is not, a version held then and promoted since is absent, and
/// another key never meets a held row.
pub async fn held_rows_at_an_instant_follow_todays_rules<S: JournalStore + Clone>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store, searches).await;
    let early = w
        .hold_kind(
            w.home,
            "draft-a",
            MemoryKind::Memory,
            "a draft about the ledger",
        )
        .await;
    let since = w
        .hold_kind(
            w.home,
            "draft-b",
            MemoryKind::Memory,
            "a second ledger draft",
        )
        .await;
    let t1 = w.instant();
    w.approve(since).await;
    let late = w
        .hold_kind(
            w.home,
            "draft-c",
            MemoryKind::Memory,
            "a third ledger draft",
        )
        .await;
    for &search in &w.searches {
        let mine = w
            .hits(&w.writer, "ledger draft", search, Some(&t1), true)
            .await;
        assert!(mine.contains(&early), "{search}: own held, recorded by t1");
        assert!(!mine.contains(&late), "{search}: recorded after t1");
        assert!(
            !mine.contains(&since),
            "{search}: held at t1 and promoted since, so neither the tip then nor held now"
        );
        w.assert_gettable_today(&w.writer, &mine).await;
        let theirs = w
            .hits(&w.reader, "ledger draft", search, Some(&t1), true)
            .await;
        assert!(
            theirs.is_empty(),
            "{search}: another key's held rows: {theirs:?}"
        );
    }
}

/// Inside an in-progress scope (ADR-0030 d3) a reader meets every author's
/// held rows today; at an instant, only those recorded by it, and a row held
/// then and approved since is absent, in recall and the scoped listing.
pub async fn inside_an_in_progress_scope_held_rows_at_an_instant_follow_todays_rules<
    S: JournalStore + Clone,
>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store, searches).await;
    w.app
        .set_workspace_mode(
            w.reviewer.value.as_str(),
            w.home.into(),
            WorkspaceMode::InProgress,
        )
        .await
        .expect("mark home in progress");
    let early = w
        .hold_kind(w.home, "wip-a", MemoryKind::Memory, "a ledger draft one")
        .await;
    let since = w
        .hold_kind(w.home, "wip-b", MemoryKind::Memory, "a ledger draft two")
        .await;
    let t1 = w.instant();
    w.approve(since).await;
    let late = w
        .hold_kind(w.home, "wip-c", MemoryKind::Memory, "a ledger draft three")
        .await;
    for &search in &w.searches {
        let today = w.hits(&w.reader, "ledger draft", search, None, true).await;
        assert_eq!(
            today,
            BTreeSet::from([early, since, late]),
            "{search}: inside, every author's held rows today, and the approved one"
        );
        let then = w
            .hits(&w.reader, "ledger draft", search, Some(&t1), true)
            .await;
        assert_eq!(
            then,
            one(early),
            "{search}: at t1, the held row recorded by then alone"
        );
        w.assert_gettable_today(&w.reader, &then).await;
    }
    let listed: BTreeSet<Uuid> = w
        .list(&w.reader, None, Some(&t1), false)
        .await
        .expect("list")
        .into_iter()
        .collect();
    assert_eq!(listed, one(early), "the scoped listing at t1");
}

/// A read scoped at an ancestor of an in-progress root serves the root's
/// held rows, every author's, beside the reader's own held rows on the
/// ancestor's other nodes, and never another author's held row outside the
/// root; a read at a sibling serves nothing of the root; at an instant, the
/// rows recorded by it (ADR-0080 d16, amending ADR-0030 d2, d3, d4). The
/// store takes the two node sets apart, the own held rows' and every
/// author's, in every statement.
pub async fn a_read_at_an_ancestor_serves_a_descendant_in_progress_subtree_beside_its_own_held_rows<
    S: JournalStore + Clone,
>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store, searches).await;
    let child = |name: &'static str| {
        let w = &w;
        async move {
            w.app
                .create_workspace(w.admin.as_str(), &fresh(name), Some(w.home.into()), None)
                .await
                .expect("create a workspace")
                .workspace
                .workspace_id
        }
    };
    let root = child("wip").await;
    let sibling = child("sib").await;
    w.app
        .set_workspace_mode(
            w.reviewer.value.as_str(),
            root.into(),
            WorkspaceMode::InProgress,
        )
        .await
        .expect("mark the child in progress");
    let second = issue(
        &w.app,
        &w.admin,
        Role::Writer,
        UsedBy::Agent,
        vec![grant(w.home, Role::Writer), grant(w.home, Role::Reader)],
    )
    .await;
    let inside = w
        .hold_kind(root, "wip-a", MemoryKind::Memory, "a ledger draft inside")
        .await;
    let own = w
        .hold_by(
            &second,
            w.home,
            "draft-b",
            MemoryKind::Memory,
            "a ledger draft at home",
        )
        .await;
    let outside = w
        .hold_kind(
            w.home,
            "draft-c",
            MemoryKind::Memory,
            "a ledger draft by another",
        )
        .await;
    let approved = w.promote(root, "wip-d", "a ledger draft approved").await;
    let t1 = w.instant();
    let late = w
        .hold_kind(root, "wip-e", MemoryKind::Memory, "a ledger draft late")
        .await;
    // A held version of an instruction kind is listed as every kind is,
    // and never a recall hit (ADR-0080 d16).
    let steering = w
        .hold_kind(
            root,
            "wip-f",
            MemoryKind::Steering,
            "prefer the ledger pool",
        )
        .await;
    let at_sibling = w.reader_of(sibling).await;
    let set = |ids: &[Uuid]| ids.iter().copied().collect::<BTreeSet<Uuid>>();
    for &search in &w.searches {
        let hits = |key, node, as_of, held| {
            let w = &w;
            async move {
                w.recall_at(key, node, "ledger draft", search, as_of, held)
                    .await
                    .expect("recall")
                    .hits
                    .iter()
                    .map(|h| (h.served.memory.memory_id, h.served.memory.status))
                    .collect::<Vec<_>>()
            }
        };
        let ids = |hits: &[(Uuid, oml_app::domain::MemoryStatus)]| {
            hits.iter().map(|h| h.0).collect::<BTreeSet<Uuid>>()
        };
        let by_default = hits(&second, w.home, None, None).await;
        assert_eq!(
            ids(&by_default),
            set(&[inside, approved, late]),
            "{search}: by default at the ancestor, the root's rows alone, never the key's own at home or {outside}"
        );
        assert_eq!(
            ids(&hits(&second, w.home, None, Some(true)).await),
            set(&[inside, own, approved, late]),
            "{search}: true adds the key's own held rows elsewhere, never {outside}"
        );
        assert!(
            by_default.iter().all(|(id, status)| (*id == approved)
                == (*status == oml_app::domain::MemoryStatus::Promoted)),
            "{search}: every held row served as held"
        );
        assert_eq!(
            ids(&hits(&second, w.home, None, Some(false)).await),
            one(approved),
            "{search}: false narrows to the promoted rows"
        );
        assert_eq!(
            ids(&hits(&w.reader, w.home, None, None).await),
            set(&[inside, approved, late]),
            "{search}: a reader of the ancestor alone"
        );
        assert!(
            hits(&at_sibling, sibling, None, Some(true))
                .await
                .is_empty(),
            "{search}: a sibling reads nothing of the root"
        );
        let then = ids(&hits(&second, w.home, Some(&t1), Some(true)).await);
        assert_eq!(
            then,
            set(&[inside, own, approved]),
            "{search}: at t1, the rows recorded by then"
        );
        w.assert_gettable_today(&second, &then).await;
    }
    let listed: BTreeSet<Uuid> = w
        .list(&w.reader, None, None, false)
        .await
        .expect("list")
        .into_iter()
        .collect();
    assert_eq!(
        listed,
        set(&[inside, approved, late, steering]),
        "the scoped listing at the ancestor, every held kind"
    );
}

/// `an_as_of_after_the_leads_clock_is_refused`, and the arguments `as_of`
/// does not combine with, each refused naming its field.
pub async fn an_as_of_after_the_leads_clock_is_refused<S: JournalStore + Clone>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store, searches).await;
    let v1 = w.promote(w.home, "later", "a later memory").await;
    let future = EventTime::parse("2099-01-01T00:00:00.000000Z").expect("an instant");
    let refused = |r: Result<_, AppError>, field: &str| match r {
        Err(AppError::AsOfInvalid { field: f, .. }) => assert_eq!(f, field),
        Err(other) => panic!("expected as_of refused naming {field}, got {other:?}"),
        Ok(_) => panic!("expected as_of refused naming {field}"),
    };
    refused(
        w.recall(&w.reader, "later", w.searches[0], Some(&future), false)
            .await
            .map(|_| ()),
        "/as_of",
    );
    refused(
        w.list(&w.reader, None, Some(&future), false)
            .await
            .map(|_| ()),
        "as_of",
    );
    refused(
        w.get(&w.reader, v1, Some(&future)).await.map(|_| ()),
        "as_of",
    );
    let now = w.instant();
    refused(
        w.list(&w.writer, None, Some(&now), true).await.map(|_| ()),
        "as_of",
    );
    let wide = w
        .app
        .recall(
            w.reader.value.as_str(),
            &RecallRequest {
                query: "later".to_owned(),
                k: 10,
                workspace: Some(w.home.into()),
                kinds: Vec::new(),
                include_text: true,
                include_unreviewed: None,
                max_tokens: None,
                search: None,
                match_mode: MatchMode::Any,
                timings: false,
                include_expired: false,
                now: None,
                anchor: AnchorFilter::default(),
                sources: Some(Sources::Wide),
                as_of: Some(now),
            },
        )
        .await;
    refused(wide.map(|_| ()), "/sources");
}

/// Every law but the erasure one, which needs a store-specific act.
/// A user's key on its own root, named `user`, of `role`.
async fn user_key<S: JournalStore + Clone>(
    w: &AsOfWorld<S>,
    user: &Name,
    role: Role,
    roles: &[Role],
) -> IssuedKey {
    w.app
        .issue_key(
            w.admin.as_str(),
            &IssueKeyRequest {
                profile: None,
                role,
                name: fresh("asof-user-key"),
                principal: format!("asof-{}", NAMES.fetch_add(1, Ordering::Relaxed)),
                used_by: Some(if role == Role::Reviewer {
                    UsedBy::Person
                } else {
                    UsedBy::Agent
                }),
                default_workspace: None,
                crew: None,
                grants: roles
                    .iter()
                    .map(|r| GrantRequest {
                        node: Reference::Name(user.clone()),
                        role: *r,
                        deny: Vec::new(),
                        covers: Covers::Subtree,
                    })
                    .collect(),
                connect_code: false,
                user: Some(user.clone()),
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
            },
        )
        .await
        .expect("a user key")
}

/// `a_claim_whose_approved_versions_are_all_demoted_takes_a_new_version_continuing_its_numbers`
/// (ADR-0089 d16, O8 of 2026-11-19): demoting every approved version of a
/// claim releases its key, a new write to it continues the chain's version
/// numbers, and the usage meter counts the chain as the one live memory it
/// was before, never a second.
pub async fn a_claim_whose_approved_versions_are_all_demoted_takes_a_new_version_continuing_its_numbers<
    S: JournalStore + Clone,
>(
    store: S,
    searches: &[&str],
) {
    let w = AsOfWorld::new(store.clone(), searches).await;
    let ada = fresh("ada");
    let writer = user_key(&w, &ada, Role::Writer, &[Role::Writer, Role::Reader]).await;
    let reviewer = user_key(&w, &ada, Role::Reviewer, &[Role::Reviewer, Role::Reader]).await;
    let root = store
        .resolve_name(w.tenant, oml_app::domain::name::NamedKind::Workspace, &ada)
        .await
        .expect("resolve")
        .expect("ada's root");
    let approve = |id: Uuid| {
        let app = &w.app;
        let reviewer = reviewer.value.clone();
        async move {
            app.post_verdict(reviewer.as_str(), id, Verdict::Approve, None)
                .await
                .expect("ada's reviewer approves");
        }
    };
    let memories = || async {
        store
            .user_usage(w.tenant, root, &w.instant())
            .await
            .expect("usage")
            .memories
    };
    let v1 = w
        .hold_by(
            &writer,
            root,
            "cache",
            MemoryKind::Memory,
            "the cache holds a day",
        )
        .await;
    approve(v1).await;
    let v2 = w
        .app
        .supersede(
            writer.value.as_str(),
            &SupersedeRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                expected_version: 1,
                idempotency_key: "asof-o8-v2".to_owned(),
                target_id: v1,
                body: "the cache holds two days".to_owned(),
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
                format: None,
                title: None,
            },
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    approve(v2).await;
    assert_eq!(memories().await, 1, "one chain, one memory");
    for id in [v2, v1] {
        w.app
            .demote_version(w.admin.as_str(), id)
            .await
            .expect("demote");
    }
    assert_eq!(memories().await, 1, "a demoted chain still counts");
    let v3 = w
        .hold_by(
            &writer,
            root,
            "cache",
            MemoryKind::Memory,
            "the cache holds an hour",
        )
        .await;
    let row = store.memory(w.tenant, v3).await.expect("read").expect("v3");
    assert_eq!(
        row.version, 3,
        "the new version continues the claim's numbers"
    );
    assert_eq!(row.claim_key, "cache");
    approve(v3).await;
    let row = store.memory(w.tenant, v3).await.expect("read").expect("v3");
    assert!(row.is_tip, "the continued claim serves");
    assert_eq!(
        memories().await,
        1,
        "the meter counts the chain live again, as the one memory it was"
    );
    for id in [v1, v2] {
        let old = store
            .memory(w.tenant, id)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(
            old.status,
            oml_app::domain::MemoryStatus::Demoted,
            "R4: kept"
        );
    }
}

pub async fn run_as_of_suite<S, F, Fut>(make: F, searches: &[&str])
where
    S: JournalStore + Clone,
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = S>,
{
    an_as_of_read_serves_the_tip_the_claim_had_then(make().await, searches).await;
    an_as_of_read_applies_todays_grants_and_denials(make().await, searches).await;
    a_retired_claim_serves_its_earlier_tip_before_the_retirement(make().await, searches).await;
    held_rows_at_an_instant_follow_todays_rules(make().await, searches).await;
    inside_an_in_progress_scope_held_rows_at_an_instant_follow_todays_rules(make().await, searches)
        .await;
    a_read_at_an_ancestor_serves_a_descendant_in_progress_subtree_beside_its_own_held_rows(
        make().await,
        searches,
    )
    .await;
    an_as_of_after_the_leads_clock_is_refused(make().await, searches).await;
    an_as_of_read_skips_a_demoted_version_and_serves_the_version_below_it(make().await, searches)
        .await;
    a_demotion_restores_the_end_the_version_had_before_its_supersession(make().await, searches)
        .await;
    demoting_a_version_below_the_tip_restores_the_end_its_ratification_gave_the_version_it_replaced(
        make().await,
        searches,
    )
    .await;
    the_demote_event_names_what_the_store_wrote_in_either_order(&make, searches).await;
    a_claim_whose_approved_versions_are_all_demoted_takes_a_new_version_continuing_its_numbers(
        make().await,
        searches,
    )
    .await;
}
