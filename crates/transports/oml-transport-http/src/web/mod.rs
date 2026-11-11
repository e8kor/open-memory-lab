//! The web pages' edge (ADR-0045): the six routes of [`WEB_ROUTES`], the
//! sealed sign-in cookie, the session cookie, the CSRF token and the
//! same-origin check, and the `web_sign_in` log line. The routes exist
//! only where the deployment sets `oauth.issuer` and at least one provider;
//! unset, each is an unknown path. A follower routes none of them.
//!
//! Nothing an agent holds reaches a page (ADR-0045 d7): a request carrying
//! an `Authorization` header is refused before anything else is read, and
//! the API and `/mcp` never read the `Cookie` header, so a session cookie
//! there is `auth.missing`. No log line holds a code, a `state`, a cookie
//! or an email (d14): the request line names the route template, and the
//! sign-in's own line names its provider, outcome, tenant and user.
//!
//! The person's session keys rest on ADR-0044's user roots, not built
//! here: the user page shows who the person is and every action but
//! `sign_out` answers the app's `not_built` refusal.

pub mod admin_read;
pub mod emails;
pub mod page;
pub mod render;
pub mod start;
pub mod views;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::{MethodRouter, get, post};
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::app::{Callback, CallbackRefusal, WEB_SESSION_TTL_SECS};
use oml_app::domain::web::{Provider, SignInRefusal, SignInState, csrf_matches};
use oml_app::ports::{
    Clock, ConnectionStore, IdGen, JournalStore, Scanner, SignInExchange, Signer, WebStore,
};
use oml_app::types::{AppError, ProviderAnswer, ProviderFailure, WebSessionRecord};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::oauth::seal::{AeadSealer, WEB_SIGN_IN_PURPOSE};
use crate::oauth::{form_pairs, percent_encode, require_media, same_origin};
use crate::traffic::{Resolved, Rid, WebLine};
use crate::{ApiState, finish_coded};
use oml_app::ports::HandleSealer as _;

/// The web pages' routes, as `(METHOD, path)` (ADR-0045 d1, d25): a
/// closed set the router serves exactly where the pages are on.
pub const WEB_ROUTES: &[(&str, &str)] = &[
    ("GET", "/web"),
    ("POST", "/web"),
    ("GET", "/web/sign-in"),
    ("POST", "/web/sign-in"),
    ("GET", "/web/callback/{provider}"),
    ("GET", "/web/admin"),
    ("POST", "/web/admin"),
    // Open sign-up and onboarding (ADR-0068 d10).
    ("POST", "/web/sign-up"),
    ("GET", "/web/start"),
    ("POST", "/web/start"),
    ("GET", "/web/start/status"),
    ("GET", "/web/start/export"),
    ("GET", "/web/admin/users"),
    ("POST", "/web/admin/users"),
];

/// The user page's closed `action` set (ADR-0045 d1, d9, d28).
pub const USER_ACTIONS: &[&str] = &[
    "issue_key",
    "connect_key",
    "connect_code",
    "cut_off",
    "create_workarea",
    "workarea_mode",
    "workarea_transition",
    "workarea_ratify",
    "review_next",
    "verdict",
    "sign_out",
];

/// The admin page's closed `action` set (ADR-0045 d1, d10, d11, d12, d28;
/// ADR-0055 d10); `demote` joins it with its verb in v0.6.0.
pub const ADMIN_ACTIONS: &[&str] = &[
    "allow_email",
    "remove_email",
    "set_admin",
    "grant_review",
    "grant_curation",
    "review_next",
    "verdict",
    "bulk_confirm",
    "bulk_verdict",
    "curate",
    "supersede",
    "cut_off",
    "workarea_mode",
    "workarea_transition",
    "workarea_ratify",
];

/// The purpose a pending sign-up is sealed under, so no other sealed
/// value opens as one (ADR-0068 d2).
const SIGN_UP_PURPOSE: &[u8] = b"oml web sign-up";

/// The session cookie (ADR-0045 d5).
pub const SESSION_COOKIE: &str = "__Host-oml_session";
/// The sign-in-in-progress cookie (ADR-0045 d3).
pub const SIGN_IN_COOKIE: &str = "__Host-oml_signin";

/// How many exchanges with one provider run at once; past it a callback
/// answers `limit.rate` (ADR-0045 d15). Stated by the build, unmeasured
/// (#119).
pub const PROVIDER_CALLS_MAX: usize = 8;

/// The `details.reason` of an `Authorization` header on a web route.
pub const HUMAN_ONLY: &str = "web_is_human_only";

const FORM: &str = "application/x-www-form-urlencoded";

