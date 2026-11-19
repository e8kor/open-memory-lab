//! The Memories list of both pages and its Atlas view (ADR-0045 d22–d32),
//! moved from the lead to paths of their own (ADR-0070 d7 (b)):
//! `/web/memories` and `/web/atlas` on the user page, `/web/admin/memories`
//! and `/web/admin/atlas` on the admin page, and their JSON at
//! `/web/data/memories` and `/web/admin/data/memories`, each with its
//! assertions unchanged but for the path (`tests/web_pages.rs`).
//!
//! The list is server-rendered rows, one page at a time, resumed by the
//! listing's keyset cursor and narrowed by five filters, in the views of
//! the Memories cockpit (ADR-0083 d1, d3 to d6), two links in
//! `nav.oml-views` that keep the workspace and the filters. The user page's
//! Approved, the default, lists the promoted tips through `GET /v1/memories`
//! with `with_own` and `state=promoted` as `<user>-web` (d23), or with a text
//! query through recall with `include_unreviewed: false`, its one ranked
//! page, so no held row reaches it; Submitted lists the person's own rows
//! held, rejected, withdrawn and approved in the last 30 days through
//! `mine=true`, each led by one plain sentence, a held one offering
//! Withdraw. The admin page's Approved, Held and Rejected list through
//! `POST /v2/admin/memories/search` as `<user>-admin`, one
//! `memory.admin_read` per page, which the API journals (d24); Held adds the
//! tenant's queue per workspace and links each row to Judge on the Personal
//! side, and no admin view judges (ADR-0083 d5, d7). The Atlas
//! reads no memory: `GET /v2/workspaces/{w}/tree?counts=true` and the scope's
//! offers (d30). Each row links to the memory's own page, `crate::memory`.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::State;
use axum::response::{IntoResponse as _, Response};
use http::{HeaderMap, StatusCode, Uri};
use oml_page::web::{
    ADMIN_ATLAS_PATH, ADMIN_MEMORIES_PATH, TODO_PATH, USER_ATLAS_PATH, USER_MEMORIES_PATH, attr,
    input, select, shell, text,
};
use oml_sdk::{AdminSearch, CountedNode, MemoryQuery, RecallQuery};
use serde_json::{Value, json};

use crate::atlas::{Counts, Tile, legend, lifebar, svg};
use crate::card::{Card, Kinds, kind_word, row_state};
use crate::keys::{instant, rfc3339, unix_now};
use crate::pages::{Fields, WalkEnd, refused, walk};
use crate::picker::{self, Browse, Chosen, Field, Names, Roots};
use crate::session::{PageCalls, Session, call, cookie};
use crate::workspaces::{
    Credentials, NodeOffers, Side as ActSide, mode_offers, offers_of, transitions_offers,
};
use crate::{Capacity, SESSION_COOKIE, Web, outcome};

/// A page's rows when the query names no size (ADR-0045 d22).
pub const PAGE_DEFAULT: u32 = 50;
/// The largest page, `api.page_limit_max`'s shipped value.
pub const PAGE_MAX: u32 = 200;
/// Recall's own bound on one answer, the user page's text search.
pub const RECALL_MAX: u32 = 100;
/// The most pages of the person's keys the text search reads to find the
/// key an author filter names: 10 pages of 200. Past them the search is
/// refused with `limit.list_too_long`, never run against a cut list. A
/// constant of the build, unmeasured (#170).
pub const KEY_PAGES_MAX: usize = 10;

/// How far back Submitted lists the person's approved rows (ADR-0083 d3).
pub const SUBMITTED_APPROVED_DAYS: i64 = 30;

/// The user page's JSON route (ADR-0045 d25).
pub const USER_DATA_PATH: &str = "/web/data/memories";
/// The admin page's JSON route.
pub const ADMIN_DATA_PATH: &str = "/web/admin/data/memories";

