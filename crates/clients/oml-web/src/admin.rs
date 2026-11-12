//! The admin page's overview and its users page (ADR-0045 d10, d13;
//! ADR-0068 d5, d6, d8), moved from the lead's `GET /web/admin` and
//! `/web/admin/users` (ADR-0070 d7 (b)):
//!
//! - `/web/admin/people`: the allowlist (`listWebEmails`), adding or
//!   changing an entry and its admin flag (`allowWebEmail`), removing one
//!   (`removeWebEmail`), both refusing to leave the organisation without an
//!   admin entry (`keep_an_admin`), and a person's review or curation
//!   capacity on a workarea (`grantWebCapacities`);
//! - `/web/admin/users`: each person's quotas (`listWebUsers`,
//!   `setWebUserQuota`), disabling a person (`disableWebUser`) and closing
//!   a deletion request (`closeWebDeletion`).
//!
//! Every act runs as the session's admin key under the form's nonce, and
//! the API replays a resubmission. The destructive acts, removing an
//! address, disabling a person and closing a deletion request, are
//! confirmed first: the first post records nothing and answers the
//! confirmation, and only its form, carrying `confirm=yes`, acts.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{ADMIN_TABS, form, hidden, input, select, shell, short, state_tag};
use oml_sdk::WebEmail;

use crate::pages::{Fields, WalkEnd, refused, walk};
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, acts, keys, outcome, usage};

/// The admin overview: the allowlist and the capacities.
pub const PEOPLE_PATH: &str = "/web/admin/people";
/// The users page: quotas, disabling, deletion requests.
pub const USERS_PATH: &str = "/web/admin/users";

/// The overview's acts (ADR-0045 d10, d13).
pub const PEOPLE_ACTIONS: &[&str] = &[
    "allow_email",
    "remove_email",
    "set_admin",
    "grant_review",
    "grant_curation",
];

/// The users page's acts (ADR-0068 d5, d6, d8).
pub const USERS_ACTIONS: &[&str] = &["set_quota", "disable_user", "close_deletion"];

/// The quotas a person has (ADR-0068 d5).
const QUOTAS: [&str; 4] = ["memories", "keys", "connections", "requests_per_minute"];

/// The allowlist's page size, and how many pages the overview reads.
const EMAIL_PAGE: u32 = 200;
const EMAIL_PAGES_MAX: usize = 50;

/// Any session but an admin entry's answers `access.not_found` (ADR-0045
/// d10).
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

fn page(session: &Session, title: &str, active: &str, note: Option<&str>, body: &str) -> Response {
    outcome::html(
        StatusCode::OK,
        shell(title, &session.who(), &ADMIN_TABS, active, note, body),
    )
}

/// The whole allowlist, or a refusal, so the list is never shown cut
/// short: `limit.list_too_long` past the pages the overview reads, and at
/// a cursor the API repeats, the API's fault, `unavailable` with 502, as
/// the key list answers (#293).
async fn entries(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
) -> Result<Vec<WebEmail>, acts::Refusal> {
    let credential = &session.credential(Capacity::Admin);
    walk(EMAIL_PAGES_MAX, |cursor| async move {
        let ids = calls.next_ids();
        let page = web
            .api
            .list_web_emails(&call(credential, &ids), EMAIL_PAGE, cursor.as_deref())
            .await?;
        Ok((page.items, page.next_cursor))
    })
    .await
    .map_err(|end| {
        Box::new(match end {
            WalkEnd::Failed(e) => outcome::page(&e, calls.page_id(), PEOPLE_PATH),
            WalkEnd::Repeated => refused(
                StatusCode::BAD_GATEWAY,
                "unavailable",
                "the server repeated a cursor of the allowlist, so the list cannot be read whole; try again",
                calls,
            ),
            WalkEnd::TooLong => refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit.list_too_long",
                "the allowlist is longer than this page reads; use the API or the command line",
                calls,
            ),
        })
    })
}

