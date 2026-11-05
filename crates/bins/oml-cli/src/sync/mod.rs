//! `oml sync` (ADR-0057): source adapters behind a port, the diff of a
//! source against a workarea, and the run that writes, checks or links it.

pub mod adapter;
pub mod adr;
pub mod api;
pub mod doctor;
pub mod hash;
pub mod headings;
pub mod lines;
pub mod markdown;
pub mod plan;
pub mod run;
pub mod source;
