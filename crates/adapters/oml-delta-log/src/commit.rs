//! A JSON commit, `_last_checkpoint`, the schema string and the clustering
//! domain: the four actions the reader decodes. Any other action in a line is
//! skipped by serde without being built.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::Value;

use crate::{Field, Limits, LogError, Meta, Protocol, State, kept, unreadable};

const CLUSTERING_DOMAIN: &str = "delta.clustering";

#[derive(Deserialize)]
struct Line {
    protocol: Option<ProtocolJson>,
    #[serde(rename = "metaData")]
    meta_data: Option<MetaJson>,
    #[serde(rename = "domainMetadata")]
    domain_metadata: Option<DomainJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolJson {
    min_reader_version: u64,
    reader_features: Option<Vec<String>>,
    writer_features: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetaJson {
    id: String,
    name: Option<String>,
    description: Option<String>,
    schema_string: String,
    #[serde(default)]
    partition_columns: Vec<String>,
    #[serde(default, deserialize_with = "kept_config")]
    configuration: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct DomainJson {
    domain: String,
    #[serde(default)]
    configuration: String,
    #[serde(default)]
    removed: bool,
}

/// A configuration map that builds an entry only for a key the reader keeps;
/// every other key's value is skipped.
fn kept_config<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, String>, D::Error> {
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a configuration object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some(key) = m.next_key::<String>()? {
                if kept(&key) {
                    if let Some(v) = m.next_value::<Option<String>>()? {
                        out.insert(key, v);
                    }
                } else {
                    m.next_value::<IgnoredAny>()?;
                }
            }
            Ok(out)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(BTreeMap::new())
        }
    }
    d.deserialize_any(V)
}

/// Applies one commit (or a V2 checkpoint's JSON file) to `state`.
pub(crate) fn replay(bytes: &[u8], state: &mut State, limits: &Limits) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("not UTF-8 ({e})"))?;
    for (n, line) in text.lines().enumerate() {
        if line.len() > limits.commit_line {
            return Err(format!(
                "line {} is {} bytes, over the {}-byte line cap",
                n + 1,
                line.len(),
                limits.commit_line
            ));
        }
        if line.trim().is_empty() {
            continue;
        }
        let action: Line =
            serde_json::from_str(line).map_err(|e| format!("line {}: {e}", n + 1))?;
        if let Some(p) = action.protocol {
            state.protocol = Some(Protocol {
                min_reader: p.min_reader_version,
                reader_features: p.reader_features.unwrap_or_default(),
                writer_features: p.writer_features.unwrap_or_default(),
            });
        }
        if let Some(m) = action.meta_data {
            state.meta = Some(Meta {
                id: m.id,
                name: m.name,
                description: m.description,
                schema_string: m.schema_string,
                partition_columns: m.partition_columns,
                configuration: m.configuration,
            });
        }
        if let Some(d) = action.domain_metadata
            && d.domain == CLUSTERING_DOMAIN
        {
            state.clustering = (!d.removed).then_some(d.configuration);
        }
    }
    Ok(())
}

/// The version `_last_checkpoint` names.
pub(crate) fn last_checkpoint(bytes: &[u8]) -> Result<u64, LogError> {
    #[derive(Deserialize)]
    struct Hint {
        version: u64,
    }
    serde_json::from_slice::<Hint>(bytes)
        .map(|h| h.version)
        .map_err(|e| unreadable(format!("_last_checkpoint does not parse ({e})")))
}

/// The fields of a `schemaString`.
pub(crate) fn parse_schema(schema_string: &str, max_columns: usize) -> Result<Vec<Field>, String> {
    let v: Value = serde_json::from_str(schema_string).map_err(|e| e.to_string())?;
    let columns = count_fields(&v, max_columns);
    if columns > max_columns {
        return Err(format!(
            "the schema has more than {max_columns} columns, over the {max_columns}-column cap"
        ));
    }
    if v["type"] != "struct" {
        return Err("the root is not a struct".into());
    }
    let fields = v["fields"].as_array().ok_or("the root has no fields")?;
    fields
        .iter()
        .map(|f| {
            Ok(Field {
                name: f["name"].as_str().ok_or("a field has no name")?.to_owned(),
                data_type: f.get("type").cloned().ok_or("a field has no type")?,
                nullable: f["nullable"].as_bool().unwrap_or(true),
                metadata: f["metadata"].as_object().cloned().unwrap_or_default(),
            })
        })
        .collect()
}

/// The `clusteringColumns` of the clustering domain's configuration.
pub(crate) fn clustering_columns(configuration: &str) -> Result<Vec<Vec<String>>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Config {
        #[serde(default)]
        clustering_columns: Vec<Vec<String>>,
    }
    serde_json::from_str::<Config>(configuration)
        .map(|c| c.clustering_columns)
        .map_err(|e| e.to_string())
}

/// The fields of a type, nested ones included, counted up to `cap + 1`.
fn count_fields(t: &Value, cap: usize) -> usize {
    let mut n = 0;
    if let Some(fields) = t.get("fields").and_then(Value::as_array) {
        n += fields.len();
        for f in fields {
            if n > cap {
                return n;
            }
            n += f
                .get("type")
                .map_or(0, |t| count_fields(t, cap - n.min(cap)));
        }
    }
    for k in ["elementType", "keyType", "valueType"] {
        if n > cap {
            break;
        }
        n += t.get(k).map_or(0, |t| count_fields(t, cap - n.min(cap)));
    }
    n
}
