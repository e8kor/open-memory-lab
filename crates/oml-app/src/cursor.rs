//! The listing cursor (http-api.md pagination; ADR-0025 d13): an opaque
//! keyset position, base64url (no padding) over the JSON
//! `{key_id, created_at, artifact_id, issued_at}`. It carries a position
//! only — every page re-applies the listing's predicates — so a client that
//! forges one for its own key learns nothing a fresh first page would not.
//! It answers only the key it was issued to, and only within
//! `api.cursor_ttl` of its issue.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use oml_domain::EventTime;

use crate::types::ListingPosition;

/// `api.cursor_ttl`, whose deployment default configuration.md leaves to
/// FR-22: until settings are built, this code constant is the lifetime of
/// every cursor this build issues, in seconds.
pub const CURSOR_TTL_SECS: i64 = 3_600;

/// A cursor this server will not resume from: each is `request.validation`
/// (errors.md, "a stale or foreign pagination cursor").
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CursorError {
    #[error("the cursor is not one this server issues")]
    Malformed,
    #[error("the cursor was issued to another key")]
    Foreign,
    #[error("the cursor expired: cursors live {CURSOR_TTL_SECS} s (api.cursor_ttl)")]
    Stale,
}

/// A decoded listing cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingCursor {
    /// The key the cursor answers.
    pub key_id: Uuid,
    pub position: ListingPosition,
    pub issued_at: EventTime,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    key_id: Uuid,
    created_at: String,
    artifact_id: Uuid,
    issued_at: String,
}

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_b64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
        // n bytes fill n + 1 sextets; padding is never written.
        for i in 0..=chunk.len() {
            out.push(char::from(B64URL[(group >> (18 - 6 * i)) as usize & 63]));
        }
    }
    out
}

fn decode_b64url(text: &str) -> Option<Vec<u8>> {
    // One trailing sextet carries no whole byte: no encoder writes it.
    if text.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0_u32, 0_u32);
    for c in text.bytes() {
        let sextet = B64URL.iter().position(|&a| a == c)?;
        acc = (acc << 6) | u32::try_from(sextet).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits).ok()?);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

impl ListingCursor {
    /// The opaque string a page's `next_cursor` carries.
    #[must_use]
    pub fn encode(&self) -> String {
        let wire = Wire {
            key_id: self.key_id,
            created_at: self.position.created_at.as_str().to_owned(),
            artifact_id: self.position.artifact_id,
            issued_at: self.issued_at.as_str().to_owned(),
        };
        let json = serde_json::to_vec(&wire).expect("a cursor serializes");
        encode_b64url(&json)
    }

    /// Decodes a cursor string; anything an encoder did not write is
    /// [`CursorError::Malformed`].
    pub fn decode(text: &str) -> Result<Self, CursorError> {
        let bytes = decode_b64url(text).ok_or(CursorError::Malformed)?;
        let wire: Wire = serde_json::from_slice(&bytes).map_err(|_| CursorError::Malformed)?;
        let time = |s: &str| EventTime::parse(s).map_err(|_| CursorError::Malformed);
        Ok(Self {
            key_id: wire.key_id,
            position: ListingPosition {
                created_at: time(&wire.created_at)?,
                artifact_id: wire.artifact_id,
            },
            issued_at: time(&wire.issued_at)?,
        })
    }

    /// The position `text` resumes at for `key_id` at `now`: a cursor
    /// issued to another key is foreign, and one older than
    /// [`CURSOR_TTL_SECS`] is stale (http-api.md pagination).
    pub fn resume(
        text: &str,
        key_id: Uuid,
        now: &EventTime,
    ) -> Result<ListingPosition, CursorError> {
        let cursor = Self::decode(text)?;
        if cursor.key_id != key_id {
            return Err(CursorError::Foreign);
        }
        if now.unix_micros() - cursor.issued_at.unix_micros() > CURSOR_TTL_SECS * 1_000_000 {
            return Err(CursorError::Stale);
        }
        Ok(cursor.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> EventTime {
        EventTime::parse(s).expect("fixed width")
    }

    fn cursor(key: u128, issued_at: &str) -> ListingCursor {
        ListingCursor {
            key_id: Uuid::from_u128(key),
            position: ListingPosition {
                created_at: at("2026-10-30T09:15:00.123456Z"),
                artifact_id: Uuid::from_u128(0xA1),
            },
            issued_at: at(issued_at),
        }
    }

    #[test]
    fn a_cursor_round_trips_through_its_opaque_form() {
        let c = cursor(7, "2026-10-30T09:16:00.000000Z");
        let text = c.encode();
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "base64url without padding: {text}"
        );
        assert_eq!(ListingCursor::decode(&text), Ok(c));
    }

    #[test]
    fn base64url_matches_the_rfc_4648_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode_b64url(plain.as_bytes()), encoded);
            assert_eq!(decode_b64url(encoded), Some(plain.as_bytes().to_vec()));
        }
        assert_eq!(encode_b64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(decode_b64url("Zm9vY"), None);
        assert_eq!(decode_b64url("Zm9v+g"), None);
    }

    #[test]
    fn a_cursor_resumes_only_for_its_key_and_only_within_the_ttl() {
        let text = cursor(7, "2026-10-30T09:16:00.000000Z").encode();
        let fresh = at("2026-10-30T10:16:00.000000Z");
        assert!(ListingCursor::resume(&text, Uuid::from_u128(7), &fresh).is_ok());
        assert_eq!(
            ListingCursor::resume(&text, Uuid::from_u128(8), &fresh),
            Err(CursorError::Foreign)
        );
        assert_eq!(
            ListingCursor::resume(
                &text,
                Uuid::from_u128(7),
                &at("2026-10-30T10:16:00.000001Z")
            ),
            Err(CursorError::Stale)
        );
        assert_eq!(
            ListingCursor::resume("not a cursor", Uuid::from_u128(7), &fresh),
            Err(CursorError::Malformed)
        );
        assert_eq!(
            ListingCursor::resume(&encode_b64url(b"{}"), Uuid::from_u128(7), &fresh),
            Err(CursorError::Malformed)
        );
    }
}
