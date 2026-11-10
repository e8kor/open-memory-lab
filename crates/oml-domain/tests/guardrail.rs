//! Guardrails in the domain: admission and its caps, the set's order,
//! conflicts and digest, and the key's scope (ADR-0037 d2, d4, d5,
//! d7, d10). The rule grammar, the check and the cache are tested in
//! `oml-guardrail`, re-exported here (d18 Q10).

use oml_domain::EventTime;
use oml_domain::guardrail::{
    BODY_MAX_BYTES, Effect, GuardrailRefusal, RULE_MAX_BYTES, Rule, Severity, admit, conflicts,
    guardrail_order,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn rule(value: Value) -> Rule {
    match Rule::parse(&value) {
        Ok(rule) => rule,
        Err(defect) => panic!("{value} was refused: {defect}"),
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn a_never_or_must_rule_may_deny_or_ask_and_a_should_rule_asks_only() {
    for severity in [Severity::Never, Severity::Must] {
        assert!(severity.admits(Effect::Deny));
        assert!(severity.admits(Effect::Ask));
    }
    assert!(Severity::Should.admits(Effect::Ask));
    assert!(!Severity::Should.admits(Effect::Deny));

    let denies =
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["rm", "*"], "effect": "deny"});
    let asks =
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["rm", "*"], "effect": "ask"});
    assert!(admit(Severity::Must, "Do not delete.", Some(&denies)).is_ok());
    assert!(admit(Severity::Should, "Ask before deleting.", Some(&asks)).is_ok());
    let refused = admit(Severity::Should, "Do not delete.", Some(&denies)).unwrap_err();
    assert_eq!(refused.code(), "guardrail.rule_unparsable");
    // A rule is optional: a text-only guardrail is admitted with none.
    let text = admit(Severity::Never, "Never share a key.", None).expect("text only");
    assert_eq!(text.rule, None);
    assert_eq!(text.severity, Severity::Never);
}

#[test]
fn a_rule_outside_oml_gr_1_or_with_allow_is_vetoed_as_guardrail_rule_unparsable() {
    let refused = [
        json!("git push"),
        json!(["oml-gr/1"]),
        json!({"tool": "shell", "command": ["git"], "effect": "deny"}),
        json!({"rule": "oml-gr/2", "tool": "shell", "command": ["git"], "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git"], "effect": "allow"}),
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git"]}),
        json!({"rule": "oml-gr/1", "tool": "Bash", "command": ["git"], "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "web", "host": "a.example", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "shell", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "shell", "command": [], "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "shell", "command": "git push", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", 1], "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "shell", "path": "/a", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "edit", "path": "", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "read", "path": "/a", "host": "a", "effect": "ask"}),
        json!({"rule": "oml-gr/1", "tool": "fetch", "host": "*.", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "fetch", "host": "a.*.example", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "fetch", "host": "Example.COM", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "fetch", "host": "example.com.", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "mcp", "name": "", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "mcp", "name": "mcp__x", "effect": "deny", "why": "x"}),
    ];
    for value in refused {
        assert!(Rule::parse(&value).is_err(), "{value} parsed");
        let refusal = admit(Severity::Must, "A body.", Some(&value)).unwrap_err();
        assert!(
            matches!(refusal, GuardrailRefusal::RuleUnparsable(_)),
            "{value}: {refusal:?}"
        );
        assert_eq!(refusal.code(), "guardrail.rule_unparsable");
    }
    for value in [
        json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"], "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "edit", "path": "/etc/**", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "read", "path": "secrets/*", "effect": "ask"}),
        json!({"rule": "oml-gr/1", "tool": "fetch", "host": "*.example.com", "effect": "ask"}),
        json!({"rule": "oml-gr/1", "tool": "fetch", "host": "127.0.0.1", "effect": "deny"}),
        json!({"rule": "oml-gr/1", "tool": "mcp", "name": "mcp__github__*", "effect": "ask"}),
    ] {
        let parsed = rule(value.clone());
        assert_eq!(
            parsed.to_value(),
            value,
            "a rule reads back as it was written"
        );
    }
}

