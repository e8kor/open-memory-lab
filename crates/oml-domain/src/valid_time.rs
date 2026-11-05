//! Valid time, the query's date window and expiry — pure
//! functions over values the app has already read (proposed ADR-0059; it
//! narrows proposed ADR-0036 d2).
//!
//! - **Valid time** (ADR-0059 d2): a version's optional ends `valid_from`
//!   and `valid_until`, each a period (`YYYY`, `YYYY-MM`, `YYYY-MM-DD`) or an
//!   RFC 3339 instant with an offset. A period end names the whole period:
//!   `valid_from` is its first instant, `valid_until` the instant after its
//!   last, so from `2015` until `2015` is the year. The interval is
//!   half-open, an absent end open.
//! - **Expiry** (d3): a version whose end is at or before the request's now.
//! - **The window** (d6, rule [`WHEN_RULE`]): at most one date expression
//!   read from a query by rule, anchored to the request's now.

use crate::model::MemoryKind;

/// The date parser's rule id (ADR-0059 d6).
pub const WHEN_RULE: &str = "oml-when/1";

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// Why a valid end is refused; each is `request.validation` naming the
/// field (ADR-0059 d2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ValidTimeError {
    /// Not `YYYY`, `YYYY-MM`, `YYYY-MM-DD` or an RFC 3339 `date-time` with
    /// an offset of at most microsecond precision.
    #[error("{field} must be YYYY, YYYY-MM, YYYY-MM-DD or an RFC 3339 date-time with an offset")]
    NotADate { field: &'static str },
    /// Both ends sent and the start is not before the end.
    #[error("valid_until must end after valid_from begins")]
    EndsBeforeItBegins,
    /// Valid time sent on a kind, or a retirement, that takes none.
    #[error("{field} is refused on {what}: only memory and derived_fact take valid time")]
    NotTaken {
        field: &'static str,
        what: &'static str,
    },
}

impl ValidTimeError {
    /// The request member `details.field` names, as a JSON pointer.
    #[must_use]
    pub fn field(&self) -> &'static str {
        match self {
            Self::NotADate { field } | Self::NotTaken { field, .. } => field,
            Self::EndsBeforeItBegins => "/valid_until",
        }
    }
}

/// One declared end: its normalized signed text and its bound in
/// microseconds since the epoch.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValidEnd {
    text: String,
    micros: i64,
}

impl ValidEnd {
    /// The normalized form: a period as sent, an instant in the fixed-width
    /// UTC form of `at`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The bound: a start's first instant, or an end's exclusive instant.
    #[must_use]
    pub fn unix_micros(&self) -> i64 {
        self.micros
    }

    /// An end as stored: its text and bound, trusted as written by
    /// [`ValidTime::parse`].
    #[must_use]
    pub fn from_stored(text: String, micros: i64) -> Self {
        Self { text, micros }
    }
}

/// A version's valid time: two optional ends (ADR-0059 d2). `Default` is
/// open on both sides, every version that declares none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ValidTime {
    pub from: Option<ValidEnd>,
    pub until: Option<ValidEnd>,
}

impl ValidTime {
    /// Parses the two ends as sent.
    ///
    /// # Errors
    ///
    /// [`ValidTimeError::NotADate`] for an end in no accepted form and
    /// [`ValidTimeError::EndsBeforeItBegins`] when both are sent and the
    /// interval is empty.
    pub fn parse(from: Option<&str>, until: Option<&str>) -> Result<Self, ValidTimeError> {
        let from = from
            .map(|s| {
                parse_period(s)
                    .map(|p| ValidEnd {
                        text: p.text,
                        micros: p.start,
                    })
                    .ok_or(ValidTimeError::NotADate {
                        field: "/valid_from",
                    })
            })
            .transpose()?;
        let until = until
            .map(|s| {
                parse_period(s)
                    .map(|p| ValidEnd {
                        text: p.text,
                        micros: p.end,
                    })
                    .ok_or(ValidTimeError::NotADate {
                        field: "/valid_until",
                    })
            })
            .transpose()?;
        if let (Some(f), Some(u)) = (&from, &until)
            && f.micros >= u.micros
        {
            return Err(ValidTimeError::EndsBeforeItBegins);
        }
        Ok(Self { from, until })
    }

