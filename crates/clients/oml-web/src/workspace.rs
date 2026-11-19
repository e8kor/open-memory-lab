//! A workspace's own page and its builder (ADR-0082 d1-d6; ADR-0083 d12,
//! d13): `GET /web/workspace?workspace=&kind=` lists the workspace's
//! skills, guardrails or documents with their acts, and the Guardrails kind
//! opens with what an agent connected here loads; `GET|POST
//! /web/workspace/new`, `/edit` and `/retire` propose a held version, each
//! previewed first through `POST /v1/memories/preview`, which writes
//! nothing; `GET /web/admin/workspace` draws the same contents read-only.
//!
//! Which key acts follows from the workspace, never from a form (d2): in
//! the person's root `<user>-web`, which holds writer there and supersedes
//! a row any key of the person wrote; outside it `<user>-review`, where it
//! holds reviewer, whose new claim is held and whose edit of another
//! author's row is a reviewer proposal. No control saves, publishes or
//! deletes: every write is a held version, and the version in force stays
//! served until a different key approves the next (ADR-0001 R2, R4).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::State;
use axum::extract::rejection::FormRejection;
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{
    ADMIN_MEMORIES_PATH, ADMIN_MEMORY_PATH, ADMIN_WORKSPACES_PATH, TODO_JUDGE_PATH,
    USER_MEMORIES_PATH, USER_MEMORY_PATH, USER_WORKSPACES_PATH, attr, hidden, shell, state_tag,
    text,
};
use oml_sdk::{
    AdminSearch, Draft, GuardrailSet, Memory, MemoryPreview, MemoryQuery, WorkspaceView, encode,
};
use serde_json::{Value, json};

use crate::acts;
use crate::diff::{Parts, labelled};
use crate::pages::{Fields, refused};
use crate::picker::{self, Chosen, Field, Names};
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, outcome};

/// The workspace page (ADR-0082 d1).
pub const WORKSPACE_PATH: &str = "/web/workspace";
/// The form that proposes a new skill, guardrail or document (d1, d4).
pub const NEW_PATH: &str = "/web/workspace/new";
/// The form that proposes the next version of an approved item (d1, d3).
pub const EDIT_PATH: &str = "/web/workspace/edit";
/// The form that proposes an approved item's retirement (d1, d3).
pub const RETIRE_PATH: &str = "/web/workspace/retire";
/// The admin side's read-only workspace page (d1).
pub const ADMIN_WORKSPACE_PATH: &str = "/web/admin/workspace";

/// The most versions one kind's table lists: one page of the listing at
/// the API's maximum; a longer list says so.
const LIST_LIMIT: u32 = 200;

/// The kinds the builder writes (ADR-0082 d4), in the order the page's
/// kind links draw them after Memories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Skill,
    Guardrail,
    Document,
}

impl Kind {
    const ALL: [Self; 3] = [Self::Skill, Self::Guardrail, Self::Document];

    fn of(sent: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == sent)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Guardrail => "guardrail",
            Self::Document => "document",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Self::Skill => "Skills",
            Self::Guardrail => "Guardrails",
            Self::Document => "Documents",
        }
    }

    /// The body's cap in UTF-8 bytes: a skill's 32,768 (ADR-0080 d9), a
    /// guardrail's 300 (ADR-0037 d7, `BODY_MAX_BYTES`), a document's the
    /// submission cap, 1 MiB (`submission.max_body_size`).
    fn body_cap(self) -> u64 {
        match self {
            Self::Skill => 32_768,
            Self::Guardrail => 300,
            Self::Document => 1_048_576,
        }
    }

    /// What stops being served once a retirement of `name` is approved
    /// (ADR-0082 d3).
    fn stops(self, name: &str) -> String {
        match self {
            Self::Skill => format!(
                "the command <code>/mcp__&lt;server&gt;__{}</code> is no longer offered",
                text(name)
            ),
            Self::Guardrail => "the rule no longer loads".to_owned(),
            Self::Document => "the document is no longer listed or recalled".to_owned(),
        }
    }
}

/// A memory's kind as the builder reads it; `None` for any other kind.
fn kind_of(m: &Memory) -> Option<Kind> {
    m.kind.as_deref().and_then(Kind::of)
}

/// The name an item goes by: a skill's name, a document's title, else its
/// claim key.
fn name_of(m: &Memory) -> String {
    if let Some(skill) = &m.skill {
        return skill.name.clone();
    }
    if let Some(title) = m.document.as_ref().and_then(|d| d.title.clone()) {
        return title;
    }
    m.claim_key
        .strip_prefix("skill/")
        .unwrap_or(&m.claim_key)
        .to_owned()
}

/// The workspace a page acts on: the node as the acting key reads it, its
/// reference, and which of the person's keys acts there.
struct Place {
    view: WorkspaceView,
    reference: String,
    /// `Web` inside the person's root, `Review` outside it (d2).
    capacity: Capacity,
    in_root: bool,
    user: String,
}

impl Place {
    fn page_link(&self, kind: Kind) -> String {
        format!(
            "{WORKSPACE_PATH}?workspace={}&amp;kind={}",
            attr(&encode(&self.reference)),
            kind.as_str()
        )
    }

    fn key_name(&self) -> String {
        format!("{}-{}", self.user, self.capacity.as_str())
    }

    /// Whether a key of the person may start or supersede an item here:
    /// the acting key lists `remember` (ADR-0082 d2, d11).
    fn writes(&self) -> bool {
        self.view.my_effective_verbs.iter().any(|v| v == "remember")
    }

    fn in_progress(&self) -> bool {
        self.view.mode.as_deref() == Some("in_progress")
    }

    /// Who approves under the workspace's policy (d1, d7).
    fn approves(&self, kind: Kind) -> String {
        let k = self.view.policy.as_ref().and_then(|p| p.k).unwrap_or(1);
        let needs = format!(
            "This workspace needs {k} approval{}",
            if k == 1 { "" } else { "s" }
        );
        if self.in_root {
            format!(
                "{needs}: in this root <code>{}-review</code> may give it, marked same user.",
                text(&self.user)
            )
        } else {
            let person = match kind {
                Kind::Skill | Kind::Guardrail => ", a person for a skill or a guardrail",
                Kind::Document => "",
            };
            format!("{needs} from a reviewer outside this account's person{person}.")
        }
    }

    /// What the held version is until approved.
    fn held_line(&self) -> &'static str {
        if self.in_progress() {
            "Held, and served at once to agents connected here, marked unreviewed, until reviewed."
        } else {
            "Held until approved; nothing new is served before then."
        }
    }
}

fn is_id(value: &str) -> bool {
    value.len() == 36 && value.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The node at `sent`, or the person's root where nothing is sent, read
/// as `<user>-web` and, where that key does not read it or it lies
/// outside the root, as `<user>-review` (ADR-0082 d2).
async fn place(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    sent: Option<&str>,
    next: &str,
) -> Result<Place, acts::Refusal> {
    let Some(user) = session.whoami.user.as_ref().map(|u| u.name.clone()) else {
        return Err(Box::new(refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            calls,
        )));
    };
    let sent = sent
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&user)
        .to_owned();
    if Chosen::of(Some(&sent)).refused() {
        return Err(Box::new(refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "workspace is a workspace's name or id, never a path",
            calls,
        )));
    }
    let root = format!("universe.{user}");
    let inside = |path: &str| path == root || path.starts_with(&format!("{root}."));
    let read = |capacity: Capacity| {
        let credential = session.credential(capacity);
        let ids = calls.next_ids();
        let sent = sent.clone();
        async move { web.api.workspace(&call(&credential, &ids), &sent).await }
    };
    // A name in the root (`alice`, `alice.proj`) is read as the web key
    // alone, so a refusal there is the page's; any other may be a shared
    // workspace the review key reads.
    let named_in_root = sent == user || sent.starts_with(&format!("{user}."));
    let view = match read(Capacity::Web).await {
        Ok(view) if inside(&view.path) => Ok((view, Capacity::Web)),
        // A shared workspace the review key does not read opens read-only
        // as the web key read it (ADR-0082 d12 B4; the review of PR #494,
        // 10).
        Ok(view) => match read(Capacity::Review).await {
            Ok(v) => Ok((v, Capacity::Review)),
            Err(e)
                if picker::is_not_found(&e)
                    || matches!(&e, oml_sdk::Error::Api(a) if a.code.starts_with("access.")) =>
            {
                Ok((view, Capacity::Web))
            }
            Err(e) => Err(e),
        },
        Err(e) if picker::is_not_found(&e) && !named_in_root => {
            read(Capacity::Review).await.map(|v| (v, Capacity::Review))
        }
        Err(e) => Err(e),
    };
    let (view, capacity) = view.map_err(|e| Box::new(outcome::page(&e, calls, next)))?;
    let reference = picker::reference(&view.path, |l| l == user);
    Ok(Place {
        in_root: inside(&view.path),
        reference,
        capacity,
        view,
        user,
    })
}

fn kind_field(fields: &Fields, calls: &PageCalls) -> Result<Kind, Box<Response>> {
    match fields.given("kind") {
        None => Ok(Kind::Skill),
        Some(sent) => Kind::of(sent).ok_or_else(|| {
            Box::new(refused(
                StatusCode::BAD_REQUEST,
                "request.validation",
                "kind is skill, guardrail or document",
                calls,
            ))
        }),
    }
}

/// The breadcrumb to `place`, and to `act` below it where given.
fn crumbs(place: &Place, kind: Kind, act: Option<&str>, admin: bool) -> String {
    let (side, list) = if admin {
        ("Administration", ADMIN_WORKSPACES_PATH)
    } else {
        ("Personal", USER_WORKSPACES_PATH)
    };
    let here = text(&place.reference);
    let tail = match act {
        Some(act) => format!(
            r#"<a href="{}">{here}</a> / {}"#,
            if admin {
                admin_link(&place.reference, kind)
            } else {
                place.page_link(kind)
            },
            text(act)
        ),
        None => here,
    };
    format!(
        r#"<nav class="crumbs" aria-label="Breadcrumb">{side} / <a href="{list}">Workspaces</a> / {tail}</nav>"#
    )
}

