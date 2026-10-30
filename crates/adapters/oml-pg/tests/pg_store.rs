//! The IDENTICAL port-contract suite against the real adapter (workspace-
//! plan.md M1 step 3), plus the adapter-specific laws: concurrent commits to
//! one tenant serialize, the journal refuses UPDATE/DELETE by mechanism
//! (ADR-0002 d4), the fork check verifies the stored head (ADR-0002 d7), and
//! grants and denials stay inside their tenant under row-level security.
//!
//! Each law runs against a throwaway database (`oml_test_<pid>_<n>`) created
//! through `DATABASE_URL` (the role needs CREATEDB), migrated by the embedded
//! migrator, and dropped best-effort afterwards.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use oml_app::domain::access::{Grant, KeyDenial};
use oml_app::domain::{
    Envelope, EventTime, KeySet, Role, Sha256Hash, event_types, sign, verify_chain,
};
use oml_app::ports::{
    CommitReceipt, CommitRequest, JournalStore, MetadataFold, OutboxRow, StoreError,
};
use oml_pg::{ForkCheckError, PgStore};
use oml_port_contracts::{
    ChainRig, key_record, run_journal_store_suite, test_signing_key, workarea_record,
};
use serde_json::json;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor, Row};
use uuid::Uuid;

fn base_url() -> String {
    // The gate's documented local profile; CI overrides with DATABASE_URL.
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

fn db_url(name: &str) -> String {
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{prefix}/{name}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

async fn fresh_store(created: &Mutex<Vec<String>>) -> PgStore {
    let name = format!(
        "oml_test_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst)
    );
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("CREATE DATABASE {name}").as_str())
        .await
        .expect("create throwaway database");
    created.lock().expect("names lock").push(name.clone());
    let store = PgStore::connect(&db_url(&name)).await.expect("connect");
    store.migrate().await.expect("embedded migrations apply");
    store
}

/// Best-effort cleanup; WITH (FORCE) evicts any pool connection still open.
async fn drop_dbs(created: &Mutex<Vec<String>>) {
    let names: Vec<String> = created.lock().expect("names lock").drain(..).collect();
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        for name in names {
            let _ = admin
                .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
                .await;
        }
    }
}

