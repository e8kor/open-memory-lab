//! The authorization page's HTML and the headers every page carries
//! (docs/api/http-api.md, "The authorization page"; ADR-0031 d9). The page
//! runs no script and sets no cookie; its one stylesheet is inline, and the
//! SHA-256 that the CSP's `style-src` names is computed from it when the
//! crate compiles, so the two cannot drift.
//!
//! The sentences are http-api.md's, word for word. Every string that comes
//! from a client is HTML-escaped and shortened, its control and
//! bidirectional formatting characters replaced, and it is never placed
//! where the page names a host: a document client's host is the server's
//! own reading of the document URL, rendered in its own element after
//! "verified by", so a crafted `client_name` cannot pass for one.

use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

use oml_app::domain::name::Name;
use oml_app::domain::oauth::CLAUDE_CALLBACK;
use oml_app::types::ConfirmationView;

/// The page's one stylesheet, inline; its digest is in the CSP.
pub const STYLESHEET: &str = "body{font-family:system-ui,sans-serif;margin:0;padding:16px;\
color:#1a1a1a;background:#fafafa}main{max-width:32rem;margin:0 auto}h1{font-size:1.4rem}\
.client{font-size:1.1rem;font-weight:600;overflow-wrap:anywhere}.verified,.unverified{margin-top:0}\
.unverified{color:#8a4b00}.error{color:#a00000;font-weight:600}dl{display:grid;\
grid-template-columns:max-content 1fr;gap:4px 12px}dt{font-weight:600}dd{margin:0;\
overflow-wrap:anywhere}label{display:block;margin:12px 0 4px}input[name=connect_code]{font-size:1.2rem;\
padding:8px;width:100%;box-sizing:border-box;letter-spacing:2px}button{font-size:1rem;\
padding:8px 16px;margin:12px 8px 0 0}";

/// How many characters of a client-supplied string the page shows.
pub const CLIENT_STRING_CHARS: usize = 64;

/// The SHA-256 of [`STYLESHEET`], computed at compile time.
const STYLE_DIGEST: [u8; 32] = sha256(STYLESHEET.as_bytes());
/// Its standard base64, the form CSP's hash source takes.
const STYLE_DIGEST_B64: [u8; 44] = base64_32(&STYLE_DIGEST);

/// The page's `Content-Security-Policy`: nothing loads but the one
/// stylesheet, the form posts only to the page itself and the one redirect
/// origin, and no frame may hold it (ADR-0031 d9).
#[must_use]
pub fn content_security_policy() -> String {
    let digest = std::str::from_utf8(&STYLE_DIGEST_B64).expect("base64 is ASCII");
    format!(
        "default-src 'none'; style-src 'sha256-{digest}'; form-action 'self' {}; \
         frame-ancestors 'none'; base-uri 'none'",
        redirect_origin()
    )
}

/// The origin of the allowlist's one redirect, `https://claude.ai`.
fn redirect_origin() -> &'static str {
    let scheme = "https://".len();
    let rest = &CLAUDE_CALLBACK[scheme..];
    &CLAUDE_CALLBACK[..scheme + rest.find('/').unwrap_or(rest.len())]
}

/// The headers every answer of `/oauth/authorize` carries, a page or a
/// redirect: the CSP, `no-store`, `same-origin` referrer policy and
/// `nosniff`. None sets a cookie.
pub(crate) fn secure(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&content_security_policy()).expect("the CSP is ASCII"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.remove(header::SET_COOKIE);
    response
}

/// A page answer: `text/html` under [`secure`]'s headers.
pub(crate) fn page_response(status: StatusCode, html: String) -> Response {
    let mut response = Response::new(Body::from(html));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    secure(response)
}

/// The sentence above the code field (http-api.md, word for word).
pub const ONLY_IF_YOURS: &str = "Enter the code only if you opened this page yourself, by adding \
the connector in Claude, in the last few minutes. If someone sent you this link, close it.";

/// The one answer to every wrong code (http-api.md, word for word).
pub const CODE_NOT_VALID: &str =
    "This code is not valid. Check it, or ask your administrator for a new one.";

