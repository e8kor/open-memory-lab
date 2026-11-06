//! The source-adapter port (ADR-0057 d7): an adapter is a pure function of a
//! path and its bytes, yielding items and edge statements in document order.
//! It reads no clock, network, environment or other file, so the same input
//! yields byte-identical output.

use serde::Serialize;

use super::hash::{HASHER, md_hash, nfc, sha256};

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
    /// The hash of the span under `hasher`.
    pub hash: String,
    /// The hasher that produced `hash`: `oml-md/1` unless the adapter names
    /// another of the closed set (ADR-0061 d7).
    pub hasher: &'static str,
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
    /// What the file holds that no item carries, said once.
    pub warnings: Vec<String>,
}

/// What a set of files yields: each file's extraction, in path order, and
/// the files an adapter could not read or chose not to read (ADR-0061 d4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetExtraction {
    pub files: Vec<(String, Extraction)>,
    /// A file read and found unreadable or unsupported: reported, fails the
    /// run, its claims neither written nor retired.
    pub unreadable: Vec<(String, String)>,
    /// A file deliberately left out (undeclared, generated): reported, does
    /// not fail the run, its claims neither written nor retired.
    pub skipped: Vec<(String, String)>,
}

/// An adapter: `extract` is pure; `accepts` chooses its files by path.
pub trait Adapter {
    fn name(&self) -> &'static str;
    fn accepts(&self, path: &str) -> bool;
    /// # Errors
    /// The reason a file is `unreadable`: invalid UTF-8, for one.
    fn extract(&self, path: &str, bytes: &[u8]) -> Result<Extraction, String>;
    /// An adapter may be a pure function of every `(path, bytes)` under the
    /// source (ADR-0061 d4); the default extracts each file alone, in path
    /// order, so `adr`, `headings` and `lines` are unchanged.
    fn extract_set(&self, files: &[(String, Vec<u8>)]) -> SetExtraction {
        let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = SetExtraction::default();
        for (file, bytes) in sorted {
            match self.extract(file, bytes) {
                Ok(x) => out.files.push((file.clone(), x)),
                Err(why) => out.unreadable.push((file.clone(), why)),
            }
        }
        out
    }
}

/// The adapters (ADR-0057 d7, d21; ADR-0061).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Adr,
    Headings,
    Lines,
    Rust,
}

impl Kind {
    /// # Errors
    /// An unknown name.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "adr" => Ok(Self::Adr),
            "headings" => Ok(Self::Headings),
            "lines" => Ok(Self::Lines),
            "delta" => Err(
                "--adapter delta requires the oml-lake build; run `oml sync <lake> --adapter \
                 delta` there"
                    .to_owned(),
            ),
            "rust" => Ok(Self::Rust),
            other => Err(format!(
                "--adapter {other:?} is invalid; choose adr, headings, lines, or rust"
            )),
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Adr => "adr",
            Self::Headings => "headings",
            Self::Lines => "lines",
            Self::Rust => "rust",
        }
    }

    /// The adapter whose namespace a claim key is in: `adr-NNNN-…` for
    /// `adr`, `doc/…/l<n>-l<m>` for `lines`, any other `doc/…` for
    /// `headings`. A key in none (an agent's memory) is no sync's.
    #[must_use]
    pub fn of_key(key: &str) -> Option<Self> {
        if key.starts_with("rust/") {
            return Some(Self::Rust);
        }
        if let Some(rest) = key.strip_prefix("adr-") {
            let num = rest.get(..5)?;
            return (num[..4].chars().all(|c| c.is_ascii_digit()) && num.ends_with('-'))
                .then_some(Self::Adr);
        }
        let rest = key.strip_prefix("doc/")?;
        let last = rest.rsplit('/').next().unwrap_or(rest);
        let is_span = last
            .strip_prefix('l')
            .and_then(|r| r.split_once("-l"))
            .is_some_and(|(n, m)| {
                !n.is_empty()
                    && !m.is_empty()
                    && n.chars().all(|c| c.is_ascii_digit())
                    && m.chars().all(|c| c.is_ascii_digit())
            });
        Some(if is_span { Self::Lines } else { Self::Headings })
    }

    #[must_use]
    pub fn adapter(self) -> Box<dyn Adapter> {
        match self {
            Self::Adr => Box::new(super::adr::AdrAdapter),
            Self::Headings => Box::new(super::headings::HeadingsAdapter),
            Self::Lines => Box::new(super::lines::LinesAdapter),
            Self::Rust => Box::new(super::rust::RustAdapter),
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
        hasher: HASHER,
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
/// `[a-z0-9._/-]` mapped to `-` (ADR-0057 d7). A mapping that lost
/// anything (a capital, a space) ends in `-` and 8 hex digits of the
/// path's SHA-256, so two paths never share a key prefix.
#[must_use]
pub fn doc_key_prefix(path: &str) -> String {
    let stem = path.strip_suffix(".md").unwrap_or(path);
    let lossless = stem.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '/' | '-')
    });
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
    if lossless {
        format!("doc/{mapped}")
    } else {
        let digest = sha256(path.as_bytes());
        format!(
            "doc/{mapped}-{}",
            &digest["sha256:".len().."sha256:".len() + 8]
        )
    }
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
