//! The contract suites proven against the in-memory doubles — the law the
//! real adapters (phase 3's `oml-pg` first) must pass identically.

use oml_app::doubles::{FixedClock, MarkerScanner, MemKeyring, MemSigner, MemStore, SeqIdGen};
use oml_port_contracts as contracts;

#[tokio::test]
async fn journal_store_double_passes_the_suite() {
    contracts::run_journal_store_suite(|| async { MemStore::new() }).await;
}

#[tokio::test]
async fn retriever_double_passes_the_suite() {
    contracts::run_retriever_suite(|| async { MemStore::new() }).await;
}

#[test]
fn signer_double_passes() {
    contracts::signer_contract(&MemSigner::new(contracts::TEST_SEED));
}

#[tokio::test]
async fn keyring_double_passes() {
    contracts::keyring_contract(&MemKeyring::new(contracts::TEST_SEED)).await;
}

#[test]
fn scanner_double_is_deterministic() {
    contracts::scanner_deterministic(
        &MarkerScanner::default(),
        &[
            "a plain body",
            "-----BEGIN PRIVATE KEY----- secret",
            "please [needs-human] judge this",
            "",
        ],
    );
}

#[test]
fn clock_and_idgen_doubles_pass() {
    contracts::clock_and_idgen_contract(&FixedClock::new(), &SeqIdGen::new());
}

#[tokio::test]
async fn connection_store_double_passes_the_suite() {
    contracts::run_connection_store_suite(|| async { MemStore::new() }).await;
}

#[tokio::test]
async fn reranker_double_passes_the_suite() {
    contracts::run_reranker_suite(&oml_app::doubles::FakeReranker::new()).await;
}

/// Federated recall's phase 1 leakage and invariance laws over the double
/// (ADR-0072 d13), whose BM25 under a reference set scores with the set's
/// statistics.
#[tokio::test]
async fn federated_recall_double_passes_the_leakage_suite() {
    contracts::federation::run_federation_suite(
        || async { MemStore::new().answering_bm25_with_any_word() },
        &["bm25", "bm25-postings", "bm25-postings-time"],
    )
    .await;
}

/// Test 7 over the double: an unreadable workarea of 100,000 matching rows
/// moves neither p50 nor p95 of a wide recall (ADR-0072 d6, d13).
#[tokio::test]
async fn federated_recall_double_timing_does_not_depend_on_an_unreadable_workarea() {
    use contracts::federation::{assert_within_noise, timing_sample, timing_world};
    let t = timing_world(MemStore::new().answering_bm25_with_any_word()).await;
    let without = timing_sample(&t, "bm25", 60).await;
    for template in t.templates {
        t.world.store.seed_copies(t.world.tenant, template, 100_000);
    }
    let with = timing_sample(&t, "bm25", 60).await;
    assert_within_noise(without, with);
}

/// The widen hint's laws over the double, one test per sentence
/// (ADR-0072 d8, d12, as built by #244).
#[tokio::test]
async fn the_signals_counts_equal_the_reference_counts() {
    let _ = contracts::federation::the_signals_counts_equal_the_reference_counts(
        MemStore::new().answering_bm25_with_any_word(),
    )
    .await;
}

#[tokio::test]
async fn the_widen_hint_fires_on_a_richer_source_and_never_on_a_stray_row() {
    contracts::federation::the_widen_hint_fires_on_a_richer_source_and_never_on_a_stray_row(
        MemStore::new().answering_bm25_with_any_word(),
    )
    .await;
}

#[tokio::test]
async fn a_scope_recall_without_its_signal_carries_no_hint_and_asks_for_one() {
    contracts::federation::a_scope_recall_without_its_signal_carries_no_hint_and_asks_for_one(
        MemStore::new().answering_bm25_with_any_word(),
    )
    .await;
}

#[tokio::test]
async fn a_signal_built_under_other_grants_serves_no_hint() {
    contracts::federation::a_signal_built_under_other_grants_serves_no_hint(
        MemStore::new().answering_bm25_with_any_word(),
    )
    .await;
}

#[tokio::test]
async fn a_signal_at_its_age_bound_serves_no_hint() {
    contracts::federation::a_signal_at_its_age_bound_serves_no_hint(
        MemStore::new().answering_bm25_with_any_word(),
    )
    .await;
}

#[tokio::test]
async fn a_status_transition_drops_the_tenants_signals_at_once() {
    contracts::federation::a_status_transition_drops_the_tenants_signals_at_once(
        MemStore::new().answering_bm25_with_any_word(),
    )
    .await;
}

