//! The connection flow's edge (ADR-0031): what the transport holds that the
//! application layer reaches only through its ports, and the unversioned
//! OAuth routes outside both rings — the two discovery documents,
//! `POST /oauth/register` and `POST /oauth/token` (http-api.md,
//! "Connections: OAuth for hosted MCP clients"; ADR-0031 d1, d3, d4, d5,
//! d13).
//!
//! The routes exist only where the deployment sets `oauth.issuer`; unset,
//! each answers `404` as an unknown path does, and the root form
//! `/.well-known/oauth-protected-resource` is never served. The documents
//! are built from the issuer alone, never from `Host`. Neither endpoint
//! makes an outbound call, and every refusal is the RFC's body
//! ([`OAuthError`]).

pub mod seal;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use serde_json::{Value, json};

use oml_app::domain::oauth::{
    Issuer, SCOPE, authorization_server_document, protected_resource_document,
};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer};
use oml_app::types::{AppError, ExchangeForm, IssuedTokens, RefreshForm};

use crate::error::{ApiError, OAuthError, map_oauth_error};
use crate::{ApiState, finish, json_response, new_request_id, retry_chain};

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
        for pair in body.split(|b| *b == b'&').filter(|p| !p.is_empty()) {
            let (name, value) = match pair.iter().position(|b| *b == b'=') {
                Some(at) => (&pair[..at], &pair[at + 1..]),
                None => (pair, &[][..]),
            };
            let name = form_decode(name)?;
            let value = form_decode(value)?;
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
