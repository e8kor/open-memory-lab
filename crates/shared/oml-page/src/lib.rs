//! Shared page helpers used by both the transport's connect/help pages and the
//! future `oml-web` client.
//!
//! This crate holds no I/O: no socket, no store, no app, no domain. Anything
//! that resolves a session must belong elsewhere (ADR-0070 d1).

/// HTML escaping for the little trusted text that never comes from memory
/// (labels, fields, quoted paths).
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Current compile-time digests are created from a byte string. The web
/// stylesheet's digest moves here after it is exacted (ADR-0069).
pub fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(input.as_bytes());
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_preserves_clean_text() {
        assert_eq!(escape("ok"), "ok");
    }

    #[test]
    fn escape_elements_are_replaced() {
        assert_eq!(escape("<a>&b'\""), "&lt;a&gt;&amp;b&#39;&quot;");
    }
}
