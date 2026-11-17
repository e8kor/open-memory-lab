//! ADR-0084 d8 and d13: the import plan, a pure function of the bundle and
//! the target's view.

mod common;

use common::*;
use oml_bundle::plan::{Ledger, Outcome, TargetTip, TargetView, plan_import};
use oml_bundle::{ReleaseRef, VersionEntry, VersionLine, sha256_digest};

/// The bundle as `read_bundle` returns it, signed by the test key.
fn verified(
    contents: oml_bundle::BundleContents,
    authenticated: bool,
) -> oml_bundle::VerifiedBundle {
    oml_bundle::VerifiedBundle {
        contents,
        signer_fingerprint: test_fingerprint(),
        authenticated,
    }
}

fn test_fingerprint() -> String {
    oml_bundle::key_fingerprint(&test_key().verifying_key())
}

fn never(_: &VersionLine, _: &TargetTip) -> bool {
    false
}

fn tip(digest: &str, written_by_last_import: bool) -> TargetTip {
    TargetTip {
        artifact_id: "tgt-1".to_owned(),
        version: 4,
        plaintext_digest: digest.to_owned(),
        retirement: false,
        written_by_last_import,
        imported_from_this_source: written_by_last_import,
    }
}

fn outcome_of(plan: &oml_bundle::plan::Plan, key: &str) -> Outcome {
    plan.keys
        .iter()
        .find(|k| k.claim_key == key)
        .expect("planned")
        .outcome
}

/// The fixture's versions and the view a first import of it would leave.
fn imported_view() -> TargetView {
    let mut view = TargetView::default();
    for entry in &contents().versions {
        if let VersionEntry::Version(v) = entry {
            view.tips
                .insert(v.claim_key.clone(), tip(&v.plaintext_digest, true));
            view.imported.insert((v.artifact_id.clone(), v.version));
        }
    }
    view.imported.insert(("art-4".to_owned(), 1));
    view.ledger = Some(Ledger {
        exported_at: manifest().exported_at,
        release_manifest_digest: None,
        signer_fingerprint: test_fingerprint(),
    });
    view
}

#[test]
fn a_first_import_plans_every_key_as_new() {
    let c = contents();
    let plan = plan_import(
        &verified(c.clone(), true),
        &TargetView::default(),
        false,
        &never,
    )
    .unwrap();
    assert_eq!(plan.keys.len(), 3);
    assert_eq!(plan.count(Outcome::New), 3);
    assert!(plan.erasure_notices.is_empty());
}

#[test]
fn a_re_import_of_the_same_bundle_writes_nothing() {
    let c = contents();
    let plan = plan_import(&verified(c.clone(), true), &imported_view(), false, &never).unwrap();
    assert_eq!(plan.count(Outcome::Unchanged), 3);
}

#[test]
fn a_key_whose_text_changed_on_the_source_plans_changed_naming_the_target_tip() {
    let mut c = contents();
    if let VersionEntry::Version(v) = &mut c.versions[2] {
        v.text = Some("Quote revised.".to_owned());
        v.plaintext_digest = sha256_digest(b"Quote revised.");
    }
    let plan = plan_import(&verified(c.clone(), true), &imported_view(), false, &never).unwrap();
    let key = plan
        .keys
        .iter()
        .find(|k| k.claim_key == "supplier-quote")
        .unwrap();
    assert_eq!(key.outcome, Outcome::Changed);
    assert_eq!(key.expected_version, Some(4));
    assert_eq!(key.source, Some(("art-3".to_owned(), 2)));
}

#[test]
fn a_change_the_callers_format_diff_finds_equivalent_plans_equivalent() {
    let mut c = contents();
    if let VersionEntry::Version(v) = &mut c.versions[2] {
        v.plaintext_digest = sha256_digest(b"Quote attached. ");
    }
    let plan = plan_import(
        &verified(c.clone(), true),
        &imported_view(),
        false,
        &|_, _| true,
    )
    .unwrap();
    assert_eq!(outcome_of(&plan, "supplier-quote"), Outcome::Equivalent);
}

