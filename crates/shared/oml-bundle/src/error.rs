//! The named refusals of ADR-0084 d5, d7 and d8, each with its code.
//!
//! A message names the file, line or field that failed and never quotes
//! content, a key or an identity: a refusal is printed and counted
//! (`oml_import_refusals_total{code}`, d15), never a place for a secret.

/// Why a bundle, or an import plan over it, was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    /// The manifest names a format this reader does not know (d1).
    #[error("bundle.format_unknown: format {found:?} is not oml-bundle/1")]
    FormatUnknown { found: String },
    /// An entry outside the fixed layout, out of order, or missing (d1, d7).
    #[error("bundle.layout: {detail}")]
    Layout { detail: String },
    /// A path with `..`, an absolute path, a symlink or a hard link (d7).
    #[error("bundle.path_unsafe: entry {path:?}: {detail}")]
    PathUnsafe { path: String, detail: String },
    /// Size or entry count over its limit (d7).
    #[error("bundle.too_large: {detail}")]
    TooLarge { detail: String },
    /// A line over 2 MiB (d7).
    #[error("bundle.line_too_long: {file} line {line}")]
    LineTooLong { file: String, line: usize },
    /// JSON nested deeper than 32 (d7).
    #[error("bundle.depth: {file} line {line}")]
    Depth { file: String, line: usize },
    /// A duplicate artifact id and version, or a duplicate body (d7).
    #[error("bundle.duplicate: {detail}")]
    Duplicate { detail: String },
    /// A body no line references (d7).
    #[error("bundle.unreferenced_body: {path}")]
    UnreferencedBody { path: String },
    /// A line that does not parse or breaks the closed schema (reading R6,
    /// ADR-0084 note of 2026-11-17).
    #[error("bundle.malformed: {file} line {line}: {reason}")]
    Malformed {
        file: String,
        line: usize,
        reason: String,
    },
    /// A file, body or version whose digest is not the one stated (d7).
    #[error("bundle.digest_mismatch: {detail}")]
    DigestMismatch { detail: String },
    /// The manifest or a source event does not verify (d5, d7).
    #[error("bundle.signature_invalid: {detail}")]
    SignatureInvalid { detail: String },
    /// The manifest verifies under a key other than the expected one (d5).
    #[error("bundle.signature_unexpected: signed by {found}, expected {expected}")]
    SignatureUnexpected { found: String, expected: String },
    /// No recipient to encrypt to, or input that is not an age file (d5).
    #[error("bundle.unencrypted: {detail}")]
    Unencrypted { detail: String },
    /// A recipient that does not parse, or an age file the identity does not
    /// open (reading of 2026-11-17). Never names the identity.
    #[error("bundle.decrypt_failed: {detail}")]
    DecryptFailed { detail: String },
    /// A bundle older than the target's ledger, without `allow_older` (d8).
    #[error("import.bundle_older: exported {exported_at}, ledger holds {ledger_at}")]
    BundleOlder {
        exported_at: String,
        ledger_at: String,
    },
    /// An incremental bundle whose base is not the last release imported (d8).
    #[error("import.since_mismatch: {detail}")]
    SinceMismatch { detail: String },
    /// An I/O failure of the reader or writer the caller handed in.
    #[error("bundle.io: {detail}")]
    Io { detail: String },
}

impl BundleError {
    /// The stable code, as the API, the CLI, the doctor and the metric name it.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::FormatUnknown { .. } => "bundle.format_unknown",
            Self::Layout { .. } => "bundle.layout",
            Self::PathUnsafe { .. } => "bundle.path_unsafe",
            Self::TooLarge { .. } => "bundle.too_large",
            Self::LineTooLong { .. } => "bundle.line_too_long",
            Self::Depth { .. } => "bundle.depth",
            Self::Duplicate { .. } => "bundle.duplicate",
            Self::UnreferencedBody { .. } => "bundle.unreferenced_body",
            Self::Malformed { .. } => "bundle.malformed",
            Self::DigestMismatch { .. } => "bundle.digest_mismatch",
            Self::SignatureInvalid { .. } => "bundle.signature_invalid",
            Self::SignatureUnexpected { .. } => "bundle.signature_unexpected",
            Self::Unencrypted { .. } => "bundle.unencrypted",
            Self::DecryptFailed { .. } => "bundle.decrypt_failed",
            Self::BundleOlder { .. } => "import.bundle_older",
            Self::SinceMismatch { .. } => "import.since_mismatch",
            Self::Io { .. } => "bundle.io",
        }
    }

    pub(crate) fn layout(detail: impl Into<String>) -> Self {
        Self::Layout {
            detail: detail.into(),
        }
    }

    pub(crate) fn malformed(file: &str, line: usize, reason: impl Into<String>) -> Self {
        Self::Malformed {
            file: file.to_owned(),
            line,
            reason: reason.into(),
        }
    }

    pub(crate) fn digest(detail: impl Into<String>) -> Self {
        Self::DigestMismatch {
            detail: detail.into(),
        }
    }

    pub(crate) fn signature(detail: impl Into<String>) -> Self {
        Self::SignatureInvalid {
            detail: detail.into(),
        }
    }

    pub(crate) fn io(err: &std::io::Error) -> Self {
        Self::Io {
            detail: err.kind().to_string(),
        }
    }
}
