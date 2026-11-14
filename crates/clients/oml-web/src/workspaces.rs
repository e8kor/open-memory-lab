//! The Workareas view of the user page and of the admin page (ADR-0045
//! d28), moved from the lead to paths of their own, `/web/workareas` and
//! `/web/admin/workareas` (ADR-0070 d7 (b)), with its assertions unchanged
//! but for the path (`tests/web_pages.rs`). Its acts are the first a page
//! of `oml-web` runs: each workarea's dropdown names exactly the acts the
//! acting keys may take now, read off `GET /v2/workareas/{w}/offers`;
//! Apply is a `GET` that changes nothing and asks again, naming a refused
//! act's refusal; the confirmation's one `POST` calls the act's documented
//! operation under the session's capacity credential, with the form's
//! idempotency key, and answers a `303` back to the view.
//!
//! On the user page every act and every offer is the person's reviewer
//! key's: a dreamer key a person holds proposes, ratifies another key's
//! proposal and sets the mode (ADR-0009 d3; ADR-0030 d1). On the admin page
//! transitions run as the admin key, which ADR-0009 admits by role, and
//! the mode as the person's reviewer key.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::{IntoResponse as _, Response};
use http::{HeaderValue, StatusCode, Uri, header};
use oml_page::web::{
    ADMIN_MEMORIES_PATH, ADMIN_WORKAREAS_PATH, USER_MEMORIES_PATH, USER_WORKAREAS_PATH, attr, form,
    hidden, input, shell, short, text,
};
use oml_sdk::{WorkareaNode, WorkareaOffers};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::pages::{Fields, refused, workareas as tree};
use crate::picker::reference;
use crate::session::{PageCalls, Session, call};
use crate::{Api, Capacity, Web, outcome};

/// The most API calls one Workareas page has in flight while it reads each
/// workarea's offers and proposals. A constant of the build, unmeasured
/// (#170).
pub const FAN_OUT: usize = 8;

/// The rows a Workareas page draws, each with its offers and proposals
/// read: the page's secondary reads are bounded by it (design notes row 26;
/// ADR-0045 d25's scaling limit).
pub const ROWS_PAGE: usize = 50;

/// The form actions of the view, each the act of one documented operation.
pub const ACTIONS: &[&str] = &[
    "workarea_transition",
    "workarea_ratify",
    "workarea_mode",
    "create_workarea",
];

/// Which page the view is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    User,
    Admin,
}

impl Side {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::User => USER_WORKAREAS_PATH,
            Self::Admin => ADMIN_WORKAREAS_PATH,
        }
    }

    /// The capacity that reads the tree.
    fn reader(self) -> Capacity {
        match self {
            Self::User => Capacity::Web,
            Self::Admin => Capacity::Admin,
        }
    }

    /// The capacity transitions run as.
    fn transitions(self) -> Capacity {
        match self {
            Self::User => Capacity::Review,
            Self::Admin => Capacity::Admin,
        }
    }
}

/// One workarea's offers as the page's keys see them: the transitions as
/// the transitions capacity's offers name them, the mode and the commit as
/// the person's reviewer key's.
#[derive(Clone)]
pub(crate) struct NodeOffers {
    transitions: WorkareaOffers,
    mode: Result<WorkareaOffers, oml_sdk::Error>,
}

/// An act of the dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Act {
    Propose(String),
    Ratify {
        transition_id: String,
        from: String,
        to: String,
    },
    InProgress,
    Commit,
}

