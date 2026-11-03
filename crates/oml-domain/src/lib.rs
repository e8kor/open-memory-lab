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
//! recall order (ADR-0025 d6, d7, d8); `name` holds the name grammar, the
//! classification of a reference as an id or a name, and the naming of rows
//! that predate names (ADR-0026 d2, d3, d11).

#![forbid(unsafe_code)]

pub mod access;
pub mod budget;
pub mod chain;
pub mod envelope;
pub mod jcs;
pub mod keys;
pub mod model;
pub mod name;

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
    HeldBy, KeyCrew, LineageClosure, MemoryKind, MemoryStatus, Role, TransitionError, VerdictError,
    check_reviewer, nfc, satisfies_crew_inequality, satisfies_manager_inequality, transition,
};
