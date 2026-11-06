//! The minimal Delta log reader (ADR-0062 d2-d4): a pure function from one
//! table's `_delta_log` files to its version, protocol features, metadata,
//! schema, partitioning and clustering.
//!
//! It decodes four things and no other: `protocol`, `metaData`, the
//! `delta.clustering` `domainMetadata` and the version number. An `add`,
//! `remove`, `cdc`, `txn`, `sidecar` or `commitInfo` is skipped as JSON
//! without being decoded, and a checkpoint's parquet is read by column
//! projection on the three columns above, so a file path, a partition value,
//! a statistic, a row count or a user name never reaches memory. It opens only
//! files directly under `_delta_log/` through [`LogStore`], and never a data
//! file. A configuration key outside [`CONFIG_KEYS`] is dropped before a
//! [`Snapshot`] holds it, since engines leave storage options and secrets
//! there.

mod checkpoint;
mod commit;
mod store;

use std::collections::BTreeMap;

use serde_json::Value;

pub use store::DirStore;

/// Why a table could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    /// The log holds something this reader does not know, named; the table's
    /// claims are neither written nor retired (ADR-0062 d4).
    Unsupported(String),
    /// The log cannot be read as it stands, with the reason; never `removed`.
    Unreadable(String),
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(w) => write!(f, "unsupported: {w}"),
            Self::Unreadable(w) => write!(f, "unreadable: {w}"),
        }
    }
}

impl std::error::Error for LogError {}

pub(crate) fn unreadable(why: impl Into<String>) -> LogError {
    LogError::Unreadable(why.into())
}

/// The files of one table's `_delta_log/`, by name. A name is one path
/// segment; a store refuses any other, so nothing outside the log is opened.
pub trait LogStore {
    /// The names of the files directly under `_delta_log/`.
    ///
    /// # Errors
    /// The listing fails.
    fn list(&self) -> Result<Vec<String>, LogError>;
    /// A file's length in bytes.
    ///
    /// # Errors
    /// The file is absent, unreadable or not a bare name.
    fn len(&self, name: &str) -> Result<u64, LogError>;
    /// A whole file.
    ///
    /// # Errors
    /// As [`LogStore::len`].
    fn get(&self, name: &str) -> Result<Vec<u8>, LogError>;
    /// `len` bytes of a file from `start`, fewer at its end.
    ///
    /// # Errors
    /// As [`LogStore::len`].
    fn get_range(&self, name: &str, start: u64, len: u64) -> Result<Vec<u8>, LogError>;
}

/// The only `metaData.configuration` keys the reader keeps (ADR-0062 d9): the
/// column mapping mode and the three `oml.` properties.
pub const CONFIG_KEYS: [&str; 4] = [
    "delta.columnMapping.mode",
    "oml.owner",
    "oml.grain",
    "oml.freshness",
];

/// The table features the reader accepts, each one that changes no part of
/// the log it reads, confirmed against the protocol's "Valid Feature Names"
/// and the sections that name them (ADR-0062 d4).
pub const ALLOWED_FEATURES: [&str; 19] = [
    "columnMapping",
    "deletionVectors",
    "timestampNtz",
    "typeWidening",
    "v2Checkpoint",
    "domainMetadata",
    "clustering",
    "inCommitTimestamp",
    "appendOnly",
    "invariants",
    "checkConstraints",
    "changeDataFeed",
    "generatedColumns",
    "identityColumns",
    "rowTracking",
    "vacuumProtocolCheck",
    "variantType",
    "icebergCompatV1",
    "icebergCompatV2",
];

/// One field of a schema, as `schemaString` states it.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    /// A type name (`"long"`, `"decimal(18,2)"`) or a type object (a struct,
    /// array or map).
    pub data_type: Value,
    pub nullable: bool,
    /// `comment`, `delta.columnMapping.id`, `delta.columnMapping.physicalName`
    /// and whatever else the writer set.
    pub metadata: serde_json::Map<String, Value>,
}

