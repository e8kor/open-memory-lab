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
//! The look is the landing page's: night indigo ground, paper ink, ice
//! links, star for the one primary act, the system sans for prose and mono
//! for labels, names and ids.

use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

use oml_app::domain::web::Provider;

use crate::oauth::page::{base64_32, client_string, escape, sha256};

/// The pages' one stylesheet, inline (BRAND.md; deploy/landing's tokens).
pub const STYLESHEET: &str = ":root{--ground:#0A0D22;--ground-alt:#0E1230;--panel:#111638;\
--ink:#EFE9DA;--ink-dim:#B7B4C6;--ink-faint:#8C8FA8;--rule:#262C52;--rule-soft:#1A1F42;\
--ice:#9FC6E6;--star:#F6A83E;--ok:#7FD1AE;--sans:ui-sans-serif,system-ui,-apple-system,\
\"Segoe UI\",Inter,sans-serif;--mono:ui-monospace,\"SF Mono\",\"JetBrains Mono\",Menlo,Consolas,\
monospace}*{box-sizing:border-box}body{margin:0;background:var(--ground);color:var(--ink);\
font:16px/1.6 var(--sans)}a{color:var(--ice);text-decoration:none}a:hover{text-decoration:\
underline}.wrap{max-width:1040px;margin:0 auto;padding:0 20px}header{border-bottom:1px solid \
var(--rule-soft)}.bar{display:flex;align-items:center;justify-content:space-between;gap:16px;\
min-height:60px;flex-wrap:wrap}.brand{font-weight:600;color:var(--ink);white-space:nowrap}\
.brand b{color:var(--star);font-weight:600}.who{font:12px var(--mono);color:var(--ink-faint)}\
nav.tabs{display:flex;gap:4px;flex-wrap:wrap;border-bottom:1px solid var(--rule-soft);\
margin:0 0 28px}nav.tabs a{padding:10px 14px;color:var(--ink-dim);font-size:14px;\
border-bottom:2px solid transparent}nav.tabs a.on{color:var(--ink);border-bottom-color:\
var(--star)}main{padding:28px 0 64px}h1{font-size:28px;line-height:1.15;margin:0 0 8px;\
font-weight:650;letter-spacing:-.01em}h2{font-size:19px;margin:32px 0 10px;font-weight:620}\
.lede{color:var(--ink-dim);margin:0 0 20px;max-width:44em}.label{font:12px var(--mono);\
letter-spacing:.12em;text-transform:uppercase;color:var(--ink-faint)}.mono,code,td.mono{\
font-family:var(--mono);font-size:14px}.card{border:1px solid var(--rule);border-radius:12px;\
padding:18px 20px;background:var(--ground-alt);margin:14px 0}.secret{border-color:var(--star)}\
.secret pre{margin:8px 0 0;font:14px/1.6 var(--mono);white-space:pre-wrap;overflow-wrap:\
anywhere;color:var(--ink)}table{width:100%;border-collapse:collapse;font-size:14px}th{text-align:\
left;font:12px var(--mono);letter-spacing:.08em;text-transform:uppercase;color:var(--ink-faint);\
padding:8px 10px;border-bottom:1px solid var(--rule)}td{padding:9px 10px;border-bottom:1px solid \
var(--rule-soft);vertical-align:top;overflow-wrap:anywhere}.text{white-space:pre-wrap}form{\
margin:0}form.row{display:flex;gap:10px;flex-wrap:wrap;align-items:flex-end;margin:12px 0}\
label{display:flex;flex-direction:column;gap:4px;font:12px var(--mono);color:var(--ink-faint)}\
input,select,textarea{font:15px var(--sans);color:var(--ink);background:var(--ground);border:1px \
solid var(--rule);border-radius:8px;padding:8px 10px;min-width:12rem}textarea{width:100%;\
min-height:7rem;font-family:var(--mono);font-size:14px}button{font:600 14px var(--sans);\
cursor:pointer;border-radius:8px;padding:9px 16px;border:1px solid var(--rule);background:\
transparent;color:var(--ink)}button:hover{border-color:var(--ice)}button.primary{background:\
var(--star);border-color:var(--star);color:#1A1206}\
.tag{display:inline-block;font:12px var(--mono);padding:1px 8px;border-radius:999px;border:1px \
solid currentColor}.held{color:var(--star)}.ok{color:var(--ok)}.dim{color:var(--ink-faint)}\
.error{color:var(--star);font-weight:600}.notice{color:var(--ok)}.signin{max-width:26rem;margin:\
48px auto}.signin button{width:100%;margin:8px 0}footer{border-top:1px solid var(--rule-soft);\
padding:18px 0;font:12px var(--mono);color:var(--ink-faint)}.wide{flex:1 1 100%}\
nav.toggle{display:inline-flex;border:1px solid var(--rule);border-radius:999px;overflow:hidden;\
margin:6px 0 14px}nav.toggle a{padding:6px 16px;font-size:14px;color:var(--ink-dim)}nav.toggle a.on{\
background:var(--panel);color:var(--ink);box-shadow:inset 0 -2px 0 var(--star)}\
.pager{display:flex;gap:16px;margin:18px 0}form.act{margin:0}form.act select{min-width:10rem}.confirm{border-color:var(--star)}\
.crumbs{font-size:14px;margin:0 0 10px;color:var(--ink-dim)}details.help{margin:-4px 0 14px;color:var(--ink-dim);\
font-size:14px;max-width:44em}details.help summary{cursor:pointer;color:var(--ink-faint);width:max-content}\
.desc{color:var(--ink-faint);font-size:14px;margin:-4px 0 14px}.filters{display:flex;flex-wrap:wrap;\
gap:var(--space-3,12px);align-items:flex-end;margin:0 0 var(--space-3,12px)}.filters label{font:400 13px/20px var(--sans);\
letter-spacing:0;text-transform:none;color:var(--ink-dim)}.filters input,.filters select{min-width:9rem}\
.mlist{display:flex;flex-direction:column;gap:var(--space-3,12px);max-width:var(--card-max,720px)}\
.mcard{border:1px solid var(--rule);border-radius:var(--radius-md,8px);background:var(--ground-alt);\
padding:var(--space-3,12px) var(--space-4,16px)}.mhead{display:flex;align-items:center;gap:var(--space-2,8px)}\
.mhead .end{margin-left:auto;display:flex;align-items:center;gap:var(--space-2,8px)}.chip{display:inline-flex;\
align-items:center;gap:var(--space-1,4px);font:500 12px/16px var(--sans);padding:1px var(--space-2,8px);\
border:1px solid currentColor;border-radius:var(--radius-pill,9999px)}.glyph{width:var(--glyph,12px);\
height:var(--glyph,12px);flex:none}.is-held,.g-held{color:var(--held,#f6a83e)}.is-promoted,.g-promoted{color:var(--promoted,#7fd1ae)}\
.is-superseded,.g-superseded{color:var(--superseded,#a6a9c0)}.is-rejected,.g-rejected{color:var(--rejected,#e99393)}\
.is-demoted,.g-demoted{color:var(--retired,#9598b0)}.g-in-progress{color:var(--in-progress,#bb95f4)}\
.ver{font:12px var(--mono);color:var(--ink-faint)}.steps{display:inline-flex;gap:2px;list-style:none;\
margin:0;padding:0}.steps li{width:12px;height:6px;border-radius:2px;background:var(--rule)}.steps li.done{background:var(--promoted,#7fd1ae)}\
.steps li.no{background:var(--rejected,#e99393)}.mtitle{font-weight:600;margin:var(--space-2,8px) 0 0}\
.mtext{margin:var(--space-2,8px) 0;color:var(--ink);max-width:var(--measure,44em);overflow-wrap:anywhere}\
.mtext.mono{font:14px/22px var(--mono)}.mby{font-size:13px;color:var(--ink-dim);margin:0}.mby b{font-weight:500;\
color:var(--ink)}details.more{margin-top:var(--space-1,4px)}details.more summary{cursor:pointer;\
color:var(--ink-faint);font-size:13px;width:max-content}.timeline{list-style:none;padding:0;margin:var(--space-2,8px) 0;\
display:grid;gap:2px;font-size:13px;color:var(--ink-dim)}.timeline li.wait{color:var(--ink-faint)}\
.timeline li.no{color:var(--rejected,#e99393)}.timeline b{display:inline-block;min-width:6.5em;color:var(--ink);\
font-weight:500}.facts{display:grid;grid-template-columns:max-content 1fr;gap:2px var(--space-3,12px);\
font-size:13px;margin:var(--space-1,4px) 0 var(--space-2,8px)}.facts dt{color:var(--ink-faint)}.facts dd{margin:0;\
overflow-wrap:anywhere}.atlas-grid{display:grid;grid-template-columns:minmax(0,1fr) var(--rail-w,264px);\
gap:var(--space-6,24px);align-items:start}@media (max-width:1100px){.atlas-grid{grid-template-columns:minmax(0,1fr)}\
}.tm-fig{margin:0}.tm{width:100%;height:auto;display:block}.tm.narrow{display:none}@media (max-width:600px){.tm.wide{display:none}\
.tm.narrow{display:block}}.tm-r{fill:var(--panel);stroke:var(--rule);stroke-width:1}.tm a:hover .tm-r,.tm a:focus .tm-r{stroke:var(--focus,#9fc6e6);\
stroke-width:2}.tm-r.ip{stroke:var(--in-progress,#bb95f4);stroke-dasharray:5 3}.tm-nm{fill:var(--ink);\
font-family:var(--mono)}.tm-n{fill:var(--ink-dim);font-family:var(--sans);font-variant-numeric:tabular-nums}\
.tm-ip{fill:var(--in-progress,#bb95f4);font-family:var(--sans)}.bar.promoted{fill:var(--promoted,#7fd1ae)}\
.bar.held{fill:var(--held,#f6a83e)}.bar.unreviewed{fill:var(--unreviewed,#bb95f4)}.bar.superseded{fill:var(--superseded,#a6a9c0)}\
.lifebar{width:100%;height:var(--bar-h,8px);display:block;margin:var(--space-1,4px) 0}.twin{margin-top:var(--space-4,16px)}\
.twin .num{text-align:right;font-variant-numeric:tabular-nums}.twin td:last-child{min-width:8rem}\
.twin .cap{font-size:12px;color:var(--ink-faint)}.legend{list-style:none;padding:0;margin:var(--space-1,4px) 0;\
display:flex;flex-wrap:wrap;gap:var(--space-1,4px) var(--space-3,12px);font-size:13px;color:var(--ink-dim)}\
.legend li{display:flex;align-items:center;gap:var(--space-1,4px)}.legend b{color:var(--ink);font-variant-numeric:tabular-nums}\
.panel{border:1px solid var(--rule);border-radius:var(--radius-md,8px);padding:var(--space-3,12px) var(--space-4,16px);\
background:var(--ground-alt)}.panel h2{margin:0 0 4px;font-size:16px;overflow-wrap:anywhere}@media (max-width:600px){.twin th:nth-child(2),.twin td:nth-child(2){display:none}\
}";

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

