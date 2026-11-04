//! Claim edges: the label a proposal carries and the ratifier that may
//! ratify it (ADR-0008 d5; ADR-0021 d5; ADR-0012; http-api.md, claim edges;
//! errors.md).

use oml_domain::claim_edges::{
    EdgeField, LabelRefusal, Ratifier, RatifyRefusal, check_label, check_ratifier,
};
use oml_domain::vocab::OmlLabel;
use oml_domain::{LineageClosure, MemoryKind, Role};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

const PROPOSER_KEY: u128 = 1;
const PROPOSER: &str = "crew:proposer";

fn ratifier(role: Role, principal: &str, key: u128) -> Ratifier<'_> {
    Ratifier {
        role,
        principal,
        key_id: id(key),
    }
}

fn ratify(r: &Ratifier<'_>, closure: &LineageClosure) -> Result<(), RatifyRefusal> {
    check_ratifier(r, id(PROPOSER_KEY), PROPOSER, closure)
}

#[test]
fn an_empty_label_is_refused_naming_the_label_field() {
    let refusal = check_label("", MemoryKind::Memory, MemoryKind::Memory).unwrap_err();
    assert_eq!(refusal.field(), EdgeField::Label);
    assert_eq!(refusal.field().as_str(), "label");
    assert_eq!(refusal.code(), "request.validation");
}

#[test]
fn a_label_not_in_nfc_is_refused() {
    // "e" followed by U+0301 COMBINING ACUTE ACCENT; NFC composes it to U+00E9.
    let decomposed = "re\u{301}sume\u{301}";
    let refusal = check_label(decomposed, MemoryKind::Memory, MemoryKind::Memory).unwrap_err();
    assert_eq!(refusal, LabelRefusal::NotNfc);
    assert_eq!(refusal.field(), EdgeField::Label);
    assert_eq!(refusal.code(), "request.validation");
    assert_eq!(
        check_label("r\u{e9}sum\u{e9}", MemoryKind::Memory, MemoryKind::Memory),
        Ok(())
    );
}

#[test]
fn an_edge_between_two_terms_must_carry_an_oml_label() {
    let refusal = check_label("mirrors", MemoryKind::Term, MemoryKind::Term).unwrap_err();
    assert_eq!(refusal.field(), EdgeField::Label);
    assert_eq!(refusal.code(), "request.validation");
    assert_eq!(
        check_label("oml:is_a", MemoryKind::Term, MemoryKind::Term),
        Ok(())
    );
}

#[test]
fn an_oml_hierarchy_label_joins_two_terms_only() {
    for label in [
        OmlLabel::IsA,
        OmlLabel::PartOf,
        OmlLabel::ReplacedBy,
        OmlLabel::Consider,
    ] {
        let label = label.as_str();
        assert_eq!(
            check_label(label, MemoryKind::Term, MemoryKind::Term),
            Ok(())
        );
        let from = check_label(label, MemoryKind::Memory, MemoryKind::Term).unwrap_err();
        assert_eq!(from.field(), EdgeField::From);
        assert_eq!(from.field().as_str(), "from_artifact_id");
        assert_eq!(from.code(), "request.validation");
        let to = check_label(label, MemoryKind::Term, MemoryKind::DerivedFact).unwrap_err();
        assert_eq!(to.field(), EdgeField::To);
        assert_eq!(to.field().as_str(), "to_artifact_id");
        assert_eq!(to.code(), "request.validation");
    }
}

#[test]
fn an_oml_about_proposal_is_refused_until_classification_is_built() {
    let about = OmlLabel::About.as_str();
    let refusal = check_label(about, MemoryKind::Memory, MemoryKind::Term).unwrap_err();
    assert_eq!(refusal, LabelRefusal::AboutNotBuilt);
    assert_eq!(refusal.field(), EdgeField::Label);
    assert_eq!(refusal.code(), "request.validation");
}

#[test]
fn a_free_label_joins_two_memories() {
    for (src, dst) in [
        (MemoryKind::Memory, MemoryKind::Memory),
        (MemoryKind::Memory, MemoryKind::DerivedFact),
        (MemoryKind::Steering, MemoryKind::Memory),
        (MemoryKind::Memory, MemoryKind::Term),
    ] {
        assert_eq!(check_label("supports", src, dst), Ok(()));
    }
}

#[test]
fn the_proposers_key_never_ratifies_its_own_edge() {
    let r = ratifier(Role::Dreamer, PROPOSER, PROPOSER_KEY);
    let refusal = ratify(&r, &LineageClosure::new()).unwrap_err();
    assert_eq!(refusal, RatifyRefusal::RatifierIsProposer);
    assert_eq!(refusal.code(), "review.ratifier_is_proposer");
}

#[test]
fn another_key_of_the_proposers_principal_is_refused_as_the_proposer() {
    let r = ratifier(Role::Dreamer, PROPOSER, 2);
    let refusal = ratify(&r, &LineageClosure::new()).unwrap_err();
    assert_eq!(refusal, RatifyRefusal::RatifierIsProposer);
    assert_eq!(refusal.code(), "review.ratifier_is_proposer");
}

#[test]
fn a_ratifier_in_either_endpoints_lineage_is_refused_author_in_lineage() {
    // The caller passes the union of the two endpoints' closures; one holds
    // the source's author by principal, the other the target's by key.
    let mut source_side = LineageClosure::new();
    source_side.add_principal("crew:source-author");
    source_side.add_key_id(id(10));
    let mut target_side = LineageClosure::new();
    target_side.add_principal("crew:target-author");
    target_side.add_key_id(id(20));

    let by_principal = ratifier(Role::Dreamer, "crew:source-author", 11);
    let refusal = ratify(&by_principal, &source_side).unwrap_err();
    assert_eq!(refusal, RatifyRefusal::AuthorInLineage);
    assert_eq!(refusal.code(), "review.author_in_lineage");

    let by_key = ratifier(Role::Dreamer, "crew:someone-else", 20);
    let refusal = ratify(&by_key, &target_side).unwrap_err();
    assert_eq!(refusal, RatifyRefusal::AuthorInLineage);
    assert_eq!(refusal.code(), "review.author_in_lineage");

    let outsider = ratifier(Role::Dreamer, "crew:outsider", 30);
    assert_eq!(ratify(&outsider, &source_side), Ok(()));
    assert_eq!(ratify(&outsider, &target_side), Ok(()));
}

#[test]
fn the_proposer_check_answers_before_the_lineage_check() {
    let mut closure = LineageClosure::new();
    closure.add_principal(PROPOSER);
    closure.add_key_id(id(PROPOSER_KEY));
    let r = ratifier(Role::Dreamer, PROPOSER, PROPOSER_KEY);
    assert_eq!(ratify(&r, &closure), Err(RatifyRefusal::RatifierIsProposer));
}

#[test]
fn a_writer_ratifier_is_refused_by_role_before_any_r2_check() {
    let mut closure = LineageClosure::new();
    closure.add_principal(PROPOSER);
    closure.add_key_id(id(PROPOSER_KEY));
    let r = ratifier(Role::Writer, PROPOSER, PROPOSER_KEY);
    let refusal = ratify(&r, &closure).unwrap_err();
    assert_eq!(refusal, RatifyRefusal::NotDreamer(Role::Writer));
    assert_eq!(refusal.code(), "access.verb_ceiling");
}
