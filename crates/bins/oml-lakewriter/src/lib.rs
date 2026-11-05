//! `oml-lakewriter` — the one process that drains the Postgres outbox into
//! the Delta lake (ADR-0003 d2, ADR-0006 d1, ADR-0058).
//!
//! A batch is cut from the outbox's stable prefix, past the global frontier
//! the `lake.batches` table's transaction version records (ADR-0058 d3, d4).
//! For each tenant, each table applies only the rows past its own
//! transaction version and commits with the batch's end as its new one, so
//! a crash between tables, a replay, or a second writer that slipped past
//! the lease writes no row twice; the frontier advances last. It is never
//! on the commit path: the lead commits whether or not this runs.

pub mod source;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU64};
use std::time::Instant;

use chrono::{DateTime, Utc};
use oml_lake::crypto::{Kek, sha256_hex};
use oml_lake::table::{
    BENCH_RESULTS, BRONZE_EVENTS, GOLD_LINEAGE, LAKE_BATCHES, RECALL_DAILY, SILVER_CLAIMS, Spec, V,
};
use oml_lake::{Lake, LakeError, Prefix, micros_to_date};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use serde_json::Value;
use uuid::Uuid;

use crate::source::{Event, Memory, Source, SourceError};

#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Lake(#[from] LakeError),
    #[error("bench file {0}: {1}")]
    Bench(String, String),
}

impl WriterError {
    /// A stable class for logs, metrics and the status file: never a URL,
    /// a credential or a row's content.
    #[must_use]
    pub fn class(&self) -> &'static str {
        match self {
            Self::Source(_) => "postgres",
            Self::Lake(LakeError::Delta(_)) => "delta_commit",
            Self::Lake(_) => "lake",
            Self::Bench(..) => "bench_file",
        }
    }
}

/// The metrics a writer serves on its own listener (ADR-0058 d16), with
/// closed label sets: a table name or a maintenance job, never a tenant.
#[derive(Debug, Default)]
pub struct Metrics {
    pub frontier: Gauge<i64, AtomicI64>,
    pub horizon: Gauge<i64, AtomicI64>,
    pub lag_rows: Gauge<i64, AtomicI64>,
    pub lag_seconds: Gauge<i64, AtomicI64>,
    pub batches: Counter<u64, AtomicU64>,
    pub rows: Family<Vec<(String, String)>, Counter<u64, AtomicU64>>,
    pub commit_failures: Family<Vec<(String, String)>, Counter<u64, AtomicU64>>,
    pub dead_letters: Counter<u64, AtomicU64>,
    pub maintenance: Family<Vec<(String, String)>, Counter<u64, AtomicU64>>,
    pub maintenance_files: Family<Vec<(String, String)>, Counter<u64, AtomicU64>>,
    pub batch_millis: Gauge<i64, AtomicI64>,
}

impl Metrics {
    pub fn register(&self, r: &mut Registry) {
        r.register(
            "oml_lake_frontier_seq",
            "Outbox seq the lake has applied through",
            self.frontier.clone(),
        );
        r.register(
            "oml_lake_horizon_seq",
            "Outbox seq of the stable prefix at the last cut",
            self.horizon.clone(),
        );
        r.register(
            "oml_lake_lag_rows",
            "Outbox seqs past the frontier",
            self.lag_rows.clone(),
        );
        r.register(
            "oml_lake_lag_seconds",
            "Age of the oldest outbox row past the frontier",
            self.lag_seconds.clone(),
        );
        r.register(
            "oml_lake_batches",
            "Batches committed",
            self.batches.clone(),
        );
        r.register(
            "oml_lake_rows",
            "Rows committed, by table",
            self.rows.clone(),
        );
        r.register(
            "oml_lake_commit_failures",
            "Failed lake commits, by table",
            self.commit_failures.clone(),
        );
        r.register(
            "oml_lake_dead_letters",
            "Outbox rows a table skipped",
            self.dead_letters.clone(),
        );
        r.register(
            "oml_lake_maintenance_runs",
            "Maintenance runs, by job and result",
            self.maintenance.clone(),
        );
        r.register(
            "oml_lake_maintenance_files",
            "Files compacted, written and vacuumed",
            self.maintenance_files.clone(),
        );
        r.register(
            "oml_lake_batch_millis",
            "Wall time of the last batch",
            self.batch_millis.clone(),
        );
    }

