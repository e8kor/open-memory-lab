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
    CACHE_MAX_AGE_DAYS, CacheState, CachedRule, Effect, FrameOptions, Rule, ToolCall, Verdict,
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
/// file: the verb fails with exit 1, a non-blocking error, so the call goes
/// through unchecked and the cause reaches the user (ADR-0037 d9, amended
/// 2026-11-26, #519).
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
    let local_skill = local_skill_write(
        &call,
        skills_home(var).as_deref(),
        var(PROJECT_DIR).as_deref(),
    );
    let ask = |reason: String| {
        json!({
            "hookEventName": "PreToolUse",
            "permissionDecision": "ask",
            "permissionDecisionReason": reason,
        })
    };
    Ok(match decide(&state, &call, &dir_text) {
        // The built-in skill rule needs no cache, so it asks with or
        // without one; a guardrail's deny on the same call wins, as deny
        // wins over ask (ADR-0080 d15; ADR-0037 d9).
        Verdict::Nothing if local_skill => {
            json!({ "hookSpecificOutput": ask(LOCAL_SKILL_REASON.to_owned()) }).to_string()
        }
        Verdict::Nothing => String::new(),
        Verdict::Warn(message) if local_skill => json!({
            "systemMessage": message,
            "hookSpecificOutput": ask(LOCAL_SKILL_REASON.to_owned()),
        })
        .to_string(),
        Verdict::Warn(message) => json!({ "systemMessage": message }).to_string(),
        Verdict::Decide(decision) if local_skill && decision.effect != Effect::Deny => json!({
            "hookSpecificOutput": ask(format!("{} {LOCAL_SKILL_REASON}", decision.reason())),
        })
        .to_string(),
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

/// The bypass: `1` makes the plugin's PreToolUse command exit 0 before it
/// runs `oml` at all, so no call is checked (ADR-0037 d9, amended
/// 2026-11-26, #519). The verb itself does not read it, so `oml doctor`'s
/// probe of the verb still answers.
pub const BYPASS: &str = "OML_HOOK_DISABLE";

/// `oml doctor`'s hook lines: the bypass set, or set to a value the
/// plugin's command does not take. Nothing when it is unset, empty or `0`.
#[must_use]
pub fn doctor_lines(var: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    match var(BYPASS).filter(|v| !v.is_empty()).as_deref() {
        None | Some("0") => Vec::new(),
        Some("1") => vec![format!(
            "warn guardrail_check_off: {BYPASS}=1, so no tool call is checked against the session's guardrails; unset it and restart the session to check them"
        )],
        Some(other) => vec![format!(
            "warn guardrail_check_off: {BYPASS} is {other:?}; it takes 1 or 0, so the check runs"
        )],
    }
}

/// The variable Claude Code sets to the session's project directory.
const PROJECT_DIR: &str = "CLAUDE_PROJECT_DIR";

/// The home whose `.claude/skills/` the local-skill rule matches: `HOME`,
/// or `USERPROFILE` when `HOME` is unset or empty, as on Windows (R20).
pub fn skills_home(var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let set = |name: &str| var(name).filter(|v| !v.is_empty());
    set("HOME").or_else(|| set("USERPROFILE"))
}

/// Why a local skill file write asks (ADR-0080 d15).
pub const LOCAL_SKILL_REASON: &str = "OML keeps skills: store this one with oml_write and kind skill (replaces to update it); allow this call to write the local file anyway.";

/// Whether `call` writes a file under a project's `.claude/skills/` or
/// `$HOME/.claude/skills/` (ADR-0080 d15): a `Write`, `Edit`, `MultiEdit`
/// or `NotebookEdit` whose path, after `~` expansion and resolution against
/// the session's working directory, lies there, in any letter case. The
/// project is the session's working directory and, when Claude Code names
/// one, its project directory. The path is normalised lexically, so `..`
/// cannot step out of the match and no file is read (R12, R17).
#[must_use]
pub fn local_skill_write(call: &ToolCall<'_>, home: Option<&str>, project: Option<&str>) -> bool {
    let field = match call.tool_name {
        "Write" | "Edit" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        _ => return false,
    };
    let Some(raw) = call.tool_input.get(field).and_then(Value::as_str) else {
        return false;
    };
    let home = home.filter(|h| Path::new(h).is_absolute());
    let expanded = match (raw.strip_prefix('~'), home) {
        (Some(""), Some(h)) => PathBuf::from(h),
        (Some(rest), Some(h)) if rest.starts_with(['/', '\\']) => {
            PathBuf::from(h).join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(raw),
    };
    let resolved = match call.cwd.filter(|_| expanded.is_relative()) {
        Some(cwd) => Path::new(cwd).join(&expanded),
        None => expanded,
    };
    if !resolved.is_absolute() {
        return false;
    }
    // Compared without regard to case, as macOS's and Windows' default
    // file systems name one directory in any case; a long s and a Kelvin
    // sign fold to `s` and `k` there too.
    let folded = |p: &Path| {
        PathBuf::from(
            p.to_string_lossy()
                .to_lowercase()
                .replace('\u{17F}', "s")
                .replace('\u{212A}', "k"),
        )
    };
    let path = folded(&normalise(&resolved));
    [call.cwd, project, home]
        .into_iter()
        .flatten()
        .filter(|base| Path::new(base).is_absolute())
        .any(|base| {
            path.starts_with(folded(
                &normalise(Path::new(base)).join(".claude").join("skills"),
            ))
        })
}

/// `path` with `.` and `..` folded away, lexically.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
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
    // The session is the key's own, so its own held guardrails load too,
    // marked and asking (ADR-0080 d14).
    let set = match client.get("/v1/guardrails?include_own_held=true") {
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

/// The served rules, each with its guardrail's id and text. A held rule the
/// server serves with `effect_inside: ask` is cached with that effect and
/// its text marked `[unreviewed]`, so it asks and never denies; the server
/// decides the downgrade, and a promoted rule keeps its own (ADR-0080
/// d10). A served rule that does not parse leaves the cache unwritten
/// rather than a rule out.
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
        let mut served = g["rule"].clone();
        let mut body = g["text"].as_str().unwrap_or_default().to_owned();
        if g["unreviewed"] == true {
            // A held rule never denies: the server's `effect_inside` where
            // it names one, `ask` where it does not.
            served["effect"] = json!(g["effect_inside"].as_str().unwrap_or("ask"));
            body = format!("[unreviewed] {body}");
        }
        let rule = Rule::parse(&served).map_err(|e| format!("a served rule is refused: {e}"))?;
        rules.push(CachedRule {
            artifact_id,
            body,
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
        "<!-- OML guardrails exported from {origin}, workspace {} ({}), guardrail scope {}, {} guardrails, set_digest {digest}: text, not enforcement; do not edit, re-run oml guardrails export -->",
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

// The cases are written with POSIX absolute paths, which Windows does not
// read as absolute.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn call<'a>(tool: &'a str, input: &'a Value, cwd: Option<&'a str>) -> ToolCall<'a> {
        ToolCall {
            tool_name: tool,
            tool_input: input,
            cwd,
        }
    }

    const HOME: &str = "/home/ada";
    const CWD: &str = "/work/repo";

    fn asks(tool: &str, field: &str, path: &str, project: Option<&str>) -> bool {
        let input = json!({ field: path });
        local_skill_write(&call(tool, &input, Some(CWD)), Some(HOME), project)
    }

    /// `MultiEdit` and `NotebookEdit` writes under `.claude/skills/` ask as
    /// `Write` and `Edit` do, each by the field it carries (ADR-0080 d15).
    #[test]
    fn multiedit_and_notebookedit_under_claude_skills_ask() {
        assert!(asks(
            "MultiEdit",
            "file_path",
            ".claude/skills/x/SKILL.md",
            None
        ));
        assert!(asks(
            "NotebookEdit",
            "notebook_path",
            "~/.claude/skills/x/n.ipynb",
            None
        ));
        assert!(!asks(
            "NotebookEdit",
            "file_path",
            "~/.claude/skills/x/n.ipynb",
            None
        ));
        assert!(!asks(
            "Read",
            "file_path",
            ".claude/skills/x/SKILL.md",
            None
        ));
    }

    /// The project Claude Code names in `CLAUDE_PROJECT_DIR` is matched
    /// beside the working directory (ADR-0080 d15; R12).
    #[test]
    fn a_write_under_the_claude_project_dir_skills_asks() {
        let path = "/srv/project/.claude/skills/x/SKILL.md";
        assert!(!asks("Write", "file_path", path, None));
        assert!(asks("Write", "file_path", path, Some("/srv/project")));
    }

    /// A `..` that leaves `.claude/skills` is not matched, and one that
    /// enters it is (R12).
    #[test]
    fn a_dot_dot_that_leaves_the_skills_directory_does_not_ask() {
        assert!(!asks(
            "Write",
            "file_path",
            ".claude/skills/../notes.md",
            None
        ));
        assert!(!asks(
            "Write",
            "file_path",
            "/work/repo/.claude/skills/x/../../a.md",
            None
        ));
        assert!(asks(
            "Write",
            "file_path",
            "src/../.claude/skills/x/SKILL.md",
            None
        ));
    }

    /// The match ignores case, as macOS's and Windows' default file systems
    /// do (ADR-0080 d15).
    #[test]
    fn a_case_variant_of_claude_skills_asks() {
        for path in [
            ".Claude/Skills/x/SKILL.md",
            "/WORK/Repo/.claude/SKILLS/x/SKILL.md",
            "~/.CLAUDE/skills/x/SKILL.md",
        ] {
            assert!(asks("Write", "file_path", path, None), "{path}");
        }
        assert!(!asks(
            "Write",
            "file_path",
            ".claude/skillset/SKILL.md",
            None
        ));
    }

    /// A long s (U+017F) and a Kelvin sign (U+212A), which a case-folding
    /// file system reads as `s` and `k`, do not slip past the ask.
    #[test]
    fn a_long_s_or_a_kelvin_sign_in_claude_skills_asks() {
        for path in [
            ".claude/\u{17F}kills/x/SKILL.md",
            ".claude/s\u{212A}ills/x/SKILL.md",
        ] {
            assert!(asks("Write", "file_path", path, None), "{path}");
        }
    }
}

/// The home the local-skill rule matches: `HOME`, or `USERPROFILE` when
/// `HOME` is unset or empty (R20).
#[cfg(test)]
mod home_tests {
    use super::*;

    fn only(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn the_home_falls_back_to_userprofile_when_home_is_unset() {
        assert_eq!(
            skills_home(&only(&[("USERPROFILE", r"C:\Users\ada")])).as_deref(),
            Some(r"C:\Users\ada")
        );
        assert_eq!(
            skills_home(&only(&[("HOME", ""), ("USERPROFILE", "/home/ada")])).as_deref(),
            Some("/home/ada")
        );
        assert_eq!(
            skills_home(&only(&[("HOME", "/home/bo"), ("USERPROFILE", "/home/ada")])).as_deref(),
            Some("/home/bo")
        );
        assert_eq!(skills_home(&only(&[])), None);
    }

    #[test]
    fn doctor_warns_when_the_bypass_skips_the_check_and_names_a_value_it_does_not_take() {
        let off = doctor_lines(&only(&[(BYPASS, "1")]));
        assert_eq!(off.len(), 1, "{off:?}");
        assert!(
            off[0].starts_with("warn guardrail_check_off: OML_HOOK_DISABLE=1")
                && off[0].contains("no tool call is checked"),
            "{off:?}"
        );
        let odd = doctor_lines(&only(&[(BYPASS, "yes")]));
        assert!(
            odd.len() == 1 && odd[0].contains("\"yes\"") && odd[0].contains("takes 1"),
            "{odd:?}"
        );
        assert!(doctor_lines(&only(&[])).is_empty());
        assert!(doctor_lines(&only(&[(BYPASS, "")])).is_empty());
        assert!(doctor_lines(&only(&[(BYPASS, "0")])).is_empty());
    }
}

// The same cases with drive paths, mixed separators and a Windows home,
// for the Windows leg of CI.
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    const HOME: &str = r"C:\Users\ada";
    const CWD: &str = r"C:\work\repo";

    fn asks(tool: &str, field: &str, path: &str, project: Option<&str>) -> bool {
        let input = json!({ field: path });
        let call = ToolCall {
            tool_name: tool,
            tool_input: &input,
            cwd: Some(CWD),
        };
        local_skill_write(&call, Some(HOME), project)
    }

    #[test]
    fn multiedit_and_notebookedit_under_claude_skills_ask_on_windows() {
        assert!(asks(
            "MultiEdit",
            "file_path",
            r".claude\skills\x\SKILL.md",
            None
        ));
        assert!(asks(
            "NotebookEdit",
            "notebook_path",
            r"~\.claude\skills\x\n.ipynb",
            None
        ));
        assert!(asks(
            "Write",
            "file_path",
            "~/.claude/skills/x/SKILL.md",
            None
        ));
        assert!(asks(
            "Write",
            "file_path",
            r"C:\Users\ada\.claude\skills\x\SKILL.md",
            None
        ));
        assert!(!asks(
            "NotebookEdit",
            "file_path",
            r"~\.claude\skills\x\n.ipynb",
            None
        ));
    }

    #[test]
    fn a_write_under_the_claude_project_dir_skills_asks_on_windows() {
        let path = r"D:\project\.claude\skills\x\SKILL.md";
        assert!(!asks("Write", "file_path", path, None));
        assert!(asks("Write", "file_path", path, Some(r"D:\project")));
    }

    #[test]
    fn a_dot_dot_that_leaves_the_skills_directory_does_not_ask_on_windows() {
        assert!(!asks(
            "Write",
            "file_path",
            r".claude\skills\..\notes.md",
            None
        ));
        assert!(!asks(
            "Write",
            "file_path",
            r"C:\work\repo\.claude\skills\x\..\..\a.md",
            None
        ));
        assert!(asks(
            "Write",
            "file_path",
            r"src\..\.claude\skills\x\SKILL.md",
            None
        ));
    }

    #[test]
    fn a_case_variant_or_mixed_separators_of_claude_skills_ask_on_windows() {
        for path in [
            "c:/WORK/Repo/.Claude/Skills/x/SKILL.md",
            r"C:\work\repo/.claude\SKILLS/x\SKILL.md",
            r"~\.CLAUDE\skills\x\SKILL.md",
            ".claude\\\u{17F}kills\\x\\SKILL.md",
        ] {
            assert!(asks("Write", "file_path", path, None), "{path}");
        }
        assert!(!asks(
            "Write",
            "file_path",
            r".claude\skillset\SKILL.md",
            None
        ));
    }
}
