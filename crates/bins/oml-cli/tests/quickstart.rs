//! `oml quickstart` through the real binary (ADR-0041 d9). A fake `docker`
//! first on `PATH` records each call and prints an enrollment token for
//! `compose logs`, and the real lead composition stands in for the stack
//! compose would start, served over TCP on the port the verb is given. The
//! compose file itself runs in CI's `compose` job, where Docker is.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::SigningKey;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};

use oml_pg::PgStore;
use oml_server_lead::{lead_state, pg_ping};

/// The enrollment token the fake's `compose logs` prints and the lead takes.
const TOKEN: &str = "enr_0123456789abcdef0123456789abcdef0123456789abcdef";

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL names a throwaway PostgreSQL cluster; no suite defaults to one, so none reaches a live server")
}

const PAIR_PASSWORD: &str = "oml_test_pair";

fn role_url(db: &str, role: &str) -> String {
    let base = base_url();
    let (scheme, rest) = base
        .split_once("://")
        .expect("DATABASE_URL is a postgres:// URL");
    let (_, host_path) = rest.rsplit_once('@').expect("DATABASE_URL names its user");
    let (host, _) = host_path
        .split_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{scheme}://{role}_{db}:{PAIR_PASSWORD}@{host}/{db}")
}

fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

/// Mints `db`'s role pair and `db` (ADR-0034 d8), or `false` with `did not
/// run` printed where the role cannot, failing under
/// `OML_TEST_REQUIRE_SPLIT=1` as CI sets it.
async fn mint_database(db: &str) -> bool {
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
            .execute(format!("CREATE ROLE {role}_{db} LOGIN PASSWORD '{PAIR_PASSWORD}'").as_str())
            .await
            .expect("mint the role pair");
    }
    admin
        .execute(format!("CREATE DATABASE {db} OWNER owner_{db}").as_str())
        .await
        .expect("create throwaway database");
    true
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
        for role in ["serving", "owner"] {
            let _ = admin
                .execute(format!("DROP ROLE IF EXISTS {role}_{name}").as_str())
                .await;
        }
    }
}

static DB_N: AtomicU64 = AtomicU64::new(0);

/// An empty directory of this test's own, under the OS temp directory.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-qs-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// A fake `docker` in `dir/bin` that appends each call's arguments to
/// `dir/docker.log`, answers `compose version`, and prints [`TOKEN`] in a
/// log line for `compose … logs …`.
fn fake_docker(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("the fake's directory");
    let script = bin.join("docker");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \" $* \" in\n\
             *' compose version '*) echo 'Docker Compose version v2.29.7' ;;\n\
             *' logs '*) echo 'oml-server-1  | enrollment token (single use): {TOKEN}' ;;\n\
             esac\n",
            dir.join("docker.log").display()
        ),
    )
    .expect("the fake docker");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("executable");
    bin
}

fn quickstart(home: &Path, path: &str, stack: &Path, port: u16) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oml"))
        .env("PATH", path)
        .env("OML_CLI_CREDENTIALS", home.join("user-credentials.toml"))
        .env_remove("OML_URL")
        .env_remove("OML_API_KEY")
        .args(["quickstart", "--dir"])
        .arg(stack)
        .args(["--port", &port.to_string()])
        .output()
        .expect("the oml binary runs")
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

fn docker_log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("docker.log")).unwrap_or_default()
}

