//! Every state the server stores maps to a `silver.claims` state, and
//! delta-tables.md lists each one (#440; ADR-0083 d13 adds `withdrawn`).

use oml_app::domain::MemoryStatus;
use oml_lakewriter::lake_state;

/// Every [`MemoryStatus`], listed through an exhaustive match, so a new
/// state fails to compile here until it is mapped.
fn every_status() -> Vec<MemoryStatus> {
    let all = [
        MemoryStatus::Held,
        MemoryStatus::Promoted,
        MemoryStatus::Demoted,
        MemoryStatus::Rejected,
        MemoryStatus::Erased,
        MemoryStatus::Withdrawn,
    ];
    for status in all {
        match status {
            MemoryStatus::Held
            | MemoryStatus::Promoted
            | MemoryStatus::Demoted
            | MemoryStatus::Rejected
            | MemoryStatus::Erased
            | MemoryStatus::Withdrawn => {}
        }
    }
    all.to_vec()
}

#[test]
fn every_memory_status_maps_to_a_lake_state_and_delta_tables_lists_it() {
    let table = include_str!("../../../../docs/data/delta-tables.md");
    let row = table
        .lines()
        .find(|l| l.starts_with("| `state` | STRING | no |"))
        .expect("delta-tables.md declares silver.claims' state");
    for status in every_status() {
        for tip in [true, false] {
            let state = lake_state(status.as_str(), tip)
                .unwrap_or_else(|| panic!("{status:?} (tip {tip}) has no lake state"));
            assert!(
                row.contains(&format!("`{state}`")),
                "delta-tables.md's state row lists {state}: {row}"
            );
        }
    }
    assert_eq!(lake_state("promoted", true), Some("promoted"));
    assert_eq!(lake_state("promoted", false), Some("superseded"));
    assert_eq!(lake_state("rejected", true), Some("rejected"));
    assert_eq!(lake_state("withdrawn", false), Some("withdrawn"));
    assert_eq!(lake_state("reopened", true), None, "an unknown state");
}
