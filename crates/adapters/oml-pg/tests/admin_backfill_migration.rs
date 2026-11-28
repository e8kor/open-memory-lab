//! Migration 0077 (ADR-0097 d10, d11; review finding store-1): 0070's
//! backfills run again under each tenant's row security, so on a database
//! migrated by the owner role, whose forced row security hides every row
//! of a tenant not named by `oml.tenant_id`, a tenant that had two admin
//! persons leaves `sole_admin`, and every admin entry accepted before the
//! upgrade gets the `admin_since` its admin-entry event records.

use std::sync::atomic::Ordering;

use oml_pg::MIGRATOR;
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag};
use sqlx::migrate::Migrate;
use sqlx::postgres::PgConnection;
use sqlx::Connection;
use uuid::Uuid;

/// The migration that adds `sole_admin` and `admin_since`.
const ADMIN_ACTS: i64 = 70;
/// The migration that backfills them under each tenant's binding.
const BACKFILL: i64 = 77;

fn db_name(tag: &str) -> String {
    format!(
        "oml_admin_backfill_{tag}_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

async fn bind_tenant(conn: &mut PgConnection, tenant: Option<Uuid>) {
    sqlx::query("SELECT set_config('oml.tenant_id', $1, false)")
        .bind(tenant.map(|t| t.to_string()).unwrap_or_default())
        .execute(&mut *conn)
        .await
        .expect("tenant binding");
}

/// One tenant whose admin entries are `admins`, each with its accepted
/// `web.email_allowed {admin: true}` event at the given time, seeded under
/// the tenant's own binding as the serving role writes it.
async fn seed_tenant(conn: &mut PgConnection, n: u128, admins: &[(&str, &str)]) -> Uuid {
    let tenant = Uuid::from_u128(0xA077 + n);
    bind_tenant(conn, Some(tenant)).await;
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, $2)")
        .bind(tenant)
        .bind(format!("acme{n}"))
        .execute(&mut *conn)
        .await
        .expect("tenant row");
    for (seq, (user, at)) in admins.iter().enumerate() {
        let email = format!("{user}@example.com");
        sqlx::query(
            "INSERT INTO web_emails (tenant_id, email_hash, email, user_name, admin, created_at, added_by) \
             VALUES ($1, sha256(convert_to($2, 'UTF8')), $2, $3, true, '2026-01-01T00:00:00Z', $4)",
        )
        .bind(tenant)
        .bind(&email)
        .bind(*user)
        .bind(Uuid::from_u128(0xC077 + n))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("entry {user}: {e}"));
        sqlx::query(
            "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, at, \
                                  payload, signed_bytes_hash, sig, signed_bytes) \
             VALUES ($1, $2, $3, 'web.email_allowed', '\\x00', 'acme/admin', $4::timestamptz, \
                     jsonb_build_object('email_digest', \
                         'sha256:' || encode(sha256(convert_to($5, 'UTF8')), 'hex'), \
                         'user', $6::text, 'admin', true), \
                     '\\x00', '\\x00', '\\x00')",
        )
        .bind(tenant)
        .bind(i64::try_from(seq).expect("small") + 1)
        .bind(Uuid::from_u128(0xE077 + n * 16 + u128::try_from(seq).expect("small")))
        .bind(*at)
        .bind(&email)
        .bind(*user)
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("event {user}: {e}"));
    }
    tenant
}

/// Two tenants seeded before 0070: one that had two admin persons, one
/// that had one. The owner role runs 0070 through 0077 with no tenant
/// bound, as production runs them; afterwards the first tenant is out of
/// `sole_admin`, the second is still in it, and each entry's
/// `admin_since` is its admin event's time.
#[tokio::test]
async fn the_admin_backfill_runs_under_each_tenants_row_security_on_the_owner_role() {
    let db = db_name("rls");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = PgConnection::connect(&owner_url(&db))
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
    for migration in MIGRATOR.iter().filter(|m| m.version < ADMIN_ACTS) {
        conn.apply(migration).await.expect("the migrations apply");
    }
    let two = seed_tenant(
        &mut conn,
        0,
        &[
            ("alice", "2026-02-01T10:00:00Z"),
            ("bob", "2026-03-01T10:00:00Z"),
        ],
    )
    .await;
    let one = seed_tenant(&mut conn, 1, &[("carol", "2026-04-01T10:00:00Z")]).await;
    // Production migrates with no tenant bound, under forced row security.
    bind_tenant(&mut conn, None).await;
    let forced: bool = sqlx::query_scalar(
        "SELECT relforcerowsecurity FROM pg_class WHERE relname = 'web_emails'",
    )
    .fetch_one(&mut conn)
    .await
    .expect("the table");
    assert!(forced, "web_emails forces row security for the owner");
    for migration in MIGRATOR
        .iter()
        .filter(|m| (ADMIN_ACTS..=BACKFILL).contains(&m.version))
    {
        conn.apply(migration)
            .await
            .unwrap_or_else(|e| panic!("migration {}: {e}", migration.version));
    }

    for (tenant, sole, entries) in [
        (
            two,
            false,
            vec![
                ("alice", "2026-02-01 10:00:00+00"),
                ("bob", "2026-03-01 10:00:00+00"),
            ],
        ),
        (one, true, vec![("carol", "2026-04-01 10:00:00+00")]),
    ] {
        bind_tenant(&mut conn, Some(tenant)).await;
        let sole_admin: bool = sqlx::query_scalar("SELECT sole_admin FROM tenants WHERE id = $1")
            .bind(tenant)
            .fetch_one(&mut conn)
            .await
            .expect("the tenant");
        assert_eq!(sole_admin, sole, "tenant {tenant}");
        for (user, at) in entries {
            let since: Option<String> = sqlx::query_scalar(
                "SELECT to_char(admin_since AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') || '+00' \
                   FROM web_emails WHERE user_name = $1",
            )
            .bind(user)
            .fetch_one(&mut conn)
            .await
            .expect("the entry");
            assert_eq!(since.as_deref(), Some(at), "{user}'s admin_since");
        }
    }
    conn.close().await.expect("close");
    drop_db(&db).await;
}
