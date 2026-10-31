//! Narrowed dreamer grants and member profiles: the `edge_labels` grant
//! attribute, narrowed effective access, the refusal-order steps that name
//! `edge_labels`, `my_edge_labels`, and the member-profile check of the
//! issuance order (ADR-0027 d3, d4, d5; roles-and-keys.md "Narrowed dreamer
//! grants: `edge_labels`", "Member profiles", "Grant admissibility and
//! refusal order"; workareas.md, effective access).

use std::collections::BTreeSet;

use oml_domain::access::{
    CrewKind, EdgeLabelsRefusal, EffectiveAccess, Grant, KeyDenial, LabelledGrant, MemberProfile,
    ProfileClause, ProfileMismatch, Verb, VerbRefusal, check_member_profile,
    check_profile_amendment, effective_access, effective_roles, effective_verbs, my_edge_labels,
    resolve_verb,
};
use oml_domain::vocab::{OmlLabel, PrefixRegistry};
use oml_domain::{HeldBy, Role};
use uuid::Uuid;

/// A tenant with one vocabulary: `universe` → {`metals-vocab` (prefix
/// `metal`) → `alloys`, `research` → `lit-review`}.
struct Tree {
    universe: Uuid,
    metals_vocab: Uuid,
    alloys: Uuid,
    research: Uuid,
    lit_review: Uuid,
    registry: PrefixRegistry,
}

impl Tree {
    fn new() -> Self {
        let metals_vocab = Uuid::from_u128(0xB1);
        let mut registry = PrefixRegistry::new();
        registry
            .register("metal", metals_vocab)
            .expect("a fresh prefix registers");
        Self {
            universe: Uuid::from_u128(0xB0),
            metals_vocab,
            alloys: Uuid::from_u128(0xB2),
            research: Uuid::from_u128(0xB3),
            lit_review: Uuid::from_u128(0xB4),
            registry,
        }
    }

    /// The ancestry of `node`, from the node to the root.
    fn ancestry(&self, node: Uuid) -> Vec<Uuid> {
        let parent = |n: Uuid| -> Option<Uuid> {
            if n == self.metals_vocab || n == self.research {
                Some(self.universe)
            } else if n == self.alloys {
                Some(self.metals_vocab)
            } else if n == self.lit_review {
                Some(self.research)
            } else {
                None
            }
        };
        let mut chain = vec![node];
        let mut at = node;
        while let Some(p) = parent(at) {
            chain.push(p);
            at = p;
        }
        chain
    }

    fn is_vocabulary_node(&self, node: Uuid) -> bool {
        self.registry.is_vocabulary_node(&self.ancestry(node))
    }
}

fn plain(n: u128, node: Uuid, role: Role, deny: &[Uuid]) -> Grant {
    Grant {
        grant_id: Uuid::from_u128(0x6000 + n),
        node,
        role,
        deny: deny.to_vec(),
    }
}

/// A grant without `edge_labels`.
fn grant(n: u128, node: Uuid, role: Role) -> LabelledGrant {
    LabelledGrant::from(plain(n, node, role, &[]))
}

/// A dreamer grant narrowed to `labels`.
fn narrowed(n: u128, node: Uuid, labels: &[&str]) -> LabelledGrant {
    LabelledGrant::new(plain(n, node, Role::Dreamer, &[]), Some(labels))
        .expect("admissible edge_labels")
}

fn labels(list: &[OmlLabel]) -> BTreeSet<OmlLabel> {
    list.iter().copied().collect()
}

fn denial(n: u128, node: Uuid) -> KeyDenial {
    KeyDenial {
        denial_id: Uuid::from_u128(0xD000 + n),
        node,
    }
}

const ABOUT: &str = "oml:about";

/// A classifier as `oml crews add-member --as classifier` issues it
/// (ADR-0027 d14): a reader grant on its `--workarea`, and on its
/// `--vocabulary` a dreamer grant narrowed to `oml:about` and a reader grant.
fn classifier_grants(tree: &Tree) -> Vec<LabelledGrant> {
    vec![
        grant(1, tree.research, Role::Reader),
        narrowed(2, tree.metals_vocab, &[ABOUT]),
        grant(3, tree.metals_vocab, Role::Reader),
    ]
}

