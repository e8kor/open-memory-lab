//! The `lines` adapter (ADR-0057 d21): a generic chunker for a document
//! without headings, blocks of at most 40 lines split at blank lines, keyed
//! `doc/<path>/L<n>-L<m>` with the fragment `L<n>-L<m>`.

