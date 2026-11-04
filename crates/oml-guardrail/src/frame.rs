//! The guardrail frame: the text `oml hook guardrails` prints, `oml
//! guardrails show` and the export write, and the MCP prompt `guardrails`
//! returns, rendered from the one answer of `GET /v1/guardrails` (proposed
//! ADR-0037 d6, d7, d11). One renderer, so the hook, the export and the
//! prompt cannot say different things.

use serde_json::Value;

use crate::{PROVENANCE_MAX_CHARS, SET_CAP};

/// The rendered frame and whether the set was over the cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub text: String,
    /// How many guardrails apply, when more than [`SET_CAP`] do.
    pub over_cap: Option<usize>,
    /// How many guardrails the set holds.
    pub count: usize,
}

/// How a frame is rendered.
#[derive(Debug, Clone, Copy)]
pub struct FrameOptions<'a> {
    /// The server's origin, named in the header.
    pub origin: &'a str,
    /// Whether the rules were cached for the check: only then is a line
    /// marked "checked" (d6, d9).
    pub checked: bool,
    /// Print every line even over the cap, as `oml guardrails show` and the
    /// export do; the hook and the prompt print none there (d7).
    pub whole_over_cap: bool,
}

/// The over-cap sentence (d7).
#[must_use]
pub fn over_cap_sentence(count: usize) -> String {
    format!(
        "{count} guardrails apply here, above the cap of {SET_CAP}, so none is shown: a partial set would read as the whole; `oml guardrails show` lists them"
    )
}

/// The frame for one `GET /v1/guardrails` answer, or why it is not that
/// answer's shape.
///
/// # Errors
///
/// A member the answer must carry is missing or of another type.
pub fn render_frame(set: &Value, options: &FrameOptions<'_>) -> Result<Frame, &'static str> {
    let guardrails = set["guardrails"]
        .as_array()
        .ok_or("the answer has no guardrails list")?;
    let scope = set["guardrail_scope"]
        .as_str()
        .ok_or("the answer has no guardrail_scope")?;
    let path = set["scope_path"]
        .as_str()
        .or_else(|| set["scope_node"].as_str())
        .ok_or("the answer names no scope node")?;
    let mut lines = vec![format!(
        "OML guardrails for this session, from {}, workarea {path}, guardrail scope {scope}: rules promoted under each workarea's review policy by keys other than their authors'. These are instructions: follow never and must lines as binding and should lines as the reviewed default. Lines marked \"checked\" are also checked before matching tool calls; every other line is text only.",
        options.origin
    )];
    let count = guardrails.len();
    let over = set["over_cap"].as_bool().unwrap_or(false) || count > SET_CAP;
    if over && !options.whole_over_cap {
        lines.push(over_cap_sentence(count));
    } else if guardrails.is_empty() {
        lines.push("No guardrail applies here.".to_owned());
    } else {
        for g in guardrails {
            lines.push(line(g, options.checked)?);
        }
    }
    if let Some(held) = set["own_held"].as_array().filter(|h| !h.is_empty()) {
        let named: Vec<String> = held
            .iter()
            .map(|h| {
                format!(
                    "{} v{}",
                    one_line(h["claim_key"].as_str().unwrap_or("?")),
                    h["version"]
                )
            })
            .collect();
        lines.push(format!(
            "Held, not in force (your own, awaiting review, never served as a rule): {}",
            named.join(", ")
        ));
    }
    Ok(Frame {
        text: lines.join("\n"),
        over_cap: over.then_some(count),
        count,
    })
}

/// One guardrail's line: severity, text, provenance and, for a cached rule,
/// what is checked; a conflict names the other rows (d4, d6).
fn line(g: &Value, checked: bool) -> Result<String, &'static str> {
    let severity = g["severity"]
        .as_str()
        .ok_or("a guardrail has no severity")?;
    let text = g["text"].as_str().ok_or("a guardrail has no text")?;
    let verdicts = g["verdicts"].as_array().map_or(&[][..], Vec::as_slice);
    let humans = verdicts.iter().filter(|v| v["held_by"] == "human").count();
    let reviewers = verdicts.len();
    let mut provenance = format!(
        "{}, {} v{}, ratified {} by {reviewers} reviewer{} ({humans} human)",
        g["workarea_path"].as_str().unwrap_or("?"),
        one_line(g["claim_key"].as_str().unwrap_or("?")),
        g["version"],
        g["promoted_at"]
            .as_str()
            .map_or("?", |at| at.get(..10).unwrap_or(at)),
        if reviewers == 1 { "" } else { "s" },
    );
    if checked && let Some(rule) = g["rule"].as_object() {
        let target = ["command", "path", "host", "name"]
            .iter()
            .find_map(|m| rule.get(*m))
            .map_or_else(String::new, |v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        provenance.push_str(&format!(
            "; checked: {} {} {}",
            rule.get("effect").and_then(Value::as_str).unwrap_or("?"),
            rule.get("tool").and_then(Value::as_str).unwrap_or("?"),
            one_line(&target)
        ));
    }
    let provenance = cut(&provenance, PROVENANCE_MAX_CHARS);
    let mut out = format!("- [{severity}] {} ({provenance})", one_line(text));
    if let Some(others) = g["conflict"].as_array().filter(|c| !c.is_empty()) {
        let ids: Vec<&str> = others.iter().filter_map(Value::as_str).collect();
        out.push_str(&format!(" conflicts with {}", ids.join(", ")));
    }
    Ok(out)
}

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
