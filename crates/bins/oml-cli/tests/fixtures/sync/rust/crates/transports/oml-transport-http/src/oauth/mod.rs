//! The connection flow's edge (ADR-0031): what the transport holds that the
//! application layer reaches only through its ports, and the unversioned
//! OAuth routes outside both rings — the two discovery documents, the
//! authorization page (`GET` and `POST /oauth/authorize`, [`page`]),
//! `POST /oauth/register` and `POST /oauth/token` (http-api.md,
//! "Connections: OAuth for hosted MCP clients"; ADR-0031 d1, d3, d4, d5,
//! d9, d13).
//!
//! The routes exist only where the deployment sets `oauth.issuer`; unset,
//! each answers `404` as an unknown path does, and the root form
//! `/.well-known/oauth-protected-resource` is never served. The documents
//! are built from the issuer alone, never from `Host`. Neither endpoint
//! makes an outbound call, and every refusal is the RFC's body
//! ([`OAuthError`]).

pub mod page;
pub mod seal;
