//! Vocabulary identifiers: the CURIE grammar over a tenant's prefix
//! registry, the vocabulary claim kinds, a term's structural `term_status`,
//! a deprecation's successors, and the register of `oml:` upper-vocabulary
//! labels — pure functions over values the caller has already read
//! (ADR-0021 d1, d2, d3; ADR-0027 d7).
//!
//! The rules are declared on `docs/architecture/ontologies.md` ("A
//! vocabulary is a workarea with a prefix", "The term lifecycle", "CURIEs
//! and permanence") and the CURIE pattern on `docs/api/openapi.yaml`
//! (`components.schemas.Curie`); this module mirrors them and adds nothing.
//!
//! Each refusal answers exactly one code of `docs/api/errors.md`:
//!
//! | Refusal | Code |
//! |---|---|
//! | [`CurieError`], every variant | `request.validation` |
//! | [`PrefixRefusal::Malformed`] | `request.validation`, `details.field: "prefix"` |
//! | [`PrefixRefusal::PrefixConflict`] | `vocab.prefix_conflict` |
//! | [`UnknownCurie`] | `vocab.unknown_curie` — on a citation; a vocab read answers absence instead |
//! | [`TermStatusError`], every variant | `request.validation` |

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;
use uuid::Uuid;

use crate::model::MemoryKind;

/// The prefix the provider-published upper vocabulary owns, reserved in
/// every tenant's namespace (ADR-0021 d2).
pub const OML_PREFIX: &str = "oml";

/// The vocabulary claim kinds, in the submit enum's order (ADR-0021 d1;
/// openapi.yaml `SubmitRequest.kind`).
pub const VOCABULARY_KINDS: [MemoryKind; 3] = [
    MemoryKind::Term,
    MemoryKind::TermMapping,
    MemoryKind::VocabRelease,
];

/// Whether `kind` is one of [`VOCABULARY_KINDS`] (ADR-0021 d1).
#[must_use]
pub fn is_vocabulary_kind(kind: MemoryKind) -> bool {
    VOCABULARY_KINDS.contains(&kind)
}

/// A malformed CURIE: the string fails openapi.yaml's
/// `^[A-Za-z][A-Za-z0-9_-]*:[^\s:]+$`. Every variant answers
/// `request.validation` (errors.md: what the contract does not admit is
/// refused, not coerced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CurieError {
    #[error("a CURIE is PREFIX:localname and no ':' separates the two (request.validation)")]
    NoSeparator,
    #[error(
        "a prefix is an ASCII letter, then ASCII letters, digits, '_' or '-' (request.validation)"
    )]
    Prefix,
    #[error(
        "a localname is one or more characters, none of them ':' or whitespace (request.validation)"
    )]
    Localname,
}

/// A compact URI, `PREFIX:localname`: a term's `claim_key`, permanent once
/// minted at first promotion (ADR-0021 d2). It keeps the exact string it was
/// parsed from and renders it back unchanged.
// reading: the docs normalize no CURIE, so none is normalized here (no case
// folding, no NFC); two CURIEs are equal when their strings are.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Curie {
    text: String,
    /// Byte offset of the separating ':'.
    colon: usize,
}

impl Curie {
    /// Parses `text` under the CURIE grammar: one ':' after a prefix
    /// (`[A-Za-z][A-Za-z0-9_-]*`), then a non-empty localname holding no
    /// ':' and no whitespace.
    pub fn parse(text: &str) -> Result<Self, CurieError> {
        let colon = text.find(':').ok_or(CurieError::NoSeparator)?;
        check_prefix(&text[..colon])?;
        let localname = &text[colon + 1..];
        if localname.is_empty() || localname.chars().any(|c| c == ':' || is_curie_space(c)) {
            return Err(CurieError::Localname);
        }
        Ok(Self {
            text: text.to_owned(),
            colon,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.text[..self.colon]
    }

    #[must_use]
    pub fn localname(&self) -> &str {
        &self.text[self.colon + 1..]
    }
}

impl fmt::Display for Curie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The prefix half of the CURIE grammar.
fn check_prefix(prefix: &str) -> Result<(), CurieError> {
    let mut chars = prefix.chars();
    let first_is_letter = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    if first_is_letter && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        Ok(())
    } else {
        Err(CurieError::Prefix)
    }
}

/// `\s` of the openapi.yaml pattern.
// reading: the pattern's `\s` is ECMA-262's (WhiteSpace and LineTerminator,
// U+FEFF among them) and Rust's `char::is_whitespace` is Unicode's
// White_Space (U+0085 among them); a localname refuses the union, the
// narrower admission.
fn is_curie_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// A refused prefix registration, in the vocabulary workarea's creation act
/// (ADR-0021 d2; http-api.md `POST /v2/workareas`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrefixRefusal {
    /// `request.validation`, `details.field: "prefix"`: the prefix fails the
    /// prefix half of the CURIE grammar.
    // reading: the docs state no grammar for a registered prefix apart from
    // the CURIE pattern, so a prefix is exactly what that pattern admits
    // before the ':'.
    #[error("{prefix:?} is not a CURIE prefix (request.validation)")]
    Malformed { prefix: String },
    /// `vocab.prefix_conflict`: the prefix is already registered in this
    /// tenant, or is `oml`, which the provider holds in every tenant.
    // reading: the docs reserve `oml:` without naming the code its
    // registration answers; a reserved prefix is one already registered, so
    // it answers the conflict, and only the exact string `oml` is reserved,
    // since prefixes compare as strings (postgres-schema.md vocab_prefixes).
    #[error("{prefix:?} is already registered in this tenant (vocab.prefix_conflict)")]
    PrefixConflict { prefix: String },
}

