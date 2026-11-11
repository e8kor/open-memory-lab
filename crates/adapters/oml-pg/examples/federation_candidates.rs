//! The candidate-count report federated recall's phase 1 reads first
//! (ADR-0072, "Who gains"; #205): for every live key of every tenant, how
//! many trusted workareas accepting federated reads it reads, |C| before a
//! scope sets any aside, computed by the application exactly as a recall
//! computes it. Prints one JSON document: the distribution and how many keys
//! fall past `max_sources` (8) and past 1,000.
//!
//! ```sh
//! cargo run --release -p oml-pg --example federation_candidates -- <url>
//! ```
//!
//! `<url>` names a role that reads every tenant's `api_keys`, such as the
//! owner. The report only reads: it opens no transaction that writes, and
//! it prints counts, never a key, a name or a workarea.

use std::collections::BTreeMap;

use oml_app::App;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, SeqIdGen};
use oml_pg::PgStore;
use serde_json::json;
use uuid::Uuid;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .ok_or("usage: federation_candidates <url>")?;
    let store = PgStore::connect(&url).await?;
    let keys: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT tenant_id, id FROM api_keys WHERE revoked_at IS NULL ORDER BY tenant_id, id",
    )
    .fetch_all(store.pool())
    .await?;
    // The signer never signs: the report commits nothing.
    let app = App::new(
        store,
        MemSigner::new([0; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let mut counts = Vec::with_capacity(keys.len());
    for (tenant, key) in &keys {
        if let Some(n) = app.federation_candidate_count(*tenant, *key).await? {
            counts.push(n);
        }
    }
    counts.sort_unstable();
    let at = |q: f64| -> usize {
        if counts.is_empty() {
            return 0;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "an index into a short list"
        )]
        let i = ((counts.len() as f64) * q).ceil() as usize;
        counts[i.saturating_sub(1).min(counts.len() - 1)]
    };
    let mut buckets: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &counts {
        let bucket = match n {
            0 => "0",
            1 => "1",
            2..=8 => "2-8",
            9..=64 => "9-64",
            65..=1_000 => "65-1000",
            _ => "over 1000",
        };
        *buckets.entry(bucket).or_default() += 1;
    }
    println!(
        "{}",
        json!({
            "keys": counts.len(),
            "p50": at(0.5),
            "p90": at(0.9),
            "p99": at(0.99),
            "max": counts.last().copied().unwrap_or(0),
            "at_most_1": counts.iter().filter(|n| **n <= 1).count(),
            "over_max_sources_8": counts.iter().filter(|n| **n > 8).count(),
            "over_1000": counts.iter().filter(|n| **n > 1_000).count(),
            "buckets": buckets,
        })
    );
    Ok(())
}
