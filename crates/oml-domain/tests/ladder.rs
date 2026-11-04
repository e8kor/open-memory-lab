//! The status ladder as pure functions: the forward-only rungs and the
//! refusal reasons of `workarea.transition_refused` (ADR-0009 d1, d3, d4;
//! workareas.md, readings the build takes), the second identity of a
//! ratification (`review.ratifier_is_proposer`, R2), and the closed list of
//! mutations a frozen area admits (ADR-0009 d5).

use oml_domain::{
    FrozenOp, LadderRefusal, WorkareaStatus, frozen_refuses, propose_check, ratifier_is_proposer,
    ratify_check,
};
use uuid::Uuid;

use WorkareaStatus::{Committed, Development, Validated};

const ALL: [WorkareaStatus; 3] = [Development, Committed, Validated];

const OPS: [FrozenOp; 10] = [
    FrozenOp::Write,
    FrozenOp::Promote,
    FrozenOp::ReassignIn,
    FrozenOp::ReassignOut,
    FrozenOp::CreateChild,
    FrozenOp::Reparent,
    FrozenOp::Demote,
    FrozenOp::Erase,
    FrozenOp::CrewUpdate,
    FrozenOp::Transition,
];

#[test]
fn each_status_spells_its_ladder_word_and_parses_back() {
    let words: Vec<&str> = ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(words, ["development", "committed", "validated"]);
    for status in ALL {
        assert_eq!(WorkareaStatus::parse(status.as_str()), Some(status));
    }
    assert_eq!(WorkareaStatus::parse("frozen"), None);
    assert_eq!(WorkareaStatus::parse("Validated"), None);
}

#[test]
fn development_moves_only_to_committed_and_committed_only_to_validated() {
    let mut admitted = Vec::new();
    for from in ALL {
        for to in ALL {
            if propose_check(false, from, to, &[Validated]).is_ok() {
                admitted.push((from, to));
            }
        }
    }
    assert_eq!(admitted, [(Development, Committed), (Committed, Validated)]);
    for status in ALL {
        assert_eq!(
            propose_check(false, status, status, &[]),
            Err(LadderRefusal::Same)
        );
    }
}

#[test]
fn a_backward_step_is_refused_as_backward_and_none_exists_from_validated() {
    assert_eq!(
        propose_check(false, Committed, Development, &[]),
        Err(LadderRefusal::Backward)
    );
    assert_eq!(
        propose_check(false, Validated, Development, &[]),
        Err(LadderRefusal::Backward)
    );
    assert_eq!(
        propose_check(false, Validated, Committed, &[]),
        Err(LadderRefusal::Backward)
    );
    for to in ALL {
        assert!(propose_check(false, Validated, to, &[]).is_err());
    }
    assert_eq!(LadderRefusal::Backward.reason(), "backward");
}

#[test]
fn validated_proposed_from_development_is_refused_as_a_skipped_rung() {
    assert_eq!(
        propose_check(false, Development, Validated, &[]),
        Err(LadderRefusal::Skipped)
    );
    assert_eq!(LadderRefusal::Skipped.reason(), "skipped");
    assert_eq!(LadderRefusal::Same.reason(), "same");
}

#[test]
fn any_transition_proposed_on_universe_is_refused() {
    for from in ALL {
        for to in ALL {
            assert_eq!(
                propose_check(true, from, to, &[Development]),
                Err(LadderRefusal::Universe),
                "{} -> {} on universe",
                from.as_str(),
                to.as_str()
            );
        }
    }
    assert_eq!(LadderRefusal::Universe.reason(), "universe");
}

#[test]
fn validated_is_refused_while_any_child_is_not_validated() {
    assert_eq!(propose_check(false, Committed, Validated, &[]), Ok(()));
    assert_eq!(
        propose_check(false, Committed, Validated, &[Validated, Validated]),
        Ok(())
    );
    assert_eq!(
        propose_check(false, Committed, Validated, &[Validated, Committed]),
        Err(LadderRefusal::ChildrenNotValidated)
    );
    assert_eq!(
        propose_check(false, Committed, Validated, &[Development]),
        Err(LadderRefusal::ChildrenNotValidated)
    );
    // The children constrain only the step to validated.
    assert_eq!(
        propose_check(false, Development, Committed, &[Development]),
        Ok(())
    );
    assert_eq!(
        LadderRefusal::ChildrenNotValidated.reason(),
        "children_not_validated"
    );
}

#[test]
fn a_proposal_whose_from_is_no_longer_the_status_is_stale_at_ratification() {
    // A live proposal ratifies.
    assert_eq!(
        ratify_check(Development, false, Development, Committed, &[]),
        Ok(())
    );
    // Another proposal was ratified first: the node moved on.
    assert_eq!(
        ratify_check(Development, false, Committed, Committed, &[]),
        Err(LadderRefusal::Stale)
    );
    assert_eq!(
        ratify_check(Committed, false, Validated, Validated, &[]),
        Err(LadderRefusal::Stale)
    );
    // The propose checks run again against the node as it is now.
    assert_eq!(
        ratify_check(Committed, false, Committed, Validated, &[Committed]),
        Err(LadderRefusal::ChildrenNotValidated)
    );
    assert_eq!(LadderRefusal::Stale.reason(), "stale");
}

#[test]
fn the_ratifier_is_refused_when_its_key_or_its_principal_is_the_proposers() {
    let proposer_key = Uuid::from_u128(1);
    let other_key = Uuid::from_u128(2);
    assert!(ratifier_is_proposer(
        proposer_key,
        "alice",
        proposer_key,
        "alice"
    ));
    assert!(ratifier_is_proposer(
        proposer_key,
        "alice",
        proposer_key,
        "bob"
    ));
    assert!(ratifier_is_proposer(
        proposer_key,
        "alice",
        other_key,
        "alice"
    ));
    assert!(!ratifier_is_proposer(
        proposer_key,
        "alice",
        other_key,
        "bob"
    ));
}

#[test]
fn a_validated_area_admits_exactly_demote_erasure_and_crew_update() {
    let admitted: Vec<FrozenOp> = OPS
        .into_iter()
        .filter(|op| !frozen_refuses(Validated, *op))
        .collect();
    assert_eq!(
        admitted,
        [FrozenOp::Demote, FrozenOp::Erase, FrozenOp::CrewUpdate]
    );
}

#[test]
fn development_and_committed_refuse_none_of_the_frozen_operations() {
    for status in [Development, Committed] {
        for op in OPS {
            assert!(
                !frozen_refuses(status, op),
                "{} refused {op:?}",
                status.as_str()
            );
        }
    }
}
