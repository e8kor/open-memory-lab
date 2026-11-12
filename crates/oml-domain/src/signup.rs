//! Open sign-up's pure rules (ADR-0068): the mode, the per-person quotas
//! and their defaults, the user name a sign-up draws, and the quota check.
//! The flow and the store are the application's.

use crate::name::Name;
use crate::oauth::CROCKFORD_ALPHABET;

/// `signup.mode` (ADR-0068 d1): `allowlist`, the shipped default, keeps
/// ADR-0045 unchanged; `open` lets a verified email no entry holds sign up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignupMode {
    #[default]
    Allowlist,
    Open,
}

impl SignupMode {
    /// The setting's value, or `None` for anything else.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "allowlist" => Some(SignupMode::Allowlist),
            "open" => Some(SignupMode::Open),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SignupMode::Allowlist => "allowlist",
            SignupMode::Open => "open",
        }
    }
}

/// One of the six per-person quotas (ADR-0068 d5; ADR-0071 d5): the
/// closed set the refusal's `details.quota` and the metric's label take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Quota {
    Memories,
    TextBytes,
    WritesPerMonth,
    Keys,
    Connections,
    RequestsPerMinute,
}

impl Quota {
    /// Every quota, in the order the pages list them.
    pub const ALL: [Quota; 6] = [
        Quota::Memories,
        Quota::TextBytes,
        Quota::WritesPerMonth,
        Quota::Keys,
        Quota::Connections,
        Quota::RequestsPerMinute,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Quota::Memories => "memories",
            Quota::TextBytes => "text_bytes",
            Quota::WritesPerMonth => "writes_per_month",
            Quota::Keys => "keys",
            Quota::Connections => "connections",
            Quota::RequestsPerMinute => "requests_per_minute",
        }
    }

    /// The largest limit an admin or a profile may set for this quota: a
    /// byte count reaches further than a count (ADR-0071 d5).
    #[must_use]
    pub fn max_limit(self) -> u64 {
        match self {
            Quota::TextBytes => TEXT_BYTES_LIMIT_MAX,
            _ => QUOTA_LIMIT_MAX,
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Quota::ALL.into_iter().find(|q| q.as_str() == value)
    }
}

/// The largest count limit an admin may set (ADR-0068 d5).
pub const QUOTA_LIMIT_MAX: u64 = 1_000_000;

/// The largest `text_bytes` limit, a terabyte (ADR-0071 d5).
pub const TEXT_BYTES_LIMIT_MAX: u64 = 1_000_000_000_000;

/// A limit no count reaches: `text_bytes` and `writes_per_month` in the
/// `default` profile until the operator sets them (ADR-0071 d5).
pub const UNLIMITED: u64 = u64::MAX;

/// The per-person limits: a profile's, or one person's after an admin's
/// override (ADR-0068 d1, d5; ADR-0071 d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quotas {
    pub memories: u64,
    pub text_bytes: u64,
    pub writes_per_month: u64,
    pub keys: u64,
    pub connections: u64,
    pub requests_per_minute: u64,
}

impl Default for Quotas {
    /// The operator's defaults of 2026-11-06, unmeasured (issue #164);
    /// `text_bytes` and `writes_per_month` unlimited until set (ADR-0071 d5).
    fn default() -> Self {
        Self {
            memories: 1000,
            text_bytes: UNLIMITED,
            writes_per_month: UNLIMITED,
            keys: 5,
            connections: 5,
            requests_per_minute: 120,
        }
    }
}

impl Quotas {
    /// The limits of an account whose profile the configuration no longer
    /// declares: every quota a write, a key or a connection is held to is
    /// zero, so each fails closed as `limit.quota`, and the request rate
    /// is `requests_per_minute`, the deployment's, so the account still
    /// reads: no quota refuses a read (ADR-0071 d5, d8).
    #[must_use]
    pub fn writes_refused(requests_per_minute: u64) -> Quotas {
        Quotas {
            memories: 0,
            text_bytes: 0,
            writes_per_month: 0,
            keys: 0,
            connections: 0,
            requests_per_minute,
        }
    }

    #[must_use]
    pub fn get(&self, quota: Quota) -> u64 {
        match quota {
            Quota::Memories => self.memories,
            Quota::TextBytes => self.text_bytes,
            Quota::WritesPerMonth => self.writes_per_month,
            Quota::Keys => self.keys,
            Quota::Connections => self.connections,
            Quota::RequestsPerMinute => self.requests_per_minute,
        }
    }

    /// These limits with `quota` set to `limit`.
    #[must_use]
    pub fn with(mut self, quota: Quota, limit: u64) -> Self {
        match quota {
            Quota::Memories => self.memories = limit,
            Quota::TextBytes => self.text_bytes = limit,
            Quota::WritesPerMonth => self.writes_per_month = limit,
            Quota::Keys => self.keys = limit,
            Quota::Connections => self.connections = limit,
            Quota::RequestsPerMinute => self.requests_per_minute = limit,
        }
        self
    }

    /// These limits with one person's overrides applied: an override
    /// replaces its quota's default, and an absent one keeps it.
    #[must_use]
    pub fn with_overrides(self, overrides: &[(Quota, u64)]) -> Self {
        overrides
            .iter()
            .fold(self, |quotas, (quota, limit)| quotas.with(*quota, *limit))
    }
}

