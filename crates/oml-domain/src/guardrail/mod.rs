//! Guardrails — the pure core of proposed ADR-0037: the kind's fields and
//! their admission caps (d2, d7, d10), the rule grammar `oml-gr/1` and its
//! tool classes (d8), the linear-time glob (d8), the set's order and its
//! conflicts (d4), the check before a tool call (d8, d9) and the rule
//! cache's codec (d9).
//!
//! The record is proposed, not accepted; this module mirrors it and adds
//! nothing the record does not state, except where a reading had to be
//! taken, which each item's documentation names. Serving, the hooks, the
//! file system and the clock are not here.

mod argument;
mod cache;
mod check;
mod glob;
mod rule;
mod shell;

use std::collections::BTreeSet;

use serde_json::Value;
use uuid::Uuid;

use crate::envelope::{EventTime, sha256_digest_string};
use crate::jcs::canonical_bytes;

pub use argument::{normalise_path, url_host};
pub use cache::{
    CacheRefusal, CacheState, CachedRule, SessionIdRefusal, cache_file_name, decode_cache,
    encode_cache,
};
pub use check::{Cause, Decision, NO_CACHE_WARNING, ToolCall, Uncheckable, Verdict, check, decide};
pub use glob::Glob;
pub use rule::{HostPattern, Matcher, Rule, RuleDefect, ToolClass};
pub use shell::{ShellRefusal, ShellSegment, shell_command, shell_segments};

/// The one rule grammar (ADR-0037 d8). A cache naming another denies every
/// call (d9).
pub const GRAMMAR: &str = "oml-gr/1";

/// Admission caps a guardrail's body at 300 UTF-8 bytes (ADR-0037 d7).
pub const BODY_MAX_BYTES: usize = 300;

/// Admission caps a rule at 160 bytes, measured on its RFC 8785 canonical
/// form (ADR-0037 d7; the form is this module's reading).
pub const RULE_MAX_BYTES: usize = 160;

/// The hook caps a line's provenance at 200 characters (ADR-0037 d7).
pub const PROVENANCE_MAX_CHARS: usize = 200;

/// The read lists at most five of the caller's own held guardrails
/// (ADR-0037 d3, d7).
pub const OWN_HELD_MAX_LINES: usize = 5;

/// At most 12 guardrails apply at a node; over it no partial set is printed
/// (ADR-0037 d7).
pub const SET_CAP: usize = 12;

/// Cache files older than 30 days are removed at a session start
/// (ADR-0037 d9).
pub const CACHE_MAX_AGE_DAYS: u32 = 30;

/// What the serve of a held guardrail adds, to its author and to its
/// reviewers alike (ADR-0037 d10, amending ADR-0012 d5).
pub const LOAD_WARNING: &str = "every key that reads this workarea will load this rule, here and below it unless its guardrail scope is workarea";

/// A key's guardrail scope, the issuance attribute that bounds which
/// applicable guardrails the key loads (ADR-0037 d4): `workarea` serves the
/// guardrails on the scope node alone, `ancestors` those of the node and of
/// every ancestor of it the key reads. An admin sets it at issuance and only
/// a rotation changes it; no client sets it, and no `off` value exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardrailScope {
    Workarea,
    Ancestors,
}

impl GuardrailScope {
    /// The issuance default, *the maintainer's reading to confirm*
    /// (ADR-0037 d4).
    pub const ISSUANCE_DEFAULT: Self = GuardrailScope::Ancestors;

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GuardrailScope::Workarea => "workarea",
            GuardrailScope::Ancestors => "ancestors",
        }
    }

    /// The scope an exact word spells; `None` for any other string, `off`
    /// included, which a caller refuses rather than coerces.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "workarea" => Some(GuardrailScope::Workarea),
            "ancestors" => Some(GuardrailScope::Ancestors),
            _ => None,
        }
    }

    /// The scope a journalled `key.issued` names in its `guardrail_scope`
    /// field: an issuance without the field, every key issued before the
    /// build included, folds as the issuance default; nothing in the journal
    /// is rewritten (ADR-0037 d4, *the maintainer's reading to confirm*).
    /// `None` for a value the field cannot hold, which a caller fails closed
    /// on.
    #[must_use]
    pub fn of_issuance(field: Option<&Value>) -> Option<Self> {
        match field {
            None | Some(Value::Null) => Some(Self::ISSUANCE_DEFAULT),
            Some(Value::String(word)) => Self::parse(word),
            Some(_) => None,
        }
    }
}

