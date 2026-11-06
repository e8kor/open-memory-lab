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
//! Each law runs against a throwaway database (`oml_test_<pid>_<n>_<tag>`)
//! owned by the owner of a role pair minted for it alone, migrated by the
//! embedded migrator through the owner login with `oml.serving_role` set,
//! asserted over the serving login, and dropped with its roles best-effort
//! afterwards (ADR-0034 d8). Minting takes a superuser at `DATABASE_URL`; a
//! role that is not one prints `did not run: <role> cannot create roles` and
//! the law returns, except under `OML_TEST_REQUIRE_SPLIT=1`, where it fails.
//! The serving role's privilege laws sit beside the journal guard law, which
//! runs as the owner (ADR-0034 d3, d6), and the role split's boot checks
//! refuse each of their reasons by name and accept a minted, migrated pair
//! (ADR-0034 d5). The status ladder runs end to end through the use cases
//! over the serving login: a proposal and its ratification move the stored
//! status and journal both events under row security, and a child create
//! racing a ratification to `validated` never leaves a validated node over
//! an unvalidated child (ADR-0009 d3, d4).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use oml_app::App;
use oml_app::domain::access::{Covers, Grant, KeyDenial};
use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::oauth::ConnectionState;
use oml_app::domain::vocab_edges::EdgeState;
use oml_app::domain::{
    Envelope, EventDraft, EventTime, FrozenOp, KeySet, LadderRefusal, MemoryKind, MemoryStatus,
    Role, Sha256Hash, SignedEnvelope, WorkareaStatus, event_types, next_envelope, sign,
    verify_chain,
};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, SeqIdGen};
use oml_app::ports::{
    CommitReceipt, CommitRequest, ConnectionStore, JournalStore, MetadataFold, OutboxRow,
    Retriever, StoreError,
};
use oml_app::search::{CandidateScope, MatchMode, OwnHeldScope, RetrievalQuery, RetrieverKind};
use oml_app::types::{AppError, BootstrapRequest, ClaimEdgeRecord, KeyRecord, PROVIDER_TENANT};
use oml_pg::{
    ForkCheckError, MIGRATOR, PgStore, RoleSplitCheckError, RoleSplitError, RoleSplitReason,
    role_split_postflight, role_split_preflight,
};
use oml_port_contracts::{
    ChainRig, KeywordAllReads as _, auth_code_hash, authorization_request, connect_code,
    connect_grant, connector_key, handle_hash, held_memory, key_record, name, pending_transition,
    run_connection_store_suite, run_journal_store_suite, run_retriever_suite, test_signing_key,
    token_pair, workarea_record,
};
use serde_json::{Value, json};
use sqlx::migrate::Migrate as _;
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::{Connection, Executor, Row};
use uuid::Uuid;

fn base_url() -> String {
    // The gate's documented local profile; CI overrides with DATABASE_URL.
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL names a throwaway PostgreSQL cluster; no suite defaults to one, so none reaches a live server")
}

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
const PAIR_PASSWORD: &str = "oml_test_pair";

/// `db` at `DATABASE_URL`'s host, logged in as `role`.
fn role_url(db: &str, role: &str) -> String {
    let base = base_url();
    let (scheme, rest) = base
        .split_once("://")
        .expect("DATABASE_URL is a postgres:// URL");
    let (_, host_path) = rest.rsplit_once('@').expect("DATABASE_URL names its user");
    let (host, _) = host_path
        .split_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{scheme}://{role}:{PAIR_PASSWORD}@{host}/{db}")
}

/// `db` through the owner login of its role pair (ADR-0034 d2, d8).
fn owner_url(db: &str) -> String {
    role_url(db, &format!("{db}_owner"))
}

/// `db` through the serving login of its role pair (ADR-0034 d2, d8).
fn serving_url(db: &str) -> String {
    role_url(db, &format!("{db}_serving"))
}

/// Whether this run can mint the owner/serving role pair every law runs
/// through (ADR-0034 d8). Minting takes a superuser; when `DATABASE_URL`'s
/// role is not one, as the maintainer's local `oml` (CREATEDB alone) is not,
/// this prints `did not run: <role> cannot create roles` and the law
/// returns, and under `OML_TEST_REQUIRE_SPLIT=1`, which CI sets, the same
/// condition fails it, so no run passes vacuously.
async fn splits() -> bool {
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    let (role, mints): (String, bool) = sqlx::query_as(
        "SELECT current_user::text, rolsuper FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut admin)
    .await
    .expect("role attributes");
    if mints {
        return true;
    }
    assert!(
        std::env::var("OML_TEST_REQUIRE_SPLIT").as_deref() != Ok("1"),
        "{role} cannot create roles, and OML_TEST_REQUIRE_SPLIT=1 requires the role pair (ADR-0034 d8)"
    );
    println!("did not run: {role} cannot create roles");
    false
}

static DB_N: AtomicU64 = AtomicU64::new(0);

/// Eight hex digits of the clock, so a name a crashed earlier run left
/// behind on the cluster under the same pid and counter is not reused.
fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

/// A throwaway database with no migration applied, owned by the owner of a
/// role pair minted for it alone (`<name>_owner`, `<name>_serving`, `LOGIN`
/// and no other attribute; ADR-0034 d2, d8); its name.
async fn fresh_database(created: &Mutex<Vec<String>>) -> String {
    let name = format!(
        "oml_test_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    for role in ["owner", "serving"] {
        admin
            .execute(format!("CREATE ROLE {name}_{role} LOGIN PASSWORD '{PAIR_PASSWORD}'").as_str())
            .await
            .expect("mint the role pair");
    }
    admin
        .execute(format!("CREATE DATABASE {name} OWNER {name}_owner").as_str())
        .await
        .expect("create throwaway database");
    created.lock().expect("names lock").push(name.clone());
    name
}

/// A store over the serving login of a fresh database, migrated through its
/// owner login with `oml.serving_role` set (ADR-0034 d4, d8).
async fn fresh_store(created: &Mutex<Vec<String>>) -> PgStore {
    let name = fresh_database(created).await;
    let store = PgStore::connect(&serving_url(&name))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&name));
    store
        .migrate(&owner_url(&name))
        .await
        .expect("embedded migrations apply");
    store
}

/// A connection to `store`'s database through its owner login, for what
/// only the owner may do: DDL, `ANALYZE`, `_sqlx_migrations`, the journal
/// guard law (ADR-0034 d8).
async fn owner_conn(store: &PgStore) -> PgConnection {
    let db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(store.pool())
        .await
        .expect("database name");
    PgConnection::connect(&owner_url(&db))
        .await
        .expect("owner connect")
}

/// Best-effort cleanup; WITH (FORCE) evicts any pool connection still open,
/// and the role pair goes with its database.
async fn drop_dbs(created: &Mutex<Vec<String>>) {
    let names: Vec<String> = created.lock().expect("names lock").drain(..).collect();
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        for name in names {
            let _ = admin
                .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
                .await;
            for role in ["serving", "owner"] {
                let _ = admin
                    .execute(format!("DROP ROLE IF EXISTS {name}_{role}").as_str())
                    .await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The identical contract suite
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journal_store_contract_suite() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    run_journal_store_suite(|| fresh_store(&created)).await;
    drop_dbs(&created).await;
}

/// The connection laws the double proves, against the real store: the
/// connection folds run inside the commit's transaction under the tenant's
/// `oml.tenant_id`, and a refused fold commits nothing (postgres-schema.md
/// "connections (ADR-0031)"; ADR-0031 d8, d12).
#[tokio::test]
async fn connection_store_contract_suite() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    run_connection_store_suite(|| fresh_store(&created)).await;
    drop_dbs(&created).await;
}

/// A fresh store with its search probe run, so the trigram retriever is
/// served wherever 0020 created `pg_trgm` (ADR-0049 d8).
async fn fresh_search_store(created: &Mutex<Vec<String>>) -> PgStore {
    fresh_store(created)
        .await
        .with_search_probe()
        .await
        .expect("search probe")
}

/// The retriever laws the double proves, against the real store, for every
/// retriever it serves: the keyword pair, bm25 and, with `pg_trgm`, trigram
/// (ADR-0049 d7).
#[tokio::test]
async fn retriever_contract_suite() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let probe = fresh_search_store(&created).await;
    assert!(
        probe.retrievers().contains(&RetrieverKind::Trigram),
        "0020 creates pg_trgm as the owner of the database, so the trigram retriever is served"
    );
    run_retriever_suite(|| fresh_search_store(&created)).await;
    drop_dbs(&created).await;
}

/// The learned sparse laws on PostgreSQL, as the serving role under row
/// security (ADR-0052 d15, d16, d17).
#[tokio::test]
async fn sparse_contract_suite() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    oml_port_contracts::run_sparse_suite(|| fresh_search_store(&created)).await;
    drop_dbs(&created).await;
}

/// `keyword-all` returns exactly what the v0.5.0 recall returned: the rows
/// and the order of its two statements, merged and cut to `k` as the core
/// merged them, for every query of a fixture (ADR-0049 d7).
#[tokio::test]
async fn keyword_all_returns_exactly_the_v0_5_0_rows_and_order() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(420);
    rig.seed_chain(&store, 0).await;
    let wa = Uuid::from_u128(0xE2);
    let own = Uuid::from_u128(0xE4);
    let bodies = [
        "deploy the billing service on friday",
        "the billing service owns invoices and billing runs nightly",
        "never deploy on friday evenings, deploy on monday",
        "invoices are numbered per tenant",
        "billing deploy billing deploy",
        "rollback the release with one command",
    ];
    for (i, body) in bodies.iter().enumerate() {
        let m = held_memory(
            rig.tenant_id,
            wa,
            Uuid::from_u128(0x900 + i as u128),
            "rita",
            Uuid::from_u128(0xE5),
            body,
        );
        oml_port_contracts::hold_and_promote(&mut rig, &store, m).await;
    }
    let held = held_memory(
        rig.tenant_id,
        wa,
        Uuid::from_u128(0x950),
        "rita",
        own,
        "billing deploy held",
    );
    oml_port_contracts::hold(&mut rig, &store, held)
        .await
        .expect("hold");
    let kinds = vec!["memory", "derived_fact"];
    for query in [
        "billing",
        "billing deploy",
        "deploy friday",
        "invoices",
        "zebra",
        "the",
    ] {
        for k in [1_usize, 2, 10] {
            let mut tx = store.pool().begin().await.expect("begin");
            sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
                .bind(rig.tenant_id.to_string())
                .execute(&mut *tx)
                .await
                .expect("tenant");
            let mut old: Vec<(Uuid, f32)> = Vec::new();
            for row in sqlx::query(oml_pg::RECALL_FTS)
                .bind(rig.tenant_id)
                .bind(vec![wa])
                .bind(query)
                .bind(&kinds)
                .bind(i64::try_from(k).unwrap())
                .fetch_all(&mut *tx)
                .await
                .expect("v0.5.0 statement")
                .iter()
                .chain(
                    sqlx::query(oml_pg::RECALL_OWN_HELD)
                        .bind(rig.tenant_id)
                        .bind(vec![own])
                        .bind(vec![wa])
                        .bind(query)
                        .bind(&kinds)
                        .bind(i64::try_from(k).unwrap())
                        .fetch_all(&mut *tx)
                        .await
                        .expect("v0.5.0 own held statement")
                        .iter(),
                )
            {
                old.push((row.get("id"), row.get("score")));
            }
            tx.commit().await.expect("commit");
            oml_app::domain::budget::hit_order(&mut old, |(id, score)| (*score, *id));
            old.truncate(k);
            let new = store
                .retrieve(
                    rig.tenant_id,
                    RetrieverKind::KeywordAll,
                    &CandidateScope {
                        anchor: oml_app::types::AnchorFilter::default(),
                        promoted_nodes: vec![wa],
                        kinds: vec![MemoryKind::Memory, MemoryKind::DerivedFact],
                        own_held: Some(OwnHeldScope {
                            any_author: false,
                            author_key_ids: vec![own],
                            nodes: vec![wa],
                        }),
                        expired_at: None,
                    },
                    RetrievalQuery {
                        query,
                        match_mode: MatchMode::Any,
                        k,
                        sparse: &[],
                        window: None,
                        within: &[],
                    },
                )
                .await
                .expect("keyword-all");
            let new: Vec<(Uuid, f32)> = new
                .candidates
                .iter()
                .map(|c| (c.memory_id, c.score))
                .collect();
            assert_eq!(new, old, "{query:?} k={k}");
        }
    }
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
    if !splits().await {
        return;
    }
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

/// Signs and commits one `connection.opened` of `connection_id` at an
/// explicit (seq, prev_hash), naming no replaced connection: what a token
/// exchange does after reading the head.
async fn open_at(
    store: &PgStore,
    tenant_id: Uuid,
    head: &oml_app::domain::ChainHead,
    connection_id: Uuid,
) -> Result<CommitReceipt, StoreError> {
    let event_id =
        Uuid::from_u128(0xE000_0000 + u128::from(EVENT_N.fetch_add(1, Ordering::SeqCst)));
    let envelope = Envelope {
        tenant_id,
        seq: head.seq + 1,
        event_id,
        event_type: event_types::CONNECTION_OPENED.to_owned(),
        prev_hash: head.hash,
        principal: "race".to_owned(),
        api_key_id: None,
        // Before the fixture authorization code's expiry, 13:01:00.
        at: EventTime::parse("2026-10-29T13:00:50.000000Z").expect("fixture time"),
        payload: json!({
            "connection_id": connection_id.to_string(),
            "replaces_connection_id": Value::Null,
        }),
    };
    let event = sign(envelope, &test_signing_key()).expect("signing");
    store
        .commit(CommitRequest {
            event: event.clone(),
            fold: MetadataFold::ConnectionOpened {
                connection_id,
                replaces: None,
                auth_code_hash: auth_code_hash(connection_id),
                tokens: token_pair(connection_id),
            },
            outbox: OutboxRow {
                event_id,
                topic: event.envelope.event_type.clone(),
                payload: json!({ "seq": event.envelope.seq }),
            },
            idempotency: None,
        })
        .await
}

/// Two racing `connection.opened` commits for one key leave exactly one
/// open connection: two token exchanges presenting one authorization code
/// each retry past the chain's conflicts, one opens the connection and the
/// other's fold, reading the row under the chain lock, is refused and
/// commits nothing (postgres-schema.md `oauth_connections_open`, redemption
/// is a conditional update; ADR-0031 d7, d10, d12).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_racing_connection_opened_commits_for_one_key_leave_exactly_one_open_connection() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = Arc::new(fresh_store(&created).await);
    let mut rig = ChainRig::new(330);
    rig.seed_chain(&*store, 0).await;
    let t = rig.tenant_id;
    let key = connector_key(t, Uuid::from_u128(0xD3300));
    rig.issue_key(&*store, key.clone()).await;
    let (code, connection) = (Uuid::from_u128(0xC3301), Uuid::from_u128(0xE3301));
    rig.mint_code(&*store, connect_code(code, key.key_id), None)
        .await
        .expect("mint");
    let spent_at = EventTime::parse("2026-10-29T13:00:20.000000Z").expect("fixture time");
    assert!(store.spend_code(t, code, &spent_at).await.expect("redeem"));
    store
        .authorization_insert(authorization_request(t, key.key_id, code, connection))
        .await
        .expect("request");
    store
        .connection_connect(&handle_hash(connection), connect_grant(connection))
        .await
        .expect("connect");

    let start = Arc::new(tokio::sync::Barrier::new(2));
    let racers: Vec<_> = (0..2)
        .map(|_| {
            let (store, start) = (Arc::clone(&store), Arc::clone(&start));
            tokio::spawn(async move {
                start.wait().await;
                loop {
                    let head = store
                        .chain_head(t)
                        .await
                        .expect("head")
                        .expect("the rig's chain has a head");
                    match open_at(&store, t, &head, connection).await {
                        // The chain's loser re-reads the head and retries,
                        // so the fold is what decides (ADR-0002 d3).
                        Err(
                            StoreError::SeqConflict { .. } | StoreError::PrevHashConflict { .. },
                        ) => {}
                        outcome => return outcome,
                    }
                }
            })
        })
        .collect();
    let mut outcomes = Vec::new();
    for racer in racers {
        outcomes.push(racer.await.expect("racer"));
    }
    let opened: Vec<&CommitReceipt> = outcomes.iter().filter_map(|o| o.as_ref().ok()).collect();
    assert_eq!(opened.len(), 1, "exactly one exchange opens: {outcomes:?}");
    assert!(
        outcomes.iter().any(|o| o.as_ref().err()
            == Some(&StoreError::ConnectionNotOpenable {
                connection_id: connection
            })),
        "the other is refused by the fold, by name: {outcomes:?}"
    );
    assert_eq!(
        store.chain_head(t).await.expect("head").map(|h| h.seq),
        Some(opened[0].seq),
        "the refused opening journals nothing"
    );

    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(t.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_connections \
         WHERE tenant_id = $1 AND key_id = $2 AND state = 'open'",
    )
    .bind(t)
    .bind(key.key_id)
    .fetch_one(&mut *tx)
    .await
    .expect("count");
    tx.commit().await.expect("commit");
    assert_eq!(open, 1, "the key has exactly one open connection");
    let row = store
        .connection(t, connection)
        .await
        .expect("read")
        .expect("the row");
    assert_eq!(row.state, ConnectionState::Open);
    assert_eq!(row.opened_seq, Some(opened[0].seq));
    drop(store);
    drop_dbs(&created).await;
}

/// ADR-0002 d4's guard law runs as the owner, the one login privileges let
/// reach the guard trigger (ADR-0034 d8).
#[tokio::test]
async fn journal_refuses_update_and_delete() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut rig = ChainRig::new(90);
    rig.seed_chain(&store, 1).await;

    // ADR-0002 d4: append-only by mechanism — the guard trigger fires even
    // for the table owner inside a correctly tenant-scoped transaction.
    let mut owner = owner_conn(&store).await;
    let mut tx = owner.begin().await.expect("begin");
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

    let mut tx = owner.begin().await.expect("begin");
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
    drop(owner);
    drop_dbs(&created).await;
}

/// The SQLSTATE a refused statement carried, or a panic naming what was
/// expected to be refused.
fn refused_code(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>, what: &str) -> String {
    let err = result.expect_err(what);
    err.as_database_error()
        .and_then(|d| d.code())
        .map(|c| c.into_owned())
        .unwrap_or_else(|| panic!("{what}: not a database error: {err}"))
}

/// 0007 grants `journal` SELECT and INSERT alone, so the serving role's
/// UPDATE and DELETE fail on privilege before the guard trigger runs
/// (ADR-0034 d3, d8; ADR-0002 d4).
#[tokio::test]
async fn the_serving_role_is_refused_update_and_delete_on_journal_by_privilege_first() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut rig = ChainRig::new(89);
    rig.seed_chain(&store, 1).await;

    for statement in [
        "UPDATE journal SET principal = 'tampered' WHERE tenant_id = $1",
        "DELETE FROM journal WHERE tenant_id = $1",
    ] {
        let mut tx = store.pool().begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(rig.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let result = sqlx::query(statement)
            .bind(rig.tenant_id)
            .execute(&mut *tx)
            .await;
        assert_eq!(
            refused_code(result, statement),
            "42501",
            "{statement} is refused by privilege, not by the trigger"
        );
        drop(tx);
    }
    drop_dbs(&created).await;
}

/// The tenants' marker words, one lexeme each under the `simple`
/// configuration `memories_claim_fts` indexes.
const LEXEME_A: &str = "quokkaalpha";
const LEXEME_B: &str = "quokkabravo";

/// `pg_stats` rows of the `public` schema whose text names `needle`, as
/// `conn` reads them with tenant `tenant` set.
async fn stats_naming(conn: &mut PgConnection, tenant: Uuid, needle: &str) -> Vec<String> {
    let mut tx = conn.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT s.tablename::text || '.' || s.attname::text FROM pg_stats s \
         WHERE s.schemaname = 'public' AND strpos(s::text, $1) > 0 ORDER BY 1",
    )
    .bind(needle)
    .fetch_all(&mut *tx)
    .await
    .expect("pg_stats read");
    tx.rollback().await.expect("rollback");
    rows
}

/// FORCE does not filter the owner's `pg_stats` view of the cross-tenant
/// index (spike 3 S3.1); the split keeps that view from the serving role,
/// which sees nothing of tenant B under tenant A, while the owner's view,
/// the control, shows B's lexemes (ADR-0034 d1, d8).
#[tokio::test]
async fn the_serving_role_sees_nothing_of_another_tenant_in_a_tenant_tables_pg_stats_while_the_owners_view_shows_tenant_bs_lexemes()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut tenants = Vec::new();
    for (seed, lexeme) in [(87_u64, LEXEME_A), (88, LEXEME_B)] {
        let mut rig = ChainRig::new(seed);
        rig.seed_chain(&store, 0).await;
        let wa = Uuid::from_u128(0xB000 + u128::from(seed));
        rig.create_workarea(&store, workarea_record(rig.tenant_id, wa, None, "universe"))
            .await;
        for n in 0..4_u128 {
            let memory = held_memory(
                rig.tenant_id,
                wa,
                Uuid::from_u128((u128::from(seed) << 16) | n),
                "stats",
                Uuid::from_u128(0xD000 + u128::from(seed)),
                &format!("{lexeme} runbook step {n}"),
            );
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
        tenants.push(rig.tenant_id);
    }
    let (a, _b) = (tenants[0], tenants[1]);
    let mut owner = owner_conn(&store).await;
    owner.execute("ANALYZE").await.expect("the owner analyses");

    let mut serving = store.pool().acquire().await.expect("serving connection");
    assert_eq!(
        stats_naming(&mut serving, a, LEXEME_B).await,
        Vec::<String>::new(),
        "the serving role, set to tenant A, sees no pg_stats row naming tenant B"
    );
    drop(serving);
    let shown = stats_naming(&mut owner, a, LEXEME_B).await;
    assert!(
        !shown.is_empty(),
        "control: the owner, set to tenant A, reads tenant B's lexemes in pg_stats"
    );
    drop(owner);
    drop_dbs(&created).await;
}

/// The serving role owns nothing and creates nothing, and a table whose
/// migration grants it nothing refuses it: each of these fails on
/// privilege (ADR-0034 d2, d3, d8).
#[tokio::test]
async fn the_serving_role_cannot_alter_drop_or_create_a_table_lift_force_truncate_a_table_or_select_or_insert_on_a_table_whose_migration_grants_nothing()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut owner = owner_conn(&store).await;
    owner
        .execute("CREATE TABLE ungranted (id int)")
        .await
        .expect("the owner creates a table no migration grants");
    drop(owner);

    for statement in [
        "ALTER TABLE memories ADD COLUMN intruder int",
        "DROP TABLE memories",
        "CREATE TABLE intruder (id int)",
        "ALTER TABLE memories NO FORCE ROW LEVEL SECURITY",
        "TRUNCATE review_queue",
        "TRUNCATE memories",
        "SELECT * FROM ungranted",
        "INSERT INTO ungranted (id) VALUES (1)",
    ] {
        let result = sqlx::query(statement).execute(store.pool()).await;
        assert_eq!(
            refused_code(result, statement),
            "42501",
            "the serving role is refused {statement}"
        );
    }
    drop_dbs(&created).await;
}

/// `_sqlx_migrations` is the one table without row-level security, and its
/// fence is privilege: 0007 revokes every privilege on it from PUBLIC and
/// grants the serving role none (ADR-0034 d6).
#[tokio::test]
async fn the_serving_role_holds_no_privilege_on_sqlx_migrations() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let held: Vec<String> = sqlx::query_scalar(
        "SELECT p FROM unnest(ARRAY['SELECT', 'INSERT', 'UPDATE', 'DELETE', 'TRUNCATE', \
         'REFERENCES', 'TRIGGER', 'MAINTAIN']) AS p \
         WHERE has_table_privilege(current_user, '_sqlx_migrations', p)",
    )
    .fetch_all(store.pool())
    .await
    .expect("privilege read");
    assert_eq!(
        held,
        Vec::<String>::new(),
        "no privilege on _sqlx_migrations"
    );
    let result = sqlx::query("SELECT version FROM _sqlx_migrations")
        .execute(store.pool())
        .await;
    assert_eq!(
        refused_code(result, "a serving read of _sqlx_migrations"),
        "42501"
    );
    drop_dbs(&created).await;
}