impl Act {
    pub(crate) fn value(&self) -> String {
        match self {
            Self::Propose(to) => format!("propose:{to}"),
            Self::Ratify { transition_id, .. } => format!("ratify:{transition_id}"),
            Self::InProgress => "mode:in_progress".to_owned(),
            Self::Commit => "mode:standard".to_owned(),
        }
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Self::Propose(to) => format!("propose {to}"),
            Self::Ratify { from, to, .. } => format!("ratify {from} → {to}"),
            Self::InProgress => "put in progress".to_owned(),
            Self::Commit => "commit".to_owned(),
        }
    }

    /// What a confirmed act will do, in a sentence, naming the workarea by
    /// its reference.
    fn consequence(&self, reference: &str) -> String {
        let path = text(reference);
        match self {
            Self::Propose(to) => format!(
                "Proposes that <code>{path}</code> becomes {}. It keeps its status until a key other than the one proposing ratifies it.",
                text(to)
            ),
            Self::Ratify { from, to, .. } => format!(
                "Ratifies the proposal that <code>{path}</code> moves from {} to {}. It is recorded under the key this page acts with; a proposer never ratifies their own proposal.",
                text(from),
                text(to)
            ),
            Self::InProgress => format!(
                "Puts <code>{path}</code> in progress: the user's agents recall each other's unreviewed memories inside it, marked unreviewed, and nothing leaves it until it is committed."
            ),
            Self::Commit => format!(
                "Commits <code>{path}</code>: it leaves in progress, which needs every memory in it reviewed."
            ),
        }
    }

    /// The confirmation's one form: the act's action and fields, the CSRF
    /// token and a fresh nonce, whose digest is the act's idempotency key.
    fn confirm_form(&self, page: &str, csrf: &str, node: &str) -> String {
        let nonce = hidden("nonce", &nonce());
        let (action, fields) = match self {
            Self::Propose(to) => (
                "workarea_transition",
                format!("{}{}", hidden("workarea", node), hidden("to", to)),
            ),
            Self::Ratify { transition_id, .. } => (
                "workarea_ratify",
                format!(
                    "{}{}",
                    hidden("workarea", node),
                    hidden("transition", transition_id)
                ),
            ),
            Self::InProgress => (
                "workarea_mode",
                format!(
                    "{}{}",
                    hidden("workarea", node),
                    hidden("mode", "in_progress")
                ),
            ),
            Self::Commit => (
                "workarea_mode",
                format!("{}{}", hidden("workarea", node), hidden("mode", "standard")),
            ),
        };
        form(
            page,
            action,
            csrf,
            &format!("{fields}{nonce}"),
            "Confirm",
            true,
        )
    }
}

/// Whether `sent` is a nonce as [`nonce`] draws it.
pub(crate) fn is_nonce(sent: &str) -> bool {
    sent.len() == 32 && sent.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The lock of one act's idempotency key in this process, kept while a send
/// under it is in flight and dropped from the table once none holds it.
pub(crate) fn in_flight(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    static TABLE: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    > = std::sync::LazyLock::new(Default::default);
    let mut table = TABLE.lock().expect("unpoisoned");
    table.retain(|_, lock| Arc::strong_count(lock) > 1);
    table.entry(key.to_owned()).or_default().clone()
}

/// The notice a `done` the view's own acts name shows; any other is none,
/// so a crafted link cannot put text on the page.
fn notice(done: Option<&str>) -> Option<&'static str> {
    match done? {
        "proposed" => Some("proposed"),
        "ratified" => Some("ratified"),
        "mode set" => Some("mode set"),
        "created" => Some("created"),
        _ => None,
    }
}

