//! Search as a pipeline of retrievers, fused in the core (ADR-0049).
//!
//! A recall's hit candidates come from one or more [`Retriever`] adapters,
//! each producing a ranked list over the candidate scope the core computes;
//! a pure [`Fusion`] strategy here merges the lists; the core then applies
//! the `k` and budget cuts it always applied. The adapters rank and never
//! decide visibility: the scope is the core's, and the core re-checks every
//! served row (ADR-0001 d3, R3; ADR-0049 d2).
//!
//! A deployment names the pipelines it serves and its default
//! ([`SearchConfig`]); a tenant may pick its default among them, journalled;
//! a request may name one among them. The names form a closed set, so a
//! metric labelled by pipeline stays bounded (ADR-0049 d4).
//!
//! [`Retriever`]: crate::ports::Retriever

use std::collections::BTreeMap;
use std::fmt;

use uuid::Uuid;

use oml_domain::MemoryKind;

use crate::types::AnchorFilter;
use oml_domain::budget::hit_order;
use oml_domain::valid_time::Window;

/// One candidate generator an adapter may serve (ADR-0049 d2). Each is a
/// ranking over the readable rows, never a visibility decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RetrieverKind {
    /// Every query word, exactly, under the `simple` configuration, ranked
    /// by `ts_rank` with normalization 0: the recall served up to v0.5.0,
    /// kept for comparison (ADR-0048 S0).
    KeywordAll,
    /// Any query word, exactly, on the same index, ranked by `ts_rank`
    /// (ADR-0048 S1).
    KeywordAny,
    /// Identifier parts, `simple` and `english` lexemes together, ranked by
    /// BM25 whose statistics are computed over the readable rows alone
    /// (ADR-0048 S2).
    Bm25,
    /// Trigram word similarity of the normalised text, for misspellings and
    /// fragments (ADR-0048 S3).
    Trigram,
    /// `Bm25`'s score read from integer postings that row security lets an
    /// index serve, with the readable rows' counts kept per workarea; a word
    /// frequent among the readable rows is pruned from the postings read and
    /// still scored for the rows the others reach, and a query of only such
    /// words is answered by `Bm25`'s scan (ADR-0052 d4, d6).
    Bm25Postings,
    /// Learned sparse weights of `doc-v3-distill`, kind 2 postings read by
    /// the same index under the same scope, scored by the sum over the
    /// query's tokens of the shipped idf times the document's weight; no
    /// corpus statistic is read (ADR-0052 d14, d16).
    Sparse,
    /// The rows whose declared valid time overlaps the query's date window,
    /// read by leakproof `int8` comparisons on an index row security lets
    /// serve, nearest start first; empty when the query names no window
    /// (proposed ADR-0059 d7).
    Time,
}

impl RetrieverKind {
    /// The name `matched.via` and the doctor use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeywordAll => "keyword_all",
            Self::KeywordAny => "keyword_any",
            Self::Bm25 => "bm25",
            Self::Trigram => "trigram",
            Self::Bm25Postings => "bm25_postings",
            Self::Sparse => "sparse",
            Self::Time => "time",
        }
    }
}

/// How a retriever read its list (ADR-0052 d6, d9): from the postings an
/// index serves, or by a scan of the scope's stored lexemes, which every
/// retriever but `Bm25Postings` always runs and which `Bm25Postings` runs
/// when every word of the query is pruned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Access {
    /// Read from the postings by their index.
    Postings,
    /// A scan of every row in the scope.
    #[default]
    Scan,
}

impl Access {
    /// The name the answer's `search.access` and the request line use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postings => "postings",
            Self::Scan => "scan",
        }
    }
}

/// How a pipeline's lists become one (ADR-0049 d3). Pure: it reads ranks
/// and scores the adapters returned and nothing else, so no verdict, no
/// reviewer and no `promoted_at` can reorder a result (ADR-0001 d3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fusion {
    /// One list, served in its own order.
    Single,
    /// Reciprocal rank fusion with constant `k`: a candidate's score is the
    /// sum over the lists holding it of `1 / (k + rank)`, rank from 1.
    Rrf { k: u32 },
    /// Convex combination: each list's scores min-max normalised over its
    /// own candidates, then summed with one weight per list in fusion
    /// order; a list that does not hold a candidate adds nothing
    /// (ADR-0052 d13).
    Cc { weights: &'static [f32] },
}

/// The fusion constant ADR-0048 d4 names, unmeasured on OML beyond the
/// recall eval.
pub const RRF_K: u32 = 60;

/// How deep each list of a fused pipeline is read before fusion: a list cut
/// at `k` would drop a candidate the other list ranks first.
pub const FUSION_DEPTH: usize = 50;

/// How deep each list of a convex combination is read: the depth the
/// offline experiment normalised over, so a leg's minimum is that of its
/// top 100 (ADR-0052 d13).
pub const CC_DEPTH: usize = 100;

/// `bm25-sparse`'s weights, `bm25_postings` then `sparse`: tuned on the
/// synthetic set alone, flat between 0.15 and 0.35 for bm25, and within
/// 0.007 nDCG@10 of each external set's own best (ADR-0052 d13).
pub const CC_BM25_SPARSE: [f32; 2] = [0.3, 0.7];

/// How many rows the `time` retriever reads at most, nearest start first
/// (proposed ADR-0059 d7).
pub const TIME_READ: usize = 200;

/// How deep the `time` list enters fusion once the core has ordered it by
/// the text list: the cap (proposed ADR-0059 d7).
pub const TIME_DEPTH: usize = FUSION_DEPTH;

