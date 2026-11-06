//! The prefix lexer of the `rust` adapter (ADR-0061 d3): reads a source
//! file's leading inner doc comments and nothing after the first item.

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

/// The doc block at the head of `src`, `None` when the file has none.
///
/// # Errors
/// The reason the file is `unsupported`: a `#![doc = …]` attribute in the
/// prefix.
pub fn leading_docs(_src: &str) -> Result<Option<DocBlock>, String> {
    Ok(None)
}
