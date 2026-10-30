//! The CURIE grammar over a tenant's prefix registry, the reserved `oml:`
//! prefix and its upper-vocabulary register, the vocabulary claim kinds,
//! and a term's structural `term_status` (ADR-0021 d1, d2; ADR-0027 d7;
//! ontologies.md; openapi.yaml `Curie`).

use oml_domain::MemoryKind;
use oml_domain::vocab::{
    Curie, CurieError, CurieOwner, OML_LABELS, OmlLabel, PrefixRefusal, PrefixRegistry, TermStatus,
    TermStatusError, UnknownCurie, VOCABULARY_KINDS, is_vocabulary_kind, term_status,
};
use proptest::prelude::*;
use serde_json::json;
use uuid::Uuid;

fn curie(text: &str) -> Curie {
    Curie::parse(text).expect("a well-formed CURIE")
}

#[test]
fn a_curie_is_a_registered_prefix_and_a_localname() {
    let parsed = curie("biz:churn-risk");
    assert_eq!(parsed.prefix(), "biz");
    assert_eq!(parsed.localname(), "churn-risk");
    assert_eq!(parsed.as_str(), "biz:churn-risk");

    // The grammar is openapi.yaml's `^[A-Za-z][A-Za-z0-9_-]*:[^\s:]+$`.
    for good in ["a:b", "Proc_2-x:supplier-quote", "metal:aluminium", "x:é/1"] {
        assert!(Curie::parse(good).is_ok(), "{good}");
    }
    let refused = [
        ("churn-risk", CurieError::NoSeparator),
        ("", CurieError::NoSeparator),
        (":churn-risk", CurieError::Prefix),
        ("2biz:churn-risk", CurieError::Prefix),
        ("b.z:churn-risk", CurieError::Prefix),
        ("bïz:churn-risk", CurieError::Prefix),
        ("biz:", CurieError::Localname),
        ("biz:churn:risk", CurieError::Localname),
        ("biz:churn risk", CurieError::Localname),
        ("biz:churn\u{3000}risk", CurieError::Localname),
        ("biz:churn\u{feff}risk", CurieError::Localname),
    ];
    for (text, error) in refused {
        assert_eq!(Curie::parse(text), Err(error), "{text:?}");
    }

    // The prefix resolves only through the tenant's registry.
    let vocab = Uuid::from_u128(0xB1);
    let mut registry = PrefixRegistry::new();
    registry.register("biz", vocab).expect("a fresh prefix");
    assert_eq!(registry.workarea_of("biz"), Some(vocab));
    assert_eq!(registry.owner(&parsed), Ok(CurieOwner::Vocabulary(vocab)));
    let unregistered = curie("ops:churn-risk");
    assert_eq!(
        registry.owner(&unregistered),
        Err(UnknownCurie {
            curie: unregistered.clone()
        })
    );

    // A prefix is registered once per tenant, permanently, under the same
    // grammar as a CURIE's prefix half.
    assert_eq!(
        registry.register("biz", Uuid::from_u128(0xB2)),
        Err(PrefixRefusal::PrefixConflict {
            prefix: "biz".to_owned()
        })
    );
    assert_eq!(registry.workarea_of("biz"), Some(vocab), "never reassigned");
    for malformed in ["", "9lives", "b z", "biz:"] {
        assert_eq!(
            registry.register(malformed, Uuid::from_u128(0xB3)),
            Err(PrefixRefusal::Malformed {
                prefix: malformed.to_owned()
            }),
            "{malformed:?}"
        );
    }

    // A vocabulary node is any workarea in a registered workarea's subtree;
    // ancestry runs from the node to the root.
    let universe = Uuid::from_u128(0xA0);
    let child = Uuid::from_u128(0xB4);
    let content = Uuid::from_u128(0xC1);
    assert!(registry.is_vocabulary_node(&[vocab, universe]));
    assert!(registry.is_vocabulary_node(&[child, vocab, universe]));
    assert!(!registry.is_vocabulary_node(&[content, universe]));
    assert!(!registry.is_vocabulary_node(&[]));
}

#[test]
fn the_oml_prefix_is_reserved_in_every_tenant() {
    let tenants = [PrefixRegistry::new(), PrefixRegistry::new()];
    for (n, mut registry) in tenants.into_iter().enumerate() {
        assert_eq!(
            registry.register("oml", Uuid::from_u128(0xB0 + n as u128)),
            Err(PrefixRefusal::PrefixConflict {
                prefix: "oml".to_owned()
            }),
            "tenant {n}"
        );
        assert_eq!(registry.workarea_of("oml"), None, "no workarea owns oml:");
        // The provider owns oml: in every tenant, registered or not, and it
        // names exactly the shipped register.
        assert_eq!(
            registry.owner(&curie("oml:is_a")),
            Ok(CurieOwner::Provider(OmlLabel::IsA))
        );
        let unshipped = curie("oml:sibling_of");
        assert_eq!(
            registry.owner(&unshipped),
            Err(UnknownCurie {
                curie: unshipped.clone()
            })
        );
    }
}