fn admin_link(reference: &str, kind: Kind) -> String {
    format!(
        "{ADMIN_WORKSPACE_PATH}?workspace={}&amp;kind={}",
        attr(&encode(reference)),
        kind.as_str()
    )
}

/// The kinds as links in `nav.oml-views`, each with its count of approved
/// claims where the API serves the counts: Memories sums the kinds the
/// Memories cockpit lists, from the cockpit's own kind set (ADR-0082 d1,
/// d10).
fn kinds_nav(
    reference: &str,
    current: Kind,
    counts: Option<&BTreeMap<String, u64>>,
    admin: bool,
) -> String {
    let n =
        |sum: Option<u64>| sum.map_or(String::new(), |n| format!(r#" <span class="n">{n}</span>"#));
    let memories = counts.map(|c| {
        c.iter()
            .filter(|(kind, _)| crate::memories::cockpit_lists(kind))
            .map(|(_, n)| n)
            .sum::<u64>()
    });
    let (cockpit, page) = if admin {
        (ADMIN_MEMORIES_PATH, ADMIN_WORKSPACE_PATH)
    } else {
        (USER_MEMORIES_PATH, WORKSPACE_PATH)
    };
    let encoded = attr(&encode(reference));
    let mut out = format!(
        r#"<nav class="oml-views" aria-label="Approved contents by kind"><a href="{cockpit}?workspace={encoded}">Memories{}</a>"#,
        n(memories)
    );
    for kind in Kind::ALL {
        let _ = write!(
            out,
            r#"<a href="{page}?workspace={encoded}&amp;kind={}"{}>{}{}</a>"#,
            kind.as_str(),
            if kind == current {
                r#" aria-current="page""#
            } else {
                ""
            },
            kind.plural(),
            n(counts.map(|c| c.get(kind.as_str()).copied().unwrap_or(0)))
        );
    }
    out.push_str("</nav>");
    out.push_str(
        r#"<p class="desc">Counts are approved items, one per claim, on this workspace alone.</p>"#,
    );
    out
}

/// One version of the kind's table, with what the person may do with it.
struct Row {
    m: Memory,
    /// A key of the person wrote it (`mine`).
    own: bool,
    /// The person's review key may judge it (the held listing).
    judge: bool,
}

/// The table's rows: the listing's, marked own where `mine` lists them and
/// judgeable where the held listing does, on this workspace alone and of
/// this kind, grouped by claim, each claim's versions newest first.
fn rows(
    workspace_id: &str,
    kind: Kind,
    listed: Vec<Memory>,
    mine: Vec<Memory>,
    held: Vec<Memory>,
) -> Vec<Vec<Row>> {
    let mut by_id: BTreeMap<String, Row> = BTreeMap::new();
    let here =
        |m: &Memory| m.workspace_id.as_deref() == Some(workspace_id) && kind_of(m) == Some(kind);
    for m in listed.into_iter().filter(here) {
        by_id.insert(
            m.artifact_id.clone(),
            Row {
                m,
                own: false,
                judge: false,
            },
        );
    }
    for m in mine.into_iter().filter(here) {
        by_id
            .entry(m.artifact_id.clone())
            .or_insert(Row {
                m,
                own: false,
                judge: false,
            })
            .own = true;
    }
    for m in held.into_iter().filter(here) {
        by_id
            .entry(m.artifact_id.clone())
            .or_insert(Row {
                m,
                own: false,
                judge: false,
            })
            .judge = true;
    }
    let mut claims: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for row in by_id.into_values() {
        claims.entry(row.m.claim_key.clone()).or_default().push(row);
    }
    let mut out: Vec<Vec<Row>> = claims.into_values().collect();
    for claim in &mut out {
        claim.sort_by_key(|r| std::cmp::Reverse(r.m.version));
    }
    out.sort_by_key(|claim| name_of(&claim[0].m));
    out
}

/// A held version's decision word (WorkspaceScreen): New skill, Edit of
/// v2, Retirement of v1.
fn decision(m: &Memory, kind: Kind) -> String {
    match &m.replaces {
        Some(r) if m.retirement => format!("Retirement of v{}", r.version),
        Some(r) => format!("Edit of v{}", r.version),
        None => format!("New {}", kind.as_str()),
    }
}

/// A rule's match member and pattern, a shell command's words joined by
/// spaces (`oml-gr/1`, ADR-0037 d8).
fn matcher(rule: &Value) -> Option<(&'static str, String)> {
    ["command", "path", "host", "name"]
        .into_iter()
        .find_map(|m| match &rule[m] {
            Value::String(p) => Some((m, p.clone())),
            Value::Array(words) => Some((
                m,
                words
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            )),
            _ => None,
        })
}

/// A rule's fields in words: tool, what it matches and its effect.
fn rule_words(rule: Option<&Value>) -> String {
    let Some(rule) = rule.filter(|r| r.is_object()) else {
        return "—".to_owned();
    };
    let tool = rule["tool"].as_str().unwrap_or("?");
    let (member, pattern) = matcher(rule).unwrap_or(("match", "?".to_owned()));
    format!(
        "{} · {member} <code>{}</code> · {}",
        text(tool),
        text(&pattern),
        text(rule["effect"].as_str().unwrap_or("?"))
    )
}

fn day(at: &str) -> &str {
    at.get(..10).unwrap_or(at)
}

/// The table of one kind's claims on the workspace, with each version's
/// act (ADR-0082 d1; ADR-0083 d13).
fn table(place: &Place, kind: Kind, claims: &[Vec<Row>], admin: bool) -> String {
    let guardrail = kind == Kind::Guardrail;
    let mut out = String::from(if admin {
        r#"<table class="oml-stack t-items t-lineage"><tr><th>Name</th><th>State</th>"#
    } else {
        r#"<table class="oml-stack t-items t-acts"><tr><th>Name</th><th>State</th>"#
    });
    // The admin search serves no severity, rule or retirement mark: its
    // guardrail rows draw the version columns rather than two that could
    // only read "—" (the review of PR #494, 17).
    let rule_columns = guardrail && !admin;
    out.push_str(if rule_columns {
        "<th>Severity</th><th>Rule</th>"
    } else {
        "<th>Version</th><th>Changed</th>"
    });
    out.push_str(if admin {
        "<th>Lineage</th></tr>"
    } else {
        "<th>Act</th></tr>"
    });
    let one = if admin {
        ADMIN_MEMORY_PATH
    } else {
        USER_MEMORY_PATH
    };
    for claim in claims {
        for row in claim {
            let m = &row.m;
            let id = attr(&encode(&m.artifact_id));
            let mut named = format!(r#"<a href="{one}?id={id}">{}</a>"#, text(&name_of(m)));
            match kind {
                Kind::Skill => {
                    if let Some(skill) = &m.skill {
                        let _ = write!(
                            named,
                            r#"<br><span class="dim">{}</span>"#,
                            text(&skill.description)
                        );
                    }
                    if m.state == "promoted" && m.tip && !m.retirement {
                        let _ = write!(
                            named,
                            r#"<br><code>/mcp__&lt;server&gt;__{}</code>"#,
                            text(&name_of(m))
                        );
                    }
                }
                Kind::Document => {
                    let _ = write!(
                        named,
                        r#"<br><span class="dim mono">{}</span>"#,
                        text(&m.claim_key)
                    );
                }
                Kind::Guardrail => {
                    if let Some(body) = m.text.as_deref().filter(|_| !m.retirement) {
                        let _ = write!(named, r#"<br><span class="dim">{}</span>"#, text(body));
                    }
                }
            }
            let held = m.state == "held";
            // A promoted version that is no longer the tip reads superseded,
            // as the card names it (ADR-0045 d21).
            let mut state = state_tag(crate::card::row_state(&m.state, m.tip));
            if held {
                let _ = write!(state, " {}", text(&decision(m, kind)));
                if place.in_progress() {
                    let _ = write!(
                        state,
                        " {} served here, unreviewed",
                        state_tag("unreviewed")
                    );
                }
            } else if m.retirement {
                state.push_str(" retirement");
            }
            let middle = if rule_columns {
                format!(
                    "<td>{}</td><td>{}</td>",
                    text(m.severity.as_deref().unwrap_or("—")),
                    rule_words(m.rule.as_ref())
                )
            } else {
                format!(
                    "<td>v{}</td><td>{}</td>",
                    m.version,
                    text(day(m.promoted_at.as_deref().unwrap_or(&m.created_at)))
                )
            };
            let last = if admin {
                match &m.replaces {
                    Some(r) if m.retirement => format!("retires v{}", r.version),
                    Some(r) => format!("replaces v{}", r.version),
                    // The admin search serves no `replaces`: a later version
                    // follows the one before it in the claim's ordinals.
                    None if m.version > 1 => format!("follows v{}", m.version - 1),
                    None => "first version".to_owned(),
                }
            } else {
                act(place, kind, claim, row)
            };
            let _ = write!(
                out,
                r#"<tr><td>{named}</td><td>{state}</td>{middle}<td>{last}</td></tr>"#
            );
        }
    }
    out.push_str("</table>");
    out
}

/// A row's act on the Personal side: Edit and Retire on an approved tip
/// with nothing held after it where a key of the person may supersede it;
/// otherwise the line naming the pending version; Judge it where the
/// review key may judge a held version; Withdraw on the person's own held
/// version, which opens its confirmation (ADR-0082 d1, d2; ADR-0083 d13).
fn act(place: &Place, kind: Kind, claim: &[Row], row: &Row) -> String {
    let m = &row.m;
    let id = attr(&encode(&m.artifact_id));
    if m.state == "promoted" && m.tip && !m.retirement {
        let pending = claim
            .iter()
            .find(|r| r.m.state == "held" && r.m.version > m.version);
        return match pending {
            Some(p) => format!(
                r#"in force · <a href="{USER_MEMORY_PATH}?id={}">{} v{} waits for review</a>"#,
                attr(&encode(&p.m.artifact_id)),
                if p.m.retirement { "retirement" } else { "edit" },
                p.m.version
            ),
            None if place.writes() => format!(
                r#"<a class="button" href="{EDIT_PATH}?memory={id}">Edit</a> <a class="button" href="{RETIRE_PATH}?memory={id}">Retire</a>"#
            ),
            None => "in force".to_owned(),
        };
    }
    if m.state == "held" {
        let mut acts = Vec::new();
        if row.judge {
            acts.push(format!(
                r#"<a class="button" href="{TODO_JUDGE_PATH}?memory={id}">Judge it</a>"#
            ));
        }
        if row.own {
            acts.push(format!(
                r#"<a href="{USER_MEMORY_PATH}?id={id}&amp;act=withdraw">Withdraw</a>"#
            ));
        }
        if acts.is_empty() {
            acts.push(format!("waits for review · {}", kind.as_str()));
        }
        return acts.join(" ");
    }
    String::new()
}

/// The Guardrails kind's opening panel: every guardrail the session key
/// here loads, the inherited first, then this workspace's own, with the
/// counts under the key's guardrail scope (ADR-0083 d12).
fn in_force(place: &Place, set: &GuardrailSet, claims: &[Vec<Row>]) -> String {
    let here: Vec<_> = set
        .guardrails
        .iter()
        .filter(|g| g.workspace_path.as_deref() == Some(place.view.path.as_str()))
        .collect();
    let inherited: Vec<_> = set
        .guardrails
        .iter()
        .filter(|g| g.workspace_path.as_deref() != Some(place.view.path.as_str()))
        .collect();
    let retiring = |claim_key: &str| {
        claims
            .iter()
            .flatten()
            .any(|r| r.m.claim_key == claim_key && r.m.state == "held" && r.m.retirement)
    };
    // The set is read under the session key, so the line names that key,
    // never an agent's, whose guardrail scope may differ (the review of PR
    // #494, 14).
    let key = text(&place.key_name());
    let counts = match set.guardrail_scope.as_str() {
        "workspace" => format!(
            "<code>{key}</code> loads the {} from here only: its guardrail scope is <code>workspace</code>.",
            here.len()
        ),
        scope => format!(
            "<code>{key}</code> loads {} inherited and {} from here, at most {}: its guardrail scope is <code>{}</code>. An agent's key loads by its own scope.",
            inherited.len(),
            here.len(),
            oml_guardrail_cap(),
            text(scope)
        ),
    };
    let mut out = format!(
        r#"<section class="card" aria-labelledby="in-force"><h2 id="in-force">In force here</h2><p class="desc">{counts}</p>"#
    );
    if set.over_cap {
        out.push_str(r#"<p class="error">Over the cap: no partial set loads here.</p>"#);
    }
    if set.guardrails.is_empty() {
        out.push_str(r#"<p class="dim">No guardrail applies here.</p>"#);
    } else {
        out.push_str("<ul>");
        for g in inherited.iter().chain(&here) {
            let path = g.workspace_path.as_deref().unwrap_or("");
            let from = if path == place.view.path {
                if retiring(&g.claim_key) {
                    "here · binding while its retirement waits".to_owned()
                } else {
                    "here".to_owned()
                }
            } else {
                let reference = picker::reference(path, |l| l == place.user);
                format!(
                    r#"<a href="{WORKSPACE_PATH}?workspace={}&amp;kind=guardrail">inherited from {}</a>"#,
                    attr(&encode(&reference)),
                    text(&reference)
                )
            };
            let _ = write!(
                out,
                r#"<li>{}<strong>{}</strong> {} <span class="dim">· <code>{}</code> v{} · {from}</span></li>"#,
                if g.unreviewed {
                    format!("{} ", state_tag("unreviewed"))
                } else {
                    String::new()
                },
                text(&g.severity),
                text(&g.text),
                text(&g.claim_key),
                g.version,
            );
        }
        out.push_str("</ul>");
    }
    out.push_str("</section>");
    out
}

/// ADR-0037 d7's set cap, as the preview serves it beside a size.
const fn oml_guardrail_cap() -> u64 {
    12
}

/// Every item of one listing page, or none where the call failed: the
/// table names the failed read beside what the others served.
type Listed = Result<(Vec<Memory>, bool), String>;

async fn listing(web: &Web, credential: &str, calls: &PageCalls, query: MemoryQuery<'_>) -> Listed {
    let ids = calls.next_ids();
    let request_id = ids.request_id.clone().unwrap_or_default();
    web.api
        .list_memories(&call(credential, &ids), &query)
        .await
        .map(|page| (page.items, page.next_cursor.is_some()))
        .map_err(|_| request_id)
}

/// `GET /web/workspace`: the workspace page (ADR-0082 d1; ADR-0083 d12).
pub async fn user_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let next = uri
        .path_and_query()
        .map_or(WORKSPACE_PATH, |pq| pq.as_str());
    let kind = match kind_field(&fields, &calls) {
        Ok(kind) => kind,
        Err(refusal) => return *refusal,
    };
    let place = match place(&web, &session, &calls, fields.get("workspace"), next).await {
        Ok(place) => place,
        Err(refusal) => return *refusal,
    };
    let Some(root) = session.whoami.user.as_ref().map(|u| u.id.clone()) else {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        );
    };
    let acting = session.credential(place.capacity);
    let personal = session.credential(Capacity::Web);
    let review = session.credential(Capacity::Review);
    let id = place.view.id.clone();
    // The `mine` read serves only the person's held rows, the ones Withdraw
    // reads `own` on (ADR-0082 d12 B5; the review of PR #494, 9).
    let query = |mine: bool| MemoryQuery {
        mine,
        workspace: Some(&id),
        kinds: Some(kind.as_str()),
        state: mine.then_some("held"),
        limit: LIST_LIMIT,
        ..MemoryQuery::default()
    };
    // The kind's versions as the acting key lists them, the person's own
    // among them (`mine`), the held ones the review key may judge, and for
    // guardrails the set an agent connected here loads: read together
    // after the node (ADR-0070 d8).
    let held = async {
        let ids = calls.next_ids();
        let request_id = ids.request_id.clone().unwrap_or_default();
        match web
            .api
            .review_held(
                &call(&review, &ids),
                &[("workspace", &id)],
                LIST_LIMIT,
                None,
            )
            .await
        {
            Ok(page) => {
                Ok::<Vec<Memory>, String>(page.items.iter().map(|h| h.memory().clone()).collect())
            }
            // A review key that holds no reviewer here judges nothing.
            Err(oml_sdk::Error::Api(e)) if e.code.starts_with("access.") => Ok(Vec::new()),
            Err(_) => Err(request_id),
        }
    };
    let set = async {
        if kind == Kind::Guardrail {
            let ids = calls.next_ids();
            Some(web.api.guardrails(&call(&acting, &ids), Some(&id)).await)
        } else {
            None
        }
    };
    let user = place.user.clone();
    let (listed, mine, held, set, names) = tokio::join!(
        listing(&web, &acting, &calls, query(false)),
        listing(&web, &personal, &calls, query(true)),
        held,
        set,
        Names::read(
            &web,
            &personal,
            &calls,
            &root,
            Some(&user),
            picker::HINTS_PAGES
        ),
    );
    let mut failed = Vec::new();
    let mut more = false;
    let mut take = |read: Listed| match read {
        Ok((items, next)) => {
            more |= next;
            items
        }
        Err(request_id) => {
            failed.push(request_id);
            Vec::new()
        }
    };
    let (listed, mine) = (take(listed), take(mine));
    let held = held.unwrap_or_else(|request_id| {
        failed.push(request_id);
        Vec::new()
    });
    let claims = rows(&id, kind, listed, mine, held);

    let chosen = Chosen::Place(picker::Place {
        id: place.view.id.clone(),
        name: place.view.name.clone(),
        path: place.view.path.clone(),
        reference: place.reference.clone(),
    });
    let keep = [("kind", kind.as_str().to_owned())];
    let field = Field {
        id: "wa",
        action: WORKSPACE_PATH,
        keep: &keep,
        chosen: &chosen,
        names: &names,
        browse: None,
        admin: false,
        standalone: true,
        empty: "Empty: the user root.",
    }
    .html();
    let mut body = crumbs(&place, kind, None, false);
    let _ = write!(body, r#"<h1 class="mono">{}</h1>"#, text(&place.reference));
    body.push_str(&field);
    body.push_str(&kinds_nav(
        &place.reference,
        kind,
        place.view.approved_by_kind.as_ref(),
        false,
    ));
    if place.in_progress() {
        body.push_str(r#"<p class="desc">Drafts visible to everyone working here: held items are served to agents connected to this workspace, marked unreviewed, until reviewed.</p>"#);
    }
    if kind == Kind::Guardrail {
        match set {
            Some(Ok(set)) => body.push_str(&in_force(&place, &set, &claims)),
            Some(Err(e)) => {
                let _ = write!(
                    body,
                    r#"<p class="error" role="status">In force here is unavailable: {}</p>"#,
                    text(&e.to_string())
                );
            }
            None => {}
        }
        body.push_str("<h2>This workspace's guardrails</h2>");
    }
    match kind {
        Kind::Skill => body.push_str(r#"<p class="desc">An approved skill is offered to every agent that reads this workspace as a slash command, loaded live from the server.</p>"#),
        Kind::Guardrail => body.push_str(r#"<p class="desc">A guardrail needs a verdict by a key other than its author's, a person's, before it loads.</p>"#),
        Kind::Document => {}
    }
    if place.writes() {
        let _ = write!(
            body,
            r#"<p><a class="button primary" href="{NEW_PATH}?workspace={}&amp;kind={}">New {}</a></p><p class="desc">Proposed under <code>{}</code>. {} {}</p>"#,
            attr(&encode(&place.reference)),
            kind.as_str(),
            kind.as_str(),
            text(&place.key_name()),
            place.held_line(),
            place.approves(kind),
        );
    }
    // The notice after Propose names the version held and links Judge it
    // where the person's review key may judge it (the review of PR #494,
    // 16); a version the page does not list keeps the plain notice.
    let named = fields
        .get("held")
        .zip(notice(fields.get("done")))
        .and_then(|(held, words)| {
            let row = claims.iter().flatten().find(|r| r.m.artifact_id == held)?;
            let judge = if row.judge {
                format!(
                    r#" <a class="button" href="{TODO_JUDGE_PATH}?memory={}">Judge it</a>"#,
                    attr(&encode(&row.m.artifact_id))
                )
            } else {
                String::new()
            };
            Some(format!(
                r#"<p class="notice" role="status"><b>{}</b> v{}. {}{judge}</p>"#,
                text(&name_of(&row.m)),
                row.m.version,
                text(words)
            ))
        });
    if let Some(named) = &named {
        body.insert_str(0, named);
    }
    if !failed.is_empty() {
        let _ = write!(
            body,
            r#"<p class="error" role="status">Part of this list is unavailable (request <code>{}</code>).</p>"#,
            text(&failed.join(", "))
        );
    }
    if claims.is_empty() {
        let _ = write!(
            body,
            r#"<p class="dim">No {} in {}.</p>"#,
            kind.as_str(),
            text(&place.reference)
        );
    } else {
        body.push_str(&table(&place, kind, &claims, false));
        if more {
            let _ = write!(
                body,
                r#"<p class="desc">The first {LIST_LIMIT} versions are listed.</p>"#
            );
        }
    }
    outcome::html(
        StatusCode::OK,
        shell(
            &place.reference,
            &session.personal(),
            USER_WORKSPACES_PATH,
            named
                .is_none()
                .then(|| notice(fields.get("done")))
                .flatten(),
            &body,
        ),
    )
}

/// The notice a write answers with (Post/Redirect/Get).
fn notice(done: Option<&str>) -> Option<&'static str> {
    match done? {
        "proposed" => Some(
            "Proposed: the new version is held until it is approved; the version in force stays served until then.",
        ),
        "retirement" => Some(
            "Proposed: the retirement is held until it is approved; the item stays in force until then.",
        ),
        _ => None,
    }
}

/// `GET /web/admin/workspace`: the same contents by kind, read-only, with
/// no builder act: an admin key writes no claim (ADR-0082 d1).
pub async fn admin_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if !session.admin {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        );
    }
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let next = uri
        .path_and_query()
        .map_or(ADMIN_WORKSPACE_PATH, |pq| pq.as_str());
    let kind = match kind_field(&fields, &calls) {
        Ok(kind) => kind,
        Err(refusal) => return *refusal,
    };
    let Some(sent) = fields.given("workspace") else {
        return acts::invalid("workspace names the workspace to read", &calls);
    };
    if Chosen::of(Some(sent)).refused() {
        return acts::invalid(
            "workspace is a workspace's name or id, never a path",
            &calls,
        );
    }
    let admin = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    let view = match web.api.workspace(&call(&admin, &ids), sent).await {
        Ok(view) => view,
        Err(e) => return outcome::page(&e, &calls, next),
    };
    let ids = calls.next_ids();
    let search = AdminSearch {
        workspace: Some(&view.id),
        kind: Some(kind.as_str()),
        limit: LIST_LIMIT,
        ..AdminSearch::default()
    };
    let found = match web.api.admin_search(&call(&admin, &ids), &search).await {
        Ok(found) => found,
        Err(e) => return outcome::page(&e, &calls, next),
    };
    let reference = picker::reference(&view.path, |l| {
        view.user_root && view.path == format!("universe.{l}")
    });
    // The admin read names no skill or document field: each row goes by
    // its claim key, on this workspace alone.
    let listed: Vec<Memory> = found
        .items
        .iter()
        .filter(|m| m.workspace_path == view.path)
        .filter_map(|m| {
            serde_json::from_value(json!({
                "artifact_id": m.artifact_id, "claim_key": m.claim_key, "version": m.version,
                // The admin search's `promoted` is the served tip (the
                // review of PR #494, C-B1).
                "tip": m.state == "promoted", "workspace_id": view.id, "workspace_path": m.workspace_path,
                "state": m.state, "principal": null, "created_at": m.created_at,
                "promoted_at": m.promoted_at, "kind": kind.as_str(), "text": m.text,
            }))
            .ok()
        })
        .collect();
    let place = Place {
        view,
        reference,
        capacity: Capacity::Admin,
        in_root: false,
        user: String::new(),
    };
    let claims = rows(&place.view.id, kind, listed, Vec::new(), Vec::new());
    let mut body = crumbs(&place, kind, None, true);
    let _ = write!(body, r#"<h1 class="mono">{}</h1>"#, text(&place.reference));
    body.push_str(&kinds_nav(
        &place.reference,
        kind,
        place.view.approved_by_kind.as_ref(),
        true,
    ));
    let user = session.whoami.user.as_ref().map_or("", |u| u.name.as_str());
    let _ = write!(
        body,
        r#"<p class="desc">Read as <code>{}-admin</code>, journalled. An admin key writes no claim: nothing here proposes, edits or retires.</p>"#,
        text(user)
    );
    if claims.is_empty() {
        let _ = write!(
            body,
            r#"<p class="dim">No {} in {}.</p>"#,
            kind.as_str(),
            text(&place.reference)
        );
    } else {
        body.push_str(&table(&place, kind, &claims, true));
    }
    outcome::html(
        StatusCode::OK,
        shell(
            &place.reference,
            &session.administration(),
            ADMIN_WORKSPACES_PATH,
            None,
            &body,
        ),
    )
}

// ---------------------------------------------------------------------------
// The forms (ADR-0082 d1, d3, d4, d5, d6)
// ---------------------------------------------------------------------------

/// Which form: a new item, the next version of one, or its retirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    New,
    Edit,
    Retire,
}

impl Act {
    fn path(self) -> &'static str {
        match self {
            Self::New => NEW_PATH,
            Self::Edit => EDIT_PATH,
            Self::Retire => RETIRE_PATH,
        }
    }

    fn action(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Edit => "edit",
            Self::Retire => "retire",
        }
    }
}

/// The fields a form takes beside `action`, `csrf_token` and `nonce`.
const FORM_FIELDS: &[&str] = &[
    "step",
    "workspace",
    "kind",
    "memory",
    "expected",
    "previewed",
    "name",
    "description",
    "body",
    "severity",
    "tool",
    "pattern",
    "effect",
    "title",
    "format",
    "claim_key",
    "reason",
    "eol",
];

/// What the person typed, kept through every redraw.
#[derive(Debug, Clone, Default)]
struct Values {
    name: String,
    description: String,
    body: String,
    severity: String,
    tool: String,
    pattern: String,
    effect: String,
    title: String,
    format: String,
    claim_key: String,
    reason: String,
    /// A new guardrail's claim key, slugged from its name (B2).
    guardrail_key: String,
    /// How the stored body breaks its lines, which a textarea cannot
    /// carry: `crlf`, `mixed`, or empty for LF alone (C-B5).
    eol: String,
}

/// The lead of every claim key the server derives, which a writer may not
/// send (ADR-0039 d6; `oml_domain::DERIVED_CLAIM_KEY_PREFIX`, which the
/// console's wall keeps out of reach).
const DERIVED_KEY_LEAD: &str = "auto-";

/// A claim key from a name, by the slug rule a document's key takes from
/// its title (ADR-0081 d2): lower case, letters and digits of `a-z 0-9`
/// kept, `.`/`_` kept, every run of anything else one `-`, no leading or
/// trailing `-`; past 64 characters the first 47, `-`, and the first 16
/// hex digits of the SHA-256 of the whole slug. Empty when nothing is left.
/// An `auto-` lead, the prefix only the server derives (ADR-0039 d6), is
/// stripped, so "Auto deploy" is `deploy` rather than a key the API
/// refuses (the review of PR #494, re-verification).
fn key_slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug
        .trim_end_matches('-')
        .trim_start_matches(DERIVED_KEY_LEAD)
        .to_owned();
    if slug.len() <= 64 {
        return slug;
    }
    let digest = ring::digest::digest(&ring::digest::SHA256, slug.as_bytes());
    let hex: String = digest.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{}-{hex}", &slug[..47])
}

/// How `body` breaks its lines: empty for LF alone, `crlf` for CR LF
/// alone, `mixed` for anything else with a CR (the review of PR #494,
/// C-B5).
fn eol_of(body: &str) -> &'static str {
    if !body.contains('\r') {
        ""
    } else if body.replace("\r\n", "").contains(['\r', '\n']) {
        "mixed"
    } else {
        "crlf"
    }
}

impl Values {
    fn of(fields: &Fields) -> Self {
        let v = |n: &str| fields.get(n).unwrap_or_default().to_owned();
        // A browser sends every line break of a textarea as CRLF: the body
        // is read as LF, and as CRLF again where the stored body was so
        // the edit keeps its bytes (C-B5).
        let eol = match v("eol").as_str() {
            "crlf" => "crlf",
            "mixed" => "mixed",
            _ => "",
        };
        let body = v("body").replace("\r\n", "\n");
        let name = v("name").trim().to_owned();
        Self {
            guardrail_key: key_slug(&name),
            name: v("name").trim().to_owned(),
            description: v("description"),
            body: if eol == "crlf" {
                body.replace('\n', "\r\n")
            } else {
                body
            },
            eol: eol.to_owned(),
            severity: v("severity"),
            tool: v("tool"),
            pattern: v("pattern"),
            effect: v("effect"),
            title: v("title"),
            format: v("format"),
            claim_key: v("claim_key").trim().to_owned(),
            reason: v("reason").replace("\r\n", "\n"),
        }
    }

    /// The fields of `m`, the version an edit starts from.
    fn of_memory(m: &Memory) -> Self {
        let rule = m.rule.as_ref().filter(|r| r.is_object());
        let pattern = rule.and_then(matcher).map(|(_, p)| p).unwrap_or_default();
        Self {
            name: name_of(m),
            description: m
                .skill
                .as_ref()
                .map(|s| s.description.clone())
                .unwrap_or_default(),
            body: m.text.clone().unwrap_or_default(),
            severity: m.severity.clone().unwrap_or_default(),
            tool: rule
                .and_then(|r| r["tool"].as_str())
                .unwrap_or_default()
                .to_owned(),
            pattern,
            effect: rule
                .and_then(|r| r["effect"].as_str())
                .unwrap_or_default()
                .to_owned(),
            title: m
                .document
                .as_ref()
                .and_then(|d| d.title.clone())
                .unwrap_or_default(),
            format: m.format.clone().unwrap_or_default(),
            claim_key: m.claim_key.clone(),
            reason: String::new(),
            eol: eol_of(m.text.as_deref().unwrap_or_default()).to_owned(),
            guardrail_key: String::new(),
        }
    }

    /// A digest of what a write would send, so Propose writes only the
    /// values a preview answered: keyed by the form's preview key, which
    /// only the session can compute and which is not the act's
    /// `Idempotency-Key`, over the workspace and the expected version too
    /// (the review of PR #494, 7).
    #[allow(clippy::too_many_arguments)]
    fn digest(
        &self,
        kind: Kind,
        act: Act,
        target: &str,
        workspace: &str,
        expected: &str,
        key: &str,
    ) -> String {
        let joined = [
            kind.as_str(),
            act.action(),
            target,
            workspace,
            expected,
            &self.name,
            &self.description,
            &self.body,
            &self.severity,
            &self.tool,
            &self.pattern,
            &self.effect,
            &self.title,
            &self.format,
            &self.claim_key,
            &self.reason,
        ]
        .join("\u{0}");
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes());
        let digest = ring::hmac::sign(&key, joined.as_bytes());
        digest.as_ref()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// The rule the three rule fields name, `None` where no tool is chosen,
    /// or why it is not one: `oml-gr/1`'s one member per tool class
    /// (ADR-0037 d8).
    fn rule(&self) -> Result<Option<Value>, String> {
        let member = match self.tool.as_str() {
            "" => return Ok(None),
            "shell" => "command",
            "edit" | "read" => "path",
            "fetch" => "host",
            "mcp" => "name",
            _ => return Err("tool is shell, edit, read, fetch or mcp".to_owned()),
        };
        if self.pattern.trim().is_empty() {
            return Err(format!("a {} rule names its {member}", self.tool));
        }
        // A shell rule matches the command's words, one glob each.
        let pattern = if member == "command" {
            json!(self.pattern.split_whitespace().collect::<Vec<_>>())
        } else {
            json!(self.pattern.trim())
        };
        Ok(Some(json!({
            "rule": "oml-gr/1",
            "tool": self.tool,
            member: pattern,
            "effect": self.effect,
        })))
    }

    fn format_or(&self, kind: Kind) -> &str {
        match kind {
            Kind::Document if self.format == "text/plain" => "text/plain",
            _ => "text/markdown",
        }
    }
}

/// A refusal drawn beside its field, leading with the sentence and ending
/// with the code (d5).
#[derive(Debug, Clone)]
struct Refusal {
    code: String,
    message: String,
    field: &'static str,
    details: Option<Value>,
}

impl Refusal {
    fn of(code: &str, message: &str, details: Option<&Value>, kind: Kind) -> Self {
        let sent = details
            .and_then(|d| d["field"].as_str())
            .unwrap_or_default();
        let name = if kind == Kind::Document {
            "claim_key"
        } else {
            "name"
        };
        let field = match (code, sent) {
            ("submission.tip_conflict" | "submission.version_conflict", _) => "form",
            ("submission.claim_key_conflict" | "name.taken", _) => name,
            ("submission.scan_veto", _) if message.contains("guardrail.") => "pattern",
            ("request.payload_too_large" | "submission.scan_veto", _) => "body",
            (_, f) if f.contains("rule") || code.starts_with("guardrail.") => "pattern",
            (_, f) if f.contains("description") => "description",
            (_, f) if f.contains("name") || f.contains("claim_key") => name,
            (_, f) if f.contains("title") => "title",
            (_, f) if f.contains("severity") => "severity",
            (_, f) if f.contains("format") => "format",
            (_, f) if f.contains("body") => "body",
            _ => "form",
        };
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            field,
            details: details.cloned(),
        }
    }

    fn local(field: &'static str, message: &str) -> Self {
        Self {
            code: "request.validation".to_owned(),
            message: message.to_owned(),
            field,
            details: None,
        }
    }

    fn html(&self) -> String {
        format!(
            r#"<p class="field-error" id="err-{}" role="alert">{} <code>{}</code></p>"#,
            self.field,
            text(&self.message),
            text(&self.code)
        )
    }

    /// The tip the edit's target was replaced by, where the API names one
    /// the key reads (`submission.tip_conflict`).
    fn current_tip(&self) -> Option<(String, u64)> {
        let tip = &self.details.as_ref()?["current_tip"];
        Some((
            tip["artifact_id"].as_str()?.to_owned(),
            tip["version"].as_u64()?,
        ))
    }

    /// The version the claim moved to: the tip a `submission.tip_conflict`
    /// names, or the latest version a `submission.version_conflict` names.
    fn moved_to(&self) -> Option<u64> {
        match self.code.as_str() {
            "submission.tip_conflict" => self.current_tip().map(|(_, v)| v),
            "submission.version_conflict" => self.details.as_ref()?["current_version"].as_u64(),
            _ => None,
        }
    }
}

