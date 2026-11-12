//! The doctor lines of `oml sync`'s failures (ADR-0057 d17, as ADR-0076 d9
//! amends it): an `origin` remote carrying credentials, a server that
//! declares itself shared without consent recorded for its origin in this
//! clone, and the old consent value `true`, which names no server. A server
//! that accepts no
//! anchor (`sources_unsupported`), the default workarea's stale versions
//! (`sources_stale`) and its waiting claim edges are the `sources` lines'.

use std::path::Path;

use super::source::{ALLOW_PUBLIC_CONFIG, Git, origin_of, repo_of_remote};

/// The lines, each `ok …` or `warn …`; empty when nothing applies.
/// `shared` is what the server's whoami said, `None` when it was not asked
/// or did not say.
#[must_use]
pub fn lines(url: Option<&str>, shared: Option<bool>, cwd: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let git = Git::at(cwd).ok();
    if let Some(git) = &git
        && let Some(remote) = git.origin_url()
        && repo_of_remote(&remote).is_ok_and(|r| r.had_credentials)
    {
        out.push(
            "warn sync_remote_credentials: this clone's origin remote URL carries credentials \
             (in .git/config or a url.insteadOf rewrite in a git config); oml sync strips them \
             from every anchor, and they belong in a credential helper"
                .to_owned(),
        );
    }
    let recorded = git
        .as_ref()
        .map(|git| git.config_all(ALLOW_PUBLIC_CONFIG))
        .unwrap_or_default();
    if recorded.iter().any(|v| v == "true") {
        out.push(format!(
            "warn sync_consent_unbound: {ALLOW_PUBLIC_CONFIG}=true names no server and is no \
             longer read; run oml sync --allow-public-server once against the server you mean"
        ));
    }
    if let (Some(_), Some(url), Some(true)) = (&git, url, shared)
        && !recorded.contains(&origin_of(url))
    {
        out.push(format!(
            "warn sync_shared_server: {url} declares itself shared, and consent is not \
             recorded for this origin in this clone; run oml sync with --allow-public-server to \
             opt in"
        ));
    }
    out
}
