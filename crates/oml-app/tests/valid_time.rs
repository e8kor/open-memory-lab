//! Valid time through the core (proposed ADR-0059): declared and signed
//! ends, expired memories hidden from recall unless asked, and the `time`
//! leg. The text list here is the in-memory
//! store's any-word list answering the `bm25_postings` kind, so these tests
//! prove the core's stages, never BM25 (ADR-0049 d7).

use std::sync::atomic::{AtomicU64, Ordering};

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::event_types;
use oml_app::domain::name::Name;
use oml_app::domain::{HeldBy, MemoryKind, Role};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore;
use oml_app::search::{MatchMode, Pipeline, RetrieverKind, SearchConfig};
use oml_app::types::{
    AnchorFilter, AppError, BootstrapRequest, GrantRequest, IssueKeyRequest, RecallRequest,
    RecallResult, SubmitRequest, SupersedeRequest, Verdict,
};
use uuid::Uuid;

const TEST_SEED: [u8; 32] = [9; 32];

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

const PIPELINES: [Pipeline; 3] = [
    Pipeline::KeywordAny,
    Pipeline::Bm25Postings,
    Pipeline::Bm25PostingsTime,
];

/// Every pipeline the expiry rule binds, `keyword-all` among them: its
/// v0.5.0 statement keeps no expiry cut, so the core's re-check is its one
/// fence (proposed ADR-0059 d3).
const EVERY_PIPELINE: [Pipeline; 4] = [
    Pipeline::KeywordAll,
    Pipeline::KeywordAny,
    Pipeline::Bm25Postings,
    Pipeline::Bm25PostingsTime,
];

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
    store: MemStore,
    clock: FixedClock,
    tenant: Uuid,
    writer: String,
    dreamer: String,
    reader: String,
}

