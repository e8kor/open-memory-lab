//! RFC 8785 (JCS) canonical JSON for the manifest's signed bytes (ADR-0084
//! d5) and the source events' signed bytes (chain-format.md).
//!
//! The same procedure as the chain's, written again here because this leaf
//! links no domain crate (the CLI links none, ADR-0006 d6), as
//! `oml audit verify-chain` writes it again. Integers below 2^53 only, no
//! floats: nothing a bundle signs holds another number.

use serde_json::Value;

const MAX_INTEGER: u64 = (1 << 53) - 1;
const MIN_INTEGER: i64 = -((1_i64 << 53) - 1);

/// The canonical bytes of `value`, or why it has none.
pub fn canonical_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    write_value(value, &mut out)?;
    Ok(out)
}

fn write_value(value: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                if u > MAX_INTEGER {
                    return Err(format!("integer {u} is at or above 2^53"));
                }
                out.extend_from_slice(u.to_string().as_bytes());
            } else if let Some(i) = n.as_i64() {
                if i < MIN_INTEGER {
                    return Err(format!("integer {i} is at or below -2^53"));
                }
                out.extend_from_slice(i.to_string().as_bytes());
            } else {
                return Err("a non-integer number cannot be signed".to_owned());
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