    fn table(
        &self,
        f: &Family<Vec<(String, String)>, Counter<u64, AtomicU64>>,
        t: &str,
    ) -> Counter<u64, AtomicU64> {
        f.get_or_create(&vec![("table".to_owned(), t.to_owned())])
            .clone()
    }
}

/// What one call to [`Writer::drain_once`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Drained {
    pub batch_start: i64,
    pub batch_end: i64,
    pub events: usize,
    pub rows: BTreeMap<&'static str, usize>,
    pub dead_letters: usize,
}

/// The writer's tunables.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The most outbox seqs one batch spans.
    pub max_batch: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { max_batch: 5000 }
    }
}

/// The lake writer over one Postgres source and one lake.
pub struct Writer {
    pub src: Source,
    pub lake: Lake,
    kek: Kek,
    pub metrics: Metrics,
    pub settings: Settings,
    frontier: Option<i64>,
}

impl Writer {
    #[must_use]
    pub fn new(src: Source, lake: Lake, kek: Kek, settings: Settings) -> Self {
        Self {
            src,
            lake,
            kek,
            metrics: Metrics::default(),
            settings,
            frontier: None,
        }
    }

    /// The global frontier: the `lake.batches` table's transaction version,
    /// read from the Delta log, the sole recovery authority (ADR-0003 d4).
    ///
    /// # Errors
    /// The log cannot be read.
    pub async fn frontier(&mut self) -> Result<i64, WriterError> {
        if let Some(f) = self.frontier {
            return Ok(f);
        }
        let f = self.lake.applied(Prefix::Provider, &LAKE_BATCHES).await?;
        self.frontier = Some(f);
        Ok(f)
    }

    /// Drains until the frontier reaches the stable prefix; the batches.
    ///
    /// # Errors
    /// The first batch that fails; the batches before it stand.
    pub async fn catch_up(&mut self) -> Result<Vec<Drained>, WriterError> {
        let mut out = Vec::new();
        loop {
            match self.drain_once().await? {
                Some(d) => out.push(d),
                None => return Ok(out),
            }
        }
    }