fn people_notice(fields: &Fields) -> Option<&'static str> {
    match fields.get("done")? {
        "added" => Some("Added."),
        "changed" => Some("Changed."),
        "removed" => Some("Removed: its sessions ended; its agent keys keep working."),
        "granted" => Some("Granted."),
        _ => None,
    }
}

/// `GET /web/admin/people`: the allowlist and the capacity forms.
pub async fn people_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let listed = match entries(&web, &session, &calls).await {
        Ok(listed) => listed,
        Err(response) => return *response,
    };
    let csrf = session.csrf();
    let mut body = String::from(
        r#"<h1>People</h1><p class="desc">Removing an address ends its sessions; its agent keys keep working.</p><table><tr><th>Email</th><th>User</th><th>Admin</th><th></th></tr>"#,
    );
    for e in &listed {
        let flip = if e.admin { "no" } else { "yes" };
        let _ = write!(
            body,
            r#"<tr><td class="mono">{}</td><td class="mono">{}</td><td>{}</td><td>{}{}</td></tr>"#,
            short(&e.email),
            short(&e.user),
            if e.admin {
                state_tag("admin")
            } else {
                String::new()
            },
            form(
                PEOPLE_PATH,
                "set_admin",
                &csrf,
                &format!(
                    "{}{}{}",
                    hidden("email", &e.email),
                    hidden("admin", flip),
                    acts::nonce_field()
                ),
                if e.admin {
                    "Remove admin"
                } else {
                    "Make admin"
                },
                false
            ),
            form(
                PEOPLE_PATH,
                "remove_email",
                &csrf,
                &hidden("email", &e.email),
                "Remove",
                false
            )
        );
    }
    body.push_str("</table><h2>Address</h2>");
    body.push_str(&form(
        PEOPLE_PATH,
        "allow_email",
        &csrf,
        &format!(
            "{}{}{}{}",
            input("email", "email", "", true),
            input("user", "user", "", true),
            select("admin", "admin", &[("no", "no"), ("yes", "yes")]),
            acts::nonce_field()
        ),
        "Add",
        true,
    ));
    body.push_str("<h2>Review and curation</h2><details class=\"help\"><summary>?</summary>Each person reviews their own workarea from their first sign-in. Grant review elsewhere, or make someone a curator of the shared area. A curator designation takes effect at the person's next sign-in.</details>");
    for (action, label, button) in [
        ("grant_review", "workarea", "Grant review"),
        ("grant_curation", "reads from workarea", "Designate curator"),
    ] {
        body.push_str(&form(
            PEOPLE_PATH,
            action,
            &csrf,
            &format!(
                "{}{}{}",
                input("user", "user", "", true),
                input("workarea", label, "", true),
                acts::nonce_field()
            ),
            button,
            false,
        ));
    }
    page(
        &session,
        "Administration",
        PEOPLE_PATH,
        people_notice(&fields),
        &body,
    )
}

fn yes(fields: &Fields, name: &str) -> bool {
    matches!(fields.given(name), Some("yes" | "true" | "on"))
}

