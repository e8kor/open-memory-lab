//! The `lines` adapter (ADR-0057 d21): a generic chunker for a document
//! without headings, blocks of at most 40 lines split at blank lines, keyed
//! `doc/<path>/L<n>-L<m>` with the fragment `L<n>-L<m>`.

use super::adapter::{Adapter, Extraction, doc_key_prefix, fit_key, item, utf8};
use super::markdown::lines;

pub struct LinesAdapter;

/// The most lines one block holds (ADR-0057 d21).
pub const BLOCK_LINES: usize = 40;

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
        // Paragraphs as index ranges, each cut at BLOCK_LINES.
        let mut paragraphs: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].text.trim().is_empty() {
                i += 1;
                continue;
            }
            let start = i;
            while i < lines.len() && !lines[i].text.trim().is_empty() && i - start < BLOCK_LINES {
                i += 1;
            }
            paragraphs.push((start, i));
        }
        let mut blocks: Vec<(usize, usize)> = Vec::new();
        for (start, end) in paragraphs {
            match blocks.last_mut() {
                Some(block) if end - block.0 <= BLOCK_LINES => block.1 = end,
                _ => blocks.push((start, end)),
            }
        }
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
