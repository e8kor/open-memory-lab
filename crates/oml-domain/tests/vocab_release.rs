//! Vocabulary releases: the snapshot a `vocab_release` ratification
//! materializes — term tips, ratified `is_a` and `replaced_by` edges and the
//! `is_a` closure — as one RFC 8785 document with its SHA-256 digest, and the
//! diff a release's serve shows against the previous release (ADR-0021 d4,
//! d5; ontologies.md; postgres-schema.md `vocab_closure`; chain-format.md).

use std::collections::{BTreeMap, BTreeSet};

use oml_domain::vocab::{Curie, OmlLabel, Successors};
use oml_domain::vocab_edges::{
    ClosureRow, CycleRefused, ExpandTerms, TermEdge, TermTip, TipState, expand_terms,
};
use oml_domain::vocab_release::{
    ReleaseClosureRow, ReleaseDiff, ReleaseDiffRefusal, ReleaseRefusal, ReleaseSnapshot,
    build_release, diff_releases,
};
use oml_domain::{Sha256Hash, canonical_bytes};
use proptest::prelude::*;

const RISK: &str = "biz:risk";
const CHURN: &str = "biz:churn-risk";
const VOLUNTARY: &str = "biz:voluntary-churn";
const ATTRITION: &str = "biz:attrition";

fn curie(text: &str) -> Curie {
    Curie::parse(text).expect("a well-formed CURIE")
}

fn curies(texts: &[&str]) -> BTreeSet<Curie> {
    texts.iter().map(|t| curie(t)).collect()
}

fn tip(text: &str, state: TipState) -> TermTip {
    TermTip {
        curie: curie(text),
        state,
    }
}

fn active(text: &str) -> TermTip {
    tip(text, TipState::Active)
}

fn replaced(successor: &str) -> TipState {
    TipState::Deprecated(Successors::ReplacedBy(curie(successor)))
}

fn term_edge(label: OmlLabel, from: &str, to: &str) -> TermEdge {
    TermEdge {
        label,
        from: curie(from),
        to: curie(to),
    }
}

fn is_a(from: &str, to: &str) -> TermEdge {
    term_edge(OmlLabel::IsA, from, to)
}

fn replaced_by(from: &str, to: &str) -> TermEdge {
    term_edge(OmlLabel::ReplacedBy, from, to)
}

fn row(ancestor: &str, descendant: &str, depth: u32) -> ReleaseClosureRow {
    ReleaseClosureRow {
        ancestor: curie(ancestor),
        descendant: curie(descendant),
        depth,
    }
}

fn build(tips: &[TermTip], edges: &[TermEdge]) -> ReleaseSnapshot {
    build_release(tips, edges).expect("an acyclic release")
}

#[test]
fn a_release_materializes_the_is_a_closure_at_each_rows_shortest_depth() {
    // biz:voluntary-churn is_a biz:churn-risk is_a biz:risk, and a ratified
    // shortcut puts biz:voluntary-churn directly below biz:risk too.
    let tips = [active(RISK), active(CHURN), active(VOLUNTARY)];
    let edges = [
        is_a(VOLUNTARY, CHURN),
        is_a(CHURN, RISK),
        is_a(VOLUNTARY, RISK),
        // The same CURIE pair from a second pair of versions is one edge.
        is_a(CHURN, RISK),
        // Only is_a and replaced_by enter a release.
        term_edge(OmlLabel::PartOf, CHURN, RISK),
    ];
    let release = build(&tips, &edges);

    assert_eq!(
        release.closure(),
        &BTreeSet::from([
            row(CHURN, VOLUNTARY, 1),
            row(RISK, CHURN, 1),
            row(RISK, VOLUNTARY, 1),
        ])
    );
    assert_eq!(
        release.edges(),
        &BTreeSet::from([
            is_a(CHURN, RISK),
            is_a(VOLUNTARY, CHURN),
            is_a(VOLUNTARY, RISK)
        ])
    );
    assert_eq!(release.terms().len(), 3);

    // Without the shortcut the deeper row is two hops.
    let deep = build(&tips, &[is_a(VOLUNTARY, CHURN), is_a(CHURN, RISK)]);
    assert!(deep.closure().contains(&row(RISK, VOLUNTARY, 2)));

    // The rows are the ones expand_terms consumes, descendants only.
    let rows: Vec<ClosureRow> = release.closure_rows();
    assert_eq!(
        rows,
        vec![
            ClosureRow {
                ancestor: curie(CHURN),
                descendant: curie(VOLUNTARY)
            },
            ClosureRow {
                ancestor: curie(RISK),
                descendant: curie(CHURN)
            },
            ClosureRow {
                ancestor: curie(RISK),
                descendant: curie(VOLUNTARY)
            },
        ]
    );
    assert_eq!(
        expand_terms(&curies(&[RISK]), ExpandTerms::Descendants, &rows),
        curies(&[RISK, CHURN, VOLUNTARY])
    );
    assert_eq!(
        expand_terms(&curies(&[VOLUNTARY]), ExpandTerms::Descendants, &rows),
        curies(&[VOLUNTARY])
    );
}

