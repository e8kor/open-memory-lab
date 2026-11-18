//! The five upload routes (ADR-0004 d3, as amended 2026-11-17; #451): open
//! a session, store a part, read its status, complete it, abandon it. Every
//! route serves the session to its opening key alone; nothing is journalled,
//! so no route takes an `Idempotency-Key`: a part is addressed by its
//! number, and a completion or an abandonment repeated answers by state.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::domain::upload::{self, PART_SIZE, UploadPurpose};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::UploadSession;

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, bearer, finish, json_response, parse_json};

/// The most bytes a part's request body may hold: one part, and a little
/// over, so a longer body is refused by length as `upload.part_size`, not
/// cut off by the body limit.
pub(crate) const PART_BODY_LIMIT: usize = PART_SIZE as usize + 1024;

/// openapi.yaml `UploadSession`. The key is never answered.
pub(crate) fn session_json(s: &UploadSession) -> Value {
    json!({
        "upload_id": s.upload_id.to_string(),
        "purpose": s.purpose.as_str(),
        "size": s.size,
        "sha256": s.sha256,
        "part_size": PART_SIZE,
        "parts": upload::part_count(s.size),
        "received": s.received.iter().collect::<Vec<_>>(),
        "missing": s.missing(),
        "state": s.state.as_str(),
        "created_at": s.created_at.as_str(),
        "expires_at": s.expires_at.as_ref().map(|t| t.as_str()),
        "completed_at": s.completed_at.as_ref().map(|t| t.as_str()),
    })
}

fn upload_id(raw: &str) -> Result<Uuid, ApiError> {
    // A malformed id names no session the key opened: absence, as for any
    // other id outside its reads.
    Uuid::parse_str(raw).map_err(|_| ApiError::not_found())
}

fn require_octets(headers: &HeaderMap) -> Result<(), ApiError> {
    let got = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let essence = got.split(';').next().unwrap_or("").trim();
    if essence.eq_ignore_ascii_case("application/octet-stream") {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "request.unsupported_media",
            "Each operation's request media type is fixed by the contract; a body in any other type is refused before parsing.",
            format!("this operation takes application/octet-stream, not {got:?}"),
        )
        .with_details(json!({ "supported": ["application/octet-stream"] })))
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
    /// `POST /v2/uploads {purpose, size, sha256}`.
    pub(crate) async fn open_upload_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct OpenBody {
            purpose: String,
            size: u64,
            sha256: String,
        }
        let result = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let req: OpenBody = parse_json(&headers, &body)?;
            let purpose = UploadPurpose::parse(&req.purpose).ok_or_else(|| {
                ApiError::validation("purpose is import")
                    .with_details(json!({ "field": "/purpose" }))
            })?;
            if req.size == 0 {
                return Err(ApiError::validation("size is at least 1 byte")
                    .with_details(json!({ "field": "/size" })));
            }
            if !upload::is_sha256_hex(&req.sha256) {
                return Err(ApiError::validation("sha256 is 64 lowercase hex digits")
                    .with_details(json!({ "field": "/sha256" })));
            }
            let session = st
                .app
                .open_upload(token.as_str(), purpose, req.size, &req.sha256)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::CREATED, &session_json(&session)))
        }
        .await;
        finish(&rid, result)
    }

    /// `PUT /v2/uploads/{id}/parts/{n}`, the body the part's bytes.
    pub(crate) async fn put_upload_part_route(
        State(st): State<Self>,
        rid: Rid,
        Path((id, n)): Path<(String, String)>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let result = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let id = upload_id(&id)?;
            require_octets(&headers)?;
            // A part number that is no number is outside 1 to the count,
            // as 0 is.
            let n: u64 = n.parse().unwrap_or(0);
            let session = st
                .app
                .put_upload_part(token.as_str(), id, n, &body)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &session_json(&session)))
        }
        .await;
        finish(&rid, result)
    }

    /// `GET /v2/uploads/{id}`.
    pub(crate) async fn upload_status_route(
        State(st): State<Self>,
        rid: Rid,
        Path(id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            let token = bearer(&headers)?;
            let id = upload_id(&id)?;
            let session = st
                .app
                .upload_status(token.as_str(), id)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &session_json(&session)))
        }
        .await;
        finish(&rid, result)
    }

    /// `POST /v2/uploads/{id}/complete`.
    pub(crate) async fn complete_upload_route(
        State(st): State<Self>,
        rid: Rid,
        Path(id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let id = upload_id(&id)?;
            let session = st
                .app
                .complete_upload(token.as_str(), id)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &session_json(&session)))
        }
        .await;
        finish(&rid, result)
    }

    /// `DELETE /v2/uploads/{id}`.
    pub(crate) async fn abandon_upload_route(
        State(st): State<Self>,
        rid: Rid,
        Path(id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let id = upload_id(&id)?;
            st.app
                .abandon_upload(token.as_str(), id)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        .await;
        finish(&rid, result)
    }
}
