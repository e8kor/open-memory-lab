//! `oml sync --adapter delta` (ADR-0062): a Delta Lake's tables read from
//! their `_delta_log` alone and mapped to anchored claims. Compiled only under
//! the `lake-sync` feature, which the default `oml` never enables.

pub mod extract;
pub mod read;
pub mod run;
pub mod uri;
