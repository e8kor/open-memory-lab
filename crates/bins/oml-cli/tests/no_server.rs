//! What a client with no server configured prints, through the real binary
//! (ADR-0076 d8): every surface names `no_server` and sends nothing. The
//! doctor's server-side line is checked against a fixture answering the
//! probes, `GET /v2/whoami` with an admin key and `/readyz` with no
//! `oauth.issuer`.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use axum::Json;
use axum::routing::get;
use serde_json::json;

const KEY: &str = "oml_test_no_server_key";

/// An empty directory of this test's own, under the OS temp directory.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-no-server-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// `oml <args>` with no start variable inherited, the credentials file in
/// `dir` (absent unless the test writes it), the plugin's variables unset,
/// and `env` set.
fn oml(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_oml"));
    command
        .current_dir(dir)
        .env("OML_CLI_CREDENTIALS", dir.join("credentials.toml"))
        .env_remove("OML_URL")
        .env_remove("OML_API_KEY")
        .env_remove("CLAUDE_PLUGIN_OPTION_URL")
        .env_remove("CLAUDE_PLUGIN_OPTION_API_KEY")
        .args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().expect("the oml binary runs")
}

/// With no pair, doctor fails naming `no_server` and probes nothing: no
/// `not probed (no URL)` line stands in for it (ADR-0076 d8).
#[test]
fn doctor_with_no_pair_prints_fail_no_server_and_probes_nothing() {
    let dir = scratch("doctor");
    let out = oml(&dir, &["doctor"], &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains(
            "fail no_server: no server URL is configured (--url, OML_URL or the credentials \
             file); nothing was probed"
        ),
        "{stdout}"
    );
    assert!(!stdout.contains("not probed (no URL)"), "{stdout}");
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

/// Another verb with no pair, or with a key and no URL, is refused as
/// before, its message naming `no_server` and `oml login <server>`
/// (ADR-0076 d5, d8).
#[test]
fn a_verb_with_no_pair_is_refused_with_the_no_server_name_and_oml_login() {
    let dir = scratch("verb");
    for env in [&[][..], &[("OML_API_KEY", KEY)][..]] {
        let out = oml(&dir, &["events"], env);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{env:?}: {stderr}");
        assert!(stderr.contains("no_server: "), "{env:?}: {stderr}");
        assert!(stderr.contains("oml login <server>"), "{env:?}: {stderr}");
    }
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

/// Doctor run where the plugin exported its key option and no url warns
/// that the plugin reaches no server (ADR-0076 d8).
#[test]
fn doctor_with_the_plugin_key_option_alone_warns_plugin_no_server() {
    let dir = scratch("plugin");
    let out = oml(&dir, &["doctor"], &[("CLAUDE_PLUGIN_OPTION_API_KEY", KEY)]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "warn plugin_no_server: the oml plugin has a key and no url; since this release its \
             url has no default, so its tools reach no server and its hooks load nothing"
        ),
        "{stdout}"
    );
    assert!(!stdout.contains(KEY), "the key is never printed");
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}

/// Doctor with an admin key against a lead that sets no `oauth.issuer`
/// warns that its pages cannot show agents the server's URL (ADR-0076
/// d6, d8).
#[test]
fn doctor_with_an_admin_key_warns_server_origin_when_oauth_issuer_is_unset() {
    let dir = scratch("origin");
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("a listener on an ephemeral port");
    let server = format!("http://{}", listener.local_addr().expect("its address"));
    let router = axum::Router::new()
        .route("/healthz", get(|| async { Json(json!({"status": "ok"})) }))
        .route(
            "/readyz",
            get(|| async {
                Json(json!({"status": "ok", "checks": {}, "oauth_issuer": null}))
            }),
        )
        .route(
            "/v2/whoami",
            get(|| async {
                Json(json!({
                    "key_id": "key_admin",
                    "name": "root-admin",
                    "role": "admin",
                    "principal": "operator",
                    "held_by": "human",
                    "tenant": {"id": "t1", "name": "oml"},
                    "default_workarea": null,
                    "crew": null,
                    "user": null,
                    "grants": [],
                    "shared": false,
                }))
            }),
        );
    runtime.spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    let out = oml(
        &dir,
        &["--url", &server, "--api-key", KEY, "doctor"],
        &[],
    );
    runtime.shutdown_background();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "warn server_origin: the server names no public origin (oauth.issuer unset), so its \
             pages cannot show the URL agents set beside their keys"
        ),
        "{stdout}"
    );
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}
