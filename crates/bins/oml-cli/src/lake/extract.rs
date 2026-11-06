//! A table's claims (ADR-0062 d6, d7): one table claim, one schema claim, and
//! a column claim only where a comment states what the column means. A claim
//! carries what the log's `protocol`, `metaData` and clustering domain say and
//! nothing else: no file, no partition value, no statistic, no user.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use oml_delta_log::{Field, Snapshot};
use serde::Serialize;
use serde_json::Value;

use crate::sync::adapter::fit_key;
use crate::sync::hash::{md_normal, nfc, sha256};

/// `delta.columnMapping.physicalName`, the key a column's claim is under.
const PHYSICAL: &str = "delta.columnMapping.physicalName";
/// The longest value of an `oml.` property the table claim states (ADR-0062 d9).
const PROPERTY_MAX: usize = 500;
/// A schema of more columns than this is split (ADR-0056 d6's caps, ADR-0062 d6).
const SCHEMA_COLUMNS_MAX: usize = 666;
const SCHEMA_PART: usize = 500;

pub const TABLE_HASHER: &str = "oml-delta-table/1";
pub const SCHEMA_HASHER: &str = "oml-delta-schema/1";
pub const COL_HASHER: &str = "oml-delta-col/1";

/// One claim a table states.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LakeItem {
    pub claim_key: String,
    /// `col=<path>` for a column claim.
    pub fragment: Option<String>,
    /// The claim's text, NFC.
    pub text: String,
    /// `sha256:` of the facts the claim states, under `hasher`.
    pub hash: String,
    pub hasher: &'static str,
    /// The table's path under the lake root.
    pub rel: String,
    /// The table's canonical location: the anchor's `uri`.
    pub uri: String,
    /// The Delta version the claim was read at: the anchor's `rev`.
    pub rev: String,
}

/// The key stem of a table: `delta/` and its path under the source, lower
/// case, every character outside `[a-z0-9._/-]` mapped to `-`; a mapping that
/// lost anything ends in `-` and the first 8 hex digits of the SHA-256 of the
/// exact path, so `Sales/Orders` and `sales/orders` never share a key.
#[must_use]
pub fn stem(rel: &str) -> String {
    format!("delta/{}", segment(rel, true))
}

fn segment(text: &str, keep_slash: bool) -> String {
    let allowed = |c: char| {
        c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || matches!(c, '.' | '_' | '-')
            || (keep_slash && c == '/')
    };
    let mapped: String = text
        .to_lowercase()
        .chars()
        .map(|c| if allowed(c) { c } else { '-' })
        .collect();
    if text.chars().all(allowed) {
        mapped
    } else {
        let digest = sha256(text.as_bytes());
        format!("{mapped}-{}", &digest["sha256:".len().."sha256:".len() + 8])
    }
}

/// Every field of the schema, parents before their children, with its
/// logical path from the top.
fn walk(fields: &[Field], parent: &[String], out: &mut Vec<Walked>) {
    for f in fields {
        let mut logical = parent.to_vec();
        logical.push(f.name.clone());
        let nested = nested_fields(&f.data_type);
        out.push(Walked {
            logical: logical.clone(),
            physical: f
                .metadata
                .get(PHYSICAL)
                .and_then(Value::as_str)
                .map(str::to_owned),
            name: f.name.clone(),
            data_type: f.data_type.clone(),
            nullable: f.nullable,
            comment: f
                .metadata
                .get("comment")
                .and_then(Value::as_str)
                .map(str::to_owned),
            leaf: nested.is_none(),
        });
        if let Some(inner) = nested {
            walk(&inner, &logical, out);
        }
    }
}

/// A field and what the claims read of it; a struct holds fields, and the
/// schema claim lists leaves.
struct Walked {
    logical: Vec<String>,
    physical: Option<String>,
    name: String,
    data_type: Value,
    nullable: bool,
    comment: Option<String>,
    leaf: bool,
}

