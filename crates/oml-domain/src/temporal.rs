//! Valid time, the as-of resolution and the valid filters — pure functions
//! over values the app has already read (proposed ADR-0036 d2, d3, d4;
//! ADR-0019 d1).
//!
//! - **Valid time** (d2): a claim version's optional ends `valid_from` and
//!   `valid_to`, each an RFC 3339 `full-date` or a `date-time` with an
//!   offset, forming the half-open interval [from, to) with an absent end
//!   open. A date compares as 00:00:00Z that day and stays a date; an
//!   instant is normalized as the envelope's `at` is (chain-format.md).
//!   Only `memory` and `derived_fact` take it.
//! - **The as-of resolution** (d3, rule [`ASOF_RULE`]): a read at one seq S
//!   takes two facts at S — a version submitted after S does not exist, and
//!   the tip is the version the last tip move at or before S made one — and
//!   everything else is today's, so a tip at S that is not promoted today is
//!   served at no S and no earlier version stands in.
//! - **The filters** (d4): `valid_at` and a valid range keep or remove a
//!   served row and never reorder the rest.

use uuid::Uuid;

use crate::envelope::EventTime;
use crate::model::{MemoryKind, MemoryStatus};

/// The as-of rule's id: existence and the tip taken at the resolved seq,
/// all else today's (ADR-0036 d3, d7's vocabulary row "as-of read").
pub const ASOF_RULE: &str = "oml-asof/1";

/// Why a temporal parameter is refused; each is `request.validation`
/// (errors.md) with `details.field` the [`TemporalError::field`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalReason {
    /// Neither an RFC 3339 `full-date` nor a `date-time` with an offset of
    /// at most microsecond precision (ADR-0036 d2, d4).
    NotADateOrInstant,
    /// `as_of` is a `date-time` with an offset, never a date (ADR-0036 d3).
    NotAnInstant,
    /// Both ends sent and `valid_from` is not before `valid_to` (d2).
    NotAfterValidFrom,
    /// Both range ends sent and `valid_since` is not before `valid_until`.
    NotAfterValidSince,
    /// Valid time sent on a kind, or on a retirement, that refuses it (d2);
    /// the member is the kind's wire name or `"retirement"`.
    NotTakenBy(&'static str),
    /// `valid_at` sent with `valid_since` or `valid_until` (d4).
    ValidAtWithARange,
    /// `as_of` and `as_of_seq` both sent (d3).
    AsOfWithAsOfSeq,
}

/// A refused temporal parameter: the wire field and the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {reason:?}")]
pub struct TemporalError {
    /// The request member named in `details.field`.
    pub field: &'static str,
    pub reason: TemporalReason,
}

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// One end of a valid interval, or a point a filter asks about: a date or
/// an instant, in its normalized signed form (ADR-0036 d2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidPoint {
    text: String,
    micros: i64,
    date: bool,
}

impl ValidPoint {
    /// Parses an RFC 3339 `full-date` (`YYYY-MM-DD`, kept as sent) or a
    /// `date-time` with `Z` or a `±hh:mm` offset and at most six fractional
    /// digits, normalized to the fixed-width UTC form of `at`
    /// (chain-format.md). A leap second, a day the month lacks, a finer
    /// fraction, and an instant whose UTC form falls outside the years
    /// 0000 to 9999 are refused; nothing is clamped.
    pub fn parse(s: &str) -> Result<Self, TemporalReason> {
        parse_point(s).ok_or(TemporalReason::NotADateOrInstant)
    }

    /// The normalized form: the date as sent, or the 27-byte UTC instant.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether this point is a date, which the signed form keeps a date.
    #[must_use]
    pub fn is_date(&self) -> bool {
        self.date
    }

    /// Microseconds since the Unix epoch, the order every comparison uses;
    /// a date is 00:00:00Z that day (ADR-0036 d2).
    #[must_use]
    pub fn unix_micros(&self) -> i64 {
        self.micros
    }
}