/// The migration whose grants name `oml.serving_role` fails at
/// `current_setting` (`42704`) on a connection that never set it, and
/// applies once it is set (ADR-0034 d3). It needs no role pair: the
/// connecting role creates the database and is its owner, so this law
/// runs wherever `DATABASE_URL`'s role has CREATEDB.
#[tokio::test]
async fn a_granting_migration_run_without_oml_serving_role_fails_at_current_setting_42704() {
    let name = format!(
        "oml_test_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("CREATE DATABASE {name}").as_str())
        .await
        .expect("create throwaway database");
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    let mut conn = PgConnection::connect(&format!("{prefix}/{name}"))
        .await
        .expect("connect");

    let err = MIGRATOR
        .run(&mut conn)
        .await
        .expect_err("a granting migration without oml.serving_role is refused");
    let refused = match &err {
        sqlx::migrate::MigrateError::ExecuteMigration(e, version) => (
            *version,
            e.as_database_error()
                .and_then(|d| d.code())
                .map(|c| c.into_owned()),
        ),
        other => panic!("not a migration's refusal: {other}"),
    };
    assert_eq!(
        refused,
        (7, Some("42704".to_owned())),
        "0007 fails at current_setting: {err}"
    );

    sqlx::query("SELECT set_config('oml.serving_role', current_user, false)")
        .execute(&mut conn)
        .await
        .expect("set the serving role");
    MIGRATOR
        .run(&mut conn)
        .await
        .expect("control: with oml.serving_role set, the migrations apply");
    drop(conn);
    let _ = admin
        .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
        .await;
}

// ---------------------------------------------------------------------------
// The role split's boot checks (ADR-0034 d5)
// ---------------------------------------------------------------------------

/// A fresh database's serving store and its owner pool, nothing migrated
/// (ADR-0034 d4, d8); the database's name.
async fn split_pair(created: &Mutex<Vec<String>>) -> (String, PgStore, PgPool) {
    let name = fresh_database(created).await;
    let store = PgStore::connect(&serving_url(&name))
        .await
        .expect("serving connect")
        .with_owner_url(&owner_url(&name));
    let owner = store
        .connect_owner(&owner_url(&name))
        .await
        .expect("owner connect");
    (name, store, owner)
}

/// [`split_pair`], migrated through the owner pool; the pre-migration
/// checks pass on it.
async fn migrated_pair(created: &Mutex<Vec<String>>) -> (String, PgStore, PgPool) {
    let (name, store, owner) = split_pair(created).await;
    role_split_preflight(&owner, store.pool())
        .await
        .expect("control: a minted pair passes the checks before migrating");
    store.migrate_as(&owner).await.expect("migrations apply");
    (name, store, owner)
}

/// A superuser connection to `db`, for what neither role of the pair may do.
async fn admin_in(db: &str) -> PgConnection {
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    PgConnection::connect(&format!("{prefix}/{db}"))
        .await
        .expect("admin connect to the throwaway database")
}

fn refused(result: Result<(), RoleSplitCheckError>, what: &str) -> RoleSplitReason {
    match result {
        Err(RoleSplitCheckError::Refused(RoleSplitError { reason })) => reason,
        Err(other) => panic!("{what}: not a refusal by name: {other}"),
        Ok(()) => panic!("{what}: the check accepted"),
    }
}

/// Both URLs logging in as one role is refused by login, not by URL: two
/// pools over the same login and database are one role (ADR-0034 d5). It
/// needs no role pair, so it runs wherever `DATABASE_URL`'s role has
/// CREATEDB.
#[tokio::test]
async fn preflight_refuses_one_login_for_both_urls_with_same_role() {
    let name = format!(
        "oml_test_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("CREATE DATABASE {name}").as_str())
        .await
        .expect("create throwaway database");
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    let url = format!("{prefix}/{name}");
    let store = PgStore::connect(&url).await.expect("serving connect");
    let owner = store.connect_owner(&url).await.expect("owner connect");

    assert_eq!(
        refused(
            role_split_preflight(&owner, store.pool()).await,
            "one login for both URLs"
        ),
        RoleSplitReason::SameRole
    );
    owner.close().await;
    store.pool().close().await;
    let _ = admin
        .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
        .await;
}

/// An owner login and a serving login that see two databases are refused
/// (ADR-0034 d5).
#[tokio::test]
async fn preflight_refuses_two_databases_with_different_databases() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (a, _store, owner) = split_pair(&created).await;
    let b = fresh_database(&created).await;
    let elsewhere = PgStore::connect(&role_url(&b, &format!("{a}_serving")))
        .await
        .expect("the serving login connects to another database");
    assert_eq!(
        refused(
            role_split_preflight(&owner, elsewhere.pool()).await,
            "two databases"
        ),
        RoleSplitReason::DifferentDatabases
    );
    owner.close().await;
    elsewhere.pool().close().await;
    drop_dbs(&created).await;
}

/// A serving role with an attribute d2 refuses, or a member of a role, is
/// refused before migrating (ADR-0034 d2, d5).
#[tokio::test]
async fn preflight_refuses_a_serving_role_with_createdb_or_a_membership_with_serving_role_privileged()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (name, store, owner) = split_pair(&created).await;
    role_split_preflight(&owner, store.pool())
        .await
        .expect("control: the minted pair passes");
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    for (grant, revoke) in [
        (
            format!("ALTER ROLE {name}_serving CREATEDB"),
            format!("ALTER ROLE {name}_serving NOCREATEDB"),
        ),
        (
            format!("GRANT {name}_owner TO {name}_serving"),
            format!("REVOKE {name}_owner FROM {name}_serving"),
        ),
    ] {
        admin.execute(grant.as_str()).await.expect("grant");
        assert_eq!(
            refused(role_split_preflight(&owner, store.pool()).await, &grant),
            RoleSplitReason::ServingRolePrivileged,
            "{grant}"
        );
        admin.execute(revoke.as_str()).await.expect("revoke");
    }
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// A serving role that owns the database, as the one role of every unsplit
/// install does, is refused before migrating (ADR-0034 d5, d7).
#[tokio::test]
async fn preflight_refuses_a_serving_role_that_owns_the_database_as_every_unsplit_install_does_with_serving_role_owns()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (name, store, owner) = split_pair(&created).await;
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("ALTER DATABASE {name} OWNER TO {name}_serving").as_str())
        .await
        .expect("the serving role takes the database");
    assert_eq!(
        refused(
            role_split_preflight(&owner, store.pool()).await,
            "the serving role owns the database"
        ),
        RoleSplitReason::ServingRoleOwns
    );
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// A migration that grants `TRUNCATE`, which row security does not bind,
/// is refused after migrating (ADR-0034 d3, d5).
#[tokio::test]
async fn postflight_refuses_a_migration_that_grants_truncate_with_serving_role_privileged() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (name, store, owner) = migrated_pair(&created).await;
    role_split_postflight(&owner, store.pool())
        .await
        .expect("control: the committed migrations pass");
    sqlx::query(&format!("GRANT TRUNCATE ON memories TO {name}_serving"))
        .execute(&owner)
        .await
        .expect("a migration grants TRUNCATE");
    assert_eq!(
        refused(
            role_split_postflight(&owner, store.pool()).await,
            "TRUNCATE granted"
        ),
        RoleSplitReason::ServingRolePrivileged
    );
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// Any privilege on `_sqlx_migrations`, a grant to `PUBLIC` among them, is
/// refused after migrating (ADR-0034 d5, d6).
#[tokio::test]
async fn postflight_refuses_any_privilege_on_sqlx_migrations_with_serving_role_reads_migrations() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (name, store, owner) = migrated_pair(&created).await;
    for privilege in [
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
        "TRUNCATE",
        "REFERENCES",
        "TRIGGER",
        "MAINTAIN",
    ] {
        for grantee in [format!("{name}_serving"), "PUBLIC".to_owned()] {
            sqlx::query(&format!(
                "GRANT {privilege} ON _sqlx_migrations TO {grantee}"
            ))
            .execute(&owner)
            .await
            .expect("grant on _sqlx_migrations");
            assert_eq!(
                refused(
                    role_split_postflight(&owner, store.pool()).await,
                    &format!("{privilege} to {grantee}")
                ),
                RoleSplitReason::ServingRoleReadsMigrations,
                "{privilege} to {grantee}"
            );
            sqlx::query(&format!(
                "REVOKE {privilege} ON _sqlx_migrations FROM {grantee}"
            ))
            .execute(&owner)
            .await
            .expect("revoke on _sqlx_migrations");
        }
    }
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// A serving login the migrations did not grant, as when `postgres.url`
/// names another role, is refused after migrating (ADR-0034 d5): here the
/// migrations granted the owner, the setting naming it.
#[tokio::test]
async fn postflight_refuses_a_serving_login_the_migrations_did_not_grant_with_serving_role_ungranted()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (name, store, owner) = split_pair(&created).await;
    let as_owner = PgStore::connect(&owner_url(&name))
        .await
        .expect("owner store");
    as_owner
        .migrate(&owner_url(&name))
        .await
        .expect("migrations granting the owner apply");
    assert_eq!(
        refused(
            role_split_postflight(&owner, store.pool()).await,
            "an ungranted serving login"
        ),
        RoleSplitReason::ServingRoleUngranted
    );
    as_owner.pool().close().await;
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// A table without `FORCE`, without a policy, or owned by a role other than
/// the owner is refused after migrating (ADR-0034 d1, d5).
#[tokio::test]
async fn postflight_refuses_a_table_without_force_or_a_policy_or_owned_by_another_role_with_table_unfenced()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (name, store, owner) = migrated_pair(&created).await;
    let mut admin = admin_in(&name).await;
    let fenced = "ENABLE ROW LEVEL SECURITY, FORCE ROW LEVEL SECURITY";
    let cases: [(&str, Vec<String>, Vec<String>, bool); 3] = [
        (
            "crews without FORCE",
            vec!["ALTER TABLE crews NO FORCE ROW LEVEL SECURITY".to_owned()],
            vec!["ALTER TABLE crews FORCE ROW LEVEL SECURITY".to_owned()],
            false,
        ),
        (
            "a table with no policy",
            vec![
                "CREATE TABLE unpoliced (id int)".to_owned(),
                format!("ALTER TABLE unpoliced {fenced}"),
            ],
            vec!["DROP TABLE unpoliced".to_owned()],
            false,
        ),
        (
            "a fenced table the superuser owns",
            vec![
                "CREATE TABLE foreign_owned (id int)".to_owned(),
                format!("ALTER TABLE foreign_owned {fenced}"),
                "CREATE POLICY foreign_owned_all ON foreign_owned USING (true)".to_owned(),
            ],
            vec!["DROP TABLE foreign_owned".to_owned()],
            true,
        ),
    ];
    for (what, make, unmake, as_admin) in cases {
        for statement in &make {
            if as_admin {
                admin.execute(statement.as_str()).await.expect(what);
            } else {
                sqlx::query(statement).execute(&owner).await.expect(what);
            }
        }
        assert_eq!(
            refused(role_split_postflight(&owner, store.pool()).await, what),
            RoleSplitReason::TableUnfenced,
            "{what}"
        );
        for statement in &unmake {
            if as_admin {
                admin.execute(statement.as_str()).await.expect(what);
            } else {
                sqlx::query(statement).execute(&owner).await.expect(what);
            }
        }
        role_split_postflight(&owner, store.pool())
            .await
            .unwrap_or_else(|e| panic!("control: undoing {what} passes: {e}"));
    }
    drop(admin);
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// A `SECURITY DEFINER` function in `public`, which would run a serving call
/// as the owner, is refused after migrating (ADR-0034 d5).
#[tokio::test]
async fn postflight_refuses_a_security_definer_function_in_public_with_definer_function() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (_name, store, owner) = migrated_pair(&created).await;
    sqlx::query(
        "CREATE FUNCTION public.as_owner() RETURNS int LANGUAGE sql SECURITY DEFINER \
         AS 'SELECT 1'",
    )
    .execute(&owner)
    .await
    .expect("the owner creates a definer function");
    assert_eq!(
        refused(
            role_split_postflight(&owner, store.pool()).await,
            "a definer function"
        ),
        RoleSplitReason::DefinerFunction
    );
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// `ltree`'s member objects, which 0006's `CREATE EXTENSION` run by the
/// owner leaves with the bootstrap superuser, are skipped by both checks
/// (ADR-0034 d2, d5): a migrated pair passes before and after migrating.
#[tokio::test]
async fn the_checks_accept_ltrees_extension_members_owned_by_the_bootstrap_superuser() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (_name, store, owner) = migrated_pair(&created).await;
    let foreign_members: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_depend d \
         JOIN pg_extension e ON e.oid = d.refobjid AND d.refclassid = 'pg_extension'::regclass \
         LEFT JOIN pg_proc p ON d.classid = 'pg_proc'::regclass AND p.oid = d.objid \
         LEFT JOIN pg_type t ON d.classid = 'pg_type'::regclass AND t.oid = d.objid \
         WHERE e.extname = 'ltree' AND d.deptype = 'e' \
           AND coalesce(p.proowner, t.typowner) IS DISTINCT FROM (SELECT oid FROM pg_roles WHERE rolname = current_user) \
           AND coalesce(p.proowner, t.typowner) IS NOT NULL",
    )
    .fetch_one(&owner)
    .await
    .expect("ltree's members");
    assert!(
        foreign_members > 0,
        "precondition: ltree's members are not the owner's"
    );
    role_split_postflight(&owner, store.pool())
        .await
        .expect("the checks after migrating accept ltree's members");
    role_split_preflight(&owner, store.pool())
        .await
        .expect("the checks before migrating accept ltree's members");
    owner.close().await;
    store.pool().close().await;
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
    if !splits().await {
        return;
    }
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
            covers: Covers::Subtree,
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
    if !splits().await {
        return;
    }
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
    let mut conn = PgConnection::connect(&owner_url(&name))
        .await
        .expect("connect");
    // 0007 and every later granting migration name the grantee through
    // oml.serving_role, as PgStore::migrate sets it (ADR-0034 d2).
    sqlx::query("SELECT set_config('oml.serving_role', $1, false)")
        .bind(format!("{name}_serving"))
        .execute(&mut conn)
        .await
        .expect("set the serving role");
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
    if !splits().await {
        return;
    }
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

    let store = PgStore::connect(&serving_url(&name))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&name));
    store
        .migrate(&owner_url(&name))
        .await
        .expect("0005 applies over PoC rows");
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
    if !splits().await {
        return;
    }
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

        let store = PgStore::connect(&serving_url(&name))
            .await
            .expect("connect")
            .with_owner_url(&owner_url(&name));
        let err = store
            .migrate(&owner_url(&name))
            .await
            .expect_err("a held first version beside a live one is refused");
        assert!(
            err.to_string()
                .contains("2 live first versions of claim_key deploy-runbook"),
            "beside a {first} first version, got: {err}"
        );
        let applied: Option<i64> =
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE version = 5")
                .fetch_optional(&mut owner_conn(&store).await)
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
    if !splits().await {
        return;
    }
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

    let store = PgStore::connect(&serving_url(&name))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&name));
    store
        .migrate(&owner_url(&name))
        .await
        .expect("0005 applies over PoC rows");
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
/// while a replaced version and another workarea's tip stand beside it;
/// the refusing index is the tenant partition's own, attached to it
/// (postgres-schema.md `memories_served_tip`; ADR-0034 d11).
#[tokio::test]
async fn the_served_tip_index_refuses_a_second_tip() {
    if !splits().await {
        return;
    }
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
    let violated = err
        .as_database_error()
        .and_then(|db| db.constraint())
        .unwrap_or_else(|| panic!("a constraint refusal, got: {err}"))
        .to_owned();
    drop(tx);
    // memories is list-partitioned by tenant (0009): the refusing index is
    // the tenant partition's, attached to memories_served_tip.
    let parent: String = sqlx::query_scalar(
        "SELECT i.inhparent::regclass::text FROM pg_inherits i \
          WHERE i.inhrelid = to_regclass($1)",
    )
    .bind(&violated)
    .fetch_one(store.pool())
    .await
    .unwrap_or_else(|e| panic!("{violated} is a partition index: {e}"));
    assert_eq!(parent, "memories_served_tip", "refused by {violated}");
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
    if !splits().await {
        return;
    }
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
    if !splits().await {
        return;
    }
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
    let mut owner = owner_conn(&store).await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query("ALTER TABLE api_keys NO FORCE ROW LEVEL SECURITY")
        .execute(&mut *tx)
        .await
        .expect("the owner lifts FORCE");
    let insert = "INSERT INTO api_keys (id, tenant_id, hash, credential, role, principal, \
                  issued_seq, name) \
                  VALUES ($1, NULL, $2, 'bearer', 'operator', 'provider/operator', 1, $3)";
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
    if !splits().await {
        return;
    }
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
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    store
        .migrate(&owner_url(&db))
        .await
        .expect("0006 applies over PoC rows");

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
    if !splits().await {
        return;
    }
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
            // 0020 adds search_default and 0023 telemetry_query_text_since,
            // null for every tenant they find
            // (ADR-0049 d4).
            (true, "tenants") => {
                "(to_jsonb(t) - 'name' - 'prior_name' - 'crew_id' - 'path' - 'search_default' \
                  - 'telemetry_query_text_since') \
                 || jsonb_build_object('name', t.prior_name)"
            }
            // Before 0017 a workarea's status is the one every build served,
            // derived from parent_id; v0.1.0 stored every node `development`,
            // and the boot stores the served one before 0017 (upgrades.md
            // "Upgrading from v0.5.1 or earlier").
            (false, "workareas") => {
                "to_jsonb(t) || jsonb_build_object('status', \
                 CASE WHEN t.parent_id IS NULL THEN 'committed' ELSE 'development' END)"
            }
            // 0014 adds user_root and namespace_root_id, folded as false and
            // null for every workarea it finds (ADR-0044 d12); 0028 adds
            // mode, standard for every workarea it finds (ADR-0030 d14).
            (true, "workareas") => {
                "(to_jsonb(t) - 'name' - 'prior_name' - 'crew_id' - 'path' - 'user_root' \
                 - 'namespace_root_id' - 'mode') || jsonb_build_object('name', t.prior_name)"
            }
            // 0012 adds credential, folded as bearer for every key it finds
            // (ADR-0031 d7); 0014 adds user_root_id, null for every key it
            // finds (ADR-0044 d12); 0015 adds session_for, null for every
            // key it finds (ADR-0045 d6); 0016 adds guardrail_scope, folded
            // as ancestors (proposed ADR-0037 d4); 0028 adds profile, null
            // for every key it finds (ADR-0030 d14).
            (true, "api_keys") => {
                "to_jsonb(t) - 'name' - 'crew_id' - 'credential' - 'user_root_id' \
                 - 'session_for' - 'guardrail_scope' - 'profile'"
            }
            // 0014 adds covers, subtree for every grant it finds (ADR-0044
            // d12).
            (true, "grants") => "to_jsonb(t) - 'covers'",
            // 0016 adds a guardrail's severity and rule, null on every other
            // kind (proposed ADR-0037 d10); 0020 adds the stored search
            // lexemes, generated from the claim (ADR-0049 d12); 0025 adds
            // the anchor and the latest source check, null on every row
            // written before it (ADR-0057 d1, d5); 0026 adds valid time, open
            // on every existing row (proposed ADR-0059 d11); 0029 adds the origin
            // kind, null on every row written before it and never backfilled
            // (ADR-0063 d2); 0032 adds the format, the title and the derived
            // search text, text/plain with none on every existing row
            // (ADR-0056 d1, d4).
            (true, "memories") => {
                "to_jsonb(t) - 'severity' - 'rule' - 'search_vector' - 'search_len' \
                 - 'anchor_repo' - 'anchor_uri' - 'anchor_rev' - 'anchor_path' \
                 - 'anchor_fragment' - 'anchor_hash' - 'anchor_hasher' - 'source_check_rev' \
                 - 'source_check_result' - 'source_check_hash' - 'source_check_path' \
                 - 'source_check_fragment' - 'source_check_at' \
                 - 'valid_from' - 'valid_until' - 'valid_from_us' - 'valid_until_us' \
                 - 'origin_kind' - 'format' - 'title' - 'search_text' - 'search_text_by'"
            }
            (true, "key_denials") => "to_jsonb(t) - 'lifted_epoch'",
            _ => "to_jsonb(t)",
        };
        // Row-level security does not bind a superuser, as CI's Postgres role
        // is, so the tenant is also named in the query: either role counts the
        // same rows.
        let owner = if table == "tenants" {
            "id"
        } else {
            "tenant_id"
        };
        let sql = format!("SELECT '{table} ' || ({row})::text FROM {table} t WHERE t.{owner} = $1");
        let texts: Vec<String> = sqlx::query_scalar(&sql)
            .bind(tenant)
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
    if !splits().await {
        return;
    }
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
    // v0.1.0 stored every node `development`, its root included.
    sqlx::query(
        "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
         VALUES ($1, $2, NULL, 'root', 'development')",
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
        .bind("development")
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
    for t in [tenant, twin] {
        let mut tx = conn.begin().await.expect("begin");
        sqlx::query(set_tenant)
            .bind(t.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let stored: String =
            sqlx::query_scalar("SELECT status FROM workareas WHERE parent_id IS NULL")
                .fetch_one(&mut *tx)
                .await
                .expect("the PoC root");
        tx.rollback().await.expect("rollback");
        assert_eq!(stored, "development", "the PoC stored its root development");
    }
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    store
        .migrate(&owner_url(&db))
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
             + (SELECT count(*) FROM crews) \
             + (SELECT count(*) FROM key_denials WHERE lifted_epoch IS NOT NULL)",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("named rows");
        tx.rollback().await.expect("rollback");
        assert_eq!(
            named, 0,
            "the migration names nothing, makes no crew and lifts no denial"
        );
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
/// PoC tenant row, each of its workarea rows and each of its keys has none,
/// while a tenant created since, with its `universe` root, a workarea and a
/// key issued under its name, reports nothing (ADR-0026 d9, d11; http-api.md
/// "Rows created before names").
#[tokio::test]
async fn rows_that_predate_names_are_reported_per_table_and_rows_created_since_are_not() {
    if !splits().await {
        return;
    }
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
    sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, hash, role, principal, held_by, issued_seq) \
         VALUES ($1, $2, $3, 'admin', 'oml/admin', 'human', 3)",
    )
    .bind(Uuid::from_u128(0xD700))
    .bind(poc)
    .bind(Sha256Hash::digest(b"poc admin key").0.to_vec())
    .execute(&mut *tx)
    .await
    .expect("PoC key");
    tx.commit().await.expect("commit the PoC rows");
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    store
        .migrate(&owner_url(&db))
        .await
        .expect("0006 applies over PoC rows");

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
    rig.issue_key(
        &store,
        KeyRecord {
            name: Some(name("agent-7")),
            ..key_record(t, key, Role::Writer, None)
        },
    )
    .await;

    assert_eq!(
        store
            .rows_without_names(&[poc, t])
            .await
            .expect("the check reads"),
        vec![("tenants", 1), ("workareas", 2), ("api_keys", 1)],
        "the PoC tenant, both its workareas and its key, and nothing created since"
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

/// The lead's naming over a database migration 0006 left: one
/// `system.names_assigned` on the chain, whose fold alone fills the tenant's,
/// each workarea's and each key's name, makes the crew records and points
/// the workareas at them, and derives every path; the journal still
/// verifies as signed, every workarea is served again, and a second run
/// journals nothing (http-api.md rows created before names; ADR-0026 d11).
#[tokio::test]
async fn the_naming_fills_the_names_crews_and_paths_of_a_migrated_poc_database_once() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 5).await;
    let tenant = Uuid::from_u128(0xA900);
    let id = |n: u128| Uuid::from_u128(0xB900 + n);
    let (root, procurement, team, again) = (id(0), id(1), id(2), id(3));
    let (admin, writer) = (Uuid::from_u128(0xD900), Uuid::from_u128(0xD901));
    let crew = json!({ "name": "Procurement Crew", "external_manager": "crewd.example.com" });
    let created_wa = |w: Uuid, parent: Option<Uuid>, wa_name: &str, crew: &Value| {
        json!({
            "workarea_id": w, "parent_id": parent, "name": wa_name,
            "status": if parent.is_none() { "committed" } else { "development" },
            "crew": crew, "prefix": null,
        })
    };
    let workareas = [
        (root, None, "root", Value::Null),
        (procurement, Some(root), "procurement", crew.clone()),
        (team, Some(root), "Procurement Team", crew.clone()),
        (again, Some(team), "procurement", Value::Null),
    ];
    let mut drafts = vec![(event_types::TENANT_GENESIS, json!({ "tenant_name": "oml" }))];
    for (w, parent, wa_name, crew) in &workareas {
        drafts.push((
            event_types::WORKAREA_CREATE,
            created_wa(*w, *parent, wa_name, crew),
        ));
    }
    drafts.push((
        event_types::KEY_ISSUED,
        json!({ "key_id": admin, "role": "admin", "principal": "oml/admin" }),
    ));
    drafts.push((
        event_types::KEY_ISSUED,
        json!({ "key_id": writer, "role": "writer", "principal": "crew/agent-7" }),
    ));
    let mut head: Option<SignedEnvelope> = None;
    let mut events = Vec::new();
    for (n, (event_type, payload)) in drafts.into_iter().enumerate() {
        let n = u64::try_from(n).expect("seq");
        let draft = EventDraft {
            tenant_id: tenant,
            event_id: Uuid::from_u128(0xE900 + u128::from(n)),
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

    // The rows the PoC folded from that chain, at the pre-0006 schema.
    let mut tx = conn.begin().await.expect("begin");
    poc_tenant(&mut tx, tenant).await;
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
    }
    let last = head.expect("the chain has a head");
    sqlx::query("INSERT INTO journal_chain_head (tenant_id, seq, head_hash) VALUES ($1, $2, $3)")
        .bind(tenant)
        .bind(i64::try_from(last.envelope.seq).expect("seq"))
        .bind(last.hash.0.to_vec())
        .execute(&mut *tx)
        .await
        .expect("chain head");
    for (w, parent, wa_name, crew) in &workareas {
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
        .bind((!crew.is_null()).then(|| crew.clone()))
        .execute(&mut *tx)
        .await
        .expect("workarea row");
    }
    for (key_id, role, principal, seq) in [
        (admin, "admin", "oml/admin", 6_i64),
        (writer, "writer", "crew/agent-7", 7),
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
    tx.commit().await.expect("commit the PoC rows");
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    store.migrate(&owner_url(&db)).await.expect("0006 applies");

    let app = App::new(
        store.clone(),
        MemSigner::new(oml_port_contracts::TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let assigned = app
        .name_rows_that_predate_names(&[tenant])
        .await
        .expect("the naming folds");
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0].journal_seq, last.envelope.seq + 1);
    let crews = &assigned[0].naming.crews;
    assert_eq!(crews.len(), 1, "one record per distinct crew object");
    let crew_id = crews[0].crew_id;
    let derived = |kind: NamedKind, id: Uuid| Name::derived(kind, id).as_str().to_owned();

    // Every row reads back with its name, path and crew, through the port.
    assert!(store.unnamed_rows(tenant).await.expect("read").is_empty());
    let named: Vec<(String, Option<Uuid>)> = {
        let mut rows = Vec::new();
        for w in [root, procurement, team, again] {
            let record = store
                .workarea(tenant, w)
                .await
                .expect("a named workarea is served")
                .expect("the node");
            rows.push((record.name.as_str().to_owned(), record.crew_id));
        }
        rows
    };
    assert_eq!(
        named,
        vec![
            ("universe".to_owned(), None),
            ("procurement".to_owned(), Some(crew_id)),
            (derived(NamedKind::Workarea, team), Some(crew_id)),
            (derived(NamedKind::Workarea, again), None),
        ]
    );
    let mut tx = conn.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let paths: Vec<String> =
        sqlx::query_scalar("SELECT path::text FROM workareas WHERE tenant_id = $1 ORDER BY id")
            .bind(tenant)
            .fetch_all(&mut *tx)
            .await
            .expect("paths");
    let tenant_name: Option<String> =
        sqlx::query_scalar("SELECT name::text FROM tenants WHERE id = $1")
            .bind(tenant)
            .fetch_one(&mut *tx)
            .await
            .expect("the tenant's name");
    tx.rollback().await.expect("rollback");
    let team_path = format!("universe.{}", derived(NamedKind::Workarea, team));
    assert_eq!(
        paths,
        vec![
            "universe".to_owned(),
            "universe.procurement".to_owned(),
            team_path.clone(),
            format!("{team_path}.{}", derived(NamedKind::Workarea, again)),
        ]
    );
    assert_eq!(tenant_name.as_deref(), Some("oml"));
    let record = store
        .crew(tenant, crew_id)
        .await
        .expect("read")
        .expect("the crew record");
    assert_eq!(record.name.as_str(), derived(NamedKind::Crew, crew_id));
    assert_eq!(
        record.external_manager.as_deref(),
        Some("crewd.example.com")
    );
    for key in [admin, writer] {
        let key_name = Name::parse_reference(&derived(NamedKind::Key, key)).expect("a name");
        assert_eq!(
            store
                .resolve_name(tenant, NamedKind::Key, &key_name)
                .await
                .expect("resolve"),
            Some(key)
        );
    }

    let keys = KeySet::new(vec![test_signing_key().verifying_key()]);
    let verified = store
        .verify_stored_chain(tenant, &keys)
        .await
        .expect("the named chain verifies");
    assert_eq!(verified.seq, last.envelope.seq + 1);

    // A second run finds nothing to name and journals nothing.
    assert!(
        app.name_rows_that_predate_names(&[tenant])
            .await
            .expect("the second run")
            .is_empty()
    );
    assert_eq!(
        store.chain_head(tenant).await.expect("head").map(|h| h.seq),
        Some(verified.seq)
    );
    drop(conn);
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// Keys into tenant tables carry tenant_id (migrations/0008_tenant_keys.sql)
// ---------------------------------------------------------------------------

/// Runs `statement`, which sets one column of a tenant's row to `$1` where
/// its key is `$2`, over the serving login under `tenant`, in a transaction
/// that rolls back; the refusal's SQLSTATE and constraint, or the rows it
/// changed.
async fn reference_under(
    store: &PgStore,
    tenant: Uuid,
    statement: &str,
    target: Uuid,
    row: Uuid,
) -> Result<u64, (String, Option<String>)> {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let result = sqlx::query(statement)
        .bind(target)
        .bind(row)
        .execute(&mut *tx)
        .await;
    tx.rollback().await.expect("rollback");
    match result {
        Ok(done) => Ok(done.rows_affected()),
        Err(e) => {
            let db = e
                .as_database_error()
                .unwrap_or_else(|| panic!("not a database error: {e}"));
            Err((
                db.code().map(|c| c.into_owned()).unwrap_or_default(),
                db.constraint().map(str::to_owned),
            ))
        }
    }
}

/// The id of the journal event that created `memory` under `tenant`.
async fn created_event(store: &PgStore, tenant: Uuid, memory: Uuid) -> Uuid {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let event: Uuid = sqlx::query_scalar("SELECT created_event_id FROM memories WHERE id = $1")
        .bind(memory)
        .fetch_one(&mut *tx)
        .await
        .expect("the memory's event");
    tx.rollback().await.expect("rollback");
    event
}

/// Commits a held memory under `rig`'s tenant, with its review_queue row
/// when `enqueue` is set.
async fn hold(
    store: &PgStore,
    rig: &mut ChainRig,
    workarea: Uuid,
    memory: Uuid,
    author: Uuid,
    enqueue: bool,
) {
    rig.commit(
        store,
        event_types::SUBMISSION_REMEMBER,
        json!({ "artifact_id": memory.to_string() }),
        MetadataFold::MemoryHeld {
            memory: held_memory(rig.tenant_id, workarea, memory, "wanda", author, "held"),
            enqueue,
        },
        None,
    )
    .await
    .expect("commit held");
}

/// Under tenant A, each reference 0008 rewrites refuses tenant B's row as it
/// refuses an id no row holds: `23503`, from the one constraint, so the
/// reference neither stores a cross-tenant id nor answers whether another
/// tenant holds one; tenant A's own row is accepted. PostgreSQL runs a
/// foreign-key check without row-level security, so the key carries
/// `tenant_id` (postgres-schema.md "Keys into tenant tables"; ADR-0034 d12,
/// d10 step 3).
#[tokio::test]
async fn under_tenant_a_a_reference_to_tenant_bs_row_fails_as_one_to_a_random_id_does() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut a = ChainRig::new(180);
    a.seed_chain(&store, 0).await;
    let mut b = ChainRig::new(181);
    b.seed_chain(&store, 0).await;
    let (a_root, a_child, b_root) = (
        Uuid::from_u128(0xB180),
        Uuid::from_u128(0xB181),
        Uuid::from_u128(0xB190),
    );
    a.create_workarea(
        &store,
        workarea_record(a.tenant_id, a_root, None, "universe"),
    )
    .await;
    a.create_workarea(
        &store,
        workarea_record(a.tenant_id, a_child, Some(a_root), "child"),
    )
    .await;
    b.create_workarea(
        &store,
        workarea_record(b.tenant_id, b_root, None, "universe"),
    )
    .await;
    let (a_key, b_key) = (Uuid::from_u128(0xD180), Uuid::from_u128(0xD190));
    a.issue_key(&store, key_record(a.tenant_id, a_key, Role::Writer, None))
        .await;
    b.issue_key(&store, key_record(b.tenant_id, b_key, Role::Writer, None))
        .await;
    let (a_memory, b_memory) = (Uuid::from_u128(0xC180), Uuid::from_u128(0xC190));
    // Tenant B's memory holds no review_queue row: review_queue's primary key
    // is the global memory_id, so moving tenant A's queue row onto a queued B
    // memory fails the unique check (23505) before the foreign key's AFTER
    // trigger runs, and the test would not reach the reference it proves.
    hold(&store, &mut a, a_child, a_memory, a_key, true).await;
    hold(&store, &mut b, b_root, b_memory, b_key, false).await;
    let a_event = created_event(&store, a.tenant_id, a_memory).await;
    let b_event = created_event(&store, b.tenant_id, b_memory).await;

    // Through the adapter: the 23503 is the backend's refusal, word for word
    // the one an id no row holds gets.
    let mut refusals = Vec::new();
    for (stray, parent) in [(0xB182, b_root), (0xB183, Uuid::from_u128(0x0BAD_B183))] {
        let stray = Uuid::from_u128(stray);
        refusals.push(
            a.commit(
                &store,
                event_types::WORKAREA_CREATE,
                json!({ "workarea_id": stray.to_string(), "name": "stray" }),
                MetadataFold::WorkareaCreated(workarea_record(
                    a.tenant_id,
                    stray,
                    Some(parent),
                    "stray",
                )),
                None,
            )
            .await,
        );
    }
    assert!(
        matches!(&refusals[0], Err(StoreError::Backend(m)) if m.contains("workareas_parent_id_fkey")),
        "a workarea under tenant B's node is the backend's refusal, got {:?}",
        refusals[0]
    );
    assert_eq!(refusals[0], refusals[1]);

    // (constraint, the statement over tenant A's row, that row, tenant A's
    // own target, tenant B's)
    let keys = [
        (
            "workareas_parent_id_fkey",
            "UPDATE workareas SET parent_id = $1 WHERE id = $2",
            a_child,
            a_root,
            b_root,
        ),
        (
            "memories_created_event_id_fkey",
            "UPDATE memories SET created_event_id = $1 WHERE id = $2",
            a_memory,
            a_event,
            b_event,
        ),
        (
            "review_queue_memory_id_fkey",
            "UPDATE review_queue SET memory_id = $1 WHERE memory_id = $2",
            a_memory,
            a_memory,
            b_memory,
        ),
        (
            "api_keys_default_workarea_id_fkey",
            "UPDATE api_keys SET default_workarea_id = $1 WHERE id = $2",
            a_key,
            a_root,
            b_root,
        ),
        (
            "api_keys_rotated_from_fkey",
            "UPDATE api_keys SET rotated_from = $1 WHERE id = $2",
            a_key,
            a_key,
            b_key,
        ),
    ];
    for (constraint, statement, row, own, other) in keys {
        assert_eq!(
            reference_under(&store, a.tenant_id, statement, own, row).await,
            Ok(1),
            "{constraint}: tenant A's own row is accepted"
        );
        let random = reference_under(
            &store,
            a.tenant_id,
            statement,
            Uuid::from_u128(0x0BAD_0008),
            row,
        )
        .await;
        let foreign = reference_under(&store, a.tenant_id, statement, other, row).await;
        assert_eq!(
            random,
            Err(("23503".to_owned(), Some(constraint.to_owned()))),
            "{constraint}: an id no row holds is refused by the one constraint"
        );
        assert_eq!(
            foreign, random,
            "{constraint}: tenant B's row is refused as an id no row holds is"
        );
    }
    drop_dbs(&created).await;
}

/// Every foreign key into a table that has a `tenant_id` column pairs the
/// referencing row's `tenant_id` with the referenced row's, read off
/// `pg_constraint`; the provider-level references `journal.api_key_id` and
/// `api_keys.issued_by` hold no foreign key at all (ADR-0034 d12, d10 step
/// 3).
#[tokio::test]
async fn pg_constraint_shows_every_foreign_key_into_a_table_with_tenant_id_carrying_tenant_id() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT c.conrelid::regclass::text, c.conname::text, \
                EXISTS (SELECT 1 FROM unnest(c.conkey, c.confkey) AS k (l, r) \
                          JOIN pg_attribute la ON la.attrelid = c.conrelid AND la.attnum = k.l \
                          JOIN pg_attribute ra ON ra.attrelid = c.confrelid AND ra.attnum = k.r \
                         WHERE la.attname = 'tenant_id' AND ra.attname = 'tenant_id') \
           FROM pg_constraint c \
          WHERE c.contype = 'f' \
            AND c.connamespace = 'public'::regnamespace \
            AND EXISTS (SELECT 1 FROM pg_attribute a \
                         WHERE a.attrelid = c.confrelid AND a.attname = 'tenant_id' \
                           AND NOT a.attisdropped) \
          ORDER BY 1, 2",
    )
    .fetch_all(store.pool())
    .await
    .expect("the foreign keys into tenant tables");
    let lacking: Vec<String> = rows
        .iter()
        .filter(|(_, _, carries)| !carries)
        .map(|(table, name, _)| format!("{table}.{name}"))
        .collect();
    assert_eq!(lacking, Vec::<String>::new(), "keys without tenant_id");
    let carrying: Vec<&str> = rows.iter().map(|(_, name, _)| name.as_str()).collect();
    for rewritten in [
        "workareas_parent_id_fkey",
        "memories_created_event_id_fkey",
        "review_queue_memory_id_fkey",
        "api_keys_default_workarea_id_fkey",
        "api_keys_rotated_from_fkey",
    ] {
        assert!(
            carrying.contains(&rewritten),
            "{rewritten} is among the keys read, got {carrying:?}"
        );
    }
    let provider_level: Vec<String> = sqlx::query_scalar(
        "SELECT c.conname::text FROM pg_constraint c \
          WHERE c.contype = 'f' \
            AND ((c.conrelid = 'journal'::regclass \
                  AND c.conkey @> ARRAY[(SELECT attnum FROM pg_attribute \
                                          WHERE attrelid = 'journal'::regclass \
                                            AND attname = 'api_key_id')]) \
              OR (c.conrelid = 'api_keys'::regclass \
                  AND c.conkey @> ARRAY[(SELECT attnum FROM pg_attribute \
                                          WHERE attrelid = 'api_keys'::regclass \
                                            AND attname = 'issued_by')]))",
    )
    .fetch_all(store.pool())
    .await
    .expect("the provider-level references' keys");
    assert_eq!(provider_level, Vec::<String>::new());
    drop_dbs(&created).await;
}

