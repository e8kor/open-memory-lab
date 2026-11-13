//! The Memories list of both pages and its Atlas view (ADR-0045 d22–d32),
//! moved from the lead to paths of their own (ADR-0070 d7 (b)):
//! `/web/memories` and `/web/atlas` on the user page, `/web/admin/memories`
//! and `/web/admin/atlas` on the admin page, and their JSON at
//! `/web/data/memories` and `/web/admin/data/memories`, each with its
//! assertions unchanged but for the path (`tests/web_pages.rs`).
//!
//! The list is server-rendered rows, one page at a time, resumed by the
//! listing's keyset cursor and narrowed by five filters. The user page
//! lists through `GET /v1/memories` with `with_own` as `<user>-web` (d23),
//! or with a text query through recall, its one ranked page; the admin page
//! lists through `POST /v2/admin/memories/search` as `<user>-admin`, one
//! `memory.admin_read` per page, which the API journals (d24). The Atlas
//! reads no memory: `GET /v2/workareas/{w}/tree?counts=true` and the scope's
//! offers (d30). Each row links to the memory's own page, `crate::memory`.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::State;
use axum::response::{IntoResponse as _, Response};
use http::{HeaderMap, StatusCode, Uri};
use oml_page::web::{
    ADMIN_ATLAS_PATH, ADMIN_MEMORIES_PATH, USER_ATLAS_PATH, USER_MEMORIES_PATH, attr, glyph_svg,
    input, select, shell, state_tag, text,
};
use oml_sdk::{AdminMemory, AdminSearch, CountedNode, Memory, MemoryQuery, RecallQuery};
use serde_json::{Value, json};

use crate::atlas::{Counts, Tile, legend, lifebar, svg};
use crate::pages::{Fields, WalkEnd, refused, walk};
use crate::picker::{self, Browse, Chosen, Field, Names};
use crate::session::{PageCalls, Session, call, cookie};
use crate::workareas::{
    Credentials, NodeOffers, Side as ActSide, mode_offers, offers_of, transitions_offers,
};
use crate::{Capacity, SESSION_COOKIE, Web, outcome};

/// A page's rows when the query names no size (ADR-0045 d22).
pub const PAGE_DEFAULT: u32 = 50;
/// The largest page, `api.page_limit_max`'s shipped value.
pub const PAGE_MAX: u32 = 200;
/// Recall's own bound on one answer, the user page's text search.
pub const RECALL_MAX: u32 = 100;
/// The most pages of the person's keys the list reads to name their kinds:
/// 10 pages of 200. Past them the list is refused with
/// `limit.list_too_long`, never shown with kinds missing. A constant of the
/// build, unmeasured (#170).
pub const KEY_PAGES_MAX: usize = 10;

/// The user page's JSON route (ADR-0045 d25).
pub const USER_DATA_PATH: &str = "/web/data/memories";
/// The admin page's JSON route.
pub const ADMIN_DATA_PATH: &str = "/web/admin/data/memories";

/// Which page the list is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    User,
    Admin,
}

impl Side {
    fn memories(self) -> &'static str {
        match self {
            Self::User => USER_MEMORIES_PATH,
            Self::Admin => ADMIN_MEMORIES_PATH,
        }
    }

    fn atlas(self) -> &'static str {
        match self {
            Self::User => USER_ATLAS_PATH,
            Self::Admin => ADMIN_ATLAS_PATH,
        }
    }

    /// Where one memory opens by its id, `?id=`.
    fn memory_page(self) -> &'static str {
        match self {
            Self::User => oml_page::web::USER_MEMORY_PATH,
            Self::Admin => oml_page::web::ADMIN_MEMORY_PATH,
        }
    }

    fn acts(self) -> ActSide {
        match self {
            Self::User => ActSide::User,
            Self::Admin => ActSide::Admin,
        }
    }

    fn lede(self) -> &'static str {
        match self {
            Self::User => {
                "Newest first. A held memory waits for another identity's review. Its author and the agent's signed-in owner can read it; an in-progress workarea also serves it to readers."
            }
            Self::Admin => "Every memory, newest first. Each page read is journalled.",
        }
    }
}

