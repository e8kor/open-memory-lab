//! The Workareas views' dropdowns (ADR-0045 d28): one `select` a workarea
//! of exactly the acts the acting keys may take there now, computed by the
//! verbs' own checks; Apply is a `GET` that changes nothing and asks again;
//! the confirmation's one `POST` runs an existing use case. A refusal at
//! either step is the verb's named refusal.

use std::fmt::Write as _;

use serde_json::json;
use uuid::Uuid;

use oml_app::app::{Caller, WorkareaOffer, WorkareaOffers};
use oml_app::domain::WorkareaStatus;
use oml_app::domain::name::Reference;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::Bearer;

use super::page::{attr, form, hidden, text};
use super::views::Fields;
use crate::ApiState;
use crate::error::{ApiError, ErrorContext, map_app_error};

/// One node of a Workareas view: its id, its path and the name a key uses.
pub(crate) struct Node {
    pub id: Uuid,
    pub path: String,
    pub name: String,
}

/// The value an option carries.
#[must_use]
pub(crate) fn offer_value(offer: &WorkareaOffer) -> String {
    match offer {
        WorkareaOffer::Propose(to) => format!("propose:{}", to.as_str()),
        WorkareaOffer::Ratify { transition_id, .. } => format!("ratify:{transition_id}"),
        WorkareaOffer::InProgress => "mode:in_progress".to_owned(),
        WorkareaOffer::Commit => "mode:standard".to_owned(),
    }
}

pub(crate) fn offer_text(offer: &WorkareaOffer) -> String {
    match offer {
        WorkareaOffer::Propose(to) => format!("propose {}", to.as_str()),
        WorkareaOffer::Ratify { from, to, .. } => {
            format!("ratify {} → {}", from.as_str(), to.as_str())
        }
        WorkareaOffer::InProgress => "put in progress".to_owned(),
        WorkareaOffer::Commit => "commit".to_owned(),
    }
}

/// The act an `op` names, its proposal's id resolved against the live
/// proposals on the node; an `op` outside the grammar names its field.
fn parse_op(op: &str, pending: &[WorkareaOffer]) -> Result<WorkareaOffer, ApiError> {
    let refused = || {
        ApiError::validation(
            "op is propose:committed, propose:validated, ratify:<transition>, mode:in_progress or mode:standard",
        )
        .with_details(json!({ "field": "op" }))
    };
    match op.split_once(':').ok_or_else(refused)? {
        ("propose", to) => WorkareaStatus::parse(to)
            .map(WorkareaOffer::Propose)
            .ok_or_else(refused),
        ("ratify", id) => {
            let id = Uuid::parse_str(id).map_err(|_| refused())?;
            Ok(pending
                .iter()
                .find(|o| matches!(o, WorkareaOffer::Ratify { transition_id, .. } if *transition_id == id))
                .copied()
                .unwrap_or(WorkareaOffer::Ratify {
                    transition_id: id,
                    from: WorkareaStatus::Development,
                    to: WorkareaStatus::Committed,
                }))
        }
        ("mode", "in_progress") => Ok(WorkareaOffer::InProgress),
        ("mode", "standard") => Ok(WorkareaOffer::Commit),
        _ => Err(refused()),
    }
}

/// What a confirmed act will do, in a sentence.
fn consequence(offer: &WorkareaOffer, path: &str) -> String {
    let path = text(path);
    match offer {
        WorkareaOffer::Propose(to) => format!(
            "Proposes that <code>{path}</code> becomes {}. It keeps its status until a key other than the one proposing ratifies it.",
            to.as_str()
        ),
        WorkareaOffer::Ratify { from, to, .. } => format!(
            "Ratifies the proposal that <code>{path}</code> moves from {} to {}. It is recorded under the key this page acts with; a proposer never ratifies their own proposal.",
            from.as_str(),
            to.as_str()
        ),
        WorkareaOffer::InProgress => format!(
            "Puts <code>{path}</code> in progress: your agents recall each other's unreviewed memories inside it, marked unreviewed, and nothing leaves it until it is committed."
        ),
        WorkareaOffer::Commit => format!(
            "Commits <code>{path}</code>: it leaves in progress, which needs every memory in it reviewed."
        ),
    }
}

