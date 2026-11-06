//! The markdown an adapter reads, as plain text: links to their text,
//! emphasis and code marks dropped, a paragraph to one line, list items and
//! table rows to lines of their own (ADR-0057 d7). Hand-written and
//! deterministic: no adapter reads anything but its path and bytes.

/// One line of the source, numbered from 1.
