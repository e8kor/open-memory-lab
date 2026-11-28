//! `KEY_ROLES` names every key role in [`Role`]'s order, so the release
//! marker that reads it at each tag sees the set the code serves (ADR-0097
//! d18; scripts/release_markers.py).

use oml_domain::Role;
use oml_domain::model::KEY_ROLES;

#[test]
fn key_roles_names_every_role_in_its_declared_order() {
    let every = [
        Role::Reader,
        Role::Contributor,
        Role::Submitter,
        Role::Reviewer,
        Role::Admin,
        Role::Operator,
        Role::Maintenance,
    ];
    // A role added to the enum and not here fails to compile below.
    for role in every {
        match role {
            Role::Reader
            | Role::Contributor
            | Role::Submitter
            | Role::Reviewer
            | Role::Admin
            | Role::Operator
            | Role::Maintenance => {}
        }
    }
    let spelled: Vec<&str> = every.iter().map(|r| r.as_str()).collect();
    assert_eq!(KEY_ROLES, spelled.as_slice());
}
