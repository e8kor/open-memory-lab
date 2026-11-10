//! The rule grammar `oml-gr/1`: one JSON object per guardrail,
//! `{"rule": "oml-gr/1", "tool": <class>, <match>, "effect": "deny" | "ask"}`,
//! whose `<match>` member the class names (ADR-0037 d8).

use serde_json::{Map, Value, json};

use crate::argument::{ends_in_a_number, ipv4, ipv6};
use crate::glob::Glob;
use crate::{Effect, GRAMMAR};

/// The five tool classes a rule names, each over the Claude Code tools of
/// ADR-0037 d8's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ToolClass {
    /// `Bash`, matched on `command`; `PowerShell` is in the class and is
    /// asked on, never tokenised.
    Shell,
    /// `Write` and every tool whose name ends in `Edit`, matched on
    /// `file_path`.
    Edit,
    /// `Read` on `file_path`; `Grep` and `Glob` on `path`, or the input's
    /// `cwd` when absent.
    Read,
    /// `WebFetch`, matched on the host of `url`.
    Fetch,
    /// Every tool named `mcp__*`, matched on the tool name.
    Mcp,
}

impl ToolClass {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ToolClass::Shell => "shell",
            ToolClass::Edit => "edit",
            ToolClass::Read => "read",
            ToolClass::Fetch => "fetch",
            ToolClass::Mcp => "mcp",
        }
    }

    /// The class an exact rule `tool` value spells; `None` otherwise.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "shell" => Some(ToolClass::Shell),
            "edit" => Some(ToolClass::Edit),
            "read" => Some(ToolClass::Read),
            "fetch" => Some(ToolClass::Fetch),
            "mcp" => Some(ToolClass::Mcp),
            _ => None,
        }
    }

    /// The class of a harness tool name, or `None` for a tool no rule
    /// matches (`WebSearch` and `Agent` included). An `mcp__` name is class
    /// `mcp` even when it ends in `Edit`.
    #[must_use]
    pub fn of(tool_name: &str) -> Option<Self> {
        match tool_name {
            "Bash" | "PowerShell" => Some(ToolClass::Shell),
            "Read" | "Grep" | "Glob" => Some(ToolClass::Read),
            "WebFetch" => Some(ToolClass::Fetch),
            name if name.starts_with("mcp__") => Some(ToolClass::Mcp),
            "Write" => Some(ToolClass::Edit),
            name if name.ends_with("Edit") => Some(ToolClass::Edit),
            _ => None,
        }
    }

    /// The `<match>` member this class's rules carry.
    fn match_member(self) -> &'static str {
        match self {
            ToolClass::Shell => "command",
            ToolClass::Edit | ToolClass::Read => "path",
            ToolClass::Fetch => "host",
            ToolClass::Mcp => "name",
        }
    }
}

/// A `fetch` rule's host: exact, or a `*.` suffix that matches any host
/// with at least one label before the suffix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostPattern {
    Exact(String),
    /// The suffix after `*.`: `example.com` for `*.example.com`.
    Suffix(String),
}

impl HostPattern {
    #[must_use]
    pub fn matches(&self, host: &str) -> bool {
        match self {
            HostPattern::Exact(h) => host == h,
            HostPattern::Suffix(s) => host
                .strip_suffix(s.as_str())
                .and_then(|label| label.strip_suffix('.'))
                .is_some_and(|label| !label.is_empty()),
        }
    }
}

/// A rule's `<match>`: the value its class matches against.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Matcher {
    /// `shell`: word globs, matched against a segment's words from its
    /// first; a final `*` word matches zero or more words.
    Command(Vec<String>),
    /// `edit` and `read`: a path glob, made absolute against the input's
    /// `cwd` when relative.
    Path(String),
    /// `fetch`.
    Host(HostPattern),
    /// `mcp`: a glob against the tool name.
    Name(String),
}

impl Matcher {
    /// Whether `words`, one segment, matches a `command` list.
    #[must_use]
    pub fn matches_words(list: &[String], words: &[String]) -> bool {
        let (globs, open) = match list.split_last() {
            Some((last, init)) if last == "*" => (init, true),
            _ => (list, false),
        };
        let length_fits = if open {
            words.len() >= globs.len()
        } else {
            words.len() == globs.len()
        };
        length_fits
            && globs
                .iter()
                .zip(words)
                .all(|(g, w)| Glob::word(g).matches(w))
    }
}

/// Why a value is not an `oml-gr/1` rule, under `guardrail.rule_unparsable`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleDefect {
    #[error("a rule is a JSON object")]
    NotAnObject,
    #[error("the rule member is not \"oml-gr/1\"")]
    UnknownGrammar,
    #[error("tool is not shell, edit, read, fetch or mcp")]
    UnknownTool,
    #[error("effect is not deny or ask; there is no allow")]
    UnknownEffect,
    #[error("the {0} member is missing or not of its type")]
    BadMatch(&'static str),
    #[error("the member {0:?} is not in oml-gr/1")]
    UnknownMember(String),
}

