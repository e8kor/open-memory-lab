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

use oml_app::app::{AdminMemory, Capacity, OwnKey};
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::web::{Email, csrf_token};
use oml_app::domain::{MemoryKind, Role};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{
    AppError, Bearer, MemoryRecord, ServedMemory, SubmitRequest, SubmitVerb, SupersedeRequest,
    WebEmailRequest, WebSessionRecord,
};

use super::page::{attr, form, hidden, input, select, shell, short, state_tag, text, textarea};
use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::{ApiState, retry_chain};
use oml_page::web::{
    ADMIN_ATLAS_PATH, ADMIN_KEYS_PATH, ADMIN_MEMORIES_PATH, ADMIN_REVIEW_PATH, ADMIN_TABS,
    ADMIN_WORKAREAS_PATH, USER_ATLAS_PATH, USER_KEYS_PATH, USER_MEMORIES_PATH, USER_REVIEW_PATH,
    USER_WORKAREAS_PATH, user_tabs,
};

/// The public server the plugins and the agent kit connect to unless told
/// otherwise (ADR-0043).
pub const DEFAULT_SERVER: &str = "https://memory.doublegate.io";

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

    pub(super) fn id(&self, field: &'static str) -> Result<Uuid, ApiError> {
        let value = self.need(field)?;
        let value = value.strip_prefix("srv_").unwrap_or(value);
        Uuid::parse_str(value).map_err(|_| {
            ApiError::validation(format!("{field} is not an id"))
                .with_details(json!({ "field": field }))
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
        _ => return None,
    })
}