    /// Whether neither end is declared.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.from.is_none() && self.until.is_none()
    }

    /// Refuses valid time on a kind that takes none, or on a retirement
    /// (ADR-0059 d2): only `memory` and `derived_fact` take it.
    ///
    /// # Errors
    ///
    /// [`ValidTimeError::NotTaken`] naming the first end sent.
    pub fn check_kind(&self, kind: MemoryKind, retirement: bool) -> Result<(), ValidTimeError> {
        if self.is_open() {
            return Ok(());
        }
        let field = if self.from.is_some() {
            "/valid_from"
        } else {
            "/valid_until"
        };
        if retirement {
            return Err(ValidTimeError::NotTaken {
                field,
                what: "a retirement",
            });
        }
        match kind {
            MemoryKind::Memory | MemoryKind::DerivedFact => Ok(()),
            other => Err(ValidTimeError::NotTaken {
                field,
                what: kind_phrase(other),
            }),
        }
    }

    /// Whether the version has ended at `now_us`: its end at or before now
    /// (ADR-0059 d3).
    #[must_use]
    pub fn expired_at(&self, now_us: i64) -> bool {
        self.until.as_ref().is_some_and(|u| u.micros <= now_us)
    }

    /// Whether a declared start exists and the interval overlaps `window`
    /// (ADR-0059 d7): a row with no start never matches.
    #[must_use]
    pub fn overlaps(&self, window: &Window) -> bool {
        let Some(from) = &self.from else {
            return false;
        };
        let before_end = window.until_us.is_none_or(|end| from.micros < end);
        let after_start = match (&self.until, window.from_us) {
            (Some(u), Some(start)) => u.micros > start,
            _ => true,
        };
        before_end && after_start
    }
}

fn kind_phrase(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Steering => "kind steering",
        MemoryKind::Guardrail => "kind guardrail",
        MemoryKind::Term => "kind term",
        MemoryKind::TermMapping => "kind term_mapping",
        MemoryKind::VocabRelease => "kind vocab_release",
        MemoryKind::Memory | MemoryKind::DerivedFact => "this kind",
    }
}

/// A parsed period or instant: its normalized text and its half-open
/// bounds.
struct Period {
    text: String,
    start: i64,
    end: i64,
}

/// Parses `YYYY`, `YYYY-MM`, `YYYY-MM-DD` or an RFC 3339 instant.
fn parse_period(s: &str) -> Option<Period> {
    let b = s.as_bytes();
    if !s.is_ascii() || b.len() < 4 {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        b.get(from..to)?.iter().try_fold(0_i64, |acc, d| {
            d.is_ascii_digit().then(|| acc * 10 + i64::from(d - b'0'))
        })
    };
    let year = num(0, 4)?;
    if b.len() == 4 {
        let start = days_from_civil(year, 1, 1) * MICROS_PER_DAY;
        let end = days_from_civil(year + 1, 1, 1) * MICROS_PER_DAY;
        return Some(Period {
            text: s.to_owned(),
            start,
            end,
        });
    }
    if b[4] != b'-' || b.len() < 7 {
        return None;
    }
    let month = num(5, 7)?;
    if !(1..=12).contains(&month) {
        return None;
    }
    if b.len() == 7 {
        let start = days_from_civil(year, month, 1) * MICROS_PER_DAY;
        let (ny, nm) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        return Some(Period {
            text: s.to_owned(),
            start,
            end: days_from_civil(ny, nm, 1) * MICROS_PER_DAY,
        });
    }
    if b[7] != b'-' || b.len() < 10 {
        return None;
    }
    let day = num(8, 10)?;
    if day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if b.len() == 10 {
        return Some(Period {
            text: s.to_owned(),
            start: days * MICROS_PER_DAY,
            end: (days + 1) * MICROS_PER_DAY,
        });
    }
    let micros = parse_instant_tail(b, days)?;
    Some(Period {
        text: fixed_width(micros),
        start: micros,
        end: micros,
    })
}

