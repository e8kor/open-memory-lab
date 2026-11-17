//! Upload sessions (ADR-0004 d3, amended 2026-11-17; #451): the fixed part
//! size, the limits, the states, the expiry rule and the ten refusals. Pure:
//! the app applies them, the store keeps the rows, the object store keeps
//! the parts.

/// Every part is this long, except the last, which is the remainder
/// (ADR-0004 d3, "Limits").
pub const PART_SIZE: u64 = 8 * 1024 * 1024;

/// `uploads.max_bytes`' default: one upload is at most 1 GiB.
pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// `uploads.tenant_max_bytes`' default: a tenant's open and unbound
/// sessions declare at most 4 GiB together.
pub const DEFAULT_TENANT_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// A key holds at most this many sessions that are open, or completed and
/// unbound.
pub const MAX_SESSIONS_PER_KEY: u64 = 4;

const HOUR_MICROS: i64 = 3_600_000_000;

/// An open session expires this long after its opening or its last part.
pub const IDLE_MICROS: i64 = 24 * HOUR_MICROS;

/// An open session never lives longer than this after it opened.
pub const CAP_MICROS: i64 = 72 * HOUR_MICROS;

/// A completed session nobody bound expires this long after completion.
pub const UNBOUND_MICROS: i64 = 24 * HOUR_MICROS;

/// What an upload is for. One purpose is built; attachments (ADR-0004 d2)
/// add theirs when they are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadPurpose {
    /// A decrypted bundle an import binds (ADR-0084 d15).
    Import,
}

impl UploadPurpose {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "import" => Some(Self::Import),
            _ => None,
        }
    }
}

/// A session's state (ADR-0004 d3, "States").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadState {
    Open,
    Completed,
    Bound,
    Failed,
    Expired,
    Abandoned,
}

impl UploadState {
    pub const ALL: [Self; 6] = [
        Self::Open,
        Self::Completed,
        Self::Bound,
        Self::Failed,
        Self::Expired,
        Self::Abandoned,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Completed => "completed",
            Self::Bound => "bound",
            Self::Failed => "failed",
            Self::Expired => "expired",
            Self::Abandoned => "abandoned",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == s)
    }

    /// Open, or completed and unbound: the states the key's and the
    /// tenant's limits count, and the only ones that expire.
    #[must_use]
    pub fn counts_against_limits(self) -> bool {
        matches!(self, Self::Open | Self::Completed)
    }
}

/// How many parts an upload of `size` bytes has.
#[must_use]
pub fn part_count(size: u64) -> u64 {
    size.div_ceil(PART_SIZE)
}

/// The length part `n` (counted from 1) of an upload of `size` bytes must
/// have, or `None` where `n` is outside 1 to [`part_count`].
#[must_use]
pub fn part_len(size: u64, n: u64) -> Option<u64> {
    let parts = part_count(size);
    if n == 0 || n > parts {
        return None;
    }
    Some(if n == parts {
        size - (parts - 1) * PART_SIZE
    } else {
        PART_SIZE
    })
}

/// When an open session that opened at `opened` and was last touched at
/// `touched` expires, in Unix microseconds: 24 hours after the touch, and
/// never later than 72 hours after the opening.
#[must_use]
pub fn open_expiry(opened: i64, touched: i64) -> i64 {
    (touched + IDLE_MICROS).min(opened + CAP_MICROS)
}

/// When a session completed at `completed` expires unbound.
#[must_use]
pub fn unbound_expiry(completed: i64) -> i64 {
    completed + UNBOUND_MICROS
}

/// Whether `s` is a SHA-256 as the open request declares it: 64 lowercase
/// hex digits.
#[must_use]
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Why an upload act was refused, each one code on the errors page
/// (ADR-0004 d3, "Refusals"). Nothing is journalled for any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadRefusal {
    /// The session's state does not allow the act.
    SessionInvalid { state: UploadState },
    /// The completed bytes' SHA-256 is not the declared one; the session
    /// failed.
    DigestMismatch,
    /// The session expired.
    Expired,
    /// The declared size is over `uploads.max_bytes`.
    TooLarge { max_bytes: u64 },
    /// A part's length is not the one its number requires.
    PartSize { expected: u64, got: u64 },
    /// The part number is outside 1 to the part count.
    PartNumber { parts: u64 },
    /// Completion with parts missing; the first missing numbers.
    Incomplete { missing: Vec<u64> },
    /// The key already holds [`MAX_SESSIONS_PER_KEY`] open or unbound
    /// sessions.
    SessionLimit { limit: u64 },
    /// Opening would take the tenant's open and unbound sessions over
    /// `uploads.tenant_max_bytes`.
    TenantLimit { max_bytes: u64 },
    /// The object store failed a read or a write; nothing changed state.
    StorageFailed,
}

