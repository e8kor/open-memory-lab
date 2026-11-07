//! The agent-session hooks (ADR-0064 d11 to d15), verbs of the one
//! `oml hook` dispatcher. Each prints nothing in a clone whose mode excludes
//! `agent`, fails open, sends no request and no file content, and reads the
//! plugin's own environment alone: never the CLI's stored credentials.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::job::Queue;
use super::{Clone_, Mode, read_json, write_json};
use crate::events::Reply;
use crate::hook::{Quiet, cache_path, read_object};
use crate::sync::hash::{HASHER, md_hash, sha256};

/// The anchored writes one session may make (ADR-0047 d13, d15).
pub const MAX_ANCHORED_PER_SESSION: u64 = 5;
/// The stale claims one Stop lists; the rest are counted (d13).
pub const MAX_LISTED: usize = 10;
/// The largest span, in lines, a hook hashes.
const MAX_SPAN: usize = 2000;
/// The largest file, in bytes, a hook reads.
const MAX_FILE: u64 = 1 << 20;

/// Whether the agent's Stop should start a sync (d10, d14): the head moved
/// since the agent last saw it, and, with `both`, no git hook handled it.
#[must_use]
pub fn sync_due(mode: Mode, seen: Option<&str>, head: &str, git_hook_head: Option<&str>) -> bool {
    match mode {
        Mode::Agent => seen != Some(head),
        Mode::Both => seen != Some(head) && git_hook_head != Some(head),
        Mode::Git | Mode::Off => false,
    }
}

fn session_path(var: &dyn Fn(&str) -> Option<String>, session_id: &str) -> Option<PathBuf> {
    cache_path(var, session_id).map(|p| p.with_file_name(format!("{session_id}.capture.json")))
}

/// The per-session capture cache: touched and read paths, the listing of
/// the workarea's anchored claims, and what was already asked (d12, d13).
pub struct Session {
    path: PathBuf,
    pub doc: Value,
}

impl Session {
    #[must_use]
    pub fn load(var: &dyn Fn(&str) -> Option<String>, session_id: &str) -> Option<Self> {
        let path = session_path(var, session_id)?;
        let doc = read_json(&path);
        Some(Self { path, doc })
    }

    pub fn save(&self) {
        let _ = write_json(&self.path, &self.doc);
    }

    fn strings(&self, key: &str) -> Vec<String> {
        self.doc[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect()
    }

    fn push(&mut self, key: &str, value: &str) {
        let list = self.doc[key].take();
        let mut items: Vec<Value> = list.as_array().cloned().unwrap_or_default();
        if !items.iter().any(|v| v.as_str() == Some(value)) {
            items.push(json!(value));
        }
        self.doc[key] = Value::Array(items);
    }

    /// Records the listing `session-start` fetched (d12).
    pub fn set_claims(&mut self, claims: Vec<Value>) {
        self.doc["claims"] = Value::Array(claims);
    }
}

fn agent_clone(object: &Map<String, Value>) -> Option<Clone_> {
    let cwd = object.get("cwd").and_then(Value::as_str)?;
    let clone = Clone_::open(Path::new(cwd))?;
    clone.mode().includes_agent().then_some(clone)
}

fn session_id(object: &Map<String, Value>) -> Option<&str> {
    object.get("session_id").and_then(Value::as_str)
}

// ---------------------------------------------------------------------------
// PostToolUse
// ---------------------------------------------------------------------------

/// `oml hook post-tool-use`: records the repository-relative path of each
/// successful edit and read, with no request (d13). Prints nothing.
///
/// # Errors
///
/// None the harness sees; the verb is silent.
pub fn post_tool_use(
    var: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
) -> Result<String, Quiet> {
    let object = read_object(stdin).map_err(|_| None)?;
    let Some(clone) = agent_clone(&object) else {
        return Err(None);
    };
    let Some(id) = session_id(&object) else {
        return Err(None);
    };
    let tool = object
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let key = match tool {
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => "touched",
        "Read" => "read",
        _ => return Err(None),
    };
    let input = object.get("tool_input").unwrap_or(&Value::Null);
    let Some(file) = input
        .get("file_path")
        .or_else(|| input.get("notebook_path"))
        .and_then(Value::as_str)
    else {
        return Err(None);
    };
    // A path outside the clone, ignored by git or secret-shaped is not
    // recorded, and so can never be chosen or opened (d13, d17).
    let Some(rel) = clone.openable(Path::new(file)) else {
        return Err(None);
    };
    let Some(mut session) = Session::load(var, id) else {
        return Err(None);
    };
    session.push(key, &rel);
    session.push(
        if key == "touched" {
            "since_touched"
        } else {
            "since_read"
        },
        &rel,
    );
    session.save();
    Err(None)
}

// ---------------------------------------------------------------------------
// PreToolUse: the anchor fill
// ---------------------------------------------------------------------------

/// The definition keywords per extension (d15, FR-171).
fn keywords(ext: &str) -> &'static [&'static str] {
    match ext {
        "rs" => &[
            "fn", "struct", "enum", "trait", "impl", "mod", "type", "const",
        ],
        "py" => &["def", "class"],
        "ts" | "js" | "tsx" | "jsx" | "mjs" => &["function", "class", "interface", "type", "const"],
        "go" => &["func", "type"],
        _ => &[],
    }
}

