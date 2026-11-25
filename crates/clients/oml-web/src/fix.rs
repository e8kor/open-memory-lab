//! The admin workspace page's **Fix** tab,
//! `/web/admin/workspace?workspace=<w>&view=fix` (ADR-0096 d5, amending
//! ADR-0083 d9 while the Releases view is unbuilt): the fixes still
//! unconfirmed here, and a form for one act of `POST /v2/admin/fixes` on a
//! target taken from an open investigation's memory page, reviewed once
//! (`step=review`) before it posts to `/web/admin/fixes`.
//!
//! The tab never resolves a claim key to a version on its own: the admin
//! reads a claim only inside an investigation, each read journalled
//! (ADR-0089 d10), and the memory page's link carries the version here.
//! The fix is served at once and waits for a person with reviewer on the
//! workspace, who confirms or reverts it; which person may is what
//! `GET /v2/review/fixes` serves that person as `acts`, never computed here
//! (ADR-0096 d5).

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::Response;
use http::StatusCode;
use oml_page::web::{ADMIN_WORKSPACES_PATH, attr, form, hidden, shell, short, text};
use oml_sdk::encode;
use serde_json::json;

use crate::keys::instant;
use crate::pages::{Fields, refused};
use crate::session::{PageCalls, Session, call};
use crate::todo::is_id;
use crate::workspace::ADMIN_WORKSPACE_PATH;
use crate::{Capacity, Web, acts, outcome};

/// Where the tab's one act posts (ADR-0096 d5).
pub const FIXES_PATH: &str = "/web/admin/fixes";

/// The tab's acts.
pub const FIX_ACTIONS: &[&str] = &["fix"];

/// The acts a workspace's status offers: a frozen workspace takes a
/// demotion or an erasure alone (ADR-0089 d12).
pub(crate) fn acts_for(status: &str) -> &'static [(&'static str, &'static str)] {
    const ALL: [(&str, &str); 4] = [
        (
            "demote",
            "demote: stop serving this version; the one before it is served again",
        ),
        ("edit", "edit: serve a new text in its place"),
        ("retire", "retire: stop serving the claim"),
        ("erase", "erase: crypto-shred every version of the claim"),
    ];
    const FROZEN: [(&str, &str); 2] = [ALL[0], ALL[3]];
    if status == "frozen" { &FROZEN } else { &ALL }
}

