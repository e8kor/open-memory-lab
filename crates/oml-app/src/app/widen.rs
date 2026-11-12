//! The widen hint's precomputed signal (ADR-0072 d8, d12, as built by
//! #244). A `scope` recall on a federated tenant reads no candidate and no
//! count: it looks its signal up, one read of the query's forms, and
//! decides d8's rule 1 from the counts the signal holds. The signal is
//! built off the request path, per key, scope and kinds, from the same
//! candidates and counts a `wide` recall reads, and serves only while the
//! key's permission epoch is the one it was built under and it is younger
//! than [`WIDEN_SIGNAL_MAX_AGE_SECS`]. Without such a signal the answer
//! carries no hint, and the recall asks for one to be built.

use std::collections::{BTreeMap, VecDeque};
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
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
/// 128 MB at 16 bytes a pair; the oldest built are dropped past it.
pub const WIDEN_SIGNAL_PAIRS_HELD: usize = 8_000_000;

/// The most (form, node) counts one tenant's signals hold, about 32 MB: a
/// tenant past it drops its own oldest first, so no tenant evicts another
/// below its share, and a signal larger on its own is never read whole. A
/// tenant of 100,000 rows over 50 workareas measured 1,332,730 (#268).
pub const WIDEN_SIGNAL_TENANT_PAIRS: usize = 2_000_000;

/// A build taking longer is stopped by the transport, counted as failed
/// and marked; the builds behind it go on.
pub const WIDEN_SIGNAL_BUILD_SECS: u64 = 30;

/// The store cancels each statement of a signal's read after this long.
/// The limit is per statement, so a build stopped at
/// [`WIDEN_SIGNAL_BUILD_SECS`] can leave the statement it was in running
/// on the database for up to this long; builds are stopped at least 30 s
/// apart, so at most one such statement runs at a time (#268).
pub const WIDEN_SIGNAL_STATEMENT_SECS: u64 = 25;

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
pub fn drops_widen_signals(event_type: &str) -> bool {
    use oml_domain::event_types as e;
    matches!(
        event_type,
        e::LIFECYCLE_ERASE
            | "lifecycle.demote"
            | e::WORKAREA_MODE_SET
            | e::WORKAREA_STATUS_TRANSITION
            | e::FEDERATION_POLICY_SET
            | e::WORKAREA_FEDERATION_SET
    )
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
    /// The tenant's drop generation when the build began: a drop since
    /// then makes the signal unusable, so a build that read rows before an
    /// erasure never serves after it.
    pub generation: u64,
}

impl WidenSignal {
    /// What it costs the lead's bounds: its pairs and its nodes, so a
    /// signal of no pairs still counts.
    fn weight(&self) -> usize {
        self.vocabulary.pairs() + self.vocabulary.rows.len()
    }
}

/// The signals the lead holds, those recalls asked for, and each tenant's
/// health for doctor.
#[derive(Debug, Default)]
pub(super) struct WidenSignals {
    pub limits: WidenLimits,
    pub held: Mutex<BTreeMap<SignalKey, Arc<WidenSignal>>>,
    /// Signals whose build failed, was stopped, passed the tenant's share
    /// or was evicted, and when: no recall asks for them again until the
    /// mark is [`WIDEN_SIGNAL_MAX_AGE_SECS`] old.
    pub marked: Mutex<BTreeMap<SignalKey, i64>>,
    /// Whether a refresh is running: one runs at a time in the process.
    pub refreshing: AtomicBool,
    /// The signals asked for, in the order they were asked.
    pub wanted: Mutex<VecDeque<SignalKey>>,
    /// When each tenant's last build began, as a count of builds: the
    /// queue serves the tenant whose turn is oldest, so no tenant waits
    /// behind another's whole demand. An entry lasts until the tenant's
    /// signals are dropped (#294).
    pub turns: Mutex<BTreeMap<Uuid, u64>>,
    /// The signal a build is working on; a recall does not ask for it.
    pub building: Mutex<Option<SignalKey>>,
    /// Each tenant's drop generation, raised by every drop.
    pub generations: Mutex<BTreeMap<Uuid, u64>>,
    pub health: Mutex<BTreeMap<Uuid, WidenSignalHealth>>,
    /// Tenants a build found with federation off, and when: their `scope`
    /// recalls ask for no signal until the mark is
    /// [`WIDEN_SIGNAL_MAX_AGE_SECS`] old or their policy changes here.
    pub off: Mutex<BTreeMap<Uuid, i64>>,
}