/// A quota reached: what was hit, its limit and what is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaReached {
    pub quota: Quota,
    pub limit: u64,
    pub used: u64,
}

/// Whether one more of `quota` fits: `used` already counted, one more
/// allowed only while `used < limit`, so a limit of zero admits none.
pub fn check_quota(quota: Quota, limit: u64, used: u64) -> Result<(), QuotaReached> {
    check_charge(quota, limit, used, 1)
}

/// Whether `delta` more of `quota` fits beside `used`: the sum may reach the
/// limit and never pass it, so a limit of zero admits nothing and a
/// `text_bytes` charge is its byte count (ADR-0071 d3).
pub fn check_charge(quota: Quota, limit: u64, used: u64, delta: u64) -> Result<(), QuotaReached> {
    if used.saturating_add(delta) <= limit {
        Ok(())
    } else {
        Err(QuotaReached { quota, limit, used })
    }
}

/// How many Crockford base32 symbols follow the `u` of a drawn user name.
pub const DRAWN_USER_SYMBOLS: usize = 10;

/// A sign-up's user name (ADR-0068 d2): `u` and ten lower-case Crockford
/// base32 symbols drawn from `random`, never from the email, so no name
/// carries the address.
#[must_use]
pub fn drawn_user(random: &[u8; 32]) -> Name {
    let alphabet = CROCKFORD_ALPHABET.as_bytes();
    let mut name = String::from("u");
    for byte in random.iter().take(DRAWN_USER_SYMBOLS) {
        name.push(char::from(alphabet[usize::from(*byte & 31)]).to_ascii_lowercase());
    }
    Name::parse(&name).expect("u and lower-case Crockford symbols are a name")
}

/// Whether the 80% warning holds for `used` of `limit`: the usage
/// endpoints' `warning` state and the doctor line (ADR-0071 d5, amending
/// ADR-0068 d5's 90%). An unlimited quota never warns.
#[must_use]
pub fn near_quota(limit: u64, used: u64) -> bool {
    limit > 0 && limit != UNLIMITED && used.saturating_mul(10) >= limit.saturating_mul(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_defaults_are_the_operators_choice() {
        let q = Quotas::default();
        assert_eq!(
            (q.memories, q.keys, q.connections, q.requests_per_minute),
            (1000, 5, 5, 120)
        );
        assert_eq!(Quota::TextBytes.max_limit(), TEXT_BYTES_LIMIT_MAX);
    }

    #[test]
    fn a_quota_admits_one_more_only_below_its_limit() {
        assert!(check_quota(Quota::Keys, 5, 4).is_ok());
        let reached = check_quota(Quota::Keys, 5, 5).unwrap_err();
        assert_eq!((reached.limit, reached.used), (5, 5));
        assert!(check_quota(Quota::Memories, 0, 0).is_err());
    }

    #[test]
    fn an_override_replaces_only_its_own_quota() {
        let q = Quotas::default().with_overrides(&[(Quota::Memories, 5000)]);
        assert_eq!(q.memories, 5000);
        assert_eq!(q.keys, 5);
    }

    #[test]
    fn a_drawn_user_is_a_name_that_starts_with_u_and_carries_no_email() {
        let name = drawn_user(&[0xff; 32]);
        assert_eq!(name.as_str().len(), 1 + DRAWN_USER_SYMBOLS);
        assert!(name.as_str().starts_with('u'));
        assert!(
            name.as_str()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
        assert_ne!(drawn_user(&[0; 32]), drawn_user(&[1; 32]));
    }

    #[test]
    fn the_mode_and_quota_names_round_trip() {
        assert_eq!(SignupMode::parse("open"), Some(SignupMode::Open));
        assert_eq!(SignupMode::parse("Open"), None);
        for q in Quota::ALL {
            assert_eq!(Quota::parse(q.as_str()), Some(q));
        }
    }

    #[test]
    fn near_quota_is_eighty_percent_and_never_for_an_unlimited_quota() {
        assert!(near_quota(1000, 800));
        assert!(!near_quota(1000, 799));
        assert!(!near_quota(0, 0));
        assert!(!near_quota(UNLIMITED, u64::MAX / 2));
    }

    #[test]
    fn a_charge_may_reach_its_limit_and_never_pass_it() {
        assert!(check_charge(Quota::TextBytes, 100, 60, 40).is_ok());
        let reached = check_charge(Quota::TextBytes, 100, 60, 41).unwrap_err();
        assert_eq!((reached.limit, reached.used), (100, 60));
        assert!(check_charge(Quota::WritesPerMonth, UNLIMITED, u64::MAX - 1, 5).is_ok());
    }

    #[test]
    fn the_new_quotas_default_to_unlimited_and_an_undeclared_profile_admits_no_write_and_keeps_its_rate()
     {
        let q = Quotas::default();
        assert_eq!((q.text_bytes, q.writes_per_month), (UNLIMITED, UNLIMITED));
        let closed = Quotas::writes_refused(120);
        for quota in Quota::ALL {
            if quota == Quota::RequestsPerMinute {
                assert_eq!(closed.get(quota), 120);
            } else {
                assert_eq!(closed.get(quota), 0);
                assert!(check_quota(quota, closed.get(quota), 0).is_err());
            }
        }
    }
}
