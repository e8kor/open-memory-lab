//! The vocab scanner: the vocabulary policy and its enforcement modes, the
//! citation checks against the pinned release, the `vocab.*` finding
//! classes of the closed registry, and the veto that keeps simulations from
//! minting vocabulary (ADR-0021 d3, d5, d7, d8, d11; admission.md,
//! ontologies.md, simulations.md, errors.md).

use std::collections::{BTreeMap, BTreeSet};

use oml_domain::MemoryKind;
use oml_domain::vocab::{
    Curie, PrefixRegistry, Successors, TermStatus, UnknownCurie, VOCABULARY_KINDS,
};
use oml_domain::vocab_edges::TipState;
use oml_domain::vocab_scan::{
    CitedTerm, Enforcement, Finding, MappingPredicate, RequireWithoutRelease, ScanRefusal,
    ScanSubject, VocabularyPolicy, deprecated_successor_findings, scan,
};
use uuid::Uuid;

const CHURN: &str = "biz:churn-risk";
const ATTRITION: &str = "biz:attrition-risk";
const LAPSE: &str = "biz:lapse-risk";

/// The vocabulary workarea that registered `biz`, and the release its
/// policy pins.
const VOCAB: Uuid = Uuid::from_u128(0xB1);
const RELEASE: Uuid = Uuid::from_u128(0xE1);

fn curie(text: &str) -> Curie {
    Curie::parse(text).expect("a well-formed CURIE")
}

/// A tenant with two vocabularies: `biz` (the policy's) and `ops`.
fn registry() -> PrefixRegistry {
    let mut registry = PrefixRegistry::new();
    registry.register("biz", VOCAB).expect("a fresh prefix");
    registry
        .register("ops", Uuid::from_u128(0xB2))
        .expect("a fresh prefix");
    registry
}

fn policy(enforcement: Enforcement) -> VocabularyPolicy {
    VocabularyPolicy::new(VOCAB, Some(RELEASE), enforcement).expect("a pinned policy")
}

fn cited(entries: &[(&str, CitedTerm)]) -> BTreeMap<Curie, CitedTerm> {
    entries
        .iter()
        .map(|(text, state)| (curie(text), state.clone()))
        .collect()
}

fn tips(entries: &[(&str, TermStatus)]) -> BTreeMap<Curie, TermStatus> {
    entries
        .iter()
        .map(|(text, status)| (curie(text), *status))
        .collect()
}

fn active() -> CitedTerm {
    CitedTerm::Tip(TipState::Active)
}

fn deprecated(successors: Successors) -> CitedTerm {
    CitedTerm::Tip(TipState::Deprecated(successors))
}

/// A memory in a content workarea citing `terms`.
fn citing(terms: &[Curie]) -> ScanSubject<'_> {
    ScanSubject {
        kind: MemoryKind::Memory,
        simulation: false,
        in_vocabulary: false,
        terms,
        term_status: None,
        successors: None,
        mapping_predicate: None,
    }
}

/// A term deprecation in the vocabulary, declaring `successors`.
fn deprecation(successors: &Successors) -> ScanSubject<'_> {
    ScanSubject {
        kind: MemoryKind::Term,
        simulation: false,
        in_vocabulary: true,
        terms: &[],
        term_status: Some(TermStatus::Deprecated),
        successors: Some(successors),
        mapping_predicate: None,
    }
}

/// A `term_mapping` in the vocabulary asserting `predicate`.
fn mapping(predicate: MappingPredicate) -> ScanSubject<'static> {
    ScanSubject {
        kind: MemoryKind::TermMapping,
        simulation: false,
        in_vocabulary: true,
        terms: &[],
        term_status: None,
        successors: None,
        mapping_predicate: Some(predicate),
    }
}

fn run(
    subject: &ScanSubject<'_>,
    policy: Option<&VocabularyPolicy>,
    cited: &BTreeMap<Curie, CitedTerm>,
    tips: &BTreeMap<Curie, TermStatus>,
) -> Result<BTreeSet<Finding>, ScanRefusal> {
    scan(subject, policy, &registry(), cited, tips)
}

fn findings(list: &[Finding]) -> BTreeSet<Finding> {
    list.iter().cloned().collect()
}

