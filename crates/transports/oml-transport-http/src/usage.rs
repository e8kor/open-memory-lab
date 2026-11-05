//! The recall telemetry's routes (ADR-0054 d2, d3, d6, d11, as accepted):
//! `GET` and `POST /v2/usage/settings`, `GET /v2/usage/events` and `POST
//! /v2/usage/eval`, admin by key role; and the `feedback` member's parse,
//! shared by `POST /v1/memories/recall` and `oml_read`.

use axum::body::Bytes;
use axum::extract::{Query, State, rejection::QueryRejection};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::app::UsageSettings;
use oml_app::domain::EventTime;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::telemetry::{Feedback, ReplayReport, StoredEvent};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, bearer, finish, idempotency_key, json_response, parse_json};

/// The most events one page of `GET /v2/usage/events` holds, and its
/// default.
const EVENTS_MAX: u32 = 200;
const EVENTS_DEFAULT: u32 = 50;

/// openapi.yaml `UsageSettingsRequest`, closed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsBody {
    query_text: bool,
}

/// openapi.yaml `UsageEvalRequest`, closed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalBody {
    pipelines: Vec<String>,
}

/// openapi.yaml `Feedback`, closed: `recall_id` and at least one of
/// `useful` and `not_useful` (ADR-0054 d6).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeedbackBody {
    recall_id: Uuid,
    #[serde(default)]
    useful: Vec<Uuid>,
    #[serde(default)]
    not_useful: Vec<Uuid>,
}

/// A read's `feedback` member, parsed; a member that is not its shape is
/// refused naming `/feedback` (ADR-0054 d6).
pub(crate) fn parse_feedback(value: &Value) -> Result<Feedback, ApiError> {
    let body: FeedbackBody = serde_json::from_value(value.clone()).map_err(|e| {
        ApiError::validation(format!(
            "feedback is {{recall_id, useful, not_useful}} with ids: {e}"
        ))
        .with_details(json!({ "field": "/feedback" }))
    })?;
    Ok(Feedback {
        recall_id: body.recall_id,
        useful: body.useful,
        not_useful: body.not_useful,
    })
}

fn settings_json(settings: &UsageSettings) -> Value {
    json!({
        "recording": settings.recording,
        "query_text": settings.query_text_since.is_some(),
        "query_text_since": settings.query_text_since.as_ref().map(EventTime::as_str),
        "retention_days": settings.retention_days,
    })
}

fn event_json(e: &StoredEvent) -> Value {
    json!({
        "recall_id": e.recall_id,
        "created_at": e.created_at.as_str(),
        "key_id": e.key_id,
        "principal": e.principal,
        "workarea_id": e.scope_workarea_id,
        "pipeline": e.pipeline,
        "access": e.access,
        "query_digest": e.query_digest,
        "query_text": e.query_text,
        "hits": e.hits.iter().map(|(id, score)| json!({
            "artifact_id": id,
            "score": f64::from(*score),
        })).collect::<Vec<_>>(),
        "labels": e.labels.iter().map(|l| json!({
            "artifact_id": l.artifact_id,
            "label": l.label,
            "source": l.source,
            "at": l.at.as_str(),
        })).collect::<Vec<_>>(),
    })
}

fn report_json(r: &ReplayReport) -> Value {
    json!({
        "labelled": r.labelled,
        "capped": r.capped,
        "skipped_revoked": r.skipped_revoked,
        "window_days": r.window_days,
        "pipelines": r.pipelines.iter().map(|p| json!({
            "pipeline": p.pipeline.as_str(),
            "queries": p.queries,
            "success_at_1": p.success_at_1,
            "success_at_5": p.success_at_5,
            "ndcg_at_10": p.ndcg_at_10,
            "recall_at_10": p.recall_at_10,
        })).collect::<Vec<_>>(),
    })
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `GET /v2/usage/settings`.
    pub(crate) async fn usage_settings(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            let token = bearer(&headers)?;
            let settings = st
                .app
                .usage_settings(&token)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &settings_json(&settings)))
        }
        .await;
        finish(&rid, result)
    }

    /// `POST /v2/usage/settings`: turns the tenant's query text on or off,
    /// journalled as `telemetry.settings_set`.
    pub(crate) async fn set_usage_settings(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let result: Result<Response, ApiError> = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let _idem = idempotency_key(&headers)?;
            let req: SettingsBody = parse_json(&headers, &body)?;
            let settings = st
                .app
                .set_usage_settings(&token, req.query_text)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &settings_json(&settings)))
        }
        .await;
        finish(&rid, result)
    }

    /// `GET /v2/usage/events?limit=&before=`: the tenant's events of the
    /// last 30 days with their labels, newest first; `next_before` pages on.
    pub(crate) async fn usage_events(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
    ) -> Response {
        let result: Result<Response, ApiError> = async {
            let Query(params) = query.map_err(|e| {
                ApiError::validation(format!(
                    "the query string is not this operation's shape: {e}"
                ))
            })?;
            let mut limit = EVENTS_DEFAULT;
            let mut before = None;
            for (name, value) in &params {
                match name.as_str() {
                    "limit" => {
                        limit = value
                            .parse()
                            .ok()
                            .filter(|l| (1..=EVENTS_MAX).contains(l))
                            .ok_or_else(|| {
                                ApiError::validation("limit must be between 1 and 200")
                                    .with_details(json!({ "field": "limit" }))
                            })?;
                    }
                    "before" => {
                        before = Some(parse_before(value)?);
                    }
                    other => {
                        return Err(ApiError::validation(format!(
                            "{other} is not a parameter of GET /v2/usage/events"
                        ))
                        .with_details(json!({ "field": other })));
                    }
                }
            }
            let token = bearer(&headers)?;
            // The listing is journalled, so a lost head race retries
            // (ADR-0054 d2, as amended).
            let events = crate::retry_chain(|| st.app.usage_events(&token, before.clone(), limit))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let next_before = (events.len() == limit as usize)
                .then(|| events.last())
                .flatten()
                .map(|e| format!("{}~{}", e.created_at.as_str(), e.recall_id));
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "items": events.iter().map(event_json).collect::<Vec<_>>(),
                    "next_before": next_before,
                }),
            ))
        }
        .await;
        finish(&rid, result)
    }

    /// `POST /v2/usage/eval`: the real-usage replay, numbers only.
    pub(crate) async fn usage_eval(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let result: Result<Response, ApiError> = async {
            let token = bearer(&headers)?;
            let req: EvalBody = parse_json(&headers, &body)?;
            if req.pipelines.is_empty() || req.pipelines.len() > 8 {
                return Err(
                    ApiError::validation("pipelines names one to eight pipelines")
                        .with_details(json!({ "field": "/pipelines" })),
                );
            }
            let report = crate::retry_chain(|| st.app.usage_eval(&token, &req.pipelines))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &report_json(&report)))
        }
        .await;
        finish(&rid, result)
    }
}

/// `before`, the `next_before` of the page before: `<created_at>~<recall_id>`.
fn parse_before(value: &str) -> Result<(EventTime, Uuid), ApiError> {
    let refuse = || {
        ApiError::validation("before is the next_before of the page before")
            .with_details(json!({ "field": "before" }))
    };
    let (at, id) = value.split_once('~').ok_or_else(refuse)?;
    Ok((
        EventTime::parse(at).map_err(|_| refuse())?,
        Uuid::parse_str(id).map_err(|_| refuse())?,
    ))
}
