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
