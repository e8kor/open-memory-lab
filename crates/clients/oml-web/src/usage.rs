//! The person's usage against their limits, drawn from the fields
//! `GET /v2/whoami`'s `person` and each row of `GET /v2/web/users` carry
//! (ADR-0071 d5, d6): the "Your usage" card of the Start and Keys pages, and
//! the meter cells of the users page.
//!
//! A meter is shown with its count, its limit where one holds, and a mark
//! at 80% of the limit and at the limit, by the rule the API's own
//! `warning` and `reached` states follow. The six usage routes of ADR-0071
//! d6 take a key and no session credential (ADR-0070 d2), so the page reads
//! none of them: the profile's name, the month and the recall count are not
//! on these fields and are not shown (#322).

use std::fmt::Write as _;

use oml_page::web::text;
use oml_sdk::{Limits, Person, Usage};

/// One meter as the page shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reading {
    pub name: &'static str,
    pub used: u64,
    /// `None` where no limit holds.
    pub limit: Option<u64>,
    /// Whether the count is bytes, shown in the unit a person reads.
    pub bytes: bool,
}

/// Where a meter stands against its limit, as the API's states name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    Ok,
    /// At 80% of the limit or more.
    Warning,
    /// At the limit: the next charge that would pass it is refused.
    Reached,
}

impl Reading {
    /// The mark: `Reached` at the limit, `Warning` from 80% of it, `Ok`
    /// with no limit (ADR-0071 d5).
    pub(crate) fn mark(&self) -> Mark {
        match self.limit {
            Some(limit) if self.used >= limit => Mark::Reached,
            Some(limit) if self.used.saturating_mul(10) >= limit.saturating_mul(8) => Mark::Warning,
            _ => Mark::Ok,
        }
    }

    fn count(&self, n: u64) -> String {
        if self.bytes { bytes(n) } else { n.to_string() }
    }

    /// The count and `limit` as the page prints them side by side. A byte
    /// count under its limit never prints equal to it: both take more
    /// digits until they differ, which the exact bytes always do.
    fn counts(&self, limit: u64) -> (String, String) {
        let mut pair = (self.count(self.used), self.count(limit));
        if self.bytes && self.used < limit {
            for digits in 2..=9 {
                if pair.0 != pair.1 {
                    break;
                }
                pair = (bytes_to(self.used, digits), bytes_to(limit, digits));
            }
        }
        pair
    }
}

/// A byte count in the unit a person reads: B, kB, MB or GB, decimal, to
/// one digit.
fn bytes(n: u64) -> String {
    bytes_to(n, 1)
}

/// [`bytes`] to `digits` digits after the point, as many as the unit has
/// at most. The count is cut, never rounded up, so it never prints as a
/// thousand of a unit or as more than it is.
fn bytes_to(n: u64, digits: u32) -> String {
    let (unit, exponent, name) = match n {
        0..1_000 => return format!("{n} B"),
        1_000..1_000_000 => (1_000_u64, 3, "kB"),
        1_000_000..1_000_000_000 => (1_000_000, 6, "MB"),
        _ => (1_000_000_000, 9, "GB"),
    };
    let digits = digits.clamp(1, exponent);
    let part = (n % unit) / 10_u64.pow(exponent - digits);
    format!(
        "{}.{part:0width$} {name}",
        n / unit,
        width = digits as usize
    )
}

/// The `low` and `high` of a meter's bar over a limit above zero, in
/// halves of a count, so the bar is in the region the chip names at every
/// count: a browser colours a count at or under `low` as fine, one up to
/// `high` as the middle and one past it as the worst, and the chip says
/// 80% from four fifths of the limit and limit at it. Each bound is half a
/// count under the count that changes the chip, so `low` is above the
/// bar's optimum, zero, even at a limit of one: a `low` equal to the
/// optimum makes a browser colour no part as the worst.
fn bar_bounds(limit: u64) -> (u64, u64) {
    let warning_from = limit.saturating_mul(8).div_ceil(10);
    (
        warning_from.saturating_mul(2).saturating_sub(1),
        limit.saturating_mul(2).saturating_sub(1),
    )
}

/// A bound in halves of a count as a `<meter>` attribute reads it.
fn halves(n: u64) -> String {
    if n.is_multiple_of(2) {
        (n / 2).to_string()
    } else {
        format!("{}.5", n / 2)
    }
}

/// The meters `usage` and `limits` carry, in the order the API lists them:
/// memories, text stored, writes this month, keys, connections. A count an
/// API before the meters does not serve is left out, never shown as zero.
/// `recalls`, which has no limit, is not on these fields.
pub(crate) fn readings(usage: &Usage, limits: &Limits) -> Vec<Reading> {
    let mut out = vec![Reading {
        name: "Memories",
        used: usage.memories,
        limit: limits.memories,
        bytes: false,
    }];
    if let Some(used) = usage.text_bytes {
        out.push(Reading {
            name: "Text stored",
            used,
            limit: limits.text_bytes,
            bytes: true,
        });
    }
    if let Some(used) = usage.writes {
        out.push(Reading {
            name: "Writes this month",
            used,
            limit: limits.writes_per_month,
            bytes: false,
        });
    }
    out.push(Reading {
        name: "Keys",
        used: usage.keys,
        limit: limits.keys,
        bytes: false,
    });
    out.push(Reading {
        name: "Connections",
        used: usage.connections,
        limit: limits.connections,
        bytes: false,
    });
    out
}

