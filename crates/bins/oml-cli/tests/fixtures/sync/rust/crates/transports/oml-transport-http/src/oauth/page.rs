//! The authorization page's HTML and the headers every page carries
//! (docs/api/http-api.md, "The authorization page"; ADR-0031 d9). The page
//! runs no script and sets no cookie; its one stylesheet is inline, and the
//! SHA-256 that the CSP's `style-src` names is computed from it when the
//! crate compiles, so the two cannot drift.
//!
//! The sentences are http-api.md's, word for word. Every string that comes
//! from a client is HTML-escaped and shortened, its control and
//! bidirectional formatting characters replaced, and it is never placed
//! where the page names a host: a document client's host is the server's
//! own reading of the document URL, rendered in its own element after
//! "verified by", so a crafted `client_name` cannot pass for one.