/// A recall serving fewer hits than this, or than its `k` when `k` is
/// smaller, before the budget cut, is thin: only then does it offer
/// `did_you_mean` (ADR-0049 d6, d12).
pub const DID_YOU_MEAN_BELOW: usize = 3;

/// A served pipeline: a closed set, so the request log and the metrics name
/// a bounded set of values (ADR-0049 d4, ADR-0042 d11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pipeline {
    /// `keyword-all`: the v0.5.0 recall, every word, `ts_rank`; kept so the
    /// eval can compare against it.
    KeywordAll,
    /// `keyword-any`: any word on the same index (S1).
    KeywordAny,
    /// `bm25`: identifier parts, stems beside exact forms, BM25 over the
    /// readable rows (S2).
    Bm25,
    /// `bm25-trigram`: `bm25` fused with trigram similarity by reciprocal
    /// rank (S3).
    Bm25Trigram,
    /// `bm25-postings`: `bm25`'s ranking read from integer postings, with
    /// frequent words pruned and the scan as the named fallback (ADR-0052
    /// d9).
    Bm25Postings,
    /// `bm25-rerank`: `bm25`'s list, its head reordered by a local
    /// cross-encoder (ADR-0048 S6; ADR-0053 d1).
    Bm25Rerank,
    /// `sparse`: learned sparse postings alone (ADR-0052 d16).
    Sparse,
    /// `bm25-sparse`: `bm25-postings` and `sparse` fused by convex
    /// combination, 0.3 and 0.7 (ADR-0052 d13, d16).
    Bm25Sparse,
    /// `bm25-postings-time`: `bm25-postings` and the `time` leg fused by
    /// reciprocal rank (proposed ADR-0059 d7).
    Bm25PostingsTime,
}

impl Pipeline {
    /// Every pipeline this build knows, in the order the docs list them.
    pub const ALL: [Self; 9] = [
        Self::KeywordAll,
        Self::KeywordAny,
        Self::Bm25,
        Self::Bm25Trigram,
        Self::Bm25Postings,
        Self::Bm25Rerank,
        Self::Sparse,
        Self::Bm25Sparse,
        Self::Bm25PostingsTime,
    ];

    /// The pipeline's name, as configuration, requests and logs spell it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeywordAll => "keyword-all",
            Self::KeywordAny => "keyword-any",
            Self::Bm25 => "bm25",
            Self::Bm25Trigram => "bm25-trigram",
            Self::Bm25Postings => "bm25-postings",
            Self::Bm25Rerank => "bm25-rerank",
            Self::Sparse => "sparse",
            Self::Bm25Sparse => "bm25-sparse",
            Self::Bm25PostingsTime => "bm25-postings-time",
        }
    }

    /// The pipeline a name spells, or `None` for a name this build does not
    /// know.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == name)
    }

    /// The retrievers whose lists the pipeline fuses, in fusion order.
    #[must_use]
    pub const fn retrievers(self) -> &'static [RetrieverKind] {
        match self {
            Self::KeywordAll => &[RetrieverKind::KeywordAll],
            Self::KeywordAny => &[RetrieverKind::KeywordAny],
            Self::Bm25 | Self::Bm25Rerank => &[RetrieverKind::Bm25],
            Self::Bm25Trigram => &[RetrieverKind::Bm25, RetrieverKind::Trigram],
            Self::Bm25Postings => &[RetrieverKind::Bm25Postings],
            Self::Sparse => &[RetrieverKind::Sparse],
            Self::Bm25Sparse => &[RetrieverKind::Bm25Postings, RetrieverKind::Sparse],
            Self::Bm25PostingsTime => &[RetrieverKind::Bm25Postings, RetrieverKind::Time],
        }
    }

    /// How the pipeline's lists become one.
    #[must_use]
    pub const fn fusion(self) -> Fusion {
        match self {
            Self::KeywordAll
            | Self::KeywordAny
            | Self::Bm25
            | Self::Bm25Postings
            | Self::Bm25Rerank
            | Self::Sparse => Fusion::Single,
            Self::Bm25Trigram | Self::Bm25PostingsTime => Fusion::Rrf { k: RRF_K },
            Self::Bm25Sparse => Fusion::Cc {
                weights: &CC_BM25_SPARSE,
            },
        }
    }

    /// Whether the pipeline reorders its head with the deployment's
    /// reranker after fusion and before the cuts (ADR-0053 d1).
    #[must_use]
    pub const fn reranks(self) -> bool {
        matches!(self, Self::Bm25Rerank)
    }

    /// Whether the pipeline offers spellings for a thin result
    /// (`did_you_mean`): every pipeline but `keyword-all`, v0.5.0's recall,
    /// which offered none. The spellings come from the store's vocabulary,
    /// not from a retriever's list (ADR-0049 d6, d12).
    #[must_use]
    pub fn suggests(self) -> bool {
        self != Self::KeywordAll
    }

    /// Whether the pipeline reads learned sparse postings, so the
    /// composition must load the sparse encoder before it serves
    /// (ADR-0052 d16).
    #[must_use]
    pub fn needs_sparse(self) -> bool {
        self.retrievers().contains(&RetrieverKind::Sparse)
    }
}

impl fmt::Display for Pipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `match`: whether every query term must match (ADR-0049 d5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MatchMode {
    /// Any term; the default.
    #[default]
    Any,
    /// Every term; when no row has them all, the recall serves the any-term
    /// list and says so with `fallback`.
    All,
}

/// The deployment's search settings: the pipelines it serves and the one a
/// recall uses when neither the request nor the tenant names one
/// (configuration.md `search.*`; ADR-0049 d4).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchConfig {
    enabled: Vec<Pipeline>,
    default: Pipeline,
    rerank: RerankPolicy,
}

