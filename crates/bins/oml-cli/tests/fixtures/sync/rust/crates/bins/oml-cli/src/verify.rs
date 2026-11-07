//! The independent chain verifier behind `oml audit verify-chain`.
//!
//! This module re-implements the verification procedure of
//! docs/data/chain-format.md (steps 1–9) from that PUBLIC page alone —
//! RFC 8785 canonicalization, SHA-256 and pure Ed25519 through sha2 and
//! ed25519-dalek directly, never through oml-domain. That is deliberate
//! (ADR-0006 d6: the CLI composes no domain crate): an auditor's verifier
//! that linked the server's own chain code could inherit the very bug it is
//! supposed to catch.

