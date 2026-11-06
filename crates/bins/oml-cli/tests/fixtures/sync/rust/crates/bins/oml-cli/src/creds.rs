//! The local credentials file: the one URL-and-key pair the CLI starts from
//! when no flag and no environment variable sets either (ADR-0039 d15;
//! cli/reference.md "Start settings"). `oml login` and `oml bootstrap claim`
//! write `url` and `api_key` and nothing else; storing the key value locally is the
//! documented convenience, and the file is owner-only like the server's
//! keyring: 0600 on Unix, one-entry protected DACL on Windows
//! ([`crate::owner_only`]).

