//! Names: the grammar, the parse-time classification of a reference, the
//! derived form, the naming of rows that predate names, and the crew
//! independence predicates (ADR-0026 d2, d3, d8, d11; http-api.md, "Names
//! and ids" and "Rows created before names").

use oml_domain::name::{
    CrewObject, CrewRecord, Name, NameRefusal, NamedKind, Naming, Reference, UnnamedRow,
    assign_names, parse_id,
};
use oml_domain::{KeyCrew, satisfies_crew_inequality, satisfies_manager_inequality};
use proptest::prelude::*;
use uuid::Uuid;

/// The value as a name, or the test fails naming why it was refused.
fn named(value: &str) -> String {
    match Name::parse(value) {
        Ok(n) => n.as_str().to_owned(),
        Err(refusal) => panic!("{value:?} was refused: {refusal}"),
    }
}

#[test]
fn a_lowercase_rfc_1035_label_is_a_name() {
    // Every example name in the doc set (ADR-0026 d2), plus the label
    // grammar's edges: one letter, a digit last, a hyphen run inside.
    for value in [
        "universe",
        "procurement",
        "metals-crew",
        "q1-2027",
        "research-crew",
        "lit-review",
        "run-grid",
        "oml",
        "a",
        "a1",
        "a--b",
        "claude-laptop-2",
    ] {
        assert_eq!(named(value), value, "names compare byte for byte");
        assert_eq!(
            Reference::parse(value),
            Ok(Reference::Name(Name::parse(value).expect("a name"))),
            "{value:?} classifies as a name"
        );
    }
    // What is not an RFC 1035 label is not a name: a letter first, a letter
    // or digit last, at least one character, ASCII only, no space.
    for value in ["", "2027-q1", "-metals", "metals-", "métal", "metals crew"] {
        assert_eq!(Name::parse(value), Err(NameRefusal::Grammar), "{value:?}");
        assert_eq!(
            Reference::parse(value),
            Err(NameRefusal::Grammar),
            "{value:?}"
        );
    }
}

#[test]
fn a_value_in_the_hex_and_dash_form_is_an_id_in_either_case() {
    let id = Uuid::from_u128(0x0190_f2a4_7c3e_4b1d_9a6f_e2d8_c51b_3a07);
    for value in [
        "0190f2a4-7c3e-4b1d-9a6f-e2d8c51b3a07",
        "0190F2A4-7C3E-4B1D-9A6F-E2D8C51B3A07",
        "0190f2A4-7c3E-4B1d-9a6F-E2d8c51B3a07",
    ] {
        assert_eq!(parse_id(value), Some(id), "{value:?}");
        assert_eq!(Reference::parse(value), Ok(Reference::Id(id)), "{value:?}");
    }
    // Served lowercase (ADR-0026 d3).
    assert_eq!(id.to_string(), "0190f2a4-7c3e-4b1d-9a6f-e2d8c51b3a07");

    // An id that the label grammar would admit — a letter first, lowercase —
    // is still an id, and is refused as a name, so the classes never overlap.
    let lettered = "abcdef01-2345-6789-abcd-ef0123456789";
    assert_eq!(
        Reference::parse(lettered),
        Ok(Reference::Id(Uuid::from_u128(
            0xabcd_ef01_2345_6789_abcd_ef01_2345_6789
        )))
    );
    assert_eq!(Name::parse(lettered), Err(NameRefusal::ReadsAsId));
}

#[test]
fn thirty_two_hex_digits_the_derived_form_uppercase_dot_slash_and_underscore_are_neither() {
    // Refused although the label grammar admits them: they read as an id.
    for value in [
        "deadbeefdeadbeefdeadbeefdeadbeef",
        "tenant-0190f2a47c3e4b1d9a6fe2d8c51b3a07",
        "wa-0190f2a47c3e4b1d9a6fe2d8c51b3a07",
        "crew-0190f2a47c3e4b1d9a6fe2d8c51b3a07",
        "key-0190f2a47c3e4b1d9a6fe2d8c51b3a07",
    ] {
        assert_eq!(parse_id(value), None, "{value:?}");
        assert_eq!(Name::parse(value), Err(NameRefusal::ReadsAsId), "{value:?}");
        assert_eq!(
            Reference::parse(value),
            Err(NameRefusal::ReadsAsId),
            "{value:?}"
        );
    }
    // Refused by the grammar itself: refused, never coerced (errors.md).
    for value in [
        "0190f2a47c3e4b1d9a6fe2d8c51b3a07",
        "Metals-Crew",
        "METALS",
        "universe.procurement",
        "crew-procure/agent-7",
        "metals_crew",
        "{0190f2a4-7c3e-4b1d-9a6f-e2d8c51b3a07}",
        "urn:uuid:0190f2a4-7c3e-4b1d-9a6f-e2d8c51b3a07",
    ] {
        assert_eq!(parse_id(value), None, "{value:?}");
        assert_eq!(Name::parse(value), Err(NameRefusal::Grammar), "{value:?}");
        assert_eq!(
            Reference::parse(value),
            Err(NameRefusal::Grammar),
            "{value:?}"
        );
    }
    // A prefix that is not one of the four kinds, or a hex run that is not
    // 32 digits long, is an ordinary name.
    assert_eq!(
        named("ws-0190f2a47c3e4b1d9a6fe2d8c51b3a07"),
        "ws-0190f2a47c3e4b1d9a6fe2d8c51b3a07"
    );
    assert_eq!(
        named("wa-0190f2a47c3e4b1d9a6fe2d8c51b3a0"),
        "wa-0190f2a47c3e4b1d9a6fe2d8c51b3a0"
    );
    assert_eq!(
        named("deadbeefdeadbeefdeadbeefdeadbeefa"),
        "deadbeefdeadbeefdeadbeefdeadbeefa"
    );
}

