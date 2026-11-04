//! `oml setup claude-code` through the real binary (ADR-0041 d7): against a
//! fixture that answers `GET /v2/whoami` with a canned key record, and with a
//! fake `claude` first on `PATH` that records each call's arguments and
//! stdin. The fixture and the fake are test doubles: no real Claude Code
//! configuration is touched.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use serde_json::{Value, json};

/// The agent key every case hands the verb; it must reach `claude` on stdin
/// alone.
const KEY: &str = "oml_test_agent_key_value";

/// An empty directory of this test's own, under the OS temp directory.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-setup-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// Answers `GET /v2/whoami` with a key of `role` when the bearer is [`KEY`].
async fn whoami(State(role): State<String>, headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if bearer != format!("Bearer {KEY}") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                json!({"error": {"code": "auth.invalid_key", "message": "no such key",
                "rule": "a key is presented as a bearer", "retryable": false}}),
            ),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "key_id": "key_01J9ZK",
            "name": "crew-agent",
            "role": role,
            "tenant": {"id": "t1", "name": "quickstart"},
            "default_workarea": {"id": "w1", "path": "universe/crew"},
        })),
    )
}

/// A fake `claude` in `dir/bin`: `--version` prints `version`, and every
/// other call appends `ARGV <args>` and `STDIN <stdin>` to `dir/claude.log`.
fn fake_claude(dir: &Path, version: &str) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("the fake's directory");
    let script = bin.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{version} (Claude Code)'; exit 0; fi\n\
             stdin=$(cat)\nprintf 'ARGV %s\\nSTDIN %s\\n' \"$*\" \"$stdin\" >> '{}'\n",
            dir.join("claude.log").display()
        ),
    )
    .expect("the fake claude");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("executable");
    bin
}

/// Runs `oml <args>` with `path` first on `PATH` (none: no `claude` at all)
/// against a fixture answering whoami with `role`.
fn run(dir: &Path, role: &str, path: Option<&Path>, args: &[&str]) -> Output {
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("a listener on an ephemeral port");
    let server = format!("http://{}", listener.local_addr().expect("its address"));
    let router = axum::Router::new()
        .route("/v2/whoami", get(whoami))
        .with_state(role.to_owned());
    runtime.spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    let search = match path {
        Some(bin) => format!("{}:/usr/bin:/bin", bin.display()),
        None => "/usr/bin/oml-no-such-dir".to_owned(),
    };
    let args: Vec<String> = args.iter().map(|a| a.replace("{url}", &server)).collect();
    let out = Command::new(env!("CARGO_BIN_EXE_oml"))
        .env("OML_CLI_CREDENTIALS", dir.join("credentials.toml"))
        .env("PATH", search)
        .env_remove("OML_URL")
        .env_remove("OML_API_KEY")
        .args(&args)
        .output()
        .expect("the oml binary runs");
    runtime.shutdown_background();
    out
}

fn key_file(dir: &Path) -> String {
    let path = dir.join("agent.key");
    std::fs::write(&path, format!("{KEY}\n")).expect("the key file");
    path.display().to_string()
}

fn log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("claude.log")).unwrap_or_default()
}

