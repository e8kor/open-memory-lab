//! Federated recall's phase 1 leakage and invariance laws (ADR-0072 d13,
//! phase 1, tests 1 to 9b), stated over the application and run
//! identically over every store: the in-memory double and PostgreSQL.
//! Each function takes a fresh store, builds its own tenant through the
//! app's verbs, and panics on violation.
//!
//! Test 9a's second half, an unreachable listed source refusing `503
//! cluster.source_unavailable` and an unreachable candidate listed in
//! `missing`, needs ADR-0073's nodes: on one database no source is ever
//! unreachable, so phase 1 proves the half it can, that a list's answer is
//! a function of the listed workareas alone.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::in_progress::{KeyProfile, WorkareaMode};
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{HeldBy, MemoryKind, Role, WorkareaStatus};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, SeqIdGen};
use oml_app::federation::{FederationPolicyChange, Sources, Via};
use oml_app::ports::JournalStore;
use oml_app::search::{MatchMode, Pipeline, SearchConfig};
use oml_app::types::{
    AnchorFilter, AppError, BootstrapRequest, GrantRequest, IssueKeyRequest, IssuedKey,
    RecallRequest, RecallResult, SubmitRequest, Verdict,
};
use uuid::Uuid;

use crate::TEST_SEED;

/// The app every law runs: the pipelines a federated recall may run, with
/// `bm25` the default.
pub type FedApp<S> = App<S, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

static NAMES: AtomicU64 = AtomicU64::new(0);

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

fn fresh(prefix: &str) -> Name {
    name(&format!(
        "{prefix}-{}",
        NAMES.fetch_add(1, Ordering::Relaxed)
    ))
}

fn grant(node: Uuid, role: Role, deny: Vec<Uuid>) -> GrantRequest {
    GrantRequest {
        node: node.into(),
        role,
        deny: deny.into_iter().map(Into::into).collect(),
        covers: Covers::Subtree,
    }
}

/// The tree every law starts from, all `committed` but `drafts`:
///
/// ```text
/// universe
///  ├── crew ── home          the scope
///  ├── legal                 a candidate
///  ├── product               a candidate
///  └── drafts (development)  never a source
/// ```
///
/// `reader` reads `crew`'s subtree, `legal`, `product` and `drafts`; the
/// writer and the reviewer, a person, hold `universe`'s subtree.
pub struct FedWorld<S: JournalStore> {
    pub app: FedApp<S>,
    pub store: S,
    pub tenant: Uuid,
    pub admin: String,
    pub writer: IssuedKey,
    pub reviewer: IssuedKey,
    pub reader: IssuedKey,
    pub nodes: BTreeMap<&'static str, Uuid>,
    parents: BTreeMap<&'static str, &'static str>,
    idem: AtomicU64,
}

impl<S: JournalStore + Clone> FedWorld<S> {
    /// The tree, the keys, and the rows each test adds to; federation on.
    pub async fn new(store: S) -> Self {
        let app = App::new(
            store.clone(),
            MemSigner::new(TEST_SEED),
            FixedClock::new(),
            SeqIdGen::new(),
            MarkerScanner::default(),
        )
        .with_search(
            SearchConfig::new(
                &[
                    Pipeline::Bm25,
                    Pipeline::Bm25Postings,
                    Pipeline::KeywordAny,
                    Pipeline::Bm25Sparse,
                    Pipeline::Bm25PostingsTime,
                ],
                Pipeline::Bm25,
            )
            .expect("a search config"),
        );
        let boot = app
            .bootstrap_tenant(&BootstrapRequest {
                tenant_name: fresh("fed"),
                admin_key_name: fresh("fed-admin"),
                admin_principal: "fed-admin".to_owned(),
            })
            .await
            .expect("bootstrap");
        let admin = boot.admin_key.value.clone();
        let root = boot.root_workarea_id;
        let issue = |role: Role, held_by: HeldBy, principal: &str, grants: Vec<GrantRequest>| {
            let req = IssueKeyRequest {
                profile: None,
                role,
                name: fresh("fed-key"),
                principal: principal.to_owned(),
                held_by: Some(held_by),
                default_workarea: None,
                crew: None,
                grants,
                connect_code: false,
                user: None,
                guardrail_scope: None,
            };
            let app = &app;
            let admin = admin.clone();
            async move { app.issue_key(admin.as_str(), &req).await.expect("issue") }
        };
        let writer = issue(
            Role::Writer,
            HeldBy::Agent,
            "fed-writer",
            vec![
                grant(root, Role::Writer, Vec::new()),
                grant(root, Role::Reader, Vec::new()),
            ],
        )
        .await;
        let reviewer = issue(
            Role::Dreamer,
            HeldBy::Human,
            "fed-reviewer",
            vec![
                grant(root, Role::Dreamer, Vec::new()),
                grant(root, Role::Reader, Vec::new()),
            ],
        )
        .await;
        let mut world = FedWorld {
            app,
            store,
            tenant: boot.tenant_id,
            admin,
            writer,
            reviewer: reviewer.clone(),
            reader: reviewer,
            nodes: BTreeMap::from([("universe", root)]),
            parents: BTreeMap::new(),
            idem: AtomicU64::new(0),
        };
        for (node, parent) in [
            ("crew", "universe"),
            ("home", "crew"),
            ("legal", "universe"),
            ("product", "universe"),
            ("drafts", "universe"),
        ] {
            world.create(node, parent).await;
        }
        world.reader = world
            .key(Role::Reader, &["crew", "legal", "product", "drafts"], &[])
            .await;
        world
            .app
            .set_federation_policy(
                world.admin.as_str(),
                FederationPolicyChange {
                    enabled: Some(true),
                    ..FederationPolicyChange::default()
                },
            )
            .await
            .expect("federation on");
        world
    }

    /// The node named `node`.
    #[must_use]
    pub fn node(&self, node: &str) -> Uuid {
        self.nodes[node]
    }

