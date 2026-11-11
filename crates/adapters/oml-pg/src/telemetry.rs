//! The recall telemetry's writer, labeller and partitions (ADR-0054 d2,
//! d3, d5, d6, as accepted; migration 0023).
//!
//! A recall hands its event to a bounded queue and never waits: a task
//! writes the queue in batches, at most [`BATCH`] items or every second,
//! each tenant's items in a transaction of their own under that tenant's
//! row security, never inside a recall's snapshot. A full queue, a store
//! error or a tenant with no partition drops the event and counts it; the
//! recall's answer is the same. The same task attaches labels by SQL and,
//! every minute, labels `no_use`, purges what is past the 30 days and
//! refreshes the health gauges doctor reads.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use sqlx::postgres::PgRow;
use sqlx::{PgConnection, PgPool, Postgres, Row as _, Transaction};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use oml_app::domain::EventTime;
use oml_app::ports::{StoreError, TelemetryMeter};
use oml_app::telemetry::{
    FeedbackLabels, INFERRED_LABELS_PER_DAY, LABEL_WINDOW_SECS, Label, LabelledQuery,
    NO_USE_AFTER_SECS, REQUERY_WINDOW_SECS, RETENTION_DAYS, RecallEvent, Remembered, ServedRecall,
    StoredEvent, StoredLabel, Telemetry, TelemetryFuture, TelemetryItem,
};

use crate::{backend, set_tenant};

/// The queue's bound: past it an event is dropped, never waited for
/// (ADR-0054 d2).
pub const QUEUE: usize = 10_000;

/// The most items one write takes.
pub const BATCH: usize = 500;

/// How often the labeller's pass runs.
pub const PASS_SECONDS: u64 = 60;

/// How often the pass counts the memories no recall returned.
pub const UNRECALLED_SECONDS: u64 = 600;

/// The most rows one purge statement deletes.
const PURGE_BATCH: i64 = 5_000;

/// How the stored times come back, in the fixed-width signed form.
const AT: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'";

/// The two telemetry tables, each list-partitioned by tenant.
pub const TABLES: [&str; 2] = ["recall_events", "recall_outcomes"];

enum Msg {
    Item(TelemetryItem),
    Flush(oneshot::Sender<()>),
}

/// The telemetry port over PostgreSQL: the queue, and the reads the
/// feedback check, the admin listing and the replay make.
pub struct PgTelemetry {
    pool: PgPool,
    queue: mpsc::Sender<Msg>,
    meter: Arc<dyn TelemetryMeter>,
}

impl std::fmt::Debug for PgTelemetry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgTelemetry").finish_non_exhaustive()
    }
}

impl PgTelemetry {
    /// The port over `pool`, the serving role's, and the task that writes
    /// its queue and runs the labeller's pass, counting into `meter`.
    pub fn spawn(
        pool: PgPool,
        meter: Arc<dyn TelemetryMeter>,
    ) -> (Arc<Self>, tokio::task::JoinHandle<()>) {
        let (queue, rx) = mpsc::channel(QUEUE);
        let last_pass = Arc::new(AtomicI64::new(unix_now()));
        let telemetry = Arc::new(Self {
            pool: pool.clone(),
            queue,
            meter: Arc::clone(&meter),
        });
        let writer = Writer::new(pool, meter, last_pass);
        (telemetry, tokio::spawn(writer.run(rx)))
    }

    /// One labeller's pass over `pool`, counted into `meter`, awaited to
    /// its end: the lag it found, `no_use`, the purge and the unrecalled
    /// count, as the spawned writer runs each minute. For tests alone,
    /// which drive it in place of the writer's minute so no assertion waits
    /// on a clock (ADR-0054 d13; #278); the lead runs [`Self::spawn`].
    #[doc(hidden)]
    pub async fn pass_once(pool: PgPool, meter: Arc<dyn TelemetryMeter>) {
        Writer::new(pool, meter, Arc::new(AtomicI64::new(unix_now())))
            .pass()
            .await;
    }

