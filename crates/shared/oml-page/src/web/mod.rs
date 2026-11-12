//! The web pages' chrome, shared by the lead's remaining pages and
//! `oml-web` (ADR-0070 d1; ADR-0045 d8): no script, one inline stylesheet in
//! the OML brand whose SHA-256 the CSP names, computed when the crate
//! compiles, and every string from a person, an agent, a provider or the
//! store escaped and stripped of control and bidirectional formatting
//! characters.
//!
//! The look is the design system's (ADR-0069): night ground, paper under
//! `prefers-color-scheme: light`, ice links, star for the one primary act,
//! each state as its word, glyph and colour, and the wordmark inline.

#[rustfmt::skip]
mod brand;

use std::fmt::Write as _;

pub use brand::{GLYPHS, GROUND_DARK, GROUND_LIGHT, MARK, WORDMARK};

use crate::digest::{base64_32, sha256};
use crate::{client_string, escape};

/// The pages' one stylesheet, inline (ADR-0069 d3): the design system's
/// tokens, generated into `tokens.css` by `scripts/gen_design_tokens.py`,
/// followed by the hand-written `web.css`, which uses only `var(--…)`, and
/// the Memories list's and the Atlas's own `memories.css`, likewise
/// (ADR-0045 d32).
pub const STYLESHEET: &str = concat!(
    include_str!("tokens.css"),
    include_str!("web.css"),
    include_str!("memories.css")
);

// The stylesheet is long enough that hashing it runs past the compiler's
// const-evaluation lint; the work is bounded by the file's size.
#[allow(long_running_const_eval)]
const STYLE_DIGEST: [u8; 32] = sha256(STYLESHEET.as_bytes());
const STYLE_DIGEST_B64: [u8; 44] = base64_32(&STYLE_DIGEST);

/// The `style-src` hash source of [`STYLESHEET`], `'sha256-…'`.
#[must_use]
pub fn style_source() -> String {
    let digest = std::str::from_utf8(&STYLE_DIGEST_B64).expect("base64 is ASCII");
    format!("'sha256-{digest}'")
}

/// The pages' `Content-Security-Policy`: nothing loads but the one
/// stylesheet, forms post only to the page and to `form_origins` (the
/// sign-in providers, on the lead's sign-in page), a frame loads only from
/// `frame_src` where a page shows a Mermaid claim (ADR-0056 d9), and no
/// frame may hold a page (ADR-0045 d8; ADR-0070 d3 layer 5).
#[must_use]
pub fn content_security_policy(form_origins: &[&str], frame_src: Option<&str>) -> String {
    let mut form_action = String::from("'self'");
    for origin in form_origins {
        form_action.push(' ');
        form_action.push_str(origin);
    }
    let frame = frame_src.map_or_else(String::new, |src| format!(" frame-src {src};"));
    format!(
        "default-src 'none'; style-src {}; form-action {form_action};{frame} \
         frame-ancestors 'none'; base-uri 'none'",
        style_source()
    )
}

/// The user page's Memories list, which `oml-web` serves (ADR-0070 d7 (b)).
pub const USER_MEMORIES_PATH: &str = "/web/memories";

/// The user page's Atlas, which `oml-web` serves (ADR-0070 d7 (b)).
pub const USER_ATLAS_PATH: &str = "/web/atlas";

/// The admin page's Memories list, which `oml-web` serves.
pub const ADMIN_MEMORIES_PATH: &str = "/web/admin/memories";

/// The admin page's Atlas, which `oml-web` serves.
pub const ADMIN_ATLAS_PATH: &str = "/web/admin/atlas";

/// The Atlas tab's link, which is also its active marker.
pub const ATLAS_TAB: &str = USER_ATLAS_PATH;

/// The user page's Workareas view, which `oml-web` serves (ADR-0070 d7 (b)).
pub const USER_WORKAREAS_PATH: &str = "/web/workareas";

/// The admin page's Workareas view, which `oml-web` serves (ADR-0070 d7 (b)).
pub const ADMIN_WORKAREAS_PATH: &str = "/web/admin/workareas";

/// The user page's Review view, review next only; bulk review stays the
/// admin page's (ADR-0055 d1). `oml-web` serves it (ADR-0070 d7 (b)).
pub const USER_REVIEW_PATH: &str = "/web/review";

/// The admin page's Review view, with the tenant's queue above it, which
/// `oml-web` serves (ADR-0070 d7 (b)).
pub const ADMIN_REVIEW_PATH: &str = "/web/admin/review";

