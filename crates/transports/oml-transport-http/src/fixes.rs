//! An admin's fixes and a person's confirmation or revert of them
//! (ADR-0089 d2, d3, d8, d15, d16): `POST|GET /v2/admin/fixes`,
//! `GET /v2/review/fixes` and `POST /v2/review/fixes/{fix_id}/confirm|revert`.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::app::FixRequest;
use oml_app::domain::EventTime;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{FixAct, FixRecord};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, acting, finish, json_response, parse_json, retry_chain};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixBody {
    act: String,
    target: Uuid,
    reason: String,
    body: Option<String>,
    title: Option<String>,
}

/// A fix as served. The admin's own view names the admin key and the
/// person it acted for at the fix; a person's reviewer view never names the
/// admin key, principal or user (d15).
fn fix_json(f: &FixRecord, now_us: i64, admin_view: bool) -> Value {
    let mut v = json!({
        "fix_id": f.fix_id,
        "act": f.act.as_str(),
        "target_artifact_id": f.target_id,
        "workspace_id": f.workspace_id,
        "claim_key": f.claim_key,
        "replaced_artifact_id": f.replaced_id,
        "served_artifact_id": f.served_id,
        "reason": f.reason,
        "loosens_guardrail": f.loosens_guardrail,
        "state": f.state.as_str(),
        "fixed_at": f.fixed_at.as_str(),
        "age_seconds": (now_us - f.fixed_at.unix_micros()).max(0) / 1_000_000,
        "settled_at": f.settled_at.as_ref().map(EventTime::as_str),
        "same_user": f.settled_same_user,
        "restored_artifact_id": f.restored_id,
        "superseded_by": f.superseded_by,
    });
    if admin_view && let Some(obj) = v.as_object_mut() {
        obj.insert("admin_key_id".to_owned(), json!(f.admin_key_id));
        obj.insert("admin_person".to_owned(), json!(f.admin_person.as_str()));
    }
    v
}

fn no_query(params: &HashMap<String, String>, allowed: &[&str]) -> Result<(), ApiError> {
    match params.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(other) => Err(
            ApiError::validation(format!("unknown query parameter {other}"))
                .with_details(json!({ "field": other })),
        ),
        None => Ok(()),
    }
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `POST /v2/admin/fixes {act, target, reason}`: an admin's marked
    /// break-glass fix, served at once (ADR-0089 d3, d15, d16).
    pub(crate) async fn fix_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let sent: FixBody = parse_json(&headers, &body)?;
            let act = FixAct::parse(&sent.act).ok_or_else(|| {
                ApiError::validation("act is demote, edit, retire or erase")
                    .with_details(json!({ "field": "/act" }))
            })?;
            let req = FixRequest {
                act,
                target: sent.target,
                reason: sent.reason,
                body: sent.body,
                title: sent.title,
            };
            let outcome = retry_chain(|| st.app.admin_fix(acting.bearer(), &req))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            // An erase of a version that is not promoted, outside
            // `sole_admin`, makes no fix: its pending shred act answers 202
            // (ADR-0097 d4, amendment of 2026-11-27, answer 5).
            Ok(match outcome.fix() {
                Some(fix) => json_response(
                    StatusCode::CREATED,
                    &fix_json(fix, st.app.now().unix_micros(), true),
                ),
                None => json_response(
                    StatusCode::ACCEPTED,
                    &crate::admin_acts::admin_act_json(outcome.act()),
                ),
            })
        };
        finish(&rid, inner.await)
    }

    /// `GET /v2/admin/fixes?state=unconfirmed|all`: the tenant's fixes,
    /// the unconfirmed ones by default (ADR-0089 d15).
    pub(crate) async fn admin_fixes_route(
        State(st): State<Self>,
        rid: Rid,
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            let acting = acting(&headers, None)?;
            no_query(&params, &["state"])?;
            let unconfirmed = match params.get("state").map(String::as_str) {
                None | Some("unconfirmed") => true,
                Some("all") => false,
                Some(_) => {
                    return Err(ApiError::validation("state is unconfirmed or all")
                        .with_details(json!({ "field": "state" })));
                }
            };
            let list = st
                .app
                .admin_fixes(acting.bearer(), unconfirmed)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let now = st.app.now().unix_micros();
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "items": list.iter().map(|f| fix_json(f, now, true)).collect::<Vec<_>>(),
                }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `GET /v2/review/fixes`: the unconfirmed fixes a person's reviewer key
    /// may see, each with the acts this key may take on it (ADR-0089 d15;
    /// amendment of 2026-11-18).
    pub(crate) async fn review_fixes_route(
        State(st): State<Self>,
        rid: Rid,
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            let acting = acting(&headers, None)?;
            no_query(&params, &[])?;
            let list = st
                .app
                .review_fixes(acting.bearer())
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let now = st.app.now().unix_micros();
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "items": list.iter().map(|r| {
                        let mut v = fix_json(&r.fix, now, false);
                        let acts: Vec<&str> = match (r.settles, r.fix.act) {
                            (false, _) => vec![],
                            (true, FixAct::Erase) => vec!["confirm"],
                            (true, _) => vec!["confirm", "revert"],
                        };
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert("acts".to_owned(), json!(acts));
                        }
                        v
                    }).collect::<Vec<_>>(),
                }),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/review/fixes/{fix_id}/confirm` (ADR-0089 d8).
    pub(crate) async fn fix_confirm_route(
        State(st): State<Self>,
        rid: Rid,
        Path(fix): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let id = Uuid::parse_str(&fix).map_err(|_| ApiError::not_found())?;
            let settled = retry_chain(|| st.app.confirm_fix(acting.bearer(), id))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &fix_json(&settled, st.app.now().unix_micros(), false),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/review/fixes/{fix_id}/revert` (ADR-0089 d8).
    pub(crate) async fn fix_revert_route(
        State(st): State<Self>,
        rid: Rid,
        Path(fix): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let id = Uuid::parse_str(&fix).map_err(|_| ApiError::not_found())?;
            let settled = retry_chain(|| st.app.revert_fix(acting.bearer(), id))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &fix_json(&settled, st.app.now().unix_micros(), false),
            ))
        };
        finish(&rid, inner.await)
    }
}
