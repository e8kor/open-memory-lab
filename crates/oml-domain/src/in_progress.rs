//! In-progress workareas and the curator profile: pure rules over the
//! workarea marks the tree read already returns (ADR-0030 r2; ADR-0019 d1).
//!
//! A node is *inside* while it has an in-progress root, its nearest
//! ancestor-or-self whose `mode` is `in_progress` (d1). Subtrees never nest,
//! so the root is unique. Nothing here reads a memory row: the boundary is
//! the tree and the marks, so a mark and a commit rewrite no row (d2).

use std::fmt;

use uuid::Uuid;

use crate::model::{HeldBy, Role};

/// A workarea's mode (ADR-0030 d1): `standard`, every workarea's default, or
/// `in_progress`, set and cleared only by a person's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WorkareaMode {
    #[default]
    Standard,
    InProgress,
}

impl WorkareaMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            WorkareaMode::Standard => "standard",
            WorkareaMode::InProgress => "in_progress",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "standard" => Some(WorkareaMode::Standard),
            "in_progress" => Some(WorkareaMode::InProgress),
            _ => None,
        }
    }
}

/// A key's profile (ADR-0030 d8): `curator` is a dreamer key held by an
/// agent that may narrow inside an in-progress scope and never widen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyProfile {
    Curator,
}

impl KeyProfile {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            KeyProfile::Curator => "curator",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "curator" => Some(KeyProfile::Curator),
            _ => None,
        }
    }
}

/// The node's in-progress root (d1): the nearest ancestor-or-self marked
/// in progress, or `None`. `ancestry` runs node to `universe`, each node with
/// whether its `mode` is `in_progress`.
#[must_use]
pub fn in_progress_root(ancestry: &[(Uuid, bool)]) -> Option<Uuid> {
    ancestry
        .iter()
        .find(|(_, in_progress)| *in_progress)
        .map(|(id, _)| *id)
}

/// The boundary (d2): a row on a node whose root is `node_root` is served to
/// a read scoped at a node whose root is `scope_root` only if the row's node
/// has no root, or the scope has the same one.
#[must_use]
pub fn crosses_into(scope_root: Option<Uuid>, node_root: Option<Uuid>) -> bool {
    node_root.is_some() && node_root != scope_root
}

/// Why setting `in_progress` was refused (d1) or a crossing act was refused
/// (d6, d7): `workarea.in_progress_refused` with `details.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InProgressReason {
    /// The node is not `development`.
    Status,
    /// The node is `universe`.
    Universe,
    /// An ancestor or a descendant is already in progress.
    Nested,
    /// A status transition on a node inside waits for the commit.
    CommitFirst,
    /// An edge whose endpoints have different roots.
    EdgeCrosses,
}

impl InProgressReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            InProgressReason::Status => "status",
            InProgressReason::Universe => "universe",
            InProgressReason::Nested => "nested",
            InProgressReason::CommitFirst => "commit_first",
            InProgressReason::EdgeCrosses => "edge_crosses",
        }
    }
}

impl fmt::Display for InProgressReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InProgressReason::Status => "only a development workarea may be put in progress",
            InProgressReason::Universe => "universe is never in progress",
            InProgressReason::Nested => "in-progress subtrees never nest",
            InProgressReason::CommitFirst => {
                "a node inside an in-progress subtree moves up the ladder only after its commit"
            }
            InProgressReason::EdgeCrosses => {
                "an edge never crosses an in-progress subtree's boundary before its commit"
            }
        })
    }
}

/// Setting `in_progress` on a node (d1): `ancestry` runs node to `universe`
/// with each node's mark, `subtree_marked` is whether any strict descendant
/// is marked, `development` whether the node's status is `development`.
/// Universe first, then status, then nesting.
pub fn check_mark(
    ancestry: &[(Uuid, bool)],
    subtree_marked: bool,
    development: bool,
) -> Result<(), InProgressReason> {
    if ancestry.len() == 1 {
        return Err(InProgressReason::Universe);
    }
    if !development {
        return Err(InProgressReason::Status);
    }
    if ancestry.iter().skip(1).any(|(_, marked)| *marked) || subtree_marked {
        return Err(InProgressReason::Nested);
    }
    Ok(())
}

/// Who may set the mode (d1): a dreamer key a person holds. Every other key
/// is refused before any lookup.
#[must_use]
pub fn may_set_mode(role: Role, held_by: HeldBy, profile: Option<KeyProfile>) -> bool {
    role == Role::Dreamer && held_by == HeldBy::Human && profile.is_none()
}

/// The issuance shape of a profile (d8): a curator is a dreamer key held by
/// an agent.
#[must_use]
pub fn profile_admits(profile: KeyProfile, role: Role, held_by: HeldBy) -> bool {
    match profile {
        KeyProfile::Curator => role == Role::Dreamer && held_by == HeldBy::Agent,
    }
}

/// The grant roles a key may carry, profile included (d8): a curator's are
/// reader, writer and dreamer; otherwise the role's own admissible set.
#[must_use]
pub fn admissible_roles(role: Role, profile: Option<KeyProfile>) -> &'static [Role] {
    match profile {
        Some(KeyProfile::Curator) => &[Role::Reader, Role::Writer, Role::Dreamer],
        None => crate::access::admissible_grant_roles(role),
    }
}