/// The connect-code form (ADR-0031 d9 steps 1 and 2): the client, the
/// redirect host and the sealed handle its form carries.
#[derive(Debug, Clone, Copy)]
pub struct CodeForm<'a> {
    pub handle: &'a str,
    /// As the client supplied it.
    pub client_name: &'a str,
    /// A document client's host; `None` for a registered client.
    pub verified_by: Option<&'a str>,
    pub redirect_host: &'a str,
    pub attempts_left: u8,
    /// Whether this form answers a wrong code.
    pub wrong_code: bool,
}

/// The connect-code form's page.
#[must_use]
pub fn code_form(form: &CodeForm<'_>) -> String {
    let mut body = String::from("<h1>Connect a client to Open Memory Lab</h1>");
    let _ = write!(
        body,
        r#"<p class="client">{}</p>"#,
        client_string(form.client_name)
    );
    match form.verified_by {
        Some(host) => {
            let _ = write!(
                body,
                r#"<p class="verified">verified by <b>{}</b></p>"#,
                escape(&host.to_ascii_lowercase())
            );
        }
        None => body.push_str(r#"<p class="unverified">name not verified</p>"#),
    }
    let _ = write!(
        body,
        "<p>Returns to <b>{}</b></p>",
        escape(form.redirect_host)
    );
    if form.wrong_code {
        let _ = write!(
            body,
            r#"<p class="error">oauth.connect_code_invalid: {CODE_NOT_VALID}</p><p>Attempts left: {}.</p>"#,
            form.attempts_left
        );
    }
    let _ = write!(
        body,
        r#"<p>{ONLY_IF_YOURS}</p><form method="post" action="/oauth/authorize"><input type="hidden" name="handle" value="{}"><label for="connect_code">Connect code</label><input id="connect_code" name="connect_code" autocomplete="one-time-code" autocapitalize="characters" spellcheck="false" required autofocus><button type="submit">Continue</button></form>"#,
        escape(form.handle)
    );
    document(&body)
}

/// The confirmation (ADR-0031 d9 step 2): what the connection will act as,
/// one sentence per kind of verb, then Connect and Cancel.
#[must_use]
pub fn confirmation(view: &ConfirmationView) -> String {
    let mut body = String::from("<h1>Connect Claude as this agent?</h1><dl>");
    let key_name = view.key_name.as_ref().map_or("", Name::as_str);
    for (term, value) in [
        ("Tenant", view.tenant_name.as_str()),
        ("Key", key_name),
        ("Principal", view.principal.as_str()),
        ("Role", view.role.as_str()),
    ] {
        let _ = write!(body, "<dt>{term}</dt><dd>{}</dd>", escape(value));
    }
    body.push_str("</dl><h2>Workareas</h2><ul>");
    for grant in &view.grants {
        let _ = write!(
            body,
            "<li>{}: {}</li>",
            escape(grant.workarea.as_str()),
            grant.role.as_str()
        );
    }
    body.push_str("</ul>");
    for (names, sentence) in [
        (
            &view.writes_in,
            "Claude can remember and learn as this agent in",
        ),
        (
            &view.reads_in,
            "Claude can recall and read as this agent in",
        ),
        (
            &view.proposes_in,
            "Claude can propose edits and edges as this agent in",
        ),
    ] {
        if !names.is_empty() {
            let _ = write!(body, "<p>{sentence} {}.</p>", joined(names));
        }
    }
    if !view.writes_in.is_empty() || !view.proposes_in.is_empty() {
        body.push_str(
            "<p>Everything it writes is held until someone other than its author reviews it.</p>",
        );
    }
    for workarea in &view.self_signed_in {
        let _ = write!(
            body,
            "<p>In {}, its writes may be self-signed and readable there without review.</p>",
            escape(workarea.as_str())
        );
    }
    let _ = write!(
        body,
        r#"<p>It cannot approve, demote, delete or administer anything.</p><form method="post" action="/oauth/authorize"><input type="hidden" name="handle" value="{}"><button type="submit" name="action" value="connect">Connect</button><button type="submit" name="action" value="cancel">Cancel</button></form>"#,
        escape(&view.handle)
    );
    document(&body)
}

/// A refusal answered on the page: its code and message, never a redirect.
#[must_use]
pub fn refusal(code: &str, message: &str) -> String {
    let message = message
        .strip_prefix(code)
        .and_then(|m| m.strip_prefix(": "))
        .unwrap_or(message);
    document(&format!(
        r#"<h1>This connection cannot continue</h1><p class="error">{}: {}</p><p>Start again by adding the connector in Claude.</p>"#,
        escape(code),
        escape(message)
    ))
}

fn document(body: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><title>Open Memory Lab: connect</title><style>{STYLESHEET}</style></head><body><main>{body}</main></body></html>"#
    )
}

