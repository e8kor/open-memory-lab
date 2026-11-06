//! The agent plugin's guardrail hooks (proposed ADR-0037 d6, d7, d9): `oml
//! hook guardrails`, which loads the set at SessionStart, after a compaction
//! and at SubagentStart, prints it in its own frame and, at SessionStart
//! alone, writes the session's rule cache; and `oml hook pre-tool-use`,
//! which checks one tool call against that cache with no network call. The
//! rule's meaning is `oml-guardrail`'s, the leaf the server reads it with
//! too (d18 Q10).

