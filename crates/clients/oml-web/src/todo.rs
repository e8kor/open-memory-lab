//! The To do page of each side and the Judge page (ADR-0083 d2, d6, d11,
//! d12, d14), in place of the Review view: what waits on the signed-in
//! person, `/web/todo`, and on the admin key, `/web/admin/todo`.
//!
//! - **To do** lists the groups the counts read serves, today To review
//!   alone (d14): the held rows the person's reviewer key, `<user>-review`,
//!   may judge, from `GET /v2/review/held`, oldest first, under the page's
//!   workspace filter; each row's act, **Judge**, opens
//!   `/web/todo/judge?memory=<id>`. The group's count is the counts read's
//!   `review`, never a fetched page counted (d10). **Next**, the page's one
//!   primary, is the console's own act: one row of the same listing, oldest
//!   first under the filter, and a `303` to its Judge page, or back here
//!   with "Nothing waits" (d14).
//! - **Judge** draws the item at page density with the review strip and,
//!   for a supersession, the change against the version it replaces, as
//!   the console's own word comparison (ADR-0082 d6; `crate::diff`), one
//!   more read of that version per item. Its verdict is a one-id listed-set
//!   verdict as the person's reviewer key (`postBulkVerdict`; ADR-0055 d4),
//!   after which the page returns to To do with its notice; Next opens the
//!   next item without a verdict.
//! - **Admin To do** has no group built yet, so it says "Nothing waits for
//!   this account." and draws no verdict, confirm or ratify (d11, d14).
//!
//! Every act is the person's reviewer key's, or a tenant key an admin
//! linked to the person that the page's `as` field names (ADR-0085 d16):
//! the admin key never judges (ADR-0045 d11), and the API refuses a
//! verdict from a key in the item's lineage, which the page shows by name
//! (`review.author_in_lineage`).

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{
    ADMIN_MEMORIES_PATH, ADMIN_TODO_PATH, Bar, TODO_JUDGE_PATH, TODO_PATH, USER_MEMORIES_PATH,
    attr, form, hidden, input, select, shell, short, text,
};
use oml_sdk::{HeldItem, HeldPage, LinkedKey, Memory, encode};

use crate::acts::Refusal;
use crate::card::{Card, View};
use crate::diff::{Parts, labelled};
use crate::keys::instant;
use crate::pages::{Fields, refused};
use crate::picker::{self, Browse, Chosen, Field, Names, Roots};
use crate::session::{PageCalls, Session, call, call_as};
use crate::{Capacity, Web, acts, outcome};

/// The To review group's page size: the held listing's largest.
pub const TODO_PAGE: u32 = 200;

/// The most of the person's keys the Judge page reads to say whether a
/// held row's author is one of them: one page (design notes row 15).
const OWN_KEYS_PAGE: u32 = 200;

/// To do's one act, Next (ADR-0083 d11, d14).
pub const TODO_ACTIONS: &[&str] = &["next", "ratify", "dismiss"];

/// One admin fix's page below To do, where a person confirms or reverts
/// it (ADR-0089 d8, d15; ADR-0096 P14).
pub const TODO_FIX_PATH: &str = "/web/todo/fix";

/// The fix page's acts.
pub const FIX_ACTIONS: &[&str] = &["confirm", "revert"];

/// Judge's one act, the verdict (ADR-0083 d2).
pub const JUDGE_ACTIONS: &[&str] = &["judge"];

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

/// The notice To do's own acts name, built from numbers and a closed set
/// of words only, so a crafted link cannot put text on the page.
fn notice(fields: &Fields) -> Option<String> {
    match fields.get("done")? {
        "verdict" => {
            let state = match fields.get("state")? {
                // The stored state `promoted` reads as approved (ADR-0091 d16).
                "promoted" => "approved",
                s @ ("held" | "rejected") => s,
                _ => return None,
            };
            Some(format!(
                "Recorded: version {} is now {state}.",
                count(fields, "version")
            ))
        }
        "empty" => Some("Nothing waits for this account.".to_owned()),
        "ratified" => Some("Ratified: the memory joins the brief.".to_owned()),
        "dismissed" => Some("Dismissed: the notice leaves To do and stays in the journal.".to_owned()),
        "fix_confirmed" => Some(
            "Confirmed: the version the fix serves reads confirmed from now on.".to_owned(),
        ),
        "fix_reverted" => Some(
            "Reverted: the text the fix took out of service returns as a new version this account approved.".to_owned(),
        ),
        "fix_confirmed_same_user" => Some(
            "Confirmed, marked same user: the person the admin acted for settled it in their own root.".to_owned(),
        ),
        "fix_reverted_same_user" => Some(
            "Reverted, marked same user: the person the admin acted for settled it in their own root.".to_owned(),
        ),
        _ => None,
    }
}

/// What the person's pages share.
struct Ctx<'a> {
    web: &'a Web,
    session: &'a Session,
    calls: &'a PageCalls,
    /// The tenant key an admin linked to the person that the page judges
    /// as, named by its `as` field; `None` judges as the person's reviewer
    /// key (ADR-0085 d16).
    acting: Option<LinkedKey>,
}

impl Ctx<'_> {
    /// The acting key's id, which every reviewer call carries.
    fn acting_id(&self) -> Option<&str> {
        self.acting.as_ref().map(|k| k.id.as_str())
    }

    /// `&amp;as=<id>` for a link that keeps the acting key, else nothing.
    fn kept_acting(&self) -> String {
        self.acting_id()
            .map_or_else(String::new, |id| format!("&amp;as={}", attr(&encode(id))))
    }

    /// The kinds of top label of `paths`, as the reviewer key reads them.
    async fn roots<'p>(&self, paths: impl IntoIterator<Item = &'p str>) -> Roots {
        Roots::default()
            .with_own(self.session.whoami.user.as_ref().map(|u| u.name.as_str()))
            .learn(
                self.web,
                &self.session.credential(Capacity::Review),
                self.calls,
                paths,
            )
            .await
    }

    /// The name of the key the page judges as: the linked key it acts as,
    /// else the person's reviewer key, `<user>-review` (ADR-0045 d6).
    fn reviewer(&self) -> String {
        if let Some(acting) = &self.acting {
            return acting.name.clone();
        }
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

/// The linked key the page's `as` field names, read only when named, so
/// the person's own To do spends no round on it (ADR-0085 d16; ADR-0070
/// d8): `Ok(None)` without the field, a refusal for a key not linked to
/// this account.
async fn acting_of(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    fields: &Fields,
) -> Result<Option<LinkedKey>, Refusal> {
    let Some(id) = fields.given("as") else {
        return Ok(None);
    };
    let credential = session.credential(Capacity::Web);
    let ids = calls.next_ids();
    let linked = match web.api.whoami_person(&call(&credential, &ids)).await {
        Ok(onboarding) => onboarding.person.map(|p| p.linked_keys).unwrap_or_default(),
        Err(e) => return Err(Box::new(outcome::page(&e, calls, TODO_PATH))),
    };
    match linked.into_iter().find(|k| k.id == id) {
        Some(key) => Ok(Some(key)),
        None => Err(Box::new(refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no key linked to this account has that id",
            calls,
        ))),
    }
}