#[test]
fn a_64_character_name_is_refused() {
    let longest = format!("a{}", "b".repeat(62));
    assert_eq!(longest.len(), 63);
    assert_eq!(named(&longest), longest, "63 characters is the limit");
    let too_long = format!("a{}", "b".repeat(63));
    assert_eq!(too_long.len(), 64);
    assert_eq!(Name::parse(&too_long), Err(NameRefusal::Grammar));
    assert_eq!(Reference::parse(&too_long), Err(NameRefusal::Grammar));
}

/// Values near both classes, so the property exercises the boundary rather
/// than only strings that are neither.
fn near_either() -> impl Strategy<Value = String> {
    prop_oneof![
        "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        "[a-f][0-9a-f]{7}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
        "[a-f0-9-]{34,38}",
        "[a-f][0-9a-f]{31}",
        "(tenant|wa|crew|key)-[0-9a-f]{30,34}",
        "[a-z][a-z0-9-]{0,64}",
        any::<String>(),
    ]
}

proptest! {
    #[test]
    fn no_string_is_both_an_id_and_a_name(value in near_either()) {
        let id = parse_id(&value);
        let as_name = Name::parse(&value);
        prop_assert!(
            !(id.is_some() && as_name.is_ok()),
            "{value:?} is both an id and a name"
        );
        match Reference::parse(&value) {
            Ok(Reference::Id(parsed)) => prop_assert_eq!(Some(parsed), id),
            Ok(Reference::Name(parsed)) => {
                prop_assert_eq!(id, None);
                prop_assert_eq!(Ok(parsed), as_name);
            }
            Err(refusal) => {
                prop_assert_eq!(id, None);
                prop_assert_eq!(Err(refusal), as_name);
            }
        }
    }
}

#[test]
fn a_derived_name_is_the_kind_prefix_and_the_32_hex_digits_of_the_id() {
    let id = Uuid::from_u128(0x0190_f2a4_7c3e_4b1d_9a6f_e2d8_c51b_3a07);
    for (kind, expected) in [
        (NamedKind::Tenant, "tenant-0190f2a47c3e4b1d9a6fe2d8c51b3a07"),
        (NamedKind::Workarea, "wa-0190f2a47c3e4b1d9a6fe2d8c51b3a07"),
        (NamedKind::Crew, "crew-0190f2a47c3e4b1d9a6fe2d8c51b3a07"),
        (NamedKind::Key, "key-0190f2a47c3e4b1d9a6fe2d8c51b3a07"),
    ] {
        let derived = Name::derived(kind, id);
        assert_eq!(derived.as_str(), expected);
        // The form is reserved: no creation can take it (ADR-0026 d2, d11).
        assert_eq!(Name::parse(expected), Err(NameRefusal::ReadsAsId));
    }
}

const ROOT: Uuid = Uuid::from_u128(0xA0);
const PROCUREMENT: Uuid = Uuid::from_u128(0xA1);
const METALS: Uuid = Uuid::from_u128(0xA2);
const LATER_METALS: Uuid = Uuid::from_u128(0xA3);

fn row(kind: NamedKind, id: Uuid, prior_name: Option<&str>) -> UnnamedRow {
    UnnamedRow {
        kind,
        id,
        prior_name: prior_name.map(str::to_owned),
    }
}

/// Fresh ids for the crew records rule 4 creates: 0xC0, 0xC1, …
fn crew_ids() -> impl FnMut() -> Uuid {
    let mut next = 0xC0;
    move || {
        let id = Uuid::from_u128(next);
        next += 1;
        id
    }
}

