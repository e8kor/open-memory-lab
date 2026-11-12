//! The onboarding page `/web/start` with its status poll and the person's
//! export, and the setup view `/web/setup` (ADR-0068 d10; ADR-0045 d9),
//! moved from the lead (ADR-0070 d7 (b)).
//!
//! - `GET /web/start` reads the person through `GET /v2/whoami?start=true`
//!   as `<user>-web`: whether an agent key connected, the newest memory of
//!   their root, their limits and a pending deletion request;
//! - step 2's button issues `<user>-claude-ai` (a connector key),
//!   `<user>-claude-code` or `<user>-agent` (`issueKey`), the next free
//!   name after it, `-2` to `-9`, where one is taken; the value or the
//!   connect code is shown once on the redirect target through the sealed
//!   cookie of [`crate::once`];
//! - the account's deletion request (`requestWebDeletion`) runs only with
//!   the word `delete` typed into its form;
//! - `GET /web/start/status` and `GET /web/start/export` are data: no
//!   session answers `404`, never a redirect.
//!
//! The page carries one inline script, its copy buttons and its status
//! poll, named by its digest in the page's own policy, the one page with a
//! script (ADR-0068 d10).

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::{IntoResponse as _, Response};
use http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use oml_page::digest::{base64_32, sha256};
use oml_page::web::{attr, glyph_svg, shell, text, user_tabs};
use oml_sdk::{Error, Person};
use serde_json::json;

use crate::middleware::OwnHeaders;
use crate::once::{self, Secret};
use crate::pages::Fields;
use crate::session::{PageCalls, Session, call, cookie};
use crate::{Capacity, SESSION_COOKIE, Web, acts, keys, outcome, usage};

/// The onboarding page.
pub const START_PATH: &str = "/web/start";
/// Its status poll, as JSON.
pub const STATUS_PATH: &str = "/web/start/status";
/// The person's export, a JSON attachment.
pub const EXPORT_PATH: &str = "/web/start/export";
/// The setup view, the server URL and the guides.
pub const SETUP_PATH: &str = "/web/setup";

const GUIDES: &str = "https://e8kor.github.io/open-memory-lab/guides";

/// The `/web/start` acts (ADR-0068 d10).
pub const START_ACTIONS: &[&str] = &[
    "issue_claude_ai",
    "issue_claude_code",
    "issue_other",
    "request_deletion",
];

/// What the onboarding page asks claude.ai to remember about the
/// connector, behind a copy button (ADR-0068 d10): the one copy of the
/// snippet the claude.ai guide quotes.
pub const PREFERENCES_SNIPPET: &str = "I use the Open Memory Lab connector as my long-term memory. Before answering about my projects, decisions or preferences, search it with oml_read. When we settle a fact, a decision or a finding worth keeping, save it with oml_write in one or two plain sentences. What you write waits for my review before anyone else can read it.";

/// The onboarding page's one script: the copy buttons, and the status poll
/// every five seconds until a first memory shows (ADR-0068 d10).
pub const START_SCRIPT: &str = "(function(){\
document.querySelectorAll('button[data-copy]').forEach(function(b){b.hidden=false;\
b.addEventListener('click',function(){var t=document.getElementById(b.getAttribute('data-copy'));\
if(!t||!navigator.clipboard)return;navigator.clipboard.writeText(t.textContent).then(function(){\
b.textContent='Copied';setTimeout(function(){b.textContent='Copy'},1500)})})});\
var w=document.getElementById('st-wait');if(!w)return;\
function el(i){return document.getElementById(i)}\
function show(j){if(j.connected){w.hidden=true;el('st-conn').hidden=false}\
el('st-held').hidden=j.first_memory!=='held';el('st-promoted').hidden=j.first_memory!=='promoted'}\
function poll(){fetch('/web/start/status',{credentials:'same-origin',cache:'no-store'})\
.then(function(r){return r.ok?r.json():null}).then(function(j){if(j){show(j);\
if(j.first_memory!=='promoted')setTimeout(poll,5000)}}).catch(function(){setTimeout(poll,10000)})}\
setTimeout(poll,5000)})();";

