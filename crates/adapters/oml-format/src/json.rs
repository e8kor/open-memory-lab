//! `application/json`: one JSON value, a duplicate key refused (ADR-0056 d6).

use std::collections::BTreeSet;
use std::fmt;

use oml_app::ports::{Format, FormatDiff, FormatRefusal, Parsed, RenderHint, ScanView};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::common::{MAX_DEPTH, MAX_SCALARS, check_source};
use crate::fragment::{self, Entry};

pub struct Json;

/// A parsed value reduced to what the adapter reads.
enum Node {
    Scalar(String),
    Map(Vec<(String, Node)>),
    Seq(Vec<Node>),
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Node;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_bool<E>(self, v: bool) -> Result<Node, E> {
                Ok(Node::Scalar(v.to_string()))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Node, E> {
                Ok(Node::Scalar(v.to_string()))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Node, E> {
                Ok(Node::Scalar(v.to_string()))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Node, E> {
                Ok(Node::Scalar(
                    serde_json::Number::from_f64(v).map_or_else(String::new, |n| n.to_string()),
                ))
            }
            fn visit_str<E>(self, v: &str) -> Result<Node, E> {
                Ok(Node::Scalar(v.to_owned()))
            }
            fn visit_unit<E>(self) -> Result<Node, E> {
                Ok(Node::Scalar("null".to_owned()))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
                let mut items = Vec::new();
                while let Some(n) = seq.next_element::<Node>()? {
                    items.push(n);
                }
                Ok(Node::Seq(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                let mut seen = BTreeSet::new();
                let mut members = Vec::new();
                while let Some(k) = map.next_key::<String>()? {
                    if !seen.insert(k.clone()) {
                        return Err(de::Error::custom(format!("duplicate key {k:?}")));
                    }
                    members.push((k, map.next_value::<Node>()?));
                }
                Ok(Node::Map(members))
            }
        }
        d.deserialize_any(V)
    }
}

fn walk(
    node: &Node,
    path: &mut Vec<String>,
    depth: usize,
    out: &mut Walk,
) -> Result<(), FormatRefusal> {
    if depth > MAX_DEPTH {
        return Err(FormatRefusal::new(
            "format.too_deep",
            format!("the document nests deeper than {MAX_DEPTH}"),
            1,
            1,
        ));
    }
    match node {
        Node::Scalar(v) => {
            if out.entries.len() >= MAX_SCALARS {
                return Err(FormatRefusal::new(
                    "format.too_many_scalars",
                    format!("the document holds more than {MAX_SCALARS} scalars"),
                    1,
                    1,
                ));
            }
            out.entries.push(Entry {
                path: path.clone(),
                value: v.clone(),
            });
        }
        Node::Map(members) => {
            for (k, v) in members {
                path.push(k.clone());
                out.keys.push(Entry {
                    path: path.clone(),
                    value: k.clone(),
                });
                walk(v, path, depth + 1, out)?;
                path.pop();
            }
        }
        Node::Seq(items) => {
            for (i, v) in items.iter().enumerate() {
                path.push(i.to_string());
                walk(v, path, depth + 1, out)?;
                path.pop();
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct Walk {
    entries: Vec<Entry>,
    keys: Vec<Entry>,
}

fn read(source: &str) -> Result<Walk, FormatRefusal> {
    check_source(source)?;
    let node: Node = serde_json::from_str(source).map_err(|e| {
        let message = e.to_string();
        let reason = if message.contains("duplicate key") {
            "json.duplicate_key"
        } else if message.contains("recursion limit") {
            "format.too_deep"
        } else {
            "json.invalid"
        };
        FormatRefusal::new(
            reason,
            message,
            u32::try_from(e.line()).unwrap_or(u32::MAX),
            u32::try_from(e.column()).unwrap_or(u32::MAX),
        )
    })?;
    let mut out = Walk::default();
    walk(&node, &mut Vec::new(), 0, &mut out)?;
    Ok(out)
}

impl Format for Json {
    fn id(&self) -> &'static str {
        "json/1"
    }
    fn validate(&self, source: &str) -> Result<Parsed, FormatRefusal> {
        let w = read(source)?;
        Ok(fragment::parsed(&w.entries, &w.keys))
    }
    fn search_text(&self, parsed: &Parsed) -> String {
        parsed.search_text.clone()
    }
    fn scan_views(&self, parsed: &Parsed) -> Vec<ScanView> {
        parsed.views.clone()
    }
    fn render_hint(&self) -> RenderHint {
        RenderHint::Escaped
    }
    fn diff(&self, old: &str, new: &str) -> Result<FormatDiff, FormatRefusal> {
        Ok(fragment::diff(&read(old)?.entries, &read(new)?.entries))
    }
}