    fn depth(&self) -> i64 {
        i64::try_from(QUEUE - self.queue.capacity()).unwrap_or(i64::MAX)
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Telemetry for PgTelemetry {
    fn offer(&self, item: TelemetryItem) {
        let is_event = matches!(item, TelemetryItem::Recall(_));
        let labels = match &item {
            TelemetryItem::Feedback(f) => (f.useful.len() + f.not_useful.len()) as u64,
            TelemetryItem::Act(_) => 1,
            _ => 0,
        };
        if self.queue.try_send(Msg::Item(item)).is_err() {
            if is_event {
                self.meter.events("dropped_queue_full", 1);
            } else if labels > 0 {
                self.meter.labels_dropped("queue_full", labels);
            }
        }
        self.meter.queue_depth(self.depth());
    }

    fn flush(&self) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            // Never waits for queue room: a full queue means the writer is
            // behind, and the caller is a read (ADR-0054 d2).
            let (done, wait) = oneshot::channel();
            if self.queue.try_send(Msg::Flush(done)).is_ok() {
                let _ = wait.await;
            }
        })
    }

    fn served(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
        recall_id: Uuid,
    ) -> TelemetryFuture<'_, Option<ServedRecall>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(backend)?;
            set_tenant(&mut tx, tenant_id).await?;
            let row = sqlx::query(&format!(
                "SELECT to_char(created_at AT TIME ZONE 'UTC', {AT}) AS at, hit_ids \
                 FROM recall_events \
                 WHERE tenant_id = $1 AND recall_id = $2 AND key_id = $3 \
                   AND created_at > now() - make_interval(secs => $4)"
            ))
            .bind(tenant_id)
            .bind(recall_id)
            .bind(key_id)
            .bind(LABEL_WINDOW_SECS as f64)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            row.map(|row| {
                Ok(ServedRecall {
                    created_at: time(&row, "at")?,
                    hit_ids: row.try_get("hit_ids").map_err(backend)?,
                })
            })
            .transpose()
        })
    }

    fn explicit_labels_today(&self, tenant_id: Uuid, key_id: Uuid) -> TelemetryFuture<'_, u64> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(backend)?;
            set_tenant(&mut tx, tenant_id).await?;
            let n = labels_today(&mut tx, tenant_id, key_id, "explicit").await?;
            tx.commit().await.map_err(backend)?;
            Ok(n)
        })
    }

    fn events(
        &self,
        tenant_id: Uuid,
        before: Option<(EventTime, Uuid)>,
        limit: u32,
    ) -> TelemetryFuture<'_, Vec<StoredEvent>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(backend)?;
            set_tenant(&mut tx, tenant_id).await?;
            let (before_at, before_id) = before.map_or((None, None), |(at, id)| {
                (Some(at.as_str().to_owned()), Some(id))
            });
            let rows = sqlx::query(&format!(
                "SELECT e.recall_id, to_char(e.created_at AT TIME ZONE 'UTC', {AT}) AS at, \
                        e.key_id, e.principal, e.scope_workarea_id, e.pipeline, e.access, \
                        encode(e.query_digest, 'hex') AS digest, e.query_text, e.hit_ids, \
                        e.hit_scores, \
                        COALESCE((SELECT json_agg(json_build_object( \
                            'artifact_id', o.artifact_id, 'label', o.label, \
                            'source', o.source, \
                            'at', to_char(o.at AT TIME ZONE 'UTC', {AT})) \
                            ORDER BY o.at, o.label) \
                          FROM recall_outcomes o \
                         WHERE o.tenant_id = e.tenant_id AND o.recall_id = e.recall_id \
                           AND o.created_at = e.created_at), '[]'::json) AS labels \
                 FROM recall_events e \
                 WHERE e.tenant_id = $1 \
                   AND e.created_at > now() - make_interval(days => $2) \
                   AND ($3::timestamptz IS NULL \
                        OR (e.created_at, e.recall_id) < ($3::timestamptz, $4::uuid)) \
                 ORDER BY e.created_at DESC, e.recall_id DESC \
                 LIMIT $5"
            ))
            .bind(tenant_id)
            .bind(i32::try_from(RETENTION_DAYS).unwrap_or(30))
            .bind(before_at)
            .bind(before_id)
            .bind(i64::from(limit))
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            rows.iter().map(stored_event).collect()
        })
    }

    fn labelled_queries(&self, tenant_id: Uuid) -> TelemetryFuture<'_, Vec<LabelledQuery>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(backend)?;
            set_tenant(&mut tx, tenant_id).await?;
            // An event with text and a label that names a version: explicit,
            // derived or get, as ADR-0054 d11 decides; superseded grades but
            // does not make a query labelled by itself.
            let rows = sqlx::query(
                "SELECT e.key_id, e.scope_workarea_id, e.query_text, \
                        array_agg(o.artifact_id ORDER BY o.artifact_id, o.label) AS ids, \
                        array_agg(o.label ORDER BY o.artifact_id, o.label) AS labels \
                 FROM recall_events e \
                 JOIN recall_outcomes o ON o.tenant_id = e.tenant_id \
                  AND o.recall_id = e.recall_id AND o.created_at = e.created_at \
                 WHERE e.tenant_id = $1 AND e.query_text IS NOT NULL \
                   AND e.created_at > now() - make_interval(days => $2) \
                   AND o.artifact_id IS NOT NULL \
                 GROUP BY e.tenant_id, e.recall_id, e.created_at, e.key_id, \
                          e.scope_workarea_id, e.query_text \
                 HAVING bool_or(o.source = 'explicit' OR o.label IN ('derived', 'get')) \
                 ORDER BY e.created_at, e.recall_id",
            )
            .bind(tenant_id)
            .bind(i32::try_from(RETENTION_DAYS).unwrap_or(30))
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            rows.iter()
                .map(|row| {
                    let ids: Vec<Uuid> = row.try_get("ids").map_err(backend)?;
                    let labels: Vec<String> = row.try_get("labels").map_err(backend)?;
                    // A version's grade is the highest any of its labels
                    // gives it.
                    let mut grades: BTreeMap<Uuid, u8> = BTreeMap::new();
                    for (id, label) in ids.into_iter().zip(labels) {
                        if let Some(g) = Label::parse(&label).and_then(Label::grade) {
                            let slot = grades.entry(id).or_insert(g);
                            *slot = (*slot).max(g);
                        }
                    }
                    Ok(LabelledQuery {
                        key_id: row.try_get("key_id").map_err(backend)?,
                        scope_workarea_id: row.try_get("scope_workarea_id").map_err(backend)?,
                        query_text: row.try_get("query_text").map_err(backend)?,
                        grades: grades.into_iter().collect(),
                    })
                })
                .collect()
        })
    }
}