/// How a reranking pipeline uses the reranker (configuration.md
/// `search.rerank.*`; ADR-0053 d4, d6): how many of the first stage's
/// candidates it reorders, and the relevance below which a reranked
/// candidate is withheld.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RerankPolicy {
    /// The head of the first stage's list the reranker reorders.
    pub depth: usize,
    /// The relevance floor, a rerank score in `[0, 1]`; `None` serves every
    /// candidate.
    pub floor: Option<f32>,
}

/// `search.rerank.depth`'s default, chosen on the eval (ADR-0053 d4).
pub const RERANK_DEPTH: usize = 10;

/// The deepest head a deployment may rerank: `k`'s own ceiling.
pub const RERANK_DEPTH_MAX: usize = 100;

/// `search.rerank.floor`'s default, chosen on the eval (ADR-0053 d6).
pub const RERANK_FLOOR: Option<f32> = None;

impl Default for RerankPolicy {
    fn default() -> Self {
        Self {
            depth: RERANK_DEPTH,
            floor: RERANK_FLOOR,
        }
    }
}

impl RerankPolicy {
    /// A policy reordering `depth` candidates and withholding those under
    /// `floor`.
    ///
    /// # Errors
    ///
    /// [`SearchConfigError::Rerank`] for a depth outside `1..=100` or a
    /// floor outside `[0, 1]`.
    pub fn new(depth: usize, floor: Option<f32>) -> Result<Self, SearchConfigError> {
        if !(1..=RERANK_DEPTH_MAX).contains(&depth) {
            return Err(SearchConfigError::Rerank("depth must be between 1 and 100"));
        }
        if floor.is_some_and(|f| !(0.0..=1.0).contains(&f)) {
            return Err(SearchConfigError::Rerank(
                "floor must be between 0 and 1, or off",
            ));
        }
        Ok(Self { depth, floor })
    }
}

/// Why a search configuration is refused at start (ADR-0049 d4, d8).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SearchConfigError {
    #[error(
        "search.enabled names {0:?}, which this build does not serve (known: keyword-all, keyword-any, bm25, bm25-trigram, bm25-postings, bm25-rerank, sparse, bm25-sparse, bm25-postings-time)"
    )]
    Unknown(String),
    #[error("search.enabled is empty: a deployment serves at least one pipeline")]
    Empty,
    #[error("search.default {0} is not among search.enabled")]
    DefaultDisabled(Pipeline),
    #[error(
        "search.enabled names {pipeline}, whose {retriever} retriever this store does not serve{hint}"
    )]
    Unsupported {
        pipeline: Pipeline,
        retriever: &'static str,
        hint: &'static str,
    },
    #[error(
        "search.enabled names {0}, which reranks, and no reranker is loaded: set search.rerank.model_dir to a directory holding the model (configuration.md search.rerank)"
    )]
    NoReranker(Pipeline),
    #[error("search.rerank.{0}")]
    Rerank(&'static str),
    #[error(
        "search.enabled names {0}, which reads learned sparse postings, and no sparse encoder is loaded: set search.sparse.model_dir to a directory holding the model, or search.sparse.fetch = true (configuration.md search.sparse)"
    )]
    NoSparseEncoder(Pipeline),
}

impl SearchConfig {
    /// A configuration serving `enabled`, defaulting to `default`.
    ///
    /// # Errors
    ///
    /// [`SearchConfigError::Empty`] for an empty set and
    /// [`SearchConfigError::DefaultDisabled`] when `default` is outside it.
    pub fn new(enabled: &[Pipeline], default: Pipeline) -> Result<Self, SearchConfigError> {
        let mut set = enabled.to_vec();
        set.sort_unstable();
        set.dedup();
        if set.is_empty() {
            return Err(SearchConfigError::Empty);
        }
        if !set.contains(&default) {
            return Err(SearchConfigError::DefaultDisabled(default));
        }
        Ok(Self {
            enabled: set,
            default,
            rerank: RerankPolicy::default(),
        })
    }

    /// The configuration with `policy` for its reranking pipelines.
    #[must_use]
    pub fn with_rerank(self, policy: RerankPolicy) -> Self {
        Self {
            rerank: policy,
            ..self
        }
    }

    /// How the reranking pipelines use the reranker.
    #[must_use]
    pub fn rerank(&self) -> RerankPolicy {
        self.rerank
    }

    /// Whether any served pipeline reranks, so the composition must load a
    /// reranker before it serves.
    #[must_use]
    pub fn needs_reranker(&self) -> bool {
        self.enabled.iter().any(|p| p.reranks())
    }

    /// Refuses a configuration serving a reranking pipeline with no
    /// reranker loaded (ADR-0053 d8).
    ///
    /// # Errors
    ///
    /// [`SearchConfigError::NoReranker`] naming the first such pipeline.
    pub fn require_reranker(&self, loaded: bool) -> Result<(), SearchConfigError> {
        match self.enabled.iter().find(|p| p.reranks()) {
            Some(&pipeline) if !loaded => Err(SearchConfigError::NoReranker(pipeline)),
            _ => Ok(()),
        }
    }

    /// Whether any served pipeline reads learned sparse postings.
    #[must_use]
    pub fn needs_sparse(&self) -> bool {
        self.enabled.iter().any(|p| p.needs_sparse())
    }

    /// Refuses a configuration serving a sparse pipeline with no encoder
    /// loaded: it fails closed, never falling back to another pipeline
    /// (ADR-0052 d16).
    ///
    /// # Errors
    ///
    /// [`SearchConfigError::NoSparseEncoder`] naming the first such
    /// pipeline.
    pub fn require_sparse(&self, loaded: bool) -> Result<(), SearchConfigError> {
        match self.enabled.iter().find(|p| p.needs_sparse()) {
            Some(&pipeline) if !loaded => Err(SearchConfigError::NoSparseEncoder(pipeline)),
            _ => Ok(()),
        }
    }