    /// A `development` node under `parent`.
    pub async fn create(&mut self, node: &'static str, parent: &'static str) -> Uuid {
        let id = self
            .app
            .create_workarea(
                self.admin.as_str(),
                &name(node),
                Some(self.node(parent).into()),
                None,
            )
            .await
            .expect("create a workarea")
            .workarea
            .workarea_id;
        self.nodes.insert(node, id);
        self.parents.insert(node, parent);
        id
    }

    /// Whether `node` lies in `top`'s subtree.
    fn under(&self, node: &str, top: &str) -> bool {
        let mut at = node;
        loop {
            if at == top {
                return true;
            }
            match self.parents.get(at) {
                Some(parent) => at = parent,
                None => return false,
            }
        }
    }

    /// An agent key with reader on each of `reads`' subtrees, each grant
    /// denying the nodes of `deny` beneath it: a per-grant deny.
    pub async fn key(&self, role: Role, reads: &[&str], deny: &[&str]) -> IssuedKey {
        self.app
            .issue_key(
                self.admin.as_str(),
                &IssueKeyRequest {
                    profile: None,
                    role,
                    name: fresh("fed-key"),
                    principal: format!("fed-{}", NAMES.fetch_add(1, Ordering::Relaxed)),
                    held_by: Some(HeldBy::Agent),
                    default_workarea: None,
                    crew: None,
                    grants: reads
                        .iter()
                        .map(|r| {
                            let beneath: Vec<Uuid> = deny
                                .iter()
                                .filter(|d| self.under(d, r))
                                .map(|d| self.node(d))
                                .collect();
                            grant(self.node(r), Role::Reader, beneath)
                        })
                        .collect(),
                    connect_code: false,
                    user: None,
                    guardrail_scope: None,
                },
            )
            .await
            .expect("issue a reader")
    }

    /// One claim on `node`, approved by the reviewer, a person.
    pub async fn promote(&self, node: &str, body: &str) -> Uuid {
        let id = self.hold(node, body).await;
        self.app
            .post_verdict(self.reviewer.value.as_str(), id, Verdict::Approve, None)
            .await
            .expect("approve");
        id
    }