/// Each assignment as (kind, id, name, prior name).
fn assigned(naming: &Naming) -> Vec<(NamedKind, Uuid, &str, Option<&str>)> {
    naming
        .names
        .iter()
        .map(|a| (a.kind, a.id, a.name.as_str(), a.prior_name.as_deref()))
        .collect()
}

#[test]
fn the_root_is_named_universe_whatever_its_creation_event_says() {
    use NamedKind::Workarea;
    // The PoC's root was created as `root`; another tenant's as `universe`;
    // one with no name at all, or with a name the grammar refuses.
    for prior in [Some("root"), Some("universe"), None, Some("Root Node")] {
        let naming = assign_names(Some(ROOT), &[row(Workarea, ROOT, prior)], &[], crew_ids());
        assert_eq!(assigned(&naming), [(Workarea, ROOT, "universe", prior)]);
        assert!(naming.crews.is_empty());
    }
    // The root keeps `universe` wherever it stands in journal order, and no
    // other workarea may hold it, before or after the root.
    let naming = assign_names(
        Some(ROOT),
        &[
            row(Workarea, PROCUREMENT, Some("universe")),
            row(Workarea, ROOT, Some("procurement")),
            row(Workarea, METALS, Some("universe")),
        ],
        &[],
        crew_ids(),
    );
    assert_eq!(
        assigned(&naming),
        [
            (
                Workarea,
                PROCUREMENT,
                "wa-000000000000000000000000000000a1",
                Some("universe")
            ),
            (Workarea, ROOT, "universe", Some("procurement")),
            (
                Workarea,
                METALS,
                "wa-000000000000000000000000000000a2",
                Some("universe")
            ),
        ]
    );
}

#[test]
fn the_first_conforming_use_of_a_name_keeps_it_and_later_ones_are_derived() {
    use NamedKind::{Key, Tenant, Workarea};
    let naming = assign_names(
        Some(ROOT),
        &[
            row(Workarea, ROOT, Some("root")),
            // A name the grammar refuses does not hold it for a later row.
            row(Workarea, Uuid::from_u128(0xA9), Some("Metals")),
            row(Workarea, METALS, Some("metals")),
            row(Workarea, LATER_METALS, Some("metals")),
            row(Workarea, PROCUREMENT, Some("procurement")),
            // Keys before names carry none; a key is its own namespace, so a
            // key may hold a name a workarea holds.
            row(Key, Uuid::from_u128(0xB1), None),
            row(Key, Uuid::from_u128(0xB2), Some("metals")),
            // A row in the derived form is named from its own id.
            row(
                Workarea,
                Uuid::from_u128(0xAB),
                Some("wa-000000000000000000000000000000a2"),
            ),
        ],
        &[],
        crew_ids(),
    );
    assert_eq!(
        assigned(&naming),
        [
            (Workarea, ROOT, "universe", Some("root")),
            (
                Workarea,
                Uuid::from_u128(0xA9),
                "wa-000000000000000000000000000000a9",
                Some("Metals")
            ),
            (Workarea, METALS, "metals", Some("metals")),
            (
                Workarea,
                LATER_METALS,
                "wa-000000000000000000000000000000a3",
                Some("metals")
            ),
            (Workarea, PROCUREMENT, "procurement", Some("procurement")),
            (
                Key,
                Uuid::from_u128(0xB1),
                "key-000000000000000000000000000000b1",
                None
            ),
            (Key, Uuid::from_u128(0xB2), "metals", Some("metals")),
            (
                Workarea,
                Uuid::from_u128(0xAB),
                "wa-000000000000000000000000000000ab",
                Some("wa-000000000000000000000000000000a2")
            ),
        ]
    );

    // The provider chain: tenants and operator keys, with no root.
    let naming = assign_names(
        None,
        &[
            row(Tenant, Uuid::from_u128(0x71), Some("oml")),
            row(Tenant, Uuid::from_u128(0x72), Some("oml")),
            row(Tenant, Uuid::from_u128(0x73), Some("universe")),
        ],
        &[],
        crew_ids(),
    );
    assert_eq!(
        assigned(&naming),
        [
            (Tenant, Uuid::from_u128(0x71), "oml", Some("oml")),
            (
                Tenant,
                Uuid::from_u128(0x72),
                "tenant-00000000000000000000000000000072",
                Some("oml")
            ),
            (Tenant, Uuid::from_u128(0x73), "universe", Some("universe")),
        ]
    );
}

fn crew(name: Option<&str>, charter_uri: Option<&str>, manager: Option<&str>) -> CrewObject {
    CrewObject {
        name: name.map(str::to_owned),
        charter_uri: charter_uri.map(str::to_owned),
        external_manager: manager.map(str::to_owned),
    }
}