/// Whether `at` is less than [`WIDEN_SIGNAL_MAX_AGE_SECS`] before `now_us`.
fn within_bound(at: i64, now_us: i64) -> bool {
    now_us - at < WIDEN_SIGNAL_MAX_AGE_SECS * 1_000_000
}

impl WidenSignals {
    /// The tenant's health since the lead started.
    pub(super) fn health_of(&self, tenant_id: Uuid) -> WidenSignalHealth {
        self.health
            .lock()
            .map(|h| h.get(&tenant_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    fn health(&self, tenant_id: Uuid, f: impl FnOnce(&mut WidenSignalHealth)) {
        if let Ok(mut h) = self.health.lock() {
            f(h.entry(tenant_id).or_default());
        }
    }

    /// The tenant's drop generation now.
    pub(super) fn generation(&self, tenant_id: Uuid) -> u64 {
        self.generations
            .lock()
            .map(|g| g.get(&tenant_id).copied().unwrap_or(0))
            .unwrap_or(u64::MAX)
    }

    /// Whether any recall asked for a signal not yet built.
    pub(super) fn any_wanted(&self) -> bool {
        self.wanted.lock().is_ok_and(|w| !w.is_empty())
    }

    /// Asks for `key`'s signal to be built, once, behind those asked for
    /// before it.
    pub(super) fn want(&self, key: SignalKey) {
        if self.building.lock().is_ok_and(|b| b.as_ref() == Some(&key)) {
            return;
        }
        if let Ok(mut wanted) = self.wanted.lock()
            && !wanted.contains(&key)
        {
            wanted.push_back(key);
        }
    }

    /// A recall answered without the hint for want of a usable signal:
    /// counted, and the signal asked for.
    pub(super) fn unserved(&self, key: &SignalKey) {
        self.health(key.tenant_id, |h| h.unserved += 1);
        self.want(key.clone());
    }

    /// Holds a built signal unless a drop came after its build began; false
    /// when it was discarded. First every signal and mark past the bound
    /// goes. Past the tenant's share its own oldest go; past the lead's
    /// bound, the oldest of a tenant above its share, then the oldest of
    /// all. The signal just built stays. Each eviction is counted and
    /// marked, so the evicted signal is not rebuilt within the bound.
    pub(super) fn hold(&self, key: SignalKey, signal: WidenSignal, now_us: i64) -> bool {
        // The generation is read under the `held` lock, which every drop
        // takes too, so a drop is either before this check or after the
        // insert, where it removes the signal.
        let Ok(mut held) = self.held.lock() else {
            return false;
        };
        if signal.generation != self.generation(key.tenant_id) {
            return false;
        }
        held.retain(|_, s| within_bound(s.built_at_us, now_us));
        if let Ok(mut marked) = self.marked.lock() {
            marked.retain(|_, at| within_bound(*at, now_us));
        }
        if let Ok(mut off) = self.off.lock() {
            off.retain(|_, at| within_bound(*at, now_us));
        }
        let tenant = key.tenant_id;
        held.insert(key.clone(), Arc::new(signal));
        let share = self.limits.tenant_pairs;
        let mut totals: BTreeMap<Uuid, usize> = BTreeMap::new();
        for (k, s) in held.iter() {
            *totals.entry(k.tenant_id).or_default() += s.weight();
        }
        let mut total: usize = totals.values().sum();
        let mut evicted = Vec::new();
        loop {
            let over_share = |t: &Uuid| totals.get(t).is_some_and(|n| *n > share);
            // A tenant over its share with no older signal of its own
            // falls through to the lead's bound, so the lead stays within
            // it whatever one signal weighs (L4, #268).
            let own = over_share(&tenant)
                .then(|| Self::oldest(&held, &key, |k| k.tenant_id == tenant))
                .flatten();
            let victim = own.or_else(|| {
                (total > self.limits.held_pairs)
                    .then(|| {
                        Self::oldest(&held, &key, |k| over_share(&k.tenant_id))
                            .or_else(|| Self::oldest(&held, &key, |_| true))
                    })
                    .flatten()
            });
            let Some(victim) = victim else {
                break;
            };
            if let Some(s) = held.remove(&victim) {
                total -= s.weight();
                if let Some(n) = totals.get_mut(&victim.tenant_id) {
                    *n -= s.weight();
                }
            }
            evicted.push(victim);
        }
        drop(held);
        for victim in evicted {
            self.health(victim.tenant_id, |h| h.evicted += 1);
            self.mark(&victim, now_us);
        }
        true
    }

    /// The oldest held signal other than `keep` that `pick` admits.
    fn oldest(
        held: &BTreeMap<SignalKey, Arc<WidenSignal>>,
        keep: &SignalKey,
        pick: impl Fn(&SignalKey) -> bool,
    ) -> Option<SignalKey> {
        held.iter()
            .filter(|(k, _)| *k != keep && pick(k))
            .min_by_key(|(_, s)| s.built_at_us)
            .map(|(k, _)| k.clone())
    }

    /// The next signal to build, taken off the queue and recorded as the
    /// one building: the oldest asked for by the tenant whose last build
    /// began longest ago, a tenant never built first. Each tenant waiting
    /// gets one build before any gets another (L3, #268).
    fn take_next(&self) -> Option<SignalKey> {
        let mut wanted = self.wanted.lock().ok()?;
        let mut turns = self.turns.lock().ok()?;
        let at = wanted
            .iter()
            .enumerate()
            .min_by_key(|(i, k)| (turns.get(&k.tenant_id).copied().unwrap_or(0), *i))
            .map(|(i, _)| i)?;
        let key = wanted.remove(at)?;
        let turn = turns.values().max().copied().unwrap_or(0) + 1;
        turns.insert(key.tenant_id, turn);
        drop((wanted, turns));
        if let Ok(mut b) = self.building.lock() {
            *b = Some(key.clone());
        }
        Some(key)
    }

    /// Marks `key` so no recall asks for it again within the bound.
    fn mark(&self, key: &SignalKey, now_us: i64) {
        if let Ok(mut marked) = self.marked.lock() {
            marked.insert(key.clone(), now_us);
        }
    }

    /// Whether `key` was marked less than [`WIDEN_SIGNAL_MAX_AGE_SECS`] ago.
    fn is_marked(&self, key: &SignalKey, now_us: i64) -> bool {
        self.marked
            .lock()
            .is_ok_and(|m| m.get(key).is_some_and(|at| within_bound(*at, now_us)))
    }

    /// Drops `key`'s signal.
    pub(super) fn drop_key(&self, key: &SignalKey) {
        if let Ok(mut held) = self.held.lock() {
            held.remove(key);
        }
    }

    /// Drops every signal of the tenant, its marks and its turn at the
    /// builds, and raises its generation so a build already reading is discarded: an event of
    /// [`drops_widen_signals`] committed.
    pub(super) fn drop_tenant(&self, tenant_id: Uuid) {
        if let Ok(mut held) = self.held.lock() {
            if let Ok(mut g) = self.generations.lock() {
                *g.entry(tenant_id).or_default() += 1;
            }
            held.retain(|k, _| k.tenant_id != tenant_id);
        }
        if let Ok(mut marked) = self.marked.lock() {
            marked.retain(|k, _| k.tenant_id != tenant_id);
        }
        if let Ok(mut off) = self.off.lock() {
            off.remove(&tenant_id);
        }
        if let Ok(mut turns) = self.turns.lock() {
            turns.remove(&tenant_id);
        }
    }

    /// Whether a build found the tenant's federation off less than
    /// [`WIDEN_SIGNAL_MAX_AGE_SECS`] ago.
    fn known_off(&self, tenant_id: Uuid, now_us: i64) -> bool {
        self.off.lock().is_ok_and(|off| {
            off.get(&tenant_id)
                .is_some_and(|at| within_bound(*at, now_us))
        })
    }
}

/// One refresh of the widen signals, the only one running in the process
/// while it lives: [`App::begin_widen_refresh`]. Dropped, it clears the
/// flag and forgets the build it was running, which a recall may ask for
/// again.
#[must_use]
pub struct WidenRefresh<'a>(&'a WidenSignals);

impl Drop for WidenRefresh<'_> {
    fn drop(&mut self) {
        if let Ok(mut b) = self.0.building.lock() {
            *b = None;
        }
        self.0.refreshing.store(false, Ordering::Release);
    }
}

/// One signal to build, taken from the asked-for queue by
/// [`App::next_widen_build`].
#[derive(Debug)]
pub struct WidenBuild(SignalKey);

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
    /// transport then runs a refresh off the request.
    pub fn widen_signals_wanted(&self) -> bool {
        self.widen.any_wanted()
    }

