//! The claim kinds (http-api.md submit), transition-function refusals
//! (admission.md lifecycle; ADR-0025 d10), the capture normalization to NFC
//! (admission.md, capture; ADR-0025 d6) and the R2 reviewer check
//! (ADR-0001 d2).

use oml_domain::{
    LineageClosure, MemoryKind, MemoryStatus, Role, TransitionError, VerdictError, check_reviewer,
    nfc, transition,
};
use uuid::Uuid;

#[test]
fn the_claim_kinds_are_the_submit_enum_and_each_name_parses_back() {
    let kinds = [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Steering,
        MemoryKind::Term,
        MemoryKind::TermMapping,
        MemoryKind::VocabRelease,
    ];
    let names: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        names,
        [
            "memory",
            "derived_fact",
            "steering",
            "term",
            "term_mapping",
            "vocab_release"
        ],
        "openapi.yaml SubmitRequest.kind"
    );
    for kind in kinds {
        assert_eq!(MemoryKind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(MemoryKind::parse("dataset_manifest"), None, "unbuilt");
    assert_eq!(MemoryKind::parse("Memory"), None, "names are exact");
}

const ALL: [MemoryStatus; 5] = [
    MemoryStatus::Held,
    MemoryStatus::Promoted,
    MemoryStatus::Demoted,
    MemoryStatus::Rejected,
    MemoryStatus::Erased,
];

#[test]
fn the_claim_states_are_the_schema_set_and_none_is_superseded() {
    let names: Vec<&str> = ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        names,
        ["held", "promoted", "demoted", "rejected", "erased"],
        "postgres-schema.md, memories.state"
    );
}

#[test]
fn the_lifecycle_admits_demote_from_promoted_and_erase_from_any_unerased_state() {
    // A supersession adds no state: the replaced version stays promoted and
    // only loses the tip (ADR-0025 d10). A demotion takes a promoted version
    // only, served or superseded, and never a held one (ADR-0089 d16); an
    // erase takes a version in any state but erased (ADR-0005 d2, amended
    // 2026-11-17). Reopen and reassignment are unbuilt.
    let mut legal = vec![
        (MemoryStatus::Held, MemoryStatus::Promoted),
        (MemoryStatus::Held, MemoryStatus::Rejected),
        (MemoryStatus::Promoted, MemoryStatus::Demoted),
    ];
    for from in ALL {
        if from != MemoryStatus::Erased {
            legal.push((from, MemoryStatus::Erased));
        }
    }
    for from in ALL {
        for to in ALL {
            let result = transition(from, to);
            if legal.contains(&(from, to)) {
                assert_eq!(result, Ok(to));
            } else {
                assert_eq!(
                    result,
                    Err(TransitionError { from, to }),
                    "{from:?}->{to:?}"
                );
            }
        }
    }
}

#[test]
fn only_a_promoted_version_can_be_demoted() {
    for from in ALL {
        let result = transition(from, MemoryStatus::Demoted);
        assert_eq!(result.is_ok(), from == MemoryStatus::Promoted, "{from:?}");
    }
    assert!(transition(MemoryStatus::Withdrawn, MemoryStatus::Demoted).is_err());
}

#[test]
fn a_withdrawn_version_can_be_erased_and_an_erased_one_cannot_be_erased_again() {
    assert_eq!(
        transition(MemoryStatus::Withdrawn, MemoryStatus::Erased),
        Ok(MemoryStatus::Erased)
    );
    assert!(transition(MemoryStatus::Erased, MemoryStatus::Erased).is_err());
}

fn key(n: u8) -> Uuid {
    Uuid::from_bytes([n; 16])
}

fn closure_with_author() -> LineageClosure {
    let mut closure = LineageClosure::new();
    closure.add_principal("crew-redline/agent-3"); // the author
    closure.add_key_id(key(1));
    closure.add_principal("crew-blue/agent-7"); // a declared ancestor's author
    closure.add_key_id(key(2));
    closure
}

#[test]
fn an_independent_reviewer_may_review() {
    let closure = closure_with_author();
    assert_eq!(
        check_reviewer(Role::Reviewer, "crew-green/reviewer-1", key(9), &closure),
        Ok(())
    );
}

#[test]
fn the_author_is_refused_the_verdict() {
    let closure = closure_with_author();
    assert_eq!(
        check_reviewer(Role::Reviewer, "crew-redline/agent-3", key(9), &closure),
        Err(VerdictError::PrincipalInLineage(
            "crew-redline/agent-3".to_owned()
        ))
    );
}

