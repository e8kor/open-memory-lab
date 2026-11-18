//! The `document` adapter (ADR-0081 d7, the build's readings R4 and R5 of
//! 2026-11-18): one claim of kind `document` per file, keyed `doc/` and the
//! path without its extension under ADR-0057 d7's mapping, anchored to the
//! whole file with no fragment and hashed `oml-md/1` over the whole file.
//! The server cuts its passages by the rule `oml-passage` holds, so a
//! passage hit and a `headings` claim of one section name one fragment.

use super::adapter::{Adapter, Extraction, doc_key_of, item, utf8};

pub struct DocumentAdapter;

/// A document key's bound and a segment's (ADR-0081 d2).
pub const DOCUMENT_KEY_MAX: usize = 128;
pub const DOCUMENT_SEGMENT_MAX: usize = 64;

/// The media type a file is loaded as, by its extension: `.md` as markdown
/// and `.txt` as plain text (R4); `None` for any other file.
#[must_use]
pub fn format_of(path: &str) -> Option<(&'static str, &str)> {
    if let Some(stem) = path.strip_suffix(".md") {
        Some(("text/markdown", stem))
    } else {
        path.strip_suffix(".txt").map(|stem| ("text/plain", stem))
    }
}

/// Whether `key` meets ADR-0081 d2's grammar: `doc/` and segments of 1 to
/// 64 characters of `a-z 0-9 . _ -`, never `.` or `..`, at most 128 in all.
#[must_use]
pub fn fits_document_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("doc/") else {
        return false;
    };
    key.len() <= DOCUMENT_KEY_MAX
        && rest.split('/').all(|s| {
            (1..=DOCUMENT_SEGMENT_MAX).contains(&s.len())
                && s != "."
                && s != ".."
                && s.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
                })
        })
}

impl Adapter for DocumentAdapter {
    fn name(&self) -> &'static str {
        "document"
    }

    fn accepts(&self, path: &str) -> bool {
        format_of(path).is_some()
    }

    fn extract(&self, path: &str, bytes: &[u8]) -> Result<Extraction, String> {
        let (format, stem) =
            format_of(path).ok_or_else(|| format!("{path} is neither .md nor .txt"))?;
        let doc = utf8(path, bytes)?;
        let key = doc_key_of(path, stem);
        if !fits_document_key(&key) {
            return Err(format!(
                "{path} maps to the key {key}, outside a document key's grammar: segments of 1 \
                 to 64 characters, at most 128 in all (ADR-0081 d2)"
            ));
        }
        let last = doc.split('\n').count();
        let mut it = item(key, None, &doc, &doc, (1, last));
        it.format = Some(format);
        Ok(Extraction {
            items: vec![it],
            ..Extraction::default()
        })
    }
}