/// The lineage check (ReviewCard): the reviewer key is outside the row's
/// lineage, which the listing guarantees (ADR-0055 d2, d5); whether the
/// author is one of the person's keys, where the page knows it, and so
/// whether approving is a self-review; and how many readable memories it
/// derives from.
fn lineage(card: &Card, reviewer: &str, same_user: Option<bool>, needs_person: bool) -> String {
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
    // Only a person's verdict approves an instruction (ADR-0080 d16).
    let person = if needs_person {
        r#"<li><span class="k">Needs a person</span><span>An instruction: only a person's reviewer key approves it; any other key may reject it.</span></li>"#
    } else {
        ""
    };
    format!(
        r#"<ul class="oml-checks" aria-label="Checks"><li><span class="k">Lineage</span><span>{line}</span></li>{person}</ul>"#
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
                .map_or_else(String::new, |at| format!(" at {}", instant(at)));
            format!(
                "Approve serves v{} to every key that reads {reference} and ends v{was}{end}. Reject keeps v{was} served.",
                card.version()
            )
        }
        // The kind comes from the item; the version it replaces is one this
        // key cannot read, so the line names it by its kind alone.
        _ if card.retirement() => "Approve stops serving the claim; the version it retires is not readable to this key. Reject keeps that version served.".to_owned(),
        _ if card.successor() => format!(
            "Approve serves v{} to every key that reads {reference} and ends the version it replaces, which is not readable to this key. Reject keeps that version served.",
            card.version()
        ),
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

/// The parts of a version the change compares: a skill's name and
/// description beside its body (ADR-0082 d6).
fn parts(m: &Memory) -> Parts<'_> {
    Parts {
        name: m.skill.as_ref().map(|s| s.name.as_str()),
        description: m.skill.as_ref().map(|s| s.description.as_str()),
        body: m.text.as_deref().unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------
// To do
// ---------------------------------------------------------------------

/// A To review row (TodoScreen): the decision, the kind where it is not a
/// memory, the name, the workspace, the author and `same user` where it
/// applies, and the wait; its act, **Judge**.
fn review_row(ctx: &Ctx<'_>, item: &HeldItem, roots: &Roots) -> String {
    let m = item.memory();
    let card = Card::of_held(m, item.same_user);
    let kind = card
        .kind()
        .filter(|k| *k != "memory")
        .map_or_else(String::new, |k| {
            format!(r#"<span class="tag oml-chip is-quiet">{}</span>"#, text(k))
        });
    let name = m
        .skill
        .as_ref()
        .map(|s| s.name.as_str())
        .or(m.title.as_deref())
        .unwrap_or(&m.claim_key);
    let author = m.principal.as_deref().map_or_else(
        || r#"<span class="k">author</span> the shared area"#.to_owned(),
        |p| {
            format!(
                r#"<span class="k">by</span> <span class="t">{}</span>"#,
                short(p)
            )
        },
    );
    let same = if item.same_user {
        r#"<span class="tag oml-chip is-quiet">same user</span>"#
    } else {
        ""
    };
    let person = if item.needs_person {
        r#"<span>needs a person</span>"#
    } else {
        ""
    };
    format!(
        r#"<li><div class="td-main"><div class="td-what">{}{kind}<span class="lnk mono">{}</span></div><div class="td-facts"><span><span class="k">in</span> <span class="t">{}</span></span><span>{author}</span>{same}{person}<span><span class="k">waiting since</span> {}</span></div></div><div class="td-act"><a class="button" href="{TODO_JUDGE_PATH}?memory={}{}">Judge</a></div></li>"#,
        card.decision(),
        short(name),
        short(&card.reference(roots)),
        instant(&item.held_since),
        attr(&encode(&m.artifact_id)),
        ctx.kept_acting(),
    )
}

/// The held listing under the picked workspace, oldest first, as the
/// person's reviewer key (or the linked key) reads it; `None` where the
/// picker refused the workspace or the API answered `access.not_found`.
async fn listing(
    ctx: &Ctx<'_>,
    chosen: &Chosen,
    limit: u32,
    cursor: Option<&str>,
) -> Result<Option<HeldPage>, oml_sdk::Error> {
    if chosen.refused() {
        return Ok(None);
    }
    let credential = ctx.session.credential(Capacity::Review);
    let ids = ctx.calls.next_ids();
    let filters: Vec<(&str, &str)> = chosen
        .value()
        .map(|w| ("workspace", w))
        .into_iter()
        .collect();
    match ctx
        .web
        .api
        .review_held(
            &call_as(&credential, &ids, ctx.acting_id()),
            &filters,
            limit,
            cursor,
        )
        .await
    {
        Ok(page) => Ok(Some(page)),
        Err(e) if picker::is_not_found(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The To review group's count: the counts read's `review` under the
/// page's workspace (ADR-0083 d11), the session's own read where the page
/// filters nothing; `None` where it did not answer, or where the page acts
/// as a linked key, which the counts read does not count.
async fn group_count(ctx: &Ctx<'_>, chosen: &Chosen) -> Option<(u64, bool)> {
    if ctx.acting.is_some() || chosen.refused() {
        return None;
    }
    match chosen.value() {
        None => ctx.session.todo.map(|t| (t.review, t.truncated)),
        Some(workspace) => {
            let credential = ctx.session.credential(Capacity::Web);
            let ids = ctx.calls.next_ids();
            ctx.web
                .api
                .todo_counts(&call(&credential, &ids), Some(workspace))
                .await
                .ok()
                .map(|t| (t.review, t.truncated))
        }
    }
}

/// `N`, `1000+` past the cap, or the page's own rows where the count did
/// not answer and the page holds every row.
fn shown_count(counted: Option<(u64, bool)>, page: &HeldPage) -> Option<String> {
    match counted {
        Some((n, true)) => Some(format!("{n}+")),
        Some((n, false)) => Some(n.to_string()),
        None if page.next_cursor.is_none() => Some(page.items.len().to_string()),
        None => None,
    }
}

/// The Personal To do (TodoScreen): the heading, the capacity line, the
/// workspace filter, Next, and the groups the API serves.
async fn todo_body(
    ctx: &Ctx<'_>,
    fields: &Fields,
    chosen: Chosen,
    browse: Option<&Browse>,
) -> Result<String, Refusal> {
    let cursor = fields.given("cursor");
    let (read, counted, briefs, fixes, notices) = tokio::join!(
        listing(ctx, &chosen, TODO_PAGE, cursor),
        group_count(ctx, &chosen),
        brief_proposals(ctx, &chosen),
        review_fixes(ctx, &chosen),
        person_notices(ctx)
    );
    let read = read.map_err(|e| Box::new(outcome::page(&e, ctx.calls, TODO_PATH)))?;
    let roots = ctx
        .roots(
            read.iter()
                .flat_map(|p| p.items.iter())
                .filter_map(|i| i.memory().workspace_path.as_deref()),
        )
        .await;
    let names = Names::of_places(
        read.iter().flat_map(|p| p.items.iter()).filter_map(|i| {
            let m = i.memory();
            Some((m.workspace_id.clone()?, m.workspace_path.clone()?))
        }),
        roots.clone(),
    );
    let (page, chosen) = match read {
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
                    lane_authors: None,
                },
                chosen,
            )
        }
    };
    let reviewer = ctx.reviewer();
    let mut body = String::from(
        r#"<header class="oml-section-head"><div class="top"><h1>To do</h1></div><p class="desc">What waits for this account's keys, oldest first in each group; each row opens the page that names the key that acts.</p></header>"#,
    );
    if ctx.acting.is_some() {
        let own = ctx.session.whoami.user.as_ref().map_or_else(
            || "the reviewer key".to_owned(),
            |u| format!("{}-review", u.name),
        );
        let _ = write!(
            body,
            r#"<p class="desc">An admin linked {} to this account; it judges in the shared workspaces it is granted. <a href="{TODO_PATH}">Judge as {}</a></p>"#,
            code(&reviewer),
            code(&own)
        );
    }
    let mut keep: Vec<(&str, String)> = Vec::new();
    if let Some(id) = ctx.acting_id() {
        keep.push(("as", id.to_owned()));
    }
    let field = Field {
        id: "todo-workspace",
        action: TODO_PATH,
        keep: &keep,
        chosen: &chosen,
        names: &names,
        browse,
        admin: false,
        standalone: true,
        empty: "Empty: every workspace.",
    }
    .html();
    body.push_str(&field);
    // Next, the page's one primary: the oldest row under the filter
    // (ADR-0083 d11, d14).
    let mut next_fields = String::new();
    if let Some(w) = chosen.value() {
        next_fields.push_str(&hidden("workspace", w));
    }
    if let Some(id) = ctx.acting_id() {
        next_fields.push_str(&hidden("as", id));
    }
    body.push_str(&form(
        TODO_PATH,
        "next",
        &ctx.session.csrf(),
        &next_fields,
        "Next",
        true,
    ));
    if chosen.refused() {
        // The field's refusal stands in for the groups.
        return Ok(body);
    }
    let briefs_section = briefs.as_ref().map(|b| brief_group(ctx, b));
    // The person's notices, whatever the workspace (ADR-0097 d20).
    let notices_section = notices.as_deref().map(|n| notice_group(ctx, n));
    // Admin fixes to confirm, after To review (TodoScreen, v0.8.5).
    let fixes_section = fixes.as_deref().map(fix_group);
    let after_review = |body: &mut String| {
        let mut nothing = Vec::new();
        for (section, title) in [
            (&notices_section, "Notices"),
            (&fixes_section, "Admin fixes to confirm"),
            (&briefs_section, "Brief proposals to ratify"),
        ] {
            match section {
                Some(Some(section)) => body.push_str(section),
                Some(None) => nothing.push(title),
                None => {}
            }
        }
        if !nothing.is_empty() {
            let _ = write!(
                body,
                r#"<p class="desc">Nothing waits in {}.</p>"#,
                nothing.join(" or ")
            );
        }
    };
    let others_wait = matches!(fixes_section, Some(Some(_)))
        || matches!(briefs_section, Some(Some(_)))
        || matches!(notices_section, Some(Some(_)));
    if page.items.is_empty() && cursor.is_none() {
        match others_wait {
            true => {
                body.push_str(r#"<p class="desc">Nothing waits in To review.</p>"#);
                after_review(&mut body);
            }
            false => {
                let _ = write!(
                    body,
                    r#"<div class="oml-empty"><p>Nothing waits for this account.</p><p class="desc">Held writes of other keys that {} may judge, admin fixes it may confirm or revert, and brief proposals it may ratify, appear here. This account's own writes and their state are under <a href="{USER_MEMORIES_PATH}?view=submitted">Memories, Submitted</a>.</p></div>"#,
                    code(&reviewer)
                );
            }
        }
        return Ok(body);
    }
    let shown = shown_count(counted, &page).map_or_else(String::new, |n| format!(" · {n}"));
    let _ = write!(
        body,
        r#"<section class="oml-todo-g" aria-labelledby="todo-review"><h2 id="todo-review">To review{shown}</h2><p class="desc">Held writes of other keys that {} may judge, oldest first. A memory this key wrote, or that derives from one, is never listed.</p><ul class="oml-todo">"#,
        code(&reviewer)
    );
    for item in &page.items {
        body.push_str(&review_row(ctx, item, &roots));
    }
    body.push_str("</ul>");
    if let Some(next) = &page.next_cursor {
        let workspace = chosen.value().map_or_else(String::new, |w| {
            format!("&amp;workspace={}", attr(&encode(w)))
        });
        let _ = write!(
            body,
            r#"<p class="pager"><a href="{TODO_PATH}?cursor={}{workspace}{}">Next page</a></p>"#,
            attr(&encode(next)),
            ctx.kept_acting()
        );
    }
    body.push_str("</section>");
    after_review(&mut body);
    Ok(body)
}

/// The person's notices not dismissed, `GET /v2/web/notices` as the web
/// session key (ADR-0097 d20): `None` where the page judges as a linked
/// key, or the read fails, which the group then leaves out.
async fn person_notices(ctx: &Ctx<'_>) -> Option<Vec<oml_sdk::Notice>> {
    if ctx.acting.is_some() {
        return None;
    }
    let credential = ctx.session.credential(Capacity::Web);
    let ids = ctx.calls.next_ids();
    Some(
        ctx.web
            .api
            .notices(&call(&credential, &ids))
            .await
            .ok()?
            .items,
    )
}

/// What a notice tells about, in words (openapi.yaml `Notice.kind`).
fn notice_words(kind: &str) -> &'static str {
    match kind {
        "act_against_admin" => "An act against an admin",
        "offensive_narrowing" => "An act that narrows a person's or a workspace's access",
        "root_reach" => "An admin reached into this account's own root",
        "recovery" => "The operator recovers the tenant",
        "settlement" => "The operator settles a dispute",
        _ => "An admin act",
    }
}

/// The Notices group (ADR-0097 d20, d6, d24): each notice not dismissed,
/// oldest first, what it tells about, the act, who took it and when, and
/// its Dismiss form; `None` when there is none.
fn notice_group(ctx: &Ctx<'_>, notices: &[oml_sdk::Notice]) -> Option<String> {
    if notices.is_empty() {
        return None;
    }
    let mut out = format!(
        r#"<section class="oml-todo-g" aria-labelledby="todo-notices"><h2 id="todo-notices">Notices · {}</h2><p class="desc">Admin acts this account is told about, oldest first: an act against an admin, an act that narrows a person's or a workspace's access, a reach into this account's own root. Dismissing one removes it from To do; the journal keeps it.</p><ul class="oml-todo">"#,
        notices.len()
    );
    for n in notices {
        let field = |name: &str| n.detail.get(name).and_then(serde_json::Value::as_str);
        let _ = write!(
            out,
            r#"<li><span class="oml-decision">{}</span> · {} · by {} · {}{}</li>"#,
            text(notice_words(&n.kind)),
            text(&field("act").unwrap_or("admin act").replace('_', " ")),
            field("actor_person").map_or_else(
                || "an admin key that counts as no person".to_owned(),
                |p| format!(r#"<span class="mono">{}</span>"#, text(p))
            ),
            instant(&n.at),
            form(
                TODO_PATH,
                "dismiss",
                &ctx.session.csrf(),
                &format!("{}{}", hidden("notice", &n.notice_id), acts::nonce_field()),
                "Dismiss",
                false
            ),
        );
    }
    out.push_str("</ul></section>");
    Some(out)
}

/// **Dismiss** (ADR-0097 d20): `POST /v2/web/notices/{notice}/dismiss` as
/// the web session key, the person's own act, and back to To do.
async fn dismiss(web: &Web, session: &Session, calls: &PageCalls, fields: &Fields) -> Response {
    let Some(notice) = fields.given("notice").filter(|id| is_id(id)) else {
        return acts::invalid("notice is a notice's id", calls);
    };
    let key = match acts::key(session, fields, calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Web);
    let ids = calls.next_ids();
    match web
        .api
        .dismiss_notice(&call(&credential, &ids), &key, notice)
        .await
    {
        Ok(_) => acts::see_other(&format!("{TODO_PATH}?done=dismissed"), &[]),
        Err(e) => outcome::page(&e, calls, TODO_PATH),
    }
}

/// The unconfirmed admin fixes `<user>-review` may settle, `GET
/// /v2/review/fixes` (ADR-0089 d15), under the filter: a fix serves its
/// workspace's id alone, so a filter by name is read once as the
/// workspace's id, `GET /v2/workspaces/{workspace}`. `None` where the page
/// judges as a linked key, or a read fails, which the group then leaves
/// out.
async fn review_fixes(ctx: &Ctx<'_>, chosen: &Chosen) -> Option<Vec<oml_sdk::Fix>> {
    if ctx.acting.is_some() {
        return None;
    }
    let credential = ctx.session.credential(Capacity::Review);
    let workspace = match chosen.value() {
        None => None,
        Some(w) if is_id(w) => Some(w.to_owned()),
        Some(name) => {
            let ids = ctx.calls.next_ids();
            let view = ctx
                .web
                .api
                .workspace(&call(&credential, &ids), name)
                .await
                .ok()?;
            Some(view.id)
        }
    };
    let ids = ctx.calls.next_ids();
    let fixes = ctx
        .web
        .api
        .review_fixes(&call(&credential, &ids))
        .await
        .ok()?;
    Some(match workspace {
        Some(w) => fixes.into_iter().filter(|f| f.workspace_id == w).collect(),
        None => fixes,
    })
}

/// The group "Admin fixes to confirm" (TodoScreen P13): the act, the
/// claim key, `unconfirmed`, `alert` where it loosens a guardrail, the
/// reason and the wait, and **Confirm or revert**, which opens the fix's
/// page. The admin is never named. `None` when nothing waits.
fn fix_group(fixes: &[oml_sdk::Fix]) -> Option<String> {
    if fixes.is_empty() {
        return None;
    }
    let mut out = format!(
        r#"<section class="oml-todo-g" aria-labelledby="todo-fixes"><h2 id="todo-fixes">Admin fixes to confirm · {}</h2><p class="desc">Break-glass changes an admin made where this account reviews; each stays marked until a person confirms or reverts it.</p><ul class="oml-todo">"#,
        fixes.len()
    );
    for f in fixes {
        let _ = write!(
            out,
            r#"<li><span class="oml-decision">{}</span> {} · <span class="oml-chip is-adminfix">unconfirmed</span>{} · {} · fixed {} · <a class="button" href="{TODO_FIX_PATH}?id={}">{}</a></li>"#,
            text(&f.act),
            code(&f.claim_key),
            if f.loosens_guardrail { " · alert" } else { "" },
            text(&f.reason),
            instant(&f.fixed_at),
            attr(&encode(&f.fix_id)),
            settle_label(&f.acts)
        );
    }
    out.push_str("</ul></section>");
    Some(out)
}

/// The fix's link as the served `acts` name it, never computed here
/// (ADR-0096 d5): **Confirm or revert**, **Confirm** for an erasure, and
/// **Open** where this account is served no act.
fn settle_label(acts: &[String]) -> &'static str {
    let may = |a: &str| acts.iter().any(|x| x == a);
    match (may("confirm"), may("revert")) {
        (true, true) => "Confirm or revert",
        (true, false) => "Confirm",
        (false, true) => "Revert",
        (false, false) => "Open",
    }
}

/// The sentence saying what this account may do with a fix, read off the
/// served `acts` (ADR-0096 d5).
fn may_settle(acts: &[String]) -> &'static str {
    match settle_label(acts) {
        "Confirm or revert" => "This account may confirm or revert it.",
        "Confirm" => "This account may confirm it; an erasure is not reverted.",
        "Revert" => "This account may revert it.",
        _ => "",
    }
}

/// What a fix's act does once settled, for its confirmation.
fn settles(act: &str, fix: &oml_sdk::Fix) -> &'static str {
    match (act, fix.act.as_str()) {
        ("revert", _) => {
            "The text the fix took out of service returns as a new version, derived from it and approved by this account's verdict."
        }
        (_, "erase") => "The erasure stands, and the mark reads confirmed.",
        _ => "The version the fix serves stays served and reads confirmed from now on.",
    }
}

/// `GET /web/todo/fix?id=` (P14): the fix as `<user>-review` reads it,
/// with the replaced version's number from the target card's read, and
/// **Confirm** and **Revert** as `acts` serves them, each confirmed on
/// `act=` before its one POST; served no act, one line why.
pub async fn fix_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let Some(id) = fields.given("id").filter(|id| is_id(id)) else {
        return acts::invalid("id is an admin fix's id", &calls);
    };
    let credential = session.credential(Capacity::Review);
    let ids = calls.next_ids();
    let fixes = match web.api.review_fixes(&call(&credential, &ids)).await {
        Ok(fixes) => fixes,
        Err(e) => return outcome::page(&e, &calls, TODO_PATH),
    };
    let Some(fix) = fixes.into_iter().find(|f| f.fix_id == id) else {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no unconfirmed admin fix with that id waits for this account",
            &calls,
        );
    };
    let ids = calls.next_ids();
    let target = web
        .api
        .get_memory(&call(&credential, &ids), &fix.target_artifact_id)
        .await
        .ok();
    let version = target
        .as_ref()
        .map_or_else(String::new, |m| format!(" v{}", m.version));
    let mut body = format!(
        r#"<nav class="crumbs" aria-label="Breadcrumb">Personal / <a href="{TODO_PATH}">To do</a> / Admin fix</nav><h1>Admin fix</h1><div class="oml-card"><p><span class="oml-decision">{}</span> {}{version} · <span class="oml-chip is-adminfix">unconfirmed</span>{}</p><p>{}</p><p class="desc">Fixed {}{}.</p></div>"#,
        text(&fix.act),
        code(&fix.claim_key),
        if fix.loosens_guardrail {
            " · alert: it loosens a guardrail"
        } else {
            ""
        },
        text(&fix.reason),
        instant(&fix.fixed_at),
        target
            .as_ref()
            .and_then(|m| m.workspace_path.as_deref())
            .map_or_else(String::new, |w| format!(" in {}", code(w)))
    );
    if fix.acts.is_empty() {
        body.push_str(r#"<p class="desc">No act: this account is the person the admin acted for, and the fix lies outside this account's root, or in a protected or frozen workspace, or in a release, so another person confirms or reverts it.</p>"#);
    } else if let Some(act) = fields
        .given("act")
        .filter(|a| fix.acts.iter().any(|x| x == a))
    {
        let word = if act == "revert" { "Revert" } else { "Confirm" };
        let _ = write!(
            body,
            r#"<h2>{word} the fix?</h2><p class="desc">{} The settlement is journalled under {}.</p>{}<p><a href="{TODO_FIX_PATH}?id={}">Keep it waiting</a></p>"#,
            settles(act, &fix),
            code(&format!(
                "{}-review",
                session
                    .whoami
                    .user
                    .as_ref()
                    .map_or("the", |u| u.name.as_str())
            )),
            form(
                TODO_FIX_PATH,
                act,
                &session.csrf(),
                &format!("{}{}", hidden("fix_id", &fix.fix_id), acts::nonce_field()),
                word,
                true
            ),
            attr(&encode(&fix.fix_id))
        );
    } else {
        let _ = write!(body, r#"<p class="desc">{}</p>"#, may_settle(&fix.acts));
        body.push_str("<p>");
        for act in &fix.acts {
            let _ = write!(
                body,
                r#"<a class="button" href="{TODO_FIX_PATH}?id={}&amp;act={}">{}</a> "#,
                attr(&encode(&fix.fix_id)),
                attr(act),
                if act == "revert" { "Revert" } else { "Confirm" }
            );
        }
        body.push_str("</p>");
    }
    outcome::html(
        StatusCode::OK,
        shell("Admin fix", &session.personal(), TODO_PATH, None, &body),
    )
}

/// `POST /web/todo/fix`: `POST /v2/review/fixes/{id}/confirm` or
/// `/revert` as `<user>-review` (ADR-0089 d15), and back to To do with its
/// notice; a refusal is drawn with its code beside its sentence.
pub async fn fix_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form_sent: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    let (fields, action) = match acts::read(form_sent, FIX_ACTIONS, |_, n| n == "fix_id", &calls) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let Some(fix) = fields.given("fix_id").filter(|id| is_id(id)) else {
        return acts::invalid("fix_id is an admin fix's id", &calls);
    };
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Review);
    let ids = calls.next_ids();
    let revert = action == "revert";
    match web
        .api
        .settle_fix(&call(&credential, &ids), &key, fix, revert)
        .await
    {
        Ok(settled) => {
            // The server records whether the admin's own person settled
            // it; the notice reads that, never a rule of its own.
            let same_user = settled.get("same_user") == Some(&serde_json::Value::Bool(true));
            let done = match (revert, same_user) {
                (true, true) => "fix_reverted_same_user",
                (true, false) => "fix_reverted",
                (false, true) => "fix_confirmed_same_user",
                (false, false) => "fix_confirmed",
            };
            acts::see_other(&format!("{TODO_PATH}?done={done}"), &[])
        }
        Err(e) => outcome::page(&e, &calls, TODO_FIX_PATH),
    }
}

