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

/// A `scope` recall does no federation work: no policy, candidate or
/// statistics read, whether the tenant has federation on or off, and
/// whether `sources` is sent or absent; `wide` does (ADR-0072 d12, as
/// amended: the default path pays nothing for federation).
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
