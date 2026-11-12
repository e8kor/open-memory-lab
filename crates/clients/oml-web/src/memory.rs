//! One memory opened by its id, on the user page and on the admin page, and
//! the admin page's Curate view (ADR-0045 d10, d12, d21; ADR-0056 d9), moved
//! from the lead's `?view=memories&memory=<id>` and `?view=curate` to paths
//! of their own (ADR-0070 d7 (b)):
//!
//! - `GET /web/memory?id=` reads the memory as the person's web key
//!   (`getMemory`): their own rows in any state and what their grants read;
//!   anything else is the API's `access.not_found`;
//! - `GET /web/admin/memory?id=` reads it with its attribution and verdicts
//!   as the admin key (`adminReadMemory`, journalled by the API as an admin
//!   read), with the form that proposes its next version or its retirement;
//!   `POST` runs it (`supersedeMemory`) as the person's curator key for the
//!   curator's own claims and as their reviewer key for any other, never as
//!   the admin key, and never on a guardrail (ADR-0037 d17);
//! - `GET /web/admin/curate` and its `POST` write a reworded copy of an
//!   approved memory into the shared area as the curator key (`submitMemory`,
//!   `learn`), held until a key outside its lineage approves it.
//!
//! A Mermaid claim is drawn beside its source in the frame of
//! [`crate::render`], on every page that shows it.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{
    ADMIN_CURATE_PATH, ADMIN_MEMORIES_PATH, ADMIN_MEMORY_PATH, ADMIN_TABS, USER_MEMORIES_PATH,
    USER_MEMORY_PATH, form, hidden, input, select, shell, short, state_tag, text, textarea,
    user_tabs,
};
use oml_sdk::{AdminRead, Error};

use crate::acts::Refusal;
use crate::card::{anchor_line, card};
use crate::pages::{Fields, refused};
use crate::review::is_id;
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, acts, outcome, render};

/// The admin memory page's one act.
pub const MEMORY_ACTIONS: &[&str] = &["supersede"];

/// The Curate view's one act.
pub const CURATE_ACTIONS: &[&str] = &["curate"];

/// The workarea a curated copy is written in: the shared area.
const UNIVERSE: &str = "universe";

/// The notice a view's own act names, from a closed set of words only, so a
/// crafted link cannot put text on the page.
fn notice(fields: &Fields) -> Option<&'static str> {
    match fields.get("done")? {
        "proposed" => Some("Proposed: held until reviewed."),
        "curated" => Some("Curated: held until someone outside its lineage approves it."),
        _ => None,
    }
}

/// The id a view names in `field`, or the refusal naming it.
fn id_in<'a>(fields: &'a Fields, field: &str, calls: &PageCalls) -> Result<&'a str, Refusal> {
    fields
        .given(field)
        .filter(|v| is_id(v))
        .ok_or_else(|| Box::new(acts::invalid(&format!("{field} is a memory's id"), calls)))
}

/// A page's answer, with its frame source allowed where it holds a drawing:
/// decided by whether one was drawn, never by the page's text.
fn page(web: &Web, html: String, framed: bool) -> Response {
    let mut response = outcome::html(StatusCode::OK, html);
    if framed {
        render::allow_frames(&mut response, web.layers.public_origin.as_deref());
    }
    response
}

/// Any session but an admin entry's answers `access.not_found` on the admin
/// page (ADR-0045 d10).
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

/// `GET /web/memory?id=`: one memory as the person's web key reads it.
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
    let id = match id_in(&fields, "id", &calls) {
        Ok(id) => id,
        Err(refusal) => return *refusal,
    };
    let credential = session.credential(Capacity::Web);
    let ids = calls.next_ids();
    let memory = match web.api.get_memory(&call(&credential, &ids), id).await {
        Ok(memory) => memory,
        Err(e) => return outcome::page(&e, calls.page_id(), USER_MEMORY_PATH),
    };
    let drawing = render::drawing(
        web.layers.public_origin.as_deref(),
        memory.format.as_deref(),
        &session.csrf(),
        &memory.artifact_id,
        memory.text.as_deref().unwrap_or_default(),
    );
    let body = format!(
        r#"<p class="label">Memories</p><h1>One memory</h1><p><a href="{USER_MEMORIES_PATH}">Back to the list</a></p>{}"#,
        card(&memory, drawing.as_deref().unwrap_or_default())
    );
    page(
        &web,
        shell(
            "Your page",
            &session.who(),
            &user_tabs(session.admin),
            USER_MEMORIES_PATH,
            None,
            &body,
        ),
        drawing.is_some(),
    )
}

/// A link to one memory on the admin page.
fn admin_link(id: &str) -> String {
    format!(
        r#"<a href="{ADMIN_MEMORY_PATH}?id={}">{}</a>"#,
        text(id),
        text(id)
    )
}

