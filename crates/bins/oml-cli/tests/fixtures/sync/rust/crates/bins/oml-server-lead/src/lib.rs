//! The lead composition pieces (workspace-plan.md: binaries are thin
//! compositions): configuration (oml.toml + `OML_` environment,
//! configuration.md), the role-split boot (ADR-0034 d4, d5), the kms-file
//! keyring (ADR-0023 d4), the runtime clock/id/scanner adapters, the
//! client document fetcher under its caps (ADR-0031 d5), the
//! startup fork check (ADR-0002 d7), the
//! startup naming and names check (ADR-0026 d11), and the start and state
//! constructors the integration tests share with `main`.

pub mod otlp;
pub mod timed;