/// The removal confirmation (ADR-0045 d13), which records nothing: when it
/// is the user's last address, their agents' keys with a cut-off beside
/// each, since removing an address cuts off none of them.
async fn removal_confirmation(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    entry: &WebEmail,
    last: bool,
) -> Response {
    let csrf = session.csrf();
    let mut body = format!(
        r#"<h1>Remove {}?</h1><p class="desc">Sessions end now. <code>{}</code>'s agent keys keep working; cut them off below.</p>"#,
        short(&entry.email),
        short(&entry.user)
    );
    if last {
        let credential = session.credential(Capacity::Admin);
        // The whole list or none of it: a key left out here would keep
        // working with no cut-off offered (#308).
        let listed = match keys::keys(web, &credential, calls, Some(&entry.user), PEOPLE_PATH).await
        {
            Ok(listed) => listed,
            Err(response) => return *response,
        };
        let agents: Vec<_> = listed
            .into_iter()
            .filter(|k| k.credential != "session")
            .collect();
        body.push_str(&keys::table(&agents, keys::Side::Admin, &csrf, &entry.user));
    } else {
        body.push_str("<p>The user keeps another address on the list, so they still sign in.</p>");
    }
    body.push_str(&form(
        PEOPLE_PATH,
        "remove_email",
        &csrf,
        &format!(
            "{}{}{}",
            hidden("email", &entry.email),
            hidden("confirm", "yes"),
            acts::nonce_field()
        ),
        "Remove the address",
        true,
    ));
    let _ = write!(body, r#"<p><a href="{PEOPLE_PATH}">Keep it</a></p>"#);
    page(session, "Remove an address", PEOPLE_PATH, None, &body)
}

fn people_defined(action: &str, name: &str) -> bool {
    match action {
        "allow_email" => matches!(name, "email" | "user" | "admin"),
        "set_admin" => matches!(name, "email" | "admin"),
        "remove_email" => matches!(name, "email" | "confirm"),
        _ => matches!(name, "user" | "workarea"),
    }
}

/// `POST /web/admin/people`: an act of the overview, as the admin key.
pub async fn people_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let (fields, action) = match acts::read(form, PEOPLE_ACTIONS, people_defined, &calls) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let required: &[&str] = match action {
        "allow_email" => &["email", "user"],
        "set_admin" | "remove_email" => &["email"],
        _ => &["user", "workarea"],
    };
    if let Some(missing) = required.iter().find(|n| fields.given(n).is_none()) {
        return acts::invalid(&format!("{missing} is required"), &calls);
    }
    let credential = session.credential(Capacity::Admin);
    let email = fields.given("email").unwrap_or_default().trim();
    // The entry an act on an existing address names, read before any key is
    // spent; the confirmation records nothing and draws its own nonce.
    let entry = if matches!(action, "set_admin" | "remove_email") {
        let listed = match entries(&web, &session, &calls).await {
            Ok(listed) => listed,
            Err(response) => return *response,
        };
        let found = listed
            .iter()
            .find(|e| e.email.eq_ignore_ascii_case(email))
            .cloned();
        // A confirmed removal goes to the API whether or not the address is
        // still listed: a resubmission replays the recorded removal there.
        let found = match found {
            None if action == "remove_email" && yes(&fields, "confirm") => Some(WebEmail {
                email: email.to_owned(),
                user: String::new(),
                admin: false,
            }),
            found => found,
        };
        let Some(entry) = found else {
            return refused(
                StatusCode::NOT_FOUND,
                "access.not_found",
                "no such address on the list",
                &calls,
            );
        };
        if action == "remove_email" && !yes(&fields, "confirm") {
            let last = !listed
                .iter()
                .any(|e| e.user == entry.user && !e.email.eq_ignore_ascii_case(email));
            return removal_confirmation(&web, &session, &calls, &entry, last).await;
        }
        Some(entry)
    } else {
        None
    };
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let ids = calls.next_ids();
    let call = call(&credential, &ids);
    let done = match action {
        "allow_email" => web
            .api
            .allow_web_email(
                &call,
                &key,
                email,
                fields.given("user").unwrap_or_default().trim(),
                yes(&fields, "admin"),
            )
            .await
            .map(|_| "added"),
        "set_admin" => {
            let entry = entry.expect("read above");
            web.api
                .allow_web_email(
                    &call,
                    &key,
                    &entry.email,
                    &entry.user,
                    yes(&fields, "admin"),
                )
                .await
                .map(|_| "changed")
        }
        "remove_email" => {
            let entry = entry.expect("read above");
            web.api
                .remove_web_email(&call, &key, &entry.email)
                .await
                .map(|_| "removed")
        }
        _ => {
            let capacity = if action == "grant_review" {
                "review"
            } else {
                "curate"
            };
            web.api
                .grant_web_capacity(
                    &call,
                    &key,
                    fields.given("user").unwrap_or_default().trim(),
                    capacity,
                    fields.given("workarea").unwrap_or_default().trim(),
                )
                .await
                .map(|_| "granted")
        }
    };
    match done {
        Ok(done) => acts::see_other(&format!("{PEOPLE_PATH}?done={done}"), &[]),
        Err(e) => outcome::page(&e, calls.page_id(), PEOPLE_PATH),
    }
}

