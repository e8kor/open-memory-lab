//! The connection flow's edge (ADR-0031): what the transport holds that the
//! application layer reaches only through its ports, and the unversioned
//! OAuth routes outside both rings — the two discovery documents, the
//! authorization page (`GET` and `POST /oauth/authorize`, [`page`]),
//! `POST /oauth/register` and `POST /oauth/token` (http-api.md,
//! "Connections: OAuth for hosted MCP clients"; ADR-0031 d1, d3, d4, d5,
//! d9, d13).
//!
//! The routes exist only where the deployment sets `oauth.issuer`; unset,
//! each answers `404` as an unknown path does, and the root form
//! `/.well-known/oauth-protected-resource` is never served. The documents
//! are built from the issuer alone, never from `Host`. Neither endpoint
//! makes an outbound call, and every refusal is the RFC's body
//! ([`OAuthError`]).

pub mod page;
pub mod seal;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::{get, post};
use serde_json::{Value, json};

use oml_app::domain::Sha256Hash;
use oml_app::domain::oauth::{
    AuthorizationRequest, Issuer, Refusal, SCOPE, authorization_server_document,
    protected_resource_document,
};
use oml_app::ports::{
    ClientDocumentFetcher, Clock, ConnectionStore, DocumentFetchFailure, FetchedDocument, IdGen,
    JournalStore, Scanner, Signer,
};
use oml_app::types::{
    AppError, AuthorizeOutcome, AuthorizeParams, CodeEntryOutcome, ConfirmAction, ConfirmOutcome,
    ExchangeForm, IssuedTokens, RefreshForm,
};

use crate::error::{ApiError, ErrorContext, OAuthError, map_app_error, map_oauth_error};
use crate::{ApiState, finish, json_response, new_request_id, retry_chain};
use page::{CodeForm, page_response, secure};

