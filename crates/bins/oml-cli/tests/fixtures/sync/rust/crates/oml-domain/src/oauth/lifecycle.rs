//! The connection lifecycle and the connect code's checks along it
//! (ADR-0031 d7, d8, d9, d12; `docs/architecture/roles-and-keys.md`,
//! "Connections", the lifecycle diagram). A connection row is created at
//! Connect, `authorized`; the token exchange opens it; it ends `revoked`,
//! once, and never reopens. An `authorized` connection whose authorization
//! code expires unexchanged, or whose code is voided, is removed rather
//! than transitioned: it was never journalled and cannot be named.

