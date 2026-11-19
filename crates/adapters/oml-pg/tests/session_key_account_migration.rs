//! Migration 0065 (ADR-0071 d1, d3; ADR-0082 d10): a person's review
//! session key belongs to its person's account, so the text a review key's
//! edit proposal wrote before 0065, charged to no account then, is that
//! account's afterwards; the migration recomputes each account's
//! `text_bytes` from `oml_account_source`, so the drift report finds none.

use std::sync::atomic::Ordering;

use oml_pg::MIGRATOR;
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag};
use sqlx::migrate::Migrate;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use uuid::Uuid;

/// The migration that gives a review session key its person's account.
const KEY_ACCOUNT: i64 = 65;

/// An account's four stocks: user root, memories, text bytes, keys and
/// connections, as `account_meters` and `oml_account_source` hold them.
type Stocks = (Uuid, i64, i64, i64, i64);

fn db_name(tag: &str) -> String {
    format!(
        "oml_key_account_{tag}_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

/// An owner connection to `db` with the migrations before `version` applied.
async fn schema_before(db: &str, version: i64) -> PgConnection {
    let mut conn = PgConnection::connect(&owner_url(db))
        .await
        .expect("connect");
    sqlx::query("SELECT set_config('oml.serving_role', $1, false)")
        .bind(format!("{db}_serving"))
        .execute(&mut conn)
        .await
        .expect("set the serving role");
    conn.ensure_migrations_table()
        .await
        .expect("migrations table");
    for migration in MIGRATOR.iter().filter(|m| m.version < version) {
        conn.apply(migration).await.expect("the migrations apply");
    }
    conn
}

/// A partition of every partitioned table for `tenant`, as the store's
/// tenant DDL makes one (ADR-0034 d11), read off the catalog so a later
/// partitioned table needs no edit here.
async fn partition_for(conn: &mut PgConnection, tenant: Uuid) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_partitioned_table p JOIN pg_class c ON c.oid = p.partrelid \
          WHERE c.relnamespace = 'public'::regnamespace ORDER BY 1",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("partitioned tables");
    let policy = "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) \
                  WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)";
    for table in tables {
        let p = format!("{table}_t_{}", tenant.simple());
        for statement in [
            format!("CREATE TABLE {p} (LIKE {table} INCLUDING ALL)"),
            format!("ALTER TABLE {p} ENABLE ROW LEVEL SECURITY"),
            format!("ALTER TABLE {p} FORCE ROW LEVEL SECURITY"),
            format!("CREATE POLICY tenant_isolation ON {p} {policy}"),
            format!("ALTER TABLE {table} ATTACH PARTITION {p} FOR VALUES IN ('{tenant}')"),
        ] {
            conn.execute(statement.as_str())
                .await
                .unwrap_or_else(|e| panic!("{statement}: {e}"));
        }
    }
}

/// A version of `claim_key` in `workspace` by `key`, held, with its own
/// journal event.
#[allow(clippy::too_many_arguments)]
async fn held_version(
    conn: &mut PgConnection,
    tenant: Uuid,
    workspace: Uuid,
    n: u128,
    claim_key: &str,
    version: i32,
    key: Uuid,
    bytes: i32,
) {
    let event = Uuid::from_u128(0xE065 + n);
    sqlx::query(
        "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, \
                              payload, signed_bytes_hash, sig, signed_bytes) \
         VALUES ($1, $2, $3, 'submission.remember', '\\x00', 'acme/agent', '{}', \
                 '\\x00', '\\x00', '\\x00')",
    )
    .bind(tenant)
    .bind(i64::try_from(n).expect("small") + 100)
    .bind(event)
    .execute(&mut *conn)
    .await
    .expect("journal row");
    sqlx::query(
        "INSERT INTO memories (id, tenant_id, workspace_id, claim, claim_digest, status, \
                               created_event_id, claim_key, author_principal, author_key_id, \
                               version, kind, body_bytes) \
         VALUES ($1, $2, $3, 'a held note', '\\x00', 'held', $4, $5, 'acme/agent', $6, \
                 $7, 'memory', $8)",
    )
    .bind(Uuid::from_u128(0xD065 + n))
    .bind(tenant)
    .bind(workspace)
    .bind(event)
    .bind(claim_key)
    .bind(key)
    .bind(version)
    .bind(bytes)
    .execute(&mut *conn)
    .await
    .unwrap_or_else(|e| panic!("version {n}: {e}"));
}

async fn stocks(conn: &mut PgConnection, query: &str, tenant: Uuid) -> Vec<Stocks> {
    sqlx::query_as(query)
        .bind(tenant)
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("{query}: {e}"))
}