#[test]
fn enforcement_require_is_refused_without_a_pinned_release_and_trunk_current_checking_is_annotate_only()
 {
    assert_eq!(
        VocabularyPolicy::new(VOCAB, None, Enforcement::Require),
        Err(RequireWithoutRelease)
    );
    for enforcement in [Enforcement::Annotate, Enforcement::Off] {
        let trunk = VocabularyPolicy::new(VOCAB, None, enforcement).expect("trunk-current");
        assert_eq!(trunk.release(), None, "{enforcement:?}");
        assert_eq!(trunk.enforcement(), enforcement);
    }
    // The scan verdict records the pinned release beside the scanner version.
    let pinned = policy(Enforcement::Require);
    assert_eq!(pinned.vocabulary(), VOCAB);
    assert_eq!(pinned.release(), Some(RELEASE));
    assert_eq!(pinned.enforcement(), Enforcement::Require);

    // The modes are exactly require, annotate and off.
    for (name, mode) in [
        ("require", Enforcement::Require),
        ("annotate", Enforcement::Annotate),
        ("off", Enforcement::Off),
    ] {
        assert_eq!(mode.as_str(), name);
        assert_eq!(Enforcement::parse(name), Some(mode));
    }
    for unknown in ["", "Require", "warn", "veto"] {
        assert_eq!(Enforcement::parse(unknown), None, "{unknown:?}");
    }
}

#[test]
fn a_workarea_with_no_vocabulary_policy_never_runs_the_vocab_scanner() {
    let terms = [curie(CHURN), curie(ATTRITION), curie("zzz:nothing")];
    let states = cited(&[
        (CHURN, CitedTerm::Held),
        (ATTRITION, deprecated(Successors::ReplacedBy(curie(LAPSE)))),
    ]);
    let replaced = Successors::ReplacedBy(curie(LAPSE));
    let lapsed = tips(&[(LAPSE, TermStatus::Deprecated)]);
    for subject in [
        citing(&terms),
        deprecation(&replaced),
        mapping(MappingPredicate::ExactMatch),
    ] {
        assert_eq!(
            run(&subject, None, &states, &lapsed),
            Ok(BTreeSet::new()),
            "{subject:?}"
        );
    }
}

#[test]
fn under_require_a_citation_of_an_active_term_of_the_pinned_release_passes() {
    let terms = [curie(CHURN), curie(ATTRITION)];
    let states = cited(&[(CHURN, active()), (ATTRITION, active())]);
    assert_eq!(
        run(
            &citing(&terms),
            Some(&policy(Enforcement::Require)),
            &states,
            &BTreeMap::new()
        ),
        Ok(BTreeSet::new())
    );
}

#[test]
fn under_require_a_citation_naming_no_term_of_the_pinned_release_is_refused_with_vocab_unknown_curie()
 {
    let require = policy(Enforcement::Require);
    // Every resolution the caller hands over says active, so only the
    // vocabulary's ownership of the prefix refuses the foreign ones.
    let states = cited(&[
        ("biz:held-term", CitedTerm::Held),
        ("ops:churn-risk", active()),
        ("zzz:churn-risk", active()),
        ("oml:is_a", active()),
    ]);
    for text in [
        // not in the reference at all, or out of the writer's read scope
        CHURN,
        // exists but held: a release pins no held version
        "biz:held-term",
        // another vocabulary's prefix: the policy's release holds none of it
        "ops:churn-risk",
        // a prefix no vocabulary registered
        "zzz:churn-risk",
        // the provider's upper vocabulary is no term of a tenant release
        "oml:is_a",
    ] {
        let terms = [curie(text)];
        assert_eq!(
            run(&citing(&terms), Some(&require), &states, &BTreeMap::new()),
            Err(ScanRefusal::UnknownCurie(UnknownCurie {
                curie: curie(text)
            })),
            "{text}"
        );
    }
}

