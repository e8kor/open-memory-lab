//! The admin acts of the administration side (ADR-0097 d2, d23): the list
//! of the tenant's acts, and one act's confirm screen, which shows what a
//! confirmation would apply (the key's role, grants, `used_by`, principal,
//! the root's person, the person who took the act and its reason) and
//! confirms or declines it. A confirmation and a decline each carry a
//! reason (d26); a confirmation is refused by the API to the act's own person and to an admin person whose
//! flag is under seven days older than the act (d11).

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{attr, form, hidden, input, shell, text};
use oml_sdk::{AdminAct, encode};

use crate::keys::instant;
use crate::pages::{Fields, refused};
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, acts, outcome};

/// The admin acts list, and one act's confirm screen at `?act=<id>`.
pub const ACTS_PATH: &str = "/web/admin/acts";

/// The confirm screen's acts.
pub const ACTS_ACTIONS: &[&str] = &["confirm", "decline"];

/// The most acts the list draws, newest first.
const LIST_MAX: usize = 200;

fn defined(action: &str, name: &str) -> bool {
    match action {
        "confirm" | "decline" => matches!(name, "act" | "reason"),
        _ => false,
    }
}

/// The session's own person, the one a confirmation never names.
fn own_person(session: &Session) -> Option<&str> {
    session.whoami.user.as_ref().map(|u| u.name.as_str())
}

/// Whether `act` waits on a second admin person (d5, d23).
fn pending(act: &AdminAct) -> bool {
    act.state == "pending_confirmation"
}

/// Whether a decline undoes `act`: a widening act a `sole_admin` tenant
/// took at once (d10; the operator's decision of 2026-11-27).
fn undoable(act: &AdminAct) -> bool {
    act.state == "effective" && act.sole_admin && act.class == "widening"
}

/// The words an act's spelling names, for a reader (ADR-0097 d2).
pub(crate) fn act_words(act: &str) -> String {
    act.replace('_', " ")
}

/// Who took `act`: the person the acting key counted as, or the
/// non-interactive cause.
pub(crate) fn taken_by(act: &AdminAct) -> String {
    match (&act.actor_person, &act.cause) {
        (Some(person), _) => format!(r#"<span class="mono">{}</span>"#, text(person)),
        (None, Some(cause)) => format!("the server ({})", text(cause)),
        (None, None) => "an admin key that counts as no person".to_owned(),
    }
}

/// One row of an act list: what, its class and state, who, when, the
/// reason, and the link to its screen.
pub(crate) fn act_row(act: &AdminAct) -> String {
    format!(
        r#"<li><span class="oml-decision">{}</span> · {} · <span class="oml-chip">{}</span>{} · by {} · {} · {} · <a href="{ACTS_PATH}?act={}">Open</a></li>"#,
        text(&act_words(&act.act)),
        text(&act.class.replace('_', " ")),
        text(&act.state.replace('_', " ")),
        if act.sole_admin { " · sole admin" } else { "" },
        taken_by(act),
        instant(&act.at),
        act.reason
            .as_deref()
            .map_or_else(|| "no reason given".to_owned(), text),
        attr(&encode(&act.act_id)),
    )
}

/// A field of the act's target, as text, or `None`.
fn target_text<'a>(act: &'a AdminAct, field: &str) -> Option<&'a str> {
    act.target.get(field).and_then(serde_json::Value::as_str)
}

