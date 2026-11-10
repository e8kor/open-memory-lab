//! Claim edges at proposal and at ratification: the label a proposal may
//! carry between two endpoint kinds, and the ratifier that may ratify it
//! (ADR-0008 d5; ADR-0021 d5; ADR-0012; http-api.md, claim edges;
//! errors.md). Edge states, classification and cycle refusal are
//! [`crate::vocab_edges`]'s; this module holds the rules that hold for
//! every label.
//!
//! Each refusal answers exactly one code of `docs/api/errors.md`:
//!
//! | Refusal | Code |
//! |---|---|
//! | [`LabelRefusal`] | `request.validation`, `details.field` [`LabelRefusal::field`] |
//! | [`RatifyRefusal::NotDreamer`] | `access.verb_ceiling` |
//! | [`RatifyRefusal::RatifierIsProposer`] | `review.ratifier_is_proposer` |
//! | [`RatifyRefusal::AuthorInLineage`] | `review.author_in_lineage` |
//!
//! A self-edge is not refused here: for `oml:is_a`
//! [`crate::vocab_edges::check_acyclic`] refuses it at ratification.
// reading: the docs refuse no self-edge at proposal; the cycle invariant of
// ADR-0021 d5 is a ratification check, so a self-edge of another label is
// admitted like any other pair.

use unicode_normalization::is_nfc;
use uuid::Uuid;

use crate::model::{LineageClosure, MemoryKind, Role, check_reviewer};
use crate::vocab::{Curie, OML_PREFIX, OmlLabel};

/// The request field a [`LabelRefusal`] names as `details.field`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeField {
    Label,
    From,
    To,
}

impl EdgeField {
    /// `details.field` of the refusal, as the proposal body spells it
    /// (http-api.md, claim edges).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeField::Label => "label",
            EdgeField::From => "from_artifact_id",
            EdgeField::To => "to_artifact_id",
        }
    }
}

/// A claim-edge label refused at proposal; every variant is
/// `request.validation`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelRefusal {
    /// The label is the empty string.
    #[error("a claim-edge label is non-empty (request.validation)")]
    Empty,
    /// The label is not in Unicode Normalization Form C.
    // reading: the docs fix no label form; a label compared byte for byte
    // must have one spelling, so it is NFC as claim text is (admission.md,
    // capture), and it is refused rather than normalized so the journalled
    // label is the one the proposer sent.
    #[error("a claim-edge label is in NFC (request.validation)")]
    NotNfc,
    /// `oml:about` is the classification relation, and classification
    /// (ADR-0027 [proposed in #10, unmerged]) is not built: refused until its term-tip and duplicate
    /// reads are.
    // reading: ADR-0027 is not accepted on this branch, and
    // `vocab_edges::check_classification` needs reads no caller makes yet;
    // the proposal fails closed until that lane lands.
    #[error("oml:about edges are not built (request.validation)")]
    AboutNotBuilt,
    /// `oml:replaced_by` or `oml:consider` at a standalone proposal: a
    /// successor pointer is declared in the deprecation submission it
    /// belongs to, under that submission's one serve and one verdict, and
    /// is never proposed or ratified apart from it (ADR-0021 d3;
    /// ontologies.md).
    #[error(
        "oml:replaced_by and oml:consider are declared in a deprecation submission, never proposed apart from it (request.validation)"
    )]
    SuccessorOutsideDeprecation,
    /// The label is under the reserved `oml:` prefix but not in its
    /// register (ADR-0021 d2).
    // reading: the docs leave labels open beyond `oml:` but reserve the
    // prefix to the provider; an `oml:` label outside the register would
    // claim a relation the provider never published, so it is refused.
    #[error("{label:?} is not in the oml: register (request.validation)")]
    UnknownOmlLabel { label: String },
    /// Both endpoints are terms and the label is not an `oml:` relation:
    /// term edges carry the closed `oml:` set (ADR-0021 d5).
    #[error("an edge between two terms carries an oml: label (request.validation)")]
    TermEdgeNeedsOmlLabel,
    /// An `oml:` term relation with an endpoint that is not a term
    /// (ADR-0021 d5).
    #[error("{} of an oml: term edge is not a term (request.validation)", field.as_str())]
    EndpointNotTerm { field: EdgeField },
    /// An `oml:` term relation with a term endpoint whose `claim_key` is not
    /// a CURIE: a term's `claim_key` is its CURIE, and the cycle check at
    /// ratification reads the relation over CURIEs (ADR-0021 d2, d5).
    // reading: capture does not hold a term's claim_key to the CURIE
    // grammar, so the proposal refuses the edge rather than admit one that
    // no ratification could check.
    #[error("{} of an oml: term edge has a claim_key that is not a CURIE (request.validation)", field.as_str())]
    EndpointNotCurie { field: EdgeField },
}

impl LabelRefusal {
    /// The error code: always `request.validation`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        "request.validation"
    }

    /// `details.field`: the endpoint for [`LabelRefusal::EndpointNotTerm`]
    /// and [`LabelRefusal::EndpointNotCurie`], the label for every other
    /// refusal.
    #[must_use]
    pub fn field(&self) -> EdgeField {
        match self {
            LabelRefusal::EndpointNotTerm { field } | LabelRefusal::EndpointNotCurie { field } => {
                *field
            }
            _ => EdgeField::Label,
        }
    }
}

