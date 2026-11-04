//! The pure core of the web pages' sign-in (ADR-0045 d2, d3, d4, d5, d8):
//! the allowlist entry's email and user, the sealed sign-in state, the
//! provider answers the server admits, the session token's CSRF companion,
//! and the refusals a sign-in answers. No I/O: the providers' answers enter
//! as values, and time as an input.

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::Sha256Hash;
use crate::name::Name;
use crate::oauth::s256_challenge;

/// The longest email an entry holds, in bytes (RFC 5321 §4.5.3.1.3's path
/// limit less its angle brackets).
pub const EMAIL_MAX_BYTES: usize = 254;

/// The longest user name an entry names, so every composed session key name
/// (`<user>-review`, the longest suffix) fits the 63-character label
/// (ADR-0045 d2).
pub const USER_MAX_CHARS: usize = 56;

/// The suffixes of the four session key names; a user key a person issues
/// may not end in one (ADR-0045 d6, d9).
pub const SESSION_KEY_SUFFIXES: [&str; 4] = ["web", "review", "curate", "admin"];

/// How long a sign-in in progress lives, from `POST /web/sign-in` to the
/// callback (ADR-0045 d3).
pub const SIGN_IN_TTL_SECS: i64 = 600;

/// A session's shipped lifetime, `web.session_ttl`'s default (ADR-0045 d5,
/// d17), and its admitted range.
pub const SESSION_TTL_DEFAULT_SECS: i64 = 8 * 3600;
pub const SESSION_TTL_MIN_SECS: i64 = 3600;
pub const SESSION_TTL_MAX_SECS: i64 = 24 * 3600;

/// The issuers a Google ID token may name (OpenID Connect discovery for
/// `accounts.google.com`).
pub const GOOGLE_ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];

/// Binds the email digest the journal holds to what it digests.
const EMAIL_DIGEST_PURPOSE: &[u8] = b"oml web email\0";
/// Binds a CSRF token to its purpose, so it is no other digest of the
/// session token (ADR-0045 d8).
const CSRF_PURPOSE: &[u8] = b"oml web csrf\0";

/// A sign-in provider (ADR-0045 d3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Provider {
    Google,
    GitHub,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Google, Provider::GitHub];

    /// The path segment and the log's `web_provider`: `google` or `github`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::GitHub => "github",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "google" => Some(Provider::Google),
            "github" => Some(Provider::GitHub),
            _ => None,
        }
    }

    /// The provider's authorization endpoint, which `POST /web/sign-in`
    /// answers `303` to: a constant, never a setting (ADR-0045 d17).
    #[must_use]
    pub fn authorization_endpoint(self) -> &'static str {
        match self {
            Provider::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Provider::GitHub => "https://github.com/login/oauth/authorize",
        }
    }

    /// The origin of [`Provider::authorization_endpoint`], which the pages'
    /// `form-action` names, since it governs the `303` after the form
    /// (ADR-0045 d8).
    #[must_use]
    pub fn origin(self) -> &'static str {
        match self {
            Provider::Google => "https://accounts.google.com",
            Provider::GitHub => "https://github.com",
        }
    }

    /// The scope the authorization request asks (ADR-0045 d3).
    #[must_use]
    pub fn scope(self) -> &'static str {
        match self {
            Provider::Google => "openid email",
            Provider::GitHub => "read:user user:email",
        }
    }

    /// The name a page shows on the sign-in button.
    #[must_use]
    pub fn display(self) -> &'static str {
        match self {
            Provider::Google => "Google",
            Provider::GitHub => "GitHub",
        }
    }

    /// Whether the provider's authorization request carries a `nonce`:
    /// Google's ID token echoes it; GitHub issues no ID token.
    #[must_use]
    pub fn uses_nonce(self) -> bool {
        matches!(self, Provider::Google)
    }
}

/// An email as an allowlist entry holds it: compared whole, ASCII
/// case-insensitively, with no dot or plus-tag folding, so it is held
/// ASCII-lower-cased (ADR-0045 d2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Email(String);

/// Why a value is not an email an entry can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EmailRefusal {
    #[error("an email is at most {EMAIL_MAX_BYTES} bytes")]
    TooLong,
    #[error("an email is one local part, one @ and one domain, with no space or control character")]
    Shape,
}

