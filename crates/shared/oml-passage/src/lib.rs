//! The passage rule (ADR-0081 d3): how a document's text is cut into the
//! passages recall ranks, by one deterministic rule the server and `oml
//! sync` both link, so sync's spans and the server's passages are the same
//! lines. It also holds the one slug rule a heading's anchor and a document
//! key derive from (ADR-0081 d2; ADR-0057 d7). No I/O, no clock: the same
//! bytes give the same passages.

pub mod markdown;

use markdown::{Line, Slugs, collapse, content_mask, fence_marker, heading, lines, plain};

/// The most lines one block holds (ADR-0057 d21; ADR-0081 d3).
pub const BLOCK_LINES: usize = 40;

/// The most passages one version holds (ADR-0081 d3): chosen, not measured.
pub const MAX_PASSAGES: usize = 4096;

/// The deepest heading that opens a passage (ADR-0081 d3).
pub const PASSAGE_HEADING_MAX: usize = 4;

/// The most characters of a heading read as its text, as ADR-0081 d1 cuts
/// a title (the review's finding S1 of 2026-11-18): a heading path entry,
/// and so a passage's search text, never carries more of one line.
pub const HEADING_TEXT_MAX: usize = 200;

/// The most headings a document's cut reads (the review's finding S1 of
/// 2026-11-18): four for each passage it may hold; past it the document
/// is refused as [`TooManyPassages`]. Chosen, not measured.
pub const MAX_HEADINGS: usize = 4 * MAX_PASSAGES;

/// How a document's text is read (ADR-0081 d1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `text/markdown`: cut at headings of level 1 to 4.
    Markdown,
    /// `text/plain`: the whole text.
    Plain,
}

/// One passage of a document (ADR-0081 d3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    /// Its place in the document, from 0.
    pub ordinal: usize,
    /// Its first and last line, 1-based and inclusive.
    pub start_line: usize,
    pub end_line: usize,
    /// The headings of level 1 to 4 whose sections hold it, outermost
    /// first, its own section's heading last, as plain text.
    pub heading_path: Vec<String>,
    /// The heading's slug when the passage is a whole section, else
    /// `L<n>-L<m>` (ADR-0057 d7, d21).
    pub fragment: String,
}

/// Why a text has no passages: more than [`MAX_PASSAGES`] (ADR-0081 d3,
/// `details.reason: too_many_passages`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyPassages {
    /// How many the cut had found when it stopped: one past
    /// [`MAX_PASSAGES`] passages, or one past [`MAX_HEADINGS`] headings, so
    /// a lower bound on what the text holds.
    pub count: usize,
}

impl std::fmt::Display for TooManyPassages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a document holds at most {MAX_PASSAGES} passages; this one would hold {}",
            self.count
        )
    }
}

impl std::error::Error for TooManyPassages {}

