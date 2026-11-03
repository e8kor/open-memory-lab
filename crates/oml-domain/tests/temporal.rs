//! Valid time, the as-of resolution and the valid filters of proposed
//! ADR-0036: d2's shape and kinds, d3's rule `oml-asof/1` over the fold it
//! records (`submitted_seq`, `tip_intervals`, the `at` watermark), and d4's
//! `valid_at` and valid ranges, which only remove.

use oml_domain::temporal::{
    ASOF_RULE, AsOfAhead, AsOfPoint, ServedAsOf, TemporalError, TemporalReason, TipInterval,
    ValidFilter, ValidInterval, ValidPoint, VersionToday, at_watermarks, exists_at, resolve_as_of,
    served_tip_as_of, tip_at,
};
use oml_domain::{EventTime, MemoryKind, MemoryStatus};
use proptest::prelude::*;
use uuid::Uuid;

fn point(s: &str) -> ValidPoint {
    ValidPoint::parse(s).unwrap_or_else(|e| panic!("{s} is a valid point: {e:?}"))
}

fn interval(from: Option<&str>, to: Option<&str>) -> ValidInterval {
    ValidInterval::parse(from, to).unwrap_or_else(|e| panic!("{from:?}..{to:?}: {e}"))
}

fn at(s: &str) -> EventTime {
    EventTime::parse(s).expect("a fixed-width instant")
}

fn id(n: u8) -> Uuid {
    Uuid::from_bytes([n; 16])
}

// ---- d2: the shape ---------------------------------------------------------

#[test]
fn a_valid_from_not_before_valid_to_is_refused() {
    for (from, to) in [
        ("2026-10-01", "2026-07-01"),
        ("2026-07-01", "2026-07-01"),
        // A date compares as 00:00:00Z that day, so these name one instant.
        ("2026-07-01", "2026-07-01T00:00:00Z"),
        ("2026-07-01T02:00:00+02:00", "2026-07-01"),
    ] {
        assert_eq!(
            ValidInterval::parse(Some(from), Some(to)),
            Err(TemporalError {
                field: "valid_to",
                reason: TemporalReason::NotAfterValidFrom,
            }),
            "{from} .. {to}"
        );
    }
    // One microsecond apart is an interval.
    let one = interval(
        Some("2026-07-01T00:00:00Z"),
        Some("2026-07-01T00:00:00.000001Z"),
    );
    assert!(!one.is_open());
}

#[test]
fn a_date_compares_as_utc_midnight_and_is_served_as_a_date() {
    let q3 = point("2026-07-01");
    assert!(q3.is_date());
    assert_eq!(q3.as_str(), "2026-07-01", "the date stays a date");
    assert_eq!(
        q3.unix_micros(),
        at("2026-07-01T00:00:00.000000Z").unix_micros()
    );
    let midnight = point("2026-07-01T00:00:00Z");
    assert!(!midnight.is_date());
    assert_eq!(midnight.unix_micros(), q3.unix_micros());
    assert_ne!(q3, midnight, "equal instants, different signed forms");
}

#[test]
fn an_instant_is_normalized_as_at_is() {
    for (sent, normalized) in [
        ("2026-07-01T00:00:00Z", "2026-07-01T00:00:00.000000Z"),
        ("2026-07-01t12:30:05.25z", "2026-07-01T12:30:05.250000Z"),
        (
            "2026-07-01T01:30:00.000001+02:00",
            "2026-06-30T23:30:00.000001Z",
        ),
        ("2024-02-28T23:00:00-01:30", "2024-02-29T00:30:00.000000Z"),
        ("1969-07-20T20:17:40Z", "1969-07-20T20:17:40.000000Z"),
        ("2026-12-31T23:00:00-01:00", "2027-01-01T00:00:00.000000Z"),
    ] {
        let p = point(sent);
        assert_eq!(p.as_str(), normalized, "{sent}");
        assert_eq!(p, point(normalized), "normalizing is idempotent");
    }
}

