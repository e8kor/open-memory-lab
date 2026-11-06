//! The recall telemetry's writer, labeller and partitions (ADR-0054 d2,
//! d3, d5, d6, as accepted; migration 0023).
//!
//! A recall hands its event to a bounded queue and never waits: a task
//! writes the queue in batches, at most [`BATCH`] items or every second,
//! each tenant's items in a transaction of their own under that tenant's
//! row security, never inside a recall's snapshot. A full queue, a store
//! error or a tenant with no partition drops the event and counts it; the
//! recall's answer is the same. The same task attaches labels by SQL and,
//! every minute, labels `no_use`, purges what is past the 30 days and
//! refreshes the health gauges doctor reads.