const MODIFIERS: [&str; 10] = [
    "pub", "async", "unsafe", "export", "default", "extern", "static", "public", "private",
    "abstract",
];

fn ident_at(s: &str) -> &str {
    let end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    &s[..end]
}

/// The definition lines of a file: identifier and 1-based line.
fn definitions(ext: &str, text: &str) -> Vec<(String, usize)> {
    let kws = keywords(ext);
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let mut rest = line.trim_start();
        loop {
            let word = rest.split_whitespace().next().unwrap_or("");
            let bare = word.split('(').next().unwrap_or(word);
            if MODIFIERS.contains(&bare) {
                rest = rest[word.len()..].trim_start();
                // `extern "C"` and `pub(crate)` carry a literal or a group.
                if rest.starts_with('"') {
                    rest = rest
                        .split_once('"')
                        .and_then(|(_, r)| r.split_once('"'))
                        .map_or("", |(_, r)| r)
                        .trim_start();
                }
            } else {
                break;
            }
        }
        let Some((word, after)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        if kws.contains(&word) {
            let after = after.trim_start();
            let name = ident_at(after);
            if !name.is_empty() {
                out.push((name.to_owned(), i + 1));
            }
        }
    }
    out
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The line span of the indentation block of the definition at line `at`
/// (ADR-0057 d21): the line, the lines more indented than it, and a closing
/// bracket line at its own indent.
fn block_span(lines: &[&str], at: usize) -> (usize, usize) {
    let base = indent(lines[at - 1]);
    let mut end = at;
    let mut i = at;
    while i < lines.len() {
        let l = lines[i];
        if l.trim().is_empty() {
            i += 1;
            continue;
        }
        if indent(l) > base {
            end = i + 1;
            i += 1;
        } else {
            if indent(l) == base && l.trim_start().starts_with(['}', ')', ']']) {
                end = i + 1;
            }
            break;
        }
    }
    (at, end)
}

fn span_hash(text: &str, from: usize, to: usize) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    (from >= 1 && from <= to && to <= lines.len() && to - from < MAX_SPAN)
        .then(|| md_hash(&lines[from - 1..to].join("\n")))
}

fn line_span(fragment: &str) -> Option<(usize, usize)> {
    let rest = fragment.strip_prefix('L')?;
    let (n, m) = match rest.split_once("-L") {
        Some((n, m)) => (n.parse().ok()?, m.parse().ok()?),
        None => {
            let n = rest.parse().ok()?;
            (n, n)
        }
    };
    (n >= 1 && n <= m).then_some((n, m))
}

