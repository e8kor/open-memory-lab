//! Transition-function refusals (admission.md lifecycle, M1 slice) and the
//! R2 reviewer check (ADR-0001 d2).

use oml_domain::{
    LineageClosure, MemoryStatus, Role, TransitionError, VerdictError, check_reviewer, transition,
};
use uuid::Uuid;

const ALL: [MemoryStatus; 4] = [
    MemoryStatus::Held,
    MemoryStatus::Promoted,
    MemoryStatus::Rejected,
    MemoryStatus::Superseded,
];

#[test]
fn exactly_three_transitions_are_legal() {
    let legal = [
        (MemoryStatus::Held, MemoryStatus::Promoted),
        (MemoryStatus::Held, MemoryStatus::Rejected),
        (MemoryStatus::Promoted, MemoryStatus::Superseded),
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
