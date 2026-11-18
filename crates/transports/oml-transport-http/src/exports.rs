//! `POST /v2/workspaces/{workspace}/exports` (ADR-0084 d15; #425): a
//! person's key exports a workspace's subtree as a signed `oml-bundle/1`
//! encrypted to the age recipients the body names, answered as the age file
//! itself. Each export journals `workspace.exported` and is a new bundle
//! under a new salt, so the route takes no `Idempotency-Key`: sending it
//! again exports again.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use oml_app::app::ExportRequest;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::Bearer;

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, finish, parse_json, reference, retry_chain};

/// The request body (openapi.yaml `ExportRequest`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportBody {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    recipients: Vec<String>,
    #[serde(default)]
    release: Option<String>,
    #[serde(default)]
    since: Option<String>,
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    pub(crate) async fn export_workspace_route(
        State(st): State<Self>,
        rid: Rid,
        Path(workspace): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let result = async {
            st.require_writes_open()?;
            let workspace = reference("workspace", &workspace)?;
            let req: ExportBody = parse_json(&headers, &body)?;
            let req = ExportRequest {
                mode: req.mode,
                recipients: req.recipients,
                release: req.release,
                since: req.since,
            };
            let caller = st.console_caller(&headers).await?;
            let exported = retry_chain(|| {
                st.app
                    .export_workspace(Bearer::Session(&caller), workspace.clone(), &req)
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let mut response = (StatusCode::OK, exported.file).into_response();
            let h = response.headers_mut();
            h.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            h.insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"workspace.oml-bundle\""),
            );
            if let Ok(digest) = HeaderValue::from_str(&exported.manifest_digest) {
                h.insert("oml-manifest-digest", digest);
            }
            Ok::<Response, ApiError>(response)
        }
        .await;
        finish(&rid, result)
    }
}
