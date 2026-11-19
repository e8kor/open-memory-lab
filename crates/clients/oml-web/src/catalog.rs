//! The Personal side's Catalog page (ADR-0092 d7): every kind of claim the
//! person's web key reads in a workspace subtree, one row per claim tip,
//! newest first, with kind tabs and their counts.
//!
//! Each page is one catalog read, `GET /v1/memories` without `own`, as
//! `<user>-web`, with no text asked for; the first page's answer carries
//! each kind's count (ADR-0092 d6). Beside it the page makes the workspace
//! picker's names read, as every page with the field does, and reads the
//! node of a top label a row names that the names leave unknown. Every
//! control is a link or a GET form, and nothing on the page writes.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::State;
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{
    USER_CATALOG_PATH, USER_MEMORY_PATH, USER_WORKSPACES_PATH, attr, glyph_svg, hidden, input,
    select, shell, state_tag, text,
};
use oml_sdk::{CatalogCount, CatalogEntry, CatalogPage, CatalogQuery};

use crate::keys::instant;
use crate::memories::{Failed, Primary, Side, failed_page, picker_reads, settle, with_status};
use crate::pages::{Fields, refused};
use crate::picker::{self, Browse, Chosen, Field, Names, Roots};
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, outcome};

/// A page's rows. The listing serves at most `api.page_limit_max` (200);
/// 50 is the Memories list's default page (ADR-0045 d22).
pub const PAGE_SIZE: u32 = 50;

/// The kind tabs, in ADR-0092 d7's order: the value the `kinds` field
/// carries (empty for All), the tab's word, and the kinds it counts.
const TABS: [(&str, &str, &[&str]); 9] = [
    ("", "All", &[]),
    ("memory", "Memories", &["memory"]),
    ("derived_fact", "Derived facts", &["derived_fact"]),
    ("document", "Documents", &["document"]),
    ("test", "Tests", &["test"]),
    ("skill", "Skills", &["skill"]),
    ("guardrail", "Guardrails", &["guardrail"]),
    ("steering", "Steering", &["steering"]),
    (
        "term,term_mapping,vocab_release",
        "Terms",
        &["term", "term_mapping", "vocab_release"],
    ),
];

/// The State field's values and the listing's `state` each sends: All
/// sends none (the design names no default, so absent is All).
const STATES: [(&str, &str, Option<&str>); 3] = [
    ("", "All", None),
    ("approved", "Approved", Some("promoted")),
    ("unreviewed", "Unreviewed", Some("held")),
];

/// The page's query, each field checked before any call.
#[derive(Clone)]
struct Query {
    workspace: Option<String>,
    /// The index of the tab in [`TABS`].
    tab: usize,
    prefix: Option<String>,
    /// The index of the state in [`STATES`].
    state: usize,
    cursor: Option<String>,
}

impl Query {
    fn parse(fields: &Fields) -> Result<Self, String> {
        let value = |name: &str| fields.given(name).map(str::to_owned);
        let kinds = fields.get("kinds").unwrap_or_default();
        let tab = TABS
            .iter()
            .position(|(v, _, _)| *v == kinds)
            .ok_or_else(|| {
                "kinds is one tab's: memory, derived_fact, document, test, skill, guardrail, steering or term,term_mapping,vocab_release".to_owned()
            })?;
        let state = fields.get("state").unwrap_or_default();
        let state = STATES
            .iter()
            .position(|(v, _, _)| *v == state)
            .ok_or_else(|| "state is approved, unreviewed or empty for all".to_owned())?;
        Ok(Self {
            workspace: value("workspace"),
            tab,
            prefix: value("prefix"),
            state,
            cursor: value("cursor"),
        })
    }

    fn kinds(&self) -> &'static str {
        TABS[self.tab].0
    }

    /// The page's link with every filter kept, `tab` in place of the
    /// current one, and the cursor when given; escaped for an attribute.
    fn link(&self, tab: usize, cursor: Option<&str>) -> String {
        let mut pairs = Vec::new();
        for (name, value) in [
            ("workspace", self.workspace.as_deref()),
            ("kinds", Some(TABS[tab].0).filter(|k| !k.is_empty())),
            ("prefix", self.prefix.as_deref()),
            (
                "state",
                Some(STATES[self.state].0).filter(|s| !s.is_empty()),
            ),
            ("cursor", cursor),
        ] {
            if let Some(value) = value {
                pairs.push(format!("{name}={}", attr(&oml_sdk::encode(value))));
            }
        }
        if pairs.is_empty() {
            USER_CATALOG_PATH.to_owned()
        } else {
            format!("{USER_CATALOG_PATH}?{}", pairs.join("&amp;"))
        }
    }

    /// The filters the picker's links keep beside the workspace.
    fn keep(&self) -> Vec<(&'static str, String)> {
        [
            (
                "kinds",
                Some(self.kinds().to_owned()).filter(|k| !k.is_empty()),
            ),
            ("prefix", self.prefix.clone()),
            (
                "state",
                Some(STATES[self.state].0.to_owned()).filter(|s| !s.is_empty()),
            ),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|v| (name, v)))
        .collect()
    }
}