    /// Sets the widen signal's bounds (#244): tests make them small.
    #[must_use]
    pub fn with_widen_limits(mut self, limits: WidenLimits) -> Self {
        self.widen.limits = limits;
        self
    }

    /// Starts a refresh, unless one is running in the process.
    pub fn begin_widen_refresh(&self) -> Option<WidenRefresh<'_>> {
        self.widen
            .refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| WidenRefresh(&self.widen))
    }

    /// The next signal to build, taken off the queue: one build at a time,
    /// the tenants waiting in turn and each tenant's in the order its
    /// recalls asked, so a tenant waits at most one build for each other
    /// tenant waiting, never for a tenant's whole demand.
    pub fn next_widen_build(&self, _refresh: &WidenRefresh<'_>) -> Option<WidenBuild> {
        self.widen.take_next().map(WidenBuild)
    }

    /// Runs one build and holds, discards or marks what it found; whether
    /// it held a signal. A key gone, a scope it no longer reads or
    /// federation off leaves none. A build that fails or passes the
    /// tenant's share leaves none, is counted with its reason for doctor,
    /// and is marked so no recall asks for it again within the bound. A
    /// signal a drop overtook is discarded and may be asked for again.
    pub async fn run_widen_build(&self, build: &WidenBuild) -> bool {
        let key = &build.0;
        let outcome = self.build_widen_signal(key).await;
        if let Ok(mut b) = self.widen.building.lock() {
            *b = None;
        }
        let failure = match outcome {
            Ok(Built::Signal(signal)) => {
                let held = self
                    .widen
                    .hold(key.clone(), signal, self.clock.now().unix_micros());
                if held {
                    self.widen.health(key.tenant_id, |h| h.built += 1);
                }
                return held;
            }
            Ok(Built::Nothing) => {
                self.widen.drop_key(key);
                return false;
            }
            Ok(Built::PastShare) => format!(
                "a signal of more than {} counts, the tenant's share",
                self.widen.limits.tenant_pairs
            ),
            Err(e) => e.to_string(),
        };
        self.widen_build_failed(key, failure);
        false
    }

    /// The transport stopped `build` at its timeout: it is counted as
    /// failed and marked, and the builds behind it go on.
    pub fn widen_build_stopped(&self, build: &WidenBuild) {
        if let Ok(mut b) = self.widen.building.lock() {
            *b = None;
        }
        self.widen_build_failed(
            &build.0,
            format!(
                "a build was stopped after {} s",
                self.widen.limits.build_timeout.as_secs()
            ),
        );
    }

    fn widen_build_failed(&self, key: &SignalKey, failure: String) {
        self.widen.health(key.tenant_id, |h| {
            h.failed += 1;
            h.last_failure = Some(failure);
        });
        self.widen.drop_key(key);
        self.widen.mark(key, self.clock.now().unix_micros());
    }

    /// Builds every signal asked for, one at a time and with no timeout,
    /// unless a refresh is running; how many it held.
    pub async fn refresh_widen_signals(&self) -> usize {
        self.refresh_widen_signals_with(|_| std::future::pending())
            .await
    }

    /// [`Self::refresh_widen_signals`] with each build raced against
    /// `timer(`[`WidenLimits::build_timeout`]`)`: a build the timer beats
    /// is stopped, counted as failed and marked, and the next goes on. The
    /// transport spawns it with its runtime's sleep after a recall that
    /// asked for a signal; the core names no runtime.
    pub async fn refresh_widen_signals_with<T>(&self, timer: impl Fn(Duration) -> T) -> usize
    where
        T: Future<Output = ()>,
    {
        let Some(refresh) = self.begin_widen_refresh() else {
            return 0;
        };
        let mut built = 0;
        while let Some(build) = self.next_widen_build(&refresh) {
            let mut run = pin!(self.run_widen_build(&build));
            let mut stop = pin!(timer(self.widen.limits.build_timeout));
            let held = poll_fn(|cx| {
                if let Poll::Ready(held) = run.as_mut().poll(cx) {
                    return Poll::Ready(Some(held));
                }
                stop.as_mut().poll(cx).map(|()| None)
            })
            .await;
            match held {
                Some(true) => built += 1,
                Some(false) => {}
                None => self.widen_build_stopped(&build),
            }
        }
        built
    }

    /// One signal (#244): the key's access read now, R = served(scope) ∪ C
    /// as a `wide` recall works them out, and every form's counts over R,
    /// labelled with the access's permission epoch and the build's time.
    /// `None` for a key the tenant no longer holds or that is revoked, a
    /// scope it no longer reads, or a tenant with federation off.
    async fn build_widen_signal(&self, key: &SignalKey) -> Result<Built, AppError> {
        let tenant_id = key.tenant_id;
        let generation = self.widen.generation(tenant_id);
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
            generation,
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
        let generation = self.widen.generation(key.tenant_id);
        let usable = held.filter(|s| {
            s.generation == generation
                && s.permission_epoch == caller.permission_epoch
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
    use std::collections::BTreeSet;

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
            generation: 0,
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
    fn a_signal_whose_build_began_before_a_drop_is_never_held() {
        let signals = WidenSignals::default();
        let mut built = signal(1, 2);
        built.generation = signals.generation(Uuid::from_u128(1));
        signals.drop_tenant(Uuid::from_u128(1));
        assert!(
            !signals.hold(key(1, 1), built, 1),
            "a signal read before the drop was held"
        );
        assert!(signals.held.lock().expect("held").is_empty());
        let mut after = signal(2, 2);
        after.generation = signals.generation(Uuid::from_u128(1));
        assert!(signals.hold(key(1, 1), after, 2));
    }

    #[test]
    fn an_evicted_signal_is_counted_and_not_asked_for_again_within_the_bound() {
        let signals = WidenSignals {
            limits: WidenLimits {
                tenant_pairs: 4,
                ..WidenLimits::default()
            },
            ..WidenSignals::default()
        };
        assert!(signals.hold(key(1, 1), signal(1, 3), 1));
        assert!(signals.hold(key(1, 2), signal(2, 3), 2));
        assert_eq!(signals.health_of(Uuid::from_u128(1)).evicted, 1);
        assert!(signals.is_marked(&key(1, 1), 3));
        assert!(!signals.is_marked(&key(1, 1), 3 + WIDEN_SIGNAL_MAX_AGE_SECS * 1_000_000));
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
            assert!(signals.hold(key(t, s), signal(at, 2), at));
        }
        // Tenant 2 grows past its share: its own oldest goes, never tenant
        // 1's, though tenant 1's are older.
        assert!(signals.hold(key(2, 1), signal(4, 4), 4));
        assert!(signals.hold(key(2, 2), signal(5, 4), 5));
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
        assert!(signals.hold(key(1, 4), signal(6, 2), 6));
        let held: BTreeSet<SignalKey> =
            signals.held.lock().expect("held").keys().cloned().collect();
        assert!(
            !held.contains(&key(1, 1)),
            "tenant 1 past its share kept its oldest"
        );
        assert!(held.contains(&key(2, 2)) && held.contains(&key(1, 4)));
    }

    /// The weight of every signal the lead holds.
    fn held_weight(signals: &WidenSignals) -> usize {
        signals
            .held
            .lock()
            .expect("held")
            .values()
            .map(|s| s.weight())
            .sum()
    }

    #[test]
    fn a_signal_over_its_tenants_share_alone_still_keeps_the_lead_within_its_bound() {
        let signals = WidenSignals {
            limits: WidenLimits {
                held_pairs: 10,
                tenant_pairs: 6,
                ..WidenLimits::default()
            },
            ..WidenSignals::default()
        };
        for (t, s, at) in [(1, 1, 1), (1, 2, 2), (2, 1, 3)] {
            assert!(signals.hold(key(t, s), signal(at, 3), at));
        }
        // Tenant 3's one signal is past its share on its own: it has no
        // older signal to drop, so the lead's bound drops the oldest of all.
        assert!(signals.hold(key(3, 1), signal(4, 7), 4));
        assert!(
            held_weight(&signals) <= 10,
            "the lead holds {} pairs past its bound of 10",
            held_weight(&signals)
        );
        let held: BTreeSet<SignalKey> =
            signals.held.lock().expect("held").keys().cloned().collect();
        assert!(held.contains(&key(3, 1)), "the signal just built went");
        assert!(!held.contains(&key(1, 1)), "the oldest of all stayed");
    }

    #[test]
    fn each_tenant_waiting_for_a_signal_gets_a_build_before_any_tenant_gets_another() {
        let signals = WidenSignals::default();
        for (t, s) in [(1, 1), (1, 2), (1, 3), (2, 1), (2, 2), (3, 1)] {
            signals.want(key(t, s));
        }
        let order: Vec<(u128, u128)> = std::iter::from_fn(|| signals.take_next())
            .map(|k| (k.tenant_id.as_u128(), k.scope.as_u128()))
            .collect();
        assert_eq!(
            order,
            [(1, 1), (2, 1), (3, 1), (1, 2), (2, 2), (1, 3)],
            "tenants 2 and 3 waited behind tenant 1's whole demand"
        );
        // A tenant that asks later is served before one already served.
        signals.want(key(1, 4));
        signals.want(key(1, 5));
        signals.want(key(4, 1));
        assert_eq!(signals.take_next(), Some(key(4, 1)));
    }

    #[test]
    fn a_tenants_turn_is_forgotten_with_its_signals_and_no_other_tenants_is() {
        let signals = WidenSignals::default();
        signals.want(key(1, 1));
        signals.want(key(2, 1));
        assert!(signals.take_next().is_some());
        assert!(signals.take_next().is_some());
        let turns = |s: &WidenSignals| -> Vec<u128> {
            s.turns
                .lock()
                .expect("turns")
                .keys()
                .map(|t| t.as_u128())
                .collect()
        };
        assert_eq!(turns(&signals), [1, 2]);
        signals.drop_tenant(Uuid::from_u128(1));
        assert_eq!(
            turns(&signals),
            [2],
            "a dropped tenant kept an entry, one per tenant ever built (#294)"
        );
    }
}
