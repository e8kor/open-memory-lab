//! A shown-once secret across Post/Redirect/Get (ADR-0070 d3 layer 4): a
//! key's value or a connect code is shown exactly once, on the page the
//! act redirects to, and `oml-web` keeps no copy. The act seals the secret
//! into a short-lived cookie that the redirect target opens, shows and
//! clears in the same answer; it is never in a URL, never in a line and
//! never in this process's memory past the request.
//!
//! The seal is ChaCha20-Poly1305 under a key derived from the session
//! token ([`oml_web_credential::once_seal_key`]), which only the browser and
//! the lead hold, so any `oml-web` replica opens it for the same session and
//! no other session can; the associated data names the page, so a seal
//! opens on that page alone.
//!
//! What the cookie costs, kept visible (ADR-0070, the table of #253):
//!
//! - `__Host-` forces `Path=/`, so the sealed cookie rides every request of
//!   the site, the API's included, until the target clears it or
//!   [`ONCE_MAX_AGE`] passes. A redirect is followed at once, so a minute
//!   bounds the window. Neither `oml-web`'s request line nor the lead's
//!   reads `Cookie`, and `oml-web` never forwards the cookie to the API; a
//!   proxy's own access log must not record `Cookie` (Caddy redacts it
//!   unless `log_credentials` is set).
//! - Clearing is the browser's: a seal copied out of the browser opens,
//!   with the session's token, until it expires, the same reach the token
//!   already gives to that session's keys.
//! - One cookie name serves every act: two acts that show a secret, sent
//!   from two tabs of one session before either target loads, leave the
//!   second seal, and the first target shows it or nothing. The first
//!   secret is not shown; the key it names is listed, and the person cuts
//!   it off and issues another, or asks a new connect code.

use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use serde::{Deserialize, Serialize};

/// The cookie that carries a sealed secret from an act to its redirect
/// target: host-only, script-blind, sent only to this site's own
/// navigations, and gone in a minute if the target is never loaded.
pub const ONCE_COOKIE: &str = "__Host-oml_once";

/// How long a sealed secret waits for its page, in seconds: the redirect
/// is followed at once, so a minute is ample and bounds how long the
/// cookie rides every request (#253; chosen, not measured).
pub const ONCE_MAX_AGE: u32 = 60;

const PURPOSE: &[u8] = b"oml-web shown-once v1\n";

/// What an act shows once.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Secret {
    /// A bearer key's value, with the server URL an agent sets beside it.
    Key {
        name: String,
        value: String,
        server: String,
    },
    /// A connector key's connect code, with its connector URL and expiry.
    Code {
        name: String,
        code: String,
        expires_at: String,
        connector_url: String,
    },
}

/// No debug output prints the value or the code.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Key { name, .. } => f.debug_struct("Key").field("name", name).finish(),
            Self::Code { name, .. } => f.debug_struct("Code").field("name", name).finish(),
        }
    }
}

fn key(session_token: &str) -> LessSafeKey {
    let derived = oml_web_credential::once_seal_key(session_token);
    LessSafeKey::new(
        UnboundKey::new(&CHACHA20_POLY1305, &derived)
            .expect("an HMAC-SHA256 tag is ChaCha20-Poly1305's key length"),
    )
}

fn aad(page: &str) -> Vec<u8> {
    let mut aad = PURPOSE.to_vec();
    aad.extend_from_slice(page.as_bytes());
    aad
}

/// `secret` sealed for `page` under the session's key, as the cookie's
/// value: lower-case hex of the nonce, the ciphertext and the tag.
#[must_use]
pub fn seal(session_token: &str, page: &str, secret: &Secret) -> String {
    let mut nonce = [0_u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).expect("the OS random source answers");
    let mut sealed = serde_json::to_vec(secret).expect("a secret serializes");
    key(session_token)
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad(page)),
            &mut sealed,
        )
        .expect("a secret is far below the AEAD's length limit");
    let mut out = String::with_capacity(2 * (NONCE_LEN + sealed.len()));
    for b in nonce.iter().chain(&sealed) {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// The secret `value` holds for `page` under the session's key; `None`
/// for anything else: another session's seal, another page's, or bytes
/// that are not a seal.
#[must_use]
pub fn open(session_token: &str, page: &str, value: &str) -> Option<Secret> {
    if !value.len().is_multiple_of(2) || value.len() < 2 * NONCE_LEN {
        return None;
    }
    let bytes: Vec<u8> = (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(value.get(i..i + 2)?, 16).ok())
        .collect::<Option<_>>()?;
    let (nonce, sealed) = bytes.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
    let mut sealed = sealed.to_vec();
    let plain = key(session_token)
        .open_in_place(nonce, Aad::from(aad(page)), &mut sealed)
        .ok()?;
    serde_json::from_slice(plain).ok()
}

/// The `Set-Cookie` that carries `sealed` to the redirect target.
#[must_use]
pub fn set_cookie(sealed: &str) -> String {
    format!(
        "{ONCE_COOKIE}={sealed}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age={ONCE_MAX_AGE}"
    )
}

/// The `Set-Cookie` that clears it, sent with the page that shows it.
#[must_use]
pub fn clear_cookie() -> String {
    format!("{ONCE_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> Secret {
        Secret::Key {
            name: "alice-laptop".to_owned(),
            value: "oml_the-value".to_owned(),
            server: "https://oml.example.com".to_owned(),
        }
    }

    #[test]
    fn a_seal_opens_for_its_session_and_page_alone() {
        let sealed = seal("session-a", "/web/keys", &secret());
        assert!(!sealed.contains("oml_the-value"));
        assert_eq!(open("session-a", "/web/keys", &sealed), Some(secret()));
        assert_eq!(open("session-b", "/web/keys", &sealed), None);
        assert_eq!(open("session-a", "/web/review", &sealed), None);
        assert_eq!(open("session-a", "/web/keys", "zz"), None);
        let mut flipped = sealed.clone().into_bytes();
        let last = flipped.len() - 1;
        flipped[last] = if flipped[last] == b'0' { b'1' } else { b'0' };
        let flipped = String::from_utf8(flipped).unwrap();
        assert_eq!(open("session-a", "/web/keys", &flipped), None);
    }

    #[test]
    fn no_debug_output_prints_a_secret() {
        let shown = format!("{:?}", secret());
        assert!(!shown.contains("oml_the-value"), "{shown}");
    }
}