/// The served set's `set_digest` (ADR-0037 d5): `sha256:` over the RFC 8785
/// canonical form of the served `[artifact_id, version]` pairs, in the
/// served order. Clients only compare it (d11); its form is this module's
/// reading.
#[must_use]
pub fn set_digest(served: &[(Uuid, u32)]) -> String {
    let pairs: Vec<Value> = served
        .iter()
        .map(|(id, version)| Value::Array(vec![Value::from(id.to_string()), Value::from(*version)]))
        .collect();
    // An array of strings and small integers always has a canonical form.
    let bytes = canonical_bytes(&Value::Array(pairs)).unwrap_or_default();
    sha256_digest_string(&bytes)
}

/// How binding a guardrail is (ADR-0037 d2): `never` and `must` are
/// binding, after RFC 2119's MUST NOT and MUST, and `should` is the
/// reviewed default. The declaration order is the set's first order key,
/// strictest first: `never`, `must`, `should` (d4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Never,
    Must,
    Should,
}

impl Severity {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Never => "never",
            Severity::Must => "must",
            Severity::Should => "should",
        }
    }

    /// The severity an exact word spells; `None` for any other string,
    /// which a caller refuses rather than coerces.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "never" => Some(Severity::Never),
            "must" => Some(Severity::Must),
            "should" => Some(Severity::Should),
            _ => None,
        }
    }

    /// Whether a rule of this severity may take `effect`: `never` and
    /// `must` deny or ask, and `should` takes `ask` only (ADR-0037 d8).
    #[must_use]
    pub fn admits(self, effect: Effect) -> bool {
        !(self == Severity::Should && effect == Effect::Deny)
    }
}

/// A rule's effect, `deny` or `ask`; there is no `allow` (ADR-0037 d8). The
/// stricter effect is the greater, so the effect of several matching rules
/// is their maximum: deny wins over ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effect {
    Ask,
    Deny,
}

impl Effect {
    /// The `permissionDecision` value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Ask => "ask",
            Effect::Deny => "deny",
        }
    }

    /// `deny` or `ask`; `None` for `allow` and every other string.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "ask" => Some(Effect::Ask),
            "deny" => Some(Effect::Deny),
            _ => None,
        }
    }
}

/// Which capped field was over its cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CappedField {
    Body,
    Rule,
}

/// A guardrail the scan vetoes, under one of ADR-0037 d10's two finding
/// classes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardrailRefusal {
    /// Outside `oml-gr/1`, an `allow`, or a `should` that denies.
    #[error("the rule is not an oml-gr/1 rule its severity admits: {0} (ADR-0037 d10)")]
    RuleUnparsable(RuleDefect),
    /// A `should` rule whose effect is `deny`.
    #[error("a should guardrail's rule may only ask (ADR-0037 d8, d10)")]
    ShouldDenies,
    /// A body over 300 bytes or a rule over 160 (d7).
    #[error("the guardrail's {0:?} is over its cap (ADR-0037 d7, d10)")]
    TooLong(CappedField),
}

impl GuardrailRefusal {
    /// The finding class: `guardrail.rule_unparsable` or
    /// `guardrail.too_long` (ADR-0037 d10).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            GuardrailRefusal::RuleUnparsable(_) | GuardrailRefusal::ShouldDenies => {
                "guardrail.rule_unparsable"
            }
            GuardrailRefusal::TooLong(_) => "guardrail.too_long",
        }
    }
}

