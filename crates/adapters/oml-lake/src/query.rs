//! The operator's named lake queries (ADR-0058 d11): how the system is
//! used, answered from the lake by DataFusion inside `oml-lakewriter`, under
//! the lake's own file permissions. No server route reaches them and no
//! client reads the lake (ADR-0013 d1); the same SQL runs in DuckDB or Spark
//! against the same paths (docs/operations/lake.md).
//!
//! Each tenant's table is registered under a numbered name and the tenants
//! are joined by a `UNION ALL` view named after the table, so every query
//! carries `tenant_id` and no query can mix tenants by forgetting a filter.

use deltalake::arrow::record_batch::RecordBatch;
use deltalake::datafusion::prelude::SessionContext;

use crate::table::{BENCH_RESULTS, BRONZE_EVENTS, GOLD_LINEAGE, RECALL_DAILY, SILVER_CLAIMS, Spec};
use crate::{Lake, LakeError, Prefix};

/// One named query: its name, what it answers, and its SQL over the views
/// `events`, `claims`, `lineage`, `recall_daily` and `bench`.
#[derive(Debug, Clone, Copy)]
pub struct Named {
    pub name: &'static str,
    pub answers: &'static str,
    pub sql: &'static str,
}

/// The queries `oml-lakewriter query <name>` runs, in the order `--list`
/// prints them.
pub const QUERIES: &[Named] = &[
    Named {
        name: "writes-per-kind-per-day",
        answers: "How many claims were written each day, by kind",
        sql: "SELECT tenant_id, event_date AS day, coalesce(kind, 'memory') AS kind, count(*) AS writes \
              FROM events WHERE event_type IN ('submission.remember', 'submission.learn') \
              GROUP BY tenant_id, event_date, coalesce(kind, 'memory') \
              ORDER BY day, tenant_id, kind",
    },
    Named {
        name: "review-latency",
        answers: "How long a written claim waited for promotion, per promotion day (seconds)",
        sql: "SELECT tenant_id, to_date(promoted_at) AS day, count(*) AS promoted, \
              round(min(secs), 1) AS min_s, round(median(secs), 1) AS median_s, \
              round(avg(secs), 1) AS mean_s, round(max(secs), 1) AS max_s \
              FROM (SELECT tenant_id, promoted_at, \
                    (arrow_cast(promoted_at, 'Int64') - arrow_cast(created_at, 'Int64')) / 1000000.0 AS secs \
                    FROM claims) \
              GROUP BY tenant_id, to_date(promoted_at) ORDER BY day, tenant_id",
    },
    Named {
        name: "recall-volume-per-pipeline",
        answers: "How many recalls each search pipeline answered per day, and how many came back empty",
        sql: "SELECT tenant_id, day, pipeline, sum(recalls) AS recalls, sum(empty_recalls) AS empty, \
              sum(hits) AS hits FROM recall_daily \
              GROUP BY tenant_id, day, pipeline ORDER BY day, tenant_id, pipeline",
    },
    Named {
        name: "recall-outcomes",
        answers: "What readers did after a recall, per pipeline over the retained 30 days",
        sql: "SELECT tenant_id, pipeline, sum(recalls) AS recalls, sum(outcome_get) AS got, \
              sum(outcome_derived) AS derived, sum(outcome_superseded) AS superseded, \
              sum(outcome_feedback_useful) AS useful, sum(outcome_feedback_not_useful) AS not_useful, \
              sum(outcome_requery) AS requery, sum(outcome_no_use) AS no_use \
              FROM recall_daily GROUP BY tenant_id, pipeline ORDER BY tenant_id, pipeline",
    },
    Named {
        name: "stale-share",
        answers: "What share of promoted versions are no longer the served tip, by kind",
        sql: "SELECT tenant_id, kind, count(*) AS versions, \
              sum(CASE WHEN state = 'promoted' THEN 0 ELSE 1 END) AS stale, \
              round(sum(CASE WHEN state = 'promoted' THEN 0 ELSE 1 END) * 1.0 / count(*), 3) AS stale_share \
              FROM claims GROUP BY tenant_id, kind ORDER BY tenant_id, kind",
    },
    Named {
        name: "top-workareas",
        answers: "Which workareas see the most journalled activity, with their names",
        sql: "SELECT e.tenant_id, e.workarea_id, max(n.name) AS name, count(*) AS events, \
              sum(CASE WHEN e.event_type LIKE 'submission.%' THEN 1 ELSE 0 END) AS writes, \
              sum(CASE WHEN e.event_type IN ('lifecycle.promote', 'lifecycle.supersede') THEN 1 ELSE 0 END) AS promotions \
              FROM events e LEFT JOIN \
                (SELECT tenant_id, workarea_id, subject_name AS name FROM events \
                 WHERE event_type = 'workarea.create') n \
                ON n.tenant_id = e.tenant_id AND n.workarea_id = e.workarea_id \
              WHERE e.workarea_id IS NOT NULL \
              GROUP BY e.tenant_id, e.workarea_id ORDER BY events DESC, e.workarea_id LIMIT 20",
    },
    Named {
        name: "bench-latest",
        answers: "Each benchmark suite's methods on their overall metrics, latest run first",
        sql: "SELECT suite, method, run_date, metric, value FROM bench \
              WHERE category = '' ORDER BY suite, run_date DESC, method, metric",
    },
];

