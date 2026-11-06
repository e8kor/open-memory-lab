//! The thin HTTP client: every verb is a request to the lab's own API and
//! nothing more — the CLI holds no authority of its own (ADR-0015,
//! cli/reference.md). Refusals arrive as the typed error envelope of
//! docs/api/errors.md and map to exit code 1; local failures map to 2.