#[test]
fn under_require_a_deprecated_citation_is_refused_with_vocab_inactive_curie_carrying_its_successors()
 {
    let require = policy(Enforcement::Require);
    let replaced = Successors::ReplacedBy(curie(ATTRITION));
    let consider = Successors::Consider(vec![curie(ATTRITION), curie(LAPSE)]);
    for (successors, relation) in [(replaced, "replaced_by"), (consider, "consider")] {
        let terms = [curie(CHURN)];
        let states = cited(&[(CHURN, deprecated(successors.clone()))]);
        let refusal = run(&citing(&terms), Some(&require), &states, &BTreeMap::new());
        assert_eq!(
            refusal,
            Err(ScanRefusal::InactiveCurie {
                curie: curie(CHURN),
                successors: successors.clone(),
            }),
            "{relation}"
        );
        let Err(ScanRefusal::InactiveCurie { successors, .. }) = refusal else {
            unreachable!()
        };
        // details.relation and details.successors, for a one-hop self-heal.
        assert_eq!(successors.relation().as_str(), relation);
        assert!(successors.curies().contains(&curie(ATTRITION)));
    }
}

#[test]
fn under_require_the_first_failing_citation_in_submitted_order_answers() {
    let require = policy(Enforcement::Require);
    let replaced = Successors::ReplacedBy(curie(LAPSE));
    let states = cited(&[(CHURN, active()), (ATTRITION, deprecated(replaced.clone()))]);
    let unknown = curie("biz:never-proposed");

    let deprecated_first = [curie(CHURN), curie(ATTRITION), unknown.clone()];
    assert_eq!(
        run(
            &citing(&deprecated_first),
            Some(&require),
            &states,
            &BTreeMap::new()
        ),
        Err(ScanRefusal::InactiveCurie {
            curie: curie(ATTRITION),
            successors: replaced,
        })
    );
    let unknown_first = [curie(CHURN), unknown.clone(), curie(ATTRITION)];
    assert_eq!(
        run(
            &citing(&unknown_first),
            Some(&require),
            &states,
            &BTreeMap::new()
        ),
        Err(ScanRefusal::UnknownCurie(UnknownCurie { curie: unknown }))
    );
}

#[test]
fn under_annotate_a_citation_of_a_held_term_records_vocab_term_pending_and_never_blocks() {
    let terms = [curie(CHURN), curie(ATTRITION)];
    let states = cited(&[(CHURN, CitedTerm::Held), (ATTRITION, active())]);
    for annotate in [
        policy(Enforcement::Annotate),
        VocabularyPolicy::new(VOCAB, None, Enforcement::Annotate).expect("trunk-current"),
    ] {
        let recorded = run(&citing(&terms), Some(&annotate), &states, &BTreeMap::new());
        assert_eq!(
            recorded,
            Ok(findings(&[Finding::TermPending {
                curie: curie(CHURN)
            }]))
        );
        let class: Vec<&str> = recorded.unwrap().iter().map(Finding::class).collect();
        assert_eq!(class, ["vocab.term_pending"]);
    }

    // Its absence: an active citation records nothing.
    let only_active = [curie(ATTRITION)];
    assert_eq!(
        run(
            &citing(&only_active),
            Some(&policy(Enforcement::Annotate)),
            &states,
            &BTreeMap::new()
        ),
        Ok(BTreeSet::new())
    );
}

#[test]
fn under_a_pinned_annotate_policy_a_curie_the_trunk_holds_and_the_release_does_not_pin_records_vocab_term_pending()
 {
    // The pinned release pins promoted tips only: CHURN, held at
    // submission seq, has no tip there; ATTRITION is active in it.
    let release: BTreeMap<Curie, TipState> = [(curie(ATTRITION), TipState::Active)].into();
    let held_at_seq: BTreeSet<Curie> = [curie(CHURN)].into();
    let terms = [curie(CHURN), curie(ATTRITION), curie(LAPSE)];
    let states: BTreeMap<Curie, CitedTerm> = terms
        .iter()
        .filter_map(|cited| {
            CitedTerm::resolve(release.get(cited).cloned(), held_at_seq.contains(cited))
                .map(|state| (cited.clone(), state))
        })
        .collect();
    assert_eq!(
        states,
        cited(&[(CHURN, CitedTerm::Held), (ATTRITION, active())])
    );
    assert_eq!(
        run(
            &citing(&terms),
            Some(&policy(Enforcement::Annotate)),
            &states,
            &BTreeMap::new()
        ),
        Ok(findings(&[Finding::TermPending {
            curie: curie(CHURN)
        }]))
    );

    // The same resolution under require: the release pins no version of
    // the held term, so it answers vocab.unknown_curie.
    assert_eq!(
        run(
            &citing(&terms),
            Some(&policy(Enforcement::Require)),
            &states,
            &BTreeMap::new()
        ),
        Err(ScanRefusal::UnknownCurie(UnknownCurie {
            curie: curie(CHURN)
        }))
    );

    // A tip in the reference wins over the held read, and a CURIE with
    // neither read is absent: unknown.
    assert_eq!(
        CitedTerm::resolve(Some(TipState::Active), true),
        Some(active())
    );
    assert_eq!(CitedTerm::resolve(None, false), None);
}

