//! The doctor lines of `oml sync`'s failures (ADR-0057 d17): an `origin`
//! remote carrying credentials, the public server without
//! `--allow-public-server` recorded for this clone, and the default
//! workarea's stale versions and waiting claim edges. A server that accepts
//! no anchor is the `sources` line's (`sources_unsupported`).

use std::path::Path;

use crate::client::{Client, encode};

use super::source::{ALLOW_PUBLIC_CONFIG, Git, is_public_server, repo_of_remote};

/// Pages of the default workarea read for the stale count, 200 rows each.
const STALE_PAGES: usize = 5;

/// The lines, each `ok …` or `warn …`; empty when nothing applies.
#[must_use]
pub fn lines(
    url: Option<&str>,
    client: Option<&Client>,
    default_workarea: Option<&str>,
    cwd: &Path,
) -> Vec<String> {
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
            "warn sync_public_server: the URL is the public server {url}, and \
             --allow-public-server is not recorded for this clone; oml sync refuses it until a \
             run passes the flag (ADR-0057 d6)"
        ));
    }
    let (Some(client), Some(workarea)) = (client, default_workarea) else {
        return out;
    };
    let mut stale = 0;
    let mut cursor: Option<String> = None;
    let mut complete = false;
    for _ in 0..STALE_PAGES {
        let mut path = format!("/v1/memories?limit=200&workarea={}", encode(workarea));
        if let Some(c) = &cursor {
            path.push_str(&format!("&cursor={}", encode(c)));
        }
        let Ok((_, page)) = client.get(&path) else {
            return out;
        };
        stale += page["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["stale"].is_string())
            .count();
        match page["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_owned()),
            None => {
                complete = true;
                break;
            }
        }
    }
    let waiting = client
        .get(&format!(
            "/v2/edges?kind=claim&limit=200&workarea_id={}",
            encode(workarea)
        ))
        .map(|(_, page)| {
            page["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|e| e["state"] == "pending")
                .count()
        })
        .unwrap_or(0);
    let at_least = if complete { "" } else { "at least " };
    out.push(format!(
        "{}{at_least}{stale} served version(s) marked stale and {waiting} claim edge(s) waiting \
         for a second key on the default workarea",
        if stale > 0 || waiting > 0 {
            "warn sync_stale: "
        } else {
            "ok "
        }
    ));
    out
}