    /// Parses `search.enabled` names.
    ///
    /// # Errors
    ///
    /// [`SearchConfigError::Unknown`] naming the first name this build does
    /// not know.
    pub fn parse_names(names: &[String]) -> Result<Vec<Pipeline>, SearchConfigError> {
        names
            .iter()
            .map(|n| Pipeline::parse(n.trim()).ok_or_else(|| SearchConfigError::Unknown(n.clone())))
            .collect()
    }

    /// The pipelines served, in [`Pipeline::ALL`] order.
    #[must_use]
    pub fn enabled(&self) -> &[Pipeline] {
        &self.enabled
    }

    /// The deployment's default.
    #[must_use]
    pub fn default_pipeline(&self) -> Pipeline {
        self.default
    }

    /// Whether `pipeline` is served here.
    #[must_use]
    pub fn serves(&self, pipeline: Pipeline) -> bool {
        self.enabled.contains(&pipeline)
    }

    /// Refuses a configuration naming a retriever the store lacks, so a
    /// deployment never starts promising a pipeline it cannot run.
    ///
    /// # Errors
    ///
    /// [`SearchConfigError::Unsupported`] naming the first pipeline and
    /// retriever missing.
    pub fn require_supported(&self, served: &[RetrieverKind]) -> Result<(), SearchConfigError> {
        for &pipeline in &self.enabled {
            if let Some(&missing) = pipeline.retrievers().iter().find(|r| !served.contains(r)) {
                return Err(SearchConfigError::Unsupported {
                    pipeline,
                    retriever: missing.as_str(),
                    hint: if missing == RetrieverKind::Trigram {
                        ": the pg_trgm extension is absent; CREATE EXTENSION pg_trgm as a role with CREATE on the database, then restart (configuration.md search.enabled)"
                    } else {
                        ""
                    },
                });
            }
        }
        Ok(())
    }
}

impl Default for SearchConfig {
    /// What a composition that names no search settings serves: the two
    /// keyword pipelines every store serves, `keyword-any` the default. The
    /// lead's own default is its configuration's (configuration.md).
    fn default() -> Self {
        Self {
            enabled: vec![Pipeline::KeywordAll, Pipeline::KeywordAny],
            default: Pipeline::KeywordAny,
            rerank: RerankPolicy::default(),
        }
    }
}

/// The rows a retriever may rank, computed by the core and never by an
/// adapter (ADR-0049 d2): the promoted served tips on `promoted_nodes` of
/// `kinds`, and, when `own_held` is set, the held rows those keys authored
/// on its nodes. Steering and the vocabulary kinds are never candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateScope {
    pub promoted_nodes: Vec<Uuid>,
    pub kinds: Vec<MemoryKind>,
    pub own_held: Option<OwnHeldScope>,
    /// The request's now in microseconds: a version whose valid time ended
    /// at or before it is outside the scope; `None` under
    /// `include_expired` (proposed ADR-0059 d3).
    pub expired_at: Option<i64>,
    /// The origin filters, a predicate of the scope itself, before any
    /// ranking or cut, so a filtered recall never under-returns (ADR-0057
    /// d4). Empty, they narrow nothing.
    pub anchor: AnchorFilter,
}

/// The own held part of a candidate scope (ADR-0025 d4, d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnHeldScope {
    pub author_key_ids: Vec<Uuid>,
    pub nodes: Vec<Uuid>,
}

/// The health of a tenant's integer postings, for doctor (ADR-0052 d12):
/// whether its partition exists; how many versions that should have
/// postings have none (the fill incomplete); how many served versions have
/// no served postings (behind); how many per-workarea counts disagree with
/// the served versions; and, of a sample of word counts, how many disagree
/// with the served postings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PostingsHealth {
    pub partition: bool,
    pub unfilled: u64,
    pub served_behind: u64,
    pub workarea_drift: u64,
    pub term_drift: u64,
    pub terms_checked: u64,
    /// Versions waiting for the sparse encoder, due now or later, parked
    /// ones aside (ADR-0052 d15).
    pub sparse_backlog: u64,
    /// Of them, the ones whose last encode or write failed.
    pub sparse_failing: u64,
    /// Versions parked after their attempts' ceiling: dead letters, kept
    /// with their reason and never retried until written again.
    pub sparse_parked: u64,
    /// Whether the tenant's sparse queue is on.
    pub sparse_enabled: bool,
}

/// The tenant's valid time for doctor (proposed ADR-0059 d12): its served
/// versions that declare a start, and those whose end has passed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidTimeCounts {
    pub declared: u64,
    pub expired: u64,
}

/// One retrieval: the query, whether every term must match, and how many
/// candidates to return at most.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetrievalQuery<'a> {
    pub query: &'a str,
    pub match_mode: MatchMode,
    pub k: usize,
    /// The query's learned sparse tokens, encoded by the core for a
    /// pipeline that reads them; empty otherwise (ADR-0052 d16).
    pub sparse: &'a [SparseTerm],
    /// The query's date window, which the `time` retriever reads; `None`
    /// when the query names none (proposed ADR-0059 d6, d7).
    pub window: Option<&'a Window>,
}

/// One query token of the learned sparse encoder: its vocabulary id, its
/// shipped idf weight and its word piece (ADR-0052 d14).
#[derive(Debug, Clone, PartialEq)]
pub struct SparseTerm {
    pub token_id: u32,
    pub weight: f32,
    pub piece: String,
}