    /// One claim on `node` by the writer, left held.
    pub async fn hold(&self, node: &str, body: &str) -> Uuid {
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
                    idempotency_key: format!("fed-{n}"),
                    claim_key: Some(format!("fed-claim-{n}")),
                    verb: None,
                    kind: MemoryKind::Memory,
                    body: body.to_owned(),
                    workarea: Some(self.node(node).into()),
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

    /// `node` moves to `committed`: the admin proposes, the person ratifies.
    pub async fn commit(&self, node: &str) {
        let proposal = self
            .app
            .propose_transition(
                self.admin.as_str(),
                self.node(node).into(),
                WorkareaStatus::Committed,
            )
            .await
            .expect("propose committed");
        self.app
            .ratify_transition(
                self.reviewer.value.as_str(),
                self.node(node).into(),
                proposal.transition_id,
            )
            .await
            .expect("ratify committed");
    }

    /// Sets the tenant's `max_sources`.
    pub async fn max_sources(&self, max: u32) {
        self.app
            .set_federation_policy(
                self.admin.as_str(),
                FederationPolicyChange {
                    max_sources: Some(max),
                    ..FederationPolicyChange::default()
                },
            )
            .await
            .expect("set max_sources");
    }

    /// A recall of `query` at `scope` under `sources`.
    pub async fn recall(
        &self,
        key: &IssuedKey,
        query: &str,
        scope: &str,
        sources: Option<Sources>,
        search: &str,
        k: usize,
    ) -> Result<RecallResult, AppError> {
        self.recall_held(key, query, scope, sources, search, k, false)
            .await
    }

    /// [`Self::recall`], with the key's own held rows when `held`.
    #[allow(clippy::too_many_arguments, reason = "one recall's fields")]
    pub async fn recall_held(
        &self,
        key: &IssuedKey,
        query: &str,
        scope: &str,
        sources: Option<Sources>,
        search: &str,
        k: usize,
        held: bool,
    ) -> Result<RecallResult, AppError> {
        self.app
            .recall(
                key.value.as_str(),
                &RecallRequest {
                    query: query.to_owned(),
                    k,
                    workarea: Some(self.node(scope).into()),
                    kinds: Vec::new(),
                    include_text: true,
                    include_unreviewed: Some(held),
                    max_tokens: None,
                    search: Some(search.to_owned()),
                    match_mode: MatchMode::Any,
                    timings: false,
                    include_expired: false,
                    now: None,
                    anchor: AnchorFilter::default(),
                    sources,
                },
            )
            .await
    }

    /// The seeded world every law but 9b reads: rows that hold the query's
    /// words in the scope, in each candidate, in `crew` and in `drafts`.
    pub async fn seeded(store: S) -> Self {
        let world = Self::new(store).await;
        for (node, body) in [
            ("home", "the invoice export runs nightly"),
            ("home", "retry the deploy twice before paging"),
            ("crew", "crew rota for the invoice desk"),
            ("legal", "the invoice retry policy is three attempts"),
            ("legal", "a legal hold keeps invoice archives"),
            ("product", "retry banner copy for the product page"),
            ("product", "the product roadmap names no invoice change"),
            ("drafts", "draft invoice retry policy"),
        ] {
            world.promote(node, body).await;
        }
        for node in ["home", "crew", "legal", "product"] {
            world.commit(node).await;
        }
        world
    }
}

/// Everything an answer serves and records, but its `recall_id` and its
/// timings: the hits with their scores' bits, terms and `via`, the steering,
/// `search.sources`, the spellings, the terms, the pruned
/// words, the served ids, the digest and the event's federation record.
#[must_use]
pub fn fingerprint(r: &RecallResult) -> String {
    let hits: Vec<String> = r
        .hits
        .iter()
        .map(|h| {
            format!(
                "{}:{:08x}:{:?}:{:?}:{:?}:{:?}",
                h.served.memory.memory_id,
                h.score.to_bits(),
                h.matched,
                h.via,
                h.source,
                h.served.workarea_path,
            )
        })
        .collect();
    let steering: Vec<Uuid> = r.steering.iter().map(|m| m.memory.memory_id).collect();
    format!(
        "hits={hits:?} steering={steering:?} sources={:?} did_you_mean={:?} terms={:?} pruned={:?} served={:?} digest={} event={:?} budget={:?} access={:?}",
        r.sources,
        r.did_you_mean,
        r.terms,
        r.pruned,
        r.served_ids,
        r.query_digest,
        r.federation,
        r.budget,
        r.access,
    )
}

/// The modes the leakage laws compare: `scope`, `wide` and a list.
fn modes() -> Vec<Option<Sources>> {
    vec![
        None,
        Some(Sources::Scope),
        Some(Sources::Wide),
        Some(Sources::List(vec![
            "legal".to_owned(),
            "product".to_owned(),
        ])),
    ]
}

const QUERIES: [&str; 3] = [
    "invoice retry policy",
    "invoice archives",
    "nightly deploy paging",
];

/// Each mode's and query's fingerprint for `key` at `home`, at `k` 10 and
/// with `max_sources` 8 and 1, so a cut is exercised.
async fn answers<S: JournalStore + Clone>(w: &FedWorld<S>, key: &IssuedKey) -> Vec<String> {
    let mut out = Vec::new();
    for max in [8, 1] {
        w.max_sources(max).await;
        for sources in modes() {
            for query in QUERIES {
                let answer = w
                    .recall(key, query, "home", sources.clone(), "bm25", 10)
                    .await;
                out.push(match answer {
                    Ok(r) => fingerprint(&r),
                    Err(e) => format!("refused {e:?}"),
                });
            }
        }
    }
    w.max_sources(8).await;
    out
}

/// Test 1: an unreadable W — a per-grant deny on a child of a readable
/// parent — whose rows match every query changes no byte of `sources`,
/// `considered`, `cut`, hits, scores, `did_you_mean`, the counts
/// or the event's record, against W absent: an empty `development` node no
/// key reads into, which is no source.
pub async fn an_unreadable_workarea_changes_no_byte_of_any_answer<S: JournalStore + Clone>(
    store: S,
) {
    let mut w = FedWorld::seeded(store).await;
    w.create("secret", "crew").await;
    let key = w
        .key(Role::Reader, &["crew", "legal", "product"], &["secret"])
        .await;
    let absent = answers(&w, &key).await;
    for _ in 0..3 {
        w.promote(
            "secret",
            "invoice retry policy archives nightly deploy paging",
        )
        .await;
    }
    w.commit("secret").await;
    let present = answers(&w, &key).await;
    assert_eq!(
        absent, present,
        "an unreadable committed workarea whose rows match every query changed an answer"
    );
}

/// Test 2: W readable but `development`, in progress, or committed with
/// `accept_federated` off changes no byte of an answer scoped elsewhere.
pub async fn a_readable_workarea_that_is_no_source_changes_no_byte<S: JournalStore + Clone>(
    store: S,
) {
    let mut w = FedWorld::seeded(store).await;
    let key = w
        .key(Role::Reader, &["crew", "legal", "product"], &[])
        .await;
    let absent = answers(&w, &key).await;
    let body = "invoice retry policy archives nightly deploy paging";

    w.create("draft-w", "crew").await;
    w.promote("draft-w", body).await;
    assert_eq!(
        absent,
        answers(&w, &key).await,
        "a development node moved an answer"
    );

    // An in-progress node is `development` by construction (ADR-0030 d1),
    // so this step holds whether or not the trust predicate checks the mark;
    // the predicate's own unit test holds the mark.
    w.app
        .set_workarea_mode(
            w.reviewer.value.as_str(),
            Reference::Id(w.node("draft-w")),
            WorkareaMode::InProgress,
        )
        .await
        .expect("mark in progress");
    assert_eq!(
        absent,
        answers(&w, &key).await,
        "an in-progress node moved an answer"
    );

    w.create("declining", "crew").await;
    w.promote("declining", body).await;
    w.commit("declining").await;
    w.app
        .set_workarea_federation(
            w.reviewer.value.as_str(),
            Reference::Id(w.node("declining")),
            false,
        )
        .await
        .expect("decline federated reads");
    assert_eq!(
        absent,
        answers(&w, &key).await,
        "a node declining federated reads moved an answer"
    );
}

/// Test 3: a key that reads the scope and not W never meets W's id or
/// name, in any recall mode's whole answer or in the scope's read.
pub async fn a_scope_reader_never_meets_an_unreadable_workarea<S: JournalStore + Clone>(store: S) {
    let w = FedWorld::seeded(store).await;
    let key = w.key(Role::Reader, &["home"], &[]).await;
    let legal = w.node("legal");
    for sources in [
        None,
        Some(Sources::Scope),
        Some(Sources::Wide),
        Some(Sources::List(vec!["home".to_owned()])),
    ] {
        for query in QUERIES {
            let answer = w
                .recall(&key, query, "home", sources.clone(), "bm25", 10)
                .await
                .expect("a recall at the scope it reads");
            let body = format!("{answer:?}");
            assert!(
                !body.contains(&legal.to_string()) && !body.contains("legal"),
                "an answer named an unreadable workarea: {body}"
            );
        }
    }
    let info = w
        .app
        .workarea_info(key.value.as_str(), Some(Reference::Id(w.node("home"))))
        .await
        .expect("the scope's read");
    let body = format!("{info:?}");
    assert!(!body.contains(&legal.to_string()) && !body.contains("legal"));
}

/// Test 4: an unreadable name in a list is refused exactly as an absent
/// one, at its index; a list with one unusable name is refused whole; the
/// two refusals' latencies are compared over many runs; and the shape
/// refusals come in d5's order: the list's shape, federation on, a `Cc`
/// pipeline, then the names.
pub async fn a_list_is_answered_whole_or_refused_in_d5s_order<S: JournalStore + Clone>(store: S) {
    let mut w = FedWorld::seeded(store).await;
    w.create("secret", "crew").await;
    w.promote("secret", "invoice retry policy").await;
    w.commit("secret").await;
    // Unreadable nodes a status or a policy would refuse if it were read
    // before the read check: a `development` one and a declining one.
    w.create("secret-dev", "crew").await;
    w.promote("secret-dev", "invoice retry policy").await;
    w.create("secret-declining", "crew").await;
    w.promote("secret-declining", "invoice retry policy").await;
    w.commit("secret-declining").await;
    // A declining node inside the scope, which a list may name.
    w.create("home-annex", "home").await;
    w.promote("home-annex", "invoice annex").await;
    w.commit("home-annex").await;
    for node in ["secret-declining", "home-annex", "product"] {
        w.app
            .set_workarea_federation(
                w.reviewer.value.as_str(),
                Reference::Id(w.node(node)),
                false,
            )
            .await
            .expect("decline federated reads");
    }
    let key = w
        .key(
            Role::Reader,
            &["crew", "legal", "product", "drafts"],
            &["secret", "secret-dev", "secret-declining"],
        )
        .await;
    let list = |names: &[&str]| {
        Some(Sources::List(
            names.iter().map(|n| (*n).to_owned()).collect(),
        ))
    };
    let refusal = |r: Result<RecallResult, AppError>| match r {
        Ok(answer) => panic!("answered: {}", fingerprint(&answer)),
        Err(e) => format!("{e:?}"),
    };
    let unreadable = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            list(&["legal", "secret"]),
            "bm25",
            10,
        )
        .await,
    );
    let absent = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            list(&["legal", "nowhere"]),
            "bm25",
            10,
        )
        .await,
    );
    assert_eq!(
        unreadable, absent,
        "an unreadable and an absent name differ"
    );
    assert_eq!(
        unreadable,
        format!("{:?}", AppError::SourceNotFound { index: 1 })
    );
    // The read check comes before any status or policy: an unreadable
    // development node and an unreadable declining one are the same bytes as
    // an absent name, at their index (d5, d6).
    for hidden in ["secret-dev", "secret-declining"] {
        assert_eq!(
            refusal(
                w.recall(
                    &key,
                    "invoice",
                    "home",
                    list(&["legal", hidden]),
                    "bm25",
                    10
                )
                .await,
            ),
            absent,
            "{hidden} was refused apart from an absent name"
        );
    }
    // The first unusable name decides, whatever follows it.
    assert_eq!(
        refusal(
            w.recall(
                &key,
                "invoice",
                "home",
                list(&["legal", "nowhere", "drafts"]),
                "bm25",
                10,
            )
            .await,
        ),
        absent
    );
    let first_untrusted = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            list(&["legal", "drafts", "nowhere"]),
            "bm25",
            10,
        )
        .await,
    );
    assert!(
        first_untrusted.contains("SourceRefused { index: 1, reason: Untrusted }"),
        "{first_untrusted}"
    );
    // A readable node outside the scope that declines is refused by its
    // reason; one inside the scope is searched.
    let declining = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            list(&["legal", "product"]),
            "bm25",
            10,
        )
        .await,
    );
    assert!(
        declining.contains("SourceRefused { index: 1, reason: DeclinesFederation }"),
        "{declining}"
    );
    let inside = w
        .recall(&key, "invoice", "home", list(&["home-annex"]), "bm25", 10)
        .await
        .expect("a declining node inside the scope may be listed");
    assert!(
        inside
            .hits
            .iter()
            .all(|h| h.served.memory.workarea_id == w.node("home-annex"))
            && !inside.hits.is_empty()
    );
    // Refused whole: a list with one untrusted name answers nothing.
    let untrusted = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            list(&["legal", "drafts"]),
            "bm25",
            10,
        )
        .await,
    );
    assert!(
        untrusted.contains("SourceRefused { index: 1, reason: Untrusted }"),
        "{untrusted}"
    );
    // The inherited channel, measured: an unreadable name's refusal and an
    // absent one's take the same time within noise.
    // The two are interleaved, after a warm-up of each, so neither pays
    // the other's cold caches.
    let (mut unreadable_t, mut absent_t) = (Vec::new(), Vec::new());
    for i in 0..44 {
        for (names, into) in [
            (["legal", "secret"], &mut unreadable_t),
            (["legal", "nowhere"], &mut absent_t),
        ] {
            let started = Instant::now();
            let _ = w
                .recall(&key, "invoice", "home", list(&names), "bm25", 10)
                .await;
            if i >= 4 {
                into.push(started.elapsed().as_secs_f64());
            }
        }
    }
    let (a, b) = (quantile(unreadable_t, 0.5), quantile(absent_t, 0.5));
    assert!(
        a <= b * 3.0 + 0.002 && b <= a * 3.0 + 0.002,
        "an unreadable name's refusal took {a}s against an absent one's {b}s"
    );
    // d5's order.
    let empty = refusal(
        w.recall(&key, "invoice", "home", list(&[]), "bm25", 10)
            .await,
    );
    assert!(
        empty.contains("/sources") && empty.contains("empty"),
        "{empty}"
    );
    let twice = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            list(&["legal", "legal"]),
            "bm25",
            10,
        )
        .await,
    );
    assert!(
        twice.contains("/sources") && twice.contains("twice"),
        "{twice}"
    );
    w.max_sources(1).await;
    let over = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            list(&["legal", "product"]),
            "bm25",
            10,
        )
        .await,
    );
    assert!(
        over.contains("/sources") && over.contains("max_sources"),
        "{over}"
    );
    w.max_sources(8).await;
    // A Cc pipeline is refused at /search before any name is looked at.
    let cc = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            list(&["secret"]),
            "bm25-sparse",
            10,
        )
        .await,
    );
    assert!(cc.contains("SourcesUnderCc"), "{cc}");
    let cc_wide = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            Some(Sources::Wide),
            "bm25-sparse",
            10,
        )
        .await,
    );
    assert!(cc_wide.contains("SourcesUnderCc"), "{cc_wide}");
    // Federation off is refused at /sources before the pipeline is.
    w.app
        .set_federation_policy(
            w.admin.as_str(),
            FederationPolicyChange {
                enabled: Some(false),
                ..FederationPolicyChange::default()
            },
        )
        .await
        .expect("federation off");
    let off = refusal(
        w.recall(
            &key,
            "invoice",
            "home",
            Some(Sources::Wide),
            "bm25-sparse",
            10,
        )
        .await,
    );
    assert!(
        off.contains("/sources") && off.contains("federation is off"),
        "{off}"
    );
    // And the shape before federation.
    let off_empty = refusal(
        w.recall(&key, "invoice", "home", list(&[]), "bm25", 10)
            .await,
    );
    assert!(off_empty.contains("empty"), "{off_empty}");
}

