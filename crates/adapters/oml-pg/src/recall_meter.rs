//! The recall meter's queue and writer (ADR-0071 d2, d8).
//!
//! A recall hands its count to a bounded queue and never waits: one task
//! takes what the queue holds, adds the counts of each account's day, and
//! writes each sum in a transaction of its own under the tenant's row
//! security. So no recall sits behind its account's counter row, and
//! recalls that arrive together are one write. A full queue or a store
//! error loses the counts it names, told to the caller's [`Lost`], which
//! the lead logs as `usage_recall_counts_lost`; the month's `recalls` then
//! reads low and nothing recomputes it.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use oml_app::domain::EventTime;
use oml_app::domain::usage::day_of;
use oml_app::ports::{JournalStore as _, RecallMeter};

use crate::PgStore;

/// The queue's bound: past it a count is dropped, never waited for.
pub const QUEUE: usize = 10_000;

/// What the writer tells of the counts a pass lost: how many, and why.
pub type Lost = Arc<dyn Fn(u64, &str) + Send + Sync>;

enum Msg {
    Count {
        tenant_id: Uuid,
        user_root: Uuid,
        day: String,
    },
    Flush(oneshot::Sender<()>),
}

/// The recall meter over PostgreSQL: the queue the app counts into.
pub struct PgRecallMeter {
    queue: mpsc::Sender<Msg>,
    dropped: Arc<AtomicU64>,
}

impl std::fmt::Debug for PgRecallMeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgRecallMeter").finish_non_exhaustive()
    }
}

impl PgRecallMeter {
    /// The meter over `store` and the task that writes its queue, telling
    /// `lost` of the counts a pass could not write.
    pub fn spawn(store: PgStore, lost: Lost) -> (Arc<Self>, tokio::task::JoinHandle<()>) {
        let (queue, rx) = mpsc::channel(QUEUE);
        let dropped = Arc::new(AtomicU64::new(0));
        let meter = Arc::new(Self {
            queue,
            dropped: Arc::clone(&dropped),
        });
        (meter, tokio::spawn(write(store, rx, dropped, lost)))
    }
}

impl RecallMeter for PgRecallMeter {
    fn count(&self, tenant_id: Uuid, user_root: Uuid, at: &EventTime) {
        let msg = Msg::Count {
            tenant_id,
            user_root,
            day: day_of(at).to_owned(),
        };
        if self.queue.try_send(msg).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn flush(&self) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            // Never waits for queue room: a full queue means the writer is
            // behind, and the caller is a read.
            let (done, wait) = oneshot::channel();
            if self.queue.try_send(Msg::Flush(done)).is_ok() {
                let _ = wait.await;
            }
        })
    }
}

/// Writes the queue until every sender is gone: each pass takes what is
/// queued now, sums it by account and day, writes each sum, and then
/// answers the flushes it took.
async fn write(
    store: PgStore,
    mut queue: mpsc::Receiver<Msg>,
    dropped: Arc<AtomicU64>,
    tell: Lost,
) {
    while let Some(first) = queue.recv().await {
        let mut counts: BTreeMap<(Uuid, Uuid, String), u64> = BTreeMap::new();
        let mut flushes = Vec::new();
        let mut next = Some(first);
        while let Some(msg) = next {
            match msg {
                Msg::Count {
                    tenant_id,
                    user_root,
                    day,
                } => *counts.entry((tenant_id, user_root, day)).or_default() += 1,
                Msg::Flush(done) => flushes.push(done),
            }
            next = queue.try_recv().ok();
        }
        let mut lost = dropped.swap(0, Ordering::Relaxed);
        let mut reason = (lost > 0).then(|| "the queue was full".to_owned());
        for ((tenant_id, user_root, day), count) in counts {
            if let Err(e) = store.meter_recalls(tenant_id, user_root, &day, count).await {
                lost += count;
                reason = Some(e.to_string());
            }
        }
        if let Some(reason) = reason {
            tell(lost, &reason);
        }
        for done in flushes {
            let _ = done.send(());
        }
    }
}
