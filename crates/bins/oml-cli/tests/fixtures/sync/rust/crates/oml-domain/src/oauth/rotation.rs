//! The token endpoint's two decisions, over rows the store has already
//! read: redeeming an authorization code, and rotating a refresh token,
//! each with its reuse detection (ADR-0031 d10, d11;
//! `docs/data/postgres-schema.md`, "connections (ADR-0031)"). The checks
//! run in the order the schema declares, and a failed check consumes,
//! rotates and revokes nothing. Each decision names the conditional update
//! the store then makes; the store's update, not this function, is what
//! keeps two racing requests from both passing.

