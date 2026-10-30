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

/// An RFC 3339 `date-time` (openapi.yaml `format: date-time`) as the
/// fixed-width UTC instant it names: the offset applied and a fraction of
/// up to six digits padded. Finer than a microsecond, or not RFC 3339, is
/// `None`. An instant outside 1970-01-01 to 9999-12-31 is clamped to that
/// range, which holds every stored row.
#[must_use]
pub fn parse_date_time(s: &str) -> Option<EventTime> {
    let b = s.as_bytes();
    if !s.is_ascii() || b.len() < 20 || !matches!(b[10], b'T' | b't') {
        return None;
    }
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let (fraction, zone) = if b[19] == b'.' {
        let n = digits(20);
        if n == 0 || n > 6 {
            return None;
        }
        (&s[20..20 + n], &b[20 + n..])
    } else {
        ("", &b[19..])
    };
    let two = |hi: u8, lo: u8| i64::from(hi - b'0') * 10 + i64::from(lo - b'0');
    let offset_secs = match zone {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2]
            if [h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit())
                && two(*h1, *h2) <= 23
                && two(*m1, *m2) <= 59 =>
        {
            let secs = (two(*h1, *h2) * 60 + two(*m1, *m2)) * 60;
            if *sign == b'-' { -secs } else { secs }
        }
        _ => return None,
    };
    let local = EventTime::parse(&format!("{}T{}.{fraction:0<6}Z", &s[..10], &s[11..19])).ok()?;
    let last = EventTime::parse("9999-12-31T23:59:59.999999Z")
        .expect("the last fixed-width instant is well-formed")
        .unix_micros();
    let utc = u64::try_from((local.unix_micros() - offset_secs * 1_000_000).clamp(0, last))
        .expect("clamped to a non-negative range");
    Some(event_time_from_unix(
        utc / 1_000_000,
        u32::try_from(utc % 1_000_000).expect("below a million"),
    ))
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
    fn a_date_time_parameter_names_its_utc_instant() {
        let utc = |s: &str| parse_date_time(s).map(|t| t.as_str().to_owned());
        assert_eq!(
            utc("2026-10-27T13:58:20Z").as_deref(),
            Some("2026-10-27T13:58:20.000000Z")
        );
        assert_eq!(
            utc("2026-10-27t13:58:20.25z").as_deref(),
            Some("2026-10-27T13:58:20.250000Z")
        );
        assert_eq!(
            utc("2026-10-27T01:30:00.000001+02:00").as_deref(),
            Some("2026-10-26T23:30:00.000001Z"),
            "the offset is applied across midnight"
        );
        assert_eq!(
            utc("2024-02-28T23:00:00-01:30").as_deref(),
            Some("2024-02-29T00:30:00.000000Z"),
            "a leap day"
        );
        assert_eq!(
            utc("1969-12-31T23:59:59Z").as_deref(),
            Some("1970-01-01T00:00:00.000000Z")
        );
        for bad in [
            "2026-10-27",
            "2026-10-27T13:58:20",
            "2026-10-27T13:58:20.Z",
            "2026-10-27T13:58:20.0000001Z",
            "2026-10-27T13:58:20+2:00",
            "2026-10-27T13:58:20+24:00",
            "2026-13-27T13:58:20Z",
            "2026-10-27 13:58:20Z",
            "2026-10-27T13:58:20\u{e9}",
        ] {
            assert_eq!(utc(bad), None, "{bad}");
        }
    }

    #[test]
    fn now_parses_as_event_time() {
        let t = system_event_time();
        assert_eq!(t.as_str().len(), 27);
    }
}