/// A form's nonce: 16 random bytes as hex.
pub(crate) fn nonce() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source answers");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl NodeOffers {
    /// The acts offered now, in the lead's order: proposals, ratifications,
    /// then the mode change or the commit.
    pub(crate) fn acts(&self) -> Vec<Act> {
        let mut acts: Vec<Act> = self
            .transitions
            .transitions
            .iter()
            .filter_map(|t| match (t.op.as_str(), &t.transition_id) {
                ("propose", _) => Some(Act::Propose(t.to.clone())),
                ("ratify", Some(id)) => Some(Act::Ratify {
                    transition_id: id.clone(),
                    from: t.from.clone().unwrap_or_default(),
                    to: t.to.clone(),
                }),
                _ => None,
            })
            .collect();
        if let Ok(mode) = &self.mode {
            if mode
                .mode_change
                .as_ref()
                .is_some_and(|m| m.to == "in_progress")
            {
                acts.push(Act::InProgress);
            }
            if mode.commit.offered {
                acts.push(Act::Commit);
            }
        }
        acts
    }

    /// The held rows that withhold the commit.
    pub(crate) fn withheld(&self) -> Option<u64> {
        self.mode.as_ref().ok().and_then(|m| m.commit.held)
    }

    /// The refusal of an act not offered, `(code, message)`, as its verb
    /// would answer it now.
    fn refusal(&self, op: &str, to: &str) -> (String, String) {
        let offers = if op == "mode" {
            match &self.mode {
                Ok(offers) => offers,
                Err(oml_sdk::Error::Api(e)) => return (e.code.clone(), e.message.clone()),
                Err(e) => return ("unavailable".to_owned(), e.to_string()),
            }
        } else {
            &self.transitions
        };
        offers
            .refused
            .iter()
            .find(|r| r.op == op && r.to == to)
            .map_or_else(
                || {
                    (
                        "unavailable".to_owned(),
                        "the server named no refusal for this act; try again".to_owned(),
                    )
                },
                |r| (r.code.clone(), r.message.clone()),
            )
    }

    /// The dropdown: the offered acts, a withheld commit shown disabled
    /// with its count, and Apply only where something is offered.
    pub(crate) fn dropdown(&self, page: &str, node: &str) -> String {
        let acts = self.acts();
        let withheld = self.withheld();
        if acts.is_empty() && withheld.is_none() {
            return r#"<span class="dim">nothing to do here now</span>"#.to_owned();
        }
        let mut out = format!(
            r#"<form class="row act" method="get" action="{page}"><input type="hidden" name="workarea" value="{}"><label>act<select name="op">"#,
            attr(node)
        );
        for act in &acts {
            let _ = write!(
                out,
                r#"<option value="{}">{}</option>"#,
                attr(&act.value()),
                text(&act.text())
            );
        }
        if let Some(held) = withheld {
            let _ = write!(
                out,
                r#"<option disabled selected>commit — {held} {} still held</option>"#,
                if held == 1 { "memory" } else { "memories" }
            );
        }
        out.push_str("</select></label>");
        if !acts.is_empty() {
            out.push_str(r#"<button type="submit">Apply</button>"#);
        }
        out.push_str("</form>");
        out
    }
}

/// The page's credentials.
#[derive(Clone)]
pub(crate) struct Credentials {
    transitions: String,
    mode: String,
    same: bool,
}

impl Credentials {
    pub(crate) fn of(session: &Session, side: Side) -> Self {
        Self {
            transitions: session.credential(side.transitions()),
            mode: session.credential(Capacity::Review),
            same: side.transitions() == Capacity::Review,
        }
    }

    /// Whether transitions run as `credential`, so a read made with it can
    /// carry their offers.
    pub(crate) fn transitions_as(&self, credential: &str) -> bool {
        self.transitions == credential
    }
}

impl NodeOffers {
    /// One workarea's offers from the transitions capacity's answer and the
    /// mode capacity's, read apart.
    pub(crate) fn of(
        transitions: WorkareaOffers,
        mode: Result<WorkareaOffers, oml_sdk::Error>,
    ) -> Self {
        Self { transitions, mode }
    }
}

/// One workarea's offers for the transitions, as the transitions
/// capacity's key sees them; the request id of the call when it fails.
pub(crate) async fn transitions_offers(
    api: &Api,
    credentials: &Credentials,
    calls: &PageCalls,
    node: &str,
) -> Result<WorkareaOffers, (String, oml_sdk::Error)> {
    let ids = calls.next_ids();
    let request_id = ids.request_id.clone().unwrap_or_default();
    api.workarea_offers(&call(&credentials.transitions, &ids), node)
        .await
        .map_err(|e| (request_id, e))
}

/// One workarea's offers for the mode and the commit, as the mode
/// capacity's key sees them.
pub(crate) async fn mode_offers(
    api: &Api,
    credentials: &Credentials,
    calls: &PageCalls,
    node: &str,
) -> Result<WorkareaOffers, oml_sdk::Error> {
    let ids = calls.next_ids();
    api.workarea_offers(&call(&credentials.mode, &ids), node)
        .await
}