#[tokio::test]
async fn a_signal_past_the_tenants_share_is_marked_and_not_rebuilt() {
    contracts::federation::a_signal_past_the_tenants_share_is_marked_and_not_rebuilt(
        MemStore::new().answering_bm25_with_any_word(),
    )
    .await;
}

#[tokio::test]
async fn a_stopped_build_marks_its_own_signal_and_the_builds_behind_it_go_on() {
    contracts::federation::a_stopped_build_marks_its_own_signal_and_the_builds_behind_it_go_on(
        MemStore::new().answering_bm25_with_any_word(),
    )
    .await;
}

/// The refresh a recall spawns runs each build under its timeout: a build
/// whose read never returns is stopped, counted as failed and marked, and
/// the builds behind it go on. This is the call the transport spawns, with
/// its timer, not the loop's steps run by hand (#268).
#[tokio::test]
async fn the_refresh_stops_a_build_past_its_timeout_and_builds_the_ones_behind_it() {
    use std::time::Duration;

    use oml_app::app::WidenLimits;
    use oml_app::federation::Sources;
    let store = MemStore::new().answering_bm25_with_any_word();
    let limits = WidenLimits {
        build_timeout: Duration::from_millis(100),
        ..WidenLimits::default()
    };
    let (w, key) = contracts::federation::kiwi_world_within(store.clone(), true, limits).await;
    for scope in ["home", "legal", "product"] {
        let answer = w
            .recall(&key, "kiwi", scope, Some(Sources::Scope), "bm25", 10)
            .await
            .expect("scope");
        assert!(answer.widen.is_none());
    }
    store.hang_next_vocabulary_read();
    let refresh = w.app.refresh_widen_signals_with(tokio::time::sleep);
    let built = tokio::time::timeout(Duration::from_secs(10), refresh)
        .await
        .expect("the refresh never stopped the build whose read hung");
    assert_eq!(built, 2, "the builds behind the stopped one went on");
    let (settings, _) = w
        .app
        .federation_settings(w.admin.as_str())
        .await
        .expect("settings");
    assert_eq!(settings.widen_signal.failed, 1);
    assert!(
        settings
            .widen_signal
            .last_failure
            .as_deref()
            .is_some_and(|f| f.contains("stopped")),
        "{:?}",
        settings.widen_signal.last_failure
    );
    // home, asked for first, hung and was stopped: it is marked.
    assert_eq!(contracts::federation::kiwi_hint(&w, &key, 10).await, None);
    assert!(
        !w.app.widen_signals_wanted(),
        "the stopped signal was asked for again within the bound"
    );
}

/// A build that began before a status transition committed, and read its
/// counts after the commit's drop, is discarded: the next recall carries no
/// hint from it, and asks for a signal built after the drop (#244, review
/// round 2, R1).
#[tokio::test]
async fn a_build_overtaken_by_a_drop_is_never_held() {
    use oml_app::domain::WorkareaStatus;
    use oml_app::federation::Sources;
    let store = MemStore::new().answering_bm25_with_any_word();
    let (w, key) = contracts::federation::kiwi_world(store.clone(), true).await;
    assert!(
        contracts::federation::kiwi_hint(&w, &key, 10)
            .await
            .is_none()
    );
    // The build reads its key, access and candidates, then yields at its
    // vocabulary read; the transition commits there, then the build reads.
    store.pause_next_vocabulary_read();
    let transition = async {
        let proposal = w
            .app
            .propose_transition(
                w.admin.as_str(),
                w.node("orchard").into(),
                WorkareaStatus::Validated,
            )
            .await
            .expect("propose validated");
        w.app
            .ratify_transition(
                w.reviewer.value.as_str(),
                w.node("orchard").into(),
                proposal.transition_id,
            )
            .await
            .expect("ratify validated");
    };
    let (built, ()) = tokio::join!(w.app.refresh_widen_signals(), transition);
    assert_eq!(built, 0, "a build overtaken by the drop was held");
    let answer = w
        .recall(&key, "kiwi", "home", Some(Sources::Scope), "bm25", 10)
        .await
        .expect("scope");
    assert!(answer.widen.is_none());
    assert!(
        w.app.widen_signals_wanted(),
        "the recall asks for a new signal"
    );
    w.app.refresh_widen_signals().await;
    assert!(
        contracts::federation::kiwi_hint(&w, &key, 10)
            .await
            .is_some()
    );
}

