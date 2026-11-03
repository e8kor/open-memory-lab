//! The vocab scanner: what a workarea's `vocabulary_policy` checks on a
//! submission in the same call as capture — its `terms` citations against
//! the policy's pinned release, the `vocab.*` finding classes a vocabulary
//! submission raises, and the veto that keeps simulations from minting
//! vocabulary — pure, deterministic functions over values the caller has
//! already read (ADR-0021 d3, d5, d7, d8, d11).
//!
//! The rules are declared on `docs/architecture/admission.md` ("The
//! deterministic scan", "The rule-set registry", "Review policy objects"),
//! `docs/architecture/ontologies.md` ("Validation: the policy and the
//! scanner") and `docs/architecture/simulations.md`; this module mirrors
//! them and adds nothing. A [`Finding`] is a row of the closed registry's
//! `vocab.*` family — review-policy vocabulary that `human_required_classes`
//! may name, never an API refusal. Each refusal answers exactly one code of
//! `docs/api/errors.md`:
//!
//! | Refusal | Code |
//! |---|---|
//! | [`ScanRefusal::SimulationMint`] | `submission.scan_veto`; `details.finding` none declared — see the variant |
//! | [`ScanRefusal::UnknownCurie`] | `vocab.unknown_curie` |
//! | [`ScanRefusal::InactiveCurie`] | `vocab.inactive_curie`, `details` `{successors, relation}` |
//! | [`RequireWithoutRelease`] | none declared — see the type |

use std::collections::{BTreeMap, BTreeSet};

use uuid::Uuid;

use crate::model::MemoryKind;
use crate::vocab::{
    Curie, CurieOwner, PrefixRegistry, SuccessorRelation, Successors, TermStatus, UnknownCurie,
    is_vocabulary_kind,
};
use crate::vocab_edges::TipState;

/// `vocabulary_policy.enforcement`: whether a failing citation blocks
/// (`require`), records a finding (`annotate`), or is not checked at all
/// (`off`) (admission.md, review policy objects; ADR-0021 d8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Enforcement {
    Require,
    Annotate,
    Off,
}

impl Enforcement {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Enforcement::Require => "require",
            Enforcement::Annotate => "annotate",
            Enforcement::Off => "off",
        }
    }

    /// The mode an exact name spells; `None` for any other string.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "require" => Some(Enforcement::Require),
            "annotate" => Some(Enforcement::Annotate),
            "off" => Some(Enforcement::Off),
            _ => None,
        }
    }
}

/// `enforcement: require` without a pinned release: trunk-current checking
/// is annotate-only (ADR-0021 d8; ontologies.md, validation).
// reading: the docs say such a policy is refused and name no code, and the
// act that sets a review policy has no declared surface (configuration.md
// lists tenant policy as not on the settings surface); this refusal names
// none, and a caller must not invent one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "enforcement: require checks against a pinned release, and this policy pins none (ADR-0021 d8)"
)]
pub struct RequireWithoutRelease;

/// A workarea's `vocabulary_policy`: the vocabulary and pinned release the
/// vocab scanner checks cited CURIEs against, and its enforcement mode
/// (admission.md, review policy objects; ADR-0021 d8). The field is
/// optional and absent by default, and where it is absent the scanner is
/// not selected ([`scan`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VocabularyPolicy {
    vocabulary: Uuid,
    release: Option<Uuid>,
    enforcement: Enforcement,
}

impl VocabularyPolicy {
    /// The policy naming the vocabulary workarea `vocabulary`, pinning the
    /// `vocab_release` claim `release`, under `enforcement`; `require`
    /// without a pinned release is refused.
    pub fn new(
        vocabulary: Uuid,
        release: Option<Uuid>,
        enforcement: Enforcement,
    ) -> Result<Self, RequireWithoutRelease> {
        if enforcement == Enforcement::Require && release.is_none() {
            return Err(RequireWithoutRelease);
        }
        Ok(Self {
            vocabulary,
            release,
            enforcement,
        })
    }

    /// The vocabulary workarea whose prefix the cited CURIEs must carry.
    #[must_use]
    pub fn vocabulary(&self) -> Uuid {
        self.vocabulary
    }

    /// The pinned release — the `vocab_release` claim's artifact id — that
    /// the scan verdict records alongside `scanner_version`; `None` where
    /// checking is trunk-current, which the verdict records as the fold at
    /// submission seq (ADR-0021 d8).
    #[must_use]
    pub fn release(&self) -> Option<Uuid> {
        self.release
    }

