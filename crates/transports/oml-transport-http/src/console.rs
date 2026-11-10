//! The ten operations the web console needs that no other operation serves
//! (ADR-0070 d5, "New operations"), each the documented face of a use case
//! the pages already call: the person's keys, the review queue, a person's
//! capacities, a workarea's offers, the end of a session, the personal
//! export and deletion request, and the admin's quota, disable and deletion
//! close. Each takes any credential under the operation's own rules; the
//! person's own acts (#1 for a non-admin, #5, #6, #7) only a session key's,
//! through its capacity credential, so an agent's bearer key cannot list,
//! end, export or delete its person (ADR-0045 d16).

use axum::body::Bytes;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use oml_app::app::{Caller, Capacity, OwnKey, WorkareaOffer};
use oml_app::domain::Role;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::signup::Quota;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{AppError, Bearer, Credential, GrantRecord};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{
    ApiState, KeyIssueBody, PAGE_LIMIT_DEFAULT, acting, connect_code_json, finish, idempotency_key,
    json_response, key_issued_json, page_limit, parse_json, parse_role, reference, retry_chain,
    session,
};

/// The most workareas one capacities request names.
const CAPACITY_WORKAREAS_MAX: usize = 50;

/// A refusal of a person's own act to anything but their session key:
/// `access.verb_ceiling`, `details.reason: session_key_only` (ADR-0070 d5;
/// ADR-0045 d16).
fn session_key_only(caller: &Caller, verb: &'static str) -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "access.verb_ceiling",
        "A person's own acts run only as their session key, so an agent's key never acts as its person (ADR-0045 d16).",
        format!("{verb} runs only as the person's web session key"),
    )
    .with_details(json!({
        "verb": verb,
        "role": caller.key().role.as_str(),
        "reason": "session_key_only",
    }))
}

/// A refusal of a key verb to a session key of another capacity than
/// `web`: `access.verb_ceiling`, `details.reason: session_capacity`. The
/// admin capacity issues no plain key and denies only as the pages do
/// (ADR-0070 d2, d5).
fn session_capacity(caller: &Caller, verb: &'static str) -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "access.verb_ceiling",
        "A session key acts only as the page act it serves (ADR-0070 d2).",
        format!("{verb} runs as a session key only through the person's web capacity"),
    )
    .with_details(json!({
        "verb": verb,
        "role": caller.key().role.as_str(),
        "reason": "session_capacity",
    }))
}

/// The digest an act's `Idempotency-Key` is held to: the path's user and
/// the body's bytes, so the same key on another request is
/// `idempotency.digest_mismatch`.
fn act_digest(user: &str, body: &Bytes) -> String {
    let mut bytes = user.as_bytes().to_vec();
    bytes.push(b'\n');
    bytes.extend_from_slice(body);
    oml_app::domain::Sha256Hash::digest(&bytes).to_prefixed()
}

/// Whether `caller` is a person's `<user>-web` session key.
fn is_web_session_key(caller: &Caller) -> bool {
    caller.key().credential == Credential::Session
        && caller.key().user.is_some()
        && caller
            .key()
            .name
            .as_ref()
            .zip(caller.key().session_user.as_ref())
            .is_some_and(|(name, user)| {
                name.as_str() == format!("{}-{}", user.as_str(), Capacity::Web.suffix())
            })
}

fn user_param(user: &str) -> Result<Name, ApiError> {
    Name::parse(user).map_err(|e| {
        ApiError::validation(format!("user {user:?} is not a name: {e}"))
            .with_details(json!({ "field": "user" }))
    })
}

