//! The fetch caps of a client ID metadata document (ADR-0031 d5;
//! `docs/api/http-api.md`, "Client identity"; `docs/architecture/
//! security.md`, "Connections: the OAuth surface"). At most 16 fetches run
//! at once on the lead, at most one per host name and at most one per
//! resolved address, so many names pointing at one slow address hold one
//! slot. A fetch past any cap waits for nothing: it is
//! [`DocumentFetchFailure::Busy`] at once, and the authorize use case then
//! serves a copy kept within 24 hours or answers `oauth.client_invalid`
//! (`reason: fetch_busy`). Every resolver answer must pass the domain's
//! address allow-list before any connection, and the connection goes to a
//! checked answer.
//!
//! A slot is held for as long as the outbound work it stands for runs, not
//! for as long as anything awaits that work: the host's slot (with one of
//! the 16) and the address's slot go to the adapter inside a
//! [`FetchGuard`], which the adapter moves into the thread running the
//! resolver or the connection. A request whose client disconnects, or a
//! resolve cut by its timeout while `getaddrinfo` still runs, keeps its
//! slots until that thread returns.
//!
//! The caps hold for one [`CappedFetcher`]: the lead composes exactly one.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use oml_domain::oauth::{ClientId, admit_fetch_answers, classify_client_id};

use crate::ports::{
    ClientDocumentFetcher, DocumentFetchFailure, DocumentFetcher, FetchGuard, FetchedDocument,
};

/// The fetches the lead runs at once (ADR-0031 d5).
pub const FETCH_SLOTS: usize = 16;

/// The caps and the address allow-list over a [`DocumentFetcher`], as the
/// [`ClientDocumentFetcher`] the authorize use case calls.
pub struct CappedFetcher<F> {
    inner: F,
    held: Arc<Mutex<Held>>,
}

/// The slots in use: the count, and the host names and addresses holding
/// one.
#[derive(Default)]
struct Held {
    slots: usize,
    hosts: BTreeSet<String>,
    addresses: BTreeSet<IpAddr>,
}

fn lock(held: &Mutex<Held>) -> MutexGuard<'_, Held> {
    // Nothing panics while the lock is held, so a poisoned lock still holds
    // a consistent count.
    held.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<F> CappedFetcher<F> {
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            held: Arc::default(),
        }
    }

    /// One of the 16 slots and `host`'s slot, together or neither.
    fn take_host(&self, host: &str) -> Result<Arc<Slot>, DocumentFetchFailure> {
        let mut held = lock(&self.held);
        if held.slots >= FETCH_SLOTS || held.hosts.contains(host) {
            return Err(DocumentFetchFailure::Busy);
        }
        held.slots += 1;
        held.hosts.insert(host.to_owned());
        Ok(Arc::new(Slot {
            held: Arc::clone(&self.held),
            taken: Taken::Host(host.to_owned()),
        }))
    }

    /// `address`'s slot.
    fn take_address(&self, address: IpAddr) -> Result<Arc<Slot>, DocumentFetchFailure> {
        if !lock(&self.held).addresses.insert(address) {
            return Err(DocumentFetchFailure::Busy);
        }
        Ok(Arc::new(Slot {
            held: Arc::clone(&self.held),
            taken: Taken::Address(address),
        }))
    }
}

/// A slot taken for one fetch, given back when the last [`FetchGuard`] or
/// handle holding it drops.
pub(crate) struct Slot {
    held: Arc<Mutex<Held>>,
    taken: Taken,
}

enum Taken {
    /// One of the 16 and the host name's.
    Host(String),
    Address(IpAddr),
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut held = lock(&self.held);
        match &self.taken {
            Taken::Host(host) => {
                held.slots -= 1;
                held.hosts.remove(host);
            }
            Taken::Address(address) => {
                held.addresses.remove(address);
            }
        }
    }
}

impl<F: DocumentFetcher> ClientDocumentFetcher for CappedFetcher<F> {
    /// The slot and the host's slot first, so a busy fetch resolves
    /// nothing; then every answer through the allow-list, then the first
    /// answer's slot, then the fetch from that answer. The resolver holds
    /// the host's slot until it returns, and the fetch holds both slots
    /// until its connection is done with.
    async fn fetch(&self, url: &str) -> Result<FetchedDocument, DocumentFetchFailure> {
        let Ok(ClientId::Document(document)) = classify_client_id(url) else {
            return Err(DocumentFetchFailure::Failed);
        };
        let host = document.host();
        let host_slot = self.take_host(host)?;
        let answers = self
            .inner
            .resolve(host, FetchGuard::new(vec![Arc::clone(&host_slot)]))
            .await?;
        admit_fetch_answers(&answers).map_err(|_| DocumentFetchFailure::Failed)?;
        let address = answers[0];
        let address_slot = self.take_address(address)?;
        self.inner
            .fetch(url, address, FetchGuard::new(vec![host_slot, address_slot]))
            .await
    }
}
