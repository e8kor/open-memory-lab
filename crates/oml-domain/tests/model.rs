//! Transition-function refusals (admission.md lifecycle; ADR-0025 d10), the
//! capture normalization to NFC (admission.md, capture; ADR-0025 d6) and the
//! R2 reviewer check (ADR-0001 d2).

use oml_domain::{
    LineageClosure, MemoryStatus, Role, TransitionError, VerdictError, check_reviewer, nfc,
    transition,
};
use uuid::Uuid;

const ALL: [MemoryStatus; 5] = [
    MemoryStatus::Held,
    MemoryStatus::Promoted,
    MemoryStatus::Demoted,
    MemoryStatus::Rejected,
    MemoryStatus::Erased,
];

#[test]
fn the_claim_states_are_the_schema_set_and_none_is_superseded() {
    let names: Vec<&str> = ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        names,
        ["held", "promoted", "demoted", "rejected", "erased"],
        "postgres-schema.md, memories.state"
    );
}

#[test]
fn transition_admits_only_held_to_promoted_or_rejected() {
    // A supersession adds no state: the replaced version stays promoted and
    // only loses the tip (ADR-0025 d10). Demote, erase, reopen and
    // reassignment are unbuilt, so nothing else is legal yet.
    let legal = [
        (MemoryStatus::Held, MemoryStatus::Promoted),
        (MemoryStatus::Held, MemoryStatus::Rejected),
    ];
    for from in ALL {
        for to in ALL {
            let result = transition(from, to);
            if legal.contains(&(from, to)) {
                assert_eq!(result, Ok(to));
            } else {
                assert_eq!(
                    result,
                    Err(TransitionError { from, to }),
                    "{from:?}->{to:?}"
                );
            }
        }
    }
}

fn key(n: u8) -> Uuid {
    Uuid::from_bytes([n; 16])
}

fn closure_with_author() -> LineageClosure {
    let mut closure = LineageClosure::new();
    closure.add_principal("crew-redline/agent-3"); // the author
    closure.add_key_id(key(1));
    closure.add_principal("crew-blue/agent-7"); // a declared ancestor's author
    closure.add_key_id(key(2));
    closure
}

#[test]
fn an_independent_dreamer_may_review() {
    let closure = closure_with_author();
    assert_eq!(
        check_reviewer(Role::Dreamer, "crew-green/reviewer-1", key(9), &closure),
        Ok(())
    );
}

#[test]
fn the_author_is_refused_the_verdict() {
    let closure = closure_with_author();
    assert_eq!(
        check_reviewer(Role::Dreamer, "crew-redline/agent-3", key(9), &closure),
        Err(VerdictError::PrincipalInLineage(
            "crew-redline/agent-3".to_owned()
        ))
    );
}

#[test]
fn a_lineage_ancestor_is_refused_the_verdict() {
    // R2 runs over the whole lineage closure, not just the author (ADR-0001 d2).
    let closure = closure_with_author();
    assert_eq!(
        check_reviewer(Role::Dreamer, "crew-blue/agent-7", key(9), &closure),
        Err(VerdictError::PrincipalInLineage(
            "crew-blue/agent-7".to_owned()
        ))
    );
}

#[test]
fn a_lineage_key_is_refused_even_under_a_fresh_principal() {
    let closure = closure_with_author();
    assert_eq!(
        check_reviewer(Role::Dreamer, "crew-green/reviewer-1", key(2), &closure),
        Err(VerdictError::KeyInLineage(key(2)))
    );
}

#[test]
fn only_dreamers_review() {
    let closure = LineageClosure::new();
    for role in [Role::Reader, Role::Writer, Role::Admin, Role::Operator] {
        assert_eq!(
            check_reviewer(role, "crew-green/reviewer-1", key(9), &closure),
            Err(VerdictError::NotDreamer(role))
        );
    }
}

#[test]
fn nfc_normalizes_decomposed_text() {
    // "e" + COMBINING ACUTE ACCENT composes to the one code point U+00E9,
    // and the octet count the budget counts drops from 3 to 2.
    let decomposed = "caf\u{0065}\u{0301}";
    let composed = nfc(decomposed);
    assert_eq!(composed, "caf\u{00E9}");
    assert_eq!(decomposed.len(), 6);
    assert_eq!(composed.len(), 5);
    // Hangul jamo compose to their syllable.
    assert_eq!(nfc("\u{1100}\u{1161}"), "\u{AC00}");
    // Text already in NFC is returned unchanged.
    assert_eq!(nfc("Accra Metals \u{00E9}"), "Accra Metals \u{00E9}");
    assert_eq!(nfc(""), "");
}