#[test]
fn a_value_that_is_neither_an_rfc_3339_date_nor_an_instant_with_an_offset_is_refused() {
    for bad in [
        "",
        "2026-7-01",
        "2026-07-01T00:00:00",
        "2026-07-01T00:00:00.Z",
        "2026-07-01T00:00:00.0000001Z",
        "2026-07-01T00:00:60Z",
        "2026-07-01T24:00:00Z",
        "2026-07-01T00:00:00+24:00",
        "2026-07-01T00:00:00+2:00",
        "2026-07-01 00:00:00Z",
        "2026-13-01",
        "2026-02-29",
        "2026-04-31",
        "2026-02-30T00:00:00Z",
        "2026-07-01Z",
        "Q3",
        "0000-01-01T00:30:00+01:00",
        "9999-12-31T23:30:00-01:00",
        "2026-07-01T00:00:00\u{e9}",
    ] {
        assert_eq!(
            ValidInterval::parse(Some(bad), None),
            Err(TemporalError {
                field: "valid_from",
                reason: TemporalReason::NotADateOrInstant,
            }),
            "{bad:?}"
        );
        assert_eq!(
            ValidInterval::parse(None, Some(bad)).map_err(|e| e.field),
            Err("valid_to"),
            "{bad:?}"
        );
    }
    assert!(ValidPoint::parse("2024-02-29").is_ok(), "a leap day");
    assert!(ValidPoint::parse("0000-01-01").is_ok());
    assert!(ValidPoint::parse("9999-12-31T23:59:59.999999Z").is_ok());
}

#[test]
fn an_absent_end_is_open() {
    let open = interval(None, None);
    assert!(open.is_open());
    assert_eq!(open, ValidInterval::OPEN);
    let from_only = interval(Some("2027-01-01"), None);
    assert_eq!(from_only.from().map(ValidPoint::as_str), Some("2027-01-01"));
    assert_eq!(from_only.to(), None);
    assert!(!from_only.is_open());
}

#[test]
fn steering_vocabulary_kinds_and_retirements_refuse_valid_time() {
    let q3 = interval(Some("2026-07-01"), Some("2026-10-01"));
    let to_only = interval(None, Some("2026-10-01"));
    for kind in [MemoryKind::Memory, MemoryKind::DerivedFact] {
        assert_eq!(q3.check_kind(kind, false), Ok(()), "{kind:?}");
        assert_eq!(
            q3.check_kind(kind, true),
            Err(TemporalError {
                field: "valid_from",
                reason: TemporalReason::NotTakenBy("retirement"),
            }),
            "a retirement of {kind:?}"
        );
    }
    for kind in [
        MemoryKind::Steering,
        MemoryKind::Term,
        MemoryKind::TermMapping,
        MemoryKind::VocabRelease,
    ] {
        assert_eq!(
            q3.check_kind(kind, false),
            Err(TemporalError {
                field: "valid_from",
                reason: TemporalReason::NotTakenBy(kind.as_str()),
            }),
            "{kind:?}"
        );
        assert_eq!(
            to_only.check_kind(kind, false).map_err(|e| e.field),
            Err("valid_to"),
            "the refusal names the end that was sent"
        );
        assert_eq!(
            ValidInterval::OPEN.check_kind(kind, true),
            Ok(()),
            "no valid time sent is never refused"
        );
    }
}

// ---- d4: the filters -------------------------------------------------------

#[test]
fn valid_at_keeps_a_version_when_from_is_at_or_before_it_and_to_is_after_it() {
    let q3 = interval(Some("2026-07-01"), Some("2026-10-01"));
    let keeps = |t: &str| ValidFilter::parse(Some(t), None, None).unwrap().keeps(&q3);
    assert!(keeps("2026-07-01"), "from is inclusive");
    assert!(keeps("2026-07-01T00:00:00Z"));
    assert!(keeps("2026-08-01"));
    assert!(keeps("2026-09-30T23:59:59.999999Z"));
    assert!(!keeps("2026-10-01"), "to is exclusive");
    assert!(!keeps("2026-10-01T00:00:00Z"));
    assert!(!keeps("2026-06-30T23:59:59.999999Z"));
    assert!(!keeps("2026-11-01"));
    let from_2027 = interval(Some("2027-01-01"), None);
    let f = ValidFilter::parse(Some("2026-11-03"), None, None).unwrap();
    assert!(!f.keeps(&from_2027));
    assert!(
        ValidFilter::None.keeps(&from_2027),
        "valid time schedules nothing"
    );
}