/// One document posting the sparse encoder writes: its token, its weight,
/// and whether the version's own tokens hold it, so a match by the
/// encoder's expansion is named as one (ADR-0052 d14, d17).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SparsePosting {
    pub token_id: u32,
    pub weight: f32,
    pub in_text: bool,
}

/// A tenant the encoder's pass has work for: whether its sparse state is
/// to be turned on or its backfill continued (ADR-0052 d15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparseTenant {
    pub tenant_id: Uuid,
    pub backfill: bool,
}

/// A version waiting for the sparse encoder: its claim, and the
/// `enqueued_at` its write must match (ADR-0052 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparsePending {
    pub memory_id: Uuid,
    pub claim: String,
    pub enqueued_at_us: i64,
}

/// A query token that scored a hit, with its contribution and whether the
/// encoder added it to a text that does not hold it (`matched.tokens`;
/// ADR-0052 d17).
#[derive(Debug, Clone, PartialEq)]
pub struct MatchedToken {
    pub token: String,
    pub weight: f32,
    pub expanded: bool,
}

/// One retriever's ranked list: the query's terms as this retriever reads
/// them, its candidates in its own total order (score descending, then id
/// ascending), how it read them, and the query words whose every form it
/// pruned from the postings it read (ADR-0052 d6, d9; `[]` for a scan).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Retrieved {
    pub terms: Vec<String>,
    pub candidates: Vec<Candidate>,
    pub access: Access,
    pub pruned: Vec<String>,
}

/// One candidate as a retriever ranks it: no body, so a row the cuts drop is
/// never hydrated (ADR-0025 d7).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub memory_id: Uuid,
    pub body_bytes: u64,
    /// The retriever's score; comparable only within its own list.
    pub score: f32,
    /// The query terms the row holds, as the retriever reads terms.
    pub matched: Vec<String>,
    /// The learned sparse tokens that scored it, largest first; empty for
    /// every other retriever (ADR-0052 d17).
    pub tokens: Vec<MatchedToken>,
}

/// A fused candidate: its fused score, the terms it matched across the
/// lists, and which retrievers found it (`matched.via`; ADR-0049 d6).
#[derive(Debug, Clone, PartialEq)]
pub struct Fused {
    pub memory_id: Uuid,
    pub body_bytes: u64,
    pub score: f32,
    pub matched: Vec<String>,
    pub via: Vec<RetrieverKind>,
    /// `matched.tokens`: the sparse tokens that scored it (ADR-0052 d17).
    pub tokens: Vec<MatchedToken>,
    /// The reranker's relevance, for a candidate the reranker scored
    /// (`matched.rerank`; ADR-0053 d7).
    pub rerank: Option<f32>,
    /// Its rank in the first stage's list, from 1, for a candidate the
    /// reranker scored (`matched.rank_before`; ADR-0053 d7).
    pub rank_before: Option<u32>,
}

/// Caps each retriever's list at `depth`, keeping its first candidates in
/// the adapter's order, then fuses them (ADR-0049 d13; ADR-0057 d22): the
/// depth the core asked for is a cap it enforces, so no retriever, however
/// many rows it returns, contributes more candidates than any other.
#[must_use]
pub fn fuse_capped(
    mut lists: Vec<(RetrieverKind, Retrieved)>,
    fusion: Fusion,
    depth: usize,
) -> Vec<Fused> {
    for (_, list) in &mut lists {
        list.candidates.truncate(depth);
    }
    fuse(&lists, fusion)
}

/// Fuses each retriever's list under `fusion`, in the total order of hits
/// (score descending, then id ascending). Pure and deterministic: equal
/// input gives equal output.
#[must_use]
pub fn fuse(lists: &[(RetrieverKind, Retrieved)], fusion: Fusion) -> Vec<Fused> {
    let mut by_id: BTreeMap<Uuid, Fused> = BTreeMap::new();
    for (i, (kind, list)) in lists.iter().enumerate() {
        // Min-max over this list's own candidates alone, so no other list,
        // query or row outside the scope sets its range (ADR-0052 d13).
        let (lo, hi) = list
            .candidates
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), c| {
                (lo.min(c.score), hi.max(c.score))
            });
        for (rank, c) in list.candidates.iter().enumerate() {
            let contribution = match fusion {
                Fusion::Single => c.score,
                Fusion::Cc { weights } => {
                    let weight = weights.get(i).copied().unwrap_or(0.0);
                    let normalised = if hi > lo {
                        (c.score - lo) / (hi - lo)
                    } else {
                        1.0
                    };
                    weight * normalised
                }
                Fusion::Rrf { k } => {
                    let rank = u32::try_from(rank).unwrap_or(u32::MAX).saturating_add(1);
                    #[allow(clippy::cast_precision_loss, reason = "ranks are small")]
                    let denominator = k.saturating_add(rank) as f32;
                    1.0 / denominator
                }
            };
            let entry = by_id.entry(c.memory_id).or_insert_with(|| Fused {
                memory_id: c.memory_id,
                body_bytes: c.body_bytes,
                score: 0.0,
                matched: Vec::new(),
                via: Vec::new(),
                tokens: Vec::new(),
                rerank: None,
                rank_before: None,
            });
            entry.score += contribution;
            for term in &c.matched {
                if !entry.matched.contains(term) {
                    entry.matched.push(term.clone());
                }
            }
            if !entry.via.contains(kind) {
                entry.via.push(*kind);
            }
            entry.tokens.extend(c.tokens.iter().cloned());
        }
    }
    let mut fused: Vec<Fused> = by_id.into_values().collect();
    for f in &mut fused {
        f.matched.sort_unstable();
    }
    hit_order(&mut fused, |f| (f.score, f.memory_id));
    fused
}

