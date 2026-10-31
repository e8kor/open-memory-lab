//! In-progress subtrees: whether a node is inside one, what its boundary
//! refuses, and the precondition of its commit — pure functions over the
//! journalled marks and the tree the app has already fetched (ADR-0030 d1,
//! d2, d12; ADR-0019 d1).
//!
//! ADR-0030 is proposed: these rules bind only on its acceptance, and no
//! use case calls them yet. They are declared on
//! `docs/architecture/workareas.md` ("In-progress workareas" and the refusal
//! matrix) and their codes on `docs/api/errors.md`; this module mirrors them
//! and adds nothing. Each [`InProgressRefusal`] variant answers exactly one
//! code.
//!
//! Every ancestry here runs from a node to the `universe` root, as
//! [`crate::access::effective_roles`] takes it.

use std::fmt;

use uuid::Uuid;

use crate::model::MemoryStatus;

/// A workarea created with `in_progress: true`, as the journal folds it
/// (ADR-0030 d1). The mark is set only at creation and never cleared;
/// `released` is whether the workarea's commit — its ratified `development`
/// → `committed` transition, journalled with `in_progress_released: true`
/// (d12) — is in the journal. A mark not yet released is an in-progress
/// root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InProgressMark {
    pub workarea_id: Uuid,
    pub released: bool,
}

/// The rule of an in-progress subtree's boundary that refused (ADR-0030 d1,
/// d2; workareas.md refusal matrix). Every case answers
/// `workarea.in_progress_refused`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryRule {
    /// A status transition on a node inside other than its root: the
    /// subtree moves up the ladder only at its root.
    Transition,
    /// A reassignment of a claim into or out of the subtree.
    Reassignment,
    /// A re-parent that would move a node into or out of the subtree.
    Reparent,
    /// An in-progress workarea created inside another's subtree, or a
    /// re-parent of an in-progress workarea, or of a node whose subtree
    /// holds one, to a destination inside another: subtrees never nest.
    Nesting,
    /// A `prefix` on a child created inside: a vocabulary is never inside
    /// one.
    Prefix,
    /// A `derives_from` target or claim-edge endpoint inside, held or
    /// promoted, named from outside the same subtree.
    Lineage,
}

impl fmt::Display for BoundaryRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BoundaryRule::Transition => {
                "a node inside an in-progress subtree other than its root makes no status transition until the commit"
            }
            BoundaryRule::Reassignment => {
                "a claim is never reassigned into or out of an in-progress subtree"
            }
            BoundaryRule::Reparent => {
                "a re-parent never moves a node into or out of an in-progress subtree"
            }
            BoundaryRule::Nesting => "in-progress subtrees never nest",
            BoundaryRule::Prefix => "a vocabulary workarea is never inside an in-progress subtree",
            BoundaryRule::Lineage => {
                "a derives_from target or claim-edge endpoint inside an in-progress subtree is named only from inside it"
            }
        })
    }
}

/// A refusal of an in-progress subtree's rules, one variant per code
/// (errors.md; ADR-0030 d13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InProgressRefusal {
    /// `workarea.in_progress_refused` (ADR-0030 d1, d2): until its commit,
    /// nothing crosses an in-progress subtree's boundary, and the subtree
    /// moves up the ladder only at its root.
    #[error("{0} (workareas.md refusal matrix, workarea.in_progress_refused)")]
    InProgressRefused(BoundaryRule),
    /// `workarea.held_remaining` (ADR-0030 d12): the commit of `workarea_id`
    /// is proposed or ratified while a version in its subtree is held. The
    /// refusal names the workarea and counts nothing.
    #[error(
        "a version in the subtree of in-progress workarea {workarea_id} is still held, so its commit waits on review (workareas.md, workarea.held_remaining)"
    )]
    HeldRemaining { workarea_id: Uuid },
    /// `request.validation` (ADR-0030 d1): a creation body that carries
    /// both `in_progress: true` and a `prefix`.
    #[error(
        "in_progress and prefix together: an in-progress workarea is never a vocabulary workarea (http-api.md workareas, request.validation)"
    )]
    InProgressWithPrefix,
}

fn is_root(node: Uuid, marks: &[InProgressMark]) -> bool {
    marks.iter().any(|m| m.workarea_id == node && !m.released)
}

/// A node's in-progress root (ADR-0030 d9, "The root"): the nearest
/// ancestor-or-self that carries a mark whose commit is not yet journalled,
/// or `None`. The node is inside that root's subtree while this is `Some`,
/// and outside every in-progress subtree otherwise (workareas.md,
/// "In-progress workareas"). `ancestry` runs from the node to the root; an
/// empty ancestry, an unknown node, is inside nothing.
#[must_use]
pub fn in_progress_root(ancestry: &[Uuid], marks: &[InProgressMark]) -> Option<Uuid> {
    ancestry.iter().copied().find(|&node| is_root(node, marks))
}

