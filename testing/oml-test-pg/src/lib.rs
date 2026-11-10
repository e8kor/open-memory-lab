//! Throwaway PostgreSQL databases for the integration suites.
//!
//! A suite names its cluster in `DATABASE_URL`; no suite defaults to one, so none reaches a live
//! server. Each test mints a database of its own with an owner/serving role pair, as the
//! role split has it (ADR-0034 d2, d8), runs against it, and drops both with the database.
//! Dev-dependency only: the lead and the CLI link none of this.

use std::sync::atomic::AtomicU64;

use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
pub const PAIR_PASSWORD: &str = "oml_test_pair";

/// A counter for database names, one per test binary: with the process id and
/// [`run_tag`] it makes a name no parallel test shares.
pub static DB_N: AtomicU64 = AtomicU64::new(0);

/// `DATABASE_URL`, the throwaway cluster the suites run against.
pub fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL names a throwaway PostgreSQL cluster; no suite defaults to one, so none reaches a live server")
}

/// `db` at `DATABASE_URL`'s host, logged in as `role`.
pub fn role_url(db: &str, role: &str) -> String {
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
pub fn owner_url(db: &str) -> String {
    role_url(db, &format!("{db}_owner"))
}

/// `db` through the serving login of its role pair (ADR-0034 d2, d8).
pub fn serving_url(db: &str) -> String {
    role_url(db, &format!("{db}_serving"))
}

/// Eight hex digits of the clock, so a name a crashed earlier run left
/// behind on the cluster under the same pid and counter is not reused.
pub fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

/// Who owns a minted database: the owner of its pair, as the split has it,
/// or the serving login, as every unsplit install has it (ADR-0034 d5).
#[derive(Clone, Copy)]
pub enum DatabaseOwner {
    /// The owner login, as the role split has it.
    Owner,
    /// The serving login, as every unsplit install has it.
    Serving,
}

/// Mints `db`'s owner/serving role pair, `LOGIN` and no other attribute,
/// and creates `db` owned by the owner (ADR-0034 d2, d8); `false`, with
/// `did not run: <role> cannot create roles` printed, when `DATABASE_URL`'s
/// role is not the superuser minting takes, as the maintainer's local `oml`
/// (CREATEDB alone) is not. Under `OML_TEST_REQUIRE_SPLIT=1`, which CI sets,
/// the same condition fails the test, so no run passes vacuously.
pub async fn mint_database(db: &str) -> bool {
    mint_database_owned_by(db, DatabaseOwner::Owner).await
}

/// [`mint_database`], with the database owned by `owner` instead of by the
/// owner login.
pub async fn mint_database_owned_by(db: &str, owner: DatabaseOwner) -> bool {
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    let (role, mints): (String, bool) = sqlx::query_as(
        "SELECT current_user::text, rolsuper FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut admin)
    .await
    .expect("role attributes");
    if !mints {
        assert!(
            std::env::var("OML_TEST_REQUIRE_SPLIT").as_deref() != Ok("1"),
            "{role} cannot create roles, and OML_TEST_REQUIRE_SPLIT=1 requires the role pair (ADR-0034 d8)"
        );
        println!("did not run: {role} cannot create roles");
        return false;
    }
    for role in ["owner", "serving"] {
        admin
            .execute(format!("CREATE ROLE {db}_{role} LOGIN PASSWORD '{PAIR_PASSWORD}'").as_str())
            .await
            .expect("mint the role pair");
    }
    let owner = match owner {
        DatabaseOwner::Owner => "owner",
        DatabaseOwner::Serving => "serving",
    };
    admin
        .execute(format!("CREATE DATABASE {db} OWNER {db}_{owner}").as_str())
        .await
        .expect("create throwaway database");
    true
}

/// Drops `name` and its role pair; a cluster that cannot be reached is not an error, since a
/// test that failed to connect has nothing to drop.
pub async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
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
