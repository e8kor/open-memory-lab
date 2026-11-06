//! The tables under a lake root, each read at one version (ADR-0062 d2, d10).

use std::path::Path;

use oml_delta_log::{LogError, Snapshot};

/// One table, read or not.
#[derive(Debug)]
pub struct TableRead {
    /// The table's path under the lake root, `/`-separated.
    pub rel: String,
    pub outcome: Result<Snapshot, LogError>,
}

#[must_use]
pub fn read_lake(_root: &Path, _depth: usize, _at: Option<u64>) -> Vec<TableRead> {
    Vec::new()
}
