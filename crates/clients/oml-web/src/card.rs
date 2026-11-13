//! One memory as one card, drawn by one function on every page that shows
//! a memory (the design system's `MemoryCard` and `ReviewCard`; ADR-0045
//! d22, d32; ADR-0056 d9; ADR-0057 d2, d5; ADR-0059 d2, d4; ADR-0074 d17;
//! ADR-0077 d2): the Memories rows of both pages, the Review lists of both
//! pages, the served review item and the one-memory page of both pages.
//!
//! A card has two densities. `row` is an `article.oml-mc` whose
//! `details.mc-d` holds only the head line in its `<summary>` (the state,
//! or in Review the kind of decision, the markers, and the title or the
//! text clamped); the head's facts follow the `details`, outside the
//! summary, and stay visible; the body follows them and shows only while
//! the `details` is open, by a sibling rule, with no script. An open row is
//! the design's `expanded` density. `page` draws the head, the facts and the
//! body whole, then the ids. The row's facts are the page's first facts, in
//! one order and one wording:
//!
//! 1. state (or decision), `stale`, a kind that is not `memory`, a format
//!    that is not prose; then the title and the text;
//! 2. the author, by the name the read serves; on the admin side, whose
//!    read serves the key's holder, its kind after it; on the person's
//!    side no kind, since the read serves none it can stand behind
//!    (`provenance` is `agent` for every key); and `same user` where the
//!    page knows it; `in` the workarea's reference; `claim` key and version, `· tip`;
//!    `replaces vN` or `retires vN`; `from` the origin kind; `scan`;
//!    `written`, the instant to the second;
//! 3. opened, the same list on every surface (the `MemoryCard` order): the
//!    format and what it will be found by, the guardrail's load warning,
//!    the change against the replaced version (page density only),
//!    replaces or retires as a link, derives from, every scan finding, the
//!    source with its last check, valid time, the approval as ADR-0077
//!    words it and never who, where it is served, and the way in; the
//!    Memories row keeps its provenance timeline above that list (design
//!    notes row 12, not decided);
//! 4. page: the id, the kind with its format and tokens, the text's digest,
//!    the attachments.
//!
//! A slot whose fact the read does not serve is left out, never guessed.

use std::fmt::Write as _;

use oml_page::web::{
    ADMIN_MEMORIES_PATH, ADMIN_MEMORY_PATH, USER_MEMORIES_PATH, USER_MEMORY_PATH, attr, glyph_svg,
    short, state_tag, text,
};
use oml_sdk::{AdminMemory, AdminRead, Anchor, Attachment, Memory, ScanFinding, SourceChecked};

use crate::keys::instant;
use crate::picker::Roots;
use serde_json::{Value, json};

/// The states the admin read names a version by.
const ADMIN_STATES: &[&str] = &["held", "promoted", "rejected", "superseded", "demoted"];

/// An anchor as one line for a reviewer (ADR-0057 d2): the repository, the
/// commit's first 12 digits, the path and the fragment, or the page;
/// unescaped, `None` for none.
pub(crate) fn anchor_text(anchor: Option<&Anchor>) -> Option<String> {
    let a = anchor?;
    let fragment = a
        .fragment
        .as_deref()
        .map_or_else(String::new, |f| format!("#{f}"));
    match (&a.repo, &a.uri) {
        (Some(repo), _) => {
            let rev = a.rev.as_deref().unwrap_or_default();
            Some(format!(
                "{repo} @ {} {}{fragment}",
                rev.chars().take(12).collect::<String>(),
                a.path.as_deref().unwrap_or_default()
            ))
        }
        (None, Some(uri)) => Some(format!("{uri}{fragment}")),
        (None, None) => None,
    }
}

/// A version's valid time as markup, each end an instant to the second in
/// UTC (`crate::keys::instant`), `None` when it declares none.
pub(crate) fn valid_html(from: Option<&str>, until: Option<&str>) -> Option<String> {
    (from.is_some() || until.is_some()).then(|| {
        format!(
            "from {}, {}",
            from.map_or_else(|| "any time".to_owned(), instant),
            until.map_or_else(|| "open".to_owned(), |u| format!("until {}", instant(u)))
        )
    })
}

/// A version's valid time as one line, unescaped, `None` when it declares
/// none: the JSON route's.
pub(crate) fn valid_text(from: Option<&str>, until: Option<&str>) -> Option<String> {
    (from.is_some() || until.is_some()).then(|| {
        format!(
            "from {}, {}",
            from.unwrap_or("any time"),
            until.map_or_else(|| "open".to_owned(), |u| format!("until {u}"))
        )
    })
}

pub(crate) fn is_prose(media_type: &str) -> bool {
    matches!(media_type, "text/plain" | "text/markdown")
}

/// The word a card names a key's holder by.
pub(crate) fn kind_word(held_by: &str) -> &'static str {
    match held_by {
        "human" => "person",
        "agent" => "agent",
        _ => "service",
    }
}

pub(crate) fn day(at: &str) -> String {
    at.chars().take(10).collect()
}

