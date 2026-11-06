//! S256 PKCE and the authorize request's parameter bounds (ADR-0031 d4;
//! RFC 7636). Only S256 is served: no `plain`, and no request without a
//! `code_challenge`.

