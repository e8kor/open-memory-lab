//! The import pages, `/web/workspaces/import` (ADR-0096 d6; ADR-0084 d15):
//! the hand-off to the command line, which runs an import, then the list
//! of every import the person's keys ran (`?list=1`) and one import
//! (`?id=`), read as `<user>-web`.
//!
//! The hand-off states the built rule: an import goes into a development
//! workspace in progress that the importing key writes, or a new name it
//! creates in progress; any other target is `import.target_not_in_progress`.
//! The first import from a source needs `--expect-key`, the source key's
//! fingerprint of 64 hex digits, and pins it. Each page makes `whoami`
//! and one read: the tree, the listing or the import.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::State;
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{USER_WORKSPACES_PATH, attr, shell, short, text};
use oml_sdk::{Import, encode};

use crate::keys::instant;
use crate::pages::{Fields, refused};
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, acts, outcome};

/// The import pages' path.
pub const IMPORT_PATH: &str = "/web/workspaces/import";

/// Whether `fp` is a source key's fingerprint: 64 hex digits.
pub(crate) fn is_fingerprint(fp: &str) -> bool {
    fp.len() == 64 && fp.bytes().all(|c| c.is_ascii_hexdigit())
}

/// The `oml workspaces import` line the hand-off shows.
pub(crate) fn command(into: &str, expect_key: Option<&str>) -> String {
    let mut line = format!(
        "oml workspaces import --into {} --identity <age identity file>",
        into
    );
    if let Some(fp) = expect_key {
        let _ = write!(line, " --expect-key {fp}");
    }
    line.push_str(" <bundle file>");
    line
}

/// A job state as a quiet chip: no glyph for running.
fn state_chip(state: &str) -> String {
    let glyph = match state {
        "done" => " g-can",
        "failed" => " g-blocked",
        _ => "",
    };
    format!(
        r#"<span class="oml-chip is-quiet{glyph}">{}</span>"#,
        text(state)
    )
}