// ---------------------------------------------------------------------------
// The identical contract suite
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journal_store_contract_suite() {
    let created = Mutex::new(Vec::new());
    run_journal_store_suite(|| fresh_store(&created)).await;
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// Adapter-specific laws
// ---------------------------------------------------------------------------

static EVENT_N: AtomicU64 = AtomicU64::new(1);

/// Builds, signs (test key) and commits one event at an explicit
/// (seq, prev_hash) — what a concurrent writer does after reading the head.
async fn commit_at(
    store: &PgStore,
    tenant_id: Uuid,
    seq: u64,
    prev_hash: Sha256Hash,
    event_type: &str,
) -> Result<CommitReceipt, StoreError> {
    let event_id =
        Uuid::from_u128(0xE000_0000 + u128::from(EVENT_N.fetch_add(1, Ordering::SeqCst)));
    let envelope = Envelope {
        tenant_id,
        seq,
        event_id,
        event_type: event_type.to_owned(),
        prev_hash,
        principal: "race".to_owned(),
        api_key_id: None,
        at: EventTime::parse("2026-10-30T00:00:00.000000Z").expect("fixture time"),
        payload: json!({ "tenant_name": "race" }),
    };
    let event = sign(envelope, &test_signing_key()).expect("signing");
    store
        .commit(CommitRequest {
            event: event.clone(),
            fold: MetadataFold::TenantGenesis,
            outbox: OutboxRow {
                event_id,
                topic: event.envelope.event_type.clone(),
                payload: json!({ "seq": seq }),
            },
            idempotency: None,
        })
        .await
}

const COMMITS_PER_TASK: u64 = 20;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_commits_serialize_per_tenant() {
    let created = Mutex::new(Vec::new());
    let store = Arc::new(fresh_store(&created).await);
    let tenant_id = Uuid::from_u128(0xCAFE);
    commit_at(
        &store,
        tenant_id,
        1,
        Sha256Hash::ZERO,
        event_types::TENANT_GENESIS,
    )
    .await
    .expect("genesis");

    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            tokio::spawn(async move {
                let mut done = 0_u64;
                while done < COMMITS_PER_TASK {
                    let head = store
                        .chain_head(tenant_id)
                        .await
                        .expect("head")
                        .expect("head exists after genesis");
                    match commit_at(
                        &store,
                        tenant_id,
                        head.seq + 1,
                        head.hash,
                        event_types::SUBMISSION_REMEMBER,
                    )
                    .await
                    {
                        Ok(_) => done += 1,
                        // The loser of the race re-reads the head and retries:
                        // the advisory lock plus the typed conflict is the
                        // serialization story (ADR-0002 d3).
                        Err(
                            StoreError::SeqConflict { .. } | StoreError::PrevHashConflict { .. },
                        ) => {}
                        Err(e) => panic!("unexpected store error: {e}"),
                    }
                }
            })
        })
        .collect();
    for task in tasks {
        task.await.expect("task");
    }

    let total = 1 + 2 * COMMITS_PER_TASK;
    let events = store
        .events_range(tenant_id, 1, 10_000)
        .await
        .expect("range");
    let seqs: Vec<u64> = events.iter().map(|e| e.envelope.seq).collect();
    assert_eq!(
        seqs,
        (1..=total).collect::<Vec<u64>>(),
        "seq is strictly increasing with no gaps or duplicates"
    );
    let keys = KeySet::new(vec![test_signing_key().verifying_key()]);
    let head = store
        .chain_head(tenant_id)
        .await
        .expect("head")
        .expect("head");
    verify_chain(&events, &keys, Some(&head)).expect("the concurrently written chain verifies");
    drop_dbs(&created).await;
}

#[tokio::test]
async fn journal_refuses_update_and_delete() {
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut rig = ChainRig::new(90);
    rig.seed_chain(&store, 1).await;

    // ADR-0002 d4: append-only by mechanism — the guard trigger fires even
    // for the table owner inside a correctly tenant-scoped transaction.
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let err = sqlx::query("UPDATE journal SET principal = 'tampered' WHERE tenant_id = $1")
        .bind(rig.tenant_id)
        .execute(&mut *tx)
        .await
        .expect_err("UPDATE on journal is refused");
    assert!(err.to_string().contains("append-only"), "got: {err}");
    drop(tx); // aborted transaction rolls back

    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let err = sqlx::query("DELETE FROM journal WHERE tenant_id = $1")
        .bind(rig.tenant_id)
        .execute(&mut *tx)
        .await
        .expect_err("DELETE on journal is refused");
    assert!(err.to_string().contains("append-only"), "got: {err}");
    drop(tx);
    drop_dbs(&created).await;
}

/// Rows of `grants` and `key_denials` this session sees with `oml.tenant_id`
/// set to `tenant` (or unset). A role that bypasses row-level security — the
/// superuser CI's service container connects as — reads as
/// `pg_read_all_data` instead, which reads every table and which the
/// policies bind; the local owner role is bound by FORCE as it is.
async fn visible_access_rows(store: &PgStore, tenant: Option<Uuid>) -> (i64, i64) {
    let mut tx = store.pool().begin().await.expect("begin");
    let bypasses: bool =
        sqlx::query("SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user")
            .fetch_one(&mut *tx)
            .await
            .expect("role attributes")
            .get(0);
    if bypasses {
        sqlx::query("SET LOCAL ROLE pg_read_all_data")
            .execute(&mut *tx)
            .await
            .expect("a role the policies bind");
    }
    if let Some(tenant) = tenant {
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
    }
    let row =
        sqlx::query("SELECT (SELECT count(*) FROM grants), (SELECT count(*) FROM key_denials)")
            .fetch_one(&mut *tx)
            .await
            .expect("counts");
    let counts = (row.get(0), row.get(1));
    tx.rollback().await.expect("rollback");
    counts
}