    #[must_use]
    pub fn enforcement(&self) -> Enforcement {
        self.enforcement
    }
}

/// A `term_mapping`'s SSSOM predicate, its structural `mapping.predicate`
/// field (ADR-0021 d6; openapi.yaml `SubmitRequest.mapping`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MappingPredicate {
    ExactMatch,
    CloseMatch,
    BroadMatch,
    NarrowMatch,
    RelatedMatch,
}

impl MappingPredicate {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MappingPredicate::ExactMatch => "exactMatch",
            MappingPredicate::CloseMatch => "closeMatch",
            MappingPredicate::BroadMatch => "broadMatch",
            MappingPredicate::NarrowMatch => "narrowMatch",
            MappingPredicate::RelatedMatch => "relatedMatch",
        }
    }

    /// The predicate an exact name spells; `None` for any other string.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "exactMatch" => Some(MappingPredicate::ExactMatch),
            "closeMatch" => Some(MappingPredicate::CloseMatch),
            "broadMatch" => Some(MappingPredicate::BroadMatch),
            "narrowMatch" => Some(MappingPredicate::NarrowMatch),
            "relatedMatch" => Some(MappingPredicate::RelatedMatch),
            _ => None,
        }
    }
}

/// One cited CURIE as the caller resolves it from two reads, both within
/// the writer's effective read scope ([`CitedTerm::resolve`]):
///
/// - whether it is **active or deprecated** is its tip in the scan's
///   reference — the policy's pinned release, or without one (`annotate`
///   only) the trunk fold at submission seq;
/// - whether it **exists but is held** is read from the trunk fold at
///   submission seq, whatever the release, since a release pins promoted
///   tips only and never holds one.
///
/// A CURIE with neither read is absent from the lookup, and an absent one
/// is unknown: out of scope and nonexistent are indistinguishable
/// (ADR-0021 d8; ontologies.md, "Validation: the policy and the scanner";
/// errors.md `vocab.unknown_curie`).
// reading: ontologies.md defines `vocab.term_pending` as "the CURIE exists
// but is held" and checks existence and activity "against the pinned
// release"; a release holds no held term, so read against it alone the
// finding could never fire under a pinned `annotate` policy, and the agent
// loop's step 2 says it does. The held read is therefore the trunk's, and
// the tip read the reference's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CitedTerm {
    /// The CURIE exists but is still held: no version of it has promoted.
    Held,
    /// The term's served tip in the reference.
    Tip(TipState),
}

impl CitedTerm {
    /// One citation's entry in the lookup [`scan`] reads: `reference_tip`
    /// is the term's tip in the reference (the pinned release, or the
    /// trunk fold without one), `held_at_seq` whether the trunk fold at
    /// submission seq holds the CURIE with no promoted version. A tip in
    /// the reference wins; `None` is an unknown CURIE (ADR-0021 d8).
    #[must_use]
    pub fn resolve(reference_tip: Option<TipState>, held_at_seq: bool) -> Option<Self> {
        match reference_tip {
            Some(tip) => Some(CitedTerm::Tip(tip)),
            None if held_at_seq => Some(CitedTerm::Held),
            None => None,
        }
    }
}

/// What the scanner reads of one submission (http-api.md, submit;
/// openapi.yaml `SubmitRequest`).
#[derive(Debug, Clone, Copy)]
pub struct ScanSubject<'a> {
    pub kind: MemoryKind,
    /// The submission's provenance is `simulation`.
    pub simulation: bool,
    /// The target workarea is a vocabulary node
    /// ([`PrefixRegistry::is_vocabulary_node`]).
    pub in_vocabulary: bool,
    /// The submission's `terms`, in the order submitted.
    pub terms: &'a [Curie],
    /// The structural `term_status`, as [`crate::vocab::term_status`] reads
    /// it.
    pub term_status: Option<TermStatus>,
    /// The successors a deprecating submission declares.
    pub successors: Option<&'a Successors>,
    /// `mapping.predicate` of a `term_mapping`.
    pub mapping_predicate: Option<MappingPredicate>,
}

