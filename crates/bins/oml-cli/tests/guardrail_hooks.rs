//! The agent plugin's guardrail hooks through the real binary (proposed
//! ADR-0037 d6, d7, d8, d9; build steps S3 and S4): `oml hook guardrails`
//! against a fixture that answers `GET /v1/guardrails` with a canned set
//! and counts what it was sent, and `oml hook pre-tool-use` against the
//! rule cache the first writes. The fixture is a test double, not a served
//! route; the route is proven in `oml-server-lead/tests/guardrails_http.rs`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::get;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_guardrail::{CachedRule, Rule, encode_cache};

const KEY: &str = "oml_agent_fixture_value_0123456789abcdef";
const SESSION: &str = "5f1c2a3e-0b7d-4e59-9a41-2c8d5e7f9b10";
const DENY_ID: &str = "00000000-0000-4000-8000-0000000000d1";
const ASK_ID: &str = "00000000-0000-4000-8000-0000000000a1";

/// An empty directory of this test's own, under the OS temp directory.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-gh-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn deny_push() -> Value {
    json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"], "effect": "deny"})
}

fn ask_commit() -> Value {
    json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "commit", "*"], "effect": "ask"})
}

fn served(id: &str, claim_key: &str, severity: &str, text: &str, rule: Value) -> Value {
    json!({
        "artifact_id": id, "claim_key": claim_key, "version": 2,
        "workarea_id": "00000000-0000-4000-8000-0000000000e1",
        "workarea_path": "universe.eng", "depth": 1,
        "severity": severity, "text": text, "rule": rule, "tokens": 5,
        "promoted_at": "2026-10-31T10:00:00.000000Z",
        "verdicts": [
            {"held_by": "human", "at": "2026-10-31T10:00:00.000000Z"},
            {"held_by": "agent", "at": "2026-10-31T09:00:00.000000Z"}
        ],
        "conflict": []
    })
}

fn the_set() -> Value {
    json!({
        "scope_node": "00000000-0000-4000-8000-0000000000e2",
        "scope_path": "universe.eng.api",
        "guardrail_scope": "ancestors",
        "set_digest": "sha256:feed",
        "over_cap": false,
        "guardrails": [
            served(DENY_ID, "no-push", "never", "Never push to main.", deny_push()),
            served(ASK_ID, "tests-first", "must", "Run the tests before every commit.", ask_commit()),
            served("00000000-0000-4000-8000-0000000000b1", "small-prs", "should", "Keep pull requests small.", Value::Null),
        ],
        "own_held": [{"claim_key": "lint-first", "version": 1}]
    })
}

/// A fixture serving `set` at `GET /v1/guardrails`; its URL and the
/// requests it answered.
async fn fixture(set: Value) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let auths: Arc<Mutex<Vec<String>>> = Arc::default();
    type S = (Value, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>);
    async fn guardrails(State((set, hits, auths)): State<S>, headers: HeaderMap) -> Json<Value> {
        hits.fetch_add(1, Ordering::SeqCst);
        auths.lock().expect("lock").push(
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        );
        Json(set)
    }
    let router = axum::Router::new()
        .route("/v1/guardrails", get(guardrails))
        .with_state((set, hits.clone(), auths.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture bind");
    let url = format!("http://{}", listener.local_addr().expect("fixture addr"));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("fixture serve");
    });
    (url, hits, auths)
}

/// `oml hook <verb>` with `stdin`, `XDG_STATE_HOME` under `dir`, and the
/// agent's pair when `url` is given.
fn hook(verb: &str, dir: &Path, url: Option<&str>, stdin: &Value) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
    for name in [
        "OML_URL",
        "OML_API_KEY",
        "OML_CLI_KEY",
        "OML_CLI_SERVER",
        "CLAUDE_PLUGIN_OPTION_URL",
        "CLAUDE_PLUGIN_OPTION_API_KEY",
        "XDG_CACHE_HOME",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("XDG_STATE_HOME", dir.join("state"))
        .env("HOME", dir)
        .env("OML_CLI_CREDENTIALS", dir.join("credentials"));
    if let Some(url) = url {
        cmd.env("OML_URL", url).env("OML_API_KEY", KEY);
    }
    let mut child = cmd
        .args(["hook", verb])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the oml binary runs");
    let _ = child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin.to_string().as_bytes());
    child.wait_with_output().expect("the oml binary ends")
}

fn start(source: &str) -> Value {
    json!({"session_id": SESSION, "hook_event_name": "SessionStart", "source": source, "cwd": "/work/repo"})
}

