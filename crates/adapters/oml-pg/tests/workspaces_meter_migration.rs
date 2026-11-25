//! Migration 0067 (ADR-0095 d4, d10): the seventh meter, `workspaces`. Its
//! fill counts each account's nodes below its root from the tree, a row
//! for an account that had none included, so the counters equal
//! `oml_account_source` and the drift report finds none; `user_quotas`
//! admits the quota by name.

use std::sync::atomic::Ordering;

use oml_pg::MIGRATOR;
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag};
use sqlx::migrate::Migrate;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use uuid::Uuid;

/// The migration that adds the `workspaces` meter.
const WORKSPACES_METER: i64 = 67;

/// An account's five stocks: user root, memories, text bytes, keys,
/// connections and workspaces.
type Stocks = (Uuid, i64, i64, i64, i64, i64);

fn db_name(tag: &str) -> String {
    format!(
        "oml_ws_meter_{tag}_{}_{}_{}",
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

async fn stocks(conn: &mut PgConnection, query: &str, tenant: Uuid) -> Vec<Stocks> {
    sqlx::query_as(query)
        .bind(tenant)
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("{query}: {e}"))
}

/// alice's root holds a child and a grandchild, and bob's one child but no
/// meter row; the shared `team` counts on no one. After 0067, applied by
/// the owner with no tenant named, alice's counter is 2 and bob has a row
/// holding 1, each equal to `oml_account_source`, and an override of the
/// `workspaces` quota is stored.
#[tokio::test]
async fn the_workspaces_meter_migration_fills_every_account_from_its_tree() {
    let db = db_name("fill");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = schema_before(&db, WORKSPACES_METER).await;
    let tenant = Uuid::from_u128(0xA067);
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
    let id = |n: u128| Uuid::from_u128(0xB067_0000 + n);
    let (universe, alice, bob, team) = (id(0), id(1), id(2), id(3));
    for (w, parent, name, user_root, namespace) in [
        (universe, None, "universe", false, None),
        (alice, Some(universe), "alice", true, None),
        (bob, Some(universe), "bob", true, None),
        (team, Some(universe), "team", false, None),
        (id(4), Some(alice), "notes", false, Some(alice)),
        (id(5), Some(id(4)), "q1", false, Some(alice)),
        (id(6), Some(bob), "notes", false, Some(bob)),
        (id(7), Some(team), "side", false, None),
    ] {
        sqlx::query(
            "INSERT INTO workspaces (id, tenant_id, parent_id, name, status, user_root, \
                                     namespace_root_id) \
             VALUES ($1, $2, $3, $4, CASE WHEN $3::uuid IS NULL THEN 'protected' ELSE 'development' END, \
                     $5, $6)",
        )
        .bind(w)
        .bind(tenant)
        .bind(parent)
        .bind(name)
        .bind(user_root)
        .bind(namespace)
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("workspace {name}: {e}"));
    }
    sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, name, role, principal, issued_seq, credential, \
                               used_by, user_root_id) \
         VALUES ($1, $2, 'alice-laptop', 'contributor', 'alice/laptop', 1, 'bearer', 'agent', $3)",
    )
    .bind(Uuid::from_u128(0xC067))
    .bind(tenant)
    .bind(alice)
    .execute(&mut conn)
    .await
    .expect("alice's key");
    // The meters as the fold kept them before 0067: alice's key on hers,
    // and bob, who never held a key, with no row.
    conn.execute(
        format!(
            "INSERT INTO account_meters (tenant_id, user_root, memories, text_bytes, keys, connections) \
             VALUES ('{tenant}', '{alice}', 0, 0, 1, 0)"
        )
        .as_str(),
    )
    .await
    .expect("the fold's meters");

    conn.execute("SELECT set_config('oml.tenant_id', '', false)")
        .await
        .expect("no tenant");
    let migration = MIGRATOR
        .iter()
        .find(|m| m.version == WORKSPACES_METER)
        .expect("migration 0067");
    conn.apply(migration).await.expect("0067 applies");

    sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
        .bind(tenant)
        .execute(&mut conn)
        .await
        .expect("tenant");
    let meters = "SELECT user_root, memories, text_bytes, keys, connections, workspaces \
                    FROM account_meters WHERE tenant_id = $1 ORDER BY user_root";
    let source = "SELECT user_root, memories, text_bytes, keys, connections, workspaces \
                    FROM oml_account_source($1) ORDER BY user_root";
    let after = stocks(&mut conn, meters, tenant).await;
    assert_eq!(after, [(alice, 0, 0, 1, 0, 2), (bob, 0, 0, 0, 0, 1)]);
    assert_eq!(
        after,
        stocks(&mut conn, source, tenant).await,
        "the drift report's recount finds no drift"
    );
    sqlx::query(
        "INSERT INTO user_quotas (tenant_id, user_root, quota, max_count) \
         VALUES ($1, $2, 'workspaces', 100)",
    )
    .bind(tenant)
    .bind(alice)
    .execute(&mut conn)
    .await
    .expect("the quota is admitted by name");
    drop(conn);
    drop_db(&db).await;
}