fn is_prose(format: Option<&str>) -> bool {
    matches!(
        format.unwrap_or("text/plain"),
        "text/plain" | "text/markdown"
    )
}

/// The admin read's card and its attribution and verdict tables, as the
/// lead drew them.
fn attribution(m: &AdminRead, drawing: &str) -> String {
    let title = m.title.as_deref().map_or_else(String::new, |t| {
        format!(r#"<p class="label">title</p><p>{}</p>"#, text(t))
    });
    let format_line = if is_prose(m.format.as_deref()) {
        String::new()
    } else {
        format!(
            r#"<p class="label">format</p><p class="mono">{}</p>"#,
            text(m.format.as_deref().unwrap_or_default())
        )
    };
    let mut out = format!(
        r#"<div class="card"><p class="label">{} · {} · version {}</p>{title}<p class="text{}">{}</p>{drawing}{format_line}</div><h2>Attribution</h2><table><tr><th>Field</th><th>Value</th></tr>"#,
        state_tag(&m.state),
        short(&m.workarea_path),
        m.version,
        if is_prose(m.format.as_deref()) {
            ""
        } else {
            " mono"
        },
        text(&m.text),
    );
    let ids = |v: &[String]| {
        if v.is_empty() {
            "—".to_owned()
        } else {
            v.iter()
                .map(|id| admin_link(id))
                .collect::<Vec<_>>()
                .join("<br>")
        }
    };
    // A key with no name is named by its id (#253).
    let key = m.key.as_deref().or(m.key_id.as_deref()).unwrap_or_default();
    let author = match &m.user {
        Some(user) => format!("{} ({})", short(key), short(user)),
        None => short(key),
    };
    let valid = if m.valid_from.is_none() && m.valid_until.is_none() {
        "none declared".to_owned()
    } else {
        text(&format!(
            "from {}, {}",
            m.valid_from.as_deref().unwrap_or("any time"),
            m.valid_until
                .as_deref()
                .map_or_else(|| "open".to_owned(), |u| format!("until {u}"))
        ))
    };
    for (field, value) in [
        ("artifact id", text(&m.artifact_id)),
        ("claim key", short(&m.claim_key)),
        ("written by key", author),
        (
            "principal",
            short(m.principal.as_deref().unwrap_or_default()),
        ),
        ("written at", short(&m.created_at)),
        ("derives from", ids(&m.derives_from)),
        (
            "replaces",
            m.replaces
                .as_ref()
                .map_or("—".to_owned(), |r| ids(std::slice::from_ref(r))),
        ),
        (
            "text digest",
            short(m.plaintext_digest.as_deref().unwrap_or_default()),
        ),
        ("source", anchor_line(m.anchor.as_ref())),
        ("valid", valid),
    ] {
        let _ = write!(
            out,
            r#"<tr><td class="label">{field}</td><td class="mono">{value}</td></tr>"#
        );
    }
    out.push_str("</table><h2>Verdicts</h2><table><tr><th>Verdict</th><th>Key</th><th>Principal</th><th>Reason</th><th>At</th></tr>");
    if m.verdicts.is_empty() {
        out.push_str(r#"<tr><td colspan="5" class="dim">No verdict yet.</td></tr>"#);
    }
    for v in &m.verdicts {
        let _ = write!(
            out,
            r#"<tr><td>{}</td><td class="mono">{}</td><td class="mono">{}</td><td>{}</td><td class="mono">{}</td></tr>"#,
            state_tag(&v.verdict),
            short(v.key.as_deref().unwrap_or_default()),
            short(v.principal.as_deref().unwrap_or_default()),
            short(v.reason.as_deref().unwrap_or_default()),
            short(&v.at)
        );
    }
    out.push_str("</table>");
    out
}

/// The admin read of `id` as the admin key.
async fn admin_read(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    id: &str,
) -> Result<AdminRead, Error> {
    let credential = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    web.api.admin_read(&call(&credential, &ids), id).await
}

/// `GET /web/admin/memory?id=`: one memory with its attribution, and the
/// form that proposes its next version.
pub async fn admin_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let id = match id_in(&fields, "id", &calls) {
        Ok(id) => id,
        Err(refusal) => return *refusal,
    };
    let read = match admin_read(&web, &session, &calls, id).await {
        Ok(read) => read,
        Err(e) => return outcome::page(&e, calls.page_id(), ADMIN_MEMORY_PATH),
    };
    let csrf = session.csrf();
    let drawing = render::drawing(
        web.layers.public_origin.as_deref(),
        read.format.as_deref(),
        &csrf,
        &read.artifact_id,
        &read.text,
    );
    let mut body = format!(
        r#"<p class="label">Memories</p><h1>One memory</h1><p><a href="{ADMIN_MEMORIES_PATH}">Back to the list</a></p>{}<h2>Replace it</h2>"#,
        attribution(&read, drawing.as_deref().unwrap_or_default())
    );
    body.push_str(&form(
        ADMIN_MEMORY_PATH,
        "supersede",
        &csrf,
        &format!(
            "{}{}{}{}{}",
            hidden("memory", &read.artifact_id),
            // The version this page rendered: the edit is refused if the
            // claim moved on since (ADR-0057 d19).
            hidden("expected_version", &read.version.to_string()),
            textarea("text", "new text", &read.text),
            select(
                "retirement",
                "retire instead",
                &[("no", "no"), ("yes", "yes")]
            ),
            acts::nonce_field()
        ),
        "Propose the new version",
        false,
    ));
    let _ = write!(
        body,
        r#"<p><a href="{ADMIN_CURATE_PATH}?memory={}">Curate</a></p>"#,
        text(&read.artifact_id)
    );
    page(
        &web,
        shell(
            "Administration",
            &session.who(),
            &ADMIN_TABS,
            ADMIN_MEMORIES_PATH,
            notice(&fields),
            &body,
        ),
        drawing.is_some(),
    )
}

/// Whether the page refuses to supersede a version of `kind`: a guardrail,
/// and a version whose kind the admin read does not name (an API before
/// v0.7.0), so the refusal fails closed (ADR-0037 d17).
fn refuses_supersession(kind: Option<&str>) -> bool {
    !matches!(kind, Some(k) if k != "guardrail")
}

/// The page for a call made as the curator key that has no usable answer:
/// a credential the API does not resolve is a person with no curator key on
/// this session (a designation takes effect at the next sign-in, ADR-0070
/// d2 as built), never a reason to sign in again.
fn curator_refused(e: &Error, calls: &PageCalls, next: &str) -> Response {
    match e {
        Error::Api(api) if api.code == "auth.invalid" => refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "you hold no curate key on this session; a curator designation takes effect at your next sign-in",
            calls,
        ),
        _ => outcome::page(e, calls.page_id(), next),
    }
}

