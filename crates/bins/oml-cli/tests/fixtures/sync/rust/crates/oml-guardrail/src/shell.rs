//! The `shell` class's argument: a `command` tokenised by POSIX quoting
//! without expansion, then split into segments at unquoted `;`, `&`, `&&`,
//! `||`, `|` and newlines (proposed ADR-0037 d8).
//!
//! What the record makes the check ask on, rather than tokenise, is a
//! [`ShellRefusal`]: a quote or escape left open, command or process
//! substitution, a subshell, a brace group or a here-document. This reading
//! adds the remaining compound forms of the same shape, which d8 neither
//! names as bypasses nor asks on: a segment whose first unquoted word is a
//! reserved word (`if`, `while`, `for`, `case`, `!`, `time`, ...), and
//! `$'…'` or `$"…"` quoting, which the tokeniser does not decode. A
//! redirection is not a word: its operator and an IO number before it are
//! dropped, and its target word is kept apart from the segment's words, so
//! no rule matches it but the built-in cache rule sees it (d9). A `#` that
//! begins a word comments out the rest of its line, as POSIX tokenising
//! does.

/// Why a command was not tokenised. Whenever a `shell` rule is cached the
/// check asks on it (proposed ADR-0037 d8).