/// A term creator as add-member issues it: a writer and a reader grant on
/// its `--vocabulary`.
fn term_creator_grants(tree: &Tree) -> Vec<LabelledGrant> {
    vec![
        grant(1, tree.metals_vocab, Role::Writer),
        grant(2, tree.metals_vocab, Role::Reader),
    ]
}

/// A curator as add-member issues it: a dreamer and a reader grant on its
/// `--vocabulary`, and on its `--workarea` a dreamer grant narrowed to
/// `oml:about` and a reader grant.
fn curator_grants(tree: &Tree) -> Vec<LabelledGrant> {
    vec![
        grant(1, tree.metals_vocab, Role::Dreamer),
        grant(2, tree.metals_vocab, Role::Reader),
        narrowed(3, tree.research, &[ABOUT]),
        grant(4, tree.research, Role::Reader),
    ]
}

fn check(
    tree: &Tree,
    crew: Option<CrewKind>,
    profile: Option<MemberProfile>,
    key_role: Role,
    held_by: Option<HeldBy>,
    grants: &[LabelledGrant],
) -> Result<(), ProfileMismatch> {
    check_member_profile(crew, profile, key_role, held_by, grants, |node| {
        tree.is_vocabulary_node(node)
    })
}

fn mismatch(
    profile: Option<MemberProfile>,
    clause: ProfileClause,
    grant_index: Option<usize>,
) -> Result<(), ProfileMismatch> {
    Err(ProfileMismatch {
        profile,
        clause,
        grant_index,
    })
}

// ---- The edge_labels attribute (roles-and-keys.md, narrowed dreamer grants) ----

#[test]
fn edge_labels_are_admitted_on_a_dreamer_grant_only_as_distinct_oml_labels() {
    let tree = Tree::new();
    let dreamer = plain(1, tree.metals_vocab, Role::Dreamer, &[]);
    let built = LabelledGrant::new(dreamer.clone(), Some(&["oml:is_a", ABOUT]))
        .expect("two distinct oml: labels on a dreamer grant");
    assert_eq!(
        built.edge_labels(),
        Some(&labels(&[OmlLabel::IsA, OmlLabel::About]))
    );
    assert_eq!(built.grant(), &dreamer);
    assert_eq!(
        LabelledGrant::new(dreamer.clone(), None).map(|g| g.edge_labels().cloned()),
        Ok(None)
    );

    for role in [Role::Reader, Role::Writer] {
        assert_eq!(
            LabelledGrant::new(plain(2, tree.research, role, &[]), Some(&[ABOUT])),
            Err(EdgeLabelsRefusal::NotDreamer { role }),
            "{role:?}"
        );
    }
    assert_eq!(
        LabelledGrant::new(dreamer.clone(), Some(&[])),
        Err(EdgeLabelsRefusal::Empty)
    );
    assert_eq!(
        LabelledGrant::new(dreamer.clone(), Some(&[ABOUT, ABOUT])),
        Err(EdgeLabelsRefusal::Repeated {
            label: ABOUT.to_owned()
        })
    );
    for outside in ["metal:about", "oml:mentions", "about"] {
        assert_eq!(
            LabelledGrant::new(dreamer.clone(), Some(&[outside])),
            Err(EdgeLabelsRefusal::OutsideOml {
                label: outside.to_owned()
            }),
            "{outside}"
        );
    }
}

// ---- Narrowed effective access (workareas.md; roles-and-keys.md) ----