/// Everything a form page draws.
struct Form<'a> {
    act: Act,
    kind: Kind,
    place: &'a Place,
    /// The version an edit or a retirement starts from.
    target: Option<&'a Memory>,
    values: &'a Values,
    nonce: &'a str,
    preview: Option<&'a MemoryPreview>,
    refusal: Option<&'a Refusal>,
}

impl Form<'_> {
    fn title(&self) -> String {
        match (self.act, self.target) {
            (Act::New, _) | (_, None) => format!("New {}", self.kind.as_str()),
            (Act::Edit, Some(t)) => format!("Edit {}", name_of(t)),
            (Act::Retire, Some(t)) => format!("Retire {}", name_of(t)),
        }
    }

    /// The one primary after a preview, named for the version it produces.
    fn propose(&self) -> String {
        match (self.act, self.target) {
            (Act::New, _) | (_, None) => format!("Propose {}", self.kind.as_str()),
            (Act::Edit, Some(t)) => format!("Propose v{}", t.version + 1),
            (Act::Retire, _) => "Propose retirement".to_owned(),
        }
    }

    fn previewed(&self) -> bool {
        self.preview.is_some_and(|p| p.would_capture) && self.refusal.is_none()
    }

    fn error(&self, field: &str) -> String {
        self.refusal
            .filter(|r| r.field == field)
            .map_or(String::new(), Refusal::html)
    }

    fn input(&self, name: &str, label: &str, value: &str, hint: &str, required: bool) -> String {
        let described = if self.refusal.is_some_and(|r| r.field == name) {
            format!(r#" aria-describedby="err-{name}" aria-invalid="true""#)
        } else {
            String::new()
        };
        format!(
            r#"<label>{}<input name="{name}" value="{}"{}{described}></label>{}{}"#,
            text(label),
            attr(value),
            if required { " required" } else { "" },
            if hint.is_empty() {
                String::new()
            } else {
                format!(r#"<p class="desc">{hint}</p>"#)
            },
            self.error(name)
        )
    }

    fn textarea(&self, name: &str, label: &str, value: &str, hint: &str) -> String {
        format!(
            // The line break after `<textarea>` is the one an HTML parser
            // drops, so a body that starts with its own keeps it.
            "<label>{}<textarea name=\"{name}\" required>\n{}</textarea></label>{}{}",
            text(label),
            // HTML escaped alone: a CR or a bidi mark is the text's own and
            // round-trips; `text` would turn it into U+FFFD (C-B5).
            oml_page::escape(value),
            if hint.is_empty() {
                String::new()
            } else {
                format!(r#"<p class="desc">{hint}</p>"#)
            },
            self.error(name)
        )
    }

    fn radios(&self, name: &str, legend: &str, options: &[(&str, &str)], chosen: &str) -> String {
        let mut out = format!("<fieldset><legend>{}</legend>", text(legend));
        for (value, words) in options {
            let _ = write!(
                out,
                r#"<label class="choice"><input type="radio" name="{name}" value="{value}"{} required> {}</label>"#,
                if *value == chosen { " checked" } else { "" },
                text(words)
            );
        }
        out.push_str("</fieldset>");
        out.push_str(&self.error(name));
        out
    }

    fn choice(&self, name: &str, label: &str, options: &[(&str, &str)], chosen: &str) -> String {
        let mut out = format!(r#"<label>{}<select name="{name}">"#, text(label));
        for (value, words) in options {
            let _ = write!(
                out,
                r#"<option value="{value}"{}>{}</option>"#,
                if *value == chosen { " selected" } else { "" },
                text(words)
            );
        }
        out.push_str("</select></label>");
        out
    }

    fn bytes(&self) -> String {
        let cap = self.kind.body_cap();
        match self.preview.and_then(|p| p.body_bytes) {
            Some(n) => format!("{n} of {cap} bytes (UTF-8)."),
            None => format!("At most {cap} bytes (UTF-8)."),
        }
    }

    /// The kind's fields (ADR-0082 d4), and a retirement's reason.
    fn fields(&self) -> String {
        let v = self.values;
        if self.act == Act::Retire {
            return self.textarea(
                "reason",
                "Reason",
                &v.reason,
                "Required: it is the retirement version's text.",
            );
        }
        let mut out = String::new();
        match self.kind {
            Kind::Skill => {
                if self.act == Act::Edit {
                    let _ = write!(
                        out,
                        r#"<p><span class="label">Name</span> <code>{}</code> <span class="dim">fixed for every version; a renamed skill is a new skill and a retirement of this one.</span></p>{}"#,
                        text(&v.name),
                        hidden("name", &v.name)
                    );
                } else {
                    out.push_str(&self.input(
                        "name",
                        "Name",
                        &v.name,
                        "Lower-case letters, digits and hyphens; it becomes the command <code>/mcp__&lt;server&gt;__&lt;name&gt;</code>, where the server is whatever the client named it, usually <code>oml</code>, and is fixed for every version.",
                        true,
                    ));
                }
                out.push_str(&self.input(
                    "description",
                    "Description",
                    &v.description,
                    "One line.",
                    true,
                ));
                out.push_str(&self.textarea("body", "Body (markdown)", &v.body, &self.bytes()));
            }
            Kind::Guardrail => {
                // B2 (ADR-0082 d4, amended 2026-11-19): an optional name,
                // the claim key's source; fixed once the claim exists.
                if self.act == Act::New {
                    out.push_str(&self.input(
                        "name",
                        "Name (optional)",
                        &v.name,
                        "It becomes the guardrail's key, in lower case with hyphens; empty, the server derives one.",
                        false,
                    ));
                }
                out.push_str(&self.radios(
                    "severity",
                    "Severity",
                    &[
                        (
                            "never",
                            "never: the agent must not do this; its rule may block it",
                        ),
                        ("must", "must: the agent has to do this"),
                        ("should", "should: advice; its rule may only ask first"),
                    ],
                    &v.severity,
                ));
                out.push_str(&self.textarea("body", "Text", &v.body, &self.bytes()));
                out.push_str(
                    r#"<fieldset><legend>Rule (optional, <code>oml-gr/1</code>)</legend>"#,
                );
                out.push_str(&self.choice(
                    "tool",
                    "Tool",
                    &[
                        ("", "no rule"),
                        ("shell", "shell"),
                        ("edit", "edit"),
                        ("read", "read"),
                        ("fetch", "fetch"),
                        ("mcp", "mcp"),
                    ],
                    &v.tool,
                ));
                out.push_str(&self.input(
                    "pattern",
                    "Match",
                    &v.pattern,
                    "shell: the command; edit and read: the path; fetch: the host; mcp: the tool's name.",
                    false,
                ));
                out.push_str(&self.choice(
                    "effect",
                    "Effect",
                    &[("deny", "deny"), ("ask", "ask")],
                    &v.effect,
                ));
                out.push_str(r#"<p class="desc">deny: the tool call is blocked; ask: the user is asked first. Deny only with never or must; should may only ask.</p></fieldset>"#);
            }
            Kind::Document => {
                out.push_str(&self.input("title", "Title", &v.title, "One line.", false));
                if self.act == Act::New {
                    out.push_str(&self.input(
                        "claim_key",
                        "Claim key (optional)",
                        &v.claim_key,
                        "<code>doc/</code> and segments of a-z 0-9 . _ -; empty, it is derived from the title. It ends the document's resource address.",
                        false,
                    ));
                }
                out.push_str(&self.choice(
                    "format",
                    "Format",
                    &[("text/markdown", "markdown"), ("text/plain", "plain text")],
                    v.format_or(Kind::Document),
                ));
                out.push_str(&self.textarea("body", "Body", &v.body, &self.bytes()));
            }
        }
        out
    }

    /// What a session will receive, drawn above the form on the page a
    /// preview answered (d5).
    fn preview_html(&self) -> String {
        let Some(p) = self.preview.filter(|_| self.previewed()) else {
            return String::new();
        };
        let v = self.values;
        let mut out = String::from(
            r#"<section class="card oml-preview" aria-labelledby="preview"><h2 id="preview">Preview</h2><p class="desc">Nothing is written until Propose.</p>"#,
        );
        // Characters no reader sees, named before anything else, so an
        // invisible instruction is not proposed unseen (the review of PR
        // #494, 8).
        let hidden = oml_page::web::hidden_chars(&v.body);
        if !hidden.is_empty() {
            let _ = write!(
                out,
                r#"<p class="error" role="alert">The text holds characters no reader sees, kept as typed: {}.</p>"#,
                hidden
                    .iter()
                    .map(|c| format!("<code>U+{:04X}</code>", u32::from(*c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if self.act == Act::Retire {
            let name = self.target.map(name_of).unwrap_or_default();
            let _ = write!(
                out,
                "<p>Once approved, {}. Until then the version in force stays served.</p>",
                self.kind.stops(&name)
            );
        } else {
            match self.kind {
                Kind::Skill => {
                    let name = p.offered_as.as_deref().unwrap_or(&v.name);
                    let _ = write!(
                        out,
                        r#"<p>A session receives the command <code>/mcp__&lt;server&gt;__{}</code>.</p><p class="label">Its index line</p><pre>- {}: {}</pre><p class="label">Its <code>prompts/list</code> entry</p><pre>{}</pre>"#,
                        text(name),
                        text(name),
                        oml_page::web::text_shown(&v.description),
                        text(
                            &serde_json::to_string(
                                &json!({"name": name, "description": v.description})
                            )
                            .unwrap_or_default()
                        ),
                    );
                    for shadow in &p.shadows {
                        let reference =
                            picker::reference(&shadow.workspace, |l| l == self.place.user);
                        let line = if shadow.relation == "shadowed_by" {
                            format!(
                                "A skill of the same name in <code>{}</code> shadows it there.",
                                text(&reference)
                            )
                        } else {
                            format!(
                                "It shadows the skill of the same name in <code>{}</code> here and below.",
                                text(&reference)
                            )
                        };
                        let _ = write!(out, "<p>{line}</p>");
                    }
                }
                Kind::Guardrail => {
                    let _ = write!(
                        out,
                        r#"<p class="label">Its session-start line</p><pre>- [{}] {}</pre>"#,
                        text(&v.severity),
                        oml_page::web::text_shown(&v.body)
                    );
                    let checks = match &p.rule {
                        // The tool is named once (the review of PR #494, 16).
                        Some(rule) => {
                            let (member, pattern) =
                                matcher(rule).unwrap_or(("match", "?".to_owned()));
                            format!(
                                "Its rule checks each {} call: {member} <code>{}</code> · {}.",
                                text(rule["tool"].as_str().unwrap_or("?")),
                                text(&pattern),
                                text(rule["effect"].as_str().unwrap_or("?"))
                            )
                        }
                        None => "No rule: the line is read at session start and nothing is checked at the tool call.".to_owned(),
                    };
                    let _ = write!(
                        out,
                        "<p>{checks}</p><p>Every key that reads this workspace loads it, here and below it unless its guardrail scope is <code>workspace</code>.</p>"
                    );
                    if let Some(after) = p.set_after {
                        let _ = write!(
                            out,
                            "<p>{} of at most {} guardrails apply here once it is approved.</p>",
                            after.size, after.cap
                        );
                    }
                }
                Kind::Document => {
                    let key = match self.target {
                        Some(t) => Some(t.claim_key.clone()),
                        None => Some(v.claim_key.clone()).filter(|k| !k.is_empty()),
                    };
                    match key.as_deref().and_then(|k| k.strip_prefix("doc/")) {
                        Some(path) => {
                            let _ = write!(
                                out,
                                "<p>Its resource address is <code>oml://workspace/{}/doc/{}</code>.</p>",
                                text(&self.place.view.name),
                                text(path)
                            );
                        }
                        None => out.push_str("<p>Its resource address ends in the claim key derived from its title.</p>"),
                    }
                    if !p.passages.is_empty() {
                        let _ = write!(
                            out,
                            r#"<p class="label">Its passages · {}</p><ul>"#,
                            p.passages.len()
                        );
                        for passage in &p.passages {
                            let _ = write!(
                                out,
                                "<li><code>#{}</code> lines {}–{}{}</li>",
                                text(&passage.fragment),
                                passage.start_line,
                                passage.end_line,
                                if passage.heading_path.is_empty() {
                                    String::new()
                                } else {
                                    format!(" · {}", text(&passage.heading_path.join(" › ")))
                                }
                            );
                        }
                        out.push_str("</ul>");
                    }
                }
            }
        }
        if p.findings.is_empty() {
            out.push_str("<p>The scan found nothing.</p>");
        } else {
            let _ = write!(out, "<p>The scan found {}:</p><ul>", p.findings.len());
            for f in &p.findings {
                let _ = write!(
                    out,
                    "<li>{} · {} · {}</li>",
                    text(&f.class),
                    text(&f.rule),
                    text(&f.verdict)
                );
            }
            out.push_str("</ul>");
        }
        if p.needs_person {
            out.push_str("<p>A person's verdict is needed to approve it.</p>");
        }
        out.push_str("</section>");
        out
    }

    /// The change against the served tip, drawn after the preview on an
    /// edit (d6).
    fn diff(&self) -> String {
        let (Some(target), true, Act::Edit) = (self.target, self.previewed(), self.act) else {
            return String::new();
        };
        let was = Values::of_memory(target);
        let skill = self.kind == Kind::Skill;
        labelled(
            target.version,
            Parts {
                name: skill.then_some(was.name.as_str()),
                description: skill.then_some(was.description.as_str()),
                body: &was.body,
            },
            Parts {
                name: skill.then_some(self.values.name.as_str()),
                description: skill.then_some(self.values.description.as_str()),
                body: &self.values.body,
            },
        )
    }

    fn html(&self, session: &Session) -> String {
        let place = self.place;
        let title = self.title();
        let mut out = crumbs(place, self.kind, Some(&title), false);
        let _ = write!(out, "<h1>{}</h1>", text(&title));
        let _ = write!(
            out,
            r#"<p class="desc">Proposed under <code>{}</code>. {} {}</p>"#,
            text(&place.key_name()),
            if self.act == Act::Retire {
                "The version in force stays served until the retirement is approved."
            } else {
                place.held_line()
            },
            place.approves(self.kind)
        );
        if self.act == Act::Retire
            && let Some(target) = self.target
        {
            let _ = write!(
                out,
                r#"<div class="card"><p>{} {} <code>{}</code> v{}{}</p><p>{}</p><p>Once approved, {}.</p></div>"#,
                state_tag(&target.state),
                self.kind.as_str(),
                text(&target.claim_key),
                target.version,
                target
                    .severity
                    .as_deref()
                    .map_or(String::new(), |s| format!(" · {}", text(s))),
                text(target.text.as_deref().unwrap_or_default()),
                self.kind.stops(&name_of(target))
            );
        }
        if let Some(r) = self.refusal.filter(|r| r.field == "form") {
            out.push_str(&r.html());
        }
        out.push_str(&self.preview_html());
        out.push_str(&self.diff());
        let target = self
            .target
            .map(|t| t.artifact_id.as_str())
            .unwrap_or_default();
        let _ = write!(
            out,
            r#"<form class="oml-form" method="post" action="{}">{}{}{}{}"#,
            self.act.path(),
            hidden("action", self.act.action()),
            hidden("csrf_token", &session.csrf()),
            hidden("nonce", self.nonce),
            hidden("kind", self.kind.as_str()),
        );
        let (workspace, expected) = match self.target {
            Some(t) => {
                out.push_str(&hidden("memory", &t.artifact_id));
                out.push_str(&hidden("expected", &t.version.to_string()));
                (String::new(), t.version.to_string())
            }
            None => {
                out.push_str(&hidden("workspace", &place.reference));
                (place.reference.clone(), String::new())
            }
        };
        if !self.values.eol.is_empty() {
            out.push_str(&hidden("eol", &self.values.eol));
        }
        let previewed = self.previewed();
        if previewed {
            out.push_str(&hidden(
                "previewed",
                &self.values.digest(
                    self.kind,
                    self.act,
                    target,
                    &workspace,
                    &expected,
                    &session.preview_key(self.nonce),
                ),
            ));
        }
        out.push_str(&self.fields());
        // Preview first in the DOM and the focus order, so Enter previews;
        // Propose only on a page a preview answered (d5).
        let _ = write!(
            out,
            r#"<div class="acts"><button type="submit" name="step" value="preview"{}>Preview</button>"#,
            if previewed { "" } else { r#" class="primary""# }
        );
        if previewed {
            let _ = write!(
                out,
                r#"<button type="submit" name="step" value="propose" class="primary">{}</button>"#,
                text(&self.propose())
            );
        }
        let _ = write!(
            out,
            r#"</div></form><p><a href="{}">Back to {}</a></p>"#,
            place.page_link(self.kind),
            text(&place.reference)
        );
        out
    }
}

/// The tip-conflict frame: nothing was proposed, the typed text is kept,
/// and the one primary re-reads the tip and redraws the edit against it
/// (`step=rebase`, d5).
fn rebase_frame(form: &Form<'_>, session: &Session, tip_version: u64) -> String {
    let place = form.place;
    let Some(target) = form.target else {
        return String::new();
    };
    let title = form.title();
    let mut out = crumbs(place, form.kind, Some(&title), false);
    let _ = write!(out, "<h1>{}</h1>", text(&title));
    let _ = write!(
        out,
        r#"<p class="error" role="alert">v{tip_version} is now the latest version; this {} was made against v{}. Nothing was proposed; the text typed here is kept. <code>{}</code></p>"#,
        if form.act == Act::Retire {
            "retirement"
        } else {
            "edit"
        },
        target.version,
        text(
            form.refusal
                .map_or("submission.tip_conflict", |r| r.code.as_str())
        )
    );
    let v = form.values;
    let _ = write!(
        out,
        r#"<form class="oml-form" method="post" action="{}">{}{}{}{}{}{}"#,
        form.act.path(),
        hidden("action", form.act.action()),
        hidden("csrf_token", &session.csrf()),
        hidden("nonce", form.nonce),
        hidden("kind", form.kind.as_str()),
        hidden("memory", &target.artifact_id),
        hidden("expected", &tip_version.to_string()),
    );
    for (name, value) in [
        ("name", &v.name),
        ("description", &v.description),
        ("body", &v.body),
        ("severity", &v.severity),
        ("tool", &v.tool),
        ("pattern", &v.pattern),
        ("effect", &v.effect),
        ("title", &v.title),
        ("format", &v.format),
        ("reason", &v.reason),
    ] {
        if !value.is_empty() {
            out.push_str(&hidden(name, value));
        }
    }
    let _ = write!(
        out,
        r#"<div class="acts"><button type="submit" name="step" value="rebase" class="primary">{} from v{tip_version} with this text</button></div></form><p><a href="{}">Back to {}</a></p>"#,
        if form.act == Act::Retire {
            "Retire"
        } else {
            "Edit"
        },
        place.page_link(form.kind),
        text(&place.reference)
    );
    out
}

/// The draft a form's values make: the kind's fields, re-sent on a
/// supersession, and for a retirement its reason alone (d3, d4).
fn draft<'a>(
    act: Act,
    kind: Kind,
    values: &'a Values,
    workspace: &'a str,
    target: Option<&'a Memory>,
    rule: Option<&'a Value>,
) -> Draft<'a> {
    let mut draft = Draft {
        kind: kind.as_str(),
        body: &values.body,
        ..Draft::default()
    };
    match target {
        Some(t) => draft.replaces = Some((&t.artifact_id, t.version)),
        None => draft.workspace = Some(workspace),
    }
    if act == Act::Retire {
        draft.body = &values.reason;
        draft.retirement = true;
        return draft;
    }
    match kind {
        Kind::Skill => {
            draft.format = Some("text/markdown");
            draft.skill = Some((values.name.as_str(), values.description.as_str()));
        }
        Kind::Guardrail => {
            draft.severity = Some(&values.severity)
                .filter(|s| !s.is_empty())
                .map(String::as_str);
            draft.rule = rule;
            if act == Act::New {
                draft.claim_key = Some(values.guardrail_key.as_str()).filter(|k| !k.is_empty());
            }
        }
        Kind::Document => {
            draft.format = Some(values.format_or(kind));
            draft.document_title = Some(values.title.as_str()).filter(|t| !t.trim().is_empty());
            if act == Act::New {
                draft.claim_key = Some(values.claim_key.as_str()).filter(|k| !k.is_empty());
            }
        }
    }
    draft
}

/// The version an edit or a retirement starts from, read as the acting
/// key, and its workspace: an approved tip of a builder kind, with no held
/// successor of the person's own (d2), else the refusal page.
async fn target(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    id: Option<&str>,
    next: &str,
) -> Result<(Memory, Place, Kind), acts::Refusal> {
    let Some(id) = id.filter(|id| is_id(id)) else {
        return Err(Box::new(acts::invalid("memory is a memory's id", calls)));
    };
    let personal = session.credential(Capacity::Web);
    let ids = calls.next_ids();
    let first = web.api.get_memory(&call(&personal, &ids), id).await;
    let m = match first {
        Ok(m) => m,
        Err(e) if picker::is_not_found(&e) => {
            let review = session.credential(Capacity::Review);
            let ids = calls.next_ids();
            web.api
                .get_memory(&call(&review, &ids), id)
                .await
                .map_err(|e| Box::new(outcome::page(&e, calls, next)))?
        }
        Err(e) => return Err(Box::new(outcome::page(&e, calls, next))),
    };
    let Some(kind) = kind_of(&m) else {
        return Err(Box::new(refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "the builder edits a skill, a guardrail or a document",
            calls,
        )));
    };
    let workspace = m.workspace_id.clone().unwrap_or_default();
    let place = place(web, session, calls, Some(&workspace), next).await?;
    Ok((m, place, kind))
}

/// The person's own held version after `m` in its claim, which an edit
/// waits for (d2): one more read, the person's own rows of the kind there.
async fn pending(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    m: &Memory,
    kind: Kind,
) -> Option<Memory> {
    let personal = session.credential(Capacity::Web);
    let ids = calls.next_ids();
    let workspace = m.workspace_id.clone().unwrap_or_default();
    let page = web
        .api
        .list_memories(
            &call(&personal, &ids),
            &MemoryQuery {
                mine: true,
                workspace: Some(&workspace),
                kinds: Some(kind.as_str()),
                state: Some("held"),
                limit: LIST_LIMIT,
                ..MemoryQuery::default()
            },
        )
        .await
        .ok()?;
    page.items
        .into_iter()
        .find(|h| h.claim_key == m.claim_key && h.version > m.version && h.state == "held")
}

/// The page an edit or a retirement of `t` draws while the person's own
/// `p` waits for review: it links `p` and offers no second proposal
/// (ADR-0082 d2). A post answers it as `409`, never as a rebase, since a
/// held version is no tip to rebase onto (the review of PR #494, C-B4).
fn waiting(
    status: StatusCode,
    place: &Place,
    session: &Session,
    kind: Kind,
    t: &Memory,
    p: &Memory,
) -> Response {
    let body = format!(
        r#"{}<h1>{}</h1><p>In force · <a href="{USER_MEMORY_PATH}?id={}">{} v{} waits for review</a>: a second proposal would race it, so none is offered until it is judged or withdrawn.</p><p><a href="{}">Back to {}</a></p>"#,
        crumbs(place, kind, Some(&name_of(t)), false),
        text(&name_of(t)),
        attr(&encode(&p.artifact_id)),
        if p.retirement { "retirement" } else { "edit" },
        p.version,
        place.page_link(kind),
        text(&place.reference)
    );
    outcome::html(
        status,
        shell(
            &place.reference,
            &session.personal(),
            USER_WORKSPACES_PATH,
            None,
            &body,
        ),
    )
}

/// A form's first frame: the new item's empty form, or an edit prefilled
/// from the served tip, or a retirement's confirmation.
async fn form_page(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    uri: Uri,
    act: Act,
) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let next = uri.path_and_query().map_or(act.path(), |pq| pq.as_str());
    let (target, place, kind) = match act {
        Act::New => {
            let kind = match kind_field(&fields, &calls) {
                Ok(kind) => kind,
                Err(refusal) => return *refusal,
            };
            match place(&web, &session, &calls, fields.get("workspace"), next).await {
                Ok(place) => (None, place, kind),
                Err(refusal) => return *refusal,
            }
        }
        Act::Edit | Act::Retire => {
            match target(&web, &session, &calls, fields.get("memory"), next).await {
                Ok((m, place, kind)) => (Some(m), place, kind),
                Err(refusal) => return *refusal,
            }
        }
    };
    if !place.writes() {
        return refused(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "no key of this account proposes here",
            &calls,
        );
    }
    if let Some(t) = &target {
        if t.state != "promoted" || !t.tip || t.retirement {
            return refused(
                StatusCode::CONFLICT,
                "submission.tip_conflict",
                "only the approved tip is edited or retired",
                &calls,
            );
        }
        if let Some(p) = pending(&web, &session, &calls, t, kind).await {
            return waiting(StatusCode::OK, &place, &session, kind, t, &p);
        }
    }
    let values = target.as_ref().map(Values::of_memory).unwrap_or_default();
    let nonce = crate::workspaces::nonce();
    let form = Form {
        act,
        kind,
        place: &place,
        target: target.as_ref(),
        values: &values,
        nonce: &nonce,
        preview: None,
        refusal: None,
    };
    let body = form.html(&session);
    outcome::html(
        StatusCode::OK,
        shell(
            &place.reference,
            &session.personal(),
            USER_WORKSPACES_PATH,
            None,
            &body,
        ),
    )
}

/// `GET /web/workspace/new`.
pub async fn new_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    form_page(web, session, calls, uri, Act::New).await
}

/// `GET /web/workspace/edit`.
pub async fn edit_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    form_page(web, session, calls, uri, Act::Edit).await
}

/// `GET /web/workspace/retire`.
pub async fn retire_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    form_page(web, session, calls, uri, Act::Retire).await
}

/// A form's post: Preview answers the page with the preview above the
/// form; Propose writes only the values a preview answered, else redraws
/// the preview; Rebase re-reads the tip and previews the typed text
/// against it (d5). A write answers `303` to the workspace page.
async fn form_act(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    form: Result<axum::extract::Form<Vec<(String, String)>>, FormRejection>,
    act: Act,
) -> Response {
    let (fields, _) = match acts::read(
        form,
        &[act.action()],
        |_, name| FORM_FIELDS.contains(&name),
        &calls,
    ) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let step = fields.get("step").unwrap_or("preview");
    if !matches!(step, "preview" | "propose" | "rebase") {
        return acts::invalid("step is preview, propose or rebase", &calls);
    }
    // The form's nonce, kept through every redraw, is the write's
    // Idempotency-Key, so a double submit captures one version (d2).
    let nonce = match fields.get("nonce") {
        Some(n) if crate::workspaces::is_nonce(n) => n.to_owned(),
        Some(_) => {
            return acts::invalid(
                "nonce is not one this page draws: 32 lower-case hexadecimal characters",
                &calls,
            );
        }
        None => crate::workspaces::nonce(),
    };
    let next = act.path();
    let (mut target, place, kind) = match act {
        Act::New => {
            let kind = match kind_field(&fields, &calls) {
                Ok(kind) => kind,
                Err(refusal) => return *refusal,
            };
            match place(&web, &session, &calls, fields.get("workspace"), next).await {
                Ok(place) => (None, place, kind),
                Err(refusal) => return *refusal,
            }
        }
        Act::Edit | Act::Retire => {
            match target(&web, &session, &calls, fields.get("memory"), next).await {
                Ok((m, place, kind)) => (Some(m), place, kind),
                Err(refusal) => return *refusal,
            }
        }
    };
    if !place.writes() {
        return refused(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "no key of this account proposes here",
            &calls,
        );
    }
    // The person's own held version of the claim stops the edit before any
    // preview: its version conflict is no tip moved (C-B4).
    if let Some(t) = &target
        && let Some(p) = pending(&web, &session, &calls, t, kind).await
    {
        return waiting(StatusCode::CONFLICT, &place, &session, kind, t, &p);
    }
    // The version the form rendered: the write's expected_version
    // (ADR-0057 d19). A rebase moves it to the tip the conflict named.
    if fields
        .get("expected")
        .is_some_and(|v| v.parse::<u32>().is_err())
    {
        return acts::invalid("expected is a version number", &calls);
    }
    let values = Values::of(&fields);
    let credential = session.credential(place.capacity);
    let draw =
        |preview: Option<&MemoryPreview>, refusal: Option<&Refusal>, target: Option<&Memory>| {
            let form = Form {
                act,
                kind,
                place: &place,
                target,
                values: &values,
                nonce: &nonce,
                preview,
                refusal,
            };
            if let Some(r) = refusal
                && let Some(version) = r.moved_to()
                && act != Act::New
            {
                return outcome::html(
                    StatusCode::CONFLICT,
                    shell(
                        &place.reference,
                        &session.personal(),
                        USER_WORKSPACES_PATH,
                        None,
                        &rebase_frame(&form, &session, version),
                    ),
                );
            }
            let status = if refusal.is_some() {
                StatusCode::UNPROCESSABLE_ENTITY
            } else {
                StatusCode::OK
            };
            outcome::html(
                status,
                shell(
                    &place.reference,
                    &session.personal(),
                    USER_WORKSPACES_PATH,
                    None,
                    &form.html(&session),
                ),
            )
        };
    // A rebase re-reads the tip the conflict named and previews the typed
    // text against it, so its page carries the change and Propose.
    if step == "rebase" {
        let Some(t) = target.as_ref() else {
            return acts::invalid("rebase is an edit's step", &calls);
        };
        // The version the conflict named, sent as expected, so the probe
        // meets the tip check, whose refusal names the tip.
        let expected = fields
            .get("expected")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(t.version);
        // A retirement's text is its reason (the review of PR #494, 15).
        let probe = Draft {
            kind: kind.as_str(),
            body: if act == Act::Retire {
                &values.reason
            } else {
                &values.body
            },
            replaces: Some((&t.artifact_id, expected)),
            retirement: act == Act::Retire,
            ..Draft::default()
        };
        let ids = calls.next_ids();
        let tip = match web.api.preview(&call(&credential, &ids), &probe).await {
            Ok(p) => p.refusal.and_then(|r| {
                Refusal::of(&r.code, &r.message, r.details.as_ref(), kind).current_tip()
            }),
            Err(e) => return outcome::page(&e, &calls, next),
        };
        if let Some((id, _)) = tip {
            let ids = calls.next_ids();
            match web.api.get_memory(&call(&credential, &ids), &id).await {
                Ok(m) => target = Some(m),
                Err(e) => return outcome::page(&e, &calls, next),
            }
        }
    }
    if act == Act::Retire && values.reason.trim().is_empty() {
        let refusal = Refusal::local(
            "reason",
            "A reason is required: it is the retirement version's text.",
        );
        return draw(None, Some(&refusal), target.as_ref());
    }
    if values.eol == "mixed" && act == Act::Edit {
        let refusal = Refusal::local(
            "body",
            "The stored text breaks its lines in more than one way, which a form cannot keep; edit it from the command line.",
        );
        return draw(None, Some(&refusal), target.as_ref());
    }
    let rule = match (kind, act) {
        (Kind::Guardrail, Act::New | Act::Edit) => match values.rule() {
            Ok(rule) => rule,
            Err(message) => {
                let refusal = Refusal::local("pattern", &message);
                return draw(None, Some(&refusal), target.as_ref());
            }
        },
        _ => None,
    };
    let draft = draft(
        act,
        kind,
        &values,
        &place.view.id,
        target.as_ref(),
        rule.as_ref(),
    );
    let target_id = target
        .as_ref()
        .map(|t| t.artifact_id.clone())
        .unwrap_or_default();
    let unchanged = step == "propose"
        && fields.get("previewed")
            == Some(
                values
                    .digest(
                        kind,
                        act,
                        &target_id,
                        fields.get("workspace").unwrap_or_default(),
                        fields.get("expected").unwrap_or_default(),
                        &session.preview_key(&nonce),
                    )
                    .as_str(),
            );
    if !unchanged {
        let ids = calls.next_ids();
        return match web.api.preview(&call(&credential, &ids), &draft).await {
            Ok(p) => {
                let refusal = p
                    .refusal
                    .as_ref()
                    .map(|r| Refusal::of(&r.code, &r.message, r.details.as_ref(), kind));
                draw(Some(&p), refusal.as_ref(), target.as_ref())
            }
            Err(e) => outcome::page(&e, &calls, next),
        };
    }
    let key = session.act_key(&nonce);
    let _in_flight = acts::hold(&key).await;
    let ids = calls.next_ids();
    let written = match act {
        Act::New => web.api.submit(&call(&credential, &ids), &key, &draft).await,
        Act::Edit | Act::Retire => {
            web.api
                .supersede_draft(&call(&credential, &ids), &key, &draft)
                .await
        }
    };
    match written {
        Ok(accepted) => acts::see_other(
            &format!(
                "{WORKSPACE_PATH}?workspace={}&kind={}&done={}&held={}",
                encode(&place.reference),
                kind.as_str(),
                if act == Act::Retire {
                    "retirement"
                } else {
                    "proposed"
                },
                encode(&accepted.artifact_id)
            ),
            &[],
        ),
        Err(oml_sdk::Error::Api(e))
            if !e.code.starts_with("auth.")
                && !e.code.starts_with("access.")
                && e.code != "limit.rate" =>
        {
            let refusal = Refusal::of(&e.code, &e.message, Some(&e.details), kind);
            draw(None, Some(&refusal), target.as_ref())
        }
        Err(e) => outcome::page(&e, &calls, next),
    }
}

/// `POST /web/workspace/new`.
pub async fn new_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<axum::extract::Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    form_act(web, session, calls, form, Act::New).await
}

/// `POST /web/workspace/edit`.
pub async fn edit_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<axum::extract::Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    form_act(web, session, calls, form, Act::Edit).await
}

/// `POST /web/workspace/retire`.
pub async fn retire_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<axum::extract::Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    form_act(web, session, calls, form, Act::Retire).await
}

/// `GET /web/guardrails`: `308` to the workspace page's Guardrails kind,
/// keeping the workspace; with none, the page opens on the person's root
/// (ADR-0083 d12).
pub async fn moved_guardrails(uri: Uri) -> Response {
    let mut kept = crate::kept_query(&uri, &["workspace"]);
    kept.push("kind=guardrail".to_owned());
    crate::permanent(&format!("{WORKSPACE_PATH}?{}", kept.join("&")))
}