/// The user page's Keys view, which `oml-web` serves (ADR-0070 d7 (b)).
pub const USER_KEYS_PATH: &str = "/web/keys";

/// The admin page's Keys view, one person's keys with a cut-off beside
/// each, which `oml-web` serves (ADR-0070 d7 (b)).
pub const ADMIN_KEYS_PATH: &str = "/web/admin/keys";

/// One memory opened by its id, `?id=`, on the user page, which `oml-web`
/// serves (ADR-0070 d7 (b)).
pub const USER_MEMORY_PATH: &str = "/web/memory";

/// One memory with its attribution and the supersede form, `?id=`, on the
/// admin page, which `oml-web` serves.
pub const ADMIN_MEMORY_PATH: &str = "/web/admin/memory";

/// The admin page's Curate view, which `oml-web` serves.
pub const ADMIN_CURATE_PATH: &str = "/web/admin/curate";

/// The admin page's tabs, `(href, text)`; the lead's admin views and
/// `oml-web`'s draw the same nav.
pub const ADMIN_TABS: [(&str, &str); 8] = [
    ("/web/admin/people", "People"),
    ("/web/admin/users", "Quotas"),
    (ADMIN_MEMORIES_PATH, "Memories"),
    (ADMIN_WORKAREAS_PATH, "Workareas"),
    (ADMIN_REVIEW_PATH, "Review"),
    (ADMIN_KEYS_PATH, "Keys"),
    (ADMIN_CURATE_PATH, "Curate"),
    ("/web", "Your page"),
];

/// The user page's tabs, `(href, text)`, with the administration tab for
/// an admin's session; the lead's pages and `oml-web`'s draw the same nav.
#[must_use]
pub fn user_tabs(admin: bool) -> Vec<(&'static str, &'static str)> {
    // The design system's shared user nav (ADR-0069 d3): Start, Memories,
    // Review, Atlas, Keys; then this page's Workareas and Guardrails.
    let mut tabs = vec![
        ("/web/start", "Start"),
        (USER_MEMORIES_PATH, "Memories"),
        (USER_REVIEW_PATH, "Review"),
        (ATLAS_TAB, "Atlas"),
        (USER_KEYS_PATH, "Keys"),
        (USER_WORKAREAS_PATH, "Workareas"),
        ("/web/guardrails", "Guardrails"),
    ];
    if admin {
        tabs.push((ADMIN_TABS[0].0, "Administration"));
    }
    tabs
}

/// The workarea name a person uses, with the tenant root removed and every
/// intermediate parent retained.
#[must_use]
pub fn workarea_label(path: &str) -> &str {
    path.strip_prefix("universe.").unwrap_or(path)
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
#[must_use]
pub fn brand(link: bool) -> String {
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

/// Where the header's sign-out control posts: the lead's `POST /web`, whose
/// one act is `sign_out` (ADR-0070 d6).
const SIGN_OUT_PATH: &str = "/web";

/// The header's sign-out control (ADR-0045 d5, d8; #327): a form that posts
/// `sign_out` with the session's CSRF token to the lead, which ends the
/// session, clears the cookie it set and answers a `303` to the console's
/// address. A `POST` and never a link, so no `GET` signs anyone out.
fn sign_out(csrf: &str) -> String {
    format!(
        r#"<form method="post" action="{SIGN_OUT_PATH}"><input type="hidden" name="action" value="sign_out"><input type="hidden" name="csrf_token" value="{}"><button type="submit" class="linkish">Sign out</button></form>"#,
        escape(csrf)
    )
}

/// A page's chrome: the brand, who is signed in, the sign-out control where
/// `csrf` is a signed-in session's CSRF token, the tabs of `tabs`
/// (`(href, text)`, the active one marked), a notice, and `body`.
#[must_use]
pub fn shell(
    title: &str,
    who: &str,
    csrf: Option<&str>,
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
            r#"<header class="top">{}{nav}<div class="end"><span class="who">{}{}</span>{}</div></header><main class="wrap">{notice}{body}</main>"#,
            brand(true),
            glyph_svg("person"),
            short(who),
            csrf.map_or_else(String::new, sign_out)
        ),
    )
}

/// A refusal answered on a page, never a redirect: its code and message,
/// drawn for no one, so its header carries the brand alone.
#[must_use]
pub fn refusal(code: &str, message: &str) -> String {
    refusal_page(code, message, None, None)
}