/// The time and offset of an RFC 3339 `date-time` after its date.
fn parse_instant_tail(b: &[u8], days: i64) -> Option<i64> {
    if b.len() < 20 || !matches!(b[10], b'T' | b't') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        b.get(from..to)?.iter().try_fold(0_i64, |acc, d| {
            d.is_ascii_digit().then(|| acc * 10 + i64::from(d - b'0'))
        })
    };
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let (fraction, zone) = if b[19] == b'.' {
        let n = b[20..].iter().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 || n > 6 {
            return None;
        }
        let padded = num(20, 20 + n)? * 10_i64.pow(u32::try_from(6 - n).ok()?);
        (padded, &b[20 + n..])
    } else {
        (0, &b[19..])
    };
    let offset_minutes = match zone {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] => {
            if ![h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let h = i64::from(h1 - b'0') * 10 + i64::from(h2 - b'0');
            let m = i64::from(m1 - b'0') * 10 + i64::from(m2 - b'0');
            if h > 23 || m > 59 {
                return None;
            }
            if *sign == b'-' {
                -(h * 60 + m)
            } else {
                h * 60 + m
            }
        }
        _ => return None,
    };
    let local = days * MICROS_PER_DAY + ((hour * 60 + minute) * 60 + second) * 1_000_000 + fraction;
    let micros = local - offset_minutes * 60_000_000;
    let first = days_from_civil(0, 1, 1) * MICROS_PER_DAY;
    let last = (days_from_civil(9999, 12, 31) + 1) * MICROS_PER_DAY - 1;
    (first..=last).contains(&micros).then_some(micros)
}

/// The request's now as `now` names it (ADR-0059 d5): a period's first
/// instant, or the instant.
#[must_use]
pub fn parse_now(s: &str) -> Option<i64> {
    parse_period(s).map(|p| p.start)
}

/// The 27-byte `YYYY-MM-DDTHH:MM:SS.ffffffZ` form of an instant in range.
#[must_use]
pub fn fixed_width(micros: i64) -> String {
    let (days, rem) = (
        micros.div_euclid(MICROS_PER_DAY),
        micros.rem_euclid(MICROS_PER_DAY),
    );
    let (year, month, day) = civil_from_days(days);
    let secs = rem / 1_000_000;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z",
        secs / 3_600,
        (secs % 3_600) / 60,
        secs % 60,
        rem % 1_000_000
    )
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

/// Howard Hinnant's `days_from_civil`, exact over the proleptic Gregorian
/// calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Howard Hinnant's `civil_from_days`, the inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// The date window a query names (ADR-0059 d6): half-open, either side
/// open, with the words it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub from_us: Option<i64>,
    pub until_us: Option<i64>,
    pub phrase: String,
}

/// A closed span in days: [start, end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: i64,
    end: i64,
}

const MONTHS: [(&str, i64); 21] = [
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
    ("jan", 1),
    ("feb", 2),
    ("apr", 4),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("sept", 9),
    ("oct", 10),
];

const MONTHS_MORE: [(&str, i64); 3] = [("nov", 11), ("dec", 12), ("mar", 3)];

const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// The words that make a bare month name a date (ADR-0059 d6).
const MONTH_CUES: [&str; 12] = [
    "in", "during", "since", "until", "till", "before", "after", "of", "last", "this", "early",
    "late",
];

const NUMBER_WORDS: [&str; 13] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve",
];

fn month_of(word: &str) -> Option<(i64, bool)> {
    let word = word.trim_end_matches('.');
    MONTHS
        .iter()
        .chain(MONTHS_MORE.iter())
        .find(|(name, _)| *name == word)
        // An abbreviation counts only beside a number; a full name alone
        // counts after a cue word.
        .map(|&(name, m)| (m, MONTHS[..12].iter().any(|(full, _)| *full == name)))
}

fn weekday_of(word: &str) -> Option<i64> {
    WEEKDAYS
        .iter()
        .position(|d| *d == word)
        .and_then(|i| i64::try_from(i).ok())
}

