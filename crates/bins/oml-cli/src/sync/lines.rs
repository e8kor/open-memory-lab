//! The `lines` adapter (ADR-0057 d21): a generic chunker for a document
//! without headings, blocks of at most 40 lines split at blank lines, keyed
//! `doc/<path>/L<n>-L<m>` with the fragment `L<n>-L<m>`.

use super::adapter::{Adapter, Extraction, doc_key_prefix, fit_key, item, utf8};
use super::markdown::lines;

pub struct LinesAdapter;

/// The most lines one block holds (ADR-0057 d21), the passage rule's.
pub const BLOCK_LINES: usize = oml_passage::BLOCK_LINES;

impl Adapter for LinesAdapter {
    fn name(&self) -> &'static str {
        "lines"
    }

    fn accepts(&self, path: &str) -> bool {
        path.ends_with(".md") || path.ends_with(".txt")
    }

    fn extract(&self, path: &str, bytes: &[u8]) -> Result<Extraction, String> {
        let doc = utf8(path, bytes)?;
        let lines = lines(&doc);
        let blocks = oml_passage::blocks(&lines, 0, lines.len());
        let prefix = doc_key_prefix(path);
        let mut out = Extraction::default();
        for (start, end) in blocks {
            let (n, m) = (lines[start].number, lines[end - 1].number);
            let fragment = format!("L{n}-L{m}");
            let span = lines[start..end]
                .iter()
                .map(|l| l.text)
                .collect::<Vec<_>>()
                .join("\n");
            out.items.push(item(
                fit_key(&format!("{prefix}/{}", fragment.to_lowercase())),
                Some(fragment),
                span.trim_end(),
                &span,
                (n, m),
            ));
        }
        Ok(out)
    }
}
