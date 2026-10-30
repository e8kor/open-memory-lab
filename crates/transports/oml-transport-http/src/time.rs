//! Wall-clock `EventTime` values: the fixed 27-byte signed form of
//! chain-format.md, derived from the system clock without a calendar crate.

use std::time::{SystemTime, UNIX_EPOCH};

use oml_app::domain::EventTime;

/// Now, as the fixed-width `YYYY-MM-DDTHH:MM:SS.ffffffZ` form.
#[must_use]
pub fn system_event_time() -> EventTime {
    system_event_time_plus(0)
}

/// Now plus `offset_secs` — the transport's expiry stamps.
#[must_use]
pub fn system_event_time_plus(offset_secs: u64) -> EventTime {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    event_time_from_unix(now.as_secs() + offset_secs, now.subsec_micros())
}

fn event_time_from_unix(secs: u64, micros: u32) -> EventTime {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX / 2);
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let s = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{micros:06}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    );
    EventTime::parse(&s).expect("a constructed civil timestamp is well-formed")
}

/// Days-since-epoch to civil date (Howard Hinnant's `civil_from_days`
/// algorithm; exact over the proleptic Gregorian calendar).
#[allow(
    clippy::cast_sign_loss,
    reason = "doe/yoe/doy are provably non-negative in this algorithm"
)]
fn civil_from_days(z: i64) -> (i64, u64, u64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = i64::try_from(yoe).unwrap_or_default() + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates_round() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_357), (2022, 12, 31));
        assert_eq!(civil_from_days(20_756), (2026, 10, 30));
    }

    #[test]
    fn now_parses_as_event_time() {
        let t = system_event_time();
        assert_eq!(t.as_str().len(), 27);
    }
}
