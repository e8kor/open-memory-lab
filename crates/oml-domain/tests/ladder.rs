//! The status ladder as pure functions: the forward-only rungs and the
//! refusal reasons of `workarea.transition_refused` (ADR-0009 d1, d3, d4;
//! workareas.md, readings the build takes), the second identity of a
//! ratification (`review.ratifier_is_proposer`, R2), and the closed list of
//! mutations a frozen area admits (ADR-0009 d5).

use oml_domain::{
    FrozenOp, LadderNode, LadderRefusal, WorkareaStatus, frozen_refuses, propose_check,
    ratifier_is_proposer, ratify_check, same_user,
};
use uuid::Uuid;

use LadderNode::{Ordinary, Universe, UserRoot};
use WorkareaStatus::{Committed, Development, Validated};

const ALL: [WorkareaStatus; 3] = [Development, Committed, Validated];

const OPS: [FrozenOp; 10] = [
    FrozenOp::Write,
    FrozenOp::Promote,
    FrozenOp::Reject,
    FrozenOp::ReassignIn,
    FrozenOp::ReassignOut,
    FrozenOp::CreateChild,
    FrozenOp::Reparent,
    FrozenOp::Demote,
    FrozenOp::Erase,
    FrozenOp::CrewUpdate,
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
            if propose_check(Ordinary, from, to, &[Validated]).is_ok() {
                admitted.push((from, to));
            }
        }
    }
    assert_eq!(admitted, [(Development, Committed), (Committed, Validated)]);
    for status in ALL {
        assert_eq!(
            propose_check(Ordinary, status, status, &[]),
            Err(LadderRefusal::Same)
        );
    }
}

#[test]
fn a_backward_step_is_refused_as_backward_and_none_exists_from_validated() {
    assert_eq!(
        propose_check(Ordinary, Committed, Development, &[]),
        Err(LadderRefusal::Backward)
    );
    assert_eq!(
        propose_check(Ordinary, Validated, Development, &[]),
        Err(LadderRefusal::Backward)
    );
    assert_eq!(
        propose_check(Ordinary, Validated, Committed, &[]),
        Err(LadderRefusal::Backward)
    );
    for to in ALL {
        assert!(propose_check(Ordinary, Validated, to, &[]).is_err());
    }
    assert_eq!(LadderRefusal::Backward.reason(), "backward");
}

#[test]
fn validated_proposed_from_development_is_refused_as_a_skipped_rung() {
    assert_eq!(
        propose_check(Ordinary, Development, Validated, &[]),
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
                propose_check(Universe, from, to, &[Development]),
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
    assert_eq!(propose_check(Ordinary, Committed, Validated, &[]), Ok(()));
    assert_eq!(
        propose_check(Ordinary, Committed, Validated, &[Validated, Validated]),
        Ok(())
    );
    assert_eq!(
        propose_check(Ordinary, Committed, Validated, &[Validated, Committed]),
        Err(LadderRefusal::ChildrenNotValidated)
    );
    assert_eq!(
        propose_check(Ordinary, Committed, Validated, &[Development]),
        Err(LadderRefusal::ChildrenNotValidated)
    );
    // The children constrain only the step to validated.
    assert_eq!(
        propose_check(Ordinary, Development, Committed, &[Development]),
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
        ratify_check(Development, Ordinary, Development, Committed, &[]),
        Ok(())
    );
    // Another proposal was ratified first: the node moved on.
    assert_eq!(
        ratify_check(Development, Ordinary, Committed, Committed, &[]),
        Err(LadderRefusal::Stale)
    );
    assert_eq!(
        ratify_check(Committed, Ordinary, Validated, Validated, &[]),
        Err(LadderRefusal::Stale)
    );
    // The propose checks run again against the node as it is now.
    assert_eq!(
        ratify_check(Committed, Ordinary, Committed, Validated, &[Committed]),
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
fn a_validated_area_admits_exactly_a_reject_demote_erasure_and_crew_update() {
    let admitted: Vec<FrozenOp> = OPS
        .into_iter()
        .filter(|op| !frozen_refuses(Validated, *op))
        .collect();
    assert_eq!(
        admitted,
        [
            FrozenOp::Reject,
            FrozenOp::Demote,
            FrozenOp::Erase,
            FrozenOp::CrewUpdate
        ]
    );
}

#[test]
fn a_transition_on_a_validated_area_is_refused_by_the_ladder_as_backward_or_same_not_by_freeze() {
    for to in [Development, Committed] {
        assert_eq!(
            propose_check(Ordinary, Validated, to, &[]),
            Err(LadderRefusal::Backward)
        );
        assert_eq!(
            ratify_check(Validated, Ordinary, Validated, to, &[]),
            Err(LadderRefusal::Backward)
        );
    }
    assert_eq!(
        propose_check(Ordinary, Validated, Validated, &[]),
        Err(LadderRefusal::Same)
    );
    assert_eq!(
        ratify_check(Validated, Ordinary, Validated, Validated, &[]),
        Err(LadderRefusal::Same)
    );
    // Freeze has no transition operation to refuse: the ladder alone decides,
    // so a store or use case that runs the freeze gate first cannot answer
    // `workarea.frozen` where errors.md documents `transition_refused`.
    // The match is exhaustive, so a transition variant fails to compile.
    for op in OPS {
        match op {
            FrozenOp::Write
            | FrozenOp::Promote
            | FrozenOp::Reject
            | FrozenOp::ReassignIn
            | FrozenOp::ReassignOut
            | FrozenOp::CreateChild
            | FrozenOp::Reparent
            | FrozenOp::Demote
            | FrozenOp::Erase
            | FrozenOp::CrewUpdate => {}
        }
    }
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

#[test]
fn a_user_root_climbs_to_committed_and_validated_is_refused_as_user_root() {
    assert_eq!(propose_check(UserRoot, Development, Committed, &[]), Ok(()));
    assert_eq!(
        propose_check(UserRoot, Committed, Validated, &[Validated]),
        Err(LadderRefusal::UserRoot)
    );
    assert_eq!(
        ratify_check(Committed, UserRoot, Committed, Validated, &[]),
        Err(LadderRefusal::UserRoot)
    );
    assert_eq!(LadderRefusal::UserRoot.reason(), "user_root");
    // Its children climb freely, so a finished subtree inside it freezes.
    assert_eq!(propose_check(Ordinary, Committed, Validated, &[]), Ok(()));
}

#[test]
fn a_ratification_is_same_user_only_when_both_keys_act_for_one_person() {
    let alice = oml_domain::name::Name::parse("alice").expect("a name");
    let bob = oml_domain::name::Name::parse("bob").expect("a name");
    assert!(same_user(Some(&alice), Some(&alice)));
    assert!(!same_user(Some(&alice), Some(&bob)));
    assert!(!same_user(Some(&alice), None));
    assert!(!same_user(None, None));
}

#[test]
fn a_validated_area_admits_a_reject_and_refuses_a_promotion() {
    assert!(!frozen_refuses(Validated, FrozenOp::Reject));
    assert!(frozen_refuses(Validated, FrozenOp::Promote));
}