#[test]
fn a_valid_range_keeps_a_version_whose_interval_overlaps_it() {
    let q3 = interval(Some("2026-07-01"), Some("2026-10-01"));
    let keeps = |since: Option<&str>, until: Option<&str>| {
        ValidFilter::parse(None, since, until).unwrap().keeps(&q3)
    };
    assert!(keeps(Some("2026-09-01"), Some("2026-11-01")));
    assert!(keeps(Some("2026-01-01"), Some("2026-07-02")));
    assert!(keeps(Some("2026-08-01"), None));
    assert!(keeps(None, Some("2026-07-02")));
    assert!(keeps(None, None));
    assert!(
        !keeps(Some("2026-10-01"), None),
        "since meets to: no overlap"
    );
    assert!(
        !keeps(None, Some("2026-07-01")),
        "until meets from: no overlap"
    );
    assert!(!keeps(Some("2026-10-01"), Some("2026-12-31")));
}

#[test]
fn an_open_version_passes_every_filter() {
    for filter in [
        ValidFilter::None,
        ValidFilter::parse(Some("0000-01-01"), None, None).unwrap(),
        ValidFilter::parse(Some("9999-12-31T23:59:59.999999Z"), None, None).unwrap(),
        ValidFilter::parse(None, Some("2026-01-01"), Some("2026-01-02")).unwrap(),
        ValidFilter::parse(None, None, Some("0000-01-02")).unwrap(),
    ] {
        assert!(filter.keeps(&ValidInterval::OPEN), "{filter:?}");
    }
}

#[test]
fn valid_at_with_valid_since_or_valid_until_is_refused() {
    for (since, until) in [(Some("2026-01-01"), None), (None, Some("2026-01-01"))] {
        assert_eq!(
            ValidFilter::parse(Some("2026-08-01"), since, until),
            Err(TemporalError {
                field: "valid_at",
                reason: TemporalReason::ValidAtWithARange,
            })
        );
    }
    assert_eq!(
        ValidFilter::parse(None, Some("2026-07"), None).map_err(|e| e.field),
        Err("valid_since")
    );
    assert_eq!(
        ValidFilter::parse(None, None, Some("soon")).map_err(|e| e.field),
        Err("valid_until")
    );
    assert_eq!(
        ValidFilter::parse(Some("today"), None, None).map_err(|e| e.field),
        Err("valid_at")
    );
    assert_eq!(ValidFilter::parse(None, None, None), Ok(ValidFilter::None));
}

#[test]
fn a_valid_range_whose_since_is_not_before_its_until_is_refused() {
    for (since, until) in [
        ("2026-10-01", "2026-07-01"),
        ("2026-07-01", "2026-07-01T00:00:00Z"),
    ] {
        assert_eq!(
            ValidFilter::parse(None, Some(since), Some(until)),
            Err(TemporalError {
                field: "valid_until",
                reason: TemporalReason::NotAfterValidSince,
            })
        );
    }
}

#[derive(Debug, Clone)]
struct Row {
    rank: usize,
    valid: ValidInterval,
}

fn arb_point() -> impl Strategy<Value = String> {
    prop_oneof![
        (2024_u32..2028, 1_u32..13, 1_u32..29)
            .prop_map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}")),
        (
            2024_u32..2028,
            1_u32..13,
            1_u32..29,
            0_u32..24,
            0_u32..1_000_000
        )
            .prop_map(|(y, m, d, h, us)| format!("{y:04}-{m:02}-{d:02}T{h:02}:00:00.{us:06}Z")),
    ]
}

fn arb_interval() -> impl Strategy<Value = ValidInterval> {
    (
        proptest::option::of(arb_point()),
        proptest::option::of(arb_point()),
    )
        .prop_map(|(from, to)| {
            ValidInterval::parse(from.as_deref(), to.as_deref()).unwrap_or(ValidInterval::OPEN)
        })
}

fn arb_filter() -> impl Strategy<Value = ValidFilter> {
    prop_oneof![
        Just(ValidFilter::None),
        arb_point().prop_map(|t| ValidFilter::parse(Some(&t), None, None).unwrap()),
        (
            proptest::option::of(arb_point()),
            proptest::option::of(arb_point())
        )
            .prop_map(|(since, until)| {
                ValidFilter::parse(None, since.as_deref(), until.as_deref())
                    .unwrap_or(ValidFilter::None)
            }),
    ]
}

