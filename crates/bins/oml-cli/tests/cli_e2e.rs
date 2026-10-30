//! End to end: the real lead composition served over TCP (phase-4 harness
//! pattern: throwaway database, `lead_state`, one router) driven only through
//! the `oml` binary — bootstrap → issue → submit → review → approve → recall
//! → verify-chain refused by name while no chain export is served (the
//! verification vectors live in `vectors.rs`).
//! The key-tool cases (ADR-0028) run the lead behind a recorder, so a test
//! can name exactly the requests a verb sent.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
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

/// The `oml` binary pointed at `server` and the fixture's credentials file.
fn oml_command(server: &str, creds: &Path, key: Option<&str>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
    cmd.arg("--server").arg(server);
    if let Some(k) = key {
        cmd.arg("--key").arg(k);
    }
    cmd.env("OML_CLI_CREDENTIALS", creds);
    // The ambient environment never leaks into the fixture.
    cmd.env_remove("OML_CLI_KEY");
    cmd.env_remove("OML_CLI_SERVER");
    cmd.env_remove("OML_CLI_SIGNING_KEY");
    cmd
}

/// Runs the `oml` binary; returns (exit code, parsed stdout JSON, stderr).
fn oml(server: &str, creds: &Path, key: Option<&str>, args: &[&str]) -> (i32, Value, String) {
    let mut cmd = oml_command(server, creds, key);
    cmd.arg("--json").args(args);
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

// ---------------------------------------------------------------------------
// The key tool (ADR-0028; cli/reference.md `oml keys`)
// ---------------------------------------------------------------------------

/// One request the lead received, as the recorder in front of it saw it.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    body: Value,
}

type Recorder = Arc<Mutex<Vec<Seen>>>;

