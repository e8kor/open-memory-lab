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
use oml_page::web::{shell, text};
use oml_sdk::WorkareaNode;

use crate::picker::{self, Browse, Chosen, Field, Names};
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

    /// Every field, `(name, value)`, in the order sent.
    pub(crate) fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    /// Every field's name, in the order sent.
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(n, _)| n.as_str())
    }

    /// The field's value where it is sent and not empty, as the lead reads
    /// a form.
    pub(crate) fn given(&self, name: &str) -> Option<&str> {
        self.get(name).filter(|v| !v.is_empty())
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
    outcome::html(status, outcome::refusal_page(code, message, calls))
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

/// Why a cursor walk read no whole list.
pub(crate) enum WalkEnd<E> {
    /// A page's call failed.
    Failed(E),
    /// The API named a cursor it had named before, at once or in a cycle.
    Repeated,
    /// The list goes on past the pages the walk reads.
    TooLong,
}

/// Every item of a listing the API pages by cursor, at most `pages_max`
/// pages of it: `page` reads the page at a cursor and answers its items
/// with the next cursor. A cursor the API names twice ends the walk as
/// [`WalkEnd::Repeated`], and a next cursor on the last page read as
/// [`WalkEnd::TooLong`], so no caller is handed a list cut short (#308).
pub(crate) async fn walk<T, E, F, Fut>(pages_max: usize, mut page: F) -> Result<Vec<T>, WalkEnd<E>>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<(Vec<T>, Option<String>), E>>,
{
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..pages_max {
        let (read, next) = page(cursor).await.map_err(WalkEnd::Failed)?;
        items.extend(read);
        match next {
            None => return Ok(items),
            Some(next) if !seen.insert(next.clone()) => return Err(WalkEnd::Repeated),
            Some(next) => cursor = Some(next),
        }
    }
    Err(WalkEnd::TooLong)
}

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
    walk(TREE_PAGES_MAX, |cursor| async move {
        let ids = calls.next_ids();
        let page = web
            .api
            .workarea_tree(&call(credential, &ids), root, TREE_PAGE, cursor.as_deref())
            .await?;
        Ok((page.items, page.next_cursor))
    })
    .await
    .map_err(|end| {
        // The call that ended the listing is the last one made.
        let request_id = calls.made().last().cloned().unwrap_or_default();
        match end {
            WalkEnd::Failed(e) => (request_id, Some(e)),
            WalkEnd::Repeated | WalkEnd::TooLong => (request_id, None),
        }
    })
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
    let user = session.whoami.user.as_ref().map(|u| u.name.clone());
    let credential = session.credential(Capacity::Web);
    // The field's value, checked before any call: a path is refused here,
    // and the set is not read for it.
    let chosen = Chosen::of(fields.get("workarea"));
    let browse_asked = picker::browse_asked(fields.get(picker::BROWSE));
    let set_ids = calls.next_ids();
    let set_call = call(&credential, &set_ids);
    let set = async {
        if chosen.refused() {
            None
        } else {
            Some(web.api.guardrails(&set_call, chosen.value()).await)
        }
    };
    let browse = async {
        if browse_asked {
            Some(Browse::read(web, &credential, calls, &root).await)
        } else {
            None
        }
    };
    let (set, names, browse) = tokio::join!(
        set,
        Names::read(
            web,
            &credential,
            calls,
            &root,
            user.as_deref(),
            picker::HINTS_PAGES
        ),
        browse,
    );
    let (chosen, set) = match set {
        None => (names.settle(chosen), None),
        Some(Ok(set)) => (names.settle(chosen), Some(set)),
        Some(Err(e)) if picker::is_not_found(&e) => match names.not_found(&chosen) {
            Some(Chosen::Place(place)) => {
                let ids = calls.next_ids();
                match web
                    .api
                    .guardrails(&call(&credential, &ids), Some(&place.id))
                    .await
                {
                    Ok(set) => (Chosen::Place(place), Some(set)),
                    Err(e) => return outcome::page(&e, calls, next),
                }
            }
            Some(miss) => (miss, None),
            None => return outcome::page(&e, calls, next),
        },
        Some(Err(e)) => return outcome::page(&e, calls, next),
    };
    // The field names the workarea in force by its reference, the scope the
    // set was read at when the field was empty.
    let chosen = match (&chosen, &set) {
        (Chosen::Every, Some(set)) => names
            .places()
            .iter()
            .find(|p| p.id == set.scope_node)
            .map_or(chosen.clone(), |p| Chosen::Place(p.clone())),
        _ => chosen,
    };
    let field = Field {
        id: "wa",
        action: crate::GUARDRAILS_PATH,
        keep: &[],
        chosen: &chosen,
        names: &names,
        browse: browse.as_ref(),
        admin: false,
        standalone: true,
        empty: "Empty: the user root.",
    }
    .html();
    let mut body = match &set {
        Some(set) => {
            let scope = match set.guardrail_scope.as_str() {
                "workarea" => "This workarea only.".to_owned(),
                "ancestors" => "This workarea and its readable parent workareas.".to_owned(),
                other => text(other),
            };
            format!(
                r#"<h1>Guardrails</h1><p class="desc">Read-only here; change them with the API or the CLI.</p><p>Rules in force at <code>{}</code>.</p><p><strong>Scope:</strong> {} Each rule names its source workarea.</p>"#,
                text(&names.reference(&set.scope_path)),
                scope,
            )
        }
        None => r#"<h1>Guardrails</h1><p class="desc">Read-only here; change them with the API or the CLI.</p>"#.to_owned(),
    };
    body.push_str(&field);
    let Some(set) = set else {
        return outcome::html(
            chosen.status(),
            shell("", &session.personal(), crate::GUARDRAILS_PATH, None, &body),
        );
    };
    // Each rule's source by its reference, the name the field takes
    // (design notes row 20); the lines are otherwise the frame's.
    let mut set = set;
    for g in &mut set.guardrails {
        if let Some(path) = &g.workarea_path {
            g.workarea_path = Some(names.reference(path));
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
            "",
            &session.personal(),
            crate::GUARDRAILS_PATH,
            fields.get("done"),
            &body,
        ),
    )
}