fn parse_point(s: &str) -> Option<ValidPoint> {
    let b = s.as_bytes();
    if !s.is_ascii() || b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        b[from..to].iter().try_fold(0_i64, |acc, d| {
            d.is_ascii_digit().then(|| acc * 10 + i64::from(d - b'0'))
        })
    };
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if b.len() == 10 {
        return Some(ValidPoint {
            text: s.to_owned(),
            micros: days * MICROS_PER_DAY,
            date: true,
        });
    }
    if b.len() < 20 || !matches!(b[10], b'T' | b't') || b[13] != b':' || b[16] != b':' {
        return None;
    }
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
            let (h, m) = (
                i64::from(h1 - b'0') * 10 + i64::from(h2 - b'0'),
                i64::from(m1 - b'0') * 10 + i64::from(m2 - b'0'),
            );
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
    if !(first..=last).contains(&micros) {
        return None;
    }
    Some(ValidPoint {
        text: fixed_width(micros),
        micros,
        date: false,
    })
}

/// The 27-byte `YYYY-MM-DDTHH:MM:SS.ffffffZ` form of an instant in range.
fn fixed_width(micros: i64) -> String {
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
/// calendar, as [`EventTime::unix_micros`] uses.
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

fn field_point(
    field: &'static str,
    value: Option<&str>,
) -> Result<Option<ValidPoint>, TemporalError> {
    value
        .map(|s| ValidPoint::parse(s).map_err(|reason| TemporalError { field, reason }))
        .transpose()
}

/// A claim version's valid time, [from, to), each end open when absent
/// (ADR-0036 d2). Immutable per version: a successor inherits none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidInterval {
    from: Option<ValidPoint>,
    to: Option<ValidPoint>,
}

impl ValidInterval {
    /// No valid time: the version holds in every period, and so passes
    /// every filter (d4). An event written before valid time folds as this.
    pub const OPEN: Self = Self {
        from: None,
        to: None,
    };

    /// The submitted `valid_from` and `valid_to`: each parsed under
    /// [`ValidPoint::parse`], and, both present, `valid_from` strictly
    /// before `valid_to` (d2), else refused naming `valid_to`.
    pub fn parse(valid_from: Option<&str>, valid_to: Option<&str>) -> Result<Self, TemporalError> {
        let from = field_point("valid_from", valid_from)?;
        let to = field_point("valid_to", valid_to)?;
        if let (Some(f), Some(t)) = (&from, &to)
            && f.micros >= t.micros
        {
            return Err(TemporalError {
                field: "valid_to",
                reason: TemporalReason::NotAfterValidFrom,
            });
        }
        Ok(Self { from, to })
    }

    /// The inclusive start, `None` when open.
    #[must_use]
    pub fn from(&self) -> Option<&ValidPoint> {
        self.from.as_ref()
    }

    /// The exclusive end, `None` when open.
    #[must_use]
    pub fn to(&self) -> Option<&ValidPoint> {
        self.to.as_ref()
    }

    /// Whether neither end is present.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.from.is_none() && self.to.is_none()
    }

    /// Whether a version of `kind`, a retirement or not, may carry this
    /// interval: `memory` and `derived_fact` take valid time; `steering`,
    /// which applies in every recall of its scope, the vocabulary kinds
    /// and a retirement refuse it (ADR-0036 d2). An open interval is never
    /// refused. The refusal names the first end sent.
    pub fn check_kind(&self, kind: MemoryKind, retirement: bool) -> Result<(), TemporalError> {
        if self.is_open() {
            return Ok(());
        }
        let refused_by = match kind {
            MemoryKind::Memory | MemoryKind::DerivedFact if retirement => "retirement",
            MemoryKind::Memory | MemoryKind::DerivedFact => return Ok(()),
            other => other.as_str(),
        };
        Err(TemporalError {
            field: if self.from.is_some() {
                "valid_from"
            } else {
                "valid_to"
            },
            reason: TemporalReason::NotTakenBy(refused_by),
        })
    }

    fn contains(&self, t: i64) -> bool {
        self.from.as_ref().is_none_or(|f| f.micros <= t)
            && self.to.as_ref().is_none_or(|e| t < e.micros)
    }

    fn overlaps(&self, since: Option<&ValidPoint>, until: Option<&ValidPoint>) -> bool {
        let starts_before_until = match (&self.from, until) {
            (Some(f), Some(u)) => f.micros < u.micros,
            _ => true,
        };
        let ends_after_since = match (&self.to, since) {
            (Some(e), Some(s)) => s.micros < e.micros,
            _ => true,
        };
        starts_before_until && ends_after_since
    }
}