#[test]
fn a_key_changed_on_the_target_since_the_last_import_plans_conflict() {
    let mut c = contents();
    if let VersionEntry::Version(v) = &mut c.versions[2] {
        v.plaintext_digest = sha256_digest(b"Quote revised.");
    }
    let mut view = imported_view();
    view.tips.insert(
        "supplier-quote".to_owned(),
        TargetTip {
            imported_from_this_source: true,
            ..tip(&sha256_digest(b"edited on the target"), false)
        },
    );
    let plan = plan_import(&verified(c.clone(), true), &view, false, &|_, _| true).unwrap();
    assert_eq!(outcome_of(&plan, "supplier-quote"), Outcome::Conflict);
}

#[test]
fn a_key_the_target_holds_of_its_own_plans_conflict_unless_the_text_is_equal() {
    let c = contents();
    let mut view = TargetView::default();
    view.tips.insert(
        "deploy-runbook".to_owned(),
        tip(&sha256_digest(b"ours"), false),
    );
    view.tips.insert(
        "supplier-quote".to_owned(),
        tip(&sha256_digest(b"Quote attached."), false),
    );
    let plan = plan_import(&verified(c.clone(), true), &view, false, &never).unwrap();
    assert_eq!(outcome_of(&plan, "deploy-runbook"), Outcome::Conflict);
    assert_eq!(outcome_of(&plan, "supplier-quote"), Outcome::Unchanged);
    assert_eq!(outcome_of(&plan, "long-design-note"), Outcome::New);
}

#[test]
fn a_retirement_on_the_source_plans_removed() {
    let mut c = contents();
    let mut retired = match &c.versions[0] {
        VersionEntry::Version(v) => (**v).clone(),
        VersionEntry::Erased(_) => unreachable!(),
    };
    retired.version = 2;
    retired.retirement = true;
    retired.source_event = None;
    retired.text = Some("Retired.".to_owned());
    retired.plaintext_digest = sha256_digest(b"Retired.");
    c.versions.push(VersionEntry::Version(Box::new(retired)));
    let plan = plan_import(&verified(c.clone(), true), &imported_view(), false, &never).unwrap();
    assert_eq!(outcome_of(&plan, "deploy-runbook"), Outcome::Removed);
    let fresh = plan_import(
        &verified(c.clone(), true),
        &TargetView::default(),
        false,
        &never,
    )
    .unwrap();
    assert_eq!(outcome_of(&fresh, "deploy-runbook"), Outcome::Unchanged);
}

#[test]
fn a_key_a_full_release_no_longer_holds_plans_removed() {
    let mut c = contents();
    c.manifest.mode = oml_bundle::Mode::Release;
    c.manifest.release = Some(ReleaseRef {
        label: "1.1.0".to_owned(),
        manifest_digest: sha256_digest(b"1.1.0"),
    });
    c.versions.remove(0);
    let plan = plan_import(&verified(c.clone(), true), &imported_view(), false, &never).unwrap();
    assert_eq!(outcome_of(&plan, "deploy-runbook"), Outcome::Removed);
}

#[test]
fn a_key_a_since_bundle_drops_plans_removed() {
    let mut c = contents();
    c.manifest.mode = oml_bundle::Mode::Release;
    c.manifest.release = Some(ReleaseRef {
        label: "1.1.0".to_owned(),
        manifest_digest: sha256_digest(b"1.1.0"),
    });
    c.manifest.since = Some(ReleaseRef {
        label: "1.0.0".to_owned(),
        manifest_digest: sha256_digest(b"1.0.0"),
    });
    c.manifest.dropped = vec!["deploy-runbook".to_owned()];
    c.versions.remove(0);
    let mut view = imported_view();
    view.ledger = Some(Ledger {
        exported_at: "2026-11-10T00:00:00.000000Z".to_owned(),
        release_manifest_digest: Some(sha256_digest(b"1.0.0")),
        signer_fingerprint: test_fingerprint(),
    });
    let plan = plan_import(&verified(c.clone(), true), &view, false, &never).unwrap();
    assert_eq!(outcome_of(&plan, "deploy-runbook"), Outcome::Removed);
}