/// One page of the catalog as `<user>-web` reads it at `workspace`.
async fn read(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    q: &Query,
    workspace: Option<&str>,
) -> Result<CatalogPage, Failed> {
    let credential = session.credential(Capacity::Web);
    let ids = calls.next_ids();
    let query = CatalogQuery {
        workspace,
        kinds: Some(q.kinds()).filter(|k| !k.is_empty()),
        prefix: q.prefix.as_deref(),
        state: STATES[q.state].2,
        limit: PAGE_SIZE,
        cursor: q.cursor.as_deref(),
    };
    Ok(web.api.catalog(&call(&credential, &ids), &query).await?)
}

/// A count as a tab shows it: digits grouped by thousands, `+` after a
/// lower bound.
fn count_word(n: u64, capped: bool) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if capped {
        out.push('+');
    }
    out
}

/// A tab's count from the first page's `counts`: the sum of its kinds,
/// every kind for All, a lower bound where any of them is capped.
fn tab_count(
    counts: &std::collections::BTreeMap<String, CatalogCount>,
    kinds: &[&str],
) -> (u64, bool) {
    counts
        .iter()
        .filter(|(kind, _)| kinds.is_empty() || kinds.contains(&kind.as_str()))
        .fold((0, false), |(n, capped), (_, count)| match count {
            CatalogCount::Exact(c) => (n + c, capped),
            CatalogCount::AtLeast { at_least } => (n + at_least, true),
        })
}

