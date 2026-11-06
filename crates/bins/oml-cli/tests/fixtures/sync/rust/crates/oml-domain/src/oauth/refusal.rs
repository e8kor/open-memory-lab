//! The refusals of the connection flow: one [`Refusal`] variant per code of
//! the `oauth` family, plus `request.validation` for the malformed
//! parameters the OAuth endpoints carry as `invalid_request` or the RFC's own
//! name (ADR-0031 d13; `docs/api/errors.md`, "`oauth` — connections"). The
//! core refuses in types; the HTTP status, the RFC `error` and the shape —
//! an RFC body, a redirect or a page — are attached once, at the edge
//! (errors.md, "Implementation mapping").
//!
//! The reason a variant carries is the core's own record of which check
//! failed. Only [`ClientInvalid::details_reason`] and [`ConnectRefused::as_str`]
//! name wire values, the `details.reason` strings errors.md declares; every
//! other reason is for tests and the edge's choice of RFC `error`, and a
//! connect code's refusal carries none at all, so that nothing downstream
//! can tell a guesser that a code existed (ADR-0031 d8).

/// The eleven codes of the `oauth` family, in the order ADR-0031 d13 lists
/// them.