/// The named query `name`.
#[must_use]
pub fn named(name: &str) -> Option<&'static Named> {
    QUERIES.iter().find(|q| q.name == name)
}

/// A session over the whole lake: the views `events`, `claims`, `lineage`,
/// `recall_daily` and `bench`, each empty when no prefix holds that table.
///
/// # Errors
/// A table cannot be opened or registered.
pub async fn session(lake: &Lake) -> Result<SessionContext, LakeError> {
    let ctx = SessionContext::new();
    let tenants = lake.tenants()?;
    let views: [(&str, &Spec); 4] = [
        ("events", &BRONZE_EVENTS),
        ("claims", &SILVER_CLAIMS),
        ("lineage", &GOLD_LINEAGE),
        ("recall_daily", &RECALL_DAILY),
    ];
    for (view, spec) in views {
        let mut parts = Vec::new();
        for (i, t) in tenants.iter().enumerate() {
            if let Some(table) = lake.open(Prefix::Tenant(*t), spec).await? {
                let name = format!("{view}_{i}");
                ctx.register_table(name.as_str(), table.table_provider().await?)?;
                parts.push(format!("SELECT * FROM {name}"));
            }
        }
        register_view(&ctx, view, spec, &parts).await?;
    }
    let mut parts = Vec::new();
    if let Some(table) = lake.open(Prefix::Provider, &BENCH_RESULTS).await? {
        ctx.register_table("bench_0", table.table_provider().await?)?;
        parts.push("SELECT * FROM bench_0".to_owned());
    }
    register_view(&ctx, "bench", &BENCH_RESULTS, &parts).await?;
    Ok(ctx)
}

async fn register_view(
    ctx: &SessionContext,
    view: &str,
    spec: &Spec,
    parts: &[String],
) -> Result<(), LakeError> {
    if parts.is_empty() {
        let empty = RecordBatch::new_empty(spec.arrow_schema());
        ctx.register_batch(view, empty)?;
    } else {
        ctx.sql(&format!(
            "CREATE VIEW {view} AS {}",
            parts.join(" UNION ALL ")
        ))
        .await?;
    }
    Ok(())
}

/// Runs `sql` over [`session`].
///
/// # Errors
/// The session cannot be built or the SQL fails.
pub async fn run(lake: &Lake, sql: &str) -> Result<Vec<RecordBatch>, LakeError> {
    let ctx = session(lake).await?;
    Ok(ctx.sql(sql).await?.collect().await?)
}

/// The batches as a text table.
///
/// # Errors
/// A column cannot be formatted.
pub fn pretty(batches: &[RecordBatch]) -> Result<String, LakeError> {
    deltalake::arrow::util::pretty::pretty_format_batches(batches)
        .map(|t| t.to_string())
        .map_err(|e| LakeError::Rows(e.to_string()))
}

/// Rows across the batches.
#[must_use]
pub fn rows(batches: &[RecordBatch]) -> usize {
    batches.iter().map(RecordBatch::num_rows).sum()
}
