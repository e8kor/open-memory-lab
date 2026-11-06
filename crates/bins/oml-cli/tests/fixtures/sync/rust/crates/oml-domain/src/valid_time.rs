//! Valid time, the query's date window and expiry — pure
//! functions over values the app has already read (proposed ADR-0059; it
//! narrows proposed ADR-0036 d2).
//!
//! - **Valid time** (ADR-0059 d2): a version's optional ends `valid_from`
//!   and `valid_until`, each a period (`YYYY`, `YYYY-MM`, `YYYY-MM-DD`) or an
//!   RFC 3339 instant with an offset. A period end names the whole period:
//!   `valid_from` is its first instant, `valid_until` the instant after its
//!   last, so from `2015` until `2015` is the year. The interval is
//!   half-open, an absent end open.
//! - **Expiry** (d3): a version whose end is at or before the request's now.
//! - **The window** (d6, rule [`WHEN_RULE`]): at most one date expression
//!   read from a query by rule, anchored to the request's now.

