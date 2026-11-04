//! The lead binary's boot under the role split (ADR-0034 d4, d5, d8): a
//! boot over one URL, with no owner URL, over an unsplit database or after
//! a migration grants `TRUNCATE` exits non-zero with `postgres.role_split`
//! and its reason before any listener binds, and once the listener binds
//! `pg_stat_activity` shows no owner connection. Each case spawns
//! `oml-server-lead` itself, so the refusal is the one `main` logs.

use std::io::Read as _;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};

/// How long a boot may take before the test calls it hung.
const BOOT_DEADLINE: Duration = Duration::from_secs(90);

// ---------------------------------------------------------------------------
// Throwaway databases and role pairs (the http_api.rs pattern)
// ---------------------------------------------------------------------------

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
const PAIR_PASSWORD: &str = "oml_test_pair";

/// `db` at `DATABASE_URL`'s host, logged in as `role` with `password`.
fn login_url(db: &str, role: &str, password: &str) -> String {
    let base = base_url();
    let (scheme, rest) = base
        .split_once("://")
        .expect("DATABASE_URL is a postgres:// URL");
    let (_, host_path) = rest.rsplit_once('@').expect("DATABASE_URL names its user");
    let (host, _) = host_path
        .split_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{scheme}://{role}:{password}@{host}/{db}")
}

/// `db` through `DATABASE_URL`'s own login.
fn admin_url(db: &str) -> String {
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{prefix}/{db}")
}

/// `db` through the owner login of its role pair (ADR-0034 d2, d8).
fn owner_url(db: &str) -> String {
    login_url(db, &format!("{db}_owner"), PAIR_PASSWORD)
}

/// `db` through the serving login of its role pair (ADR-0034 d2, d8).
fn serving_url(db: &str) -> String {
    login_url(db, &format!("{db}_serving"), PAIR_PASSWORD)
}

/// Eight hex digits of the clock, so a name a crashed earlier run left
/// behind on the cluster under the same pid and counter is not reused.
fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