/// A refused field: its name and the message.
type FieldRefusal = (&'static str, String);

/// The Memories view's query, each field checked as the lead checked it
/// (ADR-0045 d22); the API checks an author's grammar, and the picker a
/// workarea's ([`crate::picker`]).
#[derive(Clone)]
struct ListQuery {
    q: Option<String>,
    state: Option<String>,
    workarea: Option<String>,
    origin: Option<String>,
    author: Option<String>,
    cursor: Option<String>,
    limit: u32,
    limit_sent: Option<String>,
}

const STATES: &[&str] = &["held", "promoted", "rejected", "superseded", "demoted"];
const ORIGINS: &[&str] = &["repo", "doc", "code", "lake", "chat", "web", "manual"];

impl ListQuery {
    fn parse(fields: &Fields) -> Result<Self, FieldRefusal> {
        // An empty field is no filter, as a submitted filter form sends it.
        let value = |name: &str| {
            fields
                .get(name)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let limit_sent = value("limit");
        let limit = match &limit_sent {
            None => PAGE_DEFAULT,
            Some(v) => v
                .parse::<u32>()
                .ok()
                .filter(|l| (1..=PAGE_MAX).contains(l))
                .ok_or_else(|| ("limit", format!("limit is between 1 and {PAGE_MAX}")))?,
        };
        let state = value("state");
        if state.as_deref().is_some_and(|s| !STATES.contains(&s)) {
            return Err((
                "state",
                "state is held, promoted, rejected, superseded or demoted".to_owned(),
            ));
        }
        let origin = value("origin");
        if origin.as_deref().is_some_and(|o| !ORIGINS.contains(&o)) {
            return Err((
                "origin",
                "origin is repo, doc, code, lake, chat, web or manual".to_owned(),
            ));
        }
        Ok(Self {
            q: value("q"),
            state,
            workarea: value("workarea"),
            origin,
            author: value("author"),
            cursor: value("cursor"),
            limit,
            limit_sent,
        })
    }

    /// The view's link at `path` with every filter kept and the cursor when
    /// given; escaped for an attribute.
    fn link(&self, path: &str, cursor: Option<&str>) -> String {
        let mut pairs = Vec::new();
        for (name, value) in [
            ("q", self.q.as_deref()),
            ("state", self.state.as_deref()),
            ("workarea", self.workarea.as_deref()),
            ("origin", self.origin.as_deref()),
            ("author", self.author.as_deref()),
            ("limit", self.limit_sent.as_deref()),
            ("cursor", cursor),
        ] {
            if let Some(value) = value {
                pairs.push(format!("{name}={}", attr(&oml_sdk::encode(value))));
            }
        }
        if pairs.is_empty() {
            path.to_owned()
        } else {
            format!("{path}?{}", pairs.join("&amp;"))
        }
    }
}

/// One memory as the row shows it and the JSON route serves it: nothing
/// the row does not show (ADR-0045 d22, d25).
struct Card {
    id: String,
    state: &'static str,
    workarea: String,
    version: u32,
    title: Option<String>,
    body: String,
    prose: bool,
    claim_key: String,
    /// `None` where the row shows the shared area (ADR-0044 d9).
    author: Option<String>,
    /// `agent`, `person` or `service`, the author key's holder, where the
    /// page's key may know it; `None` otherwise.
    author_kind: Option<&'static str>,
    written: String,
    served: Option<String>,
    scan: Option<String>,
    /// When the approving verdict was given, the API's `approved_at` whole,
    /// and the kind of key that gave it, where the API serves them: never
    /// who (ADR-0077 d2, d4). The card shows the day; the JSON route serves
    /// the timestamp under the API's name.
    approved: Option<String>,
    approver: Option<Approver>,
    anchor: Option<String>,
    format: Option<String>,
    valid: Option<String>,
}

/// The kind of reviewer that approved a version, as the page words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Approver {
    kind: &'static str,
    /// Whether the reviewing key acts for the author's person.
    same_user: bool,
}

/// An anchor as one line (ADR-0057 d2), unescaped.
fn anchor_text(anchor: Option<&oml_sdk::Anchor>) -> Option<String> {
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

fn valid_text(from: Option<&str>, until: Option<&str>) -> Option<String> {
    (from.is_some() || until.is_some()).then(|| {
        format!(
            "from {}, {}",
            from.unwrap_or("any time"),
            until.map_or_else(|| "open".to_owned(), |u| format!("until {u}"))
        )
    })
}

fn is_prose(media_type: &str) -> bool {
    matches!(media_type, "text/plain" | "text/markdown")
}

/// The word a row names a key's holder by.
fn kind_word(held_by: &str) -> &'static str {
    match held_by {
        "human" => "person",
        "agent" => "agent",
        _ => "service",
    }
}

fn day(at: &str) -> String {
    at.chars().take(10).collect()
}

/// A served version's state as the admin read names it: a promoted version
/// that is no longer its claim's tip is `superseded` (ADR-0045 d21).
fn row_state(state: &str, tip: bool) -> &'static str {
    match state {
        "held" => "held",
        "promoted" if tip => "promoted",
        "promoted" => "superseded",
        "rejected" => "rejected",
        _ => "demoted",
    }
}

