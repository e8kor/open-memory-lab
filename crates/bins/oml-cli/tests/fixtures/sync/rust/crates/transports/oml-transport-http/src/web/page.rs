//! The web pages' HTML and the headers every answer carries (ADR-0045 d8):
//! no script, one inline stylesheet in the OML brand whose SHA-256 the CSP
//! names (computed when the crate compiles, as the connect page's is),
//! `default-src 'none'`, `form-action 'self'` plus the configured
//! providers' origins, `frame-ancestors 'none'`, `base-uri 'none'`,
//! `Cache-Control: no-store`, `Referrer-Policy: same-origin` and `nosniff`.
//! Every string from a person, an agent, a provider or the store is
//! escaped and stripped of control and bidirectional formatting characters
//! by the connect page's functions.
//!
//! The look is the landing page's: night indigo ground, paper ink, ice
//! links, star for the one primary act, the system sans for prose and mono
//! for labels, names and ids.

