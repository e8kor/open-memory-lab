//! `oml-server-lead` — boot order per deployment.md, ADR-0002 d7 and
//! ADR-0034 d4: configuration; the serving and owner pools; the role-split
//! checks before migrating, the migrations as the owner, the checks after;
//! the owner pool closed; then, over the serving pool alone, keyring, fork
//! check, the naming of rows that predate names, the enrollment token and
//! the listener. A role-split refusal exits non-zero, logged as
//! `postgres.role_split` with its reason, before the listener binds
//! (ADR-0034 d5). A failed fork check does not
//! exit: `/readyz` answers 503 and every write answers `system.unavailable`
//! until an operator intervenes (http-api.md "Health"). Nor does a failed
//! naming: a row the start could not name (ADR-0026 d11) keeps `/readyz` at
//! 503 and every request at `system.unavailable`.