/// The fields of a struct type, when `t` is one.
fn nested_fields(t: &Value) -> Option<Vec<Field>> {
    if t["type"] != "struct" {
        return None;
    }
    t["fields"].as_array().map(|a| {
        a.iter()
            .filter_map(|f| {
                Some(Field {
                    name: f["name"].as_str()?.to_owned(),
                    data_type: f.get("type")?.clone(),
                    nullable: f["nullable"].as_bool().unwrap_or(true),
                    metadata: f["metadata"].as_object().cloned().unwrap_or_default(),
                })
            })
            .collect()
    })
}

/// Delta's name for a type: `long`, `decimal(18,2)`, `array<string>`,
/// `map<string,long>`, `struct<a:string,b:long>`.
fn type_name(t: &Value) -> String {
    match t {
        Value::String(s) => s.clone(),
        Value::Object(o) => match o.get("type").and_then(Value::as_str) {
            Some("array") => format!("array<{}>", type_name(&o["elementType"])),
            Some("map") => format!(
                "map<{},{}>",
                type_name(&o["keyType"]),
                type_name(&o["valueType"])
            ),
            Some("struct") => {
                let fields: Vec<String> = o["fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|f| {
                        format!(
                            "{}:{}",
                            f["name"].as_str().unwrap_or_default(),
                            type_name(&f["type"])
                        )
                    })
                    .collect();
                format!("struct<{}>", fields.join(","))
            }
            Some(other) => other.to_owned(),
            None => "unknown".to_owned(),
        },
        _ => "unknown".to_owned(),
    }
}

/// A path's fragment: each logical name percent-encoded outside
/// `A-Z a-z 0-9 - _ ~`, joined by `.` (ADR-0062 d5).
fn fragment(logical: &[String]) -> String {
    let mut out = String::from("col=");
    for (i, name) in logical.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        for b in name.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'~') {
                out.push(char::from(b));
            } else {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[derive(Serialize)]
struct SchemaColumn {
    nullable: bool,
    path: String,
    #[serde(rename = "type")]
    ty: String,
}

/// Declared in key order: the JSON is canonical with no sorting step.
#[derive(Serialize)]
struct SchemaDoc<'a> {
    clustering_columns: &'a [String],
    column_mapping: &'a str,
    columns: Vec<SchemaColumn>,
    partition_columns: &'a [String],
    table: &'a str,
}

/// The clustering columns as logical dotted paths: a name path of physical
/// names (the protocol's "Clustered Table") is followed down the schema.
fn clustering_logical(snapshot: &Snapshot, walked: &[Walked]) -> Vec<String> {
    snapshot
        .clustering_columns
        .iter()
        .map(|path| {
            let mut logical: Vec<String> = Vec::new();
            for seg in path {
                let step = walked.iter().find(|w| {
                    w.logical.len() == logical.len() + 1
                        && w.logical[..logical.len()] == logical[..]
                        && w.physical.as_deref().unwrap_or(&w.name) == seg
                });
                logical.push(step.map_or_else(|| seg.clone(), |w| w.name.clone()));
            }
            logical.join(".")
        })
        .collect()
}

fn property(snapshot: &Snapshot, key: &str) -> Option<String> {
    let v = md_normal(snapshot.configuration.get(key)?);
    (!v.is_empty() && v.chars().count() <= PROPERTY_MAX).then_some(v)
}

fn lines_hash(lines: &[String]) -> String {
    sha256(lines.join("\n").as_bytes())
}