#[test]
fn a_since_bundle_over_another_base_is_refused_as_since_mismatch() {
    let mut c = contents();
    c.manifest.mode = oml_bundle::Mode::Release;
    c.manifest.release = Some(ReleaseRef {
        label: "1.2.0".to_owned(),
        manifest_digest: sha256_digest(b"1.2.0"),
    });
    c.manifest.since = Some(ReleaseRef {
        label: "1.1.0".to_owned(),
        manifest_digest: sha256_digest(b"1.1.0"),
    });
    let mut view = imported_view();
    view.ledger = Some(Ledger {
        exported_at: "2026-11-10T00:00:00.000000Z".to_owned(),
        release_manifest_digest: Some(sha256_digest(b"1.0.0")),
        signer_fingerprint: test_fingerprint(),
    });
    let err = plan_import(&verified(c.clone(), true), &view, false, &never).expect_err("refused");
    assert_eq!(err.code(), "import.since_mismatch");
}

#[test]
fn a_bundle_older_than_the_ledger_is_refused_unless_allow_older() {
    let c = contents();
    let mut view = imported_view();
    view.ledger = Some(Ledger {
        exported_at: "2026-11-18T00:00:00.000000Z".to_owned(),
        release_manifest_digest: None,
        signer_fingerprint: test_fingerprint(),
    });
    let err = plan_import(&verified(c.clone(), true), &view, false, &never).expect_err("refused");
    assert_eq!(err.code(), "import.bundle_older");
    let plan = plan_import(&verified(c.clone(), true), &view, true, &never).unwrap();
    assert!(plan.allow_older_used);
}

#[test]
fn a_marker_for_a_version_already_imported_raises_an_erasure_notice() {
    let c = contents();
    let plan = plan_import(&verified(c.clone(), true), &imported_view(), false, &never).unwrap();
    assert_eq!(plan.erasure_notices, vec![("art-4".to_owned(), 1)]);
}

#[test]
fn the_bundle_tip_is_the_newest_promoted_version_before_any_held_one() {
    let mut c = contents();
    let mut held = match &c.versions[2] {
        VersionEntry::Version(v) => (**v).clone(),
        VersionEntry::Erased(_) => unreachable!(),
    };
    held.version = 3;
    held.state = oml_bundle::State::Held;
    held.plaintext_digest = sha256_digest(b"unreviewed");
    c.versions.push(VersionEntry::Version(Box::new(held)));
    let plan = plan_import(&verified(c.clone(), true), &imported_view(), false, &never).unwrap();
    let key = plan
        .keys
        .iter()
        .find(|k| k.claim_key == "supplier-quote")
        .unwrap();
    assert_eq!(key.outcome, Outcome::Unchanged);
    assert_eq!(key.source, Some(("art-3".to_owned(), 2)));
}

#[test]
fn a_bundle_signed_by_another_key_than_the_ledgers_is_refused_as_source_key_changed() {
    let c = contents();
    let mut bundle = verified(c, true);
    bundle.signer_fingerprint = "f".repeat(64);
    let err = plan_import(&bundle, &imported_view(), false, &never).expect_err("refused");
    assert_eq!(err.code(), "import.source_key_changed");
}

#[test]
fn an_unauthenticated_bundle_over_an_existing_ledger_is_refused_as_source_key_changed() {
    let c = contents();
    let err =
        plan_import(&verified(c, false), &imported_view(), false, &never).expect_err("refused");
    assert_eq!(err.code(), "import.source_key_changed");
}

#[test]
fn a_first_import_says_whether_its_signature_was_authenticated() {
    let c = contents();
    let plan = plan_import(
        &verified(c.clone(), false),
        &TargetView::default(),
        false,
        &never,
    )
    .unwrap();
    assert!(!plan.authenticated);
    assert_eq!(plan.signer_fingerprint, test_fingerprint());
    let plan = plan_import(&verified(c, true), &TargetView::default(), false, &never).unwrap();
    assert!(plan.authenticated);
}