/// Reorders `head`, the first stage's candidates in its order, by the
/// reranker's `scores`, one per candidate (ADR-0053 d1, d7). Each candidate's
/// `score` becomes its relevance, `rerank` holds it and `rank_before` its
/// first-stage rank; equal relevances keep the first stage's order, which is
/// itself total, so equal input gives equal output. Pure: it reads the
/// candidates and the scores and nothing else.
///
/// # Panics
///
/// Never: a score list of another length is the caller's refusal, checked
/// before (`RerankError::Failed`).
#[must_use]
pub fn rerank_order(head: Vec<Fused>, scores: &[f32]) -> Vec<Fused> {
    debug_assert_eq!(head.len(), scores.len());
    let mut ranked: Vec<(usize, Fused)> = head
        .into_iter()
        .zip(scores)
        .enumerate()
        .map(|(i, (mut f, &s))| {
            f.rerank = Some(s);
            f.score = s;
            f.rank_before = Some(u32::try_from(i + 1).unwrap_or(u32::MAX));
            (i, f)
        })
        .collect();
    ranked.sort_by(|(ia, a), (ib, b)| b.score.total_cmp(&a.score).then(ia.cmp(ib)));
    ranked.into_iter().map(|(_, f)| f).collect()
}

/// The relevance floor over the served head (ADR-0053 d6): the candidates
/// the reranker scored at or above `floor` stay, in order; every other
/// candidate, a reranked one below the floor or one the reranker never
/// scored, is withheld, and the count withheld is returned.
#[must_use]
pub fn apply_floor(candidates: Vec<Fused>, floor: f32) -> (Vec<Fused>, usize) {
    let before = candidates.len();
    let kept: Vec<Fused> = candidates
        .into_iter()
        .filter(|c| c.rerank.is_some_and(|r| r >= floor))
        .collect();
    let withheld = before - kept.len();
    (kept, withheld)
}

/// Keeps the `time` list's rows the text list holds, orders them by the
/// leg's score (the rows the window holds first, then those that began
/// before it, nearest first), then by the text list's rank, and cuts it at
/// [`TIME_DEPTH`] (proposed ADR-0059 d7): the leg lifts a row the words
/// found and never adds one they did not. Pure: it reads the two lists and
/// nothing else.
#[must_use]
pub fn order_time_list(time: Retrieved, text: &Retrieved) -> Retrieved {
    let rank_in_text: BTreeMap<Uuid, usize> = text
        .candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (c.memory_id, i))
        .collect();
    let mut ranked: Vec<(usize, usize, Candidate)> = time
        .candidates
        .into_iter()
        .enumerate()
        .filter_map(|(own, c)| {
            let text_rank = rank_in_text.get(&c.memory_id).copied()?;
            Some((text_rank, own, c))
        })
        .collect();
    // The rows the window holds first (the leg scores them 1), then the
    // ones that began before it, nearest first; within each, the text
    // list's order.
    ranked.sort_by(|(ta, oa, a), (tb, ob, b)| {
        b.score
            .total_cmp(&a.score)
            .then(ta.cmp(tb))
            .then(oa.cmp(ob))
    });
    ranked.truncate(TIME_DEPTH);
    Retrieved {
        candidates: ranked.into_iter().map(|(_, _, c)| c).collect(),
        ..time
    }
}

/// How a pipeline read its lists: `Postings` only when every text list was
/// read from postings, else `Scan` (ADR-0052 d11).
#[must_use]
pub fn pipeline_access(lists: &[(RetrieverKind, Retrieved)]) -> Access {
    // The `time` leg reads its own index and says nothing of how the words
    // were read (proposed ADR-0059 d7).
    let text: Vec<_> = lists
        .iter()
        .filter(|(kind, _)| *kind != RetrieverKind::Time)
        .collect();
    if !text.is_empty() && text.iter().all(|(_, l)| l.access == Access::Postings) {
        Access::Postings
    } else {
        Access::Scan
    }
}

