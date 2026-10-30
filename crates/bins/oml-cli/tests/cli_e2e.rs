//! End to end: the real lead composition served over TCP (phase-4 harness
//! pattern: throwaway database, `lead_state`, one router) driven only through
//! the `oml` binary — bootstrap → issue → submit → review → approve → recall
//! → verify-chain green. A journal cannot be tampered with over HTTP
//! (append-only), so the tamper case runs the same binary against a stub
//! server replaying a hand-corrupted events document.

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

    // -- verify-chain: green under the deployment key, broken under another --
    // The events tail is reader-scope (roles never nest), so the auditor's
    // verbs run under the reader key.
    let (code, report, log) = oml(
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
    assert_eq!(code, 0, "verify-chain: {log}");
    assert_eq!(report["verified"], true, "{log}");
    assert_eq!(report["tenant_id"].as_str(), Some(tenant_id.as_str()));
    assert!(report["events"].as_u64().expect("events") >= 6, "{log}");
    assert_eq!(
        report["signing_keys"][0]["events_verified"], report["events"],
        "every event verified under the one deployment key: {log}"
    );

    let foreign = hex::encode(
        SigningKey::from_bytes(&[9_u8; 32])
            .verifying_key()
            .to_bytes(),
    );
    let (code, report, log) = oml(
        &server,
        &creds,
        Some(&reader),
        &["audit", "verify-chain", "--signing-key", &foreign],
    );
    assert_eq!(code, 1, "a foreign key set fails verification: {log}");
    assert_eq!(report["verified"], false, "{log}");

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

    // -- the tamper case: a stub replays a corrupted events document ---------
    // Pull the genuine tail, corrupt one signed field, serve it verbatim.
    let client = oml_cli::client::Client::new(server.clone(), Some(reader.clone()));
    let (_, mut doc) = client
        .get("/v2/events?after_seq=0&limit=200")
        .expect("tail fetch");
    doc["items"][2]["principal"] = Value::String("forged/principal".to_owned());
    let stub_router = axum::Router::new().route(
        "/v2/events",
        axum::routing::get(move || {
            let doc = doc.clone();
            async move { axum::Json(doc) }
        }),
    );
    let stub_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("stub bind");
    let stub_addr = stub_listener.local_addr().expect("stub addr");
    let stub_task = tokio::spawn(async move {
        axum::serve(stub_listener, stub_router)
            .await
            .expect("stub serve");
    });
    let (code, report, log) = oml(
        &format!("http://{stub_addr}"),
        &creds,
        Some(&reader),
        &["audit", "verify-chain", "--signing-key", &pubkey_hex],
    );
    assert_eq!(code, 1, "a corrupted tail fails verification: {log}");
    assert_eq!(report["verified"], false, "{log}");
    let breaks = report["breaks"].as_array().expect("breaks");
    assert!(
        breaks
            .iter()
            .any(|b| b.as_str().unwrap_or("").starts_with("step 3")),
        "the forged principal breaks the signature: {report}"
    );

    stub_task.abort();
    server_task.abort();
    let _ = std::fs::remove_dir_all(&creds_dir);
    drop_db(&db_name).await;
}
