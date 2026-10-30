//! Claim edges over vocabularies: the edge states, classification
//! validation and its duplicate rule, cycle refusal over `is_a` and
//! `replaced_by`, the lineage closure without `oml:about`, computed
//! retirement, and recall's terms match (ADR-0021 d3, d5, d9;
//! ADR-0027 d6–d11; ontologies.md, retrieval.md, audit-and-lineage.md).

use std::collections::{BTreeMap, BTreeSet};

use oml_domain::vocab::{Curie, OmlLabel, Successors, TermStatus};
use oml_domain::vocab_edges::{
    ClaimEdge, ClassificationRefusal, ClosureRow, CycleRefused, DerivesFrom, EdgeState,
    EdgeTransitionError, Endpoint, EndpointVersion, ExistingClassification, ExpandTerms, TermEdge,
    TermTip, TipState, check_acyclic, check_classification, check_live_classification,
    check_term_active, edge_transition, expand_terms, is_retired, lineage_artifacts, matches_terms,
};
use oml_domain::{MemoryKind, MemoryStatus};
use proptest::prelude::*;
use uuid::Uuid;

const CHURN: &str = "biz:churn-risk";
const ATTRITION: &str = "biz:attrition-risk";

fn curie(text: &str) -> Curie {
    Curie::parse(text).expect("a well-formed CURIE")
}

