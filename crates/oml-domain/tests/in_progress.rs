//! In-progress subtrees: whether a node is inside one, what its boundary
//! refuses, the precondition of its commit, and how its held hits merge with
//! the promoted ones before the budget cut (ADR-0030 d1, d2, d8, d9, d12,
//! proposed; workareas.md, "In-progress workareas" and the refusal matrix;
//! retrieval.md, "Held claims inside an in-progress workarea").

use std::collections::BTreeSet;

use oml_domain::MemoryStatus;
use oml_domain::budget::merge_hits;
use oml_domain::in_progress::{
    BoundaryRule, InProgressMark, InProgressRefusal, check_commit, check_create,
    check_reassignment, check_reference, check_reparent, check_transition, in_progress_root,
};
use proptest::prelude::*;
use uuid::Uuid;

/// A tenant with two in-progress workareas:
///
/// ```text
/// universe ─┬─ crew ─┬─ project* ─┬─ notes
///           │        │            └─ experiments ── runs
///           │        └─ archive
///           └─ ops ── sprint* ── sprint_sub
/// ```
///
/// `*` marks a node created with `in_progress: true`.
struct Tree {
    universe: Uuid,
    crew: Uuid,
    project: Uuid,
    notes: Uuid,
    experiments: Uuid,
    runs: Uuid,
    archive: Uuid,
    ops: Uuid,
    sprint: Uuid,
    sprint_sub: Uuid,
    /// A child of `project` created with `in_progress: true` after
    /// `project`'s commit: the next iteration.
    next: Uuid,
    next_sub: Uuid,
}

impl Tree {
    fn new() -> Self {
        Self {
            universe: Uuid::from_u128(0xB0),
            crew: Uuid::from_u128(0xB1),
            project: Uuid::from_u128(0xB2),
            notes: Uuid::from_u128(0xB3),
            experiments: Uuid::from_u128(0xB4),
            runs: Uuid::from_u128(0xB5),
            archive: Uuid::from_u128(0xB6),
            ops: Uuid::from_u128(0xB7),
            sprint: Uuid::from_u128(0xB8),
            sprint_sub: Uuid::from_u128(0xB9),
            next: Uuid::from_u128(0xBA),
            next_sub: Uuid::from_u128(0xBB),
        }
    }

    fn parent(&self, n: Uuid) -> Option<Uuid> {
        if n == self.crew || n == self.ops {
            Some(self.universe)
        } else if n == self.project || n == self.archive {
            Some(self.crew)
        } else if n == self.notes || n == self.experiments || n == self.next {
            Some(self.project)
        } else if n == self.runs {
            Some(self.experiments)
        } else if n == self.sprint {
            Some(self.ops)
        } else if n == self.sprint_sub {
            Some(self.sprint)
        } else if n == self.next_sub {
            Some(self.next)
        } else {
            None
        }
    }

    /// The ancestry of `node`, from the node to the root.
    fn ancestry(&self, node: Uuid) -> Vec<Uuid> {
        let mut chain = vec![node];
        let mut at = node;
        while let Some(p) = self.parent(at) {
            chain.push(p);
            at = p;
        }
        chain
    }

    /// Every node of `node`'s subtree, the node included.
    fn subtree(&self, node: Uuid) -> Vec<Uuid> {
        [
            self.universe,
            self.crew,
            self.project,
            self.notes,
            self.experiments,
            self.runs,
            self.archive,
            self.ops,
            self.sprint,
            self.sprint_sub,
            self.next,
            self.next_sub,
        ]
        .into_iter()
        .filter(|&n| self.ancestry(n).contains(&node))
        .collect()
    }

    /// Both in-progress workareas, neither committed.
    fn in_progress(&self) -> Vec<InProgressMark> {
        vec![mark(self.project, false), mark(self.sprint, false)]
    }

