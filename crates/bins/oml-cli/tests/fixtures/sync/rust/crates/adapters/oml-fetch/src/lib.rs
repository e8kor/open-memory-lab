//! `oml-fetch` — the outbound fetch of a client ID metadata document, the
//! lead's [`DocumentFetcher`] (ADR-0031 d5; `docs/api/http-api.md`,
//! "Client identity"; ADR-0019: outbound I/O lives in an adapter).
//!
//! It resolves a host's A and AAAA answers and fetches from one address it
//! is handed, and it decides nothing: the 16-slot, per-host and
//! per-address caps and the address allow-list are
//! `oml_app::oauth_fetch::CappedFetcher`'s, run before this adapter is
//! asked to connect. Each blocking call owns the [`FetchGuard`] it is
//! handed, so its slots stay taken until the thread returns: dropping a
//! `spawn_blocking` handle, or the resolve timeout firing, stops nothing.
//! The fetch connects to the given address with no
//! second resolution, through a `ureq` agent whose resolver answers only
//! that address; it reads no proxy from the environment, follows no
//! redirect, sends `Accept-Encoding: identity`, and is cut at 5 seconds
//! overall and at 16 KiB of body. TLS verifies the certificate against the
//! web PKI roots for the URL's host, which `ureq` also sends as SNI.
//!
//! `ureq` is built with its default `gzip` feature in this workspace, so a
//! host that answers `Content-Encoding: gzip` against `identity` is decoded
//! and the 16 KiB cut applies to the decoded bytes; the 5-second cut bounds
//! such a host as any other.

pub mod signin;