/// A citation of a CURIE that resolves to no term: its prefix is
/// registered nowhere in the tenant, or it is an `oml:` CURIE outside the
/// shipped register. Answers `vocab.unknown_curie` on a citation; a vocab
/// read answers absence instead (errors.md).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{curie} names no term in this tenant (vocab.unknown_curie)")]
pub struct UnknownCurie {
    pub curie: Curie,
}

/// Who owns a CURIE's prefix (ADR-0021 d2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurieOwner {
    /// The provider: an `oml:` label of the shipped register.
    Provider(OmlLabel),
    /// The vocabulary workarea that registered the prefix. Whether a term
    /// of that localname exists there is the caller's lookup.
    Vocabulary(Uuid),
}

/// One tenant's prefix registry, `vocab_prefixes`, folded from the journal:
/// each prefix maps to the vocabulary workarea whose creation registered
/// it. A prefix is permanent — never reused or reassigned — so nothing
/// removes one (ADR-0021 d2; postgres-schema.md).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrefixRegistry {
    prefixes: BTreeMap<String, Uuid>,
}

impl PrefixRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `prefix` for the vocabulary workarea `workarea_id`, refusing
    /// a malformed prefix, then one this tenant already holds or the
    /// reserved `oml`.
    pub fn register(&mut self, prefix: &str, workarea_id: Uuid) -> Result<(), PrefixRefusal> {
        if check_prefix(prefix).is_err() {
            return Err(PrefixRefusal::Malformed {
                prefix: prefix.to_owned(),
            });
        }
        if prefix == OML_PREFIX || self.prefixes.contains_key(prefix) {
            return Err(PrefixRefusal::PrefixConflict {
                prefix: prefix.to_owned(),
            });
        }
        self.prefixes.insert(prefix.to_owned(), workarea_id);
        Ok(())
    }

    /// The vocabulary workarea that registered `prefix`; `None` for `oml`
    /// and for any prefix this tenant has not registered.
    #[must_use]
    pub fn workarea_of(&self, prefix: &str) -> Option<Uuid> {
        self.prefixes.get(prefix).copied()
    }

    /// The owner of `curie`'s prefix: the provider for an `oml:` label of
    /// the register, else the registering vocabulary workarea.
    pub fn owner(&self, curie: &Curie) -> Result<CurieOwner, UnknownCurie> {
        let owner = if curie.prefix() == OML_PREFIX {
            OmlLabel::parse(curie.as_str()).map(CurieOwner::Provider)
        } else {
            self.workarea_of(curie.prefix()).map(CurieOwner::Vocabulary)
        };
        owner.ok_or_else(|| UnknownCurie {
            curie: curie.clone(),
        })
    }

    /// Whether the node whose ancestry (from the node to the root) is
    /// `ancestry` is a vocabulary node: a workarea in the subtree of one that
    /// registered a prefix (ADR-0027 d2). An empty ancestry is no node.
    #[must_use]
    pub fn is_vocabulary_node(&self, ancestry: &[Uuid]) -> bool {
        self.prefixes.values().any(|w| ancestry.contains(w))
    }
}

/// The register of the provider-published `oml:` upper vocabulary: the
/// labels of term edges and of classifications, shipped product data
/// versioned with the release, never journal content (ADR-0021 d2, d5;
/// ADR-0027 d7; RS-10).
// reading: ADR-0021 d2 lists `is_a`, `part_of`, `replaced_by`, `consider`
// and "…", and ADR-0027 d7 adds `about`; RS-10 owns the rest of the
// content, so the register is these five and no other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OmlLabel {
    IsA,
    PartOf,
    ReplacedBy,
    Consider,
    About,
}

/// Every label of the register, in the order the docs list them.
pub const OML_LABELS: [OmlLabel; 5] = [
    OmlLabel::IsA,
    OmlLabel::PartOf,
    OmlLabel::ReplacedBy,
    OmlLabel::Consider,
    OmlLabel::About,
];

