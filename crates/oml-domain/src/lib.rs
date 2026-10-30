//! `oml-domain` — the pure core of Open Memory Lab.
//!
//! Everything in this crate is deterministic and I/O-free: time and identity
//! enter as inputs (workspace-plan.md, clock/id port). It implements the
//! byte-level chain spec of `docs/data/chain-format.md`, the envelope of
//! `docs/data/event-catalog.md`, and the M1 slice of the domain model:
//! roles, memory status transitions, and the constitution's R2 check
//! (ADR-0001 d2). `access` holds grant admissibility, effective access, the
//! refusal order, effective verbs and content-tail visibility (ADR-0025 d1,
//! d2, d14, d15); `budget` holds the budget token, the budget cut and the
//! recall order (ADR-0025 d6, d7, d8). `vocab` holds the CURIE grammar, the
//! prefix registry, `term_status` and the `oml:` register (ADR-0021 d1, d2;
//! ADR-0027 d7); `vocab_edges` holds claim-edge states, classification
//! validation, cycle refusal, the lineage closure without `oml:about`,
//! retirement and the recall terms match (ADR-0021 d5; ADR-0027 d6–d11).

#![forbid(unsafe_code)]

pub mod access;
pub mod budget;
pub mod chain;
pub mod envelope;
pub mod jcs;
pub mod keys;
pub mod model;
pub mod vocab;
pub mod vocab_edges;

pub use chain::{
    ChainError, ChainHead, EventDraft, KeySet, SignedEnvelope, VerifyError, next_envelope, sign,
    verify_chain, verify_event, verify_genesis_reference,
};
pub use envelope::{
    DigestParseError, Envelope, EventTime, Sha256Hash, TimeParseError, event_types,
    sha256_digest_string,
};
pub use jcs::{JcsError, canonical_bytes};
pub use keys::{
    RotationVerifyError, cross_sign_rotation, key_fingerprint, rotation_message,
    verify_rotation_cross_sig,
};
pub use model::{
    HeldBy, LineageClosure, MemoryKind, MemoryStatus, Role, TransitionError, VerdictError,
    check_reviewer, nfc, transition,
};