#[test]
fn a_narrowed_dreamer_grant_confers_only_edges_with_its_labels() {
    let tree = Tree::new();
    let grants = classifier_grants(&tree);
    let at_alloys = effective_access(&grants, &[], &tree.ancestry(tree.alloys));
    assert_eq!(
        at_alloys,
        EffectiveAccess {
            roles: [Role::Reader, Role::Dreamer].into_iter().collect(),
            edge_labels: Some(labels(&[OmlLabel::About])),
        }
    );

    // Proposing, ratifying or rejecting an `oml:about` edge is allowed.
    assert_eq!(
        resolve_verb(Role::Dreamer, &at_alloys, Verb::ClaimEdge(ABOUT)),
        Ok(())
    );
    assert_eq!(
        resolve_verb(Role::Dreamer, &at_alloys, Verb::Needs(Role::Reader)),
        Ok(())
    );
    // Every other dreamer-class verb, and every other label, open or `oml:`,
    // is refused at step 3, naming the narrowed set.
    let refused = Err(VerbRefusal::GrantRefused {
        needed_role: Role::Dreamer,
        edge_labels: Some(labels(&[OmlLabel::About])),
    });
    for verb in [
        Verb::Needs(Role::Dreamer),
        Verb::ClaimEdge("oml:is_a"),
        Verb::ClaimEdge("oml:replaced_by"),
        Verb::ClaimEdge("cites"),
    ] {
        assert_eq!(
            resolve_verb(Role::Dreamer, &at_alloys, verb),
            refused,
            "{verb:?}"
        );
    }

    // A narrowed dreamer still reports `propose` and `review`.
    assert_eq!(
        effective_verbs(&at_alloys.roles, HeldBy::Agent),
        [
            "recall",
            "read",
            "graph_read",
            "events_tail",
            "propose",
            "review",
            "supersede",
        ]
    );

    // Two narrowed grants surviving at a node narrow it to their union.
    let both = [
        narrowed(1, tree.universe, &["oml:is_a"]),
        narrowed(2, tree.metals_vocab, &[ABOUT]),
    ];
    let union = effective_access(&both, &[], &tree.ancestry(tree.alloys));
    assert_eq!(
        union.edge_labels,
        Some(labels(&[OmlLabel::IsA, OmlLabel::About]))
    );
    assert_eq!(
        resolve_verb(Role::Dreamer, &union, Verb::ClaimEdge("oml:is_a")),
        Ok(())
    );
    // Without a reader grant, a refused verb is not found.
    assert_eq!(
        resolve_verb(Role::Dreamer, &union, Verb::ClaimEdge("oml:part_of")),
        Err(VerbRefusal::NotFound)
    );
}

#[test]
fn an_unnarrowed_dreamer_grant_on_the_same_node_widens_a_narrowed_one() {
    let tree = Tree::new();
    let grants = [
        narrowed(1, tree.research, &[ABOUT]),
        grant(2, tree.research, Role::Dreamer),
        grant(3, tree.research, Role::Reader),
    ];
    let access = effective_access(&grants, &[], &tree.ancestry(tree.lit_review));
    assert_eq!(access.edge_labels, None);
    assert_eq!(my_edge_labels(&access), None);
    for verb in [
        Verb::Needs(Role::Dreamer),
        Verb::ClaimEdge(ABOUT),
        Verb::ClaimEdge("oml:is_a"),
        Verb::ClaimEdge("cites"),
    ] {
        assert_eq!(
            resolve_verb(Role::Dreamer, &access, verb),
            Ok(()),
            "{verb:?}"
        );
    }

    // An unnarrowed grant whose own deny masks the node no longer survives
    // there, so the narrowed grant is the only dreamer grant left.
    let masked = [
        narrowed(1, tree.research, &[ABOUT]),
        LabelledGrant::from(plain(2, tree.universe, Role::Dreamer, &[tree.lit_review])),
        grant(3, tree.research, Role::Reader),
    ];
    let access = effective_access(&masked, &[], &tree.ancestry(tree.lit_review));
    assert_eq!(access.edge_labels, Some(labels(&[OmlLabel::About])));
    assert_eq!(
        resolve_verb(Role::Dreamer, &access, Verb::Needs(Role::Dreamer)),
        Err(VerbRefusal::GrantRefused {
            needed_role: Role::Dreamer,
            edge_labels: Some(labels(&[OmlLabel::About])),
        })
    );
}

#[test]
fn a_global_denial_empties_a_narrowed_dreamer() {
    let tree = Tree::new();
    let grants = classifier_grants(&tree);
    let denials = [denial(1, tree.metals_vocab)];
    let access = effective_access(&grants, &denials, &tree.ancestry(tree.alloys));
    assert_eq!(access, EffectiveAccess::default());
    assert_eq!(my_edge_labels(&access), None);
    assert_eq!(
        resolve_verb(Role::Dreamer, &access, Verb::ClaimEdge(ABOUT)),
        Err(VerbRefusal::NotFound)
    );
}

