//! The source-adapter port (ADR-0057 d7): an adapter is a pure function of a
//! path and its bytes, yielding items and edge statements in document order.
//! It reads no clock, network, environment or other file, so the same input
//! yields byte-identical output.