const FORM: &str = "application/x-www-form-urlencoded";
const JSON: &str = "application/json";

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `router` with the OAuth routes where `oauth.issuer` is set, else
    /// unchanged, so each of them is an unknown path (http-api.md
    /// connections). No `/v1` or `/v2` route is added.
    pub(crate) fn with_oauth_routes(&self, router: Router<Self>) -> Router<Self> {
        if self.app.oauth_issuer().is_none() {
            return router;
        }
        router
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get(Self::protected_resource),
            )
            .route(
                "/.well-known/oauth-authorization-server",
                get(Self::authorization_server),
            )
            .route(
                "/oauth/authorize",
                get(Self::authorize_page).post(Self::authorize_submit),
            )
            .route("/oauth/register", post(Self::register))
            .route("/oauth/token", post(Self::token))
    }

    /// `GET /.well-known/oauth-protected-resource/mcp` (RFC 9728).
    async fn protected_resource(State(st): State<Self>) -> Response {
        Self::document(&st, protected_resource_document)
    }

    /// `GET /.well-known/oauth-authorization-server` (RFC 8414).
    async fn authorization_server(State(st): State<Self>) -> Response {
        Self::document(&st, authorization_server_document)
    }

    fn document(st: &Self, build: fn(&Issuer) -> Value) -> Response {
        let rid = new_request_id();
        let result = st
            .app
            .oauth_issuer()
            .map(|issuer| json_response(StatusCode::OK, &build(issuer)))
            .ok_or_else(ApiError::not_found);
        finish(&rid, result)
    }

    /// `POST /oauth/register` (RFC 7591): `201` with a minted `client_id`,
    /// no secret and no registration access token (ADR-0031 d5).
    async fn register(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
        let result = Self::register_inner(&st, &headers, &body).await;
        finish(
            &rid,
            Ok(no_store(result.unwrap_or_else(OAuthError::into_response))),
        )
    }

    async fn register_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, OAuthError> {
        require_media(headers, JSON)?;
        st.require_writes_open()?;
        let registered = st.app.register(body).await.map_err(map_oauth_error)?;
        Ok(json_response(
            StatusCode::CREATED,
            &json!({
                "client_id": registered.client_id,
                "client_id_issued_at": registered.issued_at.unix_micros().div_euclid(1_000_000),
                "client_name": registered.client_name,
                "redirect_uris": registered.redirect_uris,
                "token_endpoint_auth_method": registered.token_endpoint_auth_method,
            }),
        ))
    }

    /// `POST /oauth/token` (RFC 6749 §3.2, §4.1.3, §6): every answer, the
    /// pair and each refusal, carries `Cache-Control: no-store`.
    async fn token(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
        let result = Self::token_inner(&st, &headers, &body).await;
        finish(
            &rid,
            Ok(no_store(result.unwrap_or_else(OAuthError::into_response))),
        )
    }

    /// The form is parsed here, once, so that a repeated parameter is seen;
    /// client authentication is refused before any grant parameter is read,
    /// as the application refuses it first (ADR-0031 d5).
    async fn token_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, OAuthError> {
        require_media(headers, FORM)?;
        let mut params = TokenParams::parse(body)?;
        if let Some(scheme) = authorization_scheme(headers)? {
            return Err(map_oauth_error(AppError::ClientAuthentication {
                scheme: Some(scheme),
            }));
        }
        if params.take("client_secret").is_some() {
            return Err(map_oauth_error(AppError::ClientAuthentication {
                scheme: None,
            }));
        }
        let grant_type = params.required("grant_type")?;
        let issued = match grant_type.as_str() {
            "authorization_code" => {
                let form = ExchangeForm {
                    code: params.required("code")?,
                    client_id: params.required("client_id")?,
                    redirect_uri: params.take("redirect_uri"),
                    code_verifier: params.required("code_verifier")?,
                    resource: params.take("resource"),
                    // Refused above, before any grant parameter.
                    client_authentication: None,
                };
                st.require_writes_open()?;
                retry_chain(|| st.app.exchange(form.clone())).await
            }
            "refresh_token" => {
                let form = RefreshForm {
                    refresh_token: params.required("refresh_token")?,
                    client_id: params.required("client_id")?,
                    resource: params.take("resource"),
                    // Refused above, before any grant parameter.
                    client_authentication: None,
                };
                st.require_writes_open()?;
                retry_chain(|| st.app.refresh(form.clone())).await
            }
            _ => {
                return Err(OAuthError::validation(
                    "unsupported_grant_type",
                    "grant_type must be authorization_code or refresh_token",
                ));
            }
        };
        Ok(token_response(&issued.map_err(map_oauth_error)?))
    }

    /// `GET /oauth/authorize` (http-api.md; ADR-0031 d4, d9 step 1): the
    /// query is decoded once, a repeated parameter noted for the bounds,
    /// and the request answered as the connect-code page, a refusal page,
    /// or a redirect once the client and the redirect URI are accepted.
    async fn authorize_page(State(st): State<Self>, uri: Uri) -> Response {
        let rid = new_request_id();
        let response = Self::authorize_page_inner(&st, uri.query().unwrap_or("")).await;
        logged(&rid, "GET", response)
    }

    async fn authorize_page_inner(st: &Self, query: &str) -> Response {
        let pairs = match form_pairs(query.as_bytes()) {
            Ok(pairs) => pairs,
            Err(malformed) => return oauth_error_page(malformed),
        };
        let mut params = AuthorizeParams::default();
        let mut seen = HashSet::new();
        for (name, value) in pairs {
            if !seen.insert(name.clone()) {
                params.repeated = true;
                continue;
            }
            let slot = match name.as_str() {
                "client_id" => &mut params.client_id,
                "redirect_uri" => &mut params.redirect_uri,
                "response_type" => &mut params.response_type,
                "code_challenge" => &mut params.code_challenge,
                "code_challenge_method" => &mut params.code_challenge_method,
                "state" => &mut params.state,
                "resource" => &mut params.resource,
                "scope" => &mut params.scope,
                // RFC 6749 §3.1: an unrecognized parameter is ignored.
                _ => continue,
            };
            *slot = Some(value);
        }
        let outcome = st
            .app
            .authorize_request(params, &st.documents, st.sealer.as_ref())
            .await;
        match outcome {
            Ok(AuthorizeOutcome::Page(view)) => page_response(
                StatusCode::OK,
                page::code_form(&CodeForm {
                    handle: &view.handle,
                    client_name: &view.client_name,
                    verified_by: view.verified_by.as_deref(),
                    redirect_host: &view.redirect_host,
                    attempts_left: view.attempts_left,
                    wrong_code: false,
                }),
            ),
            Ok(AuthorizeOutcome::PageRefusal(refusal)) => app_error_page(AppError::OAuth(refusal)),
            Ok(AuthorizeOutcome::RedirectRefusal {
                error,
                redirect_uri,
                state,
                iss,
            }) => refusal_redirect(&redirect_uri, error, state.as_deref(), &iss),
            Err(error) => app_error_page(error),
        }
    }

    /// `POST /oauth/authorize` (http-api.md; ADR-0031 d9 steps 2 and 3).
    /// The origin check reads the headers alone, so a refused submission
    /// consumes no attempt and spends no code; then the form is decoded
    /// once and must be exactly a code entry or exactly a confirmation.
    async fn authorize_submit(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
        let response = Self::authorize_submit_inner(&st, &headers, &body).await;
        logged(&rid, "POST", response)
    }

    async fn authorize_submit_inner(st: &Self, headers: &HeaderMap, body: &Bytes) -> Response {
        let Some(issuer) = st.app.oauth_issuer() else {
            return api_error_page(&ApiError::not_found());
        };
        if !same_origin(headers, issuer) {
            return app_error_page(AppError::OAuth(Refusal::OriginRefused));
        }
        if let Err(refused) = require_media(headers, FORM) {
            return oauth_error_page(refused);
        }
        let submission = match form_pairs(body).and_then(Submission::of) {
            Ok(submission) => submission,
            Err(refused) => return oauth_error_page(refused),
        };
        if let Err(closed) = st.require_writes_open() {
            return api_error_page(&closed);
        }
        match submission {
            Submission::Code { handle, code } => Self::enter_code(st, &handle, &code).await,
            Submission::Action { handle, action } => Self::confirm(st, &handle, action).await,
        }
    }

    /// Step 2. A handle that is a confirmation's — it does not open under
    /// the sealer and names a request row — is a second code after one
    /// verified, `request.validation`; any other handle is the
    /// application's to judge.
    async fn enter_code(st: &Self, handle: &str, code: &str) -> Response {
        if st.sealer.open(handle).is_none() {
            match st
                .store
                .authorization(&Sha256Hash::digest(handle.as_bytes()))
                .await
            {
                Ok(Some(_)) => {
                    return oauth_error_page(OAuthError::validation(
                        "invalid_request",
                        "a code was already verified on this page; choose Connect or Cancel",
                    ));
                }
                Ok(None) => {}
                Err(error) => return app_error_page(AppError::Store(error)),
            }
        }
        match st.app.enter_code(handle, code, st.sealer.as_ref()).await {
            Ok(CodeEntryOutcome::Confirmation(view)) => {
                page_response(StatusCode::OK, page::confirmation(&view))
            }
            Ok(CodeEntryOutcome::WrongCode {
                handle,
                attempts_left,
            }) => {
                let Some(request) = st
                    .sealer
                    .open(&handle)
                    .as_deref()
                    .and_then(AuthorizationRequest::decode)
                else {
                    return api_error_page(&ApiError::internal(
                        "a handle the sealer just sealed did not open",
                    ));
                };
                page_response(
                    StatusCode::BAD_REQUEST,
                    page::code_form(&CodeForm {
                        handle: &handle,
                        client_name: &request.client_name,
                        verified_by: request.document_host.as_deref(),
                        redirect_host: redirect_host(&request.redirect_uri),
                        attempts_left,
                        wrong_code: true,
                    }),
                )
            }
            Ok(CodeEntryOutcome::Denied {
                error,
                redirect_uri,
                state,
                iss,
            }) => refusal_redirect(&redirect_uri, error, state.as_deref(), &iss),
            Err(error) => app_error_page(error),
        }
    }

    /// Step 3. A handle that opens under the sealer is the first page's,
    /// an action before a verified code: `request.validation`.
    async fn confirm(st: &Self, handle: &str, action: ConfirmAction) -> Response {
        if st.sealer.open(handle).is_some() {
            return oauth_error_page(OAuthError::validation(
                "invalid_request",
                "Connect and Cancel follow a verified code; enter the connect code first",
            ));
        }
        match st.app.confirm(handle, action).await {
            Ok(ConfirmOutcome::Connected {
                redirect_uri,
                code,
                state,
                iss,
            }) => redirect(&redirect_uri, &[("code", &code)], state.as_deref(), &iss),
            Ok(ConfirmOutcome::Cancelled {
                redirect_uri,
                state,
                iss,
            }) => redirect(
                &redirect_uri,
                &[("error", "access_denied")],
                state.as_deref(),
                &iss,
            ),
            Ok(ConfirmOutcome::Denied {
                error,
                redirect_uri,
                state,
                iss,
            }) => refusal_redirect(&redirect_uri, error, state.as_deref(), &iss),
            Err(error) => app_error_page(error),
        }
    }
}