    /// One batch, or `None` when the frontier is at the stable prefix.
    ///
    /// # Errors
    /// A Postgres read or a lake commit fails; nothing past the frontier is
    /// assumed applied, and the next call replays exactly.
    pub async fn drain_once(&mut self) -> Result<Option<Drained>, WriterError> {
        let started = Instant::now();
        let frontier = self.frontier().await?;
        let Some(horizon) = self.src.horizon().await? else {
            return Ok(None);
        };
        self.metrics.horizon.set(horizon);
        self.metrics.frontier.set(frontier);
        self.metrics.lag_rows.set((horizon - frontier).max(0));
        if horizon <= frontier {
            self.metrics.lag_seconds.set(0);
            return Ok(None);
        }
        let end = horizon.min(frontier + self.settings.max_batch);
        let mut drained = Drained {
            batch_start: frontier,
            batch_end: end,
            ..Drained::default()
        };
        let mut tenants = 0_i64;
        for tenant in self.src.tenants().await? {
            let events = self.src.events(tenant, frontier, end).await?;
            if events.is_empty() {
                continue;
            }
            tenants += 1;
            drained.events += events.len();
            self.apply_tenant(tenant, &events, end, &mut drained)
                .await?;
        }
        let row = vec![
            V::Long(Some(frontier)),
            V::Long(Some(end)),
            V::Long(Some(tenants)),
            V::Long(Some(i64::try_from(drained.events).unwrap_or(i64::MAX))),
            V::Ts(Some(Utc::now().timestamp_micros())),
        ];
        self.commit(Prefix::Provider, &LAKE_BATCHES, &[row], end, false)
            .await?;
        self.frontier = Some(end);
        self.metrics.frontier.set(end);
        self.metrics.lag_rows.set((horizon - end).max(0));
        self.metrics.batches.inc();
        self.metrics
            .batch_millis
            .set(i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX));
        Ok(Some(drained))
    }

    /// The lag in seconds: the age of the oldest outbox row past the
    /// frontier, over every tenant.
    ///
    /// # Errors
    /// A read fails.
    pub async fn lag_seconds(&mut self) -> Result<i64, WriterError> {
        let frontier = self.frontier().await?;
        let mut oldest: Option<DateTime<Utc>> = None;
        for t in self.src.tenants().await? {
            if let Some(at) = self.src.oldest_pending(t, frontier).await? {
                oldest = Some(oldest.map_or(at, |o| o.min(at)));
            }
        }
        let secs = oldest.map_or(0, |o| (Utc::now() - o).num_seconds().max(0));
        self.metrics.lag_seconds.set(secs);
        Ok(secs)
    }

    async fn commit(
        &self,
        prefix: Prefix,
        spec: &Spec,
        rows: &[Vec<V>],
        version: i64,
        merge: bool,
    ) -> Result<usize, WriterError> {
        let result = if merge {
            self.lake.merge(prefix, spec, rows, version).await
        } else {
            self.lake.append(prefix, spec, rows, version).await
        };
        match result {
            Ok(n) => {
                self.metrics
                    .table(&self.metrics.rows, spec.name)
                    .inc_by(n as u64);
                Ok(n)
            }
            Err(e) => {
                self.metrics
                    .table(&self.metrics.commit_failures, spec.name)
                    .inc();
                Err(e.into())
            }
        }
    }

    async fn apply_tenant(
        &mut self,
        tenant: Uuid,
        events: &[Event],
        end: i64,
        drained: &mut Drained,
    ) -> Result<(), WriterError> {
        let p = Prefix::Tenant(tenant);
        // bronze.events: every event past the table's version.
        let applied = self.lake.applied(p, &BRONZE_EVENTS).await?;
        let rows: Vec<Vec<V>> = events
            .iter()
            .filter(|e| e.outbox_seq > applied)
            .map(|e| bronze_row(tenant, e))
            .collect();
        let n = self.commit(p, &BRONZE_EVENTS, &rows, end, false).await?;
        *drained.rows.entry(BRONZE_EVENTS.name).or_default() += n;

        // silver.claims and gold.lineage: the versions and edges the
        // events name, as Postgres holds them now (ADR-0058 d2).
        let silver_at = self.lake.applied(p, &SILVER_CLAIMS).await?;
        let lineage_at = self.lake.applied(p, &GOLD_LINEAGE).await?;
        let mut claims: BTreeMap<String, Vec<V>> = BTreeMap::new();
        let mut edges: BTreeMap<String, Vec<V>> = BTreeMap::new();
        for e in events
            .iter()
            .filter(|e| e.outbox_seq > silver_at.min(lineage_at))
        {
            let to_silver = e.outbox_seq > silver_at;
            let to_lineage = e.outbox_seq > lineage_at;
            if let Err(reason) = self
                .apply_event(tenant, e, to_silver, to_lineage, &mut claims, &mut edges)
                .await?
            {
                let table = if e.event_type == "review.ratify" {
                    GOLD_LINEAGE.name
                } else {
                    SILVER_CLAIMS.name
                };
                self.src
                    .dead_letter(tenant, e.outbox_seq, table, reason)
                    .await?;
                self.metrics.dead_letters.inc();
                drained.dead_letters += 1;
            }
        }
        let claims: Vec<Vec<V>> = claims.into_values().collect();
        let n = self.commit(p, &SILVER_CLAIMS, &claims, end, true).await?;
        *drained.rows.entry(SILVER_CLAIMS.name).or_default() += n;
        let edges: Vec<Vec<V>> = edges.into_values().collect();
        let n = self.commit(p, &GOLD_LINEAGE, &edges, end, true).await?;
        *drained.rows.entry(GOLD_LINEAGE.name).or_default() += n;
        Ok(())
    }

    /// The silver and lineage rows one event yields; `Err(reason)` when it
    /// names something that cannot be materialized, which dead-letters it.
    async fn apply_event(
        &self,
        tenant: Uuid,
        e: &Event,
        to_silver: bool,
        to_lineage: bool,
        claims: &mut BTreeMap<String, Vec<V>>,
        edges: &mut BTreeMap<String, Vec<V>>,
    ) -> Result<Result<(), &'static str>, WriterError> {
        let ty = e.event_type.as_str();
        if ty == "review.ratify" {
            let Some(edge_id) = uuid_at(&e.payload, "edge_id") else {
                return Ok(Ok(())); // a claim's ratification: its promote follows
            };
            if !to_lineage {
                return Ok(Ok(()));
            }
            let Some(edge) = self.src.edge(tenant, edge_id).await? else {
                return Ok(Err("edge_missing"));
            };
            if edge.ratified {
                let row = vec![
                    V::Str(Some(tenant.to_string())),
                    V::Str(Some(edge.id.to_string())),
                    V::Str(Some(edge.graph.to_owned())),
                    V::Str(Some(edge.label.clone())),
                    V::Str(Some(edge.src.to_string())),
                    V::Str(Some(edge.dst.to_string())),
                    V::Str(Some("ratified".to_owned())),
                    V::Str(Some(edge.proposer.to_string())),
                    V::Str(edge.ratifier.map(|r| r.to_string())),
                    V::Ts(Some(edge.created_at.timestamp_micros())),
                    V::Long(Some(e.outbox_seq)),
                ];
                edges.insert(edge.id.to_string(), row);
            }
            return Ok(Ok(()));
        }
        if !ty.starts_with("lifecycle.") {
            return Ok(Ok(()));
        }
        // The development exception never enters silver (ADR-0001,
        // delta-tables.md silver.claims).
        if e.payload.get("self_signed").and_then(Value::as_bool) == Some(true) {
            return Ok(Ok(()));
        }
        let erase = ty == "lifecycle.erase";
        let ids: Vec<Uuid> = ["artifact_id", "new_artifact_id", "replaces_artifact_id"]
            .iter()
            .filter_map(|k| uuid_at(&e.payload, k))
            .collect();
        if ids.is_empty() {
            return Ok(Err("payload_names_no_version"));
        }
        if erase && to_silver {
            for id in &ids {
                self.src.destroy_key(tenant, *id, self.kek.id()).await?;
            }
        }
        let same_user = e.payload.get("same_user").and_then(Value::as_bool);
        for id in ids {
            let Some(m) = self.src.memory(tenant, id).await? else {
                return Ok(Err("version_missing"));
            };
            let Some(promoted_at) = m.promoted_at else {
                return Ok(Err("version_not_promoted"));
            };
            if to_silver {
                let row = self
                    .claim_row(tenant, &m, promoted_at, e, same_user, erase)
                    .await?;
                match row {
                    Ok(row) => {
                        claims.insert(m.id.to_string(), row);
                    }
                    Err(reason) => return Ok(Err(reason)),
                }
            }
            if to_lineage {
                for (label, dst) in m
                    .derives_from
                    .iter()
                    .map(|d| ("derives_from", *d))
                    .chain(m.replaces.map(|r| ("supersedes", r)))
                {
                    let id = format!("{label}:{}:{dst}", m.id);
                    let row = vec![
                        V::Str(Some(tenant.to_string())),
                        V::Str(Some(id.clone())),
                        V::Str(Some("claim".to_owned())),
                        V::Str(Some(label.to_owned())),
                        V::Str(Some(m.id.to_string())),
                        V::Str(Some(dst.to_string())),
                        V::Str(Some("ratified".to_owned())),
                        V::Str(Some(m.author_key_id.to_string())),
                        V::Str(None),
                        V::Ts(Some(promoted_at.timestamp_micros())),
                        V::Long(Some(e.outbox_seq)),
                    ];
                    edges.insert(id, row);
                }
            }
        }
        Ok(Ok(()))
    }

    /// The silver row of version `m` as of event `e`: its body sealed under
    /// its own data key, or no body once that key is destroyed.
    async fn claim_row(
        &self,
        tenant: Uuid,
        m: &Memory,
        promoted_at: DateTime<Utc>,
        e: &Event,
        same_user: Option<bool>,
        erase: bool,
    ) -> Result<Result<Vec<V>, &'static str>, WriterError> {
        let key = match self.src.key(tenant, m.id).await? {
            Some(k) => k,
            None if erase || m.status == "erased" => {
                self.src.destroy_key(tenant, m.id, self.kek.id()).await?;
                self.src
                    .key(tenant, m.id)
                    .await?
                    .ok_or(SourceError::Pg(sqlx::Error::RowNotFound))?
            }
            None => {
                let (_, wrapped) = match self.kek.new_data_key(tenant, m.id) {
                    Ok(k) => k,
                    Err(_) => return Ok(Err("key_generation_failed")),
                };
                self.src
                    .put_key(tenant, m.id, &wrapped, self.kek.id())
                    .await?
            }
        };
        let sealed = match &key.wrapped {
            None => None,
            Some(wrapped) => {
                let Ok(dek) = self.kek.unwrap(tenant, m.id, wrapped) else {
                    return Ok(Err("key_unwrap_failed"));
                };
                match dek.seal_body(tenant, m.id, m.body.as_bytes()) {
                    Ok(s) => Some(s),
                    Err(_) => return Ok(Err("seal_failed")),
                }
            }
        };
        let state = if sealed.is_none() || m.status == "erased" {
            "erased"
        } else {
            match (m.status.as_str(), m.is_tip) {
                ("promoted", true) => "promoted",
                ("promoted", false) => "superseded",
                ("demoted", _) => "demoted",
                ("held", _) => "held",
                _ => return Ok(Err("version_state_unknown")),
            }
        };
        let retracted = (state != "promoted").then(|| e.at.timestamp_micros());
        Ok(Ok(vec![
            V::Str(Some(tenant.to_string())),
            V::Str(Some(m.id.to_string())),
            V::Long(Some(m.version)),
            V::Str(Some(m.claim_key.clone())),
            V::Str(Some(m.kind.clone())),
            V::Str(Some(state.to_owned())),
            V::Str(Some(m.workarea_id.to_string())),
            V::Str(Some(m.author_key_id.to_string())),
            V::Str(Some(m.author_principal.clone())),
            V::Bool(same_user),
            V::Bool(Some(m.retirement)),
            V::Str(m.replaces.map(|r| r.to_string())),
            V::Str(Some(hex::encode(&m.digest))),
            V::Str(sealed.as_deref().map(sha256_hex)),
            V::Bin(sealed),
            V::Str(Some("lake".to_owned())),
            V::Str(Some(key.kek_id.clone())),
            V::Long(Some(m.body_bytes)),
            V::Ts(Some(m.created_at.timestamp_micros())),
            V::Ts(Some(promoted_at.timestamp_micros())),
            V::Ts(retracted),
            V::Long(Some(e.outbox_seq)),
        ]))
    }

    /// Rewrites every tenant's `telemetry.recall_daily` from the 30 days
    /// Postgres keeps: a day older than that leaves the lake's current
    /// version as it leaves Postgres (ADR-0054 d2, ADR-0058 d7).
    ///
    /// # Errors
    /// A read or a commit fails.
    pub async fn refresh_telemetry(&self) -> Result<usize, WriterError> {
        let mut total = 0;
        for t in self.src.tenants().await? {
            let days = self.src.recall_days(t).await?;
            let p = Prefix::Tenant(t);
            if days.is_empty() && self.lake.open(p, &RECALL_DAILY).await?.is_none() {
                continue;
            }
            let rows: Vec<Vec<V>> = days
                .iter()
                .map(|d| {
                    let mut row = vec![
                        V::Str(Some(t.to_string())),
                        V::Date(Some(days_since_epoch(d.day))),
                        V::Str(Some(d.pipeline.clone())),
                        V::Str(Some(d.access.clone())),
                    ];
                    row.extend(d.counts.iter().map(|c| V::Long(Some(*c))));
                    row
                })
                .collect();
            match self.lake.replace(p, &RECALL_DAILY, &rows).await {
                Ok(n) => {
                    total += n;
                    self.metrics
                        .table(&self.metrics.rows, RECALL_DAILY.name)
                        .inc_by(n as u64);
                }
                Err(e) => {
                    self.metrics
                        .table(&self.metrics.commit_failures, RECALL_DAILY.name)
                        .inc();
                    return Err(e.into());
                }
            }
        }
        Ok(total)
    }

    /// One maintenance pass over every table (ADR-0006 d3, ADR-0058 d12).
    ///
    /// # Errors
    /// The lake cannot be listed; a table's own failure is counted and the
    /// pass goes on.
    pub async fn maintain(
        &self,
        retention: chrono::Duration,
    ) -> Result<oml_lake::Maintained, WriterError> {
        let mut sum = oml_lake::Maintained::default();
        for (prefix, spec) in self.lake.existing()? {
            let job = |r: &str| {
                vec![
                    ("job".to_owned(), "table".to_owned()),
                    ("result".to_owned(), r.to_owned()),
                ]
            };
            match self.lake.maintain(prefix, spec, retention).await {
                Ok(m) => {
                    self.metrics.maintenance.get_or_create(&job("ok")).inc();
                    for (kind, n) in [
                        ("compacted", m.files_compacted),
                        ("written", m.files_written),
                        ("vacuumed", m.files_vacuumed),
                    ] {
                        self.metrics
                            .maintenance_files
                            .get_or_create(&vec![("kind".to_owned(), kind.to_owned())])
                            .inc_by(n);
                    }
                    sum.files_compacted += m.files_compacted;
                    sum.files_written += m.files_written;
                    sum.files_vacuumed += m.files_vacuumed;
                    sum.logs_removed += m.logs_removed;
                }
                Err(e) => {
                    self.metrics.maintenance.get_or_create(&job("failed")).inc();
                    tracing::warn!(
                        event = "lake_maintenance_failed",
                        table = spec.name,
                        class = WriterError::from(e).class()
                    );
                }
            }
        }
        Ok(sum)
    }

    /// Opens a silver body with the lake KEK, for the erase law and the
    /// operator: `None` once its key is destroyed.
    ///
    /// # Errors
    /// A read fails.
    pub async fn open_body(
        &self,
        tenant: Uuid,
        artifact: Uuid,
        sealed: &[u8],
    ) -> Result<Option<Vec<u8>>, WriterError> {
        let Some(key) = self.src.key(tenant, artifact).await? else {
            return Ok(None);
        };
        let Some(wrapped) = key.wrapped else {
            return Ok(None);
        };
        Ok(self
            .kek
            .unwrap(tenant, artifact, &wrapped)
            .ok()
            .and_then(|dek| dek.open_body(tenant, artifact, sealed).ok()))
    }
}

