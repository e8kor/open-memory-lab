//! Checks every non-prose format shares (ADR-0056 d6, d7).

use oml_app::ports::FormatRefusal;

/// The source cap of every non-prose format, chosen, not measured (#159).
pub(crate) const MAX_SOURCE_BYTES: usize = 64 * 1024;
/// A JSON or YAML document nests at most this deep (ADR-0056 d6).
pub(crate) const MAX_DEPTH: usize = 32;
/// A JSON or YAML document holds at most this many scalars (ADR-0056 d6).
pub(crate) const MAX_SCALARS: usize = 2_000;

/// The bidirectional controls, U+202A to U+202E and U+2066 to U+2069: the
/// Trojan Source class, refused anywhere in a non-prose source.
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// The size cap and the bidirectional controls, the checks that precede any
/// parse. Line and column are 1-based, counted in characters.
pub(crate) fn check_source(source: &str) -> Result<(), FormatRefusal> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(FormatRefusal::new(
            "format.too_large",
            format!(
                "the source is {} bytes; the cap is {MAX_SOURCE_BYTES}",
                source.len()
            ),
            1,
            1,
        ));
    }
    if let Some((line, column)) = find(source, is_bidi) {
        return Err(FormatRefusal::new(
            "format.bidi_control",
            "a bidirectional control character can show text in a different order than it is stored",
            line,
            column,
        ));
    }
    Ok(())
}

/// The 1-based line and column of the first character `pred` accepts.
pub(crate) fn find(source: &str, pred: impl Fn(char) -> bool) -> Option<(u32, u32)> {
    let (mut line, mut column) = (1_u32, 1_u32);
    for c in source.chars() {
        if pred(c) {
            return Some((line, column));
        }
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    None
}