impl OmlLabel {
    /// The label as a claim edge carries it: an `oml:` CURIE.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            OmlLabel::IsA => "oml:is_a",
            OmlLabel::PartOf => "oml:part_of",
            OmlLabel::ReplacedBy => "oml:replaced_by",
            OmlLabel::Consider => "oml:consider",
            OmlLabel::About => "oml:about",
        }
    }

    /// The label's localname under `oml:`.
    #[must_use]
    pub fn localname(self) -> &'static str {
        &self.as_str()[OML_PREFIX.len() + 1..]
    }

    /// The label an exact `oml:` CURIE spells; `None` for anything else.
    #[must_use]
    pub fn parse(label: &str) -> Option<Self> {
        OML_LABELS.into_iter().find(|l| l.as_str() == label)
    }

    /// Whether an edge with this label runs from a term. `oml:about` is the
    /// one relation whose source is not a term: a classification runs from
    /// a memory version (ADR-0027 d7).
    #[must_use]
    pub fn source_is_term(self) -> bool {
        self != OmlLabel::About
    }
}

/// A term's status, read from its structural `term_status` field: active
/// is a promoted tip with `term_status: active`, deprecated one with
/// `term_status: deprecated` (ADR-0021 d1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TermStatus {
    Active,
    Deprecated,
}

impl TermStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TermStatus::Active => "active",
            TermStatus::Deprecated => "deprecated",
        }
    }

    /// The status an exact name spells; `None` for any other string.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "active" => Some(TermStatus::Active),
            "deprecated" => Some(TermStatus::Deprecated),
            _ => None,
        }
    }
}

/// A refused `term_status` field. Every variant answers
/// `request.validation`, `details.field: "term_status"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TermStatusError {
    /// A term carries no `term_status`.
    // reading: openapi.yaml does not list `term_status` as required, and
    // the docs name no default; a term without the field is refused rather
    // than read as active.
    #[error("a term carries its term_status as a structural field (request.validation)")]
    Missing,
    /// The field is neither `"active"` nor `"deprecated"`.
    #[error("term_status is active or deprecated (request.validation)")]
    Unknown,
    /// The field is sent on a kind other than `term`.
    #[error("term_status is kind=term only; this claim is {kind:?} (request.validation)")]
    NotATerm { kind: MemoryKind },
}

/// A claim's `term_status`, read from the `term_status` member of its
/// structural fields — `submission`, the clear JSON object the version was
/// submitted with — and never from its body text, so the marker is read
/// deterministically and survives crypto-shredding of the body
/// (ADR-0021 d1; openapi.yaml `SubmitRequest.term_status`). `Some` for a
/// term, `None` for any other kind. An absent or `null` member is the same
/// absence.
pub fn term_status(
    kind: MemoryKind,
    submission: &Value,
) -> Result<Option<TermStatus>, TermStatusError> {
    let field = submission.get("term_status").filter(|v| !v.is_null());
    match (kind, field) {
        (MemoryKind::Term, None) => Err(TermStatusError::Missing),
        (MemoryKind::Term, Some(value)) => value
            .as_str()
            .and_then(TermStatus::parse)
            .map(Some)
            .ok_or(TermStatusError::Unknown),
        (_, None) => Ok(None),
        (kind, Some(_)) => Err(TermStatusError::NotATerm { kind }),
    }
}

/// How a successor relates to the term a deprecation retires (ADR-0021 d3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuccessorRelation {
    /// `oml:replaced_by`: exactly one successor, safe for automated
    /// rewiring.
    ReplacedBy,
    /// `oml:consider`: one or more successors, each a judgment call.
    Consider,
}

impl SuccessorRelation {
    /// The relation as `vocab.inactive_curie`'s `details.relation` spells it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SuccessorRelation::ReplacedBy => "replaced_by",
            SuccessorRelation::Consider => "consider",
        }
    }
}

/// A deprecated term's ratified successor pointer, declared in its
/// deprecating submission: exactly one `replaced_by`, or one or more
/// `consider` (ADR-0021 d3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Successors {
    ReplacedBy(Curie),
    Consider(Vec<Curie>),
}

impl Successors {
    /// `details.relation` of `vocab.inactive_curie`.
    #[must_use]
    pub fn relation(&self) -> SuccessorRelation {
        match self {
            Successors::ReplacedBy(_) => SuccessorRelation::ReplacedBy,
            Successors::Consider(_) => SuccessorRelation::Consider,
        }
    }

    /// `details.successors` of `vocab.inactive_curie`.
    #[must_use]
    pub fn curies(&self) -> &[Curie] {
        match self {
            Successors::ReplacedBy(curie) => std::slice::from_ref(curie),
            Successors::Consider(curies) => curies,
        }
    }
}
