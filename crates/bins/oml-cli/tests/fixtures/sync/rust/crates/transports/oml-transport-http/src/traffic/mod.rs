//! The traffic layer: one tower layer outside every route of
//! [`crate::ApiState::into_router`], the probes, `/mcp` and the OAuth routes
//! included, and the routes no handler finishes. It resolves the request's
//! ids (ADR-0042 d4–d6), installs the task-local [`Scope`] the app's
//! observer and the store port's timing write into (d14), and writes one
//! `http_request` line when the inner service returns the response head
//! (d1–d3, d7, d9).

pub mod ids;
pub mod metrics;
pub mod proxies;
