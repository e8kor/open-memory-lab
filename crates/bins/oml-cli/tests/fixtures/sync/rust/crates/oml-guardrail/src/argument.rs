//! Argument normalisation for the `edit`, `read` and `fetch` classes: a
//! path made absolute against the input's `cwd` and normalised lexically,
//! and the WHATWG host of a URL, serialised as the standard serialises it,
//! lower-cased, without a trailing dot (proposed ADR-0037 d8).

/// `path` made absolute against `cwd` and normalised lexically: `.` and
/// empty components dropped, `..` removing the component before it (never
/// past the root), no trailing `/`. Symlinks are not resolved: nothing here
/// reads the file system. `None` when the path is empty, or relative with
/// no absolute `cwd` to resolve it against.