impl Email {
    /// The value ASCII-lower-cased, after a shape check that refuses what no
    /// provider calls an address: an empty side of the one `@`, whitespace
    /// or a control character.
    pub fn parse(value: &str) -> Result<Self, EmailRefusal> {
        if value.len() > EMAIL_MAX_BYTES {
            return Err(EmailRefusal::TooLong);
        }
        let mut sides = value.split('@');
        let (Some(local), Some(domain), None) = (sides.next(), sides.next(), sides.next()) else {
            return Err(EmailRefusal::Shape);
        };
        if local.is_empty()
            || domain.is_empty()
            || value.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(EmailRefusal::Shape);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The digest the journal holds in place of the address, and the key a
    /// sign-in finds its entry by before any tenant is known: SHA-256 over a
    /// fixed purpose and the held form (ADR-0045 d2).
    #[must_use]
    pub fn digest(&self) -> Sha256Hash {
        let mut hasher = Sha256::new();
        hasher.update(EMAIL_DIGEST_PURPOSE);
        hasher.update(self.0.as_bytes());
        Sha256Hash(hasher.finalize().into())
    }
}

/// Why a name is not an allowlist entry's user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a user name is at most {USER_MAX_CHARS} characters, so every session key name fits")]
pub struct UserTooLong;

/// A user name under the creation grammar that every composed session key
/// name fits (ADR-0045 d2).
pub fn check_user(user: &Name) -> Result<(), UserTooLong> {
    if user.as_str().len() > USER_MAX_CHARS {
        Err(UserTooLong)
    } else {
        Ok(())
    }
}

/// The four session key names a user's entries compose, in the order of
/// [`SESSION_KEY_SUFFIXES`] (ADR-0045 d6).
#[must_use]
pub fn session_key_names(user: &Name) -> Vec<Name> {
    SESSION_KEY_SUFFIXES
        .iter()
        .filter_map(|suffix| Name::parse(&format!("{}-{suffix}", user.as_str())).ok())
        .collect()
}

/// What a sign-in in progress carries in its sealed cookie (ADR-0045 d3):
/// the provider, the `state` the callback must echo, the PKCE verifier, the
/// nonce for Google, and when it started, in seconds since the epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInState {
    pub provider: Provider,
    pub state: String,
    pub verifier: String,
    pub nonce: Option<String>,
    pub started_at: i64,
}

impl SignInState {
    /// A sign-in started at `now` from three 256-bit secrets: `state`, the
    /// verifier and the nonce, each the lower-case hex of its bytes. The
    /// hex verifier is 64 characters of RFC 7636's alphabet.
    #[must_use]
    pub fn start(provider: Provider, secrets: [[u8; 32]; 3], now: i64) -> Self {
        let [state, verifier, nonce] = secrets;
        Self {
            provider,
            state: hex::encode(state),
            verifier: hex::encode(verifier),
            nonce: provider.uses_nonce().then(|| hex::encode(nonce)),
            started_at: now,
        }
    }

    /// The S256 challenge the authorization request sends (RFC 7636 §4.2).
    #[must_use]
    pub fn challenge(&self) -> String {
        s256_challenge(&self.verifier)
    }

    /// The bytes the cookie seals.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        json!({
            "p": self.provider.as_str(),
            "s": self.state,
            "v": self.verifier,
            "n": self.nonce,
            "t": self.started_at,
        })
        .to_string()
        .into_bytes()
    }

    /// The state an opened cookie carries; `None` for bytes no
    /// [`SignInState::encode`] wrote.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        Some(Self {
            provider: Provider::parse(value.get("p")?.as_str()?)?,
            state: value.get("s")?.as_str()?.to_owned(),
            verifier: value.get("v")?.as_str()?.to_owned(),
            nonce: match value.get("n")? {
                Value::Null => None,
                v => Some(v.as_str()?.to_owned()),
            },
            started_at: value.get("t")?.as_i64()?,
        })
    }

    /// Whether the callback for `provider`, carrying `state`, at `now`, is
    /// this sign-in's: the same provider, the same `state` byte for byte,
    /// within [`SIGN_IN_TTL_SECS`] (ADR-0045 d3).
    #[must_use]
    pub fn admits(&self, provider: Provider, state: &str, now: i64) -> bool {
        self.provider == provider
            && constant_time_eq(self.state.as_bytes(), state.as_bytes())
            && now >= self.started_at
            && now - self.started_at < SIGN_IN_TTL_SECS
    }
}

/// Why a sign-in or a session is refused, each `auth.invalid` with its
/// `details.reason` (ADR-0045 d4, d18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignInRefusal {
    #[error("this email address is not on the list of any organisation")]
    NotAllowed,
    #[error("the provider does not call this email address verified")]
    EmailUnverified,
    #[error("this sign-in was not started here, has expired, or was for another provider")]
    StateMismatch,
    #[error(
        "this email address signed in before through another account at this provider; ask an administrator to remove and re-add it"
    )]
    IdentityChanged,
    #[error("the provider refused the sign-in or could not be reached")]
    ProviderRefused,
    #[error("the session has ended; sign in again")]
    SessionExpired,
}