/// The confirmation's one form: the existing action and its fields.
fn confirm_form(base: &str, csrf: &str, node: Uuid, offer: &WorkareaOffer) -> String {
    let id = node.to_string();
    let (action, fields) = match offer {
        WorkareaOffer::Propose(to) => (
            "workarea_transition",
            format!("{}{}", hidden("workarea", &id), hidden("to", to.as_str())),
        ),
        WorkareaOffer::Ratify { transition_id, .. } => (
            "workarea_ratify",
            format!(
                "{}{}",
                hidden("workarea", &id),
                hidden("transition", &transition_id.to_string())
            ),
        ),
        WorkareaOffer::InProgress => (
            "workarea_mode",
            format!(
                "{}{}",
                hidden("workarea", &id),
                hidden("mode", "in_progress")
            ),
        ),
        WorkareaOffer::Commit => (
            "workarea_mode",
            format!("{}{}", hidden("workarea", &id), hidden("mode", "standard")),
        ),
    };
    form(base, action, csrf, &fields, "Confirm", true)
}

/// A workarea's dropdown: its offered acts, a withheld commit shown
/// disabled with its count, and Apply only where something is offered.
fn dropdown(base: &str, node: Uuid, offers: &WorkareaOffers) -> String {
    if offers.offers.is_empty() && offers.commit_withheld.is_none() {
        return r#"<span class="dim">nothing to do here now</span>"#.to_owned();
    }
    let mut out = format!(
        r#"<form class="row act" method="get" action="{base}"><input type="hidden" name="view" value="workarea"><input type="hidden" name="workarea" value="{node}"><label>act<select name="op">"#
    );
    for offer in &offers.offers {
        let _ = write!(
            out,
            r#"<option value="{}">{}</option>"#,
            attr(&offer_value(offer)),
            text(&offer_text(offer))
        );
    }
    if let Some(held) = offers.commit_withheld {
        let _ = write!(
            out,
            r#"<option disabled selected>commit — {held} {} still held</option>"#,
            if held == 1 { "memory" } else { "memories" }
        );
    }
    out.push_str("</select></label>");
    if !offers.offers.is_empty() {
        out.push_str(r#"<button type="submit">Apply</button>"#);
    }
    out.push_str("</form>");
    out
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// The table of `nodes`, each with its status, mode, live proposals and
    /// dropdown, and above it the confirmation when the query names an
    /// `op`: transitions as `transitions`, the mode as `mode`.
    pub(crate) async fn workarea_table(
        &self,
        base: &str,
        csrf: &str,
        transitions: &Caller,
        mode: &Caller,
        nodes: &[Node],
        query: &Fields,
    ) -> Result<String, ApiError> {
        let mut confirm = String::new();
        if let Some(op) = query.get("op") {
            let named = query.need("workarea")?;
            let id = match query.reference("workarea")? {
                Reference::Id(id) => Some(id),
                _ => None,
            };
            let node = nodes
                .iter()
                .find(|n| id == Some(n.id) || named == n.name || named == n.path)
                .ok_or_else(ApiError::not_found)?;
            let offers = self
                .app
                .workarea_offers(transitions, mode, node.id)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let offer = parse_op(op, &offers.offers)?;
            let check = self
                .app
                .workarea_offer_check(transitions, mode, node.id, offer)
                .await;
            confirm = match check {
                Ok(()) => format!(
                    r#"<div class="card confirm"><p class="label">Confirm</p><h2>{} on {}</h2><p>{}</p>{}<p><a href="{base}?view=workarea">Cancel</a></p></div>"#,
                    text(&offer_text(&offer)),
                    text(&node.path),
                    consequence(&offer, &node.path),
                    confirm_form(base, csrf, node.id, &offer)
                ),
                Err(e) => {
                    let e = map_app_error(e, ErrorContext::Write);
                    format!(
                        r#"<div class="card confirm"><p class="label">Not now</p><p class="error">{}: {}</p><p><a href="{base}?view=workarea">Back</a></p></div>"#,
                        text(e.code),
                        text(&e.message)
                    )
                }
            };
        }
        let mut out = confirm;
        out.push_str(
            "<table><tr><th>Path</th><th>Name to use</th><th>Status</th><th>Mode</th><th>Act</th></tr>",
        );
        for node in nodes {
            let offers = self
                .app
                .workarea_offers(transitions, mode, node.id)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            // The live proposals, shown as proposed until another key
            // ratifies one (ADR-0009 d3).
            let pending = match self
                .app
                .workarea_status(Bearer::Session(transitions), node.id.into())
                .await
            {
                Ok(view) => view
                    .pending
                    .iter()
                    .map(|t| {
                        format!(
                            "<br><span class=\"held\">proposed: {}</span>",
                            t.to.as_str()
                        )
                    })
                    .collect::<String>(),
                Err(_) => String::new(),
            };
            let _ = write!(
                out,
                r#"<tr><td class="mono">{}</td><td class="mono">{}</td><td>{}{pending}</td><td>{}</td><td>{}</td></tr>"#,
                text(&node.path),
                text(&node.name),
                offers.status.as_str(),
                if offers.in_progress {
                    "in progress"
                } else {
                    "standard"
                },
                dropdown(base, node.id, &offers)
            );
        }
        out.push_str("</table>");
        Ok(out)
    }

    /// `workarea_transition`, `workarea_ratify` and `workarea_mode`
    /// (ADR-0045 d28): the existing use cases, transitions as
    /// `transitions` and the mode as `mode`; back to the view.
    pub(crate) async fn workarea_act(
        &self,
        base: &str,
        action: &str,
        transitions: &Caller,
        mode: &Caller,
        fields: &Fields,
    ) -> Result<String, ApiError> {
        let mapped = |e| map_app_error(e, ErrorContext::Write);
        match action {
            "workarea_transition" => {
                fields.only(&["workarea", "to"])?;
                let workarea = fields.reference("workarea")?;
                let to = WorkareaStatus::parse(fields.need("to")?).ok_or_else(|| {
                    ApiError::validation("to is committed or validated")
                        .with_details(json!({ "field": "to" }))
                })?;
                crate::retry_chain(|| {
                    self.app
                        .propose_transition(Bearer::Session(transitions), workarea.clone(), to)
                })
                .await
                .map_err(mapped)?;
                Ok(format!("{base}?view=workarea&done=proposed"))
            }
            "workarea_ratify" => {
                fields.only(&["workarea", "transition"])?;
                let workarea = fields.reference("workarea")?;
                let transition = fields.id("transition")?;
                crate::retry_chain(|| {
                    self.app.ratify_transition(
                        Bearer::Session(transitions),
                        workarea.clone(),
                        transition,
                    )
                })
                .await
                .map_err(mapped)?;
                Ok(format!("{base}?view=workarea&done=ratified"))
            }
            _ => {
                fields.only(&["workarea", "mode"])?;
                let workarea = fields.reference("workarea")?;
                let wanted = fields
                    .get("mode")
                    .and_then(oml_app::domain::in_progress::WorkareaMode::parse)
                    .ok_or_else(|| {
                        ApiError::validation("mode is in_progress or standard")
                            .with_details(json!({"field": "mode"}))
                    })?;
                crate::retry_chain(|| {
                    self.app
                        .set_workarea_mode(Bearer::Session(mode), workarea.clone(), wanted)
                })
                .await
                .map_err(mapped)?;
                Ok(format!("{base}?view=workarea&done=mode+set"))
            }
        }
    }
}
