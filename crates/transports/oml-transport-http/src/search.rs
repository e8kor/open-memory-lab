//! The search settings' two routes (ADR-0049 d4): `GET /v2/search` and
//! `POST /v2/search/default`, admin by key role. A tenant's default search
//! pipeline changes here, journalled, with no restart.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use oml_app::app::SearchSettings;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, bearer, finish, idempotency_key, json_response, parse_json};

/// openapi.yaml `SearchDefaultRequest`, closed: `pipeline` is required and
/// may be `null`, which follows the deployment's default again.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultBody {
    pipeline: Option<String>,
}

/// openapi.yaml `SearchSettings`.
fn settings_json(settings: &SearchSettings) -> Value {
    let mut body = json!({
        "enabled": settings.enabled.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        "deployment_default": settings.deployment_default.as_str(),
        "tenant_default": settings.tenant_default,
        "effective": settings.effective.as_str(),
    });
    // The tenant's valid time doctor reads where a time pipeline is served
    // (proposed ADR-0058 d12).
    if let (Some(v), Some(obj)) = (&settings.valid_time, body.as_object_mut()) {
        obj.insert(
            "valid_time".to_owned(),
            json!({ "declared": v.declared, "expired": v.expired }),
        );
    }
    // The postings health doctor reads, on the read alone (ADR-0052 d12).
    if let (Some(h), Some(obj)) = (&settings.postings, body.as_object_mut()) {
        obj.insert(
            "postings".to_owned(),
            json!({
                "partition": h.partition,
                "unfilled": h.unfilled,
                "served_behind": h.served_behind,
                "workarea_drift": h.workarea_drift,
                "term_drift": h.term_drift,
                "terms_checked": h.terms_checked,
                // The learned sparse encoder's queue for this tenant
                // (ADR-0052 d15).
                "sparse_backlog": h.sparse_backlog,
                "sparse_failing": h.sparse_failing,
                "sparse_parked": h.sparse_parked,
                "sparse_enabled": h.sparse_enabled,
            }),
        );
    }
    body
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `GET /v2/search`: the served pipelines, the deployment's default,
    /// the tenant's and the one in effect.
    pub(crate) async fn search_settings(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            let token = bearer(&headers)?;
            let settings = st
                .app
                .search_settings(&token)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &settings_json(&settings)))
        }
        .await;
        finish(&rid, result)
    }

    /// `POST /v2/search/default`: sets the tenant's default pipeline, or
    /// clears it with `null`, journalled as `search.default_set`; answers
    /// the settings as they now are.
    pub(crate) async fn set_search_default(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let result: Result<Response, ApiError> = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let _idem = idempotency_key(&headers)?;
            let req: DefaultBody = parse_json(&headers, &body)?;
            let settings = st
                .app
                .set_search_default(&token, req.pipeline.as_deref())
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &settings_json(&settings)))
        }
        .await;
        finish(&rid, result)
    }
}
