//! The workarea status ladder as pure functions (ADR-0009; workareas.md, the
//! status ladder): the three statuses (d1), the forward-only step that a
//! proposal and its ratification both test (d3), the child-status constraint
//! (d4), the second identity a ratification needs (R2 as errors.md words
//! `review.ratifier_is_proposer`), and the closed list of mutations a frozen
//! area admits (d5).
//!
//! The functions decide; the caller supplies the node as it is at that
//! moment. Ratification's caller reads the node and its children under the
//! tenant's chain lock in the transaction that writes the status, so nothing
//! arrives between the test and the write (workareas.md, readings the build
//! takes).

use uuid::Uuid;

/// A workarea's status, in rung order (ADR-0009 d1). The derived order is
/// the ladder's: a step moves to the next rung and never back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkareaStatus {
    Development,
    Committed,
    Validated,
}

impl WorkareaStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            WorkareaStatus::Development => "development",
            WorkareaStatus::Committed => "committed",
            WorkareaStatus::Validated => "validated",
        }
    }

    /// The status an exact word spells; `None` for any other string, which
    /// a caller refuses rather than coerces (errors.md, unknown enum values).
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "development" => Some(WorkareaStatus::Development),
            "committed" => Some(WorkareaStatus::Committed),
            "validated" => Some(WorkareaStatus::Validated),
            _ => None,
        }
    }

    fn rung(self) -> u8 {
        match self {
            WorkareaStatus::Development => 0,
            WorkareaStatus::Committed => 1,
            WorkareaStatus::Validated => 2,
        }
    }
}

/// Why a transition is refused: the `reason` of
/// `workarea.transition_refused` (errors.md; workareas.md, a refusal names
/// its reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum LadderRefusal {
    /// To a lower rung; none exist (ADR-0009 d3).
    #[error("backward transitions do not exist on the status ladder")]
    Backward,
    /// Past the next rung: `validated` from `development` (d3).
    #[error("a transition moves one rung; this one skips a rung")]
    Skipped,
    /// To the status the node already has (d3).
    #[error("the node already has this status")]
    Same,
    /// On the universe root, which is permanently `committed`.
    #[error("the universe root takes no transition")]
    Universe,
    /// To `validated` while a child is not `validated` (d4).
    #[error("an area becomes validated only when every child is validated")]
    ChildrenNotValidated,
    /// At ratification, the proposal's `from` is no longer the node's status.
    #[error("the proposal's from is no longer the node's status")]
    Stale,
    /// To `validated` on a user root, which climbs to `committed` at most
    /// (ADR-0046 d1).
    #[error("a user root climbs to committed and is never validated")]
    UserRoot,
}

impl LadderRefusal {
    /// The `details.reason` word errors.md lists for this refusal.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            LadderRefusal::Backward => "backward",
            LadderRefusal::Skipped => "skipped",
            LadderRefusal::Same => "same",
            LadderRefusal::Universe => "universe",
            LadderRefusal::ChildrenNotValidated => "children_not_validated",
            LadderRefusal::Stale => "stale",
            LadderRefusal::UserRoot => "user_root",
        }
    }
}

/// What a node is to the ladder: the universe root, which takes no
/// transition; a user root, which climbs to `committed` at most (ADR-0044
/// d1; ADR-0046 d1); or any other node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LadderNode {
    Universe,
    UserRoot,
    Ordinary,
}

/// Whether a proposal of `current` → `to` is admitted on a node: the
/// universe first, then the rung order, then a user root's ceiling, then
/// the preconditions (ADR-0009 d3, d4; ADR-0046 d1).
pub fn propose_check(
    node: LadderNode,
    current: WorkareaStatus,
    to: WorkareaStatus,
    children: &[WorkareaStatus],
) -> Result<(), LadderRefusal> {
    if node == LadderNode::Universe {
        return Err(LadderRefusal::Universe);
    }
    match to.rung().cmp(&current.rung()) {
        std::cmp::Ordering::Equal => return Err(LadderRefusal::Same),
        std::cmp::Ordering::Less => return Err(LadderRefusal::Backward),
        std::cmp::Ordering::Greater if to.rung() - current.rung() > 1 => {
            return Err(LadderRefusal::Skipped);
        }
        std::cmp::Ordering::Greater => {}
    }
    if node == LadderNode::UserRoot && to == WorkareaStatus::Validated {
        return Err(LadderRefusal::UserRoot);
    }
    transition_preconditions(current, to, children)
}

