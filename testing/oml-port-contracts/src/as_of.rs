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
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{EventTime, HeldBy, MemoryKind, Role};
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
        let root = boot.root_workarea_id;
        let node = |name: &'static str| {
            let app = &app;
            let admin = admin.clone();
            async move {
                app.create_workarea(admin.as_str(), &fresh(name), Some(root.into()), None)
                    .await
                    .expect("create a workarea")
                    .workarea
                    .workarea_id
            }
        };
        let home = node("home").await;
        let other = node("other").await;
        let writer = issue(
            &app,
            &admin,
            Role::Writer,
            HeldBy::Agent,
            vec![grant(root, Role::Writer), grant(root, Role::Reader)],
        )
        .await;
        let reviewer = issue(
            &app,
            &admin,
            Role::Dreamer,
            HeldBy::Human,
            vec![grant(root, Role::Dreamer), grant(root, Role::Reader)],
        )
        .await;
        let reader = issue(
            &app,
            &admin,
            Role::Reader,
            HeldBy::Agent,
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
            HeldBy::Agent,
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
        let n = self.idem.fetch_add(1, Ordering::Relaxed);
        self.app
            .submit_memory(
                self.writer.value.as_str(),
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
        self.app
            .recall(
                key.value.as_str(),
                &RecallRequest {
                    query: query.to_owned(),
                    k: 10,
                    workarea: Some(self.home.into()),
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed: Some(held),
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
                    workarea: (!own).then(|| Reference::from(self.home)),
                    state: None,
                    kind: None,
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
    held_by: HeldBy,
    grants: Vec<GrantRequest>,
) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            profile: None,
            role,
            name: fresh("asof-key"),
            principal: format!("asof-{}", NAMES.fetch_add(1, Ordering::Relaxed)),
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
                workarea: Some(w.home.into()),
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
    an_as_of_after_the_leads_clock_is_refused(make().await, searches).await;
}
