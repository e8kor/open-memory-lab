//! The web pages' HTML and the headers every answer carries (ADR-0045 d8):
//! no script, one inline stylesheet in the OML brand whose SHA-256 the CSP
//! names (computed when the crate compiles, as the connect page's is),
//! `default-src 'none'`, `form-action 'self'` plus the configured
//! providers' origins, `frame-ancestors 'none'`, `base-uri 'none'`,
//! `Cache-Control: no-store`, `Referrer-Policy: same-origin` and `nosniff`.
//! Every string from a person, an agent, a provider or the store is
//! escaped and stripped of control and bidirectional formatting characters
//! by the connect page's functions.
//!
//! The look is the design system's (ADR-0069): night ground, paper under
//! `prefers-color-scheme: light`, ice links, star for the one primary act,
//! each state as its word, glyph and colour, and the wordmark inline.

use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

use oml_app::domain::web::Provider;

use super::brand::{GLYPHS, GROUND_DARK, GROUND_LIGHT, MARK, WORDMARK};

use crate::oauth::page::{base64_32, client_string, escape, sha256};

/// The pages' one stylesheet, inline (ADR-0069 d3): the design system's
/// tokens, generated into `tokens.css` by `scripts/gen_design_tokens.py`,
/// followed by the hand-written `web.css`, which uses only `var(--…)`.
pub const STYLESHEET: &str = concat!(include_str!("tokens.css"), include_str!("web.css"));

const STYLE_DIGEST: [u8; 32] = sha256(STYLESHEET.as_bytes());
const STYLE_DIGEST_B64: [u8; 44] = base64_32(&STYLE_DIGEST);

/// The pages' `Content-Security-Policy`: nothing loads but the one
/// stylesheet, forms post only to the page and redirect only to the
/// configured providers, and no frame may hold a page (ADR-0045 d8).
#[must_use]
pub fn content_security_policy(providers: &[Provider]) -> String {
    content_security_policy_framing(providers, None)
}

/// [`content_security_policy`] with the one source a page that shows a
/// Mermaid claim may frame, the render route (ADR-0056 d9): the pages stay
/// without script, and without a `frame-src` no frame loads at all.
#[must_use]
pub fn content_security_policy_framing(providers: &[Provider], frame_src: Option<&str>) -> String {
    let digest = std::str::from_utf8(&STYLE_DIGEST_B64).expect("base64 is ASCII");
    let mut form_action = String::from("'self'");
    for provider in providers {
        form_action.push(' ');
        form_action.push_str(provider.origin());
    }
    let frame = frame_src.map_or_else(String::new, |src| format!(" frame-src {src};"));
    format!(
        "default-src 'none'; style-src 'sha256-{digest}'; form-action {form_action};{frame} \
         frame-ancestors 'none'; base-uri 'none'"
    )
}

/// The headers every web answer carries, a page or a redirect.
pub(crate) fn secure(
    mut response: Response,
    providers: &[Provider],
    frame_src: Option<&str>,
) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&content_security_policy_framing(providers, frame_src))
            .expect("the CSP is ASCII"),
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
pub(crate) fn page_response(
    status: StatusCode,
    html: String,
    providers: &[Provider],
    frame_src: Option<&str>,
) -> Response {
    let mut response = Response::new(Body::from(html));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    secure(response, providers, frame_src)
}

/// A short string from a person, an agent, a provider or the store, as a
/// page shows it: escaped, control and bidirectional characters replaced,
/// cut at the connect page's length.
#[must_use]
pub fn short(value: &str) -> String {
    client_string(value)
}

/// Long text, such as a memory's, escaped whole with its line breaks.
#[must_use]
pub fn text(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            let bidi = matches!(
                c,
                '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
            );
            if (c.is_control() && c != '\n' && c != '\t') || bidi {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect();
    escape(&cleaned)
}

/// An attribute value, escaped.
#[must_use]
pub fn attr(value: &str) -> String {
    escape(value)
}

/// A state's chip (the design system's `StateChip`): the word, its glyph
/// and its colour together. A word that is not a memory's state or a
/// verdict is a quiet chip without a glyph.
#[must_use]
pub fn state_tag(state: &str) -> String {
    let (class, glyph) = match state {
        "held" | "promoted" | "superseded" | "retired" | "rejected" | "unreviewed" | "stale"
        | "self-signed" | "pending" => (state, state),
        "in progress" | "in_progress" => ("in-progress", "in-progress"),
        "approve" | "approved" => ("promoted", "promoted"),
        "reject" => ("rejected", "rejected"),
        _ => ("quiet", ""),
    };
    format!(
        r#"<span class="tag oml-chip is-{class}">{}{}</span>"#,
        glyph_svg(glyph),
        escape(state)
    )
}

/// A glyph by name as inline SVG, or nothing for a name the design system
/// does not draw.
#[must_use]
pub fn glyph_svg(name: &str) -> &'static str {
    GLYPHS
        .iter()
        .find(|(n, _)| *n == name)
        .map_or("", |(_, svg)| svg)
}

/// The header's brand: the wordmark, and the mark alone below 600px.
fn brand(link: bool) -> String {
    if link {
        format!(
            r#"<a class="brand" href="/web" aria-label="Open Memory Lab home">{WORDMARK}{MARK}</a>"#
        )
    } else {
        format!(
            r#"<span class="brand" role="img" aria-label="Open Memory Lab">{WORDMARK}{MARK}</span>"#
        )
    }
}

