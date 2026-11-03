//! `/readyz` names the build it serves (docs/api/http-api.md "Health"): the
//! version, the commit and its time, when the build ran, and the compiler,
//! target and profile — on a ready replica and on one that refuses writes, so
//! an operator can tell which build a failing probe belongs to.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;

use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::IdGen as _;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_state, pg_ping};

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

async fn router(fork_check_ok: bool) -> (Router, String) {
    let db_name = format!(
        "oml_test_build_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst)
    );
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("CREATE DATABASE {db_name}").as_str())
        .await
        .expect("create throwaway database");
    let store = PgStore::connect(&db_url(&db_name)).await.expect("connect");
    store.migrate().await.expect("embedded migrations apply");
    let router = lead_state(
        &store,
        SigningKey::from_bytes(&OsIds.new_secret()),
        pg_ping(store.pool().clone()),
        fork_check_ok,
        None,
    )
    .into_router();
    (router, db_name)
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
    }
}

async fn readyz(router: &Router) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("infallible router");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body collects");
    (
        status,
        serde_json::from_slice(&bytes).expect("readyz is JSON"),
    )
}

/// `YYYY-MM-DDTHH:MM:SSZ`, the one timestamp shape the build fields use.
fn is_utc_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            19 => *c == b'Z',
            _ => c.is_ascii_digit(),
        })
}

fn assert_names_the_build(body: &Value) {
    let build = &body["build"];
    assert_eq!(build["version"], env!("CARGO_PKG_VERSION"), "{body}");
    let commit = build["commit"].as_str().expect("commit is a string");
    assert!(
        commit == "unknown"
            || (commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit())),
        "commit is a full hex object name or unknown: {commit}"
    );
    let commit_time = build["commit_time"].as_str().expect("commit_time");
    assert!(
        commit_time == "unknown" || is_utc_timestamp(commit_time),
        "commit_time: {commit_time}"
    );
    assert!(
        is_utc_timestamp(build["build_time"].as_str().expect("build_time")),
        "build_time: {}",
        build["build_time"]
    );
    assert!(
        build["rustc"]
            .as_str()
            .expect("rustc")
            .starts_with("rustc "),
        "rustc: {}",
        build["rustc"]
    );
    assert!(!build["target"].as_str().expect("target").is_empty());
    assert!(
        ["debug", "release"].contains(&build["profile"].as_str().expect("profile")),
        "profile: {}",
        build["profile"]
    );
}

#[tokio::test]
async fn readyz_names_the_version_commit_build_time_and_toolchain_it_serves() {
    let (router, db) = router(true).await;
    let (status, body) = readyz(&router).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ok");
    assert_names_the_build(&body);
    drop_db(&db).await;
}

#[tokio::test]
async fn readyz_still_names_the_build_when_the_replica_refuses_writes() {
    let (router, db) = router(false).await;
    let (status, body) = readyz(&router).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["checks"]["fork_check"], "failed");
    assert_names_the_build(&body);
    drop_db(&db).await;
}

#[test]
fn the_reported_commit_is_the_checkout_the_binary_was_built_from() {
    // Built inside a git checkout, the build script reads HEAD; the image
    // build has no repository and passes OML_BUILD_COMMIT instead.
    let built = env!("OML_BUILD_COMMIT");
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    if let Some(head) = head {
        assert_eq!(built, head);
    }
}
