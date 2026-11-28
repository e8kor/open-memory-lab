//! The M1 domain model slice: roles, key holders, the claim kinds, the claim
//! state set and its transition function, capture's NFC normalization, the
//! constitution's R2 reviewer check (ADR-0001 d2, admission.md), and the
//! crew comparisons of the independence axis (ADR-0026 d8).

use std::collections::BTreeSet;

use sha2::{Digest as _, Sha256};
use unicode_normalization::UnicodeNormalization as _;
use uuid::Uuid;

/// The seven roles (ADR-0010; ADR-0097 d18 adds `maintenance`, admission.md; ADR-0093 d1). The order is the
/// tables' listing order — it sorts a role set
/// (`crate::access::effective_roles`) — never a privilege order: roles do not
/// imply one another (roles-and-keys.md). `Contributor` is a key role only,
/// whose recall is an explicit reader grant; `Submitter` is both the key role
/// that writes without recall and the grant role that confers writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Reader,
    Contributor,
    Submitter,
    Reviewer,
    Admin,
    Operator,
    /// A service's key that reads counts only and takes the rescan, in
    /// place of an admin key: no grant, no claim text, key value, email or
    /// key list (ADR-0097 d18).
    Maintenance,
}

/// Every key role's spelling, in the order [`Role`] declares them: the set
/// `scripts/release_markers.py` reads at each release tag, as it reads the
/// scanner's `RULESET`, to require the deployer's maintenance-key marker in
/// the first release whose set holds `maintenance` (ADR-0097 d18,
/// Production migration). A test holds it to [`Role::as_str`].
pub const KEY_ROLES: &[&str] = &[
    "reader",
    "contributor",
    "submitter",
    "reviewer",
    "admin",
    "operator",
    "maintenance",
];

impl Role {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Reader => "reader",
            Role::Contributor => "contributor",
            Role::Submitter => "submitter",
            Role::Reviewer => "reviewer",
            Role::Admin => "admin",
            Role::Operator => "operator",
            Role::Maintenance => "maintenance",
        }
    }
}

/// The issuing admin's journalled attestation of who holds a key
/// (event-catalog.md, `key.issued`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsedBy {
    Agent,
    Person,
    Service,
}

impl UsedBy {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UsedBy::Agent => "agent",
            UsedBy::Person => "person",
            UsedBy::Service => "service",
        }
    }
}

/// A claim's kind, as a submission declares it and `memories.kind` stores it
/// (http-api.md submit; postgres-schema.md `memories`; ADR-0025 d8). A
/// successor's kind changes only within its class (audit-and-lineage.md).
/// `Guardrail` is ADR-0037 d1's kind: a promoted rule with a
/// severity, served by its own read and never in recall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryKind {
    Memory,
    DerivedFact,
    Steering,
    Term,
    TermMapping,
    VocabRelease,
    Guardrail,
    /// A reviewed instruction the MCP server offers as a prompt (ADR-0080
    /// d1).
    Skill,
    /// A titled markdown or plain-text body recall cuts into passages
    /// (ADR-0081 d1).
    Document,
    /// A test case a workspace's suite runs (ADR-0086 d1).
    Test,
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
            MemoryKind::Guardrail => "guardrail",
            MemoryKind::Skill => "skill",
            MemoryKind::Document => "document",
            MemoryKind::Test => "test",
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
            "guardrail" => Some(MemoryKind::Guardrail),
            "skill" => Some(MemoryKind::Skill),
            "document" => Some(MemoryKind::Document),
            "test" => Some(MemoryKind::Test),
            _ => None,
        }
    }

    /// Every kind, in the order the submit enum lists them.
    pub const ALL: [MemoryKind; 10] = [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Steering,
        MemoryKind::Term,
        MemoryKind::TermMapping,
        MemoryKind::VocabRelease,
        MemoryKind::Guardrail,
        MemoryKind::Skill,
        MemoryKind::Document,
        MemoryKind::Test,
    ];

    /// The class whose rules the kind follows (ADR-0080 d16): knowledge is
    /// read as data, an instruction is followed by the agent that loads it,
    /// and vocabulary changes how queries match.
    #[must_use]
    pub fn class(self) -> ClaimClass {
        match self {
            MemoryKind::Memory
            | MemoryKind::DerivedFact
            | MemoryKind::Document
            | MemoryKind::Test => ClaimClass::Knowledge,
            MemoryKind::Steering | MemoryKind::Guardrail | MemoryKind::Skill => {
                ClaimClass::Instruction
            }
            MemoryKind::Term | MemoryKind::TermMapping | MemoryKind::VocabRelease => {
                ClaimClass::Vocabulary
            }
        }
    }
}