impl UploadRefusal {
    /// The ten codes, in the order the errors page lists them.
    pub const CODES: [&'static str; 10] = [
        "upload.session_invalid",
        "upload.digest_mismatch",
        "upload.expired",
        "upload.too_large",
        "upload.part_size",
        "upload.part_number",
        "upload.incomplete",
        "upload.session_limit",
        "upload.tenant_limit",
        "upload.storage_failed",
    ];

    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::SessionInvalid { .. } => Self::CODES[0],
            Self::DigestMismatch => Self::CODES[1],
            Self::Expired => Self::CODES[2],
            Self::TooLarge { .. } => Self::CODES[3],
            Self::PartSize { .. } => Self::CODES[4],
            Self::PartNumber { .. } => Self::CODES[5],
            Self::Incomplete { .. } => Self::CODES[6],
            Self::SessionLimit { .. } => Self::CODES[7],
            Self::TenantLimit { .. } => Self::CODES[8],
            Self::StorageFailed => Self::CODES[9],
        }
    }
}

impl std::fmt::Display for UploadRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionInvalid { state } => {
                write!(f, "the upload session is {}", state.as_str())
            }
            Self::DigestMismatch => {
                f.write_str("the uploaded bytes are not the SHA-256 the session declared")
            }
            Self::Expired => f.write_str("the upload session expired"),
            Self::TooLarge { max_bytes } => {
                write!(
                    f,
                    "an upload is at most {max_bytes} bytes (uploads.max_bytes)"
                )
            }
            Self::PartSize { expected, got } => {
                write!(f, "this part is {expected} bytes long, not {got}")
            }
            Self::PartNumber { parts } => {
                write!(f, "a part number is 1 to {parts}")
            }
            Self::Incomplete { missing } => {
                write!(f, "{} part(s) are missing", missing.len())
            }
            Self::SessionLimit { limit } => write!(
                f,
                "the key holds {limit} open or unbound upload sessions, its limit"
            ),
            Self::TenantLimit { max_bytes } => write!(
                f,
                "the tenant's open and unbound uploads would pass {max_bytes} bytes (uploads.tenant_max_bytes)"
            ),
            Self::StorageFailed => f.write_str("the object store failed; try again"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_is_eight_mebibytes_but_the_last_which_is_the_remainder() {
        let size = 2 * PART_SIZE + 5;
        assert_eq!(part_count(size), 3);
        assert_eq!(part_len(size, 1), Some(PART_SIZE));
        assert_eq!(part_len(size, 2), Some(PART_SIZE));
        assert_eq!(part_len(size, 3), Some(5));
        assert_eq!(part_len(size, 0), None);
        assert_eq!(part_len(size, 4), None);
        // An exact multiple has no short part.
        assert_eq!(part_count(PART_SIZE), 1);
        assert_eq!(part_len(PART_SIZE, 1), Some(PART_SIZE));
        assert_eq!(part_count(1), 1);
        assert_eq!(part_len(1, 1), Some(1));
    }

    #[test]
    fn an_open_session_slides_24_hours_and_never_passes_72() {
        let opened = 1_000;
        assert_eq!(open_expiry(opened, opened), opened + IDLE_MICROS);
        let late = opened + 60 * HOUR_MICROS;
        assert_eq!(open_expiry(opened, late), opened + CAP_MICROS);
        assert_eq!(unbound_expiry(5), 5 + UNBOUND_MICROS);
    }

    #[test]
    fn there_are_ten_upload_codes_each_named_once() {
        let mut codes = UploadRefusal::CODES.to_vec();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 10);
        assert_eq!(
            UploadRefusal::Incomplete { missing: vec![2] }.code(),
            "upload.incomplete"
        );
    }

    #[test]
    fn a_declared_sha256_is_64_lowercase_hex_digits() {
        assert!(is_sha256_hex(&"a".repeat(64)));
        assert!(!is_sha256_hex(&"A".repeat(64)));
        assert!(!is_sha256_hex(&"a".repeat(63)));
        assert!(!is_sha256_hex(&format!("sha256:{}", "a".repeat(57))));
    }

    #[test]
    fn states_and_purposes_round_trip_their_spelling() {
        for state in UploadState::ALL {
            assert_eq!(UploadState::parse(state.as_str()), Some(state));
        }
        assert_eq!(UploadPurpose::parse("import"), Some(UploadPurpose::Import));
        assert_eq!(UploadPurpose::parse("attachment"), None);
        assert!(UploadState::Completed.counts_against_limits());
        assert!(!UploadState::Bound.counts_against_limits());
    }
}