/// An operator key, whose `tenant_id` is NULL, issues a tenant's first admin
/// key: the `key.issued` event names it as `api_key_id` and the key's row as
/// `issued_by`, neither of which a key carrying `tenant_id` could match, so
/// 0008 drops both foreign keys, and a provider-level row's own references
/// are not required to match (MATCH SIMPLE) (http-api.md; ADR-0034 d12).
#[tokio::test]
async fn an_operator_key_with_tenant_id_null_still_issues_a_tenants_first_admin_key() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let operator = Uuid::from_u128(0xD1A0);
    // Provider-level rows are the operator path's, which is not built; the
    // owner writes this one with FORCE lifted and restored inside its one
    // transaction (postgres-schema.md row-level security).
    let mut owner = owner_conn(&store).await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query("ALTER TABLE api_keys NO FORCE ROW LEVEL SECURITY")
        .execute(&mut *tx)
        .await
        .expect("the owner lifts FORCE");
    sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, hash, credential, role, principal, issued_seq, \
         rotated_from) VALUES ($1, NULL, $2, 'bearer', 'operator', 'provider/operator', 1, $3)",
    )
    .bind(operator)
    .bind(vec![0xA0_u8; 32])
    .bind(Uuid::from_u128(0x0BAD_D1A0))
    .execute(&mut *tx)
    .await
    .expect("an operator key whose rotated_from no row holds: MATCH SIMPLE skips a NULL tenant");
    sqlx::query("ALTER TABLE api_keys FORCE ROW LEVEL SECURITY")
        .execute(&mut *tx)
        .await
        .expect("the owner restores FORCE");
    tx.commit().await.expect("commit the operator key");
    drop(owner);

    let tenant = Uuid::from_u128(0xA000_01A0);
    let admin = Uuid::from_u128(0xD1A1);
    let key = test_signing_key();
    let mut head: Option<SignedEnvelope> = None;
    let mut stamps = 0_u32;
    for (event_type, payload, fold) in [
        (
            event_types::TENANT_GENESIS,
            json!({ "tenant_name": "first-admin" }),
            MetadataFold::TenantGenesis {
                name: name("first-admin"),
            },
        ),
        (
            event_types::KEY_ISSUED,
            json!({ "key_id": admin.to_string(), "value_disclosed": false }),
            MetadataFold::KeyIssued(key_record(tenant, admin, Role::Admin, Some(operator))),
        ),
    ] {
        stamps += 1;
        let draft = EventDraft {
            tenant_id: tenant,
            event_id: Uuid::from_u128(0xE1A0 + u128::from(stamps)),
            event_type: event_type.to_owned(),
            principal: "provider/operator".to_owned(),
            api_key_id: Some(operator),
            at: EventTime::parse(&format!("2026-11-03T12:00:0{stamps}.000000Z")).expect("time"),
            payload,
        };
        let envelope = next_envelope(head.as_ref(), draft).expect("a lawful draft");
        let event = sign(envelope, &key).expect("sign");
        store
            .commit(CommitRequest {
                event: event.clone(),
                fold,
                outbox: OutboxRow {
                    event_id: event.envelope.event_id,
                    topic: event.envelope.event_type.clone(),
                    payload: json!({ "seq": event.envelope.seq }),
                },
                idempotency: None,
            })
            .await
            .expect("the operator's commit");
        head = Some(event);
    }

    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let issued_by: Option<Uuid> =
        sqlx::query_scalar("SELECT issued_by FROM api_keys WHERE id = $1")
            .bind(admin)
            .fetch_one(&mut *tx)
            .await
            .expect("the admin key");
    let authors: Vec<Option<Uuid>> =
        sqlx::query_scalar("SELECT api_key_id FROM journal ORDER BY seq")
            .fetch_all(&mut *tx)
            .await
            .expect("the tenant's events");
    tx.rollback().await.expect("rollback");
    assert_eq!(issued_by, Some(operator));
    assert_eq!(authors, vec![Some(operator), Some(operator)]);
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// Per-tenant FTS partitions (migrations/0009_memories_partitions.sql;
// ADR-0034 d11, d13, d10 step 4)
// ---------------------------------------------------------------------------

/// The partition `memories` holds `tenant`'s rows in, as 0009 and
/// `create_tenant_partitions` name it: its id in 32 hex digits, never its
/// name (ADR-0034 d11).
fn partition_of(tenant: Uuid) -> String {
    format!("memories_t_{}", tenant.simple())
}

/// How `partition`'s catalog rows read: (a partition of `memories`, owned
/// by `owner`, `ENABLE` and `FORCE ROW LEVEL SECURITY`, policies), or
/// `None` when no such relation exists.
async fn partition_fence(
    conn: &mut PgConnection,
    partition: &str,
    owner: &str,
) -> Option<(bool, bool, bool, bool, i64)> {
    sqlx::query_as(
        "SELECT c.relispartition \
                AND EXISTS (SELECT 1 FROM pg_inherits i \
                             WHERE i.inhrelid = c.oid AND i.inhparent = 'memories'::regclass), \
                pg_get_userbyid(c.relowner) = $2, c.relrowsecurity, c.relforcerowsecurity, \
                (SELECT count(*) FROM pg_policy p WHERE p.polrelid = c.oid) \
           FROM pg_class c \
          WHERE c.relnamespace = 'public'::regnamespace AND c.relname = $1",
    )
    .bind(partition)
    .bind(owner)
    .fetch_optional(conn)
    .await
    .expect("the partition's catalog row")
}

/// The owner role of `store`'s role pair.
async fn owner_role(store: &PgStore) -> String {
    let db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(store.pool())
        .await
        .expect("database name");
    format!("{db}_owner")
}

/// How many `tenants` rows hold `tenant`, read over the serving login with
/// `tenant` set.
async fn tenant_rows(store: &PgStore, tenant: Uuid) -> i64 {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tenants WHERE id = $1")
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .expect("tenant rows");
    tx.rollback().await.expect("rollback");
    n
}

/// The owner creates a tenant's partition before the commit whose fold
/// creates its row: once `create_tenant_partitions` returns, the partition
/// is attached to `memories`, owned by the owner, fenced with `ENABLE` and
/// `FORCE ROW LEVEL SECURITY` and a policy, while no tenant row exists;
/// the genesis commit then creates the row, and the tenant's memories land
/// in its partition (ADR-0034 d11, d10 step 4).
#[tokio::test]
async fn a_tenants_fenced_partition_exists_before_its_row_commits() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let owner = owner_role(&store).await;
    let tenant = Uuid::from_u128(0xA000_0000 + 200);
    let mut conn = owner_conn(&store).await;
    assert_eq!(
        partition_fence(&mut conn, &partition_of(tenant), &owner).await,
        None,
        "no partition before the call"
    );

    store
        .create_tenant_partitions(tenant)
        .await
        .expect("the owner creates the partition");
    assert_eq!(
        partition_fence(&mut conn, &partition_of(tenant), &owner).await,
        Some((true, true, true, true, 1)),
        "attached, the owner's, ENABLE and FORCE, one policy"
    );
    assert_eq!(tenant_rows(&store, tenant).await, 0, "no tenant row yet");

    let mut rig = ChainRig::new(200);
    assert_eq!(rig.tenant_id, tenant);
    // The rig's own genesis would ask for the partition again; this one
    // commits the row onto the partition the call above created.
    let first = rig.next_event(
        event_types::TENANT_GENESIS,
        json!({ "tenant_name": rig.tenant_name.as_str() }),
    );
    store
        .commit(CommitRequest {
            event: first.clone(),
            fold: rig.genesis_fold(),
            outbox: OutboxRow {
                event_id: first.envelope.event_id,
                topic: first.envelope.event_type.clone(),
                payload: json!({ "seq": 1 }),
            },
            idempotency: None,
        })
        .await
        .expect("the genesis commits");
    assert_eq!(tenant_rows(&store, tenant).await, 1, "the row committed");
    drop(conn);
    drop_dbs(&created).await;
}

/// A bootstrap whose genesis commit fails after its partition committed
/// leaves that partition: empty, the owner's and fenced, so the checks
/// after migrating accept it, and the retry mints a fresh id with a
/// partition of its own (ADR-0034 d5, d11).
#[tokio::test]
async fn a_partition_left_by_a_failed_genesis_commit_is_empty_fenced_and_accepted_by_the_d5_checks()
{
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (_name, store, owner) = migrated_pair(&created).await;
    let owner_name = owner_role(&store).await;
    let app = App::new(
        store.clone(),
        MemSigner::new(oml_port_contracts::TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let request = BootstrapRequest {
        tenant_name: name("acme"),
        admin_key_name: name("root-admin"),
        admin_principal: "alice-admin".to_owned(),
    };
    let first = app
        .bootstrap_tenant(&request)
        .await
        .expect("the first tenant");
    let refused = app
        .bootstrap_tenant(&request)
        .await
        .expect_err("a second tenant under the name is refused at its genesis commit");
    assert!(matches!(refused, AppError::NameTaken { .. }), "{refused:?}");

    let partitions: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_inherits i JOIN pg_class c ON c.oid = i.inhrelid \
          WHERE i.inhparent = 'memories'::regclass ORDER BY 1",
    )
    .fetch_all(&owner)
    .await
    .expect("the partitions");
    assert_eq!(partitions.len(), 2, "{partitions:?}");
    let orphan = partitions
        .iter()
        .find(|p| **p != partition_of(first.tenant_id))
        .expect("the partition the refused genesis left")
        .clone();
    let orphan_id = Uuid::parse_str(orphan.trim_start_matches("memories_t_")).expect("a hex id");
    assert_eq!(tenant_rows(&store, orphan_id).await, 0, "no tenant row");

    let mut conn = owner_conn(&store).await;
    assert_eq!(
        partition_fence(&mut conn, &orphan, &owner_name).await,
        Some((true, true, true, true, 1)),
        "the orphan is the owner's and fenced"
    );
    sqlx::query("SELECT set_config('oml.tenant_id', $1, false)")
        .bind(orphan_id.to_string())
        .execute(&mut conn)
        .await
        .expect("the orphan's tenant set");
    let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {orphan}"))
        .fetch_one(&mut conn)
        .await
        .expect("the orphan's rows");
    assert_eq!(rows, 0, "the orphan is empty");
    drop(conn);
    role_split_postflight(&owner, store.pool())
        .await
        .expect("the checks after migrating accept the orphan");

    let retry = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme-two"),
            ..request
        })
        .await
        .expect("the retry");
    assert_ne!(retry.tenant_id, orphan_id, "the retry mints a fresh id");
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// Every relation an `EXPLAIN (FORMAT JSON)` plan names.
fn plan_relations(plan: &Value, found: &mut std::collections::BTreeSet<String>) {
    match plan {
        Value::Object(node) => {
            if let Some(Value::String(relation)) = node.get("Relation Name") {
                found.insert(relation.clone());
            }
            for value in node.values() {
                plan_relations(value, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                plan_relations(item, found);
            }
        }
        _ => {}
    }
}

/// The adapter's FTS reads, their statement text as the adapter runs it,
/// planned under tenant A over the serving login, scan tenant A's partition
/// alone: pruned at planning under a custom plan, and at executor start
/// under a generic one (ADR-0034 d11).
#[tokio::test]
async fn the_adapters_fts_read_scans_its_tenants_partition_alone() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut tenants = Vec::new();
    for seed in [210_u64, 211] {
        let mut rig = ChainRig::new(seed);
        rig.seed_chain(&store, 0).await;
        tenants.push(rig.tenant_id);
    }
    let a = tenants[0];
    let readable = Uuid::from_u128(0xB210);
    let author = Uuid::from_u128(0xD210);
    let mut conn = store.pool().acquire().await.expect("serving connection");
    for (statement, types, args) in [
        (
            oml_pg::RECALL_FTS,
            "uuid, uuid[], text, text[], bigint",
            format!("'{a}', '{{{}}}', 'runbook', '{{memory}}', 10", readable),
        ),
        (
            oml_pg::RECALL_OWN_HELD,
            "uuid, uuid[], uuid[], text, text[], bigint",
            format!(
                "'{a}', '{{{}}}', '{{{}}}', 'runbook', '{{memory}}', 10",
                author, readable
            ),
        ),
    ] {
        for mode in ["force_custom_plan", "force_generic_plan"] {
            let mut tx = conn.begin().await.expect("begin");
            sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
                .bind(a.to_string())
                .execute(&mut *tx)
                .await
                .expect("set tenant");
            tx.execute(format!("SET LOCAL plan_cache_mode = {mode}").as_str())
                .await
                .expect("plan cache mode");
            tx.execute(format!("PREPARE oml_fts_probe ({types}) AS {statement}").as_str())
                .await
                .expect("prepare the adapter's statement");
            let plan: Value = sqlx::query_scalar(&format!(
                "EXPLAIN (FORMAT JSON) EXECUTE oml_fts_probe ({args})"
            ))
            .fetch_one(&mut *tx)
            .await
            .expect("explain");
            // A prepared statement outlives its transaction.
            tx.execute("DEALLOCATE oml_fts_probe")
                .await
                .expect("deallocate");
            tx.rollback().await.expect("rollback");
            let mut relations = std::collections::BTreeSet::new();
            plan_relations(&plan, &mut relations);
            assert_eq!(
                relations.into_iter().collect::<Vec<_>>(),
                vec![partition_of(a)],
                "{mode}: {plan}"
            );
        }
    }
    drop(conn);
    drop_dbs(&created).await;
}

/// The partitions of `memories` an `EXPLAIN (FORMAT JSON)` plan scans or
/// updates; an UPDATE's ModifyTable names the parent itself, and its
/// target partitions under "Target Tables".
fn plan_memories_partitions(plan: &Value) -> Vec<String> {
    let mut relations = std::collections::BTreeSet::new();
    plan_relations(plan, &mut relations);
    relations
        .into_iter()
        .filter(|r| r.starts_with("memories_t_"))
        .collect()
}

/// The `memories` partitions an `EXPLAIN (FORMAT JSON)` plan scans: every
/// plan node's relation, the UPDATE's "Target Tables" left out.
fn plan_memories_scanned(plan: &Value) -> Vec<String> {
    fn walk(plan: &Value, found: &mut std::collections::BTreeSet<String>) {
        match plan {
            Value::Object(node) => {
                if node.get("Node Type").and_then(Value::as_str) != Some("ModifyTable")
                    && let Some(Value::String(relation)) = node.get("Relation Name")
                {
                    found.insert(relation.clone());
                }
                for (key, value) in node {
                    if key != "Target Tables" {
                        walk(value, found);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, found)),
            _ => {}
        }
    }
    let mut found = std::collections::BTreeSet::new();
    walk(plan, &mut found);
    found
        .into_iter()
        .filter(|r| r.starts_with("memories_t_"))
        .collect()
}

/// Each scan of a `memories` partition in an `EXPLAIN (FORMAT JSON)`
/// plan, as its alias and the conditions it carries ("Index Cond",
/// "Recheck Cond" and "Filter", joined).
fn plan_memories_scans(plan: &Value, found: &mut Vec<(String, String)>) {
    match plan {
        Value::Object(node) => {
            if let (Some(Value::String(relation)), Some(_)) =
                (node.get("Relation Name"), node.get("Node Type"))
                && relation.starts_with("memories_t_")
            {
                let conditions = ["Index Cond", "Recheck Cond", "Filter"]
                    .iter()
                    .filter_map(|c| node.get(*c).and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                let alias = node.get("Alias").and_then(Value::as_str).unwrap_or("");
                found.push((alias.to_owned(), conditions));
            }
            for value in node.values() {
                plan_memories_scans(value, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                plan_memories_scans(item, found);
            }
        }
        _ => {}
    }
}

/// A claim edge's lineage widening names its tenant on every statement on
/// `memories`: planned under tenant A over the serving login, the
/// adapter's statement scans tenant A's partition alone under a custom plan
/// and a generic one, planned and executed, and its custom plan, the one
/// the adapter runs it under, targets that partition alone; each scan of it
/// under the custom plan carries the bound tenant, not row security's
/// alone; run through the proposal fold, it widens tenant A's rows
/// and leaves tenant B's rows with the same ids, the same authors and the
/// same derivation exactly as they were held (ADR-0034 d11, d13; ADR-0001
/// d2).
#[tokio::test]
async fn the_widening_statement_names_its_tenant_and_touches_no_other_tenants_rows() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let (x, y, z) = (
        Uuid::from_u128(0xC230_0001),
        Uuid::from_u128(0xC230_0002),
        Uuid::from_u128(0xC230_0003),
    );
    let (ann, bob) = (Uuid::from_u128(0xD230_000A), Uuid::from_u128(0xD230_000B));
    let workarea = Uuid::from_u128(0xB230);
    let mut rigs = Vec::new();
    let mut held = Vec::new();
    for seed in [230_u64, 231] {
        let mut rig = ChainRig::new(seed);
        rig.seed_chain(&store, 0).await;
        let tenant = rig.tenant_id;
        let proposer = key_record(
            tenant,
            Uuid::from_u128((u128::from(seed) << 32) | 0x02),
            Role::Dreamer,
            None,
        );
        rig.issue_key(&store, proposer.clone()).await;
        let mut child = held_memory(tenant, workarea, z, "carol", ann, "child");
        child.derives_from = vec![x];
        child.lineage_principals = vec!["ann".to_owned(), "carol".to_owned()];
        for memory in [
            held_memory(tenant, workarea, x, "ann", ann, "source"),
            held_memory(tenant, workarea, y, "bob", bob, "target"),
            child,
        ] {
            rig.commit(
                &store,
                event_types::SUBMISSION_REMEMBER,
                json!({ "artifact_id": memory.memory_id.to_string() }),
                MetadataFold::MemoryHeld {
                    memory: memory.clone(),
                    enqueue: false,
                },
                None,
            )
            .await
            .expect("commit held");
            held.push(memory);
        }
        rigs.push((rig, proposer));
    }
    let (a, b) = (rigs[0].0.tenant_id, rigs[1].0.tenant_id);

    let mut conn = store.pool().acquire().await.expect("serving connection");
    for mode in ["force_custom_plan", "force_generic_plan"] {
        let mut tx = conn.begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(a.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        tx.execute(format!("SET LOCAL plan_cache_mode = {mode}").as_str())
            .await
            .expect("plan cache mode");
        tx.execute(
            format!(
                "PREPARE oml_widen_probe (uuid, uuid, uuid) AS {}",
                oml_pg::WIDEN_LINEAGE
            )
            .as_str(),
        )
        .await
        .expect("prepare the adapter's statement");
        let plan: Value = sqlx::query_scalar(&format!(
            "EXPLAIN (FORMAT JSON) EXECUTE oml_widen_probe ('{a}', '{x}', '{y}')"
        ))
        .fetch_one(&mut *tx)
        .await
        .expect("explain");
        let analyzed: Value = sqlx::query_scalar(&format!(
            "EXPLAIN (ANALYZE, FORMAT JSON) EXECUTE oml_widen_probe ('{a}', '{x}', '{y}')"
        ))
        .fetch_one(&mut *tx)
        .await
        .expect("explain analyze");
        let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
            .fetch_one(&mut *tx)
            .await
            .expect("server version");
        tx.execute("DEALLOCATE oml_widen_probe")
            .await
            .expect("deallocate");
        tx.rollback().await.expect("rollback");
        // Every scan of `memories`, planned and executed, is tenant A's
        // partition: under a generic plan by executor-start pruning, each
        // Append's other subplans removed.
        assert_eq!(
            plan_memories_scanned(&plan),
            vec![partition_of(a)],
            "{mode}: {plan}"
        );
        assert_eq!(
            plan_memories_scanned(&analyzed),
            vec![partition_of(a)],
            "{mode}, executed: {analyzed}"
        );
        // The UPDATE's target list: the custom plan, which the adapter runs
        // the statement under, names tenant A's partition alone on every
        // version. A generic plan names it alone from PostgreSQL 18, which
        // prunes result relations at executor start; PostgreSQL 17 lists
        // every partition as a target there, opened and never scanned, which
        // the executed plan above shows.
        if mode == "force_custom_plan" || version >= 180_000 {
            assert_eq!(
                plan_memories_partitions(&plan),
                vec![partition_of(a)],
                "{mode} on {version}: {plan}"
            );
        }
        // Row security alone would prune the same way, so the custom plan's
        // scans must each carry the bound tenant itself: the statement's own
        // `m.tenant_id = $1`, not the policy's `current_setting` (ADR-0034
        // d13).
        if mode == "force_custom_plan" {
            let mut scans = Vec::new();
            plan_memories_scans(&plan, &mut scans);
            assert!(!scans.is_empty(), "the widening scans memories: {plan}");
            let named = format!("tenant_id = '{a}'::uuid");
            for (alias, conditions) in scans {
                assert!(
                    conditions.contains(&named),
                    "scan {alias} names its tenant: {conditions}"
                );
            }
        }
    }
    drop(conn);

    let (rig, proposer) = &mut rigs[0];
    let event = rig.next_event(
        event_types::CLAIM_EDGE_PROPOSED,
        json!({ "edge_id": Uuid::from_u128(0xE230).to_string() }),
    );
    let edge = ClaimEdgeRecord {
        edge_id: Uuid::from_u128(0xE230),
        tenant_id: a,
        src_artifact_id: x,
        dst_artifact_id: y,
        label: "acme:supports".to_owned(),
        state: EdgeState::Pending,
        proposer_key_id: proposer.key_id,
        proposer_principal: proposer.principal.clone(),
        ratifier_key_id: None,
        created_at: event.envelope.at.clone(),
    };
    store
        .commit(CommitRequest {
            outbox: OutboxRow {
                event_id: event.envelope.event_id,
                topic: event.envelope.event_type.clone(),
                payload: json!({ "seq": event.envelope.seq }),
            },
            event,
            fold: MetadataFold::ClaimEdgeProposed(edge),
            idempotency: None,
        })
        .await
        .expect("the proposal commits");

    let closure = |m: oml_app::types::MemoryRecord| (m.lineage_principals, m.lineage_key_ids);
    for id in [x, y, z] {
        let widened = store.memory(a, id).await.expect("read").expect("held");
        assert_eq!(
            closure(widened),
            (
                if id == z {
                    vec!["ann".to_owned(), "bob".to_owned(), "carol".to_owned()]
                } else {
                    vec!["ann".to_owned(), "bob".to_owned()]
                },
                vec![ann, bob]
            ),
            "tenant A's {id} gains the authors the edge joins"
        );
    }
    for memory in held.into_iter().filter(|m| m.tenant_id == b) {
        let stored = store
            .memory(b, memory.memory_id)
            .await
            .expect("read")
            .expect("held");
        assert_eq!(
            closure(stored),
            closure(memory.clone()),
            "tenant B's {} is as it was held",
            memory.memory_id
        );
    }
    drop_dbs(&created).await;
}

/// No partition's statistics reach the serving role: it holds no privilege
/// on a partition, and the parent's row security hides the parent's, while
/// the owner's view, the control, shows each partition's FTS index's
/// (ADR-0034 d1, d11).
#[tokio::test]
async fn the_serving_role_sees_no_partitions_pg_stats() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut tenants = Vec::new();
    for seed in [220_u64, 221] {
        let mut rig = ChainRig::new(seed);
        rig.seed_chain(&store, 0).await;
        let wa = Uuid::from_u128(0xB000 + u128::from(seed));
        for n in 0..4_u128 {
            hold(
                &store,
                &mut rig,
                wa,
                Uuid::from_u128((u128::from(seed) << 16) | n),
                Uuid::from_u128(0xD000 + u128::from(seed)),
                false,
            )
            .await;
        }
        tenants.push(rig.tenant_id);
    }
    let mut owner = owner_conn(&store).await;
    owner.execute("ANALYZE").await.expect("the owner analyses");
    let stats = "SELECT count(*) FROM pg_stats \
                  WHERE schemaname = 'public' AND tablename::text LIKE 'memories%'";
    let serving: i64 = sqlx::query_scalar(stats)
        .fetch_one(store.pool())
        .await
        .expect("the serving role's pg_stats");
    assert_eq!(serving, 0, "the serving role sees no memories statistics");
    // The partition is FORCE ROW LEVEL SECURITY and the owner bypasses
    // nothing, so `pg_stats` hides the partition's own columns from the owner
    // too; the control is the statistics of the partition's index attached to
    // `memories_claim_fts`, which stay in the owner's view (spike 3 S3.1).
    for tenant in tenants {
        let shown: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stats s \
               JOIN pg_class i ON i.relname = s.tablename \
                              AND i.relnamespace = 'public'::regnamespace \
               JOIN pg_index x ON x.indexrelid = i.oid \
               JOIN pg_inherits h ON h.inhrelid = i.oid \
              WHERE s.schemaname = 'public' \
                AND x.indrelid = $1::regclass \
                AND h.inhparent = 'memories_claim_fts'::regclass",
        )
        .bind(partition_of(tenant))
        .fetch_one(&mut owner)
        .await
        .expect("the owner's pg_stats");
        assert!(
            shown > 0,
            "control: the owner sees the statistics of {}'s FTS index",
            partition_of(tenant)
        );
    }
    drop(owner);
    drop_dbs(&created).await;
}

/// The checks after migrating read partitions as tables: a tenant's
/// partition passes them, and with `FORCE` lifted on it they refuse it
/// with `table_unfenced` (ADR-0034 d5, d11).
#[tokio::test]
async fn d5_holds_over_partitions() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (_name, store, owner) = migrated_pair(&created).await;
    let mut rig = ChainRig::new(230);
    rig.seed_chain(&store, 0).await;
    role_split_postflight(&owner, store.pool())
        .await
        .expect("control: a fenced partition passes");
    let partition = partition_of(rig.tenant_id);
    sqlx::query(&format!(
        "ALTER TABLE {partition} NO FORCE ROW LEVEL SECURITY"
    ))
    .execute(&owner)
    .await
    .expect("the owner lifts FORCE on the partition");
    assert_eq!(
        refused(
            role_split_postflight(&owner, store.pool()).await,
            "a partition without FORCE"
        ),
        RoleSplitReason::TableUnfenced
    );
    sqlx::query(&format!("ALTER TABLE {partition} FORCE ROW LEVEL SECURITY"))
        .execute(&owner)
        .await
        .expect("the owner restores FORCE");
    role_split_postflight(&owner, store.pool())
        .await
        .expect("control: FORCE restored passes");
    owner.close().await;
    store.pool().close().await;
    drop_dbs(&created).await;
}