/// The pair (RFC 6749 §5.1): `token_type` `Bearer` and `scope` `mcp`.
fn token_response(issued: &IssuedTokens) -> Response {
    json_response(
        StatusCode::OK,
        &json!({
            "access_token": issued.access_token.as_str(),
            "token_type": "Bearer",
            "expires_in": issued.expires_in,
            "refresh_token": issued.refresh_token.as_str(),
            "scope": SCOPE,
        }),
    )
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// A body sent in another media type is refused before it is read; an
/// absent `Content-Type` is read as the endpoint's own, as on the rings.
fn require_media(headers: &HeaderMap, supported: &str) -> Result<(), OAuthError> {
    let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
        return Ok(());
    };
    let essence = content_type
        .to_str()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if essence == supported {
        Ok(())
    } else {
        Err(OAuthError::unsupported_media(supported, &essence))
    }
}

/// The scheme of an `Authorization` header, which no client here sends:
/// `None` without one. A header whose scheme is not an RFC 9110 token
/// cannot be named in a challenge and is `invalid_request`.
fn authorization_scheme(headers: &HeaderMap) -> Result<Option<String>, OAuthError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let scheme = value
        .to_str()
        .ok()
        .and_then(|v| v.split(' ').next())
        .filter(|s| !s.is_empty() && s.bytes().all(is_tchar));
    match scheme {
        Some(scheme) => Ok(Some(scheme.to_owned())),
        None => Err(OAuthError::validation(
            "invalid_request",
            "the Authorization header names no scheme; clients are public and send none",
        )),
    }
}

