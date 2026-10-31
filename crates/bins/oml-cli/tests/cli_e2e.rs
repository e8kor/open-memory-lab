//! End to end: the real lead composition served over TCP (phase-4 harness
//! pattern: throwaway database, `lead_state`, one router) driven only through
//! the `oml` binary — bootstrap → issue → submit → review → approve → recall
//! → verify-chain refused by name while no chain export is served (the
//! verification vectors live in `vectors.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::SigningKey;
use serde_json::Value;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};

use oml_pg::PgStore;
use oml_server_lead::{lead_state, pg_ping};

const ENROLLMENT: &str = "enr_cli_e2e_fixture_token";

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

fn db_url(name: &str) -> String {
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{prefix}/{name}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
    }
}

/// Runs the `oml` binary; returns (exit code, parsed stdout JSON, stderr).
fn oml(server: &str, creds: &Path, key: Option<&str>, args: &[&str]) -> (i32, Value, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
    cmd.arg("--server").arg(server).arg("--json");
    if let Some(k) = key {
        cmd.arg("--key").arg(k);
    }
    cmd.args(args);
    cmd.env("OML_CLI_CREDENTIALS", creds);
    // The ambient environment never leaks into the fixture.
    cmd.env_remove("OML_CLI_KEY");
    cmd.env_remove("OML_CLI_SERVER");
    cmd.env_remove("OML_CLI_SIGNING_KEY");
    let out = cmd.output().expect("the oml binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
    (
        out.status.code().unwrap_or(-1),
        parsed,
        format!(
            "stdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_ceremony_through_the_cli() {
    // -- the real server over TCP -------------------------------------------
    let db_name = format!(
        "oml_test_cli_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst)
    );
    let mut admin_conn = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin_conn
        .execute(format!("CREATE DATABASE {db_name}").as_str())
        .await
        .expect("create throwaway database");
    let store = PgStore::connect(&db_url(&db_name)).await.expect("connect");
    store.migrate().await.expect("embedded migrations apply");

    let mut seed = [0_u8; 32];
    getrandom::getrandom(&mut seed).expect("OS randomness");
    let signing = SigningKey::from_bytes(&seed);
    let pubkey_hex = hex::encode(signing.verifying_key().to_bytes());

    let router = lead_state(
        &store,
        signing,
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral bind");
    let addr = listener.local_addr().expect("local addr");
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    let server = format!("http://{addr}");

    let creds_dir = std::env::temp_dir().join(format!("oml-cli-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&creds_dir);
    let creds: PathBuf = creds_dir.join("credentials");

    // -- bootstrap claim: show-once key, credentials stored 0600 -------------
    let (code, claim, log) = oml(
        &server,
        &creds,
        None,
        &[
            "bootstrap",
            "claim",
            "--token",
            ENROLLMENT,
            "--tenant-name",
            "cli-tenant",
            "--principal",
            "platform/admin",
        ],
    );
    assert_eq!(code, 0, "bootstrap claim: {log}");
    let tenant_id = claim["tenant_id"].as_str().expect("tenant id").to_owned();
    let root = claim["root_workarea_id"].as_str().expect("root workarea");
    assert!(claim["value"].as_str().expect("value").starts_with("oml_"));
    let stored = std::fs::read_to_string(&creds).expect("credentials written");
    assert!(stored.contains(&tenant_id), "tenant recorded: {stored}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&creds)
            .expect("credentials exist")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "credentials are owner-only");
    }

    // A second claim is refused: the token is consumed (exit 1, typed).
    let (code, envelope, log) = oml(
        &server,
        &creds,
        None,
        &["bootstrap", "claim", "--token", ENROLLMENT],
    );
    assert_eq!(code, 1, "consumed token is a typed refusal: {log}");
    assert_eq!(envelope["error"]["code"], "auth.invalid", "{log}");

    // -- issue the crew's keys under the stored admin credentials ------------
    let issue = |role: &str, principal: &str| {
        let (code, response, log) = oml(
            &server,
            &creds,
            None,
            &[
                "keys",
                "issue",
                "--role",
                role,
                "--grant",
                &format!("{root}:{role}"),
                "--principal",
                principal,
            ],
        );
        assert_eq!(code, 0, "issue {role}: {log}");
        let field = |name: &str| response[name].as_str().expect(name).to_owned();
        (field("value"), field("key_id"))
    };
    let (writer, writer_id) = issue("writer", "crew/agent-1");
    let (dreamer, _) = issue("dreamer", "crew/reviewer-1");
    let (reader, _) = issue("reader", "crew/recall-1");

    // The two-key rule travels through the CLI as a typed refusal.
    let (code, envelope, log) = oml(
        &server,
        &creds,
        None,
        &[
            "keys",
            "issue",
            "--role",
            "admin",
            "--grant",
            &format!("{root}:dreamer"),
        ],
    );
    assert_eq!(code, 1, "{log}");
    assert_eq!(envelope["error"]["code"], "access.role_exclusive", "{log}");

    // -- submit → review → approve → recall ----------------------------------
    let (code, accepted, log) = oml(
        &server,
        &creds,
        Some(&writer),
        &[
            "submit",
            "--claim",
            "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery.",
            "--claim-key",
            "supplier-quote-am-7741",
        ],
    );
    assert_eq!(code, 0, "submit: {log}");
    assert_eq!(accepted["state"], "held", "{log}");
    let artifact = accepted["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();

    let (code, serve, log) = oml(&server, &creds, Some(&dreamer), &["review", "next"]);
    assert_eq!(code, 0, "review next: {log}");
    assert_eq!(
        serve["item"]["artifact_id"], accepted["artifact_id"],
        "{log}"
    );

    let (code, verdict, log) = oml(
        &server,
        &creds,
        Some(&dreamer),
        &["review", "verdict", "--memory", &artifact, "--approve"],
    );
    assert_eq!(code, 0, "verdict: {log}");
    assert_eq!(verdict["state"], "promoted", "{log}");

    let (code, recall, log) = oml(
        &server,
        &creds,
        Some(&reader),
        &["recall", "--query", "Accra Metals"],
    );
    assert_eq!(code, 0, "recall: {log}");
    let hits = recall["hits"].as_array().expect("hits");
    assert!(
        hits.iter()
            .any(|h| h["memory"]["artifact_id"] == accepted["artifact_id"]),
        "recall finds the promoted claim: {recall}"
    );

    // -- verify-chain: the chain export is the one verification surface -----
    // The events tail is a filtered feed (ADR-0025 d15), so live
    // verification refuses by name while the server serves no chain export.
    let (code, _, log) = oml(
        &server,
        &creds,
        Some(&reader),
        &[
            "audit",
            "verify-chain",
            "--genesis",
            "--signing-key",
            &pubkey_hex,
        ],
    );
    assert_eq!(code, 2, "verify-chain: {log}");
    assert!(log.contains("chain export"), "{log}");
    assert!(log.contains(tenant_id.as_str()), "{log}");

    // Without a key set the verifier refuses locally (exit 2): the chain
    // cannot vouch for its own key (chain-format.md key authentication).
    let (code, _, log) = oml(&server, &creds, Some(&reader), &["audit", "verify-chain"]);
    assert_eq!(code, 2, "{log}");

    // -- doctor ---------------------------------------------------------------
    let (code, doctor, log) = oml(&server, &creds, Some(&reader), &["doctor"]);
    assert_eq!(code, 0, "doctor: {log}");
    assert_eq!(doctor["verdict"], "ok", "{log}");
    assert!(doctor["chain_head_seq"].as_u64().expect("head") >= 6);

    // -- tenants create is honestly out of the PoC slice ---------------------
    let (code, _, log) = oml(&server, &creds, None, &["tenants", "create", "aurora"]);
    assert_eq!(code, 2, "operator tier out of slice: {log}");

    // -- deny set: a global per-key denial wins over the key's grant ----------
    let (code, denial, log) = oml(
        &server,
        &creds,
        None,
        &["deny", "set", &writer_id, root, "--reason", "incident 44"],
    );
    assert_eq!(code, 0, "deny set: {log}");
    assert_eq!(denial["workarea_node_id"].as_str(), Some(root), "{log}");
    assert!(denial["epoch"].as_u64().is_some(), "{log}");
    let (code, envelope, log) = oml(
        &server,
        &creds,
        Some(&writer),
        &["submit", "--claim", "written after the denial"],
    );
    assert_eq!(code, 1, "{log}");
    assert_eq!(envelope["error"]["code"], "access.not_found", "{log}");

    server_task.abort();
    let _ = std::fs::remove_dir_all(&creds_dir);
    drop_db(&db_name).await;
}
