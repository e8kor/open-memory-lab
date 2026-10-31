//! Grant admissibility, effective access, the refusal order, effective verbs
//! and content-tail visibility (ADR-0025 d1, d2, d14, d15;
//! roles-and-keys.md, workareas.md, http-api.md#events-tail).

use std::collections::{BTreeMap, BTreeSet};

use oml_domain::access::{
    AccessRefusal, Grant, IssuanceRefusal, KeyDenial, TailCaller, TailSubject,
    admissible_grant_roles, check_issuance, effective_roles, effective_verbs, resolve,
    tail_visible,
};
use oml_domain::{HeldBy, Role};
use proptest::prelude::*;
use uuid::Uuid;

const ALL_ROLES: [Role; 5] = [
    Role::Reader,
    Role::Writer,
    Role::Dreamer,
    Role::Admin,
    Role::Operator,
];

fn grant(n: u128, node: Uuid, role: Role, deny: &[Uuid]) -> Grant {
    Grant {
        grant_id: Uuid::from_u128(0x6000 + n),
        node,
        role,
        deny: deny.to_vec(),
    }
}

fn denial(n: u128, node: Uuid) -> KeyDenial {
    KeyDenial {
        denial_id: Uuid::from_u128(0xD000 + n),
        node,
    }
}

fn roles(list: &[Role]) -> BTreeSet<Role> {
    list.iter().copied().collect()
}

/// The example tenant of workareas.md: `universe` → `research-crew` →
/// {`lit-review`, `experiments` → `run-grid`}, and `universe` → `ops-crew`.
struct Tree {
    universe: Uuid,
    research_crew: Uuid,
    ops_crew: Uuid,
    lit_review: Uuid,
    experiments: Uuid,
    run_grid: Uuid,
}

impl Tree {
    fn new() -> Self {
        Self {
            universe: Uuid::from_u128(0xA0),
            research_crew: Uuid::from_u128(0xA1),
            ops_crew: Uuid::from_u128(0xA2),
            lit_review: Uuid::from_u128(0xA3),
            experiments: Uuid::from_u128(0xA4),
            run_grid: Uuid::from_u128(0xA5),
        }
    }