/// alice's agent key writes a claim in a shared workspace and her review
/// session key, before 0065, proposes its edit: the meters the fold kept
/// then charge alice the agent's 11 bytes alone. After 0065, applied by the
/// owner with no tenant named, alice's meters hold the proposal's 23 bytes
/// too and equal `oml_account_source`, so the drift report finds none.
#[tokio::test]
async fn a_review_key_proposal_written_before_0065_leaves_no_drift_after_it() {
    let db = db_name("drift");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = schema_before(&db, KEY_ACCOUNT).await;
    let tenant = Uuid::from_u128(0xA065);
    sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
        .bind(tenant)
        .execute(&mut conn)
        .await
        .expect("tenant");
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, 'acme')")
        .bind(tenant)
        .execute(&mut conn)
        .await
        .expect("tenant row");
    partition_for(&mut conn, tenant).await;
    let (universe, alice, team) = (
        Uuid::from_u128(0xB065),
        Uuid::from_u128(0xB066),
        Uuid::from_u128(0xB067),
    );
    for (id, parent, name, user_root) in [
        (universe, None, "universe", false),
        (alice, Some(universe), "alice", true),
        (team, Some(universe), "team", false),
    ] {
        sqlx::query(
            "INSERT INTO workspaces (id, tenant_id, parent_id, name, status, user_root) \
             VALUES ($1, $2, $3, $4, CASE WHEN $3::uuid IS NULL THEN 'protected' ELSE 'development' END, $5)",
        )
        .bind(id)
        .bind(tenant)
        .bind(parent)
        .bind(name)
        .bind(user_root)
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("workspace {name}: {e}"));
    }
    let (agent, review) = (Uuid::from_u128(0xC065), Uuid::from_u128(0xC066));
    for (n, (key, name, role, credential, user_root, session_for)) in [
        (agent, "alice-laptop", "writer", "bearer", Some(alice), None),
        (
            review,
            "alice-review",
            "reviewer",
            "session",
            None,
            Some("alice"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO api_keys (id, tenant_id, name, role, principal, issued_seq, credential, \
                                   used_by, user_root_id, session_for) \
             VALUES ($1, $2, $3, $4, $3, $5, $6, 'person', $7, $8)",
        )
        .bind(key)
        .bind(tenant)
        .bind(name)
        .bind(role)
        .bind(i64::try_from(n).expect("small") + 1)
        .bind(credential)
        .bind(user_root)
        .bind(session_for)
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("key {name}: {e}"));
    }
    held_version(&mut conn, tenant, team, 0, "note", 1, agent, 11).await;
    held_version(&mut conn, tenant, team, 1, "note", 2, review, 23).await;
    // The meters as the fold kept them before 0065: the review key's
    // proposal charged no account.
    conn.execute(
        format!(
            "INSERT INTO account_meters (tenant_id, user_root, memories, text_bytes, keys, connections) \
             SELECT '{tenant}', s.user_root, s.memories, s.text_bytes, s.keys, s.connections \
               FROM oml_account_source('{tenant}') s"
        )
        .as_str(),
    )
    .await
    .expect("the fold's meters");
    let meters = "SELECT user_root, memories, text_bytes, keys, connections \
                    FROM account_meters WHERE tenant_id = $1 ORDER BY user_root";
    let source = "SELECT user_root, memories, text_bytes, keys, connections \
                    FROM oml_account_source($1) ORDER BY user_root";
    assert_eq!(
        stocks(&mut conn, meters, tenant).await,
        [(alice, 1, 11, 1, 0)],
        "before 0065 the proposal is charged to no account"
    );

    conn.execute("SELECT set_config('oml.tenant_id', '', false)")
        .await
        .expect("no tenant");
    let migration = MIGRATOR
        .iter()
        .find(|m| m.version == KEY_ACCOUNT)
        .expect("migration 0065");
    conn.apply(migration).await.expect("0065 applies");

    sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
        .bind(tenant)
        .execute(&mut conn)
        .await
        .expect("tenant");
    let after = stocks(&mut conn, meters, tenant).await;
    assert_eq!(
        after,
        [(alice, 1, 34, 1, 0)],
        "alice's account holds the proposal"
    );
    assert_eq!(
        after,
        stocks(&mut conn, source, tenant).await,
        "the drift report's recount finds no drift"
    );
    drop(conn);
    drop_db(&db).await;
}
