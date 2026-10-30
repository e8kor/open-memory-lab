//! The IDENTICAL port-contract suite against the real adapter (workspace-
//! plan.md M1 step 3), plus the adapter-specific laws: concurrent commits to
//! one tenant serialize, the journal refuses UPDATE/DELETE by mechanism
//! (ADR-0002 d4), the fork check verifies the stored head (ADR-0002 d7),
//! grants and denials stay inside their tenant under row-level security, the
//! served-tip index holds one tip per claim, and 0005 migrates the PoC's
//! superseded rows to versions and tips (ADR-0025 d10).
//!
//! Each law runs against a throwaway database (`oml_test_<pid>_<n>`) created
//! through `DATABASE_URL` (the role needs CREATEDB), migrated by the embedded
//! migrator, and dropped best-effort afterwards.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use oml_app::domain::access::{Grant, KeyDenial};
use oml_app::domain::{
    Envelope, EventTime, KeySet, MemoryKind, MemoryStatus, Role, Sha256Hash, event_types, sign,
    verify_chain,
};
use oml_app::ports::{
    CommitReceipt, CommitRequest, JournalStore, MetadataFold, OutboxRow, StoreError,
};
use oml_pg::{ForkCheckError, MIGRATOR, PgStore};
use oml_port_contracts::{
    ChainRig, held_memory, key_record, run_journal_store_suite, test_signing_key, workarea_record,
};
use serde_json::{Value, json};
use sqlx::migrate::Migrate as _;
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

/// A throwaway database with no migration applied; its name.
async fn fresh_database(created: &Mutex<Vec<String>>) -> String {
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
    name
}