/// The tab's body, drawn by the admin workspace page for `view=fix`.
pub(crate) async fn tab(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    fields: &Fields,
) -> Response {
    let Some(sent) = fields.given("workspace") else {
        return acts::invalid("workspace names the workspace to read", calls);
    };
    let admin = session.credential(Capacity::Admin);
    let sent_back = format!("{ADMIN_WORKSPACE_PATH}?workspace={}&view=fix", encode(sent));
    let ids = calls.next_ids();
    let view = match web.api.workspace(&call(&admin, &ids), sent).await {
        Ok(view) => view,
        Err(e) => return outcome::page(&e, calls, &sent_back),
    };
    // The tab names the workspace by its id from here on: a nested
    // workspace's bare name names no workspace on its own (ADR-0096 d5).
    let next = format!(
        "{ADMIN_WORKSPACE_PATH}?workspace={}&view=fix",
        encode(&view.id)
    );
    let ids = calls.next_ids();
    let fixes = match web.api.admin_fixes(&call(&admin, &ids)).await {
        Ok(fixes) => fixes,
        Err(e) => return outcome::page(&e, calls, &next),
    };
    let here: Vec<_> = fixes.iter().filter(|f| f.workspace_id == view.id).collect();
    let person = session
        .whoami
        .user
        .as_ref()
        .map_or("the admin's person", |u| u.name.as_str());
    // Who settles a fix is what `GET /v2/review/fixes` serves each
    // reviewer as `acts`; the admin's routes serve no `acts`, so the tab
    // names no person it would exclude (ADR-0096 d5; ADR-0089 d15).
    let mut body = format!(
        r#"<p class="label"><a href="{ADMIN_WORKSPACES_PATH}">Workspaces</a> / {}</p><h1 class="mono">{}</h1><nav class="oml-views kinds" aria-label="Views"><a href="{ADMIN_WORKSPACE_PATH}?workspace={}">Memories</a> <a aria-current="page" href="{}">Fix {}</a></nav><p class="desc">Acting as <code>{}-admin</code>. A fix is served at once, marked admin fix, and waits for a person with reviewer on the workspace to confirm or revert it, as that person's To do offers it.</p>"#,
        text(&view.name),
        text(&view.name),
        attr(&encode(&view.id)),
        attr(&next),
        here.len(),
        text(person),
    );
    body.push_str("<h2>Unconfirmed here</h2>");
    if here.is_empty() {
        body.push_str(r#"<p class="desc">No fix waits here.</p>"#);
    } else {
        body.push_str(r#"<ul class="oml-todo">"#);
        for f in &here {
            let _ = write!(
                body,
                r#"<li><span class="oml-decision">{}</span> <code>{}</code> · <span class="oml-chip is-adminfix">unconfirmed</span>{} · {} · {}</li>"#,
                text(&f.act),
                short(&f.claim_key),
                if f.loosens_guardrail { " · alert" } else { "" },
                text(&f.reason),
                instant(&f.fixed_at)
            );
        }
        body.push_str("</ul>");
    }
    body.push_str(&new_fix(session, &view.id, &view.status, fields));
    outcome::html(
        StatusCode::OK,
        shell(
            &view.name,
            &session.administration(),
            ADMIN_WORKSPACES_PATH,
            notice(fields.get("done")),
            &body,
        ),
    )
}

fn notice(done: Option<&str>) -> Option<&'static str> {
    match done? {
        "fixed" => Some(
            "fixed: served at once, waiting for a person with reviewer on the workspace to confirm or revert it",
        ),
        _ => None,
    }
}

/// **New fix**: the form, or with `step=review` its confirmation.
fn new_fix(session: &Session, workspace: &str, status: &str, fields: &Fields) -> String {
    let mut out = String::from("<h2>New fix</h2>");
    let Some(target) = fields.given("target").filter(|t| is_id(t)) else {
        out.push_str(r#"<p class="desc">No target: an investigation opens first. Open the memory on the investigation's page and follow <strong>Fix this version</strong>; the version it read becomes the target here.</p>"#);
        return out;
    };
    let offered = acts_for(status);
    let act = fields
        .given("act")
        .filter(|a| offered.iter().any(|(o, _)| o == a));
    let reason = fields.given("reason").unwrap_or("");
    let text_sent = fields.given("body").unwrap_or("");
    if let (Some("review"), Some(act)) = (fields.given("step"), act)
        && !reason.is_empty()
        && (act != "edit" || !text_sent.is_empty())
    {
        let consequence = match act {
            "erase" => {
                "Every version of the claim is crypto-shredded at once and served by no read."
            }
            "demote" => {
                "The version stops being served at once; the one before it is served again."
            }
            "retire" => "The claim stops being served at once.",
            _ => "The new text is served at once in the version's place.",
        };
        let _ = write!(
            out,
            r#"<section class="card"><h3>{} version <code>{}</code>?</h3><p class="oml-consequence">{consequence} The fix waits for a person with reviewer on the workspace to confirm or revert it.</p><p class="desc">Reason: {}</p>{}</section>"#,
            text(act),
            short(&target[..8.min(target.len())]),
            text(reason),
            {
                let mut hidden_fields = format!(
                    "{}{}{}{}",
                    hidden("workspace", workspace),
                    hidden("act", act),
                    hidden("target", target),
                    hidden("reason", reason)
                );
                if act == "edit" {
                    hidden_fields.push_str(&hidden("body", text_sent));
                }
                hidden_fields.push_str(&acts::nonce_field());
                let button = if act == "erase" { "Erase" } else { "Fix" };
                form(
                    FIXES_PATH,
                    "fix",
                    &session.csrf(),
                    &hidden_fields,
                    button,
                    true,
                )
                .replace(
                    r#"<button type="submit" class="primary">Erase</button>"#,
                    r#"<button type="submit" class="danger">Erase</button>"#,
                )
            }
        );
        return out;
    }
    let mut radios = String::new();
    for (value, label) in offered {
        let _ = write!(
            radios,
            r#"<label class="oml-check"><input type="radio" name="act" value="{value}"{}> {}</label>"#,
            if act == Some(value) { " checked" } else { "" },
            text(label)
        );
    }
    if status == "frozen" {
        radios.push_str(
            r#"<p class="desc">A frozen workspace takes a demotion or an erasure alone.</p>"#,
        );
    }
    let _ = write!(
        out,
        r#"<form class="oml-form" method="get" action="{ADMIN_WORKSPACE_PATH}">{}{}{}<fieldset class="oml-radios"><legend>Act</legend>{radios}</fieldset><p class="desc">Target: version <code>{}</code>, read on the investigation's page.</p><label class="wide">Text (edit only; the version's text is on the investigation's page)<textarea name="body">{}</textarea></label><label>Reason<input name="reason" value="{}" required></label><p class="oml-consequence">Erase crypto-shreds every version of the claim.</p><button type="submit" class="primary" name="step" value="review">Review fix</button></form>"#,
        hidden("workspace", workspace),
        hidden("view", "fix"),
        hidden("target", target),
        short(target),
        text(text_sent),
        attr(reason),
    );
    out
}

/// `POST /web/admin/fixes`: the reviewed fix, posted once to `POST
/// /v2/admin/fixes` as `<user>-admin`, back to the tab with its notice; a
/// refusal (`admin.fix_held_by_scan`, `admin.fix_target_not_promoted`,
/// `workspace.frozen`) is drawn by name.
pub async fn act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if !session.admin {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        );
    }
    let (fields, _) = match acts::read(
        form,
        FIX_ACTIONS,
        |_, n| matches!(n, "workspace" | "act" | "target" | "reason" | "body"),
        &calls,
    ) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let (Some(workspace), Some(fix), Some(target), Some(reason)) = (
        fields.given("workspace"),
        fields.given("act"),
        fields.given("target").filter(|t| is_id(t)),
        fields.given("reason"),
    ) else {
        return acts::invalid("workspace, act, target and reason are required", &calls);
    };
    if !matches!(fix, "demote" | "edit" | "retire" | "erase") {
        return acts::invalid("act is demote, edit, retire or erase", &calls);
    }
    let mut request = json!({"act": fix, "target": target, "reason": reason});
    if let Some(body) = fields.given("body").filter(|_| fix == "edit") {
        request["body"] = json!(body);
    }
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    let back = format!(
        "{ADMIN_WORKSPACE_PATH}?workspace={}&view=fix",
        encode(workspace)
    );
    match web
        .api
        .admin_fix(&call(&credential, &ids), &key, &request)
        .await
    {
        Ok(_) => acts::see_other(&format!("{back}&done=fixed"), &[]),
        Err(e) => outcome::page(&e, &calls, &back),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frozen_workspace_offers_demote_and_erase_alone() {
        let acts = |status: &str| acts_for(status).iter().map(|(a, _)| *a).collect::<Vec<_>>();
        assert_eq!(acts("frozen"), ["demote", "erase"]);
        assert_eq!(acts("protected"), ["demote", "edit", "retire", "erase"]);
        assert_eq!(acts("development"), ["demote", "edit", "retire", "erase"]);
    }
}