/// An attach that waits on a lock past the five-second timeout fails with
/// `55P03`, answered `PartitionLockTimeout`, and commits nothing: no
/// partition remains, and once the lock is gone the same call succeeds
/// (ADR-0034 d11).
#[tokio::test]
async fn an_attach_that_waits_past_the_lock_timeout_answers_partition_lock_timeout() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let owner = owner_role(&store).await;
    let tenant = Uuid::from_u128(0xA000_0000 + 240);
    let mut holder = owner_conn(&store).await;
    // SHARE UPDATE EXCLUSIVE conflicts with the attach's own lock on the
    // parent and not with the AccessShareLock LIKE takes, so the wait is the
    // attach's.
    let mut held = holder.begin().await.expect("begin");
    held.execute("LOCK TABLE memories IN SHARE UPDATE EXCLUSIVE MODE")
        .await
        .expect("a second owner connection holds a conflicting lock");
    let started = std::time::Instant::now();
    let err = store
        .create_tenant_partitions(tenant)
        .await
        .expect_err("the attach waits past its lock timeout");
    assert_eq!(err, StoreError::PartitionLockTimeout);
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(5),
        "it waited the timeout: {:?}",
        started.elapsed()
    );
    held.rollback().await.expect("release the lock");
    assert_eq!(
        partition_fence(&mut holder, &partition_of(tenant), &owner).await,
        None,
        "the refused DDL committed nothing"
    );
    store
        .create_tenant_partitions(tenant)
        .await
        .expect("with the lock gone the same call succeeds");
    drop(holder);
    drop_dbs(&created).await;
}

/// The attach clones `memories`' foreign keys, so it takes
/// `ShareRowExclusiveLock` on `journal` and then `tenants`; a serving
/// transaction shaped as a genesis commit, `tenants` then `journal`, meets
/// it in a deadlock, which the detector breaks by aborting the attach, the
/// side that waited first. The port answers that `40P01` as it answers a
/// lock timeout, retryable, and commits nothing; the serving transaction
/// commits (ADR-0034 d11).
#[tokio::test]
async fn an_attach_the_deadlock_detector_aborts_answers_partition_lock_timeout() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let owner = owner_role(&store).await;
    let tenant = Uuid::from_u128(0xA000_0000 + 245);
    let mut serving = store.pool().acquire().await.expect("serving connection");
    let mut genesis = serving.begin().await.expect("begin");
    genesis
        .execute("LOCK TABLE tenants IN ROW EXCLUSIVE MODE")
        .await
        .expect("the serving transaction takes the lock its tenants insert takes");
    let ddl = tokio::spawn({
        let store = store.clone();
        async move { store.create_tenant_partitions(tenant).await }
    });
    // The attach holds `journal` and waits on `tenants`.
    let mut waiting = false;
    for _ in 0..100 {
        waiting = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_locks \
                             WHERE relation = 'tenants'::regclass AND NOT granted)",
        )
        .fetch_one(store.pool())
        .await
        .expect("pg_locks");
        if waiting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(waiting, "control: the attach waits on tenants");
    genesis
        .execute("LOCK TABLE journal IN ROW EXCLUSIVE MODE")
        .await
        .expect("the serving transaction takes journal once the detector aborts the attach");
    genesis
        .commit()
        .await
        .expect("the serving transaction commits");
    let err = ddl
        .await
        .expect("the DDL task")
        .expect_err("the detector aborts the attach");
    assert_eq!(err, StoreError::PartitionLockTimeout);
    drop(serving);
    let mut conn = owner_conn(&store).await;
    assert_eq!(
        partition_fence(&mut conn, &partition_of(tenant), &owner).await,
        None,
        "the aborted DDL committed nothing"
    );
    drop(conn);
    drop_dbs(&created).await;
}

/// The partition DDL's owner connection is opened for the call alone:
/// once `create_tenant_partitions` returns, `pg_stat_activity` shows no
/// connection of the owner role (ADR-0034 d4, d8).
#[tokio::test]
async fn pg_stat_activity_shows_no_owner_connection_after_the_partition_ddl() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let owner = owner_role(&store).await;
    store
        .create_tenant_partitions(Uuid::from_u128(0xA000_0000 + 250))
        .await
        .expect("the partition");
    // A closed connection's backend exits just after the client leaves.
    let mut open = -1_i64;
    for _ in 0..50 {
        open = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
              WHERE datname = current_database() AND usename = $1",
        )
        .bind(&owner)
        .fetch_one(store.pool())
        .await
        .expect("pg_stat_activity");
        if open == 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(open, 0, "no owner connection outlives the partition DDL");
    drop_dbs(&created).await;
}

/// The migrations mark no function `LEAKPROOF`: only a superuser can, and
/// the owner that runs them is not one, so an FTS read scans its tenant's
/// partition under row security (ADR-0034 d13).
#[tokio::test]
async fn the_migrations_create_no_function_marked_leakproof() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let marked: Vec<String> = sqlx::query_scalar(
        "SELECT p.oid::regprocedure::text FROM pg_proc p \
          WHERE p.pronamespace = 'public'::regnamespace AND p.proleakproof \
            AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_proc'::regclass \
                             AND d.objid = p.oid AND d.deptype = 'e')",
    )
    .fetch_all(store.pool())
    .await
    .expect("the leakproof functions");
    assert_eq!(marked, Vec::<String>::new());
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// Connections (ADR-0031 d7, d8, d10): migration 0012's connector-key
// credential, connect codes, connections and their tokens
// ---------------------------------------------------------------------------

/// Runs `statement` over the serving login with `oml.tenant_id` set to
/// `tenant`, in a transaction that commits; panics naming `what` on a
/// refusal.
async fn run_under(store: &PgStore, tenant: Uuid, statement: &str, what: &str) {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(statement)
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|e| panic!("{what}: {e}"));
    tx.commit().await.expect("commit");
}

/// Runs `statement` over the serving login under `tenant`, in a transaction
/// that rolls back; the refusal's SQLSTATE and constraint, or the rows it
/// changed.
async fn attempt_under(
    store: &PgStore,
    tenant: Uuid,
    statement: &str,
) -> Result<u64, (String, Option<String>)> {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let result = sqlx::query(statement).execute(&mut *tx).await;
    tx.rollback().await.expect("rollback");
    match result {
        Ok(done) => Ok(done.rows_affected()),
        Err(e) => {
            let db = e
                .as_database_error()
                .unwrap_or_else(|| panic!("not a database error: {e}"));
            Err((
                db.code().map(|c| c.into_owned()).unwrap_or_default(),
                db.constraint().map(str::to_owned),
            ))
        }
    }
}

/// The hex of a 32-byte fixture digest, every byte `byte`.
fn digest_hex(byte: u8) -> String {
    hex::encode([byte; 32])
}

/// A connector key row: `credential` `connection` and no hash (ADR-0031 d7).
fn connector_key_sql(tenant: Uuid, key: Uuid) -> String {
    format!(
        "INSERT INTO api_keys (id, tenant_id, hash, credential, role, principal, issued_seq) \
         VALUES ('{key}', '{tenant}', NULL, 'connection', 'writer', 'crew/claude-mobile', 1)"
    )
}

/// A live connect code of `key`, stored as the digest `byte` names (ADR-0031 d8).
fn connect_code_sql(tenant: Uuid, code: Uuid, key: Uuid, byte: u8) -> String {
    let digest = digest_hex(byte);
    format!(
        "INSERT INTO connect_codes (id, tenant_id, key_id, hash, issued_seq, expires_at) \
         VALUES ('{code}', '{tenant}', '{key}', decode('{digest}', 'hex'), 2, \
                 now() + interval '10 minutes')"
    )
}

/// A connection of `key` opened from `code` in `state`, its authorization
/// code stored as the digest `byte` names (ADR-0031 d10).
fn connection_sql(
    tenant: Uuid,
    connection: Uuid,
    key: Uuid,
    code: Uuid,
    state: &str,
    byte: u8,
) -> String {
    let digest = digest_hex(byte);
    let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    format!(
        "INSERT INTO oauth_connections (id, tenant_id, key_id, code_id, client_id, client_kind, \
         client_metadata_digest, redirect_uri, redirect_uri_sent, resource, code_challenge, \
         auth_code_hash, auth_code_expires_at, state, created_at) \
         VALUES ('{connection}', '{tenant}', '{key}', '{code}', \
                 'https://client.example/metadata.json', 'cimd', decode('{digest}', 'hex'), \
                 'https://client.example/callback', true, 'https://oml.example/mcp', \
                 '{challenge}', decode('{digest}', 'hex'), now() + interval '1 minute', \
                 '{state}', now())"
    )
}

/// A token of `connection` of `kind`, stored as the digest `byte` names.
fn token_sql(tenant: Uuid, connection: Uuid, kind: &str, byte: u8) -> String {
    let digest = digest_hex(byte);
    format!(
        "INSERT INTO oauth_tokens (hash, tenant_id, connection_id, kind, expires_at, created_at) \
         VALUES (decode('{digest}', 'hex'), '{tenant}', '{connection}', '{kind}', \
                 now() + interval '1 hour', now())"
    )
}

/// A migrated store with one tenant, seeded from `seed`, holding a connector
/// key; the store, the tenant and the key.
async fn connector_rig(created: &Mutex<Vec<String>>, seed: u64) -> (PgStore, Uuid, Uuid) {
    let store = fresh_store(created).await;
    let mut rig = ChainRig::new(seed);
    rig.seed_chain(&store, 0).await;
    let key = Uuid::from_u128(0xC0DE_0000 + u128::from(seed));
    run_under(
        &store,
        rig.tenant_id,
        &connector_key_sql(rig.tenant_id, key),
        "a connector key row",
    )
    .await;
    (store, rig.tenant_id, key)
}

/// A connector key is stored with `credential` `connection` and no hash; a
/// connector key row holding a hash is refused, and a bearer key, which the
/// adapter's fold writes with its hash, cannot turn connector while it holds
/// one (ADR-0031 d7; postgres-schema.md api_keys).
#[tokio::test]
async fn a_connector_key_row_has_no_hash_and_a_bearer_key_row_holding_its_hash_cannot_turn_connector()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, tenant, connector) = connector_rig(&created, 120).await;
    let mut rig = ChainRig::new(121);
    rig.seed_chain(&store, 0).await;
    let bearer = key_record(rig.tenant_id, Uuid::from_u128(0xB0B1), Role::Writer, None);
    rig.issue_key(&store, bearer.clone()).await;

    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let (credential, hashed): (String, bool) =
        sqlx::query_as("SELECT credential, hash IS NOT NULL FROM api_keys WHERE id = $1")
            .bind(connector)
            .fetch_one(&mut *tx)
            .await
            .expect("the connector key row");
    tx.rollback().await.expect("rollback");
    assert_eq!(
        (credential.as_str(), hashed),
        ("connection", false),
        "a connector key is stored with no hash"
    );

    let hashed_connector = format!(
        "INSERT INTO api_keys (id, tenant_id, hash, credential, role, principal, issued_seq) \
         VALUES ('{}', '{tenant}', decode('{}', 'hex'), 'connection', 'writer', 'crew/x', 1)",
        Uuid::from_u128(0xC0DE_0F00),
        digest_hex(0x77)
    );
    assert_eq!(
        attempt_under(&store, tenant, &hashed_connector).await,
        Err((
            "23514".to_owned(),
            Some("api_keys_connector_has_no_hash".to_owned())
        )),
        "a connector key row holding a hash is refused"
    );

    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let (credential, hashed): (String, bool) =
        sqlx::query_as("SELECT credential, hash IS NOT NULL FROM api_keys WHERE id = $1")
            .bind(bearer.key_id)
            .fetch_one(&mut *tx)
            .await
            .expect("the bearer key row");
    tx.rollback().await.expect("rollback");
    assert_eq!(
        (credential.as_str(), hashed),
        ("bearer", true),
        "the fold writes an issued key as a bearer key with its hash"
    );
    let turn = format!(
        "UPDATE api_keys SET credential = 'connection' WHERE id = '{}'",
        bearer.key_id
    );
    assert_eq!(
        attempt_under(&store, rig.tenant_id, &turn).await,
        Err((
            "23514".to_owned(),
            Some("api_keys_connector_has_no_hash".to_owned())
        )),
        "a bearer key holding its hash cannot turn connector"
    );
    drop_dbs(&created).await;
}

/// `connect_codes_live` holds one unredeemed, unvoided code per key: a
/// second live code is refused, and a voided or a redeemed code no longer
/// counts (ADR-0031 d8).
#[tokio::test]
async fn a_key_holds_at_most_one_live_connect_code() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, tenant, key) = connector_rig(&created, 122).await;
    let [first, second, third] = [0xC1, 0xC2, 0xC3].map(Uuid::from_u128);
    run_under(
        &store,
        tenant,
        &connect_code_sql(tenant, first, key, 0x01),
        "the first code",
    )
    .await;
    assert_eq!(
        attempt_under(&store, tenant, &connect_code_sql(tenant, second, key, 0x02)).await,
        Err(("23505".to_owned(), Some("connect_codes_live".to_owned()))),
        "a second live code of one key is refused"
    );
    run_under(
        &store,
        tenant,
        &format!("UPDATE connect_codes SET voided_seq = 3 WHERE id = '{first}'"),
        "void the first code",
    )
    .await;
    run_under(
        &store,
        tenant,
        &connect_code_sql(tenant, second, key, 0x02),
        "a code beside a voided one",
    )
    .await;
    run_under(
        &store,
        tenant,
        &format!("UPDATE connect_codes SET redeemed_at = now() WHERE id = '{second}'"),
        "redeem the second code",
    )
    .await;
    run_under(
        &store,
        tenant,
        &connect_code_sql(tenant, third, key, 0x03),
        "a code beside a redeemed one",
    )
    .await;
    drop_dbs(&created).await;
}

/// `oauth_connections_open` holds one open connection per key: a second
/// open connection is refused, an authorized one beside it is not, and a
/// revoked connection no longer counts (ADR-0031 d7).
#[tokio::test]
async fn a_key_holds_at_most_one_open_connection() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, tenant, key) = connector_rig(&created, 123).await;
    let [code_a, code_b] = [0xCA, 0xCB].map(Uuid::from_u128);
    let [conn_a, conn_b] = [0xEA, 0xEB].map(Uuid::from_u128);
    for (code, byte) in [(code_a, 0x0A), (code_b, 0x0B)] {
        run_under(
            &store,
            tenant,
            &connect_code_sql(tenant, code, key, byte),
            "a code",
        )
        .await;
        run_under(
            &store,
            tenant,
            &format!("UPDATE connect_codes SET redeemed_at = now() WHERE id = '{code}'"),
            "redeem it",
        )
        .await;
    }
    run_under(
        &store,
        tenant,
        &connection_sql(tenant, conn_a, key, code_a, "open", 0x1A),
        "the first open connection",
    )
    .await;
    assert_eq!(
        attempt_under(
            &store,
            tenant,
            &connection_sql(tenant, conn_b, key, code_b, "open", 0x1B)
        )
        .await,
        Err((
            "23505".to_owned(),
            Some("oauth_connections_open".to_owned())
        )),
        "a second open connection of one key is refused"
    );
    run_under(
        &store,
        tenant,
        &connection_sql(tenant, conn_b, key, code_b, "authorized", 0x1B),
        "an authorized connection beside the open one",
    )
    .await;
    run_under(
        &store,
        tenant,
        &format!(
            "UPDATE oauth_connections SET state = 'revoked', revoked_seq = 4, \
             revoked_cause = 'replaced' WHERE id = '{conn_a}'"
        ),
        "revoke the first",
    )
    .await;
    run_under(
        &store,
        tenant,
        &format!("UPDATE oauth_connections SET state = 'open' WHERE id = '{conn_b}'"),
        "open the second once the first is revoked",
    )
    .await;
    drop_dbs(&created).await;
}

/// Rows of `connect_codes`, `oauth_connections` and `oauth_tokens` the
/// serving login sees with `oml.tenant_id` set to `tenant` and
/// `oml.api_key_hash` to `digest`, each when given.
async fn visible_connection_rows(
    store: &PgStore,
    tenant: Option<Uuid>,
    digest: Option<u8>,
) -> (i64, i64, i64) {
    let mut tx = store.pool().begin().await.expect("begin");
    if let Some(tenant) = tenant {
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
    }
    if let Some(byte) = digest {
        sqlx::query("SELECT set_config('oml.api_key_hash', $1, true)")
            .bind(digest_hex(byte))
            .execute(&mut *tx)
            .await
            .expect("set digest");
    }
    let counts = sqlx::query_as(
        "SELECT (SELECT count(*) FROM connect_codes), (SELECT count(*) FROM oauth_connections), \
                (SELECT count(*) FROM oauth_tokens)",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("counts");
    tx.rollback().await.expect("rollback");
    counts
}

/// A tenant's connect code, connection and token are invisible to another
/// tenant's session and to a session with no tenant, and another tenant's
/// row cannot reference them: the reference is refused as one to no row is
/// (postgres-schema.md connections, "Keys into tenant tables").
#[tokio::test]
async fn a_connection_row_of_one_tenant_is_invisible_to_another_tenants_session() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, a, key_a) = connector_rig(&created, 124).await;
    let (code_a, conn_a) = (Uuid::from_u128(0xCA4), Uuid::from_u128(0xEA4));
    run_under(
        &store,
        a,
        &connect_code_sql(a, code_a, key_a, 0x24),
        "A's code",
    )
    .await;
    run_under(
        &store,
        a,
        &connection_sql(a, conn_a, key_a, code_a, "open", 0x25),
        "A's connection",
    )
    .await;
    run_under(
        &store,
        a,
        &token_sql(a, conn_a, "access", 0x26),
        "A's token",
    )
    .await;
    let mut rig_b = ChainRig::new(125);
    rig_b.seed_chain(&store, 0).await;
    let b = rig_b.tenant_id;
    let key_b = Uuid::from_u128(0xC0DE_0B25);
    run_under(&store, b, &connector_key_sql(b, key_b), "B's connector key").await;

    let flags: Vec<(String, bool, bool)> = sqlx::query(
        "SELECT relname::text, relrowsecurity, relforcerowsecurity FROM pg_class \
         WHERE relname IN ('connect_codes', 'oauth_connections', 'oauth_tokens') ORDER BY relname",
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
            ("connect_codes".to_owned(), true, true),
            ("oauth_connections".to_owned(), true, true),
            ("oauth_tokens".to_owned(), true, true),
        ],
        "row-level security is enabled and forced on the three tables"
    );
    assert_eq!(
        visible_connection_rows(&store, Some(a), None).await,
        (1, 1, 1),
        "the owning tenant sees its rows"
    );
    assert_eq!(
        visible_connection_rows(&store, Some(b), None).await,
        (0, 0, 0),
        "another tenant sees none"
    );
    assert_eq!(
        visible_connection_rows(&store, None, None).await,
        (0, 0, 0),
        "no tenant context sees none"
    );

    let to_a = attempt_under(
        &store,
        b,
        &connection_sql(b, Uuid::from_u128(0xEB4), key_b, code_a, "authorized", 0x27),
    )
    .await;
    let to_none = attempt_under(
        &store,
        b,
        &connection_sql(
            b,
            Uuid::from_u128(0xEB4),
            key_b,
            Uuid::from_u128(0x0BAD_C0DE),
            "authorized",
            0x27,
        ),
    )
    .await;
    assert_eq!(
        to_a,
        Err((
            "23503".to_owned(),
            Some("oauth_connections_code_id_fkey".to_owned())
        )),
        "B's connection naming A's code is refused by the tenant-scoped key"
    );
    assert_eq!(to_a, to_none, "as one naming no code is");
    let token_to_a = attempt_under(&store, b, &token_sql(b, conn_a, "access", 0x28)).await;
    let token_to_none = attempt_under(
        &store,
        b,
        &token_sql(b, Uuid::from_u128(0x0BAD_E000), "access", 0x28),
    )
    .await;
    assert_eq!(
        token_to_a,
        Err((
            "23503".to_owned(),
            Some("oauth_tokens_connection_id_fkey".to_owned())
        )),
        "B's token naming A's connection is refused by the tenant-scoped key"
    );
    assert_eq!(token_to_a, token_to_none, "as one naming no connection is");
    drop_dbs(&created).await;
}

/// Another tenant's code naming a key whose own tenant holds a live code on
/// it is refused exactly as one naming a key no row holds: `connect_codes_live`
/// leads with `tenant_id`, so its unique check never sees the other tenant's
/// code (postgres-schema.md connections, "Keys into tenant tables").
#[tokio::test]
async fn a_code_naming_another_tenants_key_with_a_live_code_is_refused_as_one_naming_no_key_is() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, a, key_a) = connector_rig(&created, 127).await;
    run_under(
        &store,
        a,
        &connect_code_sql(a, Uuid::from_u128(0xCA7), key_a, 0x30),
        "A's live code",
    )
    .await;
    let mut rig_b = ChainRig::new(128);
    rig_b.seed_chain(&store, 0).await;
    let b = rig_b.tenant_id;

    let to_a = attempt_under(
        &store,
        b,
        &connect_code_sql(b, Uuid::from_u128(0xCB7), key_a, 0x31),
    )
    .await;
    let to_none = attempt_under(
        &store,
        b,
        &connect_code_sql(
            b,
            Uuid::from_u128(0xCB7),
            Uuid::from_u128(0x0BAD_0127),
            0x31,
        ),
    )
    .await;
    assert_eq!(
        to_a,
        Err((
            "23503".to_owned(),
            Some("connect_codes_tenant_id_key_id_fkey".to_owned())
        )),
        "B's code naming A's key is refused by the tenant-scoped key, not by A's live code"
    );
    assert_eq!(to_a, to_none, "as one naming no key is");
    drop_dbs(&created).await;
}

/// Another tenant's open connection naming a key whose own tenant holds an
/// open connection on it is refused exactly as one naming a key no row holds:
/// `oauth_connections_open` leads with `tenant_id`, so its unique check never
/// sees the other tenant's connection (postgres-schema.md connections, "Keys
/// into tenant tables").
#[tokio::test]
async fn an_open_connection_naming_another_tenants_key_with_an_open_connection_is_refused_as_one_naming_no_key_is()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, a, key_a) = connector_rig(&created, 129).await;
    let code_a = Uuid::from_u128(0xCA9);
    run_under(
        &store,
        a,
        &connect_code_sql(a, code_a, key_a, 0x32),
        "A's code",
    )
    .await;
    run_under(
        &store,
        a,
        &connection_sql(a, Uuid::from_u128(0xEA9), key_a, code_a, "open", 0x33),
        "A's open connection",
    )
    .await;
    let mut rig_b = ChainRig::new(130);
    rig_b.seed_chain(&store, 0).await;
    let b = rig_b.tenant_id;
    let key_b = Uuid::from_u128(0xC0DE_0B30);
    run_under(&store, b, &connector_key_sql(b, key_b), "B's connector key").await;
    let code_b = Uuid::from_u128(0xCB9);
    run_under(
        &store,
        b,
        &connect_code_sql(b, code_b, key_b, 0x34),
        "B's code",
    )
    .await;

    let to_a = attempt_under(
        &store,
        b,
        &connection_sql(b, Uuid::from_u128(0xEB9), key_a, code_b, "open", 0x35),
    )
    .await;
    let to_none = attempt_under(
        &store,
        b,
        &connection_sql(
            b,
            Uuid::from_u128(0xEB9),
            Uuid::from_u128(0x0BAD_0129),
            code_b,
            "open",
            0x35,
        ),
    )
    .await;
    assert_eq!(
        to_a,
        Err((
            "23503".to_owned(),
            Some("oauth_connections_tenant_id_key_id_fkey".to_owned())
        )),
        "B's open connection naming A's key is refused by the tenant-scoped key, not by A's open connection"
    );
    assert_eq!(to_a, to_none, "as one naming no key is");
    drop_dbs(&created).await;
}

/// Before the tenant is known, a session that sets `oml.api_key_hash` to a
/// code's, an authorization code's or a token's digest sees exactly the row
/// that digest names, through the key_resolution form 0001 gave
/// `api_keys.hash`, and a digest no row holds shows nothing
/// (postgres-schema.md connections).
#[tokio::test]
async fn a_digest_lookup_with_no_tenant_sees_exactly_the_row_its_digest_names() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, t, key) = connector_rig(&created, 126).await;
    let (code, conn) = (Uuid::from_u128(0xCA6), Uuid::from_u128(0xEA6));
    run_under(&store, t, &connect_code_sql(t, code, key, 0x61), "a code").await;
    run_under(
        &store,
        t,
        &connection_sql(t, conn, key, code, "authorized", 0x62),
        "a connection",
    )
    .await;
    run_under(&store, t, &token_sql(t, conn, "refresh", 0x63), "a token").await;
    assert_eq!(
        visible_connection_rows(&store, None, Some(0x61)).await,
        (1, 0, 0),
        "the code's digest finds the code alone"
    );
    assert_eq!(
        visible_connection_rows(&store, None, Some(0x62)).await,
        (0, 1, 0),
        "the authorization code's digest finds the connection alone"
    );
    assert_eq!(
        visible_connection_rows(&store, None, Some(0x63)).await,
        (0, 0, 1),
        "the token's digest finds the token alone"
    );
    assert_eq!(
        visible_connection_rows(&store, None, Some(0x64)).await,
        (0, 0, 0),
        "a digest no row holds finds nothing"
    );
    drop_dbs(&created).await;
}

/// Every `api_keys` row of a database at the v0.3 schema, 0001–0009
/// applied, tenant keys, a revoked key and a provider-level operator key
/// among them, reads as a bearer key with its hash after 0012, and a row
/// written since names its credential: the column keeps no default
/// (ADR-0031 d7; postgres-schema.md api_keys).
#[tokio::test]
async fn a_migrated_v0_3_database_keeps_every_key_as_a_bearer_key() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 9).await;
    let tenant = Uuid::from_u128(0xA03);
    let keys = [
        (
            Uuid::from_u128(0xD031),
            Some(tenant),
            "admin",
            false,
            0x31_u8,
        ),
        (Uuid::from_u128(0xD032), Some(tenant), "writer", false, 0x32),
        (Uuid::from_u128(0xD033), Some(tenant), "reader", true, 0x33),
        (Uuid::from_u128(0xD034), None, "operator", false, 0x34),
    ];
    let mut tx = conn.begin().await.expect("begin");
    sqlx::query("ALTER TABLE api_keys NO FORCE ROW LEVEL SECURITY")
        .execute(&mut *tx)
        .await
        .expect("the owner lifts FORCE to write the operator key");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'v03')")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .expect("tenant");
    for (id, key_tenant, role, revoked, byte) in keys {
        sqlx::query(
            "INSERT INTO api_keys (id, tenant_id, hash, role, principal, issued_seq, revoked_at) \
             VALUES ($1, $2, $3, $4, 'v03/key', 1, CASE WHEN $5 THEN now() END)",
        )
        .bind(id)
        .bind(key_tenant)
        .bind(vec![byte; 32])
        .bind(role)
        .bind(revoked)
        .execute(&mut *tx)
        .await
        .expect("a v0.3 key row");
    }
    sqlx::query("ALTER TABLE api_keys FORCE ROW LEVEL SECURITY")
        .execute(&mut *tx)
        .await
        .expect("restore FORCE");
    tx.commit().await.expect("commit the v0.3 rows");

    for migration in MIGRATOR.iter().filter(|m| m.version > 9) {
        conn.apply(migration)
            .await
            .expect("the later migrations apply");
    }

    let mut tx = conn.begin().await.expect("begin");
    sqlx::query("ALTER TABLE api_keys NO FORCE ROW LEVEL SECURITY")
        .execute(&mut *tx)
        .await
        .expect("the owner lifts FORCE to read every row");
    let rows: Vec<(Uuid, String, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT id, credential, hash FROM api_keys ORDER BY id")
            .fetch_all(&mut *tx)
            .await
            .expect("the migrated keys");
    tx.rollback().await.expect("rollback");
    let expected: Vec<(Uuid, String, Option<Vec<u8>>)> = keys
        .iter()
        .map(|&(id, _, _, _, byte)| (id, "bearer".to_owned(), Some(vec![byte; 32])))
        .collect();
    assert_eq!(rows, expected, "every key is a bearer key with its hash");

    let mut tx = conn.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let unnamed = sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, hash, role, principal, issued_seq) \
         VALUES ($1, $2, $3, 'writer', 'v03/new', 2)",
    )
    .bind(Uuid::from_u128(0xD035))
    .bind(tenant)
    .bind(vec![0x35_u8; 32])
    .execute(&mut *tx)
    .await;
    tx.rollback().await.expect("rollback");
    assert_eq!(
        refused_code(unnamed, "a key row that names no credential"),
        "23502",
        "the credential column keeps no default"
    );
    drop(conn);
    let _ = db;
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// The provider tenant's OAuth state (ADR-0031 d5, d9): migration 0013's
// client cache and authorization-request rows
// ---------------------------------------------------------------------------

