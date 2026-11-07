//! `oml-transport-http` — the two-ring HTTP transport (workspace-plan.md:
//! transports depend on `oml-app` only). Paths, field spellings and status
//! codes follow docs/api/openapi.yaml; every refusal is a typed code from
//! docs/api/errors.md, attached here at the edge and nowhere else
//! (errors.md "Implementation mapping").
//!
//! The PoC slice: submit / list / get / recall / supersede on `/v1`; review next,
//! verdicts, key issuance, the key read, global per-key denials, workarea
//! creation, the workarea read, a workarea's crew, the crew records, the
//! events tail and the bootstrap claim on `/v2`; `/healthz` and `/readyz`
//! unversioned and unauthenticated (http-api.md "Health"); `/mcp`, the agent
//! plugin's stateless facade over those rings ([`mcp`]).
//!
//! Every value that names a workarea, crew or key — a header, a path or
//! query parameter, a body field — is classified here, at parse, as an id or
//! a name, and refused when it is neither; the app resolves a name after the
//! verb ceiling (http-api.md "Names and ids"; ADR-0026 d3, d4).

#![forbid(unsafe_code)]

pub mod error;
pub mod mcp;
pub mod oauth;
mod search;
pub mod time;
pub mod traffic;
mod usage;
pub mod web;