/// The creation of a child under the node whose ancestry is
/// `parent_ancestry`, with the body's `in_progress` mark and whether the
/// body carries a `prefix` (ADR-0030 d1, d2). Both together is a malformed
/// body wherever it lands, and the body is judged before the tree. Under a
/// node inside an in-progress subtree, `in_progress` would nest and a
/// `prefix` would put a vocabulary inside. The existing creation rules, such
/// as `workarea.frozen` under a `validated` parent, are the caller's, beside
/// this one.
pub fn check_create(
    parent_ancestry: &[Uuid],
    in_progress: bool,
    prefix: bool,
    marks: &[InProgressMark],
) -> Result<(), InProgressRefusal> {
    if in_progress && prefix {
        return Err(InProgressRefusal::InProgressWithPrefix);
    }
    if in_progress_root(parent_ancestry, marks).is_some() {
        if in_progress {
            return Err(InProgressRefusal::InProgressRefused(BoundaryRule::Nesting));
        }
        if prefix {
            return Err(InProgressRefusal::InProgressRefused(BoundaryRule::Prefix));
        }
    }
    Ok(())
}

/// A status transition proposed on the node whose ancestry is `ancestry`
/// (ADR-0030 d2): refused on a node inside an in-progress subtree other than
/// its root. On the root, its move to `committed` is the commit, which
/// [`check_commit`] guards; the ladder's own rules (ADR-0009) apply beside
/// this one on every node.
pub fn check_transition(
    ancestry: &[Uuid],
    marks: &[InProgressMark],
) -> Result<(), InProgressRefusal> {
    if in_progress_root(ancestry, marks).is_some_and(|root| ancestry.first() != Some(&root)) {
        return Err(InProgressRefusal::InProgressRefused(
            BoundaryRule::Transition,
        ));
    }
    Ok(())
}

/// A reassignment of a claim from the workarea whose ancestry is `from` to
/// the one whose ancestry is `to` (ADR-0030 d2): this rule allows a move
/// between two nodes of one in-progress subtree, or between two nodes outside
/// every one, and refuses a move into a subtree, out of one, or from one to
/// another. The existing reassignment rules are the caller's, beside this
/// one.
pub fn check_reassignment(
    from: &[Uuid],
    to: &[Uuid],
    marks: &[InProgressMark],
) -> Result<(), InProgressRefusal> {
    if in_progress_root(from, marks) == in_progress_root(to, marks) {
        Ok(())
    } else {
        Err(InProgressRefusal::InProgressRefused(
            BoundaryRule::Reassignment,
        ))
    }
}

/// A re-parent of the node whose ancestry is `ancestry` and whose subtree,
/// the node included, is `subtree`, under the new parent whose ancestry is
/// `new_parent_ancestry` (ADR-0030 d2; ADR-0008 d6):
///
/// - a move that carries an in-progress subtree whole — the in-progress
///   workarea itself, or a node whose subtree holds one — is refused to a
///   destination inside another in-progress subtree, since subtrees never
///   nest, and this rule allows it anywhere else;
/// - any other move is refused when it would take a node into or out of an
///   in-progress subtree; a move between two nodes of one subtree crosses
///   nothing.
///
/// The existing re-parent rules, such as `workarea.frozen` at a `validated`
/// destination, are the caller's, beside this one.
pub fn check_reparent(
    ancestry: &[Uuid],
    subtree: &[Uuid],
    new_parent_ancestry: &[Uuid],
    marks: &[InProgressMark],
) -> Result<(), InProgressRefusal> {
    let destination = in_progress_root(new_parent_ancestry, marks);
    if subtree.iter().any(|&node| is_root(node, marks)) {
        return match destination {
            Some(_) => Err(InProgressRefusal::InProgressRefused(BoundaryRule::Nesting)),
            None => Ok(()),
        };
    }
    if in_progress_root(ancestry, marks) == destination {
        Ok(())
    } else {
        Err(InProgressRefusal::InProgressRefused(BoundaryRule::Reparent))
    }
}

/// A claim named across claims: a `derives_from` target, or a claim-edge
/// endpoint, in the workarea whose ancestry is `named`, from a submission
/// resolved to, or an edge's other endpoint in, the workarea whose ancestry
/// is `from` (ADR-0030 d2). A target inside an in-progress subtree, held or
/// promoted, is admitted only from inside the same subtree; a target outside
/// every one passes this rule. An edge is judged from each endpoint in turn.
/// The caller answers this only after the target passes the readability
/// check (ADR-0030 d5), so a key that cannot read the target gets
/// `access.not_found` and learns nothing.
pub fn check_reference(
    named: &[Uuid],
    from: &[Uuid],
    marks: &[InProgressMark],
) -> Result<(), InProgressRefusal> {
    match in_progress_root(named, marks) {
        Some(root) if in_progress_root(from, marks) != Some(root) => {
            Err(InProgressRefusal::InProgressRefused(BoundaryRule::Lineage))
        }
        _ => Ok(()),
    }
}

/// The precondition of the commit of the in-progress workarea `root`, at its
/// proposal and again at its ratification (ADR-0030 d12): refused while any
/// version in its subtree is in state `held`. A dead-lettered version is
/// still in state `held` (admission.md, dead letter), so it blocks;
/// rejected, demoted, erased and promoted versions do not. `states` are the
/// states of every version in the subtree: in the designed store, of every
/// version whose `in_progress_root_id` is `root`, which one probe of
/// `memories_held_by_root` answers inside the ratifying transaction.
pub fn check_commit(
    root: Uuid,
    states: impl IntoIterator<Item = MemoryStatus>,
) -> Result<(), InProgressRefusal> {
    if states.into_iter().any(|state| state == MemoryStatus::Held) {
        Err(InProgressRefusal::HeldRemaining { workarea_id: root })
    } else {
        Ok(())
    }
}