/// The three claim classes of ADR-0080 d16. A rule holds for every kind of
/// its class; only the instruction class needs a person's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimClass {
    Knowledge,
    Instruction,
    Vocabulary,
}

impl ClaimClass {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ClaimClass::Knowledge => "knowledge",
            ClaimClass::Instruction => "instruction",
            ClaimClass::Vocabulary => "vocabulary",
        }
    }

    /// Whether only a person's approving verdict counts on a version of
    /// this class (ADR-0080 d3, d16).
    #[must_use]
    pub fn needs_person(self) -> bool {
        self == ClaimClass::Instruction
    }
}

/// A claim version's state: the design set of the `memories.state` column
/// (admission.md lifecycle; postgres-schema.md). Supersession adds no state:
/// a replaced version stays promoted and only stops being the tip
/// (ADR-0025 d10). `Demoted` and `Erased` complete the set, reached only by
/// an admin's fix (ADR-0089 d16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryStatus {
    Held,
    Promoted,
    Demoted,
    Rejected,
    Erased,
    /// Retracted by its author while held: kept with its text, never served
    /// and in no review queue (ADR-0083 d13).
    Withdrawn,
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
            MemoryStatus::Withdrawn => "withdrawn",
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
/// held -> promoted | rejected | withdrawn, promoted -> demoted, and any
/// state but erased -> erased. The gate changes visibility at ratification
/// (admission.md); a withdrawal is the author retracting its own held
/// version, which promotes nothing (ADR-0083 d13). A demotion takes a
/// promoted version, served or superseded, and never a held one; an erase
/// takes every version of a claim whatever its state (ADR-0089 d16;
/// ADR-0005 d2, amended 2026-11-17). Reopen, reassignment and dead letter
/// are unbuilt; a supersession is no edge, since the tip moves and the
/// state does not (ADR-0025 d10).
pub fn transition(from: MemoryStatus, to: MemoryStatus) -> Result<MemoryStatus, TransitionError> {
    match (from, to) {
        (MemoryStatus::Held, MemoryStatus::Promoted)
        | (MemoryStatus::Held, MemoryStatus::Rejected)
        | (MemoryStatus::Held, MemoryStatus::Withdrawn)
        | (MemoryStatus::Promoted, MemoryStatus::Demoted) => Ok(to),
        (from, MemoryStatus::Erased) if from != MemoryStatus::Erased => Ok(to),
        _ => Err(TransitionError { from, to }),
    }
}

/// Who withdraws a held version (ADR-0083 d13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withdrawer {
    /// The version's own author key, an agent's included.
    AuthorKey,
    /// A person's session web key, for a version a key of that same person
    /// wrote, read the one way `same_user` reads it (ADR-0082 d2).
    PersonSession,
    /// Any other key.
    Other,
}

/// A refused withdrawal (ADR-0083 d13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WithdrawRefusal {
    /// Neither the author key nor a session key of the author's person
    /// (errors.md `access.author_only`).
    #[error("only the version's author withdraws it (access.author_only)")]
    AuthorOnly,
    /// The version is not held (errors.md `memory.not_held`).
    #[error("only a held version is withdrawn (memory.not_held)")]
    NotHeld,
}