/// Test 5: reader on a source revoked, then a recall at once: the source is
/// absent, so no candidate set was cached.
pub async fn a_revoked_source_is_absent_from_the_next_recall<S: JournalStore + Clone>(store: S) {
    let w = FedWorld::seeded(store).await;
    let key = w
        .key(Role::Reader, &["crew", "legal", "product"], &[])
        .await;
    let legal = w.node("legal");
    let named = |r: &RecallResult| {
        r.sources
            .as_ref()
            .is_some_and(|s| s.searched.iter().any(|x| x.workarea_id == legal))
            || r.hits.iter().any(|h| h.served.memory.workarea_id == legal)
    };
    let before = w
        .recall(
            &key,
            "invoice retry policy",
            "home",
            Some(Sources::Wide),
            "bm25",
            10,
        )
        .await
        .expect("wide");
    assert!(named(&before), "legal is searched while the key reads it");
    w.app
        .deny_key(
            w.admin.as_str(),
            Reference::Id(key.key_id),
            Reference::Id(legal),
        )
        .await
        .expect("deny legal");
    let after = w
        .recall(
            &key,
            "invoice retry policy",
            "home",
            Some(Sources::Wide),
            "bm25",
            10,
        )
        .await
        .expect("wide");
    assert!(!named(&after), "a revoked source was searched");
    assert!(!format!("{after:?}").contains(&legal.to_string()));
    let refused = w
        .recall(
            &key,
            "invoice",
            "home",
            Some(Sources::List(vec!["legal".to_owned()])),
            "bm25",
            10,
        )
        .await
        .expect_err("a revoked listed source");
    assert_eq!(
        format!("{refused:?}"),
        format!("{:?}", AppError::SourceNotFound { index: 0 })
    );
}

