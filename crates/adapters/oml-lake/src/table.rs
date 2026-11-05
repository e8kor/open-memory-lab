//! The lake's tables, column by column (docs/data/delta-tables.md as
//! instantiated by ADR-0058), and the generic row-to-Arrow bridge every
//! table writes through.

use std::sync::Arc;

use deltalake::arrow::array::{
    ArrayRef, BinaryArray, BooleanArray, Date32Array, Float64Array, Int64Array, StringArray,
    TimestampMicrosecondArray,
};
use deltalake::arrow::datatypes::{DataType as ArrowType, Field, Schema, SchemaRef, TimeUnit};
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::kernel::{DataType, PrimitiveType, StructField};

/// A column's type: the Delta and Arrow spellings follow from it
/// (delta-tables.md "Type conventions").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Str,
    Long,
    Bin,
    /// Microseconds since the epoch, UTC.
    Ts,
    /// Days since the epoch.
    Date,
    Bool,
    F64,
}

impl Col {
    fn delta(self) -> DataType {
        DataType::Primitive(match self {
            Self::Str => PrimitiveType::String,
            Self::Long => PrimitiveType::Long,
            Self::Bin => PrimitiveType::Binary,
            Self::Ts => PrimitiveType::Timestamp,
            Self::Date => PrimitiveType::Date,
            Self::Bool => PrimitiveType::Boolean,
            Self::F64 => PrimitiveType::Double,
        })
    }

    fn arrow(self) -> ArrowType {
        match self {
            Self::Str => ArrowType::Utf8,
            Self::Long => ArrowType::Int64,
            Self::Bin => ArrowType::Binary,
            Self::Ts => ArrowType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            Self::Date => ArrowType::Date32,
            Self::Bool => ArrowType::Boolean,
            Self::F64 => ArrowType::Float64,
        }
    }
}

/// One cell. A row is a `Vec<V>` in its table's column order.
#[derive(Debug, Clone, PartialEq)]
pub enum V {
    Str(Option<String>),
    Long(Option<i64>),
    Bin(Option<Vec<u8>>),
    Ts(Option<i64>),
    Date(Option<i32>),
    Bool(Option<bool>),
    F64(Option<f64>),
}

/// A lake table: where it lives under a tenant's prefix (or the provider's),
/// its columns, its partition column and the keys a MERGE matches on.
#[derive(Debug)]
pub struct Spec {
    /// The name the doctor, the metrics and `lake_dead_letter` use.
    pub name: &'static str,
    /// The path under the prefix, `/`-separated.
    pub path: &'static str,
    pub columns: &'static [(&'static str, Col, bool)],
    pub partition: Option<&'static str>,
    pub merge_keys: &'static [&'static str],
}

impl Spec {
    #[must_use]
    pub fn delta_columns(&self) -> Vec<StructField> {
        self.columns
            .iter()
            .map(|(n, c, null)| StructField::new(*n, c.delta(), *null))
            .collect()
    }

    #[must_use]
    pub fn arrow_schema(&self) -> SchemaRef {
        Arc::new(Schema::new(
            self.columns
                .iter()
                .map(|(n, c, null)| Field::new(*n, c.arrow(), *null))
                .collect::<Vec<_>>(),
        ))
    }