fn read_text(clone: &Clone_, rel: &str) -> Option<String> {
    let path = clone.top.join(rel);
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    String::from_utf8(std::fs::read(path).ok()?).ok()
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || "\"'`,;:()[]{}<>".contains(c))
        .map(|t| t.trim_end_matches(['.', '?', '!']).to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

/// d15.1 to d15.3: the one file the call anchors, or `None`.
fn choose(clone: &Clone_, session: &Session, text: &str) -> Option<String> {
    let known: BTreeSet<String> = session
        .strings("touched")
        .into_iter()
        .chain(session.strings("read"))
        .collect();
    let mut named: BTreeSet<String> = BTreeSet::new();
    for token in tokens(text) {
        if token.contains('/') || token.contains('.') {
            let p = clone.top.join(&token);
            if p.is_file()
                && let Some(rel) = clone.openable(&p)
            {
                named.insert(rel);
                continue;
            }
        }
        if token.contains('.') && !token.contains('/') {
            let matches: Vec<&String> = known
                .iter()
                .filter(|k| {
                    Path::new(k.as_str())
                        .file_name()
                        .is_some_and(|n| n == token.as_str())
                })
                .collect();
            if let [only] = matches.as_slice() {
                named.insert((*only).clone());
            }
        }
    }
    let pick = |set: BTreeSet<String>| -> Option<String> {
        let mut it = set.into_iter();
        match (it.next(), it.next()) {
            (Some(one), None) => Some(one),
            _ => None,
        }
    };
    if !named.is_empty() {
        return pick(named);
    }
    let touched: BTreeSet<String> = session.strings("since_touched").into_iter().collect();
    if !touched.is_empty() {
        return pick(touched);
    }
    pick(session.strings("since_read").into_iter().collect())
}

fn anchor_for(clone: &Clone_, repo: &str, rev: &str, rel: &str, text: &str, file: &str) -> Value {
    let mut anchor = json!({"repo": repo, "rev": rev, "path": rel});
    let ext = Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let defs = definitions(ext, file);
    let named: BTreeSet<String> = tokens(text)
        .into_iter()
        .filter(|t| defs.iter().any(|(n, _)| n == t))
        .collect();
    let _ = clone;
    if let Some(name) = named.iter().next().filter(|_| named.len() == 1) {
        let lines_of: Vec<usize> = defs
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, l)| *l)
            .collect();
        if let [at] = lines_of.as_slice() {
            let lines: Vec<&str> = file.split('\n').collect();
            let (n, m) = block_span(&lines, *at);
            if let Some(hash) = span_hash(file, n, m) {
                anchor["fragment"] = json!(format!("L{n}-L{m}"));
                anchor["hash"] = json!(hash);
                anchor["hasher"] = json!(HASHER);
            }
        }
    }
    anchor
}

fn normal(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Three consecutive normalised lines of the file inside the text, or a
/// fenced code block (d15, d17).
fn copies_code(text: &str, file: Option<&str>) -> bool {
    if text.contains("```") {
        return true;
    }
    let Some(file) = file else { return false };
    let theirs: Vec<String> = file.lines().map(normal).filter(|l| !l.is_empty()).collect();
    let ours: Vec<String> = text.lines().map(normal).filter(|l| !l.is_empty()).collect();
    ours.windows(3).any(|w| theirs.windows(3).any(|t| t == w))
}

fn deny(reason: &str) -> String {
    json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }})
    .to_string()
}