async fn fresh_store(created: &Mutex<Vec<String>>) -> PgStore {
    let name = fresh_database(created).await;
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

// ---------------------------------------------------------------------------
// Claim versions and tips (migrations/0005_claim_versions.sql)
// ---------------------------------------------------------------------------

/// One PoC journal row as the pre-0005 schema stores it: the migration reads
/// only its type, payload and `at`, so the hash and signature are filler.
async fn poc_event(
    conn: &mut PgConnection,
    tenant: Uuid,
    seq: i64,
    event_type: &str,
    payload: Value,
    at: &str,
) -> Uuid {
    let event_id = Uuid::from_u128(0xE500_0000 + u128::try_from(seq).expect("seq"));
    sqlx::query(
        "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, at, \
         payload, signed_bytes, signed_bytes_hash, sig) \
         VALUES ($1, $2, $3, $4, $5, 'poc', ($6)::timestamptz, $7, $8, $9, $10)",
    )
    .bind(tenant)
    .bind(seq)
    .bind(event_id)
    .bind(event_type)
    .bind(vec![0_u8; 32])
    .bind(at)
    .bind(payload)
    .bind(b"{}".to_vec())
    .bind(vec![0_u8; 32])
    .bind(vec![0_u8; 64])
    .execute(&mut *conn)
    .await
    .expect("journal row");
    event_id
}

/// One PoC `memories` row in the pre-0005 shape: a status that may be
/// 'superseded', and `superseded_by` naming the successor.
#[allow(clippy::too_many_arguments, reason = "one column per argument")]
async fn poc_memory(
    conn: &mut PgConnection,
    tenant: Uuid,
    workarea: Uuid,
    id: Uuid,
    claim_key: &str,
    claim: &str,
    status: &str,
    superseded_by: Option<Uuid>,
    created_event_id: Uuid,
    created_at: &str,
) {
    sqlx::query(
        "INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, \
         created_event_id, superseded_by, created_at, claim_key, author_principal, \
         author_key_id, lineage_principals, lineage_key_ids) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, ($9)::timestamptz, $10, 'poc/agent', $11, \
         ARRAY['poc/agent'], ARRAY[$11]::uuid[])",
    )
    .bind(id)
    .bind(tenant)
    .bind(workarea)
    .bind(claim)
    .bind(Sha256Hash::digest(claim.as_bytes()).0.to_vec())
    .bind(status)
    .bind(created_event_id)
    .bind(superseded_by)
    .bind(created_at)
    .bind(claim_key)
    .bind(Uuid::from_u128(0xD500))
    .execute(&mut *conn)
    .await
    .expect("memories row");
}

/// The PoC flipped the tip when a supersession was proposed and wrote the
/// replaced version 'superseded' (or, later, promoted with `superseded_by`
/// set). 0005 moves the tip back to where ratification left it: a version
/// whose successor never reached promotion is the served tip again; one
/// whose successor was promoted stays promoted and is history. Versions are
/// numbered per claim_key in capture order, `superseded_by` becomes the
/// successor's `replaces`, and every row gains its kind and NFC byte count
/// (ADR-0025 d6, d10).
#[tokio::test]
async fn a_poc_superseded_row_whose_successor_is_held_migrates_back_to_the_served_tip() {
    let created = Mutex::new(Vec::new());
    let name = fresh_database(&created).await;
    let mut conn = PgConnection::connect(&db_url(&name))
        .await
        .expect("connect");
    conn.ensure_migrations_table()
        .await
        .expect("migrations table");
    for migration in MIGRATOR.iter().filter(|m| m.version < 5) {
        conn.apply(migration).await.expect("0001-0004 apply");
    }

    let tenant = Uuid::from_u128(0xA500);
    let wa = Uuid::from_u128(0xB500);
    let (quote, quote_next) = (Uuid::from_u128(0xC500), Uuid::from_u128(0xC501));
    let (note, note_next) = (Uuid::from_u128(0xC502), Uuid::from_u128(0xC503));
    // The quote's first version was stored before capture normalized: its
    // byte count is taken from the NFC form.
    let decomposed = "Accra Metals quoted 412 USD/t at the cafe\u{301}";
    let composed = "Accra Metals quoted 412 USD/t at the caf\u{e9}";

    let mut tx = conn.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'poc')")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let promote = |id: Uuid| json!({ "artifact_id": id.to_string() });
    let e1 = poc_event(
        &mut tx,
        tenant,
        1,
        "submission.remember",
        json!({}),
        "2026-10-01T10:00:00Z",
    )
    .await;
    poc_event(
        &mut tx,
        tenant,
        2,
        "lifecycle.promote",
        promote(quote),
        "2026-10-01T10:00:02.5Z",
    )
    .await;
    let e3 = poc_event(
        &mut tx,
        tenant,
        3,
        "lifecycle.supersede",
        json!({}),
        "2026-10-01T10:00:03Z",
    )
    .await;
    let e4 = poc_event(
        &mut tx,
        tenant,
        4,
        "submission.remember",
        json!({}),
        "2026-10-01T10:00:04Z",
    )
    .await;
    poc_event(
        &mut tx,
        tenant,
        5,
        "lifecycle.promote",
        promote(note),
        "2026-10-01T10:00:05Z",
    )
    .await;
    let e6 = poc_event(
        &mut tx,
        tenant,
        6,
        "lifecycle.supersede",
        json!({}),
        "2026-10-01T10:00:06Z",
    )
    .await;
    poc_event(
        &mut tx,
        tenant,
        7,
        "lifecycle.promote",
        promote(note_next),
        "2026-10-01T10:00:07Z",
    )
    .await;
    // A successor is inserted before the version whose superseded_by names
    // it. The quote's successor is still held; it was written 'superseded'
    // by the first PoC. The note's successor was ratified; the later PoC
    // left the replaced version 'promoted' with superseded_by set.
    poc_memory(
        &mut tx,
        tenant,
        wa,
        quote_next,
        "quote",
        "Accra Metals revised to 408 USD/t",
        "held",
        None,
        e3,
        "2026-10-01T10:00:03Z",
    )
    .await;
    poc_memory(
        &mut tx,
        tenant,
        wa,
        quote,
        "quote",
        decomposed,
        "superseded",
        Some(quote_next),
        e1,
        "2026-10-01T10:00:01Z",
    )
    .await;
    poc_memory(
        &mut tx,
        tenant,
        wa,
        note_next,
        "note",
        "the canary note, settled",
        "promoted",
        None,
        e6,
        "2026-10-01T10:00:06Z",
    )
    .await;
    poc_memory(
        &mut tx,
        tenant,
        wa,
        note,
        "note",
        "the canary note",
        "promoted",
        Some(note_next),
        e4,
        "2026-10-01T10:00:04Z",
    )
    .await;
    // A replayable PoC submit receipt, stored before receipts named their
    // version.
    sqlx::query(
        "INSERT INTO idempotency (tenant_id, key, request_digest, response, committed_at) \
         VALUES ($1, 'poc-receipt', $2, $3, now())",
    )
    .bind(tenant)
    .bind(vec![0_u8])
    .bind(json!({
        "memory_id": quote, "claim_key": "quote", "workarea_id": wa, "state": "held",
        "journal_seq": 1, "body_digest": "sha256:00",
    }))
    .execute(&mut *tx)
    .await
    .expect("idempotency row");
    tx.commit().await.expect("commit the PoC rows");
    drop(conn);

    let store = PgStore::connect(&db_url(&name)).await.expect("connect");
    store.migrate().await.expect("0005 applies over PoC rows");
    let read = |id| {
        let store = store.clone();
        async move {
            store
                .memory(tenant, id)
                .await
                .expect("memory")
                .expect("the row survives the migration")
        }
    };
    let at = |s: &str| Some(EventTime::parse(s).expect("fixed-width time"));

    let q1 = read(quote).await;
    assert_eq!(q1.status, MemoryStatus::Promoted, "no superseded state");
    assert!(q1.is_tip, "its successor never reached promotion");
    assert_eq!((q1.claim_key.as_str(), q1.version), ("quote", 1));
    assert_eq!(q1.replaces, None);
    assert_eq!(q1.promoted_at, at("2026-10-01T10:00:02.500000Z"));
    assert_eq!(
        q1.kind,
        MemoryKind::Memory,
        "the PoC served every row as memory"
    );
    assert_eq!(q1.body_bytes, composed.len() as u64, "the NFC byte count");
    assert_eq!(q1.body, decomposed, "the stored text is not rewritten");
    assert!(!q1.retirement);

    let q2 = read(quote_next).await;
    assert_eq!(q2.status, MemoryStatus::Held);
    assert!(!q2.is_tip);
    assert_eq!(q2.version, 2);
    assert_eq!(q2.replaces, Some(quote));
    assert_eq!(q2.promoted_at, None);

    let n1 = read(note).await;
    assert_eq!(n1.status, MemoryStatus::Promoted);
    assert!(!n1.is_tip, "its successor was ratified");
    assert_eq!(n1.version, 1);
    assert_eq!(n1.promoted_at, at("2026-10-01T10:00:05.000000Z"));
    let n2 = read(note_next).await;
    assert!(n2.is_tip);
    assert_eq!(n2.version, 2);
    assert_eq!(n2.replaces, Some(note));
    assert_eq!(n2.promoted_at, at("2026-10-01T10:00:07.000000Z"));

    // Recall serves each claim's tip and nothing else.
    let hits = store
        .recall_fts(tenant, &[wa], "Accra", 10)
        .await
        .expect("recall");
    assert_eq!(
        hits.iter().map(|m| m.memory_id).collect::<Vec<_>>(),
        vec![quote]
    );
    let hits = store
        .recall_fts(tenant, &[wa], "canary", 10)
        .await
        .expect("recall");
    assert_eq!(
        hits.iter().map(|m| m.memory_id).collect::<Vec<_>>(),
        vec![note_next]
    );

    // superseded_by is gone, and the stored receipt names its version.
    let column: Option<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_name = 'memories' AND column_name = 'superseded_by'",
    )
    .fetch_optional(store.pool())
    .await
    .expect("catalog read");
    assert_eq!(column, None);
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let response: Value =
        sqlx::query_scalar("SELECT response FROM idempotency WHERE key = 'poc-receipt'")
            .fetch_one(&mut *tx)
            .await
            .expect("receipt");
    tx.rollback().await.expect("rollback");
    assert_eq!(response["version"], 1, "a PoC submit was a version 1");
    drop_dbs(&created).await;
}