/// A meter's count with its limit and its mark, for a table cell: the
/// count alone where no limit is set.
pub(crate) fn cell(reading: &Reading) -> String {
    cell_where(reading, true)
}

/// [`cell`], with no mark where the limits do not hold for the person
/// (`under_quotas` false): the next charge past such a limit is accepted,
/// so a mark would say what the server does not do.
fn cell_where(reading: &Reading, holds: bool) -> String {
    let mark = match reading.mark() {
        Mark::Warning if holds => r#" <span class="oml-chip is-held">80%</span>"#,
        Mark::Reached if holds => r#" <span class="oml-chip is-rejected">limit</span>"#,
        _ => "",
    };
    match reading.limit {
        Some(limit) => {
            let (used, limit) = reading.counts(limit);
            format!(r#"<span class="mono">{used} / {limit}</span>{mark}"#)
        }
        None => format!(
            r#"<span class="mono">{}</span>"#,
            reading.count(reading.used)
        ),
    }
}

/// The five meters of a users-page row as table cells: memories, text
/// stored, writes this month, keys and connections. A meter the API does
/// not serve is a dash, never a zero. `holds` is the row's `under_quotas`:
/// a person the limits do not hold for is shown the counts and the limits
/// with no mark, as `oml doctor` skips them.
pub(crate) fn cells(usage: &Usage, limits: &Limits, holds: bool) -> [String; 5] {
    let all = readings(usage, limits);
    [
        "Memories",
        "Text stored",
        "Writes this month",
        "Keys",
        "Connections",
    ]
    .map(|name| {
        all.iter()
            .find(|r| r.name == name)
            .map_or_else(|| "—".to_owned(), |r| cell_where(r, holds))
    })
}

/// The request rate per key, a dash where it is unlimited.
pub(crate) fn rate(limit: Option<u64>) -> String {
    limit.map_or_else(|| "—".to_owned(), |n| format!("{n} / min"))
}

/// The card's heading, as the pages' sections draw one.
fn head(desc: &str) -> String {
    format!(
        r#"<header class="oml-section-head"><div class="top"><h2>Your usage</h2></div><p class="desc">{}</p></header>"#,
        text(desc)
    )
}

/// The "Your usage" card of a person under limits, or `None` where the API
/// serves them no counts: a person outside the sign-up tenant whom no
/// profile limits has no usage on `person`, and a card of zeros would say
/// what the API did not.
pub(crate) fn card(person: &Person) -> Option<String> {
    let (usage, limits) = (person.usage?, person.quotas?);
    let readings = readings(&usage, &limits);
    let limited = readings.iter().any(|r| r.limit.is_some());
    // The users page's quota form raises four limits; text stored and
    // writes are the profile's (ADR-0071 d5), so the card says so where one
    // of the two is limited.
    let by_profile = readings
        .iter()
        .any(|r| r.limit.is_some() && matches!(r.name, "Text stored" | "Writes this month"));
    let desc = match (limited, by_profile) {
        (true, true) => {
            "Each count against its limit; writes count this UTC month. A mark shows at 80% of a limit and at the limit. An administrator can raise a limit for you. Text stored and writes follow the profile of your account, which an administrator changes with an admin key."
        }
        (true, false) => {
            "Each count against its limit; writes count this UTC month. A mark shows at 80% of a limit and at the limit. An administrator can raise a limit for you."
        }
        (false, _) => "No limit holds for your account; these are counts.",
    };
    let mut out = format!(r#"<section class="card" id="usage">{}"#, head(desc));
    out.push_str("<table><tr><th>meter</th><th>used</th><th></th></tr>");
    for r in &readings {
        let bar = match r.limit {
            Some(limit) if limit > 0 => {
                let (low, high) = bar_bounds(limit);
                let (used, of) = r.counts(limit);
                format!(
                    r#"<meter min="0" max="{limit}" low="{}" high="{}" optimum="0" value="{}" aria-label="{}: {used} of {of}"></meter>"#,
                    halves(low),
                    halves(high),
                    r.used.min(limit),
                    r.name,
                )
            }
            _ => String::new(),
        };
        let _ = write!(
            out,
            "<tr><td>{}</td><td>{}</td><td>{bar}</td></tr>",
            r.name,
            cell(r)
        );
    }
    let _ = write!(
        out,
        r#"<tr><td>Requests, per key</td><td class="mono">{}</td><td></td></tr></table></section>"#,
        rate(limits.requests_per_minute)
    );
    Some(out)
}

/// The card where the person's usage could not be read: the section with
/// the request that failed, as a partial page marks a part it lacks.
pub(crate) fn unavailable(request_id: &str) -> String {
    format!(
        r#"<section class="card" id="usage">{}<p class="error" role="status">Your usage is unavailable (request <code>{}</code>).</p></section>"#,
        head("Each count against its limit."),
        text(request_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(used: u64, limit: Option<u64>) -> Reading {
        Reading {
            name: "Memories",
            used,
            limit,
            bytes: false,
        }
    }

    #[test]
    fn a_meter_is_marked_at_eighty_percent_of_its_limit_and_at_it_and_never_without_one() {
        assert_eq!(reading(799, Some(1000)).mark(), Mark::Ok);
        assert_eq!(reading(800, Some(1000)).mark(), Mark::Warning);
        assert_eq!(reading(999, Some(1000)).mark(), Mark::Warning);
        assert_eq!(reading(1000, Some(1000)).mark(), Mark::Reached);
        assert_eq!(reading(1001, Some(1000)).mark(), Mark::Reached);
        assert_eq!(reading(u64::MAX, None).mark(), Mark::Ok);
        // A limit of zero refuses every charge, so it is reached at zero.
        assert_eq!(reading(0, Some(0)).mark(), Mark::Reached);
    }

    /// The region a browser colours a `<meter>` whose minimum and optimum
    /// are zero (HTML, the meter element's gauge regions). An optimum below
    /// `low` makes the part up to `low` the best, the part up to `high` the
    /// middle and the rest the worst. An optimum that is `low` itself makes
    /// `low` to `high` the best and every other part the middle: no part is
    /// then the worst, whatever the count.
    fn region(value: u64, low: u64, high: u64) -> Mark {
        let optimum = 0;
        if optimum >= low {
            return if value >= low && value <= high {
                Mark::Ok
            } else {
                Mark::Warning
            };
        }
        if value <= low {
            Mark::Ok
        } else if value <= high {
            Mark::Warning
        } else {
            Mark::Reached
        }
    }

    #[test]
    fn the_bar_is_in_the_region_of_the_chip_at_every_count() {
        for limit in [1, 2, 4, 5, 9, 10, 11, 100, 1000] {
            // The bounds are in halves of a count, so the count is too.
            let (low, high) = bar_bounds(limit);
            assert!(
                0 < low && low <= high && high < 2 * limit,
                "{limit}: {low} {high}"
            );
            for used in 0..=limit + 1 {
                assert_eq!(
                    region(2 * used.min(limit), low, high),
                    reading(used, Some(limit)).mark(),
                    "{used} of {limit}"
                );
            }
        }
        // At exactly 80% the chip says so, and the bar is past its low mark.
        assert_eq!(bar_bounds(1000), (1599, 1999));
        assert_eq!(
            (halves(1599), halves(1999)),
            ("799.5".to_owned(), "999.5".to_owned())
        );
        // A limit of one: the bar is fine at none and the worst at one.
        assert_eq!(bar_bounds(1), (1, 1));
        assert_eq!(halves(1), "0.5");
    }

    #[test]
    fn a_byte_count_never_prints_as_a_thousand_of_a_unit() {
        assert_eq!(bytes(999_949), "999.9 kB");
        assert_eq!(bytes(999_999), "999.9 kB");
        assert_eq!(bytes(999_999_999), "999.9 MB");
        assert_eq!(bytes(1_000_000), "1.0 MB");
    }

    #[test]
    fn a_byte_count_under_its_limit_never_prints_equal_to_it() {
        let pair = |used, limit| {
            let r = Reading {
                name: "Text stored",
                used,
                limit: Some(limit),
                bytes: true,
            };
            r.counts(limit)
        };
        // One byte under: the count is cut, not rounded up to the limit.
        assert_eq!(
            pair(1_999_999, 2_000_000),
            ("1.9 MB".to_owned(), "2.0 MB".to_owned())
        );
        // Both in the same tenth: more digits, until they differ.
        assert_eq!(
            pair(2_040_000, 2_050_000),
            ("2.04 MB".to_owned(), "2.05 MB".to_owned())
        );
        assert_eq!(
            pair(2_000_000, 2_000_001),
            ("2.000000 MB".to_owned(), "2.000001 MB".to_owned())
        );
        // At the limit and past it they may print alike: the chip says limit.
        assert_eq!(
            pair(2_000_000, 2_000_000),
            ("2.0 MB".to_owned(), "2.0 MB".to_owned())
        );
        assert_eq!(pair(900, 1_000), ("900 B".to_owned(), "1.0 kB".to_owned()));
    }

    #[test]
    fn a_byte_count_is_shown_in_the_unit_a_person_reads() {
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1_000), "1.0 kB");
        assert_eq!(bytes(204_800), "204.8 kB");
        assert_eq!(bytes(5_000_000), "5.0 MB");
        assert_eq!(bytes(2_500_000_000), "2.5 GB");
    }
}