/// A finding of the vocab scanner, a `vocab.*` row of the closed registry:
/// an `annotate` class, recorded on the held item and never blocking
/// (admission.md, the rule-set registry; ADR-0021 d5, d7, d8).
// reading: the registry fixes each class's name and what it fires on, not
// what a finding carries; a finding about one citation or one successor
// names that CURIE, and the two routing classes name nothing, since they
// fire on the submission as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Finding {
    /// `vocab.term_pending`: a citation of a CURIE that exists but is still
    /// held — distinct from unknown.
    TermPending { curie: Curie },
    /// `vocab.deprecation_replaced_by`: a term deprecation carrying a
    /// `replaced_by` successor.
    DeprecationReplacedBy,
    /// `vocab.mapping_exactMatch`: a `term_mapping` claim asserting the
    /// `exactMatch` predicate.
    MappingExactMatch,
    /// `vocab.deprecated_successor`: a deprecation whose named successor is
    /// itself already deprecated — judgment, not law.
    DeprecatedSuccessor { successor: Curie },
}

impl Finding {
    /// The class as the registry spells it: the exact name
    /// `human_required_classes` references.
    #[must_use]
    pub fn class(&self) -> &'static str {
        match self {
            Finding::TermPending { .. } => "vocab.term_pending",
            Finding::DeprecationReplacedBy => "vocab.deprecation_replaced_by",
            Finding::MappingExactMatch => "vocab.mapping_exactMatch",
            Finding::DeprecatedSuccessor { .. } => "vocab.deprecated_successor",
        }
    }
}

/// A submission the scan refuses, journalling nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScanRefusal {
    /// `submission.scan_veto`: a `provenance: simulation` submission of a
    /// vocabulary kind into a vocabulary workarea — simulations cannot mint
    /// vocabulary, since a demoted simulation row would squat a permanent
    /// CURIE (ADR-0021 d11; simulations.md).
    // reading: the docs call this a deterministic scan-class veto, and a
    // veto's `details.finding` names the class that fired, but the closed
    // registry (admission.md) has no row for this one; this refusal names
    // no class, and a caller must not invent one.
    #[error(
        "a simulation cannot mint vocabulary; this submission is {kind:?} (submission.scan_veto)"
    )]
    SimulationMint { kind: MemoryKind },
    /// `vocab.unknown_curie`: under `require`, a cited CURIE names no term
    /// of the pinned release readable by the writer.
    #[error(transparent)]
    UnknownCurie(UnknownCurie),
    /// `vocab.inactive_curie`: under `require`, a cited term's tip in the
    /// pinned release is deprecated; the successors and their relation are
    /// the details a client self-heals from in one hop.
    #[error("{curie} is deprecated (vocab.inactive_curie)")]
    InactiveCurie {
        curie: Curie,
        successors: Successors,
    },
}

