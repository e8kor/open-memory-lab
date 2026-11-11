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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

/// The most (form, node) counts the lead holds across every signal, about
/// 64 MB at 16 bytes a pair; the oldest built are dropped past it.
pub const WIDEN_SIGNAL_PAIRS_HELD: usize = 4_000_000;

/// The most (form, node) counts one tenant's signals hold, about 16 MB: a
/// tenant past it drops its own oldest first, so no tenant evicts another
/// below its share, and a signal larger on its own is never read whole.
pub const WIDEN_SIGNAL_TENANT_PAIRS: usize = 1_000_000;

/// A refresh taking longer is stopped by the transport, and the build it
/// was running is counted as failed and marked.
pub const WIDEN_SIGNAL_BUILD_SECS: u64 = 10;

/// The widen signal's bounds, the constants above unless a test sets them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidenLimits {
    pub held_pairs: usize,
    pub tenant_pairs: usize,
    pub build_timeout: Duration,
}

impl Default for WidenLimits {
    fn default() -> Self {
        WidenLimits {
            held_pairs: WIDEN_SIGNAL_PAIRS_HELD,
            tenant_pairs: WIDEN_SIGNAL_TENANT_PAIRS,
            build_timeout: Duration::from_secs(WIDEN_SIGNAL_BUILD_SECS),
        }
    }
}

