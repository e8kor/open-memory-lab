//! The CLI's guardrail flags through the real binary (proposed ADR-0037 d4,
//! d10; d18 Q8): `--guardrail-scope` on `oml keys issue` and
//! `oml keys issue-agent`, and `--severity` and `--rule` on `oml write`,
//! against a fixture that answers with a canned body and records what the
//! binary sent. The fixture is a test double, not a served route; the route
//! itself is proven in `oml-server-lead/tests/guardrails_http.rs`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use serde_json::{Value, json};

/// An empty directory of this test's own, under the OS temp directory.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-gr-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// A fixture answering `POST /v2/keys` and `POST /v1/memories` with a
/// canned `201` and `202`, recording each body; its URL and what it saw.
async fn fixture() -> (String, Seen) {
    let seen: Seen = Arc::default();
    async fn keys(State(seen): State<Seen>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
        seen.lock()
            .expect("lock")
            .push(("/v2/keys".to_owned(), body.clone()));
        (
            StatusCode::CREATED,
            Json(json!({
                "key_id": "00000000-0000-4000-8000-000000000001",
                "name": body["name"],
                "role": body["role"],
                "principal": body["principal"],
                "held_by": "agent",
                "guardrail_scope": body.get("guardrail_scope").cloned().unwrap_or(json!("ancestors")),
                "default_workarea_id": "00000000-0000-4000-8000-000000000007",
                "grants": [],
                "denials": [],
                "epoch": 1,
                "value": "oml_fixture_value",
            })),
        )
    }
    async fn memories(
        State(seen): State<Seen>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        seen.lock()
            .expect("lock")
            .push(("/v1/memories".to_owned(), body));
        (
            StatusCode::ACCEPTED,
            Json(json!({
                "artifact_id": "00000000-0000-4000-8000-0000000000a1",
                "claim_key": "auto-0123456789abcdef",
                "version": 1,
                "state": "held",
                "plaintext_digest": "sha256:00",
                "workarea_id": "00000000-0000-4000-8000-000000000007",
                "journal_seq": 9,
            })),
        )
    }
    let router = axum::Router::new()
        .route("/v2/keys", post(keys))
        .route("/v1/memories", post(memories))
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

fn oml(url: &str, dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
    for name in ["OML_URL", "OML_API_KEY", "OML_CLI_KEY", "OML_CLI_SERVER"] {
        cmd.env_remove(name);
    }
    cmd.env("OML_CLI_CREDENTIALS", dir.join("credentials"))
        .args(["--url", url, "--api-key", "oml_fixture_admin", "--json"])
        .args(args)
        .output()
        .expect("the oml binary runs")
}

fn bodies(seen: &Seen, path: &str) -> Vec<Value> {
    seen.lock()
        .expect("lock")
        .iter()
        .filter(|(p, _)| p == path)
        .map(|(_, b)| b.clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keys_issue_and_issue_agent_send_the_guardrail_scope_they_are_given() {
    let (url, seen) = fixture().await;
    let dir = scratch("scope");
    let out = oml(
        &url,
        &dir,
        &[
            "keys",
            "issue",
            "narrow",
            "--role",
            "reader",
            "--grant",
            "metals:reader",
            "--guardrail-scope",
            "workarea",
        ],
    );
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
            "issue-agent",
            "agent-7",
            "--principal",
            "crew/agent-7",
            "--workarea",
            "metals",
            "--guardrail-scope",
            "ancestors",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sent = bodies(&seen, "/v2/keys");
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0]["guardrail_scope"], "workarea", "{}", sent[0]);
    assert_eq!(sent[1]["guardrail_scope"], "ancestors", "{}", sent[1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keys_issue_sends_no_guardrail_scope_when_none_is_given() {
    let (url, seen) = fixture().await;
    let dir = scratch("no-scope");
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
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sent = bodies(&seen, "/v2/keys");
    assert_eq!(sent.len(), 1, "{sent:?}");
    // The server applies the issuance default; the CLI invents none (d4).
    assert!(sent[0].get("guardrail_scope").is_none(), "{}", sent[0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guardrail_scope_other_than_workarea_or_ancestors_is_refused_before_anything_is_sent() {
    let (url, seen) = fixture().await;
    let dir = scratch("off");
    let out = oml(
        &url,
        &dir,
        &[
            "keys",
            "issue",
            "off",
            "--role",
            "reader",
            "--grant",
            "metals:reader",
            "--guardrail-scope",
            "off",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(bodies(&seen, "/v2/keys").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_sends_kind_guardrail_with_its_severity_and_rule() {
    let (url, seen) = fixture().await;
    let dir = scratch("write");
    let rule = r#"{"rule":"oml-gr/1","tool":"shell","command":["git","push","*"],"effect":"deny"}"#;
    let out = oml(
        &url,
        &dir,
        &[
            "write",
            "Never push to main.",
            "--kind",
            "guardrail",
            "--severity",
            "never",
            "--rule",
            rule,
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sent = bodies(&seen, "/v1/memories");
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["kind"], "guardrail");
    assert_eq!(sent[0]["severity"], "never");
    assert_eq!(
        sent[0]["rule"],
        serde_json::from_str::<Value>(rule).expect("the rule is JSON")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rule_that_is_not_json_is_refused_before_anything_is_sent() {
    let (url, seen) = fixture().await;
    let dir = scratch("bad-rule");
    let out = oml(
        &url,
        &dir,
        &[
            "write",
            "Never push to main.",
            "--kind",
            "guardrail",
            "--severity",
            "never",
            "--rule",
            "git push",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(bodies(&seen, "/v1/memories").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
