//! Claim edges over vocabularies: the edge states and their transitions,
//! classification validation and the one-live-classification rule, cycle
//! refusal over `is_a` and `replaced_by`, the lineage closure without
//! `oml:about`, computed retirement, and recall's terms match — pure
//! functions over edges, versions and term tips the caller has already read
//! (ADR-0021 d3, d5, d9; ADR-0027 d6–d11).
//!
//! The rules are declared on `docs/architecture/ontologies.md`
//! ("Hierarchy and mappings"; "Classification: ontology crews attach terms
//! to memories"), `docs/architecture/audit-and-lineage.md` ("The derivation
//! graph") and `docs/architecture/retrieval.md` (the `terms` filter); this
//! module mirrors them and adds nothing.
//!
//! Each refusal answers exactly one code of `docs/api/errors.md`:
//!
//! | Refusal | Code |
//! |---|---|
//! | [`ClassificationRefusal::Validation`] | `request.validation`, `details.field` the endpoint |
//! | [`ClassificationRefusal::InactiveCurie`] | `vocab.inactive_curie`, `details` `{successors, relation}` |
//! | [`ClassificationRefusal::ClassificationExists`] | `vocab.classification_exists`, `details` `{source: "edge", edge_id, state}` or `{source: "terms"}` |
//! | [`CycleRefused`] | `vocab.cycle_refused` |
//! | [`EdgeTransitionError::RejectionNotAbout`] | `request.validation` |
//! | [`EdgeTransitionError::Illegal`] | none declared — see the variant |

use std::collections::{BTreeMap, BTreeSet};

use uuid::Uuid;

use crate::model::{MemoryKind, MemoryStatus};
use crate::vocab::{Curie, OmlLabel, Successors, TermStatus};

/// A claim edge's state, the set of `claim_edges.state`
/// (postgres-schema.md; ADR-0027 d8). Retirement is no state: it is computed
/// ([`is_retired`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeState {
    Pending,
    Ratified,
    Rejected,
}

impl EdgeState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeState::Pending => "pending",
            EdgeState::Ratified => "ratified",
            EdgeState::Rejected => "rejected",
        }
    }
}

/// A refused claim-edge transition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdgeTransitionError {
    /// `request.validation`: a rejection of an edge not labelled
    /// `oml:about` — other claim edges keep propose-then-ratify
    /// (ADR-0027 d8).
    #[error("only an oml:about edge is rejected; this edge is {label:?} (request.validation)")]
    RejectionNotAbout { label: String },
    /// A decided edge decided again, or a move back to pending.
    // reading: the docs declare no code for ratifying a ratified or rejected
    // edge, or rejecting a rejected one; this refusal names none, and a
    // caller must not invent one.
    #[error("illegal claim-edge transition {from:?} -> {to:?} (ADR-0027 d8)")]
    Illegal { from: EdgeState, to: EdgeState },
}

/// The claim-edge transition function: pending -> ratified for any label;
/// pending or ratified -> rejected for an `oml:about` edge only; rejected
/// is final, and its pair is proposed again as a new edge (ADR-0027 d8;
/// http-api.md, claim edges).
pub fn edge_transition(
    label: &str,
    from: EdgeState,
    to: EdgeState,
) -> Result<EdgeState, EdgeTransitionError> {
    match (from, to) {
        (EdgeState::Pending, EdgeState::Ratified) => Ok(to),
        (EdgeState::Pending | EdgeState::Ratified, EdgeState::Rejected) => {
            if label == OmlLabel::About.as_str() {
                Ok(to)
            } else {
                Err(EdgeTransitionError::RejectionNotAbout {
                    label: label.to_owned(),
                })
            }
        }
        _ => Err(EdgeTransitionError::Illegal { from, to }),
    }
}

/// A claim edge between two claim versions, as `claim_edges` holds it
/// (postgres-schema.md). The label vocabulary is open beyond `oml:`
/// (http-api.md, claim edges), so the label is a string; `term_curie` is
/// the target term's CURIE on an `oml:about` edge and `None` on every other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimEdge {
    pub edge_id: Uuid,
    /// `src_artifact_id`: the source version.
    pub from: Uuid,
    /// `dst_artifact_id`: the target version.
    pub to: Uuid,
    pub label: String,
    pub term_curie: Option<Curie>,
    pub state: EdgeState,
}

