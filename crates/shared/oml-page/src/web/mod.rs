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

/// The Start page, the Personal side's first page (ADR-0068 d10).
pub const START_PATH: &str = "/web/start";

/// The People page, the Administration side's first page (ADR-0045 d13).
pub const ADMIN_PEOPLE_PATH: &str = "/web/admin/people";

/// The Personal side's nav, `(href, word)`, in the order the design
/// system's BrandHeader draws it; the lead's pages and `oml-web`'s draw the
/// same nav. No tab leads to the other side: that is the switch.
pub const PERSONAL_TABS: [(&str, &str); 7] = [
    (START_PATH, "Start"),
    (USER_MEMORIES_PATH, "Memories"),
    (USER_REVIEW_PATH, "Review"),
    (USER_ATLAS_PATH, "Atlas"),
    (USER_KEYS_PATH, "Keys"),
    (USER_WORKAREAS_PATH, "Workareas"),
    ("/web/guardrails", "Guardrails"),
];

/// The Administration side's nav, `(href, word)`, in the Personal side's
/// order where the two share a page.
pub const ADMIN_TABS: [(&str, &str); 8] = [
    (ADMIN_PEOPLE_PATH, "People"),
    ("/web/admin/users", "Quotas"),
    (ADMIN_MEMORIES_PATH, "Memories"),
    (ADMIN_REVIEW_PATH, "Review"),
    (ADMIN_ATLAS_PATH, "Atlas"),
    (ADMIN_KEYS_PATH, "Keys"),
    (ADMIN_WORKAREAS_PATH, "Workareas"),
    (ADMIN_CURATE_PATH, "Curate"),
];

/// The pages below a tab, each with the tab it sits under: a refusal on
/// one leads back to that tab.
const BELOW: [(&str, &str); 3] = [
    (USER_MEMORY_PATH, USER_MEMORIES_PATH),
    (ADMIN_MEMORY_PATH, ADMIN_MEMORIES_PATH),
    ("/web/setup", START_PATH),
];

/// Which side of the console a page is on (the design system's
/// BrandHeader, variant A): the person's own pages or the tenant's. The
/// bar, its scope word and glyph, the nav and the switch follow from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The person's own pages; `admin` is whether the session holds the
    /// admin capacity, which alone draws the switch to Administration.
    Personal { admin: bool },
    /// The tenant's pages, drawn only for a session with the admin
    /// capacity.
    Administration,
}

impl Scope {
    /// The side a page at `path` is drawn on: Administration for an admin
    /// page and a session holding the admin capacity, else Personal, so a
    /// person without it is never shown the other side (ADR-0045 d10).
    #[must_use]
    pub fn of_path(path: &str, admin: bool) -> Self {
        if admin && (path == "/web/admin" || path.starts_with("/web/admin/")) {
            Self::Administration
        } else {
            Self::Personal { admin }
        }
    }

    /// The scope word, which also labels the side's nav.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Personal { .. } => "Personal",
            Self::Administration => "Administration",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Personal { .. } => "personal",
            Self::Administration => "admin",
        }
    }

    /// The side's nav, `(href, word)`.
    #[must_use]
    pub fn tabs(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Personal { .. } => &PERSONAL_TABS,
            Self::Administration => &ADMIN_TABS,
        }
    }

    /// The side's first page, where its brand leads.
    #[must_use]
    pub fn first(self) -> (&'static str, &'static str) {
        self.tabs()[0]
    }

    /// The tab whose page is `path`, or under which it sits.
    fn tab_of(self, path: &str) -> Option<(&'static str, &'static str)> {
        let path = BELOW
            .iter()
            .find(|(below, _)| *below == path)
            .map_or(path, |(_, tab)| *tab);
        self.tabs().iter().find(|(href, _)| *href == path).copied()
    }

    /// The bar's class.
    fn class(self) -> &'static str {
        match self {
            Self::Personal { .. } => "personal",
            Self::Administration => "admin",
        }
    }

    /// The one link to the other side, at the bar's fixed place: to
    /// Administration only for a session that holds it, back to Personal
    /// from Administration always.
    fn switch(self) -> String {
        let to = match self {
            Self::Personal { admin: false } => return String::new(),
            Self::Personal { admin: true } => Self::Administration,
            Self::Administration => Self::Personal { admin: true },
        };
        format!(
            r#"<a class="switch" href="{}">{}{}</a>"#,
            to.first().0,
            glyph_svg(to.glyph()),
            to.word()
        )
    }
}

