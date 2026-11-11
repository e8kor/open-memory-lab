//! The widen hint's precomputed signal (ADR-0072 d8, d12, as built by
//! #244). A `scope` recall on a federated tenant reads no candidate and no
//! count: it looks its signal up, one read of the query's forms, and
//! decides d8's rule 1 from the counts the signal holds. The signal is
//! built off the request path, per key, scope and kinds, from the same
//! candidates and counts a `wide` recall reads, and serves only while the
//! key's permission epoch is the one it was built under and it is younger
//! than [`WIDEN_SIGNAL_MAX_AGE_SECS`]. Without such a signal the answer
//! carries no hint, and the recall asks for one to be built.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use oml_domain::federation::{SourceCounts, estimate, matches_at_least, rate, richer};
use oml_domain::{MemoryKind, Role};

use super::federation::Kept;
use super::{App, Caller, ServedNodes, with_universe};
use crate::federation::{CountsScope, VocabularyCounts, Widen, WidenSignalHealth};
use crate::ports::{Clock, IdGen, JournalStore, Scanner, Signer};
use crate::types::AppError;

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
    /// The signals a build is working on: a recall asks for none of them
    /// again, so a burst of recalls starts one build per signal.
    pub building: Mutex<BTreeSet<SignalKey>>,
    pub health: Mutex<BTreeMap<Uuid, WidenSignalHealth>>,
    /// Tenants a build found with federation off, and when: their `scope`
    /// recalls ask for no signal until the mark is
    /// [`WIDEN_SIGNAL_MAX_AGE_SECS`] old or their policy changes here.
    pub off: Mutex<BTreeMap<Uuid, i64>>,
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

    /// Asks for `key`'s signal to be built.
    pub(super) fn want(&self, key: SignalKey) {
        if self.building.lock().is_ok_and(|b| b.contains(&key)) {
            return;
        }
        if let Ok(mut wanted) = self.wanted.lock() {
            wanted.insert(key);
        }
    }

    /// A recall answered without the hint for want of a usable signal:
    /// counted, and the signal asked for.
    pub(super) fn unserved(&self, key: &SignalKey) {
        if let Ok(mut health) = self.health.lock() {
            health.entry(key.tenant_id).or_default().unserved += 1;
        }
        self.want(key.clone());
    }

    /// Holds a built signal, dropping the oldest built past
    /// [`WIDEN_SIGNAL_PAIRS_HELD`].
    pub(super) fn hold(&self, key: SignalKey, signal: WidenSignal) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        held.insert(key, Arc::new(signal));
        let mut total: usize = held.values().map(|s| s.vocabulary.pairs()).sum();
        while total > WIDEN_SIGNAL_PAIRS_HELD {
            let Some(oldest) = held
                .iter()
                .min_by_key(|(_, s)| s.built_at_us)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(dropped) = held.remove(&oldest) {
                total -= dropped.vocabulary.pairs();
            }
        }
    }

    /// Drops `key`'s signal.
    pub(super) fn drop_key(&self, key: &SignalKey) {
        if let Ok(mut held) = self.held.lock() {
            held.remove(key);
        }
    }

    /// Drops every signal of the tenant: its policy or a workarea's
    /// `accept_federated` changed.
    pub(super) fn drop_tenant(&self, tenant_id: Uuid) {
        if let Ok(mut held) = self.held.lock() {
            held.retain(|k, _| k.tenant_id != tenant_id);
        }
        if let Ok(mut off) = self.off.lock() {
            off.remove(&tenant_id);
        }
    }

    /// Whether a build found the tenant's federation off less than
    /// [`WIDEN_SIGNAL_MAX_AGE_SECS`] ago.
    fn known_off(&self, tenant_id: Uuid, now_us: i64) -> bool {
        self.off.lock().is_ok_and(|off| {
            off.get(&tenant_id)
                .is_some_and(|at| now_us - at < WIDEN_SIGNAL_MAX_AGE_SECS * 1_000_000)
        })
    }
}

