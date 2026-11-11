//! The pages `oml-web` serves (ADR-0070 d7 (b)), each moved from the lead
//! to a path of its own under `/web/`, with its assertions unchanged but
//! for the path (`tests/web_pages.rs`). A page is drawn from the wire's
//! fields only.
//!
//! Moved so far: the user page's Guardrails view, `GET /web/guardrails`,
//! formerly `GET /web?view=guardrails` (ADR-0037 d17), first because it only
//! reads and maps onto `GET /v1/guardrails` (design/web-client-separation.md).

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::{Query, State};
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{attr, shell, text, user_tabs, workarea_label};
use oml_sdk::WorkareaNode;

use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, frame, outcome};

/// The tree's page size the selector reads, the API's maximum.
const TREE_PAGE: u32 = 200;

/// A query's or a form's fields, each at most once, as the lead reads a
/// page's query and form.
pub(crate) struct Fields(Vec<(String, String)>);

impl Fields {
    pub(crate) fn of(uri: &Uri) -> Result<Self, String> {
        let Query(pairs) = Query::<Vec<(String, String)>>::try_from_uri(uri)
            .map_err(|e| format!("the query is not a page's: {e}"))?;
        Self::of_pairs(pairs)
    }

    /// The fields of `pairs`, refusing a name sent more than once.
    pub(crate) fn of_pairs(pairs: Vec<(String, String)>) -> Result<Self, String> {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in &pairs {
            if !seen.insert(name.as_str()) {
                return Err(format!("the field {name:?} is sent more than once"));
            }
        }
        Ok(Self(pairs))
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The first field not in `allowed`, which an act refuses (ADR-0045
    /// d6).
    pub(crate) fn outside(&self, allowed: &[&str]) -> Option<&str> {
        self.0
            .iter()
            .map(|(n, _)| n.as_str())
            .find(|n| !allowed.contains(n))
    }
}

pub(crate) fn refused(
    status: StatusCode,
    code: &str,
    message: &str,
    calls: &PageCalls,
) -> Response {
    outcome::html(
        status,
        oml_page::web::refusal_page(code, message, Some(calls.page_id())),
    )
}

/// `GET /web/guardrails`: the user page's Guardrails view.
pub async fn guardrails_page(
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
    let next = uri
        .path_and_query()
        .map_or(crate::GUARDRAILS_PATH, |pq| pq.as_str());
    guardrails(&web, &session, &calls, &fields, next).await
}

/// The most tree pages the selector reads: 50 pages of 200, so 10,000
/// workareas. A constant of the build, unmeasured (#170).
pub const TREE_PAGES_MAX: usize = 50;

/// Every workarea under `root`, a page of the tree at a time, or the
/// request id of the call that ended the listing, with the API's error
/// where a call failed: a failed call, a cursor the API repeats, or a
/// listing past [`TREE_PAGES_MAX`] pages.
pub(crate) async fn workareas(
    web: &Web,
    credential: &str,
    calls: &PageCalls,
    root: &str,
) -> Result<Vec<WorkareaNode>, (String, Option<oml_sdk::Error>)> {
    let mut nodes = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..TREE_PAGES_MAX {
        let ids = calls.next_ids();
        let request_id = ids.request_id.clone().unwrap_or_default();
        let page = web
            .api
            .workarea_tree(&call(credential, &ids), root, TREE_PAGE, cursor.as_deref())
            .await
            .map_err(|e| (request_id.clone(), Some(e)))?;
        nodes.extend(page.items);
        match page.next_cursor {
            None => return Ok(nodes),
            Some(next) if !seen.insert(next.clone()) => return Err((request_id, None)),
            Some(next) => cursor = Some(next),
        }
    }
    Err((calls.made().last().cloned().unwrap_or_default(), None))
}

/// The guardrails in force at the selected workarea, read-only: the scope,
/// each rule with its source path, and a selector over the person's
/// workareas. The page writes, supersedes and retires none (ADR-0037 d17).
/// The set is the page's primary call and the tree its secondary one, made
/// together after the session's check (ADR-0070 d8): a failed set is the
/// error page, a failed tree the page without its selector.
async fn guardrails(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    fields: &Fields,
    next: &str,
) -> Response {
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
        Err(e) => return outcome::page(&e, calls.page_id(), next),
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
            body.push_str(r#"<form class="row" method="get" action="/web/guardrails"><label>Workarea<select name="workarea">"#);
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
        Err((request_id, _)) => {
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
            crate::GUARDRAILS_PATH,
            fields.get("done"),
            &body,
        ),
    )
}
