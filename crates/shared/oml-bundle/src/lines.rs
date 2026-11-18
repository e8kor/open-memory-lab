//! The JSON Lines records of a bundle (d1–d4, d13). Every object is closed:
//! an unknown member is `bundle.malformed` (reading R5, R6).

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::Sha256;

/// One exported workspace (`workspaces.jsonl`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLine {
    pub name: String,
    pub path: String,
    pub status: String,
    /// The parent's path; null for an exported root.
    pub parent: Option<String>,
}

/// A source key as provenance (d4): never its id or name, which stay on the
/// source, but its kind of user and a pseudonym valid inside this bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Author {
    pub used_by: String,
    pub same_user: bool,
    pub pseudonym: String,
}

/// A version's state on the source when it was exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Held,
    Promoted,
    Demoted,
    Rejected,
    Withdrawn,
}

/// An attachment, carried under `bodies/` by its plaintext digest (d3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub digest: String,
    pub media_type: String,
    pub size_bytes: u64,
}

/// One claim version (`versions.jsonl`, d2, d3). Its text is inline up to
/// 64 KiB (`text`) or a body under `bodies/` (`body`, its digest), never both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionLine {
    pub artifact_id: String,
    pub version: u64,
    pub claim_key: String,
    /// The source workspace's path.
    pub workspace: String,
    pub kind: String,
    pub format: Option<String>,
    pub title: Option<String>,
    pub state: State,
    pub retirement: bool,
    /// The version this one supersedes, if any.
    pub replaces: Option<u64>,
    /// A simulation row keeps its mark (d2, ADR-0011 d8).
    pub simulation_id: Option<String>,
    pub text: Option<String>,
    pub body: Option<String>,
    pub plaintext_digest: String,
    /// ADR-0057 d1's anchor, as signed, or null.
    pub anchor: Value,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    /// The kind's own object, as signed (ADR-0080 d1, ADR-0081 d1, ADR-0086 d1).
    pub skill: Option<Value>,
    pub document: Option<Value>,
    pub test: Option<Value>,
    /// A guardrail's `{severity, rule}` (ADR-0037 d1, d2): required on a
    /// guardrail version that is not a retirement, absent on every other
    /// (ADR-0084 reading I20).
    #[serde(default)]
    pub guardrail: Option<Value>,
    pub derives_from: Vec<String>,
    /// ADR-0063 d1's origin, verbatim as signed.
    pub origin: Value,
    pub attachments: Vec<Attachment>,
    pub author: Author,
    pub created_at: String,
    pub promoted_at: Option<String>,
    /// The source event's id: present only on the exporter's own versions
    /// (R1, the operator, 2026-11-17).
    pub source_event: Option<String>,
}

/// An erased version (d13): the artifact id, the version and the fact of
/// erasure, and nothing else, so a holder can confirm no guessed text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErasureMarker {
    pub artifact_id: String,
    pub version: u64,
    /// Always true.
    pub erased: bool,
}

/// A line of `versions.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionEntry {
    Version(Box<VersionLine>),
    Erased(ErasureMarker),
}

impl VersionEntry {
    /// The artifact id and version the line names.
    #[must_use]
    pub fn id(&self) -> (&str, u64) {
        match self {
            Self::Version(v) => (&v.artifact_id, v.version),
            Self::Erased(m) => (&m.artifact_id, m.version),
        }
    }

    pub(crate) fn to_value(&self) -> Value {
        match self {
            Self::Version(v) => serde_json::to_value(v),
            Self::Erased(m) => serde_json::to_value(m),
        }
        .unwrap_or(Value::Null)
    }

    /// A line that names `erased` is a marker and must hold exactly the
    /// marker's three members (reading R7); any other line is a version.
    pub(crate) fn from_value(value: Value) -> Result<Self, String> {
        let is_marker = value.as_object().is_some_and(|m| m.contains_key("erased"));
        if is_marker {
            let keys: Vec<&String> = value
                .as_object()
                .map(Map::keys)
                .into_iter()
                .flatten()
                .collect();
            if keys.len() != 3 || value.get("erased") != Some(&Value::Bool(true)) {
                return Err("erasure_marker_content".to_owned());
            }
            serde_json::from_value(value)
                .map(Self::Erased)
                .map_err(|_| "erasure_marker_content".to_owned())
        } else {
            serde_json::from_value::<VersionLine>(value)
                .map(|v| Self::Version(Box::new(v)))
                .map_err(|e| crate::archive::serde_reason(&e))
        }
    }
}

/// A verdict's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Approve,
    Reject,
}

/// One verdict on an exported version (`verdicts.jsonl`, d4). It carries no
/// source event (R1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictLine {
    pub artifact_id: String,
    pub version: u64,
    pub verdict: Verdict,
    pub reviewer: Author,
    pub approved_at: Option<String>,
    pub at: String,
}

/// Which edge (d1: claim and brief edges whose two ends are exported).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    Claim,
    Brief,
}

/// One edge (`edges.jsonl`). A claim edge joins two artifact ids under a
/// label; a brief edge joins an artifact id to a workspace path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeLine {
    pub kind: EdgeKind,
    pub src: String,
    pub dst: String,
    pub label: Option<String>,
}

/// One source journal event, as signed (`events.jsonl`, d1): the chain's
/// nine signed fields, `hash` and `sig` (chain-format.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventLine {
    pub tenant_id: String,
    pub seq: u64,
    pub event_id: String,
    pub event_type: String,
    pub prev_hash: String,
    pub principal: String,
    pub api_key_id: Option<String>,
    pub at: String,
    pub payload: Value,
    pub hash: String,
    pub sig: String,
}

impl EventLine {
    /// The nine signed fields as one object (chain-format.md).
    #[must_use]
    pub fn signed_value(&self) -> Value {
        serde_json::json!({
            "tenant_id": self.tenant_id,
            "seq": self.seq,
            "event_id": self.event_id,
            "event_type": self.event_type,
            "prev_hash": self.prev_hash,
            "principal": self.principal,
            "api_key_id": self.api_key_id,
            "at": self.at,
            "payload": self.payload,
        })
    }

    /// The RFC 8785 bytes the event's `hash` and `sig` cover.
    pub fn signed_bytes(&self) -> Result<Vec<u8>, String> {
        crate::jcs::canonical_bytes(&self.signed_value())
    }
}

/// The pseudonym of a source key inside one bundle (d4, reading R8):
/// lowercase hex HMAC-SHA-256 of the key id under the bundle's salt. The
/// caller draws the salt fresh for each bundle and never exports it.
#[must_use]
pub fn pseudonym(salt: &[u8], key_id: &str) -> String {
    // HMAC takes a key of any length; new_from_slice fails for none.
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(salt)
        .unwrap_or_else(|_| unreachable!("HMAC accepts every key length"));
    mac.update(key_id.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
