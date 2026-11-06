//! The two pieces of URL grammar the document URL and the issuer share.
//! Each admits a subset of what a WHATWG URL parser leaves unchanged, so
//! that an admitted string is already its own serialization (ADR-0031 d5;
//! `docs/operations/configuration.md`,
//! `oauth.issuer`); the one gap is an `xn--` label, below.

/// A port as written after `:`: one to five ASCII digits, no leading zero,
/// a value from 1 to 65535 (`docs/operations/configuration.md`,
/// `oauth.issuer`).
