//! The typed error envelope of docs/api/errors.md: every non-2xx response is
//! `{error: {code, message, rule, details?, retryable}, request_id}`, codes
//! from the closed taxonomy, `rule` stable per code.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::{Value, json};

use oml_app::domain::oauth::{
    AuthorizationCodeInvalid, ClientInvalid, ConnectRefused, Refusal, Validation,
};
use oml_app::domain::{VerdictError, WithdrawRefusal};
use oml_app::ports::StoreError;
use oml_app::types::AppError;

/// One refusal, carrying everything the envelope needs except the request id
/// (supplied where the response is finished).
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub rule: &'static str,
    pub retryable: bool,
    pub details: Option<Value>,
    /// The seconds `Retry-After` names, where a refusal knows them; a
    /// retryable refusal without them names 1 (ADR-0068 d5, d9).
    pub retry_after: Option<u64>,
}

impl ApiError {
    pub fn new(
        status: StatusCode,
        code: &'static str,
        rule: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            rule,
            retryable: false,
            details: None,
            retry_after: None,
        }
    }

    /// Marks `response` with the quota this refusal names, if any, for
    /// `oml_quota_refused_total`; a page's refusal marks its page too
    /// (ADR-0068 d5).
    pub fn mark_quota(&self, response: &mut Response) {
        if let Some(quota) = self
            .details
            .as_ref()
            .and_then(|d| d.get("quota"))
            .and_then(Value::as_str)
            .and_then(oml_app::domain::signup::Quota::parse)
        {
            response
                .extensions_mut()
                .insert(crate::traffic::QuotaRefused(quota.as_str()));
        }
    }

    /// Retryable after `secs` seconds, which `Retry-After` names.
    #[must_use]
    pub fn retry_after(mut self, secs: u64) -> Self {
        self.retryable = true;
        self.retry_after = Some(secs.max(1));
        self
    }

    #[must_use]
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    #[must_use]
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn auth_missing() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "auth.missing",
            "Every request on either ring authenticates; only /healthz and /readyz do not.",
            "no Authorization: Bearer header",
        )
    }

    pub fn auth_invalid(reason: &str) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "auth.invalid",
            "Tenant and identity derive from the key, server-side, or not at all (D6).",
            format!("the presented credential is not valid: {reason}"),
        )
        .with_details(json!({ "reason": reason }))
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "request.validation",
            "The contract is openapi.yaml; what it does not admit is refused, not coerced.",
            message,
        )
    }

    /// A field or header v0.4.0 renamed: `request.validation` naming the
    /// spelling sent in `details.field` and the one to send in
    /// `details.renamed_to`, a body field as its JSON pointer
    /// (`/include_own_held`) and a header by its name, the convention of
    /// every other `details.field`; no code is added for it (errors.md
    /// `request.validation`; ADR-0039 d11). The key and workspace routes
    /// call it for ADR-0039 d7's admin renames, and the memory routes and
    /// the events tail for its workspace and recall renames; the MCP
    /// arguments' renames are not built yet.
    pub fn renamed(field: &str, renamed_to: &str) -> Self {
        Self::validation(format!(
            "{field} is renamed {renamed_to}: send {renamed_to}"
        ))
        .with_details(json!({ "field": field, "renamed_to": renamed_to }))
    }

    pub fn idempotency_key_required() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "request.idempotency_key_required",
            "Every side-effecting request is replayable exactly once by construction (D13).",
            "a state-creating POST requires the Idempotency-Key header",
        )
    }

    pub fn unsupported_media(got: &str) -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "request.unsupported_media",
            "Each operation's request media type is fixed by the contract; a body in any other type is refused before parsing.",
            format!("this operation takes application/json, not {got:?}"),
        )
        .with_details(json!({ "supported": ["application/json"] }))
    }

    pub fn payload_too_large(what: &str) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request.payload_too_large",
            "Size bounds are admission policy, enforced before bytes are stored.",
            format!("{what} exceeds the admitted size"),
        )
    }

    pub fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "Reads fail closed as absence: the service never confirms the existence of what a key may not see (D7, R3).",
            "not found",
        )
    }

    pub fn serve_invalid(reason: &str) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "review.serve_invalid",
            "A verdict counts only for exactly what was served (D14).",
            format!("the named serve is not valid: {reason}"),
        )
        .with_details(json!({ "reason": reason }))
    }

    pub fn unavailable(reason: &str) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "system.unavailable",
            "A replica that cannot prove its journal head accepts no writes (D1).",
            format!("the replica cannot serve safely: {reason}"),
        )
        .with_details(json!({ "reason": reason }))
        .retryable()
    }

    /// A handler panicked and the panic guard turned it into this answer;
    /// the panic's text stays in the log (`event=handler_panic`).
    pub fn handler_panic() -> Self {
        Self::internal("the request hit a defect; the request_id names it in the lead's log")
            .with_details(json!({ "reason": "handler_panic" }))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "system.internal",
            "—",
            message,
        )
    }

    /// The envelope's `error` object without the request id — the shape the
    /// MCP facade carries as a refused tool's structured content.
    #[must_use]
    pub fn error_object(&self) -> Value {
        let mut error = json!({
            "code": self.code,
            "message": self.message,
            "rule": self.rule,
            "retryable": self.retryable,
        });
        if let (Some(details), Some(obj)) = (&self.details, error.as_object_mut()) {
            obj.insert("details".to_owned(), details.clone());
        }
        error
    }

    /// Finishes the refusal as the envelope response, carrying the request id
    /// in the body and the `X-Request-Id` header (errors.md "The envelope").
    #[must_use]
    pub fn into_response(self, request_id: &str) -> Response {
        let body = json!({ "error": self.error_object(), "request_id": request_id });
        let mut response = Response::new(axum::body::Body::from(body.to_string()));
        *response.status_mut() = self.status;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        // A refusal naming a quota is counted by the request layer
        // (ADR-0068 d5).
        self.mark_quota(&mut response);
        if self.retryable {
            // Retryable refusals carry Retry-After (errors.md envelope table).
            let secs = self.retry_after.unwrap_or(1).to_string();
            if let Ok(value) = HeaderValue::from_str(&secs) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

/// `submission.tip_conflict` with its stable rule (errors.md).
fn tip_conflict(message: &str) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "submission.tip_conflict",
        "Supersession promotion CAS-checks its target is the current tip; the tip flip is one journalled fact, never a lost update.",
        message,
    )
}

/// `workspace.transition_refused` with its `{workspace_id, from, to, reason}`
/// details (errors.md, workspace structure and status refusals).
fn transition_refused(
    workspace_id: uuid::Uuid,
    from: &str,
    to: &str,
    reason: &str,
    message: String,
) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "workspace.transition_refused",
        "The status ladder moves forward only — development → protected → frozen — and a workspace becomes frozen only when all its children are.",
        message,
    )
    .with_details(json!({
        "workspace_id": workspace_id.to_string(),
        "from": from,
        "to": to,
        "reason": reason,
    }))
}

/// Which endpoint family the refusal surfaced on — a refused status
/// transition means different things to a verdict (the serve no longer names
/// a held row) and to a write (the target is not the current tip).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorContext {
    Review,
    Write,
}

/// The refusal of an act that would leave no live admin person, as
/// `oml-app` words it (ADR-0097 d25).
const LAST_ADMIN_MESSAGE: &str =
    "this is the organisation's last admin address; add another admin first";