/// Test 6: a scope hit's score is the same whether a source is selected or
/// not, and the answer for any searched set is the union's restricted to
/// it, on `search` (`bm25` and `bm25-postings`; the PostgreSQL suite runs
/// the latter with limits under which a selection's postings cross the
/// budget).
pub async fn a_hits_score_never_depends_on_which_sources_were_selected<S: JournalStore + Clone>(
    store: S,
    search: &str,
) {
    a_hits_score_never_depends_on_selection_at(store, search, 100).await;
}

/// Test 6 at `k`: a small `k` makes a pruning read's rescore depth bind.
pub async fn a_hits_score_never_depends_on_selection_at<S: JournalStore + Clone>(
    store: S,
    search: &str,
    k: usize,
) {
    let w = FedWorld::seeded(store).await;
    for i in 0..6 {
        w.promote("legal", &format!("invoice retry policy note {i}"))
            .await;
    }
    let key = w
        .key(Role::Reader, &["crew", "legal", "product"], &[])
        .await;
    for query in QUERIES {
        w.max_sources(8).await;
        // The union's ranking, read deep, so restricting it and then cutting
        // at k is the law's right side (d7.2).
        let union = w
            .recall(&key, query, "home", Some(Sources::Wide), search, 100)
            .await
            .expect("the union");
        let union_report = union.sources.clone().expect("a wide report");
        assert_eq!(union_report.cut.count, 0, "below the cap nothing is cut");
        for max in [0, 1, 2] {
            w.max_sources(max).await;
            let capped = w
                .recall(&key, query, "home", Some(Sources::Wide), search, k)
                .await
                .expect("a capped wide");
            let report = capped.sources.clone().expect("a wide report");
            let searched: Vec<Uuid> = report.searched.iter().map(|s| s.workarea_id).collect();
            let restricted: Vec<(Uuid, u32)> = union
                .hits
                .iter()
                .filter(|h| searched.contains(&h.served.memory.workarea_id))
                .map(|h| (h.served.memory.memory_id, h.score.to_bits()))
                .take(k)
                .collect();
            let got: Vec<(Uuid, u32)> = capped
                .hits
                .iter()
                .map(|h| (h.served.memory.memory_id, h.score.to_bits()))
                .collect();
            assert_eq!(
                got, restricted,
                "{search} at max_sources {max} for {query:?}: the answer is not the union's restricted to its searched set"
            );
            assert_eq!(
                capped.pruned, union.pruned,
                "the pruned words moved with the selection"
            );
            assert_eq!(
                capped
                    .federation
                    .as_ref()
                    .map(|f| &f.reference_workarea_ids),
                union.federation.as_ref().map(|f| &f.reference_workarea_ids),
                "the reference set moved with the selection"
            );
        }
    }
    w.max_sources(8).await;
}

/// Test 7's world: a key reading `crew` and `legal`, and two committed
/// workareas it does not read, each holding one matching row the caller
/// copies.
pub struct TimingWorld<S: JournalStore> {
    pub world: FedWorld<S>,
    pub key: IssuedKey,
    /// The template rows of the two unreadable workareas, each to be copied
    /// into its own by the store's bulk path: `vault`, outside every grant
    /// root, and `vault-denied`, a denied child of the readable `crew`,
    /// which the candidate loop walks past.
    pub templates: [Uuid; 2],
    pub w: [Uuid; 2],
}