/// A table at one version.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub version: u64,
    /// `metaData.id`: it changes when a table is recreated.
    pub table_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub schema: Vec<Field>,
    /// Logical names, in order.
    pub partition_columns: Vec<String>,
    /// Each clustering column as a name path; physical names when column
    /// mapping is on (the protocol's "Clustered Table").
    pub clustering_columns: Vec<Vec<String>>,
    /// The [`CONFIG_KEYS`] the table sets, and no other.
    pub configuration: BTreeMap<String, String>,
    pub reader_features: Vec<String>,
    pub writer_features: Vec<String>,
}

impl Snapshot {
    /// The schema as a Delta struct type.
    #[must_use]
    pub fn schema_json(&self) -> Value {
        serde_json::json!({
            "type": "struct",
            "fields": self.schema.iter().map(field_json).collect::<Vec<_>>(),
        })
    }
}

fn field_json(f: &Field) -> Value {
    serde_json::json!({
        "name": f.name,
        "type": f.data_type,
        "nullable": f.nullable,
        "metadata": f.metadata,
    })
}

/// The protocol, as far as the reader checks it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Protocol {
    pub min_reader: u64,
    pub reader_features: Vec<String>,
    pub writer_features: Vec<String>,
}

/// The metadata the reader keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Meta {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub schema_string: String,
    pub partition_columns: Vec<String>,
    pub configuration: BTreeMap<String, String>,
}

/// What a replay of checkpoint and commits has settled.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub protocol: Option<Protocol>,
    pub meta: Option<Meta>,
    /// The `delta.clustering` domain's configuration string.
    pub clustering: Option<String>,
}

/// A log file's kind and version, by its name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Commit,
    Classic,
    /// `<v>.checkpoint.<part>.<parts>.parquet`
    Multi {
        part: u64,
        parts: u64,
    },
    /// `<v>.checkpoint.<uuid>.json|parquet`
    V2,
}

/// The longest log file name the reader looks at; any other is not one of the
/// protocol's, whatever it starts with.
const NAME_MAX: usize = 255;

/// A log file's kind and version, by its name. A number that does not fit a
/// `u64` makes the name no log file's: nothing is sized from a name.
fn classify(name: &str) -> Option<(u64, Kind)> {
    if name.len() > NAME_MAX {
        return None;
    }
    let (v, rest) = name.split_once('.')?;
    if v.len() != 20 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let version: u64 = v.parse().ok()?;
    if rest == "json" {
        return Some((version, Kind::Commit));
    }
    let rest = rest.strip_prefix("checkpoint.")?;
    if rest == "parquet" {
        return Some((version, Kind::Classic));
    }
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let parts: Vec<&str> = rest.split('.').collect();
    if let [p, n, "parquet"] = parts[..]
        && digits(p)
        && digits(n)
    {
        return Some((
            version,
            Kind::Multi {
                part: p.parse().ok()?,
                parts: n.parse().ok()?,
            },
        ));
    }
    if let [id, ext] = parts[..]
        && !id.is_empty()
        && matches!(ext, "json" | "parquet")
    {
        return Some((version, Kind::V2));
    }
    None
}

/// The checkpoint files of one version, when the version has a whole one.
fn checkpoint_files(version: u64, files: &[(String, Kind)]) -> Option<Vec<String>> {
    if let Some((n, _)) = files.iter().find(|(_, k)| *k == Kind::Classic) {
        return Some(vec![n.clone()]);
    }
    if let Some((n, _)) = files.iter().find(|(_, k)| *k == Kind::V2) {
        return Some(vec![n.clone()]);
    }
    let multi: Vec<(&String, u64, u64)> = files
        .iter()
        .filter_map(|(n, k)| match k {
            Kind::Multi { part, parts } => Some((n, *part, *parts)),
            _ => None,
        })
        .collect();
    // Keyed by part, so nothing is allocated from the `parts` a name states: a
    // checkpoint is whole when the parts present are exactly 1 to `parts`.
    let parts = multi.first()?.2;
    let mut named: BTreeMap<u64, &String> = BTreeMap::new();
    for (n, p, total) in multi {
        if total != parts || p == 0 || p > parts {
            return None;
        }
        named.insert(p, n);
    }
    let _ = version;
    if u64::try_from(named.len()).ok()? != parts {
        return None;
    }
    Some(named.into_values().cloned().collect())
}

