//! The learned sparse encoder's pass (ADR-0052 d15): the lead drains the
//! queue the write's trigger fills, off the commit path, so a write never
//! waits for the model and a version is found by `bm25_postings` from its
//! promotion and by `sparse` once encoded.