fn time(row: &PgRow, column: &str) -> Result<EventTime, StoreError> {
    let at: String = row.try_get(column).map_err(backend)?;
    EventTime::parse(&at).map_err(backend)
}

fn stored_event(row: &PgRow) -> Result<StoredEvent, StoreError> {
    let ids: Vec<Uuid> = row.try_get("hit_ids").map_err(backend)?;
    let scores: Vec<f32> = row.try_get("hit_scores").map_err(backend)?;
    let labels: serde_json::Value = row.try_get("labels").map_err(backend)?;
    let labels = labels
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            Ok(StoredLabel {
                artifact_id: l["artifact_id"]
                    .as_str()
                    .map(Uuid::parse_str)
                    .transpose()
                    .map_err(backend)?,
                label: l["label"].as_str().unwrap_or_default().to_owned(),
                source: l["source"].as_str().unwrap_or_default().to_owned(),
                at: EventTime::parse(l["at"].as_str().unwrap_or_default()).map_err(backend)?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    Ok(StoredEvent {
        recall_id: row.try_get("recall_id").map_err(backend)?,
        created_at: time(row, "at")?,
        key_id: row.try_get("key_id").map_err(backend)?,
        principal: row.try_get("principal").map_err(backend)?,
        scope_workarea_id: row.try_get("scope_workarea_id").map_err(backend)?,
        pipeline: row.try_get("pipeline").map_err(backend)?,
        access: row.try_get("access").map_err(backend)?,
        query_digest: format!(
            "sha256:{}",
            row.try_get::<String, _>("digest").map_err(backend)?
        ),
        query_text: row.try_get("query_text").map_err(backend)?,
        hits: ids.into_iter().zip(scores).collect(),
        labels,
    })
}

/// The labels of `source` on `key_id`'s events since the start of today,
/// UTC.
async fn labels_today(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    key_id: Uuid,
    source: &str,
) -> Result<u64, StoreError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM recall_outcomes o JOIN recall_events e \
           ON e.tenant_id = o.tenant_id AND e.recall_id = o.recall_id \
          AND e.created_at = o.created_at \
         WHERE o.tenant_id = $1 AND e.key_id = $2 AND o.source = $3 \
           AND o.at >= date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'",
    )
    .bind(tenant_id)
    .bind(key_id)
    .bind(source)
    .fetch_one(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(u64::try_from(n).unwrap_or(0))
}

struct Writer {
    pool: PgPool,
    meter: Arc<dyn TelemetryMeter>,
    last_pass: Arc<AtomicI64>,
    /// Each key's inferred labels today, read once a day per key and
    /// counted on (ADR-0054 d5, as accepted).
    per_key_today: BTreeMap<(Uuid, Uuid), (i64, u64)>,
    last_unrecalled: Option<Instant>,
    /// Each tenant's `no_use` high-water mark: the bound of its last pass.
    no_use_marks: BTreeMap<Uuid, String>,
}

impl Writer {
    /// A writer with nothing counted yet: the one setup `spawn` and
    /// `pass_once` share.
    fn new(pool: PgPool, meter: Arc<dyn TelemetryMeter>, last_pass: Arc<AtomicI64>) -> Self {
        Self {
            pool,
            meter,
            last_pass,
            per_key_today: BTreeMap::new(),
            last_unrecalled: None,
            no_use_marks: BTreeMap::new(),
        }
    }

    async fn run(mut self, mut rx: mpsc::Receiver<Msg>) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut pass = tokio::time::interval(Duration::from_secs(PASS_SECONDS));
        pass.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut buf: Vec<TelemetryItem> = Vec::new();
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    None => {
                        self.write(std::mem::take(&mut buf)).await;
                        return;
                    }
                    Some(Msg::Item(item)) => {
                        buf.push(item);
                        if buf.len() >= BATCH {
                            self.write(std::mem::take(&mut buf)).await;
                        }
                    }
                    Some(Msg::Flush(done)) => {
                        self.write(std::mem::take(&mut buf)).await;
                        let _ = done.send(());
                    }
                },
                _ = tick.tick() => {
                    if !buf.is_empty() {
                        self.write(std::mem::take(&mut buf)).await;
                    }
                    #[allow(clippy::cast_precision_loss, reason = "seconds")]
                    let lag = (unix_now() - self.last_pass.load(Ordering::Relaxed)).max(0) as f64;
                    self.meter.lag(lag);
                    self.meter.queue_depth(i64::try_from(rx.len()).unwrap_or(i64::MAX));
                }
                _ = pass.tick() => self.pass().await,
            }
        }
    }

    /// Writes `items`, each tenant's in transactions of its own: events
    /// first, then the labels that may name them.
    async fn write(&mut self, items: Vec<TelemetryItem>) {
        let mut by_tenant: BTreeMap<Uuid, Vec<TelemetryItem>> = BTreeMap::new();
        for item in items {
            let tenant = match &item {
                TelemetryItem::Recall(e) => e.tenant_id,
                TelemetryItem::Act(a) => a.tenant_id,
                TelemetryItem::Feedback(f) => f.tenant_id,
                TelemetryItem::Remembered(r) => r.tenant_id,
            };
            by_tenant.entry(tenant).or_default().push(item);
        }
        // Each tenant's items in the order they came: a run of events in
        // one transaction, then each act, feedback or write after them, so
        // a get that came before the next recall labels the first one
        // before that recall's requery is judged (ADR-0054 d5).
        for (tenant_id, items) in by_tenant {
            let mut run: Vec<TelemetryItem> = Vec::new();
            for item in items {
                if matches!(item, TelemetryItem::Recall(_)) {
                    run.push(item);
                    continue;
                }
                self.flush_events(tenant_id, std::mem::take(&mut run)).await;
                self.write_other(tenant_id, item).await;
            }
            self.flush_events(tenant_id, run).await;
        }
    }

    async fn flush_events(&self, tenant_id: Uuid, events: Vec<TelemetryItem>) {
        if events.is_empty() {
            return;
        }
        let n = events.len() as u64;
        match self.write_events(tenant_id, events).await {
            Ok(requeried) => {
                self.meter.events("written", n);
                if requeried > 0 {
                    self.meter.labels(Label::Requery.as_str(), requeried);
                }
            }
            Err(reason) => self.meter.events(reason, n),
        }
    }

    /// The tenant's events in one transaction, each one's text kept only
    /// where the tenant opted in at or before its instant (ADR-0054 d3),
    /// and the `requery` label on the key's previous recall in the scope
    /// within two minutes, with another digest and no positive label.
    /// Returns the requery labels written, or the drop's outcome.
    async fn write_events(
        &self,
        tenant_id: Uuid,
        events: Vec<TelemetryItem>,
    ) -> Result<u64, &'static str> {
        let result: Result<u64, sqlx::Error> = async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
                .bind(tenant_id.to_string())
                .execute(&mut *tx)
                .await?;
            let mut requeried = 0;
            for item in events {
                let TelemetryItem::Recall(e) = item else {
                    continue;
                };
                insert_event(&mut tx, &e).await?;
                requeried += requery(&mut tx, &e).await?;
            }
            tx.commit().await?;
            Ok(requeried)
        }
        .await;
        result.map_err(|e| match e.as_database_error().and_then(|d| d.code()) {
            // No partition of the tenant holds the row.
            Some(code) if code == "23514" => "dropped_no_partition",
            _ => "dropped_store_error",
        })
    }

    async fn write_other(&mut self, tenant_id: Uuid, item: TelemetryItem) {
        let result: Result<(), sqlx::Error> = async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
                .bind(tenant_id.to_string())
                .execute(&mut *tx)
                .await?;
            match item {
                TelemetryItem::Act(act) => {
                    let day = unix_now() / 86_400;
                    let key = (tenant_id, act.key_id);
                    let counted = match self.per_key_today.get(&key) {
                        Some((d, n)) if *d == day => *n,
                        _ => {
                            let n = labels_today(&mut tx, tenant_id, act.key_id, "inferred")
                                .await
                                .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
                            self.per_key_today.insert(key, (day, n));
                            n
                        }
                    };
                    if counted >= INFERRED_LABELS_PER_DAY {
                        self.meter.labels_dropped("rate_limited", 1);
                    } else if attach(&mut tx, &act.key_id, act.artifact_id, act.label, &act.at)
                        .await?
                    {
                        self.per_key_today.insert(key, (day, counted + 1));
                        self.meter.labels(act.label.as_str(), 1);
                    }
                }
                TelemetryItem::Feedback(f) => {
                    let n = feedback(&mut tx, &f).await?;
                    for (label, count) in n {
                        if count > 0 {
                            self.meter.labels(label.as_str(), count);
                        }
                    }
                }
                TelemetryItem::Remembered(r) => {
                    let same = duplicate(&mut tx, &r).await?;
                    self.meter
                        .remembered(if same { "same_text" } else { "none" });
                }
                TelemetryItem::Recall(_) => {}
            }
            tx.commit().await
        }
        .await;
        if result.is_err() {
            self.meter.labels_dropped("store_error", 1);
        }
    }

    /// The labeller's pass, every minute, over every tenant whose telemetry
    /// partition exists (ADR-0054 d2, d5, d13), each step its own
    /// transaction: the tenant's lag first (a row past the 30 days and the
    /// pass's own minute, which means the purge is behind), then `no_use`
    /// on recalls between this tenant's high-water mark and 30 minutes ago,
    /// then the purge in batches of [`PURGE_BATCH`] rows, each committed, then
    /// every 10 minutes the unrecalled count.
    async fn pass(&mut self) {
        let Ok(tenants) = telemetry_tenants(&self.pool).await else {
            return;
        };
        let mut overdue = 0;
        let mut unrecalled = [0_i64; 4];
        let count_unrecalled = self
            .last_unrecalled
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(UNRECALLED_SECONDS));
        for tenant_id in &tenants {
            match self.lagging(*tenant_id).await {
                Ok(true) | Err(_) => overdue += 1,
                Ok(false) => {}
            }
            match self.no_use(*tenant_id).await {
                Ok(n) if n > 0 => self.meter.labels(Label::NoUse.as_str(), n),
                _ => {}
            }
            for table in TABLES {
                if self.purge(*tenant_id, table).await.is_err() {
                    break;
                }
            }
            if count_unrecalled && let Ok(ages) = self.unrecalled(*tenant_id).await {
                for (sum, n) in unrecalled.iter_mut().zip(ages) {
                    *sum += n;
                }
            }
        }
        self.meter.health(overdue);
        if count_unrecalled {
            self.meter.unrecalled(unrecalled);
            self.last_unrecalled = Some(Instant::now());
        }
        self.last_pass.store(unix_now(), Ordering::Relaxed);
    }

    async fn tenant_tx(
        &self,
        tenant_id: Uuid,
    ) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    /// Whether the tenant holds an event older than the 30 days and one pass:
    /// the purge is behind, which doctor names.
    async fn lagging(&self, tenant_id: Uuid) -> Result<bool, sqlx::Error> {
        let mut tx = self.tenant_tx(tenant_id).await?;
        let stale: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM recall_events WHERE tenant_id = $1 \
               AND created_at <= now() - make_interval(days => $2, secs => $3))",
        )
        .bind(tenant_id)
        .bind(i32::try_from(RETENTION_DAYS).unwrap_or(30))
        .bind(PASS_SECONDS as f64)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(stale)
    }

    /// `no_use` on the tenant's recalls created after its high-water mark
    /// (24 hours back at the first pass) and at least 30 minutes ago with no
    /// positive label, read by the `(tenant_id, created_at)` index; the mark
    /// moves to the pass's bound.
    async fn no_use(&mut self, tenant_id: Uuid) -> Result<u64, sqlx::Error> {
        let mut tx = self.tenant_tx(tenant_id).await?;
        let mark = self.no_use_marks.get(&tenant_id).cloned();
        let row = sqlx::query(&format!(
            "WITH bound AS (SELECT now() - make_interval(secs => $2) AS upto), \
             labelled AS ( \
               INSERT INTO recall_outcomes \
                   (tenant_id, recall_id, created_at, artifact_id, label, source, at) \
               SELECT e.tenant_id, e.recall_id, e.created_at, NULL, 'no_use', 'inferred', now() \
               FROM recall_events e, bound \
               WHERE e.tenant_id = $1 \
                 AND e.created_at > COALESCE($4::timestamptz, now() - make_interval(secs => $3)) \
                 AND e.created_at <= bound.upto \
                 AND NOT EXISTS (SELECT 1 FROM recall_outcomes o \
                      WHERE o.tenant_id = e.tenant_id AND o.recall_id = e.recall_id \
                        AND o.created_at = e.created_at \
                        AND o.label IN ('get', 'derived', 'superseded', 'feedback_useful')) \
               ON CONFLICT DO NOTHING RETURNING 1) \
             SELECT (SELECT count(*) FROM labelled) AS n, \
                    to_char((SELECT upto FROM bound) AT TIME ZONE 'UTC', {AT}) AS upto"
        ))
        .bind(tenant_id)
        .bind(NO_USE_AFTER_SECS as f64)
        .bind(LABEL_WINDOW_SECS as f64)
        .bind(mark)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        let n: i64 = row.try_get("n")?;
        let upto: String = row.try_get("upto")?;
        self.no_use_marks.insert(tenant_id, upto);
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Deletes the tenant's rows of `table` older than the 30 days, at most
    /// [`PURGE_BATCH`] a transaction, each committed before the next, through
    /// the `(tenant_id, created_at)` index.
    async fn purge(&self, tenant_id: Uuid, table: &str) -> Result<u64, sqlx::Error> {
        let mut total = 0;
        loop {
            let mut tx = self.tenant_tx(tenant_id).await?;
            let gone = sqlx::query(&format!(
                "DELETE FROM {table} t USING ( \
                   SELECT tenant_id, recall_id, created_at FROM {table} \
                    WHERE tenant_id = $1 \
                      AND created_at <= now() - make_interval(days => $2) \
                    LIMIT $3) d \
                 WHERE t.tenant_id = d.tenant_id AND t.recall_id = d.recall_id \
                   AND t.created_at = d.created_at"
            ))
            .bind(tenant_id)
            .bind(i32::try_from(RETENTION_DAYS).unwrap_or(30))
            .bind(PURGE_BATCH)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            tx.commit().await?;
            total += gone;
            if gone < u64::try_from(PURGE_BATCH).unwrap_or(u64::MAX) {
                return Ok(total);
            }
        }
    }

    /// The tenant's served memories no recall of the 30 days returned, by
    /// how long ago they were promoted.
    async fn unrecalled(&self, tenant_id: Uuid) -> Result<[i64; 4], sqlx::Error> {
        let mut tx = self.tenant_tx(tenant_id).await?;
        let rows = sqlx::query(
            "WITH recalled AS ( \
               SELECT DISTINCT unnest(hit_ids) AS id FROM recall_events \
                WHERE tenant_id = $1 \
                  AND created_at > now() - make_interval(days => $2)) \
             SELECT CASE \
                      WHEN m.promoted_at > now() - interval '7 days' THEN 0 \
                      WHEN m.promoted_at > now() - interval '30 days' THEN 1 \
                      WHEN m.promoted_at > now() - interval '90 days' THEN 2 \
                      ELSE 3 END AS age, count(*) AS n \
             FROM memories m \
             WHERE m.tenant_id = $1 AND m.status = 'promoted' AND m.is_tip \
               AND NOT m.retirement AND m.kind IN ('memory', 'derived_fact') \
               AND NOT EXISTS (SELECT 1 FROM recalled r WHERE r.id = m.id) \
             GROUP BY 1",
        )
        .bind(tenant_id)
        .bind(i32::try_from(RETENTION_DAYS).unwrap_or(30))
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let mut ages = [0_i64; 4];
        for row in rows {
            let age: i32 = row.try_get("age")?;
            let n: i64 = row.try_get("n")?;
            if let Some(slot) = usize::try_from(age).ok().and_then(|a| ages.get_mut(a)) {
                *slot += n;
            }
        }
        Ok(ages)
    }
}

