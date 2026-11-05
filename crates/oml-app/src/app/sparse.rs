//! The learned sparse encoder's pass (ADR-0052 d15): the lead drains the
//! queue the write's trigger fills, off the commit path, so a write never
//! waits for the model and a version is found by `bm25_postings` from its
//! promotion and by `sparse` once encoded.

use crate::ports::{Retriever, SparseEncoder, StoreError};
use crate::search::{SparsePending, SparsePosting};

/// How many versions written before a tenant's sparse state was turned on
/// one pass queues for it: the backfill is bounded per pass and resumes
/// where it stopped (ADR-0052 d15).
pub const SPARSE_BACKFILL_STEP: usize = 1_000;

/// What one pass did, for the metrics and the log (ADR-0052 d15).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SparsePass {
    /// Versions whose kind 2 postings were written.
    pub written: u64,
    /// Versions whose encode or write failed, kept on the queue.
    pub failed: u64,
    /// Of them, those parked at the attempts' ceiling: dead letters.
    pub parked: u64,
    /// Versions taken off the queue or enqueued again by another writer
    /// meanwhile, so nothing was written for the claim read.
    pub stale: u64,
    /// Versions the backfill queued.
    pub backfilled: u64,
}

impl SparsePass {
    /// Whether the pass found anything to do, so the task runs another at
    /// once rather than waiting.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.written + self.failed + self.stale + self.backfilled > 0
    }
}

/// One pass of the encoder task: for every tenant with work, a bounded
/// backfill step where its state is not on or its backfill unfinished, then
/// up to `per_tenant` due versions, oldest first, encoded together and each
/// written in its own transaction with its version's current state. A text
/// the model fails on fails alone; a call that fails whole is split and
/// each text tried alone, so one bad claim never holds its batch
/// (ADR-0052 d15, reviewed). A free function over the store and the
/// encoder, so the lead's task holds no app.
///
/// # Errors
///
/// The store's failure to read or update the queue; a failure to encode
/// or write one version is not one, it is counted in [`SparsePass::failed`].
pub async fn encode_sparse_pass<R: Retriever>(
    store: &R,
    encoder: &dyn SparseEncoder,
    per_tenant: usize,
) -> Result<SparsePass, StoreError> {
    let mut pass = SparsePass::default();
    for work in store.sparse_tenants().await? {
        let tenant = work.tenant_id;
        if work.backfill {
            let (queued, _) = store.sparse_backfill(tenant, SPARSE_BACKFILL_STEP).await?;
            pass.backfilled += queued;
        }
        let pending = store.sparse_pending(tenant, per_tenant).await?;
        if pending.is_empty() {
            continue;
        }
        let results = encode_each(encoder, &pending).await;
        for (p, result) in pending.iter().zip(results) {
            let outcome = match result {
                Ok(postings) => store.sparse_write(tenant, p, &postings).await,
                Err(reason) => Err(StoreError::Backend(reason)),
            };
            match outcome {
                Ok(true) => pass.written += 1,
                Ok(false) => pass.stale += 1,
                Err(e) => {
                    let reason = match e {
                        StoreError::Backend(r) => r,
                        other => other.to_string(),
                    };
                    if store.sparse_failed(tenant, p, &reason).await? {
                        pass.parked += 1;
                    }
                    pass.failed += 1;
                }
            }
        }
    }
    Ok(pass)
}

/// Each pending version's postings or its own failure: the batch at once,
/// and, when the call fails whole or answers the wrong count, each text
/// alone.
async fn encode_each(
    encoder: &dyn SparseEncoder,
    pending: &[SparsePending],
) -> Vec<Result<Vec<SparsePosting>, String>> {
    let texts: Vec<String> = pending.iter().map(|p| p.claim.clone()).collect();
    let whole = match encoder.encode_documents(texts).await {
        Ok(each) if each.len() == pending.len() => return each,
        Ok(each) => format!(
            "the encoder answered {} postings lists for {} texts",
            each.len(),
            pending.len()
        ),
        Err(reason) => reason,
    };
    if pending.len() == 1 {
        return vec![Err(whole)];
    }
    let mut out = Vec::with_capacity(pending.len());
    for p in pending {
        out.push(
            match encoder.encode_documents(vec![p.claim.clone()]).await {
                Ok(mut one) if one.len() == 1 => one.remove(0),
                Ok(one) => Err(format!(
                    "the encoder answered {} postings lists for 1 text",
                    one.len()
                )),
                Err(reason) => Err(reason),
            },
        );
    }
    out
}