#[test]
fn the_roles_of_narrowed_effective_access_are_effective_roles_of_the_same_grants() {
    let tree = Tree::new();
    let denials = [denial(1, tree.lit_review)];
    for grants in [
        classifier_grants(&tree),
        term_creator_grants(&tree),
        curator_grants(&tree),
    ] {
        let unlabelled: Vec<Grant> = grants.iter().map(|g| g.grant().clone()).collect();
        for node in [
            tree.universe,
            tree.metals_vocab,
            tree.alloys,
            tree.research,
            tree.lit_review,
        ] {
            let ancestry = tree.ancestry(node);
            assert_eq!(
                effective_access(&grants, &denials, &ancestry).roles,
                effective_roles(&unlabelled, &denials, &ancestry),
                "{node}"
            );
        }
    }
}

// ---- The refusal order (roles-and-keys.md#grant-admissibility-and-refusal-order) ----

#[test]
fn a_classifier_proposing_is_a_is_grant_refused_naming_its_labels() {
    let tree = Tree::new();
    let access = effective_access(&classifier_grants(&tree), &[], &tree.ancestry(tree.alloys));
    let naming_about = Err(VerbRefusal::GrantRefused {
        needed_role: Role::Dreamer,
        edge_labels: Some(labels(&[OmlLabel::About])),
    });
    assert_eq!(
        resolve_verb(Role::Dreamer, &access, Verb::ClaimEdge("oml:is_a")),
        naming_about
    );
    // Review next and edit-as-proposal on its vocabulary answer the same.
    assert_eq!(
        resolve_verb(Role::Dreamer, &access, Verb::Needs(Role::Dreamer)),
        naming_about
    );
}

#[test]
fn a_role_outside_the_key_roles_set_is_verb_ceiling_before_any_narrowing() {
    let tree = Tree::new();
    let access = effective_access(&classifier_grants(&tree), &[], &tree.ancestry(tree.alloys));
    assert_eq!(
        resolve_verb(Role::Dreamer, &access, Verb::Needs(Role::Writer)),
        Err(VerbRefusal::VerbCeiling {
            role: Role::Dreamer,
            needed: Role::Writer,
        })
    );
    assert_eq!(
        resolve_verb(Role::Writer, &access, Verb::ClaimEdge(ABOUT)),
        Err(VerbRefusal::VerbCeiling {
            role: Role::Writer,
            needed: Role::Dreamer,
        })
    );
}

// ---- my_edge_labels (roles-and-keys.md#effective-verbs) ----

#[test]
fn my_edge_labels_is_the_narrowed_set_and_null_where_dreamer_is_unnarrowed_or_absent() {
    let tree = Tree::new();
    let classifier = classifier_grants(&tree);
    let at = |grants: &[LabelledGrant], node: Uuid| {
        my_edge_labels(&effective_access(grants, &[], &tree.ancestry(node)))
    };
    assert_eq!(at(&classifier, tree.alloys), Some(vec![ABOUT]));
    // Reader only on the content workarea: dreamer is absent there.
    assert_eq!(at(&classifier, tree.lit_review), None);
    let curator = curator_grants(&tree);
    assert_eq!(at(&curator, tree.alloys), None);
    assert_eq!(at(&curator, tree.lit_review), Some(vec![ABOUT]));
    let both = [
        narrowed(1, tree.research, &[ABOUT, "oml:part_of"]),
        narrowed(2, tree.lit_review, &["oml:is_a"]),
    ];
    assert_eq!(
        at(&both, tree.lit_review),
        Some(vec!["oml:is_a", "oml:part_of", "oml:about"])
    );
}

// ---- Member profiles (roles-and-keys.md#member-profiles) ----

#[test]
fn each_member_profile_admits_the_grants_add_member_issues() {
    let tree = Tree::new();
    let ontology = Some(CrewKind::Ontology);
    for (profile, role, grants) in [
        (
            MemberProfile::Classifier,
            Role::Dreamer,
            classifier_grants(&tree),
        ),
        (
            MemberProfile::TermCreator,
            Role::Writer,
            term_creator_grants(&tree),
        ),
        (MemberProfile::Curator, Role::Dreamer, curator_grants(&tree)),
    ] {
        assert_eq!(profile.key_role(), role);
        for held_by in [HeldBy::Agent, HeldBy::Human] {
            assert_eq!(
                check(&tree, ontology, Some(profile), role, Some(held_by), &grants),
                Ok(()),
                "{profile:?} {held_by:?}"
            );
        }
    }
}

