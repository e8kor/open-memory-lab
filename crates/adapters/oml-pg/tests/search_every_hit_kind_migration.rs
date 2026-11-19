//! Migration 0062 (ADR-0092 d3, d4; d12 reading 1): a database upgraded
//! across it, as its production owner applies it (a role with neither
//! SUPERUSER nor BYPASSRLS, so every tenant-fenced read is under row
//! security), gives every skill and guardrail written before it postings
//! of its own kind, and folds a skill's name and description into its
//! lexemes.

use std::sync::atomic::Ordering;

use oml_pg::MIGRATOR;
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag};
use sqlx::migrate::Migrate;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use uuid::Uuid;

/// The migration that gives every hit kind postings.
const EVERY_HIT_KIND: i64 = 62;

fn db_name(tag: &str) -> String {
    format!(
        "oml_hit_kinds_{tag}_{}_{}_{}",
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

/// A held skill and a held guardrail written at 0061. 0062, applied with
/// no tenant named on the connection, as a deployment runs it, enumerates
/// the tenants under 0003's startup policy and refills them: the skill
/// posts under code 5 with its name's lexeme, the guardrail under code 6.
#[tokio::test]
async fn migration_0062_gives_the_owners_skills_and_guardrails_postings_and_folds_the_skills_name()
{
    let db = db_name("upgrade");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = schema_before(&db, EVERY_HIT_KIND).await;
    let super_user: bool = sqlx::query_scalar(
        "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut conn)
    .await
    .expect("role attributes");
    assert!(
        !super_user,
        "the owner is the production shape: no superuser, no BYPASSRLS"
    );
    let tenant = Uuid::from_u128(0xA062);
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
    let universe = Uuid::from_u128(0xB062);
    sqlx::query(
        "INSERT INTO workspaces (id, tenant_id, parent_id, name, status, user_root) \
         VALUES ($1, $2, NULL, 'universe', 'protected', false)",
    )
    .bind(universe)
    .bind(tenant)
    .execute(&mut conn)
    .await
    .expect("workspace");
    let key = Uuid::from_u128(0xC062);
    sqlx::query(
        "INSERT INTO api_keys (id, tenant_id, name, role, principal, issued_seq, credential, used_by) \
         VALUES ($1, $2, 'writer-key', 'writer', 'writer-key', 1, 'bearer', 'agent')",
    )
    .bind(key)
    .bind(tenant)
    .execute(&mut conn)
    .await
    .expect("key");
    // (id, kind, claim, skill name, skill description, severity)
    let rows: [(u128, &str, &str, Option<&str>, Option<&str>, Option<&str>); 2] = [
        (
            0xD062,
            "skill",
            "run the checks then tag",
            Some("kangaroo release"),
            Some("steps before a wombat tag"),
            None,
        ),
        (
            0xD063,
            "guardrail",
            "never push on friday",
            None,
            None,
            Some("never"),
        ),
    ];
    for (n, (id, kind, claim, name, description, severity)) in rows.iter().enumerate() {
        let event = Uuid::from_u128(0xE000 + *id);
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
                                   version, kind, body_bytes, skill_name, skill_description, \
                                   severity) \
             VALUES ($1, $2, $3, $4, '\\x00', 'held', $5, $6, 'acme/agent', $7, 1, $8, 11, \
                     $9, $10, $11)",
        )
        .bind(Uuid::from_u128(*id))
        .bind(tenant)
        .bind(universe)
        .bind(*claim)
        .bind(event)
        .bind(format!("{kind}-{n}"))
        .bind(key)
        .bind(*kind)
        .bind(*name)
        .bind(*description)
        .bind(*severity)
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("{kind}: {e}"));
    }
    // A deployment's migration connection names no tenant.
    conn.execute("SELECT set_config('oml.tenant_id', '', false)")
        .await
        .expect("no tenant");
    let migration = MIGRATOR
        .iter()
        .find(|m| m.version == EVERY_HIT_KIND)
        .expect("migration 0062");
    conn.apply(migration).await.expect("0062 applies");

    sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
        .bind(tenant)
        .execute(&mut conn)
        .await
        .expect("tenant");
    for (code, id) in [(5_i16, 0xD062_u128), (6, 0xD063)] {
        let postings: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM search_postings WHERE memory_id = $1 AND memory_kind = $2",
        )
        .bind(Uuid::from_u128(id))
        .bind(code)
        .fetch_one(&mut conn)
        .await
        .expect("postings");
        assert!(postings > 0, "memory_kind {code} has postings after 0062");
    }
    let name_posted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM search_postings p JOIN memories m ON m.id = p.memory_id \
          CROSS JOIN LATERAL unnest(m.search_vector) l \
          WHERE m.id = $1 AND l.lexeme = 'kangaroo' AND p.token_id = oml_token_id(l.lexeme))",
    )
    .bind(Uuid::from_u128(0xD062))
    .fetch_one(&mut conn)
    .await
    .expect("the skill's lexemes");
    assert!(
        name_posted,
        "the skill's name is among its lexemes and postings"
    );
    drop(conn);
    drop_db(&db).await;
}