/// Whether a committed event drops its tenant's widen signals at once
/// (#244): an erasure or a demotion takes rows out of the counts, a mode or
/// status change moves a node in or out of the trust set, and a policy or
/// `accept_federated` change moves C. Other changes reach the hint within
/// [`WIDEN_SIGNAL_MAX_AGE_SECS`].
#[must_use]
pub fn drops_widen_signals(_event_type: &str) -> bool {
    false
}

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
    pub limits: WidenLimits,
    pub held: Mutex<BTreeMap<SignalKey, Arc<WidenSignal>>>,
    /// Signals whose build failed or passed the tenant's share, and when:
    /// no recall asks for them again until the mark is
    /// [`WIDEN_SIGNAL_MAX_AGE_SECS`] old.
    pub marked: Mutex<BTreeMap<SignalKey, i64>>,
    /// Whether a refresh is running: one runs at a time in the process.
    pub refreshing: AtomicBool,
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

    /// Holds a built signal. Past the tenant's share its own oldest go
    /// first; past the lead's whole bound, the oldest of the tenants above
    /// their share, then the oldest of all. The signal just built stays.
    pub(super) fn hold(&self, key: SignalKey, signal: WidenSignal) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        held.insert(key, Arc::new(signal));
        while held.values().map(|s| s.vocabulary.pairs()).sum::<usize>() > self.limits.held_pairs {
            let Some(oldest) = held
                .iter()
                .min_by_key(|(_, s)| s.built_at_us)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            held.remove(&oldest);
        }
    }

    /// Marks `key` so no recall asks for it again within the bound.
    fn mark(&self, key: &SignalKey, now_us: i64) {
        if let Ok(mut marked) = self.marked.lock() {
            marked.insert(key.clone(), now_us);
        }
    }

    /// Whether `key` was marked less than [`WIDEN_SIGNAL_MAX_AGE_SECS`] ago.
    #[allow(clippy::unused_self, reason = "the red commit's stub")]
    fn is_marked(&self, _key: &SignalKey, _now_us: i64) -> bool {
        false
    }

    #[allow(dead_code, reason = "the red commit's stub")]
    fn is_marked_built(&self, key: &SignalKey, now_us: i64) -> bool {
        self.marked.lock().is_ok_and(|m| {
            m.get(key)
                .is_some_and(|at| now_us - at < WIDEN_SIGNAL_MAX_AGE_SECS * 1_000_000)
        })
    }

    /// Drops `key`'s signal.
    pub(super) fn drop_key(&self, key: &SignalKey) {
        if let Ok(mut held) = self.held.lock() {
            held.remove(key);
        }
    }

    /// Drops every signal of the tenant, and its marks: an event of
    /// [`drops_widen_signals`] committed.
    pub(super) fn drop_tenant(&self, tenant_id: Uuid) {
        if let Ok(mut held) = self.held.lock() {
            held.retain(|k, _| k.tenant_id != tenant_id);
        }
        if let Ok(mut marked) = self.marked.lock() {
            marked.retain(|k, _| k.tenant_id != tenant_id);
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

/// What a build found: a signal, nothing to build, or a vocabulary past the
/// tenant's share, which it stopped reading.
enum Built {
    Signal(WidenSignal),
    Nothing,
    PastShare,
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

    /// How long the transport lets one refresh run before it stops it.
    pub fn widen_refresh_timeout(&self) -> Duration {
        self.widen.limits.build_timeout
    }

    /// Sets the widen signal's bounds (#244): tests make them small.
    #[must_use]
    pub fn with_widen_limits(mut self, limits: WidenLimits) -> Self {
        self.widen.limits = limits;
        self
    }

    /// Builds every signal a recall asked for, off the request path, one at
    /// a time and one refresh at a time in the process; how many it built.
    /// A key gone, a scope it no longer reads or federation off leaves
    /// none. A build that fails, takes longer than its timeout or passes
    /// the tenant's share leaves none, is counted with its reason for
    /// doctor, and is marked so no recall asks for it again within the
    /// bound.
    pub async fn refresh_widen_signals(&self) -> usize {
        if self
            .widen
            .refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return 0;
        }
        /// However the refresh ends: a build it was still running, stopped
        /// by the transport's timeout, is counted as failed and marked, and
        /// the flag clears.
        struct Running<'a>(&'a WidenSignals, i64);
        impl Drop for Running<'_> {
            fn drop(&mut self) {
                let stopped: Vec<SignalKey> = self
                    .0
                    .building
                    .lock()
                    .map(|mut b| std::mem::take(&mut *b).into_iter().collect())
                    .unwrap_or_default();
                for key in stopped {
                    if let Ok(mut h) = self.0.health.lock() {
                        let h = h.entry(key.tenant_id).or_default();
                        h.failed += 1;
                        h.last_failure = Some(format!(
                            "a build was stopped after {} s",
                            self.0.limits.build_timeout.as_secs()
                        ));
                    }
                    self.0.mark(&key, self.1);
                }
                self.0.refreshing.store(false, Ordering::Release);
            }
        }
        let _running = Running(&self.widen, self.clock.now().unix_micros());
        let mut built = 0;
        loop {
            let wanted = {
                let (Ok(mut wanted), Ok(mut building)) =
                    (self.widen.wanted.lock(), self.widen.building.lock())
                else {
                    return built;
                };
                let taken = std::mem::take(&mut *wanted);
                building.extend(taken.iter().cloned());
                taken
            };
            if wanted.is_empty() {
                return built;
            }
            for key in wanted {
                let outcome = self.build_widen_signal(&key).await;
                if let Ok(mut building) = self.widen.building.lock() {
                    building.remove(&key);
                }
                let failure = match outcome {
                    Ok(Built::Signal(signal)) => {
                        if let Ok(mut h) = self.widen.health.lock() {
                            h.entry(key.tenant_id).or_default().built += 1;
                        }
                        self.widen.hold(key, signal);
                        built += 1;
                        continue;
                    }
                    Ok(Built::Nothing) => {
                        self.widen.drop_key(&key);
                        continue;
                    }
                    Ok(Built::PastShare) => format!(
                        "a signal of more than {} counts, the tenant's share",
                        self.widen.limits.tenant_pairs
                    ),
                    Err(e) => e.to_string(),
                };
                if let Ok(mut h) = self.widen.health.lock() {
                    let h = h.entry(key.tenant_id).or_default();
                    h.failed += 1;
                    h.last_failure = Some(failure);
                }
                self.widen.drop_key(&key);
                self.widen.mark(&key, self.clock.now().unix_micros());
            }
        }
    }

    /// One signal (#244): the key's access read now, R = served(scope) ∪ C
    /// as a `wide` recall works them out, and every form's counts over R,
    /// labelled with the access's permission epoch and the build's time.
    /// `None` for a key the tenant no longer holds or that is revoked, a
    /// scope it no longer reads, or a tenant with federation off.
    async fn build_widen_signal(&self, key: &SignalKey) -> Result<Built, AppError> {
        let tenant_id = key.tenant_id;
        let Some(record) = self.journal.key(tenant_id, key.key_id).await? else {
            return Ok(Built::Nothing);
        };
        if record.revoked_at.is_some() {
            return Ok(Built::Nothing);
        }
        let caller = self.caller_of(record).await?;
        if !self.journal.federation_policy(tenant_id).await?.enabled {
            if let Ok(mut off) = self.widen.off.lock() {
                off.insert(tenant_id, self.clock.now().unix_micros());
            }
            return Ok(Built::Nothing);
        }
        if self
            .require_access(&caller, key.scope, Role::Reader, "recall")
            .await
            .is_err()
        {
            return Ok(Built::Nothing);
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
        let Some(vocabulary) = self
            .journal
            .vocabulary_counts(
                tenant_id,
                &CountsScope {
                    nodes: r,
                    kinds,
                    expired_at: (!key.include_expired).then_some(built_at_us),
                    prunes: false,
                },
                self.widen.limits.tenant_pairs,
            )
            .await?
        else {
            return Ok(Built::PastShare);
        };
        Ok(Built::Signal(WidenSignal {
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
        if self.widen.known_off(key.tenant_id, now_us) || self.widen.is_marked(&key, now_us) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(built_at_us: i64, pairs: usize) -> WidenSignal {
        let node = Uuid::from_u128(1);
        let forms = (0..pairs).map(|i| (node, i64::try_from(i).expect("small"), 1));
        WidenSignal {
            built_at_us,
            permission_epoch: 0,
            hit_nodes: Vec::new(),
            candidates: Vec::new(),
            vocabulary: VocabularyCounts::from_counts(BTreeMap::new(), forms),
        }
    }

    fn key(tenant: u128, scope: u128) -> SignalKey {
        SignalKey::new(
            Uuid::from_u128(tenant),
            Uuid::from_u128(100),
            Uuid::from_u128(scope),
            &[MemoryKind::Memory],
            false,
        )
    }

    #[test]
    fn an_erasure_a_demotion_and_a_trust_change_drop_a_tenants_signals_and_a_promotion_does_not() {
        for event in [
            "lifecycle.erase",
            "lifecycle.demote",
            "workarea.mode_set",
            "workarea.status_transition",
            "federation.policy_set",
            "workarea.federation_set",
        ] {
            assert!(drops_widen_signals(event), "{event}");
        }
        for event in ["lifecycle.promote", "lifecycle.supersede", "grant.issued"] {
            assert!(!drops_widen_signals(event), "{event}");
        }
    }

    #[test]
    fn one_tenant_never_evicts_another_below_its_share() {
        let signals = WidenSignals {
            limits: WidenLimits {
                held_pairs: 10,
                tenant_pairs: 6,
                ..WidenLimits::default()
            },
            ..WidenSignals::default()
        };
        for (t, s, at) in [(1, 1, 1), (1, 2, 2), (1, 3, 3)] {
            signals.hold(key(t, s), signal(at, 2));
        }
        // Tenant 2 grows past its share: its own oldest goes, never tenant
        // 1's, though tenant 1's are older.
        signals.hold(key(2, 1), signal(4, 4));
        signals.hold(key(2, 2), signal(5, 4));
        let held: BTreeSet<SignalKey> =
            signals.held.lock().expect("held").keys().cloned().collect();
        for s in 1..=3 {
            assert!(
                held.contains(&key(1, s)),
                "tenant 1's signal {s} was evicted"
            );
        }
        assert!(!held.contains(&key(2, 1)) && held.contains(&key(2, 2)));
        // Past the lead's bound, a tenant above its share goes first.
        signals.hold(key(1, 4), signal(6, 2));
        let held: BTreeSet<SignalKey> =
            signals.held.lock().expect("held").keys().cloned().collect();
        assert!(
            !held.contains(&key(1, 1)),
            "tenant 1 past its share kept its oldest"
        );
        assert!(held.contains(&key(2, 2)) && held.contains(&key(1, 4)));
    }
}
