//! `oml-lake` — the Delta lake adapter (ADR-0003, ADR-0006 d2, ADR-0058).
//!
//! The lake is derived and never the authority: Postgres commits, the
//! outbox carries, and this crate writes what one writer drains into
//! per-tenant Delta tables (delta-tables.md "Per-tenant prefix layout").
//! Every commit carries the application transaction `oml-lakewriter` at the
//! batch's end seq; a table applies only rows past its own last version, so
//! a replayed or re-cut batch writes nothing twice (ADR-0058 d3). It reads
//! no Postgres and holds no policy: what to write is the writer's.
//!
//! No client reads the lake (ADR-0013 d1): the only reader in the tree is
//! [`query`], the operator's named queries run by `oml-lakewriter query`.

pub mod crypto;
pub mod query;
pub mod table;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deltalake::datafusion::prelude::{SessionContext, col};
use deltalake::kernel::Transaction;
use deltalake::kernel::transaction::CommitProperties;
use deltalake::operations::optimize::OptimizeType;
use deltalake::protocol::SaveMode;
use deltalake::{DeltaTable, DeltaTableError, TableProperty};
use uuid::Uuid;

use crate::table::{PROVIDER_PREFIX, Spec, V};

/// The application id every lake commit records (delta-tables.md "The
/// commit protocol, instantiated per table").
pub const APP_ID: &str = "oml-lakewriter";

/// Commits between checkpoints (delta-tables.md maintenance: "every 100
/// commits per table").
pub const CHECKPOINT_INTERVAL: &str = "100";

/// The lake erasure bound: deleted-file and log retention on every table
/// (delta-tables.md "The erasure-SLO interaction", default 7 days).
pub const ERASURE_BOUND: &str = "interval 7 days";

