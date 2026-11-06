//! `oml demo seed` against the real lead over a migrated Postgres store
//! (ADR-0067 d4): the workarea fills through the public API with thirty
//! memories, the reviewers are never the author, and no identity the seed
//! issued can still reach the tree afterwards.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};

use oml_pg::PgStore;
use oml_server_lead::start_lead;

// ---------------------------------------------------------------------------
// Throwaway databases and role pairs (the cli_e2e.rs pattern)
// ---------------------------------------------------------------------------

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL names a throwaway PostgreSQL cluster; no suite defaults to one, so none reaches a live server")
}

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
const PAIR_PASSWORD: &str = "oml_test_pair";

fn role_url(db: &str, role: &str) -> String {
    let base = base_url();
    let (scheme, rest) = base
        .split_once("://")
        .expect("DATABASE_URL is a postgres:// URL");
    let (_, host_path) = rest.rsplit_once('@').expect("DATABASE_URL names its user");
    let (host, _) = host_path
        .split_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{scheme}://{role}:{PAIR_PASSWORD}@{host}/{db}")
}

fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

/// Mints `db`'s owner/serving role pair and creates `db` owned by the owner
/// (ADR-0034 d2, d8); `false`, with `did not run: <role> cannot create
/// roles` printed, when `DATABASE_URL`'s role is not a superuser. Under
/// `OML_TEST_REQUIRE_SPLIT=1`, which CI sets, the same condition fails.
async fn mint_database(db: &str) -> bool {
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    let (role, mints): (String, bool) = sqlx::query_as(
        "SELECT current_user::text, rolsuper FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut admin)
    .await
    .expect("role attributes");
    if !mints {
        assert!(
            std::env::var("OML_TEST_REQUIRE_SPLIT").as_deref() != Ok("1"),
            "{role} cannot create roles, and OML_TEST_REQUIRE_SPLIT=1 requires the role pair (ADR-0034 d8)"
        );
        println!("did not run: {role} cannot create roles");
        return false;
    }
    for role in ["owner", "serving"] {
        admin
            .execute(format!("CREATE ROLE {db}_{role} LOGIN PASSWORD '{PAIR_PASSWORD}'").as_str())
            .await
            .expect("mint the role pair");
    }
    admin
        .execute(format!("CREATE DATABASE {db} OWNER {db}_owner").as_str())
        .await
        .expect("create throwaway database");
    true
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
        for role in ["serving", "owner"] {
            let _ = admin
                .execute(format!("DROP ROLE IF EXISTS {name}_{role}").as_str())
                .await;
        }
    }
}

// ---------------------------------------------------------------------------
// The lead over TCP, and the oml binary against it
// ---------------------------------------------------------------------------

struct Lead {
    db: String,
    url: String,
    admin_key: String,
    creds: PathBuf,
    server: tokio::task::JoinHandle<()>,
}

impl Lead {
    /// The real lead over a fresh database, `oauth.issuer` unset, served on an ephemeral port, and its first admin key
    /// claimed over HTTP with the enrollment token the start minted.
    async fn start() -> Option<Self> {
        Self::start_behind(|router| router).await
    }

    /// [`Lead::start`], its router served through `front`: what the
    /// public origin passes to the lead, standing in for a reverse proxy.
    async fn start_behind(front: impl FnOnce(axum::Router) -> axum::Router) -> Option<Self> {
        let db = format!(
            "oml_test_demo_{}_{}_{}",
            std::process::id(),
            DB_N.fetch_add(1, Ordering::SeqCst),
            run_tag()
        );
        if !mint_database(&db).await {
            return None;
        }
        let owner = role_url(&db, &format!("{db}_owner"));
        let store = PgStore::connect(&role_url(&db, &format!("{db}_serving")))
            .await
            .expect("connect")
            .with_owner_url(&owner);
        store.migrate(&owner).await.expect("migrations apply");
        let mut seed = [0_u8; 32];
        getrandom::getrandom(&mut seed).expect("OS randomness");
        let started = start_lead(&store, SigningKey::from_bytes(&seed), None)
            .await
            .expect("the start reads the store");
        let enrollment = started
            .enrollment_token
            .expect("an empty database mints one");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral bind");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));
        let router = front(started.router);
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });
        let claim_url = format!("{url}/v2/bootstrap/claim");
        let (status, _, claimed) = blocking(move || {
            http(
                agent()
                    .post(&claim_url)
                    .set("Content-Type", "application/json"),
                Some(
                    json!({
                        "enrollment_token": enrollment,
                        "name": "root-admin",
                        "tenant_name": "acme",
                    })
                    .to_string(),
                ),
            )
        })
        .await;
        assert_eq!(status, 201, "{claimed}");
        let creds = std::env::temp_dir().join(format!("{db}.toml"));
        Some(Self {
            admin_key: claimed["value"].as_str().expect("the value").to_owned(),
            db,
            url,
            creds,
            server,
        })
    }

    /// The `oml` binary under the admin key; returns (exit code, stdout,
    /// stderr).
    async fn oml(&self, args: &[&str]) -> (i32, String, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
        cmd.arg("--url")
            .arg(&self.url)
            .arg("--api-key")
            .arg(&self.admin_key)
            .args(args)
            .env("OML_CLI_CREDENTIALS", &self.creds);
        for name in ["OML_URL", "OML_API_KEY", "OML_CLI_KEY", "OML_CLI_SERVER"] {
            cmd.env_remove(name);
        }
        let out = blocking(move || cmd.output().expect("the oml binary runs")).await;
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// [`Lead::oml`] with `--json`, its stdout parsed.
    async fn oml_json(&self, args: &[&str]) -> (i32, Value, String) {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let (code, stdout, stderr) = self.oml(&all).await;
        let parsed = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
        (code, parsed, format!("stdout: {stdout}\nstderr: {stderr}"))
    }

    async fn stop(self) {
        self.server.abort();
        let _ = self.server.await;
        let _ = std::fs::remove_file(&self.creds);
        drop_db(&self.db).await;
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f)
        .await
        .expect("the blocking call ends")
}

