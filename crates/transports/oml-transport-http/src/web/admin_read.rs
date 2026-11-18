//! The admin read's two lab routes (ADR-0045 d21): `POST
//! /v2/admin/memories/search` and `GET /v2/admin/memories/{artifact_id}`,
//! admin key role alone, every answer journalled as `memory.admin_read`.
//! The search is a `POST` so its query travels in a body.

use axum::body::Bytes;
use std::collections::HashMap;

use axum::extract::{Path, Query, State};
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
use oml_app::types::{AdminState, Investigation};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, acting, finish, json_response, parse_json, retry_chain};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchBody {
    query: Option<String>,
    workspace: Option<String>,
    state: Option<String>,
    /// One `origin.kind` (ADR-0045 d22, d24; ADR-0070 d5).
    origin_kind: Option<String>,
    /// The authoring key, by name or id, of the tenant.
    author: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
    /// The open investigation of the admin key the read is made under
    /// (ADR-0089 d10).
    investigation: Option<String>,
}

/// `POST /v2/admin/investigations`'s body (ADR-0089 d15).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenBody {
    reason: String,
}

/// An investigation id as a read names it: a malformed one names no open
/// investigation (ADR-0089 d10).
fn investigation_id(raw: Option<&str>, field: &'static str) -> Result<Option<Uuid>, ApiError> {
    raw.map(|r| {
        Uuid::parse_str(r).map_err(|_| {
            map_app_error(
                oml_app::types::AppError::InvestigationRequired { field },
                ErrorContext::Write,
            )
        })
    })
    .transpose()
}

/// An investigation as `GET /v2/admin/investigations` serves it: its id,
/// reason, read count and age, never another key's (ADR-0089 d15).
fn investigation_json(i: &Investigation, now_us: i64) -> Value {
    json!({
        "investigation_id": i.investigation_id,
        "reason": i.reason,
        "opened_at": i.opened_at.as_str(),
        "reads": i.reads,
        "age_seconds": (now_us - i.opened_at.unix_micros()).max(0) / 1_000_000,
        "state": if i.closed_at.is_some() { "closed" } else { "open" },
        "closed_at": i.closed_at.as_ref().map(EventTime::as_str),
    })
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
        "workspace_path": m.workspace_path,
        "text": m.memory.body,
        "kind": m.memory.kind.as_str(),
        "format": m.memory.format.media_type,
        "title": m.memory.format.title,
        "key": m.author.as_ref().and_then(|k| k.name.as_ref()).map(|n| n.as_str()),
        // The authoring key's id, which names a key that has no name
        // (ADR-0070 d5; #253).
        "key_id": m.memory.author_key_id.to_string(),
        "principal": m.memory.author_principal,
        "user": m.author_user.as_ref().map(|n| n.as_str()),
        "created_at": m.memory.created_at.as_str(),
        // The authoring key's holder, the version's served date, its valid
        // time and admission's finding, as the console's list shows them
        // (ADR-0045 d32; ADR-0070 d5).
        "used_by": m.author.as_ref().map(|k| k.used_by.as_str()),
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
        let workspace = req
            .workspace
            .as_deref()
            .map(|w| {
                Reference::parse(w).map_err(|e| {
                    ApiError::validation(e.to_string())
                        .with_details(json!({ "field": "/workspace" }))
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
            workspace,
            state,
            origin,
            author,
            after,
            limit,
            investigation: investigation_id(req.investigation.as_deref(), "/investigation")?,
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
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        finish(
            &rid,
            Self::admin_read_inner(&st, &artifact_id, &params, &headers).await,
        )
    }

    async fn admin_read_inner(
        st: &Self,
        artifact_id: &str,
        params: &HashMap<String, String>,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let acting = acting(headers, None)?;
        let id = Uuid::parse_str(artifact_id).map_err(|_| ApiError::not_found())?;
        if let Some(other) = params.keys().find(|k| k.as_str() != "investigation") {
            return Err(
                ApiError::validation(format!("unknown query parameter {other}"))
                    .with_details(json!({ "field": other })),
            );
        }
        let investigation = investigation_id(
            params.get("investigation").map(String::as_str),
            "investigation",
        )?;
        let read = retry_chain(|| st.app.admin_read(acting.bearer(), id, investigation))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::OK,
            &admin_memory_json(&read, true),
        ))
    }

    /// `POST /v2/admin/investigations {reason}`: opens an investigation of
    /// the admin key (ADR-0089 d10, d15).
    pub(crate) async fn investigation_open_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let req: OpenBody = parse_json(&headers, &body)?;
            let opened = retry_chain(|| st.app.open_investigation(acting.bearer(), &req.reason))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::CREATED,
                &investigation_json(&opened, st.app.now().unix_micros()),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `POST /v2/admin/investigations/{investigation_id}/close`: the key
    /// closes its own investigation (ADR-0089 d10; the operator,
    /// 2026-11-17).
    pub(crate) async fn investigation_close_route(
        State(st): State<Self>,
        rid: Rid,
        Path(investigation): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let inner = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let id = Uuid::parse_str(&investigation).map_err(|_| ApiError::not_found())?;
            let closed = retry_chain(|| st.app.close_investigation(acting.bearer(), id))
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &investigation_json(&closed, st.app.now().unix_micros()),
            ))
        };
        finish(&rid, inner.await)
    }

    /// `GET /v2/admin/investigations?state=open|closed`: the admin key's
    /// own investigations, `open` by default (ADR-0089 d15).
    pub(crate) async fn investigations_route(
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
            let open = match params.get("state").map(String::as_str) {
                None | Some("open") => true,
                Some("closed") => false,
                Some(_) => {
                    return Err(ApiError::validation("state is open or closed")
                        .with_details(json!({ "field": "state" })));
                }
            };
            let list = st
                .app
                .investigations(acting.bearer(), open)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let now = st.app.now().unix_micros();
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "items": list.iter().map(|i| investigation_json(i, now)).collect::<Vec<_>>(),
                }),
            ))
        };
        finish(&rid, inner.await)
    }
}