/// `POST /web/admin/memory`: the supersession, as the person's curator key
/// for the curator's own claim and as their reviewer key for any other.
pub async fn admin_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let (fields, _) = match acts::read(
        form,
        MEMORY_ACTIONS,
        |_, n| matches!(n, "memory" | "expected_version" | "text" | "retirement"),
        &calls,
    ) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let target = match id_in(&fields, "memory", &calls) {
        Ok(id) => id.to_owned(),
        Err(refusal) => return *refusal,
    };
    let Some(expected_version) = fields
        .given("expected_version")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v >= 1)
    else {
        return acts::invalid("expected_version is the version this page rendered", &calls);
    };
    let Some(body) = fields.given("text") else {
        return acts::invalid("text is required", &calls);
    };
    let retirement = matches!(fields.given("retirement"), Some("yes" | "true" | "on"));
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let read = match admin_read(&web, &session, &calls, &target).await {
        Ok(read) => read,
        Err(e) => return outcome::page(&e, calls.page_id(), ADMIN_MEMORY_PATH),
    };
    // No page supersedes or retires a guardrail: it is written with its
    // severity and rule on the API or the command line (ADR-0037 d17).
    if refuses_supersession(read.kind.as_deref()) {
        return acts::invalid(
            "a guardrail, or a version whose kind the server does not name, is superseded and retired on the API or the command line, never on this page",
            &calls,
        );
    }
    // The curator's own universe claim is the curator key's to supersede;
    // any other is the reviewer key's (ADR-0045 d12).
    let curate = session.credential(Capacity::Curate);
    let ids = calls.next_ids();
    // Only a credential the API does not resolve means no curator key on
    // this session; any other failure fails the act, so the acting key
    // never changes because a call failed.
    let own = match web.api.whoami(&call(&curate, &ids)).await {
        Ok(curator) => curator.name.is_some() && curator.name == read.key,
        Err(Error::Api(e)) if e.code == "auth.invalid" => false,
        Err(e) => return outcome::page(&e, calls.page_id(), ADMIN_MEMORY_PATH),
    };
    let credential = if own {
        curate
    } else {
        session.credential(Capacity::Review)
    };
    let ids = calls.next_ids();
    match web
        .api
        .supersede(
            &call(&credential, &ids),
            &key,
            &target,
            body,
            expected_version,
            retirement,
        )
        .await
    {
        Ok(_) => acts::see_other(
            &format!("{ADMIN_MEMORY_PATH}?id={target}&done=proposed"),
            &[],
        ),
        Err(e) => outcome::page(&e, calls.page_id(), ADMIN_MEMORY_PATH),
    }
}