/// The brief proposals `<user>-review` may ratify, under the filter
/// (ADR-0096 d7): `None` where the page judges as a linked key, which this
/// group does not serve, or where the read fails, which the group then
/// leaves out.
async fn brief_proposals(ctx: &Ctx<'_>, chosen: &Chosen) -> Option<oml_sdk::BriefProposalPage> {
    if ctx.acting.is_some() {
        return None;
    }
    let credential = ctx.session.credential(Capacity::Review);
    let ids = ctx.calls.next_ids();
    ctx.web
        .api
        .brief_proposals(&call(&credential, &ids), chosen.value())
        .await
        .ok()
}

/// The group "Brief proposals to ratify", after To review (ADR-0096 d7):
/// each row's claim, workspace, proposer and `same user`, and **Ratify**,
/// which asks for confirmation first. `None` when nothing waits.
fn brief_group(ctx: &Ctx<'_>, page: &oml_sdk::BriefProposalPage) -> Option<String> {
    if page.items.is_empty() {
        return None;
    }
    let reviewer = ctx.reviewer();
    let mut out = format!(
        r#"<section class="oml-todo-g" aria-labelledby="todo-briefs"><h2 id="todo-briefs">Brief proposals to ratify · {}{}</h2><p class="desc">Memories other keys proposed for a workspace's brief, which {} may ratify, oldest first. A proposal by this key, or under its principal, is never listed.</p><ul class="oml-todo">"#,
        page.items.len(),
        if page.next_cursor.is_some() { "+" } else { "" },
        code(&reviewer)
    );
    let csrf = ctx.session.csrf();
    for p in &page.items {
        let title = p
            .title
            .as_deref()
            .or(p.claim_key.as_deref())
            .unwrap_or(p.artifact_id.as_str());
        let _ = write!(
            out,
            r#"<li><span class="oml-decision">Brief</span> {} · into {} · proposed by {}{} · {} {}</li>"#,
            text(title),
            code(p.workspace.name.as_deref().unwrap_or(&p.workspace.id)),
            code(p.proposed_by.name.as_deref().unwrap_or(&p.proposed_by.id)),
            if p.same_user { " · same user" } else { "" },
            instant(&p.proposed_at),
            form(
                TODO_PATH,
                "ratify",
                &csrf,
                &format!(
                    "{}{}{}",
                    hidden("edge_id", &p.edge_id),
                    hidden("title", title),
                    acts::nonce_field()
                ),
                "Ratify",
                false
            )
        );
    }
    out.push_str("</ul></section>");
    Some(out)
}

