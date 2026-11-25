//! The two import routes (ADR-0084 d15, I4; #426): `POST /v2/imports`
//! plans or runs an import of a completed upload into an in-progress
//! workspace, and `GET /v2/imports/{id}` answers its state, counts and
//! refusals to the importing key alone.

use axum::body::Bytes;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::app::{ImportPlan, ImportStart};
use oml_app::domain::import::ImportState;
use oml_app::domain::name::Reference;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::ImportRecord;

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{
    ApiState, acting, bearer, finish, json_response, parse_json, position_cursor,
    position_cursor_of,
};

/// The `event` field of the log line an import whose staging key its end
/// could not shred writes (ADR-0084 I11).
pub const EVENT_IMPORT_SHRED_FAILED: &str = "import_shred_failed";
/// The `event` field of the log line an import that ended `failed` writes.
pub const EVENT_IMPORT_FAILED: &str = "import_failed";

/// openapi.yaml `ImportPlan`.
pub(crate) fn plan_json(p: &ImportPlan) -> Value {
    let mut counts = serde_json::Map::new();
    for key in &p.plan.keys {
        let entry = counts
            .entry(key.outcome.as_str().to_owned())
            .or_insert(json!(0));
        *entry = json!(entry.as_u64().unwrap_or(0) + 1);
    }
    json!({
        "source": {
            "server_id": p.source.server_id,
            "tenant_id": p.source.tenant_id,
            "tenant_name": p.source.tenant_name,
        },
        "exported_at": p.exported_at,
        "manifest_digest": p.manifest_digest,
        "target_workspace_id": p.target_workspace_id.map(|id| id.to_string()),
        "authenticated": p.plan.authenticated,
        "signer_fingerprint": p.plan.signer_fingerprint,
        "allow_older": p.plan.allow_older_used,
        "counts": counts,
        "keys": p.plan.keys.iter().map(|k| json!({
            "claim_key": k.claim_key,
            "outcome": k.outcome.as_str(),
            "source": k.source.as_ref().map(|(id, v)| json!({"artifact_id": id, "version": v})),
            "expected_version": k.expected_version,
        })).collect::<Vec<_>>(),
        "erasure_markers": p.plan.erasure_notices.len(),
        "edges": { "proposed": p.edges_proposable, "reported": p.edges_reported },
    })
}

/// openapi.yaml `Import`.
/// The most imports one page of `GET /v2/imports` serves (ADR-0096 d6).
const IMPORTS_PAGE_MAX: usize = 50;

pub(crate) fn import_json(i: &ImportRecord) -> Value {
    json!({
        "import_id": i.import_id.to_string(),
        "state": i.state.as_str(),
        "target_workspace_id": i.target_workspace_id.to_string(),
        "upload_id": i.upload_id.to_string(),
        "source": {
            "server_id": i.source_server_id,
            "tenant_id": i.source_tenant_id,
        },
        "manifest_digest": i.manifest_digest,
        "signer_fingerprint": i.signer_fingerprint,
        "authenticated": i.authenticated,
        "allow_older": i.allow_older,
        "position": i.position,
        "counts": i.counts,
        "refusals": i.refusals.iter().map(|r| json!({
            "claim_key": r.claim_key,
            "source": {"artifact_id": r.source_artifact_id, "version": r.source_version},
            "code": r.code,
        })).collect::<Vec<_>>(),
        "created_at": i.created_at.as_str(),
        "ended_at": i.ended_at.as_ref().map(|t| t.as_str()),
    })
}

