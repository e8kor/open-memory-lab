//! Valid time, the date window and the recency factor (proposed ADR-0058
//! d2, d3, d6, d7, d8).

use oml_domain::model::MemoryKind;
use oml_domain::valid_time::{
    RECENCY_BOUND, ValidTime, ValidTimeError, Window, fixed_width, parse_now, parse_window,
    recency_factor,
};

const DAY: i64 = 86_400_000_000;

/// Microseconds at 00:00Z of an ISO day.
fn at(day: &str) -> i64 {
    parse_now(day).expect("a test day parses")
}

#[test]
fn a_period_end_names_the_whole_period() {
    let year = ValidTime::parse(Some("2015"), Some("2015")).unwrap();
    assert_eq!(year.from.as_ref().unwrap().unix_micros(), at("2015-01-01"));
    assert_eq!(year.until.as_ref().unwrap().unix_micros(), at("2016-01-01"));
    assert_eq!(
        year.from.as_ref().unwrap().as_str(),
        "2015",
        "a period stays a period"
    );

    let month = ValidTime::parse(Some("2024-02"), Some("2024-02")).unwrap();
    assert_eq!(month.until.unwrap().unix_micros(), at("2024-03-01"));
    let december = ValidTime::parse(None, Some("2024-12")).unwrap();
    assert_eq!(december.until.unwrap().unix_micros(), at("2025-01-01"));

    let day = ValidTime::parse(None, Some("2026-11-05")).unwrap();
    assert_eq!(
        day.until.unwrap().unix_micros(),
        at("2026-11-06"),
        "until a day holds through that day"
    );
}

#[test]
fn an_instant_is_exact_and_normalized_as_at_is() {
    let t = ValidTime::parse(
        Some("2026-11-05T10:00:00+02:00"),
        Some("2026-11-05T09:00:00.5Z"),
    )
    .unwrap();
    assert_eq!(
        t.from.as_ref().unwrap().as_str(),
        "2026-11-05T08:00:00.000000Z"
    );
    assert_eq!(
        t.until.as_ref().unwrap().as_str(),
        "2026-11-05T09:00:00.500000Z"
    );
    assert_eq!(
        t.until.unwrap().unix_micros() - t.from.unwrap().unix_micros(),
        3_600_500_000
    );
}

#[test]
fn an_end_in_no_accepted_form_is_refused_naming_its_field() {
    for bad in [
        "15",
        "2015-13",
        "2015-02-30",
        "2015/03/01",
        "March 2015",
        "2026-11-05T10:00:00",
        "2026-11-05T10:00:00.1234567Z",
        "",
    ] {
        assert_eq!(
            ValidTime::parse(Some(bad), None),
            Err(ValidTimeError::NotADate {
                field: "/valid_from"
            }),
            "{bad:?}"
        );
        assert_eq!(
            ValidTime::parse(None, Some(bad)).unwrap_err().field(),
            "/valid_until",
            "{bad:?}"
        );
    }
}

#[test]
fn an_interval_that_ends_before_it_begins_is_refused() {
    assert_eq!(
        ValidTime::parse(Some("2016"), Some("2015")),
        Err(ValidTimeError::EndsBeforeItBegins)
    );
    assert_eq!(
        ValidTime::parse(Some("2026-11-05T00:00:00Z"), Some("2026-11-05T00:00:00Z")),
        Err(ValidTimeError::EndsBeforeItBegins)
    );
    assert!(ValidTime::parse(Some("2015"), Some("2015")).is_ok());
}

#[test]
fn valid_time_is_refused_on_steering_guardrail_and_a_retirement() {
    let declared = ValidTime::parse(Some("2015"), None).unwrap();
    for kind in [
        MemoryKind::Steering,
        MemoryKind::Guardrail,
        MemoryKind::Term,
        MemoryKind::TermMapping,
        MemoryKind::VocabRelease,
    ] {
        let err = declared.check_kind(kind, false).unwrap_err();
        assert_eq!(err.field(), "/valid_from", "{kind:?}");
    }
    assert!(declared.check_kind(MemoryKind::Memory, true).is_err());
    assert!(declared.check_kind(MemoryKind::Memory, false).is_ok());
    assert!(declared.check_kind(MemoryKind::DerivedFact, false).is_ok());
    assert!(
        ValidTime::default()
            .check_kind(MemoryKind::Steering, true)
            .is_ok(),
        "an open version is every kind's"
    );
}