/// A served version's state as the card names it: a promoted version that
/// is no longer its claim's tip is `superseded` (ADR-0045 d21).
pub(crate) fn row_state(state: &str, tip: bool) -> &'static str {
    match state {
        "held" => "held",
        "promoted" if tip => "promoted",
        "promoted" => "superseded",
        "rejected" => "rejected",
        _ => "demoted",
    }
}

/// The person's own keys: `(id, name, kind)`, which the Memories text
/// search reads an author's name by.
pub(crate) type Kinds = Vec<(String, Option<String>, &'static str)>;

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
/// annotation (ADR-0074 d17).
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

/// The findings block (ADR-0074 d17): one line per stored finding, refuse
/// and hold before annotate, each its verdict, `class · rule`, and where:
/// the field and line, the view the rule read when it was not the text as
/// written, and who found it when a rescan did. Never the matched text. A
/// version with no list shows its single stored finding as stored; a
/// version with neither shows nothing.
fn findings_block(findings: Option<&[ScanFinding]>, single: Option<&str>) -> String {
    let mut lines: Vec<(u8, String)> = Vec::new();
    match findings {
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
            if let Some(stored) = single {
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

/// The head's scan fact (ADR-0074 d17): the count by verdict, strongest
/// first, `1 hold · 2 annotate`; the single stored finding where the read
/// serves no list; `no findings` otherwise.
fn scan_fact(findings: Option<&[ScanFinding]>, single: Option<&str>) -> String {
    let mut counts: Vec<(u8, &str, usize)> = Vec::new();
    match findings {
        Some(list) if !list.is_empty() => {
            for f in list {
                let rank = verdict_rank(&f.verdict);
                match counts.iter_mut().find(|(_, v, _)| *v == f.verdict) {
                    Some(c) => c.2 += 1,
                    None => counts.push((rank, f.verdict.as_str(), 1)),
                }
            }
        }
        _ => return single.map_or_else(|| "no findings".to_owned(), text),
    }
    counts.sort_by_key(|(rank, _, _)| *rank);
    counts
        .iter()
        .map(|(_, v, n)| format!("{n} {}", text(v)))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The kind of reviewer that approved a version, as the card words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Approver {
    kind: &'static str,
    /// Whether the reviewing key acts for the author's person.
    same_user: bool,
}

/// What a card's density draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Density {
    /// A list's row: the head line in the summary, the facts beside it, the
    /// body while it is open.
    Row,
    /// The one-memory page: everything, then the ids.
    Page,
}

/// How a card is drawn, beside the memory itself.
#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    pub(crate) density: Density,
    /// Which side's lists and pages the card's links go to.
    pub(crate) admin: bool,
    /// A row drawn open.
    pub(crate) open: bool,
    /// Review: the kind of decision leads the head in place of the state.
    pub(crate) decision: bool,
    /// The Memories row: the version bar and the provenance timeline, kept
    /// until design notes row 12 is decided.
    pub(crate) timeline: bool,
    /// The way in to the memory's own page; never on a held row a reviewer
    /// reads, whose own page is its author's.
    pub(crate) open_link: bool,
    /// A Mermaid claim's frame ([`crate::render::drawing`]), under its
    /// source, at page density.
    pub(crate) drawing: &'a str,
    /// The change against the replaced version ([`change`]), page density.
    pub(crate) change: &'a str,
    /// The review strip, after the body.
    pub(crate) strip: &'a str,
    /// The kinds of top label the page knows, which name the workarea by
    /// its reference (design notes row 20).
    pub(crate) roots: &'a Roots,
}

impl<'a> View<'a> {
    /// A list's row, closed, on `admin`'s side.
    pub(crate) const fn row(admin: bool, roots: &'a Roots) -> Self {
        Self {
            roots,
            density: Density::Row,
            admin,
            open: false,
            decision: false,
            timeline: false,
            open_link: true,
            drawing: "",
            change: "",
            strip: "",
        }
    }

    /// The one-memory page, on `admin`'s side.
    pub(crate) const fn page(admin: bool, roots: &'a Roots) -> Self {
        Self {
            density: Density::Page,
            ..Self::row(admin, roots)
        }
    }

    const fn list_path(&self) -> &'static str {
        if self.admin {
            ADMIN_MEMORIES_PATH
        } else {
            USER_MEMORIES_PATH
        }
    }

    const fn memory_path(&self) -> &'static str {
        if self.admin {
            ADMIN_MEMORY_PATH
        } else {
            USER_MEMORY_PATH
        }
    }
}

/// One version of one claim, with every fact a card may show; the JSON
/// route serves the row's own subset (ADR-0045 d22, d25).
pub(crate) struct Card {
    id: String,
    state: &'static str,
    workarea: String,
    version: u32,
    tip: bool,
    title: Option<String>,
    body: String,
    prose: bool,
    claim_key: String,
    /// `None` where the card shows the shared area (ADR-0044 d9).
    author: Option<String>,
    /// `agent`, `person` or `service`: the admin read's `held_by`, the one
    /// read that serves the key's holder; `None` on the person's side.
    author_kind: Option<&'static str>,
    created: String,
    served: Option<String>,
    scan: Option<String>,
    findings: Option<Vec<ScanFinding>>,
    /// When the approving verdict was given, and the kind of key that gave
    /// it: never who (ADR-0077 d2, d4).
    approved: Option<String>,
    approver: Option<Approver>,
    anchor: Option<String>,
    /// The media type, where it is not prose.
    format: Option<String>,
    valid: Option<String>,
    valid_from: Option<String>,
    valid_until: Option<String>,
    kind: Option<String>,
    origin: Option<String>,
    stale: Option<String>,
    checked: Option<SourceChecked>,
    /// The version a successor replaces: its id and, where the read names
    /// it, its version.
    replaces: Option<(String, Option<i64>)>,
    replaces_valid_until: Option<String>,
    retirement: bool,
    derives_from: Vec<String>,
    load_warning: Option<String>,
    search_text: Option<String>,
    tokens: Option<u64>,
    attachments: Vec<Attachment>,
    digest: Option<String>,
    /// The held listing's mark: the author is one of the reviewer's
    /// person's keys (ADR-0055 d7).
    same_user: bool,
}

impl Card {
    /// A memory as a person's key reads it. The author is the principal
    /// the read serves, with no kind: the read's `provenance` is `agent`
    /// for every key, so a kind word here would be a guess, and the same
    /// memory would read one way on Memories and another in Review.
    pub(crate) fn of_memory(m: &Memory) -> Self {
        let format = m.format.clone().unwrap_or_else(|| "text/plain".to_owned());
        let prose = is_prose(&format);
        Self {
            author_kind: None,
            created: m.created_at.clone(),
            served: m.promoted_at.clone(),
            scan: m.scan_finding.clone(),
            findings: m.scan_findings.clone(),
            approved: m.approved_at.clone(),
            approver: m.approver.as_ref().map(|a| Approver {
                kind: kind_word(&a.held_by),
                same_user: a.same_user,
            }),
            id: m.artifact_id.clone(),
            state: row_state(&m.state, m.tip),
            workarea: m.workarea_path.clone().unwrap_or_default(),
            version: m.version,
            tip: m.tip,
            title: m.title.clone(),
            body: m.text.clone().unwrap_or_default(),
            prose,
            claim_key: m.claim_key.clone(),
            author: m.principal.clone(),
            anchor: anchor_text(m.anchor.as_ref()),
            format: (!prose).then_some(format),
            valid: valid_text(m.valid_from.as_deref(), m.valid_until.as_deref()),
            valid_from: m.valid_from.clone(),
            valid_until: m.valid_until.clone(),
            kind: m.kind.clone(),
            origin: m.origin.as_ref().map(|o| o.kind.clone()),
            stale: m.stale.clone(),
            checked: m.source_checked.clone(),
            replaces: m
                .replaces
                .as_ref()
                .map(|r| (r.artifact_id.clone(), Some(r.version))),
            replaces_valid_until: m.replaces_valid_until.clone(),
            retirement: m.retirement,
            derives_from: m.derives_from.clone(),
            load_warning: m.load_warning.clone(),
            search_text: m.search_text.clone(),
            tokens: m.tokens,
            attachments: m.attachments.clone(),
            digest: m.plaintext_digest.clone(),
            same_user: false,
        }
    }

    /// A held row as the review listing serves it, with its `same_user`
    /// mark (ADR-0055 d2, d7).
    pub(crate) fn of_held(m: &Memory, same_user: bool) -> Self {
        Self {
            same_user,
            ..Self::of_memory(m)
        }
    }

    /// A memory as the admin search serves it (ADR-0045 d21, d24).
    pub(crate) fn of_admin(m: &AdminMemory) -> Self {
        let format = m.format.clone().unwrap_or_else(|| "text/plain".to_owned());
        let prose = is_prose(&format);
        let key = m.key.clone().unwrap_or_default();
        Self {
            author_kind: m.held_by.as_deref().map(kind_word),
            created: m.created_at.clone(),
            served: m.promoted_at.clone(),
            scan: m.scan_finding.clone(),
            findings: None,
            // The admin search serves no approval: the line says approved
            // and nothing of who.
            approved: None,
            approver: None,
            id: m.artifact_id.clone(),
            state: admin_state(&m.state),
            workarea: m.workarea_path.clone(),
            version: m.version,
            tip: m.state == "promoted",
            title: m.title.clone(),
            body: m.text.clone(),
            prose,
            claim_key: m.claim_key.clone(),
            author: Some(match &m.user {
                Some(user) => format!("{key} ({user})"),
                None => key,
            }),
            anchor: anchor_text(m.anchor.as_ref()),
            format: (!prose).then_some(format),
            valid: valid_text(m.valid_from.as_deref(), m.valid_until.as_deref()),
            valid_from: m.valid_from.clone(),
            valid_until: m.valid_until.clone(),
            kind: None,
            origin: None,
            stale: None,
            checked: None,
            replaces: None,
            replaces_valid_until: None,
            retirement: false,
            derives_from: Vec::new(),
            load_warning: None,
            search_text: None,
            tokens: None,
            attachments: Vec::new(),
            digest: None,
            same_user: false,
        }
    }

    /// A memory as the admin read serves it (ADR-0045 d21): the search's
    /// facts, its lineage and digest; a key with no name is named by its id
    /// (#253).
    pub(crate) fn of_admin_read(m: &AdminRead) -> Self {
        let format = m.format.clone().unwrap_or_else(|| "text/plain".to_owned());
        let prose = is_prose(&format);
        let key = m.key.as_deref().or(m.key_id.as_deref()).unwrap_or_default();
        Self {
            author_kind: m.held_by.as_deref().map(kind_word),
            created: m.created_at.clone(),
            served: m.promoted_at.clone(),
            scan: m.scan_finding.clone(),
            findings: None,
            // When it was approved, from the last approving verdict the
            // read lists; the kind of key is not served, so the line says
            // when and nothing of who (ADR-0077 d2).
            approved: (!matches!(m.state.as_str(), "held" | "rejected"))
                .then(|| {
                    m.verdicts
                        .iter()
                        .rev()
                        .find(|v| v.verdict == "approve")
                        .map(|v| v.at.clone())
                })
                .flatten(),
            approver: None,
            id: m.artifact_id.clone(),
            state: admin_state(&m.state),
            workarea: m.workarea_path.clone(),
            version: m.version,
            tip: m.state == "promoted",
            title: m.title.clone(),
            body: m.text.clone(),
            prose,
            claim_key: m.claim_key.clone(),
            author: Some(match &m.user {
                Some(user) => format!("{key} ({user})"),
                None => key.to_owned(),
            }),
            anchor: anchor_text(m.anchor.as_ref()),
            format: (!prose).then_some(format),
            valid: valid_text(m.valid_from.as_deref(), m.valid_until.as_deref()),
            valid_from: m.valid_from.clone(),
            valid_until: m.valid_until.clone(),
            kind: m.kind.clone(),
            origin: None,
            stale: None,
            checked: None,
            replaces: m.replaces.clone().map(|id| (id, None)),
            replaces_valid_until: None,
            retirement: false,
            derives_from: m.derives_from.clone(),
            load_warning: None,
            search_text: None,
            tokens: None,
            attachments: Vec::new(),
            digest: m.plaintext_digest.clone(),
            same_user: false,
        }
    }

    /// The card with the version it replaces named by `version`, where a
    /// read the page made says it (the admin page's "Show the change").
    pub(crate) fn with_replaced_version(mut self, version: Option<i64>) -> Self {
        if let (Some((_, known)), Some(v)) = (&mut self.replaces, version) {
            *known = Some(v);
        }
        self
    }

    /// The text of the version, as served.
    pub(crate) fn body(&self) -> &str {
        &self.body
    }

    /// The version this one replaces: its id and version, where served.
    pub(crate) fn replaces(&self) -> Option<(&str, Option<i64>)> {
        self.replaces.as_ref().map(|(id, v)| (id.as_str(), *v))
    }

    pub(crate) const fn version(&self) -> u32 {
        self.version
    }

    pub(crate) const fn retirement(&self) -> bool {
        self.retirement
    }

    /// Whether the version replaces another, by the item's own facts: a
    /// retirement, a version that names what it replaces, or one after the
    /// first, whose `replaces` a key that cannot read the replaced version
    /// is not served.
    pub(crate) const fn successor(&self) -> bool {
        self.retirement || self.replaces.is_some() || self.version > 1
    }

    /// The authoring principal, or on the admin side the key and its user.
    pub(crate) fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// How many ancestors the reader can read (ADR-0025 d3).
    pub(crate) fn derives_count(&self) -> usize {
        self.derives_from.len()
    }

    /// On a held successor, the instant approving it ends the version it
    /// replaces at (ADR-0059 d4).
    pub(crate) fn replaces_valid_until(&self) -> Option<&str> {
        self.replaces_valid_until.as_deref()
    }

    /// The workarea's path, as the read serves it.
    pub(crate) fn workarea(&self) -> &str {
        &self.workarea
    }

    /// The workarea's reference, by the kinds of top label `roots` knows.
    pub(crate) fn reference(&self, roots: &Roots) -> String {
        roots.reference(&self.workarea)
    }

    pub(crate) fn json(&self) -> Value {
        json!({
            "artifact_id": self.id,
            "state": self.state,
            "workarea": self.workarea,
            "version": self.version,
            "title": self.title,
            "text": self.body,
            "claim_key": self.claim_key,
            "approved_at": self.approved,
            "approver": self.approver.map(|a| json!({
                "kind": a.kind,
                "same_user": a.same_user,
            })),
            "author": self.author,
            "author_kind": self.author_kind,
            "anchor": self.anchor,
            "format": self.format,
            "valid": self.valid,
        })
    }

    /// What the review step says of an approved version: the day it was
    /// approved and whether a person's, an agent's or a service's key
    /// approved it, and that a person's own review of their agent's write is
    /// one. The line names the key's kind: a key no one attested is stored
    /// as an agent's (`key.issued`'s `held_by`), so "a reviewer agent" would
    /// say more than is known. A field the API does not serve, an
    /// unreviewed or a `universe` row's, adds nothing, so the line never
    /// says more than the API did (ADR-0077 d2, d4).
    fn approval_line(&self) -> String {
        let mut line = String::from("approved");
        if let Some(approver) = self.approver {
            let who = match approver.kind {
                "person" => "a person's key",
                "agent" => "an agent's key",
                _ => "a service's key",
            };
            let _ = write!(line, " by {who}");
        }
        if let Some(at) = &self.approved {
            let _ = write!(line, " on {}", text(&day(at)));
        }
        if self.approver.is_some_and(|a| a.same_user) {
            line.push_str(" · self-review: the same person's keys wrote it and approved it");
        }
        line
    }

    /// The review step's sentence, by state.
    fn review_line(&self) -> String {
        match self.state {
            "held" => "held until someone else reviews it".to_owned(),
            "rejected" => "rejected by a key outside its lineage".to_owned(),
            _ => self.approval_line(),
        }
    }

    /// Where the version is served, by reference (design notes row 20).
    fn served_line(&self, roots: &Roots) -> String {
        match (self.state, &self.served) {
            ("rejected", _) => "never: the text is kept".to_owned(),
            (_, Some(at)) => format!(
                "to readers of {} · {}",
                text(&self.reference(roots)),
                day(at)
            ),
            _ => "not yet".to_owned(),
        }
    }

    /// The kind of decision a held version asks for (ReviewCard): a first
    /// version, an edit of the version it replaces, or a retirement of it.
    pub(crate) fn decision(&self) -> String {
        let of = self
            .replaces
            .as_ref()
            .and_then(|(_, v)| *v)
            .map_or_else(String::new, |v| {
                format!(r#" <span class="of">of v{v}</span>"#)
            });
        let word = match (self.retirement, self.successor()) {
            (true, _) => "Retirement",
            (false, true) => "Edit",
            (false, false) => return r#"<span class="oml-decision">New memory</span>"#.to_owned(),
        };
        format!(r#"<span class="oml-decision">{word}{of}</span>"#)
    }

    fn quiet(&self) -> bool {
        matches!(self.state, "superseded" | "rejected" | "demoted")
    }

    /// The head line's markers: the state or the decision, `stale`, a kind
    /// that is not `memory`, a format that is not prose.
    fn markers(&self, view: &View<'_>) -> String {
        let mut out = if view.decision {
            self.decision()
        } else {
            state_tag(self.state)
        };
        if self.stale.is_some() {
            out.push_str(&state_tag("stale"));
        }
        if let Some(kind) = self.kind.as_deref().filter(|k| *k != "memory") {
            let _ = write!(
                out,
                r#"<span class="tag oml-chip is-quiet">{}</span>"#,
                text(kind)
            );
        }
        if let Some(format) = &self.format {
            let _ = write!(
                out,
                r#"<span class="tag oml-chip is-quiet">{}</span>"#,
                text(format)
            );
        }
        out
    }

    /// The title, then the text.
    fn head_text(&self) -> String {
        let mono = if self.prose { "" } else { " mono" };
        let title = self
            .title
            .as_deref()
            .map_or_else(String::new, |t| format!("<b>{}</b>\n", text(t)));
        format!(
            r#"<span class="mc-tx{mono}">{title}{}</span>"#,
            text(&self.body)
        )
    }

    /// The author's kind word and glyph: `shared` for a row whose author
    /// is not served, the admin read's kind where it serves one, and none
    /// on the person's side, whose read serves no kind it can stand behind.
    fn actor_kind(&self) -> Option<(&'static str, &'static str)> {
        match (self.author.is_some(), self.author_kind) {
            (false, _) => Some(("shared", glyph_svg("shared"))),
            (true, None) => None,
            (true, Some(kind)) => Some((
                kind,
                glyph_svg(if kind == "person" { "person" } else { "agent" }),
            )),
        }
    }

    /// The author as one tag: the glyph and the kind where known, and the
    /// name as served.
    fn actor(&self) -> String {
        let (glyph, kind) = self.actor_kind().map_or(("", String::new()), |(k, g)| {
            (g, format!(r#"<span class="kind">{k}</span>"#))
        });
        format!(
            r#"<span class="oml-actor">{glyph}<span class="who">{}</span>{kind}</span>"#,
            self.who()
        )
    }

    fn who(&self) -> String {
        self.author
            .as_deref()
            .map_or_else(|| "the shared area".to_owned(), text)
    }

    /// The head's facts, outside the summary, in their one order.
    fn facts(&self, view: &View<'_>) -> String {
        let mut out = format!(r#"<ul class="mc-facts"><li>{}"#, self.actor());
        if self.same_user {
            out.push_str(r#" <span class="tag oml-chip is-quiet">same user</span>"#);
        }
        out.push_str("</li>");
        let reference = self.reference(view.roots);
        let _ = write!(
            out,
            r#"<li><span class="k">in</span><a class="v" href="{}?workarea={}">{}</a></li>"#,
            view.list_path(),
            attr(&oml_sdk::encode(&reference)),
            text(&reference)
        );
        let tip = if self.tip && self.state == "promoted" {
            " · tip"
        } else {
            ""
        };
        let bar = if view.timeline {
            let segment = match self.state {
                "demoted" => "retired",
                other => other,
            };
            format!(
                r#"<span class="vb"><ol aria-label="Versions"><li aria-current="true"><i class="{segment}"></i></li></ol></span>"#
            )
        } else {
            String::new()
        };
        let _ = write!(
            out,
            r#"<li><span class="k">claim</span><span class="v">{} · v{}{tip}</span>{bar}</li>"#,
            short(&self.claim_key),
            self.version,
        );
        if let Some((_, Some(v))) = &self.replaces {
            let word = if self.retirement {
                "retires"
            } else {
                "replaces"
            };
            let _ = write!(
                out,
                r#"<li><span class="k">{word}</span><span class="v">v{v}</span></li>"#
            );
        }
        if let Some(origin) = &self.origin {
            let _ = write!(
                out,
                r#"<li><span class="k">from</span><span class="v">{}</span></li>"#,
                text(origin)
            );
        }
        let _ = write!(
            out,
            r#"<li><span class="k">scan</span><span class="v">{}</span></li><li><span class="k">written</span><span class="v t">{}</span></li></ul>"#,
            scan_fact(self.findings.as_deref(), self.scan.as_deref()),
            instant(&self.created)
        );
        out
    }

    /// The source fact: the origin kind, the anchor, and the last source
    /// check (ADR-0057 d2, d5); `None` when the read serves neither kind
    /// nor anchor.
    fn source(&self) -> Option<String> {
        let mut parts = Vec::new();
        match (&self.origin, &self.anchor) {
            (None, None) => return None,
            (Some(o), None) => parts.push(format!("<span>{}, no anchor</span>", text(o))),
            (origin, Some(a)) => {
                if let Some(o) = origin {
                    parts.push(format!("<span>{}</span>", text(o)));
                }
                parts.push(format!(r#"<span class="id">{}</span>"#, text(a)));
            }
        }
        if self.anchor.is_some() {
            if self.state == "held" {
                parts.push("<span>not checked while held</span>".to_owned());
            } else if let Some(c) = &self.checked {
                let mut line = format!(
                    "{}: {}",
                    c.rev.chars().take(12).collect::<String>(),
                    c.result
                );
                if let Some(to) = &c.moved_to {
                    let _ = write!(
                        line,
                        " to {}{}",
                        to.path,
                        to.fragment
                            .as_deref()
                            .map_or_else(String::new, |f| format!("#{f}"))
                    );
                }
                parts.push(format!(
                    "<span>checked {} at {}</span>",
                    instant(&c.at),
                    text(&line)
                ));
            }
        }
        Some(parts.join(" "))
    }

    /// The opened facts, in their one order.
    fn body_facts(&self, view: &View<'_>) -> String {
        let link = |id: &str, shown: &str| {
            format!(
                r#"<a class="id" href="{}?id={}">{}</a>"#,
                view.memory_path(),
                attr(&oml_sdk::encode(id)),
                text(shown)
            )
        };
        let mut out = String::from(r#"<dl class="oml-facts">"#);
        let mut fact = |label: &str, value: &str| {
            let _ = write!(out, "<dt>{label}</dt><dd>{value}</dd>");
        };
        if let Some(format) = &self.format {
            fact(
                "Format",
                &format!(r#"<span class="id">{}</span>"#, text(format)),
            );
        }
        if let Some(found) = &self.search_text {
            fact(
                "Found by",
                &format!(r#"<span class="mono">{}</span>"#, text(found)),
            );
        }
        if let Some((id, version)) = &self.replaces {
            // The admin read names the replaced version by id alone; the
            // claim is the same, so the link says so.
            let shown = version.map_or_else(
                || format!("{} · the version it replaces", self.claim_key),
                |v| format!("{} · v{v}", self.claim_key),
            );
            fact(
                if self.retirement {
                    "Retires"
                } else {
                    "Replaces"
                },
                &link(id, &shown),
            );
        }
        // A successor derives from the version it replaces, which Replaces
        // already names.
        let replaced = self.replaces.as_ref().map(|(id, _)| id.as_str());
        let links: Vec<String> = self
            .derives_from
            .iter()
            .filter(|id| Some(id.as_str()) != replaced)
            .map(|id| link(id, id))
            .collect();
        if !links.is_empty() {
            fact("Derives from", &links.join("<br>"));
        }
        let findings = findings_block(self.findings.as_deref(), self.scan.as_deref());
        if !findings.is_empty() {
            fact("Scan", &findings);
        }
        if let Some(source) = self.source() {
            fact("Source", &source);
        }
        let mut valid = valid_html(self.valid_from.as_deref(), self.valid_until.as_deref())
            .unwrap_or_else(|| "none declared".to_owned());
        if let (Some(end), Some((_, Some(v)))) = (&self.replaces_valid_until, &self.replaces) {
            let _ = write!(valid, "; approving ends version {v} at {}", instant(end));
        }
        fact("Valid", &valid);
        fact("Approval", &self.review_line());
        fact("Served", &self.served_line(view.roots));
        if view.open_link {
            fact(
                "Open",
                &format!(
                    r#"<a href="{}?id={}">this memory</a>"#,
                    view.memory_path(),
                    attr(&self.id)
                ),
            );
        }
        if view.density == Density::Page {
            fact(
                "Id",
                &format!(r#"<span class="id">{}</span>"#, text(&self.id)),
            );
            let mut kind = self.kind.clone().unwrap_or_else(|| "memory".to_owned());
            let _ = write!(
                kind,
                " · {}",
                self.format.as_deref().unwrap_or("text/plain")
            );
            if let Some(tokens) = self.tokens {
                let _ = write!(kind, " · {tokens} tokens");
            }
            fact("Kind", &text(&kind));
            if let Some(digest) = &self.digest {
                fact(
                    "Text digest",
                    &format!(r#"<span class="id">{}</span>"#, text(digest)),
                );
            }
            if !self.attachments.is_empty() {
                let listed: Vec<String> = self
                    .attachments
                    .iter()
                    .map(|a| {
                        let mut line = a.media_type.clone().unwrap_or_else(|| "file".to_owned());
                        if let Some(size) = a.size_bytes {
                            let _ = write!(line, " · {size} bytes");
                        }
                        text(&line)
                    })
                    .collect();
                fact("Attachments", &listed.join("<br>"));
            }
        }
        out.push_str("</dl>");
        out
    }

    /// The provenance timeline the Memories row keeps (ADR-0045 d32;
    /// design notes row 12, not decided).
    fn timeline(&self, roots: &Roots) -> String {
        let reviewed = self.state != "held";
        let served = matches!(self.state, "promoted" | "superseded" | "demoted");
        let rejected = self.state == "rejected";
        let (kind, actor_glyph) = self
            .actor_kind()
            .map_or((String::new(), ""), |(k, g)| (format!(" · {k}"), g));
        let step = |done: bool| if done { "" } else { " class=\"wait\"" };
        format!(
            r#"<div class="oml-timeline-wrap"><ol class="oml-timeline" aria-label="Provenance"><li><span class="c-dim">{actor_glyph}</span><span class="step">Written</span><span><span class="who">{}</span>{kind}</span><span>{}</span></li><li><span class="c-held">{}</span><span class="step">Held</span><span>scan: {}</span></li><li{}><span class="{}">{}</span><span class="step">{}</span><span>{}</span></li><li{}><span class="{}">{}</span><span class="step">Served</span><span>{}</span></li></ol></div>"#,
            self.who(),
            day(&self.created),
            glyph_svg("held"),
            self.scan
                .as_deref()
                .map_or_else(|| "no findings".to_owned(), text),
            step(reviewed),
            if rejected {
                "c-rejected"
            } else if reviewed {
                "c-dim"
            } else {
                "c-faint"
            },
            glyph_svg("person"),
            if rejected { "Rejected" } else { "Reviewed" },
            if reviewed {
                self.review_line()
            } else {
                "not yet".to_owned()
            },
            step(served),
            if served { "c-promoted" } else { "c-faint" },
            glyph_svg("promoted"),
            self.served_line(roots),
        )
    }

    /// The card, at `view`'s density.
    pub(crate) fn render(&self, view: &View<'_>) -> String {
        let reviewed = self.state != "held";
        let kind = self
            .actor_kind()
            .map_or_else(String::new, |(k, _)| format!(r#" data-author-kind="{k}""#));
        let page = view.density == Density::Page;
        let mut out = format!(
            r#"<article class="oml-mc{}{}" id="m-{}"{kind} data-reviewed="{reviewed}" aria-label="{} v{}">"#,
            if page { " page" } else { "" },
            if self.quiet() { " quiet" } else { "" },
            attr(&self.id),
            attr(&self.claim_key),
            self.version
        );
        let markers = self.markers(view);
        if page {
            let _ = write!(
                out,
                r#"<div class="mc-head"><span class="mc-top">{markers}</span>{}</div>"#,
                self.head_text()
            );
        } else {
            let _ = write!(
                out,
                r#"<details class="mc-d"{}><summary><span class="mc-top">{markers}</span><span class="mc-more"><span class="cl">Open</span><span class="op">Close</span></span>{}</summary></details>"#,
                if view.open { " open" } else { "" },
                self.head_text()
            );
        }
        out.push_str(&self.facts(view));
        out.push_str(r#"<div class="mc-body">"#);
        if let Some(warning) = &self.load_warning {
            let _ = write!(out, r#"<p class="lede">Guardrail: {}.</p>"#, text(warning));
        }
        if page {
            out.push_str(view.drawing);
            out.push_str(view.change);
        }
        if view.timeline {
            out.push_str(&self.timeline(view.roots));
        }
        out.push_str(&self.body_facts(view));
        out.push_str("</div>");
        out.push_str(view.strip);
        out.push_str("</article>");
        out
    }

    /// The Memories row (ADR-0045 d22, d32), as a list item, with the
    /// version bar and the provenance timeline it keeps; `memory_page` is
    /// the side's one-memory page.
    pub(crate) fn html(&self, memory_page: &str, roots: &Roots) -> String {
        let view = View {
            timeline: true,
            ..View::row(memory_page == ADMIN_MEMORY_PATH, roots)
        };
        format!("<li>{}</li>", self.render(&view))
    }
}

/// A state the admin read names, from its closed set.
fn admin_state(state: &str) -> &'static str {
    ADMIN_STATES
        .iter()
        .find(|s| **s == state)
        .copied()
        .unwrap_or("demoted")
}

/// The words of `s`, split on whitespace.
fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// The longest common subsequence of two word lists, as a mark per word of
/// each: `true` where the word is kept.
fn kept(was: &[&str], now: &[&str]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (was.len(), now.len());
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if was[i] == now[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut a, mut b) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if was[i] == now[j] {
            a[i] = true;
            b[j] = true;
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (a, b)
}

/// The most words the change compares; past it the two texts are shown
/// whole without marks, so a page never spends quadratic time on a long
/// text. A constant of the build, unmeasured.
const CHANGE_WORDS_MAX: usize = 2_000;

/// One side of the change: each word escaped, a dropped one struck through
/// (`del`), an added one underlined (`ins`), so the change does not rest
/// on colour.
fn side(list: &[&str], marks: &[bool], tag: &str) -> String {
    list.iter()
        .zip(marks)
        .map(|(w, keep)| {
            if *keep {
                text(w)
            } else {
                format!("<{tag}>{}</{tag}>", text(w))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The change against the version a successor replaces, at page density
/// (MemoryCard): a `<details>` that names its baseline and opens on two
/// named blocks, "Was" and "Now"; for a retirement, the reason beside what
/// stops being served. The comparison is the console's own: the longest
/// common subsequence of whitespace-split words, no model.
pub(crate) fn change(
    was_version: i64,
    was: &str,
    now_version: u32,
    now: &str,
    retirement: bool,
) -> String {
    if retirement {
        return format!(
            r#"<details class="oml-diff" open><summary>What stops being served</summary><div class="sides"><div class="side"><p class="label">Reason · this version</p><p class="t">{}</p></div><div class="side"><p class="label">Stops being served · v{was_version}</p><p class="t">{}</p></div></div></details>"#,
            text(now),
            text(was)
        );
    }
    let (a, b) = (words(was), words(now));
    let (ka, kb) = if a.len() + b.len() > CHANGE_WORDS_MAX {
        (vec![true; a.len()], vec![true; b.len()])
    } else {
        kept(&a, &b)
    };
    let removed = ka.iter().filter(|k| !**k).count();
    let added = kb.iter().filter(|k| !**k).count();
    format!(
        r#"<details class="oml-diff" open><summary>Change against v{was_version} <span class="n">baseline: v{was_version} · {removed} words removed, {added} added</span></summary><div class="sides"><div class="side"><p class="label">Was · v{was_version}</p><p class="t">{}</p></div><div class="side"><p class="label">Now · v{now_version}, this version</p><p class="t">{}</p></div></div></details>"#,
        side(&a, &ka, "del"),
        side(&b, &kb, "ins"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_change_strikes_a_removed_word_and_underlines_an_added_one() {
        let block = change(2, "runs on ci-small", 3, "runs on ci-medium now", false);
        assert!(block.contains("<del>ci-small</del>"), "{block}");
        assert!(
            block.contains("<ins>ci-medium</ins> <ins>now</ins>"),
            "{block}"
        );
        assert!(block.contains("1 words removed, 2 added"), "{block}");
        assert!(block.contains("Was · v2") && block.contains("Now · v3, this version"));
        let retired = change(1, "the old wiki", 2, "shut down", true);
        assert!(retired.contains("Stops being served · v1"), "{retired}");
    }

    #[test]
    fn an_instant_reads_to_the_second_in_utc() {
        assert_eq!(
            instant("2026-11-10T08:12:44.123456Z"),
            r#"<time datetime="2026-11-10T08:12:44.123456Z">2026-11-10 08:12:44 UTC</time>"#
        );
        let valid = valid_html(Some("2026-12-20T00:00:00.000000Z"), None).unwrap();
        assert_eq!(
            valid,
            r#"from <time datetime="2026-12-20T00:00:00.000000Z">2026-12-20 00:00:00 UTC</time>, open"#
        );
    }
}