fn curies(texts: &[&str]) -> BTreeSet<Curie> {
    texts.iter().map(|t| curie(t)).collect()
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
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

/// A promoted memory version in a content workarea.
fn memory(n: u128) -> EndpointVersion {
    EndpointVersion {
        artifact_id: id(0x1000 + n),
        kind: MemoryKind::Memory,
        status: MemoryStatus::Promoted,
        in_vocabulary: false,
    }
}

/// A promoted term version in its vocabulary workarea.
fn term(n: u128) -> EndpointVersion {
    EndpointVersion {
        artifact_id: id(0x2000 + n),
        kind: MemoryKind::Term,
        status: MemoryStatus::Promoted,
        in_vocabulary: true,
    }
}

fn active(text: &str) -> TermTip {
    TermTip {
        curie: curie(text),
        state: TipState::Active,
    }
}

fn about(n: u128, from: Uuid, term_curie: &str, state: EdgeState) -> ClaimEdge {
    ClaimEdge {
        edge_id: id(0xE000 + n),
        from,
        to: id(0x2000 + n),
        label: OmlLabel::About.as_str().to_owned(),
        term_curie: Some(curie(term_curie)),
        state,
    }
}

fn edge(n: u128, from: Uuid, to: Uuid, label: &str, state: EdgeState) -> ClaimEdge {
    ClaimEdge {
        edge_id: id(0xE100 + n),
        from,
        to,
        label: label.to_owned(),
        term_curie: None,
        state,
    }
}

#[test]
fn an_is_a_edge_closing_a_cycle_is_refused_at_ratification() {
    let ratified = [is_a(CHURN, "biz:risk"), is_a("biz:risk", "biz:concern")];
    let refused = Err(CycleRefused {
        label: OmlLabel::IsA,
    });
    assert_eq!(
        check_acyclic(&ratified, &is_a("biz:concern", CHURN)),
        refused
    );
    assert_eq!(
        check_acyclic(&ratified, &is_a("biz:risk", "biz:risk")),
        refused
    );
    assert_eq!(check_acyclic(&[], &is_a(CHURN, CHURN)), refused);

    // A shortcut along the hierarchy closes nothing, nor does a new leaf.
    assert_eq!(
        check_acyclic(&ratified, &is_a(CHURN, "biz:concern")),
        Ok(())
    );
    assert_eq!(
        check_acyclic(&ratified, &is_a(ATTRITION, "biz:risk")),
        Ok(())
    );

    // Only is_a edges count toward an is_a cycle; part_of carries no
    // invariant of its own.
    let part_of = [term_edge(OmlLabel::PartOf, "biz:concern", CHURN)];
    assert_eq!(check_acyclic(&part_of, &is_a(CHURN, "biz:concern")), Ok(()));
    assert_eq!(
        check_acyclic(
            &ratified,
            &term_edge(OmlLabel::PartOf, "biz:concern", CHURN)
        ),
        Ok(())
    );
}

#[test]
fn a_replaced_by_edge_closing_a_replacement_cycle_is_refused() {
    let ratified = [
        replaced_by(CHURN, ATTRITION),
        replaced_by(ATTRITION, "biz:leave-risk"),
    ];
    assert_eq!(
        check_acyclic(&ratified, &replaced_by("biz:leave-risk", CHURN)),
        Err(CycleRefused {
            label: OmlLabel::ReplacedBy
        })
    );
    assert_eq!(
        check_acyclic(&[], &replaced_by(CHURN, CHURN)),
        Err(CycleRefused {
            label: OmlLabel::ReplacedBy
        })
    );

    // A supersession chain over time is legitimate.
    assert_eq!(
        check_acyclic(&ratified, &replaced_by("biz:leave-risk", "biz:exit-risk")),
        Ok(())
    );
    // A replacement path closes no is_a cycle, an is_a path no replacement
    // cycle, and consider carries no invariant.
    assert_eq!(
        check_acyclic(&ratified, &is_a("biz:leave-risk", CHURN)),
        Ok(())
    );
    assert_eq!(
        check_acyclic(&[is_a(CHURN, ATTRITION)], &replaced_by(ATTRITION, CHURN)),
        Ok(())
    );
    assert_eq!(
        check_acyclic(
            &ratified,
            &term_edge(OmlLabel::Consider, "biz:leave-risk", CHURN)
        ),
        Ok(())
    );
}

#[test]
fn an_oml_about_edge_never_enters_the_lineage_closure() {
    let classified = id(0x1001);
    let ancestor = id(0x1002);
    let related = id(0x1003);
    let churn_term = id(0x2001);
    let attrition_term = id(0x2002);
    let derives = [DerivesFrom {
        child: classified,
        parent: ancestor,
    }];
    let edges = [
        ClaimEdge {
            to: churn_term,
            ..about(1, classified, CHURN, EdgeState::Pending)
        },
        ClaimEdge {
            to: attrition_term,
            ..about(2, classified, ATTRITION, EdgeState::Ratified)
        },
        edge(3, classified, related, "relates_to", EdgeState::Pending),
    ];

    // The memory's closure holds its ancestor and its connection, pending
    // and ratified alike, and no term it is classified under.
    let set = |ids: &[Uuid]| ids.iter().copied().collect::<BTreeSet<Uuid>>();
    assert_eq!(
        lineage_artifacts(classified, &derives, &edges),
        set(&[classified, ancestor, related])
    );
    // Nor does a term's closure reach the memory classified under it.
    assert_eq!(
        lineage_artifacts(churn_term, &derives, &edges),
        set(&[churn_term])
    );
    assert_eq!(
        lineage_artifacts(attrition_term, &derives, &edges),
        set(&[attrition_term])
    );
    // A connection is followed from either endpoint; derivation only toward
    // the ancestors.
    assert_eq!(
        lineage_artifacts(related, &derives, &edges),
        set(&[related, classified, ancestor])
    );
    assert_eq!(
        lineage_artifacts(ancestor, &derives, &edges),
        set(&[ancestor])
    );
}

#[test]
fn a_classification_of_a_memory_inside_a_vocabulary_is_refused() {
    let inside = EndpointVersion {
        in_vocabulary: true,
        ..memory(1)
    };
    assert_eq!(
        check_classification(&inside, &term(1), &active(CHURN), &[], &[]),
        Err(ClassificationRefusal::Validation {
            field: Endpoint::From
        })
    );
    assert_eq!(Endpoint::From.as_str(), "from_artifact_id");

    // The same memory outside every vocabulary subtree is classified, and
    // the edge records the term's CURIE.
    assert_eq!(
        check_classification(&memory(1), &term(1), &active(CHURN), &[], &[]),
        Ok(curie(CHURN))
    );
}

#[test]
fn a_classification_from_steering_is_refused() {
    let refused = Err(ClassificationRefusal::Validation {
        field: Endpoint::From,
    });
    for kind in [
        MemoryKind::Steering,
        MemoryKind::Term,
        MemoryKind::TermMapping,
        MemoryKind::VocabRelease,
    ] {
        let source = EndpointVersion { kind, ..memory(1) };
        assert_eq!(
            check_classification(&source, &term(1), &active(CHURN), &[], &[]),
            refused,
            "{kind:?}"
        );
    }
    for kind in [MemoryKind::Memory, MemoryKind::DerivedFact] {
        let source = EndpointVersion { kind, ..memory(1) };
        assert_eq!(
            check_classification(&source, &term(1), &active(CHURN), &[], &[]),
            Ok(curie(CHURN)),
            "{kind:?}"
        );
    }
    for status in [
        MemoryStatus::Held,
        MemoryStatus::Demoted,
        MemoryStatus::Rejected,
        MemoryStatus::Erased,
    ] {
        let source = EndpointVersion {
            status,
            ..memory(1)
        };
        assert_eq!(
            check_classification(&source, &term(1), &active(CHURN), &[], &[]),
            refused,
            "{status:?}"
        );
    }

    // The target is a promoted term version.
    let refused_target = Err(ClassificationRefusal::Validation {
        field: Endpoint::To,
    });
    assert_eq!(Endpoint::To.as_str(), "to_artifact_id");
    let not_a_term = EndpointVersion {
        kind: MemoryKind::Memory,
        ..term(1)
    };
    let held_term = EndpointVersion {
        status: MemoryStatus::Held,
        ..term(1)
    };
    for target in [not_a_term, held_term] {
        assert_eq!(
            check_classification(&memory(1), &target, &active(CHURN), &[], &[]),
            refused_target
        );
    }
}

#[test]
fn a_classification_to_a_deprecated_term_is_refused_with_its_successors() {
    let replaced = TermTip {
        curie: curie(CHURN),
        state: TipState::Deprecated(Successors::ReplacedBy(curie(ATTRITION))),
    };
    let refusal = check_classification(&memory(1), &term(1), &replaced, &[], &[]);
    assert_eq!(
        refusal,
        Err(ClassificationRefusal::InactiveCurie {
            curie: curie(CHURN),
            successors: Successors::ReplacedBy(curie(ATTRITION)),
        })
    );
    // The details a client self-heals from in one hop.
    let Err(ClassificationRefusal::InactiveCurie { successors, .. }) = refusal else {
        unreachable!()
    };
    assert_eq!(successors.relation().as_str(), "replaced_by");
    assert_eq!(successors.curies(), [curie(ATTRITION)]);

    let considered = Successors::Consider(vec![curie(ATTRITION), curie("biz:leave-risk")]);
    let consider = TermTip {
        curie: curie(CHURN),
        state: TipState::Deprecated(considered.clone()),
    };
    assert_eq!(considered.relation().as_str(), "consider");
    assert_eq!(
        check_classification(&memory(1), &term(1), &consider, &[], &[]),
        Err(ClassificationRefusal::InactiveCurie {
            curie: curie(CHURN),
            successors: considered.clone(),
        })
    );

    // The tip is checked again at ratification.
    assert_eq!(check_term_active(&active(CHURN)), Ok(()));
    assert_eq!(
        check_term_active(&consider),
        Err(ClassificationRefusal::InactiveCurie {
            curie: curie(CHURN),
            successors: considered,
        })
    );
}

#[test]
fn a_second_live_classification_of_one_version_and_curie_is_refused() {
    let classified = memory(1);
    for state in [EdgeState::Pending, EdgeState::Ratified] {
        let live = about(1, classified.artifact_id, CHURN, state);
        assert_eq!(
            check_classification(
                &classified,
                &term(1),
                &active(CHURN),
                &[],
                std::slice::from_ref(&live)
            ),
            Err(ClassificationRefusal::ClassificationExists(
                ExistingClassification::Edge {
                    edge_id: live.edge_id,
                    state,
                }
            )),
            "{state:?}"
        );
        // Another CURIE, or another version of the same claim, is another
        // pair.
        assert_eq!(
            check_classification(
                &classified,
                &term(2),
                &active(ATTRITION),
                &[],
                std::slice::from_ref(&live)
            ),
            Ok(curie(ATTRITION))
        );
        assert_eq!(
            check_classification(
                &memory(2),
                &term(1),
                &active(CHURN),
                &[],
                std::slice::from_ref(&live)
            ),
            Ok(curie(CHURN))
        );
    }

    // A CURIE the version's own terms already cite is refused too.
    assert_eq!(
        check_classification(&classified, &term(1), &active(CHURN), &[curie(CHURN)], &[]),
        Err(ClassificationRefusal::ClassificationExists(
            ExistingClassification::Terms
        ))
    );
}

#[test]
fn a_rejected_classification_frees_its_pair() {
    let version = memory(1).artifact_id;
    let churn = curie(CHURN);
    for (n, prior) in [(1, EdgeState::Pending), (2, EdgeState::Ratified)] {
        let mut classification = about(n, version, CHURN, prior);
        let edges = [classification.clone()];
        assert_eq!(
            check_live_classification(version, &churn, &[], &edges),
            Err(ClassificationRefusal::ClassificationExists(
                ExistingClassification::Edge {
                    edge_id: classification.edge_id,
                    state: prior,
                }
            ))
        );

        classification.state =
            edge_transition(&classification.label, prior, EdgeState::Rejected).expect("rejectable");
        assert_eq!(classification.state, EdgeState::Rejected);
        assert_eq!(
            check_live_classification(version, &churn, &[], &[classification]),
            Ok(())
        );
    }

    // Rejection is decided for oml:about alone, and a decided edge is not
    // decided again.
    assert_eq!(
        edge_transition("oml:is_a", EdgeState::Pending, EdgeState::Rejected),
        Err(EdgeTransitionError::RejectionNotAbout {
            label: "oml:is_a".to_owned()
        })
    );
    assert_eq!(
        edge_transition("oml:is_a", EdgeState::Pending, EdgeState::Ratified),
        Ok(EdgeState::Ratified)
    );
    for (from, to) in [
        (EdgeState::Rejected, EdgeState::Ratified),
        (EdgeState::Rejected, EdgeState::Rejected),
        (EdgeState::Ratified, EdgeState::Ratified),
        (EdgeState::Ratified, EdgeState::Pending),
    ] {
        assert_eq!(
            edge_transition("oml:about", from, to),
            Err(EdgeTransitionError::Illegal { from, to }),
            "{from:?}->{to:?}"
        );
    }
    let names: Vec<&str> = [EdgeState::Pending, EdgeState::Ratified, EdgeState::Rejected]
        .iter()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(
        names,
        ["pending", "ratified", "rejected"],
        "claim_edges.state"
    );
}

#[test]
fn a_ratified_classification_is_retired_while_its_term_is_deprecated() {
    let version = memory(1).artifact_id;
    let ratified = about(1, version, CHURN, EdgeState::Ratified);
    assert!(!is_retired(&ratified, TermStatus::Active));
    assert!(is_retired(&ratified, TermStatus::Deprecated));
    // Computed, never written: the edge stays ratified.
    assert_eq!(ratified.state, EdgeState::Ratified);

    for state in [EdgeState::Pending, EdgeState::Rejected] {
        assert!(
            !is_retired(&about(2, version, CHURN, state), TermStatus::Deprecated),
            "{state:?}"
        );
    }
    let hierarchy = edge(3, id(0x2003), id(0x2004), "oml:is_a", EdgeState::Ratified);
    assert!(!is_retired(&hierarchy, TermStatus::Deprecated));
}

#[test]
fn a_retired_classification_matches_no_recall_while_a_citation_of_the_same_curie_still_does() {
    let cites = id(0x1001);
    let classified = id(0x1002);
    let churn = curie(CHURN);
    let edges = [about(1, classified, CHURN, EdgeState::Ratified)];
    let filter = curies(&[CHURN]);
    let mut readable = BTreeMap::from([(churn.clone(), TermStatus::Active)]);

    assert!(matches_terms(
        cites,
        std::slice::from_ref(&churn),
        &edges,
        &filter,
        &readable
    ));
    assert!(matches_terms(classified, &[], &edges, &filter, &readable));

    readable.insert(churn.clone(), TermStatus::Deprecated);
    assert!(
        matches_terms(
            cites,
            std::slice::from_ref(&churn),
            &edges,
            &filter,
            &readable
        ),
        "a citation is its author's signed bytes"
    );
    assert!(
        !matches_terms(classified, &[], &edges, &filter, &readable),
        "the classification is retired"
    );

    // Only a ratified classification of the hit's own version matches.
    readable.insert(churn.clone(), TermStatus::Active);
    for state in [EdgeState::Pending, EdgeState::Rejected] {
        let unratified = [about(2, classified, CHURN, state)];
        assert!(
            !matches_terms(classified, &[], &unratified, &filter, &readable),
            "{state:?}"
        );
    }
    assert!(!matches_terms(id(0x1003), &[], &edges, &filter, &readable));
    // A CURIE outside the filter matches nothing.
    let other = curies(&[ATTRITION]);
    assert!(!matches_terms(
        cites,
        std::slice::from_ref(&churn),
        &edges,
        &other,
        &readable
    ));
    assert!(!matches_terms(classified, &[], &edges, &other, &readable));
    // A CURIE the key cannot read matches nothing, cited or classified.
    let unreadable = BTreeMap::new();
    assert!(!matches_terms(
        cites,
        std::slice::from_ref(&churn),
        &edges,
        &filter,
        &unreadable
    ));
    assert!(!matches_terms(
        classified,
        &[],
        &edges,
        &filter,
        &unreadable
    ));
}

#[test]
fn expand_descendants_widens_both_citations_and_classifications() {
    // biz:voluntary-churn is_a biz:churn-risk is_a biz:risk, materialized.
    let row = |ancestor: &str, descendant: &str| ClosureRow {
        ancestor: curie(ancestor),
        descendant: curie(descendant),
    };
    let closure = [
        row("biz:risk", CHURN),
        row("biz:risk", "biz:voluntary-churn"),
        row(CHURN, "biz:voluntary-churn"),
    ];
    let filter = curies(&["biz:risk"]);
    assert_eq!(expand_terms(&filter, ExpandTerms::None, &closure), filter);
    let widened = expand_terms(&filter, ExpandTerms::Descendants, &closure);
    assert_eq!(widened, curies(&["biz:risk", CHURN, "biz:voluntary-churn"]));
    // Descendants only: expansion never climbs.
    assert_eq!(
        expand_terms(&curies(&[CHURN]), ExpandTerms::Descendants, &closure),
        curies(&[CHURN, "biz:voluntary-churn"])
    );

    let cites = id(0x1001);
    let classified = id(0x1002);
    let edges = [about(
        1,
        classified,
        "biz:voluntary-churn",
        EdgeState::Ratified,
    )];
    let readable: BTreeMap<Curie, TermStatus> = curies(&["biz:risk", CHURN, "biz:voluntary-churn"])
        .into_iter()
        .map(|c| (c, TermStatus::Active))
        .collect();
    let cited = [curie(CHURN)];

    assert!(!matches_terms(cites, &cited, &edges, &filter, &readable));
    assert!(!matches_terms(classified, &[], &edges, &filter, &readable));
    assert!(matches_terms(cites, &cited, &edges, &widened, &readable));
    assert!(matches_terms(classified, &[], &edges, &widened, &readable));

    for expand in [ExpandTerms::None, ExpandTerms::Descendants] {
        assert_eq!(ExpandTerms::parse(expand.as_str()), Some(expand));
    }
    assert_eq!(ExpandTerms::parse("ancestors"), None);
}

/// Kahn's algorithm: whether the edges admit a topological order.
fn acyclic(edges: &[TermEdge]) -> bool {
    let mut indegree: BTreeMap<&Curie, usize> = BTreeMap::new();
    for e in edges {
        indegree.entry(&e.from).or_default();
        *indegree.entry(&e.to).or_default() += 1;
    }
    let mut ready: Vec<&Curie> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(c, _)| *c)
        .collect();
    let mut removed = 0;
    while let Some(node) = ready.pop() {
        removed += 1;
        for e in edges.iter().filter(|e| &e.from == node) {
            let d = indegree.get_mut(&e.to).expect("counted");
            *d -= 1;
            if *d == 0 {
                ready.push(&e.to);
            }
        }
    }
    removed == indegree.len()
}

proptest! {
    #[test]
    fn a_ratified_is_a_graph_built_only_through_the_cycle_check_is_always_acyclic(
        candidates in prop::collection::vec((0u8..8, 0u8..8), 0..48),
    ) {
        let mut ratified: Vec<TermEdge> = Vec::new();
        for (from, to) in candidates {
            let candidate = is_a(&format!("t:n{from}"), &format!("t:n{to}"));
            let admitted = check_acyclic(&ratified, &candidate).is_ok();
            // The check refuses exactly the candidates that close a cycle.
            let mut with = ratified.clone();
            with.push(candidate.clone());
            prop_assert_eq!(admitted, acyclic(&with));
            if admitted {
                ratified.push(candidate);
            }
            prop_assert!(acyclic(&ratified));
        }
    }
}