/// A curator act (d9), as metrics label it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuratorAct {
    Reject,
    Retire,
    Flag,
    Write,
}

impl CuratorAct {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CuratorAct::Reject => "reject",
            CuratorAct::Retire => "retire",
            CuratorAct::Flag => "flag",
            CuratorAct::Write => "write",
        }
    }
}

/// Why a curator was refused (d9): `access.verb_ceiling` with this reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuratorRefusal {
    /// An approval, or any other review-class verb: a curator never widens.
    NarrowOnly,
    /// A row or target outside every in-progress subtree.
    OutsideInProgress,
}

impl CuratorRefusal {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CuratorRefusal::NarrowOnly => "curator_narrow_only",
            CuratorRefusal::OutsideInProgress => "curator_outside_in_progress",
        }
    }
}

/// The verbs a curator key may call at all (d9); everything else a dreamer
/// holds is [`CuratorRefusal::NarrowOnly`]. Reads are not acts.
pub fn curator_verb(verb: &str) -> Result<(), CuratorRefusal> {
    const ALLOWED: &[&str] = &[
        "recall",
        "get",
        "list",
        "workarea_info",
        "workarea_tree",
        "guardrails",
        "events_tail",
        "feedback",
        "claim_edge.list",
        "brief_edge.list",
        "workarea_status",
        "submit",
        "supersede",
        "verdict",
        "claim_edge.propose",
    ];
    if ALLOWED.contains(&verb) {
        Ok(())
    } else {
        Err(CuratorRefusal::NarrowOnly)
    }
}

/// A curator act on a row or target whose node has `root` (d9): refused
/// outside every in-progress subtree.
pub fn curator_scope(root: Option<Uuid>) -> Result<(), CuratorRefusal> {
    root.map(|_| ()).ok_or(CuratorRefusal::OutsideInProgress)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn the_root_is_the_nearest_marked_ancestor_or_self() {
        assert_eq!(
            in_progress_root(&[(id(3), false), (id(2), true), (id(1), false)]),
            Some(id(2))
        );
        assert_eq!(in_progress_root(&[(id(3), false), (id(1), false)]), None);
    }

    #[test]
    fn a_row_inside_crosses_into_a_scope_outside_or_under_another_root() {
        assert!(crosses_into(None, Some(id(2))));
        assert!(crosses_into(Some(id(9)), Some(id(2))));
        assert!(!crosses_into(Some(id(2)), Some(id(2))));
        assert!(!crosses_into(None, None));
        assert!(!crosses_into(Some(id(2)), None));
    }

    #[test]
    fn marking_refuses_universe_a_non_development_node_and_nesting() {
        assert_eq!(
            check_mark(&[(id(1), false)], false, false),
            Err(InProgressReason::Universe)
        );
        assert_eq!(
            check_mark(&[(id(2), false), (id(1), false)], false, false),
            Err(InProgressReason::Status)
        );
        assert_eq!(
            check_mark(
                &[(id(3), false), (id(2), true), (id(1), false)],
                false,
                true
            ),
            Err(InProgressReason::Nested)
        );
        assert_eq!(
            check_mark(&[(id(2), false), (id(1), false)], true, true),
            Err(InProgressReason::Nested)
        );
        assert_eq!(
            check_mark(&[(id(2), false), (id(1), false)], false, true),
            Ok(())
        );
    }

    #[test]
    fn only_a_dreamer_key_a_person_holds_sets_the_mode() {
        assert!(may_set_mode(Role::Dreamer, HeldBy::Human, None));
        for (role, held_by) in [
            (Role::Dreamer, HeldBy::Agent),
            (Role::Dreamer, HeldBy::Service),
            (Role::Admin, HeldBy::Human),
            (Role::Writer, HeldBy::Human),
            (Role::Reader, HeldBy::Human),
        ] {
            assert!(!may_set_mode(role, held_by, None), "{role:?} {held_by:?}");
        }
        assert!(!may_set_mode(
            Role::Dreamer,
            HeldBy::Human,
            Some(KeyProfile::Curator)
        ));
    }

    #[test]
    fn a_curator_is_a_dreamer_key_held_by_an_agent() {
        assert!(profile_admits(
            KeyProfile::Curator,
            Role::Dreamer,
            HeldBy::Agent
        ));
        assert!(!profile_admits(
            KeyProfile::Curator,
            Role::Dreamer,
            HeldBy::Human
        ));
        assert!(!profile_admits(
            KeyProfile::Curator,
            Role::Writer,
            HeldBy::Agent
        ));
    }

    #[test]
    fn a_curator_never_reaches_a_widening_verb() {
        for verb in [
            "review",
            "bulk_review",
            "workarea_mode",
            "propose_transition",
            "claim_edge.ratify",
            "brief_edge.propose",
        ] {
            assert_eq!(
                curator_verb(verb),
                Err(CuratorRefusal::NarrowOnly),
                "{verb}"
            );
        }
        assert_eq!(curator_scope(None), Err(CuratorRefusal::OutsideInProgress));
        assert_eq!(curator_scope(Some(id(1))), Ok(()));
    }
}
