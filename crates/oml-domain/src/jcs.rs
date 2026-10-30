//! RFC 8785 (JCS) canonicalization for the value shapes the chain allows.
//!
//! chain-format.md forbids floats outright ("No float ever appears in an
//! envelope or payload") and bounds every number below 2^53, so ECMAScript
//! float serialization is deliberately out of scope here: a non-integer or
//! out-of-range number is an error, never a formatting question. Object
//! members sort by UTF-16 code units per RFC 8785 §3.2.3; strings use the
//! RFC's minimal escaping with lowercase `\u00xx` for other control
//! characters.

use serde_json::Value;

/// Largest integer magnitude the chain admits: numbers must stay strictly
/// below 2^53 (chain-format.md, "Numbers are integers below 2^53").
pub const MAX_CHAIN_INTEGER: u64 = (1 << 53) - 1;
const MIN_CHAIN_INTEGER: i64 = -((1_i64 << 53) - 1);

/// A value that cannot be canonicalized under the chain's number rules.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum JcsError {
    /// A JSON number that is not an integer (chain-format.md: no floats).
    #[error("non-integer JSON number cannot appear in signed bytes (chain-format.md number rule)")]
    NonInteger,
    /// An integer at or above 2^53 in magnitude would canonicalize lossily.
    #[error("integer {0} is outside the chain's sub-2^53 bound (chain-format.md number rule)")]
    IntegerOutOfRange(String),
}

/// Serializes `value` as RFC 8785 canonical JSON bytes.
pub fn canonical_bytes(value: &Value) -> Result<Vec<u8>, JcsError> {
    let mut out = Vec::new();
    write_value(value, &mut out)?;
    Ok(out)
}

fn write_value(value: &Value, out: &mut Vec<u8>) -> Result<(), JcsError> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                if u > MAX_CHAIN_INTEGER {
                    return Err(JcsError::IntegerOutOfRange(u.to_string()));
                }
                out.extend_from_slice(u.to_string().as_bytes());
            } else if let Some(i) = n.as_i64() {
                if i < MIN_CHAIN_INTEGER {
                    return Err(JcsError::IntegerOutOfRange(i.to_string()));
                }
                out.extend_from_slice(i.to_string().as_bytes());
            } else {
                return Err(JcsError::NonInteger);
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
            // RFC 8785 §3.2.3: sort property names by UTF-16 code units.
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