/// RFC 9110 §5.6.2 `tchar`.
fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// A token request's parameters after exactly one
/// `application/x-www-form-urlencoded` decoding, each name at most once.
struct TokenParams(Vec<(String, String)>);

impl TokenParams {
    /// Splits on `&`, skipping empty sequences, and decodes `+` and `%XX`
    /// in each name and value. A malformed escape, a value that is not
    /// UTF-8 once decoded, or a name sent twice is `invalid_request`
    /// (RFC 6749 §3.2).
    fn parse(body: &[u8]) -> Result<Self, OAuthError> {
        let mut params: Vec<(String, String)> = Vec::new();
        for (name, value) in form_pairs(body)? {
            if params.iter().any(|(seen, _)| *seen == name) {
                return Err(OAuthError::validation(
                    "invalid_request",
                    &format!("the parameter {name:?} is sent more than once (RFC 6749 §3.2)"),
                ));
            }
            params.push((name, value));
        }
        Ok(Self(params))
    }

    fn take(&mut self, name: &str) -> Option<String> {
        let at = self.0.iter().position(|(n, _)| n == name)?;
        Some(self.0.swap_remove(at).1)
    }

    fn required(&mut self, name: &str) -> Result<String, OAuthError> {
        self.take(name).ok_or_else(|| {
            OAuthError::validation(
                "invalid_request",
                &format!("the parameter {name:?} is required"),
            )
        })
    }
}