fn users_notice(fields: &Fields) -> Option<&'static str> {
    match fields.get("done")? {
        "set" => Some("The quota is set."),
        "disabled" => Some("Disabled: their sessions ended and their keys are cut off."),
        "closed" => Some("Closed: the person is disabled and their keys are cut off."),
        _ => None,
    }
}

/// `GET /web/admin/users`: each person against their quotas, and the
/// pending deletion requests.
pub async fn users_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let credential = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    let listed = match web.api.list_web_users(&call(&credential, &ids)).await {
        Ok(listed) => listed,
        Err(e) => return outcome::page(&e, calls.page_id(), USERS_PATH),
    };
    let csrf = session.csrf();
    let mut body = String::from(
        r#"<p class="label">Administration</p><h1>Users</h1><p class="lede">What each person holds against their quotas. Raising a quota, disabling a person and closing a deletion request are journalled under your admin key.</p><h2>Deletion requests</h2>"#,
    );
    if listed.deletions.is_empty() {
        body.push_str(r#"<p class="dim">None pending.</p>"#);
    } else {
        body.push_str("<table><tr><th>user</th><th>requested</th><th></th></tr>");
        for d in &listed.deletions {
            let _ = write!(
                body,
                r#"<tr><td class="mono">{}</td><td class="mono">{}</td><td>{}</td></tr>"#,
                short(&d.user),
                short(&d.requested_at),
                form(
                    USERS_PATH,
                    "close_deletion",
                    &csrf,
                    &hidden("user", &d.user),
                    "Disable and close",
                    false
                )
            );
        }
        body.push_str("</table><p class=\"dim\">Closing disables the person and cuts off their keys; their memories stay until they are erased with <code>oml erase</code>.</p>");
    }
    body.push_str("<h2>People</h2><p class=\"dim\">Each count against its limit; a mark shows at 80% of a limit and at the limit. Writes count this UTC month.</p><table><tr><th>user</th><th>memories</th><th>text stored</th><th>writes</th><th>keys</th><th>connections</th><th>requests/min</th><th></th></tr>");
    for u in &listed.users {
        let flags = match (u.signed_up, u.disabled) {
            (_, true) => r#" <span class="tag dim">disabled</span>"#,
            (true, false) => r#" <span class="tag ok">signed up</span>"#,
            _ => "",
        };
        let disable = if u.disabled {
            String::new()
        } else {
            form(
                USERS_PATH,
                "disable_user",
                &csrf,
                &hidden("user", &u.user),
                "Disable",
                false,
            )
        };
        let [memories, text_stored, writes, key_count, connections] =
            usage::cells(&u.usage, &u.quotas);
        let _ = write!(
            body,
            r#"<tr><td class="mono">{}{flags}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{disable}</td></tr>"#,
            short(&u.user),
            memories,
            text_stored,
            writes,
            key_count,
            connections,
            u.quotas.requests_per_minute,
        );
    }
    body.push_str("</table>");
    let options: Vec<(&str, &str)> = QUOTAS.iter().map(|q| (*q, *q)).collect();
    body.push_str("<h2>Set a quota</h2>");
    body.push_str(&form(
        USERS_PATH,
        "set_quota",
        &csrf,
        &format!(
            "{}{}{}{}",
            input("user", "user", "", true),
            select("quota", "quota", &options),
            input("limit", "limit, empty for the default", "", false),
            acts::nonce_field()
        ),
        "Set",
        true,
    ));
    page(&session, "Quotas", USERS_PATH, users_notice(&fields), &body)
}

