//! The pages `oml-web` serves (ADR-0070 d7 (b)), each moved from the lead
//! behind the same URL, with its assertions unchanged
//! (`tests/web_pages.rs`). A page is drawn from the wire's fields only.
//!
//! Moved so far: the user page's Guardrails view, `GET /web?view=guardrails`
//! (ADR-0037 d17), first because it only reads and maps onto
//! `GET /v1/guardrails` (design/web-client-separation.md).

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::{Query, State};
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{attr, shell, text, user_tabs, workarea_label};
use oml_sdk::{Error, WorkareaNode};

use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, frame, outcome};

/// The tree's page size the selector reads, the API's maximum.
const TREE_PAGE: u32 = 200;

/// A query's fields, each at most once, as the lead reads a page's query.
struct Fields(Vec<(String, String)>);

impl Fields {
    fn of(uri: &Uri) -> Result<Self, String> {
        let Query(pairs) = Query::<Vec<(String, String)>>::try_from_uri(uri)
            .map_err(|e| format!("the query is not a page's: {e}"))?;
        let mut seen = std::collections::HashSet::new();
        for (name, _) in &pairs {
            if !seen.insert(name.as_str()) {
                return Err(format!("the field {name:?} is sent more than once"));
            }
        }
        Ok(Self(pairs))
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn refused(status: StatusCode, code: &str, message: &str, calls: &PageCalls) -> Response {
    outcome::html(
        status,
        oml_page::web::refusal_page(code, message, Some(calls.page_id())),
    )
}

/// `GET /web`: the user page's views that have moved here. A view still
/// served by the lead is not this process's, and the proxy never sends it.
pub async fn user_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    let fields = match Fields::of(&uri) {
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
    match fields.get("view") {
        Some("guardrails") => guardrails(&web, &session, &calls, &fields).await,
        _ => refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        ),
    }
}

/// Every workarea under the person's root, a page of the tree at a time.
async fn workareas(
    web: &Web,
    credential: &str,
    calls: &PageCalls,
    root: &str,
) -> Result<Vec<WorkareaNode>, (Error, String)> {
    let mut nodes = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let ids = calls.next_ids();
        let page = web
            .api
            .workarea_tree(&call(credential, &ids), root, TREE_PAGE, cursor.as_deref())
            .await
            .map_err(|e| (e, ids.request_id.unwrap_or_default()))?;
        nodes.extend(page.items);
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(nodes);
        }
    }
}

/// The guardrails in force at the selected workarea, read-only: the scope,
/// each rule with its source path, and a selector over the person's
/// workareas. The page writes, supersedes and retires none (ADR-0037 d17).
/// The set is the page's primary call and the tree its secondary one, made
/// together after the session's check (ADR-0070 d8): a failed set is the
/// error page, a failed tree the page without its selector.
async fn guardrails(web: &Web, session: &Session, calls: &PageCalls, fields: &Fields) -> Response {
    let Some(root) = session.whoami.user.as_ref().map(|u| u.id.clone()) else {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            calls,
        );
    };
    let credential = session.credential(Capacity::Web);
    let set_ids = calls.next_ids();
    let set_call = call(&credential, &set_ids);
    let (set, tree) = tokio::join!(
        web.api.guardrails(&set_call, fields.get("workarea")),
        workareas(web, &credential, calls, &root),
    );
    let set = match set {
        Ok(set) => set,
        Err(e) => return outcome::page(&e, calls.page_id()),
    };
    let scope = match set.guardrail_scope.as_str() {
        "workarea" => "This workarea only.".to_owned(),
        "ancestors" => "This workarea and its readable parent workareas.".to_owned(),
        other => text(other),
    };
    let mut body = format!(
        r#"<h1>Guardrails</h1><p class="desc">Read-only here; change them with the API or the CLI.</p><p>Rules in force at <code>{}</code>.</p><p><strong>Scope:</strong> {} Each rule names its source workarea.</p>"#,
        text(&set.scope_path),
        scope,
    );
    match tree {
        Ok(workareas) => {
            body.push_str(r#"<form class="row" method="get" action="/web"><input type="hidden" name="view" value="guardrails"><label>Workarea<select name="workarea">"#);
            for node in &workareas {
                let _ = write!(
                    body,
                    r#"<option value="{}"{}>{}</option>"#,
                    attr(&node.id),
                    if node.id == set.scope_node {
                        " selected"
                    } else {
                        ""
                    },
                    text(workarea_label(&node.path))
                );
            }
            body.push_str("</select></label><button type=\"submit\">Show</button></form>");
        }
        Err((_, request_id)) => {
            let _ = write!(
                body,
                r#"<p class="error" role="status">The workarea list is unavailable (request <code>{}</code>).</p>"#,
                text(&request_id)
            );
        }
    }
    body.push_str("<ul>");
    for line in frame::lines(&set) {
        let _ = write!(body, "<li>{}</li>", text(line.trim_start_matches("- ")));
    }
    body.push_str("</ul>");
    outcome::html(
        StatusCode::OK,
        shell(
            "Your page",
            &session.who(),
            &user_tabs(session.admin),
            "/web?view=guardrails",
            fields.get("done"),
            &body,
        ),
    )
}
