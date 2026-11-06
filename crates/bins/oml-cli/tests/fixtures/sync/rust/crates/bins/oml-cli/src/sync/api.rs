//! The calls a run makes, behind a port so the diff and the run are tested
//! against a fake (ADR-0057 d8–d11). [`HttpApi`] speaks the HTTP API under
//! the caller's key, one request per memory, honouring `Retry-After` (d6).

