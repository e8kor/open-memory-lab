//! The authorization request's handle sealer (ADR-0031 d9 step 1;
//! `docs/api/http-api.md`, `GET /oauth/authorize`): ChaCha20-Poly1305 under
//! a 256-bit key the process generates when it starts and holds only in
//! memory. Nothing writes the key anywhere, so a restart or a takeover of
//! the lead ends every page in progress, which starts again.
//!
//! A handle is the lower-case hex of a fresh random 96-bit nonce, then the
//! ciphertext and its 128-bit tag. The nonce is drawn from the OS for each
//! seal; at the page rates one lead serves, a repeated random nonce under
//! one key is not a practical concern within a process's life. The
//! associated data names the plaintext's purpose, so a sealed handle opens
//! as nothing else.