/// The query words any list pruned, sorted and distinct (ADR-0052 d11).
#[must_use]
pub fn pruned_words(lists: &[(RetrieverKind, Retrieved)]) -> Vec<String> {
    let mut words: Vec<String> = lists
        .iter()
        .flat_map(|(_, l)| l.pruned.iter().cloned())
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

/// The query's distinct terms across the lists, sorted: `matched.of`'s
/// denominator.
#[must_use]
pub fn query_terms(lists: &[(RetrieverKind, Retrieved)]) -> Vec<String> {
    let mut terms: Vec<String> = lists
        .iter()
        .flat_map(|(_, l)| l.terms.iter().cloned())
        .collect();
    terms.sort_unstable();
    terms.dedup();
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: u128, score: f32, matched: &[&str]) -> Candidate {
        Candidate {
            memory_id: Uuid::from_u128(id),
            body_bytes: 10,
            score,
            matched: matched.iter().map(|s| (*s).to_owned()).collect(),
            tokens: Vec::new(),
        }
    }

    fn list(cands: Vec<Candidate>) -> Retrieved {
        Retrieved {
            terms: vec!["a".into(), "b".into()],
            candidates: cands,
            ..Retrieved::default()
        }
    }

    #[test]
    fn single_fusion_keeps_the_retrievers_order_and_scores() {
        let fused = fuse(
            &[(
                RetrieverKind::KeywordAny,
                list(vec![
                    cand(2, 0.5, &["a"]),
                    cand(1, 0.5, &["b"]),
                    cand(3, 0.9, &[]),
                ]),
            )],
            Fusion::Single,
        );
        let ids: Vec<u128> = fused.iter().map(|f| f.memory_id.as_u128()).collect();
        assert_eq!(ids, vec![3, 1, 2], "score descending, then id ascending");
        assert!((fused[0].score - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn reciprocal_rank_fusion_sums_one_over_k_plus_rank_and_unions_the_reasons() {
        let fused = fuse(
            &[
                (
                    RetrieverKind::Bm25,
                    list(vec![cand(1, 9.0, &["a"]), cand(2, 1.0, &["b"])]),
                ),
                (RetrieverKind::Trigram, list(vec![cand(2, 0.9, &["a"])])),
            ],
            Fusion::Rrf { k: 60 },
        );
        let ids: Vec<u128> = fused.iter().map(|f| f.memory_id.as_u128()).collect();
        assert_eq!(ids, vec![2, 1], "found by both lists outranks first in one");
        assert!((fused[0].score - (1.0 / 62.0 + 1.0 / 61.0)).abs() < 1e-6);
        assert_eq!(fused[0].matched, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(
            fused[0].via,
            vec![RetrieverKind::Bm25, RetrieverKind::Trigram]
        );
    }

    #[test]
    fn fusion_is_deterministic_for_equal_input() {
        let lists = [
            (
                RetrieverKind::Bm25,
                list(vec![cand(5, 1.0, &[]), cand(4, 1.0, &[])]),
            ),
            (
                RetrieverKind::Trigram,
                list(vec![cand(4, 1.0, &[]), cand(5, 1.0, &[])]),
            ),
        ];
        assert_eq!(
            fuse(&lists, Fusion::Rrf { k: 60 }),
            fuse(&lists, Fusion::Rrf { k: 60 })
        );
        let ids: Vec<u128> = fuse(&lists, Fusion::Rrf { k: 60 })
            .iter()
            .map(|f| f.memory_id.as_u128())
            .collect();
        assert_eq!(ids, vec![4, 5], "equal fused scores break ties by id");
    }

    #[test]
    fn cc_normalises_each_list_over_its_own_candidates() {
        // bm25 scores 10 / 6 / 2 normalise to 1 / 0.5 / 0; sparse 4 / 1 to
        // 1 / 0. Weights 0.3 and 0.7.
        let fused = fuse(
            &[
                (
                    RetrieverKind::Bm25Postings,
                    list(vec![
                        cand(1, 10.0, &["a"]),
                        cand(2, 6.0, &["b"]),
                        cand(3, 2.0, &[]),
                    ]),
                ),
                (
                    RetrieverKind::Sparse,
                    list(vec![cand(3, 4.0, &[]), cand(2, 1.0, &[])]),
                ),
            ],
            Fusion::Cc {
                weights: &CC_BM25_SPARSE,
            },
        );
        let got: Vec<(u128, f32)> = fused
            .iter()
            .map(|f| (f.memory_id.as_u128(), f.score))
            .collect();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].0, 3, "{got:?}");
        assert!((got[0].1 - 0.7).abs() < 1e-6, "{got:?}");
        assert_eq!(got[1].0, 1);
        assert!((got[1].1 - 0.3).abs() < 1e-6);
        assert_eq!(got[2].0, 2);
        assert!((got[2].1 - 0.15).abs() < 1e-6);
        assert_eq!(
            fused[2].via,
            vec![RetrieverKind::Bm25Postings, RetrieverKind::Sparse]
        );
        // Scaling one list's scores, as a statistic over other rows would,
        // changes nothing: only its own range normalises it.
        let scaled = fuse(
            &[
                (
                    RetrieverKind::Bm25Postings,
                    list(vec![
                        cand(1, 100.0, &["a"]),
                        cand(2, 60.0, &["b"]),
                        cand(3, 20.0, &[]),
                    ]),
                ),
                (
                    RetrieverKind::Sparse,
                    list(vec![cand(3, 4.0, &[]), cand(2, 1.0, &[])]),
                ),
            ],
            Fusion::Cc {
                weights: &CC_BM25_SPARSE,
            },
        );
        let order = |f: &[Fused]| f.iter().map(|x| x.memory_id).collect::<Vec<_>>();
        assert_eq!(order(&fused), order(&scaled));
    }

    #[test]
    fn cc_gives_a_flat_list_full_weight_and_a_missing_candidate_nothing() {
        let fused = fuse(
            &[
                (RetrieverKind::Bm25Postings, list(vec![cand(7, 3.0, &[])])),
                (
                    RetrieverKind::Sparse,
                    list(vec![cand(8, 2.0, &[]), cand(9, 2.0, &[])]),
                ),
            ],
            Fusion::Cc {
                weights: &CC_BM25_SPARSE,
            },
        );
        let got: Vec<(u128, f32)> = fused
            .iter()
            .map(|f| (f.memory_id.as_u128(), f.score))
            .collect();
        assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), vec![8, 9, 7]);
        assert!((got[0].1 - 0.7).abs() < 1e-6 && (got[2].1 - 0.3).abs() < 1e-6);
    }

    #[test]
    fn a_sparse_pipeline_with_no_encoder_loaded_is_refused_by_name() {
        let config = SearchConfig::new(
            &[Pipeline::Bm25Postings, Pipeline::Bm25Sparse],
            Pipeline::Bm25Postings,
        )
        .unwrap();
        assert!(config.needs_sparse());
        let err = config.require_sparse(false).unwrap_err();
        assert_eq!(
            err,
            SearchConfigError::NoSparseEncoder(Pipeline::Bm25Sparse)
        );
        assert!(err.to_string().contains("search.sparse.model_dir"), "{err}");
        assert_eq!(config.require_sparse(true), Ok(()));
        let plain = SearchConfig::new(&[Pipeline::Bm25Postings], Pipeline::Bm25Postings).unwrap();
        assert!(!plain.needs_sparse());
        assert_eq!(plain.require_sparse(false), Ok(()));
    }

    #[test]
    fn the_time_list_keeps_the_words_rows_the_window_holds_first_in_the_words_order() {
        let text = list(vec![
            cand(1, 9.0, &["a"]),
            cand(2, 8.0, &["a"]),
            cand(3, 7.0, &["a"]),
        ]);
        // The leg: 3 and 2 inside the window, 1 before it, 4 matched no word.
        let time = list(vec![
            cand(2, 1.0, &[]),
            cand(3, 1.0, &[]),
            cand(4, 1.0, &[]),
            cand(1, 0.5, &[]),
        ]);
        let ids: Vec<u128> = order_time_list(time, &text)
            .candidates
            .iter()
            .map(|c| c.memory_id.as_u128())
            .collect();
        assert_eq!(
            ids,
            vec![2, 3, 1],
            "inside first, in the words' order; no row the words missed"
        );
    }

    #[test]
    fn every_pipeline_name_round_trips_and_an_unknown_name_is_none() {
        for p in Pipeline::ALL {
            assert_eq!(Pipeline::parse(p.as_str()), Some(p));
        }
        assert_eq!(Pipeline::parse("hybrid"), None);
    }

    #[test]
    fn a_configuration_whose_default_is_not_enabled_is_refused() {
        assert_eq!(
            SearchConfig::new(&[Pipeline::KeywordAny], Pipeline::Bm25),
            Err(SearchConfigError::DefaultDisabled(Pipeline::Bm25))
        );
        assert_eq!(
            SearchConfig::new(&[], Pipeline::Bm25),
            Err(SearchConfigError::Empty)
        );
    }

    #[test]
    fn a_pipeline_whose_retriever_the_store_lacks_is_refused_by_name() {
        let config = SearchConfig::new(&[Pipeline::Bm25Trigram], Pipeline::Bm25Trigram).unwrap();
        let err = config
            .require_supported(&[RetrieverKind::Bm25])
            .unwrap_err();
        assert!(err.to_string().contains("pg_trgm"), "{err}");
    }

    fn fused(id: u128, score: f32) -> Fused {
        Fused {
            memory_id: Uuid::from_u128(id),
            body_bytes: 10,
            score,
            matched: vec!["a".to_owned()],
            via: vec![RetrieverKind::Bm25],
            tokens: Vec::new(),
            rerank: None,
            rank_before: None,
        }
    }

    #[test]
    fn rerank_order_sorts_by_relevance_and_keeps_the_first_stages_order_on_a_tie() {
        let head = vec![fused(9, 3.0), fused(1, 2.0), fused(5, 1.0)];
        let out = rerank_order(head.clone(), &[0.2, 0.7, 0.2]);
        let ids: Vec<u128> = out.iter().map(|f| f.memory_id.as_u128()).collect();
        assert_eq!(
            ids,
            vec![1, 9, 5],
            "relevance, then the first stage's rank, never the id"
        );
        assert_eq!(out[0].rank_before, Some(2));
        assert_eq!(out[0].rerank, Some(0.7));
        assert!((out[0].score - 0.7).abs() < f32::EPSILON);
        assert_eq!(out[0].matched, vec!["a".to_owned()], "the reasons stay");
        assert_eq!(
            rerank_order(head.clone(), &[0.2, 0.7, 0.2]),
            rerank_order(head, &[0.2, 0.7, 0.2]),
            "equal input, equal output"
        );
    }

    #[test]
    fn the_floor_keeps_reranked_candidates_at_or_above_it_and_withholds_the_rest() {
        let mut unscored = fused(4, 9.0);
        unscored.rerank = None;
        let mut list = rerank_order(
            vec![fused(1, 1.0), fused(2, 1.0), fused(3, 1.0)],
            &[0.5, 0.49, 0.9],
        );
        list.push(unscored);
        let (kept, withheld) = apply_floor(list, 0.5);
        let ids: Vec<u128> = kept.iter().map(|f| f.memory_id.as_u128()).collect();
        assert_eq!(
            ids,
            vec![3, 1],
            "at the floor stays; under it and unscored do not"
        );
        assert_eq!(withheld, 2);
    }

    #[test]
    fn a_rerank_policy_outside_its_bounds_is_refused() {
        assert!(RerankPolicy::new(0, None).is_err());
        assert!(RerankPolicy::new(101, None).is_err());
        assert!(RerankPolicy::new(20, Some(1.5)).is_err());
        assert!(RerankPolicy::new(20, Some(-0.1)).is_err());
        assert_eq!(
            RerankPolicy::new(20, Some(0.3)),
            Ok(RerankPolicy {
                depth: 20,
                floor: Some(0.3)
            })
        );
    }

    #[test]
    fn a_reranking_pipeline_with_no_reranker_loaded_is_refused_by_name() {
        let config =
            SearchConfig::new(&[Pipeline::Bm25, Pipeline::Bm25Rerank], Pipeline::Bm25).unwrap();
        assert!(config.needs_reranker());
        let err = config.require_reranker(false).unwrap_err();
        assert_eq!(err, SearchConfigError::NoReranker(Pipeline::Bm25Rerank));
        assert!(err.to_string().contains("search.rerank.model_dir"), "{err}");
        assert_eq!(config.require_reranker(true), Ok(()));
        let plain = SearchConfig::new(&[Pipeline::Bm25], Pipeline::Bm25).unwrap();
        assert_eq!(plain.require_reranker(false), Ok(()));
        assert!(Pipeline::Bm25Rerank.reranks() && !Pipeline::Bm25.reranks());
        assert_eq!(Pipeline::Bm25Rerank.retrievers(), &[RetrieverKind::Bm25]);
    }
}