/// An `application/x-www-form-urlencoded` body or query as its pairs, in
/// order: split on `&`, empty sequences skipped, `+` and `%XX` decoded in
/// each name and value. A malformed escape or a value that is not UTF-8
/// once decoded is `invalid_request`.
fn form_pairs(raw: &[u8]) -> Result<Vec<(String, String)>, OAuthError> {
    raw.split(|b| *b == b'&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (name, value) = match pair.iter().position(|b| *b == b'=') {
                Some(at) => (&pair[..at], &pair[at + 1..]),
                None => (pair, &[][..]),
            };
            Ok((form_decode(name)?, form_decode(value)?))
        })
        .collect()
}

fn form_decode(raw: &[u8]) -> Result<String, OAuthError> {
    let malformed = || {
        OAuthError::validation(
            "invalid_request",
            "the body is not application/x-www-form-urlencoded: a malformed escape or a value that is not UTF-8",
        )
    };
    let mut out = Vec::with_capacity(raw.len());
    let mut bytes = raw.iter();
    while let Some(&b) = bytes.next() {
        match b {
            b'+' => out.push(b' '),
            b'%' => {
                let high = bytes.next().and_then(|h| hex_digit(*h));
                let low = bytes.next().and_then(|l| hex_digit(*l));
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(malformed());
                };
                out.push(high << 4 | low);
            }
            _ => out.push(b),
        }
    }
    String::from_utf8(out).map_err(|_| malformed())
}

