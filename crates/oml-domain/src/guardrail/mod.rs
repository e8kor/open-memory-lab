//! Guardrails in the domain — proposed ADR-0037's server side: the key's
//! guardrail scope (d4), admission and its caps (d7, d10), the set's order,
//! conflicts and digest (d4, d5) and the held guardrail's load warning
//! (d10). The rule grammar, the matcher, the check and the rule cache live
//! in the leaf crate `oml-guardrail`, re-exported here whole (d18 Q10).

use std::collections::BTreeSet;

use serde_json::Value;
use uuid::Uuid;

use crate::envelope::{EventTime, sha256_digest_string};
use crate::jcs::canonical_bytes;

pub use oml_guardrail::*;

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