#[test]
fn a_body_over_300_bytes_is_vetoed_as_guardrail_too_long() {
    assert_eq!(BODY_MAX_BYTES, 300);
    assert_eq!(RULE_MAX_BYTES, 160);
    assert!(admit(Severity::Must, &"a".repeat(300), None).is_ok());
    let refusal = admit(Severity::Must, &"a".repeat(301), None).unwrap_err();
    assert_eq!(refusal.code(), "guardrail.too_long");
    // The unit is UTF-8 bytes, never characters: 100 three-byte characters
    // are 300 bytes, and one more is over.
    assert!(admit(Severity::Must, &"\u{65E5}".repeat(100), None).is_ok());
    assert_eq!(
        admit(Severity::Must, &"\u{65E5}".repeat(101), None)
            .unwrap_err()
            .code(),
        "guardrail.too_long"
    );
    // A rule is capped at 160 bytes in its canonical (RFC 8785) form.
    let pad = |n: usize| json!({"rule": "oml-gr/1", "tool": "mcp", "name": "m".repeat(n), "effect": "ask"});
    let fixed = oml_domain::canonical_bytes(&pad(0)).expect("jcs").len();
    assert!(admit(Severity::Must, "b", Some(&pad(160 - fixed))).is_ok());
    assert_eq!(
        admit(Severity::Must, "b", Some(&pad(161 - fixed)))
            .unwrap_err()
            .code(),
        "guardrail.too_long"
    );
}

fn at(s: &str) -> EventTime {
    EventTime::parse(s).expect("fixed-width timestamp")
}

#[test]
fn the_set_orders_by_severity_then_depth_then_promoted_at_then_artifact_id() {
    let early = at("2026-10-01T00:00:00.000000Z");
    let late = at("2026-10-31T00:00:00.000000Z");
    let mut rows = vec![
        (Severity::Should, 0, early.clone(), Uuid::from_u128(1)),
        (Severity::Must, 2, early.clone(), Uuid::from_u128(2)),
        (Severity::Must, 1, late.clone(), Uuid::from_u128(3)),
        (Severity::Must, 1, early.clone(), Uuid::from_u128(5)),
        (Severity::Must, 1, early, Uuid::from_u128(4)),
        (Severity::Never, 3, late, Uuid::from_u128(6)),
    ];
    guardrail_order(&mut rows, |r| (r.0, r.1, &r.2, r.3));
    let ids: Vec<u128> = rows.iter().map(|r| r.3.as_u128()).collect();
    assert_eq!(
        ids,
        vec![6, 4, 5, 3, 2, 1],
        "strictest first, never the query"
    );
}

#[test]
fn rows_of_one_claim_key_with_different_severities_each_name_the_others_in_conflict() {
    let rows = [
        ("eng-tests", Severity::Never, Uuid::from_u128(1)),
        ("eng-tests", Severity::Must, Uuid::from_u128(2)),
        ("no-push", Severity::Must, Uuid::from_u128(3)),
        ("eng-tests", Severity::Must, Uuid::from_u128(4)),
        ("no-push", Severity::Must, Uuid::from_u128(5)),
        ("alone", Severity::Should, Uuid::from_u128(6)),
    ];
    let named = conflicts(&rows, |r| (r.0, r.1, r.2));
    let ids = |v: &[u128]| v.iter().map(|&i| Uuid::from_u128(i)).collect::<Vec<_>>();
    assert_eq!(named[0], ids(&[2, 4]));
    assert_eq!(named[1], ids(&[1, 4]));
    assert_eq!(named[3], ids(&[1, 2]));
    assert!(
        named[2].is_empty(),
        "one severity restates one topic, no conflict"
    );
    assert!(named[4].is_empty());
    assert!(named[5].is_empty());
}

// --- d8: tool classes and arguments --------------------------------------