    /// `project`'s commit is journalled; `sprint` is still in progress.
    fn project_committed(&self) -> Vec<InProgressMark> {
        vec![mark(self.project, true), mark(self.sprint, false)]
    }
}

fn mark(workarea_id: Uuid, released: bool) -> InProgressMark {
    InProgressMark {
        workarea_id,
        released,
    }
}

fn refused(rule: BoundaryRule) -> Result<(), InProgressRefusal> {
    Err(InProgressRefusal::InProgressRefused(rule))
}

#[test]
fn a_node_is_inside_an_in_progress_subtree_until_its_roots_commit() {
    let t = Tree::new();
    let marks = t.in_progress();
    let root = |node: Uuid, marks: &[InProgressMark]| in_progress_root(&t.ancestry(node), marks);

    // The in-progress workarea is its own root, and every descendant, at any
    // depth, lies inside its subtree.
    for node in [t.project, t.notes, t.experiments, t.runs] {
        assert_eq!(root(node, &marks), Some(t.project), "{node}");
    }
    for node in [t.sprint, t.sprint_sub] {
        assert_eq!(root(node, &marks), Some(t.sprint), "{node}");
    }
    // Ancestors and siblings are outside: the mark reaches down, never up.
    for node in [t.universe, t.crew, t.archive, t.ops] {
        assert_eq!(root(node, &marks), None, "{node}");
    }
    // No mark, or an unknown node, is outside.
    assert_eq!(root(t.runs, &[]), None);
    assert_eq!(in_progress_root(&[], &marks), None);

    // The commit ends the subtree: the mark stays journalled, the root is
    // `committed`, its descendants are ordinary `development` workareas, and
    // the other subtree is unchanged.
    let committed = t.project_committed();
    for node in [t.project, t.notes, t.experiments, t.runs] {
        assert_eq!(root(node, &committed), None, "{node}");
    }
    assert_eq!(root(t.sprint_sub, &committed), Some(t.sprint));

    // The next iteration, created in-progress under the committed one, is a
    // subtree of its own: the nearest uncommitted mark is the root.
    let mut next = committed.clone();
    next.push(mark(t.next, false));
    assert_eq!(root(t.next_sub, &next), Some(t.next));
    assert_eq!(root(t.next, &next), Some(t.next));
    assert_eq!(root(t.notes, &next), None);
}

#[test]
fn a_transition_inside_an_in_progress_subtree_other_than_its_root_is_refused() {
    let t = Tree::new();
    let marks = t.in_progress();

    // The subtree moves up the ladder only at its root.
    assert_eq!(check_transition(&t.ancestry(t.project), &marks), Ok(()));
    assert_eq!(check_transition(&t.ancestry(t.sprint), &marks), Ok(()));
    for node in [t.notes, t.experiments, t.runs, t.sprint_sub] {
        assert_eq!(
            check_transition(&t.ancestry(node), &marks),
            refused(BoundaryRule::Transition),
            "{node}"
        );
    }
    // Nodes outside are judged by the ladder alone.
    for node in [t.universe, t.crew, t.archive, t.ops] {
        assert_eq!(
            check_transition(&t.ancestry(node), &marks),
            Ok(()),
            "{node}"
        );
    }
    // After the commit the descendants are ordinary workarea nodes again.
    let committed = t.project_committed();
    assert_eq!(check_transition(&t.ancestry(t.runs), &committed), Ok(()));
    assert_eq!(
        check_transition(&t.ancestry(t.sprint_sub), &committed),
        refused(BoundaryRule::Transition)
    );
}

