//! One dispatcher for every harness hook event (ADR-0063 d6).
//!
//! `oml hook <event>` reads the event's JSON from stdin once, runs every
//! handler registered for that event in [`HANDLERS`] order, and merges what
//! they answer into the one document the harness reads: the first block
//! decision wins, and every context is joined under one
//! `additionalContext`. Any harness with command hooks calls the same verb
//! with the same JSON, so the Claude Code plugin's `hooks.json` is a thin
//! wrapper with no logic of its own. A new handler is one row of
//! [`HANDLERS`]: v0.6.0 registers the chat handlers; repository detection at
//! session start, anchor filling before `oml_write`, the paths an edit
//! touched, and a stale-claim check at stop are planned rows, not
//! special cases.

use std::io::Read;

use serde_json::{Map, Value, json};

use crate::hook::{Quiet, read_object};

/// The harness events the dispatcher serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    UserPromptSubmit,
    Stop,
    PreCompact,
}

impl Event {
    /// The event's name as the harness spells it in `hookEventName`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::Stop => "Stop",
            Self::PreCompact => "PreCompact",
        }
    }
}

/// What one handler answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Nothing for the harness to read.
    Nothing,
    /// Text added to the agent's context.
    Context(String),
    /// Keep the agent going (`Stop`) with this reason as its instruction.
    Block(String),
}

/// A handler: the environment and the event's JSON object, once parsed.
/// `Err(None)` is "nothing to do"; `Err(Some(why))` a reason for stderr.
pub type Handler = fn(&dyn Fn(&str) -> Option<String>, &Map<String, Value>) -> Result<Reply, Quiet>;

/// One registered handler: its event, a name for its stderr lines, and
/// the function.
pub struct Registered {
    pub event: Event,
    pub name: &'static str,
    pub handler: Handler,
}

/// Every handler, in the order each event runs them.
pub const HANDLERS: &[Registered] = &[
    Registered {
        event: Event::UserPromptSubmit,
        name: "chat",
        handler: crate::chat::on_user_prompt,
    },
    Registered {
        event: Event::Stop,
        name: "chat",
        handler: crate::chat::on_stop,
    },
    Registered {
        event: Event::PreCompact,
        name: "chat",
        handler: crate::chat::on_pre_compact,
    },
];

/// Runs `event`'s handlers over stdin's JSON object and renders the merged
/// answer, or `Err` when none answered: the first handler's reason, if any
/// gave one.
///
/// # Errors
///
/// `Err(None)` when nothing is printed and nothing is wrong; `Err(Some(why))`
/// for a stdin that is not a JSON object, or the first handler's reason.
pub fn dispatch(
    event: Event,
    var: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
) -> Result<String, Quiet> {
    let object = read_object(stdin).map_err(|why| {
        Some(format!(
            "stdin is not the {} JSON object ({why})",
            event.name()
        ))
    })?;
    let mut contexts: Vec<String> = Vec::new();
    let mut block: Option<String> = None;
    let mut reason: Quiet = None;
    for registered in HANDLERS.iter().filter(|r| r.event == event) {
        match (registered.handler)(var, &object) {
            Ok(Reply::Nothing) | Err(None) => {}
            Ok(Reply::Context(text)) => contexts.push(text),
            Ok(Reply::Block(why)) => {
                block.get_or_insert(why);
            }
            Err(Some(why)) => {
                reason.get_or_insert(why);
            }
        }
    }
    if let Some(why) = block {
        return Ok(json!({ "decision": "block", "reason": why }).to_string());
    }
    if contexts.is_empty() {
        return Err(reason);
    }
    Ok(json!({
        "hookSpecificOutput": {
            "hookEventName": event.name(),
            "additionalContext": contexts.join("\n\n"),
        }
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_has_a_handler_and_a_stdin_that_is_not_an_object_is_named() {
        for event in [Event::UserPromptSubmit, Event::Stop, Event::PreCompact] {
            assert!(HANDLERS.iter().any(|r| r.event == event), "{event:?}");
            let var = |_: &str| None;
            let err = dispatch(event, &var, &mut &b"[1]"[..]).expect_err("not an object");
            assert!(err.expect("named").contains(event.name()));
        }
    }
}
