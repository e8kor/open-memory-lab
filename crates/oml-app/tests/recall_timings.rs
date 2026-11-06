//! A recall's per-stage timing trace (ADR-0049 d14): always measured, and
//! served in the answer only on request where the deployment serves it.

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::{HeldBy, MemoryKind, Role};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::search::{MatchMode, Pipeline, SearchConfig, Stage};
use oml_app::types::{
    AppError, BootstrapRequest, GrantRequest, IssueKeyRequest, RecallRequest, SubmitRequest,
    Verdict,
};

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

async fn reader_with_a_hit(timings: bool) -> (TestApp, String) {
    let search = SearchConfig::new(&[Pipeline::KeywordAny], Pipeline::KeywordAny)
        .expect("served")
        .with_timings(timings);
    let app = App::new(
        MemStore::new(),
        MemSigner::new([7; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_search(search);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("root-admin"),
            admin_principal: "admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let root = boot.root_workarea_id;
    let mut keys = Vec::new();
    for (n, role) in [
        ("w", Role::Writer),
        ("d", Role::Dreamer),
        ("r", Role::Reader),
    ] {
        keys.push(
            app.issue_key(
                &admin,
                &IssueKeyRequest {
                    profile: None,
                    role,
                    name: name(&format!("key-{n}")),
                    principal: format!("p-{n}"),
                    held_by: Some(HeldBy::Agent),
                    default_workarea: Some(root.into()),
                    crew: None,
                    grants: vec![GrantRequest {
                        node: root.into(),
                        role,
                        deny: Vec::new(),
                        covers: Covers::Subtree,
                    }],
                    connect_code: false,
                    user: None,
                    guardrail_scope: None,
                },
            )
            .await
            .expect("key")
            .value,
        );
    }
    let id = app
        .submit_memory(
            &keys[0],
            &SubmitRequest {
                origin: None,
                valid_from: None,
                valid_until: None,
                on_similar: Default::default(),
                idempotency_key: "a".to_owned(),
                claim_key: None,
                verb: None,
                kind: MemoryKind::Memory,
                body: "the canary takes five percent of traffic".to_owned(),
                workarea: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
                anchor: None,
            },
        )
        .await
        .expect("submit")
        .receipt
        .memory_id;
    app.post_verdict(&keys[1], id, Verdict::Approve, None)
        .await
        .expect("approve");
    (app, keys[2].clone())
}

fn recall(timings: bool) -> RecallRequest {
    RecallRequest {
        include_expired: false,
        now: None,
        timings,
        query: "canary traffic".to_owned(),
        k: 10,
        workarea: None,
        kinds: Vec::new(),
        include_text: true,
        include_unreviewed: Some(false),
        max_tokens: None,
        search: None,
        match_mode: MatchMode::Any,
        anchor: Default::default(),
    }
}

#[tokio::test]
async fn a_recall_measures_each_stage_it_ran_in_order() {
    let (app, reader) = reader_with_a_hit(true).await;
    let result = app
        .recall(reader.as_str(), &recall(true))
        .await
        .expect("recall");
    assert_eq!(result.hits.len(), 1);
    let stages: Vec<&str> = result
        .timings
        .stages
        .iter()
        .map(|(s, _)| s.as_str())
        .collect();
    assert_eq!(
        stages,
        vec![
            "scope",
            "retrieve_keyword_any",
            "fuse",
            "cut",
            "suggest",
            "hydrate"
        ],
    );
    assert!(result.timings.stages.iter().all(|(_, s)| *s >= 0.0));
    assert!(
        !result
            .timings
            .stages
            .iter()
            .any(|(s, _)| *s == Stage::Rerank),
        "a pipeline that does not rerank has no rerank stage"
    );
    for name in &stages {
        assert!(Stage::NAMES.contains(name), "{name} is in the closed set");
    }
}

#[tokio::test]
async fn timings_are_refused_where_the_deployment_does_not_serve_them() {
    let (app, reader) = reader_with_a_hit(false).await;
    let refused = app
        .recall(reader.as_str(), &recall(true))
        .await
        .expect_err("refused");
    assert!(matches!(refused, AppError::TimingsOff), "{refused:?}");
    let served = app
        .recall(reader.as_str(), &recall(false))
        .await
        .expect("recall");
    assert!(
        !served.timings.stages.is_empty(),
        "measured for the histogram"
    );
}
