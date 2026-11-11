//! What a web session's token derives (ADR-0070 d2, d3; ADR-0045 d8): the
//! capacity credential `oml-web` presents to the API as one of the person's
//! session keys, and the CSRF token every signed-in form carries. The auth
//! context computes the first at session open and the lead checks the
//! second; `oml-web` computes both from the cookie. One implementation, so the two processes
//! cannot derive different values, in a leaf `oml-web` may link without
//! reaching the application core (ADR-0070 d1).

#![forbid(unsafe_code)]

use sha2::{Digest as _, Sha256};

/// Binds a CSRF token to its purpose, so it is no other digest of the
/// session token (ADR-0045 d8).
const CSRF_PURPOSE: &[u8] = b"oml web csrf\0";

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

/// Binds an act's idempotency key to its purpose, so it is no other digest
/// of the session token.
const ACT_PURPOSE: &[u8] = b"oml web act\0";

/// The `Idempotency-Key` of the API call a page's form makes: the
/// lower-case hex of SHA-256 over a fixed purpose, the session token and
/// the form's random nonce, so the same form sent again replays and no
/// other session's form names the same key (ADR-0070 d3 layer 4).
#[must_use]
pub fn act_idempotency_key(session_token: &str, nonce: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ACT_PURPOSE);
    hasher.update(session_token.as_bytes());
    hasher.update(b"\0");
    hasher.update(nonce.as_bytes());
    hex::encode(hasher.finalize())
}

/// The prefix of a capacity credential, the per-session value `oml-web`
/// presents to the API as one of the person's session keys (ADR-0070 d2).
pub const CAPACITY_CREDENTIAL_PREFIX: &str = "omls_";

/// The capacity credential of one capacity (`web`, `review`, `curate` or
/// `admin`): `omls_` and the base62 of HMAC-SHA256 keyed by the session
/// token over `oml/web-capacity/v1/<capacity>` (ADR-0070 d2). It lives as
/// long as the session, and only its SHA-256 is stored.
#[must_use]
pub fn capacity_credential(session_token: &str, capacity: &str) -> String {
    let mac = hmac_sha256(
        session_token.as_bytes(),
        format!("oml/web-capacity/v1/{capacity}").as_bytes(),
    );
    format!("{CAPACITY_CREDENTIAL_PREFIX}{}", base62(&mac))
}

/// HMAC-SHA256 (RFC 2104) over `message` under `key`.
#[must_use]
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0_u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

/// `bytes` as a big-endian number written in base62 (`0-9A-Za-z`), with
/// no padding.
#[must_use]
pub fn base62(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut number: Vec<u8> = bytes.iter().copied().skip_while(|b| *b == 0).collect();
    let mut out = Vec::new();
    while !number.is_empty() {
        let mut remainder = 0_u32;
        let mut quotient = Vec::with_capacity(number.len());
        for b in &number {
            let acc = (remainder << 8) | u32::from(*b);
            let q = u8::try_from(acc / 62).expect("a byte's quotient fits a byte");
            remainder = acc % 62;
            if !(quotient.is_empty() && q == 0) {
                quotient.push(q);
            }
        }
        out.push(DIGITS[remainder as usize]);
        number = quotient;
    }
    if out.is_empty() {
        out.push(b'0');
    }
    out.reverse();
    String::from_utf8(out).expect("base62 digits are ASCII")
}

/// Equality of two byte strings with no early exit on the first difference.
#[must_use]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_acts_key_is_the_same_for_the_same_form_and_differs_by_nonce_and_session() {
        let key = act_idempotency_key("token-a", "nonce-1");
        assert_eq!(key, act_idempotency_key("token-a", "nonce-1"));
        assert_ne!(key, act_idempotency_key("token-a", "nonce-2"));
        assert_ne!(key, act_idempotency_key("token-b", "nonce-1"));
        assert_ne!(key, csrf_token("token-a"));
        assert_eq!(key.len(), 64);
    }
}
