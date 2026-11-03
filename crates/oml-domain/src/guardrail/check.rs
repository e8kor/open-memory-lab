//! The check before a tool call: the cached rules against one call, with
//! no request, answering deny, ask or nothing, never allow; and the cache's
//! state deciding first, so a cache that cannot be checked denies and no
//! cache at all warns (proposed ADR-0037 d8, d9).

use serde_json::Value;
use uuid::Uuid;

use super::Effect;
use super::argument::{normalise_path, url_host};
use super::cache::{CacheRefusal, CacheState, CachedRule};
use super::glob::Glob;
use super::rule::{Matcher, ToolClass};
use super::shell::{ShellRefusal, shell_segments};

/// The `systemMessage` of a matching call in a session with no cache: it
/// decides nothing, and since the check keeps no state the line repeats on
/// each matching call (ADR-0037 d9, decided by the maintainer).
pub const NO_CACHE_WARNING: &str = "OML guardrails are not loaded for this session, so tool calls are not checked; restart the session to load them";

/// The word a `shell` word must not contain, and the tail of the cache
/// directory, for the built-in rule (ADR-0037 d9).
const CACHE_WORD: &str = "oml/guardrails";

/// One tool call as the PreToolUse hook input carries it: `tool_name`,
/// `tool_input` and the input's `cwd`.
#[derive(Debug, Clone, Copy)]
pub struct ToolCall<'a> {
    pub tool_name: &'a str,
    pub tool_input: &'a Value,
    pub cwd: Option<&'a str>,
}

/// Why a call of a class with a cached rule cannot be checked, so it asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Uncheckable {
    /// A `PowerShell` call: POSIX quoting does not tokenise it.
    PowerShell,
    /// A `Bash` command that does not tokenise or holds a construct the
    /// record asks on.
    Shell(ShellRefusal),
    /// A mapped call lacking its argument, or one whose argument cannot be
    /// read: a relative path with no `cwd`, a URL with no host this
    /// matcher can spell. Names the input field.
    MissingArgument(&'static str),
}

/// What decided a call.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Cause {
    /// A cached rule matched; the reason names its guardrail.
    Rule { artifact_id: Uuid, body: String },
    /// The built-in rule: an `edit` under the cache directory, or a `shell`
    /// word containing `oml/guardrails`.
    CacheDirectory,
    /// A rule of the call's class is cached and the call cannot be checked.
    Uncheckable(Uncheckable),
    /// The cache does not parse, its digest does not match or it names an
    /// unknown grammar: every call is denied.
    Cache(CacheRefusal),
}

/// A decision, `deny` or `ask`, with the causes of that effect only: when
/// any deny matched, the asks that also matched are not named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub effect: Effect,
    pub causes: Vec<Cause>,
}

impl Decision {
    /// The `permissionDecisionReason`: each cause, naming a rule's
    /// guardrail by `artifact_id` and body, and a broken cache's cause and
    /// its fix.
    #[must_use]
    pub fn reason(&self) -> String {
        let lines: Vec<String> = self
            .causes
            .iter()
            .map(|cause| match cause {
                Cause::Rule { artifact_id, body } => {
                    format!("OML guardrail {artifact_id}: {body}")
                }
                Cause::CacheDirectory => "OML guardrails: a built-in rule asks before any edit of the guardrail cache and any shell word naming oml/guardrails".to_owned(),
                Cause::Uncheckable(why) => format!(
                    "OML guardrails cannot check this call ({}), so it asks",
                    match why {
                        Uncheckable::PowerShell => "POSIX quoting does not tokenise PowerShell".to_owned(),
                        Uncheckable::Shell(refusal) => format!("the command does not tokenise: {refusal}"),
                        Uncheckable::MissingArgument(field) => format!("it lacks a checkable {field}"),
                    }
                ),
                Cause::Cache(refusal) => format!(
                    "OML guardrail cache {refusal}, so no tool call can be checked; restart the session to reload it"
                ),
            })
            .collect();
        lines.join("; ")
    }
}

/// What the PreToolUse hook does with one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Prints nothing: no rule matched, or the tool is matched by none.
    Nothing,
    /// Decides nothing and prints this one `systemMessage`.
    Warn(&'static str),
    /// Prints `permissionDecision` `deny` or `ask` with the reason.
    Decide(Decision),
}

/// The check of one call under the session's cache state (ADR-0037 d9's
/// table): a loaded cache is [`check`]ed; a broken one denies every call,
/// naming the cause and the fix; with no cache, a call of a mapped tool
/// warns and is decided by no one.
#[must_use]
pub fn decide(state: &CacheState, call: &ToolCall<'_>, cache_dir: &str) -> Verdict {
    match state {
        CacheState::Loaded(rules) => {
            check(rules, call, cache_dir).map_or(Verdict::Nothing, Verdict::Decide)
        }
        CacheState::Broken(refusal) => Verdict::Decide(Decision {
            effect: Effect::Deny,
            causes: vec![Cause::Cache(refusal.clone())],
        }),
        CacheState::NoCache => match ToolClass::of(call.tool_name) {
            Some(_) => Verdict::Warn(NO_CACHE_WARNING),
            None => Verdict::Nothing,
        },
    }
}