/// An `oauth_clients` row of `kind` for `client_id`, written with `tenant_id`
/// `tenant` (ADR-0031 d5).
fn oauth_client_sql(tenant: Uuid, client_id: &str, kind: &str) -> String {
    let digest = digest_hex(0x51);
    format!(
        "INSERT INTO oauth_clients (client_id, tenant_id, kind, client_name, redirect_uris, \
         metadata_digest, fetched_at, created_at) \
         VALUES ('{client_id}', '{tenant}', '{kind}', 'Claude', \
                 ARRAY['https://claude.ai/api/mcp/auth_callback'], decode('{digest}', 'hex'), \
                 now(), now())"
    )
}

/// An `oauth_authorizations` row written with `tenant_id` `tenant`, its
/// handle stored as the digest `byte` names, holding the verified code's id,
/// tenant and key (ADR-0031 d9).
fn authorization_sql(
    tenant: Uuid,
    id: Uuid,
    byte: u8,
    code: Uuid,
    code_tenant: Uuid,
    key: Uuid,
) -> String {
    let handle = digest_hex(byte);
    let metadata = digest_hex(0x52);
    let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    format!(
        "INSERT INTO oauth_authorizations (id, tenant_id, handle_hash, client_id, client_kind, \
         metadata_digest, redirect_uri, redirect_uri_sent, code_challenge, state, expires_at, \
         code_id, code_tenant_id, key_id) \
         VALUES ('{id}', '{tenant}', decode('{handle}', 'hex'), \
                 'https://client.example/metadata.json', 'cimd', decode('{metadata}', 'hex'), \
                 'https://client.example/callback', true, '{challenge}', 'xyz', \
                 now() + interval '10 minutes', '{code}', '{code_tenant}', '{key}')"
    )
}

/// An `oauth_clients` row whose `tenant_id` is not the reserved provider
/// tenant is refused: under the provider tenant's session by its row
/// security, and under the named tenant's own session by the column's check,
/// so no tenant's session can place a client in its own tenant (ADR-0031 d5;
/// postgres-schema.md connections).
#[tokio::test]
async fn an_oauth_clients_row_outside_the_provider_tenant_is_refused() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut rig = ChainRig::new(130);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;

    run_under(
        &store,
        PROVIDER_TENANT,
        &oauth_client_sql(
            PROVIDER_TENANT,
            "https://client.example/metadata.json",
            "cimd",
        ),
        "a provider tenant's client",
    )
    .await;
    let outside = oauth_client_sql(tenant, "https://other.example/metadata.json", "cimd");
    assert_eq!(
        attempt_under(&store, PROVIDER_TENANT, &outside).await,
        Err(("42501".to_owned(), None)),
        "the provider tenant's session cannot write another tenant's client row"
    );
    assert_eq!(
        attempt_under(&store, tenant, &outside).await,
        Err((
            "23514".to_owned(),
            Some("oauth_clients_provider_tenant".to_owned())
        )),
        "a tenant's own session cannot write a client row into its tenant"
    );
    drop_dbs(&created).await;
}

/// An `oauth_authorizations` row whose `tenant_id` is not the reserved
/// provider tenant is refused by the column's check, even under the named
/// tenant's own session (ADR-0031 d9; postgres-schema.md connections).
#[tokio::test]
async fn an_oauth_authorizations_row_outside_the_provider_tenant_is_refused() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, tenant, key) = connector_rig(&created, 131).await;
    let code = Uuid::from_u128(0xCA31);
    let outside = authorization_sql(tenant, Uuid::from_u128(0xA031), 0x31, code, tenant, key);
    assert_eq!(
        attempt_under(&store, tenant, &outside).await,
        Err((
            "23514".to_owned(),
            Some("oauth_authorizations_provider_tenant".to_owned())
        )),
        "a tenant's own session cannot write an authorization row into its tenant"
    );
    drop_dbs(&created).await;
}

/// Rows of `oauth_authorizations` holding the handle digest `byte` names
/// that the serving login sees with `oml.tenant_id` set to `tenant` and
/// `oml.api_key_hash` to that digest, each when asked.
async fn authorizations_found(
    store: &PgStore,
    tenant: Option<Uuid>,
    byte: u8,
    digest_set: bool,
) -> i64 {
    let mut tx = store.pool().begin().await.expect("begin");
    if let Some(tenant) = tenant {
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
    }
    if digest_set {
        sqlx::query("SELECT set_config('oml.api_key_hash', $1, true)")
            .bind(digest_hex(byte))
            .execute(&mut *tx)
            .await
            .expect("set digest");
    }
    let (found,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM oauth_authorizations WHERE handle_hash = decode($1, 'hex')",
    )
    .bind(digest_hex(byte))
    .fetch_one(&mut *tx)
    .await
    .expect("count");
    tx.rollback().await.expect("rollback");
    found
}

/// An authorization row is found by its handle digest under the provider
/// tenant's session, and by no other: not the code's own tenant's, not a
/// session with no tenant, and not the pre-tenant digest lookup, which this
/// table does not answer (ADR-0031 d9; postgres-schema.md connections).
#[tokio::test]
async fn an_authorization_row_is_found_by_its_handle_digest_only_under_the_provider_tenant() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, tenant, key) = connector_rig(&created, 132).await;
    let code = Uuid::from_u128(0xCA32);
    run_under(
        &store,
        PROVIDER_TENANT,
        &authorization_sql(
            PROVIDER_TENANT,
            Uuid::from_u128(0xA032),
            0x32,
            code,
            tenant,
            key,
        ),
        "an authorization row",
    )
    .await;

    let flags: Vec<(String, bool, bool)> = sqlx::query(
        "SELECT relname::text, relrowsecurity, relforcerowsecurity FROM pg_class \
         WHERE relname IN ('oauth_authorizations', 'oauth_clients') ORDER BY relname",
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
            ("oauth_authorizations".to_owned(), true, true),
            ("oauth_clients".to_owned(), true, true),
        ],
        "row-level security is enabled and forced on the provider tenant's two tables"
    );
    assert_eq!(
        authorizations_found(&store, Some(PROVIDER_TENANT), 0x32, false).await,
        1,
        "the provider tenant's session finds the row by its handle digest"
    );
    assert_eq!(
        authorizations_found(&store, Some(PROVIDER_TENANT), 0x33, false).await,
        0,
        "another handle's digest finds nothing"
    );
    assert_eq!(
        authorizations_found(&store, Some(tenant), 0x32, false).await,
        0,
        "the code's own tenant's session does not find it"
    );
    assert_eq!(
        authorizations_found(&store, None, 0x32, false).await,
        0,
        "a session with no tenant does not find it"
    );
    assert_eq!(
        authorizations_found(&store, None, 0x32, true).await,
        0,
        "the pre-tenant digest lookup does not find it"
    );
    drop_dbs(&created).await;
}

/// Deletes the authorization row whose handle digest `byte` names under
/// `tenant`'s session, in a transaction that commits; the code id, code
/// tenant and key each deleted row held.
async fn take_authorization(store: &PgStore, tenant: Uuid, byte: u8) -> Vec<(Uuid, Uuid, Uuid)> {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let taken = sqlx::query_as(
        "DELETE FROM oauth_authorizations WHERE handle_hash = decode($1, 'hex') \
         RETURNING code_id, code_tenant_id, key_id",
    )
    .bind(digest_hex(byte))
    .fetch_all(&mut *tx)
    .await
    .expect("delete by handle");
    tx.commit().await.expect("commit");
    taken
}

/// A confirmation deletes its authorization row by handle under the provider
/// tenant and reads back the code's id, tenant and key; the first delete
/// returns them, a second finds no row, and a delete under the code's own
/// tenant consumes nothing (ADR-0031 d9; postgres-schema.md connections).
#[tokio::test]
async fn deleting_an_authorization_row_by_handle_returns_its_code_tenant_and_key_exactly_once() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (store, tenant, key) = connector_rig(&created, 133).await;
    let code = Uuid::from_u128(0xCA33);
    run_under(
        &store,
        PROVIDER_TENANT,
        &authorization_sql(
            PROVIDER_TENANT,
            Uuid::from_u128(0xA033),
            0x34,
            code,
            tenant,
            key,
        ),
        "an authorization row",
    )
    .await;

    assert_eq!(
        take_authorization(&store, tenant, 0x34).await,
        Vec::new(),
        "the code's own tenant's session deletes nothing"
    );
    assert_eq!(
        take_authorization(&store, PROVIDER_TENANT, 0x34).await,
        vec![(code, tenant, key)],
        "the first delete returns the code, its tenant and its key"
    );
    assert_eq!(
        take_authorization(&store, PROVIDER_TENANT, 0x34).await,
        Vec::new(),
        "a second delete with the same handle finds no row"
    );
    drop_dbs(&created).await;
}

/// ADR-0044 d12: migration 0014 rewrites nothing and journals nothing; a key
/// issued before users folds as a tenant key, a workarea as a shared one
/// that is no user root, and a grant as covering its node's subtree, as
/// their journal events, which carry none of the new fields, say.
#[tokio::test]
async fn a_key_issued_before_users_folds_as_a_tenant_key() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 13).await;
    let tenant = Uuid::from_u128(0xA440);
    let (universe, crew, key, grant) = (
        Uuid::from_u128(0xB440),
        Uuid::from_u128(0xB441),
        Uuid::from_u128(0xD440),
        Uuid::from_u128(0xF440),
    );
    let mut tx = conn.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'oml')")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .expect("tenant");
    // A deployment created before v0.5.2 may hold its root stored
    // `development`: the build derived the served status from parent_id and
    // never read the column, as the live deployment's root shows.
    for (w, parent, w_name) in [(universe, None, "universe"), (crew, Some(universe), "crew")] {
        sqlx::query(
            "INSERT INTO workareas (id, tenant_id, parent_id, name, path, status) \
             VALUES ($1, $2, $3, $4, text2ltree($5), 'development')",
        )
        .bind(w)
        .bind(tenant)
        .bind(parent)
        .bind(w_name)
        .bind(if parent.is_some() {
            "universe.crew"
        } else {
            "universe"
        })
        .execute(&mut *tx)
        .await
        .expect("a workarea before users");
    }
    sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, hash, role, principal, held_by, issued_seq, \
         name, default_workarea_id, credential) \
         VALUES ($1, $2, $3, 'writer', 'crew/claude-mobile', 'agent', 3, 'claude-mobile', $4, \
         'bearer')",
    )
    .bind(key)
    .bind(tenant)
    .bind(Sha256Hash::digest(b"a key before users").0.to_vec())
    .bind(crew)
    .execute(&mut *tx)
    .await
    .expect("a key before users");
    sqlx::query(
        "INSERT INTO grants (id, tenant_id, key_id, role, workarea_node_id, epoch) \
         VALUES ($1, $2, $3, 'writer', $4, 4)",
    )
    .bind(grant)
    .bind(tenant)
    .bind(key)
    .bind(crew)
    .execute(&mut *tx)
    .await
    .expect("a grant before users");
    tx.commit().await.expect("commit the rows before users");
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    store
        .migrate(&owner_url(&db))
        .await
        .expect("0014 applies over rows before users");

    let stored = store
        .key(tenant, key)
        .await
        .expect("read")
        .expect("the key");
    assert_eq!(stored.user, None, "a tenant key");
    let access = store
        .key_access(tenant, key)
        .await
        .expect("read")
        .expect("access");
    assert_eq!(access.grants.len(), 1);
    assert_eq!(access.grants[0].grant.covers, Covers::Subtree);
    for w in [universe, crew] {
        let row = store
            .workarea(tenant, w)
            .await
            .expect("read")
            .expect("the workarea");
        assert!(!row.user_root);
        assert_eq!(row.namespace, None, "shared");
    }
    assert_eq!(
        store
            .resolve_name(tenant, NamedKind::Workarea, &name("crew"))
            .await
            .expect("resolve"),
        Some(crew),
        "its name stays in the shared namespace"
    );
    let journal: i64 = sqlx::query_scalar("SELECT count(*) FROM journal")
        .fetch_one(&mut conn)
        .await
        .expect("count");
    assert_eq!(journal, 0, "the upgrade journals nothing");
    drop(conn);
    drop_dbs(&created).await;
}

/// The status v0.5.1 served for a node: derived from `parent_id`, the
/// universe root `committed` and every other node `development`, whatever
/// the column stored.
fn v0_5_1_served_status(parent: Option<Uuid>) -> &'static str {
    if parent.is_none() {
        "committed"
    } else {
        "development"
    }
}

/// A v0.5.1 database, migrated through 0016, whose roots and nodes hold the
/// stored status the column was given before 0017 read it — one root
/// `development`, as the live deployment's, one `committed` — upgrades
/// through 0017 to 0019: every migration applies, every root ends
/// `committed`, every other node keeps its status, the status each read
/// serves is the one v0.5.1 served, nothing is journalled, and a second
/// boot changes nothing (ADR-0009 d1; ADR-0046).
#[tokio::test]
async fn a_v0_5_1_database_whose_root_is_stored_development_upgrades_through_0017_serving_the_same_statuses()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 16).await;
    let mut nodes: Vec<(Uuid, Uuid, Option<Uuid>)> = Vec::new();
    for (n, root_status) in [(0_u128, "development"), (1, "committed")] {
        let tenant = Uuid::from_u128(0xA170 + n);
        let root = Uuid::from_u128(0xB170 + n * 16);
        let (crew, deeper) = (
            Uuid::from_u128(0xB171 + n * 16),
            Uuid::from_u128(0xB172 + n * 16),
        );
        let mut tx = conn.begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, $2)")
            .bind(tenant)
            .bind(format!("t{n}"))
            .execute(&mut *tx)
            .await
            .expect("tenant");
        for (w, parent, w_name, path, status) in [
            (root, None, "universe", "universe", root_status),
            (crew, Some(root), "crew", "universe.crew", "development"),
            (
                deeper,
                Some(crew),
                "deeper",
                "universe.crew.deeper",
                "development",
            ),
        ] {
            sqlx::query(
                "INSERT INTO workareas (id, tenant_id, parent_id, name, path, status) \
                 VALUES ($1, $2, $3, $4, text2ltree($5), $6)",
            )
            .bind(w)
            .bind(tenant)
            .bind(parent)
            .bind(w_name)
            .bind(path)
            .bind(status)
            .execute(&mut *tx)
            .await
            .expect("a workarea as v0.5.1 stored it");
            nodes.push((tenant, w, parent));
        }
        tx.commit().await.expect("commit");
    }
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    let aligned = store
        .migrate(&owner_url(&db))
        .await
        .expect("0017 to 0019 apply over a v0.5.1 database");
    assert_eq!(aligned, 1, "only the root stored `development` is aligned");
    for (tenant, w, parent) in &nodes {
        let row = store
            .workarea(*tenant, *w)
            .await
            .expect("read")
            .expect("the workarea");
        assert_eq!(
            row.status.as_str(),
            v0_5_1_served_status(*parent),
            "{w} serves what v0.5.1 served"
        );
    }
    let journal: i64 = sqlx::query_scalar("SELECT count(*) FROM journal")
        .fetch_one(&mut conn)
        .await
        .expect("count");
    assert_eq!(journal, 0, "the upgrade journals nothing");
    let again = store
        .migrate(&owner_url(&db))
        .await
        .expect("a second boot migrates nothing");
    assert_eq!(again, 0, "a second boot aligns nothing");
    let applied: i64 =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success")
            .fetch_one(&mut conn)
            .await
            .expect("versions");
    // The ledger's highest version is read (ADR-0030 d14).
    assert_eq!(applied, 37, "every migration through 0037_open_signup");
    drop(conn);
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// Status-ladder proposals (migrations/0017_workarea_transitions.sql;
// ADR-0009 d1, d3)
// ---------------------------------------------------------------------------

/// Runs `sql` as the serving role in `tenant`'s context, binding the
/// transition `id`, the `workarea`, the proposer and the ratifier keys in
/// that order, and commits when it succeeds.
async fn transition_statement(
    store: &PgStore,
    tenant: Uuid,
    sql: &str,
    ids: [Option<Uuid>; 4],
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let mut query = sqlx::query(sql).bind(tenant);
    for id in ids {
        query = query.bind(id);
    }
    let result = query.execute(&mut *tx).await;
    if result.is_ok() {
        tx.commit().await.expect("commit");
    }
    result
}

/// A proposal is one rung forward, a ratified row names its ratifier and
/// its event's seq, the ratifier is never the proposer, and the node and
/// keys it names are its own tenant's; the serving role inserts, ratifies
/// and reads a row under its tenant's row security, and another tenant
/// sees none (ADR-0009 d1, d3; ADR-0034 d3, d12).
#[tokio::test]
async fn the_schema_holds_a_transition_to_one_rung_forward_ratified_by_another_key_in_its_own_tenant()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut a = ChainRig::new(101);
    a.seed_chain(&store, 0).await;
    let t = a.tenant_id;
    let root = Uuid::from_u128(0xB1010);
    a.create_workarea(&store, workarea_record(t, root, None, "universe"))
        .await;
    let node = Uuid::from_u128(0xB1011);
    a.create_workarea(&store, workarea_record(t, node, Some(root), "procurement"))
        .await;
    let (proposer, ratifier) = (Uuid::from_u128(0xD1010), Uuid::from_u128(0xD1011));
    for key in [proposer, ratifier] {
        a.issue_key(&store, key_record(t, key, Role::Admin, None))
            .await;
    }
    let mut b = ChainRig::new(102);
    b.seed_chain(&store, 0).await;
    let theirs = Uuid::from_u128(0xB1020);
    b.create_workarea(
        &store,
        workarea_record(b.tenant_id, theirs, None, "universe"),
    )
    .await;

    // $1 tenant, $2 id, $3 workarea, $4 proposer, $5 ratifier.
    let row = |from: &str, to: &str, state: &str, ratified_seq: Option<i64>| {
        let ratified_seq = ratified_seq.map_or_else(|| "NULL".to_owned(), |s| s.to_string());
        format!(
            "INSERT INTO workarea_transitions (tenant_id, id, workarea_id, proposer_key_id, \
             ratifier_key_id, from_status, to_status, state, proposer_principal, proposed_seq, \
             ratified_seq) VALUES ($1, $2, $3, $4, $5, '{from}', '{to}', '{state}', 'p', 7, \
             {ratified_seq})"
        )
    };
    let pending = Uuid::from_u128(0xE1010);
    transition_statement(
        &store,
        t,
        &row("development", "committed", "pending", None),
        [Some(pending), Some(node), Some(proposer), None],
    )
    .await
    .expect("a pending proposal one rung forward");

    let refusals = [
        (
            "a skipped rung",
            row("development", "validated", "pending", None),
            [
                Some(Uuid::from_u128(0xE1011)),
                Some(node),
                Some(proposer),
                None,
            ],
            "23514",
        ),
        (
            "a backward step",
            row("committed", "development", "pending", None),
            [
                Some(Uuid::from_u128(0xE1012)),
                Some(node),
                Some(proposer),
                None,
            ],
            "23514",
        ),
        (
            "a ratification by its proposer",
            row("development", "committed", "ratified", Some(8)),
            [
                Some(Uuid::from_u128(0xE1013)),
                Some(node),
                Some(proposer),
                Some(proposer),
            ],
            "23514",
        ),
        (
            "a ratified row with no ratifier",
            row("development", "committed", "ratified", Some(8)),
            [
                Some(Uuid::from_u128(0xE1014)),
                Some(node),
                Some(proposer),
                None,
            ],
            "23514",
        ),
        (
            "a ratified row with no seq",
            row("development", "committed", "ratified", None),
            [
                Some(Uuid::from_u128(0xE1015)),
                Some(node),
                Some(proposer),
                Some(ratifier),
            ],
            "23514",
        ),
        (
            "another tenant's node",
            row("development", "committed", "pending", None),
            [
                Some(Uuid::from_u128(0xE1016)),
                Some(theirs),
                Some(proposer),
                None,
            ],
            "23503",
        ),
    ];
    for (what, sql, ids, code) in refusals {
        assert_eq!(
            refused_code(transition_statement(&store, t, &sql, ids).await, what),
            code,
            "{what}"
        );
    }

    transition_statement(
        &store,
        t,
        "UPDATE workarea_transitions SET state = 'ratified', ratifier_key_id = $5, \
         ratified_seq = 9 WHERE tenant_id = $1 AND id = $2 AND workarea_id = $3 \
         AND proposer_key_id = $4",
        [Some(pending), Some(node), Some(proposer), Some(ratifier)],
    )
    .await
    .expect("the serving role ratifies a pending row");
    for (tenant, seen) in [(t, 1_i64), (b.tenant_id, 0)] {
        let mut tx = store.pool().begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("set tenant");
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM workarea_transitions")
            .fetch_one(&mut *tx)
            .await
            .expect("count");
        assert_eq!(n, seen, "tenant {tenant} sees {seen}");
    }
    drop_dbs(&created).await;
}

/// The serving role's transition reads answer under the asking tenant's
/// row security: another tenant reads none of a tenant's proposals by id
/// or by node, and under its own context, or none, the table shows it no
/// row, while the owning tenant reads its own (ADR-0009 d3; ADR-0034 d3;
/// postgres-schema.md row-level security).
#[tokio::test]
async fn the_serving_role_reads_no_other_tenants_transitions() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let mut a = ChainRig::new(103);
    a.seed_chain(&store, 0).await;
    let t = a.tenant_id;
    let root = Uuid::from_u128(0xB1030);
    a.create_workarea(&store, workarea_record(t, root, None, "universe"))
        .await;
    let node = Uuid::from_u128(0xB1031);
    a.create_workarea(&store, workarea_record(t, node, Some(root), "procurement"))
        .await;
    let proposer = Uuid::from_u128(0xD1030);
    a.issue_key(&store, key_record(t, proposer, Role::Admin, None))
        .await;
    let proposal = pending_transition(
        t,
        Uuid::from_u128(0xE1030),
        node,
        WorkareaStatus::Development,
        proposer,
    );
    a.propose_transition(&store, proposal.clone())
        .await
        .expect("proposal");
    let mut b = ChainRig::new(104);
    b.seed_chain(&store, 0).await;

    assert_eq!(
        store.transition(t, proposal.transition_id).await,
        Ok(Some(proposal.clone())),
        "the owning tenant reads its proposal"
    );
    assert_eq!(
        store.pending_transitions(t, node).await,
        Ok(vec![proposal.clone()]),
        "the owning tenant lists its proposal"
    );
    assert_eq!(
        store.transition(b.tenant_id, proposal.transition_id).await,
        Ok(None),
        "another tenant reads none by id"
    );
    assert_eq!(
        store.pending_transitions(b.tenant_id, node).await,
        Ok(Vec::new()),
        "another tenant lists none by node"
    );
    for (tenant, seen) in [(Some(t), 1_i64), (Some(b.tenant_id), 0), (None, 0)] {
        let mut tx = store.pool().begin().await.expect("begin");
        if let Some(tenant) = tenant {
            sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
                .bind(tenant.to_string())
                .execute(&mut *tx)
                .await
                .expect("set tenant");
        }
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM workarea_transitions")
            .fetch_one(&mut *tx)
            .await
            .expect("count");
        tx.rollback().await.expect("rollback");
        assert_eq!(n, seen, "under {tenant:?} the table shows {seen}");
    }
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// The status ladder end to end over the serving role (ADR-0009 d3, d4)
// ---------------------------------------------------------------------------

type PgApp = App<PgStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

