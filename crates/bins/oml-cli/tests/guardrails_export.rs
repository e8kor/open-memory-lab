//! `oml guardrails show` and `oml guardrails export` through the real
//! binary (ADR-0037 d7, d11; build step S5), against a fixture
//! that answers `GET /v1/guardrails` with a canned set it can change. The
//! fixture is a test double, not a served route.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::routing::get;
use serde_json::{Value, json};

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-gx-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn served(id: &str, claim_key: &str, text: &str) -> Value {
    json!({
        "artifact_id": id, "claim_key": claim_key, "version": 1,
        "workarea_id": "00000000-0000-4000-8000-0000000000e1",
        "workarea_path": "universe.eng", "depth": 1,
        "severity": "must", "text": text,
        "rule": {"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"], "effect": "deny"},
        "tokens": 5, "promoted_at": "2026-10-31T10:00:00.000000Z",
        "verdicts": [{"held_by": "human", "at": "2026-10-31T10:00:00.000000Z"}],
        "conflict": []
    })
}

fn set(digest: &str, guardrails: Vec<Value>) -> Value {
    let over = guardrails.len() > 12;
    json!({
        "scope_node": "00000000-0000-4000-8000-0000000000e2",
        "scope_path": "universe.eng.api",
        "guardrail_scope": "ancestors",
        "set_digest": digest,
        "over_cap": over,
        "guardrails": guardrails,
        "own_held": [{"claim_key": "lint-first", "version": 1}]
    })
}

fn the_set() -> Value {
    set(
        "sha256:aaaa",
        vec![
            served(
                "00000000-0000-4000-8000-0000000000d1",
                "no-push",
                "Never push to main.",
            ),
            served(
                "00000000-0000-4000-8000-0000000000d2",
                "tests-first",
                "Run the tests first.",
            ),
        ],
    )
}

type Shared = Arc<Mutex<Value>>;

async fn fixture(initial: Value) -> (String, Shared) {
    let shared: Shared = Arc::new(Mutex::new(initial));
    async fn guardrails(State(set): State<Shared>) -> Json<Value> {
        Json(set.lock().expect("lock").clone())
    }
    let router = axum::Router::new()
        .route("/v1/guardrails", get(guardrails))
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture bind");
    let url = format!("http://{}", listener.local_addr().expect("fixture addr"));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("fixture serve");
    });
    (url, shared)
}

fn oml(url: &str, dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
    for name in ["OML_URL", "OML_API_KEY", "OML_CLI_KEY", "OML_CLI_SERVER"] {
        cmd.env_remove(name);
    }
    cmd.env("OML_CLI_CREDENTIALS", dir.join("credentials"))
        .args(["--url", url, "--api-key", "oml_fixture_reader"])
        .args(args)
        .output()
        .expect("the oml binary runs")
}

