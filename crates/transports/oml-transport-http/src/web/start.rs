//! Open sign-up's pages (ADR-0068): the terms page and its act, the
//! onboarding page `/web/start` with its status poll, its acts and the
//! person's export, and the admin's users page. Each answer carries the
//! pages' headers; `/web/start` alone carries one inline script, hashed in
//! its own CSP, for its copy buttons and its status poll (d10).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Write as _;
use std::net::IpAddr;

use axum::Extension;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use serde_json::{Value, json};

use oml_app::app::{CallbackRefusal, Capacity, StartStatus, UserRow};
use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::signup::Quota;
use oml_app::domain::web::{
    Email, Provider, ProviderIdentity, SIGN_IN_TTL_SECS, SignInRefusal, csrf_matches, csrf_token,
};
use oml_app::domain::{MemoryStatus, Role};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{AppError, IssuedKey, WebSessionRecord};

use super::page::{STYLESHEET, attr, short};
use super::{
    Answer, append_cookie, clear_cookie, cookie, human_only, origin_refused, set_cookie,
    unique_fields, unix_now,
};
use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::oauth::page::{base64_32, escape, sha256};
use crate::oauth::{form_pairs, require_media, same_origin};
use crate::traffic::{ClientAddr, Rid, WebLine};
use crate::{ApiState, retry_chain};
use oml_app::ports::HandleSealer as _;

/// The pending sign-up's cookie: the admitted identity, sealed, for the
/// terms page's ten minutes (ADR-0068 d2).
pub const SIGN_UP_COOKIE: &str = "__Host-oml_signup";

/// The `/web/start` acts (ADR-0068 d10).
pub const START_ACTIONS: &[&str] = &[
    "issue_claude_ai",
    "issue_claude_code",
    "issue_other",
    "request_deletion",
];

/// The admin's users page acts (ADR-0068 d5, d6, d8).
pub const USERS_ACTIONS: &[&str] = &["set_quota", "disable_user", "close_deletion"];

/// What the onboarding page asks claude.ai to remember about the
/// connector, behind a copy button (ADR-0068 d10): the one copy of the
/// snippet the claude.ai guide quotes.
pub const PREFERENCES_SNIPPET: &str = "I use the Open Memory Lab connector as my long-term memory. Before answering about my projects, decisions or preferences, search it with oml_read. When we settle a fact, a decision or a finding worth keeping, save it with oml_write in one or two plain sentences. What you write waits for my review before anyone else can read it.";

/// The onboarding page's own styles: the step list, and a light scheme the
/// shared pages do not have.
pub const START_STYLESHEET: &str = "ol.steps{list-style:none;padding:0;margin:24px 0;display:grid;gap:16px}\
ol.steps>li{border:1px solid var(--rule);border-radius:12px;padding:18px 20px;background:var(--ground-alt)}\
ol.steps h2{margin:0 0 4px;font-size:18px}ol.steps p.line{margin:0 0 10px;color:var(--ink-dim)}\
.choices{display:flex;gap:8px;flex-wrap:wrap}.choices a{border:1px solid var(--rule);border-radius:999px;\
padding:6px 14px;color:var(--ink)}.choices a.on{border-color:var(--star);color:var(--star)}\
.copyrow{display:flex;gap:8px;align-items:flex-start}.copyrow pre{flex:1;margin:6px 0}\
.status{font-weight:600}.status.ok{color:var(--ok)}ol.clicks{margin:8px 0 0 18px;padding:0}\
@media (prefers-color-scheme:light){:root{--ground:#F7F4EC;--ground-alt:#FFFFFF;--panel:#FFFFFF;\
--ink:#14172E;--ink-dim:#3E4260;--ink-faint:#5F6380;--rule:#D6D2C4;--rule-soft:#E6E2D6;\
--ice:#1F5E91;--star:#B0600A;--ok:#1C7A55}button.primary{color:#FFFFFF}}";

/// The onboarding page's one script: the copy buttons, and the status poll
/// every five seconds until a first memory shows (ADR-0068 d10).
pub const START_SCRIPT: &str = "(function(){\
document.querySelectorAll('button[data-copy]').forEach(function(b){b.hidden=false;\
b.addEventListener('click',function(){var t=document.getElementById(b.getAttribute('data-copy'));\
if(!t||!navigator.clipboard)return;navigator.clipboard.writeText(t.textContent).then(function(){\
b.textContent='Copied';setTimeout(function(){b.textContent='Copy'},1500)})})});\
var c=document.getElementById('st-conn'),m=document.getElementById('st-mem');if(!c||!m)return;\
function show(j){if(j.connected){c.textContent='Connected \\u2713';c.className='status ok'}\
if(j.first_memory==='held'){m.hidden=false;m.querySelector('span').textContent='First memory held \\u2014 ';\
m.querySelector('a').hidden=false}else if(j.first_memory==='promoted'){m.hidden=false;\
m.querySelector('span').textContent='First memory reviewed \\u2713';m.querySelector('a').hidden=true}}\
function poll(){fetch('/web/start/status',{credentials:'same-origin',cache:'no-store'})\
.then(function(r){return r.ok?r.json():null}).then(function(j){if(j){show(j);\
if(j.first_memory!=='promoted')setTimeout(poll,5000)}}).catch(function(){setTimeout(poll,10000)})}\
setTimeout(poll,5000)})();";

