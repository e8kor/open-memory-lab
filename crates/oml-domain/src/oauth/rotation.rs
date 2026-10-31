//! The token endpoint's two decisions, over rows the store has already
//! read: redeeming an authorization code, and rotating a refresh token,
//! each with its reuse detection (ADR-0031 d10, d11;
//! `docs/data/postgres-schema.md`, "connections (ADR-0031)"). The checks
//! run in the order the schema declares, and a failed check consumes,
//! rotates and revokes nothing. Each decision names the conditional update
//! the store then makes; the store's update, not this function, is what
//! keeps two racing requests from both passing.

use super::discovery::Issuer;
use super::lifecycle::{ConnectionAct, ConnectionState, KeyState};
use super::pkce::verify_s256;
use super::refusal::{AuthorizationCodeInvalid, RefreshInvalid, Refusal};

/// The connection row an authorization code's digest found, with its key's
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeRow<'a> {
    pub state: ConnectionState,
    pub client_id: &'a str,
    pub code_challenge: &'a str,
    /// The authorize request's, or the client's one registered URI when the
    /// request omitted it.
    pub redirect_uri: &'a str,
    pub redirect_uri_sent: bool,
    pub resource: &'a str,
    pub key: KeyState,
    pub code_redeemed: bool,
    pub code_expired: bool,
    /// Voided by a newer code or a revoked connection of its key.
    pub code_voided: bool,
}

/// What a `grant_type=authorization_code` request presents beside the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeRequest<'a> {
    pub client_id: &'a str,
    pub code_verifier: &'a str,
    pub redirect_uri: Option<&'a str>,
}

/// The exchange's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeDecision {
    /// Mark the code redeemed where still unredeemed and unexpired, open
    /// the connection and mint its first pair.
    Redeem,
    /// `invalid_grant`; nothing is consumed.
    Refuse(AuthorizationCodeInvalid),
    /// A redeemed code presented by its holder: revoke the connection
    /// (`code_reuse`) and answer `invalid_grant`.
    RevokeForReuse,
}

impl ExchangeDecision {
    /// The refusal answered, if any.
    #[must_use]
    pub fn refusal(self) -> Option<Refusal> {
        match self {
            ExchangeDecision::Redeem => None,
            ExchangeDecision::Refuse(reason) => Some(Refusal::AuthorizationCodeInvalid(reason)),
            ExchangeDecision::RevokeForReuse => Some(Refusal::AuthorizationCodeInvalid(
                AuthorizationCodeInvalid::Reused,
            )),
        }
    }

    /// The connection's transition, if any.
    #[must_use]
    pub fn act(self) -> Option<ConnectionAct> {
        match self {
            ExchangeDecision::Redeem => Some(ConnectionAct::Exchange),
            ExchangeDecision::Refuse(_) => None,
            ExchangeDecision::RevokeForReuse => Some(ConnectionAct::CodeReuse),
        }
    }
}

/// `POST /oauth/token` with `grant_type=authorization_code` (ADR-0031 d10).
/// A code whose row is already redeemed is reuse only when the request's
/// `client_id` is the connection's and its verifier matches the stored
/// challenge, whatever the code's expiry, since only the verifier's holder
/// can have redeemed first; any other presentation of it revokes nothing.
/// Otherwise the client, the verifier, the `redirect_uri` rule, the code's
/// voiding, the key, the resource and the code's expiry are checked in that
/// order. The `redirect_uri` is required exactly when the authorize request
/// carried it and must then equal it byte for byte; when that request
/// omitted it, it may be omitted or must equal the client's one registered
/// URI.
#[must_use]
pub fn decide_exchange(
    row: Option<&ExchangeRow<'_>>,
    request: &ExchangeRequest<'_>,
    issuer: &Issuer,
) -> ExchangeDecision {
    use AuthorizationCodeInvalid as Why;
    let Some(row) = row else {
        return ExchangeDecision::Refuse(Why::Unknown);
    };
    let client = request.client_id == row.client_id;
    let verifier = verify_s256(request.code_verifier, row.code_challenge);
    if row.code_redeemed {
        return if client && verifier && row.state == ConnectionState::Open {
            ExchangeDecision::RevokeForReuse
        } else {
            ExchangeDecision::Refuse(Why::Redeemed)
        };
    }
    let redirect = match request.redirect_uri {
        Some(uri) => uri == row.redirect_uri,
        None => !row.redirect_uri_sent,
    };
    let why = if !client {
        Why::ClientMismatch
    } else if !verifier {
        Why::VerifierMismatch
    } else if !redirect {
        Why::RedirectMismatch
    } else if row.code_voided {
        Why::CodeVoided
    } else if row.key.ended() {
        Why::KeyEnded
    } else if row.resource != issuer.resource() {
        Why::ResourceNotCurrent
    } else if row.state != ConnectionState::Authorized {
        // The store's update matches only an authorized row.
        Why::Unknown
    } else if row.code_expired {
        Why::Expired
    } else {
        return ExchangeDecision::Redeem;
    };
    ExchangeDecision::Refuse(why)
}

