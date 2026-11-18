//! Recall's cost on PostgreSQL when in-progress subtrees lie below the
//! scope (ADR-0080 d16, #402), in a binary of its own so no other test
//! shares the host while it times. A read at an ancestor of an in-progress
//! root reads every author's held rows inside it beside the promoted rows,
//! through a per-node rule-set mark in the held arm's author array. The
//! measurement times the same recall at `home` before and after three roots
//! below it, at depths one, two and three, are put in progress, over a
//! corpus where every row matches every query; the p50 and p95 of each side
//! are printed, never gated.

use std::sync::atomic::Ordering;
use std::time::Instant;

use oml_app::domain::MemoryKind;
use oml_app::domain::in_progress::WorkspaceMode;
use oml_app::domain::name::Name;
use oml_app::search::Pipeline;
use oml_pg::PgStore;
use oml_port_contracts::as_of::AsOfWorld;
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use sqlx::Connection;
use sqlx::postgres::PgConnection;
use uuid::Uuid;

/// A migrated store over a fresh database, as the serving role; `None`
/// where the cluster's role cannot mint the role pair.
async fn fresh_store() -> Option<(String, PgStore)> {
    let db = format!(
        "oml_ipt_{}_{}_{}",
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

/// A measurement, run by hand (`--ignored --nocapture`), never gated:
/// 100,000 rows, 50,000 promoted at `home`, 10,000 held in each of three
/// in-progress roots below it and 20,000 held at `home` by another key.
/// The copy alone runs past half an hour on the lane host, the insert
/// triggers' cost per row.
#[tokio::test]
#[ignore = "a measurement: 100,000 rows, over half an hour"]
async fn the_recall_cost_at_100k_rows_with_three_in_progress_roots_below_the_scope() {
    in_progress_timing(50_000, 10_000, 20_000).await;
}

/// The same measurement at a tenth of the size: 10,000 rows in the same
/// proportions.
#[tokio::test]
#[ignore = "a measurement: 10,000 rows, a few minutes"]
async fn the_recall_cost_at_10k_rows_with_three_in_progress_roots_below_the_scope() {
    in_progress_timing(5_000, 1_000, 2_000).await;
}

const QUERIES: [&str; 3] = ["deploy runbook", "runbook names", "deploy names"];

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

/// `runs` recalls at `home` by the reader, `include_unreviewed` absent,
/// after one warm one: their seconds and the held hits of the last.
async fn timed(w: &AsOfWorld<PgStore>, search: Pipeline, runs: usize) -> (Vec<f64>, usize) {
    let mut seconds = Vec::with_capacity(runs);
    let mut held = 0;
    for i in 0..=runs {
        let started = Instant::now();
        let answer = w
            .recall_at(
                &w.reader,
                w.home,
                QUERIES[i % QUERIES.len()],
                search,
                None,
                None,
            )
            .await
            .expect("recall");
        let elapsed = started.elapsed().as_secs_f64();
        assert!(!answer.hits.is_empty(), "the corpus matches every query");
        held = answer
            .hits
            .iter()
            .filter(|h| h.served.memory.status == oml_app::domain::MemoryStatus::Held)
            .count();
        if i > 0 {
            seconds.push(elapsed);
        }
    }
    (seconds, held)
}

async fn in_progress_timing(promoted: u32, per_root: u32, outside: u32) {
    let Some((db, store)) = fresh_store().await else {
        return;
    };
    let w = AsOfWorld::new(store, &["bm25", "bm25-postings"]).await;
    let node = |parent: Uuid, name: &'static str| {
        let w = &w;
        async move {
            w.app
                .create_workspace(
                    w.admin.as_str(),
                    &Name::parse(&format!("{name}-{}", run_tag())).expect("a name"),
                    Some(parent.into()),
                    None,
                )
                .await
                .expect("create a workspace")
                .workspace
                .workspace_id
        }
    };
    let one = node(w.home, "r1").await;
    let two = node(node(w.home, "d2").await, "r2").await;
    let three = node(node(node(w.home, "d3").await, "d3b").await, "r3").await;
    let roots = [one, two, three];
    let mut templates = vec![(
        w.promote(w.home, "runbook", "the deploy runbook names alpha")
            .await,
        promoted,
    )];
    for (i, &root) in roots.iter().enumerate() {
        let id = w
            .hold_kind(
                root,
                &format!("draft-{i}"),
                MemoryKind::Memory,
                "a deploy runbook draft names beta",
            )
            .await;
        templates.push((id, per_root));
    }
    templates.push((
        w.hold_kind(
            w.home,
            "elsewhere",
            MemoryKind::Memory,
            "a deploy runbook note names gamma",
        )
        .await,
        outside,
    ));
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
    for &(template, copies) in &templates {
        sqlx::query(&sql)
            .bind(w.tenant)
            .bind(template)
            .bind(i32::try_from(copies - 1).expect("copies fit an int4"))
            .execute(&mut owner)
            .await
            .expect("copy the template");
    }
    for table in ["memories", "search_postings", "search_term_rows"] {
        sqlx::query(&format!("ANALYZE {table}"))
            .execute(&mut owner)
            .await
            .expect("analyze");
    }
    drop(owner);
    let rows = promoted + 3 * per_root + outside;
    let runs = 20;
    let mut before = Vec::new();
    for search in [Pipeline::Bm25, Pipeline::Bm25Postings] {
        before.push(timed(&w, search, runs).await);
    }
    for root in roots {
        w.app
            .set_workspace_mode(
                w.reviewer.value.as_str(),
                root.into(),
                WorkspaceMode::InProgress,
            )
            .await
            .expect("put a root in progress");
    }
    for (search, (none, none_held)) in [Pipeline::Bm25, Pipeline::Bm25Postings]
        .into_iter()
        .zip(before)
    {
        let (three, held) = timed(&w, search, runs).await;
        println!(
            "{rows} rows, {search}: no root in progress p50 {:.1} ms p95 {:.1} ms \
             ({none_held} held hits); three roots below the scope p50 {:.1} ms p95 {:.1} ms \
             ({held} held hits)",
            quantile(none.clone(), 0.5) * 1e3,
            quantile(none, 0.95) * 1e3,
            quantile(three.clone(), 0.5) * 1e3,
            quantile(three, 0.95) * 1e3,
        );
    }
    drop_db(&db).await;
}
