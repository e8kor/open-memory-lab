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
use oml_app::domain::guardrail::{FrameOptions, GuardrailScope, render_frame};
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::web::{Email, csrf_token};
use oml_app::domain::{MemoryKind, Role};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{
    AppError, Bearer, GuardrailRequest, MemoryRecord, NamedServe, ServedMemory, SubmitRequest,
    SubmitVerb, SupersedeRequest, Verdict, VerdictRequest, WebEmailRequest, WebSessionRecord,
};
use oml_app::types::{BULK_VERDICT_MAX, BulkVerdictRequest, HELD_PAGE_DEFAULT, HeldListingRequest};

use super::page::{attr, form, hidden, input, select, shell, short, state_tag, text, textarea};
use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::oauth::percent_encode;
use crate::{ApiState, retry_chain};

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

    /// The ids a listing's boxes picked, `pick_000` to `pick_199`, in the
    /// listing's order (ADR-0055 d10).
    fn picks(&self) -> Result<Vec<Uuid>, ApiError> {
        let mut picked: Vec<(&str, &str)> = self
            .0
            .iter()
            .filter(|(name, value)| name.starts_with("pick_") && !value.is_empty())
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        picked.sort_unstable();
        picked
            .into_iter()
            .map(|(name, value)| {
                Uuid::parse_str(value).map_err(|_| {
                    ApiError::validation(format!("{name} is not an id"))
                        .with_details(json!({ "field": name }))
                })
            })
            .collect()
    }

    /// [`Self::only`] that also admits the `pick_NNN` boxes of a listing.
    fn only_picks(&self, allowed: &[&str]) -> Result<(), ApiError> {
        for name in self.0.keys() {
            let pick = name
                .strip_prefix("pick_")
                .is_some_and(|n| n.len() == 3 && n.bytes().all(|b| b.is_ascii_digit()));
            if !pick
                && name != "action"
                && name != "csrf_token"
                && !allowed.contains(&name.as_str())
            {
                return Err(
                    ApiError::validation(format!("this action takes no field {name}"))
                        .with_details(json!({ "field": name })),
                );
            }
        }
        Ok(())
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

/// The Atlas tab's link, which is also its active marker.
const ATLAS_TAB: &str = "/web?view=memories&amp;display=atlas";

fn user_tabs(admin: bool) -> Vec<(&'static str, &'static str)> {
    // The design system's shared user nav (ADR-0069 d3): Start, Memories,
    // Review, Atlas, Keys; then this page's Workareas and Guardrails.
    let mut tabs = vec![
        ("/web/start", "Start"),
        ("/web?view=memories", "Memories"),
        ("/web?view=review", "Review"),
        (ATLAS_TAB, "Atlas"),
        ("/web", "Keys"),
        ("/web?view=workarea", "Workareas"),
        ("/web?view=guardrails", "Guardrails"),
    ];
    if admin {
        tabs.push(("/web/admin", "Administration"));
    }
    tabs
}

const ADMIN_TABS: [(&str, &str); 9] = [
    ("/web/admin", "People"),
    ("/web/admin/users", "Quotas"),
    ("/web/admin?view=queue", "Queue"),
    ("/web/admin?view=memories", "Memories"),
    ("/web/admin?view=workarea", "Workareas"),
    ("/web/admin?view=review", "Review"),
    ("/web/admin?view=held", "Bulk review"),
    ("/web/admin?view=curate", "Curate"),
    ("/web", "Your page"),
];

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
        let (active, body) = match query.get("view").unwrap_or("keys") {
            "setup" => ("/web?view=setup", self.setup_view()),
            "memories" => (
                if query.get("display") == Some("atlas") && query.get("memory").is_none() {
                    ATLAS_TAB
                } else {
                    "/web?view=memories"
                },
                self.memories_view(&web, session, &csrf, query).await?,
            ),
            "workarea" => (
                "/web?view=workarea",
                self.workarea_view(&web, session, &csrf, query).await?,
            ),
            "review" => ("/web?view=review", review_start("/web", &csrf)),
            "guardrails" => (
                "/web?view=guardrails",
                self.guardrails_view(&web, query).await?,
            ),
            _ => ("/web", self.keys_view(&web, session, &csrf).await?),
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

    /// The guardrails in force at the selected workarea, read-only. The page
    /// shows the scope and each rule's source path; it writes, supersedes and
    /// retires none (proposed ADR-0037 d17).
    async fn guardrails_view(
        &self,
        web: &oml_app::app::Caller,
        query: &Fields,
    ) -> Result<String, ApiError> {
        let request = GuardrailRequest {
            workarea: query
                .get("workarea")
                .map(|_| query.reference("workarea"))
                .transpose()?,
        };
        let set = self
            .app
            .guardrails(Bearer::Session(web), &request)
            .await
            .map_err(app_error)?;
        let root = web.key().user.ok_or_else(ApiError::not_found)?;
        let tree = self
            .app
            .workarea_tree(Bearer::Session(web), Reference::Id(root), None, 200)
            .await
            .map_err(app_error)?;
        let frame = render_frame(
            &crate::guardrail_set_json(&set),
            &FrameOptions {
                origin: "this server",
                checked: false,
                whole_over_cap: true,
            },
        )
        .map_err(ApiError::internal)?;
        let scope = match set.guardrail_scope {
            GuardrailScope::Workarea => "This workarea only.",
            GuardrailScope::Ancestors => "This workarea and its readable parent workareas.",
        };
        let mut body = format!(
            r#"<h1>Guardrails</h1><p class="desc">Read-only here; change them with the API or the CLI.</p><p>Rules in force at <code>{}</code>.</p><p><strong>Scope:</strong> {} Each rule names its source workarea.</p><form class="row" method="get" action="/web"><input type="hidden" name="view" value="guardrails"><label>Workarea<select name="workarea">"#,
            text(&set.scope_path),
            scope,
        );
        for node in &tree.items {
            let id = node.workarea.workarea_id.to_string();
            let label = node.path.strip_prefix("universe.").unwrap_or(&node.path);
            let _ = write!(
                body,
                r#"<option value="{}"{}>{}</option>"#,
                attr(&id),
                if node.workarea.workarea_id == set.scope_node {
                    " selected"
                } else {
                    ""
                },
                text(label)
            );
        }
        body.push_str("</select></label><button type=\"submit\">Show</button></form><ul>");
        let mut lines = frame.text.lines();
        let _header = lines.next();
        for line in lines {
            let _ = write!(body, "<li>{}</li>", text(line.trim_start_matches("- ")));
        }
        body.push_str("</ul>");
        Ok(body)
    }

    async fn keys_view(
        &self,
        web: &oml_app::app::Caller,
        session: &WebSessionRecord,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let keys = self.app.web_own_keys(web).await.map_err(app_error)?;
        let mut body = format!(
            r#"<h1>Keys</h1><p class="desc">Keys acting for <code>{}</code>.</p><table><tr><th>Key</th><th>Role</th><th>Access</th><th>State</th><th></th></tr>"#,
            short(session.user.as_str())
        );
        for own in &keys {
            body.push_str(&self.key_row(web, own, "/web", csrf).await?);
        }
        body.push_str("</table>");
        let prefix = format!("{}-", session.user.as_str());
        let _ = write!(
            body,
            "<h2>Agent key</h2><p class=\"desc\">Your workarea only. The value is shown once.</p>{}<h2>Connector key</h2><p class=\"desc\">For Claude on the web or a phone; its connect code is shown once.</p>{}",
            form(
                "/web",
                "issue_key",
                csrf,
                &format!(
                    "{}{}",
                    input("name", &format!("name, after {prefix}"), "", true),
                    select(
                        "role",
                        "role",
                        &[("writer", "writer"), ("reader", "reader")]
                    )
                ),
                "Issue key",
                true
            ),
            form(
                "/web",
                "connect_key",
                csrf,
                &format!(
                    "{}{}",
                    input("name", &format!("name, after {prefix}"), "", true),
                    select(
                        "role",
                        "role",
                        &[("writer", "writer"), ("reader", "reader")]
                    )
                ),
                "Issue connector key",
                false
            )
        );
        Ok(body)
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
            if key.credential.as_str() == "connection" && path == "/web" {
                acts.push_str(&form(
                    path,
                    "connect_code",
                    csrf,
                    &hidden("key", name),
                    "New connect code",
                    false,
                ));
            }
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

    async fn memories_view(
        &self,
        web: &oml_app::app::Caller,
        session: &WebSessionRecord,
        csrf: &str,
        query: &Fields,
    ) -> Result<String, ApiError> {
        let bearer = Bearer::Session(web);
        if let Some(id) = query.get("memory") {
            let mut body = String::from(
                r#"<p class="label">Memories</p><h1>One memory</h1><p><a href="/web?view=memories">Back to the list</a></p>"#,
            );
            let id = Uuid::parse_str(id).map_err(|_| {
                ApiError::validation("memory is not an id").with_details(json!({"field": "memory"}))
            })?;
            let served = self.app.get_memory(bearer, id).await.map_err(app_error)?;
            body.push_str(&memory_card(&served, &self.frame_for(csrf, &served.memory)));
            return Ok(body);
        }
        let list = super::memories::ListQuery::parse(query)?;
        let lede = "Newest first. A held memory waits for another identity's review. Its author and the agent's signed-in owner can read it; an in-progress workarea also serves it to readers.";
        if list.atlas {
            // The Atlas reads no memory: the person's workareas and their
            // counts, from the root unless a workarea is chosen (d32).
            let review = self.caller(session, Capacity::Review).await?;
            let scope = list
                .workarea_reference()
                .or_else(|| web.key().user.map(Reference::Id));
            let mut body = super::memories::list_view("/web", &list, None, lede);
            body.push_str(
                &self
                    .atlas_view("/web", web, &review, &review, scope, false)
                    .await?,
            );
            return Ok(body);
        }
        let page = self.user_cards(web, &list).await?;
        Ok(super::memories::list_view("/web", &list, Some(&page), lede))
    }

    async fn workarea_view(
        &self,
        web: &oml_app::app::Caller,
        session: &WebSessionRecord,
        csrf: &str,
        query: &Fields,
    ) -> Result<String, ApiError> {
        let root = web.key().user.ok_or_else(ApiError::not_found)?;
        let tree = self
            .app
            .workarea_tree(Bearer::Session(web), Reference::Id(root), None, 200)
            .await
            .map_err(app_error)?;
        let nodes: Vec<super::workareas::Node> = tree
            .items
            .iter()
            .map(|node| super::workareas::Node {
                id: node.workarea.workarea_id,
                path: node.path.clone(),
                name: if node.workarea.workarea_id == root {
                    node.workarea.name.as_str().to_owned()
                } else {
                    node.path
                        .strip_prefix("universe.")
                        .unwrap_or(&node.path)
                        .to_owned()
                },
            })
            .collect();
        // Every act here is the person's reviewer key's: a dreamer key a
        // person holds proposes, ratifies another key's proposal, and sets
        // the mode (ADR-0009 d3; ADR-0030 d1; ADR-0045 d28).
        let review = self.caller(session, Capacity::Review).await?;
        let mut body = format!(
            r#"<h1>Workareas</h1><p class="desc">Under <code>universe.{}</code>. Apply asks you to confirm.</p>"#,
            short(session.user.as_str())
        );
        body.push_str(
            &self
                .workarea_table("/web", csrf, &review, &review, &nodes, query)
                .await?,
        );
        body.push_str("<details class=\"help\"><summary>?</summary>In progress: your agents recall each other's unreviewed memories inside it, and nothing leaves it until you commit, which needs every memory in it reviewed.</details>");
        body.push_str("<h2>New workarea</h2>");
        body.push_str(&form(
            "/web",
            "create_workarea",
            csrf,
            &format!(
                "{}{}",
                input("name", "name", "", true),
                input("parent", "inside (blank: your root)", "", false)
            ),
            "Create",
            true,
        ));
        Ok(body)
    }

    /// One act of the user page (ADR-0045 d9): review as `<user>-review`,
    /// every other act as `<user>-web`.
    pub(crate) async fn user_page_act(
        &self,
        token: &str,
        session: &WebSessionRecord,
        action: &str,
        fields: &Fields,
    ) -> Result<Outcome, ApiError> {
        match action {
            "issue_key" | "connect_key" => {
                fields.only(&["name", "role"])?;
                let suffix = fields.need("name")?.to_owned();
                let role = match fields.get("role").unwrap_or("writer") {
                    "reader" => Role::Reader,
                    "writer" => Role::Writer,
                    _ => {
                        return Err(ApiError::validation("role is reader or writer")
                            .with_details(json!({"field": "role"})));
                    }
                };
                let web = self.caller(session, Capacity::Web).await?;
                let connector = action == "connect_key";
                let issued = retry_chain(|| self.app.web_issue_key(&web, &suffix, role, connector))
                    .await
                    .map_err(app_error)?;
                Ok(Outcome::Page(
                    self.issued_page(token, session, &issued).await,
                ))
            }
            "connect_code" => {
                fields.only(&["key"])?;
                let key = fields.name("key")?;
                let web = self.caller(session, Capacity::Web).await?;
                let code = retry_chain(|| self.app.web_connect_code(&web, &key))
                    .await
                    .map_err(app_error)?;
                let body = connect_card(key.as_str(), &code);
                Ok(Outcome::Page(shell(
                    "Connect code",
                    &self.who(session).await,
                    &user_tabs(session.admin),
                    "/web",
                    None,
                    &body,
                )))
            }
            "cut_off" => {
                fields.only(&["key"])?;
                let key = fields.name("key")?;
                let web = self.caller(session, Capacity::Web).await?;
                retry_chain(|| self.app.web_cut_off(&web, &key))
                    .await
                    .map_err(app_error)?;
                Ok(Outcome::Redirect("/web?done=cut+off".to_owned()))
            }
            "create_workarea" => {
                fields.only(&["name", "parent"])?;
                let name = fields.name("name")?;
                let parent = match fields.get("parent") {
                    Some(_) => Some(fields.reference("parent")?),
                    None => None,
                };
                let web = self.caller(session, Capacity::Web).await?;
                retry_chain(|| {
                    self.app
                        .create_workarea(Bearer::Session(&web), &name, parent.clone(), None)
                })
                .await
                .map_err(app_error)?;
                Ok(Outcome::Redirect(
                    "/web?view=workarea&done=created".to_owned(),
                ))
            }
            "workarea_mode" | "workarea_transition" | "workarea_ratify" => {
                let review = self.caller(session, Capacity::Review).await?;
                Ok(Outcome::Redirect(
                    self.workarea_act("/web", action, &review, &review, fields)
                        .await?,
                ))
            }
            "review_next" | "verdict" => {
                self.review_act("/web", token, session, action, fields)
                    .await
            }
            _ => Err(
                ApiError::validation("action is not one of this page's actions")
                    .with_details(json!({"field": "action"})),
            ),
        }
    }

    async fn issued_page(
        &self,
        _token: &str,
        session: &WebSessionRecord,
        issued: &oml_app::types::IssuedKey,
    ) -> String {
        let issuer = self
            .app
            .oauth_issuer()
            .map_or(DEFAULT_SERVER, |i| i.as_str())
            .to_owned();
        let body = match &issued.connect_code {
            Some(code) => connect_card(issued.name.as_str(), code),
            None => format!(
                r#"<h1>{}</h1><p class="desc">Shown once: copy it now. Then <a href="/web?view=setup">set up</a> your agent.</p><div class="card secret"><p class="label">For your agent</p><pre>OML_URL={}
OML_API_KEY={}</pre></div><p><a href="/web">Back to your keys</a></p>"#,
                short(issued.name.as_str()),
                attr(&issuer),
                attr(&issued.value)
            ),
        };
        shell(
            "Key issued",
            &self.who(session).await,
            &user_tabs(session.admin),
            "/web",
            None,
            &body,
        )
    }

    /// `review_next` and `verdict` (ADR-0045 d11), as `<user>-review`: the
    /// served item rendered whole with its `serve_id`; a verdict carries an
    /// idempotency key minted from the serve, so a resubmitted form replays.
    async fn review_act(
        &self,
        path: &str,
        token: &str,
        session: &WebSessionRecord,
        action: &str,
        fields: &Fields,
    ) -> Result<Outcome, ApiError> {
        let review = self.caller(session, Capacity::Review).await?;
        let csrf = csrf_token(token);
        let (tabs, active): (Vec<(&str, &str)>, &str) = if path == "/web" {
            (user_tabs(session.admin), "/web?view=review")
        } else {
            (ADMIN_TABS.to_vec(), "/web/admin?view=review")
        };
        let mut notice = None;
        if action == "verdict" {
            fields.only(&["serve_id", "verdict", "reason"])?;
            let serve_id = fields.need("serve_id")?.to_owned();
            let memory_id = fields.id("serve_id")?;
            let verdict = match fields.need("verdict")? {
                "approve" => Verdict::Approve,
                "reject" => Verdict::Reject,
                _ => {
                    return Err(ApiError::validation("verdict is approve or reject")
                        .with_details(json!({"field": "verdict"})));
                }
            };
            let req = VerdictRequest {
                serve: Some(NamedServe {
                    serve_id: serve_id.clone(),
                    memory_id,
                }),
                verdict,
                reason: fields.get("reason").map(ToOwned::to_owned),
                idempotency_key: format!("web-verdict-{serve_id}"),
            };
            let outcome = retry_chain(|| self.app.review_verdict(Bearer::Session(&review), &req))
                .await
                .map_err(app_error)?;
            notice = Some(format!(
                "Recorded: version {} is now {}.",
                outcome.version,
                outcome.state.as_str()
            ));
        } else {
            fields.only(&[])?;
        }
        let next = retry_chain(|| self.app.review_next(Bearer::Session(&review)))
            .await
            .map_err(app_error)?;
        let mut body = String::from(r#"<p class="label">Review</p><h1>Review</h1>"#);
        match next {
            None => body.push_str(r#"<p class="desc">Nothing waits for you.</p>"#),
            Some(served) => {
                let serve_id = format!("srv_{}", served.memory.memory_id.simple());
                body.push_str(&memory_card(
                    &served,
                    &self.frame_for(&csrf, &served.memory),
                ));
                body.push_str(&form(
                    path,
                    "verdict",
                    &csrf,
                    &format!(
                        "{}{}{}",
                        hidden("serve_id", &serve_id),
                        select(
                            "verdict",
                            "verdict",
                            &[("approve", "approve"), ("reject", "reject")]
                        ),
                        input("reason", "reason", "", false)
                    ),
                    "Record verdict",
                    true,
                ));
            }
        }
        body.push_str(&review_start(path, &csrf));
        Ok(Outcome::Page(shell(
            "Review",
            &self.who(session).await,
            &tabs,
            active,
            notice.as_deref(),
            &body,
        )))
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
            "queue" => ("/web/admin?view=queue", self.queue_view(&admin).await?),
            "memories" => (
                "/web/admin?view=memories",
                self.admin_memories_view(&admin, session, query, &csrf)
                    .await?,
            ),
            "review" => ("/web/admin?view=review", review_start("/web/admin", &csrf)),
            "workarea" => (
                "/web/admin?view=workarea",
                self.admin_workarea_view(&admin, session, &csrf, query)
                    .await?,
            ),
            "held" => (
                "/web/admin?view=held",
                self.held_view(session, query, &csrf).await?,
            ),
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

    /// The bulk review listing (ADR-0055 d10): the rows the person's
    /// reviewer key may judge, filtered by workarea, author key and claim
    /// key prefix, each with its text and a box to pick it; "select all"
    /// renders the page with every box checked, since the page carries no
    /// script. Picking leads to `bulk_confirm`, never straight to a verdict.
    async fn held_view(
        &self,
        session: &WebSessionRecord,
        query: &Fields,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let review = self.caller(session, Capacity::Review).await?;
        let reference = |field: &'static str| -> Result<Option<Reference>, ApiError> {
            query
                .get(field)
                .map(|v| {
                    Reference::parse(v).map_err(|e| {
                        ApiError::validation(e.to_string()).with_details(json!({ "field": field }))
                    })
                })
                .transpose()
        };
        let req = HeldListingRequest {
            workarea: reference("workarea")?,
            author_key: reference("author_key")?,
            claim_key_prefix: query.get("claim_key_prefix").map(ToOwned::to_owned),
            cursor: query.get("cursor").map(ToOwned::to_owned),
            limit: HELD_PAGE_DEFAULT,
        };
        let page = self
            .app
            .review_held(Bearer::Session(&review), &req)
            .await
            .map_err(app_error)?;
        let all = query.get("select") == Some("all");
        let filters = [
            ("workarea", query.get("workarea").unwrap_or("")),
            ("author_key", query.get("author_key").unwrap_or("")),
            (
                "claim_key_prefix",
                query.get("claim_key_prefix").unwrap_or(""),
            ),
        ];
        let kept: String = filters
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("&amp;{k}={}", attr(&percent_encode(v))))
            .collect();
        let cursor = query.get("cursor").map_or(String::new(), |c| {
            format!("&amp;cursor={}", attr(&percent_encode(c)))
        });
        let mut body = format!(
            r#"<h1>Bulk review</h1><p class="desc">Oldest first, {HELD_PAGE_DEFAULT} a page. Each pick gets its own verdict; you confirm first. A memory your reviewer key wrote, or that derives from one, is never listed.</p><form class="row" method="get" action="/web/admin"><input type="hidden" name="view" value="held">{}{}{}<button type="submit">Filter</button></form><p><a href="/web/admin?view=held{kept}{cursor}&amp;select=all">Select all on this page</a> · <a href="/web/admin?view=held{kept}{cursor}">Clear</a></p>"#,
            input("workarea", "workarea", filters[0].1, false),
            input("author_key", "author key", filters[1].1, false),
            input("claim_key_prefix", "claim key prefix", filters[2].1, false),
        );
        let mut rows = String::from(
            "<table><tr><th></th><th>Claim key</th><th>Workarea</th><th>From</th><th>Held since</th><th>Text</th><th>Source</th><th>Valid</th></tr>",
        );
        if page.items.is_empty() {
            rows.push_str(
                r#"<tr><td colspan="8" class="dim">Nothing waits for you here.</td></tr>"#,
            );
        }
        for (i, item) in page.items.iter().enumerate() {
            let m = &item.served.memory;
            let _ = write!(
                rows,
                r#"<tr><td><input type="checkbox" name="pick_{i:03}" value="{}"{}></td><td class="mono">{}</td><td class="mono">{}</td><td class="mono">{}{}</td><td class="mono">{}</td><td class="text">{}</td><td class="mono">{}</td><td class="mono">{}</td></tr>"#,
                m.memory_id,
                if all { " checked" } else { "" },
                short(&m.claim_key),
                short(item.served.workarea_path.as_deref().unwrap_or("")),
                short(&m.author_principal),
                if item.same_user { " (you)" } else { "" },
                short(item.held_since.as_str()),
                text(&m.body),
                anchor_line(m.anchor.as_deref()),
                valid_line(&item.served)
            );
        }
        rows.push_str("</table>");
        body.push_str(&form(
            "/web/admin",
            "bulk_confirm",
            csrf,
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
        if let Some(next) = &page.next_cursor {
            let _ = write!(
                body,
                r#"<p><a href="/web/admin?view=held{kept}&amp;cursor={}">Next page</a></p>"#,
                attr(&percent_encode(next))
            );
        }
        Ok(body)
    }

    /// `bulk_confirm` and `bulk_verdict` (ADR-0055 d10): the first shows
    /// what was picked and asks again, carrying the ids, the verdict, the
    /// reason and an idempotency key minted from them; the second records
    /// one verdict per id as the person's reviewer key and shows each
    /// outcome. A resubmitted confirmation replays.
    async fn bulk_act(
        &self,
        token: &str,
        session: &WebSessionRecord,
        action: &str,
        fields: &Fields,
    ) -> Result<Outcome, ApiError> {
        let review = self.caller(session, Capacity::Review).await?;
        let csrf = csrf_token(token);
        let verdict = match fields.need("verdict")? {
            "approve" => Verdict::Approve,
            "reject" => Verdict::Reject,
            _ => {
                return Err(ApiError::validation("verdict is approve or reject")
                    .with_details(json!({"field": "verdict"})));
            }
        };
        let reason = fields.get("reason").map(ToOwned::to_owned);
        let refused = |reason: &str| {
            ApiError::validation(reason.to_owned()).with_details(json!({"field": "artifact_ids"}))
        };
        let ids: Vec<Uuid> = if action == "bulk_confirm" {
            fields.only_picks(&["verdict", "reason"])?;
            fields.picks()?
        } else {
            fields.only(&["verdict", "reason", "artifact_ids", "idempotency_key"])?;
            fields
                .need("artifact_ids")?
                .split(' ')
                .map(|id| {
                    Uuid::parse_str(id)
                        .map_err(|_| refused("artifact_ids holds a value that is not an id"))
                })
                .collect::<Result<_, _>>()?
        };
        if ids.is_empty() {
            return Err(refused("pick at least one memory"));
        }
        if ids.len() > BULK_VERDICT_MAX {
            return Err(refused("a page picks at most 200 memories"));
        }
        if verdict == Verdict::Reject && reason.is_none() {
            return Err(ApiError::validation("reason is required to reject")
                .with_details(json!({"field": "reason"})));
        }
        let verdict_word = if verdict == Verdict::Approve {
            "approve"
        } else {
            "reject"
        };
        let joined = ids
            .iter()
            .map(Uuid::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        if action == "bulk_confirm" {
            let digest = oml_app::domain::Sha256Hash::digest(
                format!(
                    "{verdict_word}\n{}\n{joined}",
                    reason.as_deref().unwrap_or("")
                )
                .as_bytes(),
            );
            let named = self
                .app
                .review_named(Bearer::Session(&review), &ids)
                .await
                .map_err(app_error)?;
            let mut body = format!(
                r#"<h1>{} {} memories?</h1><p class="desc">One verdict each. Any no longer held is skipped.</p><table><tr><th>Claim key</th><th>First line</th><th>Artifact</th></tr>"#,
                if verdict == Verdict::Approve {
                    "Approve"
                } else {
                    "Reject"
                },
                ids.len()
            );
            for (id, served) in &named {
                match served {
                    Some(m) => {
                        let _ = write!(
                            body,
                            r#"<tr><td class="mono">{}</td><td class="text">{}</td><td class="mono">{id}</td></tr>"#,
                            short(&m.memory.claim_key),
                            text(m.memory.body.lines().next().unwrap_or_default())
                        );
                    }
                    None => {
                        let _ = write!(
                            body,
                            r#"<tr><td class="dim" colspan="2">no longer held, or not yours to judge: it will be skipped</td><td class="mono">{id}</td></tr>"#
                        );
                    }
                }
            }
            body.push_str("</table>");
            if let Some(reason) = &reason {
                let _ = write!(body, r#"<p>Reason: {}</p>"#, text(reason));
            }
            body.push_str(&form(
                "/web/admin",
                "bulk_verdict",
                &csrf,
                &format!(
                    "{}{}{}{}",
                    hidden("verdict", verdict_word),
                    hidden("reason", reason.as_deref().unwrap_or("")),
                    hidden("artifact_ids", &joined),
                    hidden(
                        "idempotency_key",
                        &format!("web-bulk-{}", &digest.to_prefixed()[7..39])
                    ),
                ),
                &format!("Yes, {verdict_word} {}", ids.len()),
                true,
            ));
            body.push_str(r#"<p><a href="/web/admin?view=held">Back to the list</a></p>"#);
            return Ok(Outcome::Page(shell(
                "Bulk review",
                &self.who(session).await,
                &ADMIN_TABS,
                "/web/admin?view=held",
                None,
                &body,
            )));
        }
        let req = BulkVerdictRequest {
            verdict,
            artifact_ids: ids,
            reason,
            idempotency_key: fields.need("idempotency_key")?.to_owned(),
        };
        let items = self
            .app
            .review_bulk(Bearer::Session(&review), &req)
            .await
            .map_err(app_error)?;
        crate::count_bulk(self.traffic.metrics(), &items);
        let answer = crate::bulk_json(items);
        let mut body = format!(
            r#"<h1>Recorded</h1><p class="desc">{} approved, {} rejected, {} skipped.</p><table><tr><th>Artifact</th><th>Outcome</th><th>Detail</th></tr>"#,
            answer["summary"]["approved"],
            answer["summary"]["rejected"],
            answer["summary"]["skipped"]
        );
        for item in answer["items"].as_array().into_iter().flatten() {
            let detail = match item["code"].as_str() {
                Some(code) => format!("{}: {}", code, item["message"].as_str().unwrap_or_default()),
                None => format!(
                    "journal seq {}{}{}",
                    item["journal_seq"],
                    if item["same_user"] == true {
                        ", same user"
                    } else {
                        ""
                    },
                    if item["replayed"] == true {
                        ", replayed"
                    } else {
                        ""
                    }
                ),
            };
            let _ = write!(
                body,
                r#"<tr><td class="mono">{}</td><td>{}</td><td class="mono">{}</td></tr>"#,
                short(item["artifact_id"].as_str().unwrap_or_default()),
                state_tag(item["outcome"].as_str().unwrap_or_default()),
                short(&detail)
            );
        }
        body.push_str(r#"</table><p><a href="/web/admin?view=held">Back to the list</a></p>"#);
        Ok(Outcome::Page(shell(
            "Bulk review",
            &self.who(session).await,
            &ADMIN_TABS,
            "/web/admin?view=held",
            None,
            &body,
        )))
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

    /// The tenant's tree with each workarea's dropdown (ADR-0045 d28): the
    /// admin key reads the tree as structure (ADR-0026 d15) and proposes
    /// and ratifies; the mode is the person's reviewer key's.
    async fn admin_workarea_view(
        &self,
        admin: &oml_app::app::Caller,
        session: &WebSessionRecord,
        csrf: &str,
        query: &Fields,
    ) -> Result<String, ApiError> {
        let tree = self
            .app
            .workarea_tree(
                Bearer::Session(admin),
                Reference::Name(Name::parse(oml_app::domain::name::UNIVERSE).expect("a name")),
                None,
                200,
            )
            .await
            .map_err(app_error)?;
        let nodes: Vec<super::workareas::Node> = tree
            .items
            .iter()
            .map(|node| super::workareas::Node {
                id: node.workarea.workarea_id,
                path: node.path.clone(),
                name: node
                    .path
                    .strip_prefix("universe.")
                    .unwrap_or(&node.path)
                    .to_owned(),
            })
            .collect();
        let review = self.caller(session, Capacity::Review).await?;
        let mut body = String::from(
            r#"<h1>Workareas</h1><p class="desc">Apply asks you to confirm.</p><details class="help"><summary>?</summary>Status changes run as your admin key and need a second key to ratify; in progress and commit run as your reviewer key.</details>"#,
        );
        body.push_str(
            &self
                .workarea_table("/web/admin", csrf, admin, &review, &nodes, query)
                .await?,
        );
        Ok(body)
    }

    async fn queue_view(&self, admin: &oml_app::app::Caller) -> Result<String, ApiError> {
        let rows = self.app.web_queue(admin).await.map_err(app_error)?;
        let mut body = String::from(
            r#"<h1>Queue</h1><p class="desc">Held memories per workarea.</p><table><tr><th>Workarea</th><th>Held</th><th>Oldest since</th></tr>"#,
        );
        if rows.is_empty() {
            body.push_str(r#"<tr><td colspan="3" class="dim">Nothing is held.</td></tr>"#);
        }
        for row in &rows {
            let _ = write!(
                body,
                r#"<tr><td class="mono">{}</td><td>{}</td><td class="mono">{}</td></tr>"#,
                short(&row.path),
                row.depth.held,
                short(row.depth.oldest.as_str())
            );
        }
        body.push_str("</table>");
        Ok(body)
    }

    async fn admin_memories_view(
        &self,
        admin: &oml_app::app::Caller,
        session: &WebSessionRecord,
        query: &Fields,
        csrf: &str,
    ) -> Result<String, ApiError> {
        if let Some(id) = query.get("memory") {
            let mut body = String::from(
                r#"<p class="label">Memories</p><h1>One memory</h1><p><a href="/web/admin?view=memories">Back to the list</a></p>"#,
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
            return Ok(body);
        }
        let list = super::memories::ListQuery::parse(query)?;
        let lede = "Every memory, newest first. Each page you read is journalled.";
        if list.atlas {
            let mut body = super::memories::list_view("/web/admin", &list, None, lede);
            body.push_str(
                &self
                    .atlas_view(
                        "/web/admin",
                        admin,
                        admin,
                        &self.caller(session, Capacity::Review).await?,
                        list.workarea_reference(),
                        true,
                    )
                    .await?,
            );
            return Ok(body);
        }
        let page = self.admin_cards(admin, &list).await?;
        Ok(super::memories::list_view(
            "/web/admin",
            &list,
            Some(&page),
            lede,
        ))
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
            "review_next" | "verdict" => {
                return self
                    .review_act("/web/admin", token, session, action, fields)
                    .await;
            }
            "bulk_confirm" | "bulk_verdict" => {
                return self.bulk_act(token, session, action, fields).await;
            }
            "workarea_mode" | "workarea_transition" | "workarea_ratify" => {
                // Transitions as the admin key, which ADR-0009 admits by
                // role; the mode as the person's reviewer key, a dreamer
                // key a person holds (ADR-0030 d1; ADR-0045 d28).
                let admin = self.caller(session, Capacity::Admin).await?;
                let review = self.caller(session, Capacity::Review).await?;
                return Ok(Outcome::Redirect(
                    self.workarea_act("/web/admin", action, &admin, &review, fields)
                        .await?,
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
                // (proposed ADR-0037 d17).
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
            "cut_off" => {
                fields.only(&["key"])?;
                let key = fields.name("key")?;
                retry_chain(|| self.app.web_cut_off(&admin, &key))
                    .await
                    .map_err(app_error)?;
                Ok(Outcome::Redirect("/web/admin?done=cut+off".to_owned()))
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
                body.push_str(&self.key_row(admin, own, "/web/admin", &csrf).await?);
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

fn review_start(path: &str, csrf: &str) -> String {
    format!(
        r#"<h1>Review</h1><p class="desc">Your own writes are never served to you.</p>{}"#,
        form(path, "review_next", csrf, "", "Review next", true)
    )
}

fn connect_card(key: &str, code: &oml_app::types::ConnectCodeIssued) -> String {
    format!(
        r#"<p class="label">Connect code</p><h1>{}</h1><p class="desc">Shown once; works once, until {}. Add a custom connector in Claude with this URL, then enter the code.</p><div class="card secret"><p class="label">Connector URL</p><pre>{}</pre><p class="label">Connect code</p><pre>{}</pre></div><p><a href="/web">Back to your keys</a></p>"#,
        short(key),
        short(code.expires_at.as_str()),
        attr(&code.connector_url),
        attr(code.code.as_deref().unwrap_or(""))
    )
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

/// An anchor as one line for a reviewer (ADR-0057 d2): the repository, the
/// commit's first 12 digits, the path and the fragment, or the page; a
/// dash for none.
fn anchor_line(anchor: Option<&oml_app::domain::anchor::Anchor>) -> String {
    super::memories::anchor_text(anchor).map_or_else(|| "—".to_owned(), |line| short(&line))
}

/// A version's valid time as one line for a reviewer, and on a held
/// successor the end approving it gives the version it replaces, so a
/// verdict is given on what it makes true (proposed ADR-0059 d2, d4).
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
