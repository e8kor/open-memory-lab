//! The independent chain verifier behind `oml audit verify-chain`.
//!
//! This module re-implements the verification procedure of
//! docs/data/chain-format.md (steps 1–9) from that PUBLIC page alone —
//! RFC 8785 canonicalization, SHA-256 and pure Ed25519 through sha2 and
//! ed25519-dalek directly, never through oml-domain. That is deliberate
//! (ADR-0006 d6: the CLI composes no domain crate): an auditor's verifier
//! that linked the server's own chain code could inherit the very bug it is
//! supposed to catch.

use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// `sha256:` + 32 zero bytes — the genesis `prev_hash` (chain-format.md).
pub const ZERO_PREV_HASH: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

// ---------------------------------------------------------------------------
// RFC 8785 (JCS) canonicalization — chain-format.md "signed_bytes"
// ---------------------------------------------------------------------------

/// Serializes `value` as RFC 8785 canonical JSON. The chain admits only
/// integers strictly below 2^53 in magnitude and no floats (chain-format.md
/// number rule), so any other number is a verification failure here, not a
/// formatting question.
pub fn canonical_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    write_value(value, &mut out)?;
    Ok(out)
}

const MAX_CHAIN_INTEGER: u64 = (1 << 53) - 1;
const MIN_CHAIN_INTEGER: i64 = -((1_i64 << 53) - 1);