#[test]
fn a_version_has_expired_when_its_end_is_at_or_before_now() {
    let t = ValidTime::parse(None, Some("2026-11-04")).unwrap();
    assert!(!t.expired_at(at("2026-11-04") + 12 * 3_600_000_000));
    assert!(t.expired_at(at("2026-11-05")));
    assert!(t.expired_at(at("2027-01-01")));
    assert!(!ValidTime::default().expired_at(i64::MAX));
    let future = ValidTime::parse(Some("2030"), None).unwrap();
    assert!(
        !future.expired_at(at("2026-11-05")),
        "valid time schedules nothing"
    );
}

fn window(from: Option<&str>, until: Option<&str>) -> Window {
    Window {
        from_us: from.map(at),
        until_us: until.map(at),
        phrase: String::new(),
    }
}

#[test]
fn a_row_with_no_valid_from_never_matches_a_window() {
    let w = window(Some("2015-01-01"), Some("2016-01-01"));
    assert!(!ValidTime::default().overlaps(&w));
    assert!(!ValidTime::parse(None, Some("2020")).unwrap().overlaps(&w));
    assert!(ValidTime::parse(Some("2014"), None).unwrap().overlaps(&w));
    assert!(
        ValidTime::parse(Some("2015-06"), Some("2015-06"))
            .unwrap()
            .overlaps(&w)
    );
    assert!(!ValidTime::parse(Some("2016"), None).unwrap().overlaps(&w));
    assert!(
        !ValidTime::parse(Some("2013"), Some("2014"))
            .unwrap()
            .overlaps(&w)
    );
    let open_start = window(None, Some("2015-01-01"));
    assert!(
        ValidTime::parse(Some("2014"), None)
            .unwrap()
            .overlaps(&open_start)
    );
}

/// ADR-0058 d6's table, now Thursday 2026-11-05 at 15:00Z: the query, then
/// the window's from and until as ISO days (`-` for an open side), then the
/// phrase read.
const CASES: &[(&str, &str, &str, &str)] = &[
    (
        "what happened on 2026-03-14",
        "2026-03-14",
        "2026-03-15",
        "2026-03-14",
    ),
    ("deploys in 2026-03", "2026-03-01", "2026-04-01", "2026-03"),
    (
        "what did Melanie paint in 2022?",
        "2022-01-01",
        "2023-01-01",
        "2022",
    ),
    (
        "the outage of March 2025",
        "2025-03-01",
        "2025-04-01",
        "march 2025",
    ),
    (
        "on March 14, 2025 we shipped",
        "2025-03-14",
        "2025-03-15",
        "march 14 2025",
    ),
    ("14 March 2025", "2025-03-14", "2025-03-15", "14 march 2025"),
    (
        "the 3rd of May 2024",
        "2024-05-03",
        "2024-05-04",
        "the 3rd of may 2024",
    ),
    ("Sept 9", "2026-09-09", "2026-09-10", "sept 9"),
    ("Dec 24", "2025-12-24", "2025-12-25", "dec 24"),
    ("what changed in March", "2026-03-01", "2026-04-01", "march"),
    (
        "what changed in December",
        "2025-12-01",
        "2026-01-01",
        "december",
    ),
    (
        "what is planned for Q1 2027",
        "2027-01-01",
        "2027-04-01",
        "q1 2027",
    ),
    ("numbers for Q3", "2026-07-01", "2026-10-01", "q3"),
    ("numbers for 2026 Q4", "2026-10-01", "2027-01-01", "2026 q4"),
    ("what did I do today", "2026-11-05", "2026-11-06", "today"),
    (
        "what broke yesterday",
        "2026-11-04",
        "2026-11-05",
        "yesterday",
    ),
    ("plans for tomorrow", "2026-11-06", "2026-11-07", "tomorrow"),
    (
        "what did we decide last week",
        "2026-10-26",
        "2026-11-02",
        "last week",
    ),
    ("this week", "2026-11-02", "2026-11-09", "this week"),
    ("next week", "2026-11-09", "2026-11-16", "next week"),
    ("last weekend", "2026-10-31", "2026-11-02", "last weekend"),
    ("last month", "2026-10-01", "2026-11-01", "last month"),
    ("this month", "2026-11-01", "2026-12-01", "this month"),
    ("last quarter", "2026-07-01", "2026-10-01", "last quarter"),
    ("last year", "2025-01-01", "2026-01-01", "last year"),
    ("next year", "2027-01-01", "2028-01-01", "next year"),
    ("last March", "2026-03-01", "2026-04-01", "last march"),
    ("last November", "2025-11-01", "2025-12-01", "last november"),
    (
        "what did I buy last Saturday",
        "2026-10-31",
        "2026-11-01",
        "last saturday",
    ),
    ("last Thursday", "2026-10-29", "2026-10-30", "last thursday"),
    ("on Thursday", "2026-11-05", "2026-11-06", "on thursday"),
    ("Saturday", "2026-10-31", "2026-11-01", "saturday"),
    ("next Monday", "2026-11-09", "2026-11-10", "next monday"),
    ("3 days ago", "2026-11-02", "2026-11-03", "3 days ago"),
    ("a week ago", "2026-10-26", "2026-11-02", "a week ago"),
    (
        "three weeks ago",
        "2026-10-12",
        "2026-10-19",
        "three weeks ago",
    ),
    (
        "two months ago",
        "2026-09-01",
        "2026-10-01",
        "two months ago",
    ),
    ("one year ago", "2025-01-01", "2026-01-01", "one year ago"),
    (
        "in the last 3 days",
        "2026-11-03",
        "2026-11-06",
        "the last 3 days",
    ),
    (
        "past two weeks",
        "2026-10-23",
        "2026-11-06",
        "past two weeks",
    ),
    (
        "in the past week",
        "2026-10-30",
        "2026-11-06",
        "the past week",
    ),
    (
        "between 2014 and 2016",
        "2014-01-01",
        "2017-01-01",
        "between 2014 and 2016",
    ),
    (
        "from March 2025 to May 2025",
        "2025-03-01",
        "2025-06-01",
        "from march 2025 to may 2025",
    ),
    ("since March 2025", "2025-03-01", "-", "since march 2025"),
    ("after 2020", "2021-01-01", "-", "after 2020"),
    ("before 2020", "-", "2020-01-01", "before 2020"),
    ("until last week", "-", "2026-11-02", "until last week"),
    (
        "what changed after the August incident",
        "2026-09-01",
        "-",
        "after the august",
    ),
];