const STYLE_DIGEST: [u8; 44] = base64_32(&sha256(STYLESHEET.as_bytes()));
const START_STYLE_DIGEST: [u8; 44] = base64_32(&sha256(START_STYLESHEET.as_bytes()));
const SCRIPT_DIGEST: [u8; 44] = base64_32(&sha256(START_SCRIPT.as_bytes()));

const GUIDES: &str = "https://e8kor.github.io/open-memory-lab/guides";
const FORM: &str = "application/x-www-form-urlencoded";

/// The CSP of these pages: the two stylesheets, the one script on
/// `/web/start`, its same-origin poll, forms to the page alone, no frame.
#[must_use]
pub fn start_csp(script: bool) -> String {
    let d = |b: &[u8; 44]| std::str::from_utf8(b).expect("base64 is ASCII").to_owned();
    let script = if script {
        format!(
            " script-src 'sha256-{}'; connect-src 'self';",
            d(&SCRIPT_DIGEST)
        )
    } else {
        String::new()
    };
    format!(
        "default-src 'none'; style-src 'sha256-{}' 'sha256-{}';{script} form-action 'self'; \
         frame-ancestors 'none'; base-uri 'none'",
        d(&STYLE_DIGEST),
        d(&START_STYLE_DIGEST)
    )
}

fn secured(mut response: Response, script: bool) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&start_csp(script)).expect("the CSP is ASCII"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn html_response(status: StatusCode, html: String, script: bool) -> Response {
    let mut response = Response::new(Body::from(html));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    secured(response, script)
}

fn json_answer(value: &Value, attachment: Option<&str>) -> Response {
    let mut response = Response::new(Body::from(value.to_string()));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    if let Some(name) = attachment
        && let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
    {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, v);
    }
    secured(response, false)
}

/// A document of these pages: both stylesheets, and the script only where
/// `script` is set.
fn document(
    title: &str,
    who: Option<&str>,
    tabs: &[(&str, &str)],
    body: &str,
    script: bool,
) -> String {
    let mut nav = String::new();
    if !tabs.is_empty() {
        nav.push_str(r#"<nav class="tabs">"#);
        for (href, shown) in tabs {
            let _ = write!(nav, r#"<a href="{href}">{}</a>"#, escape(shown));
        }
        nav.push_str("</nav>");
    }
    let script = if script {
        format!("<script>{START_SCRIPT}</script>")
    } else {
        String::new()
    };
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><meta name="color-scheme" content="dark light"><title>Open Memory Lab: {}</title><style>{STYLESHEET}</style><style>{START_STYLESHEET}</style></head><body><header><div class="wrap bar"><a class="brand" href="/web">Open <b>Memory</b> Lab</a><span class="who">{}</span></div></header><main class="wrap">{nav}{body}</main><footer><div class="wrap">Every act on these pages is journalled under one of your keys.</div></footer>{script}</body></html>"##,
        escape(title),
        who.map(short).unwrap_or_default()
    )
}

/// One value with a copy button, shown only where the script runs.
fn copy_block(id: &str, label: &str, value: &str) -> String {
    format!(
        r#"<p class="label">{}</p><div class="copyrow"><pre id="{id}">{}</pre><button type="button" data-copy="{id}" hidden>Copy</button></div>"#,
        escape(label),
        attr(value)
    )
}

/// The client a step-2 button issues for (ADR-0068 d10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    ClaudeAi,
    ClaudeCode,
    Other,
}

impl Client {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "claude-ai" => Some(Client::ClaudeAi),
            "claude-code" => Some(Client::ClaudeCode),
            "other" => Some(Client::Other),
            _ => None,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Client::ClaudeAi => "claude-ai",
            Client::ClaudeCode => "claude-code",
            Client::Other => "other",
        }
    }

    fn shown(self) -> &'static str {
        match self {
            Client::ClaudeAi => "claude.ai, Desktop or phone",
            Client::ClaudeCode => "Claude Code",
            Client::Other => "Other MCP tools",
        }
    }

    fn action(self) -> &'static str {
        match self {
            Client::ClaudeAi => "issue_claude_ai",
            Client::ClaudeCode => "issue_claude_code",
            Client::Other => "issue_other",
        }
    }

    /// The key's name after the user's: `<user>-claude-ai`, …
    fn key_suffix(self) -> &'static str {
        match self {
            Client::ClaudeAi => "claude-ai",
            Client::ClaudeCode => "claude-code",
            Client::Other => "agent",
        }
    }

    fn of_action(action: &str) -> Option<Self> {
        [Client::ClaudeAi, Client::ClaudeCode, Client::Other]
            .into_iter()
            .find(|c| c.action() == action)
    }
}

/// The pending sign-up the cookie seals (ADR-0068 d2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSignUp {
    pub identity: ProviderIdentity,
    pub started_at: i64,
}

impl PendingSignUp {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        json!({
            "p": self.identity.provider.as_str(),
            "s": self.identity.subject,
            "e": self.identity.email.as_str(),
            "t": self.started_at,
        })
        .to_string()
        .into_bytes()
    }

    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let v: Value = serde_json::from_slice(bytes).ok()?;
        Some(Self {
            identity: ProviderIdentity {
                provider: Provider::parse(v.get("p")?.as_str()?)?,
                subject: v.get("s")?.as_str()?.to_owned(),
                email: Email::parse(v.get("e")?.as_str()?).ok()?,
            },
            started_at: v.get("t")?.as_i64()?,
        })
    }

    fn live(&self, now: i64) -> bool {
        now >= self.started_at && now - self.started_at < SIGN_IN_TTL_SECS
    }
}

