//! The web pages' HTML and the headers every answer carries (ADR-0045 d8):
//! no script, one inline stylesheet in the OML brand whose SHA-256 the CSP
//! names (computed when the crate compiles, as the connect page's is),
//! `default-src 'none'`, `form-action 'self'` plus the configured
//! providers' origins, `frame-ancestors 'none'`, `base-uri 'none'`,
//! `Cache-Control: no-store`, `Referrer-Policy: same-origin` and `nosniff`.
//! Every string from a person, a provider or the store is escaped and
//! stripped of control and bidirectional formatting characters by the
//! connect page's functions.

use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

use oml_app::domain::web::Provider;

use crate::oauth::page::{base64_32, client_string, escape, sha256};

/// The pages' one stylesheet, inline: the OML brand's night indigo, ice,
/// star and paper, in the system sans and mono faces (BRAND.md; the docs'
/// tokens).
pub const STYLESHEET: &str = ":root{--night:#0A0D22;--ice:#9FC6E6;--star:#F6A83E;\
--paper:#EFE9DA;--dim:#8A90A8;--line:#22284A}*{box-sizing:border-box}body{margin:0;\
background:var(--night);color:var(--paper);font-family:system-ui,-apple-system,\"Segoe UI\",\
Roboto,sans-serif;line-height:1.5}main{max-width:40rem;margin:0 auto;padding:48px 16px}\
.mark{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:.85rem;\
letter-spacing:.12em;text-transform:uppercase;color:var(--ice)}.mark b{color:var(--star)}\
h1{font-size:1.6rem;font-weight:600;margin:.4em 0 .6em}p{margin:.6em 0}.lede{color:var(--dim)}\
code,dd.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}\
form{margin:12px 0}button{font:inherit;font-weight:600;cursor:pointer;border-radius:6px;\
padding:10px 18px;border:1px solid var(--ice);background:transparent;color:var(--ice);\
min-width:16rem}button.primary{background:var(--star);border-color:var(--star);\
color:var(--night)}dl{display:grid;grid-template-columns:max-content 1fr;gap:6px 16px;\
margin:16px 0;padding:16px;border:1px solid var(--line);border-radius:8px}dt{color:var(--dim)}\
dd{margin:0;overflow-wrap:anywhere}.error{color:var(--star);font-weight:600}\
nav{margin-top:32px;padding-top:16px;border-top:1px solid var(--line);display:flex;gap:16px;\
flex-wrap:wrap;align-items:center}a{color:var(--ice)}";

const STYLE_DIGEST: [u8; 32] = sha256(STYLESHEET.as_bytes());
const STYLE_DIGEST_B64: [u8; 44] = base64_32(&STYLE_DIGEST);

/// The pages' `Content-Security-Policy`: nothing loads but the one
/// stylesheet, forms post only to the page and redirect only to the
/// configured providers, and no frame may hold a page (ADR-0045 d8).
#[must_use]
pub fn content_security_policy(providers: &[Provider]) -> String {
    let digest = std::str::from_utf8(&STYLE_DIGEST_B64).expect("base64 is ASCII");
    let mut form_action = String::from("'self'");
    for provider in providers {
        form_action.push(' ');
        form_action.push_str(provider.origin());
    }
    format!(
        "default-src 'none'; style-src 'sha256-{digest}'; form-action {form_action}; \
         frame-ancestors 'none'; base-uri 'none'"
    )
}

/// The headers every web answer carries, a page or a redirect.
pub(crate) fn secure(mut response: Response, providers: &[Provider]) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&content_security_policy(providers)).expect("the CSP is ASCII"),
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
    response
}

/// A page answer: `text/html` under [`secure`]'s headers.
pub(crate) fn page_response(status: StatusCode, html: String, providers: &[Provider]) -> Response {
    let mut response = Response::new(Body::from(html));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    secure(response, providers)
}

/// The signed-out page: one form per configured provider, and a refusal
/// above them when a sign-in or a session was refused.
#[must_use]
pub fn sign_in(providers: &[Provider], refusal: Option<(&str, &str)>) -> String {
    let mut body = String::from(
        "<h1>Sign in</h1><p class=\"lede\">Sign in with the account whose email address your \
         administrator put on the list. Open Memory Lab stores no password and sends no mail.</p>",
    );
    if let Some((code, message)) = refusal {
        let _ = write!(
            body,
            r#"<p class="error">{}: {}</p>"#,
            escape(code),
            escape(message)
        );
    }
    for (index, provider) in providers.iter().enumerate() {
        let class = if index == 0 { " class=\"primary\"" } else { "" };
        let _ = write!(
            body,
            r#"<form method="post" action="/web/sign-in"><input type="hidden" name="provider" value="{}"><button type="submit"{class}>Sign in with {}</button></form>"#,
            provider.as_str(),
            provider.display()
        );
    }
    document("Sign in", &body)
}

