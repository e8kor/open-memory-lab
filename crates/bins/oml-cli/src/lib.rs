//! `oml` — the administration surface of Open Memory Lab as a library, so
//! the verifier and client are testable. The CLI is a lab client: it speaks
//! the same HTTP API as every agent, holds no authority of its own, and
//! links neither oml-domain nor oml-app (ADR-0006 d6, ADR-0015,
//! workspace-plan.md: "oml-cli has no inward edge").

pub mod client;
pub mod creds;
pub mod issue;
pub mod verify;