fn joined(names: &[Name]) -> String {
    names
        .iter()
        .map(|n| escape(n.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A client-supplied string as the page shows it: control and
/// bidirectional formatting characters replaced, at most
/// [`CLIENT_STRING_CHARS`] characters with an ellipsis where it was cut,
/// then HTML-escaped.
pub(crate) fn client_string(value: &str) -> String {
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
fn is_bidi_format(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// HTML-escapes text and attribute values alike.
pub(crate) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

// -- the stylesheet's digest, at compile time ---------------------------------

const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// Byte `i` of `data` after SHA-256 padding to `total` bytes (FIPS 180-4
/// §5.1.1).
const fn padded(data: &[u8], i: usize, total: usize) -> u8 {
    let len = data.len();
    if i < len {
        data[i]
    } else if i == len {
        0x80
    } else if i >= total - 8 {
        let bits = (len as u64) * 8;
        (bits >> (8 * (total - 1 - i))) as u8
    } else {
        0
    }
}

/// SHA-256 (FIPS 180-4 §6.2), as a `const fn` so the stylesheet's digest is
/// a compile-time constant.
pub(crate) const fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let total = (data.len() + 9).div_ceil(64) * 64;
    let mut block = 0;
    while block < total / 64 {
        let mut w = [0_u32; 64];
        let mut t = 0;
        while t < 16 {
            let at = block * 64 + t * 4;
            w[t] = u32::from_be_bytes([
                padded(data, at, total),
                padded(data, at + 1, total),
                padded(data, at + 2, total),
                padded(data, at + 3, total),
            ]);
            t += 1;
        }
        while t < 64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16]
                .wrapping_add(s0)
                .wrapping_add(w[t - 7])
                .wrapping_add(s1);
            t += 1;
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        t = 0;
        while t < 64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[t])
                .wrapping_add(w[t]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
            t += 1;
        }
        let add = [a, b, c, d, e, f, g, hh];
        let mut i = 0;
        while i < 8 {
            h[i] = h[i].wrapping_add(add[i]);
            i += 1;
        }
        block += 1;
    }
    let mut out = [0_u8; 32];
    let mut i = 0;
    while i < 8 {
        let word = h[i].to_be_bytes();
        out[i * 4] = word[0];
        out[i * 4 + 1] = word[1];
        out[i * 4 + 2] = word[2];
        out[i * 4 + 3] = word[3];
        i += 1;
    }
    out
}

/// Standard base64 with padding (RFC 4648 §4) of a 32-byte digest.
pub(crate) const fn base64_32(bytes: &[u8; 32]) -> [u8; 44] {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = [b'='; 44];
    let mut i = 0;
    let mut o = 0;
    while i < 32 {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < 32 { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < 32 { bytes[i + 2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out[o] = ALPHABET[((n >> 18) & 63) as usize];
        out[o + 1] = ALPHABET[((n >> 12) & 63) as usize];
        if i + 1 < 32 {
            out[o + 2] = ALPHABET[((n >> 6) & 63) as usize];
        }
        if i + 2 < 32 {
            out[o + 3] = ALPHABET[(n & 63) as usize];
        }
        i += 3;
        o += 4;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compile_time_digest_is_sha256_of_the_stylesheet() {
        let digest = ring::digest::digest(&ring::digest::SHA256, STYLESHEET.as_bytes());
        assert_eq!(digest.as_ref(), STYLE_DIGEST);
        for (input, expected) in [
            (
                &b""[..],
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                &b"abc"[..],
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
        ] {
            assert_eq!(hex::encode(sha256(input)), expected);
        }
        let long = [b'a'; 1_000];
        assert_eq!(
            sha256(&long).as_slice(),
            ring::digest::digest(&ring::digest::SHA256, &long).as_ref()
        );
    }

    #[test]
    fn the_csp_names_the_redirect_origin() {
        assert_eq!(redirect_origin(), "https://claude.ai");
        // SHA-256 of "" in base64 (RFC 4648 §10 alphabet).
        assert_eq!(
            &base64_32(&sha256(b"")),
            b"47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
    }
}
