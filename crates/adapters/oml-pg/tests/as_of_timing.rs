//! The as-of recall's cost on PostgreSQL (ADR-0059 d18, d20, d21), in a
//! binary of its own so no other test shares the host while it times. Over
//! a workarea where every claim has a replaced version, a recall at an
//! instant reads twice the rows today's reads, the replaced versions beside
//! the tips, each replaced one probing `memories_tip_moves`: at CI's size its
//! p50 and p95 stay within three times today's on the same pipeline, plus 10
//! ms of noise, and at 100,000 rows within five times (measured 3.5 and 4.2).

use std::sync::atomic::Ordering;
use std::time::Instant;

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

/// At CI's size: 5,000 copies of a two-version chain, 10,000 rows.
#[tokio::test]
async fn an_as_of_recall_keeps_its_p95_near_todays() {
    as_of_timing(5_000, 3.0).await;
}

/// At the record's size, run by hand (`--ignored --nocapture`): 50,000
/// copies of a two-version chain, 100,000 rows, every one matching every
/// query. Measured 2026-11-11 on the shared host under load: today p50 490
/// ms, p95 541 ms; at an instant p50 1,736 ms, p95 2,284 ms. The bound is
/// five times today's: the correlated probe runs once per replaced row in
/// each of `bm25`'s two scope reads, the cost ADR-0059 d20 states.
#[tokio::test]
#[ignore = "a measurement: 100,000 rows, several minutes"]
async fn an_as_of_recall_keeps_its_p95_near_todays_at_100k_rows() {
    as_of_timing(50_000, 5.0).await;
}

const QUERIES: [&str; 3] = ["deploy runbook", "runbook names", "deploy names"];

async fn sample(
    w: &AsOfWorld<PgStore>,
    search: Pipeline,
    as_of: Option<&oml_app::domain::EventTime>,
    runs: usize,
) -> Vec<f64> {
    let mut out = Vec::with_capacity(runs);
    for i in 0..=runs {
        let started = Instant::now();
        let answer = w
            .recall(&w.reader, QUERIES[i % QUERIES.len()], search, as_of, false)
            .await
            .expect("recall");
        let elapsed = started.elapsed().as_secs_f64();
        assert!(!answer.hits.is_empty(), "the corpus matches every query");
        if i > 0 {
            out.push(elapsed);
        }
    }
    out
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

/// `copies` copies of a claim and its ratified successor, copied in by SQL
/// as the owner, each copy its own claim: a recall at an instant after
/// every promotion serves the successors, and its p50 and p95 stay within
/// `factor` times today's on `bm25`, plus 10 ms; `bm25-postings` at the
/// instant, read by the scan, is printed beside today's postings read.
async fn as_of_timing(copies: u32, factor: f64) {
    let Some((db, store)) = fresh_store().await else {
        return;
    };
    let w = AsOfWorld::new(store, &["bm25", "bm25-postings"]).await;
    let v1 = w
        .promote(w.home, "runbook", "the deploy runbook names alpha")
        .await;
    let v2 = w
        .supersede(v1, 1, "the deploy runbook names beta", false)
        .await;
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
        // One chain per copy: both versions keep their ordinal under the
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
    for template in [v1, v2] {
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
    let today = sample(&w, Pipeline::Bm25, None, runs).await;
    let then = sample(&w, Pipeline::Bm25, Some(&at), runs).await;
    let postings_today = sample(&w, Pipeline::Bm25Postings, None, runs).await;
    let postings_then = sample(&w, Pipeline::Bm25Postings, Some(&at), runs).await;
    for (label, v) in [
        ("bm25 today", &today),
        ("bm25 as of", &then),
        ("bm25-postings today", &postings_today),
        ("bm25-postings as of (the scan)", &postings_then),
    ] {
        println!(
            "{} rows, {label}: p50 {:.1} ms, p95 {:.1} ms",
            2 * copies,
            quantile(v.clone(), 0.5) * 1e3,
            quantile(v.clone(), 0.95) * 1e3
        );
    }
    for q in [0.5, 0.95] {
        let (a, b) = (quantile(today.clone(), q), quantile(then.clone(), q));
        assert!(
            b <= a * factor + 0.010,
            "the as-of recall's p{} is {b}s against today's {a}s",
            (q * 100.0).round()
        );
    }
    drop_db(&db).await;
}
