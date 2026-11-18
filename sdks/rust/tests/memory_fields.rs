//! The fields a served memory carries that the web console draws on its
//! card (design notes row 10): the SDK reads each as the API serves it, and
//! an API that omits one still decodes.

use oml_sdk::Memory;
use serde_json::json;

fn served() -> serde_json::Value {
    json!({
        "artifact_id": "6f1c2d4e-0000-4000-8000-000000000003",
        "claim_key": "backups-nightly", "version": 3, "tip": false,
        "workspace_id": "w1", "workspace_path": "universe.alice.proj",
        "kind": "memory", "state": "held", "unreviewed": true, "tokens": 12,
        "retirement": false,
        "replaces": {"artifact_id": "6f1c2d4e-0000-4000-8000-000000000002", "version": 2},
        "held_expires_at": "2026-12-01T00:00:00Z",
        "created_at": "2026-11-12T10:00:00Z", "promoted_at": null,
        "anchor": {"repo": "github.com/e8kor/oml", "rev": "51a0c3e9d2f7aa", "path": "README.md"},
        "stale": "changed",
        "source_checked": {"rev": "51a0c3e9d2f7", "at": "2026-11-09T04:00:12Z", "result": "moved",
                           "moved_to": {"path": "docs/x.md", "fragment": null}},
        "origin": {"kind": "repo"}, "provenance": "agent",
        "derives_from": ["6f1c2d4e-0000-4000-8000-000000000001"],
        "attachments": [{"digest": "sha256:00", "media_type": "application/pdf", "size_bytes": 10}],
        "plaintext_digest": "sha256:ab",
        "text": "Nightly backups run at 02:00."
    })
}

/// A served memory's kind, provenance, origin, retirement, stale, source
/// check, attachments, tokens, unreviewed mark and held expiry are read as
/// the API serves them.
#[test]
fn the_sdks_memory_reads_every_field_the_card_draws() {
    let m: Memory = serde_json::from_value(served()).expect("a memory");
    assert_eq!(m.kind.as_deref(), Some("memory"));
    assert_eq!(m.provenance.as_deref(), Some("agent"));
    assert_eq!(m.origin.as_ref().map(|o| o.kind.as_str()), Some("repo"));
    assert!(!m.retirement);
    assert_eq!(m.stale.as_deref(), Some("changed"));
    let checked = m.source_checked.as_ref().expect("a source check");
    assert_eq!(
        (checked.rev.as_str(), checked.result.as_str()),
        ("51a0c3e9d2f7", "moved")
    );
    assert_eq!(
        checked.moved_to.as_ref().map(|t| t.path.as_str()),
        Some("docs/x.md")
    );
    assert_eq!(m.attachments.len(), 1);
    assert_eq!(
        m.attachments[0].media_type.as_deref(),
        Some("application/pdf")
    );
    assert_eq!(m.tokens, Some(12));
    assert!(m.unreviewed);
    assert_eq!(m.held_expires_at.as_deref(), Some("2026-12-01T00:00:00Z"));
    assert_eq!(m.derives_from.len(), 1);
    assert_eq!(m.plaintext_digest.as_deref(), Some("sha256:ab"));
}

/// An API that serves none of them still decodes: each is absent, empty or
/// false.
#[test]
fn a_memory_without_the_card_fields_still_decodes() {
    let mut v = served();
    for field in [
        "kind",
        "provenance",
        "origin",
        "retirement",
        "stale",
        "source_checked",
        "derives_from",
        "attachments",
        "tokens",
        "unreviewed",
        "held_expires_at",
        "plaintext_digest",
    ] {
        v.as_object_mut().unwrap().remove(field);
    }
    let m: Memory = serde_json::from_value(v).expect("a memory");
    assert!(m.kind.is_none() && m.origin.is_none() && m.source_checked.is_none());
    assert!(!m.retirement && !m.unreviewed && m.attachments.is_empty());
}

/// The admin read's holder, promotion time and scan finding, which the
/// admin's One memory page prints on its card, are read as served.
#[test]
fn the_admin_read_carries_its_holder_its_promotion_and_its_scan_finding() {
    let read: oml_sdk::AdminRead = serde_json::from_value(json!({
        "artifact_id": "m1", "claim_key": "c", "version": 1, "state": "held",
        "workspace_path": "universe.alice", "text": "t", "key": "alice-laptop",
        "key_id": "k1", "principal": "alice-laptop", "user": "alice",
        "created_at": "2026-11-11T10:00:00Z", "used_by": "agent",
        "promoted_at": null, "scan_finding": "injection.override: ignore-instructions at /body line 2"
    }))
    .expect("an admin read");
    assert_eq!(read.used_by.as_deref(), Some("agent"));
    assert_eq!(read.promoted_at, None);
    assert!(
        read.scan_finding
            .as_deref()
            .is_some_and(|f| f.starts_with("injection"))
    );
}

#[test]
fn a_held_guardrail_decodes_with_no_promotion_and_its_unreviewed_mark() {
    let held: oml_sdk::ServedGuardrail = serde_json::from_value(json!({
        "claim_key": "no-force-push", "version": 2, "workspace_path": "universe.alice",
        "severity": "never", "text": "Never force-push.", "promoted_at": null,
        "unreviewed": true, "effect_inside": "ask", "verdicts": []
    }))
    .expect("a held guardrail");
    assert_eq!(held.promoted_at, None);
    assert!(held.unreviewed);
    assert_eq!(held.effect_inside.as_deref(), Some("ask"));
    let promoted: oml_sdk::ServedGuardrail = serde_json::from_value(json!({
        "claim_key": "k", "version": 1, "severity": "must", "text": "t",
        "promoted_at": "2026-11-09T10:11:12Z"
    }))
    .expect("a promoted guardrail");
    assert!(!promoted.unreviewed);
    assert_eq!(promoted.effect_inside, None);
}

/// A recall whose `kinds` does not name steering answers `steering:
/// "not_requested"`, which the SDK reads as an empty list; any other
/// string is refused (ADR-0092 d2).
#[test]
fn a_recall_answering_steering_not_requested_decodes_with_no_steering() {
    let answer: oml_sdk::RecallResponse =
        serde_json::from_value(json!({"steering": "not_requested", "hits": []}))
            .expect("a recall");
    assert!(answer.steering.is_empty());
    assert!(
        serde_json::from_value::<oml_sdk::RecallResponse>(json!({"steering": "x", "hits": []}))
            .is_err()
    );
}