proptest! {
    #[test]
    fn valid_filters_only_remove_and_never_reorder(
        intervals in proptest::collection::vec(arb_interval(), 0..24),
        filter in arb_filter(),
    ) {
        let rows: Vec<Row> = intervals
            .into_iter()
            .enumerate()
            .map(|(rank, valid)| Row { rank, valid })
            .collect();
        let mut kept = rows.clone();
        filter.retain(&mut kept, |r| &r.valid);
        // Every kept row is a row the filter keeps, in its original order,
        // and every row the filter keeps is kept.
        let kept_ranks: Vec<usize> = kept.iter().map(|r| r.rank).collect();
        let expected: Vec<usize> = rows
            .iter()
            .filter(|r| filter.keeps(&r.valid))
            .map(|r| r.rank)
            .collect();
        prop_assert_eq!(&kept_ranks, &expected);
        prop_assert!(kept_ranks.windows(2).all(|w| w[0] < w[1]));
        prop_assert!(kept.len() <= rows.len());
        // An open version passes whatever the filter.
        prop_assert!(filter.keeps(&ValidInterval::OPEN));
    }
}

// ---- d3: the point ---------------------------------------------------------

#[test]
fn as_of_and_as_of_seq_together_are_refused() {
    assert_eq!(ASOF_RULE, "oml-asof/1");
    assert_eq!(
        AsOfPoint::parse(Some("2026-10-10T00:00:00Z"), Some(7)),
        Err(TemporalError {
            field: "as_of",
            reason: TemporalReason::AsOfWithAsOfSeq,
        })
    );
    assert_eq!(AsOfPoint::parse(None, None), Ok(None));
    assert_eq!(AsOfPoint::parse(None, Some(7)), Ok(Some(AsOfPoint::Seq(7))));
    assert_eq!(
        AsOfPoint::parse(Some("2026-10-10T02:00:00+02:00"), None),
        Ok(Some(AsOfPoint::Instant(at("2026-10-10T00:00:00.000000Z"))))
    );
    for bad in ["2026-10-10", "2026-10-10T00:00:00", "yesterday"] {
        assert_eq!(
            AsOfPoint::parse(Some(bad), None),
            Err(TemporalError {
                field: "as_of",
                reason: TemporalReason::NotAnInstant,
            }),
            "{bad}: as_of is a date-time with an offset"
        );
    }
}

/// The probe d3 names: the first seq whose watermark exceeds `t`.
fn probe(watermarks: &[EventTime]) -> impl Fn(&EventTime) -> Option<u64> + '_ {
    move |t| {
        watermarks
            .iter()
            .position(|w| w.as_str() > t.as_str())
            .map(|i| i as u64 + 1)
    }
}

#[test]
fn an_as_of_seq_past_the_applied_head_or_an_instant_the_watermark_does_not_exceed_is_ahead() {
    let ats = [
        at("2026-10-01T00:00:00.000000Z"),
        at("2026-10-02T00:00:00.000000Z"),
        at("2026-10-03T00:00:00.000000Z"),
    ];
    let marks = at_watermarks(&ats);
    let resolve = |p: AsOfPoint| resolve_as_of(&p, 3, probe(&marks));
    assert_eq!(resolve(AsOfPoint::Seq(3)), Ok(3));
    assert_eq!(resolve(AsOfPoint::Seq(0)), Ok(0), "the empty prefix");
    assert_eq!(
        resolve(AsOfPoint::Seq(4)),
        Err(AsOfAhead { applied_seq: 3 })
    );
    assert_eq!(
        resolve(AsOfPoint::Instant(at("2026-10-03T00:00:00.000000Z"))),
        Err(AsOfAhead { applied_seq: 3 }),
        "the newest at does not exceed itself: a later event may share it"
    );
    assert_eq!(
        resolve(AsOfPoint::Instant(at("2026-10-02T23:59:59.999999Z"))),
        Ok(2)
    );
    assert_eq!(
        resolve(AsOfPoint::Instant(at("2026-09-30T00:00:00.000000Z"))),
        Ok(0)
    );
    // A probe that answers a seq the replica has not applied is ahead too.
    assert_eq!(
        resolve_as_of(&AsOfPoint::Instant(ats[1].clone()), 2, probe(&marks)),
        Err(AsOfAhead { applied_seq: 2 })
    );
}

