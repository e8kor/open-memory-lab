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