/// A guardrail's own fields: its severity, its body and its optional rule
/// (ADR-0037 d1, d2, d8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guardrail {
    pub severity: Severity,
    pub body: String,
    pub rule: Option<Rule>,
}

/// The scan of one guardrail version (ADR-0037 d10): the caps first, the
/// body's UTF-8 bytes and the rule's canonical bytes (d7), then the rule
/// under `oml-gr/1` and its severity (d8). The first finding is returned.
/// `body` is the body as capture normalised it.
pub fn admit(
    severity: Severity,
    body: &str,
    rule: Option<&Value>,
) -> Result<Guardrail, GuardrailRefusal> {
    if body.len() > BODY_MAX_BYTES {
        return Err(GuardrailRefusal::TooLong(CappedField::Body));
    }
    let rule = match rule {
        None => None,
        Some(value) => {
            // A value with no canonical form holds a float or an integer
            // past 2^53, which no oml-gr/1 member takes: name the defect.
            let Ok(canonical) = canonical_bytes(value) else {
                let defect = Rule::parse(value).err().unwrap_or(RuleDefect::NotAnObject);
                return Err(GuardrailRefusal::RuleUnparsable(defect));
            };
            if canonical.len() > RULE_MAX_BYTES {
                return Err(GuardrailRefusal::TooLong(CappedField::Rule));
            }
            let rule = Rule::parse(value).map_err(GuardrailRefusal::RuleUnparsable)?;
            if !severity.admits(rule.effect()) {
                return Err(GuardrailRefusal::ShouldDenies);
            }
            Some(rule)
        }
    };
    Ok(Guardrail {
        severity,
        body: body.to_owned(),
        rule,
    })
}

/// Orders applicable guardrails structurally, never by the query
/// (ADR-0037 d4): severity, strictest first, then depth from the root
/// ascending, then `promoted_at` ascending, then `artifact_id` ascending in
/// byte order. `position` names a row's (severity, depth, `promoted_at`,
/// `artifact_id`); `promoted_at` compares as its fixed-width signed string.
pub fn guardrail_order<T>(
    rows: &mut [T],
    position: impl Fn(&T) -> (Severity, u32, &EventTime, Uuid),
) {
    rows.sort_by(|a, b| {
        let (a_severity, a_depth, a_at, a_id) = position(a);
        let (b_severity, b_depth, b_at, b_id) = position(b);
        a_severity
            .cmp(&b_severity)
            .then_with(|| a_depth.cmp(&b_depth))
            .then_with(|| a_at.as_str().cmp(b_at.as_str()))
            .then_with(|| a_id.cmp(&b_id))
    });
}

/// Each row's `conflict` (ADR-0037 d4): rows sharing a `claim_key` restate
/// one topic, and when their severities are not all one, each row of that
/// key names the `artifact_id`s of the others, in the rows' order. A key
/// whose rows share one severity, or a key on one row, names none. All of
/// them still apply: no row is dropped, and the strictest comes first by
/// [`guardrail_order`]. `topic` names a row's (`claim_key`, severity,
/// `artifact_id`); the answer is index-aligned with `rows`.
pub fn conflicts<T>(rows: &[T], topic: impl Fn(&T) -> (&str, Severity, Uuid)) -> Vec<Vec<Uuid>> {
    let topics: Vec<(&str, Severity, Uuid)> = rows.iter().map(&topic).collect();
    topics
        .iter()
        .enumerate()
        .map(|(i, (key, _, _))| {
            let same: Vec<usize> = (0..topics.len()).filter(|&j| topics[j].0 == *key).collect();
            let severities: BTreeSet<Severity> = same.iter().map(|&j| topics[j].1).collect();
            if severities.len() < 2 {
                return Vec::new();
            }
            same.into_iter()
                .filter(|&j| j != i)
                .map(|j| topics[j].2)
                .collect()
        })
        .collect()
}
