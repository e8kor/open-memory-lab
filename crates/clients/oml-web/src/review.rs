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
//! - on both pages, the held listing as cards (`MemoryCard` and
//!   `ReviewCard`, `card.rs`): on the user page grouped by workarea
//!   from one read, each row with "Open to judge", which draws the item's
//!   card with a verdict form whose act, `judge`, records a one-id
//!   listed-set verdict (`postBulkVerdict`; ADR-0055 d4, as the operator
//!   decided on 2026-11-13: design notes rows 1 and 2);
//! - on the admin page only, bulk review (ADR-0055 d1): the held listing
//!   with its filters and a box per card;
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
    ADMIN_REVIEW_PATH, Bar, USER_MEMORIES_PATH, USER_REVIEW_PATH, attr, form, hidden, input,
    select, shell, short, text,
};
use oml_sdk::{HeldPage, Memory, encode};

use crate::acts::Refusal;
use crate::card::{Card, View, change};
use crate::pages::{Fields, refused};
use crate::picker::{self, Chosen, Field, Names, Roots};
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, acts, outcome};

/// The held listing's page size on the admin page (ADR-0055 d10).
pub const HELD_PAGE: u32 = 50;

/// The held listing's page size on the user page: the API's largest, so
/// "Waiting for review" is exact from one read up to it (design notes
/// row 6).
pub const WAITING_PAGE: u32 = 200;

/// The most of the person's keys the served page reads to say whether a
/// held row's author is one of them: one page (design notes row 15).
const OWN_KEYS_PAGE: u32 = 200;

/// The most memories one bulk verdict names (ADR-0055 d3).
pub const BULK_MAX: usize = 200;

/// The user page's acts: review next and the verdict.
pub const USER_ACTIONS: &[&str] = &["review_next", "verdict", "judge"];

/// The admin page's acts: review next, the served verdict and bulk review
/// (ADR-0055 d1); "Open to judge" is the person's page's alone (ADR-0055
/// d10 as amended on 2026-11-13: the admin page is unchanged).
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
        "empty" => Some("Nothing waits for review.".to_owned()),
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

impl ViewCtx<'_> {
    /// The kinds of top label of `paths`, as `capacity`'s key reads them:
    /// the person's own root, and one read of any other top label the
    /// paths name (design notes row 20).
    async fn roots<'p>(
        &self,
        capacity: Capacity,
        paths: impl IntoIterator<Item = &'p str>,
    ) -> Roots {
        Roots::default()
            .with_own(self.session.whoami.user.as_ref().map(|u| u.name.as_str()))
            .learn(
                self.web,
                &self.session.credential(capacity),
                self.calls,
                paths,
            )
            .await
    }

    /// The person's reviewer key's name, `<user>-review` (ADR-0045 d6).
    fn reviewer(&self) -> String {
        self.session.whoami.user.as_ref().map_or_else(
            || "the reviewer key".to_owned(),
            |u| format!("{}-review", u.name),
        )
    }
}

/// `value` as inline code.
fn code(value: &str) -> String {
    format!("<code>{}</code>", short(value))
}

/// The lineage check (ReviewCard): the reviewer key is outside the row's
/// lineage, which the listing guarantees (ADR-0055 d2, d5); whether the
/// author is one of the person's keys, where the page knows it, and so
/// whether approving is a self-review; and how many readable memories it
/// derives from.
fn lineage(card: &Card, reviewer: &str, same_user: Option<bool>) -> String {
    let author = card
        .author()
        .map_or_else(|| "the shared area".to_owned(), code);
    let mut line = format!("{} is outside it", code(reviewer));
    match same_user {
        Some(true) => {
            let _ = write!(
                line,
                "; the author, {author}, is one of this account's keys: approving is a self-review, marked same user."
            );
        }
        Some(false) => {
            let _ = write!(
                line,
                "; the author, {author}, is not one of this account's keys."
            );
        }
        None => line.push('.'),
    }
    match card.derives_count() {
        0 => {}
        1 => line.push_str(" It derives from 1 readable memory."),
        n => {
            let _ = write!(line, " It derives from {n} readable memories.");
        }
    }
    format!(
        r#"<ul class="oml-checks" aria-label="Checks"><li><span class="k">Lineage</span><span>{line}</span></li></ul>"#
    )
}