/// Test 7, the world.
pub async fn timing_world<S: JournalStore + Clone>(store: S) -> TimingWorld<S> {
    let mut world = FedWorld::seeded(store).await;
    let mut templates = [Uuid::nil(); 2];
    for (i, (node, parent)) in [("vault", "universe"), ("vault-denied", "crew")]
        .into_iter()
        .enumerate()
    {
        world.create(node, parent).await;
        templates[i] = world
            .promote(node, "invoice retry policy archives nightly deploy paging")
            .await;
        world.commit(node).await;
    }
    let key = world
        .key(Role::Reader, &["crew", "legal"], &["vault-denied"])
        .await;
    let w = [world.node("vault"), world.node("vault-denied")];
    TimingWorld {
        world,
        key,
        templates,
        w,
    }
}

/// Test 7, one sample: wall times of `runs` `wide` recalls under `search`,
/// after one warm-up.
pub async fn timing_sample<S: JournalStore + Clone>(
    t: &TimingWorld<S>,
    search: &str,
    runs: usize,
) -> Vec<f64> {
    let mut out = Vec::with_capacity(runs);
    for i in 0..=runs {
        let started = Instant::now();
        let answer = t
            .world
            .recall(
                &t.key,
                QUERIES[i % QUERIES.len()],
                "home",
                Some(Sources::Wide),
                search,
                10,
            )
            .await
            .expect("wide");
        let elapsed = started.elapsed().as_secs_f64();
        assert!(
            answer
                .hits
                .iter()
                .all(|h| !t.w.contains(&h.served.memory.workarea_id))
        );
        if i > 0 {
            out.push(elapsed);
        }
    }
    out
}

/// Test 7, the comparison: W with no copies against W with many, p50 and
/// p95 within noise — p50 within a factor of 2 and 3 ms, p95, which the
/// suite's other tests running beside it move most, within a factor of 3
/// and 10 ms. A read that scanned W's rows grows with them past both.
pub fn assert_within_noise(without: Vec<f64>, with: Vec<f64>) {
    for (q, factor, slack) in [(0.5, 2.0, 0.003), (0.95, 3.0, 0.010)] {
        let (a, b) = (quantile(without.clone(), q), quantile(with.clone(), q));
        assert!(
            b <= a * factor + slack && a <= b * factor + slack,
            "p{} moved from {a}s to {b}s with the unreadable rows",
            (q * 100.0) as u32
        );
    }
}

fn quantile(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    let i = ((v.len() as f64) * q).ceil() as usize;
    v[i.saturating_sub(1).min(v.len() - 1)]
}

/// Test 8: with federation off, `scope` is today's answer byte for byte —
/// absent and explicit alike, no report, no hint, no record — and `wide`
/// and a list are refused at `/sources`; turned on, a `scope` answer
/// is the same answer byte for byte.
pub async fn with_federation_off_scope_is_todays_answer_and_wider_is_refused<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let w = FedWorld::seeded(store).await;
    let key = w
        .key(Role::Reader, &["crew", "legal", "product"], &[])
        .await;
    let on: Vec<RecallResult> = {
        let mut v = Vec::new();
        for query in QUERIES {
            v.push(
                w.recall(&key, query, "home", None, "bm25", 10)
                    .await
                    .expect("scope"),
            );
        }
        v
    };
    w.app
        .set_federation_policy(
            w.admin.as_str(),
            FederationPolicyChange {
                enabled: Some(false),
                ..FederationPolicyChange::default()
            },
        )
        .await
        .expect("federation off");
    for (i, query) in QUERIES.iter().enumerate() {
        let absent = w
            .recall(&key, query, "home", None, "bm25", 10)
            .await
            .expect("scope");
        let explicit = w
            .recall(&key, query, "home", Some(Sources::Scope), "bm25", 10)
            .await
            .expect("scope");
        assert_eq!(fingerprint(&absent), fingerprint(&explicit));
        assert!(absent.sources.is_none() && absent.federation.is_none());
        assert!(absent.hits.iter().all(|h| h.source.is_none()));
        // On, the same answer byte for byte: `scope` does no federation
        // work (d12, as amended).
        assert_eq!(fingerprint(&absent), fingerprint(&on[i]), "{query}");
        for sources in [Sources::Wide, Sources::List(vec!["legal".to_owned()])] {
            let refused = w
                .recall(&key, query, "home", Some(sources), "bm25", 10)
                .await
                .expect_err("refused with federation off");
            let text = format!("{refused:?}");
            assert!(
                text.contains("/sources") && text.contains("federation is off"),
                "{text}"
            );
        }
    }
}

/// Test 9a: a list's scores and answer are byte-identical whatever the
/// unlisted workareas hold.
pub async fn a_lists_answer_depends_on_the_listed_workareas_alone<S: JournalStore + Clone>(
    store: S,
) {
    let mut w = FedWorld::seeded(store).await;
    let key = w
        .key(Role::Reader, &["crew", "legal", "product"], &[])
        .await;
    let listed = || {
        Some(Sources::List(vec![
            "legal".to_owned(),
            "product".to_owned(),
        ]))
    };
    let mut before = Vec::new();
    for query in QUERIES {
        before.push(fingerprint(
            &w.recall(&key, query, "home", listed(), "bm25", 10)
                .await
                .expect("a list"),
        ));
    }
    for node in ["home", "crew"] {
        for _ in 0..3 {
            w.promote(node, "invoice retry policy archives nightly deploy paging")
                .await;
        }
    }
    w.create("annex", "universe").await;
    w.promote("annex", "invoice retry policy archives").await;
    w.commit("annex").await;
    let key_after = &key;
    for (i, query) in QUERIES.iter().enumerate() {
        let after = fingerprint(
            &w.recall(key_after, query, "home", listed(), "bm25", 10)
                .await
                .expect("a list"),
        );
        assert_eq!(
            before[i], after,
            "an unlisted workarea moved a list's answer for {query:?}"
        );
    }
}