#[derive(Debug, thiserror::Error)]
pub enum LakeError {
    /// The lake URL names a scheme this build cannot write (ADR-0058 d15).
    #[error("lake.url scheme {0:?} is not supported by this build; use a file path or file:// URL")]
    Scheme(String),
    #[error("lake path: {0}")]
    Path(String),
    #[error("delta: {0}")]
    Delta(#[from] DeltaTableError),
    #[error("datafusion: {0}")]
    DataFusion(#[from] deltalake::datafusion::error::DataFusionError),
    #[error("rows: {0}")]
    Rows(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Whose prefix a table lives under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prefix {
    Tenant(Uuid),
    Provider,
}

/// The lake root: a directory on the local disk (ADR-0058 d15).
#[derive(Debug, Clone)]
pub struct Lake {
    root: PathBuf,
}

impl Lake {
    /// The lake at `url`: a bare absolute path or a `file://` URL. Another
    /// scheme is refused by name; the URL is never logged with userinfo.
    ///
    /// # Errors
    /// [`LakeError::Scheme`] for a scheme this build cannot write.
    pub fn at(url: &str) -> Result<Self, LakeError> {
        let root = if let Some(rest) = url.strip_prefix("file://") {
            PathBuf::from(rest)
        } else if url.contains("://") {
            let scheme = url.split("://").next().unwrap_or_default();
            return Err(LakeError::Scheme(scheme.to_owned()));
        } else {
            PathBuf::from(url)
        };
        if !root.is_absolute() {
            return Err(LakeError::Path(format!(
                "{} is not absolute",
                root.display()
            )));
        }
        Ok(Self { root })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory of `spec` under `prefix`.
    #[must_use]
    pub fn dir(&self, prefix: Prefix, spec: &Spec) -> PathBuf {
        let p = match prefix {
            Prefix::Tenant(t) => self.root.join(t.to_string()),
            Prefix::Provider => self.root.join(PROVIDER_PREFIX),
        };
        p.join(spec.path)
    }

    /// The tenants that have a prefix: every directory under the root whose
    /// name is a UUID.
    ///
    /// # Errors
    /// The root cannot be listed.
    pub fn tenants(&self) -> Result<Vec<Uuid>, LakeError> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|n| Uuid::parse_str(n).ok())
            {
                out.push(id);
            }
        }
        out.sort();
        Ok(out)
    }

    /// The table, if it exists.
    ///
    /// # Errors
    /// The log cannot be read.
    pub async fn open(&self, prefix: Prefix, spec: &Spec) -> Result<Option<DeltaTable>, LakeError> {
        let dir = self.dir(prefix, spec);
        if !dir.join("_delta_log").exists() {
            return Ok(None);
        }
        let table = DeltaTable::try_from_url(dir_url(&dir)?).await?;
        Ok(table.version().map(|_| table))
    }

    /// The table, created with its columns and properties when absent.
    ///
    /// # Errors
    /// The directory or the first commit cannot be written.
    pub async fn open_or_create(
        &self,
        prefix: Prefix,
        spec: &Spec,
    ) -> Result<DeltaTable, LakeError> {
        if let Some(t) = self.open(prefix, spec).await? {
            return Ok(t);
        }
        let dir = self.dir(prefix, spec);
        std::fs::create_dir_all(&dir)?;
        let table = DeltaTable::try_from_url(dir_url(&dir)?).await?;
        let mut create = table
            .create()
            .with_columns(spec.delta_columns())
            .with_table_name(spec.name)
            // No setTransactionRetentionDuration: an expired txn would reset
            // recovery to "never applied" (ADR-0003 consequences, FR-16).
            .with_configuration_property(
                TableProperty::CheckpointInterval,
                Some(CHECKPOINT_INTERVAL),
            )
            .with_configuration_property(TableProperty::LogRetentionDuration, Some(ERASURE_BOUND))
            .with_configuration_property(
                TableProperty::DeletedFileRetentionDuration,
                Some(ERASURE_BOUND),
            );
        if let Some(p) = spec.partition {
            create = create.with_partition_columns([p]);
        }
        Ok(create.await?)
    }

    /// The last application transaction version `oml-lakewriter` recorded
    /// on the table; 0 when the table or the transaction does not exist
    /// (delta-tables.md: "a table that does not yet exist is treated as
    /// version 0").
    ///
    /// # Errors
    /// The log cannot be read.
    pub async fn applied(&self, prefix: Prefix, spec: &Spec) -> Result<i64, LakeError> {
        match self.open(prefix, spec).await? {
            None => Ok(0),
            Some(t) => txn_version(&t).await,
        }
    }

    /// Appends `rows` with the transaction version `version`; nothing when
    /// there are no rows.
    ///
    /// # Errors
    /// The rows do not fit the table or the commit fails.
    pub async fn append(
        &self,
        prefix: Prefix,
        spec: &Spec,
        rows: &[Vec<V>],
        version: i64,
    ) -> Result<usize, LakeError> {
        if rows.is_empty() {
            return Ok(0);
        }
        let table = self.open_or_create(prefix, spec).await?;
        let batch = spec.batch(rows).map_err(LakeError::Rows)?;
        table
            .write(vec![batch])
            .with_save_mode(SaveMode::Append)
            .with_commit_properties(commit(version))
            .await?;
        Ok(rows.len())
    }

    /// Upserts `rows` on the table's merge keys; a matched row is replaced
    /// only by a row with a greater `applied_seq` when the table has that
    /// column, so an older operation never overwrites a newer one
    /// (delta-tables.md per-row frontier column).
    ///
    /// # Errors
    /// The rows do not fit the table or the commit fails.
    pub async fn merge(
        &self,
        prefix: Prefix,
        spec: &Spec,
        rows: &[Vec<V>],
        version: i64,
    ) -> Result<usize, LakeError> {
        if rows.is_empty() {
            return Ok(0);
        }
        let table = self.open_or_create(prefix, spec).await?;
        let batch = spec.batch(rows).map_err(LakeError::Rows)?;
        let ctx = SessionContext::new();
        let source = ctx.read_batch(batch)?;
        let mut on = None;
        for k in spec.merge_keys {
            let eq = col(format!("t.{k}")).eq(col(format!("s.{k}")));
            on = Some(match on {
                None => eq,
                Some(prev) => deltalake::datafusion::prelude::Expr::and(prev, eq),
            });
        }
        let on = on.ok_or_else(|| LakeError::Rows(format!("{} has no merge keys", spec.name)))?;
        let guarded = spec.columns.iter().any(|(n, _, _)| *n == "applied_seq");
        let columns: Vec<&str> = spec.columns.iter().map(|(n, _, _)| *n).collect();
        let keys = spec.merge_keys;
        let mut builder = table
            .merge(source, on)
            .with_source_alias("s")
            .with_target_alias("t")
            .with_commit_properties(commit(version));
        if guarded {
            builder = builder.when_matched_update(|u| {
                let mut u = u.predicate(col("s.applied_seq").gt(col("t.applied_seq")));
                for c in &columns {
                    if !keys.contains(c) {
                        u = u.update(*c, col(format!("s.{c}")));
                    }
                }
                u
            })?;
        }
        builder = builder.when_not_matched_insert(|i| {
            let mut i = i;
            for c in &columns {
                i = i.set(*c, col(format!("s.{c}")));
            }
            i
        })?;
        builder.await?;
        Ok(rows.len())
    }

    /// Replaces the whole table with `rows`: the snapshot tables, whose
    /// source of truth is recomputed whole each time (ADR-0058 d7).
    ///
    /// # Errors
    /// The rows do not fit the table or the commit fails.
    pub async fn replace(
        &self,
        prefix: Prefix,
        spec: &Spec,
        rows: &[Vec<V>],
    ) -> Result<usize, LakeError> {
        let table = self.open_or_create(prefix, spec).await?;
        let batch = spec.batch(rows).map_err(LakeError::Rows)?;
        table
            .write(vec![batch])
            .with_save_mode(SaveMode::Overwrite)
            .await?;
        Ok(rows.len())
    }

    /// Every table that exists, with its prefix.
    ///
    /// # Errors
    /// The root cannot be listed.
    pub fn existing(&self) -> Result<Vec<(Prefix, &'static Spec)>, LakeError> {
        let mut out = Vec::new();
        for t in self.tenants()? {
            for spec in table::TENANT_TABLES {
                if self
                    .dir(Prefix::Tenant(t), spec)
                    .join("_delta_log")
                    .exists()
                {
                    out.push((Prefix::Tenant(t), spec));
                }
            }
        }
        for spec in table::PROVIDER_TABLES {
            if self.dir(Prefix::Provider, spec).join("_delta_log").exists() {
                out.push((Prefix::Provider, spec));
            }
        }
        Ok(out)
    }

    /// The columns of an existing table that differ from its spec, by name;
    /// empty when the table matches or does not exist (the doctor's schema
    /// drift line, ADR-0058 d16).
    ///
    /// # Errors
    /// The log cannot be read.
    pub async fn drift(&self, prefix: Prefix, spec: &Spec) -> Result<Vec<String>, LakeError> {
        let Some(t) = self.open(prefix, spec).await? else {
            return Ok(Vec::new());
        };
        let schema = t.snapshot()?.schema();
        let have: HashMap<String, String> = schema
            .fields()
            .map(|f| (f.name().clone(), format!("{:?}", f.data_type())))
            .collect();
        let want: HashMap<String, String> = spec
            .delta_columns()
            .iter()
            .map(|f| (f.name().clone(), format!("{:?}", f.data_type())))
            .collect();
        let mut out: Vec<String> = want
            .iter()
            .filter(|(n, ty)| have.get(*n) != Some(ty))
            .map(|(n, _)| n.clone())
            .chain(have.keys().filter(|n| !want.contains_key(*n)).cloned())
            .collect();
        out.sort();
        Ok(out)
    }

    /// One maintenance pass over one table: bin-pack OPTIMIZE, a checkpoint,
    /// VACUUM of files unreferenced for longer than `retention`, and expired
    /// log cleanup (delta-tables.md maintenance; ADR-0006 d3; ADR-0058 d12).
    ///
    /// # Errors
    /// Any step fails; the steps before it stand.
    pub async fn maintain(
        &self,
        prefix: Prefix,
        spec: &Spec,
        retention: chrono::Duration,
    ) -> Result<Maintained, LakeError> {
        let Some(table) = self.open(prefix, spec).await? else {
            return Ok(Maintained::default());
        };
        let (table, metrics) = table.optimize().with_type(OptimizeType::Compact).await?;
        deltalake::checkpoints::create_checkpoint(&table, None).await?;
        let logs_removed = deltalake::checkpoints::cleanup_metadata(&table, None).await?;
        let (_table, vacuumed) = table
            .vacuum()
            .with_retention_period(retention)
            .with_enforce_retention_duration(false)
            .await?;
        Ok(Maintained {
            files_compacted: metrics.num_files_removed,
            files_written: metrics.num_files_added,
            files_vacuumed: vacuumed.files_deleted.len() as u64,
            logs_removed: logs_removed as u64,
        })
    }

    /// Every file under the lake root, for the erase and isolation laws and
    /// the doctor's size line.
    ///
    /// # Errors
    /// A directory cannot be listed.
    pub fn files(&self) -> Result<Vec<PathBuf>, LakeError> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
            for e in std::fs::read_dir(dir)? {
                let p = e?.path();
                if p.is_dir() {
                    walk(&p, out)?;
                } else {
                    out.push(p);
                }
            }
            Ok(())
        }
        let mut out = Vec::new();
        if self.root.exists() {
            walk(&self.root, &mut out)?;
        }
        Ok(out)
    }
}