fn count_of(word: &str) -> Option<i64> {
    if word == "a" || word == "an" {
        return Some(1);
    }
    if let Some(i) = NUMBER_WORDS.iter().position(|w| *w == word) {
        return i64::try_from(i).ok();
    }
    if word.len() <= 3 && word.bytes().all(|c| c.is_ascii_digit()) {
        return word.parse().ok();
    }
    None
}

fn year_of(word: &str) -> Option<i64> {
    if word.len() == 4 && word.bytes().all(|c| c.is_ascii_digit()) {
        let y: i64 = word.parse().ok()?;
        return (1900..=2100).contains(&y).then_some(y);
    }
    None
}

fn day_number(word: &str) -> Option<i64> {
    let digits = word
        .trim_end_matches("st")
        .trim_end_matches("nd")
        .trim_end_matches("rd")
        .trim_end_matches("th");
    if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let d: i64 = digits.parse().ok()?;
    (1..=31).contains(&d).then_some(d)
}

fn month_span(year: i64, month: i64) -> Span {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    Span {
        start: days_from_civil(year, month, 1),
        end: days_from_civil(ny, nm, 1),
    }
}

fn year_span(year: i64) -> Span {
    Span {
        start: days_from_civil(year, 1, 1),
        end: days_from_civil(year + 1, 1, 1),
    }
}

fn day_span(day: i64) -> Span {
    Span {
        start: day,
        end: day + 1,
    }
}

fn quarter_span(year: i64, q: i64) -> Span {
    let first = (q - 1) * 3 + 1;
    let end = if q == 4 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, first + 3, 1)
    };
    Span {
        start: days_from_civil(year, first, 1),
        end,
    }
}

/// Monday of the week holding `day` (ISO weeks; 1970-01-01 was a
/// Thursday).
fn week_start(day: i64) -> i64 {
    day - (day + 3).rem_euclid(7)
}

fn shift_months(year: i64, month: i64, by: i64) -> (i64, i64) {
    let index = year * 12 + (month - 1) + by;
    (index.div_euclid(12), index.rem_euclid(12) + 1)
}

