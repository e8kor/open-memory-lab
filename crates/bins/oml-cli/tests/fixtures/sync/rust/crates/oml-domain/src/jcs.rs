//! RFC 8785 (JCS) canonicalization for the value shapes the chain allows.
//!
//! chain-format.md forbids floats outright ("No float ever appears in an
//! envelope or payload") and bounds every number below 2^53, so ECMAScript
//! float serialization is deliberately out of scope here: a non-integer or
//! out-of-range number is an error, never a formatting question. Object
//! members sort by UTF-16 code units per RFC 8785 §3.2.3; strings use the
//! RFC's minimal escaping with lowercase `\u00xx` for other control
//! characters.