/// A parsed `oml-gr/1` rule. Its fields agree by construction: the matcher
/// is the one its class takes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rule {
    tool: ToolClass,
    matcher: Matcher,
    effect: Effect,
}

impl Rule {
    /// Parses a rule object. Every member is required, none other is
    /// admitted, and every value is taken as it is or refused: a host is
    /// already lower-case and has no trailing dot, a glob and a word are
    /// non-empty, a command has at least one word.
    pub fn parse(value: &Value) -> Result<Self, RuleDefect> {
        let object = value.as_object().ok_or(RuleDefect::NotAnObject)?;
        if object.get("rule").and_then(Value::as_str) != Some(GRAMMAR) {
            return Err(RuleDefect::UnknownGrammar);
        }
        let tool = object
            .get("tool")
            .and_then(Value::as_str)
            .and_then(ToolClass::parse)
            .ok_or(RuleDefect::UnknownTool)?;
        let effect = object
            .get("effect")
            .and_then(Value::as_str)
            .and_then(Effect::parse)
            .ok_or(RuleDefect::UnknownEffect)?;
        let member = tool.match_member();
        if let Some(unknown) = object
            .keys()
            .find(|k| !matches!(k.as_str(), "rule" | "tool" | "effect") && *k != member)
        {
            return Err(RuleDefect::UnknownMember(unknown.clone()));
        }
        let bad = || RuleDefect::BadMatch(member);
        let text = |v: Option<&Value>| {
            v.and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let matcher = match tool {
            ToolClass::Shell => {
                let words = object
                    .get(member)
                    .and_then(Value::as_array)
                    .filter(|a| !a.is_empty())
                    .ok_or_else(bad)?;
                Matcher::Command(
                    words
                        .iter()
                        .map(|w| text(Some(w)))
                        .collect::<Option<_>>()
                        .ok_or_else(bad)?,
                )
            }
            ToolClass::Edit | ToolClass::Read => {
                Matcher::Path(text(object.get(member)).ok_or_else(bad)?)
            }
            ToolClass::Mcp => Matcher::Name(text(object.get(member)).ok_or_else(bad)?),
            ToolClass::Fetch => {
                let host = text(object.get(member)).ok_or_else(bad)?;
                Matcher::Host(host_pattern(&host).ok_or_else(bad)?)
            }
        };
        Ok(Rule {
            tool,
            matcher,
            effect,
        })
    }

    #[must_use]
    pub fn tool(&self) -> ToolClass {
        self.tool
    }

    #[must_use]
    pub fn matcher(&self) -> &Matcher {
        &self.matcher
    }

    #[must_use]
    pub fn effect(&self) -> Effect {
        self.effect
    }

    /// The rule as its JSON object, which [`Rule::parse`] reads back to the
    /// same rule.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let matcher = match &self.matcher {
            Matcher::Command(words) => json!(words),
            Matcher::Path(p) | Matcher::Name(p) => json!(p),
            Matcher::Host(HostPattern::Exact(h)) => json!(h),
            Matcher::Host(HostPattern::Suffix(s)) => json!(format!("*.{s}")),
        };
        let mut object = Map::new();
        object.insert("rule".to_owned(), json!(GRAMMAR));
        object.insert("tool".to_owned(), json!(self.tool.as_str()));
        object.insert(self.tool.match_member().to_owned(), matcher);
        object.insert("effect".to_owned(), json!(self.effect.as_str()));
        Value::Object(object)
    }
}

/// A host pattern as written: an exact host, or `*.` and a domain, each
/// already in the form the matched host is serialised in, so that no
/// admitted rule binds nothing (ADR-0037 d8). A domain is
/// lower-case ASCII letters, digits, `-` and `_` in non-empty dot-separated
/// labels with no trailing dot; one that ends in a number is an IPv4
/// address and is taken only as four canonical decimals, exact. An IPv6
/// address is taken bracketed, exact and in its WHATWG serialisation.
fn host_pattern(text: &str) -> Option<HostPattern> {
    let (suffix, host) = match text.strip_prefix("*.") {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if let Some(address) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return (!suffix && ipv6(address).as_deref() == Some(address))
            .then(|| HostPattern::Exact(host.to_owned()));
    }
    let domain = !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
        });
    if !domain || ends_in_a_number(host) && (suffix || ipv4(host).as_deref() != Some(host)) {
        return None;
    }
    Some(if suffix {
        HostPattern::Suffix(host.to_owned())
    } else {
        HostPattern::Exact(host.to_owned())
    })
}
