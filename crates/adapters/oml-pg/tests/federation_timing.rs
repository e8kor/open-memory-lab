//! Test 7 of federated recall's phase 1 (ADR-0072 d6, d13) on PostgreSQL,
//! in a binary of its own: an unreadable workarea of many rows matching
//! every query changes no wide answer (gating), and moves neither p50 nor
//! p95 of a wide recall (a measurement, `--ignored`: wall-clock quantiles
//! move with the host's load, #278).

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

/// Test 7 on PostgreSQL at CI's size, its deterministic half: an
/// unreadable workarea of 5,000 rows matching every query, analyzed,
/// changes no `wide` answer on `bm25-postings` or `bm25`, byte for byte,
/// the read's access path included (ADR-0072 d6, d13; #278).
#[tokio::test]
async fn federated_recall_answers_do_not_depend_on_an_unreadable_workarea() {
    unreadable_rows_move_no_answer(5_000).await;
}

/// Test 7 on PostgreSQL at CI's size, its timing half, a measurement run
/// by hand or by the weekly bench (`--ignored`): an unreadable workarea of
/// 5,000 rows matching every query moves neither p50 nor p95 of a wide
/// recall (ADR-0072 d6, d13). Wall-clock quantiles move with the host's
/// load, so it gates no change (#278).
#[tokio::test]
#[ignore = "a measurement: wall-clock quantiles, run by the weekly bench"]
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
    for search in SEARCHES {
        without.insert(search, timing_sample(&t, search, 40).await);
    }
    copy_into_w(&db, &t, copies).await;
    for search in SEARCHES {
        let with = timing_sample(&t, search, 40).await;
        assert_within_noise(without.remove(search).expect("sampled"), with);
    }
    drop_db(&db).await;
}

/// The same copies change no `wide` answer on `bm25-postings` or `bm25`.
async fn unreadable_rows_move_no_answer(copies: u32) {
    use oml_port_contracts::federation::{answer_sample, timing_world};
    let Some((db, store)) = fresh_store().await else {
        return;
    };
    let t = timing_world(store).await;
    let mut without = BTreeMap::new();
    for search in SEARCHES {
        without.insert(search, answer_sample(&t, search).await);
    }
    copy_into_w(&db, &t, copies).await;
    for search in SEARCHES {
        assert_eq!(
            answer_sample(&t, search).await,
            without.remove(search).expect("sampled"),
            "{search}: the unreadable rows moved a wide answer"
        );
    }
    drop_db(&db).await;
}

/// The pipelines test 7 reads under.
const SEARCHES: [&str; 2] = ["bm25-postings", "bm25"];

/// Copies each of `t`'s templates `copies` times into its own unreadable
/// workarea, by SQL as the owner through the postings and similar-key
/// triggers, then analyzes the tables a recall reads.
async fn copy_into_w(
    db: &str,
    t: &oml_port_contracts::federation::TimingWorld<PgStore>,
    copies: u32,
) {
    let mut owner = PgConnection::connect(&owner_url(db))
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
}

/// The lead's resident set in bytes, and its peak, from `/proc/self/status`.
fn rss() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|kb| kb.parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    };
    (field("VmRSS:"), field("VmHWM:"))
}