/// One workarea's offers: one call where one key takes every act, two
/// where the admin key takes the transitions; the request id of the call
/// that failed otherwise.
pub(crate) async fn offers_of(
    api: &Api,
    credentials: &Credentials,
    calls: &PageCalls,
    node: &str,
) -> Result<NodeOffers, (String, oml_sdk::Error)> {
    let ids = calls.next_ids();
    let request_id = ids.request_id.clone().unwrap_or_default();
    let transitions = api
        .workarea_offers(&call(&credentials.transitions, &ids), node)
        .await
        .map_err(|e| (request_id, e))?;
    let mode = if credentials.same {
        Ok(transitions.clone())
    } else {
        let ids = calls.next_ids();
        api.workarea_offers(&call(&credentials.mode, &ids), node)
            .await
    };
    Ok(NodeOffers { transitions, mode })
}

/// One row's reads: the offers, and the live proposals of the node.
struct Row {
    offers: Result<NodeOffers, String>,
    pending: Vec<String>,
}

/// Each node's offers and proposals, read concurrently, at most
/// [`FAN_OUT`] calls at a time, in the nodes' order.
async fn rows(
    api: &Api,
    credentials: &Credentials,
    calls: &Arc<PageCalls>,
    nodes: &[WorkareaNode],
) -> Vec<Row> {
    let permits = Arc::new(Semaphore::new(FAN_OUT));
    let mut set = JoinSet::new();
    for (i, node) in nodes.iter().enumerate() {
        let (api, credentials, calls, permits) = (
            api.clone(),
            credentials.clone(),
            calls.clone(),
            permits.clone(),
        );
        let id = node.id.clone();
        set.spawn(async move {
            let offers = {
                let _permit = permits.acquire().await.expect("never closed");
                offers_of(&api, &credentials, &calls, &id)
                    .await
                    .map_err(|(request_id, _)| request_id)
            };
            // The live proposals, shown as proposed until another key
            // ratifies one (ADR-0009 d3); a failed read shows none, as the
            // lead's page did.
            let pending = {
                let _permit = permits.acquire().await.expect("never closed");
                let ids = calls.next_ids();
                api.workarea(&call(&credentials.transitions, &ids), &id)
                    .await
                    .map(|view| view.pending_transitions.into_iter().map(|t| t.to).collect())
                    .unwrap_or_default()
            };
            (i, Row { offers, pending })
        });
    }
    let mut out: Vec<Option<Row>> = (0..nodes.len()).map(|_| None).collect();
    while let Some(joined) = set.join_next().await {
        if let Ok((i, row)) = joined {
            out[i] = Some(row);
        }
    }
    out.into_iter()
        .map(|row| {
            row.unwrap_or(Row {
                offers: Err("a read that did not finish".to_owned()),
                pending: Vec::new(),
            })
        })
        .collect()
}

/// The reference of each of `nodes`, the form every field takes
/// (design notes row 20): the user roots are the tree's own, and the
/// person's where the page knows it.
fn references(nodes: &[WorkareaNode], own_root: Option<&str>) -> Vec<String> {
    let roots: std::collections::BTreeSet<&str> = nodes
        .iter()
        .filter(|n| n.user_root)
        .map(|n| n.name.as_str())
        .chain(own_root)
        .collect();
    nodes
        .iter()
        .map(|n| reference(&n.path, |l| roots.contains(l)))
        .collect()
}