/// A read's valid-time filter (ADR-0036 d4): none, `valid_at`, or a range
/// of `valid_since` and `valid_until`, either optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidFilter {
    None,
    /// Keeps a version when (`valid_from` null or ≤ t) and (`valid_to`
    /// null or t < it).
    At(ValidPoint),
    /// Keeps a version whose interval overlaps [since, until); at least
    /// one end is present.
    Range {
        since: Option<ValidPoint>,
        until: Option<ValidPoint>,
    },
}

impl ValidFilter {
    /// The read's `valid_at`, `valid_since` and `valid_until`: `valid_at`
    /// with either range end is refused naming `valid_at`; each value parses
    /// under [`ValidPoint::parse`]; both range ends present need since
    /// strictly before until, else refused naming `valid_until`.
    pub fn parse(
        valid_at: Option<&str>,
        valid_since: Option<&str>,
        valid_until: Option<&str>,
    ) -> Result<Self, TemporalError> {
        if valid_at.is_some() && (valid_since.is_some() || valid_until.is_some()) {
            return Err(TemporalError {
                field: "valid_at",
                reason: TemporalReason::ValidAtWithARange,
            });
        }
        if let Some(t) = field_point("valid_at", valid_at)? {
            return Ok(Self::At(t));
        }
        let since = field_point("valid_since", valid_since)?;
        let until = field_point("valid_until", valid_until)?;
        match (&since, &until) {
            (None, None) => Ok(Self::None),
            (Some(s), Some(u)) if s.micros >= u.micros => Err(TemporalError {
                field: "valid_until",
                reason: TemporalReason::NotAfterValidSince,
            }),
            _ => Ok(Self::Range { since, until }),
        }
    }

    /// Whether a served row with this valid time stays. An open interval
    /// is kept by every filter, so steering is never filtered.
    #[must_use]
    pub fn keeps(&self, valid: &ValidInterval) -> bool {
        match self {
            Self::None => true,
            Self::At(t) => valid.contains(t.micros),
            Self::Range { since, until } => valid.overlaps(since.as_ref(), until.as_ref()),
        }
    }

    /// Removes the rows the filter does not keep, in place, keeping the
    /// order of the rest: applied inside the scan before `k` and the
    /// budget, a removed row takes no place in either (ADR-0036 d4).
    pub fn retain<T>(&self, rows: &mut Vec<T>, valid: impl Fn(&T) -> &ValidInterval) {
        if *self != Self::None {
            rows.retain(|row| self.keeps(valid(row)));
        }
    }
}

/// The point an as-of read names: `as_of`, an instant, or `as_of_seq`
/// (ADR-0036 d3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsOfPoint {
    Instant(EventTime),
    Seq(u64),
}

impl AsOfPoint {
    /// The read's `as_of` and `as_of_seq`, never both: `None` is today's
    /// read. `as_of` is a `date-time` with an offset, normalized to `at`'s
    /// fixed-width form; a date is refused.
    pub fn parse(
        as_of: Option<&str>,
        as_of_seq: Option<u64>,
    ) -> Result<Option<Self>, TemporalError> {
        let refuse = |reason| TemporalError {
            field: "as_of",
            reason,
        };
        match (as_of, as_of_seq) {
            (Some(_), Some(_)) => Err(refuse(TemporalReason::AsOfWithAsOfSeq)),
            (None, Some(s)) => Ok(Some(Self::Seq(s))),
            (None, None) => Ok(None),
            (Some(t), None) => {
                let point = parse_point(t)
                    .filter(|p| !p.date)
                    .ok_or(refuse(TemporalReason::NotAnInstant))?;
                let at = EventTime::parse(&point.text)
                    .map_err(|_| refuse(TemporalReason::NotAnInstant))?;
                Ok(Some(Self::Instant(at)))
            }
        }
    }
}

/// `request.as_of_ahead` (`409`): a point this replica has not applied
/// (ADR-0036 d3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsOfAhead {
    pub applied_seq: u64,
}