fn arb_at() -> impl Strategy<Value = EventTime> {
    (0_u32..6, 0_u32..3).prop_map(|(d, h)| at(&format!("2026-10-0{}T0{h}:00:00.000000Z", d + 1)))
}

proptest! {
    #[test]
    fn an_instant_resolves_to_the_longest_prefix_stamped_at_or_before_it(
        ats in proptest::collection::vec(arb_at(), 0..20),
        t in arb_at(),
    ) {
        let marks = at_watermarks(&ats);
        prop_assert_eq!(marks.len(), ats.len());
        let head = ats.len() as u64;
        let longest = ats
            .iter()
            .take_while(|a| a.as_str() <= t.as_str())
            .count() as u64;
        let got = resolve_as_of(&AsOfPoint::Instant(t.clone()), head, probe(&marks));
        if ats.iter().any(|a| a.as_str() > t.as_str()) {
            prop_assert_eq!(got, Ok(longest));
        } else {
            prop_assert_eq!(got, Err(AsOfAhead { applied_seq: head }));
        }
    }
}

// ---- d3: the rule ----------------------------------------------------------

#[test]
fn the_tip_at_s_is_the_version_the_last_tip_move_at_or_before_s_made_one() {
    let intervals = [
        TipInterval {
            artifact_id: id(1),
            from_seq: 10,
            to_seq: Some(20),
        },
        TipInterval {
            artifact_id: id(2),
            from_seq: 20,
            to_seq: None,
        },
    ];
    assert_eq!(tip_at(&intervals, 9), None, "no tip before the promote");
    assert_eq!(tip_at(&intervals, 10), Some(id(1)));
    assert_eq!(tip_at(&intervals, 19), Some(id(1)));
    assert_eq!(tip_at(&intervals, 20), Some(id(2)), "to_seq is exclusive");
    assert_eq!(tip_at(&intervals, u64::MAX), Some(id(2)));
}

#[test]
fn a_get_at_s_of_a_version_submitted_after_s_is_not_found() {
    assert!(!exists_at(2310, 2300));
    assert!(!exists_at(2310, 2309));
    assert!(exists_at(2310, 2310));
    assert!(exists_at(2310, 9000));
}

#[test]
fn a_tip_at_s_demoted_or_erased_today_is_served_at_no_s_and_no_earlier_version_stands_in() {
    let intervals = [
        TipInterval {
            artifact_id: id(1),
            from_seq: 2,
            to_seq: Some(4),
        },
        TipInterval {
            artifact_id: id(2),
            from_seq: 4,
            to_seq: None,
        },
    ];
    for gone in [
        MemoryStatus::Demoted,
        MemoryStatus::Erased,
        MemoryStatus::Held,
    ] {
        let versions = [
            VersionToday {
                artifact_id: id(1),
                submitted_seq: 1,
                state: MemoryStatus::Promoted,
            },
            VersionToday {
                artifact_id: id(2),
                submitted_seq: 3,
                state: gone,
            },
        ];
        for s in 4..8 {
            assert_eq!(served_tip_as_of(&versions, &intervals, s), None, "{gone:?}");
        }
        assert_eq!(
            served_tip_as_of(&versions, &intervals, 3),
            Some(ServedAsOf {
                artifact_id: id(1),
                tip: false,
            })
        );
    }
}

// ---- the record's worked example (ADR-0036, Consequences) -----------------

