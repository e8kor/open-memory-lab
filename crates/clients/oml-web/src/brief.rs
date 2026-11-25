//! A workspace's brief and status acts on its own page (ADR-0046 d7, d8;
//! ADR-0095 d5, d6 and the amendment of 2026-11-24; ADR-0096, frames P7
//! and P9 of `WorkspaceManage`).
//!
//! `GET /web/workspace?workspace=&kind=brief` lists the brief's members as
//! the node read serves them and its pending proposals as `GET
//! /v2/edges?kind=brief` lists them, each with **Ratify** as
//! `<user>-review` unless that key proposed it (ADR-0001 R2). Outside the
//! person's root the page also offers the status moves `GET
//! /v2/workspaces/{w}/offers` names, as `<user>-review`, each confirmed
//! before its one POST; a user root takes no status move (ADR-0085 d17).
//! `POST /web/workspace` runs the two acts, and a second live proposal is
//! drawn from the row the `409 proposal.pending` names: who made it, by
//! name only to the same person, and when.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::Response;
use http::StatusCode;
use oml_page::web::{USER_MEMORY_PATH, USER_WORKSPACES_PATH, attr, form, hidden, shell, text};
use oml_sdk::{BriefEdge, Error, ProposedBy, encode};

use crate::keys::instant;
use crate::pages::{Fields, refused};
use crate::session::{PageCalls, Session, call};
use crate::todo::is_id;
use crate::workspace::{
    Kind, Place, WORKSPACE_PATH, brief_members, crumbs, kinds_nav, notice, place,
};
use crate::{Capacity, Web, acts, outcome};

/// The workspace page's acts (ADR-0095 d5, d6).
pub const WORKSPACE_ACTIONS: &[&str] = &["propose_status", "ratify_brief"];

/// The refusal code of a second live proposal (ADR-0095 d5, d6).
pub(crate) const PENDING: &str = "proposal.pending";

/// `value` as inline code.
fn code(value: &str) -> String {
    format!("<code>{}</code>", text(value))
}

/// Who made a live proposal: the key's name where the read serves it, to
/// a key of the same person, else "another key" (ADR-0095, amendment of
/// 2026-11-24).
pub(crate) fn proposer(by: Option<&ProposedBy>) -> String {
    match by {
        Some(by) => code(by.name.as_deref().unwrap_or(&by.id)),
        None => "another key".to_owned(),
    }
}

/// "made <date> by <key>", from the row alone.
pub(crate) fn made(at: Option<&str>, by: Option<&ProposedBy>) -> String {
    match at {
        Some(at) => format!("made {} by {}", instant(at), proposer(by)),
        None => format!("made by {}", proposer(by)),
    }
}

/// The `details` member `name` of a `409 proposal.pending`, where `e` is
/// one.
pub(crate) fn pending_id<'a>(e: &'a Error, name: &str) -> Option<&'a str> {
    match e {
        Error::Api(a) if a.code == PENDING => a.details[name].as_str().filter(|id| is_id(id)),
        _ => None,
    }
}

