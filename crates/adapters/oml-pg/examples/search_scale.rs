//! The scale lane's served-path driver for text search (`bench/scale/`;
//! ADR-0052 FR-100): runs each query of a workload through
//! [`PgStore::retrieve`], as the lead's recall does, as the serving role
//! under row security, and prints one JSON line per call with its wall
//! time, how it read its list and what it pruned. The statements timed are
//! the adapter's own, so nothing here restates them.
//!
//! ```sh
//! cargo run --release -p oml-pg --example search_scale -- \
//!     <serving url> <workload.json> <retriever> [frequent_share posting_budget]
//! ```
//!
//! The workload is a JSON array of `{"tenant", "nodes", "query", "k",
//! "own_keys", "own_nodes", "match"}`; `retriever` is `bm25` or
//! `bm25_postings`. Each query runs twice, and the second run is timed, so
//! the numbers are warm.

use std::time::Instant;

use oml_app::domain::MemoryKind;
use oml_app::ports::Retriever as _;
use oml_app::search::{CandidateScope, MatchMode, OwnHeldScope, RetrievalQuery, RetrieverKind};
use oml_pg::{PgStore, PruneLimits};
use serde_json::{Value, json};
use uuid::Uuid;

fn uuids(v: &Value) -> Vec<Uuid> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, url, workload, retriever, rest @ ..] = args.as_slice() else {
        return Err("usage: search_scale <url> <workload.json> <retriever> [share budget]".into());
    };
    let kind = match retriever.as_str() {
        "bm25" => RetrieverKind::Bm25,
        "bm25_postings" => RetrieverKind::Bm25Postings,
        other => return Err(format!("unknown retriever {other}").into()),
    };
    let limits = match rest {
        [share, budget] => PruneLimits {
            frequent_share: share.parse()?,
            posting_budget: budget.parse()?,
        },
        _ => PruneLimits::SERVED,
    };
    let store = PgStore::connect(url).await?.with_prune_limits(limits);
    let items: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(workload)?)?;
    for (i, item) in items.iter().enumerate() {
        let tenant = Uuid::parse_str(item["tenant"].as_str().unwrap_or_default())?;
        let own_keys = uuids(&item["own_keys"]);
        let scope = CandidateScope {
            promoted_nodes: uuids(&item["nodes"]),
            kinds: vec![MemoryKind::Memory, MemoryKind::DerivedFact],
            own_held: (!own_keys.is_empty()).then(|| OwnHeldScope {
                author_key_ids: own_keys,
                nodes: uuids(&item["own_nodes"]),
            }),
        };
        let query = RetrievalQuery {
            query: item["query"].as_str().unwrap_or_default(),
            match_mode: if item["match"].as_str() == Some("all") {
                MatchMode::All
            } else {
                MatchMode::Any
            },
            k: item["k"]
                .as_u64()
                .map_or(10, |k| usize::try_from(k).unwrap_or(10)),
            sparse: &[],
        };
        store.retrieve(tenant, kind, &scope, query).await?;
        let started = Instant::now();
        let list = store.retrieve(tenant, kind, &scope, query).await?;
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{}",
            json!({
                "i": i,
                "ms": ms,
                "access": list.access.as_str(),
                "pruned": list.pruned,
                "ids": list.candidates.iter().map(|c| c.memory_id.to_string()).collect::<Vec<_>>(),
                "scores": list.candidates.iter().map(|c| c.score).collect::<Vec<_>>(),
            })
        );
    }
    Ok(())
}