impl ClaimEdge {
    /// Whether the edge is a classification: labelled `oml:about`
    /// (ADR-0027 d6).
    #[must_use]
    pub fn is_classification(&self) -> bool {
        self.label == OmlLabel::About.as_str()
    }

    /// Whether the edge is a live classification of `version` under
    /// `curie`: pending or ratified (ADR-0027 d7).
    fn classifies(&self, version: Uuid, curie: &Curie) -> bool {
        self.is_classification()
            && self.from == version
            && self.term_curie.as_ref() == Some(curie)
            && self.state != EdgeState::Rejected
    }
}

/// A claim version as one endpoint of a classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointVersion {
    pub artifact_id: Uuid,
    pub kind: MemoryKind,
    pub status: MemoryStatus,
    /// The version's workarea is a vocabulary node
    /// ([`crate::vocab::PrefixRegistry::is_vocabulary_node`]).
    pub in_vocabulary: bool,
}

/// Which endpoint a [`ClassificationRefusal::Validation`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    From,
    To,
}

impl Endpoint {
    /// `details.field` of the refusal.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Endpoint::From => "from_artifact_id",
            Endpoint::To => "to_artifact_id",
        }
    }
}

/// A term's served tip, by its CURIE, as a classification reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermTip {
    pub curie: Curie,
    pub state: TipState,
}

/// Whether a term's served tip is active, or deprecated with the successor
/// pointer its deprecating submission declared (ADR-0021 d3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TipState {
    Active,
    Deprecated(Successors),
}

impl TipState {
    #[must_use]
    pub fn status(&self) -> TermStatus {
        match self {
            TipState::Active => TermStatus::Active,
            TipState::Deprecated(_) => TermStatus::Deprecated,
        }
    }
}

/// The live classification a duplicate meets (ADR-0027 d7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingClassification {
    /// `{source: "edge", edge_id, state}`: a pending or ratified
    /// `oml:about` edge joins the pair.
    Edge { edge_id: Uuid, state: EdgeState },
    /// `{source: "terms"}`: the version's own `terms` cite the CURIE.
    Terms,
}

/// A refused classification, at proposal or at ratification.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClassificationRefusal {
    /// `request.validation`, `details.field` naming the endpoint: the source
    /// is not a promoted `memory` or `derived_fact` version outside every
    /// vocabulary subtree, or the target is not a promoted `term` version
    /// (ADR-0027 d6).
    #[error("{} is not a classification endpoint (request.validation)", field.as_str())]
    Validation { field: Endpoint },
    /// `vocab.inactive_curie`: the term's served tip is deprecated; the
    /// successors and their relation are the details a client self-heals
    /// from in one hop (ADR-0027 d6; ADR-0021 d8).
    #[error("{curie} is deprecated (vocab.inactive_curie)")]
    InactiveCurie {
        curie: Curie,
        successors: Successors,
    },
    /// `vocab.classification_exists`: the version already carries a live
    /// classification under the CURIE (ADR-0027 d7).
    #[error("the version is already classified under this CURIE (vocab.classification_exists)")]
    ClassificationExists(ExistingClassification),
}

/// The classification checks at proposal (ontologies.md, classification;
/// ADR-0027 d6, d7), first failure answering: the source shape, the target
/// shape, the term's tip active, then one live classification per version
/// and CURIE. `cited` is the source version's own `terms`; `edges` may hold
/// any claim edges, of which the source's live `oml:about` edges count.
/// Returns the `term_curie` the edge records. Authority and R2 are the
/// access and review checks', not this function's.
// reading: the docs state each refusal but not their order; the shape
// checks come first, source before target, since they need nothing else,
// and the duplicate test last, as the unique index that is its authority
// answers at insertion.
pub fn check_classification(
    source: &EndpointVersion,
    target: &EndpointVersion,
    tip: &TermTip,
    cited: &[Curie],
    edges: &[ClaimEdge],
) -> Result<Curie, ClassificationRefusal> {
    let source_kind = matches!(source.kind, MemoryKind::Memory | MemoryKind::DerivedFact);
    if !source_kind || source.status != MemoryStatus::Promoted || source.in_vocabulary {
        return Err(ClassificationRefusal::Validation {
            field: Endpoint::From,
        });
    }
    if target.kind != MemoryKind::Term || target.status != MemoryStatus::Promoted {
        return Err(ClassificationRefusal::Validation {
            field: Endpoint::To,
        });
    }
    check_term_active(tip)?;
    check_live_classification(source.artifact_id, &tip.curie, cited, edges)?;
    Ok(tip.curie.clone())
}