#[test]
fn under_annotate_an_unknown_or_deprecated_citation_never_blocks() {
    let annotate = policy(Enforcement::Annotate);
    let states = cited(&[
        (ATTRITION, deprecated(Successors::ReplacedBy(curie(LAPSE)))),
        // held, but under another vocabulary's prefix: not this policy's
        ("ops:churn-risk", CitedTerm::Held),
    ]);
    let terms = [
        curie(CHURN),
        curie(ATTRITION),
        curie("ops:churn-risk"),
        curie("zzz:churn-risk"),
    ];
    assert_eq!(
        run(&citing(&terms), Some(&annotate), &states, &BTreeMap::new()),
        Ok(BTreeSet::new())
    );
}

#[test]
fn under_enforcement_off_no_citation_is_checked() {
    let off = policy(Enforcement::Off);
    let terms = [curie(CHURN), curie(ATTRITION), curie("zzz:churn-risk")];
    let states = cited(&[
        (CHURN, CitedTerm::Held),
        (ATTRITION, deprecated(Successors::ReplacedBy(curie(LAPSE)))),
    ]);
    assert_eq!(
        run(&citing(&terms), Some(&off), &states, &BTreeMap::new()),
        Ok(BTreeSet::new())
    );
}

#[test]
fn enforcement_governs_citations_only_and_the_routing_classes_fire_in_every_mode() {
    let replaced = Successors::ReplacedBy(curie(ATTRITION));
    for enforcement in [
        Enforcement::Require,
        Enforcement::Annotate,
        Enforcement::Off,
    ] {
        let selected = policy(enforcement);
        assert_eq!(
            run(
                &deprecation(&replaced),
                Some(&selected),
                &BTreeMap::new(),
                &BTreeMap::new()
            ),
            Ok(findings(&[Finding::DeprecationReplacedBy])),
            "{enforcement:?}"
        );
        assert_eq!(
            run(
                &mapping(MappingPredicate::ExactMatch),
                Some(&selected),
                &BTreeMap::new(),
                &BTreeMap::new()
            ),
            Ok(findings(&[Finding::MappingExactMatch])),
            "{enforcement:?}"
        );
    }
}

#[test]
fn a_term_deprecation_carrying_a_replaced_by_successor_records_vocab_deprecation_replaced_by() {
    let annotate = policy(Enforcement::Annotate);
    let no_cited = BTreeMap::new();
    let successor_active = tips(&[(ATTRITION, TermStatus::Active)]);
    let replaced = Successors::ReplacedBy(curie(ATTRITION));
    let recorded = run(
        &deprecation(&replaced),
        Some(&annotate),
        &no_cited,
        &successor_active,
    );
    assert_eq!(recorded, Ok(findings(&[Finding::DeprecationReplacedBy])));
    assert_eq!(
        Finding::DeprecationReplacedBy.class(),
        "vocab.deprecation_replaced_by"
    );

    // Its absence: a consider-only deprecation, which is a judgment call.
    let consider = Successors::Consider(vec![curie(ATTRITION), curie(LAPSE)]);
    assert_eq!(
        run(
            &deprecation(&consider),
            Some(&annotate),
            &no_cited,
            &successor_active
        ),
        Ok(BTreeSet::new())
    );
    // Nor an active term, which deprecates nothing.
    let edit = ScanSubject {
        term_status: Some(TermStatus::Active),
        successors: None,
        ..deprecation(&replaced)
    };
    assert_eq!(
        run(&edit, Some(&annotate), &no_cited, &successor_active),
        Ok(BTreeSet::new())
    );
    // Nor any claim that is not a term.
    let not_a_term = ScanSubject {
        kind: MemoryKind::Memory,
        in_vocabulary: false,
        ..deprecation(&replaced)
    };
    assert_eq!(
        run(&not_a_term, Some(&annotate), &no_cited, &successor_active),
        Ok(BTreeSet::new())
    );
}

