//! `oml hook session-start` (ADR-0029; cli/reference.md `oml hook
//! session-start`): the `oml` plugin's SessionStart hook. Under the agent's
//! own key, from the plugin's two options or else `OML_URL` and
//! `OML_API_KEY`, one pair from one source (ADR-0040 d3), it reads the own-work
//! listing and one budgeted recall on the provider ring and returns the
//! SessionStart hook document Claude Code adds to the session, or the reason
//! it prints nothing. It never reads the CLI's stored credentials, and every
//! failure is silent on stdout: the caller exits 0 either way.
//!
//! `oml hook tool-failure` and `oml hook session-end` (ADR-0038; cli/
//! reference.md `oml hook tool-failure`): with `OML_FAILURE_RECALL=1`, one
//! budgeted recall from a failed Bash command's distinctive error words,
//! deduplicated through a per-session cache of ids and digests that the
//! SessionEnd hook deletes.

