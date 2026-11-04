//! The rule cache's codec: the served rules, each with its guardrail's
//! `artifact_id` and body, as one JSON line, then a line holding that
//! line's SHA-256 in lower-case hex (proposed ADR-0037 d9). Reading and
//! writing the file, its directory's modes, the rename and the 30-day sweep
//! are the hook's; this module holds the bytes and what each one means.
//!
//! The line is `{"rules":[{"artifact_id":…,"body":…,"rule":{…}}, …]}` in
//! RFC 8785 canonical form, and the file is `<line>\n<digest>\n` exactly.

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::GRAMMAR;
use crate::canonical::canonical_line;
use crate::rule::Rule;

/// One cached rule: the rule, with its guardrail's `artifact_id` and body
/// for the reason a decision prints.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CachedRule {
    pub artifact_id: Uuid,
    pub body: String,
    pub rule: Rule,
}

/// Why a cache file that exists cannot be checked. Each denies every call:
/// a known rule that cannot be checked fails closed (ADR-0037 d9).
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum CacheRefusal {
    /// Not two lines, not UTF-8, not the cache's JSON shape, or a rule of
    /// the known grammar that does not parse under it.
    #[error("does not parse")]
    Unparsable,
    /// The digest line is not the SHA-256 of the JSON line.
    #[error("has a digest line that does not match its rules")]
    DigestMismatch,
    /// A rule names a grammar this check does not know.
    #[error("names the unknown grammar {0:?}")]
    UnknownGrammar(String),
}

/// A session's cache as the check finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheState {
    /// No cache file: the check decides nothing and warns.
    NoCache,
    Loaded(Vec<CachedRule>),
    /// The file exists and cannot be checked: every call is denied.
    Broken(CacheRefusal),
}

/// The cache file for `rules`: one canonical JSON line, then its SHA-256.
#[must_use]
pub fn encode_cache(rules: &[CachedRule]) -> String {
    let rules: Vec<Value> = rules
        .iter()
        .map(|r| {
            json!({
                "artifact_id": r.artifact_id.to_string(),
                "body": r.body,
                "rule": r.rule.to_value(),
            })
        })
        .collect();
    let line = canonical_line(&json!({ "rules": rules }))
        .expect("the cache holds strings and arrays, never a number");
    let digest = hex::encode(Sha256::digest(line.as_bytes()));
    format!("{line}\n{digest}\n")
}

/// The cache state of a session from its file's bytes, `None` when there
/// is no file. The structure is checked first, then the digest, so that a
/// changed line reads as a mismatch, and then the JSON and each rule.
#[must_use]
pub fn decode_cache(contents: Option<&[u8]>) -> CacheState {
    let Some(contents) = contents else {
        return CacheState::NoCache;
    };
    match decode(contents) {
        Ok(rules) => CacheState::Loaded(rules),
        Err(refusal) => CacheState::Broken(refusal),
    }
}

fn decode(contents: &[u8]) -> Result<Vec<CachedRule>, CacheRefusal> {
    let text = std::str::from_utf8(contents).map_err(|_| CacheRefusal::Unparsable)?;
    let body = text.strip_suffix('\n').ok_or(CacheRefusal::Unparsable)?;
    let (line, digest) = body.split_once('\n').ok_or(CacheRefusal::Unparsable)?;
    if digest.contains('\n') {
        return Err(CacheRefusal::Unparsable);
    }
    if hex::encode(Sha256::digest(line.as_bytes())) != digest {
        return Err(CacheRefusal::DigestMismatch);
    }
    let value: Value = serde_json::from_str(line).map_err(|_| CacheRefusal::Unparsable)?;
    let rules = only(&value, &["rules"])?
        .get("rules")
        .and_then(Value::as_array)
        .ok_or(CacheRefusal::Unparsable)?;
    rules.iter().map(cached_rule).collect()
}

/// The value as an object with exactly these members.
fn only<'a>(value: &'a Value, members: &[&str]) -> Result<&'a Map<String, Value>, CacheRefusal> {
    let object = value.as_object().ok_or(CacheRefusal::Unparsable)?;
    if object.len() != members.len() || members.iter().any(|m| !object.contains_key(*m)) {
        return Err(CacheRefusal::Unparsable);
    }
    Ok(object)
}

fn cached_rule(value: &Value) -> Result<CachedRule, CacheRefusal> {
    let object = only(value, &["artifact_id", "body", "rule"])?;
    let artifact_id = object["artifact_id"]
        .as_str()
        .and_then(|s| Uuid::try_parse(s).ok())
        .ok_or(CacheRefusal::Unparsable)?;
    let body = object["body"]
        .as_str()
        .ok_or(CacheRefusal::Unparsable)?
        .to_owned();
    let rule = &object["rule"];
    let grammar = rule
        .get("rule")
        .and_then(Value::as_str)
        .ok_or(CacheRefusal::Unparsable)?;
    if grammar != GRAMMAR {
        return Err(CacheRefusal::UnknownGrammar(grammar.to_owned()));
    }
    let rule = Rule::parse(rule).map_err(|_| CacheRefusal::Unparsable)?;
    Ok(CachedRule {
        artifact_id,
        body,
        rule,
    })
}

/// A `session_id` that may not name a cache file: empty, or holding a
/// character outside `[A-Za-z0-9-]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a session_id names a cache file only in [A-Za-z0-9-] (ADR-0037 d9)")]
pub struct SessionIdRefusal;

/// The cache file's name in the cache directory, `<session_id>.json`, for
/// a `session_id` of letters, digits and hyphens only.
pub fn cache_file_name(session_id: &str) -> Result<String, SessionIdRefusal> {
    if session_id.is_empty()
        || !session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(SessionIdRefusal);
    }
    Ok(format!("{session_id}.json"))
}
