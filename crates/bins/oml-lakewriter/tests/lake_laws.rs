//! The lake writer's laws (ADR-0003, ADR-0058 d18), against a throwaway
//! PostgreSQL cluster at `DATABASE_URL` (a superuser, as the oml-pg suite
//! takes) and a lake under the target's temp directory:
//!
//! - no serving or client binary can reach the lake: the lead, the HTTP
//!   transport and the CLI depend on no lake crate (ADR-0003 d1, ADR-0013 d1);
//! - a drained lake holds every outbox event once and every promoted version
//!   sealed, and a replay from a reset frontier writes no row twice;
//! - a second writer is refused the lease, and two writers past the lease
//!   still write no row twice;
//! - an erased version's body opens under no key in any lake version, its
//!   text is in no lake file, and VACUUM removes its old ciphertext;
//! - each tenant's prefix holds only that tenant's rows;
//! - a crash between tables, or one that leaves an uncommitted data file,
//!   replays to the same lake;
//! - recall telemetry reaches the lake as counts without query text, for
//!   the 30 days Postgres keeps;
//! - bench results load once however often they are loaded, and every named
//!   query runs.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use deltalake::arrow::array::Array as _;
use oml_app::App;
use oml_app::domain::name::Name;
use oml_app::domain::{Envelope, EventTime, MemoryKind, Role, event_types, sign};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, SeqIdGen};
use oml_app::ports::{CommitRequest, JournalStore as _, MetadataFold, OutboxRow};
use oml_app::types::{BootstrapRequest, GrantRequest, IssueKeyRequest, SubmitRequest, Verdict};
use oml_lake::crypto::Kek;
use oml_lake::table::{BRONZE_EVENTS, GOLD_LINEAGE, LAKE_BATCHES, RECALL_DAILY, SILVER_CLAIMS};
use oml_lake::{Lake, Prefix};
use oml_lakewriter::source::{Lease, Source};
use oml_lakewriter::{Settings, Writer, load_bench};
use oml_pg::PgStore;
use serde_json::json;
use sqlx::postgres::PgConnection;
use sqlx::{Connection as _, Executor as _};
use uuid::Uuid;

type TestApp = App<PgStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

const PASSWORD: &str = "oml_test_lake";
static N: AtomicU64 = AtomicU64::new(0);

fn base_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

/// `db` at `DATABASE_URL`'s host as `role`.
fn url(db: &str, role: &str) -> String {
    let base = base_url().expect("DATABASE_URL");
    let (scheme, rest) = base.split_once("://").expect("a postgres URL");
    let (_, host_path) = rest.rsplit_once('@').expect("a user");
    let (host, _) = host_path.split_once('/').expect("a database");
    format!("{scheme}://{role}:{PASSWORD}@{host}/{db}")
}

/// A database migrated through its owner, with a serving role and a lake
/// role of its own granted by the writer's own grant step (ADR-0058 d13).
struct World {
    db: String,
    store: PgStore,
    app: TestApp,
    lake_url: String,
    lake_dir: PathBuf,
}