fn personal(session: &Session) -> Bar {
    session.personal()
}

/// `GET /web/todo`: the Personal To do.
pub async fn user_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let acting = match acting_of(&web, &session, &calls, &fields).await {
        Ok(acting) => acting,
        Err(refusal) => return *refusal,
    };
    let ctx = Ctx {
        web: &web,
        session: &session,
        calls: &calls,
        acting,
    };
    // Browse, opened by the field's link: the counted tree as the web key,
    // made only when asked (design notes row 14).
    let browse = if picker::browse_asked(fields.get(picker::BROWSE)) {
        Some(Browse::read(&web, &session.credential(Capacity::Web), &calls, "universe").await)
    } else {
        None
    };
    let chosen = Chosen::of(fields.get("workspace"));
    match todo_body(&ctx, &fields, chosen, browse.as_ref()).await {
        Ok(body) => outcome::html(
            StatusCode::OK,
            shell(
                "To do",
                &personal(&session),
                TODO_PATH,
                notice(&fields).as_deref(),
                &body,
            ),
        ),
        Err(refusal) => *refusal,
    }
}

/// The fields To do's and Judge's acts define, beside `action`,
/// `csrf_token` and `nonce`.
fn defined(action: &str, name: &str) -> bool {
    match action {
        "next" => matches!(name, "workspace" | "as" | "after"),
        "ratify" => matches!(name, "edge_id" | "title" | "confirm"),
        "dismiss" => name == "notice",
        "judge" => matches!(name, "artifact_id" | "verdict" | "reason" | "as"),
        _ => false,
    }
}