fn counts(import: &Import) -> String {
    if import.counts.is_empty() {
        return "none yet".to_owned();
    }
    import
        .counts
        .iter()
        .map(|(outcome, n)| format!("{} {n}", text(outcome)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `GET /web/workspaces/import`.
pub async fn page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let Some(user) = session.whoami.user.as_ref().map(|u| u.name.clone()) else {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        );
    };
    let credential = session.credential(Capacity::Web);
    let draw = |title: &str, body: &str| {
        outcome::html(
            StatusCode::OK,
            shell(title, &session.personal(), USER_WORKSPACES_PATH, None, body),
        )
    };
    if let Some(id) = fields.given("id") {
        let ids = calls.next_ids();
        let import = match web.api.get_import(&call(&credential, &ids), id).await {
            Ok(import) => import,
            Err(e) => return outcome::page(&e, &calls, IMPORT_PATH),
        };
        return draw("Import", &one(&import));
    }
    if fields.given("list").is_some() {
        let ids = calls.next_ids();
        let page = match web
            .api
            .list_imports(
                &call(&credential, &ids),
                fields.given("state"),
                fields.given("cursor"),
            )
            .await
        {
            Ok(page) => page,
            Err(e) => return outcome::page(&e, &calls, IMPORT_PATH),
        };
        return draw("Imports", &list(&page));
    }
    draw("Import", &hand_off(&user, &fields))
}

/// P11: the hand-off, then with `into` its command line.
pub(crate) fn hand_off(user: &str, fields: &Fields) -> String {
    let into = fields.given("into").unwrap_or("");
    let expect = fields.given("expect_key").unwrap_or("");
    let mut out = format!(
        r#"<header class="oml-section-head"><div class="top"><h1>Import</h1></div><p class="desc">An import runs from the command line under one of this account's keys; its progress is on <a href="{IMPORT_PATH}?list=1">Imports</a>.</p></header><p class="desc">An import goes into a development workspace in progress that the key writes, or a new name it creates in progress; any other target is refused <code>import.target_not_in_progress</code>. The first import from a source needs the source key's fingerprint, 64 hex digits, and pins it.</p><form class="oml-form" method="get" action="{IMPORT_PATH}"><label>Into<span class="oml-prefix"><span class="p">{}.</span><input class="oml-input" name="into" value="{}" required></span></label><label>Source key fingerprint (first import)<input class="oml-input" name="expect_key" value="{}"></label><button type="submit">Show the command</button></form>"#,
        short(user),
        attr(into.strip_prefix(&format!("{user}.")).unwrap_or(into)),
        attr(expect),
    );
    if into.is_empty() {
        return out;
    }
    let target = if into.starts_with(&format!("{user}.")) {
        into.to_owned()
    } else {
        format!("{user}.{into}")
    };
    if !expect.is_empty() && !is_fingerprint(expect) {
        out.push_str(r#"<div class="oml-notice err" role="alert"><p>A fingerprint is 64 hex digits. <code>request.validation</code></p></div>"#);
        return out;
    }
    let _ = write!(
        out,
        r#"<h2>The command</h2><pre>{}</pre>{}"#,
        text(&command(&target, (!expect.is_empty()).then_some(expect))),
        if expect.is_empty() {
            r#"<p class="desc">Without a fingerprint, a first import from a source is refused (<code>/expect_key</code>, <code>first_import</code>); a pinned source needs none.</p>"#
        } else {
            ""
        }
    );
    out
}

/// P12: the imports of every key of the person's, newest first.
fn list(page: &oml_sdk::ImportPage) -> String {
    let mut out = format!(
        r#"<header class="oml-section-head"><div class="top"><h1>Imports</h1></div><p class="desc">Imports any key of this account ran, newest first. <a href="{IMPORT_PATH}">Import</a></p></header>"#
    );
    if page.items.is_empty() {
        out.push_str(r#"<div class="oml-empty"><p>No import yet.</p><p class="desc">An import run from the command line under one of this account's keys appears here.</p></div>"#);
        return out;
    }
    out.push_str(r#"<table class="oml-table"><tr><th>Import</th><th>Key</th><th>State</th><th>Versions recorded</th><th>Counts</th><th>Started</th></tr>"#);
    for i in &page.items {
        let key = i
            .key
            .as_ref()
            .map(|k| k.name.clone().unwrap_or_else(|| k.id.clone()))
            .unwrap_or_default();
        let _ = write!(
            out,
            r#"<tr><td class="mono"><a href="{IMPORT_PATH}?id={}">{}</a></td><td class="mono">{}</td><td>{}</td><td>{}</td><td class="c-meta wrap prose">{}</td><td>{}</td></tr>"#,
            attr(&encode(&i.import_id)),
            short(&i.import_id[..8.min(i.import_id.len())]),
            short(&key),
            state_chip(&i.state),
            i.position,
            counts(i),
            i.created_at.as_deref().map(instant).unwrap_or_default()
        );
    }
    out.push_str("</table>");
    if let Some(next) = &page.next_cursor {
        let _ = write!(
            out,
            r#"<p class="pager"><a href="{IMPORT_PATH}?list=1&amp;cursor={}">Older imports</a></p>"#,
            attr(&encode(next))
        );
    }
    out
}

/// P12: one import, its position, counts by outcome and refusals by code.
pub(crate) fn one(import: &Import) -> String {
    let mut out = format!(
        r#"<header class="oml-section-head"><div class="top"><h1 class="mono">{}</h1></div><p class="desc"><a href="{IMPORT_PATH}?list=1">Imports</a></p></header><table class="oml-table"><tbody><tr><th scope="row">State</th><td>{}</td></tr><tr><th scope="row">Into</th><td class="mono">{}</td></tr><tr><th scope="row">Versions recorded</th><td>{}</td></tr><tr><th scope="row">Counts</th><td>{}</td></tr></tbody></table>"#,
        short(&import.import_id),
        state_chip(&import.state),
        short(&import.target_workspace_id),
        import.position,
        counts(import)
    );
    if import.state == "failed" {
        out.push_str(r#"<p class="desc">A failed import stopped mid-run; its cause is not on the record. The versions recorded stand.</p>"#);
    }
    if !import.refusals.is_empty() {
        out.push_str("<h2>Refusals</h2><ul>");
        for r in &import.refusals {
            let code = r.get("code").and_then(|c| c.as_str()).unwrap_or("refused");
            let _ = write!(out, "<li><code>{}</code></li>", text(code));
        }
        out.push_str("</ul>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hand_off_shows_the_command_with_the_fingerprint_and_refuses_one_of_another_shape() {
        let fields = |q: &str| Fields::of(&format!("{IMPORT_PATH}?{q}").parse().unwrap()).unwrap();
        let fp = "a".repeat(64);
        let page = hand_off("alice", &fields(&format!("into=docs&expect_key={fp}")));
        assert!(
            page.contains(&format!(
                "oml workspaces import --into alice.docs --identity &lt;age identity file&gt; --expect-key {fp} &lt;bundle file&gt;"
            )),
            "{page}"
        );
        let short_fp = hand_off("alice", &fields("into=docs&expect_key=abc"));
        assert!(short_fp.contains("64 hex digits"), "{short_fp}");
        assert!(!short_fp.contains("<h2>The command</h2>"), "{short_fp}");
        let first = hand_off("alice", &fields("into=alice.docs"));
        assert!(first.contains("first_import"), "{first}");
    }

    #[test]
    fn a_failed_import_says_its_cause_is_not_on_the_record() {
        let import: Import = serde_json::from_value(serde_json::json!({
            "import_id": "11111111-2222-4333-8444-555555555555",
            "state": "failed",
            "target_workspace_id": "w",
            "position": 312,
            "counts": {"landed": 300},
            "refusals": [{"code": "import.over_quota"}],
        }))
        .unwrap();
        let page = one(&import);
        assert!(page.contains("not on the record"), "{page}");
        assert!(page.contains("<code>import.over_quota</code>"), "{page}");
        assert!(page.contains("is-quiet g-blocked"), "{page}");
    }
}
