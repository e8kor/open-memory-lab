//! The issuer, the two discovery documents and the `/mcp` challenge, all
//! built from `oauth.issuer` alone, never from a request's `Host`
//! (ADR-0031 d1, d2, d3; `docs/api/http-api.md`, "Connections: OAuth for
//! hosted MCP clients"; `docs/operations/configuration.md`, `oauth.issuer`).