/// The pages' configuration and counters, built once by the composition:
/// the configured providers with their client ids, the exchange that
/// reaches them, the session lifetime, the sealer of the sign-in cookie
/// under its own purpose and in-memory key, and per provider the calls in
/// flight (ADR-0045 d15).
pub struct WebPages {
    providers: Vec<(Provider, String)>,
    exchange: Arc<dyn DynExchange>,
    session_ttl_secs: i64,
    sealer: AeadSealer,
    /// Seals a pending sign-up's identity for the terms page (ADR-0068 d2).
    signup_sealer: AeadSealer,
    /// Sign-ups per client address in the last hour (ADR-0068 d9).
    signups: start::SignupLimiter,
    /// Seals the frame route's tokens, which open for no other purpose
    /// (ADR-0056 d9).
    render_sealer: AeadSealer,
    /// The sources the frame route draws, held here and never in a token.
    render_sources: render::Sources,
    counters: [ProviderCounter; 2],
    /// Open sign-up as configured, which the composition hands the app
    /// (ADR-0068 d1).
    signup: Option<oml_app::types::SignupPolicy>,
}

#[derive(Default)]
struct ProviderCounter {
    in_flight: AtomicUsize,
}

impl std::fmt::Debug for WebPages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebPages")
            .field("providers", &self.providers)
            .field("session_ttl_secs", &self.session_ttl_secs)
            .finish_non_exhaustive()
    }
}

impl WebPages {
    /// The pages over `providers`, each with its client id, reached through
    /// `exchange`, with sessions living the shipped `web.session_ttl`.
    #[must_use]
    pub fn new<X: SignInExchange + 'static>(
        providers: Vec<(Provider, String)>,
        exchange: X,
    ) -> Self {
        Self {
            providers,
            exchange: Arc::new(exchange),
            session_ttl_secs: WEB_SESSION_TTL_SECS,
            sealer: AeadSealer::generate_for(WEB_SIGN_IN_PURPOSE),
            signup_sealer: AeadSealer::generate_for(SIGN_UP_PURPOSE),
            signups: start::SignupLimiter::default(),
            render_sealer: AeadSealer::generate_for(render::RENDER_PURPOSE),
            render_sources: render::Sources::default(),
            counters: Default::default(),
            signup: None,
        }
    }

    /// The pages with open sign-up configured (ADR-0068 d1).
    #[must_use]
    pub fn with_signup(mut self, policy: oml_app::types::SignupPolicy) -> Self {
        self.signup = Some(policy);
        self
    }

    /// The configured sign-up, which the composition hands the app.
    #[must_use]
    pub fn signup_policy(&self) -> Option<&oml_app::types::SignupPolicy> {
        self.signup.as_ref()
    }

    /// A session lifetime other than the shipped default.
    #[must_use]
    pub fn with_session_ttl(mut self, secs: i64) -> Self {
        self.session_ttl_secs = secs;
        self
    }

    fn provider_list(&self) -> Vec<Provider> {
        self.providers.iter().map(|(p, _)| *p).collect()
    }

    fn client_id(&self, provider: Provider) -> Option<&str> {
        self.providers
            .iter()
            .find(|(p, _)| *p == provider)
            .map(|(_, id)| id.as_str())
    }

    fn counter(&self, provider: Provider) -> &ProviderCounter {
        &self.counters[match provider {
            Provider::Google => 0,
            Provider::GitHub => 1,
        }]
    }

    /// `/readyz`'s `web` section: the configured providers, by name. Their
    /// failed exchanges are counted on the metrics listener,
    /// `oml_web_exchanges_failed_total`, never here: this probe answers
    /// anyone (ADR-0045 d18; ADR-0054 d1).
    #[must_use]
    pub fn readyz(&self) -> Value {
        json!({ "providers": self.provider_list().iter().map(|p| p.as_str()).collect::<Vec<_>>() })
    }

    /// One exchange under the provider's cap on concurrent calls.
    async fn redeem(
        &self,
        provider: Provider,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<ProviderAnswer, ProviderFailure> {
        let counter = self.counter(provider);
        if counter.in_flight.fetch_add(1, Ordering::SeqCst) >= PROVIDER_CALLS_MAX {
            counter.in_flight.fetch_sub(1, Ordering::SeqCst);
            return Err(ProviderFailure::Busy);
        }
        let answer = self
            .exchange
            .redeem_dyn(provider, code, verifier, redirect_uri)
            .await;
        counter.in_flight.fetch_sub(1, Ordering::SeqCst);
        answer
    }
}

/// The exchange as the app's port sees it: the pages' cap in front of the
/// composition's adapter.
struct Capped<'a>(&'a WebPages);

impl SignInExchange for Capped<'_> {
    async fn redeem(
        &self,
        provider: Provider,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<ProviderAnswer, ProviderFailure> {
        self.0.redeem(provider, code, verifier, redirect_uri).await
    }
}

type RedeemFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderAnswer, ProviderFailure>> + Send + 'a>>;

trait DynExchange: Send + Sync {
    fn redeem_dyn<'a>(
        &'a self,
        provider: Provider,
        code: &'a str,
        verifier: &'a str,
        redirect_uri: &'a str,
    ) -> RedeemFuture<'a>;
}

