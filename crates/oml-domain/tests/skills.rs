//! The offered skill set: one command per name, the nearer workspace winning
//! a shared name, an ancestor winning a tie, and a held edit as a second
//! command (ADR-0080 d4, d5, d10, d16); and the `oml://` resource URIs
//! (ADR-0081 d6; ADR-0080 d7).

use oml_domain::skills::{
    HELD_SKILLS_MAX, ResourceUri, SkillCandidate, UriRefusal, offer, parse_resource_uri, skill_uri,
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
        created_at: "2026-11-17T10:00:00.000000Z".to_owned(),
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

/// A held-only workspace never takes the bare name of a skill another
/// offered workspace holds approved, however near: it answers as
/// `<name>-unreviewed` beside it, and the approved one keeps `<name>`
/// (ADR-0080 d10, d16: a held version only adds; R15).
#[test]
fn a_nearer_held_first_version_answers_as_unreviewed_beside_a_farther_approved_skill() {
    let approved = candidate("deploy", "universe.org", 1, true, false);
    let held = candidate("deploy", "universe.org.team", 0, true, true);
    let set = offer(vec![held.clone(), approved.clone()]);
    let by_command: Vec<(String, Uuid)> = set
        .offered
        .iter()
        .map(|o| (o.command.clone(), o.candidate.artifact_id))
        .collect();
    assert_eq!(
        by_command,
        vec![
            ("deploy-unreviewed".to_owned(), held.artifact_id),
            ("deploy".to_owned(), approved.artifact_id),
        ]
    );
    assert!(
        set.offered.iter().all(|o| o.overrides.is_empty()),
        "neither shadows the other: {:?}",
        set.offered
    );
    // The held one is read by URI under the command it answers.
    assert!(
        set.all.iter().any(
            |o| o.command == "deploy-unreviewed" && o.candidate.artifact_id == held.artifact_id
        )
    );
}

/// Two held-only workspaces beside an approved skill collide on
/// `<name>-unreviewed`: the nearer answers and the other stays readable by
/// URI (R15).
#[test]
fn two_held_only_places_beside_an_approved_skill_give_the_unreviewed_command_to_the_nearer() {
    let approved = candidate("deploy", "universe.org", 2, true, false);
    let near = candidate("deploy", "universe.org.team.app", 0, true, true);
    let far = candidate("deploy", "universe.org.team", 1, true, true);
    let set = offer(vec![far.clone(), approved.clone(), near.clone()]);
    assert_eq!(
        commands(&set.offered),
        vec![
            (
                "deploy-unreviewed".to_owned(),
                "universe.org.team.app".to_owned()
            ),
            ("deploy".to_owned(), "universe.org".to_owned()),
        ]
    );
    assert_eq!(
        set.offered[0].overrides,
        vec!["universe.org.team".to_owned()]
    );
    assert!(
        set.all
            .iter()
            .any(|o| o.command == "deploy-unreviewed" && o.candidate.artifact_id == far.artifact_id)
    );
}

/// With no approved version anywhere offered, the nearest held version 1
/// takes `<name>`, as d10 says, and a farther held one is shadowed.
#[test]
fn with_no_approved_version_anywhere_the_nearest_held_first_version_takes_the_name() {
    let near = candidate("draft", "universe.org.team", 0, true, true);
    let far = candidate("draft", "universe.org", 1, true, true);
    let set = offer(vec![far, near.clone()]);
    assert_eq!(set.offered.len(), 1);
    assert_eq!(set.offered[0].command, "draft");
    assert_eq!(set.offered[0].candidate.artifact_id, near.artifact_id);
    assert_eq!(set.offered[0].overrides, vec!["universe.org".to_owned()]);
}

/// A shadowed approved workspace's held edit is not offered: only the
/// workspace that answers `<name>` offers its edit as `<name>-unreviewed`.
#[test]
fn a_shadowed_approved_skill_offers_no_unreviewed_edit() {
    let near = candidate("deploy", "universe.org.team", 0, true, false);
    let far = candidate("deploy", "universe.org", 1, true, false);
    let mut far_edit = candidate("deploy", "universe.org", 1, true, true);
    far_edit.version = 2;
    let set = offer(vec![far, far_edit, near.clone()]);
    assert_eq!(
        commands(&set.offered),
        vec![("deploy".to_owned(), "universe.org.team".to_owned())]
    );
}

/// Two descendants at one distance that hold one name go to the smaller
/// path, whatever the read order (R2).
#[test]
fn two_descendants_at_one_distance_resolve_to_the_smaller_path() {
    let b = candidate("lint", "universe.team.b", 1, false, false);
    let a = candidate("lint", "universe.team.a", 1, false, false);
    for order in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]] {
        let set = offer(order);
        assert_eq!(set.offered.len(), 1);
        assert_eq!(set.offered[0].candidate.path, "universe.team.a");
        assert_eq!(set.offered[0].overrides, vec!["universe.team.b".to_owned()]);
    }
}

