//! The learned sparse encoder's pass (ADR-0052 d15): the lead drains the
//! queue the write's trigger fills, off the commit path, so a write never
//! waits for the model and a version is found by `bm25_postings` from its
//! promotion and by `sparse` once encoded.

use super::App;
use crate::ports::{Clock, IdGen, JournalStore, Scanner, Signer, StoreError};
use crate::types::AppError;

/// What one pass did, for the metrics and the log (ADR-0052 d15).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SparsePass {
    /// Versions whose kind 2 postings were written.
    pub written: u64,
    /// Versions whose encode or write failed, kept on the queue.
    pub failed: u64,
    /// Versions taken off the queue or enqueued again by another writer
    /// meanwhile, so nothing was written for the claim read.
    pub stale: u64,
}

impl SparsePass {
    /// Whether the pass found anything to do, so the task runs another at
    /// once rather than waiting.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.written + self.failed + self.stale > 0
    }
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// One pass of the encoder task: for every tenant with entries due, up
    /// to `per_tenant` of them, oldest first, encoded together and each
    /// written in its own transaction with its version's current state. A
    /// failed encode keeps every entry of the batch, each with the reason
    /// (ADR-0052 d15).
    ///
    /// # Errors
    ///
    /// The store's failure to read the queue; a failure to encode is not
    /// one, it is counted in [`SparsePass::failed`].
    pub async fn encode_sparse_pass(&self, per_tenant: usize) -> Result<SparsePass, AppError> {
        let encoder = self.sparse.as_deref().ok_or_else(|| {
            AppError::Store(StoreError::Backend(
                "the sparse encoder task runs only with a sparse encoder loaded".to_owned(),
            ))
        })?;
        let mut pass = SparsePass::default();
        for tenant in self.journal.sparse_tenants().await? {
            let pending = self.journal.sparse_pending(tenant, per_tenant).await?;
            if pending.is_empty() {
                continue;
            }
            let texts = pending.iter().map(|p| p.claim.clone()).collect();
            let encoded = match encoder.encode_documents(texts).await {
                Ok(lists) if lists.len() == pending.len() => Ok(lists),
                Ok(lists) => Err(format!(
                    "the encoder answered {} postings lists for {} texts",
                    lists.len(),
                    pending.len()
                )),
                Err(reason) => Err(reason),
            };
            match encoded {
                Ok(lists) => {
                    for (p, postings) in pending.iter().zip(&lists) {
                        match self.journal.sparse_write(tenant, p, postings).await {
                            Ok(true) => pass.written += 1,
                            Ok(false) => pass.stale += 1,
                            Err(e) => {
                                self.journal
                                    .sparse_failed(tenant, p, &e.to_string())
                                    .await?;
                                pass.failed += 1;
                            }
                        }
                    }
                }
                Err(reason) => {
                    for p in &pending {
                        self.journal.sparse_failed(tenant, p, &reason).await?;
                        pass.failed += 1;
                    }
                }
            }
        }
        Ok(pass)
    }
}
