//! A memory's source anchor: the origin its author declares, checked
//! against one grammar and never fetched (ADR-0057 d1-d3, d21).
//!
//! The anchor is part of the signed submission, immutable per version, and
//! clear metadata. This module holds the grammar alone: what a well-formed
//! anchor, filter and source check are, and which field a refusal names. It
//! computes nothing from an anchor.