#[test]
fn a_key_of_a_general_crew_or_of_none_without_a_profile_is_not_checked() {
    let tree = Tree::new();
    let broad = [grant(1, tree.universe, Role::Dreamer)];
    for crew in [None, Some(CrewKind::General)] {
        assert_eq!(
            check(
                &tree,
                crew,
                None,
                Role::Dreamer,
                Some(HeldBy::Service),
                &broad
            ),
            Ok(()),
            "{crew:?}"
        );
    }
}

#[test]
fn an_ontology_crew_key_without_a_profile_is_profile_required() {
    let tree = Tree::new();
    assert_eq!(
        check(
            &tree,
            Some(CrewKind::Ontology),
            None,
            Role::Dreamer,
            Some(HeldBy::Agent),
            &classifier_grants(&tree),
        ),
        mismatch(None, ProfileClause::ProfileRequired, None)
    );
}

#[test]
fn a_profile_on_a_key_of_a_general_crew_is_a_profile_mismatch_on_crew_kind() {
    let tree = Tree::new();
    let classifier = Some(MemberProfile::Classifier);
    for crew in [Some(CrewKind::General), None] {
        assert_eq!(
            check(
                &tree,
                crew,
                classifier,
                Role::Dreamer,
                Some(HeldBy::Agent),
                &classifier_grants(&tree),
            ),
            mismatch(classifier, ProfileClause::CrewKind, None),
            "{crew:?}"
        );
    }
}

#[test]
fn a_profile_on_another_key_role_is_a_profile_mismatch_on_key_role() {
    let tree = Tree::new();
    let curator = Some(MemberProfile::Curator);
    assert_eq!(
        check(
            &tree,
            Some(CrewKind::Ontology),
            curator,
            Role::Writer,
            Some(HeldBy::Agent),
            &term_creator_grants(&tree),
        ),
        mismatch(curator, ProfileClause::KeyRole, None)
    );
}

#[test]
fn a_service_attested_member_key_is_a_profile_mismatch_on_held_by() {
    let tree = Tree::new();
    let classifier = Some(MemberProfile::Classifier);
    for held_by in [Some(HeldBy::Service), None] {
        assert_eq!(
            check(
                &tree,
                Some(CrewKind::Ontology),
                classifier,
                Role::Dreamer,
                held_by,
                &classifier_grants(&tree),
            ),
            mismatch(classifier, ProfileClause::HeldBy, None),
            "{held_by:?}"
        );
    }
}

#[test]
fn a_classifier_with_an_unnarrowed_dreamer_grant_is_a_profile_mismatch_naming_the_grant() {
    let tree = Tree::new();
    let classifier = Some(MemberProfile::Classifier);
    let mut grants = classifier_grants(&tree);
    grants.push(grant(4, tree.alloys, Role::Dreamer));
    assert_eq!(
        check(
            &tree,
            Some(CrewKind::Ontology),
            classifier,
            Role::Dreamer,
            Some(HeldBy::Agent),
            &grants,
        ),
        mismatch(classifier, ProfileClause::Grant, Some(3))
    );
    // A narrowed dreamer grant off a vocabulary node, or one narrowed to
    // another label, is outside the row too.
    for stray in [
        narrowed(4, tree.research, &[ABOUT]),
        narrowed(4, tree.alloys, &["oml:is_a"]),
        narrowed(4, tree.alloys, &[ABOUT, "oml:is_a"]),
    ] {
        let mut grants = classifier_grants(&tree);
        grants.insert(0, stray);
        assert_eq!(
            check(
                &tree,
                Some(CrewKind::Ontology),
                classifier,
                Role::Dreamer,
                Some(HeldBy::Agent),
                &grants,
            ),
            mismatch(classifier, ProfileClause::Grant, Some(0))
        );
    }
}