/// A `wide` answer marks each hit's source: `scope` for the scope's
/// workareas, `federated` for every other (d8).
pub async fn a_wide_answer_marks_each_hits_source<S: JournalStore + Clone>(store: S) {
    let w = FedWorld::seeded(store).await;
    let key = w
        .key(Role::Reader, &["crew", "legal", "product"], &[])
        .await;
    let wide = w
        .recall(
            &key,
            "invoice retry policy",
            "home",
            Some(Sources::Wide),
            "bm25",
            10,
        )
        .await
        .expect("wide");
    assert!(wide.hits.iter().any(|h| h.source == Some(Via::Federated)));
    for h in &wide.hits {
        let expected = if h.served.memory.workarea_id == w.node("home") {
            Via::Scope
        } else {
            Via::Federated
        };
        assert_eq!(h.source, Some(expected));
    }
}

/// Held rows are never federated: under `wide` and a list, with
/// `include_unreviewed`, the writer's own held row on a candidate is never a
/// hit, while its held row in the scope is (ADR-0072 d7).
pub async fn a_held_row_is_never_federated<S: JournalStore + Clone>(store: S) {
    let w = FedWorld::seeded(store).await;
    let elsewhere = w.hold("legal", "invoice retry policy held elsewhere").await;
    let here = w.hold("home", "invoice retry policy held here").await;
    for sources in [
        Some(Sources::Wide),
        Some(Sources::List(vec!["legal".to_owned(), "home".to_owned()])),
    ] {
        for search in ["bm25", "bm25-postings"] {
            let answer = w
                .recall_held(
                    &w.writer,
                    "invoice retry policy",
                    "home",
                    sources.clone(),
                    search,
                    100,
                    true,
                )
                .await
                .expect("a recall with the writer's held rows");
            let ids: Vec<Uuid> = answer
                .hits
                .iter()
                .map(|h| h.served.memory.memory_id)
                .collect();
            assert!(
                !ids.contains(&elsewhere),
                "{search} {sources:?} served a held row of a federated source"
            );
            assert!(
                ids.contains(&here),
                "{search} {sources:?} lost the writer's held row in the scope"
            );
        }
    }
}

/// A held row's score is the same on the scan and on the postings under a
/// reference set, a word no row of the set holds scoring nothing on either
/// (ADR-0072 d7; ADR-0052 d4).
pub async fn a_held_row_scores_alike_on_the_scan_and_the_postings<S: JournalStore + Clone>(
    store: S,
) {
    let w = FedWorld::seeded(store).await;
    let held = w.hold("home", "invoice zebra stripes").await;
    for sources in [
        Some(Sources::Wide),
        Some(Sources::List(vec!["home".to_owned(), "legal".to_owned()])),
    ] {
        let score = |search: &'static str| {
            let w = &w;
            let sources = sources.clone();
            async move {
                w.recall_held(
                    &w.writer,
                    "invoice zebra",
                    "home",
                    sources,
                    search,
                    100,
                    true,
                )
                .await
                .expect("a recall with the writer's held rows")
                .hits
                .iter()
                .find(|h| h.served.memory.memory_id == held)
                .map(|h| h.score)
                .expect("the held row is a hit")
            }
        };
        let (scan, postings) = (score("bm25").await, score("bm25-postings").await);
        assert!(
            (scan - postings).abs() <= 1e-4 * scan.abs().max(1.0),
            "{sources:?}: the scan scored the held row {scan}, the postings {postings}"
        );
    }
}

/// The policies are a person's or an admin's act: an agent's or a
/// curator's key never sets `accept_federated`, no key but an admin sets the
/// tenant's policy, each change is journalled with what it replaced, a
/// change that changes nothing journals nothing, and `max_sources` past 64
/// is refused at its field (ADR-0072 d5).
pub async fn the_policies_are_never_an_agents_act<S: JournalStore + Clone>(store: S) {
    let w = FedWorld::seeded(store).await;
    let root = w.node("universe");
    let dreamer = |held_by: HeldBy, profile: Option<KeyProfile>| {
        let w = &w;
        async move {
            w.app
                .issue_key(
                    w.admin.as_str(),
                    &IssueKeyRequest {
                        profile,
                        role: Role::Dreamer,
                        name: fresh("fed-dreamer"),
                        principal: format!("fed-{}", NAMES.fetch_add(1, Ordering::Relaxed)),
                        held_by: Some(held_by),
                        default_workarea: None,
                        crew: None,
                        grants: vec![
                            grant(root, Role::Dreamer, Vec::new()),
                            grant(root, Role::Reader, Vec::new()),
                        ],
                        connect_code: false,
                        user: None,
                        guardrail_scope: None,
                    },
                )
                .await
                .expect("issue a dreamer")
        }
    };
    for (key, what) in [
        (dreamer(HeldBy::Agent, None).await, "an agent's dreamer key"),
        (
            dreamer(HeldBy::Agent, Some(KeyProfile::Curator)).await,
            "a curator key",
        ),
        (w.writer.clone(), "a writer key"),
        (w.reader.clone(), "a reader key"),
    ] {
        let refused = w
            .app
            .set_workarea_federation(key.value.as_str(), Reference::Id(w.node("legal")), false)
            .await
            .expect_err(what);
        assert!(
            matches!(refused, AppError::FederationHumanOnly { .. }),
            "{what}: {refused:?}"
        );
        let refused = w
            .app
            .set_federation_policy(
                key.value.as_str(),
                FederationPolicyChange {
                    enabled: Some(false),
                    ..FederationPolicyChange::default()
                },
            )
            .await
            .expect_err(what);
        assert!(
            matches!(refused, AppError::VerbCeiling { .. }),
            "{what}: {refused:?}"
        );
    }
    // A person's reviewer key and an admin may; each change is journalled
    // with what it replaced, and a repeat journals nothing.
    let events = |kind: &'static str| {
        let w = &w;
        async move {
            w.store
                .events_range(w.tenant, 0, 100_000)
                .await
                .expect("the chain")
                .into_iter()
                .filter(|e| e.envelope.event_type == kind)
                .map(|e| e.envelope.payload)
                .collect::<Vec<_>>()
        }
    };
    let before = events("workarea.federation_set").await.len();
    for (key, accept) in [(w.reviewer.value.as_str(), false), (w.admin.as_str(), true)] {
        for _ in 0..2 {
            w.app
                .set_workarea_federation(key, Reference::Id(w.node("legal")), accept)
                .await
                .expect("a person's key or an admin sets it");
        }
    }
    let set = events("workarea.federation_set").await;
    assert_eq!(set.len(), before + 2, "a repeat journalled: {set:?}");
    assert_eq!(set[before]["from"], true);
    assert_eq!(set[before]["value"], false);
    assert_eq!(set[before + 1]["from"], false);
    assert_eq!(set[before + 1]["field"], "accept_federated");
    let before = events("federation.policy_set").await.len();
    for _ in 0..2 {
        w.max_sources(5).await;
    }
    let set = events("federation.policy_set").await;
    assert_eq!(set.len(), before + 1, "a repeat journalled: {set:?}");
    assert_eq!(set[before]["from"]["max_sources"], 8);
    assert_eq!(set[before]["policy"]["max_sources"], 5);
    let over = w
        .app
        .set_federation_policy(
            w.admin.as_str(),
            FederationPolicyChange {
                max_sources: Some(65),
                ..FederationPolicyChange::default()
            },
        )
        .await
        .expect_err("max_sources past 64");
    assert!(
        matches!(
            over,
            AppError::SourcesInvalid {
                field: "/max_sources",
                ..
            }
        ),
        "{over:?}"
    );
}

