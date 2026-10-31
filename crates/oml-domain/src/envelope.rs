//! The signed event envelope — the nine signed fields of
//! `docs/data/chain-format.md` with their canonical encodings, plus the
//! event-type spellings of `docs/data/event-catalog.md`.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::jcs::{self, JcsError};

/// Event-type spellings, exactly as `docs/data/event-catalog.md` names them.
/// The PoC's M1 slice emits these; the catalog holds the rest.
pub mod event_types {
    /// First event of every tenant chain (chain-format.md genesis rule).
    pub const TENANT_GENESIS: &str = "tenant.genesis";
    /// Tenant creation in the reserved provider tenant's chain.
    pub const TENANT_CREATED: &str = "tenant.created";
    /// New workarea node under an existing parent.
    pub const WORKAREA_CREATE: &str = "workarea.create";
    /// A workarea now names another crew record (ADR-0026 d8).
    pub const WORKAREA_CREW_UPDATED: &str = "workarea.crew_updated";
    /// A named crew record, created by an admin (ADR-0026 d8).
    pub const CREW_CREATED: &str = "crew.created";
    /// A crew record's charter URI or external manager, corrected by an
    /// admin, the full new values; never its name (ADR-0026 d8).
    pub const CREW_UPDATED: &str = "crew.updated";
    /// Key issuance without its value (show-once ceremony).
    pub const KEY_ISSUED: &str = "key.issued";
    /// One grant on an issued key; its seq is the grant's epoch (ADR-0025 d2).
    pub const GRANT_ISSUED: &str = "grant.issued";
    /// A global per-key denial, deny-wins; its seq is the denial's epoch
    /// (ADR-0025 d2).
    pub const KEY_DENIED: &str = "key.denied";
    /// A global per-key denial lifted; its seq is the lift's epoch, and the
    /// denial row stays as history. The lead lifts every denial on an admin
    /// key in the commit that names its chain (ADR-0026 d11, rule 5).
    pub const KEY_DENIAL_LIFTED: &str = "key.denial_lifted";
    /// A key's revocation; journalled for a key whose issuance failed after
    /// its `key.issued` (ADR-0025 d2, reason `issuance_incomplete`).
    pub const KEY_REVOKED: &str = "key.revoked";
    /// A writer submits a memory; held on arrival.
    pub const SUBMISSION_REMEMBER: &str = "submission.remember";
    /// A derived claim with its lineage; a supersession whose successor is
    /// a `derived_fact` journals it too (ADR-0025 d10).
    pub const SUBMISSION_LEARN: &str = "submission.learn";
    /// Approving verdict from a valid serve.
    pub const REVIEW_APPROVE: &str = "review.approve";
    /// Rejecting verdict from a valid serve — the act behind "rejected".
    pub const REVIEW_REJECT: &str = "review.reject";
    /// The act that finds the review policy satisfied.
    pub const REVIEW_RATIFY: &str = "review.ratify";
    /// Ratified content becomes readable by others.
    pub const LIFECYCLE_PROMOTE: &str = "lifecycle.promote";
    /// A promoted supersession takes the tip from the version it replaces.
    pub const LIFECYCLE_SUPERSEDE: &str = "lifecycle.supersede";
    /// Crypto-shredding erasure (vector 3 of chain-format.md).
    pub const LIFECYCLE_ERASE: &str = "lifecycle.erase";
    /// Deployment signing-key rotation, first event the new key signs.
    pub const SYSTEM_SIGNING_KEY_ROTATED: &str = "system.signing_key_rotated";
    /// The names of a chain's rows that predate names, and the crew records
    /// made from their crew objects, journalled once per chain by the lead
    /// (ADR-0026 d11).
    pub const SYSTEM_NAMES_ASSIGNED: &str = "system.names_assigned";
}

/// A SHA-256 value. Displayed and journalled as `sha256:` + 64 lowercase hex
/// characters (chain-format.md: "Digests are strings, everywhere").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256Hash(pub [u8; 32]);

/// A malformed `sha256:<64 hex>` digest string.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("not a `sha256:` + 64 lowercase hex digest string: {0:?}")]
pub struct DigestParseError(pub String);

impl Sha256Hash {
    /// The genesis `prev_hash`: 32 zero bytes (chain-format.md genesis rule).
    pub const ZERO: Self = Self([0_u8; 32]);

    /// SHA-256 of `bytes`.
    #[must_use]
    pub fn digest(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// The canonical string spelling: `sha256:` + 64 lowercase hex.
    #[must_use]
    pub fn to_prefixed(&self) -> String {
        format!("sha256:{}", hex::encode(self.0))
    }

    /// Parses the canonical `sha256:<64 lowercase hex>` spelling.
    pub fn from_prefixed(s: &str) -> Result<Self, DigestParseError> {
        let err = || DigestParseError(s.to_owned());
        let hex_part = s.strip_prefix("sha256:").ok_or_else(err)?;
        if hex_part.len() != 64 || hex_part.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(err());
        }
        let raw = hex::decode(hex_part).map_err(|_| err())?;
        let arr: [u8; 32] = raw.try_into().map_err(|_| err())?;
        Ok(Self(arr))
    }
}