async fn insert_event(
    tx: &mut Transaction<'_, Postgres>,
    e: &RecallEvent,
) -> Result<(), sqlx::Error> {
    let digest = e
        .query_digest
        .strip_prefix("sha256:")
        .and_then(|h| hex::decode(h).ok())
        .unwrap_or_default();
    let (ids, scores): (Vec<Uuid>, Vec<f32>) = e.hits.iter().copied().unzip();
    sqlx::query(
        "INSERT INTO recall_events (tenant_id, recall_id, created_at, key_id, principal, \
             scope_workarea_id, source_workarea_id, pipeline, access, query_digest, \
             query_text, hit_ids, hit_scores, sources, federated_workarea_ids, \
             reference_workarea_ids) \
         VALUES ($1, $2, $3::timestamptz, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
    )
    .bind(e.tenant_id)
    .bind(e.recall_id)
    .bind(e.created_at.as_str())
    .bind(e.key_id)
    .bind(&e.principal)
    .bind(e.scope_workarea_id)
    .bind(e.source_workarea_id)
    .bind(e.pipeline)
    .bind(e.access)
    .bind(digest)
    .bind(&e.query_text)
    .bind(ids)
    .bind(scores)
    .bind(e.federation.as_ref().map(|f| f.sources.clone()))
    .bind(e.federation.as_ref().map(|f| f.federated_workarea_ids.clone()))
    .bind(e.federation.as_ref().map(|f| f.reference_workarea_ids.clone()))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn requery(tx: &mut Transaction<'_, Postgres>, e: &RecallEvent) -> Result<u64, sqlx::Error> {
    let digest = e
        .query_digest
        .strip_prefix("sha256:")
        .and_then(|h| hex::decode(h).ok())
        .unwrap_or_default();
    Ok(sqlx::query(
        "INSERT INTO recall_outcomes \
             (tenant_id, recall_id, created_at, artifact_id, label, source, at) \
         SELECT p.tenant_id, p.recall_id, p.created_at, NULL, 'requery', 'inferred', \
                $5::timestamptz \
         FROM recall_events p \
         WHERE p.tenant_id = $1 AND p.key_id = $2 AND p.scope_workarea_id = $3 \
           AND p.recall_id <> $4 \
           AND p.created_at < $5::timestamptz \
           AND p.created_at >= $5::timestamptz - make_interval(secs => $6) \
           AND p.query_digest <> $7 \
           AND NOT EXISTS (SELECT 1 FROM recall_outcomes o \
                WHERE o.tenant_id = p.tenant_id AND o.recall_id = p.recall_id \
                  AND o.created_at = p.created_at \
                  AND o.label IN ('get', 'derived', 'superseded', 'feedback_useful')) \
         ORDER BY p.created_at DESC LIMIT 1 \
         ON CONFLICT DO NOTHING",
    )
    .bind(e.tenant_id)
    .bind(e.key_id)
    .bind(e.scope_workarea_id)
    .bind(e.recall_id)
    .bind(e.created_at.as_str())
    .bind(REQUERY_WINDOW_SECS as f64)
    .bind(digest)
    .execute(&mut **tx)
    .await?
    .rows_affected())
}

/// An inferred label on `key_id`'s most recent event, inside the label's
/// window before `at`, whose hits hold `artifact_id`; false when no event
/// explains the act, which labels nothing (ADR-0054 d5).
async fn attach(
    tx: &mut Transaction<'_, Postgres>,
    key_id: &Uuid,
    artifact_id: Uuid,
    label: Label,
    at: &EventTime,
) -> Result<bool, sqlx::Error> {
    let window = label.window_secs().unwrap_or(0);
    Ok(sqlx::query(
        "INSERT INTO recall_outcomes \
             (tenant_id, recall_id, created_at, artifact_id, label, source, at) \
         SELECT e.tenant_id, e.recall_id, e.created_at, $2, $3, 'inferred', $4::timestamptz \
         FROM recall_events e \
         WHERE e.tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid \
           AND e.key_id = $1 AND $2 = ANY (e.hit_ids) \
           AND e.created_at <= $4::timestamptz \
           AND e.created_at >= $4::timestamptz - make_interval(secs => $5) \
         ORDER BY e.created_at DESC LIMIT 1 \
         ON CONFLICT DO NOTHING",
    )
    .bind(key_id)
    .bind(artifact_id)
    .bind(label.as_str())
    .bind(at.as_str())
    .bind(window as f64)
    .execute(&mut **tx)
    .await?
    .rows_affected()
        > 0)
}

async fn feedback(
    tx: &mut Transaction<'_, Postgres>,
    f: &FeedbackLabels,
) -> Result<Vec<(Label, u64)>, sqlx::Error> {
    let mut written = Vec::new();
    for (label, ids) in [
        (Label::FeedbackUseful, &f.useful),
        (Label::FeedbackNotUseful, &f.not_useful),
    ] {
        let mut n = 0;
        for id in ids {
            n += sqlx::query(
                "INSERT INTO recall_outcomes \
                     (tenant_id, recall_id, created_at, artifact_id, label, source, at) \
                 VALUES ($1, $2, $3::timestamptz, $4, $5, 'explicit', $6::timestamptz) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(f.tenant_id)
            .bind(f.recall_id)
            .bind(f.created_at.as_str())
            .bind(id)
            .bind(label.as_str())
            .bind(f.at.as_str())
            .execute(&mut **tx)
            .await?
            .rows_affected();
        }
        written.push((label, n));
    }
    Ok(written)
}

/// Whether a served memory of the workarea, other than this one, holds
/// exactly this text (ADR-0054 d12, as accepted).
async fn duplicate(
    tx: &mut Transaction<'_, Postgres>,
    r: &Remembered,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM memories m \
          WHERE m.tenant_id = $1 AND m.workarea_id = $2 AND m.status = 'promoted' \
            AND m.is_tip AND NOT m.retirement AND m.claim_digest = $3 AND m.id <> $4)",
    )
    .bind(r.tenant_id)
    .bind(r.workarea_id)
    .bind(
        r.body_digest
            .strip_prefix("sha256:")
            .and_then(|h| hex::decode(h).ok())
            .unwrap_or_default(),
    )
    .bind(r.memory_id)
    .fetch_one(&mut **tx)
    .await
}

