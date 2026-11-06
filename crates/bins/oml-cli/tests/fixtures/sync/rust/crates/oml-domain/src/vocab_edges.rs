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