/// Who the signed-in person is, as the user page shows them.
#[derive(Debug, Clone, Copy)]
pub struct Person<'a> {
    pub user: &'a str,
    pub organisation: Option<&'a str>,
    pub admin: bool,
    pub expires_at: &'a str,
    pub csrf_token: &'a str,
}

/// The user page while session keys are not built: who the person is, when
/// the session ends, the admin page's link for an admin, and sign-out.
#[must_use]
pub fn user(person: &Person<'_>) -> String {
    let mut body = format!(
        "<h1>Signed in as {}</h1><dl><dt>User</dt><dd class=\"mono\">{}</dd>",
        client_string(person.user),
        client_string(person.user)
    );
    if let Some(organisation) = person.organisation {
        let _ = write!(
            body,
            "<dt>Organisation</dt><dd class=\"mono\">{}</dd>",
            client_string(organisation)
        );
    }
    let _ = write!(
        body,
        "<dt>Administrator</dt><dd>{}</dd><dt>Session ends</dt><dd class=\"mono\">{}</dd></dl>",
        if person.admin { "yes" } else { "no" },
        escape(person.expires_at)
    );
    body.push_str(
        "<p class=\"lede\">Your own keys, connect codes, workareas and review of your agents' \
         work open on this page once this server builds your workarea. Until then an \
         administrator issues your keys.</p>",
    );
    body.push_str("<nav>");
    if person.admin {
        body.push_str("<a href=\"/web/admin\">Administration</a>");
    }
    let _ = write!(
        body,
        r#"<form method="post" action="/web"><input type="hidden" name="action" value="sign_out"><input type="hidden" name="csrf_token" value="{}"><button type="submit">Sign out</button></form></nav>"#,
        escape(person.csrf_token)
    );
    document("Your page", &body)
}

/// The admin page while its tools are not built: the CLI verbs that keep
/// the list, and the way back.
#[must_use]
pub fn admin(person: &Person<'_>) -> String {
    let mut body = format!(
        "<h1>Administration</h1><p class=\"lede\">Signed in as {}.</p>",
        client_string(person.user)
    );
    body.push_str(
        "<p>The list of email addresses that may sign in is kept from the command line until \
         its tools open on this page:</p><dl><dt>Add</dt><dd class=\"mono\">oml web allow \
         &lt;email&gt; --user &lt;user&gt; [--admin]</dd><dt>List</dt><dd class=\"mono\">oml web \
         list</dd><dt>Remove</dt><dd class=\"mono\">oml web remove &lt;email&gt;</dd></dl>",
    );
    let _ = write!(
        body,
        r#"<nav><a href="/web">Your page</a><form method="post" action="/web"><input type="hidden" name="action" value="sign_out"><input type="hidden" name="csrf_token" value="{}"><button type="submit">Sign out</button></form></nav>"#,
        escape(person.csrf_token)
    );
    document("Administration", &body)
}

/// A refusal answered on a page, never a redirect: its code and message.
#[must_use]
pub fn refusal(code: &str, message: &str) -> String {
    document(
        "Not available",
        &format!(
            r#"<h1>This cannot continue</h1><p class="error">{}: {}</p><nav><a href="/web">Back to sign in</a></nav>"#,
            escape(code),
            escape(message)
        ),
    )
}

fn document(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><title>Open Memory Lab: {}</title><style>{STYLESHEET}</style></head><body><main><p class="mark">Open <b>Memory</b> Lab</p>{body}</main></body></html>"#,
        escape(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compile_time_digest_is_sha256_of_the_stylesheet() {
        let digest = ring::digest::digest(&ring::digest::SHA256, STYLESHEET.as_bytes());
        assert_eq!(digest.as_ref(), STYLE_DIGEST);
    }

    #[test]
    fn form_action_names_only_the_configured_providers() {
        let csp = content_security_policy(&[Provider::GitHub]);
        assert!(
            csp.contains("form-action 'self' https://github.com;"),
            "{csp}"
        );
        assert!(!csp.contains("accounts.google.com"), "{csp}");
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(csp.starts_with("default-src 'none'"));
    }

    #[test]
    fn a_page_escapes_what_a_person_or_the_store_supplied() {
        let html = user(&Person {
            user: "<script>",
            organisation: Some("a\u{202E}b"),
            admin: false,
            expires_at: "2026-11-04T00:00:00Z",
            csrf_token: "\"x",
        });
        assert!(!html.contains("<script>"));
        assert!(!html.contains('\u{202E}'));
        assert!(html.contains("&quot;x"));
    }
}
