//! The as-of recall's cost on PostgreSQL (ADR-0059 d18, d20, d21), in a
//! binary of its own so no other test shares the host while it times. Over
//! a workarea where every row matches every query and every claim is a
//! chain of `k` versions, a recall at an instant reads all `k` versions of
//! each claim where today's reads the tip alone, and each replaced version
//! probes `memories_tip_moves` once in each of `bm25`'s two scope reads, so
//! the cost grows with the chain. Each pair runs a recall of today and one
//! at the instant back to back, so a burst of load on the host moves both;
//! CI gates only the median of the pairs' ratios, at CI's size, and the p50
//! and p95 of each side are printed, never gated.

use std::sync::atomic::Ordering;
use std::time::Instant;

use oml_app::domain::EventTime;
use oml_app::search::Pipeline;
use oml_pg::PgStore;
use oml_port_contracts::as_of::AsOfWorld;
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use sqlx::Connection;
use sqlx::postgres::PgConnection;

/// A migrated store over a fresh database, as the serving role; `None`
/// where the cluster's role cannot mint the role pair.
async fn fresh_store() -> Option<(String, PgStore)> {
    let db = format!(
        "oml_asof_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db).await {
        return None;
    }
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    store
        .migrate(&owner_url(&db))
        .await
        .expect("embedded migrations apply");
    Some((db, store))
}

/// CI's gate: 5,000 copies of a two-version chain, 10,000 rows; the median
/// of 30 paired ratios stays within three times. Measured 2.0 on CI
/// (ADR-0059 d20).
#[tokio::test]
async fn an_as_of_recall_costs_at_most_three_times_todays_over_two_version_chains() {
    as_of_timing(5_000, 2, Some(3.0)).await;
}

/// A measurement, run by hand (`--ignored --nocapture`), never gated:
/// 50,000 copies of a two-version chain, 100,000 rows.
#[tokio::test]
#[ignore = "a measurement: 100,000 rows, about half an hour"]
async fn the_as_of_recall_cost_at_100k_rows() {
    as_of_timing(50_000, 2, None).await;
}

/// A measurement, run by hand (`--ignored --nocapture`), never gated: 4,000
/// copies of a five-version chain, 20,000 rows, 4,000 tips today.
#[tokio::test]
#[ignore = "a measurement: five-version chains, several minutes"]
async fn the_as_of_recall_cost_over_five_version_chains() {
    as_of_timing(4_000, 5, None).await;
}

const QUERIES: [&str; 3] = ["deploy runbook", "runbook names", "deploy names"];

/// One recall's seconds.
async fn timed(
    w: &AsOfWorld<PgStore>,
    search: Pipeline,
    as_of: Option<&EventTime>,
    i: usize,
) -> f64 {
    let started = Instant::now();
    let answer = w
        .recall(&w.reader, QUERIES[i % QUERIES.len()], search, as_of, false)
        .await
        .expect("recall");
    let elapsed = started.elapsed().as_secs_f64();
    assert!(!answer.hits.is_empty(), "the corpus matches every query");
    elapsed
}

/// `runs` pairs, each a recall of today then one at `at`, after one warm
/// pair: (today's seconds, the instant's seconds).
async fn paired(
    w: &AsOfWorld<PgStore>,
    search: Pipeline,
    at: &EventTime,
    runs: usize,
) -> (Vec<f64>, Vec<f64>) {
    let (mut today, mut then) = (Vec::with_capacity(runs), Vec::with_capacity(runs));
    for i in 0..=runs {
        let a = timed(w, search, None, i).await;
        let b = timed(w, search, Some(at), i).await;
        if i > 0 {
            today.push(a);
            then.push(b);
        }
    }
    (today, then)
}

fn quantile(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a sample of a few dozen"
    )]
    let i = ((v.len() as f64) * q).ceil() as usize;
    v[i.saturating_sub(1).min(v.len() - 1)]
}

