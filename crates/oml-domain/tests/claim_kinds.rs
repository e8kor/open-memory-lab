//! The fields and keys of the claim kinds `skill`, `document` and `test`
//! (ADR-0080 d1, d9, d10; ADR-0081 d1, d2; ADR-0086 d1), and the withdrawn
//! state's one edge (ADR-0083 d13).

use oml_domain::claim_kinds::{
    TestShape, check_document_key, check_document_title, check_skill_body, check_skill_description,
    check_skill_name, check_test_key, document_key_from_title, markdown_title, skill_claim_key,
};
use oml_domain::{ClaimClass, MemoryKind, MemoryStatus, TransitionError, transition};

#[test]
fn a_skill_name_ending_in_unreviewed_is_refused() {
    let err = check_skill_name("deploy-unreviewed").unwrap_err();
    assert_eq!(err.field, "/skill/name");
    assert!(check_skill_name("deploy").is_ok());
    assert_eq!(skill_claim_key("deploy"), "skill/deploy");
}

#[test]
fn a_skill_name_outside_the_label_grammar_or_reserved_is_refused_naming_the_field() {
    for name in [
        "Deploy",
        "1deploy",
        "deploy-",
        "de--ploy",
        "guardrails",
        &"a".repeat(53),
        "",
    ] {
        assert_eq!(
            check_skill_name(name).map_err(|e| e.field),
            Err("/skill/name"),
            "{name:?}"
        );
    }
    assert!(check_skill_name(&"a".repeat(52)).is_ok());
}

#[test]
fn a_skill_description_is_one_line_of_1_to_1024_characters() {
    assert!(check_skill_description("Deploy the service.").is_ok());
    for bad in ["", "two\nlines", &"x".repeat(1025)] {
        assert_eq!(
            check_skill_description(bad).map_err(|e| e.field),
            Err("/skill/description")
        );
    }
}

#[test]
fn a_skill_body_opening_with_front_matter_is_refused_naming_body() {
    let err = check_skill_body("---\nname: deploy\n---\nRun it.").unwrap_err();
    assert_eq!(err.field, "/body");
    assert!(check_skill_body("Run the deploy.\n---\nthen check.").is_ok());
}

#[test]
fn a_document_key_outside_the_grammar_is_refused_naming_claim_key() {
    for key in [
        "docs/guide",
        "doc/",
        "doc/Guide",
        "doc/a//b",
        "doc/./a",
        "doc/../a",
        "doc/a b",
        &format!("doc/{}", "a".repeat(65)),
        &format!("doc/{}", ["a"; 63].join("/")),
    ] {
        assert_eq!(
            check_document_key(key).map_err(|e| e.field),
            Err("/claim_key"),
            "{key:?}"
        );
    }
    for key in ["doc/guide", "doc/ops/runbook.v2", "doc/a_b-c"] {
        assert!(check_document_key(key).is_ok(), "{key}");
    }
}

#[test]
fn a_documents_title_is_its_first_level_1_heading_and_its_key_follows_the_title() {
    assert_eq!(
        markdown_title("intro\n## Not this\n# Release Notes 2026\n# Later").as_deref(),
        Some("Release Notes 2026")
    );
    assert_eq!(markdown_title("no heading here"), None);
    assert_eq!(
        document_key_from_title("Release Notes 2026"),
        "doc/release-notes-2026"
    );
    assert!(check_document_key(&document_key_from_title(&"T".repeat(300))).is_ok());
    assert!(check_document_title("Runbook").is_ok());
    assert_eq!(
        check_document_title("two\nlines").map_err(|e| e.field),
        Err("/document/title")
    );
}

#[test]
fn a_test_key_is_test_and_a_label_and_its_shape_is_one_of_four() {
    assert!(check_test_key("test/recall-finds-the-runbook").is_ok());
    for key in ["test/", "tests/a", "test/A", "test/a/b"] {
        assert_eq!(
            check_test_key(key).map_err(|e| e.field),
            Err("/claim_key"),
            "{key}"
        );
    }
    for word in ["recall", "guardrail", "skill", "document"] {
        assert_eq!(TestShape::parse(word).map(TestShape::as_str), Some(word));
    }
    assert_eq!(TestShape::parse("unit"), None);
}

#[test]
fn every_kind_has_exactly_one_class() {
    for (kind, class) in [
        (MemoryKind::Memory, ClaimClass::Knowledge),
        (MemoryKind::DerivedFact, ClaimClass::Knowledge),
        (MemoryKind::Document, ClaimClass::Knowledge),
        (MemoryKind::Test, ClaimClass::Knowledge),
        (MemoryKind::Steering, ClaimClass::Instruction),
        (MemoryKind::Guardrail, ClaimClass::Instruction),
        (MemoryKind::Skill, ClaimClass::Instruction),
        (MemoryKind::Term, ClaimClass::Vocabulary),
        (MemoryKind::TermMapping, ClaimClass::Vocabulary),
        (MemoryKind::VocabRelease, ClaimClass::Vocabulary),
    ] {
        assert_eq!(kind.class(), class, "{}", kind.as_str());
        assert_eq!(MemoryKind::parse(kind.as_str()), Some(kind));
    }
}

#[test]
fn only_a_held_version_is_withdrawn() {
    assert_eq!(
        transition(MemoryStatus::Held, MemoryStatus::Withdrawn),
        Ok(MemoryStatus::Withdrawn)
    );
    for from in [
        MemoryStatus::Promoted,
        MemoryStatus::Rejected,
        MemoryStatus::Withdrawn,
    ] {
        assert_eq!(
            transition(from, MemoryStatus::Withdrawn),
            Err(TransitionError {
                from,
                to: MemoryStatus::Withdrawn
            })
        );
    }
    assert_eq!(MemoryStatus::Withdrawn.as_str(), "withdrawn");
}
