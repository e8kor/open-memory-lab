//! The user page's and the admin page's views and acts (ADR-0045 d9–d13,
//! d21): each view a `GET` selected by `view`, each act one `action` of the
//! page's closed set run as the session key its capacity needs. An act
//! either redirects back to its view (`303`) or answers a page that shows
//! what it produced once — a key value, a connect code, a served review
//! item — under `no-store`.

use std::collections::HashMap;
use std::fmt::Write as _;

use serde_json::json;
use uuid::Uuid;

use oml_app::app::{Capacity, OwnKey};
use oml_app::domain::Role;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::web::{Email, csrf_token};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{AppError, Bearer, WebEmailRequest, WebSessionRecord};

use super::page::{attr, form, hidden, input, select, shell, short, state_tag};
use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::{ApiState, retry_chain};
use oml_page::web::{
    ADMIN_ATLAS_PATH, ADMIN_CURATE_PATH, ADMIN_KEYS_PATH, ADMIN_MEMORIES_PATH, ADMIN_MEMORY_PATH,
    ADMIN_REVIEW_PATH, ADMIN_TABS, ADMIN_WORKAREAS_PATH, USER_ATLAS_PATH, USER_KEYS_PATH,
    USER_MEMORIES_PATH, USER_MEMORY_PATH, USER_REVIEW_PATH, USER_WORKAREAS_PATH, user_tabs,
};

const GUIDES: &str = "https://e8kor.github.io/open-memory-lab/guides";

/// What an act answers.
pub(crate) enum Outcome {
    /// `303` to this view.
    Redirect(String),
    /// A page, shown once.
    Page(String),
}

/// A form's or a query's fields, each name at most once.
pub(crate) struct Fields(HashMap<String, String>);

impl Fields {
    pub(crate) fn of(pairs: Vec<(String, String)>) -> Self {
        Self(pairs.into_iter().collect())
    }

    pub(super) fn get(&self, name: &str) -> Option<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    pub(super) fn need(&self, name: &'static str) -> Result<&str, ApiError> {
        self.get(name).ok_or_else(|| {
            ApiError::validation(format!("{name} is required"))
                .with_details(json!({ "field": name }))
        })
    }

    /// A field the action does not define is refused (ADR-0045 d6).
    pub(super) fn only(&self, allowed: &[&str]) -> Result<(), ApiError> {
        for name in self.0.keys() {
            if name != "action" && name != "csrf_token" && !allowed.contains(&name.as_str()) {
                return Err(
                    ApiError::validation(format!("this action takes no field {name}"))
                        .with_details(json!({ "field": name })),
                );
            }
        }
        Ok(())
    }

    pub(super) fn name(&self, field: &'static str) -> Result<Name, ApiError> {
        Name::parse(self.need(field)?).map_err(|e| {
            ApiError::validation(e.to_string()).with_details(json!({ "field": field }))
        })
    }

    pub(super) fn reference(&self, field: &'static str) -> Result<Reference, ApiError> {
        Reference::parse(self.need(field)?).map_err(|e| {
            ApiError::validation(e.to_string()).with_details(json!({ "field": field }))
        })
    }

    fn yes(&self, field: &str) -> bool {
        matches!(self.get(field), Some("yes" | "true" | "on"))
    }
}

fn app_error(e: AppError) -> ApiError {
    map_app_error(e, ErrorContext::Write)
}

/// A grant as a page names it: `writer on alice`.
fn grant_line(role: Role, path: &str) -> String {
    format!("{} on {}", role.as_str(), short(path))
}