/// Whether a ratification of a proposal made from `proposal_from` is
/// admitted on the node as it is now: stale when the node has moved on, and
/// otherwise the propose checks again against the current children
/// (workareas.md, ratification re-checks; ADR-0009 d3, d4).
pub fn ratify_check(
    proposal_from: WorkareaStatus,
    node: LadderNode,
    current: WorkareaStatus,
    to: WorkareaStatus,
    children: &[WorkareaStatus],
) -> Result<(), LadderRefusal> {
    if proposal_from != current {
        return Err(LadderRefusal::Stale);
    }
    propose_check(node, current, to, children)
}

/// The preconditions of a one-rung step, the one place each lands. Today
/// that is d4's child-status constraint on the step to `validated`. The
/// matrix's "no simulation is fenced on the subtree" holds vacuously while
/// simulations are unbuilt (workareas.md, readings the build takes) and
/// lands here with them, as does any precondition on
/// `development` → `committed`.
pub fn transition_preconditions(
    current: WorkareaStatus,
    to: WorkareaStatus,
    children: &[WorkareaStatus],
) -> Result<(), LadderRefusal> {
    match (current, to) {
        (WorkareaStatus::Committed, WorkareaStatus::Validated)
            if children.iter().any(|c| *c != WorkareaStatus::Validated) =>
        {
            Err(LadderRefusal::ChildrenNotValidated)
        }
        _ => Ok(()),
    }
}

/// Whether a ratifier is the proposer: its key or its principal equals the
/// proposer's, either one suffices (errors.md,
/// `review.ratifier_is_proposer`; ADR-0009 d3).
#[must_use]
pub fn ratifier_is_proposer(
    proposer_key: Uuid,
    proposer_principal: &str,
    ratifier_key: Uuid,
    ratifier_principal: &str,
) -> bool {
    proposer_key == ratifier_key || proposer_principal == ratifier_principal
}

/// Whether a ratification's two keys name one user: `same_user` on the
/// ratification event (ADR-0046 d2). A key that names no user, a tenant
/// key, is never the same user as any key.
#[must_use]
pub fn same_user(proposer_user: Option<Uuid>, ratifier_user: Option<Uuid>) -> bool {
    matches!((proposer_user, ratifier_user), (Some(p), Some(r)) if p == r)
}

/// An operation a frozen area may refuse (workareas.md, refusal matrix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrozenOp {
    /// A new claim write into the area.
    Write,
    /// A promotion of a claim in the area.
    Promote,
    /// A reject verdict on a held row in the area, which promotes nothing.
    Reject,
    /// A reassignment of a claim into the area.
    ReassignIn,
    /// A reassignment of a claim out of the area.
    ReassignOut,
    /// A child workarea created under the node.
    CreateChild,
    /// A re-parent of the node's subtree.
    Reparent,
    /// A demotion, journalled.
    Demote,
    /// An erasure by crypto-shredding, journalled.
    Erase,
    /// A crew-metadata update, journalled.
    CrewUpdate,
}

/// Whether `status` refuses `op` because the area is frozen
/// (`workarea.frozen`): a `validated` area admits exactly demote, erasure
/// and crew-metadata update, and admits a reject verdict, which promotes
/// nothing (ADR-0046 d10), and every other status refuses none of these
/// operations by status (ADR-0009 d5). A status transition is not one of
/// these operations: [`propose_check`] and [`ratify_check`] alone refuse it,
/// as `workarea.transition_refused` with `backward` or `same` from
/// `validated` (errors.md).
#[must_use]
pub fn frozen_refuses(status: WorkareaStatus, op: FrozenOp) -> bool {
    status == WorkareaStatus::Validated
        && !matches!(
            op,
            FrozenOp::Demote | FrozenOp::Erase | FrozenOp::CrewUpdate | FrozenOp::Reject
        )
}
