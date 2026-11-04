//! `oml doctor` names the server's version from `/readyz`'s
//! `build.version`, which the lead has served since v0.4.0, rather than
//! saying the API exposes none (http-api.md "Health").

use std::process::Command;
use std::sync::Arc;

use serde_json::Value;

use oml_app::App;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_transport_http::ApiState;

/// A lead over the in-memory store, its build the real binary's, served
/// on an ephemeral port: no Postgres.
async fn serve() -> (String, tokio::task::JoinHandle<()>) {
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new([9; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let router = ApiState::new(
        Arc::new(app),
        store,
        Arc::new(|| Box::pin(async { true })),
        true,
        None,
    )
    .with_build(oml_server_lead::build_info())
    .into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("an ephemeral port");
    let url = format!("http://{}", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (url, server)
}

fn doctor(url: &str, json: bool) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
    if json {
        cmd.arg("--json");
    }
    cmd.arg("--url").arg(url).arg("doctor");
    for name in ["OML_URL", "OML_API_KEY", "OML_CLI_KEY", "OML_CLI_SERVER"] {
        cmd.env_remove(name);
    }
    cmd.env("OML_CLI_CREDENTIALS", "/nonexistent/oml-credentials");
    let out = cmd.output().expect("the oml binary runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_prints_the_server_version_readyz_reports() {
    let (url, server) = serve().await;
    let version = env!("CARGO_PKG_VERSION");
    let probe = url.clone();
    let (text, json) =
        tokio::task::spawn_blocking(move || (doctor(&probe, false), doctor(&probe, true)))
            .await
            .expect("the doctor runs");
    server.abort();

    let line = text
        .lines()
        .find(|l| l.starts_with("server "))
        .unwrap_or_else(|| panic!("no server line:\n{text}"));
    assert_eq!(
        line,
        format!("server      : oml-server-lead {version}"),
        "{text}"
    );
    let json: Value = serde_json::from_str(json.trim()).expect("doctor --json");
    assert_eq!(json["server_version"], version, "{json}");
}