/// Records every request, body included, then hands it on unchanged.
async fn record(State(seen): State<Recorder>, request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("the request body reads");
    seen.lock().expect("recorder lock").push(Seen {
        method: parts.method.to_string(),
        path: parts.uri.path().to_owned(),
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    });
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

/// The real lead over TCP behind the recorder, its tenant bootstrap-claimed
/// through the CLI so the stored credentials hold the first admin key.
struct Lab {
    server: String,
    dir: PathBuf,
    creds: PathBuf,
    root: String,
    seen: Recorder,
    db_name: String,
    task: tokio::task::JoinHandle<()>,
}

impl Lab {
    async fn start() -> Self {
        let n = DB_N.fetch_add(1, Ordering::SeqCst);
        let db_name = format!("oml_test_cli_{}_{n}", std::process::id());
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
        let seen: Recorder = Arc::new(Mutex::new(Vec::new()));
        let router = lead_state(
            &store,
            SigningKey::from_bytes(&seed),
            pg_ping(store.pool().clone()),
            true,
            Some(ENROLLMENT.to_owned()),
        )
        .into_router()
        .layer(middleware::from_fn_with_state(seen.clone(), record));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral bind");
        let addr = listener.local_addr().expect("local addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });
        let server = format!("http://{addr}");

        let dir = std::env::temp_dir().join(format!("oml-cli-e2e-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture directory");
        let creds = dir.join("credentials");
        let (code, claim, log) = oml(
            &server,
            &creds,
            None,
            &[
                "bootstrap",
                "claim",
                "--token",
                ENROLLMENT,
                "--principal",
                "platform/admin",
            ],
        );
        assert_eq!(code, 0, "bootstrap claim: {log}");
        let root = claim["root_workarea_id"]
            .as_str()
            .expect("root workarea")
            .to_owned();
        Self {
            server,
            dir,
            creds,
            root,
            seen,
            db_name,
            task,
        }
    }

    /// `oml --json <args>` under the stored admin credentials, or `key`.
    fn json(&self, key: Option<&str>, args: &[&str]) -> (i32, Value, String) {
        oml(&self.server, &self.creds, key, args)
    }

    /// `oml <args>` under the stored admin credentials, without `--json`;
    /// returns (exit code, stdout, stderr) as the operator's shell gets them.
    fn text(&self, args: &[&str]) -> (i32, String, String) {
        let out = oml_command(&self.server, &self.creds, None)
            .args(args)
            .output()
            .expect("the oml binary runs");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// A fresh child of the root.
    fn workarea(&self, name: &str) -> String {
        let (code, created, log) = self.json(None, &["workareas", "create", &self.root, name]);
        assert_eq!(code, 0, "workareas create: {log}");
        created["id"].as_str().expect("workarea id").to_owned()
    }

    /// `oml keys issue <args>` under the admin credentials; the `201` body.
    fn issue(&self, args: &[&str]) -> Value {
        let mut all = vec!["keys", "issue"];
        all.extend_from_slice(args);
        let (code, issued, log) = self.json(None, &all);
        assert_eq!(code, 0, "keys issue: {log}");
        issued
    }

    /// Every request the lead received since the last call, in order.
    fn take_requests(&self) -> Vec<Seen> {
        std::mem::take(&mut *self.seen.lock().expect("recorder lock"))
    }

    async fn stop(self) {
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
        drop_db(&self.db_name).await;
    }
}

fn routes(seen: &[Seen]) -> Vec<String> {
    seen.iter()
        .map(|s| format!("{} {}", s.method, s.path))
        .collect()
}

fn grant(workarea: &str, role: &str) -> Value {
    json!({ "workarea_node_id": workarea, "role": role })
}

/// The (workarea, role) pairs a `201` lists, in its order.
fn granted(issued: &Value) -> Vec<(String, String)> {
    issued["grants"]
        .as_array()
        .expect("grants")
        .iter()
        .map(|g| {
            (
                g["workarea_node_id"].as_str().expect("node").to_owned(),
                g["role"].as_str().expect("role").to_owned(),
            )
        })
        .collect()
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .expect("the file exists")
        .permissions()
        .mode()
        & 0o777
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keys_issue_with_one_writer_grant_sends_a_reader_grant_on_the_same_workarea() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    lab.take_requests();

    let issued = lab.issue(&[
        "--role",
        "writer",
        "--principal",
        "crew/agent-1",
        "--grant",
        &format!("{wa}:writer"),
    ]);

    let seen = lab.take_requests();
    assert_eq!(routes(&seen), ["POST /v2/keys"]);
    assert_eq!(
        seen[0].body["grants"],
        json!([grant(&wa, "writer"), grant(&wa, "reader")]),
        "the reader grant rides beside the writer grant in the one request"
    );
    assert_eq!(
        granted(&issued),
        [
            (wa.clone(), "writer".to_owned()),
            (wa.clone(), "reader".to_owned())
        ]
    );
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keys_issue_with_write_only_sends_only_the_writer_grant() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    lab.take_requests();

    let issued = lab.issue(&[
        "--role",
        "writer",
        "--principal",
        "crew/agent-1",
        "--grant",
        &format!("{wa}:writer"),
        "--write-only",
    ]);

    let seen = lab.take_requests();
    assert_eq!(routes(&seen), ["POST /v2/keys"]);
    assert_eq!(seen[0].body["grants"], json!([grant(&wa, "writer")]));
    assert_eq!(granted(&issued), [(wa.clone(), "writer".to_owned())]);
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_sends_one_post_v2_keys_and_no_other_request() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    lab.take_requests();

    let (code, _, stderr) = lab.text(&[
        "keys",
        "issue-agent",
        "--principal",
        "crew/agent-7",
        "--workarea",
        &wa,
    ]);
    assert_eq!(code, 0, "{stderr}");

    let seen = lab.take_requests();
    assert_eq!(routes(&seen), ["POST /v2/keys"]);
    assert_eq!(
        seen[0].body,
        json!({
            "role": "writer",
            "held_by": "agent",
            "principal": "crew/agent-7",
            "default_workarea_id": wa,
            "grants": [grant(&wa, "writer"), grant(&wa, "reader")],
        })
    );
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_prints_exactly_the_oml_url_and_oml_api_key_lines() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");

    let (code, stdout, stderr) = lab.text(&[
        "keys",
        "issue-agent",
        "--principal",
        "crew/agent-7",
        "--workarea",
        &wa,
    ]);
    assert_eq!(code, 0, "{stderr}");
    let value = stdout
        .lines()
        .nth(1)
        .and_then(|l| l.strip_prefix("export OML_API_KEY='"))
        .and_then(|l| l.strip_suffix('\''))
        .expect("the second line exports the key value")
        .to_owned();
    assert!(
        value.starts_with("oml_") && value.len() == 47,
        "an oml_ key value: {stdout}"
    );
    assert_eq!(
        stdout,
        format!(
            "export OML_URL='{}'\nexport OML_API_KEY='{value}'\n",
            lab.server
        )
    );
    assert!(
        !stderr.contains(&value),
        "the value is printed once: {stderr}"
    );

    // The exported value is the key itself, and it writes to the workarea.
    let (code, accepted, log) = lab.json(
        Some(&value),
        &["submit", "--claim", "The exported key remembers."],
    );
    assert_eq!(code, 0, "{log}");
    assert_eq!(accepted["state"], "held", "{log}");
    assert_eq!(accepted["workarea_id"].as_str(), Some(wa.as_str()), "{log}");
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_out_writes_the_key_to_a_0600_file_and_prints_no_key_value() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    let path = lab.dir.join("keys").join("crew-agent-7");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("key directory");
    let shown = path.to_str().expect("a UTF-8 path");

    let (code, stdout, stderr) = lab.text(&[
        "keys",
        "issue-agent",
        "--principal",
        "crew/agent-7",
        "--workarea",
        &wa,
        "--out",
        shown,
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        format!(
            "export OML_URL='{}'\nexport OML_API_KEY=\"$(cat '{shown}')\"\n",
            lab.server
        )
    );
    let written = std::fs::read_to_string(&path).expect("the key file");
    let value = written
        .strip_suffix('\n')
        .expect("the value and one newline");
    assert!(
        value.starts_with("oml_") && value.len() == 47,
        "an oml_ key value: {written:?}"
    );
    #[cfg(unix)]
    assert_eq!(mode_of(&path), 0o600, "the key file is owner-only");
    assert!(!stdout.contains(value), "no value on stdout: {stdout}");
    assert!(!stderr.contains(value), "no value on stderr: {stderr}");

    // The file holds the key itself.
    let (code, accepted, log) = lab.json(
        Some(value),
        &["submit", "--claim", "The key from the file remembers."],
    );
    assert_eq!(code, 0, "{log}");
    assert_eq!(accepted["state"], "held", "{log}");
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_out_refuses_an_existing_file_and_sends_nothing() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    let path = lab.dir.join("unrelated");
    std::fs::write(&path, "an unrelated file\n").expect("an existing file");
    lab.take_requests();

    let (code, stdout, stderr) = lab.text(&[
        "keys",
        "issue-agent",
        "--principal",
        "crew/agent-7",
        "--workarea",
        &wa,
        "--out",
        path.to_str().expect("a UTF-8 path"),
    ]);
    assert_eq!(code, 2, "a local refusal: {stdout}{stderr}");
    assert!(
        stderr.contains("already exists"),
        "the reason is named: {stderr}"
    );
    assert!(stdout.is_empty(), "{stdout}");
    assert!(lab.take_requests().is_empty(), "nothing was sent");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file stays"),
        "an unrelated file\n",
        "the existing file is untouched"
    );
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_out_with_json_is_refused() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    let path = lab.dir.join("crew-agent-7");
    lab.take_requests();

    let (code, _, log) = lab.json(
        None,
        &[
            "keys",
            "issue-agent",
            "--principal",
            "crew/agent-7",
            "--workarea",
            &wa,
            "--out",
            path.to_str().expect("a UTF-8 path"),
        ],
    );
    assert_eq!(code, 2, "the 201 body carries the value: {log}");
    assert!(log.contains("--json"), "the reason is named: {log}");
    assert!(lab.take_requests().is_empty(), "nothing was sent: {log}");
    assert!(!path.exists(), "no key file is created: {log}");
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_out_refuses_a_path_with_a_quote_or_a_newline_and_sends_nothing() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    lab.take_requests();

    for name in ["it's", "say-\"hi\"", "two\nlines"] {
        let path = lab.dir.join(name);
        let (code, stdout, stderr) = lab.text(&[
            "keys",
            "issue-agent",
            "--principal",
            "crew/agent-7",
            "--workarea",
            &wa,
            "--out",
            path.to_str().expect("a UTF-8 path"),
        ]);
        assert_eq!(code, 2, "{name:?}: {stdout}{stderr}");
        assert!(
            stderr.contains("a quote or a newline"),
            "{name:?}: the reason is named: {stderr}"
        );
        assert!(!path.exists(), "{name:?}: no key file is created");
    }
    assert!(lab.take_requests().is_empty(), "nothing was sent");
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_writer_key_issued_without_write_only_recalls_promoted_memories_on_its_workarea() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    let writer = lab.issue(&[
        "--role",
        "writer",
        "--principal",
        "crew/agent-1",
        "--grant",
        &format!("{wa}:writer"),
    ])["value"]
        .as_str()
        .expect("writer value")
        .to_owned();
    let dreamer = lab.issue(&[
        "--role",
        "dreamer",
        "--held-by",
        "human",
        "--principal",
        "crew/reviewer-1",
        "--grant",
        &format!("{wa}:dreamer"),
    ])["value"]
        .as_str()
        .expect("dreamer value")
        .to_owned();

    let (code, accepted, log) = lab.json(
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
    let artifact = accepted["artifact_id"].as_str().expect("artifact id");
    let (code, serve, log) = lab.json(Some(&dreamer), &["review", "next"]);
    assert_eq!(code, 0, "review next: {log}");
    assert_eq!(
        serve["item"]["artifact_id"].as_str(),
        Some(artifact),
        "{log}"
    );
    let (code, verdict, log) = lab.json(
        Some(&dreamer),
        &["review", "verdict", "--memory", artifact, "--approve"],
    );
    assert_eq!(code, 0, "verdict: {log}");
    assert_eq!(verdict["state"], "promoted", "{log}");

    let (code, recall, log) = lab.json(Some(&writer), &["recall", "--query", "Accra Metals"]);
    assert_eq!(code, 0, "the writer key recalls: {log}");
    assert!(
        recall["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .any(|h| h["memory"]["artifact_id"].as_str() == Some(artifact)),
        "the writer key's own recall finds the promoted claim: {recall}"
    );
    lab.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_writer_key_issued_with_write_only_is_refused_recall_but_still_reads_its_own_held_row() {
    let lab = Lab::start().await;
    let wa = lab.workarea("metals");
    let writer = lab.issue(&[
        "--role",
        "writer",
        "--principal",
        "crew/agent-1",
        "--grant",
        &format!("{wa}:writer"),
        "--write-only",
    ])["value"]
        .as_str()
        .expect("writer value")
        .to_owned();

    let (code, accepted, log) = lab.json(
        Some(&writer),
        &[
            "submit",
            "--claim",
            "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery.",
        ],
    );
    assert_eq!(code, 0, "submit: {log}");
    assert_eq!(accepted["state"], "held", "{log}");
    let artifact = accepted["artifact_id"].as_str().expect("artifact id");

    let (code, envelope, log) = lab.json(Some(&writer), &["recall", "--query", "Accra Metals"]);
    assert_eq!(code, 1, "recall needs a reader grant: {log}");
    assert_eq!(envelope["error"]["code"], "access.not_found", "{log}");

    // Own work needs no grant (roles-and-keys.md "Own", ADR-0025 d3).
    let client = oml_cli::client::Client::new(lab.server.clone(), Some(writer.clone()));
    let (status, page) = client
        .get("/v1/memories?own=true")
        .expect("the own-work listing");
    assert_eq!(status, 200);
    assert!(
        page["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|m| m["artifact_id"].as_str() == Some(artifact) && m["state"] == "held"),
        "the held row is listed as the key's own: {page}"
    );
    let (status, row) = client
        .get(&format!("/v1/memories/{artifact}"))
        .expect("get of the key's own held row");
    assert_eq!(status, 200);
    assert_eq!(row["state"], "held", "{row}");
    lab.stop().await;
}
