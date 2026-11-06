//! The rule cache's codec: the served rules, each with its guardrail's
//! `artifact_id` and body, as one JSON line, then a line holding that
//! line's SHA-256 in lower-case hex (proposed ADR-0037 d9). Reading and
//! writing the file, its directory's modes, the rename and the 30-day sweep
//! are the hook's; this module holds the bytes and what each one means.
//!
//! The line is `{"rules":[{"artifact_id":…,"body":…,"rule":{…}}, …]}` in
//! RFC 8785 canonical form, and the file is `<line>\n<digest>\n` exactly.