fn grant_json(grant: &GrantRecord) -> Value {
    json!({
        "grant_id": grant.grant.grant_id,
        "workarea_node_id": grant.grant.node,
        "role": grant.grant.role.as_str(),
        "covers": grant.grant.covers.as_str(),
        "epoch": grant.epoch,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapacitiesBody {
    capacity: String,
    workareas: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuotaBody {
    #[serde(default, deserialize_with = "present")]
    memories: Option<Option<u64>>,
    #[serde(default, deserialize_with = "present")]
    keys: Option<Option<u64>>,
    #[serde(default, deserialize_with = "present")]
    connections: Option<Option<u64>>,
    #[serde(default, deserialize_with = "present")]
    requests_per_minute: Option<Option<u64>>,
}

/// A member sent as `null` is `Some(None)`, the default again; one not
/// sent is `None`, left as it is.
fn present<'de, D>(deserializer: D) -> Result<Option<Option<u64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u64>::deserialize(deserializer).map(Some)
}

/// `POST /v2/web/users/{user}/disable`'s body: empty, since no reason is
/// journalled in this build and a field the server does not keep is
/// refused, never dropped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisableBody {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseBody {
    outcome: String,
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// The caller of this request's credential, whichever presented it.
    async fn console_caller(&self, headers: &HeaderMap) -> Result<Caller, ApiError> {
        let acting = acting(headers, None)?;
        self.app
            .caller(acting.bearer())
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))
    }

    /// The outcome recorded for this act under the request's
    /// `Idempotency-Key`, answered as it was with `Idempotency-Replay:
    /// true`, or `None` to act (http-api.md idempotency; ADR-0070 d3 layer
    /// 4).
    async fn replayed(
        &self,
        caller: &Caller,
        act: &str,
        idem: &str,
        digest: &str,
    ) -> Result<Option<Response>, ApiError> {
        let Some(recorded) = self
            .app
            .recorded_act(caller, act, idem, digest)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?
        else {
            return Ok(None);
        };
        let status = recorded["status"]
            .as_u64()
            .and_then(|s| u16::try_from(s).ok())
            .and_then(|s| StatusCode::from_u16(s).ok())
            .unwrap_or(StatusCode::OK);
        let mut response = json_response(status, &recorded["body"]);
        response
            .headers_mut()
            .insert("idempotency-replay", HeaderValue::from_static("true"));
        Ok(Some(response))
    }

    /// Records the act's answer under its `Idempotency-Key` and answers it.
    async fn record_outcome(
        &self,
        caller: &Caller,
        act: &str,
        idem: &str,
        digest: &str,
        status: StatusCode,
        body: &Value,
    ) -> Result<Response, ApiError> {
        self.app
            .record_act(
                caller,
                act,
                idem,
                digest,
                json!({ "status": status.as_u16(), "body": body }),
            )
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(status, body))
    }

    // -- #1 GET /v2/keys ------------------------------------------------------

    pub(crate) async fn list_keys(
        State(st): State<Self>,
        rid: Rid,
        query: Result<Query<Vec<(String, String)>>, QueryRejection>,
        headers: HeaderMap,
    ) -> Response {
        let result = match query {
            Ok(Query(params)) => Self::list_keys_inner(&st, &params, &headers).await,
            Err(e) => Err(ApiError::validation(format!(
                "the query string is not this operation's shape: {e}"
            ))),
        };
        finish(&rid, result)
    }

    /// `GET /v2/keys`: the person's own keys for their web session key,
    /// session keys among them (ADR-0045 d9); for the admin key role, the
    /// keys naming `user`, session keys aside (ADR-0045 d13). In name
    /// order, a page at a time, the cursor the last name served.
    async fn list_keys_inner(
        st: &Self,
        params: &[(String, String)],
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let caller = st.console_caller(headers).await?;
        let (mut limit, mut cursor, mut user) = (PAGE_LIMIT_DEFAULT, None, None);
        let mut seen = std::collections::HashSet::new();
        for (name, value) in params {
            if !seen.insert(name.as_str()) {
                return Err(ApiError::validation(format!(
                    "the query parameter {name} is given more than once"
                )));
            }
            match name.as_str() {
                "limit" => limit = page_limit(value)?,
                "cursor" => cursor = Some(value.clone()),
                "user" => user = Some(user_param(value)?),
                other => {
                    return Err(ApiError::validation(format!(
                        "unknown query parameter {other:?}"
                    )));
                }
            }
        }
        let keys = if caller.key().role == Role::Admin {
            let user = user.ok_or_else(|| {
                ApiError::validation("user is required for the admin key role")
                    .with_details(json!({ "field": "user" }))
            })?;
            st.app.web_keys_of_user(&caller, &user).await
        } else {
            if !is_web_session_key(&caller) {
                return Err(session_key_only(&caller, "list_keys"));
            }
            if user.is_some_and(|u| caller.key().session_user.as_ref() != Some(&u)) {
                // Another person's keys are not this key's to know of.
                return Err(ApiError::not_found());
            }
            st.app.web_own_keys(&caller).await
        };
        let mut keys: Vec<OwnKey> = keys.map_err(|e| map_app_error(e, ErrorContext::Write))?;
        keys.sort_by(|a, b| a.key.name.cmp(&b.key.name));
        let after = cursor
            .map(|c| {
                hex::decode(&c)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .ok_or_else(|| {
                        ApiError::validation("cursor is not a cursor this listing served")
                            .with_details(json!({ "field": "cursor" }))
                    })
            })
            .transpose()?;
        let page: Vec<&OwnKey> = keys
            .iter()
            .filter(|k| {
                after
                    .as_deref()
                    .is_none_or(|a| k.key.name.as_ref().map_or("", Name::as_str) > a)
            })
            .take(limit + 1)
            .collect();
        let more = page.len() > limit;
        let mut items = Vec::with_capacity(limit);
        for k in page.iter().take(limit) {
            let mut grants = Vec::new();
            for g in k.access.iter().flat_map(|a| &a.grants) {
                let path = st
                    .app
                    .web_path(&caller, g.grant.node)
                    .await
                    .map_err(|e| map_app_error(e, ErrorContext::Write))?;
                grants.push(json!({
                    "role": g.grant.role.as_str(),
                    "workarea_id": g.grant.node,
                    "path": path,
                }));
            }
            items.push(json!({
                "id": k.key.key_id,
                "name": k.key.name.as_ref().map(Name::as_str),
                "role": k.key.role.as_str(),
                "held_by": k.key.held_by.as_str(),
                "credential": k.key.credential.as_str(),
                "user_root_id": k.key.user,
                "revoked": k.key.revoked_at.is_some(),
                "cut_off": k.cut_off,
                "grants": grants,
            }));
        }
        let next_cursor = if more {
            page.get(limit - 1)
                .and_then(|k| k.key.name.as_ref())
                .map(|n| hex::encode(n.as_str()))
        } else {
            None
        };
        Ok(json_response(
            StatusCode::OK,
            &json!({ "items": items, "next_cursor": next_cursor }),
        ))
    }

    // -- #2 GET /v2/review/queue ---------------------------------------------

    pub(crate) async fn review_queue(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::review_queue_inner(&st, &headers).await)
    }

    /// `GET /v2/review/queue`, admin by key role: the held count and the
    /// oldest per workarea, tenant-wide, by path, and no content
    /// (ADR-0045 d10).
    async fn review_queue_inner(st: &Self, headers: &HeaderMap) -> Result<Response, ApiError> {
        let caller = st.console_caller(headers).await?;
        let rows = st
            .app
            .web_queue(&caller)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "items": rows.iter().map(|r| json!({
                    "workarea_id": r.depth.workarea_id,
                    "path": r.path,
                    "held": r.depth.held,
                    "oldest_at": r.depth.oldest.as_str(),
                })).collect::<Vec<_>>(),
            }),
        ))
    }

    // -- #3 POST /v2/web/users/{user}/capacities -----------------------------

    pub(crate) async fn grant_capacities(
        State(st): State<Self>,
        rid: Rid,
        Path(user): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::grant_capacities_inner(&st, &user, &headers, &body).await,
        )
    }

    /// `POST /v2/web/users/{user}/capacities`, admin by key role: the
    /// person's `<user>-review` gains dreamer and reader on each workarea,
    /// or `<user>-curate`, created at the first designation, gains reader on
    /// each; every grant a `grant.issued` (ADR-0045 d10; ADR-0044 d7).
    async fn grant_capacities_inner(
        st: &Self,
        user: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let caller = st.console_caller(headers).await?;
        let idem = idempotency_key(headers)?;
        let digest = act_digest(user, body);
        if let Some(replayed) = st
            .replayed(&caller, "web.capacities", &idem, &digest)
            .await?
        {
            return Ok(replayed);
        }
        let user = user_param(user)?;
        let req: CapacitiesBody = parse_json(headers, body)?;
        let curate = match req.capacity.as_str() {
            "review" => false,
            "curate" => true,
            _ => {
                return Err(ApiError::validation("capacity is review or curate")
                    .with_details(json!({ "field": "/capacity" })));
            }
        };
        if req.workareas.is_empty() || req.workareas.len() > CAPACITY_WORKAREAS_MAX {
            return Err(ApiError::validation(format!(
                "workareas names 1 to {CAPACITY_WORKAREAS_MAX} workareas"
            ))
            .with_details(json!({ "field": "/workareas" })));
        }
        let workareas: Vec<Reference> = req
            .workareas
            .iter()
            .enumerate()
            .map(|(i, w)| reference(&format!("/workareas/{i}"), w))
            .collect::<Result<_, _>>()?;
        let mut key = None;
        let mut added = Vec::new();
        for workarea in workareas {
            let granted = if curate {
                retry_chain(|| st.app.web_grant_curation(&caller, &user, workarea.clone())).await
            } else {
                retry_chain(|| st.app.web_grant_review(&caller, &user, workarea.clone())).await
            }
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            added.extend(granted.grants_added);
            key = Some(granted.key);
        }
        let key = key.ok_or_else(ApiError::not_found)?;
        st.record_outcome(
            &caller,
            "web.capacities",
            &idem,
            &digest,
            StatusCode::OK,
            &json!({
                "key": { "id": key.key_id, "name": key.name.as_ref().map(Name::as_str) },
                "grants_added": added.iter().map(grant_json).collect::<Vec<_>>(),
                "journal_seq": added.iter().map(|g| g.epoch).max(),
            }),
        )
        .await
    }

    // -- #4 GET /v2/workareas/{workarea}/offers -------------------------------

    pub(crate) async fn workarea_offers(
        State(st): State<Self>,
        rid: Rid,
        Path(workarea): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        finish(
            &rid,
            Self::workarea_offers_inner(&st, &workarea, &headers).await,
        )
    }

    /// `GET /v2/workareas/{workarea}/offers`: the transitions, the mode
    /// change and the commit the caller's verbs admit on the workarea now,
    /// each judged by the verb's own check, and a commit withheld with the
    /// held rows that stop it (ADR-0045 d28). A workarea the caller cannot
    /// read is `access.not_found`.
    async fn workarea_offers_inner(
        st: &Self,
        workarea: &str,
        headers: &HeaderMap,
    ) -> Result<Response, ApiError> {
        let acting = acting(headers, None)?;
        let node = reference("workarea", workarea)?;
        // A workarea the caller cannot read is absent, as its read answers
        // it (R3); the offers name nothing of one.
        st.app
            .workarea_info(acting.bearer(), Some(node.clone()))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let offers = st
            .app
            .workarea_offers_for(acting.bearer(), node)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let mut transitions = Vec::new();
        let mut mode_change = Value::Null;
        let mut commit_offered = false;
        for offer in &offers.offers {
            match offer {
                WorkareaOffer::Propose(to) => {
                    transitions.push(json!({ "op": "propose", "to": to.as_str() }));
                }
                WorkareaOffer::Ratify {
                    transition_id,
                    from,
                    to,
                } => transitions.push(json!({
                    "op": "ratify",
                    "transition_id": transition_id,
                    "from": from.as_str(),
                    "to": to.as_str(),
                })),
                WorkareaOffer::InProgress => mode_change = json!({ "to": "in_progress" }),
                WorkareaOffer::Commit => commit_offered = true,
            }
        }
        Ok(json_response(
            StatusCode::OK,
            &json!({
                "workarea_id": offers.workarea_id,
                "status": offers.status.as_str(),
                "mode": if offers.in_progress { "in_progress" } else { "standard" },
                "transitions": transitions,
                "mode_change": mode_change,
                "commit": {
                    "offered": commit_offered,
                    "held": offers.commit_withheld,
                },
            }),
        ))
    }

    // -- #5 POST /v2/web/session/end ------------------------------------------

    pub(crate) async fn end_session(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::end_session_inner(&st, &headers).await)
    }

    /// `POST /v2/web/session/end`: the session the capacity credential
    /// derives from ends, and every credential with it (ADR-0045 d5;
    /// ADR-0070 d2). Only a capacity credential names a session.
    async fn end_session_inner(st: &Self, headers: &HeaderMap) -> Result<Response, ApiError> {
        let Some(session) = session::session_record() else {
            let caller = st.console_caller(headers).await?;
            return Err(session_key_only(&caller, "end_session"));
        };
        st.app
            .web_sign_out(&session)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let mut response = Response::new(axum::body::Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        Ok(response)
    }

    // -- #6 GET /v2/web/export -------------------------------------------------

    pub(crate) async fn web_export(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::web_export_inner(&st, &headers).await)
    }

    /// `GET /v2/web/export`: everything the person holds, as a JSON
    /// attachment, for their web session key only (ADR-0068 d6).
    async fn web_export_inner(st: &Self, headers: &HeaderMap) -> Result<Response, ApiError> {
        let caller = st.console_caller(headers).await?;
        if !is_web_session_key(&caller) {
            return Err(session_key_only(&caller, "export"));
        }
        let export = st
            .app
            .web_export(&caller)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let mut response = json_response(StatusCode::OK, &export);
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"oml-export.json\""),
        );
        Ok(response)
    }

    // -- #7 POST /v2/web/deletion-requests -------------------------------------

    pub(crate) async fn request_deletion(
        State(st): State<Self>,
        rid: Rid,
        headers: HeaderMap,
    ) -> Response {
        finish(&rid, Self::request_deletion_inner(&st, &headers).await)
    }

    /// `POST /v2/web/deletion-requests`: the person asks for deletion,
    /// journalled as `web.deletion_requested`; it ends nothing and changes
    /// no access. For their web session key only (ADR-0068 d6).
    async fn request_deletion_inner(st: &Self, headers: &HeaderMap) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let caller = st.console_caller(headers).await?;
        let idem = idempotency_key(headers)?;
        let digest = act_digest("", &Bytes::new());
        if let Some(replayed) = st
            .replayed(&caller, "web.deletion_requested", &idem, &digest)
            .await?
        {
            return Ok(replayed);
        }
        if !is_web_session_key(&caller) {
            return Err(session_key_only(&caller, "request_deletion"));
        }
        let seq = retry_chain(|| st.app.web_request_deletion(&caller))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        let person = st
            .app
            .whoami_person(Bearer::Session(&caller))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        st.record_outcome(
            &caller,
            "web.deletion_requested",
            &idem,
            &digest,
            StatusCode::OK,
            &json!({
                "requested_at": person
                    .and_then(|p| p.deletion_requested_at)
                    .map(|t| t.as_str().to_owned()),
                "journal_seq": seq,
            }),
        )
        .await
    }

    // -- #8 POST /v2/web/users/{user}/quota -------------------------------------

    pub(crate) async fn set_quota(
        State(st): State<Self>,
        rid: Rid,
        Path(user): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::set_quota_inner(&st, &user, &headers, &body).await,
        )
    }

    /// `POST /v2/web/users/{user}/quota`, admin by key role: each quota
    /// sent is set, `null` returning it to the default, each journalled as
    /// `web.quota_set`; answers the person's row as `GET /v2/web/users`
    /// serves it (ADR-0068 d5).
    async fn set_quota_inner(
        st: &Self,
        user: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let caller = st.console_caller(headers).await?;
        let idem = idempotency_key(headers)?;
        let digest = act_digest(user, body);
        if let Some(replayed) = st
            .replayed(&caller, "web.quota_set", &idem, &digest)
            .await?
        {
            return Ok(replayed);
        }
        let user = user_param(user)?;
        let req: QuotaBody = parse_json(headers, body)?;
        let sent: Vec<(Quota, Option<u64>)> = [
            (Quota::Memories, req.memories),
            (Quota::Keys, req.keys),
            (Quota::Connections, req.connections),
            (Quota::RequestsPerMinute, req.requests_per_minute),
        ]
        .into_iter()
        .filter_map(|(q, v)| v.map(|v| (q, v)))
        .collect();
        if sent.is_empty() {
            return Err(ApiError::validation(
                "send at least one of memories, keys, connections and requests_per_minute",
            ));
        }
        for (quota, limit) in sent {
            retry_chain(|| st.app.web_set_quota(&caller, &user, quota, limit))
                .await
                .map_err(|e| match e {
                    AppError::WebField { message, .. } => ApiError::validation(message)
                        .with_details(json!({ "field": format!("/{}", quota.as_str()) })),
                    e => map_app_error(e, ErrorContext::Write),
                })?;
        }
        let row = st
            .app
            .web_users(&caller)
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?
            .into_iter()
            .find(|u| u.user == user)
            .ok_or_else(ApiError::not_found)?;
        st.record_outcome(
            &caller,
            "web.quota_set",
            &idem,
            &digest,
            StatusCode::OK,
            &crate::web::emails::user_row_json(&row),
        )
        .await
    }

    // -- #9 POST /v2/web/users/{user}/disable -----------------------------------

    pub(crate) async fn disable_user(
        State(st): State<Self>,
        rid: Rid,
        Path(user): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::disable_user_inner(&st, &user, &headers, &body).await,
        )
    }

    /// `POST /v2/web/users/{user}/disable`, admin by key role: a person the
    /// tenant holds who is not an admin is disabled, journalled as
    /// `web.user_disabled`, their sessions end, and each live bearer and
    /// connector key of theirs is denied on `universe` (ADR-0068 d8). A
    /// user no entry holds, or an admin's, is `access.not_found`.
    async fn disable_user_inner(
        st: &Self,
        user: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let caller = st.console_caller(headers).await?;
        let idem = idempotency_key(headers)?;
        let digest = act_digest(user, body);
        if let Some(replayed) = st
            .replayed(&caller, "web.user_disabled", &idem, &digest)
            .await?
        {
            return Ok(replayed);
        }
        let user = user_param(user)?;
        if !body.is_empty() {
            let _: DisableBody = parse_json(headers, body)?;
        }
        let done = retry_chain(|| st.app.web_disable_user(&caller, &user))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        st.record_outcome(
            &caller,
            "web.user_disabled",
            &idem,
            &digest,
            StatusCode::OK,
            &json!({ "keys_cut_off": done.keys_cut_off, "journal_seq": done.journal_seq }),
        )
        .await
    }

    // -- #10 POST /v2/web/users/{user}/deletion/close ----------------------------

    pub(crate) async fn close_deletion(
        State(st): State<Self>,
        rid: Rid,
        Path(user): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        finish(
            &rid,
            Self::close_deletion_inner(&st, &user, &headers, &body).await,
        )
    }

    /// `POST /v2/web/users/{user}/deletion/close`, admin by key role: a
    /// pending deletion request is closed, the person disabled first, and
    /// journalled as `web.deletion_closed`; a user with no pending request
    /// is `access.not_found` (ADR-0068 d6). `outcome` is `deleted`, the one
    /// close this build makes.
    async fn close_deletion_inner(
        st: &Self,
        user: &str,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> Result<Response, ApiError> {
        st.require_writes_open()?;
        let caller = st.console_caller(headers).await?;
        let idem = idempotency_key(headers)?;
        let digest = act_digest(user, body);
        if let Some(replayed) = st
            .replayed(&caller, "web.deletion_closed", &idem, &digest)
            .await?
        {
            return Ok(replayed);
        }
        let user = user_param(user)?;
        let req: CloseBody = parse_json(headers, body)?;
        if req.outcome != "deleted" {
            return Err(ApiError::validation("outcome is deleted")
                .with_details(json!({ "field": "/outcome" })));
        }
        let seq = retry_chain(|| st.app.web_close_deletion(&caller, &user))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        st.record_outcome(
            &caller,
            "web.deletion_closed",
            &idem,
            &digest,
            StatusCode::OK,
            &json!({ "journal_seq": seq }),
        )
        .await
    }

    // -- d5's unchanged key operations, as a session key -------------------

    /// `POST /v2/keys` as `<user>-web`: the person's own issuance, a reader
    /// or writer key named `<user>-<suffix>` on their root, its grants the
    /// use case's, never the body's; every other field is refused naming
    /// it. Any other session capacity is `session_capacity` (ADR-0070 d2,
    /// d5; ADR-0045 d9, d16).
    pub(crate) async fn issue_key_as_session(
        st: &Self,
        caller: &Caller,
        req: &KeyIssueBody,
        name: &Name,
    ) -> Result<Response, ApiError> {
        if !is_web_session_key(caller) {
            return Err(session_capacity(caller, "issue_key"));
        }
        let set: [(&str, bool); 9] = [
            ("/grants", !req.grants.is_empty()),
            ("/label", req.label.is_some()),
            ("/principal", req.principal.is_some()),
            ("/held_by", req.held_by.is_some()),
            ("/default_workarea", req.default_workarea.is_some()),
            ("/crew", req.crew.is_some()),
            ("/user", req.user.is_some()),
            ("/guardrail_scope", req.guardrail_scope.is_some()),
            (
                "/profile",
                req.profile.is_some() || req.expiry.is_some() || req.tenant.is_some(),
            ),
        ];
        if let Some((field, _)) = set.iter().find(|(_, set)| *set) {
            return Err(ApiError::validation(format!(
                "{field} is the person's own issuance's to set: send name, role and connect_code"
            ))
            .with_details(json!({ "field": field })));
        }
        let user = caller
            .key()
            .session_user
            .clone()
            .ok_or_else(ApiError::not_found)?;
        let suffix = name
            .as_str()
            .strip_prefix(&format!("{}-", user.as_str()))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ApiError::validation(format!("a key you issue is named {}-<name>", user.as_str()))
                    .with_details(json!({ "field": "/name" }))
            })?
            .to_owned();
        let role = parse_role(&req.role)?;
        let issued = retry_chain(|| {
            st.app
                .web_issue_key(caller, &suffix, role, req.connect_code)
        })
        .await
        .map_err(|e| match e {
            AppError::WebField { field, message } => {
                ApiError::validation(message).with_details(json!({ "field": format!("/{field}") }))
            }
            e => map_app_error(e, ErrorContext::Write),
        })?;
        Ok(json_response(
            StatusCode::CREATED,
            &key_issued_json(&issued, None),
        ))
    }

    /// `POST /v2/keys/{key}/denials` as a session key: `<user>-web` cuts
    /// off one of the person's own keys, `<user>-admin` one of the
    /// tenant's, each only on `universe` and never a session key, as the
    /// pages' `cut_off` does (ADR-0070 d5; ADR-0045 d9, d13).
    pub(crate) async fn cut_off_as_session(
        st: &Self,
        caller: &Caller,
        key: &Reference,
        node: &Reference,
    ) -> Result<oml_app::types::DenialRecord, ApiError> {
        let capacity_web = is_web_session_key(caller);
        if !capacity_web && caller.key().role != Role::Admin {
            return Err(session_capacity(caller, "deny_key"));
        }
        if !matches!(node, Reference::Name(n) if n.as_str() == "universe") {
            return Err(
                ApiError::validation("a session key cuts a key off on universe only")
                    .with_details(json!({ "field": "/workarea" })),
            );
        }
        let Reference::Name(key) = key else {
            return Err(
                ApiError::validation("a session key names the key by its name")
                    .with_details(json!({ "field": "key" })),
            );
        };
        retry_chain(|| st.app.web_cut_off(caller, key))
            .await
            .map_err(|e| match e {
                AppError::WebField { field, message } => {
                    ApiError::validation(message).with_details(json!({ "field": field }))
                }
                e => map_app_error(e, ErrorContext::Write),
            })
    }

    /// `POST /v2/keys/{key}/connect-codes` as `<user>-web`: a fresh code
    /// for one of the person's own connector keys, at the default lifetime;
    /// another person's key is `access.not_found` (ADR-0070 d5; ADR-0045
    /// d9).
    pub(crate) async fn connect_code_as_session(
        st: &Self,
        caller: &Caller,
        key: &Reference,
        ttl_seconds: Option<i64>,
    ) -> Result<Response, ApiError> {
        if !is_web_session_key(caller) {
            return Err(session_capacity(caller, "mint_connect_code"));
        }
        if ttl_seconds.is_some() {
            return Err(
                ApiError::validation("a person's connect code lives the default lifetime")
                    .with_details(json!({ "field": "/ttl_seconds" })),
            );
        }
        let Reference::Name(key) = key else {
            return Err(
                ApiError::validation("a session key names the key by its name")
                    .with_details(json!({ "field": "key" })),
            );
        };
        let code = retry_chain(|| st.app.web_connect_code(caller, key))
            .await
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
        Ok(json_response(
            StatusCode::CREATED,
            &connect_code_json(&code),
        ))
    }
}
