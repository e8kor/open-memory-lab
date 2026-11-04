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
    AdminState, AppError, Bearer, HIT_KINDS, ListRequest, NamedServe, RecallRequest, ServedMemory,
    SubmitRequest, SubmitVerb, SupersedeRequest, Verdict, VerdictRequest, WebEmailRequest,
    WebSessionRecord,
};

use super::page::{attr, form, hidden, input, select, shell, short, state_tag, text, textarea};
use crate::error::{ApiError, ErrorContext, map_app_error};
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

    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn need(&self, name: &'static str) -> Result<&str, ApiError> {
        self.get(name).ok_or_else(|| {
            ApiError::validation(format!("{name} is required"))
                .with_details(json!({ "field": name }))
        })
    }

    /// A field the action does not define is refused (ADR-0045 d6).
    fn only(&self, allowed: &[&str]) -> Result<(), ApiError> {
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

    fn name(&self, field: &'static str) -> Result<Name, ApiError> {
        Name::parse(self.need(field)?).map_err(|e| {
            ApiError::validation(e.to_string()).with_details(json!({ "field": field }))
        })
    }

    fn reference(&self, field: &'static str) -> Result<Reference, ApiError> {
        Reference::parse(self.need(field)?).map_err(|e| {
            ApiError::validation(e.to_string()).with_details(json!({ "field": field }))
        })
    }

    fn id(&self, field: &'static str) -> Result<Uuid, ApiError> {
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

fn user_tabs(admin: bool) -> Vec<(&'static str, &'static str)> {
    let mut tabs = vec![
        ("/web", "Keys"),
        ("/web?view=setup", "Set up"),
        ("/web?view=memories", "Memories"),
        ("/web?view=workarea", "Workareas"),
        ("/web?view=review", "Review"),
    ];
    if admin {
        tabs.push(("/web/admin", "Administration"));
    }
    tabs
}

const ADMIN_TABS: [(&str, &str); 6] = [
    ("/web/admin", "People"),
    ("/web/admin?view=queue", "Queue"),
    ("/web/admin?view=memories", "Memories"),
    ("/web/admin?view=review", "Review"),
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
    async fn caller(
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
            "memories" => ("/web?view=memories", self.memories_view(&web, query).await?),
            "workarea" => (
                "/web?view=workarea",
                self.workarea_view(&web, session, &csrf).await?,
            ),
            "review" => ("/web?view=review", review_start("/web", &csrf)),
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

    async fn keys_view(
        &self,
        web: &oml_app::app::Caller,
        session: &WebSessionRecord,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let keys = self.app.web_own_keys(web).await.map_err(app_error)?;
        let mut body = format!(
            r#"<p class="label">Keys</p><h1>Your keys</h1><p class="lede">Every key that acts for <code>{}</code>: the keys your agents hold, and the one this page acts with. When each key was last used is not recorded.</p><table><tr><th>Key</th><th>Role</th><th>Access</th><th>State</th><th></th></tr>"#,
            short(session.user.as_str())
        );
        for own in &keys {
            body.push_str(&self.key_row(web, own, "/web", csrf).await?);
        }
        body.push_str("</table>");
        let prefix = format!("{}-", session.user.as_str());
        let _ = write!(
            body,
            "<h2>Issue a key for an agent</h2><p class=\"lede\">The key reads and writes in your own workarea and nothing else; its value is shown once.</p>{}<h2>Connect Claude on the web or a phone</h2><p class=\"lede\">A connector key holds a connection instead of a value; you enter its connect code once, in Claude.</p>{}",
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
            format!(
                "The Claude Code plugin and the agent kit connect to <code>{DEFAULT_SERVER}</code> unless told otherwise, which is this server: an agent needs only its key."
            )
        } else {
            format!(
                "The Claude Code plugin and the agent kit connect to <code>{DEFAULT_SERVER}</code> unless told otherwise; this server is <code>{}</code>, so give each agent this URL beside its key.",
                attr(&issuer)
            )
        };
        format!(
            r#"<p class="label">Set up</p><h1>Connect your agents</h1><p class="lede">{url_line}</p><div class="card"><p class="label">Server</p><pre class="mono">{}</pre></div><h2>Claude Code</h2><p>Issue a writer key on the Keys tab, then follow <a href="{GUIDES}/claude-code/">the Claude Code guide</a>: the plugin keeps the key in the system keychain.</p><h2>Claude on the web and on a phone</h2><p>Issue a connector key, then follow <a href="{GUIDES}/claude-ai-and-mobile/">the claude.ai and mobile guide</a> with its connector URL and connect code.</p><h2>Other agent tools</h2><p>Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop: <a href="{GUIDES}/other-harnesses/">the other agent tools guide</a>.</p>"#,
            attr(&issuer)
        )
    }

    async fn memories_view(
        &self,
        web: &oml_app::app::Caller,
        query: &Fields,
    ) -> Result<String, ApiError> {
        let bearer = Bearer::Session(web);
        let mut body = format!(
            r#"<p class="label">Memories</p><h1>Your memories</h1><p class="lede">What your agents wrote, in every state, and what your workarea and the shared area serve. A held memory is visible to its own agent until someone other than its author approves it.</p><form class="row" method="get" action="/web"><input type="hidden" name="view" value="memories">{}<button type="submit">Search</button></form>"#,
            input("q", "search", query.get("q").unwrap_or(""), false)
        );
        if let Some(id) = query.get("memory") {
            let id = Uuid::parse_str(id).map_err(|_| {
                ApiError::validation("memory is not an id").with_details(json!({"field": "memory"}))
            })?;
            let served = self.app.get_memory(bearer, id).await.map_err(app_error)?;
            body.push_str(&memory_card(&served));
            return Ok(body);
        }
        if let Some(q) = query.get("q") {
            let result = self
                .app
                .recall(
                    bearer,
                    &RecallRequest {
                        query: q.to_owned(),
                        k: 25,
                        workarea: None,
                        kinds: HIT_KINDS.to_vec(),
                        include_text: true,
                        include_unreviewed: true,
                        max_tokens: None,
                    },
                )
                .await
                .map_err(app_error)?;
            body.push_str("<h2>Found</h2>");
            let served: Vec<&ServedMemory> = result
                .steering
                .iter()
                .chain(result.hits.iter().map(|h| &h.served))
                .collect();
            body.push_str(&memory_table(&served, "/web?view=memories"));
            return Ok(body);
        }
        let own = self
            .app
            .list_memories(
                bearer,
                &ListRequest {
                    own: true,
                    workarea: None,
                    state: None,
                    kind: None,
                    claim_key: None,
                    since: None,
                    until: None,
                    limit: 50,
                    cursor: None,
                },
            )
            .await
            .map_err(app_error)?;
        body.push_str("<h2>Your agents' work</h2>");
        body.push_str(&memory_table(
            &own.items.iter().collect::<Vec<_>>(),
            "/web?view=memories",
        ));
        let shared = self
            .app
            .list_memories(
                bearer,
                &ListRequest {
                    own: false,
                    workarea: Some(Reference::Name(
                        Name::parse(oml_app::domain::name::UNIVERSE).expect("a name"),
                    )),
                    state: None,
                    kind: None,
                    claim_key: None,
                    since: None,
                    until: None,
                    limit: 50,
                    cursor: None,
                },
            )
            .await;
        if let Ok(shared) = shared {
            body.push_str("<h2>The shared area</h2><p class=\"lede\">Curated claims, each reviewed by someone else, shown without their author or source.</p>");
            body.push_str(&memory_table(
                &shared.items.iter().collect::<Vec<_>>(),
                "/web?view=memories",
            ));
        }
        Ok(body)
    }

    async fn workarea_view(
        &self,
        web: &oml_app::app::Caller,
        session: &WebSessionRecord,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let root = web.key().user.ok_or_else(ApiError::not_found)?;
        let tree = self
            .app
            .workarea_tree(Bearer::Session(web), Reference::Id(root), None, 200)
            .await
            .map_err(app_error)?;
        let mut body = format!(
            r#"<p class="label">Workareas</p><h1>Your workarea</h1><p class="lede">Your keys write, read and make workareas under <code>universe.{}</code>, and nowhere else.</p><table><tr><th>Path</th><th>Name to use</th></tr>"#,
            short(session.user.as_str())
        );
        for node in &tree.items {
            let qualified = if node.workarea.workarea_id == root {
                node.workarea.name.as_str().to_owned()
            } else {
                format!("{}.{}", session.user.as_str(), node.workarea.name.as_str())
            };
            let _ = write!(
                body,
                r#"<tr><td class="mono">{}</td><td class="mono">{}</td></tr>"#,
                short(&node.path),
                short(&qualified)
            );
        }
        body.push_str("</table><h2>Make a workarea</h2>");
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
                r#"<p class="label">Key issued</p><h1>{}</h1><p class="lede">Copy the value now: it is shown once and nothing keeps it. Put it where your agent reads its key, then follow <a href="/web?view=setup">Set up</a>.</p><div class="card secret"><p class="label">For your agent</p><pre>OML_URL={}
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
            None => body.push_str(
                r#"<p class="lede">Nothing waits for you. A write your own key authored, or that derives from one, is never served to it.</p>"#,
            ),
            Some(served) => {
                let serve_id = format!("srv_{}", served.memory.memory_id.simple());
                body.push_str(&memory_card(&served));
                body.push_str(&form(
                    path,
                    "verdict",
                    &csrf,
                    &format!(
                        "{}{}{}",
                        hidden("serve_id", &serve_id),
                        select("verdict", "verdict", &[("approve", "approve"), ("reject", "reject")]),
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
                self.admin_memories_view(&admin, query, &csrf).await?,
            ),
            "review" => ("/web/admin?view=review", review_start("/web/admin", &csrf)),
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
            r#"<p class="label">People</p><h1>Who signs in</h1><p class="lede">An address on this list signs in with Google or GitHub as its user. Removing it ends its sessions; the user's agent keys keep working until you cut them off.</p><table><tr><th>Email</th><th>User</th><th>Admin</th><th></th></tr>"#,
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
        body.push_str("</table><h2>Add an address</h2>");
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
        body.push_str("<h2>Review and curation</h2><p class=\"lede\">Every person reviews their own workarea from their first sign-in. Grant review elsewhere, or designate a curator who writes into the shared area; each act is a journalled grant.</p>");
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

    async fn queue_view(&self, admin: &oml_app::app::Caller) -> Result<String, ApiError> {
        let rows = self.app.web_queue(admin).await.map_err(app_error)?;
        let mut body = String::from(
            r#"<p class="label">Queue</p><h1>Waiting for review</h1><p class="lede">Held memories per workarea and the age of the oldest. No content: open a memory under Memories.</p><table><tr><th>Workarea</th><th>Held</th><th>Oldest since</th></tr>"#,
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
        query: &Fields,
        csrf: &str,
    ) -> Result<String, ApiError> {
        let mut body = format!(
            r#"<p class="label">Memories</p><h1>Every memory</h1><p class="lede">An administrator reads every memory of the organisation with who wrote, derived and reviewed it. Every read is recorded in the history.</p><form class="row" method="get" action="/web/admin"><input type="hidden" name="view" value="memories">{}{}{}<button type="submit">Search</button></form>"#,
            input("q", "search", query.get("q").unwrap_or(""), false),
            input(
                "workarea",
                "workarea",
                query.get("workarea").unwrap_or(""),
                false
            ),
            select(
                "state",
                "state",
                &[
                    ("", "any"),
                    ("held", "held"),
                    ("promoted", "promoted"),
                    ("rejected", "rejected"),
                    ("superseded", "superseded"),
                    ("demoted", "demoted")
                ]
            )
        );
        if let Some(id) = query.get("memory") {
            let id = Uuid::parse_str(id).map_err(|_| {
                ApiError::validation("memory is not an id").with_details(json!({"field": "memory"}))
            })?;
            let read = retry_chain(|| self.app.admin_read(Bearer::Session(admin), id))
                .await
                .map_err(app_error)?;
            body.push_str(&attribution_card(&read));
            body.push_str("<h2>Replace it</h2>");
            body.push_str(&form(
                "/web/admin",
                "supersede",
                csrf,
                &format!(
                    "{}{}{}",
                    hidden("memory", &read.memory.memory_id.to_string()),
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
                r#"<p><a href="/web/admin?view=curate&amp;memory={}">Curate it into the shared area</a></p>"#,
                read.memory.memory_id
            );
            return Ok(body);
        }
        let workarea = match query.get("workarea") {
            Some(w) => Some(Reference::parse(w).map_err(|e| {
                ApiError::validation(e.to_string()).with_details(json!({"field": "workarea"}))
            })?),
            None => None,
        };
        let state = query.get("state").and_then(AdminState::parse);
        let page = retry_chain(|| {
            self.app.admin_search(
                Bearer::Session(admin),
                query.get("q"),
                workarea.clone(),
                state,
                None,
                50,
            )
        })
        .await
        .map_err(app_error)?;
        body.push_str(
            "<table><tr><th>State</th><th>Workarea</th><th>Author</th><th>Text</th></tr>",
        );
        if page.items.is_empty() {
            body.push_str(r#"<tr><td colspan="4" class="dim">No memory matches.</td></tr>"#);
        }
        for m in &page.items {
            let _ = write!(
                body,
                r#"<tr><td>{}</td><td class="mono">{}</td><td class="mono">{}</td><td><a href="/web/admin?view=memories&amp;memory={}">{}</a></td></tr>"#,
                state_tag(m.state.as_str()),
                short(&m.workarea_path),
                author_line(m),
                m.memory.memory_id,
                short(&m.memory.body)
            );
        }
        body.push_str("</table>");
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
            r#"<p class="label">Curate</p><h1>Into the shared area</h1><p class="lede">Curate a memory someone other than its author has approved. The copy is written by your curator key, held until someone outside its lineage approves it, and served to everyone without its author or source. Reword it: a copy whose text or name repeats its source's can be linked to it.</p>"#,
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
            "curate" => {
                fields.only(&["source", "text", "claim_key"])?;
                let source = fields.id("source")?;
                let text_value = fields.need("text")?.to_owned();
                let curate = self.caller(session, Capacity::Curate).await?;
                let digest = oml_app::domain::Sha256Hash::digest(
                    format!("{source}\n{text_value}").as_bytes(),
                );
                let req = SubmitRequest {
                    idempotency_key: format!("web-curate-{}", &digest.to_prefixed()[7..39]),
                    claim_key: fields.get("claim_key").map(ToOwned::to_owned),
                    verb: Some(SubmitVerb::Learn),
                    kind: MemoryKind::Memory,
                    body: text_value,
                    workarea: Some(Reference::Name(
                        Name::parse(oml_app::domain::name::UNIVERSE).expect("a name"),
                    )),
                    derives_from: vec![source],
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
                fields.only(&["memory", "text", "retirement"])?;
                let target = fields.id("memory")?;
                let body = fields.need("text")?.to_owned();
                let retirement = fields.yes("retirement");
                let admin = self.caller(session, Capacity::Admin).await?;
                let read = retry_chain(|| self.app.admin_read(Bearer::Session(&admin), target))
                    .await
                    .map_err(app_error)?;
                let curate = self.caller(session, Capacity::Curate).await.ok();
                let acting = match curate {
                    Some(c) if read.memory.author_key_id == c.key().key_id => c,
                    _ => self.caller(session, Capacity::Review).await?,
                };
                let digest =
                    oml_app::domain::Sha256Hash::digest(format!("{target}\n{body}").as_bytes());
                let req = SupersedeRequest {
                    idempotency_key: format!("web-supersede-{}", &digest.to_prefixed()[7..39]),
                    target_id: target,
                    body,
                    kind: None,
                    retirement,
                    derives_from: Vec::new(),
                    reason: None,
                    workarea: None,
                    refuse_vocabulary_target: false,
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
            r#"<p class="label">Remove</p><h1>Remove {}?</h1><p class="lede">The address's sessions end at once. The keys of <code>{}</code>'s agents keep working: cut each off here if the person is leaving.</p>"#,
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
        r#"<p class="lede">Review serves the next memory held in the workareas your reviewer key reaches: your own from your first sign-in, and any other an administrator granted. You never review a write your reviewer key authored or that derives from one.</p>{}"#,
        form(
            path,
            "review_next",
            csrf,
            "",
            "Review the next memory",
            true
        )
    )
}

fn connect_card(key: &str, code: &oml_app::types::ConnectCodeIssued) -> String {
    format!(
        r#"<p class="label">Connect code</p><h1>{}</h1><p class="lede">In Claude, add a custom connector with this URL, then enter the code when the page asks. The code is shown once and works once, until {}.</p><div class="card secret"><p class="label">Connector URL</p><pre>{}</pre><p class="label">Connect code</p><pre>{}</pre></div><p><a href="/web">Back to your keys</a></p>"#,
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

fn memory_table(items: &[&ServedMemory], base: &str) -> String {
    let mut out =
        String::from("<table><tr><th>State</th><th>Workarea</th><th>From</th><th>Text</th></tr>");
    if items.is_empty() {
        out.push_str(r#"<tr><td colspan="4" class="dim">Nothing yet.</td></tr>"#);
    }
    for m in items {
        let from = if m.anonymous {
            "the shared area".to_owned()
        } else {
            short(&m.memory.author_principal)
        };
        let _ = write!(
            out,
            r#"<tr><td>{}</td><td class="mono">{}</td><td class="mono">{}</td><td><a href="{base}&amp;memory={}">{}</a></td></tr>"#,
            state_tag(memory_state(m)),
            short(m.workarea_path.as_deref().unwrap_or("")),
            from,
            m.memory.memory_id,
            short(&m.memory.body)
        );
    }
    out.push_str("</table>");
    out
}

fn memory_card(m: &ServedMemory) -> String {
    let from = if m.anonymous {
        "the shared area".to_owned()
    } else {
        short(&m.memory.author_principal)
    };
    format!(
        r#"<div class="card"><p class="label">{} · {} · version {}</p><p class="text">{}</p><p class="dim mono">{} · from {}</p></div>"#,
        state_tag(memory_state(m)),
        short(m.workarea_path.as_deref().unwrap_or("")),
        m.memory.version,
        text(&m.memory.body),
        short(&m.memory.claim_key),
        from
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

fn attribution_card(m: &AdminMemory) -> String {
    let mut out = format!(
        r#"<div class="card"><p class="label">{} · {} · version {}</p><p class="text">{}</p></div><h2>Attribution</h2><table><tr><th>Field</th><th>Value</th></tr>"#,
        state_tag(m.state.as_str()),
        short(&m.workarea_path),
        m.memory.version,
        text(&m.memory.body)
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