/// The table of `nodes`, each by its reference and the one it sits in,
/// with its status, mode, live proposals and dropdown.
fn table(
    side: Side,
    nodes: &[WorkareaNode],
    refs: &[String],
    shown: std::ops::Range<usize>,
    rows: &[Row],
) -> String {
    let page = side.path();
    let memories = match side {
        Side::User => USER_MEMORIES_PATH,
        Side::Admin => ADMIN_MEMORIES_PATH,
    };
    let mut out = String::from(
        "<table class=\"oml-stack t-wa\"><tr><th>Workarea</th><th>Inside</th><th>Status</th><th>Mode</th><th>Act</th></tr>",
    );
    for ((node, reference), row) in nodes[shown.clone()]
        .iter()
        .zip(&refs[shown.clone()])
        .zip(rows)
    {
        let inside = node
            .parent_id
            .as_deref()
            .and_then(|p| nodes.iter().position(|n| n.id == p))
            .map_or_else(String::new, |i| text(&refs[i]));
        let named = format!(
            r#"<td class="mono"><a href="{memories}?workarea={}">{}</a></td><td class="mono">{inside}</td>"#,
            attr(&oml_sdk::encode(reference)),
            text(reference),
        );
        let pending: String = row
            .pending
            .iter()
            .map(|to| format!("<br><span class=\"held\">proposed: {}</span>", text(to)))
            .collect();
        match &row.offers {
            Ok(offers) => {
                let _ = write!(
                    out,
                    r#"<tr>{named}<td>{}{pending}</td><td>{}</td><td>{}</td></tr>"#,
                    text(&offers.transitions.status),
                    if offers.transitions.mode == "in_progress" {
                        "in progress"
                    } else {
                        "standard"
                    },
                    offers.dropdown(page, &node.id)
                );
            }
            // A failed secondary read marks its row, with the request id
            // (ADR-0070 d8).
            Err(request_id) => {
                let _ = write!(
                    out,
                    r#"<tr>{named}<td colspan="3"><span class="error" role="status">unavailable (request <code>{}</code>)</span></td></tr>"#,
                    text(request_id)
                );
            }
        }
    }
    out.push_str("</table>");
    if shown.start > 0 || shown.end < nodes.len() {
        let _ = write!(
            out,
            r#"<p class="pager">{} of {}"#,
            if shown.is_empty() {
                "none".to_owned()
            } else {
                format!("{}–{}", shown.start + 1, shown.end)
            },
            nodes.len()
        );
        if shown.start > 0 {
            let _ = write!(out, r#" · <a href="{page}">First page</a>"#);
        }
        if shown.end < nodes.len() {
            let _ = write!(
                out,
                r#" · <a rel="next" href="{page}?from={}">Next page</a>"#,
                shown.end
            );
        }
        out.push_str("</p>");
    }
    out
}

/// What the view's reads share.
#[derive(Clone, Copy)]
struct ViewCtx<'a> {
    web: &'a Web,
    session: &'a Session,
    calls: &'a PageCalls,
    side: Side,
    credentials: &'a Credentials,
}