/// A state's tag: held in star, promoted in green, the rest dim.
#[must_use]
pub fn state_tag(state: &str) -> String {
    let class = match state {
        "held" => "held",
        "promoted" => "ok",
        _ => "dim",
    };
    format!(r#"<span class="tag {class}">{}</span>"#, escape(state))
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
    let mut nav = String::from(r#"<nav class="tabs">"#);
    for (href, shown) in tabs {
        let on = if *href == active {
            r#" class="on""#
        } else {
            ""
        };
        let _ = write!(nav, r#"<a href="{href}"{on}>{}</a>"#, escape(shown));
    }
    nav.push_str("</nav>");
    let notice = notice.map_or(String::new(), |n| {
        format!(r#"<p class="notice">{}</p>"#, escape(n))
    });
    document(
        title,
        &format!(
            r#"<header><div class="wrap bar"><a class="brand" href="/web">Open <b>Memory</b> Lab</a><span class="who">{}</span></div></header><main class="wrap">{nav}{notice}{body}</main>"#,
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
    body.push_str("</div>");
    document(
        "Sign in",
        &format!(
            r#"<header><div class="wrap bar"><span class="brand">Open <b>Memory</b> Lab</span></div></header><main class="wrap">{body}</main>"#
        ),
    )
}

/// A refusal answered on a page, never a redirect: its code and message.
#[must_use]
pub fn refusal(code: &str, message: &str) -> String {
    document(
        "Not available",
        &format!(
            r#"<header><div class="wrap bar"><a class="brand" href="/web">Open <b>Memory</b> Lab</a></div></header><main class="wrap"><h1>This cannot continue</h1><p class="error">{}: {}</p><p><a href="/web">Back</a></p></main>"#,
            escape(code),
            escape(message)
        ),
    )
}

fn document(title: &str, body: &str) -> String {
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><meta name="theme-color" content="#0A0D22"><title>Open Memory Lab: {}</title><style>{STYLESHEET}</style></head><body>{body}<footer><div class="wrap">Every act on these pages is journalled under one of your keys.</div></footer></body></html>"##,
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
