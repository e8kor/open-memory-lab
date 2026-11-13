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

use oml_page::escape;
pub use oml_page::web::{
    Bar, STYLESHEET, Scope, attr, bare, brand, document, refusal, refusal_page, short,
};

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
    let origins: Vec<&str> = providers.iter().map(|p| p.origin()).collect();
    oml_page::web::content_security_policy(&origins, frame_src)
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

/// The signed-out page: one form per configured provider, and a refusal
/// above them when a sign-in or a session was refused.
#[must_use]
pub fn sign_in(providers: &[Provider], refusal: Option<(&str, &str)>, next: &str) -> String {
    let mut body = String::from(
        r#"<div class="signin"><p class="label">Sign in</p><h1>Open Memory Lab</h1><p class="desc">Sign in with the address an administrator listed. No password is stored.</p>"#,
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
            r#"<form method="post" action="/web/sign-in"><input type="hidden" name="provider" value="{}"><input type="hidden" name="next" value="{}"><button type="submit"{class}>Sign in with {}</button></form>"#,
            provider.as_str(),
            escape(next),
            provider.display()
        );
    }
    body.push_str("</div>");
    document(
        "Sign in",
        &format!(
            r#"<header class="top">{}</header><main class="wrap">{body}</main>"#,
            brand()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