/// d8's rule 1 over the counts a signal gives for one query: some
/// candidate at least twice as rich per readable row as the scope, with a
/// kept word on two of its rows; with how many candidates hold a kept word
/// and the lower bound on their rows. The phase 1 rule, unchanged.
fn rule_one(kept: &Kept, hit_nodes: &[Uuid], candidates: &[Uuid]) -> Option<Widen> {
    let reference = kept.sum(hit_nodes.iter().chain(candidates));
    let scope = kept.sum(hit_nodes.iter());
    let counts: Vec<SourceCounts> = candidates.iter().map(|c| kept.of_node(c)).collect();
    let scope_rate = rate(&scope, &reference);
    counts
        .iter()
        .any(|c| richer(c, scope_rate, &reference))
        .then(|| Widen {
            candidates: counts
                .iter()
                .filter(|c| estimate(c, &reference) > 0.0)
                .count(),
            matches_at_least: matches_at_least(kept.words.len(), &counts),
        })
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// Whether a recall asked for a signal that is not built yet: the
    /// transport then runs [`Self::refresh_widen_signals`] off the request.
    pub fn widen_signals_wanted(&self) -> bool {
        self.widen.any_wanted()
    }

    /// Builds every signal a recall asked for, off the request path; how
    /// many it built. A key gone, a scope it no longer reads or federation
    /// off leaves none; a failed build leaves none and is counted, with its
    /// reason, for doctor.
    pub async fn refresh_widen_signals(&self) -> usize {
        let wanted = {
            let (Ok(mut wanted), Ok(mut building)) =
                (self.widen.wanted.lock(), self.widen.building.lock())
            else {
                return 0;
            };
            let taken = std::mem::take(&mut *wanted);
            building.extend(taken.iter().cloned());
            taken
        };
        let mut built = 0;
        for key in wanted {
            let outcome = self.build_widen_signal(&key).await;
            if let Ok(mut building) = self.widen.building.lock() {
                building.remove(&key);
            }
            let mut health = self.widen.health.lock().ok();
            let health = health.as_mut().map(|h| h.entry(key.tenant_id).or_default());
            match outcome {
                Ok(Some(signal)) if signal.vocabulary.pairs() > WIDEN_SIGNAL_PAIRS_HELD => {
                    if let Some(h) = health {
                        h.failed += 1;
                        h.last_failure = Some(format!(
                            "a signal of {} counts is more than the {WIDEN_SIGNAL_PAIRS_HELD} the lead holds",
                            signal.vocabulary.pairs()
                        ));
                    }
                    self.widen.drop_key(&key);
                }
                Ok(Some(signal)) => {
                    if let Some(h) = health {
                        h.built += 1;
                    }
                    self.widen.hold(key, signal);
                    built += 1;
                }
                Ok(None) => self.widen.drop_key(&key),
                Err(e) => {
                    if let Some(h) = health {
                        h.failed += 1;
                        h.last_failure = Some(e.to_string());
                    }
                    self.widen.drop_key(&key);
                }
            }
        }
        built
    }

    /// One signal (#244): the key's access read now, R = served(scope) ∪ C
    /// as a `wide` recall works them out, and every form's counts over R,
    /// labelled with the access's permission epoch and the build's time.
    /// `None` for a key the tenant no longer holds or that is revoked, a
    /// scope it no longer reads, or a tenant with federation off.
    async fn build_widen_signal(&self, key: &SignalKey) -> Result<Option<WidenSignal>, AppError> {
        let tenant_id = key.tenant_id;
        let Some(record) = self.journal.key(tenant_id, key.key_id).await? else {
            return Ok(None);
        };
        if record.revoked_at.is_some() {
            return Ok(None);
        }
        let caller = self.caller_of(record).await?;
        if !self.journal.federation_policy(tenant_id).await?.enabled {
            if let Ok(mut off) = self.widen.off.lock() {
                off.insert(tenant_id, self.clock.now().unix_micros());
            }
            return Ok(None);
        }
        if self
            .require_access(&caller, key.scope, Role::Reader, "recall")
            .await
            .is_err()
        {
            return Ok(None);
        }
        let scope_ancestry = self
            .journal
            .workarea_ancestry(tenant_id, key.scope)
            .await?
            .unwrap_or_default();
        let ServedNodes {
            nodes: readable, ..
        } = self.served_nodes(&caller, key.scope).await?;
        let hit_nodes = with_universe(&caller, &readable, &scope_ancestry);
        let (candidates, _) = self.candidates(&caller, key.scope, &hit_nodes).await?;
        let candidates: Vec<Uuid> = candidates.into_iter().map(|c| c.id).collect();
        let built_at_us = self.clock.now().unix_micros();
        let mut r = hit_nodes.clone();
        r.extend(candidates.iter().copied());
        let kinds: Vec<MemoryKind> = super::HIT_KINDS
            .into_iter()
            .filter(|k| key.kinds.contains(&k.as_str()))
            .collect();
        let vocabulary = self
            .journal
            .vocabulary_counts(
                tenant_id,
                &CountsScope {
                    nodes: r,
                    kinds,
                    expired_at: (!key.include_expired).then_some(built_at_us),
                    prunes: false,
                },
            )
            .await?;
        Ok(Some(WidenSignal {
            built_at_us,
            permission_epoch: caller.permission_epoch,
            hit_nodes,
            candidates,
            vocabulary,
        }))
    }

    /// `search.widen` for a `scope` recall (d8, as built by #244), from its
    /// held signal alone: one read of the query's forms and none of any
    /// candidate or count. No signal, one built under another permission
    /// epoch or another served node set, or one at its age bound, serves
    /// no hint and asks for a signal; one past its refresh age serves and
    /// asks to be rebuilt. A failed read serves no hint and is counted.
    #[allow(clippy::too_many_arguments, reason = "one recall's inputs")]
    pub(super) async fn widen_hint(
        &self,
        caller: &Caller,
        scope: Uuid,
        hit_nodes: &[Uuid],
        kinds: &[MemoryKind],
        include_expired: bool,
        prunes: bool,
        query: &str,
    ) -> Option<Widen> {
        let key = SignalKey::new(
            caller.key.tenant_id,
            caller.key.key_id,
            scope,
            kinds,
            include_expired,
        );
        let now_us = self.clock.now().unix_micros();
        if self.widen.known_off(key.tenant_id, now_us) {
            return None;
        }
        let held = self
            .widen
            .held
            .lock()
            .ok()
            .and_then(|h| h.get(&key).cloned());
        let usable = held.filter(|s| {
            s.permission_epoch == caller.permission_epoch
                && s.hit_nodes == hit_nodes
                && now_us - s.built_at_us < WIDEN_SIGNAL_MAX_AGE_SECS * 1_000_000
        });
        let Some(signal) = usable else {
            self.widen.unserved(&key);
            return None;
        };
        if now_us - signal.built_at_us >= WIDEN_SIGNAL_REFRESH_SECS * 1_000_000 {
            self.widen.want(key.clone());
        }
        match self
            .journal
            .counts_from_vocabulary(&signal.vocabulary, query, prunes)
            .await
        {
            Ok(counts) => rule_one(&Kept::of(&counts), &signal.hit_nodes, &signal.candidates),
            Err(e) => {
                if let Ok(mut h) = self.widen.health.lock() {
                    let h = h.entry(key.tenant_id).or_default();
                    h.failed += 1;
                    h.last_failure = Some(format!("the signal's read: {e}"));
                }
                None
            }
        }
    }
}