#[test]
fn the_q3_forecast_is_12m_as_of_10_october_and_12_4m_today() {
    let (v1, v2) = (id(1), id(2));
    // Seq n is stamped at a time; 1040 and 1052 in July, 2310 and 2321 in October.
    let at_of = |seq: u64| match seq {
        0..=1039 => at("2026-07-02T00:00:00.000000Z"),
        1040..=1051 => at("2026-07-03T00:00:00.000000Z"),
        1052..=2309 => at("2026-07-04T00:00:00.000000Z"),
        2310..=2320 => at("2026-10-14T00:00:00.000000Z"),
        _ => at("2026-10-15T00:00:00.000000Z"),
    };
    let head = 2400;
    let ats: Vec<EventTime> = (1..=head).map(at_of).collect();
    let marks = at_watermarks(&ats);
    let intervals = [
        TipInterval {
            artifact_id: v1,
            from_seq: 1052,
            to_seq: Some(2321),
        },
        TipInterval {
            artifact_id: v2,
            from_seq: 2321,
            to_seq: None,
        },
    ];
    let mut versions = vec![
        VersionToday {
            artifact_id: v1,
            submitted_seq: 1040,
            state: MemoryStatus::Promoted,
        },
        VersionToday {
            artifact_id: v2,
            submitted_seq: 2310,
            state: MemoryStatus::Promoted,
        },
    ];
    let q3 = |id: Uuid| {
        assert!(id == v1 || id == v2);
        interval(Some("2026-07-01"), Some("2026-10-01"))
    };
    let recall = |versions: &[VersionToday], as_of: Option<&str>, valid_at: &str| {
        let s = match AsOfPoint::parse(as_of, None).unwrap() {
            Some(p) => resolve_as_of(&p, head, probe(&marks)).unwrap(),
            None => head,
        };
        let filter = ValidFilter::parse(Some(valid_at), None, None).unwrap();
        let mut served: Vec<(ServedAsOf, ValidInterval)> =
            served_tip_as_of(versions, &intervals, s)
                .into_iter()
                .map(|hit| (hit, q3(hit.artifact_id)))
                .collect();
        filter.retain(&mut served, |(_, valid)| valid);
        served.into_iter().map(|(hit, _)| hit).collect::<Vec<_>>()
    };

    // 3: as of 10 October, valid on 1 August: V1, the tip then and not now.
    assert_eq!(
        recall(&versions, Some("2026-10-10T00:00:00Z"), "2026-08-01"),
        [ServedAsOf {
            artifact_id: v1,
            tip: false
        }]
    );
    // 4: today: V2, "12.4M".
    assert_eq!(
        recall(&versions, None, "2026-08-01"),
        [ServedAsOf {
            artifact_id: v2,
            tip: true
        }]
    );
    // 5: valid on 1 November: V1's interval ends 2026-10-01.
    assert_eq!(
        recall(&versions, Some("2026-10-10T00:00:00Z"), "2026-11-01"),
        []
    );
    // 6: get of V2 at seq 2300: submitted at 2310, so not found.
    assert_eq!(
        resolve_as_of(&AsOfPoint::Seq(2300), head, probe(&marks)),
        Ok(2300)
    );
    assert!(!exists_at(versions[1].submitted_seq, 2300));
    // 8: an admin demotes V1: no hit, and V2 does not stand in.
    versions[0].state = MemoryStatus::Demoted;
    assert_eq!(
        recall(&versions, Some("2026-10-10T00:00:00Z"), "2026-08-01"),
        []
    );
}

// ---- properties over a journal ---------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Act {
    Submit,
    Promote(usize),
    Supersede(usize),
    TipOverride(usize),
    Demote(usize),
    Erase(usize),
    Reject(usize),
    Other,
}

fn arb_act() -> impl Strategy<Value = Act> {
    prop_oneof![
        3 => Just(Act::Submit),
        2 => (0_usize..8).prop_map(Act::Promote),
        2 => (0_usize..8).prop_map(Act::Supersede),
        1 => (0_usize..8).prop_map(Act::TipOverride),
        1 => (0_usize..8).prop_map(Act::Demote),
        1 => (0_usize..8).prop_map(Act::Erase),
        1 => (0_usize..8).prop_map(Act::Reject),
        1 => Just(Act::Other),
    ]
}

/// A model of the fold: `memories` as today's read sees it (state and
/// `is_tip`), with `tip_intervals` written as d3 says, one row opened for
/// the version whose `is_tip` a tip move sets and the row of the version
/// whose `is_tip` it clears closed, in the same event.
#[derive(Debug, Default)]
struct Journal {
    head: u64,
    versions: Vec<(VersionToday, bool)>,
    intervals: Vec<TipInterval>,
    /// Today's read and the version each seq's tip was, recorded as the
    /// journal ran: the oracle for every s.
    tip_then: Vec<Option<Uuid>>,
}

