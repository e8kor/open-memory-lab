//! The client address behind a proxy: `X-Forwarded-For` is read only when
//! the socket peer is in `server.trusted_proxies`, right to left, skipping
//! every listed address (ADR-0042 d7).