/// The page's policy: the pages' own, with the one hashed script and its
/// same-origin poll.
#[must_use]
pub fn start_csp() -> String {
    let digest = base64_32(&sha256(START_SCRIPT.as_bytes()));
    let digest = std::str::from_utf8(&digest).expect("base64 is ASCII");
    oml_page::web::content_security_policy(&[], None).replacen(
        " form-action",
        &format!(" script-src 'sha256-{digest}'; connect-src 'self'; form-action"),
        1,
    )
}

/// The client a step-2 button issues for (ADR-0068 d10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Client {
    ClaudeAi,
    ClaudeCode,
    Other,
}

impl Client {
    const ALL: [Self; 3] = [Self::ClaudeAi, Self::ClaudeCode, Self::Other];

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.slug() == value)
    }

    fn slug(self) -> &'static str {
        match self {
            Self::ClaudeAi => "claude-ai",
            Self::ClaudeCode => "claude-code",
            Self::Other => "other",
        }
    }

    fn shown(self) -> &'static str {
        match self {
            Self::ClaudeAi => "claude.ai, Desktop or phone",
            Self::ClaudeCode => "Claude Code",
            Self::Other => "Other MCP tools",
        }
    }

    fn action(self) -> &'static str {
        match self {
            Self::ClaudeAi => "issue_claude_ai",
            Self::ClaudeCode => "issue_claude_code",
            Self::Other => "issue_other",
        }
    }

    /// The key's name after the user's: `<user>-claude-ai`, …
    fn key_suffix(self) -> &'static str {
        match self {
            Self::ClaudeAi => "claude-ai",
            Self::ClaudeCode => "claude-code",
            Self::Other => "agent",
        }
    }

    fn of_action(action: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.action() == action)
    }
}

/// One value with a copy button, shown only where the script runs.
fn copy_block(id: &str, label: &str, value: &str) -> String {
    format!(
        r#"<p class="label">{}</p><pre id="{id}">{}</pre><p><button type="button" data-copy="{id}" hidden>Copy</button></p>"#,
        text(label),
        attr(value)
    )
}