fn bronze_row(tenant: Uuid, e: &Event) -> Vec<V> {
    let at = e.at.timestamp_micros();
    let s = |k: &str| e.payload.get(k).and_then(Value::as_str).map(str::to_owned);
    let artifact = s("artifact_id").or_else(|| s("new_artifact_id"));
    let version = e
        .payload
        .get("version")
        .or_else(|| e.payload.get("new_version"))
        .and_then(Value::as_i64);
    let name = (e.event_type == "workarea.create")
        .then(|| s("name"))
        .flatten();
    vec![
        V::Str(Some(tenant.to_string())),
        V::Long(Some(e.journal_seq)),
        V::Str(Some(e.event_id.to_string())),
        V::Str(Some(e.event_type.clone())),
        V::Ts(Some(at)),
        V::Date(Some(micros_to_date(at))),
        V::Bin(Some(e.prev_hash.clone())),
        V::Bin(Some(e.hash.clone())),
        V::Bin(Some(e.sig.clone())),
        V::Bin(Some(e.signed_bytes.clone())),
        V::Str(Some(e.payload.to_string())),
        V::Str(Some(e.principal.clone())),
        V::Str(e.api_key_id.map(|k| k.to_string())),
        V::Str(artifact),
        V::Long(version),
        V::Str(s("workarea_id")),
        V::Str(s("kind")),
        V::Str(name),
        V::Bool(e.payload.get("same_user").and_then(Value::as_bool)),
        V::Long(Some(e.outbox_seq)),
    ]
}

