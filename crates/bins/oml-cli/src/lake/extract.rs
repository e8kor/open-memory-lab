//! A table's claims (ADR-0062 d6, d7).

use oml_delta_log::Snapshot;

/// One claim a table states.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LakeItem {
    pub claim_key: String,
    pub fragment: Option<String>,
    pub text: String,
    pub hash: String,
    pub hasher: &'static str,
    /// The table's path under the lake root.
    pub rel: String,
}

#[must_use]
pub fn items(_rel: &str, _table_uri: &str, _snapshot: &Snapshot) -> Vec<LakeItem> {
    Vec::new()
}
