//! Per-account usage metering's pure rules (ADR-0071): the six meters, the
//! quota each is held to, a meter's state against its limit, and the UTC
//! month a flow counts in. The counters and the ledger are the store's.

use crate::envelope::EventTime;
use crate::signup::{Quota, near_quota};

/// One of the six meters (ADR-0071 d2), in the order the endpoints and the
/// pages list them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Meter {
    Memories,
    TextBytes,
    Writes,
    Recalls,
    Keys,
    Connections,
}

impl Meter {
    pub const ALL: [Meter; 6] = [
        Meter::Memories,
        Meter::TextBytes,
        Meter::Writes,
        Meter::Recalls,
        Meter::Keys,
        Meter::Connections,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Meter::Memories => "memories",
            Meter::TextBytes => "text_bytes",
            Meter::Writes => "writes",
            Meter::Recalls => "recalls",
            Meter::Keys => "keys",
            Meter::Connections => "connections",
        }
    }

    /// The quota that limits this meter; `recalls` has none, since reading
    /// never blocks (ADR-0071 d5).
    #[must_use]
    pub fn quota(self) -> Option<Quota> {
        match self {
            Meter::Memories => Some(Quota::Memories),
            Meter::TextBytes => Some(Quota::TextBytes),
            Meter::Writes => Some(Quota::WritesPerMonth),
            Meter::Recalls => None,
            Meter::Keys => Some(Quota::Keys),
            Meter::Connections => Some(Quota::Connections),
        }
    }

    /// Whether the meter counts the current month alone (a flow) or what is
    /// held now (a stock).
    #[must_use]
    pub fn is_flow(self) -> bool {
        matches!(self, Meter::Writes | Meter::Recalls)
    }
}

/// Where a meter stands against its limit (ADR-0071 d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterState {
    Ok,
    /// At 80% of the limit or more.
    Warning,
    /// At the limit: the next charge that would pass it is refused.
    Reached,
}

impl MeterState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MeterState::Ok => "ok",
            MeterState::Warning => "warning",
            MeterState::Reached => "reached",
        }
    }
}

/// The state of `used` against `limit`; no limit is always `ok`.
#[must_use]
pub fn meter_state(limit: Option<u64>, used: u64) -> MeterState {
    match limit {
        Some(limit) if used >= limit => MeterState::Reached,
        Some(limit) if near_quota(limit, used) => MeterState::Warning,
        _ => MeterState::Ok,
    }
}

/// `used` as a whole percentage of `limit`, `None` without a limit or with
/// a limit of zero.
#[must_use]
pub fn percent(limit: Option<u64>, used: u64) -> Option<u64> {
    let limit = limit.filter(|l| *l > 0 && *l != crate::signup::UNLIMITED)?;
    let pct = u128::from(used) * 100 / u128::from(limit);
    Some(u64::try_from(pct).unwrap_or(u64::MAX))
}

/// A UTC calendar month, `YYYY-MM`: the period a flow counts in and the
/// ledger export's `period` (ADR-0071 d2, d6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Month {
    year: u16,
    month: u8,
}

impl Month {
    /// `YYYY-MM`, the year from 1970 to 9999; anything else is `None`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let b = value.as_bytes();
        if b.len() != 7
            || b[4] != b'-'
            || !b
                .iter()
                .enumerate()
                .all(|(i, c)| i == 4 || c.is_ascii_digit())
        {
            return None;
        }
        let year: u16 = value[..4].parse().ok()?;
        let month: u8 = value[5..].parse().ok()?;
        ((1970..=9999).contains(&year) && (1..=12).contains(&month)).then_some(Self { year, month })
    }

    /// The month `at` falls in.
    #[must_use]
    pub fn of(at: &EventTime) -> Self {
        Self::parse(&at.as_str()[..7]).expect("an event time starts with its YYYY-MM")
    }

    /// The month after this one.
    #[must_use]
    pub fn next(self) -> Self {
        if self.month == 12 {
            Self {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Self {
                year: self.year,
                month: self.month + 1,
            }
        }
    }

    /// `YYYY-MM`.
    #[must_use]
    pub fn as_string(self) -> String {
        format!("{:04}-{:02}", self.year, self.month)
    }

    /// The month's first day, `YYYY-MM-01`.
    #[must_use]
    pub fn first_day(self) -> String {
        format!("{}-01", self.as_string())
    }
}

/// The UTC day `at` falls in, `YYYY-MM-DD`.
#[must_use]
pub fn day_of(at: &EventTime) -> &str {
    &at.as_str()[..10]
}

/// How long the ledger keeps a day, in months (ADR-0071 d4).
pub const LEDGER_RETENTION_MONTHS: u32 = 25;

/// How long a day's flows are kept once rolled up, in days (ADR-0071 d4).
pub const ACTIVITY_RETENTION_DAYS: u32 = 62;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signup::UNLIMITED;

    #[test]
    fn every_meter_but_recalls_has_a_quota() {
        let limited: Vec<_> = Meter::ALL.iter().filter(|m| m.quota().is_some()).collect();
        assert_eq!(limited.len(), 5);
        assert_eq!(Meter::Recalls.quota(), None);
        assert!(Meter::Writes.is_flow() && Meter::Recalls.is_flow());
        assert!(!Meter::Memories.is_flow());
    }

    #[test]
    fn a_meter_warns_at_eighty_percent_and_is_reached_at_its_limit() {
        assert_eq!(meter_state(Some(1000), 799), MeterState::Ok);
        assert_eq!(meter_state(Some(1000), 800), MeterState::Warning);
        assert_eq!(meter_state(Some(1000), 1000), MeterState::Reached);
        assert_eq!(meter_state(None, 10_000), MeterState::Ok);
        assert_eq!(meter_state(Some(0), 0), MeterState::Reached);
        assert_eq!(meter_state(Some(UNLIMITED), 5), MeterState::Ok);
    }

    #[test]
    fn percent_is_whole_and_absent_without_a_limit() {
        assert_eq!(percent(Some(1000), 812), Some(81));
        assert_eq!(percent(None, 5), None);
        assert_eq!(percent(Some(UNLIMITED), 5), None);
        assert_eq!(percent(Some(0), 0), None);
    }

    #[test]
    fn a_month_parses_only_yyyy_mm_and_rolls_over_the_year() {
        let m = Month::parse("2026-12").expect("a month");
        assert_eq!(m.next().as_string(), "2027-01");
        assert_eq!(m.first_day(), "2026-12-01");
        for bad in [
            "2026-13", "2026-1", "26-11", "2026/11", "2026-00", "abcd-ef",
        ] {
            assert_eq!(Month::parse(bad), None, "{bad}");
        }
        let at = EventTime::parse("2026-11-06T10:00:00.000000Z").expect("a time");
        assert_eq!(Month::of(&at).as_string(), "2026-11");
        assert_eq!(day_of(&at), "2026-11-06");
    }
}