/// Held versions of one version number in one workspace go to the later
/// created, then the larger artifact id, so the answer never depends on
/// read order.
#[test]
fn ties_among_held_versions_go_to_the_later_created_then_the_larger_id() {
    let mut early = candidate("draft", "universe.team", 0, true, true);
    early.created_at = "2026-11-17T09:00:00.000000Z".to_owned();
    let late = candidate("draft", "universe.team", 0, true, true);
    for order in [
        vec![early.clone(), late.clone()],
        vec![late.clone(), early.clone()],
    ] {
        assert_eq!(
            offer(order).offered[0].candidate.artifact_id,
            late.artifact_id
        );
    }
    let mut low = candidate("same", "universe.team", 0, true, true);
    low.artifact_id = Uuid::from_u128(1);
    let mut high = candidate("same", "universe.team", 0, true, true);
    high.artifact_id = Uuid::from_u128(u128::MAX);
    for order in [
        vec![low.clone(), high.clone()],
        vec![high.clone(), low.clone()],
    ] {
        assert_eq!(
            offer(order).offered[0].candidate.artifact_id,
            high.artifact_id
        );
    }
}

/// Held commands are capped on their own, after the sort, and dropped
/// before any promoted one is (ADR-0080 d10, as the held guardrails are).
#[test]
fn sixty_held_skills_give_at_most_fifty_held_commands_and_every_promoted_one_is_kept() {
    let mut candidates: Vec<SkillCandidate> = (0..60)
        .map(|i| candidate(&format!("held-{i:02}"), "universe.team", 0, true, true))
        .collect();
    let promoted: Vec<SkillCandidate> = (0..5)
        .map(|i| candidate(&format!("zz-approved-{i}"), "universe.team", 0, true, false))
        .collect();
    candidates.extend(promoted.iter().cloned());
    let set = offer(candidates);
    let held = set.offered.iter().filter(|o| o.candidate.held).count();
    assert_eq!(held, HELD_SKILLS_MAX);
    assert_eq!(HELD_SKILLS_MAX, 50);
    for p in &promoted {
        assert!(
            set.offered
                .iter()
                .any(|o| o.candidate.artifact_id == p.artifact_id),
            "{} dropped",
            p.name
        );
    }
    // `all` is the URI lookup and keeps every held command.
    assert_eq!(set.all.iter().filter(|o| o.candidate.held).count(), 60);
    // The first fifty held in the set's order are kept.
    assert!(set.offered.iter().any(|o| o.command == "held-49"));
    assert!(!set.offered.iter().any(|o| o.command == "held-50"));
}

/// Every offered command stays readable by URI however many held ones are
/// shadowed before it in the set's order: 26 names held as version 1 in two
/// sibling workspaces at one distance, nothing approved, offer each name
/// from the smaller path, and the listed URI of every one, and of every
/// shadowed one, resolves in `all` (ADR-0080 d7, d10; R15, R19).
#[test]
fn every_offered_held_command_stays_readable_by_uri_past_the_held_cap() {
    let mut candidates = Vec::new();
    for i in 0..26 {
        let name = format!("s{i:02}");
        candidates.push(candidate(&name, "universe.team.wip-a", 1, false, true));
        candidates.push(candidate(&name, "universe.team.wip-b", 1, false, true));
    }
    let set = offer(candidates.clone());
    assert_eq!(set.offered.len(), 26);
    for o in &set.offered {
        assert!(
            set.all
                .iter()
                .any(|a| a.candidate.artifact_id == o.candidate.artifact_id
                    && a.command == o.command),
            "{} from {} is offered and not readable by URI",
            o.command,
            o.candidate.path
        );
    }
    for c in &candidates {
        assert!(
            set.all
                .iter()
                .any(|a| a.candidate.artifact_id == c.artifact_id),
            "{} at {} is not readable by URI",
            c.name,
            c.path
        );
    }
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
        "oml://tenant/team/skill/deploy",
        "oml://workspace/team/skill/",
        "oml://workspace/team/skill/a/b",
        "oml://workspace//skill/deploy",
        "oml://workspace/team/claim/x",
        "oml://workspace/team/skill/deploy?passage=x",
    ] {
        assert_eq!(parse_resource_uri(uri), Err(UriRefusal::NotFound), "{uri}");
    }
}
