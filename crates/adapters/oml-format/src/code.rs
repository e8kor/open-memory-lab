//! `text/vnd.oml.code; lang=<tag>`: a snippet in a named language, indexed as
//! its source, no symbols extracted (ADR-0056 d7).

use oml_app::ports::{
    Format, FormatChange, FormatDiff, FormatRefusal, Parsed, RenderHint, ScanView,
};

use crate::common::check_source;

pub struct Code;

/// The closed list of languages (ADR-0056 d7).
pub const LANGUAGES: [&str; 20] = [
    "rust",
    "python",
    "typescript",
    "javascript",
    "go",
    "java",
    "kotlin",
    "scala",
    "sql",
    "shell",
    "c",
    "cpp",
    "csharp",
    "ruby",
    "php",
    "swift",
    "hcl",
    "toml",
    "dockerfile",
    "plantuml",
];

/// The lines of a source after span normalisation: trailing space and blank
/// lines dropped, indentation reduced to its width in spaces (oml-span/1's
/// reading of a reformat as no change).
fn normalised(source: &str) -> Vec<String> {
    source
        .lines()
        .map(|l| {
            let indent: usize = l
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum();
            format!("{}{}", " ".repeat(indent), l.trim())
        })
        .filter(|l| !l.trim().is_empty())
        .collect()
}

impl Format for Code {
    fn id(&self) -> &'static str {
        "code/1"
    }
    fn validate(&self, source: &str) -> Result<Parsed, FormatRefusal> {
        check_source(source)?;
        Ok(Parsed {
            search_text: source.to_owned(),
            ..Parsed::default()
        })
    }
    fn search_text(&self, parsed: &Parsed) -> String {
        parsed.search_text.clone()
    }
    fn scan_views(&self, _parsed: &Parsed) -> Vec<ScanView> {
        Vec::new()
    }
    fn render_hint(&self) -> RenderHint {
        RenderHint::Escaped
    }
    fn diff(&self, old: &str, new: &str) -> Result<FormatDiff, FormatRefusal> {
        let (a, b) = (normalised(old), normalised(new));
        if a == b {
            return Ok(FormatDiff {
                equivalent: true,
                changes: Vec::new(),
            });
        }
        let mut changes = Vec::new();
        for l in &a {
            if !b.contains(l) {
                changes.push(FormatChange {
                    op: "remove",
                    path: String::new(),
                    before: Some(l.trim().to_owned()),
                    after: None,
                });
            }
        }
        for l in &b {
            if !a.contains(l) {
                changes.push(FormatChange {
                    op: "add",
                    path: String::new(),
                    before: None,
                    after: Some(l.trim().to_owned()),
                });
            }
        }
        Ok(FormatDiff {
            equivalent: changes.is_empty(),
            changes,
        })
    }
}
