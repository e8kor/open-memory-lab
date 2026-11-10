//! The agent plugin's guardrail hooks (ADR-0037 d6, d7, d9): `oml
//! hook guardrails`, which loads the set at SessionStart, after a compaction
//! and at SubagentStart, prints it in its own frame and, at SessionStart
//! alone, writes the session's rule cache; and `oml hook pre-tool-use`,
//! which checks one tool call against that cache with no network call. The
//! rule's meaning is `oml-guardrail`'s, the leaf the server reads it with
//! too (d18 Q10).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use oml_guardrail::{
    CACHE_MAX_AGE_DAYS, CacheState, CachedRule, FrameOptions, Rule, ToolCall, Verdict,
    cache_file_name, decide, decode_cache, encode_cache, over_cap_sentence, render_frame,
};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::client::Client;
use crate::hook::{REQUEST_TIMEOUT, describe, origin, resolve_pair_for};

const REDACTED: &str = "[OML_API_KEY]";

/// The rule cache's directory: `oml/guardrails` under `XDG_STATE_HOME`, or
/// `HOME/.local/state` (d9). `None` when neither names an absolute path.
#[must_use]
pub fn cache_dir(var: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let absolute = |v: String| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    let base = var("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .and_then(absolute)
        .or_else(|| {
            var("HOME")
                .and_then(absolute)
                .map(|h| h.join(".local").join("state"))
        })?;
    Some(base.join("oml").join("guardrails"))
}

fn read_object(stdin: &mut dyn Read) -> Result<Map<String, Value>, String> {
    let mut text = String::new();
    stdin
        .read_to_string(&mut text)
        .map_err(|e| format!("stdin is unreadable: {e}"))?;
    match serde_json::from_str(&text) {
        Ok(Value::Object(object)) => Ok(object),
        Ok(_) => Err("stdin is not a JSON object".to_owned()),
        Err(e) => Err(format!("stdin is not JSON: {e}")),
    }
}

/// `oml hook pre-tool-use`: the check of one call against the session's
/// cache (d9's table). The document to print, empty when nothing is
/// decided; never `allow`. It reads no variable but the cache's directory
/// and sends no request.
///
/// # Errors
///
/// Stdin is not the PreToolUse object, or its `session_id` names no cache
/// file: the verb fails, which blocks the call (`onFailure: "block"`, and
/// exit 2 for an older Claude Code).
pub fn pre_tool_use(
    var: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
) -> Result<String, String> {
    let object = read_object(stdin)?;
    let session_id = object
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or("stdin carries no session_id")?;
    let file = cache_file_name(session_id).map_err(|e| e.to_string())?;
    let tool_name = object
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or("stdin carries no tool_name")?;
    let null = Value::Null;
    let call = ToolCall {
        tool_name,
        tool_input: object.get("tool_input").unwrap_or(&null),
        cwd: object.get("cwd").and_then(Value::as_str),
    };
    let dir = cache_dir(var);
    let state = match &dir {
        None => CacheState::NoCache,
        Some(dir) => match std::fs::read(dir.join(&file)) {
            Ok(bytes) => decode_cache(Some(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => CacheState::NoCache,
            // A cache that exists and cannot be read is a cache that cannot
            // be checked: it denies, as one that does not parse does.
            Err(_) => decode_cache(Some(b"")),
        },
    };
    let dir_text = dir
        .as_deref()
        .and_then(Path::to_str)
        .unwrap_or_default()
        .to_owned();
    Ok(match decide(&state, &call, &dir_text) {
        Verdict::Nothing => String::new(),
        Verdict::Warn(message) => json!({ "systemMessage": message }).to_string(),
        Verdict::Decide(decision) => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": decision.effect.as_str(),
                "permissionDecisionReason": decision.reason(),
            }
        })
        .to_string(),
    })
}