/// A `scope` recall does no federation work: no policy, candidate or
/// statistics read, whether the tenant has federation on or off, and
/// whether `sources` is sent or absent; `wide` does (ADR-0072 d12, as
/// amended: the default path pays nothing for federation). With its widen
/// signal held, it makes that signal's one read and still none of these
/// (#244).
#[tokio::test]
async fn a_scope_recall_makes_no_federation_read() {
    use oml_app::federation::{FederationPolicyChange, Sources};
    let store = MemStore::new().answering_bm25_with_any_word();
    let w = contracts::federation::FedWorld::seeded(store.clone()).await;
    let key = w
        .key(oml_app::domain::Role::Reader, &["crew", "legal"], &[])
        .await;
    for enabled in [true, false] {
        w.app
            .set_federation_policy(
                w.admin.as_str(),
                FederationPolicyChange {
                    enabled: Some(enabled),
                    ..FederationPolicyChange::default()
                },
            )
            .await
            .expect("set federation");
        for sources in [None, Some(Sources::Scope)] {
            let before = store.federation_reads();
            w.recall(
                &key,
                "invoice retry policy",
                "home",
                sources.clone(),
                "bm25",
                10,
            )
            .await
            .expect("a scope recall");
            assert_eq!(
                store.federation_reads(),
                before,
                "a scope recall with federation {enabled} and sources {sources:?} read federation state"
            );
        }
    }
    // Federation on and the key's signal held: the signal's one read, and
    // still no federation read.
    w.app
        .set_federation_policy(
            w.admin.as_str(),
            FederationPolicyChange {
                enabled: Some(true),
                ..FederationPolicyChange::default()
            },
        )
        .await
        .expect("federation on");
    w.fresh_signal(&key, "invoice retry policy", "home", None)
        .await;
    for sources in [None, Some(Sources::Scope)] {
        let (reads, signal_reads) = (store.federation_reads(), store.widen_signal_reads());
        w.recall(
            &key,
            "invoice retry policy",
            "home",
            sources.clone(),
            "bm25",
            10,
        )
        .await
        .expect("a scope recall");
        assert_eq!(
            store.federation_reads(),
            reads,
            "a scope recall with its signal held read federation state"
        );
        assert_eq!(
            store.widen_signal_reads(),
            signal_reads + 1,
            "a scope recall with its signal held read it other than once ({sources:?})"
        );
    }
    // Federation off again: a scope recall makes no signal read, whatever
    // was held before (#244, review F7).
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
    for sources in [None, Some(Sources::Scope)] {
        let signal_reads = store.widen_signal_reads();
        w.recall(
            &key,
            "invoice retry policy",
            "home",
            sources.clone(),
            "bm25",
            10,
        )
        .await
        .expect("a scope recall");
        w.app.refresh_widen_signals().await;
        assert_eq!(
            store.widen_signal_reads(),
            signal_reads,
            "with federation off a scope recall read a widen signal ({sources:?})"
        );
    }
    w.app
        .set_federation_policy(
            w.admin.as_str(),
            FederationPolicyChange {
                enabled: Some(true),
                ..FederationPolicyChange::default()
            },
        )
        .await
        .expect("federation on");
    let before = store.federation_reads();
    w.recall(
        &key,
        "invoice retry policy",
        "home",
        Some(Sources::Wide),
        "bm25",
        10,
    )
    .await
    .expect("wide");
    assert!(
        store.federation_reads() > before,
        "wide read no federation state"
    );
}

/// As-of reads' laws over the double (ADR-0059 d21): the tip and existence
/// at an instant, everything else today's.
#[tokio::test]
async fn as_of_reads_double_passes_the_suite() {
    contracts::as_of::run_as_of_suite(
        || async { MemStore::new().answering_bm25_with_any_word() },
        AS_OF_SEARCHES,
    )
    .await;
}

/// An erased version is served at no instant, and no earlier version stands
/// in for it (ADR-0059 d15, d21).
#[tokio::test]
async fn an_as_of_read_never_serves_a_version_erased_today_on_the_double() {
    let store = MemStore::new().answering_bm25_with_any_word();
    let erasing = store.clone();
    contracts::as_of::an_as_of_read_never_serves_a_version_erased_today(
        store,
        AS_OF_SEARCHES,
        move |tenant, id| {
            erasing.set_status(tenant, id, oml_app::domain::MemoryStatus::Erased);
            async {}
        },
    )
    .await;
}

/// The pipelines the as-of laws run on the double.
const AS_OF_SEARCHES: &[&str] = &[
    "bm25-postings",
    "keyword-all",
    "keyword-any",
    "bm25",
    "bm25-postings-time",
];