/// The use cases over `store`, signing with the committed test key.
fn app_over_pg(store: &PgStore) -> PgApp {
    App::new(
        store.clone(),
        MemSigner::new(oml_port_contracts::TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
}

/// A tenant bootstrapped through the use cases under `tenant_name`, its
/// first admin key named `root-admin` and held by `alice-admin`.
async fn bootstrap_pg(app: &PgApp, tenant_name: &str) -> oml_app::types::BootstrapResult {
    app.bootstrap_tenant(&BootstrapRequest {
        tenant_name: name(tenant_name),
        admin_key_name: name("root-admin"),
        admin_principal: "alice-admin".to_owned(),
    })
    .await
    .expect("bootstrap")
}

/// A workarea `child` under `parent`, created by `admin`; its id.
async fn workarea_pg(app: &PgApp, admin: &str, child: &str, parent: Uuid) -> Uuid {
    app.create_workarea(admin, &name(child), Some(parent.into()), None)
        .await
        .expect("create workarea")
        .workarea
        .workarea_id
}

/// A dreamer key held by `principal` with one dreamer grant on `node`.
async fn dreamer_pg(
    app: &PgApp,
    admin: &str,
    key_name: &str,
    principal: &str,
    node: Uuid,
) -> oml_app::types::IssuedKey {
    app.issue_key(
        admin,
        &oml_app::types::IssueKeyRequest {
            profile: None,
            role: Role::Dreamer,
            name: name(key_name),
            principal: principal.to_owned(),
            held_by: None,
            default_workarea: None,
            crew: None,
            connect_code: false,
            user: None,
            guardrail_scope: None,
            grants: vec![oml_app::types::GrantRequest {
                node: node.into(),
                role: Role::Dreamer,
                deny: Vec::new(),
                covers: oml_app::domain::access::Covers::Subtree,
            }],
        },
    )
    .await
    .expect("issue a dreamer key")
}

/// The stored state of transition `id` as the serving role reads it under
/// `tenant`'s row security: (state, ratifier, proposed seq, ratified seq).
async fn transition_row(
    store: &PgStore,
    tenant: Uuid,
    id: Uuid,
) -> Option<(String, Option<Uuid>, i64, Option<i64>)> {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row = sqlx::query_as(
        "SELECT state, ratifier_key_id, proposed_seq, ratified_seq \
           FROM workarea_transitions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .expect("the transition row");
    tx.rollback().await.expect("rollback");
    row
}

/// Propose then ratify through the use cases over the serving login: the
/// node's stored status moves to the proposal's `to`, the journal holds one
/// `workarea.status_proposed` signed by the proposer and one
/// `workarea.status_transition` signed by the ratifier, the row is ratified
/// at the second event's seq, the stored chain verifies, and another tenant
/// reads none of it under row security (ADR-0009 d3; ADR-0010 R2; ADR-0034
/// d3).
#[tokio::test]
async fn propose_then_ratify_through_the_serving_role_moves_status_and_journals_both_events_under_rls()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let app = app_over_pg(&store);
    let boot = bootstrap_pg(&app, "acme").await;
    let (t, admin) = (boot.tenant_id, boot.admin_key.value.clone());
    let crew = workarea_pg(&app, &admin, "crew", boot.root_workarea_id).await;
    let dora = dreamer_pg(&app, &admin, "dora-key", "dora", crew).await;
    let other = bootstrap_pg(&app, "globex").await;

    let proposal = app
        .propose_transition(&admin, crew.into(), WorkareaStatus::Committed)
        .await
        .expect("the admin proposes");
    let pending = app
        .workarea_status(&admin, crew.into())
        .await
        .expect("the status read");
    assert_eq!(pending.status, WorkareaStatus::Development);
    assert_eq!(
        pending
            .pending
            .iter()
            .map(|p| p.transition_id)
            .collect::<Vec<_>>(),
        vec![proposal.transition_id],
        "the proposal waits, the node unmoved"
    );
    let record = app
        .ratify_transition(&dora.value, crew.into(), proposal.transition_id)
        .await
        .expect("a second identity ratifies");
    assert_eq!(record.status, WorkareaStatus::Committed);
    assert_eq!(
        store
            .workarea(t, crew)
            .await
            .expect("read")
            .expect("the node")
            .status,
        WorkareaStatus::Committed,
        "the stored status moved"
    );
    assert!(
        store
            .pending_transitions(t, crew)
            .await
            .expect("read")
            .is_empty(),
        "nothing pending"
    );

    let events = store.events_range(t, 1, 1000).await.expect("the journal");
    let ladder: Vec<&SignedEnvelope> = events
        .iter()
        .filter(|e| e.envelope.event_type.starts_with("workarea.status_"))
        .collect();
    assert_eq!(ladder.len(), 2, "both events journalled");
    let (proposed, ratified) = (ladder[0], ladder[1]);
    assert_eq!(
        (
            proposed.envelope.event_type.as_str(),
            proposed.envelope.principal.as_str(),
            proposed.envelope.api_key_id,
        ),
        (
            event_types::WORKAREA_STATUS_PROPOSED,
            "alice-admin",
            Some(boot.admin_key.key_id),
        )
    );
    assert_eq!(
        (
            ratified.envelope.event_type.as_str(),
            ratified.envelope.principal.as_str(),
            ratified.envelope.api_key_id,
        ),
        (
            event_types::WORKAREA_STATUS_TRANSITION,
            "dora",
            Some(dora.key_id),
        )
    );
    assert_eq!(
        ratified.envelope.payload,
        json!({
            "workarea_id": crew.to_string(),
            "from": "development",
            "to": "committed",
            "transition_id": proposal.transition_id.to_string(),
            "proposed_by_key_id": boot.admin_key.key_id.to_string(),
            "ratified_by_key_id": dora.key_id.to_string(),
            "same_user": false,
            "policy_snapshot": { "k": 1, "required_roles": ["dreamer"] },
        })
    );
    assert_eq!(
        transition_row(&store, t, proposal.transition_id).await,
        Some((
            "ratified".to_owned(),
            Some(dora.key_id),
            i64::try_from(proposed.envelope.seq).expect("a seq"),
            Some(i64::try_from(ratified.envelope.seq).expect("a seq")),
        )),
        "the row is ratified at the transition's seq"
    );
    let keys = KeySet::new(vec![test_signing_key().verifying_key()]);
    let head = store
        .verify_stored_chain(t, &keys)
        .await
        .expect("the chain verifies");
    assert_eq!(head.seq, ratified.envelope.seq);

    // Another tenant reads none of it under row security.
    let theirs = other.tenant_id;
    assert_eq!(store.workarea(theirs, crew).await.expect("read"), None);
    assert_eq!(
        store.transition(theirs, proposal.transition_id).await,
        Ok(None)
    );
    assert_eq!(
        transition_row(&store, theirs, proposal.transition_id).await,
        None
    );
    assert!(
        store
            .events_range(theirs, 1, 1000)
            .await
            .expect("their journal")
            .iter()
            .all(|e| !e.envelope.event_type.starts_with("workarea.status_")),
        "their journal holds no ladder event"
    );
    drop_dbs(&created).await;
}

/// Every workarea of `tenant` stored `validated` with a child that is not,
/// as `parent/child` names; the tree invariant holds when it is empty
/// (ADR-0009 d4).
async fn validated_over_unvalidated(store: &PgStore, tenant: Uuid) -> Vec<String> {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let found = sqlx::query_scalar(
        "SELECT p.name::text || '/' || c.name::text FROM workareas p \
           JOIN workareas c ON c.tenant_id = p.tenant_id AND c.parent_id = p.id \
          WHERE p.tenant_id = $1 AND p.status = 'validated' AND c.status <> 'validated' \
          ORDER BY 1",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("the invariant query");
    tx.rollback().await.expect("rollback");
    found
}

/// Whether `result` lost a race on the chain head, by seq or by
/// `prev_hash`, and may be retried as the HTTP transport answers it.
fn lost_the_head<T>(result: &Result<T, AppError>) -> bool {
    matches!(
        result,
        Err(AppError::Store(
            StoreError::SeqConflict { .. } | StoreError::PrevHashConflict { .. }
        ))
    )
}

/// A child create under a committed node and the ratification of that
/// node's proposal to `validated` run as two tasks on the serving login,
/// each retrying a lost chain head: exactly one of them lands, the other is
/// refused by name (`workarea.frozen` for the create, `children_not_validated`
/// for the ratification), and after both no validated node has a child that
/// is not validated (ADR-0009 d4, d5; workareas.md, ratification
/// re-checks).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_child_create_and_a_validated_ratification_never_leave_a_validated_node_with_an_unvalidated_child()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let app = Arc::new(app_over_pg(&store));
    let boot = bootstrap_pg(&app, "acme").await;
    let (t, admin) = (boot.tenant_id, boot.admin_key.value.clone());
    let dora = dreamer_pg(&app, &admin, "dora-key", "dora", boot.root_workarea_id).await;

    let (mut created_won, mut ratified_won) = (0, 0);
    for round in 0..8 {
        let node = workarea_pg(
            &app,
            &admin,
            &format!("area-{round}"),
            boot.root_workarea_id,
        )
        .await;
        let to_committed = app
            .propose_transition(&admin, node.into(), WorkareaStatus::Committed)
            .await
            .expect("propose committed");
        app.ratify_transition(&dora.value, node.into(), to_committed.transition_id)
            .await
            .expect("ratify committed");
        let to_validated = app
            .propose_transition(&admin, node.into(), WorkareaStatus::Validated)
            .await
            .expect("propose validated with no child");

        let create = {
            let (app, admin) = (Arc::clone(&app), admin.clone());
            let child = name(&format!("child-{round}"));
            tokio::spawn(async move {
                loop {
                    let result = app
                        .create_workarea(&admin, &child, Some(node.into()), None)
                        .await;
                    if !lost_the_head(&result) {
                        return result.map(|c| c.workarea.workarea_id);
                    }
                }
            })
        };
        let ratify = {
            let (app, dreamer) = (Arc::clone(&app), dora.value.clone());
            let id = to_validated.transition_id;
            tokio::spawn(async move {
                loop {
                    let result = app.ratify_transition(&dreamer, node.into(), id).await;
                    if !lost_the_head(&result) {
                        return result.map(|w| w.status);
                    }
                }
            })
        };
        let (create, ratify) = (
            create.await.expect("the create task"),
            ratify.await.expect("the ratify task"),
        );

        match (&create, &ratify) {
            (Ok(_), Err(err)) => {
                assert!(
                    matches!(
                        err,
                        AppError::TransitionRefused {
                            reason: LadderRefusal::ChildrenNotValidated,
                            ..
                        } | AppError::Store(StoreError::ChildrenNotValidated { .. })
                    ),
                    "round {round}: the ratification after the child: {err:?}"
                );
                created_won += 1;
            }
            (Err(err), Ok(status)) => {
                assert_eq!(*status, WorkareaStatus::Validated);
                assert!(
                    matches!(
                        err,
                        AppError::WorkareaFrozen {
                            op: FrozenOp::CreateChild,
                            ..
                        }
                    ),
                    "round {round}: the create after the ratification: {err:?}"
                );
                ratified_won += 1;
            }
            other => panic!("round {round}: exactly one lands, got {other:?}"),
        }
        assert_eq!(
            validated_over_unvalidated(&store, t).await,
            Vec::<String>::new(),
            "round {round}: no validated node over an unvalidated child"
        );
    }
    println!("the create landed {created_won} times, the ratification {ratified_won}");
    let keys = KeySet::new(vec![test_signing_key().verifying_key()]);
    store
        .verify_stored_chain(t, &keys)
        .await
        .expect("the chain verifies after the races");
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// claim_edges (0018): the keys, the checks, the fence and the grant
// ---------------------------------------------------------------------------

/// One tenant's two memories and two writer keys, under its `universe`, for
/// the claim-edge laws: the edge's endpoints and its proposer and ratifier.
struct EdgeTenant {
    rig: ChainRig,
    src: Uuid,
    dst: Uuid,
    proposer: Uuid,
    ratifier: Uuid,
}

/// Seeds `seed`'s chain with a `universe`, two keys and two held memories,
/// ids drawn from `base` so two tenants in one database never share one.
async fn edge_tenant(store: &PgStore, seed: u64, base: u128) -> EdgeTenant {
    let mut rig = ChainRig::new(seed);
    rig.seed_chain(store, 0).await;
    let root = Uuid::from_u128(base);
    rig.create_workarea(
        store,
        workarea_record(rig.tenant_id, root, None, "universe"),
    )
    .await;
    let (proposer, ratifier) = (Uuid::from_u128(base + 1), Uuid::from_u128(base + 2));
    for key in [proposer, ratifier] {
        rig.issue_key(store, key_record(rig.tenant_id, key, Role::Writer, None))
            .await;
    }
    let (src, dst) = (Uuid::from_u128(base + 3), Uuid::from_u128(base + 4));
    hold(store, &mut rig, root, src, proposer, false).await;
    hold(store, &mut rig, root, dst, proposer, false).await;
    EdgeTenant {
        rig,
        src,
        dst,
        proposer,
        ratifier,
    }
}

/// One `claim_edges` row as the serving login inserts it under `tenant`.
struct EdgeRow {
    id: Uuid,
    src: Uuid,
    dst: Uuid,
    state: &'static str,
    proposer: Uuid,
    ratifier: Option<Uuid>,
}

/// Inserts `edge` over the serving login under `tenant`, committed when
/// `commit` is set and rolled back otherwise; the refusal's SQLSTATE and
/// constraint, or the rows it inserted.
async fn edge_under(
    store: &PgStore,
    tenant: Uuid,
    edge: &EdgeRow,
    commit: bool,
) -> Result<u64, (String, Option<String>)> {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let result = sqlx::query(
        "INSERT INTO claim_edges (id, tenant_id, src_artifact_id, dst_artifact_id, label, \
                                  state, proposer_key_id, ratifier_key_id) \
         VALUES ($1, $2, $3, $4, 'relates_to', $5, $6, $7)",
    )
    .bind(edge.id)
    .bind(tenant)
    .bind(edge.src)
    .bind(edge.dst)
    .bind(edge.state)
    .bind(edge.proposer)
    .bind(edge.ratifier)
    .execute(&mut *tx)
    .await;
    match result {
        Ok(done) => {
            if commit {
                tx.commit().await.expect("commit");
            } else {
                tx.rollback().await.expect("rollback");
            }
            Ok(done.rows_affected())
        }
        Err(e) => {
            let db = e
                .as_database_error()
                .unwrap_or_else(|| panic!("not a database error: {e}"));
            Err((
                db.code().map(|c| c.into_owned()).unwrap_or_default(),
                db.constraint().map(str::to_owned),
            ))
        }
    }
}

/// A pending edge between `t`'s two memories, proposed by its proposer.
fn pending_edge(t: &EdgeTenant, id: u128) -> EdgeRow {
    EdgeRow {
        id: Uuid::from_u128(id),
        src: t.src,
        dst: t.dst,
        state: "pending",
        proposer: t.proposer,
        ratifier: None,
    }
}

/// Under tenant A, a claim edge whose endpoint or key is tenant B's row is
/// refused as one naming no row is: `23503`, from the one constraint, so
/// the edge neither stores a cross-tenant id nor answers whether another
/// tenant holds one; tenant A's own rows are accepted. A foreign-key check
/// runs without row-level security, so each key carries `tenant_id`
/// (postgres-schema.md "Keys into tenant tables"; ADR-0034 d12, ADR-0008).
#[tokio::test]
async fn a_claim_edge_naming_tenant_bs_memory_is_refused_as_one_naming_no_memory() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let a = edge_tenant(&store, 300, 0x3000).await;
    let b = edge_tenant(&store, 301, 0x3100).await;
    let tenant = a.rig.tenant_id;
    let nothing = Uuid::from_u128(0x0BAD_0011);

    let own = EdgeRow {
        ratifier: Some(a.ratifier),
        state: "ratified",
        ..pending_edge(&a, 0x3E00)
    };
    assert_eq!(
        edge_under(&store, tenant, &own, false).await,
        Ok(1),
        "tenant A's own memories and keys are accepted"
    );

    type Point = fn(&mut EdgeRow, Uuid);
    let references: [(&str, Point, Uuid); 4] = [
        ("claim_edges_src_artifact_id_fkey", |e, v| e.src = v, b.src),
        ("claim_edges_dst_artifact_id_fkey", |e, v| e.dst = v, b.dst),
        (
            "claim_edges_proposer_key_id_fkey",
            |e, v| e.proposer = v,
            b.proposer,
        ),
        (
            "claim_edges_ratifier_key_id_fkey",
            |e, v| e.ratifier = Some(v),
            b.ratifier,
        ),
    ];
    for (constraint, point, foreign) in references {
        let mut random_row = EdgeRow {
            ratifier: Some(a.ratifier),
            state: "ratified",
            ..pending_edge(&a, 0x3E01)
        };
        point(&mut random_row, nothing);
        let mut foreign_row = EdgeRow {
            ratifier: Some(a.ratifier),
            state: "ratified",
            ..pending_edge(&a, 0x3E02)
        };
        point(&mut foreign_row, foreign);
        let random = edge_under(&store, tenant, &random_row, false).await;
        assert_eq!(
            random,
            Err(("23503".to_owned(), Some(constraint.to_owned()))),
            "{constraint}: an id no row holds is refused by the one constraint"
        );
        assert_eq!(
            edge_under(&store, tenant, &foreign_row, false).await,
            random,
            "{constraint}: tenant B's row is refused as an id no row holds is"
        );
    }
    drop_dbs(&created).await;
}

/// 0018 grants `claim_edges` SELECT, INSERT and UPDATE alone: nothing
/// deletes an edge, so the serving role's DELETE fails on privilege
/// (`42501`) while its insert, ratifying update and read succeed
/// (postgres-schema.md "Privileges"; ADR-0034 d3).
#[tokio::test]
async fn the_serving_role_is_refused_delete_on_claim_edges_by_privilege() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let a = edge_tenant(&store, 302, 0x3200).await;
    let tenant = a.rig.tenant_id;
    let edge = pending_edge(&a, 0x3E10);
    assert_eq!(edge_under(&store, tenant, &edge, true).await, Ok(1));

    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let ratified = sqlx::query(
        "UPDATE claim_edges SET state = 'ratified', ratifier_key_id = $1 WHERE id = $2",
    )
    .bind(a.ratifier)
    .bind(edge.id)
    .execute(&mut *tx)
    .await
    .expect("the serving role ratifies an edge")
    .rows_affected();
    assert_eq!(ratified, 1);
    let read: i64 = sqlx::query_scalar("SELECT count(*) FROM claim_edges WHERE state = 'ratified'")
        .fetch_one(&mut *tx)
        .await
        .expect("the serving role reads its edges");
    assert_eq!(read, 1);
    let result = sqlx::query("DELETE FROM claim_edges WHERE id = $1")
        .bind(edge.id)
        .execute(&mut *tx)
        .await;
    assert_eq!(
        refused_code(result, "DELETE FROM claim_edges"),
        "42501",
        "the serving role is refused DELETE on claim_edges"
    );
    drop(tx);
    drop_dbs(&created).await;
}

/// `claim_edges` carries the FORCE `tenant_isolation` policy 0009 gave
/// `memories`: under tenant A's `oml.tenant_id`, or none, no row of tenant
/// B's shows, while tenant B reads its own (postgres-schema.md "Row-level
/// security"; security.md).
#[tokio::test]
async fn tenant_as_context_reads_none_of_tenant_bs_claim_edges() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let a = edge_tenant(&store, 304, 0x3400).await;
    let b = edge_tenant(&store, 305, 0x3500).await;
    assert_eq!(
        edge_under(&store, b.rig.tenant_id, &pending_edge(&b, 0x3E20), true).await,
        Ok(1)
    );

    let flags: (bool, bool) = sqlx::query_as(
        "SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE relname = 'claim_edges'",
    )
    .fetch_one(store.pool())
    .await
    .expect("table flags");
    assert_eq!(flags, (true, true), "row security is enabled and forced");

    let mut seen = Vec::new();
    for tenant in [Some(a.rig.tenant_id), None, Some(b.rig.tenant_id)] {
        let mut tx = store.pool().begin().await.expect("begin");
        if let Some(tenant) = tenant {
            sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
                .bind(tenant.to_string())
                .execute(&mut *tx)
                .await
                .expect("set tenant");
        }
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM claim_edges")
            .fetch_one(&mut *tx)
            .await
            .expect("count");
        tx.rollback().await.expect("rollback");
        seen.push(n);
    }
    assert_eq!(
        seen,
        vec![0, 0, 1],
        "tenant A and no tenant read none of tenant B's edges; tenant B reads its own"
    );
    drop_dbs(&created).await;
}

/// The key-inequality CHECK is R2's cheap spelling on edges: a ratifier key
/// equal to the proposer key is refused (`23514`) by
/// `claim_edges_ratifier_is_not_proposer`; the principal rule is the
/// ratification transaction's (postgres-schema.md; ADR-0012).
#[tokio::test]
async fn a_claim_edge_whose_ratifier_is_its_proposer_is_refused_by_its_check() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let a = edge_tenant(&store, 306, 0x3600).await;
    let selfish = EdgeRow {
        state: "ratified",
        ratifier: Some(a.proposer),
        ..pending_edge(&a, 0x3E30)
    };
    assert_eq!(
        edge_under(&store, a.rig.tenant_id, &selfish, false).await,
        Err((
            "23514".to_owned(),
            Some("claim_edges_ratifier_is_not_proposer".to_owned())
        ))
    );
    drop_dbs(&created).await;
}

/// An edge is ratified exactly when it names a ratifier: a ratified row
/// without one, and a pending row with one, are refused (`23514`) by
/// `claim_edges_ratified_has_ratifier` (postgres-schema.md; ADR-0008).
#[tokio::test]
async fn a_ratified_claim_edge_without_a_ratifier_is_refused_by_its_check() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let a = edge_tenant(&store, 307, 0x3700).await;
    let refusal = Err((
        "23514".to_owned(),
        Some("claim_edges_ratified_has_ratifier".to_owned()),
    ));
    let unratified = EdgeRow {
        state: "ratified",
        ..pending_edge(&a, 0x3E40)
    };
    assert_eq!(
        edge_under(&store, a.rig.tenant_id, &unratified, false).await,
        refusal,
        "a ratified edge without a ratifier"
    );
    let premature = EdgeRow {
        ratifier: Some(a.ratifier),
        ..pending_edge(&a, 0x3E41)
    };
    assert_eq!(
        edge_under(&store, a.rig.tenant_id, &premature, false).await,
        refusal,
        "a pending edge with a ratifier"
    );
    drop_dbs(&created).await;
}

/// The planner's row estimate, as `EXPLAIN (FORMAT JSON)` gives it, for
/// `query` over the serving login under `tenant`, beside the rows it returns.
async fn estimate_under(store: &PgStore, tenant: Uuid, query: &str) -> (f64, i64) {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let plan: Value = sqlx::query_scalar(&format!("EXPLAIN (FORMAT JSON) {query}"))
        .fetch_one(&mut *tx)
        .await
        .expect("explain");
    let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM ({query}) q"))
        .fetch_one(&mut *tx)
        .await
        .expect("the rows");
    tx.rollback().await.expect("rollback");
    let estimated = plan[0]["Plan"]["Plan Rows"]
        .as_f64()
        .unwrap_or_else(|| panic!("no Plan Rows in {plan}"));
    (estimated, rows)
}

/// After the owner analyses `claim_edges`, each probe pair's estimate and
/// rows under `tenant`: another tenant's value first, an unused one second.
async fn edge_estimates(
    store: &PgStore,
    owner: &mut PgConnection,
    tenant: Uuid,
    probes: &[(String, String)],
) -> Vec<((f64, i64), (f64, i64))> {
    owner
        .execute("ANALYZE claim_edges")
        .await
        .expect("the owner analyses");
    let mut found = Vec::new();
    for (theirs, unused) in probes {
        found.push((
            estimate_under(store, tenant, theirs).await,
            estimate_under(store, tenant, unused).await,
        ));
    }
    found
}

/// Under tenant A, EXPLAIN estimates as many claim edges for a label only
/// tenant B's edges carry, and for one of tenant B's endpoints, as for a
/// label and an id no tenant holds: 0018 keeps no column statistics on
/// `claim_edges`, whose labels are tenant text, so the estimate cannot tell
/// tenant A whether another tenant uses an edge label (FR-45; ADR-0034 d1,
/// d11; spike 3's S3.4/estimate). The control restores the default target,
/// and the same estimates then differ.
#[tokio::test]
async fn under_tenant_a_explain_estimates_tenant_bs_edge_label_as_it_estimates_an_unused_one() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_store(&created).await;
    let a = edge_tenant(&store, 304, 0x3400).await;
    let b = edge_tenant(&store, 305, 0x3500).await;
    let mut owner = owner_conn(&store).await;
    // Each tenant holds as many edges, each under a label of its own, so the
    // default statistics, the control's, tell the two tenants' values apart.
    for (t, label) in [(&a, "own:relates_to"), (&b, "acme:merger_target")] {
        sqlx::query("SELECT set_config('oml.tenant_id', $1, false)")
            .bind(t.rig.tenant_id.to_string())
            .execute(&mut owner)
            .await
            .expect("the owner set to the tenant");
        sqlx::query(
            "INSERT INTO claim_edges (id, tenant_id, src_artifact_id, dst_artifact_id, label, \
                                      state, proposer_key_id, ratifier_key_id) \
             SELECT gen_random_uuid(), $1, $2, $3, $4, 'ratified', $5, $6 \
               FROM generate_series(1, 2000)",
        )
        .bind(t.rig.tenant_id)
        .bind(t.src)
        .bind(t.dst)
        .bind(label)
        .bind(t.proposer)
        .bind(t.ratifier)
        .execute(&mut owner)
        .await
        .expect("the tenant's edges");
    }

    let probes = [
        (
            "SELECT id FROM claim_edges \
              WHERE state = 'ratified' AND label = 'acme:merger_target'"
                .to_owned(),
            "SELECT id FROM claim_edges \
              WHERE state = 'ratified' AND label = 'zzz:unused_label'"
                .to_owned(),
        ),
        (
            format!(
                "SELECT id FROM claim_edges WHERE src_artifact_id = '{}'",
                b.src
            ),
            format!(
                "SELECT id FROM claim_edges WHERE src_artifact_id = '{}'",
                Uuid::from_u128(0x0BAD_0011)
            ),
        ),
    ];
    let tenant = a.rig.tenant_id;
    for ((theirs, unused), (query, _)) in edge_estimates(&store, &mut owner, tenant, &probes)
        .await
        .into_iter()
        .zip(&probes)
    {
        assert_eq!(
            theirs.1, 0,
            "tenant A reads none of tenant B's edges: {query}"
        );
        assert_eq!(
            theirs.0, unused.0,
            "under tenant A, {query} is estimated as an unused value is \
             (estimated, returned: {theirs:?} and {unused:?})"
        );
    }

    // Control: with the default statistics target the same estimates differ,
    // so the law above measures 0018's setting, not an empty table.
    for column in ["label", "src_artifact_id", "state", "tenant_id"] {
        owner
            .execute(
                format!("ALTER TABLE claim_edges ALTER COLUMN {column} SET STATISTICS -1").as_str(),
            )
            .await
            .expect("the owner restores the default target");
    }
    for ((theirs, unused), (query, _)) in edge_estimates(&store, &mut owner, tenant, &probes)
        .await
        .into_iter()
        .zip(&probes)
    {
        assert_ne!(
            theirs.0, unused.0,
            "control: with column statistics, {query} is estimated apart from an unused value"
        );
    }
    drop(owner);
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// Stored search lexemes (migrations/0020_search.sql; ADR-0049 d12)
// ---------------------------------------------------------------------------

/// The bodies the stored-lexeme laws write: an identifier, inflected words,
/// a word repeated, and English stop words.
const LEXEME_BODIES: [&str; 5] = [
    "The hook lives in useInvoiceFilters.ts and deploys nightly",
    "prices rose and the price of aluminium rose again",
    "deploy deploy deploy the canary",
    "rollback the release with one command",
    "Q1-2027 delivery for quote AM-7741",
];

/// Holds and promotes `LEXEME_BODIES` on `wa` through `rig`, and holds one
/// more row there unpromoted; the promoted ids, then the held one.
async fn lexeme_rows(rig: &mut ChainRig, store: &PgStore, wa: Uuid) -> (Vec<Uuid>, Uuid) {
    let mut promoted = Vec::new();
    for (i, body) in LEXEME_BODIES.iter().enumerate() {
        let id = Uuid::from_u128(0xA20_0000 + i as u128);
        let m = held_memory(rig.tenant_id, wa, id, "rita", Uuid::from_u128(0xE5), body);
        oml_port_contracts::hold_and_promote(rig, store, m).await;
        promoted.push(id);
    }
    let held = Uuid::from_u128(0xA20_00FF);
    oml_port_contracts::hold(
        rig,
        store,
        held_memory(
            rig.tenant_id,
            wa,
            held,
            "rita",
            Uuid::from_u128(0xE5),
            "kubernetes ingress held for review",
        ),
    )
    .await
    .expect("hold");
    (promoted, held)
}

/// Whether every row of `tenant` holds `oml_search_vector(claim)` as its
/// stored `search_vector` and that vector's length as `search_len`, read by
/// the serving role under the tenant's row security; and how many rows.
async fn stored_lexemes_hold(store: &PgStore, tenant: Uuid) -> (bool, i64) {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let row: (Option<bool>, i64) = sqlx::query_as(
        "SELECT bool_and(search_vector = oml_search_vector(claim) \
                         AND search_len = length(search_vector)), count(*) \
           FROM memories WHERE tenant_id = $1",
    )
    .bind(tenant)
    .fetch_one(&mut *tx)
    .await
    .expect("stored lexemes");
    tx.commit().await.expect("commit");
    (row.0.unwrap_or(false), row.1)
}

/// Each version's search vector, `simple` and `english` over the claim with
/// its identifier parts, is set once when the row is written, with its
/// length beside it, and a write cannot store lexemes that are not its
/// claim's; no text index is kept on it or on the vocabulary, since under
/// row security none would serve the serving role (ADR-0049 d12; ADR-0034
/// d13; postgres-schema.md, search).
#[tokio::test]
async fn every_memory_versions_search_vector_is_set_once_by_its_write_and_no_index_the_serving_role_cannot_use_is_kept()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(560);
    rig.seed_chain(&store, 0).await;
    let wa = Uuid::from_u128(0xE2);
    let (promoted, _) = lexeme_rows(&mut rig, &store, wa).await;
    assert_eq!(
        stored_lexemes_hold(&store, rig.tenant_id).await,
        (true, 6),
        "every version, held or promoted, stores its lexemes"
    );
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    sqlx::query(
        "UPDATE memories SET search_vector = to_tsvector('simple', 'zebra'), search_len = 1 \
          WHERE tenant_id = $1 AND id = $2",
    )
    .bind(rig.tenant_id)
    .bind(promoted[0])
    .execute(&mut *tx)
    .await
    .expect("a write naming the lexemes");
    tx.commit().await.expect("commit");
    assert_eq!(
        stored_lexemes_hold(&store, rig.tenant_id).await,
        (true, 6),
        "the write stores its claim's lexemes, not the ones it named"
    );
    let mut owner = owner_conn(&store).await;
    let text_indexes: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid \
           JOIN pg_class t ON t.oid = i.indrelid \
          WHERE t.relname IN ('memories', 'search_vocabulary') \
            AND (pg_get_indexdef(i.indexrelid) LIKE '%search_vector%' \
                 OR pg_get_indexdef(i.indexrelid) LIKE '%gin_trgm_ops%') \
          ORDER BY 1",
    )
    .fetch_all(&mut owner)
    .await
    .expect("indexes");
    assert_eq!(text_indexes, Vec::<String>::new());
    drop(owner);
    drop_dbs(&created).await;
}

/// The `bm25` statement scores from the stored lexemes: its plan reads no
/// row's claim, so no query rebuilds a vector from text (ADR-0049 d12;
/// eval/bench/REPORT.md, latency).
#[tokio::test]
async fn the_bm25_statement_reads_the_stored_lexemes_and_never_a_rows_claim() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(561);
    rig.seed_chain(&store, 0).await;
    let wa = Uuid::from_u128(0xE2);
    lexeme_rows(&mut rig, &store, wa).await;
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let plan: Vec<String> = sqlx::query_scalar(&format!(
        "EXPLAIN (VERBOSE, COSTS OFF) {}",
        oml_pg::bm25_sql()
    ))
    .bind(rig.tenant_id)
    .bind(vec![wa])
    .bind(vec!["memory", "derived_fact"])
    .bind(Vec::<Uuid>::new())
    .bind(Vec::<Uuid>::new())
    .bind("the prices of useInvoiceFilters")
    .bind(10_i64)
    .bind(false)
    .bind(i64::MIN)
    .fetch_all(&mut *tx)
    .await
    .expect("plan");
    tx.commit().await.expect("commit");
    let plan = plan.join("\n");
    assert!(plan.contains("search_vector"), "{plan}");
    // VERBOSE qualifies every column it reads (`m.claim`); an index's name
    // may hold the word.
    assert!(
        !plan.contains(".claim") && !plan.contains("(claim"),
        "no row's claim is read, so no vector is rebuilt: {plan}"
    );
    drop_dbs(&created).await;
}

/// The bm25 list of `query` over `wa`, with own held rows of `own`.
async fn bm25_list(
    store: &PgStore,
    tenant: Uuid,
    wa: Uuid,
    query: &str,
) -> Vec<(Uuid, f32, Vec<String>)> {
    store
        .retrieve(
            tenant,
            RetrieverKind::Bm25,
            &CandidateScope {
                anchor: oml_app::types::AnchorFilter::default(),
                promoted_nodes: vec![wa],
                kinds: oml_app::types::HIT_KINDS.to_vec(),
                own_held: None,
                expired_at: None,
            },
            RetrievalQuery {
                query,
                match_mode: MatchMode::Any,
                k: 50,
                sparse: &[],
                window: None,
                within: &[],
            },
        )
        .await
        .expect("bm25")
        .candidates
        .into_iter()
        .map(|c| (c.memory_id, c.score, c.matched))
        .collect()
}

/// The vocabulary `did_you_mean` reads for `wa`, as the serving role sees
/// it under the tenant's row security: (word, rows) in word order.
async fn vocabulary(store: &PgStore, tenant: Uuid, wa: Uuid) -> Vec<(String, i32)> {
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let words = sqlx::query_as(
        "SELECT word, rows FROM search_vocabulary \
          WHERE tenant_id = $1 AND workarea_id = $2 ORDER BY word COLLATE \"C\"",
    )
    .bind(tenant)
    .bind(wa)
    .fetch_all(&mut *tx)
    .await
    .expect("vocabulary");
    tx.commit().await.expect("commit");
    words
}

