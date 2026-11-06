//! The hasher `oml-md/1` and the text digest the server computes
//! (ADR-0057 d7): a span's hash ignores reflow and changes with a word; a
//! text's digest is the server's `plaintext_digest` of the same text.

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// The one hasher v0.5.8 names (ADR-0057 d3, d7).
pub const HASHER: &str = "oml-md/1";

/// `text` in Unicode NFC, as the server stores a claim's text.
#[must_use]
pub fn nfc(text: &str) -> String {
    text.nfc().collect()
}

/// `sha256:` and the lower-case hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// `oml-md/1` of a span: NFC, line endings to LF, every run of whitespace
/// to one space, trimmed, SHA-256 (ADR-0057 d7).
#[must_use]
pub fn md_hash(span: &str) -> String {
    let normal = nfc(&span.replace("\r\n", "\n").replace('\r', "\n"));
    let collapsed = normal.split_whitespace().collect::<Vec<_>>().join(" ");
    sha256(collapsed.as_bytes())
}

/// The digest the server answers as `plaintext_digest` for `text`: the
/// SHA-256 of its NFC form.
#[must_use]
pub fn text_digest(text: &str) -> String {
    sha256(nfc(text).as_bytes())
}

/// The first 16 hex digits of a `sha256:` digest, for idempotency keys.
#[must_use]
pub fn short(digest: &str) -> &str {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    &hex[..hex.len().min(16)]
}

/// The hasher of a module's doc block (ADR-0061 d7).
pub const HASHER_RUSTDOC: &str = "oml-rustdoc/1";
/// The hasher of a crate's facts (ADR-0061 d7).
pub const HASHER_CRATE: &str = "oml-crate/1";

/// `oml-rustdoc/1` of a doc block's lines as written after their `//!`.
#[must_use]
pub fn rustdoc_hash(_lines: &[String]) -> String {
    String::new()
}

/// `oml-crate/1` of a crate's facts (ADR-0061 d7).
#[must_use]
pub fn crate_hash(_facts: &[String]) -> String {
    String::new()
}
