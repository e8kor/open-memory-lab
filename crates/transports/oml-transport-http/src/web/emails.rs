//! The allowlist's three lab routes (ADR-0045 d2): `GET /v2/web/emails`,
//! `POST /v2/web/emails` and `POST /v2/web/emails/remove`, admin by key
//! role. The email travels in a body, never in a path or query string, and
//! the listing's cursor is an email's digest, so no proxy log records an
//! address.

use axum::body::Bytes;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use oml_app::app::UserRow;
use oml_app::domain::Sha256Hash;
use oml_app::domain::name::Name;
use oml_app::domain::web::Email;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{WebEmailRequest, WebEntry};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{
    ApiState, acting, finish, idempotency_key, json_response, page_params, parse_json, retry_chain,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowBody {
    email: Option<String>,
    user: Option<String>,
    admin: Option<bool>,
    /// Refuse un-flagging the tenant's last admin entry (ADR-0045 d10).
    keep_an_admin: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveBody {
    email: Option<String>,
    /// Refuse removing the tenant's last admin entry (ADR-0045 d10).
    keep_an_admin: Option<bool>,
}

fn entry_json(entry: &WebEntry) -> Value {
    json!({
        "email": entry.email.as_str(),
        "user": entry.user.as_str(),
        "admin": entry.admin,
    })
}

/// One person as `GET /v2/web/users` serves them, and as the quota
/// update answers (ADR-0068 d5; ADR-0070 d5 #8).
pub(crate) fn user_row_json(u: &UserRow) -> Value {
    json!({
        "user": u.user.as_str(),
        "signed_up": u.signed_up,
        "disabled": u.disabled,
        "under_quotas": u.under_quotas,
        "usage": {
            "memories": u.usage.memories,
            "keys": u.usage.keys,
            "connections": u.usage.connections,
        },
        "quotas": {
            "memories": u.quotas.memories,
            "keys": u.quotas.keys,
            "connections": u.quotas.connections,
            "requests_per_minute": u.quotas.requests_per_minute,
        },
    })
}

fn email_field(value: Option<&str>) -> Result<Email, ApiError> {
    let value = value.ok_or_else(|| {
        ApiError::validation("email is required").with_details(json!({ "field": "/email" }))
    })?;
    Email::parse(value)
        .map_err(|e| ApiError::validation(e.to_string()).with_details(json!({ "field": "/email" })))
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    pub(crate) async fn list_web_emails(
        State(st): State<Self>,
        rid: Rid,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => Self::list_web_emails_inner(&st, &params, &headers).await,
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    pub(crate) async fn list_web_users(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::list_web_users_inner(&st, &headers).await)
    }

    /// `GET /v2/web/users`, admin by key role (ADR-0068 d5, d6, d11): the
    /// sign-up mode, whether sign-ups share one address bucket, each
    /// person's usage and limits, and the pending deletion requests.
    async fn list_web_users_inner(st: &Self, headers: &HeaderMap) -> Result<Response, ApiError> {
        let acting = acting(headers, None)?;
        let (users, deletions) = st
            .app
            .signup_overview(acting.bearer())
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let policy = st.app.signup_policy();
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "mode": policy.map_or("allowlist", |p| p.mode.as_str()),
                "address_limit_shared": !st.traffic.proxies_trusted(),
                "users": users.iter().map(user_row_json).collect::<Vec<_>>(),
                "deletions": deletions.iter().map(|d| json!({
                    "user": d.user.as_str(),
                    "requested_at": d.requested_at.as_str(),
                })).collect::<Vec<_>>(),
            }),
        ))
    }

    /// `GET /v2/web/emails`: one page of the tenant's entries, after the
    /// cursor, which is the hex of the last entry's email digest.
    async fn list_web_emails_inner(
        st: &Self,
        params: &[(String, String)],
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, None)?;
        let (limit, cursor) = page_params(params)?;
        let after = cursor
            .map(|c| {
                hex::decode(c)
                    .ok()
                    .and_then(|b| <[u8; 32]>::try_from(b).ok())
                    .map(Sha256Hash)
                    .ok_or_else(|| {
                        ApiError::validation("cursor is not a cursor this listing served")
                            .with_details(json!({ "field": "cursor" }))
                    })
            })
            .transpose()?;
        let entries = st
            .app
            .web_emails(acting.bearer(), after.as_ref(), limit)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let next_cursor = (entries.len() == limit)
            .then(|| entries.last().map(|e| hex::encode(e.email.digest().0)))
            .flatten();
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "items": entries.iter().map(entry_json).collect::<Vec<_>>(),
                "next_cursor": next_cursor,
            }),
        ))
    }

    pub(crate) async fn allow_web_email(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::allow_web_email_inner(&st, &headers, &body).await,
        )
    }

    /// `POST /v2/web/emails`: adds an entry or changes one, journalled with
    /// the email's digest; answers the entry as stored.
    async fn allow_web_email_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let _idem = idempotency_key(headers)?;
        let req: AllowBody = parse_json(headers, body)?;
        let email = email_field(req.email.as_deref())?;
        let user = req
            .user
            .as_deref()
            .ok_or_else(|| {
                ApiError::validation("user is required").with_details(json!({ "field": "/user" }))
            })
            .and_then(|u| {
                Name::parse(u).map_err(|e| {
                    ApiError::validation(e.to_string()).with_details(json!({ "field": "/user" }))
                })
            })?;
        let admin = req.admin.ok_or_else(|| {
            ApiError::validation("admin is required").with_details(json!({ "field": "/admin" }))
        })?;
        let request = WebEmailRequest { email, user, admin };
        let (entry, _) = retry_chain(|| {
            st.app.allow_web_email_keeping(
                acting.bearer(),
                &request,
                req.keep_an_admin == Some(true),
            )
        })
        .await
        .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &entry_json(&entry)))
    }

    pub(crate) async fn remove_web_email(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::remove_web_email_inner(&st, &headers, &body).await,
        )
    }

    /// `POST /v2/web/emails/remove`: the entry and its sessions end; the
    /// user's agent keys keep working (ADR-0045 d13).
    async fn remove_web_email_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let _idem = idempotency_key(headers)?;
        let req: RemoveBody = parse_json(headers, body)?;
        let email = email_field(req.email.as_deref())?;
        let (_, seq, event_id) = retry_chain(|| {
            st.app.remove_web_email_keeping(
                acting.bearer(),
                &email,
                req.keep_an_admin == Some(true),
            )
        })
        .await
        .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::OK,
            &json!({ "journal_seq": seq, "event_id": event_id.to_string(), "dry_run": false }),
        ))
    }
}