/// The user page's views `oml-web` serves, each at a path of its own
/// (ADR-0070 d7 (b) as amended): the lead answers `GET /web?view=<view>`
/// with a `303` to that fixed path, and no longer renders the view.
pub const MOVED_VIEWS: &[(&str, &str)] = &[
    ("guardrails", "/web/guardrails"),
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

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    pub(super) async fn caller(
        &self,
        session: &WebSessionRecord,
        capacity: Capacity,
    ) -> Result<oml_app::app::Caller, ApiError> {
        self.app
            .web_caller(session, capacity)
            .await
            .map_err(app_error)
    }

    async fn who(&self, session: &WebSessionRecord) -> String {
        let organisation = self
            .store
            .tenant_name(session.tenant_id)
            .await
            .ok()
            .flatten();
        match organisation {
            Some(org) => format!("{} · {}", session.user.as_str(), org.as_str()),
            None => session.user.as_str().to_owned(),
        }
    }

    // -- the user page ------------------------------------------------------

    /// One view of the user page (ADR-0045 d9), as `<user>-web`.
    pub(crate) async fn user_view(
        &self,
        session: &WebSessionRecord,
        query: &Fields,
    ) -> Result<String, ApiError> {
        self.caller(session, Capacity::Web).await?;
        let (active, body) = match query.get("view").unwrap_or_default() {
            "setup" => ("/web?view=setup", self.setup_view()),
            // Every other view, the Keys view among them, is oml-web's or
            // none (ADR-0070 d7 (b)).
            _ => return Err(ApiError::not_found()),
        };
        Ok(shell(
            "Your page",
            &self.who(session).await,
            &user_tabs(session.admin),
            active,
            query.get("done"),
            &body,
        ))
    }

    /// One key's row, with the acts the page allows on it.
    async fn key_row(
        &self,
        acting: &oml_app::app::Caller,
        own: &OwnKey,
        path: &str,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let key = &own.key;
        let name = key.name.as_ref().map_or("", Name::as_str);
        let mut access = Vec::new();
        for g in own.access.iter().flat_map(|a| &a.grants) {
            let p = self
                .app
                .web_path(acting, g.grant.node)
                .await
                .map_err(app_error)?;
            access.push(grant_line(g.grant.role, &p));
        }
        let state = if key.revoked_at.is_some() {
            "revoked"
        } else if own.cut_off {
            "cut off"
        } else {
            "live"
        };
        let mut acts = String::new();
        let session_key = key.credential.as_str() == "session";
        if !session_key && state == "live" {
            acts.push_str(&form(
                path,
                "cut_off",
                csrf,
                &hidden("key", name),
                "Cut off",
                false,
            ));
        }
        Ok(format!(
            r#"<tr><td class="mono">{}</td><td>{}<br><span class="dim">{} · {}</span></td><td>{}</td><td>{}</td><td>{}</td></tr>"#,
            short(name),
            key.role.as_str(),
            key.credential.as_str(),
            key.held_by.as_str(),
            access.join("<br>"),
            state_tag(state),
            acts
        ))
    }

    fn setup_view(&self) -> String {
        // The server's own oauth.issuer is the URL an agent sets beside its
        // key; no other server is named, and there is no default
        // (ADR-0076 d6, d8). The pages need oauth.issuer to be on at all.
        let (issuer, url_line) = match self.app.oauth_issuer() {
            Some(issuer) => (
                issuer.as_str().to_owned(),
                "This server's address: give each agent this URL with its key.".to_owned(),
            ),
            None => (
                String::new(),
                "This server's address is not configured (no_server: set oauth.issuer); ask its \
                 operator for the URL to give your agents."
                    .to_owned(),
            ),
        };
        format!(
            r#"<h1>Set up</h1><div class="card"><p class="label">Server</p><pre class="mono">{}</pre></div><p class="desc">{url_line}</p><h2>Claude Code</h2><p>Agent key · <a href="{GUIDES}/claude-code/">guide</a></p><h2>Claude on the web and on a phone</h2><p>Connector key · <a href="{GUIDES}/claude-ai-and-mobile/">guide</a></p><h2>Other agent tools</h2><p>Codex CLI, Cursor, Gemini CLI, VS Code, Claude Desktop · <a href="{GUIDES}/other-harnesses/">guide</a></p>"#,
            attr(&issuer)
        )
    }

    /// One act of the user page (ADR-0045 d9): every act it had is a view's
    /// that `oml-web` serves, so the lead runs none (ADR-0070 d7 (b)).
    pub(crate) fn user_page_act(action: &str) -> Result<Outcome, ApiError> {
        match action {
            // An act of a view oml-web serves: run nothing, and send the
            // person to the view (ADR-0070 d7 (b)).
            action if moved_action(action, false).is_some() => Ok(Outcome::Redirect(
                moved_action(action, false)
                    .unwrap_or(USER_KEYS_PATH)
                    .to_owned(),
            )),
            _ => Err(
                ApiError::validation("action is not one of this page's actions")
                    .with_details(json!({"field": "action"})),
            ),
        }
    }

    // -- the admin page -----------------------------------------------------

    /// One view of the admin page (ADR-0045 d10, d21), each through the
    /// capacity it needs.
    pub(crate) async fn admin_view(
        &self,
        token: &str,
        session: &WebSessionRecord,
        query: &Fields,
    ) -> Result<String, ApiError> {
        let admin = self.caller(session, Capacity::Admin).await?;
        let csrf = csrf_token(token);
        let (active, body) = match query.get("view").unwrap_or("people") {
            // A view oml-web serves: the proxy routes it there (ADR-0070
            // d7 (b)).
            view if MOVED_ADMIN_VIEWS.iter().any(|(v, _)| *v == view) => {
                return Err(ApiError::not_found());
            }
            _ => ("/web/admin", self.people_view(&admin, &csrf).await?),
        };
        Ok(shell(
            "Administration",
            &self.who(session).await,
            &ADMIN_TABS,
            active,
            query.get("done"),
            &body,
        ))
    }

    async fn people_view(
        &self,
        admin: &oml_app::app::Caller,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let entries = self.app.web_all_entries(admin).await.map_err(app_error)?;
        let mut body = String::from(
            r#"<h1>People</h1><p class="desc">Removing an address ends its sessions; its agent keys keep working.</p><table><tr><th>Email</th><th>User</th><th>Admin</th><th></th></tr>"#,
        );
        for e in &entries {
            let flip = if e.admin { "no" } else { "yes" };
            let _ = write!(
                body,
                r#"<tr><td class="mono">{}</td><td class="mono">{}</td><td>{}</td><td>{}{}</td></tr>"#,
                short(e.email.as_str()),
                short(e.user.as_str()),
                if e.admin {
                    state_tag("admin")
                } else {
                    String::new()
                },
                form(
                    "/web/admin",
                    "set_admin",
                    csrf,
                    &format!(
                        "{}{}",
                        hidden("email", e.email.as_str()),
                        hidden("admin", flip)
                    ),
                    if e.admin {
                        "Remove admin"
                    } else {
                        "Make admin"
                    },
                    false
                ),
                form(
                    "/web/admin",
                    "remove_email",
                    csrf,
                    &hidden("email", e.email.as_str()),
                    "Remove",
                    false
                )
            );
        }
        body.push_str("</table><h2>Address</h2>");
        body.push_str(&form(
            "/web/admin",
            "allow_email",
            csrf,
            &format!(
                "{}{}{}",
                input("email", "email", "", true),
                input("user", "user", "", true),
                select("admin", "admin", &[("no", "no"), ("yes", "yes")])
            ),
            "Add",
            true,
        ));
        body.push_str("<h2>Review and curation</h2><details class=\"help\"><summary>?</summary>Each person reviews their own workarea from their first sign-in. Grant review elsewhere, or make someone a curator of the shared area.</details>");
        body.push_str(&form(
            "/web/admin",
            "grant_review",
            csrf,
            &format!(
                "{}{}",
                input("user", "user", "", true),
                input("workarea", "workarea", "", true)
            ),
            "Grant review",
            false,
        ));
        body.push_str(&form(
            "/web/admin",
            "grant_curation",
            csrf,
            &format!(
                "{}{}",
                input("user", "user", "", true),
                input("workarea", "reads from workarea", "", true)
            ),
            "Designate curator",
            false,
        ));
        Ok(body)
    }

    /// One act of the admin page (ADR-0045 d10–d13): the list and the
    /// capacities as `<user>-admin`, review as `<user>-review`, curation as
    /// `<user>-curate`, supersession as the curator for its own universe
    /// claims and as the reviewer for any other.
    pub(crate) async fn admin_page_act(
        &self,
        token: &str,
        session: &WebSessionRecord,
        action: &str,
        fields: &Fields,
    ) -> Result<Outcome, ApiError> {
        match action {
            // An act of a view oml-web serves: run nothing, and send the
            // person to the view (ADR-0070 d7 (b)).
            action if moved_action(action, true).is_some() => {
                return Ok(Outcome::Redirect(
                    moved_action(action, true)
                        .unwrap_or(ADMIN_REVIEW_PATH)
                        .to_owned(),
                ));
            }
            _ => {}
        }
        let admin = self.caller(session, Capacity::Admin).await?;
        match action {
            "allow_email" => {
                fields.only(&["email", "user", "admin"])?;
                let req = WebEmailRequest {
                    email: email_field(fields)?,
                    user: fields.name("user")?,
                    admin: fields.yes("admin"),
                };
                retry_chain(|| self.app.allow_web_email(Bearer::Session(&admin), &req))
                    .await
                    .map_err(app_error)?;
                Ok(Outcome::Redirect("/web/admin?done=added".to_owned()))
            }
            "set_admin" => {
                fields.only(&["email", "admin"])?;
                let email = email_field(fields)?;
                let entry = self
                    .app
                    .web_all_entries(&admin)
                    .await
                    .map_err(app_error)?
                    .into_iter()
                    .find(|e| e.email == email)
                    .ok_or_else(ApiError::not_found)?;
                let flag = fields.yes("admin");
                if entry.admin && !flag {
                    self.require_other_admin(&admin, &email).await?;
                }
                let req = WebEmailRequest {
                    email,
                    user: entry.user,
                    admin: flag,
                };
                retry_chain(|| self.app.allow_web_email(Bearer::Session(&admin), &req))
                    .await
                    .map_err(app_error)?;
                Ok(Outcome::Redirect("/web/admin?done=changed".to_owned()))
            }
            "remove_email" => {
                fields.only(&["email", "confirm"])?;
                let email = email_field(fields)?;
                let entries = self.app.web_all_entries(&admin).await.map_err(app_error)?;
                let entry = entries
                    .iter()
                    .find(|e| e.email == email)
                    .ok_or_else(ApiError::not_found)?;
                if entry.admin {
                    self.require_other_admin(&admin, &email).await?;
                }
                if !fields.yes("confirm") {
                    let last = !entries
                        .iter()
                        .any(|e| e.user == entry.user && e.email != email);
                    let body = self
                        .removal_confirmation(&admin, token, entry.user.clone(), &email, last)
                        .await?;
                    return Ok(Outcome::Page(shell(
                        "Remove an address",
                        &self.who(session).await,
                        &ADMIN_TABS,
                        "/web/admin",
                        None,
                        &body,
                    )));
                }
                retry_chain(|| self.app.remove_web_email(Bearer::Session(&admin), &email))
                    .await
                    .map_err(app_error)?;
                Ok(Outcome::Redirect("/web/admin?done=removed".to_owned()))
            }
            "grant_review" | "grant_curation" => {
                fields.only(&["user", "workarea"])?;
                let user = fields.name("user")?;
                let workarea = fields.reference("workarea")?;
                if action == "grant_review" {
                    retry_chain(|| self.app.web_grant_review(&admin, &user, workarea.clone()))
                        .await
                        .map_err(app_error)?;
                } else {
                    retry_chain(|| self.app.web_grant_curation(&admin, &user, workarea.clone()))
                        .await
                        .map_err(app_error)?;
                }
                Ok(Outcome::Redirect("/web/admin?done=granted".to_owned()))
            }
            _ => Err(
                ApiError::validation("action is not one of this page's actions")
                    .with_details(json!({"field": "action"})),
            ),
        }
    }

    /// The last admin entry is never removed or un-flagged from the page,
    /// which could then not be reopened; the CLI still can (ADR-0045 d10).
    async fn require_other_admin(
        &self,
        admin: &oml_app::app::Caller,
        email: &Email,
    ) -> Result<(), ApiError> {
        if self
            .app
            .web_other_admin(admin, &email.digest())
            .await
            .map_err(app_error)?
        {
            Ok(())
        } else {
            Err(ApiError::validation(
                "this is the organisation's last admin address; add another admin first",
            )
            .with_details(json!({"field": "admin"})))
        }
    }

    /// The removal confirmation (ADR-0045 d13): when it is the user's last
    /// address, their agents' keys with a cut-off beside each, since
    /// removing an address cuts off none of them.
    async fn removal_confirmation(
        &self,
        admin: &oml_app::app::Caller,
        token: &str,
        user: Name,
        email: &Email,
        last: bool,
    ) -> Result<String, ApiError> {
        let csrf = csrf_token(token);
        let mut body = format!(
            r#"<h1>Remove {}?</h1><p class="desc">Sessions end now. <code>{}</code>'s agent keys keep working; cut them off below.</p>"#,
            short(email.as_str()),
            short(user.as_str())
        );
        if last {
            let keys = self
                .app
                .web_keys_of_user(admin, &user)
                .await
                .map_err(app_error)?;
            body.push_str(
                "<table><tr><th>Key</th><th>Role</th><th>Access</th><th>State</th><th></th></tr>",
            );
            for own in &keys {
                body.push_str(&self.key_row(admin, own, ADMIN_KEYS_PATH, &csrf).await?);
            }
            body.push_str("</table>");
        } else {
            body.push_str(
                "<p>The user keeps another address on the list, so they still sign in.</p>",
            );
        }
        body.push_str(&form(
            "/web/admin",
            "remove_email",
            &csrf,
            &format!(
                "{}{}",
                hidden("email", email.as_str()),
                hidden("confirm", "yes")
            ),
            "Remove the address",
            true,
        ));
        body.push_str(r#"<p><a href="/web/admin">Keep it</a></p>"#);
        Ok(body)
    }
}

fn email_field(fields: &Fields) -> Result<Email, ApiError> {
    Email::parse(fields.need("email")?)
        .map_err(|e| ApiError::validation(e.to_string()).with_details(json!({"field": "email"})))
}