fn uuid_at(v: &Value, key: &str) -> Option<Uuid> {
    v.get(key)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
}

fn days_since_epoch(d: chrono::NaiveDate) -> i32 {
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default();
    i32::try_from((d - epoch).num_days()).unwrap_or(i32::MAX)
}

/// Loads every `oml-bench/1` JSON file under `paths` into `bench.results`,
/// one row per number, idempotent by the file's digest (ADR-0058 d8).
/// Files of another schema are skipped. Returns (files loaded, rows).
///
/// # Errors
/// A file cannot be read or the commit fails.
pub async fn load_bench(
    lake: &Lake,
    paths: &[std::path::PathBuf],
) -> Result<(usize, usize), WriterError> {
    let mut files = Vec::new();
    for p in paths {
        collect_json(p, &mut files)
            .map_err(|e| WriterError::Bench(p.display().to_string(), e.to_string()))?;
    }
    files.sort();
    let mut rows = Vec::new();
    let mut loaded = 0;
    let now = Utc::now().timestamp_micros();
    for f in &files {
        let bytes = std::fs::read(f)
            .map_err(|e| WriterError::Bench(f.display().to_string(), e.to_string()))?;
        let Ok(doc) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if doc.get("schema").and_then(Value::as_str) != Some("oml-bench/1") {
            continue;
        }
        loaded += 1;
        let digest = sha256_hex(&bytes);
        let s = |k: &str| doc.get(k).and_then(Value::as_str).map(str::to_owned);
        let env = |k: &str| {
            doc.pointer(&format!("/environment/{k}"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let n = |k: &str| doc.get(k).and_then(Value::as_i64);
        let date = s("date")
            .and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
            .map_or(0, days_since_epoch);
        let mut numbers: Vec<(String, String, f64)> = Vec::new();
        for (section, category) in [
            ("metrics", String::new()),
            ("latency_ms", "latency".to_owned()),
        ] {
            if let Some(obj) = doc.get(section).and_then(Value::as_object) {
                for (k, v) in obj {
                    if let Some(x) = v.as_f64() {
                        numbers.push((category.clone(), k.clone(), x));
                    }
                }
            }
        }
        if let Some(cats) = doc.get("per_category").and_then(Value::as_object) {
            for (cat, obj) in cats {
                for (k, v) in obj.as_object().into_iter().flatten() {
                    if let Some(x) = v.as_f64() {
                        numbers.push((cat.clone(), k.clone(), x));
                    }
                }
            }
        }
        for (category, metric, value) in numbers {
            rows.push(vec![
                V::Str(Some(digest.clone())),
                V::Str(Some(
                    f.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )),
                V::Str(Some(s("suite").unwrap_or_default())),
                V::Str(s("suite_version")),
                V::Str(s("kind")),
                V::Str(Some(s("method").unwrap_or_default())),
                V::Date(Some(date)),
                V::Str(s("commit")),
                V::Str(env("host")),
                V::Str(env("postgres")),
                V::Str(env("build")),
                V::Long(n("n_queries")),
                V::Long(n("n_docs")),
                V::Long(n("sampled")),
                V::Str(Some(category)),
                V::Str(Some(metric)),
                V::F64(Some(value)),
                V::Ts(Some(now)),
            ]);
        }
    }
    // Duplicate keys within one load (two copies of one file) keep one row.
    let mut by_key: BTreeMap<(String, String, String), Vec<V>> = BTreeMap::new();
    for r in rows {
        let k = |i: usize| match &r[i] {
            V::Str(Some(s)) => s.clone(),
            _ => String::new(),
        };
        by_key.insert((k(0), k(14), k(15)), r);
    }
    let rows: Vec<Vec<V>> = by_key.into_values().collect();
    let n = rows.len();
    lake.merge(Prefix::Provider, &BENCH_RESULTS, &rows, 0)
        .await?;
    Ok((loaded, n))
}

fn collect_json(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    if p.is_dir() {
        for e in std::fs::read_dir(p)? {
            collect_json(&e?.path(), out)?;
        }
    } else if p.extension().is_some_and(|x| x == "json") {
        out.push(p.to_path_buf());
    }
    Ok(())
}

/// The status the writer leaves for the doctor, at
/// `<lake>/_provider/lake/status.json`: times, seqs, counts and error
/// classes only, never a URL or a credential (ADR-0058 d15, d16).
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Status {
    pub pid: u32,
    pub at: i64,
    pub frontier: i64,
    pub horizon: i64,
    pub lag_seconds: i64,
    pub failed_commits: u64,
    pub last_failure_class: Option<String>,
    pub last_failure_at: Option<i64>,
    pub last_maintenance_at: Option<i64>,
}

impl Status {
    #[must_use]
    pub fn path(lake: &Lake) -> std::path::PathBuf {
        lake.root()
            .join(oml_lake::table::PROVIDER_PREFIX)
            .join("lake")
            .join("status.json")
    }

    /// Writes the status atomically (a temp file renamed over it).
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn write(&self, lake: &Lake) -> std::io::Result<()> {
        let path = Self::path(lake);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default())?;
        std::fs::rename(tmp, path)
    }

    #[must_use]
    pub fn read(lake: &Lake) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(Self::path(lake)).ok()?).ok()
    }
}