#[test]
fn setup_claude_code_installs_the_plugin_at_the_clis_own_tag_and_hands_the_key_to_claude_on_stdin_alone()
 {
    let dir = scratch("installs");
    let bin = fake_claude(&dir, "2.1.312");
    let file = key_file(&dir);
    let out = run(
        &dir,
        "writer",
        Some(&bin),
        &[
            "--url",
            "{url}",
            "setup",
            "claude-code",
            "--key-file",
            &file,
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}{stderr}");

    let log = log(&dir);
    let argv: Vec<&str> = log
        .lines()
        .filter_map(|l| l.strip_prefix("ARGV "))
        .collect();
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        argv,
        [
            format!("plugin marketplace add e8kor/open-memory-lab#v{version}").as_str(),
            "plugin install oml@open-memory-lab",
            "plugin configure oml@open-memory-lab --values-stdin",
        ],
        "the three claude commands, in order: {log}"
    );
    let configure_stdin = log
        .lines()
        .filter_map(|l| l.strip_prefix("STDIN "))
        .nth(2)
        .expect("the configure call's stdin");
    let values: Value = serde_json::from_str(configure_stdin).expect("a JSON object on stdin");
    let url = values["url"].as_str().expect("url").to_owned();
    assert!(url.starts_with("http://127.0.0.1:"), "{values}");
    assert_eq!(values, json!({"url": url, "api_key": KEY}));
    assert!(
        argv.iter().all(|a| !a.contains(KEY)),
        "the key is in no argv: {log}"
    );
    assert!(
        !stdout.contains(KEY) && !stderr.contains(KEY),
        "the key is never printed"
    );
    assert!(stdout.contains("/reload-plugins"), "{stdout}");
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

#[test]
fn setup_claude_code_refuses_an_admin_key_as_setup_admin_key_and_runs_no_claude_command() {
    for role in ["admin", "operator"] {
        let dir = scratch(&format!("refuses-{role}"));
        let bin = fake_claude(&dir, "2.1.312");
        let out = run(
            &dir,
            role,
            Some(&bin),
            &["--url", "{url}", "--api-key", KEY, "setup", "claude-code"],
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{role}: {stderr}");
        assert!(stderr.contains("setup.admin_key"), "{role}: {stderr}");
        assert!(out.stdout.is_empty(), "{role}: nothing on stdout");
        assert_eq!(log(&dir), "", "{role}: no claude command ran");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }
}

#[test]
fn setup_claude_code_refuses_a_claude_code_older_than_2_1_285_and_changes_nothing() {
    let dir = scratch("old-claude");
    let bin = fake_claude(&dir, "2.1.284");
    let file = key_file(&dir);
    let out = run(
        &dir,
        "writer",
        Some(&bin),
        &[
            "--url",
            "{url}",
            "setup",
            "claude-code",
            "--key-file",
            &file,
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("2.1.284") && stderr.contains("2.1.285"),
        "names the version found and the one required: {stderr}"
    );
    assert_eq!(log(&dir), "", "no claude command ran");
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

#[test]
fn setup_claude_code_with_print_or_without_claude_runs_nothing_and_prints_a_configure_line_that_reads_the_key_file()
 {
    for (case, with_claude, print) in [("print", true, true), ("no-claude", false, false)] {
        let dir = scratch(case);
        let bin = fake_claude(&dir, "2.1.312");
        let file = key_file(&dir);
        let mut args = vec![
            "--url",
            "{url}",
            "setup",
            "claude-code",
            "--key-file",
            &file,
        ];
        if print {
            args.push("--print");
        }
        let out = run(&dir, "writer", with_claude.then_some(bin.as_path()), &args);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{case}: {stdout}{stderr}");
        assert_eq!(log(&dir), "", "{case}: no claude command ran");
        let version = env!("CARGO_PKG_VERSION");
        assert!(
            stdout.contains(&format!(
                "claude plugin marketplace add e8kor/open-memory-lab#v{version}"
            )) && stdout.contains("claude plugin install oml@open-memory-lab"),
            "{case}: {stdout}"
        );
        assert!(
            stdout.contains(&format!("--rawfile k '{file}'"))
                && stdout.contains("claude plugin configure oml@open-memory-lab --values-stdin"),
            "{case}: the configure line reads the key file: {stdout}"
        );
        assert!(!stdout.contains(KEY), "{case}: the key is never printed");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }
}

#[test]
fn setup_claude_code_refuses_key_file_beside_api_key_and_key_file_without_url_before_anything_is_sent()
 {
    let dir = scratch("key-file-pair");
    let bin = fake_claude(&dir, "2.1.312");
    let file = key_file(&dir);
    for (args, names) in [
        (
            vec![
                "--url",
                "{url}",
                "--api-key",
                KEY,
                "setup",
                "claude-code",
                "--key-file",
                &file,
            ],
            "--api-key",
        ),
        (vec!["setup", "claude-code", "--key-file", &file], "--url"),
    ] {
        let out = run(&dir, "writer", Some(&bin), &args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(
            stderr.contains("--key-file") && stderr.contains(names),
            "{args:?}: {stderr}"
        );
        assert_eq!(log(&dir), "", "{args:?}: no claude command ran");
    }
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}