fn tool(tool_name: &str, tool_input: Value) -> Value {
    json!({"session_id": SESSION, "hook_event_name": "PreToolUse", "tool_name": tool_name,
           "tool_input": tool_input, "cwd": "/work/repo"})
}

fn stdout_json(out: &Output) -> Value {
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(text.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is one JSON document ({e}): {text}; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn context(out: &Output) -> String {
    stdout_json(out)["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn cache_file(dir: &Path) -> PathBuf {
    dir.join("state/oml/guardrails")
        .join(format!("{SESSION}.json"))
}

/// A cache holding `rules`, written as `oml hook guardrails` writes it.
fn write_cache(dir: &Path, rules: &[(&str, &str, Value)]) {
    let rules: Vec<CachedRule> = rules
        .iter()
        .map(|(id, body, rule)| CachedRule {
            artifact_id: Uuid::parse_str(id).expect("an id"),
            body: (*body).to_owned(),
            rule: Rule::parse(rule).expect("an oml-gr/1 rule"),
        })
        .collect();
    let path = cache_file(dir);
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("the cache directory");
    std::fs::write(&path, encode_cache(&rules)).expect("the cache file");
}

fn bash(command: &str) -> Value {
    tool("Bash", json!({"command": command}))
}

// --- S3: oml hook guardrails --------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_line_names_its_workarea_claim_key_version_and_reviewer_count() {
    let (url, hits, _) = fixture(the_set()).await;
    let dir = scratch("lines");
    let out = hook("guardrails", &dir, Some(&url), &start("startup"));
    assert!(out.status.success());
    assert_eq!(hits.load(Ordering::SeqCst), 1, "one GET /v1/guardrails");
    let text = context(&out);
    assert!(
        text.contains("- [never] Never push to main. (universe.eng, no-push v2, ratified 2026-10-31 by 2 reviewers (1 human); checked: deny shell [\"git\",\"push\",\"*\"])"),
        "{text}"
    );
    assert!(
        text.contains("- [should] Keep pull requests small. (universe.eng, small-prs v2, ratified 2026-10-31 by 2 reviewers (1 human))"),
        "a line with no rule is text only: {text}"
    );
    assert!(
        text.contains("lint-first v1"),
        "the caller's own held guardrail is listed: {text}"
    );
    assert_eq!(
        stdout_json(&out)["hookSpecificOutput"]["hookEventName"],
        "SessionStart"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_header_names_the_guardrail_scope_the_server_applied() {
    let (url, _, _) = fixture(the_set()).await;
    let dir = scratch("header");
    let text = context(&hook("guardrails", &dir, Some(&url), &start("startup")));
    let first = text.lines().next().unwrap_or_default();
    assert!(
        first.starts_with("OML guardrails for this session, from ")
            && first.contains("workarea universe.eng.api, guardrail scope ancestors:"),
        "{first}"
    );
    assert!(text.contains("These are instructions"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conflict_line_names_the_other_artifact_ids() {
    let mut set = the_set();
    set["guardrails"][0]["conflict"] = json!([ASK_ID]);
    set["guardrails"][1]["conflict"] = json!([DENY_ID]);
    let (url, _, _) = fixture(set).await;
    let dir = scratch("conflict");
    let text = context(&hook("guardrails", &dir, Some(&url), &start("startup")));
    let line = text
        .lines()
        .find(|l| l.contains("Never push to main."))
        .unwrap_or_default();
    assert!(
        line.ends_with(&format!("conflicts with {ASK_ID}")),
        "{line}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_the_cap_no_guardrail_line_is_printed_and_the_user_is_told() {
    let mut set = the_set();
    let many: Vec<Value> = (0..13)
        .map(|i| {
            served(
                &format!("00000000-0000-4000-8000-0000000001{i:02}"),
                &format!("rule-{i}"),
                "must",
                &format!("Rule number {i}."),
                Value::Null,
            )
        })
        .collect();
    set["guardrails"] = json!(many);
    set["over_cap"] = json!(true);
    let (url, _, _) = fixture(set).await;
    let dir = scratch("over-cap");
    let out = hook("guardrails", &dir, Some(&url), &start("startup"));
    let text = context(&out);
    assert!(!text.contains("Rule number"), "{text}");
    assert!(
        text.contains("13 guardrails apply here, above the cap of 12, so none is shown"),
        "{text}"
    );
    assert!(
        stdout_json(&out)["systemMessage"]
            .as_str()
            .is_some_and(|m| m.contains("13 guardrails")),
        "the user is told: {}",
        stdout_json(&out)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_compact_source_prints_the_whole_set_again() {
    let (url, hits, _) = fixture(the_set()).await;
    let dir = scratch("compact");
    let first = context(&hook("guardrails", &dir, Some(&url), &start("startup")));
    let again = context(&hook("guardrails", &dir, Some(&url), &start("compact")));
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(first, again);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_key_value_is_redacted_in_every_line() {
    let mut set = the_set();
    set["guardrails"][2]["text"] = json!(format!("Never paste {KEY} anywhere."));
    let (url, _, auths) = fixture(set).await;
    let dir = scratch("redact");
    let out = hook("guardrails", &dir, Some(&url), &start("startup"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains(KEY), "{stdout}");
    assert!(stdout.contains("[OML_API_KEY]"), "{stdout}");
    assert_eq!(
        auths.lock().expect("lock").as_slice(),
        [format!("Bearer {KEY}")],
        "the agent's own key, sent once"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_cannot_be_reached_exits_0_and_tells_the_user() {
    let dir = scratch("unreachable");
    let out = hook(
        "guardrails",
        &dir,
        Some("http://127.0.0.1:9"),
        &start("startup"),
    );
    assert!(out.status.success());
    let doc = stdout_json(&out);
    assert!(
        doc["systemMessage"]
            .as_str()
            .is_some_and(|m| m.contains("OML guardrails were not loaded")),
        "{doc}"
    );
    assert!(!cache_file(&dir).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_writes_the_cache_the_check_denies_from() {
    let (url, _, _) = fixture(the_set()).await;
    let dir = scratch("roundtrip");
    let out = hook("guardrails", &dir, Some(&url), &start("startup"));
    assert!(out.status.success());
    assert!(cache_file(&dir).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&cache_file(&dir)), 0o600);
        assert_eq!(mode(cache_file(&dir).parent().expect("dir")), 0o700);
    }
    let out = hook("pre-tool-use", &dir, None, &bash("git push origin main"));
    let doc = stdout_json(&out);
    assert_eq!(
        doc["hookSpecificOutput"]["permissionDecision"], "deny",
        "{doc}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subagent_start_writes_no_cache() {
    let (url, _, _) = fixture(the_set()).await;
    let dir = scratch("subagent");
    let stdin =
        json!({"session_id": SESSION, "hook_event_name": "SubagentStart", "agent_type": "Explore"});
    let out = hook("guardrails", &dir, Some(&url), &stdin);
    assert!(out.status.success());
    assert_eq!(
        stdout_json(&out)["hookSpecificOutput"]["hookEventName"],
        "SubagentStart"
    );
    assert!(!cache_file(&dir).exists());
    // A subagent's lines are checked by its session's cache, which only a
    // SessionStart writes; none is claimed here.
    assert!(!context(&out).contains("checked:"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cache_file_older_than_thirty_days_is_removed_at_start() {
    let (url, _, _) = fixture(the_set()).await;
    let dir = scratch("sweep");
    let cache_dir = dir.join("state/oml/guardrails");
    std::fs::create_dir_all(&cache_dir).expect("cache dir");
    let old = cache_dir.join("old-session.json");
    let recent = cache_dir.join("recent-session.json");
    std::fs::write(&old, "x").expect("old");
    std::fs::write(&recent, "x").expect("recent");
    std::fs::File::options()
        .write(true)
        .open(&old)
        .expect("open")
        .set_modified(SystemTime::now() - Duration::from_secs(31 * 24 * 3600))
        .expect("age it");
    let out = hook("guardrails", &dir, Some(&url), &start("startup"));
    assert!(out.status.success());
    assert!(!old.exists(), "the 31-day-old cache is removed");
    assert!(recent.exists(), "a recent cache is kept");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cache_not_written_prints_no_line_as_checked_and_names_the_cause() {
    let (url, _, _) = fixture(the_set()).await;
    let dir = scratch("unwritable");
    // The state directory is a file, so no cache directory can be made.
    std::fs::write(dir.join("state"), "not a directory").expect("a file in its place");
    let out = hook("guardrails", &dir, Some(&url), &start("startup"));
    assert!(out.status.success());
    let doc = stdout_json(&out);
    let text = context(&out);
    assert!(text.contains("Never push to main."), "{text}");
    assert!(!text.contains("checked:"), "{text}");
    assert!(
        doc["systemMessage"]
            .as_str()
            .is_some_and(|m| m.contains("not checked")),
        "{doc}"
    );
}

// --- S4: oml hook pre-tool-use ------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_matching_deny_rule_answers_deny_with_the_guardrail_id_and_body() {
    let dir = scratch("deny");
    write_cache(&dir, &[(DENY_ID, "Never push to main.", deny_push())]);
    let out = hook("pre-tool-use", &dir, None, &bash("git push origin main"));
    assert!(out.status.success());
    let doc = stdout_json(&out);
    let decision = &doc["hookSpecificOutput"];
    assert_eq!(decision["hookEventName"], "PreToolUse");
    assert_eq!(decision["permissionDecision"], "deny");
    let reason = decision["permissionDecisionReason"]
        .as_str()
        .unwrap_or_default();
    assert!(
        reason.contains(DENY_ID) && reason.contains("Never push to main."),
        "{reason}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deny_wins_over_ask() {
    let dir = scratch("deny-wins");
    let ask_push =
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "*"], "effect": "ask"});
    write_cache(
        &dir,
        &[
            (ASK_ID, "Ask before git.", ask_push),
            (DENY_ID, "Never push to main.", deny_push()),
        ],
    );
    let doc = stdout_json(&hook("pre-tool-use", &dir, None, &bash("git push")));
    assert_eq!(doc["hookSpecificOutput"]["permissionDecision"], "deny");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_input_ever_answers_allow() {
    let dir = scratch("no-allow");
    write_cache(&dir, &[(DENY_ID, "Never push to main.", deny_push())]);
    for input in [
        bash("git status"),
        tool("Read", json!({"file_path": "/work/repo/README.md"})),
        tool("WebSearch", json!({"query": "git push"})),
    ] {
        let out = hook("pre-tool-use", &dir, None, &input);
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.trim().is_empty(), "nothing, never allow: {stdout}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_check_sends_no_request() {
    let (url, hits, _) = fixture(the_set()).await;
    let dir = scratch("no-request");
    write_cache(&dir, &[(DENY_ID, "Never push to main.", deny_push())]);
    let out = hook("pre-tool-use", &dir, Some(&url), &bash("git push"));
    assert_eq!(
        stdout_json(&out)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    let out = hook(
        "pre-tool-use",
        &scratch("no-request-no-cache"),
        Some(&url),
        &bash("git push"),
    );
    assert!(out.status.success());
    assert_eq!(hits.load(Ordering::SeqCst), 0, "no request, cache or none");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreadable_cache_denies_every_call() {
    let dir = scratch("unreadable");
    let path = cache_file(&dir);
    std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
    std::fs::write(&path, "not a cache").expect("garbage");
    let doc = stdout_json(&hook("pre-tool-use", &dir, None, &bash("ls")));
    assert_eq!(doc["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(
        doc["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .is_some_and(|r| r.contains("restart the session")),
        "{doc}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cache_whose_digest_line_does_not_match_denies_every_call() {
    let dir = scratch("digest");
    write_cache(&dir, &[(DENY_ID, "Never push to main.", deny_push())]);
    let path = cache_file(&dir);
    let text = std::fs::read_to_string(&path).expect("cache");
    std::fs::write(&path, text.replace("Never push", "Always push")).expect("tampered");
    let doc = stdout_json(&hook("pre-tool-use", &dir, None, &bash("git status")));
    assert_eq!(doc["hookSpecificOutput"]["permissionDecision"], "deny");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_with_no_cache_decides_nothing_and_warns_in_one_line() {
    let dir = scratch("no-cache");
    let out = hook("pre-tool-use", &dir, None, &bash("git push"));
    assert!(out.status.success());
    let doc = stdout_json(&out);
    assert!(doc.get("hookSpecificOutput").is_none(), "{doc}");
    let warning = doc["systemMessage"].as_str().unwrap_or_default();
    assert!(
        warning.starts_with("OML guardrails are not loaded for this session")
            && !warning.contains('\n'),
        "{warning}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_edit_of_the_cache_directory_asks() {
    let dir = scratch("cache-edit");
    write_cache(&dir, &[(DENY_ID, "Never push to main.", deny_push())]);
    let target = cache_file(&dir);
    let doc = stdout_json(&hook(
        "pre-tool-use",
        &dir,
        None,
        &tool(
            "Write",
            json!({"file_path": target.to_str().expect("utf-8"), "content": "{}"}),
        ),
    ));
    assert_eq!(doc["hookSpecificOutput"]["permissionDecision"], "ask");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_id_that_names_no_cache_file_blocks_the_call() {
    let dir = scratch("bad-session");
    let stdin = json!({"session_id": "../etc", "tool_name": "Bash",
                       "tool_input": {"command": "ls"}, "cwd": "/work"});
    let out = hook("pre-tool-use", &dir, None, &stdin);
    assert_eq!(out.status.code(), Some(2), "a blocking error");
    assert!(String::from_utf8_lossy(&out.stderr).contains("session_id"));
}
