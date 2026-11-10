//! `/readyz` names the build it serves (docs/api/http-api.md "Health"): the
//! version, the commit and its time, when the build ran, and the compiler,
//! target and profile — on a ready replica and on one that refuses writes, so
//! an operator can tell which build a failing probe belongs to.

use std::sync::atomic::Ordering;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt as _;

use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::IdGen as _;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_state, pg_ping};
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};

/// A store over `db`'s serving login, migrated through its owner login with
/// `oml.serving_role` set (ADR-0034 d4, d8).
async fn serving_store(db: &str) -> PgStore {
    let store = PgStore::connect(&serving_url(db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(db));
    store
        .migrate(&owner_url(db))
        .await
        .expect("embedded migrations apply");
    store
}

/// The lead's router over the serving login of a fresh database and its
/// name, or `None` when this run cannot mint the role pair (ADR-0034 d8).
async fn router(fork_check_ok: bool) -> Option<(Router, String)> {
    let db_name = format!(
        "oml_test_build_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db_name).await {
        return None;
    }
    let store = serving_store(&db_name).await;
    let router = lead_state(
        &store,
        SigningKey::from_bytes(&OsIds.new_secret()),
        pg_ping(store.pool().clone()),
        fork_check_ok,
        None,
    )
    .into_router();
    Some((router, db_name))
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
    let Some((router, db)) = router(true).await else {
        return;
    };
    let (status, body) = readyz(&router).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ok");
    assert_names_the_build(&body);
    drop_db(&db).await;
}

#[tokio::test]
async fn readyz_still_names_the_build_when_the_replica_refuses_writes() {
    let Some((router, db)) = router(false).await else {
        return;
    };
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