/// The confirmation above the table when the query names an `op`: the
/// act's consequence and its one form, or the act's refusal by name with
/// no form. An `op` outside the grammar is the form again; a proposal no
/// longer live, or a workarea the keys cannot read, is not found.
async fn confirmation(
    view: &ViewCtx<'_>,
    nodes: &[WorkareaNode],
    refs: &[String],
    fields: &Fields,
    op: &str,
) -> Result<String, Box<Response>> {
    let ViewCtx {
        web,
        session,
        calls,
        side,
        credentials,
    } = *view;
    let page = side.path();
    let Some(named) = fields.get("workarea") else {
        return Err(Box::new(refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "workarea is required",
            calls,
        )));
    };
    let listed = nodes
        .iter()
        .zip(refs)
        .find(|(n, r)| n.id == named || n.path == named || *r == named)
        .map(|(n, r)| (n.id.clone(), r.clone()));
    // A workarea the table does not list, zoomed to from the Atlas, is read
    // through the API, which checks the key may read it.
    let (id, path) = match listed {
        Some(node) => node,
        None => {
            let ids = calls.next_ids();
            match web
                .api
                .workarea(&call(&credentials.transitions, &ids), named)
                .await
            {
                Ok(view) => {
                    let own = own_root(session, side);
                    let reference = reference(&view.path, |l| {
                        own.as_deref() == Some(l)
                            || nodes.iter().any(|n| n.user_root && n.name == l)
                    });
                    (view.id, reference)
                }
                Err(e) => return Err(Box::new(outcome::page(&e, calls, page))),
            }
        }
    };
    let offers = offers_of(&web.api, credentials, calls, &id)
        .await
        .map_err(|(_, e)| Box::new(outcome::page(&e, calls, page)))?;
    let grammar = || {
        Box::new(refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "op is propose:committed, propose:validated, ratify:<transition>, mode:in_progress or mode:standard",
            calls,
        ))
    };
    let acts = offers.acts();
    let (wanted, refusal) = match op.split_once(':').ok_or_else(grammar)? {
        ("propose", to @ ("committed" | "validated")) => {
            (Act::Propose(to.to_owned()), ("propose", to))
        }
        ("ratify", transition) => {
            let Some(act) = acts.iter().find(
                |a| matches!(a, Act::Ratify { transition_id, .. } if transition_id == transition),
            ) else {
                return Err(Box::new(refused(
                    StatusCode::NOT_FOUND,
                    "access.not_found",
                    "no such proposal is live here",
                    calls,
                )));
            };
            (act.clone(), ("ratify", transition))
        }
        ("mode", "in_progress") => (Act::InProgress, ("mode", "in_progress")),
        ("mode", "standard") => (Act::Commit, ("mode", "standard")),
        _ => return Err(grammar()),
    };
    let back = format!(r#"<p><a href="{page}">Cancel</a></p>"#);
    if acts.contains(&wanted) {
        return Ok(format!(
            r#"<div class="card confirm"><p class="label">Confirm</p><h2>{} on {}</h2><p>{}</p>{}{back}</div>"#,
            text(&wanted.text()),
            text(&path),
            wanted.consequence(&path),
            wanted.confirm_form(page, &session.csrf(), &id)
        ));
    }
    let (code, message) = offers.refusal(refusal.0, refusal.1);
    Ok(format!(
        r#"<div class="card confirm"><p class="label">Not now</p><p class="error">{}: {}</p><p><a href="{page}">Back</a></p></div>"#,
        text(&code),
        text(&message)
    ))
}

/// The person's root's name on the user page, which makes the names
/// beneath it `<user>.<name>`.
fn own_root(session: &Session, side: Side) -> Option<String> {
    match side {
        Side::User => session.whoami.user.as_ref().map(|u| u.name.clone()),
        Side::Admin => None,
    }
}

