//! The fields and keys of the claim kinds `skill`, `document` and `test`, as
//! pure checks (ADR-0080 d1, d9, d10; ADR-0081 d1, d2; ADR-0086 d1). Each
//! check answers the field a refusal names and why, so a transport refuses
//! `request.validation` naming the field; nothing here reads a store.

use crate::name::Name;

/// A skill body's bound: 32,768 UTF-8 bytes after NFC (ADR-0080 d9).
pub const SKILL_BODY_MAX_BYTES: usize = 32_768;

/// A skill name's bound, so `<name>-unreviewed` is still a label (ADR-0080
/// d1, d10).
pub const SKILL_NAME_MAX: usize = 52;

/// A skill description's bound, in characters (ADR-0080 d1).
pub const SKILL_DESCRIPTION_MAX: usize = 1024;

/// A document title's bound, in characters (ADR-0081 d1).
pub const DOCUMENT_TITLE_MAX: usize = 200;

/// A document key's bound and a segment's (ADR-0081 d2; ADR-0057 d7).
pub const DOCUMENT_KEY_MAX: usize = 128;
pub const DOCUMENT_SEGMENT_MAX: usize = 64;

/// The skill name the prompt list keeps for the guardrail set (ADR-0080
/// d5).
pub const RESERVED_SKILL_NAME: &str = "guardrails";

/// The suffix the prompt list gives a skill's held edit (ADR-0080 d10).
pub const UNREVIEWED_SUFFIX: &str = "-unreviewed";

/// Why a kind's field is refused: the JSON Pointer of the field and the
/// sentence the refusal carries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {reason}")]
pub struct FieldRefusal {
    pub field: &'static str,
    pub reason: &'static str,
}

const fn refused(field: &'static str, reason: &'static str) -> FieldRefusal {
    FieldRefusal { field, reason }
}

/// A test case's shape (ADR-0086 d1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestShape {
    Recall,
    Guardrail,
    Skill,
    Document,
}

impl TestShape {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TestShape::Recall => "recall",
            TestShape::Guardrail => "guardrail",
            TestShape::Skill => "skill",
            TestShape::Document => "document",
        }
    }

    /// The shape an exact word spells; `None` for any other.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "recall" => Some(TestShape::Recall),
            "guardrail" => Some(TestShape::Guardrail),
            "skill" => Some(TestShape::Skill),
            "document" => Some(TestShape::Document),
            _ => None,
        }
    }
}

/// Whether `name` may name a skill: ADR-0026 d2's creation grammar (a
/// lowercase RFC 1035 label), at most 52 characters, no `--`, and neither
/// `guardrails` nor a name ending in `-unreviewed` (ADR-0080 d1, d5, d10).
pub fn check_skill_name(name: &str) -> Result<(), FieldRefusal> {
    const FIELD: &str = "/skill/name";
    if Name::parse(name).is_err() {
        return Err(refused(
            FIELD,
            "a skill name is a lowercase label: a letter first, then letters, digits and hyphens, ending in a letter or digit",
        ));
    }
    if name.len() > SKILL_NAME_MAX {
        return Err(refused(FIELD, "a skill name is at most 52 characters"));
    }
    if name.contains("--") {
        return Err(refused(FIELD, "a skill name holds no `--`"));
    }
    if name == RESERVED_SKILL_NAME {
        return Err(refused(FIELD, "`guardrails` is the guardrail set's prompt"));
    }
    if name.ends_with(UNREVIEWED_SUFFIX) {
        return Err(refused(
            FIELD,
            "a name ending in `-unreviewed` is kept for a skill's held edit",
        ));
    }
    Ok(())
}

/// Whether `text` is one line of 1 to `max` characters.
fn one_line(text: &str, max: usize) -> bool {
    let count = text.chars().count();
    (1..=max).contains(&count) && !text.contains(['\n', '\r'])
}

/// Whether `description` may describe a skill: one line of 1 to 1024
/// characters (ADR-0080 d1).
pub fn check_skill_description(description: &str) -> Result<(), FieldRefusal> {
    if one_line(description, SKILL_DESCRIPTION_MAX) {
        Ok(())
    } else {
        Err(refused(
            "/skill/description",
            "a skill description is one line of 1 to 1024 characters",
        ))
    }
}