#[test]
fn a_rule_host_not_in_whatwg_serialised_form_is_vetoed() {
    for host in [
        "127.1",
        "0x7f.0.0.1",
        "2130706433",
        "0177.0.0.1",
        "127.0.0.01",
        "256.0.0.1",
        "example.123",
        "*.0.0.1",
        "*.1",
        "[0:0::1]",
        "[0:0:0:0:0:0:0:1]",
        "[::ffff:127.0.0.1]",
        "[::0001]",
        "[1::2::3]",
        "*.[::1]",
    ] {
        let value = json!({"rule": "oml-gr/1", "tool": "fetch", "host": host, "effect": "deny"});
        assert!(Rule::parse(&value).is_err(), "{host} parsed");
        assert!(
            matches!(
                admit(Severity::Must, "A body.", Some(&value)),
                Err(GuardrailRefusal::RuleUnparsable(_))
            ),
            "{host} admitted"
        );
    }
    for host in [
        "127.0.0.1",
        "10.0.0.255",
        "[::1]",
        "[2001:db8::1:0:0:1]",
        "[::ffff:7f00:1]",
        "*.example.com",
        "1.example",
    ] {
        let value = json!({"rule": "oml-gr/1", "tool": "fetch", "host": host, "effect": "deny"});
        assert_eq!(rule(value.clone()).to_value(), value, "{host}");
    }
}

// --- the glob is linear: no regular expressions, no backtracking ---------

#[test]
fn a_journalled_issuance_without_guardrail_scope_folds_as_the_issuance_default() {
    use oml_domain::guardrail::GuardrailScope;
    assert_eq!(
        GuardrailScope::of_issuance(None),
        Some(GuardrailScope::Ancestors)
    );
    assert_eq!(
        GuardrailScope::of_issuance(Some(&Value::Null)),
        Some(GuardrailScope::Ancestors)
    );
    assert_eq!(
        GuardrailScope::of_issuance(Some(&json!("workarea"))),
        Some(GuardrailScope::Workarea)
    );
    assert_eq!(
        GuardrailScope::of_issuance(Some(&json!("ancestors"))),
        Some(GuardrailScope::Ancestors)
    );
    for refused in [
        json!("off"),
        json!("Workarea"),
        json!(1),
        json!(["workarea"]),
    ] {
        assert_eq!(
            GuardrailScope::of_issuance(Some(&refused)),
            None,
            "{refused} is no scope: a caller fails closed on it"
        );
    }
}

#[test]
fn the_set_digest_names_the_served_ids_and_versions_in_order() {
    use oml_domain::guardrail::set_digest;
    let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let canonical = format!("[[\"{a}\",1],[\"{b}\",3]]");
    let expected = format!("sha256:{}", hex_digest(canonical.as_bytes()));
    assert_eq!(set_digest(&[(a, 1), (b, 3)]), expected);
    assert_ne!(set_digest(&[(a, 1), (b, 3)]), set_digest(&[(b, 3), (a, 1)]));
    assert_ne!(set_digest(&[(a, 1), (b, 3)]), set_digest(&[(a, 1), (b, 4)]));
}

/// The cache line the leaf crate writes is the RFC 8785 form the domain's
/// JCS gives the same value, so the cache's "canonical" means one thing
/// (ADR-0037 d9; two paths that must agree, asserted equal).
#[test]
fn the_cache_line_is_the_domains_canonical_form_of_its_rules() {
    use oml_domain::guardrail::{CachedRule, encode_cache};
    let rules = vec![
        CachedRule {
            artifact_id: Uuid::from_u128(1),
            body: "Never push to main.\t\u{1}é \"quoted\"".to_owned(),
            rule: rule(
                json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"], "effect": "deny"}),
            ),
        },
        CachedRule {
            artifact_id: Uuid::from_u128(2),
            body: "Ask before fetching.".to_owned(),
            rule: rule(
                json!({"rule": "oml-gr/1", "tool": "fetch", "host": "*.example.com", "effect": "ask"}),
            ),
        },
    ];
    let file = encode_cache(&rules);
    let line = file.split('\n').next().expect("a first line");
    let value: Value = serde_json::from_str(line).expect("the line is JSON");
    let canonical = oml_domain::canonical_bytes(&value).expect("canonical");
    assert_eq!(line.as_bytes(), canonical.as_slice());
}