/// `GET /web/admin/curate`: the form that curates a memory, filled from
/// the memory `memory=` names.
pub async fn curate_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let (mut source, mut prefill) = (String::new(), String::new());
    if let Some(id) = fields.given("memory").filter(|v| is_id(v)) {
        match admin_read(&web, &session, &calls, id).await {
            Ok(read) => {
                source = read.artifact_id;
                prefill = read.text;
            }
            Err(e) => return outcome::page(&e, calls.page_id(), ADMIN_CURATE_PATH),
        }
    }
    let mut body = String::from(
        r#"<h1>Curate</h1><p class="desc">The copy is held until someone outside its lineage reviews it, then served without its author.</p><details class="help"><summary>?</summary>Curate only an approved memory. Reword it: a copy repeating its source's text or name can be linked to it.</details>"#,
    );
    body.push_str(&form(
        ADMIN_CURATE_PATH,
        "curate",
        &session.csrf(),
        &format!(
            "{}{}{}{}",
            input("source", "source memory id", &source, true),
            input("claim_key", "claim key (optional)", "", false),
            textarea("text", "text", &prefill),
            acts::nonce_field()
        ),
        "Curate",
        true,
    ));
    page(
        &web,
        shell(
            "Administration",
            &session.who(),
            &ADMIN_TABS,
            ADMIN_CURATE_PATH,
            notice(&fields),
            &body,
        ),
        false,
    )
}

/// `POST /web/admin/curate`: a reworded copy of `source` in the shared
/// area, as the person's curator key, held until reviewed.
pub async fn curate_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let (fields, _) = match acts::read(
        form,
        CURATE_ACTIONS,
        |_, n| matches!(n, "source" | "text" | "claim_key"),
        &calls,
    ) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let source = match id_in(&fields, "source", &calls) {
        Ok(id) => id.to_owned(),
        Err(refusal) => return *refusal,
    };
    let Some(body) = fields.given("text") else {
        return acts::invalid("text is required", &calls);
    };
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Curate);
    let ids = calls.next_ids();
    let derives_from = [source.as_str()];
    match web
        .api
        .learn(
            &call(&credential, &ids),
            &key,
            &oml_sdk::Learn {
                body,
                workarea: UNIVERSE,
                derives_from: &derives_from,
                claim_key: fields.given("claim_key"),
            },
        )
        .await
    {
        Ok(_) => acts::see_other(&format!("{ADMIN_CURATE_PATH}?done=curated"), &[]),
        Err(e) => curator_refused(&e, &calls, ADMIN_CURATE_PATH),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_supersession_is_refused_unless_the_read_names_a_kind_other_than_guardrail() {
        assert!(refuses_supersession(None), "an API that names no kind");
        assert!(refuses_supersession(Some("guardrail")));
        assert!(!refuses_supersession(Some("memory")));
        assert!(!refuses_supersession(Some("derived_fact")));
    }

    /// A key with no name is named by its id, so "written by key" is never
    /// empty (#253, m5).
    #[test]
    fn the_attribution_names_a_nameless_authoring_key_by_its_id() {
        let read: AdminRead = serde_json::from_value(serde_json::json!({
            "artifact_id": "m1", "claim_key": "c", "version": 1, "state": "promoted",
            "workarea_path": "universe.alice", "text": "t", "key": null,
            "key_id": "6f1c2d4e-0000-4000-8000-0000000000aa",
            "principal": "alice", "user": "alice", "created_at": "2026-11-11T10:00:00Z",
        }))
        .unwrap();
        let page = attribution(&read, "");
        assert!(
            page.contains("6f1c2d4e-0000-4000-8000-0000000000aa (alice)"),
            "{page}"
        );
        let named: AdminRead = serde_json::from_value(serde_json::json!({
            "artifact_id": "m1", "claim_key": "c", "version": 1, "state": "promoted",
            "workarea_path": "universe.alice", "text": "t", "key": "alice-laptop",
            "key_id": "6f1c2d4e-0000-4000-8000-0000000000aa",
            "principal": "alice", "user": null, "created_at": "2026-11-11T10:00:00Z",
        }))
        .unwrap();
        let page = attribution(&named, "");
        assert!(page.contains("alice-laptop"), "{page}");
        assert!(
            !page.contains("0000000000aa"),
            "a named key shows its name: {page}"
        );
    }

    #[test]
    fn a_crafted_notice_shows_only_the_views_own_words() {
        let fields = |q: &str| Fields::of(&format!("/x?{q}").parse::<Uri>().unwrap()).unwrap();
        assert_eq!(
            notice(&fields("done=proposed")),
            Some("Proposed: held until reviewed.")
        );
        assert_eq!(notice(&fields("done=%3Cscript%3E")), None);
        assert_eq!(notice(&fields("done=curated:+anything")), None);
    }
}
