//! Where the user page's and the admin page's views and acts went
//! (ADR-0070 d7 (b)): every view and every act of `GET`/`POST /web` and
//! `/web/admin` is `oml-web`'s at a path of its own, but sign-out, and the
//! lead answers each old URL and act with a `303` there, running nothing.

use uuid::Uuid;

use oml_page::web::{
    ADMIN_ATLAS_PATH, ADMIN_CURATE_PATH, ADMIN_KEYS_PATH, ADMIN_MEMORIES_PATH, ADMIN_MEMORY_PATH,
    ADMIN_REVIEW_PATH, ADMIN_WORKAREAS_PATH, USER_ATLAS_PATH, USER_KEYS_PATH, USER_MEMORIES_PATH,
    USER_MEMORY_PATH, USER_REVIEW_PATH, USER_WORKAREAS_PATH,
};

/// The setup view, which `oml-web` serves.
pub const SETUP_PATH: &str = "/web/setup";

/// The admin overview, the allowlist and the capacities, which `oml-web`
/// serves; the lead's `GET /web/admin` answers a `303` here.
pub const PEOPLE_PATH: &str = "/web/admin/people";

/// The user page's views `oml-web` serves, each at a path of its own
/// (ADR-0070 d7 (b) as amended): the lead answers `GET /web?view=<view>`
/// with a `303` to that fixed path, and no longer renders the view.
pub const MOVED_VIEWS: &[(&str, &str)] = &[
    ("guardrails", "/web/guardrails"),
    ("setup", SETUP_PATH),
    ("workarea", USER_WORKAREAS_PATH),
    ("review", USER_REVIEW_PATH),
    ("keys", USER_KEYS_PATH),
];

/// The admin page's views `oml-web` serves, each at a path of its own: the
/// lead answers `GET /web/admin?view=<view>` with a `303` there.
pub const MOVED_ADMIN_VIEWS: &[(&str, &str)] = &[
    ("workarea", ADMIN_WORKAREAS_PATH),
    ("review", ADMIN_REVIEW_PATH),
    ("held", ADMIN_REVIEW_PATH),
    ("queue", ADMIN_REVIEW_PATH),
    ("curate", ADMIN_CURATE_PATH),
    ("people", PEOPLE_PATH),
];

/// The path of the view a moved act belongs to, on the user page or the
/// admin page; `None` for an act the lead still runs. The lead runs none of
/// the moved acts and answers each with a `303` there (ADR-0070 d7 (b)).
/// Bulk review is the admin page's only (ADR-0055 d1), so on the user page
/// its acts are no act of the page.
#[must_use]
pub fn moved_action(action: &str, admin: bool) -> Option<&'static str> {
    Some(match (action, admin) {
        (
            "workarea_mode" | "workarea_transition" | "workarea_ratify" | "create_workarea",
            false,
        ) => USER_WORKAREAS_PATH,
        ("workarea_mode" | "workarea_transition" | "workarea_ratify" | "create_workarea", true) => {
            ADMIN_WORKAREAS_PATH
        }
        ("review_next" | "verdict", false) => USER_REVIEW_PATH,
        ("review_next" | "verdict" | "bulk_confirm" | "bulk_verdict", true) => ADMIN_REVIEW_PATH,
        ("issue_key" | "connect_key" | "connect_code" | "cut_off", false) => USER_KEYS_PATH,
        ("cut_off", true) => ADMIN_KEYS_PATH,
        ("curate", true) => ADMIN_CURATE_PATH,
        ("supersede", true) => ADMIN_MEMORIES_PATH,
        (
            "allow_email" | "remove_email" | "set_admin" | "grant_review" | "grant_curation",
            true,
        ) => PEOPLE_PATH,
        _ => return None,
    })
}

/// Where the Memories list, the Atlas or one memory lives, for a query
/// whose one `view` is `memories` (ADR-0070 d7 (b)): one memory opened by
/// its id at its own page, carrying the id alone, parsed and written anew,
/// so nothing else of the request travels; else the Atlas where `display`
/// is `atlas`, else the list. A `memory` that is not one id answers `None`,
/// and the lead refuses the view.
#[must_use]
pub fn moved_memories(query: &str, admin: bool) -> Option<String> {
    let pairs = crate::oauth::form_pairs(query.as_bytes()).ok()?;
    let one = |name: &str| {
        let mut values = pairs.iter().filter(|(n, _)| n == name).map(|(_, v)| v);
        match (values.next(), values.next()) {
            (Some(v), None) => Some(v.as_str()),
            _ => None,
        }
    };
    if one("view") != Some("memories") {
        return None;
    }
    if pairs.iter().any(|(n, v)| n == "memory" && !v.is_empty()) {
        let id = Uuid::parse_str(one("memory")?).ok()?;
        let page = if admin {
            ADMIN_MEMORY_PATH
        } else {
            USER_MEMORY_PATH
        };
        return Some(format!("{page}?id={}", id.hyphenated()));
    }
    let atlas = pairs.iter().any(|(n, v)| n == "display" && v == "atlas");
    Some(
        match (admin, atlas) {
            (false, false) => USER_MEMORIES_PATH,
            (false, true) => USER_ATLAS_PATH,
            (true, false) => ADMIN_MEMORIES_PATH,
            (true, true) => ADMIN_ATLAS_PATH,
        }
        .to_owned(),
    )
}

/// The fixed path a moved view lives at, for a query whose one `view` names
/// one of `moved`; `None` otherwise.
#[must_use]
pub fn moved_view_of(moved: &[(&str, &'static str)], query: &str) -> Option<&'static str> {
    let pairs = crate::oauth::form_pairs(query.as_bytes()).ok()?;
    let mut views = pairs.iter().filter(|(n, _)| n == "view");
    let (_, view) = views.next()?;
    if views.next().is_some() {
        return None;
    }
    moved.iter().find(|(v, _)| v == view).map(|(_, path)| *path)
}

/// [`moved_view_of`] the user page's [`MOVED_VIEWS`].
#[must_use]
pub fn moved_view(query: &str) -> Option<&'static str> {
    moved_view_of(MOVED_VIEWS, query)
}