/// What each verdict does, in one muted line, by the kind of decision
/// (ReviewCard).
fn verdict_line(card: &Card, reviewer: &str, roots: &Roots) -> String {
    let reference = code(&card.reference(roots));
    let mut line = match card.replaces() {
        Some((_, Some(was))) if card.retirement() => format!(
            "Approve stops serving the claim: v{was} and this reason are kept. Reject keeps v{was} served."
        ),
        Some((_, Some(was))) => {
            let end = card
                .replaces_valid_until()
                .map_or_else(String::new, |at| format!(" at {}", short(at)));
            format!(
                "Approve serves v{} to every key that reads {reference} and ends v{was}{end}. Reject keeps v{was} served.",
                card.version()
            )
        }
        _ => format!(
            "Approve serves it to every key that reads {reference}. Reject keeps the text, as rejected."
        ),
    };
    let _ = write!(
        line,
        " Recorded under {}; a verdict is not undone.",
        code(reviewer)
    );
    format!(r#"<p class="desc">{line}</p>"#)
}

/// A listed row as a review card at row density: the decision first, the
/// facts, and opened, the checks, what each verdict does and "Open to
/// judge", which draws the item with its verdict form.
fn review_row(view: &ViewCtx<'_>, m: &Memory, same_user: bool, roots: &Roots) -> String {
    let card = Card::of_held(m, same_user);
    let reviewer = view.reviewer();
    // "Open to judge" is the person's page's; the admin page picks and
    // confirms instead (ADR-0055 d10 as amended).
    let open = if view.side == Side::User {
        format!(
            r#"<p><a class="button" href="{}?memory={}">Open to judge</a></p>"#,
            view.side.path(),
            attr(&encode(&m.artifact_id)),
        )
    } else {
        String::new()
    };
    let strip = format!(
        r#"<div class="oml-strip" aria-label="Review">{}{}{open}</div>"#,
        lineage(&card, &reviewer, Some(same_user)),
        verdict_line(&card, &reviewer, roots),
    );
    card.render(&View {
        decision: true,
        open_link: false,
        strip: &strip,
        ..View::row(view.side == Side::Admin, roots)
    })
}

/// Whether the author of `m` is one of the person's keys, from the
/// person's own key list as `<user>-web` (`GET /v2/keys`, one page; design
/// notes row 15): `None` when the list cannot say, read whole or not.
async fn own_author(view: &ViewCtx<'_>, m: &Memory) -> Option<bool> {
    let author = m.author_key_id.as_deref()?;
    let credential = view.session.credential(Capacity::Web);
    let ids = view.calls.next_ids();
    let page = view
        .web
        .api
        .list_keys(&call(&credential, &ids), None, OWN_KEYS_PAGE, None)
        .await
        .ok()?;
    let found = page.items.iter().any(|k| k.id == author);
    (found || page.next_cursor.is_none()).then_some(found)
}

/// The change against the version `m` replaces, read as the person's
/// reviewer key; nothing when it replaces none or the read fails.
async fn replaced_change(view: &ViewCtx<'_>, card: &Card) -> String {
    let Some((id, Some(was_version))) = card.replaces() else {
        return String::new();
    };
    let credential = view.session.credential(Capacity::Review);
    let ids = view.calls.next_ids();
    match view.web.api.get_memory(&call(&credential, &ids), id).await {
        Ok(was) => change(
            was_version,
            was.text.as_deref().unwrap_or_default(),
            card.version(),
            card.body(),
            card.retirement(),
        ),
        Err(_) => String::new(),
    }
}

/// The item the view judges, at page density, with the review strip: read
/// by its id from the held listing, which claims nothing; `act` is the
/// verdict form's hidden fields and action. A served item carries the
/// serve id as the API gave it (`verdict`); an item opened from the list
/// carries its own id, judged as a one-id listed-set verdict (`judge`).
async fn item(
    view: &ViewCtx<'_>,
    memory: Option<&str>,
    act: Act<'_>,
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
        .map_err(|e| Box::new(outcome::page(&e, view.calls, view.side.path())))?;
    let Some(item) = named.items.into_iter().find_map(|n| n.item) else {
        return Ok((
            r#"<p class="oml-noact">That memory is no longer held, or this reviewer key may not judge it.</p>"#
                .to_owned(),
            false,
        ));
    };
    let mut card = Card::of_held(&item, false);
    // The person's keys, the replaced version and the kinds of the
    // workarea's top label, in one round (design notes rows 4, 15, 20).
    let (same_user, changed, roots) = tokio::join!(
        own_author(view, &item),
        replaced_change(view, &card),
        view.roots(Capacity::Review, [card.workarea()]),
    );
    if same_user == Some(true) {
        card = Card::of_held(&item, true);
    }
    // A held diagram is drawn for the reviewer from the source the API
    // served them here, bound to their session (ADR-0056 d9; #189).
    let drawing = crate::render::drawing(
        view.web.layers.public_origin.as_deref(),
        item.format.as_deref(),
        &view.session.csrf(),
        &item.artifact_id,
        item.text.as_deref().unwrap_or_default(),
    );
    let reviewer = view.reviewer();
    let (action, fields) = match act {
        Act::Serve(serve_id) => ("verdict", hidden("serve_id", serve_id)),
        Act::Judge => ("judge", hidden("artifact_id", &item.artifact_id)),
    };
    let strip = format!(
        r#"<div class="oml-strip" aria-label="Review">{}{}{}</div>"#,
        lineage(&card, &reviewer, same_user),
        verdict_line(&card, &reviewer, &roots),
        form(
            view.side.path(),
            action,
            &view.session.csrf(),
            &format!(
                "{fields}{}{}{}",
                select(
                    "verdict",
                    "verdict",
                    &[("approve", "approve"), ("reject", "reject")]
                ),
                input("reason", "reason (required to reject)", "", false),
                acts::nonce_field()
            ),
            "Record verdict",
            true,
        ),
    );
    let out = card.render(&View {
        decision: true,
        open_link: false,
        drawing: drawing.as_deref().unwrap_or_default(),
        change: &changed,
        strip: &strip,
        ..View::page(view.side == Side::Admin, &roots)
    });
    Ok((out, drawing.is_some()))
}

/// How the item's verdict form acts.
#[derive(Clone, Copy)]
enum Act<'a> {
    /// The served ceremony, under the serve id (ADR-0045 d11).
    Serve(&'a str),
    /// A one-id listed-set verdict (ADR-0055 d4).
    Judge,
}

/// The tenant's queue, as the admin key reads it; each workarea links to
/// its held rows in the list below, by reference. Beside it, the queue's
/// references, the bulk filter's hints.
async fn queue(view: &ViewCtx<'_>) -> Result<(String, Names), Refusal> {
    let credential = view.session.credential(Capacity::Admin);
    let ids = view.calls.next_ids();
    // Beside the queue, one page of the tree as the admin key, whose
    // `user_root` marks name the top labels' kinds (design notes row 20):
    // one call, where a read per person's root would grow with the tenant.
    let own = view.session.whoami.user.as_ref().map(|u| u.name.as_str());
    let queue_call = call(&credential, &ids);
    let (rows, tree) = tokio::join!(
        view.web.api.review_queue(&queue_call),
        Names::read(
            view.web,
            &credential,
            view.calls,
            "universe",
            own,
            picker::LIST_HINTS_PAGES
        ),
    );
    let rows = rows.map_err(|e| Box::new(outcome::page(&e, view.calls, view.side.path())))?;
    let page = view.side.path();
    let roots = tree
        .roots()
        .clone()
        .learn(
            view.web,
            &credential,
            view.calls,
            rows.items.iter().map(|r| r.path.as_str()),
        )
        .await;
    let mut body = String::from(
        r#"<h2>Queue</h2><p class="desc">Held memories per workarea, every one of the tenant's; the list below shows only what this reviewer key may judge.</p><table><tr><th>Workarea</th><th>Held</th><th>Oldest since</th></tr>"#,
    );
    if rows.items.is_empty() {
        body.push_str(r#"<tr><td colspan="3" class="dim">Nothing is held.</td></tr>"#);
    }
    for row in &rows.items {
        let reference = roots.reference(&row.path);
        let _ = write!(
            body,
            r#"<tr><td class="mono"><a href="{page}?held=1&amp;workarea={}">{}</a></td><td>{}</td><td class="mono">{}</td></tr>"#,
            attr(&encode(&reference)),
            short(&reference),
            row.held,
            short(&row.oldest_at)
        );
    }
    body.push_str("</table>");
    let names = Names::of_places(
        rows.items
            .iter()
            .map(|r| (r.workarea_id.clone(), r.path.clone())),
        roots,
    );
    Ok((body, names))
}

/// The filters, in the order both pages draw them: the workarea first, as
/// the picker's `WorkareaField`, whose hints are the workareas the page's
/// own read names (the held listing on the user page, the queue on the
/// admin page), so the field costs no read (design notes row 21); then the
/// author key and the claim key prefix.
fn filters(view: &ViewCtx<'_>, fields: &Fields, names: &Names, chosen: &Chosen) -> String {
    let page = view.side.path();
    let value = |name: &str| fields.given(name).unwrap_or("");
    let mut keep: Vec<(&str, String)> = vec![("held", "1".to_owned())];
    for name in ["author_key", "claim_key_prefix"] {
        if let Some(v) = fields.given(name) {
            keep.push((name, v.to_owned()));
        }
    }
    let field = Field {
        id: "review-workarea",
        action: page,
        keep: &keep,
        chosen,
        names,
        browse: None,
        admin: view.side == Side::Admin,
        standalone: false,
        empty: "Empty: every workarea with something waiting.",
    }
    .html();
    format!(
        r#"<form class="row filters" method="get" action="{page}">{field}{}{}{}<button type="submit">Filter</button></form>"#,
        hidden("held", "1"),
        input("author_key", "author key", value("author_key"), false),
        input(
            "claim_key_prefix",
            "claim key prefix",
            value("claim_key_prefix"),
            false
        ),
    )
}

/// The filters the listing is read with: the chosen workarea as the
/// picker sends it (its id where the page's names hold it), and the
/// others as typed.
fn listing_filters<'a>(fields: &'a Fields, chosen: &'a Chosen) -> Vec<(&'a str, &'a str)> {
    let mut out: Vec<(&str, &str)> = chosen
        .value()
        .map(|w| ("workarea", w))
        .into_iter()
        .collect();
    out.extend(
        ["author_key", "claim_key_prefix"]
            .into_iter()
            .filter_map(|name| fields.given(name).map(|v| (name, v))),
    );
    out
}

/// The filters a link keeps, as typed.
fn kept_filters(fields: &Fields) -> String {
    ["workarea", "author_key", "claim_key_prefix"]
        .into_iter()
        .filter_map(|name| fields.given(name).map(|v| (name, v)))
        .map(|(k, v)| format!("&amp;{k}={}", attr(&encode(v))))
        .collect()
}

/// The held listing as the person's reviewer key reads it; `None` where
/// the picker refused the workarea before any call.
async fn listing(
    view: &ViewCtx<'_>,
    fields: &Fields,
    chosen: &Chosen,
    limit: u32,
) -> Result<Option<HeldPage>, Refusal> {
    if chosen.refused() {
        return Ok(None);
    }
    let credential = view.session.credential(Capacity::Review);
    let ids = view.calls.next_ids();
    match view
        .web
        .api
        .review_held(
            &call(&credential, &ids),
            &listing_filters(fields, chosen),
            limit,
            fields.given("cursor"),
        )
        .await
    {
        Ok(page) => Ok(Some(page)),
        Err(e) if picker::is_not_found(&e) => Ok(None),
        Err(e) => Err(Box::new(outcome::page(&e, view.calls, view.side.path()))),
    }
}

/// The workareas of a listing, each with its count and the instant its
/// oldest row was held, in the listing's order (oldest first).
fn waiting(page: &HeldPage, roots: &Roots) -> Vec<(String, usize, String)> {
    let mut groups: Vec<(String, usize, String)> = Vec::new();
    for item in &page.items {
        let reference = roots.reference(item.memory().workarea_path.as_deref().unwrap_or(""));
        match groups.iter_mut().find(|(r, _, _)| *r == reference) {
            Some(group) => group.1 += 1,
            None => groups.push((reference, 1, item.held_since.clone())),
        }
    }
    groups
}

/// The picker's answer to the listing: the rows, or the field's refusal
/// where the picker refused the workarea before any call or the API
/// answered `access.not_found` for it.
fn settled(listing: Option<HeldPage>, chosen: Chosen, names: &Names) -> (HeldPage, Chosen) {
    match listing {
        Some(page) => (page, names.settle(chosen)),
        None => {
            let chosen = if chosen.refused() {
                names.settle(chosen)
            } else {
                names.not_found(&chosen).unwrap_or(chosen)
            };
            (
                HeldPage {
                    items: Vec::new(),
                    next_cursor: None,
                },
                chosen,
            )
        }
    }
}

/// The user page's list (ReviewScreen): "Waiting for review" by workarea,
/// the filters, and every row the person's reviewer key may judge, as
/// review cards, from one read of the held listing (design notes rows 1,
/// 6, 15).
async fn person_list(view: &ViewCtx<'_>, fields: &Fields) -> Result<String, Refusal> {
    let page = view.side.path();
    let chosen = Chosen::of(fields.get("workarea"));
    let read = listing(view, fields, &chosen, WAITING_PAGE).await?;
    let roots = view
        .roots(
            Capacity::Review,
            read.iter()
                .flat_map(|p| p.items.iter())
                .filter_map(|i| i.memory().workarea_path.as_deref()),
        )
        .await;
    let groups = read
        .as_ref()
        .map(|p| waiting(p, &roots))
        .unwrap_or_default();
    let names = Names::of_places(
        read.iter().flat_map(|p| p.items.iter()).filter_map(|i| {
            let m = i.memory();
            Some((m.workarea_id.clone()?, m.workarea_path.clone()?))
        }),
        roots.clone(),
    );
    let (listing, chosen) = settled(read, chosen, &names);
    let total = if listing.next_cursor.is_some() {
        format!("{WAITING_PAGE} or more")
    } else {
        listing.items.len().to_string()
    };
    let mut body = format!(
        r#"<h2>Waiting for review · {total}</h2><table><tr><th>Workarea</th><th>Waiting</th><th>Oldest since</th></tr>"#
    );
    if groups.is_empty() {
        body.push_str(r#"<tr><td colspan="3" class="dim">Nothing waits for review.</td></tr>"#);
    }
    for (reference, n, oldest) in &groups {
        let _ = write!(
            body,
            r#"<tr><td class="mono"><a href="{page}?workarea={}">{}</a></td><td>{n}</td><td class="mono">{}</td></tr>"#,
            attr(&encode(reference)),
            short(reference),
            short(oldest)
        );
    }
    body.push_str("</table>");
    let _ = write!(
        body,
        r#"<h2>Held memories</h2><p class="desc">Oldest first, {WAITING_PAGE} a page.</p>{}"#,
        filters(view, fields, &names, &chosen)
    );
    if chosen.refused() {
        // The field's refusal stands in for the rows.
    } else if listing.items.is_empty() {
        body.push_str(r#"<p class="dim">Nothing waits for review.</p>"#);
    } else {
        body.push_str(r#"<ul class="oml-mcs">"#);
        for item in &listing.items {
            let _ = write!(
                body,
                "<li>{}</li>",
                review_row(view, item.memory(), item.same_user, &roots)
            );
        }
        body.push_str("</ul>");
    }
    if let Some(next) = &listing.next_cursor {
        let _ = write!(
            body,
            r#"<p><a href="{page}?held=1{}&amp;cursor={}">Next page</a></p>"#,
            kept_filters(fields),
            attr(&encode(next))
        );
    }
    Ok(body)
}

/// The bulk listing (AdminReviewScreen): the rows the reviewer key may
/// judge, filtered, each a review card with a box to pick it; "select all"
/// draws the page with every box checked, since the page carries no
/// script. The picker's names are the queue's workareas.
async fn held(view: &ViewCtx<'_>, fields: &Fields, names: &Names) -> Result<String, Refusal> {
    let page = view.side.path();
    let chosen = names.resolve(Chosen::of(fields.get("workarea")));
    let read = listing(view, fields, &chosen, HELD_PAGE).await?;
    let (listing, chosen) = settled(read, chosen, names);
    // The queue's kinds, and a read of any top label the listing names
    // beyond them, as the admin key, which reads every node.
    let roots = names
        .roots()
        .clone()
        .learn(
            view.web,
            &view.session.credential(Capacity::Admin),
            view.calls,
            listing
                .items
                .iter()
                .filter_map(|i| i.memory().workarea_path.as_deref()),
        )
        .await;
    let all = fields.get("select") == Some("all");
    let kept = kept_filters(fields);
    let cursor = fields.given("cursor").map_or(String::new(), |c| {
        format!("&amp;cursor={}", attr(&encode(c)))
    });
    let mut body = format!(
        r#"<h2>Bulk review</h2><p class="desc">Oldest first, {HELD_PAGE} a page. A memory this reviewer key wrote, or that derives from one, is never listed.</p>{}<p><a href="{page}?held=1{kept}{cursor}&amp;select=all">Select all on this page</a> · <a href="{page}?held=1{kept}{cursor}">Clear</a></p>"#,
        filters(view, fields, names, &chosen)
    );
    let mut rows = String::new();
    if chosen.refused() {
        // The field's refusal stands in for the rows.
    } else if listing.items.is_empty() {
        rows.push_str(r#"<p class="dim">Nothing waits for review here.</p>"#);
    } else {
        rows.push_str(r#"<ul class="oml-mcs">"#);
        for (i, item) in listing.items.iter().enumerate() {
            let m = item.memory();
            let _ = write!(
                rows,
                r#"<li class="oml-pick"><span class="box"><input type="checkbox" aria-label="Pick {} v{}" name="pick_{i:03}" value="{}"{}></span>{}</li>"#,
                attr(&m.claim_key),
                m.version,
                attr(&m.artifact_id),
                if all { " checked" } else { "" },
                review_row(view, m, item.same_user, &roots)
            );
        }
        rows.push_str("</ul>");
    }
    body.push_str(&form(
        page,
        "bulk_confirm",
        &view.session.csrf(),
        &format!(
            r#"{rows}<div class="oml-bulkbar">{}{}<p class="desc">Each pick gets its own verdict under {}; a confirmation comes first.</p></div>"#,
            select(
                "verdict",
                "verdict",
                &[("approve", "approve"), ("reject", "reject")]
            ),
            input("reason", "reason (required to reject)", "", false),
            code(&view.reviewer())
        ),
        "Review picks",
        false,
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

fn bar(session: &Session, side: Side) -> Bar {
    match side {
        Side::User => session.personal(),
        Side::Admin => session.administration(),
    }
}

/// Review next, the view's one primary act, and what it serves.
fn review_next(view: &ViewCtx<'_>, fields: &Fields, primary: bool) -> String {
    let mut out = form(
        view.side.path(),
        "review_next",
        &view.session.csrf(),
        &acts::nonce_field(),
        "Review next",
        primary,
    );
    // The serve takes no workarea yet (design notes row 7).
    out.push_str(if fields.given("workarea").is_some() {
        r#"<p class="desc">Oldest first, in every workarea.</p>"#
    } else {
        r#"<p class="desc">Oldest first.</p>"#
    });
    out
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
    let reviewer = ctx.reviewer();
    let mut body = String::from(r#"<p class="label">Review</p><h1>Review</h1>"#);
    let _ = match side {
        Side::User => write!(
            body,
            r#"<p class="desc">Judged as {}. A memory this reviewer key wrote, or that derives from one, is never listed. Held writes by this account's keys, whoever reviews them: <a href="{USER_MEMORIES_PATH}?state=held">Memories, state held</a>.</p>"#,
            code(&reviewer)
        ),
        Side::Admin => write!(
            body,
            r#"<p class="desc">The queue is read as {}, which never judges. Every verdict here is {}'s.</p>"#,
            code(&session.whoami.user.as_ref().map_or_else(
                || "the admin key".to_owned(),
                |u| format!("{}-admin", u.name)
            )),
            code(&reviewer)
        ),
    };
    let mut framed = false;
    // One item, served by Review next or opened to judge from the list,
    // drawn alone at page density.
    let act = match (fields.given("serve"), fields.given("memory"), side) {
        (Some(serve), _, _) => Some(Act::Serve(serve)),
        (None, Some(_), Side::User) => Some(Act::Judge),
        _ => None,
    };
    if let Some(act) = act {
        if let (Act::Serve(_), Some(at)) = (act, expiry(&fields)) {
            let _ = write!(
                body,
                r#"<p class="desc">Served for review until {}. Held text is read by its author, the reviewer it is served to, an admin through the journalled admin read, and readers inside an in-progress workarea.</p>"#,
                text(at)
            );
        }
        match item(&ctx, fields.given("memory"), act).await {
            Ok((card, drawn)) => {
                body.push_str(&card);
                framed = drawn;
            }
            Err(response) => return *response,
        }
        body.push_str(&review_next(&ctx, &fields, false));
        let _ = write!(body, r#"<p><a href="{page}">Back to the list</a></p>"#);
    } else {
        body.push_str(&review_next(&ctx, &fields, true));
        let listed = match side {
            Side::User => person_list(&ctx, &fields).await,
            Side::Admin => match queue(&ctx).await {
                Ok(queue) => {
                    body.push_str(&queue.0);
                    held(&ctx, &fields, &queue.1).await
                }
                Err(response) => Err(response),
            },
        };
        match listed {
            Ok(listed) => body.push_str(&listed),
            Err(response) => return *response,
        }
    }
    let mut response = outcome::html(
        StatusCode::OK,
        shell(
            if act.is_some() { "One memory" } else { "" },
            &bar(&session, side),
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
        "judge" => matches!(name, "artifact_id" | "verdict" | "reason"),
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

/// [`served_location`] with the serve's expiry, which the page shows
/// (design notes row 18). The claim a verdict chains goes without it: a
/// replayed claim answers its serve with a later expiry, and a resubmitted
/// verdict must land where the first did.
fn served_location_until(page: &str, serve: &oml_sdk::Serve) -> String {
    format!(
        "{}&expires={}",
        served_location(page, serve),
        encode(&serve.expires_at)
    )
}

/// The serve's expiry the redirect carried (design notes row 18), read as
/// an instant's characters only, so a crafted link puts no other text on
/// the page.
fn expiry(fields: &Fields) -> Option<&str> {
    fields.given("expires").filter(|at| {
        at.len() <= 40
            && at.bytes().next().is_some_and(|b| b.is_ascii_digit())
            && at
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b':' | b'.' | b'T' | b'Z' | b'+'))
    })
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
                Ok(Some(serve)) => acts::see_other(&served_location_until(page, &serve), &[]),
                Ok(None) => acts::see_other(&format!("{page}?done=empty"), &[]),
                Err(e) => outcome::page(&e, &calls, page),
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
                Err(e) => return outcome::page(&e, &calls, page),
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
        "judge" => judge(&web, &session, &calls, &fields, side).await,
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
                Err(e) => outcome::page(&e, &calls, page),
            }
        }
    }
}

/// The page status of a skipped item's code, as the single verdict would
/// answer it (`docs/api/errors.md`).
fn skip_status(code: &str) -> StatusCode {
    match code {
        "access.not_found" => StatusCode::NOT_FOUND,
        "review.author_in_lineage" | "access.grant_refused" => StatusCode::FORBIDDEN,
        _ => StatusCode::CONFLICT,
    }
}

/// `judge`: one listed item's verdict, recorded as a listed-set verdict of
/// one id (`POST /v2/review/verdicts/bulk`), which runs the single
/// verdict's use case and journals its events (ADR-0055 d4). An item the
/// API skips is refused by its code and records nothing: R2's
/// `review.author_in_lineage` for a row in the reviewer key's lineage
/// (d5), `review.serve_invalid` for one no longer held (d6).
async fn judge(
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
    if verdict == "reject" && reason.is_none() {
        return acts::invalid("reason is required to reject", calls);
    }
    let Some(id) = fields.given("artifact_id").filter(|id| is_id(id)) else {
        return acts::invalid("artifact_id is the listed memory's id", calls);
    };
    let key = match acts::key(session, fields, calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Review);
    let ids = calls.next_ids();
    let result = match web
        .api
        .post_bulk_verdict(&call(&credential, &ids), &key, verdict, &[id], reason)
        .await
    {
        Ok(result) => result,
        Err(e) => return outcome::page(&e, calls, page),
    };
    let Some(item) = result.items.into_iter().find(|i| i.artifact_id == id) else {
        return refused(
            StatusCode::BAD_GATEWAY,
            "unavailable",
            "the server answered the verdict without the item it names",
            calls,
        );
    };
    if item.outcome == "skipped" {
        let code = item.code.as_deref().unwrap_or("unavailable");
        return refused(
            skip_status(code),
            code,
            item.message
                .as_deref()
                .unwrap_or("the verdict was not recorded"),
            calls,
        );
    }
    let state = item
        .state
        .as_deref()
        .filter(|s| matches!(*s, "promoted" | "rejected"))
        .unwrap_or("held");
    acts::see_other(
        &format!(
            "{page}?done=verdict&version={}&state={state}",
            item.version.unwrap_or_default()
        ),
        &[],
    )
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
        Err(e) => return outcome::page(&e, calls, page),
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
                    r#"<tr><td class="dim" colspan="2">no longer held, or this reviewer key may not judge it: it will be skipped</td><td class="mono">{}</td></tr>"#,
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
        shell("Confirm", &bar(session, side), page, None, &body),
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

    #[test]
    fn a_serves_expiry_is_shown_only_as_an_instants_characters() {
        let fields = |q: &str| Fields::of(&format!("/web/review?{q}").parse().unwrap()).unwrap();
        assert_eq!(
            expiry(&fields("expires=2026-11-12T17%3A10%3A00.000000Z")),
            Some("2026-11-12T17:10:00.000000Z")
        );
        assert_eq!(expiry(&fields("expires=%3Cb%3E")), None);
        assert_eq!(expiry(&fields("expires=soon")), None);
    }
}
