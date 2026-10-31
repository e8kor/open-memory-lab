//! The M1 domain model slice: roles, key holders, the claim kinds, the claim
//! state set and its transition function, capture's NFC normalization, the
//! constitution's R2 reviewer check (ADR-0001 d2, admission.md), and the
//! crew comparisons of the independence axis (ADR-0026 d8).

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization as _;
use uuid::Uuid;

/// The five roles (ADR-0010, admission.md). The order is the tables' listing
/// order — it sorts a role set (`crate::access::effective_roles`) — never a
/// privilege order: roles do not imply one another (roles-and-keys.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Reader,
    Writer,
    Dreamer,
    Admin,
    Operator,
}

impl Role {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Reader => "reader",
            Role::Writer => "writer",
            Role::Dreamer => "dreamer",
            Role::Admin => "admin",
            Role::Operator => "operator",
        }
    }
}

/// The issuing admin's journalled attestation of who holds a key
/// (event-catalog.md, `key.issued`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeldBy {
    Agent,
    Human,
    Service,
}

impl HeldBy {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            HeldBy::Agent => "agent",
            HeldBy::Human => "human",
            HeldBy::Service => "service",
        }
    }
}

/// A claim's kind, as a submission declares it and `memories.kind` stores it
/// (http-api.md submit; postgres-schema.md `memories`; ADR-0025 d8). A
/// successor's kind changes only within its class (audit-and-lineage.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryKind {
    Memory,
    DerivedFact,
    Steering,
    Term,
    TermMapping,
    VocabRelease,
}

impl MemoryKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryKind::Memory => "memory",
            MemoryKind::DerivedFact => "derived_fact",
            MemoryKind::Steering => "steering",
            MemoryKind::Term => "term",
            MemoryKind::TermMapping => "term_mapping",
            MemoryKind::VocabRelease => "vocab_release",
        }
    }

    /// The kind an exact name spells; `None` for any other string, which a
    /// caller refuses rather than coerces (errors.md, unknown enum values).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "memory" => Some(MemoryKind::Memory),
            "derived_fact" => Some(MemoryKind::DerivedFact),
            "steering" => Some(MemoryKind::Steering),
            "term" => Some(MemoryKind::Term),
            "term_mapping" => Some(MemoryKind::TermMapping),
            "vocab_release" => Some(MemoryKind::VocabRelease),
            _ => None,
        }
    }
}

/// A claim version's state: the design set of the `memories.state` column
/// (admission.md lifecycle; postgres-schema.md). Supersession adds no state:
/// a replaced version stays promoted and only stops being the tip
/// (ADR-0025 d10). `Demoted` and `Erased` complete the set; demote and erase
/// are unbuilt, so [`transition`] reaches neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryStatus {
    Held,
    Promoted,
    Demoted,
    Rejected,
    Erased,
}

impl MemoryStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryStatus::Held => "held",
            MemoryStatus::Promoted => "promoted",
            MemoryStatus::Demoted => "demoted",
            MemoryStatus::Rejected => "rejected",
            MemoryStatus::Erased => "erased",
        }
    }
}

/// A refused status transition.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("illegal memory status transition {from:?} -> {to:?} (admission.md lifecycle)")]
pub struct TransitionError {
    pub from: MemoryStatus,
    pub to: MemoryStatus,
}

/// The pure transition function of the built lifecycle:
/// held -> promoted | rejected. Everything else is refused — the gate changes
/// visibility exactly once, at ratification (admission.md). The diagram's
/// other edges (reopen, reassignment, dead letter, demote, erase) are
/// unbuilt; a supersession is no edge, since the tip moves and the state
/// does not (ADR-0025 d10).
pub fn transition(from: MemoryStatus, to: MemoryStatus) -> Result<MemoryStatus, TransitionError> {
    match (from, to) {
        (MemoryStatus::Held, MemoryStatus::Promoted)
        | (MemoryStatus::Held, MemoryStatus::Rejected) => Ok(to),
        _ => Err(TransitionError { from, to }),
    }
}