/// A database shaped like v0.5.4's, migrated through 0019 and holding
/// promoted and held memories, migrates through 0020: every row's lexemes
/// are stored, the vocabulary holds the words of the promoted tips alone,
/// nothing is journalled, and bm25 ranks as it does over the same rows
/// written after 0020 (ADR-0049 d12; upgrades.md).
#[tokio::test]
async fn a_v0_5_4_database_with_memories_migrates_through_0020_storing_every_rows_lexemes_and_ranking_as_a_fresh_one()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 19).await;
    let upgraded = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    let mut rig = ChainRig::new(562);
    rig.seed_chain(&upgraded, 0).await;
    let wa = Uuid::from_u128(0xE2);
    lexeme_rows(&mut rig, &upgraded, wa).await;
    let journal_before: i64 = sqlx::query_scalar("SELECT count(*) FROM journal")
        .fetch_one(&mut conn)
        .await
        .expect("count");
    upgraded
        .migrate(&owner_url(&db))
        .await
        .expect("0020 applies over a v0.5.4 database");
    let upgraded = upgraded.with_search_probe().await.expect("probe");
    assert_eq!(
        stored_lexemes_hold(&upgraded, rig.tenant_id).await,
        (true, 6),
        "0020 stores every existing row's lexemes"
    );
    let journal_after: i64 = sqlx::query_scalar("SELECT count(*) FROM journal")
        .fetch_one(&mut conn)
        .await
        .expect("count");
    assert_eq!(
        journal_after, journal_before,
        "the upgrade journals nothing"
    );
    let applied: i64 =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success")
            .fetch_one(&mut conn)
            .await
            .expect("versions");
    // The ledger's highest version is read (ADR-0030 d14).
    assert_eq!(applied, 37, "every migration through 0037_open_signup");
    drop(conn);

    let fresh = fresh_search_store(&created).await;
    let mut twin = ChainRig::new(562);
    twin.seed_chain(&fresh, 0).await;
    lexeme_rows(&mut twin, &fresh, wa).await;
    let words = vocabulary(&upgraded, rig.tenant_id, wa).await;
    assert!(
        !words.is_empty(),
        "0020 fills the vocabulary of existing rows"
    );
    assert!(
        !words.iter().any(|(w, _)| w == "kubernetes"),
        "a held row's words are not in the vocabulary: {words:?}"
    );
    assert_eq!(
        words,
        vocabulary(&fresh, twin.tenant_id, wa).await,
        "the backfilled vocabulary is the one the writes keep"
    );
    for query in [
        "price",
        "the prices of aluminium",
        "invoice filters",
        "deploy canary",
        "AM-7741",
        "zebra",
    ] {
        assert_eq!(
            bm25_list(&upgraded, rig.tenant_id, wa, query).await,
            bm25_list(&fresh, twin.tenant_id, wa, query).await,
            "{query}: the upgraded database ranks as a fresh one"
        );
    }
    drop_dbs(&created).await;
}

// ---------------------------------------------------------------------------
// Integer postings (migrations/0021_search_postings.sql; ADR-0052)
// ---------------------------------------------------------------------------

/// A fresh store whose `bm25_postings` prunes by `limits`.
async fn postings_store(created: &Mutex<Vec<String>>, limits: oml_pg::PruneLimits) -> PgStore {
    fresh_search_store(created).await.with_prune_limits(limits)
}

/// Forms above a share of 0.3 of the readable rows are pruned once the
/// query's readable postings pass a budget of 5, which the fixture's
/// frequent words do: the limits the pruning laws name.
const LAW_PRUNING: oml_pg::PruneLimits = oml_pg::PruneLimits {
    frequent_share: 0.3,
    posting_budget: 5,
};

/// The same share with a budget the fixture's postings fit.
const LAW_FITTING: oml_pg::PruneLimits = oml_pg::PruneLimits {
    frequent_share: 0.3,
    posting_budget: 1_000,
};

/// The postings laws of `oml-port-contracts` against the real store, each
/// with the limits its doc names (ADR-0052 d12).
#[tokio::test]
async fn bm25_postings_contract_laws() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    oml_port_contracts::bm25_postings_ranks_as_bm25_does_when_nothing_is_pruned(
        &postings_store(&created, oml_pg::PruneLimits::NONE).await,
    )
    .await;
    oml_port_contracts::bm25_postings_ranks_as_bm25_does_when_a_readable_row_has_expired(
        &postings_store(&created, oml_pg::PruneLimits::NONE).await,
    )
    .await;
    oml_port_contracts::a_pruned_list_scores_each_of_its_rows_as_bm25_does(
        &postings_store(&created, LAW_PRUNING).await,
    )
    .await;
    oml_port_contracts::which_words_are_pruned_depends_on_the_readable_rows_alone(
        &postings_store(&created, LAW_PRUNING).await,
    )
    .await;
    oml_port_contracts::a_query_of_only_frequent_words_is_answered_by_the_scan_and_says_so(
        &postings_store(&created, LAW_PRUNING).await,
    )
    .await;
    oml_port_contracts::a_pruned_list_is_bm25s_over_the_rows_its_unpruned_words_reach(
        &postings_store(&created, LAW_PRUNING).await,
    )
    .await;
    oml_port_contracts::a_scope_whose_postings_fit_the_budget_is_read_whole(
        &postings_store(&created, LAW_FITTING).await,
    )
    .await;
    for limits in [
        oml_pg::PruneLimits::NONE,
        LAW_PRUNING,
        oml_pg::PruneLimits::SERVED,
    ] {
        oml_port_contracts::a_word_only_unreadable_rows_hold_is_answered_as_a_word_no_row_holds(
            &postings_store(&created, limits).await,
        )
        .await;
    }
    // The scope and no-leak laws, with pruning that bites on their fixture.
    oml_port_contracts::a_retriever_never_returns_a_row_outside_the_scope(
        &postings_store(&created, LAW_PRUNING).await,
    )
    .await;
    oml_port_contracts::a_row_outside_the_scope_never_changes_a_candidates_score(|| {
        postings_store(&created, LAW_PRUNING)
    })
    .await;
    drop_dbs(&created).await;
}

/// `oml_token_id` in SQL and [`oml_pg::token_id`] give every lexeme the same
/// id, ASCII, accented, CJK, an identifier part and the empty string alike
/// (ADR-0052 d2, d12).
#[tokio::test]
async fn oml_token_id_in_sql_equals_the_adapters() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let lexemes = [
        "",
        "a",
        "billing",
        "deploy",
        "useinvoicefilters",
        "am-7741",
        "café",
        "naïve",
        "東京",
        "ünïcødé",
        "x'y",
        "back\\slash",
    ];
    for lexeme in lexemes {
        let sql: i64 = sqlx::query_scalar("SELECT oml_token_id($1)")
            .bind(lexeme)
            .fetch_one(store.pool())
            .await
            .expect("oml_token_id");
        assert_eq!(sql, oml_pg::token_id(lexeme), "{lexeme:?}");
    }
    drop_dbs(&created).await;
}

/// `oml_similar_keys` and `oml_similar_words` in SQL and
/// `oml_domain::similar` give every text the same words and keys, so a
/// write's keys meet the stored ones (ADR-0050 d17).
#[tokio::test]
async fn similar_keys_computed_in_sql_equal_the_cores() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let texts = [
        "",
        "--- !!!",
        "ok",
        "Port 8080, not 8081.",
        "Production deploys run only on Tuesday and Thursday between 10:00 and 12:00 CET; \
         outside that window a deploy needs the on-call lead's approval.",
        "Use `pnpm`, not NPM: v0.5.7 — ok? Grüße aus Köln, 東京 and naïve café.",
        "UPPER lower MiXeD\ttabs\nnew lines  and \u{a0}no-break spaces",
        "x'y back\\slash under_score dash-ed dot.ted",
        "yes yes yes yes yes yes yes yes yes yes yes",
    ];
    for text in texts {
        let words: Vec<String> = sqlx::query_scalar("SELECT oml_similar_words($1)")
            .bind(text)
            .fetch_one(store.pool())
            .await
            .expect("oml_similar_words");
        assert_eq!(words, oml_app::domain::similar::words(text), "{text:?}");
        let mut sql: Vec<(i16, i64)> = sqlx::query_as("SELECT band, key FROM oml_similar_keys($1)")
            .bind(text)
            .fetch_all(store.pool())
            .await
            .expect("oml_similar_keys");
        sql.sort_unstable();
        let mut core = oml_app::domain::similar::Fingerprint::of(text).stored_keys();
        core.sort_unstable();
        assert_eq!(sql, core, "{text:?}");
    }
    drop_dbs(&created).await;
}

/// What the postings and counts of `tenant` hold, read as the owner under
/// the tenant's row security: per memory, its postings' (state, count); per
/// (workarea, memory kind, token), the count row; per (workarea, memory
/// kind), the row count and summed length.
type Postings = (
    BTreeMap<Uuid, (i16, i64)>,
    BTreeMap<(Uuid, i16, i64), i32>,
    BTreeMap<(Uuid, i16), (i32, i64)>,
);

async fn postings_of(store: &PgStore, tenant: Uuid) -> Postings {
    let mut owner = owner_conn(store).await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let per_memory: Vec<(Uuid, i16, i64)> = sqlx::query_as(
        "SELECT memory_id, max(state), count(*) FROM search_postings \
          WHERE tenant_id = $1 GROUP BY memory_id HAVING min(state) = max(state)",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("postings");
    let mixed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT memory_id FROM search_postings WHERE tenant_id = $1 \
          GROUP BY memory_id HAVING min(state) <> max(state)) x",
    )
    .bind(tenant)
    .fetch_one(&mut *tx)
    .await
    .expect("mixed");
    assert_eq!(mixed, 0, "a version's postings share one state");
    let terms: Vec<(Uuid, i16, i64, i32)> = sqlx::query_as(
        "SELECT workarea_id, memory_kind, token_id, rows FROM search_term_rows \
          WHERE tenant_id = $1 AND kind = 1",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("term rows");
    let areas: Vec<(Uuid, i16, i32, i64)> = sqlx::query_as(
        "SELECT workarea_id, memory_kind, rows, total_len FROM search_workarea_rows \
          WHERE tenant_id = $1 AND kind = 1",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("workarea rows");
    tx.commit().await.expect("commit");
    (
        per_memory
            .into_iter()
            .map(|(m, s, n)| (m, (s, n)))
            .collect(),
        terms
            .into_iter()
            .map(|(w, k, t, n)| ((w, k, t), n))
            .collect(),
        areas
            .into_iter()
            .map(|(w, k, n, l)| ((w, k), (n, l)))
            .collect(),
    )
}

/// The counts recomputed from the postings and the versions: what the
/// trigger's counts must equal (ADR-0052 d5).
async fn counts_from_postings(
    store: &PgStore,
    tenant: Uuid,
) -> (
    BTreeMap<(Uuid, i16, i64), i32>,
    BTreeMap<(Uuid, i16), (i32, i64)>,
) {
    let mut owner = owner_conn(store).await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let terms: Vec<(Uuid, i16, i64, i32)> = sqlx::query_as(
        "SELECT workarea_id, memory_kind, token_id, count(DISTINCT memory_id)::int4 \
           FROM search_postings WHERE tenant_id = $1 AND kind = 1 AND state = 1 \
          GROUP BY 1, 2, 3",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("terms");
    let areas: Vec<(Uuid, i16, i32, i64)> = sqlx::query_as(
        "SELECT workarea_id, oml_search_memory_kind(kind), count(*)::int4, sum(search_len)::int8 \
           FROM memories WHERE tenant_id = $1 \
            AND status = 'promoted' AND is_tip AND NOT retirement \
            AND kind IN ('memory', 'derived_fact') \
          GROUP BY 1, 2",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("areas");
    tx.commit().await.expect("commit");
    (
        terms
            .into_iter()
            .map(|(w, k, t, n)| ((w, k, t), n))
            .collect(),
        areas
            .into_iter()
            .map(|(w, k, n, l)| ((w, k), (n, l)))
            .collect(),
    )
}

/// Runs `sql` as the owner under `tenant`'s row security: the demotion and
/// erasure the folds do not build yet (ADR-0052 d7).
async fn as_owner(store: &PgStore, tenant: Uuid, sql: &str, id: Uuid) {
    let mut owner = owner_conn(store).await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    sqlx::query(sql)
        .bind(tenant)
        .bind(id)
        .execute(&mut *tx)
        .await
        .expect("owner statement");
    tx.commit().await.expect("commit");
}

/// Promoted, held, superseded, retired, rejected, demoted and steering rows
/// on two workareas, of both hit kinds; returns (replaced, retired tip,
/// demoted, rejected, served).
async fn lifecycle_rows(rig: &mut ChainRig, store: &PgStore) -> (Uuid, Uuid, Uuid, Uuid, Uuid) {
    let tenant = rig.tenant_id;
    let (wa, wb) = (Uuid::from_u128(0xE2), Uuid::from_u128(0xE3));
    let key = Uuid::from_u128(0xE5);
    let row = |id: u128, node: Uuid, body: &str| {
        held_memory(tenant, node, Uuid::from_u128(id), "rita", key, body)
    };
    for (id, node, body) in [
        (0xB01, wa, "deploy the billing service on friday"),
        (0xB02, wa, "the billing service owns invoices"),
        (0xB03, wb, "never deploy on friday evenings"),
        (0xB04, wb, "invoices are numbered per tenant"),
    ] {
        oml_port_contracts::hold_and_promote(rig, store, row(id, node, body)).await;
    }
    oml_port_contracts::hold_and_promote(
        rig,
        store,
        oml_app::types::MemoryRecord {
            kind: MemoryKind::DerivedFact,
            ..row(0xB05, wa, "billing derived fact for friday")
        },
    )
    .await;
    oml_port_contracts::hold_and_promote(
        rig,
        store,
        oml_app::types::MemoryRecord {
            kind: MemoryKind::Steering,
            ..row(0xB06, wa, "steering says billing")
        },
    )
    .await;
    oml_port_contracts::hold(rig, store, row(0xB07, wa, "billing held notes"))
        .await
        .expect("hold");
    oml_port_contracts::hold(rig, store, row(0xB08, wa, "billing to be rejected"))
        .await
        .expect("hold");
    oml_port_contracts::reject(rig, store, Uuid::from_u128(0xB08))
        .await
        .expect("reject");
    // 0xB01 is replaced by 0xB11; 0xB02 is retired by 0xB12; 0xB04 is
    // demoted by the owner, as the demotion fold will.
    let replaced = row(0xB01, wa, "deploy the billing service on friday");
    oml_port_contracts::supersede(
        rig,
        store,
        &replaced,
        Uuid::from_u128(0xB11),
        "deploy billing on monday",
        false,
    )
    .await;
    let retired = row(0xB02, wa, "the billing service owns invoices");
    oml_port_contracts::supersede(
        rig,
        store,
        &retired,
        Uuid::from_u128(0xB12),
        "the billing service owns invoices",
        true,
    )
    .await;
    as_owner(
        store,
        tenant,
        "UPDATE memories SET status = 'demoted', is_tip = false WHERE tenant_id = $1 AND id = $2",
        Uuid::from_u128(0xB04),
    )
    .await;
    (
        Uuid::from_u128(0xB01),
        Uuid::from_u128(0xB02),
        Uuid::from_u128(0xB04),
        Uuid::from_u128(0xB08),
        Uuid::from_u128(0xB03),
    )
}

/// The counts the trigger keeps equal the served postings counted in scope,
/// per workarea, memory kind and token, through promotion, supersession,
/// retirement, rejection and demotion (ADR-0052 d5, d12).
#[tokio::test]
async fn the_counts_summed_over_readable_nodes_equal_the_postings_counted_in_scope() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(571);
    rig.seed_chain(&store, 0).await;
    lifecycle_rows(&mut rig, &store).await;
    let (_, terms, areas) = postings_of(&store, rig.tenant_id).await;
    let (want_terms, want_areas) = counts_from_postings(&store, rig.tenant_id).await;
    assert!(!terms.is_empty(), "served versions are counted");
    assert_eq!(terms, want_terms, "per token");
    assert_eq!(areas, want_areas, "per workarea");
    assert_eq!(
        areas.keys().map(|(_, k)| *k).collect::<BTreeSet<_>>(),
        BTreeSet::from([1, 2]),
        "both hit kinds are counted apart"
    );
    drop_dbs(&created).await;
}

/// A superseded, demoted, retired or rejected version keeps its postings,
/// flipped off, and leaves the counts; a held one's are held; the served
/// tip's are served; a steering row and a retirement version have none
/// (ADR-0052 d1, d7, d12).
#[tokio::test]
async fn a_superseded_demoted_or_retired_version_keeps_its_postings_and_leaves_the_counts() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(572);
    rig.seed_chain(&store, 0).await;
    let (replaced, retired, demoted, rejected, served) = lifecycle_rows(&mut rig, &store).await;
    let (memories, terms, _) = postings_of(&store, rig.tenant_id).await;
    for (what, id) in [
        ("replaced", replaced),
        ("retired", retired),
        ("demoted", demoted),
        ("rejected", rejected),
    ] {
        let (state, n) = memories.get(&id).copied().unwrap_or((-1, 0));
        assert_eq!(state, 2, "the {what} version's postings are off");
        assert!(n > 0, "the {what} version keeps its postings");
    }
    assert_eq!(
        memories[&served].0, 1,
        "the served tip's postings are served"
    );
    assert_eq!(
        memories[&Uuid::from_u128(0xB11)].0,
        1,
        "the successor is served"
    );
    assert_eq!(
        memories[&Uuid::from_u128(0xB07)].0,
        0,
        "a held version's are held"
    );
    assert!(
        !memories.contains_key(&Uuid::from_u128(0xB06)),
        "a steering row has no postings"
    );
    assert!(
        !memories.contains_key(&Uuid::from_u128(0xB12)),
        "a retirement version has no postings"
    );
    // `invoices` was said by the retired and the demoted rows alone among
    // the served: no count holds it.
    let invoices = oml_pg::token_id("invoices");
    assert!(
        !terms.keys().any(|(_, _, t)| *t == invoices),
        "a word no served version says leaves the counts"
    );
    let monday = oml_pg::token_id("monday");
    assert_eq!(
        terms.get(&(Uuid::from_u128(0xE2), 1, monday)),
        Some(&1),
        "the successor's words are counted"
    );
    drop_dbs(&created).await;
}

/// An erased version leaves no posting and no count, whether the erase
/// marks it `erased` or deletes the row; the other versions' stay
/// (ADR-0052 d7; ADR-0005 d4).
#[tokio::test]
async fn an_erased_version_leaves_no_posting_and_no_count() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(573);
    rig.seed_chain(&store, 0).await;
    let (replaced, _, _, _, served) = lifecycle_rows(&mut rig, &store).await;
    let tenant = rig.tenant_id;
    as_owner(
        &store,
        tenant,
        "UPDATE memories SET status = 'erased', is_tip = false WHERE tenant_id = $1 AND id = $2",
        served,
    )
    .await;
    as_owner(
        &store,
        tenant,
        "UPDATE memories SET status = 'erased' WHERE tenant_id = $1 AND id = $2",
        replaced,
    )
    .await;
    as_owner(
        &store,
        tenant,
        "DELETE FROM review_queue WHERE tenant_id = $1 AND memory_id = $2",
        Uuid::from_u128(0xB07),
    )
    .await;
    as_owner(
        &store,
        tenant,
        "DELETE FROM memories WHERE tenant_id = $1 AND id = $2",
        Uuid::from_u128(0xB07),
    )
    .await;
    let (memories, terms, areas) = postings_of(&store, tenant).await;
    for id in [served, replaced, Uuid::from_u128(0xB07)] {
        assert!(!memories.contains_key(&id), "{id}: no posting is left");
    }
    let evenings = oml_pg::token_id("evenings");
    assert!(
        !terms.keys().any(|(_, _, t)| *t == evenings),
        "the erased version's words leave the counts"
    );
    assert_eq!(
        (terms, areas),
        counts_from_postings(&store, tenant).await,
        "the counts are the remaining served postings'"
    );
    assert!(
        memories.contains_key(&Uuid::from_u128(0xB11)),
        "the other versions keep theirs"
    );
    drop_dbs(&created).await;
}

/// The memories of `tenant` that hold similar keys, with how many, read as
/// the owner under the tenant's row security.
async fn similar_keys_of(store: &PgStore, tenant: Uuid) -> BTreeMap<Uuid, i64> {
    let mut owner = owner_conn(store).await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let rows: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT memory_id, count(*) FROM similar_keys WHERE tenant_id = $1 GROUP BY memory_id",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("similar keys");
    rows.into_iter().collect()
}

/// A replaced, retired, rejected, demoted or erased version, and a deleted
/// row, holds no similar key, and the keys the check reads never outlive the
/// text they were made from; doctor finds neither a gap nor a drift
/// afterwards (ADR-0050 d17, R4; ADR-0005 d4). A demotion or an erase that
/// leaves `is_tip` set is covered as much as one that clears it.
#[tokio::test]
async fn a_demoted_retired_or_erased_version_leaves_no_similar_key() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(574);
    rig.seed_chain(&store, 0).await;
    let (replaced, retired, demoted, rejected, served) = lifecycle_rows(&mut rig, &store).await;
    let tenant = rig.tenant_id;
    let before = similar_keys_of(&store, tenant).await;
    for (what, id) in [
        ("replaced", replaced),
        ("retired", retired),
        ("demoted", demoted),
        ("rejected", rejected),
        ("retirement version", Uuid::from_u128(0xB12)),
    ] {
        assert!(!before.contains_key(&id), "the {what} version holds no key");
    }
    for (what, id) in [
        ("served tip", served),
        ("successor", Uuid::from_u128(0xB11)),
        ("held", Uuid::from_u128(0xB07)),
        ("derived fact", Uuid::from_u128(0xB05)),
        ("steering", Uuid::from_u128(0xB06)),
    ] {
        assert!(before.contains_key(&id), "the {what} version holds keys");
    }
    // A demotion and an erase that leave is_tip set.
    as_owner(
        &store,
        tenant,
        "UPDATE memories SET status = 'demoted' WHERE tenant_id = $1 AND id = $2",
        served,
    )
    .await;
    as_owner(
        &store,
        tenant,
        "UPDATE memories SET status = 'erased' WHERE tenant_id = $1 AND id = $2",
        Uuid::from_u128(0xB05),
    )
    .await;
    // A held row erased by deletion.
    as_owner(
        &store,
        tenant,
        "DELETE FROM review_queue WHERE tenant_id = $1 AND memory_id = $2",
        Uuid::from_u128(0xB07),
    )
    .await;
    as_owner(
        &store,
        tenant,
        "DELETE FROM memories WHERE tenant_id = $1 AND id = $2",
        Uuid::from_u128(0xB07),
    )
    .await;
    let after = similar_keys_of(&store, tenant).await;
    for id in [served, Uuid::from_u128(0xB05), Uuid::from_u128(0xB07)] {
        assert!(!after.contains_key(&id), "{id}: no key is left");
    }
    assert_eq!(
        after.keys().copied().collect::<Vec<_>>(),
        vec![Uuid::from_u128(0xB06), Uuid::from_u128(0xB11)],
        "the other versions keep theirs"
    );
    let health = store
        .postings_health(tenant)
        .await
        .expect("health")
        .expect("PostgreSQL keeps keys");
    assert_eq!(
        (health.similar_unfilled, health.similar_drift),
        (0, 0),
        "{health:?}"
    );
    drop_dbs(&created).await;
}

/// Doctor's similar-key health names a version with no key and a key in the
/// wrong state, from the sample it reads, and reads clean again after the
/// owner's refill (ADR-0050 d15, d17).
#[tokio::test]
async fn the_similar_health_names_a_missing_key_and_a_wrong_state_until_a_refill() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(575);
    rig.seed_chain(&store, 0).await;
    let (_, _, _, _, served) = lifecycle_rows(&mut rig, &store).await;
    let tenant = rig.tenant_id;
    let health = || async {
        let h = store
            .postings_health(tenant)
            .await
            .expect("health")
            .expect("PostgreSQL keeps keys");
        (h.similar_partition, h.similar_unfilled, h.similar_drift)
    };
    assert_eq!(health().await, (true, 0, 0));
    as_owner(
        &store,
        tenant,
        "DELETE FROM similar_keys WHERE tenant_id = $1 AND memory_id = $2",
        served,
    )
    .await;
    assert_eq!(
        health().await,
        (true, 1, 0),
        "a served version without keys"
    );
    as_owner(
        &store,
        tenant,
        "UPDATE similar_keys SET state = 0 WHERE tenant_id = $1 AND memory_id = $2",
        Uuid::from_u128(0xB11),
    )
    .await;
    assert_eq!(health().await, (true, 1, 1), "keys in another state");
    let mut owner = owner_conn(&store).await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    sqlx::query("SELECT oml_similar_keys_refill($1)")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .expect("refill");
    tx.commit().await.expect("commit");
    assert_eq!(health().await, (true, 0, 0), "a refill repairs both");
    drop_dbs(&created).await;
}

/// The `bm25_postings` statement reads the postings by their token index as
/// the serving role, under row security: integer equality is leakproof, so
/// the policy leaves the index usable (ADR-0052 Context, d1).
#[tokio::test]
async fn the_bm25_postings_statement_reads_the_postings_by_their_index_under_row_security() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(574);
    rig.seed_chain(&store, 0).await;
    let wa = Uuid::from_u128(0xE2);
    lexeme_rows(&mut rig, &store, wa).await;
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    // Sequential scans off, so a plan that can use the index does: on five
    // rows the planner would otherwise scan, and the law is that it may.
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *tx)
        .await
        .expect("seqscan off");
    let token = oml_pg::token_id("deploy");
    let plan: Vec<String> = sqlx::query_scalar(&format!(
        "EXPLAIN (COSTS OFF) {}",
        oml_pg::bm25_postings_sql()
    ))
    .bind(rig.tenant_id)
    .bind(vec![wa])
    .bind(vec![1_i16, 2])
    .bind(Vec::<Uuid>::new())
    .bind(vec!["deploy".to_owned()])
    .bind(vec![token])
    .bind(vec![2.0_f64])
    .bind(5.0_f64)
    .bind(10.0_f64)
    .bind(vec![token])
    .bind(Some(10_i64))
    .bind(false)
    .bind(1_i64)
    .bind(10_i64)
    .bind(i64::MIN)
    .fetch_all(&mut *tx)
    .await
    .expect("plan");
    tx.commit().await.expect("commit");
    let plan = plan.join("\n");
    assert!(
        plan.lines()
            .any(|l| l.contains("Index Cond") && l.contains("token_id")),
        "the token is an index condition under the policy: {plan}"
    );
    // The served index carries what a score needs, ordered by workarea after
    // the token, so a recall reads its readable workareas' postings alone,
    // without the heap (ADR-0052 d1, open question 1).
    assert!(
        plan.lines()
            .any(|l| l.contains("Index Only Scan") && l.contains("kind_token_id_workarea_id")),
        "an index-only scan of the partition's search_postings_served: {plan}"
    );
    assert!(
        plan.lines().any(|l| l.contains("Index Cond")
            && l.contains("token_id")
            && l.contains("workarea_id")),
        "the readable workareas are an index condition beside the token: {plan}"
    );
    // The parent keeps no statistics of its own, which an ANALYZE of it
    // would gather across every tenant; the tenant's partition keeps its own
    // (FR-45; review F5).
    let mut owner = owner_conn(&store).await;
    let targets: Vec<(String, Option<i16>)> = sqlx::query_as(
        "SELECT c.relname::text, a.attstattarget::int2 FROM pg_attribute a \
           JOIN pg_class c ON c.oid = a.attrelid \
          WHERE a.attname = 'token_id' \
            AND (c.relname = 'search_postings' OR c.relname = $1) ORDER BY 1",
    )
    .bind(format!("search_postings_t_{}", rig.tenant_id.simple()))
    .fetch_all(&mut owner)
    .await
    .expect("statistics targets");
    assert_eq!(targets.len(), 2, "{targets:?}");
    assert_eq!(
        targets[0].1,
        Some(0),
        "the parent gathers none: {targets:?}"
    );
    assert_ne!(
        targets[1].1,
        Some(0),
        "the partition keeps its own: {targets:?}"
    );
    drop_dbs(&created).await;
}

/// A database shaped like v0.5.5's, migrated through 0020 and holding
/// promoted, held, replaced and rejected memories, migrates through 0021:
/// its postings and counts are those the trigger writes for the same rows
/// written after 0021, nothing is journalled, and `bm25_postings` ranks as
/// it does over them (ADR-0052 d8, d12).
#[tokio::test]
async fn a_v0_5_5_database_migrated_through_0021_ranks_as_one_written_after_it() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 20).await;
    let upgraded = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    let mut rig = ChainRig::new(575);
    rig.seed_chain(&upgraded, 0).await;
    let wa = Uuid::from_u128(0xE2);
    lexeme_rows(&mut rig, &upgraded, wa).await;
    lifecycle_rows(&mut rig, &upgraded).await;
    let journal_before: i64 = sqlx::query_scalar("SELECT count(*) FROM journal")
        .fetch_one(&mut conn)
        .await
        .expect("count");
    upgraded
        .migrate(&owner_url(&db))
        .await
        .expect("0021 applies over a v0.5.5 database");
    let upgraded = upgraded.with_search_probe().await.expect("probe");
    let journal_after: i64 = sqlx::query_scalar("SELECT count(*) FROM journal")
        .fetch_one(&mut conn)
        .await
        .expect("count");
    assert_eq!(
        journal_after, journal_before,
        "the upgrade journals nothing"
    );
    let applied: i64 =
        sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success")
            .fetch_one(&mut conn)
            .await
            .expect("versions");
    // The ledger's highest version is read (ADR-0030 d14).
    assert_eq!(applied, 37, "every migration through 0037_open_signup");
    drop(conn);

    let fresh = fresh_search_store(&created).await;
    let mut twin = ChainRig::new(575);
    twin.seed_chain(&fresh, 0).await;
    lexeme_rows(&mut twin, &fresh, wa).await;
    lifecycle_rows(&mut twin, &fresh).await;
    let filled = postings_of(&upgraded, rig.tenant_id).await;
    assert!(
        !filled.0.is_empty(),
        "0021 fills the postings of existing rows"
    );
    assert_eq!(
        filled,
        postings_of(&fresh, twin.tenant_id).await,
        "the filled postings and counts are the ones the writes keep"
    );
    let list = |store: &PgStore, tenant: Uuid, query: &'static str| {
        let store = store.clone();
        async move {
            store
                .retrieve(
                    tenant,
                    RetrieverKind::Bm25Postings,
                    &CandidateScope {
                        anchor: oml_app::types::AnchorFilter::default(),
                        promoted_nodes: vec![wa, Uuid::from_u128(0xE3)],
                        kinds: oml_app::types::HIT_KINDS.to_vec(),
                        own_held: Some(OwnHeldScope {
                            any_author: false,
                            author_key_ids: vec![Uuid::from_u128(0xE5)],
                            nodes: vec![wa],
                        }),
                        expired_at: None,
                    },
                    RetrievalQuery {
                        query,
                        match_mode: MatchMode::Any,
                        k: 50,
                        sparse: &[],
                        window: None,
                        within: &[],
                    },
                )
                .await
                .expect("bm25_postings")
        }
    };
    for query in [
        "price",
        "the prices of aluminium",
        "invoice filters",
        "deploy canary",
        "billing friday",
        "AM-7741",
        "zebra",
    ] {
        assert_eq!(
            list(&upgraded, rig.tenant_id, query).await,
            list(&fresh, twin.tenant_id, query).await,
            "{query}: the upgraded database ranks as a fresh one"
        );
    }
    drop_dbs(&created).await;
}