/// [`refusal`], naming the request id a person can quote, as `oml-web`'s
/// error pages do (ADR-0070 d3 layer 7), and carrying the header's
/// sign-out control where `csrf` is a session's CSRF token, as [`shell`]
/// draws it: a person a page refuses still ends their session (#327).
#[must_use]
pub fn refusal_page(
    code: &str,
    message: &str,
    request_id: Option<&str>,
    csrf: Option<&str>,
) -> String {
    let request = request_id.map_or_else(String::new, |id| {
        format!(r#"<p class="desc">Request <code>{}</code></p>"#, escape(id))
    });
    let end = csrf.map_or_else(String::new, |csrf| {
        format!(r#"<div class="end">{}</div>"#, sign_out(csrf))
    });
    document(
        "Not available",
        &format!(
            r#"<header class="top">{}{end}</header><main class="wrap"><h1>This cannot continue</h1><p class="error" role="alert">{}: {}</p>{request}<p><a href="/web">Back</a></p></main>"#,
            brand(true),
            escape(code),
            escape(message)
        ),
    )
}

/// A whole page: the head with the one stylesheet, `body`, and the footer.
#[must_use]
pub fn document(title: &str, body: &str) -> String {
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><meta name="color-scheme" content="dark light"><meta name="theme-color" content="{GROUND_DARK}" media="(prefers-color-scheme: dark)"><meta name="theme-color" content="{GROUND_LIGHT}" media="(prefers-color-scheme: light)"><title>Open Memory Lab: {}</title><style>{STYLESHEET}</style></head><body>{body}<footer><div class="wrap">Every act on these pages is journalled under one of your keys.</div></footer></body></html>"##,
        escape(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest as _, Sha256};

    #[test]
    fn the_compile_time_digest_is_sha256_of_the_stylesheet() {
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(STYLESHEET.as_bytes())),
            STYLE_DIGEST
        );
    }

    #[test]
    fn form_action_names_only_the_origins_given() {
        let csp = content_security_policy(&["https://github.com"], None);
        assert!(
            csp.contains("form-action 'self' https://github.com;"),
            "{csp}"
        );
        assert!(!csp.contains("frame-src"), "{csp}");
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(csp.starts_with("default-src 'none'"));
        assert!(csp.contains(&style_source()));
    }

    #[test]
    fn a_page_escapes_what_a_person_or_the_store_supplied() {
        let html = shell(
            "t",
            "<script>",
            Some("\"x"),
            &[("/web", "Keys")],
            "/web",
            None,
            &text("a\u{202E}b <img>"),
        );
        assert!(!html.contains("<script>"));
        assert!(!html.contains('\u{202E}'));
        assert!(!html.contains("<img>"));
        assert!(html.contains(r#"name="csrf_token" value="&quot;x">"#));
        let f = form("/web", "cut_off", "\"x", "", "Cut off", false);
        assert!(f.contains("&quot;x"));
    }

    /// The header carries the sign-out control exactly where a session's
    /// CSRF token is given: a `POST` form to the lead's sign-out, never a
    /// link (#327).
    #[test]
    fn the_header_draws_the_sign_out_control_only_for_a_signed_in_session() {
        let signed_in = shell("t", "alice", Some("c5"), &[], "", None, "");
        assert!(signed_in.contains(
            r#"<form method="post" action="/web"><input type="hidden" name="action" value="sign_out"><input type="hidden" name="csrf_token" value="c5"><button type="submit" class="linkish">Sign out</button></form></div></header>"#
        ), "{signed_in}");
        assert!(!signed_in.contains(r#"href="/web?action=sign_out"#));
        let no_one = shell("t", "", None, &[], "", None, "");
        assert!(!no_one.contains("sign_out"), "{no_one}");
    }

    /// A refusal page carries the same control where a session's CSRF
    /// token is given, and none for no one (#327).
    #[test]
    fn a_refusal_page_draws_the_sign_out_control_only_for_a_signed_in_session() {
        let control = r#"<div class="end"><form method="post" action="/web"><input type="hidden" name="action" value="sign_out"><input type="hidden" name="csrf_token" value="c5"><button type="submit" class="linkish">Sign out</button></form></div></header>"#;
        let signed_in = refusal_page("access.not_found", "not found", Some("req-1"), Some("c5"));
        assert!(signed_in.contains(control), "{signed_in}");
        assert!(
            shell("t", "alice", Some("c5"), &[], "", None, "").contains(&control[17..]),
            "the control is the one shell draws"
        );
        for no_one in [
            refusal("access.not_found", "not found"),
            refusal_page("access.not_found", "not found", Some("req-1"), None),
        ] {
            assert!(!no_one.contains("sign_out"), "{no_one}");
            assert!(!no_one.contains(r#"class="end""#), "{no_one}");
        }
    }
}
