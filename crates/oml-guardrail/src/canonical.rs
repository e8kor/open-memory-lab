//! The cache line's canonical form: RFC 8785 for the shapes a cache holds,
//! objects, arrays, strings and `null`, with members sorted by UTF-16 code
//! units and the RFC's minimal string escaping (proposed ADR-0037 d9). A
//! number, which no cached rule carries, has no form here and is `None`.
//! `oml-domain`'s test holds this equal to its `jcs::canonical_bytes`.

use serde_json::Value;

/// The canonical JSON text of `value`, or `None` when it holds a number or
/// a boolean.
pub(crate) fn canonical_line(value: &Value) -> Option<String> {
    let mut out = String::new();
    write(value, &mut out)?;
    Some(out)
}

fn write(value: &Value, out: &mut String) -> Option<()> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(_) | Value::Number(_) => return None,
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write(&map[key.as_str()], out)?;
            }
            out.push('}');
        }
    }
    Some(())
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