/// Which page the list is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
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

    /// The view's one line under its heading: on the admin side the
    /// capacity line, which names the key that reads (BrandHeader,
    /// MemoriesCockpit, AdminMemoriesCockpit, AtlasScreen).
    fn lede(self, view: Option<View>, session: &Session) -> String {
        let user = session.whoami.user.as_ref().map(|u| u.name.as_str());
        let admin = user.map_or_else(|| "the admin key".to_owned(), |u| format!("{u}-admin"));
        let read_as = |what: &str| {
            format!(
                "Read as <code>{}</code>: every {what} of the tenant, newest first. Each page read is journalled.",
                text(&admin)
            )
        };
        match (self, view) {
            (Self::User, Some(View::Submitted)) => format!("Writes by this account's keys, newest first: held, rejected, withdrawn, and approved in the last {SUBMITTED_APPROVED_DAYS} days. A held one can be withdrawn; a rejected or withdrawn one stays listed with its text."),
            (Self::User, Some(_)) => "Approved versions this account reads, newest first. A held, rejected or withdrawn version is never listed here.".to_owned(),
            (Self::User, None) => "Each workspace sized by the memories this account reads in it, by state.".to_owned(),
            (Self::Admin, Some(View::Held)) => format!(
                "{} This key never judges; verdicts are given on the Personal side, as <code>{}</code>.",
                read_as("held memory"),
                text(&user.map_or_else(|| "the reviewer key".to_owned(), |u| format!("{u}-review")))
            ),
            (Self::Admin, Some(View::Rejected)) => format!("{} This key never judges.", read_as("rejected memory")),
            (Self::Admin, Some(_)) => format!(
                "{} This key never recalls and never judges.",
                read_as("approved memory")
            ),
            (Self::Admin, None) => format!(
                "Read as <code>{}</code>: every workspace of the tenant, sized by its memories, each row counted as structure. No text is read.",
                text(&admin)
            ),
        }
    }

    /// The side's views, the default first (ADR-0083 d6).
    const fn views(self) -> &'static [View] {
        match self {
            Self::User => &[View::Approved, View::Submitted],
            Self::Admin => &[View::Approved, View::Held, View::Rejected],
        }
    }
}

/// A view of the Memories cockpit, named by `?view=` and none for Approved
/// (ADR-0083 d1, d3, d5, d6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    /// The promoted tips (or superseded versions, from the filter).
    Approved,
    /// The person's own held, rejected, withdrawn and recently approved
    /// rows; the user page only.
    Submitted,
    /// The tenant's held rows; the admin page only.
    Held,
    /// The tenant's rejected rows; the admin page only.
    Rejected,
}

impl View {
    /// `value` read on `side`; a view the side does not draw is refused
    /// naming the field. Approved, the default, has no value.
    fn of(side: Side, value: Option<&str>) -> Result<Self, FieldRefusal> {
        let view = match value {
            None => return Ok(Self::Approved),
            Some("submitted") => Self::Submitted,
            Some("held") => Self::Held,
            Some("rejected") => Self::Rejected,
            Some(_) => Self::Approved,
        };
        if value.is_some() && (view == Self::Approved || !side.views().contains(&view)) {
            return Err((
                "view",
                match side {
                    Side::User => "view is submitted, or none for Approved".to_owned(),
                    Side::Admin => "view is held or rejected, or none for Approved".to_owned(),
                },
            ));
        }
        Ok(view)
    }

    const fn value(self) -> Option<&'static str> {
        match self {
            Self::Approved => None,
            Self::Submitted => Some("submitted"),
            Self::Held => Some("held"),
            Self::Rejected => Some("rejected"),
        }
    }

    const fn word(self) -> &'static str {
        match self {
            Self::Approved => "Approved",
            Self::Submitted => "Submitted",
            Self::Held => "Held",
            Self::Rejected => "Rejected",
        }
    }

    /// The states its State filter offers, value and word, the default
    /// first; none where the view is one state.
    const fn states(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Approved => &[("promoted", "approved"), ("superseded", "superseded")],
            Self::Submitted => &[
                ("", "any of these"),
                ("held", "held"),
                ("rejected", "rejected"),
                ("withdrawn", "withdrawn"),
                ("promoted", "approved in the last 30 days"),
            ],
            Self::Held | Self::Rejected => &[],
        }
    }

    /// The `state` the view reads when the filter names none.
    const fn default_state(self) -> &'static str {
        match self {
            Self::Approved => "promoted",
            Self::Submitted => "held,rejected,withdrawn,promoted",
            Self::Held => "held",
            Self::Rejected => "rejected",
        }
    }
}

