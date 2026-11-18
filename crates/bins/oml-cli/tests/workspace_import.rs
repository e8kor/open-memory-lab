//! `oml workspaces import` through the real binary against a stub server
//! (ADR-0084 d5, d7, I10; #426): the CLI polls a running import until it
//! ends, abandons the upload session it opened on every failure, reuses a
//! dry run's upload, refuses a first import without `--expect-key` before
//! uploading, and warns of an identity file others can read.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use serde_json::{Value, json};

const KEY: &str = "oml_test_import_key";
const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../shared/oml-bundle/tests/vectors/"
);

fn vector(name: &str) -> Vec<u8> {
    std::fs::read(format!("{VECTORS}{name}")).expect("a committed vector")
}

/// The committed TEST-ONLY identity, as `age-keygen` writes a file.
fn identity_file() -> String {
    let hex_bytes = String::from_utf8(vector("TEST-ONLY-age-identity.hex")).expect("text");
    let hex_bytes = hex_bytes.trim();
    let bytes: Vec<u8> = (0..hex_bytes.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_bytes[i..i + 2], 16).expect("hex"))
        .collect();
    let secret = bech32::encode(
        "age-secret-key-",
        bech32::ToBase32::to_base32(&bytes),
        bech32::Variant::Bech32,
    )
    .expect("bech32")
    .to_uppercase();
    format!("# TEST-ONLY\n{secret}\n")
}

fn fingerprint() -> String {
    String::from_utf8(vector("signing-key.fingerprint"))
        .expect("text")
        .trim()
        .to_owned()
}

/// A scratch directory holding the sealed bundle and an identity file of
/// `mode`.
fn scratch(test: &str, mode: u32) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-import-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    std::fs::write(dir.join("b.oml-bundle"), vector("signed-bundle.tar.age")).expect("bundle");
    let id = dir.join("id.txt");
    std::fs::write(&id, identity_file()).expect("identity");
    std::fs::set_permissions(&id, std::fs::Permissions::from_mode(mode)).expect("chmod");
    dir
}

fn oml(dir: &Path, server: &str, extra: &[&str]) -> Output {
    let mut args = vec![
        "--url",
        server,
        "--api-key",
        KEY,
        "workspaces",
        "import",
        "b.oml-bundle",
        "--into",
        "ada-notes",
        "--identity",
        "id.txt",
    ];
    args.extend_from_slice(extra);
    Command::new(env!("CARGO_BIN_EXE_oml"))
        .current_dir(dir)
        .env("OML_CLI_CREDENTIALS", dir.join("credentials.toml"))
        .env_remove("OML_URL")
        .env_remove("OML_API_KEY")
        .env_remove("CLAUDE_PLUGIN_OPTION_URL")
        .env_remove("CLAUDE_PLUGIN_OPTION_API_KEY")
        .args(&args)
        .output()
        .expect("the oml binary runs")
}

/// How the stub answers, and what it was asked, as `METHOD path` lines.
#[derive(Default)]
struct Stub {
    calls: Mutex<Vec<String>>,
    /// The import request is refused `request.validation`.
    refuse_import: bool,
    /// The target workspace exists.
    target_exists: bool,
    /// `GET /v2/imports/{id}` answers `running` this many times first.
    running_reads: Mutex<u32>,
}

type Shared = Arc<Stub>;

fn note(stub: &Stub, line: String) {
    stub.calls.lock().expect("lock").push(line);
}

fn import(state: &str) -> Value {
    json!({
        "import_id": "imp-1",
        "state": state,
        "counts": { "versions": 3 },
        "refusals": [],
    })
}

fn refusal(status: StatusCode, code: &str, field: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "code": code,
                "message": "refused",
                "rule": "",
                "details": { "field": field },
            }
        })),
    )
        .into_response()
}

