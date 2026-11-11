//! The admin read's two lab routes (ADR-0045 d21): `POST
//! /v2/admin/memories/search` and `GET /v2/admin/memories/{artifact_id}`,
//! admin key role alone, every answer journalled as `memory.admin_read`.
//! The search is a `POST` so its query travels in a body.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::app::{AdminMemory, AdminPage, AdminSearchRequest};
use oml_app::domain::EventTime;
use oml_app::domain::name::Reference;
use oml_app::domain::origin::OriginKind;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::AdminState;

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, acting, finish, json_response, parse_json, retry_chain};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchBody {
    query: Option<String>,
    workarea: Option<String>,
    state: Option<String>,
    /// One `origin.kind` (ADR-0045 d22, d24; ADR-0070 d5).
    origin_kind: Option<String>,
    /// The authoring key, by name or id, of the tenant.
    author: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

/// The cursor: the last item's `created_at` and id, as
/// `<rfc3339>_<uuid>`.
pub(crate) fn cursor_of(next: Option<&(EventTime, Uuid)>) -> Value {
    next.map_or(Value::Null, |(at, id)| {
        json!(format!("{}_{id}", at.as_str()))
    })
}

pub(crate) fn parse_cursor(cursor: &str) -> Result<(EventTime, Uuid), ApiError> {
    let refused = || {
        ApiError::validation("cursor is not a cursor this search served")
            .with_details(json!({ "field": "/cursor" }))
    };
    let (at, id) = cursor.rsplit_once('_').ok_or_else(refused)?;
    Ok((
        EventTime::parse(at).map_err(|_| refused())?,
        Uuid::parse_str(id).map_err(|_| refused())?,
    ))
}

/// One memory as the admin read serves it.
pub(crate) fn admin_memory_json(m: &AdminMemory, full: bool) -> Value {
    let mut out = json!({
        "artifact_id": m.memory.memory_id.to_string(),
        "claim_key": m.memory.claim_key,
        "version": m.memory.version,
        "state": m.state.as_str(),
        "workarea_path": m.workarea_path,
        "text": m.memory.body,
        "kind": m.memory.kind.as_str(),
        "format": m.memory.format.media_type,
        "title": m.memory.format.title,
        "key": m.author.as_ref().and_then(|k| k.name.as_ref()).map(|n| n.as_str()),
        "principal": m.memory.author_principal,
        "user": m.author_user.as_ref().map(|n| n.as_str()),
        "created_at": m.memory.created_at.as_str(),
        // The authoring key's holder, the version's served date, its valid
        // time and admission's finding, as the console's list shows them
        // (ADR-0045 d32; ADR-0070 d5).
        "held_by": m.author.as_ref().map(|k| k.held_by.as_str()),
        "promoted_at": m.memory.promoted_at.as_ref().map(|at| at.as_str()),
        "valid_from": m.memory.valid.from.as_ref().map(|v| v.as_str()),
        "valid_until": m.memory.valid.until.as_ref().map(|v| v.as_str()),
        "scan_finding": m.memory.scan_finding,
        // The reviewer judges the anchor beside the text (ADR-0057 d2).
        "anchor": m
            .memory
            .anchor
            .as_deref()
            .map_or(Value::Null, oml_app::domain::anchor::Anchor::to_json),
    });
    if full && let Some(obj) = out.as_object_mut() {
        obj.insert(
            "derives_from".to_owned(),
            json!(
                m.memory
                    .derives_from
                    .iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>()
            ),
        );
        obj.insert(
            "replaces".to_owned(),
            json!(m.memory.replaces.map(|r| r.to_string())),
        );
        obj.insert("plaintext_digest".to_owned(), json!(m.memory.body_digest));
        obj.insert(
            "verdicts".to_owned(),
            json!(
                m.verdicts
                    .iter()
                    .map(|v| json!({
                        "verdict": v.verdict,
                        "key": v.key_name.as_ref().map(|n| n.as_str()),
                        "principal": v.principal,
                        "reason": v.reason,
                        "at": v.at.as_str(),
                    }))
                    .collect::<Vec<_>>()
            ),
        );
    }
    out
}

fn page_json(page: &AdminPage) -> Value {
    json!({
        "items": page.items.iter().map(|m| admin_memory_json(m, false)).collect::<Vec<_>>(),
        "next_cursor": cursor_of(page.next.as_ref()),
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
    pub(crate) async fn admin_search_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(&rid, Self::admin_search_inner(&st, &headers, &body).await)
    }

    async fn admin_search_inner(
        st: &Self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let req: SearchBody = parse_json(headers, body)?;
        let workarea = req
            .workarea
            .as_deref()
            .map(|w| {
                Reference::parse(w).map_err(|e| {
                    ApiError::validation(e.to_string())
                        .with_details(json!({ "field": "/workarea" }))
                })
            })
            .transpose()?;
        let state = req
            .state
            .as_deref()
            .map(|s| {
                AdminState::parse(s).ok_or_else(|| {
                    ApiError::validation("state is held, promoted, rejected, superseded or demoted")
                        .with_details(json!({ "field": "/state" }))
                })
            })
            .transpose()?;
        let limit = req.limit.unwrap_or(50);
        if !(1..=200).contains(&limit) {
            return Err(ApiError::validation("limit is between 1 and 200")
                .with_details(json!({ "field": "/limit" })));
        }
        let after = req.cursor.as_deref().map(parse_cursor).transpose()?;
        let origin = req
            .origin_kind
            .as_deref()
            .map(|k| {
                OriginKind::parse(k).ok_or_else(|| {
                    ApiError::validation(
                        "origin_kind is one of repo, doc, code, lake, chat, web, manual",
                    )
                    .with_details(json!({ "field": "/origin_kind" }))
                })
            })
            .transpose()?;
        let author = req
            .author
            .as_deref()
            .map(|a| {
                Reference::parse(a).map_err(|e| {
                    ApiError::validation(e.to_string()).with_details(json!({ "field": "/author" }))
                })
            })
            .transpose()?;
        let search = AdminSearchRequest {
            query: req.query.as_deref(),
            workarea,
            state,
            origin,
            author,
            after,
            limit,
        };
        let page = retry_chain(|| st.app.admin_search_filtered(acting.bearer(), &search))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(StatusCode::OK, &page_json(&page)))
    }

    pub(crate) async fn admin_read_route(
        State(st): State<Self>,
        rid: Rid,
        Path(artifact_id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        finish(
            &rid,
            Self::admin_read_inner(&st, &artifact_id, &headers).await,
        )
    }

    async fn admin_read_inner(
        st: &Self,
        artifact_id: &str,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let id = Uuid::parse_str(artifact_id).map_err(|_| ApiError::not_found())?;
        let read = retry_chain(|| st.app.admin_read(acting.bearer(), id))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::OK,
            &admin_memory_json(&read, true),
        ))
    }
}