/// The term-tip check a classification meets at proposal and again at
/// ratification: the served tip is active, else `vocab.inactive_curie`
/// with its successors (ADR-0027 d6).
pub fn check_term_active(tip: &TermTip) -> Result<(), ClassificationRefusal> {
    match &tip.state {
        TipState::Active => Ok(()),
        TipState::Deprecated(successors) => Err(ClassificationRefusal::InactiveCurie {
            curie: tip.curie.clone(),
            successors: successors.clone(),
        }),
    }
}

/// The duplicate rule: a memory version has at most one live
/// classification per CURIE. A pending or ratified `oml:about` edge joining
/// `version` and `curie`, or a citation of `curie` in the version's own
/// `cited` terms, refuses another; a rejected edge frees the pair
/// (ADR-0027 d7, d8).
pub fn check_live_classification(
    version: Uuid,
    curie: &Curie,
    cited: &[Curie],
    edges: &[ClaimEdge],
) -> Result<(), ClassificationRefusal> {
    if let Some(live) = edges.iter().find(|e| e.classifies(version, curie)) {
        return Err(ClassificationRefusal::ClassificationExists(
            ExistingClassification::Edge {
                edge_id: live.edge_id,
                state: live.state,
            },
        ));
    }
    if cited.contains(curie) {
        return Err(ClassificationRefusal::ClassificationExists(
            ExistingClassification::Terms,
        ));
    }
    Ok(())
}

/// A term edge by its endpoints' CURIEs, as the cycle invariant and the
/// per-release closure see it: `from` `label` `to` (ADR-0021 d4, d5).
// reading: an edge binds versions, but hierarchy and replacement are
// relations between terms, as the CURIE-keyed closure table records them,
// so a cycle is one over CURIEs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermEdge {
    pub label: OmlLabel,
    pub from: Curie,
    pub to: Curie,
}

/// `vocab.cycle_refused`: ratifying the candidate would close a cycle over
/// the ratified edges of its label (ADR-0021 d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("ratifying this {} edge would close a cycle (vocab.cycle_refused)", label.as_str())]
pub struct CycleRefused {
    pub label: OmlLabel,
}

/// The cycle invariant at ratification, a deterministic transaction-time
/// check (ADR-0021 d5): a candidate `is_a` edge is refused when it would
/// close a cycle over the ratified `is_a` edges plus itself, and a candidate
/// `replaced_by` edge when it would close one over the ratified
/// `replaced_by` edges plus itself. A self-edge is such a cycle. Every other
/// label carries no cycle invariant. `ratified` may hold edges of any label;
/// only the candidate's own label counts.
// reading: `is_a` and `replaced_by` are two invariants, each over its own
// relation; a loop that mixes the labels closes neither.
pub fn check_acyclic(ratified: &[TermEdge], candidate: &TermEdge) -> Result<(), CycleRefused> {
    let label = candidate.label;
    if !matches!(label, OmlLabel::IsA | OmlLabel::ReplacedBy) {
        return Ok(());
    }
    let refused = Err(CycleRefused { label });
    if candidate.from == candidate.to {
        return refused;
    }
    // The candidate closes a cycle iff its source is reachable from its
    // target over the ratified edges of its label.
    let mut seen: BTreeSet<&Curie> = BTreeSet::from([&candidate.to]);
    let mut frontier = vec![&candidate.to];
    while let Some(node) = frontier.pop() {
        for edge in ratified
            .iter()
            .filter(|e| e.label == label && &e.from == node)
        {
            if edge.to == candidate.from {
                return refused;
            }
            if seen.insert(&edge.to) {
                frontier.push(&edge.to);
            }
        }
    }
    Ok(())
}

/// One declared `derives_from` edge: `child` was built from `parent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivesFrom {
    pub child: Uuid,
    pub parent: Uuid,
}

