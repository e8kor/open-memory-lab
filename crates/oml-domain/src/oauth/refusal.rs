//! The refusals of the connection flow: one [`Refusal`] variant per code of
//! the `oauth` family, plus `request.validation` for the malformed
//! parameters the OAuth endpoints carry as `invalid_request` or the RFC's own
//! name (ADR-0031 d13; `docs/api/errors.md`, "`oauth` — connections"). The
//! core refuses in types; the HTTP status, the RFC `error` and the shape —
//! an RFC body, a redirect or a page — are attached once, at the edge
//! (errors.md, "Implementation mapping").
//!
//! The reason a variant carries is the core's own record of which check
//! failed. Only [`ClientInvalid::details_reason`] and [`ConnectRefused::as_str`]
//! name wire values, the `details.reason` strings errors.md declares; every
//! other reason is for tests and the edge's choice of RFC `error`, and a
//! connect code's refusal carries none at all, so that nothing downstream
//! can tell a guesser that a code existed (ADR-0031 d8).

/// The eleven codes of the `oauth` family, in the order ADR-0031 d13 lists
/// them.
pub const OAUTH_CODES: [&str; 11] = [
    "oauth.client_invalid",
    "oauth.redirect_refused",
    "oauth.pkce_required",
    "oauth.resource_mismatch",
    "oauth.authorization_expired",
    "oauth.connect_code_invalid",
    "oauth.authorization_code_invalid",
    "oauth.refresh_invalid",
    "oauth.origin_refused",
    "oauth.attach_refused",
    "oauth.connect_refused",
];

/// A refusal of the connection flow. Each variant is exactly one code; its
/// `Display` begins with the code and names the rule, the form
/// `error_description` takes (ADR-0031 d13). No variant carries a secret or
/// a digest of one (ADR-0031 d10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error(
        "oauth.client_invalid: clients are public and identified by a verified document or a registration (ADR-0031 d5)"
    )]
    ClientInvalid(ClientInvalid),
    #[error(
        "oauth.redirect_refused: an authorization code goes only to https://claude.ai/api/mcp/auth_callback or a loopback address, by exact match (ADR-0031 d6)"
    )]
    RedirectRefused(RedirectRefused),
    #[error(
        "oauth.pkce_required: every authorization is bound by S256 PKCE to the client that started it (ADR-0031 d4)"
    )]
    PkceRequired(PkceRequired),
    #[error(
        "oauth.resource_mismatch: a token is issued for the one protected resource and accepted nowhere else (ADR-0031 d10)"
    )]
    ResourceMismatch,
    #[error(
        "oauth.authorization_expired: a page acts only on a live request it rendered, for a bounded number of tries (ADR-0031 d9)"
    )]
    AuthorizationExpired(AuthorizationExpired),
    #[error(
        "oauth.connect_code_invalid: a connect code is single-use and short-lived, and the page never tells a guesser that a code existed (ADR-0031 d8, d9)"
    )]
    ConnectCodeInvalid,
    #[error(
        "oauth.authorization_code_invalid: an authorization code is redeemed once, by the client that holds the verifier, within its sixty seconds (ADR-0031 d9, d10)"
    )]
    AuthorizationCodeInvalid(AuthorizationCodeInvalid),
    #[error(
        "oauth.refresh_invalid: a refresh token is used once; a reused one ends its connection (ADR-0031 d10)"
    )]
    RefreshInvalid(RefreshInvalid),
    #[error(
        "oauth.origin_refused: the page acts only on its own form; a cross-site submission is refused before its fields are read (ADR-0031 d9)"
    )]
    OriginRefused,
    #[error(
        "oauth.attach_refused: an upload's parts travel on /v1 under a key value, which a connector key does not have (ADR-0031 d11)"
    )]
    AttachRefused,
    #[error(
        "oauth.connect_refused: a connection acts as one agent key on the agent-safe surface (ADR-0031 d7, d8)"
    )]
    ConnectRefused(ConnectRefused),
    #[error(
        "request.validation: the contract is openapi.yaml; what it does not admit is refused, not coerced"
    )]
    RequestValidation(Validation),
}

impl Refusal {
    /// The code this refusal is, one-to-one: the eleven of [`OAUTH_CODES`]
    /// and `request.validation`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::ClientInvalid(_) => OAUTH_CODES[0],
            Refusal::RedirectRefused(_) => OAUTH_CODES[1],
            Refusal::PkceRequired(_) => OAUTH_CODES[2],
            Refusal::ResourceMismatch => OAUTH_CODES[3],
            Refusal::AuthorizationExpired(_) => OAUTH_CODES[4],
            Refusal::ConnectCodeInvalid => OAUTH_CODES[5],
            Refusal::AuthorizationCodeInvalid(_) => OAUTH_CODES[6],
            Refusal::RefreshInvalid(_) => OAUTH_CODES[7],
            Refusal::OriginRefused => OAUTH_CODES[8],
            Refusal::AttachRefused => OAUTH_CODES[9],
            Refusal::ConnectRefused(_) => OAUTH_CODES[10],
            Refusal::RequestValidation(_) => "request.validation",
        }
    }
}