/// `grants` and `key_denials` carry 0001's FORCE `tenant_isolation`
/// policy: under another tenant's `oml.tenant_id`, or none, neither table
/// shows a row, and `key_access` answers nothing across tenants
/// (postgres-schema.md row-level security; security.md).
#[tokio::test]
async fn grants_of_another_tenant_are_invisible_under_rls() {
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut a = ChainRig::new(92);
    a.seed_chain(&store, 0).await;
    let node = Uuid::from_u128(0xB92);
    a.create_workarea(&store, workarea_record(a.tenant_id, node, None, "universe"))
        .await;
    let key = key_record(a.tenant_id, Uuid::from_u128(0xD92), Role::Writer, None);
    a.issue_key(&store, key.clone()).await;
    a.issue_grant(
        &store,
        key.key_id,
        Grant {
            grant_id: Uuid::from_u128(0xF92),
            node,
            role: Role::Reader,
            deny: Vec::new(),
        },
    )
    .await
    .expect("grant");
    a.deny_key(
        &store,
        key.key_id,
        KeyDenial {
            denial_id: Uuid::from_u128(0xF93),
            node,
        },
        key.key_id,
    )
    .await
    .expect("denial");
    let mut b = ChainRig::new(93);
    b.seed_chain(&store, 0).await;

    assert_eq!(
        store.key_access(b.tenant_id, key.key_id).await,
        Ok(None),
        "another tenant's key_access answers nothing"
    );
    let flags: Vec<(String, bool, bool)> = sqlx::query(
        "SELECT relname::text, relrowsecurity, relforcerowsecurity FROM pg_class \
         WHERE relname IN ('grants', 'key_denials') ORDER BY relname",
    )
    .fetch_all(store.pool())
    .await
    .expect("table flags")
    .iter()
    .map(|r| (r.get(0), r.get(1), r.get(2)))
    .collect();
    assert_eq!(
        flags,
        vec![
            ("grants".to_owned(), true, true),
            ("key_denials".to_owned(), true, true),
        ],
        "row-level security is enabled and forced on both tables"
    );
    assert_eq!(
        visible_access_rows(&store, Some(a.tenant_id)).await,
        (1, 1),
        "the owning tenant sees its rows"
    );
    assert_eq!(
        visible_access_rows(&store, Some(b.tenant_id)).await,
        (0, 0),
        "another tenant sees none"
    );
    assert_eq!(
        visible_access_rows(&store, None).await,
        (0, 0),
        "no tenant context sees none"
    );
    drop_dbs(&created).await;
}

#[tokio::test]
async fn fork_check_verifies_and_detects() {
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut rig = ChainRig::new(91);
    rig.seed_chain(&store, 2).await; // seq 1..=3

    let keys = KeySet::new(vec![test_signing_key().verifying_key()]);
    let head = store
        .verify_stored_chain(rig.tenant_id, &keys)
        .await
        .expect("a clean stored chain verifies");
    assert_eq!(head.seq, 3);

    // Tamper the stored head (the head table carries no append-only
    // trigger); the fork check must now refuse — detection is the backstop
    // (ADR-0002 d7).
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("UPDATE journal_chain_head SET head_hash = $2 WHERE tenant_id = $1")
        .bind(rig.tenant_id)
        .bind(vec![0_u8; 32])
        .execute(&mut *tx)
        .await
        .expect("tamper the head");
    tx.commit().await.expect("commit tamper");

    let err = store
        .verify_stored_chain(rig.tenant_id, &keys)
        .await
        .expect_err("a forked head is refused");
    assert!(
        matches!(
            err,
            ForkCheckError::Verify(oml_app::domain::VerifyError::HeadMismatch)
        ),
        "got: {err:?}"
    );
    drop_dbs(&created).await;
}
