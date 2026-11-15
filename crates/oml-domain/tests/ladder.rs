//! The status ladder as pure functions: the forward-only rungs and the
//! refusal reasons of `workspace.transition_refused` (ADR-0009 d1, d3, d4;
//! workspaces.md, readings the build takes), the second identity of a
//! ratification (`review.ratifier_is_proposer`, R2), and the closed list of
//! mutations a frozen area admits (ADR-0009 d5).

use oml_domain::{
    FrozenOp, LadderNode, LadderRefusal, WorkspaceStatus, frozen_refuses, propose_check,
    ratifier_is_proposer, ratify_check, same_user,
};
use uuid::Uuid;

use LadderNode::{Ordinary, Universe, UserRoot};
use WorkspaceStatus::{Development, Frozen, Protected};

const ALL: [WorkspaceStatus; 3] = [Development, Protected, Frozen];

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
    assert_eq!(words, ["development", "protected", "frozen"]);
    for status in ALL {
        assert_eq!(WorkspaceStatus::parse(status.as_str()), Some(status));
    }
    assert_eq!(WorkspaceStatus::parse("Frozen"), None);
    // The former words are unknown input, as any other word is (ADR-0091 d2).
    assert_eq!(WorkspaceStatus::parse("committed"), None);
    assert_eq!(WorkspaceStatus::parse("validated"), None);
}

#[test]
fn development_moves_only_to_committed_and_committed_only_to_validated() {
    let mut admitted = Vec::new();
    for from in ALL {
        for to in ALL {
            if propose_check(Ordinary, from, to, &[Frozen]).is_ok() {
                admitted.push((from, to));
            }
        }
    }
    assert_eq!(admitted, [(Development, Protected), (Protected, Frozen)]);
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
        propose_check(Ordinary, Protected, Development, &[]),
        Err(LadderRefusal::Backward)
    );
    assert_eq!(
        propose_check(Ordinary, Frozen, Development, &[]),
        Err(LadderRefusal::Backward)
    );
    assert_eq!(
        propose_check(Ordinary, Frozen, Protected, &[]),
        Err(LadderRefusal::Backward)
    );
    for to in ALL {
        assert!(propose_check(Ordinary, Frozen, to, &[]).is_err());
    }
    assert_eq!(LadderRefusal::Backward.reason(), "backward");
}

#[test]
fn validated_proposed_from_development_is_refused_as_a_skipped_rung() {
    assert_eq!(
        propose_check(Ordinary, Development, Frozen, &[]),
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
    assert_eq!(propose_check(Ordinary, Protected, Frozen, &[]), Ok(()));
    assert_eq!(
        propose_check(Ordinary, Protected, Frozen, &[Frozen, Frozen]),
        Ok(())
    );
    assert_eq!(
        propose_check(Ordinary, Protected, Frozen, &[Frozen, Protected]),
        Err(LadderRefusal::ChildrenNotFrozen)
    );
    assert_eq!(
        propose_check(Ordinary, Protected, Frozen, &[Development]),
        Err(LadderRefusal::ChildrenNotFrozen)
    );
    // The children constrain only the step to frozen.
    assert_eq!(
        propose_check(Ordinary, Development, Protected, &[Development]),
        Ok(())
    );
    assert_eq!(
        LadderRefusal::ChildrenNotFrozen.reason(),
        "children_not_frozen"
    );
}

#[test]
fn a_proposal_whose_from_is_no_longer_the_status_is_stale_at_ratification() {
    // A live proposal ratifies.
    assert_eq!(
        ratify_check(Development, Ordinary, Development, Protected, &[]),
        Ok(())
    );
    // Another proposal was ratified first: the node moved on.
    assert_eq!(
        ratify_check(Development, Ordinary, Protected, Protected, &[]),
        Err(LadderRefusal::Stale)
    );
    assert_eq!(
        ratify_check(Protected, Ordinary, Frozen, Frozen, &[]),
        Err(LadderRefusal::Stale)
    );
    // The propose checks run again against the node as it is now.
    assert_eq!(
        ratify_check(Protected, Ordinary, Protected, Frozen, &[Protected]),
        Err(LadderRefusal::ChildrenNotFrozen)
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
        .filter(|op| !frozen_refuses(Frozen, *op))
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
    for to in [Development, Protected] {
        assert_eq!(
            propose_check(Ordinary, Frozen, to, &[]),
            Err(LadderRefusal::Backward)
        );
        assert_eq!(
            ratify_check(Frozen, Ordinary, Frozen, to, &[]),
            Err(LadderRefusal::Backward)
        );
    }
    assert_eq!(
        propose_check(Ordinary, Frozen, Frozen, &[]),
        Err(LadderRefusal::Same)
    );
    assert_eq!(
        ratify_check(Frozen, Ordinary, Frozen, Frozen, &[]),
        Err(LadderRefusal::Same)
    );
    // Freeze has no transition operation to refuse: the ladder alone decides,
    // so a store or use case that runs the freeze gate first cannot answer
    // `workspace.frozen` where errors.md documents `transition_refused`.
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
    for status in [Development, Protected] {
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
fn a_user_root_is_refused_protected() {
    // A user root and every node under it stay development (ADR-0085 d17):
    // the ladder's one forward step from there is refused as user_root, at
    // the proposal and again at ratification.
    assert_eq!(
        propose_check(UserRoot, Development, Protected, &[]),
        Err(LadderRefusal::UserRoot)
    );
    assert_eq!(
        ratify_check(Development, UserRoot, Development, Protected, &[]),
        Err(LadderRefusal::UserRoot)
    );
    assert_eq!(LadderRefusal::UserRoot.reason(), "user_root");
}

#[test]
fn a_user_root_already_protected_is_refused_frozen_as_user_root() {
    // A root that climbed before the cap moves no further (ADR-0085 d17).
    assert_eq!(
        propose_check(UserRoot, Protected, Frozen, &[Frozen]),
        Err(LadderRefusal::UserRoot)
    );
    assert_eq!(
        ratify_check(Protected, UserRoot, Protected, Frozen, &[]),
        Err(LadderRefusal::UserRoot)
    );
}

#[test]
fn a_backward_or_same_step_inside_a_user_root_keeps_its_own_reason() {
    assert_eq!(
        propose_check(UserRoot, Protected, Development, &[]),
        Err(LadderRefusal::Backward)
    );
    assert_eq!(
        propose_check(UserRoot, Development, Development, &[]),
        Err(LadderRefusal::Same)
    );
    // A shared workspace still climbs and freezes (ADR-0009 d3).
    assert_eq!(propose_check(Ordinary, Development, Protected, &[]), Ok(()));
    assert_eq!(propose_check(Ordinary, Protected, Frozen, &[]), Ok(()));
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
    assert!(!frozen_refuses(Frozen, FrozenOp::Reject));
    assert!(frozen_refuses(Frozen, FrozenOp::Promote));
}
