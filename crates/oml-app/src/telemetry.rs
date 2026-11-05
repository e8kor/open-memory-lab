//! Recall telemetry, the measurement half of ADR-0054 as accepted: what a
//! recall records, the labels of what its reader did next, and the port the
//! lead's writer implements. A recall hands its event to the port without
//! waiting, and the port writes it off the recall's path (d2); nothing here
//! is a claim, and nothing here reaches a ranking (d12).

use core::future::Future;
use core::pin::Pin;

use uuid::Uuid;

use oml_domain::EventTime;

use crate::ports::StoreError;

/// How many served hits an event keeps, in rank order (ADR-0054 d2).
pub const HITS_STORED: usize = 20;

/// The most ids one `feedback` member names, `useful` and `not_useful`
/// together (ADR-0054 d6).
pub const FEEDBACK_IDS_MAX: usize = 20;

/// How long after its recall a `recall_id` takes feedback, and the oldest
/// event any label attaches to (ADR-0054 d5, d6).
pub const LABEL_WINDOW_SECS: i64 = 24 * 3600;

/// A key's explicit labels past this many in a UTC day answer `limit.rate`
/// (ADR-0054 d6, as accepted).
pub const EXPLICIT_LABELS_PER_DAY: u64 = 1_000;

/// A key's feedback members checked in a UTC day, refused or not, past which
/// a read carrying one answers `limit.rate` before any lookup (ADR-0054 d6,
/// as amended on review).
pub const FEEDBACK_PER_KEY_PER_DAY: u64 = 1_000;

/// The most labelled recalls one replay runs, the newest, and the most
/// seconds it runs before it answers what it scored (ADR-0054 d11, as
/// amended on review).
pub const REPLAY_QUERIES_MAX: usize = 500;
pub const REPLAY_DEADLINE_SECS: u64 = 60;

/// A key's inferred labels past this many in a UTC day are dropped and
/// counted (ADR-0054 d5, as accepted).
pub const INFERRED_LABELS_PER_DAY: u64 = 1_000;

/// The windows inside which an act labels the acting key's recall: a get,
/// a derivation, a supersession, a second recall in other words; and how
/// long a recall waits before it is labelled `no_use` (ADR-0054 d5). Starting
/// values, which FR-105 measures.
pub const GET_WINDOW_SECS: i64 = 10 * 60;
pub const DERIVED_WINDOW_SECS: i64 = 24 * 3600;
pub const SUPERSEDED_WINDOW_SECS: i64 = 24 * 3600;
pub const REQUERY_WINDOW_SECS: i64 = 2 * 60;
pub const NO_USE_AFTER_SECS: i64 = 30 * 60;

/// Every event and label is purged this many days after its recall
/// (ADR-0054 d2).
pub const RETENTION_DAYS: i64 = 30;

/// What a reader did next, as `recall_outcomes.label` and the
/// `oml_recall_outcomes_total` label spell it (ADR-0054 d5, d6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Label {
    Get,
    Derived,
    Superseded,
    FeedbackUseful,
    FeedbackNotUseful,
    Requery,
    NoUse,
}

impl Label {
    pub const ALL: [Self; 7] = [
        Self::Get,
        Self::Derived,
        Self::Superseded,
        Self::FeedbackUseful,
        Self::FeedbackNotUseful,
        Self::Requery,
        Self::NoUse,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Derived => "derived",
            Self::Superseded => "superseded",
            Self::FeedbackUseful => "feedback_useful",
            Self::FeedbackNotUseful => "feedback_not_useful",
            Self::Requery => "requery",
            Self::NoUse => "no_use",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == s)
    }

    /// The graded relevance a label gives its version in the replay:
    /// `feedback_useful` and `derived` 2, `get` and `superseded` 1,
    /// `feedback_not_useful` 0; a recall-level label grades nothing
    /// (ADR-0054 d11).
    #[must_use]
    pub const fn grade(self) -> Option<u8> {
        match self {
            Self::FeedbackUseful | Self::Derived => Some(2),
            Self::Get | Self::Superseded => Some(1),
            Self::FeedbackNotUseful => Some(0),
            Self::Requery | Self::NoUse => None,
        }
    }

    /// Whether the label is inferred from an act within a window, rather
    /// than sent (ADR-0054 d5).
    #[must_use]
    pub const fn window_secs(self) -> Option<i64> {
        match self {
            Self::Get => Some(GET_WINDOW_SECS),
            Self::Derived => Some(DERIVED_WINDOW_SECS),
            Self::Superseded => Some(SUPERSEDED_WINDOW_SECS),
            _ => None,
        }
    }
}

