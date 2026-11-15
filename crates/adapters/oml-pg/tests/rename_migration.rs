//! Migration 0047, the v0.8.0 rename (ADR-0091 d6): after it the catalog
//! names no `workarea`, no role, status or holder check names a former
//! value, and a database migrated across it holds its rows in the new
//! names. The rows are seeded at 0046 in the former names, as a v0.7
//! deployment holds them.

use std::sync::atomic::Ordering;

use oml_app::ports::JournalStore;
use oml_pg::{MIGRATOR, PgStore};
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use sqlx::migrate::Migrate;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor, Row};
use uuid::Uuid;

/// The migration that renames; every earlier one keeps the former names.
const RENAME: i64 = 47;

fn db_name(tag: &str) -> String {
    format!(
        "oml_rename_{tag}_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

/// An owner connection to `db` with the migrations up to `version` applied.
async fn schema_through(db: &str, version: i64) -> PgConnection {
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
    for migration in MIGRATOR.iter().filter(|m| m.version <= version) {
        conn.apply(migration).await.expect("the migrations apply");
    }
    conn
}

/// Each catalog name or text holding a former name, as `what: name`.
async fn former_names_in_catalog(conn: &mut PgConnection) -> Vec<String> {
    let queries = [
        // Relations, indexes, sequences and views, by name. An index's own
        // column names are copies the server never reads by name, so
        // pg_attribute is read for tables and views only.
        "SELECT 'relation: ' || relname FROM pg_class \
          WHERE relnamespace = 'public'::regnamespace AND relname ~ 'workarea'",
        "SELECT 'column: ' || c.relname || '.' || a.attname FROM pg_attribute a \
           JOIN pg_class c ON c.oid = a.attrelid \
          WHERE c.relnamespace = 'public'::regnamespace AND c.relkind IN ('r', 'p', 'v', 'm') \
            AND a.attnum > 0 AND NOT a.attisdropped \
            AND (a.attname ~ 'workarea' OR a.attname IN ('held_by', 'approver_held_by'))",
        "SELECT 'constraint: ' || conname FROM pg_constraint \
          WHERE connamespace = 'public'::regnamespace AND conname ~ 'workarea|committed'",
        "SELECT 'check: ' || conname || ' ' || pg_get_constraintdef(oid) FROM pg_constraint \
          WHERE connamespace = 'public'::regnamespace AND contype = 'c' \
            AND pg_get_constraintdef(oid) ~ '''(dreamer|committed|validated|human|workarea)'''",
        "SELECT 'policy: ' || polname FROM pg_policy WHERE polname ~ 'workarea'",
        "SELECT 'function: ' || proname FROM pg_proc \
          WHERE pronamespace = 'public'::regnamespace \
            AND (proname ~ 'workarea' OR prosrc ~ 'workarea')",
        "SELECT 'trigger: ' || tgname FROM pg_trigger WHERE tgname ~ 'workarea'",
        "SELECT 'comment: ' || description FROM pg_description d \
           JOIN pg_class c ON c.oid = d.objoid \
          WHERE c.relnamespace = 'public'::regnamespace AND description ~* 'workarea|dreamer'",
    ];
    let mut found = Vec::new();
    for q in queries {
        let rows: Vec<String> = sqlx::query_scalar(q)
            .fetch_all(&mut *conn)
            .await
            .expect("catalog read");
        found.extend(rows);
    }
    found
}

/// The catalog after every migration names no `workarea`, and no role,
/// status, holder or scope check names a former value, and a tenant's
/// partitions created after the cut derive no index name that holds it
/// (ADR-0091 d6).
#[tokio::test]
async fn the_schema_names_no_workarea_after_the_rename_migration() {
    let db = db_name("schema");
    if !mint_database(&db).await {
        return;
    }
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    store
        .migrate(&owner_url(&db))
        .await
        .expect("embedded migrations apply");
    store
        .create_tenant_partitions(Uuid::from_u128(0xA047))
        .await
        .expect("a tenant's partitions after the cut");
    let mut conn = PgConnection::connect(&owner_url(&db))
        .await
        .expect("connect");
    let before_the_cut = {
        let other = db_name("before");
        if mint_database(&other).await {
            let mut at_0046 = schema_through(&other, RENAME - 1).await;
            let n = former_names_in_catalog(&mut at_0046).await.len();
            at_0046.close().await.expect("close");
            drop_db(&other).await;
            Some(n)
        } else {
            None
        }
    };
    let found = former_names_in_catalog(&mut conn).await;
    conn.close().await.expect("close");
    drop_db(&db).await;
    assert!(found.is_empty(), "former names left: {found:#?}");
    // The law reads what it claims to: at 0046 it finds the former names.
    if let Some(n) = before_the_cut {
        assert!(n > 20, "the catalog at 0046 holds the former names ({n})");
    }
}

/// A v0.7 database's rows, seeded at 0046 in the former names, read in the
/// new names after 0047: roles, holders, the guardrail scope, the statuses
/// of nodes and transitions, and the act names of idempotency claims; a
/// client's own idempotency key keeps its text (ADR-0091 d6, d13, d14, d15).
#[tokio::test]
async fn a_database_migrated_across_the_rename_holds_its_rows_in_the_new_names() {
    let db = db_name("rows");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = schema_through(&db, RENAME - 1).await;
    let tenant = Uuid::from_u128(0xA047);
    let (root, child) = (Uuid::from_u128(0xB047), Uuid::from_u128(0xB048));
    let (admin, reviewer) = (Uuid::from_u128(0xC047), Uuid::from_u128(0xC048));
    let seed = [
        "SELECT set_config('oml.tenant_id', $1::text, false)",
        "INSERT INTO tenants (id, name) VALUES ($1, 'acme')",
        "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
           VALUES ('00000000-0000-0000-0000-00000000b047', $1, NULL, 'universe', 'committed')",
        "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
           VALUES ('00000000-0000-0000-0000-00000000b048', $1, \
                   '00000000-0000-0000-0000-00000000b047', 'metals', 'validated')",
        "INSERT INTO api_keys (id, tenant_id, name, role, principal, issued_seq, credential, \
                               held_by, guardrail_scope, default_workarea_id) \
           VALUES ('00000000-0000-0000-0000-00000000c047', $1, 'acme-admin', 'admin', \
                   'acme/admin', 1, 'bearer', 'human', 'ancestors', \
                   '00000000-0000-0000-0000-00000000b047')",
        "INSERT INTO api_keys (id, tenant_id, name, role, principal, issued_seq, credential, \
                               held_by, guardrail_scope, default_workarea_id) \
           VALUES ('00000000-0000-0000-0000-00000000c048', $1, 'the-dreamer', 'dreamer', \
                   'acme/ana', 2, 'bearer', 'agent', 'workarea', \
                   '00000000-0000-0000-0000-00000000b048')",
        "INSERT INTO grants (id, tenant_id, key_id, role, workarea_node_id, epoch) \
           VALUES ('00000000-0000-0000-0000-00000000d047', $1, \
                   '00000000-0000-0000-0000-00000000c048', 'dreamer', \
                   '00000000-0000-0000-0000-00000000b048', 3)",
        "INSERT INTO workarea_transitions (id, tenant_id, workarea_id, from_status, to_status, \
                                           state, proposer_key_id, proposer_principal, proposed_seq) \
           VALUES ('00000000-0000-0000-0000-00000000e047', $1, \
                   '00000000-0000-0000-0000-00000000b048', 'committed', 'validated', 'pending', \
                   '00000000-0000-0000-0000-00000000c047', 'acme/admin', 4)",
        "INSERT INTO idempotency (tenant_id, key, request_digest) \
           VALUES ($1, '00000000-0000-0000-0000-00000000c047:00000000-0000-0000-0000-000000000000:workarea.create:k-1', '\\x00'), \
                  ($1, '00000000-0000-0000-0000-00000000c047:00000000-0000-0000-0000-00000000b048:workarea.create-mine', '\\x00')",
    ];
    for statement in seed {
        sqlx::query(statement)
            .bind(tenant)
            .execute(&mut conn)
            .await
            .unwrap_or_else(|e| panic!("{statement}: {e}"));
    }
    let rename = MIGRATOR
        .iter()
        .find(|m| m.version == RENAME)
        .expect("migration 0047");
    conn.apply(rename).await.expect("0047 applies");

    let statuses: Vec<(Uuid, String)> =
        sqlx::query("SELECT id, status FROM workspaces WHERE tenant_id = $1 ORDER BY id")
            .bind(tenant)
            .fetch_all(&mut conn)
            .await
            .expect("workspaces")
            .iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect();
    assert_eq!(
        statuses,
        [(root, "protected".to_owned()), (child, "frozen".to_owned())]
    );
    let keys: Vec<(Uuid, String, String, String, Uuid)> = sqlx::query(
        "SELECT id, role, used_by, guardrail_scope, default_workspace_id FROM api_keys \
          WHERE tenant_id = $1 ORDER BY id",
    )
    .bind(tenant)
    .fetch_all(&mut conn)
    .await
    .expect("api_keys")
    .iter()
    .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
    .collect();
    assert_eq!(
        keys,
        [
            (
                admin,
                "admin".to_owned(),
                "person".to_owned(),
                "ancestors".to_owned(),
                root
            ),
            (
                reviewer,
                "reviewer".to_owned(),
                "agent".to_owned(),
                "workspace".to_owned(),
                child
            ),
        ]
    );
    let names: Vec<String> =
        sqlx::query_scalar("SELECT name FROM api_keys WHERE tenant_id = $1 ORDER BY id")
            .bind(tenant)
            .fetch_all(&mut conn)
            .await
            .expect("names");
    assert_eq!(
        names,
        ["acme-admin", "the-dreamer"],
        "a name is a key and keeps its spelling"
    );
    let grant: (String, Uuid) =
        sqlx::query("SELECT role, workspace_node_id FROM grants WHERE tenant_id = $1")
            .bind(tenant)
            .fetch_one(&mut conn)
            .await
            .map(|r| (r.get(0), r.get(1)))
            .expect("grant");
    assert_eq!(grant, ("reviewer".to_owned(), child));
    let transition: (String, String) = sqlx::query(
        "SELECT from_status, to_status FROM workspace_transitions WHERE tenant_id = $1",
    )
    .bind(tenant)
    .fetch_one(&mut conn)
    .await
    .map(|r| (r.get(0), r.get(1)))
    .expect("transition");
    assert_eq!(transition, ("protected".to_owned(), "frozen".to_owned()));
    let idempotency: Vec<String> =
        sqlx::query_scalar("SELECT key FROM idempotency WHERE tenant_id = $1 ORDER BY key")
            .bind(tenant)
            .fetch_all(&mut conn)
            .await
            .expect("idempotency");
    assert_eq!(
        idempotency,
        [
            "00000000-0000-0000-0000-00000000c047:00000000-0000-0000-0000-000000000000:workspace.create:k-1",
            "00000000-0000-0000-0000-00000000c047:00000000-0000-0000-0000-00000000b048:workarea.create-mine",
        ],
        "an act's name is renamed; a client's own key keeps its text"
    );
    // The replaced checks hold the new values and refuse the former ones.
    let refused = sqlx::query("UPDATE api_keys SET role = 'dreamer' WHERE tenant_id = $1")
        .bind(tenant)
        .execute(&mut conn)
        .await;
    assert!(refused.is_err(), "the role check refuses dreamer");
    conn.close().await.expect("close");
    drop_db(&db).await;
}

/// A tenant's partitions as v0.7's lead created them: each table `LIKE` its
/// parent `INCLUDING ALL`, fenced, and attached, so each derives its index
/// names and copies its column comments from the former schema.
fn v0_7_partition_ddl(tenant: Uuid) -> Vec<String> {
    let policy = "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) \
                  WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)";
    [
        "search_postings",
        "similar_keys",
        "memories",
        "recall_events",
        "recall_outcomes",
    ]
    .iter()
    .flat_map(|table| {
        let p = format!("{table}_t_{}", tenant.simple());
        [
            format!("CREATE TABLE {p} (LIKE {table} INCLUDING ALL)"),
            format!("ALTER TABLE {p} ENABLE ROW LEVEL SECURITY"),
            format!("ALTER TABLE {p} FORCE ROW LEVEL SECURITY"),
            format!("CREATE POLICY tenant_isolation ON {p} {policy}"),
            format!("ALTER TABLE {table} ATTACH PARTITION {p} FOR VALUES IN ('{tenant}')"),
        ]
    })
    .collect()
}

/// Two tenants partitioned by v0.7's DDL, each with a promoted memory whose
/// approval a person gave (`approver_held_by = 'human'`), migrate across
/// 0047: each reads `person`, through the owner and through the serving
/// role under its own tenant alone, and the catalog then names no former
/// name, the partitions' derived index names and copied comments included
/// (ADR-0091 d6, d15).
#[tokio::test]
async fn two_tenants_partitioned_by_v0_7_read_their_approvals_as_a_persons_after_the_rename() {
    let db = db_name("parts");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = schema_through(&db, RENAME - 1).await;
    let tenants = [Uuid::from_u128(0xA0471), Uuid::from_u128(0xA0472)];
    for (n, tenant) in tenants.iter().enumerate() {
        let n = n as u128;
        let (root, event, memory) = (
            Uuid::from_u128(0xB0471 + n),
            Uuid::from_u128(0xE0471 + n),
            Uuid::from_u128(0xC0471 + n),
        );
        sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
            .bind(tenant)
            .execute(&mut conn)
            .await
            .expect("tenant");
        sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, $2)")
            .bind(tenant)
            .bind(format!("acme{n}"))
            .execute(&mut conn)
            .await
            .expect("tenant row");
        for statement in v0_7_partition_ddl(*tenant) {
            conn.execute(statement.as_str())
                .await
                .unwrap_or_else(|e| panic!("{statement}: {e}"));
        }
        sqlx::query(
            "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
             VALUES ($1, $2, NULL, 'universe', 'committed')",
        )
        .bind(root)
        .bind(tenant)
        .execute(&mut conn)
        .await
        .expect("root");
        sqlx::query(
            "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, \
                                  payload, signed_bytes_hash, sig, signed_bytes) \
             VALUES ($1, 1, $2, 'submission.remember', '\\x00', 'acme/agent', '{}', \
                     '\\x00', '\\x00', '\\x00')",
        )
        .bind(tenant)
        .bind(event)
        .execute(&mut conn)
        .await
        .expect("journal row");
        sqlx::query(
            "INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, \
                                   created_event_id, claim_key, author_principal, author_key_id, \
                                   version, kind, body_bytes, approver_held_by, \
                                   approver_same_user, approved_at) \
             VALUES ($1, $2, $3, 'nickel quote', '\\x00', 'promoted', $4, 'quote', 'acme/agent', \
                     $5, 1, 'memory', 12, 'human', false, now())",
        )
        .bind(memory)
        .bind(tenant)
        .bind(root)
        .bind(event)
        .bind(Uuid::from_u128(0xD0471 + n))
        .execute(&mut conn)
        .await
        .expect("a promoted memory a person approved");
    }
    let rename = MIGRATOR
        .iter()
        .find(|m| m.version == RENAME)
        .expect("migration 0047");
    conn.apply(rename).await.expect("0047 applies");

    for tenant in &tenants {
        sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
            .bind(tenant)
            .execute(&mut conn)
            .await
            .expect("tenant");
        let holders: Vec<String> =
            sqlx::query_scalar("SELECT approver_used_by FROM memories WHERE tenant_id = $1")
                .bind(tenant)
                .fetch_all(&mut conn)
                .await
                .expect("approvals");
        assert_eq!(holders, ["person"], "{tenant}");
    }
    let found = former_names_in_catalog(&mut conn).await;
    assert!(found.is_empty(), "former names left: {found:#?}");
    conn.close().await.expect("close");

    let mut serving = PgConnection::connect(&serving_url(&db))
        .await
        .expect("serving connect");
    for tenant in &tenants {
        let mut tx = serving.begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1::text, true)")
            .bind(tenant)
            .execute(&mut *tx)
            .await
            .expect("tenant");
        let read: Vec<(Uuid, String)> = sqlx::query(
            "SELECT tenant_id, approver_used_by FROM memories WHERE workspace_id IS NOT NULL",
        )
        .fetch_all(&mut *tx)
        .await
        .expect("the serving role reads the renamed columns")
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
        assert_eq!(
            read,
            [(*tenant, "person".to_owned())],
            "{tenant} reads its own row alone"
        );
        tx.rollback().await.expect("rollback");
    }
    serving.close().await.expect("close");
    drop_db(&db).await;
}