    /// The rows as one batch in this table's schema.
    ///
    /// # Errors
    /// A row whose length or cell kinds do not match the columns.
    pub fn batch(&self, rows: &[Vec<V>]) -> Result<RecordBatch, String> {
        let mut arrays: Vec<ArrayRef> = Vec::with_capacity(self.columns.len());
        for (i, (name, col, _)) in self.columns.iter().enumerate() {
            let cell = |r: &Vec<V>| r.get(i).cloned().ok_or_else(|| format!("row lacks {name}"));
            let mismatch = || format!("column {name} is not {col:?}");
            let array: ArrayRef = match col {
                Col::Str => Arc::new(StringArray::from(
                    rows.iter()
                        .map(|r| match cell(r)? {
                            V::Str(v) => Ok(v),
                            _ => Err(mismatch()),
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                )),
                Col::Long => Arc::new(Int64Array::from(
                    rows.iter()
                        .map(|r| match cell(r)? {
                            V::Long(v) => Ok(v),
                            _ => Err(mismatch()),
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                )),
                Col::Bin => {
                    let vals = rows
                        .iter()
                        .map(|r| match cell(r)? {
                            V::Bin(v) => Ok(v),
                            _ => Err(mismatch()),
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    Arc::new(BinaryArray::from(
                        vals.iter().map(Option::as_deref).collect::<Vec<_>>(),
                    ))
                }
                Col::Ts => Arc::new(
                    TimestampMicrosecondArray::from(
                        rows.iter()
                            .map(|r| match cell(r)? {
                                V::Ts(v) => Ok(v),
                                _ => Err(mismatch()),
                            })
                            .collect::<Result<Vec<_>, String>>()?,
                    )
                    .with_timezone("UTC"),
                ),
                Col::Date => Arc::new(Date32Array::from(
                    rows.iter()
                        .map(|r| match cell(r)? {
                            V::Date(v) => Ok(v),
                            _ => Err(mismatch()),
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                )),
                Col::Bool => Arc::new(BooleanArray::from(
                    rows.iter()
                        .map(|r| match cell(r)? {
                            V::Bool(v) => Ok(v),
                            _ => Err(mismatch()),
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                )),
                Col::F64 => Arc::new(Float64Array::from(
                    rows.iter()
                        .map(|r| match cell(r)? {
                            V::F64(v) => Ok(v),
                            _ => Err(mismatch()),
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                )),
            };
            arrays.push(array);
        }
        RecordBatch::try_new(self.arrow_schema(), arrays).map_err(|e| e.to_string())
    }
}

/// `bronze.events`: the digest-only mirror of the journal, one row per
/// event, append only (delta-tables.md; ADR-0003 d3).
pub const BRONZE_EVENTS: Spec = Spec {
    name: "bronze.events",
    path: "bronze/events",
    columns: &[
        ("tenant_id", Col::Str, false),
        ("journal_seq", Col::Long, false),
        ("event_id", Col::Str, false),
        ("event_type", Col::Str, false),
        ("at", Col::Ts, false),
        ("event_date", Col::Date, false),
        ("prev_hash", Col::Bin, false),
        ("hash", Col::Bin, false),
        ("sig", Col::Bin, false),
        ("signed_bytes", Col::Bin, false),
        ("payload", Col::Str, false),
        ("principal", Col::Str, false),
        ("api_key_id", Col::Str, true),
        ("artifact_id", Col::Str, true),
        ("version", Col::Long, true),
        ("workarea_id", Col::Str, true),
        ("kind", Col::Str, true),
        ("subject_name", Col::Str, true),
        ("same_user", Col::Bool, true),
        ("outbox_seq", Col::Long, false),
    ],
    partition: Some("event_date"),
    merge_keys: &[],
};

/// `silver.claims`: one row per materialized claim version, the body sealed
/// under the version's own data key (delta-tables.md; ADR-0058 d2, d6).
pub const SILVER_CLAIMS: Spec = Spec {
    name: "silver.claims",
    path: "silver/claims",
    columns: &[
        ("tenant_id", Col::Str, false),
        ("artifact_id", Col::Str, false),
        ("version", Col::Long, false),
        ("claim_key", Col::Str, false),
        ("kind", Col::Str, false),
        ("state", Col::Str, false),
        ("workarea_id", Col::Str, false),
        ("writer_identity", Col::Str, false),
        ("principal", Col::Str, false),
        ("same_user", Col::Bool, true),
        ("retirement", Col::Bool, false),
        ("replaces_artifact_id", Col::Str, true),
        ("plaintext_digest", Col::Str, false),
        ("ciphertext_digest", Col::Str, true),
        ("body_ciphertext", Col::Bin, true),
        ("body_ref", Col::Str, false),
        ("kek_id", Col::Str, true),
        ("body_bytes", Col::Long, false),
        ("created_at", Col::Ts, false),
        ("promoted_at", Col::Ts, false),
        ("retracted_at", Col::Ts, true),
        ("applied_seq", Col::Long, false),
    ],
    partition: None,
    merge_keys: &["artifact_id"],
};

/// `gold.lineage`: ratified edges and the materialized versions' own
/// derives-from and supersedes links (delta-tables.md; ADR-0058 d9).
pub const GOLD_LINEAGE: Spec = Spec {
    name: "gold.lineage",
    path: "gold/lineage",
    columns: &[
        ("tenant_id", Col::Str, false),
        ("edge_id", Col::Str, false),
        ("graph", Col::Str, false),
        ("edge_label", Col::Str, false),
        ("src_id", Col::Str, false),
        ("dst_id", Col::Str, false),
        ("state", Col::Str, false),
        ("proposer", Col::Str, true),
        ("ratifier", Col::Str, true),
        ("created_at", Col::Ts, false),
        ("applied_seq", Col::Long, false),
    ],
    partition: None,
    merge_keys: &["edge_id"],
};

/// `telemetry.recall_daily`: daily recall aggregates, no query text, no
/// version ids, the last 30 days only (ADR-0054 d2, d3, d4; ADR-0058 d7).
pub const RECALL_DAILY: Spec = Spec {
    name: "telemetry.recall_daily",
    path: "telemetry/recall_daily",
    columns: &[
        ("tenant_id", Col::Str, false),
        ("day", Col::Date, false),
        ("pipeline", Col::Str, false),
        ("access", Col::Str, false),
        ("recalls", Col::Long, false),
        ("hits", Col::Long, false),
        ("empty_recalls", Col::Long, false),
        ("with_query_text", Col::Long, false),
        ("outcome_get", Col::Long, false),
        ("outcome_derived", Col::Long, false),
        ("outcome_superseded", Col::Long, false),
        ("outcome_feedback_useful", Col::Long, false),
        ("outcome_feedback_not_useful", Col::Long, false),
        ("outcome_requery", Col::Long, false),
        ("outcome_no_use", Col::Long, false),
    ],
    partition: Some("day"),
    merge_keys: &[],
};

/// `bench.results`: every number of every `oml-bench/1` file loaded, one
/// row per metric and category; provider data, not a tenant's
/// (ADR-0058 d8).
pub const BENCH_RESULTS: Spec = Spec {
    name: "bench.results",
    path: "bench/results",
    columns: &[
        ("run_digest", Col::Str, false),
        ("source", Col::Str, false),
        ("suite", Col::Str, false),
        ("suite_version", Col::Str, true),
        ("bench_kind", Col::Str, true),
        ("method", Col::Str, false),
        ("run_date", Col::Date, false),
        ("commit", Col::Str, true),
        ("host", Col::Str, true),
        ("postgres", Col::Str, true),
        ("build", Col::Str, true),
        ("n_queries", Col::Long, true),
        ("n_docs", Col::Long, true),
        ("sampled", Col::Long, true),
        ("category", Col::Str, false),
        ("metric", Col::Str, false),
        ("value", Col::F64, false),
        ("loaded_at", Col::Ts, false),
    ],
    partition: Some("run_date"),
    merge_keys: &["run_digest", "category", "metric"],
};

/// `lake.batches`: one row per drained batch, its application transaction
/// version the global frontier (ADR-0058 d3).
pub const LAKE_BATCHES: Spec = Spec {
    name: "lake.batches",
    path: "lake/batches",
    columns: &[
        ("batch_start", Col::Long, false),
        ("batch_end", Col::Long, false),
        ("tenants", Col::Long, false),
        ("events", Col::Long, false),
        ("committed_at", Col::Ts, false),
    ],
    partition: None,
    merge_keys: &[],
};

/// The tables under each tenant's prefix.
pub const TENANT_TABLES: [&Spec; 4] =
    [&BRONZE_EVENTS, &SILVER_CLAIMS, &GOLD_LINEAGE, &RECALL_DAILY];

/// The tables under the provider's prefix, `_provider/`.
pub const PROVIDER_TABLES: [&Spec; 2] = [&LAKE_BATCHES, &BENCH_RESULTS];

/// The provider prefix's directory name: never a tenant id.
pub const PROVIDER_PREFIX: &str = "_provider";