/// The withdraw rule (ADR-0083 d13): the author key on its own held
/// version, or a person's session key on a held version any key of that
/// person wrote; who comes first, so another key learns nothing of the
/// version's state. It promotes, signs, demotes and deletes nothing (R1):
/// the state becomes `withdrawn` and the version is kept.
pub fn withdraw(status: MemoryStatus, by: Withdrawer) -> Result<MemoryStatus, WithdrawRefusal> {
    if by == Withdrawer::Other {
        return Err(WithdrawRefusal::AuthorOnly);
    }
    transition(status, MemoryStatus::Withdrawn).map_err(|_| WithdrawRefusal::NotHeld)
}

/// Claim text in Unicode Normalization Form C. Capture normalizes before
/// the plaintext digest, the scan and the `body_bytes` count, so all three
/// see one canonical byte form (admission.md, capture; ADR-0025 d6). Text
/// already in NFC comes back unchanged.
#[must_use]
pub fn nfc(text: &str) -> String {
    text.nfc().collect()
}

/// The prefix every server-derived `claim_key` carries; a writer may not
/// send a key that starts with it (ADR-0039 d6).
pub const DERIVED_CLAIM_KEY_PREFIX: &str = "auto-";

/// The `claim_key` the server derives when a submission omits one:
/// `auto-` plus the first 16 lowercase hex digits of the SHA-256 of the
/// author's key id (its hyphenated lowercase spelling), a zero byte and the
/// NFC text (ADR-0039 d6). With the author in the digest, two keys that
/// record one text hold two claims, and a conflict tells a writer only
/// about its own rows (ADR-0025 d5, #129).
#[must_use]
pub fn derive_claim_key(author_root_key_id: Uuid, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(author_root_key_id.to_string().as_bytes());
    hasher.update([0_u8]);
    hasher.update(nfc(text).as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("{DERIVED_CLAIM_KEY_PREFIX}{}", &digest[..16])
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
        "verdicts are the reviewer surface; a {0:?} key cannot review (ADR-0001 R1, admission.md)"
    )]
    NotReviewer(Role),
    #[error("reviewer principal {0:?} appears in the row's lineage closure (ADR-0001 R2)")]
    PrincipalInLineage(String),
    #[error("reviewer key {0} appears in the row's lineage closure (ADR-0001 R2)")]
    KeyInLineage(Uuid),
    /// An approval of an instruction-class version from a key that is not a
    /// person's reviewer key (ADR-0080 d3, d16): `review.person_required`,
    /// reason `instruction_needs_person`.
    #[error(
        "a {} version is an instruction, and only a person's reviewer key approves one (ADR-0080 d16)",
        .0.as_str()
    )]
    PersonRequired(MemoryKind),
}

/// A person's reviewer key: role reviewer, `used_by: person`, no crew and no
/// profile, the predicate ADR-0074 d2 (item 4) names after ADR-0055 d1 and
/// ADR-0030 d1.
#[must_use]
pub fn is_person_reviewer(role: Role, used_by: UsedBy, crew: bool, profile: bool) -> bool {
    role == Role::Reviewer && used_by == UsedBy::Person && !crew && !profile
}

/// Whether an approving verdict on a version of `kind` counts from this
/// key: always for knowledge and vocabulary, and for an instruction only
/// from a person's reviewer key (ADR-0080 d3, d16). A reject promotes
/// nothing and is not checked here.
pub fn check_approval_class(
    kind: MemoryKind,
    role: Role,
    used_by: UsedBy,
    crew: bool,
    profile: bool,
) -> Result<(), VerdictError> {
    if kind.class().needs_person() && !is_person_reviewer(role, used_by, crew, profile) {
        return Err(VerdictError::PersonRequired(kind));
    }
    Ok(())
}

/// The R2 check at verdict time: a reviewer whose key or principal appears
/// anywhere in the lineage closure — the author included — is refused the
/// verdict (ADR-0001 d2). The caller builds the closure with the author in it.
pub fn check_reviewer(
    reviewer_role: Role,
    reviewer_principal: &str,
    reviewer_key_id: Uuid,
    lineage: &LineageClosure,
) -> Result<(), VerdictError> {
    if reviewer_role != Role::Reviewer {
        return Err(VerdictError::NotReviewer(reviewer_role));
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