/// The hook document for an `oml_write` call: `updatedInput` completed with
/// an anchor, an origin and a workarea; a deny, once, for a text that copies
/// code, repeats an existing anchor, or is the session's sixth anchored
/// write; or `None` when the call goes through unchanged (d15).
#[must_use]
pub fn fill_anchor(
    var: &dyn Fn(&str) -> Option<String>,
    object: &Map<String, Value>,
) -> Option<String> {
    let tool = object.get("tool_name").and_then(Value::as_str)?;
    if !tool.ends_with("oml_write") {
        return None;
    }
    let clone = agent_clone(object)?;
    let id = session_id(object)?;
    let input = object.get("tool_input")?.as_object()?;
    let text = input.get("text").and_then(Value::as_str).unwrap_or("");
    let repo = clone.repo()?;
    let rev = clone.head()?;
    let mut session = Session::load(var, id)?;

    let mut updated = input.clone();
    let anchor: Option<Value> = match input.get("anchor") {
        Some(Value::Object(sent)) => {
            // Completed, never replaced (d15): a path only gets the rest.
            let path = sent.get("path").and_then(Value::as_str);
            path.and_then(|p| clone.openable(Path::new(p))).map(|rel| {
                let mut a = Value::Object(sent.clone());
                a["path"] = json!(rel);
                if let Some(m) = a.as_object_mut() {
                    m.entry("repo").or_insert(json!(repo));
                    m.entry("rev").or_insert(json!(rev));
                }
                if a["hash"].is_null()
                    && let Some((n, m)) = a["fragment"].as_str().and_then(line_span)
                    && let Some(file) = read_text(&clone, &rel)
                    && let Some(hash) = span_hash(&file, n, m)
                {
                    a["hash"] = json!(hash);
                    a["hasher"] = json!(HASHER);
                }
                a
            })
        }
        Some(_) => None,
        None => choose(&clone, &session, text).and_then(|rel| {
            let file = read_text(&clone, &rel)?;
            Some(anchor_for(&clone, &repo, &rev, &rel, text, &file))
        }),
    };
    let anchored = anchor.is_some();
    if let Some(a) = &anchor {
        updated.insert("anchor".into(), a.clone());
        if !updated.contains_key("origin") {
            let kind = if a["path"].as_str().is_some_and(|p| p.ends_with(".md")) {
                "doc"
            } else {
                "code"
            };
            updated.insert("origin".into(), json!({"kind": kind}));
        }
        if !updated.contains_key("workarea")
            && let Some(w) = clone.get("workarea")
        {
            updated.insert("workarea".into(), json!(w));
        }
    }

    // Refusals, each once per call (d15); the sixth anchored write always.
    if anchored {
        let writes = session.doc["anchored_writes"].as_u64().unwrap_or(0);
        if writes >= MAX_ANCHORED_PER_SESSION {
            return Some(deny(&format!(
                "this session has made {MAX_ANCHORED_PER_SESSION} anchored writes, the most one session makes (capture.max_per_session); write the rest in the next session, or send the write without an anchor"
            )));
        }
        let call = sha256(Value::Object(input.clone()).to_string().as_bytes());
        let refused = session.strings("refused");
        let a = anchor.as_ref().unwrap_or(&Value::Null);
        let path = a["path"].as_str().unwrap_or("");
        let file = read_text(&clone, path);
        let duplicate = input.get("replaces").is_none()
            && session.doc["claims"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|c| {
                    c["path"].as_str() == Some(path)
                        && c["fragment"].as_str() == a["fragment"].as_str()
                        && c["artifact_id"].is_string()
                });
        let reason = if copies_code(text, file.as_deref()) {
            Some("the text holds three or more consecutive lines of the anchored file, or a fenced code block: a memory states the fact and cites the code, it does not carry it. Resend it as a sentence (ADR-0064 d17)".to_owned())
        } else if duplicate {
            Some(format!(
                "a claim in this workarea is already anchored at {path}{}: resend with `replaces` and `expected_version` to supersede it, or send the same call again to write a second claim",
                a["fragment"]
                    .as_str()
                    .map(|f| format!("#{f}"))
                    .unwrap_or_default()
            ))
        } else {
            None
        };
        if let Some(reason) = reason
            && !refused.contains(&call)
        {
            session.push("refused", &call);
            session.save();
            return Some(deny(&reason));
        }
        session.doc["anchored_writes"] = json!(writes + 1);
    }
    session.doc["since_touched"] = json!([]);
    session.doc["since_read"] = json!([]);
    session.save();
    if !anchored {
        return None;
    }
    Some(json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "updatedInput": Value::Object(updated)}}).to_string())
}

// ---------------------------------------------------------------------------
// Stop
// ---------------------------------------------------------------------------

/// Whether a span with this hash still exists anywhere in the file: lines
/// inserted above it move it, and a move is not a change (ADR-0057 d18).
fn span_present(file: &str, hash: &str, len: usize) -> bool {
    let lines: Vec<&str> = file.split('\n').collect();
    if len == 0 || len > lines.len() {
        return false;
    }
    (0..=lines.len() - len).any(|s| md_hash(&lines[s..s + len].join("\n")) == hash)
}