/// The confirmation of disabling a person or closing their request, which
/// records nothing.
fn user_confirmation(session: &Session, action: &str, user: &str) -> Response {
    let (title, consequence, button) = if action == "disable_user" {
        (
            "Disable",
            "Their sessions end and every agent key of theirs is cut off. Their memories stay.",
            "Disable the person",
        )
    } else {
        (
            "Close the deletion request of",
            "The person is disabled, their sessions end and every agent key is cut off; their memories stay until the operator erases them.",
            "Disable and close",
        )
    };
    let mut body = format!(
        r#"<h1>{title} {}?</h1><p class="oml-consequence">{consequence}</p>"#,
        short(user)
    );
    body.push_str(&form(
        USERS_PATH,
        action,
        &session.csrf(),
        &format!(
            "{}{}{}",
            hidden("user", user),
            hidden("confirm", "yes"),
            acts::nonce_field()
        ),
        button,
        true,
    ));
    let _ = write!(body, r#"<p><a href="{USERS_PATH}">Keep them</a></p>"#);
    page(session, "Quotas", USERS_PATH, None, &body)
}

/// Whether `user` is a name under the creation grammar, the API's own.
fn is_user(user: &str) -> bool {
    let b = user.as_bytes();
    !b.is_empty()
        && b.len() <= 56
        && b[0].is_ascii_lowercase()
        && b.last()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// `POST /web/admin/users`: a quota, or a confirmed disable or close.
pub async fn users_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let (fields, action) = match acts::read(
        form,
        USERS_ACTIONS,
        |a, n| match a {
            "set_quota" => matches!(n, "user" | "quota" | "limit"),
            _ => matches!(n, "user" | "confirm"),
        },
        &calls,
    ) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let Some(user) = fields.given("user").map(str::trim).filter(|u| is_user(u)) else {
        return acts::invalid("user names a person", &calls);
    };
    // Disabling and closing are confirmed first, and fail closed: nothing
    // acts without the confirmation's own field.
    if action != "set_quota" && !yes(&fields, "confirm") {
        return user_confirmation(&session, action, user);
    }
    let quota = if action == "set_quota" {
        let Some(quota) = fields.given("quota").filter(|q| QUOTAS.contains(q)) else {
            return acts::invalid(
                "quota is one of memories, keys, connections, requests_per_minute",
                &calls,
            );
        };
        let limit = match fields
            .given("limit")
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            None => None,
            Some(l) => match l.parse::<u64>() {
                Ok(l) => Some(l),
                Err(_) => {
                    return acts::invalid(
                        "a quota's limit is a whole number from 0 to 1000000",
                        &calls,
                    );
                }
            },
        };
        Some((quota, limit))
    } else {
        None
    };
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    let call = call(&credential, &ids);
    let done = match (action, quota) {
        ("set_quota", Some((quota, limit))) => web
            .api
            .set_web_user_quota(&call, &key, user, quota, limit)
            .await
            .map(|_| "set"),
        ("disable_user", _) => web
            .api
            .disable_web_user(&call, &key, user)
            .await
            .map(|_| "disabled"),
        _ => web
            .api
            .close_web_deletion(&call, &key, user)
            .await
            .map(|_| "closed"),
    };
    match done {
        Ok(done) => acts::see_other(&format!("{USERS_PATH}?done={done}"), &[]),
        Err(e) => outcome::page(&e, calls.page_id(), USERS_PATH),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_field_is_a_name_under_the_creation_grammar() {
        assert!(is_user("alice") && is_user("a-1"));
        for bad in ["", "Alice", "-a", "a-", "a/b", "a b"] {
            assert!(!is_user(bad), "{bad}");
        }
    }
}
