//! `oml workspaces export` through the real binary (ADR-0084 d5, d15;
//! #425): no recipient is refused before anything is sent or created; the
//! server's age file lands in a new owner-only file; an existing file is
//! never overwritten.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::post;

const KEY: &str = "oml_test_export_key";
const RECIPIENT: &str = "age1c7uc8xfckg2ntq76m2tsazw6xqsm9awvg8v4f4g57xqgcvs3e5pq9963un";
const FILE: &[u8] = b"age-encryption.org/v1\nfixture bytes";

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-export-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn oml(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oml"))
        .current_dir(dir)
        .env("OML_CLI_CREDENTIALS", dir.join("credentials.toml"))
        .env_remove("OML_URL")
        .env_remove("OML_API_KEY")
        .env_remove("CLAUDE_PLUGIN_OPTION_URL")
        .env_remove("CLAUDE_PLUGIN_OPTION_API_KEY")
        .args(args)
        .output()
        .expect("the oml binary runs")
}

/// A server answering the export route with [`FILE`], counting its calls.
fn fixture(runtime: &tokio::runtime::Runtime) -> (String, Arc<AtomicU64>) {
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("a listener on an ephemeral port");
    let server = format!("http://{}", listener.local_addr().expect("its address"));
    let calls = Arc::new(AtomicU64::new(0));
    let counted = calls.clone();
    let router = axum::Router::new().route(
        "/v2/workspaces/{workspace}/exports",
        post(move || {
            let counted = counted.clone();
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                (
                    [
                        (header::CONTENT_TYPE, "application/octet-stream"),
                        (header::HeaderName::from_static("oml-manifest-digest"), "sha256:abc"),
                    ],
                    FILE,
                )
                    .into_response()
            }
        }),
    );
    runtime.spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    (server, calls)
}

#[test]
fn an_export_without_a_recipient_is_refused_before_anything_is_sent_or_created() {
    let dir = scratch("no-recipient");
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let (server, calls) = fixture(&runtime);
    let out = dir.join("ada.oml-bundle");
    let run = oml(
        &dir,
        &[
            "--url",
            &server,
            "--api-key",
            KEY,
            "workspaces",
            "export",
            "ada",
            "--out",
            out.to_str().expect("utf-8"),
        ],
    );
    runtime.shutdown_background();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert_eq!(run.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("bundle.unencrypted"), "{stderr}");
    assert_eq!(calls.load(Ordering::SeqCst), 0, "nothing was sent");
    assert!(!out.exists(), "nothing was created");
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

#[test]
fn an_export_lands_in_a_new_owner_only_file_and_never_overwrites_one() {
    let dir = scratch("owner-only");
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let (server, calls) = fixture(&runtime);
    let out = dir.join("ada.oml-bundle");
    let args = [
        "--url",
        &server,
        "--api-key",
        KEY,
        "workspaces",
        "export",
        "ada",
        "--recipient",
        RECIPIENT,
        "--out",
        out.to_str().expect("utf-8"),
    ];
    let run = oml(&dir, &args);
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert_eq!(
        run.status.code(),
        Some(0),
        "{stdout}{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(stdout.contains("manifest sha256:abc"), "{stdout}");
    assert_eq!(std::fs::read(&out).expect("written"), FILE);
    let mode = std::fs::metadata(&out).expect("exists").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "owner-only");

    let again = oml(&dir, &args);
    runtime.shutdown_background();
    let stderr = String::from_utf8_lossy(&again.stderr);
    assert_eq!(again.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("already exists"), "{stderr}");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the second run sent nothing");
    assert_eq!(std::fs::read(&out).expect("kept"), FILE);
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}