#[test]
fn the_commit_is_refused_while_any_version_in_the_subtree_is_held() {
    let t = Tree::new();
    let held_remaining = Err(InProgressRefusal::HeldRemaining {
        workarea_id: t.project,
    });

    // One held version anywhere in the subtree refuses the proposal and,
    // under the same test, the ratification.
    assert_eq!(
        check_commit(
            t.project,
            [
                MemoryStatus::Promoted,
                MemoryStatus::Promoted,
                MemoryStatus::Held,
            ]
        ),
        held_remaining
    );
    // A dead-lettered version is still in state `held` (admission.md, dead
    // letter): it blocks until it is re-enqueued and reviewed, or erased.
    assert_eq!(
        check_commit(t.project, [MemoryStatus::Held]),
        held_remaining
    );
    assert_eq!(
        check_commit(t.project, [MemoryStatus::Held, MemoryStatus::Held]),
        held_remaining
    );

    // Nothing held: the commit proceeds.
    assert_eq!(check_commit(t.project, [MemoryStatus::Promoted]), Ok(()));
    assert_eq!(check_commit(t.project, []), Ok(()));
}

#[test]
fn a_rejected_version_does_not_block_the_commit() {
    let t = Tree::new();
    assert_eq!(check_commit(t.project, [MemoryStatus::Rejected]), Ok(()));
    // Nor does a demoted or an erased one.
    assert_eq!(
        check_commit(
            t.project,
            [
                MemoryStatus::Rejected,
                MemoryStatus::Promoted,
                MemoryStatus::Demoted,
                MemoryStatus::Erased,
            ]
        ),
        Ok(())
    );
    // A rejected version beside a held one does not make the held one pass.
    assert_eq!(
        check_commit(t.project, [MemoryStatus::Rejected, MemoryStatus::Held]),
        Err(InProgressRefusal::HeldRemaining {
            workarea_id: t.project
        })
    );
}

#[test]
fn a_reassignment_across_the_boundary_is_refused_and_a_root_reparent_moves_the_subtree() {
    let t = Tree::new();
    let marks = t.in_progress();
    let reassign = |from: Uuid, to: Uuid, marks: &[InProgressMark]| {
        check_reassignment(&t.ancestry(from), &t.ancestry(to), marks)
    };
    let reparent = |node: Uuid, new_parent: Uuid, marks: &[InProgressMark]| {
        check_reparent(
            &t.ancestry(node),
            &t.subtree(node),
            &t.ancestry(new_parent),
            marks,
        )
    };

    // Reassignment: between two nodes inside one subtree it is allowed;
    // into it, out of it, or from one subtree to another it is refused.
    assert_eq!(reassign(t.notes, t.runs, &marks), Ok(()));
    assert_eq!(reassign(t.runs, t.project, &marks), Ok(()));
    assert_eq!(
        reassign(t.notes, t.crew, &marks),
        refused(BoundaryRule::Reassignment)
    );
    assert_eq!(
        reassign(t.archive, t.notes, &marks),
        refused(BoundaryRule::Reassignment)
    );
    assert_eq!(
        reassign(t.notes, t.sprint_sub, &marks),
        refused(BoundaryRule::Reassignment)
    );
    assert_eq!(reassign(t.crew, t.archive, &marks), Ok(()));

    // Re-parenting the in-progress workarea moves its subtree whole.
    assert_eq!(reparent(t.project, t.ops, &marks), Ok(()));
    assert_eq!(reparent(t.project, t.archive, &marks), Ok(()));
    // So does moving an outside node whose subtree holds one, to a
    // destination outside every in-progress subtree.
    assert_eq!(reparent(t.crew, t.ops, &marks), Ok(()));
    // A node inside moves within its subtree only.
    assert_eq!(reparent(t.experiments, t.notes, &marks), Ok(()));
    assert_eq!(
        reparent(t.experiments, t.crew, &marks),
        refused(BoundaryRule::Reparent)
    );
    assert_eq!(
        reparent(t.runs, t.sprint, &marks),
        refused(BoundaryRule::Reparent)
    );
    // A node outside never moves in.
    assert_eq!(
        reparent(t.archive, t.notes, &marks),
        refused(BoundaryRule::Reparent)
    );

    // After the commit nothing is inside, so nothing crosses.
    let committed = t.project_committed();
    assert_eq!(reassign(t.notes, t.crew, &committed), Ok(()));
    assert_eq!(reparent(t.experiments, t.crew, &committed), Ok(()));
}

