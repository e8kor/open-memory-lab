//! The guardrail frame: the text `oml hook guardrails` prints, `oml
//! guardrails show` and the export write, and the MCP prompt `guardrails`
//! returns, rendered from the one answer of `GET /v1/guardrails` (proposed
//! ADR-0037 d6, d7, d11). One renderer, so the hook, the export and the
//! prompt cannot say different things.