/// The tenants whose `recall_events` partition exists, read from the
/// catalog, which names each by its id alone.
async fn telemetry_tenants(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_inherits i JOIN pg_class c ON c.oid = i.inhrelid \
         WHERE i.inhparent = 'recall_events'::regclass",
    )
    .fetch_all(pool)
    .await?;
    Ok(names
        .iter()
        .filter_map(|n| n.strip_prefix("recall_events_t_"))
        .filter_map(|hex| Uuid::try_parse(hex).ok())
        .collect())
}

// ---------------------------------------------------------------------------
// The partitions: the owner's DDL, with the tenant (ADR-0034 d11; ADR-0054 d2)
// ---------------------------------------------------------------------------

const POLICY: &str = "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) \
                      WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)";

fn tenant_partition(table: &str, tenant_id: Uuid) -> String {
    format!("{table}_t_{}", tenant_id.simple())
}

/// The DDL of `tenant_id`'s two telemetry partitions, fenced before each is
/// attached, with no statistics on the columns that hold a reader's words or
/// names, as 0023 builds every existing tenant's.
pub(crate) fn tenant_partitions_ddl(tenant_id: Uuid) -> Vec<String> {
    TABLES
        .iter()
        .flat_map(|table| {
            let p = tenant_partition(table, tenant_id);
            let stats = if *table == "recall_events" {
                format!(
                    "ALTER TABLE {p} ALTER COLUMN query_text SET STATISTICS 0, \
                     ALTER COLUMN query_digest SET STATISTICS 0, \
                     ALTER COLUMN principal SET STATISTICS 0, \
                     ALTER COLUMN hit_ids SET STATISTICS 0"
                )
            } else {
                format!("ALTER TABLE {p} ALTER COLUMN artifact_id SET STATISTICS 0")
            };
            [
                format!("CREATE TABLE {p} (LIKE {table} INCLUDING ALL)"),
                stats,
                format!("ALTER TABLE {p} ENABLE ROW LEVEL SECURITY"),
                format!("ALTER TABLE {p} FORCE ROW LEVEL SECURITY"),
                format!("CREATE POLICY tenant_isolation ON {p} {POLICY}"),
                format!("ALTER TABLE {table} ATTACH PARTITION {p} FOR VALUES IN ('{tenant_id}')"),
            ]
        })
        .collect()
}

