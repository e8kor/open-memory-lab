//! Federated recall's request, policy and answer types (ADR-0072 phase 1):
//! the `sources` field, the tenant's `federation` policy and a workarea's
//! `accept_federated`, the counts a store reads over the reference set, and
//! what an answer says about the sources it searched.

use std::collections::BTreeMap;

use oml_domain::federation::{MAX_SOURCES_DEFAULT, SourceCounts};
use uuid::Uuid;

use oml_domain::MemoryKind;

/// Where a recall searches (d5): a string is a mode and an array is a fixed
/// set of workareas, answered whole or refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sources {
    /// Today's answer exactly, plus the widen hint where a signal serves it
    /// (#244).
    Scope,
    /// The scope plus the candidates selection chooses.
    Wide,
    /// The listed workareas alone, by id or name as sent, in the caller's
    /// order; validated whole before any search.
    List(Vec<String>),
}

impl Sources {
    /// The mode's name, `list` for a fixed set.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Sources::Scope => "scope",
            Sources::Wide => "wide",
            Sources::List(_) => "list",
        }
    }
}

/// A tenant's default for an absent `sources`: a mode, never a list (d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DefaultSources {
    #[default]
    Scope,
    Wide,
}

impl DefaultSources {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DefaultSources::Scope => "scope",
            DefaultSources::Wide => "wide",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "scope" => Some(DefaultSources::Scope),
            "wide" => Some(DefaultSources::Wide),
            _ => None,
        }
    }
}

/// The tenant's `federation` policy (d5): off until a tenant admin enables
/// it, journalled as `federation.policy_set`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FederationPolicy {
    pub enabled: bool,
    pub default_sources: DefaultSources,
    /// At most this many candidates are searched beside the scope under
    /// `wide`, and a list names at most this many workareas.
    pub max_sources: u32,
}

impl Default for FederationPolicy {
    fn default() -> Self {
        FederationPolicy {
            enabled: false,
            default_sources: DefaultSources::Scope,
            max_sources: MAX_SOURCES_DEFAULT,
        }
    }
}

/// A change to the tenant's policy: each field `None` keeps its value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FederationPolicyChange {
    pub enabled: Option<bool>,
    pub default_sources: Option<DefaultSources>,
    pub max_sources: Option<u32>,
}

/// The tenant's policy as `GET /v2/federation` serves it: the policy and,
/// for doctor, the condition its line names (d12, Consequences).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationSettings {
    pub policy: FederationPolicy,
    /// The widen hint's signal on this tenant since the lead started, for
    /// doctor (#244).
    pub widen_signal: WidenSignalHealth,
}

/// How the widen hint's precomputed signal fared on a tenant since the
/// lead started (ADR-0072 d8, as built by #244): a `scope` recall that
/// found no signal it could use answered without the hint, and a build
/// that failed left none. It decides nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidenSignalHealth {
    /// Signals built.
    pub built: u64,
    /// `scope` recalls answered without the hint because their signal was
    /// missing, older than its bound or built under other grants.
    pub unserved: u64,
    /// Signals dropped to keep the tenant, or the lead, within its share;
    /// each is not rebuilt for 300 s.
    pub evicted: u64,
    /// Builds that failed, and the last one's reason.
    pub failed: u64,
    pub last_failure: Option<String>,
}

/// A workarea's federation field as its read serves it (d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkareaFederation {
    pub accept_federated: bool,
}

impl Default for WorkareaFederation {
    fn default() -> Self {
        WorkareaFederation {
            accept_federated: true,
        }
    }
}

/// What a store reads to compute the reference set's counts (d4.2, d12):
/// the served versions on `nodes` of `kinds`, less those whose valid time
/// ended at or before `expired_at`, counted per node over the query's kept
/// words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountsScope {
    pub nodes: Vec<Uuid>,
    pub kinds: Vec<MemoryKind>,
    pub expired_at: Option<i64>,
    /// Whether the pipeline prunes frequent words from its postings read
    /// (ADR-0052 d6): the pruned words are then decided over the nodes'
    /// sums, as the retriever decides them under the same reference set.
    pub prunes: bool,
}

/// The counts over a reference set (d4.2): the query's kept words in byte
/// order, the words a pruning pipeline drops over the set's sums, and each
/// node's served rows and per-word document frequency. Exact, never a
/// sketch (d12). A word's frequency is its most frequent form's, the exact
/// word or its stem, as the BM25 statements score a word by its best form.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReferenceCounts {
    pub words: Vec<String>,
    pub pruned: Vec<String>,
    pub nodes: BTreeMap<Uuid, SourceCounts>,
}