/// Step 2's answer: the credential, shown once, with what to do with it.
fn credential_card(client: Client, secret: &Secret, server: &str) -> String {
    let mcp = format!("{server}/mcp");
    let mut out = String::from(r#"<div class="card secret">"#);
    match (client, secret) {
        (
            _,
            Secret::Code {
                code,
                connector_url,
                ..
            },
        ) => {
            out.push_str(&copy_block("cred-url", "Connector URL", connector_url));
            out.push_str(&copy_block(
                "cred-code",
                "Connect code, once, for ten minutes",
                code,
            ));
            out.push_str(r#"<p class="desc">Shown once · works once · expires in ten minutes.</p><ol><li>In claude.ai open Customize, then Connectors.</li><li>Add custom connector and paste the URL.</li><li>Connect, then enter the code on the page that opens.</li></ol>"#);
            out.push_str(&copy_block(
                "cred-prefs",
                "Optional: paste into claude.ai's personal preferences",
                PREFERENCES_SNIPPET,
            ));
        }
        (Client::ClaudeCode, Secret::Key { value, .. }) => {
            out.push_str(&copy_block("cred-key", "Your key, shown once", value));
            if server.is_empty() {
                let _ = write!(out, r#"<p class="desc">{}</p>"#, keys::NO_SERVER);
            }
            out.push_str(&copy_block(
                "cred-cmd",
                "In Claude Code",
                "/plugin marketplace add e8kor/open-memory-lab\n/plugin install oml@open-memory-lab",
            ));
            let url = if server.is_empty() {
                "the server's address as <code>url</code>".to_owned()
            } else {
                format!("<code>{}</code> as <code>url</code>", attr(server))
            };
            let _ = write!(
                out,
                r#"<p class="dim">The install asks for the plugin's options: paste the key as <code>api_key</code> and {url}. Then run <code>/reload-plugins</code>.</p>"#
            );
        }
        (_, Secret::Key { value, .. }) => {
            if server.is_empty() {
                let _ = write!(out, r#"<p class="desc">{}</p>"#, keys::NO_SERVER);
            } else {
                out.push_str(&copy_block("cred-url", "MCP URL", &mcp));
            }
            out.push_str(&copy_block(
                "cred-key",
                "Your key, shown once, as a Bearer token",
                value,
            ));
            let _ = write!(
                out,
                r#"<p class="dim">Send <code>Authorization: Bearer &lt;key&gt;</code>. Configurations for Codex, Gemini CLI, Cursor and others are in the <a href="{GUIDES}/other-harnesses/">agent kit guide</a>.</p>"#
            );
        }
    }
    out.push_str("</div>");
    out
}

/// The notice the page's own act names, from a closed set.
fn notice(fields: &Fields) -> Option<&'static str> {
    match fields.get("done")? {
        "requested" => Some("Your deletion request is with the administrator."),
        _ => None,
    }
}

fn head(h: &str, level: &str, desc: &str) -> String {
    format!(
        r#"<header class="oml-section-head"><div class="top"><{level}>{}</{level}></div><p class="desc">{}</p></header>"#,
        text(h),
        text(desc)
    )
}

/// The onboarding page's body: three steps, the person's limits, then the
/// account's acts, as the lead drew it.
fn render(
    person: &Person,
    client: Option<Client>,
    shown: Option<(&Secret, &str)>,
    csrf: &str,
    note: Option<&str>,
    denied: bool,
) -> String {
    let mut body = head("Connect an agent", "h1", "Three steps, about two minutes.");
    if let Some(note) = note {
        let _ = write!(
            body,
            r#"<p class="notice" role="status">{}</p>"#,
            text(note)
        );
    }
    if denied {
        body.push_str(&keys::denied_notice());
    }
    body.push_str(r#"<section class="card">"#);
    body.push_str(&head(
        "1. Choose your client",
        "h2",
        "Where will your agent run?",
    ));
    body.push_str(r#"<nav class="toggle" aria-label="Client">"#);
    for c in Client::ALL {
        let on = if Some(c) == client {
            r#" class="on" aria-current="page""#
        } else {
            ""
        };
        let _ = write!(
            body,
            r#"<a href="{START_PATH}?client={}"{on}>{}</a>"#,
            c.slug(),
            text(c.shown())
        );
    }
    body.push_str("</nav></section>");
    body.push_str(r#"<section class="card">"#);
    match (client, shown) {
        (None, _) => body.push_str(&head(
            "2. Get your credential",
            "h2",
            "Choose a client first.",
        )),
        (Some(c), None) if denied => {
            // The API refuses the issue for this session's key, now and for
            // good, so no button is drawn (ADR-0070 d2).
            body.push_str(&head(
                "2. Get your credential",
                "h2",
                &format!("No key can be issued for {} from this session.", c.shown()),
            ));
        }
        (Some(c), None) => {
            body.push_str(&head(
                "2. Get your credential",
                "h2",
                &format!("One button issues a key for {}.", c.shown()),
            ));
            let _ = write!(
                body,
                r#"<details class="oml-help"><summary>?</summary><p>The key can write only into your own space, and every write waits for your review. A connect code works once, for ten minutes; a key's value is shown once and never again.</p></details><form method="post" action="{START_PATH}"><input type="hidden" name="action" value="{}"><input type="hidden" name="csrf_token" value="{}">{}<p class="oml-consequence">Issues one key; it counts against your limit of keys.</p><button type="submit" class="primary">Issue my credential</button></form>"#,
                c.action(),
                attr(csrf),
                acts::nonce_field()
            );
        }
        (Some(c), Some((secret, server))) => {
            body.push_str(&head(
                "2. Get your credential",
                "h2",
                "Shown once: copy it now.",
            ));
            body.push_str(&credential_card(c, secret, server));
        }
    }
    body.push_str("</section>");
    let status = person.start.clone().unwrap_or(oml_sdk::StartStatus {
        connected: false,
        first_memory: None,
    });
    let first = status.first_memory.as_deref();
    let hide = |shown: bool| if shown { "" } else { " hidden" };
    body.push_str(r#"<section class="card">"#);
    body.push_str(&head(
        "3. Wait for the first connection",
        "h2",
        "Ask your agent to remember one thing about this project.",
    ));
    let _ = write!(
        body,
        r#"<ul aria-live="polite"><li id="st-wait"{}>{}Waiting for your first connection…</li><li id="st-conn"{}>{}Connected ✓</li><li id="st-held"{}>{}First memory held — <a href="{}">review it</a></li><li id="st-promoted"{}>{}First memory reviewed ✓</li></ul></section>"#,
        hide(!status.connected),
        glyph_svg("in-progress"),
        hide(status.connected),
        glyph_svg("promoted"),
        hide(first == Some("held")),
        glyph_svg("held"),
        oml_page::web::USER_REVIEW_PATH,
        hide(first == Some("promoted")),
        glyph_svg("promoted"),
    );
    if let Some(card) = usage::card(person) {
        body.push_str(&card);
    }
    body.push_str(r#"<section class="card">"#);
    body.push_str(&head(
        "Your account",
        "h2",
        "The operator's admins can read every memory through a journalled read.",
    ));
    let _ = write!(
        body,
        r#"<p class="oml-consequence">A JSON download of every memory in your space, held and rejected included, and your keys.</p><p><a href="{EXPORT_PATH}">Export my memories</a></p>"#
    );
    if person.deletion_requested_at.is_some() {
        body.push_str(r#"<p class="notice" role="status">Your deletion request is with the administrator. Your agents keep working until it is closed.</p>"#);
    } else {
        let _ = write!(
            body,
            r#"<form method="post" action="{START_PATH}"><input type="hidden" name="action" value="request_deletion"><input type="hidden" name="csrf_token" value="{}">{}<label>Type delete to confirm<input name="confirm" autocomplete="off" required></label><p class="oml-consequence">Your agents keep working until an admin acts. Closing the request disables your account and cuts off your keys; your memories stay on record until the operator erases them.</p><button type="submit">Request deletion</button></form>"#,
            attr(csrf),
            acts::nonce_field()
        );
    }
    body.push_str("</section>");
    body
}

/// The person `whoami?start=true` names, or the page that ends the view.
async fn person(web: &Web, session: &Session, calls: &PageCalls) -> Result<Person, acts::Refusal> {
    let credential = session.credential(Capacity::Web);
    let ids = calls.next_ids();
    match web.api.whoami_start(&call(&credential, &ids)).await {
        Ok(found) => found.person.ok_or_else(|| {
            Box::new(crate::pages::refused(
                StatusCode::NOT_FOUND,
                "access.not_found",
                "no such page here",
                calls,
            ))
        }),
        Err(e) => Err(Box::new(outcome::page(&e, calls, START_PATH))),
    }
}

/// `GET /web/start`: the onboarding page, and once, after an issue, the
/// credential it produced.
pub async fn page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let client = fields.get("client").and_then(Client::parse);
    let person = match person(&web, &session, &calls).await {
        Ok(person) => person,
        Err(response) => return *response,
    };
    let sealed = cookie(&headers, once::ONCE_COOKIE);
    let secret = sealed
        .as_deref()
        .and_then(|v| once::open(session.token(), START_PATH, v));
    let server = keys::server(&web).await;
    let body = render(
        &person,
        client,
        secret
            .as_ref()
            .zip(client)
            .map(|(s, _)| (s, server.as_str())),
        &session.csrf(),
        notice(&fields),
        keys::denied(&session),
    );
    let html = shell(
        "Start",
        &session.who(),
        Some(&session.csrf()),
        &user_tabs(session.admin),
        START_PATH,
        None,
        &body,
    )
    .replacen(
        "</body>",
        &format!("<script>{START_SCRIPT}</script></body>"),
        1,
    );
    let mut response = outcome::html(StatusCode::OK, html);
    if let Ok(csp) = HeaderValue::from_str(&start_csp()) {
        response
            .headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, csp);
        response.extensions_mut().insert(OwnHeaders);
    }
    if sealed.is_some() {
        keys::clearing(response)
    } else {
        response
    }
}

/// The idempotency key of the `n`th name an issue tries: the form's own
/// for the first, a digest of it and `n` after, so a resubmission replays
/// every attempt it made.
fn attempt_key(key: &str, n: u32) -> String {
    if n == 1 {
        return key.to_owned();
    }
    sha256(format!("{key}\n{n}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `POST /web/start`: an issue or the deletion request.
pub async fn act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    let (fields, action) = match acts::read(
        form,
        START_ACTIONS,
        |a, n| a == "request_deletion" && n == "confirm",
        &calls,
    ) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    if action == "request_deletion" && fields.given("confirm").map(str::trim) != Some("delete") {
        return acts::invalid("type delete to confirm", &calls);
    }
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(Capacity::Web);
    if action == "request_deletion" {
        let ids = calls.next_ids();
        return match web
            .api
            .request_web_deletion(&call(&credential, &ids), &key)
            .await
        {
            Ok(_) => acts::see_other(&format!("{START_PATH}?done=requested"), &[]),
            Err(e) => outcome::page(&e, &calls, START_PATH),
        };
    }
    let Some(client) = Client::of_action(action) else {
        return acts::invalid("action is not one of this page's actions", &calls);
    };
    let user = session.whoami.user.as_ref().map_or("", |u| u.name.as_str());
    let target = format!("{START_PATH}?client={}", client.slug());
    for n in 1..=9_u32 {
        let name = if n == 1 {
            format!("{user}-{}", client.key_suffix())
        } else {
            format!("{user}-{}-{n}", client.key_suffix())
        };
        let ids = calls.next_ids();
        let issued = web
            .api
            .issue_key(
                &call(&credential, &ids),
                &attempt_key(&key, n),
                &name,
                "writer",
                client == Client::ClaudeAi,
            )
            .await;
        match issued {
            // A replay shows neither: the first answer's cookie holds the
            // secret, and this one sets none over it.
            Ok(issued) if issued.replayed() => return acts::see_other(&target, &[]),
            Ok(issued) => {
                let code = issued
                    .connect_code
                    .and_then(|c| Some((c.code?, c.expires_at, c.connector_url)));
                let secret = match (issued.value, code) {
                    (Some(value), _) => Secret::Key {
                        name: issued.name,
                        value,
                        server: keys::server(&web).await,
                    },
                    (None, Some((code, expires_at, connector_url))) => Secret::Code {
                        name: issued.name,
                        code,
                        expires_at,
                        connector_url,
                    },
                    (None, None) => return keys::no_secret(&calls),
                };
                let sealed = once::seal(session.token(), START_PATH, &secret);
                return acts::see_other(&target, &[once::set_cookie(&sealed)]);
            }
            Err(Error::Api(e)) if e.code == "name.taken" => {}
            Err(e) => return outcome::page(&e, &calls, START_PATH),
        }
    }
    acts::invalid(
        "nine keys of this kind already exist; issue one on the Keys tab",
        &calls,
    )
}

/// A JSON refusal: the data routes answer no page and no redirect.
fn json_refused(status: StatusCode, code: &str, calls: &PageCalls) -> Response {
    (
        status,
        axum::Json(json!({
            "error": { "code": code, "message": "not found" },
            "request_id": calls.page_id(),
        })),
    )
        .into_response()
}

/// A data route's answer to a failed call: a credential the API does not
/// resolve is not found; a refusal keeps the API's status and code, and a
/// rate refusal its `Retry-After`; no answer is `502` (ADR-0070 d3 layer 7).
fn json_failed(e: &Error, calls: &PageCalls) -> Response {
    match e {
        Error::Api(api) if api.code == "auth.invalid" => {
            json_refused(StatusCode::NOT_FOUND, "access.not_found", calls)
        }
        Error::Api(api) => {
            let mut response = json_refused(
                StatusCode::from_u16(api.status).unwrap_or(StatusCode::BAD_GATEWAY),
                &api.code,
                calls,
            );
            if let Some(after) = api
                .retry_after
                .as_deref()
                .and_then(|v| HeaderValue::from_str(v).ok())
            {
                response.headers_mut().insert(header::RETRY_AFTER, after);
            }
            response
        }
        Error::Transport(oml_sdk::TransportError::Timeout) => {
            json_refused(StatusCode::GATEWAY_TIMEOUT, "unavailable", calls)
        }
        _ => json_refused(StatusCode::BAD_GATEWAY, "unavailable", calls),
    }
}

fn calls_of(calls: Option<Extension<Arc<PageCalls>>>) -> Arc<PageCalls> {
    calls.map_or_else(|| Arc::new(PageCalls::new("data")), |c| c.0)
}

/// The web credential the cookie's session derives, or the `404` of no
/// session. The route's one call is the session check (ADR-0070 d3 layer
/// 3): no separate lookup is spent on it.
fn web_credential(headers: &HeaderMap, calls: &PageCalls) -> Result<String, acts::Refusal> {
    cookie(headers, SESSION_COOKIE)
        .map(|token| oml_web_credential::capacity_credential(&token, Capacity::Web.as_str()))
        .ok_or_else(|| {
            Box::new(json_refused(
                StatusCode::NOT_FOUND,
                "access.not_found",
                calls,
            ))
        })
}

/// `GET /web/start/status`: what the page's poll reads, one API call.
pub async fn status(
    State(web): State<Web>,
    calls: Option<Extension<Arc<PageCalls>>>,
    headers: HeaderMap,
) -> Response {
    let calls = calls_of(calls);
    let credential = match web_credential(&headers, &calls) {
        Ok(credential) => credential,
        Err(response) => return *response,
    };
    let ids = calls.next_ids();
    match web.api.whoami_start(&call(&credential, &ids)).await {
        Ok(found) => {
            let Some(person) = found.person else {
                return json_refused(StatusCode::NOT_FOUND, "access.not_found", &calls);
            };
            let start = person.start.unwrap_or(oml_sdk::StartStatus {
                connected: false,
                first_memory: None,
            });
            axum::Json(json!({
                "connected": start.connected,
                "first_memory": start.first_memory,
            }))
            .into_response()
        }
        Err(e) => json_failed(&e, &calls),
    }
}

/// `GET /web/start/export`: everything the person holds, as the API
/// exports it, as an attachment; one API call.
pub async fn export(
    State(web): State<Web>,
    calls: Option<Extension<Arc<PageCalls>>>,
    headers: HeaderMap,
) -> Response {
    let calls = calls_of(calls);
    let credential = match web_credential(&headers, &calls) {
        Ok(credential) => credential,
        Err(response) => return *response,
    };
    let ids = calls.next_ids();
    match web.api.export_web_person(&call(&credential, &ids)).await {
        Ok(export) => {
            let mut response = axum::Json(export).into_response();
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"oml-export.json\""),
            );
            response
        }
        Err(e) => json_failed(&e, &calls),
    }
}

/// The setup view's server and the line under it: the address to give
/// agents, or with none known the named `no_server` line and no address
/// (ADR-0076 d6, d8).
fn setup_server(issuer: &str) -> (String, String) {
    if issuer.is_empty() {
        (String::new(), keys::NO_SERVER.to_owned())
    } else {
        (
            issuer.to_owned(),
            "This server's address: give each agent this URL with its key.".to_owned(),
        )
    }
}

/// `GET /web/setup`: the server an agent connects to and the guides.
pub async fn setup(State(web): State<Web>, Extension(session): Extension<Session>) -> Response {
    let issuer = keys::server(&web).await;
    let (issuer, url_line) = setup_server(&issuer);
    let body = format!(
        r#"<h1>Set up</h1><div class="card"><p class="label">Server</p><pre class="mono">{}</pre></div><p class="desc">{url_line}</p><h2>Claude Code</h2><p>Agent key · <a href="{GUIDES}/claude-code/">guide</a></p><h2>Claude on the web and on a phone</h2><p>Connector key · <a href="{GUIDES}/claude-ai-and-mobile/">guide</a></p><h2>Other agent tools</h2><p>Codex CLI, Cursor, Gemini CLI, VS Code, Claude Desktop · <a href="{GUIDES}/other-harnesses/">guide</a></p>"#,
        attr(&issuer)
    );
    outcome::html(
        StatusCode::OK,
        shell(
            "Your page",
            &session.who(),
            Some(&session.csrf()),
            &user_tabs(session.admin),
            SETUP_PATH,
            None,
            &body,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key's card with no server address known names `no_server` and shows
    /// no empty URL, neither as the plugin's `url` nor as an MCP URL
    /// (ADR-0076 d8).
    #[test]
    fn a_new_keys_card_with_no_server_address_names_no_server_and_shows_no_empty_url() {
        let key = |client| {
            let secret = Secret::Key {
                name: "alice-agent".to_owned(),
                value: "oml_value".to_owned(),
                server: String::new(),
            };
            credential_card(client, &secret, "")
        };
        for client in [Client::ClaudeCode, Client::Other] {
            let card = key(client);
            assert!(card.contains("no_server"), "{card}");
            assert!(card.contains("oml_value"), "{card}");
            assert!(!card.contains("<code></code>"), "{card}");
            assert!(!card.contains(r#"<pre id="cred-url">/mcp</pre>"#), "{card}");
        }
        let with = credential_card(
            Client::ClaudeCode,
            &Secret::Key {
                name: "alice-agent".to_owned(),
                value: "oml_value".to_owned(),
                server: "https://oml.example.com".to_owned(),
            },
            "https://oml.example.com",
        );
        assert!(!with.contains("no_server"), "{with}");
        assert!(with.contains("https://oml.example.com"), "{with}");
    }

    /// The setup view shows the server's own address as the URL to give
    /// agents, and with none known names `no_server` and shows no URL; it
    /// names no other server and no default (ADR-0076 d6, d8).
    #[test]
    fn the_setup_view_shows_the_servers_own_issuer_and_names_no_other_server() {
        let (shown, line) = setup_server("https://oml.example.com");
        assert_eq!(shown, "https://oml.example.com");
        assert!(
            line.contains("give each agent this URL with its key"),
            "{line}"
        );
        let (shown, line) = setup_server("");
        assert_eq!(shown, "");
        assert!(line.contains("no_server"), "{line}");
        for text in [&line, &setup_server("https://oml.example.com").1] {
            assert!(!text.to_lowercase().contains("default"), "{text}");
            assert!(!text.contains("doublegate"), "{text}");
        }
    }

    #[test]
    fn the_start_policy_names_the_one_script_and_its_poll_alone() {
        let csp = start_csp();
        assert_eq!(csp.matches("script-src 'sha256-").count(), 1, "{csp}");
        assert!(csp.contains("connect-src 'self'"), "{csp}");
        assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
        assert!(!csp.contains("unsafe"), "{csp}");
    }

    #[test]
    fn each_issue_attempt_has_a_key_of_its_own_and_the_first_is_the_forms() {
        assert_eq!(attempt_key("k", 1), "k");
        assert_ne!(attempt_key("k", 2), attempt_key("k", 3));
        assert_eq!(attempt_key("k", 2).len(), 64);
    }
}
