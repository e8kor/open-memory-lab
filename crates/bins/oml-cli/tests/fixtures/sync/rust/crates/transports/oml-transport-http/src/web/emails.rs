//! The allowlist's three lab routes (ADR-0045 d2): `GET /v2/web/emails`,
//! `POST /v2/web/emails` and `POST /v2/web/emails/remove`, admin by key
//! role. The email travels in a body, never in a path or query string, and
//! the listing's cursor is an email's digest, so no proxy log records an
//! address.

