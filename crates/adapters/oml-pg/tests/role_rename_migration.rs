//! Migration 0066, the v0.8.4 role rename (ADR-0093 d5): after it no role
//! column and no role check holds the former role, and a database migrated
//! across it holds each key and grant in the role the former-name map's
//! rule 7 reads its signed event as (d4), so the rows a rebuild would fold
//! equal the migrated ones. The rows are seeded at 0065 in the former role,
//! as a v0.8.3 deployment holds them, for two tenants under forced row
//! security.

use std::sync::atomic::Ordering;

use oml_app::domain::{
    EventDraft, EventTime, KeySet, SignedEnvelope, event_types, next_envelope, sign,
};
use oml_pg::{MIGRATOR, PgStore};
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use sqlx::migrate::Migrate;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Row};
use uuid::Uuid;

/// The migration that renames the role; every earlier one keeps it.
const ROLE_RENAME: i64 = 66;

fn db_name(tag: &str) -> String {
    format!(
        "oml_roles_{tag}_{}_{}_{}",
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

/// Each role column value, role check and function body that names the
/// former role, as `what: text`.
async fn former_role_in_schema(conn: &mut PgConnection) -> Vec<String> {
    let queries = [
        "SELECT 'check: ' || conname || ' ' || pg_get_constraintdef(oid) FROM pg_constraint \
          WHERE connamespace = 'public'::regnamespace AND contype = 'c' \
            AND pg_get_constraintdef(oid) ~ '''writer'''",
        "SELECT 'function: ' || proname FROM pg_proc \
          WHERE pronamespace = 'public'::regnamespace AND prosrc ~ '''writer'''",
        "SELECT 'role column: ' || c.relname || '.' || a.attname FROM pg_attribute a \
           JOIN pg_class c ON c.oid = a.attrelid \
          WHERE c.relnamespace = 'public'::regnamespace AND c.relkind IN ('r', 'p') \
            AND a.attnum > 0 AND NOT a.attisdropped AND a.attname ~ 'role' \
            AND (c.relname, a.attname) NOT IN (('api_keys', 'role'), ('grants', 'role'))",
    ];
    let mut found = Vec::new();
    for q in queries {
        let rows: Vec<String> = sqlx::query_scalar(q)
            .fetch_all(&mut *conn)
            .await
            .expect("catalog read");
        found.extend(rows);
    }
    // The two role columns, read under the startup marker across every
    // tenant's rows.
    sqlx::query("SELECT set_config('oml.startup_check', 'on', false)")
        .execute(&mut *conn)
        .await
        .expect("startup marker");
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM tenants ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .expect("tenants");
    for tenant in tenants {
        sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
            .bind(tenant)
            .execute(&mut *conn)
            .await
            .expect("tenant");
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT 'api_keys.role: ' || name FROM api_keys WHERE role = 'writer' \
             UNION ALL SELECT 'grants.role: ' || id::text FROM grants WHERE role = 'writer'",
        )
        .fetch_all(&mut *conn)
        .await
        .expect("role values");
        found.extend(rows);
    }
    sqlx::query("SELECT set_config('oml.tenant_id', '', false)")
        .execute(&mut *conn)
        .await
        .expect("clear tenant");
    sqlx::query("SELECT set_config('oml.startup_check', '', false)")
        .execute(&mut *conn)
        .await
        .expect("clear marker");
    found
}

/// One v0.8.3 tenant: the root, three keys as v0.8.3 issued them (a writer
/// key with a writer and a reader grant, a writer key issued with
/// `--write-only` and its writer grant alone, a reviewer key with a reviewer
/// grant), their rows, and the signed chain that issued them.
struct Seeded {
    tenant: Uuid,
    root: Uuid,
    recalls: Uuid,
    write_only: Uuid,
    reviewer: Uuid,
}

impl Seeded {
    fn new(n: u128) -> Self {
        Self {
            tenant: Uuid::from_u128(0xA066_0 + n),
            root: Uuid::from_u128(0xB066_0 + n),
            recalls: Uuid::from_u128(0xC066_0 + n),
            write_only: Uuid::from_u128(0xC066_8 + n),
            reviewer: Uuid::from_u128(0xC066_F + n),
        }
    }

    /// `(key, name, role)` and `(grant id, key, role)` as v0.8.3 stored them.
    fn keys(&self) -> [(Uuid, &'static str, &'static str); 3] {
        [
            (self.recalls, "agent-recalls", "writer"),
            (self.write_only, "agent-write-only", "writer"),
            (self.reviewer, "agent-reviews", "reviewer"),
        ]
    }

    fn grants(&self) -> [(Uuid, Uuid, &'static str); 4] {
        let g = |k: u128| Uuid::from_u128((self.tenant.as_u128() << 8) + k);
        [
            (g(1), self.recalls, "writer"),
            (g(2), self.recalls, "reader"),
            (g(3), self.write_only, "writer"),
            (g(4), self.reviewer, "reviewer"),
        ]
    }
}

/// The tenant's chain as v0.8.3 signed it: a genesis, a `key.issued` per
/// key and a `grant.issued` per grant, each stored with its signed bytes,
/// and the chain head.
async fn v0_8_3_signed_chain(conn: &mut PgConnection, seeded: &Seeded) {
    sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
        .bind(seeded.tenant)
        .execute(&mut *conn)
        .await
        .expect("tenant");
    let key = oml_port_contracts::test_signing_key();
    let mut drafts = vec![(
        event_types::TENANT_GENESIS.to_owned(),
        serde_json::json!({ "tenant_name": format!("acme-{}", seeded.tenant.simple()) }),
    )];
    for (id, name, role) in seeded.keys() {
        drafts.push((
            event_types::KEY_ISSUED.to_owned(),
            serde_json::json!({ "key_id": id.to_string(), "name": name, "role": role,
                                "used_by": "agent", "credential": "bearer" }),
        ));
    }
    for (id, key_id, role) in seeded.grants() {
        drafts.push((
            event_types::GRANT_ISSUED.to_owned(),
            serde_json::json!({ "grant_id": id.to_string(), "key_id": key_id.to_string(),
                                "workspace_node_id": seeded.root.to_string(), "role": role,
                                "deny": [], "covers": "subtree" }),
        ));
    }
    let mut head: Option<SignedEnvelope> = None;
    for (n, (event_type, payload)) in drafts.into_iter().enumerate() {
        let draft = EventDraft {
            tenant_id: seeded.tenant,
            event_id: Uuid::from_u128((seeded.tenant.as_u128() << 16) + n as u128),
            event_type,
            principal: "acme/admin".to_owned(),
            api_key_id: None,
            at: EventTime::parse(&format!("2026-11-19T09:00:0{n}.000000Z")).expect("time"),
            payload,
        };
        let event =
            sign(next_envelope(head.as_ref(), draft).expect("lawful"), &key).expect("signs");
        let env = &event.envelope;
        sqlx::query(
            "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, \
             api_key_id, at, payload, signed_bytes, signed_bytes_hash, sig) \
             VALUES ($1, $2, $3, $4, $5, $6, NULL, ($7)::timestamptz, $8, $9, $10, $11)",
        )
        .bind(seeded.tenant)
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
        .execute(&mut *conn)
        .await
        .expect("journal row");
        head = Some(event);
    }
    let last = head.expect("a head");
    sqlx::query("INSERT INTO journal_chain_head (tenant_id, seq, head_hash) VALUES ($1, $2, $3)")
        .bind(seeded.tenant)
        .bind(i64::try_from(last.envelope.seq).expect("seq"))
        .bind(last.hash.0.to_vec())
        .execute(&mut *conn)
        .await
        .expect("chain head");
}

/// The tenant's rows at 0065, written under its own `oml.tenant_id`, as the
/// forced row security of `api_keys` and `grants` requires of the owner.
async fn v0_8_3_rows(conn: &mut PgConnection, seeded: &Seeded) {
    sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
        .bind(seeded.tenant)
        .execute(&mut *conn)
        .await
        .expect("tenant");
    sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, $2)")
        .bind(seeded.tenant)
        .bind(format!("acme-{}", seeded.tenant.simple()))
        .execute(&mut *conn)
        .await
        .expect("tenant row");
    sqlx::query(
        "INSERT INTO workspaces (id, tenant_id, parent_id, name, status) \
         VALUES ($1, $2, NULL, 'universe', 'protected')",
    )
    .bind(seeded.root)
    .bind(seeded.tenant)
    .execute(&mut *conn)
    .await
    .expect("root");
    for (n, (id, name, role)) in seeded.keys().into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO api_keys (id, tenant_id, name, role, principal, issued_seq, credential, \
                                   used_by, default_workspace_id) \
             VALUES ($1, $2, $3, $4, $3, $5, 'bearer', 'agent', $6)",
        )
        .bind(id)
        .bind(seeded.tenant)
        .bind(name)
        .bind(role)
        .bind(i64::try_from(n).expect("small") + 2)
        .bind(seeded.root)
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("key {name}: {e}"));
    }
    for (n, (id, key_id, role)) in seeded.grants().into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO grants (id, tenant_id, key_id, role, workspace_node_id, epoch) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(seeded.tenant)
        .bind(key_id)
        .bind(role)
        .bind(seeded.root)
        .bind(i64::try_from(n).expect("small") + 5)
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|e| panic!("grant {id}: {e}"));
    }
    sqlx::query("SELECT set_config('oml.tenant_id', '', false)")
        .execute(&mut *conn)
        .await
        .expect("clear tenant");
}