impl World {
    /// `None` when no superuser cluster is named: the law prints that it did
    /// not run, and fails under `OML_TEST_REQUIRE_SPLIT=1` as CI sets it.
    async fn new(tag: &str) -> Option<Self> {
        let Some(base) = base_url() else {
            assert!(
                std::env::var("OML_TEST_REQUIRE_SPLIT").as_deref() != Ok("1"),
                "DATABASE_URL is required"
            );
            println!("did not run: no DATABASE_URL");
            return None;
        };
        let db = format!(
            "oml_lake_{}_{}_{tag}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        );
        let mut admin = PgConnection::connect(&base).await.expect("admin");
        for role in ["owner", "serving", "lake"] {
            admin
                .execute(format!("CREATE ROLE {db}_{role} LOGIN PASSWORD '{PASSWORD}'").as_str())
                .await
                .expect("role");
        }
        admin
            .execute(format!("CREATE DATABASE {db} OWNER {db}_owner").as_str())
            .await
            .expect("database");
        let owner = url(&db, &format!("{db}_owner"));
        let store = PgStore::connect(&url(&db, &format!("{db}_serving")))
            .await
            .expect("serving")
            .with_owner_url(&owner);
        store.migrate(&owner).await.expect("migrations");
        Source::grant(&owner, &format!("{db}_lake"))
            .await
            .expect("lake grants");
        let app = App::new(
            store.clone(),
            MemSigner::new(oml_port_contracts::TEST_SEED),
            FixedClock::new(),
            SeqIdGen::new(),
            MarkerScanner::default(),
        );
        let lake_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(&db);
        let _ = std::fs::remove_dir_all(&lake_dir);
        Some(Self {
            lake_url: url(&db, &format!("{db}_lake")),
            db,
            store,
            app,
            lake_dir,
        })
    }

    fn lake(&self) -> Lake {
        Lake::at(self.lake_dir.to_str().expect("utf-8 path")).expect("lake")
    }

    async fn writer(&self) -> Writer {
        let src = Source::connect(&self.lake_url)
            .await
            .expect("lake role connects");
        Writer::new(src, self.lake(), kek(), Settings { max_batch: 10 })
    }

    async fn owner(&self) -> PgConnection {
        PgConnection::connect(&url(&self.db, &format!("{}_owner", self.db)))
            .await
            .expect("owner")
    }

    async fn drop(self) {
        drop(self.store);
        let _ = std::fs::remove_dir_all(&self.lake_dir);
        if let Some(base) = base_url()
            && let Ok(mut admin) = PgConnection::connect(&base).await
        {
            let db = &self.db;
            let _ = admin
                .execute(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)").as_str())
                .await;
            for role in ["owner", "serving", "lake"] {
                let _ = admin
                    .execute(format!("DROP ROLE IF EXISTS {db}_{role}").as_str())
                    .await;
            }
        }
    }
}

fn kek() -> Kek {
    Kek::from_file_bytes(&[0x42_u8; 32]).expect("kek")
}

/// A tenant with one workarea, a writer and a dreamer key, and `n`
/// promoted memories whose bodies carry `canary` and their index; every
/// third derives from the first. Returns (tenant, promoted ids).
async fn tenant(app: &TestApp, name: &str, canary: &str, n: usize) -> (Uuid, Vec<Uuid>) {
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse(name).expect("name"),
            admin_key_name: Name::parse(&format!("{name}-admin")).expect("name"),
            admin_principal: format!("{name}-admin"),
        })
        .await
        .expect("bootstrap");
    let admin = &boot.admin_key.value;
    let crew = app
        .create_workarea(
            admin,
            &Name::parse("crew").expect("name"),
            Some(boot.root_workarea_id.into()),
            None,
        )
        .await
        .expect("workarea")
        .workarea
        .workarea_id;
    let issue = |role: Role, principal: &str, key: &str| IssueKeyRequest {
        role,
        name: Name::parse(key).expect("name"),
        principal: principal.to_owned(),
        held_by: None,
        default_workarea: None,
        crew: None,
        grants: vec![GrantRequest {
            node: crew.into(),
            role,
            deny: Vec::new(),
            covers: oml_app::domain::access::Covers::Subtree,
        }],
        connect_code: false,
        user: None,
        guardrail_scope: None,
    };
    let writer = app
        .issue_key(
            admin,
            &issue(Role::Writer, &format!("{name}-wanda"), &format!("{name}-w")),
        )
        .await
        .expect("writer");
    let dreamer = app
        .issue_key(
            admin,
            &issue(Role::Dreamer, &format!("{name}-dana"), &format!("{name}-d")),
        )
        .await
        .expect("dreamer");
    let mut ids = Vec::new();
    for i in 0..n {
        let req = SubmitRequest {
            idempotency_key: format!("{name}-{i}"),
            claim_key: Some(format!("claim-{i}")),
            verb: None,
            kind: MemoryKind::Memory,
            body: format!("{canary} number {i}"),
            workarea: None,
            derives_from: if i % 3 == 2 { vec![ids[0]] } else { Vec::new() },
            severity: None,
            rule: None,
        };
        let id = app
            .submit_memory(&writer.value, &req)
            .await
            .expect("submit")
            .receipt
            .memory_id;
        app.post_verdict(&dreamer.value, id, Verdict::Approve, None)
            .await
            .expect("approve");
        ids.push(id);
    }
    (boot.tenant_id, ids)
}

