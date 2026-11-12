//! Per-account usage's routes (ADR-0071 d6): `GET /v1/account/usage` for a
//! key's own account, and, admin by key role, `GET` an account's usage,
//! `GET` the month's ledger as JSON or CSV, `GET` the drift report, and
//! `PUT` and `DELETE` an account's quota profile. Counts only, never
//! content (d7).

use std::collections::BTreeMap;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State, rejection::QueryRejection};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use oml_app::domain::signup::Quota;
use oml_app::domain::usage::{Meter, Month};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{AccountUsage, ProfileAssignment};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, bearer, finish, json_response, parse_json};

/// openapi.yaml `AccountProfileRequest`, closed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileBody {
    profile: String,
    #[serde(default)]
    overrides: BTreeMap<String, u64>,
}

/// openapi.yaml `AccountUsage`: each meter against its limit this month.
#[must_use]
pub fn usage_json(usage: &AccountUsage) -> Value {
    json!({
        "account": usage.user_root.to_string(),
        "user": usage.user.as_ref().map(|u| u.as_str()),
        "profile": usage.profile,
        "profile_declared": usage.profile_declared,
        "period": usage.month.as_string(),
        "meters": usage.meters.iter().map(|m| json!({
            "meter": m.meter.as_str(),
            "used": m.used,
            "limit": m.limit,
            "percent": m.percent,
            "state": m.state.as_str(),
        })).collect::<Vec<_>>(),
    })
}

/// The ledger's CSV header, the columns in [`Meter::ALL`]'s order.
const CSV_HEADER: &str =
    "day,account,user,profile,memories,text_bytes,writes,recalls,keys,connections";

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    pub(crate) async fn account_usage_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            let token = bearer(&headers)?;
            let usage = st
                .app
                .account_usage(&token)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &usage_json(&usage)))
        }
        .await;
        finish(&rid, result)
    }

    pub(crate) async fn admin_account_usage_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        Path(account): Path<String>,
    ) -> Response {
        let result = async {
            let token = bearer(&headers)?;
            let usage = st
                .app
                .admin_account_usage(&token, &account)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &usage_json(&usage)))
        }
        .await;
        finish(&rid, result)
    }

    pub(crate) async fn set_account_profile_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        Path(account): Path<String>,
        body: Bytes,
    ) -> Response {
        let result = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let body: ProfileBody = parse_json(&headers, &body)?;
            let overrides = body
                .overrides
                .iter()
                .map(|(name, limit)| {
                    Quota::parse(name).map(|q| (q, *limit)).ok_or_else(|| {
                        ApiError::validation(format!("{name:?} is not a quota"))
                            .with_details(json!({ "field": "overrides" }))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let usage = st
                .app
                .admin_set_profile(
                    &token,
                    &account,
                    &ProfileAssignment {
                        profile: body.profile,
                        overrides,
                    },
                )
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &usage_json(&usage)))
        }
        .await;
        finish(&rid, result)
    }

    pub(crate) async fn clear_account_profile_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        Path(account): Path<String>,
    ) -> Response {
        let result = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let usage = st
                .app
                .admin_clear_profile(&token, &account)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &usage_json(&usage)))
        }
        .await;
        finish(&rid, result)
    }

    pub(crate) async fn admin_usage_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
    ) -> Response {
        let result = async {
            // The credential first: a request without one is auth.missing
            // whatever its query says.
            let token = bearer(&headers)?;
            let Query(params) = query.map_err(|e| {
                ApiError::validation(format!(
                    "the query string is not this operation's shape: {e}"
                ))
            })?;
            let mut period = None;
            let mut csv = false;
            for (name, value) in &params {
                match name.as_str() {
                    "period" => {
                        period = Some(Month::parse(value).ok_or_else(|| {
                            ApiError::validation("period is a UTC month, YYYY-MM")
                                .with_details(json!({ "field": "period" }))
                        })?);
                    }
                    "format" => {
                        csv = match value.as_str() {
                            "json" => false,
                            "csv" => true,
                            _ => {
                                return Err(ApiError::validation("format is json or csv")
                                    .with_details(json!({ "field": "format" })));
                            }
                        };
                    }
                    other => {
                        return Err(ApiError::validation(format!(
                            "{other} is not a parameter of this operation"
                        ))
                        .with_details(json!({ "field": other })));
                    }
                }
            }
            let period = period.ok_or_else(|| {
                ApiError::validation("period is required, a UTC month, YYYY-MM")
                    .with_details(json!({ "field": "period" }))
            })?;
            let rows = st
                .app
                .admin_usage_ledger(&token, period)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            if csv {
                let mut out = String::from(CSV_HEADER);
                out.push('\n');
                for (row, user) in &rows {
                    let counts: Vec<String> = Meter::ALL
                        .iter()
                        .map(|m| row.usage.meter(*m).to_string())
                        .collect();
                    out.push_str(&format!(
                        "{},{},{},{},{}\n",
                        row.day,
                        row.user_root,
                        user.as_ref().map_or("", |u| u.as_str()),
                        row.profile.as_deref().unwrap_or(""),
                        counts.join(",")
                    ));
                }
                let mut response = Response::new(Body::from(out));
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/csv; charset=utf-8"),
                );
                return Ok(response);
            }
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "period": period.as_string(),
                    "rows": rows.iter().map(|(row, user)| {
                        let mut v = json!({
                            "day": row.day,
                            "account": row.user_root.to_string(),
                            "user": user.as_ref().map(|u| u.as_str()),
                            "profile": row.profile,
                        });
                        for m in Meter::ALL {
                            v[m.as_str()] = json!(row.usage.meter(m));
                        }
                        v
                    }).collect::<Vec<_>>(),
                }),
            ))
        }
        .await;
        finish(&rid, result)
    }

    pub(crate) async fn admin_usage_drift_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            let token = bearer(&headers)?;
            let drift = st
                .app
                .admin_usage_drift(&token)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "drift": drift.drift.iter().map(|(d, user)| json!({
                        "account": d.user_root.to_string(),
                        "user": user.as_ref().map(|u| u.as_str()),
                        "meter": d.meter.as_str(),
                        "counter": d.counter,
                        "rows": d.rows,
                    })).collect::<Vec<_>>(),
                    "undeclared_profiles": drift.undeclared.iter().map(|(root, user, profile)| json!({
                        "account": root.to_string(),
                        "user": user.as_ref().map(|u| u.as_str()),
                        "profile": profile,
                    })).collect::<Vec<_>>(),
                }),
            ))
        }
        .await;
        finish(&rid, result)
    }
}