#[test]
fn a_lineage_ancestor_is_refused_the_verdict() {
    // R2 runs over the whole lineage closure, not just the author (ADR-0001 d2).
    let closure = closure_with_author();
    assert_eq!(
        check_reviewer(Role::Reviewer, "crew-blue/agent-7", key(9), &closure),
        Err(VerdictError::PrincipalInLineage(
            "crew-blue/agent-7".to_owned()
        ))
    );
}

#[test]
fn a_lineage_key_is_refused_even_under_a_fresh_principal() {
    let closure = closure_with_author();
    assert_eq!(
        check_reviewer(Role::Reviewer, "crew-green/reviewer-1", key(2), &closure),
        Err(VerdictError::KeyInLineage(key(2)))
    );
}

#[test]
fn only_reviewers_review() {
    let closure = LineageClosure::new();
    for role in [Role::Reader, Role::Writer, Role::Admin, Role::Operator] {
        assert_eq!(
            check_reviewer(role, "crew-green/reviewer-1", key(9), &closure),
            Err(VerdictError::NotReviewer(role))
        );
    }
}

#[test]
fn nfc_normalizes_decomposed_text() {
    // "e" + COMBINING ACUTE ACCENT composes to the one code point U+00E9,
    // and the octet count the budget counts drops from 3 to 2.
    let decomposed = "caf\u{0065}\u{0301}";
    let composed = nfc(decomposed);
    assert_eq!(composed, "caf\u{00E9}");
    assert_eq!(decomposed.len(), 6);
    assert_eq!(composed.len(), 5);
    // Hangul jamo compose to their syllable.
    assert_eq!(nfc("\u{1100}\u{1161}"), "\u{AC00}");
    // Text already in NFC is returned unchanged.
    assert_eq!(nfc("Accra Metals \u{00E9}"), "Accra Metals \u{00E9}");
    assert_eq!(nfc(""), "");
}

/// Kind `guardrail` is ADR-0037 d1's: a kind the domain names, not
/// yet in `openapi.yaml`'s submit enum, which changes with its route
/// (d14).
#[test]
fn guardrail_is_a_claim_kind_whose_name_parses_back() {
    assert_eq!(MemoryKind::Guardrail.as_str(), "guardrail");
    assert_eq!(MemoryKind::parse("guardrail"), Some(MemoryKind::Guardrail));
    assert_eq!(MemoryKind::parse("Guardrail"), None, "names are exact");
}

#[test]
fn every_kind_belongs_to_the_class_adr_0080_d16_names() {
    use oml_domain::ClaimClass::{Instruction, Knowledge, Vocabulary};
    for (kind, class) in [
        (MemoryKind::Memory, Knowledge),
        (MemoryKind::DerivedFact, Knowledge),
        (MemoryKind::Steering, Instruction),
        (MemoryKind::Guardrail, Instruction),
        (MemoryKind::Term, Vocabulary),
        (MemoryKind::TermMapping, Vocabulary),
        (MemoryKind::VocabRelease, Vocabulary),
    ] {
        assert_eq!(kind.class(), class, "{}", kind.as_str());
    }
    assert!(Instruction.needs_person());
    assert!(!Knowledge.needs_person() && !Vocabulary.needs_person());
}

#[test]
fn an_approval_of_an_instruction_class_version_is_refused_from_every_key_but_a_persons_reviewer_key() {
    use oml_domain::{UsedBy, check_approval_class};
    for kind in [MemoryKind::Steering, MemoryKind::Guardrail] {
        assert_eq!(
            check_approval_class(kind, Role::Reviewer, UsedBy::Person, false, false),
            Ok(())
        );
        for (used_by, crew, profile) in [
            (UsedBy::Agent, false, false),
            (UsedBy::Service, false, false),
            (UsedBy::Person, true, false),
            (UsedBy::Person, false, true),
        ] {
            assert_eq!(
                check_approval_class(kind, Role::Reviewer, used_by, crew, profile),
                Err(VerdictError::PersonRequired(kind)),
                "{} {used_by:?} crew {crew} profile {profile}",
                kind.as_str()
            );
        }
    }
    // A crew's agent key still approves knowledge and vocabulary.
    for kind in [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Term,
    ] {
        assert_eq!(
            check_approval_class(kind, Role::Reviewer, UsedBy::Agent, true, false),
            Ok(())
        );
    }
}
