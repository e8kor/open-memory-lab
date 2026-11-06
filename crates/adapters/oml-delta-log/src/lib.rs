//! The minimal Delta log reader (ADR-0062 d2-d4).

use std::collections::BTreeMap;


/// Why a table could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    /// The log holds something this reader does not know; named.
    Unsupported(String),
    /// The log cannot be read as it stands; named.
    Unreadable(String),
}

/// The log files of one table, by name under `_delta_log/`.
pub trait LogStore {
    /// # Errors
    /// The listing fails.
    fn list(&self) -> Result<Vec<String>, LogError>;
    /// # Errors
    /// The file is absent or unreadable.
    fn len(&self, name: &str) -> Result<u64, LogError>;
    /// # Errors
    /// As [`LogStore::len`].
    fn get(&self, name: &str) -> Result<Vec<u8>, LogError>;
    /// # Errors
    /// As [`LogStore::len`].
    fn get_range(&self, name: &str, start: u64, len: u64) -> Result<Vec<u8>, LogError>;
}

/// One field of a schema, as `schemaString` states it.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub data_type: serde_json::Value,
    pub nullable: bool,
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

/// A table at one version.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub version: u64,
    pub table_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub schema: Vec<Field>,
    pub partition_columns: Vec<String>,
    pub clustering_columns: Vec<Vec<String>>,
    /// The d9 keys alone.
    pub configuration: BTreeMap<String, String>,
    pub reader_features: Vec<String>,
    pub writer_features: Vec<String>,
}

impl Snapshot {
    /// The schema as a Delta struct type.
    #[must_use]
    pub fn schema_json(&self) -> serde_json::Value {
        serde_json::json!({"type": "struct", "fields": []})
    }
}

/// The log of a table on a local disk: it opens a file directly under
/// `<root>/_delta_log/` and nothing else.
#[derive(Debug, Clone)]
pub struct DirStore {
    #[allow(dead_code, reason = "read by the reader that follows this red commit")]
    root: std::path::PathBuf,
}

impl DirStore {
    #[must_use]
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl LogStore for DirStore {
    fn list(&self) -> Result<Vec<String>, LogError> {
        Err(LogError::Unreadable("not built".into()))
    }
    fn len(&self, _name: &str) -> Result<u64, LogError> {
        Err(LogError::Unreadable("not built".into()))
    }
    fn get(&self, _name: &str) -> Result<Vec<u8>, LogError> {
        Err(LogError::Unreadable("not built".into()))
    }
    fn get_range(&self, _name: &str, _start: u64, _len: u64) -> Result<Vec<u8>, LogError> {
        Err(LogError::Unreadable("not built".into()))
    }
}

/// # Errors
/// [`LogError`].
pub fn read_table(_store: &dyn LogStore, _at: Option<u64>) -> Result<Snapshot, LogError> {
    Err(LogError::Unreadable("the reader is not built".into()))
}
