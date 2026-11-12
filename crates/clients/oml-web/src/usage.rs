//! The person's usage against their limits, drawn from the fields
//! `GET /v2/whoami`'s `person` and each row of `GET /v2/web/users` carry
//! (ADR-0071 d5, d6): the "Your usage" card of the Start and Keys pages, and
//! the meter cells of the users page.
//!
//! A meter is shown with its count, its limit where one holds, and a mark
//! at 80% of the limit and at the limit, by the rule the API's own
//! `warning` and `reached` states follow. The five usage routes take a key
//! and no session credential (ADR-0070 d2), so the page reads none of them:
//! the profile's name, the month and the recall count are not on these
//! fields and are not shown.

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
}

/// A byte count in the unit a person reads: B, kB, MB or GB, decimal.
fn bytes(n: u64) -> String {
    match n {
        0..1_000 => format!("{n} B"),
        1_000..1_000_000 => format!("{:.1} kB", n as f64 / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} MB", n as f64 / 1e6),
        _ => format!("{:.1} GB", n as f64 / 1e9),
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
/// count alone where no limit holds.
pub(crate) fn cell(reading: &Reading) -> String {
    let mark = match reading.mark() {
        Mark::Ok => "",
        Mark::Warning => r#" <span class="oml-chip is-held">80%</span>"#,
        Mark::Reached => r#" <span class="oml-chip is-rejected">limit</span>"#,
    };
    match reading.limit {
        Some(limit) => format!(
            r#"<span class="mono">{} / {}</span>{mark}"#,
            reading.count(reading.used),
            reading.count(limit)
        ),
        None => format!(
            r#"<span class="mono">{}</span>"#,
            reading.count(reading.used)
        ),
    }
}

/// The five meters of a users-page row as table cells: memories, text
/// stored, writes this month, keys and connections. A meter the API does
/// not serve is a dash, never a zero.
pub(crate) fn cells(usage: &Usage, limits: &Limits) -> [String; 5] {
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
            .map_or_else(|| "—".to_owned(), cell)
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
    let desc = if limited {
        "Each count against its limit; writes count this UTC month. A mark shows at 80% of a limit and at the limit. An administrator can raise a limit for you."
    } else {
        "No limit holds for your account; these are counts."
    };
    let mut out = format!(r#"<section class="card" id="usage">{}"#, head(desc));
    out.push_str("<table><tr><th>meter</th><th>used</th><th></th></tr>");
    for r in &readings {
        let bar = match r.limit {
            Some(limit) if limit > 0 => format!(
                r#"<meter min="0" max="{limit}" low="{low}" high="{low}" optimum="0" value="{}" aria-label="{}: {} of {}"></meter>"#,
                r.used.min(limit),
                r.name,
                r.count(r.used),
                r.count(limit),
                low = limit.saturating_mul(8) / 10,
            ),
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

    #[test]
    fn a_byte_count_is_shown_in_the_unit_a_person_reads() {
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1_000), "1.0 kB");
        assert_eq!(bytes(204_800), "204.8 kB");
        assert_eq!(bytes(5_000_000), "5.0 MB");
        assert_eq!(bytes(2_500_000_000), "2.5 GB");
    }
}