/// `oml hook guardrails`: one `GET /v1/guardrails` under the agent's own
/// pair, the frame as `additionalContext` for the event stdin names, and at
/// SessionStart the rule cache, after a sweep of caches older than 30 days.
/// Every failure is a `systemMessage` the user sees, exit 0: memory may fail
/// silently, rules may not (d6).
///
/// # Errors
///
/// None in practice: every failure is a document. The signature is the
/// hook runner's.
pub fn guardrails(
    var: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
) -> Result<String, Option<String>> {
    let failed = |why: String| {
        json!({
            "systemMessage": format!(
                "OML guardrails were not loaded for this session ({why}), so their rules are not shown or checked; restart the session to retry"
            )
        })
        .to_string()
    };
    let pair = match resolve_pair_for(var, "no guardrails were loaded") {
        Ok(pair) => pair,
        Err(why) => return Ok(failed(why)),
    };
    let redact = |text: &str| text.replace(pair.key.as_str(), REDACTED);
    let object = match read_object(stdin) {
        Ok(object) => object,
        Err(why) => return Ok(redact(&failed(why))),
    };
    let event = object
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("SessionStart")
        .to_owned();
    let session_id = object
        .get("session_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let client = Client::for_hook(
        pair.url.clone(),
        Some(pair.key.clone()),
        REQUEST_TIMEOUT,
        "guardrails",
    );
    let set = match client.get("/v1/guardrails") {
        Ok((_, set)) => set,
        Err(e) => return Ok(redact(&failed(describe(e)))),
    };

    let mut messages = Vec::new();
    let mut checked = false;
    if event == "SessionStart" {
        match write_cache(var, session_id.as_deref(), &set) {
            Ok(()) => checked = true,
            Err(why) => messages.push(format!(
                "OML guardrails are shown but not checked this session: the rule cache was not written ({why}); restart the session to retry"
            )),
        }
    }
    let frame = match render_frame(
        &set,
        &FrameOptions {
            origin: &origin(&pair.url),
            checked,
            whole_over_cap: false,
        },
    ) {
        Ok(frame) => frame,
        Err(why) => return Ok(redact(&failed(why.to_owned()))),
    };
    if let Some(count) = frame.over_cap {
        messages.insert(0, format!("OML: {}", over_cap_sentence(count)));
    }
    let mut document = json!({
        "hookSpecificOutput": {
            "hookEventName": event,
            "additionalContext": frame.text,
        }
    });
    if !messages.is_empty() {
        document["systemMessage"] = json!(messages.join(" "));
    }
    Ok(redact(&document.to_string()))
}