/// The catalog after every migration holds no role check, function body or
/// role column that names the former role, and a database whose keys were
/// writers at 0065 holds none after 0066 (ADR-0093 d5).
#[tokio::test]
async fn the_schema_holds_no_writer_role_after_the_role_migration() {
    let db = db_name("schema");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = schema_through(&db, ROLE_RENAME - 1).await;
    let seeded = Seeded::new(1);
    v0_8_3_rows(&mut conn, &seeded).await;
    // The law reads what it claims to: at 0065 it finds the former role in
    // both checks and in the seeded rows.
    let before = former_role_in_schema(&mut conn).await;
    assert_eq!(before.len(), 6, "{before:#?}");
    for migration in MIGRATOR.iter().filter(|m| m.version >= ROLE_RENAME) {
        conn.apply(migration).await.expect("the migrations apply");
    }
    let found = former_role_in_schema(&mut conn).await;
    conn.close().await.expect("close");
    drop_db(&db).await;
    assert!(found.is_empty(), "the former role is left: {found:#?}");
}

/// Two v0.8.3 tenants, each with a writer key that recalls, a writer key
/// issued with `--write-only` and a reviewer key, migrate across 0066 under
/// forced row security: every writer key reads `contributor`, the one
/// without a reader grant included, and every writer grant `submitter`,
/// through the owner and through the serving role under its own tenant
/// alone. Each row equals what rule 7 reads its signed event as, so a
/// rebuild folds the same rows (ADR-0093 d3, d4, d5); the chain verifies as
/// signed and leaves no unmapped former name, and the new checks refuse the
/// former role.
#[tokio::test]
async fn a_database_migrated_across_the_role_rename_holds_its_rows_in_the_roles_rule_7_reads() {
    let db = db_name("rows");
    if !mint_database(&db).await {
        return;
    }
    let mut conn = schema_through(&db, ROLE_RENAME - 1).await;
    let tenants = [Seeded::new(1), Seeded::new(2)];
    for seeded in &tenants {
        v0_8_3_rows(&mut conn, seeded).await;
        v0_8_3_signed_chain(&mut conn, seeded).await;
    }
    let rename = MIGRATOR
        .iter()
        .find(|m| m.version == ROLE_RENAME)
        .expect("migration 0066");
    conn.apply(rename).await.expect("0066 applies");

    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    for seeded in &tenants {
        sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
            .bind(seeded.tenant)
            .execute(&mut conn)
            .await
            .expect("tenant");
        let keys: Vec<(Uuid, String)> =
            sqlx::query("SELECT id, role FROM api_keys WHERE tenant_id = $1 ORDER BY id")
                .bind(seeded.tenant)
                .fetch_all(&mut conn)
                .await
                .expect("api_keys")
                .iter()
                .map(|r| (r.get(0), r.get(1)))
                .collect();
        assert_eq!(
            keys,
            [
                (seeded.recalls, "contributor".to_owned()),
                (seeded.write_only, "contributor".to_owned()),
                (seeded.reviewer, "reviewer".to_owned()),
            ],
            "{}: a write-only key is a contributor without recall",
            seeded.tenant
        );
        let grants: Vec<(Uuid, String)> =
            sqlx::query("SELECT id, role FROM grants WHERE tenant_id = $1 ORDER BY id")
                .bind(seeded.tenant)
                .fetch_all(&mut conn)
                .await
                .expect("grants")
                .iter()
                .map(|r| (r.get(0), r.get(1)))
                .collect();
        let expected: Vec<(Uuid, String)> = seeded
            .grants()
            .iter()
            .map(|(id, _, role)| {
                (
                    *id,
                    if *role == "writer" { "submitter" } else { role }.to_owned(),
                )
            })
            .collect();
        assert_eq!(grants, expected, "{}", seeded.tenant);

        // M ∘ fold = fold ∘ map: each row's role is what rule 7 reads the
        // event that issued it as.
        let mut unmapped = Vec::new();
        let mut read_roles: Vec<(Uuid, String)> = Vec::new();
        store
            .verify_stored_chain_reading(
                seeded.tenant,
                &KeySet::new(vec![
                    oml_port_contracts::test_signing_key().verifying_key(),
                ]),
                |event| {
                    let env = &event.envelope;
                    unmapped.extend(oml_app::domain::former_names::unmapped_former_names(
                        &env.event_type,
                        &env.payload,
                    ));
                    let read =
                        oml_app::domain::former_names::read_event(&env.event_type, &env.payload)
                            .expect("mapped");
                    let id = match read.event_type.as_str() {
                        event_types::KEY_ISSUED => read.payload["key_id"].as_str(),
                        event_types::GRANT_ISSUED => read.payload["grant_id"].as_str(),
                        _ => None,
                    };
                    if let Some(id) = id {
                        read_roles.push((
                            Uuid::parse_str(id).expect("id"),
                            read.payload["role"].as_str().expect("role").to_owned(),
                        ));
                    }
                },
            )
            .await
            .expect("the v0.8.3 chain verifies after 0066");
        assert_eq!(unmapped, Vec::<String>::new(), "rule 7 reads every writer");
        read_roles.sort();
        let mut rows: Vec<(Uuid, String)> = keys.into_iter().chain(grants).collect();
        rows.sort();
        assert_eq!(rows, read_roles, "{}: the rows equal the map's reading", seeded.tenant);
    }
    // The serving role reads its own tenant's renamed rows alone.
    let mut serving = PgConnection::connect(&serving_url(&db))
        .await
        .expect("serving connect");
    for seeded in &tenants {
        let mut tx = serving.begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1::text, true)")
            .bind(seeded.tenant)
            .execute(&mut *tx)
            .await
            .expect("tenant");
        let read: Vec<(Uuid, String)> =
            sqlx::query("SELECT tenant_id, role FROM grants ORDER BY role")
                .fetch_all(&mut *tx)
                .await
                .expect("the serving role reads the grants")
                .iter()
                .map(|r| (r.get(0), r.get(1)))
                .collect();
        assert_eq!(
            read,
            [
                (seeded.tenant, "reader".to_owned()),
                (seeded.tenant, "reviewer".to_owned()),
                (seeded.tenant, "submitter".to_owned()),
                (seeded.tenant, "submitter".to_owned()),
            ],
            "{} reads its own rows alone",
            seeded.tenant
        );
        tx.rollback().await.expect("rollback");
    }
    serving.close().await.expect("close");
    // The replaced checks refuse the former role.
    sqlx::query("SELECT set_config('oml.tenant_id', $1::text, false)")
        .bind(tenants[0].tenant)
        .execute(&mut conn)
        .await
        .expect("tenant");
    for refused in [
        "UPDATE api_keys SET role = 'writer' WHERE tenant_id = $1",
        "UPDATE grants SET role = 'writer' WHERE tenant_id = $1",
        "UPDATE grants SET role = 'contributor' WHERE tenant_id = $1",
    ] {
        assert!(
            sqlx::query(refused)
                .bind(tenants[0].tenant)
                .execute(&mut conn)
                .await
                .is_err(),
            "{refused}"
        );
    }
    conn.close().await.expect("close");
    drop_db(&db).await;
}
