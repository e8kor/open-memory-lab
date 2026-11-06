//! The pure core of the web pages' sign-in (ADR-0045 d2, d3, d4, d5, d8):
//! the allowlist entry's email and user, the sealed sign-in state, the
//! provider answers the server admits, the session token's CSRF companion,
//! and the refusals a sign-in answers. No I/O: the providers' answers enter
//! as values, and time as an input.