/// The canonical document of the vector below, written out by hand.
const VECTOR_DOCUMENT: &str = concat!(
    r#"{"closure":["#,
    r#"{"ancestor":"biz:churn-risk","depth":1,"descendant":"biz:voluntary-churn"},"#,
    r#"{"ancestor":"biz:risk","depth":1,"descendant":"biz:churn-risk"},"#,
    r#"{"ancestor":"biz:risk","depth":2,"descendant":"biz:voluntary-churn"}],"#,
    r#""edges":["#,
    r#"{"from":"biz:churn-risk","label":"oml:is_a","to":"biz:risk"},"#,
    r#"{"from":"biz:voluntary-churn","label":"oml:is_a","to":"biz:churn-risk"},"#,
    r#"{"from":"biz:attrition","label":"oml:replaced_by","to":"biz:churn-risk"}],"#,
    r#""terms":["#,
    r#"{"curie":"biz:attrition","successors":[{"curie":"biz:churn-risk","relation":"replaced_by"}],"term_status":"deprecated"},"#,
    r#"{"curie":"biz:churn-risk","successors":[],"term_status":"active"},"#,
    r#"{"curie":"biz:risk","successors":[],"term_status":"active"},"#,
    r#"{"curie":"biz:voluntary-churn","successors":[],"term_status":"active"}]}"#,
);

/// SHA-256 of [`VECTOR_DOCUMENT`]'s bytes, computed with coreutils
/// `sha256sum` over the literal.
const VECTOR_DIGEST: &str =
    "sha256:0b0b0b46d05925886a307d3b28acd2d8936e5b5f2ae274b96a646675ad2f41f2";

#[test]
fn a_release_is_one_canonical_document_with_its_sha256_digest() {
    let release = build(
        &[
            active(VOLUNTARY),
            tip(ATTRITION, replaced(CHURN)),
            active(RISK),
            active(CHURN),
        ],
        &[
            replaced_by(ATTRITION, CHURN),
            is_a(VOLUNTARY, CHURN),
            is_a(CHURN, RISK),
        ],
    );
    let bytes = release.canonical_bytes();
    assert_eq!(std::str::from_utf8(bytes), Ok(VECTOR_DOCUMENT));

    // RFC 8785: the bytes re-canonicalize to themselves (chain-format.md,
    // verification step 6).
    let parsed: serde_json::Value = serde_json::from_slice(bytes).expect("JSON");
    assert_eq!(canonical_bytes(&parsed).as_deref(), Ok(bytes));

    // The digest is SHA-256 of those bytes, spelled `sha256:` + 64 lowercase
    // hex (chain-format.md, "Digests are strings, everywhere").
    assert_eq!(release.digest(), Sha256Hash::digest(bytes));
    assert_eq!(release.digest().to_prefixed(), VECTOR_DIGEST);
}

