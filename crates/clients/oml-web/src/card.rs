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
/// The order a finding's verdict takes on the card: refuse, then hold,
/// then annotate (ADR-0074 d17).
fn verdict_rank(verdict: &str) -> u8 {
    match verdict {
        "refuse" => 0,
        "hold" => 1,
        _ => 2,
    }
}

/// The chip a finding's verdict shows: the held colour for a hold, the
/// rejected one for a refuse-class finding a rescan made, grey for an
/// annotation (ADR-0074 d17). The design system's own finding mark
/// replaces it once ADR-0069 adds one.
fn verdict_chip(verdict: &str) -> String {
    let class = match verdict {
        "refuse" => "rejected",
        "hold" => "held",
        _ => "quiet",
    };
    format!(
        r#"<span class="tag oml-chip is-{class}">{}</span>"#,
        text(verdict)
    )
}

/// The findings block of a card (ADR-0074 d17): one line per stored
/// finding, refuse and hold before annotate, each its verdict, `class ·
/// rule`, and where: the field and line, the view the rule read when it
/// was not the text as written, and who found it when a rescan did. Never
/// the matched text. A version whose single stored finding has no list
/// (written before the findings list) shows that finding as stored; a
/// version with neither shows nothing.
pub(crate) fn findings_block(m: &Memory) -> String {
    let mut lines: Vec<(u8, String)> = Vec::new();
    match &m.scan_findings {
        Some(list) if !list.is_empty() => {
            for f in list {
                let mut place = match (&f.field, f.line) {
                    (Some(field), Some(line)) => format!("{} line {line}", text(field)),
                    (Some(field), None) => text(field),
                    (None, _) => f
                        .migrated_from
                        .as_deref()
                        .map_or_else(|| "stored before the list".to_owned(), text),
                };
                if let Some(via) = f.via.as_deref().filter(|v| *v != "raw") {
                    let _ = write!(place, " · read {}", text(via));
                }
                if f.found_by != "admission" {
                    let _ = write!(place, " · found by {}", text(&f.found_by));
                }
                lines.push((
                    verdict_rank(&f.verdict),
                    format!(
                        r#"<li>{} <span class="mono">{} · {}</span> <span class="dim mono">{place}</span></li>"#,
                        verdict_chip(&f.verdict),
                        text(&f.class),
                        text(&f.rule),
                    ),
                ));
            }
        }
        _ => {
            if let Some(stored) = &m.scan_finding {
                lines.push((
                    1,
                    format!(
                        r#"<li>{} <span class="mono">{}</span></li>"#,
                        verdict_chip("hold"),
                        text(stored)
                    ),
                ));
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    lines.sort_by_key(|(rank, _)| *rank);
    format!(
        r#"<div class="findings"><p class="label">scan findings · {}</p><ul>{}</ul></div>"#,
        lines.len(),
        lines.into_iter().map(|(_, l)| l).collect::<String>()
    )
}

/// The bulk table's Findings cell (ADR-0074 d17): the count by verdict,
/// strongest first, `1 hold · 2 annotate`; a dash when there is none.
pub(crate) fn findings_cell(m: &Memory) -> String {
    let mut counts: Vec<(u8, &str, usize)> = Vec::new();
    match &m.scan_findings {
        Some(list) if !list.is_empty() => {
            for f in list {
                let rank = verdict_rank(&f.verdict);
                match counts.iter_mut().find(|(_, v, _)| *v == f.verdict) {
                    Some(c) => c.2 += 1,
                    None => counts.push((rank, f.verdict.as_str(), 1)),
                }
            }
        }
        _ if m.scan_finding.is_some() => counts.push((1, "hold", 1)),
        _ => return "-".to_owned(),
    }
    counts.sort_by_key(|(rank, _, _)| *rank);
    counts
        .iter()
        .map(|(_, v, n)| format!("{n} {}", text(v)))
        .collect::<Vec<_>>()
        .join(" · ")
}

pub(crate) fn card(m: &Memory, drawing: &str) -> String {
    let warning = m.load_warning.as_deref().map_or_else(String::new, |w| {
        format!(r#"<p class="lede">Guardrail: {}.</p>"#, text(w))
    });
    let title = m.title.as_deref().map_or_else(String::new, |t| {
        format!(r#"<p class="label">title</p><p>{}</p>"#, text(t))
    });
    format!(
        r#"{warning}<div class="card"><p class="label">{} · {} · version {}</p>{}{title}<p class="text{}">{}</p>{drawing}{}<p class="dim mono">{} · from {}</p><p class="dim mono">source: {}</p><p class="dim mono">valid {}</p></div>"#,
        state_tag(memory_state(m)),
        short(m.workarea_path.as_deref().unwrap_or("")),
        m.version,
        findings_block(m),
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