/// What the bar is drawn for: the side, who is signed in (the person and
/// their organisation, as the session names them) and the session's CSRF
/// token, which the sign-out control posts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub scope: Scope,
    pub who: String,
    pub csrf: String,
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

/// The header's brand for a page drawn for no one: the wordmark, and the
/// mark alone below 600px.
#[must_use]
pub fn brand() -> String {
    format!(
        r#"<span class="brand" role="img" aria-label="Open Memory Lab">{WORDMARK}{MARK}</span>"#
    )
}

/// The brand on a side's bar: a link to the side's first page, never to
/// `/web`.
fn brand_of(scope: Scope) -> String {
    let (href, word) = scope.first();
    format!(
        r#"<a class="brand" href="{href}" aria-label="Open Memory Lab: {}, {word}">{WORDMARK}{MARK}</a>"#,
        scope.word()
    )
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

/// The bar of `bar`'s side (the design system's BrandHeader, variant A),
/// with `active` the current tab: the brand, the scope word and its glyph,
/// the side's nav labelled with that word, the switch where the session
/// holds the other side, who is signed in and the sign-out control.
fn header(bar: &Bar, active: &str) -> String {
    let scope = bar.scope;
    let mut nav = format!(r#"<nav aria-label="{}">"#, scope.word());
    for (href, shown) in scope.tabs() {
        let on = if *href == active {
            r#" class="on" aria-current="page""#
        } else {
            ""
        };
        let _ = write!(nav, r#"<a href="{href}"{on}>{shown}</a>"#);
    }
    nav.push_str("</nav>");
    let who = if bar.who.is_empty() {
        String::new()
    } else {
        format!(
            r#"<span class="who">{}<span class="name">{}</span></span>"#,
            glyph_svg("person"),
            short(&bar.who)
        )
    };
    format!(
        r#"<header class="top {}">{}<span class="scope">{}{}</span>{nav}{}<div class="end">{who}{}</div></header>"#,
        scope.class(),
        brand_of(scope),
        glyph_svg(scope.glyph()),
        scope.word(),
        scope.switch(),
        sign_out(&bar.csrf)
    )
}

/// The footer of a side's page: the scope word again, and whose keys
/// journal what the pages do.
fn footer(scope: Scope) -> String {
    format!(
        r#"<span class="scope">{}{}</span> {JOURNALLED}"#,
        glyph_svg(scope.glyph()),
        scope.word()
    )
}

/// The footer's sentence.
const JOURNALLED: &str = "Every act on these pages is journalled under one of this account's keys.";

/// A page's chrome on `bar`'s side: the bar with `active` the current tab,
/// a notice, `body`, and the side's footer. The `<title>` is the tab's word,
/// after `title` where the page is one below its tab (`One memory`), and
/// after `Admin` on Administration: `Admin · One memory · Memories · OML`.
/// A tab's own page passes its word or nothing as `title`.
#[must_use]
pub fn shell(title: &str, bar: &Bar, active: &str, notice: Option<&str>, body: &str) -> String {
    let tab = bar
        .scope
        .tabs()
        .iter()
        .find(|(href, _)| *href == active)
        .map(|(_, word)| *word);
    let mut name = match tab {
        Some(tab) if title.is_empty() || title == tab => tab.to_owned(),
        Some(tab) => format!("{title} · {tab}"),
        None => title.to_owned(),
    };
    if bar.scope == Scope::Administration {
        name = format!("Admin · {name}");
    }
    let notice = notice.map_or(String::new(), |n| {
        format!(r#"<p class="notice" role="status">{}</p>"#, escape(n))
    });
    page(
        &name,
        &format!(
            r#"{}<main class="wrap">{notice}{body}</main>"#,
            header(bar, active)
        ),
        &footer(bar.scope),
    )
}

/// A page drawn for no one, such as the sign-up terms: the brand alone in
/// the header.
#[must_use]
pub fn bare(title: &str, body: &str) -> String {
    document(
        title,
        &format!(
            r#"<header class="top">{}</header><main class="wrap">{body}</main>"#,
            brand()
        ),
    )
}

/// A refusal answered on a page, never a redirect: its code and message,
/// drawn for no one, so its header carries the brand alone.
#[must_use]
pub fn refusal(code: &str, message: &str) -> String {
    refusal_page(code, message, None, None, "/web")
}

/// [`refusal`], naming the request id a person can quote, as `oml-web`'s
/// error pages do (ADR-0070 d3 layer 7). Where `bar` names a session the
/// page is drawn inside that side's bar, with its sign-out control (#327)
/// and one link back named by the tab of `path`, the page that refused, or
/// by the side's first page where `path` is under no tab of the side; for
/// no one the header carries the brand alone and the link goes to `/web`.
#[must_use]
pub fn refusal_page(
    code: &str,
    message: &str,
    request_id: Option<&str>,
    bar: Option<&Bar>,
    path: &str,
) -> String {
    let request = request_id.map_or_else(String::new, |id| {
        format!(r#"<p class="desc">Request <code>{}</code></p>"#, escape(id))
    });
    let back = bar.map_or_else(
        || r#"<a href="/web">Back</a>"#.to_owned(),
        |bar| {
            let (href, word) = bar.scope.tab_of(path).unwrap_or(bar.scope.first());
            format!(r#"<a href="{href}">Back to {word}</a>"#)
        },
    );
    let main = format!(
        r#"<main class="wrap"><div class="refusal"><h1>This cannot continue</h1><p class="error" role="alert"><code>{}</code>: {}</p>{request}<p>{back}</p></div></main>"#,
        escape(code),
        escape(message)
    );
    match bar {
        Some(bar) => {
            let title = match bar.scope {
                Scope::Administration => "Admin · Not available",
                Scope::Personal { .. } => "Not available",
            };
            page(
                title,
                &format!("{}{main}", header(bar, "")),
                &footer(bar.scope),
            )
        }
        None => document(
            "Not available",
            &format!(r#"<header class="top">{}</header>{main}"#, brand()),
        ),
    }
}

/// A whole page drawn for no one: the head with the one stylesheet,
/// `body`, and the footer.
#[must_use]
pub fn document(title: &str, body: &str) -> String {
    page(title, body, JOURNALLED)
}

/// A whole page: the head with the one stylesheet and the `<title>`
/// `<title> · OML`, `body`, and `footer`'s HTML.
fn page(title: &str, body: &str, footer: &str) -> String {
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><meta name="color-scheme" content="dark light"><meta name="theme-color" content="{GROUND_DARK}" media="(prefers-color-scheme: dark)"><meta name="theme-color" content="{GROUND_LIGHT}" media="(prefers-color-scheme: light)"><title>{} · OML</title><style>{STYLESHEET}</style></head><body>{body}<footer><div class="wrap">{footer}</div></footer></body></html>"##,
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

    fn bar(scope: Scope) -> Bar {
        Bar {
            scope,
            who: "alice · oml".to_owned(),
            csrf: "c5".to_owned(),
        }
    }

    /// A page without its stylesheet, whose comments and selectors name
    /// words the markup must not.
    fn markup(html: &str) -> String {
        html.replace(STYLESHEET, "")
    }

    /// The `<nav>` of a page, whole.
    fn nav(html: &str) -> &str {
        let start = html.find("<nav").expect("a nav");
        let end = html[start..].find("</nav>").expect("a closed nav") + start + 6;
        &html[start..end]
    }

    #[test]
    fn a_page_escapes_what_a_person_or_the_store_supplied() {
        let html = shell(
            "t",
            &Bar {
                scope: Scope::Personal { admin: false },
                who: "<script>".to_owned(),
                csrf: "\"x".to_owned(),
            },
            START_PATH,
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

    /// The header carries the sign-out control on every page drawn for a
    /// session: a `POST` form to the lead's sign-out, never a link; a page
    /// drawn for no one carries none (#327).
    #[test]
    fn the_header_draws_the_sign_out_control_only_for_a_signed_in_session() {
        for scope in [Scope::Personal { admin: false }, Scope::Administration] {
            let signed_in = shell("t", &bar(scope), "", None, "");
            assert!(signed_in.contains(
                r#"<form method="post" action="/web"><input type="hidden" name="action" value="sign_out"><input type="hidden" name="csrf_token" value="c5"><button type="submit" class="linkish">Sign out</button></form></div></header>"#
            ), "{signed_in}");
            assert!(!signed_in.contains(r#"href="/web?action=sign_out"#));
        }
        let no_one = bare("t", "");
        assert!(!no_one.contains("sign_out"), "{no_one}");
    }

    /// A refusal page carries the same control where a session's CSRF
    /// token is given, and none for no one (#327).
    #[test]
    fn a_refusal_page_draws_the_sign_out_control_only_for_a_signed_in_session() {
        let control = r#"<form method="post" action="/web"><input type="hidden" name="action" value="sign_out"><input type="hidden" name="csrf_token" value="c5"><button type="submit" class="linkish">Sign out</button></form></div></header>"#;
        let signed_in = refusal_page(
            "access.not_found",
            "not found",
            Some("req-1"),
            Some(&bar(Scope::Personal { admin: false })),
            "/web/keys",
        );
        assert!(signed_in.contains(control), "{signed_in}");
        for no_one in [
            refusal("access.not_found", "not found"),
            refusal_page("access.not_found", "not found", Some("req-1"), None, "/web"),
        ] {
            assert!(!no_one.contains("sign_out"), "{no_one}");
            assert!(!no_one.contains(r#"class="end""#), "{no_one}");
        }
    }

    /// Each side's nav holds that side's pages alone, in the BrandHeader's
    /// order, under a label that is the scope word; the switch sits
    /// outside it (design notes row 13).
    #[test]
    fn each_side_draws_its_own_nav_exactly() {
        let personal = shell(
            "",
            &bar(Scope::Personal { admin: true }),
            USER_KEYS_PATH,
            None,
            "",
        );
        assert_eq!(
            nav(&personal),
            r#"<nav aria-label="Personal"><a href="/web/start">Start</a><a href="/web/memories">Memories</a><a href="/web/review">Review</a><a href="/web/atlas">Atlas</a><a href="/web/keys" class="on" aria-current="page">Keys</a><a href="/web/workareas">Workareas</a><a href="/web/guardrails">Guardrails</a></nav>"#
        );
        let admin = shell("", &bar(Scope::Administration), ADMIN_ATLAS_PATH, None, "");
        assert_eq!(
            nav(&admin),
            r#"<nav aria-label="Administration"><a href="/web/admin/people">People</a><a href="/web/admin/users">Quotas</a><a href="/web/admin/memories">Memories</a><a href="/web/admin/review">Review</a><a href="/web/admin/atlas" class="on" aria-current="page">Atlas</a><a href="/web/admin/keys">Keys</a><a href="/web/admin/workareas">Workareas</a><a href="/web/admin/curate">Curate</a></nav>"#
        );
        assert!(
            personal.contains(r#"<header class="top personal">"#),
            "{personal}"
        );
        assert!(admin.contains(r#"<header class="top admin">"#), "{admin}");
        assert!(
            personal.contains(r#"<a class="brand" href="/web/start" aria-label="Open Memory Lab: Personal, Start">"#),
            "{personal}"
        );
        assert!(
            admin.contains(r#"<a class="brand" href="/web/admin/people" aria-label="Open Memory Lab: Administration, People">"#),
            "{admin}"
        );
        assert!(personal.contains(&format!(
            r#"<span class="scope">{}Personal</span>"#,
            glyph_svg("personal")
        )));
        assert!(admin.contains(&format!(
            r#"<span class="scope">{}Administration</span>"#,
            glyph_svg("admin")
        )));
        // One switch, after the nav, to the other side's first page.
        assert!(personal.contains(&format!(
            r#"</nav><a class="switch" href="/web/admin/people">{}Administration</a><div class="end">"#,
            glyph_svg("admin")
        )), "{personal}");
        assert!(
            admin.contains(&format!(
                r#"</nav><a class="switch" href="/web/start">{}Personal</a><div class="end">"#,
                glyph_svg("personal")
            )),
            "{admin}"
        );
        assert!(!personal.contains("Your page") && !admin.contains("Your page"));
    }

    /// A person without the admin capacity is shown no word, link or
    /// address of the other side, on a page or on a refusal (ADR-0045 d10).
    #[test]
    fn a_session_without_the_admin_capacity_sees_nothing_of_administration() {
        let bar = bar(Scope::of_path("/web/admin/people", false));
        assert_eq!(bar.scope, Scope::Personal { admin: false });
        for html in [
            shell("", &bar, USER_REVIEW_PATH, None, ""),
            refusal_page(
                "access.not_found",
                "no such page here",
                Some("r"),
                Some(&bar),
                "/web/admin/people",
            ),
        ] {
            let html = markup(&html);
            for word in [
                "Administration",
                "/web/admin",
                "Admin ·",
                r#"class="switch""#,
            ] {
                assert!(!html.contains(word), "{word}: {html}");
            }
        }
    }

    /// The `<title>` names the page, its tab and the side; a page below a
    /// tab puts its own name first.
    #[test]
    fn the_title_names_the_page_its_tab_and_the_side() {
        let title = |html: &str| {
            let start = html.find("<title>").unwrap() + 7;
            html[start..start + html[start..].find("</title>").unwrap()].to_owned()
        };
        let personal = bar(Scope::Personal { admin: false });
        let admin = bar(Scope::Administration);
        assert_eq!(
            title(&shell("", &personal, USER_MEMORIES_PATH, None, "")),
            "Memories · OML"
        );
        assert_eq!(
            title(&shell("Memories", &personal, USER_MEMORIES_PATH, None, "")),
            "Memories · OML"
        );
        assert_eq!(
            title(&shell(
                "One memory",
                &personal,
                USER_MEMORIES_PATH,
                None,
                ""
            )),
            "One memory · Memories · OML"
        );
        assert_eq!(
            title(&shell("", &admin, "/web/admin/users", None, "")),
            "Admin · Quotas · OML"
        );
        assert_eq!(
            title(&shell("Confirm", &admin, ADMIN_REVIEW_PATH, None, "")),
            "Admin · Confirm · Review · OML"
        );
        assert_eq!(
            title(&refusal("access.not_found", "x")),
            "Not available · OML"
        );
        assert_eq!(
            title(&refusal_page(
                "access.not_found",
                "x",
                None,
                Some(&admin),
                ADMIN_KEYS_PATH
            )),
            "Admin · Not available · OML"
        );
    }

    /// A refusal is drawn inside the bar of the side it happened on, with
    /// one link back named by the page's tab, or by the side's first page
    /// where the path is under none (design notes row 25).
    #[test]
    fn a_refusal_page_is_drawn_in_its_sides_bar_with_a_named_link_back() {
        let admin = bar(Scope::Administration);
        let page = markup(&refusal_page(
            "limit.quota",
            "the memories quota of 1000 is reached",
            Some("req-9"),
            Some(&admin),
            ADMIN_MEMORY_PATH,
        ));
        assert!(page.contains(r#"<header class="top admin">"#), "{page}");
        assert!(
            page.contains(r#"<nav aria-label="Administration">"#),
            "{page}"
        );
        assert!(!page.contains("aria-current"), "{page}");
        assert!(page.contains(r#"<p class="error" role="alert"><code>limit.quota</code>: the memories quota of 1000 is reached</p>"#), "{page}");
        assert!(
            page.contains(r#"<p><a href="/web/admin/memories">Back to Memories</a></p>"#),
            "{page}"
        );
        let personal = bar(Scope::Personal { admin: false });
        let page = refusal_page(
            "access.not_found",
            "no such page here",
            None,
            Some(&personal),
            "/web/admin/review",
        );
        assert!(page.contains(r#"<header class="top personal">"#), "{page}");
        assert!(
            page.contains(r#"<p><a href="/web/start">Back to Start</a></p>"#),
            "{page}"
        );
        let page = refusal_page(
            "request.validation",
            "x",
            None,
            Some(&personal),
            "/web/keys",
        );
        assert!(
            page.contains(r#"<p><a href="/web/keys">Back to Keys</a></p>"#),
            "{page}"
        );
        assert!(refusal("access.not_found", "x").contains(r#"<p><a href="/web">Back</a></p>"#));
    }

    /// Who is signed in stays in the bar at every width: no rule of the
    /// stylesheet hides the name, the tenant or the scope word, and the
    /// nav wraps instead of scrolling inside itself (BrandHeader, Widths).
    #[test]
    fn the_bar_keeps_the_side_and_the_person_at_every_width() {
        let page = shell(
            "",
            &bar(Scope::Administration),
            ADMIN_MEMORIES_PATH,
            None,
            "",
        );
        assert!(
            page.contains(r#"<span class="name">alice · oml</span>"#),
            "{page}"
        );
        let sheet = include_str!("web.css");
        // Each rule, its selector and its declarations, inside a media
        // query or not.
        for rule in sheet.split('}') {
            let Some((selector, declarations)) = rule.rsplit_once('{') else {
                continue;
            };
            if [".who", ".name", ".scope", "header.top nav"]
                .iter()
                .any(|kept| selector.contains(kept))
            {
                assert!(!declarations.contains("display:none"), "{rule}");
                assert!(!declarations.contains("overflow-x:auto"), "{rule}");
            }
        }
        assert!(
            sheet.contains("header.top{position:sticky;top:0"),
            "the bar is pinned"
        );
    }
}