/// `tenant_id`'s telemetry partitions where missing, every statement in the
/// caller's owner transaction; whether it created them.
pub(crate) async fn ensure_tenant(
    owner: &mut PgConnection,
    tenant_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(tenant_partition("recall_events", tenant_id))
        .fetch_one(&mut *owner)
        .await?;
    if exists {
        return Ok(false);
    }
    for statement in tenant_partitions_ddl(tenant_id) {
        sqlx::query(&statement)
            .persistent(false)
            .execute(&mut *owner)
            .await?;
    }
    Ok(true)
}

/// At the lead's start, after migrating: the telemetry partitions of every
/// tenant whose memories partition exists and whose telemetry ones do not,
/// as a lead built before 0023 leaves a tenant it creates; each tenant in an
/// owner transaction of its own, so one lock timeout stops only that one.
/// Boot-time DDL alone, as ADR-0034 d4 allows; nothing runs on a schedule.
/// Returns the tenants repaired.
pub(crate) async fn ensure_partitions(owner: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = owner.begin().await?;
    sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
        .execute(&mut *tx)
        .await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM tenants ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut repaired = 0;
    for tenant_id in tenants {
        let has_memories: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("memories_t_{}", tenant_id.simple()))
            .fetch_one(owner)
            .await?;
        if !has_memories {
            continue;
        }
        let mut tx = owner.begin().await?;
        sqlx::query(&format!(
            "SET LOCAL lock_timeout = '{}'",
            crate::PARTITION_LOCK_TIMEOUT
        ))
        .persistent(false)
        .execute(&mut *tx)
        .await?;
        if ensure_tenant(&mut tx, tenant_id).await? {
            repaired += 1;
        }
        tx.commit().await?;
    }
    Ok(repaired)
}