/// The confirm screen's facts (d23): role, grants, `used_by`, principal,
/// the root's person, the person who took the act and its reason, with a
/// flag where the principal spells an existing person's name.
fn facts(act: &AdminAct) -> String {
    let mut rows: Vec<(String, String)> = vec![
        ("Act".to_owned(), text(&act_words(&act.act))),
        ("Class".to_owned(), text(&act.class.replace('_', " "))),
        ("State".to_owned(), text(&act.state.replace('_', " "))),
        ("Taken by".to_owned(), taken_by(act)),
        ("Taken".to_owned(), instant(&act.at)),
        (
            "Reason".to_owned(),
            act.reason
                .as_deref()
                .map_or_else(|| "none given".to_owned(), text),
        ),
    ];
    for (field, label) in [
        ("name", "Key"),
        ("role", "Role"),
        ("used_by", "Used by"),
        ("principal", "Principal"),
        ("user", "The root's person"),
        ("workspace_id", "Workspace"),
        ("email", "Email"),
    ] {
        if let Some(value) = target_text(act, field) {
            rows.push((
                label.to_owned(),
                format!(r#"<span class="mono">{}</span>"#, text(value)),
            ));
        }
    }
    if act.target.get("principal_names_person") == Some(&serde_json::Value::Bool(true)) {
        rows.push((
            "Check".to_owned(),
            r#"<span class="oml-chip is-adminfix">the principal spells an existing person's name</span>"#
                .to_owned(),
        ));
    }
    if act.sole_admin {
        rows.push((
            "Marked".to_owned(),
            "sole admin: taken at once, since the tenant had only ever had one admin person"
                .to_owned(),
        ));
    }
    if let Some(expires) = &act.expires_at {
        rows.push(("Expires as declined".to_owned(), instant(expires)));
    }
    if let Some(by) = &act.confirmed_by_person {
        rows.push((
            "Confirmed by".to_owned(),
            format!(r#"<span class="mono">{}</span>"#, text(by)),
        ));
    }
    if let Some(why) = &act.confirm_reason {
        rows.push(("Reason for the confirmation".to_owned(), text(why)));
    }
    let mut out = String::from(r#"<dl class="oml-facts">"#);
    for (label, value) in rows {
        let _ = write!(out, "<dt>{}</dt><dd>{value}</dd>", text(&label));
    }
    out.push_str("</dl>");
    if !act.grants.is_empty() {
        // A list, not a table: a grant's line wraps at a phone's width.
        out.push_str(r#"<h2>Grants a confirmation applies</h2><ul class="oml-todo">"#);
        for grant in &act.grants {
            let field = |name: &str| grant.get(name).and_then(serde_json::Value::as_str);
            let denies = grant
                .get("deny")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            let _ = write!(
                out,
                r#"<li><span class="oml-decision">{}</span> on <span class="mono">{}</span> · covers the {} · {} denial{}</li>"#,
                text(field("role").unwrap_or("?")),
                text(field("node").unwrap_or("?")),
                text(field("covers").unwrap_or("?")),
                denies,
                if denies == 1 { "" } else { "s" },
            );
        }
        out.push_str("</ul>");
    }
    out
}

fn notice(done: Option<&str>) -> Option<&'static str> {
    match done? {
        "confirmed" => Some("confirmed: the act took effect in the confirmation's commit"),
        "declined" => Some(
            "declined: the act never takes effect, and one taken at once under sole admin was undone",
        ),
        _ => None,
    }
}

fn not_found(calls: &PageCalls) -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "access.not_found",
        "no such page here",
        calls,
    )
}

/// `GET /web/admin/acts`: the tenant's admin acts, newest first, or with
/// `?act=<id>` that act's confirm screen. One API read either way.
pub async fn page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if !session.admin {
        return not_found(&calls);
    }
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let credential = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    let listed = match web.api.admin_acts(&call(&credential, &ids), "all").await {
        Ok(listed) => listed.items,
        Err(e) => return outcome::page(&e, &calls, ACTS_PATH),
    };
    let body = match fields.given("act") {
        Some(id) => match listed.iter().find(|a| a.act_id == id) {
            Some(act) => screen(&session, act),
            None => return not_found(&calls),
        },
        None => list(&listed),
    };
    outcome::html(
        StatusCode::OK,
        shell(
            "Admin acts",
            &session.administration(),
            ACTS_PATH,
            notice(fields.get("done")),
            &body,
        ),
    )
}