#[test]
fn quickstart_without_docker_compose_is_refused_as_quickstart_docker_and_writes_nothing() {
    let dir = scratch("no-docker");
    let stack = dir.join("oml");
    let out = quickstart(&dir, "/usr/bin/oml-no-such-dir", &stack, 18_080);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("quickstart.docker"), "{stderr}");
    assert!(!stack.exists(), "nothing is written: {stderr}");
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quickstart_brings_up_the_compose_profile_claims_the_lead_and_prints_the_agents_key_file_and_login_line()
 {
    let db = format!(
        "oml_test_qs_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db).await {
        return;
    }
    let store = PgStore::connect(&role_url(&db, "serving"))
        .await
        .expect("connect")
        .with_owner_url(&role_url(&db, "owner"));
    store
        .migrate(&role_url(&db, "owner"))
        .await
        .expect("embedded migrations apply");
    let mut seed = [0_u8; 32];
    getrandom::getrandom(&mut seed).expect("OS randomness");
    let router = lead_state(
        &store,
        SigningKey::from_bytes(&seed),
        pg_ping(store.pool().clone()),
        true,
        Some(TOKEN.to_owned()),
    )
    .into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral bind");
    let port = listener.local_addr().expect("local addr").port();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });

    let dir = scratch("up");
    let bin = fake_docker(&dir);
    let stack = dir.join("oml");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = {
        let (dir, path, stack) = (dir.clone(), path.clone(), stack.clone());
        tokio::task::spawn_blocking(move || quickstart(&dir, &path, &stack, port))
            .await
            .expect("the run ends")
    };
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stdout}{stderr}");

    // The compose profile: the image at the CLI's own version, never latest,
    // on 127.0.0.1 alone, with the owner and serving URLs from .env.
    let compose = std::fs::read_to_string(stack.join("compose.yaml")).expect("compose.yaml");
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        compose.contains(&format!("image: ghcr.io/e8kor/open-memory-lab:{version}\n")),
        "{compose}"
    );
    assert!(
        !compose.contains("latest") && !compose.contains("oml_dev_only"),
        "{compose}"
    );
    assert!(
        compose.contains(&format!("\"127.0.0.1:{port}:8080\"")),
        "{compose}"
    );
    for url in [
        "OML_POSTGRES__URL: postgres://oml_server:${OML_SERVER_PASSWORD:?",
        "OML_POSTGRES__OWNER_URL: postgres://oml_owner:${OML_OWNER_PASSWORD:?",
    ] {
        assert!(compose.contains(url), "{url}: {compose}");
    }
    let env = std::fs::read_to_string(stack.join(".env")).expect(".env");
    assert_eq!(mode(&stack.join(".env")), 0o600, ".env is owner-only");
    let passwords: Vec<&str> = env
        .lines()
        .filter_map(|l| l.split_once('=').map(|(_, v)| v))
        .collect();
    assert_eq!(passwords.len(), 3, "{env}");
    assert!(
        passwords
            .iter()
            .all(|p| p.len() == 32 && p.bytes().all(|b| b.is_ascii_hexdigit())),
        "three generated passwords"
    );
    assert!(
        passwords[0] != passwords[1]
            && passwords[1] != passwords[2]
            && passwords[0] != passwords[2],
        "each role its own password"
    );

    // The project is named after the directory's whole path, never its
    // basename alone, in the file and in every compose call.
    let name = compose
        .lines()
        .find_map(|l| l.strip_prefix("name: "))
        .expect("compose.yaml names its project")
        .to_owned();
    assert!(name.starts_with("oml-") && name.len() == 16, "{name}");
    let log = docker_log(&dir);
    let project = format!(
        "compose --project-name {name} --project-directory {}",
        stack.display()
    );
    assert!(
        log.lines()
            .any(|l| l == format!("{project} up --detach --wait")),
        "{log}"
    );
    assert!(
        log.lines()
            .any(|l| l == format!("{project} logs oml-server")),
        "{log}"
    );

    // The keys, owner-only; the agent's is a writer key on the crew workarea.
    let agent_file = stack.join("keys").join("agent");
    for name in ["agent", "reviewer"] {
        assert_eq!(mode(&stack.join("keys").join(name)), 0o600, "{name}");
    }
    assert_eq!(mode(&stack.join("keys")), 0o700, "keys/ is owner-only");
    assert_eq!(mode(&stack.join("credentials")), 0o600, "the admin pair");
    let agent = std::fs::read_to_string(&agent_file).expect("the agent key");
    let agent = agent.trim_end();
    let me: serde_json::Value = ureq::get(&format!("http://127.0.0.1:{port}/v2/whoami"))
        .set("Authorization", &format!("Bearer {agent}"))
        .call()
        .expect("whoami under the agent key")
        .into_json()
        .expect("JSON");
    assert_eq!(me["role"], "writer", "{me}");
    assert_eq!(me["default_workarea"]["path"], "universe.crew", "{me}");
    assert!(
        !dir.join("user-credentials.toml").exists(),
        "the user's own credentials file is left alone"
    );

    // What it prints: the key file and the two lines, never a key value.
    let url = format!("http://127.0.0.1:{port}");
    assert!(
        stdout.contains(&agent_file.display().to_string()),
        "{stdout}"
    );
    assert!(stdout.contains(&format!("oml login {url}")), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "oml setup claude-code --url {url} --key-file {}",
            agent_file.display()
        )),
        "{stdout}"
    );
    assert!(
        !stdout.contains(agent) && !stderr.contains(agent),
        "no key is printed"
    );

    // A second run against the stack is refused, naming the directory, and
    // runs no docker command.
    let lines = log.lines().count();
    let again = tokio::task::spawn_blocking({
        let (dir, path, stack) = (dir.clone(), path.clone(), stack.clone());
        move || quickstart(&dir, &path, &stack, port)
    })
    .await
    .expect("the run ends");
    let stderr = String::from_utf8_lossy(&again.stderr);
    assert_eq!(again.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("quickstart.exists") && stderr.contains(&stack.display().to_string()),
        "{stderr}"
    );
    assert_eq!(
        docker_log(&dir).lines().count(),
        lines,
        "no docker command ran"
    );

    server_task.abort();
    let _ = std::fs::remove_dir_all(&dir);
    drop_db(&db).await;
}
