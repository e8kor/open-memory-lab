//! Migration 0061 (ADR-0085 d16, amended 2026-11-18; #448): every version
//! written before it records the person its author key resolves to when the
//! migration runs, read as `same_user` reads it: a session key's person, a
//! user key's root, a linked tenant key's root; none for a key that acts for
//! no person.

use std::sync::atomic::Ordering;

use oml_pg::MIGRATOR;
use oml_test_pg::{DB_N, mint_database, owner_url, run_tag};
use sqlx::migrate::Migrate;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use uuid::Uuid;

/// The migration that records the author's person.
const AUTHOR_PERSON: i64 = 61;

/// An author key: its id, credential, user root, session person and linked
/// root, and the person 0061 records for its versions.
type AuthorKey = (
    u128,
    &'static str,
    Option<Uuid>,
    Option<&'static str>,
    Option<Uuid>,
    Option<&'static str>,
);

fn db_name(tag: &str) -> String {
    format!(
        "oml_author_person_{tag}_{}_{}_{}",
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

/// Four versions, one per author key: alice's user key, bob's session key,
/// carol's linked tenant key and an unlinked tenant key. After 0061 the
/// first three name their person and the fourth names none.
#[tokio::test]
async fn migration_0061_backfills_each_versions_author_person_from_its_key_today() {
    let db = db_name("backfill");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = schema_before(&db, AUTHOR_PERSON).await;
    let tenant = Uuid::from_u128(0xA061);
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
    let universe = Uuid::from_u128(0xB061);
    let (alice, carol) = (Uuid::from_u128(0xB062), Uuid::from_u128(0xB063));
    for (id, parent, name, user_root) in [
        (universe, None, "universe", false),
        (alice, Some(universe), "alice", true),
        (carol, Some(universe), "carol", true),
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
    // (key, credential, user root, session person, linked root, the person 0061 records)
    let keys: [AuthorKey; 4] = [
        (0xC061, "bearer", Some(alice), None, None, Some("alice")),
        (0xC062, "session", None, Some("bob"), None, Some("bob")),
        (0xC063, "bearer", None, None, Some(carol), Some("carol")),
        (0xC064, "bearer", None, None, None, None),
    ];
    for (n, (key, credential, user_root, session_for, linked, _)) in keys.iter().enumerate() {
        sqlx::query(
            "INSERT INTO api_keys (id, tenant_id, name, role, principal, issued_seq, credential, \
                                   used_by, user_root_id, session_for, linked_user_id) \
             VALUES ($1, $2, $3, 'writer', $3, $4, $5, 'person', $6, $7, $8)",
        )
        .bind(Uuid::from_u128(*key))
        .bind(tenant)
        .bind(format!("key-{n}"))
        .bind(i64::try_from(n).expect("small") + 1)
        .bind(*credential)
        .bind(*user_root)
        .bind(*session_for)
        .bind(*linked)
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("key {n}: {e}"));
        let event = Uuid::from_u128(0xE061 + *key);
        sqlx::query(
            "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, \
                                  payload, signed_bytes_hash, sig, signed_bytes) \
             VALUES ($1, $2, $3, 'submission.remember', '\\x00', 'acme/agent', '{}', \
                     '\\x00', '\\x00', '\\x00')",
        )
        .bind(tenant)
        .bind(i64::try_from(n).expect("small") + 100)
        .bind(event)
        .execute(&mut conn)
        .await
        .expect("journal row");
        sqlx::query(
            "INSERT INTO memories (id, tenant_id, workspace_id, claim, claim_digest, status, \
                                   created_event_id, claim_key, author_principal, author_key_id, \
                                   version, kind, body_bytes) \
             VALUES ($1, $2, $3, 'a held note', '\\x00', 'held', $4, $5, 'acme/agent', $6, \
                     1, 'memory', 11)",
        )
        .bind(Uuid::from_u128(0xD061 + *key))
        .bind(tenant)
        .bind(universe)
        .bind(event)
        .bind(format!("note-{n}"))
        .bind(Uuid::from_u128(*key))
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("memory {n}: {e}"));
    }
    let migration = MIGRATOR
        .iter()
        .find(|m| m.version == AUTHOR_PERSON)
        .expect("migration 0061");
    conn.apply(migration).await.expect("0061 applies");

    sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
        .bind(tenant)
        .execute(&mut conn)
        .await
        .expect("tenant");
    for (key, .., person) in keys {
        let recorded: Option<String> =
            sqlx::query_scalar("SELECT author_person::text FROM memories WHERE author_key_id = $1")
                .bind(Uuid::from_u128(key))
                .fetch_one(&mut conn)
                .await
                .expect("the version");
        assert_eq!(recorded.as_deref(), person, "key {key:#x}");
    }
}