#[test]
fn an_is_a_or_replaced_by_cycle_in_a_release_is_refused() {
    let tips = [active(RISK), active(CHURN)];
    assert_eq!(
        build_release(&tips, &[is_a(RISK, CHURN), is_a(CHURN, RISK)]),
        Err(ReleaseRefusal::Cycle(CycleRefused {
            label: OmlLabel::IsA
        }))
    );
    assert_eq!(
        build_release(
            &[active(RISK), tip(CHURN, replaced(CHURN))],
            &[replaced_by(CHURN, CHURN)]
        ),
        Err(ReleaseRefusal::Cycle(CycleRefused {
            label: OmlLabel::ReplacedBy
        }))
    );
    // Each invariant runs over its own relation: a loop that mixes the
    // labels closes neither, and part_of carries no invariant.
    assert!(
        build_release(
            &[active(RISK), tip(CHURN, replaced(RISK))],
            &[is_a(RISK, CHURN), replaced_by(CHURN, RISK)]
        )
        .is_ok()
    );
    assert!(
        build_release(
            &tips,
            &[
                term_edge(OmlLabel::PartOf, RISK, CHURN),
                term_edge(OmlLabel::PartOf, CHURN, RISK)
            ]
        )
        .is_ok()
    );
}

#[test]
fn a_tip_replaced_by_itself_or_a_tip_cycle_is_refused() {
    // A tip replaced by itself, alone or with the self-edge it implies.
    let selfish = [tip(CHURN, replaced(CHURN))];
    assert_eq!(
        build_release(&selfish, &[]),
        Err(ReleaseRefusal::SuccessorEdgeMismatch {
            curie: curie(CHURN)
        })
    );
    assert_eq!(
        build_release(&selfish, &[replaced_by(CHURN, CHURN)]),
        Err(ReleaseRefusal::Cycle(CycleRefused {
            label: OmlLabel::ReplacedBy
        }))
    );
    // Two tips replaced by each other, alone or with their edges.
    let pair = [tip(RISK, replaced(CHURN)), tip(CHURN, replaced(RISK))];
    assert_eq!(
        build_release(&pair, &[]),
        Err(ReleaseRefusal::SuccessorEdgeMismatch {
            curie: curie(CHURN)
        })
    );
    assert_eq!(
        build_release(&pair, &[replaced_by(RISK, CHURN), replaced_by(CHURN, RISK)]),
        Err(ReleaseRefusal::Cycle(CycleRefused {
            label: OmlLabel::ReplacedBy
        }))
    );
}

#[test]
fn a_tip_successor_that_no_replaced_by_edge_carries_is_refused() {
    let tips = [active(RISK), active(CHURN), tip(ATTRITION, replaced(CHURN))];
    let mismatch = Err(ReleaseRefusal::SuccessorEdgeMismatch {
        curie: curie(ATTRITION),
    });
    // The tip names a successor and no edge carries it.
    assert_eq!(build_release(&tips, &[]), mismatch);
    // The edge names another successor than the tip does.
    assert_eq!(
        build_release(&tips, &[replaced_by(ATTRITION, RISK)]),
        mismatch
    );
    assert_eq!(
        build_release(
            &tips,
            &[replaced_by(ATTRITION, CHURN), replaced_by(ATTRITION, RISK)]
        ),
        mismatch
    );
    // A replaced_by edge out of an active term, a consider term, or a term
    // with no tip.
    let consider = tip(
        VOLUNTARY,
        TipState::Deprecated(Successors::Consider(vec![curie(RISK)])),
    );
    let all = [
        active(RISK),
        active(CHURN),
        tip(ATTRITION, replaced(CHURN)),
        consider,
    ];
    let carried = replaced_by(ATTRITION, CHURN);
    for (stray, named) in [
        (replaced_by(CHURN, RISK), CHURN),
        (replaced_by(VOLUNTARY, RISK), VOLUNTARY),
        (replaced_by("biz:absent", RISK), "biz:absent"),
    ] {
        assert_eq!(
            build_release(&all, &[carried.clone(), stray]),
            Err(ReleaseRefusal::SuccessorEdgeMismatch {
                curie: curie(named)
            })
        );
    }
    // The tip and its edge together are one release.
    assert!(build_release(&all, &[carried]).is_ok());
}