#[test]
fn a_term_mapping_asserting_the_exact_match_predicate_records_vocab_mapping_exact_match() {
    let annotate = policy(Enforcement::Annotate);
    let (no_cited, no_tips) = (BTreeMap::new(), BTreeMap::new());
    assert_eq!(
        run(
            &mapping(MappingPredicate::ExactMatch),
            Some(&annotate),
            &no_cited,
            &no_tips
        ),
        Ok(findings(&[Finding::MappingExactMatch]))
    );
    assert_eq!(
        Finding::MappingExactMatch.class(),
        "vocab.mapping_exactMatch"
    );

    // Its absence: every other SSSOM predicate is AI-only.
    for predicate in [
        MappingPredicate::CloseMatch,
        MappingPredicate::BroadMatch,
        MappingPredicate::NarrowMatch,
        MappingPredicate::RelatedMatch,
    ] {
        assert_eq!(
            run(&mapping(predicate), Some(&annotate), &no_cited, &no_tips),
            Ok(BTreeSet::new()),
            "{predicate:?}"
        );
    }
    // Nor a claim that is not a term_mapping.
    let not_a_mapping = ScanSubject {
        kind: MemoryKind::Term,
        term_status: Some(TermStatus::Active),
        ..mapping(MappingPredicate::ExactMatch)
    };
    assert_eq!(
        run(&not_a_mapping, Some(&annotate), &no_cited, &no_tips),
        Ok(BTreeSet::new())
    );

    // The predicates are exactly the five of the mapping field.
    for (name, predicate) in [
        ("exactMatch", MappingPredicate::ExactMatch),
        ("closeMatch", MappingPredicate::CloseMatch),
        ("broadMatch", MappingPredicate::BroadMatch),
        ("narrowMatch", MappingPredicate::NarrowMatch),
        ("relatedMatch", MappingPredicate::RelatedMatch),
    ] {
        assert_eq!(predicate.as_str(), name);
        assert_eq!(MappingPredicate::parse(name), Some(predicate));
    }
    for unknown in ["", "exactmatch", "ExactMatch", "skos:exactMatch"] {
        assert_eq!(MappingPredicate::parse(unknown), None, "{unknown:?}");
    }
}

#[test]
fn a_deprecation_whose_named_successor_is_itself_already_deprecated_records_vocab_deprecated_successor()
 {
    let annotate = policy(Enforcement::Annotate);
    let (no_cited, no_tips) = (BTreeMap::new(), BTreeMap::new());

    let replaced = Successors::ReplacedBy(curie(ATTRITION));
    let retired = tips(&[(ATTRITION, TermStatus::Deprecated)]);
    assert_eq!(
        run(
            &deprecation(&replaced),
            Some(&annotate),
            &no_cited,
            &retired
        ),
        Ok(findings(&[
            Finding::DeprecationReplacedBy,
            Finding::DeprecatedSuccessor {
                successor: curie(ATTRITION)
            },
        ]))
    );
    assert_eq!(
        Finding::DeprecatedSuccessor {
            successor: curie(ATTRITION)
        }
        .class(),
        "vocab.deprecated_successor"
    );

    // One finding per deprecated successor, under consider as well.
    let consider = Successors::Consider(vec![curie(ATTRITION), curie(LAPSE)]);
    let one_retired = tips(&[
        (ATTRITION, TermStatus::Deprecated),
        (LAPSE, TermStatus::Active),
    ]);
    assert_eq!(
        run(
            &deprecation(&consider),
            Some(&annotate),
            &no_cited,
            &one_retired
        ),
        Ok(findings(&[Finding::DeprecatedSuccessor {
            successor: curie(ATTRITION)
        }]))
    );

    // Its absence: active successors, and a successor whose tip the caller
    // could not read.
    let all_active = tips(&[(ATTRITION, TermStatus::Active), (LAPSE, TermStatus::Active)]);
    assert_eq!(
        run(
            &deprecation(&consider),
            Some(&annotate),
            &no_cited,
            &all_active
        ),
        Ok(BTreeSet::new())
    );
    assert_eq!(
        run(
            &deprecation(&consider),
            Some(&annotate),
            &no_cited,
            &no_tips
        ),
        Ok(BTreeSet::new())
    );
}

