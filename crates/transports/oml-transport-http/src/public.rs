//! `POST /v2/workspaces/{workspace}/public` (ADR-0098 d6, d9): an admin
//! person sets or clears a workspace's `public` flag, with a reason. A
//! clear, and a set under `sole_admin`, answer 200 `effective`; a set that
//! waits on a second admin person answers 202 `pending_confirmation`, and
//! `confirmAdminAct` or `declineAdminAct` settles it.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use oml_app::domain::admin_act::AdminActState;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{
    ApiState, acting, finish, idempotency_key, json_response, parse_json, reference, retry_chain,
};

/// openapi.yaml `WorkspacePublicRequest`, closed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicBody {
    public: bool,
    reason: Option<String>,
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `POST /v2/workspaces/{workspace}/public`, operation
    /// `setWorkspacePublic`.
    pub(crate) async fn set_workspace_public(
        State(st): State<Self>,
        rid: Rid,
        Path(workspace_id): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let result: Result<Response, ApiError> = async {
            st.require_writes_open()?;
            let acting = acting(&headers, None)?;
            let _idem = idempotency_key(&headers)?;
            let workspace = reference("workspace", &workspace_id)?;
            let req: PublicBody = parse_json(&headers, &body)?;
            let outcome = retry_chain(|| {
                st.app.set_workspace_public(
                    acting.bearer(),
                    workspace.clone(),
                    req.public,
                    req.reason.as_deref(),
                )
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(if outcome.state == AdminActState::PendingConfirmation {
                json_response(
                    StatusCode::ACCEPTED,
                    &json!({
                        "workspace": outcome.workspace_id,
                        "act_id": outcome.act_id,
                        "state": outcome.state.as_str(),
                    }),
                )
            } else {
                json_response(
                    StatusCode::OK,
                    &json!({
                        "workspace": outcome.workspace_id,
                        "public": outcome.public,
                        "act_id": outcome.act_id,
                        "state": outcome.state.as_str(),
                    }),
                )
            })
        }
        .await;
        finish(&rid, result)
    }
}
