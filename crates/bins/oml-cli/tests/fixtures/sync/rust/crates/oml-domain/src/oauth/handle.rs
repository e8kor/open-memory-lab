//! The authorization request a page's form carries (ADR-0031 d9 step 1;
//! `docs/api/http-api.md`, `GET /oauth/authorize`). A valid authorize
//! request writes no row: its handle is the request itself, sealed by an
//! AEAD under a key the lead holds only in memory. This module is the
//! plaintext — its fields, its one canonical byte encoding and when an
//! opened handle names a live request. Sealing and opening are the edge's.