/// The widen signal's build on a realistic tenant (#244, review F9), by
/// hand (`--ignored --nocapture`, release): `OML_WIDEN_ROWS` promoted rows
/// (100,000 by default) over 50 committed workareas a key reads, each row
/// 30 words drawn from a 50,000-word vocabulary with a Zipf-like skew
/// (word `floor(50,000 · u³)`), copied by SQL as the owner through the
/// postings triggers. Prints the build's time, the signal's counts and
/// estimated bytes, the process's resident set before and after, and the
/// signal's read on a `scope` recall.
#[tokio::test]
#[ignore = "a measurement: 100,000 rows, several minutes"]
#[allow(clippy::cast_precision_loss, reason = "a printed measurement")]
async fn the_widen_signal_on_a_tenant_of_100k_rows() {
    use oml_app::app::WidenLimits;
    use oml_app::domain::Role;
    use oml_app::federation::{CountsScope, Sources};
    use oml_app::ports::JournalStore as _;
    use oml_port_contracts::federation::{FedWorld, quantile_of};
    let rows: u32 = std::env::var("OML_WIDEN_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    let Some((db, store)) = fresh_store().await else {
        return;
    };
    let limits = WidenLimits {
        held_pairs: usize::MAX,
        tenant_pairs: usize::MAX,
        ..WidenLimits::default()
    };
    let mut w = FedWorld::with_limits(store, limits).await;
    w.promote("home", "kiwi orchard notes").await;
    w.commit("home").await;
    w.create("corpus", "universe").await;
    let mut templates = Vec::new();
    for i in 0..50 {
        let node: &'static str = Box::leak(format!("corpus-{i}").into_boxed_str());
        w.create(node, "corpus").await;
        templates.push(w.promote(node, "w1 w2 w3").await);
        w.commit(node).await;
    }
    let key = w.key(Role::Reader, &["crew", "corpus"], &[]).await;
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
        "claim" => "(SELECT string_agg('w' || floor(50000 * power(random(), 3))::int, ' ') \
                    FROM generate_series(1, 30) x WHERE g > 0)"
            .to_owned(),
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
    let started = std::time::Instant::now();
    for template in &templates {
        sqlx::query(&sql)
            .bind(w.tenant)
            .bind(*template)
            .bind(i32::try_from(rows / 50).expect("rows fit an int4"))
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
    println!(
        "loaded {rows} rows over 50 workareas in {:?}",
        started.elapsed()
    );
    // The build, as the lead runs it after the recall that asks for it.
    let _ = w
        .recall(
            &key,
            "w1 w7",
            "home",
            Some(Sources::Scope),
            "bm25-postings",
            10,
        )
        .await;
    let (before, _) = rss();
    let started = std::time::Instant::now();
    assert_eq!(w.app.refresh_widen_signals().await, 1, "the signal built");
    let build = started.elapsed();
    let (after, peak) = rss();
    // The same vocabulary read alone, for its size.
    let nodes: Vec<uuid::Uuid> = w
        .store
        .workarea_subtree(w.tenant, w.node("corpus"))
        .await
        .expect("the corpus")
        .into_iter()
        .map(|n| n.workarea_id)
        .collect();
    let vocabulary = w
        .store
        .vocabulary_counts(
            w.tenant,
            &CountsScope {
                nodes,
                kinds: vec![
                    oml_app::domain::MemoryKind::Memory,
                    oml_app::domain::MemoryKind::DerivedFact,
                ],
                expired_at: Some(i64::MAX / 2),
                prunes: false,
            },
            usize::MAX,
        )
        .await
        .expect("the vocabulary")
        .expect("no cap");
    println!(
        "the signal's build: {:.0} ms; {} (form, node) counts, about {:.1} MB; resident set {:.1} MB before, {:.1} MB after, peak {:.1} MB",
        build.as_secs_f64() * 1e3,
        vocabulary.pairs(),
        vocabulary.bytes() as f64 / 1e6,
        before as f64 / 1e6,
        after as f64 / 1e6,
        peak as f64 / 1e6,
    );
    let mut reads = Vec::new();
    for i in 0..41 {
        let started = std::time::Instant::now();
        let answer = w
            .recall(
                &key,
                ["w1 w7", "w12 w400", "w3 w9000 w2"][i % 3],
                "home",
                Some(Sources::Scope),
                "bm25-postings",
                10,
            )
            .await
            .expect("a scope recall");
        if i > 0 {
            reads.push(started.elapsed().as_secs_f64());
        }
        if i == 1 {
            println!("the hint on {:?}: {:?}", "w12 w400", answer.widen);
        }
    }
    println!(
        "scope with the signal held: p50 {:.1} ms, p95 {:.1} ms",
        quantile_of(reads.clone(), 0.5) * 1e3,
        quantile_of(reads, 0.95) * 1e3
    );
    drop_db(&db).await;
}
