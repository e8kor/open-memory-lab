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
//! The caps hold for one [`CappedFetcher`]: the lead composes exactly one.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};

use oml_domain::oauth::{ClientId, admit_fetch_answers, classify_client_id};

use crate::ports::{ClientDocumentFetcher, DocumentFetchFailure, DocumentFetcher, FetchedDocument};

/// The fetches the lead runs at once (ADR-0031 d5).
pub const FETCH_SLOTS: usize = 16;

/// The caps and the address allow-list over a [`DocumentFetcher`], as the
/// [`ClientDocumentFetcher`] the authorize use case calls.
pub struct CappedFetcher<F> {
    inner: F,
    held: Mutex<Held>,
}

/// The slots in use: the count, and the host names and addresses holding
/// one.
#[derive(Default)]
struct Held {
    slots: usize,
    hosts: BTreeSet<String>,
    addresses: BTreeSet<IpAddr>,
}

impl<F> CappedFetcher<F> {
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            held: Mutex::new(Held::default()),
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        // Nothing panics while the lock is held, so a poisoned lock still
        // holds a consistent count.
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One of the 16 slots and `host`'s slot, together or neither.
    fn take_host(&self, host: &str) -> Result<Slot<'_, F>, DocumentFetchFailure> {
        let mut held = self.held();
        if held.slots >= FETCH_SLOTS || held.hosts.contains(host) {
            return Err(DocumentFetchFailure::Busy);
        }
        held.slots += 1;
        held.hosts.insert(host.to_owned());
        Ok(Slot {
            fetcher: self,
            taken: Taken::Host(host.to_owned()),
        })
    }

    /// `address`'s slot.
    fn take_address(&self, address: IpAddr) -> Result<Slot<'_, F>, DocumentFetchFailure> {
        if !self.held().addresses.insert(address) {
            return Err(DocumentFetchFailure::Busy);
        }
        Ok(Slot {
            fetcher: self,
            taken: Taken::Address(address),
        })
    }
}

/// A slot held for one fetch, given back when the fetch ends or its future
/// is dropped.
struct Slot<'a, F> {
    fetcher: &'a CappedFetcher<F>,
    taken: Taken,
}

enum Taken {
    /// One of the 16 and the host name's.
    Host(String),
    Address(IpAddr),
}

impl<F> Drop for Slot<'_, F> {
    fn drop(&mut self) {
        let mut held = self.fetcher.held();
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
    /// answer's slot, then the fetch from that answer.
    async fn fetch(&self, url: &str) -> Result<FetchedDocument, DocumentFetchFailure> {
        let Ok(ClientId::Document(document)) = classify_client_id(url) else {
            return Err(DocumentFetchFailure::Failed);
        };
        let host = document.host();
        let _host = self.take_host(host)?;
        let answers = self.inner.resolve(host).await?;
        admit_fetch_answers(&answers).map_err(|_| DocumentFetchFailure::Failed)?;
        let address = answers[0];
        let _address = self.take_address(address)?;
        self.inner.fetch(url, address).await
    }
}