/// Reads a table at `at`, or at its newest version.
///
/// It lists `_delta_log/`, takes the newest whole checkpoint at or below the
/// version and the JSON commits after it, and replays them.
///
/// # Errors
/// [`LogError::Unsupported`] for a protocol feature or checkpoint codec the
/// reader does not know or a catalog-managed table;
/// [`LogError::Unreadable`] for a gap in the log, a `_last_checkpoint` that
/// names a missing file, a file that does not parse, or a version the log does
/// not reach.
pub fn read_table(store: &dyn LogStore, at: Option<u64>) -> Result<Snapshot, LogError> {
    read_table_with(store, at, &Limits::default())
}

/// The bounds a read keeps, each refused by name as `unreadable` (ADR-0062 d3).
#[derive(Debug, Clone)]
pub struct Limits {
    /// Bytes of one JSON commit (or JSON checkpoint) file.
    pub commit_bytes: u64,
    /// Bytes of one line of one.
    pub commit_line: usize,
    /// Bytes of a checkpoint's parquet footer.
    pub footer_bytes: u64,
    /// Bytes of a checkpoint's projected column chunks, compressed and
    /// uncompressed, summed.
    pub checkpoint_bytes: u64,
    /// Fields of a schema, nested ones included.
    pub columns: usize,
    /// Files directly under `_delta_log/`.
    pub files: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            commit_bytes: 64 << 20,
            commit_line: 16 << 20,
            footer_bytes: 32 << 20,
            checkpoint_bytes: 256 << 20,
            columns: 20_000,
            files: 100_000,
        }
    }
}

/// As [`read_table`], under `limits`.
///
/// # Errors
/// As [`read_table`], and [`LogError::Unreadable`] naming the bound a file
/// or a listing is over.
pub fn read_table_with(
    store: &dyn LogStore,
    at: Option<u64>,
    limits: &Limits,
) -> Result<Snapshot, LogError> {
    let mut commits: BTreeMap<u64, String> = BTreeMap::new();
    let mut checkpoints: BTreeMap<u64, Vec<(String, Kind)>> = BTreeMap::new();
    let mut hint = false;
    let names = store.list()?;
    if names.len() > limits.files {
        return Err(unreadable(format!(
            "_delta_log holds {} files, over the {}-file cap",
            names.len(),
            limits.files
        )));
    }
    for name in names {
        if name == "_last_checkpoint" {
            hint = true;
        } else if let Some((v, kind)) = classify(&name) {
            match kind {
                Kind::Commit => {
                    commits.insert(v, name);
                }
                other => checkpoints.entry(v).or_default().push((name, other)),
            }
        }
    }
    let whole: BTreeMap<u64, Vec<String>> = checkpoints
        .iter()
        .filter_map(|(v, files)| Some((*v, checkpoint_files(*v, files)?)))
        .collect();
    let newest = commits.keys().chain(whole.keys()).max().copied();
    let target = match (at, newest) {
        (Some(v), Some(n)) if v <= n => v,
        (Some(v), Some(n)) => {
            return Err(unreadable(format!(
                "version {v} is past the newest the log holds, {n}"
            )));
        }
        (None, Some(n)) => n,
        (_, None) => return Err(unreadable("the log holds no commit and no checkpoint")),
    };
    if hint {
        let named = commit::last_checkpoint(&fetch(store, "_last_checkpoint", limits)?)?;
        if named <= target && !whole.contains_key(&named) {
            return Err(unreadable(format!(
                "_last_checkpoint names version {named}, whose checkpoint is not in the log"
            )));
        }
    }
    let base = whole.range(..=target).next_back();
    // A checkpoint at the top of u64 has no commit after it.
    let first_commit = base.map_or(Some(0), |(v, _)| v.checked_add(1));
    let mut state = State::default();
    if let Some((v, files)) = base {
        for f in files {
            // The parquet crate asserts on some corrupt files instead of
            // returning an error; that is this table's `unreadable`, not the
            // run's end.
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                checkpoint::replay(store, f, &mut state, limits)
            }))
            .unwrap_or_else(|_| Err(unreadable(format!("the reader panicked on {f}"))));
            read.map_err(|e| match e {
                LogError::Unreadable(w) => unreadable(format!("checkpoint {v}: {w}")),
                other => other,
            })?;
        }
    }
    for v in first_commit.map_or(1..=0, |first| first..=target) {
        let Some(name) = commits.get(&v) else {
            return Err(unreadable(format!(
                "the log has no commit {v:020}.json between {} and {target}",
                base.map_or_else(
                    || "version 0".to_owned(),
                    |(c, _)| format!("checkpoint {c}")
                )
            )));
        };
        commit::replay(&fetch(store, name, limits)?, &mut state, limits)
            .map_err(|e| unreadable(format!("{name}: {e}")))?;
    }
    finish(target, state, limits)
}