/// The `at_watermark` per event: the running maximum of `at` over the
/// chain, since `at` may regress (storage.md, time; ADR-0036 d3).
/// `ats[i]` is the `at` of seq i + 1, and so is the result's.
#[must_use]
pub fn at_watermarks(ats: &[EventTime]) -> Vec<EventTime> {
    let mut marks: Vec<EventTime> = Vec::with_capacity(ats.len());
    for at in ats {
        let next = match marks.last() {
            Some(prev) if prev.as_str() >= at.as_str() => prev.clone(),
            _ => at.clone(),
        };
        marks.push(next);
    }
    marks
}

/// Resolves the point to the one seq S the read is made at (ADR-0036 d3).
/// `as_of_seq` is S. An instant t names the longest chain prefix whose
/// every `at` is at or before t: S is one before the first seq whose
/// watermark exceeds t, which `first_seq_exceeding` probes. A seq past
/// `applied_seq`, or an instant no applied watermark exceeds, is
/// [`AsOfAhead`], so an instant that answers once names one S everywhere.
pub fn resolve_as_of(
    point: &AsOfPoint,
    applied_seq: u64,
    first_seq_exceeding: impl FnOnce(&EventTime) -> Option<u64>,
) -> Result<u64, AsOfAhead> {
    let ahead = AsOfAhead { applied_seq };
    match point {
        AsOfPoint::Seq(s) if *s <= applied_seq => Ok(*s),
        AsOfPoint::Seq(_) => Err(ahead),
        AsOfPoint::Instant(t) => match first_seq_exceeding(t) {
            Some(q) if q <= applied_seq => Ok(q.saturating_sub(1)),
            _ => Err(ahead),
        },
    }
}

/// One row of `tip_intervals`: the version a tip move made the tip at
/// `from_seq`, until the move that cleared it at `to_seq`, `None` while
/// open (ADR-0036 d3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipInterval {
    pub artifact_id: Uuid,
    pub from_seq: u64,
    pub to_seq: Option<u64>,
}

impl TipInterval {
    /// Whether the version was the tip at seq `s`: [from, to).
    #[must_use]
    pub fn contains(&self, s: u64) -> bool {
        self.from_seq <= s && self.to_seq.is_none_or(|to| s < to)
    }
}

/// The tip of one tip line (the default tip, or one overlay) at seq `s`:
/// the version the last tip move at or before `s` made the tip (ADR-0036
/// d3); `None` before the line's first promote.
#[must_use]
pub fn tip_at(intervals: &[TipInterval], s: u64) -> Option<Uuid> {
    intervals
        .iter()
        .filter(|row| row.contains(s))
        .max_by_key(|row| row.from_seq)
        .map(|row| row.artifact_id)
}

/// Whether a version submitted at `submitted_seq` exists at seq `s`: one
/// submitted after `s` does not, and a get of it at `s` is
/// `access.not_found` (ADR-0036 d3).
#[must_use]
pub fn exists_at(submitted_seq: u64, s: u64) -> bool {
    submitted_seq <= s
}

/// A version of one claim as today's fold holds it: its submission seq
/// (`memories.submitted_seq`) and today's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionToday {
    pub artifact_id: Uuid,
    pub submitted_seq: u64,
    pub state: MemoryStatus,
}

/// The version an as-of read serves for a claim: always `tip_as_of`, and
/// `tip` whether it is today's tip (ADR-0036 d3, d7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServedAsOf {
    pub artifact_id: Uuid,
    pub tip: bool,
}

/// Rule `oml-asof/1` for one tip line at seq `s` (ADR-0036 d3): the tip at
/// `s`, served only when it exists at `s` and is promoted today. A tip at
/// `s` that is demoted, erased or held today is served at no `s`, and no
/// earlier version stands in. Today's grants, workarea, taint, audience and
/// every other rule of today's read are the caller's, applied as today.
#[must_use]
pub fn served_tip_as_of(
    versions: &[VersionToday],
    intervals: &[TipInterval],
    s: u64,
) -> Option<ServedAsOf> {
    let tip = tip_at(intervals, s)?;
    let version = versions.iter().find(|v| v.artifact_id == tip)?;
    if !exists_at(version.submitted_seq, s) || version.state != MemoryStatus::Promoted {
        return None;
    }
    Some(ServedAsOf {
        artifact_id: tip,
        tip: intervals
            .iter()
            .any(|row| row.artifact_id == tip && row.to_seq.is_none()),
    })
}
