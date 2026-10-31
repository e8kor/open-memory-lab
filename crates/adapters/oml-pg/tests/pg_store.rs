//! The IDENTICAL port-contract suite against the real adapter (workspace-
//! plan.md M1 step 3), plus the adapter-specific laws: concurrent commits to
//! one tenant serialize, the journal refuses UPDATE/DELETE by mechanism
//! (ADR-0002 d4), the fork check verifies the stored head (ADR-0002 d7),
//! grants and denials stay inside their tenant under row-level security, the
//! served-tip index holds one tip per claim, 0005 migrates the PoC's
//! superseded rows to versions and tips and refuses two live first versions
//! of one claim_key (ADR-0025 d10, d18), and 0006's names hold: the
//! `oml_name` domain checks the reference grammar, operator keys share one
//! namespace, the root's `universe` is the tenant's alone, paths derive from
//! names, and a PoC database keeps every row and event (ADR-0026 d1, d2, d6,
//! d7, d11).
//!
//! Each law runs against a throwaway database (`oml_test_<pid>_<n>`) created
//! through `DATABASE_URL` (the role needs CREATEDB), migrated by the embedded
//! migrator, and dropped best-effort afterwards.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use oml_app::domain::access::{Grant, KeyDenial};
use oml_app::domain::name::NamedKind;
use oml_app::domain::{
    Envelope, EventDraft, EventTime, KeySet, MemoryKind, MemoryStatus, Role, Sha256Hash,
    SignedEnvelope, event_types, next_envelope, sign, verify_chain,
};
use oml_app::ports::{
    CommitReceipt, CommitRequest, JournalStore, MetadataFold, OutboxRow, StoreError,
};
use oml_app::types::KeyRecord;
use oml_pg::{ForkCheckError, MIGRATOR, PgStore};
use oml_port_contracts::{
    ChainRig, held_memory, key_record, name, run_journal_store_suite, test_signing_key,
    workarea_record,
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
            fold: MetadataFold::TenantGenesis { name: name("race") },
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

/// A throwaway database at the pre-0005 schema, 0001–0004 applied; its name
/// and a connection to it for seeding PoC rows.
async fn poc_schema(created: &Mutex<Vec<String>>) -> (String, PgConnection) {
    schema_through(created, 4).await
}

/// A throwaway database with the migrations up to `version` applied and no
/// later one; its name and a connection to it for seeding rows.
async fn schema_through(created: &Mutex<Vec<String>>, version: i64) -> (String, PgConnection) {
    let name = fresh_database(created).await;
    let mut conn = PgConnection::connect(&db_url(&name))
        .await
        .expect("connect");
    conn.ensure_migrations_table()
        .await
        .expect("migrations table");
    for migration in MIGRATOR.iter().filter(|m| m.version <= version) {
        conn.apply(migration)
            .await
            .expect("the earlier migrations apply");
    }
    (name, conn)
}

/// Binds the transaction to `tenant` and stores the tenant row.
async fn poc_tenant(conn: &mut PgConnection, tenant: Uuid) {
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *conn)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'poc')")
        .bind(tenant)
        .execute(&mut *conn)
        .await
        .expect("tenant");
}

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
    let (name, mut conn) = poc_schema(&created).await;

    let tenant = Uuid::from_u128(0xA500);
    let wa = Uuid::from_u128(0xB500);
    let (quote, quote_next) = (Uuid::from_u128(0xC500), Uuid::from_u128(0xC501));
    let (note, note_next) = (Uuid::from_u128(0xC502), Uuid::from_u128(0xC503));
    // The quote's first version was stored before capture normalized: its
    // byte count is taken from the NFC form.
    let decomposed = "Accra Metals quoted 412 USD/t at the cafe\u{301}";
    let composed = "Accra Metals quoted 412 USD/t at the caf\u{e9}";

    let mut tx = conn.begin().await.expect("begin");
    poc_tenant(&mut tx, tenant).await;
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
        .recall_fts(tenant, &[wa], "Accra", &[MemoryKind::Memory], 10)
        .await
        .expect("recall");
    assert_eq!(
        hits.iter().map(|c| c.memory_id).collect::<Vec<_>>(),
        vec![quote]
    );
    let hits = store
        .recall_fts(tenant, &[wa], "canary", &[MemoryKind::Memory], 10)
        .await
        .expect("recall");
    assert_eq!(
        hits.iter().map(|c| c.memory_id).collect::<Vec<_>>(),
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

/// The PoC never refused a second claim under a claim_key already in use,
/// so its rows can hold two live first versions of one claim_key in one
/// workarea: a held one beside a promoted one, or two held ones. d18 never
/// lets them stand together, and approving the held one would meet the
/// served tip at promotion as a constraint error, so 0005 refuses them by
/// name and migrates nothing (ADR-0025 d18).
#[tokio::test]
async fn a_poc_held_first_version_beside_a_live_one_refuses_the_migration_by_name() {
    let created = Mutex::new(Vec::new());
    for first in ["promoted", "held"] {
        let (name, mut conn) = poc_schema(&created).await;
        let tenant = Uuid::from_u128(0xA510);
        let wa = Uuid::from_u128(0xB510);
        let mut tx = conn.begin().await.expect("begin");
        poc_tenant(&mut tx, tenant).await;
        let e1 = poc_event(
            &mut tx,
            tenant,
            1,
            "submission.remember",
            json!({}),
            "2026-10-01T10:00:00Z",
        )
        .await;
        let e2 = poc_event(
            &mut tx,
            tenant,
            2,
            "submission.remember",
            json!({}),
            "2026-10-01T10:00:01Z",
        )
        .await;
        poc_memory(
            &mut tx,
            tenant,
            wa,
            Uuid::from_u128(0xC510),
            "deploy-runbook",
            "roll back before the canary",
            first,
            None,
            e1,
            "2026-10-01T10:00:00Z",
        )
        .await;
        poc_memory(
            &mut tx,
            tenant,
            wa,
            Uuid::from_u128(0xC511),
            "deploy-runbook",
            "drain the canary, then roll back",
            "held",
            None,
            e2,
            "2026-10-01T10:00:01Z",
        )
        .await;
        tx.commit().await.expect("commit the PoC rows");
        drop(conn);

        let store = PgStore::connect(&db_url(&name)).await.expect("connect");
        let err = store
            .migrate()
            .await
            .expect_err("a held first version beside a live one is refused");
        assert!(
            err.to_string()
                .contains("2 live first versions of claim_key deploy-runbook"),
            "beside a {first} first version, got: {err}"
        );
        let applied: Option<i64> =
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE version = 5")
                .fetch_optional(store.pool())
                .await
                .expect("migration ledger read");
        assert_eq!(applied, None, "0005 migrates nothing it refuses");
    }
    drop_dbs(&created).await;
}

/// A first version rejected without ever being promoted released its
/// claim_key, so the PoC claim captured after it under that key is a fresh
/// version 1 and the served tip, and that claim's successor is 1 + the
/// greatest version before it, whatever its state (audit-and-lineage.md
/// supersession chains; ADR-0025 d10, d18).
#[tokio::test]
async fn a_poc_claim_captured_after_a_rejected_first_version_migrates_as_a_fresh_version_one() {
    let created = Mutex::new(Vec::new());
    let (name, mut conn) = poc_schema(&created).await;
    let tenant = Uuid::from_u128(0xA520);
    let wa = Uuid::from_u128(0xB520);
    let (rejected, fresh, next) = (
        Uuid::from_u128(0xC520),
        Uuid::from_u128(0xC521),
        Uuid::from_u128(0xC522),
    );
    let mut tx = conn.begin().await.expect("begin");
    poc_tenant(&mut tx, tenant).await;
    let mut events = Vec::new();
    for (seq, at) in [
        "2026-10-01T10:00:00Z",
        "2026-10-01T10:00:01Z",
        "2026-10-01T10:00:02Z",
    ]
    .into_iter()
    .enumerate()
    {
        let seq = i64::try_from(seq).expect("seq") + 1;
        events.push((
            poc_event(&mut tx, tenant, seq, "submission.remember", json!({}), at).await,
            at,
        ));
    }
    // The successor is inserted before the version whose superseded_by
    // names it.
    for (id, claim, status, superseded_by, (event, at)) in [
        (
            rejected,
            "roll back before the canary",
            "rejected",
            None,
            events[0],
        ),
        (
            next,
            "drain the canary within 5 minutes, then roll back",
            "held",
            None,
            events[2],
        ),
        (
            fresh,
            "drain the canary, then roll back",
            "superseded",
            Some(next),
            events[1],
        ),
    ] {
        poc_memory(
            &mut tx,
            tenant,
            wa,
            id,
            "deploy-runbook",
            claim,
            status,
            superseded_by,
            event,
            at,
        )
        .await;
    }
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
    let r = read(rejected).await;
    assert_eq!(
        (r.status, r.version, r.is_tip),
        (MemoryStatus::Rejected, 1, false)
    );
    let f = read(fresh).await;
    assert_eq!(
        (f.status, f.version, f.replaces),
        (MemoryStatus::Promoted, 1, None),
        "the released key starts a fresh version 1"
    );
    assert!(f.is_tip, "its successor never reached promotion");
    let n = read(next).await;
    assert_eq!(
        (n.status, n.version, n.replaces),
        (MemoryStatus::Held, 2, Some(fresh))
    );
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

// ---------------------------------------------------------------------------
// Names (migrations/0006_names.sql)
// ---------------------------------------------------------------------------

/// The `oml_name` domain every named column carries checks the reference
/// grammar: a lowercase RFC 1035 label of 1–63 characters that reads as no
/// id, the derived form admitted, since the fold of the lead's naming writes
/// it (postgres-schema.md names and tenants; ADR-0026 d2).
#[tokio::test]
async fn the_oml_name_domain_admits_the_reference_grammar_and_nothing_else() {
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let longest = format!("a{}", "b".repeat(62));
    for value in [
        "universe",
        "metals-crew",
        "q1-2027",
        "a",
        "key-0190f2a47c3e4b1d9a6fe2d8c51b3a07",
        longest.as_str(),
    ] {
        let stored: String = sqlx::query_scalar("SELECT ($1::oml_name)::text")
            .bind(value)
            .fetch_one(store.pool())
            .await
            .unwrap_or_else(|e| panic!("{value:?} is a name: {e}"));
        assert_eq!(stored, value, "stored byte for byte");
    }
    let too_long = format!("a{}", "b".repeat(63));
    for value in [
        "",
        "Zulu",
        "metals_crew",
        "universe.procurement",
        "crew-procure/agent-7",
        "-metals",
        "metals-",
        "2027-q1",
        "étude",
        "0190f2a4-7c3e-4b1d-9a6f-e2d8c51b3a07",
        "abcdef01-2345-6789-abcd-ef0123456789",
        "deadbeefdeadbeefdeadbeefdeadbeef",
        too_long.as_str(),
    ] {
        let err = sqlx::query("SELECT $1::oml_name")
            .bind(value)
            .execute(store.pool())
            .await
            .expect_err("refused, never coerced");
        assert!(err.to_string().contains("oml_name"), "{value:?}: {err}");
    }
    drop_dbs(&created).await;
}

/// Operator keys carry no tenant and share the provider's one namespace:
/// the `api_keys` name index takes their NULL tenant as one value, so a
/// second operator key of a name is refused, while a tenant's key of that
/// name stands beside it and keys with no name never collide
/// (postgres-schema.md `api_keys`; ADR-0026 d1).
#[tokio::test]
async fn two_operator_keys_cannot_share_a_name_though_their_tenant_is_null() {
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut rig = ChainRig::new(96);
    rig.seed_chain(&store, 0).await;
    let tenant_key = KeyRecord {
        name: Some(name("root-operator")),
        ..key_record(rig.tenant_id, Uuid::from_u128(0xD960), Role::Admin, None)
    };
    rig.issue_key(&store, tenant_key).await;

    // Provider-level rows are the operator path's, which is not built; the
    // owner writes them here by lifting FORCE inside a transaction that
    // rolls back (postgres-schema.md row-level security).
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("ALTER TABLE api_keys NO FORCE ROW LEVEL SECURITY")
        .execute(&mut *tx)
        .await
        .expect("the owner lifts FORCE");
    let insert = "INSERT INTO api_keys (id, tenant_id, hash, role, principal, issued_seq, name) \
                  VALUES ($1, NULL, $2, 'operator', 'provider/operator', 1, $3)";
    for (n, key_name) in [(1_u8, None), (2, None), (3, Some("root-operator"))] {
        sqlx::query(insert)
            .bind(Uuid::from_u128(0xD970 + u128::from(n)))
            .bind(vec![n; 32])
            .bind(key_name)
            .execute(&mut *tx)
            .await
            .expect("unnamed operator keys and a tenant key's name stand together");
    }
    let err = sqlx::query(insert)
        .bind(Uuid::from_u128(0xD974))
        .bind(vec![4_u8; 32])
        .bind(Some("root-operator"))
        .execute(&mut *tx)
        .await
        .expect_err("a second operator key of one name is refused");
    assert!(
        err.to_string().contains("api_keys_tenant_id_name"),
        "got: {err}"
    );
    drop(tx);
    drop_dbs(&created).await;
}

/// The schema itself refuses a non-root workarea named `universe`, whatever
/// wrote it: 0006's CHECK ties `universe` to the root, so the refusal holds
/// under a root that predates names and has no name yet, as every tenant
/// 0006 migrates has, and not only under a root that already holds
/// `universe`. The CHECK passes every unnamed row, so it is laid beside the
/// nullable names. The store answers the creation with `NameTaken`, as
/// errors.md answers a non-root `universe` (postgres-schema.md `workareas`
/// and "The migration that adds names"; ADR-0026 d5, d6).
#[tokio::test]
async fn the_schema_refuses_a_non_root_workarea_named_universe() {
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 5).await;
    let (poc, poc_root) = (Uuid::from_u128(0xA960), Uuid::from_u128(0xB960));
    let mut tx = conn.begin().await.expect("begin");
    poc_tenant(&mut tx, poc).await;
    sqlx::query(
        "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
         VALUES ($1, $2, NULL, 'root', 'committed')",
    )
    .bind(poc_root)
    .bind(poc)
    .execute(&mut *tx)
    .await
    .expect("PoC root");
    tx.commit().await.expect("commit the PoC rows");
    let store = PgStore::connect(&db_url(&db)).await.expect("connect");
    store.migrate().await.expect("0006 applies over PoC rows");

    let mut rig = ChainRig::new(97);
    rig.seed_chain(&store, 0).await;
    let t = rig.tenant_id;
    let root = Uuid::from_u128(0xB970);
    rig.create_workarea(&store, workarea_record(t, root, None, "universe"))
        .await;

    let impostor = Uuid::from_u128(0xB971);
    let refused = rig
        .commit(
            &store,
            event_types::WORKAREA_CREATE,
            json!({ "workarea_id": impostor.to_string(), "name": "universe" }),
            MetadataFold::WorkareaCreated(workarea_record(t, impostor, Some(root), "universe")),
            None,
        )
        .await;
    assert_eq!(
        refused,
        Err(StoreError::NameTaken {
            kind: NamedKind::Workarea,
            name: name("universe"),
        })
    );

    for (tenant, parent, under) in [
        (poc, poc_root, "a root that has no name yet"),
        (t, root, "the root named universe"),
    ] {
        let mut tx = store.pool().begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let err = sqlx::query(
            "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
             VALUES ($1, $2, $3, 'universe', 'development')",
        )
        .bind(impostor)
        .bind(tenant)
        .bind(parent)
        .execute(&mut *tx)
        .await
        .expect_err("the schema refuses a non-root universe");
        assert!(
            err.to_string().contains("workareas_universe_is_root"),
            "under {under}, got: {err}"
        );
    }
    drop(conn);
    drop_dbs(&created).await;
}

/// A workarea is stored with its `path`, derived at creation from its
/// parent's path and its own name: the `ltree` value of the names from
/// `universe` down (postgres-schema.md `workareas`; ADR-0026 d7).
#[tokio::test]
async fn a_workarea_is_stored_with_the_path_its_names_derive() {
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut rig = ChainRig::new(98);
    rig.seed_chain(&store, 0).await;
    let t = rig.tenant_id;
    let (root, procurement, metals) = (
        Uuid::from_u128(0xB980),
        Uuid::from_u128(0xB981),
        Uuid::from_u128(0xB982),
    );
    for w in [
        workarea_record(t, root, None, "universe"),
        workarea_record(t, procurement, Some(root), "procurement"),
        workarea_record(t, metals, Some(procurement), "metals-crew"),
    ] {
        rig.create_workarea(&store, w).await;
    }
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let paths: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, path::text FROM workareas WHERE tenant_id = $1 ORDER BY id")
            .bind(t)
            .fetch_all(&mut *tx)
            .await
            .expect("paths");
    tx.rollback().await.expect("rollback");
    assert_eq!(
        paths,
        vec![
            (root, "universe".to_owned()),
            (procurement, "universe.procurement".to_owned()),
            (metals, "universe.procurement.metals-crew".to_owned()),
        ]
    );
    drop_dbs(&created).await;
}

/// The PoC tables 0006 reaches, besides the journal, in the order a
/// snapshot lists them.
const POC_TABLES: [&str; 10] = [
    "tenants",
    "workareas",
    "api_keys",
    "journal_chain_head",
    "outbox",
    "memories",
    "review_queue",
    "idempotency",
    "grants",
    "key_denials",
];

/// Every row `tenant`'s row-level security shows of the PoC tables and the
/// journal, one sorted JSON text per row. `renamed` reads the schema after
/// 0006: the columns it adds are left out, and `prior_name` is read under
/// the name the PoC schema gave it, so a row 0006 kept compares equal.
async fn poc_rows(conn: &mut PgConnection, tenant: Uuid, renamed: bool) -> Vec<String> {
    let mut tx = conn.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let mut rows = Vec::new();
    for table in POC_TABLES.iter().copied().chain(["journal"]) {
        let row = match (renamed, table) {
            (true, "tenants" | "workareas") => {
                "(to_jsonb(t) - 'name' - 'prior_name' - 'crew_id' - 'path') \
                 || jsonb_build_object('name', t.prior_name)"
            }
            (true, "api_keys") => "to_jsonb(t) - 'name' - 'crew_id'",
            _ => "to_jsonb(t)",
        };
        let sql = format!("SELECT '{table} ' || ({row})::text FROM {table} t");
        let texts: Vec<String> = sqlx::query_scalar(&sql)
            .fetch_all(&mut *tx)
            .await
            .unwrap_or_else(|e| panic!("{table}: {e}"));
        rows.extend(texts);
    }
    tx.rollback().await.expect("rollback");
    rows.sort();
    rows
}

/// The PoC's duplicate, ungrammatical and reserved names do not stop 0006:
/// every row of every table survives with every value it held, the PoC's
/// names as `prior_name`, and no row gains a name, since only the lead's
/// first start names rows, in a journalled event (ADR-0026 d11). The
/// journal is untouched and still verifies, and the store serves no
/// workarea that has no name yet (postgres-schema.md names and tenants).
#[tokio::test]
async fn a_migrated_poc_database_keeps_every_row_and_every_journal_event() {
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 5).await;
    let (tenant, twin) = (Uuid::from_u128(0xA600), Uuid::from_u128(0xA601));
    let id = |n: u128| Uuid::from_u128(0xB600 + n);
    let (root, procurement, team, again, impostor) = (id(0), id(1), id(2), id(3), id(4));
    let (admin, writer) = (Uuid::from_u128(0xD600), Uuid::from_u128(0xD601));
    let memory = Uuid::from_u128(0xC600);
    let created_wa = |w: Uuid, parent: Option<Uuid>, wa_name: &str, crew: Value| {
        json!({
            "workarea_id": w, "parent_id": parent, "name": wa_name,
            "status": if parent.is_none() { "committed" } else { "development" },
            "crew": crew, "prefix": null,
        })
    };
    // The PoC's chain, signed: its root named `root`, a crew object, an
    // ungrammatical name, a duplicate one and a non-root `universe`.
    let drafts = [
        (event_types::TENANT_GENESIS, json!({ "tenant_name": "oml" })),
        (
            event_types::WORKAREA_CREATE,
            created_wa(root, None, "root", Value::Null),
        ),
        (
            event_types::WORKAREA_CREATE,
            created_wa(
                procurement,
                Some(root),
                "procurement",
                json!({ "name": "Procurement Crew" }),
            ),
        ),
        (
            event_types::WORKAREA_CREATE,
            created_wa(team, Some(root), "Procurement Team", Value::Null),
        ),
        (
            event_types::WORKAREA_CREATE,
            created_wa(again, Some(team), "procurement", Value::Null),
        ),
        (
            event_types::WORKAREA_CREATE,
            created_wa(impostor, Some(team), "universe", Value::Null),
        ),
        (
            event_types::KEY_ISSUED,
            json!({ "key_id": admin, "role": "admin", "principal": "oml/admin" }),
        ),
        (
            event_types::KEY_ISSUED,
            json!({ "key_id": writer, "role": "writer", "principal": "crew/agent-7" }),
        ),
        (
            event_types::GRANT_ISSUED,
            json!({ "grant_id": id(10), "key_id": writer, "workarea_node_id": procurement }),
        ),
        (
            event_types::KEY_DENIED,
            json!({ "denial_id": id(11), "key_id": admin, "workarea_node_id": team }),
        ),
        (
            event_types::SUBMISSION_REMEMBER,
            json!({ "artifact_id": memory, "workarea_id": procurement }),
        ),
        (event_types::KEY_REVOKED, json!({ "key_id": writer })),
    ];
    let mut head: Option<SignedEnvelope> = None;
    let mut events = Vec::new();
    for (n, (event_type, payload)) in drafts.into_iter().enumerate() {
        let n = u64::try_from(n).expect("seq");
        let draft = EventDraft {
            tenant_id: tenant,
            event_id: Uuid::from_u128(0xE600 + u128::from(n)),
            event_type: event_type.to_owned(),
            principal: "oml/admin".to_owned(),
            api_key_id: None,
            at: EventTime::parse(&format!("2026-10-01T09:00:{n:02}.000000Z")).expect("time"),
            payload,
        };
        let event = sign(
            next_envelope(head.as_ref(), draft).expect("lawful"),
            &test_signing_key(),
        )
        .expect("signing");
        head = Some(event.clone());
        events.push(event);
    }

    let mut tx = conn.begin().await.expect("begin");
    let set_tenant = "SELECT set_config('oml.tenant_id', $1, true)";
    // A second tenant the PoC gave the same name, with its own `root`.
    sqlx::query(set_tenant)
        .bind(twin.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'oml')")
        .bind(twin)
        .execute(&mut *tx)
        .await
        .expect("twin tenant");
    sqlx::query(
        "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
         VALUES ($1, $2, NULL, 'root', 'committed')",
    )
    .bind(id(9))
    .bind(twin)
    .execute(&mut *tx)
    .await
    .expect("twin root");

    sqlx::query(set_tenant)
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'oml')")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .expect("tenant");
    for event in &events {
        let env = &event.envelope;
        sqlx::query(
            "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, \
             api_key_id, at, payload, signed_bytes, signed_bytes_hash, sig) \
             VALUES ($1, $2, $3, $4, $5, $6, NULL, ($7)::timestamptz, $8, $9, $10, $11)",
        )
        .bind(tenant)
        .bind(i64::try_from(env.seq).expect("seq"))
        .bind(env.event_id)
        .bind(&env.event_type)
        .bind(env.prev_hash.0.to_vec())
        .bind(&env.principal)
        .bind(env.at.as_str())
        .bind(&env.payload)
        .bind(&event.signed_bytes)
        .bind(event.hash.0.to_vec())
        .bind(event.sig.to_bytes().to_vec())
        .execute(&mut *tx)
        .await
        .expect("journal row");
        sqlx::query(
            "INSERT INTO outbox (tenant_id, tenant_seq, kind, payload) \
             VALUES ($1, $2, 'bronze-event', $3)",
        )
        .bind(tenant)
        .bind(i64::try_from(env.seq).expect("seq"))
        .bind(json!({ "event_id": env.event_id }))
        .execute(&mut *tx)
        .await
        .expect("outbox row");
    }
    let last = head.expect("the chain has a head");
    sqlx::query("INSERT INTO journal_chain_head (tenant_id, seq, head_hash) VALUES ($1, $2, $3)")
        .bind(tenant)
        .bind(i64::try_from(last.envelope.seq).expect("seq"))
        .bind(last.hash.0.to_vec())
        .execute(&mut *tx)
        .await
        .expect("chain head");
    for (w, parent, wa_name, crew) in [
        (root, None, "root", None),
        (
            procurement,
            Some(root),
            "procurement",
            Some(json!({ "name": "Procurement Crew" })),
        ),
        (team, Some(root), "Procurement Team", None),
        (again, Some(team), "procurement", None),
        (impostor, Some(team), "universe", None),
    ] {
        sqlx::query(
            "INSERT INTO workareas (id, tenant_id, parent_id, name, status, crew) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(w)
        .bind(tenant)
        .bind(parent)
        .bind(wa_name)
        .bind(if parent.is_none() {
            "committed"
        } else {
            "development"
        })
        .bind(crew)
        .execute(&mut *tx)
        .await
        .expect("workarea row");
    }
    for (key_id, role, principal, seq) in [
        (admin, "admin", "oml/admin", 7_i64),
        (writer, "writer", "crew/agent-7", 8),
    ] {
        sqlx::query(
            "INSERT INTO api_keys (id, tenant_id, hash, role, principal, held_by, \
             default_workarea_id, issued_seq) VALUES ($1, $2, $3, $4, $5, 'agent', $6, $7)",
        )
        .bind(key_id)
        .bind(tenant)
        .bind(Sha256Hash::digest(key_id.as_bytes()).0.to_vec())
        .bind(role)
        .bind(principal)
        .bind(root)
        .bind(seq)
        .execute(&mut *tx)
        .await
        .expect("key row");
    }
    sqlx::query("UPDATE api_keys SET revoked_at = '2026-10-01T09:00:11Z' WHERE id = $1")
        .bind(writer)
        .execute(&mut *tx)
        .await
        .expect("revocation");
    sqlx::query(
        "INSERT INTO grants (id, tenant_id, key_id, role, workarea_node_id, epoch) \
         VALUES ($1, $2, $3, 'writer', $4, 9)",
    )
    .bind(id(10))
    .bind(tenant)
    .bind(writer)
    .bind(procurement)
    .execute(&mut *tx)
    .await
    .expect("grant row");
    sqlx::query(
        "INSERT INTO key_denials (id, tenant_id, key_id, workarea_node_id, epoch, created_by) \
         VALUES ($1, $2, $3, $4, 10, $3)",
    )
    .bind(id(11))
    .bind(tenant)
    .bind(admin)
    .bind(team)
    .execute(&mut *tx)
    .await
    .expect("denial row");
    sqlx::query(
        "INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, \
         created_event_id, claim_key, author_principal, author_key_id, version, kind, \
         body_bytes) VALUES ($1, $2, $3, 'nickel quote', $4, 'held', $5, 'quote', \
         'crew/agent-7', $6, 1, 'memory', 12)",
    )
    .bind(memory)
    .bind(tenant)
    .bind(procurement)
    .bind(Sha256Hash::digest(b"nickel quote").0.to_vec())
    .bind(events[10].envelope.event_id)
    .bind(writer)
    .execute(&mut *tx)
    .await
    .expect("memory row");
    sqlx::query("INSERT INTO review_queue (memory_id, tenant_id) VALUES ($1, $2)")
        .bind(memory)
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .expect("queue row");
    sqlx::query(
        "INSERT INTO idempotency (tenant_id, key, request_digest, response, committed_at) \
         VALUES ($1, 'poc-receipt', $2, $3, now())",
    )
    .bind(tenant)
    .bind(vec![0_u8])
    .bind(json!({ "memory_id": memory, "version": 1 }))
    .execute(&mut *tx)
    .await
    .expect("idempotency row");
    tx.commit().await.expect("commit the PoC rows");

    let before = [
        poc_rows(&mut conn, tenant, false).await,
        poc_rows(&mut conn, twin, false).await,
    ];
    let store = PgStore::connect(&db_url(&db)).await.expect("connect");
    store
        .migrate()
        .await
        .expect("0006 applies over the PoC's names");
    let after = [
        poc_rows(&mut conn, tenant, true).await,
        poc_rows(&mut conn, twin, true).await,
    ];
    assert_eq!(
        after, before,
        "every row keeps every value, the PoC's names as prior_name"
    );
    assert_eq!(before[0].len(), 2 * events.len() + 14, "the seeded rows");

    for t in [tenant, twin] {
        let mut tx = conn.begin().await.expect("begin");
        sqlx::query(set_tenant)
            .bind(t.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let named: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM tenants WHERE name IS NOT NULL) \
             + (SELECT count(*) FROM workareas \
                WHERE name IS NOT NULL OR crew_id IS NOT NULL OR path IS NOT NULL) \
             + (SELECT count(*) FROM api_keys WHERE name IS NOT NULL OR crew_id IS NOT NULL) \
             + (SELECT count(*) FROM crews)",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("named rows");
        tx.rollback().await.expect("rollback");
        assert_eq!(named, 0, "the migration names nothing and makes no crew");
    }

    let keys = KeySet::new(vec![test_signing_key().verifying_key()]);
    let verified = store
        .verify_stored_chain(tenant, &keys)
        .await
        .expect("the journal verifies as signed");
    assert_eq!(verified.seq, last.envelope.seq);
    assert!(
        matches!(
            store.workarea(tenant, root).await,
            Err(StoreError::Backend(_))
        ),
        "a workarea with no name yet is not served"
    );
    drop(conn);
    drop_dbs(&created).await;
}