fn list(listed: &[AdminAct]) -> String {
    let mut body = String::from(
        r#"<header class="oml-section-head"><div class="top"><h1>Admin acts</h1></div><p class="desc">Every act an admin took outside content review, newest first: narrowing and neutral acts took effect at once, and widening waits on a second admin person unless the tenant has only ever had one.</p></header>"#,
    );
    if listed.is_empty() {
        body.push_str(r#"<div class="oml-empty"><p>No admin act is recorded.</p></div>"#);
        return body;
    }
    body.push_str(r#"<ul class="oml-todo">"#);
    for act in listed.iter().rev().take(LIST_MAX) {
        body.push_str(&act_row(act));
    }
    body.push_str("</ul>");
    if listed.len() > LIST_MAX {
        let _ = write!(
            body,
            r#"<p class="desc">The newest {LIST_MAX} of {} acts; <code>oml admin acts --state all</code> lists every one.</p>"#,
            listed.len()
        );
    }
    body
}

/// One act's confirm screen (d23): its facts, and the forms the act admits
/// for this session's person.
fn screen(session: &Session, act: &AdminAct) -> String {
    let mut body = format!(
        r#"<header class="oml-section-head"><div class="top"><h1>Admin act</h1></div><p class="desc">{}</p></header>"#,
        if pending(act) {
            "Nothing of this act took effect: it applies only once a second admin person confirms it, and a decline is final."
        } else if undoable(act) {
            "This act took effect at once under sole admin: a decline undoes it with the matching narrowing act."
        } else {
            "This act is settled here: nothing waits on a confirmation."
        }
    );
    body.push_str(&facts(act));
    let me = own_person(session);
    if !(pending(act) || undoable(act)) {
        return body;
    }
    let csrf = session.csrf();
    let id = hidden("act", &act.act_id);
    if pending(act) && act.actor_person.as_deref() != me {
        body.push_str(&form(
            ACTS_PATH,
            "confirm",
            &csrf,
            &format!(
                "{id}{}{}",
                input("reason", "Reason for the confirmation", "", true),
                acts::nonce_field()
            ),
            "Confirm",
            true,
        ));
    } else if pending(act) {
        body.push_str(
            r#"<p class="desc">This act is this person's own: another admin person confirms it.</p>"#,
        );
    }
    body.push_str(&form(
        ACTS_PATH,
        "decline",
        &csrf,
        &format!(
            "{id}{}{}",
            input("reason", "Reason for the decline", "", true),
            acts::nonce_field()
        ),
        "Decline",
        false,
    ));
    body
}

/// `POST /web/admin/acts`: confirm or decline one act; each requires a
/// reason. A success answers `303` back to the act's screen.
pub async fn act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if !session.admin {
        return not_found(&calls);
    }
    let (fields, action) = match acts::read(form, ACTS_ACTIONS, defined, &calls) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let Some(act_id) = fields.given("act").filter(|a| crate::todo::is_id(a)) else {
        return acts::invalid("act is an admin act's id", &calls);
    };
    let reason = fields.given("reason");
    // A confirmation and a decline each carry a reason (ADR-0097 d26;
    // amendment of 2026-11-27, answer 18).
    let Some(reason) = reason else {
        return acts::invalid(
            if action == "decline" {
                "a decline carries a reason"
            } else {
                "a confirmation carries a reason"
            },
            &calls,
        );
    };
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    let back = format!("{ACTS_PATH}?act={}", encode(act_id));
    let answer = if action == "confirm" {
        web.api
            .confirm_admin_act(&call(&credential, &ids), &key, act_id, reason)
            .await
            .map(|_| "confirmed")
    } else {
        web.api
            .decline_admin_act(
                &call(&credential, &ids),
                &key,
                act_id,
                reason,
            )
            .await
            .map(|_| "declined")
    };
    match answer {
        Ok(done) => acts::see_other(&format!("{back}&done={done}"), &[]),
        Err(e) => outcome::page(&e, &calls, &back),
    }
}