/// The served-tip index is the schema's own refusal: a second promoted tip
/// of one claim_key in one workarea fails at the index whatever wrote it,
/// while a replaced version and another workarea's tip stand beside it
/// (postgres-schema.md `memories_served_tip`).
#[tokio::test]
async fn the_served_tip_index_refuses_a_second_tip() {
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut rig = ChainRig::new(95);
    rig.seed_chain(&store, 0).await;
    let wa = Uuid::from_u128(0xB95);
    let author = Uuid::from_u128(0xD95);
    let tip = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0xC95),
        "wanda",
        author,
        "tip",
    );
    let history = oml_app::types::MemoryRecord {
        memory_id: Uuid::from_u128(0xC96),
        version: 2,
        body: "history".to_owned(),
        ..tip.clone()
    };
    let elsewhere = oml_app::types::MemoryRecord {
        memory_id: Uuid::from_u128(0xC97),
        workarea_id: Uuid::from_u128(0xB96),
        ..tip.clone()
    };
    // Held through the store, then promoted by hand, so that only the
    // index stands between the rows and a second tip.
    for memory in [tip.clone(), history.clone(), elsewhere.clone()] {
        rig.commit(
            &store,
            event_types::SUBMISSION_REMEMBER,
            json!({ "artifact_id": memory.memory_id.to_string() }),
            MetadataFold::MemoryHeld {
                memory,
                enqueue: false,
            },
            None,
        )
        .await
        .expect("hold");
    }
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let set_tip = "UPDATE memories SET status = 'promoted', is_tip = $2 WHERE id = $1";
    for (id, is_tip) in [
        (tip.memory_id, true),
        (history.memory_id, false),
        (elsewhere.memory_id, true),
    ] {
        sqlx::query(set_tip)
            .bind(id)
            .bind(is_tip)
            .execute(&mut *tx)
            .await
            .expect("a replaced version and another workarea's tip stand");
    }
    let err = sqlx::query(set_tip)
        .bind(history.memory_id)
        .bind(true)
        .execute(&mut *tx)
        .await
        .expect_err("a second tip of one claim is refused");
    assert!(
        err.to_string().contains("memories_served_tip"),
        "got: {err}"
    );
    drop(tx);
    drop_dbs(&created).await;
}