/// A refused field: its name and the message.
type FieldRefusal = (&'static str, String);

/// The Memories view's query, each field checked as the lead checked it
/// (ADR-0045 d22); the API checks an author's grammar, and the picker a
/// workspace's ([`crate::picker`]).
#[derive(Clone)]
struct ListQuery {
    view: View,
    q: Option<String>,
    state: Option<String>,
    workspace: Option<String>,
    origin: Option<String>,
    /// The admin views' kind filter, one kind (ADR-0083 d5); the Personal
    /// views take none.
    kind: Option<String>,
    author: Option<String>,
    cursor: Option<String>,
    limit: u32,
    limit_sent: Option<String>,
}

const ORIGINS: &[&str] = &["repo", "doc", "code", "lake", "chat", "web", "manual"];

/// The admin views' Kind filter, value and word, "any" first: the kinds the
/// admin search takes that a person or an agent writes (ADR-0083 d5;
/// ADR-0082 d8 item 3).
const KINDS: &[(&str, &str)] = &[
    ("", "any"),
    ("memory", "memory"),
    ("derived_fact", "derived fact"),
    ("steering", "steering"),
    ("guardrail", "guardrail"),
    ("skill", "skill"),
    ("document", "document"),
    ("test", "test"),
];

/// The kinds the Personal cockpit's listing asks for, as its `kind`: none
/// sent, so every kind (ADR-0083 d1). The workspace page's Memories count
/// sums the approved claims of exactly these kinds (ADR-0082 d10).
pub(crate) const COCKPIT_KIND: Option<&str> = None;

/// Whether the Personal cockpit lists claims of `kind`.
pub(crate) fn cockpit_lists(kind: &str) -> bool {
    COCKPIT_KIND.is_none_or(|kinds| kinds.split(',').any(|k| k == kind))
}

impl ListQuery {
    fn parse(fields: &Fields, side: Side) -> Result<Self, FieldRefusal> {
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
        let view = View::of(side, value("view").as_deref())?;
        let state = value("state");
        let offered = view.states();
        if let Some(sent) = state.as_deref()
            && !offered.iter().any(|(v, _)| !v.is_empty() && *v == sent)
        {
            let named: Vec<&str> = offered
                .iter()
                .filter(|(v, _)| !v.is_empty())
                .map(|(v, _)| *v)
                .collect();
            return Err((
                "state",
                if named.is_empty() {
                    format!("{} lists one state; send no state", view.word())
                } else {
                    format!("on {}, state is {}", view.word(), named.join(" or "))
                },
            ));
        }
        let origin = value("origin");
        if origin.as_deref().is_some_and(|o| !ORIGINS.contains(&o)) {
            return Err((
                "origin",
                "origin is repo, doc, code, lake, chat, web or manual".to_owned(),
            ));
        }
        let kind = value("kind");
        if let Some(sent) = kind.as_deref() {
            if side == Side::User {
                return Err(("kind", "the Personal views take no kind filter".to_owned()));
            }
            if !KINDS.iter().any(|(v, _)| !v.is_empty() && *v == sent) {
                return Err((
                    "kind",
                    "kind is memory, derived_fact, steering, guardrail, skill, document or test"
                        .to_owned(),
                ));
            }
        }
        Ok(Self {
            view,
            q: value("q"),
            state,
            workspace: value("workspace"),
            origin,
            kind,
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
            ("view", self.view.value()),
            ("q", self.q.as_deref()),
            ("state", self.state.as_deref()),
            ("workspace", self.workspace.as_deref()),
            ("origin", self.origin.as_deref()),
            ("kind", self.kind.as_deref()),
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

    /// The same query on `view`, its first page: the workspace and every
    /// filter but the state, whose values are each view's own (ADR-0083 d6).
    fn on(&self, view: View) -> Self {
        Self {
            view,
            state: if view == self.view {
                self.state.clone()
            } else {
                None
            },
            cursor: None,
            ..self.clone()
        }
    }

    /// The state the view reads: the filter's, or the view's default.
    fn state_sent(&self) -> &str {
        self.state.as_deref().unwrap_or(self.view.default_state())
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
pub(crate) enum Failed {
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

/// The person's own keys, through `GET /v2/keys` as `<user>-web`, a page
/// at a time, for the text search's author filter, which recall cannot
/// take by name; past [`KEY_PAGES_MAX`] pages the page is refused
/// `limit.list_too_long`, and at a cursor the API repeats, the API's
/// fault, `unavailable` with 502 (#281): a list cut short would send an
/// author search to `access.not_found` for a key the person holds.
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
            .map(|k| (k.id, k.name, kind_word(&k.used_by)))
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

/// The instant Submitted lists approved rows from: [`SUBMITTED_APPROVED_DAYS`]
/// before this process's clock (ADR-0083 d3).
fn submitted_since() -> String {
    rfc3339(unix_now() - SUBMITTED_APPROVED_DAYS * 86_400)
}

/// The user page's page of the list, as `<user>-web`. Approved: the merged
/// listing of the promoted tips, or with `q` recall's one ranked page with
/// `include_unreviewed: false`, narrowed by the other filters, so no held
/// row is listed (ADR-0083 d1). Submitted: the person's own rows, `mine`,
/// in its four states, each a card led by its sentence (ADR-0083 d3, d4).
async fn user_cards(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    q: &ListQuery,
) -> Result<CardPage, Failed> {
    let credential = session.credential(Capacity::Web);
    let submitted = q.view == View::Submitted;
    let text_query = q.q.as_ref().filter(|_| !submitted);
    let Some(text_query) = text_query else {
        let ids = calls.next_ids();
        let since = submitted.then(submitted_since);
        let query = MemoryQuery {
            mine: submitted,
            workspace: q.workspace.as_deref(),
            state: Some(q.state_sent()),
            promoted_since: since.as_deref(),
            origin: q.origin.as_deref(),
            author: q.author.as_deref(),
            kinds: COCKPIT_KIND,
            limit: q.limit,
            cursor: q.cursor.as_deref(),
        };
        let page = web
            .api
            .list_memories(&call(&credential, &ids), &query)
            .await?;
        // Every row Submitted lists is the person's own (`mine`), so a held
        // one offers Withdraw without a read of the person's keys.
        let cards = page
            .items
            .iter()
            .map(|m| {
                let card = Card::of_memory(m);
                if submitted { card.submitted() } else { card }
            })
            .collect();
        return Ok(CardPage {
            cards,
            next: page.next_cursor,
        });
    };
    // The person's keys beside the recall, one round for both (#286), read
    // only where an author names one of them: the recall filters by key id.
    let ids = calls.next_ids();
    let recall_call = call(&credential, &ids);
    let recall = RecallQuery {
        query: text_query,
        k: q.limit.min(RECALL_MAX),
        kinds: Some("memory,derived_fact"),
        workspace: q.workspace.as_deref(),
        origin: q.origin.as_deref(),
        include_unreviewed: false,
    };
    let keys = async {
        match &q.author {
            Some(_) => own_kinds(web, &credential, calls).await.map(Some),
            None => Ok(None),
        }
    };
    let (kinds, result) = tokio::join!(keys, web.api.recall(&recall_call, &recall));
    let kinds = kinds?;
    // The author, one of the person's own keys; a name none of them holds
    // is refused whatever the recall found.
    let author = match (&q.author, &kinds) {
        (Some(name), Some(kinds)) => Some(
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
        _ => None,
    };
    let result = result?;
    // Approved's state, read off each hit as the card names it: a held row
    // the recall served would be `held`, which no filter value names.
    let state = q.state_sent();
    let cards = result
        .steering
        .iter()
        .flatten()
        .chain(result.hits.iter().map(|h| &h.memory))
        .filter(|m| row_state(&m.state, m.tip) == state)
        .filter(|m| {
            author
                .as_deref()
                .is_none_or(|a| m.author_key_id.as_deref() == Some(a))
        })
        .map(Card::of_memory)
        .collect();
    Ok(CardPage { cards, next: None })
}

/// Whether a key of the signed-in person wrote the version `author` names,
/// read from the person's keys as the list reads them; a failed read is
/// the page that names it, never a guess (ADR-0083 d13).
pub(crate) async fn written_by_own_key(
    web: &Web,
    credential: &str,
    calls: &PageCalls,
    author: Option<&str>,
    next: &str,
) -> Result<bool, crate::acts::Refusal> {
    let Some(author) = author else {
        return Ok(false);
    };
    match own_kinds(web, credential, calls).await {
        Ok(kinds) => Ok(kinds.iter().any(|(id, _, _)| id == author)),
        Err(failed) => Err(Box::new(failed_page(failed, calls, next))),
    }
}

/// The admin page's page of the list, as `<user>-admin`: one admin read,
/// which the API journals as one event, in the view's state; a Held row
/// links to Judge on the Personal side as the person's reviewer key
/// (ADR-0083 d5).
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
                workspace: q.workspace.as_deref(),
                state: Some(q.state_sent()),
                origin_kind: q.origin.as_deref(),
                author: q.author.as_deref(),
                kinds: q.kind.as_deref(),
                limit: q.limit,
                cursor: q.cursor.as_deref(),
                investigation: session.investigation.as_deref(),
            },
        )
        .await?;
    let reviewer = session
        .whoami
        .user
        .as_ref()
        .map(|u| format!("{}-review", u.name));
    Ok(CardPage {
        cards: page
            .items
            .iter()
            .map(|m| match (&reviewer, q.view) {
                (Some(reviewer), View::Held) => Card::of_admin(m).judged_as(reviewer),
                _ => Card::of_admin(m),
            })
            .collect(),
        next: page.next_cursor,
    })
}

/// The filters the picker's links keep beside the workspace, in the form's
/// order.
fn keep(q: &ListQuery) -> Vec<(&'static str, String)> {
    let view = q.view.value().map(str::to_owned);
    [
        ("view", &view),
        ("q", &q.q),
        ("state", &q.state),
        ("origin", &q.origin),
        ("kind", &q.kind),
        ("author", &q.author),
        ("limit", &q.limit_sent),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.clone().map(|v| (name, v)))
    .collect()
}

/// The workspace field of the view's filter form: the person's Atlas
/// empty is the person's root (ADR-0045 d25), every other view's every
/// workspace the key reads.
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
            (Side::User, false) => "Empty: every readable workspace.",
            (Side::Admin, _) => "Empty: every workspace.",
        },
    }
    .html()
}

/// The views as two or three links in `nav.oml-views`, the current one
/// `aria-current="page"`, each keeping the workspace and the filters but
/// the state; `count`, where the page read one, on the current view's link
/// as a word and a number ("Held 21") (ADR-0083 d5, d6).
fn views_nav(side: Side, q: &ListQuery, count: Option<u64>) -> String {
    let mut out = String::from(r#"<nav class="oml-views" aria-label="Memories views">"#);
    for view in side.views() {
        let current = *view == q.view;
        let _ = write!(
            out,
            r#"<a href="{}"{}>{}{}</a>"#,
            q.on(*view).link(side.memories(), None),
            if current {
                r#" aria-current="page""#
            } else {
                ""
            },
            view.word(),
            match count {
                Some(n) if current => format!(r#" <span class="n">{n}</span>"#),
                _ => String::new(),
            }
        );
    }
    out.push_str("</nav>");
    out
}

/// The filter form, its workspace field first, the link to the same
/// filters on the other view and, for the list, the rows and the pager
/// (ADR-0045 d22, d25, d27; design notes row 23). On the list the views
/// lead, with `above`, the admin Held view's queue, under the lede; the
/// form offers Text wherever recall or the admin search runs, never on
/// Submitted, and the view's own states (ADR-0083 d4, d5).
fn list_view(
    side: Side,
    lede: &str,
    q: &ListQuery,
    page: Option<(&CardPage, &Roots)>,
    atlas: bool,
    field: &str,
    above: Option<(&str, Option<u64>)>,
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
    let states = selected(q.view.states(), state);
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
        r#"<header class="oml-section-head"><div class="top"><h1>{}</h1></div>"#,
        if atlas { "Atlas" } else { "Memories" },
    );
    if !atlas {
        out.push_str(&views_nav(side, q, above.and_then(|(_, n)| n)));
    }
    let _ = write!(out, r#"<p class="desc">{lede}</p></header>"#);
    if let Some((table, _)) = above {
        out.push_str(table);
    }
    let submitted = q.view == View::Submitted;
    // Submitted runs no search (d4): the view and a query sent on another
    // view ride along unseen, so the links back keep them.
    let mut hidden = String::new();
    if let Some(view) = q.view.value().filter(|_| !atlas) {
        let _ = write!(
            hidden,
            r#"<input type="hidden" name="view" value="{view}">"#
        );
    }
    let text_field = match (&q.q, submitted) {
        (Some(query), true) => {
            let _ = write!(
                hidden,
                r#"<input type="hidden" name="q" value="{}">"#,
                attr(query)
            );
            String::new()
        }
        (_, true) => String::new(),
        (query, false) => input("q", "Text", query.as_deref().unwrap_or(""), false),
    };
    let state_field = if states.is_empty() {
        String::new()
    } else {
        select("state", "State", &states)
    };
    // The admin search's kind filter, on the admin list's views alone
    // (ADR-0083 d5).
    let kind_field = if side == Side::Admin && !atlas {
        select(
            "kind",
            "Kind",
            &selected(KINDS, q.kind.as_deref().unwrap_or("")),
        )
    } else {
        String::new()
    };
    let _ = write!(
        out,
        r#"<form class="filters" method="get" action="{}">{hidden}{field}{text_field}{state_field}{kind_field}{}{}<button type="submit">Filter</button></form>"#,
        if atlas { side.atlas() } else { side.memories() },
        select("origin", "Source", &origins),
        input("author", "Author", q.author.as_deref().unwrap_or(""), false),
    );
    let approved = q.on(View::Approved);
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
            approved.link(side.atlas(), None)
        )
    };
    let Some((page, roots)) = page else {
        return out;
    };
    if q.q.is_some() && side == Side::User && !submitted {
        out.push_str(r#"<p class="dim">Best text matches first; one page.</p>"#);
    }
    if page.cards.is_empty() {
        out.push_str(match q.view {
            View::Submitted => r#"<p class="dim">Nothing submitted matches: no held, rejected or withdrawn write, and none approved in the last 30 days.</p>"#,
            View::Held => r#"<p class="dim">Nothing held matches.</p>"#,
            _ => r#"<p class="dim">No memory matches.</p>"#,
        });
    }
    out.push_str(r#"<ul class="oml-mlist">"#);
    for card in &page.cards {
        out.push_str(&card.html(side.memory_page(), roots));
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

/// The admin Held view's "Held per workspace" table from the tenant's
/// queue, oldest first, each workspace a link that filters the rows below
/// with the view kept; and the held total, the view's count (ADR-0083 d5).
fn held_table(queue: &oml_sdk::ReviewQueue, roots: &Roots) -> (String, u64) {
    let mut rows: Vec<&oml_sdk::QueueItem> = queue.items.iter().collect();
    rows.sort_by(|a, b| a.oldest_at.cmp(&b.oldest_at));
    let mut out = String::from(
        r#"<h2>Held per workspace</h2><p class="desc">Oldest first; each workspace filters the rows below.</p><table><tr><th scope="col">Workspace</th><th scope="col" class="num">Held</th><th scope="col">Oldest since</th></tr>"#,
    );
    if rows.is_empty() {
        out.push_str(r#"<tr><td colspan="3" class="dim">Nothing is held in the tenant.</td></tr>"#);
    }
    for row in &rows {
        let reference = roots.reference(&row.path);
        let _ = write!(
            out,
            r#"<tr><td class="mono"><a href="{ADMIN_MEMORIES_PATH}?view=held&amp;workspace={}">{}</a></td><td class="num">{}</td><td class="mono">{}</td></tr>"#,
            attr(&oml_sdk::encode(&reference)),
            text(&reference),
            row.held,
            instant(&row.oldest_at)
        );
    }
    out.push_str("</table>");
    (out, rows.iter().map(|r| r.held).sum())
}

/// The Atlas's scope: the workspace the filter names, else the person's
/// root, or `universe` on the admin page.
fn scope(side: Side, session: &Session, q: &ListQuery) -> Option<String> {
    q.workspace.clone().or_else(|| default_scope(side, session))
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
    Result<oml_sdk::WorkspaceCounts, oml_sdk::Error>,
    Result<NodeOffers, (String, oml_sdk::Error)>,
) {
    let credential = session.credential(reader(side));
    let credentials = Credentials::of(session, side.acts());
    let ids = calls.next_ids();
    let counts_call = call(&credential, &ids);
    if !credentials.transitions_as(&credential) {
        return tokio::join!(
            web.api.workspace_counts(&counts_call, scope, false),
            offers_of(&web.api, &credentials, calls, scope),
        );
    }
    // The key that reads the counts takes the transitions: the counted tree
    // carries the scope's offers to it, beside the reviewer key's for the
    // mode, so the offers add no round (#286).
    let request_id = ids.request_id.clone().unwrap_or_default();
    let (counts, mode) = tokio::join!(
        web.api.workspace_counts(&counts_call, scope, true),
        mode_offers(&web.api, &credentials, calls, scope),
    );
    match counts {
        Ok(oml_sdk::WorkspaceCounts {
            offers: Some(transitions),
            items,
            truncated,
        }) => (
            Ok(oml_sdk::WorkspaceCounts {
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
                web.api.workspace_counts(&counts_call, scope, false),
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
    // Every link, tile and heading names a workspace by its reference, the
    // form every field takes on both pages (design notes row 20).
    let zoom = |path: &str| {
        format!(
            "{}?workspace={}",
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
            href: format!(
                "{}?workspace={}",
                side.memories(),
                oml_sdk::encode(&top_ref)
            ),
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
    out.push_str(r#"<table class="twin" id="atlas-table"><thead><tr><th scope="col">Workspace</th><th scope="col">Status</th><th scope="col" class="num">Readable</th><th scope="col">By state</th></tr></thead><tbody>"#);
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
        r#"<aside class="oml-rail-side" aria-labelledby="sel-h"><section class="oml-panel"><h2 id="sel-h" class="mono">{}</h2>{}<p class="desc">{}{} · <b>{}</b> readable</p>{}{}<p><a href="{}?workspace={}">Open memories</a></p>"#,
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

/// The page's chrome around `body`, with `active` the tab marked and
/// `title` before the tab's word in the title, where a view names one
/// (`Submitted · Memories · OML`).
fn page(side: Side, session: &Session, active: &str, title: &str, body: &str) -> Response {
    let bar = match side {
        Side::User => session.personal(),
        Side::Admin => session.administration(),
    };
    // An admin page that read claim text names the open investigation's
    // close control (ADR-0089 d10).
    let body = match side {
        Side::Admin if session.investigation.is_some() => {
            format!("{}{body}", crate::investigation::line(&session.csrf()))
        }
        _ => body.to_owned(),
    };
    outcome::html(StatusCode::OK, shell(title, &bar, active, None, &body))
}

pub(crate) fn failed_page(failed: Failed, calls: &PageCalls, next: &str) -> Response {
    match failed {
        Failed::Api(e) => outcome::page(&e, calls, next),
        Failed::Refused(status, code, message) => refused(status, code, &message, calls),
    }
}

/// One page of the list as the page's key reads it, `workspace` the value
/// sent in place of the query's.
async fn cards(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    side: Side,
    q: &ListQuery,
    workspace: Option<&str>,
) -> Result<CardPage, Failed> {
    let mut sent = q.clone();
    sent.workspace = workspace.map(str::to_owned);
    match side {
        Side::User => user_cards(web, session, calls, &sent).await,
        Side::Admin => admin_cards(web, session, calls, &sent).await,
    }
}

/// The picker's reads, made beside the page's primary read: the names for
/// its hints, from the uncounted tree, and Browse's counted tree only when
/// the query asks for Browse open (the operator's decision on the design
/// notes' row 14).
pub(crate) async fn picker_reads(
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
                Names::read(
                    web,
                    &credential,
                    calls,
                    "universe",
                    own.as_deref(),
                    picker::LIST_HINTS_PAGES,
                )
                .await
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
pub(crate) fn with_status(mut response: Response, status: StatusCode) -> Response {
    *response.status_mut() = status;
    response
}

/// What a page's primary read answered for the field's value.
pub(crate) enum Primary<T> {
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
pub(crate) async fn settle<T, F, Fut>(
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
        ListQuery::parse(&f, side)
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
    let chosen = Chosen::of(q.workspace.as_deref());
    let first = async {
        match chosen.value() {
            _ if chosen.refused() => None,
            value => Some(cards(&web, &session, &calls, side, &q, value).await),
        }
    };
    // The admin Held view's queue, beside the page's first read (d5).
    let queue = async {
        if q.view == View::Held {
            let credential = session.credential(Capacity::Admin);
            let ids = calls.next_ids();
            Some(web.api.review_queue(&call(&credential, &ids)).await)
        } else {
            None
        }
    };
    let ((names, browse), first, queue) = tokio::join!(
        picker_reads(&web, &session, &calls, side, browse, true),
        first,
        queue
    );
    let queue = match queue.transpose() {
        Ok(queue) => queue,
        Err(e) => return failed_page(Failed::Api(e), &calls, next),
    };
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
    q.workspace = chosen.shown().map(str::to_owned);
    // Each row names its workspace by reference: the names' kinds, and a
    // read of any top label they leave unknown (design notes row 20).
    let queued: Vec<&str> = queue
        .iter()
        .flat_map(|q| q.items.iter().map(|r| r.path.as_str()))
        .collect();
    let roots = match &cards {
        Some(cards) => {
            names
                .roots()
                .clone()
                .learn(
                    &web,
                    &session.credential(reader(side)),
                    &calls,
                    cards
                        .cards
                        .iter()
                        .map(Card::workspace)
                        .chain(queued.iter().copied()),
                )
                .await
        }
        None => {
            names
                .roots()
                .clone()
                .learn(&web, &session.credential(reader(side)), &calls, queued)
                .await
        }
    };
    let held = queue.as_ref().map(|queue| held_table(queue, &roots));
    let field = field(side, &q, false, &chosen, &names, browse.as_ref());
    let title = match q.view {
        View::Approved => "",
        view => view.word(),
    };
    with_status(
        page(
            side,
            &session,
            side.memories(),
            title,
            &list_view(
                side,
                &side.lede(Some(q.view), &session),
                &q,
                cards.as_ref().map(|c| (c, &roots)),
                false,
                &field,
                held.as_ref().map(|(table, n)| (table.as_str(), Some(*n))),
            ),
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
        ListQuery::parse(&f, side)
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
    let chosen = Chosen::of(q.workspace.as_deref());
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
    q.workspace = chosen.shown().map(str::to_owned);
    let field = field(side, &q, true, &chosen, &names, browse.as_ref());
    let mut body = list_view(
        side,
        &side.lede(None, &session),
        &q,
        None,
        true,
        &field,
        None,
    );
    body.push_str(&drawn);
    with_status(
        page(side, &session, side.atlas(), "", &body),
        chosen.status(),
    )
}

/// `GET /web/memories`: the user page's Memories list.
pub async fn user_list(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    // The cockpit's drafted To review and Fixes views are To do's groups:
    // their links answer 308 there, keeping the workspace (ADR-0083 d6).
    if let Ok(fields) = Fields::of(&uri)
        && matches!(fields.get("view"), Some("review" | "fixes"))
    {
        let location = fields.given("workspace").map_or_else(
            || TODO_PATH.to_owned(),
            |w| format!("{TODO_PATH}?workspace={}", oml_sdk::encode(w)),
        );
        return (
            StatusCode::PERMANENT_REDIRECT,
            [(http::header::LOCATION, location)],
        )
            .into_response();
    }
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
                // yet; the badge shows when a workspace can say so.
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
    let investigation =
        cookie(headers, crate::investigation::INVESTIGATION_COOKIE).filter(|id| !id.is_empty());
    let session = match crate::session::resolve(&web, &calls, token).await {
        Ok(session) => session.investigating(investigation),
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
    let q = match Fields::of(uri).and_then(|f| ListQuery::parse(&f, side).map_err(|(_, m)| m)) {
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
    answer["workspaces"] = regions_json(&counts.items, counts.truncated, offers.as_ref().ok());
    if offers.is_err()
        && let Some(scope) = answer["workspaces"]["items"].get_mut(0)
    {
        scope["offers_unavailable"] = json!(true);
    }
    axum::Json(answer).into_response()
}

/// `GET /web/data/memories`: the user page's page of the list and its
/// Atlas's workspaces, as JSON.
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
            ("view".to_owned(), "submitted".to_owned()),
        ];
        let q = ListQuery::parse(&Fields::of_pairs(pairs).unwrap(), Side::User).unwrap();
        assert_eq!(
            q.link(USER_ATLAS_PATH, None),
            "/web/atlas?view=submitted&amp;state=held&amp;author=alice-phone"
        );
        assert_eq!(
            q.link(USER_MEMORIES_PATH, Some("c 1")),
            "/web/memories?view=submitted&amp;state=held&amp;author=alice-phone&amp;cursor=c%201"
        );
        // Another view keeps every filter but the state, on its first page.
        assert_eq!(
            q.on(View::Approved).link(USER_MEMORIES_PATH, None),
            "/web/memories?author=alice-phone"
        );
        let none = ListQuery::parse(&Fields::of_pairs(Vec::new()).unwrap(), Side::User).unwrap();
        assert_eq!(none.link(USER_MEMORIES_PATH, None), "/web/memories");
    }

    fn parsed_on(side: Side, pairs: &[(&str, &str)]) -> Result<ListQuery, FieldRefusal> {
        let pairs = pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect();
        ListQuery::parse(&Fields::of_pairs(pairs).unwrap(), side)
    }

    fn parsed(pairs: &[(&str, &str)]) -> Result<ListQuery, FieldRefusal> {
        parsed_on(Side::User, pairs)
    }

    #[test]
    fn each_view_reads_its_own_states_and_refuses_another_views() {
        for (side, pairs, sent) in [
            (Side::User, &[][..], "promoted"),
            (Side::User, &[("state", "superseded")][..], "superseded"),
            (
                Side::User,
                &[("view", "submitted")][..],
                "held,rejected,withdrawn,promoted",
            ),
            (
                Side::User,
                &[("view", "submitted"), ("state", "withdrawn")][..],
                "withdrawn",
            ),
            (Side::Admin, &[][..], "promoted"),
            (Side::Admin, &[("view", "held")][..], "held"),
            (Side::Admin, &[("view", "rejected")][..], "rejected"),
        ] {
            assert_eq!(
                parsed_on(side, pairs).unwrap().state_sent(),
                sent,
                "{pairs:?}"
            );
        }
        for (side, pairs, field) in [
            (Side::User, &[("state", "held")][..], "state"),
            (Side::User, &[("view", "held")][..], "view"),
            (Side::Admin, &[("view", "submitted")][..], "view"),
            (
                Side::Admin,
                &[("view", "held"), ("state", "held")][..],
                "state",
            ),
            (
                Side::User,
                &[("view", "submitted"), ("state", "superseded")][..],
                "state",
            ),
        ] {
            let refused = parsed_on(side, pairs).err().map(|(name, _)| name);
            assert_eq!(refused, Some(field), "{pairs:?}");
        }
    }

    #[test]
    fn an_empty_field_is_no_filter_and_the_default_page_size_holds() {
        let q = parsed(&[
            ("q", ""),
            ("state", ""),
            ("workspace", ""),
            ("origin", ""),
            ("author", ""),
            ("cursor", ""),
            ("limit", ""),
        ])
        .unwrap();
        assert!(q.q.is_none() && q.state.is_none() && q.workspace.is_none());
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
    fn the_admin_views_take_one_kind_and_the_personal_views_none() {
        let q = parsed_on(Side::Admin, &[("view", "held"), ("kind", "skill")]).unwrap();
        assert_eq!(q.kind.as_deref(), Some("skill"));
        assert_eq!(
            q.link(ADMIN_MEMORIES_PATH, None),
            "/web/admin/memories?view=held&amp;kind=skill"
        );
        assert!(
            parsed_on(Side::Admin, &[("kind", "")])
                .unwrap()
                .kind
                .is_none()
        );
        for (side, pairs) in [
            (Side::Admin, &[("kind", "Skill")][..]),
            (Side::Admin, &[("kind", "skill,guardrail")][..]),
            (Side::User, &[("kind", "skill")][..]),
        ] {
            let refused = parsed_on(side, pairs).err().map(|(name, _)| name);
            assert_eq!(refused, Some("kind"), "{side:?} {pairs:?}");
        }
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
        // The admin search serves `superseded` as a state of its own.
        assert_eq!(row_state("superseded", false), "superseded");
    }
}