#[test]
fn oml_about_is_the_one_oml_label_whose_source_is_not_a_term() {
    let names: Vec<&str> = OML_LABELS.iter().map(|l| l.as_str()).collect();
    assert_eq!(
        names,
        [
            "oml:is_a",
            "oml:part_of",
            "oml:replaced_by",
            "oml:consider",
            "oml:about"
        ],
        "ADR-0021 d2, ADR-0027 d7"
    );
    for label in OML_LABELS {
        assert_eq!(OmlLabel::parse(label.as_str()), Some(label));
        let as_curie = curie(label.as_str());
        assert_eq!(as_curie.prefix(), "oml");
        assert_eq!(as_curie.localname(), label.localname());
    }
    let not_term_sourced: Vec<OmlLabel> = OML_LABELS
        .into_iter()
        .filter(|l| !l.source_is_term())
        .collect();
    assert_eq!(not_term_sourced, [OmlLabel::About]);
    assert_eq!(OmlLabel::parse("about"), None, "labels carry their prefix");
    assert_eq!(OmlLabel::parse("biz:about"), None, "only oml: spells one");
}

#[test]
fn the_vocabulary_kinds_are_term_term_mapping_and_vocab_release() {
    assert_eq!(
        VOCABULARY_KINDS,
        [
            MemoryKind::Term,
            MemoryKind::TermMapping,
            MemoryKind::VocabRelease
        ]
    );
    for kind in [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Steering,
    ] {
        assert!(!is_vocabulary_kind(kind), "{kind:?}");
    }
    for kind in VOCABULARY_KINDS {
        assert!(is_vocabulary_kind(kind), "{kind:?}");
    }
}

#[test]
fn term_status_is_read_from_the_structural_field_never_the_body() {
    // A body that spells a status changes nothing.
    let active = json!({
        "kind": "term",
        "body": "term_status: deprecated — superseded by biz:attrition-risk",
        "term_status": "active",
    });
    assert_eq!(
        term_status(MemoryKind::Term, &active),
        Ok(Some(TermStatus::Active))
    );

    // Shredded, the body is gone and the marker is still read.
    let shredded = json!({"kind": "term", "body": null, "term_status": "deprecated"});
    assert_eq!(
        term_status(MemoryKind::Term, &shredded),
        Ok(Some(TermStatus::Deprecated))
    );

    // Without the structural field, the body is never consulted.
    let body_only = json!({"kind": "term", "body": "term_status: deprecated"});
    assert_eq!(
        term_status(MemoryKind::Term, &body_only),
        Err(TermStatusError::Missing)
    );
    assert_eq!(
        term_status(MemoryKind::Term, &json!({"term_status": "retired"})),
        Err(TermStatusError::Unknown)
    );
    assert_eq!(
        term_status(MemoryKind::Term, &json!({"term_status": true})),
        Err(TermStatusError::Unknown)
    );

    // The field is kind=term only.
    assert_eq!(
        term_status(MemoryKind::Memory, &json!({"body": "term_status: active"})),
        Ok(None)
    );
    assert_eq!(
        term_status(MemoryKind::Memory, &json!({"term_status": "active"})),
        Err(TermStatusError::NotATerm {
            kind: MemoryKind::Memory
        })
    );

    for status in [TermStatus::Active, TermStatus::Deprecated] {
        assert_eq!(TermStatus::parse(status.as_str()), Some(status));
    }
}

proptest! {
    #[test]
    fn a_parsed_curie_renders_back_to_the_same_string(
        prefix in "[A-Za-z][A-Za-z0-9_-]{0,15}",
        localname in "[^\\s:\u{feff}]{1,24}",
    ) {
        let text = format!("{prefix}:{localname}");
        let parsed = Curie::parse(&text).expect("generated from the grammar");
        prop_assert_eq!(parsed.to_string(), text.clone());
        prop_assert_eq!(parsed.as_str(), text.as_str());
        prop_assert_eq!(parsed.prefix(), prefix.as_str());
        prop_assert_eq!(parsed.localname(), localname.as_str());
    }

    #[test]
    fn any_string_that_parses_as_a_curie_renders_back_unchanged(text in "\\PC{0,24}") {
        if let Ok(parsed) = Curie::parse(&text) {
            prop_assert_eq!(parsed.to_string(), text);
        }
    }
}
