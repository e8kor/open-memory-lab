//! The Review view of the user page and of the admin page (ADR-0045 d11;
//! ADR-0055 d10), moved from the lead to paths of their own, `/web/review`
//! and `/web/admin/review` (ADR-0070 d7 (b)), with its assertions unchanged
//! but for the path (`tests/web_pages.rs`).
//!
//! Every act is the person's reviewer key's, `<user>-review`, on both
//! pages: the admin key never judges (ADR-0045 d11), and the API refuses a
//! verdict from a key in the item's lineage, which the page shows by name
//! (`review.author_in_lineage`). The view holds:
//!
//! - on the admin page, the tenant's queue: held rows per workarea, read
//!   as the admin key (`reviewQueue`);
//! - review next: `POST` claims the next item (`reviewNext`) and redirects
//!   to the view with its serve id and the served memory's id; the view
//!   draws the item's card from the held listing by that memory id
//!   (`reviewHeld?ids=`, which claims nothing) with the verdict form, and
//!   never reads the serve id, which the contract leaves opaque; the
//!   verdict (`postVerdict`) redirects to the next item;
//! - on the admin page only, bulk review (ADR-0055 d1): the held listing
//!   with its filters and a box per row;
//!   picking posts `bulk_confirm`, which records nothing and shows each
//!   picked row by its claim key and first line with the one form that
//!   records a verdict on each (`postBulkVerdict`) and redirects to the
//!   view with the counts. A resubmitted confirmation replays.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{
    ADMIN_REVIEW_PATH, ADMIN_TABS, USER_REVIEW_PATH, attr, form, hidden, input, select, shell,
    short, text, user_tabs,
};
use oml_sdk::encode;

use crate::acts::Refusal;
use crate::card::{anchor_line, card, findings_cell, valid_line};
use crate::pages::{Fields, refused};
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, acts, outcome};

/// The held listing's page size (ADR-0055 d10).
pub const HELD_PAGE: u32 = 50;

/// The most memories one bulk verdict names (ADR-0055 d3).
pub const BULK_MAX: usize = 200;

/// The user page's acts: review next and the verdict.
pub const USER_ACTIONS: &[&str] = &["review_next", "verdict"];

/// The admin page's acts: the user page's and bulk review (ADR-0055 d1).
pub const ADMIN_ACTIONS: &[&str] = &["review_next", "verdict", "bulk_confirm", "bulk_verdict"];

/// Which page the view is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    User,
    Admin,
}

impl Side {
    fn path(self) -> &'static str {
        match self {
            Self::User => USER_REVIEW_PATH,
            Self::Admin => ADMIN_REVIEW_PATH,
        }
    }
}

/// Whether `value` is an id as the API writes one.
pub(crate) fn is_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// A count named in a notice's query, read as a number only.
fn count(fields: &Fields, name: &str) -> u64 {
    fields
        .get(name)
        .and_then(|v| v.parse().ok())
        .unwrap_or_default()
}

/// The notice the view's own acts name, built from numbers and a closed
/// set of words only, so a crafted link cannot put text on the page.
fn notice(fields: &Fields) -> Option<String> {
    match fields.get("done")? {
        "verdict" => {
            let state = match fields.get("state")? {
                s @ ("promoted" | "held" | "rejected") => s,
                _ => return None,
            };
            Some(format!(
                "Recorded: version {} is now {state}.",
                count(fields, "version")
            ))
        }
        "empty" => Some("Nothing waits for you.".to_owned()),
        "bulk" => {
            let mut line = format!(
                "Recorded: {} approved, {} rejected, {} skipped.",
                count(fields, "approved"),
                count(fields, "rejected"),
                count(fields, "skipped")
            );
            let same = count(fields, "same_user");
            if same > 0 {
                let _ = write!(line, " {same} by the same user.");
            }
            let replayed = count(fields, "replayed");
            if replayed > 0 {
                let _ = write!(line, " {replayed} replayed: already recorded.");
            }
            Some(line)
        }
        _ => None,
    }
}

