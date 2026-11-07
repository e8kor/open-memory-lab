//! The cache line's canonical form: RFC 8785 for the shapes a cache holds,
//! objects, arrays, strings and `null`, with members sorted by UTF-16 code
//! units and the RFC's minimal string escaping (proposed ADR-0037 d9). A
//! number, which no cached rule carries, has no form here and is `None`.
//! `oml-domain`'s test holds this equal to its `jcs::canonical_bytes`.