fn err(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_writes_promoted_guardrails_only() {
    let (url, _) = fixture(the_set()).await;
    let dir = scratch("promoted");
    let path = dir.join("oml-guardrails.md");
    let out = oml(
        &url,
        &dir,
        &[
            "guardrails",
            "export",
            "--format",
            "claude-md",
            "--out",
            path.to_str().expect("utf-8"),
        ],
    );
    assert!(out.status.success(), "{}", err(&out));
    let text = std::fs::read_to_string(&path).expect("the file");
    assert!(text.contains("- [must] Never push to main."), "{text}");
    assert!(text.contains("- [must] Run the tests first."), "{text}");
    assert!(!text.contains("lint-first"), "no held guardrail: {text}");
    assert!(!text.contains("checked:"), "text, not enforcement: {text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_header_carries_origin_scope_node_guardrail_scope_and_set_digest() {
    let (url, _) = fixture(the_set()).await;
    let dir = scratch("header");
    let path = dir.join("rules.md");
    let out = oml(
        &url,
        &dir,
        &[
            "guardrails",
            "export",
            "--format",
            "claude-md",
            "--out",
            path.to_str().expect("utf-8"),
        ],
    );
    assert!(out.status.success(), "{}", err(&out));
    let text = std::fs::read_to_string(&path).expect("the file");
    let header = text.lines().next().unwrap_or_default();
    for part in [
        url.as_str(),
        "universe.eng.api",
        "00000000-0000-4000-8000-0000000000e2",
        "guardrail scope ancestors",
        "2 guardrails",
        "set_digest sha256:aaaa",
        "text, not enforcement; do not edit",
    ] {
        assert!(header.contains(part), "{part} in {header}");
    }
    assert!(!text.starts_with("---"), "no paths frontmatter: {text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn check_exits_one_when_the_set_changed() {
    let (url, shared) = fixture(the_set()).await;
    let dir = scratch("check");
    let path = dir.join("rules.md");
    let p = path.to_str().expect("utf-8");
    let out = oml(
        &url,
        &dir,
        &["guardrails", "export", "--format", "claude-md", "--out", p],
    );
    assert!(out.status.success(), "{}", err(&out));
    let written = std::fs::read_to_string(&path).expect("the file");
    let check = [
        "guardrails",
        "export",
        "--format",
        "claude-md",
        "--out",
        p,
        "--check",
    ];
    assert_eq!(oml(&url, &dir, &check).status.code(), Some(0));
    *shared.lock().expect("lock") = set(
        "sha256:bbbb",
        vec![served(
            "00000000-0000-4000-8000-0000000000d1",
            "no-push",
            "Never push to main.",
        )],
    );
    let out = oml(&url, &dir, &check);
    assert_eq!(out.status.code(), Some(1), "{}", err(&out));
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file"),
        written,
        "--check writes nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_md_refuses_a_file_without_exactly_one_marker_pair() {
    let (url, _) = fixture(the_set()).await;
    let dir = scratch("markers");
    let path = dir.join("AGENTS.md");
    let p = path.to_str().expect("utf-8");
    for body in [
        "# Project\n\nNo markers here.\n",
        "<!-- oml:guardrails -->\n<!-- oml:guardrails -->\n<!-- oml:guardrails -->\n",
    ] {
        std::fs::write(&path, body).expect("the file");
        let out = oml(
            &url,
            &dir,
            &["guardrails", "export", "--format", "agents-md", "--out", p],
        );
        assert_eq!(out.status.code(), Some(2), "{}", err(&out));
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file"),
            body,
            "unchanged"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_md_replaces_only_the_text_between_the_markers() {
    let (url, _) = fixture(the_set()).await;
    let dir = scratch("between");
    let path = dir.join("AGENTS.md");
    let p = path.to_str().expect("utf-8");
    std::fs::write(
        &path,
        "# Project\n\nKeep this.\n\n<!-- oml:guardrails -->\nstale text\n<!-- oml:guardrails -->\n\nAnd this.\n",
    )
    .expect("the file");
    let out = oml(
        &url,
        &dir,
        &["guardrails", "export", "--format", "agents-md", "--out", p],
    );
    assert!(out.status.success(), "{}", err(&out));
    let text = std::fs::read_to_string(&path).expect("the file");
    assert!(
        text.starts_with("# Project\n\nKeep this.\n\n<!-- oml:guardrails -->\n"),
        "{text}"
    );
    assert!(
        text.ends_with("<!-- oml:guardrails -->\n\nAnd this.\n"),
        "{text}"
    );
    assert!(!text.contains("stale text"), "{text}");
    assert!(text.contains("- [must] Never push to main."), "{text}");
    assert_eq!(text.matches("<!-- oml:guardrails -->").count(), 2, "{text}");
    let check = [
        "guardrails",
        "export",
        "--format",
        "agents-md",
        "--out",
        p,
        "--check",
    ];
    assert_eq!(oml(&url, &dir, &check).status.code(), Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn show_prints_the_hook_text_and_exits_one_over_the_cap() {
    let (url, shared) = fixture(the_set()).await;
    let dir = scratch("show");
    let out = oml(&url, &dir, &["guardrails", "show"]);
    assert!(out.status.success(), "{}", err(&out));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("OML guardrails for this session, from "),
        "{text}"
    );
    assert!(text.contains("- [must] Never push to main."), "{text}");
    let many: Vec<Value> = (0..13)
        .map(|i| {
            served(
                &format!("00000000-0000-4000-8000-0000000001{i:02}"),
                &format!("r-{i}"),
                &format!("Rule {i}."),
            )
        })
        .collect();
    *shared.lock().expect("lock") = set("sha256:cccc", many);
    let out = oml(&url, &dir, &["guardrails", "show"]);
    assert_eq!(out.status.code(), Some(1), "{}", err(&out));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("- [must] Rule 12."),
        "show lists them all: {text}"
    );
}
