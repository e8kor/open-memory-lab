//! The guardrail lines the Guardrails view lists, from the wire's
//! `GuardrailSet` (ADR-0037 d6, d7, d17). They are the lines of the frame
//! `oml-guardrail` renders for `oml guardrails show` (every line, even over
//! the cap; nothing marked checked), after its header: `oml-web` may not
//! link `oml-guardrail` (ADR-0070 d1), so the reading is written here and
//! `the_view_lists_the_lines_of_the_frame_oml_guardrail_renders` holds the
//! two equal.

use oml_sdk::{GuardrailSet, ServedGuardrail};

/// The hook caps a line's provenance at 200 characters (ADR-0037 d7), as
/// `oml_guardrail::PROVENANCE_MAX_CHARS` does.
pub const PROVENANCE_MAX_CHARS: usize = 200;

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cut(text: &str, cap: usize) -> String {
    if text.chars().count() <= cap {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(cap.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// One guardrail's line: severity, text and provenance; a conflict names
/// the other rows (ADR-0037 d4, d6).
fn line(g: &ServedGuardrail) -> String {
    let humans = g.verdicts.iter().filter(|v| v.held_by == "human").count();
    let reviewers = g.verdicts.len();
    let provenance = format!(
        "{}, {} v{}, ratified {} by {reviewers} reviewer{} ({humans} human)",
        g.workarea_path.as_deref().unwrap_or("?"),
        one_line(&g.claim_key),
        g.version,
        g.promoted_at.get(..10).unwrap_or(&g.promoted_at),
        if reviewers == 1 { "" } else { "s" },
    );
    let provenance = cut(&provenance, PROVENANCE_MAX_CHARS);
    let mut out = format!("- [{}] {} ({provenance})", g.severity, one_line(&g.text));
    if let Some(others) = g.conflict.as_ref().filter(|c| !c.is_empty()) {
        out.push_str(&format!(" conflicts with {}", others.join(", ")));
    }
    out
}

/// The frame's lines after its header: one per guardrail, or the sentence
/// that none applies, then the caller's own held guardrails by key.
#[must_use]
pub fn lines(set: &GuardrailSet) -> Vec<String> {
    let mut lines = Vec::new();
    if set.guardrails.is_empty() {
        lines.push("No guardrail applies here.".to_owned());
    } else {
        lines.extend(set.guardrails.iter().map(line));
    }
    if !set.own_held.is_empty() {
        let named: Vec<String> = set
            .own_held
            .iter()
            .map(|h| format!("{} v{}", one_line(&h.claim_key), h.version))
            .collect();
        lines.push(format!(
            "Held, not in force (this key's own, awaiting review, never served as a rule): {}",
            named.join(", ")
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use oml_guardrail::{FrameOptions, render_frame};
    use serde_json::{Value, json};

    use super::*;

    fn guardrail(i: usize, extra: Value) -> Value {
        let mut g = json!({
            "artifact_id": format!("a{i}"),
            "claim_key": format!("rule  {i}"),
            "version": i,
            "workarea_id": "w",
            "workarea_path": "universe.alice",
            "depth": 1,
            "severity": "must",
            "text": format!("Run\tthe tests  before commit {i}."),
            "rule": null,
            "tokens": 9,
            "promoted_at": "2026-11-09T10:11:12Z",
            "verdicts": [{"held_by": "human", "at": "x"}, {"held_by": "agent", "at": "y"}],
            "conflict": [],
        });
        for (k, v) in extra.as_object().unwrap() {
            g[k] = v.clone();
        }
        g
    }

    fn set(guardrails: Vec<Value>, own_held: Value) -> Value {
        json!({
            "scope_node": "n",
            "scope_path": "universe.alice",
            "guardrail_scope": "ancestors",
            "set_digest": "d",
            "over_cap": guardrails.len() > 12,
            "guardrails": guardrails,
            "own_held": own_held,
        })
    }

    #[test]
    fn the_view_lists_the_lines_of_the_frame_oml_guardrail_renders() {
        let long = "x".repeat(300);
        let cases = [
            set(vec![], json!([])),
            set(vec![guardrail(1, json!({}))], json!([])),
            set(
                vec![
                    guardrail(1, json!({"severity": "never", "conflict": ["a7", "a8"]})),
                    guardrail(
                        2,
                        json!({"workarea_path": null, "verdicts": [{"held_by": "agent", "at": "z"}]}),
                    ),
                    guardrail(3, json!({"claim_key": long, "promoted_at": "2026"})),
                    guardrail(
                        4,
                        json!({"rule": {"effect": "deny", "tool": "bash", "command": "git push"}}),
                    ),
                ],
                json!([{"claim_key": "mine  one", "version": 2}, {"claim_key": "two", "version": 1}]),
            ),
            set(
                (0..14).map(|i| guardrail(i, json!({}))).collect(),
                json!([]),
            ),
        ];
        for case in cases {
            let theirs = render_frame(
                &case,
                &FrameOptions {
                    origin: "this server",
                    checked: false,
                    whole_over_cap: true,
                },
            )
            .unwrap();
            let theirs: Vec<&str> = theirs.text.lines().skip(1).collect();
            let set: GuardrailSet = serde_json::from_value(case).unwrap();
            assert_eq!(lines(&set), theirs);
        }
    }

    #[test]
    fn the_provenance_cap_is_oml_guardrails() {
        assert_eq!(PROVENANCE_MAX_CHARS, oml_guardrail::PROVENANCE_MAX_CHARS);
    }
}
