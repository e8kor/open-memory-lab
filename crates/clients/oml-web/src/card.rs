//! A memory's card, one renderer for every page that shows one: the Review
//! view's served item and the memory's own page (ADR-0045 d11; ADR-0056 d9;
//! ADR-0057 d2; ADR-0059 d2, d4).

use std::fmt::Write as _;

use oml_page::web::{short, state_tag, text};
use oml_sdk::{Anchor, Memory};

/// An anchor as one line for a reviewer (ADR-0057 d2): the repository, the
/// commit's first 12 digits, the path and the fragment, or the page; a
/// dash for none.
pub(crate) fn anchor_line(anchor: Option<&Anchor>) -> String {
    let Some(a) = anchor else {
        return "—".to_owned();
    };
    let fragment = a
        .fragment
        .as_deref()
        .map_or_else(String::new, |f| format!("#{f}"));
    let line = match (a.repo.as_deref(), a.uri.as_deref()) {
        (Some(repo), _) => {
            let rev = a.rev.as_deref().unwrap_or_default();
            format!(
                "{repo} @ {} {}{fragment}",
                &rev[..rev.len().min(12)],
                a.path.as_deref().unwrap_or_default()
            )
        }
        (None, Some(uri)) => format!("{uri}{fragment}"),
        (None, None) => return "—".to_owned(),
    };
    short(&line)
}

/// A version's valid time as one line, and on a held successor the end
/// approving it gives the version it replaces (ADR-0059 d2, d4).
pub(crate) fn valid_line(m: &Memory) -> String {
    let mut line = if m.valid_from.is_none() && m.valid_until.is_none() {
        "none declared".to_owned()
    } else {
        format!(
            "from {}, {}",
            m.valid_from.as_deref().unwrap_or("any time"),
            m.valid_until
                .as_deref()
                .map_or_else(|| "open".to_owned(), |u| format!("until {u}"))
        )
    };
    if let (Some(end), Some(replaces)) = (&m.replaces_valid_until, &m.replaces) {
        let _ = write!(
            line,
            "; approving ends version {} at {end}",
            replaces.version
        );
    }
    text(&line)
}

pub(crate) fn prose(m: &Memory) -> bool {
    matches!(
        m.format.as_deref().unwrap_or("text/plain"),
        "text/plain" | "text/markdown"
    )
}

/// What a card says of a version's format (ADR-0056 d9): the title, the
/// media type, and what a derived claim will be found by. The source is
/// the card's text, escaped, never rendered.
pub(crate) fn format_lines(m: &Memory) -> String {
    let mut out = String::new();
    if !prose(m) {
        let _ = write!(
            out,
            r#"<p class="label">format</p><p class="mono">{}</p>"#,
            text(m.format.as_deref().unwrap_or_default())
        );
    }
    if let Some(found) = &m.search_text {
        let _ = write!(
            out,
            r#"<p class="label">it will be found by</p><p class="text mono dim">{}</p>"#,
            text(found)
        );
    }
    out
}

fn memory_state(m: &Memory) -> &str {
    match m.state.as_str() {
        "promoted" if !m.tip => "superseded",
        "held" | "promoted" | "rejected" => m.state.as_str(),
        _ => "demoted",
    }
}

/// A memory's card, as the lead drew it, with `drawing` (the frame of a
/// Mermaid claim, [`crate::render::drawing`], or nothing) under its source:
/// the source is always the card's text, escaped, never rendered.
pub(crate) fn card(m: &Memory, drawing: &str) -> String {
    let warning = m.load_warning.as_deref().map_or_else(String::new, |w| {
        format!(r#"<p class="lede">Guardrail: {}.</p>"#, text(w))
    });
    let title = m.title.as_deref().map_or_else(String::new, |t| {
        format!(r#"<p class="label">title</p><p>{}</p>"#, text(t))
    });
    format!(
        r#"{warning}<div class="card"><p class="label">{} · {} · version {}</p>{title}<p class="text{}">{}</p>{drawing}{}<p class="dim mono">{} · from {}</p><p class="dim mono">source: {}</p><p class="dim mono">valid {}</p></div>"#,
        state_tag(memory_state(m)),
        short(m.workarea_path.as_deref().unwrap_or("")),
        m.version,
        if prose(m) { "" } else { " mono" },
        text(m.text.as_deref().unwrap_or_default()),
        format_lines(m),
        short(&m.claim_key),
        m.principal
            .as_deref()
            .map_or_else(|| "the shared area".to_owned(), short),
        anchor_line(m.anchor.as_ref()),
        valid_line(m)
    )
}