fn hex_digit(b: u8) -> Option<u8> {
    char::from(b)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// What a page's `POST` submits: exactly `handle` and `connect_code`, or
/// exactly `handle` and `action` (http-api.md `POST /oauth/authorize`).
enum Submission {
    Code {
        handle: String,
        code: String,
    },
    Action {
        handle: String,
        action: ConfirmAction,
    },
}

impl Submission {
    /// Any other combination — both fields, a field missing, another field,
    /// a name sent twice or an unknown action — is `request.validation`.
    fn of(fields: Vec<(String, String)>) -> Result<Self, OAuthError> {
        let refused = || {
            OAuthError::validation(
                "invalid_request",
                "a submission carries exactly handle and connect_code, or exactly handle and action",
            )
        };
        let mut handle = None;
        let mut code = None;
        let mut action = None;
        for (name, value) in fields {
            let slot = match name.as_str() {
                "handle" => &mut handle,
                "connect_code" => &mut code,
                "action" => &mut action,
                _ => return Err(refused()),
            };
            if slot.replace(value).is_some() {
                return Err(refused());
            }
        }
        match (handle, code, action) {
            (Some(handle), Some(code), None) => Ok(Submission::Code { handle, code }),
            (Some(handle), None, Some(action)) => {
                let action = match action.as_str() {
                    "connect" => ConfirmAction::Connect,
                    "cancel" => ConfirmAction::Cancel,
                    _ => return Err(refused()),
                };
                Ok(Submission::Action { handle, action })
            }
            _ => Err(refused()),
        }
    }
}

/// The page's cross-site check, on headers alone (ADR-0031 d9):
/// `Sec-Fetch-Site: same-origin`, or with that header absent an `Origin`
/// equal to the issuer's. `Origin: null`, any other value, and a header
/// sent twice are refused.
fn same_origin(headers: &HeaderMap, issuer: &Issuer) -> bool {
    let single = |name: &str| {
        let mut values = headers.get_all(name).iter();
        match (values.next(), values.next()) {
            (Some(value), None) => Some(value.as_bytes()),
            _ => None,
        }
    };
    if headers.contains_key("sec-fetch-site") {
        single("sec-fetch-site") == Some(b"same-origin")
    } else {
        single(header::ORIGIN.as_str()) == Some(issuer.as_str().as_bytes())
    }
}

/// One log line per authorization-page request: its id, method, status
/// and, for a redirect, the target without its query, which carries the
/// authorization code. No body, handle, code or digest is logged
/// (observability.md, the plaintext rule).
fn logged(request_id: &str, method: &'static str, response: Response) -> Response {
    let location = response
        .headers()
        .get(header::LOCATION)
        .and_then(|l| l.to_str().ok())
        .map_or("", |l| l.split('?').next().unwrap_or(""));
    tracing::info!(
        event = "oauth_authorize",
        request_id,
        method,
        status = response.status().as_u16(),
        location,
    );
    finish(request_id, Ok(response))
}

/// A refusal on the page: its errors.md status, code and message.
fn app_error_page(error: AppError) -> Response {
    api_error_page(&map_app_error(error, ErrorContext::Write))
}

fn api_error_page(error: &ApiError) -> Response {
    page_response(error.status, page::refusal(error.code, &error.message))
}

fn oauth_error_page(error: OAuthError) -> Response {
    page_response(error.status, page::refusal(error.code, &error.description))
}

/// A refusal redirected after the client and the redirect URI were
/// accepted: its RFC `error` and description (RFC 6749 §4.1.2.1).
fn refusal_redirect(
    redirect_uri: &str,
    error: Refusal,
    state: Option<&str>,
    iss: &str,
) -> Response {
    redirect(
        redirect_uri,
        &[
            ("error", error.authorize_error()),
            ("error_description", &error.to_string()),
        ],
        state,
        iss,
    )
}

/// `303` to `redirect_uri` with `params`, then the `state` verbatim and
/// `iss` (RFC 9207 §2), each percent-encoded as a query parameter.
fn redirect(
    redirect_uri: &str,
    params: &[(&str, &str)],
    state: Option<&str>,
    iss: &str,
) -> Response {
    let mut location = redirect_uri.to_owned();
    let mut separator = if redirect_uri.contains('?') { '&' } else { '?' };
    let tail = state
        .map(|s| ("state", s))
        .into_iter()
        .chain([("iss", iss)]);
    for (name, value) in params.iter().copied().chain(tail) {
        location.push(separator);
        location.push_str(name);
        location.push('=');
        location.push_str(&percent_encode(value));
        separator = '&';
    }
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).expect("a percent-encoded location is ASCII"),
    );
    secure(response)
}

/// Every byte outside RFC 3986's unreserved set as `%XX`.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The host of an allowlisted redirect URI, as the first page names it.
fn redirect_host(uri: &str) -> &str {
    let authority = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let authority = authority.split(['/', '?']).next().unwrap_or(authority);
    authority.split(':').next().unwrap_or(authority)
}

/// The client document fetcher behind the authorize page, as a shared
/// object: the composition's, or none, which fails every fetch so a
/// document client is `oauth.client_invalid` and nothing goes out.
#[derive(Clone, Default)]
pub(crate) struct Documents(Option<Arc<dyn DynFetch>>);

impl Documents {
    pub(crate) fn new<F: ClientDocumentFetcher + 'static>(fetcher: F) -> Self {
        Self(Some(Arc::new(fetcher)))
    }
}

type FetchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<FetchedDocument, DocumentFetchFailure>> + Send + 'a>>;

trait DynFetch: Send + Sync {
    fn fetch_dyn<'a>(&'a self, url: &'a str) -> FetchFuture<'a>;
}

impl<F: ClientDocumentFetcher> DynFetch for F {
    fn fetch_dyn<'a>(&'a self, url: &'a str) -> FetchFuture<'a> {
        Box::pin(self.fetch(url))
    }
}

impl ClientDocumentFetcher for Documents {
    async fn fetch(&self, url: &str) -> Result<FetchedDocument, DocumentFetchFailure> {
        match &self.0 {
            Some(fetcher) => fetcher.fetch_dyn(url).await,
            None => Err(DocumentFetchFailure::Failed),
        }
    }
}