    /// The ancestry of `node`, from the node to the root.
    fn ancestry(&self, node: Uuid) -> Vec<Uuid> {
        let parent = |n: Uuid| -> Option<Uuid> {
            if n == self.research_crew || n == self.ops_crew {
                Some(self.universe)
            } else if n == self.lit_review || n == self.experiments {
                Some(self.research_crew)
            } else if n == self.run_grid {
                Some(self.experiments)
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
}

// ---- Grant admissibility (roles-and-keys.md#grant-admissibility-and-refusal-order) ----

#[test]
fn a_reader_key_admits_reader_grants_only() {
    assert_eq!(admissible_grant_roles(Role::Reader), &[Role::Reader]);
}

#[test]
fn a_writer_key_admits_reader_and_writer_grants_only() {
    assert_eq!(
        admissible_grant_roles(Role::Writer),
        &[Role::Reader, Role::Writer]
    );
    let tree = Tree::new();
    for role in ALL_ROLES {
        let result = check_issuance(
            Role::Admin,
            Role::Writer,
            &[grant(1, tree.research_crew, role, &[])],
        );
        if matches!(role, Role::Reader | Role::Writer) {
            assert_eq!(result, Ok(()), "{role:?}");
        } else {
            assert_eq!(
                result,
                Err(IssuanceRefusal::IssuerCeiling {
                    role,
                    admissible: &[Role::Reader, Role::Writer],
                    grant_index: Some(0),
                }),
                "{role:?}"
            );
        }
    }
}

#[test]
fn a_dreamer_key_admits_reader_and_dreamer_grants_only() {
    assert_eq!(
        admissible_grant_roles(Role::Dreamer),
        &[Role::Reader, Role::Dreamer]
    );
    let tree = Tree::new();
    for role in ALL_ROLES {
        let result = check_issuance(
            Role::Admin,
            Role::Dreamer,
            &[grant(1, tree.research_crew, role, &[])],
        );
        if matches!(role, Role::Reader | Role::Dreamer) {
            assert_eq!(result, Ok(()), "{role:?}");
        } else {
            assert_eq!(
                result,
                Err(IssuanceRefusal::IssuerCeiling {
                    role,
                    admissible: &[Role::Reader, Role::Dreamer],
                    grant_index: Some(0),
                }),
                "{role:?}"
            );
        }
    }
}

#[test]
fn admin_and_operator_keys_admit_no_grant() {
    assert!(admissible_grant_roles(Role::Admin).is_empty());
    assert!(admissible_grant_roles(Role::Operator).is_empty());
    let tree = Tree::new();
    // An operator issues a tenant's first admin key; that key carries no grant.
    assert_eq!(
        check_issuance(
            Role::Operator,
            Role::Admin,
            &[grant(1, tree.research_crew, Role::Reader, &[])],
        ),
        Err(IssuanceRefusal::IssuerCeiling {
            role: Role::Reader,
            admissible: &[],
            grant_index: Some(0),
        })
    );
    assert_eq!(
        check_issuance(
            Role::Operator,
            Role::Operator,
            &[grant(1, tree.research_crew, Role::Writer, &[])],
        ),
        Err(IssuanceRefusal::IssuerCeiling {
            role: Role::Writer,
            admissible: &[],
            grant_index: Some(0),
        })
    );
    assert_eq!(check_issuance(Role::Admin, Role::Admin, &[]), Ok(()));
    assert_eq!(check_issuance(Role::Operator, Role::Admin, &[]), Ok(()));
}

#[test]
fn an_admin_key_with_a_dreamer_grant_is_role_exclusive_before_issuer_ceiling() {
    // Admin admits no grant, so the dreamer grant would also fail step 3;
    // the two-key rule answers first (issuance order step 1).
    let tree = Tree::new();
    let grants = [
        grant(1, tree.research_crew, Role::Reader, &[]),
        grant(2, tree.research_crew, Role::Dreamer, &[]),
    ];
    assert_eq!(
        check_issuance(Role::Admin, Role::Admin, &grants),
        Err(IssuanceRefusal::RoleExclusive)
    );
    assert_eq!(
        check_issuance(Role::Operator, Role::Admin, &grants),
        Err(IssuanceRefusal::RoleExclusive)
    );
}

#[test]
fn an_admin_minting_an_operator_key_is_issuer_ceiling_naming_the_issuable_roles() {
    assert_eq!(
        check_issuance(Role::Admin, Role::Operator, &[]),
        Err(IssuanceRefusal::IssuerCeiling {
            role: Role::Operator,
            admissible: &[Role::Reader, Role::Writer, Role::Dreamer, Role::Admin],
            grant_index: None,
        })
    );
    // The operator issues operator keys and a tenant's first admin key only.
    assert_eq!(
        check_issuance(Role::Operator, Role::Writer, &[]),
        Err(IssuanceRefusal::IssuerCeiling {
            role: Role::Writer,
            admissible: &[Role::Admin, Role::Operator],
            grant_index: None,
        })
    );
}

#[test]
fn the_key_role_ceiling_answers_before_grant_admissibility() {
    // Issuance order: step 2 (key role) before step 3 (grants).
    let tree = Tree::new();
    assert_eq!(
        check_issuance(
            Role::Admin,
            Role::Operator,
            &[grant(1, tree.research_crew, Role::Dreamer, &[])],
        ),
        Err(IssuanceRefusal::IssuerCeiling {
            role: Role::Operator,
            admissible: &[Role::Reader, Role::Writer, Role::Dreamer, Role::Admin],
            grant_index: None,
        })
    );
}

#[test]
fn the_first_inadmissible_grant_is_named_by_its_index() {
    let tree = Tree::new();
    let grants = [
        grant(1, tree.research_crew, Role::Reader, &[]),
        grant(2, tree.research_crew, Role::Writer, &[]),
        grant(3, tree.lit_review, Role::Dreamer, &[]),
        grant(4, tree.ops_crew, Role::Admin, &[]),
    ];
    assert_eq!(
        check_issuance(Role::Admin, Role::Writer, &grants),
        Err(IssuanceRefusal::IssuerCeiling {
            role: Role::Dreamer,
            admissible: &[Role::Reader, Role::Writer],
            grant_index: Some(2),
        })
    );
}

// ---- Effective access (workareas.md#effective-access-union-of-attenuated-grants-then-deny-wins) ----

#[test]
fn the_workareas_md_worked_example_resolves_row_for_row() {
    // Grant A = reader on research-crew with a deny on experiments;
    // grant B = reader on experiments; a global deny on run-grid.
    let tree = Tree::new();
    let a = grant(1, tree.research_crew, Role::Reader, &[tree.experiments]);
    let b = grant(2, tree.experiments, Role::Reader, &[]);
    let both = [a.clone(), b.clone()];
    let global = [denial(1, tree.run_grid)];
    let yes = roles(&[Role::Reader]);
    let no = BTreeSet::new();
    // | Node | Grant A (attenuated) | Grant B | Union | Effective |
    let rows = [
        (tree.research_crew, &yes, &no, &yes, &yes),
        (tree.lit_review, &yes, &no, &yes, &yes),
        (tree.experiments, &no, &yes, &yes, &yes),
        (tree.run_grid, &no, &yes, &yes, &no),
    ];
    for (node, grant_a, grant_b, union, effective) in rows {
        let ancestry = tree.ancestry(node);
        assert_eq!(
            &effective_roles(std::slice::from_ref(&a), &[], &ancestry),
            grant_a,
            "grant A at {node}"
        );
        assert_eq!(
            &effective_roles(std::slice::from_ref(&b), &[], &ancestry),
            grant_b,
            "grant B at {node}"
        );
        assert_eq!(
            &effective_roles(&both, &[], &ancestry),
            union,
            "union at {node}"
        );
        assert_eq!(
            &effective_roles(&both, &global, &ancestry),
            effective,
            "effective at {node}"
        );
    }
}

#[test]
fn a_per_grant_deny_never_defeats_another_grant() {
    let tree = Tree::new();
    let grants = [
        grant(1, tree.research_crew, Role::Writer, &[tree.experiments]),
        grant(2, tree.research_crew, Role::Reader, &[]),
    ];
    // The writer grant's deny holes the writer grant only.
    assert_eq!(
        effective_roles(&grants, &[], &tree.ancestry(tree.run_grid)),
        roles(&[Role::Reader])
    );
    assert_eq!(
        effective_roles(&grants, &[], &tree.ancestry(tree.lit_review)),
        roles(&[Role::Reader, Role::Writer])
    );
}

#[test]
fn a_global_deny_defeats_every_grant() {
    let tree = Tree::new();
    let grants = [
        grant(1, tree.research_crew, Role::Writer, &[]),
        grant(2, tree.research_crew, Role::Reader, &[]),
        grant(3, tree.experiments, Role::Reader, &[]),
    ];
    let global = [denial(1, tree.experiments)];
    assert!(effective_roles(&grants, &global, &tree.ancestry(tree.experiments)).is_empty());
    assert!(effective_roles(&grants, &global, &tree.ancestry(tree.run_grid)).is_empty());
    assert_eq!(
        effective_roles(&grants, &global, &tree.ancestry(tree.lit_review)),
        roles(&[Role::Reader, Role::Writer])
    );
}

#[test]
fn a_grant_covers_its_subtree_and_nothing_above_or_beside_it() {
    let tree = Tree::new();
    let grants = [grant(1, tree.experiments, Role::Reader, &[])];
    assert!(effective_roles(&grants, &[], &tree.ancestry(tree.research_crew)).is_empty());
    assert!(effective_roles(&grants, &[], &tree.ancestry(tree.lit_review)).is_empty());
    assert!(effective_roles(&grants, &[], &tree.ancestry(tree.universe)).is_empty());
    assert!(effective_roles(&grants, &[], &[]).is_empty());
    assert_eq!(
        effective_roles(&grants, &[], &tree.ancestry(tree.run_grid)),
        roles(&[Role::Reader])
    );
}

// ---- The refusal order (roles-and-keys.md#grant-admissibility-and-refusal-order) ----

#[test]
fn a_role_outside_the_key_roles_set_is_verb_ceiling() {
    // Step 1 needs no node: it answers even where the role is effective.
    let all = roles(&[Role::Reader, Role::Writer, Role::Dreamer]);
    assert_eq!(
        resolve(Role::Reader, &all, Role::Writer),
        Err(AccessRefusal::VerbCeiling {
            role: Role::Reader,
            needed: Role::Writer,
        })
    );
    assert_eq!(
        resolve(Role::Writer, &all, Role::Dreamer),
        Err(AccessRefusal::VerbCeiling {
            role: Role::Writer,
            needed: Role::Dreamer,
        })
    );
    assert_eq!(
        resolve(Role::Dreamer, &BTreeSet::new(), Role::Writer),
        Err(AccessRefusal::VerbCeiling {
            role: Role::Dreamer,
            needed: Role::Writer,
        })
    );
    for key_role in [Role::Admin, Role::Operator] {
        for needed in [Role::Reader, Role::Writer, Role::Dreamer] {
            assert_eq!(
                resolve(key_role, &all, needed),
                Err(AccessRefusal::VerbCeiling {
                    role: key_role,
                    needed,
                })
            );
        }
    }
}

#[test]
fn an_unreadable_node_is_not_found() {
    let tree = Tree::new();
    let grants = [
        grant(1, tree.research_crew, Role::Writer, &[]),
        grant(2, tree.research_crew, Role::Reader, &[]),
    ];
    let eff = effective_roles(&grants, &[], &tree.ancestry(tree.ops_crew));
    assert_eq!(
        resolve(Role::Writer, &eff, Role::Writer),
        Err(AccessRefusal::NotFound)
    );
    assert_eq!(
        resolve(Role::Writer, &eff, Role::Reader),
        Err(AccessRefusal::NotFound)
    );
}

#[test]
fn a_readable_node_without_the_role_is_grant_refused() {
    let tree = Tree::new();
    let grants = [grant(1, tree.research_crew, Role::Reader, &[])];
    let eff = effective_roles(&grants, &[], &tree.ancestry(tree.lit_review));
    assert_eq!(
        resolve(Role::Writer, &eff, Role::Writer),
        Err(AccessRefusal::GrantRefused {
            needed_role: Role::Writer,
        })
    );
    let dreamer_grants = [grant(2, tree.research_crew, Role::Reader, &[])];
    let eff = effective_roles(&dreamer_grants, &[], &tree.ancestry(tree.experiments));
    assert_eq!(
        resolve(Role::Dreamer, &eff, Role::Dreamer),
        Err(AccessRefusal::GrantRefused {
            needed_role: Role::Dreamer,
        })
    );
}

#[test]
fn a_writer_only_grant_admits_submit_on_its_node() {
    let tree = Tree::new();
    let grants = [grant(1, tree.research_crew, Role::Writer, &[])];
    for node in [tree.research_crew, tree.experiments] {
        let eff = effective_roles(&grants, &[], &tree.ancestry(node));
        assert_eq!(resolve(Role::Writer, &eff, Role::Writer), Ok(()));
        // No reader grant: recall there is absence, never grant_refused.
        assert_eq!(
            resolve(Role::Writer, &eff, Role::Reader),
            Err(AccessRefusal::NotFound)
        );
    }
}

#[test]
fn one_writer_key_with_writer_and_reader_grants_remembers_and_recalls() {
    let tree = Tree::new();
    let grants = [
        grant(1, tree.research_crew, Role::Writer, &[]),
        grant(2, tree.research_crew, Role::Reader, &[]),
    ];
    let eff = effective_roles(&grants, &[], &tree.ancestry(tree.lit_review));
    assert_eq!(resolve(Role::Writer, &eff, Role::Writer), Ok(()));
    assert_eq!(resolve(Role::Writer, &eff, Role::Reader), Ok(()));
}

// ---- Effective verbs (roles-and-keys.md#effective-verbs) ----

#[test]
fn effective_verbs_for_writer_and_reader_are_the_union_of_the_table() {
    assert_eq!(
        effective_verbs(&roles(&[Role::Writer, Role::Reader]), HeldBy::Agent),
        [
            "recall",
            "read",
            "graph_read",
            "events_tail",
            "remember",
            "learn",
            "attach",
            "supersede",
        ]
    );
    assert_eq!(
        effective_verbs(&roles(&[Role::Reader]), HeldBy::Human),
        ["recall", "read", "graph_read", "events_tail"]
    );
    assert!(effective_verbs(&BTreeSet::new(), HeldBy::Agent).is_empty());
}

#[test]
fn effective_verbs_for_dreamer_and_reader_list_supersede_once_in_table_order() {
    assert_eq!(
        effective_verbs(&roles(&[Role::Dreamer, Role::Reader]), HeldBy::Agent),
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
    // No key role admits both writer and dreamer, but the union is still
    // free of duplicates in table order.
    assert_eq!(
        effective_verbs(&roles(&[Role::Writer, Role::Dreamer]), HeldBy::Agent),
        [
            "remember",
            "learn",
            "attach",
            "supersede",
            "propose",
            "review",
        ]
    );
}

#[test]
fn a_service_keys_effective_verbs_omit_review() {
    assert_eq!(
        effective_verbs(&roles(&[Role::Dreamer, Role::Reader]), HeldBy::Service),
        [
            "recall",
            "read",
            "graph_read",
            "events_tail",
            "propose",
            "supersede",
        ]
    );
}

// ---- Content-tail visibility (http-api.md#events-tail, ADR-0025 d15) ----

const ME: Uuid = Uuid::from_u128(0xC1);
const OTHER: Uuid = Uuid::from_u128(0xC2);

/// A writer-role key holding writer and reader on `research-crew`, tailing
/// `research-crew`.
fn writer_reader_grants(tree: &Tree) -> Vec<Grant> {
    vec![
        grant(1, tree.research_crew, Role::Writer, &[]),
        grant(2, tree.research_crew, Role::Reader, &[]),
    ]
}

fn caller<'a>(grants: &'a [Grant], denials: &'a [KeyDenial], scope: Uuid) -> TailCaller<'a> {
    TailCaller {
        own_keys: std::slice::from_ref(&ME),
        grants,
        denials,
        scope,
    }
}

fn subject(workareas: &[Vec<Uuid>], author: Option<Uuid>, promoted: bool) -> TailSubject<'_> {
    TailSubject {
        workareas,
        author_key: author,
        promoted,
        self_signed: false,
        outside_audience: false,
    }
}

#[test]
fn another_keys_submission_event_is_invisible() {
    let tree = Tree::new();
    let grants = writer_reader_grants(&tree);
    let me = caller(&grants, &[], tree.research_crew);
    let at = [tree.ancestry(tree.lit_review)];
    for event_type in [
        "submission.remember",
        "submission.learn",
        "submission.attach",
        "simulation.write",
    ] {
        assert!(
            !tail_visible(event_type, &subject(&at, Some(OTHER), false), &me),
            "{event_type}"
        );
        assert!(
            tail_visible(event_type, &subject(&at, Some(ME), false), &me),
            "{event_type}"
        );
    }
}

#[test]
fn an_authors_own_submission_is_visible_without_a_reader_grant() {
    let tree = Tree::new();
    let grants = [grant(1, tree.research_crew, Role::Writer, &[])];
    let me = caller(&grants, &[], tree.research_crew);
    let at = [tree.ancestry(tree.experiments)];
    assert!(tail_visible(
        "submission.remember",
        &subject(&at, Some(ME), false),
        &me
    ));
}

#[test]
fn a_global_denial_hides_the_authors_own_submission() {
    let tree = Tree::new();
    let grants = writer_reader_grants(&tree);
    let global = [denial(1, tree.experiments)];
    let me = caller(&grants, &global, tree.research_crew);
    let denied = [tree.ancestry(tree.run_grid)];
    assert!(!tail_visible(
        "submission.remember",
        &subject(&denied, Some(ME), false),
        &me
    ));
    let open = [tree.ancestry(tree.lit_review)];
    assert!(tail_visible(
        "submission.remember",
        &subject(&open, Some(ME), false),
        &me
    ));
}

#[test]
fn an_event_outside_the_tails_subtree_is_hidden() {
    let tree = Tree::new();
    let grants = writer_reader_grants(&tree);
    let me = caller(&grants, &[], tree.experiments);
    let beside = [tree.ancestry(tree.lit_review)];
    assert!(!tail_visible(
        "submission.remember",
        &subject(&beside, Some(ME), false),
        &me
    ));
    assert!(!tail_visible(
        "lifecycle.promote",
        &subject(&beside, Some(OTHER), true),
        &me
    ));
    assert!(!tail_visible(
        "workarea.create",
        &subject(&beside, None, false),
        &me
    ));
}

#[test]
fn a_promoted_subjects_lifecycle_event_is_visible_to_a_reader_of_its_workarea() {
    let tree = Tree::new();
    let reader = [grant(1, tree.research_crew, Role::Reader, &[])];
    let me = caller(&reader, &[], tree.research_crew);
    let at = [tree.ancestry(tree.lit_review)];
    for event_type in [
        "lifecycle.promote",
        "lifecycle.supersede",
        "lifecycle.demote",
        "lifecycle.erase",
        "lifecycle.tip_override",
        "lifecycle.reassign",
    ] {
        assert!(
            tail_visible(event_type, &subject(&at, Some(OTHER), true), &me),
            "{event_type}"
        );
    }
    // A writer-only key is no reader: another author's promotion stays out.
    let writer = [grant(1, tree.research_crew, Role::Writer, &[])];
    let writer_only = caller(&writer, &[], tree.research_crew);
    assert!(!tail_visible(
        "lifecycle.promote",
        &subject(&at, Some(OTHER), true),
        &writer_only
    ));
}

#[test]
fn a_never_promoted_subjects_lifecycle_event_reaches_only_its_author() {
    // The erase of a held version never reaches another key (R3).
    let tree = Tree::new();
    let grants = writer_reader_grants(&tree);
    let me = caller(&grants, &[], tree.research_crew);
    let at = [tree.ancestry(tree.lit_review)];
    assert!(!tail_visible(
        "lifecycle.erase",
        &subject(&at, Some(OTHER), false),
        &me
    ));
    assert!(tail_visible(
        "lifecycle.erase",
        &subject(&at, Some(ME), false),
        &me
    ));
}

#[test]
fn a_self_signed_subjects_lifecycle_event_shows_only_on_a_tail_scoped_to_its_workarea() {
    let tree = Tree::new();
    let reader = [grant(1, tree.research_crew, Role::Reader, &[])];
    let at = [tree.ancestry(tree.experiments)];
    let self_signed = TailSubject {
        self_signed: true,
        ..subject(&at, Some(OTHER), true)
    };
    assert!(!tail_visible(
        "lifecycle.promote",
        &self_signed,
        &caller(&reader, &[], tree.research_crew)
    ));
    assert!(tail_visible(
        "lifecycle.promote",
        &self_signed,
        &caller(&reader, &[], tree.experiments)
    ));
    // The author clause carries no self-signed condition.
    let own = TailSubject {
        author_key: Some(ME),
        ..self_signed
    };
    assert!(tail_visible(
        "lifecycle.promote",
        &own,
        &caller(&reader, &[], tree.research_crew)
    ));
}

#[test]
fn a_subject_outside_the_audience_is_hidden_even_from_its_author() {
    let tree = Tree::new();
    let grants = writer_reader_grants(&tree);
    let me = caller(&grants, &[], tree.research_crew);
    let at = [tree.ancestry(tree.lit_review)];
    for (event_type, author) in [
        ("simulation.write", Some(ME)),
        ("lifecycle.promote", Some(ME)),
        ("lifecycle.promote", Some(OTHER)),
    ] {
        let fenced = TailSubject {
            outside_audience: true,
            ..subject(&at, author, true)
        };
        assert!(!tail_visible(event_type, &fenced, &me), "{event_type}");
    }
}

#[test]
fn a_reassign_is_visible_through_either_listed_workarea() {
    // A key reading only experiments sees a promoted version move in from
    // ops-crew, and sees one leave experiments for ops-crew.
    let tree = Tree::new();
    let reader = [grant(1, tree.experiments, Role::Reader, &[])];
    let me = caller(&reader, &[], tree.experiments);
    let inbound = [tree.ancestry(tree.ops_crew), tree.ancestry(tree.run_grid)];
    let outbound = [tree.ancestry(tree.run_grid), tree.ancestry(tree.ops_crew)];
    assert!(tail_visible(
        "lifecycle.reassign",
        &subject(&inbound, Some(OTHER), true),
        &me
    ));
    assert!(tail_visible(
        "lifecycle.reassign",
        &subject(&outbound, Some(OTHER), true),
        &me
    ));
    // Neither listed workarea readable: hidden.
    let elsewhere = [tree.ancestry(tree.ops_crew), tree.ancestry(tree.lit_review)];
    assert!(!tail_visible(
        "lifecycle.reassign",
        &subject(&elsewhere, Some(OTHER), true),
        &me
    ));
}

#[test]
fn a_workarea_event_needs_reader_on_its_node_and_a_relate_event_on_both_endpoints() {
    let tree = Tree::new();
    let reader = [grant(1, tree.research_crew, Role::Reader, &[])];
    let me = caller(&reader, &[], tree.experiments);
    for event_type in [
        "workarea.create",
        "workarea.reparent",
        "workarea.status_proposed",
        "workarea.status_transition",
        "workarea.crew_updated",
    ] {
        let node = [tree.ancestry(tree.run_grid)];
        assert!(
            tail_visible(event_type, &subject(&node, None, false), &me),
            "{event_type}"
        );
        let unread = [tree.ancestry(tree.ops_crew)];
        assert!(
            !tail_visible(event_type, &subject(&unread, None, false), &me),
            "{event_type}"
        );
    }
    for event_type in ["workarea.relate_proposed", "workarea.relate_ratified"] {
        // Both readable, one inside the tail's subtree.
        let both = [tree.ancestry(tree.run_grid), tree.ancestry(tree.lit_review)];
        assert!(
            tail_visible(event_type, &subject(&both, None, false), &me),
            "{event_type}"
        );
        // One endpoint unreadable.
        let one = [tree.ancestry(tree.run_grid), tree.ancestry(tree.ops_crew)];
        assert!(
            !tail_visible(event_type, &subject(&one, None, false), &me),
            "{event_type}"
        );
        // Both readable, neither inside the tail's subtree.
        let outside = [
            tree.ancestry(tree.lit_review),
            tree.ancestry(tree.research_crew),
        ];
        assert!(
            !tail_visible(event_type, &subject(&outside, None, false), &me),
            "{event_type}"
        );
    }
}

#[test]
fn review_and_key_events_are_never_visible_on_the_content_tail() {
    // Even to the acting key's own author, reading everything.
    let tree = Tree::new();
    let grants = writer_reader_grants(&tree);
    let me = caller(&grants, &[], tree.research_crew);
    let at = [tree.ancestry(tree.lit_review)];
    for event_type in [
        "review.serve",
        "review.approve",
        "review.reject",
        "review.ratify",
        "review.reopen",
        "claim_edge.proposed",
        "key.issued",
        "key.revoked",
        "key.rotated",
        "key.denied",
        "key.denial_lifted",
        "grant.issued",
        "grant.revoked",
        "tenant.created",
        "tenant.genesis",
        "system.signing_key_rotated",
        "system.readlog_anchor",
        // Types the tables do not map are not shown either.
        "ban.imposed",
        "simulation.opened",
        "export.requested",
        "submission.unknown",
        "lifecycle.unknown",
    ] {
        assert!(
            !tail_visible(event_type, &subject(&at, Some(ME), true), &me),
            "{event_type}"
        );
    }
}

#[test]
fn a_subject_with_the_wrong_number_of_workareas_is_hidden() {
    let tree = Tree::new();
    let grants = writer_reader_grants(&tree);
    let me = caller(&grants, &[], tree.research_crew);
    let none: [Vec<Uuid>; 0] = [];
    let two = [
        tree.ancestry(tree.lit_review),
        tree.ancestry(tree.experiments),
    ];
    let one = [tree.ancestry(tree.lit_review)];
    assert!(!tail_visible(
        "submission.remember",
        &subject(&none, Some(ME), false),
        &me
    ));
    assert!(!tail_visible(
        "submission.remember",
        &subject(&two, Some(ME), false),
        &me
    ));
    assert!(!tail_visible(
        "lifecycle.promote",
        &subject(&two, Some(ME), true),
        &me
    ));
    assert!(!tail_visible(
        "workarea.relate_ratified",
        &subject(&one, None, false),
        &me
    ));
}

// ---- The brute-force oracle ----

/// A random tree: node 0 is the root and node i > 0 has a parent below i.
fn arb_tree() -> impl Strategy<Value = Vec<usize>> {
    (2_usize..10).prop_flat_map(|n| {
        (1..n)
            .map(|i| (0..i).boxed())
            .collect::<Vec<_>>()
            .prop_map(|parents| {
                let mut all = vec![0];
                all.extend(parents);
                all
            })
    })
}

fn node_id(i: usize) -> Uuid {
    Uuid::from_u128(0x1_0000 + i as u128)
}

/// Every node in the subtree of `root`, by walking children downward —
/// independent of the ancestry walk the function under test is given.
fn subtree(parents: &[usize], root: usize) -> BTreeSet<usize> {
    let mut children: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (child, &parent) in parents.iter().enumerate().skip(1) {
        children.entry(parent).or_default().push(child);
    }
    let mut seen = BTreeSet::from([root]);
    let mut frontier = vec![root];
    while let Some(at) = frontier.pop() {
        for &child in children.get(&at).into_iter().flatten() {
            if seen.insert(child) {
                frontier.push(child);
            }
        }
    }
    seen
}

fn ancestry_of(parents: &[usize], node: usize) -> Vec<Uuid> {
    let mut chain = vec![node_id(node)];
    let mut at = node;
    while at != 0 {
        at = parents[at];
        chain.push(node_id(at));
    }
    chain
}

proptest! {
    #[test]
    fn effective_roles_equals_a_brute_force_oracle(
        (parents, grant_specs, denial_nodes) in arb_tree().prop_flat_map(|parents| {
            let n = parents.len();
            let grant_spec = (
                0..n,
                proptest::sample::select(vec![Role::Reader, Role::Writer, Role::Dreamer]),
                proptest::collection::vec(0..n, 0..3),
            );
            (
                Just(parents),
                proptest::collection::vec(grant_spec, 0..5),
                proptest::collection::vec(0..n, 0..3),
            )
        }),
    ) {
        let grants: Vec<Grant> = grant_specs
            .iter()
            .enumerate()
            .map(|(i, (node, role, deny))| Grant {
                grant_id: Uuid::from_u128(0x6000 + i as u128),
                node: node_id(*node),
                role: *role,
                deny: deny.iter().map(|&d| node_id(d)).collect(),
            })
            .collect();
        let denials: Vec<KeyDenial> = denial_nodes
            .iter()
            .enumerate()
            .map(|(i, &node)| KeyDenial {
                denial_id: Uuid::from_u128(0xD000 + i as u128),
                node: node_id(node),
            })
            .collect();
        for node in 0..parents.len() {
            let globally_denied = denial_nodes
                .iter()
                .any(|&d| subtree(&parents, d).contains(&node));
            let oracle: BTreeSet<Role> = if globally_denied {
                BTreeSet::new()
            } else {
                grant_specs
                    .iter()
                    .filter(|(g, _, _)| subtree(&parents, *g).contains(&node))
                    .filter(|(_, _, deny)| {
                        deny.iter().all(|&d| !subtree(&parents, d).contains(&node))
                    })
                    .map(|(_, role, _)| *role)
                    .collect()
            };
            prop_assert_eq!(
                effective_roles(&grants, &denials, &ancestry_of(&parents, node)),
                oracle,
                "node {}",
                node
            );
        }
    }
}
