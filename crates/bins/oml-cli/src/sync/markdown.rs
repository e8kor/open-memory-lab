//! The markdown an adapter reads, as plain text (ADR-0057 d7): the rule
//! lives in the leaf crate `oml-passage`, which the server links too, so a
//! heading's slug and a section's lines are one rule on both (ADR-0081 d2,
//! d3).

pub use oml_passage::markdown::*;