/// An agent that follows no redirect, so the authorize page's `303` is the
/// answer read.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(30))
        .build()
}

/// Sends `request` with `body`; returns (status, headers, body as JSON, or
/// as a string when it is not JSON).
fn http(request: ureq::Request, body: Option<String>) -> (u16, Vec<(String, String)>, Value) {
    let result = match body {
        Some(body) => request.send_string(&body),
        None => request.call(),
    };
    let response = match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(e) => panic!("the lead answers: {e}"),
    };
    let status = response.status();
    let headers = response
        .headers_names()
        .into_iter()
        .map(|name| {
            let value = response.header(&name).unwrap_or_default().to_owned();
            (name, value)
        })
        .collect();
    let text = response.into_string().expect("a readable body");
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, headers, body)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_demo_seed_fills_a_workarea_with_approved_superseded_and_held_memories_and_fences_its_identities()
 {
    let Some(lead) = Lead::start().await else {
        return;
    };
    let (code, stdout, stderr) = lead.oml(&["demo", "seed", "reviewer-demo"]).await;
    assert_eq!(code, 0, "stdout: {stdout}\nstderr: {stderr}");
    assert!(
        stdout.contains("24 approved (4 of them replaced by an approved newer version), 6 held"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("oml_"),
        "no key value is printed: {stdout}"
    );

    // The promoted tips: twenty plain approvals and the four successors, the
    // replaced versions and the held claims absent from them.
    let (code, promoted, why) = lead
        .oml_json(&[
            "admin",
            "read",
            "--workarea",
            "reviewer-demo",
            "--state",
            "promoted",
        ])
        .await;
    assert_eq!(code, 0, "{why}");
    let texts: Vec<String> = promoted["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["text"].as_str().expect("text").to_owned())
        .collect();
    assert_eq!(texts.len(), 24, "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("capped at 40 connections")));
    assert!(
        !texts
            .iter()
            .any(|t| t.ends_with("capped at 20 connections per instance."))
    );

    let (code, superseded, why) = lead
        .oml_json(&[
            "admin",
            "read",
            "--workarea",
            "reviewer-demo",
            "--state",
            "superseded",
        ])
        .await;
    assert_eq!(code, 0, "{why}");
    assert_eq!(superseded["items"].as_array().expect("items").len(), 4);

    let (code, held, why) = lead
        .oml_json(&[
            "admin",
            "read",
            "--workarea",
            "reviewer-demo",
            "--state",
            "held",
        ])
        .await;
    assert_eq!(code, 0, "{why}");
    let held = held["items"].as_array().expect("items");
    assert_eq!(held.len(), 6);

    // R2: every verdict came from a reviewer identity, never the author's,
    // and both reviewers gave some.
    let (code, author, why) = lead
        .oml_json(&["keys", "show", "reviewer-demo-author"])
        .await;
    assert_eq!(code, 0, "{why}");
    let author_key = author["key_id"].as_str().expect("key_id").to_owned();
    let mut reviewers = std::collections::BTreeSet::new();
    for item in promoted["items"].as_array().expect("items") {
        let id = item["artifact_id"].as_str().expect("artifact_id");
        let (code, memory, why) = lead.oml_json(&["admin", "read", id]).await;
        assert_eq!(code, 0, "{why}");
        assert_eq!(memory["principal"], "demo/reviewer-demo-author", "{memory}");
        let verdicts = memory["verdicts"].as_array().expect("verdicts");
        assert!(
            !verdicts.is_empty(),
            "a promoted memory has a verdict: {memory}"
        );
        for verdict in verdicts {
            let by = verdict["key"].as_str().expect("verdict key").to_owned();
            assert_ne!(by, author_key, "the reviewer is the author: {memory}");
            reviewers.insert(by);
        }
    }
    assert_eq!(reviewers.len(), 2, "{reviewers:?}");

    // The seed's identities are fenced: each key still exists, as history, and
    // reaches nothing.
    for name in [
        "reviewer-demo-author",
        "reviewer-demo-reviewer-a",
        "reviewer-demo-reviewer-b",
    ] {
        let (code, shown, why) = lead.oml_json(&["keys", "show", name]).await;
        assert_eq!(code, 0, "{why}");
        assert_eq!(
            shown["denials"].as_array().expect("denials").len(),
            1,
            "{shown}"
        );
    }

    // A second seed under the same name is refused by name, nothing doubled.
    let (code, _, stderr) = lead.oml(&["demo", "seed", "reviewer-demo"]).await;
    assert_ne!(code, 0);
    assert!(
        stderr.contains("name.taken") || stderr.contains("taken"),
        "{stderr}"
    );

    lead.stop().await;
}