#[test]
fn two_tips_for_one_curie_are_refused() {
    for second in [active(CHURN), tip(CHURN, replaced(RISK))] {
        assert_eq!(
            build_release(&[active(RISK), active(CHURN), second], &[]),
            Err(ReleaseRefusal::TwoTips {
                curie: curie(CHURN)
            })
        );
    }
}

#[test]
fn a_consider_list_is_held_sorted_and_once_each() {
    let consider = |names: &[&str]| {
        tip(
            ATTRITION,
            TipState::Deprecated(Successors::Consider(
                names.iter().map(|n| curie(n)).collect(),
            )),
        )
    };
    let unsorted = build(&[consider(&[RISK, CHURN, RISK])], &[]);
    let sorted = build(&[consider(&[CHURN, RISK])], &[]);
    assert_eq!(unsorted, sorted);
    assert_eq!(unsorted.digest(), sorted.digest());
    assert_eq!(
        unsorted.terms().get(&curie(ATTRITION)),
        Some(&TipState::Deprecated(Successors::Consider(vec![
            curie(CHURN),
            curie(RISK)
        ])))
    );
}

#[test]
fn a_diff_names_terms_added_deprecations_with_their_successors_and_edge_and_closure_changes() {
    let previous = build(
        &[active(RISK), active(CHURN), active(ATTRITION)],
        &[is_a(CHURN, RISK), is_a(ATTRITION, RISK)],
    );
    let next = build(
        &[
            active(RISK),
            active(CHURN),
            tip(ATTRITION, replaced(CHURN)),
            active(VOLUNTARY),
        ],
        &[
            is_a(CHURN, RISK),
            is_a(VOLUNTARY, CHURN),
            replaced_by(ATTRITION, CHURN),
        ],
    );
    let diff = diff_releases(&previous, &next).expect("next evolves from previous");
    assert_eq!(
        diff,
        ReleaseDiff {
            terms_added: BTreeMap::from([(curie(VOLUNTARY), TipState::Active)]),
            deprecations: BTreeMap::from([(
                curie(ATTRITION),
                Successors::ReplacedBy(curie(CHURN))
            )]),
            edges_added: BTreeSet::from([is_a(VOLUNTARY, CHURN), replaced_by(ATTRITION, CHURN)]),
            edges_removed: BTreeSet::from([is_a(ATTRITION, RISK)]),
            closure_added: BTreeSet::from([row(CHURN, VOLUNTARY, 1), row(RISK, VOLUNTARY, 2)]),
            closure_removed: BTreeSet::from([row(RISK, ATTRITION, 1)]),
        }
    );
    assert!(!diff.is_empty());

    // A deprecated term whose deprecation is unchanged is no deprecation.
    let unchanged = build(
        &[
            active(RISK),
            active(CHURN),
            tip(ATTRITION, replaced(CHURN)),
            active(VOLUNTARY),
            active("biz:turnover"),
        ],
        &[
            is_a(CHURN, RISK),
            is_a(VOLUNTARY, CHURN),
            replaced_by(ATTRITION, CHURN),
        ],
    );
    assert_eq!(
        diff_releases(&next, &unchanged),
        Ok(ReleaseDiff {
            terms_added: BTreeMap::from([(curie("biz:turnover"), TipState::Active)]),
            ..ReleaseDiff::default()
        })
    );

    // A term first seen already deprecated is a term added, with its state.
    let born_deprecated = build(
        &[active(RISK), tip(CHURN, replaced(RISK))],
        &[replaced_by(CHURN, RISK)],
    );
    let diff = diff_releases(&build(&[active(RISK)], &[]), &born_deprecated).expect("added");
    assert_eq!(
        diff.terms_added,
        BTreeMap::from([(curie(CHURN), replaced(RISK))])
    );
    assert!(diff.deprecations.is_empty());
}

