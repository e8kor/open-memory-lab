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

/// Where a person with no session signs in: the lead's sign-in page
/// (ADR-0070 d4, d6).
pub const SIGN_IN_PATH: &str = "/web/sign-in";

/// Where a page sends a person with no session: the lead's sign-in page,
/// relative, carrying `next`, the page's own path and query, percent-encoded
/// so it is one query value (ADR-0070 d4). Every shape has one public origin,
/// so the relative redirect lands on the lead.
#[must_use]
pub fn sign_in_location(next: &str) -> String {
    let mut out = String::from(SIGN_IN_PATH);
    out.push_str("?next=");
    for b in next.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The longest `next` the lead keeps; a longer one is dropped to `/web`.
pub const NEXT_MAX: usize = 2048;

/// `next` as the lead keeps it, or `/web` (ADR-0070 d4): a same-origin
/// relative path whose path is `/web` or starts with `/web/`, at most
/// [`NEXT_MAX`] bytes of visible ASCII, holding no `//`, no `\`, no `@`
/// and no `:` before its query; `raw` is the value as its query decoded it,
/// and the same rule holds once more after one more percent-decoding, so an
/// encoded `//` or `\` is refused too. Anything else is `/web`.
#[must_use]
pub fn admitted_next(raw: &str) -> String {
    fn holds(value: &str) -> bool {
        let path = value.split('?').next().unwrap_or("");
        (path == "/web" || path.starts_with("/web/"))
            && !value.contains("//")
            && !value.contains('\\')
            && !value.contains('@')
            && !path.contains(':')
    }
    let decoded = percent_decode(raw);
    // The value kept is `raw`, so only it must be visible ASCII; the
    // decoded form may hold a space a query encoded.
    let visible = raw.len() <= NEXT_MAX && raw.bytes().all(|b| b.is_ascii_graphic());
    if visible && holds(raw) && decoded.as_deref().is_some_and(holds) {
        raw.to_owned()
    } else {
        "/web".to_owned()
    }
}

/// `value` with each `%XX` decoded once; `None` when an escape is broken or
/// the bytes are not UTF-8.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = value.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
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
    fn a_next_outside_web_or_naming_another_origin_returns_to_web() {
        for kept in [
            "/web",
            "/web?view=guardrails",
            "/web/admin",
            "/web/start?x=a%20b",
        ] {
            assert_eq!(admitted_next(kept), kept);
        }
        for dropped in [
            "",
            "web",
            "/webfoo",
            "/v2/keys",
            "//evil.example",
            "https://evil.example/web",
            "/web//evil.example",
            "/web/\\evil",
            "/web@evil.example",
            "/web:8080",
            "/web/%2F%2Fevil.example",
            "/web/%5Cevil",
            "/web/%40evil",
            "/web/%zz",
            "/web/ a",
            "/web/\u{e9}",
        ] {
            assert_eq!(admitted_next(dropped), "/web", "{dropped:?}");
        }
        assert_eq!(
            admitted_next(&format!("/web/{}", "a".repeat(NEXT_MAX))),
            "/web"
        );
    }

    #[test]
    fn the_sign_in_location_carries_the_page_as_one_query_value() {
        assert_eq!(
            sign_in_location("/web?view=guardrails&workarea=a b"),
            "/web/sign-in?next=%2Fweb%3Fview%3Dguardrails%26workarea%3Da%20b"
        );
    }

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
