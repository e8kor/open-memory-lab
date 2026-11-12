//! The recall meter's queue and writer (ADR-0071 d2, d8).
//!
//! A recall hands its count to a bounded queue and never waits: one task
//! takes what the queue holds, adds the counts of each account's day, and
//! writes each sum in a transaction of its own under the tenant's row
//! security. So no recall sits behind its account's counter row, and
//! recalls that arrive together are one write. A full queue or a store
//! error loses the counts it names, told to the caller's [`Lost`], which
//! the lead logs as `usage_recall_counts_lost` and counts on its metrics
//! listener; the month's `recalls` then reads low and nothing recomputes
//! it. A graceful stop writes what the queue still holds first
//! ([`PgRecallMeter::flush_at_stop`]) and tells of what it had to leave.
//!
//! Every count handed to the meter is one of three things: written, told
//! as lost, or still `unaccounted`. A stop that cannot wait for the writer
//! tells what is still unaccounted, so the lost counts it and the writer
//! have told sum to what was accepted and never written, none twice.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

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
    /// Counts handed to the meter that have been neither written nor told
    /// of as lost: queued, dropped by a full queue and not yet told, in the
    /// pass in flight, or refused and not yet told. What a stop that cannot
    /// wait leaves unwritten.
    unaccounted: Arc<AtomicU64>,
    lost: Lost,
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
        let unaccounted = Arc::new(AtomicU64::new(0));
        let meter = Arc::new(Self {
            queue,
            dropped: Arc::clone(&dropped),
            unaccounted: Arc::clone(&unaccounted),
            lost: Arc::clone(&lost),
        });
        (
            meter,
            tokio::spawn(write(store, rx, dropped, unaccounted, lost)),
        )
    }

    /// The lead's graceful stop: writes the counts the queue holds, then
    /// answers. Unlike [`RecallMeter::flush`], which a read calls and which
    /// never waits for queue room, this waits up to `within` for the
    /// writer. When the writer does not answer in time the counts accepted
    /// and neither written nor yet told of as lost, the ones a full queue
    /// dropped and the ones the store refused in the pass in flight
    /// included once the writer has told of them, are told to the meter's
    /// [`Lost`] and returned: they end with the process. The writer tells
    /// of a refused sum as it fails, so those are not counted again here.
    ///
    /// # Errors
    /// The number of counts this stop told as lost. It is not the run's
    /// total: counts the writer told earlier, as dropped by a full queue or
    /// refused by the store, were told then and are not in it.
    pub async fn flush_at_stop(&self, within: Duration) -> Result<(), u64> {
        let (done, wait) = oneshot::channel();
        let written = tokio::time::timeout(within, async {
            self.queue.send(Msg::Flush(done)).await.is_ok() && wait.await.is_ok()
        })
        .await;
        if written == Ok(true) {
            return Ok(());
        }
        // Claims what the writer has not told of; a late writer then finds
        // nothing of these to tell a second time.
        self.dropped.swap(0, Ordering::Relaxed);
        let unwritten = self.unaccounted.swap(0, Ordering::Relaxed);
        if unwritten > 0 {
            (self.lost)(
                unwritten,
                "the lead stopped before the writer had written them",
            );
        }
        Err(unwritten)
    }
}

impl RecallMeter for PgRecallMeter {
    fn count(&self, tenant_id: Uuid, user_root: Uuid, at: &EventTime) {
        let msg = Msg::Count {
            tenant_id,
            user_root,
            day: day_of(at).to_owned(),
        };
        // Unaccounted before it is queued, so the writer never takes a count
        // the stop does not know of; a dropped one stays so until the writer
        // tells of it.
        self.unaccounted.fetch_add(1, Ordering::Relaxed);
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

/// Takes up to `n` counts off the unaccounted total and returns how many it
/// took: fewer than `n` when a stop has already claimed them and told of
/// them, so a count is told of once.
fn account(unaccounted: &AtomicU64, n: u64) -> u64 {
    let before = unaccounted
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |u| {
            Some(u.saturating_sub(n))
        })
        .unwrap_or_default();
    before.min(n)
}

/// Writes the queue until every sender is gone: each pass takes what is
/// queued now, sums it by account and day, writes each sum, and then
/// answers the flushes it took.
async fn write(
    store: PgStore,
    mut queue: mpsc::Receiver<Msg>,
    dropped: Arc<AtomicU64>,
    unaccounted: Arc<AtomicU64>,
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
        let full = dropped.swap(0, Ordering::Relaxed);
        if full > 0 {
            let taken = account(&unaccounted, full);
            if taken > 0 {
                tell(taken, "the queue was full");
            }
        }
        for ((tenant_id, user_root, day), count) in counts {
            // Written, or told of as lost as it fails: no longer the stop's
            // to name.
            match store.meter_recalls(tenant_id, user_root, &day, count).await {
                Ok(()) => {
                    account(&unaccounted, count);
                }
                Err(e) => {
                    let taken = account(&unaccounted, count);
                    if taken > 0 {
                        tell(taken, &e.to_string());
                    }
                }
            }
        }
        for done in flushes {
            let _ = done.send(());
        }
    }
}