/// What the view's reads share.
struct ViewCtx<'a> {
    web: &'a Web,
    session: &'a Session,
    calls: &'a PageCalls,
    side: Side,
}

/// The served item's card and its verdict form: the card read by the
/// memory id the redirect named from the held listing, which claims
/// nothing, and the form carrying the serve id as the API gave it.
async fn served(
    view: &ViewCtx<'_>,
    serve_id: &str,
    memory: Option<&str>,
) -> Result<(String, bool), Refusal> {
    let Some(memory_id) = memory.filter(|m| is_id(m)) else {
        return Err(Box::new(acts::invalid(
            "memory is the served memory's id",
            view.calls,
        )));
    };
    let credential = view.session.credential(Capacity::Review);
    let ids = view.calls.next_ids();
    let named = view
        .web
        .api
        .review_named(&call(&credential, &ids), &[memory_id])
        .await
        .map_err(|e| Box::new(outcome::page(&e, view.calls.page_id(), view.side.path())))?;
    let Some(item) = named.items.into_iter().find_map(|n| n.item) else {
        return Ok((
            r#"<p class="desc">That memory is no longer held, or not yours to judge.</p>"#
                .to_owned(),
            false,
        ));
    };
    // A held diagram is drawn for the reviewer from the source the API
    // served them here, bound to their session (ADR-0056 d9; #189).
    let drawing = crate::render::drawing(
        view.web.layers.public_origin.as_deref(),
        item.format.as_deref(),
        &view.session.csrf(),
        &item.artifact_id,
        item.text.as_deref().unwrap_or_default(),
    );
    let mut out = card(&item, drawing.as_deref().unwrap_or_default());
    out.push_str(&form(
        view.side.path(),
        "verdict",
        &view.session.csrf(),
        &format!(
            "{}{}{}{}",
            hidden("serve_id", serve_id),
            select(
                "verdict",
                "verdict",
                &[("approve", "approve"), ("reject", "reject")]
            ),
            input("reason", "reason", "", false),
            acts::nonce_field()
        ),
        "Record verdict",
        true,
    ));
    Ok((out, drawing.is_some()))
}