/// Sweeps the cache directory and writes this session's cache, 0700 and
/// 0600, by write and rename (d9).
fn write_cache(
    var: &dyn Fn(&str) -> Option<String>,
    session_id: Option<&str>,
    set: &Value,
) -> Result<(), String> {
    let session_id = session_id.ok_or("stdin carries no session_id")?;
    let file = cache_file_name(session_id).map_err(|e| e.to_string())?;
    let dir = cache_dir(var).ok_or("neither XDG_STATE_HOME nor HOME names a directory")?;
    let rules = cached_rules(set)?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    sweep(&dir);
    let path = dir.join(file);
    let tmp = dir.join(format!(".{session_id}.tmp-{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut out = options.open(&tmp).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut out, encode_cache(&rules).as_bytes())
        .map_err(|e| e.to_string())?;
    drop(out);
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// The served rules, each with its guardrail's id and text. A served rule
/// that does not parse leaves the cache unwritten rather than a rule out.
fn cached_rules(set: &Value) -> Result<Vec<CachedRule>, String> {
    let guardrails = set["guardrails"]
        .as_array()
        .ok_or("the answer has no guardrails list")?;
    let mut rules = Vec::new();
    for g in guardrails {
        if g["rule"].is_null() {
            continue;
        }
        let artifact_id = g["artifact_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or("a guardrail's artifact_id is not an id")?;
        let rule = Rule::parse(&g["rule"]).map_err(|e| format!("a served rule is refused: {e}"))?;
        rules.push(CachedRule {
            artifact_id,
            body: g["text"].as_str().unwrap_or_default().to_owned(),
            rule,
        });
    }
    Ok(rules)
}

/// Removes cache files older than [`CACHE_MAX_AGE_DAYS`]; a file it cannot
/// read or remove is left.
fn sweep(dir: &Path) {
    let max_age = Duration::from_secs(u64::from(CACHE_MAX_AGE_DAYS) * 24 * 3600);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .is_some_and(|age| age > max_age);
        if old && path.extension().is_some_and(|e| e == "json") {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// `oml guardrails show`: the hook's frame with every line, even over the
/// cap, and whether the set was over it (d7).
///
/// # Errors
///
/// The answer is not `GET /v1/guardrails`'s shape.
pub fn show_text(set: &Value, origin: &str) -> Result<(String, bool), String> {
    let frame = render_frame(
        set,
        &FrameOptions {
            origin,
            checked: false,
            whole_over_cap: true,
        },
    )
    .map_err(ToOwned::to_owned)?;
    Ok((frame.text, frame.over_cap.is_some()))
}

/// The marker that bounds the exported text in an `agents-md` file (d11).
pub const MARKER: &str = "<!-- oml:guardrails -->";

/// `oml guardrails export`: writes the promoted set as text, or with
/// `check` writes nothing and answers whether the file's `set_digest` is
/// the live one. `Ok(true)` is written or current, `Ok(false)` changed.
///
/// # Errors
///
/// The answer is not its shape, an `agents-md` file does not hold exactly
/// one marker pair, or the file cannot be read or written; nothing is
/// written then.
pub fn export(
    set: &Value,
    origin: &str,
    format: &str,
    out: &Path,
    check: bool,
) -> Result<bool, String> {
    let digest = set["set_digest"]
        .as_str()
        .ok_or("the answer has no set_digest")?;
    if check {
        let text = std::fs::read_to_string(out)
            .map_err(|e| format!("{} cannot be read: {e}", out.display()))?;
        let written = text
            .split("set_digest ")
            .nth(1)
            .and_then(|rest| rest.split([' ', ',']).next())
            .map(|d| d.trim_end_matches(':'));
        return Ok(written == Some(digest));
    }
    let frame = render_frame(
        set,
        &FrameOptions {
            origin,
            checked: false,
            whole_over_cap: true,
        },
    )
    .map_err(ToOwned::to_owned)?;
    let lines: Vec<&str> = frame
        .text
        .lines()
        .filter(|l| l.starts_with("- ["))
        .collect();
    let header = format!(
        "<!-- OML guardrails exported from {origin}, workarea {} ({}), guardrail scope {}, {} guardrails, set_digest {digest}: text, not enforcement; do not edit, re-run oml guardrails export -->",
        set["scope_path"].as_str().unwrap_or("?"),
        set["scope_node"].as_str().unwrap_or("?"),
        set["guardrail_scope"].as_str().unwrap_or("?"),
        frame.count,
    );
    let body = if lines.is_empty() {
        "No guardrail applies here.".to_owned()
    } else {
        lines.join("\n")
    };
    let block = format!("{header}\n\n{body}\n");
    let contents = match format {
        "claude-md" => block,
        "agents-md" => {
            let text = std::fs::read_to_string(out)
                .map_err(|e| format!("{} cannot be read: {e}", out.display()))?;
            let parts: Vec<&str> = text.split(MARKER).collect();
            if parts.len() != 3 {
                return Err(format!(
                    "refused: {} has {} {MARKER} markers; exactly one pair is required; nothing was written",
                    out.display(),
                    parts.len() - 1
                ));
            }
            format!("{}{MARKER}\n{block}{MARKER}{}", parts[0], parts[2])
        }
        other => return Err(format!("unknown format {other:?}")),
    };
    std::fs::write(out, contents)
        .map_err(|e| format!("{} cannot be written: {e}", out.display()))?;
    Ok(true)
}
