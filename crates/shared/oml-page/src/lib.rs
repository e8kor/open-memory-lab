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

/// A request or correlation id the rule admits: one to 128 characters from
/// `A`–`Z`, `a`–`z`, `0`–`9`, `-`, `_`, `.` and `:`, so an honoured id can
/// neither break a JSON line nor forge a second one (ADR-0042 d4). The same
/// rule the API's traffic layer applies, so a page id the web client
/// forwards is one the API honours.
#[must_use]
pub fn request_id_admitted(value: &[u8]) -> bool {
    (1..=128).contains(&value.len())
        && value
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
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
    fn the_request_id_rule_admits_req_ids_and_refuses_spaces_quotes_and_long_values() {
        assert!(request_id_admitted(b"req_0123456789abcdef"));
        assert!(request_id_admitted(b"page-1.2:3"));
        assert!(!request_id_admitted(b""));
        assert!(!request_id_admitted(b"a b"));
        assert!(!request_id_admitted(b"a\"b"));
        assert!(!request_id_admitted(&[b'a'; 129]));
    }

    #[test]
    fn escape_elements_are_replaced() {
        assert_eq!(escape("<a>&b'\""), "&lt;a&gt;&amp;b&#39;&quot;");
    }
}
