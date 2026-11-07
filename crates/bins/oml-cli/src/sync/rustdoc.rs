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

/// The largest doc block, in bytes of its lines, a claim carries.
pub const MAX_DOC_BYTES: usize = 64 * 1024;

/// What the prefix lexer found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lexed {
    pub doc: Option<DocBlock>,
    /// A `/*!` block or a `#![` attribute that never closes: the file does
    /// not compile, so it has no doc and nothing after the opener is text.
    pub unterminated: bool,
}

/// The doc block at the head of `src`, `None` when the file has none or its
/// head never closes (see [`lex`]).
///
/// # Errors
/// The reason the file is `unsupported`: a `#![doc = …]` attribute in the
/// prefix, whose text the lexer does not read.
pub fn leading_docs(src: &str) -> Result<Option<DocBlock>, String> {
    lex(src).map(|l| l.doc)
}

/// The prefix of `src`, with whether it ended inside an unterminated block.
///
/// # Errors
/// As [`leading_docs`].
pub fn lex(src: &str) -> Result<Lexed, String> {
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
                    lines.push(
                        src.get(i + 3..end)
                            .unwrap_or("")
                            .trim_end_matches('\r')
                            .to_owned(),
                    );
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
                let mut closed = false;
                while i < b.len() {
                    if b[i..].starts_with(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if b[i..].starts_with(b"*/") {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            closed = true;
                            break;
                        }
                    } else {
                        if b[i] == b'\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                }
                if inner && !closed {
                    return Ok(Lexed {
                        doc: None,
                        unterminated: true,
                    });
                }
                if inner {
                    let body = src
                        .get(start + 3..i.saturating_sub(2).max(start + 3))
                        .unwrap_or("");
                    lines.extend(
                        body.split('\n')
                            .map(|l| l.trim_end_matches('\r').to_owned()),
                    );
                    note(&mut first, &mut last, start_line, line);
                }
            }
            b'#' if b[i..].starts_with(b"#![") => {
                let start = i;
                let Some(end) = skip_brackets(b, i + 2) else {
                    return Ok(Lexed {
                        doc: None,
                        unterminated: true,
                    });
                };
                i = end;
                let attr = src.get(start + 3..i - 1).unwrap_or("");
                let name_end = attr
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(attr.len());
                if &attr[..name_end] == "doc" && attr[name_end..].trim_start().starts_with('=') {
                    return Err(format!(
                        "unsupported: #![doc = …] at line {line}; use a doc comment instead"
                    ));
                }
                line += src[start..i].matches('\n').count();
            }
            _ => break,
        }
    }
    Ok(Lexed {
        doc: (!lines.is_empty()).then_some(DocBlock { lines, first, last }),
        unterminated: false,
    })
}

/// The index after the `]` that closes the `[` at `open`, strings skipped;
/// `None` when it never closes.
fn skip_brackets(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
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
    None
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