fn serve(runtime: &tokio::runtime::Runtime, stub: Stub) -> (String, Shared) {
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("a listener on an ephemeral port");
    let server = format!("http://{}", listener.local_addr().expect("its address"));
    let shared: Shared = Arc::new(stub);
    let router = axum::Router::new()
        .route(
            "/v2/uploads",
            post(|State(s): State<Shared>| async move {
                note(&s, "POST /v2/uploads".to_owned());
                Json(json!({ "upload_id": "up-1", "state": "open" }))
            }),
        )
        .route(
            "/v2/uploads/{id}",
            axum::routing::delete(|State(s): State<Shared>, UrlPath(id): UrlPath<String>| async move {
                note(&s, format!("DELETE /v2/uploads/{id}"));
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/v2/uploads/{id}/parts/{n}",
            put(
                |State(s): State<Shared>, UrlPath((id, n)): UrlPath<(String, u64)>| async move {
                    note(&s, format!("PUT /v2/uploads/{id}/parts/{n}"));
                    Json(json!({ "part": n }))
                },
            ),
        )
        .route(
            "/v2/uploads/{id}/complete",
            post(|State(s): State<Shared>, UrlPath(id): UrlPath<String>| async move {
                note(&s, format!("POST /v2/uploads/{id}/complete"));
                Json(json!({ "upload_id": id, "state": "completed" }))
            }),
        )
        .route(
            "/v2/workspaces/{w}",
            get(|State(s): State<Shared>, UrlPath(w): UrlPath<String>| async move {
                note(&s, format!("GET /v2/workspaces/{w}"));
                if s.target_exists {
                    Json(json!({ "workspace_id": "w-1", "name": w })).into_response()
                } else {
                    refusal(StatusCode::NOT_FOUND, "access.not_found", "")
                }
            }),
        )
        .route(
            "/v2/imports",
            post(|State(s): State<Shared>, Json(body): Json<Value>| async move {
                note(&s, format!("POST /v2/imports upload={}", body["upload_id"]));
                if s.refuse_import {
                    return refusal(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "request.validation",
                        "/allow_older",
                    );
                }
                if body["dry_run"] == true {
                    return Json(json!({
                        "plan": {
                            "counts": { "versions": 3 },
                            "source": { "tenant_name": "acme" },
                            "exported_at": "2026-11-17T10:00:00Z",
                            "authenticated": true,
                        }
                    }))
                    .into_response();
                }
                (StatusCode::ACCEPTED, Json(json!({ "import": import("running") })))
                    .into_response()
            }),
        )
        .route(
            "/v2/imports/{id}",
            get(|State(s): State<Shared>, UrlPath(id): UrlPath<String>| async move {
                note(&s, format!("GET /v2/imports/{id}"));
                let mut left = s.running_reads.lock().expect("lock");
                let state = if *left > 0 {
                    *left -= 1;
                    "running"
                } else {
                    "done"
                };
                Json(json!({ "import": import(state) }))
            }),
        )
        .with_state(shared.clone());
    runtime.spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    (server, shared)
}

fn calls(stub: &Stub) -> Vec<String> {
    stub.calls.lock().expect("lock").clone()
}

fn texts(run: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

#[test]
fn an_import_polls_its_import_until_it_is_no_longer_running_and_prints_the_final_import() {
    let dir = scratch("poll", 0o600);
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let (server, stub) = serve(
        &runtime,
        Stub {
            target_exists: true,
            running_reads: Mutex::new(1),
            ..Stub::default()
        },
    );
    let run = oml(&dir, &server, &["--expect-key", &fingerprint()]);
    runtime.shutdown_background();
    let (stdout, stderr) = texts(&run);
    assert_eq!(run.status.code(), Some(0), "{stdout}{stderr}");
    assert!(stdout.contains("import imp-1 done"), "{stdout}");
    let reads = calls(&stub)
        .iter()
        .filter(|c| c.as_str() == "GET /v2/imports/imp-1")
        .count();
    assert_eq!(reads, 2, "{:?}", calls(&stub));
    assert!(!stderr.contains("readable by its group"), "{stderr}");
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

#[test]
fn a_refused_import_abandons_the_upload_session_it_opened() {
    let dir = scratch("abandon", 0o600);
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let (server, stub) = serve(
        &runtime,
        Stub {
            target_exists: true,
            refuse_import: true,
            ..Stub::default()
        },
    );
    let run = oml(&dir, &server, &["--expect-key", &fingerprint()]);
    runtime.shutdown_background();
    let (stdout, stderr) = texts(&run);
    assert_ne!(run.status.code(), Some(0), "{stdout}{stderr}");
    let seen = calls(&stub);
    assert_eq!(
        seen.last().map(String::as_str),
        Some("DELETE /v2/uploads/up-1"),
        "{seen:?}"
    );
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

#[test]
fn a_dry_runs_upload_is_named_and_the_real_run_imports_it_without_uploading_again() {
    let dir = scratch("reuse", 0o600);
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let (server, stub) = serve(
        &runtime,
        Stub {
            target_exists: true,
            ..Stub::default()
        },
    );
    let dry = oml(&dir, &server, &["--expect-key", &fingerprint(), "--dry-run"]);
    let (stdout, stderr) = texts(&dry);
    assert_eq!(dry.status.code(), Some(0), "{stdout}{stderr}");
    assert!(stdout.contains("--upload up-1"), "{stdout}");
    let opened = calls(&stub).len();
    assert!(
        !calls(&stub).iter().any(|c| c.starts_with("DELETE")),
        "a dry run keeps its upload"
    );

    let real = oml(
        &dir,
        &server,
        &["--expect-key", &fingerprint(), "--upload", "up-1"],
    );
    runtime.shutdown_background();
    let (stdout, stderr) = texts(&real);
    assert_eq!(real.status.code(), Some(0), "{stdout}{stderr}");
    let after: Vec<String> = calls(&stub).split_off(opened);
    assert!(
        after.iter().all(|c| !c.starts_with("POST /v2/uploads")
            && !c.starts_with("PUT")),
        "{after:?}"
    );
    assert!(
        after.contains(&"POST /v2/imports upload=\"up-1\"".to_owned()),
        "{after:?}"
    );
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

#[test]
fn a_first_import_into_a_new_name_without_expect_key_is_refused_before_uploading() {
    let dir = scratch("expect-key", 0o600);
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let (server, stub) = serve(&runtime, Stub::default());
    let run = oml(&dir, &server, &[]);
    runtime.shutdown_background();
    let (stdout, stderr) = texts(&run);
    assert_ne!(run.status.code(), Some(0), "{stdout}{stderr}");
    assert!(
        stderr.contains(&format!("--expect-key {}", fingerprint())),
        "{stderr}"
    );
    assert!(
        !calls(&stub).iter().any(|c| c.starts_with("POST")),
        "{:?}",
        calls(&stub)
    );
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

#[test]
fn an_identity_file_readable_by_others_is_warned_of_on_stderr() {
    let dir = scratch("mode", 0o644);
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let (server, _) = serve(
        &runtime,
        Stub {
            target_exists: true,
            ..Stub::default()
        },
    );
    let run = oml(&dir, &server, &["--expect-key", &fingerprint()]);
    runtime.shutdown_background();
    let (stdout, stderr) = texts(&run);
    assert_eq!(run.status.code(), Some(0), "{stdout}{stderr}");
    assert!(
        stderr.contains("identity file is readable by its group or others (mode 644)"),
        "{stderr}"
    );
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}