impl std::fmt::Display for Sha256Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_prefixed())
    }
}

/// Convenience for payload digest fields: `sha256:<64 lowercase hex>` of `bytes`.
#[must_use]
pub fn sha256_digest_string(bytes: &[u8]) -> String {
    Sha256Hash::digest(bytes).to_prefixed()
}

/// The envelope `at` value: RFC 3339 UTC with exactly six fractional digits
/// and a `Z` suffix — fixed width, never trimmed in the signed form
/// (chain-format.md, "`at` is fixed-width").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventTime(String);

/// A timestamp that is not in the fixed-width signed form.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("not a `YYYY-MM-DDTHH:MM:SS.ffffffZ` UTC timestamp: {0:?}")]
pub struct TimeParseError(pub String);

impl EventTime {
    /// Validates the fixed 27-byte form `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
    pub fn parse(s: &str) -> Result<Self, TimeParseError> {
        let err = || TimeParseError(s.to_owned());
        let b = s.as_bytes();
        if b.len() != 27 {
            return Err(err());
        }
        let digit_positions = [
            0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22, 23, 24, 25,
        ];
        if digit_positions.iter().any(|&i| !b[i].is_ascii_digit()) {
            return Err(err());
        }
        if b[4] != b'-'
            || b[7] != b'-'
            || b[10] != b'T'
            || b[13] != b':'
            || b[16] != b':'
            || b[19] != b'.'
            || b[26] != b'Z'
        {
            return Err(err());
        }
        let two = |i: usize| (b[i] - b'0') * 10 + (b[i + 1] - b'0');
        let (month, day, hour, minute, second) = (two(5), two(8), two(11), two(14), two(17));
        if !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return Err(err());
        }
        Ok(Self(s.to_owned()))
    }

    /// The validated string, exactly as signed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Microseconds since the Unix epoch: the arithmetic an expiry check
    /// needs, such as a listing cursor's `api.cursor_ttl` (http-api.md
    /// pagination). The string stays the value signed and stored.
    #[must_use]
    pub fn unix_micros(&self) -> i64 {
        let b = self.0.as_bytes();
        let num = |from: usize, to: usize| {
            b[from..to]
                .iter()
                .fold(0_i64, |acc, d| acc * 10 + i64::from(d - b'0'))
        };
        let (year, month, day) = (num(0, 4), num(5, 7), num(8, 10));
        // Howard Hinnant's days_from_civil, exact over the proleptic
        // Gregorian calendar.
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        let secs = days * 86_400 + num(11, 13) * 3_600 + num(14, 16) * 60 + num(17, 19);
        secs * 1_000_000 + num(20, 26)
    }
}

impl std::fmt::Display for EventTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The nine signed envelope fields, exactly as chain-format.md enumerates
/// them. `hash` and `sig` are derived and live on
/// [`crate::chain::SignedEnvelope`], never here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub tenant_id: Uuid,
    pub seq: u64,
    pub event_id: Uuid,
    pub event_type: String,
    pub prev_hash: Sha256Hash,
    pub principal: String,
    /// `None` journals as JSON `null` — the field is always present
    /// (chain-format.md: "The nine signed fields are always present").
    pub api_key_id: Option<Uuid>,
    pub at: EventTime,
    /// The event's payload object, canonicalized as part of the same document.
    pub payload: Value,
}

impl Envelope {
    /// The JSON object holding exactly the nine signed fields, with the
    /// canonical per-field encodings of chain-format.md.
    #[must_use]
    pub fn signed_value(&self) -> Value {
        let mut obj = Map::new();
        obj.insert(
            "tenant_id".to_owned(),
            Value::String(self.tenant_id.to_string()),
        );
        obj.insert("seq".to_owned(), Value::from(self.seq));
        obj.insert(
            "event_id".to_owned(),
            Value::String(self.event_id.to_string()),
        );
        obj.insert(
            "event_type".to_owned(),
            Value::String(self.event_type.clone()),
        );
        obj.insert(
            "prev_hash".to_owned(),
            Value::String(self.prev_hash.to_prefixed()),
        );
        obj.insert(
            "principal".to_owned(),
            Value::String(self.principal.clone()),
        );
        obj.insert(
            "api_key_id".to_owned(),
            match self.api_key_id {
                Some(id) => Value::String(id.to_string()),
                None => Value::Null,
            },
        );
        obj.insert("at".to_owned(), Value::String(self.at.as_str().to_owned()));
        obj.insert("payload".to_owned(), self.payload.clone());
        Value::Object(obj)
    }

    /// `signed_bytes`: RFC 8785 canonicalization of the nine signed fields.
    pub fn signed_bytes(&self) -> Result<Vec<u8>, JcsError> {
        jcs::canonical_bytes(&self.signed_value())
    }
}
