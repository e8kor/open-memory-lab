//! Connections: the pure core of OAuth for Claude's hosted apps
//! (ADR-0031, proposed). The connection flow is decided, not built: these
//! modules land ahead of it and no route serves them (d16). Everything
//! here is deterministic and I/O-free: randomness, time, rows and resolved
//! addresses enter as inputs, and no function fetches, resolves, stores or
//! logs.
//!
//! - the connect code's format, entry and digest (d8);
//! - S256 PKCE and the authorize request's bounds (d4);
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
mod lifecycle;
mod pkce;
mod redirect;
mod refusal;
mod rotation;
mod syntax;
mod token;

pub use address::{admit_fetch_answers, admitted_fetch_address};
pub use client::{
    ClientId, ClientMetadata, DOCUMENT_MAX_BYTES, DocumentUrl, classify_client_id,
    validate_document, validate_registration,
};
pub use code::{CODE_SYMBOLS, CROCKFORD_ALPHABET, ConnectCode};
pub use discovery::{
    ChallengeCause, Issuer, IssuerError, RESOURCE_NAME, SCOPE, authorization_server_document,
    check_resource, mcp_challenge, protected_resource_document,
};
pub use lifecycle::{
    ATTEMPTS_PER_PAGE, CodeRow, ConnectionAct, ConnectionState, KeyState, LifecycleError,
    RevokedCause, after_wrong_entry, check_code_entry, check_confirmation, connection_transition,
};
pub use pkce::{
    AuthorizeBounds, CLIENT_ID_MAX_BYTES, CODE_CHALLENGE_CHARS, REDIRECT_URI_MAX_BYTES,
    STATE_MAX_BYTES, check_authorize_bounds, check_pkce_method, s256_challenge, verify_s256,
};
pub use redirect::{
    CLAUDE_CALLBACK, RedirectKind, SelectedRedirect, admit_redirect_uri, redirect_matches,
    select_redirect,
};
pub use refusal::{
    AuthorizationCodeInvalid, AuthorizationExpired, ClientInvalid, ConnectRefused, OAUTH_CODES,
    PkceRequired, RedirectRefused, RefreshInvalid, Refusal, Validation,
};
pub use rotation::{
    ExchangeDecision, ExchangeRequest, ExchangeRow, RefreshDecision, RefreshRow, decide_exchange,
    decide_refresh,
};
pub use token::{
    BASE62_CHARS, KEY_PREFIX, Token, TokenKind, base62_43, is_key_value, refresh_token_digest,
    token_kind,
};
