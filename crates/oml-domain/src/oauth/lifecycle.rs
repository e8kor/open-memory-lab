//! The connection lifecycle and the connect code's checks along it
//! (ADR-0031 d7, d8, d9, d12; `docs/architecture/roles-and-keys.md`,
//! "Connections", the lifecycle diagram). A connection row is created at
//! Connect, `authorized`; the token exchange opens it; it ends `revoked`,
//! once, and never reopens. An `authorized` connection whose authorization
//! code expires unexchanged, or whose code is voided, is removed rather
//! than transitioned: it was never journalled and cannot be named.

use super::refusal::{AuthorizationExpired, Refusal};

/// A key's state as every lookup reads it (ADR-0031 d11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Active,
    Revoked,
    Expired,
}

impl KeyState {
    pub(super) fn ended(self) -> bool {
        self != KeyState::Active
    }
}

/// `oauth_connections.state`, with the revocation's cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Authorized,
    Open,
    Revoked(RevokedCause),
}

impl ConnectionState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectionState::Authorized => "authorized",
            ConnectionState::Open => "open",
            ConnectionState::Revoked(_) => "revoked",
        }
    }
}

/// `oauth_connections.revoked_cause`. `connection.revoked` journals the
/// first three; a replaced connection is named by the newer
/// `connection.opened`'s `replaces_connection_id` (ADR-0031 d12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokedCause {
    Admin,
    RefreshReuse,
    CodeReuse,
    Replaced,
}

impl RevokedCause {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RevokedCause::Admin => "admin",
            RevokedCause::RefreshReuse => "refresh_reuse",
            RevokedCause::CodeReuse => "code_reuse",
            RevokedCause::Replaced => "replaced",
        }
    }
}

/// What can happen to a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionAct {
    /// The token exchange redeems the authorization code
    /// (`connection.opened`).
    Exchange,
    /// `oml keys disconnect` (`connection.revoked`, `admin`).
    AdminRevoke,
    /// A rotated refresh token presented again (`refresh_reuse`).
    RefreshReuse,
    /// A redeemed authorization code presented again by its holder
    /// (`code_reuse`).
    CodeReuse,
    /// A newer connection of the key opens (`replaced`).
    Replace,
}

/// An act that does not apply to the connection's state. Naming an
/// `authorized` connection to revoke answers `access.not_found`, as an
/// unknown id does (ADR-0031 d11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "{act:?} does not apply to a connection in state {from:?} (roles-and-keys.md, connections lifecycle)"
)]
pub struct LifecycleError {
    pub from: ConnectionState,
    pub act: ConnectionAct,
}

/// The connection's transition function: `authorized` → `open` by the
/// exchange, `open` → `revoked` by an admin, a reuse or a replacement, and
/// nothing else. A revoked connection is revoked once and never reopens.
pub fn connection_transition(
    from: ConnectionState,
    act: ConnectionAct,
) -> Result<ConnectionState, LifecycleError> {
    use ConnectionAct as Act;
    use ConnectionState::{Authorized, Open, Revoked};
    match (from, act) {
        (Authorized, Act::Exchange) => Ok(Open),
        (Open, Act::AdminRevoke) => Ok(Revoked(RevokedCause::Admin)),
        (Open, Act::RefreshReuse) => Ok(Revoked(RevokedCause::RefreshReuse)),
        (Open, Act::CodeReuse) => Ok(Revoked(RevokedCause::CodeReuse)),
        (Open, Act::Replace) => Ok(Revoked(RevokedCause::Replaced)),
        _ => Err(LifecycleError { from, act }),
    }
}

/// A connect code's row as the page's code entry reads it, with its key's
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeRow {
    /// Spent by an earlier correct entry.
    pub redeemed: bool,
    /// Voided by a newer code, a revoked connection or `key.revoked`.
    pub voided: bool,
    pub expired: bool,
    pub key: KeyState,
}

/// Code entry (ADR-0031 d8, d9): a found code that is unspent, unvoided,
/// unexpired and of a key neither revoked nor expired is spent; every other
/// entry — an unknown code included — gets one answer,
/// `oauth.connect_code_invalid`, so the page never shows a dead key's
/// grants nor tells a guesser that a code existed.
pub fn check_code_entry(found: Option<&CodeRow>) -> Result<(), Refusal> {
    match found {
        Some(row) if !row.redeemed && !row.voided && !row.expired && !row.key.ended() => Ok(()),
        _ => Err(Refusal::ConnectCodeInvalid),
    }
}

/// Wrong entries an authorization request admits: they bound one page's
/// typing errors, not guessing (ADR-0031 d8, d9).
pub const ATTEMPTS_PER_PAGE: u8 = 5;

/// A wrong entry with `attempts_left` before it: the form again with one
/// attempt fewer, or, for the fifth wrong entry, the end of the request,
/// `oauth.authorization_expired`, redirected as `access_denied`.
pub fn after_wrong_entry(attempts_left: u8) -> Result<u8, Refusal> {
    match attempts_left {
        2.. => Ok(attempts_left - 1),
        _ => Err(Refusal::AuthorizationExpired(
            AuthorizationExpired::AttemptsSpent,
        )),
    }
}

/// The confirmation (ADR-0031 d9): its row's code must not have been
/// voided by a newer code or a revoked connection of its key since it
/// verified, else `oauth.authorization_expired`.
pub fn check_confirmation(code_voided: bool) -> Result<(), Refusal> {
    if code_voided {
        Err(Refusal::AuthorizationExpired(
            AuthorizationExpired::CodeVoided,
        ))
    } else {
        Ok(())
    }
}
