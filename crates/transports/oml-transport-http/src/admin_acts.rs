//! The admin acts a second admin person settles (ADR-0097 d2, d5, d11,
//! d23): `GET /v2/admin/acts`, `POST /v2/admin/acts/{act}/confirm` and
//! `POST /v2/admin/acts/{act}/decline`; and the contest and settlement of
//! an act against an admin (d8).

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::domain::EventTime;
use oml_app::domain::admin_act::AdminActState;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::AdminActRecord;

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, acting, finish, json_response, parse_json, retry_chain};

/// The body of a confirm or a decline: the reason (ADR-0097 d26).
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DeclineBody {
    reason: Option<String>,
}

/// The body of `POST /v2/admin/acts/{act}/settle` (ADR-0097 d8).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettleBody {
    outcome: String,
}

/// The body of `POST /v2/keys/self/link`: the person the calling admin key
/// links itself to (ADR-0097 d22).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkSelfBody {
    user: String,
    /// Required: a link is a consent admin act (ADR-0097 d7, d26).
    reason: Option<String>,
}

/// The body of `POST /v2/keys/self/convert` (ADR-0097 d18).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConvertBody {
    to: String,
}

/// The body of `POST /v2/keys/{key}/revoke`: `reason` alone, so `cascade`
/// and rotation can join later as new members (ADR-0097 d14).
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RevokeBody {
    reason: Option<String>,
}

/// A notice as the API serves it (ADR-0097 d20).
fn notice_json(n: &oml_app::types::NoticeRecord) -> Value {
    json!({
        "notice_id": n.notice_id.to_string(),
        "kind": n.kind.as_str(),
        "act_id": n.act_id.map(|a| a.to_string()),
        "detail": n.detail,
        "at": n.at.as_str(),
        "dismissed_at": n.dismissed_at.as_ref().map(EventTime::as_str),
    })
}

/// An admin act as served (openapi.yaml `AdminAct`): what the confirm
/// screen shows, the act's role, grants, `used_by`, principal, the root's
/// person, the issuing person and the reason among it (ADR-0097 d23).
pub(crate) fn admin_act_json(a: &AdminActRecord) -> Value {
    let grants = a.effect.get("grants").cloned().unwrap_or_else(|| json!([]));
    json!({
        "act_id": a.act_id,
        "act": a.act.as_str(),
        "class": a.class().as_str(),
        "state": a.state.as_str(),
        "target": a.target,
        "grants": grants,
        "reason": a.reason,
        "actor_key_id": a.actor_key_id,
        "actor_person": a.actor_person.as_ref().map(|p| p.as_str()),
        "confirmed_by_person": a.confirmed_by_person.as_ref().map(|p| p.as_str()),
        "confirm_reason": a.confirm_reason,
        "sole_admin": a.sole_admin,
        "cause": a.cause.map(oml_app::domain::admin_act::AdminActCause::as_str),
        "at": a.at.as_str(),
        "settled_at": a.settled_at.as_ref().map(EventTime::as_str),
        "expires_at": a.expires_at().as_ref().map(EventTime::as_str),
    })
}