/// The blocks of the lines `start..end` (indices into `lines`), ADR-0057
/// d21's rule: paragraphs (runs of non-blank lines, each cut at
/// [`BLOCK_LINES`]) joined while a block spans at most [`BLOCK_LINES`]
/// lines. Each block is an index range `(first, past_last)`.
#[must_use]
pub fn blocks(lines: &[Line<'_>], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut paragraphs: Vec<(usize, usize)> = Vec::new();
    let mut i = start;
    while i < end {
        if lines[i].text.trim().is_empty() {
            i += 1;
            continue;
        }
        let first = i;
        while i < end && !lines[i].text.trim().is_empty() && i - first < BLOCK_LINES {
            i += 1;
        }
        paragraphs.push((first, i));
    }
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (first, past) in paragraphs {
        match out.last_mut() {
            Some(block) if past - block.0 <= BLOCK_LINES => block.1 = past,
            _ => out.push((first, past)),
        }
    }
    out
}

/// A heading outside fences, comments and front matter: its line index,
/// level and plain text.
#[derive(Debug, Clone)]
pub struct Heading {
    pub at: usize,
    pub level: usize,
    pub text: String,
}

/// Every ATX heading of a markdown document outside fences, HTML comments
/// and front matter, in document order, its text as plain text (the
/// `headings` adapter's reading, ADR-0057 d7).
#[must_use]
pub fn headings(lines: &[Line<'_>], mask: &[bool]) -> Vec<Heading> {
    headings_up_to(lines, mask, usize::MAX)
}

/// [`headings`], stopping once it holds more than `limit`, its text cut to
/// [`HEADING_TEXT_MAX`] characters.
fn headings_up_to(lines: &[Line<'_>], mask: &[bool], limit: usize) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut fence: Option<String> = None;
    for (i, line) in lines.iter().enumerate() {
        if !mask[i] {
            continue;
        }
        let t = line.text.trim_start();
        if let Some(f) = &fence {
            if t.starts_with(f.as_str()) {
                fence = None;
            }
            continue;
        }
        if let Some(f) = fence_marker(t) {
            fence = Some(f);
            continue;
        }
        if let Some((level, text)) = heading(line.text) {
            let text: String = collapse(&plain(text))
                .chars()
                .take(HEADING_TEXT_MAX)
                .collect();
            out.push(Heading {
                at: i,
                level,
                text: text.trim_end().to_owned(),
            });
            if out.len() > limit {
                break;
            }
        }
    }
    out
}

/// The passages of `text` (ADR-0081 d3, with the build's readings R1 of
/// 2026-11-18): for markdown, each heading of level 1 to 4 with its lines up
/// to the next such heading, and the lines before the first heading; for
/// plain text, the whole text. A heading with no content line before the
/// next is no passage of its own and joins the `heading_path` below it; the
/// lines before the first heading are a passage only when one is content.
/// Trailing blank lines are not part of a passage, and a passage over
/// [`BLOCK_LINES`] lines is cut into [`blocks`].
///
/// # Errors
/// [`TooManyPassages`] past [`MAX_PASSAGES`].
pub fn passages(text: &str, format: Format) -> Result<Vec<Passage>, TooManyPassages> {
    let lines = lines(text);
    let mut spans: Vec<(usize, usize, Vec<String>, Option<String>)> = Vec::new();
    match format {
        Format::Plain => {
            if let Some((s, e)) = content_span(&lines, &vec![true; lines.len()], 0, lines.len()) {
                spans.push((s, e, Vec::new(), None));
            }
        }
        Format::Markdown => {
            let mask = content_mask(&lines);
            let all = headings_up_to(&lines, &mask, MAX_HEADINGS);
            if all.len() > MAX_HEADINGS {
                return Err(TooManyPassages { count: all.len() });
            }
            let bounds: Vec<usize> = all
                .iter()
                .filter(|h| h.level <= PASSAGE_HEADING_MAX)
                .map(|h| h.at)
                .collect();
            let first = bounds.first().copied().unwrap_or(lines.len());
            if let Some((s, e)) = content_span(&lines, &mask, 0, first) {
                spans.push((s, e, Vec::new(), None));
            }
            let mut slugs = Slugs::default();
            let mut stack: Vec<(usize, String)> = Vec::new();
            // The next bound is read by index: each heading of level 1 to 4
            // is the next bound in turn.
            let mut bound = 0;
            for h in &all {
                let slug = slugs.next(&h.text);
                if h.level > PASSAGE_HEADING_MAX {
                    continue;
                }
                bound += 1;
                stack.retain(|(l, _)| *l < h.level);
                stack.push((h.level, h.text.clone()));
                let next = bounds.get(bound).copied().unwrap_or(lines.len());
                // R1: a heading with no content line is no passage.
                let Some((_, e)) = content_span(&lines, &mask, h.at + 1, next) else {
                    continue;
                };
                let path = stack.iter().map(|(_, t)| t.clone()).collect();
                spans.push((h.at, e, path, Some(slug)));
                // Every span is at least one passage: stop past the cap.
                if spans.len() > MAX_PASSAGES {
                    return Err(TooManyPassages { count: spans.len() });
                }
            }
        }
    }
    let mut out = Vec::new();
    for (start, past, heading_path, slug) in spans {
        let whole = past - start <= BLOCK_LINES;
        if whole && let Some(slug) = slug {
            out.push((start, past, heading_path, slug));
            continue;
        }
        let cut = if whole {
            vec![(start, past)]
        } else {
            blocks(&lines, start, past)
        };
        for (s, e) in cut {
            let fragment = format!("L{}-L{}", lines[s].number, lines[e - 1].number);
            out.push((s, e, heading_path.clone(), fragment));
            if out.len() > MAX_PASSAGES {
                return Err(TooManyPassages { count: out.len() });
            }
        }
    }
    if out.len() > MAX_PASSAGES {
        return Err(TooManyPassages { count: out.len() });
    }
    Ok(out
        .into_iter()
        .enumerate()
        .map(|(ordinal, (s, e, heading_path, fragment))| Passage {
            ordinal,
            start_line: lines[s].number,
            end_line: lines[e - 1].number,
            heading_path,
            fragment,
        })
        .collect())
}

/// The span of `start..end` from its first content line to its last
/// non-blank line, or `None` when no line in it is content (masked in and
/// not blank). A span that opens at a heading keeps the heading line: the
/// caller passes `at + 1` to test the body and keeps `at` as the start.
fn content_span(
    lines: &[Line<'_>],
    mask: &[bool],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let content = |i: usize| mask[i] && !lines[i].text.trim().is_empty();
    let first = (start..end).find(|&i| content(i))?;
    let mut past = end;
    while past > first + 1 && lines[past - 1].text.trim().is_empty() {
        past -= 1;
    }
    Some((first, past))
}

/// The fragment a passage's URI names (the review's reading R11 of
/// 2026-11-18): its heading slug when that begins with a letter or a digit
/// and holds nothing outside `[a-z0-9_-]`, else its lines, `L<start>-L<end>`, so every URI is
/// RFC 3986 text and every passage is readable through its own.
#[must_use]
pub fn uri_fragment(fragment: &str, start_line: usize, end_line: usize) -> String {
    // A slug that begins with `-` or `_` was cut from a heading with no
    // letter or digit of its own (a repeat suffix of an empty slug).
    let safe = fragment
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && fragment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if safe {
        fragment.to_owned()
    } else {
        lines_fragment(start_line, end_line)
    }
}

/// A span's line fragment, `L<start>-L<end>` (ADR-0057 d21).
#[must_use]
pub fn lines_fragment(start_line: usize, end_line: usize) -> String {
    format!("L{start_line}-L{end_line}")
}

impl Passage {
    /// Whether `?passage=<f>` names this passage: its fragment or its
    /// lines (R11).
    #[must_use]
    pub fn named_by(&self, f: &str) -> bool {
        self.fragment == f || lines_fragment(self.start_line, self.end_line) == f
    }
}

/// A passage's text as served: its source lines, `start_line` to
/// `end_line`, byte for byte as `text` holds them (ADR-0081 d4, d6, the
/// build's reading R2), so a passage read is the same span of a full read.
#[must_use]
pub fn passage_text<'a>(text: &'a str, passage: &Passage) -> &'a str {
    let mut offset = 0;
    let mut start = None;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let number = i + 1;
        if number == passage.start_line {
            start = Some(offset);
        }
        offset += line.len();
        if number == passage.end_line {
            let s = start.unwrap_or(offset);
            let piece = &text[s..offset];
            let piece = piece.strip_suffix('\n').unwrap_or(piece);
            return piece.strip_suffix('\r').unwrap_or(piece);
        }
    }
    start.map_or("", |s| &text[s..])
}

/// A passage's search text (the build's reading R2): the document's title,
/// then its heading path joined by ` > ` (a first heading equal to the
/// title said once), then its lines.
#[must_use]
pub fn search_text(title: &str, text: &str, passage: &Passage) -> String {
    let mut path = passage.heading_path.as_slice();
    if path.first().is_some_and(|h| h == title) {
        path = &path[1..];
    }
    // Each part at most HEADING_TEXT_MAX characters, so the line before a
    // passage's lines is bounded however long a title or heading was sent.
    let line = std::iter::once(title)
        .chain(path.iter().map(String::as_str))
        .map(|part| part.chars().take(HEADING_TEXT_MAX).collect::<String>())
        .collect::<Vec<_>>()
        .join(" > ");
    format!("{line}\n\n{}", passage_text(text, passage))
}

/// A heading's anchor and a document key's slug: the one rule (ADR-0081 d2,
/// amended 2026-11-18), re-exported where a caller wants it by name.
pub use markdown::slug;