async fn view(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    uri: &Uri,
    side: Side,
) -> Response {
    let fields = match Fields::of(uri) {
        Ok(fields) => fields,
        Err(message) => {
            return refused(
                StatusCode::BAD_REQUEST,
                "request.validation",
                &message,
                &calls,
            );
        }
    };
    let page = side.path();
    let next = uri.path_and_query().map_or(page, |pq| pq.as_str());
    let root = match side {
        Side::User => match session.whoami.user.as_ref() {
            Some(user) => user.id.clone(),
            None => {
                return refused(
                    StatusCode::NOT_FOUND,
                    "access.not_found",
                    "no such page here",
                    &calls,
                );
            }
        },
        Side::Admin => "universe".to_owned(),
    };
    let credentials = Credentials::of(&session, side);
    let reader = session.credential(side.reader());
    // The tree is the page's primary read; each row's offers and proposals
    // its secondary ones (ADR-0070 d8).
    let nodes = match tree(&web, &reader, &calls, &root).await {
        Ok(nodes) => nodes,
        Err((_, Some(e))) => return outcome::page(&e, &calls, next),
        Err((request_id, None)) => {
            return refused(
                StatusCode::BAD_GATEWAY,
                "unavailable",
                &format!("the workarea list did not end (request {request_id}); try again"),
                &calls,
            );
        }
    };
    let refs = references(&nodes, own_root(&session, side).as_deref());
    let from = match fields.get("from").map(str::parse::<usize>) {
        None => 0,
        Some(Ok(from)) => from.min(nodes.len()),
        Some(Err(_)) => {
            return refused(
                StatusCode::BAD_REQUEST,
                "request.validation",
                "from is a whole number",
                &calls,
            );
        }
    };
    let shown = from..(from + ROWS_PAGE).min(nodes.len());
    let ctx = ViewCtx {
        web: &web,
        session: &session,
        calls: &calls,
        side,
        credentials: &credentials,
    };
    let (confirm, rows) = match fields.get("op") {
        Some(op) => {
            let (confirm, rows) = tokio::join!(
                confirmation(&ctx, &nodes, &refs, &fields, op),
                rows(&web.api, &credentials, &calls, &nodes[shown.clone()]),
            );
            match confirm {
                Ok(confirm) => (confirm, rows),
                Err(response) => return *response,
            }
        }
        None => (
            String::new(),
            rows(&web.api, &credentials, &calls, &nodes[shown.clone()]).await,
        ),
    };
    let table = table(side, &nodes, &refs, shown, &rows);
    let (bar, body) = match side {
        Side::User => {
            let user = session.whoami.user.as_ref().map_or("", |u| u.name.as_str());
            let mut body = format!(
                r#"<h1>Workareas</h1><p class="desc">Under <code>{}</code>. Apply asks for a confirmation.</p>{confirm}{table}"#,
                short(user)
            );
            body.push_str("<details class=\"help\"><summary>?</summary>In progress: the user's agents recall each other's unreviewed memories inside it, and nothing leaves it until it is committed, which needs every memory in it reviewed.</details>");
            body.push_str("<h2>New workarea</h2>");
            // Creating a workarea is the view's act, through `POST
            // /v2/workareas` under the form's nonce (ADR-0070 d7 (b)).
            body.push_str(&form(
                USER_WORKAREAS_PATH,
                "create_workarea",
                &session.csrf(),
                &format!(
                    "{}{}{}",
                    input("name", "name", "", true),
                    input("parent", "inside (blank: the user root)", "", false),
                    hidden("nonce", &nonce())
                ),
                "Create",
                true,
            ));
            (session.personal(), body)
        }
        Side::Admin => (session.administration(), {
            let user = short(session.whoami.user.as_ref().map_or("", |u| u.name.as_str()));
            format!(
                r#"<h1>Workareas</h1><p class="desc">Apply asks for a confirmation.</p><details class="help"><summary>?</summary>Status changes run as <code>{user}-admin</code> and need a second key to ratify; in progress and commit run as <code>{user}-review</code>.</details>{confirm}{table}"#
            )
        }),
    };
    outcome::html(
        StatusCode::OK,
        shell("", &bar, page, notice(fields.get("done")), &body),
    )
}

/// Any session but an admin entry's answers `access.not_found` on the
/// admin page, so the page's existence confirms nothing (ADR-0045 d10).
fn admin_only(session: &Session, calls: &PageCalls) -> Option<Response> {
    (!session.admin).then(|| {
        refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            calls,
        )
    })
}

/// `GET /web/workareas`: the user page's Workareas view.
pub async fn user_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    view(web, session, calls, &uri, Side::User).await
}

/// `GET /web/admin/workareas`: the admin page's Workareas view, the
/// tenant's tree.
pub async fn admin_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    view(web, session, calls, &uri, Side::Admin).await
}

/// The fields each action defines, beside `action`, `csrf_token` and
/// `nonce`.
fn defined(action: &str) -> &'static [&'static str] {
    match action {
        "create_workarea" => &["action", "csrf_token", "nonce", "name", "parent"],
        "workarea_transition" => &["action", "csrf_token", "nonce", "workarea", "to"],
        "workarea_ratify" => &["action", "csrf_token", "nonce", "workarea", "transition"],
        _ => &["action", "csrf_token", "nonce", "workarea", "mode"],
    }
}