/// The fixed path the Memories list or the Atlas lives at, for a query
/// whose one `view` is `memories` and that opens no memory by its id: the
/// Atlas where `display` is `atlas`, else the list (ADR-0070 d7 (b)). One
/// memory opened by its id stays the lead's view.
#[must_use]
pub fn moved_memories(query: &str, admin: bool) -> Option<&'static str> {
    let pairs = crate::oauth::form_pairs(query.as_bytes()).ok()?;
    let one = |name: &str| {
        let mut values = pairs.iter().filter(|(n, _)| n == name).map(|(_, v)| v);
        match (values.next(), values.next()) {
            (Some(v), None) => Some(v.as_str()),
            _ => None,
        }
    };
    if one("view") != Some("memories") || pairs.iter().any(|(n, v)| n == "memory" && !v.is_empty())
    {
        return None;
    }
    let atlas = pairs.iter().any(|(n, v)| n == "display" && v == "atlas");
    Some(match (admin, atlas) {
        (false, false) => USER_MEMORIES_PATH,
        (false, true) => USER_ATLAS_PATH,
        (true, false) => ADMIN_MEMORIES_PATH,
        (true, true) => ADMIN_ATLAS_PATH,
    })
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
        token: &str,
        session: &WebSessionRecord,
        query: &Fields,
    ) -> Result<String, ApiError> {
        let web = self.caller(session, Capacity::Web).await?;
        let csrf = csrf_token(token);
        let (active, body) = match query.get("view").unwrap_or_default() {
            "setup" => ("/web?view=setup", self.setup_view()),
            // One memory opened by its id; the list and the Atlas are
            // oml-web's (ADR-0070 d7 (b)).
            "memories" => (
                USER_MEMORIES_PATH,
                self.memories_view(&web, &csrf, query).await?,
            ),
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
        let issuer = self
            .app
            .oauth_issuer()
            .map_or(DEFAULT_SERVER, |i| i.as_str())
            .to_owned();
        let url_line = if issuer == DEFAULT_SERVER {
            "The plugins' default server: an agent needs only its key.".to_owned()
        } else {
            format!(
                "Not the plugins' default (<code>{DEFAULT_SERVER}</code>): give each agent this URL with its key."
            )
        };
        format!(
            r#"<h1>Set up</h1><div class="card"><p class="label">Server</p><pre class="mono">{}</pre></div><p class="desc">{url_line}</p><h2>Claude Code</h2><p>Agent key · <a href="{GUIDES}/claude-code/">guide</a></p><h2>Claude on the web and on a phone</h2><p>Connector key · <a href="{GUIDES}/claude-ai-and-mobile/">guide</a></p><h2>Other agent tools</h2><p>Codex CLI, Cursor, Gemini CLI, VS Code, Claude Desktop · <a href="{GUIDES}/other-harnesses/">guide</a></p>"#,
            attr(&issuer)
        )
    }

    /// One memory, opened by its id from the list `oml-web` serves.
    async fn memories_view(
        &self,
        web: &oml_app::app::Caller,
        csrf: &str,
        query: &Fields,
    ) -> Result<String, ApiError> {
        let bearer = Bearer::Session(web);
        let mut body = format!(
            r#"<p class="label">Memories</p><h1>One memory</h1><p><a href="{USER_MEMORIES_PATH}">Back to the list</a></p>"#,
        );
        let id = Uuid::parse_str(query.need("memory")?).map_err(|_| {
            ApiError::validation("memory is not an id").with_details(json!({"field": "memory"}))
        })?;
        let served = self.app.get_memory(bearer, id).await.map_err(app_error)?;
        body.push_str(&memory_card(&served, &self.frame_for(csrf, &served.memory)));
        Ok(body)
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
            "memories" => (
                ADMIN_MEMORIES_PATH,
                self.admin_memories_view(&admin, query, &csrf).await?,
            ),
            // A view oml-web serves: the proxy routes it there (ADR-0070
            // d7 (b)).
            view if MOVED_ADMIN_VIEWS.iter().any(|(v, _)| *v == view) => {
                return Err(ApiError::not_found());
            }
            "curate" => (
                "/web/admin?view=curate",
                self.curate_view(&admin, query, &csrf).await?,
            ),
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

    /// One memory's attribution, opened by its id from the list `oml-web`
    /// serves.
    async fn admin_memories_view(
        &self,
        admin: &oml_app::app::Caller,
        query: &Fields,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let id = query.need("memory")?;
        let mut body = format!(
            r#"<p class="label">Memories</p><h1>One memory</h1><p><a href="{ADMIN_MEMORIES_PATH}">Back to the list</a></p>"#,
        );
        let id = Uuid::parse_str(id).map_err(|_| {
            ApiError::validation("memory is not an id").with_details(json!({"field": "memory"}))
        })?;
        let read = retry_chain(|| self.app.admin_read(Bearer::Session(admin), id))
            .await
            .map_err(app_error)?;
        body.push_str(&attribution_card(
            &read,
            &self.frame_for(csrf, &read.memory),
        ));
        body.push_str("<h2>Replace it</h2>");
        body.push_str(&form(
            "/web/admin",
            "supersede",
            csrf,
            &format!(
                "{}{}{}{}",
                hidden("memory", &read.memory.memory_id.to_string()),
                // The version this page rendered: the edit is refused
                // if the claim moved on since (ADR-0057 d19).
                hidden("expected_version", &read.memory.version.to_string()),
                textarea("text", "new text", &read.memory.body),
                select(
                    "retirement",
                    "retire instead",
                    &[("no", "no"), ("yes", "yes")]
                )
            ),
            "Propose the new version",
            false,
        ));
        let _ = write!(
            body,
            r#"<p><a href="/web/admin?view=curate&amp;memory={}">Curate</a></p>"#,
            read.memory.memory_id
        );
        Ok(body)
    }

    async fn curate_view(
        &self,
        admin: &oml_app::app::Caller,
        query: &Fields,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let mut source = String::new();
        let mut prefill = String::new();
        if let Some(id) = query.get("memory")
            && let Ok(id) = Uuid::parse_str(id)
        {
            let read = retry_chain(|| self.app.admin_read(Bearer::Session(admin), id))
                .await
                .map_err(app_error)?;
            source = id.to_string();
            prefill = read.memory.body;
        }
        let mut body = String::from(
            r#"<h1>Curate</h1><p class="desc">The copy is held until someone outside its lineage reviews it, then served without its author.</p><details class="help"><summary>?</summary>Curate only an approved memory. Reword it: a copy repeating its source's text or name can be linked to it.</details>"#,
        );
        body.push_str(&form(
            "/web/admin",
            "curate",
            csrf,
            &format!(
                "{}{}{}",
                input("source", "source memory id", &source, true),
                input("claim_key", "claim key (optional)", "", false),
                textarea("text", "text", &prefill)
            ),
            "Curate",
            true,
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
            "curate" => {
                fields.only(&["source", "text", "claim_key"])?;
                let source = fields.id("source")?;
                let text_value = fields.need("text")?.to_owned();
                let curate = self.caller(session, Capacity::Curate).await?;
                let digest = oml_app::domain::Sha256Hash::digest(
                    format!("{source}\n{text_value}").as_bytes(),
                );
                let req = SubmitRequest {
                    on_similar: Default::default(),
                    idempotency_key: format!("web-curate-{}", &digest.to_prefixed()[7..39]),
                    claim_key: fields.get("claim_key").map(ToOwned::to_owned),
                    verb: Some(SubmitVerb::Learn),
                    kind: MemoryKind::Memory,
                    body: text_value,
                    workarea: Some(Reference::Name(
                        Name::parse(oml_app::domain::name::UNIVERSE).expect("a name"),
                    )),
                    derives_from: vec![source],
                    severity: None,
                    rule: None,
                    valid_from: None,
                    valid_until: None,
                    anchor: None,
                    format: None,
                    title: None,
                    origin: None,
                };
                retry_chain(|| self.app.submit_memory(Bearer::Session(&curate), &req))
                    .await
                    .map_err(app_error)?;
                return Ok(Outcome::Redirect(
                    "/web/admin?view=curate&done=curated:+held+until+someone+outside+its+lineage+approves+it"
                        .to_owned(),
                ));
            }
            "supersede" => {
                fields.only(&["memory", "expected_version", "text", "retirement"])?;
                let target = fields.id("memory")?;
                let expected_version = fields
                    .need("expected_version")?
                    .parse::<u32>()
                    .ok()
                    .filter(|v| *v >= 1)
                    .ok_or_else(|| {
                        ApiError::validation("expected_version is the version this page rendered")
                            .with_details(json!({"field": "expected_version"}))
                    })?;
                let body = fields.need("text")?.to_owned();
                let retirement = fields.yes("retirement");
                let admin = self.caller(session, Capacity::Admin).await?;
                let read = retry_chain(|| self.app.admin_read(Bearer::Session(&admin), target))
                    .await
                    .map_err(app_error)?;
                // No page supersedes or retires a guardrail: it is written
                // with its severity and rule on the API or the command line
                // (ADR-0037 d17).
                if read.memory.kind == MemoryKind::Guardrail {
                    return Err(ApiError::validation(
                        "a guardrail is superseded and retired on the API or the command line, with its severity and rule, never on this page",
                    )
                    .with_details(json!({"field": "memory"})));
                }
                let curate = self.caller(session, Capacity::Curate).await.ok();
                let acting = match curate {
                    Some(c) if read.memory.author_key_id == c.key().key_id => c,
                    _ => self.caller(session, Capacity::Review).await?,
                };
                let digest =
                    oml_app::domain::Sha256Hash::digest(format!("{target}\n{body}").as_bytes());
                let req = SupersedeRequest {
                    on_similar: Default::default(),
                    idempotency_key: format!("web-supersede-{}", &digest.to_prefixed()[7..39]),
                    target_id: target,
                    body,
                    kind: None,
                    retirement,
                    derives_from: Vec::new(),
                    reason: None,
                    workarea: None,
                    refuse_vocabulary_target: false,
                    severity: None,
                    rule: None,
                    valid_from: None,
                    valid_until: None,
                    anchor: None,
                    origin: None,
                    expected_version,
                    format: None,
                    title: None,
                };
                retry_chain(|| self.app.supersede(Bearer::Session(&acting), &req))
                    .await
                    .map_err(app_error)?;
                return Ok(Outcome::Redirect(format!(
                    "/web/admin?view=memories&memory={target}&done=proposed:+held+until+reviewed"
                )));
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

fn memory_state(m: &ServedMemory) -> &'static str {
    match m.memory.status {
        oml_app::domain::MemoryStatus::Held => "held",
        oml_app::domain::MemoryStatus::Promoted if m.memory.is_tip => "promoted",
        oml_app::domain::MemoryStatus::Promoted => "superseded",
        oml_app::domain::MemoryStatus::Rejected => "rejected",
        _ => "demoted",
    }
}

/// An anchor as one line (ADR-0057 d2), unescaped.
fn anchor_text(anchor: Option<&oml_app::domain::anchor::Anchor>) -> Option<String> {
    use oml_app::domain::anchor::Origin;
    let a = anchor?;
    let fragment = a
        .fragment
        .as_deref()
        .map_or_else(String::new, |f| format!("#{f}"));
    Some(match &a.origin {
        Origin::Repo { repo, rev, path } => {
            format!("{repo} @ {} {path}{fragment}", &rev[..rev.len().min(12)])
        }
        Origin::Uri { uri, .. } => format!("{uri}{fragment}"),
    })
}

/// An anchor as one line for a reviewer (ADR-0057 d2): the repository, the
/// commit's first 12 digits, the path and the fragment, or the page; a
/// dash for none.
fn anchor_line(anchor: Option<&oml_app::domain::anchor::Anchor>) -> String {
    anchor_text(anchor).map_or_else(|| "—".to_owned(), |line| short(&line))
}

/// A version's valid time as one line for a reviewer, and on a held
/// successor the end approving it gives the version it replaces, so a
/// verdict is given on what it makes true (ADR-0059 d2, d4).
fn valid_line(m: &ServedMemory) -> String {
    let valid = &m.memory.valid;
    let mut line = if valid.is_open() {
        "none declared".to_owned()
    } else {
        format!(
            "from {}, {}",
            valid.from.as_ref().map_or("any time", |f| f.as_str()),
            valid
                .until
                .as_ref()
                .map_or_else(|| "open".to_owned(), |u| format!("until {}", u.as_str()))
        )
    };
    if let (Some(end), Some(replaces)) = (&m.replaces_valid_until, &m.replaces) {
        line.push_str(&format!(
            "; approving ends version {} at {}",
            replaces.version,
            end.as_str()
        ));
    }
    text(&line)
}

/// What a card says of a version's format (ADR-0056 d9): for prose without a
/// title nothing, so a prose card is as it was; otherwise the title, the
/// media type, and for a derived search text what the claim will be found
/// by. The source itself is always the card's text, escaped, never rendered.
fn title_line(m: &MemoryRecord) -> String {
    m.format.title.as_ref().map_or_else(String::new, |t| {
        format!(r#"<p class="label">title</p><p>{}</p>"#, text(t))
    })
}

/// The class of the paragraph holding the source: monospaced for a format
/// that is not prose, where indentation carries meaning.
fn source_class(m: &MemoryRecord) -> &'static str {
    if matches!(m.format.media_type.as_str(), "text/plain" | "text/markdown") {
        ""
    } else {
        " mono"
    }
}

fn format_lines(m: &MemoryRecord) -> String {
    let f = &m.format;
    let prose = matches!(f.media_type.as_str(), "text/plain" | "text/markdown");
    if prose && f.search_text.is_none() {
        return String::new();
    }
    let mut out = String::new();
    if !prose {
        out.push_str(&format!(
            r#"<p class="label">format</p><p class="mono">{}</p>"#,
            text(&f.media_type)
        ));
    }
    if let Some(found) = &f.search_text {
        out.push_str(&format!(
            r#"<p class="label">it will be found by</p><p class="text mono dim">{}</p>"#,
            text(found)
        ));
    }
    out
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// The drawing beside a Mermaid claim's source, in a sandboxed frame
    /// (ADR-0056 d9); nothing for any other format.
    fn frame_for(&self, csrf: &str, m: &MemoryRecord) -> String {
        if m.format.media_type == "text/vnd.oml.mermaid" {
            self.render_iframe(csrf, &m.memory_id.to_string(), &m.body)
        } else {
            String::new()
        }
    }
}

fn memory_card(m: &ServedMemory, frame: &str) -> String {
    let from = if m.anonymous {
        "the shared area".to_owned()
    } else {
        short(&m.memory.author_principal)
    };
    // A held guardrail tells every reader what approving it does (proposed
    // ADR-0037 d10, d17).
    let warning = m.load_warning.map_or_else(String::new, |w| {
        format!(r#"<p class="lede">Guardrail: {}.</p>"#, text(w))
    });
    warning
        + &format!(
            r#"<div class="card"><p class="label">{} · {} · version {}</p>{}<p class="text{}">{}</p>{}{}<p class="dim mono">{} · from {}</p><p class="dim mono">source: {}</p><p class="dim mono">valid {}</p></div>"#,
            state_tag(memory_state(m)),
            short(m.workarea_path.as_deref().unwrap_or("")),
            m.memory.version,
            title_line(&m.memory),
            source_class(&m.memory),
            text(&m.memory.body),
            frame,
            format_lines(&m.memory),
            short(&m.memory.claim_key),
            from,
            anchor_line(m.memory.anchor.as_deref()),
            valid_line(m)
        )
}

fn author_line(m: &AdminMemory) -> String {
    let key = m.author.as_ref().and_then(|k| k.name.as_ref()).map_or_else(
        || m.memory.author_key_id.to_string(),
        |n| n.as_str().to_owned(),
    );
    match &m.author_user {
        Some(user) => format!("{} ({})", short(&key), short(user.as_str())),
        None => short(&key),
    }
}

fn attribution_card(m: &AdminMemory, frame: &str) -> String {
    let mut out = format!(
        r#"<div class="card"><p class="label">{} · {} · version {}</p>{}<p class="text{}">{}</p>{}{}</div><h2>Attribution</h2><table><tr><th>Field</th><th>Value</th></tr>"#,
        state_tag(m.state.as_str()),
        short(&m.workarea_path),
        m.memory.version,
        title_line(&m.memory),
        source_class(&m.memory),
        text(&m.memory.body),
        frame,
        format_lines(&m.memory)
    );
    let ids = |v: &[Uuid]| {
        if v.is_empty() {
            "—".to_owned()
        } else {
            v.iter()
                .map(|id| format!(r#"<a href="/web/admin?view=memories&amp;memory={id}">{id}</a>"#))
                .collect::<Vec<_>>()
                .join("<br>")
        }
    };
    for (field, value) in [
        ("artifact id", m.memory.memory_id.to_string()),
        ("claim key", short(&m.memory.claim_key)),
        ("written by key", author_line(m)),
        ("principal", short(&m.memory.author_principal)),
        ("written at", short(m.memory.created_at.as_str())),
        ("derives from", ids(&m.memory.derives_from)),
        (
            "replaces",
            m.memory.replaces.map_or("—".to_owned(), |r| ids(&[r])),
        ),
        ("text digest", short(&m.memory.body_digest)),
        ("source", anchor_line(m.memory.anchor.as_deref())),
        (
            "valid",
            if m.memory.valid.is_open() {
                "none declared".to_owned()
            } else {
                text(&format!(
                    "from {}, {}",
                    m.memory
                        .valid
                        .from
                        .as_ref()
                        .map_or("any time", |f| f.as_str()),
                    m.memory
                        .valid
                        .until
                        .as_ref()
                        .map_or_else(|| "open".to_owned(), |u| format!("until {}", u.as_str()))
                ))
            },
        ),
    ] {
        let _ = write!(
            out,
            r#"<tr><td class="label">{field}</td><td class="mono">{value}</td></tr>"#
        );
    }
    out.push_str("</table><h2>Verdicts</h2><table><tr><th>Verdict</th><th>Key</th><th>Principal</th><th>Reason</th><th>At</th></tr>");
    if m.verdicts.is_empty() {
        out.push_str(r#"<tr><td colspan="5" class="dim">No verdict yet.</td></tr>"#);
    }
    for v in &m.verdicts {
        let _ = write!(
            out,
            r#"<tr><td>{}</td><td class="mono">{}</td><td class="mono">{}</td><td>{}</td><td class="mono">{}</td></tr>"#,
            state_tag(&v.verdict),
            short(v.key_name.as_ref().map_or("", Name::as_str)),
            short(&v.principal),
            short(v.reason.as_deref().unwrap_or("")),
            short(v.at.as_str())
        );
    }
    out.push_str("</table>");
    out
}