fn fresh_name() -> String {
    format!(
        "oml_test_split_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

/// Who owns a minted database: the owner of its pair, as the split has it,
/// or the serving login, as every unsplit install has it (ADR-0034 d5).
#[derive(Clone, Copy)]
enum DatabaseOwner {
    Owner,
    Serving,
}

/// Mints `db`'s owner/serving role pair, `LOGIN` and no other attribute,
/// and creates `db` owned by `owner` (ADR-0034 d2, d8); `false`, with
/// `did not run: <role> cannot create roles` printed, when `DATABASE_URL`'s
/// role is not the superuser minting takes, as the maintainer's local `oml`
/// (CREATEDB alone) is not. Under `OML_TEST_REQUIRE_SPLIT=1`, which CI sets,
/// the same condition fails the test, so no run passes vacuously.
async fn mint_database(db: &str, owner: DatabaseOwner) -> bool {
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

/// A throwaway database `DATABASE_URL`'s own role creates and owns, which
/// takes only `CREATEDB`, so a case that needs no role pair runs locally.
async fn plain_database(db: &str) {
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("CREATE DATABASE {db}").as_str())
        .await
        .expect("create throwaway database");
}

async fn drop_db(name: &str) {
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

// ---------------------------------------------------------------------------
// The lead binary
// ---------------------------------------------------------------------------

/// A port no listener holds now: the kernel's pick, released at once.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("its address")
        .port()
}

/// A running `oml-server-lead` and what it has written to stderr so far.
struct Lead {
    child: Child,
    stderr: Arc<Mutex<String>>,
    port: u16,
    scratch: std::path::PathBuf,
}

impl Lead {
    /// Spawns the lead with `postgres.url` and, when given,
    /// `postgres.owner_url`, on a fresh port and keyring, its environment
    /// cleared so no `OML_` variable of the caller's, a live key among
    /// them, reaches it, and with no `NO_COLOR`: stderr is a pipe, as under
    /// docker, kubectl or journald, and the fields must be plain there.
    fn spawn(serving: &str, owner: Option<&str>) -> Self {
        let port = free_port();
        let scratch = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "role_split_{}_{}",
            std::process::id(),
            run_tag()
        ));
        std::fs::create_dir_all(&scratch).expect("keyring scratch");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml-server-lead"));
        cmd.env_clear()
            .env("OML_POSTGRES__URL", serving)
            .env("OML_SERVER__LISTEN", format!("127.0.0.1:{port}"))
            .env("OML_KMS__KEYRING_PATH", scratch.join("keyring"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(owner) = owner {
            cmd.env("OML_POSTGRES__OWNER_URL", owner);
        }
        let mut child = cmd.spawn().expect("the oml-server-lead binary runs");
        let stderr = Arc::new(Mutex::new(String::new()));
        let mut pipe = child.stderr.take().expect("stderr is piped");
        let sink = Arc::clone(&stderr);
        std::thread::spawn(move || {
            let mut buf = [0_u8; 4096];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock()
                    .expect("stderr buffer")
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        });
        Self {
            child,
            stderr,
            port,
            scratch,
        }
    }

    fn stderr(&self) -> String {
        self.stderr.lock().expect("stderr buffer").clone()
    }

    /// Waits for the lead to exit, killing it and failing past the
    /// deadline, as a lead that served instead of refusing would.
    async fn exit(&mut self) -> ExitStatus {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().expect("poll the lead") {
                // Let the reader thread drain what the lead wrote last.
                tokio::time::sleep(Duration::from_millis(100)).await;
                return status;
            }
            if started.elapsed() > BOOT_DEADLINE {
                let _ = self.child.kill();
                panic!(
                    "the lead did not exit within {BOOT_DEADLINE:?}; stderr:\n{}",
                    self.stderr()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Waits until the lead accepts a TCP connection on its port.
    async fn listening(&mut self) {
        let started = Instant::now();
        loop {
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .is_ok()
            {
                return;
            }
            if let Some(status) = self.child.try_wait().expect("poll the lead") {
                panic!(
                    "the lead exited with {status} before it listened; stderr:\n{}",
                    self.stderr()
                );
            }
            if started.elapsed() > BOOT_DEADLINE {
                let _ = self.child.kill();
                panic!(
                    "the lead did not listen within {BOOT_DEADLINE:?}; stderr:\n{}",
                    self.stderr()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl Drop for Lead {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

/// The lead exited non-zero, logged `postgres.role_split` with `reason`
/// as the stable structured fields `event`, `code` and `reason` of a JSON
/// line (observability.md, structured logging; ADR-0042 d1), never logged that it serves,
/// and nothing accepts a connection on its port (ADR-0034 d5, d8).
async fn assert_refused(lead: &mut Lead, reason: &str) {
    let status = lead.exit().await;
    let stderr = lead.stderr();
    assert!(
        !status.success(),
        "a refused boot exits non-zero; got {status}, stderr:\n{stderr}"
    );
    let line = stderr
        .lines()
        .find(|l| l.contains("postgres.role_split"))
        .unwrap_or_else(|| panic!("no postgres.role_split line; stderr:\n{stderr}"));
    // A JSON line, the stable fields at its top level (ADR-0042 d1).
    let line: serde_json::Value = serde_json::from_str(line)
        .unwrap_or_else(|e| panic!("the refusal is a JSON line ({e}): {line}"));
    for (field, value) in [
        ("event", "role_split_refused"),
        ("code", "postgres.role_split"),
        ("reason", reason),
    ] {
        assert_eq!(
            line[field], value,
            "the refusal carries the stable field {field}; got: {line}"
        );
    }
    assert!(
        !stderr.contains("serving both rings"),
        "a refused boot never reaches the listener; stderr:\n{stderr}"
    );
    let connect = tokio::net::TcpStream::connect(("127.0.0.1", lead.port)).await;
    assert!(
        connect.is_err(),
        "no listener is bound on the refused lead's port"
    );
}

/// Whether `db` holds `_sqlx_migrations`, read through `DATABASE_URL`'s
/// own login, which created it.
async fn migrated(db: &str) -> bool {
    let mut conn = PgConnection::connect(&admin_url(db))
        .await
        .expect("connect to the throwaway database");
    sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
        .fetch_one(&mut conn)
        .await
        .expect("read the catalog")
}

// ---------------------------------------------------------------------------
// The refusals (ADR-0034 d5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_boot_with_no_owner_url_exits_with_owner_url_missing() {
    // The refusal is the configuration's, before any connection, so the
    // URL needs no database behind it.
    let mut lead = Lead::spawn(&admin_url("oml_role_split_never_connected"), None);
    assert_refused(&mut lead, "owner_url_missing").await;
}

#[tokio::test]
async fn a_boot_over_one_url_exits_with_postgres_role_split_same_role_and_binds_no_listener() {
    // One login for both URLs takes no role pair: `DATABASE_URL`'s own
    // role, in a database it created, runs this case wherever it can
    // create a database.
    let db = fresh_name();
    plain_database(&db).await;
    let url = admin_url(&db);
    let mut lead = Lead::spawn(&url, Some(&url));
    assert_refused(&mut lead, "same_role").await;
    assert!(
        !migrated(&db).await,
        "the refusal comes before migrating (ADR-0034 d5)"
    );
    drop(lead);
    drop_db(&db).await;
}

#[tokio::test]
async fn a_boot_over_an_unsplit_database_exits_with_serving_role_owns() {
    let db = fresh_name();
    if !mint_database(&db, DatabaseOwner::Serving).await {
        return;
    }
    let mut lead = Lead::spawn(&serving_url(&db), Some(&owner_url(&db)));
    assert_refused(&mut lead, "serving_role_owns").await;
    drop(lead);
    drop_db(&db).await;
}

#[tokio::test]
async fn a_boot_after_a_migration_grants_truncate_exits_with_serving_role_privileged() {
    let db = fresh_name();
    if !mint_database(&db, DatabaseOwner::Owner).await {
        return;
    }
    // A first boot migrates and serves; the grant then stands in for a
    // migration that grants `TRUNCATE`, which the second boot's checks
    // after migrating refuse.
    {
        let mut lead = Lead::spawn(&serving_url(&db), Some(&owner_url(&db)));
        lead.listening().await;
    }
    let mut owner = PgConnection::connect(&owner_url(&db))
        .await
        .expect("owner connect");
    owner
        .execute(format!("GRANT TRUNCATE ON review_queue TO {db}_serving").as_str())
        .await
        .expect("the owner grants TRUNCATE");
    owner.close().await.expect("close the owner connection");
    let mut lead = Lead::spawn(&serving_url(&db), Some(&owner_url(&db)));
    assert_refused(&mut lead, "serving_role_privileged").await;
    drop(lead);
    drop_db(&db).await;
}

// ---------------------------------------------------------------------------
// The owner connection closes before the listener binds (ADR-0034 d4, d8)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn once_the_listener_binds_pg_stat_activity_shows_no_owner_connection() {
    let db = fresh_name();
    if !mint_database(&db, DatabaseOwner::Owner).await {
        return;
    }
    let mut lead = Lead::spawn(&serving_url(&db), Some(&owner_url(&db)));
    lead.listening().await;
    // Read as the minting superuser, which sees every role's backends. A
    // closed connection's backend leaves pg_stat_activity as it exits, after
    // the client's Terminate, so the read is retried for a second before it
    // counts an owner backend as one the boot kept open.
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    let started = Instant::now();
    let (owners, servings) = loop {
        let (owners, servings): (i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE usename = $1), count(*) FILTER (WHERE usename = $2) \
             FROM pg_stat_activity WHERE datname = $3",
        )
        .bind(format!("{db}_owner"))
        .bind(format!("{db}_serving"))
        .bind(&db)
        .fetch_one(&mut admin)
        .await
        .expect("read pg_stat_activity");
        if owners == 0 || started.elapsed() > Duration::from_secs(1) {
            break (owners, servings);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        owners, 0,
        "no owner connection outlives the boot (ADR-0034 d4, d8)"
    );
    assert!(
        servings > 0,
        "the serving pool's connections show, so the read sees the lead's backends"
    );
    let stderr = lead.stderr();
    let line: serde_json::Value = stderr
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|l| l["event"] == "role_split_booted")
        .unwrap_or_else(|| panic!("no role_split_booted line; stderr:\n{stderr}"));
    for (field, value) in [
        ("owner_role", format!("{db}_owner")),
        ("serving_role", format!("{db}_serving")),
    ] {
        assert_eq!(
            line[field], value,
            "the boot logs both role names as fields (ADR-0034 d4); {field} in: {line}"
        );
    }
    drop(lead);
    drop(admin);
    drop_db(&db).await;
}
