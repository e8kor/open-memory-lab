//! Test 7 of federated recall's phase 1 (ADR-0072 d6, d13) on PostgreSQL,
//! in a binary of its own so no other test shares the host while it times:
//! an unreadable workarea of many rows matching every query moves neither
//! p50 nor p95 of a wide recall.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use oml_pg::PgStore;
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use sqlx::Connection;
use sqlx::postgres::PgConnection;

/// A migrated store over a fresh database, as the serving role; `None`
/// where the cluster's role cannot mint the role pair.
async fn fresh_store() -> Option<(String, PgStore)> {
    let db = format!(
        "oml_timing_{}_{}_{}",
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

/// Test 7 on PostgreSQL at CI's size: an unreadable workarea of 5,000 rows
/// matching every query moves neither p50 nor p95 of a wide recall
/// (ADR-0072 d6, d13).
#[tokio::test]
async fn federated_recall_timing_does_not_depend_on_an_unreadable_workarea() {
    unreadable_rows_move_no_timing(5_000).await;
}

/// Test 7 at the record's size, run by hand (`--ignored --nocapture`):
/// 100,000 rows, copied through the postings and similar-key triggers in
/// several minutes (ADR-0072 d13).
#[tokio::test]
#[ignore = "a measurement: 100,000 rows, several minutes"]
async fn federated_recall_timing_does_not_depend_on_100k_unreadable_rows() {
    unreadable_rows_move_no_timing(100_000).await;
}

/// An unreadable workarea of `copies` rows matching every query, copied in
/// by SQL as the owner, moves neither p50 nor p95 of a wide recall, on
/// `bm25-postings` and on `bm25`.
async fn unreadable_rows_move_no_timing(copies: u32) {
    use oml_port_contracts::federation::{assert_within_noise, timing_sample, timing_world};
    let Some((db, store)) = fresh_store().await else {
        return;
    };
    let t = timing_world(store).await;
    let mut without = BTreeMap::new();
    for search in ["bm25-postings", "bm25"] {
        without.insert(search, timing_sample(&t, search, 40).await);
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
        "claim_key" => "claim_key || '-copy-' || g".to_owned(),
        // Each copy's own text, so no two copies are near copies of each
        // other for the similar-key trigger, every one holding every query
        // word.
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
        .bind(t.world.tenant.to_string())
        .execute(&mut owner)
        .await
        .expect("tenant");
    for template in t.templates {
        sqlx::query(&sql)
            .bind(t.world.tenant)
            .bind(template)
            .bind(i32::try_from(copies).expect("copies fit an int4"))
            .execute(&mut owner)
            .await
            .expect("copy the template into W");
    }
    println!("copied {copies} rows into each unreadable workarea");
    for table in ["memories", "search_postings", "search_term_rows"] {
        sqlx::query(&format!("ANALYZE {table}"))
            .execute(&mut owner)
            .await
            .expect("analyze");
    }
    drop(owner);
    for search in ["bm25-postings", "bm25"] {
        let with = timing_sample(&t, search, 40).await;
        assert_within_noise(without.remove(search).expect("sampled"), with);
    }
    drop_db(&db).await;
}