/// The Stop check (d13) and the Stop-time sync (d14).
///
/// # Errors
///
/// `Err(None)` when nothing is due.
pub fn on_stop(
    var: &dyn Fn(&str) -> Option<String>,
    object: &Map<String, Value>,
) -> Result<Reply, Quiet> {
    if object.get("stop_hook_active") == Some(&Value::Bool(true)) {
        return Err(None);
    }
    let clone = agent_clone(object).ok_or(None)?;
    let id = session_id(object).ok_or(None)?;
    let mut session = Session::load(var, id).ok_or(None)?;
    let touched = session.strings("touched");
    let already = session.strings("listed");
    let mut stale: Vec<Value> = Vec::new();
    let mut others = 0_usize;
    for claim in session.doc["claims"].as_array().into_iter().flatten() {
        let (Some(path), Some(fragment), Some(hash)) = (
            claim["path"].as_str(),
            claim["fragment"].as_str(),
            claim["hash"].as_str(),
        ) else {
            continue;
        };
        let Some((n, m)) = line_span(fragment) else {
            continue;
        };
        if !touched.iter().any(|t| t == path) {
            continue;
        }
        let Some(file) = clone
            .openable(Path::new(path))
            .and_then(|rel| read_text(&clone, &rel))
        else {
            continue;
        };
        if span_hash(&file, n, m).as_deref() == Some(hash) || span_present(&file, hash, m - n + 1) {
            continue;
        }
        if claim["own"].as_bool() == Some(true) {
            let current = span_hash(&file, n, m).unwrap_or_default();
            let marker = format!("{}:{current}", claim["artifact_id"].as_str().unwrap_or(""));
            if !already.contains(&marker) {
                stale.push(json!({"claim": claim, "marker": marker}));
            }
        } else {
            others += 1;
        }
    }
    let reason = (!stale.is_empty()).then(|| {
        let mut lines = vec![format!(
            "OML: {} of your anchored claim(s) no longer match the code you changed. Answer each with oml_write using `replaces` and `expected_version`; every answer is a held version a person reviews, and nothing is re-anchored silently:",
            stale.len()
        )];
        for item in stale.iter().take(MAX_LISTED) {
            let c = &item["claim"];
            let (aid, v) = (c["artifact_id"].as_str().unwrap_or(""), c["version"].as_u64().unwrap_or(0));
            lines.push(format!(
                "- {aid} v{v}  {}#{}\n    confirm: oml_write with replaces={aid}, expected_version={v} and the same text (the hook fills the new anchor)\n    update: the same with corrected text\n    retire: oml_write with replaces={aid}, expected_version={v}, retirement=true and why in the text",
                c["path"].as_str().unwrap_or(""),
                c["fragment"].as_str().unwrap_or("")
            ));
        }
        if stale.len() > MAX_LISTED {
            lines.push(format!("{} more not listed.", stale.len() - MAX_LISTED));
        }
        if others > 0 {
            lines.push(format!("{others} other claim(s) of other keys under these paths are marked by the next sync, not listed here."));
        }
        lines.join("\n")
    });
    let mut listed = already;
    listed.extend(
        stale
            .iter()
            .filter_map(|s| s["marker"].as_str().map(ToOwned::to_owned)),
    );
    session.doc["listed"] = json!(listed);
    // d14: the sync, never waited for.
    if let Some(head) = clone.head() {
        let state = read_json(&clone.oml_dir().join("state.json"));
        if sync_due(
            clone.mode(),
            session.doc["seen_head"].as_str(),
            &head,
            state["hook_head"].as_str(),
        ) && clone.on_sync_branch()
        {
            queue_whole(&clone, &head);
        }
        session.doc["seen_head"] = json!(head);
    }
    session.save();
    reason.map(Reply::Block).ok_or(None)
}

/// Queues every source whole at `head` and starts the job (d14).
fn queue_whole(clone: &Clone_, head: &str) {
    let sources = clone.sources();
    let _ = Queue::update(clone, |q| {
        for s in &sources {
            q.enqueue(super::job::QueueEntry {
                source: s.path.clone(),
                adapter: s.adapter.clone(),
                commit: head.to_owned(),
                paths: Vec::new(),
                reason: "stop".to_owned(),
                rewritten: false,
                exclude: super::excludes_for(s, &sources),
            });
        }
    });
    if !Queue::load(clone).is_empty() {
        super::hooks::spawn_job(clone);
    }
}

