//! Connections: the pure core of OAuth for Claude's hosted apps
//! (ADR-0031, proposed). The connection flow is decided, not built: these
//! modules land ahead of it and no route serves them (d16). Everything
//! here is deterministic and I/O-free: randomness, time, rows and resolved
//! addresses enter as inputs, and no function fetches, resolves, stores or
//! logs.
//!
//! - the connect code's format, entry and digest (d8);
//! - S256 PKCE and the authorize request's bounds (d4);
//! - the authorization request a page's sealed handle carries (d9);
//! - the exact-match redirect allowlist (d6);
//! - client identity: the document URL's form, the fetched document and a
//!   dynamic registration (d5);
//! - the outbound-address allow-list of a document fetch (d5);
//! - the issuer, the two discovery documents and the `/mcp` challenge
//!   (d1, d2, d3);
//! - token formats that never parse as keys (d10);
//! - code redemption and refresh rotation with reuse detection (d10, d11);
//! - the connection lifecycle and the code's checks along it (d7, d8, d9,
//!   d12);
//! - the refusals, one variant per code (d13).
//!
//! The rules are declared on `docs/api/http-api.md` ("Connections: OAuth
//! for hosted MCP clients"), `docs/architecture/roles-and-keys.md`
//! ("Connections"), `docs/api/errors.md` ("`oauth` — connections") and
//! `docs/data/postgres-schema.md` ("connections (ADR-0031)"); this module
//! mirrors them and adds nothing.
//!
//! Constant time: ADR-0031 requires no constant-time comparison, since
//! codes and tokens are matched by digest lookup in the store. The one
//! secret-derived comparison made in memory, a PKCE challenge against its
//! verifier's S256, reads every byte before answering, written by hand
//! over `std::hint::black_box` with no added dependency.

mod address;
mod client;
mod code;
mod discovery;
mod handle;
mod lifecycle;
mod pkce;
mod redirect;
mod refusal;
mod rotation;
mod syntax;
mod token;
