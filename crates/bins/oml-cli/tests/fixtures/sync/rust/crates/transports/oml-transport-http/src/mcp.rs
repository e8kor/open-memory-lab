//! The agent plugin: MCP over streamable HTTP, stateless (docs/api/mcp-plugin.md,
//! ADR-0014). One POST carries one complete JSON-RPC request and receives one
//! complete JSON response; no `Mcp-Session-Id` is issued, and `GET` — the
//! server-initiated stream — is refused.
//!
//! Every tool translates to exactly one call on the HTTP API: the handler
//! builds that call's headers and body and runs the same handler the ring
//! route runs, so validation, idempotency and the typed refusals cannot drift
//! between the two surfaces. A workarea argument is passed through as sent,
//! an id or a name; the facade resolves nothing (mcp-plugin.md; ADR-0026
//! d4). `tools/list` serves docs/api/mcp-tools.json whole, its one
//! declaration site: the three tools `oml_write`, `oml_read` and
//! `oml_workarea` (ADR-0039 d2). The designed tools and arguments live in
//! docs/api/mcp-tools.designed.json, which is never served. The eight v0.3.0
//! tool names were removed in v0.5.1 and answer as any unknown tool does
//! (ADR-0039 d12).

mod call;