/// One recall as `recall_events` stores it (ADR-0054 d2). `query_text` is
/// the NFC query, always handed over; the writer keeps it only where the
/// tenant opted in before `created_at` (d3).
#[derive(Debug, Clone, PartialEq)]
pub struct RecallEvent {
    pub tenant_id: Uuid,
    pub recall_id: Uuid,
    pub created_at: EventTime,
    pub key_id: Uuid,
    pub principal: String,
    pub scope_workarea_id: Uuid,
    pub source_workarea_id: Option<Uuid>,
    pub pipeline: &'static str,
    pub access: &'static str,
    /// The request's `query_digest`, `sha256:` and 64 hex characters.
    pub query_digest: String,
    pub query_text: String,
    /// The served hits in rank order, at most [`HITS_STORED`].
    pub hits: Vec<(Uuid, f32)>,
}

/// A key's committed act that may label its recent recall: a get by id, a
/// write citing the version, or a supersession of it (ADR-0054 d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Act {
    pub tenant_id: Uuid,
    pub key_id: Uuid,
    pub artifact_id: Uuid,
    pub label: Label,
    pub at: EventTime,
}

/// A validated `feedback` member: the labels attach to the event exactly,
/// by its id and its `created_at` (ADR-0054 d6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackLabels {
    pub tenant_id: Uuid,
    pub recall_id: Uuid,
    pub created_at: EventTime,
    pub useful: Vec<Uuid>,
    pub not_useful: Vec<Uuid>,
    pub at: EventTime,
}

/// A memory written, for the remember side's duplicate count: its body
/// digest compared with the workarea's served memories (ADR-0054 d12, as
/// accepted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remembered {
    pub tenant_id: Uuid,
    pub workarea_id: Uuid,
    pub memory_id: Uuid,
    pub body_digest: String,
}

/// What the app hands the writer, never waiting on it.
#[derive(Debug, Clone, PartialEq)]
pub enum TelemetryItem {
    Recall(Box<RecallEvent>),
    Act(Act),
    Feedback(FeedbackLabels),
    Remembered(Remembered),
}

/// The `feedback` member of a read (ADR-0054 d6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Feedback {
    pub recall_id: Uuid,
    pub useful: Vec<Uuid>,
    pub not_useful: Vec<Uuid>,
}

/// A stored event the feedback check reads: when it was created and what
/// it served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedRecall {
    pub created_at: EventTime,
    pub hit_ids: Vec<Uuid>,
}

/// One label as `GET /v2/usage/events` serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLabel {
    pub artifact_id: Option<Uuid>,
    pub label: String,
    pub source: String,
    pub at: EventTime,
}

/// One event as `GET /v2/usage/events` serves it to a tenant admin.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    pub recall_id: Uuid,
    pub created_at: EventTime,
    pub key_id: Uuid,
    pub principal: String,
    pub scope_workarea_id: Uuid,
    pub pipeline: String,
    pub access: String,
    pub query_digest: String,
    pub query_text: Option<String>,
    pub hits: Vec<(Uuid, f32)>,
    pub labels: Vec<StoredLabel>,
}

/// A labelled query of the real-usage set: an event whose text is stored,
/// with each labelled version's grade (ADR-0054 d11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelledQuery {
    pub key_id: Uuid,
    pub scope_workarea_id: Uuid,
    pub query_text: String,
    pub grades: Vec<(Uuid, u8)>,
}

/// A future a [`Telemetry`] read answers.
pub type TelemetryFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;

/// The telemetry port: a bounded queue the app offers to without waiting,
/// and the reads the feedback check, the admin's event listing and the
/// replay make, each under the tenant's row security. Held as `Arc<dyn
/// Telemetry>`, as the reranker is: a deployment may run without one, and
/// then a recall records nothing.
pub trait Telemetry: Send + Sync {
    /// Queues `item`; a full queue drops it and counts it, and never blocks.
    fn offer(&self, item: TelemetryItem);

