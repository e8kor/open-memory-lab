//! `oml anchor <path>[#<fragment>]` (ADR-0057 d13): the anchor of a
//! committed file or of one part of it, at `--rev`, as one JSON object an
//! agent passes to `oml_write` or `oml write --anchor`.

use std::path::Path;

use serde_json::{Value, json};

use super::adapter::Kind;
use super::hash::{HASHER, md_hash};
use super::source::{Git, check_repo, repo_of_remote};

/// The anchor of `target` (`path` or `path#fragment`, relative to `cwd`).
/// A fragment is a decision handle, a heading's slug or a line span
/// `L<n>` / `L<n>-L<m>`; `adapter` names how the file is split, by default
/// `adr` for an `ADR-NNNN-*.md` and `headings` otherwise.
///
/// # Errors
/// A path outside a clone or absent at `rev`, an unknown fragment (the
/// error names the ones there are), a repository name that cannot be
/// derived.
pub fn anchor(
    cwd: &Path,
    target: &str,
    rev: &str,
    repo: Option<&str>,
    adapter: Option<Kind>,
) -> Result<Value, String> {
    let (path_arg, fragment) = match target.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (target, None),
    };
    let start = cwd.join(path_arg);
    let dir = start
        .ancestors()
        .find(|d| d.is_dir())
        .unwrap_or(cwd)
        .to_path_buf();
    let git = Git::at(&dir)?;
    let path = git.relative(cwd, path_arg)?;
    let commit = git.commit(rev)?;
    let repo = match repo {
        Some(r) => {
            check_repo(r)?;
            r.to_owned()
        }
        None => {
            let url = git
                .origin_url()
                .ok_or("the clone has no origin remote; pass --repo (ADR-0057 d3)")?;
            repo_of_remote(&url)?.repo
        }
    };
    let bytes = git
        .read(&commit, &path)
        .map_err(|_| format!("{path} is not a file at {}", &commit[..12]))?;
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("{path} is not UTF-8 ({e})"))?;
    let (fragment, hash) = match fragment {
        None => (None, md_hash(text)),
        Some(f) => match line_span(f) {
            Some((n, m)) => {
                let lines: Vec<&str> = text.split('\n').collect();
                if m > lines.len() {
                    return Err(format!(
                        "{path} has {} lines; {f} is past its end",
                        lines.len()
                    ));
                }
                (Some(f.to_owned()), md_hash(&lines[n - 1..m].join("\n")))
            }
            None => {
                let kind =
                    adapter.unwrap_or_else(|| Kind::default_for(std::slice::from_ref(&path)));
                let x = kind.adapter().extract(&path, &bytes)?;
                let Some(item) = x.items.iter().find(|i| i.fragment.as_deref() == Some(f)) else {
                    let there: Vec<&str> = x
                        .items
                        .iter()
                        .filter_map(|i| i.fragment.as_deref())
                        .collect();
                    return Err(format!(
                        "{path} has no fragment {f} under the {} adapter; it has: {}",
                        kind.name(),
                        there.join(", ")
                    ));
                };
                (Some(f.to_owned()), item.hash.clone())
            }
        },
    };
    let mut out = json!({ "repo": repo, "rev": commit, "path": path });
    if let Some(f) = fragment {
        out["fragment"] = json!(f);
    }
    out["hash"] = json!(hash);
    out["hasher"] = json!(HASHER);
    Ok(out)
}

/// `L<n>` or `L<n>-L<m>`, 1-based and inclusive, `n <= m` (ADR-0057 d21).
fn line_span(fragment: &str) -> Option<(usize, usize)> {
    let rest = fragment.strip_prefix('L')?;
    let (n, m) = match rest.split_once("-L") {
        Some((n, m)) => (n.parse().ok()?, m.parse().ok()?),
        None => {
            let n = rest.parse().ok()?;
            (n, n)
        }
    };
    (n >= 1 && n <= m).then_some((n, m))
}
