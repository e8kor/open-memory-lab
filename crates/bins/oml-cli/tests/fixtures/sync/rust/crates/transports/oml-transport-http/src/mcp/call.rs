//! The `mcp_call` line: one per `/mcp` POST that carries a JSON-RPC
//! message, written before that request's `http_request` line, under its
//! ids, with closed values only. Neither the JSON-RPC `id` nor any argument
//! is logged, and a caller-chosen name the facade does not recognize logs
//! `unknown` (ADR-0042 d10).

