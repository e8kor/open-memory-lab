//! `--reason` and `--create` on `oml keys issue-agent`, and `--reason` on
//! `oml keys issue`, through the real binary (ADR-0097 d26, d31): each
//! verb sends the reason it is given in the issuance body, and with stdin
//! not a terminal and no reason it sends none and asks for nothing. The
//! fixture records the body and answers a canned pending issuance.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use serde_json::{Value, json};

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-rs-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

type Seen = Arc<Mutex<Vec<Value>>>;

/// A fixture answering `POST /v2/keys` with a `201` whose issuance waits
/// on a second admin person, recording each body.
async fn fixture() -> (String, Seen) {
    let seen: Seen = Arc::default();
    async fn keys(State(seen): State<Seen>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
        seen.lock().expect("lock").push(body.clone());
        (
            StatusCode::CREATED,
            Json(json!({
                "key_id": "00000000-0000-4000-8000-000000000001",
                "name": body["name"],
                "role": body["role"],
                "principal": body["principal"],
                "used_by": "agent",
                "default_workspace_id": "00000000-0000-4000-8000-000000000007",
                "grants": [],
                "denials": [],
                "epoch": 1,
                "value": "oml_fixture_value",
                "admin_act": {
                    "act_id": "00000000-0000-4000-8000-0000000000ac",
                    "state": "pending_confirmation",
                },
            })),
        )
    }
    let router = axum::Router::new()
        .route("/v2/keys", post(keys))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture bind");
    let url = format!("http://{}", listener.local_addr().expect("fixture addr"));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("fixture serve");
    });
    (url, seen)
}

/// The binary with stdin closed, so it is never a terminal.
fn oml(url: &str, dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
    for name in ["OML_URL", "OML_API_KEY", "OML_CLI_KEY", "OML_CLI_SERVER"] {
        cmd.env_remove(name);
    }
    cmd.env("OML_CLI_CREDENTIALS", dir.join("credentials"))
        .args(["--url", url, "--api-key", "oml_fixture_admin"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("the oml binary runs")
}

fn agent(extra: &[&'static str]) -> Vec<&'static str> {
    let mut args = vec![
        "keys",
        "issue-agent",
        "agent-7",
        "--principal",
        "crew/agent-7",
        "--workspace",
        "metals",
    ];
    args.extend_from_slice(extra);
    args
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_sends_its_reason_and_says_the_issuance_waits_on_another_admin() {
    let (url, seen) = fixture().await;
    let dir = scratch("agent-reason");
    let out = oml(&url, &dir, &agent(&["--reason", "the nightly digest"]));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let sent = seen.lock().expect("lock").clone();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["reason"], "the nightly digest", "{}", sent[0]);
    assert!(stderr.contains("waits on another admin"), "{stderr}");
    assert!(
        stderr.contains("oml admin confirm 00000000-0000-4000-8000-0000000000ac"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_and_issue_agent_off_a_terminal_without_a_reason_send_none_and_ask_nothing() {
    let (url, seen) = fixture().await;
    let dir = scratch("no-reason");
    let out = oml(&url, &dir, &agent(&[]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = oml(
        &url,
        &dir,
        &[
            "keys",
            "issue",
            "plain",
            "--role",
            "reader",
            "--grant",
            "metals:reader",
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(!stderr.contains("Reason for"), "{stderr}");
    let sent = seen.lock().expect("lock").clone();
    assert_eq!(sent.len(), 2, "{sent:?}");
    for body in &sent {
        assert!(body.get("reason").is_none(), "{body}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keys_issue_sends_its_reason() {
    let (url, seen) = fixture().await;
    let dir = scratch("issue-reason");
    let out = oml(
        &url,
        &dir,
        &[
            "keys",
            "issue",
            "plain",
            "--role",
            "reader",
            "--grant",
            "metals:reader",
            "--reason",
            "a reader for the audit",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sent = seen.lock().expect("lock").clone();
    assert_eq!(sent[0]["reason"], "a reader for the audit", "{}", sent[0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_with_create_sends_the_create_flag_on_its_submitter_grant_alone() {
    let (url, seen) = fixture().await;
    let dir = scratch("create");
    let out = oml(
        &url,
        &dir,
        &agent(&["--create", "--reason", "an agent that files"]),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sent = seen.lock().expect("lock").clone();
    let grants = sent[0]["grants"].as_array().expect("grants").clone();
    let submitter = grants
        .iter()
        .find(|g| g["role"] == "submitter")
        .expect("a submitter grant");
    assert_eq!(submitter["create"], true, "{grants:?}");
    let reader = grants
        .iter()
        .find(|g| g["role"] == "reader")
        .expect("a reader grant");
    assert!(reader.get("create").is_none(), "{grants:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_agent_refuses_create_on_a_reader_key_and_sends_nothing() {
    let (url, seen) = fixture().await;
    let dir = scratch("create-reader");
    let out = oml(&url, &dir, &agent(&["--create", "--role", "reader"]));
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--create needs a write grant"), "{stderr}");
    assert!(seen.lock().expect("lock").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