impl Journal {
    fn run(acts: &[Act]) -> Self {
        let mut j = Journal {
            tip_then: vec![None],
            ..Self::default()
        };
        for &act in acts {
            j.head += 1;
            let seq = j.head;
            let n = j.versions.len();
            let tip = j.versions.iter().position(|(_, is_tip)| *is_tip);
            match act {
                Act::Submit => j.versions.push((
                    VersionToday {
                        artifact_id: id(u8::try_from(n + 1).unwrap()),
                        submitted_seq: seq,
                        state: MemoryStatus::Held,
                    },
                    false,
                )),
                Act::Promote(i) | Act::Supersede(i)
                    if i < n
                        && j.versions[i].0.state == MemoryStatus::Held
                        && tip.is_none() == matches!(act, Act::Promote(_)) =>
                {
                    j.versions[i].0.state = MemoryStatus::Promoted;
                    j.move_tip(seq, tip, i);
                }
                Act::TipOverride(i)
                    if i < n
                        && j.versions[i].0.state == MemoryStatus::Promoted
                        && tip.is_some_and(|t| t != i) =>
                {
                    j.move_tip(seq, tip, i);
                }
                Act::Demote(i) if i < n && j.versions[i].0.state == MemoryStatus::Promoted => {
                    j.versions[i].0.state = MemoryStatus::Demoted;
                }
                Act::Erase(i) if i < n => j.versions[i].0.state = MemoryStatus::Erased,
                Act::Reject(i) if i < n && j.versions[i].0.state == MemoryStatus::Held => {
                    j.versions[i].0.state = MemoryStatus::Rejected;
                }
                _ => {}
            }
            let now = j
                .versions
                .iter()
                .find(|(_, is_tip)| *is_tip)
                .map(|(v, _)| v.artifact_id);
            j.tip_then.push(now);
        }
        j
    }

    fn move_tip(&mut self, seq: u64, cleared: Option<usize>, set: usize) {
        if let Some(c) = cleared {
            self.versions[c].1 = false;
            let cleared_id = self.versions[c].0.artifact_id;
            for row in &mut self.intervals {
                if row.artifact_id == cleared_id && row.to_seq.is_none() {
                    row.to_seq = Some(seq);
                }
            }
        }
        self.versions[set].1 = true;
        self.intervals.push(TipInterval {
            artifact_id: self.versions[set].0.artifact_id,
            from_seq: seq,
            to_seq: None,
        });
    }

    fn today(&self) -> Vec<VersionToday> {
        self.versions.iter().map(|(v, _)| v.clone()).collect()
    }

    /// Today's read: the version whose `is_tip` is set, served when it is
    /// promoted today.
    fn todays_read(&self) -> Option<ServedAsOf> {
        self.versions
            .iter()
            .find(|(v, is_tip)| *is_tip && v.state == MemoryStatus::Promoted)
            .map(|(v, _)| ServedAsOf {
                artifact_id: v.artifact_id,
                tip: true,
            })
    }
}

proptest! {
    #[test]
    fn resolution_at_the_current_seq_equals_todays_read(
        acts in proptest::collection::vec(arb_act(), 0..40),
    ) {
        let j = Journal::run(&acts);
        prop_assert_eq!(
            served_tip_as_of(&j.today(), &j.intervals, j.head),
            j.todays_read()
        );
    }

    #[test]
    fn no_erased_or_demoted_version_is_ever_served_at_any_seq(
        acts in proptest::collection::vec(arb_act(), 0..40),
    ) {
        let j = Journal::run(&acts);
        let today = j.today();
        for s in 0..=j.head + 2 {
            let tip_then = j.tip_then[usize::try_from(s.min(j.head)).unwrap()];
            prop_assert_eq!(tip_at(&j.intervals, s), tip_then, "the tip at {}", s);
            let served = served_tip_as_of(&today, &j.intervals, s);
            if let Some(hit) = served {
                let v = today.iter().find(|v| v.artifact_id == hit.artifact_id).unwrap();
                prop_assert_eq!(v.state, MemoryStatus::Promoted);
                prop_assert!(exists_at(v.submitted_seq, s));
                prop_assert_eq!(Some(hit.artifact_id), tip_then, "no stand-in at {}", s);
            } else if let Some(t) = tip_then {
                let v = today.iter().find(|v| v.artifact_id == t).unwrap();
                prop_assert_ne!(v.state, MemoryStatus::Promoted);
            }
        }
    }
}