/// Whether a skill body is admitted: no front matter, since the name and
/// the description are fields (ADR-0080 d1).
pub fn check_skill_body(body: &str) -> Result<(), FieldRefusal> {
    if body
        .lines()
        .next()
        .is_some_and(|line| line.trim_end() == "---")
    {
        return Err(refused(
            "/body",
            "a skill body opens with no `---` front matter: the name and the description are fields",
        ));
    }
    Ok(())
}

/// A skill's claim key, derived and never chosen (ADR-0080 d1).
#[must_use]
pub fn skill_claim_key(name: &str) -> String {
    format!("skill/{name}")
}

/// Whether `title` may title a document: one line of 1 to 200 characters
/// (ADR-0081 d1).
pub fn check_document_title(title: &str) -> Result<(), FieldRefusal> {
    if one_line(title, DOCUMENT_TITLE_MAX) {
        Ok(())
    } else {
        Err(refused(
            "/document/title",
            "a document title is one line of 1 to 200 characters",
        ))
    }
}

/// Whether `key` is a document key: `doc/` and one or more segments of 1 to
/// 64 characters of `a-z 0-9 . _ -`, never `.` or `..`, the whole at most
/// 128 characters (ADR-0081 d2).
pub fn check_document_key(key: &str) -> Result<(), FieldRefusal> {
    let bad = || {
        refused(
            "/claim_key",
            "a document key is doc/ and segments of 1 to 64 of a-z, 0-9, ., _ and -, never . or .., at most 128 characters",
        )
    };
    let rest = key.strip_prefix("doc/").ok_or_else(bad)?;
    if key.len() > DOCUMENT_KEY_MAX {
        return Err(bad());
    }
    for segment in rest.split('/') {
        let valid = (1..=DOCUMENT_SEGMENT_MAX).contains(&segment.len())
            && segment != "."
            && segment != ".."
            && segment.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
            });
        if !valid {
            return Err(bad());
        }
    }
    Ok(())
}

/// A document's title when it sends none: the first level-1 heading of a
/// markdown body (ADR-0081 d1), cut to 200 characters at a character
/// boundary so it meets the title's bound. `None` when there is none; the
/// caller then takes the claim key's last segment.
#[must_use]
pub fn markdown_title(body: &str) -> Option<String> {
    body.lines()
        .filter_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .find(|title| !title.is_empty())
        .map(|title| title.chars().take(DOCUMENT_TITLE_MAX).collect())
}

/// The document key derived from a title, by ADR-0057 d7's rule
/// (ADR-0081 d2): the title's heading slug as GitHub renders it (lower
/// case, punctuation dropped, spaces to `-`), then every character outside
/// `a-z 0-9 . _ -` mapped to `-`. A slug longer than one segment's 64
/// characters keeps its first 47 and ends in `-` and the first 16 hex
/// digits of the SHA-256 of the full key, ADR-0057 d7's fitting cut to
/// ADR-0081 d2's segment bound, so two long titles sharing a prefix keep
/// distinct keys. The caller checks the result, so a title that slugs to
/// nothing, such as `.` or `..`, is refused.
#[must_use]
pub fn document_key_from_title(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c == ' ' {
                Some('-')
            } else if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else {
                None
            }
        })
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let key = format!("doc/{slug}");
    if slug.len() <= DOCUMENT_SEGMENT_MAX {
        return key;
    }
    let digest = crate::sha256_digest_string(key.as_bytes());
    let hex = digest.strip_prefix("sha256:").unwrap_or(&digest);
    let head: String = slug.chars().take(DOCUMENT_SEGMENT_MAX - 17).collect();
    format!("doc/{head}-{}", &hex[..16])
}

/// Whether `key` is a test case's key: `test/` and a name under ADR-0026
/// d2's creation grammar (ADR-0086 d1).
pub fn check_test_key(key: &str) -> Result<(), FieldRefusal> {
    match key.strip_prefix("test/") {
        Some(name) if Name::parse(name).is_ok() => Ok(()),
        _ => Err(refused(
            "/claim_key",
            "a test key is test/ and a lowercase label: a letter first, then letters, digits and hyphens",
        )),
    }
}
