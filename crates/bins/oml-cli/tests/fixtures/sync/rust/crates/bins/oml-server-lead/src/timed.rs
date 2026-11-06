//! The store port, timed: every call into the Postgres adapter's store
//! ports is one span named by its port method, never by SQL text or bound
//! values, and its wall time and count are summed into the running
//! request's `store_ms` and `store_calls`, pool acquisition and the
//! chain-lock wait included (ADR-0042 d13, d14). Outside a request, the
//! start and the hourly sweep, a call is timed into nothing.
//!
//! Every method forwards to the wrapped store unchanged: the decorator adds
//! an `Instant` and an addition per call and decides nothing.