#[test]
fn a_diff_refuses_a_later_release_that_drops_or_reinstates_a_term() {
    let previous = build(
        &[active(RISK), tip(ATTRITION, replaced(RISK))],
        &[replaced_by(ATTRITION, RISK)],
    );
    assert_eq!(
        diff_releases(&previous, &build(&[active(RISK)], &[])),
        Err(ReleaseDiffRefusal::TermDropped {
            curie: curie(ATTRITION)
        })
    );
    assert_eq!(
        diff_releases(&previous, &build(&[active(RISK), active(ATTRITION)], &[])),
        Err(ReleaseDiffRefusal::TermReinstated {
            curie: curie(ATTRITION)
        })
    );
}

#[test]
fn a_diff_refuses_a_later_release_that_changes_a_deprecated_terms_successors() {
    let previous = build(
        &[active(RISK), active(CHURN), tip(ATTRITION, replaced(CHURN))],
        &[replaced_by(ATTRITION, CHURN)],
    );
    let redirected = build(
        &[active(RISK), active(CHURN), tip(ATTRITION, replaced(RISK))],
        &[replaced_by(ATTRITION, RISK)],
    );
    let reconsidered = build(
        &[
            active(RISK),
            active(CHURN),
            tip(
                ATTRITION,
                TipState::Deprecated(Successors::Consider(vec![curie(RISK), curie(CHURN)])),
            ),
        ],
        &[],
    );
    for next in [&redirected, &reconsidered] {
        assert_eq!(
            diff_releases(&previous, next),
            Err(ReleaseDiffRefusal::TermRedeprecated {
                curie: curie(ATTRITION)
            })
        );
    }
}

// --- properties -----------------------------------------------------------

/// Terms of the property releases are `t:n0` .. `t:n{N-1}`.
const N: u8 = 6;

fn node(i: u8) -> Curie {
    curie(&format!("t:n{i}"))
}

/// Shortest `is_a` distances by Floyd–Warshall over `pairs` (`(i, j)`: node
/// i is_a node j), with no zero-length path: `dist[i][i]` is set only when i
/// lies on a cycle.
fn shortest(pairs: &[(u8, u8)]) -> Vec<Vec<Option<u32>>> {
    let n = usize::from(N);
    let mut dist = vec![vec![None; n]; n];
    for &(i, j) in pairs {
        dist[usize::from(i)][usize::from(j)] = Some(1);
    }
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                if let (Some(a), Some(b)) = (dist[i][k], dist[k][j]) {
                    let through = a + b;
                    if dist[i][j].is_none_or(|d| through < d) {
                        dist[i][j] = Some(through);
                    }
                }
            }
        }
    }
    dist
}

#[derive(Debug, Clone)]
enum Spec {
    Active,
    ReplacedBy(u8),
    Consider(Vec<u8>),
}

impl Spec {
    fn state(&self) -> TipState {
        match self {
            Spec::Active => TipState::Active,
            Spec::ReplacedBy(j) => TipState::Deprecated(Successors::ReplacedBy(node(*j))),
            Spec::Consider(js) => {
                TipState::Deprecated(Successors::Consider(js.iter().map(|j| node(*j)).collect()))
            }
        }
    }

    fn is_deprecated(&self) -> bool {
        !matches!(self, Spec::Active)
    }
}

fn arb_spec() -> impl Strategy<Value = Spec> {
    prop_oneof![
        Just(Spec::Active),
        (0..N).prop_map(Spec::ReplacedBy),
        prop::collection::vec(0..N, 1..4).prop_map(Spec::Consider),
    ]
}

/// `is_a` and `part_of` edges, each from a higher node to a lower, so no
/// label's graph holds a cycle; the `replaced_by` edges come from the tips
/// ([`release_input`]).
fn arb_edges() -> impl Strategy<Value = Vec<TermEdge>> {
    let labels = [OmlLabel::IsA, OmlLabel::PartOf];
    prop::collection::vec((0..labels.len(), 0..N, 0..N), 0..16).prop_map(move |raw| {
        raw.into_iter()
            .filter(|(_, i, j)| i != j)
            .map(|(l, i, j)| TermEdge {
                label: labels[l],
                from: node(i.max(j)),
                to: node(i.min(j)),
            })
            .collect()
    })
}

