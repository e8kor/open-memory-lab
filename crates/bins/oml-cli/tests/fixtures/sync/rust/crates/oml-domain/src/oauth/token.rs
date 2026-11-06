//! Token formats: an access token is `oml_at_` and a refresh token
//! `oml_rt_`, each followed by 43 base62 characters encoding 256 bits under
//! the key format's generation rule; a key is `oml_` and 43 base62
//! characters. The second underscore keeps a token from ever parsing as a
//! key, and a key from ever parsing as a token (ADR-0031 d10;
//! `docs/architecture/roles-and-keys.md`, "Key format" and "Connections").
//! The randomness is the caller's.

