//! Token formats: an access token is `oml_at_` and a refresh token
//! `oml_rt_`, each followed by 43 base62 characters encoding 256 bits under
//! the key format's generation rule; a key is `oml_` and 43 base62
//! characters. The second underscore keeps a token from ever parsing as a
//! key, and a key from ever parsing as a token (ADR-0031 d10;
//! `docs/architecture/roles-and-keys.md`, "Key format" and "Connections").
//! The randomness is the caller's.

use std::fmt;

use sha2::{Digest, Sha256};

use super::refusal::{RefreshInvalid, Refusal};

/// The key format's prefix.
pub const KEY_PREFIX: &str = "oml_";

/// The base62 characters after each prefix.
pub const BASE62_CHARS: usize = 43;

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// A connection's two credentials (`oauth_tokens.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Access,
    Refresh,
}

impl TokenKind {
    /// `oml_at_` or `oml_rt_`.
    #[must_use]
    pub fn prefix(self) -> &'static str {
        match self {
            TokenKind::Access => "oml_at_",
            TokenKind::Refresh => "oml_rt_",
        }
    }
}

/// A minted token. Its `Debug` names only its kind, and it has no
/// `Display`: [`Token::as_str`] is for the token response alone, and the
/// store keeps [`Token::digest`] (ADR-0031 d10).
#[derive(Clone, PartialEq, Eq)]
pub struct Token {
    kind: TokenKind,
    value: String,
}

impl Token {
    /// The token for 32 bytes the caller drew from a cryptographically
    /// secure generator.
    #[must_use]
    pub fn mint(kind: TokenKind, random: [u8; 32]) -> Self {
        Token {
            kind,
            value: format!("{}{}", kind.prefix(), base62_43(&random)),
        }
    }

    #[must_use]
    pub fn kind(&self) -> TokenKind {
        self.kind
    }

    /// The token string, for the token response and nowhere else.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// `oauth_tokens.hash`: the SHA-256 of the whole string, prefix
    /// included.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.value.as_bytes()).into()
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Token")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// 256 bits as exactly 43 base62 characters, most significant first and
/// zero-padded: the key format's encoding (roles-and-keys.md, "Key
/// format"). 62⁴³ exceeds 2²⁵⁶, so 43 characters always suffice.
#[must_use]
pub fn base62_43(bytes: &[u8; 32]) -> String {
    let mut number = *bytes;
    let mut digits = [b'0'; BASE62_CHARS];
    for digit in digits.iter_mut().rev() {
        let mut remainder = 0_u32;
        for byte in &mut number {
            let acc = (remainder << 8) | u32::from(*byte);
            // acc < 62 · 256, so the quotient fits a byte.
            *byte = u8::try_from(acc / 62).unwrap_or(u8::MAX);
            remainder = acc % 62;
        }
        *digit = BASE62[remainder as usize];
    }
    String::from_utf8(digits.to_vec()).unwrap_or_default()
}

fn is_base62_43(s: &str) -> bool {
    s.len() == BASE62_CHARS && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether `s` is in the key format: `oml_` and 43 base62 characters.
#[must_use]
pub fn is_key_value(s: &str) -> bool {
    s.strip_prefix(KEY_PREFIX).is_some_and(is_base62_43)
}

/// The kind of token `s` is in the format of, if any.
#[must_use]
pub fn token_kind(s: &str) -> Option<TokenKind> {
    [TokenKind::Access, TokenKind::Refresh]
        .into_iter()
        .find(|kind| s.strip_prefix(kind.prefix()).is_some_and(is_base62_43))
}

/// The lookup digest of a presented `refresh_token`. Only a string in the
/// refresh-token format is looked up; an access token, a key or anything
/// else is `oauth.refresh_invalid` (`invalid_grant`) before any lookup
/// (ADR-0031 d11).
pub fn refresh_token_digest(presented: &str) -> Result<[u8; 32], Refusal> {
    if token_kind(presented) == Some(TokenKind::Refresh) {
        Ok(Sha256::digest(presented.as_bytes()).into())
    } else {
        Err(Refusal::RefreshInvalid(RefreshInvalid::NotARefreshToken))
    }
}
