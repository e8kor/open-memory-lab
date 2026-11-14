//! The former-name map of ADR-0091 d4: one total, pure function from a
//! journal event as stored to the event as read.
//!
//! The journal is signed, so an event written before v0.8.0 keeps the names
//! it was signed with: `workarea.*` types, `workarea` in payload keys, the
//! role `dreamer`, the statuses `committed` and `validated`, and a key's
//! `held_by` valued `human`. Every reader that interprets an event (the
//! events tail and its visibility, naming, a key's detail, the lake writer's
//! projection, `oml events`) reads it through [`read_event`] and through
//! nothing else, after the stored bytes are verified as they stand; the
//! mapped event is never signed, hashed or re-serialized into the journal.
//!
//! The map is the identity on every event the v0.8.0 server writes, so
//! applying it twice changes nothing and no reader needs a cut-over date. It
//! is the one place outside the applied migrations and the catalog's
//! former-name table that spells the former names (ADR-0091 d11).

#![forbid(unsafe_code)]

use serde_json::{Map, Value};

/// The closed list of former event types and their successors (rule 1):
/// the six the event catalog listed, and every `workarea.*` type a released
/// binary named.
pub const FORMER_EVENT_TYPES: &[(&str, &str)] = &[
    ("workarea.create", "workspace.create"),
    ("workarea.mode_set", "workspace.mode_set"),
    ("workarea.federation_set", "workspace.federation_set"),
    ("workarea.status_proposed", "workspace.status_proposed"),
    ("workarea.status_transition", "workspace.status_transition"),
    ("workarea.crew_updated", "workspace.crew_updated"),
    ("workarea.reparent", "workspace.reparent"),
    ("workarea.relate_proposed", "workspace.relate_proposed"),
    ("workarea.relate_ratified", "workspace.relate_ratified"),
];

/// The prefix of a former event type: one outside [`FORMER_EVENT_TYPES`]
/// fails closed.
const FORMER_TYPE_PREFIX: &str = "workarea.";

/// The doctor check that names an unmapped former name (ADR-0091 d4).
pub const DOCTOR_CHECK: &str = "journal_unmapped_former_name";

/// An event as a reader reads it: its type and payload in the current names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadEvent {
    pub event_type: String,
    pub payload: Value,
}

/// A former name the map does not list: the reader fails closed and names
/// the event type and the name (ADR-0091 d4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappedFormerName {
    pub event_type: String,
    pub name: String,
}

impl UnmappedFormerName {
    /// The `oml doctor` check this failure is reported under.
    #[must_use]
    pub const fn doctor_check(&self) -> &'static str {
        DOCTOR_CHECK
    }
}

impl std::fmt::Display for UnmappedFormerName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{DOCTOR_CHECK}: event {} names the former name {:?}, which the former-name map does not list",
            self.event_type, self.name
        )
    }
}

impl std::error::Error for UnmappedFormerName {}

/// The successor of a former event type, or the type itself when it is
/// current.
///
/// # Errors
///
/// [`UnmappedFormerName`] for a `workarea.*` type outside the closed list.
pub fn read_event_type(event_type: &str) -> Result<String, UnmappedFormerName> {
    if let Some((_, current)) = FORMER_EVENT_TYPES.iter().find(|(f, _)| *f == event_type) {
        return Ok((*current).to_owned());
    }
    if event_type.starts_with(FORMER_TYPE_PREFIX) {
        return Err(UnmappedFormerName {
            event_type: event_type.to_owned(),
            name: event_type.to_owned(),
        });
    }
    Ok(event_type.to_owned())
}

/// A payload key as read (rule 2): the token `workarea` becomes `workspace`,
/// so `workareas` becomes `workspaces`.
#[must_use]
pub fn read_key(key: &str) -> String {
    if key.contains("workarea") {
        key.replace("workarea", "workspace")
    } else {
        key.to_owned()
    }
}

/// Maps one event as stored to the event as read (ADR-0091 d4, rules 1-6).
///
/// # Errors
///
/// [`UnmappedFormerName`] for a former event type the map does not list.
pub fn read_event(event_type: &str, payload: &Value) -> Result<ReadEvent, UnmappedFormerName> {
    let event_type = read_event_type(event_type)?;
    let mut payload = read_keys(payload);
    if let Value::Object(fields) = &mut payload {
        read_values(&event_type, fields);
    }
    read_snapshots(&event_type, &mut payload);
    Ok(ReadEvent {
        event_type,
        payload,
    })
}

/// Rule 2, at every depth.
fn read_keys(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(k, v)| (read_key(k), read_keys(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(read_keys).collect()),
        other => other.clone(),
    }
}

fn replace_str(value: &mut Value, from: &str, to: &str) {
    if value.as_str() == Some(from) {
        *value = Value::String(to.to_owned());
    }
}

fn read_status(value: &mut Value) {
    replace_str(value, "committed", "protected");
    replace_str(value, "validated", "frozen");
}

/// Rules 3 to 6 on the top-level fields of the listed event types.
fn read_values(event_type: &str, fields: &mut Map<String, Value>) {
    // Rule 3: the guardrail scope of any event that records it.
    if let Some(scope) = fields.get_mut("guardrail_scope") {
        replace_str(scope, "workarea", "workspace");
    }
    match event_type {
        "system.names_assigned" => {
            if let Some(Value::Array(names)) = fields.get_mut("names") {
                for row in names {
                    if let Some(kind) = row.get_mut("kind") {
                        replace_str(kind, "workarea", "workspace");
                    }
                }
            }
        }
        "key.issued" => {
            if let Some(role) = fields.get_mut("role") {
                replace_str(role, "dreamer", "reviewer");
            }
            // Rule 6: the one event that records a key's holder.
            if let Some(mut held_by) = fields.remove("held_by") {
                replace_str(&mut held_by, "human", "person");
                fields.insert("used_by".to_owned(), held_by);
            }
        }
        "grant.issued" => {
            if let Some(role) = fields.get_mut("role") {
                replace_str(role, "dreamer", "reviewer");
            }
        }
        "workspace.create" => {
            if let Some(status) = fields.get_mut("status") {
                read_status(status);
            }
        }
        "workspace.status_proposed" | "workspace.status_transition" => {
            for field in ["from", "to"] {
                if let Some(status) = fields.get_mut(field) {
                    read_status(status);
                }
            }
        }
        _ => {}
    }
}

/// Rule 4 inside every `policy_snapshot`, and rule 5's snapshot key in a
/// status transition.
fn read_snapshots(event_type: &str, value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, inner) in fields.iter_mut() {
                if key == "policy_snapshot"
                    && let Value::Object(snapshot) = inner
                {
                    if let Some(Value::Array(roles)) = snapshot.get_mut("required_roles") {
                        for role in roles {
                            replace_str(role, "dreamer", "reviewer");
                        }
                    }
                    if event_type == "workspace.status_transition"
                        && let Some(all) = snapshot.remove("children_all_validated")
                    {
                        snapshot.insert("children_all_frozen".to_owned(), all);
                    }
                }
                read_snapshots(event_type, inner);
            }
        }
        Value::Array(items) => {
            for item in items {
                read_snapshots(event_type, item);
            }
        }
        _ => {}
    }
}
