//! The connect code: twelve symbols of Crockford's base32, 60 bits, shown
//! as three groups of four, entered under Crockford's decoding rule, stored
//! as the SHA-256 of its decoded symbols (ADR-0031 d8;
//! `docs/architecture/roles-and-keys.md`, "Connections"). The randomness is
//! the caller's: this module only formats it.