/// The one place `oml-app`'s typed refusals become errors.md codes
/// (errors.md "Implementation mapping": the attachment happens once, at the
/// transport edge).
#[must_use]
pub fn map_app_error(err: AppError, ctx: ErrorContext) -> ApiError {
    match err {
        AppError::Unauthorized => ApiError::auth_invalid("unknown"),
        // Told only to a caller that presented the key's own credential
        // (ADR-0079 d2).
        AppError::KeyExpired { expiry } => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "auth.invalid",
            "From its expiry on a key authenticates nothing; its row stays, and an admin key may move the date (ADR-0079 d2, d5).",
            format!("this key expired at {}", expiry.as_str()),
        )
        .with_details(json!({ "reason": "expired", "expiry": expiry.as_str() })),
        AppError::KeyRevocation(refusal) => ApiError::validation(refusal.message())
            .with_details(json!({ "field": "key", "reason": refusal.reason() })),
        AppError::Timebox(refusal) => ApiError::validation(refusal.message())
            .with_details(json!({ "field": "/timebox", "reason": refusal.reason() })),
        AppError::AdminAct(refusal) => ApiError::validation(refusal.message())
            .with_details(json!({ "field": refusal.field(), "reason": refusal.reason() })),
        AppError::KeyPendingConfirmation => ApiError::auth_invalid("pending_confirmation"),
        AppError::KeySuspended => ApiError::auth_invalid("suspended"),
        AppError::KeyLinkPending => ApiError::auth_invalid("link_pending"),
        AppError::KeyPerson(reason) => ApiError::validation(err.to_string())
            .with_details(json!({ "field": "/person", "reason": reason })),
        AppError::AdminActReasonRequired => ApiError::validation(
            "this admin act waits on a second admin person, and requires a reason",
        )
        .with_details(json!({ "field": "/reason", "reason": "required" })),
        AppError::KeyRotation(refusal) => ApiError::validation(refusal.message()).with_details(
            json!({ "field": refusal.field(), "reason": refusal.reason() }),
        ),
        AppError::KeyLimits(refusal) => {
            let mut details = serde_json::Map::new();
            if let Some(field) = refusal.field() {
                details.insert("field".to_owned(), json!(field));
            }
            if let Some(reason) = refusal.reason() {
                details.insert("reason".to_owned(), json!(reason));
            }
            let refused = ApiError::validation(refusal.message());
            if details.is_empty() {
                refused
            } else {
                refused.with_details(Value::Object(details))
            }
        }
        AppError::VerbCeiling { role, verb } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: the surface an agent can call never promotes, signs, demotes or deletes.",
            format!("{verb} is outside the {} verb ceiling", role.as_str()),
        )
        .with_details(json!({ "verb": verb, "role": role.as_str() })),
        // A denial wins over every grant, so a session key that carries
        // one hands on no access (ADR-0070 d2; #325).
        AppError::SessionKeyDenied { role, verb } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "A session key an admin denied runs none of its key acts: it issues no key, mints no connect code and cuts off no key (ADR-0070 d2).",
            format!(
                "{verb} is refused: an admin denied this session key, so it issues no key, mints no connect code and cuts off no key; signing out still works"
            ),
        )
        .with_details(json!({
            "verb": verb,
            "role": role.as_str(),
            "reason": "session_key_denied",
        })),
        // The grace keeps a deployed value working, never its key acts
        // (ADR-0096 d13, the operator's decision of 2026-11-25).
        AppError::PreviousKeyValue { verb } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "A rotated key's previous value authenticates until its grace ends and runs none of the key acts: it rotates, issues, mints, cuts off, denies, sets limits and links nothing (ADR-0096 d13).",
            format!(
                "{verb} is refused: this value is the key's previous one, kept only for its grace; use the new value"
            ),
        )
        .with_details(json!({
            "verb": verb,
            "reason": "previous_value",
        })),
        AppError::IssuerCeiling {
            role,
            admissible,
            grant_index,
        } => {
            // One-hop recovery details (ADR-0025 d2, d16): the refused role,
            // the set that would be admitted, and which grant it was.
            let mut details = json!({
                "role": role.as_str(),
                "admissible": admissible.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
            });
            if let (Some(index), Some(obj)) = (grant_index, details.as_object_mut()) {
                obj.insert("grant_index".to_owned(), json!(index));
            }
            let message = match grant_index {
                None => format!("the issuer may not issue a {} key", role.as_str()),
                Some(index) => format!(
                    "grant {index} has role {}, which the key role does not admit",
                    role.as_str()
                ),
            };
            ApiError::new(
                StatusCode::FORBIDDEN,
                "access.issuer_ceiling",
                "A key is issued with a role no higher than the issuer's (D9).",
                message,
            )
            .with_details(details)
        }
        AppError::GrantOutsideUserRoot {
            role,
            admissible,
            grant_index,
            user,
        } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.issuer_ceiling",
            "A key is issued with a role no higher than the issuer's (D9).",
            format!(
                "grant {grant_index} lies outside the root of user {}: a user key's every grant lies in its own root",
                user.as_str()
            ),
        )
        .with_details(json!({
            "role": role.as_str(),
            "admissible": admissible.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
            "grant_index": grant_index,
            "user": user.as_str(),
        })),
        AppError::UserRefused(why) => {
            ApiError::validation(why).with_details(json!({ "field": "/user" }))
        }
        AppError::NodeGrantDeny { grant_index } => ApiError::validation(format!(
            "grant {grant_index} covers its node alone, so it takes no deny entry"
        ))
        .with_details(json!({ "field": format!("/grants/{grant_index}/deny") })),
        AppError::UniverseRefused => ApiError::new(
            StatusCode::CONFLICT,
            "submission.universe_refused",
            "Universe holds only curated claims: a curator writes there and a second identity reviews it; a user key reads it and never writes it (ADR-0044 d7).",
            "universe is curated: a user key never writes there",
        ),
        AppError::UserRootRefused => ApiError::new(
            StatusCode::CONFLICT,
            "submission.user_root_refused",
            "A memory is started in a user root by its own user's keys only: a new memory, or a successor of a ratified retirement, which puts the claim back in service. A key granted there reads, reviews and proposes a successor or a retirement of a served memory (ADR-0044 d16).",
            "this workspace lies in a user's root: only that user's keys start a memory there",
        ),
        AppError::ReviewKeyInOwnRoot => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "Inside a person's own root their web key starts a claim; their review key starts one only in a shared workspace where it holds reviewer, so every write in the root counts on the person's account (ADR-0082 d2, d10).",
            "this workspace lies in the person's own root: start the claim as the person's web key",
        )
        .with_details(json!({
            "reason": "review_key_in_own_root",
            "verb": "submit",
            "role": "reviewer",
        })),
        AppError::UserRootInvalid => ApiError::auth_invalid("user_root"),
        AppError::RoleExclusive => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.role_exclusive",
            "Admin is never a reviewer (D9): a key never combines admin with reviewer verbs.",
            "a key holding the admin role never carries reviewer-class grants",
        ),
        AppError::GrantRefused {
            workspace_id,
            needed_role,
        } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.grant_refused",
            "Effective access is the union of grants, each attenuated by its denies, deny-wins for global denials; what a grant does not confer, the key does not have (D7).",
            format!(
                "the key reads this workspace but holds no effective {} role on it",
                needed_role.as_str()
            ),
        )
        .with_details(json!({
            "workspace_id": workspace_id.to_string(),
            "needed_role": needed_role.as_str(),
        })),
        AppError::DenyOutsideGrant {
            grant_index,
            subtree_node,
        } => ApiError::validation(format!(
            "grant {grant_index}'s deny names {subtree_node}, which lies outside the grant's subtree"
        )),
        AppError::ContributorGrants => ApiError::validation(
            "a contributor key carries at least one reader grant and one submitter grant; the server takes exactly the grants it is sent and adds none",
        )
        .with_details(json!({ "field": "/grants", "reason": "contributor_grants" })),
        AppError::DefaultUncovered { workspace } => ApiError::validation(format!(
            "default_workspace_id {workspace} is covered by no grant of the key"
        )),
        AppError::SearchUnavailable { name, served } => ApiError::validation(format!(
            "search {name:?} is not served here; served: {served}"
        ))
        .with_details(json!({ "field": "/search", "served": served.split(", ").collect::<Vec<_>>() })),
        AppError::ConnectCodeLifetime { seconds, max } => ApiError::validation(format!(
            "ttl_seconds {seconds} is outside 1 to {max}, the span a connect code may live"
        ))
        .with_details(json!({ "field": "/ttl_seconds", "max": max })),
        AppError::BudgetWithoutBodies => ApiError::validation(
            "max_tokens is refused with include_text false: a budget counts text that would not be sent",
        ),
        AppError::WorkspaceUnresolved => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.workspace_unresolved",
            "The target workspace comes from the token (ADR-0022 d4); a key with neither a default nor a workspace field is refused, never guessed for (ADR-0039 d7).",
            "the key has no default_workspace and the request names none",
        ),
        AppError::AfterSeqBeyondHead {
            after_seq,
            head_seq,
        } => ApiError::validation(format!(
            "after_seq {after_seq} is beyond head_seq {head_seq}: pass back a next_after_seq this tail served"
        )),
        AppError::NotFound => ApiError::not_found(),
        AppError::Upload(refusal) => upload_error(&refusal),
        AppError::Bundle(refusal) => bundle_error(&refusal),
        AppError::ImportTargetNotInProgress { workspace_id } => ApiError::new(
            StatusCode::CONFLICT,
            "import.target_not_in_progress",
            "An import lands in a development workspace in progress, or a new name it creates in progress, and every version it writes is held (ADR-0084 d6).",
            "the import target is not a development workspace in progress",
        )
        .with_details(json!({ "workspace_id": workspace_id.to_string() })),
        AppError::CreateIsOwnRootOnly { role } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: over MCP a contributor key naming a user creates a child inside that person's own root, and no other key creates (ADR-0095 d3).",
            format!(
                "create_workspace needs a contributor key that names a user, and this is a {} key",
                role.as_str()
            ),
        )
        .with_details(json!({
            "verb": "create_workspace",
            "role": role.as_str(),
            "reason": "create_is_own_root_only",
        })),
        AppError::ParentFrozen { workspace_id } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "A grant carrying create admits a child under an unfrozen parent only; a frozen node never acquires a descendant (ADR-0097 d31; ADR-0009 d5).",
            "create_workspace under a frozen parent is refused",
        )
        .with_details(json!({
            "verb": "create_workspace",
            "workspace_id": workspace_id.to_string(),
            "reason": "parent_frozen",
        })),
        AppError::TransitionPending { transition_id } => ApiError::new(
            StatusCode::CONFLICT,
            "proposal.pending",
            "A workspace holds one live proposal per target status, and a claim one proposal per brief; a second identity ratifies the live one (ADR-0095 d5, d6).",
            "a live proposal of this status move is already pending on this workspace",
        )
        .with_details(json!({ "transition_id": transition_id.to_string() })),
        AppError::BriefEdgePending { edge_id } => ApiError::new(
            StatusCode::CONFLICT,
            "proposal.pending",
            "A workspace holds one live proposal per target status, and a claim one proposal per brief; a second identity ratifies the live one (ADR-0095 d5, d6).",
            "this claim is already proposed for this brief or is a member of it",
        )
        .with_details(json!({ "edge_id": edge_id.to_string() })),
        AppError::ImportPersonOnly { role, used_by } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: the surface an agent can call never imports; a person's contributor key imports (ADR-0084 d11; ADR-0093 d6).",
            format!(
                "an import needs a contributor key used by a person, and this is a {} key used by {}",
                role.as_str(),
                used_by.as_str()
            ),
        )
        .with_details(json!({
            "verb": "import",
            "role": role.as_str(),
            "used_by": used_by.as_str(),
            "reason": "import_person_only",
        })),
        AppError::ImportExpectKeyRequired => ApiError::validation(
            "a first import from a source names the source key's fingerprint in expect_key, which the target then pins (ADR-0084 R11)",
        )
        .with_details(json!({ "field": "/expect_key", "reason": "first_import" })),
        AppError::ImportOverQuota(reached) => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "import.over_quota",
            "An import's whole plan fits the target account's meters, or nothing is written (ADR-0084 d7).",
            format!(
                "the import does not fit the {} quota of {}",
                reached.quota.as_str(),
                reached.limit
            ),
        )
        .with_details(json!({
            "quota": reached.quota.as_str(),
            "limit": reached.limit,
            "used": reached.used,
        })),
        AppError::ImportBusy => ApiError::new(
            StatusCode::CONFLICT,
            "import.busy",
            "An import holds its bundle in memory, so a tenant runs one import or dry run at a time and the server a bounded number (ADR-0084 I13).",
            "an import or dry run of this tenant's is running, or the server runs its bound of them; retry when it ends",
        )
        .retryable(),
        AppError::ExportBusy => ApiError::new(
            StatusCode::CONFLICT,
            "export.busy",
            "An export builds its bundle in memory, so a tenant runs one export at a time (ADR-0084 I13).",
            "an export of this tenant's is running; retry when it ends",
        )
        .retryable(),
        AppError::ExportTooLarge { max_bytes } => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "export.too_large",
            "A bundle is at most uploads.max_bytes, the most an import on any server of this build takes (ADR-0084 I13).",
            format!("the bundle would be larger than {max_bytes} bytes; export a smaller subtree"),
        )
        .with_details(json!({ "max_bytes": max_bytes })),
        AppError::ExportPersonOnly { role, used_by } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "A workspace export is a person's act: no agent, admin or service key exports (ADR-0084 d10).",
            format!(
                "exporting a workspace needs a key used by a person, and this is a {} key used by {}",
                role.as_str(),
                used_by.as_str()
            ),
        )
        .with_details(json!({
            "reason": "export_person_only",
            "role": role.as_str(),
            "used_by": used_by.as_str(),
        })),
        AppError::ExportDisabled { reason } => {
            let error = ApiError::new(
                StatusCode::FORBIDDEN,
                "export.disabled",
                "The tenant's export policy decides what may leave the server (ADR-0084 d10).",
                match reason {
                    Some(_) => "the tenant's export policy is own_root, this workspace lies outside the exporter's own root, and no admin allowance is in force",
                    None => "the tenant's export policy is off",
                },
            );
            match reason {
                Some(reason) => error.with_details(json!({ "reason": reason })),
                None => error.with_details(json!({ "reason": "off" })),
            }
        }
        AppError::ExportNotReadable => ApiError::new(
            StatusCode::FORBIDDEN,
            "export.not_readable",
            "An export carries a whole subtree, so a node the exporter cannot read refuses all of it (ADR-0084 d10).",
            "a workspace of the subtree is not readable by this key",
        ),
        AppError::BundleUnencrypted { detail } => ApiError::new(
            StatusCode::BAD_REQUEST,
            "bundle.unencrypted",
            "A bundle is always encrypted to at least one named age recipient (ADR-0084 d5).",
            detail,
        )
        .with_details(json!({ "field": "/recipients" })),
        AppError::ExportModeRefused { field, reason } => ApiError::validation(format!(
            "{field} cannot be taken: {}",
            if reason == "releases_unbuilt" {
                "releases are not built, so an export is history mode only"
            } else {
                reason
            }
        ))
        .with_details(json!({ "field": format!("/{field}"), "reason": reason })),
        AppError::LineageUnknown(id) => ApiError::not_found().with_details(json!({
            // derives_from targets fail closed as absence, like CURIE
            // citations (errors.md vocab.unknown_curie rationale).
            "derives_from": id.to_string(),
        })),
        AppError::AuthorOnly => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.author_only",
            "A submitter proposes supersessions of its own claims only (D9).",
            "a key without the reviewer role may supersede only its own claims",
        ),
        AppError::Withdraw(WithdrawRefusal::AuthorOnly) => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.author_only",
            "Only a version's author withdraws it: its own key, or its person's session key (ADR-0083 d13).",
            "only the version's author withdraws it",
        ),
        AppError::Withdraw(WithdrawRefusal::NotHeld) => ApiError::new(
            StatusCode::CONFLICT,
            "memory.not_held",
            "Only a held version is withdrawn; a reviewed version is superseded or retired instead (ADR-0083 d13).",
            "the version is not held",
        ),
        AppError::TipConflict { current_tip } => {
            let error = tip_conflict("the supersede target is not the current served tip");
            match current_tip {
                // One-hop recovery: the tip to propose against, named only
                // when the caller can read it (ADR-0025 d10, d16).
                Some(tip) => error.with_details(json!({
                    "current_tip": {
                        "artifact_id": tip.artifact_id.to_string(),
                        "version": tip.version,
                    },
                })),
                None => error,
            }
        }
        AppError::VersionConflict { current } => ApiError::new(
            StatusCode::CONFLICT,
            "submission.version_conflict",
            "A supersession names the claim's latest version, so two sessions that read the same version never each leave a successor for the reviewer to arbitrate (ADR-0057 d19).",
            format!("the claim's latest version is {current}: read it again before superseding"),
        )
        .with_details(json!({ "current_version": current })),
        AppError::FormatUnknown { allowed } => ApiError::validation(
            "format is not one of the accepted values: send one of details.allowed",
        )
        .with_details(json!({ "field": "format", "allowed": allowed })),
        AppError::FormatKindRefused { kind } => ApiError::validation(format!(
            "a {} claim is read by rules that expect prose: format is text/plain or text/markdown",
            kind.as_str()
        ))
        .with_details(json!({ "field": "format", "allowed": ["text/plain", "text/markdown"] })),
        AppError::TitleInvalid(reason) => ApiError::validation(format!("title: {reason}"))
            .with_details(json!({ "field": "title" })),
        AppError::FormatInvalid(refusal) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "submission.format_invalid",
            "The source is the signed text, so it must be a document its format accepts; the refusal names the rule and where it was found (ADR-0056 d13).",
            format!("the source is not valid {}", refusal),
        )
        .with_details(json!({
            "reason": refusal.reason,
            "line": refusal.line,
            "column": refusal.column,
        })),
        AppError::AnchorScanVeto { field, finding } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "submission.scan_veto",
            "The scan is rules, not a model: same input, same verdict — and it can veto but never promote (D11). An anchor is kept for good, so a finding in it refuses the write (ADR-0057 d2).",
            format!("the deterministic scan vetoed {field}: {finding}"),
        )
        .with_details(json!({ "finding": finding, "field": field })),
        AppError::SourceCheckInvalid { field, reason } => {
            ApiError::validation(format!("{field}: {reason}"))
                .with_details(json!({ "field": field }))
        }
        AppError::WorkspaceNotTarget { workspace } => ApiError::validation(format!(
            "workspace names {workspace}, but a supersession is captured in its target's workspace"
        ))
        .with_details(json!({ "field": "/workspace" })),
        AppError::KindChangeRefused { target, sent } => ApiError::validation(format!(
            "a {} target admits no {} successor: a kind changes only within its class, and a retirement keeps the target's",
            target.as_str(),
            sent.as_str()
        )),
        AppError::TermRetirement => ApiError::validation(
            "a term is never retired: terms deprecate, through a supersession with term_status deprecated",
        ),
        AppError::GuardrailSeverityRequired => ApiError::validation(
            "a guardrail carries a severity, never, must or should, unless it retires",
        )
        .with_details(json!({ "field": "/severity" })),
        AppError::GuardrailFieldRefused { field } => ApiError::validation(format!(
            "{field} is a guardrail's field, refused on any other kind and on a retirement"
        ))
        .with_details(json!({ "field": format!("/{field}") })),
        AppError::KindField(refusal) => ApiError::validation(refusal.reason)
            .with_details(json!({ "field": refusal.field })),
        e @ AppError::DemoteKindUnsupported { .. } => ApiError::validation(e.to_string())
            .with_details(json!({ "reason": "lifecycle.demote_kind_unsupported" })),
        AppError::KeyLinkRefused { reason } => {
            // ADR-0085 d16: the refusal names the member at fault, as a
            // limit's refusal does (ADR-0079 d5).
            let field = if matches!(reason, "user_unknown" | "user_disabled" | "self_link") {
                "/user"
            } else {
                "key"
            };
            ApiError::validation(format!("the key cannot be linked or unlinked: {reason}"))
                .with_details(json!({ "field": field, "reason": reason }))
        }
        AppError::WebEmailRefused { reason } => ApiError::validation(format!(
            "the sign-in email cannot be accepted or removed here: {reason}"
        ))
        .with_details(json!({
            "field": if reason == "changed" { "/admin" } else { "/email" },
            "reason": reason,
        })),
        AppError::ActingKeyNotLinked => ApiError::auth_invalid("acting_key_not_linked"),
        AppError::SkillBodyTooLarge => ApiError::payload_too_large("a skill body past 32,768 bytes")
            .with_details(json!({ "field": "/body" })),
        AppError::TooManyPassages { count } => ApiError::validation(format!(
            "a document holds at most 4,096 passages; this one would hold {count}"
        ))
            .with_details(json!({ "field": "/body", "reason": "too_many_passages" })),
        AppError::ValidTime(err) => ApiError::validation(err.to_string())
            .with_details(json!({ "field": err.field() })),
        AppError::AsOfInvalid { field, reason } => {
            ApiError::validation(reason).with_details(json!({ "field": field }))
        }
        AppError::NowUnparsable => ApiError::validation(
            "now must be YYYY, YYYY-MM, YYYY-MM-DD or an RFC 3339 date-time with an offset",
        )
        .with_details(json!({ "field": "/now" })),
        // Raised only from the agent plugin, whose oml_write names the
        // target `replaces` (ADR-0039 d3).
        AppError::VocabularyTarget(kind) => ApiError::validation(format!(
            "a {} target is superseded on the HTTP API: the plugin's tools carry no vocabulary fields",
            kind.as_str()
        ))
        .with_details(json!({ "field": "/replaces" })),
        AppError::DerivedClaimKeySent => ApiError::validation(
            "claim_key starts auto-, the prefix of the keys the server derives: omit claim_key to have one derived, or send one with another prefix",
        )
        .with_details(json!({ "field": "/claim_key" })),
        // A verdict without serve_id defaults to the one claimed item
        // (ADR-0039 d10).
        AppError::ServeIdUnresolved { several } => ApiError::validation(format!(
            "serve_id is omitted and this key has taken {} review items: send the serve_id review next answered",
            if several { "several" } else { "no" }
        ))
        .with_details(json!({ "field": "/serve_id" })),
        // A brief serves only promoted served tips (ADR-0046 d8).
        AppError::BriefMemberNotServed { artifact_id } => ApiError::validation(format!(
            "artifact {artifact_id} is not a promoted served tip: only a claim review promoted joins a brief"
        ))
        .with_details(json!({ "field": "/from_artifact_id" })),
        // A claim-edge label the domain refused, naming the body field
        // (errors.md request.validation; ADR-0021 d5).
        AppError::ClaimEdgeLabel { field, reason } => {
            ApiError::validation(reason.to_string()).with_details(json!({ "field": field.as_str() }))
        }
        // Bulk review is a person's act (ADR-0055 d1).
        AppError::BulkReviewHumanOnly { role, used_by } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: the surface an agent can call never promotes, signs, demotes or deletes; bulk review is a person's act.",
            format!(
                "bulk review needs a reviewer key used by a person, and this is a {} key used by {}",
                role.as_str(),
                used_by.as_str()
            ),
        )
        .with_details(json!({
            "reason": "bulk_review_is_person_only",
            "role": role.as_str(),
            "used_by": used_by.as_str(),
        })),
        AppError::OriginInvalid { field, reason } => {
            ApiError::validation(format!("{field}: {reason}"))
                .with_details(json!({ "field": field }))
        }
        // The chat workspace is a person's to open (ADR-0063 d8).
        AppError::ChatScopeHumanOnly { used_by } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: an agent never opens the scope that holds a person's own words; the person does.",
            format!(
                "a workspace named chat under a user root is created by a key used by a person, and this key is used by {}",
                used_by.as_str()
            ),
        )
        .with_details(json!({
            "reason": "chat_scope_is_person_only",
            "used_by": used_by.as_str(),
        })),
        // Only a person's key sets a workspace's mode (ADR-0030 d1).
        AppError::ModeHumanOnly { role, used_by } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: the surface an agent can call never promotes, signs, demotes or deletes; a workspace's mode is set by a person.",
            format!(
                "setting a workspace's mode needs a reviewer key used by a person, and this is a {} key used by {}",
                role.as_str(),
                used_by.as_str()
            ),
        )
        .with_details(json!({
            "reason": "mode_is_person_only",
            "role": role.as_str(),
            "used_by": used_by.as_str(),
        })),
        // Only an admin person sets or clears `public` (ADR-0098 d6).
        AppError::PublicIsAdminOnly { role } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: marking a workspace public gives every reading key of the tenant a read, an admin person's act, never an agent's or a workspace reviewer's.",
            format!(
                "setting or clearing a workspace's public flag needs an admin person, a linked admin key or <user>-admin, and this is a {} key",
                role.as_str()
            ),
        )
        .with_details(json!({
            "reason": "public_is_admin_only",
            "role": role.as_str(),
        })),
        // What the node is or holds refuses the flag (ADR-0098 d5, d6).
        AppError::WorkspacePublicRefused {
            workspace_id,
            reason,
            act_id,
        } => {
            let mut details = json!({
                "workspace_id": workspace_id.to_string(),
                "reason": reason.reason(),
            });
            if let (Some(act_id), Some(obj)) = (act_id, details.as_object_mut()) {
                obj.insert("act_id".to_owned(), json!(act_id.to_string()));
            }
            ApiError::new(
                StatusCode::CONFLICT,
                "workspace.public_refused",
                "Only a protected or frozen workspace outside every user root is marked public, one set at a time, and only a public one is cleared.",
                reason.message().to_owned(),
            )
            .with_details(details)
        }
        // Only a person's reviewer key or a tenant admin sets a workspace's
        // federation field (ADR-0072 d5).
        AppError::FederationHumanOnly { role, used_by } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: a policy that changes what other keys' recalls search is a person's or an admin's act, never an agent's.",
            format!(
                "setting a workspace's federation field needs a reviewer key used by a person or an admin key, and this is a {} key used by {}",
                role.as_str(),
                used_by.as_str()
            ),
        )
        .with_details(json!({
            "reason": "federation_is_person_only",
            "role": role.as_str(),
            "used_by": used_by.as_str(),
        })),
        // The shape of `sources`, federation off, or a policy value out of
        // range (ADR-0072 d5).
        AppError::SourcesInvalid { field, message } => {
            ApiError::validation(message).with_details(json!({ "field": field }))
        }
        AppError::SourcesUnderCc { pipeline } => ApiError::validation(format!(
            "{pipeline} normalises each list by its own bounds, so sources wide and a list are refused on it; use another pipeline"
        ))
        .with_details(json!({ "field": "/search" })),
        // An unreadable and an absent listed name are the same bytes, with
        // the position the caller sent (ADR-0072 d5, d6).
        AppError::SourceNotFound { index } => {
            ApiError::not_found().with_details(json!({ "field": format!("/sources/{index}") }))
        }
        AppError::SourceRefused { index, reason } => ApiError::validation(format!(
            "sources/{index} is not a usable source: {}",
            match reason {
                oml_app::federation::SourceRefusal::Untrusted =>
                    "it is not protected or frozen, or lies inside an in-progress subtree",
                oml_app::federation::SourceRefusal::DeclinesFederation =>
                    "it declines federated reads from recalls scoped elsewhere",
            }
        ))
        .with_details(json!({
            "field": format!("/sources/{index}"),
            "reason": reason.as_str(),
        })),
        // A curator narrows inside an in-progress scope only (ADR-0030 d9).
        AppError::Curator { verb, refusal } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: a curator key narrows inside an in-progress scope and never widens.",
            format!("a curator key may not {verb} here"),
        )
        .with_details(json!({ "reason": refusal.as_str(), "verb": verb })),
        AppError::ProfileMismatch { profile } => ApiError::validation(format!(
            "profile {profile} needs a reviewer key used by an agent"
        ))
        .with_details(json!({ "field": "/profile" })),
        // ADR-0030 d1, d6, d7.
        AppError::InProgressRefused {
            workspace_id,
            reason,
        } => ApiError::new(
            StatusCode::CONFLICT,
            "workspace.in_progress_refused",
            "Until its commit, nothing crosses an in-progress subtree's boundary, and subtrees never nest.",
            format!("workspace {workspace_id}: {reason}"),
        )
        .with_details(json!({ "workspace": workspace_id, "reason": reason.as_str() })),
        // ADR-0030 d6: no count, so a committer learns nothing of rows it
        // cannot read.
        // The use case maps the store's in-transaction refusal to
        // HeldRemaining with the root; reaching here unmapped still answers
        // the code, naming no workspace (ADR-0030 d6).
        AppError::Store(StoreError::HeldRemaining) => ApiError::new(
            StatusCode::CONFLICT,
            "workspace.held_remaining",
            "Nothing leaves an in-progress workspace until every memory in it is reviewed.",
            "the subtree still holds unreviewed memories: review them, then commit".to_owned(),
        ),
        AppError::HeldRemaining { workspace_id } => ApiError::new(
            StatusCode::CONFLICT,
            "workspace.held_remaining",
            "Nothing leaves an in-progress workspace until every memory in it is reviewed.",
            format!(
                "workspace {workspace_id} still holds unreviewed memories in its subtree: review them, then commit"
            ),
        )
        .with_details(json!({ "workspace": workspace_id })),
        AppError::BulkShape { field, reason } => {
            ApiError::validation(reason).with_details(json!({ "field": field }))
        }
        AppError::AuthorInLineage => ApiError::new(
            StatusCode::FORBIDDEN,
            "review.author_in_lineage",
            "R2: the reviewer is never the author — enforced against the full lineage closure.",
            "the ratifying key or principal is in an endpoint's lineage closure",
        ),
        AppError::CountsAsNoPerson { act } => ApiError::new(
            StatusCode::FORBIDDEN,
            "review.person_required",
            "An instruction verdict, a fix settlement and a ladder ratification each need a key that counts as a person (ADR-0097 d12).",
            format!("this key counts as no person, so it takes no {act}"),
        )
        .with_details(json!({ "reason": "counts_as_no_person", "act": act })),
        AppError::RatifierSamePerson { issued_the_proposer } => ApiError::new(
            StatusCode::FORBIDDEN,
            "review.ratifier_is_proposer",
            "The ratifier counts as a person other than the proposer's, and other than the admin who issued an agent proposer (ADR-0097 d17).",
            "the ratifying key counts as the proposer's person",
        )
        .with_details(json!({
            "reason": if issued_the_proposer { "issued_the_proposer" } else { "same_person" },
        })),
        AppError::AdminReadRequiresPerson => ApiError::new(
            StatusCode::FORBIDDEN,
            "admin.read_requires_person",
            "The admin read needs a person-linked admin key or a session, and counts its reads per person (ADR-0097 d16).",
            "this admin key counts as no person",
        )
        .with_details(json!({ "reason": "counts_as_no_person" })),
        AppError::SamePerson => ApiError::new(
            StatusCode::FORBIDDEN,
            "review.author_in_lineage",
            "Outside a person's own root, no key acting for that person judges a version any of their keys wrote (ADR-0001 d5; ADR-0082 d7).",
            "this key acts for the person whose key wrote the version, outside that person's root",
        )
        .with_details(json!({ "reason": "same_person" })),
        // A decided edge answers as a verdict on a decided row does: errors.md
        // declares no edge-specific code (ADR-0021 d5).
        AppError::EdgeConsumed { edge_id } => ApiError::serve_invalid("consumed")
            .with_details(json!({ "reason": "consumed", "edge_id": edge_id.to_string() })),
        AppError::CycleRefused(refused) => ApiError::new(
            StatusCode::CONFLICT,
            "vocab.cycle_refused",
            "Term hierarchy and supersession chains are acyclic by domain invariant — a deterministic transaction-time check at ratification (ADR-0021).",
            refused.to_string(),
        )
        .with_details(json!({ "label": refused.label.as_str() })),
        // A stored term edge whose endpoint has no CURIE is a defect; the
        // ratification journalled nothing (ADR-0021 d2, d5).
        AppError::TermEndpoint { claim_key } => {
            ApiError::internal(format!("term edge endpoint {claim_key:?} is not a term's CURIE"))
        }
        AppError::ClaimKeyConflict => ApiError::new(
            StatusCode::CONFLICT,
            "submission.claim_key_conflict",
            "A claim_key names one live claim per (tenant, workspace); a claim whose every version is rejected or erased releases it (D7).",
            "a held, promoted or demoted version already holds this claim_key in the workspace",
        ),
        AppError::TimingsOff => ApiError::validation(
            "timings is refused: this deployment does not serve recall timings; an operator sets search.timings to serve them",
        )
        .with_details(json!({ "field": "/timings" })),
        AppError::SimilarExists(similar) => ApiError::new(
            StatusCode::CONFLICT,
            "submission.similar_exists",
            "The similar check names what the writing key may already read and decides nothing: with on_similar refuse the writing key reads the named rows and chooses, before anything waits for review (ADR-0050 d1).",
            "on_similar is refuse and the writing key may already read a row with the same text or a near copy",
        )
        .with_details(json!({ "similar": crate::similar_json(&similar) })),
        AppError::ScanVeto(finding) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "submission.scan_veto",
            "The scan is rules, not a model: same input, same verdict — and it can veto but never promote (D11).",
            format!("the deterministic scan vetoed the submission: {finding}"),
        )
        .with_details(json!({ "finding": finding })),
        AppError::IdempotencyInProgress => ApiError::new(
            StatusCode::CONFLICT,
            "idempotency.in_progress",
            "An idempotency key acts once: a resubmission while the first attempt holds the key waits for its outcome, and is refused rather than acting a second time (D13).",
            "another attempt under this Idempotency-Key has not recorded its outcome yet",
        )
        .retry_after(1),
        AppError::IdempotencyDigestMismatch => ApiError::new(
            StatusCode::CONFLICT,
            "idempotency.digest_mismatch",
            "An idempotency key names exactly one request; a different request under the same key is refused, never silently honored as new work (D13).",
            "this Idempotency-Key was already used for a different request",
        ),
        AppError::Verdict(VerdictError::NotReviewer(role)) => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: verdicts are the reviewer surface.",
            format!("a {} key cannot review", role.as_str()),
        ),
        AppError::Verdict(e @ VerdictError::PersonRequired(kind)) => ApiError::new(
            StatusCode::FORBIDDEN,
            "review.person_required",
            "An instruction is followed, not weighed as data, so only a person's reviewer key approves a steering or guardrail version (ADR-0080 d16).",
            e.to_string(),
        )
        .with_details(json!({ "reason": "instruction_needs_person", "kind": kind.as_str() })),
        AppError::Verdict(e) => ApiError::new(
            StatusCode::FORBIDDEN,
            "review.author_in_lineage",
            "R2: the reviewer is never the author — enforced against the full lineage closure.",
            e.to_string(),
        ),
        AppError::Transition(e) => match ctx {
            ErrorContext::Review => ApiError::serve_invalid("consumed")
                .with_details(json!({ "reason": "consumed", "transition": e.to_string() })),
            ErrorContext::Write => ApiError::new(
                StatusCode::CONFLICT,
                "submission.tip_conflict",
                "Supersession targets the current served tip; any other source state is refused.",
                e.to_string(),
            ),
        },
        AppError::Store(StoreError::SeqConflict { .. } | StoreError::PrevHashConflict { .. }) => {
            // Surfaces only after the write path exhausted its head-retry
            // loop (the oml-pg phase's intended lead write shape).
            ApiError::unavailable("chain head contention")
        }
        AppError::Store(StoreError::TipMoved { .. }) => {
            tip_conflict("the supersede target stopped being the served tip before ratification")
        }
        // The ratifying commit's re-checks refused it and journalled nothing
        // (errors.md `workspace.transition_refused`, `review.ratifier_is_proposer`;
        // workspaces.md, ratification re-checks; ADR-0009 d3, d4).
        AppError::Store(StoreError::TransitionStale {
            transition_id,
            workspace_id,
            from,
            to,
        }) => transition_refused(
            workspace_id,
            from.as_str(),
            to.as_str(),
            "stale",
            format!("transition {transition_id} is no longer live on its node"),
        ),
        AppError::Store(StoreError::ChildrenNotFrozen {
            transition_id,
            workspace_id,
        }) => transition_refused(
            workspace_id,
            "protected",
            "frozen",
            "children_not_frozen",
            format!("transition {transition_id}'s node has a child that is not frozen"),
        ),
        AppError::Store(StoreError::RatifierIsProposer { transition_id }) => ApiError::new(
            StatusCode::FORBIDDEN,
            "review.ratifier_is_proposer",
            "R2 extends to edges and transitions: proposal and ratification are two identities or they are nothing.",
            format!("the key or principal ratifying transition {transition_id} proposed it"),
        ),
        // The ladder refused a proposal before anything was sealed
        // (errors.md `workspace.transition_refused`; ADR-0009 d1, d3, d4).
        AppError::TransitionRefused {
            workspace_id,
            from,
            to,
            reason,
        } => transition_refused(
            workspace_id,
            from.as_str(),
            to.as_str(),
            reason.reason(),
            format!(
                "{} to {} is refused on workspace {workspace_id}: {reason}",
                from.as_str(),
                to.as_str()
            ),
        ),
        AppError::InvestigationRequired { field } => ApiError::new(
            StatusCode::FORBIDDEN,
            "admin.investigation_required",
            "An admin reads claim text under one reason per investigation: open one with POST /v2/admin/investigations {reason} and name its id (ADR-0089 d10).",
            format!("this read names no open investigation of the admin key in {field}"),
        )
        .with_details(json!({ "field": field })),
        AppError::Store(StoreError::InvestigationNotOpen { .. }) => ApiError::new(
            StatusCode::FORBIDDEN,
            "admin.investigation_required",
            "An admin reads claim text under one reason per investigation: open one with POST /v2/admin/investigations {reason} and name its id (ADR-0089 d10).",
            "the investigation was closed while the read was made",
        )
        .with_details(json!({ "field": "investigation" })),
        AppError::FixRequiresPerson { reason } => ApiError::new(
            StatusCode::FORBIDDEN,
            "admin.fix_requires_person",
            "A fix is break-glass: it is taken by an admin key used by a person and linked to that person (ADR-0089 d1, the operator's decision of 2026-11-19).",
            if *reason == *"unlinked" {
                "this admin key is linked to no person: a console admin links it, or the bootstrap claim names its person"
            } else {
                "this admin key is not used by a person"
            },
        )
        .with_details(json!({ "reason": reason })),
        AppError::FixTargetNotPromoted { status } => ApiError::new(
            StatusCode::CONFLICT,
            "admin.fix_target_not_promoted",
            "Only an approved version is demoted: a held one is withdrawn by its author or rejected by a reviewer (ADR-0089 d16).",
            format!("the version is {}", status.as_str()),
        )
        .with_details(json!({ "state": status.as_str() })),
        AppError::FixScanHold { .. } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "admin.fix_held_by_scan",
            "A fix serves at once, so an edit whose body the scan would hold is refused; demote instead, which serves already-reviewed text (ADR-0089 d4).",
            "the edit's body carries a finding that would hold it",
        ),
        AppError::FixSuperseded => ApiError::new(
            StatusCode::CONFLICT,
            "admin.fix_superseded",
            "A fix whose claim has moved on is not reverted: a person fixes the claim forward (ADR-0089 d8).",
            "the claim is no longer served by the version the fix made its tip",
        ),
        AppError::FixNotRevertible => ApiError::new(
            StatusCode::CONFLICT,
            "admin.fix_not_revertible",
            "An erasure is confirmed and never reverted (ADR-0089 d8).",
            "the fix is an erasure",
        ),
        AppError::FixSettled { state } => ApiError::new(
            StatusCode::CONFLICT,
            "admin.fix_settled",
            "A fix is confirmed or reverted once (ADR-0089 d8).",
            format!("the fix is already {state}"),
        )
        .with_details(json!({ "state": state })),
        AppError::Store(StoreError::FixSettled { .. }) => ApiError::new(
            StatusCode::CONFLICT,
            "admin.fix_settled",
            "A fix is confirmed or reverted once (ADR-0089 d8).",
            "the fix was settled while this request was made",
        ),
        AppError::Store(StoreError::FixVersionMissing { .. }) => ApiError::new(
            StatusCode::CONFLICT,
            "admin.fix_superseded",
            "A fix marks the version it serves; that version was not found as the fix named it, so nothing was written (ADR-0089 d7).",
            "the version the fix serves was not found",
        )
        .with_details(json!({ "reason": "served_version_missing" })),
        AppError::AdminField { field, message } => ApiError::validation(format!("{field}: {message}"))
            .with_details(json!({ "field": field })),
        AppError::WorkspaceFrozen { workspace_id, op } => ApiError::new(
            StatusCode::CONFLICT,
            "workspace.frozen",
            "A frozen workspace admits only demote, erasure and crew-metadata update, each journalled.",
            format!("workspace {workspace_id} is frozen and refuses {op:?}"),
        ),
        AppError::RatifierIsProposer => ApiError::new(
            StatusCode::FORBIDDEN,
            "review.ratifier_is_proposer",
            "R2 extends to edges and transitions: proposal and ratification are two identities or they are nothing.",
            "the key or principal ratifying this proposal proposed it",
        ),
        // The ratify use case answers a lost edge compare-and-set as
        // `AppError::EdgeConsumed`, so one reaching a transport is a defect
        // (ADR-0021 d5).
        AppError::Store(StoreError::EdgeNotPending { edge_id }) => {
            ApiError::internal(format!("claim edge {edge_id} is not pending"))
        }
        AppError::Store(StoreError::IdempotencyRecorded) => {
            // Surfaces only when the record a concurrent attempt committed
            // was gone again when the use-case read it back to replay it.
            ApiError::unavailable("idempotency record contention")
        }
        // A connection.opened fold refused under the chain lock: the token
        // exchange judged the code by the reuse rule before it reached
        // here, so what remains revokes nothing and is `invalid_grant`
        // (errors.md `oauth.authorization_code_invalid`; ADR-0031 d10).
        AppError::Store(StoreError::ConnectionNotOpenable { .. }) => oauth_refusal(
            Refusal::AuthorizationCodeInvalid(AuthorizationCodeInvalid::Redeemed),
        ),
        // The mint's code moved under it and it committed nothing;
        // retryable, as a head conflict is (ADR-0031 d8).
        AppError::Store(StoreError::ConnectCodeMoved { .. }) => {
            ApiError::unavailable("connect code contention")
        }
        // A registration found the unused registrations at their cap and
        // wrote nothing; retryable once some lapse (errors.md `limit.rate`;
        // ADR-0031 d5).
        AppError::Store(StoreError::CapReached) => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "limit.rate",
            "At most 10 000 registered clients that have not connected exist at once; a \
             registration past them waits until some lapse 24 hours after registering \
             (ADR-0031 d5).",
            "the unused client registrations are at their cap",
        )
        .retryable(),
        AppError::Store(StoreError::Backend(e)) => ApiError::unavailable(&e),
        // A tenant's partition DDL lost a lock contest, a timeout or a
        // deadlock, and committed nothing; retryable. Any other DDL failure is a defect
        // (errors.md `system.unavailable`; ADR-0034 d11).
        AppError::Store(StoreError::PartitionLockTimeout) => {
            ApiError::unavailable("partition_lock_timeout")
        }
        AppError::Store(StoreError::Partition(e)) => ApiError::internal(e),
        // The application always sends the approval; a promotion without
        // one is a defect, committed nowhere (ADR-0077 d6).
        AppError::Store(e @ StoreError::ApprovalMissing { .. }) => {
            ApiError::internal(e.to_string())
        }
        // The unique index refused a creation's name; nothing was
        // journalled (errors.md `name.taken`; ADR-0026 d5). The application
        // answers a store's refusal as `AppError::NameTaken`.
        AppError::NameTaken { kind, name }
        | AppError::Store(StoreError::NameTaken { kind, name }) => ApiError::new(
            StatusCode::CONFLICT,
            "name.taken",
            "A name is a unique, immutable key in its namespace and is never released, so it \
             means one row for as long as the deployment exists (ADR-0026 d1, d10).",
            format!(
                "the {} name {} is already taken in its namespace",
                kind.as_str(),
                name.as_str()
            ),
        )
        .with_details(json!({ "kind": kind.as_str(), "name": name.as_str() })),
        // The email is unique in the deployment; the refusal names the
        // kind and nothing more (errors.md `name.taken`; ADR-0045 d2).
        AppError::WebEmailTaken | AppError::Store(StoreError::EmailTaken) => ApiError::new(
            StatusCode::CONFLICT,
            "name.taken",
            "An allowlist entry's email is unique in the deployment, so a sign-in finds at most one entry (ADR-0045 d2).",
            "the email is on another organisation's list",
        )
        .with_details(json!({ "kind": "email" })),
        AppError::WebUserTooLong(e) => {
            ApiError::validation(e.to_string()).with_details(json!({ "field": "user" }))
        }
        AppError::WebSignIn(refusal) => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "auth.invalid",
            "A person signs in only with a verified email on an organisation's list, through the account that first signed in with it (ADR-0045 d4).",
            refusal.to_string(),
        )
        .with_details(json!({ "reason": refusal.reason() })),
        AppError::WebKeyMissing { capacity } => ApiError::new(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "Every page act runs as one of the person's session keys (ADR-0045 d6).",
            format!("this session holds no {capacity} key on this organisation"),
        )
        .with_details(json!({ "capacity": capacity })),
        // The last live admin person, refused on every surface with its
        // reason (ADR-0097 d25).
        AppError::WebField {
            field: "admin",
            message,
        } if message == LAST_ADMIN_MESSAGE => ApiError::validation(message)
            .with_details(json!({ "field": "admin", "reason": "last_admin" })),
        AppError::WebField { field, message } => {
            ApiError::validation(message).with_details(json!({ "field": field }))
        }
        // Converted to `WebField` by `From<StoreError>`; never built raw.
        AppError::Store(StoreError::LastAdminEntry) => ApiError::validation(LAST_ADMIN_MESSAGE)
            .with_details(json!({ "field": "admin", "reason": "last_admin" })),
        // A read's feedback member is refused by the field it names, and
        // the read is not run (ADR-0054 d6).
        AppError::Feedback { field, message } => {
            ApiError::validation(message).with_details(json!({ "field": field }))
        }
        AppError::FeedbackRateLimited => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "limit.rate",
            "A key's explicit feedback labels are capped at 1000 a UTC day (ADR-0054 d6).",
            "this key's feedback today is at its cap; send the read without feedback",
        )
        .retryable(),
        AppError::TelemetryOff => ApiError::unavailable("telemetry_off"),
        // An account's quota, refused before the signature or by the
        // commit's ceiling; nothing was journalled (errors.md `limit.quota`;
        // ADR-0068 d5; ADR-0071 d3).
        AppError::QuotaReached(reached) | AppError::Store(StoreError::QuotaReached(reached)) => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "limit.quota",
            "An account holds at most its profile's memories, text bytes, monthly writes, keys, connections and workspaces; an administrator can raise one (ADR-0068 d5; ADR-0071 d5; ADR-0095 d4).",
            format!(
                "the {} quota of {} is reached",
                reached.quota.as_str(),
                reached.limit
            ),
        )
        .with_details(json!({
            "quota": reached.quota.as_str(),
            "limit": reached.limit,
            "used": reached.used,
        })),
        AppError::KeyRateLimited {
            limit,
            retry_after_secs,
        } => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "limit.rate",
            "A key makes at most its requests per minute: its own rate, and its account's (ADR-0068 d5; ADR-0079 d4).",
            format!("this key made {limit} requests this minute, its limit"),
        )
        .with_details(json!({ "quota": "requests_per_minute", "limit": limit }))
        .retry_after(retry_after_secs),
        AppError::SignupRateLimited { retry_after_secs } => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "limit.rate",
            "Sign-ups from one address are limited per hour (ADR-0068 d9).",
            "too many accounts were created from this address in the last hour",
        )
        .retry_after(retry_after_secs),
        AppError::ReplayReranks { name } => ApiError::validation(format!(
            "the replay runs no reranking pipeline, so it never takes a live recall's reranker slot: {name}"
        ))
        .with_details(json!({ "field": "/pipelines" })),
        // Denials attenuate grants, and an admin key carries none
        // (http-api.md keys, grants, denials; ADR-0026 d15).
        AppError::DenialOnAdminKey => ApiError::validation(
            "a global denial on an admin key is refused: an admin key carries no grant for a denial to attenuate",
        )
        .with_details(json!({ "field": "key" })),
        // A corrupt row: no verb issues an admin key under a user root,
        // and no denial cuts one off (ADR-0068 d8, d11; #277).
        AppError::AdminKeyUnderUserRoot { user, key } => ApiError::new(
            StatusCode::CONFLICT,
            "workspace.admin_key_under_root",
            "A person is disabled only when every key under their root can be cut off, and no denial cuts off an admin key (ADR-0068 d8).",
            format!(
                "admin key {key} names the user root of {}, which no verb does; clear its row's user_root_id, then disable again",
                user.as_str()
            ),
        )
        .with_details(json!({ "user": user.as_str(), "key": key })),
        AppError::EmptyCrewUpdate => ApiError::validation(
            "a crew update sends charter_uri or external_manager, at least one",
        ),
        // The lead's startup naming; no request reaches it, and a lead that
        // could not name its rows serves none (ADR-0026 d11).
        AppError::NamingRefused { .. } => ApiError::unavailable("names_assigned"),
        // A stale or foreign pagination cursor (errors.md request.validation).
        AppError::Cursor(e) => ApiError::validation(e.to_string()),
        AppError::StoredResponse(e) => ApiError::internal(e),
        AppError::Chain(e) => ApiError::internal(e.to_string()),
        AppError::ChainVerify(e) => ApiError::internal(e.to_string()),
        AppError::Jcs(e) => ApiError::internal(e.to_string()),
        AppError::OAuth(refusal) => oauth_refusal(refusal),
        // The `/oauth/token` edge answers this in the RFC's shape with the
        // scheme; elsewhere it is the envelope's `oauth.client_invalid`.
        AppError::ClientAuthentication { .. } => oauth_refusal(Refusal::ClientInvalid(
            ClientInvalid::ClientAuthentication,
        )),
    }
}

