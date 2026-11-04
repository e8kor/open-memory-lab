//! The authorization request's handle sealer (ADR-0031 d9 step 1;
//! `docs/api/http-api.md`, `GET /oauth/authorize`): ChaCha20-Poly1305 under
//! a 256-bit key the process generates when it starts and holds only in
//! memory. Nothing writes the key anywhere, so a restart or a takeover of
//! the lead ends every page in progress, which starts again.
//!
//! A handle is the lower-case hex of a fresh random 96-bit nonce, then the
//! ciphertext and its 128-bit tag. The nonce is drawn from the OS for each
//! seal; at the page rates one lead serves, a repeated random nonce under
//! one key is not a practical concern within a process's life. The
//! associated data names the plaintext's purpose, so a sealed handle opens
//! as nothing else.

use std::fmt;

use oml_app::ports::HandleSealer;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};

/// Binds every handle to what it carries.
const PURPOSE: &[u8] = b"oml oauth authorization request";

/// The web sign-in cookie's purpose, so a connect page's handle never opens
/// as a sign-in and the reverse (ADR-0045 d3).
pub const WEB_SIGN_IN_PURPOSE: &[u8] = b"oml web sign-in";

/// The lead's sealer: one key per process, never stored.
pub struct AeadSealer {
    key: LessSafeKey,
    purpose: &'static [u8],
}

impl AeadSealer {
    /// A sealer under a fresh 256-bit key from the OS random source, for
    /// the authorization request's handle.
    #[must_use]
    pub fn generate() -> Self {
        Self::generate_for(PURPOSE)
    }

    /// A sealer under a fresh key whose seals open only as `purpose`.
    #[must_use]
    pub fn generate_for(purpose: &'static [u8]) -> Self {
        let mut key = [0_u8; 32];
        getrandom::getrandom(&mut key).expect("the OS random source is available");
        let key = UnboundKey::new(&CHACHA20_POLY1305, &key)
            .expect("a 256-bit key is ChaCha20-Poly1305's key length");
        Self {
            key: LessSafeKey::new(key),
            purpose,
        }
    }
}

impl fmt::Debug for AeadSealer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AeadSealer(ChaCha20-Poly1305, key withheld)")
    }
}

impl HandleSealer for AeadSealer {
    fn seal(&self, plaintext: &[u8]) -> String {
        let mut nonce = [0_u8; NONCE_LEN];
        getrandom::getrandom(&mut nonce).expect("the OS random source is available");
        let mut sealed = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(self.purpose),
                &mut sealed,
            )
            .expect("a handle's plaintext is far below the AEAD's length limit");
        let mut handle = Vec::with_capacity(NONCE_LEN + sealed.len());
        handle.extend_from_slice(&nonce);
        handle.extend_from_slice(&sealed);
        hex::encode(handle)
    }

    fn open(&self, handle: &str) -> Option<Vec<u8>> {
        if !handle
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return None;
        }
        let bytes = hex::decode(handle).ok()?;
        if bytes.len() < NONCE_LEN + CHACHA20_POLY1305.tag_len() {
            return None;
        }
        let (nonce, sealed) = bytes.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
        let mut sealed = sealed.to_vec();
        let plaintext = self
            .key
            .open_in_place(nonce, Aad::from(self.purpose), &mut sealed)
            .ok()?;
        Some(plaintext.to_vec())
    }
}