impl SignInRefusal {
    /// `details.reason`, and the `web_sign_in` line's `outcome`.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            SignInRefusal::NotAllowed => "not_allowed",
            SignInRefusal::EmailUnverified => "email_unverified",
            SignInRefusal::StateMismatch => "state_mismatch",
            SignInRefusal::IdentityChanged => "identity_changed",
            SignInRefusal::ProviderRefused => "provider_refused",
            SignInRefusal::SessionExpired => "session_expired",
        }
    }
}

/// Who the provider says signed in: its subject for the account and the
/// verified email the server admits (ADR-0045 d4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderIdentity {
    pub provider: Provider,
    pub subject: String,
    pub email: Email,
}

/// The claims of a Google ID token received from Google's token endpoint
/// over the TLS connection the server opened, which OpenID Connect Core 1.0
/// §3.1.3.7 item 6 lets the client accept without checking its signature:
/// `iss` Google's, `aud` the client id, `exp` after `now`, `nonce` the
/// sealed one, and `email_verified` true (ADR-0045 d4).
pub fn admit_google_id_token(
    id_token: &str,
    client_id: &str,
    nonce: &str,
    now: i64,
) -> Result<ProviderIdentity, SignInRefusal> {
    let refused = SignInRefusal::ProviderRefused;
    let mut parts = id_token.split('.');
    let (Some(_header), Some(payload), Some(_signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(refused);
    };
    let claims: Value = base64url_decode(payload)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or(refused)?;
    let issuer = claims.get("iss").and_then(Value::as_str).ok_or(refused)?;
    if !GOOGLE_ISSUERS.contains(&issuer) {
        return Err(refused);
    }
    let audience_matches = match claims.get("aud") {
        Some(Value::String(aud)) => aud == client_id,
        Some(Value::Array(auds)) => auds.iter().any(|a| a.as_str() == Some(client_id)),
        _ => false,
    };
    if !audience_matches {
        return Err(refused);
    }
    let expires = claims.get("exp").and_then(Value::as_i64).ok_or(refused)?;
    if expires <= now {
        return Err(refused);
    }
    let sent = claims.get("nonce").and_then(Value::as_str).ok_or(refused)?;
    if !constant_time_eq(sent.as_bytes(), nonce.as_bytes()) {
        return Err(SignInRefusal::StateMismatch);
    }
    let verified = match claims.get("email_verified") {
        Some(Value::Bool(v)) => *v,
        Some(Value::String(v)) => v == "true",
        _ => false,
    };
    if !verified {
        return Err(SignInRefusal::EmailUnverified);
    }
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .and_then(|e| Email::parse(e).ok())
        .ok_or(SignInRefusal::EmailUnverified)?;
    let subject = claims.get("sub").and_then(Value::as_str).ok_or(refused)?;
    if subject.is_empty() {
        return Err(refused);
    }
    Ok(ProviderIdentity {
        provider: Provider::Google,
        subject: subject.to_owned(),
        email,
    })
}

/// One entry of GitHub's `GET /user/emails`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubEmail {
    pub email: String,
    pub primary: bool,
    pub verified: bool,
}

/// The GitHub account `id` and the one address it lists with `primary` and
/// `verified` both true; no such address is `email_unverified`, and the
/// profile's public email is never read (ADR-0045 d4).
pub fn admit_github(
    account_id: u64,
    emails: &[GitHubEmail],
) -> Result<ProviderIdentity, SignInRefusal> {
    let primary = emails
        .iter()
        .find(|e| e.primary && e.verified)
        .ok_or(SignInRefusal::EmailUnverified)?;
    let email = Email::parse(&primary.email).map_err(|_| SignInRefusal::EmailUnverified)?;
    Ok(ProviderIdentity {
        provider: Provider::GitHub,
        subject: account_id.to_string(),
        email,
    })
}

/// The CSRF token of every form of a signed-in page: the lower-case hex of
/// SHA-256 over a fixed purpose and the session token, so it needs no
/// column and is worthless without the cookie (ADR-0045 d8).
#[must_use]
pub fn csrf_token(session_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CSRF_PURPOSE);
    hasher.update(session_token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Whether `sent` is the session's CSRF token, compared in constant time.
#[must_use]
pub fn csrf_matches(session_token: &str, sent: &str) -> bool {
    constant_time_eq(csrf_token(session_token).as_bytes(), sent.as_bytes())
}

/// Equality of two byte strings with no early exit on the first difference.
#[must_use]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Unpadded base64url (RFC 4648 §5), as a JWT segment is written; `None`
/// for any other byte or an impossible length.
#[must_use]
pub fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for b in text.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
    }
    if bits >= 6 {
        return None;
    }
    Some(out)
}