/// The kind tabs: each a link keeping every filter, with its count on the
/// first page; a zero is greyed and still a link.
fn tabs(q: &Query, page: Option<&CatalogPage>) -> String {
    let counts = page.and_then(|p| p.counts.as_ref());
    let mut out = String::from(r#"<nav class="oml-views kinds" aria-label="Kinds">"#);
    for (i, (_, word, kinds)) in TABS.iter().enumerate() {
        let (n, zero) = match counts {
            Some(counts) => {
                let (n, capped) = tab_count(counts, kinds);
                (
                    format!(r#" <span class="n">{}</span>"#, count_word(n, capped)),
                    n == 0,
                )
            }
            None => (String::new(), false),
        };
        let current = if i == q.tab {
            r#" aria-current="page""#
        } else {
            ""
        };
        let class = if zero { r#" class="zero""# } else { "" };
        let _ = write!(
            out,
            r#"<a href="{}"{class}{current}>{word}{n}</a>"#,
            q.link(i, None)
        );
    }
    out.push_str("</nav>");
    out
}

/// The word a kind's chip shows.
fn kind_word(kind: &str) -> &str {
    match kind {
        "derived_fact" => "derived fact",
        "term_mapping" => "term mapping",
        "vocab_release" => "vocabulary release",
        other => other,
    }
}

/// Whether a kind's row links to the workspace page rather than to the
/// memory's own page (ADR-0082 d1's kind view, here the Workspaces page).
fn opens_at_workspace(kind: &str) -> bool {
    matches!(kind, "skill" | "guardrail" | "document")
}

/// A guardrail's checked rule as words: its effect, tool and command where
/// it has them (ADR-0037 d8).
fn rule_words(rule: Option<&serde_json::Value>) -> Option<String> {
    let rule = rule?.as_object()?;
    let words: Vec<&str> = ["effect", "tool", "command"]
        .iter()
        .filter_map(|k| rule.get(*k).and_then(|v| v.as_str()))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// The State cell (ADR-0092 d7; ADR-0077 d2's words): retired; held and
/// served here, with "needs a person" on an instruction; approved with the
/// instant and the reviewer's kind; any other state as its chip.
fn state_cell(e: &CatalogEntry) -> String {
    if e.retirement {
        return state_tag("retired");
    }
    if e.unreviewed {
        let mut out = format!(
            r#"{}<span class="when">served here, unreviewed</span>"#,
            state_tag("unreviewed")
        );
        if e.needs_person {
            let _ = write!(
                out,
                r#" <span class="tag oml-chip is-quiet">{}needs a person</span>"#,
                glyph_svg("person")
            );
        }
        return out;
    }
    if e.state != "promoted" {
        return state_tag(&e.state);
    }
    let mut out = state_tag("promoted");
    if let Some(at) = &e.approved_at {
        let by = e
            .approver
            .as_ref()
            .map_or("", |a| match a.used_by.as_str() {
                "person" => " by a person reviewer",
                "agent" => " by an agent reviewer",
                _ => " by a service reviewer",
            });
        let _ = write!(out, r#"<span class="when">{}{by}</span>"#, instant(at));
    }
    out
}

/// One row: Name, Kind, Description, Workspace, Version, State, Changed.
fn row(e: &CatalogEntry, scope: &str, roots: &Roots) -> String {
    let path = e.workspace_path.as_deref().unwrap_or_default();
    let reference = roots.reference(path);
    let workspace_link = format!(
        "{USER_WORKSPACES_PATH}?workspace={}",
        attr(&oml_sdk::encode(&reference))
    );
    let shown = match e.kind.as_str() {
        "skill" => e.name.as_deref(),
        "document" => e.title.as_deref(),
        _ => None,
    };
    let href = if opens_at_workspace(&e.kind) {
        workspace_link.clone()
    } else {
        format!(
            "{USER_MEMORY_PATH}?id={}",
            attr(&oml_sdk::encode(&e.artifact_id))
        )
    };
    let name = match shown {
        Some(name) => format!(
            r#"<a href="{href}">{}</a><span class="key">{}</span>"#,
            text(name),
            text(&e.claim_key)
        ),
        None => format!(
            r#"<a href="{href}" class="mono">{}</a>"#,
            text(&e.claim_key)
        ),
    };
    let mut kind = format!(
        r#"<span class="tag oml-chip is-quiet">{}</span>"#,
        text(kind_word(&e.kind))
    );
    let here = |yes: bool, word: &str| {
        if yes {
            format!(
                r#"<span class="mark">{}{word} here</span>"#,
                glyph_svg("can")
            )
        } else {
            format!(
                r#"<span class="mark">not {word} at <code>{}</code></span>"#,
                text(scope)
            )
        }
    };
    if !e.retirement {
        match e.kind.as_str() {
            "skill" => kind.push_str(&here(e.prompt.is_some(), "offered")),
            "guardrail" => kind.push_str(&here(e.in_force == Some(true), "in force")),
            _ => {}
        }
    }
    let description = if e.retirement {
        "—".to_owned()
    } else {
        match e.kind.as_str() {
            "guardrail" => {
                let severity = e.severity.as_deref().unwrap_or_default();
                let rule = rule_words(e.rule.as_ref())
                    .map_or_else(String::new, |r| format!(" {}", text(&r)));
                format!(r#"<b class="sev">{}</b>{rule}"#, text(severity))
            }
            "document" => e.size.map_or_else(
                || "—".to_owned(),
                |size| {
                    format!(
                        r#"<span class="sz">{} bytes</span>"#,
                        count_word(size, false)
                    )
                },
            ),
            _ => match &e.description {
                Some(d) if e.preview => format!(
                    r#"<span class="tag oml-chip is-quiet">preview</span>{}"#,
                    text(d)
                ),
                Some(d) => text(d),
                None => "—".to_owned(),
            },
        }
    };
    format!(
        r#"<tr><td class="c-name">{name}</td><td class="c-kind"><span class="cell">{kind}</span></td><td class="c-desc">{description}</td><td class="c-ws c-meta"><a href="{workspace_link}">{}</a></td><td class="c-ver c-meta">v{}</td><td class="c-state">{}</td><td class="c-when c-meta">{}</td></tr>"#,
        text(&reference),
        e.version,
        state_cell(e),
        instant(&e.created_at),
    )
}

/// The page's body: breadcrumb, heading and capacity line, the filter
/// form, the tabs, then the table or the empty state, and the pager.
fn body(
    session: &Session,
    q: &Query,
    field: &str,
    scope: &str,
    page: Option<(&CatalogPage, &Roots)>,
) -> String {
    let web_key = session
        .whoami
        .user
        .as_ref()
        .map_or_else(|| "the web key".to_owned(), |u| format!("{}-web", u.name));
    let mut out = format!(
        r#"<p class="oml-crumbs"><a href="{}">Personal</a> / Catalog</p><header class="oml-section-head"><div class="top"><h1>Catalog</h1></div><p class="desc">Read as <code>{}</code>: one row per claim, newest version first.</p></header>"#,
        oml_page::web::START_PATH,
        text(&web_key)
    );
    // Unreviewed is offered only where the read serves held rows: the page
    // holds one, or it is the filter already (the build's reading).
    let held_served =
        q.state == 2 || page.is_some_and(|(p, _)| p.items.iter().any(|e| e.unreviewed));
    let mut states: Vec<(&str, &str)> = vec![(STATES[q.state].0, STATES[q.state].1)];
    states.extend(
        STATES
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != q.state && (*i != 2 || held_served))
            .map(|(_, (v, w, _))| (*v, *w)),
    );
    let _ = write!(
        out,
        r#"<form class="filters" method="get" action="{USER_CATALOG_PATH}">{field}{}{}{}<button type="submit">Apply</button></form><p class="oml-scope-line">Reads <code>{}</code> and every workspace below it that the key reads.</p><p class="oml-scope-line">Name starts with: skill/, doc/runbooks/ or any claim key.</p>"#,
        hidden("kinds", q.kinds()),
        input(
            "prefix",
            "Name starts with",
            q.prefix.as_deref().unwrap_or(""),
            false
        ),
        select("state", "State", &states),
        text(scope),
    );
    out.push_str(&tabs(q, page.map(|(p, _)| p)));
    let Some((page, roots)) = page else {
        return out;
    };
    if page.items.is_empty() {
        let _ = write!(
            out,
            r#"<div class="oml-empty"><p>Nothing of this kind in <code>{}</code> that this key reads.</p></div>"#,
            text(scope)
        );
    } else {
        out.push_str(r#"<table class="oml-table cat"><thead><tr><th scope="col">Name</th><th scope="col">Kind</th><th scope="col">Description</th><th scope="col">Workspace</th><th scope="col">Version</th><th scope="col">State</th><th scope="col">Changed</th></tr></thead><tbody>"#);
        for e in &page.items {
            out.push_str(&row(e, scope, roots));
        }
        out.push_str("</tbody></table>");
    }
    out.push_str(r#"<p class="pager">"#);
    if q.cursor.is_some() {
        let _ = write!(out, r#"<a href="{}">First page</a> "#, q.link(q.tab, None));
    }
    if let Some(next) = &page.next_cursor {
        let _ = write!(
            out,
            r#"<a rel="next" href="{}">Next page</a>"#,
            q.link(q.tab, Some(next))
        );
    }
    out.push_str("</p>");
    out
}

/// `GET /web/catalog`: the Personal side's Catalog page (ADR-0092 d7).
pub async fn page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    let next = uri
        .path_and_query()
        .map_or(USER_CATALOG_PATH, |pq| pq.as_str());
    let parsed = Fields::of(&uri)
        .and_then(|f| Query::parse(&f).map(|q| (q, picker::browse_asked(f.get(picker::BROWSE)))));
    let (mut q, browse) = match parsed {
        Ok(parsed) => parsed,
        Err(message) => {
            return refused(
                StatusCode::BAD_REQUEST,
                "request.validation",
                &message,
                &calls,
            );
        }
    };
    // The empty field reads the person's root, as the Atlas does: the web
    // key names no default workspace (ADR-0045 d25).
    let own = session.whoami.user.as_ref().map(|u| u.name.clone());
    let chosen = Chosen::of(q.workspace.as_deref());
    let first = async {
        if chosen.refused() {
            None
        } else {
            let at = chosen.value().or(own.as_deref());
            Some(read(&web, &session, &calls, &q, at).await)
        }
    };
    let ((names, browse), first) = tokio::join!(
        picker_reads(&web, &session, &calls, Side::User, browse, true),
        first
    );
    let primary = settle(&names, chosen, first, |id| {
        let (web, session, calls, q) = (&web, &session, &calls, &q);
        async move { read(web, session, calls, q, Some(&id)).await }
    })
    .await;
    let (chosen, page) = match primary {
        Primary::Served(chosen, page) => (chosen, Some(page)),
        Primary::Missed(chosen) => (chosen, None),
        Primary::Failed(Failed::Api(e)) => return outcome::page(&e, &calls, next),
        Primary::Failed(failed) => return failed_page(failed, &calls, next),
    };
    q.workspace = chosen.shown().map(str::to_owned);
    let scope = q
        .workspace
        .clone()
        .or_else(|| own.clone())
        .unwrap_or_default();
    // Each row names its workspace by reference, as the Memories list does.
    let roots = match &page {
        Some(page) => {
            names
                .roots()
                .clone()
                .with_own(own.as_deref())
                .learn(
                    &web,
                    &session.credential(Capacity::Web),
                    &calls,
                    page.items
                        .iter()
                        .filter_map(|e| e.workspace_path.as_deref()),
                )
                .await
        }
        None => names.roots().clone(),
    };
    let keep = q.keep();
    let field = picker_field(&keep, &chosen, &names, browse.as_ref());
    let drawn = body(
        &session,
        &q,
        &field,
        &scope,
        page.as_ref().map(|p| (p, &roots)),
    );
    with_status(
        outcome::html(
            StatusCode::OK,
            shell("", &session.personal(), USER_CATALOG_PATH, None, &drawn),
        ),
        chosen.status(),
    )
}

/// The workspace field: names only, empty the person's root.
fn picker_field(
    keep: &[(&'static str, String)],
    chosen: &Chosen,
    names: &Names,
    browse: Option<&Browse>,
) -> String {
    Field {
        id: "wa",
        action: USER_CATALOG_PATH,
        keep,
        chosen,
        names,
        browse,
        admin: false,
        standalone: false,
        empty: "Empty: the user root.",
    }
    .html()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(pairs: &[(&str, &str)]) -> Result<Query, String> {
        let pairs = pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect();
        Query::parse(&Fields::of_pairs(pairs).unwrap())
    }

    #[test]
    fn a_tab_link_keeps_every_filter_and_drops_the_cursor() {
        let q = parsed(&[
            ("workspace", "alice.proj"),
            ("kinds", "skill"),
            ("prefix", "skill/de"),
            ("state", "approved"),
            ("cursor", "c 1"),
        ])
        .unwrap();
        assert_eq!(
            q.link(6, None),
            "/web/catalog?workspace=alice.proj&amp;kinds=guardrail&amp;prefix=skill%2Fde&amp;state=approved"
        );
        assert_eq!(
            q.link(0, Some("c 2")),
            "/web/catalog?workspace=alice.proj&amp;prefix=skill%2Fde&amp;state=approved&amp;cursor=c%202"
        );
        assert_eq!(
            q.link(8, None),
            "/web/catalog?workspace=alice.proj&amp;kinds=term%2Cterm_mapping%2Cvocab_release&amp;prefix=skill%2Fde&amp;state=approved"
        );
    }

    #[test]
    fn an_unknown_tab_or_state_is_refused_and_empty_fields_are_no_filter() {
        assert!(parsed(&[("kinds", "skills")]).is_err());
        assert!(parsed(&[("kinds", "term")]).is_err());
        assert!(parsed(&[("state", "held")]).is_err());
        let q = parsed(&[
            ("kinds", ""),
            ("state", ""),
            ("prefix", ""),
            ("workspace", ""),
        ])
        .unwrap();
        assert_eq!((q.tab, q.state), (0, 0));
        assert!(q.prefix.is_none() && q.workspace.is_none());
        assert_eq!(q.link(0, None), "/web/catalog");
    }

    #[test]
    fn a_tab_counts_its_kinds_and_a_capped_kind_makes_the_sum_a_lower_bound() {
        let counts: std::collections::BTreeMap<String, CatalogCount> = [
            (
                "memory".to_owned(),
                CatalogCount::AtLeast { at_least: 10_000 },
            ),
            ("term".to_owned(), CatalogCount::Exact(2)),
            ("term_mapping".to_owned(), CatalogCount::Exact(3)),
            ("skill".to_owned(), CatalogCount::Exact(0)),
        ]
        .into_iter()
        .collect();
        assert_eq!(tab_count(&counts, TABS[8].2), (5, false));
        assert_eq!(tab_count(&counts, TABS[5].2), (0, false));
        let (all, capped) = tab_count(&counts, TABS[0].2);
        assert_eq!(count_word(all, capped), "10,005+");
        assert_eq!(count_word(10_000, true), "10,000+");
        assert_eq!(count_word(999, false), "999");
    }
}
