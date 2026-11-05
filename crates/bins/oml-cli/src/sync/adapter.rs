//! The source-adapter port (ADR-0057 d7): an adapter is a pure function of a
//! path and its bytes, yielding items and edge statements in document order.
//! It reads no clock, network, environment or other file, so the same input
//! yields byte-identical output.

use serde::Serialize;

use super::hash::{md_hash, nfc, sha256};

/// One extracted item: a claim the source states.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    pub claim_key: String,
    /// The part of the file: a decision handle, a heading's slug or a line
    /// span (ADR-0057 d21); `None` for the whole file.
    pub fragment: Option<String>,
    /// The claim's text, NFC.
    pub text: String,
    /// The span's first and last line, 1-based and inclusive.
    pub lines: (usize, usize),
    /// `oml-md/1` of the span's source text.
    pub hash: String,
}

/// One edge a document states, by claim key (ADR-0057 d10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EdgeStatement {
    pub from: String,
    pub label: String,
    pub to: String,
}

/// What one file yields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extraction {
    pub items: Vec<Item>,
    pub edges: Vec<EdgeStatement>,
}

/// An adapter: `extract` is pure; `accepts` chooses its files by path.
pub trait Adapter {
    fn name(&self) -> &'static str;
    fn accepts(&self, path: &str) -> bool;
    /// # Errors
    /// The reason a file is `unreadable`: invalid UTF-8, for one.
    fn extract(&self, path: &str, bytes: &[u8]) -> Result<Extraction, String>;
}

/// The adapters v0.5.8 builds (ADR-0057 d7, d21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Adr,
    Headings,
    Lines,
}

impl Kind {
    /// # Errors
    /// An unknown name.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "adr" => Ok(Self::Adr),
            "headings" => Ok(Self::Headings),
            "lines" => Ok(Self::Lines),
            other => Err(format!(
                "--adapter {other:?} is not one of adr, headings, lines (ADR-0057 d7, d21)"
            )),
        }
    }

    #[must_use]
    pub fn adapter(self) -> Box<dyn Adapter> {
        match self {
            Self::Adr => Box::new(super::adr::AdrAdapter),
            Self::Headings => Box::new(super::headings::HeadingsAdapter),
            Self::Lines => Box::new(super::lines::LinesAdapter),
        }
    }

    /// The default for a source: `adr` when it holds `ADR-NNNN-*.md` files
    /// (an ADR directory's template and index are then not loaded),
    /// `headings` otherwise (ADR-0057 d7).
    #[must_use]
    pub fn default_for(paths: &[String]) -> Self {
        if paths.iter().any(|p| super::adr::adr_number(p).is_some()) {
            Self::Adr
        } else {
            Self::Headings
        }
    }
}

/// The bytes of a file as text, or why it is unreadable.
///
/// # Errors
/// Invalid UTF-8.
pub fn utf8(path: &str, bytes: &[u8]) -> Result<String, String> {
    std::str::from_utf8(bytes)
        .map(nfc)
        .map_err(|e| format!("{path} is not UTF-8 ({e})"))
}

/// An item from its parts, its text NFC and its span hashed.
#[must_use]
pub fn item(
    claim_key: String,
    fragment: Option<String>,
    text: &str,
    span: &str,
    lines: (usize, usize),
) -> Item {
    Item {
        claim_key,
        fragment,
        text: nfc(text),
        lines,
        hash: md_hash(span),
    }
}

/// The longest claim key ADR-0050 d2's grammar admits.
pub const KEY_MAX: usize = 128;

/// A claim key fitted to the grammar's length: a longer one keeps its first
/// 111 characters and ends in `-` and the first 16 hex digits of the SHA-256
/// of the full key (ADR-0057 d7).
#[must_use]
pub fn fit_key(key: &str) -> String {
    if key.len() <= KEY_MAX {
        return key.to_owned();
    }
    let digest = sha256(key.as_bytes());
    let head: String = key.chars().take(111).collect();
    format!("{head}-{}", &digest["sha256:".len().."sha256:".len() + 16])
}

/// `doc/` and a path without `.md`, in lower case, every character outside
/// `[a-z0-9._/-]` mapped to `-` (ADR-0057 d7).
#[must_use]
pub fn doc_key_prefix(path: &str) -> String {
    let stem = path.strip_suffix(".md").unwrap_or(path);
    let mapped: String = stem
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '/' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("doc/{mapped}")
}

/// A key segment mapped to the grammar, as [`doc_key_prefix`] maps a path.
#[must_use]
pub fn key_segment(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}