/// The label check at proposal, first failure answering: the label's form,
/// then `oml:about`, then the successor labels, then the `oml:` register,
/// then the endpoint kinds. `oml:replaced_by` and `oml:consider` are
/// refused here, as they travel only inside a deprecation (ADR-0021 d3);
/// `oml:is_a` and `oml:part_of` join two terms only; an edge between two
/// terms carries one of them; every other label is open (ADR-0021 d5;
/// http-api.md, claim edges). Authority, R2 and the endpoints' existence
/// are the caller's.
// reading: the docs fix no length cap on a label, so none is checked here.
pub fn check_label(
    label: &str,
    src_kind: MemoryKind,
    dst_kind: MemoryKind,
) -> Result<(), LabelRefusal> {
    if label.is_empty() {
        return Err(LabelRefusal::Empty);
    }
    if !is_nfc(label) {
        return Err(LabelRefusal::NotNfc);
    }
    let both_terms = src_kind == MemoryKind::Term && dst_kind == MemoryKind::Term;
    match OmlLabel::parse(label) {
        Some(OmlLabel::About) => Err(LabelRefusal::AboutNotBuilt),
        Some(OmlLabel::ReplacedBy | OmlLabel::Consider) => {
            Err(LabelRefusal::SuccessorOutsideDeprecation)
        }
        Some(_) => {
            if src_kind != MemoryKind::Term {
                Err(LabelRefusal::EndpointNotTerm {
                    field: EdgeField::From,
                })
            } else if dst_kind != MemoryKind::Term {
                Err(LabelRefusal::EndpointNotTerm {
                    field: EdgeField::To,
                })
            } else {
                Ok(())
            }
        }
        None if is_oml_prefixed(label) => Err(LabelRefusal::UnknownOmlLabel {
            label: label.to_owned(),
        }),
        None if both_terms => Err(LabelRefusal::TermEdgeNeedsOmlLabel),
        None => Ok(()),
    }
}

/// The endpoints' `claim_key`s of an `oml:` term relation, after
/// [`check_label`] admitted the label: each is a CURIE, `from` first
/// (ADR-0021 d2, d5). `oml:about` and every open label pass, as their
/// endpoints need not both be terms.
pub fn check_term_curies(label: &str, src_key: &str, dst_key: &str) -> Result<(), LabelRefusal> {
    if matches!(OmlLabel::parse(label), None | Some(OmlLabel::About)) {
        return Ok(());
    }
    for (key, field) in [(src_key, EdgeField::From), (dst_key, EdgeField::To)] {
        if Curie::parse(key).is_err() {
            return Err(LabelRefusal::EndpointNotCurie { field });
        }
    }
    Ok(())
}

fn is_oml_prefixed(label: &str) -> bool {
    label
        .strip_prefix(OML_PREFIX)
        .is_some_and(|rest| rest.starts_with(':'))
}

/// The key ratifying a claim edge, as the caller read it off the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ratifier<'a> {
    pub role: Role,
    pub principal: &'a str,
    pub key_id: Uuid,
}

/// A refused claim-edge ratification.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RatifyRefusal {
    /// `access.verb_ceiling`: ratification is dreamer(review); no other
    /// role reaches it (errors.md).
    #[error("a {0:?} key cannot ratify a claim edge (access.verb_ceiling)")]
    NotDreamer(Role),
    /// `review.ratifier_is_proposer`: the ratifying key is the proposer's
    /// key, or another key of the proposer's principal (errors.md; ADR-0008
    /// d5).
    #[error("the proposer never ratifies its own edge (review.ratifier_is_proposer)")]
    RatifierIsProposer,
    /// `review.author_in_lineage`: the ratifier's key or principal is in
    /// either endpoint's lineage closure (http-api.md, claim edges;
    /// ADR-0012).
    #[error("the ratifier is in an endpoint's lineage closure (review.author_in_lineage)")]
    AuthorInLineage,
}

impl RatifyRefusal {
    /// The error code of `docs/api/errors.md` this refusal answers.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            RatifyRefusal::NotDreamer(_) => "access.verb_ceiling",
            RatifyRefusal::RatifierIsProposer => "review.ratifier_is_proposer",
            RatifyRefusal::AuthorInLineage => "review.author_in_lineage",
        }
    }
}

/// The ratifier check, first failure answering: the role, then the
/// proposer, then the lineage. `closure` is the union of both endpoints'
/// lineage closures, pending and ratified edges alike, computed by the
/// caller at ratification (http-api.md, claim edges; ADR-0001 d2).
// reading: the docs state the three refusals but not their order; the role
// comes first as the verb ceiling is step 1 of the refusal order
// (roles-and-keys.md), and the proposer check before the lineage check so a
// proposer whose own work is an endpoint is named as the proposer.
pub fn check_ratifier(
    ratifier: &Ratifier<'_>,
    proposer_key_id: Uuid,
    proposer_principal: &str,
    closure: &LineageClosure,
) -> Result<(), RatifyRefusal> {
    if ratifier.role != Role::Dreamer {
        return Err(RatifyRefusal::NotDreamer(ratifier.role));
    }
    if ratifier.key_id == proposer_key_id || ratifier.principal == proposer_principal {
        return Err(RatifyRefusal::RatifierIsProposer);
    }
    check_reviewer(ratifier.role, ratifier.principal, ratifier.key_id, closure)
        .map_err(|_| RatifyRefusal::AuthorInLineage)
}
