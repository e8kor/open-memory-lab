//! `oml setup claude-code`'s local half (ADR-0041 d7): the roles it refuses,
//! the Claude Code version it requires, how it reads the marketplace Claude
//! Code already holds under the name `open-memory-lab`, the `claude` commands
//! it runs,
//! the values it writes to `claude plugin configure --values-stdin`, and the
//! lines it prints in their place with `--print` or with no `claude` on
//! `PATH`. The key travels on stdin alone, never in argv (ADR-0040 d4).