#[test]
fn a_curator_or_term_creator_grant_outside_its_row_is_a_profile_mismatch_naming_the_grant() {
    let tree = Tree::new();
    let ontology = Some(CrewKind::Ontology);
    let curator = Some(MemberProfile::Curator);
    for stray in [
        grant(5, tree.research, Role::Dreamer),
        narrowed(5, tree.metals_vocab, &[ABOUT]),
    ] {
        let mut grants = curator_grants(&tree);
        grants.push(stray);
        assert_eq!(
            check(
                &tree,
                ontology,
                curator,
                Role::Dreamer,
                Some(HeldBy::Human),
                &grants
            ),
            mismatch(curator, ProfileClause::Grant, Some(4))
        );
    }
    let term_creator = Some(MemberProfile::TermCreator);
    for stray in [
        grant(3, tree.research, Role::Reader),
        grant(3, tree.universe, Role::Writer),
    ] {
        let mut grants = term_creator_grants(&tree);
        grants.push(stray);
        assert_eq!(
            check(
                &tree,
                ontology,
                term_creator,
                Role::Writer,
                Some(HeldBy::Agent),
                &grants
            ),
            mismatch(term_creator, ProfileClause::Grant, Some(2))
        );
    }
}

#[test]
fn a_member_key_without_its_must_grant_is_a_profile_mismatch_on_required_grant() {
    let tree = Tree::new();
    let ontology = Some(CrewKind::Ontology);
    for (profile, role, grants) in [
        (
            MemberProfile::Classifier,
            Role::Dreamer,
            vec![grant(1, tree.research, Role::Reader)],
        ),
        (
            MemberProfile::TermCreator,
            Role::Writer,
            vec![grant(1, tree.metals_vocab, Role::Reader)],
        ),
        (
            MemberProfile::Curator,
            Role::Dreamer,
            vec![
                narrowed(1, tree.research, &[ABOUT]),
                grant(2, tree.metals_vocab, Role::Reader),
            ],
        ),
        (MemberProfile::Curator, Role::Dreamer, vec![]),
    ] {
        assert_eq!(
            check(
                &tree,
                ontology,
                Some(profile),
                role,
                Some(HeldBy::Agent),
                &grants
            ),
            mismatch(Some(profile), ProfileClause::RequiredGrant, None),
            "{profile:?}"
        );
    }
}

#[test]
fn a_deny_mask_inside_a_grant_never_fails_a_profile() {
    let tree = Tree::new();
    let mut grants = curator_grants(&tree);
    grants[0] = LabelledGrant::from(plain(1, tree.metals_vocab, Role::Dreamer, &[tree.alloys]));
    assert_eq!(
        check(
            &tree,
            Some(CrewKind::Ontology),
            Some(MemberProfile::Curator),
            Role::Dreamer,
            Some(HeldBy::Agent),
            &grants,
        ),
        Ok(())
    );
}

#[test]
fn a_grant_added_to_a_profiled_key_is_checked_on_its_shape_alone() {
    let tree = Tree::new();
    let is_vocabulary = |node| tree.is_vocabulary_node(node);
    // The key's required grant was revoked; a grant within the row is still
    // admitted, since only the new shape is checked at amendment.
    let mut grants = vec![grant(1, tree.research, Role::Reader)];
    grants.push(grant(2, tree.lit_review, Role::Reader));
    assert_eq!(
        check_profile_amendment(MemberProfile::Classifier, &grants, is_vocabulary),
        Ok(())
    );
    grants.push(grant(3, tree.metals_vocab, Role::Dreamer));
    assert_eq!(
        check_profile_amendment(MemberProfile::Classifier, &grants, is_vocabulary),
        mismatch(
            Some(MemberProfile::Classifier),
            ProfileClause::Grant,
            Some(2)
        )
    );
}

#[test]
fn profile_and_clause_spell_the_wire_names() {
    assert_eq!(
        [
            MemberProfile::Classifier,
            MemberProfile::TermCreator,
            MemberProfile::Curator,
        ]
        .map(MemberProfile::as_str),
        ["classifier", "term_creator", "curator"]
    );
    assert_eq!(
        [
            ProfileClause::CrewKind,
            ProfileClause::ProfileRequired,
            ProfileClause::KeyRole,
            ProfileClause::HeldBy,
            ProfileClause::Grant,
            ProfileClause::RequiredGrant,
        ]
        .map(ProfileClause::as_str),
        [
            "crew_kind",
            "profile_required",
            "key_role",
            "held_by",
            "grant",
            "required_grant",
        ]
    );
    assert_eq!(
        [CrewKind::General, CrewKind::Ontology].map(CrewKind::as_str),
        ["general", "ontology"]
    );
}