/// Deletes the session's capture cache with the failure-recall cache
/// (SessionEnd, ADR-0038 d9).
pub fn forget(var: &dyn Fn(&str) -> Option<String>, session_id: &str) {
    if let Some(path) = session_path(var, session_id) {
        let _ = std::fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// SessionStart
// ---------------------------------------------------------------------------

/// The listing cap of d12; a longer listing is cut and the cut is named.
pub const LISTING_CAP: usize = 2000;
/// Session start's recall: `k` and the token budget (d12, FR-173).
const RECALL_K: u64 = 6;
const RECALL_TOKENS: u64 = 600;

fn claim_of(item: &crate::sync::api::Listed, own: bool) -> Option<Value> {
    let a = item.anchor.as_ref()?;
    let mut claim = json!({
        "artifact_id": item.artifact_id, "version": item.version, "own": own,
        "path": a.path, "kind": "memory",
    });
    if let (Some(f), Some(h)) = (&a.fragment, &a.hash) {
        claim["fragment"] = json!(f);
        claim["hash"] = json!(h);
        claim["hasher"] = json!(HASHER);
    }
    Some(claim)
}

/// d12: in a clone whose mode includes `agent`, the repository workarea's
/// anchored claims go into the session cache for d13 and d15, and a recall
/// scoped to it, `k` 6 and 600 tokens, is returned for the context. The
/// requests run in parallel under the hook's 3 s timeout each; any failure
/// costs this section alone.
#[must_use]
pub fn session_start_section(
    var: &dyn Fn(&str) -> Option<String>,
    server: &str,
    key: &str,
    cwd: Option<&str>,
    session_id: Option<&str>,
    redact: &dyn Fn(&str) -> String,
) -> Option<String> {
    use crate::client::{CallOpts, Client};
    use crate::hook::REQUEST_TIMEOUT;
    use crate::sync::api::{HttpApi, ListQuery, SyncApi as _};
    let clone = Clone_::open(Path::new(cwd?))?;
    if !clone.mode().includes_agent() {
        return None;
    }
    let workarea = clone.get("workarea")?.to_owned();
    // The public server needs the person's recorded confirmation (d17).
    if crate::sync::source::is_public_server(server)
        && clone.get("allowpublicserver") != Some("true")
    {
        return None;
    }
    let repo = clone.repo()?;
    let make = || {
        Client::for_hook(
            server.to_owned(),
            Some(key.to_owned()),
            REQUEST_TIMEOUT,
            "session-start",
        )
    };
    let query = |own: bool| ListQuery {
        workarea: workarea.clone(),
        own,
        anchor_repo: Some(repo.clone()),
        anchor_path_prefix: None,
    };
    let words = crate::hook::query_words(cwd);
    let (own, served, recall) = std::thread::scope(|s| {
        let own = s.spawn(|| HttpApi { client: make() }.list(&query(true)));
        let served = s.spawn(|| HttpApi { client: make() }.list(&query(false)));
        let recall =
            s.spawn(|| {
                make().post(
                "/v1/memories/recall",
                &json!({
                    "query": words, "k": RECALL_K, "include_text": true, "include_unreviewed": true,
                    "max_tokens": RECALL_TOKENS, "workarea": workarea, "anchor_repo": repo,
                }),
                &CallOpts { auth: true, idempotency: false },
            )
            });
        (own.join().ok(), served.join().ok(), recall.join().ok())
    });
    if let (Some(Ok(own)), Some(served), Some(id)) = (&own, &served, session_id)
        && let Some(mut session) = Session::load(var, id)
    {
        let mut claims: Vec<Value> = own
            .iter()
            .filter(|i| i.tip)
            .filter_map(|i| claim_of(i, true))
            .collect();
        if let Ok(served) = served {
            let mine: BTreeSet<&str> = own.iter().map(|i| i.artifact_id.as_str()).collect();
            claims.extend(
                served
                    .iter()
                    .filter(|i| i.tip && !mine.contains(i.artifact_id.as_str()))
                    .filter_map(|i| claim_of(i, false)),
            );
        }
        let cut = claims.len() > LISTING_CAP;
        claims.truncate(LISTING_CAP);
        session.set_claims(claims);
        session.doc["listing_cut"] = json!(cut);
        session.save();
    }
    let (_, recall) = recall?.ok()?;
    let hits = recall["hits"].as_array()?;
    if hits.is_empty() {
        return None;
    }
    let mut lines = vec![format!(
        "Repository knowledge, workarea {workarea} (in progress: an item marked unreviewed has not been judged by a person):"
    )];
    for hit in hits {
        let m = &hit["memory"];
        let text: String = redact(m["text"].as_str().unwrap_or(""))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(240)
            .collect();
        let at = match (
            m["anchor"]["path"].as_str(),
            m["anchor"]["fragment"].as_str(),
        ) {
            (Some(p), Some(f)) => format!(" [{p}#{f}]"),
            (Some(p), None) => format!(" [{p}]"),
            _ => String::new(),
        };
        let unreviewed = if m["state"].as_str() == Some("held") {
            " (unreviewed)"
        } else {
            ""
        };
        lines.push(format!(
            "- {}{at}{unreviewed}: {text}",
            redact(m["claim_key"].as_str().unwrap_or("?"))
        ));
    }
    Some(lines.join("\n"))
}