fn act_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::not_found())
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `GET /v2/admin/acts?state=`: the tenant's admin acts, oldest first;
    /// `pending_confirmation` by default, `all` for every one.
    /// `GET /v2/web/admin-reads` (ADR-0097 d16): the count of admin reads
    /// of the calling person's own root, by month.
    pub(crate) async fn own_admin_reads_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            let acting = acting(&headers, None)?;
            let (person, months) = st
                .app
                .own_admin_reads(acting.bearer())
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "person": person.as_str(),
                    "months": months
                        .iter()
                        .map(|m| json!({ "month": m.month, "reads": m.reads }))
                        .collect::<Vec<_>>(),
                }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `GET /v2/web/notices` (ADR-0097 d20): the calling person's notices,
    /// the dismissed ones too with `dismissed=true`.
    pub(crate) async fn notices_route(
        State(st): State<Self>,
        rid: Rid,
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            let acting = acting(&headers, None)?;
            if let Some(other) = params.keys().find(|k| k.as_str() != "dismissed") {
                return Err(
                    ApiError::validation(format!("unknown query parameter {other}"))
                        .with_details(json!({ "field": other })),
                );
            }
            let dismissed = match params.get("dismissed").map(String::as_str) {
                None | Some("false") => false,
                Some("true") => true,
                Some(_) => {
                    return Err(ApiError::validation("dismissed is true or false")
                        .with_details(json!({ "field": "dismissed" })));
                }
            };
            let notices = st
                .app
                .notices(acting.bearer(), dismissed)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &json!({ "items": notices.iter().map(notice_json).collect::<Vec<_>>() }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/web/notices/{notice}/dismiss` (ADR-0097 d20).
    pub(crate) async fn notice_dismiss_route(
        State(st): State<Self>,
        rid: Rid,
        Path(notice): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let notice_id = Uuid::parse_str(&notice).map_err(|_| {
                ApiError::validation("notice is a notice id")
                    .with_details(json!({ "field": "notice" }))
            })?;
            let notice = retry_chain(|| st.app.dismiss_notice(acting.bearer(), notice_id))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &notice_json(&notice)))
        };
        finish(&rid, inner.await)
    }

    pub(crate) async fn admin_acts_route(
        State(st): State<Self>,
        rid: Rid,
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            let acting = acting(&headers, None)?;
            if let Some(other) = params.keys().find(|k| k.as_str() != "state") {
                return Err(
                    ApiError::validation(format!("unknown query parameter {other}"))
                        .with_details(json!({ "field": other })),
                );
            }
            let state = match params.get("state").map(String::as_str) {
                None => Some(AdminActState::PendingConfirmation),
                Some("all") => None,
                Some(s) => Some(AdminActState::parse(s).ok_or_else(|| {
                    ApiError::validation(
                        "state is all, effective, pending_confirmation, confirmed, declined, \
                         expired, contested or settled",
                    )
                    .with_details(json!({ "field": "state" }))
                })?),
            };
            let acts = st
                .app
                .admin_acts_listed(acting.bearer(), state)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            // Each act says whether the caller's person may confirm or
            // review it, as the To do count judges it (ADR-0097 d2, d11).
            let items: Vec<Value> = acts
                .iter()
                .map(|(act, to_confirm)| {
                    let mut item = admin_act_json(act);
                    item["to_confirm"] = json!(to_confirm);
                    item
                })
                .collect();
            Ok(json_response(StatusCode::OK, &json!({ "items": items })))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/admin/acts/{act}/confirm {reason}`: a second admin person
    /// confirms a pending widening act, which takes effect in this commit;
    /// the reason is required and kept with the act (ADR-0097 d26;
    /// amendment of 2026-11-27, answer 18).
    pub(crate) async fn admin_act_confirm_route(
        State(st): State<Self>,
        rid: Rid,
        Path(act): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let id = act_id(&act)?;
            let sent: DeclineBody = if body.is_empty() {
                DeclineBody::default()
            } else {
                parse_json(&headers, &body)?
            };
            let act = retry_chain(|| {
                st.app
                    .confirm_admin_act(acting.bearer(), id, sent.reason.as_deref())
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &admin_act_json(&act)))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/admin/acts/{act}/contest` (ADR-0097 d8): the admin person
    /// an open act against an admin targets contests it from their own
    /// session; the flag and sign-in come back, the suspension stays.
    pub(crate) async fn admin_act_contest_route(
        State(st): State<Self>,
        rid: Rid,
        Path(act): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let id = act_id(&act)?;
            let act = retry_chain(|| st.app.contest_admin_act(acting.bearer(), id))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &admin_act_json(&act)))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/admin/acts/{act}/settle {outcome}` (ADR-0097 d8): with
    /// three or more admin persons, a third admin person who counts settles
    /// an open act against an admin, `confirm` or `reverse`.
    pub(crate) async fn admin_act_settle_route(
        State(st): State<Self>,
        rid: Rid,
        Path(act): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let id = act_id(&act)?;
            let sent: SettleBody = parse_json(&headers, &body)?;
            let confirm = match sent.outcome.as_str() {
                "confirm" => true,
                "reverse" => false,
                _ => {
                    return Err(ApiError::validation("outcome is confirm or reverse")
                        .with_details(json!({ "field": "/outcome" })));
                }
            };
            let act = retry_chain(|| st.app.settle_admin_act(acting.bearer(), id, confirm))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &admin_act_json(&act)))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/keys/{key}/revoke {reason?}` (ADR-0097 d14): one key
    /// ends, and nothing more.
    pub(crate) async fn revoke_key_route(
        State(st): State<Self>,
        rid: Rid,
        Path(key): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let key = crate::reference("key", &key)?;
            let sent: RevokeBody = if body.is_empty() {
                RevokeBody::default()
            } else {
                parse_json(&headers, &body)?
            };
            let revoked = retry_chain(|| {
                st.app
                    .revoke_key(acting.bearer(), key.clone(), sent.reason.as_deref())
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "key_id": revoked.key.key_id,
                    "name": revoked.key.name.as_ref().map(|n| n.as_str()),
                    "revoked_at": revoked.revoked_at.as_str(),
                    "act_id": revoked.act_id,
                    "journal_seq": revoked.journal_seq,
                }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/keys/self/link {user}` (ADR-0097 d22): the calling admin
    /// key links itself to a person, pending that person's acceptance.
    pub(crate) async fn link_self_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let sent: LinkSelfBody = parse_json(&headers, &body)?;
            let user = oml_app::domain::name::Name::parse(&sent.user).map_err(|_| {
                ApiError::validation("user is a user's name")
                    .with_details(json!({ "field": "/user" }))
            })?;
            let key = retry_chain(|| {
                st.app
                    .link_self(acting.bearer(), &user, sent.reason.as_deref())
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "key_id": key.key_id,
                    "name": key.name.as_ref().map(|n| n.as_str()),
                    "user": user.as_str(),
                    "accepted": false,
                }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/keys/self/convert {to: maintenance}` (ADR-0097 d18): the
    /// calling admin key becomes a maintenance key in place, keeping its
    /// value; journalled as `key.converted`.
    pub(crate) async fn convert_self_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let sent: ConvertBody = parse_json(&headers, &body)?;
            let to = match sent.to.as_str() {
                "maintenance" => oml_app::domain::Role::Maintenance,
                _ => {
                    return Err(ApiError::validation(
                        "an admin key converts into a maintenance key only (ADR-0097 d18)",
                    )
                    .with_details(json!({ "field": "/to", "reason": "not_maintenance" })));
                }
            };
            let key = retry_chain(|| st.app.convert_self(acting.bearer(), to))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "key_id": key.key_id,
                    "name": key.name.as_ref().map(|n| n.as_str()),
                    "role": key.role.as_str(),
                    "requests_per_minute": key.requests_per_minute,
                }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/admin/acts/{act}/decline {reason?}`: a pending act never
    /// takes effect, and a `sole_admin` act is revoked.
    pub(crate) async fn admin_act_decline_route(
        State(st): State<Self>,
        rid: Rid,
        Path(act): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let id = act_id(&act)?;
            let sent: DeclineBody = if body.is_empty() {
                DeclineBody::default()
            } else {
                parse_json(&headers, &body)?
            };
            let act = retry_chain(|| {
                st.app
                    .decline_admin_act(acting.bearer(), id, sent.reason.as_deref())
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &admin_act_json(&act)))
        };
        finish(&rid, inner.await)
    }
}