/// Sign-ups per client address in a rolling hour (ADR-0068 d9): the
/// process's own count, one bucket per address, and one shared bucket
/// where the address is unknown.
#[derive(Debug, Default)]
pub struct SignupLimiter(std::sync::Mutex<HashMap<Option<IpAddr>, VecDeque<i64>>>);

impl SignupLimiter {
    /// Takes one slot for `addr` at `now`, or the seconds until one frees.
    pub fn take(&self, addr: Option<IpAddr>, now: i64, per_hour: u64) -> Result<(), u64> {
        let mut map = self.0.lock().expect("signup limiter lock");
        let slots = map.entry(addr).or_default();
        while slots.front().is_some_and(|t| now - t >= 3600) {
            slots.pop_front();
        }
        if slots.len() as u64 >= per_hour {
            let oldest = slots.front().copied().unwrap_or(now);
            return Err(u64::try_from(3600 - (now - oldest)).unwrap_or(1).max(1));
        }
        slots.push_back(now);
        Ok(())
    }
}

fn status_word(status: Option<MemoryStatus>) -> Option<&'static str> {
    match status {
        Some(MemoryStatus::Held) => Some("held"),
        Some(MemoryStatus::Promoted) => Some("promoted"),
        _ => None,
    }
}

fn status_json(status: StartStatus) -> Value {
    json!({
        "connected": status.connected,
        "first_memory": status_word(status.first_memory),
    })
}

fn app_error(e: AppError) -> ApiError {
    map_app_error(e, ErrorContext::Write)
}

fn field_error(field: &str, message: &str) -> ApiError {
    ApiError::validation(message.to_owned()).with_details(json!({ "field": field }))
}