/// A JSON log file, once its length is within the commit cap.
pub(crate) fn fetch(
    store: &dyn LogStore,
    name: &str,
    limits: &Limits,
) -> Result<Vec<u8>, LogError> {
    let len = store.len(name)?;
    if len > limits.commit_bytes {
        return Err(unreadable(format!(
            "{name} is {len} bytes, over the {}-byte commit cap",
            limits.commit_bytes
        )));
    }
    store.get(name)
}

fn finish(version: u64, state: State, limits: &Limits) -> Result<Snapshot, LogError> {
    let Some(protocol) = state.protocol else {
        return Err(unreadable(format!(
            "no protocol action up to version {version}"
        )));
    };
    check_protocol(&protocol)?;
    let Some(meta) = state.meta else {
        return Err(unreadable(format!(
            "no metaData action up to version {version}"
        )));
    };
    let schema = commit::parse_schema(&meta.schema_string, limits.columns)
        .map_err(|e| unreadable(format!("the schema at version {version}: {e}")))?;
    let clustering_columns = match state.clustering {
        None => Vec::new(),
        Some(c) => commit::clustering_columns(&c)
            .map_err(|e| unreadable(format!("the clustering domain at version {version}: {e}")))?,
    };
    Ok(Snapshot {
        version,
        table_id: meta.id,
        name: meta.name,
        description: meta.description,
        schema,
        partition_columns: meta.partition_columns,
        clustering_columns,
        configuration: meta.configuration,
        reader_features: protocol.reader_features,
        writer_features: protocol.writer_features,
    })
}

fn check_protocol(p: &Protocol) -> Result<(), LogError> {
    for f in p.reader_features.iter().chain(&p.writer_features) {
        if f == "catalogManaged" {
            return Err(LogError::Unsupported(
                "catalogManaged: the table's commits are not in _delta_log alone".into(),
            ));
        }
    }
    if p.min_reader > 3 {
        return Err(LogError::Unsupported(format!(
            "minReaderVersion {}",
            p.min_reader
        )));
    }
    for f in p.reader_features.iter().chain(&p.writer_features) {
        if !ALLOWED_FEATURES.contains(&f.as_str()) {
            return Err(LogError::Unsupported(format!("table feature {f}")));
        }
    }
    Ok(())
}

/// Whether `key` is one the reader keeps.
pub(crate) fn kept(key: &str) -> bool {
    CONFIG_KEYS.contains(&key)
}