/// A connection-flow refusal in the envelope: its errors.md status, its
/// code, and the `details.reason` errors.md declares for it, where it
/// declares one. The `/oauth/*` endpoints answer in the RFC's shapes
/// instead; this is the shape on `/v2` and `/mcp` (errors.md "`oauth` —
/// connections"; ADR-0031 d13).
fn oauth_refusal(refusal: Refusal) -> ApiError {
    let (status, rule) = match refusal {
        Refusal::ClientInvalid(_) => (
            StatusCode::BAD_REQUEST,
            "Clients are public and identified by a verified document or a registration (ADR-0031 d5).",
        ),
        Refusal::RedirectRefused(_) => (
            StatusCode::BAD_REQUEST,
            "An authorization code goes only to https://claude.ai/api/mcp/auth_callback, by exact match; a refused redirect is never followed (ADR-0031 d6; RFC 6749 §4.1.2.1).",
        ),
        Refusal::PkceRequired(_) => (
            StatusCode::BAD_REQUEST,
            "Every authorization request is bound by S256 PKCE to the client that started it (ADR-0031 d4).",
        ),
        Refusal::ResourceMismatch => (
            StatusCode::BAD_REQUEST,
            "A token is issued for the one protected resource and accepted nowhere else (ADR-0031 d10).",
        ),
        Refusal::AuthorizationExpired(_) => (
            StatusCode::BAD_REQUEST,
            "A page acts only on a live request it rendered, for a bounded number of tries (ADR-0031 d9).",
        ),
        Refusal::ConnectCodeInvalid => (
            StatusCode::BAD_REQUEST,
            "A connect code is single-use and short-lived, and the page never tells a guesser that a code existed (ADR-0031 d8, d9).",
        ),
        Refusal::AuthorizationCodeInvalid(_) => (
            StatusCode::BAD_REQUEST,
            "An authorization code is redeemed once, by the client that holds the verifier, within its sixty seconds (ADR-0031 d9, d10).",
        ),
        Refusal::RefreshInvalid(_) => (
            StatusCode::BAD_REQUEST,
            "A refresh token is used once; a reused one ends its connection (ADR-0031 d10).",
        ),
        Refusal::OriginRefused => (
            StatusCode::FORBIDDEN,
            "The page acts only on its own form; a cross-site submission is refused before its fields are read (ADR-0031 d9).",
        ),
        Refusal::AttachRefused => (
            StatusCode::FORBIDDEN,
            "An upload's parts travel on /v1 under a key value, which a connector key does not have (ADR-0031 d11).",
        ),
        Refusal::ConnectRefused(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "A key that cannot act on the agent-safe surface, or already has a value, gets no connect code, and a connector key never gains a value by rotation (ADR-0031 d7, d8).",
        ),
        Refusal::RequestValidation(_) => (
            StatusCode::BAD_REQUEST,
            "The contract is openapi.yaml; what it does not admit is refused, not coerced.",
        ),
    };
    let reason = match refusal {
        Refusal::ConnectRefused(reason) => Some(reason.as_str()),
        Refusal::ClientInvalid(reason) => reason.details_reason(),
        _ => None,
    };
    // A refused connection names its reason in the sentence as well as in
    // `details.reason`, since a terminal prints the sentence (ADR-0067 d3).
    let message = match refusal {
        Refusal::ConnectRefused(ConnectRefused::NotConfined) => format!(
            "{refusal} (reason: not_confined; a code that outlives ten minutes is minted only \
             for a key whose grants sit on one workspace other than the universe and that names \
             no user root)"
        ),
        Refusal::ConnectRefused(why) => format!("{refusal} (reason: {})", why.as_str()),
        _ => refusal.to_string(),
    };
    let error = ApiError::new(status, refusal.code(), rule, message);
    match reason {
        Some(reason) => error.with_details(json!({ "reason": reason })),
        None => error,
    }
}

