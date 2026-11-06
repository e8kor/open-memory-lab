//! The web pages' use cases (ADR-0045): the email allowlist an admin key
//! keeps over `/v2/web/emails` (d2), the admission of a provider's answer
//! against it (d3, d4), and the session rows a signed-in browser acts
//! through (d5). The pages themselves are the transport's.
//!
//! The person's session keys and the pages' acts through them are
//! `pages.rs`'s.