    /// Writes what the queue holds now, then answers.
    fn flush(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// The tenant's event `recall_id` of `key_id`, created within
    /// [`LABEL_WINDOW_SECS`]; `None` for one the key did not issue, or
    /// older.
    fn served(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
        recall_id: Uuid,
    ) -> TelemetryFuture<'_, Option<ServedRecall>>;

    /// The explicit labels on `key_id`'s events since the start of today,
    /// UTC.
    fn explicit_labels_today(&self, tenant_id: Uuid, key_id: Uuid) -> TelemetryFuture<'_, u64>;

    /// The tenant's events of the last [`RETENTION_DAYS`], newest first,
    /// before `before` (`created_at`, `recall_id`) when given, at most
    /// `limit`, each with its labels.
    fn events(
        &self,
        tenant_id: Uuid,
        before: Option<(EventTime, Uuid)>,
        limit: u32,
    ) -> TelemetryFuture<'_, Vec<StoredEvent>>;

    /// The tenant's labelled queries of the last [`RETENTION_DAYS`]
    /// (ADR-0054 d11).
    fn labelled_queries(&self, tenant_id: Uuid) -> TelemetryFuture<'_, Vec<LabelledQuery>>;
}

/// The replay's scores of one pipeline over the labelled queries it ran
/// (ADR-0054 d11): numbers and counts, never a query, a hit or an id.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayScores {
    pub pipeline: crate::search::Pipeline,
    pub queries: u64,
    pub success_at_1: f64,
    pub success_at_5: f64,
    pub ndcg_at_10: f64,
    pub recall_at_10: f64,
}

/// What `POST /v2/usage/eval` answers.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayReport {
    /// The labelled queries in the window.
    pub labelled: u64,
    /// Whether the replay stopped short: past [`REPLAY_QUERIES_MAX`]
    /// queries, the newest ran, or at [`REPLAY_DEADLINE_SECS`].
    pub capped: bool,
    /// Those skipped because their key is revoked or gone.
    pub skipped_revoked: u64,
    pub window_days: i64,
    pub pipelines: Vec<ReplayScores>,
}

/// One query's scores from its ranked ids and its grades: success at 1 and
/// 5 (a version graded above 0 in the head), nDCG at 10 with gain
/// `2^grade − 1`, and recall at 10 over the versions graded above 0.
#[must_use]
pub fn score_query(ranked: &[Uuid], grades: &[(Uuid, u8)]) -> [f64; 4] {
    let grade = |id: &Uuid| {
        grades
            .iter()
            .find(|(g, _)| g == id)
            .map_or(0, |(_, grade)| *grade)
    };
    let hit_in = |k: usize| ranked.iter().take(k).any(|id| grade(id) > 0);
    let gain = |g: u8| f64::from((1_u32 << g) - 1);
    let dcg: f64 = ranked
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, id)| gain(grade(id)) / ((i + 2) as f64).log2())
        .sum();
    let mut ideal: Vec<u8> = grades.iter().map(|(_, g)| *g).collect();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let idcg: f64 = ideal
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, g)| gain(*g) / ((i + 2) as f64).log2())
        .sum();
    let relevant = grades.iter().filter(|(_, g)| *g > 0).count();
    let found = ranked.iter().take(10).filter(|id| grade(id) > 0).count();
    [
        f64::from(u8::from(hit_in(1))),
        f64::from(u8::from(hit_in(5))),
        if idcg > 0.0 { dcg / idcg } else { 0.0 },
        if relevant > 0 {
            found as f64 / relevant as f64
        } else {
            0.0
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_whose_useful_version_ranks_first_scores_one_everywhere() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        assert_eq!(score_query(&[a, b], &[(a, 2)]), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn a_not_useful_version_counts_as_no_hit() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let s = score_query(&[a, b], &[(a, 0), (b, 1)]);
        assert_eq!(s[0], 0.0);
        assert_eq!(s[1], 1.0);
        assert!((s[2] - 1.0 / 3_f64.log2()).abs() < 1e-12, "{s:?}");
        assert_eq!(s[3], 1.0);
    }
}