/// The words of a query as the parser reads them: lower-case, split on
/// whitespace, trimmed of punctuation around them.
fn words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                .trim_matches('-')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Reads the date window `query` names, anchored to `now_us` (ADR-0059
/// d6, rule [`WHEN_RULE`]): the first expression from the left, the longer
/// forms first; `None` when the query names no date.
#[must_use]
pub fn parse_window(query: &str, now_us: i64) -> Option<Window> {
    let w = words(query);
    let today = now_us.div_euclid(MICROS_PER_DAY);
    for i in 0..w.len() {
        if let Some((span_from, span_until, used)) = range_at(&w, i, today) {
            return Some(Window {
                from_us: span_from.map(|d| d * MICROS_PER_DAY),
                until_us: span_until.map(|d| d * MICROS_PER_DAY),
                phrase: w[i..i + used].join(" "),
            });
        }
    }
    None
}

type Range = (Option<i64>, Option<i64>, usize);

/// A range or a single expression starting at word `i`.
fn range_at(w: &[String], i: usize, today: i64) -> Option<Range> {
    let head = w[i].as_str();
    match head {
        "between" | "from" => {
            let (a, used_a) = expression_at(w, i + 1, today, true)?;
            let joiner = w.get(i + 1 + used_a)?.as_str();
            let joins = if head == "between" {
                joiner == "and"
            } else {
                matches!(joiner, "to" | "until" | "till" | "through" | "and")
            };
            if !joins {
                // A lone "from March": the expression alone.
                return Some((Some(a.start), Some(a.end), 1 + used_a));
            }
            let (b, used_b) = expression_at(w, i + 2 + used_a, today, true)?;
            Some((Some(a.start), Some(b.end.max(a.end)), 2 + used_a + used_b))
        }
        "since" | "after" => {
            let (a, used) = expression_at(w, i + 1, today, true)?;
            let start = if head == "since" { a.start } else { a.end };
            Some((Some(start), None, 1 + used))
        }
        "before" => {
            let (a, used) = expression_at(w, i + 1, today, true)?;
            Some((None, Some(a.start), 1 + used))
        }
        "until" | "till" => {
            let (a, used) = expression_at(w, i + 1, today, true)?;
            Some((None, Some(a.end), 1 + used))
        }
        _ => {
            let cued = i > 0 && MONTH_CUES.contains(&w[i - 1].as_str());
            let (a, used) = expression_at(w, i, today, cued)?;
            Some((Some(a.start), Some(a.end), used))
        }
    }
}

/// One date expression at word `i`: its span in days and the words used.
/// `cued`: a bare month name counts (it follows a cue word).
#[allow(
    clippy::too_many_lines,
    reason = "one arm per row of ADR-0059 d6's table"
)]
fn expression_at(w: &[String], i: usize, today: i64, cued: bool) -> Option<(Span, usize)> {
    let word = w.get(i)?.as_str();
    let next = w.get(i + 1).map(String::as_str);
    let after = w.get(i + 2).map(String::as_str);
    let (ty, tm, _) = civil_from_days(today);

    // ISO day and month.
    if let Some(p) = iso_day_or_month(word) {
        return Some((p, 1));
    }
    if let Some(y) = year_of(word) {
        // "2027 q1"
        if let Some(q) = next.and_then(quarter_of) {
            return Some((quarter_span(y, q), 2));
        }
        return Some((year_span(y), 1));
    }
    if let Some(q) = quarter_of(word) {
        if let Some(y) = next.and_then(year_of) {
            return Some((quarter_span(y, q), 2));
        }
        let current_q = (tm - 1) / 3 + 1;
        let y = if q <= current_q { ty } else { ty - 1 };
        return Some((quarter_span(y, q), 1));
    }
    match word {
        "today" => return Some((day_span(today), 1)),
        "yesterday" => return Some((day_span(today - 1), 1)),
        "tomorrow" => return Some((day_span(today + 1), 1)),
        "the" => {
            // "the 14th of March 2025", "the day before yesterday" are not
            // read; "the" before an expression is skipped.
            let (span, used) = expression_at(w, i + 1, today, cued)?;
            return Some((span, used + 1));
        }
        "this" | "last" | "next" | "past" | "on" => {
            if let Some(found) = relative_at(w, i, today) {
                return Some(found);
            }
        }
        _ => {}
    }
    // n units ago
    if let (Some(n), Some(unit), Some("ago")) = (count_of(word), next, after) {
        let unit = unit.trim_end_matches('s');
        let span = match unit {
            "day" => day_span(today - n),
            "week" => {
                let s = week_start(today - 7 * n);
                Span {
                    start: s,
                    end: s + 7,
                }
            }
            "month" => {
                let (y, m) = shift_months(ty, tm, -n);
                month_span(y, m)
            }
            "year" => year_span(ty - n),
            _ => return None,
        };
        return Some((span, 3));
    }
    // weekday, bare or after "on": the latest on or before today.
    if let Some(d) = weekday_of(word) {
        let back = (today - week_start(today) - d).rem_euclid(7);
        return Some((day_span(today - back), 1));
    }
    // month name forms
    if let Some((m, full)) = month_of(word) {
        // "March 14, 2025", "March 14", "March 2025"
        if let Some(d) = next.and_then(day_number) {
            let (y, used) = match after.and_then(year_of) {
                Some(y) => (y, 3),
                None => {
                    let y = if (m, d) <= (tm, civil_from_days(today).2) {
                        ty
                    } else {
                        ty - 1
                    };
                    (y, 2)
                }
            };
            if d <= days_in_month(y, m) {
                return Some((day_span(days_from_civil(y, m, d)), used));
            }
            return None;
        }
        if let Some(y) = next.and_then(year_of) {
            return Some((month_span(y, m), 2));
        }
        if full && cued {
            let y = if m <= tm { ty } else { ty - 1 };
            return Some((month_span(y, m), 1));
        }
        return None;
    }
    // "14 March 2025", "14th of March"
    if let Some(d) = day_number(word) {
        let (month_at, of) = if next == Some("of") {
            (i + 2, 1)
        } else {
            (i + 1, 0)
        };
        if let Some((m, _)) = w.get(month_at).and_then(|x| month_of(x)) {
            let year_word = w.get(month_at + 1).map(String::as_str);
            let (y, used) = match year_word.and_then(year_of) {
                Some(y) => (y, 3 + of),
                None => {
                    let y = if (m, d) <= (tm, civil_from_days(today).2) {
                        ty
                    } else {
                        ty - 1
                    };
                    (y, 2 + of)
                }
            };
            if d <= days_in_month(y, m) {
                return Some((day_span(days_from_civil(y, m, d)), used));
            }
        }
    }
    None
}

