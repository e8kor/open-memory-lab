//! The typed error envelope of docs/api/errors.md: every non-2xx response is
//! `{error: {code, message, rule, details?, retryable}, request_id}`, codes
//! from the closed taxonomy, `rule` stable per code.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::{Value, json};

use oml_app::domain::VerdictError;
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
        }
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
        if self.retryable {
            // Retryable refusals carry Retry-After (errors.md envelope table).
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
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

/// Which endpoint family the refusal surfaced on — a refused status
/// transition means different things to a verdict (the serve no longer names
/// a held row) and to a write (the target is not the current tip).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorContext {
    Review,
    Write,
}

/// The one place `oml-app`'s typed refusals become errors.md codes
/// (errors.md "Implementation mapping": the attachment happens once, at the
/// transport edge).
#[must_use]
pub fn map_app_error(err: AppError, ctx: ErrorContext) -> ApiError {
    match err {
        AppError::Unauthorized => ApiError::auth_invalid("unknown"),
        AppError::VerbCeiling { role, verb } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: the surface an agent can call never promotes, signs, demotes or deletes.",
            format!("{verb} is outside the {} verb ceiling", role.as_str()),
        )
        .with_details(json!({ "verb": verb, "role": role.as_str() })),
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
        AppError::RoleExclusive => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.role_exclusive",
            "Admin is never a reviewer (D9): a key never combines admin with dreamer verbs.",
            "a key holding the admin role never carries dreamer-class grants",
        ),
        AppError::GrantRefused {
            workarea_id,
            needed_role,
        } => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.grant_refused",
            "Effective access is the union of grants, each attenuated by its denies, deny-wins for global denials; what a grant does not confer, the key does not have (D7).",
            format!(
                "the key reads this workarea but holds no effective {} role on it",
                needed_role.as_str()
            ),
        )
        .with_details(json!({
            "workarea_id": workarea_id.to_string(),
            "needed_role": needed_role.as_str(),
        })),
        AppError::DenyOutsideGrant {
            grant_index,
            subtree_node_id,
        } => ApiError::validation(format!(
            "grant {grant_index}'s deny names {subtree_node_id}, which lies outside the grant's subtree"
        )),
        AppError::DefaultUncovered { workarea_id } => ApiError::validation(format!(
            "default_workarea_id {workarea_id} is covered by no grant of the key"
        )),
        AppError::BudgetWithoutBodies => ApiError::validation(
            "max_tokens is refused with include_bodies false: a budget counts bodies that would not be sent",
        ),
        AppError::WorkareaUnresolved => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.workarea_unresolved",
            "The target workarea comes from the token (ADR-0022 d4); a key with neither a default nor the X-OML-Workarea header is refused, never guessed for.",
            "the key has no default_workarea and the request names none",
        ),
        AppError::NotFound => ApiError::not_found(),
        AppError::LineageUnknown(id) => ApiError::not_found().with_details(json!({
            // derives_from targets fail closed as absence, like CURIE
            // citations (errors.md vocab.unknown_curie rationale).
            "derives_from": id.to_string(),
        })),
        AppError::AuthorOnly => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.author_only",
            "A writer proposes supersessions of its own claims only (D9).",
            "a writer key may supersede only its own claims",
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
        AppError::WorkareaNotTarget { workarea_id } => ApiError::validation(format!(
            "X-OML-Workarea names {workarea_id}, but a supersession is captured in its target's workarea"
        )),
        AppError::KindChangeRefused { target, sent } => ApiError::validation(format!(
            "a {} target admits no {} successor: a kind changes only within its class, and a retirement keeps the target's",
            target.as_str(),
            sent.as_str()
        )),
        AppError::TermRetirement => ApiError::validation(
            "a term is never retired: terms deprecate, through a supersession with term_status deprecated",
        ),
        AppError::ClaimKeyConflict => ApiError::new(
            StatusCode::CONFLICT,
            "submission.claim_key_conflict",
            "A claim_key names one live claim per (tenant, workarea); a claim whose every version is rejected or erased releases it (D7).",
            "a held, promoted or demoted version already holds this claim_key in the workarea",
        ),
        AppError::ScanVeto(finding) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "submission.scan_veto",
            "The scan is rules, not a model: same input, same verdict — and it can veto but never promote (D11).",
            format!("the deterministic scan vetoed the submission: {finding}"),
        )
        .with_details(json!({ "finding": finding })),
        AppError::IdempotencyDigestMismatch => ApiError::new(
            StatusCode::CONFLICT,
            "idempotency.digest_mismatch",
            "An idempotency key names exactly one request; a different request under the same key is refused, never silently honored as new work (D13).",
            "this Idempotency-Key was already used for a different request",
        ),
        AppError::Verdict(VerdictError::NotDreamer(role)) => ApiError::new(
            StatusCode::FORBIDDEN,
            "access.verb_ceiling",
            "R1: verdicts are the dreamer surface.",
            format!("a {} key cannot review", role.as_str()),
        ),
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
        AppError::Store(StoreError::IdempotencyRecorded) => {
            // Surfaces only when the record a concurrent attempt committed
            // was gone again when the use-case read it back to replay it.
            ApiError::unavailable("idempotency record contention")
        }
        AppError::Store(StoreError::Backend(e)) => ApiError::unavailable(&e),
        // A stale or foreign pagination cursor (errors.md request.validation).
        AppError::Cursor(e) => ApiError::validation(e.to_string()),
        AppError::StoredResponse(e) => ApiError::internal(e),
        AppError::Chain(e) => ApiError::internal(e.to_string()),
        AppError::ChainVerify(e) => ApiError::internal(e.to_string()),
        AppError::Jcs(e) => ApiError::internal(e.to_string()),
    }
}
