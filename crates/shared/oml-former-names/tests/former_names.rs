//! The former-name map of ADR-0091 d4: each rule, its identity on what the
//! v0.8.0 server writes, and its fail-closed case.

use oml_former_names::{FORMER_EVENT_TYPES, UnmappedFormerName, read_event};
use serde_json::{Value, json};

fn read(event_type: &str, payload: &Value) -> (String, Value) {
    let read = read_event(event_type, payload).expect("mapped");
    (read.event_type, read.payload)
}

#[test]
fn every_former_workarea_event_type_reads_as_the_same_suffix_under_workspace() {
    for (former, current) in FORMER_EVENT_TYPES {
        let suffix = former.strip_prefix("workarea.").expect("a workarea type");
        assert_eq!(*current, format!("workspace.{suffix}"));
        assert_eq!(read(former, &json!({})).0, *current);
    }
    for former in [
        "workarea.create",
        "workarea.mode_set",
        "workarea.federation_set",
        "workarea.status_proposed",
        "workarea.status_transition",
        "workarea.crew_updated",
    ] {
        assert!(
            FORMER_EVENT_TYPES.iter().any(|(f, _)| *f == former),
            "{former} is one of the catalog's six"
        );
    }
}

#[test]
fn a_payload_key_holding_workarea_at_any_depth_reads_with_workspace() {
    let (_, payload) = read(
        "key.issued",
        &json!({
            "default_workarea_id": "a",
            "nested": {"workarea_ids": ["b"], "list": [{"to_workarea_id": "c"}]},
        }),
    );
    assert_eq!(
        payload,
        json!({
            "default_workspace_id": "a",
            "nested": {"workspace_ids": ["b"], "list": [{"to_workspace_id": "c"}]},
        })
    );
}

#[test]
fn a_workarea_create_reads_as_a_workspace_create_with_its_status_renamed() {
    let (event_type, payload) = read(
        "workarea.create",
        &json!({"workarea_id": "w", "name": "universe", "status": "committed"}),
    );
    assert_eq!(event_type, "workspace.create");
    assert_eq!(
        payload,
        json!({"workspace_id": "w", "name": "universe", "status": "protected"})
    );
}

#[test]
fn a_status_transition_reads_its_statuses_and_its_snapshot_in_the_new_names() {
    let (event_type, payload) = read(
        "workarea.status_transition",
        &json!({
            "workarea_id": "w",
            "from": "committed",
            "to": "validated",
            "policy_snapshot": {"k": 2, "required_roles": ["dreamer"], "children_all_validated": true},
        }),
    );
    assert_eq!(event_type, "workspace.status_transition");
    assert_eq!(
        payload,
        json!({
            "workspace_id": "w",
            "from": "protected",
            "to": "frozen",
            "policy_snapshot": {"k": 2, "required_roles": ["reviewer"], "children_all_frozen": true},
        })
    );
    let (_, proposed) = read(
        "workarea.status_proposed",
        &json!({"from": "development", "to": "committed"}),
    );
    assert_eq!(proposed, json!({"from": "development", "to": "protected"}));
}

#[test]
fn a_mode_sets_from_is_a_mode_and_is_not_read_as_a_status() {
    let (_, payload) = read(
        "workarea.mode_set",
        &json!({"mode": "standard", "from": "committed"}),
    );
    assert_eq!(payload, json!({"mode": "standard", "from": "committed"}));
}

#[test]
fn a_key_issued_reads_its_role_holder_and_scope_in_the_new_names() {
    let (_, payload) = read(
        "key.issued",
        &json!({
            "role": "dreamer",
            "held_by": "human",
            "guardrail_scope": "workarea",
            "default_workarea_id": "w",
            "name": "the-dreamer",
        }),
    );
    assert_eq!(
        payload,
        json!({
            "role": "reviewer",
            "used_by": "person",
            "guardrail_scope": "workspace",
            "default_workspace_id": "w",
            "name": "the-dreamer",
        })
    );
}

#[test]
fn a_grant_issued_reads_its_role_as_reviewer() {
    let (_, payload) = read(
        "grant.issued",
        &json!({"role": "dreamer", "workarea_node_id": "n"}),
    );
    assert_eq!(
        payload,
        json!({"role": "reviewer", "workspace_node_id": "n"})
    );
}

#[test]
fn a_review_ratify_snapshot_reads_its_required_roles_as_reviewer() {
    let (_, payload) = read(
        "review.ratify",
        &json!({"policy_snapshot": {"k": 1, "required_roles": ["dreamer", "admin"], "human_required": true}}),
    );
    assert_eq!(
        payload,
        json!({"policy_snapshot": {"k": 1, "required_roles": ["reviewer", "admin"], "human_required": true}})
    );
}

#[test]
fn names_assigned_reads_a_workarea_kind_as_workspace_and_leaves_names_alone() {
    let (_, payload) = read(
        "system.names_assigned",
        &json!({"names": [
            {"kind": "workarea", "id": "a", "name": "workarea-notes", "prior_name": null},
            {"kind": "key", "id": "b", "name": "the-dreamer", "prior_name": null},
        ]}),
    );
    assert_eq!(
        payload,
        json!({"names": [
            {"kind": "workspace", "id": "a", "name": "workarea-notes", "prior_name": null},
            {"kind": "key", "id": "b", "name": "the-dreamer", "prior_name": null},
        ]})
    );
}

#[test]
fn values_outside_the_closed_lists_keep_their_spelling() {
    // A role, status or holder word anywhere but its listed field is content.
    let payload = json!({
        "role": "dreamer",
        "status": "committed",
        "held_by": "human",
        "note": "workarea",
    });
    let (_, read_payload) = read("lifecycle.promote", &payload);
    assert_eq!(read_payload, payload);
}

#[test]
fn the_map_is_the_identity_on_every_event_the_new_server_writes() {
    let written = [
        (
            "workspace.create",
            json!({"workspace_id": "w", "status": "protected"}),
        ),
        (
            "workspace.status_transition",
            json!({"from": "protected", "to": "frozen", "policy_snapshot": {"required_roles": ["reviewer"], "children_all_frozen": true}}),
        ),
        (
            "key.issued",
            json!({"role": "reviewer", "used_by": "person", "guardrail_scope": "workspace", "default_workspace_id": "w"}),
        ),
        (
            "grant.issued",
            json!({"role": "reviewer", "workspace_node_id": "n"}),
        ),
        ("submission.remember", json!({"workspace_id": "w"})),
    ];
    for (event_type, payload) in written {
        let once = read_event(event_type, &payload).expect("mapped");
        assert_eq!(once.event_type, event_type);
        assert_eq!(once.payload, payload);
        let twice = read_event(&once.event_type, &once.payload).expect("mapped");
        assert_eq!(twice, once, "applying the map twice changes nothing");
    }
}

#[test]
fn a_workarea_event_type_outside_the_list_fails_closed_naming_the_event_and_the_name() {
    let err: UnmappedFormerName =
        read_event("workarea.unheard_of", &json!({})).expect_err("unmapped");
    assert_eq!(err.event_type, "workarea.unheard_of");
    assert_eq!(err.name, "workarea.unheard_of");
    assert_eq!(err.doctor_check(), "journal_unmapped_former_name");
    assert!(err.to_string().contains("workarea.unheard_of"));
}
