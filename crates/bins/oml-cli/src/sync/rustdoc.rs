//! The prefix lexer of the `rust` adapter (ADR-0061 d3): reads a source
//! file's leading inner doc comments and nothing after the first item, so no
//! line of code reaches it. rustc refuses an inner doc comment after an item
//! (E0753), so the prefix is the whole of the doc's possible place.

/// The `//!` and `/*! */` docs at the head of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocBlock {
    /// Each doc line as written after its `//!`, a block's lines after its
    /// `/*!`, in order.
    pub lines: Vec<String>,
    /// The first and last line of the doc, 1-based and inclusive.
    pub first: usize,
    pub last: usize,
}

impl DocBlock {
    /// The doc as prose: each line without one space after its marker, the
    /// whole trimmed.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.strip_prefix(' ').unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_owned()
    }

    /// The doc's first paragraph, its lines joined by single spaces.
    #[must_use]
    pub fn first_paragraph(&self) -> String {
        self.text()
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The doc block at the head of `src`, `None` when the file has none.
///
/// # Errors
/// The reason the file is `unsupported`: a `#![doc = …]` attribute in the
/// prefix, whose text the lexer does not read.
pub fn leading_docs(src: &str) -> Result<Option<DocBlock>, String> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let b = src.as_bytes();
    let mut i = 0;
    let mut line = 1;
    if b.starts_with(b"#!") && !b.starts_with(b"#![") {
        while i < b.len() && b[i] != b'\n' {
            i += 1;
        }
    }
    let mut lines: Vec<String> = Vec::new();
    let (mut first, mut last) = (0, 0);
    fn note(first: &mut usize, last: &mut usize, from: usize, to: usize) {
        if *first == 0 {
            *first = from;
        }
        *last = to;
    }
    while i < b.len() {
        match b[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            b'/' if b[i..].starts_with(b"//") => {
                let end = b[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(b.len(), |n| i + n);
                if b[i..].starts_with(b"//!") {
                    lines.push(src[i + 3..end].trim_end_matches('\r').to_owned());
                    note(&mut first, &mut last, line, line);
                } else if b[i..].starts_with(b"///") && !b[i..].starts_with(b"////") {
                    break;
                }
                i = end;
            }
            b'/' if b[i..].starts_with(b"/*") => {
                let inner = b[i..].starts_with(b"/*!");
                let outer = b[i..].starts_with(b"/**")
                    && !b[i..].starts_with(b"/**/")
                    && !b[i..].starts_with(b"/***");
                if outer {
                    break;
                }
                let start = i;
                let start_line = line;
                let mut depth = 0;
                while i < b.len() {
                    if b[i..].starts_with(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if b[i..].starts_with(b"*/") {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        if b[i] == b'\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                }
                if inner {
                    let body = &src[start + 3..i.saturating_sub(2).max(start + 3)];
                    lines.extend(
                        body.split('\n')
                            .map(|l| l.trim_end_matches('\r').to_owned()),
                    );
                    note(&mut first, &mut last, start_line, line);
                }
            }
            b'#' if b[i..].starts_with(b"#![") => {
                let start = i;
                i = skip_brackets(b, i + 2);
                let attr = &src[start + 3..i.saturating_sub(1).max(start + 3)];
                let name_end = attr
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(attr.len());
                if &attr[..name_end] == "doc" && attr[name_end..].trim_start().starts_with('=') {
                    return Err(format!(
                        "unsupported: a #![doc = …] attribute at line {line}, whose text this adapter \
                         does not read (ADR-0061 d3)"
                    ));
                }
                line += src[start..i].matches('\n').count();
            }
            _ => break,
        }
    }
    Ok((!lines.is_empty()).then_some(DocBlock { lines, first, last }))
}

/// The index after the `]` that closes the `[` at `open`, strings skipped.
fn skip_brackets(b: &[u8], open: usize) -> usize {
    let mut depth = 0;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            b'"' => {
                i = skip_string(b, i);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// The index after the string literal whose opening quote is at `open`.
pub(super) fn skip_string(b: &[u8], open: usize) -> usize {
    let mut i = open + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}
