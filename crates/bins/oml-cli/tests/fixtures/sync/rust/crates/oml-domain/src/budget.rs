//! Recall budgets and the recall order — pure functions over counts and
//! ranks that the app has already fetched (ADR-0025 d6, d7, d8;
//! ADR-0019 d1).
//!
//! The rules are declared on `docs/architecture/retrieval.md`: "Token
//! budgets" (the budget token and the cut) and "The recall-ordering
//! contract" (segment 1's structural order and the hits' total order). This
//! module mirrors them and adds nothing.