/// Why a client was not identified (errors.md, `oauth.client_invalid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientInvalid {
    /// A `client_id` no registration minted.
    Unknown,
    /// A document URL outside the admitted form (ADR-0031 d5).
    ClientIdForm,
    /// The document could not be fetched within the fetch bounds: an answer
    /// outside the address allow-list, or a body past 16 KiB, among others.
    Fetch,
    /// A fetch cap is full and no cached copy can serve.
    FetchBusy,
    /// The document is not a JSON object.
    NotJson,
    /// The document's `client_id` is absent or is not exactly its URL.
    ClientIdMismatch,
    /// The document lacks `client_name` or `redirect_uris`.
    MissingMember,
    /// The document asks for client authentication other than `none`.
    ClientAuthentication,
}

impl ClientInvalid {
    /// The `details.reason` errors.md declares for this reason, where it
    /// declares one.
    #[must_use]
    pub fn details_reason(self) -> Option<&'static str> {
        match self {
            ClientInvalid::ClientIdForm => Some("client_id_form"),
            ClientInvalid::FetchBusy => Some("fetch_busy"),
            _ => None,
        }
    }
}

/// Why a redirect URI was refused (errors.md, `oauth.redirect_refused`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectRefused {
    /// Outside the fixed allowlist (ADR-0031 d6).
    OutsideAllowlist,
    /// Inside the allowlist, but none of the client's registered URIs.
    NotRegistered,
    /// Omitted by a client that does not have exactly one registered URI.
    Omitted,
}

/// Why an authorize request is not bound by S256 PKCE (errors.md,
/// `oauth.pkce_required`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkceRequired {
    ChallengeAbsent,
    /// Absent, which RFC 7636 §4.3 reads as `plain`.
    MethodAbsent,
    MethodNotS256,
}

/// Why an authorization request ended (errors.md,
/// `oauth.authorization_expired`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationExpired {
    /// The fifth wrong entry (ADR-0031 d9).
    AttemptsSpent,
    /// The confirmation's code was voided by a newer code or a revoked
    /// connection of its key (ADR-0031 d8).
    CodeVoided,
}

/// Why an authorization code was not redeemed (errors.md,
/// `oauth.authorization_code_invalid`). Every reason is `invalid_grant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationCodeInvalid {
    Unknown,
    Expired,
    /// Redeemed already, presented without the connection's client and
    /// verifier: revokes nothing.
    Redeemed,
    /// Redeemed already, presented by the connection's client with the
    /// matching verifier: the connection is revoked (`code_reuse`).
    Reused,
    ClientMismatch,
    VerifierMismatch,
    /// The `redirect_uri` does not follow the token endpoint's rule.
    RedirectMismatch,
    CodeVoided,
    /// The key is revoked or expired.
    KeyEnded,
    /// The connection's resource is not the current `<issuer>/mcp`.
    ResourceNotCurrent,
}

/// Why a refresh did not rotate (errors.md, `oauth.refresh_invalid`). Every
/// reason is `invalid_grant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshInvalid {
    /// Not in the refresh-token format, an access token or a key included:
    /// refused before any lookup.
    NotARefreshToken,
    Unknown,
    Expired,
    /// Rotated already: the connection is revoked (`refresh_reuse`).
    Reused,
    ConnectionNotOpen,
    ClientMismatch,
    /// The key is revoked or expired.
    KeyEnded,
    /// The connection's resource is not the current `<issuer>/mcp`.
    ResourceNotCurrent,
}

/// Why a key cannot hold a connection (errors.md, `oauth.connect_refused`);
/// each is a declared `details.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectRefused {
    Role,
    HeldByService,
    BearerKey,
    Revoked,
    Expired,
    NotConfigured,
    ConnectorKey,
}

impl ConnectRefused {
    /// The declared `details.reason`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectRefused::Role => "role",
            ConnectRefused::HeldByService => "held_by_service",
            ConnectRefused::BearerKey => "bearer_key",
            ConnectRefused::Revoked => "revoked",
            ConnectRefused::Expired => "expired",
            ConnectRefused::NotConfigured => "not_configured",
            ConnectRefused::ConnectorKey => "connector_key",
        }
    }
}

/// Which parameter of an OAuth endpoint is malformed (`request.validation`).
/// The edge carries it as `invalid_request`, or as `invalid_client_metadata`
/// for a registration's members (errors.md, "`oauth` — connections").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validation {
    /// `state` past 512 bytes (ADR-0031 d4).
    StateTooLong,
    /// `client_id` past 2 048 bytes.
    ClientIdTooLong,
    /// `redirect_uri` past 2 048 bytes.
    RedirectUriTooLong,
    /// A present `code_challenge` not matching `^[A-Za-z0-9_-]{43}$`.
    CodeChallengeMalformed,
    /// A registration body that is not a JSON object.
    NotJsonObject,
    /// A registration without `client_name`, which the page shows.
    ClientNameMissing,
    /// A registration without a non-empty `redirect_uris` array of strings.
    RedirectUrisMissing,
    /// A registration whose `token_endpoint_auth_method` is present and not
    /// `none`.
    TokenEndpointAuthMethod,
    /// A registration's `grant_types` outside `authorization_code` and
    /// `refresh_token`.
    GrantTypes,
    /// A registration's `response_types` other than `["code"]`.
    ResponseTypes,
}
