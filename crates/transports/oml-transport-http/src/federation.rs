//! Federated recall's policy routes (ADR-0072 d5): `GET /v2/federation`
//! and `POST /v2/federation`, admin by key role, and `POST
//! /v2/workspaces/{workspace}/federation`, a person's reviewer key or a tenant
//! admin. Each change is journalled; nothing here grants a read.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use oml_app::app::WideKey;
use oml_app::federation::{DefaultSources, FederationPolicyChange, FederationSettings};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{
    ApiState, acting, bearer, finish, idempotency_key, json_response, parse_json, reference,
    retry_chain,
};

/// openapi.yaml `FederationPolicyRequest`, closed: each field sent replaces
/// its value.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyBody {
    enabled: Option<bool>,
    default_sources: Option<String>,
    max_sources: Option<u32>,
    /// The admin act's reason (ADR-0097 d26).
    reason: Option<String>,
}

/// openapi.yaml `WorkspaceFederationRequest`, closed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceBody {
    accept_federated: bool,
    /// An admin act's reason (ADR-0097 d26).
    reason: Option<String>,
}

/// openapi.yaml `FederationSettings`: the policy, the keys that met more
/// than 1,000 candidate sources since the lead started (d12), and how the
/// widen hint's signal fared (#244).
fn settings_json(settings: &FederationSettings, wide_keys: &[WideKey]) -> Value {
    json!({
        "enabled": settings.policy.enabled,
        "default_sources": settings.policy.default_sources.as_str(),
        "max_sources": settings.policy.max_sources,
        "wide_keys": wide_keys.iter().map(|k| json!({
            "key_id": k.key_id,
            "candidates": k.candidates,
        })).collect::<Vec<_>>(),
        "widen_signal": {
            "built": settings.widen_signal.built,
            "unserved": settings.widen_signal.unserved,
            "evicted": settings.widen_signal.evicted,
            "failed": settings.widen_signal.failed,
            "last_failure": settings.widen_signal.last_failure,
        },
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
    /// `GET /v2/federation`: the tenant's federation policy.
    pub(crate) async fn federation_settings(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            let token = bearer(&headers)?;
            let (settings, wide) = st
                .app
                .federation_settings(&token)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &settings_json(&settings, &wide),
            ))
        }
        .await;
        finish(&rid, result)
    }

    /// `POST /v2/federation`: sets the tenant's policy, journalled as
    /// `federation.policy_set`; answers the policy as it now is.
    pub(crate) async fn set_federation_policy(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let result: Result<Response, ApiError> = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let _idem = idempotency_key(&headers)?;
            let req: PolicyBody = parse_json(&headers, &body)?;
            let default_sources = req
                .default_sources
                .as_deref()
                .map(|d| {
                    DefaultSources::parse(d).ok_or_else(|| {
                        ApiError::validation(format!(
                            "default_sources must be scope or wide, not {d:?}: a list is never a default"
                        ))
                        .with_details(json!({ "field": "/default_sources" }))
                    })
                })
                .transpose()?;
            let change = FederationPolicyChange {
                enabled: req.enabled,
                default_sources,
                max_sources: req.max_sources,
            };
            let taken = retry_chain(|| {
                st.app
                    .set_federation_policy_with_reason(&token, change, req.reason.as_deref())
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            // A widening that waits on a second admin person answers its
            // act (ADR-0097 d5).
            if let Some(act) = &taken.pending {
                return Ok(json_response(
                    StatusCode::ACCEPTED,
                    &crate::admin_acts::admin_act_json(act),
                ));
            }
            let (settings, wide) = &taken.now;
            Ok(json_response(StatusCode::OK, &settings_json(settings, wide)))
        }
        .await;
        finish(&rid, result)
    }

    /// `POST /v2/workspaces/{workspace}/federation`: sets the node's
    /// `accept_federated`, journalled as `workspace.federation_set`; answers
    /// the field as it now is.
    pub(crate) async fn set_workspace_federation(
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
            let req: WorkspaceBody = parse_json(&headers, &body)?;
            let taken = retry_chain(|| {
                st.app.set_workspace_federation_with_reason(
                    acting.bearer(),
                    workspace.clone(),
                    req.accept_federated,
                    req.reason.as_deref(),
                )
            })
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            // An admin's acceptance that waits on a second admin person
            // answers its act (ADR-0097 d5).
            if let Some(act) = &taken.pending {
                return Ok(json_response(
                    StatusCode::ACCEPTED,
                    &crate::admin_acts::admin_act_json(act),
                ));
            }
            Ok(json_response(
                StatusCode::OK,
                &json!({ "accept_federated": taken.now.accept_federated }),
            ))
        }
        .await;
        finish(&rid, result)
    }
}