/// `copies` copies of a claim of `k` ratified versions, copied in by SQL as
/// the owner, each copy its own claim. A recall at an instant after every
/// promotion is paired with today's on `bm25` and on `bm25-postings`, which
/// reads the scan at the instant; with `gate`, the median of `bm25`'s pair
/// ratios is at most `gate`.
async fn as_of_timing(copies: u32, k: u32, gate: Option<f64>) {
    let Some((db, store)) = fresh_store().await else {
        return;
    };
    let w = AsOfWorld::new(store, &["bm25", "bm25-postings"]).await;
    let mut chain = vec![
        w.promote(w.home, "runbook", "the deploy runbook names version one")
            .await,
    ];
    for v in 1..k {
        let next = w
            .supersede(
                *chain.last().expect("a version"),
                v,
                &format!("the deploy runbook names version {}", v + 1),
                false,
            )
            .await;
        chain.push(next);
    }
    let mut owner = PgConnection::connect(&owner_url(&db))
        .await
        .expect("owner connect");
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_name = 'memories' AND table_schema = 'public' \
           AND is_generated = 'NEVER' ORDER BY ordinal_position",
    )
    .fetch_all(&mut owner)
    .await
    .expect("columns");
    let names: Vec<&str> = columns
        .iter()
        .map(String::as_str)
        .filter(|c| !matches!(*c, "search_vector" | "search_len"))
        .collect();
    let pick = |c: &str| match c {
        "id" => "gen_random_uuid()".to_owned(),
        // One chain per copy: every version keeps its ordinal under the
        // copy's own claim key.
        "claim_key" => "claim_key || '-copy-' || g".to_owned(),
        "claim" => "claim || ' copy ' || g".to_owned(),
        other => other.to_owned(),
    };
    let sql = format!(
        "INSERT INTO memories ({}) SELECT {} FROM memories, generate_series(1, $3) g \
         WHERE tenant_id = $1 AND id = $2",
        names.join(", "),
        names.iter().map(|c| pick(c)).collect::<Vec<_>>().join(", "),
    );
    sqlx::query("SELECT set_config('oml.tenant_id', $1, false)")
        .bind(w.tenant.to_string())
        .execute(&mut owner)
        .await
        .expect("tenant");
    for &template in &chain {
        sqlx::query(&sql)
            .bind(w.tenant)
            .bind(template)
            .bind(i32::try_from(copies).expect("copies fit an int4"))
            .execute(&mut owner)
            .await
            .expect("copy the chain");
    }
    for table in ["memories", "search_postings", "search_term_rows"] {
        sqlx::query(&format!("ANALYZE {table}"))
            .execute(&mut owner)
            .await
            .expect("analyze");
    }
    drop(owner);
    let at = w.instant();
    let runs = 30;
    let rows = copies * k;
    let mut ratio = 0.0;
    for search in [Pipeline::Bm25, Pipeline::Bm25Postings] {
        let (today, then) = paired(&w, search, &at, runs).await;
        let ratios: Vec<f64> = today.iter().zip(&then).map(|(a, b)| b / a).collect();
        let median = quantile(ratios, 0.5);
        println!(
            "{rows} rows, chains of {k}, {search}: today p50 {:.1} ms p95 {:.1} ms; \
             at the instant p50 {:.1} ms p95 {:.1} ms; median pair ratio {median:.2}",
            quantile(today.clone(), 0.5) * 1e3,
            quantile(today, 0.95) * 1e3,
            quantile(then.clone(), 0.5) * 1e3,
            quantile(then, 0.95) * 1e3,
        );
        if search == Pipeline::Bm25 {
            ratio = median;
        }
    }
    if let Some(gate) = gate {
        assert!(
            ratio <= gate,
            "the median as-of to today ratio on bm25 is {ratio:.2}, above {gate}"
        );
    }
    drop_db(&db).await;
}
