//! The pages `oml-web` serves (ADR-0070 d7 (b)), each moved from the lead
//! to a path of its own under `/web/`, with its assertions unchanged but
//! for the path (`tests/web_pages.rs`). A page is drawn from the wire's
//! fields only.
//!
//! What the pages share: a query's or a form's fields, the refusal page,
//! and the walk over a listing the API pages by cursor.

use axum::extract::Query;
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_sdk::WorkspaceNode;

use crate::session::{PageCalls, call};
use crate::{Web, outcome};

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

/// The most tree pages the selector reads: 50 pages of 200, so 10,000
/// workspaces. A constant of the build, unmeasured (#170).
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

/// Every workspace under `root`, a page of the tree at a time, or the
/// request id of the call that ended the listing, with the API's error
/// where a call failed: a failed call, a cursor the API repeats, or a
/// listing past [`TREE_PAGES_MAX`] pages.
pub(crate) async fn workspaces(
    web: &Web,
    credential: &str,
    calls: &PageCalls,
    root: &str,
) -> Result<Vec<WorkspaceNode>, (String, Option<oml_sdk::Error>)> {
    walk(TREE_PAGES_MAX, |cursor| async move {
        let ids = calls.next_ids();
        let page = web
            .api
            .workspace_tree(&call(credential, &ids), root, TREE_PAGE, cursor.as_deref())
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