/// The tabs every one of these pages shows.
fn tabs(admin: bool) -> Vec<(&'static str, &'static str)> {
    let mut tabs = vec![
        ("/web/start", "Start"),
        ("/web", "Keys"),
        ("/web?view=review", "Review"),
    ];
    if admin {
        tabs.push(("/web/admin", "Administration"));
        tabs.push(("/web/admin/users", "Users"));
    }
    tabs
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    fn start_refused(error: &ApiError, line: WebLine) -> Answer {
        let mut answer = Answer {
            response: html_response(
                error.status,
                document(
                    "Not available",
                    None,
                    &[],
                    &format!(
                        r#"<h1>This cannot continue</h1><p class="error">{}: {}</p><p><a href="/web/start">Back</a></p>"#,
                        escape(error.code),
                        escape(&error.message)
                    ),
                    false,
                ),
                false,
            ),
            error_code: Some(error.code),
            line,
        };
        error.mark_quota(&mut answer.response);
        answer
    }

    // -- the terms page --------------------------------------------------------

    /// The callback's answer for an unlisted identity in open mode
    /// (ADR-0068 d2): the identity sealed into the sign-up cookie, and the
    /// terms page. Nothing is written.
    pub(crate) fn terms_page(&self, identity: ProviderIdentity) -> Response {
        let Some(policy) = self.app.signup_policy() else {
            return html_response(StatusCode::NOT_FOUND, String::new(), false);
        };
        let pending = PendingSignUp {
            identity,
            started_at: i64::try_from(unix_now()).unwrap_or(0),
        };
        let sealed = self.pages().signup_sealer.seal(&pending.encode());
        let body = format!(
            r#"<div class="signin"><p class="label">Create your account</p><h1>One step left</h1><p class="lede">You get a private space for your memories, a key for each agent, and a review page. Your agents' writes wait for your review there; the operator of this server can read what you store.</p><form method="post" action="/web/sign-up"><input type="hidden" name="terms_version" value="{}"><label style="flex-direction:row;gap:10px;align-items:flex-start;font:15px var(--sans);color:var(--ink)"><input type="checkbox" name="accept_terms" value="yes" required style="min-width:auto"> <span>I accept the <a href="{}">terms</a> (version {}) and have read the <a href="{}">privacy notice</a>.</span></label><button type="submit" class="primary">Create my account</button></form></div>"#,
            attr(&policy.terms_version),
            attr(&policy.terms_url),
            short(&policy.terms_version),
            attr(&policy.privacy_url),
        );
        let mut response = html_response(
            StatusCode::OK,
            document("Create your account", None, &[], &body, false),
            false,
        );
        append_cookie(
            &mut response,
            &set_cookie(SIGN_UP_COOKIE, &sealed, SIGN_IN_TTL_SECS),
        );
        response
    }

    /// `POST /web/sign-up` (ADR-0068 d2, d9): the same-origin check, the
    /// sealed identity, the accepted current terms, one slot of the
    /// address's hourly limit; then the entry, the keys and a session,
    /// and `303 /web/start`.
    pub(crate) async fn sign_up(
        State(st): State<Self>,
        rid: Rid,
        addr: Option<Extension<ClientAddr>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let addr = addr.and_then(|Extension(a)| a.0);
        let answer = st.sign_up_inner(&rid, &headers, &body, addr).await;
        Self::web_finish(&rid, answer)
    }

    async fn sign_up_inner(
        &self,
        request_id: &str,
        headers: &HeaderMap,
        body: &Bytes,
        addr: Option<IpAddr>,
    ) -> Answer {
        let line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return Self::start_refused(&refused, line);
        }
        let issuer = self.app.oauth_issuer().expect("routed only with an issuer");
        if !same_origin(headers, issuer) {
            return Self::start_refused(&origin_refused(), line);
        }
        if let Err(e) = require_media(headers, FORM) {
            return Self::start_refused(&ApiError::validation(e.description), line);
        }
        let fields: HashMap<String, String> =
            match form_pairs(body).and_then(|pairs| unique_fields(&pairs)) {
                Ok(f) => f.into_iter().collect(),
                Err(e) => return Self::start_refused(&ApiError::validation(e.description), line),
            };
        if let Some(name) = fields
            .keys()
            .find(|n| !matches!(n.as_str(), "accept_terms" | "terms_version"))
        {
            return Self::start_refused(
                &field_error(name, &format!("the terms page takes no field {name}")),
                line,
            );
        }
        let now = i64::try_from(unix_now()).unwrap_or(0);
        let Some(pending) = cookie(headers, SIGN_UP_COOKIE)
            .and_then(|sealed| self.pages().signup_sealer.open(&sealed))
            .and_then(|bytes| PendingSignUp::decode(&bytes))
            .filter(|p| p.live(now))
        else {
            let error = app_error(AppError::WebSignIn(SignInRefusal::StateMismatch));
            let mut answer = Self::start_refused(&error, line);
            append_cookie(&mut answer.response, &clear_cookie(SIGN_UP_COOKIE));
            return answer;
        };
        if fields.get("accept_terms").map(String::as_str) != Some("yes") {
            return Self::start_refused(
                &field_error("accept_terms", "accept the terms to create your account"),
                line,
            );
        }
        let Some(policy) = self.app.signup_policy() else {
            return Self::start_refused(&ApiError::not_found(), line);
        };
        let per_hour = policy.per_ip_per_hour;
        // A sign-up of an address an entry already holds is a sign-in,
        // and takes no slot (d9).
        let listed = self
            .store
            .web_entry_by_email(&pending.identity.email.digest())
            .await
            .ok()
            .flatten()
            .is_some();
        if !listed && let Err(retry) = self.pages().signups.take(addr, now, per_hour) {
            return Self::start_refused(
                &app_error(AppError::SignupRateLimited {
                    retry_after_secs: retry,
                }),
                line,
            );
        }
        let accepted = fields.get("terms_version").cloned().unwrap_or_default();
        let entry = match retry_chain(|| self.app.web_sign_up(&pending.identity, &accepted)).await {
            Ok(entry) => entry,
            Err(e) => return Self::start_refused(&app_error(e), line),
        };
        if let Err(e) = retry_chain(|| self.app.web_ensure_keys(&entry)).await {
            return Self::start_refused(&app_error(e), line);
        }
        let old = cookie(headers, super::SESSION_COOKIE);
        let opened = match self
            .app
            .web_session_open(&entry, self.pages().session_ttl_secs, old.as_deref())
            .await
        {
            Ok(opened) => opened,
            Err(e) => return Self::start_refused(&app_error(e), line),
        };
        let mut line = line;
        line.session(&opened.record);
        super::sign_in_line(
            request_id,
            pending.identity.provider,
            "signed_up",
            Some(&opened.record),
        );
        let mut response = self.redirect(
            "/web/start",
            Some(set_cookie(
                super::SESSION_COOKIE,
                &opened.token,
                self.pages().session_ttl_secs,
            )),
        );
        append_cookie(&mut response, &clear_cookie(SIGN_UP_COOKIE));
        Answer {
            response,
            error_code: None,
            line,
        }
    }

    // -- GET /web/start -------------------------------------------------------

    pub(crate) async fn start_page(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        uri: Uri,
    ) -> Response {
        let answer = st
            .start_page_inner(&headers, uri.query().unwrap_or(""))
            .await;
        Self::web_finish(&rid, answer)
    }

    async fn start_page_inner(&self, headers: &HeaderMap, query: &str) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return Self::start_refused(&refused, line);
        }
        let (token, session) = match self.session_of(headers).await {
            Err(e) => return Self::start_refused(&e, line),
            Ok(None) => {
                return Answer {
                    response: self.sign_in_page(StatusCode::OK, None),
                    error_code: None,
                    line,
                };
            }
            Ok(Some(found)) => found,
        };
        line.session(&session);
        let pairs = form_pairs(query.as_bytes()).unwrap_or_default();
        let client = pairs
            .iter()
            .find(|(n, _)| n == "client")
            .and_then(|(_, v)| Client::parse(v));
        match self
            .render_start(&token, &session, client, None, None)
            .await
        {
            Ok(html) => Answer {
                response: html_response(StatusCode::OK, html, true),
                error_code: None,
                line,
            },
            Err(e) => Self::start_refused(&e, line),
        }
    }

    /// The onboarding page: three steps, then the account's acts.
    async fn render_start(
        &self,
        token: &str,
        session: &WebSessionRecord,
        client: Option<Client>,
        issued: Option<(Client, &IssuedKey)>,
        notice: Option<&str>,
    ) -> Result<String, ApiError> {
        let web = self
            .app
            .web_caller(session, Capacity::Web)
            .await
            .map_err(app_error)?;
        let status = self.app.web_start_status(&web).await.map_err(app_error)?;
        let pending = self
            .app
            .web_deletion_pending(&web)
            .await
            .map_err(app_error)?;
        let csrf = csrf_token(token);
        let issuer = self
            .app
            .oauth_issuer()
            .map_or("", |i| i.as_str())
            .to_owned();
        let client = issued.map(|(c, _)| c).or(client);
        let mut body = String::from(
            r#"<p class="label">Start</p><h1>Connect your first agent</h1><p class="lede">Three steps, about two minutes.</p>"#,
        );
        if let Some(notice) = notice {
            let _ = write!(body, r#"<p class="notice">{}</p>"#, escape(notice));
        }
        body.push_str(r#"<ol class="steps">"#);
        // Step 1.
        body.push_str(r#"<li><h2>1. Choose your client</h2><p class="line">Where will your agent run?</p><div class="choices">"#);
        for c in [Client::ClaudeAi, Client::ClaudeCode, Client::Other] {
            let on = if Some(c) == client {
                r#" class="on""#
            } else {
                ""
            };
            let _ = write!(
                body,
                r#"<a href="/web/start?client={}"{on}>{}</a>"#,
                c.slug(),
                escape(c.shown())
            );
        }
        body.push_str("</div></li>");
        // Step 2.
        body.push_str(r#"<li><h2>2. Get your credential</h2>"#);
        match (client, issued) {
            (None, _) => body.push_str(r#"<p class="line">Choose a client first.</p>"#),
            (Some(c), None) => {
                let _ = write!(
                    body,
                    r#"<p class="line">One click issues a key for {}; it is shown once.</p><form method="post" action="/web/start"><input type="hidden" name="action" value="{}"><input type="hidden" name="csrf_token" value="{}"><button type="submit" class="primary">Issue my {} credential</button></form>"#,
                    escape(c.shown()),
                    c.action(),
                    attr(&csrf),
                    escape(c.shown())
                );
            }
            (Some(_), Some((c, key))) => body.push_str(&credential_card(c, key, &issuer)),
        }
        body.push_str("</li>");
        // Step 3.
        let (conn_class, conn_text) = if status.connected {
            ("status ok", "Connected \u{2713}")
        } else {
            ("status", "Waiting for your first connection\u{2026}")
        };
        let (mem_hidden, mem_text, link_hidden) = match status.first_memory {
            Some(MemoryStatus::Held) => ("", "First memory held \u{2014} ", ""),
            Some(MemoryStatus::Promoted) => ("", "First memory reviewed \u{2713}", " hidden"),
            _ => (" hidden", "", ""),
        };
        let _ = write!(
            body,
            r#"<li><h2>3. Say hello from your agent</h2><p class="line">Ask it to remember something; you review it here.</p><p id="st-conn" class="{conn_class}">{conn_text}</p><p id="st-mem" class="status"{mem_hidden}><span>{mem_text}</span><a href="/web?view=review"{link_hidden}>review it</a></p></li></ol>"#
        );
        // The account.
        let policy = self.app.signup_policy();
        let _ = write!(
            body,
            r#"<h2>Your account</h2><p><a href="/web/start/export">Export everything as JSON</a>{}</p>"#,
            policy.map_or(String::new(), |p| format!(
                r#" · <a href="{}">Terms</a> · <a href="{}">Privacy</a>"#,
                attr(&p.terms_url),
                attr(&p.privacy_url)
            ))
        );
        if pending {
            body.push_str(r#"<p class="notice">Your deletion request is with the administrator. Your agents keep working until it is closed.</p>"#);
        } else {
            let _ = write!(
                body,
                r#"<form class="row" method="post" action="/web/start"><input type="hidden" name="action" value="request_deletion"><input type="hidden" name="csrf_token" value="{}"><label>Type delete to ask for your account to be deleted<input name="confirm" required></label><button type="submit">Request deletion</button></form>"#,
                attr(&csrf)
            );
        }
        let who = session.user.as_str().to_owned();
        Ok(document(
            "Start",
            Some(&who),
            &tabs(session.admin),
            &body,
            true,
        ))
    }

    // -- POST /web/start and POST /web/admin/users -------------------------------

    pub(crate) async fn start_act(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let answer = st.form_act(&headers, &body, false).await;
        Self::web_finish(&rid, answer)
    }

    pub(crate) async fn users_act(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let answer = st.form_act(&headers, &body, true).await;
        Self::web_finish(&rid, answer)
    }

    /// One act of `/web/start` or `/web/admin/users`, checked as every page
    /// act is (ADR-0045 d1, d8): no `Authorization`, the same origin, a
    /// form, a live session (an admin's for the users page), its CSRF token
    /// and an action from the page's closed set.
    async fn form_act(&self, headers: &HeaderMap, body: &Bytes, users: bool) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return Self::start_refused(&refused, line);
        }
        let issuer = self.app.oauth_issuer().expect("routed only with an issuer");
        if !same_origin(headers, issuer) {
            return Self::start_refused(&origin_refused(), line);
        }
        if let Err(e) = require_media(headers, FORM) {
            return Self::start_refused(&ApiError::validation(e.description), line);
        }
        let fields: HashMap<String, String> =
            match form_pairs(body).and_then(|pairs| unique_fields(&pairs)) {
                Ok(f) => f.into_iter().collect(),
                Err(e) => return Self::start_refused(&ApiError::validation(e.description), line),
            };
        let (token, session) = match self.session_of(headers).await {
            Err(e) => return Self::start_refused(&e, line),
            Ok(Some((token, session))) if !users || session.admin => (token, session),
            Ok(_) if users => return Self::start_refused(&ApiError::not_found(), line),
            Ok(_) => {
                let expired = app_error(AppError::WebSignIn(SignInRefusal::SessionExpired));
                return Answer {
                    response: self.sign_in_page(expired.status, Some(&expired)),
                    error_code: Some(expired.code),
                    line,
                };
            }
        };
        line.session(&session);
        if !fields
            .get("csrf_token")
            .is_some_and(|sent| csrf_matches(&token, sent))
        {
            return Self::start_refused(
                &field_error(
                    "csrf_token",
                    "the form's csrf_token is absent or not this session's",
                ),
                line,
            );
        }
        let actions = if users { USERS_ACTIONS } else { START_ACTIONS };
        let Some(action) = fields
            .get("action")
            .and_then(|v| actions.iter().find(|a| **a == v.as_str()).copied())
        else {
            return Self::start_refused(
                &field_error("action", "action is not one of this page's actions"),
                line,
            );
        };
        line.action = Some(action);
        if let Err(closed) = self.require_writes_open() {
            return Self::start_refused(&closed, line);
        }
        let outcome = if users {
            self.users_page_act(&session, action, &fields).await
        } else {
            self.start_page_act(&token, &session, action, &fields).await
        };
        match outcome {
            Ok(Ok(html)) => Answer {
                response: html_response(StatusCode::OK, html, !users),
                error_code: None,
                line,
            },
            Ok(Err(location)) => Answer {
                response: self.redirect(&location, None),
                error_code: None,
                line,
            },
            Err(e) => Self::start_refused(&e, line),
        }
    }

    /// `Ok(Ok(page))` answers a page; `Ok(Err(location))` a redirect.
    async fn start_page_act(
        &self,
        token: &str,
        session: &WebSessionRecord,
        action: &str,
        fields: &HashMap<String, String>,
    ) -> Result<Result<String, String>, ApiError> {
        let only = |allowed: &[&str]| {
            fields
                .keys()
                .find(|n| {
                    !matches!(n.as_str(), "action" | "csrf_token") && !allowed.contains(&n.as_str())
                })
                .map_or(Ok(()), |n| {
                    Err(field_error(n, &format!("this action takes no field {n}")))
                })
        };
        let web = self
            .app
            .web_caller(session, Capacity::Web)
            .await
            .map_err(app_error)?;
        if action == "request_deletion" {
            only(&["confirm"])?;
            if fields.get("confirm").map(|v| v.trim()) != Some("delete") {
                return Err(field_error("confirm", "type delete to confirm"));
            }
            retry_chain(|| self.app.web_request_deletion(&web))
                .await
                .map_err(app_error)?;
            return Ok(Err("/web/start".to_owned()));
        }
        only(&[])?;
        let client = Client::of_action(action).expect("the closed set");
        let issued = self.issue_for(&web, client).await?;
        Ok(Ok(self
            .render_start(token, session, Some(client), Some((client, &issued)), None)
            .await?))
    }

    /// Issues `<user>-<suffix>`, or `-2` … `-9` after it where the name is
    /// taken, writer on the person's root: a connector key for claude.ai.
    async fn issue_for(
        &self,
        web: &oml_app::app::Caller,
        client: Client,
    ) -> Result<IssuedKey, ApiError> {
        let connector = client == Client::ClaudeAi;
        for n in 1..=9 {
            let suffix = if n == 1 {
                client.key_suffix().to_owned()
            } else {
                format!("{}-{n}", client.key_suffix())
            };
            match retry_chain(|| {
                self.app
                    .web_issue_key(web, &suffix, Role::Writer, connector)
            })
            .await
            {
                Ok(issued) => return Ok(issued),
                Err(AppError::NameTaken {
                    kind: NamedKind::Key,
                    ..
                }) => {}
                Err(e) => return Err(app_error(e)),
            }
        }
        Err(field_error(
            "name",
            "nine keys of this kind already exist; issue one on the Keys tab",
        ))
    }

    // -- GET /web/start/status and /web/start/export -------------------------------

    pub(crate) async fn start_status(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let answer = st.start_json(&headers, false).await;
        Self::web_finish(&rid, answer)
    }

    pub(crate) async fn start_export(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let answer = st.start_json(&headers, true).await;
        Self::web_finish(&rid, answer)
    }

    /// The status or the export, for a session alone: without one,
    /// `access.not_found` (ADR-0068 d7).
    async fn start_json(&self, headers: &HeaderMap, export: bool) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return Self::start_refused(&refused, line);
        }
        let session = match self.session_of(headers).await {
            Ok(Some((_, session))) => session,
            Ok(None) => return Self::start_refused(&ApiError::not_found(), line),
            Err(e) => return Self::start_refused(&e, line),
        };
        line.session(&session);
        let result = async {
            let web = self.app.web_caller(&session, Capacity::Web).await?;
            if export {
                self.app.web_export(&web).await
            } else {
                Ok(status_json(self.app.web_start_status(&web).await?))
            }
        }
        .await;
        match result {
            Ok(value) => Answer {
                response: json_answer(
                    &value,
                    export
                        .then(|| format!("oml-export-{}.json", session.user.as_str()))
                        .as_deref(),
                ),
                error_code: None,
                line,
            },
            Err(e) => Self::start_refused(&app_error(e), line),
        }
    }

    // -- GET /web/admin/users ------------------------------------------------------

    pub(crate) async fn users_page(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let answer = st.users_page_inner(&headers, None).await;
        Self::web_finish(&rid, answer)
    }

    async fn users_page_inner(&self, headers: &HeaderMap, notice: Option<&str>) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return Self::start_refused(&refused, line);
        }
        let (token, session) = match self.session_of(headers).await {
            Ok(Some((token, session))) if session.admin => (token, session),
            Ok(Some((_, session))) => {
                line.session(&session);
                return Self::start_refused(&ApiError::not_found(), line);
            }
            Ok(None) => return Self::start_refused(&ApiError::not_found(), line),
            Err(e) => return Self::start_refused(&e, line),
        };
        line.session(&session);
        match self.render_users(&token, &session, notice).await {
            Ok(html) => Answer {
                response: html_response(StatusCode::OK, html, false),
                error_code: None,
                line,
            },
            Err(e) => Self::start_refused(&e, line),
        }
    }

    async fn render_users(
        &self,
        token: &str,
        session: &WebSessionRecord,
        notice: Option<&str>,
    ) -> Result<String, ApiError> {
        let admin = self
            .app
            .web_caller(session, Capacity::Admin)
            .await
            .map_err(app_error)?;
        let users = self.app.web_users(&admin).await.map_err(app_error)?;
        let deletions = self.app.web_deletions(&admin).await.map_err(app_error)?;
        let csrf = csrf_token(token);
        let mut body = String::from(
            r#"<p class="label">Administration</p><h1>Users</h1><p class="lede">What each person holds against their quotas. Raising a quota, disabling a person and closing a deletion request are journalled under your admin key.</p>"#,
        );
        if let Some(notice) = notice {
            let _ = write!(body, r#"<p class="notice">{}</p>"#, escape(notice));
        }
        body.push_str("<h2>Deletion requests</h2>");
        if deletions.is_empty() {
            body.push_str(r#"<p class="dim">None pending.</p>"#);
        } else {
            body.push_str("<table><tr><th>user</th><th>requested</th><th></th></tr>");
            for d in &deletions {
                let _ = write!(
                    body,
                    r#"<tr><td class="mono">{}</td><td class="mono">{}</td><td><form method="post" action="/web/admin/users"><input type="hidden" name="action" value="close_deletion"><input type="hidden" name="csrf_token" value="{}"><input type="hidden" name="user" value="{}"><button type="submit">Disable and close</button></form></td></tr>"#,
                    short(d.user.as_str()),
                    short(d.requested_at.as_str()),
                    attr(&csrf),
                    attr(d.user.as_str())
                );
            }
            body.push_str("</table><p class=\"dim\">Closing disables the person and cuts off their keys; their memories stay until they are erased with <code>oml erase</code>.</p>");
        }
        body.push_str("<h2>People</h2><table><tr><th>user</th><th>memories</th><th>keys</th><th>connections</th><th>requests/min</th><th></th></tr>");
        for u in &users {
            body.push_str(&user_row(u, &csrf));
        }
        body.push_str("</table>");
        let quotas: Vec<(&str, &str)> = Quota::ALL
            .iter()
            .map(|q| (q.as_str(), q.as_str()))
            .collect();
        let mut options = String::new();
        for (v, t) in quotas {
            let _ = write!(options, r#"<option value="{v}">{t}</option>"#);
        }
        let _ = write!(
            body,
            r#"<h2>Set a quota</h2><form class="row" method="post" action="/web/admin/users"><input type="hidden" name="action" value="set_quota"><input type="hidden" name="csrf_token" value="{}"><label>user<input name="user" required></label><label>quota<select name="quota">{options}</select></label><label>limit, empty for the default<input name="limit" inputmode="numeric"></label><button type="submit" class="primary">Set</button></form>"#,
            attr(&csrf)
        );
        let who = session.user.as_str().to_owned();
        Ok(document("Users", Some(&who), &tabs(true), &body, false))
    }

    async fn users_page_act(
        &self,
        session: &WebSessionRecord,
        action: &str,
        fields: &HashMap<String, String>,
    ) -> Result<Result<String, String>, ApiError> {
        let allowed: &[&str] = match action {
            "set_quota" => &["user", "quota", "limit"],
            _ => &["user"],
        };
        if let Some(n) = fields.keys().find(|n| {
            !matches!(n.as_str(), "action" | "csrf_token") && !allowed.contains(&n.as_str())
        }) {
            return Err(field_error(n, &format!("this action takes no field {n}")));
        }
        let user = fields
            .get("user")
            .and_then(|u| Name::parse(u.trim()).ok())
            .ok_or_else(|| field_error("user", "user names a person"))?;
        let admin = self
            .app
            .web_caller(session, Capacity::Admin)
            .await
            .map_err(app_error)?;
        match action {
            "set_quota" => {
                let quota = fields
                    .get("quota")
                    .and_then(|q| Quota::parse(q))
                    .ok_or_else(|| {
                        field_error(
                            "quota",
                            "quota is one of memories, keys, connections, requests_per_minute",
                        )
                    })?;
                let limit = match fields
                    .get("limit")
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                {
                    None => None,
                    Some(l) => Some(l.parse::<u64>().map_err(|_| {
                        field_error(
                            "limit",
                            "a quota's limit is a whole number from 0 to 1000000",
                        )
                    })?),
                };
                retry_chain(|| self.app.web_set_quota(&admin, &user, quota, limit))
                    .await
                    .map_err(app_error)?;
            }
            "disable_user" => {
                retry_chain(|| self.app.web_disable_user(&admin, &user))
                    .await
                    .map_err(app_error)?;
            }
            _ => {
                retry_chain(|| self.app.web_close_deletion(&admin, &user))
                    .await
                    .map_err(app_error)?;
            }
        }
        Ok(Err("/web/admin/users".to_owned()))
    }

    /// The callback's redirect after an admitted sign-in (ADR-0068 d10):
    /// `/web/start` while the person holds no agent key, `/web` after.
    pub(crate) async fn landing(&self, entry: &oml_app::types::WebEntry) -> &'static str {
        match self.app.web_has_agent_key(entry).await {
            Ok(false) => "/web/start",
            _ => "/web",
        }
    }

    /// The callback's answer to an unlisted identity, or `None` where the
    /// refusal is another.
    pub(crate) fn unlisted(&self, refusal: &CallbackRefusal) -> Option<Response> {
        match refusal {
            CallbackRefusal::Unlisted(identity) => Some(self.terms_page(identity.clone())),
            _ => None,
        }
    }
}

fn user_row(u: &UserRow, csrf: &str) -> String {
    let cell = |used: u64, limit: u64| format!("{used} / {limit}");
    let flags = match (u.signed_up, u.disabled) {
        (_, true) => r#" <span class="tag dim">disabled</span>"#,
        (true, false) => r#" <span class="tag ok">signed up</span>"#,
        _ => "",
    };
    let disable = if u.disabled {
        String::new()
    } else {
        format!(
            r#"<form method="post" action="/web/admin/users"><input type="hidden" name="action" value="disable_user"><input type="hidden" name="csrf_token" value="{}"><input type="hidden" name="user" value="{}"><button type="submit">Disable</button></form>"#,
            attr(csrf),
            attr(u.user.as_str())
        )
    };
    format!(
        r#"<tr><td class="mono">{}{flags}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{disable}</td></tr>"#,
        short(u.user.as_str()),
        cell(u.usage.memories, u.quotas.memories),
        cell(u.usage.keys, u.quotas.keys),
        cell(u.usage.connections, u.quotas.connections),
        u.quotas.requests_per_minute,
    )
}

/// Step 2's answer: the credential, shown once, with what to do with it.
fn credential_card(client: Client, key: &IssuedKey, issuer: &str) -> String {
    let mcp = format!("{issuer}/mcp");
    let mut out = String::from(r#"<div class="card secret">"#);
    match client {
        Client::ClaudeAi => {
            let code = key.connect_code.as_ref();
            out.push_str(&copy_block(
                "cred-url",
                "Connector URL",
                code.map_or(&mcp, |c| &c.connector_url),
            ));
            out.push_str(&copy_block(
                "cred-code",
                "Connect code, once, for ten minutes",
                code.and_then(|c| c.code.as_deref()).unwrap_or(""),
            ));
            out.push_str(r#"<ol class="clicks"><li>In claude.ai open Customize, then Connectors.</li><li>Add custom connector and paste the URL.</li><li>Connect, then enter the code on the page that opens.</li></ol>"#);
            out.push_str(&copy_block(
                "cred-prefs",
                "Optional: paste into claude.ai's personal preferences",
                PREFERENCES_SNIPPET,
            ));
        }
        Client::ClaudeCode => {
            out.push_str(&copy_block("cred-key", "Your key, shown once", &key.value));
            out.push_str(&copy_block(
                "cred-cmd",
                "In Claude Code",
                "/plugin marketplace add e8kor/open-memory-lab\n/plugin install oml@open-memory-lab",
            ));
            let _ = write!(
                out,
                r#"<p class="dim">The install asks for the plugin's options: paste the key as <code>api_key</code> and <code>{}</code> as <code>url</code>. Then run <code>/reload-plugins</code>.</p>"#,
                attr(issuer)
            );
        }
        Client::Other => {
            out.push_str(&copy_block("cred-url", "MCP URL", &mcp));
            out.push_str(&copy_block(
                "cred-key",
                "Your key, shown once, as a Bearer token",
                &key.value,
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

/// The users page's map of users to rows, for tests.
#[must_use]
pub fn quota_cells(rows: &[UserRow]) -> BTreeMap<String, (u64, u64)> {
    rows.iter()
        .map(|r| {
            (
                r.user.as_str().to_owned(),
                (r.usage.memories, r.quotas.memories),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_csp_names_the_script_only_on_the_start_page() {
        assert!(start_csp(true).contains("script-src 'sha256-"));
        assert!(start_csp(true).contains("connect-src 'self'"));
        assert!(!start_csp(false).contains("script-src"));
        assert!(start_csp(false).starts_with("default-src 'none'"));
    }

    #[test]
    fn the_compile_time_digests_are_sha256() {
        for (text, digest) in [
            (START_SCRIPT, SCRIPT_DIGEST),
            (START_STYLESHEET, START_STYLE_DIGEST),
        ] {
            let d = ring::digest::digest(&ring::digest::SHA256, text.as_bytes());
            assert_eq!(base64_32(d.as_ref().try_into().unwrap()), digest);
        }
    }

    #[test]
    fn the_address_limit_admits_its_count_an_hour() {
        let limiter = SignupLimiter::default();
        let ip: Option<IpAddr> = Some("203.0.113.9".parse().unwrap());
        for _ in 0..5 {
            assert!(limiter.take(ip, 1000, 5).is_ok());
        }
        assert!(limiter.take(ip, 1000, 5).is_err());
        assert!(
            limiter
                .take(Some("203.0.113.10".parse().unwrap()), 1000, 5)
                .is_ok()
        );
        assert!(
            limiter.take(ip, 4600, 5).is_ok(),
            "an hour later a slot frees"
        );
    }

    #[test]
    fn a_pending_sign_up_round_trips_and_expires() {
        let p = PendingSignUp {
            identity: ProviderIdentity {
                provider: Provider::GitHub,
                subject: "7".to_owned(),
                email: Email::parse("a@example.com").unwrap(),
            },
            started_at: 100,
        };
        assert_eq!(PendingSignUp::decode(&p.encode()), Some(p.clone()));
        assert!(p.live(100 + SIGN_IN_TTL_SECS - 1));
        assert!(!p.live(100 + SIGN_IN_TTL_SECS));
    }
}