/// The claims `snapshot` states, the table's first, then its schema's, then
/// its columns' in schema order.
#[must_use]
pub fn items(rel: &str, table_uri: &str, snapshot: &Snapshot) -> Vec<LakeItem> {
    let stem = stem(rel);
    let rev = snapshot.version.to_string();
    let item = |key: String, fragment: Option<String>, text: &str, hash: String, hasher| LakeItem {
        claim_key: fit_key(&key),
        fragment,
        text: nfc(text),
        hash,
        hasher,
        rel: rel.to_owned(),
        uri: table_uri.to_owned(),
        rev: rev.clone(),
    };
    let mut walked = Vec::new();
    walk(&snapshot.schema, &[], &mut walked);
    let name = snapshot
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| rel.rsplit('/').next().unwrap_or(rel).to_owned());
    let mapping = snapshot
        .configuration
        .get("delta.columnMapping.mode")
        .map_or("none", String::as_str);
    let clustering = clustering_logical(snapshot, &walked);

    // The table claim.
    let description = snapshot
        .description
        .as_deref()
        .map(md_normal)
        .filter(|d| !d.is_empty());
    let owner = property(snapshot, "oml.owner");
    let grain = property(snapshot, "oml.grain");
    let freshness = property(snapshot, "oml.freshness");
    let mut text = format!("Delta table {name} at {table_uri}.");
    let mut facts = vec![format!("name {}", md_normal(&name))];
    for (label, tag, value) in [
        ("Purpose", "desc", &description),
        ("Owner", "owner", &owner),
        ("Grain", "grain", &grain),
        ("Freshness", "freshness", &freshness),
    ] {
        if let Some(v) = value {
            let _ = write!(text, "\n{label}: {v}");
            facts.push(format!("{tag} {v}"));
        }
    }
    if !snapshot.partition_columns.is_empty() {
        let _ = write!(
            text,
            "\nPartitioned by: {}",
            snapshot.partition_columns.join(", ")
        );
        facts.extend(
            snapshot
                .partition_columns
                .iter()
                .map(|c| format!("part {c}")),
        );
    }
    if !clustering.is_empty() {
        let _ = write!(text, "\nClustered by: {}", clustering.join(", "));
        facts.extend(clustering.iter().map(|c| format!("clus {c}")));
    }
    let mut out = vec![item(
        format!("{stem}/table"),
        None,
        &text,
        lines_hash(&facts),
        TABLE_HASHER,
    )];

    // The schema claim: leaves in schema order, as one JSON object, or parts.
    let columns: Vec<SchemaColumn> = walked
        .iter()
        .filter(|w| w.leaf)
        .map(|w| SchemaColumn {
            nullable: w.nullable,
            path: w.logical.join("."),
            ty: type_name(&w.data_type),
        })
        .collect();
    let chunks: Vec<Vec<SchemaColumn>> = if columns.len() > SCHEMA_COLUMNS_MAX {
        let mut it = columns.into_iter().peekable();
        let mut parts = Vec::new();
        while it.peek().is_some() {
            parts.push(it.by_ref().take(SCHEMA_PART).collect());
        }
        parts
    } else {
        vec![columns]
    };
    let split = chunks.len() > 1;
    for (n, columns) in chunks.into_iter().enumerate() {
        let doc = SchemaDoc {
            clustering_columns: &clustering,
            column_mapping: mapping,
            columns,
            partition_columns: &snapshot.partition_columns,
            table: &name,
        };
        let json = serde_json::to_string(&doc).unwrap_or_default();
        let key = if split {
            format!("{stem}/schema/{}", n + 1)
        } else {
            format!("{stem}/schema")
        };
        out.push(item(
            key,
            None,
            &json,
            sha256(json.as_bytes()),
            SCHEMA_HASHER,
        ));
    }

    // A column claim where a comment states what the column means.
    let mut by_key: BTreeMap<String, ()> = BTreeMap::new();
    for w in &walked {
        let Some(comment) = w
            .comment
            .as_deref()
            .map(md_normal)
            .filter(|c| !c.is_empty())
        else {
            continue;
        };
        let path = w.logical.join(".");
        let physical = match (&w.physical, mapping) {
            (Some(p), m) if m != "none" => segment(p, false),
            _ => segment(&path, false),
        };
        let key = fit_key(&format!("{stem}/col/{physical}"));
        if by_key.insert(key, ()).is_some() {
            continue;
        }
        let text = format!("Column {path} of table {name}: {comment}");
        out.push(item(
            format!("{stem}/col/{physical}"),
            Some(fragment(&w.logical)),
            &text,
            lines_hash(&[format!("col {path}"), format!("comment {comment}")]),
            COL_HASHER,
        ));
    }
    out
}
