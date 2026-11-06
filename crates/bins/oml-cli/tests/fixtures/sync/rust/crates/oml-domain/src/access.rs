//! Grant admissibility, effective access, the refusal order, effective
//! verbs and content-tail visibility — pure functions over journalled grant
//! and denial records, computed on every request with no cache
//! (ADR-0025 d1, d2, d14, d15; ADR-0019 d1) — and the dreamer grant narrowed
//! by `edge_labels`, with the member profile an ontology crew's keys carry
//! (ADR-0027 d3, d4, d5).
//!
//! The tables are declared on `docs/architecture/roles-and-keys.md`
//! (grant admissibility and refusal order, effective verbs, narrowed
//! dreamer grants, member profiles), `docs/architecture/workareas.md`
//! (effective access) and `docs/api/http-api.md` (events tail); this module
//! mirrors them and adds nothing.
//!
//! The ADR-0027 refusals answer these codes of `docs/api/errors.md`:
//!
//! | Refusal | Code |
//! |---|---|
//! | [`EdgeLabelsRefusal`], every variant | `request.validation` |
//! | [`VerbRefusal::VerbCeiling`] | `access.verb_ceiling` |
//! | [`VerbRefusal::GrantRefused`] | `access.grant_refused`, `details` `{workarea_id, needed_role}` plus `edge_labels` where dreamer is narrowed |
//! | [`VerbRefusal::NotFound`] | `access.not_found` |
//! | [`ProfileMismatch`] | `access.profile_mismatch` (403), `details` `{profile, clause, grant_index}` |

