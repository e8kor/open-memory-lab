//! Implicit capture's agent layer through the real binary (ADR-0064 d11 to
//! d15): the pre-tool hook fills the anchor, the origin and the workarea of
//! an `oml_write`; the post-tool hook records the paths an edit touched;
//! Stop asks for a reviewed supersession of a claim whose span changed.
#![cfg(unix)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use oml_cli::capture::agent::sync_due;
use oml_cli::capture::Mode;
use oml_cli::sync::hash::md_hash;
use serde_json::{Value, json};

const OML: &str = env!("CARGO_BIN_EXE_oml");
const WRITE_TOOL: &str = "mcp__plugin_oml_oml__oml_write";
const SESSION: &str = "sess-1";

const LIB: &str = "// widgets\n\
pub fn parse_header(input: &str) -> usize {\n\
    let mut n = 0;\n\
    for _ in input.lines() { n += 1; }\n\
    n\n\
}\n\
\n\
pub fn render_footer() -> &'static str {\n\
    \"footer\"\n\
}\n";

fn scratch(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("oml-cap-agent-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("home")).unwrap();
    root
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn write(dir: &Path, path: &str, text: &str) {
    let p = dir.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn clone(test: &str, mode: &str) -> PathBuf {
    let root = scratch(test);
    let dir = root.join("clone");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["remote", "add", "origin", "https://github.com/acme/widgets.git"]);
    write(&dir, "src/lib.rs", LIB);
    write(&dir, "src/other.rs", "pub fn other() {}\n");
    write(&dir, "src/third.rs", "pub fn third() {}\n");
    write(&dir, ".gitignore", "ignored.rs\n");
    write(&dir, ".env", "TOKEN=abc\n");
    write(&dir, "ignored.rs", "pub fn hidden() {}\n");
    git(&dir, &["add", "-f", "."]);
    git(&dir, &["reset", "-q", ".env", "ignored.rs"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    git(&dir, &["config", "--local", "oml.mode", mode]);
    git(&dir, &["config", "--local", "oml.workarea", "pat.widgets-3fa2c1"]);
    dir
}

fn home(dir: &Path) -> PathBuf {
    dir.parent().unwrap().join("home")
}

fn hook(dir: &Path, verb: &str, input: &Value) -> Output {
    let h = home(dir);
    let mut child = Command::new(OML)
        .current_dir(dir)
        .args(["hook", verb])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", &h)
        .env("XDG_CACHE_HOME", h.join("cache"))
        .env("XDG_STATE_HOME", h.join("state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn pre_write(dir: &Path, tool_input: &Value) -> Output {
    hook(
        dir,
        "pre-tool-use",
        &json!({
            "session_id": SESSION, "hook_event_name": "PreToolUse", "cwd": dir,
            "tool_name": WRITE_TOOL, "tool_input": tool_input,
        }),
    )
}

fn edited(dir: &Path, path: &str) -> Output {
    hook(
        dir,
        "post-tool-use",
        &json!({
            "session_id": SESSION, "hook_event_name": "PostToolUse", "cwd": dir,
            "tool_name": "Edit", "tool_input": {"file_path": dir.join(path)},
        }),
    )
}

fn read_file(dir: &Path, path: &str) -> Output {
    hook(
        dir,
        "post-tool-use",
        &json!({
            "session_id": SESSION, "hook_event_name": "PostToolUse", "cwd": dir,
            "tool_name": "Read", "tool_input": {"file_path": dir.join(path)},
        }),
    )
}

/// The completed input of an allowed rewrite, or None when the call goes
/// through unchanged.
fn updated(out: &Output) -> Option<Value> {
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    if text.trim().is_empty() {
        return None;
    }
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    doc["hookSpecificOutput"]["updatedInput"].as_object().map(|_| doc["hookSpecificOutput"]["updatedInput"].clone())
}

fn denied(out: &Output) -> bool {
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    doc["hookSpecificOutput"]["permissionDecision"] == "deny"
}

fn session_file(dir: &Path) -> PathBuf {
    home(dir).join("cache/oml/sessions").join(format!("{SESSION}.capture.json"))
}

#[test]
fn pre_tool_use_anchors_the_one_named_path_and_fills_origin_and_workarea() {
    let dir = clone("named", "agent");
    let input = updated(&pre_write(&dir, &json!({"text": "src/lib.rs counts the lines of its input"}))).expect("anchored");
    assert_eq!(input["anchor"]["path"], "src/lib.rs");
    assert_eq!(input["anchor"]["repo"], "acme/widgets");
    assert_eq!(input["anchor"]["rev"], git(&dir, &["rev-parse", "HEAD"]).as_str());
    assert_eq!(input["origin"]["kind"], "code");
    assert_eq!(input["workarea"], "pat.widgets-3fa2c1");
    assert_eq!(input["text"], "src/lib.rs counts the lines of its input", "the sentence is the agent's");
}

#[test]
fn markdown_is_origin_doc_and_an_agent_origin_is_kept() {
    let dir = clone("origin", "agent");
    write(&dir, "docs/guide.md", "# Guide\n\nwords\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "doc"]);
    let input = updated(&pre_write(&dir, &json!({"text": "docs/guide.md explains the setup"}))).expect("anchored");
    assert_eq!(input["origin"]["kind"], "doc");
    let input = updated(&pre_write(&dir, &json!({"text": "docs/guide.md again", "origin": {"kind": "chat"}}))).expect("anchored");
    assert_eq!(input["origin"]["kind"], "chat", "the agent's origin stands");
}

#[test]
fn two_named_paths_anchor_nothing() {
    let dir = clone("two-named", "agent");
    assert!(updated(&pre_write(&dir, &json!({"text": "src/lib.rs and src/other.rs share a parser"}))).is_none());
}

#[test]
fn one_touched_file_is_anchored_when_none_is_named_and_two_are_not() {
    let dir = clone("touched", "agent");
    assert!(edited(&dir, "src/lib.rs").status.success());
    let input = updated(&pre_write(&dir, &json!({"text": "the header parser counts lines"}))).expect("anchored");
    assert_eq!(input["anchor"]["path"], "src/lib.rs");
    // The previous write resets the touched set; two edits since anchor nothing.
    assert!(edited(&dir, "src/lib.rs").status.success());
    assert!(edited(&dir, "src/other.rs").status.success());
    assert!(updated(&pre_write(&dir, &json!({"text": "a second, unrelated fact"}))).is_none());
}

#[test]
fn one_read_file_is_anchored_when_nothing_was_edited_and_two_are_not() {
    let dir = clone("read", "agent");
    assert!(read_file(&dir, "src/other.rs").status.success());
    let input = updated(&pre_write(&dir, &json!({"text": "this module is a stub"}))).expect("anchored");
    assert_eq!(input["anchor"]["path"], "src/other.rs");
    assert!(read_file(&dir, "src/other.rs").status.success());
    assert!(read_file(&dir, "src/third.rs").status.success());
    assert!(updated(&pre_write(&dir, &json!({"text": "both are stubs"}))).is_none());
}

#[test]
fn a_secret_shaped_or_ignored_file_is_never_opened_or_chosen() {
    let dir = clone("secret", "agent");
    assert!(updated(&pre_write(&dir, &json!({"text": ".env holds the deploy token"}))).is_none());
    assert!(updated(&pre_write(&dir, &json!({"text": "ignored.rs hides a function"}))).is_none());
    assert!(edited(&dir, ".env").status.success());
    assert!(edited(&dir, "ignored.rs").status.success());
    let state = std::fs::read_to_string(session_file(&dir)).unwrap_or_default();
    assert!(!state.contains(".env") && !state.contains("ignored.rs"), "not recorded: {state}");
    // A path outside the clone is not recorded either.
    let out = hook(&dir, "post-tool-use", &json!({
        "session_id": SESSION, "cwd": dir, "tool_name": "Edit", "tool_input": {"file_path": "/etc/hosts"},
    }));
    assert!(out.status.success());
}

#[test]
fn a_unique_definition_gives_a_line_span_and_a_hash() {
    let dir = clone("span", "agent");
    let input = updated(&pre_write(&dir, &json!({"text": "parse_header in src/lib.rs counts lines"}))).expect("anchored");
    assert_eq!(input["anchor"]["fragment"], "L2-L6", "the indentation block of the definition line");
    let span = LIB.split('\n').collect::<Vec<_>>()[1..6].join("\n");
    assert_eq!(input["anchor"]["hash"], md_hash(&span).as_str());
    assert_eq!(input["anchor"]["hasher"], "oml-md/1");
}

#[test]
fn no_unique_definition_gives_an_unhashed_file_anchor() {
    let dir = clone("unhashed", "agent");
    let input = updated(&pre_write(&dir, &json!({"text": "src/lib.rs is the crate root"}))).expect("anchored");
    assert_eq!(input["anchor"]["path"], "src/lib.rs");
    assert!(input["anchor"].get("fragment").is_none(), "{input}");
    assert!(input["anchor"].get("hash").is_none(), "{input}");
}

#[test]
fn the_hash_is_of_the_working_tree_span_and_the_rev_is_head() {
    let dir = clone("worktree", "agent");
    let edited_lib = LIB.replace("n += 1", "n += 2");
    write(&dir, "src/lib.rs", &edited_lib);
    let input = updated(&pre_write(&dir, &json!({"text": "parse_header in src/lib.rs counts by twos"}))).expect("anchored");
    let span = edited_lib.split('\n').collect::<Vec<_>>()[1..6].join("\n");
    assert_eq!(input["anchor"]["hash"], md_hash(&span).as_str());
    assert_eq!(input["anchor"]["rev"], git(&dir, &["rev-parse", "HEAD"]).as_str());
}

#[test]
fn an_anchor_the_agent_sent_is_completed_never_replaced() {
    let dir = clone("completed", "agent");
    let input = updated(&pre_write(&dir, &json!({"text": "src/other.rs and src/third.rs", "anchor": {"path": "src/lib.rs", "fragment": "L8-L10"}}))).expect("completed");
    assert_eq!(input["anchor"]["path"], "src/lib.rs", "never replaced");
    assert_eq!(input["anchor"]["fragment"], "L8-L10");
    assert_eq!(input["anchor"]["repo"], "acme/widgets");
    let span = LIB.split('\n').collect::<Vec<_>>()[7..10].join("\n");
    assert_eq!(input["anchor"]["hash"], md_hash(&span).as_str());
    assert_eq!(input["workarea"], "pat.widgets-3fa2c1");
}

#[test]
fn three_copied_lines_and_a_fenced_block_are_refused_once() {
    let dir = clone("copied", "agent");
    let copied = "src/lib.rs has\n    let mut n = 0;\n    for _ in input.lines() { n += 1; }\n    n\n";
    let first = pre_write(&dir, &json!({"text": copied}));
    assert!(denied(&first), "{}", String::from_utf8_lossy(&first.stdout));
    assert!(String::from_utf8_lossy(&first.stdout).contains("three"), "the reason says why");
    let again = pre_write(&dir, &json!({"text": copied}));
    assert!(!denied(&again), "the same call again goes through");
    let fenced = pre_write(&dir, &json!({"text": "src/other.rs has\n```rust\nfn x() {}\n```\n"}));
    assert!(denied(&fenced));
}

#[test]
fn a_duplicate_anchor_is_refused_once_unless_it_replaces() {
    let dir = clone("duplicate", "agent");
    let first = updated(&pre_write(&dir, &json!({"text": "parse_header in src/lib.rs"}))).expect("anchored");
    // The session's listing holds a claim on that path and fragment.
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(session_file(&dir)).unwrap()).unwrap();
    state["claims"] = json!([{
        "artifact_id": "11111111-1111-4111-8111-111111111111", "version": 2, "own": true,
        "path": "src/lib.rs", "fragment": first["anchor"]["fragment"], "hash": first["anchor"]["hash"],
        "hasher": "oml-md/1", "kind": "memory",
    }]);
    state["anchored_writes"] = json!(0);
    std::fs::write(session_file(&dir), state.to_string()).unwrap();
    let again = pre_write(&dir, &json!({"text": "parse_header in src/lib.rs, said differently"}));
    assert!(denied(&again), "{}", String::from_utf8_lossy(&again.stdout));
    let replay = pre_write(&dir, &json!({"text": "parse_header in src/lib.rs, said differently"}));
    assert!(!denied(&replay), "refused once per call");
    let replaces = pre_write(&dir, &json!({"text": "parse_header, revised in src/lib.rs", "replaces": "11111111-1111-4111-8111-111111111111", "expected_version": 2}));
    assert!(!denied(&replaces), "a replacement names the claim it supersedes");
}

#[test]
fn the_sixth_anchored_write_of_a_session_is_refused() {
    let dir = clone("sixth", "agent");
    for i in 1..=5 {
        let out = pre_write(&dir, &json!({"text": format!("src/lib.rs fact number {i}")}));
        assert!(updated(&out).is_some(), "write {i}");
    }
    assert!(denied(&pre_write(&dir, &json!({"text": "src/lib.rs fact number 6"}))));
}

#[test]
fn a_clone_whose_mode_excludes_agent_prints_nothing() {
    for mode in ["git", "off"] {
        let dir = clone(&format!("silent-{mode}"), mode);
        let out = pre_write(&dir, &json!({"text": "src/lib.rs counts lines"}));
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).trim().is_empty(), "{mode}: {}", String::from_utf8_lossy(&out.stdout));
        let out = edited(&dir, "src/lib.rs");
        assert!(out.status.success() && out.stdout.is_empty());
        assert!(!session_file(&dir).exists(), "{mode}: nothing recorded");
    }
    let dir = scratch("not-a-clone");
    let out = pre_write(&dir, &json!({"text": "src/lib.rs"}));
    assert!(out.status.success());
}

#[test]
fn post_tool_use_prints_nothing_and_sends_nothing() {
    let dir = clone("post", "agent");
    // The server named is unreachable; the hook has no business with it.
    let started = std::time::Instant::now();
    let out = edited(&dir, "src/lib.rs");
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
}

fn seed_claims(dir: &Path, claims: Value) {
    let path = session_file(dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, json!({"claims": claims, "touched": [], "read": []}).to_string()).unwrap();
}

fn stop(dir: &Path, active: bool) -> Output {
    hook(dir, "stop", &json!({"session_id": SESSION, "hook_event_name": "Stop", "cwd": dir, "stop_hook_active": active}))
}

fn claim(id: &str, own: bool, fragment: &str, text_of_span: &str) -> Value {
    json!({
        "artifact_id": id, "version": 3, "own": own, "path": "src/lib.rs",
        "fragment": fragment, "hash": md_hash(text_of_span), "hasher": "oml-md/1", "kind": "memory",
    })
}

#[test]
fn stop_lists_only_own_claims_whose_span_changed_and_blocks_once() {
    let dir = clone("stop", "agent");
    let old_span = LIB.split('\n').collect::<Vec<_>>()[1..6].join("\n");
    seed_claims(&dir, json!([
        claim("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", true, "L2-L6", &old_span),
        claim("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", false, "L2-L6", &old_span),
        claim("cccccccc-cccc-4ccc-8ccc-cccccccccccc", true, "L8-L10", &LIB.split('\n').collect::<Vec<_>>()[7..10].join("\n")),
    ]));
    write(&dir, "src/lib.rs", &LIB.replace("n += 1", "n += 2"));
    assert!(edited(&dir, "src/lib.rs").status.success());
    let out = stop(&dir, false);
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).expect("a block document");
    assert_eq!(doc["decision"], "block");
    let reason = doc["reason"].as_str().unwrap();
    assert!(reason.contains("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"), "{reason}");
    assert!(reason.contains("expected_version") && reason.contains("replaces"), "{reason}");
    assert!(reason.contains("confirm") && reason.contains("update") && reason.contains("retire"), "{reason}");
    assert!(!reason.contains("bbbbbbbb"), "another key's claim is counted, not listed: {reason}");
    assert!(reason.contains("1 other"), "{reason}");
    assert!(!reason.contains("cccccccc"), "the unchanged span is not listed: {reason}");
    // Once: the same claims are not asked about again.
    let again = stop(&dir, false);
    assert!(again.stdout.is_empty(), "{}", String::from_utf8_lossy(&again.stdout));
}

#[test]
fn stop_does_not_block_when_stop_hook_active() {
    let dir = clone("stop-active", "agent");
    let old_span = LIB.split('\n').collect::<Vec<_>>()[1..6].join("\n");
    seed_claims(&dir, json!([claim("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", true, "L2-L6", &old_span)]));
    write(&dir, "src/lib.rs", &LIB.replace("n += 1", "n += 2"));
    assert!(edited(&dir, "src/lib.rs").status.success());
    assert!(stop(&dir, true).stdout.is_empty());
}

#[test]
fn a_span_moved_within_the_file_is_not_listed() {
    let dir = clone("moved", "agent");
    let old_span = LIB.split('\n').collect::<Vec<_>>()[1..6].join("\n");
    seed_claims(&dir, json!([claim("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", true, "L2-L6", &old_span)]));
    write(&dir, "src/lib.rs", &format!("// one\n// two\n{LIB}"));
    assert!(edited(&dir, "src/lib.rs").status.success());
    let out = stop(&dir, false);
    assert!(out.stdout.is_empty(), "lines inserted above are not a change: {}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn an_unhashed_claim_is_never_listed() {
    let dir = clone("unhashed-stop", "agent");
    seed_claims(&dir, json!([{
        "artifact_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "version": 1, "own": true,
        "path": "src/lib.rs", "kind": "memory",
    }]));
    write(&dir, "src/lib.rs", &LIB.replace("n += 1", "n += 2"));
    assert!(edited(&dir, "src/lib.rs").status.success());
    assert!(stop(&dir, false).stdout.is_empty());
}

#[test]
fn the_agent_syncs_at_stop_only_when_head_moved_and_no_git_hook_saw_it() {
    // agent alone: the Stop sync is the only one.
    assert!(sync_due(Mode::Agent, Some("a"), "b", None));
    assert!(!sync_due(Mode::Agent, Some("b"), "b", None), "head did not move");
    assert!(sync_due(Mode::Agent, None, "b", None), "first sight of a head");
    // both: git owns the sync unless its hooks did not fire for this head.
    assert!(!sync_due(Mode::Both, Some("a"), "b", Some("b")), "a git hook fired for it");
    assert!(sync_due(Mode::Both, Some("a"), "b", Some("a")), "no git hook fired since: a broken install degrades to the agent");
    // git alone and off never sync from the agent.
    assert!(!sync_due(Mode::Git, Some("a"), "b", None));
    assert!(!sync_due(Mode::Off, Some("a"), "b", None));
}

#[test]
fn no_agent_hook_reads_the_cli_credentials_file() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/capture");
    for file in std::fs::read_dir(&here).unwrap() {
        let path = file.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        if path.file_name().unwrap() == "agent.rs" {
            assert!(!text.contains("creds::"), "{} reads the CLI's stored credentials", path.display());
        }
    }
}
