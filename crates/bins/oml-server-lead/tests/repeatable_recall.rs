//! Repeatable recall (#176): for the same key, query, approved set and
//! `now`, the default pipeline answers the same bytes, in the same order,
//! call after call and from one lead process to another (ADR-0049 d3, d12;
//! ADR-0052 d6, d13; ADR-0072 d9; docs/architecture/repeatable-recall.md).
//!
//! Each case spawns `oml-server-lead` itself over a throwaway database, so
//! the answers compared are the ones the binary serves, and two leads over
//! one database are two operating-system processes. The bytes compared are
//! the whole response body with the value of `recall_id` alone replaced:
//! that id names the recall, so it is new on every call by design
//! (ADR-0054 d2). Nothing is sorted, parsed and re-serialised, or rounded
//! before the comparison. Each test is named after the sentence it proves.

use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use serde_json::{Value, json};

/// How long a boot may take before the test calls it hung.
const BOOT_DEADLINE: Duration = Duration::from_secs(120);

/// How many times each recall is sent to one process.
const REPEATS: usize = 20;

// ---------------------------------------------------------------------------
// The lead binary (the role_split.rs pattern, with stdout kept for the
// enrollment token)
// ---------------------------------------------------------------------------

fn fresh_name() -> String {
    format!(
        "oml_test_repeat_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

/// A port no listener holds now: the kernel's pick, released at once.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("its address")
        .port()
}

fn drain(mut pipe: impl std::io::Read + Send + 'static) -> Arc<Mutex<String>> {
    let sink = Arc::new(Mutex::new(String::new()));
    let into = Arc::clone(&sink);
    std::thread::spawn(move || {
        let mut buf = [0_u8; 4096];
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            into.lock()
                .expect("pipe buffer")
                .push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });
    sink
}

/// A running `oml-server-lead` over `db`, with its own listener and the
/// keyring every lead of this database shares.
struct Lead {
    child: Child,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    port: u16,
}

impl Lead {
    /// Spawns the lead with its environment cleared, so no `OML_` variable
    /// of the caller's, a live key among them, reaches it, and with every
    /// search setting at its default: the pipeline under test is the one a
    /// deployment that configures nothing serves.
    fn spawn(db: &str, keyring: &std::path::Path) -> Self {
        let port = free_port();
        let mut child = Command::new(env!("CARGO_BIN_EXE_oml-server-lead"))
            .env_clear()
            .env("OML_POSTGRES__URL", serving_url(db))
            .env("OML_POSTGRES__OWNER_URL", owner_url(db))
            .env("OML_SERVER__LISTEN", format!("127.0.0.1:{port}"))
            .env("OML_KMS__KEYRING_PATH", keyring)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the oml-server-lead binary runs");
        let stdout = drain(child.stdout.take().expect("stdout is piped"));
        let stderr = drain(child.stderr.take().expect("stderr is piped"));
        Self {
            child,
            stdout,
            stderr,
            port,
        }
    }

    fn stderr(&self) -> String {
        self.stderr.lock().expect("stderr buffer").clone()
    }

    /// Waits until the lead accepts a TCP connection on its port.
    async fn listening(&mut self) {
        let started = Instant::now();
        loop {
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .is_ok()
            {
                return;
            }
            if let Some(status) = self.child.try_wait().expect("poll the lead") {
                panic!(
                    "the lead exited with {status} before it listened; stderr:\n{}",
                    self.stderr()
                );
            }
            if started.elapsed() > BOOT_DEADLINE {
                let _ = self.child.kill();
                panic!(
                    "the lead did not listen within {BOOT_DEADLINE:?}; stderr:\n{}",
                    self.stderr()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The single-use enrollment token an empty deployment prints: the line
    /// after its heading (deployment.md bootstrap).
    async fn enrollment_token(&self) -> String {
        let started = Instant::now();
        loop {
            let printed = self.stdout.lock().expect("stdout buffer").clone();
            let mut lines = printed.lines();
            if lines.any(|l| l.starts_with("OML bootstrap:"))
                && let Some(token) = lines.next()
            {
                return token.trim().to_owned();
            }
            assert!(
                started.elapsed() < BOOT_DEADLINE,
                "the lead printed no enrollment token; stdout:\n{printed}\nstderr:\n{}",
                self.stderr()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

impl Drop for Lead {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// HTTP, as any client speaks it
// ---------------------------------------------------------------------------

/// One request: the status and the body's bytes as served.
async fn send(
    http: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    key: Option<&str>,
    body: Option<&Value>,
) -> (u16, Vec<u8>) {
    let mut request = http.request(method, url);
    if let Some(key) = key {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    if let Some(body) = body {
        request = request
            .header("content-type", "application/json")
            .header(
                "idempotency-key",
                format!(
                    "repeat-{}-{}",
                    DB_N.fetch_add(1, Ordering::SeqCst),
                    run_tag()
                ),
            )
            .body(body.to_string());
    }
    let response = request.send().await.expect("the lead answers");
    let status = response.status().as_u16();
    let bytes = response.bytes().await.expect("a readable body").to_vec();
    (status, bytes)
}

/// A POST whose answer is JSON with the expected status.
async fn post(http: &reqwest::Client, lead: &Lead, path: &str, key: &str, body: Value) -> Value {
    post_expecting(http, lead, path, Some(key), body, &[200, 201, 202]).await
}

async fn post_expecting(
    http: &reqwest::Client,
    lead: &Lead,
    path: &str,
    key: Option<&str>,
    body: Value,
    statuses: &[u16],
) -> Value {
    let (status, bytes) = send(
        http,
        reqwest::Method::POST,
        lead.url(path),
        key,
        Some(&body),
    )
    .await;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(
        statuses.contains(&status),
        "POST {path} answered {status}: {text}"
    );
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("POST {path} is JSON ({e}): {text}"))
}

/// One recall's body as served, with the value of `recall_id` replaced and
/// nothing else touched, and the parsed answer beside it.
async fn recall(http: &reqwest::Client, lead: &Lead, key: &str, body: &Value) -> (String, Value) {
    let (status, bytes) = send(
        http,
        reqwest::Method::POST,
        lead.url("/v1/memories/recall"),
        Some(key),
        Some(body),
    )
    .await;
    let text = String::from_utf8(bytes).expect("a recall answers UTF-8");
    assert_eq!(status, 200, "recall {body}: {text}");
    let parsed: Value = serde_json::from_str(&text).expect("a recall answers JSON");
    let recall_id = parsed["recall_id"]
        .as_str()
        .expect("every recall names itself")
        .to_owned();
    assert_eq!(
        text.matches(recall_id.as_str()).count(),
        1,
        "the recall's id appears once in its answer: {text}"
    );
    (text.replace(recall_id.as_str(), "<recall_id>"), parsed)
}

/// The artifact ids of an answer's hits, in the served order.
fn hit_ids(answer: &Value) -> Vec<String> {
    answer["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| h["memory"]["artifact_id"].as_str().expect("id").to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// The world: one tenant, a writer, a reviewer, a reader, and a corpus with
// ties
// ---------------------------------------------------------------------------

struct World {
    db: String,
    scratch: std::path::PathBuf,
    http: reqwest::Client,
    first: Lead,
    writer: String,
    dreamer: String,
    reader: String,
    team: String,
}

impl World {
    fn keyring(&self) -> std::path::PathBuf {
        self.scratch.join("keyring")
    }

    /// A second lead process over the same database and keyring.
    async fn second_lead(&self) -> Lead {
        let mut lead = Lead::spawn(&self.db, &self.keyring());
        lead.listening().await;
        lead
    }

    /// Writes one memory as the writer; it is held.
    async fn remember(&self, workarea: &str, claim_key: &str, kind: &str, body: &str) -> String {
        self.remember_with(workarea, claim_key, kind, body, json!({}))
            .await
    }

    async fn remember_with(
        &self,
        workarea: &str,
        claim_key: &str,
        kind: &str,
        body: &str,
        more: Value,
    ) -> String {
        let mut request = json!({
            "verb": "remember", "claim_key": claim_key, "kind": kind, "body": body,
            "workarea": workarea,
        });
        if let (Some(request), Some(more)) = (request.as_object_mut(), more.as_object()) {
            request.extend(more.clone());
        }
        let accepted = post(
            &self.http,
            &self.first,
            "/v1/memories",
            &self.writer,
            request,
        )
        .await;
        accepted["artifact_id"]
            .as_str()
            .expect("artifact id")
            .to_owned()
    }

    /// The reviewer takes its next held item, which is `id`, and approves
    /// it: a key other than the author's (ADR-0001 d2).
    async fn approve(&self, id: &str) {
        let serve = post(
            &self.http,
            &self.first,
            "/v2/review/next",
            &self.dreamer,
            json!({}),
        )
        .await;
        assert_eq!(
            serve["item"]["artifact_id"], id,
            "the oldest held row is served: {serve}"
        );
        post(
            &self.http,
            &self.first,
            "/v2/review/verdicts",
            &self.dreamer,
            json!({ "serve_id": serve["serve_id"], "verdict": "approve" }),
        )
        .await;
    }

    async fn promoted(&self, workarea: &str, claim_key: &str, kind: &str, body: &str) -> String {
        let id = self.remember(workarea, claim_key, kind, body).await;
        self.approve(&id).await;
        id
    }

    async fn issue(
        lead: &Lead,
        http: &reqwest::Client,
        admin: &str,
        root: &str,
        name: &str,
        role: &str,
        grants: &[&str],
    ) -> String {
        let grants: Vec<Value> = grants
            .iter()
            .map(|role| json!({ "workarea": root, "role": role }))
            .collect();
        let issued = post(
            http,
            lead,
            "/v2/keys",
            admin,
            json!({
                "role": role, "name": name, "principal": format!("crew/{name}"),
                "held_by": "agent", "default_workarea": root, "grants": grants,
            }),
        )
        .await;
        issued["value"]
            .as_str()
            .expect("show-once value")
            .to_owned()
    }

    /// Boots the first lead over a fresh database, claims it, and writes
    /// and approves the corpus; `None`, with the reason printed, where
    /// `DATABASE_URL`'s role cannot mint the role pair.
    async fn boot() -> Option<Self> {
        let db = fresh_name();
        if !mint_database(&db).await {
            return None;
        }
        let scratch = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(&db);
        std::fs::create_dir_all(&scratch).expect("keyring scratch");
        let mut first = Lead::spawn(&db, &scratch.join("keyring"));
        first.listening().await;
        // The client speaks plain HTTP to loopback; reqwest still wants a
        // TLS provider named before it builds, as the lead's exporter does.
        let _already = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::new();
        let claimed = post_expecting(
            &http,
            &first,
            "/v2/bootstrap/claim",
            None,
            json!({
                "enrollment_token": first.enrollment_token().await,
                "name": "root-admin",
                "tenant_name": "repeat",
                "admin_principal": "platform/admin",
            }),
            &[201],
        )
        .await;
        let admin = claimed["value"].as_str().expect("admin key").to_owned();
        let root = claimed["root_workarea_id"]
            .as_str()
            .expect("root workarea")
            .to_owned();
        let team = post(
            &http,
            &first,
            "/v2/workareas",
            &admin,
            json!({ "parent": root, "name": "team" }),
        )
        .await["id"]
            .as_str()
            .expect("workarea id")
            .to_owned();
        let writer = Self::issue(
            &first,
            &http,
            &admin,
            &root,
            "writer",
            "writer",
            &["writer", "reader"],
        )
        .await;
        let dreamer = Self::issue(
            &first,
            &http,
            &admin,
            &root,
            "reviewer",
            "dreamer",
            &["dreamer"],
        )
        .await;
        let reader = Self::issue(
            &first,
            &http,
            &admin,
            &root,
            "reader",
            "reader",
            &["reader"],
        )
        .await;
        let world = Self {
            db,
            scratch,
            http,
            first,
            writer,
            dreamer,
            reader,
            team,
        };
        world.corpus(&root).await;
        Some(world)
    }

    /// Steering at two depths; twelve rows whose scores for `invoice` tie
    /// in threes, each the same words around one that differs; and rows
    /// that hold some of a longer query's words, in both workareas.
    async fn corpus(&self, root: &str) {
        self.promoted(
            root,
            "rule-sources",
            "steering",
            "Cite the ledger entry for every figure.",
        )
        .await;
        self.promoted(
            &self.team,
            "rule-currency",
            "steering",
            "State every amount in euros.",
        )
        .await;
        let fillers = [
            "amber", "birch", "cedar", "dune", "ember", "fjord", "grove", "heron", "inlet", "jade",
            "kelp", "lumen",
        ];
        for (i, filler) in fillers.iter().enumerate() {
            let (workarea, body) = match i % 4 {
                0 => (root, format!("The invoice for {filler} was paid.")),
                1 => (
                    root,
                    format!("An invoice and a second invoice name {filler} here."),
                ),
                2 => (
                    self.team.as_str(),
                    format!("The invoice for {filler} was paid."),
                ),
                _ => (
                    self.team.as_str(),
                    format!("Ledger reconciliation of the {filler} invoice runs quarterly."),
                ),
            };
            self.promoted(workarea, &format!("row-{filler}"), "memory", &body)
                .await;
        }
        self.promoted(
            root,
            "ledger-close",
            "derived_fact",
            "The ledger closes on the last working day; reconciliation follows within a week.",
        )
        .await;
        self.promoted(
            &self.team,
            "quarterly-report",
            "memory",
            "The quarterly report lists every reconciliation the ledger team ran.",
        )
        .await;
    }

    async fn close(self) {
        let Self {
            db, scratch, first, ..
        } = self;
        drop(first);
        drop_db(&db).await;
        let _ = std::fs::remove_dir_all(&scratch);
    }
}

/// The recalls every case repeats: the default request, a deeper `k`, every
/// word, a budget, text off, a child scope, a misspelling, and a sent
/// `now`. None names a pipeline, so each runs the deployment's default.
fn recalls(team: &str) -> Vec<Value> {
    vec![
        json!({ "query": "invoice" }),
        json!({ "query": "invoice ledger reconciliation quarterly", "k": 25 }),
        json!({ "query": "ledger reconciliation", "match": "all" }),
        json!({ "query": "invoice paid", "max_tokens": 60 }),
        json!({ "query": "quarterly invoice", "include_text": false, "k": 3 }),
        json!({ "query": "invoice", "workarea": team }),
        json!({ "query": "reconcilation ledgr" }),
        json!({ "query": "invoice ledger", "now": "2030-01-01T00:00:00Z" }),
    ]
}

// ---------------------------------------------------------------------------
// The laws
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_default_pipeline_answers_a_recall_with_the_same_bytes_every_time_and_from_a_second_process()
 {
    let Some(world) = World::boot().await else {
        return;
    };
    let recalls = recalls(&world.team);

    // One process, the same recall again and again.
    let mut expected = Vec::with_capacity(recalls.len());
    for body in &recalls {
        let (first, answer) = recall(&world.http, &world.first, &world.reader, body).await;
        assert_eq!(
            answer["search"]["pipeline"], "bm25-postings",
            "the recall names no pipeline, so the shipped default ranks it: {answer}"
        );
        for call in 1..REPEATS {
            let (again, _) = recall(&world.http, &world.first, &world.reader, body).await;
            assert_eq!(
                again, first,
                "call {call} of {body} differs from the first in one process"
            );
        }
        expected.push((first, answer));
    }

    // The control: the comparison sees a single differing value. Two
    // answers to one recall differ as served, by their `recall_id`.
    let (_, one) = recall(&world.http, &world.first, &world.reader, &recalls[0]).await;
    let (_, other) = recall(&world.http, &world.first, &world.reader, &recalls[0]).await;
    assert_ne!(
        one["recall_id"], other["recall_id"],
        "each call names itself, so the served bytes differ there and nowhere else"
    );

    // The cases are not vacuous: steering is served, hits tie, and a tie is
    // served by artifact id ascending (retrieval.md, order of the answer).
    let (_, plain) = &expected[0];
    assert_eq!(
        plain["steering"].as_array().map(Vec::len),
        Some(1),
        "{plain}"
    );
    let hits = plain["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 8, "the default k of eight is served: {plain}");
    let mut tied = 0;
    for pair in hits.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (sa, sb) = (
            a["score"].as_f64().expect("score"),
            b["score"].as_f64().expect("score"),
        );
        assert!(sa >= sb, "hits are served by score, highest first: {plain}");
        if sa == sb {
            tied += 1;
            assert!(
                a["memory"]["artifact_id"].as_str() < b["memory"]["artifact_id"].as_str(),
                "a tie is served by artifact id ascending: {plain}"
            );
        }
    }
    assert!(
        tied >= 4,
        "the corpus holds rows whose scores tie; {tied} adjacent ties: {plain}"
    );
    let (_, scoped) = &expected[5];
    assert_eq!(
        scoped["steering"].as_array().map(Vec::len),
        Some(2),
        "a child scope is served its ancestor's steering and its own: {scoped}"
    );
    let (_, budgeted) = &expected[3];
    assert_eq!(
        budgeted["budget"]["truncated"], true,
        "the budget cut the list: {budgeted}"
    );
    let (_, misspelt) = &expected[6];
    assert!(
        misspelt["search"]["did_you_mean"]
            .as_array()
            .is_some_and(|s| !s.is_empty()),
        "a misspelt query is offered spellings: {misspelt}"
    );

    // A second process over the same database, while the first still
    // serves: the same bytes from each, in turn.
    let second = world.second_lead().await;
    for round in 0..3 {
        for (body, (first, _)) in recalls.iter().zip(&expected) {
            let (from_second, _) = recall(&world.http, &second, &world.reader, body).await;
            assert_eq!(
                &from_second, first,
                "round {round}: the second process answers {body} differently from the first"
            );
            let (from_first, _) = recall(&world.http, &world.first, &world.reader, body).await;
            assert_eq!(
                &from_first, first,
                "round {round}: the first process answers {body} differently beside a second"
            );
        }
    }
    drop(second);

    // A process started after the first one stopped, as a restart is.
    let World {
        db,
        scratch,
        http,
        first,
        reader,
        ..
    } = world;
    drop(first);
    let mut third = Lead::spawn(&db, &scratch.join("keyring"));
    third.listening().await;
    for (body, (first, _)) in recalls.iter().zip(&expected) {
        let (after_restart, _) = recall(&http, &third, &reader, body).await;
        assert_eq!(
            &after_restart, first,
            "a lead started after the first stopped answers {body} differently"
        );
    }
    drop(third);
    drop_db(&db).await;
    let _ = std::fs::remove_dir_all(&scratch);
}

#[tokio::test]
async fn a_held_write_leaves_a_recall_unchanged_and_its_approval_changes_it_once() {
    let Some(world) = World::boot().await else {
        return;
    };
    let body = json!({ "query": "invoice ledger reconciliation quarterly", "k": 25 });
    let (before, _) = recall(&world.http, &world.first, &world.reader, &body).await;

    // A write that matches every word, held: no other key's recall moves,
    // in its hits, their scores or their order (ADR-0001 d3).
    let held = world
        .remember(
            &world.team,
            "row-new",
            "memory",
            "Quarterly ledger reconciliation of every invoice, invoice by invoice.",
        )
        .await;
    for call in 0..REPEATS {
        let (while_held, _) = recall(&world.http, &world.first, &world.reader, &body).await;
        assert_eq!(
            while_held, before,
            "call {call}: a held write changed another key's recall"
        );
    }

    // Its approval changes the answer, and the changed answer repeats.
    world.approve(&held).await;
    let (after, answer) = recall(&world.http, &world.first, &world.reader, &body).await;
    assert_ne!(
        after, before,
        "an approval changes the approved set and the answer"
    );
    assert!(
        hit_ids(&answer).contains(&held),
        "the approved row is a hit: {answer}"
    );
    for call in 1..REPEATS {
        let (again, _) = recall(&world.http, &world.first, &world.reader, &body).await;
        assert_eq!(again, after, "call {call} after the approval differs");
    }
    // A lead that was not running when the row was approved reads the
    // approved set from the database, not from anything it kept.
    let second = world.second_lead().await;
    let (from_second, _) = recall(&world.http, &second, &world.reader, &body).await;
    assert_eq!(
        from_second, after,
        "a second process answers the changed set alike"
    );
    drop(second);
    world.close().await;
}

#[tokio::test]
async fn a_recall_that_sends_now_reads_expiry_at_that_instant_and_repeats_whatever_the_clock() {
    let Some(world) = World::boot().await else {
        return;
    };
    let ending = world
        .remember_with(
            &world.team,
            "row-ending",
            "memory",
            "The invoice freeze holds until the audit ends.",
            json!({ "valid_until": "2031-06-30" }),
        )
        .await;
    world.approve(&ending).await;

    // The same request at two sent instants: the row is a hit before its
    // end and no hit after it, and each instant's answer repeats.
    let at = |now: &str| json!({ "query": "invoice freeze audit", "now": now });
    let (before_end, answer) = recall(
        &world.http,
        &world.first,
        &world.reader,
        &at("2031-01-01T00:00:00Z"),
    )
    .await;
    assert!(
        hit_ids(&answer).contains(&ending),
        "in force at the sent now: {answer}"
    );
    let (after_end, answer) = recall(
        &world.http,
        &world.first,
        &world.reader,
        &at("2032-01-01T00:00:00Z"),
    )
    .await;
    assert!(
        !hit_ids(&answer).contains(&ending),
        "ended at the sent now: {answer}"
    );
    let second = world.second_lead().await;
    for call in 0..REPEATS {
        for (now, expected) in [
            ("2031-01-01T00:00:00Z", &before_end),
            ("2032-01-01T00:00:00Z", &after_end),
        ] {
            let lead = if call % 2 == 0 { &world.first } else { &second };
            let (again, _) = recall(&world.http, lead, &world.reader, &at(now)).await;
            assert_eq!(&again, expected, "call {call} at now {now} differs");
        }
    }
    drop(second);
    world.close().await;
}
