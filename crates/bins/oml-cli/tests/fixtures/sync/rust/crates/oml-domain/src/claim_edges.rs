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

