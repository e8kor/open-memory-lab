//! The sign-in providers' outbound calls, the lead's [`SignInExchange`]
//! (ADR-0045 d3, d4; ADR-0019: outbound I/O lives in an adapter).
//!
//! Google: the code, the PKCE verifier and the client secret go to Google's
//! token endpoint, and the answer's `id_token` comes back for the
//! application to judge. GitHub: the same to GitHub's token endpoint, then
//! `GET /user` for the account's numeric `id` and `GET /user/emails` for
//! its addresses. The provider's access token is dropped when the call
//! returns; nothing here stores it. The endpoints are constants, never
//! settings (ADR-0045 d17). Each call reads no proxy from the environment,
//! follows no redirect, and is cut at [`CALL_TIMEOUT`] and [`BODY_MAX_BYTES`];
//! TLS verifies the certificate against the web PKI roots. The adapter
//! judges nothing: an error answer, a timeout or a body it cannot read is
//! [`ProviderFailure::Refused`] with a closed reason.