/// `this`, `last`, `next`, `past` and `on` before a unit, a count or a
/// weekday.
fn relative_at(w: &[String], i: usize, today: i64) -> Option<(Span, usize)> {
    let head = w[i].as_str();
    let next = w.get(i + 1)?.as_str();
    let (ty, tm, _) = civil_from_days(today);
    // "last 3 days", "past two weeks": the n units before today, through
    // today.
    // "past week": the past one week.
    let (counted, unit_at) = match count_of(next) {
        Some(n) => (Some(n), i + 2),
        None if head == "past" => (Some(1), i + 1),
        None => (None, i + 2),
    };
    if matches!(head, "last" | "past")
        && let Some(n) = counted
        && let Some(unit) = w.get(unit_at)
    {
        let unit = unit.trim_end_matches('s');
        let start = match unit {
            "day" => today - n + 1,
            "week" => today - 7 * n + 1,
            "month" => {
                let (y, m) = shift_months(ty, tm, -n);
                days_from_civil(y, m, civil_from_days(today).2.min(days_in_month(y, m))) + 1
            }
            "year" => today - 365 * n + 1,
            _ => return None,
        };
        return Some((
            Span {
                start,
                end: today + 1,
            },
            unit_at - i + 1,
        ));
    }
    let by: i64 = match head {
        "last" | "past" => -1,
        "next" => 1,
        "this" | "on" => 0,
        _ => return None,
    };
    if let Some(d) = weekday_of(next) {
        let this_week = week_start(today) + d;
        let day = match head {
            // the latest such day strictly before today
            "last" | "past" => today - 1 - (today - 1 - this_week).rem_euclid(7),
            // the first such day strictly after today
            "next" => today + 1 + (this_week - today - 1).rem_euclid(7),
            "this" => this_week,
            // "on Saturday": the latest on or before today
            _ => today - (today - this_week).rem_euclid(7),
        };
        return Some((day_span(day), 2));
    }
    if head == "on" {
        return None;
    }
    let span = match next {
        "week" => {
            let s = week_start(today) + 7 * by;
            Span {
                start: s,
                end: s + 7,
            }
        }
        "weekend" => {
            let s = week_start(today) + 7 * by + 5;
            Span {
                start: s,
                end: s + 2,
            }
        }
        "month" => {
            let (y, m) = shift_months(ty, tm, by);
            month_span(y, m)
        }
        "quarter" => {
            let q = (tm - 1) / 3 + by;
            let (y, q) = (ty + q.div_euclid(4), q.rem_euclid(4) + 1);
            quarter_span(y, q)
        }
        "year" => year_span(ty + by),
        _ => {
            // "last March": the latest March strictly before this month.
            if let Some((m, true)) = month_of(next) {
                let y = match head {
                    "last" | "past" => {
                        if m < tm {
                            ty
                        } else {
                            ty - 1
                        }
                    }
                    "next" => {
                        if m > tm {
                            ty
                        } else {
                            ty + 1
                        }
                    }
                    _ => ty,
                };
                return Some((month_span(y, m), 2));
            }
            return None;
        }
    };
    Some((span, 2))
}

fn quarter_of(word: &str) -> Option<i64> {
    match word {
        "q1" => Some(1),
        "q2" => Some(2),
        "q3" => Some(3),
        "q4" => Some(4),
        _ => None,
    }
}

fn iso_day_or_month(word: &str) -> Option<Span> {
    let b = word.as_bytes();
    if !(b.len() == 7 || b.len() == 10) || b[4] != b'-' {
        return None;
    }
    let p = parse_period(word)?;
    Some(Span {
        start: p.start.div_euclid(MICROS_PER_DAY),
        end: p.end.div_euclid(MICROS_PER_DAY),
    })
}
