//! The offered skill set: one command per name, the nearer workspace winning
//! a shared name, an ancestor winning a tie, and a held edit as a second
//! command (ADR-0080 d4, d5, d10, d16); and the `oml://` resource URIs
//! (ADR-0081 d6; ADR-0080 d7).

use oml_domain::skills::{
    ResourceUri, SkillCandidate, UriRefusal, offer, parse_resource_uri, skill_uri,
};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use uuid::Uuid;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn node_of(path: &str) -> Uuid {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    Uuid::from_u128(u128::from(h.finish()))
}

fn candidate(name: &str, path: &str, distance: u32, ancestor: bool, held: bool) -> SkillCandidate {
    SkillCandidate {
        artifact_id: Uuid::from_u128(u128::from(NEXT.fetch_add(1, Ordering::Relaxed))),
        name: name.to_owned(),
        node: node_of(path),
        path: path.to_owned(),
        distance,
        ancestor,
        held,
        version: 1,
    }
}

fn commands(offered: &[oml_domain::skills::OfferedSkill]) -> Vec<(String, String)> {
    offered
        .iter()
        .map(|o| (o.command.clone(), o.candidate.path.clone()))
        .collect()
}

#[test]
fn the_nearer_skill_wins_a_shared_name_and_an_ancestor_wins_a_tie() {
    let set = offer(vec![
        // The scope's parent and grandparent both hold `deploy`: the parent
        // is nearer.
        candidate("deploy", "universe.org", 2, true, false),
        candidate("deploy", "universe.org.team", 1, true, false),
        // A child inside an in-progress root holds `lint` at distance 1, as
        // the parent does: the ancestor wins the tie.
        candidate("lint", "universe.org.team.app.wip", 1, false, false),
        candidate("lint", "universe.org.team", 1, true, false),
        candidate("review", "universe.org.team.app", 0, true, false),
    ]);
    assert_eq!(
        commands(&set.offered),
        vec![
            ("review".to_owned(), "universe.org.team.app".to_owned()),
            ("deploy".to_owned(), "universe.org.team".to_owned()),
            ("lint".to_owned(), "universe.org.team".to_owned()),
        ],
        "nearest first, then by name"
    );
    let deploy = &set.offered[1];
    assert_eq!(deploy.overrides, vec!["universe.org".to_owned()]);
    let lint = &set.offered[2];
    assert_eq!(lint.overrides, vec!["universe.org.team.app.wip".to_owned()]);
    // Every candidate stays readable as a resource, the shadowed ones too.
    assert_eq!(set.all.len(), 5);
}

#[test]
fn a_held_edit_of_a_promoted_skill_is_a_second_command_and_a_held_first_version_takes_the_name() {
    let mut promoted = candidate("deploy", "universe.team", 0, true, false);
    promoted.version = 2;
    let mut edit = candidate("deploy", "universe.team", 0, true, true);
    edit.version = 3;
    let draft = candidate("draft", "universe.team", 0, true, true);
    let set = offer(vec![edit.clone(), promoted.clone(), draft.clone()]);
    let by_command: Vec<(String, Uuid)> = set
        .offered
        .iter()
        .map(|o| (o.command.clone(), o.candidate.artifact_id))
        .collect();
    assert_eq!(
        by_command,
        vec![
            ("deploy".to_owned(), promoted.artifact_id),
            ("deploy-unreviewed".to_owned(), edit.artifact_id),
            ("draft".to_owned(), draft.artifact_id),
        ]
    );
}

#[test]
fn a_held_version_older_than_the_promoted_one_is_not_offered() {
    let mut promoted = candidate("deploy", "universe.team", 0, true, false);
    promoted.version = 3;
    let mut stale = candidate("deploy", "universe.team", 0, true, true);
    stale.version = 2;
    let set = offer(vec![promoted.clone(), stale]);
    assert_eq!(set.offered.len(), 1);
    assert_eq!(set.offered[0].candidate.artifact_id, promoted.artifact_id);
}

#[test]
fn a_skill_uri_names_the_workspace_and_the_command_and_parses_back() {
    let uri = skill_uri("team", "deploy-unreviewed");
    assert_eq!(uri, "oml://workspace/team/skill/deploy-unreviewed");
    assert_eq!(
        parse_resource_uri(&uri),
        Ok(ResourceUri::Skill {
            workspace: "team".to_owned(),
            name: "deploy-unreviewed".to_owned(),
        })
    );
    assert_eq!(
        parse_resource_uri("oml://workspace/team/doc/guides/setup.md?passage=install"),
        Ok(ResourceUri::Document {
            workspace: "team".to_owned(),
            path: "guides/setup.md".to_owned(),
            passage: Some("install".to_owned()),
        })
    );
}

#[test]
fn a_resource_uri_with_a_percent_or_a_reserved_character_is_refused_as_escaped() {
    for uri in [
        "oml://workspace/te%61m/skill/deploy",
        "oml://workspace/team/skill/de%2Fploy",
        "oml://workspace/team/skill/deploy;x",
        "oml://workspace/team/doc/a@b",
    ] {
        assert_eq!(parse_resource_uri(uri), Err(UriRefusal::Escaped), "{uri}");
    }
}

#[test]
fn a_resource_uri_outside_the_grammar_is_not_found() {
    for uri in [
        "https://workspace/team/skill/deploy",
        "oml://workarea/team/skill/deploy",
        "oml://workspace/team/skill/",
        "oml://workspace/team/skill/a/b",
        "oml://workspace//skill/deploy",
        "oml://workspace/team/claim/x",
        "oml://workspace/team/skill/deploy?passage=x",
    ] {
        assert_eq!(parse_resource_uri(uri), Err(UriRefusal::NotFound), "{uri}");
    }
}
