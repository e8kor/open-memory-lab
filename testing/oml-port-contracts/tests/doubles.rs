//! The contract suites proven against the in-memory doubles — the law the
//! real adapters (phase 3's `oml-pg` first) must pass identically.

use oml_app::doubles::{FixedClock, MarkerScanner, MemKeyring, MemSigner, MemStore, SeqIdGen};
use oml_port_contracts as contracts;

#[tokio::test]
async fn journal_store_double_passes_the_suite() {
    contracts::run_journal_store_suite(|| async { MemStore::new() }).await;
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