/// Claim text in Unicode Normalization Form C. Capture normalizes before
/// the plaintext digest, the scan and the `body_bytes` count, so all three
/// see one canonical byte form (admission.md, capture; ADR-0025 d6). Text
/// already in NFC comes back unchanged.
#[must_use]
pub fn nfc(text: &str) -> String {
    text.nfc().collect()
}

/// The derivation lineage closure of a row — the author plus every declared
/// ancestor's principal and key id, pending and ratified edges alike
/// (ADR-0001 d2). Only as good as what writers declared (ADR-0001 d6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineageClosure {
    principals: BTreeSet<String>,
    key_ids: BTreeSet<Uuid>,
}

impl LineageClosure {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_principal(&mut self, principal: impl Into<String>) {
        self.principals.insert(principal.into());
    }

    pub fn add_key_id(&mut self, key_id: Uuid) {
        self.key_ids.insert(key_id);
    }

    #[must_use]
    pub fn contains_principal(&self, principal: &str) -> bool {
        self.principals.contains(principal)
    }

    #[must_use]
    pub fn contains_key_id(&self, key_id: Uuid) -> bool {
        self.key_ids.contains(&key_id)
    }
}

/// A refused verdict.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum VerdictError {
    #[error(
        "verdicts are the dreamer surface; a {0:?} key cannot review (ADR-0001 R1, admission.md)"
    )]
    NotDreamer(Role),
    #[error("reviewer principal {0:?} appears in the row's lineage closure (ADR-0001 R2)")]
    PrincipalInLineage(String),
    #[error("reviewer key {0} appears in the row's lineage closure (ADR-0001 R2)")]
    KeyInLineage(Uuid),
}

/// The R2 check at verdict time: a dreamer whose key or principal appears
/// anywhere in the lineage closure — the author included — is refused the
/// verdict (ADR-0001 d2). The caller builds the closure with the author in it.
pub fn check_reviewer(
    reviewer_role: Role,
    reviewer_principal: &str,
    reviewer_key_id: Uuid,
    lineage: &LineageClosure,
) -> Result<(), VerdictError> {
    if reviewer_role != Role::Dreamer {
        return Err(VerdictError::NotDreamer(reviewer_role));
    }
    if lineage.contains_principal(reviewer_principal) {
        return Err(VerdictError::PrincipalInLineage(
            reviewer_principal.to_owned(),
        ));
    }
    if lineage.contains_key_id(reviewer_key_id) {
        return Err(VerdictError::KeyInLineage(reviewer_key_id));
    }
    Ok(())
}

/// A key's crew as the journal holds it at a promotion: the crew record the
/// key was issued into, fixed for the key's life, and that record's
/// external manager (ADR-0026 d8; roles-and-keys.md, review independence).
/// A key with no crew is `None` where one is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCrew {
    pub crew_id: Uuid,
    pub external_manager: Option<String>,
}

/// Whether a verdict establishes a crew-inequality requirement of the
/// independence axis: the authoring key and the reviewing key both have a
/// crew, and the crews differ (roles-and-keys.md, review independence;
/// ADR-0026 d8, amending ADR-0012 d4). A key with no crew satisfies none:
/// the verdict does not count toward the requirement, and the row waits for
/// one that does — failing closed by holding.
#[must_use]
pub fn satisfies_crew_inequality(author: Option<&KeyCrew>, reviewer: Option<&KeyCrew>) -> bool {
    match (author, reviewer) {
        (Some(author), Some(reviewer)) => author.crew_id != reviewer.crew_id,
        _ => false,
    }
}

/// Whether a verdict establishes a manager-inequality requirement: both
/// keys have a crew, both crews have an external manager, and the managers
/// differ (ADR-0026 d8, amending ADR-0012 d4). A key with no crew, or a crew
/// with no external manager, satisfies none, and the row waits as for
/// [`satisfies_crew_inequality`].
#[must_use]
pub fn satisfies_manager_inequality(author: Option<&KeyCrew>, reviewer: Option<&KeyCrew>) -> bool {
    match (
        author.and_then(|c| c.external_manager.as_deref()),
        reviewer.and_then(|c| c.external_manager.as_deref()),
    ) {
        (Some(author), Some(reviewer)) => author != reviewer,
        _ => false,
    }
}