/// The postings' health doctor reads (ADR-0052 d12): clean where the
/// trigger kept everything; then each fault named by its own count — a
/// served version whose postings are gone (behind, and the fill
/// incomplete), a version of another state whose postings are gone (the
/// fill incomplete alone), a workarea's count drifting from its versions,
/// a token's count drifting from its postings; and clean again after the
/// owner's refill.
#[tokio::test]
async fn the_postings_health_is_clean_where_the_trigger_kept_it_and_names_each_fault_until_a_refill()
 {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(576);
    rig.seed_chain(&store, 0).await;
    let (_, _, _, _, served) = lifecycle_rows(&mut rig, &store).await;
    let tenant = rig.tenant_id;
    let health = |store: &PgStore| {
        let store = store.clone();
        async move {
            store
                .postings_health(tenant)
                .await
                .expect("health")
                .expect("PostgreSQL keeps postings")
        }
    };
    let clean = health(&store).await;
    assert!(clean.partition, "the tenant's postings partition exists");
    assert_eq!(
        (
            clean.unfilled,
            clean.served_behind,
            clean.workarea_drift,
            clean.term_drift
        ),
        (0, 0, 0, 0),
        "{clean:?}"
    );
    assert!(clean.terms_checked > 0, "{clean:?}");

    as_owner(
        &store,
        tenant,
        "DELETE FROM search_postings WHERE tenant_id = $1 AND memory_id = $2",
        served,
    )
    .await;
    as_owner(
        &store,
        tenant,
        "DELETE FROM search_postings WHERE tenant_id = $1 AND memory_id = $2",
        Uuid::from_u128(0xB07),
    )
    .await;
    as_owner(
        &store,
        tenant,
        "UPDATE search_workarea_rows SET rows = rows + 1 \
          WHERE tenant_id = $1 AND workarea_id = $2",
        Uuid::from_u128(0xE2),
    )
    .await;
    as_owner(
        &store,
        tenant,
        "UPDATE search_term_rows SET rows = rows + 1 \
          WHERE tenant_id = $1 AND workarea_id = $2",
        Uuid::from_u128(0xE2),
    )
    .await;
    let faulty = health(&store).await;
    assert_eq!(faulty.served_behind, 1, "the served version: {faulty:?}");
    assert_eq!(
        faulty.unfilled, 2,
        "the served and the held version: {faulty:?}"
    );
    assert_eq!(
        faulty.workarea_drift, 2,
        "0xE2's two count rows, memory and derived_fact: {faulty:?}"
    );
    assert!(faulty.term_drift > 0, "{faulty:?}");

    as_owner(
        &store,
        tenant,
        "SELECT oml_search_postings_refill($1) WHERE $2::uuid IS NOT NULL",
        Uuid::nil(),
    )
    .await;
    let refilled = health(&store).await;
    assert_eq!(refilled, clean, "the refill restores what the trigger kept");
    assert_eq!(
        postings_of(&store, tenant).await.1,
        counts_from_postings(&store, tenant).await.0,
        "the refilled counts are the served postings'"
    );
    drop_dbs(&created).await;
}

/// The served state alone of the postings, the counts and the refill are
/// the trigger's and the owner's: the serving role may not run the refill
/// (ADR-0052 d12; review F3).
#[tokio::test]
async fn the_serving_role_may_not_run_the_postings_refill() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(577);
    rig.seed_chain(&store, 0).await;
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let refused = sqlx::query("SELECT oml_search_postings_refill($1)")
        .bind(rig.tenant_id)
        .execute(&mut *tx)
        .await
        .expect_err("the serving role is refused");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("42501"),
        "{refused}"
    );
    drop(tx);
    drop_dbs(&created).await;
}

/// A refill holds its own tenant's writes, by that tenant's chain lock,
/// and no other tenant's: another tenant's write commits while it runs
/// (ADR-0052 d12; review F3).
#[tokio::test]
async fn a_refill_holds_its_own_tenants_writes_and_no_other_tenants() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut mine = ChainRig::new(578);
    mine.seed_chain(&store, 0).await;
    lifecycle_rows(&mut mine, &store).await;
    let mut theirs = ChainRig::new(579);
    theirs.seed_chain(&store, 0).await;
    let mut owner = owner_conn(&store).await;
    let mut refill = owner.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(mine.tenant_id.to_string())
        .execute(&mut *refill)
        .await
        .expect("tenant");
    sqlx::query("SELECT oml_search_postings_refill($1)")
        .bind(mine.tenant_id)
        .execute(&mut *refill)
        .await
        .expect("refill");
    let other = held_memory(
        theirs.tenant_id,
        Uuid::from_u128(0xE2),
        Uuid::from_u128(0xC01),
        "rita",
        Uuid::from_u128(0xE5),
        "another tenant writes during the refill",
    );
    let written = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        oml_port_contracts::hold(&mut theirs, &store, other),
    )
    .await;
    assert!(
        matches!(written, Ok(Ok(_))),
        "another tenant's write is not held by the refill: {written:?}"
    );
    let own = held_memory(
        mine.tenant_id,
        Uuid::from_u128(0xE2),
        Uuid::from_u128(0xC02),
        "rita",
        Uuid::from_u128(0xE5),
        "this tenant waits for its refill",
    );
    let waited = tokio::time::timeout(
        std::time::Duration::from_millis(1_500),
        oml_port_contracts::hold(&mut mine, &store, own),
    )
    .await;
    assert!(
        waited.is_err(),
        "the refilled tenant's write waits: {waited:?}"
    );
    refill.commit().await.expect("commit");
    drop(owner);
    drop_dbs(&created).await;
}

/// A tenant whose postings partition is missing, as a lead before 0021
/// leaves a tenant it creates after another lead migrated, has every memory
/// write refused; the next start's migration creates the partition and
/// fills it, and its writes resume (ADR-0052 d1; review F4).
#[tokio::test]
async fn a_tenant_without_a_postings_partition_gets_one_at_the_next_start_and_its_writes_resume() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(580);
    rig.seed_chain(&store, 0).await;
    lifecycle_rows(&mut rig, &store).await;
    let tenant = rig.tenant_id;
    let partition = format!("search_postings_t_{}", tenant.simple());
    let mut owner = owner_conn(&store).await;
    owner
        .execute(format!("ALTER TABLE search_postings DETACH PARTITION {partition}").as_str())
        .await
        .expect("detach");
    owner
        .execute(format!("DROP TABLE {partition}").as_str())
        .await
        .expect("drop");
    drop(owner);
    let row = |id: u128| {
        held_memory(
            tenant,
            Uuid::from_u128(0xE2),
            Uuid::from_u128(id),
            "rita",
            Uuid::from_u128(0xE5),
            "written while the partition is missing",
        )
    };
    assert!(
        oml_port_contracts::hold(&mut rig, &store, row(0xC10))
            .await
            .is_err(),
        "a write needs the tenant's postings partition"
    );
    let health = store
        .postings_health(tenant)
        .await
        .expect("health")
        .expect("pg");
    assert!(!health.partition, "{health:?}");
    let db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(store.pool())
        .await
        .expect("db");
    store
        .migrate(&owner_url(&db))
        .await
        .expect("the next start migrates");
    let health = store
        .postings_health(tenant)
        .await
        .expect("health")
        .expect("pg");
    assert!(health.partition, "the start creates it: {health:?}");
    assert_eq!(
        (
            health.unfilled,
            health.served_behind,
            health.workarea_drift,
            health.term_drift
        ),
        (0, 0, 0, 0),
        "and fills it: {health:?}"
    );
    oml_port_contracts::hold(&mut rig, &store, row(0xC11))
        .await
        .expect("writes resume");
    drop_dbs(&created).await;
}

/// The health check's word-count sample moves: drift in a count past the
/// first 200 a fixed sample would read is found within a few reads
/// (ADR-0052 d12; review F7).
#[tokio::test]
async fn the_postings_health_samples_word_counts_across_the_whole_range() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(581);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    let words: Vec<String> = (0..600).map(|i| format!("word{i:04}")).collect();
    for (n, chunk) in words.chunks(100).enumerate() {
        let m = held_memory(
            tenant,
            Uuid::from_u128(0xE2),
            Uuid::from_u128(0xD00 + n as u128),
            "rita",
            Uuid::from_u128(0xE5),
            &chunk.join(" "),
        );
        oml_port_contracts::hold_and_promote(&mut rig, &store, m).await;
    }
    // The count of the largest token: past any fixed first 200.
    as_owner(
        &store,
        tenant,
        "UPDATE search_term_rows SET rows = rows + 1 WHERE tenant_id = $1 AND token_id = \
          (SELECT max(token_id) FROM search_term_rows WHERE tenant_id = $1) AND $2::uuid IS NOT NULL",
        Uuid::nil(),
    )
    .await;
    let mut found = false;
    for _ in 0..20 {
        let h = store
            .postings_health(tenant)
            .await
            .expect("health")
            .expect("pg");
        found |= h.term_drift > 0;
    }
    assert!(found, "twenty reads sample the drifted count at least once");
    drop_dbs(&created).await;
}

/// The counts' writes lock rows in token order alone, so concurrent writes
/// that skip the chain lock, as a future encoder or repair may, neither
/// deadlock nor race a count below zero (ADR-0052 d5; review F9).
#[tokio::test]
async fn concurrent_count_updates_without_the_chain_lock_never_deadlock() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(582);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    let db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(store.pool())
        .await
        .expect("db");
    // Half the words already counted once, so a take-back deletes some rows
    // and updates others in the same call.
    let half = "alpha bravo charlie delta echo foxtrot golf hotel india juliet";
    let all = format!("{half} kilo lima mike november oscar papa quebec romeo sierra tango");
    let seed =
        format!("SELECT oml_search_count($1, $2, 1::int2, to_tsvector('simple', '{half}'), 10, 1)");
    let mut tasks = Vec::new();
    {
        let mut c = PgConnection::connect(&owner_url(&db)).await.expect("owner");
        let mut tx = c.begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("tenant");
        sqlx::query(&seed)
            .bind(tenant)
            .bind(Uuid::from_u128(0xE2))
            .execute(&mut *tx)
            .await
            .expect("seed");
        tx.commit().await.expect("commit");
    }
    for worker in 0..6 {
        let url = owner_url(&db);
        let all = all.clone();
        tasks.push(tokio::spawn(async move {
            let mut c = PgConnection::connect(&url).await.expect("owner");
            for i in 0..60 {
                let delta = if (i + worker) % 2 == 0 { 1 } else { -1 };
                let mut tx = c.begin().await.expect("begin");
                sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
                    .bind(tenant.to_string())
                    .execute(&mut *tx)
                    .await
                    .expect("tenant");
                let sql = format!(
                    "SELECT oml_search_count($1, $2, 1::int2, to_tsvector('simple', '{all}'), 20, {delta})"
                );
                if let Err(e) = sqlx::query(&sql)
                    .bind(tenant)
                    .bind(Uuid::from_u128(0xE2))
                    .execute(&mut *tx)
                    .await
                {
                    return Err(e.to_string());
                }
                tx.commit().await.expect("commit");
            }
            Ok(())
        }));
    }
    for t in tasks {
        let done = t.await.expect("task");
        assert!(
            done.is_ok(),
            "no deadlock and no count below zero: {done:?}"
        );
    }
    drop_dbs(&created).await;
}

/// Rows of `search_sparse_queue` for `tenant`, and kind 2 postings per
/// version, as the owner sees them (ADR-0052 d15).
async fn sparse_state_of(store: &PgStore, tenant: Uuid) -> (i64, BTreeMap<Uuid, i64>) {
    let mut owner = owner_conn(store).await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM search_sparse_queue WHERE tenant_id = $1")
            .bind(tenant)
            .fetch_one(&mut *tx)
            .await
            .expect("queue");
    let kind2: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT memory_id, count(*) FROM search_postings WHERE tenant_id = $1 AND kind = 2 \
          GROUP BY memory_id",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("kind 2");
    tx.commit().await.expect("commit");
    (queued, kind2.into_iter().collect())
}

/// With no sparse pipeline ever served, nothing is queued for the encoder:
/// not by a write, a promotion, a supersession, nor by the owner's refill,
/// so a deployment on the default pipelines keeps no queue (ADR-0052 d15,
/// amended on review).
#[tokio::test]
async fn with_sparse_never_served_writes_and_refills_enqueue_nothing() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(581);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    lifecycle_rows(&mut rig, &store).await;
    assert_eq!(
        sparse_state_of(&store, tenant).await.0,
        0,
        "writes enqueue nothing"
    );
    as_owner(
        &store,
        tenant,
        "SELECT oml_search_postings_refill($1) WHERE $2::uuid IS NOT NULL",
        Uuid::nil(),
    )
    .await;
    assert_eq!(
        sparse_state_of(&store, tenant).await.0,
        0,
        "the refill enqueues nothing"
    );
    drop_dbs(&created).await;
}

/// The owner's refill rebuilds kind 1 and keeps the encoder's kind 2
/// postings, with their versions' states (ADR-0052 d15, d16).
#[tokio::test]
async fn a_refill_keeps_the_encoders_postings() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(582);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    lifecycle_rows(&mut rig, &store).await;
    oml_port_contracts::encode_sparse_backlog(&store).await;
    let (_, before) = sparse_state_of(&store, tenant).await;
    assert!(!before.is_empty(), "the encoder wrote kind 2");
    let states_before = postings_of(&store, tenant).await.0;
    as_owner(
        &store,
        tenant,
        "SELECT oml_search_postings_refill($1) WHERE $2::uuid IS NOT NULL",
        Uuid::nil(),
    )
    .await;
    let (queued, after) = sparse_state_of(&store, tenant).await;
    assert_eq!(after, before, "every version keeps its kind 2 postings");
    assert_eq!(queued, 0, "nothing encoded is queued again");
    assert_eq!(
        postings_of(&store, tenant).await.0,
        states_before,
        "one state per version"
    );
    drop_dbs(&created).await;
}

/// Enqueues `id` for the encoder as the owner, as the trigger does where a
/// sparse pipeline is served.
async fn enqueue_as_owner(store: &PgStore, tenant: Uuid, id: Uuid) {
    as_owner(
        store,
        tenant,
        "INSERT INTO search_sparse_queue (tenant_id, memory_id) VALUES ($1, $2) \
         ON CONFLICT DO NOTHING",
        id,
    )
    .await;
}

/// A queued version that is erased or deleted leaves the queue with its
/// postings, so the encoder never writes for it (ADR-0052 d7, d15).
#[tokio::test]
async fn a_removed_queued_version_leaves_the_queue() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(583);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    let (replaced, _, _, _, served) = lifecycle_rows(&mut rig, &store).await;
    for id in [served, replaced, Uuid::from_u128(0xB07)] {
        enqueue_as_owner(&store, tenant, id).await;
    }
    assert_eq!(sparse_state_of(&store, tenant).await.0, 3);
    as_owner(
        &store,
        tenant,
        "UPDATE memories SET status = 'erased', is_tip = false WHERE tenant_id = $1 AND id = $2",
        served,
    )
    .await;
    as_owner(
        &store,
        tenant,
        "UPDATE memories SET status = 'erased' WHERE tenant_id = $1 AND id = $2",
        replaced,
    )
    .await;
    as_owner(
        &store,
        tenant,
        "DELETE FROM review_queue WHERE tenant_id = $1 AND memory_id = $2",
        Uuid::from_u128(0xB07),
    )
    .await;
    as_owner(
        &store,
        tenant,
        "DELETE FROM memories WHERE tenant_id = $1 AND id = $2",
        Uuid::from_u128(0xB07),
    )
    .await;
    assert_eq!(
        sparse_state_of(&store, tenant).await.0,
        0,
        "erased and deleted versions leave the queue"
    );
    drop_dbs(&created).await;
}

/// The drain's write and a version's removal never deadlock: the drain
/// takes its locks in the trigger's order, the version first, then the
/// queue entry. Interleaved step by step: the drain's first step, then an
/// erase-shaped update whose trigger takes the queue entry, then the
/// drain's second step; both commit (ADR-0052 d15, reviewed).
#[tokio::test]
async fn the_drains_write_and_an_erase_never_deadlock() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(584);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    let (_, _, _, _, served) = lifecycle_rows(&mut rig, &store).await;
    enqueue_as_owner(&store, tenant, served).await;
    let mut drain = store.pool().begin().await.expect("drain");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *drain)
        .await
        .expect("tenant");
    let enqueued: i64 = sqlx::query_scalar(
        "SELECT (extract(epoch FROM enqueued_at) * 1000000)::int8 FROM search_sparse_queue \
          WHERE tenant_id = $1 AND memory_id = $2",
    )
    .bind(tenant)
    .bind(served)
    .fetch_one(&mut *drain)
    .await
    .expect("entry");
    let step = |s: oml_pg::SparseWriteStep| match s {
        oml_pg::SparseWriteStep::TakeEntry => oml_pg::SPARSE_TAKE_ENTRY,
        oml_pg::SparseWriteStep::HoldVersion => oml_pg::SPARSE_HOLD_VERSION,
    };
    let [first, second] = oml_pg::SPARSE_WRITE_ORDER;
    let first_sql = step(first);
    let q = sqlx::query(first_sql).bind(tenant).bind(served);
    let q = if first == oml_pg::SparseWriteStep::TakeEntry {
        q.bind(enqueued)
    } else {
        q
    };
    q.execute(&mut *drain)
        .await
        .expect("the drain's first step");
    let db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(store.pool())
        .await
        .expect("db");
    let erase = tokio::spawn(async move {
        let mut owner = PgConnection::connect(&owner_url(&db)).await.expect("owner");
        let mut tx = owner.begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .expect("tenant");
        sqlx::query(
            "UPDATE memories SET status = 'erased', is_tip = false \
              WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant)
        .bind(served)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let q = sqlx::query(step(second)).bind(tenant).bind(served);
    let q = if second == oml_pg::SparseWriteStep::TakeEntry {
        q.bind(enqueued)
    } else {
        q
    };
    let drained = q.execute(&mut *drain).await;
    let drained = match drained {
        Ok(_) => drain.commit().await.map(|_| ()),
        Err(e) => Err(e),
    };
    let erased = erase.await.expect("join");
    assert!(drained.is_ok(), "the drain's write: {drained:?}");
    assert!(erased.is_ok(), "the erase: {erased:?}");
    assert_eq!(sparse_state_of(&store, tenant).await.0, 0);
    drop_dbs(&created).await;
}

/// Turned off, as a lead serving no sparse pipeline turns it at start, the
/// queue empties and stays empty through writes and refills; turned on
/// again by the encoder's pass, a bounded backfill resumes step by step
/// until every version written meanwhile is queued, and the pass encodes
/// them (ADR-0052 d15, HIGH-1 on review).
#[tokio::test]
async fn a_queue_turned_off_stays_empty_and_one_turned_on_backfills_in_resumable_steps() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(585);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    let wa = Uuid::from_u128(0xE2);
    let key = Uuid::from_u128(0xE5);
    oml_port_contracts::encode_sparse_backlog(&store).await;
    oml_port_contracts::hold(
        &mut rig,
        &store,
        oml_port_contracts::held_memory(tenant, wa, Uuid::from_u128(0xC01), "rita", key, "kowari"),
    )
    .await
    .expect("hold");
    assert_eq!(
        sparse_state_of(&store, tenant).await.0,
        1,
        "queued while on"
    );
    assert_eq!(store.sparse_off().await.expect("off"), 1);
    assert_eq!(
        sparse_state_of(&store, tenant).await.0,
        0,
        "off empties the queue"
    );
    for i in 0..5_u128 {
        oml_port_contracts::hold(
            &mut rig,
            &store,
            oml_port_contracts::held_memory(
                tenant,
                wa,
                Uuid::from_u128(0xC10 + i),
                "rita",
                key,
                "kowari burrow",
            ),
        )
        .await
        .expect("hold");
    }
    as_owner(
        &store,
        tenant,
        "SELECT oml_search_postings_refill($1) WHERE $2::uuid IS NOT NULL",
        Uuid::nil(),
    )
    .await;
    assert_eq!(
        sparse_state_of(&store, tenant).await.0,
        0,
        "nothing queued while off"
    );
    let health = store
        .postings_health(tenant)
        .await
        .expect("health")
        .expect("postings");
    assert!(!health.sparse_enabled, "{health:?}");
    // On again: two versions a step, resumed where the last stopped.
    let mut steps = 0;
    let mut queued = 0;
    loop {
        let (n, done) = store.sparse_backfill(tenant, 2).await.expect("step");
        queued += n;
        steps += 1;
        if done {
            break;
        }
        assert!(steps < 20, "the backfill ends");
    }
    assert!(steps >= 3, "bounded: {steps} steps");
    assert_eq!(queued, 6, "every version without kind 2 is queued once");
    assert_eq!(oml_port_contracts::encode_sparse_backlog(&store).await, 6);
    assert_eq!(sparse_state_of(&store, tenant).await.1.len(), 6);
    drop_dbs(&created).await;
}

/// Turning a tenant's sparse state on waits for every write in flight: a
/// write whose trigger read the state off and commits after the backfill's
/// first read would be queued by neither, and the sparse lists would never
/// hold it (ADR-0052 d15). The step holds until that write commits, so its
/// read sees it.
#[tokio::test]
async fn turning_the_sparse_state_on_waits_for_writes_in_flight_so_the_backfill_sees_them() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(587);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    // A write in flight: its transaction holds the lock every insert or
    // update of a memory takes, and has not committed.
    let mut writer = owner_conn(&store).await;
    let mut tx = writer.begin().await.expect("begin");
    sqlx::query("LOCK TABLE memories IN ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .expect("a write's lock");
    let step = store.sparse_backfill(tenant, 10);
    tokio::pin!(step);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(500), &mut step)
            .await
            .is_err(),
        "the first step waits for the write in flight"
    );
    tx.commit().await.expect("the write commits");
    let (_, done) = tokio::time::timeout(std::time::Duration::from_secs(10), step)
        .await
        .expect("the step proceeds once the write committed")
        .expect("step");
    assert!(done, "one step reads every version");
    // On already: a later step takes no lock, so writes never wait for it.
    let mut writer = owner_conn(&store).await;
    let mut tx = writer.begin().await.expect("begin");
    sqlx::query("LOCK TABLE memories IN ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .expect("a write's lock");
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        store.sparse_backfill(tenant, 10),
    )
    .await
    .expect("a step on a state already on does not wait")
    .expect("step");
    tx.commit().await.expect("commit");
    drop_dbs(&created).await;
}

/// A version enqueued again after the drain read it is not written with
/// the claim read before: the guarded take finds a newer entry, writes
/// nothing, and the next read carries the new claim (ADR-0052 d15; L3 on
/// review).
#[tokio::test]
async fn a_version_enqueued_again_after_its_read_is_not_written_with_the_older_claim() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(586);
    rig.seed_chain(&store, 0).await;
    let tenant = rig.tenant_id;
    oml_port_contracts::encode_sparse_backlog(&store).await;
    let id = Uuid::from_u128(0xC21);
    oml_port_contracts::hold(
        &mut rig,
        &store,
        oml_port_contracts::held_memory(
            tenant,
            Uuid::from_u128(0xE2),
            id,
            "rita",
            Uuid::from_u128(0xE5),
            "mulgara notes",
        ),
    )
    .await
    .expect("hold");
    let read = store.sparse_pending(tenant, 10).await.expect("pending");
    assert_eq!(read.len(), 1);
    as_owner(
        &store,
        tenant,
        "UPDATE memories SET claim = claim || ' revised' WHERE tenant_id = $1 AND id = $2",
        id,
    )
    .await;
    assert!(
        !store
            .sparse_write(
                tenant,
                &read[0],
                &oml_app::doubles::FakeSparseEncoder::document(&read[0].claim)
            )
            .await
            .expect("write"),
        "the older read writes nothing"
    );
    assert!(sparse_state_of(&store, tenant).await.1.is_empty());
    assert_eq!(oml_port_contracts::encode_sparse_backlog(&store).await, 1);
    let list = store.sparse_pending(tenant, 10).await.expect("pending");
    assert!(list.is_empty(), "written with the new claim and dequeued");
    drop_dbs(&created).await;
}

/// Migration 0026 is shaped to run on a live database, as 0025 is: it waits
/// for a lock 5 s at most, adds its check NOT VALID and validates it apart,
/// and builds each partition's index on its own and attaches it to an
/// index created ON ONLY the parent; over a database with a tenant
/// partition the check ends validated and every index valid and attached
/// (proposed ADR-0059 d11).
#[tokio::test]
async fn migration_0026_is_live_safe_and_leaves_every_partitions_indexes_attached() {
    let sql = MIGRATOR
        .iter()
        .find(|m| m.version == 26)
        .expect("migration 0026")
        .sql
        .to_string();
    for shape in [
        "SET LOCAL lock_timeout = '5s'",
        ") NOT VALID;",
        "VALIDATE CONSTRAINT memories_valid_time_ends",
        "ON ONLY memories (tenant_id, valid_from_us)",
        "ON ONLY memories (tenant_id, valid_until_us)",
        "ATTACH PARTITION",
    ] {
        assert!(sql.contains(shape), "0026 lacks {shape:?}");
    }
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let (db, mut conn) = schema_through(&created, 25).await;
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    let mut rig = ChainRig::new(591);
    rig.seed_chain(&store, 0).await;
    store
        .migrate(&owner_url(&db))
        .await
        .expect("0026 applies over a database with a tenant partition");
    let partitions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_inherits WHERE inhparent = 'memories'::regclass",
    )
    .fetch_one(&mut conn)
    .await
    .expect("partitions");
    assert!(partitions >= 1, "the seeded tenant has a partition");
    for index in ["memories_valid_from", "memories_valid_until"] {
        let (attached, valid): (i64, bool) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM pg_inherits WHERE inhparent = $1::regclass), \
                    (SELECT indisvalid FROM pg_index WHERE indexrelid = $1::regclass)",
        )
        .bind(index)
        .fetch_one(&mut conn)
        .await
        .expect("index");
        assert_eq!(attached, partitions, "{index}: one attached per partition");
        assert!(valid, "{index} is valid once every partition's is attached");
    }
    let validated: bool = sqlx::query_scalar(
        "SELECT convalidated FROM pg_constraint WHERE conname = 'memories_valid_time_ends' \
           AND conrelid = 'memories'::regclass",
    )
    .fetch_one(&mut conn)
    .await
    .expect("constraint");
    assert!(validated, "the check is validated");
    drop(conn);
    drop_dbs(&created).await;
}

/// The expiry cut of `bm25-postings` compares `valid_until_us` with a bound
/// `int8`, leakproof, so under row security the serving role reads the
/// ended versions by `memories_valid_until`'s index condition instead of
/// filtering every dated row of the tenant (proposed ADR-0059 d3).
#[tokio::test]
async fn the_expiry_cut_is_an_index_condition_for_the_serving_role_under_row_security() {
    if !splits().await {
        return;
    }
    let created = Mutex::new(Vec::new());
    let store = fresh_search_store(&created).await;
    let mut rig = ChainRig::new(592);
    rig.seed_chain(&store, 0).await;
    let wa = Uuid::from_u128(0xE2);
    lexeme_rows(&mut rig, &store, wa).await;
    let mut tx = store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(rig.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *tx)
        .await
        .expect("seqscan off");
    let token = oml_pg::token_id("deploy");
    let plan: Vec<String> = sqlx::query_scalar(&format!("EXPLAIN {}", oml_pg::bm25_postings_sql()))
        .bind(rig.tenant_id)
        .bind(vec![wa])
        .bind(vec![1_i16, 2])
        .bind(Vec::<Uuid>::new())
        .bind(vec!["deploy".to_owned()])
        .bind(vec![token])
        .bind(vec![2.0_f64])
        .bind(5.0_f64)
        .bind(10.0_f64)
        .bind(vec![token])
        .bind(Some(10_i64))
        .bind(false)
        .bind(1_i64)
        .bind(10_i64)
        .bind(1_793_000_000_000_000_i64)
        .fetch_all(&mut *tx)
        .await
        .expect("plan");
    tx.commit().await.expect("commit");
    let plan = plan.join("\n");
    eprintln!("bm25-postings plan as the serving role:\n{plan}");
    assert!(
        plan.lines()
            .any(|l| l.contains("Index Cond") && l.contains("valid_until_us <=")),
        "the expiry cut is an index condition under the policy: {plan}"
    );
    drop_dbs(&created).await;
}
