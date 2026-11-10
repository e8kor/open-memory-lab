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
use oml_page::digest::{base64_32, sha256};
use oml_page::{client_string, escape};

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
pub use oml_page::CLIENT_STRING_CHARS;

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

/// Where the code comes from, with a same-origin link to the account page.
pub const WHERE_THE_CODE_COMES_FROM: &str = "Get a connect code from your account page: \
<a href=\"/web\">Connect Claude on the web or a phone</a>. Codes are shown once; use New \
connect code on the key's row if you lost it.";

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
        r#"<p>{ONLY_IF_YOURS}</p><p>{WHERE_THE_CODE_COMES_FROM}</p><form method="post" action="/oauth/authorize"><input type="hidden" name="handle" value="{}"><label for="connect_code">Connect code</label><input id="connect_code" name="connect_code" autocomplete="one-time-code" autocapitalize="characters" spellcheck="false" required autofocus><button type="submit">Continue</button></form>"#,
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
    fn the_code_form_says_where_the_code_comes_from_and_links_the_account_page() {
        let html = code_form(&CodeForm {
            handle: "h",
            client_name: "Claude",
            verified_by: Some("claude.ai"),
            redirect_host: "claude.ai",
            attempts_left: 5,
            wrong_code: false,
        });
        assert!(html.contains(
            "Get a connect code from your account page: \
             <a href=\"/web\">Connect Claude on the web or a phone</a>."
        ));
        assert!(html.contains(
            "Codes are shown once; use New connect code on the key's row if you lost it."
        ));
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
