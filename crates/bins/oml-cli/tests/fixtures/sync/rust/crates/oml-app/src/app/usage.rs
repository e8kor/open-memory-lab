//! The measurement half of ADR-0054 as accepted: the `feedback` member a
//! read carries, and a tenant admin's three acts on the recall telemetry:
//! the query-text setting, the event listing and the real-usage replay.
//! No agent, writer, reader or dreamer key reads an event, a label or a
//! count here (d2); every act but feedback is admin by key role.