/// The rows of one page Next reads to find the one after the item left.
/// A constant of the build, unmeasured.
const NEXT_SCAN_ROWS: u32 = 50;

/// The pages Next reads at most to find the item left: past them it opens
/// the oldest row, so Next walks the first 1,000 rows of the queue in
/// order. A constant of the build, unmeasured.
const NEXT_SCAN_PAGES: usize = 20;

/// `POST /web/todo`, Next (ADR-0083 d11, d14): the held listing under the
/// form's workspace, oldest first, read a page at a time until the row
/// after `after`, the item the person leaves unjudged, is found, and a
/// `303` to its Judge page, or to the oldest row's past the last; with
/// none, back to To do with its notice. It records nothing.
pub async fn user_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    let (fields, action) = match acts::read(form, TODO_ACTIONS, defined, &calls) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    if action == "ratify" {
        return ratify(&web, &session, &calls, &fields).await;
    }
    if action == "dismiss" {
        return dismiss(&web, &session, &calls, &fields).await;
    }
    let acting = match fields.given("as") {
        None => None,
        Some(id) if is_id(id) => Some(id),
        Some(_) => return acts::invalid("as is a linked key's id", &calls),
    };
    let after = match fields.given("after") {
        None => None,
        Some(id) if is_id(id) => Some(id),
        Some(_) => return acts::invalid("after is the left item's id", &calls),
    };
    let workspace = fields.given("workspace");
    let credential = session.credential(Capacity::Review);
    let filters: Vec<(&str, &str)> = workspace.map(|w| ("workspace", w)).into_iter().collect();
    // The queue a page at a time, oldest first: Next opens the row after
    // the one left unjudged, so it walks the queue instead of cycling
    // between the two oldest, and pages on past the first page so a row
    // left there is not followed by the oldest again; past the last it
    // starts over at the oldest (the review of PR #494, 6 and its
    // re-verification).
    let mut oldest: Option<String> = None;
    let mut next: Option<String> = None;
    let mut left_seen = false;
    let mut cursor: Option<String> = None;
    for _ in 0..NEXT_SCAN_PAGES {
        let ids = calls.next_ids();
        let page = match web
            .api
            .review_held(
                &call_as(&credential, &ids, acting),
                &filters,
                NEXT_SCAN_ROWS,
                cursor.as_deref(),
            )
            .await
        {
            Ok(page) => page,
            Err(e) => return outcome::page(&e, &calls, TODO_PATH),
        };
        let rows: Vec<&str> = page
            .items
            .iter()
            .map(|i| i.memory().artifact_id.as_str())
            .collect();
        if oldest.is_none() {
            oldest = rows
                .iter()
                .find(|id| Some(**id) != after)
                .map(|id| (*id).to_owned());
        }
        let from = if left_seen {
            Some(0)
        } else {
            after
                .and_then(|a| rows.iter().position(|id| *id == a))
                .map(|i| i + 1)
        };
        if let Some(from) = from {
            left_seen = true;
            if let Some(id) = rows.get(from) {
                next = Some((*id).to_owned());
                break;
            }
        }
        match (after, page.next_cursor) {
            (Some(_), Some(more)) => cursor = Some(more),
            _ => break,
        }
    }
    let next = next.or(oldest);
    let kept_acting = acting.map_or_else(String::new, |id| format!("&as={}", encode(id)));
    match next.as_deref() {
        Some(id) => acts::see_other(
            &format!("{TODO_JUDGE_PATH}?memory={}{kept_acting}", encode(id)),
            &[],
        ),
        None => {
            let workspace =
                workspace.map_or_else(String::new, |w| format!("&workspace={}", encode(w)));
            acts::see_other(
                &format!("{TODO_PATH}?done=empty{workspace}{kept_acting}"),
                &[],
            )
        }
    }
}