/// Journals `lifecycle.erase` of `artifact` (erase has no use case yet,
/// ADR-0052 d7; the lake honours the event when it is journalled).
async fn erase(store: &PgStore, tenant: Uuid, artifact: Uuid) {
    let head = store
        .chain_head(tenant)
        .await
        .expect("head")
        .expect("a chain");
    let event_id = Uuid::new_v4();
    let envelope = Envelope {
        tenant_id: tenant,
        seq: head.seq + 1,
        event_id,
        event_type: event_types::LIFECYCLE_ERASE.to_owned(),
        prev_hash: head.hash,
        principal: "oml-server".to_owned(),
        api_key_id: None,
        at: EventTime::parse("2026-11-05T00:00:00.000000Z").expect("time"),
        payload: json!({ "artifact_id": artifact.to_string() }),
    };
    let event = sign(envelope, &oml_port_contracts::test_signing_key()).expect("sign");
    store
        .commit(CommitRequest {
            event: event.clone(),
            fold: MetadataFold::AdminRead,
            outbox: OutboxRow {
                event_id,
                topic: event.envelope.event_type.clone(),
                payload: json!({}),
            },
            idempotency: None,
        })
        .await
        .expect("erase commits");
}

async fn count(lake: &Lake, sql: &str) -> i64 {
    let batches = oml_lake::query::run(lake, sql).await.expect("query");
    let batch = batches.iter().find(|b| b.num_rows() > 0).expect("a row");
    batch
        .column(0)
        .as_any()
        .downcast_ref::<deltalake::arrow::array::Int64Array>()
        .expect("a count")
        .value(0)
}

async fn outbox_rows(w: &World, tenant: Uuid) -> i64 {
    let mut c = w.owner().await;
    let mut tx = c.begin().await.expect("tx");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let n = sqlx::query_scalar("SELECT count(*) FROM outbox WHERE tenant_id = $1")
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .expect("count");
    tx.rollback().await.expect("rollback");
    n
}

// ---------------------------------------------------------------------------

/// The normal-dependency closure of `package` in `cargo metadata`.
fn closure(meta: &serde_json::Value, package: &str) -> BTreeSet<String> {
    let nodes = meta["resolve"]["nodes"].as_array().expect("resolve");
    let name_of = |id: &str| id.to_owned();
    let root = meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|p| p["name"] == package)
        .and_then(|p| p["id"].as_str())
        .expect("package")
        .to_owned();
    let mut seen = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !seen.insert(name_of(&id)) {
            continue;
        }
        let node = nodes.iter().find(|n| n["id"] == id.as_str()).expect("node");
        for dep in node["deps"].as_array().into_iter().flatten() {
            let normal = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if normal {
                stack.push(dep["pkg"].as_str().expect("pkg").to_owned());
            }
        }
    }
    seen
}