impl<X: SignInExchange> DynExchange for X {
    fn redeem_dyn<'a>(
        &'a self,
        provider: Provider,
        code: &'a str,
        verifier: &'a str,
        redirect_uri: &'a str,
    ) -> RedeemFuture<'a> {
        Box::pin(self.redeem(provider, code, verifier, redirect_uri))
    }
}

/// A web answer and what the request line learns from it (ADR-0045 d14).
struct Answer {
    response: Response,
    error_code: Option<&'static str>,
    line: WebLine,
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `router` with [`WEB_ROUTES`] where `oauth.issuer` and the pages are
    /// set, else unchanged, so each is an unknown path (ADR-0045 d1, d17).
    pub(crate) fn with_web_routes(&self, router: Router<Self>) -> Router<Self> {
        if self.app.oauth_issuer().is_none() || self.web.is_none() {
            return router;
        }
        let router = WEB_ROUTES.iter().fold(router, |router, &(method, path)| {
            router.route(path, Self::web_handler(method, path))
        });
        render::RENDER_ROUTES
            .iter()
            .fold(router, |router, &(_, path)| match path {
                "/web/render/{token}" => router.route(path, get(Self::render_frame)),
                _ => router.route(path, get(Self::render_asset)),
            })
    }

    /// What a page may frame: the render route under the issuer, where the
    /// pages are served (ADR-0056 d9).
    pub(crate) fn frame_src(&self) -> Option<String> {
        self.app
            .oauth_issuer()
            .map(|issuer| render::frame_src(issuer.as_str()))
    }

    /// The iframe a card embeds for a Mermaid claim's source, bound to the
    /// session by its CSRF token, or nothing where no issuer is set.
    pub(crate) fn render_iframe(&self, csrf: &str, memory_id: &str, source: &str) -> String {
        if self.app.oauth_issuer().is_none() {
            return String::new();
        }
        render::iframe(&render::token(
            &self.pages().render_sealer,
            &self.pages().render_sources,
            csrf,
            memory_id,
            source,
            unix_now(),
        ))
    }

    // -- GET /web/render/{token}, GET /web/assets/{file} -----------------------

    async fn render_frame(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        Path(token): Path<String>,
    ) -> Response {
        let answer = st.render_frame_inner(&headers, &token).await;
        Self::web_finish(&rid, answer)
    }

