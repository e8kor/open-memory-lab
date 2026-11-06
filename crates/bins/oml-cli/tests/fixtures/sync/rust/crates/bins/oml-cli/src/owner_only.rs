//! Files only their owner opens: the `--out` key file ([`crate::issue`]) and
//! the stored credentials ([`crate::creds`]) each hold a key value, so each
//! is made owner-only before a byte of it is written (ADR-0028 d1;
//! cli/reference.md "Owner-only files").
//!
//! On Unix the file is mode 0600. On Windows it carries a protected DACL with
//! one entry, allowing the current user full access and inheriting nothing,
//! set on a handle opened with no sharing, so no other open of the file
//! succeeds while the CLI holds it. On any other platform [`restrict`]
//! refuses, so the CLI writes no key value there.

