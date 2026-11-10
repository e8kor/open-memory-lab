//! `oml-domain` — the pure core of Open Memory Lab.
//!
//! Everything in this crate is deterministic and I/O-free: time and identity
//! enter as inputs (workspace-plan.md, clock/id port). It implements the
//! byte-level chain spec of `docs/data/chain-format.md`, the envelope of
//! `docs/data/event-catalog.md`, and the M1 slice of the domain model:
//! roles, memory status transitions, and the constitution's R2 check
//! (ADR-0001 d2). `access` holds grant admissibility, effective access, the
//! refusal order, effective verbs and content-tail visibility (ADR-0025 d1,
//! d2, d14, d15), with dreamer grants narrowed by `edge_labels` and the
//! member-profile check (ADR-0027 [proposed in #10, unmerged] d3, d4, d5); `budget` holds the budget token, the budget cut and the
//! recall order (ADR-0025 d6, d7, d8); `name` holds the name grammar, the
//! classification of a reference as an id or a name, and the naming of rows
//! that predate names (ADR-0026 d2, d3, d11); `ladder` holds the workarea
//! status ladder, its refusal reasons, the ratifier's independence and the
//! frozen area's closed list of mutations (ADR-0009 d1, d3, d4, d5); `oauth`
//! holds the pure core of connections — the connect code, PKCE, the redirect
//! allowlist, client identity, the fetch-address allow-list, discovery, token
//! formats, rotation with reuse detection and the connection lifecycle
//! (ADR-0031, proposed); `web` holds the web pages' sign-in core — the
//! allowlist entry's email and user, the sealed sign-in state, the provider
//! answers the server admits and the CSRF token (ADR-0045, proposed); `guardrail`
//! holds the guardrail kind's fields and caps, the `oml-gr/1` rule grammar,
//! its tool classes, glob and check, the set's order and conflicts, and the
//! rule cache's codec (ADR-0037 d2, d4, d7, d8, d9, d10).
//! `vocab` holds the CURIE grammar, the prefix registry, `term_status` and
//! the `oml:` register (ADR-0021 d1, d2; ADR-0027 d7); `vocab_edges` holds
//! claim-edge states, classification validation, cycle refusal, the lineage
//! closure without `oml:about`, retirement and the recall terms match
//! (ADR-0021 d5; ADR-0027 d6–d11); `claim_edges` holds the label check at a
//! claim-edge proposal and the ratifier check at its ratification (ADR-0008
//! d5; ADR-0021 d5; ADR-0012). `similar` holds the near-copy check's words,
//! keys, guard and exact relation (ADR-0050 d1, d17); `valid_time` holds a
//! version's declared valid time, expiry and the query's date window under
//! rule `oml-when/1` (ADR-0059).

#![forbid(unsafe_code)]

pub mod access;
pub mod anchor;
pub mod brief;
pub mod budget;
pub mod chain;
pub mod claim_edges;
pub mod envelope;
pub mod guardrail;
pub mod in_progress;
pub mod jcs;
pub mod keys;
pub mod ladder;
pub mod model;
pub mod name;
pub mod oauth;
pub mod origin;
pub mod signup;
pub mod similar;
pub mod valid_time;
pub mod vocab;
pub mod vocab_edges;
pub mod web;

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
pub use ladder::{
    FrozenOp, LadderNode, LadderRefusal, WorkareaStatus, frozen_refuses, propose_check,
    ratifier_is_proposer, ratify_check, same_user, transition_preconditions,
};
pub use model::{
    DERIVED_CLAIM_KEY_PREFIX, HeldBy, KeyCrew, LineageClosure, MemoryKind, MemoryStatus, Role,
    TransitionError, VerdictError, check_reviewer, derive_claim_key, nfc,
    satisfies_crew_inequality, satisfies_manager_inequality, transition,
};