/// A refusal at `POST /oauth/token` or `POST /oauth/register` in the RFC's
/// shape, `{error, error_description}` (RFC 6749 §5.2, RFC 7591 §3.2.2):
/// `error` is the RFC code a client branches on, and `error_description`
/// begins with the OML code, a colon and its rule (errors.md, "`oauth` —
/// connections"; ADR-0031 d13). No envelope and no `request_id` in the
/// body; the response still carries `X-Request-Id`.
#[derive(Debug)]
pub struct OAuthError {
    pub status: StatusCode,
    /// The RFC `error`.
    pub error: &'static str,
    /// The OML code `description` begins with.
    pub code: &'static str,
    pub description: String,
    /// The `WWW-Authenticate` value of a `401`.
    pub challenge: Option<String>,
    pub retryable: bool,
}

impl OAuthError {
    /// A malformed parameter, `request.validation` carried as `error`:
    /// `invalid_request` or the RFC's own name for the fault.
    pub fn validation(error: &'static str, message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error,
            code: "request.validation",
            description: format!("request.validation: {message}"),
            challenge: None,
            retryable: false,
        }
    }

    /// A body in a media type other than the endpoint's, refused before it
    /// is read (`request.unsupported_media` as `invalid_request`).
    pub fn unsupported_media(supported: &str, got: &str) -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            error: "invalid_request",
            code: "request.unsupported_media",
            description: format!(
                "request.unsupported_media: this endpoint takes {supported}, not {got:?}"
            ),
            challenge: None,
            retryable: false,
        }
    }

    /// The RFC body, `application/json`, with `WWW-Authenticate` on a
    /// challenge and `Retry-After` on a retryable refusal.
    #[must_use]
    pub fn into_response(self) -> Response {
        let body = json!({ "error": self.error, "error_description": self.description });
        let mut response = Response::new(axum::body::Body::from(body.to_string()));
        *response.status_mut() = self.status;
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        if let Some(challenge) = self.challenge.and_then(|c| HeaderValue::from_str(&c).ok()) {
            headers.insert(header::WWW_AUTHENTICATE, challenge);
        }
        if self.retryable {
            headers.insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }
}