/// One form posting `action` to `path` with the session's CSRF token, its
/// fields' HTML and a button.
#[must_use]
pub fn form(
    path: &str,
    action: &str,
    csrf: &str,
    fields: &str,
    button: &str,
    primary: bool,
) -> String {
    format!(
        r#"<form class="row" method="post" action="{path}"><input type="hidden" name="action" value="{action}"><input type="hidden" name="csrf_token" value="{}">{fields}<button type="submit"{}>{}</button></form>"#,
        escape(csrf),
        if primary { r#" class="primary""# } else { "" },
        escape(button)
    )
}

/// A labelled text input.
#[must_use]
pub fn input(name: &str, label: &str, value: &str, required: bool) -> String {
    format!(
        r#"<label>{}<input name="{name}" value="{}"{}></label>"#,
        escape(label),
        escape(value),
        if required { " required" } else { "" }
    )
}

/// A labelled text area.
#[must_use]
pub fn textarea(name: &str, label: &str, value: &str) -> String {
    format!(
        r#"<label class="wide">{}<textarea name="{name}" required>{}</textarea></label>"#,
        escape(label),
        text(value)
    )
}

/// A labelled select over `(value, text)` options.
#[must_use]
pub fn select(name: &str, label: &str, options: &[(&str, &str)]) -> String {
    let mut out = format!(r#"<label>{}<select name="{name}">"#, escape(label));
    for (value, shown) in options {
        let _ = write!(
            out,
            r#"<option value="{}">{}</option>"#,
            escape(value),
            escape(shown)
        );
    }
    out.push_str("</select></label>");
    out
}

/// A hidden field.
#[must_use]
pub fn hidden(name: &str, value: &str) -> String {
    format!(
        r#"<input type="hidden" name="{name}" value="{}">"#,
        escape(value)
    )
}

/// A page's chrome: the brand, who is signed in, the tabs of `tabs`
/// (`(href, text)`, the active one marked), a notice, and `body`.
#[must_use]
pub fn shell(
    title: &str,
    who: &str,
    tabs: &[(&str, &str)],
    active: &str,
    notice: Option<&str>,
    body: &str,
) -> String {
    let mut nav = String::from(r#"<nav aria-label="Main">"#);
    for (href, shown) in tabs {
        let on = if *href == active {
            r#" class="on" aria-current="page""#
        } else {
            ""
        };
        let _ = write!(nav, r#"<a href="{href}"{on}>{}</a>"#, escape(shown));
    }
    nav.push_str("</nav>");
    let notice = notice.map_or(String::new(), |n| {
        format!(r#"<p class="notice" role="status">{}</p>"#, escape(n))
    });
    document(
        title,
        &format!(
            r#"<header class="top">{}{nav}<span class="end"><span class="who">{}{}</span></span></header><main class="wrap">{notice}{body}</main>"#,
            brand(true),
            glyph_svg("person"),
            short(who)
        ),
    )
}

/// The signed-out page: one form per configured provider, and a refusal
/// above them when a sign-in or a session was refused.
#[must_use]
pub fn sign_in(providers: &[Provider], refusal: Option<(&str, &str)>) -> String {
    let mut body = String::from(
        r#"<div class="signin"><p class="label">Sign in</p><h1>Your memory, your agents</h1><p class="desc">Use the address your administrator listed. No password is stored.</p>"#,
    );
    if let Some((code, message)) = refusal {
        let _ = write!(
            body,
            r#"<p class="error" role="alert">{}: {}</p>"#,
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
    body.push_str("</div>");
    document(
        "Sign in",
        &format!(
            r#"<header class="top">{}</header><main class="wrap">{body}</main>"#,
            brand(false)
        ),
    )
}

/// A refusal answered on a page, never a redirect: its code and message.
#[must_use]
pub fn refusal(code: &str, message: &str) -> String {
    document(
        "Not available",
        &format!(
            r#"<header class="top">{}</header><main class="wrap"><h1>This cannot continue</h1><p class="error" role="alert">{}: {}</p><p><a href="/web">Back</a></p></main>"#,
            brand(true),
            escape(code),
            escape(message)
        ),
    )
}

fn document(title: &str, body: &str) -> String {
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><meta name="color-scheme" content="dark light"><meta name="theme-color" content="{GROUND_DARK}" media="(prefers-color-scheme: dark)"><meta name="theme-color" content="{GROUND_LIGHT}" media="(prefers-color-scheme: light)"><title>Open Memory Lab: {}</title><style>{STYLESHEET}</style></head><body>{body}<footer><div class="wrap">Every act on these pages is journalled under one of your keys.</div></footer></body></html>"##,
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
        let html = shell(
            "t",
            "<script>",
            &[("/web", "Keys")],
            "/web",
            None,
            &text("a\u{202E}b <img>"),
        );
        assert!(!html.contains("<script>"));
        assert!(!html.contains('\u{202E}'));
        assert!(!html.contains("<img>"));
        let f = form("/web", "sign_out", "\"x", "", "Sign out", false);
        assert!(f.contains("&quot;x"));
    }
}