#[test]
fn identical_crew_objects_become_one_crew_record_distinct_ones_distinct_records() {
    use NamedKind::Workarea;
    let redline = crew(
        Some("redline"),
        Some("https://example.com/redline"),
        Some("crewd.example.com"),
    );
    // Same name, another charter: a distinct record, and a later use of the name.
    let other_redline = crew(
        Some("redline"),
        Some("https://example.com/other"),
        Some("crewd.example.com"),
    );
    let unnamed = crew(None, None, Some("crewd.example.com"));
    let (a, b, c, d, e) = (
        Uuid::from_u128(0xA1),
        Uuid::from_u128(0xA2),
        Uuid::from_u128(0xA3),
        Uuid::from_u128(0xA4),
        Uuid::from_u128(0xA5),
    );
    let naming = assign_names(
        Some(ROOT),
        &[
            row(Workarea, ROOT, Some("root")),
            row(Workarea, a, Some("a")),
            row(Workarea, b, Some("b")),
            row(Workarea, c, Some("c")),
            row(Workarea, d, Some("d")),
            row(Workarea, e, Some("e")),
        ],
        &[
            (a, redline.clone()),
            (b, other_redline.clone()),
            (c, redline.clone()),
            (d, unnamed.clone()),
            (e, unnamed.clone()),
        ],
        crew_ids(),
    );
    assert_eq!(
        naming.crews,
        [
            CrewRecord {
                crew_id: Uuid::from_u128(0xC0),
                name: Name::parse("redline").expect("a name"),
                charter_uri: redline.charter_uri,
                external_manager: redline.external_manager,
                workarea_ids: vec![a, c],
            },
            CrewRecord {
                crew_id: Uuid::from_u128(0xC1),
                name: Name::derived(NamedKind::Crew, Uuid::from_u128(0xC1)),
                charter_uri: other_redline.charter_uri,
                external_manager: other_redline.external_manager,
                workarea_ids: vec![b],
            },
            CrewRecord {
                crew_id: Uuid::from_u128(0xC2),
                name: Name::derived(NamedKind::Crew, Uuid::from_u128(0xC2)),
                charter_uri: None,
                external_manager: unnamed.external_manager,
                workarea_ids: vec![d, e],
            },
        ]
    );
    assert_eq!(
        naming.crews[1].name.as_str(),
        "crew-000000000000000000000000000000c1"
    );
    // Crew records are listed with the crews, never among the row names.
    assert!(naming.names.iter().all(|a| a.kind == Workarea));
}

fn key_crew(crew: u128, manager: Option<&str>) -> KeyCrew {
    KeyCrew {
        crew_id: Uuid::from_u128(crew),
        external_manager: manager.map(str::to_owned),
    }
}

#[test]
fn a_key_with_no_crew_satisfies_no_crew_inequality_requirement() {
    let research = key_crew(0xC1, Some("crewd.example.com"));
    let ops = key_crew(0xC2, Some("ops.example.com"));
    assert!(satisfies_crew_inequality(Some(&research), Some(&ops)));
    assert!(!satisfies_crew_inequality(Some(&research), Some(&research)));
    assert!(
        !satisfies_crew_inequality(None, Some(&ops)),
        "no author crew"
    );
    assert!(
        !satisfies_crew_inequality(Some(&research), None),
        "no reviewer crew"
    );
    assert!(!satisfies_crew_inequality(None, None));
    // Nor any manager-inequality requirement.
    assert!(!satisfies_manager_inequality(None, Some(&ops)));
    assert!(!satisfies_manager_inequality(Some(&research), None));
}

#[test]
fn a_crew_with_no_external_manager_satisfies_no_manager_inequality_requirement() {
    let research = key_crew(0xC1, Some("crewd.example.com"));
    let ops = key_crew(0xC2, Some("ops.example.com"));
    let unmanaged = key_crew(0xC3, None);
    let also_unmanaged = key_crew(0xC4, None);
    assert!(satisfies_manager_inequality(Some(&research), Some(&ops)));
    assert!(!satisfies_manager_inequality(Some(&unmanaged), Some(&ops)));
    assert!(!satisfies_manager_inequality(
        Some(&research),
        Some(&unmanaged)
    ));
    assert!(!satisfies_manager_inequality(
        Some(&unmanaged),
        Some(&also_unmanaged)
    ));
    // Two crews under one manager satisfy the crew requirement, not the
    // manager one.
    let research_too = key_crew(0xC5, Some("crewd.example.com"));
    assert!(satisfies_crew_inequality(
        Some(&research),
        Some(&research_too)
    ));
    assert!(!satisfies_manager_inequality(
        Some(&research),
        Some(&research_too)
    ));
    // The crew requirement does not need a manager.
    assert!(satisfies_crew_inequality(
        Some(&unmanaged),
        Some(&also_unmanaged)
    ));
}