/// The cached rules and the built-in rule against one call: `None` when
/// nothing matches, else the strictest effect that matched, deny winning
/// over ask. `cache_dir` is the rule cache's directory, for the built-in
/// rule.
#[must_use]
pub fn check(rules: &[CachedRule], call: &ToolCall<'_>, cache_dir: &str) -> Option<Decision> {
    let class = ToolClass::of(call.tool_name)?;
    let of_class: Vec<&CachedRule> = rules.iter().filter(|r| r.rule.tool() == class).collect();
    let mut found: Vec<(Effect, Cause)> = Vec::new();
    let input = call.tool_input;
    let field = |name: &str| input.get(name).and_then(Value::as_str);
    let uncheckable = |found: &mut Vec<(Effect, Cause)>, why| {
        if !of_class.is_empty() {
            found.push((Effect::Ask, Cause::Uncheckable(why)));
        }
    };
    match class {
        ToolClass::Shell => match field("command") {
            None => uncheckable(&mut found, Uncheckable::MissingArgument("command")),
            Some(command) if call.tool_name == "PowerShell" => {
                uncheckable(&mut found, Uncheckable::PowerShell);
                if command.contains(CACHE_WORD) {
                    found.push((Effect::Ask, Cause::CacheDirectory));
                }
            }
            Some(command) => match shell_segments(command) {
                Err(refusal) => {
                    uncheckable(&mut found, Uncheckable::Shell(refusal));
                    if command.contains(CACHE_WORD) {
                        found.push((Effect::Ask, Cause::CacheDirectory));
                    }
                }
                Ok(segments) => {
                    for words in &segments {
                        if words.iter().any(|w| w.contains(CACHE_WORD)) {
                            found.push((Effect::Ask, Cause::CacheDirectory));
                        }
                        for r in &of_class {
                            if let Matcher::Command(list) = r.rule.matcher()
                                && Matcher::matches_words(list, words)
                            {
                                found.push((r.rule.effect(), cause(r)));
                            }
                        }
                    }
                }
            },
        },
        ToolClass::Edit | ToolClass::Read => {
            let (name, raw) = match call.tool_name {
                "Grep" | "Glob" => ("path", field("path").or(call.cwd)),
                _ => ("file_path", field("file_path")),
            };
            match raw.and_then(|p| normalise_path(p, call.cwd)) {
                None => uncheckable(&mut found, Uncheckable::MissingArgument(name)),
                Some(path) => {
                    if class == ToolClass::Edit && under(&path, cache_dir) {
                        found.push((Effect::Ask, Cause::CacheDirectory));
                    }
                    for r in &of_class {
                        let Matcher::Path(glob) = r.rule.matcher() else {
                            continue;
                        };
                        match normalise_path(glob, call.cwd) {
                            Some(glob) if Glob::path(&glob).matches(&path) => {
                                found.push((r.rule.effect(), cause(r)));
                            }
                            Some(_) => {}
                            None => uncheckable(&mut found, Uncheckable::MissingArgument("cwd")),
                        }
                    }
                }
            }
        }
        ToolClass::Fetch => match field("url").and_then(url_host) {
            None => uncheckable(&mut found, Uncheckable::MissingArgument("url")),
            Some(host) => {
                for r in &of_class {
                    if let Matcher::Host(pattern) = r.rule.matcher()
                        && pattern.matches(&host)
                    {
                        found.push((r.rule.effect(), cause(r)));
                    }
                }
            }
        },
        ToolClass::Mcp => {
            for r in &of_class {
                if let Matcher::Name(glob) = r.rule.matcher()
                    && Glob::word(glob).matches(call.tool_name)
                {
                    found.push((r.rule.effect(), cause(r)));
                }
            }
        }
    }
    let effect = found.iter().map(|(e, _)| *e).max()?;
    let mut causes: Vec<Cause> = Vec::new();
    for (e, c) in found {
        if e == effect && !causes.contains(&c) {
            causes.push(c);
        }
    }
    Some(Decision { effect, causes })
}

fn cause(r: &CachedRule) -> Cause {
    Cause::Rule {
        artifact_id: r.artifact_id,
        body: r.body.clone(),
    }
}

/// Whether the normalised `path` is `dir` or below it.
fn under(path: &str, dir: &str) -> bool {
    let Some(dir) = normalise_path(dir, None) else {
        return false;
    };
    path == dir
        || dir == "/"
        || path
            .strip_prefix(dir.as_str())
            .is_some_and(|r| r.starts_with('/'))
}