/// Any other refusal keeps its errors.md status and code, carried as
/// `invalid_request`: `limit.rate` and `system.unavailable` among them,
/// with `Retry-After` where the code is retryable.
impl From<ApiError> for OAuthError {
    fn from(api: ApiError) -> Self {
        Self {
            status: api.status,
            error: "invalid_request",
            code: api.code,
            description: format!("{}: {}", api.code, api.message),
            challenge: None,
            retryable: api.retryable,
        }
    }
}

/// The one place the token and registration endpoints' refusals become RFC
/// bodies (errors.md, "`oauth` — connections"): the status and code of
/// [`map_app_error`], the RFC `error` the table's third column names, and
/// for client authentication sent in an `Authorization` header a `401`
/// whose challenge names the scheme the request used (RFC 6749 §5.2).
#[must_use]
pub fn map_oauth_error(err: AppError) -> OAuthError {
    match err {
        AppError::OAuth(refusal) => rfc_refusal(refusal),
        AppError::ClientAuthentication { scheme } => {
            let mut refused =
                rfc_refusal(Refusal::ClientInvalid(ClientInvalid::ClientAuthentication));
            if let Some(scheme) = scheme {
                refused.status = StatusCode::UNAUTHORIZED;
                refused.challenge = Some(format!(r#"{scheme} realm="oml""#));
            }
            refused
        }
        other => map_app_error(other, ErrorContext::Write).into(),
    }
}

/// A connection-flow refusal as its RFC body: `Display` begins with the
/// code and names the rule, the form `error_description` takes.
fn rfc_refusal(refusal: Refusal) -> OAuthError {
    let error = match refusal {
        Refusal::AuthorizationCodeInvalid(_) | Refusal::RefreshInvalid(_) => "invalid_grant",
        Refusal::ResourceMismatch => "invalid_target",
        Refusal::ClientInvalid(_) => "invalid_client",
        Refusal::RedirectRefused(_) => "invalid_redirect_uri",
        Refusal::RequestValidation(
            Validation::NotJsonObject
            | Validation::ClientNameMissing
            | Validation::RedirectUrisMissing
            | Validation::TokenEndpointAuthMethod
            | Validation::GrantTypes
            | Validation::ResponseTypes,
        ) => "invalid_client_metadata",
        Refusal::RequestValidation(Validation::ScopeUnsupported) => "invalid_scope",
        _ => "invalid_request",
    };
    OAuthError {
        status: oauth_refusal(refusal).status,
        error,
        code: refusal.code(),
        description: refusal.to_string(),
        challenge: None,
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use oml_app::domain::oauth::{ConnectRefused, OAUTH_CODES};

    use super::*;

    #[test]
    fn an_oauth_refusal_answers_its_errors_md_status_code_and_declared_reason() {
        let refused = map_app_error(
            AppError::OAuth(Refusal::ConnectRefused(ConnectRefused::BearerKey)),
            ErrorContext::Write,
        );
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(refused.code, OAUTH_CODES[10]);
        assert_eq!(refused.details, Some(json!({ "reason": "bearer_key" })));

        let attach = map_app_error(AppError::OAuth(Refusal::AttachRefused), ErrorContext::Write);
        assert_eq!(attach.status, StatusCode::FORBIDDEN);
        assert_eq!(attach.code, "oauth.attach_refused");
        assert_eq!(attach.details, None);

        let code = map_app_error(
            AppError::OAuth(Refusal::ConnectCodeInvalid),
            ErrorContext::Write,
        );
        assert_eq!(code.status, StatusCode::BAD_REQUEST);
        assert_eq!(code.code, "oauth.connect_code_invalid");
        assert_eq!(code.details, None, "a code's refusal names no reason");
    }

    #[test]
    fn an_oauth_refusals_error_description_begins_with_its_oml_code() {
        for refusal in [
            Refusal::ClientInvalid(ClientInvalid::Unknown),
            Refusal::ResourceMismatch,
            Refusal::AuthorizationCodeInvalid(AuthorizationCodeInvalid::Reused),
            Refusal::RequestValidation(Validation::ClientNameMissing),
        ] {
            let refused = map_oauth_error(AppError::OAuth(refusal));
            assert_eq!(refused.code, refusal.code());
            assert!(
                refused
                    .description
                    .starts_with(&format!("{}: ", refusal.code())),
                "{}",
                refused.description
            );
        }
        let rate = map_oauth_error(AppError::Store(StoreError::CapReached));
        assert_eq!(
            (rate.status, rate.error, rate.retryable),
            (StatusCode::TOO_MANY_REQUESTS, "invalid_request", true)
        );
        assert!(rate.description.starts_with("limit.rate: "), "{rate:?}");
    }

    /// A field or header v0.4.0 renamed is refused with the code every other
    /// malformed request gets, naming a body field by its JSON pointer and
    /// the spelling to send instead; no code is added for it (ADR-0039 d11).
    #[test]
    fn a_renamed_field_refusal_is_request_validation_naming_the_new_spelling() {
        let refusal = ApiError::renamed("/include_own_held", "/include_unreviewed");
        assert_eq!(refusal.status, StatusCode::BAD_REQUEST);
        assert_eq!(refusal.code, "request.validation");
        assert_eq!(refusal.rule, ApiError::validation("").rule);
        assert!(!refusal.retryable);
        assert_eq!(
            refusal.details,
            Some(json!({ "field": "/include_own_held", "renamed_to": "/include_unreviewed" }))
        );
        assert!(
            refusal.message.contains("include_unreviewed"),
            "{}",
            refusal.message
        );
        assert_eq!(
            refusal.error_object()["details"]["renamed_to"],
            "/include_unreviewed"
        );
    }

    /// A verdict refused by R2 by person answers the lineage code with
    /// `details.reason: same_person`, on the single verdict and as a bulk
    /// item alike (ADR-0082 d7; errors.md).
    #[test]
    fn a_same_person_verdict_is_author_in_lineage_naming_same_person() {
        for ctx in [ErrorContext::Write, ErrorContext::Review] {
            let refusal = map_app_error(AppError::SamePerson, ctx);
            assert_eq!(refusal.status, StatusCode::FORBIDDEN);
            assert_eq!(refusal.code, "review.author_in_lineage");
            assert_eq!(refusal.details, Some(json!({ "reason": "same_person" })));
        }
    }

    /// A global denial on an admin key is refused naming `key`, the field
    /// v0.4.0 sends, and the architecture page that declares the rule names
    /// the same field (roles-and-keys.md footnote 6; ADR-0026 d15, ADR-0039 d7).
    #[test]
    fn a_denial_on_an_admin_key_names_the_key_field_as_roles_and_keys_states() {
        let refusal = map_app_error(AppError::DenialOnAdminKey, ErrorContext::Write);
        assert_eq!(refusal.code, "request.validation");
        assert_eq!(refusal.details, Some(json!({ "field": "key" })));
        let page = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../docs/architecture/roles-and-keys.md"
        ))
        .expect("docs/architecture/roles-and-keys.md reads");
        let flat = page.split_whitespace().collect::<Vec<_>>().join(" ");
        let sentence = "A global denial on an admin key is refused, `request.validation` with `details.field: ";
        let at = flat.find(sentence).expect("footnote 6 states the refusal") + sentence.len();
        assert!(
            flat[at..].starts_with("key`"),
            "roles-and-keys.md names {:?}",
            &flat[at..at + 12]
        );
    }
}

#[cfg(test)]
mod ladder_tests {
    use oml_app::domain::{FrozenOp, LadderRefusal, WorkspaceStatus};
    use uuid::Uuid;

    use super::*;

    fn mapped(e: StoreError) -> ApiError {
        map_app_error(AppError::Store(e), ErrorContext::Write)
    }

    #[test]
    fn a_ratification_refused_by_its_commits_re_check_answers_transition_refused_with_its_reason() {
        let (transition_id, workspace_id) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let stale = mapped(StoreError::TransitionStale {
            transition_id,
            workspace_id,
            from: WorkspaceStatus::Development,
            to: WorkspaceStatus::Protected,
        });
        assert_eq!(
            (stale.status, stale.code, stale.details),
            (
                StatusCode::CONFLICT,
                "workspace.transition_refused",
                Some(json!({
                    "workspace_id": workspace_id.to_string(),
                    "from": "development",
                    "to": "protected",
                    "reason": "stale",
                }))
            )
        );
        let children = mapped(StoreError::ChildrenNotFrozen {
            transition_id,
            workspace_id,
        });
        assert_eq!(
            (children.status, children.code, children.details),
            (
                StatusCode::CONFLICT,
                "workspace.transition_refused",
                Some(json!({
                    "workspace_id": workspace_id.to_string(),
                    "from": "protected",
                    "to": "frozen",
                    "reason": "children_not_frozen",
                }))
            )
        );
    }

    #[test]
    fn a_proposal_the_ladder_refuses_answers_transition_refused_with_its_reason_and_freeze_answers_frozen()
     {
        let workspace_id = Uuid::from_u128(2);
        let refused = map_app_error(
            AppError::TransitionRefused {
                workspace_id,
                from: WorkspaceStatus::Development,
                to: WorkspaceStatus::Frozen,
                reason: LadderRefusal::Skipped,
            },
            ErrorContext::Write,
        );
        assert_eq!(
            (refused.status, refused.code, refused.details),
            (
                StatusCode::CONFLICT,
                "workspace.transition_refused",
                Some(json!({
                    "workspace_id": workspace_id.to_string(),
                    "from": "development",
                    "to": "frozen",
                    "reason": "skipped",
                }))
            )
        );
        let frozen = map_app_error(
            AppError::WorkspaceFrozen {
                workspace_id,
                op: FrozenOp::Write,
            },
            ErrorContext::Write,
        );
        assert_eq!(
            (frozen.status, frozen.code),
            (StatusCode::CONFLICT, "workspace.frozen")
        );
        let ratifier = map_app_error(AppError::RatifierIsProposer, ErrorContext::Review);
        assert_eq!(
            (ratifier.status, ratifier.code),
            (StatusCode::FORBIDDEN, "review.ratifier_is_proposer")
        );
    }

    #[test]
    fn a_ratification_by_the_proposers_key_or_principal_answers_ratifier_is_proposer() {
        let e = mapped(StoreError::RatifierIsProposer {
            transition_id: Uuid::from_u128(1),
        });
        assert_eq!(
            (e.status, e.code),
            (StatusCode::FORBIDDEN, "review.ratifier_is_proposer")
        );
    }
}

#[cfg(test)]
mod claim_edge_tests {
    use super::*;
    use oml_app::domain::claim_edges::{EdgeField, LabelRefusal};
    use oml_app::domain::vocab::OmlLabel;
    use oml_app::domain::vocab_edges::CycleRefused;
    use uuid::Uuid;

    const EDGE_ID: Uuid = Uuid::from_u128(7);

    /// A refusal, and the status, code and details it answers on the wire.
    type Case = (fn() -> AppError, StatusCode, &'static str, Option<Value>);

    /// Each claim-edge refusal reaches the wire as its errors.md code and
    /// status, in either context, with the details the code declares.
    #[test]
    fn the_transport_maps_every_claim_edge_app_error_to_its_errors_md_code() {
        let cases: [Case; 5] = [
            (
                || AppError::ClaimEdgeLabel {
                    field: EdgeField::Label,
                    reason: LabelRefusal::Empty,
                },
                StatusCode::BAD_REQUEST,
                "request.validation",
                Some(json!({ "field": "label" })),
            ),
            (
                || AppError::RatifierIsProposer,
                StatusCode::FORBIDDEN,
                "review.ratifier_is_proposer",
                None,
            ),
            (
                || AppError::AuthorInLineage,
                StatusCode::FORBIDDEN,
                "review.author_in_lineage",
                None,
            ),
            (
                || {
                    AppError::CycleRefused(CycleRefused {
                        label: OmlLabel::IsA,
                    })
                },
                StatusCode::CONFLICT,
                "vocab.cycle_refused",
                Some(json!({ "label": OmlLabel::IsA.as_str() })),
            ),
            (
                || AppError::EdgeConsumed { edge_id: EDGE_ID },
                StatusCode::CONFLICT,
                "review.serve_invalid",
                Some(json!({ "reason": "consumed", "edge_id": EDGE_ID.to_string() })),
            ),
        ];
        for (err, status, code, details) in cases {
            let label = format!("{:?}", err());
            for ctx in [ErrorContext::Review, ErrorContext::Write] {
                let wire = map_app_error(err(), ctx);
                assert_eq!((wire.status, wire.code), (status, code), "{label}");
                assert!(!wire.retryable, "{label}: a refusal is not retryable");
                assert_eq!(wire.details, details, "{label}");
            }
        }
    }
}

/// The ten upload refusals (ADR-0004 d3, as amended 2026-11-17): each its
/// own code, status and rule; nothing is journalled for any of them.
#[must_use]
pub fn upload_error(refusal: &oml_app::domain::upload::UploadRefusal) -> ApiError {
    use oml_app::domain::upload::UploadRefusal as R;
    const RULE: &str = "An upload is the opening key's alone, verified at completion and never journalled (ADR-0004 d3).";
    let status = match refusal {
        R::SessionInvalid { .. }
        | R::Incomplete { .. }
        | R::SessionLimit { .. }
        | R::TenantLimit { .. } => StatusCode::CONFLICT,
        R::DigestMismatch | R::PartSize { .. } | R::PartNumber { .. } => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        R::Expired => StatusCode::GONE,
        R::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        R::StorageFailed => StatusCode::SERVICE_UNAVAILABLE,
    };
    let details = match refusal {
        R::SessionInvalid { state } => Some(json!({ "state": state.as_str() })),
        R::TooLarge { max_bytes } | R::TenantLimit { max_bytes } => {
            Some(json!({ "max_bytes": max_bytes }))
        }
        R::PartSize { expected, got } => Some(json!({ "expected": expected, "got": got })),
        R::PartNumber { parts } => Some(json!({ "parts": parts })),
        R::Incomplete { missing } => Some(json!({ "missing": missing })),
        R::SessionLimit { limit } => Some(json!({ "limit": limit })),
        R::DigestMismatch | R::Expired | R::StorageFailed => None,
    };
    let error = ApiError::new(status, refusal.code(), RULE, refusal.to_string());
    let error = match details {
        Some(details) => error.with_details(details),
        None => error,
    };
    if matches!(refusal, R::StorageFailed) {
        error.retry_after(1)
    } else {
        error
    }
}

/// A bundle or plan refusal under its own code (ADR-0084 d7, d8, R6, R11,
/// R16): `bundle.too_large` is 413, the plan's three `import.*` refusals
/// are 409, and every other bundle code is 422. The message names a file,
/// line or field, never content.
#[must_use]
pub fn bundle_error(refusal: &oml_app::bundle::BundleError) -> ApiError {
    const RULE: &str = "An import verifies the whole bundle before it writes, and each check refuses under its own code (ADR-0084 d7).";
    let code = refusal.code();
    let status = match code {
        "bundle.too_large" => StatusCode::PAYLOAD_TOO_LARGE,
        // The export's code, a request the caller fixes (errors.md).
        "bundle.unencrypted" => StatusCode::BAD_REQUEST,
        "import.bundle_older" | "import.since_mismatch" | "import.source_key_changed" => {
            StatusCode::CONFLICT
        }
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    ApiError::new(status, code, RULE, refusal.to_string())
}

#[cfg(test)]
mod bundle_refusals {
    use super::*;
    use oml_app::bundle::BundleError as B;

    /// Every refusal of ADR-0084 d7, d8 and R11 reaches the import route
    /// under its own code, with the status errors.md gives it.
    #[test]
    fn every_bundle_and_plan_refusal_reaches_the_import_route_under_its_code() {
        let s = || "x".to_owned();
        let cases = [
            (B::FormatUnknown { found: s() }, 422),
            (B::Layout { detail: s() }, 422),
            (
                B::PathUnsafe {
                    path: s(),
                    detail: s(),
                },
                422,
            ),
            (B::TooLarge { detail: s() }, 413),
            (B::LineTooLong { file: s(), line: 1 }, 422),
            (B::Depth { file: s(), line: 1 }, 422),
            (B::Duplicate { detail: s() }, 422),
            (B::UnreferencedBody { path: s() }, 422),
            (
                B::Malformed {
                    file: s(),
                    line: 1,
                    reason: s(),
                },
                422,
            ),
            (B::DigestMismatch { detail: s() }, 422),
            (B::SignatureInvalid { detail: s() }, 422),
            (
                B::SignatureUnexpected {
                    found: s(),
                    expected: s(),
                },
                422,
            ),
            (B::Unencrypted { detail: s() }, 400),
            (B::DecryptFailed { detail: s() }, 422),
            (
                B::BundleOlder {
                    exported_at: s(),
                    ledger_at: s(),
                },
                409,
            ),
            (B::SinceMismatch { detail: s() }, 409),
            (B::SourceKeyChanged { detail: s() }, 409),
            (B::Io { detail: s() }, 422),
        ];
        let mut codes = std::collections::BTreeSet::new();
        for (refusal, status) in cases {
            let code = refusal.code();
            let error = map_app_error(AppError::Bundle(refusal), ErrorContext::Write);
            assert_eq!(error.code, code);
            assert_eq!(error.status.as_u16(), status, "{code}");
            codes.insert(code);
        }
        assert_eq!(codes.len(), 18, "each refusal has its own code");
        let errors_md = include_str!("../../../../docs/api/errors.md");
        for code in &codes {
            assert!(
                errors_md.contains(&format!("| `{code}` |")),
                "{code} has a row on errors.md"
            );
        }
    }
}