/// The person's own keys: `(id, name, kind)`.
type Kinds = Vec<(String, Option<String>, &'static str)>;

impl Card {
    fn of_memory(m: &Memory, kinds: &Kinds) -> Self {
        let format = m.format.clone().unwrap_or_else(|| "text/plain".to_owned());
        let prose = is_prose(&format);
        Self {
            author_kind: m.principal.as_ref().and_then(|_| {
                let key = m.author_key_id.as_deref()?;
                kinds
                    .iter()
                    .find(|(id, _, _)| id == key)
                    .map(|(_, _, w)| *w)
            }),
            written: day(&m.created_at),
            served: m.promoted_at.as_deref().map(day),
            scan: m.scan_finding.clone(),
            approved: m.approved_at.clone(),
            approver: m.approver.as_ref().map(|a| Approver {
                kind: kind_word(&a.held_by),
                same_user: a.same_user,
            }),
            id: m.artifact_id.clone(),
            state: row_state(&m.state, m.tip),
            workarea: m.workarea_path.clone().unwrap_or_default(),
            version: m.version,
            title: m.title.clone(),
            body: m.text.clone().unwrap_or_default(),
            prose,
            claim_key: m.claim_key.clone(),
            author: m.principal.clone(),
            anchor: anchor_text(m.anchor.as_ref()),
            format: (!prose).then_some(format),
            valid: valid_text(m.valid_from.as_deref(), m.valid_until.as_deref()),
        }
    }

    fn of_admin(m: &AdminMemory) -> Self {
        let format = m.format.clone().unwrap_or_else(|| "text/plain".to_owned());
        let prose = is_prose(&format);
        let key = m.key.clone().unwrap_or_default();
        Self {
            author_kind: m.held_by.as_deref().map(kind_word),
            written: day(&m.created_at),
            served: m.promoted_at.as_deref().map(day),
            scan: m.scan_finding.clone(),
            // The admin search serves no approval: the line says approved
            // and nothing of who.
            approved: None,
            approver: None,
            id: m.artifact_id.clone(),
            state: STATES
                .iter()
                .find(|s| **s == m.state)
                .copied()
                .unwrap_or("demoted"),
            workarea: m.workarea_path.clone(),
            version: m.version,
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
        }
    }

    fn json(&self) -> Value {
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
    /// say more than is known. A field the
    /// API does not serve, an unreviewed or a `universe` row's, adds nothing,
    /// so the line never says more than the API did (ADR-0077 d2, d4).
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

    /// The memory as the design system's Round 3 compact row (ADR-0045
    /// d32; ADR-0069 d3): the summary holds the state and the text,
    /// clamped, then the author's kind, the version and the way in; the
    /// body, closed by default, holds the provenance timeline and the facts.
    fn html(&self, memory_page: &str) -> String {
        let reviewed = self.state != "held";
        let served = matches!(self.state, "promoted" | "superseded" | "demoted");
        let rejected = self.state == "rejected";
        let quiet = matches!(self.state, "superseded" | "rejected" | "demoted");
        let who = self
            .author
            .as_deref()
            .map_or_else(|| "the shared area".to_owned(), text);
        let kind = self.author_kind.unwrap_or("shared");
        let actor_glyph = match self.author_kind {
            Some("person") => glyph_svg("person"),
            Some(_) => glyph_svg("agent"),
            None => glyph_svg("shared"),
        };
        let segment = match self.state {
            "demoted" => "retired",
            other => other,
        };
        let mut out = format!(
            r#"<li><details class="oml-mrow{}" id="m-{}" data-author-kind="{kind}" data-reviewed="{reviewed}"><summary><div class="top">{}<span class="tx">{}</span></div><div class="sub"><span class="oml-actor">{actor_glyph}<span class="who">{who}</span><span class="kind">{kind}</span></span><span class="vb"><ol aria-label="Versions"><li aria-current="true"><i class="{segment}"></i></li></ol>v{}{}</span><span class="more"><span class="w">Provenance</span></span></div></summary><div class="body">"#,
            if quiet { " quiet" } else { "" },
            self.id,
            state_tag(self.state),
            text(self.title.as_deref().unwrap_or(&self.body)),
            self.version,
            if self.state == "promoted" {
                " · tip"
            } else {
                ""
            },
        );
        if self.title.is_some() {
            let _ = write!(
                out,
                r#"<p class="text{}">{}</p>"#,
                if self.prose { "" } else { " mono" },
                text(&self.body)
            );
        }
        let step = |done: bool| if done { "" } else { " class=\"wait\"" };
        let review_line = match self.state {
            "held" => "held until someone else reviews it".to_owned(),
            "rejected" => "rejected by a key outside its lineage".to_owned(),
            _ => self.approval_line(),
        };
        let served_line = match (self.state, &self.served) {
            ("rejected", _) => "never: the text is kept".to_owned(),
            (_, Some(d)) => format!("to readers of {} · {d}", text(&self.workarea)),
            _ => "not yet".to_owned(),
        };
        let _ = write!(
            out,
            r#"<div class="oml-timeline-wrap"><ol class="oml-timeline" aria-label="Provenance"><li><span class="c-dim">{actor_glyph}</span><span class="step">Written</span><span><span class="who">{who}</span> · {kind}</span><span>{}</span></li><li><span class="c-held">{}</span><span class="step">Held</span><span>scan: {}</span></li><li{}><span class="{}">{}</span><span class="step">{}</span><span>{review_line}</span></li><li{}><span class="{}">{}</span><span class="step">Served</span><span>{served_line}</span></li></ol></div><details class="oml-more"><summary>Details</summary><dl class="oml-facts"><dt>Id</dt><dd><span class="id">{}</span></dd><dt>Workarea</dt><dd><span class="id">{}</span></dd><dt>Claim key</dt><dd><span class="id">{}</span></dd>"#,
            self.written,
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
            step(served),
            if served { "c-promoted" } else { "c-faint" },
            glyph_svg("promoted"),
            self.id,
            text(&self.workarea),
            text(&self.claim_key),
        );
        for (label, value) in [
            ("Source", &self.anchor),
            ("Format", &self.format),
            ("Valid", &self.valid),
        ] {
            if let Some(value) = value {
                let _ = write!(out, "<dt>{label}</dt><dd>{}</dd>", text(value));
            }
        }
        let _ = write!(
            out,
            r#"<dt>Open</dt><dd><a href="{memory_page}?id={}">this memory</a></dd></dl></details></div></details></li>"#,
            self.id
        );
        out
    }
}

/// One page of rows and the cursor of the next.
struct CardPage {
    cards: Vec<Card>,
    next: Option<String>,
}

impl CardPage {
    fn json(&self) -> Value {
        json!({
            "items": self.cards.iter().map(Card::json).collect::<Vec<_>>(),
            "next_cursor": self.next,
        })
    }
}

/// Why a page of the list has no answer.
enum Failed {
    /// The API refused or did not answer.
    Api(oml_sdk::Error),
    /// The page refused a value before any call.
    Refused(StatusCode, &'static str, String),
}

impl From<oml_sdk::Error> for Failed {
    fn from(e: oml_sdk::Error) -> Self {
        Self::Api(e)
    }
}

/// The person's own keys, each with its kind (ADR-0045 d32), through
/// `GET /v2/keys` as `<user>-web`, a page at a time; past
/// [`KEY_PAGES_MAX`] pages the page is refused `limit.list_too_long`, and
/// at a cursor the API repeats, the API's fault, `unavailable` with 502
/// (#281): a list cut short would leave rows without their kind and send
/// an author search to `access.not_found` for a key the person holds.
async fn own_kinds(web: &Web, credential: &str, calls: &PageCalls) -> Result<Kinds, Failed> {
    walk(KEY_PAGES_MAX, |cursor| async move {
        let ids = calls.next_ids();
        let page = web
            .api
            .list_keys(&call(credential, &ids), None, PAGE_MAX, cursor.as_deref())
            .await?;
        let kinds: Kinds = page
            .items
            .into_iter()
            .map(|k| (k.id, k.name, kind_word(&k.held_by)))
            .collect();
        Ok((kinds, page.next_cursor))
    })
    .await
    .map_err(|end| match end {
        WalkEnd::Failed(e) => Failed::Api(e),
        WalkEnd::Repeated => Failed::Refused(
            StatusCode::BAD_GATEWAY,
            "unavailable",
            "the server repeated a cursor of this account's key list, so the list cannot be read whole; try again"
                .to_owned(),
        ),
        WalkEnd::TooLong => Failed::Refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            "limit.list_too_long",
            format!(
                "this account holds more keys than this page reads ({} pages of {PAGE_MAX}); use the API or the command line",
                KEY_PAGES_MAX
            ),
        ),
    })
}

/// The user page's page of the list, as `<user>-web`: the merged listing,
/// or with `q` recall's one ranked page narrowed by the other filters.
async fn user_cards(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    q: &ListQuery,
) -> Result<CardPage, Failed> {
    let credential = session.credential(Capacity::Web);
    let Some(text_query) = &q.q else {
        let ids = calls.next_ids();
        let query = MemoryQuery {
            workarea: q.workarea.as_deref(),
            state: q.state.as_deref(),
            origin: q.origin.as_deref(),
            author: q.author.as_deref(),
            limit: q.limit,
            cursor: q.cursor.as_deref(),
        };
        let listing_call = call(&credential, &ids);
        let (page, kinds) = tokio::join!(
            web.api.list_memories(&listing_call, &query),
            own_kinds(web, &credential, calls),
        );
        let (page, kinds) = (page?, kinds?);
        return Ok(CardPage {
            cards: page
                .items
                .iter()
                .map(|m| Card::of_memory(m, &kinds))
                .collect(),
            next: page.next_cursor,
        });
    };
    // The person's keys beside the recall, one round for both (#286).
    let ids = calls.next_ids();
    let recall_call = call(&credential, &ids);
    let recall = RecallQuery {
        query: text_query,
        k: q.limit.min(RECALL_MAX),
        workarea: q.workarea.as_deref(),
        origin: q.origin.as_deref(),
    };
    let (kinds, result) = tokio::join!(
        own_kinds(web, &credential, calls),
        web.api.recall(&recall_call, &recall),
    );
    let kinds = kinds?;
    // The author, one of the person's own keys; a name none of them holds
    // is refused whatever the recall found.
    let author = match &q.author {
        None => None,
        Some(name) => Some(
            kinds
                .iter()
                .find(|(_, n, _)| n.as_deref() == Some(name.as_str()))
                .map(|(id, _, _)| id.clone())
                .ok_or_else(|| {
                    Failed::Refused(
                        StatusCode::NOT_FOUND,
                        "access.not_found",
                        "no such key of this account".to_owned(),
                    )
                })?,
        ),
    };
    let result = result?;
    let cards = result
        .steering
        .iter()
        .chain(result.hits.iter().map(|h| &h.memory))
        .filter(|m| {
            q.state
                .as_deref()
                .is_none_or(|s| row_state(&m.state, m.tip) == s)
        })
        .filter(|m| {
            author
                .as_deref()
                .is_none_or(|a| m.author_key_id.as_deref() == Some(a))
        })
        .map(|m| Card::of_memory(m, &kinds))
        .collect();
    Ok(CardPage { cards, next: None })
}

/// The admin page's page of the list, as `<user>-admin`: one admin read,
/// which the API journals as one event.
async fn admin_cards(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    q: &ListQuery,
) -> Result<CardPage, Failed> {
    let credential = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    let page = web
        .api
        .admin_search(
            &call(&credential, &ids),
            &AdminSearch {
                query: q.q.as_deref(),
                workarea: q.workarea.as_deref(),
                state: q.state.as_deref(),
                origin_kind: q.origin.as_deref(),
                author: q.author.as_deref(),
                limit: q.limit,
                cursor: q.cursor.as_deref(),
            },
        )
        .await?;
    Ok(CardPage {
        cards: page.items.iter().map(Card::of_admin).collect(),
        next: page.next_cursor,
    })
}

/// The filters the picker's links keep beside the workarea, in the form's
/// order.
fn keep(q: &ListQuery) -> Vec<(&'static str, String)> {
    [
        ("q", &q.q),
        ("state", &q.state),
        ("origin", &q.origin),
        ("author", &q.author),
        ("limit", &q.limit_sent),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.clone().map(|v| (name, v)))
    .collect()
}

/// The workarea field of the view's filter form: the person's Atlas
/// empty is the person's root (ADR-0045 d25), every other view's every
/// workarea the key reads.
fn field(
    side: Side,
    q: &ListQuery,
    atlas: bool,
    chosen: &Chosen,
    names: &Names,
    browse: Option<&Browse>,
) -> String {
    let keep = keep(q);
    Field {
        id: "wa",
        action: if atlas { side.atlas() } else { side.memories() },
        keep: &keep,
        chosen,
        names,
        browse,
        admin: side == Side::Admin,
        standalone: false,
        empty: match (side, atlas) {
            (Side::User, true) => "Empty: the user root.",
            (Side::User, false) => "Empty: every readable workarea.",
            (Side::Admin, _) => "Empty: every workarea.",
        },
    }
    .html()
}

/// The filter form, its workarea field first, the link to the same
/// filters on the other view and, for the list, the rows and the pager
/// (ADR-0045 d22, d25, d27; design notes row 23).
fn list_view(
    side: Side,
    q: &ListQuery,
    page: Option<&CardPage>,
    atlas: bool,
    field: &str,
) -> String {
    let state = q.state.as_deref().unwrap_or("");
    let origin = q.origin.as_deref().unwrap_or("");
    let selected = |options: &[(&'static str, &'static str)], chosen: &str| {
        // The chosen value first, so the form shows what this page filters.
        let mut out: Vec<(&str, &str)> = options
            .iter()
            .filter(|(v, _)| *v == chosen)
            .copied()
            .collect();
        out.extend(options.iter().filter(|(v, _)| *v != chosen).copied());
        out
    };
    let states = selected(
        &[
            ("", "any"),
            ("held", "held"),
            ("promoted", "promoted"),
            ("rejected", "rejected"),
            ("superseded", "superseded"),
            ("demoted", "demoted"),
        ],
        state,
    );
    let origins = selected(
        &[
            ("", "any"),
            ("manual", "manual"),
            ("repo", "repo"),
            ("doc", "doc"),
            ("code", "code"),
            ("lake", "lake"),
            ("chat", "chat"),
            ("web", "web"),
        ],
        origin,
    );
    let mut out = format!(
        r#"<header class="oml-section-head"><div class="top"><h1>{}</h1></div><p class="desc">{}</p></header>"#,
        if atlas { "Atlas" } else { "Memories" },
        side.lede(),
    );
    let _ = write!(
        out,
        r#"<form class="filters" method="get" action="{}">{field}{}{}{}{}<button type="submit">Filter</button></form>"#,
        if atlas { side.atlas() } else { side.memories() },
        input("q", "Text", q.q.as_deref().unwrap_or(""), false),
        select("state", "State", &states),
        select("origin", "Source", &origins),
        input("author", "Author", q.author.as_deref().unwrap_or(""), false),
    );
    let _ = if atlas {
        write!(
            out,
            r#"<p class="oml-other-view"><a href="{}">The same filters as a list</a></p>"#,
            q.link(side.memories(), None)
        )
    } else {
        write!(
            out,
            r#"<p class="oml-other-view"><a href="{}">The same filters on the Atlas</a></p>"#,
            q.link(side.atlas(), None)
        )
    };
    let Some(page) = page else {
        return out;
    };
    if q.q.is_some() && side == Side::User {
        out.push_str(r#"<p class="dim">Best text matches first; one page.</p>"#);
    }
    if page.cards.is_empty() {
        out.push_str(r#"<p class="dim">No memory matches.</p>"#);
    }
    out.push_str(r#"<ul class="oml-mlist">"#);
    for card in &page.cards {
        out.push_str(&card.html(side.memory_page()));
    }
    out.push_str(r#"</ul><p class="pager">"#);
    if q.cursor.is_some() {
        let _ = write!(
            out,
            r#"<a href="{}">First page</a> "#,
            q.link(side.memories(), None)
        );
    }
    if let Some(next) = &page.next {
        let _ = write!(
            out,
            r#"<a rel="next" href="{}">Next page</a>"#,
            q.link(side.memories(), Some(next))
        );
    }
    out.push_str("</p>");
    out
}

/// The Atlas's scope: the workarea the filter names, else the person's
/// root, or `universe` on the admin page.
fn scope(side: Side, session: &Session, q: &ListQuery) -> Option<String> {
    q.workarea.clone().or_else(|| default_scope(side, session))
}

/// The Atlas's scope when the field is empty (ADR-0045 d25).
fn default_scope(side: Side, session: &Session) -> Option<String> {
    match side {
        Side::User => session.whoami.user.as_ref().map(|u| u.id.clone()),
        Side::Admin => Some("universe".to_owned()),
    }
}

/// The person's root's name, which makes the names beneath it
/// `<user>.<name>`; none on the admin page.
fn own_root(side: Side, session: &Session) -> Option<String> {
    match side {
        Side::User => session.whoami.user.as_ref().map(|u| u.name.clone()),
        Side::Admin => None,
    }
}

/// The reader of the Atlas's counts.
fn reader(side: Side) -> Capacity {
    match side {
        Side::User => Capacity::Web,
        Side::Admin => Capacity::Admin,
    }
}

/// The counted subtree and the scope's offers, read together: the counts
/// are the primary read, the offers the secondary one (ADR-0070 d8).
async fn regions(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    side: Side,
    scope: &str,
) -> (
    Result<oml_sdk::WorkareaCounts, oml_sdk::Error>,
    Result<NodeOffers, (String, oml_sdk::Error)>,
) {
    let credential = session.credential(reader(side));
    let credentials = Credentials::of(session, side.acts());
    let ids = calls.next_ids();
    let counts_call = call(&credential, &ids);
    if !credentials.transitions_as(&credential) {
        return tokio::join!(
            web.api.workarea_counts(&counts_call, scope, false),
            offers_of(&web.api, &credentials, calls, scope),
        );
    }
    // The key that reads the counts takes the transitions: the counted tree
    // carries the scope's offers to it, beside the reviewer key's for the
    // mode, so the offers add no round (#286).
    let request_id = ids.request_id.clone().unwrap_or_default();
    let (counts, mode) = tokio::join!(
        web.api.workarea_counts(&counts_call, scope, true),
        mode_offers(&web.api, &credentials, calls, scope),
    );
    match counts {
        Ok(oml_sdk::WorkareaCounts {
            offers: Some(transitions),
            items,
            truncated,
        }) => (
            Ok(oml_sdk::WorkareaCounts {
                items,
                truncated,
                offers: None,
            }),
            Ok(NodeOffers::of(transitions, mode)),
        ),
        // An API one release behind refuses `offers` as a parameter it does
        // not know (ADR-0070 d4): the counts and the transitions' offers are
        // read apart, a round more, as before #286.
        Err(oml_sdk::Error::Api(e)) if e.code == "request.validation" => {
            let ids = calls.next_ids();
            let counts_call = call(&credential, &ids);
            let (counts, transitions) = tokio::join!(
                web.api.workarea_counts(&counts_call, scope, false),
                transitions_offers(&web.api, &credentials, calls, scope),
            );
            (counts, transitions.map(|t| NodeOffers::of(t, mode)))
        }
        Ok(counted) => (
            Ok(counted),
            Err((
                request_id,
                oml_sdk::Error::Decode {
                    status: 200,
                    why: "the counted tree carried no offers".to_owned(),
                },
            )),
        ),
        Err(e) => (Err(e.clone()), Err((request_id, e))),
    }
}

/// The Atlas of `nodes`: the map, the table twin and the scope's panel with
/// its counts, its links and its dropdown of acts (d28, d30, d32).
fn atlas_view(
    side: Side,
    nodes: &[CountedNode],
    partial: bool,
    offers: &Result<NodeOffers, (String, oml_sdk::Error)>,
    refer: &dyn Fn(&str) -> String,
) -> Option<String> {
    let top = nodes.first()?;
    let top_id = top.id.as_str();
    let index = |id: &str| nodes.iter().position(|n| n.id == id);
    // Each node's subtree counts, children folded into parents; the tree
    // lists a parent before its children.
    let mut subtree: Vec<Counts> = nodes.iter().map(|n| Counts::of(&n.counts)).collect();
    for (i, n) in nodes.iter().enumerate().rev() {
        if i == 0 {
            continue;
        }
        if let Some(parent) = n.parent_id.as_deref().and_then(index)
            && parent != i
        {
            let own = subtree[i];
            subtree[parent].add(own);
        }
    }
    // Every link, tile and heading names a workarea by its reference, the
    // form every field takes on both pages (design notes row 20).
    let zoom = |path: &str| {
        format!(
            "{}?workarea={}",
            side.atlas(),
            oml_sdk::encode(&refer(path))
        )
    };
    let top_ref = refer(&top.path);
    let in_progress = |n: &CountedNode| n.mode == "in_progress";
    let mut tiles: Vec<Tile> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.parent_id.as_deref() == Some(top_id))
        .map(|(i, n)| Tile {
            name: refer(&n.path),
            href: zoom(&n.path),
            counts: subtree[i],
            status: n.status.clone(),
            in_progress: in_progress(n),
        })
        .collect();
    let own = Counts::of(&top.counts);
    if own.total() > 0 || tiles.is_empty() {
        tiles.push(Tile {
            name: format!("{top_ref} (own rows)"),
            href: format!("{}?workarea={}", side.memories(), oml_sdk::encode(&top_ref)),
            counts: own,
            status: top.status.clone(),
            in_progress: in_progress(top),
        });
    }
    tiles.sort_by(|a, b| {
        b.counts
            .total()
            .cmp(&a.counts.total())
            .then(a.name.cmp(&b.name))
    });
    let drawn: Vec<Tile> = tiles
        .iter()
        .filter(|t| t.counts.total() > 0)
        .cloned()
        .collect();
    let total = subtree[0];

    // No breadcrumb: the scope is the field's value, and the panel links
    // up to the scope's parent by its reference (AtlasScreen).
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<div class="oml-main"><div class="col"><p class="desc">{} readable · area = count{}</p>"#,
        total.total(),
        if partial {
            " · counted from the first 2,000 rows"
        } else {
            ""
        }
    );
    if drawn.is_empty() {
        out.push_str(r#"<p class="desc">Nothing readable here yet.</p>"#);
    } else {
        out.push_str(r#"<figure class="tm-fig">"#);
        out.push_str(&svg(&drawn, 960.0, 540.0, 20.0, "wide"));
        out.push_str(&svg(&drawn, 360.0, 270.0, 14.0, "narrow"));
        out.push_str("</figure>");
    }
    // The table twin: the same rows, the numbers right-aligned.
    out.push_str(r#"<table class="twin" id="atlas-table"><thead><tr><th scope="col">Workarea</th><th scope="col">Status</th><th scope="col" class="num">Readable</th><th scope="col">By state</th></tr></thead><tbody>"#);
    for t in &tiles {
        let _ = write!(
            out,
            r#"<tr><td class="mono"><a href="{}">{}</a></td><td>{}{}</td><td class="num">{}</td><td>{}<span class="cap">{}</span></td></tr>"#,
            attr(&t.href),
            text(&t.name),
            text(&t.status),
            if t.in_progress { " · in progress" } else { "" },
            t.counts.total(),
            lifebar(t.counts),
            text(&t.counts.words())
        );
    }
    out.push_str("</tbody></table></div>");
    // The scope's panel: what it holds, where to read it, what to do.
    let _ = write!(
        out,
        r#"<aside class="oml-rail-side" aria-labelledby="sel-h"><section class="oml-panel"><h2 id="sel-h" class="mono">{}</h2>{}<p class="desc">{}{} · <b>{}</b> readable</p>{}{}<p><a href="{}?workarea={}">Open memories</a></p>"#,
        text(&top_ref),
        top.path
            .rsplit_once('.')
            .map_or_else(String::new, |(parent, _)| format!(
                r#"<p class="oml-atlas-up">Up to <a href="{}">{}</a></p>"#,
                attr(&zoom(parent)),
                text(&refer(parent))
            )),
        text(&top.status),
        if in_progress(top) {
            " · in progress"
        } else {
            ""
        },
        total.total(),
        lifebar(total),
        legend(total),
        side.memories(),
        attr(&oml_sdk::encode(&top_ref)),
    );
    match offers {
        Ok(offers) => out.push_str(&offers.dropdown(side.acts().path(), top_id)),
        // A failed secondary read marks its part, with the request id
        // (ADR-0070 d8).
        Err((request_id, _)) => {
            let _ = write!(
                out,
                r#"<p class="error" role="status">The acts are unavailable (request <code>{}</code>).</p>"#,
                text(request_id)
            );
        }
    }
    out.push_str("</section></aside></div>");
    Some(out)
}

/// Any session but an admin entry's answers `access.not_found` on the
/// admin page, so the page's existence confirms nothing (ADR-0045 d10).
fn admin_only(side: Side, session: &Session, calls: &PageCalls) -> Option<Response> {
    (side == Side::Admin && !session.admin).then(|| {
        refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            calls,
        )
    })
}

/// The page's chrome around `body`, with `active` the tab marked.
fn page(side: Side, session: &Session, active: &str, body: &str) -> Response {
    let bar = match side {
        Side::User => session.personal(),
        Side::Admin => session.administration(),
    };
    outcome::html(StatusCode::OK, shell("", &bar, active, None, body))
}

fn failed_page(failed: Failed, calls: &PageCalls, next: &str) -> Response {
    match failed {
        Failed::Api(e) => outcome::page(&e, calls, next),
        Failed::Refused(status, code, message) => refused(status, code, &message, calls),
    }
}

/// One page of the list as the page's key reads it, `workarea` the value
/// sent in place of the query's.
async fn cards(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    side: Side,
    q: &ListQuery,
    workarea: Option<&str>,
) -> Result<CardPage, Failed> {
    let mut sent = q.clone();
    sent.workarea = workarea.map(str::to_owned);
    match side {
        Side::User => user_cards(web, session, calls, &sent).await,
        Side::Admin => admin_cards(web, session, calls, &sent).await,
    }
}

/// The picker's reads, made beside the page's primary read: the names for
/// its hints, from the uncounted tree, and Browse's counted tree only when
/// the query asks for Browse open (the operator's decision on the design
/// notes' row 14).
async fn picker_reads(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    side: Side,
    browse: bool,
    names: bool,
) -> (Names, Option<Browse>) {
    let credential = session.credential(reader(side));
    let own = own_root(side, session);
    tokio::join!(
        async {
            if names {
                Names::read(web, &credential, calls, "universe", own.as_deref()).await
            } else {
                Names::default()
            }
        },
        async {
            if browse {
                Some(Browse::read(web, &credential, calls, "universe").await)
            } else {
                None
            }
        }
    )
}

/// A page drawn with `status`: the picker's refusal answers 404 or 400
/// with the page whole around it.
fn with_status(mut response: Response, status: StatusCode) -> Response {
    *response.status_mut() = status;
    response
}

/// What a page's primary read answered for the field's value.
enum Primary<T> {
    /// Served, for `Chosen`.
    Served(Chosen, T),
    /// The field is refused; nothing is read or drawn below it.
    Missed(Chosen),
    /// The read failed for another reason.
    Failed(Failed),
}

/// The primary read `read` for `chosen`, settled against `names`: an
/// `access.not_found` to the field's value is its miss, or one more read by
/// the place a bare name names (ADR-0044 d5, the admin page's case).
async fn settle<T, F, Fut>(
    names: &Names,
    chosen: Chosen,
    first: Option<Result<T, Failed>>,
    read: F,
) -> Primary<T>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<T, Failed>>,
{
    match first {
        None => Primary::Missed(names.settle(chosen)),
        Some(Ok(served)) => Primary::Served(names.settle(chosen), served),
        Some(Err(Failed::Api(e))) if picker::is_not_found(&e) => match names.not_found(&chosen) {
            Some(Chosen::Place(place)) => match read(place.id.clone()).await {
                Ok(served) => Primary::Served(Chosen::Place(place), served),
                Err(failed) => Primary::Failed(failed),
            },
            Some(miss) => Primary::Missed(miss),
            None => Primary::Failed(Failed::Api(e)),
        },
        Some(Err(failed)) => Primary::Failed(failed),
    }
}

async fn list_page(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    uri: &Uri,
    side: Side,
) -> Response {
    if let Some(refusal) = admin_only(side, &session, &calls) {
        return refusal;
    }
    let next = uri
        .path_and_query()
        .map_or(side.memories(), |pq| pq.as_str());
    let parsed = Fields::of(uri).and_then(|f| {
        ListQuery::parse(&f)
            .map(|q| (q, picker::browse_asked(f.get(picker::BROWSE))))
            .map_err(|(_, m)| m)
    });
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
    let chosen = Chosen::of(q.workarea.as_deref());
    let first = async {
        match chosen.value() {
            _ if chosen.refused() => None,
            value => Some(cards(&web, &session, &calls, side, &q, value).await),
        }
    };
    let ((names, browse), first) = tokio::join!(
        picker_reads(&web, &session, &calls, side, browse, true),
        first
    );
    let primary = settle(&names, chosen, first, |id| {
        let (web, session, calls, q) = (&web, &session, &calls, &q);
        async move { cards(web, session, calls, side, q, Some(&id)).await }
    })
    .await;
    let (chosen, cards) = match primary {
        Primary::Served(chosen, cards) => (chosen, Some(cards)),
        Primary::Missed(chosen) => (chosen, None),
        Primary::Failed(failed) => return failed_page(failed, &calls, next),
    };
    q.workarea = chosen.shown().map(str::to_owned);
    let field = field(side, &q, false, &chosen, &names, browse.as_ref());
    with_status(
        page(
            side,
            &session,
            side.memories(),
            &list_view(side, &q, cards.as_ref(), false, &field),
        ),
        chosen.status(),
    )
}

async fn atlas_page(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    uri: &Uri,
    side: Side,
) -> Response {
    if let Some(refusal) = admin_only(side, &session, &calls) {
        return refusal;
    }
    let next = uri.path_and_query().map_or(side.atlas(), |pq| pq.as_str());
    let parsed = Fields::of(uri).and_then(|f| {
        ListQuery::parse(&f)
            .map(|q| (q, picker::browse_asked(f.get(picker::BROWSE))))
            .map_err(|(_, m)| m)
    });
    let (mut q, browse_asked) = match parsed {
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
    let Some(default) = default_scope(side, &session) else {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        );
    };
    let chosen = Chosen::of(q.workarea.as_deref());
    let scope = chosen.value().unwrap_or(&default).to_owned();
    // The Atlas of `universe` is the tree the hints and Browse read: its
    // counted read gives the names and an open Browse, and neither costs a
    // call of its own.
    let whole = scope == "universe";
    let reuse = browse_asked && whole;
    let first = async {
        if chosen.refused() {
            None
        } else {
            Some(regions(&web, &session, &calls, side, &scope).await)
        }
    };
    let ((names, browse), first) = tokio::join!(
        picker_reads(&web, &session, &calls, side, browse_asked && !reuse, !whole),
        first
    );
    // The counts are the primary read; the offers ride with them.
    let mut offers_of_first = None;
    let first = first.map(|(counts, offers)| {
        offers_of_first = Some(offers);
        counts.map_err(Failed::Api)
    });
    let retried = std::sync::Mutex::new(None);
    let primary = settle(&names, chosen, first, |id| {
        let (web, session, calls, retried) = (&web, &session, &calls, &retried);
        async move {
            let (counts, offers) = regions(web, session, calls, side, &id).await;
            *retried.lock().expect("unpoisoned") = Some(offers);
            counts.map_err(Failed::Api)
        }
    })
    .await;
    let offers = retried
        .into_inner()
        .expect("unpoisoned")
        .or(offers_of_first);
    let own = own_root(side, &session);
    let mut names = names;
    let (chosen, drawn, reused) = match primary {
        Primary::Served(chosen, counts) => {
            if whole {
                names = Names::of_counted(&counts.items, counts.truncated);
            }
            let chosen = names.settle(chosen);
            let refer = |path: &str| {
                picker::reference(path, |l| {
                    names.is_user_root(l)
                        || own.as_deref() == Some(l)
                        || counts.items.iter().any(|n| n.user_root && n.name == l)
                })
            };
            let offers = offers.unwrap_or_else(|| {
                Err((
                    String::new(),
                    oml_sdk::Error::Decode {
                        status: 200,
                        why: "no offers were read".to_owned(),
                    },
                ))
            });
            let Some(drawn) = atlas_view(side, &counts.items, counts.truncated, &offers, &refer)
            else {
                return refused(
                    StatusCode::NOT_FOUND,
                    "access.not_found",
                    "not found",
                    &calls,
                );
            };
            let browse = reuse.then(|| Browse::of(counts.items.clone(), counts.truncated));
            (chosen, drawn, browse)
        }
        Primary::Missed(chosen) => (chosen, String::new(), None),
        Primary::Failed(Failed::Api(e)) => return outcome::page(&e, &calls, next),
        Primary::Failed(failed) => return failed_page(failed, &calls, next),
    };
    let browse = reused.or(browse);
    q.workarea = chosen.shown().map(str::to_owned);
    let field = field(side, &q, true, &chosen, &names, browse.as_ref());
    let mut body = list_view(side, &q, None, true, &field);
    body.push_str(&drawn);
    with_status(page(side, &session, side.atlas(), &body), chosen.status())
}

/// `GET /web/memories`: the user page's Memories list.
pub async fn user_list(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    list_page(web, session, calls, &uri, Side::User).await
}

/// `GET /web/admin/memories`: the admin page's Memories list.
pub async fn admin_list(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    list_page(web, session, calls, &uri, Side::Admin).await
}

/// `GET /web/atlas`: the user page's Atlas.
pub async fn user_atlas(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    atlas_page(web, session, calls, &uri, Side::User).await
}

/// `GET /web/admin/atlas`: the admin page's Atlas, the tenant's tree.
pub async fn admin_atlas(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    atlas_page(web, session, calls, &uri, Side::Admin).await
}

// -- the JSON routes (ADR-0045 d25) -----------------------------------------

/// A JSON refusal in the API's envelope, naming the page's request id.
fn json_refusal(status: StatusCode, code: &str, message: &str, calls: &PageCalls) -> Response {
    (
        status,
        axum::Json(json!({
            "error": { "code": code, "message": message },
            "request_id": calls.page_id(),
        })),
    )
        .into_response()
}

fn json_failed(failed: Failed, calls: &PageCalls) -> Response {
    match failed {
        Failed::Api(oml_sdk::Error::Api(e)) => json_refusal(
            StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY),
            &e.code,
            &e.message,
            calls,
        ),
        Failed::Api(oml_sdk::Error::Transport(oml_sdk::TransportError::Timeout)) => json_refusal(
            StatusCode::GATEWAY_TIMEOUT,
            "unavailable",
            "the server did not answer in time; try again",
            calls,
        ),
        Failed::Api(_) => json_refusal(
            StatusCode::BAD_GATEWAY,
            "unavailable",
            "the server could not answer; try again",
            calls,
        ),
        Failed::Refused(status, code, message) => json_refusal(status, code, &message, calls),
    }
}

/// The regions as JSON (ADR-0045 d30): each node with its rows by state;
/// only the scope node, the first, carries the offers, as the Atlas's
/// panel does, or `offers_unavailable` where they failed.
fn regions_json(nodes: &[CountedNode], partial: bool, offers: Option<&NodeOffers>) -> Value {
    let items: Vec<Value> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let c = Counts::of(&n.counts);
            let mut region = json!({
                "id": n.id,
                "parent": n.parent_id,
                "name": n.name,
                "path": n.path,
                "status": n.status,
                "in_progress": n.mode == "in_progress",
                // Ontology crews are designed (ADR-0027) and not stored
                // yet; the badge shows when a workarea can say so.
                "ontology": false,
                "counts": {
                    "approved": c.approved,
                    "held": c.held,
                    "in_progress": c.in_progress,
                    "other": c.other,
                    "total": c.total(),
                },
            });
            if let (0, Some(offers)) = (i, offers) {
                region["offers"] = json!(
                    offers
                        .acts()
                        .iter()
                        .map(|a| json!({ "value": a.value(), "text": a.text() }))
                        .collect::<Vec<_>>()
                );
                region["commit_withheld"] = json!(offers.withheld());
            }
            region
        })
        .collect();
    json!({ "items": items, "partial": partial })
}

async fn data(
    web: Web,
    calls: Arc<PageCalls>,
    headers: &HeaderMap,
    uri: &Uri,
    side: Side,
) -> Response {
    // No session is not found, never a redirect: the route is data.
    let Some(token) = cookie(headers, SESSION_COOKIE) else {
        return json_refusal(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "not found",
            &calls,
        );
    };
    let session = match crate::session::resolve(&web, &calls, token).await {
        Ok(session) => session,
        Err(oml_sdk::Error::Api(e)) if e.code == "auth.invalid" => {
            return json_refusal(
                StatusCode::NOT_FOUND,
                "access.not_found",
                "not found",
                &calls,
            );
        }
        Err(e) => return json_failed(Failed::Api(e), &calls),
    };
    if side == Side::Admin && !session.admin {
        return json_refusal(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "not found",
            &calls,
        );
    }
    let q = match Fields::of(uri).and_then(|f| ListQuery::parse(&f).map_err(|(_, m)| m)) {
        Ok(q) => q,
        Err(message) => {
            return json_refusal(
                StatusCode::BAD_REQUEST,
                "request.validation",
                &message,
                &calls,
            );
        }
    };
    let Some(scope) = scope(side, &session, &q) else {
        return json_refusal(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "not found",
            &calls,
        );
    };
    let cards = async {
        match side {
            Side::User => user_cards(&web, &session, &calls, &q).await,
            Side::Admin => admin_cards(&web, &session, &calls, &q).await,
        }
    };
    let (cards, (counts, offers)) =
        tokio::join!(cards, regions(&web, &session, &calls, side, &scope));
    let cards = match cards {
        Ok(cards) => cards,
        Err(failed) => return json_failed(failed, &calls),
    };
    let counts = match counts {
        Ok(counts) => counts,
        Err(e) => return json_failed(Failed::Api(e), &calls),
    };
    // The offers are the secondary read (d8): their failure marks the scope
    // node's offers unavailable, as the Atlas's panel does, and the regions
    // and rows still answer.
    let mut answer = cards.json();
    answer["workareas"] = regions_json(&counts.items, counts.truncated, offers.as_ref().ok());
    if offers.is_err()
        && let Some(scope) = answer["workareas"]["items"].get_mut(0)
    {
        scope["offers_unavailable"] = json!(true);
    }
    axum::Json(answer).into_response()
}

/// `GET /web/data/memories`: the user page's page of the list and its
/// Atlas's workareas, as JSON.
pub async fn user_data(
    State(web): State<Web>,
    Extension(calls): Extension<Arc<PageCalls>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    data(web, calls, &headers, &uri, Side::User).await
}

/// `GET /web/admin/data/memories`: the admin page's, through the admin
/// read; the page is one `memory.admin_read`.
pub async fn admin_data(
    State(web): State<Web>,
    Extension(calls): Extension<Arc<PageCalls>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    data(web, calls, &headers, &uri, Side::Admin).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_keeps_every_filter_in_the_leads_order_and_escapes_it() {
        let pairs = vec![
            ("cursor".to_owned(), "c 1".to_owned()),
            ("state".to_owned(), "held".to_owned()),
            ("q".to_owned(), String::new()),
            ("author".to_owned(), "alice-phone".to_owned()),
        ];
        let q = ListQuery::parse(&Fields::of_pairs(pairs).unwrap()).unwrap();
        assert_eq!(
            q.link(USER_ATLAS_PATH, None),
            "/web/atlas?state=held&amp;author=alice-phone"
        );
        assert_eq!(
            q.link(USER_MEMORIES_PATH, Some("c 1")),
            "/web/memories?state=held&amp;author=alice-phone&amp;cursor=c%201"
        );
        let none = ListQuery::parse(&Fields::of_pairs(Vec::new()).unwrap()).unwrap();
        assert_eq!(none.link(USER_MEMORIES_PATH, None), "/web/memories");
    }

    fn parsed(pairs: &[(&str, &str)]) -> Result<ListQuery, FieldRefusal> {
        let pairs = pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect();
        ListQuery::parse(&Fields::of_pairs(pairs).unwrap())
    }

    #[test]
    fn an_empty_field_is_no_filter_and_the_default_page_size_holds() {
        let q = parsed(&[
            ("q", ""),
            ("state", ""),
            ("workarea", ""),
            ("origin", ""),
            ("author", ""),
            ("cursor", ""),
            ("limit", ""),
        ])
        .unwrap();
        assert!(q.q.is_none() && q.state.is_none() && q.workarea.is_none());
        assert!(q.origin.is_none() && q.author.is_none() && q.cursor.is_none());
        assert_eq!(q.limit, PAGE_DEFAULT);
        assert_eq!(q.link(USER_MEMORIES_PATH, None), "/web/memories");
    }

    #[test]
    fn a_malformed_limit_state_or_origin_is_refused_naming_its_field() {
        for (pairs, field) in [
            (&[("limit", "0")][..], "limit"),
            (&[("limit", "201")][..], "limit"),
            (&[("limit", "ten")][..], "limit"),
            (&[("limit", "-1")][..], "limit"),
            (&[("state", "approved")][..], "state"),
            (&[("state", "HELD")][..], "state"),
            (&[("origin", "email")][..], "origin"),
        ] {
            let refused = parsed(pairs).err().map(|(name, _)| name);
            assert_eq!(refused, Some(field), "{pairs:?}");
        }
        assert_eq!(parsed(&[("limit", "200")]).unwrap().limit, PAGE_MAX);
    }

    #[test]
    fn a_field_sent_twice_is_refused_before_it_is_read() {
        let pairs = vec![
            ("state".to_owned(), "held".to_owned()),
            ("state".to_owned(), "promoted".to_owned()),
        ];
        assert!(Fields::of_pairs(pairs).is_err());
    }

    #[test]
    fn a_promoted_version_that_is_no_longer_the_tip_is_superseded() {
        assert_eq!(row_state("promoted", true), "promoted");
        assert_eq!(row_state("promoted", false), "superseded");
        assert_eq!(row_state("erased", false), "demoted");
    }
}
