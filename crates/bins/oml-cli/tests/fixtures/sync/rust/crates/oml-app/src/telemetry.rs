//! Recall telemetry, the measurement half of ADR-0054 as accepted: what a
//! recall records, the labels of what its reader did next, and the port the
//! lead's writer implements. A recall hands its event to the port without
//! waiting, and the port writes it off the recall's path (d2); nothing here
//! is a claim, and nothing here reaches a ranking (d12).

