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
pub mod ports;
pub mod types;

pub use app::App;

/// The domain surface, re-exported so adapters depend on `oml-app` only
/// (workspace-plan.md dependency direction).
pub use oml_domain as domain;

/// `ed25519-dalek` is part of the domain's public signature types; adapters
/// reach it here rather than declaring the dependency themselves.
pub use ed25519_dalek;