/// The vocab scanner over one submission, in the same call as capture and
/// before the commit transaction: a refusal journals nothing, and the
/// findings are recorded on the held item and never block (admission.md,
/// the deterministic scan; ADR-0021 d8, d11). First failure answering:
///
/// 1. **Simulations cannot mint vocabulary.** A `provenance: simulation`
///    submission of a vocabulary kind into a vocabulary node is
///    [`ScanRefusal::SimulationMint`], whatever the workarea's policy.
/// 2. **No policy, no scanner.** With no `vocabulary_policy` the scanner is
///    not selected and nothing else is checked: an O(1) no-op.
/// 3. **Citations, by enforcement.** Each of the submission's `terms` is
///    checked against `cited`, the caller's resolution under the policy's
///    reference ([`CitedTerm`]); a CURIE whose prefix is not the policy
///    vocabulary's names no term of its release, whatever `cited` says.
///    Under `require`, an active tip passes, a deprecated one is
///    [`ScanRefusal::InactiveCurie`] and anything else is
///    [`ScanRefusal::UnknownCurie`], the first failing citation in submitted
///    order answering; under `annotate`, a held term records
///    [`Finding::TermPending`] and nothing blocks; under `off`, nothing is
///    checked.
/// 4. **The routing classes, in every mode.** A term deprecation carrying a
///    `replaced_by` successor records [`Finding::DeprecationReplacedBy`];
///    one whose named successor's tip in `successor_tips` is deprecated
///    records [`Finding::DeprecatedSuccessor`]
///    ([`deprecated_successor_findings`]); a `term_mapping` asserting
///    `exactMatch` records [`Finding::MappingExactMatch`].
// reading: the docs place the simulation veto on vocabulary workareas, not
// on a policy, so it runs with or without one, and first, since it needs no
// lookup; they scope `enforcement` to "a failing citation", so `off` checks
// no citation and the routing classes fire under any selected policy.
// reading: under `require` a held term answers `vocab.unknown_curie`: the
// release pins no version of it, though the caller still reports it as
// [`CitedTerm::Held`] from the trunk read, and `require` treats held and
// absent alike.
// reading: under `annotate`, an unknown or deprecated citation records
// nothing: errors.md says the deprecated one "records a finding", but the
// closed registry has no class for it, and this function invents none.
// reading: the policy names one vocabulary and one release, so a citation
// under another prefix — another vocabulary's, an unregistered one, or
// `oml:` — has no term in that release.
pub fn scan(
    subject: &ScanSubject<'_>,
    policy: Option<&VocabularyPolicy>,
    registry: &PrefixRegistry,
    cited: &BTreeMap<Curie, CitedTerm>,
    successor_tips: &BTreeMap<Curie, TermStatus>,
) -> Result<BTreeSet<Finding>, ScanRefusal> {
    if subject.simulation && subject.in_vocabulary && is_vocabulary_kind(subject.kind) {
        return Err(ScanRefusal::SimulationMint { kind: subject.kind });
    }
    let Some(policy) = policy else {
        return Ok(BTreeSet::new());
    };
    let mut findings = check_citations(subject.terms, policy, registry, cited)?;
    if subject.kind == MemoryKind::Term
        && subject.term_status == Some(TermStatus::Deprecated)
        && let Some(successors) = subject.successors
    {
        if successors.relation() == SuccessorRelation::ReplacedBy {
            findings.insert(Finding::DeprecationReplacedBy);
        }
        findings.extend(deprecated_successor_findings(successors, successor_tips));
    }
    if subject.kind == MemoryKind::TermMapping
        && subject.mapping_predicate == Some(MappingPredicate::ExactMatch)
    {
        findings.insert(Finding::MappingExactMatch);
    }
    Ok(findings)
}

/// Step 3 of [`scan`]: the submission's `terms` under the policy's
/// enforcement.
fn check_citations(
    terms: &[Curie],
    policy: &VocabularyPolicy,
    registry: &PrefixRegistry,
    cited: &BTreeMap<Curie, CitedTerm>,
) -> Result<BTreeSet<Finding>, ScanRefusal> {
    let vocabulary = CurieOwner::Vocabulary(policy.vocabulary);
    let resolve = |curie: &Curie| {
        cited
            .get(curie)
            .filter(|_| registry.owner(curie) == Ok(vocabulary))
    };
    match policy.enforcement {
        Enforcement::Off => Ok(BTreeSet::new()),
        Enforcement::Annotate => Ok(terms
            .iter()
            .filter(|curie| resolve(curie) == Some(&CitedTerm::Held))
            .map(|curie| Finding::TermPending {
                curie: curie.clone(),
            })
            .collect()),
        Enforcement::Require => {
            for curie in terms {
                match resolve(curie) {
                    Some(CitedTerm::Tip(TipState::Active)) => {}
                    Some(CitedTerm::Tip(TipState::Deprecated(successors))) => {
                        return Err(ScanRefusal::InactiveCurie {
                            curie: curie.clone(),
                            successors: successors.clone(),
                        });
                    }
                    Some(CitedTerm::Held) | None => {
                        return Err(ScanRefusal::UnknownCurie(UnknownCurie {
                            curie: curie.clone(),
                        }));
                    }
                }
            }
            Ok(BTreeSet::new())
        }
    }
}

/// `vocab.deprecated_successor`, once for each named successor whose tip
/// `successor_tips` reads as deprecated; a successor absent from it records
/// nothing (ADR-0021 d5). [`scan`] applies it to the tips read at capture;
/// the registry names the tips at ratification time, so the caller applies
/// it again then.
// reading: a deprecated tip is never active again (ontologies.md, the term
// lifecycle), so a finding at capture still holds at ratification, and
// only a successor deprecated in between adds one.
#[must_use]
pub fn deprecated_successor_findings(
    successors: &Successors,
    successor_tips: &BTreeMap<Curie, TermStatus>,
) -> BTreeSet<Finding> {
    successors
        .curies()
        .iter()
        .filter(|curie| successor_tips.get(curie) == Some(&TermStatus::Deprecated))
        .map(|curie| Finding::DeprecatedSuccessor {
            successor: curie.clone(),
        })
        .collect()
}