/// The artifacts in `row`'s lineage closure, `row` included: the transitive
/// closure over `derives_from` and claim edges, pending and ratified alike,
/// with every `oml:about` edge left out — a classification says what a
/// memory is about, not what it was built from (audit-and-lineage.md, the
/// derivation graph; ADR-0001 d2; ADR-0027 d9). The caller resolves the
/// artifacts' authors into a [`crate::model::LineageClosure`].
// reading: `derives_from` is followed from a child to its parents only, as
// ADR-0001 d2's "declared ancestor" says; a claim edge is a connection and
// is followed from either endpoint, the reading ADR-0027 d9's two-sided
// consequence takes (neither endpoint's authors leave the other's closure
// once `oml:about` is excluded). A rejected edge confers nothing and is
// left out too.
#[must_use]
pub fn lineage_artifacts(
    row: Uuid,
    derives_from: &[DerivesFrom],
    claim_edges: &[ClaimEdge],
) -> BTreeSet<Uuid> {
    let mut closure = BTreeSet::from([row]);
    let mut frontier = vec![row];
    while let Some(node) = frontier.pop() {
        let parents = derives_from
            .iter()
            .filter(|d| d.child == node)
            .map(|d| d.parent);
        let connected = claim_edges
            .iter()
            .filter(|e| !e.is_classification() && e.state != EdgeState::Rejected)
            .filter_map(|e| {
                if e.from == node {
                    Some(e.to)
                } else if e.to == node {
                    Some(e.from)
                } else {
                    None
                }
            });
        for next in parents.chain(connected) {
            if closure.insert(next) {
                frontier.push(next);
            }
        }
    }
    closure
}

/// Whether a classification is retired: a ratified `oml:about` edge whose
/// term's served tip is deprecated, read from the tip's `term_status` at
/// each read and never written to the edge, the memory or the journal
/// (ADR-0027 d10).
#[must_use]
pub fn is_retired(edge: &ClaimEdge, term: TermStatus) -> bool {
    edge.is_classification() && edge.state == EdgeState::Ratified && term == TermStatus::Deprecated
}

/// Recall's `expand_terms` (ADR-0021 d9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpandTerms {
    None,
    Descendants,
}

impl ExpandTerms {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ExpandTerms::None => "none",
            ExpandTerms::Descendants => "descendants",
        }
    }

    /// The mode an exact name spells; `None` for any other string.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "none" => Some(ExpandTerms::None),
            "descendants" => Some(ExpandTerms::Descendants),
            _ => None,
        }
    }
}

/// One row of a materialized `is_a` closure, `vocab_closure`: `descendant`
/// lies below `ancestor` (ADR-0021 d4; postgres-schema.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureRow {
    pub ancestor: Curie,
    pub descendant: Curie,
}

/// The recall `terms` filter set after `expand`: under `descendants`,
/// every CURIE of `filter` plus each descendant the materialized closure
/// lists for it — a lookup over the rows, never a walk (ADR-0021 d4, d9).
/// The caller passes the rows of the releases its vocabularies pin.
#[must_use]
pub fn expand_terms(
    filter: &BTreeSet<Curie>,
    expand: ExpandTerms,
    closure: &[ClosureRow],
) -> BTreeSet<Curie> {
    let mut widened = filter.clone();
    if expand == ExpandTerms::Descendants {
        widened.extend(
            closure
                .iter()
                .filter(|row| filter.contains(&row.ancestor))
                .map(|row| row.descendant.clone()),
        );
    }
    widened
}

/// Whether a hit matches the (already expanded) recall `terms` filter: a
/// CURIE the hit's version `cites` in its own `terms`, whatever its term's
/// status, or the `term_curie` of a ratified, unretired classification of
/// that version among `edges`, meets the filter (ADR-0027 d11;
/// retrieval.md). `readable` is the served tip status of every term the
/// calling key can read; a CURIE outside it matches nothing.
#[must_use]
pub fn matches_terms(
    version: Uuid,
    cites: &[Curie],
    edges: &[ClaimEdge],
    filter: &BTreeSet<Curie>,
    readable: &BTreeMap<Curie, TermStatus>,
) -> bool {
    let cited = cites
        .iter()
        .any(|c| filter.contains(c) && readable.contains_key(c));
    cited
        || edges.iter().any(|e| {
            e.is_classification()
                && e.from == version
                && e.state == EdgeState::Ratified
                && e.term_curie.as_ref().is_some_and(|c| {
                    filter.contains(c)
                        && readable
                            .get(c)
                            .is_some_and(|&status| !is_retired(e, status))
                })
        })
}
