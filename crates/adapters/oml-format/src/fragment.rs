//! The scalar tree JSON and YAML share: search text, scan views, the keyed
//! secret rule and the structural diff (ADR-0056 d6).

use std::collections::BTreeMap;

use oml_app::ports::{FormatChange, FormatDiff, Parsed, ScanView};

/// One scalar with the path of keys and indexes that leads to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub path: Vec<String>,
    /// The key that names it, decoded, when its parent is a mapping.
    pub value: String,
}

const SECRET_KEYS: [&str; 10] = [
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "private_key",
    "client_secret",
    "authorization",
    "credentials",
];

fn is_secret_key(segment: &str) -> bool {
    let k = segment.to_lowercase();
    SECRET_KEYS.contains(&k.as_str())
        || k.ends_with("_secret")
        || k.ends_with("_password")
        || k.ends_with("_passwd")
        || k.ends_with("_token")
        || k.ends_with("_key")
}

fn is_placeholder(value: &str) -> bool {
    let v = value.trim();
    let lower = v.to_lowercase();
    v.is_empty()
        || lower == "null"
        || (v.starts_with('<') && v.ends_with('>'))
        || (v.starts_with("${") && v.ends_with('}'))
        || (v.starts_with("{{") && v.ends_with("}}"))
        || v.chars().all(|c| c == '*')
        || matches!(lower.as_str(), "changeme" | "example" | "redacted")
}

/// Whether any segment of the path names a secret, so a value nested in an
/// array or object under `password` is as keyed as one directly under it.
fn keyed_secret(entry: &Entry) -> bool {
    entry.path.iter().any(|s| is_secret_key(s))
}

/// The search text, the scan views and the keyed-secret finding of a
/// document's scalars, in document order.
#[must_use]
pub(crate) fn parsed(entries: &[Entry], keys: &[Entry]) -> Parsed {
    let mut lines = Vec::new();
    let mut views = Vec::new();
    let mut veto = None;
    for entry in entries {
        let dotted = entry.path.join(".");
        if keyed_secret(entry) {
            if !is_placeholder(&entry.value) && veto.is_none() {
                veto = Some(format!("secret.keyed_value at {dotted}"));
            }
        } else {
            lines.push(format!("{} is {}", entry.path.join(" "), entry.value));
        }
        views.push(ScanView {
            path: dotted,
            text: entry.value.clone(),
        });
    }
    for key in keys {
        views.push(ScanView {
            path: key.path.join("."),
            text: key.value.clone(),
        });
    }
    Parsed {
        search_text: lines.join("\n"),
        views,
        veto,
    }
}

/// The structural diff of two documents: equal when every path holds the
/// same scalar, whatever the order of keys or the layout of the source.
#[must_use]
pub(crate) fn diff(old: &[Entry], new: &[Entry]) -> FormatDiff {
    let map = |entries: &[Entry]| -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|e| (e.path.join("."), e.value.clone()))
            .collect()
    };
    let (a, b) = (map(old), map(new));
    let mut changes = Vec::new();
    for (path, before) in &a {
        match b.get(path) {
            None => changes.push(FormatChange {
                op: "remove",
                path: path.clone(),
                before: Some(before.clone()),
                after: None,
            }),
            Some(after) if after != before => changes.push(FormatChange {
                op: "replace",
                path: path.clone(),
                before: Some(before.clone()),
                after: Some(after.clone()),
            }),
            Some(_) => {}
        }
    }
    for (path, after) in &b {
        if !a.contains_key(path) {
            changes.push(FormatChange {
                op: "add",
                path: path.clone(),
                before: None,
                after: Some(after.clone()),
            });
        }
    }
    FormatDiff {
        equivalent: changes.is_empty(),
        changes,
    }
}