#[test]
fn an_in_progress_subtree_cannot_nest_or_hold_a_vocabulary_workarea() {
    let t = Tree::new();
    let marks = t.in_progress();
    let create = |parent: Uuid, in_progress: bool, prefix: bool, marks: &[InProgressMark]| {
        check_create(&t.ancestry(parent), in_progress, prefix, marks)
    };
    let reparent = |node: Uuid, new_parent: Uuid, marks: &[InProgressMark]| {
        check_reparent(
            &t.ancestry(node),
            &t.subtree(node),
            &t.ancestry(new_parent),
            marks,
        )
    };

    // A plain child inside is allowed: it is inside the subtree.
    assert_eq!(create(t.notes, false, false, &marks), Ok(()));
    // An in-progress child inside would nest.
    assert_eq!(
        create(t.project, true, false, &marks),
        refused(BoundaryRule::Nesting)
    );
    assert_eq!(
        create(t.runs, true, false, &marks),
        refused(BoundaryRule::Nesting)
    );
    // A vocabulary child inside is refused at the boundary; an in-progress
    // workarea that is itself a vocabulary is a malformed body, wherever it
    // is created.
    assert_eq!(
        create(t.notes, false, true, &marks),
        refused(BoundaryRule::Prefix)
    );
    assert_eq!(
        create(t.crew, true, true, &marks),
        Err(InProgressRefusal::InProgressWithPrefix)
    );
    assert_eq!(
        create(t.notes, true, true, &marks),
        Err(InProgressRefusal::InProgressWithPrefix)
    );
    // Outside every in-progress subtree, this rule refuses neither.
    assert_eq!(create(t.crew, true, false, &marks), Ok(()));
    assert_eq!(create(t.archive, false, true, &marks), Ok(()));

    // A re-parent never nests: neither the in-progress workarea nor a node
    // whose subtree holds one moves inside another subtree.
    assert_eq!(
        reparent(t.project, t.sprint, &marks),
        refused(BoundaryRule::Nesting)
    );
    assert_eq!(
        reparent(t.project, t.sprint_sub, &marks),
        refused(BoundaryRule::Nesting)
    );
    assert_eq!(
        reparent(t.crew, t.sprint_sub, &marks),
        refused(BoundaryRule::Nesting)
    );

    // After the commit the former subtree may hold the next iteration.
    let committed = t.project_committed();
    assert_eq!(create(t.project, true, false, &committed), Ok(()));
    assert_eq!(create(t.notes, false, true, &committed), Ok(()));
}

#[test]
fn a_lineage_or_claim_edge_across_the_boundary_is_refused() {
    let t = Tree::new();
    let marks = t.in_progress();
    let reference = |named: Uuid, from: Uuid, marks: &[InProgressMark]| {
        check_reference(&t.ancestry(named), &t.ancestry(from), marks)
    };

    // A target inside, named from inside the same subtree: admitted.
    assert_eq!(reference(t.notes, t.runs, &marks), Ok(()));
    // A target inside, named from outside it or from another subtree.
    assert_eq!(
        reference(t.notes, t.crew, &marks),
        refused(BoundaryRule::Lineage)
    );
    assert_eq!(
        reference(t.notes, t.sprint_sub, &marks),
        refused(BoundaryRule::Lineage)
    );
    // A target outside, named from inside: nothing inside leaves.
    assert_eq!(reference(t.crew, t.notes, &marks), Ok(()));
    // An edge is judged from each endpoint, so one inside and one outside
    // is refused whichever is the source.
    assert_eq!(
        reference(t.notes, t.archive, &marks).and_then(|()| reference(t.archive, t.notes, &marks)),
        refused(BoundaryRule::Lineage)
    );
    // After the commit nothing is inside.
    assert_eq!(reference(t.notes, t.crew, &t.project_committed()), Ok(()));
}

