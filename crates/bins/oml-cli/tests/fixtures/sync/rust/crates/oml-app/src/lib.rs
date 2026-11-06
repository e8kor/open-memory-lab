//! `oml-app` — the application layer: the driven port traits
//! (workspace-plan.md port table, ADR-0019 d2) and the use-cases that drive
//! `oml-domain` through them. Every constitution invariant is enforced in
//! this crate or the domain, never in an adapter (ADR-0019 d1).
//!
//! Only this crate depends on `oml-domain`; adapters, transports and test
//! rigs see the domain types through the [`domain`] re-export.

#![forbid(unsafe_code)]

pub mod app;
pub mod cursor;
pub mod doubles;
pub mod oauth_fetch;
pub mod ports;
pub mod search;
pub mod telemetry;
pub mod types;