#[test]
fn the_date_parser_reads_each_form_in_its_table() {
    let now = at("2026-11-05") + 15 * 3_600_000_000;
    for &(query, from, until, phrase) in CASES {
        let w = parse_window(query, now).unwrap_or_else(|| panic!("{query:?} names a window"));
        let side = |s: &str| (s != "-").then(|| at(s));
        assert_eq!(
            (w.from_us, w.until_us, w.phrase.as_str()),
            (side(from), side(until), phrase),
            "{query:?}: read {:?} to {:?}",
            w.from_us.map(fixed_width),
            w.until_us.map(fixed_width)
        );
    }
}

#[test]
fn a_query_with_no_date_reads_no_window() {
    let now = at("2026-11-05");
    for query in [
        "latest deploy window",
        "after the postgres upgrade",
        "since when is two-factor mandatory",
        "most recent incident",
        "may I deploy on a weekend",
        "we march on",
        "port 5432 refused",
        "the last one",
        "the jan file",
        "",
    ] {
        assert_eq!(parse_window(query, now), None, "{query:?}");
    }
}

#[test]
fn the_first_expression_from_the_left_wins() {
    let now = at("2026-11-05");
    let w = parse_window("in 2019 and then in 2021", now).unwrap();
    assert_eq!(w.from_us, Some(at("2019-01-01")));
    assert_eq!(w.phrase, "2019");
}

#[test]
fn the_recency_factor_stays_within_ten_percent() {
    let now = at("2026-11-05");
    assert!((recency_factor(now, now) - (1.0 + RECENCY_BOUND)).abs() < 1e-6);
    assert!(
        (recency_factor(now + 400 * DAY, now) - 1.1).abs() < 1e-6,
        "a future start is age 0"
    );
    let year = recency_factor(now - 365 * DAY, now);
    assert!(
        (year - 1.0).abs() < 1e-6,
        "one half-life is the midpoint: {year}"
    );
    let mut last = f32::INFINITY;
    for days in [0, 1, 30, 365, 3_650, 36_500, 365_000] {
        let f = recency_factor(now - days * DAY, now);
        assert!(f > 0.9 - 1e-6 && f <= 1.1 + 1e-6, "{days}: {f}");
        assert!(f <= last, "older is never boosted more");
        last = f;
    }
}