/// A hit candidate: its score, its version's `artifact_id`, its count in
/// budget tokens and whether it is held.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hit {
    score: f32,
    id: Uuid,
    tokens: u64,
    held: bool,
}

proptest! {
    #[test]
    fn held_and_promoted_hits_merge_under_one_total_order_before_the_budget_cut(
        // Distinct versions by artifact_id, each with a score from a small
        // set so that ties are common.
        versions in proptest::collection::btree_map(
            0_u128..40,
            (0_u8..4, 0_u64..200, any::<bool>()),
            0..16,
        ),
        twice in proptest::collection::vec(any::<prop::sample::Index>(), 0..4),
        steering in proptest::collection::vec(0_u64..100, 0..3),
        k in 0_usize..20,
        max in proptest::option::of(0_u64..1_500),
    ) {
        let hits: Vec<Hit> = versions
            .iter()
            .map(|(&id, &(score, tokens, held))| Hit {
                score: f32::from(score) / 4.0,
                id: Uuid::from_u128(id),
                tokens,
                held,
            })
            .collect();
        let promoted: Vec<Hit> = hits.iter().copied().filter(|h| !h.held).collect();
        let mut held: Vec<Hit> = hits.iter().copied().filter(|h| h.held).collect();
        // A key's own held row inside is reached by both `include_unreviewed`
        // and `include_own_held`, so a held version can arrive twice.
        if !held.is_empty() {
            let again: Vec<Hit> = twice.iter().map(|i| held[i.index(held.len())]).collect();
            held.extend(again);
        }

        // The expected answer, computed here: every version once, score
        // descending then artifact_id ascending, whatever its state, then the
        // k cap, then the longest prefix that fits what steering leaves.
        let mut expected = hits.clone();
        expected.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        expected.truncate(k);
        let served_len = match max {
            None => expected.len(),
            Some(max) => {
                let mut remaining = max.saturating_sub(steering.iter().sum());
                expected
                    .iter()
                    .take_while(|h| {
                        let fits = h.tokens <= remaining;
                        if fits {
                            remaining -= h.tokens;
                        }
                        fits
                    })
                    .count()
            }
        };

        let rank = |h: &Hit| (h.score, h.id);
        let tokens = |h: &Hit| h.tokens;
        let (served, budget) =
            merge_hits(promoted.clone(), held.clone(), k, &steering, max, rank, tokens);

        prop_assert_eq!(&served[..], &expected[..served_len]);
        let ids: BTreeSet<Uuid> = served.iter().map(|h| h.id).collect();
        prop_assert_eq!(ids.len(), served.len(), "nothing is served twice");
        prop_assert_eq!(budget.is_some(), max.is_some());
        if let (Some(max), Some(budget)) = (max, budget) {
            prop_assert_eq!(budget.max_tokens, max);
            prop_assert_eq!(budget.hits_served, served_len);
            // Held and promoted hits count alike against the budget.
            prop_assert_eq!(budget.hits_omitted, expected.len() - served_len);
            prop_assert_eq!(
                budget.next_hit_tokens,
                expected.get(served_len).map(|h| h.tokens)
            );
        }

        // The state never moves a hit: ratifying every held hit, which
        // changes no score (ADR-0030 d9), serves the same versions in the
        // same order.
        let ratified: Vec<Hit> = promoted
            .iter()
            .chain(hits.iter().filter(|h| h.held))
            .map(|h| Hit { held: false, ..*h })
            .collect();
        let (after, _) = merge_hits(ratified, Vec::new(), k, &steering, max, rank, tokens);
        let before: Vec<Uuid> = served.iter().map(|h| h.id).collect();
        let after: Vec<Uuid> = after.iter().map(|h| h.id).collect();
        prop_assert_eq!(before, after);
    }
}
