//! What the lead's pages (the connect page, the sign-in page and the pages
//! not yet moved) and `oml-web`'s pages share (ADR-0070 d1): escaping, the
//! compile-time digest a CSP names, the same-origin rule, and in [`web`] the
//! design system's stylesheet, marks and page chrome (ADR-0069).
//!
//! This crate holds no I/O: no socket, no store, no app, no domain. Anything
//! that resolves a session must belong elsewhere (ADR-0070 d1).

pub mod digest;
pub mod web;

/// How many characters of a client-supplied string a page shows.
pub const CLIENT_STRING_CHARS: usize = 64;

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

/// A client-supplied string as a page shows it: control and bidirectional
/// formatting characters replaced, at most [`CLIENT_STRING_CHARS`]
/// characters with an ellipsis where it was cut, then HTML-escaped.
#[must_use]
pub fn client_string(value: &str) -> String {
    let cleaned: Vec<char> = value
        .chars()
        .map(|c| {
            if c.is_control() || is_bidi_format(c) {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect();
    let shown: String = if cleaned.len() > CLIENT_STRING_CHARS {
        cleaned[..CLIENT_STRING_CHARS - 1]
            .iter()
            .chain(std::iter::once(&'…'))
            .collect()
    } else {
        cleaned.into_iter().collect()
    };
    escape(&shown)
}

/// The Unicode bidirectional formatting characters (UAX #9 §2), which
/// reorder how the text around them displays.
#[must_use]
pub fn is_bidi_format(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Whether a state-changing request came from the page's own origin
/// (ADR-0045 d8; ADR-0031 d9), read through `values`, which answers every
/// value a request carries for a lower-case header name. A browser that
/// sends `Sec-Fetch-Site` is judged by it alone, and it must be the one
/// value `same-origin`; any other request must carry exactly one `Origin`,
/// equal to `origin`. The lead's pages and `oml-web` call this one function,
/// so the two processes admit the same requests (#230).
#[must_use]
pub fn same_origin<'a>(values: impl Fn(&str) -> Vec<&'a [u8]>, origin: &str) -> bool {
    let single = |name: &str| match values(name).as_slice() {
        [value] => Some(*value),
        _ => None,
    };
    if values("sec-fetch-site").is_empty() {
        single("origin") == Some(origin.as_bytes())
    } else {
        single("sec-fetch-site") == Some(b"same-origin".as_slice())
    }
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
    fn same_origin_reads_sec_fetch_site_first_and_else_one_matching_origin() {
        let origin = "https://memory.example";
        let with = |pairs: &'static [(&'static str, &'static str)]| {
            same_origin(
                |name: &str| {
                    pairs
                        .iter()
                        .filter(|(n, _)| *n == name)
                        .map(|(_, v)| v.as_bytes())
                        .collect()
                },
                origin,
            )
        };
        assert!(with(&[("sec-fetch-site", "same-origin")]));
        assert!(!with(&[
            ("sec-fetch-site", "cross-site"),
            ("origin", "https://memory.example")
        ]));
        assert!(!with(&[
            ("sec-fetch-site", "same-origin"),
            ("sec-fetch-site", "same-origin")
        ]));
        assert!(with(&[("origin", "https://memory.example")]));
        assert!(!with(&[("origin", "https://evil.example")]));
        assert!(!with(&[
            ("origin", "https://memory.example"),
            ("origin", "https://memory.example")
        ]));
        assert!(!with(&[]));
    }

    #[test]
    fn a_client_string_is_cleaned_cut_and_escaped() {
        assert_eq!(client_string("a\u{202E}b<"), "a\u{FFFD}b&lt;");
        let long = "x".repeat(CLIENT_STRING_CHARS + 5);
        assert_eq!(client_string(&long).chars().count(), CLIENT_STRING_CHARS);
        assert!(client_string(&long).ends_with('…'));
    }

    #[test]
    fn escape_elements_are_replaced() {
        assert_eq!(escape("<a>&b'\""), "&lt;a&gt;&amp;b&#39;&quot;");
    }
}
