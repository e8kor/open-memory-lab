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
use oml_domain::budget::hit_order;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fusion {
    /// One list, served in its own order.
    Single,
    /// Reciprocal rank fusion with constant `k`: a candidate's score is the
    /// sum over the lists holding it of `1 / (k + rank)`, rank from 1.
    Rrf { k: u32 },
}

/// The fusion constant ADR-0048 d4 names, unmeasured on OML beyond the
/// recall eval.
pub const RRF_K: u32 = 60;

/// How deep each list of a fused pipeline is read before fusion: a list cut
/// at `k` would drop a candidate the other list ranks first.
pub const FUSION_DEPTH: usize = 50;

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
}

impl Pipeline {
    /// Every pipeline this build knows, in the order the docs list them.
    pub const ALL: [Self; 5] = [
        Self::KeywordAll,
        Self::KeywordAny,
        Self::Bm25,
        Self::Bm25Trigram,
        Self::Bm25Postings,
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
            Self::Bm25 => &[RetrieverKind::Bm25],
            Self::Bm25Trigram => &[RetrieverKind::Bm25, RetrieverKind::Trigram],
            Self::Bm25Postings => &[RetrieverKind::Bm25Postings],
        }
    }

    /// How the pipeline's lists become one.
    #[must_use]
    pub const fn fusion(self) -> Fusion {
        match self {
            Self::KeywordAll | Self::KeywordAny | Self::Bm25 | Self::Bm25Postings => Fusion::Single,
            Self::Bm25Trigram => Fusion::Rrf { k: RRF_K },
        }
    }

    /// Whether the pipeline offers spellings for a thin result
    /// (`did_you_mean`): every pipeline but `keyword-all`, v0.5.0's recall,
    /// which offered none. The spellings come from the store's vocabulary,
    /// not from a retriever's list (ADR-0049 d6, d12).
    #[must_use]
    pub fn suggests(self) -> bool {
        self != Self::KeywordAll
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchConfig {
    enabled: Vec<Pipeline>,
    default: Pipeline,
}

/// Why a search configuration is refused at start (ADR-0049 d4, d8).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SearchConfigError {
    #[error(
        "search.enabled names {0:?}, which this build does not serve (known: keyword-all, keyword-any, bm25, bm25-trigram, bm25-postings)"
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
        })
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
}

/// The own held part of a candidate scope (ADR-0025 d4, d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnHeldScope {
    pub author_key_ids: Vec<Uuid>,
    pub nodes: Vec<Uuid>,
}

/// One retrieval: the query, whether every term must match, and how many
/// candidates to return at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrievalQuery<'a> {
    pub query: &'a str,
    pub match_mode: MatchMode,
    pub k: usize,
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
}

/// Fuses each retriever's list under `fusion`, in the total order of hits
/// (score descending, then id ascending). Pure and deterministic: equal
/// input gives equal output.
#[must_use]
pub fn fuse(lists: &[(RetrieverKind, Retrieved)], fusion: Fusion) -> Vec<Fused> {
    let mut by_id: BTreeMap<Uuid, Fused> = BTreeMap::new();
    for (kind, list) in lists {
        for (rank, c) in list.candidates.iter().enumerate() {
            let contribution = match fusion {
                Fusion::Single => c.score,
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
        }
    }
    let mut fused: Vec<Fused> = by_id.into_values().collect();
    for f in &mut fused {
        f.matched.sort_unstable();
    }
    hit_order(&mut fused, |f| (f.score, f.memory_id));
    fused
}

/// How a pipeline read its lists: `Postings` only when every list was read
/// from postings, else `Scan` (ADR-0052 d11).
#[must_use]
pub fn pipeline_access(lists: &[(RetrieverKind, Retrieved)]) -> Access {
    if !lists.is_empty() && lists.iter().all(|(_, l)| l.access == Access::Postings) {
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
}
