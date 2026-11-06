//! In-memory doubles for every port: the contract suites in
//! `oml-port-contracts` run against these first, and a real adapter passes
//! the identical suite (workspace-plan.md M1 build order, step 2).
//!
//! The doubles are deterministic — fixed clock, counting id generator — so
//! every test chain is reproducible byte for byte.