type ReleaseInput = (Vec<TermTip>, Vec<TermEdge>);

/// One release's input from a spec per node and its `is_a` and `part_of`
/// edges: a `replaced_by` tip pointing at a lower node rides with its edge,
/// as its deprecating submission declares it, and one pointing at itself or
/// a higher node is held as `consider` instead, so no `replaced_by` cycle
/// arises. A spec yields the same state wherever it is used.
fn release_input(specs: &[Option<Spec>], edges: Vec<TermEdge>) -> ReleaseInput {
    let mut edges = edges;
    let tips = (0..N)
        .zip(specs)
        .filter_map(|(i, spec)| {
            let spec = match spec.as_ref()? {
                Spec::ReplacedBy(j) if *j >= i => Spec::Consider(vec![*j]),
                spec => spec.clone(),
            };
            if let Spec::ReplacedBy(j) = spec {
                edges.push(TermEdge {
                    label: OmlLabel::ReplacedBy,
                    from: node(i),
                    to: node(j),
                });
            }
            Some(TermTip {
                curie: node(i),
                state: spec.state(),
            })
        })
        .collect();
    (tips, edges)
}

/// One release's input: a tip for some of the terms, and its edges.
fn arb_release() -> impl Strategy<Value = ReleaseInput> {
    (
        prop::collection::vec(prop::option::of(arb_spec()), usize::from(N)),
        arb_edges(),
    )
        .prop_map(|(specs, edges)| release_input(&specs, edges))
}

/// Two releases' inputs, the later evolved from the earlier as the term
/// lifecycle allows: every earlier term is kept, a deprecated one stays
/// deprecated with its successors unchanged, an active one may be deprecated,
/// terms may be added, and the edges are drawn afresh.
fn arb_release_pair() -> impl Strategy<Value = (ReleaseInput, ReleaseInput)> {
    (
        prop::collection::vec(prop::option::of(arb_spec()), usize::from(N)),
        prop::collection::vec(prop::option::of(arb_spec()), usize::from(N)),
        arb_edges(),
        arb_edges(),
    )
        .prop_map(|(earlier, later, earlier_edges, later_edges)| {
            let evolved: Vec<Option<Spec>> = earlier
                .iter()
                .zip(later)
                .map(|(before, after)| match (before, after) {
                    (Some(before), None) => Some(before.clone()),
                    (Some(before), Some(_)) if before.is_deprecated() => Some(before.clone()),
                    (_, after) => after,
                })
                .collect();
            (
                release_input(&earlier, earlier_edges),
                release_input(&evolved, later_edges),
            )
        })
}

/// What applying `diff` to `release` yields: its terms, edges and closure
/// rows with the diff's changes made, each change meeting the state it
/// names.
fn apply(
    release: &ReleaseSnapshot,
    diff: &ReleaseDiff,
) -> (
    BTreeMap<Curie, TipState>,
    BTreeSet<TermEdge>,
    BTreeSet<ReleaseClosureRow>,
) {
    let mut terms = release.terms().clone();
    for (curie, state) in &diff.terms_added {
        assert!(terms.insert(curie.clone(), state.clone()).is_none());
    }
    for (curie, successors) in &diff.deprecations {
        let before = terms.insert(curie.clone(), TipState::Deprecated(successors.clone()));
        assert!(before.is_some());
    }
    let mut edges = release.edges().clone();
    for edge in &diff.edges_removed {
        assert!(edges.remove(edge));
    }
    for edge in &diff.edges_added {
        assert!(edges.insert(edge.clone()));
    }
    let mut closure = release.closure().clone();
    for row in &diff.closure_removed {
        assert!(closure.remove(row));
    }
    for row in &diff.closure_added {
        assert!(closure.insert(row.clone()));
    }
    (terms, edges, closure)
}