/// The tenant's queue, as the admin key reads it.
async fn queue(view: &ViewCtx<'_>) -> Result<String, Refusal> {
    let credential = view.session.credential(Capacity::Admin);
    let ids = view.calls.next_ids();
    let rows = view
        .web
        .api
        .review_queue(&call(&credential, &ids))
        .await
        .map_err(|e| Box::new(outcome::page(&e, view.calls.page_id(), view.side.path())))?;
    let mut body = String::from(
        r#"<h2>Queue</h2><p class="desc">Held memories per workarea.</p><table><tr><th>Workarea</th><th>Held</th><th>Oldest since</th></tr>"#,
    );
    if rows.items.is_empty() {
        body.push_str(r#"<tr><td colspan="3" class="dim">Nothing is held.</td></tr>"#);
    }
    for row in &rows.items {
        let _ = write!(
            body,
            r#"<tr><td class="mono">{}</td><td>{}</td><td class="mono">{}</td></tr>"#,
            short(&row.path),
            row.held,
            short(&row.oldest_at)
        );
    }
    body.push_str("</table>");
    Ok(body)
}

/// The bulk listing: the rows the reviewer key may judge, filtered, each
/// with its text and a box to pick it; "select all" draws the page with
/// every box checked, since the page carries no script.
async fn held(view: &ViewCtx<'_>, fields: &Fields) -> Result<String, Refusal> {
    let page = view.side.path();
    let filters: Vec<(&str, &str)> = ["workarea", "author_key", "claim_key_prefix"]
        .into_iter()
        .filter_map(|name| fields.given(name).map(|v| (name, v)))
        .collect();
    let credential = view.session.credential(Capacity::Review);
    let ids = view.calls.next_ids();
    let listing = view
        .web
        .api
        .review_held(
            &call(&credential, &ids),
            &filters,
            HELD_PAGE,
            fields.given("cursor"),
        )
        .await
        .map_err(|e| Box::new(outcome::page(&e, view.calls.page_id(), page)))?;
    let all = fields.get("select") == Some("all");
    let value = |name: &str| fields.given(name).unwrap_or("");
    let kept: String = filters
        .iter()
        .map(|(k, v)| format!("&amp;{k}={}", attr(&encode(v))))
        .collect();
    let cursor = fields.given("cursor").map_or(String::new(), |c| {
        format!("&amp;cursor={}", attr(&encode(c)))
    });
    let mut body = format!(
        r#"<h2>Bulk review</h2><p class="desc">Oldest first, {HELD_PAGE} a page. Each pick gets its own verdict; you confirm first. A memory your reviewer key wrote, or that derives from one, is never listed.</p><form class="row" method="get" action="{page}">{}{}{}<button type="submit">Filter</button></form><p><a href="{page}?held=1{kept}{cursor}&amp;select=all">Select all on this page</a> · <a href="{page}?held=1{kept}{cursor}">Clear</a></p>"#,
        input("workarea", "workarea", value("workarea"), false),
        input("author_key", "author key", value("author_key"), false),
        input(
            "claim_key_prefix",
            "claim key prefix",
            value("claim_key_prefix"),
            false
        ),
    );
    let mut rows = String::from(
        "<table><tr><th></th><th>Claim key</th><th>Workarea</th><th>From</th><th>Held since</th><th>Findings</th><th>Text</th><th>Source</th><th>Valid</th></tr>",
    );
    if listing.items.is_empty() {
        rows.push_str(r#"<tr><td colspan="9" class="dim">Nothing waits for you here.</td></tr>"#);
    }
    for (i, item) in listing.items.iter().enumerate() {
        let m = item.memory();
        let _ = write!(
            rows,
            r#"<tr><td><input type="checkbox" name="pick_{i:03}" value="{}"{}></td><td class="mono">{}</td><td class="mono">{}</td><td class="mono">{}{}</td><td class="mono">{}</td><td class="mono findings">{}</td><td class="text">{}</td><td class="mono">{}</td><td class="mono">{}</td></tr>"#,
            attr(&m.artifact_id),
            if all { " checked" } else { "" },
            short(&m.claim_key),
            short(m.workarea_path.as_deref().unwrap_or("")),
            short(m.principal.as_deref().unwrap_or("")),
            if item.same_user { " (you)" } else { "" },
            short(&item.held_since),
            findings_cell(m),
            text(m.text.as_deref().unwrap_or_default()),
            anchor_line(m.anchor.as_ref()),
            valid_line(m)
        );
    }
    rows.push_str("</table>");
    body.push_str(&form(
        page,
        "bulk_confirm",
        &view.session.csrf(),
        &format!(
            "{rows}{}{}",
            select(
                "verdict",
                "verdict",
                &[("approve", "approve"), ("reject", "reject")]
            ),
            input("reason", "reason (required to reject)", "", false)
        ),
        "Review picks",
        true,
    ));
    if let Some(next) = &listing.next_cursor {
        let _ = write!(
            body,
            r#"<p><a href="{page}?held=1{kept}&amp;cursor={}">Next page</a></p>"#,
            attr(&encode(next))
        );
    }
    Ok(body)
}

fn tabs(session: &Session, side: Side) -> Vec<(&'static str, &'static str)> {
    match side {
        Side::User => user_tabs(session.admin),
        Side::Admin => ADMIN_TABS.to_vec(),
    }
}

fn title(side: Side) -> &'static str {
    match side {
        Side::User => "Your page",
        Side::Admin => "Administration",
    }
}

async fn view(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    uri: &Uri,
    side: Side,
) -> Response {
    let fields = match Fields::of(uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let page = side.path();
    let ctx = ViewCtx {
        web: &web,
        session: &session,
        calls: &calls,
        side,
    };
    let mut body = String::from(
        r#"<p class="label">Review</p><h1>Review</h1><p class="desc">Your own writes are never served to you.</p>"#,
    );
    if side == Side::Admin {
        match queue(&ctx).await {
            Ok(queue) => body.push_str(&queue),
            Err(response) => return *response,
        }
    }
    let mut framed = false;
    if let Some(serve_id) = fields.given("serve") {
        match served(&ctx, serve_id, fields.given("memory")).await {
            Ok((card, drawn)) => {
                body.push_str(&card);
                framed = drawn;
            }
            Err(response) => return *response,
        }
    }
    body.push_str(&form(
        page,
        "review_next",
        &session.csrf(),
        &acts::nonce_field(),
        "Review next",
        true,
    ));
    if side == Side::Admin {
        match held(&ctx, &fields).await {
            Ok(listing) => body.push_str(&listing),
            Err(response) => return *response,
        }
    }
    let mut response = outcome::html(
        StatusCode::OK,
        shell(
            title(side),
            &session.who(),
            Some(&session.csrf()),
            &tabs(&session, side),
            page,
            notice(&fields).as_deref(),
            &body,
        ),
    );
    if framed {
        crate::render::allow_frames(&mut response, web.layers.public_origin.as_deref());
    }
    response
}

/// Any session but an admin entry's answers `access.not_found` on the
/// admin page (ADR-0045 d10).
fn admin_only(session: &Session, calls: &PageCalls) -> Option<Response> {
    (!session.admin).then(|| {
        refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            calls,
        )
    })
}

/// `GET /web/review`: the user page's Review view.
pub async fn user_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    view(web, session, calls, &uri, Side::User).await
}