fn write_value(value: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                if u > MAX_CHAIN_INTEGER {
                    return Err(format!("integer {u} is at or above 2^53"));
                }
                out.extend_from_slice(u.to_string().as_bytes());
            } else if let Some(i) = n.as_i64() {
                if i < MIN_CHAIN_INTEGER {
                    return Err(format!("integer {i} is at or below -2^53"));
                }
                out.extend_from_slice(i.to_string().as_bytes());
            } else {
                return Err(format!(
                    "non-integer number {n} cannot appear in signed bytes"
                ));
            }
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_value(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            // RFC 8785 §3.2.3: property names sort by UTF-16 code units.
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push(b'{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(key, out);
                out.push(b':');
                write_value(&map[key.as_str()], out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

/// RFC 8785 string form: minimal escaping, lowercase `\u00xx` for the
/// remaining control characters, raw UTF-8 otherwise.
fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{0008}' => out.extend_from_slice(b"\\b"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\u{000c}' => out.extend_from_slice(b"\\f"),
            '\r' => out.extend_from_slice(b"\\r"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                let mut buf = [0_u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

// ---------------------------------------------------------------------------
// The served event, keys and fingerprints
// ---------------------------------------------------------------------------

/// One item of `GET /v2/events` as the lab serves it. The tail serves parsed
/// envelope fields, not the stored `signed_bytes` column, so the verifier
/// reconstructs the canonical bytes from these fields plus the tenant id.
#[derive(Debug, Clone)]
pub struct TailEvent {
    pub seq: u64,
    pub event_id: String,
    pub event_type: String,
    pub prev_hash: String,
    pub hash: String,
    pub sig: String,
    pub principal: String,
    pub api_key_id: Option<String>,
    pub at: String,
    pub payload: Value,
}

impl TailEvent {
    /// Parses one served item; a missing or mistyped field is named.
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let field = |name: &str| -> Result<String, String> {
            v[name]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("event field {name:?} is missing or not a string: {v}"))
        };
        Ok(Self {
            seq: v["seq"]
                .as_u64()
                .ok_or_else(|| format!("event field \"seq\" is missing or not an integer: {v}"))?,
            event_id: field("event_id")?,
            event_type: field("event_type")?,
            prev_hash: field("prev_hash")?,
            hash: field("hash")?,
            sig: field("sig")?,
            principal: field("principal")?,
            api_key_id: match &v["api_key_id"] {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                other => return Err(format!("api_key_id is neither string nor null: {other}")),
            },
            at: field("at")?,
            payload: v
                .get("payload")
                .cloned()
                .ok_or_else(|| format!("event field \"payload\" is missing: {v}"))?,
        })
    }

    /// The nine signed fields (chain-format.md "The signed fields") as one
    /// JSON object; `hash` and `sig` are derived and therefore excluded.
    fn signed_object(&self, tenant_id: &str) -> Value {
        json!({
            "tenant_id": tenant_id,
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
}

/// A signing key's id IS its fingerprint: the lowercase hex SHA-256 of the
/// 32 raw public key bytes (chain-format.md "The signing key").
#[must_use]
pub fn fingerprint(key: &VerifyingKey) -> String {
    hex::encode(Sha256::digest(key.to_bytes()))
}

/// Parses a raw Ed25519 public key given as 64 lowercase hex characters.
pub fn parse_verifying_key(s: &str) -> Result<VerifyingKey, String> {
    let bytes = hex::decode(s.trim())
        .map_err(|_| format!("{s:?} is not hex (expected the 32 raw public key bytes as hex)"))?;
    let raw: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "a raw ed25519 public key is exactly 32 bytes".to_owned())?;
    VerifyingKey::from_bytes(&raw).map_err(|e| format!("not a valid ed25519 public key: {e}"))
}

fn parse_sig(s: &str) -> Result<Signature, String> {
    // The tail serves the signature as bare hex; chain-format.md displays it
    // as `ed25519:` + hex. Both spell the same 64 bytes.
    let hex_part = s.strip_prefix("ed25519:").unwrap_or(s);
    let bytes =
        hex::decode(hex_part).map_err(|_| format!("sig is not hex: {}", truncate(s, 24)))?;
    let raw: [u8; 64] = bytes
        .try_into()
        .map_err(|_| "an ed25519 signature is exactly 64 bytes".to_owned())?;
    Ok(Signature::from_bytes(&raw))
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

// ---------------------------------------------------------------------------
// The verification procedure (chain-format.md "Verification", steps 1–9)
// ---------------------------------------------------------------------------

/// What the run found: breaks are verification failures (any one fails the
/// chain), notes are the honest statements about what this transport can and
/// cannot check.
#[derive(Debug)]
pub struct ChainReport {
    pub tenant_id: String,
    pub events: usize,
    pub head_seq: u64,
    /// Recomputed SHA-256 of the last event's canonical bytes, lowercase hex.
    pub head_hash: String,
    pub breaks: Vec<String>,
    pub notes: Vec<String>,
    /// Per key: (fingerprint, events its signature verified).
    pub key_events: Vec<(String, usize)>,
}

impl ChainReport {
    #[must_use]
    pub fn verified(&self) -> bool {
        self.breaks.is_empty()
    }
}

/// Runs the chain-format.md procedure over one tenant's events, seq
/// ascending, against the authenticated key set. The key set is the
/// caller's input by design: an embedded key authenticates nothing by
/// itself — the operator confirms a fingerprint out of band
/// (chain-format.md "How the public key reaches an export recipient").
#[must_use]
pub fn verify_chain(
    tenant_id: &str,
    events: &[TailEvent],
    stated_head_seq: u64,
    keys: &[VerifyingKey],
) -> ChainReport {
    let mut breaks = Vec::new();
    let mut notes = Vec::new();
    let mut key_events = vec![0_usize; keys.len()];
    let mut prev_computed_hash: Option<String> = None;
    let mut last_hash_hex = String::new();

    if events.is_empty() {
        breaks.push("the tail served no events; a tenant chain opens at seq 1".to_owned());
    }

    for (i, event) in events.iter().enumerate() {
        let want_seq = i as u64 + 1;
        let label = format!("seq {}", event.seq);

        // Step 1: seq starts at 1 and increments by exactly 1 — a gap is a
        // verification failure, which is what makes omission detectable.
        if event.seq != want_seq {
            breaks.push(format!(
                "step 1: expected seq {want_seq}, served seq {} — gap or duplicate",
                event.seq
            ));
        }

        // Step 2: genesis shape.
        if i == 0 {
            if event.event_type != "tenant.genesis" {
                breaks.push(format!(
                    "step 2: event 1 is {:?}, not tenant.genesis",
                    event.event_type
                ));
            }
            if event.prev_hash != ZERO_PREV_HASH {
                breaks.push(format!(
                    "step 2: genesis prev_hash is {}, not 32 zero bytes",
                    event.prev_hash
                ));
            }
        }

        // Steps 6–7 collapse over this transport: the tail serves the parsed
        // envelope fields, not the stored signed_bytes column, so the
        // verifier reconstructs the canonical form from the served fields.
        // A writer that drifted from RFC 8785 canonical form therefore fails
        // step 3/4 here instead of a byte-compare.
        let signed_bytes = match canonical_bytes(&event.signed_object(tenant_id)) {
            Ok(bytes) => bytes,
            Err(e) => {
                breaks.push(format!("{label}: cannot canonicalize signed fields: {e}"));
                continue;
            }
        };

        // Step 4: hash = SHA-256(signed_bytes).
        let computed = hex::encode(Sha256::digest(&signed_bytes));
        let computed_prefixed = format!("sha256:{computed}");
        if event.hash != computed_prefixed {
            breaks.push(format!(
                "step 4: {label}: served hash {} does not match recomputed {computed_prefixed}",
                event.hash
            ));
        }

        // Step 3: sig verifies under some key in the authenticated set —
        // per-event key attribution is by trial verification.
        match parse_sig(&event.sig) {
            Err(e) => breaks.push(format!("step 3: {label}: {e}")),
            Ok(sig) => {
                let verified_by = keys
                    .iter()
                    .position(|key| key.verify(&signed_bytes, &sig).is_ok());
                match verified_by {
                    Some(k) => key_events[k] += 1,
                    None => breaks.push(format!(
                        "step 3: {label}: signature verifies under no key in the authenticated set"
                    )),
                }
            }
        }

        // Step 5: prev_hash equals the previous event's RECOMPUTED hash.
        if let Some(prev) = &prev_computed_hash
            && event.prev_hash != *prev
        {
            breaks.push(format!(
                "step 5: {label}: prev_hash {} does not name the previous event's recomputed hash {prev}",
                event.prev_hash
            ));
        }
        prev_computed_hash = Some(computed_prefixed);
        last_hash_hex = computed;

        // Step 8: the genesis payload's provider-chain reference.
        if i == 0 {
            if event.payload.get("created_event_id").is_some() {
                notes.push(
                    "step 8: the genesis names a provider-chain created_event_id; the events \
                     tail serves one tenant's chain, so the cross-chain reference is not \
                     checkable over this surface"
                        .to_owned(),
                );
            } else {
                notes.push(
                    "step 8 skipped: the standalone PoC deployment has no provider chain and \
                     its genesis payload carries no created_event_id"
                        .to_owned(),
                );
            }
        }
    }

    // Step 9: the stated chain head is the last event. The tail states the
    // head as head_seq (no head-hash field), and the head's signature was
    // verified in step 3 like every other event's.
    if let Some(last) = events.last()
        && stated_head_seq != last.seq
    {
        breaks.push(format!(
            "step 9: the server states head seq {stated_head_seq} but served events end at seq {}",
            last.seq
        ));
    }
    notes.push(
        "steps 6–7: served fields are the only input over GET /v2/events, so canonical form \
         is reconstructed rather than byte-compared against stored signed_bytes"
            .to_owned(),
    );

    ChainReport {
        tenant_id: tenant_id.to_owned(),
        events: events.len(),
        head_seq: stated_head_seq,
        head_hash: last_hash_hex,
        breaks,
        notes,
        key_events: keys
            .iter()
            .zip(key_events)
            .map(|(key, n)| (fingerprint(key), n))
            .collect(),
    }
}