#[test]
fn the_deprecated_successor_class_is_read_again_from_the_tips_at_ratification_time() {
    let consider = Successors::Consider(vec![curie(ATTRITION), curie(LAPSE)]);
    let at_capture = tips(&[(ATTRITION, TermStatus::Active), (LAPSE, TermStatus::Active)]);
    assert_eq!(
        deprecated_successor_findings(&consider, &at_capture),
        BTreeSet::new()
    );
    // LAPSE was deprecated between capture and ratification.
    let at_ratification = tips(&[
        (ATTRITION, TermStatus::Active),
        (LAPSE, TermStatus::Deprecated),
    ]);
    assert_eq!(
        deprecated_successor_findings(&consider, &at_ratification),
        findings(&[Finding::DeprecatedSuccessor {
            successor: curie(LAPSE)
        }])
    );
}

#[test]
fn vocabulary_workareas_refuse_simulation_submissions_of_the_vocabulary_kinds() {
    let (no_cited, no_tips) = (BTreeMap::new(), BTreeMap::new());
    let require = policy(Enforcement::Require);
    for kind in VOCABULARY_KINDS {
        let minted = ScanSubject {
            kind,
            simulation: true,
            in_vocabulary: true,
            terms: &[],
            term_status: None,
            successors: None,
            mapping_predicate: None,
        };
        // A veto of the vocabulary workarea, whatever its policy says.
        for selected in [None, Some(&require)] {
            assert_eq!(
                run(&minted, selected, &no_cited, &no_tips),
                Err(ScanRefusal::SimulationMint { kind }),
                "{kind:?} under {selected:?}"
            );
        }
        // Its absence: a submission of any other provenance.
        assert_eq!(
            run(
                &ScanSubject {
                    simulation: false,
                    ..minted
                },
                None,
                &no_cited,
                &no_tips
            ),
            Ok(BTreeSet::new()),
            "{kind:?}"
        );
        // The rule is the vocabulary workarea's: where else a vocabulary
        // kind may be written is not the scanner's to say.
        assert_eq!(
            run(
                &ScanSubject {
                    in_vocabulary: false,
                    ..minted
                },
                None,
                &no_cited,
                &no_tips
            ),
            Ok(BTreeSet::new()),
            "{kind:?}"
        );
    }
    // A simulation memory in a vocabulary workarea mints nothing.
    let memory = ScanSubject {
        simulation: true,
        in_vocabulary: true,
        ..citing(&[])
    };
    assert_eq!(run(&memory, None, &no_cited, &no_tips), Ok(BTreeSet::new()));

    // The veto needs no lookup, so it answers before any citation does.
    let terms = [curie("biz:never-proposed")];
    let cited_term = ScanSubject {
        kind: MemoryKind::Term,
        simulation: true,
        in_vocabulary: true,
        terms: &terms,
        term_status: Some(TermStatus::Active),
        successors: None,
        mapping_predicate: None,
    };
    assert_eq!(
        run(&cited_term, Some(&require), &no_cited, &no_tips),
        Err(ScanRefusal::SimulationMint {
            kind: MemoryKind::Term
        })
    );
}

#[test]
fn every_finding_names_a_vocab_class_of_the_closed_registry() {
    let every = [
        Finding::TermPending {
            curie: curie(CHURN),
        },
        Finding::DeprecationReplacedBy,
        Finding::MappingExactMatch,
        Finding::DeprecatedSuccessor {
            successor: curie(ATTRITION),
        },
    ];
    let classes: Vec<&str> = every.iter().map(Finding::class).collect();
    assert_eq!(
        classes,
        [
            "vocab.term_pending",
            "vocab.deprecation_replaced_by",
            "vocab.mapping_exactMatch",
            "vocab.deprecated_successor",
        ],
        "admission.md, the v1 shipped registry"
    );
}
