//! S256 PKCE and the authorize request's parameter bounds (ADR-0031 d4;
//! RFC 7636). Only S256 is served: no `plain`, and no request without a
//! `code_challenge`.

use sha2::{Digest, Sha256};

use super::refusal::{PkceRequired, Refusal, Validation};

/// `state` is bounded at 512 bytes (ADR-0031 d4).
pub const STATE_MAX_BYTES: usize = 512;
/// `client_id` is bounded at 2 048 bytes.
pub const CLIENT_ID_MAX_BYTES: usize = 2048;
/// `redirect_uri` is bounded at 2 048 bytes.
pub const REDIRECT_URI_MAX_BYTES: usize = 2048;
/// An S256 challenge is the 43 base64url characters of a SHA-256 digest.
pub const CODE_CHALLENGE_CHARS: usize = 43;

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The authorize parameters that are bounded before anything else is
/// checked or fetched, each as received; `None` where absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizeBounds<'a> {
    pub state: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub redirect_uri: Option<&'a str>,
    pub code_challenge: Option<&'a str>,
}

/// The first check of `GET /oauth/authorize` (ADR-0031 d4): `state` at most
/// 512 bytes, `client_id` and `redirect_uri` at most 2 048 bytes each, and a
/// `code_challenge` that is present exactly `^[A-Za-z0-9_-]{43}$`. A
/// failure is `request.validation`, answered on the page and never
/// redirected. An absent `code_challenge` passes here; [`check_pkce_method`]
/// refuses it once the client and the redirect URI are accepted.
pub fn check_authorize_bounds(bounds: &AuthorizeBounds<'_>) -> Result<(), Refusal> {
    let past = |value: Option<&str>, max: usize| value.is_some_and(|v| v.len() > max);
    let fault = if past(bounds.state, STATE_MAX_BYTES) {
        Validation::StateTooLong
    } else if past(bounds.client_id, CLIENT_ID_MAX_BYTES) {
        Validation::ClientIdTooLong
    } else if past(bounds.redirect_uri, REDIRECT_URI_MAX_BYTES) {
        Validation::RedirectUriTooLong
    } else if bounds.code_challenge.is_some_and(|c| !is_s256_challenge(c)) {
        Validation::CodeChallengeMalformed
    } else {
        return Ok(());
    };
    Err(Refusal::RequestValidation(fault))
}

/// The PKCE check of an authorize request whose client and redirect URI are
/// accepted (ADR-0031 d4): a `code_challenge` must be present and
/// `code_challenge_method` must be exactly `S256`; an absent method is
/// `plain` (RFC 7636 §4.3) and refused like any other. A refusal is
/// `oauth.pkce_required`, redirected.
pub fn check_pkce_method(
    code_challenge: Option<&str>,
    method: Option<&str>,
) -> Result<(), Refusal> {
    let reason = match (code_challenge, method) {
        (None, _) => PkceRequired::ChallengeAbsent,
        (Some(_), Some("S256")) => return Ok(()),
        (Some(_), None) => PkceRequired::MethodAbsent,
        (Some(_), Some(_)) => PkceRequired::MethodNotS256,
    };
    Err(Refusal::PkceRequired(reason))
}

/// `BASE64URL(SHA-256(ASCII(verifier)))` without padding (RFC 7636 §4.2).
#[must_use]
pub fn s256_challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// Whether `verifier` redeems `challenge` under S256 (RFC 7636 §4.6). A
/// verifier outside RFC 7636 §4.1's syntax — 43 to 128 characters of
/// `[A-Za-z0-9-._~]` — never does, and nor does a challenge outside the
/// S256 form. The comparison runs over all 43 bytes with no early exit.
#[must_use]
pub fn verify_s256(verifier: &str, challenge: &str) -> bool {
    is_verifier(verifier)
        && is_s256_challenge(challenge)
        && constant_time_eq(s256_challenge(verifier).as_bytes(), challenge.as_bytes())
}

fn is_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

fn is_s256_challenge(challenge: &str) -> bool {
    challenge.len() == CODE_CHALLENGE_CHARS
        && challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(char::from(BASE64URL[((n >> (18 - 6 * i)) & 0x3f) as usize]));
        }
    }
    out
}

/// Byte equality that reads every byte of equal-length inputs before
/// answering, so the time taken does not depend on where they first
/// differ. Lengths are not secret here: both sides are 43 bytes. Written
/// by hand over `std::hint::black_box`, since the crate takes no
/// constant-time dependency.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let difference = a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(difference) == 0
}
