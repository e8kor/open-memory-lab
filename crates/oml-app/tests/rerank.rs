//! The reranking stage of recall (ADR-0053): after fusion and before the
//! cuts, over rows the core admits, falling back to the first stage's order
//! when the reranker is busy, late or failing, and withholding what scores
//! under the relevance floor. The first stage here is the in-memory store's
//! any-word list answering the `bm25` kind, so these tests prove the core's
//! stage, never BM25 (ADR-0049 d7).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::{HeldBy, MemoryKind, Role};
use oml_app::doubles::{FakeReranker, FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::RerankError;
use oml_app::search::{MatchMode, Pipeline, RerankPolicy, SearchConfig};
use oml_app::types::{
    BootstrapRequest, GrantRequest, IssueKeyRequest, RecallRequest, RecallResult, RerankOutcome,
    SubmitRequest, Verdict,
};
use uuid::Uuid;

const TEST_SEED: [u8; 32] = [7; 32];

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

/// An app serving `bm25-rerank` by default over the any-word first stage,
/// reranking with `reranker` under `policy`.
fn app_with(reranker: Arc<FakeReranker>, policy: RerankPolicy) -> TestApp {
    let search = SearchConfig::new(
        &[Pipeline::KeywordAny, Pipeline::Bm25Rerank],
        Pipeline::Bm25Rerank,
    )
    .expect("a served default")
    .with_rerank(policy);
    App::new(
        MemStore::new().answering_bm25_with_any_word(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_search(search)
    .with_reranker(reranker)
}

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

async fn issue(app: &TestApp, admin: &str, role: Role, grants: Vec<GrantRequest>) -> String {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    let n = ISSUED.fetch_add(1, Ordering::Relaxed);
    app.issue_key(
        admin,
        &IssueKeyRequest {
            role,
            name: name(&format!("rerank-key-{n}")),
            principal: format!("principal-{n}"),
            held_by: Some(HeldBy::Agent),
            default_workarea: None,
            crew: None,
            grants,
            connect_code: false,
            user: None,
            guardrail_scope: None,
        },
    )
    .await
    .expect("issue key")
    .value
}

/// A tenant with two sibling workareas: the writer, the dreamer and the
/// reader hold `crew`; only the writer and the dreamer hold `other`.
struct World {
    app: TestApp,
    writer: String,
    dreamer: String,
    reader: String,
    other: Uuid,
}

async fn world(reranker: Arc<FakeReranker>, policy: RerankPolicy) -> World {
    let app = app_with(reranker, policy);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("root-admin"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let mut nodes = Vec::new();
    for n in ["crew", "other"] {
        nodes.push(
            app.create_workarea(&admin, &name(n), Some(boot.root_workarea_id.into()), None)
                .await
                .expect("workarea")
                .workarea
                .workarea_id,
        );
    }
    let (crew, other) = (nodes[0], nodes[1]);
    let writer = issue(
        &app,
        &admin,
        Role::Writer,
        vec![
            grant(crew, Role::Writer),
            grant(crew, Role::Reader),
            grant(other, Role::Writer),
        ],
    )
    .await;
    let dreamer = issue(
        &app,
        &admin,
        Role::Dreamer,
        vec![grant(crew, Role::Dreamer), grant(other, Role::Dreamer)],
    )
    .await;
    let reader = issue(&app, &admin, Role::Reader, vec![grant(crew, Role::Reader)]).await;
    World {
        app,
        writer,
        dreamer,
        reader,
        other,
    }
}

fn submit(idem: &str, body: &str, workarea: Option<Uuid>) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workarea: workarea.map(Into::into),
        derives_from: Vec::new(),
        severity: None,
        rule: None,
    }
}

impl World {
    async fn held(&self, idem: &str, body: &str, workarea: Option<Uuid>) -> Uuid {
        self.app
            .submit_memory(&self.writer, &submit(idem, body, workarea))
            .await
            .expect("submit")
            .receipt
            .memory_id
    }

    async fn promoted(&self, idem: &str, body: &str, workarea: Option<Uuid>) -> Uuid {
        let id = self.held(idem, body, workarea).await;
        self.app
            .post_verdict(&self.dreamer, id, Verdict::Approve, None)
            .await
            .expect("approve");
        id
    }

    async fn recall(&self, key: &str, query: &str) -> RecallResult {
        self.app.recall(key, &recall(query)).await.expect("recall")
    }
}

fn recall(query: &str) -> RecallRequest {
    RecallRequest {
        timings: false,
        query: query.to_owned(),
        k: 10,
        workarea: None,
        kinds: Vec::new(),
        include_text: true,
        include_unreviewed: false,
        max_tokens: None,
        search: None,
        match_mode: MatchMode::Any,
    }
}

fn ids(r: &RecallResult) -> Vec<Uuid> {
    r.hits.iter().map(|h| h.memory.memory_id).collect()
}

/// The reranker that prefers passages naming a canary: 0.9 for those, 0.1
/// for every other.
fn prefers_canary(_query: &str, passage: &str) -> f32 {
    if passage.contains("canary") { 0.9 } else { 0.1 }
}

fn policy(depth: usize, floor: Option<f32>) -> RerankPolicy {
    RerankPolicy::new(depth, floor).expect("a valid policy")
}

#[tokio::test]
async fn bm25_rerank_reorders_the_first_stages_head_by_the_rerankers_relevance() {
    let w = world(
        Arc::new(FakeReranker::scoring(prefers_canary)),
        policy(20, None),
    )
    .await;
    let checklist = w
        .promoted("a", "deploy friday checklist for the api", None)
        .await;
    let canary = w.promoted("b", "deploy behind a canary first", None).await;

    let first = w
        .app
        .recall(
            &w.reader,
            &RecallRequest {
                timings: false,
                search: Some("keyword-any".to_owned()),
                ..recall("deploy friday")
            },
        )
        .await
        .expect("the first stage alone");
    assert_eq!(ids(&first), vec![checklist, canary], "two words before one");

    let r = w.recall(&w.reader, "deploy friday").await;
    assert_eq!(r.pipeline, Pipeline::Bm25Rerank);
    assert_eq!(r.rerank, RerankOutcome::Applied);
    assert_eq!(ids(&r), vec![canary, checklist], "the reranker's order");
    assert_eq!(r.hits[0].rerank, Some(0.9));
    assert_eq!(r.hits[0].rank_before, Some(2));
    assert!(
        (r.hits[0].score - 0.9).abs() < f32::EPSILON,
        "the score is the relevance"
    );
    assert_eq!(r.hits[1].rank_before, Some(1));
    assert_eq!(
        r.hits[0].matched,
        vec!["deploy".to_owned()],
        "the first stage's reasons stay"
    );
    assert_eq!(r.floor, None);
    assert_eq!(r.below_floor, 0);
}

#[tokio::test]
async fn the_reranker_is_handed_only_rows_the_core_would_serve() {
    let reranker = Arc::new(FakeReranker::new());
    let w = world(reranker.clone(), policy(20, None)).await;
    w.promoted("a", "deploy notes, served", None).await;
    w.held("b", "deploy notes, held and not reviewed", None)
        .await;
    let other = w.other;
    w.promoted(
        "c",
        "deploy notes in a workarea the reader cannot read",
        Some(other),
    )
    .await;

    let r = w.recall(&w.reader, "deploy notes").await;
    assert_eq!(r.hits.len(), 1);
    assert_eq!(
        reranker.seen(),
        vec![vec!["deploy notes, served".to_owned()]],
        "no held row of another key and no unreadable row reaches the model"
    );
}

#[tokio::test]
async fn an_own_held_row_is_reranked_by_the_same_function_as_a_promoted_one() {
    let w = world(Arc::new(FakeReranker::new()), policy(20, None)).await;
    let promoted = w.promoted("a", "rotate the signing key yearly", None).await;
    let held = w.held("b", "rotate the signing key yearly", None).await;

    let r = w
        .app
        .recall(
            &w.writer,
            &RecallRequest {
                timings: false,
                include_unreviewed: true,
                ..recall("rotate signing key")
            },
        )
        .await
        .expect("recall with own held rows");
    let score = |id| {
        r.hits
            .iter()
            .find(|h| h.memory.memory_id == id)
            .and_then(|h| h.rerank)
    };
    assert!(score(promoted).is_some());
    assert_eq!(
        score(promoted),
        score(held),
        "no verdict reaches the relevance"
    );
}

#[tokio::test]
async fn a_busy_reranker_serves_the_first_stages_order_and_says_skipped() {
    for (error, reason) in [
        (RerankError::Busy, "busy"),
        (RerankError::Timeout, "timeout"),
        (RerankError::Failed("tensor shape".to_owned()), "failed"),
    ] {
        let w = world(
            Arc::new(FakeReranker::failing(error)),
            policy(20, Some(0.5)),
        )
        .await;
        let checklist = w.promoted("a", "deploy friday checklist", None).await;
        let canary = w.promoted("b", "deploy behind a canary", None).await;

        let r = w.recall(&w.reader, "deploy friday").await;
        assert_eq!(r.rerank, RerankOutcome::Skipped(reason));
        assert_eq!(ids(&r), vec![checklist, canary], "the first stage's order");
        assert!(
            r.hits
                .iter()
                .all(|h| h.rerank.is_none() && h.rank_before.is_none())
        );
        assert_eq!(r.floor, None, "no rerank score, so no floor applies");
        assert_eq!(r.below_floor, 0);
    }
}

#[tokio::test]
async fn the_floor_withholds_reranked_hits_below_it_and_counts_them() {
    let w = world(
        Arc::new(FakeReranker::scoring(prefers_canary)),
        policy(20, Some(0.5)),
    )
    .await;
    w.promoted("a", "deploy friday checklist", None).await;
    let canary = w.promoted("b", "deploy behind a canary", None).await;

    let r = w.recall(&w.reader, "deploy friday").await;
    assert_eq!(ids(&r), vec![canary]);
    assert_eq!(r.floor, Some(0.5));
    assert_eq!(r.below_floor, 1);
    assert_eq!(
        r.served_ids,
        vec![canary],
        "the read log records what was served"
    );
}

#[tokio::test]
async fn nothing_above_the_floor_is_an_explicit_empty_answer() {
    let w = world(Arc::new(FakeReranker::new()), policy(20, Some(0.9))).await;
    w.promoted("a", "deploy friday checklist", None).await;
    w.promoted("b", "deploy behind a canary", None).await;

    let r = w.recall(&w.reader, "deploy saturday morning").await;
    assert!(r.hits.is_empty());
    assert_eq!(r.rerank, RerankOutcome::Applied);
    assert_eq!(
        r.below_floor, 2,
        "both candidates were withheld, and counted"
    );
    assert!(
        !r.terms.is_empty(),
        "the answer still says what was searched"
    );
}

#[tokio::test]
async fn the_reranker_reorders_only_the_head_and_the_tail_follows_in_its_first_stage_order() {
    let w = world(
        Arc::new(FakeReranker::scoring(prefers_canary)),
        policy(1, None),
    )
    .await;
    let both = w.promoted("a", "deploy friday checklist", None).await;
    let canary = w.promoted("b", "deploy behind a canary", None).await;

    let r = w.recall(&w.reader, "deploy friday").await;
    assert_eq!(
        ids(&r),
        vec![both, canary],
        "a head of one cannot move the second"
    );
    assert_eq!(r.hits[0].rerank, Some(0.1));
    assert_eq!(r.hits[1].rerank, None, "the tail was not scored");
}

#[tokio::test]
async fn a_pipeline_that_does_not_rerank_never_calls_the_reranker() {
    let reranker = Arc::new(FakeReranker::new());
    let w = world(reranker.clone(), policy(20, Some(0.9))).await;
    w.promoted("a", "deploy friday checklist", None).await;

    let r = w
        .app
        .recall(
            &w.reader,
            &RecallRequest {
                timings: false,
                search: Some("keyword-any".to_owned()),
                ..recall("deploy saturday")
            },
        )
        .await
        .expect("keyword-any");
    assert_eq!(r.rerank, RerankOutcome::NotRun);
    assert_eq!(r.hits.len(), 1, "no floor without a rerank");
    assert!(reranker.seen().is_empty());
}

fn not_a_number(_query: &str, _passage: &str) -> f32 {
    f32::NAN
}

fn above_one(_query: &str, _passage: &str) -> f32 {
    1.5
}

/// A score that is not a finite relevance in `[0, 1]` would sort first or
/// serialise as null: the core refuses the whole list as `failed` and serves
/// the first stage's order (ADR-0053 d5).
#[tokio::test]
async fn a_score_that_is_not_a_finite_relevance_skips_the_rerank_as_failed() {
    for relevance in [not_a_number as fn(&str, &str) -> f32, above_one] {
        let w = world(Arc::new(FakeReranker::scoring(relevance)), policy(20, None)).await;
        let checklist = w.promoted("a", "deploy friday checklist", None).await;
        let canary = w.promoted("b", "deploy behind a canary", None).await;

        let r = w.recall(&w.reader, "deploy friday").await;
        assert_eq!(r.rerank, RerankOutcome::Skipped("failed"));
        assert_eq!(ids(&r), vec![checklist, canary]);
        assert!(r.hits.iter().all(|h| h.rerank.is_none()));
    }
}
