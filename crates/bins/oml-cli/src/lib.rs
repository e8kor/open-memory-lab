//! `oml` — the administration surface of Open Memory Lab as a library, so
//! the verifier and client are testable. The CLI is a lab client: it speaks
//! the same HTTP API as every agent, holds no authority of its own, and
//! links neither oml-domain nor oml-app (ADR-0006 d6, ADR-0015,
//! workspace-plan.md: "oml-cli has no inward edge"), only the leaf
//! `oml-guardrail`, so its hooks read a guardrail rule as the server does
//! (proposed ADR-0037 d18 Q10).

pub mod chat;
pub mod client;
pub mod creds;
pub mod discovery;
pub mod events;
pub mod guardrails;
pub mod hook;
pub mod issue;
pub mod owner_only;
pub mod quickstart;
pub mod setup;
pub mod sync;
pub mod verify;
