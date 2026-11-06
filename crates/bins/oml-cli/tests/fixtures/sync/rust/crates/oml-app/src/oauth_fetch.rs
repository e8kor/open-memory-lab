//! The fetch caps of a client ID metadata document (ADR-0031 d5;
//! `docs/api/http-api.md`, "Client identity"; `docs/architecture/
//! security.md`, "Connections: the OAuth surface"). At most 16 fetches run
//! at once on the lead, at most one per host name and at most one per
//! resolved address, so many names pointing at one slow address hold one
//! slot. A fetch past any cap waits for nothing: it is
//! [`DocumentFetchFailure::Busy`] at once, and the authorize use case then
//! serves a copy kept within 24 hours or answers `oauth.client_invalid`
//! (`reason: fetch_busy`). Every resolver answer must pass the domain's
//! address allow-list before any connection, and the connection goes to a
//! checked answer.
//!
//! A slot is held for as long as the outbound work it stands for runs, not
//! for as long as anything awaits that work: the host's slot (with one of
//! the 16) and the address's slot go to the adapter inside a
//! [`FetchGuard`], which the adapter moves into the thread running the
//! resolver or the connection. A request whose client disconnects, or a
//! resolve cut by its timeout while `getaddrinfo` still runs, keeps its
//! slots until that thread returns.
//!
//! The caps hold for one [`CappedFetcher`]: the lead composes exactly one.

