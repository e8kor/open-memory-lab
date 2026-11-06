//! `application/yaml`: one YAML 1.2 document with no anchor, alias, tag, merge
//! key or duplicate key, parsed by `saphyr-parser` (ADR-0056 d6, question 2).

use std::collections::BTreeSet;

use oml_app::ports::{Format, FormatDiff, FormatRefusal, Parsed, RenderHint, ScanView};
use saphyr_parser::{Event, Parser, Span};

use crate::common::{MAX_DEPTH, MAX_SCALARS, check_source};
use crate::fragment::{self, Entry};

pub struct Yaml;

fn refuse(reason: &'static str, message: &str, span: Span) -> FormatRefusal {
    FormatRefusal::new(
        reason,
        message,
        u32::try_from(span.start.line()).unwrap_or(u32::MAX),
        u32::try_from(span.start.col() + 1).unwrap_or(u32::MAX),
    )
}

/// An open collection: its path segment source and what the next scalar is.
enum Frame {
    Map {
        /// The key read, awaiting its value.
        key: Option<String>,
        seen: BTreeSet<String>,
    },
    Seq {
        next: usize,
    },
}

#[derive(Default)]
struct Walk {
    entries: Vec<Entry>,
    keys: Vec<Entry>,
}

fn path_of(stack: &[Frame]) -> Vec<String> {
    stack
        .iter()
        .filter_map(|f| match f {
            Frame::Map { key, .. } => key.clone(),
            Frame::Seq { next } => Some(next.saturating_sub(1).to_string()),
        })
        .collect()
}

fn read(source: &str) -> Result<Walk, FormatRefusal> {
    check_source(source)?;
    let mut out = Walk::default();
    let mut stack: Vec<Frame> = Vec::new();
    let mut documents = 0_u32;
    let mut root_seen = false;
    for item in Parser::new_from_str(source) {
        let (event, span) = item.map_err(|e| {
            FormatRefusal::new(
                "yaml.invalid",
                e.info().to_owned(),
                u32::try_from(e.marker().line()).unwrap_or(u32::MAX),
                u32::try_from(e.marker().col() + 1).unwrap_or(u32::MAX),
            )
        })?;
        let is_map = matches!(event, Event::MappingStart(..));
        match event {
            Event::DocumentStart(_) => {
                documents += 1;
                if documents > 1 {
                    return Err(refuse(
                        "yaml.second_document",
                        "a claim is one YAML document",
                        span,
                    ));
                }
            }
            Event::Alias(_) => {
                return Err(refuse(
                    "yaml.alias_refused",
                    "an alias can expand into a document many times its size",
                    span,
                ));
            }
            Event::Scalar(value, _, anchor, tag) => {
                if anchor != 0 {
                    return Err(refuse(
                        "yaml.alias_refused",
                        "an anchor is refused with its aliases",
                        span,
                    ));
                }
                if tag.is_some() {
                    return Err(refuse(
                        "yaml.tag_refused",
                        "a tag names a type the reader would build",
                        span,
                    ));
                }
                root_seen = true;
                // A mapping's first scalar of a pair is its key.
                if let Some(Frame::Map { key, seen }) = stack.last_mut()
                    && key.is_none()
                {
                    if value == "<<" {
                        return Err(refuse(
                            "yaml.merge_refused",
                            "a merge key is an alias in disguise",
                            span,
                        ));
                    }
                    if !seen.insert(value.to_string()) {
                        return Err(refuse(
                            "yaml.duplicate_key",
                            &format!("duplicate key {value:?}"),
                            span,
                        ));
                    }
                    *key = Some(value.to_string());
                    let path = path_of(&stack);
                    out.keys.push(Entry {
                        path,
                        value: value.to_string(),
                    });
                    continue;
                }
                if out.entries.len() >= MAX_SCALARS {
                    return Err(refuse(
                        "format.too_many_scalars",
                        &format!("the document holds more than {MAX_SCALARS} scalars"),
                        span,
                    ));
                }
                if let Some(Frame::Seq { next }) = stack.last_mut() {
                    *next += 1;
                }
                out.entries.push(Entry {
                    path: path_of(&stack),
                    value: value.to_string(),
                });
                if let Some(Frame::Map { key, .. }) = stack.last_mut() {
                    *key = None;
                }
            }
            Event::SequenceStart(anchor, tag) | Event::MappingStart(anchor, tag) => {
                if anchor != 0 {
                    return Err(refuse(
                        "yaml.alias_refused",
                        "an anchor is refused with its aliases",
                        span,
                    ));
                }
                if tag.is_some() {
                    return Err(refuse(
                        "yaml.tag_refused",
                        "a tag names a type the reader would build",
                        span,
                    ));
                }
                root_seen = true;
                if let Some(Frame::Map { key: None, .. }) = stack.last() {
                    return Err(refuse("yaml.complex_key", "a key is one scalar", span));
                }
                if let Some(Frame::Seq { next }) = stack.last_mut() {
                    *next += 1;
                }
                if stack.len() >= MAX_DEPTH {
                    return Err(refuse(
                        "format.too_deep",
                        &format!("the document nests deeper than {MAX_DEPTH}"),
                        span,
                    ));
                }
                stack.push(if is_map {
                    Frame::Map {
                        key: None,
                        seen: BTreeSet::new(),
                    }
                } else {
                    Frame::Seq { next: 0 }
                });
            }
            Event::SequenceEnd | Event::MappingEnd => {
                stack.pop();
                if let Some(Frame::Map { key, .. }) = stack.last_mut() {
                    *key = None;
                }
            }
            _ => {}
        }
    }
    if !root_seen {
        return Err(FormatRefusal::new(
            "yaml.empty",
            "the document holds no value",
            1,
            1,
        ));
    }
    Ok(out)
}

impl Format for Yaml {
    fn id(&self) -> &'static str {
        "yaml/1"
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