/// One act: the CSRF token was checked by layer 4, before any API call; the
/// form names one of [`ACTIONS`] and no field it does not define; the act
/// calls its operation under the form's idempotency key, and a success is
/// a `303` back to the view (Post/Redirect/Get). A form without a nonce
/// acts once under a fresh key.
async fn act(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
    side: Side,
) -> Response {
    let page = side.path();
    let fields = match form
        .map_err(|e| e.body_text())
        .and_then(|Form(pairs)| Fields::of_pairs(pairs))
    {
        Ok(fields) => fields,
        Err(message) => {
            return refused(
                StatusCode::BAD_REQUEST,
                "request.validation",
                &message,
                &calls,
            );
        }
    };
    let Some(action) = fields
        .get("action")
        .and_then(|a| ACTIONS.iter().find(|x| **x == a).copied())
    else {
        return refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "action is not one of this page's actions",
            &calls,
        );
    };
    calls.act(action);
    // Creating a workarea is the user page's act alone.
    if action == "create_workarea" && side == Side::Admin {
        return refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "action is not one of this page's actions",
            &calls,
        );
    }
    if let Some(name) = fields.outside(defined(action)) {
        return refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            &format!("{action} takes no field {name}"),
            &calls,
        );
    }
    let required: &[&str] = match action {
        "create_workarea" => &["name"],
        "workarea_transition" => &["workarea", "to"],
        "workarea_ratify" => &["workarea", "transition"],
        _ => &["workarea", "mode"],
    };
    if let Some(missing) = required.iter().find(|n| fields.get(n).is_none()) {
        return refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            &format!("{missing} is required"),
            &calls,
        );
    }
    let field = |name: &str| fields.get(name).unwrap_or_default();
    let workarea = field("workarea");
    if action == "create_workarea" && field("name").is_empty() {
        return refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "name is required",
            &calls,
        );
    }
    // A nonce the page did not draw is refused, so a mangled form never
    // loses its replay silently; a form without one acts once under a
    // fresh key.
    let nonce = match fields.get("nonce") {
        Some(sent) if is_nonce(sent) => sent.to_owned(),
        Some(_) => {
            return refused(
                StatusCode::BAD_REQUEST,
                "request.validation",
                "nonce is not one this page draws: 32 lower-case hexadecimal characters",
                &calls,
            );
        }
        None => nonce(),
    };
    let key = session.act_key(&nonce);
    // A second send of the same form while the first is in flight (a double
    // click) waits for it, then replays its recorded outcome at the API.
    // Two oml-web processes racing one key are #233's case, closed where
    // the API records the outcome.
    let _in_flight = in_flight(&key).lock_owned().await;
    let ids = calls.next_ids();
    let (outcome, done) = match action {
        "create_workarea" => {
            // A blank parent is the person's own root.
            let parent = fields.get("parent").filter(|p| !p.is_empty());
            let credential = session.credential(Capacity::Web);
            (
                web.api
                    .create_workarea(&call(&credential, &ids), &key, field("name"), parent)
                    .await
                    .map(|_| ()),
                "created",
            )
        }
        "workarea_transition" => {
            let to = field("to");
            let credential = session.credential(side.transitions());
            (
                web.api
                    .propose_transition(&call(&credential, &ids), &key, workarea, to)
                    .await
                    .map(|_| ()),
                "proposed",
            )
        }
        "workarea_ratify" => {
            let transition = field("transition");
            let credential = session.credential(side.transitions());
            (
                web.api
                    .ratify_transition(&call(&credential, &ids), &key, workarea, transition)
                    .await
                    .map(|_| ()),
                "ratified",
            )
        }
        _ => {
            let mode = field("mode");
            let credential = session.credential(Capacity::Review);
            (
                web.api
                    .set_workarea_mode(&call(&credential, &ids), &key, workarea, mode)
                    .await
                    .map(|_| ()),
                "mode+set",
            )
        }
    };
    match outcome {
        Ok(()) => {
            let location = format!("{page}?done={done}");
            let mut response = StatusCode::SEE_OTHER.into_response();
            if let Ok(value) = HeaderValue::from_str(&location) {
                response.headers_mut().insert(header::LOCATION, value);
            }
            response
        }
        Err(e) => outcome::page(&e, &calls, page),
    }
}

/// `POST /web/workareas`: an act of the user page's view.
pub async fn user_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    act(web, session, calls, form, Side::User).await
}

/// `POST /web/admin/workareas`: an act of the admin page's view.
pub async fn admin_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    act(web, session, calls, form, Side::Admin).await
}