/// `GET /web/workspace?kind=brief` (P7): the members, one line on what an
/// agent is served, and the pending proposals with Ratify.
pub(crate) async fn page(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    fields: &Fields,
    next: &str,
) -> Response {
    let place = match place(web, session, calls, fields.get("workspace"), next).await {
        Ok(place) => place,
        Err(refusal) => return *refusal,
    };
    let acting = session.credential(place.capacity);
    let ids = calls.next_ids();
    let edges = web
        .api
        .brief_edges(&call(&acting, &ids), &place.view.id)
        .await;
    let review = format!("{}-review", place.user);
    let mut body = crumbs(&place, Kind::Skill, None, false);
    let _ = write!(body, r#"<h1 class="mono">{}</h1>"#, text(&place.reference));
    body.push_str(&kinds_nav(
        &place.reference,
        None,
        place.view.approved_by_kind.as_ref(),
        Some(brief_members(&place)),
        false,
    ));
    let (members, budget) = place.view.brief.as_ref().map_or((&[][..], None), |b| {
        (&b.items[..], b.budget.and_then(|budget| budget.max_tokens))
    });
    let _ = write!(
        body,
        r#"<h2>Brief · {}</h2><p class="desc">An agent connected here is served these approved memories in ratification order{}. A superseded or retired memory leaves the brief by itself.</p>"#,
        members.len(),
        budget.map_or_else(String::new, |n| format!(", up to {n} tokens"))
    );
    if members.is_empty() {
        body.push_str(r#"<p class="dim">No memory is in this brief. Propose an approved memory from its page.</p>"#);
    } else {
        body.push_str(r#"<ul class="oml-todo">"#);
        for m in members {
            let _ = write!(
                body,
                r#"<li><a href="{USER_MEMORY_PATH}?id={}">{}</a> · v{}</li>"#,
                attr(&encode(&m.artifact_id)),
                text(m.title.as_deref().unwrap_or(&m.claim_key)),
                m.version
            );
        }
        body.push_str("</ul>");
    }
    match edges {
        Ok(page) => {
            let pending: Vec<&BriefEdge> =
                page.items.iter().filter(|e| e.state == "pending").collect();
            let _ = write!(
                body,
                r#"<h2>Proposals · {}{}</h2><p class="desc">Each joins once a key and principal other than its proposer's ratifies it. {} ratifies here, marked same user on a proposal by another key of this account.</p>"#,
                pending.len(),
                if page.next_cursor.is_some() { "+" } else { "" },
                code(&review)
            );
            if pending.is_empty() {
                body.push_str(r#"<p class="dim">No proposal waits for this brief.</p>"#);
            } else {
                body.push_str(r#"<ul class="oml-todo">"#);
                let csrf = session.csrf();
                for e in pending {
                    let own = e.proposed_by.as_ref().and_then(|b| b.name.as_deref())
                        == Some(review.as_str());
                    let act = if own {
                        format!(
                            r#"<span class="desc">{} proposed it, so another key ratifies it.</span>"#,
                            code(&review)
                        )
                    } else {
                        form(
                            WORKSPACE_PATH,
                            "ratify_brief",
                            &csrf,
                            &format!(
                                "{}{}",
                                hidden("workspace", &place.reference),
                                hidden("edge_id", &e.edge_id)
                            ),
                            "Ratify",
                            false,
                        )
                    };
                    let _ = write!(
                        body,
                        r#"<li><span class="oml-decision">Brief</span> <a href="{USER_MEMORY_PATH}?id={}">{}</a> · {}{} {act}</li>"#,
                        attr(&encode(&e.from_artifact_id)),
                        code(&e.from_artifact_id),
                        made(e.proposed_at.as_deref(), e.proposed_by.as_ref()),
                        if e.proposed_by.is_some() {
                            " · same user"
                        } else {
                            ""
                        },
                    );
                }
                body.push_str("</ul>");
            }
        }
        Err(e) => {
            let _ = write!(
                body,
                r#"<p class="error" role="status">The proposals are unavailable: {}</p>"#,
                text(&e.to_string())
            );
        }
    }
    outcome::html(
        StatusCode::OK,
        shell(
            &place.reference,
            &session.personal(),
            USER_WORKSPACES_PATH,
            notice(fields.get("done")),
            &body,
        ),
    )
}

/// The status section of a workspace page (P9): in a user root one line
/// that status does not change there; outside it the moves the offers
/// name as `<user>-review`, the live proposals with who and when, and,
/// where `propose` names a move, its confirmation. `pending` names the
/// live transition a `409 proposal.pending` refused a second one for.
pub(crate) async fn status_section(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    place: &Place,
    fields: &Fields,
) -> String {
    // A user root takes no status move (ADR-0085 d17), so none is offered.
    if place.in_root || place.capacity != Capacity::Review {
        return String::new();
    }
    let review = session.credential(Capacity::Review);
    let ids = calls.next_ids();
    let offers = match web
        .api
        .workspace_offers(&call(&review, &ids), &place.view.id)
        .await
    {
        Ok(offers) => offers,
        Err(_) => return String::new(),
    };
    let mut out = format!(
        r#"<h2>Status</h2><p class="desc">{} is {}. A move is proposed as {} and changes nothing until a key and principal other than the proposer's ratifies it.</p>"#,
        text(&place.reference),
        text(&place.view.status),
        code(&format!("{}-review", place.user))
    );
    if let Some(id) = fields.given("pending") {
        let live = place
            .view
            .pending_transitions
            .iter()
            .find(|t| t.transition_id == id);
        let _ = write!(
            out,
            r#"<p class="error" role="status"><code>{PENDING}</code> {}</p>"#,
            live.map_or_else(
                || "A proposal to this status is already live.".to_owned(),
                |t| format!(
                    "A proposal to {} is already live, {}.",
                    text(&t.to),
                    made(t.proposed_at.as_deref(), t.proposed_by.as_ref())
                )
            )
        );
    }
    for t in &place.view.pending_transitions {
        let _ = write!(
            out,
            r#"<p class="desc">A proposal to {} is live, {}.</p>"#,
            text(&t.to),
            made(t.proposed_at.as_deref(), t.proposed_by.as_ref())
        );
    }
    // A status with a live proposal is offered no second one: the server
    // would answer `proposal.pending` (ADR-0095 d6).
    let proposable: Vec<&str> = offers
        .transitions
        .iter()
        .filter(|t| t.op == "propose")
        .map(|t| t.to.as_str())
        .filter(|to| !place.view.pending_transitions.iter().any(|p| p.to == *to))
        .collect();
    match fields.given("propose") {
        Some(to) if proposable.contains(&to) => {
            let _ = write!(
                out,
                r#"<div class="oml-card"><h3>Propose {} for {}?</h3><p class="desc">{}</p>{}<p><a href="{WORKSPACE_PATH}?workspace={}">Keep the status</a></p></div>"#,
                text(to),
                text(&place.reference),
                if to == "frozen" {
                    "Once ratified, nothing new is written here: no version, no workspace below it."
                } else {
                    "Once ratified, every write here needs the workspace's review policy, and a person approves every skill and guardrail."
                },
                form(
                    WORKSPACE_PATH,
                    "propose_status",
                    &session.csrf(),
                    &format!(
                        "{}{}{}",
                        hidden("workspace", &place.reference),
                        hidden("to", to),
                        acts::nonce_field()
                    ),
                    &format!("Propose {to}"),
                    true,
                ),
                attr(&encode(&place.reference))
            );
        }
        _ => {
            for to in proposable {
                let _ = write!(
                    out,
                    r#"<p><a class="button" href="{WORKSPACE_PATH}?workspace={}&amp;propose={}">Propose {}</a></p>"#,
                    attr(&encode(&place.reference)),
                    attr(&encode(to)),
                    text(to)
                );
            }
        }
    }
    out
}

/// The fields each act defines, beside `action`, `csrf_token` and `nonce`.
fn defined(action: &str, name: &str) -> bool {
    match action {
        "propose_status" => matches!(name, "workspace" | "to"),
        "ratify_brief" => matches!(name, "workspace" | "edge_id" | "confirm"),
        _ => false,
    }
}

/// `POST /web/workspace`: **Propose** a status move as `<user>-review`, or
/// **Ratify** a brief proposal as `<user>-review` after its confirmation;
/// each answers `303` to the workspace page with its notice, and a second
/// live status proposal to the page naming the live one.
pub async fn act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form_sent: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    let (fields, action) = match acts::read(form_sent, WORKSPACE_ACTIONS, defined, &calls) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let Some(workspace) = fields.given("workspace") else {
        return acts::invalid("workspace is required", &calls);
    };
    let back = format!("{WORKSPACE_PATH}?workspace={}", encode(workspace));
    let review = session.credential(Capacity::Review);
    if action == "ratify_brief" {
        let Some(edge) = fields.given("edge_id").filter(|id| is_id(id)) else {
            return acts::invalid("edge_id is a brief proposal's id", &calls);
        };
        if fields.given("confirm") != Some("yes") {
            let reviewer = session.whoami.user.as_ref().map_or_else(
                || "the reviewer key".to_owned(),
                |u| format!("{}-review", u.name),
            );
            let body = format!(
                r#"<h1>Ratify the brief proposal?</h1><p class="desc">The memory joins the brief of {}, ratified by {}. An agent is served the brief up to its budget.</p>{}<p><a href="{back}&amp;kind=brief">Keep it waiting</a></p>"#,
                code(workspace),
                code(&reviewer),
                form(
                    WORKSPACE_PATH,
                    "ratify_brief",
                    &session.csrf(),
                    &format!(
                        "{}{}{}{}",
                        hidden("workspace", workspace),
                        hidden("edge_id", edge),
                        hidden("confirm", "yes"),
                        acts::nonce_field()
                    ),
                    "Ratify",
                    true
                )
            );
            return outcome::html(
                StatusCode::OK,
                shell(
                    "Confirm",
                    &session.personal(),
                    USER_WORKSPACES_PATH,
                    None,
                    &body,
                ),
            );
        }
        let key = match acts::key(&session, &fields, &calls) {
            Ok(key) => key,
            Err(refusal) => return *refusal,
        };
        let _in_flight = acts::hold(&key).await;
        let ids = calls.next_ids();
        return match web.api.ratify_edge(&call(&review, &ids), &key, edge).await {
            Ok(_) => acts::see_other(&format!("{back}&kind=brief&done=ratified"), &[]),
            Err(e) => outcome::page(&e, &calls, WORKSPACE_PATH),
        };
    }
    let Some(to) = fields
        .given("to")
        .filter(|to| matches!(*to, "protected" | "frozen"))
    else {
        return refused(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "to is protected or frozen",
            &calls,
        );
    };
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let ids = calls.next_ids();
    match web
        .api
        .propose_transition(&call(&review, &ids), &key, workspace, to)
        .await
    {
        Ok(_) => acts::see_other(&format!("{back}&done=status_proposed"), &[]),
        Err(e) => match pending_id(&e, "transition_id") {
            Some(live) => acts::see_other(&format!("{back}&pending={live}"), &[]),
            None => outcome::page(&e, &calls, WORKSPACE_PATH),
        },
    }
}

/// The line a live transition is drawn with, for tests of the wording.
#[cfg(test)]
fn live_line(t: &oml_sdk::Transition) -> String {
    format!(
        "A proposal to {} is live, {}.",
        t.to,
        made(t.proposed_at.as_deref(), t.proposed_by.as_ref())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition(by: Option<&str>) -> oml_sdk::Transition {
        oml_sdk::Transition {
            transition_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            from: "development".to_owned(),
            to: "protected".to_owned(),
            state: "pending".to_owned(),
            proposed_at: Some("2026-11-25T10:00:00Z".to_owned()),
            proposed_by: by.map(|name| ProposedBy {
                id: "00000000-0000-4000-8000-000000000002".to_owned(),
                name: Some(name.to_owned()),
            }),
        }
    }

    #[test]
    fn a_live_proposal_names_its_key_only_where_the_read_serves_it() {
        assert!(live_line(&transition(Some("alice-review"))).contains("<code>alice-review</code>"));
        let other = live_line(&transition(None));
        assert!(other.contains("by another key"), "{other}");
        assert!(!other.contains("alice"), "{other}");
    }

    #[test]
    fn a_proposal_pending_refusal_names_its_live_row_and_nothing_else_does() {
        let refusal = |code: &str| {
            Error::Api(Box::new(oml_sdk::ApiError {
                status: 409,
                code: code.to_owned(),
                message: "pending".to_owned(),
                details: serde_json::json!({"edge_id": "00000000-0000-4000-8000-000000000003"}),
                request_id: None,
                retry_after: None,
            }))
        };
        assert_eq!(
            pending_id(&refusal(PENDING), "edge_id"),
            Some("00000000-0000-4000-8000-000000000003")
        );
        assert_eq!(pending_id(&refusal(PENDING), "transition_id"), None);
        assert_eq!(pending_id(&refusal("name.taken"), "edge_id"), None);
    }
}