async fn issue(app: &TestApp, admin: &str, role: Role, grants: Vec<GrantRequest>) -> String {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    let n = ISSUED.fetch_add(1, Ordering::Relaxed);
    app.issue_key(
        admin,
        &IssueKeyRequest {
            role,
            name: name(&format!("time-key-{n}")),
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

async fn world() -> World {
    let store = MemStore::new().answering_bm25_with_any_word();
    let clock = FixedClock::new();
    let search =
        SearchConfig::new(&EVERY_PIPELINE, Pipeline::Bm25Postings).expect("a served default");
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_search(search);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("root-admin"),
            admin_principal: "alice-admin".to_owned(),
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
        .expect("workarea")
        .workarea
        .workarea_id;
    let writer = issue(
        &app,
        &admin,
        Role::Writer,
        vec![grant(crew, Role::Writer), grant(crew, Role::Reader)],
    )
    .await;
    let dreamer = issue(
        &app,
        &admin,
        Role::Dreamer,
        vec![grant(crew, Role::Dreamer)],
    )
    .await;
    let reader = issue(&app, &admin, Role::Reader, vec![grant(crew, Role::Reader)]).await;
    World {
        app,
        store,
        clock,
        tenant: boot.tenant_id,
        writer,
        dreamer,
        reader,
    }
}

fn submit(idem: &str, body: &str, from: Option<&str>, until: Option<&str>) -> SubmitRequest {
    SubmitRequest {
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workarea: None,
        derives_from: Vec::new(),
        severity: None,
        rule: None,
        valid_from: from.map(str::to_owned),
        valid_until: until.map(str::to_owned),
        anchor: None,
        format: None,
        title: None,
    }
}

fn recall(query: &str, pipeline: Pipeline) -> RecallRequest {
    RecallRequest {
        query: query.to_owned(),
        k: 10,
        workarea: None,
        kinds: Vec::new(),
        include_text: true,
        include_unreviewed: false,
        max_tokens: None,
        search: Some(pipeline.as_str().to_owned()),
        match_mode: MatchMode::Any,
        include_expired: false,
        now: None,
        anchor: AnchorFilter::default(),
    }
}

fn ids(r: &RecallResult) -> Vec<Uuid> {
    r.hits.iter().map(|h| h.memory.memory_id).collect()
}

impl World {
    async fn promoted(
        &self,
        idem: &str,
        body: &str,
        from: Option<&str>,
        until: Option<&str>,
    ) -> Uuid {
        let id = self
            .app
            .submit_memory(&self.writer, &submit(idem, body, from, until))
            .await
            .expect("submit")
            .receipt
            .memory_id;
        self.app
            .post_verdict(&self.dreamer, id, Verdict::Approve, None)
            .await
            .expect("approve");
        id
    }

    async fn recall(&self, req: &RecallRequest) -> RecallResult {
        self.app.recall(&self.reader, req).await.expect("recall")
    }
}

#[tokio::test]
async fn an_expired_memory_is_hidden_from_recall_unless_asked() {
    let w = world().await;
    // The fixed clock reads 2026-10-29.
    let ended = w
        .promoted(
            "a",
            "the deploy canary takes five percent",
            None,
            Some("2026-01"),
        )
        .await;
    let current = w
        .promoted(
            "b",
            "the deploy canary takes ten percent",
            Some("2026-02"),
            None,
        )
        .await;
    for pipeline in EVERY_PIPELINE {
        let hidden = w.recall(&recall("deploy canary", pipeline)).await;
        assert_eq!(ids(&hidden), vec![current], "{pipeline}");
        let asked = w
            .recall(&RecallRequest {
                include_expired: true,
                ..recall("deploy canary", pipeline)
            })
            .await;
        let mut both = ids(&asked);
        both.sort_unstable();
        let mut want = vec![ended, current];
        want.sort_unstable();
        assert_eq!(both, want, "{pipeline}: include_expired serves it");
    }
    // Get by id serves it as it always did, with its end.
    let got = w.app.get_memory(&w.reader, ended).await.expect("get");
    assert_eq!(
        got.memory.valid.until.as_ref().map(|u| u.as_str()),
        Some("2026-01")
    );
}

#[tokio::test]
async fn an_expired_memory_spends_no_place_in_k() {
    let w = world().await;
    // The expired row holds more of the query's words, so it would rank
    // first; it must not take the one place k leaves.
    w.promoted("a", "deploy canary rollback window", None, Some("2025"))
        .await;
    let current = w.promoted("b", "deploy canary", None, None).await;
    for pipeline in PIPELINES {
        let r = w
            .recall(&RecallRequest {
                k: 1,
                ..recall("deploy canary rollback window", pipeline)
            })
            .await;
        assert_eq!(ids(&r), vec![current], "{pipeline}");
    }
}

#[tokio::test]
async fn the_recalls_now_decides_what_has_expired() {
    let w = world().await;
    let id = w
        .promoted("a", "the freeze holds", None, Some("2026-11-15"))
        .await;
    let at = |now: &str| RecallRequest {
        now: Some(now.to_owned()),
        ..recall("freeze", Pipeline::Bm25Postings)
    };
    assert_eq!(
        ids(&w.recall(&at("2026-11-15")).await),
        vec![id],
        "through that day"
    );
    assert!(ids(&w.recall(&at("2026-11-16")).await).is_empty());
    assert!(ids(&w.recall(&at("2026-11-16T00:00:00+01:00")).await).len() == 1);
    let err = w
        .app
        .recall(&w.reader, &at("next tuesday"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NowUnparsable), "{err:?}");
}

#[tokio::test]
async fn a_query_with_no_window_answers_as_bm25_postings_does() {
    let w = world().await;
    for (i, body) in [
        "deploy canary five percent",
        "the canary waits twenty minutes",
        "deploy freeze on fridays",
        "rollback deploy by tag",
    ]
    .iter()
    .enumerate()
    {
        let from = (i % 2 == 0).then_some("2015");
        w.promoted(&format!("m{i}"), body, from, None).await;
    }
    let base = w
        .recall(&recall("deploy canary", Pipeline::Bm25Postings))
        .await;
    for pipeline in [Pipeline::Bm25PostingsTime] {
        let r = w.recall(&recall("deploy canary", pipeline)).await;
        assert_eq!(ids(&r), ids(&base), "{pipeline}");
        assert!(r.window.is_none());
        assert!(r.hits.iter().all(|h| !h.via.contains(&RetrieverKind::Time)));
    }
}

#[tokio::test]
async fn the_time_leg_lifts_the_memory_whose_valid_time_overlaps_the_querys_window() {
    let w = world().await;
    // Equal text matches; ids order the 2019 row first without a window.
    let late = w
        .promoted(
            "a",
            "we moved the deploy to friday",
            Some("2019"),
            Some("2019"),
        )
        .await;
    let early = w
        .promoted(
            "b",
            "we moved the deploy to monday",
            Some("2015-03"),
            Some("2015-06"),
        )
        .await;
    let open = w.promoted("c", "we moved the deploy", None, None).await;
    // Both dated rows ended long before the clock's now; history is asked
    // for with include_expired.
    let history = |pipeline| RecallRequest {
        include_expired: true,
        ..recall("when we moved the deploy in 2015", pipeline)
    };
    let plain = w.recall(&history(Pipeline::Bm25Postings)).await;
    assert_ne!(ids(&plain).first(), Some(&early));
    let r = w.recall(&history(Pipeline::Bm25PostingsTime)).await;
    assert_eq!(
        ids(&r).first(),
        Some(&early),
        "{:?}",
        r.hits
            .iter()
            .map(|h| (
                h.memory.memory_id,
                h.score,
                h.via.clone(),
                h.matched.clone()
            ))
            .collect::<Vec<_>>()
    );
    let window = r.window.as_ref().expect("the query names 2015");
    assert_eq!(window.phrase, "2015");
    let hit = &r.hits[0];
    assert!(hit.via.contains(&RetrieverKind::Time));
    // The hit carries the dates it was found by, so a "when" question can
    // be answered from them (proposed ADR-0059 d9).
    assert_eq!(
        hit.memory.valid.from.as_ref().map(|f| f.as_str()),
        Some("2015-03")
    );
    assert_eq!(
        hit.memory.valid.until.as_ref().map(|u| u.as_str()),
        Some("2015-06")
    );
    for other in [late, open] {
        let h = r
            .hits
            .iter()
            .find(|h| h.memory.memory_id == other)
            .expect("still served");
        assert!(
            !h.via.contains(&RetrieverKind::Time),
            "outside the window, or open"
        );
    }
}

#[tokio::test]
async fn no_ranking_input_reads_promoted_at_or_a_verdict() {
    let w = world().await;
    // Two rows equal in text and declared time, submitted together and
    // promoted an hour apart, in the reverse of their ids' order.
    let first = w
        .app
        .submit_memory(
            &w.writer,
            &submit("a", "the cache warms at noon", Some("2026-01"), None),
        )
        .await
        .expect("submit")
        .receipt
        .memory_id;
    let second = w
        .app
        .submit_memory(
            &w.writer,
            &submit("b", "the cache warms at noon", Some("2026-01"), None),
        )
        .await
        .expect("submit")
        .receipt
        .memory_id;
    w.app
        .post_verdict(&w.dreamer, second, Verdict::Approve, None)
        .await
        .expect("approve");
    w.clock.advance(3_600);
    w.app
        .post_verdict(&w.dreamer, first, Verdict::Approve, Some("checked twice"))
        .await
        .expect("approve");
    for pipeline in PIPELINES {
        let r = w.recall(&recall("cache warms", pipeline)).await;
        assert_eq!(r.hits.len(), 2, "{pipeline}");
        // A single list scores equal rows equal; reciprocal rank gives a tie
        // consecutive ranks, broken by id in the text list.
        if !matches!(pipeline.fusion(), oml_app::search::Fusion::Rrf { .. }) {
            assert!(
                (r.hits[0].score - r.hits[1].score).abs() < f32::EPSILON,
                "{pipeline}: equal rows score equal"
            );
        }
        let mut want = vec![first, second];
        want.sort_unstable();
        assert_eq!(ids(&r), want, "{pipeline}: ties by id, never by promotion");
    }
}

#[tokio::test]
async fn valid_time_is_refused_on_steering_and_on_a_retirement() {
    let w = world().await;
    let steering = SubmitRequest {
        kind: MemoryKind::Steering,
        ..submit("s", "always deploy behind a canary", Some("2015"), None)
    };
    let err = w.app.submit_memory(&w.writer, &steering).await.unwrap_err();
    assert!(matches!(err, AppError::ValidTime(_)), "{err:?}");
    let bad = submit("x", "the deploy", Some("in 2015"), None);
    let err = w.app.submit_memory(&w.writer, &bad).await.unwrap_err();
    assert!(
        matches!(&err, AppError::ValidTime(e) if e.field() == "/valid_from"),
        "{err:?}"
    );
    let target = w
        .promoted("t", "the deploy runs on fridays", None, None)
        .await;
    let retire = SupersedeRequest {
        idempotency_key: "r".to_owned(),
        target_id: target,
        body: "no longer true".to_owned(),
        kind: None,
        retirement: true,
        derives_from: Vec::new(),
        reason: None,
        workarea: None,
        refuse_vocabulary_target: false,
        anchor: None,
        expected_version: 1,
        severity: None,
        rule: None,
        valid_from: None,
        valid_until: Some("2026".to_owned()),
        format: None,
        title: None,
    };
    let err = w.app.supersede(&w.writer, &retire).await.unwrap_err();
    assert!(matches!(err, AppError::ValidTime(_)), "{err:?}");
}

#[tokio::test]
async fn a_fact_ends_by_a_reviewed_supersession_and_a_successor_inherits_no_valid_time() {
    let w = world().await;
    let v1 = w
        .promoted("a", "the freeze holds", Some("2026-10"), None)
        .await;
    let supersede = |idem: &str, until: Option<&str>| SupersedeRequest {
        idempotency_key: idem.to_owned(),
        target_id: v1,
        body: "the freeze holds".to_owned(),
        kind: None,
        retirement: false,
        derives_from: Vec::new(),
        reason: Some("ended".to_owned()),
        workarea: None,
        refuse_vocabulary_target: false,
        anchor: None,
        expected_version: 1,
        severity: None,
        rule: None,
        valid_from: None,
        valid_until: until.map(str::to_owned),
        format: None,
        title: None,
    };
    let v2 = w
        .app
        .supersede(&w.writer, &supersede("b", Some("2026-10-20")))
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    let held = w
        .app
        .get_memory(&w.writer, v2)
        .await
        .expect("own read")
        .memory;
    assert!(
        held.valid.from.is_none(),
        "the successor inherits no valid_from"
    );
    // Held, it ends nothing: v1 is still the tip and still served.
    assert_eq!(
        ids(&w.recall(&recall("freeze", Pipeline::Bm25Postings)).await),
        vec![v1]
    );
    w.app
        .post_verdict(&w.dreamer, v2, Verdict::Approve, None)
        .await
        .expect("approve");
    assert!(ids(&w.recall(&recall("freeze", Pipeline::Bm25Postings)).await).is_empty());
    // The replaced version keeps its own signed interval.
    let old = w
        .app
        .get_memory(&w.reader, v1)
        .await
        .expect("history")
        .memory;
    assert_eq!(old.valid.from.as_ref().map(|f| f.as_str()), Some("2026-10"));
    assert!(old.valid.until.is_none());
}

#[tokio::test]
async fn the_signed_payload_carries_the_ends_only_when_sent() {
    let w = world().await;
    let dated = w
        .promoted(
            "a",
            "the office moved",
            Some("2015"),
            Some("2016-03-01T09:30:00+01:00"),
        )
        .await;
    let open = w.promoted("b", "the office is in lviv", None, None).await;
    let events = w
        .store
        .events_range(w.tenant, 1, 1000)
        .await
        .expect("journal");
    let payload = |id: Uuid| {
        events
            .iter()
            .map(|e| &e.envelope)
            .find(|e| {
                e.event_type == event_types::SUBMISSION_REMEMBER
                    && e.payload["artifact_id"] == id.to_string()
            })
            .expect("the submission is journalled")
            .payload
            .clone()
    };
    let p = payload(dated);
    assert_eq!(p["valid_from"], "2015");
    assert_eq!(p["valid_until"], "2016-03-01T08:30:00.000000Z");
    let p = payload(open);
    assert!(p.get("valid_from").is_none() && p.get("valid_until").is_none());
}

#[tokio::test]
async fn a_ratified_supersession_ends_the_replaced_version_where_the_successor_begins() {
    let w = world().await;
    let v1 = w
        .promoted("a", "the api is at version 3", Some("2025-01"), None)
        .await;
    let next = SupersedeRequest {
        idempotency_key: "b".to_owned(),
        target_id: v1,
        body: "the api is at version 4".to_owned(),
        kind: None,
        retirement: false,
        derives_from: Vec::new(),
        reason: None,
        workarea: None,
        refuse_vocabulary_target: false,
        anchor: None,
        expected_version: 1,
        severity: None,
        rule: None,
        valid_from: Some("2026-06-01".to_owned()),
        valid_until: None,
        format: None,
        title: None,
    };
    let v2 = w
        .app
        .supersede(&w.writer, &next)
        .await
        .expect("supersede")
        .receipt
        .memory_id;
    // Held, it ends nothing.
    let before = w.app.get_memory(&w.reader, v1).await.expect("get").memory;
    assert!(before.valid.until.is_none(), "never on submission");
    w.app
        .post_verdict(&w.dreamer, v2, Verdict::Approve, None)
        .await
        .expect("approve");
    let after = w.app.get_memory(&w.reader, v1).await.expect("get").memory;
    assert_eq!(
        after.valid.until.as_ref().map(|u| u.as_str()),
        Some("2026-06-01T00:00:00.000000Z"),
        "the ratification ends it where the successor begins"
    );
    let events = w
        .store
        .events_range(w.tenant, 1, 1000)
        .await
        .expect("journal");
    let ratified = events
        .iter()
        .map(|e| &e.envelope)
        .find(|e| e.event_type == event_types::LIFECYCLE_SUPERSEDE)
        .expect("the supersession is ratified");
    assert_eq!(
        ratified.payload["replaces_valid_until"], "2026-06-01T00:00:00.000000Z",
        "signed in the ratifying event"
    );
}