fn import_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::not_found())
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `POST /v2/imports {upload_id, target, dry_run?, allow_older?,
    /// expect_key?, expect_tenant?}`: `200 {plan}` for a dry run; otherwise
    /// `202 {import}`, the row `running`, while the run goes on in its own
    /// task (ADR-0084 I10).
    pub(crate) async fn import_route(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ImportBody {
            upload_id: String,
            target: String,
            #[serde(default)]
            dry_run: bool,
            #[serde(default)]
            allow_older: bool,
            #[serde(default)]
            expect_key: Option<String>,
            #[serde(default)]
            expect_tenant: Option<String>,
        }
        let result = async {
            st.require_writes_open()?;
            let token = bearer(&headers)?;
            let req: ImportBody = parse_json(&headers, &body)?;
            let upload_id = Uuid::parse_str(&req.upload_id).map_err(|_| {
                ApiError::validation("upload_id is an upload session's id")
                    .with_details(json!({ "field": "/upload_id" }))
            })?;
            let target = Reference::parse(&req.target).map_err(|_| {
                ApiError::validation("target names a workspace by id or name")
                    .with_details(json!({ "field": "/target" }))
            })?;
            let fingerprint = |field: &str, value: Option<String>| match value {
                Some(v) if v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()) => {
                    Ok(Some(v.to_ascii_lowercase()))
                }
                Some(_) => Err(ApiError::validation(format!(
                    "{field} is the source key's fingerprint, 64 hex digits"
                ))
                .with_details(json!({ "field": format!("/{field}") }))),
                None => Ok(None),
            };
            let request = oml_app::app::ImportRequest {
                upload_id,
                target,
                dry_run: req.dry_run,
                allow_older: req.allow_older,
                expect_key: fingerprint("expect_key", req.expect_key)?,
                expect_tenant: req.expect_tenant,
            };
            let start = st
                .app
                .import_start(token.as_str(), &request)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(match start {
                ImportStart::Planned(plan) => {
                    json_response(StatusCode::OK, &json!({ "plan": plan_json(&plan) }))
                }
                ImportStart::Started(job) => {
                    // I10: the run is its own task; the request answers 202
                    // with the row running, and a client gone changes
                    // nothing.
                    let body = json!({ "import": import_json(job.record()) });
                    let app = std::sync::Arc::clone(&st.app);
                    let traffic = st.traffic.clone();
                    tokio::spawn(async move {
                        let code_of =
                            |e| map_app_error(e, ErrorContext::Write).code.to_owned();
                        let import_id = job.record().import_id;
                        let finished = app.import_finish(*job, &code_of).await;
                        if let Some(reason) = &finished.shred_failure {
                            tracing::error!(
                                event = EVENT_IMPORT_SHRED_FAILED,
                                %import_id,
                                %reason,
                                "IMPORT STAGING KEY NOT SHREDDED — the next upload sweep nulls it (ADR-0084 I11)"
                            );
                        }
                        match finished.import {
                            Ok(import) => traffic.metrics().import_versions(&import.counts),
                            Err(e) => tracing::warn!(
                                event = EVENT_IMPORT_FAILED,
                                %import_id,
                                reason = %e,
                                "import ended failed; GET /v2/imports/{{id}} names its position"
                            ),
                        }
                    });
                    json_response(StatusCode::ACCEPTED, &body)
                }
            })
        }
        .await;
        finish(&rid, result)
    }

    /// `GET /v2/imports`: newest first, a page of at most 50, as a
    /// person's `<user>-web` the imports of every key of the person, as any
    /// other key its own; `state` is `running`, `done` or `failed`
    /// (ADR-0096 d6).
    pub(crate) async fn list_imports_route(
        State(st): State<Self>,
        rid: Rid,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            let acting = acting(&headers, None)?;
            let Query(params) = query.map_err(|e| {
                ApiError::validation(format!(
                    "the query string is not this operation's shape: {e}"
                ))
            })?;
            let (mut state, mut limit, mut cursor) = (None, IMPORTS_PAGE_MAX, None);
            let mut seen = std::collections::HashSet::new();
            for (name, value) in &params {
                if !seen.insert(name.as_str()) {
                    return Err(ApiError::validation(format!(
                        "the query parameter {name} is given more than once"
                    )));
                }
                match name.as_str() {
                    "state" => {
                        state = Some(ImportState::parse(value).ok_or_else(|| {
                            ApiError::validation("state is running, done or failed")
                                .with_details(json!({ "field": "state" }))
                        })?);
                    }
                    "limit" => {
                        limit = value
                            .parse::<usize>()
                            .ok()
                            .filter(|n| (1..=IMPORTS_PAGE_MAX).contains(n))
                            .ok_or_else(|| {
                                ApiError::validation(format!(
                                    "limit is a whole number from 1 to {IMPORTS_PAGE_MAX}"
                                ))
                                .with_details(json!({ "field": "limit" }))
                            })?;
                    }
                    "cursor" => cursor = Some(position_cursor(value)?),
                    other => {
                        return Err(ApiError::validation(format!(
                            "unknown query parameter {other:?}"
                        )));
                    }
                }
            }
            let listing = st
                .app
                .list_imports(acting.bearer(), state, cursor, limit)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let items: Vec<Value> = listing
                .items
                .iter()
                .map(|(import, name)| {
                    let mut item = import_json(import);
                    item["key"] = json!({
                        "id": import.importer_key_id,
                        "name": name.as_ref().map(oml_app::domain::name::Name::as_str),
                    });
                    item
                })
                .collect();
            Ok(json_response(
                StatusCode::OK,
                &json!({
                    "items": items,
                    "next_cursor": listing.next.as_ref().map(|(at, id)| position_cursor_of(at, *id)),
                }),
            ))
        }
        .await;
        finish(&rid, result)
    }

    /// `GET /v2/imports/{id}`: as the importing key, or as a person's
    /// `<user>-web` for an import any key of the person ran (ADR-0096 d6).
    pub(crate) async fn import_status_route(
        State(st): State<Self>,
        rid: Rid,
        Path(id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let result = async {
            let acting = acting(&headers, None)?;
            let id = import_id(&id)?;
            let import = st
                .app
                .import_status(acting.bearer(), id)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(
                StatusCode::OK,
                &json!({ "import": import_json(&import) }),
            ))
        }
        .await;
        finish(&rid, result)
    }
}
