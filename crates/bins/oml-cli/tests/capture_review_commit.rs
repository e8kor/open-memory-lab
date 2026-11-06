//! `oml review commit` (ADR-0064 d18) against a fixture of the review and
//! workarea routes: the held set goes to a file, the file's verdicts are
//! submitted, and the workarea is committed and marked in progress again
//! only when nothing is held. The fixture is a test double; the routes are
//! proven in the server crates.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::routing::{get, post};
use serde_json::{Value, json};

const ID: &str = "11111111-1111-4111-8111-111111111111";

#[derive(Default)]
struct Fixture {
    held: Vec<Value>,
    verdicts: Vec<Value>,
    modes: Vec<Value>,
}

type Shared = Arc<Mutex<Fixture>>;

async fn serve(held: Vec<Value>) -> (String, Shared) {
    let shared: Shared = Arc::new(Mutex::new(Fixture {
        held,
        ..Fixture::default()
    }));
    async fn list(State(s): State<Shared>) -> Json<Value> {
        Json(json!({"items": s.lock().unwrap().held, "next_cursor": null}))
    }
    async fn bulk(State(s): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
        let mut f = s.lock().unwrap();
        let ids: Vec<String> = body["artifact_ids"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
            .collect();
        f.held
            .retain(|h| !ids.contains(&h["artifact_id"].as_str().unwrap_or("").to_owned()));
        f.verdicts.push(body.clone());
        let approved = if body["verdict"] == "approve" {
            ids.len()
        } else {
            0
        };
        Json(json!({
            "items": ids.iter().map(|i| json!({"artifact_id": i, "outcome": body["verdict"], "journal_seq": 1})).collect::<Vec<_>>(),
            "summary": {"approved": approved, "rejected": ids.len() - approved, "skipped": 0},
        }))
    }
    async fn mode(State(s): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
        s.lock().unwrap().modes.push(body);
        Json(json!({}))
    }
    let router = axum::Router::new()
        .route("/v2/review/held", get(list))
        .route("/v2/review/verdicts/bulk", post(bulk))
        .route("/v2/workareas/{w}/mode", post(mode))
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (url, shared)
}

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cap-review-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn commit(dir: &Path, url: &str, file: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oml"))
        .current_dir(dir)
        .args(["review", "commit", "pat.widgets-3fa2c1", "--file"])
        .arg(file)
        .env_clear()
        .env("HOME", dir)
        .env("OML_URL", url)
        .env("OML_API_KEY", "oml_reviewer_fixture_key")
        .output()
        .unwrap()
}

fn held() -> Vec<Value> {
    vec![
        json!({"artifact_id": ID, "claim_key": "k1", "text": "a fact", "anchor": {"path": "src/x.rs"}}),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn review_commit_writes_the_held_set_to_a_file_and_commits_nothing_yet() {
    let dir = scratch("writes");
    let (url, shared) = serve(held()).await;
    let file = dir.join("decisions.txt");
    let out = commit(&dir, &url, &file);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(std::fs::read_to_string(&file).unwrap().contains(ID));
    assert!(
        shared.lock().unwrap().modes.is_empty(),
        "no commit before the person decides"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn review_commit_stops_before_the_commit_while_anything_is_held() {
    let dir = scratch("held");
    let (url, shared) = serve(held()).await;
    let file = dir.join("decisions.txt");
    std::fs::write(&file, format!("leave {ID}\n")).unwrap();
    let out = commit(&dir, &url, &file);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("still held"));
    assert!(
        shared.lock().unwrap().modes.is_empty(),
        "the commit is refused while anything is held"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn review_commit_re_marks_the_workarea_in_progress() {
    let dir = scratch("remark");
    let (url, shared) = serve(held()).await;
    let file = dir.join("decisions.txt");
    std::fs::write(&file, format!("approve {ID}\n")).unwrap();
    let out = commit(&dir, &url, &file);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let f = shared.lock().unwrap();
    assert_eq!(f.verdicts[0]["verdict"], "approve");
    let modes: Vec<&str> = f.modes.iter().filter_map(|m| m["mode"].as_str()).collect();
    assert_eq!(
        modes,
        ["standard", "in_progress"],
        "the commit, then the next iteration's mark"
    );
}