/// Every form's counts over a set of nodes, read once off the request path
/// and held as the widen hint's signal (ADR-0072 d8, as built by #244):
/// per node its served versions, and its forms, each an opaque key the
/// store gives it, with how many of those versions hold it, sorted by form
/// (16 bytes a pair, no allocation per form). The same subtraction of
/// expired versions as [`ReferenceCounts`] applies, as of the read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VocabularyCounts {
    pub rows: BTreeMap<Uuid, u64>,
    pub forms: BTreeMap<Uuid, Vec<(i64, u64)>>,
}

impl VocabularyCounts {
    /// How many (form, node) counts it holds.
    #[must_use]
    pub fn pairs(&self) -> usize {
        self.forms.values().map(Vec::len).sum()
    }

    /// Its size in memory, estimated: 16 bytes a pair in each node's
    /// vector and about 100 bytes a node for the two map entries and the
    /// vector's header.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.pairs() * 16 + self.rows.len() * 100
    }

    /// How many of `node`'s versions hold `form`.
    #[must_use]
    pub fn df(&self, node: &Uuid, form: i64) -> u64 {
        self.forms
            .get(node)
            .and_then(|f| {
                f.binary_search_by_key(&form, |(k, _)| *k)
                    .ok()
                    .map(|i| f[i].1)
            })
            .unwrap_or(0)
    }

    /// Builds it from (node, form, count) triples in any order; a count of
    /// 0 is left out.
    #[must_use]
    pub fn from_counts(
        rows: BTreeMap<Uuid, u64>,
        counts: impl IntoIterator<Item = (Uuid, i64, u64)>,
    ) -> Self {
        let mut forms: BTreeMap<Uuid, Vec<(i64, u64)>> = BTreeMap::new();
        for (node, form, n) in counts {
            if n > 0 {
                forms.entry(node).or_default().push((form, n));
            }
        }
        for f in forms.values_mut() {
            f.sort_unstable();
            f.shrink_to_fit();
        }
        VocabularyCounts { rows, forms }
    }
}

/// `search.widen` on a `scope` answer (d8, as amended): how many
/// candidates hold a kept word and a lower bound on their matching rows.
/// It names no workarea.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Widen {
    pub candidates: usize,
    pub matches_at_least: u64,
}

/// Whether a hit came from the scope or a federated source (d8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Scope,
    Federated,
}

impl Via {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Via::Scope => "scope",
            Via::Federated => "federated",
        }
    }
}

/// One searched source in `search.sources.searched` (d8): a readable node,
/// whether it is the scope's or federated, its estimate and the kept words
/// its rows hold. Phase 1 has no descriptor, so `matched` names words only.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchedSource {
    pub workarea_id: Uuid,
    pub workarea_path: String,
    pub via: Via,
    pub estimate: f64,
    pub matched_words: Vec<String>,
}

/// The candidates a `wide` recall did not search (d4.6): how many hold a
/// kept word, and a lower bound on their rows that do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CutReport {
    pub count: usize,
    pub matches_at_least: u64,
}

/// `search.sources` under `wide` or a list (d8).
#[derive(Debug, Clone, PartialEq)]
pub struct SourcesReport {
    /// `wide` or `list`.
    pub mode: &'static str,
    /// |C|, the trusted candidates the key reads outside the scope; for a
    /// list, the listed nodes.
    pub considered: usize,
    pub searched: Vec<SearchedSource>,
    pub cut: CutReport,
}

/// What a federated recall adds to its event (d9): the `sources` given, as
/// the mode or the list's workarea ids, the federated nodes searched, and
/// the reference set's nodes, so a replay computes the same R.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FederationRecord {
    /// `"scope"`, `"wide"`, or the list as an array of workarea ids.
    pub sources: serde_json::Value,
    pub federated_workarea_ids: Vec<Uuid>,
    pub reference_workarea_ids: Vec<Uuid>,
}

/// Why a readable name in a `sources` list is not a usable source (d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRefusal {
    /// Not `committed` or `validated`, or inside an in-progress subtree.
    Untrusted,
    /// Outside subtree(scope) on a workarea whose `accept_federated` is off.
    DeclinesFederation,
}

impl SourceRefusal {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SourceRefusal::Untrusted => "source_untrusted",
            SourceRefusal::DeclinesFederation => "source_declines_federation",
        }
    }
}
