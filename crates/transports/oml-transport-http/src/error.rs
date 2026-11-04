//! The typed error envelope of docs/api/errors.md: every non-2xx response is
//! `{error: {code, message, rule, details?, retryable}, request_id}`, codes
//! from the closed taxonomy, `rule` stable per code.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::{Value, json};

use oml_app::domain::VerdictError;
use oml_app::domain::oauth::{AuthorizationCodeInvalid, ClientInvalid, Refusal, Validation};
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

    /// A field or header v0.4.0 renamed: `request.validation` naming the
    /// spelling sent in `details.field` and the one to send in
    /// `details.renamed_to`, a body field as its JSON pointer
    /// (`/include_own_held`) and a header by its name, the convention of
    /// every other `details.field`; no code is added for it (errors.md
    /// `request.validation`; ADR-0039 d11). The key and workarea routes
    /// call it for ADR-0039 d7's admin renames, and the memory routes and
    /// the events tail for its workarea and recall renames; the MCP
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
        AppError::UserRootInvalid => ApiError::auth_invalid("user_root"),
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
            subtree_node,
        } => ApiError::validation(format!(
            "grant {grant_index}'s deny names {subtree_node}, which lies outside the grant's subtree"
        )),
        AppError::DefaultUncovered { workarea } => ApiError::validation(format!(
            "default_workarea_id {workarea} is covered by no grant of the key"
        )),
        AppError::BudgetWithoutBodies => ApiError::validation(
            "max_tokens is refused with include_text false: a budget counts text that would not be sent",
        ),
        AppError::WorkareaUnresolved => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.workarea_unresolved",
            "The target workarea comes from the token (ADR-0022 d4); a key with neither a default nor a workarea field is refused, never guessed for (ADR-0039 d7).",
            "the key has no default_workarea and the request names none",
        ),
        AppError::AfterSeqBeyondHead {
            after_seq,
            head_seq,
        } => ApiError::validation(format!(
            "after_seq {after_seq} is beyond head_seq {head_seq}: pass back a next_after_seq this tail served"
        )),
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
        AppError::WorkareaNotTarget { workarea } => ApiError::validation(format!(
            "workarea names {workarea}, but a supersession is captured in its target's workarea"
        ))
        .with_details(json!({ "field": "/workarea" })),
        AppError::KindChangeRefused { target, sent } => ApiError::validation(format!(
            "a {} target admits no {} successor: a kind changes only within its class, and a retirement keeps the target's",
            target.as_str(),
            sent.as_str()
        )),
        AppError::TermRetirement => ApiError::validation(
            "a term is never retired: terms deprecate, through a supersession with term_status deprecated",
        ),
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
            "serve_id is omitted and this key holds {} claimed review items: send the serve_id review next answered",
            if several { "several" } else { "no" }
        ))
        .with_details(json!({ "field": "/serve_id" })),
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
            format!("you hold no {capacity} key on this organisation"),
        )
        .with_details(json!({ "capacity": capacity })),
        AppError::WebField { field, message } => {
            ApiError::validation(message).with_details(json!({ "field": field }))
        }
        // Denials attenuate grants, and an admin key carries none
        // (http-api.md keys, grants, denials; ADR-0026 d15).
        AppError::DenialOnAdminKey => ApiError::validation(
            "a global denial on an admin key is refused: an admin key carries no grant for a denial to attenuate",
        )
        .with_details(json!({ "field": "key" })),
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
    let error = ApiError::new(status, refusal.code(), rule, refusal.to_string());
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