/// The store reports, per table, the rows that predate names: after 0006 a
/// PoC tenant row and each of its workarea rows has none, while a tenant
/// created since, with its `universe` root, a workarea and a key, reports
/// nothing, its key's missing name included, since issuance names no key
/// yet (ADR-0026 d9, d11; http-api.md "Rows created before names").
#[tokio::test]
async fn rows_that_predate_names_are_reported_per_table_and_rows_created_since_are_not() {
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 5).await;
    let poc = Uuid::from_u128(0xA700);
    let (poc_root, poc_team) = (Uuid::from_u128(0xB700), Uuid::from_u128(0xB701));
    let mut tx = conn.begin().await.expect("begin");
    poc_tenant(&mut tx, poc).await;
    for (w, parent, poc_name, status) in [
        (poc_root, None, "root", "committed"),
        (poc_team, Some(poc_root), "Procurement Team", "development"),
    ] {
        sqlx::query(
            "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(w)
        .bind(poc)
        .bind(parent)
        .bind(poc_name)
        .bind(status)
        .execute(&mut *tx)
        .await
        .expect("PoC workarea");
    }
    tx.commit().await.expect("commit the PoC rows");
    let store = PgStore::connect(&db_url(&db)).await.expect("connect");
    store.migrate().await.expect("0006 applies over PoC rows");

    let mut rig = ChainRig::new(97);
    rig.seed_chain(&store, 0).await;
    let t = rig.tenant_id;
    let (root, crew, key) = (
        Uuid::from_u128(0xB970),
        Uuid::from_u128(0xB971),
        Uuid::from_u128(0xD970),
    );
    rig.create_workarea(&store, workarea_record(t, root, None, "universe"))
        .await;
    rig.create_workarea(&store, workarea_record(t, crew, Some(root), "metals-crew"))
        .await;
    rig.issue_key(&store, key_record(t, key, Role::Writer, None))
        .await;

    assert_eq!(
        store
            .rows_without_names(&[poc, t])
            .await
            .expect("the check reads"),
        vec![("tenants", 1), ("workareas", 2)],
        "the PoC tenant and both its workareas, and nothing created since"
    );
    assert_eq!(
        store
            .rows_without_names(&[t])
            .await
            .expect("the check reads"),
        Vec::<(&str, u64)>::new(),
        "a tenant created since names reports nothing"
    );
    drop(conn);
    drop_dbs(&created).await;
}
