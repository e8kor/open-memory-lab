//! The doctor lines of `oml sync`'s failures (ADR-0057 d17): an `origin`
//! remote carrying credentials, and the public server without
//! `--allow-public-server` recorded for this clone. A server that accepts no
//! anchor (`sources_unsupported`), the default workarea's stale versions
//! (`sources_stale`) and its waiting claim edges are the `sources` lines'.

use std::path::Path;

use super::source::{ALLOW_PUBLIC_CONFIG, Git, is_public_server, repo_of_remote};

/// The lines, each `ok …` or `warn …`; empty when nothing applies.
#[must_use]
pub fn lines(url: Option<&str>, cwd: &Path) -> Vec<String> {
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
    if let (Some(git), Some(url)) = (&git, url)
        && is_public_server(url)
        && git.config(ALLOW_PUBLIC_CONFIG).as_deref() != Some("true")
    {
        out.push(format!(
            "warn sync_public_server: {url} is the maintainer's public server, but consent is \
             not recorded for this clone; run oml sync with --allow-public-server to opt in"
        ));
    }
    out
}