proptest! {
    #[test]
    fn a_release_closure_is_transitive_and_acyclic(
        pairs in prop::collection::vec((0..N, 0..N), 0..20),
    ) {
        let tips: Vec<TermTip> = (0..N)
            .map(|i| TermTip { curie: node(i), state: TipState::Active })
            .collect();
        let edges: Vec<TermEdge> = pairs
            .iter()
            .map(|&(i, j)| TermEdge { label: OmlLabel::IsA, from: node(i), to: node(j) })
            .collect();
        let dist = shortest(&pairs);
        let cyclic = (0..usize::from(N)).any(|i| dist[i][i].is_some());

        match build_release(&tips, &edges) {
            Err(refusal) => {
                prop_assert!(cyclic);
                prop_assert_eq!(
                    refusal,
                    ReleaseRefusal::Cycle(CycleRefused { label: OmlLabel::IsA })
                );
            }
            Ok(release) => {
                prop_assert!(!cyclic);
                let below: BTreeSet<(Curie, Curie)> = release
                    .closure()
                    .iter()
                    .map(|r| (r.ancestor.clone(), r.descendant.clone()))
                    .collect();
                prop_assert_eq!(below.len(), release.closure().len());
                for (ancestor, descendant) in &below {
                    // Acyclic: no term lies below itself, and no pair runs
                    // both ways.
                    prop_assert_ne!(ancestor, descendant);
                    prop_assert!(!below.contains(&(descendant.clone(), ancestor.clone())));
                    // Transitive: below something below the ancestor is
                    // below the ancestor.
                    for (middle, lower) in &below {
                        if middle == descendant {
                            prop_assert!(below.contains(&(ancestor.clone(), lower.clone())));
                        }
                    }
                }
                // The rows are exactly is_a reachability, each at its
                // shortest depth.
                let mut expected = BTreeSet::new();
                for i in 0..N {
                    for j in 0..N {
                        if let Some(depth) = dist[usize::from(i)][usize::from(j)] {
                            expected.insert(ReleaseClosureRow {
                                ancestor: node(j),
                                descendant: node(i),
                                depth,
                            });
                        }
                    }
                }
                prop_assert_eq!(release.closure(), &expected);
            }
        }
    }

    #[test]
    fn the_diff_of_a_release_with_itself_is_empty((tips, edges) in arb_release()) {
        let release = build(&tips, &edges);
        let diff = diff_releases(&release, &release);
        prop_assert_eq!(&diff, &Ok(ReleaseDiff::default()));
        prop_assert!(diff.is_ok_and(|d| d.is_empty()));
    }

    #[test]
    fn applying_the_diff_of_two_releases_to_the_first_yields_the_second(
        ((earlier_tips, earlier_edges), (later_tips, later_edges)) in arb_release_pair(),
    ) {
        let earlier = build(&earlier_tips, &earlier_edges);
        let later = build(&later_tips, &later_edges);
        let diff = diff_releases(&earlier, &later).expect("the later evolves from the earlier");

        let (terms, edges, closure) = apply(&earlier, &diff);
        prop_assert_eq!(&closure, later.closure());
        let tips: Vec<TermTip> = terms
            .into_iter()
            .map(|(curie, state)| TermTip { curie, state })
            .collect();
        let edges: Vec<TermEdge> = edges.into_iter().collect();
        prop_assert_eq!(build(&tips, &edges), later);
    }

    #[test]
    fn the_release_digest_is_stable_across_input_order(
        ((tips, edges), (shuffled_tips, shuffled_edges)) in arb_release().prop_flat_map(
            |(tips, edges)| {
                let shuffled = (Just(tips.clone()).prop_shuffle(), Just(edges.clone()).prop_shuffle());
                (Just((tips, edges)), shuffled)
            },
        ),
    ) {
        let release = build(&tips, &edges);
        let shuffled = build(&shuffled_tips, &shuffled_edges);
        prop_assert_eq!(release.canonical_bytes(), shuffled.canonical_bytes());
        prop_assert_eq!(release.digest(), shuffled.digest());
        prop_assert_eq!(release, shuffled);
    }
}
