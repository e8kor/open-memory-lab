//! The redirect allowlist: exactly Claude's hosted callback, matched on the
//! string as written and never after URL parsing (ADR-0031 d6;
//! `docs/api/http-api.md`, "The redirect allowlist"). The list is fixed,
//! not a setting: each entry decides where an authorization code may be
//! delivered. Loopback redirects are deferred, and admitting one is a later
//! recorded decision (ADR-0031, "Loopback redirects for Claude Code").

