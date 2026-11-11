//! The widen hint's precomputed signal (ADR-0072 d8, d12, as built by
//! #244). A `scope` recall on a federated tenant reads no candidate and no
//! count: it looks its signal up, one read of the query's forms, and
//! decides d8's rule 1 from the counts the signal holds. The signal is
//! built off the request path, per key, scope and kinds, from the same
//! candidates and counts a `wide` recall reads, and serves only while the
//! key's permission epoch is the one it was built under and it is younger
//! than [`WIDEN_SIGNAL_MAX_AGE_SECS`]. Without such a signal the answer
//! carries no hint, and the recall asks for one to be built.

#![allow(dead_code, reason = "the red commit's stub; #244 builds it")]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use oml_domain::MemoryKind;

use crate::federation::{VocabularyCounts, Widen, WidenSignalHealth};

/// A signal older than this is rebuilt after the recall that reads it,
/// which it still serves.
pub const WIDEN_SIGNAL_REFRESH_SECS: i64 = 60;

/// A signal this old or older serves no hint: the bound on how stale the
/// counts behind a served hint may be.
pub const WIDEN_SIGNAL_MAX_AGE_SECS: i64 = 300;

/// The most (form, node) counts the lead holds across every signal; the
/// oldest built are dropped past it, and a signal larger on its own is not
/// held.
pub const WIDEN_SIGNAL_PAIRS_HELD: usize = 4_000_000;

/// What a signal is built for: one key's recalls at one scope over one set
/// of kinds, with or without expired versions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SignalKey {
    pub tenant_id: Uuid,
    pub key_id: Uuid,
    pub scope: Uuid,
    pub kinds: Vec<&'static str>,
    pub include_expired: bool,
}

impl SignalKey {
    pub(super) fn new(
        tenant_id: Uuid,
        key_id: Uuid,
        scope: Uuid,
        kinds: &[MemoryKind],
        include_expired: bool,
    ) -> Self {
        let mut kinds: Vec<&'static str> = kinds.iter().map(|k| k.as_str()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        SignalKey {
            tenant_id,
            key_id,
            scope,
            kinds,
            include_expired,
        }
    }
}

/// One built signal: the counts over R = served(scope) ∪ C as they stood
/// when it was built, under the key's permission epoch then.
#[derive(Debug)]
pub(super) struct WidenSignal {
    pub built_at_us: i64,
    pub permission_epoch: u64,
    /// served(scope) when built; a recall whose served nodes differ reads
    /// no hint from it.
    pub hit_nodes: Vec<Uuid>,
    /// C when built, ordered by name.
    pub candidates: Vec<Uuid>,
    pub vocabulary: VocabularyCounts,
}

/// The signals the lead holds, those recalls asked for, and each tenant's
/// health for doctor.
#[derive(Debug, Default)]
pub(super) struct WidenSignals {
    pub held: Mutex<BTreeMap<SignalKey, Arc<WidenSignal>>>,
    pub wanted: Mutex<BTreeSet<SignalKey>>,
    pub health: Mutex<BTreeMap<Uuid, WidenSignalHealth>>,
}

impl WidenSignals {
    /// The tenant's health since the lead started.
    pub(super) fn health_of(&self, tenant_id: Uuid) -> WidenSignalHealth {
        self.health
            .lock()
            .map(|h| h.get(&tenant_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Whether any recall asked for a signal not yet built.
    pub(super) fn any_wanted(&self) -> bool {
        self.wanted.lock().is_ok_and(|w| !w.is_empty())
    }

    /// Drops every signal of the tenant: its policy or a workarea's
    /// `accept_federated` changed.
    pub(super) fn drop_tenant(&self, tenant_id: Uuid) {
        if let Ok(mut held) = self.held.lock() {
            held.retain(|k, _| k.tenant_id != tenant_id);
        }
    }
}

/// The hint a signal's counts decide: unused until #244's implementation.
#[allow(dead_code, reason = "the red commit's stub")]
pub(super) fn decide(_signal: &WidenSignal) -> Option<Widen> {
    None
}

impl<J, S, C, I, Sc> super::App<J, S, C, I, Sc>
where
    J: crate::ports::JournalStore,
    S: crate::ports::Signer,
    C: crate::ports::Clock,
    I: crate::ports::IdGen,
    Sc: crate::ports::Scanner,
{
    /// Whether a recall asked for a signal that is not built yet: the
    /// transport then runs [`Self::refresh_widen_signals`] off the request.
    pub fn widen_signals_wanted(&self) -> bool {
        self.widen.any_wanted()
    }

    /// Builds every signal a recall asked for; how many it built.
    pub async fn refresh_widen_signals(&self) -> usize {
        0
    }

    /// `search.widen` for a `scope` recall, from its held signal alone.
    #[allow(clippy::too_many_arguments, reason = "one recall's inputs")]
    #[allow(clippy::unused_async, reason = "the red commit's stub")]
    pub(super) async fn widen_hint(
        &self,
        _caller: &super::Caller,
        _scope: Uuid,
        _hit_nodes: &[Uuid],
        _kinds: &[MemoryKind],
        _include_expired: bool,
        _prunes: bool,
        _query: &str,
    ) -> Option<Widen> {
        None
    }
}
