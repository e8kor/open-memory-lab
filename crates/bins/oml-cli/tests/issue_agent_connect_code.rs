//! `oml keys issue-agent --connect-code` through the real binary
//! (cli/reference.md `--connect-code`; ADR-0031 d7): its local refusal with
//! no server behind it, and its request and its answer to a fresh and a
//! replayed `201` against a fixture that answers `POST /v2/keys` with a
//! canned body and records what the binary sent. The fixture is a test
//! double, not a served route; the end-to-end tests against the lead follow
//! ADR-0039.

use std::io::ErrorKind;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::routing::post;
use serde_json::{Value, json};

/// An empty directory of this test's own, under the OS temp directory.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

#[test]
fn out_with_connect_code_is_refused_with_exit_2_before_anything_is_sent() {
    let dir = scratch("connect-code-out");
    let listener = TcpListener::bind("127.0.0.1:0").expect("a listener on an ephemeral port");
    listener.set_nonblocking(true).expect("non-blocking");
    let server = format!("http://{}", listener.local_addr().expect("its address"));
    let path = dir.join("crew-claude-mobile");

    let out = Command::new(env!("CARGO_BIN_EXE_oml"))
        .env("OML_CLI_CREDENTIALS", dir.join("credentials.toml"))
        .env_remove("OML_CLI_KEY")
        .env_remove("OML_CLI_SERVER")
        .args(["--url", &server, "--api-key", "oml_test_admin"])
        .args([
            "keys",
            "issue-agent",
            "claude-mobile",
            "--principal",
            "crew/claude-mobile",
            "--workarea",
            "metals",
            "--connect-code",
            "--out",
        ])
        .arg(&path)
        .output()
        .expect("the oml binary runs");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(2), "a local refusal: {stderr}");
    assert!(out.stdout.is_empty(), "nothing on stdout: {stderr}");
    assert!(
        stderr.contains("--out") && stderr.contains("--connect-code"),
        "the refusal names both flags: {stderr}"
    );
    assert!(!path.exists(), "no key file is created: {stderr}");
    match listener.accept() {
        Err(e) if e.kind() == ErrorKind::WouldBlock => {}
        Ok(_) => panic!("the CLI connected to the server: {stderr}"),
        Err(e) => panic!("the listener failed: {e}"),
    }
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

/// What the fixture answers and what it saw.
#[derive(Clone)]
struct Fixture {
    answer: Value,
    replay: bool,
    seen: Arc<Mutex<Vec<Value>>>,
}

async fn issue(
    State(fixture): State<Fixture>,
    Json(body): Json<Value>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    fixture.seen.lock().expect("the record").push(body);
    let mut headers = HeaderMap::new();
    if fixture.replay {
        headers.insert("Idempotency-Replay", HeaderValue::from_static("true"));
    }
    (StatusCode::CREATED, headers, Json(fixture.answer))
}

/// A `201` of `POST /v2/keys` with `connect_code: true`, as openapi.yaml
/// `IssuedKey` and `ConnectCode` shape it, whose connector URL names a host
/// other than the fixture's own address; a replay omits the code.
fn issued(code: Option<&str>) -> Value {
    let mut connect_code = json!({
        "code_id": "cc_01J9ZK",
        "expires_at": "2026-11-04T10:10:00Z",
        "connector_url": "https://connector.example.org/mcp",
    });
    if let Some(code) = code {
        connect_code["code"] = json!(code);
    }
    json!({
        "key_id": "key_01J9ZK",
        "name": "claude-mobile",
        "role": "writer",
        "principal": "crew/claude-mobile",
        "default_workarea_id": "metals",
        "grants": [],
        "connect_code": connect_code,
    })
}

/// Runs `oml keys issue-agent claude-mobile --connect-code` against a fixture
/// answering `answer`, with `Idempotency-Replay: true` when `replay`, and
/// returns the binary's output and every body the fixture received.
fn issue_against_fixture(test: &str, answer: Value, replay: bool) -> (Output, Vec<Value>) {
    let dir = scratch(test);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fixture = Fixture {
        answer,
        replay,
        seen: Arc::clone(&seen),
    };
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("a listener on an ephemeral port");
    let server = format!("http://{}", listener.local_addr().expect("its address"));
    let router = axum::Router::new()
        .route("/v2/keys", post(issue))
        .with_state(fixture);
    runtime.spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });

    let out = Command::new(env!("CARGO_BIN_EXE_oml"))
        .env("OML_CLI_CREDENTIALS", dir.join("credentials.toml"))
        .env_remove("OML_CLI_KEY")
        .env_remove("OML_CLI_SERVER")
        .args(["--url", &server, "--api-key", "oml_test_admin"])
        .args([
            "keys",
            "issue-agent",
            "claude-mobile",
            "--principal",
            "crew/claude-mobile",
            "--workarea",
            "metals",
            "--connect-code",
        ])
        .output()
        .expect("the oml binary runs");
    runtime.shutdown_background();
    std::fs::remove_dir_all(&dir).expect("cleaned up");
    let seen = seen.lock().expect("the record").clone();
    (out, seen)
}

#[test]
fn a_connect_code_issuance_sends_connect_code_true_and_prints_the_servers_connector_url() {
    let (out, seen) =
        issue_against_fixture("connect-code-fresh", issued(Some("7Q4M-K2XP-9D")), false);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(seen.len(), 1, "one POST /v2/keys: {stderr}");
    assert_eq!(
        seen[0]["connect_code"],
        json!(true),
        "the request asks for a connect code: {}",
        seen[0]
    );
    assert_eq!(out.status.code(), Some(0), "a fresh 201: {stderr}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "https://connector.example.org/mcp\n7Q4M-K2XP-9D\n",
        "stdout is exactly the server's connector URL and the code: {stderr}"
    );
}

#[test]
fn a_replayed_connect_code_issuance_through_the_binary_prints_no_code_and_exits_2() {
    let (out, seen) = issue_against_fixture("connect-code-replay", issued(None), true);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(seen.len(), 1, "one POST /v2/keys: {stderr}");
    assert_eq!(
        out.status.code(),
        Some(2),
        "a replay carries no code: {stderr}"
    );
    assert!(
        out.stdout.is_empty(),
        "nothing on stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        stderr.contains("`oml keys connect-code claude-mobile`"),
        "stderr names how to mint a code: {stderr}"
    );
    assert!(
        stderr.contains("idempotent replay"),
        "the binary read the Idempotency-Replay header: {stderr}"
    );
}