    /// The frame's document for a token this session holds, else
    /// `access.not_found`: a forged, expired or another session's token
    /// answers alike (ADR-0056 d9).
    async fn render_frame_inner(&self, headers: &HeaderMap, token: &str) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return self.refused(&refused, line);
        }
        let session = match self.session_of(headers).await {
            Ok(Some(found)) => found,
            Ok(None) => return self.refused(&ApiError::not_found(), line),
            Err(e) => return self.refused(&e, line),
        };
        line.session(&session.1);
        let csrf = oml_app::domain::web::csrf_token(&session.0);
        let source = render::open(
            &self.pages().render_sealer,
            &self.pages().render_sources,
            token,
            &csrf,
            unix_now(),
        );
        match (source, self.app.oauth_issuer()) {
            (Some(source), Some(issuer)) => Answer {
                response: render::frame_response(issuer.as_str(), &source),
                error_code: None,
                line,
            },
            _ => self.refused(&ApiError::not_found(), line),
        }
    }

    async fn render_asset(State(_): State<Self>, Path(file): Path<String>) -> Response {
        if file == format!("mermaid-{}.js", render::BUNDLE_SHA256) {
            render::bundle_response()
        } else {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NOT_FOUND;
            response
        }
    }

    fn web_handler(method: &str, path: &str) -> MethodRouter<Self> {
        match (method, path) {
            ("GET", "/web") => get(Self::user_page),
            ("POST", "/web") => post(Self::user_act),
            ("GET", "/web/sign-in") => get(Self::sign_in_get),
            ("POST", "/web/sign-in") => post(Self::sign_in),
            ("GET", "/web/callback/{provider}") => get(Self::callback),
            ("GET", "/web/admin") => get(Self::admin_page),
            ("POST", "/web/admin") => post(Self::admin_act),
            ("POST", "/web/sign-up") => post(Self::sign_up),
            ("GET", "/web/start") => get(Self::start_page),
            ("POST", "/web/start") => post(Self::start_act),
            ("GET", "/web/start/status") => get(Self::start_status),
            ("GET", "/web/start/export") => get(Self::start_export),
            ("GET", "/web/admin/users") => get(Self::users_page),
            ("POST", "/web/admin/users") => post(Self::users_act),
            _ => panic!("{method} {path} is in WEB_ROUTES with no handler"),
        }
    }

    fn pages(&self) -> &WebPages {
        self.web
            .as_deref()
            .expect("the web routes are routed only with the pages set")
    }

    /// The request line's fields for a web answer: `credential: session`
    /// and the tenant once a session resolved, the action and the provider
    /// (ADR-0045 d14).
    fn web_finish(rid: &str, answer: Answer) -> Response {
        let mut response = finish_coded(rid, answer.response, answer.error_code);
        if let Some(resolved) = response.extensions_mut().get_mut::<Resolved>() {
            resolved.web = Some(answer.line);
        }
        response
    }

    fn page(&self, status: StatusCode, html: String) -> Response {
        page::page_response(
            status,
            html,
            &self.pages().provider_list(),
            self.frame_src().as_deref(),
        )
    }

    fn refusal_page(&self, error: &ApiError) -> Response {
        self.page(error.status, page::refusal(error.code, &error.message))
    }

    fn refused(&self, error: &ApiError, line: WebLine) -> Answer {
        let mut response = self.refusal_page(error);
        error.mark_quota(&mut response);
        Answer {
            response,
            error_code: Some(error.code),
            line,
        }
    }

    /// The sign-in page, with a refusal above the buttons when there is one.
    fn sign_in_page(&self, status: StatusCode, refusal: Option<&ApiError>) -> Response {
        self.sign_in_page_to(status, refusal, "/web")
    }

    /// The sign-in page whose forms carry `next`, already admitted
    /// (ADR-0070 d4).
    fn sign_in_page_to(
        &self,
        status: StatusCode,
        refusal: Option<&ApiError>,
        next: &str,
    ) -> Response {
        let providers = self.pages().provider_list();
        let html = page::sign_in(
            &providers,
            refusal.map(|e| (e.code, e.message.as_str())),
            next,
        );
        page::page_response(status, html, &providers, self.frame_src().as_deref())
    }

    /// `next` on the configured public origin, never on a request header
    /// (ADR-0070 d4); `/web` stays the relative redirect it always was.
    fn next_location(&self, next: &str) -> String {
        match self.app.oauth_issuer() {
            Some(issuer) if next != "/web" => format!("{}{next}", issuer.as_str()),
            _ => "/web".to_owned(),
        }
    }

    /// The live session the cookie names, or `None`.
    async fn session_of(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<(String, WebSessionRecord)>, ApiError> {
        let Some(token) = cookie(headers, SESSION_COOKIE) else {
            return Ok(None);
        };
        let session = self
            .app
            .web_session(&token)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(session.map(|s| (token, s)))
    }

    // -- GET /web ---------------------------------------------------------------

    async fn user_page(State(st): State<Self>, rid: Rid, headers: HeaderMap, uri: Uri) -> Response {
        let answer = st
            .user_page_inner(&headers, uri.query().unwrap_or(""))
            .await;
        Self::web_finish(&rid, answer)
    }

    /// The query's views, each field at most once.
    fn query_fields(query: &str) -> Result<views::Fields, ApiError> {
        form_pairs(query.as_bytes())
            .and_then(|pairs| unique_fields(&pairs))
            .map(views::Fields::of)
            .map_err(|e| ApiError::validation(e.description))
    }

    async fn user_page_inner(&self, headers: &HeaderMap, query: &str) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return self.refused(&refused, line);
        }
        // A view oml-web serves at its own path: a 303 to that fixed path,
        // carrying nothing from the request (ADR-0070 d7 (b) as amended).
        if let Some(path) = views::moved_view(query).or_else(|| views::moved_memories(query, false))
        {
            return Answer {
                response: self.redirect(path, None),
                error_code: None,
                line,
            };
        }
        match self.session_of(headers).await {
            Err(e) => self.refused(&e, line),
            Ok(None) => Answer {
                response: self.sign_in_page(StatusCode::OK, None),
                error_code: None,
                line,
            },
            Ok(Some((token, session))) => {
                line.session(&session);
                let fields = Self::query_fields(query);
                let html = match fields {
                    Ok(q) => self.user_view(&token, &session, &q).await,
                    Err(e) => Err(e),
                };
                match html {
                    Ok(html) => Answer {
                        response: self.view_response(html),
                        error_code: None,
                        line,
                    },
                    Err(e) => self.refused(&e, line),
                }
            }
        }
    }

    // -- GET /web/admin -------------------------------------------------------

    async fn admin_page(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        uri: Uri,
    ) -> Response {
        let answer = st
            .admin_page_inner(&headers, uri.query().unwrap_or(""))
            .await;
        Self::web_finish(&rid, answer)
    }

    /// Any session but an admin entry's, and no session, answers
    /// `access.not_found`, so the page's existence confirms nothing
    /// (ADR-0045 d10).
    async fn admin_page_inner(&self, headers: &HeaderMap, query: &str) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return self.refused(&refused, line);
        }
        // A view oml-web serves at its own path: a 303 to that fixed path,
        // carrying nothing from the request (ADR-0070 d7 (b) as amended).
        if let Some(path) = views::moved_view_of(views::MOVED_ADMIN_VIEWS, query)
            .or_else(|| views::moved_memories(query, true))
        {
            return Answer {
                response: self.redirect(path, None),
                error_code: None,
                line,
            };
        }
        match self.session_of(headers).await {
            Err(e) => self.refused(&e, line),
            Ok(Some((token, session))) if session.admin => {
                line.session(&session);
                let fields = Self::query_fields(query);
                let html = match fields {
                    Ok(q) => self.admin_view(&token, &session, &q).await,
                    Err(e) => Err(e),
                };
                match html {
                    Ok(html) => Answer {
                        response: self.view_response(html),
                        error_code: None,
                        line,
                    },
                    Err(e) => self.refused(&e, line),
                }
            }
            Ok(other) => {
                if let Some((_, session)) = &other {
                    line.session(session);
                }
                self.refused(&ApiError::not_found(), line)
            }
        }
    }

    /// A view's page under d8's policy: no view runs script (ADR-0045
    /// d32), the Atlas included.
    fn view_response(&self, html: String) -> Response {
        self.page(StatusCode::OK, html)
    }

    // -- POST /web and POST /web/admin ------------------------------------------

    async fn user_act(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let answer = st.act(&headers, &body, false).await;
        Self::web_finish(&rid, answer)
    }

    async fn admin_act(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let answer = st.act(&headers, &body, true).await;
        Self::web_finish(&rid, answer)
    }

    /// One act of a page (ADR-0045 d1, d8): no `Authorization` header, the
    /// same-origin check, the form decoded once, a live session (an admin
    /// one on the admin page), the session's `csrf_token`, and an `action`
    /// from the page's closed set with no field it does not define. Every
    /// refusal before the act journals nothing.
    async fn act(&self, headers: &HeaderMap, body: &Bytes, admin_page: bool) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return self.refused(&refused, line);
        }
        let issuer = self.app.oauth_issuer().expect("routed only with an issuer");
        if !same_origin(headers, issuer) {
            return self.refused(&origin_refused(), line);
        }
        if let Err(e) = require_media(headers, FORM) {
            return self.refused(
                &ApiError::validation(e.description).with_details(json!({"field": "content-type"})),
                line,
            );
        }
        let fields = match form_pairs(body).and_then(|pairs| unique_fields(&pairs)) {
            Ok(fields) => fields,
            Err(e) => return self.refused(&ApiError::validation(e.description), line),
        };
        let session = match self.session_of(headers).await {
            Err(e) => return self.refused(&e, line),
            Ok(Some((token, session))) if !admin_page || session.admin => (token, session),
            Ok(Some((_, session))) => {
                line.session(&session);
                return self.refused(&ApiError::not_found(), line);
            }
            Ok(None) if admin_page => return self.refused(&ApiError::not_found(), line),
            Ok(None) => {
                let expired = map_app_error(
                    AppError::WebSignIn(SignInRefusal::SessionExpired),
                    ErrorContext::Write,
                );
                return Answer {
                    response: self.sign_in_page(expired.status, Some(&expired)),
                    error_code: Some(expired.code),
                    line,
                };
            }
        };
        let (token, session) = session;
        line.session(&session);
        let sent = fields
            .iter()
            .find(|(name, _)| name == "csrf_token")
            .map(|(_, v)| v.as_str());
        if !sent.is_some_and(|sent| csrf_matches(&token, sent)) {
            return self.refused(
                &ApiError::validation("the form's csrf_token is absent or not this session's")
                    .with_details(json!({"field": "csrf_token"})),
                line,
            );
        }
        let actions = if admin_page {
            ADMIN_ACTIONS
        } else {
            USER_ACTIONS
        };
        let Some(action) = fields
            .iter()
            .find(|(name, _)| name == "action")
            .and_then(|(_, v)| actions.iter().find(|a| **a == v.as_str()).copied())
        else {
            return self.refused(
                &ApiError::validation("action is not one of this page's actions")
                    .with_details(json!({"field": "action"})),
                line,
            );
        };
        line.action = Some(action);
        if action == "sign_out" {
            if let Some((name, _)) = fields
                .iter()
                .find(|(name, _)| name != "action" && name != "csrf_token")
            {
                return self.refused(
                    &ApiError::validation(format!("sign_out takes no field {name}"))
                        .with_details(json!({"field": name})),
                    line,
                );
            }
            if let Err(e) = self.app.web_sign_out(&session).await {
                return self.refused(&map_app_error(e, ErrorContext::Write), line);
            }
            return Answer {
                response: self.redirect("/web", Some(clear_cookie(SESSION_COOKIE))),
                error_code: None,
                line,
            };
        }
        // Every other act runs as one of the person's session keys.
        if let Err(closed) = self.require_writes_open() {
            return self.refused(&closed, line);
        }
        let fields = views::Fields::of(fields);
        let outcome = if admin_page {
            self.admin_page_act(&token, &session, action, &fields).await
        } else {
            self.user_page_act(&token, &session, action, &fields).await
        };
        match outcome {
            Ok(views::Outcome::Redirect(location)) => Answer {
                response: self.redirect(&location, None),
                error_code: None,
                line,
            },
            Ok(views::Outcome::Page(html)) => Answer {
                response: self.page(StatusCode::OK, html),
                error_code: None,
                line,
            },
            Err(e) => self.refused(&e, line),
        }
    }

    // -- GET /web/sign-in -------------------------------------------------------

    async fn sign_in_get(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        uri: Uri,
    ) -> Response {
        let answer = st
            .sign_in_get_inner(&headers, uri.query().unwrap_or(""))
            .await;
        Self::web_finish(&rid, answer)
    }

    /// The sign-in page, where `oml-web` sends a person with no session,
    /// carrying the page as `next` (ADR-0070 d4, d6). A browser already
    /// signed in goes straight on to `next`; anyone else gets the page,
    /// whose forms carry `next` as admitted, or `/web`.
    async fn sign_in_get_inner(&self, headers: &HeaderMap, query: &str) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return self.refused(&refused, line);
        }
        let pairs = form_pairs(query.as_bytes()).unwrap_or_default();
        let mut sent = pairs.iter().filter(|(n, _)| n == "next");
        let next = match (sent.next(), sent.next()) {
            (Some((_, v)), None) => oml_page::admitted_next(v),
            _ => "/web".to_owned(),
        };
        match self.session_of(headers).await {
            Ok(Some((_, session))) => {
                line.session(&session);
                Answer {
                    response: self.redirect(&self.next_location(&next), None),
                    error_code: None,
                    line,
                }
            }
            Ok(None) => Answer {
                response: self.sign_in_page_to(StatusCode::OK, None, &next),
                error_code: None,
                line,
            },
            Err(e) => self.refused(&e, line),
        }
    }

    // -- POST /web/sign-in ------------------------------------------------------

    async fn sign_in(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let answer = st.sign_in_inner(&headers, &body);
        Self::web_finish(&rid, answer)
    }

    /// The start of a sign-in (ADR-0045 d3): the same-origin check, one
    /// field, `provider`, then fresh `state`, verifier and nonce sealed into
    /// `__Host-oml_signin` for ten minutes, and `303` to the provider's
    /// authorization endpoint. Writes no row.
    fn sign_in_inner(&self, headers: &HeaderMap, body: &Bytes) -> Answer {
        let mut line = WebLine::default();
        if let Some(refused) = human_only(headers) {
            return self.refused(&refused, line);
        }
        let issuer = self.app.oauth_issuer().expect("routed only with an issuer");
        if !same_origin(headers, issuer) {
            return self.refused(&origin_refused(), line);
        }
        if let Err(e) = require_media(headers, FORM) {
            return self.refused(&ApiError::validation(e.description), line);
        }
        let fields = match form_pairs(body).and_then(|pairs| unique_fields(&pairs)) {
            Ok(fields) => fields,
            Err(e) => return self.refused(&ApiError::validation(e.description), line),
        };
        // `provider`, and `next` when the page carried one (ADR-0070 d4).
        let mut next = None;
        let mut provider = None;
        let mut known = true;
        for (name, value) in &fields {
            match name.as_str() {
                "provider" => provider = Provider::parse(value),
                "next" => next = Some(oml_page::admitted_next(value)),
                _ => known = false,
            }
        }
        let provider = provider.filter(|_| known);
        let Some((provider, client_id)) =
            provider.and_then(|p| self.pages().client_id(p).map(|id| (p, id.to_owned())))
        else {
            return self.refused(
                &ApiError::validation(
                    "a sign-in sends provider, naming a configured provider, and at most next",
                )
                .with_details(json!({"field": "provider"})),
                line,
            );
        };
        line.provider = Some(provider.as_str());
        let mut started: SignInState = self.app.web_sign_in_start(provider);
        started.next = next.filter(|n| n != "/web");
        let sealed = self.pages().sealer.seal(&started.encode());
        let redirect_uri = format!("{}/web/callback/{}", issuer.as_str(), provider.as_str());
        let mut params: Vec<(&str, String)> = vec![
            ("response_type", "code".to_owned()),
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("scope", provider.scope().to_owned()),
            ("state", started.state.clone()),
            ("code_challenge", started.challenge()),
            ("code_challenge_method", "S256".to_owned()),
        ];
        if let Some(nonce) = &started.nonce {
            params.push(("nonce", nonce.clone()));
        }
        let query: Vec<String> = params
            .iter()
            .map(|(name, value)| format!("{name}={}", percent_encode(value)))
            .collect();
        let location = format!("{}?{}", provider.authorization_endpoint(), query.join("&"));
        Answer {
            response: self.redirect(
                &location,
                Some(set_cookie(
                    SIGN_IN_COOKIE,
                    &sealed,
                    oml_app::domain::web::SIGN_IN_TTL_SECS,
                )),
            ),
            error_code: None,
            line,
        }
    }

    // -- GET /web/callback/{provider} ---------------------------------------------

    async fn callback(
        State(st): State<Self>,
        rid: Rid,
        Path(provider): Path<String>,
        headers: HeaderMap,
        uri: Uri,
    ) -> Response {
        let answer = st
            .callback_inner(&rid, &provider, &headers, uri.query().unwrap_or(""))
            .await;
        Self::web_finish(&rid, answer)
    }

    /// The provider's redirect (ADR-0045 d3, d4, d5): the sign-in cookie is
    /// cleared in every answer; no outbound call is made unless it opens,
    /// names this provider, is under ten minutes old and carries the
    /// query's `state`; an admitted entry gets a fresh session under a new
    /// token, the browser's old session ending with it, and `303 /web`.
    /// Every refusal is a page. One `web_sign_in` line names the outcome.
    async fn callback_inner(
        &self,
        request_id: &str,
        provider: &str,
        headers: &HeaderMap,
        query: &str,
    ) -> Answer {
        let mut line = WebLine::default();
        let Some(provider) = Provider::parse(provider) else {
            return self.refused(&ApiError::not_found(), line);
        };
        line.provider = Some(provider.as_str());
        if let Some(refused) = human_only(headers) {
            return self.refused(&refused, line);
        }
        let Some(client_id) = self.pages().client_id(provider).map(ToOwned::to_owned) else {
            return self.refused(&ApiError::not_found(), line);
        };
        let issuer = self.app.oauth_issuer().expect("routed only with an issuer");
        let pairs = form_pairs(query.as_bytes()).unwrap_or_default();
        let first = |name: &str| {
            let mut values = pairs.iter().filter(|(n, _)| n == name);
            match (values.next(), values.next()) {
                (Some((_, v)), None) => Some(v.as_str()),
                _ => None,
            }
        };
        let opened = cookie(headers, SIGN_IN_COOKIE)
            .and_then(|sealed| self.pages().sealer.open(&sealed))
            .and_then(|bytes| SignInState::decode(&bytes));
        // The page to return to, admitted again: the sealed cookie carries
        // what the sign-in page admitted (ADR-0070 d4).
        let next = opened
            .as_ref()
            .and_then(|s| s.next.as_deref())
            .map_or_else(|| "/web".to_owned(), oml_page::admitted_next);
        let redirect_uri = format!("{}/web/callback/{}", issuer.as_str(), provider.as_str());
        let admitted = self
            .app
            .web_admit(
                &Capped(self.pages()),
                Callback {
                    provider,
                    code: first("code"),
                    state: first("state"),
                    opened: opened.as_ref(),
                    redirect_uri: &redirect_uri,
                    client_id: &client_id,
                },
            )
            .await;
        let entry = match admitted {
            Ok(entry) => entry,
            Err(refusal) => {
                // Open mode: a verified identity no entry holds goes to the
                // terms page, with nothing written (ADR-0068 d2).
                if let Some(mut response) = self.unlisted(&refusal) {
                    sign_in_line(request_id, provider, "terms", None);
                    append_cookie(&mut response, &clear_cookie(SIGN_IN_COOKIE));
                    return Answer {
                        response,
                        error_code: None,
                        line,
                    };
                }
                let (error, outcome) = match refusal {
                    CallbackRefusal::Unlisted(_) => unreachable!("answered above"),
                    CallbackRefusal::Refused(r) => (
                        map_app_error(AppError::WebSignIn(r), ErrorContext::Write),
                        r.reason(),
                    ),
                    CallbackRefusal::Provider(ProviderFailure::Busy) => {
                        self.traffic.metrics().web_failed(provider.as_str(), "busy");
                        (
                            ApiError::new(
                                StatusCode::TOO_MANY_REQUESTS,
                                "limit.rate",
                                "The lead's calls to a sign-in provider are capped (ADR-0045 d15).",
                                "too many sign-ins are in progress with this provider; try again shortly",
                            )
                            .retryable(),
                            "limit_rate",
                        )
                    }
                    CallbackRefusal::Provider(ProviderFailure::Refused(reason)) => {
                        self.traffic.metrics().web_failed(provider.as_str(), reason);
                        (
                            map_app_error(
                                AppError::WebSignIn(SignInRefusal::ProviderRefused),
                                ErrorContext::Write,
                            ),
                            SignInRefusal::ProviderRefused.reason(),
                        )
                    }
                    CallbackRefusal::Store(e) => (ApiError::unavailable(&e), "unavailable"),
                };
                sign_in_line(request_id, provider, outcome, None);
                let mut response = self.sign_in_page(error.status, Some(&error));
                append_cookie(&mut response, &clear_cookie(SIGN_IN_COOKIE));
                return Answer {
                    response,
                    error_code: Some(error.code),
                    line,
                };
            }
        };
        // The first sign-in issues the person's session keys, and a sign-in
        // after one that failed part way completes them (ADR-0045 d6).
        if let Err(e) = crate::retry_chain(|| self.app.web_ensure_keys(&entry)).await {
            sign_in_line(request_id, provider, "session_keys", None);
            let error = map_app_error(e, ErrorContext::Write);
            let mut response = self.refusal_page(&error);
            append_cookie(&mut response, &clear_cookie(SIGN_IN_COOKIE));
            return Answer {
                response,
                error_code: Some(error.code),
                line,
            };
        }
        let old = cookie(headers, SESSION_COOKIE);
        let opened = match self
            .app
            .web_session_open(&entry, self.pages().session_ttl_secs, old.as_deref())
            .await
        {
            Ok(opened) => opened,
            Err(e) => {
                sign_in_line(request_id, provider, "unavailable", None);
                let error = map_app_error(e, ErrorContext::Write);
                let mut response = self.refusal_page(&error);
                append_cookie(&mut response, &clear_cookie(SIGN_IN_COOKIE));
                return Answer {
                    response,
                    error_code: Some(error.code),
                    line,
                };
            }
        };
        line.session(&opened.record);
        sign_in_line(request_id, provider, "signed_in", Some(&opened.record));
        // A person with no agent key yet lands on the onboarding page
        // (ADR-0068 d10).
        let landing = match self.landing(&entry).await {
            "/web" => self.next_location(&next),
            onboarding => onboarding.to_owned(),
        };
        let mut response = self.redirect(
            &landing,
            Some(set_cookie(
                SESSION_COOKIE,
                &opened.token,
                self.pages().session_ttl_secs,
            )),
        );
        append_cookie(&mut response, &clear_cookie(SIGN_IN_COOKIE));
        Answer {
            response,
            error_code: None,
            line,
        }
    }

    /// `303` to `location` under the pages' headers, setting `cookie`.
    fn redirect(&self, location: &str, cookie: Option<String>) -> Response {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::SEE_OTHER;
        if let Ok(value) = HeaderValue::from_str(location) {
            response.headers_mut().insert(header::LOCATION, value);
        }
        if let Some(cookie) = cookie {
            append_cookie(&mut response, &cookie);
        }
        page::secure(
            response,
            &self.pages().provider_list(),
            self.frame_src().as_deref(),
        )
    }
}