/// What one maintenance pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Maintained {
    pub files_compacted: u64,
    pub files_written: u64,
    pub files_vacuumed: u64,
    pub logs_removed: u64,
}

/// The table's `oml-lakewriter` transaction version, 0 when none.
///
/// # Errors
/// The log cannot be read.
pub async fn txn_version(table: &DeltaTable) -> Result<i64, LakeError> {
    Ok(table
        .snapshot()?
        .transaction_version(table.log_store().as_ref(), APP_ID)
        .await?
        .unwrap_or(0))
}

fn commit(version: i64) -> CommitProperties {
    CommitProperties::default().with_application_transaction(Transaction::new(APP_ID, version))
}

fn dir_url(dir: &Path) -> Result<url::Url, LakeError> {
    url::Url::from_directory_path(dir)
        .map_err(|()| LakeError::Path(format!("{} is not an absolute directory", dir.display())))
}

/// Days since the epoch of a UTC instant in microseconds: a date partition.
#[must_use]
pub fn micros_to_date(micros: i64) -> i32 {
    i32::try_from(micros.div_euclid(86_400_000_000)).unwrap_or(i32::MAX)
}

/// The shared reference count type DataFusion registers tables under.
pub type Provider = Arc<dyn deltalake::datafusion::datasource::TableProvider>;