/// `GET /web/admin/review`: the admin page's Review view, the queue above.
pub async fn admin_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    view(web, session, calls, &uri, Side::Admin).await
}

/// Whether `name` is a listing box, `pick_000` to `pick_199`.
fn is_pick(name: &str) -> bool {
    name.strip_prefix("pick_")
        .is_some_and(|n| n.len() == 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The fields each action defines, beside `action`, `csrf_token` and
/// `nonce`.
fn defined(action: &str, name: &str) -> bool {
    match action {
        "verdict" => matches!(name, "serve_id" | "verdict" | "reason"),
        "bulk_confirm" => matches!(name, "verdict" | "reason") || is_pick(name),
        "bulk_verdict" => matches!(name, "verdict" | "reason" | "artifact_ids"),
        _ => false,
    }
}

/// `verdict`, as the form names it.
fn verdict_of(fields: &Fields, calls: &PageCalls) -> Result<&'static str, Refusal> {
    match fields.given("verdict") {
        Some("approve") => Ok("approve"),
        Some("reject") => Ok("reject"),
        Some(_) => Err(Box::new(acts::invalid(
            "verdict is approve or reject",
            calls,
        ))),
        None => Err(Box::new(acts::invalid("verdict is required", calls))),
    }
}

/// `{page}?serve=…&memory=…` for a served item: the serve id as the API
/// gave it, and the served memory's id, which the view reads the card by.
fn served_location(page: &str, serve: &oml_sdk::Serve) -> String {
    format!(
        "{page}?serve={}&memory={}",
        encode(&serve.serve_id),
        encode(&serve.item.artifact_id)
    )
}

/// One act of the view, as the person's reviewer key.
async fn act(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
    side: Side,
) -> Response {
    let page = side.path();
    let actions = match side {
        Side::User => USER_ACTIONS,
        Side::Admin => ADMIN_ACTIONS,
    };
    let (fields, action) = match acts::read(form, actions, defined, &calls) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let credential = session.credential(Capacity::Review);
    match action {
        "review_next" => {
            let key = match acts::key(&session, &fields, &calls) {
                Ok(key) => key,
                Err(refusal) => return *refusal,
            };
            let _in_flight = acts::hold(&key).await;
            let ids = calls.next_ids();
            match web.api.review_next(&call(&credential, &ids), &key).await {
                Ok(Some(serve)) => acts::see_other(&served_location(page, &serve), &[]),
                Ok(None) => acts::see_other(&format!("{page}?done=empty"), &[]),
                Err(e) => outcome::page(&e, calls.page_id(), page),
            }
        }
        "verdict" => {
            let verdict = match verdict_of(&fields, &calls) {
                Ok(v) => v,
                Err(refusal) => return *refusal,
            };
            let Some(serve_id) = fields.given("serve_id") else {
                return acts::invalid("serve_id is required", &calls);
            };
            let key = match acts::key(&session, &fields, &calls) {
                Ok(key) => key,
                Err(refusal) => return *refusal,
            };
            let _in_flight = acts::hold(&key).await;
            let ids = calls.next_ids();
            let recorded = match web
                .api
                .post_verdict(
                    &call(&credential, &ids),
                    &key,
                    serve_id,
                    verdict,
                    fields.given("reason"),
                )
                .await
            {
                Ok(recorded) => recorded,
                Err(e) => return outcome::page(&e, calls.page_id(), page),
            };
            let state = match recorded.state.as_str() {
                s @ ("promoted" | "held" | "rejected") => s,
                _ => "held",
            };
            let done = format!("done=verdict&version={}&state={state}", recorded.version);
            // The next claim runs under a key derived from the verdict's, so
            // a resubmitted verdict replays its claim too; a claim that
            // fails leaves the recorded verdict's notice on the view.
            let next = session.act_key(&format!("{key}/next"));
            let ids = calls.next_ids();
            let location = match web.api.review_next(&call(&credential, &ids), &next).await {
                Ok(Some(serve)) => format!("{}&{done}", served_location(page, &serve)),
                Ok(None) | Err(_) => format!("{page}?{done}"),
            };
            acts::see_other(&location, &[])
        }
        "bulk_confirm" => bulk_confirm(&web, &session, &calls, &fields, side).await,
        _ => {
            let verdict = match verdict_of(&fields, &calls) {
                Ok(v) => v,
                Err(refusal) => return *refusal,
            };
            let reason = fields.given("reason");
            if verdict == "reject" && reason.is_none() {
                return acts::invalid("reason is required to reject", &calls);
            }
            let ids: Vec<&str> = fields
                .given("artifact_ids")
                .unwrap_or_default()
                .split(' ')
                .filter(|id| !id.is_empty())
                .collect();
            if ids.is_empty() {
                return acts::invalid("pick at least one memory", &calls);
            }
            if ids.len() > BULK_MAX || !ids.iter().all(|id| is_id(id)) {
                return acts::invalid("artifact_ids holds at most 200 ids", &calls);
            }
            let key = match acts::key(&session, &fields, &calls) {
                Ok(key) => key,
                Err(refusal) => return *refusal,
            };
            let _in_flight = acts::hold(&key).await;
            let call_ids = calls.next_ids();
            match web
                .api
                .post_bulk_verdict(&call(&credential, &call_ids), &key, verdict, &ids, reason)
                .await
            {
                Ok(result) => {
                    let same = result
                        .items
                        .iter()
                        .filter(|i| i.same_user == Some(true))
                        .count();
                    let replayed = result
                        .items
                        .iter()
                        .filter(|i| i.replayed == Some(true))
                        .count();
                    acts::see_other(
                        &format!(
                            "{page}?done=bulk&approved={}&rejected={}&skipped={}&same_user={same}&replayed={replayed}",
                            result.summary.approved,
                            result.summary.rejected,
                            result.summary.skipped
                        ),
                        &[],
                    )
                }
                Err(e) => outcome::page(&e, calls.page_id(), page),
            }
        }
    }
}

/// `bulk_confirm`: what was picked, by claim key and first line, with the
/// one form that records the verdict on each. Records nothing.
async fn bulk_confirm(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    fields: &Fields,
    side: Side,
) -> Response {
    let page = side.path();
    let verdict = match verdict_of(fields, calls) {
        Ok(v) => v,
        Err(refusal) => return *refusal,
    };
    let reason = fields.given("reason");
    let mut picks: Vec<(&str, &str)> = fields
        .pairs()
        .filter(|(name, value)| is_pick(name) && !value.is_empty())
        .collect();
    picks.sort_unstable();
    if let Some((name, _)) = picks.iter().find(|(_, value)| !is_id(value)) {
        return acts::invalid(&format!("{name} is not an id"), calls);
    }
    let ids: Vec<&str> = picks.iter().map(|(_, id)| *id).collect();
    if ids.is_empty() {
        return acts::invalid("pick at least one memory", calls);
    }
    if ids.len() > BULK_MAX {
        return acts::invalid("a page picks at most 200 memories", calls);
    }
    if verdict == "reject" && reason.is_none() {
        return acts::invalid("reason is required to reject", calls);
    }
    let credential = session.credential(Capacity::Review);
    let call_ids = calls.next_ids();
    let named = match web
        .api
        .review_named(&call(&credential, &call_ids), &ids)
        .await
    {
        Ok(named) => named,
        Err(e) => return outcome::page(&e, calls.page_id(), page),
    };
    let mut body = format!(
        r#"<h1>{} {} memories?</h1><p class="desc">One verdict each. Any no longer held is skipped.</p><table><tr><th>Claim key</th><th>First line</th><th>Artifact</th></tr>"#,
        if verdict == "approve" {
            "Approve"
        } else {
            "Reject"
        },
        ids.len()
    );
    for id in &ids {
        match named
            .items
            .iter()
            .find(|n| n.id == *id)
            .and_then(|n| n.item.as_ref())
        {
            Some(m) => {
                let _ = write!(
                    body,
                    r#"<tr><td class="mono">{}</td><td class="text">{}</td><td class="mono">{}</td></tr>"#,
                    short(&m.claim_key),
                    text(
                        m.text
                            .as_deref()
                            .unwrap_or_default()
                            .lines()
                            .next()
                            .unwrap_or_default()
                    ),
                    attr(id)
                );
            }
            None => {
                let _ = write!(
                    body,
                    r#"<tr><td class="dim" colspan="2">no longer held, or not yours to judge: it will be skipped</td><td class="mono">{}</td></tr>"#,
                    attr(id)
                );
            }
        }
    }
    body.push_str("</table>");
    if let Some(reason) = reason {
        let _ = write!(body, r#"<p>Reason: {}</p>"#, text(reason));
    }
    body.push_str(&form(
        page,
        "bulk_verdict",
        &session.csrf(),
        &format!(
            "{}{}{}{}",
            hidden("verdict", verdict),
            hidden("reason", reason.unwrap_or("")),
            hidden("artifact_ids", &ids.join(" ")),
            acts::nonce_field()
        ),
        &format!("Yes, {verdict} {}", ids.len()),
        true,
    ));
    let _ = write!(body, r#"<p><a href="{page}">Back to the list</a></p>"#);
    outcome::html(
        StatusCode::OK,
        shell(
            "Bulk review",
            &session.who(),
            Some(&session.csrf()),
            &tabs(session, side),
            page,
            None,
            &body,
        ),
    )
}

/// `POST /web/review`: an act of the user page's view.
pub async fn user_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    act(web, session, calls, form, Side::User).await
}

/// `POST /web/admin/review`: an act of the admin page's view.
pub async fn admin_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    act(web, session, calls, form, Side::Admin).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crafted_notice_shows_only_numbers_and_the_views_own_words() {
        let fields = |q: &str| Fields::of(&format!("/web/review?{q}").parse().unwrap()).unwrap();
        assert_eq!(
            notice(&fields("done=verdict&version=3&state=promoted")).as_deref(),
            Some("Recorded: version 3 is now promoted.")
        );
        assert_eq!(
            notice(&fields("done=verdict&version=3&state=%3Cb%3E")),
            None
        );
        assert_eq!(
            notice(&fields("done=bulk&approved=x%3Cb%3E&rejected=0&skipped=0")).as_deref(),
            Some("Recorded: 0 approved, 0 rejected, 0 skipped.")
        );
        assert_eq!(notice(&fields("done=hello")), None);
    }
}