/// An `Authorization` header on a web route: refused before anything else
/// is read, so no key, connection token or future credential reaches a
/// page (ADR-0045 d7).
fn human_only(headers: &HeaderMap) -> Option<ApiError> {
    headers.contains_key(header::AUTHORIZATION).then(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: the pages are a person's, reached by a signed-in browser and by nothing an agent holds (ADR-0045 d7).",
            "the web pages take no Authorization header",
        )
        .with_details(json!({ "reason": HUMAN_ONLY }))
    })
}

fn origin_refused() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "oauth.origin_refused",
        "A page's form is accepted only from the page's own origin (ADR-0045 d8; ADR-0031 d9).",
        "this form was not sent from this server's own page",
    )
}

/// The form's fields, each name at most once.
fn unique_fields(
    pairs: &[(String, String)],
) -> Result<Vec<(String, String)>, crate::error::OAuthError> {
    let mut seen = HashSet::new();
    for (name, _) in pairs {
        if !seen.insert(name.as_str()) {
            return Err(crate::error::OAuthError::validation(
                "invalid_request",
                &format!("the field {name:?} is sent more than once"),
            ));
        }
    }
    Ok(pairs.to_vec())
}

/// The value of the cookie `name`, from the one or several `Cookie`
/// headers; `None` when it is absent or not visible ASCII.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(n, _)| *n == name)
        .map(|(_, v)| v.to_owned())
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_graphic()))
}

/// A `__Host-` cookie: `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/` and
/// no `Domain` (ADR-0045 d3, d5).
fn set_cookie(name: &str, value: &str, max_age: i64) -> String {
    format!("{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={max_age}")
}

fn clear_cookie(name: &str) -> String {
    format!("{name}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0")
}

fn append_cookie(response: &mut Response, cookie: &str) {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

/// The sign-in's own line under the request's ids: the provider, the
/// outcome, and the tenant and user when admitted; never the email, the
/// provider subject, the code or the `state` (ADR-0045 d14).
fn sign_in_line(
    request_id: &str,
    provider: Provider,
    outcome: &str,
    session: Option<&WebSessionRecord>,
) {
    let tenant_id: Option<Uuid> = session.map(|s| s.tenant_id);
    tracing::info!(
        event = "web_sign_in",
        request_id,
        provider = provider.as_str(),
        outcome,
        tenant_id = tenant_id.map(tracing::field::display),
        user = session.map(|s| s.user.as_str()),
    );
}

impl WebLine {
    fn session(&mut self, session: &WebSessionRecord) {
        self.session = true;
        self.tenant_id = Some(session.tenant_id);
    }
}

/// Seconds since the epoch, for a render token's expiry.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