/// The candidate-count report counts |C| with no scope set aside: the
/// trusted workareas the key reads that accept federated reads, never a
/// `development` or a declining one (ADR-0072, "Who gains"; #205).
pub async fn the_candidate_count_is_the_trusted_sources_a_key_reads<S: JournalStore + Clone>(
    store: S,
) {
    let w = FedWorld::seeded(store).await;
    let key = w
        .key(Role::Reader, &["crew", "legal", "product", "drafts"], &[])
        .await;
    let count = |key: Uuid| {
        let w = &w;
        async move {
            w.app
                .federation_candidate_count(w.tenant, key)
                .await
                .expect("a count")
        }
    };
    // crew, home, legal and product; drafts is development.
    assert_eq!(count(key.key_id).await, Some(4));
    w.app
        .set_workarea_federation(
            w.reviewer.value.as_str(),
            Reference::Id(w.node("product")),
            false,
        )
        .await
        .expect("decline");
    assert_eq!(count(key.key_id).await, Some(3));
    assert_eq!(count(Uuid::nil()).await, None);
}

/// A world of `n` committed workareas under `fleet`, one memory each, and a
/// key reading them all: d12's budget at 1,000 candidates is measured over
/// it (ADR-0072 d12).
pub async fn fleet_world<S: JournalStore + Clone>(store: S, n: usize) -> (FedWorld<S>, IssuedKey) {
    let mut w = FedWorld::new(store).await;
    w.create("fleet", "universe").await;
    for i in 0..n {
        let node: &'static str = Box::leak(format!("fleet-{i}").into_boxed_str());
        w.create(node, "fleet").await;
        w.promote(
            node,
            &format!("fleet note {i} on invoice retry policy {}", i % 17),
        )
        .await;
        w.commit(node).await;
    }
    let key = w.key(Role::Reader, &["fleet"], &[]).await;
    (w, key)
}

/// One mode's wall times at `fleet-0`, after a warm-up.
pub async fn fleet_sample<S: JournalStore + Clone>(
    w: &FedWorld<S>,
    key: &IssuedKey,
    sources: Option<Sources>,
    runs: usize,
) -> Vec<f64> {
    let mut out = Vec::with_capacity(runs);
    for i in 0..=runs {
        let started = Instant::now();
        w.recall(
            key,
            QUERIES[i % QUERIES.len()],
            "fleet-0",
            sources.clone(),
            "bm25-postings",
            10,
        )
        .await
        .expect("a recall");
        if i > 0 {
            out.push(started.elapsed().as_secs_f64());
        }
    }
    out
}

/// The p-quantile of `v`, for a measurement's report.
#[must_use]
pub fn quantile_of(v: Vec<f64>, q: f64) -> f64 {
    quantile(v, q)
}

/// Every phase 1 law over fresh stores from `make`, but test 7, which needs
/// the store's own bulk path.
pub async fn run_federation_suite<S, F, Fut>(make: F, searches: &[&str])
where
    S: JournalStore + Clone,
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = S>,
{
    an_unreadable_workarea_changes_no_byte_of_any_answer(make().await).await;
    a_readable_workarea_that_is_no_source_changes_no_byte(make().await).await;
    a_scope_reader_never_meets_an_unreadable_workarea(make().await).await;
    a_list_is_answered_whole_or_refused_in_d5s_order(make().await).await;
    a_revoked_source_is_absent_from_the_next_recall(make().await).await;
    for search in searches {
        a_hits_score_never_depends_on_which_sources_were_selected(make().await, search).await;
    }
    with_federation_off_scope_is_todays_answer_and_wider_is_refused(make().await).await;
    a_lists_answer_depends_on_the_listed_workareas_alone(make().await).await;
    a_wide_answer_marks_each_hits_source(make().await).await;
    a_held_row_is_never_federated(make().await).await;
    a_held_row_scores_alike_on_the_scan_and_the_postings(make().await).await;
    the_policies_are_never_an_agents_act(make().await).await;
    the_candidate_count_is_the_trusted_sources_a_key_reads(make().await).await;
}