/// The `oauth_tokens` row of kind `refresh` a presented refresh token's
/// digest found ([`super::refresh_token_digest`]), with its connection and
/// its key's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshRow<'a> {
    pub rotated: bool,
    pub expired: bool,
    pub connection: ConnectionState,
    pub client_id: &'a str,
    pub resource: &'a str,
    pub key: KeyState,
}

/// A refresh's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshDecision {
    /// Mark the presented token rotated where still unrotated and
    /// unexpired, and mint a new pair.
    Rotate,
    /// `invalid_grant`; nothing rotates and nothing is revoked.
    Refuse(RefreshInvalid),
    /// A rotated token presented again: revoke the connection and every
    /// token it holds (`refresh_reuse`) and answer `invalid_grant`.
    RevokeForReuse,
}

impl RefreshDecision {
    /// The refusal answered, if any.
    #[must_use]
    pub fn refusal(self) -> Option<Refusal> {
        match self {
            RefreshDecision::Rotate => None,
            RefreshDecision::Refuse(reason) => Some(Refusal::RefreshInvalid(reason)),
            RefreshDecision::RevokeForReuse => {
                Some(Refusal::RefreshInvalid(RefreshInvalid::Reused))
            }
        }
    }

    /// The connection's transition, if any; a rotation is none.
    #[must_use]
    pub fn act(self) -> Option<ConnectionAct> {
        match self {
            RefreshDecision::RevokeForReuse => Some(ConnectionAct::RefreshReuse),
            RefreshDecision::Rotate | RefreshDecision::Refuse(_) => None,
        }
    }
}

/// `POST /oauth/token` with `grant_type=refresh_token` (ADR-0031 d10,
/// d11). Under the connection's row lock: the connection must be open, the
/// request's `client_id` the connection's, the key neither revoked nor
/// expired and the resource the current `<issuer>/mcp`, else the refusal
/// rotates and revokes nothing. Then a token already rotated is reuse —
/// rotated wins over expired, because a rotated row stays until its own
/// expiry or the revocation so that a reuse inside that window is always
/// recognized — an expired one is refused, and an unrotated, unexpired one
/// rotates. There is no grace window.
#[must_use]
pub fn decide_refresh(
    row: Option<&RefreshRow<'_>>,
    client_id: &str,
    issuer: &Issuer,
) -> RefreshDecision {
    use RefreshInvalid as Why;
    let Some(row) = row else {
        return RefreshDecision::Refuse(Why::Unknown);
    };
    let why = if row.connection != ConnectionState::Open {
        Why::ConnectionNotOpen
    } else if client_id != row.client_id {
        Why::ClientMismatch
    } else if row.key.ended() {
        Why::KeyEnded
    } else if row.resource != issuer.resource() {
        Why::ResourceNotCurrent
    } else if row.rotated {
        return RefreshDecision::RevokeForReuse;
    } else if row.expired {
        Why::Expired
    } else {
        return RefreshDecision::Rotate;
    };
    RefreshDecision::Refuse(why)
}