#[test]
fn no_serving_or_client_binary_depends_on_a_lake_crate() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let reaches = |package: &str| {
        closure(&meta, package)
            .into_iter()
            .filter(|id| {
                ["oml-lake", "deltalake", "datafusion"]
                    .iter()
                    .any(|n| id.contains(&format!("#{n}@")) || id.contains(&format!("/{n}#")))
            })
            .collect::<Vec<String>>()
    };
    // The detector sees the one binary that does reach the lake.
    assert!(
        !reaches("oml-lakewriter").is_empty(),
        "the detector finds the lake writer's lake crates"
    );
    for package in [
        "oml-server-lead",
        "oml-transport-http",
        "oml-cli",
        "oml-app",
        "oml-pg",
    ] {
        let reached = reaches(package);
        assert!(
            reached.is_empty(),
            "{package} reaches the lake through {reached:?}"
        );
    }
    // And no source outside the two lake crates names the lake crate.
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut stack = vec![crates];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).expect("read") {
            let p = e.expect("entry").path();
            let s = p.to_string_lossy();
            if s.contains("oml-lake") {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs" || x == "toml") {
                let text = std::fs::read_to_string(&p).expect("source");
                assert!(
                    !text.contains("oml_lake")
                        && !text.contains("oml-lake")
                        && !text.contains("deltalake"),
                    "{} names the lake",
                    p.display()
                );
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_drained_lake_holds_every_event_once_and_a_replay_from_a_reset_frontier_writes_nothing_twice()
 {
    let Some(w) = World::new("replay").await else {
        return;
    };
    let (t, ids) = tenant(&w.app, "acme", "replay canary", 12).await;
    let mut writer = w.writer().await;
    let batches = writer.catch_up().await.expect("drain");
    assert!(batches.len() > 1, "a 10-seq batch cap cuts several batches");
    let lake = w.lake();
    let events = outbox_rows(&w, t).await;
    assert_eq!(count(&lake, "SELECT count(*) FROM events").await, events);
    assert_eq!(
        count(&lake, "SELECT count(DISTINCT outbox_seq) FROM events").await,
        events
    );
    assert_eq!(
        count(&lake, "SELECT count(*) FROM claims").await,
        ids.len() as i64
    );
    assert_eq!(
        count(
            &lake,
            "SELECT count(*) FROM claims WHERE state = 'promoted' AND body_ciphertext IS NOT NULL"
        )
        .await,
        ids.len() as i64
    );
    assert_eq!(
        count(
            &lake,
            "SELECT count(*) FROM lineage WHERE edge_label = 'derives_from'"
        )
        .await,
        4
    );
    assert!(writer.drain_once().await.expect("idle").is_none());

    // The frontier lost, as if the batches table never committed: every
    // table skips what its own transaction version already holds.
    std::fs::remove_dir_all(lake.dir(Prefix::Provider, &LAKE_BATCHES)).expect("reset");
    let mut again = w.writer().await;
    let replayed = again.catch_up().await.expect("replay");
    assert!(!replayed.is_empty());
    assert!(
        replayed.iter().all(|b| b.rows.values().all(|n| *n == 0)),
        "{replayed:?}"
    );
    assert_eq!(count(&lake, "SELECT count(*) FROM events").await, events);
    assert_eq!(
        count(&lake, "SELECT count(*) FROM claims").await,
        ids.len() as i64
    );

    // Every named query runs over it.
    for q in oml_lake::query::QUERIES {
        oml_lake::query::run(&lake, q.sql)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", q.name));
    }
    w.drop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_writer_is_refused_the_lease_and_two_writers_past_it_write_no_row_twice() {
    let Some(w) = World::new("fence").await else {
        return;
    };
    let first = Lease::try_acquire(&w.lake_url)
        .await
        .expect("connect")
        .expect("the lease");
    assert!(
        Lease::try_acquire(&w.lake_url)
            .await
            .expect("connect")
            .is_none(),
        "a second writer is refused while the first holds it"
    );
    assert!(w.writer().await.src.lease_held().await.expect("read"));
    let (t, ids) = tenant(&w.app, "acme", "fence canary", 8).await;
    // Two writers that both believe they hold the lease, racing.
    let (mut a, mut b) = (w.writer().await, w.writer().await);
    let (ra, rb) = tokio::join!(a.catch_up(), b.catch_up());
    // A loser may fail its commit on the transaction conflict; it never
    // writes a row twice, and a later drain completes the lake.
    let _ = (ra, rb);
    let mut c = w.writer().await;
    c.catch_up().await.expect("the survivor completes");
    let lake = w.lake();
    let events = outbox_rows(&w, t).await;
    assert_eq!(count(&lake, "SELECT count(*) FROM events").await, events);
    assert_eq!(
        count(&lake, "SELECT count(*) FROM claims").await,
        ids.len() as i64
    );
    first.release().await.expect("release");
    assert!(
        Lease::try_acquire(&w.lake_url)
            .await
            .expect("connect")
            .is_some()
    );
    w.drop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_erased_body_opens_in_no_lake_version_its_text_is_in_no_lake_file_and_vacuum_removes_its_ciphertext()
 {
    let Some(w) = World::new("erase").await else {
        return;
    };
    let canary = "erase-canary-7f3a";
    let (t, ids) = tenant(&w.app, "acme", canary, 3).await;
    let mut writer = w.writer().await;
    writer.catch_up().await.expect("drain");
    let lake = w.lake();
    let sealed_before = sealed(&lake, ids[0]).await.expect("a sealed body");
    let opened = writer
        .open_body(t, ids[0], &sealed_before)
        .await
        .expect("read");
    assert_eq!(
        opened.as_deref(),
        Some(format!("{canary} number 0").as_bytes())
    );

    erase(&w.store, t, ids[0]).await;
    writer.catch_up().await.expect("drain the erase");
    assert_eq!(
        count(&lake, &format!("SELECT count(*) FROM claims WHERE artifact_id = '{}' AND state = 'erased' AND body_ciphertext IS NULL", ids[0])).await,
        1
    );
    // The pre-erase ciphertext is still in an old table version until
    // VACUUM, and opens under no key: the key is destroyed.
    assert_eq!(
        writer
            .open_body(t, ids[0], &sealed_before)
            .await
            .expect("read"),
        None
    );
    let mut c = w.owner().await;
    let mut tx = c.begin().await.expect("tx");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .expect("t");
    let wrapped: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT wrapped_dek FROM artifact_keys WHERE tenant_id = $1 AND artifact_id = $2",
    )
    .bind(t)
    .bind(ids[0])
    .fetch_one(&mut *tx)
    .await
    .expect("key row");
    assert!(wrapped.is_none(), "the data key is destroyed");
    tx.rollback().await.expect("rollback");
    // The other versions still open.
    let other = sealed(&lake, ids[1]).await.expect("sealed");
    assert!(
        writer
            .open_body(t, ids[1], &other)
            .await
            .expect("read")
            .is_some()
    );
    // No lake file, data or log, holds the text.
    assert!(files_holding(&lake, canary.as_bytes()).is_empty());
    assert!(
        !files_holding(&lake, &sealed_before).is_empty(),
        "an old version still holds the ciphertext before VACUUM"
    );
    writer
        .maintain(chrono::Duration::zero())
        .await
        .expect("maintain");
    assert!(
        files_holding(&lake, &sealed_before).is_empty(),
        "VACUUM removed the old ciphertext"
    );
    // Maintenance kept the transaction versions: a replay still skips.
    std::fs::remove_dir_all(lake.dir(Prefix::Provider, &LAKE_BATCHES)).expect("reset");
    let replay = w.writer().await.catch_up().await.expect("replay");
    assert!(
        replay.iter().all(|b| b.rows.values().all(|n| *n == 0)),
        "{replay:?}"
    );
    w.drop().await;
}

async fn sealed(lake: &Lake, id: Uuid) -> Option<Vec<u8>> {
    let batches = oml_lake::query::run(
        lake,
        &format!(
            "SELECT arrow_cast(body_ciphertext, 'Binary') FROM claims WHERE artifact_id = '{id}'"
        ),
    )
    .await
    .expect("query");
    let batch = batches.iter().find(|b| b.num_rows() > 0)?;
    let arr = batch
        .column(0)
        .as_any()
        .downcast_ref::<deltalake::arrow::array::BinaryArray>()?;
    (!arr.is_null(0)).then(|| arr.value(0).to_vec())
}

fn files_holding(lake: &Lake, needle: &[u8]) -> Vec<PathBuf> {
    lake.files()
        .expect("files")
        .into_iter()
        .filter(|f| std::fs::read(f).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle)))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_tenant_prefix_holds_only_its_own_rows() {
    let Some(w) = World::new("isolation").await else {
        return;
    };
    let (a, _) = tenant(&w.app, "acme", "acme canary", 4).await;
    let (b, _) = tenant(&w.app, "bolt", "bolt canary", 3).await;
    w.writer().await.catch_up().await.expect("drain");
    let lake = w.lake();
    for (mine, theirs) in [(a, b), (b, a)] {
        for spec in [&BRONZE_EVENTS, &SILVER_CLAIMS, &GOLD_LINEAGE] {
            let Some(t) = lake.open(Prefix::Tenant(mine), spec).await.expect("open") else {
                continue;
            };
            let ctx = deltalake::datafusion::prelude::SessionContext::new();
            ctx.register_table("x", t.table_provider().await.expect("provider"))
                .expect("register");
            let rows = ctx
                .sql("SELECT DISTINCT arrow_cast(tenant_id, 'Utf8') FROM x")
                .await
                .expect("sql")
                .collect()
                .await
                .expect("rows");
            let rows: Vec<_> = rows.into_iter().filter(|b| b.num_rows() > 0).collect();
            let ids = rows[0]
                .column(0)
                .as_any()
                .downcast_ref::<deltalake::arrow::array::StringArray>()
                .expect("str");
            assert_eq!(ids.len(), 1);
            assert_eq!(ids.value(0), mine.to_string(), "{} under {mine}", spec.name);
        }
        let prefix = lake.root().join(mine.to_string());
        for f in lake
            .files()
            .expect("files")
            .into_iter()
            .filter(|f| f.starts_with(&prefix))
        {
            let bytes = std::fs::read(&f).expect("file");
            assert!(
                !bytes
                    .windows(36)
                    .any(|w| w == theirs.to_string().as_bytes()),
                "{} names the other tenant",
                f.display()
            );
        }
    }
    w.drop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crash_between_tables_or_with_an_uncommitted_data_file_replays_to_the_same_lake() {
    let Some(w) = World::new("crash").await else {
        return;
    };
    let (t, ids) = tenant(&w.app, "acme", "crash canary", 6).await;
    let mut writer = w.writer().await;
    writer.catch_up().await.expect("drain");
    let lake = w.lake();
    let events = outbox_rows(&w, t).await;
    // The crash: bronze committed, silver and lineage never did, nor the
    // frontier.
    for spec in [&SILVER_CLAIMS, &GOLD_LINEAGE] {
        std::fs::remove_dir_all(lake.dir(Prefix::Tenant(t), spec)).expect("drop table");
    }
    std::fs::remove_dir_all(lake.dir(Prefix::Provider, &LAKE_BATCHES)).expect("reset");
    // And a data file a crashed commit wrote but never logged.
    let bronze = lake.dir(Prefix::Tenant(t), &BRONZE_EVENTS);
    let data = lake
        .files()
        .expect("files")
        .into_iter()
        .find(|f| f.starts_with(&bronze) && f.extension().is_some_and(|x| x == "parquet"))
        .expect("a bronze data file");
    std::fs::copy(
        &data,
        data.with_file_name("part-00000-orphan-crash.snappy.parquet"),
    )
    .expect("orphan");

    let replay = w.writer().await.catch_up().await.expect("replay");
    let bronze_rows: usize = replay
        .iter()
        .map(|b| b.rows.get(BRONZE_EVENTS.name).copied().unwrap_or(0))
        .sum();
    assert_eq!(bronze_rows, 0, "bronze skips what it holds");
    assert_eq!(count(&lake, "SELECT count(*) FROM events").await, events);
    assert_eq!(
        count(&lake, "SELECT count(*) FROM claims").await,
        ids.len() as i64
    );
    assert_eq!(count(&lake, "SELECT count(*) FROM lineage").await, 2);
    w.drop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recall_counts_reach_the_lake_without_query_text_for_the_30_days_postgres_keeps() {
    let Some(w) = World::new("telemetry").await else {
        return;
    };
    let (t, ids) = tenant(&w.app, "acme", "telemetry canary", 2).await;
    let mut c = w.owner().await;
    let mut tx = c.begin().await.expect("tx");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .expect("t");
    sqlx::query(
        "UPDATE tenants SET telemetry_query_text_since = now() - interval '90 days' WHERE id = $1",
    )
    .bind(t)
    .execute(&mut *tx)
    .await
    .expect("opt in");
    let secret = "opted-in query text 9c1e";
    for (age_days, pipeline, hits) in [
        (0, "bm25", 2),
        (1, "bm25", 0),
        (1, "sparse", 1),
        (45, "bm25", 1),
    ] {
        let hit_ids: Vec<Uuid> = ids.iter().take(hits).copied().collect();
        let scores: Vec<f32> = hit_ids.iter().map(|_| 1.0).collect();
        sqlx::query(
            "INSERT INTO recall_events (tenant_id, recall_id, created_at, key_id, principal, \
             scope_workarea_id, pipeline, access, query_digest, query_text, hit_ids, hit_scores) \
             SELECT $1, gen_random_uuid(), now() - make_interval(days => $2), id, 'p', \
                    (SELECT id FROM workareas WHERE tenant_id = $1 LIMIT 1), $3, 'postings', \
                    sha256('q'::bytea), $4, $5, $6 FROM api_keys WHERE tenant_id = $1 LIMIT 1",
        )
        .bind(t)
        .bind(age_days)
        .bind(pipeline)
        .bind(secret)
        .bind(&hit_ids)
        .bind(&scores)
        .execute(&mut *tx)
        .await
        .expect("recall event");
    }
    let with_text: i64 =
        sqlx::query_scalar("SELECT count(*) FROM recall_events WHERE query_text IS NOT NULL")
            .fetch_one(&mut *tx)
            .await
            .expect("count");
    assert!(with_text >= 3, "the opt-in kept the text in Postgres");
    tx.commit().await.expect("commit");

    let writer = w.writer().await;
    writer.refresh_telemetry().await.expect("refresh");
    let lake = w.lake();
    assert_eq!(
        count(&lake, "SELECT sum(recalls) FROM recall_daily").await,
        3,
        "the 45-day-old recall is not kept"
    );
    assert_eq!(
        count(&lake, "SELECT sum(empty_recalls) FROM recall_daily").await,
        1
    );
    assert_eq!(
        count(
            &lake,
            "SELECT count(*) FROM recall_daily WHERE pipeline = 'sparse'"
        )
        .await,
        1
    );
    assert!(
        files_holding(&lake, secret.as_bytes()).is_empty(),
        "no lake file holds query text"
    );
    assert!(
        lake.open(Prefix::Tenant(t), &RECALL_DAILY)
            .await
            .expect("open")
            .is_some()
    );
    w.drop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_results_load_once_however_often_they_are_loaded() {
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("bench_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lake = Lake::at(dir.to_str().expect("utf-8")).expect("lake");
    let results = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../eval/bench/results");
    let (files, rows) = load_bench(&lake, std::slice::from_ref(&results))
        .await
        .expect("load");
    assert!(files > 10 && rows > 100, "{files} files, {rows} rows");
    load_bench(&lake, &[results]).await.expect("again");
    assert_eq!(
        count(&lake, "SELECT count(*) FROM bench").await,
        rows as i64
    );
    assert!(
        oml_lake::query::rows(
            &oml_lake::query::run(
                &lake,
                oml_lake::query::named("bench-latest").expect("q").sql
            )
            .await
            .expect("run")
        ) > 0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drain_throughput_is_measured() {
    let Some(w) = World::new("throughput").await else {
        return;
    };
    let n = std::env::var("OML_LAKE_THROUGHPUT_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let started = std::time::Instant::now();
    let (t, _) = tenant(&w.app, "acme", "throughput", n).await;
    let wrote = started.elapsed();
    let events = outbox_rows(&w, t).await;
    let src = Source::connect(&w.lake_url).await.expect("connect");
    let mut writer = Writer::new(src, w.lake(), kek(), Settings::default());
    let lag = writer.lag_seconds().await.expect("lag");
    let started = std::time::Instant::now();
    writer.catch_up().await.expect("drain");
    let drained = started.elapsed();
    println!(
        "throughput: {events} events ({n} promoted memories) written in {:.1} s; lag before drain {lag} s; drained in {:.2} s = {:.0} events/s",
        wrote.as_secs_f64(),
        drained.as_secs_f64(),
        events as f64 / drained.as_secs_f64()
    );
    assert_eq!(writer.lag_seconds().await.expect("lag"), 0);
    w.drop().await;
}
