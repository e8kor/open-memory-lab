//! The tables under a lake root, each read at one version (ADR-0062 d2, d10).

use std::path::{Path, PathBuf};

use oml_delta_log::{DirStore, LogError, Snapshot, read_table};

/// One table, read or not.
#[derive(Debug)]
pub struct TableRead {
    /// The table's path under the lake root, `/`-separated.
    pub rel: String,
    pub outcome: Result<Snapshot, LogError>,
}

/// Every table under `root` to `depth` prefixes, found by its `_delta_log/`
/// child and read at `at` or at its newest version, in path order. A table is
/// not descended into: what lies under it is its data.
#[must_use]
pub fn read_lake(root: &Path, depth: usize, at: Option<u64>) -> Vec<TableRead> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    discover(root, "", depth, &mut found);
    found.sort();
    found
        .into_iter()
        .map(|(rel, dir)| TableRead {
            outcome: read_table(&DirStore::new(dir), at),
            rel,
        })
        .collect()
}

fn discover(dir: &Path, rel: &str, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }
        let path = entry.path();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let child = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if path.join("_delta_log").is_dir() {
            out.push((child, path));
        } else if child.matches('/').count() + 1 < depth {
            discover(&path, &child, depth, out);
        }
    }
}