/// **Ratify** (ADR-0096 d7): a confirmation first, then `POST
/// /v2/edges/{id}/ratify` as `<user>-review`, and back to To do.
async fn ratify(web: &Web, session: &Session, calls: &PageCalls, fields: &Fields) -> Response {
    let Some(edge) = fields.given("edge_id").filter(|id| is_id(id)) else {
        return acts::invalid("edge_id is a brief proposal's id", calls);
    };
    let title = fields.given("title").unwrap_or(edge);
    if fields.given("confirm") != Some("yes") {
        let reviewer = session.whoami.user.as_ref().map_or_else(
            || "the reviewer key".to_owned(),
            |u| format!("{}-review", u.name),
        );
        let body = format!(
            r#"<h1>Ratify the brief proposal?</h1><p class="desc">{} joins the brief, ratified by {}. An agent is served the brief up to its budget.</p>{}<p><a href="{TODO_PATH}">Keep it waiting</a></p>"#,
            text(title),
            code(&reviewer),
            form(
                TODO_PATH,
                "ratify",
                &session.csrf(),
                &format!(
                    "{}{}{}{}",
                    hidden("edge_id", edge),
                    hidden("title", title),
                    hidden("confirm", "yes"),
                    acts::nonce_field()
                ),
                "Ratify",
                true
            )
        );
        return outcome::html(
            StatusCode::OK,
            shell("Confirm", &session.personal(), TODO_PATH, None, &body),
        );
    }
    let key = match acts::key(session, fields, calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Review);
    let ids = calls.next_ids();
    match web
        .api
        .ratify_edge(&call(&credential, &ids), &key, edge)
        .await
    {
        Ok(_) => acts::see_other(&format!("{TODO_PATH}?done=ratified"), &[]),
        Err(e) => outcome::page(&e, calls, TODO_PATH),
    }
}

// ---------------------------------------------------------------------
// Judge
// ---------------------------------------------------------------------

/// Whether the author of `m` is one of the person's keys, from the
/// person's own key list as `<user>-web` (`GET /v2/keys`, one page; design
/// notes row 15): `None` when the list cannot say.
async fn own_author(ctx: &Ctx<'_>, m: &Memory) -> Option<bool> {
    let author = m.author_key_id.as_deref()?;
    let credential = ctx.session.credential(Capacity::Web);
    let ids = ctx.calls.next_ids();
    let page = ctx
        .web
        .api
        .list_keys(&call(&credential, &ids), None, OWN_KEYS_PAGE, None)
        .await
        .ok()?;
    let found = page.items.iter().any(|k| k.id == author);
    (found || page.next_cursor.is_none()).then_some(found)
}

/// The change of a held supersession against the version it replaces,
/// read as the reviewer key, one more read per item (ADR-0082 d6;
/// `web_console_reads.rs`): for an edit the labelled word comparison of
/// name, description and body; for a retirement what stops being served;
/// nothing for a first version or where the read fails.
async fn replaced_change(ctx: &Ctx<'_>, card: &Card, now: &Memory) -> String {
    const UNREADABLE: &str = r#"<p class="desc">The version it replaces is not readable to this key, so no change is drawn.</p>"#;
    let Some((id, Some(was_version))) = card.replaces() else {
        return if card.successor() {
            UNREADABLE.to_owned()
        } else {
            String::new()
        };
    };
    let credential = ctx.session.credential(Capacity::Review);
    let ids = ctx.calls.next_ids();
    match ctx
        .web
        .api
        .get_memory(&call_as(&credential, &ids, ctx.acting_id()), id)
        .await
    {
        Ok(was) if card.retirement() => crate::diff::change(
            was_version,
            was.text.as_deref().unwrap_or_default(),
            card.version(),
            card.body(),
            true,
        ),
        Ok(was) => labelled(
            u32::try_from(was_version).unwrap_or(was.version),
            parts(&was),
            parts(now),
        ),
        Err(_) => UNREADABLE.to_owned(),
    }
}

/// The nonce of the verdict form drawn for `id`: bound to the memory, so
/// the form resent with another id conflicts as any act's resend with
/// another request does. The API keys a listed verdict per item
/// (`{key}/{id}`), so a nonce alone would judge the other memory too.
fn judge_nonce(session: &Session, id: &str) -> String {
    session.act_key(&format!("judge/{id}"))[..32].to_owned()
}

