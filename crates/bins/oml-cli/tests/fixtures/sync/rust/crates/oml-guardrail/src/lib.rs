//! Guardrail rules — the part of proposed ADR-0037 both the server and the
//! agent plugin's hooks read: the rule grammar `oml-gr/1` and its tool
//! classes (d8), the severity and effect it names (d2, d8), the linear-time
//! glob (d8), the check before a tool call (d8, d9) and the rule cache's
//! codec (d9), with the design constants they share (d7, d9).
//!
//! A leaf crate: it links no workspace crate, so `oml-domain`, which
//! re-exports it, and `oml-cli`, whose hooks run the check, cannot disagree
//! on what a rule means (d18 Q10, amending ADR-0006 d6 by this one leaf).
//! Serving, admission, the hooks' I/O and the clock are not here.

#![forbid(unsafe_code)]

mod argument;
mod cache;
mod canonical;
mod check;
mod frame;
mod glob;
mod rule;
mod shell;