/// The Judge page's body: the breadcrumb, the item at page density with
/// the review strip and its verdict form, and Next; whether a diagram's
/// frame is drawn.
async fn judge_body(ctx: &Ctx<'_>, memory: Option<&str>) -> Result<(String, bool), Refusal> {
    let Some(memory_id) = memory.filter(|m| is_id(m)) else {
        return Err(Box::new(acts::invalid(
            "memory is the held memory's id",
            ctx.calls,
        )));
    };
    let mut body = format!(
        r#"<nav class="crumbs" aria-label="Breadcrumb">Personal / <a href="{TODO_PATH}">To do</a> / Judge</nav><h1>Judge</h1>"#
    );
    let credential = ctx.session.credential(Capacity::Review);
    let ids = ctx.calls.next_ids();
    let named = ctx
        .web
        .api
        .review_named(&call_as(&credential, &ids, ctx.acting_id()), &[memory_id])
        .await
        .map_err(|e| Box::new(outcome::page(&e, ctx.calls, TODO_JUDGE_PATH)))?;
    let Some((item, needs_person)) = named
        .items
        .into_iter()
        .find_map(|n| n.item.map(|item| (item, n.needs_person)))
    else {
        let _ = write!(
            body,
            r#"<p class="oml-noact">That memory is no longer held, or this reviewer key may not judge it.</p><p><a href="{TODO_PATH}">Back to To do</a></p>"#
        );
        return Ok((body, false));
    };
    let mut card = Card::of_held(&item, false);
    // The person's keys, the replaced version and the kinds of the
    // workspace's top label, in one round (design notes rows 4, 15, 20).
    let (same_user, changed, roots) = tokio::join!(
        own_author(ctx, &item),
        replaced_change(ctx, &card, &item),
        ctx.roots([card.workspace()]),
    );
    if same_user == Some(true) {
        card = Card::of_held(&item, true);
    }
    // A held diagram is drawn for the reviewer from the source the API
    // served them here, bound to their session (ADR-0056 d9; #189).
    let drawing = crate::render::drawing(
        ctx.web.layers.public_origin.as_deref(),
        item.format.as_deref(),
        &ctx.session.csrf(),
        &item.artifact_id,
        item.text.as_deref().unwrap_or_default(),
    );
    let reviewer = ctx.reviewer();
    if same_user == Some(true) {
        body.push_str(r#"<p class="desc">Written by one of this account's keys inside its own root: approving it is marked <code>same_user</code>.</p>"#);
    }
    let fields = format!(
        "{}{}",
        hidden("artifact_id", &item.artifact_id),
        ctx.acting_id()
            .map_or_else(String::new, |id| hidden("as", id))
    );
    let strip = format!(
        r#"<div class="oml-strip" aria-label="Review">{}{}{}</div>"#,
        lineage(&card, &reviewer, same_user, needs_person),
        verdict_line(&card, &reviewer, &roots),
        form(
            TODO_JUDGE_PATH,
            "judge",
            &ctx.session.csrf(),
            &format!(
                "{fields}{}{}{}",
                select(
                    "verdict",
                    "Verdict",
                    &[("approve", "approve"), ("reject", "reject")]
                ),
                input("reason", "Reason (required to reject)", "", false),
                hidden("nonce", &judge_nonce(ctx.session, &item.artifact_id)),
            ),
            "Record verdict",
            true,
        ),
    );
    body.push_str(&card.render(&View {
        decision: true,
        open_link: false,
        drawing: drawing.as_deref().unwrap_or_default(),
        change: &changed,
        strip: &strip,
        ..View::page(false, &roots)
    }));
    // Next opens the next item without a verdict (ADR-0083 d2).
    let mut next_fields = hidden("after", &item.artifact_id);
    if let Some(id) = ctx.acting_id() {
        next_fields.push_str(&hidden("as", id));
    }
    body.push_str(&form(
        TODO_PATH,
        "next",
        &ctx.session.csrf(),
        &next_fields,
        "Next",
        false,
    ));
    let _ = write!(
        body,
        r#"<p><a href="{TODO_PATH}{}">Back to To do</a></p>"#,
        ctx.acting_id()
            .map_or_else(String::new, |id| format!("?as={}", attr(&encode(id))))
    );
    Ok((body, drawing.is_some()))
}

/// `GET /web/todo/judge?memory=`: one held item to judge (ADR-0083 d2).
pub async fn judge_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let acting = match acting_of(&web, &session, &calls, &fields).await {
        Ok(acting) => acting,
        Err(refusal) => return *refusal,
    };
    let ctx = Ctx {
        web: &web,
        session: &session,
        calls: &calls,
        acting,
    };
    match judge_body(&ctx, fields.given("memory")).await {
        Ok((body, framed)) => {
            let mut response = outcome::html(
                StatusCode::OK,
                shell(
                    "Judge · To do",
                    &personal(&session),
                    TODO_JUDGE_PATH,
                    None,
                    &body,
                ),
            );
            if framed {
                crate::render::allow_frames(&mut response, web.layers.public_origin.as_deref());
            }
            response
        }
        Err(refusal) => *refusal,
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

/// The page status of a skipped item's code, as the single verdict would
/// answer it (`docs/api/errors.md`).
fn skip_status(code: &str) -> StatusCode {
    match code {
        "access.not_found" => StatusCode::NOT_FOUND,
        "review.author_in_lineage" | "access.grant_refused" => StatusCode::FORBIDDEN,
        _ => StatusCode::CONFLICT,
    }
}

/// `POST /web/todo/judge`: the item's verdict, recorded as a listed-set
/// verdict of one id (`POST /v2/review/verdicts/bulk`), which runs the
/// single verdict's use case and journals its events (ADR-0055 d4); then
/// back to To do with its notice (ADR-0083 d2). An item the API skips is
/// refused by its code and records nothing: R2's
/// `review.author_in_lineage` for a row in the reviewer key's lineage
/// (d5), `review.serve_invalid` for one no longer held (d6).
pub async fn judge_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    let (fields, _) = match acts::read(form, JUDGE_ACTIONS, defined, &calls) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let verdict = match verdict_of(&fields, &calls) {
        Ok(v) => v,
        Err(refusal) => return *refusal,
    };
    let reason = fields.given("reason");
    if verdict == "reject" && reason.is_none() {
        return acts::invalid("reason is required to reject", &calls);
    }
    let Some(id) = fields.given("artifact_id").filter(|id| is_id(id)) else {
        return acts::invalid("artifact_id is the held memory's id", &calls);
    };
    // A sent nonce is the one drawn for this memory; with none, the act
    // runs under a fresh one, as every act does.
    if fields
        .given("nonce")
        .is_some_and(|nonce| nonce != judge_nonce(&session, id))
    {
        return refused(
            StatusCode::CONFLICT,
            "idempotency.digest_mismatch",
            "this form was drawn for another memory; open the memory to judge it",
            &calls,
        );
    }
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    // The linked key the item was opened as, which the lead checks is
    // linked to this session's person (ADR-0085 d16).
    let acting = match fields.given("as") {
        None => None,
        Some(id) if is_id(id) => Some(id),
        Some(_) => return acts::invalid("as is a linked key's id", &calls),
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Review);
    let ids = calls.next_ids();
    let result = match web
        .api
        .post_bulk_verdict(
            &call_as(&credential, &ids, acting),
            &key,
            verdict,
            &[id],
            reason,
        )
        .await
    {
        Ok(result) => result,
        Err(e) => return outcome::page(&e, &calls, TODO_JUDGE_PATH),
    };
    let Some(item) = result.items.into_iter().find(|i| i.artifact_id == id) else {
        return refused(
            StatusCode::BAD_GATEWAY,
            "unavailable",
            "the server answered the verdict without the item it names",
            &calls,
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
            &calls,
        );
    }
    let state = item
        .state
        .as_deref()
        .filter(|s| matches!(*s, "promoted" | "rejected"))
        .unwrap_or("held");
    let kept = acting.map_or_else(String::new, |id| format!("&as={}", encode(id)));
    acts::see_other(
        &format!(
            "{TODO_PATH}?done=verdict&version={}&state={state}{kept}",
            item.version.unwrap_or_default()
        ),
        &[],
    )
}

// ---------------------------------------------------------------------
// Administration
// ---------------------------------------------------------------------

/// The administration To do's "Done: public cleared" group (ADR-0098 d6):
/// a clear narrows every key's default recall at once and waits on nobody,
/// so it is listed as done, the newest first, and never counted; empty
/// when no clear is recorded.
fn cleared_section(acts: &[oml_sdk::AdminAct]) -> String {
    let cleared: Vec<String> = acts
        .iter()
        .rev()
        .filter(|a| a.act == "workspace_public_clear")
        .take(DONE_MAX)
        .map(crate::admin_acts::act_row)
        .collect();
    if cleared.is_empty() {
        return String::new();
    }
    let mut out = format!(
        r#"<section class="oml-todo-g" aria-labelledby="todo-done"><h2 id="todo-done">Done: public cleared · {}</h2><p class="desc">Public flags an admin person cleared, newest first: each took effect at once and needs nothing more, and each opens its act.</p><ul class="oml-todo">"#,
        cleared.len()
    );
    for r in &cleared {
        out.push_str(r);
    }
    out.push_str("</ul></section>");
    out
}

/// The most done acts the administration To do lists, newest first.
const DONE_MAX: usize = 20;

/// `GET /web/admin/todo`: the administration To do (ADR-0083 d11; ADR-0096
/// d5, d7): the tenant's fixes no person has confirmed or reverted, the
/// guardrail-loosening alerts among them, and the open investigations,
/// each row opening the admin page where it is handled. No row offers a
/// verdict, a confirmation or a ratification: those are a person's acts.
pub async fn admin_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
) -> Response {
    if !session.admin {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        );
    }
    let admin = session.whoami.user.as_ref().map_or_else(
        || "the admin key".to_owned(),
        |u| format!("{}-admin", u.name),
    );
    let credential = session.credential(Capacity::Admin);
    let (fix_ids, investigation_ids, act_ids) =
        (calls.next_ids(), calls.next_ids(), calls.next_ids());
    let (fix_call, investigation_call, act_call) = (
        call(&credential, &fix_ids),
        call(&credential, &investigation_ids),
        call(&credential, &act_ids),
    );
    let (fixes, investigations, admin_acts) = tokio::join!(
        web.api.admin_fixes(&fix_call),
        web.api.investigations(&investigation_call),
        web.api.admin_acts(&act_call, "all")
    );
    let fixes = match fixes {
        Ok(fixes) => fixes,
        Err(e) => return outcome::page(&e, &calls, ADMIN_TODO_PATH),
    };
    let investigations = match investigations {
        Ok(open) => open,
        Err(e) => return outcome::page(&e, &calls, ADMIN_TODO_PATH),
    };
    let admin_acts = match admin_acts {
        Ok(listed) => listed.items,
        Err(e) => return outcome::page(&e, &calls, ADMIN_TODO_PATH),
    };
    // Admin acts to confirm (ADR-0097 d2, d10, d11, d23): the acts the API
    // marks this person may confirm or review, as its count judges them.
    let act_rows: Vec<String> = admin_acts
        .iter()
        .filter(|a| a.to_confirm)
        .map(crate::admin_acts::act_row)
        .collect();
    let mut body = format!(
        r#"<header class="oml-section-head"><div class="top"><h1>To do</h1></div><p class="desc">What waits for {}, oldest first in each group. Verdicts, confirmations and ratifications are a person's acts, on the Personal side.</p></header>"#,
        code(&admin)
    );
    let row = |f: &oml_sdk::AdminFix| {
        format!(
            r#"<li><span class="oml-decision">{}</span> {} · <span class="oml-chip is-adminfix">unconfirmed</span> · {}{} · fixed {} · waits for a person with reviewer on the workspace · <a href="{}?workspace={}&amp;view=fix">Open</a></li>"#,
            text(&f.act),
            code(&f.claim_key),
            text(&f.reason),
            if f.loosens_guardrail { " · alert" } else { "" },
            instant(&f.fixed_at),
            crate::workspace::ADMIN_WORKSPACE_PATH,
            attr(&encode(&f.workspace_id))
        )
    };
    let (alerts, plain): (Vec<_>, Vec<_>) = fixes.iter().partition(|f| f.loosens_guardrail);
    // Open investigations (AdminTodo, group 2): each opens the admin
    // Memories page, whose reads name the session's open investigation;
    // ADR-0089 d10 closes one from the line those pages draw.
    let investigation_rows: Vec<String> = investigations
        .iter()
        .map(|i| {
            format!(
                r#"<li><span class="oml-decision">Investigation</span> {} · {} · {} read{} journalled · opened {} · <a href="{ADMIN_MEMORIES_PATH}">Open</a></li>"#,
                text(&i.reason),
                code(&i.investigation_id),
                i.reads,
                if i.reads == 1 { "" } else { "s" },
                instant(&i.opened_at)
            )
        })
        .collect();
    let plain_rows: Vec<String> = plain.iter().map(|f| row(f)).collect();
    let alert_rows: Vec<String> = alerts.iter().map(|f| row(f)).collect();
    let mut empty = Vec::new();
    for (id, title, desc, rows) in [
        (
            "todo-acts",
            "Admin acts to confirm",
            "Widening acts of another admin person that wait on a second admin person, and acts a sole admin took at once; each opens its confirm screen.",
            &act_rows,
        ),
        (
            "todo-fixes",
            "Admin fixes, unconfirmed",
            "Fixes in this tenant that no person has confirmed or reverted.",
            &plain_rows,
        ),
        (
            "todo-investigations",
            "Open investigations",
            "This admin key's investigations still open: every read of claim text under one is journalled with its reason.",
            &investigation_rows,
        ),
        (
            "todo-alerts",
            "Guardrail-loosening alerts",
            "Unconfirmed fixes that loosen a guardrail: the doctor's alert stays open until a person confirms or reverts each.",
            &alert_rows,
        ),
    ] {
        if rows.is_empty() {
            empty.push(title);
            continue;
        }
        let _ = write!(
            body,
            r#"<section class="oml-todo-g" aria-labelledby="{id}"><h2 id="{id}">{title} · {}</h2><p class="desc">{desc}</p><ul class="oml-todo">"#,
            rows.len()
        );
        for r in rows.iter() {
            body.push_str(r);
        }
        body.push_str("</ul></section>");
    }
    body.push_str(&cleared_section(&admin_acts));
    if empty.len() == 4 {
        body.push_str(r#"<div class="oml-empty"><p>Nothing waits for this account.</p><p class="desc">Admin acts to confirm, admin fixes no person has confirmed or reverted, the alerts among them, and this key's open investigations appear here.</p></div>"#);
    } else if !empty.is_empty() {
        let _ = write!(
            body,
            r#"<p class="desc">Nothing waits in {}.</p>"#,
            empty.join(" or ")
        );
    }
    outcome::html(
        StatusCode::OK,
        shell(
            "To do",
            &session.administration(),
            ADMIN_TODO_PATH,
            None,
            &body,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cleared public flag is listed in the administration To do as
    /// done, newest first, and a set is not (ADR-0098 d6).
    #[test]
    fn a_cleared_public_flag_is_listed_as_done_and_a_set_is_not() {
        let act = |id: &str, act: &str, at: &str| {
            serde_json::from_value::<oml_sdk::AdminAct>(serde_json::json!({
                "act_id": id, "act": act, "class": "narrowing", "state": "effective",
                "target": {"workspace_id": "w"}, "grants": [], "actor_person": "bob",
                "sole_admin": false, "at": at,
            }))
            .expect("an act")
        };
        let acts = [
            act("a1", "workspace_public_set", "2026-11-27T10:00:00Z"),
            act("a2", "workspace_public_clear", "2026-11-28T10:00:00Z"),
        ];
        let html = cleared_section(&acts);
        assert!(html.contains("Done: public cleared · 1"), "{html}");
        assert!(html.contains("workspace public clear"), "{html}");
        assert!(!html.contains("workspace public set"), "{html}");
        assert!(cleared_section(&acts[..1]).is_empty());
    }

    #[test]
    fn a_crafted_notice_shows_only_numbers_and_the_pages_own_words() {
        let fields = |q: &str| Fields::of(&format!("/web/todo?{q}").parse().unwrap()).unwrap();
        assert_eq!(
            notice(&fields("done=verdict&version=3&state=promoted")).as_deref(),
            Some("Recorded: version 3 is now approved.")
        );
        assert_eq!(
            notice(&fields("done=verdict&version=3&state=%3Cb%3E")),
            None
        );
        assert_eq!(
            notice(&fields("done=empty")).as_deref(),
            Some("Nothing waits for this account.")
        );
        assert_eq!(notice(&fields("done=hello")), None);
    }

    /// A held successor as the listing serves it to a key that cannot read
    /// the version it replaces: `replaces` null, the item's own version
    /// and `retirement` still served.
    fn unreadable_successor(retirement: bool) -> Card {
        let m: Memory = serde_json::from_value(serde_json::json!({
            "artifact_id": "m2", "claim_key": "c", "version": 2, "tip": false,
            "workspace_path": "universe.alice", "state": "held", "principal": "helper",
            "created_at": "2026-11-09T08:00:00.000000Z", "text": "t",
            "retirement": retirement, "replaces": null,
        }))
        .unwrap();
        Card::of_held(&m, false)
    }

    #[test]
    fn an_edit_or_retirement_whose_replaced_version_is_unreadable_is_labelled_by_its_kind() {
        let roots = Roots::default();
        let edit = unreadable_successor(false);
        assert_eq!(edit.decision(), r#"<span class="oml-decision">Edit</span>"#);
        let line = verdict_line(&edit, "alice-review", &roots);
        assert!(line.contains("Approve serves v2"), "{line}");
        assert!(line.contains("not readable to this key"), "{line}");
        assert!(
            !line.contains("Reject keeps the text, as rejected"),
            "{line}"
        );
        let retirement = unreadable_successor(true);
        assert_eq!(
            retirement.decision(),
            r#"<span class="oml-decision">Retirement</span>"#
        );
        let line = verdict_line(&retirement, "alice-review", &roots);
        assert!(line.contains("Approve stops serving the claim"), "{line}");
        assert!(!line.contains("Approve serves it"), "{line}");
    }

    #[test]
    fn the_group_count_is_the_counts_reads_and_never_a_page_counted_past_its_end() {
        let page = |more: bool| HeldPage {
            items: Vec::new(),
            next_cursor: more.then(|| "c".to_owned()),
            lane_authors: None,
        };
        assert_eq!(
            shown_count(Some((7, false)), &page(true)).as_deref(),
            Some("7")
        );
        assert_eq!(
            shown_count(Some((1000, true)), &page(true)).as_deref(),
            Some("1000+")
        );
        assert_eq!(shown_count(None, &page(false)).as_deref(), Some("0"));
        assert_eq!(shown_count(None, &page(true)), None);
    }
}
